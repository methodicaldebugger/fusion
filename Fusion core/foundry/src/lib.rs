//contents of lib.rs

#[path = "../../linker_config.rs"]
mod linker_config;

use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const MANIFEST: &str = "Foundry.toml";
pub const LEGACY_MANIFEST: &str = "fusion.toml";
pub const LOCKFILE: &str = "foundry.lock";

mod fingerprint {
    use sha2::{Digest, Sha256};
    use std::{fs, io, path::Path};

    pub fn file_sha256(path: &Path) -> io::Result<String> {
        let bytes = fs::read(path)?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    pub fn fingerprint_fields<'a, I>(fields: I) -> String
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        let mut hasher = Sha256::new();
        for field in fields {
            hasher.update((field.len() as u64).to_le_bytes());
            hasher.update(field);
        }
        hex::encode(hasher.finalize())
    }
}
pub mod graph;
pub mod manifest;
pub mod package_graph;
pub mod plugin;
pub mod process;
pub mod template;
pub mod registry;
pub mod cache;
pub mod toolchain;
pub mod test_harness;

#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub package: Package,
    pub dependencies: BTreeMap<String, Dependency>,
    pub dev_dependencies: BTreeMap<String, Dependency>,
    pub plugins: BTreeMap<String, Dependency>,
    pub toolchain: Toolchain,
}
#[derive(Debug, Clone, Default)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub edition: String,
    pub description: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Dependency {
    Version(String),
    Path(PathBuf),
    Git { url: String, rev: Option<String> },
}
#[derive(Debug, Clone, Default)]
pub struct Toolchain {
    pub fusion: Option<String>,
    pub channel: Option<String>,
}
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
}
#[derive(Debug, Clone, Copy)]
pub enum BuildProfile {
    Debug,
    Release,
}
impl BuildProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

pub fn project_root(start: &Path) -> io::Result<PathBuf> {
    let mut p = if start.is_file() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if p.join(MANIFEST).is_file() || p.join(LEGACY_MANIFEST).is_file() {
            return Ok(p);
        }
        if !p.pop() {
            break;
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not find fusion.toml in this directory or a parent directory",
    ))
}

pub fn load_project(root: &Path) -> Result<Project, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("project root: {e}"))?;
    let manifest_path = if root.join(MANIFEST).is_file() { root.join(MANIFEST) } else { root.join(LEGACY_MANIFEST) };
    let text = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    Ok(Project {
        root,
        manifest: parse_manifest(&text)?,
    })
}

pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    // Parse the manifest as TOML instead of splitting lines manually. This correctly
    // handles escaped strings, comments, dotted keys, and inline tables.
    let value: toml::Value = toml::from_str(text)
        .map_err(|e| format!("invalid Foundry manifest: {e}"))?;
    let root = value.as_table().ok_or("manifest root must be a TOML table")?;
    let package = root
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or("manifest: [package] table is required")?;

    let get_string = |table: &toml::map::Map<String, toml::Value>, key: &str| -> Result<Option<String>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(v) => v.as_str().map(|s| Some(s.to_owned()))
                .ok_or_else(|| format!("manifest: '{key}' must be a string")),
        }
    };

    let name = get_string(package, "name")?
        .filter(|s| !s.trim().is_empty())
        .ok_or("manifest: [package].name is required and must not be empty")?;
    if !valid_manifest_name(&name) {
        return Err(format!("manifest: invalid package name '{name}' (use letters, digits, '_' or '-')"));
    }
    let mut m = Manifest::default();
    m.package.name = name;
    m.package.version = get_string(package, "version")?.unwrap_or_else(|| "0.1.0".into());
    m.package.edition = get_string(package, "edition")?.unwrap_or_else(|| "2021".into());
    m.package.description = get_string(package, "description")?;

    for section in ["dependencies", "dev-dependencies", "plugins"] {
        if let Some(table) = root.get(section) {
            let table = table.as_table().ok_or_else(|| format!("manifest: [{section}] must be a table"))?;
            for (dependency_name, spec) in table {
                if !valid_manifest_name(dependency_name) {
                    return Err(format!("manifest: invalid dependency name '{dependency_name}'"));
                }
                let parsed = parse_dependency_value(spec, dependency_name)?;
                match section {
                    "dependencies" => { m.dependencies.insert(dependency_name.clone(), parsed); }
                    "dev-dependencies" => { m.dev_dependencies.insert(dependency_name.clone(), parsed); }
                    "plugins" => { m.plugins.insert(dependency_name.clone(), parsed); }
                    _ => unreachable!(),
                }
            }
        }
    }

    validate_unique_dependency_aliases(&m)?;

    if let Some(toolchain) = root.get("toolchain") {
        let table = toolchain.as_table().ok_or("manifest: [toolchain] must be a table")?;
        m.toolchain.fusion = get_string(table, "fusion")?;
        m.toolchain.channel = get_string(table, "channel")?;
    }

    // Fail clearly on unsupported top-level sections rather than silently ignoring them.
    for key in root.keys() {
        if !matches!(key.as_str(), "package" | "dependencies" | "dev-dependencies" | "plugins" | "toolchain") {
            return Err(format!("manifest: unsupported top-level section or key '{key}'"));
        }
    }
    Ok(m)
}

fn valid_manifest_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn validate_unique_dependency_aliases(manifest: &Manifest) -> Result<(), String> {
    let mut aliases = std::collections::BTreeMap::<&str, &str>::new();
    for (section, dependencies) in [
        ("dependencies", &manifest.dependencies),
        ("dev-dependencies", &manifest.dev_dependencies),
        ("plugins", &manifest.plugins),
    ] {
        for name in dependencies.keys() {
            if let Some(previous) = aliases.insert(name.as_str(), section) {
                return Err(format!(
                    "manifest: dependency alias '{name}' is declared in both [{previous}] and [{section}]"
                ));
            }
        }
    }
    Ok(())
}

fn parse_dependency_value(value: &toml::Value, name: &str) -> Result<Dependency, String> {
    if let Some(version) = value.as_str() {
        if version.trim().is_empty() {
            return Err(format!("manifest: dependency '{name}' has an empty version requirement"));
        }
        return Ok(Dependency::Version(version.to_owned()));
    }
    let table = value.as_table().ok_or_else(|| format!("manifest: invalid dependency specification for '{name}'"))?;
    let version = table.get("version").and_then(toml::Value::as_str).map(str::to_owned);
    let path = table.get("path").and_then(toml::Value::as_str).map(PathBuf::from);
    let git = table.get("git").and_then(toml::Value::as_str).map(str::to_owned);
    let source_count = usize::from(version.is_some()) + usize::from(path.is_some()) + usize::from(git.is_some());
    if source_count != 1 {
        return Err(format!("manifest: dependency '{name}' must specify exactly one of version, path, or git"));
    }
    let rev = table.get("rev").and_then(toml::Value::as_str).map(str::to_owned);
    let tag = table.get("tag").and_then(toml::Value::as_str);
    let branch = table.get("branch").and_then(toml::Value::as_str);
    if (rev.is_some() || tag.is_some() || branch.is_some()) && git.is_none() {
        return Err(format!("manifest: dependency '{name}' uses rev/tag/branch without git"));
    }
    if usize::from(rev.is_some()) + usize::from(tag.is_some()) + usize::from(branch.is_some()) > 1 {
        return Err(format!("manifest: dependency '{name}' must not specify more than one of rev, tag, or branch"));
    }
    if let Some(path) = path {
        Ok(Dependency::Path(path))
    } else if let Some(git) = git {
        let selector = rev.or_else(|| tag.map(str::to_owned)).or_else(|| branch.map(str::to_owned));
        Ok(Dependency::Git { url: git, rev: selector })
    } else {
        Ok(Dependency::Version(version.unwrap_or_default()))
    }
}

fn strip_comment(s: &str) -> &str {
    let mut q = false;
    let mut esc = false;
    for (i, c) in s.char_indices() {
        if c == '"' && !esc {
            q = !q;
        }
        if c == '#' && !q {
            return &s[..i];
        }
        esc = c == '\\' && !esc;
        if c != '\\' {
            esc = false;
        }
    }
    s
}
fn parse_string(v: &str) -> Result<String, String> {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Ok(v[1..v.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"))
    } else {
        Err(format!("expected quoted string, got {v}"))
    }
}
fn parse_dependency(v: &str) -> Result<Dependency, String> {
    let v = v.trim();
    if v.starts_with('"') {
        return Ok(Dependency::Version(parse_string(v)?));
    }
    if v.starts_with('{') && v.ends_with('}') {
        let mut version = None;
        let mut path = None;
        let mut git = None;
        let mut rev = None;
        for part in v[1..v.len() - 1].split(',') {
            if let Some((k, val)) = part.split_once('=') {
                match k.trim() {
                    "version" => version = Some(parse_string(val)?),
                    "path" => path = Some(PathBuf::from(parse_string(val)?)),
                    "git" => git = Some(parse_string(val)?),
                    "rev" => rev = Some(parse_string(val)?),
                    _ => {}
                }
            }
        }
        if let Some(p) = path {
            return Ok(Dependency::Path(p));
        }
        if let Some(u) = git {
            return Ok(Dependency::Git { url: u, rev });
        }
        if let Some(v) = version {
            return Ok(Dependency::Version(v));
        }
    }
    Err(format!("invalid dependency specification: {v}"))
}

pub fn manifest_template(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n"
    )
}

fn directory_revision(root: &Path) -> String {
    fn walk(dir:&Path, out:&mut Vec<PathBuf>) {
        let Ok(entries)=fs::read_dir(dir) else{return};
        for entry in entries.flatten(){let path=entry.path();
            if path.file_name().map(|n| n == ".git" || n == "target" || n == OsStr::new(LOCKFILE)).unwrap_or(false) { continue; }
            if path.is_dir(){walk(&path,out)}else if path.is_file(){out.push(path)}
        }
    }
    let mut files=Vec::new();walk(root,&mut files);files.sort();
    let mut fields=Vec::<Vec<u8>>::new();
    for file in files { fields.push(file.strip_prefix(root).unwrap_or(&file).to_string_lossy().as_bytes().to_vec()); if let Ok(bytes)=fs::read(&file){fields.push(bytes)} }
    fingerprint::fingerprint_fields(fields.iter().map(|v|v.as_slice()))
}

fn resolve_git_for_lock(project: &Project, alias: &str, url: &str, selector: Option<&str>) -> Result<String, String> {
    let cache = project.root.join("target/foundry/deps");
    fs::create_dir_all(&cache).map_err(|error| format!("create Git dependency cache: {error}"))?;
    let destination = cache.join(alias);
    if !destination.join(".git").is_dir() {
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|error| format!("replace Git dependency cache for '{alias}': {error}"))?;
        }
        let mut clone = Command::new("git");
        clone.args(["clone", url]).arg(&destination);
        if !run(&mut clone, &project.root)? {
            return Err(format!("failed to clone Git dependency '{alias}'"));
        }
    }
    let mut fetch = Command::new("git");
    fetch.current_dir(&destination).args(["fetch", "--all", "--tags", "--force"]);
    if !run(&mut fetch, &destination)? {
        return Err(format!("failed to fetch Git dependency '{alias}' while generating foundry.lock"));
    }
    refresh_git_default_branch(&destination);
    let commit = git_resolve_commit(&destination, selector)?;
    let mut checkout = Command::new("git");
    checkout.current_dir(&destination).args(["checkout", "--detach", commit.as_str()]);
    if !run(&mut checkout, &destination)? {
        return Err(format!("failed to checkout commit {commit} for Git dependency '{alias}'"));
    }
    let actual = git_head(&destination)
        .ok_or_else(|| format!("could not verify Git commit for dependency '{alias}'"))?;
    if actual != commit {
        return Err(format!("Git dependency '{alias}' checked out {actual}, expected {commit}"));
    }
    Ok(commit)
}

pub fn write_lock(project: &Project) -> Result<(), String> {
    let manifest_path = if project.root.join(MANIFEST).is_file() { project.root.join(MANIFEST) } else { project.root.join(LEGACY_MANIFEST) };
    let manifest_bytes = fs::read(&manifest_path).map_err(|e| format!("read manifest for lockfile: {e}"))?;
    let manifest_hash = fingerprint::fingerprint_fields([manifest_bytes.as_slice()]);
    let mut out = format!("# This file is generated by Foundry. Do not edit by hand.\nversion = 3\nmanifest-sha256 = {:?}\n\n", manifest_hash);
    for (name, dep) in project.manifest.dependencies.iter().chain(project.manifest.dev_dependencies.iter()).chain(project.manifest.plugins.iter()) {
        let (requested, source, revision, checksum, registry_origin) = match dep {
            Dependency::Version(v) => (v.clone(), "registry".to_string(), String::new(), String::new(), env::var("FOUNDRY_REGISTRY").unwrap_or_default().trim_end_matches('/').to_owned()),
            Dependency::Path(p) => {
                let full = project.root.join(p);
                let canonical = full.canonicalize().unwrap_or(full);
                let rev = if canonical.exists(){directory_revision(&canonical)}else{"missing-path".into()};
                ("path".into(), format!("path+{}", canonical.display()), rev, String::new(), String::new())
            },
            Dependency::Git { url, rev } => {
                let actual = resolve_git_for_lock(project, name, url, rev.as_deref())?;
                (rev.clone().unwrap_or_else(||"*".into()), format!("git+{url}"), actual, String::new(), String::new())
            }
        };
        let lock_version = if matches!(dep, Dependency::Version(_)) { "unresolved" } else { requested.as_str() };
        out.push_str(&format!("[[package]]\nname = {:?}\nrequested = {:?}\nversion = {:?}\nsource = {:?}\nrevision = {:?}\nchecksum = {:?}\nregistry = {:?}\n\n", name, requested, lock_version, source, revision, checksum, registry_origin));
    }
    write_text_atomically(&project.root.join(LOCKFILE), &out).map_err(|e| format!("write lockfile atomically: {e}"))
}

fn write_resolved_lock(project: &Project, resolved: &[registry::ResolvedPackage], include_auxiliary: bool) -> Result<(), String> {
    let manifest_path = if project.root.join(MANIFEST).is_file() { project.root.join(MANIFEST) } else { project.root.join(LEGACY_MANIFEST) };
    let bytes = fs::read(&manifest_path).map_err(|e| format!("read manifest for lockfile: {e}"))?;
    let hash = fingerprint::fingerprint_fields([bytes.as_slice()]);
    let mut out = format!("# This file is generated by Foundry. Do not edit by hand.\nversion = 3\nmanifest-sha256 = {:?}\n\n", hash);
    for r in resolved { out.push_str(&format!("[[package]]\nname = {:?}\nrequested = {:?}\nversion = {:?}\nsource = {:?}\nrevision = {:?}\nchecksum = {:?}\nregistry = {:?}\n\n",r.name,r.requested,r.version,"registry","",r.checksum.as_deref().unwrap_or(""),r.registry_url.trim_end_matches('/'))); }
    // Preserve path and Git records alongside registry resolutions. Dependency
    // packages only lock runtime dependencies; dev-dependencies and plugins
    // belong to the root package's own development workflow.
    let lock_dependencies = if include_auxiliary {
        project.manifest.dependencies.iter()
            .chain(project.manifest.dev_dependencies.iter())
            .chain(project.manifest.plugins.iter())
            .collect::<Vec<_>>()
    } else {
        project.manifest.dependencies.iter().collect::<Vec<_>>()
    };
    for (name, dep) in lock_dependencies {
        if resolved.iter().any(|r|r.name==*name) { continue; }
        let (requested,source,revision,registry_origin)=match dep {
            Dependency::Version(v)=>(v.clone(),"registry".to_string(),"unresolved".to_string(),env::var("FOUNDRY_REGISTRY").unwrap_or_default().trim_end_matches('/').to_owned()),
            Dependency::Path(path)=>{let full=project.root.join(path).canonicalize().unwrap_or(project.root.join(path));let rev=if full.exists(){directory_revision(&full)}else{"missing-path".into()};("path".into(),format!("path+{}",full.display()),rev,String::new())},
            Dependency::Git{url,rev}=>{let dst=project.root.join("target/foundry/deps").join(name);let actual=if dst.join(".git").exists(){Command::new("git").current_dir(&dst).args(["rev-parse","HEAD"]).output().ok().filter(|o|o.status.success()).map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(||rev.clone().unwrap_or_else(||"unresolved".into()))}else{rev.clone().unwrap_or_else(||"unresolved".into())};(rev.clone().unwrap_or_else(||"*".into()),format!("git+{url}"),actual,String::new())}
        };
        out.push_str(&format!("[[package]]\nname = {:?}\nrequested = {:?}\nversion = {:?}\nsource = {:?}\nrevision = {:?}\nchecksum = {:?}\nregistry = {:?}\n\n",name,requested,requested,source,revision,String::new(),registry_origin));
    }
    write_text_atomically(&project.root.join(LOCKFILE), &out).map_err(|e|format!("write lockfile atomically: {e}"))
}

pub fn ensure_layout(project: &Project) -> Result<(), String> {
    for d in ["src", "tests", "foreign", "target"] {
        fs::create_dir_all(project.root.join(d)).map_err(|e| format!("create {d}: {e}"))?;
    }
    let main = project.root.join("src/main.fusion");
    if !main.exists() {
        fs::write(main, "main:\n    print(\"Hello from Fusion!\")\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn resolve_dependencies(project: &Project, offline: bool) -> Result<(), String> {
    resolve_dependencies_with_update(project, offline, false)
}

/// Resolve runtime dependencies for the complete package tree. The root
/// package also resolves its dev-dependencies and plugins; dependencies of
/// dependencies remain runtime-only.
pub fn resolve_dependencies_with_update(project: &Project, offline: bool, update: bool) -> Result<(), String> {
    if update && offline {
        return Err("foundry update requires online dependency metadata; remove --offline and retry".into());
    }
    let mut active = Vec::new();
    let mut visited = std::collections::BTreeSet::new();
    resolve_dependency_tree(project, offline, update, true, &mut active, &mut visited)?;
    validate_workspace_registry_pins(project)
}

fn validate_workspace_registry_pin_group(
    name: &str,
    entries: &[(String, String, String)],
) -> Result<(), String> {
    let versions = entries.iter().map(|entry| entry.2.as_str()).collect::<std::collections::BTreeSet<_>>();
    if versions.len() > 1 {
        let detail = entries.iter().map(|(requester, requirement, version)| {
            format!("{requester} requires {requirement} (locked {version})")
        }).collect::<Vec<_>>().join("; ");
        return Err(format!(
            "workspace dependency version conflict for '{name}': packages have different locked versions ({detail}); run 'foundry update' after aligning constraints"
        ));
    }
    if let Some((_, selected, _)) = entries.first() {
        if entries.iter().any(|(_, requirement, version)| {
            version != selected || !registry::satisfies(selected, requirement)
        }) {
            let detail = entries.iter().map(|(requester, requirement, version)| {
                format!("{requester} requires {requirement} (locked {version})")
            }).collect::<Vec<_>>().join("; ");
            return Err(format!(
                "no single locked version of workspace dependency '{name}' satisfies all packages ({detail})"
            ));
        }
    }
    Ok(())
}

/// Validate registry pins across the fully materialized runtime dependency graph.
/// Individual package lockfiles remain local to their manifests, but a single
/// workspace build must not silently load two versions of the same registry
/// package under the same dependency identity.
fn validate_workspace_registry_pins(project: &Project) -> Result<(), String> {
    let graph = package_graph::resolve_package_graph(project)
        .map_err(|error| format!("build dependency graph for workspace lock validation: {error}"))?;
    let mut requirements: BTreeMap<(String, String), Vec<(String, String, String)>> = BTreeMap::new();

    for node in graph.nodes.values() {
        let package = load_project(&node.root)?;
        let manifest_path = if node.root.join(MANIFEST).is_file() {
            node.root.join(MANIFEST)
        } else {
            node.root.join(LEGACY_MANIFEST)
        };
        let manifest_bytes = fs::read(&manifest_path)
            .map_err(|error| format!("read manifest {}: {error}", manifest_path.display()))?;
        let manifest_hash = fingerprint::fingerprint_fields([manifest_bytes.as_slice()]);
        let lock_path = node.root.join(LOCKFILE);
        let locked = read_validated_lock(&lock_path, &manifest_hash)?;

        for (alias, dependency) in &package.manifest.dependencies {
            let Dependency::Version(requirement) = dependency else { continue };
            let entry = locked.get(alias).ok_or_else(|| format!(
                "workspace lockfile for package '{}' has no registry pin for dependency '{alias}'",
                package.manifest.package.name
            ))?;
            if entry.source != "registry" || entry.registry.trim().is_empty() || !registry::is_exact_version(&entry.version) {
                return Err(format!(
                    "workspace lockfile entry '{alias}' in package '{}' is not an exact registry pin; run 'foundry update'",
                    package.manifest.package.name
                ));
            }
            if !registry::satisfies(&entry.version, requirement) {
                return Err(format!(
                    "locked version {} of '{alias}' in package '{}' does not satisfy '{}'; run 'foundry update'",
                    entry.version, package.manifest.package.name, requirement
                ));
            }
            requirements.entry((entry.registry.clone(), alias.clone())).or_default().push((
                package.manifest.package.name.clone(),
                requirement.clone(),
                entry.version.clone(),
            ));
        }
    }

    for ((registry_origin, name), entries) in requirements {
        validate_workspace_registry_pin_group(&format!("{name} from {registry_origin}"), &entries)?;
    }
    Ok(())
}

fn resolve_dependency_tree(
    project: &Project,
    offline: bool,
    update: bool,
    include_auxiliary: bool,
    active: &mut Vec<PathBuf>,
    visited: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<(), String> {
    let root = project.root.canonicalize().map_err(|e| format!("canonicalize package {}: {e}", project.root.display()))?;
    if let Some(index) = active.iter().position(|path| path == &root) {
        let mut cycle = active[index..].iter().map(|path| path.display().to_string()).collect::<Vec<_>>();
        cycle.push(root.display().to_string());
        return Err(format!("package dependency cycle detected: {}", cycle.join(" -> ")));
    }
    if visited.contains(&root) { return Ok(()); }

    active.push(root.clone());
    resolve_dependencies_one(project, offline, update, include_auxiliary)?;

    for (alias, dependency) in &project.manifest.dependencies {
        let dependency_root = match dependency {
            Dependency::Path(path) => root.join(path),
            Dependency::Git { .. } | Dependency::Version(_) => root.join("target/foundry/deps").join(alias),
        };
        if !dependency_root.is_dir() {
            return Err(format!("dependency '{alias}' of package '{}' was not materialized at {}", project.manifest.package.name, dependency_root.display()));
        }
        let dependency_project = load_project(&dependency_root)?;
        resolve_dependency_tree(&dependency_project, offline, update, false, active, visited)?;
    }

    active.pop();
    visited.insert(root);
    Ok(())
}

#[derive(Debug, Clone)]
struct LockedPackage {
    requested: String,
    version: String,
    source: String,
    revision: String,
    checksum: String,
    registry: String,
}

fn read_validated_lock(path: &Path, manifest_hash: &str) -> Result<BTreeMap<String, LockedPackage>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read lockfile {}: {e}", path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|e| format!("invalid lockfile {}: {e}; run 'foundry update' to regenerate it", path.display()))?;
    if value.get("version").and_then(toml::Value::as_integer) != Some(3) {
        return Err(format!("unsupported lockfile format in {}; run 'foundry update'", path.display()));
    }
    let stored_manifest_hash = value.get("manifest-sha256")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("lockfile {} is missing a string 'manifest-sha256'", path.display()))?;
    if !is_sha256(stored_manifest_hash) {
        return Err(format!("lockfile {} has an invalid manifest SHA-256", path.display()));
    }
    if stored_manifest_hash != manifest_hash {
        // Do not silently resolve a different graph during a normal build. The
        // explicit update command is the only operation that may replace pins.
        return Err(format!("manifest changed since {}; run 'foundry update' to refresh the dependency lockfile", path.display()));
    }
    // A dependency-free project has a valid lockfile containing only the
    // header. Treat an absent `package` array as an empty dependency set, but
    // reject a present value of the wrong TOML type.
    let empty_packages = Vec::new();
    let packages = match value.get("package") {
        None => &empty_packages,
        Some(value) => value.as_array().ok_or_else(|| {
            format!("lockfile {} has a non-array 'package' value", path.display())
        })?,
    };
    let mut entries = BTreeMap::new();
    for package in packages {
        let name = lock_string(package, "name", path)?.to_owned();
        if name.trim().is_empty() {
            return Err(format!("lockfile {} contains an empty package alias", path.display()));
        }
        let entry = LockedPackage {
            requested: lock_string(package, "requested", path)?.to_owned(),
            version: lock_string(package, "version", path)?.to_owned(),
            source: lock_string(package, "source", path)?.to_owned(),
            revision: lock_string(package, "revision", path)?.to_owned(),
            checksum: lock_string(package, "checksum", path)?.to_owned(),
            registry: lock_string(package, "registry", path)?.to_owned(),
        };
        validate_locked_package(&name, &entry, path)?;
        if entries.insert(name.clone(), entry).is_some() {
            return Err(format!("lockfile {} contains duplicate package alias '{name}'", path.display()));
        }
    }
    Ok(entries)
}

fn validate_lock_aliases(
    locked: &BTreeMap<String, LockedPackage>,
    manifest: &Manifest,
    lock_path: &Path,
) -> Result<(), String> {
    // Accept known dev/plugin aliases when reading an older dependency lockfile
    // so it can be rewritten without them; only aliases absent from every
    // manifest dependency section are stale.
    let declared_aliases = manifest.dependencies.keys()
        .chain(manifest.dev_dependencies.keys())
        .chain(manifest.plugins.keys())
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(unused) = locked.keys().find(|name| !declared_aliases.contains(name.as_str())) {
        return Err(format!("lockfile {} contains stale dependency alias '{unused}' that is not declared by this package; run 'foundry update'", lock_path.display()));
    }

    // A valid alias is not sufficient: its locked source and selector must
    // still describe the dependency declared under that alias.
    for (alias, entry) in locked {
        let dependency = manifest.dependencies.get(alias)
            .or_else(|| manifest.dev_dependencies.get(alias))
            .or_else(|| manifest.plugins.get(alias))
            .expect("declared alias checked above");
        let matches = match dependency {
            Dependency::Version(requirement) => {
                entry.source == "registry" && entry.requested == *requirement
            }
            Dependency::Git { url, rev } => {
                entry.source == format!("git+{url}")
                    && entry.requested == rev.as_deref().unwrap_or("*")
                    && is_git_commit(&entry.revision)
            }
            Dependency::Path(path) => {
                let resolved = lock_path.parent().unwrap_or_else(|| Path::new(".")).join(path);
                let canonical = resolved.canonicalize().unwrap_or(resolved);
                entry.source == format!("path+{}", canonical.display())
                    && entry.requested == "path"
            }
        };
        if !matches {
            return Err(format!(
                "lockfile entry '{alias}' does not match its dependency alias/source/selector in the manifest; run 'foundry update'"
            ));
        }
    }
    Ok(())
}

fn validate_registry_origin(name: &str, entry: &LockedPackage, registry_origin: &str) -> Result<(), String> {
    if entry.source != "registry" || entry.registry != registry_origin {
        return Err(format!("lockfile registry origin for dependency '{name}' does not match FOUNDRY_REGISTRY; run 'foundry update'"));
    }
    Ok(())
}

fn exact_registry_pin<'a>(
    name: &str,
    entry: Option<&'a LockedPackage>,
    offline: bool,
) -> Result<Option<&'a LockedPackage>, String> {
    let pinned = entry.filter(|entry| registry::is_exact_version(&entry.version));
    if offline && pinned.is_none() {
        return Err(format!(
            "registry dependency '{name}' has no exact locked version and checksum; run 'foundry update' while online"
        ));
    }
    Ok(pinned)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_locked_package(name: &str, entry: &LockedPackage, path: &Path) -> Result<(), String> {
    if entry.requested.trim().is_empty() || entry.source.trim().is_empty() {
        return Err(format!("lockfile {} has an incomplete package entry for '{name}'", path.display()));
    }

    match entry.source.as_str() {
        "registry" => {
            if entry.version != "unresolved" && entry.registry.trim().is_empty() {
                return Err(format!("lockfile {} registry entry '{name}' is missing its registry origin", path.display()));
            }
            if entry.registry.contains('\n') || entry.registry.contains('\r') {
                return Err(format!("lockfile {} registry entry '{name}' has an invalid registry origin", path.display()));
            }
            if !entry.revision.is_empty() {
                return Err(format!("lockfile {} registry entry '{name}' must not have a Git revision", path.display()));
            }
            if entry.version != "unresolved"
                && (!registry::is_exact_version(&entry.version) || !is_sha256(&entry.checksum))
            {
                return Err(format!("lockfile {} registry entry '{name}' must pin an exact version and SHA-256 checksum", path.display()));
            }
            if entry.version == "unresolved" && !entry.checksum.is_empty() {
                return Err(format!("lockfile {} provisional registry entry '{name}' must not contain a checksum", path.display()));
            }
        }
        source if source.starts_with("git+") => {
            if !entry.registry.is_empty() {
                return Err(format!("lockfile {} Git entry '{name}' must not specify a registry origin", path.display()));
            }
            if !entry.checksum.is_empty() {
                return Err(format!("lockfile {} Git entry '{name}' must not contain a registry checksum", path.display()));
            }
            if !is_git_commit(&entry.revision) {
                return Err(format!("lockfile {} Git entry '{name}' must pin a full immutable commit ID", path.display()));
            }
            if entry.version != entry.requested {
                return Err(format!("lockfile {} Git entry '{name}' has inconsistent selector metadata", path.display()));
            }
        }
        source if source.starts_with("path+") => {
            if !entry.registry.is_empty() {
                return Err(format!("lockfile {} path entry '{name}' must not specify a registry origin", path.display()));
            }
            if !entry.checksum.is_empty()
                || !(entry.revision == "missing-path" || is_sha256(&entry.revision))
            {
                return Err(format!("lockfile {} path entry '{name}' has an invalid source fingerprint", path.display()));
            }
        }
        _ => return Err(format!("lockfile {} package '{name}' has unsupported source '{}'; run 'foundry update'", path.display(), entry.source)),
    }
    Ok(())
}

