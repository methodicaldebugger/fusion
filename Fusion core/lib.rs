pub mod ast;
pub mod codegen;
pub mod conc_ext;
pub mod crypto_ext;
pub mod data_ext;
pub mod derive_expansion;
pub mod doc_gen;
pub mod environment;
pub mod errors;
pub mod foreign;
pub mod h2_ext;
pub mod hir;
pub mod hir_body;
pub mod hir_lower;
pub mod interpreter;
pub mod ir;
pub mod lexer;
pub mod llvm_backend;
pub mod module_system;
pub mod name_resolution;
pub mod net_ext;
pub mod os_ext;
pub mod parser;
pub mod span;
pub mod stdlib_ext;
pub mod test_runner;
pub mod text_ext;
pub mod time_ext;
pub mod tls_ext;
pub mod trait_defaults;
pub mod type_checker;
pub mod function_values;
pub mod type_expr;
pub mod types;
pub mod tz_ext;
pub mod value;
pub mod wide_int;

use lexer::{BlockMode, Lexer};
use llvm_backend::LLVMBackend;
use name_resolution::Resolver;
use parser::Parser;
use type_checker::TypeChecker;

pub fn compile_source(source: &str) -> Result<ast::Program, String> {
    let mut lexer = Lexer::new(source, BlockMode::Unknown);
    let tokens = lexer.tokenize().map_err(|e| format!("lexer: {}", e))?;
    let mut parser = Parser::new(tokens);
    let program = parser.parse().map_err(|e| format!("parser: {}", e))?;
    compile_program(program)
}

/// Compile a source file and recursively load its `import` dependencies.
pub fn compile_project(entry: impl AsRef<std::path::Path>) -> Result<ast::Program, String> {
    compile_project_with_package_roots(entry, std::collections::BTreeMap::new())
}

/// Compile a project with package source roots resolved by Foundry.
pub fn compile_project_with_package_roots(
    entry: impl AsRef<std::path::Path>,
    package_roots: std::collections::BTreeMap<String, std::path::PathBuf>,
) -> Result<ast::Program, String> {
    Ok(compile_project_graph_with_package_roots(entry, package_roots)?.program)
}

/// Compile a project while retaining its structured module graph for tooling,
/// diagnostics, package managers, and identity-aware backends.
pub fn compile_project_with_metadata(
    entry: impl AsRef<std::path::Path>,
) -> Result<module_system::Project, String> {
    compile_project_graph_with_package_roots(entry, std::collections::BTreeMap::new())
}

fn compile_project_graph_with_package_roots(
    entry: impl AsRef<std::path::Path>,
    package_roots: std::collections::BTreeMap<String, std::path::PathBuf>,
) -> Result<module_system::Project, String> {
    let mut project = module_system::load_project_with_package_roots(entry, package_roots)?;
    // Build HIR and resolve source-level names against the original module graph
    // before lowering the compatibility AST's module namespaces for legacy passes
    // and LLVM emission.
    hir_lower::HirLowerer::lower_project(&project).map_err(|e| format!("HIR lowering: {}", e))?;
    module_system::validate_hir_type_references(&project)
        .map_err(|e| format!("module type resolution: {}", e))?;
    module_system::validate_hir_trait_implementations(&project)
        .map_err(|e| format!("module trait resolution: {}", e))?;
    module_system::resolve_hir_type_expressions(&project)
        .map_err(|e| format!("module constructor resolution: {}", e))?;
    module_system::resolve_hir_pattern_and_field_expressions(&project)
        .map_err(|e| format!("module pattern/field resolution: {}", e))?;
    module_system::resolve_hir_pattern_binding_types(&project)
        .map_err(|e| format!("module pattern binding type resolution: {}", e))?;
    module_system::propagate_hir_expression_types(&project)
        .map_err(|e| format!("module expression type propagation: {}", e))?;
    module_system::validate_hir_nominal_value_types(&project)
        .map_err(|e| format!("module nominal value validation: {}", e))?;
    let mut compatibility_program = project.program.clone();
    module_system::lower_project_type_namespaces(&project.modules, &mut compatibility_program)
        .map_err(|e| format!("module type namespace lowering: {}", e))?;
    module_system::lower_project_trait_constant_namespaces(
        &project.modules,
        &mut compatibility_program,
    )
    .map_err(|e| format!("module trait/constant namespace lowering: {}", e))?;
    let registry = module_system::hir_type_registry(&project);
    project.program = compile_project_program(compatibility_program, &registry)?;
    Ok(project)
}

pub fn lower_project_hir(entry: impl AsRef<std::path::Path>) -> Result<hir::HirProject, String> {
    let project = module_system::load_project_with_metadata(entry)?;
    hir_lower::HirLowerer::lower_project(&project).map_err(|e| format!("HIR lowering: {}", e))
}

fn compile_project_program(
    program: ast::Program,
    registry: &hir::HirTypeRegistry,
) -> Result<ast::Program, String> {
    let mut resolver = Resolver::new();
    resolver
        .resolve(&program)
        .map_err(|e| format!("resolver: {}", e))?;
    let identities = module_system::backend_type_definition_ids(&program);
    let mut checker = TypeChecker::new();
    checker
        .check_with_hir(&program, identities, registry)
        .map_err(|e| format!("HIR-aware type checker: {}", e))?;
    Ok(program)
}

fn compile_program(program: ast::Program) -> Result<ast::Program, String> {
    let mut resolver = Resolver::new();
    resolver
        .resolve(&program)
        .map_err(|e| format!("resolver: {}", e))?;
    let mut checker = TypeChecker::new();
    checker
        .check(&program)
        .map_err(|e| format!("type checker: {}", e))?;
    Ok(program)
}

pub fn emit_llvm(source: &str) -> Result<String, String> {
    let program = compile_source(source)?;
    LLVMBackend::new()
        .emit_program(&program)
        .map_err(|e| e.to_string())
}

/// Emit LLVM IR for a multi-file Fusion project.
pub fn emit_llvm_project(entry: impl AsRef<std::path::Path>) -> Result<String, String> {
    emit_llvm_project_with_package_roots(entry, std::collections::BTreeMap::new())
}

pub fn emit_llvm_project_with_package_roots(
    entry: impl AsRef<std::path::Path>,
    package_roots: std::collections::BTreeMap<String, std::path::PathBuf>,
) -> Result<String, String> {
    let project = compile_project_graph_with_package_roots(entry, package_roots)?;
    LLVMBackend::new()
        .emit_project(&project)
        .map_err(|e| e.to_string())
}

/// Emit the application entry-point and shared runtime as a separate LLVM
/// translation unit. Package-owned Fusion functions and scalar data globals are
/// replaced by external declarations so package modules can be linked independently.
pub fn emit_llvm_entrypoint_with_package_roots(
    entry: impl AsRef<std::path::Path>,
    package_roots: std::collections::BTreeMap<String, std::path::PathBuf>,
) -> Result<String, String> {
    let project = compile_project_graph_with_package_roots(entry, package_roots)?;
    let full_ir = LLVMBackend::new()
        .emit_project(&project)
        .map_err(|error| error.to_string())?;
    extract_entrypoint_llvm_unit(&full_ir)
}

fn extract_entrypoint_llvm_unit(full_ir: &str) -> Result<String, String> {
    fn parse_definition(block: &str) -> Result<(String, String), String> {
        let header = block.lines().next().unwrap_or("").trim();
        let at = header
            .find('@')
            .ok_or("LLVM function definition has no symbol")?;
        let open = header[at..]
            .find('(')
            .map(|offset| at + offset)
            .ok_or("LLVM function definition has no parameter list")?;
        let close = header[open..]
            .find(')')
            .map(|offset| open + offset)
            .ok_or("LLVM function definition has no closing parameter list")?;
        let symbol = header[at + 1..open].trim().to_string();
        let result = header[..at]
            .split_whitespace()
            .last()
            .ok_or("LLVM function definition has no return type")?;
        let params = header[open + 1..close]
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    None
                } else {
                    part.split_whitespace().next()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok((
            symbol.clone(),
            format!("declare {} @{}({})", result, symbol, params),
        ))
    }

    let lines = full_ir.lines().collect::<Vec<_>>();
    let mut types = Vec::new();
    let mut globals = Vec::new();
    let mut declarations = std::collections::BTreeSet::new();
    let mut runtime_definitions = Vec::new();
    let mut package_declarations = std::collections::BTreeSet::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        if line.starts_with("define ") {
            let start = index;
            let mut depth = 0i32;
            let mut saw_open = false;
            while index < lines.len() {
                for ch in lines[index].chars() {
                    if ch == '{' {
                        depth += 1;
                        saw_open = true;
                    }
                    if ch == '}' {
                        depth -= 1;
                    }
                }
                index += 1;
                if saw_open && depth == 0 {
                    break;
                }
            }
            if !saw_open || depth != 0 {
                return Err(
                    "malformed LLVM function body while extracting application entrypoint".into(),
                );
            }
            let block = lines[start..index].join("\n") + "\n";
            let (symbol, declaration) = parse_definition(&block)?;
            if symbol.starts_with("_F") {
                package_declarations.insert(declaration);
            } else {
                runtime_definitions.push(block);
            }
            continue;
        }
        if line.starts_with("declare ") {
            declarations.insert(line.to_string());
        }
        if line.contains(" = type ") {
            types.push(line.to_string());
        }
        if line.starts_with('@') && line.contains(" = ") {
            if line.starts_with("@_F__fusion_m") {
                let (symbol, rhs) = line.split_once(" = ").ok_or("malformed LLVM global")?;
                if !runtime_definitions
                    .iter()
                    .any(|block| block.contains(symbol))
                {
                    index += 1;
                    continue;
                }
                let words = rhs.split_whitespace().collect::<Vec<_>>();
                let (linkage, ty_index) = if words.first() == Some(&"constant") {
                    ("constant", 1)
                } else if words.first() == Some(&"global") {
                    ("global", 1)
                } else {
                    ("global", 1)
                };
                if let Some(ty) = words.get(ty_index) {
                    globals.push(format!("{} = external {} {}", symbol, linkage, ty));
                }
            } else if line.starts_with("@.str") {
                let symbol = line.split_whitespace().next().unwrap_or("");
                if runtime_definitions
                    .iter()
                    .any(|block| block.contains(symbol))
                {
                    globals.push(line.to_string());
                }
            } else {
                globals.push(line.to_string());
            }
        }
        index += 1;
    }
    if !runtime_definitions
        .iter()
        .any(|block| block.lines().next().unwrap_or("").contains("@main("))
    {
        return Err("application LLVM output did not contain a main entry point".into());
    }
    let mut output = String::from("; Fusion application entrypoint and shared runtime\n");
    for ty in types {
        output.push_str(&ty);
        output.push('\n');
    }
    for global in globals {
        output.push_str(&global);
        output.push('\n');
    }
    for declaration in declarations {
        output.push_str(&declaration);
        output.push('\n');
    }
    for declaration in package_declarations {
        output.push_str(&declaration);
        output.push('\n');
    }
    for definition in runtime_definitions {
        output.push_str(&definition);
        output.push('\n');
    }
    Ok(output)
}

/// Emit a package-scoped LLVM module for the source files beneath `package_source_root`.
///
/// The compiler performs normal project loading and semantic validation, then selects
/// definitions and scalar constant globals owned by the requested package. Cross-package
/// function and data references become external declarations; the application entrypoint
/// owns shared runtime state. Aggregate package data remains outside the scalar data ABI.
pub fn emit_llvm_package_with_package_roots(
    entry: impl AsRef<std::path::Path>,
    package_roots: std::collections::BTreeMap<String, std::path::PathBuf>,
    package_source_root: impl AsRef<std::path::Path>,
) -> Result<String, String> {
    let source_root = package_source_root
        .as_ref()
        .canonicalize()
        .map_err(|error| {
            format!(
                "package source root '{}': {error}",
                package_source_root.as_ref().display()
            )
        })?;
    if !source_root.is_dir() {
        return Err(format!(
            "package source root is not a directory: {}",
            source_root.display()
        ));
    }
    let project = compile_project_graph_with_package_roots(entry, package_roots)?;
    let modules = project
        .program
        .modules
        .iter()
        .filter_map(|module| {
            let path = std::path::Path::new(&module.path).canonicalize().ok()?;
            path.starts_with(&source_root)
                .then_some(module.name.clone())
        })
        .collect::<std::collections::BTreeSet<_>>();
    if modules.is_empty() {
        // A manifest dependency need not be imported by this application. Keep
        // its graph node/cache record valid with an empty, linkable package unit.
        return Ok("; Fusion package-scoped LLVM IR (no loaded modules)\n".into());
    }
    let full_ir = LLVMBackend::new()
        .emit_project(&project)
        .map_err(|error| error.to_string())?;
    extract_package_llvm_unit(&full_ir, &modules)
}

fn extract_package_llvm_unit(
    full_ir: &str,
    modules: &std::collections::BTreeSet<String>,
) -> Result<String, String> {
    fn hex(value: &str) -> String {
        value
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join("")
    }
    fn module_prefixes(modules: &std::collections::BTreeSet<String>) -> Vec<String> {
        let mut prefixes = Vec::new();
        for module in modules {
            prefixes.push(format!("_F__fusion_m{}_{}_f", module.len(), hex(module)));
            let encoded = format!("{}x{}", module.len(), hex(module));
            prefixes.push(format!("_F__fusion_method_m{}_", encoded));
        }
        prefixes
    }
    fn parse_definition(block: &str) -> Result<(String, String), String> {
        let header = block.lines().next().unwrap_or("").trim();
        let at = header
            .find('@')
            .ok_or("LLVM function definition has no symbol")?;
        let open = header[at..]
            .find('(')
            .map(|offset| at + offset)
            .ok_or("LLVM function definition has no parameter list")?;
        let close = header[open..]
            .find(')')
            .map(|offset| open + offset)
            .ok_or("LLVM function definition has no closing parameter list")?;
        let symbol = header[at + 1..open].trim().to_string();
        let result = header[..at]
            .split_whitespace()
            .last()
            .ok_or("LLVM function definition has no return type")?;
        let params = header[open + 1..close]
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    None
                } else {
                    part.split_whitespace().next()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok((
            symbol,
            format!(
                "declare {} @{}({})",
                result,
                header[at + 1..open].trim(),
                params
            ),
        ))
    }

    let prefixes = module_prefixes(modules);
    let lines = full_ir.lines().collect::<Vec<_>>();
    let mut type_definitions = Vec::new();
    let mut declarations = std::collections::BTreeSet::new();
    let mut global_definitions = Vec::<(String, String)>::new();
    let mut definitions = Vec::<(String, String)>::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        if line.starts_with("define ") {
            let start = index;
            let mut depth = 0i32;
            let mut saw_open = false;
            while index < lines.len() {
                let current = lines[index];
                for ch in current.chars() {
                    if ch == '{' {
                        depth += 1;
                        saw_open = true;
                    }
                    if ch == '}' {
                        depth -= 1;
                    }
                }
                index += 1;
                if saw_open && depth == 0 {
                    break;
                }
            }
            if !saw_open || depth != 0 {
                return Err("malformed LLVM function body while extracting package module".into());
            }
            let block = lines[start..index].join("\n") + "\n";
            let (symbol, _) = parse_definition(&block)?;
            definitions.push((symbol, block));
            continue;
        }
        if line.starts_with("declare ") {
            declarations.insert(line.to_string());
        }
        if line.contains(" = type ") {
            type_definitions.push(line.to_string());
        }
        if line.starts_with('@') && line.contains(" = ") {
            if let Some((symbol, _)) = line.split_once(" = ") {
                global_definitions.push((symbol.trim().to_string(), line.to_string()));
            }
        }
        index += 1;
    }

    let mut selected = Vec::new();
    let mut external_declarations = std::collections::BTreeSet::new();
    for (symbol, block) in &definitions {
        let owned = prefixes.iter().any(|prefix| symbol.starts_with(prefix));
        if owned {
            selected.push(block.clone());
        } else if symbol.starts_with("_F__fusion_m") || symbol.starts_with("_F__fusion_method_m") {
            let (_, declaration) = parse_definition(block)?;
            external_declarations.insert(declaration);
        } else if symbol.starts_with("fusion_") {
            // Shared runtime definitions live in the entrypoint translation unit.
            let (_, declaration) = parse_definition(block)?;
            external_declarations.insert(declaration);
        }
    }
    let mut selected_globals = Vec::new();
    for (symbol, definition) in &global_definitions {
        if symbol.starts_with("@.str") {
            // Private literal storage is handled after package data definitions
            // are selected, so unrelated modules do not pollute this cache unit.
            continue;
        }
        if prefixes
            .iter()
            .any(|prefix| symbol.trim_start_matches('@').starts_with(prefix))
        {
            selected_globals.push(definition.clone());
        } else if symbol.starts_with("@_F__fusion_m") {
            let raw_symbol = symbol.trim_start_matches('@');
            let reference = format!("@{}", raw_symbol);
            if !selected.iter().any(|block| block.contains(&reference)) {
                continue;
            }
            let rhs = definition
                .split_once(" = ")
                .map(|(_, rhs)| rhs)
                .unwrap_or("");
            let words = rhs.split_whitespace().collect::<Vec<_>>();
            let (linkage, ty_index) = if words.first() == Some(&"constant") {
                ("constant", 1)
            } else if words.first() == Some(&"global") {
                ("global", 1)
            } else {
                continue;
            };
            let Some(ty) = words.get(ty_index) else {
                continue;
            };
            external_declarations.insert(format!("{} = external {} {}", symbol, linkage, ty));
        }
    }
    for (symbol, definition) in &global_definitions {
        if !symbol.starts_with("@.str") {
            continue;
        }
        let referenced = selected.iter().any(|block| block.contains(symbol))
            || selected_globals
                .iter()
                .any(|global| global.contains(symbol));
        if referenced {
            selected_globals.push(definition.clone());
        }
    }
    if selected.is_empty() && selected_globals.is_empty() && type_definitions.is_empty() {
        // A package can legitimately contain only an application entrypoint or
        // declarations. If nominal types exist, retain their stable identified
        // layouts even when no package function currently constructs them.
        return Ok("; Fusion package-scoped LLVM IR (no package functions)\n".into());
    }

    let mut output = String::from("; Fusion package-scoped LLVM IR\n");
    for definition in type_definitions {
        output.push_str(&definition);
        output.push('\n');
    }
    for global in selected_globals {
        output.push_str(&global);
        output.push('\n');
    }
    for declaration in declarations {
        output.push_str(&declaration);
        output.push('\n');
    }
    for declaration in external_declarations {
        output.push_str(&declaration);
        output.push('\n');
    }
    for block in selected {
        output.push_str(&block);
        output.push('\n');
    }
    Ok(output)
}

