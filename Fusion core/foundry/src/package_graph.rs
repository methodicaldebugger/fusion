//! Deterministic package dependency graph used by Foundry before compiler
//! module loading. This layer resolves package roots; it does not parse Fusion
//! modules or perform source-level name resolution.

use crate::{
    load_project, Dependency, Project,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use thiserror::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageNode {
    /// Canonical package directory. This is the stable graph identity.
    pub root: PathBuf,
    pub name: String,
    pub version: String,
    /// Canonical `src` directory, when present.
    pub source_root: PathBuf,
    /// Root application entry prefers `src/main.fusion`; dependency packages prefer `src/lib.fusion`.
    pub entry: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageEdge {
    pub from: PathBuf,
    /// Dependency key as written in the parent manifest.
    pub alias: String,
    pub to: PathBuf,
}

/// Stable, TOML-serializable representation of a resolved package graph.
/// Vectors are used instead of maps so canonical path keys are portable TOML.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageGraphSnapshot {
    pub root: PathBuf,
    pub nodes: Vec<PackageNode>,
    pub edges: Vec<PackageEdge>,
    pub build_order: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct PackageGraph {
    pub root: PathBuf,
    /// Nodes are ordered by canonical path for reproducible output.
    pub nodes: BTreeMap<PathBuf, PackageNode>,
    /// Edges are ordered by (parent, dependency alias).
    pub edges: Vec<PackageEdge>,
    /// Dependency-first order, excluding neither the root nor path packages.
    pub build_order: Vec<PathBuf>,
}

/// Result of comparing two resolved package graphs. `rebuild_order` is always
/// expressed in the current graph, while `removed_packages` records nodes that
/// existed only in the previous graph and therefore cannot appear in that order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageGraphDelta {
    pub changed_packages: BTreeSet<PathBuf>,
    pub removed_packages: BTreeSet<PathBuf>,
    pub rebuild_order: Vec<PathBuf>,
}

impl PackageGraph {
    pub fn snapshot(&self) -> PackageGraphSnapshot {
        PackageGraphSnapshot {
            root: self.root.clone(),
            nodes: self.nodes.values().cloned().collect(),
            edges: self.edges.clone(),
            build_order: self.build_order.clone(),
        }
    }

    pub fn from_snapshot(snapshot: PackageGraphSnapshot) -> Self {
        let nodes = snapshot.nodes.into_iter()
            .map(|node| (node.root.clone(), node))
            .collect();
        Self {
            root: snapshot.root,
            nodes,
            edges: snapshot.edges,
            build_order: snapshot.build_order,
        }
    }

    /// Compare this graph with a previously persisted graph snapshot.
    ///
    /// Added/removed packages invalidate themselves (when still present) and
    /// changes to dependency edges invalidate the consuming package. The
    /// normal reverse-dependency closure then propagates those changes to all
    /// transitive dependants. Removed packages are reported separately so a
    /// caller can discard their cached artifacts.
    pub fn delta_from(&self, previous: &PackageGraph) -> PackageGraphDelta {
        let mut changed = BTreeSet::new();
        let mut removed = BTreeSet::new();

        for (root, node) in &self.nodes {
            match previous.nodes.get(root) {
                None => { changed.insert(root.clone()); }
                Some(old) if old != node => { changed.insert(root.clone()); }
                Some(_) => {}
            }
        }
        for root in previous.nodes.keys() {
            if !self.nodes.contains_key(root) {
                removed.insert(root.clone());
            }
        }

        let edge_key = |edge: &PackageEdge| (edge.from.clone(), edge.alias.clone(), edge.to.clone());
        let current_edges = self.edges.iter().map(edge_key).collect::<BTreeSet<_>>();
        let previous_edges = previous.edges.iter().map(edge_key).collect::<BTreeSet<_>>();
        for (from, _, _) in current_edges.symmetric_difference(&previous_edges) {
            if self.nodes.contains_key(from) {
                changed.insert(from.clone());
            }
        }

        let rebuild_order = self.rebuild_order_for_changes(&changed);
        PackageGraphDelta {
            changed_packages: changed,
            removed_packages: removed,
            rebuild_order,
        }
    }

    /// Return every changed package and every transitive dependant that must be
    /// rebuilt because it consumes one of those packages.
    ///
    /// The result is keyed by canonical package roots, matching `nodes` and
    /// `edges`. Unknown roots are ignored; callers comparing old and new graphs
    /// must separately account for removed nodes and changed dependency edges.
    pub fn affected_packages(&self, changed: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
        let mut affected = changed
            .iter()
            .filter(|root| self.nodes.contains_key(*root))
            .cloned()
            .collect::<BTreeSet<_>>();

        loop {
            let previous_len = affected.len();
            for edge in &self.edges {
                if affected.contains(&edge.to) {
                    affected.insert(edge.from.clone());
                }
            }
            if affected.len() == previous_len {
                break;
            }
        }
        affected
    }

    /// Return the dependency-first subset of `build_order` affected by changes.
    pub fn rebuild_order_for_changes(&self, changed: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
        let affected = self.affected_packages(changed);
        self.build_order
            .iter()
            .filter(|root| affected.contains(*root))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum PackageGraphError {
    #[error("could not load package at '{path}': {message}")]
    Load { path: PathBuf, message: String },
    #[error("dependency '{dependency}' of package '{package}' is not available at '{path}'")]
    MissingDependency { package: String, dependency: String, path: PathBuf },
    #[error("package dependency cycle detected: {0}")]
    Cycle(String),
    #[error("package '{package}' has no source directory: expected '{path}'")]
    MissingSource { package: String, path: PathBuf },
}

fn package_source(node_root: &Path, package_name: &str, is_root: bool) -> Result<(PathBuf, Option<PathBuf>), PackageGraphError> {
    let source = node_root.join("src");
    if !source.is_dir() {
        return Err(PackageGraphError::MissingSource { package: package_name.to_owned(), path: source });
    }
    let source_root = source.canonicalize().map_err(|e| PackageGraphError::Load {
        path: source.clone(), message: e.to_string(),
    })?;
    let preferred = if is_root { ["main.fusion", "lib.fusion", "mod.fusion"] } else { ["lib.fusion", "main.fusion", "mod.fusion"] };
    let entry = if let Some(conventional) = preferred.iter().map(|name| source_root.join(name)).find(|path| path.is_file()) {
        Some(conventional.canonicalize().map_err(|e| PackageGraphError::Load { path: conventional, message: e.to_string() })?)
    } else {
        let mut candidates = Vec::new();
        collect_fusion(&source_root, &mut candidates);
        candidates.sort();
        candidates.into_iter().next()
    };
    Ok((source_root, entry))
}

fn collect_fusion(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fusion(&path, out);
        } else if path.extension().and_then(|v| v.to_str()) == Some("fusion") {
            out.push(path);
        }
    }
}

struct Builder {
    graph: PackageGraph,
    active: Vec<PathBuf>,
    visited: BTreeSet<PathBuf>,
}

impl Builder {
    fn visit(&mut self, root: &Path, supplied: Option<&Project>) -> Result<(), PackageGraphError> {
        let canonical = root.canonicalize().map_err(|e| PackageGraphError::Load { path: root.to_path_buf(), message: e.to_string() })?;
        if let Some(index) = self.active.iter().position(|p| p == &canonical) {
            let mut cycle: Vec<String> = self.active[index..].iter().map(|p| p.display().to_string()).collect();
            cycle.push(canonical.display().to_string());
            return Err(PackageGraphError::Cycle(cycle.join(" -> ")));
        }
        if self.visited.contains(&canonical) { return Ok(()); }

        let project = match supplied {
            Some(project) => project.clone(),
            None => load_project(&canonical).map_err(|message| PackageGraphError::Load { path: canonical.clone(), message })?,
        };
        let (source_root, entry) = package_source(&canonical, &project.manifest.package.name, supplied.is_some())?;
        let node = PackageNode {
            root: canonical.clone(),
            name: project.manifest.package.name.clone(),
            version: project.manifest.package.version.clone(),
            source_root,
            entry,
        };
        self.graph.nodes.insert(canonical.clone(), node);
        self.active.push(canonical.clone());

        for (alias, dependency) in &project.manifest.dependencies {
            let dependency_root = match dependency {
                Dependency::Path(path) => canonical.join(path),
                Dependency::Git { .. } | Dependency::Version(_) => canonical.join("target/foundry/deps").join(alias),
            };
            if !dependency_root.is_dir() {
                return Err(PackageGraphError::MissingDependency {
                    package: project.manifest.package.name.clone(),
                    dependency: alias.clone(),
                    path: dependency_root,
                });
            }
            let dependency_canonical = dependency_root.canonicalize().map_err(|e| PackageGraphError::Load {
                path: dependency_root.clone(), message: e.to_string(),
            })?;
            self.graph.edges.push(PackageEdge {
                from: canonical.clone(), alias: alias.clone(), to: dependency_canonical.clone(),
            });
            self.visit(&dependency_canonical, None)?;
        }
        self.active.pop();
        self.visited.insert(canonical.clone());
        self.graph.build_order.push(canonical);
        Ok(())
    }
}

/// Construct the package graph after dependency resolution has populated the
/// local cache. Registry and Git dependencies are read from `target/foundry/deps`.
pub fn resolve_package_graph(project: &Project) -> Result<PackageGraph, PackageGraphError> {
    let root = project.root.canonicalize().map_err(|e| PackageGraphError::Load {
        path: project.root.clone(), message: e.to_string(),
    })?;
    let mut builder = Builder {
        graph: PackageGraph { root: root.clone(), ..PackageGraph::default() },
        active: Vec::new(),
        visited: BTreeSet::new(),
    };
    builder.visit(&root, Some(project))?;
    builder.graph.edges.sort_by(|a, b| (&a.from, &a.alias).cmp(&(&b.from, &b.alias)));
    Ok(builder.graph)
}

#[cfg(test)]
mod tests {
    use super::{resolve_package_graph, PackageGraphError};
    use crate::load_project;
    use std::{fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let root = std::env::temp_dir().join(format!(
                "foundry-package-graph-{label}-{}-{nonce}", std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn package(&self, relative: &str, name: &str, dependencies: &[(&str, &str)]) -> PathBuf {
            let root = self.0.join(relative);
            fs::create_dir_all(root.join("src")).unwrap();
            let mut manifest = format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\n"
            );
            for (alias, path) in dependencies {
                manifest.push_str(&format!("{alias} = {{ path = \"{path}\" }}\n"));
            }
            fs::write(root.join("Foundry.toml"), manifest).unwrap();
            fs::write(root.join("src/main.fusion"), "main:\n    print(\"ok\")\n").unwrap();
            root
        }

        fn project(&self, root: &Path) -> crate::Project {
            load_project(root).unwrap()
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn diamond_graph_is_deduplicated_and_dependency_first() {
        let workspace = TestWorkspace::new("diamond");
        workspace.package("shared", "shared", &[]);
        workspace.package("left", "left", &[("shared", "../shared")]);
        workspace.package("right", "right", &[("shared", "../shared")]);
        let app = workspace.package(
            "app",
            "app",
            &[("left", "../left"), ("right", "../right")],
        );

        let graph = resolve_package_graph(&workspace.project(&app)).unwrap();
        assert_eq!(graph.nodes.len(), 4, "shared dependency must only be visited once");
        assert_eq!(graph.edges.len(), 4, "both edges to shared must be retained");

        let position = |relative: &str| {
            let canonical = workspace.0.join(relative).canonicalize().unwrap();
            graph.build_order.iter().position(|path| path == &canonical).unwrap()
        };
        assert!(position("shared") < position("left"));
        assert!(position("shared") < position("right"));
        assert!(position("left") < position("app"));
        assert!(position("right") < position("app"));

        let changed = [workspace.0.join("shared").canonicalize().unwrap()]
            .into_iter()
            .collect();
        let rebuild = graph.rebuild_order_for_changes(&changed);
        assert_eq!(rebuild, graph.build_order, "changing the shared leaf invalidates both branches and the root");
    }

    #[test]
    fn graph_delta_invalidates_consumers_when_a_dependency_is_removed() {
        let workspace = TestWorkspace::new("delta-removed");
        workspace.package("shared", "shared", &[]);
        workspace.package("left", "left", &[("shared", "../shared")]);
        workspace.package("right", "right", &[("shared", "../shared")]);
        let app = workspace.package(
            "app", "app", &[("left", "../left"), ("right", "../right")],
        );
        let previous = resolve_package_graph(&workspace.project(&app)).unwrap();
        let mut current = previous.clone();
        let removed = workspace.0.join("left").canonicalize().unwrap();
        let app_root = app.canonicalize().unwrap();

        current.nodes.remove(&removed);
        current.edges.retain(|edge| edge.from != removed && edge.to != removed);
        current.edges.retain(|edge| !(edge.from == app_root && edge.alias == "left"));
        current.build_order.retain(|root| root != &removed);

        let delta = current.delta_from(&previous);
        assert_eq!(delta.removed_packages, [removed.clone()].into_iter().collect());
        assert!(delta.changed_packages.contains(&app_root));
        assert_eq!(delta.rebuild_order, vec![app_root],
            "the consumer must rebuild even though the removed package is absent from the new graph");
    }

    #[test]
    fn root_and_dependency_entry_selection_matches_compiler_conventions() {
        let workspace = TestWorkspace::new("entry-selection");
        let dependency = workspace.package("dependency", "dependency", &[]);
        fs::write(dependency.join("src/lib.fusion"), "pub fn answer() -> num:\n    return 42\n").unwrap();
        let app = workspace.package("app", "app", &[("dependency", "../dependency")]);
        let graph = resolve_package_graph(&workspace.project(&app)).unwrap();
        let root_entry = graph.nodes.get(&graph.root).unwrap().entry.as_ref().unwrap();
        let dependency_root = dependency.canonicalize().unwrap();
        let dependency_entry = graph.nodes.get(&dependency_root).unwrap().entry.as_ref().unwrap();
        assert_eq!(root_entry.file_name().unwrap(), "main.fusion");
        assert_eq!(dependency_entry.file_name().unwrap(), "lib.fusion");
    }

    #[test]
    fn graph_delta_marks_added_packages_and_their_consumers() {
        let workspace = TestWorkspace::new("delta-added");
        workspace.package("shared", "shared", &[]);
        let app = workspace.package("app", "app", &[("shared", "../shared")]);
        let previous = resolve_package_graph(&workspace.project(&app)).unwrap();
        let mut current = previous.clone();
        let added_root = workspace.package("extra", "extra", &[]).canonicalize().unwrap();
        let source_root = added_root.join("src").canonicalize().unwrap();
        let entry = source_root.join("main.fusion");
        current.nodes.insert(added_root.clone(), super::PackageNode {
            root: added_root.clone(), name: "extra".into(), version: "0.1.0".into(),
            source_root, entry: Some(entry),
        });
        current.build_order.insert(0, added_root.clone());

        let delta = current.delta_from(&previous);
        assert!(delta.changed_packages.contains(&added_root));
        assert_eq!(delta.rebuild_order, vec![added_root]);
        assert!(delta.removed_packages.is_empty());
    }

    #[test]
    fn package_graph_snapshot_round_trips_without_losing_edges_or_order() {
        let workspace = TestWorkspace::new("snapshot");
        workspace.package("shared", "shared", &[]);
        let app = workspace.package("app", "app", &[("shared", "../shared")]);
        let graph = resolve_package_graph(&workspace.project(&app)).unwrap();
        let encoded = toml::to_string(&graph.snapshot()).unwrap();
        let decoded: super::PackageGraphSnapshot = toml::from_str(&encoded).unwrap();
        let restored = super::PackageGraph::from_snapshot(decoded);
        assert_eq!(restored.root, graph.root);
        assert_eq!(restored.nodes, graph.nodes);
        assert_eq!(restored.edges, graph.edges);
        assert_eq!(restored.build_order, graph.build_order);
    }

    #[test]
    fn package_graph_reports_a_readable_path_dependency_cycle() {
        let workspace = TestWorkspace::new("cycle");
        workspace.package("a", "a", &[("b", "../b")]);
        let b = workspace.package("b", "b", &[("a", "../a")]);
        let error = resolve_package_graph(&workspace.project(&workspace.0.join("a"))).unwrap_err();

        match error {
            PackageGraphError::Cycle(cycle) => {
                assert!(cycle.contains(&workspace.0.join("a").canonicalize().unwrap().display().to_string()));
                assert!(cycle.contains(&b.canonicalize().unwrap().display().to_string()));
            }
            other => panic!("expected dependency cycle, got {other}"),
        }
    }
}