fn lock_string<'a>(package: &'a toml::Value, key: &str, path: &Path) -> Result<&'a str, String> {
    package.get(key).and_then(toml::Value::as_str)
        .ok_or_else(|| format!("lockfile {} has a package entry missing '{key}'", path.display()))
}

fn is_git_commit(value: &str) -> bool {
    // Git supports both SHA-1 and SHA-256 object formats.
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_output(repository: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").current_dir(repository).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_head(repository: &Path) -> Option<String> {
    let revision = git_output(repository, &["rev-parse", "HEAD"])?.to_ascii_lowercase();
    is_git_commit(&revision).then_some(revision)
}

fn refresh_git_default_branch(repository: &Path) {
    // `git fetch` updates branch refs but does not necessarily refresh the
    // symbolic origin/HEAD when a remote changes its default branch. If the
    // remote no longer advertises a default, discard the old symbolic ref
    // rather than silently pinning a branch that is no longer its default.
    let refreshed = Command::new("git")
        .current_dir(repository)
        .args(["remote", "set-head", "origin", "--auto"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !refreshed {
        let _ = Command::new("git")
            .current_dir(repository)
            .args(["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"])
            .output();
    }
}

fn git_resolve_commit(repository: &Path, selector: Option<&str>) -> Result<String, String> {
    let resolve = |reference: &str| -> Option<String> {
        let expression = format!("{reference}^{{commit}}");
        let revision = git_output(repository, &[
            "rev-parse", "--verify", "--quiet", "--end-of-options", expression.as_str(),
        ])?.to_ascii_lowercase();
        is_git_commit(&revision).then_some(revision)
    };

    let revision = if let Some(selector) = selector {
        // Preserve Git's normal selector precedence first, then explicitly
        // support remote-only branches and tags that are not local short refs.
        resolve(selector)
            .or_else(|| resolve(&format!("refs/remotes/origin/{selector}")))
            .or_else(|| resolve(&format!("refs/heads/{selector}")))
            .or_else(|| resolve(&format!("refs/tags/{selector}")))
            .ok_or_else(|| format!("Git selector '{selector}' does not resolve to a commit in {}", repository.display()))?
    } else {
        let default_ref = git_output(repository, &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]);
        default_ref.as_deref().and_then(|reference| resolve(reference))
            .ok_or_else(|| format!("could not resolve the advertised default branch of Git dependency in {}; specify rev, tag, or branch", repository.display()))?
    };

    Ok(revision)
}

/// Remove registry cache entries not referenced by this project's validated lockfile.
/// The registry cache is project-local (`target/foundry/deps`), so another
/// workspace's lockfile and cached versions are never affected.
/// Read-only registry cache diagnostics for the current workspace.
pub fn inspect_registry_cache(project: &Project) -> Result<Vec<registry::CachePackageInspection>, String> {
    registry::inspect_cache(&project.root.join("target/foundry/deps"))
}

pub fn prune_registry_cache(project: &Project) -> Result<usize, String> {
    let cache = project.root.join("target/foundry/deps");
    if !cache.is_dir() { return Ok(0); }
    let _workspace_lock = registry::lock_workspace_cache(&cache)?;
    let manifest_path = if project.root.join(MANIFEST).is_file() { project.root.join(MANIFEST) } else { project.root.join(LEGACY_MANIFEST) };
    let manifest_bytes = fs::read(&manifest_path).map_err(|error| format!("read manifest for cache pruning: {error}"))?;
    let manifest_hash = fingerprint::fingerprint_fields([manifest_bytes.as_slice()]);
    let lock_path = project.root.join(LOCKFILE);
    if !lock_path.is_file() {
        return Err("cannot prune registry cache without foundry.lock; run 'foundry fetch' first".into());
    }
    let locked = read_validated_lock(&lock_path, &manifest_hash)?;
    let retained = locked.iter().filter_map(|(name, entry)| {
        (entry.source == "registry" && entry.version != "unresolved")
            .then(|| (name.clone(), entry.version.clone()))
    }).collect::<std::collections::BTreeSet<_>>();
    let mut candidates = Vec::<(String, String)>::new();
    for entry in fs::read_dir(&cache).map_err(|error| format!("read registry cache: {error}"))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else { continue };
        if let Some(stem) = file_name.strip_prefix("registry-meta-").and_then(|value| value.strip_suffix(".toml")) {
            let metadata = fs::read_to_string(entry.path()).ok().and_then(|text| toml::from_str::<toml::Value>(&text).ok());
            let identity = metadata.as_ref().and_then(|value| {
                Some((value.get("name")?.as_str()?.to_owned(), value.get("version")?.as_str()?.to_owned()))
            });
            if let Some(identity) = identity {
                if !retained.contains(&identity) { candidates.push(identity); }
            } else {
                // Malformed metadata is not enough evidence to safely infer a
                // package identity from a hyphenated filename; leave it alone.
                let _ = stem;
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    let mut removed = 0usize;
    for (name, version) in candidates {
        if registry::safe_registry_cache_component(&name).is_err() || registry::safe_registry_cache_component(&version).is_err() { continue; }
        let _lock = registry::lock_package_cache(&cache, &name)?;
        // Re-read the lockfile after obtaining the package lock so a concurrent
        // update that has just pinned this version wins over cache pruning.
        let latest = read_validated_lock(&lock_path, &manifest_hash)?;
        if latest.get(&name).is_some_and(|entry| entry.source == "registry" && entry.version == version) { continue; }
        let paths = [
            cache.join(format!("{name}-{version}")),
            cache.join(format!("{name}-{version}.tar")),
            cache.join(format!("{name}-{version}.tree.toml")),
            cache.join(format!("registry-meta-{name}-{version}.toml")),
        ];
        let existed = paths.iter().any(|path| fs::symlink_metadata(path).is_ok());
        for path in &paths { remove_cache_entry(path); }
        if existed { removed += 1; }
    }
    Ok(removed)
}

fn remove_cache_entry(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => { let _ = fs::remove_dir_all(path); }
        Ok(_) => { let _ = fs::remove_file(path); }
        Err(_) => {}
    }
}

fn resolve_dependencies_one(project: &Project, offline: bool, update: bool, include_auxiliary: bool) -> Result<(), String> {
    let cache = project.root.join("target/foundry/deps");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    // Hold this through lockfile publication: otherwise pruning could observe
    // the old lockfile after a package was populated but before its new pin lands.
    let _workspace_lock = registry::lock_workspace_cache(&cache)?;
    let registry_url = env::var("FOUNDRY_REGISTRY").ok();
    let manifest_path = if project.root.join(MANIFEST).is_file() { project.root.join(MANIFEST) } else { project.root.join(LEGACY_MANIFEST) };
    let manifest_bytes = fs::read(&manifest_path).map_err(|e|format!("read manifest: {e}"))?;
    let manifest_hash = fingerprint::fingerprint_fields([manifest_bytes.as_slice()]);
    let lock_path=project.root.join(LOCKFILE);
    let dependencies = if include_auxiliary {
        project.manifest.dependencies.iter().chain(project.manifest.dev_dependencies.iter()).chain(project.manifest.plugins.iter()).collect::<Vec<_>>()
    } else {
        project.manifest.dependencies.iter().collect::<Vec<_>>()
    };

    let locked = if !update && lock_path.is_file() {
        read_validated_lock(&lock_path, &manifest_hash)?
    } else {
        BTreeMap::new()
    };
    validate_lock_aliases(&locked, &project.manifest, &lock_path)?;

    let mut resolved = Vec::new();
    for (name, dep) in dependencies {
        match dep {
            Dependency::Path(p) => {
                let full = project.root.join(p);
                if !full.exists() { return Err(format!("dependency '{name}' path does not exist: {}", full.display())); }
                let canonical = full.canonicalize().map_err(|error| format!("canonicalize path dependency '{name}': {error}"))?;
                let source = format!("path+{}", canonical.display());
                let revision = directory_revision(&canonical);
                match locked.get(name) {
                    Some(entry) if entry.source == source && entry.requested == "path" && entry.revision == revision => {}
                    Some(_) => return Err(format!("path dependency '{name}' changed since foundry.lock was generated; run 'foundry update'")),
                    None if offline => return Err(format!("path dependency '{name}' has no lockfile entry (--offline); run 'foundry update' while online")),
                    None => {}
                }
            },
            Dependency::Git { url, rev } => {
                let dst = cache.join(name);
                if !dst.exists() {
                    if offline { return Err(format!("Git dependency '{name}' is not cached (--offline)")); }
                    let mut command = Command::new("git");
                    command.args(["clone", url]).arg(&dst);
                    if !run(&mut command, &project.root)? { return Err(format!("failed to clone dependency '{name}'")); }
                }

                let source = format!("git+{url}");
                let requested = rev.as_deref().unwrap_or("*");
                let existing_entry = locked.get(name);
                if let Some(entry) = existing_entry {
                    if entry.source != source || entry.requested != requested {
                        return Err(format!("lockfile source/selector for Git dependency '{name}' does not match Foundry.toml; run 'foundry update'"));
                    }
                }
                let locked_revision = existing_entry
                    .map(|entry| entry.revision.as_str())
                    .filter(|revision| is_git_commit(revision));
                if offline && locked_revision.is_none() {
                    return Err(format!("Git dependency '{name}' has no immutable commit in foundry.lock; run 'foundry update' while online"));
                }

                // Normal resolution preserves the lockfile commit. `foundry update`
                // refreshes remote refs and resolves the manifest selector (or the
                // remote's default branch) to a full object ID before checkout.
                let commit = if !update {
                    if let Some(revision) = locked_revision {
                        revision.to_owned()
                    } else {
                        if offline {
                            return Err(format!("Git dependency '{name}' is not pinned in foundry.lock (--offline)"));
                        }
                        let mut command = Command::new("git");
                        command.current_dir(&dst).args(["fetch", "--all", "--tags", "--force"]);
                        if !run(&mut command, &dst)? { return Err(format!("failed to fetch Git dependency '{name}'")); }
                        refresh_git_default_branch(&dst);
                        git_resolve_commit(&dst, rev.as_deref())?
                    }
                } else {
                    if offline {
                        return Err(format!("cannot update Git dependency '{name}' while offline"));
                    }
                    let mut command = Command::new("git");
                    command.current_dir(&dst).args(["fetch", "--all", "--tags", "--force"]);
                    if !run(&mut command, &dst)? { return Err(format!("failed to fetch Git dependency '{name}'")); }
                    refresh_git_default_branch(&dst);
                    git_resolve_commit(&dst, rev.as_deref())?
                };

                let mut command = Command::new("git");
                command.current_dir(&dst).args(["checkout", "--detach", commit.as_str()]);
                if !run(&mut command, &dst)? {
                    return Err(format!("failed to checkout resolved commit {commit} for Git dependency '{name}'"));
                }
                let actual = git_head(&dst).ok_or_else(|| format!("could not determine resolved Git commit for dependency '{name}'"))?;
                if actual != commit {
                    return Err(format!("Git dependency '{name}' checked out {actual}, expected resolved commit {commit}"));
                }
            },
            Dependency::Version(req) => {
                let registry=registry_url.as_deref().ok_or_else(||format!("dependency '{name}' requires a registry version '{req}', but FOUNDRY_REGISTRY is not set (expected a registry URL or local registry path)"))?;
                let existing_entry = locked.get(name);
                let registry_origin = registry.trim_end_matches('/');
                if let Some(entry) = existing_entry {
                    if entry.requested != *req {
                        return Err(format!("lockfile requirement for registry dependency '{name}' does not match Foundry.toml; run 'foundry update'"));
                    }
                    validate_registry_origin(name, entry, registry_origin)?;
                }
                // Offline resolution must never select a version from a cached
                // range index: the exact version and archive checksum must both
                // be present in foundry.lock, even if no lockfile existed yet.
                let locked_entry = exact_registry_pin(name, existing_entry, offline && !update)?;
                let requirement = if !update {
                    locked_entry.map(|entry| format!("={}", entry.version)).unwrap_or_else(|| req.clone())
                } else { req.clone() };
                let package = registry::resolve(name, &requirement, registry, &cache, offline)?;
                if !registry::satisfies(&package.version, req) {
                    return Err(format!("locked version {} of '{name}' no longer satisfies manifest requirement '{req}'", package.version));
                }
                if let Some(entry) = locked_entry {
                    if !entry.checksum.is_empty() && package.checksum.as_deref() != Some(entry.checksum.as_str()) {
                        return Err(format!("registry checksum changed for locked dependency '{name}' {}; run 'foundry update' only if you intend to refresh it", package.version));
                    }
                }
                let extracted = cache.join(format!("{}-{}", package.name, package.version));
                let alias_root = cache.join(name);
                materialize_package_alias(&extracted, &alias_root)?;
                resolved.push(registry::ResolvedPackage{requested:req.clone(),..package});
            }
        }
    }
    write_resolved_lock(project, &resolved, include_auxiliary)?;
    Ok(())
}

fn copy_package_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let package_root = source.canonicalize().map_err(|e| format!("canonicalize {}: {e}", source.display()))?;
    copy_package_tree_inner(source, destination, &package_root)
}

fn copy_package_tree_inner(source: &Path, destination: &Path, package_root: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| format!("create {}: {e}", destination.display()))?;
    for entry in fs::read_dir(source).map_err(|e| format!("read {}: {e}", source.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            copy_package_tree_inner(&from, &to, package_root)?;
        } else if kind.is_file() {
            fs::copy(&from, &to).map_err(|e| format!("copy {}: {e}", from.display()))?;
        } else if kind.is_symlink() {
            let resolved = from.canonicalize().map_err(|e| format!("resolve package symlink {}: {e}", from.display()))?;
            if !resolved.starts_with(package_root) {
                return Err(format!("registry package symlink '{}' escapes the extracted package root", from.display()));
            }
            if resolved.is_dir() {
                return Err(format!("registry package contains directory symlink '{}'; refusing to materialize it", from.display()));
            }
            if resolved.is_file() {
                fs::copy(&resolved, &to).map_err(|e| format!("copy symlink target {}: {e}", resolved.display()))?;
            }
        }
    }
    Ok(())
}

/// Registry extraction uses a name-version directory, while the package graph
/// addresses dependencies by their manifest alias. Materialize a stable alias
/// directory so aliases (including renamed dependencies) resolve consistently.
fn materialize_package_alias(extracted: &Path, alias_root: &Path) -> Result<(), String> {
    if !extracted.is_dir() {
        return Err(format!("resolved registry package directory is missing: {}", extracted.display()));
    }
    if alias_root.is_dir() { fs::remove_dir_all(alias_root).map_err(|e| format!("replace dependency alias {}: {e}", alias_root.display()))?; }
    else if alias_root.exists() { fs::remove_file(alias_root).map_err(|e| format!("replace dependency alias {}: {e}", alias_root.display()))?; }
    copy_package_tree(extracted, alias_root)
}

fn run(c: &mut Command, cwd: &Path) -> Result<bool, String> {
    c.current_dir(cwd);
    Ok(c.status()
        .map_err(|e| format!("could not start command: {e}"))?
        .success())
}
fn find_on_path(name: &str) -> Option<PathBuf> {
    let p = env::var_os("PATH")?;
    for d in env::split_paths(&p) {
        let x = d.join(name);
        if x.is_file() {
            return Some(x);
        }
    }
    None
}
pub fn find_fusion(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return Ok(p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    }
    if let Ok(p) = env::var("FUSION_COMPILER") {
        let path = PathBuf::from(p);
        return Ok(path.canonicalize().unwrap_or(path));
    }
    let name = if cfg!(windows) {
        "fusion.exe"
    } else {
        "fusion"
    };
    if let Some(p) = find_on_path(name) {
        return Ok(p);
    }
    if let Ok(me) = env::current_exe() {
        for base in me.ancestors().take(5) {
            for p in [
                base.join("fusion"),
                base.join("target/debug/fusion"),
                base.join("target/release/fusion"),
            ] {
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    Err("could not find Fusion compiler; set FUSION_COMPILER or put fusion on PATH".into())
}
/// Resolve a project-pinned compiler. A toolchain feed entry is TOML with `version`, `url`,
/// `sha256`, and `executable`; the archive is verified before it is used.
pub fn find_project_fusion(project: &Project, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if explicit.is_some() || env::var_os("FUSION_COMPILER").is_some() || project.manifest.toolchain.fusion.is_none() {
        return find_fusion(explicit);
    }
    let version=project.manifest.toolchain.fusion.as_deref().unwrap();
    let feed=env::var("FOUNDRY_TOOLCHAIN_FEED").map_err(|_|format!("project pins Fusion {version}, but FOUNDRY_TOOLCHAIN_FEED is not set"))?;
    let url=format!("{}/{}.toml",feed.trim_end_matches('/'),version);
    let metadata=if let Some(path)=url.strip_prefix("file://") { fs::read_to_string(path).map_err(|e|e.to_string())? } else if !url.contains("://") { fs::read_to_string(&url).map_err(|e|e.to_string())? } else {
        let out=Command::new("curl").args(["--fail","--silent","--show-error","--location"]).arg(&url).output().map_err(|e|format!("start curl for toolchain metadata: {e}"))?;
        if !out.status.success(){return Err(format!("could not retrieve pinned toolchain metadata: {url}"));} String::from_utf8(out.stdout).map_err(|e|e.to_string())?
    };
    let value:toml::Value=toml::from_str(&metadata).map_err(|e|format!("invalid toolchain metadata: {e}"))?;
    let actual=value.get("version").and_then(toml::Value::as_str).unwrap_or("");
    if actual!=version{return Err(format!("toolchain feed returned version '{actual}', but project pins '{version}'"));}
    let url=value.get("url").and_then(toml::Value::as_str).ok_or("toolchain metadata missing url")?.to_string();
    let sha256=value.get("sha256").and_then(toml::Value::as_str).ok_or("toolchain metadata missing sha256")?.to_string();
    let executable=value.get("executable").and_then(toml::Value::as_str).unwrap_or(if cfg!(windows){"fusion.exe"}else{"fusion"}).to_string();
    let home=env::var_os("FOUNDRY_HOME").map(PathBuf::from).or_else(||env::var_os("HOME").map(|h|PathBuf::from(h).join(".foundry"))).unwrap_or_else(||project.root.join("target/foundry-home"));
    let path=toolchain::install(&toolchain::ToolchainSpec{version:version.into(),url,sha256,executable},&home.join("toolchains"),env::var_os("FOUNDRY_OFFLINE").is_some())?;
    Ok(path)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out)
        } else if p.extension() == Some(OsStr::new("fusion")) {
            out.push(p)
        }
    }
}
pub fn source_files(p: &Project) -> Vec<PathBuf> {
    let mut v = Vec::new();
    collect(&p.root.join("src"), &mut v);
    v.sort();
    v
}
pub fn test_files(p: &Project) -> Vec<PathBuf> {
    let mut v = Vec::new();
    collect(&p.root.join("tests"), &mut v);
    v.sort();
    v
}
pub fn main_source(p: &Project) -> Result<PathBuf, String> {
    let x = p.root.join("src/main.fusion");
    if x.is_file() {
        Ok(x)
    } else {
        source_files(p)
            .into_iter()
            .next()
            .ok_or_else(|| "no .fusion source files found in src/".into())
    }
}

pub fn build(p: &Project, profile: BuildProfile, fusion: Option<&Path>, extra: &[String]) -> Result<PathBuf, String> {
    build_inner(p, profile, fusion, extra, true)
}


fn package_root_arguments_for(
    graph: &package_graph::PackageGraph,
    package_root: &Path,
) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut roots_by_alias = BTreeMap::<String, PathBuf>::new();
    let mut pending = vec![package_root.to_path_buf()];
    let mut visited = std::collections::BTreeSet::new();

    // Include the complete transitive dependency closure of the package being
    // compiled. A dependency's own imports must resolve without relying on the
    // root application's direct-dependency list.
    while let Some(parent) = pending.pop() {
        if !visited.insert(parent.clone()) { continue; }
        for edge in graph.edges.iter().filter(|edge| edge.from == parent) {
            let node = graph.nodes.get(&edge.to).ok_or_else(|| {
                format!("package graph is missing dependency node for alias '{}'", edge.alias)
            })?;
            let import_alias = edge.alias.replace('-', "_");
            if let Some(previous) = roots_by_alias.insert(import_alias.clone(), node.source_root.clone()) {
                if previous != node.source_root {
                    return Err(format!(
                        "transitive dependency alias '{}' resolves to multiple packages; use distinct dependency aliases so compiler module imports are unambiguous",
                        import_alias,
                    ));
                }
            }
            pending.push(edge.to.clone());
        }
    }

    for (import_alias, source_root) in roots_by_alias {
        args.push("--package-root".to_string());
        args.push(format!("{}={}", import_alias, source_root.display()));
    }
    Ok(args)
}

fn package_root_arguments(graph: &package_graph::PackageGraph) -> Result<Vec<String>, String> {
    package_root_arguments_for(graph, &graph.root)
}

fn resolved_package_graph(p: &Project) -> Result<package_graph::PackageGraph, String> {
    resolve_dependencies(p, env::var_os("FOUNDRY_OFFLINE").is_some())?;
    package_graph::resolve_package_graph(p).map_err(|e| e.to_string())
}

/// Resolve Clang using the same precedence as Fusion's `link-llvm` command.
/// This intentionally mirrors main.rs::find_clang so a toolchain change invalidates
/// the executable fingerprint even when the Fusion compiler itself is unchanged.
fn resolve_linker_path() -> Option<PathBuf> {
    for variable in ["FUSION_LLVM_DIR", "LLVM_HOME"] {
        if let Ok(dir) = env::var(variable) {
            let candidate = PathBuf::from(dir).join("bin").join(if cfg!(windows) { "clang.exe" } else { "clang" });
            if candidate.is_file() { return Some(candidate); }
        }
    }
    let command = if cfg!(windows) { "clang.exe" } else { "clang" };
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let candidate = directory.join(command);
            if candidate.is_file() { return Some(candidate); }
        }
    }
    #[cfg(windows)]
    {
        let candidates = [
            PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files (x86)\LLVM\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin\clang.exe"),
        ];
        for candidate in candidates { if candidate.is_file() { return Some(candidate); } }
    }
    None
}

/// Identity of the actual native linker invoked by `fusion link-llvm`.
/// The Clang executable hash is authoritative; version output is retained for
/// human-readable provenance and to capture wrapper/configuration differences.
fn query_linker(path: &Path, argument: &str) -> (String, String, Vec<u8>) {
    match Command::new(path).arg(argument).output() {
        Ok(output) => {
            let status = if output.status.success() { "success".to_string() }
                else { format!("exit:{}", output.status.code().map(|code| code.to_string()).unwrap_or_else(|| "signal".into())) };
            let mut bytes = output.stdout;
            bytes.extend_from_slice(&output.stderr);
            (status, String::from_utf8_lossy(&bytes).trim().to_string(), bytes)
        }
        Err(error) => (format!("spawn-error:{error}"), format!("<unavailable:{error}>"), Vec::new()),
    }
}

