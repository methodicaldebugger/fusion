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

#[test]
fn discovers_nested_sources_and_multifile_integration_tests() {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("foundry-multi-module-{}-{nonce}", std::process::id()));
    fs::create_dir_all(root.join("src/math")).unwrap();
    fs::create_dir_all(root.join("tests/integration")).unwrap();
    fs::write(root.join("Foundry.toml"), "[package]\nname = \"multi_module_demo\"\nversion = \"0.1.0\"\n").unwrap();
    fs::write(root.join("src/main.fusion"), "import math\nmain:\n    print(double(21))\n").unwrap();
    fs::write(root.join("src/math/mod.fusion"), "pub fn double(value: num) -> num:\n    return value * 2\n").unwrap();
    fs::write(root.join("tests/integration/modules.fusion"), "main:\n    print(42)\n").unwrap();

    let project = foundry::load_project(&root).unwrap();
    let sources = foundry::source_files(&project);
    let tests = foundry::test_files(&project);
    assert!(foundry::main_source(&project).unwrap().ends_with("src/main.fusion"));
    assert!(sources.iter().any(|p| p.ends_with("src/main.fusion")));
    assert!(sources.iter().any(|p| p.ends_with("src/math/mod.fusion")));
    assert!(tests.iter().any(|p| p.ends_with("tests/integration/modules.fusion")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolves_path_dependency_package_graph_in_dependency_first_order() {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("foundry-package-graph-{}-{nonce}", std::process::id()));
    let app = root.join("app");
    let math = root.join("math");
    let core = root.join("core");
    for package in [&app, &math, &core] { fs::create_dir_all(package.join("src")).unwrap(); }
    fs::write(app.join("Foundry.toml"), "[package]\nname = \"app\"\n[dependencies]\nmath = { path = \"../math\" }\n").unwrap();
    fs::write(math.join("Foundry.toml"), "[package]\nname = \"math\"\n[dependencies]\ncore = { path = \"../core\" }\n").unwrap();
    fs::write(core.join("Foundry.toml"), "[package]\nname = \"core\"\n").unwrap();
    fs::write(app.join("src/main.fusion"), "main:\n    print(1)\n").unwrap();
    fs::write(math.join("src/lib.fusion"), "pub fn double(x: num) -> num:\n    return x * 2\n").unwrap();
    fs::write(core.join("src/lib.fusion"), "pub const ANSWER = 42\n").unwrap();

    let project = foundry::load_project(&app).unwrap();
    let graph = foundry::package_graph::resolve_package_graph(&project).unwrap();
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.build_order, vec![core.canonicalize().unwrap(), math.canonicalize().unwrap(), app.canonicalize().unwrap()]);
    assert!(graph.nodes[&math.canonicalize().unwrap()].entry.as_ref().unwrap().ends_with("lib.fusion"));
    assert_eq!(graph.edges.len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_path_dependency_package_cycles_with_a_readable_chain() {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("foundry-package-cycle-{}-{nonce}", std::process::id()));
    let a = root.join("a");
    let b = root.join("b");
    for package in [&a, &b] { fs::create_dir_all(package.join("src")).unwrap(); }
    fs::write(a.join("Foundry.toml"), "[package]\nname = \"a\"\n[dependencies]\nb = { path = \"../b\" }\n").unwrap();
    fs::write(b.join("Foundry.toml"), "[package]\nname = \"b\"\n[dependencies]\na = { path = \"../a\" }\n").unwrap();
    fs::write(a.join("src/main.fusion"), "main:\n    print(1)\n").unwrap();
    fs::write(b.join("src/lib.fusion"), "pub const VALUE = 2\n").unwrap();

    let project = foundry::load_project(&a).unwrap();
    let error = foundry::package_graph::resolve_package_graph(&project).unwrap_err().to_string();
    assert!(error.contains("package dependency cycle detected"));
    assert!(error.contains("a"));
    assert!(error.contains("b"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolves_transitive_path_dependencies_and_writes_each_package_lockfile() {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("foundry-transitive-resolve-{}-{nonce}", std::process::id()));
    let app = root.join("app");
    let facade = root.join("facade");
    let core = root.join("core");
    let test_helper = root.join("test_helper");
    for package in [&app, &facade, &core, &test_helper] { fs::create_dir_all(package.join("src")).unwrap(); }
    fs::write(app.join("Foundry.toml"), "[package]\nname = \"app\"\n[dependencies]\nfacade = { path = \"../facade\" }\n").unwrap();
    fs::write(facade.join("Foundry.toml"), "[package]\nname = \"facade\"\n[dependencies]\ncore = { path = \"../core\" }\n[dev-dependencies]\ntest_helper = { path = \"../test_helper\" }\n").unwrap();
    fs::write(core.join("Foundry.toml"), "[package]\nname = \"core\"\n").unwrap();
    fs::write(test_helper.join("Foundry.toml"), "[package]\nname = \"test_helper\"\n").unwrap();
    fs::write(app.join("src/main.fusion"), "main:\n    print(1)\n").unwrap();
    fs::write(facade.join("src/lib.fusion"), "pub const VALUE = 2\n").unwrap();
    fs::write(core.join("src/lib.fusion"), "pub const BASE = 1\n").unwrap();

    let project = foundry::load_project(&app).unwrap();
    foundry::resolve_dependencies(&project, true).unwrap();
    // Resolve a second time to exercise the just-written lockfiles, including
    // the header-only lockfile of the dependency-free leaf package.
    foundry::resolve_dependencies(&project, true).unwrap();
    assert!(app.join("foundry.lock").is_file());
    assert!(facade.join("foundry.lock").is_file());
    assert!(core.join("foundry.lock").is_file());
    let facade_lock = fs::read_to_string(facade.join("foundry.lock")).unwrap();
    assert!(facade_lock.contains("core"));
    assert!(!facade_lock.contains("test_helper"), "a dependency package must not resolve or lock its own dev-dependencies");
    let graph = foundry::package_graph::resolve_package_graph(&project).unwrap();
    assert_eq!(graph.nodes.len(), 3);
    let _ = fs::remove_dir_all(root);
}
