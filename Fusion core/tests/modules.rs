use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct ProjectDir(PathBuf);
impl ProjectDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fusion-project-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn entry(&self) -> PathBuf {
        self.0.join("main.fusion")
    }
}
impl Drop for ProjectDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn module_imports_cannot_escape_the_project_source_root_through_symlinks() {
    use std::os::unix::fs::symlink;

    let p = ProjectDir::new();
    let outside = p.0.parent().unwrap().join(format!(
        "fusion-outside-module-{}-{}.fusion",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&outside, "pub fn secret() -> num:\n    return 7\n").unwrap();
    symlink(&outside, p.0.join("escape.fusion")).unwrap();
    p.write("main.fusion", "import escape\nmain:\n    print(1)\n");

    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("resolves outside its package source root"),
        "unexpected diagnostic: {error}"
    );
    let _ = fs::remove_file(outside);
}

#[test]
fn project_compiles_and_runs_public_imported_function() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import math\nmain:\n    print(double(21))\n");
    p.write(
        "math.fusion",
        "pub fn double(value: num) -> num:\n    return value * 2\n",
    );
    let program = fusion::compile_project(p.entry()).expect("project should compile");
    assert!(program
        .modules
        .iter()
        .any(|m| m.name == "math" && m.public_symbols.iter().any(|s| s == "double")));
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn project_rejects_private_function_access() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import math\nmain:\n    print(secret())\n");
    p.write("math.fusion", "fn secret() -> num:\n    return 42\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(error.contains("privacy violation"), "{error}");
}

#[test]
fn same_function_names_are_isolated_by_module_namespace() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import alpha\nimport beta\nmain:\n    print(alpha.helper())\n    print(beta.helper())\n",
    );
    p.write("alpha.fusion", "pub fn helper() -> num:\n    return 1\n");
    p.write("beta.fusion", "pub fn helper() -> num:\n    return 2\n");
    let program =
        fusion::compile_project(p.entry()).expect("same local names in separate modules are valid");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["1".to_string(), "2".to_string()]);
}

#[test]
fn import_alias_resolves_qualified_function_calls() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import math as m\nmain:\n    print(m.double(21))\n",
    );
    p.write(
        "math.fusion",
        "pub fn double(value: num) -> num:\n    return value * 2\n",
    );
    let program = fusion::compile_project(p.entry()).expect("qualified alias call should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn ambiguous_unqualified_import_requires_qualification() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import alpha\nimport beta\nmain:\n    print(helper())\n",
    );
    p.write("alpha.fusion", "pub fn helper() -> num:\n    return 1\n");
    p.write("beta.fusion", "pub fn helper() -> num:\n    return 2\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("ambiguous imported function 'helper'"),
        "{error}"
    );
}

#[test]
fn project_exposes_reexport_metadata() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import facade\nmain:\n    print(1)\n");
    p.write("facade.fusion", "pub import math\n");
    p.write("math.fusion", "pub fn value() -> num:\n    return 7\n");
    let project = fusion::module_system::load_project_with_metadata(p.entry()).unwrap();
    assert!(
        project
            .modules
            .iter()
            .any(|m| m.name == "facade"
                && m.imports.iter().any(|i| i.path == "math" && i.re_exported))
    );
}

#[test]
fn unimported_module_symbols_are_not_accidentally_visible() {
    let p = ProjectDir::new();
    p.write("main.fusion", "main:\n    print(secret())\n");
    p.write("helper.fusion", "pub fn secret() -> num:\n    return 42\n");
    // The helper is intentionally not imported and therefore is not loaded as
    // part of the project; this test also documents that imports define scope.
    let result = fusion::compile_project(p.entry());
    assert!(
        result.is_err(),
        "a declaration in an unimported file must not become visible: {result:?}"
    );
}

#[test]
fn privacy_validation_does_not_cross_unrelated_module_boundaries() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import alpha\nmain:\n    print(public_value())\n",
    );
    p.write(
        "alpha.fusion",
        "import beta\npub fn public_value() -> num:\n    return 1\n",
    );
    p.write("beta.fusion", "fn secret() -> num:\n    return 2\n");
    // beta is imported by alpha but its private declaration is not referenced.
    let result = fusion::compile_project(p.entry());
    assert!(
        result.is_ok(),
        "unrelated module declarations must not trigger privacy checks: {result:?}"
    );
}