/// Hash the Clang resource directory as part of the effective toolchain identity.
/// This covers builtin headers/runtime resources that can affect compilation even
/// when the clang executable itself is unchanged.
fn linker_resource_directory_hash(path: &Path) -> (String, String, String) {
    let (status, resource_dir, _) = query_linker(path, "-print-resource-dir");
    if status != "success" {
        return (status, resource_dir, "<unavailable>".into());
    }
    if resource_dir.is_empty() {
        return ("resource-dir-empty".into(), resource_dir, "<unavailable>".into());
    }
    let root = PathBuf::from(&resource_dir);
    let canonical = root.canonicalize().unwrap_or(root);
    if !canonical.is_dir() {
        return ("resource-dir-not-directory".into(), resource_dir, "<unavailable>".into());
    }
    fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|error| format!("read {}: {error}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                collect(&path, files)?;
            } else if kind.is_file() {
                files.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    if let Err(error) = collect(&canonical, &mut files) {
        return (format!("resource-hash-error:{error}"), resource_dir, "<unavailable>".into());
    }
    files.sort();
    let mut fields = Vec::<Vec<u8>>::new();
    for file in files {
        let relative = file.strip_prefix(&canonical).unwrap_or(&file).to_string_lossy().into_owned();
        fields.push(relative.into_bytes());
        match fs::read(&file) {
            Ok(bytes) => fields.push(bytes),
            Err(error) => return (format!("resource-hash-error:{error}"), resource_dir, "<unavailable>".into()),
        }
    }
    let digest = fingerprint::fingerprint_fields(fields.iter().map(|field| field.as_slice()));
    (status, resource_dir, digest)
}

#[derive(Clone, Debug)]
struct LinkerIdentity {
    path: String,
    binary_sha256: String,
    version_status: String,
    version_text: String,
    version_sha256: String,
    target_status: String,
    target_triple: String,
    resource_status: String,
    resource_dir: String,
    resource_sha256: String,
    search_dirs_status: String,
    search_dirs: String,
    search_dirs_sha256: String,
    environment: Vec<(String, String)>,
}

impl LinkerIdentity {
    fn fingerprint_fields(&self) -> Vec<Vec<u8>> {
        let mut fields = vec![
            b"foundry-linker-identity-v3".to_vec(), self.path.as_bytes().to_vec(),
            self.binary_sha256.as_bytes().to_vec(), self.version_status.as_bytes().to_vec(),
            self.version_sha256.as_bytes().to_vec(), self.version_text.as_bytes().to_vec(),
            self.target_status.as_bytes().to_vec(), self.target_triple.as_bytes().to_vec(),
            self.resource_status.as_bytes().to_vec(), self.resource_dir.as_bytes().to_vec(),
            self.resource_sha256.as_bytes().to_vec(), self.search_dirs_status.as_bytes().to_vec(),
            self.search_dirs.as_bytes().to_vec(), self.search_dirs_sha256.as_bytes().to_vec(),
        ];
        for (name, value) in &self.environment {
            fields.push(name.as_bytes().to_vec());
            fields.push(value.as_bytes().to_vec());
        }
        fields.extend(linker_config::FLAGS.iter().map(|flag| flag.as_bytes().to_vec()));
        fields
    }
}

fn linker_identity_for(path: Option<&Path>) -> LinkerIdentity {
    let Some(path) = path else {
        return LinkerIdentity {
            path: "<clang-unresolved>".into(), binary_sha256: "<unresolved>".into(),
            version_status: "unresolved".into(), version_text: "<unresolved>".into(),
            version_sha256: fingerprint::fingerprint_fields([b"<unresolved>".as_slice()]),
            target_status: "unresolved".into(), target_triple: "<unresolved>".into(),
            resource_status: "unresolved".into(), resource_dir: "<unresolved>".into(),
            resource_sha256: "<unresolved>".into(), search_dirs_status: "unresolved".into(),
            search_dirs: "<unresolved>".into(), search_dirs_sha256: "<unresolved>".into(),
            environment: linker_environment_identity(),
        };
    };
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let binary_sha256 = fingerprint::file_sha256(&resolved)
        .unwrap_or_else(|error| format!("<unavailable:{error}>"));
    let (version_status, version_text, version_bytes) = query_linker(&resolved, "--version");
    let version_sha256 = fingerprint::fingerprint_fields([version_bytes.as_slice()]);
    let (target_status, target_triple, _) = query_linker(&resolved, "-dumpmachine");
    let (resource_status, resource_dir, resource_sha256) = linker_resource_directory_hash(&resolved);
    let (search_dirs_status, search_dirs, search_dirs_bytes) = query_linker(&resolved, "-print-search-dirs");
    let search_dirs_sha256 = fingerprint::fingerprint_fields([search_dirs_bytes.as_slice()]);
    LinkerIdentity {
        path: resolved.to_string_lossy().into_owned(), binary_sha256, version_status, version_text,
        version_sha256, target_status, target_triple, resource_status, resource_dir, resource_sha256,
        search_dirs_status, search_dirs, search_dirs_sha256, environment: linker_environment_identity(),
    }
}

/// Capture environment variables that can redirect Clang's headers, SDK, or
/// native library selection. Values are recorded explicitly in local provenance
/// and included in the root fingerprint; unset variables are explicit too.
fn linker_environment_identity() -> Vec<(String, String)> {
    const VARIABLES: &[&str] = &[
        "SDKROOT", "DEVELOPER_DIR", "MACOSX_DEPLOYMENT_TARGET", "COMPILER_PATH",
        "GCC_EXEC_PREFIX", "CLANG_CONFIG_FILE", "CLANG_CONFIG_FILE_USER_DIR",
        "CLANG_CONFIG_FILE_SYSTEM_DIR", "CCC_OVERRIDE_OPTIONS", "CLANG_RESOURCE_DIR",
        "LIBRARY_PATH", "CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH",
        "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "SOURCE_DATE_EPOCH",
    ];
    VARIABLES.iter().map(|name| {
        let value = env::var_os(name).map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<unset>".into());
        ((*name).to_string(), value)
    }).collect()
}

#[cfg(test)]
fn linker_identity_fields_for(path: Option<&Path>) -> Vec<Vec<u8>> {
    linker_identity_for(path).fingerprint_fields()
}

/// Return stable compiler identity inputs for package and root build keys. The
/// executable content hash is authoritative: replacing a compiler in place must
/// invalidate artifacts even when its path and reported version remain unchanged.
/// Version output and the resolved path remain useful provenance/debugging inputs.
fn compiler_identity_fields(compiler: &Path) -> Vec<Vec<u8>> {
    let resolved = compiler.canonicalize().unwrap_or_else(|_| compiler.to_path_buf());
    let binary_hash = fingerprint::file_sha256(&resolved)
        .unwrap_or_else(|error| format!("<unavailable:{error}>"));
    let version_result = Command::new(compiler).arg("--version").output();
    let (version_status, version_bytes) = match version_result {
        Ok(output) => {
            let status = if output.status.success() { "success".to_string() }
                else { format!("exit:{}", output.status.code().map(|code| code.to_string()).unwrap_or_else(|| "signal".into())) };
            let mut bytes = output.stdout;
            bytes.extend_from_slice(&output.stderr);
            (status, bytes)
        }
        Err(error) => (format!("spawn-error:{error}"), Vec::new()),
    };
    let version_hash = fingerprint::fingerprint_fields([version_bytes.as_slice()]);
    vec![
        b"foundry-compiler-identity-v2".to_vec(),
        resolved.to_string_lossy().as_bytes().to_vec(),
        binary_hash.into_bytes(),
        version_status.into_bytes(),
        version_hash.into_bytes(),
        version_bytes,
    ]
}

/// Compute deterministic, dependency-aware fingerprints for every package in the
/// graph. Each key addresses a package-scoped LLVM module; the root key also
/// addresses the separately cached application entrypoint/runtime module.
fn package_build_fingerprints(
    graph: &package_graph::PackageGraph,
    compiler: &Path,
    profile: BuildProfile,
    extra: &[String],
) -> Result<BTreeMap<PathBuf, String>, String> {
    let compiler_fields = compiler_identity_fields(compiler);

    let mut fingerprints = BTreeMap::<PathBuf, String>::new();
    for package_root in &graph.build_order {
        let node = graph.nodes.get(package_root).ok_or_else(|| {
            format!("package graph is missing node {}", package_root.display())
        })?;
        let manifest_path = if package_root.join(MANIFEST).is_file() {
            package_root.join(MANIFEST)
        } else {
            package_root.join(LEGACY_MANIFEST)
        };
        let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
            format!("read package manifest {}: {error}", manifest_path.display())
        })?;

        let mut fields = vec![
            b"foundry-package-key-v2".to_vec(),
            node.name.as_bytes().to_vec(),
            node.version.as_bytes().to_vec(),
            profile.name().as_bytes().to_vec(),
            format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH).into_bytes(),
            manifest_bytes,
        ];
        fields.extend(compiler_fields.iter().cloned());
        for argument in extra {
            fields.push(argument.as_bytes().to_vec());
        }

        let lock_path = package_root.join(LOCKFILE);
        if lock_path.is_file() {
            fields.push(fs::read(&lock_path).map_err(|error| {
                format!("read package lockfile {}: {error}", lock_path.display())
            })?);
        } else {
            fields.push(b"<no-lockfile>".to_vec());
        }

        let mut sources = Vec::new();
        collect(&node.source_root, &mut sources);
        sources.sort();
        for source in sources {
            let relative = source
                .strip_prefix(&node.source_root)
                .unwrap_or(&source)
                .to_string_lossy()
                .into_owned();
            fields.push(relative.into_bytes());
            fields.push(fs::read(&source).map_err(|error| {
                format!("read package source {}: {error}", source.display())
            })?);
        }

        // A package key incorporates its direct dependency keys. Since the graph
        // order is dependency-first, every child key is already available here.
        let mut edges = graph.edges.iter().filter(|edge| &edge.from == package_root).collect::<Vec<_>>();
        edges.sort_by(|left, right| left.alias.cmp(&right.alias));
        for edge in edges {
            fields.push(edge.alias.as_bytes().to_vec());
            let child_key = fingerprints.get(&edge.to).ok_or_else(|| {
                format!(
                    "package graph order is invalid: dependency '{}' of '{}' has no fingerprint",
                    edge.alias, node.name
                )
            })?;
            fields.push(child_key.as_bytes().to_vec());
        }

        let key = fingerprint::fingerprint_fields(fields.iter().map(|field| field.as_slice()));
        fingerprints.insert(package_root.clone(), key);
    }
    Ok(fingerprints)
}


/// Compare current dependency-aware package keys with the last successful build.
/// The resulting plan is package-granular even while the compiler emits one
/// application artifact for the complete dependency graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PackageBuildPlan {
    changed: Vec<PathBuf>,
    unchanged: Vec<PathBuf>,
    removed: Vec<PathBuf>,
}

fn package_state_path(output_dir: &Path) -> PathBuf {
    output_dir.join(".foundry-package-fingerprints.toml")
}

fn package_graph_state_path(output_dir: &Path) -> PathBuf {
    output_dir.join(".foundry-package-graph.toml")
}

/// Package artifacts are independently emitted LLVM modules. The application
/// entrypoint/runtime is emitted separately and all modules are passed to Clang
/// together for the final native link.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PackageArtifactRecord {
    format_version: u32,
    package_root: PathBuf,
    package_name: String,
    package_version: String,
    fingerprint: String,
    artifact_file: String,
    artifact_sha256: String,
    artifact_kind: String,
    interface_file: String,
    interface_sha256: String,
}

fn package_artifact_store_path(output_dir: &Path) -> PathBuf {
    output_dir.join(".foundry-package-artifacts")
}

fn package_artifact_record_path(output_dir: &Path, package_root: &Path) -> PathBuf {
    let identity = fingerprint::fingerprint_fields([package_root.to_string_lossy().as_bytes()]);
    package_artifact_store_path(output_dir).join(identity).join("artifact.toml")
}

fn read_package_artifact_record(path: &Path) -> Option<PackageArtifactRecord> {
    let contents = fs::read_to_string(path).ok()?;
    let record = toml::from_str::<PackageArtifactRecord>(&contents).ok()?;
    (record.format_version == 6).then_some(record)
}

fn package_record_artifact_path(output_dir: &Path, record: &PackageArtifactRecord) -> Option<PathBuf> {
    let relative = Path::new(&record.artifact_file);
    if relative.as_os_str().is_empty()
        || !relative.components().all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(package_artifact_store_path(output_dir).join(relative))
}

fn package_record_interface_path(output_dir: &Path, record: &PackageArtifactRecord) -> Option<PathBuf> {
    let relative = Path::new(&record.interface_file);
    if relative.as_os_str().is_empty()
        || !relative.components().all(|component| matches!(component, std::path::Component::Normal(_)))
    { return None; }
    Some(package_artifact_store_path(output_dir).join(relative))
}

/// Validate package-owned data/type ABI records before accepting cached or
/// freshly emitted interfaces. Struct records must carry stable identified-type
/// names and a self-consistent ordered storage layout; unsupported array and
/// nested aggregate fields remain explicit and are rejected by link validation.
fn validate_package_interface(contents: &str, package_name: &str) -> Result<(), String> {
    use sha2::Digest;
    if !contents.starts_with("FUSION-PACKAGE-INTERFACE\t6\n") {
        return Err(format!("package '{}' has an unsupported interface version (expected v6)", package_name));
    }
    let mut structs = std::collections::BTreeSet::<String>::new();
    let mut enums = std::collections::BTreeSet::<String>::new();
    let mut type_symbols = std::collections::BTreeSet::<String>::new();
    let mut constants = std::collections::BTreeSet::<String>::new();
    let mut data_symbols = std::collections::BTreeSet::<String>::new();
    for (index, line) in contents.lines().enumerate().skip(1) {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.first().copied() {
            Some("abi-struct") => {
                if fields.len() != 7 {
                    return Err(format!("package '{}' has malformed struct ABI record at line {}", package_name, index + 1));
                }
                let identity = format!("{}::{}", fields[1], fields[2]);
                if !fields[3].starts_with("%fusion.type.__fusion_m") || !fields[3].bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$' | b'%')) || !type_symbols.insert(fields[3].to_string()) {
                    return Err(format!("package '{}' has invalid or duplicate type symbol for '{}'", package_name, identity));
                }
                if !structs.insert(identity.clone()) {
                    return Err(format!("package '{}' declares struct ABI '{}' more than once", package_name, identity));
                }
                let count = fields[4].parse::<usize>().map_err(|_| format!("package '{}' has invalid field count for '{}'", package_name, identity))?;
                if fields[5].len() != 64 || !fields[5].bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(format!("package '{}' has invalid layout digest for '{}'", package_name, identity));
                }
                let actual_layout_digest = format!("{:x}", sha2::Sha256::digest(fields[6].as_bytes()));
                if actual_layout_digest != fields[5] {
                    return Err(format!("package '{}' has inconsistent layout digest for '{}'", package_name, identity));
                }
                let entries = if fields[6].is_empty() { Vec::new() } else { fields[6].split(',').collect::<Vec<_>>() };
                if entries.len() != count {
                    return Err(format!("package '{}' has inconsistent field count for '{}'", package_name, identity));
                }
                let mut names = std::collections::BTreeSet::new();
                for entry in entries {
                    let (name, ty) = entry.split_once(':').ok_or_else(|| format!("package '{}' has malformed field in '{}'", package_name, identity))?;
                    if name.is_empty() || !names.insert(name) || !matches!(ty, "i64" | "double" | "i1" | "ptr" | "unsupported-array" | "unsupported-aggregate") {
                        return Err(format!("package '{}' has invalid or duplicate field in '{}'", package_name, identity));
                    }
                }
            }
            Some("abi-enum") => {
                if fields.len() != 7 {
                    return Err(format!("package '{}' has malformed enum ABI record at line {}", package_name, index + 1));
                }
                let identity = format!("{}::{}", fields[1], fields[2]);
                if !fields[3].starts_with("%fusion.type.__fusion_m") || !fields[3].bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$' | b'%')) || !type_symbols.insert(fields[3].to_string()) {
                    return Err(format!("package '{}' has invalid or duplicate enum type symbol for '{}'", package_name, identity));
                }
                if !enums.insert(identity.clone()) {
                    return Err(format!("package '{}' declares enum ABI '{}' more than once", package_name, identity));
                }
                let count = fields[4].parse::<usize>().map_err(|_| format!("package '{}' has invalid variant count for '{}'", package_name, identity))?;
                if fields[5].len() != 64 || !fields[5].bytes().all(|byte| byte.is_ascii_hexdigit()) || format!("{:x}", sha2::Sha256::digest(fields[6].as_bytes())) != fields[5] {
                    return Err(format!("package '{}' has inconsistent enum layout digest for '{}'", package_name, identity));
                }
                let variants = if fields[6].is_empty() { Vec::new() } else { fields[6].split(';').collect::<Vec<_>>() };
                if variants.len() != count { return Err(format!("package '{}' has inconsistent enum variant count for '{}'", package_name, identity)); }
                let mut names = std::collections::BTreeSet::new();
                for (expected, variant) in variants.iter().enumerate() {
                    let (name, payload) = variant.split_once('=').ok_or_else(|| format!("package '{}' has malformed enum variant in '{}'", package_name, identity))?;
                    let (discriminant, payload) = payload.split_once('(').ok_or_else(|| format!("package '{}' has malformed enum payload in '{}'", package_name, identity))?;
                    if !payload.ends_with(')') || name.is_empty() || !names.insert(name) || discriminant.parse::<usize>().ok() != Some(expected) {
                        return Err(format!("package '{}' has invalid enum variant in '{}'", package_name, identity));
                    }
                    let payload = &payload[..payload.len()-1];
                    if !payload.is_empty() && !payload.split(',').all(|ty| matches!(ty, "i64" | "double" | "i1" | "ptr")) {
                        return Err(format!("package '{}' has unsupported enum payload type in '{}'", package_name, identity));
                    }
                }
            }
            Some("abi-enum-payload") => {
                if fields.len() != 8 || fields[1].is_empty() || fields[2].is_empty() || fields[3].is_empty()
                    || !fields[4].starts_with("%fusion.variant.")
                    || !fields[4].bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$' | b'%')) {
                    return Err(format!("package '{}' has malformed enum payload ABI at line {}", package_name, index + 1));
                }
                let count = fields[5].parse::<usize>().map_err(|_| format!("package '{}' has invalid enum payload field count", package_name))?;
                let payload = if fields[7].is_empty() { Vec::new() } else { fields[7].split(',').collect::<Vec<_>>() };
                if payload.len() != count || !payload.iter().all(|ty| matches!(*ty, "i64" | "double" | "i1" | "ptr"))
                    || fields[6].len() != 64 || format!("{:x}", sha2::Sha256::digest(fields[7].as_bytes())) != fields[6] {
                    return Err(format!("package '{}' has inconsistent enum payload layout", package_name));
                }
            }
            Some("abi-data") | Some("abi-internal-data") => {
                if fields.len() != 7 || fields[1].is_empty() || fields[2].is_empty() {
                    return Err(format!("package '{}' has malformed data ABI record at line {}", package_name, index + 1));
                }
                let identity = format!("{}::{}", fields[1], fields[2]);
                if fields[3].is_empty() || !fields[3].starts_with("_F") || !fields[3].bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$')) || !data_symbols.insert(fields[3].to_string()) {
                    return Err(format!("package '{}' has invalid or duplicate data symbol for '{}'", package_name, identity));
                }
                if !constants.insert(identity.clone()) {
                    return Err(format!("package '{}' declares data ABI '{}' more than once", package_name, identity));
                }
                let valid_type = matches!(fields[4], "i64" | "double" | "i1" | "ptr");
                let valid_value = match (fields[4], fields[5]) {
                    ("i64", "integer") => fields[6].parse::<i64>().is_ok(),
                    ("double", "float-bits") => fields[6].len() == 16 && fields[6].bytes().all(|byte| byte.is_ascii_hexdigit()),
                    ("i1", "boolean") => fields[6] == "0" || fields[6] == "1",
                    ("ptr", "utf8-hex") => fields[6].len() % 2 == 0 && fields[6].bytes().all(|byte| byte.is_ascii_hexdigit()),
                    _ => false,
                };
                if !valid_type || !valid_value {
                    return Err(format!("package '{}' has invalid scalar data ABI '{}'", package_name, identity));
                }
            }
            Some("abi-data-unsupported") | Some("abi-internal-data-unsupported") if fields.len() != 4 => {
                return Err(format!("package '{}' has malformed unsupported data record at line {}", package_name, index + 1));
            }
            _ => {}
        }
    }
    Ok(())
}

fn package_artifact_cache_hits(
    output_dir: &Path,
    graph: &package_graph::PackageGraph,
    fingerprints: &BTreeMap<PathBuf, String>,
) -> usize {
    let mut hits = 0;
    for (root, node) in &graph.nodes {
        let Some(expected) = fingerprints.get(root) else { continue };
        let Some(record) = read_package_artifact_record(&package_artifact_record_path(output_dir, root)) else { continue };
        let Some(artifact) = package_record_artifact_path(output_dir, &record) else { continue };
        let Some(interface) = package_record_interface_path(output_dir, &record) else { continue };
        if record.package_root != *root || record.fingerprint != *expected || record.artifact_kind != "llvm-package-ir-v5" {
            continue;
        }
        if fingerprint::file_sha256(&artifact).ok().as_deref() != Some(record.artifact_sha256.as_str())
            || fingerprint::file_sha256(&interface).ok().as_deref() != Some(record.interface_sha256.as_str()) {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&interface) else { continue };
        if validate_package_interface(&contents, &node.name).is_ok() { hits += 1; }
    }
    hits
}

