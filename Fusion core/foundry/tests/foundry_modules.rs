//contents of foundry_modules.rs

use foundry::{
    graph::{GraphError, TaskGraph},
    manifest::ProjectManifest,
    plugin::{Capability, PluginDescriptor, PluginKind},
};
use std::collections::BTreeSet;
use std::str::FromStr;

#[test]
fn parses_fusion_manifest() {
    let input = r#"
[package]
name = "demo"

[dependencies]
math = { path = "../math" }
serde = "1"

[plugins]
c = { version = "0.1" }
"#;

    let manifest = ProjectManifest::from_str(input).unwrap();

    assert_eq!(manifest.package.name, "demo");
    assert_eq!(manifest.package.version, "0.1.0");
    assert!(manifest.dependencies.contains_key("math"));
}

#[test]
fn rejects_dependency_with_multiple_sources() {
    let input = r#"
[package]
name = "demo"

[dependencies]
invalid = { version = "1", git = "https://example.com/repo.git" }
"#;

    assert!(ProjectManifest::from_str(input).is_err());
}

#[test]
fn orders_build_tasks() {
    let mut graph = TaskGraph::default();

    graph.add_task("link", ["compile"]).unwrap();
    graph.add_task("compile", ["parse"]).unwrap();
    graph
        .add_task("parse", std::iter::empty::<String>())
        .unwrap();

    assert_eq!(
        graph.execution_order().unwrap(),
        vec!["parse", "compile", "link"]
    );
}

#[test]
fn detects_build_cycles() {
    let mut graph = TaskGraph::default();

    graph.add_task("compile-a", ["compile-b"]).unwrap();
    graph.add_task("compile-b", ["compile-a"]).unwrap();

    assert!(matches!(graph.execution_order(), Err(GraphError::Cycle(_))));
}

#[test]
fn validates_ecosystem_plugins() {
    let plugin = PluginDescriptor {
        name: "c".into(),
        version: "0.1.0".into(),
        kind: PluginKind::Ecosystem,
        capabilities: BTreeSet::from([Capability::BuildSource, Capability::LinkArtifact]),
        requires: BTreeSet::from(["fusion-interop-model".into()]),
    };

    assert!(plugin.validate(&BTreeSet::new()).is_err());

    let installed = BTreeSet::from(["fusion-interop-model".into()]);

    assert!(plugin.validate(&installed).is_ok());
}