#[test]
fn compile_project_can_return_the_structured_module_graph() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import math\nmain:\n    print(double(2))\n");
    p.write(
        "math.fusion",
        "pub fn double(value: num) -> num:\n    return value * 2\n",
    );
    let project = fusion::compile_project_with_metadata(p.entry()).unwrap();
    assert_eq!(project.entry_module, "crate");
    assert!(project.modules.iter().any(|module| module.name == "math"));
    assert!(project
        .program
        .modules
        .iter()
        .any(|module| module.name == "math"));
}

#[test]
fn import_alias_is_preserved_in_module_metadata() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import math as m\nmain:\n    print(1)\n");
    p.write(
        "math.fusion",
        "pub fn double(value: num) -> num:\n    return value * 2\n",
    );
    let project = fusion::compile_project_with_metadata(p.entry()).unwrap();
    let entry = project
        .modules
        .iter()
        .find(|module| module.name == "crate")
        .unwrap();
    let import = entry
        .imports
        .iter()
        .find(|import| import.path == "math")
        .unwrap();
    assert_eq!(import.alias.as_deref(), Some("m"));
    let ast_entry = project
        .program
        .modules
        .iter()
        .find(|module| module.name == "crate")
        .unwrap();
    assert_eq!(ast_entry.imports[0].alias.as_deref(), Some("m"));
}

#[test]
fn nested_module_imports_resolve_relative_to_the_importing_module() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import graphics\nmain:\n    print(1)\n");
    p.write(
        "graphics/mod.fusion",
        "import geometry\npub fn marker() -> num:\n    return 1\n",
    );
    p.write(
        "graphics/geometry.fusion",
        "pub fn area() -> num:\n    return 4\n",
    );
    let project = fusion::compile_project_with_metadata(p.entry()).unwrap();
    assert!(project
        .modules
        .iter()
        .any(|module| module.name == "graphics.geometry"));
}

#[test]
fn imported_module_without_main_can_establish_indentation_style() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import math\nmain:\n    print(double(21))\n");
    p.write(
        "math.fusion",
        "pub fn double(value: num):\n    return value * 2\n",
    );
    let program = fusion::compile_project(p.entry()).expect("module files do not need main");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn imported_module_without_main_can_establish_brace_style() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import math\nmain {\n    print(double(21))\n}\n",
    );
    p.write(
        "math.fusion",
        "pub fn double(value: num) -> num {\n    return value * 2\n}\n",
    );
    let program =
        fusion::compile_project(p.entry()).expect("brace-style module files do not need main");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn selective_import_exposes_only_named_public_functions() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "from math import double\nmain:\n    print(double(21))\n",
    );
    p.write("math.fusion", "pub fn double(value: num) -> num:\n    return value * 2\npub fn triple(value: num) -> num:\n    return value * 3\n");
    let program = fusion::compile_project(p.entry()).expect("selective import should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn selective_import_does_not_expose_unselected_functions() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "from math import double\nmain:\n    print(triple(7))\n",
    );
    p.write("math.fusion", "pub fn double(value: num) -> num:\n    return value * 2\npub fn triple(value: num) -> num:\n    return value * 3\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("not imported")
            || error.contains("not defined")
            || error.contains("unknown function"),
        "{error}"
    );
}

#[test]
fn public_reexport_can_be_called_through_facade() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import facade\nmain:\n    print(facade.value())\n",
    );
    p.write("facade.fusion", "pub import math\n");
    p.write("math.fusion", "pub fn value() -> num:\n    return 7\n");
    let program = fusion::compile_project(p.entry()).expect("re-exported function should resolve");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["7".to_string()]);
}

#[test]
fn selective_import_is_retained_in_module_metadata() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "from math import double\nmain:\n    print(double(2))\n",
    );
    p.write(
        "math.fusion",
        "pub fn double(value: num) -> num:\n    return value * 2\n",
    );
    let project = fusion::compile_project_with_metadata(p.entry()).unwrap();
    let entry = project
        .modules
        .iter()
        .find(|module| module.name == "crate")
        .unwrap();
    assert_eq!(entry.imports[0].selected, vec!["double".to_string()]);
}

#[test]
fn conflicting_default_module_aliases_require_explicit_aliases() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import first.math\nimport second.math\nmain:\n    print(1)\n",
    );
    p.write("first/math.fusion", "pub fn one() -> num:\n    return 1\n");
    p.write("second/math.fusion", "pub fn two() -> num:\n    return 2\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("module alias 'math' is ambiguous"),
        "{error}"
    );
}