/// Commit package cache records only after the whole root build succeeds (or a
/// matching remote/fresh artifact is restored). Then prune records for packages
/// no longer present in the graph. Cleanup is restricted to Foundry's own store
/// and only removes hash-named directories containing a valid v5 record.
fn persist_package_artifact_records(
    output_dir: &Path,
    graph: &package_graph::PackageGraph,
    fingerprints: &BTreeMap<PathBuf, String>,
    artifacts: &BTreeMap<PathBuf, PathBuf>,
) -> Result<usize, String> {
    let store = package_artifact_store_path(output_dir);
    fs::create_dir_all(&store).map_err(|error| format!("create package artifact store: {error}"))?;

    // Validate and serialize the complete active set before touching any committed
    // record. A bad/missing artifact must not leave a half-updated cache generation.
    let mut staged = Vec::<(PathBuf, PathBuf, String)>::new();
    for (root, node) in &graph.nodes {
        let fingerprint = fingerprints.get(root).ok_or_else(|| {
            format!("missing package fingerprint for {}", root.display())
        })?;
        let artifact = artifacts.get(root).ok_or_else(|| {
            format!("missing compiled package artifact for {}", root.display())
        })?;
        let artifact_file = artifact.strip_prefix(&store).map_err(|_| {
            format!("package artifact {} is outside the Foundry artifact store", artifact.display())
        })?.to_string_lossy().into_owned();
        let artifact_sha256 = fingerprint::file_sha256(artifact)
            .map_err(|error| format!("hash package artifact {}: {error}", artifact.display()))?;
        let interface = artifact.with_extension("fpi");
        if !interface.is_file() {
            return Err(format!("package interface is missing for {}", root.display()));
        }
        let interface_header = fs::read_to_string(&interface)
            .map_err(|error| format!("read package interface {}: {error}", interface.display()))?;
        validate_package_interface(&interface_header, &node.name)?;
        let interface_file = interface.strip_prefix(&store).map_err(|_| {
            format!("package interface {} is outside the Foundry artifact store", interface.display())
        })?.to_string_lossy().into_owned();
        let interface_sha256 = fingerprint::file_sha256(&interface)
            .map_err(|error| format!("hash package interface {}: {error}", interface.display()))?;
        let record = PackageArtifactRecord {
            format_version: 6,
            package_root: root.clone(),
            package_name: node.name.clone(),
            package_version: node.version.clone(),
            fingerprint: fingerprint.clone(),
            artifact_file,
            artifact_sha256,
            artifact_kind: "llvm-package-ir-v5".into(),
            interface_file,
            interface_sha256,
        };
        let path = package_artifact_record_path(output_dir, root);
        let contents = toml::to_string(&record)
            .map_err(|error| format!("serialize package artifact record: {error}"))?;
        let temporary = path.with_extension("toml.tmp");
        staged.push((path, temporary, contents));
    }

    // Write every staged record first. Only after all writes succeed do we rename
    // them into place; the build fingerprint remains the final success marker.
    let mut written = Vec::new();
    for (_, temporary, contents) in &staged {
        if let Some(parent) = temporary.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("create package record directory: {error}"))?;
        }
        if let Err(error) = fs::write(temporary, contents) {
            for path in &written { let _ = fs::remove_file(path); }
            return Err(format!("write package artifact record: {error}"));
        }
        written.push(temporary.clone());
    }
    for (path, temporary, _) in &staged {
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path).map_err(|error| format!("replace package artifact record: {error}"))?;
        }
        if let Err(error) = fs::rename(temporary, path) {
            for (_, pending, _) in &staged { let _ = fs::remove_file(pending); }
            return Err(format!("commit package artifact record: {error}"));
        }
    }

    let active = graph.nodes.keys().cloned().collect::<std::collections::BTreeSet<_>>();
    let mut removed = 0;
    let entries = fs::read_dir(&store).map_err(|error| format!("read package artifact store: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read package artifact store entry: {error}"))?;
        let path = entry.path();
        if !path.is_dir() { continue; }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() != 64 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) { continue; }
        let record_path = path.join("artifact.toml");
        let Some(record) = read_package_artifact_record(&record_path) else { continue };
        if !active.contains(&record.package_root) {
            fs::remove_dir_all(&path).map_err(|error| format!("remove stale package artifact record {}: {error}", path.display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Compile each package to a reusable package-scoped LLVM module. The separate
/// entrypoint artifact owns `main` and shared runtime definitions; the final
/// link consumes all modules together.
fn compile_package_artifacts(
    output_dir: &Path,
    graph: &package_graph::PackageGraph,
    fingerprints: &BTreeMap<PathBuf, String>,
    compiler: &Path,
) -> Result<(BTreeMap<PathBuf, PathBuf>, usize), String> {
    let store = package_artifact_store_path(output_dir);
    fs::create_dir_all(&store).map_err(|error| format!("create package artifact store: {error}"))?;
    let mut artifacts = BTreeMap::new();
    let mut hits = 0usize;
    for package_root in &graph.build_order {
        let node = graph.nodes.get(package_root).ok_or_else(|| {
            format!("package graph is missing node {}", package_root.display())
        })?;
        let key = fingerprints.get(package_root).ok_or_else(|| {
            format!("missing package fingerprint for {}", package_root.display())
        })?;
        let record_path = package_artifact_record_path(output_dir, package_root);
        if let Some(record) = read_package_artifact_record(&record_path) {
            let artifact = package_record_artifact_path(output_dir, &record);
            if let Some(artifact) = artifact.filter(|artifact| record.package_root == *package_root
                && record.fingerprint == *key
                && record.artifact_kind == "llvm-package-ir-v5"
                && fingerprint::file_sha256(artifact).ok().as_deref() == Some(record.artifact_sha256.as_str()))
            {
                let interface = package_record_interface_path(output_dir, &record);
                if let Some(interface) = interface.filter(|path| fingerprint::file_sha256(path).ok().as_deref() == Some(record.interface_sha256.as_str())) {
                    let contents = fs::read_to_string(&interface).map_err(|error| format!("read package interface {}: {error}", interface.display()))?;
                    if validate_package_interface(&contents, &node.name).is_ok() {
                        artifacts.insert(package_root.clone(), artifact);
                        hits += 1;
                        continue;
                    }
                }
            }
        }
        // Compile every package against the same application-root module graph.
        // This preserves the package-qualified module identities used by imports
        // in the application (for example `math`, rather than a fresh `crate`).
        let entry = graph.nodes.get(&graph.root).and_then(|root| root.entry.as_ref()).ok_or_else(|| {
            format!("root package has no Fusion entry file while compiling '{}'", node.name)
        })?;
        let identity = fingerprint::fingerprint_fields([package_root.to_string_lossy().as_bytes()]);
        let package_dir = store.join(&identity);
        fs::create_dir_all(&package_dir).map_err(|error| format!("create artifact directory: {error}"))?;
        let artifact = package_dir.join(format!("{key}.ll"));
        let temporary = package_dir.join(format!("{key}.ll.tmp"));
        let mut command = Command::new(compiler);
        command.current_dir(package_root)
            .arg("emit-package-llvm")
            .arg(entry)
            .arg("--package-source-root")
            .arg(&node.source_root)
            .arg("-o")
            .arg(&temporary);
        command.args(package_root_arguments(graph)?);
        if !run(&mut command, package_root)? {
            let _ = fs::remove_file(&temporary);
            return Err(format!("could not emit reusable LLVM artifact for package '{}'", node.name));
        }
        if !temporary.is_file() {
            return Err(format!("compiler did not produce package artifact for '{}'", node.name));
        }
        #[cfg(windows)]
        if artifact.exists() { fs::remove_file(&artifact).map_err(|error| error.to_string())?; }
        fs::rename(&temporary, &artifact).map_err(|error| format!("commit package artifact: {error}"))?;
        let interface = artifact.with_extension("fpi");
        let interface_temporary = package_dir.join(format!("{key}.fpi.tmp"));
        let mut interface_command = Command::new(compiler);
        interface_command.current_dir(package_root)
            .arg("package-interface")
            .arg(entry)
            .arg("--package-source-root")
            .arg(&node.source_root)
            .arg("-o")
            .arg(&interface_temporary);
        interface_command.args(package_root_arguments(graph)?);
        if !run(&mut interface_command, package_root)? {
            let _ = fs::remove_file(&interface_temporary);
            return Err(format!("could not emit package interface for '{}'", node.name));
        }
        if !interface_temporary.is_file() {
            return Err(format!("compiler did not produce package interface for '{}'", node.name));
        }
        let interface_contents = fs::read_to_string(&interface_temporary)
            .map_err(|error| format!("read generated package interface for '{}': {error}", node.name))?;
        validate_package_interface(&interface_contents, &node.name)?;
        #[cfg(windows)]
        if interface.exists() { fs::remove_file(&interface).map_err(|error| error.to_string())?; }
        fs::rename(&interface_temporary, &interface).map_err(|error| format!("commit package interface: {error}"))?;
        artifacts.insert(package_root.clone(), artifact);
    }
    Ok((artifacts, hits))
}

/// Compile/cache the application entrypoint plus the one shared runtime module.
/// Its key follows the root package fingerprint, which includes all transitive
/// dependency keys and therefore cannot become stale after an ABI-relevant change.
fn compile_entrypoint_artifact(
    output_dir: &Path,
    graph: &package_graph::PackageGraph,
    root_fingerprint: &str,
    compiler: &Path,
) -> Result<PathBuf, String> {
    let root_node = graph.nodes.get(&graph.root).ok_or("package graph has no root node")?;
    let entry = root_node.entry.as_ref().ok_or_else(|| {
        format!("root package '{}' has no Fusion entry file", root_node.name)
    })?;
    let identity = fingerprint::fingerprint_fields([graph.root.to_string_lossy().as_bytes()]);
    let package_dir = package_artifact_store_path(output_dir).join(identity);
    fs::create_dir_all(&package_dir).map_err(|error| format!("create entrypoint artifact directory: {error}"))?;
    let artifact = package_dir.join(format!("{root_fingerprint}.entry.ll"));
    let checksum_path = package_dir.join(format!("{root_fingerprint}.entry.sha256"));
    if artifact.is_file() {
        if let (Ok(expected), Ok(actual)) = (fs::read_to_string(&checksum_path), fingerprint::file_sha256(&artifact)) {
            if expected.trim() == actual { return Ok(artifact); }
        }
    }
    let temporary = package_dir.join(format!("{root_fingerprint}.entry.ll.tmp"));
    let mut command = Command::new(compiler);
    command.current_dir(&graph.root)
        .arg("emit-entrypoint-llvm")
        .arg(entry)
        .arg("-o")
        .arg(&temporary);
    command.args(package_root_arguments_for(graph, &graph.root)?);
    if !run(&mut command, &graph.root)? {
        let _ = fs::remove_file(&temporary);
        return Err("could not emit the application entrypoint/shared runtime LLVM module".into());
    }
    if !temporary.is_file() {
        return Err("compiler did not produce the application entrypoint LLVM module".into());
    }
    #[cfg(windows)]
    if artifact.exists() { fs::remove_file(&artifact).map_err(|error| error.to_string())?; }
    fs::rename(&temporary, &artifact).map_err(|error| format!("commit entrypoint LLVM artifact: {error}"))?;
    let checksum = fingerprint::file_sha256(&artifact).map_err(|error| format!("hash entrypoint LLVM artifact: {error}"))?;
    let checksum_tmp = checksum_path.with_extension("sha256.tmp");
    fs::write(&checksum_tmp, checksum).map_err(|error| format!("write entrypoint checksum: {error}"))?;
    #[cfg(windows)]
    if checksum_path.exists() { fs::remove_file(&checksum_path).map_err(|error| error.to_string())?; }
    fs::rename(&checksum_tmp, &checksum_path).map_err(|error| format!("commit entrypoint checksum: {error}"))?;
    Ok(artifact)
}

/// Canonical LLVM function signature used while checking package interfaces
/// against the actual package modules that Foundry is about to link.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LlvmFunctionSignature {
    result: String,
    parameters: Vec<String>,
}

impl LlvmFunctionSignature {
    fn display(&self) -> String {
        format!("{}({})", self.result, self.parameters.join(","))
    }
}

fn parse_llvm_function_header(line: &str) -> Result<(String, LlvmFunctionSignature), String> {
    let at = line.find('@').ok_or_else(|| format!("LLVM function header has no symbol: {line}"))?;
    let open = line[at..].find('(').map(|offset| at + offset)
        .ok_or_else(|| format!("LLVM function header has no parameter list: {line}"))?;
    let close = line[open..].find(')').map(|offset| open + offset)
        .ok_or_else(|| format!("LLVM function header has no closing parameter list: {line}"))?;
    let symbol = line[at + 1..open].trim().trim_matches('"').to_string();
    if symbol.is_empty() {
        return Err(format!("LLVM function header has an empty symbol: {line}"));
    }
    let result = line[..at].split_whitespace().last()
        .ok_or_else(|| format!("LLVM function header has no return type: {line}"))?.to_string();
    let parameters = line[open + 1..close].split(',').filter_map(|part| {
        let part = part.trim();
        if part.is_empty() { None } else { part.split_whitespace().next().map(str::to_string) }
    }).collect::<Vec<_>>();
    Ok((symbol, LlvmFunctionSignature { result, parameters }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageAbiExport {
    signature: LlvmFunctionSignature,
    public: bool,
}

fn read_package_abi_exports(interface_path: &Path) -> Result<BTreeMap<String, PackageAbiExport>, String> {
    let contents = fs::read_to_string(interface_path)
        .map_err(|error| format!("read package ABI {}: {error}", interface_path.display()))?;
    if !contents.starts_with("FUSION-PACKAGE-INTERFACE\t6\n") {
        return Err(format!("package ABI {} has an unsupported interface version", interface_path.display()));
    }
    let mut exports = BTreeMap::new();
    for (index, line) in contents.lines().enumerate().skip(1) {
        let fields = line.split('\t').collect::<Vec<_>>();
        let public = match fields.first().copied() {
            Some("abi-function") => true,
            Some("abi-internal-function") => false,
            _ => continue,
        };
        if fields.len() != 6 {
            return Err(format!("malformed ABI function record at {}:{}", interface_path.display(), index + 1));
        }
        let symbol = fields[3];
        if symbol.is_empty() || !symbol.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$')) {
            return Err(format!("invalid ABI symbol '{}' in {}", symbol, interface_path.display()));
        }
        let valid_type = |ty: &str| matches!(ty, "i64" | "double" | "i1" | "ptr" | "void");
        if !valid_type(fields[4]) {
            return Err(format!("unsupported ABI return type '{}' for '{}'", fields[4], symbol));
        }
        let parameters = if fields[5].is_empty() { Vec::new() } else {
            fields[5].split(',').map(|ty| {
                if valid_type(ty) && ty != "void" { Ok(ty.to_string()) }
                else { Err(format!("unsupported ABI parameter type '{}' for '{}'", ty, symbol)) }
            }).collect::<Result<Vec<_>, _>>()?
        };
        let export = PackageAbiExport {
            signature: LlvmFunctionSignature { result: fields[4].to_string(), parameters },
            public,
        };
        if let Some(previous) = exports.insert(symbol.to_string(), export.clone()) {
            if previous != export {
                return Err(format!("package interface declares conflicting ABI signatures for '{}'", symbol));
            }
        }
    }
    Ok(exports)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageAbiData {
    llvm_type: String,
    public: bool,
}

fn read_package_abi_data(interface_path: &Path) -> Result<BTreeMap<String, PackageAbiData>, String> {
    let contents = fs::read_to_string(interface_path)
        .map_err(|error| format!("read package data ABI {}: {error}", interface_path.display()))?;
    if !contents.starts_with("FUSION-PACKAGE-INTERFACE\t6\n") {
        return Err(format!("package data ABI {} has an unsupported interface version", interface_path.display()));
    }
    validate_package_interface(&contents, &interface_path.display().to_string())?;
    let mut data = BTreeMap::new();
    for (index, line) in contents.lines().enumerate().skip(1) {
        let fields = line.split('\t').collect::<Vec<_>>();
        let public = match fields.first().copied() {
            Some("abi-data") => true,
            Some("abi-internal-data") => false,
            _ => continue,
        };
        if fields.len() != 7 || fields[3].is_empty() || !fields[3].starts_with("_F") {
            return Err(format!("malformed package data ABI record at {}:{}", interface_path.display(), index + 1));
        }
        if !matches!(fields[4], "i64" | "double" | "i1" | "ptr") {
            return Err(format!("unsupported package data type '{}' for '{}'", fields[4], fields[3]));
        }
        let record = PackageAbiData { llvm_type: fields[4].to_string(), public };
        if let Some(previous) = data.insert(fields[3].to_string(), record.clone()) {
            if previous != record {
                return Err(format!("conflicting package data ABI records for '{}'", fields[3]));
            }
        }
    }
    Ok(data)
}

fn parse_llvm_global_record(line: &str) -> Result<(String, String, bool), String> {
    let (symbol, rhs) = line.split_once(" = ")
        .ok_or_else(|| format!("malformed LLVM global record: {line}"))?;
    let symbol = symbol.trim().trim_start_matches('@').trim_matches('"').to_string();
    if symbol.is_empty() { return Err(format!("LLVM global has an empty symbol: {line}")); }
    let words = rhs.split_whitespace().collect::<Vec<_>>();
    let (is_external, storage_index) = if words.first() == Some(&"external") { (true, 1) } else { (false, 0) };
    let storage = words.get(storage_index).copied().unwrap_or("");
    if storage != "constant" {
        return Err(format!("package data ABI symbol '{}' must be an immutable LLVM constant", symbol));
    }
    let llvm_type = words.get(storage_index + 1).copied()
        .ok_or_else(|| format!("LLVM global '{}' has no type", symbol))?.to_string();
    Ok((symbol, llvm_type, is_external))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageAbiStruct {
    llvm_name: String,
    fields: Vec<String>,
}

fn read_package_abi_structs(interface_path: &Path) -> Result<BTreeMap<String, PackageAbiStruct>, String> {
    let contents = fs::read_to_string(interface_path)
        .map_err(|error| format!("read package struct ABI {}: {error}", interface_path.display()))?;
    if !contents.starts_with("FUSION-PACKAGE-INTERFACE\t6\n") {
        return Err(format!("package struct ABI {} has an unsupported interface version", interface_path.display()));
    }
    validate_package_interface(&contents, &interface_path.display().to_string())?;
    let mut result = BTreeMap::new();
    for (line_number, line) in contents.lines().enumerate().skip(1) {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.first().copied() != Some("abi-struct") { continue; }
        if fields.len() != 7 {
            return Err(format!("malformed struct ABI record at {}:{}", interface_path.display(), line_number + 1));
        }
        let storage_fields = if fields[6].is_empty() { Vec::new() } else {
            fields[6].split(',').map(|entry| {
                let (_, ty) = entry.split_once(':').ok_or_else(|| format!("malformed struct field in {}:{}", interface_path.display(), line_number + 1))?;
                if !matches!(ty, "i64" | "double" | "i1" | "ptr") {
                    return Err(format!("struct ABI '{}' contains a field layout not yet supported for linking: {}", fields[2], ty));
                }
                Ok(ty.to_string())
            }).collect::<Result<Vec<_>, String>>()?
        };
        result.insert(fields[3].to_string(), PackageAbiStruct { llvm_name: fields[3].to_string(), fields: storage_fields });
    }
    Ok(result)
}

fn parse_llvm_named_struct(line: &str) -> Result<(String, Vec<String>), String> {
    let (name, rhs) = line.split_once(" = type ")
        .ok_or_else(|| format!("malformed LLVM identified type: {line}"))?;
    let name = name.trim().to_string();
    let body = rhs.trim();
    let body = body.strip_prefix('{').and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| format!("LLVM type '{}' is not a concrete struct layout", name))?;
    let fields = if body.trim().is_empty() { Vec::new() } else {
        body.split(',').map(|field| {
            let ty = field.trim();
            if !matches!(ty, "i64" | "i32" | "double" | "i1" | "ptr") {
                return Err(format!("LLVM type '{}' uses unsupported field storage '{}'; package ABI currently supports scalar and pointer fields", name, ty));
            }
            Ok(ty.to_string())
        }).collect::<Result<Vec<_>, String>>()?
    };
    Ok((name, fields))
}

fn validate_package_struct_layouts(
    package_units: &[(PathBuf, PathBuf)],
    entrypoint: &Path,
) -> Result<(), String> {
    let mut actual_layouts = BTreeMap::<String, (Vec<String>, PathBuf)>::new();
    let mut unit_contents = Vec::new();
    for (ir_path, interface_path) in package_units {
        let ir = fs::read_to_string(ir_path)
            .map_err(|error| format!("read package LLVM module {}: {error}", ir_path.display()))?;
        for line in ir.lines().map(str::trim).filter(|line| (line.starts_with("%fusion.type.") || line.starts_with("%fusion.variant.")) && line.contains(" = type ")) {
            let (name, fields) = parse_llvm_named_struct(line)?;
            if let Some((previous, previous_path)) = actual_layouts.insert(name.clone(), (fields.clone(), ir_path.clone())) {
                if previous != fields {
                    return Err(format!("identified type '{}' has conflicting layouts in {} and {}", name, previous_path.display(), ir_path.display()));
                }
            }
        }
        unit_contents.push((ir_path.clone(), interface_path.clone(), ir));
    }
    let entry_ir = fs::read_to_string(entrypoint)
        .map_err(|error| format!("read application entrypoint {}: {error}", entrypoint.display()))?;
    for line in entry_ir.lines().map(str::trim).filter(|line| (line.starts_with("%fusion.type.") || line.starts_with("%fusion.variant.")) && line.contains(" = type ")) {
        let (name, fields) = parse_llvm_named_struct(line)?;
        if let Some((previous, previous_path)) = actual_layouts.insert(name.clone(), (fields.clone(), entrypoint.to_path_buf())) {
            if previous != fields {
                return Err(format!("identified type '{}' has conflicting layouts in {} and {}", name, previous_path.display(), entrypoint.display()));
            }
        }
    }
    for (ir_path, interface_path, ir) in unit_contents {
        let structs = read_package_abi_structs(&interface_path)?;
        for (symbol, expected) in structs {
            let line = ir.lines().map(str::trim).find(|line| line.starts_with(&format!("{} = type ", symbol)))
                .ok_or_else(|| format!("package interface exports struct '{}' but {} does not emit its identified LLVM layout", symbol, ir_path.display()))?;
            let (actual_name, actual_fields) = parse_llvm_named_struct(line)?;
            if actual_name != expected.llvm_name || actual_fields != expected.fields {
                return Err(format!("struct layout ABI mismatch for '{}': interface declares {:?}, LLVM emits {:?}", symbol, expected.fields, actual_fields));
            }
            if let Some((known, _)) = actual_layouts.get(&symbol) {
                if known != &expected.fields {
                    return Err(format!("struct layout ABI '{}' differs from its package interface", symbol));
                }
            }
        }
        // The enum ABI is deliberately fixed to { i32 tag, ptr payload }.
        // Validate the concrete identified LLVM type emitted by each provider,
        // rather than trusting only the textual interface fingerprint.
        let interface = fs::read_to_string(&interface_path)
            .map_err(|error| format!("read package enum ABI {}: {error}", interface_path.display()))?;
        for record in interface.lines().skip(1) {
            let fields = record.split('\t').collect::<Vec<_>>();
            if fields.first().copied() != Some("abi-enum") { continue; }
            if fields.len() != 7 {
                return Err(format!("malformed enum ABI record in {}", interface_path.display()));
            }
            let symbol = fields[3];
            let line = ir.lines().map(str::trim).find(|line| line.starts_with(&format!("{} = type ", symbol)))
                .ok_or_else(|| format!("package interface exports enum '{}' but {} does not emit its identified LLVM layout", symbol, ir_path.display()))?;
            let (actual_name, actual_fields) = parse_llvm_named_struct(line)?;
            if actual_name != symbol || actual_fields != vec!["i32".to_string(), "ptr".to_string()] {
                return Err(format!("enum layout ABI mismatch for '{}': expected {{ i32, ptr }}, LLVM emits {:?}", symbol, actual_fields));
            }
        }
        // Validate every concrete variant payload type against the exported
        // payload metadata. A matching enum header alone is insufficient:
        // mismatched payload storage would make field loads unsafe.
        for record in interface.lines().skip(1) {
            let fields = record.split('\t').collect::<Vec<_>>();
            if fields.first().copied() != Some("abi-enum-payload") { continue; }
            if fields.len() != 8 { return Err(format!("malformed enum payload ABI record in {}", interface_path.display())); }
            let symbol = fields[4];
            let line = ir.lines().map(str::trim).find(|line| line.starts_with(&format!("{} = type ", symbol)))
                .ok_or_else(|| format!("enum payload '{}' is not emitted by {}", symbol, ir_path.display()))?;
            let (actual_name, actual_fields) = parse_llvm_named_struct(line)?;
            let expected_fields = if fields[7].is_empty() { Vec::new() } else { fields[7].split(',').map(str::to_string).collect() };
            if actual_name != symbol || actual_fields != expected_fields {
                return Err(format!("enum payload layout ABI mismatch for '{}': interface declares {:?}, LLVM emits {:?}", symbol, expected_fields, actual_fields));
            }
        }
    }
    Ok(())
}

/// Validate the package interfaces against every package-scoped LLVM unit and
/// the application entrypoint before invoking Clang. This catches missing,
/// duplicate, private/unsupported, and signature-drifted cross-package symbols
/// before the native linker reports a lower-level error.
fn validate_package_link_inputs(
    package_units: &[(PathBuf, PathBuf)],
    entrypoint: &Path,
    root_package_interface: &Path,
) -> Result<(), String> {
    validate_package_struct_layouts(package_units, entrypoint)?;
    let mut providers = BTreeMap::<String, (PathBuf, PackageAbiExport)>::new();
    let mut definitions = BTreeMap::<String, (PathBuf, LlvmFunctionSignature)>::new();
    let mut declarations = Vec::<(PathBuf, String, LlvmFunctionSignature)>::new();
    let mut data_providers = BTreeMap::<String, (PathBuf, PathBuf, PackageAbiData)>::new();
    let mut data_declarations = Vec::<(PathBuf, String, String)>::new();
    let mut data_definitions = BTreeMap::<String, (PathBuf, String)>::new();
    for (ir_path, interface_path) in package_units {
        let exports = read_package_abi_exports(interface_path)?;
        let data_exports = read_package_abi_data(interface_path)?;
        let ir = fs::read_to_string(ir_path)
            .map_err(|error| format!("read package LLVM module {}: {error}", ir_path.display()))?;
        let mut unit_definitions = BTreeMap::new();
        for line in ir.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("@_F__fusion_m") && trimmed.contains(" = ") {
                let (symbol, llvm_type, external) = parse_llvm_global_record(trimmed)?;
                if symbol.starts_with("_F__fusion_m") {
                    if external {
                        let reference = format!("@{}", symbol);
                        let used = ir.lines().any(|candidate| {
                            let candidate = candidate.trim_start();
                            !candidate.starts_with('@') && candidate.contains(&reference)
                        });
                        if used { data_declarations.push((ir_path.clone(), symbol, llvm_type)); }
                    } else if let Some(previous) = data_definitions.insert(symbol.clone(), (ir_path.clone(), llvm_type.clone())) {
                        return Err(format!("duplicate package data definition '{}' in {} and {}", symbol, previous.0.display(), ir_path.display()));
                    }
                }
            }
            if trimmed.starts_with("define ") {
                let (symbol, signature) = parse_llvm_function_header(trimmed)?;
                if let Some(previous) = definitions.insert(symbol.clone(), (ir_path.clone(), signature.clone())) {
                    return Err(format!("duplicate LLVM definition '{}' in {} and {}", symbol, previous.0.display(), ir_path.display()));
                }
                unit_definitions.insert(symbol, signature);
            } else if trimmed.starts_with("declare ") {
                let (symbol, signature) = parse_llvm_function_header(trimmed)?;
                let reference = format!("@{}(", symbol);
                if ir.lines().any(|candidate| {
                    let candidate = candidate.trim_start();
                    !candidate.starts_with("declare ") && !candidate.starts_with("define ") && candidate.contains(&reference)
                }) {
                    declarations.push((ir_path.clone(), symbol, signature));
                }
            }
        }
        for (symbol, expected) in exports {
            let Some(actual) = unit_definitions.get(&symbol) else {
                return Err(format!("package ABI exports '{}' but {} does not define it", symbol, ir_path.display()));
            };
            if actual != &expected.signature {
                return Err(format!("package ABI mismatch for '{}': interface says {}, LLVM defines {}", symbol, expected.signature.display(), actual.display()));
            }
            if let Some(previous) = providers.insert(symbol.clone(), (interface_path.clone(), expected.clone())) {
                return Err(format!("duplicate package ABI provider '{}' in {} and {}", symbol, previous.0.display(), ir_path.display()));
            }
        }
        for (symbol, expected) in data_exports {
            let Some((provider_ir, actual_type)) = data_definitions.get(&symbol) else {
                return Err(format!("package data ABI exports '{}' but {} does not define it", symbol, ir_path.display()));
            };
            if actual_type != &expected.llvm_type {
                return Err(format!("package data ABI mismatch for '{}': interface says {}, LLVM defines {}", symbol, expected.llvm_type, actual_type));
            }
            if let Some(previous) = data_providers.insert(symbol.clone(), (provider_ir.clone(), interface_path.clone(), expected.clone())) {
                return Err(format!("duplicate package data ABI provider '{}' in {} and {}", symbol, previous.1.display(), interface_path.display()));
            }
        }
    }

    // The entrypoint can also contain package declarations, and runtime
    // declarations are intentionally excluded because the entrypoint owns them.
    let entry_ir = fs::read_to_string(entrypoint)
        .map_err(|error| format!("read application entrypoint LLVM module {}: {error}", entrypoint.display()))?;
    for line in entry_ir.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("@_F__fusion_m") && trimmed.contains(" = ") {
            let (symbol, llvm_type, external) = parse_llvm_global_record(trimmed)?;
            if symbol.starts_with("_F__fusion_m") {
                if external {
                    let reference = format!("@{}", symbol);
                    let used = entry_ir.lines().any(|candidate| {
                        let candidate = candidate.trim_start();
                        !candidate.starts_with('@') && candidate.contains(&reference)
                    });
                    if used { data_declarations.push((entrypoint.to_path_buf(), symbol, llvm_type)); }
                } else if let Some(previous) = data_definitions.insert(symbol.clone(), (entrypoint.to_path_buf(), llvm_type.clone())) {
                    return Err(format!("duplicate package data definition '{}' in {} and {}", symbol, previous.0.display(), entrypoint.display()));
                }
            }
        }
        if trimmed.starts_with("define ") {
            let (symbol, signature) = parse_llvm_function_header(trimmed)?;
            if let Some(previous) = definitions.insert(symbol.clone(), (entrypoint.to_path_buf(), signature)) {
                return Err(format!("duplicate LLVM definition '{}' in {} and {}", symbol, previous.0.display(), entrypoint.display()));
            }
        } else if trimmed.starts_with("declare ") {
            let (symbol, signature) = parse_llvm_function_header(trimmed)?;
            let reference = format!("@{}(", symbol);
            if entry_ir.lines().any(|candidate| {
                let candidate = candidate.trim_start();
                !candidate.starts_with("declare ") && !candidate.starts_with("define ") && candidate.contains(&reference)
            }) {
                declarations.push((entrypoint.to_path_buf(), symbol, signature));
            }
        }
    }

    for (consumer, symbol, actual) in declarations {
        if !symbol.starts_with("_F") { continue; }
        // A declaration satisfied by a definition in the same unit is a legal
        // internal forward reference and does not cross a package boundary.
        if definitions.get(&symbol).map(|(owner, _)| owner == &consumer).unwrap_or(false) {
            continue;
        }
        let Some((provider_path, expected)) = providers.get(&symbol) else {
            return Err(format!("unresolved Fusion package symbol '{}' declared by {}: no package exports a supported ABI for it", symbol, consumer.display()));
        };
        if expected.signature != actual {
            return Err(format!("cross-package ABI mismatch for '{}': {} declares {}, but provider {} exports {}", symbol, consumer.display(), actual.display(), provider_path.display(), expected.signature.display()));
        }
        if !expected.public && !(consumer == entrypoint && provider_path == root_package_interface) {
            return Err(format!("private package symbol '{}' is referenced outside its owning package", symbol));
        }
    }
    for (consumer, symbol, actual_type) in data_declarations {
        let Some((provider_ir, provider_interface, expected)) = data_providers.get(&symbol) else {
            return Err(format!("unresolved Fusion package data '{}' declared by {}: no package exports a supported data ABI for it", symbol, consumer.display()));
        };
        if expected.llvm_type != actual_type {
            return Err(format!("cross-package data ABI mismatch for '{}': {} declares {}, but provider {} exports {}", symbol, consumer.display(), actual_type, provider_ir.display(), expected.llvm_type));
        }
        if !expected.public && !(consumer == entrypoint && provider_interface == root_package_interface) && consumer.as_path() != provider_ir.as_path() {
            return Err(format!("private package data '{}' is referenced outside its owning package", symbol));
        }
    }
    Ok(())
}

fn read_package_graph_state(path: &Path) -> Option<package_graph::PackageGraph> {
    let contents = fs::read_to_string(path).ok()?;
    let snapshot = toml::from_str::<package_graph::PackageGraphSnapshot>(&contents).ok()?;
    Some(package_graph::PackageGraph::from_snapshot(snapshot))
}

fn write_package_graph_state(
    path: &Path,
    graph: &package_graph::PackageGraph,
) -> Result<(), String> {
    let contents = toml::to_string(&graph.snapshot())
        .map_err(|error| format!("serialize package graph state: {error}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("create package graph state directory: {error}"))?;
    }
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, contents).map_err(|error| format!("write package graph state: {error}"))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("replace package graph state: {error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("commit package graph state: {error}"))?;
    Ok(())
}

fn persist_successful_package_state(
    fingerprint_path: &Path,
    graph_path: &Path,
    fingerprints: &BTreeMap<PathBuf, String>,
    graph: &package_graph::PackageGraph,
    artifacts: &BTreeMap<PathBuf, PathBuf>,
) -> Result<(), String> {
    // Commit validated package records first, then graph state, and the package
    // fingerprint last. The executable stamp is written by the caller after this
    // function returns, so neither success marker can bless a partial cache update.
    let removed = persist_package_artifact_records(
        fingerprint_path.parent().unwrap_or_else(|| Path::new(".")),
        graph,
        fingerprints,
        artifacts,
    )?;
    write_package_graph_state(graph_path, graph)?;
    write_package_fingerprint_state(fingerprint_path, fingerprints)?;
    if removed > 0 {
        println!("Foundry: removed {removed} stale package cache record(s)");
    }
    Ok(())
}

fn read_package_fingerprint_state(path: &Path) -> BTreeMap<PathBuf, String> {
    let Ok(contents) = fs::read_to_string(path) else { return BTreeMap::new() };
    let Ok(value) = contents.parse::<toml::Value>() else { return BTreeMap::new() };
    let Some(entries) = value.get("package").and_then(toml::Value::as_array) else {
        return BTreeMap::new();
    };
    entries.iter().filter_map(|entry| {
        let root = entry.get("root")?.as_str()?;
        let fingerprint = entry.get("fingerprint")?.as_str()?;
        Some((PathBuf::from(root), fingerprint.to_owned()))
    }).collect()
}

fn plan_package_build(
    current: &BTreeMap<PathBuf, String>,
    previous: &BTreeMap<PathBuf, String>,
) -> PackageBuildPlan {
    let mut plan = PackageBuildPlan::default();
    for (root, fingerprint) in current {
        if previous.get(root) == Some(fingerprint) {
            plan.unchanged.push(root.clone());
        } else {
            plan.changed.push(root.clone());
        }
    }
    for root in previous.keys() {
        if !current.contains_key(root) { plan.removed.push(root.clone()); }
    }
    plan
}

fn write_package_fingerprint_state(
    path: &Path,
    fingerprints: &BTreeMap<PathBuf, String>,
) -> Result<(), String> {
    let mut contents = String::from("# Generated by Foundry after a successful build.\n");
    for (root, fingerprint) in fingerprints {
        contents.push_str("[[package]]\nroot = ");
        contents.push_str(&toml::Value::String(root.to_string_lossy().into_owned()).to_string());
        contents.push_str("\nfingerprint = ");
        contents.push_str(&toml::Value::String(fingerprint.clone()).to_string());
        contents.push_str("\n\n");
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("create package state directory: {error}"))?;
    }
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, contents).map_err(|error| format!("write package fingerprint state: {error}"))?;
    // Windows does not replace an existing destination with `rename`.
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("replace package fingerprint state: {error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("commit package fingerprint state: {error}"))?;
    Ok(())
}

/// Compare complete provenance documents semantically. TOML key ordering and
/// whitespace are irrelevant, but every recorded identity and ordered input must match.
fn provenance_documents_match(expected: &str, actual: &str) -> bool {
    match (toml::from_str::<toml::Value>(expected), toml::from_str::<toml::Value>(actual)) {
        (Ok(expected), Ok(actual)) => expected == actual,
        _ => false,
    }
}

/// Recognize a valid local build and repair its package state if that state file
/// was removed or lost. The executable fingerprint is the commit marker: package
/// state is only rewritten when the artifact, matching stamp, and matching
/// provenance record all exist.
fn restore_fresh_build_state(
    artifact: &Path,
    stamp: &Path,
    expected_fingerprint: &str,
    package_state: &Path,
    package_fingerprints: &BTreeMap<PathBuf, String>,
    graph_state: &Path,
    graph: &package_graph::PackageGraph,
    artifacts: &BTreeMap<PathBuf, PathBuf>,
    expected_provenance: Option<&str>,
) -> Result<bool, String> {
    if !artifact.is_file()
        || fs::read_to_string(stamp).ok().as_deref() != Some(expected_fingerprint)
    {
        return Ok(false);
    }
    // A matching stamp without matching provenance is not a trustworthy fresh
    // build. This also detects an executable replaced outside Foundry.
    let provenance_path = stamp.parent().unwrap_or_else(|| Path::new(".")).join(".foundry-build-provenance.toml");
    let provenance = match fs::read_to_string(&provenance_path)
        .ok()
        .and_then(|contents| toml::from_str::<toml::Value>(&contents).ok()) {
        Some(value) => value,
        None => return Ok(false),
    };
    if provenance.get("format_version").and_then(toml::Value::as_integer) != Some(5)
        || provenance.get("build_fingerprint").and_then(toml::Value::as_str) != Some(expected_fingerprint)
        || provenance.get("executable_sha256").and_then(toml::Value::as_str)
            != fingerprint::file_sha256(artifact).ok().as_deref()
        || expected_provenance.map_or(false, |expected| !provenance_documents_match(expected, &fs::read_to_string(&provenance_path).unwrap_or_default()))
    {
        return Ok(false);
    }
    persist_successful_package_state(package_state, graph_state, package_fingerprints, graph, artifacts)?;
    Ok(true)
}

fn link_staging_path(output: &Path) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = output.file_name().unwrap_or_else(|| OsStr::new("application")).to_string_lossy();
    output.with_file_name(format!(".{file_name}.foundry-link-{}-{nonce}.tmp", std::process::id()))
}

fn write_text_atomically(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = path.file_name().unwrap_or_else(|| OsStr::new("foundry-state" )).to_string_lossy();
    let temporary = path.with_file_name(format!(".{file_name}.{}-{nonce}.tmp", std::process::id()));
    fs::write(&temporary, contents)?;
    #[cfg(windows)]
    if path.exists() {
        let backup = path.with_file_name(format!(".{file_name}.{}-{nonce}.bak", std::process::id()));
        if let Err(error) = fs::rename(path, &backup) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        return match fs::rename(&temporary, path) {
            Ok(()) => {
                let _ = fs::remove_file(backup);
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&backup, path);
                let _ = fs::remove_file(&temporary);
                Err(error)
            }
        };
    }
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn replace_linked_output(staged: &Path, output: &Path) -> Result<(), String> {
    #[cfg(windows)]
    if output.exists() {
        let backup = output.with_extension("foundry-backup");
        let _ = fs::remove_file(&backup);
        fs::rename(output, &backup)
            .map_err(|error| format!("preserve previous executable {}: {error}", output.display()))?;
        match fs::rename(staged, output) {
            Ok(()) => { let _ = fs::remove_file(backup); return Ok(()); }
            Err(error) => {
                let _ = fs::rename(&backup, output);
                return Err(format!("publish executable {}: {error}", output.display()));
            }
        }
    }
    fs::rename(staged, output)
        .map_err(|error| format!("publish executable {}: {error}", output.display()))
}

fn build_provenance_contents(
    compiler: &Path,
    linker: &LinkerIdentity,
    profile: BuildProfile,
    build_fingerprint: &str,
    executable: &Path,
    entrypoint_artifact: &Path,
    graph: &package_graph::PackageGraph,
    package_keys: &BTreeMap<PathBuf, String>,
    package_artifacts: &BTreeMap<PathBuf, PathBuf>,
) -> Result<String, String> {
    let resolved_compiler = compiler.canonicalize().unwrap_or_else(|_| compiler.to_path_buf());
    let compiler_sha256 = fingerprint::file_sha256(&resolved_compiler)
        .map_err(|error| format!("hash Fusion compiler {}: {error}", resolved_compiler.display()))?;
    let (version_text, version_bytes) = match Command::new(compiler).arg("--version").output() {
        Ok(version) => {
            let mut bytes = version.stdout; bytes.extend_from_slice(&version.stderr);
            let text = if version.status.success() { String::from_utf8_lossy(&bytes).trim().to_string() }
                else { format!("<version-command-failed:{}>", version.status) };
            (text, bytes)
        }
        Err(error) => (format!("<version-command-unavailable:{error}>"), Vec::new()),
    };
    let version_sha256 = fingerprint::fingerprint_fields([version_bytes.as_slice()]);
    let linker_path_text = linker.path.clone();
    let linker_sha256 = linker.binary_sha256.clone();
    let linker_version_text = format!("{}: {}", linker.version_status, linker.version_text);
    let linker_version_sha256 = linker.version_sha256.clone();
    let linker_target_status = linker.target_status.clone();
    let linker_target_triple = linker.target_triple.clone();
    let linker_resource_status = linker.resource_status.clone();
    let linker_resource_dir = linker.resource_dir.clone();
    let linker_resource_sha256 = linker.resource_sha256.clone();
    let linker_search_dirs_status = linker.search_dirs_status.clone();
    let linker_search_dirs = linker.search_dirs.clone();
    let linker_search_dirs_sha256 = linker.search_dirs_sha256.clone();
    let linker_environment = linker.environment.clone();
    let executable_sha256 = fingerprint::file_sha256(executable)
        .map_err(|error| format!("hash linked executable {}: {error}", executable.display()))?;

    // Preserve the exact command-line order: entrypoint first, followed by package
    // modules in dependency-first graph order. Hashes make the link inputs auditable.
    let mut ordered_link_inputs = Vec::<String>::new();
    let entrypoint_sha256 = fingerprint::file_sha256(entrypoint_artifact)
        .map_err(|error| format!("hash entrypoint LLVM artifact {}: {error}", entrypoint_artifact.display()))?;
    ordered_link_inputs.push(format!("0:entrypoint={entrypoint_sha256}"));
    for (index, package_root) in graph.build_order.iter().enumerate() {
        let node = graph.nodes.get(package_root).ok_or_else(|| format!("missing package node {} while writing provenance", package_root.display()))?;
        let artifact = package_artifacts.get(package_root).ok_or_else(|| format!("missing package artifact for {} while writing provenance", package_root.display()))?;
        let hash = fingerprint::file_sha256(artifact).map_err(|error| format!("hash package artifact {}: {error}", artifact.display()))?;
        ordered_link_inputs.push(format!("{}:package:{}={hash}", index + 1, node.name));
    }
    let link_input_refs = ordered_link_inputs.iter().map(|item| item.as_bytes()).collect::<Vec<_>>();
    let link_inputs_sha256 = fingerprint::fingerprint_fields(link_input_refs);

    let mut contents = String::from("format_version = 5\n");
    contents.push_str(&format!("build_fingerprint = {}\n", toml::Value::String(build_fingerprint.to_owned())));
    contents.push_str(&format!("compiler_path = {}\n", toml::Value::String(resolved_compiler.to_string_lossy().into_owned())));
    contents.push_str(&format!("compiler_sha256 = {}\n", toml::Value::String(compiler_sha256)));
    contents.push_str(&format!("compiler_version = {}\n", toml::Value::String(version_text)));
    contents.push_str(&format!("compiler_version_sha256 = {}\n", toml::Value::String(version_sha256)));
    contents.push_str(&format!("linker_path = {}\n", toml::Value::String(linker_path_text)));
    contents.push_str(&format!("linker_sha256 = {}\n", toml::Value::String(linker_sha256)));
    contents.push_str(&format!("linker_version = {}\n", toml::Value::String(linker_version_text)));
    contents.push_str(&format!("linker_version_sha256 = {}\n", toml::Value::String(linker_version_sha256)));
    contents.push_str(&format!("linker_target_status = {}\n", toml::Value::String(linker_target_status)));
    contents.push_str(&format!("linker_target_triple = {}\n", toml::Value::String(linker_target_triple)));
    contents.push_str(&format!("linker_resource_status = {}\n", toml::Value::String(linker_resource_status)));
    contents.push_str(&format!("linker_resource_dir = {}\n", toml::Value::String(linker_resource_dir)));
    contents.push_str(&format!("linker_resource_sha256 = {}\n", toml::Value::String(linker_resource_sha256)));
    contents.push_str(&format!("linker_search_dirs_status = {}\n", toml::Value::String(linker_search_dirs_status)));
    contents.push_str(&format!("linker_search_dirs = {}\n", toml::Value::String(linker_search_dirs)));
    contents.push_str(&format!("linker_search_dirs_sha256 = {}\n", toml::Value::String(linker_search_dirs_sha256)));
    contents.push_str("linker_environment = [\n");
    for (name, value) in &linker_environment {
        contents.push_str(&format!("  {},\n", toml::Value::String(format!("{name}={value}"))));
    }
    contents.push_str("]\nlinker_flags = [\n");
    for flag in linker_config::FLAGS { contents.push_str(&format!("  {},\n", toml::Value::String((*flag).to_string()))); }
    contents.push_str("]\n");
    contents.push_str(&format!("link_inputs_sha256 = {}\n", toml::Value::String(link_inputs_sha256)));
    contents.push_str(&format!("profile = {}\n", toml::Value::String(profile.name().into())));
    contents.push_str("package_interface_version = 6\n");
    contents.push_str(&format!("executable_sha256 = {}\n", toml::Value::String(executable_sha256)));
    contents.push_str(&format!("entrypoint_llvm_sha256 = {}\n", toml::Value::String(entrypoint_sha256)));
    contents.push_str("link_inputs = [\n");
    for item in &ordered_link_inputs { contents.push_str(&format!("  {},\n", toml::Value::String(item.clone()))); }
    contents.push_str("]\npackage_fingerprints = [\n");
    for root in &graph.build_order {
        let node = graph.nodes.get(root).ok_or_else(|| format!("missing package node {} while writing provenance", root.display()))?;
        let key = package_keys.get(root).ok_or_else(|| format!("missing package fingerprint for {} while writing provenance", root.display()))?;
        contents.push_str(&format!("  {},\n", toml::Value::String(format!("{}={}", node.name, key))));
    }
    contents.push_str("]\npackage_lockfiles = [\n");
    for root in &graph.build_order {
        let node = graph.nodes.get(root).ok_or_else(|| format!("missing package node {} while writing lockfile provenance", root.display()))?;
        let lock_path = root.join(LOCKFILE);
        let lock_digest = if lock_path.is_file() {
            fingerprint::file_sha256(&lock_path)
                .map_err(|error| format!("hash package lockfile {} for provenance: {error}", lock_path.display()))?
        } else {
            fingerprint::fingerprint_fields([b"<no-lockfile>".as_slice()])
        };
        contents.push_str(&format!("  {},\n", toml::Value::String(format!("{}={lock_digest}", node.name))));
    }
    contents.push_str("]\npackage_artifacts = [\n");
    for root in &graph.build_order {
        let node = graph.nodes.get(root).ok_or_else(|| format!("missing package node {} while writing provenance", root.display()))?;
        let artifact = package_artifacts.get(root).ok_or_else(|| format!("missing package artifact for {} while writing provenance", root.display()))?;
        let hash = fingerprint::file_sha256(artifact).map_err(|error| format!("hash package artifact {}: {error}", artifact.display()))?;
        contents.push_str(&format!("  {},\n", toml::Value::String(format!("{}={hash}", node.name))));
    }
    contents.push_str("]\n");
    Ok(contents)
}

/// Collect C translation units from the project's `c/` directory (recursive).
fn foundry_c_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("read foreign source directory {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read directory entry: {e}"))?;
            let path = entry.path();
            if path.is_dir() { walk(&path, out)?; }
            else if path.extension().and_then(OsStr::to_str).is_some_and(|ext| ext.eq_ignore_ascii_case("c")) { out.push(path); }
        }
        Ok(())
    }
    let dir = root.join("c");
    if !dir.is_dir() { return Ok(Vec::new()); }
    let mut sources = Vec::new();
    walk(&dir, &mut sources)?;
    sources.sort();
    Ok(sources)
}