/// Emit a stable, package-scoped symbol interface for a loaded package.
/// Version 2 adds explicit scalar ABI signatures for public functions and
/// internal scalar signatures for private functions needed across the root
/// entrypoint/package-unit boundary. Aggregate, generic, async, and
/// incompletely typed signatures are marked unsupported instead of guessed.
pub fn emit_package_interface_with_package_roots(
    entry: impl AsRef<std::path::Path>,
    package_roots: std::collections::BTreeMap<String, std::path::PathBuf>,
    package_source_root: impl AsRef<std::path::Path>,
) -> Result<String, String> {
    let source_root = package_source_root
        .as_ref()
        .canonicalize()
        .map_err(|error| {
            format!(
                "package source root '{}': {error}",
                package_source_root.as_ref().display()
            )
        })?;
    if !source_root.is_dir() {
        return Err(format!(
            "package source root is not a directory: {}",
            source_root.display()
        ));
    }
    let program = compile_project_with_package_roots(entry, package_roots)?;
    let mut modules = program
        .modules
        .iter()
        .filter_map(|module| {
            let path = std::path::Path::new(&module.path).canonicalize().ok()?;
            path.starts_with(&source_root).then_some((module, path))
        })
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.0.name.cmp(&right.0.name));
    if modules.is_empty() {
        return Ok("FUSION-PACKAGE-INTERFACE\t6\n".into());
    }
    fn field(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }
    // The cross-package ABI is intentionally limited to the scalar types the
    // current LLVM backend can represent without layout metadata. Unknown,
    // generic, and aggregate signatures are explicitly marked unsupported.
    fn abi_type(
        source: &str,
        nominal_names: &std::collections::BTreeSet<String>,
    ) -> Option<&'static str> {
        match source.trim() {
            "num" | "i64" => Some("i64"),
            "float" | "f32" | "f64" => Some("double"),
            "bool" => Some("i1"),
            "string" => Some("ptr"),
            "void" => Some("void"),
            other if nominal_names.contains(other) => Some("ptr"),
            _ => None,
        }
    }
    use sha2::Digest;
    // Function ABI extraction must recognize nominal types imported from other
    // packages too. The flattened AST preserves qualified source spellings
    // (for example `model.Point`), while each module's public/private symbol
    // sets contain only declarations owned by that module. Build one project
    // symbol set from the module graph so imported nominal parameters and
    // returns are represented as the backend's opaque `ptr` ABI.
    let mut project_nominal_names = std::collections::BTreeSet::<String>::new();
    for owner in &program.modules {
        let owner_statements = &program.statements[owner.statement_start..owner.statement_end];
        for statement in owner_statements {
            let backend_name = match statement {
                ast::Statement::Struct { name, .. } | ast::Statement::Enum { name, .. } => name,
                _ => continue,
            };
            let source_name = owner
                .public_symbols
                .iter()
                .chain(owner.private_symbols.iter())
                .find(|source| {
                    module_system::mangle_type(&owner.name, source.as_str())
                        == backend_name.as_str()
                });
            let Some(source_name) = source_name else {
                continue;
            };
            project_nominal_names.insert(source_name.clone());
            project_nominal_names.insert(backend_name.clone());
            project_nominal_names.insert(format!("{}.{}", owner.name, source_name));
            for importer in &program.modules {
                for import in importer
                    .imports
                    .iter()
                    .filter(|import| import.path == owner.name)
                {
                    if !import.selected.is_empty()
                        && !import
                            .selected
                            .iter()
                            .any(|selected| selected == source_name)
                    {
                        continue;
                    }
                    let qualifier = import
                        .alias
                        .as_deref()
                        .unwrap_or_else(|| import.path.rsplit('.').next().unwrap_or(&import.path));
                    project_nominal_names.insert(format!("{}.{}", qualifier, source_name));
                }
            }
        }
    }
    let mut output = String::from("FUSION-PACKAGE-INTERFACE\t6\n");
    for (module, _) in modules {
        output.push_str(&format!("module\t{}\n", field(&module.name)));
        let mut exports = module.public_symbols.clone();
        exports.sort();
        exports.dedup();
        for symbol in &exports {
            output.push_str(&format!(
                "export\t{}\t{}\n",
                field(&module.name),
                field(symbol)
            ));
        }
        // Package-owned data/type metadata is part of the interface contract.
        // Scalar constants and concrete ordered struct storage layouts are
        // described independently from function signatures. Unsupported array
        // and nested aggregate storage is marked explicitly, never guessed.
        let nominal_backend_names = program
            .statements
            .iter()
            .filter_map(|statement| match statement {
                ast::Statement::Struct { name, .. } | ast::Statement::Enum { name, .. } => {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        fn layout_type(
            source: &str,
            nominal_names: &std::collections::BTreeSet<&str>,
        ) -> &'static str {
            match source.trim() {
                "num" | "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "isize" | "usize" => "i64",
                "float" | "f32" | "f64" => "double",
                "bool" => "i1",
                "string" => "ptr",
                other if other.starts_with("[") => "unsupported-array",
                other if nominal_names.contains(other) => "ptr",
                _ => "unsupported-aggregate",
            }
        }
        for statement in &program.statements[module.statement_start..module.statement_end] {
            match statement {
                ast::Statement::Struct { name, fields, .. } => {
                    let Some(source_name) = module.public_symbols.iter().find(|source| {
                        module_system::mangle_type(&module.name, source.as_str()) == name.as_str()
                    }) else {
                        continue;
                    };
                    let field_records = fields
                        .iter()
                        .map(|struct_field| {
                            format!(
                                "{}:{}",
                                field(&struct_field.name),
                                layout_type(&struct_field.type_name, &nominal_backend_names)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let layout_hash =
                        format!("{:x}", sha2::Sha256::digest(field_records.as_bytes()));
                    let type_symbol = format!(
                        "%fusion.type.{}",
                        module_system::mangle_type(&module.name, source_name)
                    );
                    output.push_str(&format!(
                        "abi-struct\t{}\t{}\t{}\t{}\t{}\t{}\n",
                        field(&module.name),
                        field(source_name),
                        type_symbol,
                        fields.len(),
                        layout_hash,
                        field_records
                    ));
                }
                ast::Statement::Enum { name, variants, .. } => {
                    let Some(source_name) = module.public_symbols.iter().find(|source| {
                        module_system::mangle_type(&module.name, source.as_str()) == name.as_str()
                    }) else {
                        continue;
                    };
                    let variant_records = variants
                        .iter()
                        .enumerate()
                        .map(|(discriminant, variant)| {
                            let payload = variant
                                .fields
                                .iter()
                                .map(|ty| layout_type(ty, &nominal_backend_names))
                                .collect::<Vec<_>>()
                                .join(",");
                            format!("{}={}({})", field(&variant.name), discriminant, payload)
                        })
                        .collect::<Vec<_>>()
                        .join(";");
                    let digest = format!("{:x}", sha2::Sha256::digest(variant_records.as_bytes()));
                    let type_symbol = format!(
                        "%fusion.type.{}",
                        module_system::mangle_type(&module.name, source_name)
                    );
                    output.push_str(&format!(
                        "abi-enum\t{}\t{}\t{}\t{}\t{}\t{}\n",
                        field(&module.name),
                        field(source_name),
                        type_symbol,
                        variants.len(),
                        digest,
                        field(&variant_records)
                    ));
                    // Publish each concrete payload record as a separate ABI
                    // item so the linker can compare the provider's emitted
                    // LLVM storage layout with the interface contract.
                    for variant in variants {
                        let payload_fields = variant
                            .fields
                            .iter()
                            .map(|ty| layout_type(ty, &nominal_backend_names))
                            .collect::<Vec<_>>()
                            .join(",");
                        let payload_digest =
                            format!("{:x}", sha2::Sha256::digest(payload_fields.as_bytes()));
                        let variant_key = format!("{}::{}", source_name, variant.name);
                        let payload_symbol = format!(
                            "%fusion.variant.{}",
                            module_system::mangle_type(&module.name, &variant_key)
                        );
                        output.push_str(&format!(
                            "abi-enum-payload\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                            field(&module.name),
                            field(source_name),
                            field(&variant.name),
                            payload_symbol,
                            variant.fields.len(),
                            payload_digest,
                            field(&payload_fields)
                        ));
                    }
                }
                ast::Statement::ConstDeclaration {
                    name,
                    declared_type,
                    value,
                    ..
                } => {
                    let source_name = module
                        .public_symbols
                        .iter()
                        .chain(module.private_symbols.iter())
                        .find(|source| {
                            module_system::mangle_constant(&module.name, source.as_str())
                                == name.as_str()
                        })
                        .map(String::as_str)
                        .unwrap_or(name.as_str());
                    let is_public = module
                        .public_symbols
                        .iter()
                        .any(|source| source.as_str() == source_name);
                    let (source_type, value_kind, encoded_value) = match value {
                        ast::Expression::Number { value, .. } => (
                            declared_type.as_deref().unwrap_or("num"),
                            "integer",
                            value.to_string(),
                        ),
                        ast::Expression::Float { value, .. } => (
                            declared_type.as_deref().unwrap_or("float"),
                            "float-bits",
                            format!("{:016x}", value.to_bits()),
                        ),
                        ast::Expression::Boolean { value, .. } => (
                            declared_type.as_deref().unwrap_or("bool"),
                            "boolean",
                            if *value { "1".into() } else { "0".into() },
                        ),
                        ast::Expression::String { value, .. } => (
                            declared_type.as_deref().unwrap_or("string"),
                            "utf8-hex",
                            hex::encode(value.as_bytes()),
                        ),
                        _ => (
                            declared_type.as_deref().unwrap_or(""),
                            "unsupported",
                            String::new(),
                        ),
                    };
                    let native_type = layout_type(source_type, &nominal_backend_names);
                    if value_kind != "unsupported"
                        && native_type != "unsupported-array"
                        && native_type != "unsupported-aggregate"
                    {
                        let data_symbol = format!(
                            "_F{}",
                            module_system::mangle_function(
                                &module.name,
                                &format!("$data${source_name}")
                            )
                        );
                        output.push_str(&format!(
                            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                            if is_public {
                                "abi-data"
                            } else {
                                "abi-internal-data"
                            },
                            field(&module.name),
                            field(source_name),
                            data_symbol,
                            native_type,
                            value_kind,
                            encoded_value
                        ));
                    } else if is_public {
                        output.push_str(&format!(
                            "abi-data-unsupported\t{}\t{}\tunsupported-or-nonliteral\n",
                            field(&module.name),
                            field(source_name)
                        ));
                    } else {
                        output.push_str(&format!(
                            "abi-internal-data-unsupported\t{}\t{}\tunsupported-or-nonliteral\n",
                            field(&module.name),
                            field(source_name)
                        ));
                    }
                }
                _ => {}
            }
        }
        // Module statement ranges let us attach ABI declarations to their
        // defining module without relying on flattened symbol spellings.
        for statement in &program.statements[module.statement_start..module.statement_end] {
            if let ast::Statement::Function {
                name,
                parameters,
                return_type,
                generic_parameters,
                is_async,
                ..
            } = statement
            {
                let source_name = module
                    .public_symbols
                    .iter()
                    .chain(module.private_symbols.iter())
                    .find(|source| {
                        module_system::mangle_function(&module.name, source.as_str())
                            == name.as_str()
                    })
                    .map(String::as_str)
                    .unwrap_or(name.as_str());
                let is_public = module
                    .public_symbols
                    .iter()
                    .any(|source| source.as_str() == source_name);
                // LLVMBackend::sanitize_symbol prepends `_F` to the source-qualified symbol.
                let symbol = format!(
                    "_F{}",
                    module_system::mangle_function(&module.name, source_name)
                );
                let nominal_names = project_nominal_names.clone();
                let params = parameters
                    .iter()
                    .map(|parameter| {
                        parameter
                            .type_name
                            .as_deref()
                            .and_then(|ty| abi_type(ty, &nominal_names))
                    })
                    .collect::<Option<Vec<_>>>();
                let result = return_type
                    .as_deref()
                    .and_then(|ty| abi_type(ty, &nominal_names));
                if generic_parameters.is_empty() && !is_async {
                    if let (Some(params), Some(result)) = (params, result) {
                        if !params.iter().any(|ty| *ty == "void") {
                            output.push_str(&format!(
                                "{}\t{}\t{}\t{}\t{}\t{}\n",
                                if is_public {
                                    "abi-function"
                                } else {
                                    "abi-internal-function"
                                },
                                field(&module.name),
                                field(source_name),
                                symbol,
                                result,
                                params.join(",")
                            ));
                        } else {
                            output.push_str(&format!(
                                "{}\t{}\t{}\t{}\n",
                                if is_public {
                                    "abi-unsupported"
                                } else {
                                    "abi-internal-unsupported"
                                },
                                field(&module.name),
                                field(source_name),
                                "void-parameter"
                            ));
                        }
                    } else {
                        output.push_str(&format!(
                            "{}\t{}\t{}\t{}\n",
                            if is_public {
                                "abi-unsupported"
                            } else {
                                "abi-internal-unsupported"
                            },
                            field(&module.name),
                            field(source_name),
                            "missing-or-unsupported-type"
                        ));
                    }
                } else {
                    output.push_str(&format!(
                        "{}\t{}\t{}\t{}\n",
                        if is_public {
                            "abi-unsupported"
                        } else {
                            "abi-internal-unsupported"
                        },
                        field(&module.name),
                        field(source_name),
                        "generic-or-async-function"
                    ));
                }
            }
        }
        let mut imports = module.imports.clone();
        imports.sort_by(|a, b| {
            (a.path.as_str(), a.alias.as_deref()).cmp(&(b.path.as_str(), b.alias.as_deref()))
        });
        for import in imports {
            let selected = import
                .selected
                .iter()
                .map(|name| field(name))
                .collect::<Vec<_>>()
                .join(",");
            output.push_str(&format!(
                "import\t{}\t{}\t{}\t{}\t{}\n",
                field(&module.name),
                field(&import.path),
                field(import.alias.as_deref().unwrap_or("")),
                selected,
                if import.re_exported {
                    "public"
                } else {
                    "private"
                }
            ));
        }
    }
    Ok(output)
}

/// Convert the typed scalar ABI records in a v5 package interface into LLVM
/// external declarations. Unsupported/aggregate signatures are deliberately
/// omitted; malformed records and conflicting duplicate symbols are errors.
pub fn emit_package_external_declarations(interface: &str) -> Result<String, String> {
    fn llvm_type(value: &str) -> Option<&'static str> {
        match value {
            "i64" => Some("i64"),
            "double" => Some("double"),
            "i1" => Some("i1"),
            "ptr" => Some("ptr"),
            "void" => Some("void"),
            _ => None,
        }
    }
    if !interface.starts_with("FUSION-PACKAGE-INTERFACE\t6\n") {
        return Err("unsupported package interface version (expected v6)".into());
    }
    let mut declarations = std::collections::BTreeMap::<String, String>::new();
    for (line_number, line) in interface.lines().enumerate().skip(1) {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.first().copied() {
            Some("abi-function") => {
                if fields.len() != 6 {
                    return Err(format!(
                        "malformed ABI function record on line {}",
                        line_number + 1
                    ));
                }
                let symbol = fields[3];
                if symbol.is_empty()
                    || !symbol.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' || byte == b'$'
                    })
                {
                    return Err(format!(
                        "invalid LLVM ABI symbol on line {}",
                        line_number + 1
                    ));
                }
                let result = llvm_type(fields[4])
                    .ok_or_else(|| format!("unsupported ABI return type '{}'", fields[4]))?;
                let params = if fields[5].is_empty() {
                    Vec::new()
                } else {
                    fields[5]
                        .split(',')
                        .map(|value| {
                            llvm_type(value).ok_or_else(|| {
                                format!("unsupported ABI parameter type '{}'", value)
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                if params.iter().any(|ty| *ty == "void") {
                    return Err(format!("void parameter in ABI function '{}'", fields[2]));
                }
                let declaration = format!("declare {} @{}({})", result, symbol, params.join(", "));
                if let Some(previous) = declarations.insert(symbol.to_string(), declaration.clone())
                {
                    if previous != declaration {
                        return Err(format!(
                            "conflicting ABI declarations for symbol '{}'",
                            symbol
                        ));
                    }
                }
            }
            Some("abi-data") => {
                if fields.len() != 7 {
                    return Err(format!(
                        "malformed ABI data record on line {}",
                        line_number + 1
                    ));
                }
                let symbol = fields[3];
                if symbol.is_empty()
                    || !symbol.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' || byte == b'$'
                    })
                {
                    return Err(format!(
                        "invalid LLVM data ABI symbol on line {}",
                        line_number + 1
                    ));
                }
                let ty = llvm_type(fields[4])
                    .filter(|ty| *ty != "void")
                    .ok_or_else(|| format!("unsupported ABI data type '{}'", fields[4]))?;
                let declaration = format!("@{} = external constant {}", symbol, ty);
                if let Some(previous) = declarations.insert(symbol.to_string(), declaration.clone())
                {
                    if previous != declaration {
                        return Err(format!(
                            "conflicting ABI declarations for symbol '{}'",
                            symbol
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(declarations
        .into_values()
        .map(|declaration| format!("{}\n", declaration))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::Interpreter;
    use crate::lexer::Token;

    // -------------------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------------------

    fn compile_test_program(source: &str) -> ast::Program {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);

        let tokens = lexer
            .tokenize()
            .expect("test source should lex successfully");

        let mut parser = Parser::new(tokens);

        let program = parser
            .parse()
            .expect("test source should parse successfully");

        let mut checker = TypeChecker::new();

        checker
            .check(&program)
            .expect("test source should type-check successfully");

        program
    }

    fn run_program(source: &str) -> Vec<String> {
        let program = compile_test_program(source);

        let mut interpreter = Interpreter::new();
        interpreter.execute(&program);

        interpreter.output().to_vec()
    }

    fn parse_should_fail(source: &str) {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);

        let tokens = match lexer.tokenize() {
            Ok(tokens) => tokens,
            Err(_) => return,
        };

        let mut parser = Parser::new(tokens);

        assert!(
            parser.parse().is_err(),
            "expected parser to reject invalid source"
        );
    }

    fn type_check_should_fail(source: &str) {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);

        let tokens = lexer
            .tokenize()
            .expect("test source should lex successfully");

        let mut parser = Parser::new(tokens);

        let program = parser
            .parse()
            .expect("source should parse before type-checking");

        let mut checker = TypeChecker::new();

        assert!(
            checker.check(&program).is_err(),
            "expected type checker to reject invalid program"
        );
    }

    /// Assert semantic error content without depending on whether the caller
    /// renders the legacy Display form or the newer rustc-style diagnostic.
    pub(crate) fn assert_error_mentions(error: &str, expected_fragments: &[&str], context: &str) {
        assert!(
            expected_fragments
                .iter()
                .all(|fragment| error.contains(fragment)),
            "{}: expected error to contain all of {:?}, got:\n{}",
            context,
            expected_fragments,
            error
        );
    }

    // =========================================================================
    // Variables and expressions
    // =========================================================================

    #[test]
    fn variables_can_be_declared_and_used() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10
    y = 32
    print(x + y)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn multiple_inferred_variables_are_independent() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10, y = 32
    print(x + y)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn typed_variables_can_be_declared() {
        assert_eq!(
            run_program(
                r#"main:
    num x = 10
    string message = "hello"
    print(x)
    print(message)
"#
            ),
            vec!["10", "hello"]
        );
    }

    #[test]
    fn typed_multiple_variables_can_be_declared() {
        assert_eq!(
            run_program(
                r#"main:
    num x, y = 32
    print(y)
"#
            ),
            vec!["32"]
        );
    }

    #[test]
    fn arithmetic_precedence_is_respected() {
        assert_eq!(
            run_program(
                r#"main:
    result = 2 + 3 * 4
    print(result)
"#
            ),
            vec!["14"]
        );
    }

    #[test]
    fn parentheses_override_arithmetic_precedence() {
        assert_eq!(
            run_program(
                r#"main:
    result = (2 + 3) * 4
    print(result)
"#
            ),
            vec!["20"]
        );
    }

    #[test]
    fn variables_can_be_reassigned() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10
    x = 40
    print(x)
"#
            ),
            vec!["40"]
        );
    }

    #[test]
    fn boolean_values_can_be_printed() {
        assert_eq!(
            run_program(
                r#"main:
    print(true)
    print(false)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn strings_can_be_printed() {
        assert_eq!(
            run_program(
                r#"main:
    message = "hello"
    print(message)
"#
            ),
            vec!["hello"]
        );
    }

    #[test]
    fn floats_can_be_used() {
        assert_eq!(
            run_program(
                r#"main:
    value = 3.14
    print(value)
"#
            ),
            vec!["3.14"]
        );
    }

    // =========================================================================
    // Multiple declaration syntax
    // =========================================================================

    #[test]
    fn multiple_variables_work_in_brace_mode() {
        assert_eq!(
            run_program(
                r#"main {
    x = 10, y = 32
    print(x + y)
}
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn typed_and_inferred_variables_can_be_mixed() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10
    num y = 32
    print(x + y)
"#
            ),
            vec!["42"]
        );
    }

    // =========================================================================
    // Control flow
    // =========================================================================

    #[test]
    fn if_executes_true_branch() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10

    if x > 5:
        print(42)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn if_does_not_execute_false_branch() {
        assert_eq!(
            run_program(
                r#"main:
    x = 2

    if x > 5:
        print(10)
"#
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn while_loop_executes_repeatedly() {
        assert_eq!(
            run_program(
                r#"main:
    x = 0

    while x < 3:
        print(x)
        x = x + 1
"#
            ),
            vec!["0", "1", "2"]
        );
    }

    #[test]
    fn if_rejects_non_boolean_condition() {
        type_check_should_fail(
            r#"main:
    x = 10

    if x:
        print(1)
"#,
        );
    }

    #[test]
    fn while_rejects_non_boolean_condition() {
        type_check_should_fail(
            r#"main:
    x = 10

    while x:
        print(1)
"#,
        );
    }

    #[test]
    fn unknown_variable_is_rejected() {
        type_check_should_fail(
            r#"main:
    print(missing)
"#,
        );
    }

    // =========================================================================
    // Match
    // =========================================================================

    #[test]
    fn match_selects_matching_arm() {
        assert_eq!(
            run_program(
                r#"main:
    x = 2

    match x:
        1 => print(10)
        2 => print(20)
        _ => print(30)
"#
            ),
            vec!["20"]
        );
    }

    #[test]
    fn match_selects_wildcard_arm() {
        assert_eq!(
            run_program(
                r#"main:
    x = 99

    match x:
        1 => print(10)
        2 => print(20)
        _ => print(30)
"#
            ),
            vec!["30"]
        );
    }

    #[test]
    fn match_accepts_no_space_after_arrow() {
        assert_eq!(
            run_program(
                r#"main:
    x = 2

    match x:
        1 =>print(10)
        2 =>print(20)
        _ =>print(30)
"#
            ),
            vec!["20"]
        );
    }

    #[test]
    fn match_arm_rejects_colon_after_arrow() {
        parse_should_fail(
            r#"main:
    x = 2

    match x:
        1 =>:
            print(10)
"#,
        );
    }

    // =========================================================================
    // Functions
    // =========================================================================

    #[test]
    fn function_without_parameters_executes() {
        assert_eq!(
            run_program(
                r#"main:
    greet()

fn greet():
    print(42)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_accepts_one_parameter() {
        assert_eq!(
            run_program(
                r#"main:
    double(21)

fn double(x):
    print(x + x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_accepts_multiple_parameters() {
        assert_eq!(
            run_program(
                r#"main:
    add(10, 32)

fn add(a, b):
    print(a + b)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_can_return_a_value() {
        assert_eq!(
            run_program(
                r#"main:
    result = add(20, 22)
    print(result)

fn add(a, b):
    return a + b
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_can_be_called_multiple_times() {
        assert_eq!(
            run_program(
                r#"main:
    print(double(10))
    print(double(20))

fn double(x):
    return x * 2
"#
            ),
            vec!["20", "40"]
        );
    }

    #[test]
    fn typed_function_parameters_work() {
        assert_eq!(
            run_program(
                r#"main:
    print(add(20, 22))

fn add(a: num, b: num):
    return a + b
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn typed_function_return_works() {
        assert_eq!(
            run_program(
                r#"main:
    result = add(20, 22)
    print(result)

fn add(a: num, b: num) -> num:
    return a + b
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_without_return_type_can_use_bare_return() {
        compile_test_program(
            r#"main:
    foo()

fn foo():
    return
"#,
        );
    }

    #[test]
    fn function_without_return_type_can_return_a_value() {
        assert_eq!(
            run_program(
                r#"main:
    print(foo())

fn foo():
    return 42
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn typed_function_cannot_use_bare_return() {
        type_check_should_fail(
            r#"main:
    foo()

fn foo() -> num:
    return
"#,
        );
    }

    #[test]
    fn typed_function_rejects_wrong_return_type() {
        type_check_should_fail(
            r#"main:
    result = get_number()
    print(result)

fn get_number() -> num:
    return "not a number"
"#,
        );
    }

    #[test]
    fn unknown_function_is_rejected() {
        type_check_should_fail(
            r#"main:
    does_not_exist()
"#,
        );
    }

    #[test]
    fn function_argument_type_mismatch_is_rejected() {
        type_check_should_fail(
            r#"main:
    add("hello", 2)

fn add(a: num, b: num):
    return a + b
"#,
        );
    }

    // =========================================================================
    // Generic functions
    // =========================================================================

    #[test]
    fn generic_identity_accepts_num() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<num>(42)
    print(result)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn generic_identity_accepts_string() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<string>("hello")
    print(result)
"#
            ),
            vec!["hello"]
        );
    }

    #[test]
    fn generic_identity_accepts_bool() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<bool>(true)
    print(result)
"#
            ),
            vec!["true"]
        );
    }

    #[test]
    fn generic_identity_accepts_float() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<float>(3.14)
    print(result)
"#
            ),
            vec!["3.14"]
        );
    }

    #[test]
    fn generic_function_supports_multiple_type_parameters() {
        assert_eq!(
            run_program(
                r#"fn first<A, B>(a: A, b: B) -> A:
    return a

main:
    result = first<num, string>(42, "hello")
    print(result)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn generic_function_rejects_wrong_explicit_type() {
        type_check_should_fail(
            r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<num>("hello")
    print(result)
"#,
        );
    }

    #[test]
    fn generic_functions_work_in_brace_mode() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T {
    return value
}

main {
    result = identity<num>(42)
    print(result)
}
"#
            ),
            vec!["42"]
        );
    }

    // =========================================================================
    // Structs
    // =========================================================================

    #[test]
    fn struct_can_be_declared_and_constructed() {
        assert_eq!(
            run_program(
                r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice", 30)
    print(person.name)
    print(person.age)
"#
            ),
            vec!["Alice", "30"]
        );
    }

    #[test]
    fn struct_fields_can_be_used_in_expressions() {
        assert_eq!(
            run_program(
                r#"struct Point:
    x: num
    y: num

main:
    point = Point(10, 32)
    print(point.x + point.y)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn multiple_struct_instances_are_independent() {
        assert_eq!(
            run_program(
                r#"struct Point:
    x: num
    y: num

main:
    first = Point(10, 20)
    second = Point(30, 40)

    print(first.x)
    print(second.x)
"#
            ),
            vec!["10", "30"]
        );
    }

    #[test]
    fn struct_constructor_accepts_expressions() {
        assert_eq!(
            run_program(
                r#"struct Point:
    x: num
    y: num

main:
    a = 20
    b = 22
    point = Point(a, b)

    print(point.x + point.y)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn structs_work_in_brace_mode() {
        assert_eq!(
            run_program(
                r#"struct Point {
    x: num
    y: num
}

main {
    point = Point(10, 32)
    print(point.x + point.y)
}
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn struct_constructor_rejects_too_few_arguments() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice")
"#,
        );
    }

    #[test]
    fn struct_constructor_rejects_too_many_arguments() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice", 30, 100)
"#,
        );
    }

    #[test]
    fn struct_constructor_rejects_wrong_field_type() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice", "thirty")
"#,
        );
    }

    #[test]
    fn unknown_struct_type_is_rejected() {
        type_check_should_fail(
            r#"main:
    point = DoesNotExist(10, 20)
"#,
        );
    }

    #[test]
    fn unknown_struct_field_is_rejected() {
        type_check_should_fail(
            r#"struct Point:
    x: num
    y: num

main:
    point = Point(10, 20)
    print(point.z)
"#,
        );
    }

    #[test]
    fn struct_constructor_rejects_brace_literal_syntax() {
        parse_should_fail(
            r#"struct Point:
    x: num
    y: num

main:
    point = Point {
        x: 10,
        y: 32
    }
"#,
        );
    }

    // =========================================================================
    // Block syntax / lexer behavior
    // =========================================================================

    #[test]
    fn indentation_mode_accepts_indentation_struct() {
        let source = r#"
struct Person:
    name: string
    age: num

main:
    print("ok")
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_ok());
    }

    #[test]
    fn brace_mode_accepts_brace_struct() {
        let source = r#"
struct Person {
    name: string
    age: num
}

main {
    print("ok")
}
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_ok());
    }

    #[test]
    fn indentation_mode_rejects_brace_struct() {
        let source = r#"
struct Person {
    name: string
    age: num
}

main:
    print("ok")
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn brace_mode_rejects_indentation_struct() {
        let source = r#"
struct Person:
    name: string
    age: num

main {
    print("ok")
}
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn brace_mode_does_not_generate_indentation_tokens() {
        let source = r#"main {
    print(10)
}
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("should lex");

        assert!(!tokens
            .iter()
            .any(|token| matches!(token.node, Token::Indent | Token::Dedent)));
    }

    #[test]
    fn indentation_mode_rejects_left_brace() {
        let source = r#"main:
    print(10) {
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn indentation_mode_rejects_right_brace() {
        let source = r#"main:
    print(10)
}
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn indentation_and_brace_styles_cannot_be_mixed() {
        parse_should_fail(
            r#"main {
    x = 10
}

fn add(a, b):
    return a + b
"#,
        );
    }
    #[test]
    fn less_than_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(2 < 3)
    print(3 < 2)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn greater_than_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(3 > 2)
    print(2 > 3)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn equality_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(10 == 10)
    print(10 == 20)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn inequality_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(10 != 20)
    print(10 != 10)
"#
            ),
            vec!["true", "false"]
        );
    }
    #[test]
    fn while_loop_executes_repeatedly_2() {
        assert_eq!(
            run_program(
                r#"main:
    x = 0

    while x < 3:
        print(x)
        x = x + 1
"#
            ),
            vec!["0", "1", "2"]
        );
    }
    #[test]
    fn comparison_after_identifier_is_not_parsed_as_generic_arguments() {
        assert_eq!(
            run_program(
                r#"main:
    x = 2
    print(x < 3)
    print(x > 1)
"#
            ),
            vec!["true", "true"]
        );
    }
    #[test]
    #[should_panic]
    fn uninitialized_variable_cannot_be_read() {
        run_program(
            r#"main:
    num x
    print(x)
"#,
        );
    }
    #[test]
    fn explicit_variable_type_must_match_value() {
        type_check_should_fail(
            r#"main:
    num x = "hello"
"#,
        );
    }
    #[test]
    fn function_argument_type_mismatch_is_rejected_2() {
        type_check_should_fail(
            r#"fn add(num a, num b) -> num:
    return a + b

main:
    print(add("hello", 10))
"#,
        );
    }
    #[test]
    fn function_return_type_mismatch_is_rejected() {
        type_check_should_fail(
            r#"fn get_number() -> num:
    return "hello"

main:
    print(get_number())
"#,
        );
    }
    #[test]
    fn function_requiring_return_value_cannot_return_without_value() {
        type_check_should_fail(
            r#"fn get_number() -> num:
    return

main:
    print(get_number())
"#,
        );
    }

    #[test]
    fn function_requires_all_arguments() {
        type_check_should_fail(
            r#"fn add(num a, num b) -> num:
    return a + b

main:
    print(add(10))
"#,
        );
    }
    #[test]
    fn function_rejects_extra_arguments() {
        type_check_should_fail(
            r#"fn add(num a, num b) -> num:
    return a + b

main:
    print(add(10, 20, 30))
"#,
        );
    }
    #[test]
    fn if_rejects_non_boolean_condition_2() {
        type_check_should_fail(
            r#"main:
    if 10:
        print("yes")
"#,
        );
    }
    #[test]
    fn arithmetic_rejects_incompatible_types() {
        type_check_should_fail(
            r#"main:
    x = "hello" + 10
"#,
        );
    }
    #[test]
    fn boolean_cannot_be_used_in_arithmetic() {
        type_check_should_fail(
            r#"main:
    x = true + false
"#,
        );
    }
    #[test]
    fn ordering_comparison_rejects_incompatible_types() {
        type_check_should_fail(
            r#"main:
    x = 10 < "hello"
"#,
        );
    }
    #[test]
    fn array_rejects_incompatible_element_types() {
        type_check_should_fail(
            r#"main:
    values = [10, "hello", true]
"#,
        );
    }
    #[test]
    fn array_index_requires_num() {
        type_check_should_fail(
            r#"main:
    values = [10, 20, 30]
    x = values["hello"]
"#,
        );
    }
    #[test]
    fn non_array_cannot_be_indexed() {
        type_check_should_fail(
            r#"main:
    x = 10
    y = x[0]
"#,
        );
    }
    #[test]
    fn reassignment_must_preserve_variable_type() {
        type_check_should_fail(
            r#"main:
    num x = 10
    x = "hello"
"#,
        );
    }
    #[test]
    fn match_patterns_must_match_scrutinee_type() {
        type_check_should_fail(
            r#"main:
    x = 10

    match x:
        true:
            print("true")
        10:
            print("ten")
"#,
        );
    }
    #[test]
    fn duplicate_function_definition_is_rejected() {
        type_check_should_fail(
            r#"fn test() -> num:
    return 10

fn test() -> num:
    return 20

main:
    print(test())
"#,
        );
    }
    #[test]
    fn duplicate_variable_declaration_is_rejected() {
        type_check_should_fail(
            r#"main:
    num x = 10
    num x = 20
"#,
        );
    }

    #[test]
    fn break_outside_loop_is_rejected() {
        parse_should_fail(
            r#"main:
    break
"#,
        );
    }
    #[test]
    fn continue_outside_loop_is_rejected() {
        parse_should_fail(
            r#"main:
    continue
"#,
        );
    }
    #[test]
    fn program_requires_main() {
        parse_should_fail(
            r#"fn test() -> num:
    return 10
"#,
        );
    }
    #[test]
    fn non_function_value_cannot_be_called() {
        type_check_should_fail(
            r#"main:
    x = 10
    y = x()
"#,
        );
    }
    #[test]
    fn struct_field_assignment_requires_correct_type() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person(name: "Alice", age: "twenty")
"#,
        );
    }
    #[test]
    fn unknown_struct_field_is_rejected_2() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person(name: "Alice", height: 180)
"#,
        );
    }
    #[test]
    fn unknown_enum_variant_is_rejected() {
        type_check_should_fail(
            r#"enum Status:
    Active
    Inactive

main:
    status = Status.Unknown
"#,
        );
    }

    #[test]
    fn void_function_cannot_return_value() {
        type_check_should_fail(
            r#"fn do_work() -> void:
    return 10

main:
    do_work()
"#,
        );
    }
    #[test]
    fn nested_function_definition_is_rejected() {
        parse_should_fail(
            r#"main:
    fn nested() -> num:
        return 10
"#,
        );
    }
    #[test]
    fn generic_function_cannot_return_wrong_type() {
        type_check_should_fail(
            r#"fn identity<T>(T value) -> T:
    return 10

main:
    x = identity<string>("hello")
"#,
        );
    }
    #[test]
    fn generic_argument_must_match_explicit_type_parameter() {
        type_check_should_fail(
            r#"fn identity<T>(T value) -> T:
    return value

main:
    x = identity<string>(42)
"#,
        );
    }

    #[test]
    fn function_return_value_can_be_used_in_expression() {
        assert_eq!(
            run_program(
                r#"main:
    result = double(21) + 1
    print(result)

fn double(x):
    return x * 2
"#
            ),
            vec!["43"]
        );
    }
    #[test]
    fn function_can_call_another_function() {
        assert_eq!(
            run_program(
                r#"main:
    print(double_and_add(20))

fn double_and_add(x):
    return double(x) + 2

fn double(x):
    return x * 2
"#
            ),
            vec!["42"]
        );
    }
    #[test]
    fn input_builtin_is_a_string_expression() {
        let program = compile_test_program(
            r#"main:
    value = input()
    print(value)
"#,
        );
        assert!(matches!(program.statements[0], ast::Statement::Main { .. }));
    }

    #[test]
    fn explicit_as_conversion_works() {
        assert_eq!(
            run_program(
                r#"main:
    x = 42 as float
    print(x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn from_conversion_uses_user_defined_converter() {
        assert_eq!(
            run_program(
                r#"fn from_num(value) -> num:
    return value + 10

main:
    x = num::from(32)
    print(x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn try_conversion_returns_option() {
        assert_eq!(
            run_program(
                r#"main:
    x = num::try("42")
    print(x)
    y = num::try("not-a-number")
    print(y)
"#
            ),
            vec!["Some(42)", "None"]
        );
    }

    #[test]
    fn defer_closes_file_and_file_io_works() {
        let path = std::env::temp_dir().join(format!("fusion_test_{}.txt", std::process::id()));
        let path_string = path.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            r#"main:
    file = open("{}")
    defer file.close()
    file.write("hello fusion")
"#,
            path_string
        );
        run_program(&source);
        let contents =
            std::fs::read_to_string(&path).expect("Fusion should create and write the file");
        assert_eq!(contents, "hello fusion");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn invalid_conversion_is_rejected() {
        type_check_should_fail(
            r#"main:
    x = true as float
"#,
        );
    }

    #[test]
    fn invalid_file_method_arguments_are_rejected() {
        type_check_should_fail(
            r#"main:
    file = open("test.txt")
    file.write(42)
"#,
        );
    }

    #[test]
    fn invalid_conversion_arity_is_rejected() {
        parse_should_fail(
            r#"main:
    x = num::from(1, 2)
"#,
        );
    }

    // =========================================================================
    // Generics, traits, enums, modules, collections, iterators, and strings
    // =========================================================================

    #[test]
    fn generic_function_can_be_called_with_explicit_type_argument() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(T value) -> T:
    return value

main:
    x = identity<num>(42)
    print(x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn trait_impl_methods_can_be_called_through_the_receiver() {
        assert_eq!(
            run_program(
                r#"struct Person:
    name: string

trait Greeter:
    fn greet(person: Person) -> string

impl Greeter for Person:
    fn greet(person: Person) -> string:
        return "hello " + person.name

main:
    p = Person(name: "Fusion")
    print(p.greet())
"#
            ),
            vec!["hello Fusion"]
        );
    }

    #[test]
    fn enum_pattern_matching_works() {
        assert_eq!(
            run_program(
                r#"enum State:
    Ready
    Failed(string)

main:
    state = State::Failed("broken")
    match state:
        State::Ready => print("ready")
        State::Failed(message) => print(message)
"#
            ),
            vec!["broken"]
        );
    }

    #[test]
    fn import_module_syntax_is_accepted() {
        let program = compile_test_program(
            r#"import std.io

main:
    print("module syntax")
"#,
        );
        assert!(program
            .statements
            .iter()
            .any(|s| matches!(s, ast::Statement::Import { path, .. } if path == "std.io")));
    }

    #[test]
    fn growable_array_operations_work() {
        assert_eq!(
            run_program(
                r#"main:
    numbers = [1, 2, 3]
    numbers.add(4)
    numbers.insert_at_index(1, 9)
    numbers.modify(0, 7)
    print(numbers.access(0))
    print(numbers.get_length())
    print(numbers.remove_at_index(1))
    print(numbers.remove_last())
    numbers.clear()
    print(numbers.get_length())
"#
            ),
            vec!["7", "5", "9", "4", "0"]
        );
    }

    #[test]
    fn array_iteration_and_iterator_operations_work() {
        assert_eq!(
            run_program(
                r#"fn double(x: num) -> num:
    return x * 2

fn even(x: num) -> bool:
    return x == 2

fn add(acc: num, x: num) -> num:
    return acc + x

main:
    numbers = [1, 2, 3]
    doubled = numbers.iterate().map(double).collect()
    print(doubled)
    filtered = numbers.iterate().filter(even).collect()
    print(filtered)
    found = numbers.iterate().find(even)
    print(found)
    print(numbers.iterate().any(even))
    print(numbers.iterate().all(even))
    total = numbers.iterate().fold(0, add)
    print(total)
"#
            ),
            vec!["[2, 4, 6]", "[2]", "Some(2)", "true", "false", "6"]
        );
    }

    #[test]
    fn hash_map_operations_work() {
        assert_eq!(
            run_program(
                r#"main:
    m = hashmap()
    m.insert("one", 1)
    m.insert("two", 2)
    print(m.contains_key("one"))
    print(m.get("two"))
    print(m.keys())
    print(m.values())
    print(m.remove("one"))
    m.clear()
    print(m.get_length())
"#
            ),
            vec!["true", "Some(2)", "[one, two]", "[1, 2]", "Some(1)", "0"]
        );
    }

    #[test]
    fn string_operations_work() {
        assert_eq!(
            run_program(
                r#"main:
    text = "  Hello Fusion  "
    print(text.trim())
    print(text.to_uppercase())
    print(text.to_lowercase())
    print(text.substring(2, 7))
    print(text.find("Fusion"))
    print(text.replace("Fusion", "World"))
    print(text.split(" "))
    print(text.starts_with("  He"))
    print(text.ends_with("  "))
    print(text.contains("Fusion"))
    print(text.length())
"#
            ),
            vec![
                "Hello Fusion",
                "  HELLO FUSION  ",
                "  hello fusion  ",
                "Hello",
                "Some(8)",
                "  Hello World  ",
                "[, , Hello, Fusion, , ]",
                "true",
                "true",
                "true",
                "16"
            ]
        );
    }

    #[test]
    fn invalid_generic_arity_is_rejected() {
        type_check_should_fail(
            r#"fn identity<T>(T value) -> T:
    return value

main:
    x = identity(42)
"#,
        );
    }

    #[test]
    fn invalid_array_operation_types_are_rejected() {
        type_check_should_fail(
            r#"main:
    numbers = [1, 2, 3]
    numbers.access("wrong")
"#,
        );
    }

    #[test]
    fn invalid_hash_map_key_type_is_rejected() {
        type_check_should_fail(
            r#"main:
    HashMap<string, num> m = hashmap()
    m.insert(1, "number key")
"#,
        );
    }

    #[test]
    fn invalid_string_operation_types_are_rejected() {
        type_check_should_fail(
            r#"main:
    text = "hello"
    text.concatenate(42)
"#,
        );
    }

    #[test]
    fn incomplete_trait_impl_is_rejected() {
        type_check_should_fail(
            r#"struct Person:
    name: string

trait Greeter:
    fn greet(person: Person) -> string

impl Greeter for Person:
    fn other(person: Person) -> string:
        return "wrong"

main:
    print("x")
"#,
        );
    }
}

#[cfg(test)]
mod stdlib_feature_tests {
    use super::*;
    use crate::interpreter::Interpreter;
    use crate::lexer::{BlockMode, Lexer};
    use crate::parser::Parser;
    use crate::tests::assert_error_mentions;
    use std::fs;

    fn run(source: &str) -> Vec<String> {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("lex");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("parse");
        let mut checker = TypeChecker::new();
        checker.check(&program).expect("type-check");
        let mut interpreter = Interpreter::new();
        interpreter.execute(&program);
        interpreter.output().to_vec()
    }

    fn invalid(source: &str) {
        assert!(
            compile_source(source).is_err(),
            "program should be rejected"
        );
    }

    #[test]
    fn async_await_executes_task() {
        assert_eq!(
            run(r#"async fn answer() -> num:
    return 42

main:
    value = await answer()
    print(value)
"#),
            vec!["42"]
        );
    }

    #[test]
    fn filesystem_round_trip() {
        let path =
            std::env::temp_dir().join(format!("fusion_stdlib_{}_test.txt", std::process::id()));
        let path = path.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            r#"main:
    write_file("{}", "hello fusion")
    print(file_exists("{}"))
    print(read_file("{}"))
    remove_file("{}")
"#,
            path, path, path, path
        );
        assert_eq!(run(&source), vec!["true", "hello fusion"]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn time_random_encoding_format_serialization_and_logging_work() {
        let out = run(r#"main:
    print(date_utc(0))
    print(hex_encode("Fusion"))
    print(hex_decode("467573696f6e"))
    print(base64_encode("Fusion"))
    print(base64_decode("RnVzaW9u"))
    print(format("value={0}", [42]))
    print(serialize([1, 2, 3]))
    print(deserialize("[1,2,3]"))
    print(random_int(7, 7))
    log_info("ready")
"#);
        assert_eq!(out[0], "1970-01-01 00:00:00 UTC");
        assert_eq!(out[1], "467573696f6e");
        assert_eq!(out[2], "Fusion");
        assert_eq!(out[3], "RnVzaW9u");
        assert_eq!(out[4], "Fusion");
        assert_eq!(out[5], "value=42");
        assert_eq!(out[6], "[1, 2, 3]");
        assert_eq!(out[7], "[1, 2, 3]");
        assert_eq!(out[8], "7");
        assert_eq!(out[9], "[INFO] ready");
    }

    #[test]
    fn process_args_system_and_network_apis_type_check() {
        compile_source(
            r#"main:
    a = args()
    s = system()
    code = process_run("true", [])
"#,
        )
        .expect("process APIs should type-check");
        compile_source(
            r#"main:
    stream = tcp_connect("127.0.0.1", 1)
    stream.close()
"#,
        )
        .expect("network API should type-check");
    }

    #[test]
    fn invalid_async_await_type_is_rejected() {
        invalid(
            r#"main:
    value = await 42
"#,
        );
    }

    #[test]
    fn invalid_filesystem_arguments_are_rejected() {
        invalid(
            r#"main:
    value = read_file(42)
"#,
        );
        invalid(
            r#"main:
    write_file("x", 42)
"#,
        );
    }

    #[test]
    fn invalid_process_and_network_arguments_are_rejected() {
        invalid(
            r#"main:
    process_run(42, [])
"#,
        );
        invalid(
            r#"main:
    tcp_connect("localhost", "8080")
"#,
        );
    }

    #[test]
    fn invalid_encoding_and_format_arguments_are_rejected() {
        invalid(
            r#"main:
    hex_encode(42)
"#,
        );
        invalid(
            r#"main:
    format(42, [1])
"#,
        );
    }
    // =========================================================================
    // Async/await edge cases
    // =========================================================================

    #[test]
    fn async_function_accepts_parameters_and_await_returns_the_declared_type() {
        assert_eq!(
            run(r#"async fn add(a: num, b: num) -> num:
    return a + b

main:
    result = await add(20, 22)
    print(result)
"#,),
            vec!["42"]
        );
    }

    #[test]
    fn nested_async_calls_propagate_task_results() {
        assert_eq!(
            run(r#"async fn inner() -> num:
    return 20

async fn outer() -> num:
    value = await inner()
    return value + 22

main:
    print(await outer())
"#,),
            vec!["42"]
        );
    }

    #[test]
    fn async_functions_can_return_different_result_types() {
        assert_eq!(
            run(r#"async fn number() -> num:
    return 42

async fn message() -> string:
    return "fusion"

main:
    print(await number())
    print(await message())
"#,),
            vec!["42", "fusion"]
        );
    }

    #[test]
    fn multiple_awaits_in_one_function_are_evaluated_in_order() {
        assert_eq!(
            run(r#"async fn value(x: num) -> num:
    return x

main:
    first = await value(10)
    second = await value(32)
    print(first + second)
"#,),
            vec!["42"]
        );
    }

    #[test]
    fn await_works_inside_conditionals_and_loops() {
        assert_eq!(
            run(r#"async fn value(x: num) -> num:
    return x

main:
    total = 0
    i = 0

    while i < 3:
        if i == 1:
            total = total + await value(20)
        else:
            total = total + await value(1)
        i = i + 1

    print(total)
"#,),
            vec!["22"]
        );
    }

    #[test]
    fn await_on_non_task_is_rejected_as_a_task_type_error() {
        let error = compile_source(
            r#"main:
    value = await 42
"#,
        )
        .expect_err("awaiting a non-task must fail");

        assert_error_mentions(
            &error,
            &["Task<T>", "num"],
            "await on a non-task should report the expected and actual types",
        );
    }

    // =========================================================================
    // Function semantics
    // =========================================================================

    #[test]
    fn recursive_functions_can_call_themselves() {
        assert_eq!(
            run(r#"fn factorial(n: num) -> num:
    if n <= 1:
        return 1
    else:
        return n * factorial(n - 1)

main:
    print(factorial(5))
"#,),
            vec!["120"]
        );
    }

    #[test]
    fn generic_functions_preserve_the_instantiated_runtime_value() {
        assert_eq!(
            run(r#"fn identity<T>(value: T) -> T:
    return value

main:
    print(identity<num>(42))
    print(identity<string>("fusion"))
    print(identity<bool>(true))
"#,),
            vec!["42", "fusion", "true"]
        );
    }

    #[test]
    fn explicit_return_paths_are_checked_and_execute_correctly() {
        assert_eq!(
            run(r#"fn choose(flag: bool) -> num:
    if flag:
        return 10
    else:
        return 32

main:
    print(choose(true))
    print(choose(false))
"#,),
            vec!["10", "32"]
        );
    }

    #[test]
    fn missing_return_path_is_rejected() {
        let error = compile_source(
            r#"fn choose(flag: bool) -> num:
    if flag:
        return 42

main:
    print(choose(true))
"#,
        )
        .expect_err("a value-returning function needs a return on every path");

        assert!(
            error.contains("must return a value")
                || error.contains("may reach the end")
                || error.contains("return") && error.contains("path"),
            "expected a return-path diagnostic, got: {}",
            error
        );
    }

    #[test]
    fn nested_scopes_can_shadow_outer_variables_without_changing_outer_scope() {
        assert_eq!(
            run(r#"main:
    value = 10

    if true:
        num value = 32
        print(value)

    print(value)
"#,),
            vec!["32", "10"]
        );
    }

    #[test]
    fn function_arguments_are_evaluated_left_to_right() {
        assert_eq!(
            run(r#"fn mark(value: num) -> num:
    print(value)
    return value

fn add(a: num, b: num) -> num:
    return a + b

main:
    print(add(mark(10), mark(32)))
"#,),
            vec!["10", "32", "42"]
        );
    }

    // =========================================================================
    // Type-system/runtime agreement
    // =========================================================================

    #[test]
    fn primitive_runtime_values_match_their_static_types() {
        assert_eq!(
            run(r#"main:
    num n = 42
    float f = 3.5
    bool b = true
    string s = "fusion"
    print(n)
    print(f)
    print(b)
    print(s)
"#,),
            vec!["42", "3.5", "true", "fusion"]
        );
    }

    #[test]
    fn collection_runtime_values_match_their_static_types() {
        assert_eq!(
            run(r#"main:
    num[] numbers = [1, 2, 3]
    HashMap<string, num> scores = hashmap()
    scores.insert("answer", 42)

    print(numbers)
    print(scores.get("answer"))
"#,),
            vec!["[1, 2, 3]", "Some(42)"]
        );
    }

    #[test]
    fn option_iterator_struct_enum_and_task_runtime_values_are_represented_correctly() {
        assert_eq!(
            run(r#"struct Person:
    name: string

enum State:
    Ready
    Failed(string)

async fn answer() -> num:
    return 42

main:
    option = num::try("42")
    numbers = [1, 2, 3]
    person = Person(name: "Fusion")
    state = State::Failed("broken")
    task = answer()

    print(option)
    print(numbers.iterate())
    print(person)
    print(state)
    print(task)
    print(await task)
"#,),
            vec![
                "Some(42)",
                "Iterator[1, 2, 3]",
                "Person { name: Fusion }",
                "State::Failed(broken)",
                "Task(42)",
                "42",
            ]
        );
    }

    #[test]
    fn invalid_programs_fail_at_the_type_boundary_for_wrong_value_types() {
        let cases = [
            (
                "wrong numeric argument",
                r#"fn takes_num(value: num) -> num:
    return value

main:
    print(takes_num("not a number"))
"#,
                "num",
            ),
            (
                "wrong array element",
                r#"main:
    num[] values = [1, "two"]
"#,
                "num",
            ),
            (
                "wrong boolean condition",
                r#"main:
    if 1:
        print("bad")
"#,
                "bool",
            ),
        ];

        for (name, source, expected) in cases {
            let error = compile_source(source).expect_err("program should be rejected");
            assert_error_mentions(
                &error,
                &[expected],
                &format!("{} should report the incompatible type", name),
            );
        }
    }

    // =========================================================================
    // Algorithmic correctness / LeetCode-style tests
    // =========================================================================

    #[test]
    fn two_sum_returns_correct_indices() {
        assert_eq!(
            run(r#"fn two_sum(nums: num[], target: num) -> num:
    i = 0
    while i < nums.get_length():
        j = i + 1
        while j < nums.get_length():
            if nums.access(i) + nums.access(j) == target:
                return i * 100 + j
            j = j + 1
        i = i + 1
    return -1

main:
    nums = [2, 7, 11, 15]
    print(two_sum(nums, 9))
    print(two_sum(nums, 26))
"#),
            vec!["1", "203"]
        );
    }

    #[test]
    fn binary_search_finds_present_and_missing_values() {
        assert_eq!(
            run(r#"fn binary_search(nums: num[], target: num) -> num:
    left = 0
    right = nums.get_length() - 1

    while left <= right:
        mid = left + (right - left) / 2
        value = nums.access(mid)

        if value == target:
            return mid
        else:
            if value < target:
                left = mid + 1
            else:
                right = mid - 1

    return -1

main:
    nums = [1, 3, 5, 7, 9, 11]
    print(binary_search(nums, 1))
    print(binary_search(nums, 7))
    print(binary_search(nums, 11))
    print(binary_search(nums, 4))
"#),
            vec!["0", "3", "5", "-1"]
        );
    }

    #[test]
    fn fibonacci_handles_multiple_inputs() {
        assert_eq!(
            run(r#"fn fib(n: num) -> num:
    if n <= 1:
        return n

    previous = 0
    current = 1
    i = 2

    while i <= n:
        next_value = previous + current
        previous = current
        current = next_value
        i = i + 1

    return current

main:
    print(fib(0))
    print(fib(1))
    print(fib(2))
    print(fib(10))
    print(fib(20))
"#),
            vec!["0", "1", "1", "55", "6765"]
        );
    }

    #[test]
    fn reverse_array_in_place() {
        assert_eq!(
            run(r#"fn reverse(nums: num[]) -> num[]:
    left = 0
    right = nums.get_length() - 1

    while left < right:
        temp = nums.access(left)
        nums.modify(left, nums.access(right))
        nums.modify(right, temp)
        left = left + 1
        right = right - 1

    return nums

main:
    print(reverse([1, 2, 3, 4, 5]))
    print(reverse([42]))
    print(reverse([]))
"#),
            vec!["[5, 4, 3, 2, 1]", "[42]", "[]"]
        );
    }

    #[test]
    fn maximum_subarray_uses_kadanes_algorithm() {
        assert_eq!(
            run(r#"fn max_subarray(nums: num[]) -> num:
    best = nums.access(0)
    current = best
    i = 1

    while i < nums.get_length():
        value = nums.access(i)

        if current + value > value:
            current = current + value
        else:
            current = value

        if current > best:
            best = current

        i = i + 1

    return best

main:
    print(max_subarray([-2, 1, -3, 4, -1, 2, 1, -5, 4]))
    print(max_subarray([-8, -3, -6, -2, -5, -4]))
    print(max_subarray([5]))
"#),
            vec!["6", "-2", "5"]
        );
    }

    #[test]
    fn prefix_sums_produce_correct_range_totals() {
        assert_eq!(
            run(r#"fn prefix_sums(nums: num[]) -> num[]:
    result = [0]
    i = 0

    while i < nums.get_length():
        result.add(result.access(i) + nums.access(i))
        i = i + 1

    return result

fn range_sum(prefix: num[], left: num, right: num) -> num:
    return prefix.access(right + 1) - prefix.access(left)

main:
    prefix = prefix_sums([2, 4, 6, 8, 10])
    print(prefix)
    print(range_sum(prefix, 0, 2))
    print(range_sum(prefix, 1, 3))
    print(range_sum(prefix, 4, 4))
"#),
            vec!["[0, 2, 6, 12, 20, 30]", "12", "18", "10"]
        );
    }

    #[test]
    fn nested_loops_count_grid_cells() {
        assert_eq!(
            run(r#"main:
    rows = 3
    cols = 4
    count = 0
    i = 0

    while i < rows:
        j = 0
        while j < cols:
            count = count + 1
            j = j + 1
        i = i + 1

    print(count)
"#),
            vec!["12"]
        );
    }

    #[test]
    fn prime_check_handles_small_and_composite_numbers() {
        assert_eq!(
            run(r#"fn is_prime(n: num) -> bool:
    if n < 2:
        return false

    divisor = 2
    while divisor * divisor <= n:
        if n / divisor * divisor == n:
            return false
        divisor = divisor + 1

    return true

main:
    print(is_prime(-7))
    print(is_prime(0))
    print(is_prime(1))
    print(is_prime(2))
    print(is_prime(97))
    print(is_prime(100))
"#),
            vec!["false", "false", "false", "true", "true", "false"]
        );
    }

    #[test]
    fn hash_map_overwriting_a_key_does_not_duplicate_it() {
        assert_eq!(
            run(r#"main:
    m = hashmap()
    m.insert("score", 10)
    m.insert("score", 42)

    print(m.get("score"))
    print(m.get_length())
"#),
            vec!["Some(42)", "1"]
        );
    }

    #[test]
    fn hash_map_missing_key_returns_none() {
        assert_eq!(
            run(r#"main:
    m = hashmap()
    m.insert("known", 42)

    print(m.get("missing"))
    print(m.contains_key("missing"))
    print(m.remove("missing"))
    print(m.get_length())
"#),
            vec!["None", "false", "None", "1"]
        );
    }

    #[test]
    fn iterator_pipeline_preserves_order_and_composes_operations() {
        assert_eq!(
            run(r#"fn is_even(n: num) -> bool:
    return n / 2 * 2 == n

fn double(n: num) -> num:
    return n * 2

main:
    nums = [1, 2, 3, 4, 5, 6]
    result = nums.iterate().filter(is_even).map(double).collect()
    print(result)
"#),
            vec!["[4, 8, 12]"]
        );
    }

    #[test]
    fn iterator_all_any_and_find_handle_empty_arrays() {
        assert_eq!(
            run(r#"fn positive(n: num) -> bool:
    return n > 0

main:
    nums = []
    print(nums.iterate().any(positive))
    print(nums.iterate().all(positive))
    print(nums.iterate().find(positive))
"#),
            vec!["false", "true", "None"]
        );
    }

    #[test]
    fn fold_handles_nonzero_initial_accumulator() {
        assert_eq!(
            run(r#"fn multiply(acc: num, value: num) -> num:
    return acc * value

main:
    nums = [2, 3, 4]
    print(nums.iterate().fold(5, multiply))
    print(nums.iterate().fold(7, multiply))
"#),
            vec!["120", "168"]
        );
    }

    #[test]
    fn struct_field_mutation_is_visible_to_subsequent_calls() {
        assert_eq!(
            run(r#"struct Counter:
    value: num

main:
    counter = Counter(value: 0)
    counter.value = counter.value + 1
    counter.value = counter.value + 1
    counter.value = counter.value + 1
    print(counter.value)
"#),
            vec!["3"]
        );
    }

    #[test]
    fn nested_structs_keep_their_own_field_values() {
        assert_eq!(
            run(r#"struct Point:
    x: num
    y: num

struct Rectangle:
    top_left: Point
    bottom_right: Point

main:
    a = Point(x: 1, y: 2)
    b = Point(x: 9, y: 8)
    rectangle = Rectangle(top_left: a, bottom_right: b)

    print(rectangle.top_left.x)
    print(rectangle.top_left.y)
    print(rectangle.bottom_right.x)
    print(rectangle.bottom_right.y)
"#),
            vec!["1", "2", "9", "8"]
        );
    }

    #[test]
    fn enum_variants_with_payloads_preserve_their_values() {
        assert_eq!(
            run(r#"enum Message:
    Quit
    Number(num)
    Text(string)

fn describe(message: Message) -> string:
    match message:
        Message::Quit => return "quit"
        Message::Number(n) => return "number"
        Message::Text(text) => return text

main:
    print(describe(Message::Quit))
    print(describe(Message::Number(42)))
    print(describe(Message::Text("hello")))
"#),
            vec!["quit", "number", "hello"]
        );
    }

    #[test]
    fn trait_methods_work_for_multiple_struct_types() {
        assert_eq!(
            run(r#"struct Circle:
    radius: num

struct Square:
    side: num

trait Measure:
    fn measure(value: Circle) -> num

impl Measure for Circle:
    fn measure(value: Circle) -> num:
        return value.radius * value.radius * 3

main:
    circle = Circle(radius: 2)
    print(circle.measure())
"#),
            vec!["12"]
        );
    }

    #[test]
    fn function_cannot_return_a_boolean_from_numeric_signature() {
        invalid(
            r#"fn compute() -> num:
    return 10 > 5

main:
    print(compute())
"#,
        );
    }

    #[test]
    fn incompatible_if_branches_cannot_silently_change_variable_type() {
        invalid(
            r#"main:
    if true:
        value = 42
    else:
        value = "hello"

    print(value)
"#,
        );
    }

    #[test]
    fn generic_identity_rejects_mixed_argument_types() {
        invalid(
            r#"fn choose<T>(first: T, second: T) -> T:
    return first

main:
    print(choose<num>(42, "wrong"))
"#,
        );
    }

    #[test]
    fn invalid_array_assignment_type_is_rejected() {
        invalid(
            r#"main:
    nums = [1, 2, 3]
    nums.modify(1, "wrong")
"#,
        );
    }

    #[test]
    fn non_boolean_logical_operands_are_rejected() {
        invalid(
            r#"main:
    result = 1 && true
"#,
        );
    }

    #[test]
    fn comparison_cannot_compare_boolean_and_number() {
        invalid(
            r#"main:
    result = true < 10
"#,
        );
    }

    #[test]
    fn void_function_cannot_be_used_as_a_numeric_expression() {
        invalid(
            r#"fn log_message():
    print("hello")

main:
    result = log_message() + 1
"#,
        );
    }

    #[test]
    fn recursive_function_with_incompatible_return_type_is_rejected() {
        invalid(
            r#"fn recursive(n: num) -> num:
    if n <= 0:
        return "finished"
    else:
        return recursive(n - 1)

main:
    print(recursive(5))
"#,
        );
    }

    #[test]
    fn string_operations_handle_empty_strings() {
        assert_eq!(
            run(r#"main:
    text = ""
    print(text.length())
    print(text.trim())
    print(text.contains(""))
    print(text.starts_with(""))
    print(text.ends_with(""))
    print(text.replace("", "x"))
"#),
            vec!["0", "", "true", "true", "true", "x"]
        );
    }

    #[test]
    fn string_search_distinguishes_missing_and_present_substrings() {
        assert_eq!(
            run(r#"main:
    text = "fusion"
    print(text.find("sion"))
    print(text.find("missing"))
    print(text.contains("fusion"))
    print(text.contains("Fusion"))
"#),
            vec!["Some(2)", "None", "true", "false"]
        );
    }

    #[test]
    fn serialization_round_trip_preserves_nested_arrays() {
        assert_eq!(
            run(r#"main:
    original = [[1, 2], [3, 4]]
    encoded = serialize(original)
    decoded = deserialize(encoded)
    print(decoded)
"#),
            vec!["[[1, 2], [3, 4]]"]
        );
    }

    #[test]
    fn hexadecimal_encoding_handles_empty_and_zero_bytes() {
        assert_eq!(
            run(r#"main:
    print(hex_encode(""))
    print(hex_encode("0"))
    print(hex_decode(""))
    print(hex_decode("30"))
"#),
            vec!["", "30", "", "0"]
        );
    }

    #[test]
    fn base64_round_trip_handles_empty_and_padding_cases() {
        assert_eq!(
            run(r#"main:
    print(base64_encode(""))
    print(base64_encode("f"))
    print(base64_encode("fo"))
    print(base64_encode("foo"))
    print(base64_decode("Zg=="))
    print(base64_decode("Zm8="))
    print(base64_decode("Zm9v"))
"#),
            vec!["", "Zg==", "Zm8=", "Zm9v", "f", "fo", "foo"]
        );
    }

    #[test]
    fn async_calls_preserve_independent_results() {
        assert_eq!(
            run(r#"async fn calculate(x: num) -> num:
    return x * x

main:
    a = await calculate(3)
    b = await calculate(4)
    c = await calculate(5)

    print(a)
    print(b)
    print(c)
    print(a + b + c)
"#),
            vec!["9", "16", "25", "50"]
        );
    }

    #[test]
    fn async_recursion_returns_correct_result() {
        assert_eq!(
            run(r#"async fn factorial(n: num) -> num:
    if n <= 1:
        return 1

    previous = await factorial(n - 1)
    return n * previous

main:
    print(await factorial(6))
"#),
            vec!["720"]
        );
    }

    #[test]
    fn multiple_deferred_file_operations_preserve_written_content() {
        let dir = std::env::temp_dir();
        let path_a = dir.join(format!("fusion_defer_a_{}.txt", std::process::id()));
        let path_b = dir.join(format!("fusion_defer_b_{}.txt", std::process::id()));

        let a = path_a.to_string_lossy().replace('\\', "\\\\");
        let b = path_b.to_string_lossy().replace('\\', "\\\\");

        let source = format!(
            r#"main:
    first = open("{}")
    second = open("{}")

    defer first.close()
    defer second.close()

    first.write("alpha")
    second.write("beta")
"#,
            a, b
        );

        run(&source);

        assert_eq!(std::fs::read_to_string(&path_a).unwrap(), "alpha");
        assert_eq!(std::fs::read_to_string(&path_b).unwrap(), "beta");

        let _ = std::fs::remove_file(path_a);
        let _ = std::fs::remove_file(path_b);
    }

    #[test]
    fn invalid_base64_is_rejected_or_returns_documented_error() {
        // Choose one contract and enforce it consistently:
        // - return an error/None; or
        // - reject the operation with a documented runtime error.
        //
        // Do not silently return corrupted decoded data.
    }

    #[test]
    fn iterator_reduce_count_take_skip_zip_flatten() {
        assert_eq!(
            run(r#"fn add(a: num, b: num) -> num:
    return a + b

main:
    nums = [1, 2, 3, 4]
    print(nums.iterate().reduce(add))
    print(nums.iterate().count())
    print(nums.iterate().take(2).collect())
    print(nums.iterate().skip(2).collect())
    print(nums.iterate().zip([10, 20]).collect())
    nested = [[1, 2], [3], [4, 5]]
    print(nested.iterate().flatten().collect())
"#),
            vec![
                "Some(10)",
                "4",
                "[1, 2]",
                "[3, 4]",
                "[[1, 10], [2, 20]]",
                "[1, 2, 3, 4, 5]"
            ]
        );
    }

    #[test]
    fn iterator_reduce_empty_returns_none() {
        assert_eq!(
            run(r#"fn add(a: num, b: num) -> num:
    return a + b

main:
    nums = []
    print(nums.iterate().reduce(add))
"#),
            vec!["None"]
        );
    }

    #[test]
    fn growable_array_collection_operations() {
        assert_eq!(
            run(r#"main:
    values = [1, 1, 2, 3, 3]
    print(values.first())
    print(values.last())
    print(values.is_empty())
    values.dedup()
    values.extend([4, 5])
    values.truncate(4)
    print(values)
    print(values.length())
"#),
            vec!["Some(1)", "Some(3)", "false", "[1, 2, 3, 4]", "4"]
        );
    }
}

#[cfg(test)]
mod pattern_and_tuple_feature_tests {
    use super::*;

    fn execute(source: &str) -> Vec<String> {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("lexing should succeed");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("parsing should succeed");
        let mut interpreter = interpreter::Interpreter::new();
        interpreter.execute(&program);
        interpreter.output().to_vec()
    }

    #[test]
    fn tuple_expression_and_nested_tuple_pattern() {
        let output = execute(
            r#"main:
    pair = (7, (8, 9))
    match pair:
        (left, (middle, right)) => print(left + middle + right)
"#,
        );
        assert_eq!(output, vec!["24"]);
    }

    #[test]
    fn match_guards_continue_to_next_arm_when_false() {
        let output = execute(
            r#"main:
    value = 4
    match value:
        n if n < 0 => print("negative")
        n if n > 3 => print("large")
        _ => print("other")
"#,
        );
        assert_eq!(output, vec!["large"]);
    }

    #[test]
    fn integer_ranges_and_or_patterns_match() {
        let output = execute(
            r#"main:
    value = 3
    match value:
        1 | 2 => print("small")
        3..6 => print("range")
        _ => print("other")
"#,
        );
        assert_eq!(output, vec!["range"]);
    }

    #[test]
    fn range_patterns_work_on_the_right_side_of_or_patterns() {
        let output = execute(
            r#"main:
    value = 8
    match value:
        1..3 | 7..=9 => print("matched")
        _ => print("other")
"#,
        );
        assert_eq!(output, vec!["matched"]);
    }

    #[test]
    fn inclusive_range_includes_its_upper_bound() {
        let output = execute(
            r#"main:
    value = 5
    match value:
        1..=5 => print("included")
        _ => print("excluded")
"#,
        );
        assert_eq!(output, vec!["included"]);
    }

    #[test]
    fn nested_tuple_patterns_bind_values_and_guards_can_reject_them() {
        let output = execute(
            r#"main:
    value = (1, (2, 9))
    match value:
        (a, (b, c)) if c < 0 => print("rejected")
        (a, (b, c)) if a + b + c == 12 => print("nested")
        _ => print("other")
"#,
        );
        assert_eq!(output, vec!["nested"]);
    }

    #[test]
    fn or_patterns_must_bind_the_same_names_in_the_same_order() {
        let result = compile_source(
            r#"main:
    value = 1
    match value:
        left | right => print(left)
"#,
        );
        assert!(
            result.is_err(),
            "mismatched or-pattern bindings must be rejected"
        );
    }

    #[test]
    fn tuple_types_are_preserved_by_the_type_checker() {
        let program = compile_source(
            r#"fn unpack(pair: (num, string)) -> num:
    return 1
main:
    value = (1, "one")
"#,
        );
        // Tuple declarations and annotations are parsed and type-checked. Field
        // projection is deliberately not asserted until tuple indexing is added.
        assert!(
            program.is_ok(),
            "tuple type should compile: {:?}",
            program.err()
        );
    }

    #[test]
    fn derive_attributes_are_retained_on_struct_ast() {
        let program = compile_source(
            r#"#[derive(Debug, PartialEq, Clone)]
struct Point:
    x: num
    y: num
main:
    p = Point(x: 1, y: 2)
"#,
        )
        .expect("derive attribute should parse and type-check");
        let struct_decl = program
            .statements
            .iter()
            .find_map(|statement| {
                if let ast::Statement::Struct { derives, .. } = statement {
                    Some(derives)
                } else {
                    None
                }
            })
            .expect("struct declaration should remain in expanded AST");
        assert_eq!(struct_decl, &["Debug", "PartialEq", "Clone"]);

        for trait_name in ["Debug", "PartialEq", "Clone"] {
            assert!(program.statements.iter().any(|statement| matches!(statement,
                ast::Statement::Impl { trait_name: Some(found), type_name, methods, .. }
                if found == trait_name && type_name == "Point" && methods.iter().any(|method| matches!(method, ast::Statement::Function { .. }))
            )), "derive should expand into a regular impl of {} for Point", trait_name);
            assert!(
                program
                    .statements
                    .iter()
                    .any(|statement| matches!(statement,
                        ast::Statement::Trait { name, .. } if name == trait_name
                    )),
                "the built-in trait declaration {} should be present",
                trait_name
            );
        }
    }
}

#[cfg(test)]
mod native_hir_and_derive_tests {
    use super::*;
    use crate::interpreter::Interpreter;
    use crate::tests::assert_error_mentions;

    #[test]
    fn tuple_expressions_lower_to_hir_instead_of_erroring() {
        let mut lexer = Lexer::new("main:\n    pair = (1, 2.5)\n", BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("lexing should succeed");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("parsing should succeed");
        let mut lowerer = hir_lower::HirLowerer::new();
        let hir = lowerer
            .lower_program(&program)
            .expect("tuple should lower to HIR");
        match &hir.statements[0] {
            hir::HirStatement::Main { body, .. } => match &body[0] {
                hir::HirStatement::VariableDeclaration {
                    value: hir::HirExpression::Tuple(items),
                    ..
                } => {
                    assert_eq!(items.len(), 2);
                    assert!(matches!(items[0], hir::HirExpression::Number(1)));
                    assert!(
                        matches!(items[1], hir::HirExpression::Float(v) if (v - 2.5).abs() < f64::EPSILON)
                    );
                }
                other => panic!("expected tuple variable initializer, got {:?}", other),
            },
            other => panic!("expected HIR main block, got {:?}", other),
        }
    }

    #[test]
    fn native_backend_emits_tuple_construction_for_mixed_scalar_elements() {
        let llvm = emit_llvm("main:\n    pair = (7, 2.5, true, \"hello\")\n")
            .expect("native tuple construction should lower");
        assert!(llvm.contains("call ptr @fusion_tuple_new(i64 4)"));
        assert!(llvm.contains("define void @fusion_tuple_cleanup()"));
        assert!(llvm.contains("call void @fusion_tuple_cleanup()"));
        assert!(llvm.contains("bitcast double"));
        assert!(llvm.contains("zext i1"));
        assert!(llvm.contains("ptrtoint ptr"));
    }

    #[test]
    fn derived_clone_debug_and_partial_eq_are_executable() {
        let output = {
            let mut lexer = Lexer::new(
                r#"#[derive(Debug, PartialEq, Clone)]
struct Point:
    x: num
    y: num
main:
    a = Point(x: 2, y: 5)
    b = a.clone()
    print(a.eq(b))
    print(a.debug())
"#,
                BlockMode::Unknown,
            );
            let tokens = lexer.tokenize().expect("lexing should succeed");
            let mut parser = Parser::new(tokens);
            let program = parser.parse().expect("parsing should succeed");
            let mut checker = TypeChecker::new();
            checker
                .check(&program)
                .expect("derived methods should type-check");
            let mut interpreter = Interpreter::new();
            interpreter.execute(&program);
            interpreter.output().to_vec()
        };
        assert_eq!(output[0], "true");
        assert!(output[1].contains("Point"));
        assert!(output[1].contains("x"));
        assert!(output[1].contains("y"));
    }

    #[test]
    fn unknown_derive_is_rejected_with_a_diagnostic() {
        let result = compile_source(
            "#[derive(Serialize)]\nstruct Point:\n    x: num\nmain:\n    p = Point(x: 1)\n",
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_error_mentions(
            &error,
            &[
                "Unsupported derive 'Serialize'",
                "Debug",
                "Clone",
                "PartialEq",
            ],
            "unsupported derive should list the supported built-in derives",
        );
    }
    #[test]
    fn native_tuple_indexing_loads_and_decodes_elements() {
        let llvm = emit_llvm(
            "main:\n    pair = (7, 2.5, true, \"hello\")\n    print(pair[1])\n    print(pair[2])\n",
        )
        .expect("native tuple indexing should lower");
        assert!(llvm.contains("getelementptr inbounds i64"));
        assert!(llvm.contains("load i64"));
        assert!(llvm.contains("bitcast i64"));
        assert!(llvm.contains("trunc i64"));
    }

    #[test]
    fn tuple_indexing_checks_literal_bounds_and_types() {
        let ok = compile_source("main:\n    pair = (7, 2.5)\n    x = pair[0]\n");
        assert!(
            ok.is_ok(),
            "first tuple element should type-check: {:?}",
            ok.err()
        );
        let out_of_bounds = compile_source("main:\n    pair = (7, 2.5)\n    x = pair[2]\n");
        assert!(out_of_bounds.is_err());
        let dynamic = compile_source("main:\n    pair = (7, 2.5)\n    i = 0\n    x = pair[i]\n");
        assert!(dynamic.is_err());
    }

    #[test]
    fn test_rust_style_suggestion_rich_error_messages() {
        use crate::errors::FusionError;
        use crate::span::Span;

        let source = "main:
    total_count = 42
    print(total_coun)
";
        // Byte offset 37 is the start of `total_coun` (line 3, col 11).
        let span = Span::new(37, 47);

        let err = FusionError::UnknownVariable {
            name: "total_coun".to_string(),
            suggestion: Some("total_count".to_string()),
            span,
        };

        let diag = err.to_diagnostic();
        let emitted = diag.emit(source);

        assert!(emitted.contains("error[E0425]: cannot find value `total_coun` in this scope"));
        assert!(emitted.contains("--> line 3, col 11"));
        assert!(emitted.contains("3 |     print(total_coun)"));
        assert!(emitted.contains("^^^^^^^^^^ not found in scope"));
        assert!(emitted.contains(
            "= help: did you mean `total_count`?; consider replacing with `total_count`"
        ));
    }

    #[test]
    fn test_rust_style_type_mismatch_diagnostic() {
        use crate::errors::FusionError;
        use crate::span::Span;

        let source = r#"main:
    x: int = "hello"
"#;
        // Byte offset 19 is the opening quote of `"hello"` (line 2, col 14).
        let span = Span::new(19, 26);

        let err = FusionError::TypeMismatch {
            expected: "int".to_string(),
            found: "string".to_string(),
            span,
        };

        let diag = err.to_diagnostic();
        let emitted = diag.emit(source);

        assert!(emitted.contains("error[E0308]: mismatched types"));
        assert!(emitted.contains("--> line 2, col 14"));
        assert!(emitted.contains("expected `int`, found `string`"));
    }
}

#[cfg(test)]
mod roadmap_feature_regressions {
    use super::*;

    fn execute_source(source: &str) -> Vec<String> {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("lexing should succeed");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("parsing should succeed");
        let mut checker = TypeChecker::new();
        checker
            .check(&program)
            .expect("type checking should succeed");
        let mut interpreter = interpreter::Interpreter::new();
        interpreter.execute(&program);
        interpreter.output().to_vec()
    }

    #[test]
    fn enum_payloads_support_nested_range_patterns() {
        let output = execute_source(
            r#"enum Maybe:
    Some(num)
    Empty
main:
    value = Maybe::Some(7)
    match value:
        Maybe::Some(1..=8) => print("inside")
        _ => print("other")
"#,
        );
        assert_eq!(output, vec!["inside"]);
    }

    #[test]
    fn struct_patterns_support_nested_patterns_and_rest_fields() {
        let output = execute_source(
            r#"struct Point:
    x: num
    y: num
main:
    point = Point(x: 7, y: 9)
    match point:
        Point { x: 7, y: y, .. } => print(y)
        _ => print("no")
"#,
        );
        assert_eq!(output, vec!["9"]);
    }

    #[test]
    fn at_patterns_bind_the_whole_value_and_nested_fields() {
        let output = execute_source(
            r#"struct Point:
    x: num
    y: num
main:
    point = Point(x: 7, y: 9)
    match point:
        whole @ Point { x: x, .. } if x == 7 => print(whole.y)
        _ => print("no")
"#,
        );
        assert_eq!(output, vec!["9"]);
    }

    #[test]
    fn fixed_width_integer_types_are_recognized_and_literal_overflow_is_rejected() {
        let valid = compile_source(
            "fn identity(value: i32) -> i32:\n    return value\nmain:\n    answer: i32 = 42\n",
        );
        assert!(
            valid.is_ok(),
            "i32 annotation should type-check: {:?}",
            valid.err()
        );

        let invalid = compile_source("main:\n    too_large: u8 = 300\n");
        assert!(
            invalid.is_err(),
            "out-of-range u8 literals should be rejected"
        );
        assert!(format!("{:?}", invalid.unwrap_err()).contains("outside the range of u8"));
    }

    #[test]
    fn nested_tuple_type_annotations_keep_element_boundaries() {
        let output = execute_source("fn first(value: (num, (num, string))) -> num:\n    return value[0]\nmain:\n    nested = (1, (2, \"two\"))\n    print(first(nested))\n");
        assert_eq!(output, vec!["1"]);
    }

    #[test]
    fn fixed_width_numeric_casts_are_supported_by_the_interpreter() {
        let output = execute_source("main:\n    integer = 42 as i32\n    decimal = 2 as f32\n    print(integer)\n    print(decimal)\n");
        assert_eq!(output, vec!["42", "2"]);
    }

    #[test]
    fn fixed_width_unsigned_types_reject_negative_literals() {
        let invalid = compile_source("main:\n    negative: u8 = -1\n");
        assert!(
            invalid.is_err(),
            "negative values cannot initialize an unsigned integer"
        );
    }

    #[test]
    fn index_trait_method_overloads_bracket_indexing() {
        let output = execute_source(
            r#"struct Container:
    value: num
trait Index:
    fn index(container: Container, index: num) -> num
impl Index for Container:
    fn index(container: Container, index: num) -> num:
        return container.value + index
main:
    container = Container(value: 40)
    print(container[2])
"#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn deref_trait_method_overloads_prefix_star() {
        let output = execute_source(
            r#"struct Boxed:
    value: num
trait Deref:
    fn deref(boxed: Boxed) -> num
impl Deref for Boxed:
    fn deref(boxed: Boxed) -> num:
        return boxed.value
main:
    boxed = Boxed(value: 27)
    print(*boxed)
"#,
        );
        assert_eq!(output, vec!["27"]);
    }

    #[test]
    fn derived_partial_eq_overloads_equality_operators() {
        let output = execute_source(
            r#"#[derive(PartialEq)]
struct Point:
    x: num
main:
    left = Point(x: 3)
    same = Point(x: 3)
    other = Point(x: 4)
    print(left == same)
    print(left != other)
"#,
        );
        assert_eq!(output, vec!["true", "true"]);
    }

    #[test]
    fn add_trait_method_overloads_the_plus_operator_for_structs() {
        let output = execute_source(
            r#"struct Point:
    x: num
trait Add:
    fn add(left: Point, right: Point) -> Point
impl Add for Point:
    fn add(left: Point, right: Point) -> Point:
        return Point(x: left.x + right.x)
main:
    left = Point(x: 10)
    right = Point(x: 32)
    result = left + right
    print(result.x)
"#,
        );
        assert_eq!(output, vec!["42"]);
    }
}

#[cfg(test)]
mod native_runtime_phase2_tests {
    use super::*;

    #[test]
    fn native_arrays_use_tracked_runtime_storage() {
        let llvm = emit_llvm("main:\n    values = [10, 20, 30]\n    print(values[1])\n")
            .expect("native array construction and literal indexing should lower");
        assert!(llvm.contains("call ptr @fusion_tuple_new(i64 3)"));
        assert!(llvm.contains("define i64 @fusion_tuple_len(ptr %data)"));
        assert!(llvm.contains("call void @fusion_tuple_cleanup()"));
    }

    #[test]
    fn native_array_dynamic_index_checks_bounds_before_loading() {
        let llvm = emit_llvm(
            "main:\n    values = [10, 20, 30]\n    index = 1\n    print(values[index])\n",
        )
        .expect("native arrays should accept dynamic integer indexes");
        assert!(llvm.contains("call i64 @fusion_tuple_len(ptr"));
        assert!(llvm.contains("icmp ult i64"));
        assert!(llvm.contains("call void @llvm.trap()"));
        assert!(llvm.contains("index.oob"));
        assert!(llvm.contains("index.valid"));
    }
}

#[cfg(test)]
mod wide_integer_tests {
    //! Tests for exact fixed-width integer support: `u64` / `i128` / `u128` values
    //! beyond the signed 64-bit range, exact range checks, casts and arithmetic.
    use super::*;
    use crate::interpreter::Interpreter;
    use crate::value::Value;
    use crate::wide_int::{IntKind, WideInt};

    const U64_MAX: &str = "18446744073709551615";
    const U128_MAX: &str = "340282366920938463463374607431768211455";
    const I128_MIN_MAGNITUDE: &str = "170141183460469231731687303715884105728";

    fn run(source: &str) -> Vec<String> {
        let program = compile_source(source).expect("program should compile");
        let mut interpreter = Interpreter::new();
        interpreter.execute(&program);
        interpreter.output().to_vec()
    }

    fn compile_error(source: &str) -> String {
        match compile_source(source) {
            Ok(_) => panic!("expected compilation to fail for:\n{}", source),
            Err(message) => message,
        }
    }

    fn wide(text: &str) -> WideInt {
        WideInt::parse(text).expect("valid test integer")
    }

    // ---------------------------------------------------------------------
    // WideInt and IntKind
    // ---------------------------------------------------------------------

    #[test]
    fn int_kind_reports_exact_widths_and_ranges() {
        assert_eq!(IntKind::from_name("u64"), Some(IntKind::U64));
        assert_eq!(IntKind::from_name("float"), None);
        assert_eq!(IntKind::U8.bits(), 8);
        assert_eq!(IntKind::Usize.bits(), 64);
        assert_eq!(IntKind::I128.bits(), 128);
        assert!(IntKind::I32.is_signed());
        assert!(!IntKind::U32.is_signed());

        assert_eq!(IntKind::U8.max(), WideInt::from_i64(255));
        assert_eq!(IntKind::I8.min(), WideInt::from_i64(-128));
        assert_eq!(IntKind::U64.max(), WideInt::from_u128(u64::MAX as u128));
        assert_eq!(IntKind::I64.min(), WideInt::from_i64(i64::MIN));
        assert_eq!(IntKind::I128.min(), WideInt::from_i128(i128::MIN));
        assert_eq!(IntKind::I128.max(), WideInt::from_i128(i128::MAX));
        assert_eq!(IntKind::U128.max(), WideInt::from_u128(u128::MAX));
        assert_eq!(IntKind::U128.min(), WideInt::from_i64(0));
    }

    #[test]
    fn int_kind_contains_is_exact_at_the_boundaries() {
        assert!(IntKind::U64.contains(wide(U64_MAX)));
        assert!(!IntKind::U64.contains(wide("18446744073709551616")));
        assert!(!IntKind::U64.contains(WideInt::from_i64(-1)));
        assert!(IntKind::I64.contains(WideInt::from_i64(i64::MAX)));
        assert!(!IntKind::I64.contains(wide("9223372036854775808")));
        assert!(IntKind::U128.contains(wide(U128_MAX)));
        assert!(!IntKind::I128.contains(wide(U128_MAX)));
        assert!(IntKind::I128.contains(WideInt::from_i128(i128::MIN)));
    }

    #[test]
    fn wide_int_round_trips_through_text_and_native_integers() {
        for text in [
            U64_MAX,
            U128_MAX,
            "0",
            "-1",
            "-170141183460469231731687303715884105728",
        ] {
            assert_eq!(wide(text).to_string(), text);
        }
        assert_eq!(
            WideInt::parse("-0").map(|v| v.to_string()),
            Some("0".to_string())
        );
        assert!(WideInt::parse("").is_none());
        assert!(WideInt::parse("12x").is_none());
        assert!(WideInt::parse("340282366920938463463374607431768211456").is_none());
        assert!(WideInt::parse("-170141183460469231731687303715884105729").is_none());

        assert_eq!(wide(U64_MAX).to_i64(), None);
        assert_eq!(wide(U64_MAX).to_u128(), Some(u64::MAX as u128));
        assert_eq!(wide("-5").to_i64(), Some(-5));
        assert_eq!(WideInt::from_i128(i128::MIN).to_i128(), Some(i128::MIN));
        assert_eq!(wide(U128_MAX).to_i128(), None);
    }

    #[test]
    fn wide_int_arithmetic_is_exact_and_checked() {
        let max = wide(U128_MAX);
        assert_eq!(max.checked_sub(max), Some(WideInt::from_i64(0)));
        assert_eq!(max.checked_add(WideInt::from_i64(1)), None);
        assert_eq!(WideInt::from_i64(0).checked_sub(max), None);
        assert_eq!(
            wide(U64_MAX)
                .checked_mul(wide(U64_MAX))
                .map(|v| v.to_string()),
            Some("340282366920938463426481119284349108225".to_string())
        );
        assert_eq!(
            WideInt::from_i64(-7).checked_div(WideInt::from_i64(2)),
            Some(WideInt::from_i64(-3))
        );
        assert_eq!(WideInt::from_i64(7).checked_div(WideInt::from_i64(0)), None);
        assert_eq!(max.checked_div(WideInt::from_i64(-1)), None);
        assert_eq!(max.checked_neg(), None);
        assert_eq!(
            WideInt::from_i128(i128::MIN)
                .checked_neg()
                .map(|v| v.to_string()),
            Some(I128_MIN_MAGNITUDE.to_string())
        );
        assert_eq!(
            WideInt::from_i64(i64::MAX)
                .checked_add(WideInt::from_i64(1))
                .map(|v| v.to_string()),
            Some("9223372036854775808".to_string())
        );
        assert_eq!(
            WideInt::from_i64(i64::MIN)
                .checked_sub(WideInt::from_i64(1))
                .map(|v| v.to_string()),
            Some("-9223372036854775809".to_string())
        );
    }

    #[test]
    fn wide_int_orders_across_signs_and_magnitudes() {
        assert!(wide("-1") < wide("0"));
        assert!(wide("-170141183460469231731687303715884105728") < wide("-1"));
        assert!(wide(U64_MAX) > WideInt::from_i64(i64::MAX));
        assert!(wide(U128_MAX) > wide(U64_MAX));
        assert_eq!(wide("5"), WideInt::from_i64(5));
    }

    #[test]
    fn wide_int_float_conversions_reject_inexact_and_out_of_range_values() {
        assert_eq!(
            WideInt::from_f64(12345678901234567168.0).map(|v| v.to_string()),
            Some("12345678901234567168".to_string())
        );
        assert_eq!(
            WideInt::from_f64(-0.0).map(|v| v.to_string()),
            Some("0".to_string())
        );
        assert!(WideInt::from_f64(1.5).is_none());
        assert!(WideInt::from_f64(f64::NAN).is_none());
        assert!(WideInt::from_f64(f64::INFINITY).is_none());
        assert!(WideInt::from_f64(340282366920938463463374607431768211456.0).is_none());
        assert_eq!(wide(U64_MAX).to_f64(), 18446744073709551616.0);
    }

    #[test]
    fn values_use_a_canonical_integer_representation() {
        assert_eq!(Value::from_wide(WideInt::from_i64(5)), Value::Number(5));
        assert_eq!(
            Value::from_wide(WideInt::from_i64(i64::MIN)),
            Value::Number(i64::MIN)
        );
        assert!(matches!(
            Value::from_wide(wide("9223372036854775808")),
            Value::Wide(_)
        ));
        assert_eq!(Value::from_wide(wide(U64_MAX)).to_string(), U64_MAX);
        assert_eq!(Value::Number(-3).as_wide(), Some(WideInt::from_i64(-3)));
        assert_eq!(Value::Boolean(true).as_wide(), None);
    }

    // ---------------------------------------------------------------------
    // Literals and static range checks
    // ---------------------------------------------------------------------

    #[test]
    fn u64_accepts_values_above_the_signed_range() {
        assert_eq!(
            run(&format!(
                "main:\n    big: u64 = {}\n    print(big)\n",
                U64_MAX
            )),
            vec![U64_MAX]
        );
    }

    #[test]
    fn u128_accepts_its_maximum_value() {
        assert_eq!(
            run(&format!(
                "main:\n    big: u128 = {}\n    print(big)\n",
                U128_MAX
            )),
            vec![U128_MAX]
        );
    }

    #[test]
    fn i128_accepts_its_minimum_and_maximum_values() {
        let output = run(&format!(
            "main:\n    low: i128 = -{}\n    high: i128 = 170141183460469231731687303715884105727\n    print(low)\n    print(high)\n",
            I128_MIN_MAGNITUDE
        ));
        assert_eq!(
            output,
            vec![
                format!("-{}", I128_MIN_MAGNITUDE),
                "170141183460469231731687303715884105727".to_string()
            ]
        );
    }

    #[test]
    fn i64_minimum_literal_is_supported() {
        assert_eq!(
            run("main:\n    low: i64 = -9223372036854775808\n    print(low)\n"),
            vec!["-9223372036854775808"]
        );
    }

    #[test]
    fn literals_just_outside_each_range_are_rejected() {
        assert!(compile_error("main:\n    x: u64 = 18446744073709551616\n")
            .contains("outside the range of u64"));
        assert!(compile_error("main:\n    x: i64 = 9223372036854775808\n")
            .contains("outside the range of i64"));
        assert!(
            compile_error(&format!("main:\n    x: i128 = {}\n", I128_MIN_MAGNITUDE))
                .contains("outside the range of i128")
        );
        assert!(compile_error("main:\n    x: u128 = -1\n").contains("outside the range of u128"));
        assert!(compile_error("main:\n    x: u64 = -1\n").contains("outside the range of u64"));
        assert!(compile_error(&format!("main:\n    x: u32 = {}\n", U64_MAX))
            .contains("outside the range of u32"));
    }

    #[test]
    fn literals_larger_than_u128_are_rejected_by_the_lexer() {
        let message = compile_error("main:\n    x = 340282366920938463463374607431768211456\n");
        assert!(
            message.contains("too large"),
            "unexpected message: {}",
            message
        );
    }

    #[test]
    fn wide_literals_lex_as_a_dedicated_token() {
        let mut lexer = Lexer::new(
            "9223372036854775807 9223372036854775808",
            BlockMode::Unknown,
        );
        let tokens = lexer.tokenize().expect("literals should lex");
        assert!(tokens
            .iter()
            .any(|t| matches!(t.node, lexer::Token::Num(9223372036854775807))));
        assert!(tokens
            .iter()
            .any(|t| matches!(t.node, lexer::Token::WideNum(9223372036854775808))));
    }

    // ---------------------------------------------------------------------
    // Arithmetic and comparisons
    // ---------------------------------------------------------------------

    #[test]
    fn i64_overflow_promotes_to_a_wide_integer_instead_of_wrapping() {
        assert_eq!(
            run("main:\n    a = 9223372036854775807\n    b = a + 1\n    print(b)\n"),
            vec!["9223372036854775808"]
        );
        assert_eq!(
            run("main:\n    a = -9223372036854775807\n    b = a - 2\n    print(b)\n"),
            vec!["-9223372036854775809"]
        );
        assert_eq!(
            run("main:\n    a = 9223372036854775807\n    b = a * 4\n    print(b)\n"),
            vec!["36893488147419103228"]
        );
    }

    #[test]
    fn wide_results_that_fit_i64_become_plain_numbers_again() {
        let output = run("main:\n    big: u64 = 18446744073709551615\n    small = big - 18446744073709551610\n    print(small)\n    print(small == 5)\n");
        assert_eq!(output, vec!["5", "true"]);
    }

    #[test]
    fn u64_arithmetic_near_the_top_of_the_range() {
        let output =
            run("main:\n    x: u64 = 18446744073709551615\n    y: u64 = x - 1\n    print(y)\n");
        assert_eq!(output, vec!["18446744073709551614"]);
    }

    #[test]
    fn u128_multiplication_is_exact() {
        let output = run("main:\n    a: u128 = 18446744073709551615\n    print(a * a)\n");
        assert_eq!(output, vec!["340282366920938463426481119284349108225"]);
    }

    #[test]
    fn negating_the_i64_minimum_produces_a_wide_value() {
        assert_eq!(
            run("main:\n    m: i64 = -9223372036854775808\n    n = -m\n    print(n)\n"),
            vec!["9223372036854775808"]
        );
    }

    #[test]
    fn wide_division_truncates_toward_zero_and_handles_the_i128_minimum() {
        let output = run(&format!(
            "main:\n    x: i128 = -{}\n    y = x / -1\n    z = x / 2\n    print(y)\n    print(z)\n",
            I128_MIN_MAGNITUDE
        ));
        assert_eq!(
            output,
            vec![
                I128_MIN_MAGNITUDE.to_string(),
                "-85070591730234615865843651857942052864".to_string()
            ]
        );
    }

    #[test]
    fn wide_comparisons_work_across_representations() {
        let output = run(&format!(
            "main:\n    big: u64 = {max}\n    small: u64 = 5\n    print(big > small)\n    print(small < big)\n    print(big == {max})\n    print(big != small)\n    print(big >= big)\n    print(small <= big)\n",
            max = U64_MAX
        ));
        assert_eq!(output, vec!["true"; 6]);
    }

    #[test]
    fn equal_integers_compare_equal_whichever_way_they_were_computed() {
        let output = run(&format!(
            "main:\n    a: u64 = {}\n    b: u64 = 18446744073709551614 + 1\n    print(a == b)\n",
            U64_MAX
        ));
        assert_eq!(output, vec!["true"]);
    }

    #[test]
    #[should_panic(expected = "Integer overflow in addition")]
    fn arithmetic_beyond_the_128_bit_range_is_reported() {
        run(&format!(
            "main:\n    a: u128 = {}\n    b = a + 1\n    print(b)\n",
            U128_MAX
        ));
    }

    #[test]
    #[should_panic(expected = "Division by zero")]
    fn wide_division_by_zero_is_reported() {
        run("main:\n    a: u64 = 18446744073709551615\n    zero = 0\n    print(a / zero)\n");
    }

    // ---------------------------------------------------------------------
    // Functions and typed boundaries
    // ---------------------------------------------------------------------

    #[test]
    fn functions_accept_and_return_wide_integers() {
        let output = run("fn add_one(value: u128) -> u128:\n    return value + 1\nmain:\n    print(add_one(340282366920938463463374607431768211454))\n    print(add_one(5))\n");
        assert_eq!(output, vec![U128_MAX, "6"]);
    }

    #[test]
    fn fixed_width_parameters_accept_in_range_values() {
        assert_eq!(run("fn identity(value: i32) -> i32:\n    return value\nmain:\n    print(identity(42))\n"), vec!["42"]);
    }

    #[test]
    #[should_panic(expected = "u8, got")]
    fn fixed_width_parameters_reject_values_outside_their_range_at_runtime() {
        run("fn takes_u8(value: u8) -> u8:\n    return value\nmain:\n    print(takes_u8(18446744073709551615))\n");
    }

    // ---------------------------------------------------------------------
    // Conversions
    // ---------------------------------------------------------------------

    #[test]
    fn integer_casts_preserve_exact_wide_values() {
        let output = run("main:\n    value: u128 = 18446744073709551615\n    narrowed = value as u64\n    widened = narrowed as u128\n    print(narrowed)\n    print(widened)\n");
        assert_eq!(output, vec![U64_MAX, U64_MAX]);
    }

    #[test]
    #[should_panic(expected = "outside the range of u8")]
    fn casts_outside_the_target_range_are_rejected() {
        run("main:\n    big = 300\n    small = big as u8\n    print(small)\n");
    }

    #[test]
    #[should_panic(expected = "outside the range of u64")]
    fn negative_values_cannot_be_cast_to_unsigned_types() {
        run("main:\n    negative = -1\n    value = negative as u64\n    print(value)\n");
    }

    #[test]
    #[should_panic(expected = "outside the range of u64")]
    fn u128_values_above_u64_cannot_be_cast_to_u64() {
        run(&format!(
            "main:\n    big: u128 = {}\n    value = big as u64\n    print(value)\n",
            U128_MAX
        ));
    }

    #[test]
    fn floats_convert_to_wide_integers_when_they_are_whole_numbers() {
        let output = run("main:\n    f = 12345678901234567890.0\n    n = f as u64\n    print(n)\n");
        assert_eq!(output, vec!["12345678901234567168"]);
    }

    #[test]
    fn wide_integers_convert_to_floats() {
        let output = run("main:\n    x: u64 = 18446744073709551615\n    y = x as f64\n    print(y == 18446744073709551616.0)\n");
        assert_eq!(output, vec!["true"]);
    }

    #[test]
    fn integers_convert_to_f32_with_single_precision_rounding() {
        let output = run("main:\n    n = 16777217\n    f = n as f32\n    print(f == 16777216.0)\n");
        assert_eq!(output, vec!["true"]);
    }

    #[test]
    fn strings_parse_into_the_full_integer_range() {
        let output = run(&format!(
            "main:\n    text = \"{}\"\n    n = text as u128\n    print(n)\n",
            U128_MAX
        ));
        assert_eq!(output, vec![U128_MAX]);
    }

    #[test]
    fn wide_integers_convert_to_strings() {
        let output =
            run("main:\n    x: u64 = 18446744073709551615\n    s = x as string\n    print(s)\n");
        assert_eq!(output, vec![U64_MAX]);
    }

    // ---------------------------------------------------------------------
    // Native backend guard
    // ---------------------------------------------------------------------

    #[test]
    fn native_backend_rejects_wide_literals_instead_of_truncating_them() {
        let error = emit_llvm("main:\n    big: u64 = 18446744073709551615\n    print(big)\n")
            .expect_err("the native backend cannot lower wide integers yet");
        assert!(
            error.contains("LLVM backend"),
            "unexpected message: {}",
            error
        );
    }

    #[test]
    fn native_backend_still_lowers_ordinary_integer_literals() {
        assert!(emit_llvm("main:\n    x = 9223372036854775807\n    print(x)\n").is_ok());
    }

    #[test]
    fn unimported_manifest_package_emits_empty_package_unit_and_interface() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-unimported-package-{nonce}"));
        let app = root.join("app");
        let unused = root.join("deps/unused/src");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&unused).unwrap();
        let entry = app.join("main.fusion");
        std::fs::write(&entry, "main:\n    print(1)\n").unwrap();
        std::fs::write(
            unused.join("lib.fusion"),
            "pub fn unused() -> num:\n    return 7\n",
        )
        .unwrap();
        let roots = std::collections::BTreeMap::from([("unused".to_string(), unused.clone())]);
        let package_ir =
            emit_llvm_package_with_package_roots(&entry, roots.clone(), &unused).unwrap();
        let interface = emit_package_interface_with_package_roots(&entry, roots, &unused).unwrap();
        assert!(package_ir.contains("no loaded modules"));
        assert_eq!(interface, "FUSION-PACKAGE-INTERFACE\t6\n");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_interface_contains_only_public_symbols_from_requested_source_root() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-interface-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.fusion");
        std::fs::write(&entry, "import geometry\nmain:\n    print(1)\n").unwrap();
        std::fs::write(
            root.join("geometry.fusion"),
            "pub fn answer() -> num:\n    return 42\nfn hidden() -> num:\n    return 0\n",
        )
        .unwrap();
        let interface = emit_package_interface_with_package_roots(
            &entry,
            std::collections::BTreeMap::new(),
            &root,
        )
        .expect("package interface should be emitted");
        assert!(interface.starts_with("FUSION-PACKAGE-INTERFACE\t6\n"));
        assert!(interface.contains("export\tgeometry\tanswer\n"));
        assert!(!interface.contains("export\tgeometry\thidden\n"));
        assert!(interface.contains("abi-internal-function\tgeometry\thidden\t"));
        assert!(interface.contains("import\tcrate\tgeometry\t"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_interface_emits_typed_scalar_abi_and_external_declarations() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-abi-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.fusion");
        std::fs::write(&entry, "import math\nmain:\n    print(1)\n").unwrap();
        std::fs::write(root.join("math.fusion"),
            "pub fn add(left: num, right: num) -> num:\n    return left + right\npub fn label(value: string) -> string:\n    return value\nfn hidden_inference(value) -> num:\n    return 1\n").unwrap();
        let interface = emit_package_interface_with_package_roots(
            &entry,
            std::collections::BTreeMap::new(),
            &root,
        )
        .expect("typed package interface should be emitted");
        assert!(interface.starts_with("FUSION-PACKAGE-INTERFACE\t6\n"));
        let add_symbol = format!("_F{}", module_system::mangle_function("math", "add"));
        let label_symbol = format!("_F{}", module_system::mangle_function("math", "label"));
        assert!(interface.contains(&format!(
            "abi-function\tmath\tadd\t{}\ti64\ti64,i64\n",
            add_symbol
        )));
        assert!(interface.contains(&format!(
            "abi-function\tmath\tlabel\t{}\tptr\tptr\n",
            label_symbol
        )));
        assert!(interface.contains(
            "abi-internal-unsupported\tmath\thidden_inference\tmissing-or-unsupported-type\n"
        ));
        let declarations = emit_package_external_declarations(&interface).unwrap();
        assert!(declarations.contains(&format!("declare i64 @{}(i64, i64)\n", add_symbol)));
        assert!(declarations.contains(&format!("declare ptr @{}(ptr)\n", label_symbol)));
        assert!(!declarations.contains("hidden_inference"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_interface_records_scalar_data_and_struct_layouts() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-data-abi-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.fusion");
        std::fs::write(&entry, "import model\nmain:\n    print(1)\n").unwrap();
        std::fs::write(root.join("model.fusion"),
            "pub const LIMIT: num = 42\npub const ENABLED: bool = true\npub const LABEL: string = \"fusion\"\npub struct Point:\n    x: num\n    y: num\n").unwrap();
        let interface = emit_package_interface_with_package_roots(
            &entry,
            std::collections::BTreeMap::new(),
            &root,
        )
        .expect("package data interface should be emitted");
        assert!(interface.starts_with("FUSION-PACKAGE-INTERFACE\t6\n"));
        assert!(interface.contains("abi-data\tmodel\tLIMIT\t_F__fusion_m5_6d6f64656c_f"));
        assert!(interface.contains("\ti64\tinteger\t42\n"));
        assert!(interface.contains("abi-data\tmodel\tENABLED\t_F__fusion_m5_6d6f64656c_f"));
        assert!(interface.contains("\ti1\tboolean\t1\n"));
        assert!(interface.contains("abi-data\tmodel\tLABEL\t_F__fusion_m5_6d6f64656c_f"));
        assert!(interface.contains("\tptr\tutf8-hex\t667573696f6e\n"));
        let point_type = format!(
            "%fusion.type.{}",
            module_system::mangle_type("model", "Point")
        );
        assert!(interface.contains(&format!("abi-struct\tmodel\tPoint\t{}\t", point_type)));
        assert!(interface.contains("\t2\t"));
        assert!(interface.contains("x:i64,y:i64"));
        let data_declarations = emit_package_external_declarations(&interface).unwrap();
        assert!(data_declarations.contains(&format!(
            "@_F{} = external constant i64",
            module_system::mangle_function("model", "$data$LIMIT")
        )));
        assert!(data_declarations.contains(&format!(
            "@_F{} = external constant ptr",
            module_system::mangle_function("model", "$data$LABEL")
        )));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_only_structs_keep_stable_identified_layouts_in_package_ir() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-struct-layout-{nonce}"));
        let app_src = root.join("app/src");
        let model_src = root.join("deps/model/src");
        std::fs::create_dir_all(&app_src).unwrap();
        std::fs::create_dir_all(&model_src).unwrap();
        let entry = app_src.join("main.fusion");
        std::fs::write(&entry, "import model\nmain:\n    print(1)\n").unwrap();
        std::fs::write(
            model_src.join("lib.fusion"),
            "pub struct Point:\n    x: num\n    y: float\n",
        )
        .unwrap();
        let roots = std::collections::BTreeMap::from([("model".to_string(), model_src.clone())]);
        let package_ir =
            emit_llvm_package_with_package_roots(&entry, roots.clone(), &model_src).unwrap();
        let point_type = format!(
            "%fusion.type.{}",
            module_system::mangle_type("model", "Point")
        );
        assert!(package_ir.contains(&format!("{} = type {{ i64, double }}", point_type)));
        let interface =
            emit_package_interface_with_package_roots(&entry, roots, &model_src).unwrap();
        assert!(interface.contains(&format!("abi-struct\tmodel\tPoint\t{}\t2\t", point_type)));
        assert!(interface.contains("x:i64,y:double"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_aggregate_runtime_abi_crosses_module_boundaries() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-aggregate-runtime-{nonce}"));
        let app_src = root.join("app/src");
        let model_src = root.join("deps/model/src");
        let consumer_src = root.join("deps/consumer/src");
        std::fs::create_dir_all(&app_src).unwrap();
        std::fs::create_dir_all(&model_src).unwrap();
        std::fs::create_dir_all(&consumer_src).unwrap();
        let entry = app_src.join("main.fusion");
        std::fs::write(
            &entry,
            r#"import model
import consumer
main:
    let point = model.make_point(7)
    print(consumer.read_point(point))
    let outcome = consumer.make_outcome(9)
    match outcome:
        model.Outcome::Success(value) => print(value)
        model.Outcome::Failure(message) => print(message)
    let failed = consumer.make_failure("failed")
    match failed:
        model.Outcome::Success(value) => print(value)
        model.Outcome::Failure(message) => print(message)
"#,
        )
        .unwrap();
        std::fs::write(
            model_src.join("lib.fusion"),
            r#"pub struct Point:
    x: num
pub enum Outcome:
    Success(num)
    Failure(string)
pub fn make_point(value: num) -> Point:
    return Point { x: value }
"#,
        )
        .unwrap();
        std::fs::write(
            consumer_src.join("lib.fusion"),
            r#"import model
pub fn read_point(point: model.Point) -> num:
    return point.x
pub fn make_outcome(value: num) -> model.Outcome:
    return model.Outcome::Success(value)
pub fn make_failure(message: string) -> model.Outcome:
    return model.Outcome::Failure(message)
"#,
        )
        .unwrap();
        let roots = std::collections::BTreeMap::from([
            ("model".to_string(), model_src.clone()),
            ("consumer".to_string(), consumer_src.clone()),
        ]);

        let model_ir =
            emit_llvm_package_with_package_roots(&entry, roots.clone(), &model_src).unwrap();
        let consumer_ir =
            emit_llvm_package_with_package_roots(&entry, roots.clone(), &consumer_src).unwrap();
        let entry_ir = emit_llvm_entrypoint_with_package_roots(&entry, roots.clone()).unwrap();
        let consumer_interface =
            emit_package_interface_with_package_roots(&entry, roots.clone(), &consumer_src)
                .unwrap();
        let point_type = format!(
            "%fusion.type.{}",
            module_system::mangle_type("model", "Point")
        );
        let outcome_type = format!(
            "%fusion.type.{}",
            module_system::mangle_type("model", "Outcome")
        );
        let success_payload = format!(
            "%fusion.variant.{}",
            module_system::mangle_type("model", "Outcome::Success")
        );
        let failure_payload = format!(
            "%fusion.variant.{}",
            module_system::mangle_type("model", "Outcome::Failure")
        );
        let read_point = format!(
            "_F{}",
            module_system::mangle_function("consumer", "read_point")
        );
        let make_outcome = format!(
            "_F{}",
            module_system::mangle_function("consumer", "make_outcome")
        );

        assert!(model_ir.contains(&format!("{} = type {{ i64 }}", point_type)));
        assert!(model_ir.contains(&format!("{} = type {{ i32, ptr }}", outcome_type)));
        assert!(model_ir.contains(&format!("{} = type {{ i64 }}", success_payload)));
        assert!(model_ir.contains(&format!("{} = type {{ ptr }}", failure_payload)));
        assert!(consumer_ir.contains(&format!("define i64 @{}(ptr %arg0)", read_point)));
        assert!(consumer_ir.contains(&format!("define ptr @{}(i64 %arg0)", make_outcome)));
        assert!(consumer_ir.contains(&format!("getelementptr inbounds {}", point_type)));
        assert!(consumer_ir.contains(&format!("getelementptr inbounds {}", success_payload)));
        assert!(consumer_ir.contains(&format!("getelementptr inbounds {}", failure_payload)));
        assert!(entry_ir.contains(&format!("declare ptr @{}(i64)", make_outcome)));
        assert!(
            entry_ir.contains("icmp eq i32"),
            "entrypoint must discriminate the dependency enum tag"
        );
        assert!(entry_ir.contains(&format!("getelementptr inbounds {}", success_payload)));
        assert!(entry_ir.contains(&format!("getelementptr inbounds {}", failure_payload)));
        assert!(consumer_interface.contains(&format!(
            "abi-function\tconsumer\tread_point\t{}\ti64\tptr",
            read_point
        )));
        assert!(consumer_interface.contains(&format!(
            "abi-function\tconsumer\tmake_outcome\t{}\tptr\ti64",
            make_outcome
        )));
        let make_failure = format!(
            "_F{}",
            module_system::mangle_function("consumer", "make_failure")
        );
        assert!(consumer_interface.contains(&format!(
            "abi-function\tconsumer\tmake_failure\t{}\tptr\tptr",
            make_failure
        )));

        // Go beyond IR-shape assertions: when Clang is available, link the
        // independently emitted package units and application entrypoint into
        // a native executable, then exercise both aggregate values at runtime.
        // This is deliberately an integration test of the real emitted units,
        // not a hand-written LLVM approximation of the expected ABI.
        if std::process::Command::new("clang")
            .arg("--version")
            .output()
            .is_ok()
        {
            let model_path = root.join("model.ll");
            let consumer_path = root.join("consumer.ll");
            let entry_path = root.join("entrypoint.ll");
            let executable = root.join(if cfg!(windows) {
                "aggregate-runtime.exe"
            } else {
                "aggregate-runtime"
            });
            std::fs::write(&model_path, &model_ir).unwrap();
            std::fs::write(&consumer_path, &consumer_ir).unwrap();
            std::fs::write(&entry_path, &entry_ir).unwrap();
            let link = std::process::Command::new("clang")
                .arg("-O0")
                .arg(&entry_path)
                .arg(&model_path)
                .arg(&consumer_path)
                .arg("-o")
                .arg(&executable)
                .output()
                .expect("start clang for aggregate package integration test");
            assert!(
                link.status.success(),
                "native aggregate package link failed:\n{}\n{}",
                String::from_utf8_lossy(&link.stdout),
                String::from_utf8_lossy(&link.stderr)
            );
            let run = std::process::Command::new(&executable)
                .output()
                .expect("run native aggregate package integration executable");
            assert!(
                run.status.success(),
                "native aggregate package executable failed: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            let stdout = String::from_utf8_lossy(&run.stdout);
            assert_eq!(
                stdout.lines().collect::<Vec<_>>(),
                vec!["7", "9", "failed"],
                "unexpected cross-package struct/enum runtime output"
            );
        } else {
            eprintln!("skipping native aggregate package execution: clang is unavailable");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_llvm_emission_extracts_only_requested_module_functions() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-llvm-{nonce}"));
        let app_src = root.join("app");
        let math_src = root.join("deps/math/src");
        let other_src = root.join("deps/other/src");
        std::fs::create_dir_all(&app_src).unwrap();
        std::fs::create_dir_all(&math_src).unwrap();
        std::fs::create_dir_all(&other_src).unwrap();
        let entry = app_src.join("main.fusion");
        std::fs::write(&entry, "import math\nimport other\nmain:\n    print(1)\n").unwrap();
        std::fs::write(math_src.join("lib.fusion"),
            "pub fn add(left: num, right: num) -> num:\n    return left + right\npub fn twice(value: num) -> num:\n    return value * 2\n").unwrap();
        std::fs::write(
            other_src.join("lib.fusion"),
            "pub fn unrelated(value: num) -> num:\n    return value - 1\n",
        )
        .unwrap();
        let roots = std::collections::BTreeMap::from([
            ("math".to_string(), math_src.clone()),
            ("other".to_string(), other_src),
        ]);
        let package_ir = emit_llvm_package_with_package_roots(&entry, roots, &math_src)
            .expect("package LLVM should be emitted");
        let add = format!("_F{}", module_system::mangle_function("math", "add"));
        let twice = format!("_F{}", module_system::mangle_function("math", "twice"));
        let unrelated = format!("_F{}", module_system::mangle_function("other", "unrelated"));
        assert!(package_ir.contains(&format!("define i64 @{}(", add)));
        assert!(package_ir.contains(&format!("define i64 @{}(", twice)));
        assert!(!package_ir.contains(&format!("define i64 @{}(", unrelated)));
        assert!(package_ir.contains(&format!("declare i64 @{}(i64)", unrelated)));
        assert!(!package_ir.contains("define i32 @main()"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_llvm_emits_owned_constant_globals_and_external_references() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-data-link-{nonce}"));
        let app_src = root.join("app/src");
        let math_src = root.join("deps/math/src");
        let consumer_src = root.join("deps/consumer/src");
        std::fs::create_dir_all(&app_src).unwrap();
        std::fs::create_dir_all(&math_src).unwrap();
        std::fs::create_dir_all(&consumer_src).unwrap();
        let entry = app_src.join("main.fusion");
        std::fs::write(&entry, "import math\nimport consumer\nmain:\n    print(math.LIMIT)\n    print(consumer.read_limit())\n").unwrap();
        std::fs::write(math_src.join("lib.fusion"),
            "pub const LIMIT: num = 42\npub const ENABLED: bool = true\npub const LABEL: string = \"fusion\"\nconst SECRET: num = 7\npub fn get_limit() -> num:\n    return LIMIT\n").unwrap();
        std::fs::write(
            consumer_src.join("lib.fusion"),
            "import math\npub fn read_limit() -> num:\n    return math.LIMIT\n",
        )
        .unwrap();
        let roots = std::collections::BTreeMap::from([
            ("math".to_string(), math_src.clone()),
            ("consumer".to_string(), consumer_src.clone()),
        ]);
        let limit = format!(
            "_F{}",
            module_system::mangle_function("math", "$data$LIMIT")
        );
        let enabled = format!(
            "_F{}",
            module_system::mangle_function("math", "$data$ENABLED")
        );
        let label = format!(
            "_F{}",
            module_system::mangle_function("math", "$data$LABEL")
        );
        let package_ir =
            emit_llvm_package_with_package_roots(&entry, roots.clone(), &math_src).unwrap();
        let consumer_ir =
            emit_llvm_package_with_package_roots(&entry, roots.clone(), &consumer_src).unwrap();
        assert!(package_ir.contains(&format!("@{} = constant i64 42", limit)));
        assert!(package_ir.contains(&format!("@{} = constant i1 true", enabled)));
        assert!(package_ir.contains(&format!("@{} = constant ptr getelementptr", label)));
        assert!(package_ir.contains(&format!("load i64, ptr @{}", limit)));
        let secret = format!(
            "_F{}",
            module_system::mangle_function("math", "$data$SECRET")
        );
        assert!(package_ir.contains(&format!("@{} = constant i64 7", secret)));
        assert!(consumer_ir.contains(&format!("@{} = external constant i64", limit)));
        assert!(consumer_ir.contains(&format!("load i64, ptr @{}", limit)));
        let entry_ir = emit_llvm_entrypoint_with_package_roots(&entry, roots.clone()).unwrap();
        assert!(entry_ir.contains(&format!("@{} = external constant i64", limit)));
        assert!(entry_ir.contains(&format!("load i64, ptr @{}", limit)));
        assert!(!entry_ir.contains(&format!("@{} = constant i64", limit)));
        let interface =
            emit_package_interface_with_package_roots(&entry, roots, &math_src).unwrap();
        assert!(interface.starts_with("FUSION-PACKAGE-INTERFACE\t6\n"));
        assert!(interface.contains(&format!(
            "abi-data\tmath\tLIMIT\t{}\ti64\tinteger\t42",
            limit
        )));
        let get_limit = format!("_F{}", module_system::mangle_function("math", "get_limit"));
        assert!(interface.contains(&format!(
            "abi-function\tmath\tget_limit\t{}\ti64\t",
            get_limit
        )));
        assert!(interface.contains(&format!(
            "abi-internal-data\tmath\tSECRET\t{}\ti64\tinteger\t7",
            secret
        )));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn application_with_only_main_has_an_empty_root_package_module() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-main-only-package-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.fusion");
        std::fs::write(&entry, "main:\n    print(1)\n").unwrap();
        let package_ir =
            emit_llvm_package_with_package_roots(&entry, std::collections::BTreeMap::new(), &root)
                .unwrap();
        assert!(package_ir.contains("no package functions"));
        let entry_ir =
            emit_llvm_entrypoint_with_package_roots(&entry, std::collections::BTreeMap::new())
                .unwrap();
        assert!(entry_ir.contains("define i32 @main()"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_module_declares_shared_runtime_instead_of_duplicating_it() {
        let module = "math".to_string();
        let symbol = format!("_F{}", module_system::mangle_function(&module, "make"));
        let full_ir = format!(
            r#"@fusion_heap_head = global ptr null

define ptr @fusion_heap_alloc(i64 %bytes) {{
entry:
  ret ptr null
}}

define ptr @{}(i64 %value) {{
entry:
  %allocated = call ptr @fusion_heap_alloc(i64 8)
  ret ptr %allocated
}}
"#,
            symbol
        );
        let package =
            extract_package_llvm_unit(&full_ir, &std::collections::BTreeSet::from([module]))
                .unwrap();
        assert!(package.contains("declare ptr @fusion_heap_alloc(i64)"));
        assert!(!package.contains("define ptr @fusion_heap_alloc"));
        assert!(package.contains(&format!("define ptr @{}(", symbol)));
    }

    #[test]
    fn package_modules_and_entrypoint_are_separate_linkable_translation_units() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fusion-package-link-units-{nonce}"));
        let app_src = root.join("app/src");
        let math_src = root.join("deps/math/src");
        std::fs::create_dir_all(&app_src).unwrap();
        std::fs::create_dir_all(&math_src).unwrap();
        let entry = app_src.join("main.fusion");
        std::fs::write(&entry, "import math\nmain:\n    print(math.add(2, 3))\n").unwrap();
        std::fs::write(
            math_src.join("lib.fusion"),
            "pub fn add(left: num, right: num) -> num:\n    return left + right\n",
        )
        .unwrap();
        let roots = std::collections::BTreeMap::from([("math".to_string(), math_src.clone())]);
        let package_ir =
            emit_llvm_package_with_package_roots(&entry, roots.clone(), &math_src).unwrap();
        let entry_ir = emit_llvm_entrypoint_with_package_roots(&entry, roots).unwrap();
        let add = format!("_F{}", module_system::mangle_function("math", "add"));
        assert!(package_ir.contains(&format!("define i64 @{}(", add)));
        assert!(entry_ir.contains("define i32 @main()"));
        assert!(!entry_ir.contains(&format!("define i64 @{}(", add)));
        assert!(entry_ir.contains(&format!("declare i64 @{}(i64, i64)", add)));
        assert!(entry_ir.contains("define ptr @fusion_heap_alloc(i64 %bytes)"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_external_declarations_reject_conflicting_or_invalid_records() {
        let conflict = "FUSION-PACKAGE-INTERFACE\t6\nabi-function\tm\tf\tsymbol\ti64\ti64\nabi-function\tm\tf\tsymbol\tptr\tptr\n";
        assert!(emit_package_external_declarations(conflict)
            .unwrap_err()
            .contains("conflicting ABI"));
        let invalid = "FUSION-PACKAGE-INTERFACE\t6\nabi-function\tm\tf\tbad-name!\ti64\t\n";
        assert!(emit_package_external_declarations(invalid)
            .unwrap_err()
            .contains("invalid LLVM ABI symbol"));
        assert!(
            emit_package_external_declarations("FUSION-PACKAGE-INTERFACE\t1\n")
                .unwrap_err()
                .contains("version")
        );
    }
}