#[test]
fn same_named_structs_are_rejected_until_type_namespaces_exist() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import alpha\nimport beta\nmain:\n    print(1)\n",
    );
    p.write("alpha.fusion", "pub struct Point:\n    x: num\n");
    p.write("beta.fusion", "pub struct Point:\n    y: num\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("module symbol collision") && error.contains("Point"),
        "{error}"
    );
}

#[test]
fn module_public_function_can_call_its_private_helper() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import math\nmain:\n    print(math.double(21))\n",
    );
    p.write("math.fusion", "fn twice(value: num) -> num:\n    return value * 2\npub fn double(value: num) -> num:\n    return twice(value)\n");
    let program =
        fusion::compile_project(p.entry()).expect("module-local private helper should resolve");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn private_function_cannot_be_called_through_module_qualifier() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import math\nmain:\n    print(math.secret())\n",
    );
    p.write("math.fusion", "fn secret() -> num:\n    return 42\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("does not export function 'secret'"),
        "{error}"
    );
}

#[test]
fn imported_struct_types_resolve_through_module_aliases() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import geometry as g\nfn inspect(point: g.Point) -> num:\n    return 1\nmain:\n    print(1)\n");
    p.write(
        "geometry.fusion",
        "pub struct Point:\n    x: num\n    y: num\n",
    );
    // The function signature is checked even though the test program does not
    // construct a Point value yet; qualified constructor syntax is a follow-up.
    fusion::compile_project(p.entry()).expect("qualified imported struct type should resolve");
}

#[test]
fn private_struct_types_are_not_exposed_through_import_aliases() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import geometry as g\nfn inspect(point: g.Point) -> num:\n    return 1\nmain:\n    print(1)\n");
    p.write("geometry.fusion", "struct Point:\n    x: num\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(error.contains("Unknown type 'g.Point'"), "{error}");
}

#[test]
fn duplicate_file_and_directory_module_identities_are_rejected() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import geometry\nimport geometry.mod\nmain:\n    print(1)\n",
    );
    p.write("geometry.fusion", "pub fn marker() -> num:\n    return 1\n");
    p.write(
        "geometry/mod.fusion",
        "pub fn other() -> num:\n    return 2\n",
    );
    // `geometry.fusion` and `geometry/mod.fusion` both claim the logical
    // identity `geometry`; accepting both would make imports order-dependent.
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("duplicate module identity 'geometry'"),
        "{error}"
    );
}

#[test]
fn module_metadata_exports_are_sorted_deterministically() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import math\nmain:\n    print(1)\n");
    p.write("math.fusion", "pub fn zebra() -> num:\n    return 1\npub fn alpha() -> num:\n    return 2\nfn hidden() -> num:\n    return 3\n");
    let project = fusion::compile_project_with_metadata(p.entry()).unwrap();
    let module = project
        .program
        .modules
        .iter()
        .find(|module| module.name == "math")
        .unwrap();
    assert_eq!(module.public_symbols, vec!["alpha", "zebra"]);
    assert_eq!(module.private_symbols, vec!["hidden"]);
}

#[test]
fn qualified_struct_constructor_syntax_is_parsed_and_type_checked() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import geometry as g\nmain:\n    let point = g.Point { x: 3, y: 4 }\n    print(1)\n",
    );
    p.write(
        "geometry.fusion",
        "pub struct Point:\n    x: num\n    y: num\n",
    );
    fusion::compile_project(p.entry()).expect("qualified struct construction should type-check");
}

#[test]
fn qualified_enum_constructor_syntax_is_type_checked() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import geometry as g\nmain:\n    let color = g.Color::Red\n    print(1)\n",
    );
    p.write("geometry.fusion", "pub enum Color:\n    Red\n    Blue\n");
    fusion::compile_project(p.entry()).expect("qualified enum construction should type-check");
}

#[test]
fn qualified_enum_variant_pattern_syntax_is_type_checked() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import geometry as g\nfn inspect(color: g.Color) -> num:\n    match color:\n        g.Color::Red => return 1\n        g.Color::Blue => return 2\nmain:\n    print(1)\n");
    p.write("geometry.fusion", "pub enum Color:\n    Red\n    Blue\n");
    fusion::compile_project(p.entry()).expect("qualified enum patterns should type-check");
}