fn c_compiler() -> Result<PathBuf, String> {
    if let Some(cc) = env::var_os("CC") {
        let path = PathBuf::from(cc);
        if path.is_file() { return Ok(path); }
        if let Some(found) = find_on_path(path.to_string_lossy().as_ref()) { return Ok(found); }
        return Err(format!("CC is set to '{}' but the C compiler was not found", path.display()));
    }
    let compiler_names: &[&str] = if cfg!(windows) {
        &["clang", "clang.exe", "gcc", "gcc.exe"]
    } else {
        &["clang", "gcc", "cc"]
    };
    for name in compiler_names {
        if let Some(path) = find_on_path(name) { return Ok(path); }
    }
    Err("C sources found in project 'c/' but no C compiler detected; install Clang or GCC, or set CC".into())
}

fn compile_project_c(root: &Path, sources: &[PathBuf], out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    if sources.is_empty() { return Ok(Vec::new()); }
    let compiler = c_compiler()?;
    let c_dir = root.join("c");
    let object_dir = out_dir.join("foreign-c");
    fs::create_dir_all(&object_dir).map_err(|e| format!("create C object directory: {e}"))?;
    let flags: Vec<String> = env::var("FOUNDRY_CFLAGS").unwrap_or_default().split_whitespace().map(str::to_owned).collect();
    let version = Command::new(&compiler).arg("--version").output().map(|output| {
        format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    }).unwrap_or_else(|_| "<compiler-version-unavailable>".to_string());
    let compiler_identity = fingerprint::fingerprint_fields([compiler.to_string_lossy().as_bytes(), version.as_bytes(), flags.join("\0").as_bytes()]);
    let mut objects = Vec::new();
    for (i, source) in sources.iter().enumerate() {
        let rel = source.strip_prefix(&c_dir).unwrap_or(source);
        let source_bytes = fs::read(source).map_err(|e| format!("read C source {}: {e}", source.display()))?;
        let identity = fingerprint::fingerprint_fields([rel.to_string_lossy().as_bytes(), source_bytes.as_slice(), compiler_identity.as_bytes()]);
        let object = object_dir.join(format!("{i}-{identity}.{}", if cfg!(windows) { "obj" } else { "o" }));
        if !object.is_file() {
            let temp = object.with_extension(if cfg!(windows) { "obj.tmp" } else { "o.tmp" });
            let mut cmd = Command::new(&compiler);
            cmd.args(&flags).arg("-c").arg(source).arg("-o").arg(&temp).current_dir(root);
            let success = cmd.status().map_err(|e| format!("could not start C compiler '{}': {e}", compiler.display()))?.success();
            if !success { let _ = fs::remove_file(&temp); return Err(format!("C compiler failed compiling {}", source.display())); }
            if !temp.is_file() { return Err(format!("C compiler reported success but did not create {}", temp.display())); }
            if object.exists() { fs::remove_file(&object).map_err(|e| format!("replace C object: {e}"))?; }
            fs::rename(&temp, &object).map_err(|e| format!("publish C object {}: {e}", object.display()))?;
        }
        objects.push(object);
    }
    Ok(objects)
}

fn build_inner(p: &Project, profile: BuildProfile, fusion: Option<&Path>, extra: &[String], resolve: bool) -> Result<PathBuf, String> {
    if resolve {
        resolve_dependencies(p, env::var_os("FOUNDRY_OFFLINE").is_some())?;
    }
    // Resolve the package graph before invoking the compiler. Foundry owns
    // package discovery and dependency ordering; the Fusion compiler remains
    // responsible only for source-level modules within the roots it is given.
    let package_graph = package_graph::resolve_package_graph(p).map_err(|e| e.to_string())?;
    let target = extra.windows(2).find(|w|w[0]=="--target").map(|w|w[1].clone());
    let mut dir = p.root.join("target").join(profile.name());
    if let Some(t)=&target { dir=dir.join(t); }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(if cfg!(windows) {
        format!("{}.exe", p.manifest.package.name)
    } else {
        p.manifest.package.name.clone()
    });
    let compiler = find_project_fusion(p, fusion)?;
    let c_sources = foundry_c_sources(&p.root)?;
    let mut fingerprint_inputs: Vec<Vec<u8>> = Vec::new();
    fingerprint_inputs.push(b"foundry-build-key-v3".to_vec());
    fingerprint_inputs.push(p.manifest.package.name.as_bytes().to_vec());
    fingerprint_inputs.push(p.manifest.package.version.as_bytes().to_vec());
    fingerprint_inputs.push(profile.name().as_bytes().to_vec());
    fingerprint_inputs.push(format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH).into_bytes());
    fingerprint_inputs.extend(compiler_identity_fields(&compiler));
    // Capture the effective linker toolchain once. The same immutable snapshot is
    // used by the build key and provenance, avoiding repeated resource-tree hashes.
    let linker_identity = linker_identity_for(resolve_linker_path().as_deref());
    fingerprint_inputs.extend(linker_identity.fingerprint_fields());
    let manifest_path = if p.root.join(MANIFEST).is_file() { p.root.join(MANIFEST) } else { p.root.join(LEGACY_MANIFEST) };
    fingerprint_inputs.push(fs::read(&manifest_path).map_err(|e| format!("read manifest for fingerprint: {e}"))?);
    fingerprint_inputs.push(format!("{:?}", p.manifest.dependencies).into_bytes());
    if let Ok(lock_bytes)=fs::read(p.root.join(LOCKFILE)) { fingerprint_inputs.push(lock_bytes); }
    for dependency in p.manifest.dependencies.values().chain(p.manifest.dev_dependencies.values()) {
        match dependency {
            Dependency::Path(path)=>{let full=p.root.join(path);fingerprint_inputs.push(directory_revision(&full).into_bytes());},
            Dependency::Git{url,rev}=>{fingerprint_inputs.push(url.as_bytes().to_vec());fingerprint_inputs.push(rev.clone().unwrap_or_default().into_bytes());},
            Dependency::Version(version)=>fingerprint_inputs.push(version.as_bytes().to_vec()),
        }
    }
    fingerprint_inputs.extend(extra.iter().map(|x| x.as_bytes().to_vec()));
    fingerprint_inputs.push(b"foundry-c-sources-v2".to_vec());
    if !c_sources.is_empty() {
        let c_compiler = c_compiler()?;
        fingerprint_inputs.push(c_compiler.to_string_lossy().as_bytes().to_vec());
        let c_version = Command::new(&c_compiler).arg("--version").output().map(|output| {
            format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
        }).unwrap_or_else(|_| "<compiler-version-unavailable>".to_string());
        fingerprint_inputs.push(c_version.into_bytes());
        fingerprint_inputs.push(env::var("FOUNDRY_CFLAGS").unwrap_or_default().into_bytes());
    }
    for source in &c_sources {
        fingerprint_inputs.push(source.strip_prefix(&p.root).unwrap_or(source).to_string_lossy().as_bytes().to_vec());
        fingerprint_inputs.push(fs::read(source).map_err(|e| format!("read C source {} for fingerprint: {e}", source.display()))?);
    }
    // Keep package-level keys in dependency-first order. Each package module is
    // independently cached, while the entrypoint/runtime key follows the root
    // package key so transitive ABI changes invalidate the final link inputs.
    let package_keys = package_build_fingerprints(&package_graph, &compiler, profile, extra)?;
    for package_root in &package_graph.build_order {
        let node = package_graph.nodes.get(package_root).ok_or_else(|| {
            format!("package graph is missing node {}", package_root.display())
        })?;
        fingerprint_inputs.push(node.name.as_bytes().to_vec());
        fingerprint_inputs.push(package_keys[package_root].as_bytes().to_vec());
    }
    let refs = fingerprint_inputs.iter().map(|x| x.as_slice()).collect::<Vec<_>>();
    let fingerprint = fingerprint::fingerprint_fields(refs);
    let package_state = package_state_path(&dir);
    let graph_state = package_graph_state_path(&dir);
    let previous_package_keys = read_package_fingerprint_state(&package_state);
    let package_plan = plan_package_build(&package_keys, &previous_package_keys);
    let graph_delta = read_package_graph_state(&graph_state)
        .map(|previous| package_graph.delta_from(&previous));
    if let Some(delta) = &graph_delta {
        if !delta.changed_packages.is_empty() || !delta.removed_packages.is_empty() {
            println!(
                "Foundry: package graph changed; {} package(s) invalidated by graph changes; {} removed",
                delta.rebuild_order.len(), delta.removed_packages.len()
            );
        }
    }
    if !package_plan.changed.is_empty() {
        println!("Foundry: {} package(s) changed; {} unchanged; {} removed", package_plan.changed.len(), package_plan.unchanged.len(), package_plan.removed.len());
    }
    let (package_artifacts, package_cache_hits) =
        compile_package_artifacts(&dir, &package_graph, &package_keys, &compiler)?;
    let entrypoint_artifact = compile_entrypoint_artifact(
        &dir, &package_graph, &package_keys[&package_graph.root], &compiler,
    )?;
    let package_link_units = package_graph.build_order.iter().map(|package_root| {
        let artifact = package_artifacts.get(package_root).ok_or_else(|| {
            format!("package artifact cache did not produce a module for {}", package_root.display())
        })?;
        let interface = artifact.with_extension("fpi");
        if !interface.is_file() {
            return Err(format!("package interface is missing for {}", package_root.display()));
        }
        Ok((artifact.clone(), interface))
    }).collect::<Result<Vec<_>, String>>()?;
    let root_package_interface = package_artifacts.get(&package_graph.root)
        .ok_or("root package LLVM artifact is missing")?.with_extension("fpi");
    validate_package_link_inputs(&package_link_units, &entrypoint_artifact, &root_package_interface)?;
    if package_cache_hits > 0 {
        println!("Foundry: reused {package_cache_hits}/{} compiled package LLVM artifact(s)", package_graph.nodes.len());
    }
    let stamp = dir.join(".foundry-fingerprint");
    let expected_local_provenance = if out.is_file() {
        Some(build_provenance_contents(&compiler, &linker_identity, profile, &fingerprint, &out, &entrypoint_artifact, &package_graph, &package_keys, &package_artifacts)?)
    } else { None };
    if restore_fresh_build_state(
        &out, &stamp, &fingerprint, &package_state, &package_keys,
        &graph_state, &package_graph, &package_artifacts, expected_local_provenance.as_deref(),
    )? {
        println!("Fresh {} (cached)", out.display());
        return Ok(out);
    }
    if let Ok(remote)=env::var("FOUNDRY_REMOTE_CACHE") {
        let remote_staged = link_staging_path(&out);
        if cache::restore(&remote,&fingerprint,&remote_staged)? {
            if !remote_staged.is_file() {
                let _ = fs::remove_file(&remote_staged);
                return Err("remote cache reported success without producing an executable".into());
            }
            let remote_provenance_path = link_staging_path(&out).with_extension("remote-provenance.toml");
            let remote_provenance_valid = if cache::restore_metadata(&remote, &fingerprint, &remote_provenance_path)? {
                let expected = build_provenance_contents(&compiler, &linker_identity, profile, &fingerprint, &remote_staged, &entrypoint_artifact, &package_graph, &package_keys, &package_artifacts)?;
                let expected_value = toml::from_str::<toml::Value>(&expected).ok();
                let remote_value = fs::read_to_string(&remote_provenance_path).ok().and_then(|text| toml::from_str::<toml::Value>(&text).ok());
                expected_value.is_some() && remote_value.is_some() && provenance_documents_match(&expected, &fs::read_to_string(&remote_provenance_path).unwrap_or_default())
            } else { false };
            let _ = fs::remove_file(&remote_provenance_path);
            if !remote_provenance_valid {
                let _ = fs::remove_file(&remote_staged);
                eprintln!("Foundry: ignoring remote executable without matching build provenance");
            } else {
            if let Err(error) = persist_successful_package_state(&package_state, &graph_state, &package_keys, &package_graph, &package_artifacts) {
                let _ = fs::remove_file(&remote_staged);
                return Err(error);
            }
            if let Err(error) = replace_linked_output(&remote_staged, &out) {
                let _ = fs::remove_file(&remote_staged);
                return Err(error);
            }
            let provenance = build_provenance_contents(&compiler, &linker_identity, profile, &fingerprint, &out, &entrypoint_artifact, &package_graph, &package_keys, &package_artifacts)?;
            write_text_atomically(&dir.join(".foundry-build-provenance.toml"), &provenance)
                .map_err(|e| format!("write build provenance: {e}"))?;
            write_text_atomically(&stamp, &fingerprint)
                .map_err(|e| format!("write build fingerprint: {e}"))?;
            println!("Restored {} from remote cache",out.display());
            return Ok(out);
            }
        }
        let _ = fs::remove_file(&remote_staged);
    }
    // Link to a sibling staging path. A failed linker must never truncate or
    // replace the last known-good executable from the previous successful build.
    let c_objects = compile_project_c(&p.root, &c_sources, &dir)?;
    let staged_output = link_staging_path(&out);
    let _ = fs::remove_file(&staged_output);
    let mut linker = Command::new(&compiler);
    linker.current_dir(&p.root)
        .arg("link-llvm")
        .arg(&entrypoint_artifact);
    for package_root in &package_graph.build_order {
        let artifact = package_artifacts.get(package_root).ok_or_else(|| {
            format!("package artifact cache did not produce a module for {}", package_root.display())
        })?;
        linker.arg(artifact);
    }
    for object in &c_objects { linker.arg(object); }
    linker.arg("-o").arg(&staged_output);
    let link_succeeded = match run(&mut linker, &p.root) {
        Ok(succeeded) => succeeded,
        Err(error) => {
            let _ = fs::remove_file(&staged_output);
            return Err(error);
        }
    };
    if !link_succeeded {
        let _ = fs::remove_file(&staged_output);
        return Err("Fusion LLVM link step failed".into());
    }
    if !staged_output.is_file() {
        let _ = fs::remove_file(&staged_output);
        return Err(format!("Fusion linker reported success but did not produce {}", staged_output.display()));
    }
    // Publish package metadata only after the link has succeeded. Then replace
    // the executable and write its fingerprint last as the build success marker.
    if let Err(error) = persist_successful_package_state(&package_state, &graph_state, &package_keys, &package_graph, &package_artifacts) {
        let _ = fs::remove_file(&staged_output);
        return Err(error);
    }
    if let Err(error) = replace_linked_output(&staged_output, &out) {
        let _ = fs::remove_file(&staged_output);
        return Err(error);
    }
    let provenance = build_provenance_contents(&compiler, &linker_identity, profile, &fingerprint, &out, &entrypoint_artifact, &package_graph, &package_keys, &package_artifacts)?;
    write_text_atomically(&dir.join(".foundry-build-provenance.toml"), &provenance)
        .map_err(|e| format!("write build provenance: {e}"))?;
    write_text_atomically(&stamp, &fingerprint)
        .map_err(|e| format!("write build fingerprint: {e}"))?;
    if let Ok(remote)=env::var("FOUNDRY_REMOTE_CACHE") {
        // Publish the sidecar first; the executable upload is the final cache commit marker.
        if let Err(error) = cache::publish_metadata(&remote, &fingerprint, &provenance) {
            eprintln!("warning: could not publish remote build provenance: {error}");
        } else if let Err(error) = cache::publish(&remote, &fingerprint, &out) {
            eprintln!("warning: {error}");
        }
    }
    Ok(out)
}
/// Build several explicit targets concurrently. Each target receives an isolated output directory;
/// the dependency graph validates the independent target tasks before worker threads start.
pub fn build_targets(p: &Project, profile: BuildProfile, fusion: Option<&Path>, targets: &[String], extra: &[String]) -> Result<Vec<PathBuf>, String> {
    if targets.is_empty() { return Ok(vec![build(p,profile,fusion,extra)?]); }
    let mut graph=graph::TaskGraph::default();
    for target in targets { graph.add_task(format!("target:{target}"),std::iter::empty::<String>()).map_err(|e|e.to_string())?; }
    graph.execution_order().map_err(|e|e.to_string())?;
    resolve_dependencies(p, env::var_os("FOUNDRY_OFFLINE").is_some())?;
    let pinned_compiler=find_project_fusion(p,fusion)?;
    let results=std::thread::scope(|scope| {
        let handles=targets.iter().map(|target| {
            let project=p.clone(); let target=target.clone(); let compiler=pinned_compiler.clone(); let mut args=extra.to_vec();
            args.push("--target".into()); args.push(target.clone());
            scope.spawn(move || build_inner(&project,profile,Some(&compiler),&args,false).map(|path|(target,path)))
        }).collect::<Vec<_>>();
        handles.into_iter().map(|h|h.join().unwrap_or_else(|_|Err("target build worker panicked".into()))).collect::<Vec<_>>()
    });
    let mut outputs=Vec::new(); let mut errors=Vec::new();
    for result in results { match result {Ok((target,path))=>{println!("Target {target}: {}",path.display());outputs.push(path)},Err(e)=>errors.push(e)} }
    if errors.is_empty(){Ok(outputs)}else{Err(errors.join("; "))}
}

pub fn check(p: &Project, fusion: Option<&Path>) -> Result<(), String> {
    // A Fusion project is compiled from its entry file. The compiler recursively
    // loads imports, so checking every discovered source independently would
    // incorrectly require imported modules to declare `main` and would bypass
    // the project's actual dependency graph.
    let compiler = find_project_fusion(p, fusion)?;
    let package_graph = resolved_package_graph(p)?;
    let package_args = package_root_arguments(&package_graph)?;
    let entry = main_source(p)?;
    let mut command = Command::new(compiler);
    command.current_dir(&p.root).args(["check"]).arg(&entry).args(&package_args);
    if run(&mut command, &p.root)? {
        Ok(())
    } else {
        Err(format!("Fusion project check failed for {}", entry.display()))
    }
}

pub fn run_program(p: &Project, fusion: Option<&Path>, args: &[String]) -> Result<(), String> {
    let compiler = find_project_fusion(p, fusion)?;
    let package_graph = resolved_package_graph(p)?;
    let package_args = package_root_arguments(&package_graph)?;
    let mut c = Command::new(compiler);
    c.current_dir(&p.root)
        .args(["run"])
        .arg(main_source(p)?)
        .args(&package_args)
        .args(args);
    if !run(&mut c, &p.root)? {
        return Err("Fusion program exited with failure".into());
    }
    Ok(())
}
pub fn test(p: &Project, fusion: Option<&Path>) -> Result<(), String> {
    test_with_args(p, fusion, &[])
}

/// Runs package unit tests (`#[test]` functions) and then integration tests in `tests/`.
/// Extra arguments are passed to `fusion test`, such as `--filter name` or `--ignored`.
pub fn test_with_args(p: &Project, fusion: Option<&Path>, extra: &[String]) -> Result<(), String> {
    let compiler = find_project_fusion(p, fusion)?;
    let package_graph = resolved_package_graph(p)?;
    let package_args = package_root_arguments(&package_graph)?;
    let entry = main_source(p)?;
    let mut unit = Command::new(&compiler);
    unit.current_dir(&p.root).arg("test").arg(&entry).args(&package_args).args(extra);
    let unit_ok = run(&mut unit, &p.root)?;
    let files = test_files(p);
    let integration = if files.is_empty() {
        println!("Foundry: no integration tests found in tests/");
        Ok(())
    } else {
        test_harness::run_files(&compiler, &p.root, &files, &package_args).map(|_| ())
    };
    match (unit_ok, integration) {
        (true, Ok(())) => Ok(()),
        (false, Ok(())) => Err("unit tests failed".to_string()),
        (_, Err(e)) => Err(e),
    }
}

/// Generates Markdown and HTML API documentation from Fusion doc comments.
pub fn api_docs(p: &Project, fusion: Option<&Path>, out_dir: &Path) -> Result<(), String> {
    let compiler = find_project_fusion(p, fusion)?;
    let src = if p.root.join("src").is_dir() { p.root.join("src") } else { p.root.clone() };
    fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    for (flag, file) in [(None, "api.md"), (Some("--html"), "api.html")] {
        let mut command = Command::new(&compiler);
        command.current_dir(&p.root).arg("doc").arg(&src);
        if let Some(flag) = flag { command.arg(flag); }
        command.arg("-o").arg(out_dir.join(file));
        if !run(&mut command, &p.root)? {
            return Err(format!("fusion doc failed for {}", src.display()));
        }
    }
    Ok(())
}