#[test]
fn same_named_module_constants_and_traits_have_independent_namespaces() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import alpha as a\nimport beta as b\nmain:\n    print(a.LIMIT + b.LIMIT)\n",
    );
    p.write(
        "alpha.fusion",
        "pub const LIMIT: num = 3\npub trait Marker:\n    fn mark(value: num) -> num\n",
    );
    p.write(
        "beta.fusion",
        "pub const LIMIT: num = 4\npub trait Marker:\n    fn mark(value: num) -> num\n",
    );
    let program = fusion::compile_project(p.entry())
        .expect("module constants and traits should be namespaced independently");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["7".to_string()]);
    let alpha_trait = fusion::module_system::mangle_trait("alpha", "Marker");
    let beta_trait = fusion::module_system::mangle_trait("beta", "Marker");
    assert_ne!(alpha_trait, beta_trait);
    assert_eq!(
        interpreter
            .trait_definition_id(&alpha_trait)
            .unwrap()
            .module,
        "alpha"
    );
    assert_eq!(
        interpreter.trait_definition_id(&beta_trait).unwrap().module,
        "beta"
    );
    let alpha_constant = fusion::module_system::mangle_constant("alpha", "LIMIT");
    let beta_constant = fusion::module_system::mangle_constant("beta", "LIMIT");
    assert_eq!(
        interpreter
            .constant_definition_id(&alpha_constant)
            .unwrap()
            .module,
        "alpha"
    );
    assert_eq!(
        interpreter
            .constant_definition_id(&beta_constant)
            .unwrap()
            .module,
        "beta"
    );
}

#[test]
fn interpreter_dispatches_impl_methods_through_hir_receiver_identity() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import model\nmain:\n    let thing = model.Thing { value: 5 }\n    print(thing.double())\n");
    p.write("model.fusion", "pub struct Thing { value: num }\nimpl Thing:\n    fn double(self: Thing) -> num:\n        return self.value * 2\n");
    let program =
        fusion::compile_project(p.entry()).expect("inherent method project should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["10".to_string()]);
    let receiver = fusion::module_system::mangle_type("model", "Thing");
    let method_id = interpreter
        .method_definition_id(&receiver, "double")
        .expect("method HIR identity");
    assert_eq!(method_id.module, "model");
    assert_eq!(method_id.name, "double");
    assert_eq!(
        method_id.scope,
        vec!["inherent".to_string(), "model::Thing".to_string()]
    );
    let field_id = interpreter
        .struct_field_definition_id(&receiver, "value")
        .expect("field HIR identity");
    assert_eq!(field_id.module, "model");
    assert_eq!(field_id.name, "value");
    assert_eq!(field_id.kind, fusion::hir::HirDefinitionKind::Field);
}

#[test]
fn trait_impl_resolution_is_module_aware_and_checks_required_methods() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import model\nmain:\n    let thing = model.Thing { value: 9 }\n    print(thing.render())\n");
    p.write("model.fusion", "pub trait Renderable:\n    fn render(self: Thing) -> num\npub struct Thing { value: num }\nimpl Renderable for Thing:\n    fn render(self: Thing) -> num:\n        return self.value\n");
    let program = fusion::compile_project(p.entry())
        .expect("trait implementation should resolve against its module's HIR declarations");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["9".to_string()]);
}

#[test]
fn trait_impl_missing_method_is_rejected_during_hir_resolution() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import model\nmain:\n    print(1)\n");
    p.write("model.fusion", "pub trait Renderable:\n    fn render(self: Thing) -> num\npub struct Thing { value: num }\nimpl Renderable for Thing:\n    fn other(self: Thing) -> num:\n        return self.value\n");
    let error = fusion::compile_project(p.entry()).unwrap_err();
    assert!(
        error.contains("missing a method 'render'"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn hir_index_contains_scoped_struct_fields_and_enum_variants() {
    let p = ProjectDir::new();
    p.write("main.fusion", "import model\nmain:\n    print(1)\n");
    p.write(
        "model.fusion",
        "pub struct Point { x: num }\npub enum Shape { Circle(num), Square }\n",
    );
    let project = fusion::module_system::load_project_with_metadata(p.entry()).unwrap();
    let index = fusion::module_system::hir_symbol_index(&project);
    let field = fusion::hir::HirDefinitionId::scoped(
        "model",
        ["Point".to_string()],
        "x",
        fusion::hir::HirDefinitionKind::Field,
    );
    let variant = fusion::hir::HirDefinitionId::scoped(
        "model",
        ["Shape".to_string()],
        "Circle",
        fusion::hir::HirDefinitionKind::Variant,
    );
    assert!(index.contains(&field));
    assert!(index.contains(&variant));
}

#[test]
fn selective_constant_import_and_local_shadowing_preserve_identity() {
    let p = ProjectDir::new();
    p.write("main.fusion", "from limits import LIMIT\nconst BASE: num = LIMIT + 1\nmain:\n    let LIMIT = 2\n    print(BASE + LIMIT)\n");
    p.write("limits.fusion", "pub const LIMIT: num = 5\n");
    let program = fusion::compile_project(p.entry())
        .expect("selective constant import should lower to its provider identity");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["8".to_string()]);
}

#[test]
fn enum_variant_runtime_metadata_is_scoped_by_enum_hir_identity() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import model\nmain:\n    let state = model.State::Ready(7)\n    print(1)\n",
    );
    p.write("model.fusion", "pub enum State { Ready(num), Waiting }\n");
    let program =
        fusion::compile_project(p.entry()).expect("qualified enum construction should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    let enum_symbol = fusion::module_system::mangle_type("model", "State");
    let variant = interpreter
        .enum_variant_hir_definition_id(&enum_symbol, "Ready")
        .expect("variant identity");
    assert_eq!(variant.module, "model");
    assert_eq!(variant.scope, vec!["State".to_string()]);
    assert_eq!(variant.name, "Ready");
    assert_eq!(variant.kind, fusion::hir::HirDefinitionKind::Variant);
}