pub fn clean(p: &Project) -> Result<(), String> {
    // Keep downloaded dependencies and toolchains cached under target/foundry.
    // `clean` should remove generated build outputs, not force users to download
    // every dependency again.
    let target = p.root.join("target");
    for relative in ["debug", "release", "doc"] {
        let path = target.join(relative);
        if path.exists() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("remove build output {}: {e}", path.display()))?;
        }
    }
    Ok(())
}
pub fn tree(p: &Project) -> String {
    let mut s = format!(
        "{} v{}\n",
        p.manifest.package.name, p.manifest.package.version
    );
    for (n, d) in &p.manifest.dependencies {
        s.push_str(&format!("├── {n} {}\n", label(d)))
    }
    for (n, d) in &p.manifest.dev_dependencies {
        s.push_str(&format!("[dev]── {n} {}\n", label(d)))
    }
    s
}
fn label(d: &Dependency) -> String {
    match d {
        Dependency::Version(v) => v.clone(),
        Dependency::Path(p) => format!("path:{}", p.display()),
        Dependency::Git { url, rev } => format!(
            "git:{url}{}",
            rev.as_ref().map(|x| format!("#{x}")).unwrap_or_default()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_package_roots_include_transitive_dependency_aliases() {
        let app = PathBuf::from("/workspace/app");
        let consumer = PathBuf::from("/workspace/consumer");
        let model = PathBuf::from("/workspace/model");
        let graph = package_graph::PackageGraph {
            root: app.clone(),
            nodes: BTreeMap::from([
                (app.clone(), package_graph::PackageNode { root: app.clone(), name: "app".into(), version: "1.0.0".into(), source_root: app.join("src"), entry: None }),
                (consumer.clone(), package_graph::PackageNode { root: consumer.clone(), name: "consumer".into(), version: "1.0.0".into(), source_root: consumer.join("src"), entry: None }),
                (model.clone(), package_graph::PackageNode { root: model.clone(), name: "model".into(), version: "1.0.0".into(), source_root: model.join("src"), entry: None }),
            ]),
            edges: vec![
                package_graph::PackageEdge { from: app.clone(), alias: "consumer".into(), to: consumer.clone() },
                package_graph::PackageEdge { from: consumer.clone(), alias: "model".into(), to: model.clone() },
            ],
            build_order: vec![model, consumer, app],
        };
        let args = package_root_arguments(&graph).unwrap();
        assert!(args.windows(2).any(|pair| pair[0] == "--package-root" && pair[1].starts_with("consumer=")));
        assert!(args.windows(2).any(|pair| pair[0] == "--package-root" && pair[1].starts_with("model=")));
    }

    #[test]
    fn registry_cache_prune_preserves_locked_version_and_removes_unreferenced_version() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("foundry-cache-prune-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MANIFEST), manifest_template("prune-fixture")).unwrap();
        let project = load_project(&root).unwrap();
        let resolved = registry::ResolvedPackage {
            name: "demo".into(), requested: "=1.2.3".into(), version: "1.2.3".into(),
            source: "file:///registry/packages/demo/1.2.3.toml".into(),
            registry_url: "file:///registry".into(), checksum: Some("a".repeat(64)),
        };
        write_resolved_lock(&project, &[resolved], false).unwrap();
        let cache = root.join("target/foundry/deps");
        fs::create_dir_all(cache.join("demo-1.2.3")).unwrap();
        fs::create_dir_all(cache.join("demo-1.0.0")).unwrap();
        fs::write(cache.join("demo-1.2.3.tar"), b"current").unwrap();
        fs::write(cache.join("demo-1.0.0.tar"), b"old").unwrap();
        fs::write(cache.join("registry-meta-demo-1.2.3.toml"), "name = \"demo\"\nversion = \"1.2.3\"\n").unwrap();
        fs::write(cache.join("registry-meta-demo-1.0.0.toml"), "name = \"demo\"\nversion = \"1.0.0\"\n").unwrap();
        assert_eq!(prune_registry_cache(&project).unwrap(), 1);
        assert!(cache.join("demo-1.2.3").is_dir());
        assert!(cache.join("demo-1.2.3.tar").is_file());
        assert!(!cache.join("demo-1.0.0").exists());
        assert!(!cache.join("demo-1.0.0.tar").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_cache_prune_preserves_malformed_and_unindexed_entries() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("foundry-cache-prune-edge-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MANIFEST), manifest_template("prune-edge-fixture")).unwrap();
        let project = load_project(&root).unwrap();
        let resolved = registry::ResolvedPackage {
            name: "demo".into(), requested: "=1.2.3".into(), version: "1.2.3".into(),
            source: "file:///registry/packages/demo/1.2.3.toml".into(),
            registry_url: "file:///registry".into(), checksum: Some("a".repeat(64)),
        };
        write_resolved_lock(&project, &[resolved], false).unwrap();
        let cache = root.join("target/foundry/deps");
        fs::create_dir_all(cache.join("demo-1.2.3")).unwrap();
        fs::create_dir_all(cache.join("demo-1.0.0")).unwrap();
        fs::create_dir_all(cache.join("orphan-9.9.9")).unwrap();
        fs::write(cache.join("registry-meta-demo-1.2.3.toml"), "name = \"demo\"\nversion = \"1.2.3\"\n").unwrap();
        fs::write(cache.join("registry-meta-demo-1.0.0.toml"), "malformed = [").unwrap();
        fs::write(cache.join("orphan-9.9.9.tar"), b"no metadata record").unwrap();
        assert_eq!(prune_registry_cache(&project).unwrap(), 0);
        assert!(cache.join("demo-1.2.3").is_dir());
        assert!(cache.join("demo-1.0.0").is_dir(), "malformed metadata must not be used to infer a deletion target");
        assert!(cache.join("orphan-9.9.9.tar").is_file(), "unindexed artifacts must be left untouched");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_build_plan_tracks_changed_unchanged_and_removed_packages() {
        let a = PathBuf::from("/workspace/a");
        let b = PathBuf::from("/workspace/b");
        let c = PathBuf::from("/workspace/c");
        let current = BTreeMap::from([(a.clone(), "new-a".to_string()), (b.clone(), "same-b".to_string())]);
        let previous = BTreeMap::from([(a.clone(), "old-a".to_string()), (b.clone(), "same-b".to_string()), (c.clone(), "old-c".to_string())]);
        let plan = plan_package_build(&current, &previous);
        assert_eq!(plan.changed, vec![a]);
        assert_eq!(plan.unchanged, vec![b]);
        assert_eq!(plan.removed, vec![c]);
    }

    #[test]
    fn fresh_artifact_repairs_missing_package_fingerprint_state() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-fresh-state-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let artifact = dir.join("app");
        let stamp = dir.join(".foundry-fingerprint");
        let state = package_state_path(&dir);
        let graph_state = package_graph_state_path(&dir);
        let graph = package_graph::PackageGraph::default();
        fs::write(&artifact, b"compiled executable").unwrap();
        fs::write(&stamp, "matching-key").unwrap();
        let artifact_hash = fingerprint::file_sha256(&artifact).unwrap();
        fs::write(dir.join(".foundry-build-provenance.toml"), format!("format_version = 5\nbuild_fingerprint = \"matching-key\"\nexecutable_sha256 = \"{artifact_hash}\"\n")).unwrap();
        let fingerprints = BTreeMap::from([
            (PathBuf::from("/workspace/app"), "app-key".to_string()),
            (PathBuf::from("/workspace/lib"), "lib-key".to_string()),
        ]);

        assert!(!state.exists());
        assert!(restore_fresh_build_state(
            &artifact, &stamp, "matching-key", &state, &fingerprints,
            &graph_state, &graph, &BTreeMap::new(), None,
        ).unwrap());
        assert_eq!(read_package_fingerprint_state(&state), fingerprints);
        assert!(read_package_graph_state(&graph_state).is_some());

        // A mismatched stamp must never overwrite state for a different build.
        fs::write(&state, "sentinel").unwrap();
        assert!(!restore_fresh_build_state(
            &artifact, &stamp, "different-key", &state, &BTreeMap::new(),
            &graph_state, &package_graph::PackageGraph::default(), &BTreeMap::new(), None,
        ).unwrap());
        assert_eq!(fs::read_to_string(&state).unwrap(), "sentinel");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_enum_abi_validates_ordered_discriminants_and_payload_types() {
        use sha2::Digest;
        let layout = "None=0();Some=1(i64);Pair=2(i64,double)";
        let digest = format!("{:x}", sha2::Sha256::digest(layout.as_bytes()));
        let valid = format!("FUSION-PACKAGE-INTERFACE\t6\nabi-enum\toption\tMaybe\t%fusion.type.__fusion_m6_6f7074696f6e_t5_4d61796265\t3\t{}\t{}\n", digest, layout);
        assert!(validate_package_interface(&valid, "option").is_ok());

        let bad_order = format!("FUSION-PACKAGE-INTERFACE\t6\nabi-enum\toption\tMaybe\t%fusion.type.__fusion_m6_6f7074696f6e_t5_4d61796265\t3\t{}\tNone=1();Some=0(i64);Pair=2(i64,double)\n", digest);
        assert!(validate_package_interface(&bad_order, "option").unwrap_err().contains("invalid enum variant"));

        let bad_payload = "FUSION-PACKAGE-INTERFACE\t6\nabi-enum\toption\tMaybe\t%fusion.type.__fusion_m6_6f7074696f6e_t5_4d61796265\t1\t0000000000000000000000000000000000000000000000000000000000000000\tSome=0(unsupported-array)\n";
        assert!(validate_package_interface(bad_payload, "option").unwrap_err().contains("layout digest"));
    }

    #[test]
    fn package_struct_abi_must_match_emitted_llvm_identified_type() {
        use sha2::Digest;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-struct-layout-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let ir = dir.join("model.ll");
        let interface = dir.join("model.fpi");
        let symbol = "%fusion.type.__fusion_m5_6d6f64656c_t5_506f696e74";
        let layout = "x:i64,y:double";
        let digest = format!("{:x}", sha2::Sha256::digest(layout.as_bytes()));
        fs::write(&interface, format!("FUSION-PACKAGE-INTERFACE\t6\nabi-struct\tmodel\tPoint\t{}\t2\t{}\t{}\n", symbol, digest, layout)).unwrap();
        fs::write(&ir, format!("{} = type {{ i64, double }}\n", symbol)).unwrap();
        let entry = dir.join("entry.ll");
        fs::write(&entry, "; entry\n").unwrap();
        let units = vec![(ir.clone(), interface.clone())];
        assert!(validate_package_struct_layouts(&units, &entry).is_ok());
        fs::write(&ir, format!("{} = type {{ i64, ptr }}\n", symbol)).unwrap();
        assert!(validate_package_struct_layouts(&units, &entry).unwrap_err().contains("struct layout ABI mismatch"));
        fs::write(&ir, format!("{} = type {{ i64, double }}\n", symbol)).unwrap();
        let second_ir = dir.join("consumer.ll");
        fs::write(&second_ir, format!("{} = type {{ i64, ptr }}\n", symbol)).unwrap();
        let conflicting_units = vec![(ir.clone(), interface.clone()), (second_ir, interface.clone())];
        assert!(validate_package_struct_layouts(&conflicting_units, &entry).unwrap_err().contains("conflicting layouts"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_enum_abi_must_match_emitted_llvm_tagged_pointer_layout() {
        use sha2::Digest;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-enum-layout-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let ir = dir.join("option.ll");
        let interface = dir.join("option.fpi");
        let symbol = "%fusion.type.__fusion_m6_6f7074696f6e_t5_4d61796265";
        let layout = "None=0();Some=1(i64)";
        let digest = format!("{:x}", sha2::Sha256::digest(layout.as_bytes()));
        let payload_digest = format!("{:x}", sha2::Sha256::digest(b"i64"));
        let payload_symbol = "%fusion.variant.__fusion_m6_6f7074696f6e_t11_4d617962653a3a536f6d65";
        fs::write(&interface, format!("FUSION-PACKAGE-INTERFACE\t6\nabi-enum\toption\tMaybe\t{}\t2\t{}\t{}\nabi-enum-payload\toption\tMaybe\tSome\t{}\t1\t{}\ti64\n", symbol, digest, layout, payload_symbol, payload_digest)).unwrap();
        let entry = dir.join("entry.ll");
        fs::write(&entry, "; entry\n").unwrap();
        let units = vec![(ir.clone(), interface.clone())];
        fs::write(&ir, format!("{} = type {{ i32, ptr }}\n{} = type {{ i64 }}\n", symbol, payload_symbol)).unwrap();
        assert!(validate_package_struct_layouts(&units, &entry).is_ok());
        fs::write(&ir, format!("{} = type {{ i32, ptr }}\n{} = type {{ double }}\n", symbol, payload_symbol)).unwrap();
        assert!(validate_package_struct_layouts(&units, &entry).unwrap_err().contains("enum payload layout ABI mismatch"));
        fs::write(&ir, format!("{} = type {{ i64, ptr }}\n{} = type {{ i64 }}\n", symbol, payload_symbol)).unwrap();
        assert!(validate_package_struct_layouts(&units, &entry).unwrap_err().contains("enum layout ABI mismatch"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_link_gate_rejects_struct_layout_drift_before_linking() {
        use sha2::Digest;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-link-gate-struct-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let ir = dir.join("model.ll");
        let interface = dir.join("model.fpi");
        let entry = dir.join("entry.ll");
        let symbol = "%fusion.type.__fusion_m5_6d6f64656c_t5_506f696e74";
        let layout = "x:i64,y:double";
        let digest = format!("{:x}", sha2::Sha256::digest(layout.as_bytes()));
        fs::write(&interface, format!("FUSION-PACKAGE-INTERFACE\t6\nabi-struct\tmodel\tPoint\t{}\t2\t{}\t{}\n", symbol, digest, layout)).unwrap();
        fs::write(&ir, format!("{} = type {{ i64, ptr }}\n", symbol)).unwrap();
        fs::write(&entry, "; entrypoint\n").unwrap();
        let error = validate_package_link_inputs(&[(ir, interface.clone())], &entry, &interface).unwrap_err();
        assert!(error.contains("struct layout ABI mismatch"), "unexpected link-gate error: {error}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_link_gate_rejects_enum_payload_drift_before_linking() {
        use sha2::Digest;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-link-gate-enum-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let ir = dir.join("option.ll");
        let interface = dir.join("option.fpi");
        let entry = dir.join("entry.ll");
        let enum_symbol = "%fusion.type.__fusion_m6_6f7074696f6e_t5_4d61796265";
        let payload_symbol = "%fusion.variant.__fusion_m6_6f7074696f6e_t11_4d617962653a3a536f6d65";
        let enum_layout = "None=0();Some=1(i64)";
        let enum_digest = format!("{:x}", sha2::Sha256::digest(enum_layout.as_bytes()));
        let payload_digest = format!("{:x}", sha2::Sha256::digest(b"i64"));
        fs::write(&interface, format!(
            "FUSION-PACKAGE-INTERFACE\t6\nabi-enum\toption\tMaybe\t{}\t2\t{}\t{}\nabi-enum-payload\toption\tMaybe\tSome\t{}\t1\t{}\ti64\n",
            enum_symbol, enum_digest, enum_layout, payload_symbol, payload_digest,
        )).unwrap();
        fs::write(&ir, format!("{} = type {{ i32, ptr }}\n{} = type {{ double }}\n", enum_symbol, payload_symbol)).unwrap();
        fs::write(&entry, "; entrypoint\n").unwrap();
        let error = validate_package_link_inputs(&[(ir, interface.clone())], &entry, &interface).unwrap_err();
        assert!(error.contains("enum payload layout ABI mismatch"), "unexpected link-gate error: {error}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_interface_data_abi_rejects_bad_layouts_and_duplicate_constants() {
        use sha2::Digest;
        let digest = format!("{:x}", sha2::Sha256::digest(b"x:i64,y:i64"));
        let valid = format!("FUSION-PACKAGE-INTERFACE\t6\nabi-struct\tmodel\tPoint\t%fusion.type.__fusion_m5_6d6f64656c_t5_506f696e74\t2\t{}\tx:i64,y:i64\nabi-data\tmodel\tLIMIT\t_Fdata\ti64\tinteger\t42\n", digest);
        assert!(validate_package_interface(&valid, "sample").is_ok());
        let bad_count = format!("FUSION-PACKAGE-INTERFACE\t6\nabi-struct\tmodel\tPoint\t%fusion.type.__fusion_m5_6d6f64656c_t5_506f696e74\t3\t{}\tx:i64,y:i64\n", digest);
        assert!(validate_package_interface(&bad_count, "sample").unwrap_err().contains("inconsistent field count"));
        let duplicate = "FUSION-PACKAGE-INTERFACE\t6\nabi-data\tmodel\tLIMIT\t_Fdata\ti64\tinteger\t42\nabi-data\tmodel\tLIMIT\t_Fdata\ti64\tinteger\t43\n";
        assert!(validate_package_interface(duplicate, "sample").unwrap_err().contains("more than once"));
        let invalid = "FUSION-PACKAGE-INTERFACE\t6\nabi-data\tmodel\tFLAG\t_Fdata\ti1\tboolean\t2\n";
        assert!(validate_package_interface(invalid, "sample").unwrap_err().contains("invalid scalar data ABI"));
    }

    #[test]
    fn failed_package_state_commit_does_not_publish_partial_cache_or_success_state() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-package-state-transaction-{nonce}"));
        let app = dir.join("app");
        let lib = dir.join("lib");
        let graph = package_graph::PackageGraph {
            root: app.clone(),
            nodes: BTreeMap::from([
                (app.clone(), package_graph::PackageNode { root: app.clone(), name: "app".into(), version: "1.0.0".into(), source_root: app.join("src"), entry: None }),
                (lib.clone(), package_graph::PackageNode { root: lib.clone(), name: "lib".into(), version: "1.0.0".into(), source_root: lib.join("src"), entry: None }),
            ]),
            edges: Vec::new(),
            build_order: vec![lib.clone(), app.clone()],
        };
        let fingerprints = BTreeMap::from([(app.clone(), "app-key".into()), (lib.clone(), "lib-key".into())]);
        let store = package_artifact_store_path(&dir);
        let app_artifact = store.join("app-store").join("app.ll");
        let lib_artifact = store.join("lib-store").join("lib.ll");
        fs::create_dir_all(app_artifact.parent().unwrap()).unwrap();
        fs::create_dir_all(lib_artifact.parent().unwrap()).unwrap();
        fs::write(&app_artifact, "; app module\n").unwrap();
        fs::write(app_artifact.with_extension("fpi"), "FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        fs::write(&lib_artifact, "; lib module\n").unwrap();
        fs::write(lib_artifact.with_extension("fpi"), "FUSION-PACKAGE-INTERFACE\t5\n").unwrap();
        let artifacts = BTreeMap::from([(app.clone(), app_artifact), (lib.clone(), lib_artifact)]);
        let package_state = package_state_path(&dir);
        let graph_state = package_graph_state_path(&dir);

        let error = persist_successful_package_state(
            &package_state, &graph_state, &fingerprints, &graph, &artifacts,
        ).unwrap_err();
        assert!(error.contains("unsupported interface version"), "unexpected error: {error}");
        assert!(!package_state.exists(), "failed commit must not publish package fingerprints");
        assert!(!graph_state.exists(), "failed commit must not publish graph state");
        assert!(!package_artifact_record_path(&dir, &app).exists(), "valid earlier package must not be partially committed");
        assert!(!package_artifact_record_path(&dir, &lib).exists(), "invalid package must not be committed");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_artifact_records_are_content_addressed_and_stale_records_are_collected() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-package-artifacts-{nonce}"));
        let app = PathBuf::from("/workspace/app");
        let old = PathBuf::from("/workspace/old-lib");
        let graph = package_graph::PackageGraph {
            root: app.clone(),
            nodes: BTreeMap::from([
                (app.clone(), package_graph::PackageNode { root: app.clone(), name: "app".into(), version: "1.0.0".into(), source_root: app.join("src"), entry: None }),
                (old.clone(), package_graph::PackageNode { root: old.clone(), name: "old-lib".into(), version: "1.0.0".into(), source_root: old.join("src"), entry: None }),
            ]),
            edges: Vec::new(),
            build_order: vec![old.clone(), app.clone()],
        };
        let keys = BTreeMap::from([(app.clone(), "app-key".to_string()), (old.clone(), "old-key".to_string())]);
        let store = package_artifact_store_path(&dir);
        let app_artifact = store.join("app.ll");
        let old_artifact = store.join("old.ll");
        fs::create_dir_all(&store).unwrap();
        fs::write(&app_artifact, b"app llvm ir").unwrap();
        fs::write(&old_artifact, b"old llvm ir").unwrap();
        fs::write(app_artifact.with_extension("fpi"), b"FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        fs::write(old_artifact.with_extension("fpi"), b"FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        let artifacts = BTreeMap::from([(app.clone(), app_artifact), (old.clone(), old_artifact)]);
        assert_eq!(persist_package_artifact_records(&dir, &graph, &keys, &artifacts).unwrap(), 0);
        assert_eq!(package_artifact_cache_hits(&dir, &graph, &keys), 2);

        let reduced = package_graph::PackageGraph {
            root: app.clone(),
            nodes: BTreeMap::from([(app.clone(), graph.nodes[&app].clone())]),
            edges: Vec::new(),
            build_order: vec![app.clone()],
        };
        let reduced_keys = BTreeMap::from([(app.clone(), "app-key-2".to_string())]);
        let app_artifact = package_artifact_store_path(&dir).join("app-new.ll");
        fs::write(&app_artifact, b"new app llvm ir").unwrap();
        fs::write(app_artifact.with_extension("fpi"), b"FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        let reduced_artifacts = BTreeMap::from([(app.clone(), app_artifact)]);
        assert_eq!(persist_package_artifact_records(&dir, &reduced, &reduced_keys, &reduced_artifacts).unwrap(), 1);
        assert_eq!(package_artifact_cache_hits(&dir, &reduced, &reduced_keys), 0);
        assert!(!package_artifact_record_path(&dir, &old).exists());
        assert!(package_artifact_record_path(&dir, &app).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn linker_binary_replacement_changes_linker_identity() {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-linker-identity-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let linker = dir.join("clang");
        fs::write(&linker, "#!/bin/sh\necho fake clang 1.0\n").unwrap();
        let mut permissions = fs::metadata(&linker).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&linker, permissions).unwrap();
        let before = linker_identity_fields_for(Some(&linker));
        fs::write(&linker, "#!/bin/sh\necho fake clang 1.0\n# binary changed\n").unwrap();
        let after = linker_identity_fields_for(Some(&linker));
        assert_ne!(before, after, "changing linker executable bytes must change its identity");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn linker_target_and_resource_directory_changes_invalidate_identity() {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-linker-resource-{nonce}"));
        let resources = dir.join("clang-resource");
        fs::create_dir_all(resources.join("include")).unwrap();
        fs::write(resources.join("include/builtin.h"), b"#define BUILTIN_VALUE 1\n").unwrap();
        let linker = dir.join("clang");
        let script = format!(
            "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'Fake Clang 1.0' ;;\n  -dumpmachine) echo 'x86_64-test-unknown' ;;\n  -print-resource-dir) echo '{}' ;;\n  -print-search-dirs) echo 'programs: =/fake/bin'; echo 'libraries: =/fake/lib:/fake/sdk/lib' ;;\nesac\n",
            resources.display()
        );
        fs::write(&linker, &script).unwrap();
        let mut permissions = fs::metadata(&linker).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&linker, permissions).unwrap();

        let before = linker_identity_fields_for(Some(&linker));
        assert!(before.iter().any(|field| field.as_slice() == b"x86_64-test-unknown"));
        assert!(before.iter().any(|field| field.as_slice() == resources.to_string_lossy().as_bytes()));
        assert!(before.iter().any(|field| field.as_slice() == b"programs: =/fake/bin\nlibraries: =/fake/lib:/fake/sdk/lib"));
        assert!(linker_environment_identity().iter().any(|(name, _)| name == "SDKROOT"));
        fs::write(resources.join("include/builtin.h"), b"#define BUILTIN_VALUE 2\n").unwrap();
        let after_resource_change = linker_identity_fields_for(Some(&linker));
        assert_ne!(before, after_resource_change, "resource header changes must invalidate linker identity");
        fs::write(linker, script.replace("x86_64-test-unknown", "aarch64-test-unknown")).unwrap();
        let mut permissions = fs::metadata(&linker).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&linker, permissions).unwrap();
        let after_target_change = linker_identity_fields_for(Some(&linker));
        assert!(after_target_change.iter().any(|field| field.as_slice() == b"aarch64-test-unknown"));
        assert_ne!(after_resource_change, after_target_change, "target triple changes must invalidate linker identity");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn provenance_comparison_covers_linker_identity_and_ordered_link_inputs() {
        let expected = "format_version = 5\nlinker_sha256 = \"clang-a\"\nlinker_target_triple = \"x86_64-test-linux\"\nlinker_resource_sha256 = \"resource-a\"\nlinker_search_dirs_sha256 = \"search-a\"\nlinker_environment = [\"SDKROOT=<unset>\"]\nlinker_flags = [\"-O2\"]\nlink_inputs = [\"0:entrypoint=a\", \"1:package:model=b\"]\n";
        let reordered = "link_inputs = [\"0:entrypoint=a\", \"1:package:model=b\"]\nlinker_resource_sha256 = \"resource-a\"\nlinker_search_dirs_sha256 = \"search-a\"\nlinker_environment = [\"SDKROOT=<unset>\"]\nlinker_flags = [\"-O2\"]\nlinker_target_triple = \"x86_64-test-linux\"\nlinker_sha256 = \"clang-a\"\nformat_version = 5\n";
        let changed_linker = expected.replace("clang-a", "clang-b");
        let changed_target = expected.replace("x86_64-test-linux", "aarch64-test-linux");
        let changed_resource = expected.replace("resource-a", "resource-b");
        let changed_flags = expected.replace("-O2", "-O0");
        let changed_search_dirs = expected.replace("search-a", "search-b");
        let changed_environment = expected.replace("SDKROOT=<unset>", "SDKROOT=/different/sdk");
        let changed_order = expected.replace("1:package:model=b", "2:package:model=b");
        assert!(provenance_documents_match(expected, reordered));
        assert!(!provenance_documents_match(expected, &changed_linker));
        assert!(!provenance_documents_match(expected, &changed_target));
        assert!(!provenance_documents_match(expected, &changed_resource));
        assert!(!provenance_documents_match(expected, &changed_flags));
        assert!(!provenance_documents_match(expected, &changed_search_dirs));
        assert!(!provenance_documents_match(expected, &changed_environment));
        assert!(!provenance_documents_match(expected, &changed_order));
    }

    #[cfg(unix)]
    fn make_fake_build_workspace(test_name: &str, fail_link: bool) -> (PathBuf, Project, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-{test_name}-{nonce}"));
        let app = dir.join("app");
        let dependency = dir.join("model");
        fs::create_dir_all(app.join("src")).unwrap();
        fs::create_dir_all(dependency.join("src")).unwrap();
        fs::write(
            app.join(MANIFEST),
            "[package]\nname = \"app\"\nversion = \"1.0.0\"\n[dependencies]\nmodel = { path = \"../model\" }\n",
        ).unwrap();
        fs::write(dependency.join(MANIFEST), "[package]\nname = \"model\"\nversion = \"1.0.0\"\n").unwrap();
        fs::write(app.join("src/main.fusion"), "fn main() {}\n").unwrap();
        fs::write(dependency.join("src/lib.fusion"), "pub const VALUE = 7\n").unwrap();
        let log = dir.join("compiler.log");
        let fail_marker = dir.join("fail-link");
        let invalid_interface_marker = dir.join("invalid-interface");
        if fail_link { fs::write(&fail_marker, "fail").unwrap(); }
        let compiler = dir.join("fake-fusion");
        let script = format!(r#"#!/bin/sh
mode="$1"
shift
printf '%s\n' "$mode" >> '{}'
if [ "$mode" = "--version" ]; then echo 'fake fusion 1.0'; exit 0; fi
out=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then shift; out="$1"; fi
  shift
done
case "$mode" in
  emit-package-llvm) printf '; fake package module\n' > "$out" ;;
  package-interface)
    if [ -f '{}' ]; then printf 'FUSION-PACKAGE-INTERFACE\t1\n' > "$out";
    else printf 'FUSION-PACKAGE-INTERFACE\t6\n' > "$out"; fi
    ;;
  emit-entrypoint-llvm) printf '; fake entrypoint module\n' > "$out" ;;
  link-llvm)
    if [ -f '{}' ]; then echo 'intentional fake link failure' >&2; exit 23; fi
    printf '#!/bin/sh\nexit 0\n' > "$out"
    chmod +x "$out"
    ;;
  *) echo "unexpected fake compiler command: $mode" >&2; exit 29 ;;
esac
"#, log.display(), fail_marker.display(), invalid_interface_marker.display());
        fs::write(&compiler, script).unwrap();
        let mut permissions = fs::metadata(&compiler).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&compiler, permissions).unwrap();
        let project = load_project(&app).unwrap();
        (dir, project, compiler, log)
    }

    #[cfg(unix)]
    #[test]
    fn build_inner_runs_package_compile_validate_link_and_reuses_successful_build() {
        let (dir, project, compiler, log) = make_fake_build_workspace("build-orchestration", false);
        let output = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        assert!(output.is_file(), "successful link must produce the application executable");
        let graph = package_graph::resolve_package_graph(&project).unwrap();
        assert_eq!(graph.build_order.len(), 2, "fixture must exercise a dependency and root package");
        let output_dir = output.parent().unwrap();
        assert!(package_state_path(output_dir).is_file());
        assert!(package_graph_state_path(output_dir).is_file());
        assert!(output_dir.join(".foundry-fingerprint").is_file());
        let provenance_text = fs::read_to_string(output_dir.join(".foundry-build-provenance.toml")).unwrap();
        let provenance: toml::Value = toml::from_str(&provenance_text).unwrap();
        assert_eq!(provenance.get("format_version").and_then(toml::Value::as_integer), Some(5));
        assert_eq!(provenance.get("build_fingerprint").and_then(toml::Value::as_str), fs::read_to_string(output_dir.join(".foundry-fingerprint")).ok().as_deref());
        assert_eq!(provenance.get("compiler_sha256").and_then(toml::Value::as_str), fingerprint::file_sha256(&compiler).ok().as_deref());
        assert_eq!(provenance.get("executable_sha256").and_then(toml::Value::as_str), fingerprint::file_sha256(&output).ok().as_deref());
        assert!(provenance.get("entrypoint_llvm_sha256").and_then(toml::Value::as_str).is_some());
        assert_eq!(provenance.get("package_interface_version").and_then(toml::Value::as_integer), Some(6));
        assert_eq!(provenance.get("linker_sha256").and_then(toml::Value::as_str), resolve_linker_path().and_then(|path| fingerprint::file_sha256(&path.canonicalize().unwrap_or(path)).ok()).as_deref());
        assert!(provenance.get("linker_path").and_then(toml::Value::as_str).is_some());
        assert!(provenance.get("linker_version").and_then(toml::Value::as_str).is_some());
        assert!(provenance.get("linker_target_triple").and_then(toml::Value::as_str).is_some());
        assert!(provenance.get("linker_resource_dir").and_then(toml::Value::as_str).is_some());
        assert!(provenance.get("linker_resource_sha256").and_then(toml::Value::as_str).is_some());
        assert!(provenance.get("linker_search_dirs").and_then(toml::Value::as_str).is_some());
        assert!(provenance.get("linker_search_dirs_sha256").and_then(toml::Value::as_str).is_some());
        assert!(provenance.get("linker_environment").and_then(toml::Value::as_array).map_or(false, |values| !values.is_empty()));
        assert_eq!(provenance.get("linker_flags").and_then(toml::Value::as_array).map(Vec::len), Some(1));
        assert_eq!(provenance.get("link_inputs").and_then(toml::Value::as_array).map(Vec::len), Some(3));
        assert!(provenance.get("link_inputs_sha256").and_then(toml::Value::as_str).is_some());
        let lockfile_records = provenance.get("package_lockfiles").and_then(toml::Value::as_array).unwrap();
        assert_eq!(lockfile_records.len(), graph.nodes.len());
        for item in lockfile_records {
            let item = item.as_str().unwrap();
            let (name, digest) = item.split_once('=').unwrap();
            let package_root = graph.nodes.iter().find(|(_, node)| node.name == name).unwrap().0;
            let lock_path = package_root.join(LOCKFILE);
            let expected = if lock_path.is_file() {
                fingerprint::file_sha256(&lock_path).unwrap()
            } else {
                fingerprint::fingerprint_fields([b"<no-lockfile>".as_slice()])
            };
            assert_eq!(digest, expected);
        }
        for root in graph.nodes.keys() {
            assert!(package_artifact_record_path(output_dir, root).is_file(), "successful build should commit every package record");
        }

        let first_log = fs::read_to_string(&log).unwrap();
        assert_eq!(first_log.lines().filter(|line| *line == "emit-package-llvm").count(), 2);
        assert_eq!(first_log.lines().filter(|line| *line == "package-interface").count(), 2);
        assert_eq!(first_log.lines().filter(|line| *line == "emit-entrypoint-llvm").count(), 1);
        assert_eq!(first_log.lines().filter(|line| *line == "link-llvm").count(), 1);

        // The second invocation still resolves and validates the package inputs,
        // but the matching executable stamp restores the successful build without
        // recompiling packages or invoking the native linker again.
        let cached = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        assert_eq!(cached, output);
        let second_log = fs::read_to_string(&log).unwrap();
        assert_eq!(second_log.lines().filter(|line| *line == "emit-package-llvm").count(), 2);
        assert_eq!(second_log.lines().filter(|line| *line == "package-interface").count(), 2);
        assert_eq!(second_log.lines().filter(|line| *line == "emit-entrypoint-llvm").count(), 1);
        assert_eq!(second_log.lines().filter(|line| *line == "link-llvm").count(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn compiler_binary_change_invalidates_package_and_root_artifacts() {
        let (dir, project, compiler, log) = make_fake_build_workspace("compiler-binary-identity", false);
        let graph = package_graph::resolve_package_graph(&project).unwrap();
        let before = package_build_fingerprints(&graph, &compiler, BuildProfile::Debug, &[]).unwrap();
        let output = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let initial_stamp = fs::read_to_string(output.parent().unwrap().join(".foundry-fingerprint")).unwrap();
        let initial_provenance: toml::Value = toml::from_str(&fs::read_to_string(output.parent().unwrap().join(".foundry-build-provenance.toml")).unwrap()).unwrap();
        let initial_compiler_hash = initial_provenance.get("compiler_sha256").and_then(toml::Value::as_str).unwrap().to_string();

        // Keep the path and --version string stable while changing executable
        // contents. A version-only or path-only cache key would miss this update.
        let script = fs::read_to_string(&compiler).unwrap();
        assert!(script.contains("fake package module"));
        fs::write(&compiler, script.replace("fake package module", "fake package module rebuilt")).unwrap();
        let after_graph = package_graph::resolve_package_graph(&project).unwrap();
        let after = package_build_fingerprints(&after_graph, &compiler, BuildProfile::Debug, &[]).unwrap();
        for root in graph.nodes.keys() {
            assert_ne!(before[root], after[root], "compiler binary change must invalidate package {}", root.display());
        }
        build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let output_dir = output.parent().unwrap();
        let next_stamp = fs::read_to_string(output_dir.join(".foundry-fingerprint")).unwrap();
        let next_provenance: toml::Value = toml::from_str(&fs::read_to_string(output_dir.join(".foundry-build-provenance.toml")).unwrap()).unwrap();
        assert_ne!(initial_stamp, next_stamp, "root build key must include compiler executable bytes");
        assert_ne!(initial_compiler_hash, next_provenance.get("compiler_sha256").and_then(toml::Value::as_str).unwrap());
        let log_text = fs::read_to_string(&log).unwrap();
        assert_eq!(log_text.lines().filter(|line| *line == "emit-package-llvm").count(), 4,
            "changing the compiler executable should rebuild both package modules");
        assert_eq!(log_text.lines().filter(|line| *line == "link-llvm").count(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn tampered_executable_provenance_prevents_fresh_build_reuse() {
        let (dir, project, compiler, log) = make_fake_build_workspace("provenance-tamper", false);
        let output = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let output_dir = output.parent().unwrap().to_path_buf();
        fs::write(output_dir.join(".foundry-build-provenance.toml"), "format_version = 5\nexecutable_sha256 = \"tampered\"\n").unwrap();
        build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let log_text = fs::read_to_string(&log).unwrap();
        assert_eq!(log_text.lines().filter(|line| *line == "link-llvm").count(), 2,
            "a mismatched provenance hash must force a fresh link");
        let provenance: toml::Value = toml::from_str(&fs::read_to_string(output_dir.join(".foundry-build-provenance.toml")).unwrap()).unwrap();
        assert_eq!(provenance.get("executable_sha256").and_then(toml::Value::as_str), fingerprint::file_sha256(&output).ok().as_deref());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn tampered_linker_provenance_prevents_fresh_build_reuse() {
        let (dir, project, compiler, log) = make_fake_build_workspace("provenance-linker-tamper", false);
        let output = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let output_dir = output.parent().unwrap();
        let provenance_path = output_dir.join(".foundry-build-provenance.toml");
        let provenance_text = fs::read_to_string(&provenance_path).unwrap();
        let mut provenance: toml::Value = toml::from_str(&provenance_text).unwrap();
        provenance.as_table_mut().unwrap().insert("linker_sha256".into(), toml::Value::String("tampered-linker-hash".into()));
        fs::write(&provenance_path, toml::to_string(&provenance).unwrap()).unwrap();
        let _ = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let log_text = fs::read_to_string(&log).unwrap();
        assert_eq!(log_text.lines().filter(|line| *line == "link-llvm").count(), 2,
            "tampered linker provenance must force a relink");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn invalid_package_interface_stops_full_build_before_native_link() {
        let (dir, project, compiler, log) = make_fake_build_workspace("build-invalid-interface", false);
        fs::write(dir.join("invalid-interface"), "invalid").unwrap();
        let error = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap_err();
        assert!(error.contains("unsupported interface version"), "unexpected build error: {error}");
        let commands = fs::read_to_string(&log).unwrap();
        assert_eq!(commands.lines().filter(|line| *line == "link-llvm").count(), 0, "linker must not run after package interface validation fails");
        let output_dir = project.root.join("target/debug");
        assert!(!output_dir.join(".foundry-fingerprint").exists());
        assert!(!package_state_path(&output_dir).exists());
        assert!(!package_graph_state_path(&output_dir).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn failed_native_link_does_not_commit_success_state_and_retry_recovers() {
        let (dir, project, compiler, log) = make_fake_build_workspace("build-link-recovery", true);
        let error = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap_err();
        assert!(error.contains("link step failed"), "unexpected build error: {error}");
        let output_dir = project.root.join("target/debug");
        assert!(!output_dir.join(".foundry-fingerprint").exists(), "failed link must not publish executable success stamp");
        assert!(!package_state_path(&output_dir).exists(), "failed link must not publish package fingerprint state");
        assert!(!package_graph_state_path(&output_dir).exists(), "failed link must not publish package graph state");
        let graph = package_graph::resolve_package_graph(&project).unwrap();
        for root in graph.nodes.keys() {
            assert!(!package_artifact_record_path(&output_dir, root).exists(), "failed link must not commit package cache records");
        }

        // Once the linker recovers, Foundry recompiles the uncommitted package
        // modules and completes the same build successfully.
        fs::remove_file(dir.join("fail-link")).unwrap();
        let output = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        assert!(output.is_file());
        assert!(output_dir.join(".foundry-fingerprint").is_file());
        assert!(package_state_path(&output_dir).is_file());
        assert!(package_graph_state_path(&output_dir).is_file());
        let log_contents = fs::read_to_string(&log).unwrap();
        assert_eq!(log_contents.lines().filter(|line| *line == "link-llvm").count(), 2);
        assert_eq!(log_contents.lines().filter(|line| *line == "emit-package-llvm").count(), 4);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn incremental_graph_changes_preserve_last_good_executable_and_collect_removed_packages() {
        let (dir, project, compiler, log) = make_fake_build_workspace("incremental-graph-safety", false);
        let output = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let output_dir = output.parent().unwrap().to_path_buf();
        let initial_executable = fs::read(&output).unwrap();
        let initial_stamp = fs::read_to_string(output_dir.join(".foundry-fingerprint")).unwrap();
        let initial_package_state = fs::read_to_string(package_state_path(&output_dir)).unwrap();
        let initial_graph_state = fs::read_to_string(package_graph_state_path(&output_dir)).unwrap();
        let initial_graph = package_graph::resolve_package_graph(&project).unwrap();
        let model_root = initial_graph.nodes.values()
            .find(|node| node.name == "model").unwrap().root.clone();
        let model_record = package_artifact_record_path(&output_dir, &model_root);
        assert!(model_record.is_file());

        // Editing a transitive dependency invalidates both that package and the
        // root package key. A failed incremental link must leave the previous
        // executable and all last-successful metadata untouched.
        fs::write(model_root.join("src/lib.fusion"), "pub const VALUE = 88\n").unwrap();
        fs::write(dir.join("fail-link"), "fail").unwrap();
        let error = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap_err();
        assert!(error.contains("link step failed"), "unexpected incremental failure: {error}");
        assert_eq!(fs::read(&output).unwrap(), initial_executable, "failed link must not damage last known-good executable");
        assert_eq!(fs::read_to_string(output_dir.join(".foundry-fingerprint")).unwrap(), initial_stamp);
        assert_eq!(fs::read_to_string(package_state_path(&output_dir)).unwrap(), initial_package_state);
        assert_eq!(fs::read_to_string(package_graph_state_path(&output_dir)).unwrap(), initial_graph_state);
        assert!(model_record.is_file(), "failed incremental build must not collect committed package records");
        assert!(!fs::read_dir(&output_dir).unwrap().filter_map(Result::ok).any(|entry| {
            entry.file_name().to_string_lossy().contains("foundry-link-")
        }), "failed linker staging output should be cleaned up");

        // A retry commits the changed transitive keys. Both package modules are
        // rebuilt because the root key includes its dependency's fingerprint.
        fs::remove_file(dir.join("fail-link")).unwrap();
        build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let after_retry = fs::read_to_string(&log).unwrap();
        assert_eq!(after_retry.lines().filter(|line| *line == "emit-package-llvm").count(), 6,
            "initial build plus failed and successful changed-key attempts should compile both packages");
        assert_eq!(after_retry.lines().filter(|line| *line == "link-llvm").count(), 3);
        assert_ne!(fs::read_to_string(output_dir.join(".foundry-fingerprint")).unwrap(), initial_stamp);

        // Removing the dependency changes the graph and root key. The removed
        // package's committed artifact record is collected only after link success.
        fs::write(project.root.join(MANIFEST), "[package]\nname = \"app\"\nversion = \"1.0.0\"\n").unwrap();
        let project_without_model = load_project(&project.root).unwrap();
        build_inner(&project_without_model, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let reduced_graph = package_graph::resolve_package_graph(&project_without_model).unwrap();
        assert_eq!(reduced_graph.nodes.len(), 1);
        assert!(!model_record.exists(), "removed dependency's stale package record should be collected");
        assert!(package_artifact_record_path(&output_dir, &reduced_graph.root).is_file());
        let final_log = fs::read_to_string(&log).unwrap();
        assert_eq!(final_log.lines().filter(|line| *line == "emit-package-llvm").count(), 7,
            "removing the dependency should rebuild only the root package");
        assert_eq!(final_log.lines().filter(|line| *line == "link-llvm").count(), 4);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn identical_projects_produce_byte_identical_package_artifacts_and_isolated_caches() {
        let (dir_a, project_a, compiler, log) = make_fake_build_workspace("reproducible-project-a", false);
        let dir_b = dir_a.join("independent-copy");
        let app_b = dir_b.join("app");
        let model_b = dir_b.join("model");
        fs::create_dir_all(app_b.join("src")).unwrap();
        fs::create_dir_all(model_b.join("src")).unwrap();
        fs::write(app_b.join(MANIFEST), fs::read(project_a.root.join(MANIFEST)).unwrap()).unwrap();
        fs::write(model_b.join(MANIFEST), fs::read(project_a.root.join("../model").join(MANIFEST)).unwrap()).unwrap();
        fs::write(app_b.join("src/main.fusion"), fs::read(project_a.root.join("src/main.fusion")).unwrap()).unwrap();
        fs::write(model_b.join("src/lib.fusion"), fs::read(project_a.root.join("../model/src/lib.fusion")).unwrap()).unwrap();
        let project_b = load_project(&app_b).unwrap();

        let graph_a = package_graph::resolve_package_graph(&project_a).unwrap();
        let graph_b = package_graph::resolve_package_graph(&project_b).unwrap();
        let keys_a = package_build_fingerprints(&graph_a, &compiler, BuildProfile::Debug, &[]).unwrap();
        let keys_b = package_build_fingerprints(&graph_b, &compiler, BuildProfile::Debug, &[]).unwrap();
        for name in ["app", "model"] {
            let root_a = graph_a.nodes.values().find(|node| node.name == name).unwrap().root.clone();
            let root_b = graph_b.nodes.values().find(|node| node.name == name).unwrap().root.clone();
            assert_eq!(keys_a[&root_a], keys_b[&root_b], "identical package inputs should have reproducible keys for {name}");
        }

        let output_a = build_inner(&project_a, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let output_b = build_inner(&project_b, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        assert_ne!(output_a.parent(), output_b.parent(), "independent projects must own separate output/cache roots");
        let graph_a = package_graph::resolve_package_graph(&project_a).unwrap();
        let graph_b = package_graph::resolve_package_graph(&project_b).unwrap();
        for name in ["app", "model"] {
            let root_a = graph_a.nodes.values().find(|node| node.name == name).unwrap().root.clone();
            let root_b = graph_b.nodes.values().find(|node| node.name == name).unwrap().root.clone();
            let record_a = read_package_artifact_record(&package_artifact_record_path(output_a.parent().unwrap(), &root_a)).unwrap();
            let record_b = read_package_artifact_record(&package_artifact_record_path(output_b.parent().unwrap(), &root_b)).unwrap();
            let artifact_a = package_record_artifact_path(output_a.parent().unwrap(), &record_a).unwrap();
            let artifact_b = package_record_artifact_path(output_b.parent().unwrap(), &record_b).unwrap();
            let interface_a = package_record_interface_path(output_a.parent().unwrap(), &record_a).unwrap();
            let interface_b = package_record_interface_path(output_b.parent().unwrap(), &record_b).unwrap();
            assert_eq!(fs::read(&artifact_a).unwrap(), fs::read(&artifact_b).unwrap(), "package IR must be byte-identical for {name}");
            assert_eq!(fs::read(&interface_a).unwrap(), fs::read(&interface_b).unwrap(), "package interface must be byte-identical for {name}");
            assert_ne!(artifact_a, artifact_b, "cache files must not be shared across project roots");
        }

        // Damage project A's cache. Project B must continue to hit its own intact
        // records without invoking the compiler or observing A's corruption.
        let model_a = graph_a.nodes.values().find(|node| node.name == "model").unwrap().root.clone();
        let record_a = read_package_artifact_record(&package_artifact_record_path(output_a.parent().unwrap(), &model_a)).unwrap();
        let artifact_a = package_record_artifact_path(output_a.parent().unwrap(), &record_a).unwrap();
        fs::write(&artifact_a, "corrupted project A cache").unwrap();
        let before_log = fs::read_to_string(&log).unwrap();
        let before_compiles = before_log.lines().filter(|line| *line == "emit-package-llvm" || *line == "package-interface").count();
        build_inner(&project_b, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let after_log = fs::read_to_string(&log).unwrap();
        let after_compiles = after_log.lines().filter(|line| *line == "emit-package-llvm" || *line == "package-interface").count();
        assert_eq!(after_compiles, before_compiles, "project B's warm cache must be independent of project A corruption");
        let _ = fs::remove_dir_all(dir_a);
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_builds_with_distinct_targets_use_isolated_cache_directories() {
        let (dir, project, compiler, _log) = make_fake_build_workspace("concurrent-target-isolation", false);
        let project_a = project.clone();
        let compiler_a = compiler.clone();
        let project_b = project.clone();
        let compiler_b = compiler.clone();
        let (result_a, result_b) = std::thread::scope(|scope| {
            let a = scope.spawn(move || build_inner(&project_a, BuildProfile::Debug, Some(&compiler_a), &["--target".into(), "target-a".into()], false));
            let b = scope.spawn(move || build_inner(&project_b, BuildProfile::Debug, Some(&compiler_b), &["--target".into(), "target-b".into()], false));
            (a.join().unwrap(), b.join().unwrap())
        });
        let output_a = result_a.unwrap();
        let output_b = result_b.unwrap();
        assert_ne!(output_a, output_b);
        assert!(output_a.is_file() && output_b.is_file());
        let dir_a = output_a.parent().unwrap();
        let dir_b = output_b.parent().unwrap();
        for output_dir in [dir_a, dir_b] {
            assert!(output_dir.join(".foundry-fingerprint").is_file());
            assert!(package_state_path(output_dir).is_file());
            assert!(package_graph_state_path(output_dir).is_file());
            assert!(package_artifact_store_path(output_dir).is_dir());
        }
        assert_ne!(package_artifact_store_path(dir_a), package_artifact_store_path(dir_b));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn debug_and_release_profiles_keep_distinct_artifacts_and_success_state() {
        let (dir, project, compiler, _log) = make_fake_build_workspace("profile-cache-isolation", false);
        let debug = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let release = build_inner(&project, BuildProfile::Release, Some(&compiler), &[], false).unwrap();
        assert_ne!(debug.parent(), release.parent());
        assert!(debug.is_file() && release.is_file());
        let graph = package_graph::resolve_package_graph(&project).unwrap();
        let root = graph.root.clone();
        let debug_record = read_package_artifact_record(&package_artifact_record_path(debug.parent().unwrap(), &root)).unwrap();
        let release_record = read_package_artifact_record(&package_artifact_record_path(release.parent().unwrap(), &root)).unwrap();
        assert_ne!(debug_record.fingerprint, release_record.fingerprint, "profile must participate in package cache identity");
        assert_ne!(
            package_record_artifact_path(debug.parent().unwrap(), &debug_record).unwrap(),
            package_record_artifact_path(release.parent().unwrap(), &release_record).unwrap(),
        );
        assert!(debug.parent().unwrap().join(".foundry-fingerprint").is_file());
        assert!(release.parent().unwrap().join(".foundry-fingerprint").is_file());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn interrupted_package_record_publication_preserves_last_good_build_and_recovers() {
        let (dir, project, compiler, _log) = make_fake_build_workspace("interrupted-cache-publication", false);
        let output = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        let output_dir = output.parent().unwrap();
        let original_executable = fs::read(&output).unwrap();
        let original_stamp = fs::read_to_string(output_dir.join(".foundry-fingerprint")).unwrap();
        let original_package_state = fs::read_to_string(package_state_path(output_dir)).unwrap();
        let original_graph_state = fs::read_to_string(package_graph_state_path(output_dir)).unwrap();
        let graph = package_graph::resolve_package_graph(&project).unwrap();
        let model_root = graph.nodes.values().find(|node| node.name == "model").unwrap().root.clone();
        let record_path = package_artifact_record_path(output_dir, &model_root);
        let interrupted_temp = record_path.with_extension("toml.tmp");

        fs::write(model_root.join("src/lib.fusion"), "pub const VALUE = 123\n").unwrap();
        // A directory at the deterministic staging filename simulates a process
        // interruption or stale filesystem object left during record publication.
        fs::create_dir_all(&interrupted_temp).unwrap();
        let error = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap_err();
        assert!(error.contains("write package artifact record"), "unexpected publication failure: {error}");
        assert_eq!(fs::read(&output).unwrap(), original_executable, "failed cache publication must preserve the last good executable");
        assert_eq!(fs::read_to_string(output_dir.join(".foundry-fingerprint")).unwrap(), original_stamp);
        assert_eq!(fs::read_to_string(package_state_path(output_dir)).unwrap(), original_package_state);
        assert_eq!(fs::read_to_string(package_graph_state_path(output_dir)).unwrap(), original_graph_state);
        assert!(!fs::read_dir(output_dir).unwrap().filter_map(Result::ok).any(|entry| entry.file_name().to_string_lossy().contains("foundry-link-")));

        fs::remove_dir(&interrupted_temp).unwrap();
        let recovered = build_inner(&project, BuildProfile::Debug, Some(&compiler), &[], false).unwrap();
        assert_eq!(recovered, output);
        assert!(fs::read_to_string(output_dir.join(".foundry-fingerprint")).unwrap() != original_stamp);
        assert!(fs::read_to_string(package_state_path(output_dir)).unwrap() != original_package_state);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn package_artifact_cache_rebuilds_corrupt_ir_and_interface_before_link() {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-package-cache-recovery-{nonce}"));
        let root = dir.join("app");
        let source = root.join("src");
        fs::create_dir_all(&source).unwrap();
        let entry = source.join("main.fusion");
        fs::write(&entry, "fn main() {}\n").unwrap();
        let graph = package_graph::PackageGraph {
            root: root.clone(),
            nodes: BTreeMap::from([(root.clone(), package_graph::PackageNode {
                root: root.clone(), name: "app".into(), version: "1.0.0".into(),
                source_root: source, entry: Some(entry),
            })]),
            edges: Vec::new(), build_order: vec![root.clone()],
        };
        let keys = BTreeMap::from([(root.clone(), "stable-package-key".to_string())]);
        let compiler = dir.join("fake-fusion");
        let log = dir.join("compiler.log");
        let script = format!(r#"#!/bin/sh
mode="$1"
shift
out=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then shift; out="$1"; fi
  shift
done
printf '%s\n' "$mode" >> '{}'
case "$mode" in
  emit-package-llvm) printf '; valid package LLVM\n' > "$out" ;;
  package-interface) printf 'FUSION-PACKAGE-INTERFACE\t6\n' > "$out" ;;
  *) exit 17 ;;
esac
"#, log.display());
        fs::write(&compiler, script).unwrap();
        let mut permissions = fs::metadata(&compiler).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&compiler, permissions).unwrap();

        let output = dir.join("debug");
        let (first, first_hits) = compile_package_artifacts(&output, &graph, &keys, &compiler).unwrap();
        assert_eq!(first_hits, 0);
        persist_package_artifact_records(&output, &graph, &keys, &first).unwrap();
        let artifact = first[&root].clone();
        let interface = artifact.with_extension("fpi");

        let (cached, cache_hits) = compile_package_artifacts(&output, &graph, &keys, &compiler).unwrap();
        assert_eq!(cache_hits, 1, "a valid artifact/interface pair should be reused");
        assert_eq!(cached[&root], artifact);
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);

        // A checksum mismatch in the LLVM module must force regeneration.
        fs::write(&artifact, "; corrupted LLVM\\n").unwrap();
        let (rebuilt_ir, ir_hits) = compile_package_artifacts(&output, &graph, &keys, &compiler).unwrap();
        assert_eq!(ir_hits, 0);
        persist_package_artifact_records(&output, &graph, &keys, &rebuilt_ir).unwrap();
        assert!(fs::read_to_string(&artifact).unwrap().contains("valid package LLVM"));

        // A checksum mismatch in the interface must also force regeneration,
        // even when the LLVM artifact itself is intact.
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\\t1\\n").unwrap();
        let (rebuilt_interface, interface_hits) = compile_package_artifacts(&output, &graph, &keys, &compiler).unwrap();
        assert_eq!(interface_hits, 0);
        assert!(validate_package_interface(&fs::read_to_string(&interface).unwrap(), "app").is_ok());
        persist_package_artifact_records(&output, &graph, &keys, &rebuilt_interface).unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 6);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_artifact_cache_rejects_fingerprint_mismatches() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-package-artifact-mismatch-{nonce}"));
        let root = PathBuf::from("/workspace/lib");
        let graph = package_graph::PackageGraph {
            root: root.clone(),
            nodes: BTreeMap::from([(root.clone(), package_graph::PackageNode { root: root.clone(), name: "lib".into(), version: "1.0.0".into(), source_root: root.join("src"), entry: None })]),
            edges: Vec::new(), build_order: vec![root.clone()],
        };
        let first = BTreeMap::from([(root.clone(), "fingerprint-a".to_string())]);
        let artifact = package_artifact_store_path(&dir).join("lib.ll");
        fs::create_dir_all(package_artifact_store_path(&dir)).unwrap();
        fs::write(&artifact, b"llvm ir").unwrap();
        fs::write(artifact.with_extension("fpi"), b"FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        let artifacts = BTreeMap::from([(root.clone(), artifact)]);
        persist_package_artifact_records(&dir, &graph, &first, &artifacts).unwrap();
        let changed = BTreeMap::from([(root.clone(), "fingerprint-b".to_string())]);
        assert_eq!(package_artifact_cache_hits(&dir, &graph, &changed), 0);
        // Matching metadata is not enough: damaged IR or interface bytes invalidate the hit.
        fs::write(&artifacts[&root], b"corrupted llvm ir").unwrap();
        assert_eq!(package_artifact_cache_hits(&dir, &graph, &first), 0);
        fs::write(&artifacts[&root], b"llvm ir").unwrap();
        fs::write(artifacts[&root].with_extension("fpi"), b"FUSION-PACKAGE-INTERFACE\t1\n").unwrap();
        assert_eq!(package_artifact_cache_hits(&dir, &graph, &first), 0);
        fs::write(artifacts[&root].with_extension("fpi"), b"corrupted interface").unwrap();
        assert_eq!(package_artifact_cache_hits(&dir, &graph, &first), 0);
        fs::write(artifacts[&root].with_extension("fpi"), b"FUSION-PACKAGE-INTERFACE\t6\nabi-data\tmodel\tFLAG\t_Fdata\ti1\tboolean\t2\n").unwrap();
        let updated_digest = fingerprint::file_sha256(&artifacts[&root].with_extension("fpi")).unwrap();
        let record_path = package_artifact_record_path(&dir, &root);
        let mut record = read_package_artifact_record(&record_path).unwrap();
        record.interface_sha256 = updated_digest;
        fs::write(&record_path, toml::to_string(&record).unwrap()).unwrap();
        assert_eq!(package_artifact_cache_hits(&dir, &graph, &first), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_artifact_records_cannot_escape_the_foundry_store() {
        let dir = PathBuf::from("/tmp/foundry-cache");
        let record = PackageArtifactRecord {
            format_version: 6,
            package_root: PathBuf::from("/workspace/lib"),
            package_name: "lib".into(),
            package_version: "1.0.0".into(),
            fingerprint: "key".into(),
            artifact_file: "../../outside.ll".into(),
            artifact_sha256: "digest".into(),
            artifact_kind: "llvm-package-ir-v5".into(),
            interface_file: "../../outside.fpi".into(),
            interface_sha256: "digest".into(),
        };
        assert!(package_record_artifact_path(&dir, &record).is_none());
        assert!(package_record_interface_path(&dir, &record).is_none());
    }

    #[test]
    fn package_fingerprint_state_round_trips_and_is_replaced_atomically() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-package-state-{nonce}"));
        let path = package_state_path(&dir);
        let expected = BTreeMap::from([
            (PathBuf::from("/workspace/app"), "abc123".to_string()),
            (PathBuf::from("/workspace/lib with spaces"), "def456".to_string()),
        ]);
        write_package_fingerprint_state(&path, &expected).unwrap();
        assert_eq!(read_package_fingerprint_state(&path), expected);
        let replacement = BTreeMap::from([(PathBuf::from("/workspace/app"), "updated".to_string())]);
        write_package_fingerprint_state(&path, &replacement).unwrap();
        assert_eq!(read_package_fingerprint_state(&path), replacement);
        assert!(!path.with_extension("toml.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn package_graph_state_round_trips_and_replaces_previous_snapshot() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("foundry-package-graph-state-{nonce}"));
        let path = package_graph_state_path(&dir);
        let root = PathBuf::from("/workspace/app");
        let graph = package_graph::PackageGraph {
            root: root.clone(),
            nodes: BTreeMap::from([(root.clone(), package_graph::PackageNode {
                root: root.clone(), name: "app".into(), version: "0.1.0".into(),
                source_root: root.join("src"), entry: Some(root.join("src/main.fusion")),
            })]),
            edges: Vec::new(),
            build_order: vec![root],
        };
        write_package_graph_state(&path, &graph).unwrap();
        let restored = read_package_graph_state(&path).unwrap();
        assert_eq!(restored.nodes, graph.nodes);
        assert_eq!(restored.build_order, graph.build_order);
        assert_eq!(restored.edges, graph.edges);
        let replacement = package_graph::PackageGraph::default();
        write_package_graph_state(&path, &replacement).unwrap();
        assert_eq!(read_package_graph_state(&path).unwrap().nodes, replacement.nodes);
        assert!(!path.with_extension("toml.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn transitive_package_changes_invalidate_downstream_package_keys() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("foundry-package-keys-{nonce}"));
        let app = root.join("app");
        let facade = root.join("facade");
        let core = root.join("core");
        for package in [&app, &facade, &core] {
            fs::create_dir_all(package.join("src")).unwrap();
        }
        fs::write(
            app.join(MANIFEST),
            "[package]\nname = \"app\"\n[dependencies]\nfacade = { path = \"../facade\" }\n",
        ).unwrap();
        fs::write(
            facade.join(MANIFEST),
            "[package]\nname = \"facade\"\n[dependencies]\ncore = { path = \"../core\" }\n",
        ).unwrap();
        fs::write(core.join(MANIFEST), "[package]\nname = \"core\"\n").unwrap();
        fs::write(app.join("src/main.fusion"), "main:\n    print(1)\n").unwrap();
        fs::write(facade.join("src/lib.fusion"), "pub const VALUE = 2\n").unwrap();
        fs::write(core.join("src/lib.fusion"), "pub const BASE = 1\n").unwrap();

        let project = load_project(&app).unwrap();
        let graph = package_graph::resolve_package_graph(&project).unwrap();
        let compiler = Path::new("fusion-test-compiler");
        let first = package_build_fingerprints(&graph, compiler, BuildProfile::Debug, &[]).unwrap();
        let app_root = app.canonicalize().unwrap();
        let facade_root = facade.canonicalize().unwrap();
        let core_root = core.canonicalize().unwrap();

        // A transitive source edit changes the core key and propagates through
        // facade to the root package, while the package graph remains stable.
        fs::write(core.join("src/lib.fusion"), "pub const BASE = 99\n").unwrap();
        let second_graph = package_graph::resolve_package_graph(&project).unwrap();
        let second = package_build_fingerprints(&second_graph, compiler, BuildProfile::Debug, &[]).unwrap();
        assert_ne!(first[&core_root], second[&core_root]);
        assert_ne!(first[&facade_root], second[&facade_root]);
        assert_ne!(first[&app_root], second[&app_root]);

        // Package settings are part of the key even if the source text is stable.
        fs::write(core.join(MANIFEST), "[package]\nname = \"core\"\nversion = \"0.2.0\"\n").unwrap();
        let third_graph = package_graph::resolve_package_graph(&project).unwrap();
        let third = package_build_fingerprints(&third_graph, compiler, BuildProfile::Debug, &[]).unwrap();
        assert_ne!(second[&core_root], third[&core_root]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn semver_ranges_are_resolved_conservatively() {
        assert!(registry::satisfies("1.4.2", "^1.2"));
        assert!(!registry::satisfies("2.0.0", "^1.2"));
        assert!(registry::satisfies("1.2.9", "~1.2"));
        assert!(!registry::satisfies("1.3.0", "~1.2"));
        assert!(registry::satisfies("1.2.0", ">=1.2"));
    }

    #[test]
    fn dependency_update_is_rejected_in_offline_mode() {
        let root = std::env::temp_dir().join(format!("foundry-offline-update-{}", std::process::id()));
        let project = Project { root, manifest: Manifest::default() };
        let error = resolve_dependencies_with_update(&project, true, true).unwrap_err();
        assert!(error.contains("requires online dependency metadata"));
    }

    #[test]
    fn manifest_parser_uses_toml_semantics_for_inline_tables_and_escapes() {
        let m = parse_manifest(r#"
[package]
name = "demo"
description = "line one # not a comment"

[dependencies]
path_dep = { path = "../libs/a,b" }
git_dep = { git = "https://example.invalid/repo.git", tag = "v1.2.0" }
"#).unwrap();
        assert_eq!(m.package.description.as_deref(), Some("line one # not a comment"));
        assert!(matches!(m.dependencies["path_dep"], Dependency::Path(_)));
        assert!(matches!(&m.dependencies["git_dep"], Dependency::Git { rev: Some(tag), .. } if tag == "v1.2.0"));
    }

    #[test]
    fn manifest_rejects_multiple_dependency_sources_and_unsafe_names() {
        let multiple = "[package]\nname = \"demo\"\n[dependencies]\nx = { path = \"../x\", version = \"1\" }\n";
        assert!(parse_manifest(multiple).is_err());
        let unsafe_name = "[package]\nname = \"demo\"\n[dependencies]\n\"../escape\" = \"1\"\n";
        assert!(parse_manifest(unsafe_name).is_err());
    }

    #[test]
    fn manifest_rejects_duplicate_aliases_across_dependency_sections() {
        let input = r#"[package]
name = "demo"
[dependencies]
shared = "^1"
[dev-dependencies]
shared = "^2"
"#;
        assert!(parse_manifest(input).unwrap_err().contains("declared in both [dependencies] and [dev-dependencies]"));
    }

    #[test]
    fn package_link_validation_accepts_matching_provider_and_consumer_abi() {
        let root = std::env::temp_dir().join(format!("foundry-link-abi-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("math.ll");
        let interface = root.join("math.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "define i64 @_Fmath_add(i64 %a, i64 %b) {\n ret i64 0\n}\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-function\tmath\tadd\t_Fmath_add\ti64\ti64,i64\n").unwrap();
        fs::write(&app, "declare i64 @_Fmath_add(i64, i64)\ndefine i32 @main() {\n %x = call i64 @_Fmath_add(i64 1, i64 2)\n ret i32 0\n}\n").unwrap();
        assert!(validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_accepts_exported_scalar_data_global() {
        let root = std::env::temp_dir().join(format!("foundry-link-data-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("math.ll");
        let interface = root.join("math.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "@_Fmath_limit = constant i64 42, align 8\ndefine i64 @_Fmath_get() {\n ret i64 42\n}\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-data\tmath\tLIMIT\t_Fmath_limit\ti64\tinteger\t42\n").unwrap();
        fs::write(&app, "@_Fmath_limit = external constant i64\ndefine i32 @main() {\n %v = load i64, ptr @_Fmath_limit\n ret i32 0\n}\n").unwrap();
        assert!(validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_allows_root_private_scalar_data_from_entrypoint() {
        let root = std::env::temp_dir().join(format!("foundry-link-data-root-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("app.ll");
        let interface = root.join("app.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "@_Fapp_secret = constant i64 9, align 8\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-internal-data\tapp\tSECRET\t_Fapp_secret\ti64\tinteger\t9\n").unwrap();
        fs::write(&app, "@_Fapp_secret = external constant i64\ndefine i32 @main() {\n %v = load i64, ptr @_Fapp_secret\n ret i32 0\n}\n").unwrap();
        assert!(validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_rejects_scalar_data_type_mismatch() {
        let root = std::env::temp_dir().join(format!("foundry-link-data-drift-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("math.ll");
        let interface = root.join("math.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "@_Fmath_limit = constant i64 42, align 8\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-data\tmath\tLIMIT\t_Fmath_limit\ti64\tinteger\t42\n").unwrap();
        fs::write(&app, "@_Fmath_limit = external constant ptr\ndefine i32 @main() {\n %v = load ptr, ptr @_Fmath_limit\n ret i32 0\n}\n").unwrap();
        let error = validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).unwrap_err();
        assert!(error.contains("cross-package data ABI mismatch"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_rejects_dependency_private_data() {
        let root = std::env::temp_dir().join(format!("foundry-link-data-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("dep.ll");
        let interface = root.join("dep.fpi");
        let consumer = root.join("consumer.ll");
        let consumer_interface = root.join("consumer.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "@_Fdep_secret = constant i64 7, align 8\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-internal-data\tdep\tSECRET\t_Fdep_secret\ti64\tinteger\t7\n").unwrap();
        fs::write(&consumer, "@_Fdep_secret = external constant i64\ndefine i64 @_Fconsumer_read() {\n %v = load i64, ptr @_Fdep_secret\n ret i64 %v\n}\n").unwrap();
        fs::write(&consumer_interface, "FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        fs::write(&app, "define i32 @main() {\n ret i32 0\n}\n").unwrap();
        let error = validate_package_link_inputs(&[(package, interface.clone()), (consumer, consumer_interface)], &app, &root.join("not-root.fpi")).unwrap_err();
        assert!(error.contains("private package data"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_rejects_consumer_signature_drift() {
        let root = std::env::temp_dir().join(format!("foundry-link-abi-drift-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("math.ll");
        let interface = root.join("math.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "define i64 @_Fmath_add(i64 %a) {\n ret i64 0\n}\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-function\tmath\tadd\t_Fmath_add\ti64\ti64\n").unwrap();
        fs::write(&app, "declare ptr @_Fmath_add(ptr)\ndefine i32 @main() {\n %x = call ptr @_Fmath_add(ptr null)\n ret i32 0\n}\n").unwrap();
        let error = validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).unwrap_err();
        assert!(error.contains("cross-package ABI mismatch"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_rejects_abi_without_matching_definition() {
        let root = std::env::temp_dir().join(format!("foundry-link-abi-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("math.ll");
        let interface = root.join("math.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "; empty package module\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-function\tmath\tadd\t_Fmath_add\ti64\ti64\n").unwrap();
        fs::write(&app, "define i32 @main() {\n ret i32 0\n}\n").unwrap();
        let error = validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).unwrap_err();
        assert!(error.contains("does not define it"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_rejects_unprovided_fusion_declarations() {
        let root = std::env::temp_dir().join(format!("foundry-link-abi-unresolved-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("consumer.ll");
        let interface = root.join("consumer.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "declare i64 @_Fmissing_call(i64)\ndefine i64 @_Fconsumer_use(i64 %a) {\n %x = call i64 @_Fmissing_call(i64 %a)\n ret i64 %x\n}\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        fs::write(&app, "define i32 @main() {\n ret i32 0\n}\n").unwrap();
        let error = validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).unwrap_err();
        assert!(error.contains("no package exports a supported ABI"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_rejects_duplicate_definitions() {
        let root = std::env::temp_dir().join(format!("foundry-link-abi-duplicate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let one = root.join("one.ll");
        let two = root.join("two.ll");
        let one_fpi = root.join("one.fpi");
        let two_fpi = root.join("two.fpi");
        let app = root.join("entry.ll");
        let definition = "define i64 @_Fmath_add(i64 %a) {\n ret i64 0\n}\n";
        fs::write(&one, definition).unwrap();
        fs::write(&two, definition).unwrap();
        let interface = "FUSION-PACKAGE-INTERFACE\t6\nabi-function\tmath\tadd\t_Fmath_add\ti64\ti64\n";
        fs::write(&one_fpi, interface).unwrap();
        fs::write(&two_fpi, interface).unwrap();
        fs::write(&app, "define i32 @main() {\n ret i32 0\n}\n").unwrap();
        let error = validate_package_link_inputs(&[(one, one_fpi.clone()), (two, two_fpi)], &app, &one_fpi).unwrap_err();
        assert!(error.contains("duplicate LLVM definition"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_allows_root_private_symbols_from_entrypoint() {
        let root = std::env::temp_dir().join(format!("foundry-link-private-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("app.ll");
        let interface = root.join("app.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "define i64 @_Fapp_helper(i64 %a) {\n ret i64 0\n}\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-internal-function\tapp\thelper\t_Fapp_helper\ti64\ti64\n").unwrap();
        fs::write(&app, "declare i64 @_Fapp_helper(i64)\ndefine i32 @main() {\n %x = call i64 @_Fapp_helper(i64 1)\n ret i32 0\n}\n").unwrap();
        assert!(validate_package_link_inputs(&[(package, interface.clone())], &app, &interface).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_link_validation_rejects_dependency_private_symbols() {
        let root = std::env::temp_dir().join(format!("foundry-link-private-dep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("dep.ll");
        let interface = root.join("dep.fpi");
        let consumer = root.join("consumer.ll");
        let consumer_interface = root.join("consumer.fpi");
        let app = root.join("entry.ll");
        fs::write(&package, "define i64 @_Fdep_helper(i64 %a) {\n ret i64 0\n}\n").unwrap();
        fs::write(&interface, "FUSION-PACKAGE-INTERFACE\t6\nabi-internal-function\tdep\thelper\t_Fdep_helper\ti64\ti64\n").unwrap();
        fs::write(&consumer, "declare i64 @_Fdep_helper(i64)\ndefine i64 @_Fconsumer_use(i64 %a) {\n %x = call i64 @_Fdep_helper(i64 %a)\n ret i64 %x\n}\n").unwrap();
        fs::write(&consumer_interface, "FUSION-PACKAGE-INTERFACE\t6\n").unwrap();
        fs::write(&app, "define i32 @main() {\n ret i32 0\n}\n").unwrap();
        let error = validate_package_link_inputs(&[(package, interface.clone()), (consumer, consumer_interface)], &app, &root.join("non-root.fpi")).unwrap_err();
        assert!(error.contains("private package symbol"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clean_preserves_dependency_cache() {
        let root = std::env::temp_dir().join(format!("foundry-clean-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join("target/foundry/deps")).unwrap();
        fs::write(root.join("target/debug/app"), "artifact").unwrap();
        fs::write(root.join("target/foundry/deps/keep"), "cached").unwrap();
        let project = Project { root: root.clone(), manifest: Manifest::default() };
        clean(&project).unwrap();
        assert!(!root.join("target/debug").exists());
        assert!(root.join("target/foundry/deps/keep").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_rejects_unsafe_package_name() {
        let input = r#"[package]
name = "../escape"
"#;
        assert!(parse_manifest(input).is_err());
    }

    #[test]
    fn manifest_parses() {
        let m=parse_manifest("[package]\nname=\"x\"\n[dependencies]\na=\"1\"\nb={ path=\"../b\" }\nc={ git=\"https://x/c.git\", rev=\"abc\" }\n").unwrap();
        assert_eq!(m.package.name, "x");
        assert!(matches!(m.dependencies["a"], Dependency::Version(_)));
        assert!(matches!(m.dependencies["b"], Dependency::Path(_)));
        assert!(matches!(m.dependencies["c"], Dependency::Git { .. }));
    }

    #[test]
    fn lockfile_parser_rejects_duplicate_aliases_and_incomplete_records() {
        let root = std::env::temp_dir().join(format!("foundry-lock-parse-{}", std::process::id()));
        let _ = fs::remove_file(&root);
        let manifest_hash = "c".repeat(64);
        let valid = format!("version = 3\nmanifest-sha256 = \"{manifest_hash}\"\n\n[[package]]\nname = \"dep\"\nrequested = \"^1\"\nversion = \"1.2.0\"\nsource = \"registry\"\nrevision = \"\"\nchecksum = \"{}\"\nregistry = \"https://registry.example\"\n", "a".repeat(64));
        fs::write(&root, &valid).unwrap();
        let parsed = read_validated_lock(&root, &manifest_hash).unwrap();
        assert_eq!(parsed["dep"].version, "1.2.0");
        fs::write(&root, format!("{valid}\n[[package]]\nname = \"dep\"\nrequested = \"^1\"\nversion = \"1.2.1\"\nsource = \"registry\"\nrevision = \"\"\nchecksum = \"{}\"\nregistry = \"https://registry.example\"\n", "b".repeat(64))).unwrap();
        assert!(read_validated_lock(&root, &manifest_hash).unwrap_err().contains("duplicate package alias"));
        fs::write(&root, format!("version = 3\nmanifest-sha256 = \"{manifest_hash}\"\n\n[[package]]\nname = \"broken\"\n")).unwrap();
        assert!(read_validated_lock(&root, &manifest_hash).unwrap_err().contains("missing 'requested'"));
        fs::write(&root, valid).unwrap();
        assert!(read_validated_lock(&root, &"d".repeat(64)).unwrap_err().contains("manifest changed"));
        fs::write(&root, "version = 3\n").unwrap();
        assert!(read_validated_lock(&root, &"c".repeat(64)).unwrap_err().contains("missing a string 'manifest-sha256'"));
        let _ = fs::remove_file(root);
    }

    #[test]
    fn path_dependency_drift_requires_an_explicit_lock_update() {
        let root = std::env::temp_dir().join(format!("foundry-path-lock-drift-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dependency = root.join("dep");
        fs::create_dir_all(dependency.join("src")).unwrap();
        fs::write(root.join(MANIFEST), "[package]\nname = \"app\"\n[dependencies]\ndep = { path = \"dep\" }\n").unwrap();
        fs::write(dependency.join("src/lib.fusion"), "pub const VALUE = 1\n").unwrap();
        let project = Project { root: root.clone(), manifest: parse_manifest("[package]\nname = \"app\"\n[dependencies]\ndep = { path = \"dep\" }\n").unwrap() };
        write_lock(&project).unwrap();
        let lock_path = root.join(LOCKFILE);
        let saved_lock = fs::read(&lock_path).unwrap();
        fs::remove_file(&lock_path).unwrap();
        let missing = resolve_dependencies_one(&project, true, false, true).unwrap_err();
        assert!(missing.contains("has no lockfile entry (--offline)"));
        fs::write(&lock_path, saved_lock).unwrap();
        // Generated dependency lockfiles are metadata, not source-tree content;
        // creating one must not invalidate a parent's path dependency digest.
        fs::write(dependency.join(LOCKFILE), "version = 3\n").unwrap();
        assert!(resolve_dependencies_one(&project, true, false, true).is_ok());
        fs::write(dependency.join("src/lib.fusion"), "pub const VALUE = 2\n").unwrap();
        let error = resolve_dependencies_one(&project, true, false, true).unwrap_err();
        assert!(error.contains("path dependency 'dep' changed"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn lockfile_publication_is_atomic_when_commit_fails() {
        let root = std::env::temp_dir().join(format!("foundry-lock-atomic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("foundry.lock");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("sentinel"), "old-state").unwrap();
        assert!(write_text_atomically(&destination, "new lock contents").is_err());
        assert_eq!(fs::read_to_string(destination.join("sentinel")).unwrap(), "old-state");
        let leftovers = fs::read_dir(&root).unwrap().filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains("tmp"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_origin_is_written_into_lockfile() {
        let root = std::env::temp_dir().join(format!("foundry-lock-registry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MANIFEST), "[package]\nname = \"app\"\n[dependencies]\ndemo = \"^1.0\"\n").unwrap();
        let project = Project { root: root.clone(), manifest: parse_manifest("[package]\nname = \"app\"\n[dependencies]\ndemo = \"^1.0\"\n").unwrap() };
        let resolved = vec![registry::ResolvedPackage { name: "demo".into(), requested: "^1.0".into(), version: "1.2.3".into(), source: "unused".into(), registry_url: "https://registry.example/".into(), checksum: Some("a".repeat(64)) }];
        write_resolved_lock(&project, &resolved, true).unwrap();
        let text = fs::read_to_string(root.join(LOCKFILE)).unwrap();
        assert!(text.contains("registry = \"https://registry.example\""));
        let manifest_hash = fingerprint::fingerprint_fields([fs::read(root.join(MANIFEST)).unwrap().as_slice()]);
        let lock = read_validated_lock(&root.join(LOCKFILE), &manifest_hash).unwrap();
        assert_eq!(lock["demo"].registry, "https://registry.example");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lockfile_rejects_aliases_not_declared_by_the_manifest() {
        let locked = BTreeMap::from([("stale".to_owned(), LockedPackage {
            requested: "^1".into(),
            version: "1.2.0".into(),
            source: "path+/tmp/stale".into(),
            revision: "a".repeat(64),
            checksum: String::new(),
            registry: String::new(),
        })]);
        let manifest = Manifest::default();
        let error = validate_lock_aliases(&locked, &manifest, Path::new("foundry.lock")).unwrap_err();
        assert!(error.contains("stale dependency alias 'stale'"));
    }

    #[test]
    fn git_lock_entries_require_full_commit_ids() {
        let path = Path::new("foundry.lock");
        let invalid = LockedPackage {
            requested: "main".into(),
            version: "main".into(),
            source: "git+https://example.com/dep.git".into(),
            revision: "main".into(),
            checksum: String::new(),
            registry: String::new(),
        };
        let error = validate_locked_package("dep", &invalid, path).unwrap_err();
        assert!(error.contains("full immutable commit ID"));

        let valid = LockedPackage { revision: "a".repeat(40), ..invalid };
        assert!(validate_locked_package("dep", &valid, path).is_ok());
    }

    #[test]
    fn lockfile_alias_must_match_declared_dependency_source() {
        let manifest = parse_manifest(
            "[package]\nname = \"app\"\n[dependencies]\ndep = { git = \"https://example.com/real.git\", rev = \"main\" }\n"
        ).unwrap();
        let locked = BTreeMap::from([("dep".to_owned(), LockedPackage {
            requested: "main".into(),
            version: "main".into(),
            source: "git+https://example.com/attacker.git".into(),
            revision: "a".repeat(40),
            checksum: String::new(),
            registry: String::new(),
        })]);
        let error = validate_lock_aliases(&locked, &manifest, Path::new("foundry.lock")).unwrap_err();
        assert!(error.contains("does not match its dependency alias/source/selector"));
    }

    #[test]
    fn registry_origin_changes_require_an_explicit_lock_update() {
        let entry = LockedPackage {
            requested: "^1.0".into(),
            version: "1.2.3".into(),
            source: "registry".into(),
            revision: String::new(),
            checksum: "a".repeat(64),
            registry: "https://registry-a.example".into(),
        };
        assert!(validate_registry_origin("demo", &entry, "https://registry-a.example").is_ok());
        let error = validate_registry_origin("demo", &entry, "https://registry-b.example").unwrap_err();
        assert!(error.contains("does not match FOUNDRY_REGISTRY"));
    }

    #[test]
    fn offline_registry_resolution_requires_an_exact_lock_entry() {
        let error = exact_registry_pin("serde", None, true).unwrap_err();
        assert!(error.contains("no exact locked version and checksum"));
        assert!(exact_registry_pin("serde", None, false).unwrap().is_none());
    }

    #[test]
    fn dependency_free_lockfile_is_valid() {
        let root = std::env::temp_dir().join(format!("foundry-empty-lock-{}", std::process::id()));
        let _ = fs::remove_file(&root);
        fs::write(&root, format!("version = 3\nmanifest-sha256 = \"{}\"\n", "e".repeat(64))).unwrap();
        assert!(read_validated_lock(&root, &"e".repeat(64)).unwrap().is_empty());
        let _ = fs::remove_file(root);
    }

    #[test]
    fn lockfile_rejects_unknown_sources() {
        let root = std::env::temp_dir().join(format!("foundry-lock-source-{}", std::process::id()));
        let _ = fs::remove_file(&root);
        let invalid = format!("version = 3\nmanifest-sha256 = \"{}\"\n\n[[package]]\nname = \"dep\"\nrequested = \"^1\"\nversion = \"1.2.0\"\nsource = \"other\"\nrevision = \"\"\nchecksum = \"\"\nregistry = \"\"\n", "f".repeat(64));
        fs::write(&root, invalid).unwrap();
        assert!(read_validated_lock(&root, &"f".repeat(64)).unwrap_err().contains("unsupported source"));
        let _ = fs::remove_file(root);
    }

    #[test]
    fn git_commit_pins_accept_sha1_and_sha256_object_ids() {
        assert!(is_git_commit("0123456789abcdef0123456789abcdef01234567"));
        assert!(is_git_commit("0123456789abcdef0123456789abcdef012345670123456789abcdef01234567"));
        assert!(!is_git_commit("main"));
        assert!(!is_git_commit("0123456789abcdef"));
    }

    #[test]
    fn git_selector_resolves_remote_branches_and_default_branch_to_commits() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("foundry-git-lock-{}-{nonce}", std::process::id()));
        let bare = root.join("origin.git");
        let work = root.join("work");
        let cache = root.join("cache");
        fs::create_dir_all(&root).unwrap();
        let run_git = |cwd: &Path, args: &[&str]| {
            let status = Command::new("git").current_dir(cwd).args(args).status().unwrap();
            assert!(status.success(), "git {:?} failed in {}", args, cwd.display());
        };
        run_git(&root, &["init", "--bare", bare.to_str().unwrap()]);
        run_git(&root, &["init", work.to_str().unwrap()]);
        run_git(&work, &["config", "user.email", "fusion-tests@example.invalid"]);
        run_git(&work, &["config", "user.name", "Fusion tests"]);
        fs::write(work.join("file.txt"), "main\n").unwrap();
        run_git(&work, &["add", "file.txt"]);
        run_git(&work, &["commit", "-m", "main"]);
        run_git(&work, &["branch", "-M", "main"]);
        let main_commit = git_head(&work).unwrap();
        run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        run_git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        run_git(&work, &["push", "-u", "origin", "main"]);
        run_git(&work, &["checkout", "-b", "feature/phase3"]);
        fs::write(work.join("file.txt"), "feature\n").unwrap();
        run_git(&work, &["commit", "-am", "feature"]);
        let feature_commit = git_head(&work).unwrap();
        run_git(&work, &["push", "origin", "refs/heads/feature/phase3:refs/heads/feature/phase3"]);
        run_git(&root, &["clone", bare.to_str().unwrap(), cache.to_str().unwrap()]);
        run_git(&cache, &["fetch", "--all", "--tags"]);

        assert_eq!(git_resolve_commit(&cache, Some("feature/phase3")).unwrap(), feature_commit);
        assert_eq!(git_resolve_commit(&cache, None).unwrap(), main_commit);
        run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/feature/phase3"]);
        refresh_git_default_branch(&cache);
        assert_eq!(git_resolve_commit(&cache, None).unwrap(), feature_commit);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_registry_pin_validation_accepts_one_shared_version() {
        let entries = vec![
            ("app".to_string(), "^1.2.0".to_string(), "1.8.0".to_string()),
            ("ui".to_string(), ">=1.5.0, <2.0.0".to_string(), "1.8.0".to_string()),
        ];
        assert!(validate_workspace_registry_pin_group("shared", &entries).is_ok());
    }

    #[test]
    fn workspace_registry_pin_validation_rejects_mixed_versions() {
        let entries = vec![
            ("app".to_string(), "^1.0.0".to_string(), "1.9.0".to_string()),
            ("legacy".to_string(), "^1.5.0".to_string(), "1.7.0".to_string()),
        ];
        let error = validate_workspace_registry_pin_group("shared", &entries).unwrap_err();
        assert!(error.contains("app requires ^1.0.0 (locked 1.9.0)"));
        assert!(error.contains("legacy requires ^1.5.0 (locked 1.7.0)"));
    }

    #[test]
    fn workspace_registry_pin_validation_rejects_incompatible_requirement() {
        let entries = vec![
            ("app".to_string(), "^1.0.0".to_string(), "1.9.0".to_string()),
            ("next".to_string(), "^2.0.0".to_string(), "1.9.0".to_string()),
        ];
        let error = validate_workspace_registry_pin_group("shared", &entries).unwrap_err();
        assert!(error.contains("no single locked version"));
    }
}

#[cfg(test)]
mod dependency_materialization_tests {
    use super::materialize_package_alias;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn registry_package_is_materialized_at_manifest_alias() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("foundry-registry-alias-{}-{nonce}", std::process::id()));
        let extracted = root.join("registry-name-1.2.3");
        let alias = root.join("short-alias");
        fs::create_dir_all(extracted.join("src")).unwrap();
        fs::write(extracted.join("src/lib.fusion"), "pub const VALUE = 7\n").unwrap();

        materialize_package_alias(&extracted, &alias).unwrap();
        assert_eq!(fs::read_to_string(alias.join("src/lib.fusion")).unwrap(), "pub const VALUE = 7\n");

        let _ = fs::remove_dir_all(root);
    }
}