#[test]
fn foundry_package_root_imports_public_dependency_symbols() {
    let p = ProjectDir::new();
    p.write(
        "src/main.fusion",
        "import math\nmain:\n    print(math.double(21))\n",
    );
    p.write(
        "deps/math/src/lib.fusion",
        "pub fn double(value: num) -> num:\n    return value * 2\n",
    );
    let roots = std::collections::BTreeMap::from([("math".to_string(), p.0.join("deps/math/src"))]);
    let program = fusion::compile_project_with_package_roots(p.0.join("src/main.fusion"), roots)
        .expect("a declared package root should be importable by its dependency alias");
    assert!(program
        .modules
        .iter()
        .any(|m| m.name == "math" && m.public_symbols.iter().any(|s| s == "double")));
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn package_imports_keep_dependency_source_root_isolated() {
    let p = ProjectDir::new();
    p.write("src/main.fusion", "import math\nmain:\n    print(1)\n");
    p.write(
        "deps/math/src/lib.fusion",
        "import helpers\npub fn answer() -> num:\n    return helpers.value()\n",
    );
    p.write(
        "deps/math/src/helpers.fusion",
        "pub fn value() -> num:\n    return 42\n",
    );
    let roots = std::collections::BTreeMap::from([("math".to_string(), p.0.join("deps/math/src"))]);
    let program = fusion::compile_project_with_package_roots(p.0.join("src/main.fusion"), roots)
        .expect("dependency-local imports should resolve inside that dependency source root");
    assert!(program.modules.iter().any(|m| m.name == "math.helpers"));
}

#[test]
fn package_imports_enforce_public_visibility() {
    let p = ProjectDir::new();
    p.write(
        "src/main.fusion",
        "import math\nmain:\n    print(math.secret())\n",
    );
    p.write(
        "deps/math/src/lib.fusion",
        "fn secret() -> num:\n    return 42\n",
    );
    let roots = std::collections::BTreeMap::from([("math".to_string(), p.0.join("deps/math/src"))]);
    let error =
        fusion::compile_project_with_package_roots(p.0.join("src/main.fusion"), roots).unwrap_err();
    assert!(
        error.contains("privacy violation"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn package_public_reexports_resolve_transitively_to_the_original_provider() {
    let p = ProjectDir::new();
    p.write(
        "app/src/main.fusion",
        "import facade as f\nmain:\n    print(f.compute(20))\n",
    );
    p.write("deps/facade/src/lib.fusion", "pub import api\n");
    p.write(
        "deps/facade/src/api.fusion",
        "pub from implementation import compute\n",
    );
    p.write(
        "deps/facade/src/implementation.fusion",
        "pub fn compute(value: num) -> num:\n    return value + 22\n",
    );

    let roots =
        std::collections::BTreeMap::from([("facade".to_string(), p.0.join("deps/facade/src"))]);
    let program =
        fusion::compile_project_with_package_roots(p.0.join("app/src/main.fusion"), roots).expect(
            "a package facade should expose a symbol through multiple public re-export edges",
        );
    assert!(program
        .modules
        .iter()
        .any(|module| module.name == "facade.implementation"
            && module.public_symbols.iter().any(|s| s == "compute")));
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn package_reexport_does_not_expose_unselected_symbols() {
    let p = ProjectDir::new();
    p.write(
        "app/src/main.fusion",
        "import facade as f\nmain:\n    print(f.hidden())\n",
    );
    p.write(
        "deps/facade/src/lib.fusion",
        "pub from api import visible\n",
    );
    p.write(
        "deps/facade/src/api.fusion",
        "pub fn visible() -> num:\n    return 1\npub fn hidden() -> num:\n    return 2\n",
    );

    let roots =
        std::collections::BTreeMap::from([("facade".to_string(), p.0.join("deps/facade/src"))]);
    let error = fusion::compile_project_with_package_roots(p.0.join("app/src/main.fusion"), roots)
        .expect_err("selective public re-exports must not leak sibling declarations");
    assert!(
        error.contains("does not export")
            || error.contains("not imported")
            || error.contains("privacy violation"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn package_module_cycles_report_the_cross_package_chain() {
    let p = ProjectDir::new();
    p.write(
        "app/src/main.fusion",
        "import facade\nmain:\n    print(1)\n",
    );
    p.write("deps/facade/src/lib.fusion", "pub import api\n");
    p.write(
        "deps/facade/src/api.fusion",
        "import lib\npub fn value() -> num:\n    return 1\n",
    );

    let roots =
        std::collections::BTreeMap::from([("facade".to_string(), p.0.join("deps/facade/src"))]);
    let error = fusion::compile_project_with_package_roots(p.0.join("app/src/main.fusion"), roots)
        .expect_err("a package import cycle must be rejected");
    assert!(
        error.contains("module dependency cycle detected"),
        "unexpected diagnostic: {error}"
    );
    assert!(
        error.contains("facade") && error.contains("facade.api"),
        "cycle chain should identify both modules: {error}"
    );
}

#[test]
fn package_dependency_can_import_its_own_transitive_dependency() {
    let p = ProjectDir::new();
    p.write(
        "app/src/main.fusion",
        "import consumer\nmain:\n    print(consumer.answer())\n",
    );
    p.write(
        "deps/consumer/src/lib.fusion",
        "import model\npub fn answer() -> num:\n    return model.value()\n",
    );
    p.write(
        "deps/model/src/lib.fusion",
        "pub fn value() -> num:\n    return 42\n",
    );
    let roots = std::collections::BTreeMap::from([
        ("consumer".to_string(), p.0.join("deps/consumer/src")),
        ("model".to_string(), p.0.join("deps/model/src")),
    ]);
    let program =
        fusion::compile_project_with_package_roots(p.0.join("app/src/main.fusion"), roots)
            .expect("dependency package should resolve its own transitive dependency");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn sibling_module_import_wins_over_same_named_crate_root_module() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import shapes.circle as circle\nmain:\n    print(circle.value())\n",
    );
    p.write("util.fusion", "pub fn value() -> num:\n    return 7\n");
    p.write(
        "shapes/circle.fusion",
        "import util\npub fn value() -> num:\n    return util.value()\n",
    );
    p.write(
        "shapes/util.fusion",
        "pub fn value() -> num:\n    return 42\n",
    );
    let program = fusion::compile_project(p.entry())
        .expect("sibling import should resolve before crate-root fallback");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["42".to_string()]);
}

#[test]
fn same_leaf_module_names_in_different_packages_resolve_from_importer_ancestors() {
    let p = ProjectDir::new();
    p.write(
        "main.fusion",
        "import alpha.api as a\nimport beta.api as b\nmain:\n    print(a.value() + b.value())\n",
    );
    p.write(
        "deps/alpha/src/api.fusion",
        "import models\npub fn value() -> num:\n    return models.value()\n",
    );
    p.write(
        "deps/alpha/src/models.fusion",
        "pub fn value() -> num:\n    return 10\n",
    );
    p.write(
        "deps/beta/src/api.fusion",
        "import models\npub fn value() -> num:\n    return models.value()\n",
    );
    p.write(
        "deps/beta/src/models.fusion",
        "pub fn value() -> num:\n    return 20\n",
    );
    let roots = std::collections::BTreeMap::from([
        ("alpha".to_string(), p.0.join("deps/alpha/src")),
        ("beta".to_string(), p.0.join("deps/beta/src")),
    ]);
    let program = fusion::compile_project_with_package_roots(p.entry(), roots)
        .expect("same leaf module names should resolve within each package namespace");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    assert_eq!(interpreter.output(), &["30".to_string()]);
}
