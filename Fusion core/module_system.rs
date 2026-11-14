//! Project/module loading and semantic metadata for Fusion multi-file projects.
//!
//! The current execution backends still consume a flattened AST, so `Project`
//! retains a parallel, explicit module graph for identity, visibility and import
//! validation. This is intentionally separate from source spans (which are local
//! to each file and are not globally unique).
use crate::ast::{Program, Statement};
use crate::lexer::{BlockMode, Lexer};
use crate::parser::Parser;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleImport {
    pub path: String,
    pub alias: Option<String>,
    pub selected: Vec<String>,
    pub re_exported: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleUnit {
    /// Stable, project-relative dotted module identity (`crate` for the entry file).
    pub name: String,
    pub path: PathBuf,
    pub imports: Vec<ModuleImport>,
    pub public_symbols: HashSet<String>,
    /// Declarations physically defined in this module (excludes re-exported names).
    pub local_public_symbols: HashSet<String>,
    /// Original function spellings, retained before compatibility-AST mangling.
    /// HIR identities and diagnostics must never inherit backend symbol names.
    pub function_symbols: HashSet<String>,
    pub private_symbols: HashSet<String>,
    pub statements: Vec<Statement>,
    pub statement_start: usize,
    pub statement_end: usize,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub entry_module: String,
    pub modules: Vec<ModuleUnit>,
    /// Compatibility representation consumed by today's resolver/type checker/backends.
    pub program: Program,
}

#[derive(Default)]
struct Loader {
    loaded: HashSet<PathBuf>,
    active: Vec<PathBuf>,
    modules: Vec<ModuleUnit>,
    statements: Vec<Statement>,
    root: PathBuf,
    entry: PathBuf,
    /// Manifest dependency alias -> canonical package `src` directory.
    package_roots: BTreeMap<String, PathBuf>,
}

fn collect_fusion_files(directory: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fusion_files(&path, output);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("fusion") {
            output.push(path);
        }
    }
}

impl Loader {
    fn canonical(path: &Path) -> Result<PathBuf, String> {
        path.canonicalize().map_err(|e| format!("module file '{}' could not be resolved: {}", path.display(), e))
    }

    fn package_for_path<'a>(&'a self, path: &Path) -> Option<(&'a str, &'a Path)> {
        self.package_roots.iter()
            .filter(|(_, root)| path.starts_with(root.as_path()))
            .max_by_key(|(_, root)| root.components().count())
            .map(|(alias, root)| (alias.as_str(), root.as_path()))
    }

    fn source_root_for(&self, importer: &Path) -> &Path {
        self.package_for_path(importer).map(|(_, root)| root).unwrap_or(&self.root)
    }

    fn module_candidates(&self, importer: &Path, module: &str) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        let relative = PathBuf::from(module.replace('.', std::path::MAIN_SEPARATOR_STR));

        // A sibling module is the most local interpretation of a short import.
        // This prevents `shapes/circle.fusion` importing `util` from accidentally
        // binding to a crate-level `util.fusion`.
        if let Some(parent) = importer.parent() {
            candidates.push(parent.join(&relative).with_extension("fusion"));
            candidates.push(parent.join(&relative).join("mod.fusion"));
        }

        // Dependency aliases are available from package modules too, not only
        // from the root package. Foundry supplies the resolved transitive roots.
        let mut parts = module.split('.');
        if let Some(alias) = parts.next() {
            if let Some(package_root) = self.package_roots.get(alias) {
                let rest = parts.collect::<Vec<_>>().join(".");
                if rest.is_empty() {
                    for file in ["lib.fusion", "main.fusion", "mod.fusion"] {
                        candidates.push(package_root.join(file));
                    }
                    let mut discovered = Vec::new();
                    collect_fusion_files(package_root, &mut discovered);
                    discovered.sort();
                    candidates.extend(discovered);
                } else {
                    let relative = PathBuf::from(rest.replace('.', std::path::MAIN_SEPARATOR_STR));
                    candidates.push(package_root.join(&relative).with_extension("fusion"));
                    candidates.push(package_root.join(&relative).join("mod.fusion"));
                }
            }
        }

        let local_root = self.source_root_for(importer);
        candidates.push(local_root.join(&relative).with_extension("fusion"));
        candidates.push(local_root.join(&relative).join("mod.fusion"));
        candidates.dedup();
        candidates
    }

    fn resolve_module(&self, importer: &Path, module: &str) -> Result<PathBuf, String> {
        for candidate in self.module_candidates(importer, module) {
            if !candidate.is_file() {
                continue;
            }

            // Canonicalize before checking so `..` components and symlinks cannot
            // escape the source root selected by the candidate path.
            let resolved = Self::canonical(&candidate)?;
            // The resolved file itself determines its package boundary. This
            // permits a package module to import one of its declared dependency
            // roots while still rejecting symlinks that escape every known root.
            let expected_root = self.package_for_path(&candidate)
                .map(|(_, root)| root)
                .unwrap_or_else(|| self.source_root_for(importer));
            if !resolved.starts_with(expected_root) {
                return Err(format!(
                    "module '{}' imported by '{}' resolves outside its package source root: '{}'",
                    module, importer.display(), resolved.display(),
                ));
            }
            return Ok(resolved);
        }
        Err(format!("module '{}' imported by '{}' was not found; checked package-local module paths and declared dependency roots", module, importer.display()))
    }

    fn identity(&self, path: &Path) -> String {
        if path == self.entry { return "crate".to_string(); }
        let (prefix, base) = self.package_for_path(path).map(|(alias, root)| (Some(alias), root)).unwrap_or((None, self.root.as_path()));
        let relative = path.strip_prefix(base).unwrap_or(path);
        let mut parts: Vec<String> = relative.with_extension("").components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
        if parts.last().map(String::as_str) == Some("mod") || parts.last().map(String::as_str) == Some("lib") || parts.last().map(String::as_str) == Some("main") { parts.pop(); }
        let suffix = if parts.is_empty() { String::new() } else { parts.join(".") };
        match (prefix, suffix.is_empty()) {
            (Some(alias), true) => alias.to_string(),
            (Some(alias), false) => format!("{}.{}", alias, suffix),
            (None, true) => "crate".into(),
            (None, false) => suffix,
        }
    }

    fn scan_declarations(source: &str) -> (HashSet<String>, HashSet<String>, Vec<ModuleImport>) {
        let mut public = HashSet::new();
        let mut private = HashSet::new();
        let mut imports = Vec::new();
        for raw in source.lines() {
            let line = raw.trim();
            let (is_public, item) = if let Some(rest) = line.strip_prefix("pub ") { (true, rest.trim()) } else { (false, line) };
            if let Some(rest) = item.strip_prefix("import ") {
                let mut words = rest.split_whitespace();
                let path = words.next().unwrap_or("").trim_end_matches(';');
                let alias = match (words.next(), words.next()) {
                    (Some("as"), Some(name)) => Some(name.trim_end_matches(';').to_string()),
                    _ => None,
                };
                if !path.is_empty() { imports.push(ModuleImport { path: path.to_string(), alias, selected: Vec::new(), re_exported: is_public }); }
                continue;
            }
            if let Some(rest) = item.strip_prefix("from ") {
                if let Some((path, names)) = rest.split_once(" import ") {
                    let selected = names.split(',').map(|name| name.trim().trim_end_matches(';').to_string()).filter(|name| !name.is_empty()).collect();
                    imports.push(ModuleImport { path: path.trim().to_string(), alias: None, selected, re_exported: is_public });
                }
                continue;
            }
            let name = ["fn ", "async fn ", "struct ", "enum ", "trait ", "const "]
                .iter().find_map(|prefix| item.strip_prefix(prefix).map(|tail| tail.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("")));
            if let Some(name) = name.filter(|n| !n.is_empty()) {
                if is_public { public.insert(name.to_string()); } else { private.insert(name.to_string()); }
            }
        }
        (public, private, imports)
    }

    fn visit(&mut self, path: &Path) -> Result<(), String> {
        let canonical = Self::canonical(path)?;
        if self.active.contains(&canonical) {
            let mut cycle: Vec<String> = self.active.iter().map(|p| self.identity(p)).collect();
            cycle.push(self.identity(&canonical));
            return Err(format!("module dependency cycle detected: {}", cycle.join(" -> ")));
        }
        if self.loaded.contains(&canonical) { return Ok(()); }

        let source = fs::read_to_string(&canonical).map_err(|e| format!("could not read module '{}': {}", canonical.display(), e))?;
        let (public_symbols, private_symbols, imports) = Self::scan_declarations(&source);
        let tokens = Lexer::new(&source, BlockMode::Unknown).tokenize().map_err(|e| format!("lexer in '{}': {}", canonical.display(), e))?;
        let mut parser = if canonical == self.entry { Parser::new(tokens) } else { Parser::new_module(tokens) };
        let program = parser.parse().map_err(|e| format!("parser in '{}': {}", canonical.display(), e))?;
        let function_symbols: HashSet<String> = program.statements.iter().filter_map(|statement| {
            if let Statement::Function { name, .. } = statement { Some(name.clone()) } else { None }
        }).collect();

        self.active.push(canonical.clone());
        for import in &imports {
            let dependency = self.resolve_module(&canonical, &import.path)?;
            self.visit(&dependency)?;
        }
        self.active.pop();

        if canonical != self.entry && program.statements.iter().any(|s| matches!(s, Statement::Main { .. })) {
            return Err(format!("imported module '{}' must not declare `main`; only the project entry point may define it", canonical.display()));
        }
        let statements: Vec<Statement> = program.statements.into_iter().filter(|s| !matches!(s, Statement::Import { .. })).collect();
        let statement_start = self.statements.len();
        self.statements.extend(statements.iter().cloned());
        let statement_end = self.statements.len();
        self.modules.push(ModuleUnit { name: self.identity(&canonical), path: canonical.clone(), imports, local_public_symbols: public_symbols.clone(), public_symbols, function_symbols, private_symbols, statements, statement_start, statement_end });
        self.loaded.insert(canonical);
        Ok(())
    }
}

/// Load all source files and retain their module identities and visibility tables.
pub fn load_project_with_metadata(entry: impl AsRef<Path>) -> Result<Project, String> {
    load_project_with_package_roots(entry, BTreeMap::new())
}

/// Load a project with explicitly resolved Foundry dependency source roots.
/// Keys are dependency aliases from `Foundry.toml`; values must be package `src` directories.
pub fn load_project_with_package_roots(
    entry: impl AsRef<Path>,
    package_roots: BTreeMap<String, PathBuf>,
) -> Result<Project, String> {
    let entry = Loader::canonical(entry.as_ref())?;
    let root = entry.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut canonical_roots = BTreeMap::new();
    for (alias, path) in package_roots {
        if alias.is_empty() || !alias.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("invalid package import alias '{}'; aliases used in Fusion imports must contain only ASCII letters, digits, or underscores", alias));
        }
        let canonical = Loader::canonical(&path).map_err(|e| format!("dependency '{}' source root: {}", alias, e))?;
        if !canonical.is_dir() { return Err(format!("dependency '{}' source root is not a directory: {}", alias, canonical.display())); }
        canonical_roots.insert(alias, canonical);
    }
    let mut loader = Loader { root: root.clone(), entry: entry.clone(), package_roots: canonical_roots, ..Loader::default() };
    loader.visit(&entry)?;
    validate_project(&mut loader.modules)?;
    lower_module_function_namespaces(&mut loader.modules)?;
    // Rebuild the compatibility AST after module-aware lowering. Each module's
    // ranges now point at its transformed statements, not the pre-lowering copy.
    loader.statements.clear();
    for module in &mut loader.modules {
        module.statement_start = loader.statements.len();
        loader.statements.extend(module.statements.iter().cloned());
        module.statement_end = loader.statements.len();
    }
    let span = loader.statements.first().map(statement_span).unwrap_or(crate::span::Span { start: 0, end: 0 });
    let module_info = loader.modules.iter().map(|module| {
        // HashSet iteration order is intentionally unspecified. Sort exported and
        // private names so metadata, diagnostics, snapshots and tooling output are
        // reproducible across runs and platforms.
        let mut public_symbols: Vec<_> = module.public_symbols.iter().cloned().collect();
        let mut private_symbols: Vec<_> = module.private_symbols.iter().cloned().collect();
        public_symbols.sort();
        private_symbols.sort();
        crate::ast::ModuleInfo {
            name: module.name.clone(),
            path: module.path.to_string_lossy().into_owned(),
            statement_start: module.statement_start,
            statement_end: module.statement_end,
            imports: module.imports.iter().map(|import| crate::ast::ModuleImportInfo { path: import.path.clone(), alias: import.alias.clone(), selected: import.selected.clone(), re_exported: import.re_exported }).collect(),
            public_symbols,
            private_symbols,
        }
    }).collect();
    Ok(Project { entry_module: "crate".into(), modules: loader.modules, program: Program { statements: loader.statements, span, modules: module_info } })
}

/// Compatibility API for consumers which only need the flattened AST.
pub fn load_project(entry: impl AsRef<Path>) -> Result<Program, String> {
    Ok(load_project_with_metadata(entry)?.program)
}

/// Lower module-local function names into stable internal symbols and rewrite
/// imported calls before the legacy flat resolver/type-checker sees the AST.
/// This is an incremental bridge: user-facing syntax remains module-oriented,
/// while existing execution engines receive collision-free function names.
fn lower_module_function_namespaces(modules: &mut [ModuleUnit]) -> Result<(), String> {
    let snapshots = modules.to_vec();
    let mut local_functions: HashMap<String, HashSet<String>> = HashMap::new();
    for module in &snapshots {
        let names = module.statements.iter().filter_map(|statement| match statement {
            Statement::Function { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        local_functions.insert(module.name.clone(), names);
    }

    for module in modules.iter_mut() {
        let module_name = module.name.clone();
        let local = local_functions.get(&module_name).cloned().unwrap_or_default();
        let mut import_aliases: HashMap<String, String> = HashMap::new();
        let mut unqualified: HashMap<String, Vec<String>> = HashMap::new();

        for import in &module.imports {
            let Some(target) = resolve_import_target(&snapshots, module, &import.path) else { continue; };
            if import.selected.is_empty() {
                let default_alias = import.path.rsplit('.').next().unwrap_or(&import.path).to_string();
                let alias = import.alias.clone().unwrap_or(default_alias);
                if let Some(previous) = import_aliases.insert(alias.clone(), target.name.clone()) {
                    if previous != target.name {
                        return Err(format!("module alias '{}' is ambiguous between '{}' and '{}'; add explicit aliases", alias, previous, target.name));
                    }
                }
            }
            let symbols: Vec<String> = if import.selected.is_empty() {
                target.public_symbols.iter().cloned().collect()
            } else {
                import.selected.clone()
            };
            for symbol in &symbols {
                if !target.public_symbols.contains(symbol) {
                    return Err(format!("module '{}' does not publicly export imported symbol '{}'", target.name, symbol));
                }
                if let Some(provider) = find_exported_function_provider(&snapshots, target, symbol, &mut HashSet::new()) {
                    unqualified.entry(symbol.clone()).or_default().push(provider.name.clone());
                    unqualified.entry(format!("{}::{}", target.name, symbol)).or_default().push(mangle_function(&provider.name, symbol));
                } else if !import.selected.is_empty() {
                    // Selective non-function imports are resolved by their own
                    // namespace passes (types, traits and constants). Only reject
                    // names that are not an exported declaration of any supported kind.
                    let supported = find_exported_type_provider(&snapshots, target, symbol, &mut HashSet::new()).is_some()
                        || find_exported_definition_provider(&snapshots, target, symbol, crate::hir::HirDefinitionKind::Trait, &mut HashSet::new()).is_some()
                        || find_exported_constant_provider(&snapshots, target, symbol, &mut HashSet::new()).is_some();
                    if !supported {
                        return Err(format!("selective import '{}' from '{}' does not resolve to a supported exported declaration", symbol, target.name));
                    }
                }
            }
        }
        for providers in unqualified.values_mut() {
            providers.sort();
            providers.dedup();
        }

        let mut function_map = HashMap::new();
        for name in &local {
            function_map.insert(name.clone(), mangle_function(&module_name, name));
        }
        for statement in &mut module.statements {
            rewrite_statement_module_calls(statement, &module_name, &function_map, &import_aliases, &unqualified)?;
        }
    }
    Ok(())
}

/// Internal symbol name for a function declared in `module` (the entry module is `crate`).
pub fn mangle_function(module: &str, function: &str) -> String {
    // Length-prefixed UTF-8 hex encoding is injective: paths such as `a.b` and
    // `a_b` cannot collapse to the same backend symbol after sanitization.
    fn encode(value: &str) -> String {
        value.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join("")
    }
    format!("__fusion_m{}_{}_f{}_{}", module.len(), encode(module), function.len(), encode(function))
}

/// Lower nominal type declarations and references into collision-free internal
/// names. Source-level names remain available in Project.modules/HIR metadata;
/// only the compatibility AST consumed by the legacy checker/backends is
/// rewritten. This lets two modules independently define `Point`, `Error`, etc.
pub fn lower_project_type_namespaces(modules: &[ModuleUnit], program: &mut Program) -> Result<(), String> {
    let snapshots = modules.to_vec();
    for module in modules {
        let mut names = HashMap::<String, String>::new();
        for statement in &module.statements {
            if let Statement::Struct { name, .. } | Statement::Enum { name, .. } = statement {
                names.insert(name.clone(), mangle_type(&module.name, name));
            }
        }
        for import in &module.imports {
            let Some(target) = resolve_import_target(&snapshots, module, &import.path) else { continue; };
            let alias = import.alias.clone().unwrap_or_else(|| import.path.rsplit('.').next().unwrap_or(&import.path).to_string());
            let candidates: Vec<String> = if import.selected.is_empty() {
                target.public_symbols.iter().cloned().collect()
            } else { import.selected.clone() };
            for symbol in candidates {
                let Some(provider) = find_exported_type_provider(&snapshots, target, &symbol, &mut HashSet::new()) else { continue; };
                let internal = mangle_type(&provider.name, &symbol);
                if import.selected.contains(&symbol) && !names.contains_key(&symbol) {
                    // Local declarations shadow selective imports, matching the
                    // source-level HIR resolver's lookup precedence.
                    names.insert(symbol.clone(), internal.clone());
                }
                if import.selected.is_empty() {
                    names.insert(format!("{}.{}", alias, symbol), internal.clone());
                    names.insert(format!("{}.{}", target.name, symbol), internal);
                }
            }
        }
        let end = module.statement_end.min(program.statements.len());
        let start = module.statement_start.min(end);
        for statement in &mut program.statements[start..end] {
            rewrite_statement_type_names(statement, &names);
        }
    }
    Ok(())
}

/// Lower traits and constants into module-local backend namespaces. Constant
/// references are rewritten with lexical shadow tracking, so a local variable
/// with the same spelling is never mistaken for an imported module constant.
pub fn lower_project_trait_constant_namespaces(modules: &[ModuleUnit], program: &mut Program) -> Result<(), String> {
    let snapshots = modules.to_vec();
    for module in modules {
        let mut trait_names = HashMap::<String, String>::new();
        let mut constant_names = HashMap::<String, String>::new();
        let mut aliases = HashMap::<String, String>::new();
        let mut imported_traits = HashMap::<String, String>::new();
        let mut imported_constants = HashMap::<String, String>::new();
        let local_trait_names: HashSet<String> = module.statements.iter().filter_map(|statement| match statement { Statement::Trait { name, .. } => Some(name.clone()), _ => None }).collect();
        let local_constant_names: HashSet<String> = module.statements.iter().filter_map(|statement| match statement { Statement::ConstDeclaration { name, .. } => Some(name.clone()), _ => None }).collect();
        for statement in &module.statements {
            match statement {
                Statement::Trait { name, .. } => { trait_names.insert(name.clone(), mangle_trait(&module.name, name)); }
                Statement::ConstDeclaration { name, .. } => { constant_names.insert(name.clone(), mangle_constant(&module.name, name)); }
                _ => {}
            }
        }
        for import in &module.imports {
            let Some(target) = resolve_import_target(&snapshots, module, &import.path) else { continue; };
            let alias = import.alias.clone().unwrap_or_else(|| import.path.rsplit('.').next().unwrap_or(&import.path).to_string());
            if import.selected.is_empty() { aliases.insert(alias.clone(), target.name.clone()); }
            let symbols: Vec<String> = if import.selected.is_empty() { target.public_symbols.iter().cloned().collect() } else { import.selected.clone() };
            for symbol in symbols {
                if !target.public_symbols.contains(&symbol) {
                    return Err(format!("module '{}' does not publicly export imported symbol '{}'", target.name, symbol));
                }
                if let Some(provider) = find_exported_definition_provider(&snapshots, target, &symbol, crate::hir::HirDefinitionKind::Trait, &mut HashSet::new()) {
                    let internal = mangle_trait(&provider.name, &symbol);
                    if !import.selected.is_empty() && !local_trait_names.contains(&symbol) {
                        if let Some(previous) = imported_traits.insert(symbol.clone(), internal.clone()) {
                            if previous != internal { return Err(format!("ambiguous imported trait '{}': it is provided by multiple modules; qualify the implementation or import only one provider", symbol)); }
                        }
                        trait_names.insert(symbol.clone(), internal.clone());
                    }
                    if import.selected.is_empty() { trait_names.insert(format!("{}.{}", alias, symbol), internal); }
                }
                if let Some(provider) = find_exported_constant_provider(&snapshots, target, &symbol, &mut HashSet::new()) {
                    let internal = mangle_constant(&provider.name, &symbol);
                    if !import.selected.is_empty() && !local_constant_names.contains(&symbol) {
                        if let Some(previous) = imported_constants.insert(symbol.clone(), internal.clone()) {
                            if previous != internal { return Err(format!("ambiguous imported constant '{}': it is provided by multiple modules", symbol)); }
                        }
                        constant_names.insert(symbol.clone(), internal.clone());
                    }
                    if import.selected.is_empty() { constant_names.insert(format!("{}::{}", target.name, symbol), internal); }
                }
            }
        }
        let start = module.statement_start.min(program.statements.len());
        let end = module.statement_end.min(program.statements.len()).max(start);
        for statement in &mut program.statements[start..end] {
            match statement {
                Statement::Trait { name, .. } => if let Some(internal) = trait_names.get(name) { *name = internal.clone(); },
                Statement::Impl { trait_name: Some(name), .. } => if let Some(internal) = trait_names.get(name) { *name = internal.clone(); },
                _ => {}
            }
            rewrite_constant_statement(statement, &constant_names, &aliases, &mut HashSet::new(), true);
        }
    }
    Ok(())
}

pub fn mangle_trait(module: &str, name: &str) -> String {
    format!("__fusion_m{}_{}_r{}_{}", module.len(), module.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""), name.len(), name.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""))
}

pub fn mangle_constant(module: &str, name: &str) -> String {
    format!("__fusion_m{}_{}_c{}_{}", module.len(), module.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""), name.len(), name.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""))
}

fn reexport_allows(import: &ModuleImport, symbol: &str) -> bool {
    import.re_exported && (import.selected.is_empty() || import.selected.iter().any(|name| name == symbol))
}

fn find_exported_constant_provider<'a>(modules: &'a [ModuleUnit], module: &'a ModuleUnit, symbol: &str, visited: &mut HashSet<String>) -> Option<&'a ModuleUnit> {
    if !visited.insert(module.name.clone()) || !module.public_symbols.contains(symbol) { return None; }
    if module.local_public_symbols.contains(symbol) && module.statements.iter().any(|s| matches!(s, Statement::ConstDeclaration { name, .. } if name == symbol)) { return Some(module); }
    for import in module.imports.iter().filter(|i| reexport_allows(i, symbol)) {
        if let Some(target) = resolve_import_target(modules, module, &import.path) {
            if let Some(found) = find_exported_constant_provider(modules, target, symbol, visited) { return Some(found); }
        }
    }
    None
}

fn rewrite_constant_statement(statement: &mut Statement, names: &HashMap<String, String>, aliases: &HashMap<String, String>, shadowed: &mut HashSet<String>, top_level: bool) {
    use crate::ast::{Expression as E, Statement as S};
    fn expr(e: &mut E, names: &HashMap<String, String>, aliases: &HashMap<String, String>, shadowed: &HashSet<String>) {
        match e {
            E::Identifier { name, .. } => if !shadowed.contains(name) { if let Some(mapped) = names.get(name) { *name = mapped.clone(); } },
            E::Property { object, name, span } => {
                let replacement = if let E::Identifier { name: alias, .. } = object.as_ref() {
                    if !shadowed.contains(alias) {
                        aliases.get(alias).and_then(|target| names.get(&format!("{}::{}", target, name))).cloned()
                    } else { None }
                } else { None };
                if let Some(mapped) = replacement {
                    let source_span = *span;
                    *e = E::Identifier { name: mapped, span: source_span };
                    return;
                }
                expr(object, names, aliases, shadowed);
            }
            E::Array { elements, .. } | E::Tuple { elements, .. } => for x in elements { expr(x, names, aliases, shadowed); },
            E::Index { array, index, .. } | E::Binary { left: array, right: index, .. } => { expr(array, names, aliases, shadowed); expr(index, names, aliases, shadowed); },
            E::Unary { expression, .. } | E::Await { expression, .. } | E::Conversion { expression, .. } => expr(expression, names, aliases, shadowed),
            E::MethodCall { object, arguments, .. } => { expr(object, names, aliases, shadowed); for x in arguments { expr(x, names, aliases, shadowed); } },
            E::Call { arguments, .. } => for x in arguments { expr(x, names, aliases, shadowed); },
            E::StructConstructor { fields, .. } => for (_, x) in fields { expr(x, names, aliases, shadowed); },
            E::EnumConstructor { arguments, .. } => for x in arguments { expr(x, names, aliases, shadowed); },
            E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. } | E::String { .. } => {}
        }
    }
    fn block(body: &mut [S], names: &HashMap<String, String>, aliases: &HashMap<String, String>, shadowed: &mut HashSet<String>) {
        let mut local = shadowed.clone();
        for child in body { rewrite_constant_statement(child, names, aliases, &mut local, false); }
    }
    fn collect_pattern_names(pattern: &crate::ast::Pattern, output: &mut HashSet<String>) {
        use crate::ast::PatternKind as P;
        match &pattern.kind {
            P::Identifier(name) => { output.insert(name.clone()); }
            P::At { name, pattern } => { output.insert(name.clone()); collect_pattern_names(pattern, output); }
            P::Variant { bindings, .. } | P::Tuple(bindings) | P::Or(bindings) => for child in bindings { collect_pattern_names(child, output); },
            P::Struct { fields, .. } => for (_, child) in fields { collect_pattern_names(child, output); },
            _ => {}
        }
    }
    match statement {
        S::ConstDeclaration { name, value, .. } => {
            expr(value, names, aliases, shadowed);
            if top_level { if let Some(mapped) = names.get(name) { *name = mapped.clone(); } }
            shadowed.insert(name.clone());
        }
        S::VariableDeclarations { declarations, .. } => for declaration in declarations {
            if let Some(value) = &mut declaration.value { expr(value, names, aliases, shadowed); }
            shadowed.insert(declaration.name.clone());
        },
        S::Function { parameters, body, .. } => {
            let mut local = shadowed.clone();
            for parameter in parameters { local.insert(parameter.name.clone()); }
            block(body, names, aliases, &mut local);
        }
        S::Main { body, .. } => block(body, names, aliases, shadowed),
        S::Assignment { target, value, .. } => { expr(target, names, aliases, shadowed); expr(value, names, aliases, shadowed); },
        S::Expression { expression, .. } | S::Call { expression, .. } | S::Defer { expression, .. } => expr(expression, names, aliases, shadowed),
        S::Return { value: Some(value), .. } => expr(value, names, aliases, shadowed),
        S::If { condition, body, else_body, .. } => { expr(condition, names, aliases, shadowed); block(body, names, aliases, shadowed); if let Some(body) = else_body { block(body, names, aliases, shadowed); } },
        S::While { condition, body, .. } => { expr(condition, names, aliases, shadowed); block(body, names, aliases, shadowed); },
        S::For { variable, start, end, body, .. } => { expr(start, names, aliases, shadowed); expr(end, names, aliases, shadowed); let mut local = shadowed.clone(); local.insert(variable.clone()); block(body, names, aliases, &mut local); },
        S::ForEach { variable, iterable, body, .. } => { expr(iterable, names, aliases, shadowed); let mut local = shadowed.clone(); local.insert(variable.clone()); block(body, names, aliases, &mut local); },
        S::Match { expression, arms, .. } => { expr(expression, names, aliases, shadowed); for arm in arms { let mut local = shadowed.clone(); collect_pattern_names(&arm.pattern, &mut local); if let Some(guard) = &mut arm.guard { expr(guard, names, aliases, &local); } block(&mut arm.body, names, aliases, &mut local); } },
        S::Impl { methods, .. } => block(methods, names, aliases, shadowed),
        S::Struct { .. } | S::Enum { .. } | S::Trait { .. } | S::Import { .. } | S::Return { value: None, .. } | S::Break { .. } | S::Continue { .. } => {}
    }
}

/// Build the backend-symbol to stable-HIR-identity table for nominal types.
/// The compatibility AST uses injective internal names, while this table keeps
/// the declaration identity available to execution and code-generation stages.
pub fn backend_type_definition_ids(program: &Program) -> HashMap<String, crate::hir::HirDefinitionId> {
    let mut result = HashMap::new();
    for module in &program.modules {
        let end = module.statement_end.min(program.statements.len());
        let start = module.statement_start.min(end);
        for statement in &program.statements[start..end] {
            let (backend_symbol, kind) = match statement {
                Statement::Struct { name, .. } => (name, crate::hir::HirDefinitionKind::Struct),
                Statement::Enum { name, .. } => (name, crate::hir::HirDefinitionKind::Enum),
                _ => continue,
            };
            // ModuleInfo deliberately retains source spellings even though the
            // compatibility AST declaration name has already been mangled.
            // Recover the local spelling by checking the module's declared names.
            let source_name = module
                .public_symbols
                .iter()
                .chain(module.private_symbols.iter())
                .find(|source_name| mangle_type(&module.name, source_name) == backend_symbol.as_str());
            if let Some(source_name) = source_name {
                result.insert(
                    backend_symbol.clone(),
                    crate::hir::HirDefinitionId::new(module.name.clone(), source_name.clone(), kind),
                );
            }
        }
    }
    result
}

pub fn mangle_type(module: &str, type_name: &str) -> String {
    fn encode(value: &str) -> String {
        value.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join("")
    }
    format!("__fusion_m{}_{}_t{}_{}", module.len(), encode(module), type_name.len(), encode(type_name))
}

fn find_exported_type_provider<'a>(
    modules: &'a [ModuleUnit], module: &'a ModuleUnit, symbol: &str, visited: &mut HashSet<String>,
) -> Option<&'a ModuleUnit> {
    if !visited.insert(module.name.clone()) || !module.public_symbols.contains(symbol) { return None; }
    if module.local_public_symbols.contains(symbol) && module.statements.iter().any(|statement| matches!(statement,
        Statement::Struct { name, .. } | Statement::Enum { name, .. } if name == symbol)) {
        return Some(module);
    }
    for import in module.imports.iter().filter(|import| reexport_allows(import, symbol)) {
        if let Some(target) = resolve_import_target(modules, module, &import.path) {
            if let Some(provider) = find_exported_type_provider(modules, target, symbol, visited) { return Some(provider); }
        }
    }
    None
}

fn rewrite_type_spelling(value: &mut String, names: &HashMap<String, String>) {
    if let Some(replacement) = names.get(value) { *value = replacement.clone(); return; }
    // Preserve generic/container syntax while replacing complete type-name
    // tokens (including dotted module aliases) inside e.g. `Array<Point>`.
    let mut output = String::with_capacity(value.len());
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphanumeric() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.') { i += 1; }
            let token: String = chars[start..i].iter().collect();
            output.push_str(names.get(&token).map(String::as_str).unwrap_or(&token));
        } else { output.push(chars[i]); i += 1; }
    }
    *value = output;
}

fn rewrite_statement_type_names(statement: &mut Statement, names: &HashMap<String, String>) {
    use crate::ast::{Expression as E, PatternKind as P, Statement as S};
    fn expr(e: &mut E, names: &HashMap<String, String>) {
        match e {
            E::Array { elements, .. } | E::Tuple { elements, .. } => for x in elements { expr(x, names); },
            E::Index { array, index, .. } | E::Binary { left: array, right: index, .. } => { expr(array, names); expr(index, names); },
            E::Property { object, .. } | E::Await { expression: object, .. } | E::Unary { expression: object, .. } => expr(object, names),
            E::Conversion { expression, target_type, .. } => { expr(expression, names); rewrite_type_spelling(target_type, names); },
            E::MethodCall { object, arguments, generic_arguments, .. } => { expr(object, names); for x in arguments { expr(x, names); } for t in generic_arguments { rewrite_type_spelling(t, names); } },
            E::Call { arguments, generic_arguments, .. } => { for x in arguments { expr(x, names); } for t in generic_arguments { rewrite_type_spelling(t, names); } },
            E::StructConstructor { name, fields, .. } => { if let Some(n) = names.get(name) { *name = n.clone(); } for (_, x) in fields { expr(x, names); } },
            E::EnumConstructor { enum_name, arguments, .. } => { if let Some(n) = names.get(enum_name) { *enum_name = n.clone(); } for x in arguments { expr(x, names); } },
            E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. } | E::String { .. } | E::Identifier { .. } => {}
        }
    }
    fn pattern(p: &mut crate::ast::Pattern, names: &HashMap<String, String>) {
        match &mut p.kind {
            P::Variant { name, bindings } => {
                if let Some((prefix, suffix)) = name.split_once("::").map(|(a, b)| (a.to_string(), b.to_string())) {
                    if let Some(n) = names.get(&prefix) { *name = format!("{}::{}", n, suffix); }
                }
                for child in bindings { pattern(child, names); }
            }
            P::Struct { name, fields, .. } => { if let Some(n) = names.get(name) { *name = n.clone(); } for (_, child) in fields { pattern(child, names); } },
            P::Tuple(children) | P::Or(children) => for child in children { pattern(child, names); },
            P::At { pattern: child, .. } => pattern(child, names),
            _ => {}
        }
    }
    fn block(body: &mut [S], names: &HashMap<String, String>) { for child in body { rewrite_statement_type_names(child, names); } }
    match statement {
        S::VariableDeclarations { declarations, .. } => for d in declarations { if let Some(t) = &mut d.declared_type { rewrite_type_spelling(t, names); } if let Some(v) = &mut d.value { expr(v, names); } },
        S::ConstDeclaration { declared_type, value, .. } => { if let Some(t) = declared_type { rewrite_type_spelling(t, names); } expr(value, names); },
        S::Assignment { target, value, .. } => { expr(target, names); expr(value, names); },
        S::Function { parameters, return_type, body, .. } => { for p in parameters { if let Some(t) = &mut p.type_name { rewrite_type_spelling(t, names); } } if let Some(t) = return_type { rewrite_type_spelling(t, names); } block(body, names); },
        S::Struct { name, fields, .. } => { if let Some(n) = names.get(name) { *name = n.clone(); } for f in fields { rewrite_type_spelling(&mut f.type_name, names); } },
        S::Enum { name, variants, .. } => { if let Some(n) = names.get(name) { *name = n.clone(); } for v in variants { for t in &mut v.fields { rewrite_type_spelling(t, names); } } },
        S::Main { body, .. } => block(body, names),
        S::Trait { methods, .. } => for m in methods { for p in &mut m.parameters { if let Some(t) = &mut p.type_name { rewrite_type_spelling(t, names); } } if let Some(t) = &mut m.return_type { rewrite_type_spelling(t, names); } },
        S::Impl { type_name, methods, .. } => { rewrite_type_spelling(type_name, names); block(methods, names); },
        S::Match { expression, arms, .. } => { expr(expression, names); for arm in arms { pattern(&mut arm.pattern, names); if let Some(g) = &mut arm.guard { expr(g, names); } block(&mut arm.body, names); } },
        S::Defer { expression, .. } | S::Return { value: Some(expression), .. } | S::Expression { expression, .. } | S::Call { expression, .. } => expr(expression, names),
        S::If { condition, body, else_body, .. } => { expr(condition, names); block(body, names); if let Some(b) = else_body { block(b, names); } },
        S::While { condition, body, .. } => { expr(condition, names); block(body, names); },
        S::For { start, end, body, .. } => { expr(start, names); expr(end, names); block(body, names); },
        S::ForEach { iterable, body, .. } => { expr(iterable, names); block(body, names); },
        S::Return { value: None, .. } | S::Break { .. } | S::Continue { .. } | S::Import { .. } => {}
    }
}

fn find_exported_function_provider<'a>(
    modules: &'a [ModuleUnit],
    module: &'a ModuleUnit,
    symbol: &str,
    visited: &mut HashSet<String>,
) -> Option<&'a ModuleUnit> {
    if !visited.insert(module.name.clone()) || !module.public_symbols.contains(symbol) { return None; }
    if module.local_public_symbols.contains(symbol)
        && module.statements.iter().any(|s| matches!(s, Statement::Function { name, .. } if name == symbol)) {
        return Some(module);
    }
    for import in module.imports.iter().filter(|i| reexport_allows(i, symbol)) {
        if let Some(target) = resolve_import_target(modules, module, &import.path) {
            if let Some(found) = find_exported_function_provider(modules, target, symbol, visited) { return Some(found); }
        }
    }
    None
}

fn rewrite_statement_module_calls(
    statement: &mut Statement,
    current_module: &str,
    local_functions: &HashMap<String, String>,
    aliases: &HashMap<String, String>,
    imported: &HashMap<String, Vec<String>>,
) -> Result<(), String> {
    use crate::ast::Expression;
    let rewrite = |expr: &mut Expression| rewrite_expression_module_calls(expr, current_module, local_functions, aliases, imported);
    match statement {
        Statement::VariableDeclarations { declarations, .. } => for d in declarations { if let Some(v) = &mut d.value { rewrite(v)?; } },
        Statement::ConstDeclaration { value, .. } => rewrite(value)?,
        Statement::Assignment { target, value, .. } => { rewrite(target)?; rewrite(value)?; },
        Statement::Function { name, body, .. } => {
            if let Some(mangled) = local_functions.get(name) { *name = mangled.clone(); }
            for child in body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; }
        }
        Statement::Main { body, .. } => for child in body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; },
        Statement::Match { expression, arms, .. } => {
            rewrite(expression)?;
            for arm in arms { if let Some(g) = &mut arm.guard { rewrite(g)?; } for child in &mut arm.body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; } }
        }
        Statement::Defer { expression, .. } | Statement::Return { value: Some(expression), .. } => rewrite(expression)?,
        Statement::If { condition, body, else_body, .. } => {
            rewrite(condition)?;
            for child in body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; }
            if let Some(body) = else_body { for child in body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; } }
        }
        Statement::While { condition, body, .. } => { rewrite(condition)?; for child in body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; } }
        Statement::For { start, end, body, .. } => { rewrite(start)?; rewrite(end)?; for child in body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; } }
        Statement::ForEach { iterable, body, .. } => { rewrite(iterable)?; for child in body { rewrite_statement_module_calls(child, current_module, local_functions, aliases, imported)?; } }
        Statement::Expression { expression, .. } | Statement::Call { expression, .. } => rewrite(expression)?,
        Statement::Return { value: None, .. } | Statement::Break { .. } | Statement::Continue { .. }
        | Statement::Struct { .. } | Statement::Enum { .. } | Statement::Trait { .. }
        | Statement::Impl { .. } | Statement::Import { .. } => {}
    }
    Ok(())
}

fn rewrite_expression_module_calls(
    expression: &mut crate::ast::Expression,
    current_module: &str,
    local_functions: &HashMap<String, String>,
    aliases: &HashMap<String, String>,
    imported: &HashMap<String, Vec<String>>,
) -> Result<(), String> {
    use crate::ast::Expression as E;
    let mut replacement = None;
    match expression {
        E::Array { elements, .. } | E::Tuple { elements, .. } => for e in elements { rewrite_expression_module_calls(e, current_module, local_functions, aliases, imported)?; },
        E::Index { array, index, .. } => { rewrite_expression_module_calls(array, current_module, local_functions, aliases, imported)?; rewrite_expression_module_calls(index, current_module, local_functions, aliases, imported)?; },
        E::Property { object, .. } | E::Await { expression: object, .. } | E::Unary { expression: object, .. } | E::Conversion { expression: object, .. } => rewrite_expression_module_calls(object, current_module, local_functions, aliases, imported)?,
        E::MethodCall { object, method, arguments, generic_arguments, span } => {
            rewrite_expression_module_calls(object, current_module, local_functions, aliases, imported)?;
            for arg in arguments.iter_mut() { rewrite_expression_module_calls(arg, current_module, local_functions, aliases, imported)?; }
            if let E::Identifier { name: module_alias, .. } = object.as_ref() {
                if let Some(target_module) = aliases.get(module_alias) {
                    let qualified_key = format!("{}::{}", target_module, method);
                    let mangled = imported.get(&qualified_key).and_then(|items| items.first()).cloned()
                        .ok_or_else(|| format!("module '{}' does not export function '{}'", target_module, method))?;
                    replacement = Some(E::Call {
                        name: mangled,
                        arguments: arguments.clone(),
                        generic_arguments: generic_arguments.clone(),
                        span: *span,
                    });
                }
            }
        }
        E::Call { name, arguments, .. } => {
            for arg in arguments.iter_mut() { rewrite_expression_module_calls(arg, current_module, local_functions, aliases, imported)?; }
            if let Some(mangled) = local_functions.get(name) { *name = mangled.clone(); }
            else if let Some(providers) = imported.get(name) {
                if providers.len() == 1 { *name = mangle_function(&providers[0], name); }
                else if providers.len() > 1 { return Err(format!("ambiguous imported function '{}': qualify the call with its module name or alias", name)); }
            }
        }
        E::StructConstructor { fields, .. } => for (_, value) in fields { rewrite_expression_module_calls(value, current_module, local_functions, aliases, imported)?; },
        E::EnumConstructor { arguments, .. } => for arg in arguments { rewrite_expression_module_calls(arg, current_module, local_functions, aliases, imported)?; },
        E::Binary { left, right, .. } => { rewrite_expression_module_calls(left, current_module, local_functions, aliases, imported)?; rewrite_expression_module_calls(right, current_module, local_functions, aliases, imported)?; },
        E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. } | E::String { .. } | E::Identifier { .. } => {}
    }
    if let Some(replacement) = replacement { *expression = replacement; }
    let _ = current_module;
    Ok(())
}

fn validate_project(modules: &mut [ModuleUnit]) -> Result<(), String> {
    // A file module (`foo.fusion`) and a directory module (`foo/mod.fusion`)
    // have the same logical identity. Loading both through different import
    // paths must be rejected rather than silently choosing whichever happens
    // to appear last in the flattened namespace.
    let mut identities: HashMap<&str, &Path> = HashMap::new();
    for module in modules.iter() {
        if let Some(previous_path) = identities.insert(&module.name, &module.path) {
            if previous_path != module.path.as_path() {
                return Err(format!(
                    "duplicate module identity '{}': '{}' and '{}' both resolve to the same module path; use one module layout",
                    module.name,
                    previous_path.display(),
                    module.path.display(),
                ));
            }
        }
    }

    // Resolve every import to exactly one loaded module. Ambiguous short names
    // must be diagnosed instead of binding according to module traversal order.
    for importer in modules.iter() {
        for import in &importer.imports {
            if resolve_import_target(modules, importer, &import.path).is_none() {
                return Err(format!(
                    "module import '{}' in '{}' is ambiguous or has no loaded module target; use an explicit package-qualified module path",
                    import.path, importer.name,
                ));
            }
        }
    }

    // Propagate exported names through `pub import ...` re-export edges to a fixed point.
    loop {
        let mut additions: Vec<(usize, Vec<String>)> = Vec::new();
        for (index, module) in modules.iter().enumerate() {
            let mut names = Vec::new();
            for import in module.imports.iter().filter(|i| i.re_exported) {
                // Resolve through the same module/package-aware lookup used by
                // semantic resolution. Looking up `import.path` directly in the
                // module-name map loses package-qualified targets such as
                // `math.geometry` and makes their public API vanish at re-export.
                if let Some(target) = resolve_import_target(modules, module, &import.path) {
                    if import.selected.is_empty() {
                        names.extend(target.public_symbols.iter().cloned());
                    } else {
                        names.extend(import.selected.iter().filter(|name| target.public_symbols.contains(*name)).cloned());
                    }
                }
            }
            additions.push((index, names));
        }
        let mut changed = false;
        for (index, names) in additions {
            for name in names { changed |= modules[index].public_symbols.insert(name); }
        }
        if !changed { break; }
    }
    // Functions, nominal types, traits and constants are lowered into
    // module-qualified compatibility symbols before the legacy type checker
    // runs. HIR identities remain source-level and independent of these names.
    let mut declarations: HashMap<String, (String, &'static str)> = HashMap::new();
    for module in modules.iter() {
        for statement in &module.statements {
            let (name, namespace) = match statement {
                Statement::Function { name, .. } => (Some(name.as_str()), "function"),
                Statement::Struct { name, .. } | Statement::Enum { name, .. } => (Some(name.as_str()), "type"),
                Statement::Trait { name, .. } => (Some(name.as_str()), "trait"),
                Statement::ConstDeclaration { name, .. } => (Some(name.as_str()), "constant"),
                _ => (None, ""),
            };
            if let Some(name) = name {
                if let Some((previous_module, previous_namespace)) = declarations.get(name) {
                    let both_namespaced = (*previous_namespace == namespace)
                        && matches!(namespace, "function" | "type" | "trait" | "constant");
                    if previous_module != &module.name && !both_namespaced {
                        return Err(format!("module symbol collision: '{}' is declared in both '{}' and '{}'; this declaration namespace is not yet module-qualified", name, previous_module, module.name));
                    }
                } else {
                    declarations.insert(name.to_string(), (module.name.clone(), namespace));
                }
            }
        }
    }

    // Validate re-export targets and reject references to private declarations from
    // every other module, including references that would otherwise bypass privacy
    // through a re-export edge. This is deliberately conservative while the resolver
    // still consumes a flattened compatibility AST.
    for importer in modules.iter() {
        let direct_targets: HashSet<&str> = importer.imports.iter()
            .filter_map(|import| resolve_import_target(modules, importer, &import.path).map(|m| m.name.as_str()))
            .collect();
        let direct_imported_symbols: HashSet<String> = importer.imports.iter().filter_map(|import| {
            resolve_import_target(modules, importer, &import.path).map(|target| {
                if import.selected.is_empty() {
                    target.public_symbols.iter().cloned().collect::<Vec<_>>()
                } else {
                    import.selected.clone()
                }
            })
        }).flatten().collect();
        for import in &importer.imports {
            let target = resolve_import_target(modules, importer, &import.path);
            if import.re_exported {
                if let Some(target) = target {
                    if target.public_symbols.is_empty() {
                        return Err(format!("cannot re-export module '{}': it has no public declarations", import.path));
                    }
                }
            }
            // Privacy applies along an actual import edge, not between arbitrary
            // modules that happen to be present in the same project.
            if let Some(target) = target {
                for selected in &import.selected {
                    if !target.public_symbols.contains(selected) {
                        return Err(format!(
                            "module '{}' cannot import or re-export '{}' from '{}': that symbol is not publicly exported",
                            importer.name, selected, target.name,
                        ));
                    }
                }
                for symbol in &target.private_symbols {
                    let locally_defined = importer.local_public_symbols.contains(symbol) || importer.private_symbols.contains(symbol);
                    if !locally_defined && source_references_symbol(&importer.statements, symbol) {
                        return Err(format!("privacy violation: module '{}' references private symbol '{}' from module '{}'; declare it `pub` to expose it", importer.name, symbol, target.name));
                    }
                }
                if !import.selected.is_empty() {
                    for symbol in &target.public_symbols {
                        let locally_defined = importer.local_public_symbols.contains(symbol) || importer.private_symbols.contains(symbol);
                        if !import.selected.contains(symbol) && !locally_defined && source_references_symbol(&importer.statements, symbol) {
                            return Err(format!("module '{}' uses '{}.{}' without selectively importing it", importer.name, target.name, symbol));
                        }
                    }
                }
            }
        }
        // The flattened compatibility resolver must not make declarations from
        // unimported modules accidentally visible. Reject such references here.
        for target in modules.iter().filter(|m| m.name != importer.name && !direct_targets.contains(m.name.as_str())) {
            for symbol in target.local_public_symbols.iter().chain(target.private_symbols.iter()) {
                let locally_defined = importer.local_public_symbols.contains(symbol) || importer.private_symbols.contains(symbol);
                // A same-spelled declaration in an unrelated module must not
                // make a valid imported symbol appear ambiguous or unimported.
                if !locally_defined && !direct_imported_symbols.contains(symbol)
                    && source_references_symbol(&importer.statements, symbol) {
                    return Err(format!("module access error: '{}' references '{}' from '{}', but does not import that module", importer.name, symbol, target.name));
                }
            }
        }
    }
    Ok(())
}

fn resolve_import_target<'a>(modules: &'a [ModuleUnit], importer: &ModuleUnit, path: &str) -> Option<&'a ModuleUnit> {
    // Explicit dotted paths are absolute identities. Short paths are resolved
    // from the importing module's nearest namespace outward, then at crate root.
    if path.contains('.') {
        if let Some(exact) = modules.iter().find(|module| module.name == path) {
            return Some(exact);
        }
    }

    let mut namespace = importer.name.as_str();
    while namespace != "crate" && !namespace.is_empty() {
        let candidate = format!("{namespace}.{path}");
        if let Some(found) = modules.iter().find(|module| module.name == candidate) {
            return Some(found);
        }
        namespace = namespace.rsplit_once('.').map(|(parent, _)| parent).unwrap_or("crate");
    }

    modules.iter().find(|module| module.name == path)
}

fn source_references_symbol(statements: &[Statement], symbol: &str) -> bool {
    statements.iter().any(|statement| statement_references_symbol(statement, symbol))
}

fn statement_references_symbol(statement: &Statement, symbol: &str) -> bool {
    use crate::ast::Statement as S;
    match statement {
        S::VariableDeclarations { declarations, .. } => declarations.iter().any(|d| d.value.as_ref().is_some_and(|e| expression_references_symbol(e, symbol))),
        S::ConstDeclaration { value, .. } => expression_references_symbol(value, symbol),
        S::Assignment { target, value, .. } => expression_references_symbol(target, symbol) || expression_references_symbol(value, symbol),
        S::Function { body, .. } | S::Main { body, .. } => body.iter().any(|s| statement_references_symbol(s, symbol)),
        S::Match { expression, arms, .. } => expression_references_symbol(expression, symbol) || arms.iter().any(|arm| arm.guard.as_ref().is_some_and(|g| expression_references_symbol(g, symbol)) || arm.body.iter().any(|s| statement_references_symbol(s, symbol))),
        S::Defer { expression, .. } => expression_references_symbol(expression, symbol),
        S::If { condition, body, else_body, .. } => expression_references_symbol(condition, symbol) || body.iter().any(|s| statement_references_symbol(s, symbol)) || else_body.as_ref().is_some_and(|b| b.iter().any(|s| statement_references_symbol(s, symbol))),
        S::While { condition, body, .. } => expression_references_symbol(condition, symbol) || body.iter().any(|s| statement_references_symbol(s, symbol)),
        S::For { start, end, body, .. } => expression_references_symbol(start, symbol) || expression_references_symbol(end, symbol) || body.iter().any(|s| statement_references_symbol(s, symbol)),
        S::ForEach { iterable, body, .. } => expression_references_symbol(iterable, symbol) || body.iter().any(|s| statement_references_symbol(s, symbol)),
        S::Return { value, .. } => value.as_ref().is_some_and(|e| expression_references_symbol(e, symbol)),
        S::Expression { expression, .. } | S::Call { expression, .. } => expression_references_symbol(expression, symbol),
        S::Impl { methods, .. } => methods.iter().any(|method| statement_references_symbol(method, symbol)),
        S::Break { .. } | S::Continue { .. } | S::Struct { .. } | S::Enum { .. } | S::Trait { .. } | S::Import { .. } => false,
    }
}

fn expression_references_symbol(expression: &crate::ast::Expression, symbol: &str) -> bool {
    use crate::ast::Expression as E;
    match expression {
        E::Call { name, arguments, .. } => name == symbol || arguments.iter().any(|e| expression_references_symbol(e, symbol)),
        E::MethodCall { object, arguments, .. } => expression_references_symbol(object, symbol) || arguments.iter().any(|e| expression_references_symbol(e, symbol)),
        E::StructConstructor { name, fields, .. } => name == symbol || fields.iter().any(|(_, e)| expression_references_symbol(e, symbol)),
        E::EnumConstructor { enum_name, arguments, .. } => enum_name == symbol || arguments.iter().any(|e| expression_references_symbol(e, symbol)),
        E::Array { elements, .. } | E::Tuple { elements, .. } => elements.iter().any(|e| expression_references_symbol(e, symbol)),
        E::Index { array, index, .. } => expression_references_symbol(array, symbol) || expression_references_symbol(index, symbol),
        E::Property { object, .. } | E::Await { expression: object, .. } | E::Unary { expression: object, .. } | E::Conversion { expression: object, .. } => expression_references_symbol(object, symbol),
        E::Binary { left, right, .. } => expression_references_symbol(left, symbol) || expression_references_symbol(right, symbol),
        E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. } | E::String { .. } | E::Identifier { .. } => false,
    }
}

fn statement_span(statement: &Statement) -> crate::span::Span {
    match statement {
        Statement::VariableDeclarations { span, .. } | Statement::ConstDeclaration { span, .. }
        | Statement::Assignment { span, .. } | Statement::Function { span, .. }
        | Statement::Struct { span, .. } | Statement::Enum { span, .. } | Statement::Main { span, .. }
        | Statement::Import { span, .. } | Statement::Trait { span, .. } | Statement::Impl { span, .. }
        | Statement::Expression { span, .. } | Statement::Call { span, .. } | Statement::Return { span, .. }
        | Statement::Break { span } | Statement::Continue { span } | Statement::Defer { span, .. }
        | Statement::If { span, .. } | Statement::While { span, .. } | Statement::For { span, .. }
        | Statement::ForEach { span, .. } | Statement::Match { span, .. } => *span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-modules-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn module_identity_and_public_visibility_are_preserved() {
        let p = TempProject::new();
        p.write("main.fusion", "import math\nmain:\n    print(double(21))\n");
        p.write("math.fusion", "pub fn double(value: num) -> num:\n    return value * 2\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        assert!(project.modules.iter().any(|m| m.name == "math" && m.public_symbols.contains("double")));
    }
    #[test]
    fn reports_missing_imported_module() {
        let p = TempProject::new(); p.write("main.fusion", "import missing\nmain:\n    print(1)\n");
        assert!(load_project(p.0.join("main.fusion")).unwrap_err().contains("was not found"));
    }
    #[test]
    fn reports_module_dependency_cycles() {
        let p = TempProject::new(); p.write("main.fusion", "import alpha\nmain:\n    print(1)\n"); p.write("alpha.fusion", "import main\n");
        assert!(load_project(p.0.join("main.fusion")).unwrap_err().contains("cycle detected"));
    }
    #[test]
    fn reexports_public_symbols() {
        let p = TempProject::new();
        p.write("main.fusion", "import facade\nmain:\n    print(1)\n");
        p.write("facade.fusion", "pub import math\n");
        p.write("math.fusion", "pub fn value() -> num:\n    return 7\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let facade = project.modules.iter().find(|m| m.name == "facade").unwrap();
        assert!(facade.public_symbols.contains("value"));
    }
    #[test]
    fn dependency_package_can_reexport_its_public_module_api() {
        use crate::hir::HirDefinitionKind;
        let p = TempProject::new();
        p.write("app/main.fusion", "import facade\nmain:\n    let point: facade.Point = facade.Point { x: 1 }\n    print(point.x)\n");
        p.write("deps/facade/src/lib.fusion", "pub import geometry\n");
        p.write("deps/facade/src/geometry.fusion", "pub struct Point { x: num }\npub struct Hidden { y: num }\n");
        let mut roots = BTreeMap::new();
        roots.insert("facade".to_string(), p.0.join("deps/facade/src"));
        let project = load_project_with_package_roots(p.0.join("app/main.fusion"), roots).unwrap();
        let facade = project.modules.iter().find(|module| module.name == "facade").unwrap();
        assert!(facade.public_symbols.contains("Point"));
        assert!(facade.public_symbols.contains("Hidden"));
        let point = resolve_qualified_definition(&project, "crate", "facade", "Point", HirDefinitionKind::Struct).unwrap();
        assert_eq!(point.module, "facade.geometry");
        let compiled = crate::compile_project_with_package_roots(p.0.join("app/main.fusion"), {
            let mut roots = BTreeMap::new();
            roots.insert("facade".to_string(), p.0.join("deps/facade/src"));
            roots
        });
        assert!(compiled.is_ok(), "dependency re-exported type should compile: {:?}", compiled.err());
    }

    #[test]
    fn dependency_package_selectively_reexports_trait_and_type_identities() {
        use crate::hir::HirDefinitionKind;
        let p = TempProject::new();
        p.write("app/main.fusion", "import facade as f\nmain:\n    print(1)\n");
        p.write("deps/facade/src/lib.fusion", "pub from contracts import Readable\npub from models import Point\n");
        p.write("deps/facade/src/contracts.fusion", "pub trait Readable { fn read() -> num }\npub trait Internal { fn hidden() -> num }\n");
        p.write("deps/facade/src/models.fusion", "pub struct Point { x: num }\npub struct PrivatePoint { y: num }\n");
        let mut roots = BTreeMap::new();
        roots.insert("facade".to_string(), p.0.join("deps/facade/src"));
        let project = load_project_with_package_roots(p.0.join("app/main.fusion"), roots).unwrap();

        let trait_id = resolve_qualified_definition(
            &project, "crate", "f", "Readable", HirDefinitionKind::Trait,
        ).unwrap();
        assert_eq!(trait_id.module, "facade.contracts");
        assert_eq!(trait_id.name, "Readable");
        let type_id = resolve_qualified_definition(
            &project, "crate", "f", "Point", HirDefinitionKind::Struct,
        ).unwrap();
        assert_eq!(type_id.module, "facade.models");
        assert_eq!(type_id.name, "Point");

        assert!(resolve_qualified_definition(
            &project, "crate", "f", "Internal", HirDefinitionKind::Trait,
        ).is_err());
        assert!(resolve_qualified_definition(
            &project, "crate", "f", "PrivatePoint", HirDefinitionKind::Struct,
        ).is_err());
    }

    #[test]
    fn selectively_reexports_only_named_public_symbols() {
        let p = TempProject::new();
        p.write("main.fusion", "import facade\nmain:\n    print(1)\n");
        p.write("facade.fusion", "pub from geometry import Point\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\npub struct Vector { y: num }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let facade = project.modules.iter().find(|m| m.name == "facade").unwrap();
        assert!(facade.public_symbols.contains("Point"));
        assert!(!facade.public_symbols.contains("Vector"));
        let provider = find_exported_type_provider(&project.modules, facade, "Point", &mut HashSet::new());
        assert_eq!(provider.map(|module| module.name.as_str()), Some("geometry"));
        assert!(find_exported_type_provider(&project.modules, facade, "Vector", &mut HashSet::new()).is_none());
        let resolved = resolve_qualified_definition(
            &project, "crate", "facade", "Point", crate::hir::HirDefinitionKind::Struct,
        ).unwrap();
        assert_eq!(resolved.module, "geometry");
    }

    #[test]
    fn distinct_modules_can_define_and_use_same_named_structs() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry\nimport graphics\nmain:\n    let point = geometry.origin()\n    let vector = graphics.origin()\n    let local_point = geometry.Point { x: 4 }\n    let local_vector = graphics.Point { y: 5 }\n    print(point.x + vector.y + local_point.x + local_vector.y)\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\npub fn origin() -> Point:\n    return Point { x: 2 }\n");
        p.write("graphics.fusion", "pub struct Point { y: num }\npub fn origin() -> Point:\n    return Point { y: 3 }\n");
        let result = crate::compile_project(p.0.join("main.fusion"));
        assert!(result.is_ok(), "same-named module-local types should compile: {:?}", result.err());
    }

    #[test]
    fn same_named_structs_remain_distinct_across_module_boundaries() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry\nimport graphics\nmain:\n    let point = geometry.origin()\n    let vector = graphics.origin()\n    let invalid: geometry.Point = vector\n    print(point.x)\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\npub fn origin() -> Point:\n    return Point { x: 2 }\n");
        p.write("graphics.fusion", "pub struct Point { y: num }\npub fn origin() -> Point:\n    return Point { y: 3 }\n");
        let error = crate::compile_project(p.0.join("main.fusion")).unwrap_err();
        assert!(error.contains("nominal") || error.contains("type mismatch") || error.contains("expected"), "unexpected diagnostic: {}", error);
    }

    #[test]
    fn hir_index_keeps_declarations_module_qualified() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry\nimport graphics\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\n");
        p.write("graphics.fusion", "pub struct Vector { x: num }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let index = hir_symbol_index(&project);
        let geometry = crate::hir::HirDefinitionId::new("geometry", "Point", crate::hir::HirDefinitionKind::Struct);
        let graphics = crate::hir::HirDefinitionId::new("graphics", "Vector", crate::hir::HirDefinitionKind::Struct);
        assert!(index.contains(&geometry));
        assert!(index.contains(&graphics));
    }

    #[test]
    fn hir_index_scopes_impl_methods_by_type() {
        let p = TempProject::new();
        p.write("main.fusion", "main:\n    print(1)\n");
        // The index API is also covered directly because this source snapshot's
        // parser does not yet accept all qualified impl forms.
        let a = crate::hir::HirDefinitionId::scoped("crate", ["inherent".into(), "Point".into()], "new", crate::hir::HirDefinitionKind::Method);
        let b = crate::hir::HirDefinitionId::scoped("crate", ["inherent".into(), "Vector".into()], "new", crate::hir::HirDefinitionKind::Method);
        assert_ne!(a, b);
    }

    #[test]
    fn hir_symbol_index_uses_source_function_names_not_compatibility_mangled_names() {
        let p = TempProject::new();
        p.write("main.fusion", "import math\nmain:\n    print(math.double(21))\n");
        p.write("math.fusion", "pub fn double(value: num) -> num:\n    return value * 2\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let index = hir_symbol_index(&project);
        let source_id = crate::hir::HirDefinitionId::new(
            "math", "double", crate::hir::HirDefinitionKind::Function,
        );
        assert!(index.contains(&source_id));
        assert!(!index.definitions.iter().any(|id| id.module == "math" && id.name.starts_with("__fusion_")));
    }

    #[test]
    fn same_local_function_name_is_lowered_per_module() {
        let p = TempProject::new();
        p.write("main.fusion", "import alpha\nimport beta\nmain:\n    print(1)\n");
        p.write("alpha.fusion", "pub fn helper() -> num:\n    return 1\n");
        p.write("beta.fusion", "pub fn helper() -> num:\n    return 2\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let alpha = project.modules.iter().find(|m| m.name == "alpha").unwrap();
        let beta = project.modules.iter().find(|m| m.name == "beta").unwrap();
        let alpha_name = match &alpha.statements[0] { Statement::Function { name, .. } => name, _ => panic!("expected function") };
        let beta_name = match &beta.statements[0] { Statement::Function { name, .. } => name, _ => panic!("expected function") };
        assert_ne!(alpha_name, beta_name);
    }

    #[test]
    fn selective_reexport_provider_follows_only_the_edge_that_exports_the_name() {
        let p = TempProject::new();
        p.write("main.fusion", "import facade\nmain:\n    print(1)\n");
        p.write("facade.fusion", "pub from a import Shared\npub from b import Other\n");
        p.write("a.fusion", "pub fn Shared() -> num:\n    return 1\n");
        p.write("b.fusion", "pub fn Shared() -> num:\n    return 2\npub fn Other() -> num:\n    return 3\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let facade = project.modules.iter().find(|module| module.name == "facade").unwrap();
        let provider = find_exported_function_provider(&project.modules, facade, "Shared", &mut HashSet::new());
        assert_eq!(provider.map(|module| module.name.as_str()), Some("a"));
    }

    #[test]
    fn selective_reexport_of_missing_symbol_reports_diagnostic() {
        let p = TempProject::new();
        p.write("main.fusion", "import facade\nmain:\n    print(1)\n");
        p.write("facade.fusion", "pub from api import typo\n");
        p.write("api.fusion", "pub fn valid() -> num:\n    return 1\n");
        let error = load_project_with_metadata(p.0.join("main.fusion")).unwrap_err();
        assert!(error.contains("typo") && error.contains("not publicly exported"), "{error}");
    }

}

/// Build a module-qualified HIR declaration index from the loaded project.
/// This deliberately does not flatten declarations: identical local names in
/// different modules (and methods in different impl blocks) retain identity.
pub fn hir_symbol_index(project: &Project) -> crate::hir::HirSymbolIndex {
    use crate::hir::{HirDefinitionId as Id, HirDefinitionKind as Kind, HirSymbolIndex};
    let mut index = HirSymbolIndex::default();
    for module in &project.modules {
        for name in &module.function_symbols {
            index.insert(Id::new(&module.name, name, Kind::Function));
        }
        for statement in &module.statements {
            match statement {
                // Function names in `module.statements` have already been
                // rewritten for the legacy flattened AST. Index the original
                // source spelling so HIR IDs remain semantic, not backend-specific.
                Statement::Function { .. } => {},
                Statement::Struct { name, fields, .. } => {
                    index.insert(Id::new(&module.name, name, Kind::Struct));
                    for field in fields {
                        index.insert(Id::scoped(&module.name, [name.clone()], field.name.clone(), Kind::Field));
                    }
                }
                Statement::Enum { name, variants, .. } => {
                    index.insert(Id::new(&module.name, name, Kind::Enum));
                    for variant in variants {
                        index.insert(Id::scoped(&module.name, [name.clone()], variant.name.clone(), Kind::Variant));
                    }
                }
                Statement::Trait { name, methods, .. } => {
                    index.insert(Id::new(&module.name, name, Kind::Trait));
                    for method in methods {
                        index.insert(Id::scoped(&module.name, [format!("{}::{}", module.name, name)], method.name.clone(), Kind::Method));
                    }
                }
                Statement::ConstDeclaration { name, .. } => { index.insert(Id::new(&module.name, name, Kind::Constant)); }
                Statement::Impl { trait_name, type_name, methods, .. } => {
                    let trait_scope = trait_name.as_ref().map(|name| {
                        if module.statements.iter().any(|item| matches!(item, Statement::Trait { name: local, .. } if local == name)) {
                            format!("{}::{}", module.name, name)
                        } else {
                            module.imports.iter().find_map(|import| {
                                let target = resolve_import_target(&project.modules, module, &import.path)?;
                                find_exported_definition_provider(&project.modules, target, name, Kind::Trait, &mut HashSet::new())
                                    .map(|provider| format!("{}::{}", provider.name, name))
                            }).unwrap_or_else(|| name.clone())
                        }
                    }).unwrap_or_else(|| "inherent".into());
                    let receiver_scope = if module.statements.iter().any(|item| matches!(item, Statement::Struct { name: local, .. } | Statement::Enum { name: local, .. } if local == type_name)) {
                        format!("{}::{}", module.name, type_name)
                    } else {
                        module.imports.iter().find_map(|import| {
                            let target = resolve_import_target(&project.modules, module, &import.path)?;
                            find_exported_type_provider(&project.modules, target, type_name, &mut HashSet::new())
                                .map(|provider| format!("{}::{}", provider.name, type_name))
                        }).unwrap_or_else(|| type_name.clone())
                    };
                    let scope = vec![trait_scope, receiver_scope];
                    for method in methods {
                        if let Statement::Function { name, .. } = method {
                            index.insert(Id::scoped(&module.name, scope.clone(), name.clone(), Kind::Method));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    index
}



fn trait_is_visible_to_module(project: &Project, importing_module: &str, trait_id: &crate::hir::HirDefinitionId) -> bool {
    use crate::hir::HirDefinitionKind as Kind;
    if resolve_unqualified_definition(project, importing_module, &trait_id.name, Kind::Trait)
        .map(|visible| &visible == trait_id).unwrap_or(false) { return true; }
    let Some(importer) = project.modules.iter().find(|module| module.name == importing_module) else { return false; };
    importer.imports.iter().filter(|import| import.selected.is_empty()).any(|import| {
        let alias = import.alias.as_deref().unwrap_or_else(|| import.path.rsplit('.').next().unwrap_or(&import.path));
        resolve_qualified_definition(project, importing_module, alias, &trait_id.name, Kind::Trait)
            .map(|visible| &visible == trait_id).unwrap_or(false)
    })
}

/// Resolve a method against a receiver's nominal HIR identity. Inherent methods
/// take precedence; otherwise a single trait implementation may provide the
/// method. Multiple trait providers are reported as ambiguous rather than
/// selected by module traversal order.
pub fn resolve_hir_method(
    project: &Project,
    importing_module: &str,
    receiver_type_name: &str,
    method_name: &str,
) -> Result<crate::hir::HirDefinitionId, String> {
    let receiver_id = resolve_hir_type_name(project, importing_module, receiver_type_name)?
        .ok_or_else(|| format!("unknown nominal receiver type '{}'", receiver_type_name))?;
    resolve_hir_method_by_id(project, importing_module, &receiver_id, method_name)
}

/// Identity-based form used once expression propagation has resolved a receiver.
pub fn resolve_hir_method_by_id(
    project: &Project,
    importing_module: &str,
    receiver_id: &crate::hir::HirDefinitionId,
    method_name: &str,
) -> Result<crate::hir::HirDefinitionId, String> {
    use crate::hir::{HirDefinitionId as Id, HirDefinitionKind as Kind};
    let mut inherent = Vec::new();
    let mut trait_methods = Vec::new();
    for module in &project.modules {
        for statement in &module.statements {
            let Statement::Impl { trait_name, type_name, methods, .. } = statement else { continue; };
            let Some(impl_receiver) = resolve_hir_type_name(project, &module.name, type_name)? else { continue; };
            if &impl_receiver != receiver_id { continue; }
            if !methods.iter().any(|method| matches!(method, Statement::Function { name, .. } if name == method_name)) { continue; }
            if let Some(trait_name) = trait_name {
                let trait_id = if let Some((qualifier, name)) = trait_name.split_once('.') {
                    resolve_qualified_definition(project, &module.name, qualifier, name, Kind::Trait)?
                } else {
                    resolve_unqualified_definition(project, &module.name, trait_name, Kind::Trait)?
                };
                if !trait_is_visible_to_module(project, importing_module, &trait_id) { continue; }
                trait_methods.push(Id::scoped(module.name.clone(),
                    [format!("{}::{}", trait_id.module, trait_id.name), format!("{}::{}", receiver_id.module, receiver_id.name)],
                    method_name, Kind::Method));
            } else {
                inherent.push(Id::scoped(module.name.clone(),
                    ["inherent".to_string(), format!("{}::{}", receiver_id.module, receiver_id.name)],
                    method_name, Kind::Method));
            }
        }
    }
    if inherent.len() == 1 { return Ok(inherent.remove(0)); }
    if inherent.len() > 1 { return Err(format!("ambiguous inherent method '{}.{}'", receiver_id.name, method_name)); }
    match trait_methods.as_slice() {
        [method] => Ok(method.clone()),
        [] => Err(format!("type '{}.{}' has no method '{}' in the visible impl set", receiver_id.module, receiver_id.name, method_name)),
        _ => Err(format!("ambiguous trait method '{}.{}': multiple visible trait implementations provide it", receiver_id.name, method_name)),
    }
}

/// Build a backend-symbol keyed method identity table. Each method identity keeps
/// its defining module and impl scope; the receiver type is resolved through the
/// nominal type identity table rather than its source spelling.
pub fn backend_method_definition_ids(
    program: &Program,
) -> HashMap<(String, String), crate::hir::HirDefinitionId> {
    use crate::hir::{HirDefinitionId as Id, HirDefinitionKind as Kind};
    let type_ids = backend_type_definition_ids(program);
    let mut result: HashMap<(String, String), Id> = HashMap::new();
    for module in &program.modules {
        let start = module.statement_start.min(program.statements.len());
        let end = module.statement_end.min(program.statements.len()).max(start);
        for statement in &program.statements[start..end] {
            if let Statement::Impl { trait_name, type_name, methods, .. } = statement {
                let Some(receiver_id) = type_ids.get(type_name) else { continue; };
                let trait_scope = trait_name.as_ref().map(|backend_trait| {
                    program.modules.iter().find_map(|owner| {
                        owner.public_symbols.iter().chain(owner.private_symbols.iter())
                            .find(|source| mangle_trait(&owner.name, source) == backend_trait.as_str())
                            .map(|source| format!("{}::{}", owner.name, source))
                    }).unwrap_or_else(|| backend_trait.clone())
                }).unwrap_or_else(|| "inherent".into());
                let scope = vec![trait_scope, format!("{}::{}", receiver_id.module, receiver_id.name)];
                for method in methods {
                    if let Statement::Function { name, .. } = method {
                        let key = (type_name.clone(), name.clone());
                        let candidate = Id::scoped(module.name.clone(), scope.clone(), name.clone(), Kind::Method);
                        let replace = match result.get(&key) {
                            Some(existing) if existing.scope.first().map(String::as_str) == Some("inherent") => false,
                            Some(_) if candidate.scope.first().map(String::as_str) != Some("inherent") => false,
                            _ => true,
                        };
                        if replace { result.insert(key, candidate); }
                    }
                }
            }
        }
    }
    result
}

/// Recover trait declaration identities from module-qualified compatibility symbols.
pub fn backend_trait_definition_ids(program: &Program) -> HashMap<String, crate::hir::HirDefinitionId> {
    let mut result = HashMap::new();
    for module in &program.modules {
        let start = module.statement_start.min(program.statements.len());
        let end = module.statement_end.min(program.statements.len()).max(start);
        for statement in &program.statements[start..end] {
            if let Statement::Trait { name: backend_name, .. } = statement {
                if let Some(source_name) = module.public_symbols.iter().chain(module.private_symbols.iter())
                    .find(|source| mangle_trait(&module.name, source) == backend_name.as_str()) {
                    result.insert(backend_name.clone(), crate::hir::HirDefinitionId::new(module.name.clone(), source_name.clone(), crate::hir::HirDefinitionKind::Trait));
                }
            }
        }
    }
    result
}

/// Recover constant declaration identities from module-qualified symbols.
pub fn backend_constant_definition_ids(program: &Program) -> HashMap<String, crate::hir::HirDefinitionId> {
    let mut result = HashMap::new();
    for module in &program.modules {
        let start = module.statement_start.min(program.statements.len());
        let end = module.statement_end.min(program.statements.len()).max(start);
        for statement in &program.statements[start..end] {
            if let Statement::ConstDeclaration { name: backend_name, .. } = statement {
                if let Some(source_name) = module.public_symbols.iter().chain(module.private_symbols.iter())
                    .find(|source| mangle_constant(&module.name, source) == backend_name.as_str()) {
                    result.insert(backend_name.clone(), crate::hir::HirDefinitionId::new(module.name.clone(), source_name.clone(), crate::hir::HirDefinitionKind::Constant));
                }
            }
        }
    }
    result
}

/// Build HIR identities for struct fields and enum variants in the lowered AST.
pub fn backend_member_definition_ids(program: &Program) -> (
    HashMap<(String, String), crate::hir::HirDefinitionId>,
    HashMap<(String, String), crate::hir::HirDefinitionId>,
) {
    use crate::hir::{HirDefinitionId as Id, HirDefinitionKind as Kind};
    let type_ids = backend_type_definition_ids(program);
    let mut fields: HashMap<(String, String), Id> = HashMap::new();
    let mut variants: HashMap<(String, String), Id> = HashMap::new();
    for module in &program.modules {
        let start = module.statement_start.min(program.statements.len());
        let end = module.statement_end.min(program.statements.len()).max(start);
        for statement in &program.statements[start..end] {
            match statement {
                Statement::Struct { name, fields: declarations, .. } => {
                    if let Some(owner) = type_ids.get(name) {
                        for field in declarations {
                            fields.insert((name.clone(), field.name.clone()), Id::scoped(
                                owner.module.clone(), [owner.name.clone()], field.name.clone(), Kind::Field,
                            ));
                        }
                    }
                }
                Statement::Enum { name, variants: declarations, .. } => {
                    if let Some(owner) = type_ids.get(name) {
                        for variant in declarations {
                            variants.insert((name.clone(), variant.name.clone()), Id::scoped(
                                owner.module.clone(), [owner.name.clone()], variant.name.clone(), Kind::Variant,
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (fields, variants)
}

/// Resolve an enum variant to the identity of its declaring enum. The variant
/// spelling is intentionally scoped beneath the enum and is never globally keyed.
pub fn backend_enum_variant_definition_id(
    program: &Program,
    enum_symbol: &str,
    variant_name: &str,
) -> Option<(crate::hir::HirDefinitionId, String)> {
    let type_ids = backend_type_definition_ids(program);
    let enum_id = type_ids.get(enum_symbol)?.clone();
    for module in &program.modules {
        let start = module.statement_start.min(program.statements.len());
        let end = module.statement_end.min(program.statements.len()).max(start);
        for statement in &program.statements[start..end] {
            if let Statement::Enum { name, variants, .. } = statement {
                if name == enum_symbol && variants.iter().any(|variant| variant.name == variant_name) {
                    return Some((enum_id, variant_name.to_string()));
                }
            }
        }
    }
    None
}

/// Validate every trait implementation against the module graph. Trait names
/// resolve in the impl's owning module, and receiver types resolve to nominal
/// HIR identities. This prevents a same-named trait in another module from
/// accidentally satisfying an implementation.
fn nominal_or_spelling_type_matches(
    project: &Project, expected_module: &str, expected: &str,
    actual_module: &str, actual: &str, receiver_id: &crate::hir::HirDefinitionId,
) -> bool {
    if expected == "Self" {
        return resolve_hir_type_name(project, actual_module, actual).ok().flatten().as_ref() == Some(receiver_id);
    }
    if actual == "Self" {
        return resolve_hir_type_name(project, expected_module, expected).ok().flatten().as_ref() == Some(receiver_id);
    }
    let expected_structural = resolve_hir_type_spelling(project, expected_module, Some(expected), &[]).ok();
    let actual_structural = resolve_hir_type_spelling(project, actual_module, Some(actual), &[]).ok();
    match (expected_structural, actual_structural) {
        (Some(expected), Some(actual)) => expected == actual,
        _ => {
            let expected_id = resolve_hir_type_name(project, expected_module, expected).ok().flatten();
            let actual_id = resolve_hir_type_name(project, actual_module, actual).ok().flatten();
            match (expected_id, actual_id) {
                (Some(expected), Some(actual)) => expected == actual,
                (None, None) => expected == actual,
                _ => false,
            }
        }
    }
}

fn project_has_inherent_method(project: &Project, receiver_id: &crate::hir::HirDefinitionId, method_name: &str) -> Result<bool, String> {
    for module in &project.modules {
        for statement in &module.statements {
            if let Statement::Impl { trait_name: None, type_name, methods, .. } = statement {
                if resolve_hir_type_name(project, &module.name, type_name)?.as_ref() == Some(receiver_id)
                    && methods.iter().any(|method| matches!(method, Statement::Function { name, .. } if name == method_name)) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

pub fn validate_hir_trait_implementations(project: &Project) -> Result<Vec<crate::hir::HirDefinitionId>, String> {
    use crate::hir::{HirDefinitionId as Id, HirDefinitionKind as Kind};
    let mut resolved = Vec::new();
    let mut implemented_pairs = HashSet::new();
    let mut trait_method_owners: HashMap<(crate::hir::HirDefinitionId, String), crate::hir::HirDefinitionId> = HashMap::new();
    for module in &project.modules {
        for statement in &module.statements {
            if let Statement::Impl { trait_name: Some(trait_name), type_name, methods, .. } = statement {
                let trait_id = if let Some((qualifier, name)) = trait_name.split_once('.') {
                    resolve_qualified_definition(project, &module.name, qualifier, name, Kind::Trait)?
                } else {
                    resolve_unqualified_definition(project, &module.name, trait_name, Kind::Trait)?
                };
                let receiver_id = resolve_hir_type_name(project, &module.name, type_name)?
                    .ok_or_else(|| format!("impl receiver '{}' in module '{}' is not a declared nominal type", type_name, module.name))?;
                if !implemented_pairs.insert((trait_id.clone(), receiver_id.clone())) {
                    return Err(format!("duplicate implementation of trait '{}.{}' for '{}.{}'", trait_id.module, trait_id.name, receiver_id.module, receiver_id.name));
                }
                let trait_decl = project.modules.iter().find(|candidate| candidate.name == trait_id.module)
                    .and_then(|owner| owner.statements.iter().find_map(|candidate| match candidate {
                        Statement::Trait { name, methods, .. } if name == &trait_id.name => Some(methods), _ => None,
                    })).ok_or_else(|| format!("resolved trait '{}.{}' has no declaration", trait_id.module, trait_id.name))?;
                for required in trait_decl {
                    // `self` is an ABI receiver, not a user-visible trait argument.
                    // Accept both implicit receivers (`fn read()`) and explicit
                    // receiver spelling (`fn read(self)`) in both trait contracts
                    // and implementation bodies.
                    let required_receiver = required.parameters.first().filter(|parameter| parameter.name == "self");
                    let expected_parameters = if required_receiver.is_some() { &required.parameters[1..] } else { required.parameters.as_slice() };
                    if let Some(receiver) = required_receiver {
                        if let Some(receiver_type) = receiver.type_name.as_deref() {
                            if receiver_type != "Self"
                                && resolve_hir_type_name(project, &trait_id.module, receiver_type)?.as_ref() != Some(&receiver_id) {
                                return Err(format!("trait '{}.{}' method '{}' declares a receiver type incompatible with '{}.{}'", trait_id.module, trait_id.name, required.name, receiver_id.module, receiver_id.name));
                            }
                        }
                    }
                    let mut implementation = None;
                    for method in methods {
                        let Statement::Function {
                            name, generic_parameters, parameters, return_type,
                            is_async, ..
                        } = method else { continue; };
                        if name != &required.name { continue; }
                        let explicit_self = parameters.first().filter(|parameter| parameter.name == "self");
                        let user_parameters = if explicit_self.is_some() { &parameters[1..] } else { parameters.as_slice() };
                        if !generic_parameters.is_empty() || *is_async || user_parameters.len() != expected_parameters.len() {
                            continue;
                        }
                        if let Some(receiver) = explicit_self {
                            if let Some(receiver_type) = receiver.type_name.as_deref() {
                                if receiver_type != "Self"
                                    && resolve_hir_type_name(project, &module.name, receiver_type)?.as_ref() != Some(&receiver_id) {
                                    continue;
                                }
                            }
                        }
                        let parameters_match = expected_parameters.iter().zip(user_parameters).all(|(expected, actual)| {
                            match (expected.type_name.as_deref(), actual.type_name.as_deref()) {
                                (Some(expected_type), Some(actual_type)) => nominal_or_spelling_type_matches(
                                    project, &trait_id.module, expected_type, &module.name, actual_type, &receiver_id,
                                ),
                                (None, None) => true,
                                _ => false,
                            }
                        });
                        let returns_match = match (required.return_type.as_deref(), return_type.as_deref()) {
                            (Some(expected), Some(actual)) => nominal_or_spelling_type_matches(project, &trait_id.module, expected, &module.name, actual, &receiver_id),
                            (None, None) => true,
                            _ => false,
                        };
                        if parameters_match && returns_match {
                            implementation = Some(());
                            break;
                        }
                    }
                    if implementation.is_none() {
                        return Err(format!("impl of trait '{}.{}' for '{}.{}' must provide method '{}' with matching non-receiver parameters and return type (trait methods currently require non-generic, synchronous implementations)", trait_id.module, trait_id.name, receiver_id.module, receiver_id.name, required.name));
                    }
                    let method_key = (receiver_id.clone(), required.name.clone());
                    if let Some(previous_trait) = trait_method_owners.insert(method_key, trait_id.clone()) {
                        if previous_trait != trait_id && !project_has_inherent_method(project, &receiver_id, &required.name)? {
                            return Err(format!("ambiguous trait method '{}.{}': both '{}.{}' and '{}.{}' implement it; add an inherent method to select a dispatch target", receiver_id.module, receiver_id.name, previous_trait.module, previous_trait.name, trait_id.module, trait_id.name));
                        }
                    }
                }
                resolved.push(Id::scoped(module.name.clone(), [trait_id.module.clone(), trait_id.name.clone(), receiver_id.module.clone(), receiver_id.name.clone()], "impl", Kind::Method));
            }
        }
    }
    Ok(resolved)
}

/// Build module-qualified struct/enum metadata without flattening declarations
/// into the legacy type checker's name-keyed maps.
pub fn hir_type_registry(project: &Project) -> crate::hir::HirTypeRegistry {
    use crate::hir::{
        HirDefinitionId as Id, HirDefinitionKind as Kind, HirTypeDefinition,
        HirTypeField, HirTypeRegistry, HirTypeShape, HirTypeVariant,
    };
    let mut registry = HirTypeRegistry::default();
    for module in &project.modules {
        for statement in &module.statements {
            let definition = match statement {
                Statement::Struct { name, fields, derives, .. } => Some(HirTypeDefinition {
                    id: Id::new(&module.name, name, Kind::Struct),
                    is_public: module.local_public_symbols.contains(name),
                    shape: HirTypeShape::Struct {
                        fields: fields.iter().map(|field| HirTypeField {
                            name: field.name.clone(),
                            type_name: field.type_name.clone(),
                        }).collect(),
                        derives: derives.clone(),
                    },
                }),
                Statement::Enum { name, variants, .. } => Some(HirTypeDefinition {
                    id: Id::new(&module.name, name, Kind::Enum),
                    is_public: module.local_public_symbols.contains(name),
                    shape: HirTypeShape::Enum {
                        variants: variants.iter().map(|variant| HirTypeVariant {
                            name: variant.name.clone(),
                            fields: variant.fields.clone(),
                        }).collect(),
                    },
                }),
                _ => None,
            };
            if let Some(definition) = definition {
                registry.insert(definition);
            }
        }
    }
    registry
}


/// Resolve an unqualified declaration reference in a module.
///
/// Resolution order is deliberately explicit:
/// 1. A declaration in the current module wins (including private declarations).
/// 2. A matching selective import is considered only when the target exports it.
/// 3. Multiple imported providers are an error rather than an arbitrary choice.
///
/// Whole-module imports do not inject names into the unqualified namespace; use
/// `resolve_qualified_definition` for those. The returned ID always names the
/// module that physically owns the declaration, not an intermediate re-export.
pub fn resolve_unqualified_definition(
    project: &Project,
    importing_module: &str,
    name: &str,
    kind: crate::hir::HirDefinitionKind,
) -> Result<crate::hir::HirDefinitionId, String> {
    use crate::hir::{HirDefinitionId as Id, HirDefinitionKind as Kind};

    let importer = project.modules.iter()
        .find(|module| module.name == importing_module)
        .ok_or_else(|| format!("unknown importing module '{}'", importing_module))?;

    let declares_locally = match kind {
        Kind::Function => importer.function_symbols.contains(name),
        Kind::Struct => importer.statements.iter().any(|s| matches!(s, Statement::Struct { name: n, .. } if n == name)),
        Kind::Enum => importer.statements.iter().any(|s| matches!(s, Statement::Enum { name: n, .. } if n == name)),
        Kind::Trait => importer.statements.iter().any(|s| matches!(s, Statement::Trait { name: n, .. } if n == name)),
        Kind::Constant => importer.statements.iter().any(|s| matches!(s, Statement::ConstDeclaration { name: n, .. } if n == name)),
        Kind::Method | Kind::Field | Kind::Variant => false,
    };
    if declares_locally {
        let id = Id::new(importing_module, name, kind);
        return if hir_symbol_index(project).contains(&id) { Ok(id) }
            else { Err(format!("internal module index is missing local {:?} '{}'", kind, name)) };
    }

    let mut providers = Vec::new();
    let mut private_provider = None;
    for import in importer.imports.iter().filter(|import| import.selected.iter().any(|item| item == name)) {
        let Some(target) = resolve_import_target(&project.modules, importer, &import.path) else { continue; };
        if let Some(provider) = find_exported_definition_provider(
            &project.modules, target, name, kind, &mut HashSet::new(),
        ) {
            providers.push(Id::new(&provider.name, name, kind));
        } else if target.private_symbols.contains(name) {
            private_provider = Some(target.name.clone());
        }
    }
    providers.sort();
    providers.dedup();
    match providers.as_slice() {
        [id] => {
            if hir_symbol_index(project).contains(id) { Ok(id.clone()) }
            else { Err(format!("internal module index is missing imported {:?} '{}'", kind, name)) }
        }
        [] => if let Some(module) = private_provider {
            Err(format!("cannot import private symbol '{}' from module '{}'", name, module))
        } else {
            Err(format!("'{}' is not declared locally or selectively imported into module '{}'", name, importing_module))
        },
        _ => Err(format!("ambiguous imported {:?} '{}': it is provided by {}; qualify the reference or import only one provider", kind, name,
            providers.iter().map(|id| id.module.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// Resolve a qualified source-level name through the import table of an importing
/// module and return its stable HIR identity. The lookup follows public re-exports
/// to the module that actually owns the declaration; aliases are used only for
/// lookup and never become part of the resulting identity.
///
/// This is the semantic lookup primitive for the HIR migration. It deliberately
/// does not consult compatibility-AST mangled names.
pub fn resolve_qualified_definition(
    project: &Project,
    importing_module: &str,
    qualifier: &str,
    name: &str,
    kind: crate::hir::HirDefinitionKind,
) -> Result<crate::hir::HirDefinitionId, String> {
    let importer = project.modules.iter()
        .find(|module| module.name == importing_module)
        .ok_or_else(|| format!("unknown importing module '{}'", importing_module))?;

    let import = importer.imports.iter().find(|import| {
        if !import.selected.is_empty() { return false; }
        let default_alias = import.path.rsplit('.').next().unwrap_or(&import.path);
        import.alias.as_deref().unwrap_or(default_alias) == qualifier
    }).ok_or_else(|| format!("module '{}' has no import available under qualifier '{}'", importing_module, qualifier))?;

    let target = resolve_import_target(&project.modules, importer, &import.path)
        .ok_or_else(|| format!("cannot resolve imported module '{}'", import.path))?;
    let provider = find_exported_definition_provider(
        &project.modules,
        target,
        name,
        kind,
        &mut HashSet::new(),
    ).ok_or_else(|| {
        if target.public_symbols.contains(name) {
            format!("module '{}' does not export a {:?} named '{}'", target.name, kind, name)
        } else {
            format!("'{}.{}' is private or does not exist", qualifier, name)
        }
    })?;

    let id = crate::hir::HirDefinitionId::new(&provider.name, name, kind);
    let index = hir_symbol_index(project);
    if index.contains(&id) { Ok(id) } else {
        Err(format!("internal module index is missing {:?} definition '{}.{}'", kind, provider.name, name))
    }
}

/// Resolve a source type name to its module-qualified HIR declaration identity.
///
/// Built-in and generic/container type spellings are not declarations in the
/// module graph and therefore return `Ok(None)`. A dotted name is interpreted
/// as `import_alias.Type`; an unqualified name uses local declarations followed
/// by selective imports. If the same spelling resolves as both a struct and an
/// enum, the caller must provide a more specific type context rather than have
/// this layer choose arbitrarily.
pub fn resolve_hir_type_name(
    project: &Project,
    importing_module: &str,
    source_name: &str,
) -> Result<Option<crate::hir::HirDefinitionId>, String> {
    use crate::hir::HirDefinitionKind::{Enum, Struct};

    let name = source_name.trim();
    if name.is_empty() || name.contains('<') || name.contains('[') || name.contains('(') {
        return Ok(None);
    }

    let candidates = if let Some((qualifier, member)) = name.split_once('.') {
        if qualifier.is_empty() || member.is_empty() || member.contains('.') {
            return Err(format!("unsupported qualified type name '{}'; expected import_alias.Type", name));
        }
        let mut found = Vec::new();
        for kind in [Struct, Enum] {
            match resolve_qualified_definition(project, importing_module, qualifier, member, kind) {
                Ok(id) => found.push(id),
                Err(error) if error.contains("private or does not exist")
                    || error.contains("does not export") => {},
                Err(error) if error.contains("no import available") => return Err(error),
                Err(error) => return Err(error),
            }
        }
        found
    } else {
        let mut found = Vec::new();
        for kind in [Struct, Enum] {
            match resolve_unqualified_definition(project, importing_module, name, kind) {
                Ok(id) => found.push(id),
                Err(error) if error.contains("not declared locally or selectively imported") => {},
                Err(error) if error.contains("private symbol") => return Err(error),
                Err(error) => return Err(error),
            }
        }
        found
    };

    match candidates.as_slice() {
        [id] => Ok(Some(id.clone())),
        [] => Ok(None),
        _ => Err(format!("type name '{}' is ambiguous between struct and enum declarations", name)),
    }
}


/// Validate every user-defined type reference stored in struct fields and enum
/// payloads. This is the first project-level semantic pass that consumes the
/// HIR registry and module resolver together; it deliberately does not mutate
/// the compatibility AST or claim to type-check expressions/constructors.
pub fn validate_hir_type_references(project: &Project) -> Result<(), String> {
    let registry = hir_type_registry(project);
    for definition in registry.definitions.values() {
        let mut references: Vec<(String, String)> = Vec::new();
        match &definition.shape {
            crate::hir::HirTypeShape::Struct { fields, .. } => {
                for field in fields {
                    references.push((field.name.clone(), field.type_name.clone()));
                }
            }
            crate::hir::HirTypeShape::Enum { variants } => {
                for variant in variants {
                    for (index, payload_type) in variant.fields.iter().enumerate() {
                        references.push((format!("{}[{}]", variant.name, index), payload_type.clone()));
                    }
                }
            }
        }

        for (member, type_name) in references {
            validate_hir_type_spelling(project, &definition.id.module, &type_name)
                .map_err(|error| format!(
                    "in module '{}', type '{}', member '{}': {}",
                    definition.id.module, definition.id.name, member, error
                ))?;
        }
    }
    Ok(())
}

fn validate_hir_type_spelling(project: &Project, module: &str, spelling: &str) -> Result<(), String> {
    let name = spelling.trim();
    if name.is_empty() {
        return Err("empty type name".into());
    }

    const BUILTINS: &[&str] = &[
        "num", "float", "bool", "string", "void", "unknown", "File", "TcpStream",
        "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128",
        "isize", "usize", "f32", "f64",
    ];
    if BUILTINS.contains(&name) {
        return Ok(());
    }

    if let Some(inner) = name.strip_suffix("[]") {
        return validate_hir_type_spelling(project, module, inner);
    }

    if name.starts_with('(') && name.ends_with(')') {
        let parts = split_hir_type_arguments(&name[1..name.len() - 1]);
        if parts.len() < 2 {
            return Err(format!("tuple type '{}' must contain at least two elements", name));
        }
        for part in parts { validate_hir_type_spelling(project, module, part)?; }
        return Ok(());
    }

    if let Some(open) = name.find('<') {
        if !name.ends_with('>') {
            return Err(format!("malformed generic type '{}'", name));
        }
        let constructor = &name[..open];
        let args = split_hir_type_arguments(&name[open + 1..name.len() - 1]);
        let expected = match constructor {
            "Array" | "Iterator" | "Option" | "Task" => 1,
            "HashMap" | "Result" => 2,
            _ => return Err(format!("unsupported generic type constructor '{}'", constructor)),
        };
        if args.len() != expected {
            return Err(format!("{} requires {} type argument(s), found {}", constructor, expected, args.len()));
        }
        for arg in args { validate_hir_type_spelling(project, module, arg)?; }
        return Ok(());
    }

    match resolve_hir_type_name(project, module, name)? {
        Some(_) => Ok(()),
        None => Err(format!("unknown type '{}'", name)),
    }
}

fn split_hir_type_arguments(source: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut angle_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in source.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            ',' if angle_depth == 0 && paren_depth == 0 => {
                parts.push(source[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = source[start..].trim();
    if !tail.is_empty() { parts.push(tail); }
    parts
}


/// Resolve a struct field through the module-aware type registry.
///
/// The returned definition ID identifies the declaring struct, while the field
/// metadata preserves the source-level type spelling for a later type-checker
/// pass. Private imported types are rejected by `resolve_hir_type_name` before
/// their fields can be inspected.
pub fn resolve_hir_struct_field(
    project: &Project,
    importing_module: &str,
    source_type_name: &str,
    field_name: &str,
) -> Result<(crate::hir::HirDefinitionId, crate::hir::HirTypeField), String> {
    use crate::hir::{HirDefinitionKind, HirTypeShape};

    let id = resolve_hir_type_name(project, importing_module, source_type_name)?
        .ok_or_else(|| format!("unknown struct type '{}'", source_type_name))?;
    if id.kind != HirDefinitionKind::Struct {
        return Err(format!("'{}' resolves to an enum, not a struct", source_type_name));
    }
    let registry = hir_type_registry(project);
    let definition = registry.get(&id)
        .ok_or_else(|| format!("internal HIR type registry is missing '{}.{}'", id.module, id.name))?;
    let HirTypeShape::Struct { fields, .. } = &definition.shape else {
        return Err(format!("internal HIR kind mismatch for '{}.{}'", id.module, id.name));
    };
    let field = fields.iter().find(|field| field.name == field_name)
        .ok_or_else(|| format!("struct '{}.{}' has no field '{}'", id.module, id.name, field_name))?;
    Ok((id, field.clone()))
}

/// Resolve an enum variant through the module-aware type registry.
///
/// The returned field type spellings are kept in declaration order so the
/// eventual constructor/pattern checker can validate payload arity and types.
pub fn resolve_hir_enum_variant(
    project: &Project,
    importing_module: &str,
    source_type_name: &str,
    variant_name: &str,
) -> Result<(crate::hir::HirDefinitionId, crate::hir::HirTypeVariant), String> {
    use crate::hir::{HirDefinitionKind, HirTypeShape};

    let id = resolve_hir_type_name(project, importing_module, source_type_name)?
        .ok_or_else(|| format!("unknown enum type '{}'", source_type_name))?;
    if id.kind != HirDefinitionKind::Enum {
        return Err(format!("'{}' resolves to a struct, not an enum", source_type_name));
    }
    let registry = hir_type_registry(project);
    let definition = registry.get(&id)
        .ok_or_else(|| format!("internal HIR type registry is missing '{}.{}'", id.module, id.name))?;
    let HirTypeShape::Enum { variants } = &definition.shape else {
        return Err(format!("internal HIR kind mismatch for '{}.{}'", id.module, id.name));
    };
    let variant = variants.iter().find(|variant| variant.name == variant_name)
        .ok_or_else(|| format!("enum '{}.{}' has no variant '{}'", id.module, id.name, variant_name))?;
    Ok((id, variant.clone()))
}


/// A constructor expression resolved against the module-aware HIR type registry.
/// The span is file-local, so the module name is part of the identity key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirResolvedTypeExpression {
    pub module: String,
    pub span: crate::span::Span,
    pub definition: crate::hir::HirDefinitionId,
    pub kind: HirResolvedTypeExpressionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirResolvedTypeExpressionKind {
    Struct { supplied_fields: Vec<String> },
    EnumVariant { variant: String, payload_types: Vec<String> },
}

/// Resolve and structurally validate every explicit struct/enum constructor in
/// every source module. This is a module-aware semantic side table: it records
/// stable declaration identities without mutating the compatibility AST or
/// pretending that the legacy type checker has already migrated to HIR IDs.
pub fn resolve_hir_type_expressions(
    project: &Project,
) -> Result<Vec<HirResolvedTypeExpression>, String> {
    let mut resolved = Vec::new();
    for module in &project.modules {
        for statement in &module.statements {
            resolve_statement_type_expressions(project, &module.name, statement, &mut resolved)?;
        }
    }
    Ok(resolved)
}

fn resolve_statement_type_expressions(
    project: &Project,
    module: &str,
    statement: &Statement,
    output: &mut Vec<HirResolvedTypeExpression>,
) -> Result<(), String> {
    use crate::ast::Statement as S;
    match statement {
        S::VariableDeclarations { declarations, .. } => for declaration in declarations {
            if let Some(value) = &declaration.value { resolve_expression_type_expressions(project, module, value, output)?; }
        },
        S::ConstDeclaration { value, .. } => resolve_expression_type_expressions(project, module, value, output)?,
        S::Assignment { target, value, .. } => {
            resolve_expression_type_expressions(project, module, target, output)?;
            resolve_expression_type_expressions(project, module, value, output)?;
        }
        S::Function { body, .. } | S::Main { body, .. } => for child in body {
            resolve_statement_type_expressions(project, module, child, output)?;
        },
        S::Impl { methods, .. } => for child in methods {
            resolve_statement_type_expressions(project, module, child, output)?;
        },
        S::Match { expression, arms, .. } => {
            resolve_expression_type_expressions(project, module, expression, output)?;
            for arm in arms {
                if let Some(guard) = &arm.guard { resolve_expression_type_expressions(project, module, guard, output)?; }
                for child in &arm.body { resolve_statement_type_expressions(project, module, child, output)?; }
            }
        }
        S::Defer { expression, .. } | S::Expression { expression, .. } | S::Call { expression, .. } => {
            resolve_expression_type_expressions(project, module, expression, output)?;
        }
        S::If { condition, body, else_body, .. } => {
            resolve_expression_type_expressions(project, module, condition, output)?;
            for child in body { resolve_statement_type_expressions(project, module, child, output)?; }
            if let Some(body) = else_body { for child in body { resolve_statement_type_expressions(project, module, child, output)?; } }
        }
        S::While { condition, body, .. } => {
            resolve_expression_type_expressions(project, module, condition, output)?;
            for child in body { resolve_statement_type_expressions(project, module, child, output)?; }
        }
        S::For { start, end, body, .. } => {
            resolve_expression_type_expressions(project, module, start, output)?;
            resolve_expression_type_expressions(project, module, end, output)?;
            for child in body { resolve_statement_type_expressions(project, module, child, output)?; }
        }
        S::ForEach { iterable, body, .. } => {
            resolve_expression_type_expressions(project, module, iterable, output)?;
            for child in body { resolve_statement_type_expressions(project, module, child, output)?; }
        }
        S::Return { value: Some(value), .. } => resolve_expression_type_expressions(project, module, value, output)?,
        S::Trait { .. } | S::Struct { .. } | S::Enum { .. } | S::Import { .. }
        | S::Return { value: None, .. } | S::Break { .. } | S::Continue { .. } => {}
    }
    Ok(())
}

fn resolve_expression_type_expressions(
    project: &Project,
    module: &str,
    expression: &crate::ast::Expression,
    output: &mut Vec<HirResolvedTypeExpression>,
) -> Result<(), String> {
    use crate::ast::Expression as E;
    match expression {
        E::StructConstructor { name, fields, span } => {
            let definition = resolve_hir_type_name(project, module, name)?
                .ok_or_else(|| format!("in module '{}': unknown struct type '{}'", module, name))?;
            if definition.kind != crate::hir::HirDefinitionKind::Struct {
                return Err(format!("in module '{}': '{}' resolves to an enum, not a struct", module, name));
            }
            let registry = hir_type_registry(project);
            let definition_data = registry.get(&definition).ok_or_else(|| format!("missing HIR type '{}.{}'", definition.module, definition.name))?;
            let crate::hir::HirTypeShape::Struct { fields: declared, .. } = &definition_data.shape else {
                return Err(format!("'{}' is not a struct", name));
            };
            let mut supplied = HashSet::new();
            for (field_name, value) in fields {
                if !supplied.insert(field_name.clone()) {
                    return Err(format!("in module '{}': duplicate field '{}' in constructor '{}', at {}..{}", module, field_name, name, span.start, span.end));
                }
                if !declared.iter().any(|field| field.name == *field_name) {
                    return Err(format!("in module '{}': struct '{}.{}' has no field '{}'", module, definition.module, definition.name, field_name));
                }
                resolve_expression_type_expressions(project, module, value, output)?;
            }
            for field in declared {
                if !supplied.contains(&field.name) {
                    return Err(format!("in module '{}': missing field '{}' in constructor '{}.{}'", module, field.name, definition.module, definition.name));
                }
            }
            output.push(HirResolvedTypeExpression {
                module: module.to_string(), span: *span, definition,
                kind: HirResolvedTypeExpressionKind::Struct { supplied_fields: fields.iter().map(|(name, _)| name.clone()).collect() },
            });
        }
        E::EnumConstructor { enum_name, variant, arguments, span } => {
            let (definition, variant_data) = resolve_hir_enum_variant(project, module, enum_name, variant)
                .map_err(|error| format!("in module '{}': {}", module, error))?;
            if arguments.len() != variant_data.fields.len() {
                return Err(format!("in module '{}': enum '{}.{}::{}' expects {} payload value(s), found {}", module, definition.module, definition.name, variant, variant_data.fields.len(), arguments.len()));
            }
            for argument in arguments { resolve_expression_type_expressions(project, module, argument, output)?; }
            output.push(HirResolvedTypeExpression {
                module: module.to_string(), span: *span, definition,
                kind: HirResolvedTypeExpressionKind::EnumVariant { variant: variant.clone(), payload_types: variant_data.fields },
            });
        }
        E::Array { elements, .. } | E::Tuple { elements, .. } => for item in elements {
            resolve_expression_type_expressions(project, module, item, output)?;
        },
        E::Index { array, index, .. } | E::Binary { left: array, right: index, .. } => {
            resolve_expression_type_expressions(project, module, array, output)?;
            resolve_expression_type_expressions(project, module, index, output)?;
        }
        E::Property { object, .. } | E::Await { expression: object, .. } | E::Unary { expression: object, .. } => {
            resolve_expression_type_expressions(project, module, object, output)?;
        }
        E::MethodCall { object, arguments, .. } => {
            resolve_expression_type_expressions(project, module, object, output)?;
            for argument in arguments { resolve_expression_type_expressions(project, module, argument, output)?; }
        }
        E::Call { arguments, .. } => for argument in arguments {
            resolve_expression_type_expressions(project, module, argument, output)?;
        },
        E::Conversion { expression, .. } => resolve_expression_type_expressions(project, module, expression, output)?,
        E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. }
        | E::String { .. } | E::Identifier { .. } => {}
    }
    Ok(())
}


/// A field access whose receiver type was established from a constructor or a
/// local variable initialized from one. This deliberately records only proven
/// receiver types; it does not guess types from arbitrary expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirResolvedFieldAccess {
    pub module: String,
    pub span: crate::span::Span,
    pub definition: crate::hir::HirDefinitionId,
    pub field: crate::hir::HirTypeField,
}

/// Resolve enum/struct patterns against the enum or struct type inferred from
/// the match scrutinee. Qualified pattern names are also checked directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirResolvedPattern {
    pub module: String,
    pub span: crate::span::Span,
    pub definition: crate::hir::HirDefinitionId,
    pub kind: HirResolvedPatternKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirResolvedPatternKind {
    EnumVariant { variant: String, binding_count: usize, payload_types: Vec<String> },
    Struct { fields: Vec<String>, has_rest: bool },
}

/// Resolve field accesses for expressions whose receiver type is known from a
/// local constructor initializer. Variables are tracked within each lexical
/// statement block; branches receive cloned environments so branch-local
/// assignments cannot leak into sibling branches.
pub fn resolve_hir_field_accesses(project: &Project) -> Result<Vec<HirResolvedFieldAccess>, String> {
    let mut output = Vec::new();
    for module in &project.modules {
        let mut environment = HashMap::<String, crate::hir::HirDefinitionId>::new();
        resolve_field_accesses_in_statements(project, &module.name, &module.statements, &mut environment, &mut output)?;
    }
    Ok(output)
}

fn resolve_field_accesses_in_statements(
    project: &Project, module: &str, statements: &[Statement],
    environment: &mut HashMap<String, crate::hir::HirDefinitionId>,
    output: &mut Vec<HirResolvedFieldAccess>,
) -> Result<(), String> {
    use crate::ast::Statement as S;
    for statement in statements {
        match statement {
            S::VariableDeclarations { declarations, .. } => for declaration in declarations {
                if let Some(value) = &declaration.value {
                    resolve_field_accesses_in_expression(project, module, value, environment, output)?;
                    if let Some(id) = infer_constructor_definition(project, module, value)? {
                        environment.insert(declaration.name.clone(), id);
                    } else if declaration.declared_type.is_none() {
                        environment.remove(&declaration.name);
                    }
                }
            },
            S::ConstDeclaration { name, declared_type, value, .. } => {
                resolve_field_accesses_in_expression(project, module, value, environment, output)?;
                if let Some(id) = infer_constructor_definition(project, module, value)? { environment.insert(name.clone(), id); }
                else if declared_type.is_none() { environment.remove(name); }
            }
            S::Assignment { target, value, .. } => {
                resolve_field_accesses_in_expression(project, module, target, environment, output)?;
                resolve_field_accesses_in_expression(project, module, value, environment, output)?;
                if let crate::ast::Expression::Identifier { name, .. } = target {
                    if let Some(id) = infer_constructor_definition(project, module, value)? { environment.insert(name.clone(), id); }
                    else { environment.remove(name); }
                }
            }
            S::Function { body, .. } | S::Main { body, .. } => {
                let mut nested = HashMap::new();
                resolve_field_accesses_in_statements(project, module, body, &mut nested, output)?;
            }
            S::Impl { methods, .. } => for method in methods {
                resolve_field_accesses_in_statements(project, module, std::slice::from_ref(method), &mut HashMap::new(), output)?;
            },
            S::Match { expression, arms, .. } => {
                resolve_field_accesses_in_expression(project, module, expression, environment, output)?;
                for arm in arms {
                    if let Some(guard) = &arm.guard { resolve_field_accesses_in_expression(project, module, guard, environment, output)?; }
                    let mut nested = environment.clone();
                    resolve_field_accesses_in_statements(project, module, &arm.body, &mut nested, output)?;
                }
            }
            S::If { condition, body, else_body, .. } => {
                resolve_field_accesses_in_expression(project, module, condition, environment, output)?;
                let mut nested = environment.clone();
                resolve_field_accesses_in_statements(project, module, body, &mut nested, output)?;
                if let Some(body) = else_body { let mut nested = environment.clone(); resolve_field_accesses_in_statements(project, module, body, &mut nested, output)?; }
            }
            S::While { condition, body, .. } => {
                resolve_field_accesses_in_expression(project, module, condition, environment, output)?;
                let mut nested = environment.clone(); resolve_field_accesses_in_statements(project, module, body, &mut nested, output)?;
            }
            S::For { start, end, body, .. } => {
                resolve_field_accesses_in_expression(project, module, start, environment, output)?;
                resolve_field_accesses_in_expression(project, module, end, environment, output)?;
                let mut nested = environment.clone(); resolve_field_accesses_in_statements(project, module, body, &mut nested, output)?;
            }
            S::ForEach { iterable, body, .. } => {
                resolve_field_accesses_in_expression(project, module, iterable, environment, output)?;
                let mut nested = environment.clone(); resolve_field_accesses_in_statements(project, module, body, &mut nested, output)?;
            }
            S::Return { value: Some(value), .. } | S::Defer { expression: value, .. }
            | S::Expression { expression: value, .. } | S::Call { expression: value, .. } => {
                resolve_field_accesses_in_expression(project, module, value, environment, output)?;
            }
            S::Trait { .. } | S::Struct { .. } | S::Enum { .. } | S::Import { .. }
            | S::Return { value: None, .. } | S::Break { .. } | S::Continue { .. } => {}
        }
    }
    Ok(())
}

fn infer_constructor_definition(project: &Project, module: &str, expression: &crate::ast::Expression) -> Result<Option<crate::hir::HirDefinitionId>, String> {
    match expression {
        crate::ast::Expression::StructConstructor { name, .. } => {
            let id = resolve_hir_type_name(project, module, name)?.ok_or_else(|| format!("in module '{}': unknown struct type '{}'", module, name))?;
            if id.kind != crate::hir::HirDefinitionKind::Struct { return Err(format!("'{}' is not a struct", name)); }
            Ok(Some(id))
        }
        crate::ast::Expression::EnumConstructor { enum_name, .. } => {
            let id = resolve_hir_type_name(project, module, enum_name)?.ok_or_else(|| format!("in module '{}': unknown enum type '{}'", module, enum_name))?;
            if id.kind != crate::hir::HirDefinitionKind::Enum { return Err(format!("'{}' is not an enum", enum_name)); }
            Ok(Some(id))
        }
        _ => Ok(None),
    }
}

fn resolve_field_accesses_in_expression(
    project: &Project, module: &str, expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>, output: &mut Vec<HirResolvedFieldAccess>,
) -> Result<(), String> {
    use crate::ast::Expression as E;
    match expression {
        E::Property { object, name, span } => {
            resolve_field_accesses_in_expression(project, module, object, environment, output)?;
            let receiver = infer_constructor_definition(project, module, object)?
                .or_else(|| if let E::Identifier { name, .. } = object.as_ref() { environment.get(name).cloned() } else { None });
            if let Some(id) = receiver {
                if id.kind == crate::hir::HirDefinitionKind::Struct {
                    // Resolve from the declaring module, not by reinterpreting
                    // its identity as an import alias in the use-site module.
                    let (definition, field) = resolve_hir_struct_field(project, &id.module, &id.name, name)?;
                    output.push(HirResolvedFieldAccess { module: module.to_string(), span: *span, definition, field });
                }
            }
        }
        E::StructConstructor { fields, .. } => for (_, value) in fields { resolve_field_accesses_in_expression(project, module, value, environment, output)?; },
        E::EnumConstructor { arguments, .. } | E::Array { elements: arguments, .. } | E::Tuple { elements: arguments, .. } => for value in arguments { resolve_field_accesses_in_expression(project, module, value, environment, output)?; },
        E::Index { array, index, .. } | E::Binary { left: array, right: index, .. } => {
            resolve_field_accesses_in_expression(project, module, array, environment, output)?;
            resolve_field_accesses_in_expression(project, module, index, environment, output)?;
        }
        E::MethodCall { object, arguments, .. } => {
            resolve_field_accesses_in_expression(project, module, object, environment, output)?;
            for value in arguments { resolve_field_accesses_in_expression(project, module, value, environment, output)?; }
        }
        E::Call { arguments, .. } => for value in arguments { resolve_field_accesses_in_expression(project, module, value, environment, output)?; },
        E::Await { expression, .. } | E::Unary { expression, .. } | E::Conversion { expression, .. } => resolve_field_accesses_in_expression(project, module, expression, environment, output)?,
        E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. } | E::String { .. } | E::Identifier { .. } => {}
    }
    Ok(())
}

fn validate_and_record_pattern(
    project: &Project, module: &str, scrutinee: &crate::hir::HirDefinitionId,
    pattern: &crate::ast::Pattern, output: &mut Vec<HirResolvedPattern>,
) -> Result<(), String> {
    use crate::ast::PatternKind as P;
    match &pattern.kind {
        P::Variant { name, bindings } => {
            let (type_name, variant_name) = name.rsplit_once("::").ok_or_else(|| format!("invalid enum pattern '{}'", name))?;
            let (resolved_id, variant) = if type_name.contains('.') {
                resolve_hir_enum_variant(project, module, type_name, variant_name)?
            } else {
                let id = scrutinee.clone();
                if id.kind != crate::hir::HirDefinitionKind::Enum || id.name != type_name {
                    return Err(format!("in module '{}': enum pattern '{}' does not match scrutinee type '{}.{}'", module, name, id.module, id.name));
                }
                resolve_hir_enum_variant(project, &id.module, &id.name, variant_name)?
            };
            if resolved_id != *scrutinee { return Err(format!("in module '{}': enum pattern '{}' belongs to '{}.{}', but match scrutinee is '{}.{}'", module, name, resolved_id.module, resolved_id.name, scrutinee.module, scrutinee.name)); }
            if bindings.len() != variant.fields.len() { return Err(format!("in module '{}': pattern '{}::{}' expects {} binding(s), found {}", module, type_name, variant_name, variant.fields.len(), bindings.len())); }
            output.push(HirResolvedPattern { module: module.to_string(), span: pattern.span, definition: resolved_id, kind: HirResolvedPatternKind::EnumVariant { variant: variant_name.to_string(), binding_count: bindings.len(), payload_types: variant.fields } });
            for binding in bindings { validate_and_record_nested_pattern(project, module, scrutinee, binding, output)?; }
        }
        P::Struct { name, fields, has_rest } => {
            let id = resolve_hir_type_name(project, module, name)?.ok_or_else(|| format!("in module '{}': unknown struct pattern type '{}'", module, name))?;
            if id != *scrutinee || id.kind != crate::hir::HirDefinitionKind::Struct { return Err(format!("in module '{}': struct pattern '{}' does not match scrutinee type '{}.{}'", module, name, scrutinee.module, scrutinee.name)); }
            let mut seen = HashSet::new();
            let mut names = Vec::new();
            for (field_name, nested) in fields {
                if !seen.insert(field_name.clone()) { return Err(format!("in module '{}': duplicate field '{}' in struct pattern '{}'", module, field_name, name)); }
                resolve_hir_struct_field(project, module, name, field_name)?;
                names.push(field_name.clone());
                validate_and_record_nested_pattern(project, module, scrutinee, nested, output)?;
            }
            output.push(HirResolvedPattern { module: module.to_string(), span: pattern.span, definition: id, kind: HirResolvedPatternKind::Struct { fields: names, has_rest: *has_rest } });
        }
        P::Tuple(items) | P::Or(items) => for item in items { validate_and_record_nested_pattern(project, module, scrutinee, item, output)?; },
        P::At { pattern, .. } => validate_and_record_nested_pattern(project, module, scrutinee, pattern, output)?,
        P::Wildcard | P::Identifier(_) | P::Number(_) | P::Float(_) | P::String(_) | P::Boolean(_) | P::Range { .. } => {}
    }
    Ok(())
}

fn validate_and_record_nested_pattern(
    _project: &Project, _module: &str, _scrutinee: &crate::hir::HirDefinitionId,
    _pattern: &crate::ast::Pattern, _output: &mut Vec<HirResolvedPattern>,
) -> Result<(), String> {
    // The payload type at this nesting level must be propagated from the
    // parent variant/field before a nested pattern can be resolved correctly.
    // Do not compare a nested pattern against its parent's type or guess.
    Ok(())
}

/// Resolve typed struct/enum patterns and constructor-backed field accesses.
/// This intentionally avoids inferring types for arbitrary function calls or
/// parameters until expression type propagation is available.
pub fn resolve_hir_pattern_and_field_expressions(project: &Project) -> Result<(Vec<HirResolvedPattern>, Vec<HirResolvedFieldAccess>), String> {
    let mut patterns = Vec::new();
    let mut fields = resolve_hir_field_accesses(project)?;
    for module in &project.modules {
        collect_patterns_in_statements(project, &module.name, &module.statements, &mut HashMap::new(), &mut patterns)?;
    }
    // Field pass also discovers pattern-independent property expressions.
    fields.shrink_to_fit();
    Ok((patterns, fields))
}

fn collect_patterns_in_statements(
    project: &Project, module: &str, statements: &[Statement],
    environment: &mut HashMap<String, crate::hir::HirDefinitionId>,
    output: &mut Vec<HirResolvedPattern>,
) -> Result<(), String> {
    use crate::ast::Statement as S;
    for statement in statements {
        match statement {
            S::VariableDeclarations { declarations, .. } => for declaration in declarations {
                if let Some(value) = &declaration.value {
                    if let Some(id) = infer_constructor_definition(project, module, value)? { environment.insert(declaration.name.clone(), id); }
                    else { environment.remove(&declaration.name); }
                }
            },
            S::ConstDeclaration { name, value, .. } => {
                if let Some(id) = infer_constructor_definition(project, module, value)? { environment.insert(name.clone(), id); }
                else { environment.remove(name); }
            }
            S::Assignment { target, value, .. } => if let crate::ast::Expression::Identifier { name, .. } = target {
                if let Some(id) = infer_constructor_definition(project, module, value)? { environment.insert(name.clone(), id); }
                else { environment.remove(name); }
            },
            S::Match { expression, arms, .. } => {
                let id = infer_constructor_definition(project, module, expression)?
                    .or_else(|| if let crate::ast::Expression::Identifier { name, .. } = expression { environment.get(name).cloned() } else { None });
                for arm in arms {
                    if let Some(id) = &id { validate_and_record_pattern(project, module, id, &arm.pattern, output)?; }
                    let mut nested = environment.clone();
                    collect_patterns_in_statements(project, module, &arm.body, &mut nested, output)?;
                }
            }
            S::Function { body, .. } | S::Main { body, .. } => {
                let mut nested = HashMap::new();
                collect_patterns_in_statements(project, module, body, &mut nested, output)?;
            }
            S::Impl { methods, .. } => for method in methods {
                collect_patterns_in_statements(project, module, std::slice::from_ref(method), &mut HashMap::new(), output)?;
            },
            S::If { body, else_body, .. } => {
                let mut nested = environment.clone();
                collect_patterns_in_statements(project, module, body, &mut nested, output)?;
                if let Some(body) = else_body { let mut nested = environment.clone(); collect_patterns_in_statements(project, module, body, &mut nested, output)?; }
            }
            S::While { body, .. } | S::For { body, .. } | S::ForEach { body, .. } => {
                let mut nested = environment.clone();
                collect_patterns_in_statements(project, module, body, &mut nested, output)?;
            }
            _ => {}
        }
    }
    Ok(())
}


/// A nominal expression type proven by module-aware semantic analysis.
/// The expression span is file-local, so `module` participates in its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirResolvedExpressionType {
    pub module: String,
    pub span: crate::span::Span,
    pub definition: crate::hir::HirDefinitionId,
}

/// A pattern-bound local whose nominal type is proven by the matched enum
/// payload or struct field declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirResolvedPatternBinding {
    pub module: String,
    pub span: crate::span::Span,
    pub name: String,
    pub definition: crate::hir::HirDefinitionId,
}

/// Propagate nominal HIR identities through typed parameters, local variables,
/// assignments, struct fields, explicit nominal function returns, and match
/// pattern bindings. This is intentionally conservative: expressions whose
/// type cannot be proven from declarations are omitted rather than guessed.
pub fn propagate_hir_expression_types(project: &Project) -> Result<Vec<HirResolvedExpressionType>, String> {
    let mut output = Vec::new();
    for module in &project.modules {
        propagate_types_in_statements(project, &module.name, &module.statements, &mut HashMap::new(), &mut output)?;
    }
    Ok(output)
}

/// Resolve nominal types introduced by match-pattern bindings. Payload and
/// field types are interpreted in the declaring module, then recorded against
/// the use-site module and pattern binding span.
pub fn resolve_hir_pattern_binding_types(project: &Project) -> Result<Vec<HirResolvedPatternBinding>, String> {
    let mut output = Vec::new();
    for module in &project.modules {
        collect_pattern_bindings_in_statements(project, &module.name, &module.statements, &mut HashMap::new(), &mut output)?;
    }
    Ok(output)
}

fn resolve_nominal_spelling(project: &Project, module: &str, spelling: &str) -> Result<Option<crate::hir::HirDefinitionId>, String> {
    resolve_hir_type_name(project, module, spelling)
}

fn resolve_function_identity(project: &Project, module: &str, source_name: &str) -> Result<Option<crate::hir::HirDefinitionId>, String> {
    use crate::hir::HirDefinitionKind::Function;
    // The compatibility AST rewrites function references to injective internal
    // symbols. Recover the source-level identity before trying source spelling.
    for owner in &project.modules {
        for original_name in &owner.function_symbols {
            if mangle_function(&owner.name, original_name) == source_name {
                return Ok(Some(crate::hir::HirDefinitionId::new(&owner.name, original_name, Function)));
            }
        }
    }
    if let Some((qualifier, name)) = source_name.split_once('.') {
        match resolve_qualified_definition(project, module, qualifier, name, Function) {
            Ok(id) => Ok(Some(id)),
            Err(error) if error.contains("private or does not exist") || error.contains("does not export") => Ok(None),
            Err(error) if error.contains("no import available") => Ok(None),
            Err(error) => Err(error),
        }
    } else {
        match resolve_unqualified_definition(project, module, source_name, Function) {
            Ok(id) => Ok(Some(id)),
            Err(error) if error.contains("not declared locally or selectively imported") => Ok(None),
            Err(error) => Err(error),
        }
    }
}


/// Resolve every project call site to its authoritative HIR declaration ID and
/// validate the portions of its signature whose nominal types are known.
pub fn resolve_hir_calls(project: &Project) -> Result<Vec<crate::hir::HirResolvedCall>, String> {
    let mut resolved = Vec::new();
    for module in &project.modules {
        collect_resolved_calls_in_statements(
            project, &module.name, &module.statements, &mut HashMap::new(), &mut resolved,
        )?;
    }
    resolved.sort_by(|a, b| {
        (&a.module, a.span.start, a.span.end, &a.declaration)
            .cmp(&(&b.module, b.span.start, b.span.end, &b.declaration))
    });
    Ok(resolved)
}

fn collect_resolved_calls_in_statements(
    project: &Project,
    module: &str,
    statements: &[Statement],
    environment: &mut HashMap<String, crate::hir::HirDefinitionId>,
    output: &mut Vec<crate::hir::HirResolvedCall>,
) -> Result<(), String> {
    use crate::ast::Statement as S;
    for statement in statements {
        match statement {
            S::VariableDeclarations { declarations, .. } => {
                for declaration in declarations {
                    if let Some(value) = &declaration.value {
                        collect_resolved_calls_in_expression(project, module, value, environment, output)?;
                    }
                    let inferred = declaration.value.as_ref()
                        .map(|value| infer_hir_expression_type(project, module, value, environment))
                        .transpose()?.flatten();
                    let declared = declaration.declared_type.as_deref()
                        .map(|name| resolve_nominal_spelling(project, module, name)).transpose()?.flatten();
                    if let Some(identity) = declared.or(inferred) {
                        environment.insert(declaration.name.clone(), identity);
                    } else {
                        environment.remove(&declaration.name);
                    }
                }
            }
            S::ConstDeclaration { name, declared_type, value, .. } => {
                collect_resolved_calls_in_expression(project, module, value, environment, output)?;
                let inferred = infer_hir_expression_type(project, module, value, environment)?;
                let declared = declared_type.as_deref()
                    .map(|name| resolve_nominal_spelling(project, module, name)).transpose()?.flatten();
                if let Some(identity) = declared.or(inferred) { environment.insert(name.clone(), identity); }
            }
            S::Assignment { target, value, .. } => {
                collect_resolved_calls_in_expression(project, module, target, environment, output)?;
                collect_resolved_calls_in_expression(project, module, value, environment, output)?;
                if let crate::ast::Expression::Identifier { name, .. } = target {
                    let inferred = infer_hir_expression_type(project, module, value, environment)?;
                    if let Some(identity) = inferred { environment.insert(name.clone(), identity); }
                }
            }
            S::Function { parameters, body, .. } => {
                let mut nested = HashMap::new();
                for parameter in parameters {
                    if let Some(type_name) = &parameter.type_name {
                        if let Some(identity) = resolve_nominal_spelling(project, module, type_name)? {
                            nested.insert(parameter.name.clone(), identity);
                        }
                    }
                }
                collect_resolved_calls_in_statements(project, module, body, &mut nested, output)?;
            }
            S::Main { body, .. } => {
                collect_resolved_calls_in_statements(project, module, body, environment, output)?;
            }
            S::Impl { type_name, methods, .. } => {
                let receiver_identity = resolve_hir_type_name(project, module, type_name)?;
                for method in methods {
                    if let S::Function { parameters, body, .. } = method {
                        let mut nested = environment.clone();
                        for parameter in parameters {
                            if parameter.name == "self" {
                                if let Some(identity) = receiver_identity.clone() {
                                    nested.insert(parameter.name.clone(), identity);
                                }
                            } else if let Some(type_name) = &parameter.type_name {
                                if type_name == "Self" { continue; }
                                if let Some(identity) = resolve_nominal_spelling(project, module, type_name)? {
                                    nested.insert(parameter.name.clone(), identity);
                                }
                            }
                        }
                        collect_resolved_calls_in_statements(project, module, body, &mut nested, output)?;
                    }
                }
            }
            S::Expression { expression, .. } | S::Call { expression, .. } | S::Defer { expression, .. } => {
                collect_resolved_calls_in_expression(project, module, expression, environment, output)?;
            }
            S::Return { value: Some(value), .. } => {
                collect_resolved_calls_in_expression(project, module, value, environment, output)?;
            }
            S::Return { value: None, .. } | S::Import { .. } | S::Struct { .. } | S::Enum { .. }
            | S::Trait { .. } | S::Break { .. } | S::Continue { .. } => {}
            S::If { condition, body, else_body, .. } => {
                collect_resolved_calls_in_expression(project, module, condition, environment, output)?;
                let mut then_env = environment.clone();
                collect_resolved_calls_in_statements(project, module, body, &mut then_env, output)?;
                if let Some(other) = else_body {
                    let mut else_env = environment.clone();
                    collect_resolved_calls_in_statements(project, module, other, &mut else_env, output)?;
                }
            }
            S::While { condition, body, .. } => {
                collect_resolved_calls_in_expression(project, module, condition, environment, output)?;
                let mut nested = environment.clone();
                collect_resolved_calls_in_statements(project, module, body, &mut nested, output)?;
            }
            S::For { variable, start, end, body, .. } => {
                collect_resolved_calls_in_expression(project, module, start, environment, output)?;
                collect_resolved_calls_in_expression(project, module, end, environment, output)?;
                let mut nested = environment.clone();
                nested.remove(variable);
                collect_resolved_calls_in_statements(project, module, body, &mut nested, output)?;
            }
            S::ForEach { variable, iterable, body, .. } => {
                collect_resolved_calls_in_expression(project, module, iterable, environment, output)?;
                let mut nested = environment.clone();
                nested.remove(variable);
                collect_resolved_calls_in_statements(project, module, body, &mut nested, output)?;
            }
            S::Match { expression, arms, .. } => {
                collect_resolved_calls_in_expression(project, module, expression, environment, output)?;
                for arm in arms {
                    let mut nested = environment.clone();
                    if let Some(guard) = &arm.guard {
                        collect_resolved_calls_in_expression(project, module, guard, &nested, output)?;
                    }
                    collect_resolved_calls_in_statements(project, module, &arm.body, &mut nested, output)?;
                }
            }
        }
    }
    Ok(())
}

fn collect_resolved_calls_in_expression(
    project: &Project,
    module: &str,
    expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
    output: &mut Vec<crate::hir::HirResolvedCall>,
) -> Result<(), String> {
    use crate::ast::Expression as E;
    match expression {
        E::Array { elements, .. } | E::Tuple { elements, .. } => {
            for element in elements { collect_resolved_calls_in_expression(project, module, element, environment, output)?; }
        }
        E::Index { array, index, .. } => {
            collect_resolved_calls_in_expression(project, module, array, environment, output)?;
            collect_resolved_calls_in_expression(project, module, index, environment, output)?;
        }
        E::Property { object, .. } | E::Await { expression: object, .. }
        | E::Unary { expression: object, .. } | E::Conversion { expression: object, .. } => {
            collect_resolved_calls_in_expression(project, module, object, environment, output)?;
        }
        E::Binary { left, right, .. } => {
            collect_resolved_calls_in_expression(project, module, left, environment, output)?;
            collect_resolved_calls_in_expression(project, module, right, environment, output)?;
        }
        E::StructConstructor { fields, .. } => {
            for (_, value) in fields { collect_resolved_calls_in_expression(project, module, value, environment, output)?; }
        }
        E::EnumConstructor { arguments, .. } | E::Call { arguments, .. } => {
            for argument in arguments { collect_resolved_calls_in_expression(project, module, argument, environment, output)?; }
        }
        E::MethodCall { object, arguments, .. } => {
            collect_resolved_calls_in_expression(project, module, object, environment, output)?;
            for argument in arguments { collect_resolved_calls_in_expression(project, module, argument, environment, output)?; }
        }
        E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. }
        | E::String { .. } | E::Identifier { .. } => {}
    }

    match expression {
        E::Call { name, arguments, generic_arguments, span } => {
            let Some(declaration) = resolve_function_identity(project, module, name)? else {
                // Intrinsics are represented as declarations in a reserved
                // module namespace too, so they participate in the same call
                // identity table as user-defined functions.
                let builtins = crate::type_checker::TypeChecker::new();
                if let Some(signature) = builtins.environment.functions.get(name) {
                    if signature.parameters.len() != arguments.len() {
                        return Err(format!("builtin '{}' expects {} arguments but received {}",
                            name, signature.parameters.len(), arguments.len()));
                    }
                    let argument_types = arguments.iter()
                        .map(|argument| infer_hir_expression_type(project, module, argument, environment))
                        .collect::<Result<Vec<_>, _>>()?;
                    output.push(crate::hir::HirResolvedCall {
                        module: module.to_string(), span: *span,
                        kind: crate::hir::HirCallKind::Function,
                        declaration: crate::hir::HirDefinitionId::new("builtin", name, crate::hir::HirDefinitionKind::Function),
                        receiver_type: None,
                        parameter_types: vec![None; signature.parameters.len()],
                        argument_types,
                        return_type: None,
                        generic_parameters: signature.generic_parameters.clone(),
                        is_async: false,
                        receiver_hir_type: None,
                        parameter_hir_types: vec![crate::hir::HirType::Unknown; signature.parameters.len()],
                        argument_hir_types: arguments.iter().map(|arg| infer_hir_structural_type(project, module, arg, environment)).collect::<Result<Vec<_>, _>>()?,
                        return_hir_type: crate::hir::HirType::Unknown,
                    });
                }
                return Ok(());
            };
            let Some(owner) = project.modules.iter().find(|candidate| candidate.name == declaration.module) else { return Ok(()); };
            let signature = owner.statements.iter().find_map(|statement| match statement {
                crate::ast::Statement::Function {
                    name: declared, generic_parameters, parameters, return_type, is_async, ..
                } if declared == &mangle_function(&owner.name, &declaration.name) || declared == &declaration.name =>
                    Some((generic_parameters, parameters, return_type, is_async)),
                _ => None,
            });
            let Some((generic_parameters, parameters, return_type, is_async)) = signature else {
                return Err(format!("resolved function identity '{}::{}' has no declaration", declaration.module, declaration.name));
            };
            if parameters.len() != arguments.len() {
                return Err(format!("call to '{}::{}' expects {} arguments but received {}",
                    declaration.module, declaration.name, parameters.len(), arguments.len()));
            }
            if !generic_arguments.is_empty() && generic_arguments.len() != generic_parameters.len() {
                return Err(format!("call to '{}::{}' expects {} explicit generic arguments but received {}",
                    declaration.module, declaration.name, generic_parameters.len(), generic_arguments.len()));
            }
            let parameter_types = parameters.iter().map(|parameter| {
                match parameter.type_name.as_deref() {
                    Some(name) => resolve_nominal_spelling(project, &owner.name, name),
                    None => Ok(None),
                }
            }).collect::<Result<Vec<_>, _>>()?;
            let argument_types = arguments.iter().map(|argument| infer_hir_expression_type(project, module, argument, environment))
                .collect::<Result<Vec<_>, _>>()?;
            for (index, (expected, actual)) in parameter_types.iter().zip(argument_types.iter()).enumerate() {
                if let (Some(expected), Some(actual)) = (expected, actual) {
                    if expected != actual {
                        return Err(format!("argument {} to '{}::{}' has nominal type '{}::{}', expected '{}::{}'",
                            index + 1, declaration.module, declaration.name,
                            actual.module, actual.name, expected.module, expected.name));
                    }
                }
            }
            let mut generic_bindings = HashMap::new();
            for (generic, explicit) in generic_parameters.iter().zip(generic_arguments.iter()) {
                generic_bindings.insert(generic.clone(), resolve_hir_type_spelling(project, module, Some(explicit), &[])?);
            }
            let parameter_hir_types = parameters.iter().map(|p| resolve_hir_type_spelling(project, &owner.name, p.type_name.as_deref(), generic_parameters)).collect::<Result<Vec<_>, _>>()?;
            let argument_hir_types = arguments.iter().map(|arg| infer_hir_structural_type(project, module, arg, environment)).collect::<Result<Vec<_>, _>>()?;
            for (index, (expected, actual)) in parameter_hir_types.iter().zip(&argument_hir_types).enumerate() {
                if !unify_hir_call_type(expected, actual, &mut generic_bindings) {
                    return Err(format!("argument {} to '{}::{}' has type {:?}, expected {:?}", index + 1, declaration.module, declaration.name, actual, expected));
                }
            }
            let resolved_return = if let Some(name) = return_type.as_deref() {
                if name == "Self" { None } else { resolve_nominal_spelling(project, &owner.name, name)? }
            } else { None };
            let structural_return = substitute_hir_generics(resolve_hir_type_spelling(project, &owner.name, return_type.as_deref(), generic_parameters)?, &generic_bindings);
            output.push(crate::hir::HirResolvedCall {
                module: module.to_string(), span: *span, kind: crate::hir::HirCallKind::Function,
                declaration, receiver_type: None, parameter_types, argument_types,
                return_type: resolved_return, generic_parameters: generic_parameters.clone(), is_async: *is_async,
                receiver_hir_type: None,
                parameter_hir_types,
                argument_hir_types,
                return_hir_type: structural_return,
            });
        }
        E::MethodCall { object, method, arguments, generic_arguments, span } => {
            if let Some(receiver_type) = infer_hir_expression_type(project, module, object, environment)? {
                let declaration = resolve_hir_method_by_id(project, module, &receiver_type, method)?;
                let owner = project.modules.iter().find(|candidate| candidate.name == declaration.module)
                    .ok_or_else(|| format!("method owner module '{}' is missing", declaration.module))?;
                let mut found = None;
                for statement in &owner.statements {
                    if let crate::ast::Statement::Impl { trait_name, type_name, methods, .. } = statement {
                        if resolve_hir_type_name(project, &owner.name, type_name)?.as_ref() != Some(&receiver_type) { continue; }
                        let trait_scope = if let Some(trait_name) = trait_name {
                            let trait_id = if let Some((qualifier, name)) = trait_name.split_once('.') {
                                resolve_qualified_definition(project, &owner.name, qualifier, name, crate::hir::HirDefinitionKind::Trait)?
                            } else {
                                resolve_unqualified_definition(project, &owner.name, trait_name, crate::hir::HirDefinitionKind::Trait)?
                            };
                            format!("{}::{}", trait_id.module, trait_id.name)
                        } else { "inherent".to_string() };
                        if declaration.scope.first() != Some(&trait_scope) { continue; }
                        for method_statement in methods {
                            if let crate::ast::Statement::Function {
                                name, generic_parameters, parameters, return_type, is_async, ..
                            } = method_statement {
                                if name == method {
                                    found = Some((generic_parameters, parameters, return_type, is_async));
                                    break;
                                }
                            }
                        }
                    }
                    if found.is_some() { break; }
                }
                let Some((generic_parameters, parameters, return_type, is_async)) = found else {
                    return Err(format!("resolved method identity '{}::{}' has no declaration", declaration.module, declaration.name));
                };
                let explicit_self = parameters.first().map(|parameter| parameter.name == "self").unwrap_or(false);
                let user_parameters = if explicit_self { &parameters[1..] } else { parameters.as_slice() };
                if user_parameters.len() != arguments.len() {
                    return Err(format!("method '{}::{}' expects {} arguments but received {}",
                        declaration.module, declaration.name, user_parameters.len(), arguments.len()));
                }
                if !generic_arguments.is_empty() && generic_arguments.len() != generic_parameters.len() {
                    return Err(format!("method '{}::{}' expects {} explicit generic arguments but received {}",
                        declaration.module, declaration.name, generic_parameters.len(), generic_arguments.len()));
                }
                let parameter_types = user_parameters.iter().map(|parameter| {
                    match parameter.type_name.as_deref() {
                        Some(name) => resolve_nominal_spelling(project, &owner.name, name),
                        None => Ok(None),
                    }
                }).collect::<Result<Vec<_>, _>>()?;
                let argument_types = arguments.iter().map(|argument| infer_hir_expression_type(project, module, argument, environment))
                    .collect::<Result<Vec<_>, _>>()?;
                for (index, (expected, actual)) in parameter_types.iter().zip(argument_types.iter()).enumerate() {
                    if let (Some(expected), Some(actual)) = (expected, actual) {
                        if expected != actual {
                            return Err(format!("argument {} to method '{}::{}' has nominal type '{}::{}', expected '{}::{}'",
                                index + 1, declaration.module, declaration.name,
                                actual.module, actual.name, expected.module, expected.name));
                        }
                    }
                }
                let mut generic_bindings = HashMap::new();
                for (generic, explicit) in generic_parameters.iter().zip(generic_arguments.iter()) {
                    generic_bindings.insert(generic.clone(), resolve_hir_type_spelling(project, module, Some(explicit), &[])?);
                }
                let parameter_hir_types = user_parameters.iter().map(|p| resolve_hir_type_spelling(project, &owner.name, p.type_name.as_deref(), generic_parameters)).collect::<Result<Vec<_>, _>>()?;
                let argument_hir_types = arguments.iter().map(|arg| infer_hir_structural_type(project, module, arg, environment)).collect::<Result<Vec<_>, _>>()?;
                for (index, (expected, actual)) in parameter_hir_types.iter().zip(&argument_hir_types).enumerate() {
                    if !unify_hir_call_type(expected, actual, &mut generic_bindings) {
                        return Err(format!("argument {} to method '{}::{}' has type {:?}, expected {:?}", index + 1, declaration.module, declaration.name, actual, expected));
                    }
                }
                let resolved_return = if let Some(name) = return_type.as_deref() {
                    if name == "Self" { Some(receiver_type.clone()) } else { resolve_nominal_spelling(project, &owner.name, name)? }
                } else { None };
                let structural_return = if return_type.as_deref() == Some("Self") { crate::hir::HirType::Nominal(receiver_type.clone()) } else { substitute_hir_generics(resolve_hir_type_spelling(project, &owner.name, return_type.as_deref(), generic_parameters)?, &generic_bindings) };
                output.push(crate::hir::HirResolvedCall {
                    module: module.to_string(), span: *span, kind: crate::hir::HirCallKind::Method,
                    declaration, receiver_type: Some(receiver_type.clone()), parameter_types, argument_types,
                    return_type: resolved_return, generic_parameters: generic_parameters.clone(), is_async: *is_async,
                    receiver_hir_type: Some(crate::hir::HirType::Nominal(receiver_type.clone())),
                    parameter_hir_types,
                    argument_hir_types,
                    return_hir_type: structural_return,
                });
            }
        }
        _ => {}
    }
    Ok(())
}

/// Resolve source type spelling into the structural HIR type algebra.
fn resolve_hir_type_spelling(
    project: &Project, module: &str, spelling: Option<&str>, generics: &[String],
) -> Result<crate::hir::HirType, String> {
    use crate::hir::HirType as T;
    let Some(raw) = spelling else { return Ok(T::Unknown); };
    let name = raw.trim();
    if generics.iter().any(|g| g == name) { return Ok(T::Generic(name.to_string())); }
    if let Some(inner) = name.strip_suffix("[]") {
        return Ok(T::Array(Box::new(resolve_hir_type_spelling(project, module, Some(inner), generics)?)));
    }
    if name.starts_with('(') && name.ends_with(')') {
        let inner = &name[1..name.len()-1];
        let parts = split_hir_type_arguments_owned(inner);
        if parts.len() == 1 && parts[0].trim().is_empty() { return Ok(T::Tuple(Vec::new())); }
        return Ok(T::Tuple(parts.iter().map(|p| resolve_hir_type_spelling(project, module, Some(p), generics)).collect::<Result<Vec<_>, _>>()?));
    }
    let (base, args) = if let Some(open) = name.find('<') {
        if !name.ends_with('>') { return Err(format!("malformed type expression '{}'", name)); }
        (&name[..open], split_hir_type_arguments_owned(&name[open+1..name.len()-1]))
    } else { (name, Vec::new()) };
    let primitive = match base.trim() {
        "num" | "int" | "i64" | "u64" => Some(T::Num),
        "float" | "f64" => Some(T::Float), "bool" => Some(T::Bool),
        "string" | "str" => Some(T::String), "void" | "unit" => Some(T::Void),
        "File" => Some(T::File), "TcpStream" => Some(T::TcpStream), _ => None,
    };
    if let Some(t) = primitive { if args.is_empty() { return Ok(t); } }
    let nested = |i: usize| -> Result<T, String> {
        let arg = args.get(i).ok_or_else(|| format!("type '{}' is missing generic argument {}", name, i+1))?;
        resolve_hir_type_spelling(project, module, Some(arg), generics)
    };
    match base.trim() {
        "Iterator" if args.len()==1 => return Ok(T::Iterator(Box::new(nested(0)?))),
        "Option" if args.len()==1 => return Ok(T::Option(Box::new(nested(0)?))),
        "Task" if args.len()==1 => return Ok(T::Task(Box::new(nested(0)?))),
        "HashMap" if args.len()==2 => return Ok(T::HashMap(Box::new(nested(0)?), Box::new(nested(1)?))),
        "Result" if args.len()==2 => return Ok(T::Result(Box::new(nested(0)?), Box::new(nested(1)?))),
        "Iterator" | "Option" | "Task" | "HashMap" | "Result" => return Err(format!("type '{}' has an invalid number of generic arguments", name)),
        _ => {}
    }
    let id = resolve_hir_type_name(project, module, base.trim())?;
    if let Some(id) = id {
        let constructor = T::Nominal(id);
        if args.is_empty() { return Ok(constructor); }
        let resolved_args = args.iter().map(|arg| resolve_hir_type_spelling(project, module, Some(arg), generics)).collect::<Result<Vec<_>, _>>()?;
        return Ok(T::Applied { constructor: Box::new(constructor), arguments: resolved_args });
    }
    if !args.is_empty() { return Err(format!("unknown generic type constructor '{}'", base.trim())); }
    if name == "unknown" { return Ok(T::Unknown); }
    Err(format!("in module '{}': unresolved type '{}'", module, name))
}

fn split_hir_type_arguments_owned(input: &str) -> Vec<String> {
    let mut result = Vec::new(); let mut depth = 0i32; let mut start = 0usize;
    for (i, ch) in input.char_indices() {
        match ch { '<' | '(' | '[' => depth += 1, '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => { result.push(input[start..i].trim().to_string()); start = i+1; }, _ => {} }
    }
    if start < input.len() || !input.trim().is_empty() { result.push(input[start..].trim().to_string()); }
    result
}

fn unify_hir_call_type(expected: &crate::hir::HirType, actual: &crate::hir::HirType, bindings: &mut HashMap<String, crate::hir::HirType>) -> bool {
    use crate::hir::HirType as T;
    match (expected, actual) {
        (T::Unknown, _) | (_, T::Unknown) => true,
        (T::Generic(name), value) => match bindings.get(name) { Some(existing) => existing == value, None => { bindings.insert(name.clone(), value.clone()); true } },
        (T::Array(a), T::Array(b)) | (T::Iterator(a), T::Iterator(b)) | (T::Option(a), T::Option(b)) | (T::Task(a), T::Task(b)) => unify_hir_call_type(a, b, bindings),
        (T::HashMap(ak, av), T::HashMap(bk, bv)) | (T::Result(ak, av), T::Result(bk, bv)) => unify_hir_call_type(ak, bk, bindings) && unify_hir_call_type(av, bv, bindings),
        (T::Tuple(a), T::Tuple(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x,y)| unify_hir_call_type(x,y,bindings)),
        (T::Applied { constructor: ac, arguments: aa }, T::Applied { constructor: bc, arguments: ba }) => aa.len() == ba.len() && unify_hir_call_type(ac,bc,bindings) && aa.iter().zip(ba).all(|(x,y)| unify_hir_call_type(x,y,bindings)),
        _ => expected == actual,
    }
}

fn substitute_hir_generics(ty: crate::hir::HirType, bindings: &HashMap<String, crate::hir::HirType>) -> crate::hir::HirType {
    use crate::hir::HirType as T;
    match ty {
        T::Generic(name) => bindings.get(&name).cloned().unwrap_or(T::Generic(name)),
        T::Array(x) => T::Array(Box::new(substitute_hir_generics(*x, bindings))),
        T::Iterator(x) => T::Iterator(Box::new(substitute_hir_generics(*x, bindings))),
        T::Option(x) => T::Option(Box::new(substitute_hir_generics(*x, bindings))),
        T::Task(x) => T::Task(Box::new(substitute_hir_generics(*x, bindings))),
        T::HashMap(k,v) => T::HashMap(Box::new(substitute_hir_generics(*k, bindings)), Box::new(substitute_hir_generics(*v, bindings))),
        T::Result(a,b) => T::Result(Box::new(substitute_hir_generics(*a, bindings)), Box::new(substitute_hir_generics(*b, bindings))),
        T::Tuple(items) => T::Tuple(items.into_iter().map(|x| substitute_hir_generics(x, bindings)).collect()),
        T::Applied { constructor, arguments } => T::Applied { constructor: Box::new(substitute_hir_generics(*constructor, bindings)), arguments: arguments.into_iter().map(|x| substitute_hir_generics(x, bindings)).collect() },
        other => other,
    }
}

/// Build a structural local-type table for the complete project. Unlike the
/// older nominal-only propagation pass, this retains primitive/container types
/// through local bindings, assignments and nested lexical scopes.
pub fn resolve_hir_local_types(project: &Project) -> Result<Vec<crate::hir::HirResolvedLocalType>, String> {
    let mut output = Vec::new();
    for module in &project.modules {
        collect_hir_structural_locals(project, &module.name, &module.statements, &mut HashMap::new(), None, &[], &mut output)?;
    }
    output.sort_by(|a, b| (&a.module, a.span.start, &a.name).cmp(&(&b.module, b.span.start, &b.name)));
    Ok(output)
}

fn merge_hir_local_scopes(
    target: &mut HashMap<String, crate::hir::HirType>,
    branches: &[HashMap<String, crate::hir::HirType>],
) {
    if branches.is_empty() { return; }
    target.retain(|name, ty| branches.iter().all(|branch| branch.get(name) == Some(ty)));
}

fn collect_hir_structural_locals(
    project: &Project,
    module: &str,
    statements: &[Statement],
    environment: &mut HashMap<String, crate::hir::HirType>,
    return_type: Option<&str>,
    generic_parameters: &[String],
    output: &mut Vec<crate::hir::HirResolvedLocalType>,
) -> Result<(), String> {
    use crate::ast::{Expression as E, Statement as S};
    use crate::hir::{HirResolvedLocalType, HirType as T};
    for statement in statements {
        match statement {
            S::VariableDeclarations { declarations, .. } => for declaration in declarations {
                let inferred = declaration.value.as_ref().map(|value| infer_hir_structural_type_with_locals(project, module, value, environment)).transpose()?;
                let declared = declaration.declared_type.as_deref().map(|name| resolve_hir_type_spelling(project, module, Some(name), generic_parameters)).transpose()?;
                let ty = match (declared, inferred) {
                    (Some(expected), Some(actual)) if expected != T::Unknown && actual != T::Unknown => {
                        let mut bindings = HashMap::new();
                        if !unify_hir_call_type(&expected, &actual, &mut bindings) {
                            return Err(format!("in module '{}': local '{}' has initializer type {:?}, expected {:?}", module, declaration.name, actual, expected));
                        }
                        substitute_hir_generics(expected, &bindings)
                    }
                    (Some(expected), _) => expected,
                    (_, Some(actual)) => actual,
                    _ => T::Unknown,
                };
                environment.insert(declaration.name.clone(), ty.clone());
                output.push(HirResolvedLocalType { module: module.to_string(), name: declaration.name.clone(), span: declaration.name_span, ty });
            },
            S::ConstDeclaration { name, name_span, declared_type, value, .. } => {
                let actual = infer_hir_structural_type_with_locals(project, module, value, environment)?;
                let expected = declared_type.as_deref().map(|name| resolve_hir_type_spelling(project, module, Some(name), generic_parameters)).transpose()?;
                let ty = match expected {
                    Some(expected) if expected != T::Unknown && actual != T::Unknown => {
                        if !unify_hir_call_type(&expected, &actual, &mut HashMap::new()) {
                            return Err(format!("in module '{}': constant '{}' has type {:?}, expected {:?}", module, name, actual, expected));
                        }
                        expected
                    }
                    Some(expected) => expected,
                    None => actual,
                };
                environment.insert(name.clone(), ty.clone());
                output.push(HirResolvedLocalType { module: module.to_string(), name: name.clone(), span: *name_span, ty });
            }
            S::Assignment { target, value, .. } => {
                if let E::Identifier { name, .. } = target {
                    let actual = infer_hir_structural_type_with_locals(project, module, value, environment)?;
                    if let Some(expected) = environment.get(name) {
                        if *expected != T::Unknown && actual != T::Unknown && !unify_hir_call_type(expected, &actual, &mut HashMap::new()) {
                            return Err(format!("in module '{}': assignment to '{}' changes structural type from {:?} to {:?}", module, name, expected, actual));
                        }
                    }
                    environment.insert(name.clone(), actual);
                }
            }
            S::Function { name: _, generic_parameters: generics, parameters, return_type: declared_return, body, .. } => {
                let mut nested = HashMap::new();
                for parameter in parameters {
                    let ty = parameter.type_name.as_deref().map(|name| resolve_hir_type_spelling(project, module, Some(name), generics)).transpose()?.unwrap_or(T::Unknown);
                    nested.insert(parameter.name.clone(), ty.clone());
                    output.push(HirResolvedLocalType { module: module.to_string(), name: parameter.name.clone(), span: parameter.name_span, ty });
                }
                collect_hir_structural_locals(project, module, body, &mut nested, declared_return.as_deref(), generics, output)?;
            }
            S::Main { body, .. } => collect_hir_structural_locals(project, module, body, environment, None, &[], output)?,
            S::Impl { type_name, methods, .. } => {
                let receiver = resolve_hir_type_spelling(project, module, Some(type_name), &[])?;
                for method in methods {
                    if let S::Function { generic_parameters: generics, parameters, return_type: declared_return, body, .. } = method {
                        let mut nested = HashMap::new();
                        for parameter in parameters {
                            let ty = if parameter.name == "self" { receiver.clone() } else { parameter.type_name.as_deref().map(|name| if name == "Self" { Ok(receiver.clone()) } else { resolve_hir_type_spelling(project, module, Some(name), generics) }).transpose()?.unwrap_or(T::Unknown) };
                            nested.insert(parameter.name.clone(), ty.clone());
                            output.push(HirResolvedLocalType { module: module.to_string(), name: parameter.name.clone(), span: parameter.name_span, ty });
                        }
                        let method_return = if declared_return.as_deref() == Some("Self") { Some(type_name.as_str()) } else { declared_return.as_deref() };
                        collect_hir_structural_locals(project, module, body, &mut nested, method_return, generics, output)?;
                    }
                }
            }
            S::Return { value: Some(value), .. } => if let Some(expected) = return_type {
                let expected = resolve_hir_type_spelling(project, module, Some(expected), generic_parameters)?;
                let actual = infer_hir_structural_type_with_locals(project, module, value, environment)?;
                if expected != T::Unknown && actual != T::Unknown && !unify_hir_call_type(&expected, &actual, &mut HashMap::new()) {
                    return Err(format!("in module '{}': return value has structural type {:?}, expected {:?}", module, actual, expected));
                }
            },
            S::If { body, else_body, .. } => {
                let mut then_env = environment.clone();
                collect_hir_structural_locals(project, module, body, &mut then_env, return_type, generic_parameters, output)?;
                let mut branches = vec![then_env];
                if let Some(other) = else_body {
                    let mut else_env = environment.clone();
                    collect_hir_structural_locals(project, module, other, &mut else_env, return_type, generic_parameters, output)?;
                    branches.push(else_env);
                } else { branches.push(environment.clone()); }
                merge_hir_local_scopes(environment, &branches);
            }
            S::While { body, .. } => {
                let mut nested = environment.clone();
                collect_hir_structural_locals(project, module, body, &mut nested, return_type, generic_parameters, output)?;
            }
            S::For { variable, start, end, body, .. } => {
                let start_type = infer_hir_structural_type_with_locals(project, module, start, environment)?;
                let end_type = infer_hir_structural_type_with_locals(project, module, end, environment)?;
                if start_type != T::Unknown && end_type != T::Unknown && !unify_hir_call_type(&start_type, &end_type, &mut HashMap::new()) {
                    return Err(format!("in module '{}': numeric for-loop bounds have incompatible types {:?} and {:?}", module, start_type, end_type));
                }
                let mut nested = environment.clone();
                nested.insert(variable.clone(), if start_type == T::Unknown { T::Num } else { start_type });
                collect_hir_structural_locals(project, module, body, &mut nested, return_type, generic_parameters, output)?;
            }
            S::ForEach { variable, iterable, body, .. } => {
                let iterable_type = infer_hir_structural_type_with_locals(project, module, iterable, environment)?;
                let element_type = match iterable_type { T::Array(inner) | T::Iterator(inner) => *inner, T::String => T::String, _ => T::Unknown };
                let mut nested = environment.clone();
                nested.insert(variable.clone(), element_type.clone());
                output.push(HirResolvedLocalType { module: module.to_string(), name: variable.clone(), span: statement.span(), ty: element_type });
                collect_hir_structural_locals(project, module, body, &mut nested, return_type, generic_parameters, output)?;
            }
            S::Match { arms, .. } => {
                let mut branches = Vec::new();
                for arm in arms {
                    let mut nested = environment.clone();
                    collect_hir_structural_locals(project, module, &arm.body, &mut nested, return_type, generic_parameters, output)?;
                    branches.push(nested);
                }
                merge_hir_local_scopes(environment, &branches);
            }
            _ => {}
        }
    }
    Ok(())
}

fn infer_hir_structural_type_with_locals(
    project: &Project,
    module: &str,
    expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirType>,
) -> Result<crate::hir::HirType, String> {
    use crate::ast::Expression as E;
    use crate::hir::HirType as T;
    match expression {
        E::Identifier { name, .. } => Ok(environment.get(name).cloned().unwrap_or(T::Unknown)),
        E::Array { elements, .. } => {
            let mut element = T::Unknown;
            for (index, value) in elements.iter().enumerate() {
                let actual = infer_hir_structural_type_with_locals(project, module, value, environment)?;
                if index == 0 || element == T::Unknown { element = actual; }
                else if actual != T::Unknown && !unify_hir_call_type(&element, &actual, &mut HashMap::new()) {
                    return Err(format!("array elements must have a consistent type; element {} has type {:?}, conflicting with {:?}", index + 1, actual, element));
                }
            }
            Ok(T::Array(Box::new(element)))
        }
        E::Tuple { elements, .. } => Ok(T::Tuple(elements.iter().map(|value| infer_hir_structural_type_with_locals(project, module, value, environment)).collect::<Result<Vec<_>, _>>()?)),
        E::Index { array, index, .. } => match infer_hir_structural_type_with_locals(project, module, array, environment)? {
            T::Array(inner) | T::Iterator(inner) => Ok(*inner),
            T::Tuple(items) => match index.as_ref() { E::Number { value, .. } if *value >= 0 => Ok(items.get(*value as usize).cloned().unwrap_or(T::Unknown)), _ => Ok(T::Unknown) },
            T::String => Ok(T::String), _ => Ok(T::Unknown),
        },
        E::Unary { operator, expression, .. } => match operator {
            crate::ast::UnaryOperator::Not => Ok(T::Bool),
            _ => infer_hir_structural_type_with_locals(project, module, expression, environment),
        },
        E::Binary { left, operator, right, .. } => match operator {
            crate::ast::Operator::Equal | crate::ast::Operator::NotEqual | crate::ast::Operator::Less | crate::ast::Operator::Greater | crate::ast::Operator::LessEqual | crate::ast::Operator::GreaterEqual | crate::ast::Operator::And | crate::ast::Operator::Or => Ok(T::Bool),
            _ => {
                let lhs = infer_hir_structural_type_with_locals(project, module, left, environment)?;
                let rhs = infer_hir_structural_type_with_locals(project, module, right, environment)?;
                if lhs != T::Unknown && rhs != T::Unknown && !unify_hir_call_type(&lhs, &rhs, &mut HashMap::new()) { return Err(format!("binary operands have incompatible structural types {:?} and {:?}", lhs, rhs)); }
                Ok(if lhs == T::Unknown { rhs } else { lhs })
            }
        },
        E::Await { expression, .. } => match infer_hir_structural_type_with_locals(project, module, expression, environment)? { T::Task(inner) => Ok(*inner), _ => Ok(T::Unknown) },
        E::Conversion { target_type, .. } => resolve_hir_type_spelling(project, module, Some(target_type), &[]),
        E::Property { object, name, .. } => match infer_hir_structural_type_with_locals(project, module, object, environment)? {
            T::Nominal(id) => {
                let registry = hir_type_registry(project);
                if let Some(definition) = registry.get(&id) {
                    if let crate::hir::HirTypeShape::Struct { fields, .. } = &definition.shape {
                        if let Some(field) = fields.iter().find(|field| field.name == *name) { return resolve_hir_type_spelling(project, &id.module, Some(&field.type_name), &[]); }
                    }
                }
                Ok(T::Unknown)
            }
            _ => Ok(T::Unknown),
        },
        E::Call { name, arguments, generic_arguments, .. } => {
            let Some(identity) = resolve_function_identity(project, module, name)? else { return Ok(T::Unknown); };
            let Some(owner) = project.modules.iter().find(|candidate| candidate.name == identity.module) else { return Ok(T::Unknown); };
            if let Some((generics, parameters, return_type)) = owner.statements.iter().find_map(|statement| match statement {
                Statement::Function { name: declared, generic_parameters, parameters, return_type, .. } if declared == &mangle_function(&owner.name, &identity.name) || declared == &identity.name => Some((generic_parameters, parameters, return_type)), _ => None,
            }) {
                if parameters.len() != arguments.len() { return Ok(T::Unknown); }
                if !generic_arguments.is_empty() && generic_arguments.len() != generics.len() {
                    return Err(format!("call to '{}::{}' expects {} explicit generic arguments but received {}", identity.module, identity.name, generics.len(), generic_arguments.len()));
                }
                let mut bindings = HashMap::new();
                for (generic, explicit) in generics.iter().zip(generic_arguments.iter()) { bindings.insert(generic.clone(), resolve_hir_type_spelling(project, module, Some(explicit), &[])?); }
                for (parameter, argument) in parameters.iter().zip(arguments) {
                    let expected = resolve_hir_type_spelling(project, &owner.name, parameter.type_name.as_deref(), generics)?;
                    let actual = infer_hir_structural_type_with_locals(project, module, argument, environment)?;
                    if !unify_hir_call_type(&expected, &actual, &mut bindings) { return Err(format!("argument to '{}::{}' has type {:?}, expected {:?}", identity.module, identity.name, actual, expected)); }
                }
                return Ok(substitute_hir_generics(resolve_hir_type_spelling(project, &owner.name, return_type.as_deref(), generics)?, &bindings));
            }
            Ok(T::Unknown)
        }
        E::MethodCall { object, method, arguments, generic_arguments, .. } => {
            let receiver = match infer_hir_structural_type_with_locals(project, module, object, environment)? {
                T::Nominal(id) => id,
                _ => return Ok(T::Unknown),
            };
            let identity = resolve_hir_method_by_id(project, module, &receiver, method)?;
            let Some(owner) = project.modules.iter().find(|candidate| candidate.name == identity.module) else { return Ok(T::Unknown); };
            for statement in &owner.statements {
                let impl_data = match statement { Statement::Impl { trait_name, type_name, methods, .. } => Some((trait_name, type_name, methods)), _ => None };
                let Some((trait_name, type_name, methods)) = impl_data else { continue; };
                if resolve_hir_type_name(project, &owner.name, type_name)?.as_ref() != Some(&receiver) { continue; }
                let scope = if let Some(trait_name) = trait_name {
                    let trait_id = if let Some((qualifier, name)) = trait_name.split_once('.') { resolve_qualified_definition(project, &owner.name, qualifier, name, crate::hir::HirDefinitionKind::Trait)? } else { resolve_unqualified_definition(project, &owner.name, trait_name, crate::hir::HirDefinitionKind::Trait)? };
                    format!("{}::{}", trait_id.module, trait_id.name)
                } else { "inherent".to_string() };
                if identity.scope.first() != Some(&scope) { continue; }
                for method_statement in methods {
                    let Statement::Function { name, generic_parameters, parameters, return_type, .. } = method_statement else { continue; };
                    if name != method { continue; }
                    let user_parameters = if parameters.first().map(|p| p.name == "self").unwrap_or(false) { &parameters[1..] } else { parameters.as_slice() };
                    if user_parameters.len() != arguments.len() { return Ok(T::Unknown); }
                    if !generic_arguments.is_empty() && generic_arguments.len() != generic_parameters.len() { return Err(format!("method '{}' expects {} explicit generic arguments but received {}", method, generic_parameters.len(), generic_arguments.len())); }
                    let mut bindings = HashMap::new();
                    for (generic, explicit) in generic_parameters.iter().zip(generic_arguments) { bindings.insert(generic.clone(), resolve_hir_type_spelling(project, module, Some(explicit), &[])?); }
                    for (parameter, argument) in user_parameters.iter().zip(arguments) {
                        let expected = if parameter.type_name.as_deref() == Some("Self") { T::Nominal(receiver.clone()) } else { resolve_hir_type_spelling(project, &owner.name, parameter.type_name.as_deref(), generic_parameters)? };
                        let actual = infer_hir_structural_type_with_locals(project, module, argument, environment)?;
                        if !unify_hir_call_type(&expected, &actual, &mut bindings) { return Err(format!("argument to method '{}' has type {:?}, expected {:?}", method, actual, expected)); }
                    }
                    let result = if return_type.as_deref() == Some("Self") { T::Nominal(receiver.clone()) } else { resolve_hir_type_spelling(project, &owner.name, return_type.as_deref(), generic_parameters)? };
                    return Ok(substitute_hir_generics(result, &bindings));
                }
            }
            Ok(T::Unknown)
        }
        _ => infer_hir_structural_type(project, module, expression, &HashMap::new()),
    }
}

fn infer_hir_structural_type(
    project: &Project, module: &str, expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
) -> Result<crate::hir::HirType, String> {
    use crate::ast::Expression as E;
    use crate::hir::HirType as T;

    if let Some(id) = infer_hir_expression_type(project, module, expression, environment)? {
        return Ok(T::Nominal(id));
    }

    Ok(match expression {
        E::Number { .. } | E::WideNumber { .. } => T::Num,
        E::Float { .. } => T::Float,
        E::Boolean { .. } => T::Bool,
        E::String { .. } => T::String,
        E::Tuple { elements, .. } => T::Tuple(
            elements.iter()
                .map(|element| infer_hir_structural_type(project, module, element, environment))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        E::Array { elements, .. } => {
            let mut element_type = T::Unknown;
            for (index, element) in elements.iter().enumerate() {
                let actual = infer_hir_structural_type(project, module, element, environment)?;
                if index == 0 || element_type == T::Unknown {
                    element_type = actual;
                } else if actual != T::Unknown {
                    let mut bindings = HashMap::new();
                    if !unify_hir_call_type(&element_type, &actual, &mut bindings) {
                        return Err(format!(
                            "array elements must have a consistent type; element {} has type {:?}, which conflicts with {:?}",
                            index + 1, actual, element_type
                        ));
                    }
                    element_type = substitute_hir_generics(element_type, &bindings);
                }
            }
            T::Array(Box::new(element_type))
        }
        E::Index { array, index, .. } => {
            match infer_hir_structural_type(project, module, array, environment)? {
                T::Array(element) | T::Iterator(element) => *element,
                T::Tuple(items) => match index.as_ref() {
                    E::Number { value, .. } if *value >= 0 => items.get(*value as usize).cloned().unwrap_or(T::Unknown),
                    _ => T::Unknown,
                },
                T::String => T::String,
                _ => T::Unknown,
            }
        }
        E::Property { object, name, .. } => {
            let Some(receiver) = infer_hir_expression_type(project, module, object, environment)? else {
                return Ok(T::Unknown);
            };
            let registry = hir_type_registry(project);
            let Some(definition) = registry.get(&receiver) else {
                return Err(format!("missing HIR type metadata for '{}.{}'", receiver.module, receiver.name));
            };
            match &definition.shape {
                crate::hir::HirTypeShape::Struct { fields, .. } => {
                    let field = fields.iter().find(|field| field.name == *name)
                        .ok_or_else(|| format!("in module '{}': struct '{}.{}' has no field '{}'", module, receiver.module, receiver.name, name))?;
                    resolve_hir_type_spelling(project, &receiver.module, Some(&field.type_name), &[])?
                }
                _ => T::Unknown,
            }
        }
        E::Conversion { target_type, .. } => {
            resolve_hir_type_spelling(project, module, Some(target_type), &[])?
        }
        E::Await { expression, .. } => match infer_hir_structural_type(project, module, expression, environment)? {
            T::Task(inner) => *inner,
            _ => T::Unknown,
        },
        E::Call { name, arguments, generic_arguments, .. } => {
            infer_hir_function_call_return_type(project, module, name, arguments, generic_arguments, environment)?
        }
        E::MethodCall { object, method, arguments, generic_arguments, .. } => {
            infer_hir_method_call_return_type(project, module, object, method, arguments, generic_arguments, environment)?
        }
        E::Unary { operator, expression, .. } => match operator {
            crate::ast::UnaryOperator::Not => T::Bool,
            _ => infer_hir_structural_type(project, module, expression, environment)?,
        },
        E::Binary { left, operator, right, .. } => match operator {
            crate::ast::Operator::Equal | crate::ast::Operator::NotEqual
            | crate::ast::Operator::Less | crate::ast::Operator::Greater
            | crate::ast::Operator::LessEqual | crate::ast::Operator::GreaterEqual
            | crate::ast::Operator::And | crate::ast::Operator::Or => T::Bool,
            crate::ast::Operator::Plus | crate::ast::Operator::Minus
            | crate::ast::Operator::Multiply | crate::ast::Operator::Divide => {
                let left_type = infer_hir_structural_type(project, module, left, environment)?;
                let right_type = infer_hir_structural_type(project, module, right, environment)?;
                if !unify_hir_call_type(&left_type, &right_type, &mut HashMap::new()) {
                    return Err(format!("binary operands have incompatible types {:?} and {:?}", left_type, right_type));
                }
                left_type
            }
        },
        E::Identifier { .. } | E::StructConstructor { .. } | E::EnumConstructor { .. } => T::Unknown,
    })
}

fn infer_hir_function_call_return_type(
    project: &Project,
    module: &str,
    name: &str,
    arguments: &[crate::ast::Expression],
    generic_arguments: &[String],
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
) -> Result<crate::hir::HirType, String> {
    let Some(identity) = resolve_function_identity(project, module, name)? else {
        return Ok(crate::hir::HirType::Unknown);
    };
    let Some(owner) = project.modules.iter().find(|candidate| candidate.name == identity.module) else {
        return Ok(crate::hir::HirType::Unknown);
    };
    let signature = owner.statements.iter().find_map(|statement| match statement {
        Statement::Function { name: declared, generic_parameters, parameters, return_type, .. }
            if declared == &mangle_function(&owner.name, &identity.name) || declared == &identity.name =>
                Some((generic_parameters, parameters, return_type)),
        _ => None,
    });
    let Some((generics, parameters, return_type)) = signature else { return Ok(crate::hir::HirType::Unknown); };
    if parameters.len() != arguments.len() { return Ok(crate::hir::HirType::Unknown); }
    if !generic_arguments.is_empty() && generic_arguments.len() != generics.len() {
        return Err(format!("call to '{}::{}' expects {} explicit generic arguments but received {}", identity.module, identity.name, generics.len(), generic_arguments.len()));
    }
    let mut bindings = HashMap::new();
    for (generic, explicit) in generics.iter().zip(generic_arguments.iter()) {
        bindings.insert(generic.clone(), resolve_hir_type_spelling(project, module, Some(explicit), &[])?);
    }
    for (parameter, argument) in parameters.iter().zip(arguments.iter()) {
        let expected = resolve_hir_type_spelling(project, &owner.name, parameter.type_name.as_deref(), generics)?;
        let actual = infer_hir_structural_type(project, module, argument, environment)?;
        if !unify_hir_call_type(&expected, &actual, &mut bindings) {
            return Err(format!("argument to '{}::{}' has type {:?}, expected {:?}", identity.module, identity.name, actual, expected));
        }
    }
    Ok(substitute_hir_generics(
        resolve_hir_type_spelling(project, &owner.name, return_type.as_deref(), generics)?,
        &bindings,
    ))
}

fn infer_hir_method_call_return_type(
    project: &Project,
    module: &str,
    object: &crate::ast::Expression,
    method: &str,
    arguments: &[crate::ast::Expression],
    generic_arguments: &[String],
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
) -> Result<crate::hir::HirType, String> {
    let Some(receiver) = infer_hir_expression_type(project, module, object, environment)? else {
        return Ok(crate::hir::HirType::Unknown);
    };
    let identity = resolve_hir_method_by_id(project, module, &receiver, method)?;
    let Some(owner) = project.modules.iter().find(|candidate| candidate.name == identity.module) else {
        return Ok(crate::hir::HirType::Unknown);
    };
    for statement in &owner.statements {
        let Statement::Impl { trait_name, type_name, methods, .. } = statement else { continue; };
        if resolve_hir_type_name(project, &owner.name, type_name)?.as_ref() != Some(&receiver) { continue; }
        let scope = if let Some(trait_name) = trait_name {
            let trait_id = if let Some((qualifier, name)) = trait_name.split_once('.') {
                resolve_qualified_definition(project, &owner.name, qualifier, name, crate::hir::HirDefinitionKind::Trait)?
            } else {
                resolve_unqualified_definition(project, &owner.name, trait_name, crate::hir::HirDefinitionKind::Trait)?
            };
            format!("{}::{}", trait_id.module, trait_id.name)
        } else { "inherent".to_string() };
        if identity.scope.first() != Some(&scope) { continue; }
        for method_statement in methods {
            let Statement::Function { name, generic_parameters, parameters, return_type, .. } = method_statement else { continue; };
            if name != method { continue; }
            let user_parameters = if parameters.first().map(|p| p.name == "self").unwrap_or(false) { &parameters[1..] } else { parameters.as_slice() };
            if user_parameters.len() != arguments.len() { return Ok(crate::hir::HirType::Unknown); }
            if !generic_arguments.is_empty() && generic_arguments.len() != generic_parameters.len() {
                return Err(format!("method '{}::{}' expects {} explicit generic arguments but received {}", identity.module, identity.name, generic_parameters.len(), generic_arguments.len()));
            }
            let mut bindings = HashMap::new();
            for (generic, explicit) in generic_parameters.iter().zip(generic_arguments.iter()) {
                bindings.insert(generic.clone(), resolve_hir_type_spelling(project, module, Some(explicit), &[])?);
            }
            for (parameter, argument) in user_parameters.iter().zip(arguments.iter()) {
                let expected = resolve_hir_type_spelling(project, &owner.name, parameter.type_name.as_deref(), generic_parameters)?;
                let actual = infer_hir_structural_type(project, module, argument, environment)?;
                if !unify_hir_call_type(&expected, &actual, &mut bindings) {
                    return Err(format!("argument to method '{}::{}' has type {:?}, expected {:?}", identity.module, identity.name, actual, expected));
                }
            }
            let result = if return_type.as_deref() == Some("Self") {
                crate::hir::HirType::Nominal(receiver.clone())
            } else {
                resolve_hir_type_spelling(project, &owner.name, return_type.as_deref(), generic_parameters)?
            };
            return Ok(substitute_hir_generics(result, &bindings));
        }
    }
    Ok(crate::hir::HirType::Unknown)
}

fn infer_hir_expression_type(
    project: &Project,
    module: &str,
    expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
) -> Result<Option<crate::hir::HirDefinitionId>, String> {
    use crate::ast::Expression as E;
    use crate::hir::{HirDefinitionKind as K, HirTypeShape};
    match expression {
        E::StructConstructor { name, .. } | E::EnumConstructor { enum_name: name, .. } => {
            let id = resolve_hir_type_name(project, module, name)?;
            if let Some(id) = &id {
                let expected = if matches!(expression, E::StructConstructor { .. }) { K::Struct } else { K::Enum };
                if id.kind != expected { return Err(format!("in module '{}': '{}' is not a {:?}", module, name, expected)); }
            }
            Ok(id)
        }
        E::Identifier { name, .. } => Ok(environment.get(name).cloned()),
        E::Property { object, name, .. } => {
            let Some(receiver) = infer_hir_expression_type(project, module, object, environment)? else { return Ok(None); };
            if receiver.kind != K::Struct { return Ok(None); }
            let registry = hir_type_registry(project);
            let Some(definition) = registry.get(&receiver) else { return Err(format!("missing HIR type metadata for '{}.{}'", receiver.module, receiver.name)); };
            let HirTypeShape::Struct { fields, .. } = &definition.shape else { return Ok(None); };
            let Some(field) = fields.iter().find(|field| field.name == *name) else {
                return Err(format!("in module '{}': struct '{}.{}' has no field '{}'", module, receiver.module, receiver.name, name));
            };
            resolve_nominal_spelling(project, &receiver.module, &field.type_name)
        }
        E::MethodCall { object, method, .. } => {
            let Some(receiver_id) = infer_hir_expression_type(project, module, object, environment)? else { return Ok(None); };
            let method_id = resolve_hir_method_by_id(project, module, &receiver_id, method)?;
            let owner = project.modules.iter().find(|candidate| candidate.name == method_id.module);
            if let Some(owner) = owner {
                for statement in &owner.statements {
                    if let Statement::Impl { trait_name, type_name, methods, .. } = statement {
                        if resolve_hir_type_name(project, &owner.name, type_name)?.as_ref() != Some(&receiver_id) { continue; }
                        let impl_scope = if let Some(trait_name) = trait_name {
                            let trait_id = if let Some((qualifier, name)) = trait_name.split_once('.') {
                                resolve_qualified_definition(project, &owner.name, qualifier, name, crate::hir::HirDefinitionKind::Trait)?
                            } else {
                                resolve_unqualified_definition(project, &owner.name, trait_name, crate::hir::HirDefinitionKind::Trait)?
                            };
                            format!("{}::{}", trait_id.module, trait_id.name)
                        } else { "inherent".to_string() };
                        if method_id.scope.first() != Some(&impl_scope) { continue; }
                        if let Some(return_type) = methods.iter().find_map(|method| match method {
                            crate::ast::Statement::Function { name, return_type, .. } if name == &method_id.name => return_type.as_deref(),
                            _ => None,
                        }) {
                            if return_type == "Self" { return Ok(Some(receiver_id)); }
                            return resolve_nominal_spelling(project, &owner.name, return_type);
                        }
                    }
                }
            }
            Ok(None)
        }
        E::Call { name, .. } => {
            let Some(function_id) = resolve_function_identity(project, module, name)? else { return Ok(None); };
            let Some(owner) = project.modules.iter().find(|candidate| candidate.name == function_id.module) else { return Ok(None); };
            for statement in &owner.statements {
                if let crate::ast::Statement::Function { name: declared, return_type: Some(return_type), .. } = statement {
                    if declared == &mangle_function(&owner.name, &function_id.name) || declared == &function_id.name {
                        return resolve_nominal_spelling(project, &owner.name, return_type);
                    }
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn record_expression_type(
    project: &Project,
    module: &str,
    expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
    output: &mut Vec<HirResolvedExpressionType>,
) -> Result<(), String> {
    walk_expression_children(project, module, expression, environment, output)?;
    if let Some(definition) = infer_hir_expression_type(project, module, expression, environment)? {
        output.push(HirResolvedExpressionType { module: module.to_string(), span: expression.span(), definition });
    }
    Ok(())
}

fn bind_pattern_nominal_types(
    project: &Project,
    module: &str,
    expected: Option<&crate::hir::HirDefinitionId>,
    pattern: &crate::ast::Pattern,
    environment: &mut HashMap<String, crate::hir::HirDefinitionId>,
    output: &mut Vec<HirResolvedPatternBinding>,
) -> Result<(), String> {
    use crate::ast::PatternKind as P;
    use crate::hir::{HirDefinitionKind as K, HirTypeShape};
    let bind_name = |name: &str, id: &crate::hir::HirDefinitionId,
                     environment: &mut HashMap<String, crate::hir::HirDefinitionId>,
                     output: &mut Vec<HirResolvedPatternBinding>| {
        environment.insert(name.to_string(), id.clone());
        output.push(HirResolvedPatternBinding { module: module.to_string(), span: pattern.span, name: name.to_string(), definition: id.clone() });
    };

    match &pattern.kind {
        P::Identifier(name) => if let Some(id) = expected { bind_name(name, id, environment, output); },
        P::At { name, pattern: nested } => {
            if let Some(id) = expected { bind_name(name, id, environment, output); }
            bind_pattern_nominal_types(project, module, expected, nested, environment, output)?;
        }
        P::Variant { name, bindings } => {
            let Some(expected_id) = expected else { return Ok(()); };
            if expected_id.kind != K::Enum {
                return Err(format!("in module '{}': enum pattern '{}' is nested under non-enum type '{}.{}'", module, name, expected_id.module, expected_id.name));
            }
            let (type_name, variant_spelling) = name.rsplit_once("::")
                .ok_or_else(|| format!("in module '{}': invalid enum pattern '{}'", module, name))?;
            let (variant_owner, variant) = if type_name.contains('.') {
                resolve_hir_enum_variant(project, module, type_name, variant_spelling)?
            } else {
                if type_name != expected_id.name {
                    return Err(format!("in module '{}': nested enum pattern '{}' names '{}', expected '{}'", module, name, type_name, expected_id.name));
                }
                resolve_hir_enum_variant(project, &expected_id.module, &expected_id.name, variant_spelling)?
            };
            if variant_owner != *expected_id {
                return Err(format!("in module '{}': nested enum pattern '{}' belongs to '{}.{}', expected '{}.{}'", module, name, variant_owner.module, variant_owner.name, expected_id.module, expected_id.name));
            }
            if bindings.len() != variant.fields.len() {
                return Err(format!("in module '{}': pattern '{}' expects {} binding(s), found {}", module, name, variant.fields.len(), bindings.len()));
            }
            for (binding, payload_type) in bindings.iter().zip(variant.fields.iter()) {
                let payload_id = resolve_nominal_spelling(project, &variant_owner.module, payload_type)?;
                bind_pattern_nominal_types(project, module, payload_id.as_ref(), binding, environment, output)?;
            }
        }
        P::Struct { name, fields, .. } => {
            let Some(expected_id) = expected else { return Ok(()); };
            let struct_id = resolve_hir_type_name(project, module, name)?
                .ok_or_else(|| format!("in module '{}': unknown struct pattern type '{}'", module, name))?;
            if struct_id != *expected_id || struct_id.kind != K::Struct {
                return Err(format!("in module '{}': nested struct pattern '{}' does not match expected type '{}.{}'", module, name, expected_id.module, expected_id.name));
            }
            let registry = hir_type_registry(project);
            let definition = registry.get(&struct_id)
                .ok_or_else(|| format!("missing HIR type metadata for '{}.{}'", struct_id.module, struct_id.name))?;
            let HirTypeShape::Struct { fields: declared_fields, .. } = &definition.shape else {
                return Err(format!("'{}.{}' is not a struct", struct_id.module, struct_id.name));
            };
            for (field_name, nested) in fields {
                let field = declared_fields.iter().find(|field| field.name == *field_name)
                    .ok_or_else(|| format!("in module '{}': struct '{}.{}' has no field '{}'", module, struct_id.module, struct_id.name, field_name))?;
                let field_id = resolve_nominal_spelling(project, &struct_id.module, &field.type_name)?;
                bind_pattern_nominal_types(project, module, field_id.as_ref(), nested, environment, output)?;
            }
        }
        P::Tuple(items) => {
            // Tuple element types are not represented in HirTypeShape yet.
            for item in items { bind_pattern_nominal_types(project, module, None, item, environment, output)?; }
        }
        P::Or(alternatives) => {
            // Each alternative gets an isolated environment. Only names with
            // the same nominal identity in every alternative escape the pattern.
            let mut common: Option<HashMap<String, crate::hir::HirDefinitionId>> = None;
            for alternative in alternatives {
                let mut branch = environment.clone();
                let mut ignored = Vec::new();
                bind_pattern_nominal_types(project, module, expected, alternative, &mut branch, &mut ignored)?;
                common = Some(match common {
                    None => branch,
                    Some(previous) => previous.into_iter().filter(|(name, id)| branch.get(name) == Some(id)).collect(),
                });
            }
            if let Some(common) = common {
                for (name, id) in common {
                    if environment.get(&name) != Some(&id) { bind_name(&name, &id, environment, output); }
                }
            }
        }
        P::Wildcard | P::Number(_) | P::Float(_) | P::String(_) | P::Boolean(_) | P::Range { .. } => {}
    }
    Ok(())
}

fn collect_pattern_bindings_in_statements(
    project: &Project, module: &str, statements: &[Statement],
    environment: &mut HashMap<String, crate::hir::HirDefinitionId>,
    output: &mut Vec<HirResolvedPatternBinding>,
) -> Result<(), String> {
    use crate::ast::Statement as S;
    for statement in statements {
        match statement {
            S::VariableDeclarations { declarations, .. } => for declaration in declarations {
                if let Some(value) = &declaration.value {
                    if let Some(id) = infer_hir_expression_type(project, module, value, environment)? { environment.insert(declaration.name.clone(), id); }
                    else { environment.remove(&declaration.name); }
                }
            },
            S::ConstDeclaration { name, value, .. } => {
                if let Some(id) = infer_hir_expression_type(project, module, value, environment)? { environment.insert(name.clone(), id); }
                else { environment.remove(name); }
            }
            S::Assignment { target, value, .. } => if let crate::ast::Expression::Identifier { name, .. } = target {
                if let Some(id) = infer_hir_expression_type(project, module, value, environment)? { environment.insert(name.clone(), id); }
                else { environment.remove(name); }
            },
            S::Match { expression, arms, .. } => {
                let scrutinee = infer_hir_expression_type(project, module, expression, environment)?;
                for arm in arms {
                    let mut branch = environment.clone();
                    bind_pattern_nominal_types(project, module, scrutinee.as_ref(), &arm.pattern, &mut branch, output)?;
                    collect_pattern_bindings_in_statements(project, module, &arm.body, &mut branch, output)?;
                }
            }
            S::Function { parameters, body, .. } => {
                let mut nested = HashMap::new();
                for parameter in parameters {
                    if let Some(type_name) = &parameter.type_name {
                        if let Some(id) = resolve_nominal_spelling(project, module, type_name)? { nested.insert(parameter.name.clone(), id); }
                    }
                }
                collect_pattern_bindings_in_statements(project, module, body, &mut nested, output)?;
            }
            S::Main { body, .. } => collect_pattern_bindings_in_statements(project, module, body, &mut HashMap::new(), output)?,
            S::Impl { methods, .. } => for method in methods { collect_pattern_bindings_in_statements(project, module, std::slice::from_ref(method), &mut HashMap::new(), output)?; },
            S::If { body, else_body, .. } => {
                let mut branch = environment.clone(); collect_pattern_bindings_in_statements(project, module, body, &mut branch, output)?;
                if let Some(body) = else_body { let mut branch = environment.clone(); collect_pattern_bindings_in_statements(project, module, body, &mut branch, output)?; }
            }
            S::While { body, .. } | S::For { body, .. } | S::ForEach { body, .. } => {
                let mut branch = environment.clone(); collect_pattern_bindings_in_statements(project, module, body, &mut branch, output)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn walk_expression_children(
    project: &Project, module: &str, expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>, output: &mut Vec<HirResolvedExpressionType>,
) -> Result<(), String> {
    use crate::ast::Expression as E;
    match expression {
        E::Array { elements, .. } | E::Tuple { elements, .. } => for item in elements { record_expression_type(project, module, item, environment, output)?; },
        E::Index { array, index, .. } | E::Binary { left: array, right: index, .. } => {
            record_expression_type(project, module, array, environment, output)?;
            record_expression_type(project, module, index, environment, output)?;
        }
        E::Property { object, .. } | E::Await { expression: object, .. } | E::Unary { expression: object, .. } | E::Conversion { expression: object, .. } => record_expression_type(project, module, object, environment, output)?,
        E::MethodCall { object, method, arguments, .. } => {
            record_expression_type(project, module, object, environment, output)?;
            if let Some(receiver_id) = infer_hir_expression_type(project, module, object, environment)? {
                resolve_hir_method_by_id(project, module, &receiver_id, method)?;
            }
            for argument in arguments { record_expression_type(project, module, argument, environment, output)?; }
        }
        E::Call { arguments, .. } | E::EnumConstructor { arguments, .. } => for argument in arguments { record_expression_type(project, module, argument, environment, output)?; },
        E::StructConstructor { fields, .. } => for (_, value) in fields { record_expression_type(project, module, value, environment, output)?; },
        E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. } | E::String { .. } | E::Identifier { .. } => {}
    }
    Ok(())
}

fn propagate_types_in_statements(
    project: &Project, module: &str, statements: &[Statement],
    environment: &mut HashMap<String, crate::hir::HirDefinitionId>, output: &mut Vec<HirResolvedExpressionType>,
) -> Result<(), String> {
    use crate::ast::Statement as S;
    for statement in statements {
        match statement {
            S::VariableDeclarations { declarations, .. } => for declaration in declarations {
                let inferred = if let Some(value) = &declaration.value {
                    record_expression_type(project, module, value, environment, output)?;
                    infer_hir_expression_type(project, module, value, environment)?
                } else { None };
                let declared = declaration.declared_type.as_deref().map(|name| resolve_nominal_spelling(project, module, name)).transpose()?.flatten();
                if let Some(id) = declared.or(inferred) { environment.insert(declaration.name.clone(), id); }
                else { environment.remove(&declaration.name); }
            },
            S::ConstDeclaration { name, declared_type, value, .. } => {
                record_expression_type(project, module, value, environment, output)?;
                let inferred = infer_hir_expression_type(project, module, value, environment)?;
                let declared = declared_type.as_deref().map(|name| resolve_nominal_spelling(project, module, name)).transpose()?.flatten();
                if let Some(id) = declared.or(inferred) { environment.insert(name.clone(), id); } else { environment.remove(name); }
            }
            S::Assignment { target, value, .. } => {
                record_expression_type(project, module, target, environment, output)?;
                record_expression_type(project, module, value, environment, output)?;
                if let crate::ast::Expression::Identifier { name, .. } = target {
                    if let Some(id) = infer_hir_expression_type(project, module, value, environment)? { environment.insert(name.clone(), id); } else { environment.remove(name); }
                }
            }
            S::Function { parameters, body, .. } => {
                let mut nested = HashMap::new();
                for parameter in parameters {
                    if let Some(type_name) = &parameter.type_name {
                        if let Some(id) = resolve_nominal_spelling(project, module, type_name)? { nested.insert(parameter.name.clone(), id); }
                    }
                }
                propagate_types_in_statements(project, module, body, &mut nested, output)?;
            }
            S::Main { body, .. } => propagate_types_in_statements(project, module, body, &mut HashMap::new(), output)?,
            S::Impl { methods, .. } => for method in methods { propagate_types_in_statements(project, module, std::slice::from_ref(method), &mut HashMap::new(), output)?; },
            S::If { condition, body, else_body, .. } => {
                record_expression_type(project, module, condition, environment, output)?;
                let mut branch = environment.clone(); propagate_types_in_statements(project, module, body, &mut branch, output)?;
                if let Some(body) = else_body { let mut branch = environment.clone(); propagate_types_in_statements(project, module, body, &mut branch, output)?; }
            }
            S::While { condition, body, .. } => { record_expression_type(project, module, condition, environment, output)?; let mut branch = environment.clone(); propagate_types_in_statements(project, module, body, &mut branch, output)?; }
            S::For { start, end, body, .. } => { record_expression_type(project, module, start, environment, output)?; record_expression_type(project, module, end, environment, output)?; let mut branch = environment.clone(); propagate_types_in_statements(project, module, body, &mut branch, output)?; }
            S::ForEach { iterable, body, .. } => { record_expression_type(project, module, iterable, environment, output)?; let mut branch = environment.clone(); propagate_types_in_statements(project, module, body, &mut branch, output)?; }
            S::Match { expression, arms, .. } => {
                record_expression_type(project, module, expression, environment, output)?;
                let scrutinee_type = infer_hir_expression_type(project, module, expression, environment)?;
                for arm in arms {
                    let mut branch = environment.clone();
                    let mut pattern_bindings = Vec::new();
                    bind_pattern_nominal_types(project, module, scrutinee_type.as_ref(), &arm.pattern, &mut branch, &mut pattern_bindings)?;
                    if let Some(guard) = &arm.guard { record_expression_type(project, module, guard, &branch, output)?; }
                    propagate_types_in_statements(project, module, &arm.body, &mut branch, output)?;
                }
            }
            S::Return { value: Some(value), .. } | S::Defer { expression: value, .. } | S::Expression { expression: value, .. } | S::Call { expression: value, .. } => record_expression_type(project, module, value, environment, output)?,
            S::Trait { .. } | S::Struct { .. } | S::Enum { .. } | S::Import { .. } | S::Return { value: None, .. } | S::Break { .. } | S::Continue { .. } => {}
        }
    }
    Ok(())
}

/// Validate nominal custom-type values at constructor and return boundaries.
/// This pass deliberately reports a mismatch only when both sides have a
/// proven HIR identity; primitive/generic inference remains the responsibility
/// of the legacy checker until the HIR type model represents those types.
pub fn validate_hir_nominal_value_types(project: &Project) -> Result<(), String> {
    for module in &project.modules {
        validate_nominal_statements(project, &module.name, &module.statements, &mut HashMap::new(), None)?;
    }
    Ok(())
}

fn check_nominal_value(
    project: &Project, module: &str, expected_module: &str, context: &str, expected_spelling: &str,
    expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
) -> Result<(), String> {
    let expected = resolve_nominal_spelling(project, expected_module, expected_spelling)?;
    let actual = infer_hir_expression_type(project, module, expression, environment)?;
    if let (Some(expected), Some(actual)) = (expected, actual) {
        if expected != actual {
            return Err(format!(
                "in module '{}': {} expects nominal type '{}.{}', found '{}.{}'",
                module, context, expected.module, expected.name, actual.module, actual.name
            ));
        }
    }
    Ok(())
}

fn validate_nominal_expression(
    project: &Project, module: &str, expression: &crate::ast::Expression,
    environment: &HashMap<String, crate::hir::HirDefinitionId>,
) -> Result<(), String> {
    use crate::ast::Expression as E;
    use crate::hir::HirTypeShape;
    match expression {
        E::StructConstructor { name, fields, .. } => {
            for (_, value) in fields { validate_nominal_expression(project, module, value, environment)?; }
            let Some(id) = resolve_hir_type_name(project, module, name)? else { return Ok(()); };
            let registry = hir_type_registry(project);
            let Some(definition) = registry.get(&id) else { return Err(format!("missing HIR type metadata for '{}.{}'", id.module, id.name)); };
            if let HirTypeShape::Struct { fields: declared, .. } = &definition.shape {
                for (field_name, value) in fields {
                    if let Some(field) = declared.iter().find(|field| field.name == *field_name) {
                        check_nominal_value(project, module, &id.module, &format!("field '{}.{}.{}'", id.module, id.name, field_name), &field.type_name, value, environment)?;
                    }
                }
            }
        }
        E::EnumConstructor { enum_name, variant, arguments, .. } => {
            for value in arguments { validate_nominal_expression(project, module, value, environment)?; }
            let (id, variant_info) = resolve_hir_enum_variant(project, module, enum_name, variant)?;
            for (index, (value, expected_type)) in arguments.iter().zip(variant_info.fields.iter()).enumerate() {
                check_nominal_value(project, module, &id.module, &format!("payload {} of '{}.{}::{}'", index + 1, id.module, id.name, variant), expected_type, value, environment)?;
            }
        }
        E::Array { elements, .. } | E::Tuple { elements, .. } => for value in elements { validate_nominal_expression(project, module, value, environment)?; },
        E::Index { array, index, .. } | E::Binary { left: array, right: index, .. } => {
            validate_nominal_expression(project, module, array, environment)?;
            validate_nominal_expression(project, module, index, environment)?;
        }
        E::Property { object, .. } | E::Await { expression: object, .. } | E::Unary { expression: object, .. } | E::Conversion { expression: object, .. } => validate_nominal_expression(project, module, object, environment)?,
        E::MethodCall { object, arguments, .. } => {
            validate_nominal_expression(project, module, object, environment)?;
            for value in arguments { validate_nominal_expression(project, module, value, environment)?; }
        }
        E::Call { arguments, .. } => for value in arguments { validate_nominal_expression(project, module, value, environment)?; },
        E::Number { .. } | E::WideNumber { .. } | E::Float { .. } | E::Boolean { .. } | E::String { .. } | E::Identifier { .. } => {}
    }
    Ok(())
}

fn validate_nominal_statements(
    project: &Project, module: &str, statements: &[Statement],
    environment: &mut HashMap<String, crate::hir::HirDefinitionId>,
    return_type: Option<&str>,
) -> Result<(), String> {
    use crate::ast::{Expression as E, Statement as S};
    for statement in statements {
        match statement {
            S::VariableDeclarations { declarations, .. } => for declaration in declarations {
                if let Some(value) = &declaration.value {
                    validate_nominal_expression(project, module, value, environment)?;
                    let declared = declaration.declared_type.as_deref().map(|n| resolve_nominal_spelling(project, module, n)).transpose()?.flatten();
                    let inferred = infer_hir_expression_type(project, module, value, environment)?;
                    if let (Some(expected), Some(actual)) = (&declared, &inferred) {
                        if expected != actual { return Err(format!("in module '{}': variable '{}' declares '{}.{}' but is initialized with '{}.{}'", module, declaration.name, expected.module, expected.name, actual.module, actual.name)); }
                    }
                    if let Some(id) = declared.or(inferred) { environment.insert(declaration.name.clone(), id); }
                    else { environment.remove(&declaration.name); }
                }
            },
            S::ConstDeclaration { name, declared_type, value, .. } => {
                validate_nominal_expression(project, module, value, environment)?;
                if let Some(expected_name) = declared_type { check_nominal_value(project, module, module, &format!("constant '{}'", name), expected_name, value, environment)?; }
                if let Some(id) = declared_type.as_deref().map(|n| resolve_nominal_spelling(project, module, n)).transpose()?.flatten().or(infer_hir_expression_type(project, module, value, environment)?) { environment.insert(name.clone(), id); }
            }
            S::Assignment { target, value, .. } => {
                validate_nominal_expression(project, module, value, environment)?;
                if let E::Identifier { name, .. } = target { if let Some(expected) = environment.get(name) {
                    if let Some(actual) = infer_hir_expression_type(project, module, value, environment)? { if expected != &actual { return Err(format!("in module '{}': assignment to '{}' changes nominal type from '{}.{}' to '{}.{}'", module, name, expected.module, expected.name, actual.module, actual.name)); } }
                } }
            }
            S::Function { parameters, return_type: declared_return, body, .. } => {
                let mut nested = HashMap::new();
                for parameter in parameters { if let Some(type_name) = &parameter.type_name { if let Some(id) = resolve_nominal_spelling(project, module, type_name)? { nested.insert(parameter.name.clone(), id); } } }
                validate_nominal_statements(project, module, body, &mut nested, declared_return.as_deref())?;
            }
            S::Main { body, .. } => validate_nominal_statements(project, module, body, &mut HashMap::new(), None)?,
            S::Impl { methods, .. } => for method in methods { validate_nominal_statements(project, module, std::slice::from_ref(method), &mut HashMap::new(), None)?; },
            S::Return { value: Some(value), .. } => { validate_nominal_expression(project, module, value, environment)?; if let Some(expected) = return_type { check_nominal_value(project, module, module, "function return value", expected, value, environment)?; } },
            S::Expression { expression, .. } | S::Call { expression, .. } | S::Defer { expression, .. } => validate_nominal_expression(project, module, expression, environment)?,
            S::If { condition, body, else_body, .. } => {
                validate_nominal_expression(project, module, condition, environment)?;
                let mut branch = environment.clone(); validate_nominal_statements(project, module, body, &mut branch, return_type)?;
                if let Some(body) = else_body { let mut branch = environment.clone(); validate_nominal_statements(project, module, body, &mut branch, return_type)?; }
            }
            S::While { condition, body, .. } => { validate_nominal_expression(project, module, condition, environment)?; let mut branch = environment.clone(); validate_nominal_statements(project, module, body, &mut branch, return_type)?; }
            S::For { start, end, body, .. } => { validate_nominal_expression(project, module, start, environment)?; validate_nominal_expression(project, module, end, environment)?; let mut branch = environment.clone(); validate_nominal_statements(project, module, body, &mut branch, return_type)?; }
            S::ForEach { iterable, body, .. } => { validate_nominal_expression(project, module, iterable, environment)?; let mut branch = environment.clone(); validate_nominal_statements(project, module, body, &mut branch, return_type)?; }
            S::Match { expression, arms, .. } => { validate_nominal_expression(project, module, expression, environment)?; for arm in arms { let mut branch = environment.clone(); if let Some(guard) = &arm.guard { validate_nominal_expression(project, module, guard, &branch)?; } validate_nominal_statements(project, module, &arm.body, &mut branch, return_type)?; } }
            S::Trait { .. } | S::Struct { .. } | S::Enum { .. } | S::Import { .. } | S::Return { value: None, .. } | S::Break { .. } | S::Continue { .. } => {}
        }
    }
    Ok(())
}

fn find_exported_definition_provider<'a>(
    modules: &'a [ModuleUnit],
    module: &'a ModuleUnit,
    name: &str,
    kind: crate::hir::HirDefinitionKind,
    visited: &mut HashSet<String>,
) -> Option<&'a ModuleUnit> {
    use crate::hir::HirDefinitionKind as Kind;
    if !visited.insert(module.name.clone()) || !module.public_symbols.contains(name) {
        return None;
    }

    let locally_declares = match kind {
        Kind::Function => module.function_symbols.contains(name),
        Kind::Struct => module.statements.iter().any(|statement| matches!(statement, Statement::Struct { name: declared, .. } if declared == name)),
        Kind::Enum => module.statements.iter().any(|statement| matches!(statement, Statement::Enum { name: declared, .. } if declared == name)),
        Kind::Trait => module.statements.iter().any(|statement| matches!(statement, Statement::Trait { name: declared, .. } if declared == name)),
        Kind::Constant => module.statements.iter().any(|statement| matches!(statement, Statement::ConstDeclaration { name: declared, .. } if declared == name)),
        Kind::Method | Kind::Field | Kind::Variant => false,
    };
    if locally_declares && module.local_public_symbols.contains(name) {
        return Some(module);
    }

    for import in module.imports.iter().filter(|import| reexport_allows(import, name)) {
        if let Some(target) = resolve_import_target(modules, module, &import.path) {
            if let Some(provider) = find_exported_definition_provider(modules, target, name, kind, visited) {
                return Some(provider);
            }
        }
    }
    None
}

#[cfg(test)]
mod qualified_hir_resolution_tests {
    use super::*;
    use crate::hir::HirDefinitionKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-resolve-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn qualified_lookup_resolves_alias_to_original_module_identity() {
        let temp = TempProject::new();
        temp.write("main.fusion", "import geometry as g\nmain:\n    print(1)\n");
        temp.write("geometry.fusion", "pub struct Point { x: num }\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();

        let resolved = resolve_qualified_definition(
            &project, "crate", "g", "Point", HirDefinitionKind::Struct,
        ).unwrap();
        assert_eq!(resolved.module, "geometry");
        assert_eq!(resolved.name, "Point");
        assert_eq!(resolved.kind, HirDefinitionKind::Struct);
    }

    #[test]
    fn qualified_lookup_follows_public_reexports_to_declaration_owner() {
        let temp = TempProject::new();
        temp.write("main.fusion", "import facade as f\nmain:\n    print(1)\n");
        temp.write("facade.fusion", "pub import geometry\n");
        temp.write("geometry.fusion", "pub struct Point { x: num }\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();

        let resolved = resolve_qualified_definition(
            &project, "crate", "f", "Point", HirDefinitionKind::Struct,
        ).unwrap();
        assert_eq!(resolved.module, "geometry");
    }

    #[test]
    fn qualified_lookup_rejects_private_declarations() {
        let temp = TempProject::new();
        temp.write("main.fusion", "import geometry as g\nmain:\n    print(1)\n");
        temp.write("geometry.fusion", "struct Point { x: num }\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();

        let error = resolve_qualified_definition(
            &project, "crate", "g", "Point", HirDefinitionKind::Struct,
        ).unwrap_err();
        assert!(error.contains("private or does not exist"));
    }

    #[test]
    fn method_resolution_uses_receiver_identity_and_prefers_inherent_methods() {
        let p = TempProject::new();
        p.write("main.fusion", "import model\nmain:\n    print(1)\n");
        p.write("model.fusion", "pub struct Thing { value: num }\nimpl Thing:\n    fn read(self: Thing) -> num:\n        return self.value\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let resolved = resolve_hir_method(&project, "crate", "model.Thing", "read").unwrap();
        assert_eq!(resolved.module, "model");
        assert_eq!(resolved.name, "read");
        assert_eq!(resolved.scope, vec!["inherent".to_string(), "model::Thing".to_string()]);
    }

    #[test]
    fn package_qualified_trait_dispatch_survives_three_level_selective_reexports() {
        use crate::hir::HirDefinitionKind;
        let p = TempProject::new();
        p.write("app/main.fusion", "import facade as f\nimpl f.Readable for f.Point:\n    fn read(self: f.Point) -> num:\n        return self.value\nfn consume(point: f.Point) -> num:\n    return point.read()\nmain:\n    print(1)\n");
        p.write("deps/facade/src/lib.fusion", "pub import api\n");
        p.write("deps/facade/src/api.fusion", "pub from contracts import Readable\npub from models import Point\n");
        p.write("deps/facade/src/contracts.fusion", "pub trait Readable { fn read() -> num }\npub trait Internal { fn hidden() -> num }\n");
        p.write("deps/facade/src/models.fusion", "pub struct Point { value: num }\npub struct PrivatePoint { value: num }\n");

        let mut roots = BTreeMap::new();
        roots.insert("facade".to_string(), p.0.join("deps/facade/src"));
        let project = load_project_with_package_roots(p.0.join("app/main.fusion"), roots).unwrap();

        let trait_id = resolve_qualified_definition(
            &project, "crate", "f", "Readable", HirDefinitionKind::Trait,
        ).unwrap();
        let type_id = resolve_qualified_definition(
            &project, "crate", "f", "Point", HirDefinitionKind::Struct,
        ).unwrap();
        assert_eq!(trait_id.module, "facade.contracts");
        assert_eq!(type_id.module, "facade.models");

        assert!(resolve_qualified_definition(
            &project, "crate", "f", "Internal", HirDefinitionKind::Trait,
        ).is_err());
        assert!(resolve_qualified_definition(
            &project, "crate", "f", "PrivatePoint", HirDefinitionKind::Struct,
        ).is_err());

        let method = resolve_hir_method(&project, "crate", "f.Point", "read").unwrap();
        assert_eq!(method.module, "crate");
        assert_eq!(method.name, "read");
        assert_eq!(method.scope[0], "facade.contracts::Readable");
        assert_eq!(method.scope[1], "facade.models::Point");

        // Exercise call-site resolution consumed by HIR lowering and backend
        // dispatch, not only the standalone symbol lookup helper.
        let calls = resolve_hir_calls(&project).unwrap();
        let dispatched = calls.iter().find(|call| call.kind == crate::hir::HirCallKind::Method
            && call.declaration.name == "read").expect("package-qualified trait method call should resolve");
        assert_eq!(dispatched.declaration, method);
        assert_eq!(dispatched.receiver_type.as_ref(), Some(&type_id));

        // Run the complete compiler project pipeline with the same package root.
        let compiled = crate::compile_project_with_package_roots(
            p.0.join("app/main.fusion"),
            { let mut roots = BTreeMap::new(); roots.insert("facade".to_string(), p.0.join("deps/facade/src")); roots },
        );
        assert!(compiled.is_ok(), "package-qualified trait method should compile: {:?}", compiled.err());
    }
}



#[cfg(test)]
mod unqualified_hir_resolution_tests {
    use super::*;
    use crate::hir::HirDefinitionKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-unqualified-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn selective_import_resolves_to_original_definition_identity() {
        let temp = TempProject::new();
        temp.write("main.fusion", "from geometry import Point\nmain:\n    print(1)\n");
        temp.write("geometry.fusion", "pub fn Point() -> num:\n    return 1\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();
        let resolved = resolve_unqualified_definition(&project, "crate", "Point", HirDefinitionKind::Function).unwrap();
        assert_eq!(resolved.module, "geometry");
        assert_eq!(resolved.name, "Point");
    }

    #[test]
    fn selective_import_reports_ambiguity_instead_of_choosing_a_provider() {
        let temp = TempProject::new();
        temp.write("main.fusion", "from geometry import Point\nfrom graphics import Point\nmain:\n    print(1)\n");
        temp.write("geometry.fusion", "pub fn Point() -> num:\n    return 1\n");
        temp.write("graphics.fusion", "pub fn Point() -> num:\n    return 2\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();
        let error = resolve_unqualified_definition(&project, "crate", "Point", HirDefinitionKind::Function).unwrap_err();
        assert!(error.contains("ambiguous imported"));
        assert!(error.contains("geometry"));
        assert!(error.contains("graphics"));
    }

    #[test]
    fn local_declaration_shadows_selective_import_for_hir_lookup() {
        let temp = TempProject::new();
        temp.write("main.fusion", "from geometry import Point\nfn Point() -> num:\n    return 3\nmain:\n    print(1)\n");
        temp.write("geometry.fusion", "pub fn Point() -> num:\n    return 1\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();
        let resolved = resolve_unqualified_definition(&project, "crate", "Point", HirDefinitionKind::Function).unwrap();
        assert_eq!(resolved.module, "crate");
    }
}

#[cfg(test)]
mod hir_type_registry_tests {
    use super::*;
    use crate::hir::{HirDefinitionId, HirDefinitionKind, HirTypeShape};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-types-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn registry_captures_struct_fields_enum_variants_and_visibility() {
        let temp = TempProject::new();
        temp.write("main.fusion", "import geometry\nmain:\n    print(1)\n");
        temp.write("geometry.fusion", "pub struct Point { x: num, y: num }\nenum Color { Red, Rgb(u8, u8, u8) }\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();
        let registry = hir_type_registry(&project);

        let point = registry.get(&HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct)).unwrap();
        assert!(point.is_public);
        match &point.shape {
            HirTypeShape::Struct { fields, .. } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "x");
                assert_eq!(fields[1].type_name, "num");
            }
            _ => panic!("expected struct metadata"),
        }

        let color = registry.get(&HirDefinitionId::new("geometry", "Color", HirDefinitionKind::Enum)).unwrap();
        assert!(!color.is_public);
        match &color.shape {
            HirTypeShape::Enum { variants } => {
                assert_eq!(variants.len(), 2);
                assert_eq!(variants[0].name, "Red");
                assert_eq!(variants[1].fields, vec!["u8", "u8", "u8"]);
            }
            _ => panic!("expected enum metadata"),
        }
    }
}


#[cfg(test)]
mod hir_type_name_resolution_tests {
    use super::*;
    use crate::hir::{HirDefinitionId, HirDefinitionKind};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-type-name-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn qualified_type_name_resolves_through_import_alias() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        assert_eq!(resolve_hir_type_name(&project, "crate", "geo.Point").unwrap(), Some(
            HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct)));
    }

    #[test]
    fn selectively_imported_type_resolves_to_its_owner() {
        let p = TempProject::new();
        p.write("main.fusion", "from geometry import Point\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        assert_eq!(resolve_hir_type_name(&project, "crate", "Point").unwrap(), Some(
            HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct)));
    }

    #[test]
    fn builtins_are_not_reported_as_module_declarations() {
        let p = TempProject::new();
        p.write("main.fusion", "main:\n    print(1)\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        assert_eq!(resolve_hir_type_name(&project, "crate", "num").unwrap(), None);
    }
}


#[cfg(test)]
mod hir_member_resolution_tests {
    use super::*;
    use crate::hir::{HirDefinitionId, HirDefinitionKind};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-members-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn resolves_struct_fields_through_module_alias() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "pub struct Point { x: num, label: string }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let (id, field) = resolve_hir_struct_field(&project, "crate", "geo.Point", "label").unwrap();
        assert_eq!(id, HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct));
        assert_eq!(field.type_name, "string");
        assert!(resolve_hir_struct_field(&project, "crate", "geo.Point", "missing").is_err());
    }

    #[test]
    fn resolves_enum_variants_and_preserves_payload_order() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "pub enum Color { Rgb(u8, u8, u8), Named(string) }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let (id, variant) = resolve_hir_enum_variant(&project, "crate", "geo.Color", "Rgb").unwrap();
        assert_eq!(id, HirDefinitionId::new("geometry", "Color", HirDefinitionKind::Enum));
        assert_eq!(variant.fields, vec!["u8", "u8", "u8"]);
    }

    #[test]
    fn member_lookup_rejects_private_imported_type() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "struct Point { x: num }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        assert!(resolve_hir_struct_field(&project, "crate", "geometry.Point", "x").is_err());
    }
}

#[cfg(test)]
mod hir_type_reference_validation_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-type-refs-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn validates_qualified_types_inside_structs_and_enum_payloads() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nstruct Envelope { point: geo.Point, samples: Array<geo.Point> }\nenum Event { Moved(geo.Point) }\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        validate_hir_type_references(&project).unwrap();
    }

    #[test]
    fn rejects_unknown_type_in_struct_field_with_context() {
        let p = TempProject::new();
        p.write("main.fusion", "struct Envelope { point: MissingPoint }\nmain:\n    print(1)\n");
        let project = load_project_with_metadata(p.0.join("main.fusion")).unwrap();
        let error = validate_hir_type_references(&project).unwrap_err();
        assert!(error.contains("Envelope"));
        assert!(error.contains("point"));
        assert!(error.contains("MissingPoint"));
    }

    #[test]
    fn rejects_private_type_referenced_through_module_alias() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nstruct Envelope { point: geo.Hidden }\nmain:\n    print(1)\n");
        p.write("geometry.fusion", "struct Hidden { x: num }\n");
        let error = load_project_with_metadata(p.0.join("main.fusion")).unwrap_err();
        assert!(error.contains("Hidden"));
    }
}

#[cfg(test)]
mod hir_constructor_resolution_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-constructors-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        fn load(&self) -> Project { load_project_with_metadata(self.0.join("main.fusion")).unwrap() }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn resolves_qualified_struct_constructor_to_original_declaration() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let point = geo.Point { x: 3, y: 4 }\n");
        p.write("geometry.fusion", "pub struct Point:\n    x: num\n    y: num\n");
        let project = p.load();
        let resolved = resolve_hir_type_expressions(&project).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].definition, crate::hir::HirDefinitionId::new("geometry", "Point", crate::hir::HirDefinitionKind::Struct));
        assert_eq!(resolved[0].kind, HirResolvedTypeExpressionKind::Struct { supplied_fields: vec!["x".into(), "y".into()] });
    }

    #[test]
    fn resolves_enum_constructor_and_retains_payload_types() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let color = geo.Color::Rgb(1, 2, 3)\n");
        p.write("geometry.fusion", "pub enum Color:\n    Rgb(u8, u8, u8)\n    Named(string)\n");
        let project = p.load();
        let resolved = resolve_hir_type_expressions(&project).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].definition, crate::hir::HirDefinitionId::new("geometry", "Color", crate::hir::HirDefinitionKind::Enum));
        assert_eq!(resolved[0].kind, HirResolvedTypeExpressionKind::EnumVariant {
            variant: "Rgb".into(), payload_types: vec!["u8".into(), "u8".into(), "u8".into()],
        });
    }

    #[test]
    fn rejects_unknown_and_duplicate_struct_constructor_fields() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let point = geo.Point { x: 1, missing: 2 }\n");
        p.write("geometry.fusion", "pub struct Point:\n    x: num\n");
        let error = resolve_hir_type_expressions(&p.load()).unwrap_err();
        assert!(error.contains("no field 'missing'"), "{error}");

        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let point = geo.Point { x: 1, x: 2 }\n");
        p.write("geometry.fusion", "pub struct Point:\n    x: num\n");
        let error = resolve_hir_type_expressions(&p.load()).unwrap_err();
        assert!(error.contains("duplicate field 'x'"), "{error}");
    }

    #[test]
    fn rejects_missing_struct_fields_and_wrong_enum_payload_arity() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let point = geo.Point { x: 1 }\n");
        p.write("geometry.fusion", "pub struct Point:\n    x: num\n    y: num\n");
        let error = resolve_hir_type_expressions(&p.load()).unwrap_err();
        assert!(error.contains("missing field 'y'"), "{error}");

        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let color = geo.Color::Rgb(1, 2)\n");
        p.write("geometry.fusion", "pub enum Color:\n    Rgb(u8, u8, u8)\n");
        let error = resolve_hir_type_expressions(&p.load()).unwrap_err();
        assert!(error.contains("expects 3 payload value(s), found 2"), "{error}");
    }
}

#[cfg(test)]
mod hir_expression_member_resolution_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-members-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
        fn write(&self, name: &str, source: &str) {
            let path = self.0.join(name); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, source).unwrap();
        }
        fn load(&self) -> Project { load_project_with_metadata(self.0.join("main.fusion")).unwrap() }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn field_access_on_constructor_initialized_local_resolves_module_identity() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let point = geo.Point { x: 1, y: 2 }\n    print(point.x)\n");
        p.write("geometry.fusion", "pub struct Point { x: num, y: num }\n");
        let project = p.load();
        let (_, fields) = resolve_hir_pattern_and_field_expressions(&project).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].definition.module, "geometry");
        assert_eq!(fields[0].definition.name, "Point");
        assert_eq!(fields[0].field.name, "x");
    }

    #[test]
    fn field_access_rejects_unknown_field_on_known_receiver() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let point = geo.Point { x: 1 }\n    print(point.missing)\n");
        p.write("geometry.fusion", "pub struct Point { x: num }\n");
        let error = resolve_hir_pattern_and_field_expressions(&p.load()).unwrap_err();
        assert!(error.contains("has no field 'missing'"), "{error}");
    }

    #[test]
    fn enum_pattern_on_constructor_initialized_local_resolves_variant_identity() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let color = geo.Color::Rgb(1, 2, 3)\n    match color:\n        geo.Color::Rgb(r, g, b) => print(r)\n");
        p.write("geometry.fusion", "pub enum Color { Rgb(num, num, num), Black }\n");
        let project = p.load();
        let (patterns, _) = resolve_hir_pattern_and_field_expressions(&project).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].definition.module, "geometry");
        assert_eq!(patterns[0].definition.name, "Color");
        assert!(matches!(&patterns[0].kind, HirResolvedPatternKind::EnumVariant { variant, binding_count: 3, .. } if variant == "Rgb"));
    }

    #[test]
    fn enum_pattern_rejects_wrong_binding_count() {
        let p = TempProject::new();
        p.write("main.fusion", "import geometry as geo\nmain:\n    let color = geo.Color::Rgb(1, 2, 3)\n    match color:\n        geo.Color::Rgb(r, g) => print(r)\n");
        p.write("geometry.fusion", "pub enum Color { Rgb(num, num, num) }\n");
        let error = resolve_hir_pattern_and_field_expressions(&p.load()).unwrap_err();
        assert!(error.contains("expects 3 binding(s), found 2"), "{error}");
    }
}

#[cfg(test)]
mod hir_expression_type_propagation_tests {
    use super::*;
    use crate::hir::HirDefinitionKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-type-flow-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, relative: &str, source: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn propagates_nominal_type_from_imported_function_return_through_nested_fields() {
        let temp = TempProject::new();
        temp.write("main.fusion", "import geometry as g\nmain:\n    let person = g.make_person()\n    print(person.address.city.code)\n");
        temp.write("geometry.fusion", "pub struct City { code: num }\npub struct Address { city: City }\npub struct Person { address: Address }\npub fn make_person() -> Person:\n    return Person { address: Address { city: City { code: 7 } } }\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();
        let types = propagate_hir_expression_types(&project).unwrap();
        let names: HashSet<_> = types.iter().map(|item| item.definition.name.as_str()).collect();
        assert!(names.contains("Person"));
        assert!(names.contains("Address"));
        assert!(names.contains("City"));
        assert!(types.iter().any(|item| item.definition.module == "geometry" && item.definition.kind == HirDefinitionKind::Struct));
    }

    #[test]
    fn propagates_parameter_type_into_nested_property_chain() {
        let temp = TempProject::new();
        temp.write("main.fusion", "import geometry\nmain:\n    print(1)\n");
        temp.write("geometry.fusion", "pub struct City { code: num }\npub struct Address { city: City }\npub struct Person { address: Address }\npub fn inspect(person: Person) -> num:\n    return person.address.city.code\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();
        let types = propagate_hir_expression_types(&project).unwrap();
        assert!(types.iter().any(|item| item.definition.name == "Address"));
        assert!(types.iter().any(|item| item.definition.name == "City"));
    }

    #[test]
    fn explicit_nominal_local_type_is_available_to_following_expressions() {
        let temp = TempProject::new();
        temp.write("main.fusion", "import geometry as g\nmain:\n    let person: g.Person = g.make_person()\n    print(person.address)\n");
        temp.write("geometry.fusion", "pub struct Address { code: num }\npub struct Person { address: Address }\npub fn make_person() -> Person:\n    return Person { address: Address { code: 1 } }\n");
        let project = load_project_with_metadata(temp.0.join("main.fusion")).unwrap();
        let types = propagate_hir_expression_types(&project).unwrap();
        assert!(types.iter().any(|item| item.definition.name == "Person"));
        assert!(types.iter().any(|item| item.definition.name == "Address"));
    }
}


#[cfg(test)]
mod hir_pattern_binding_type_tests {
    use super::*;
    use crate::hir::HirDefinitionKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-pattern-binding-types-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
        fn write(&self, name: &str, source: &str) {
            let path = self.0.join(name); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, source).unwrap();
        }
        fn load(&self) -> Project { load_project_with_metadata(self.0.join("main.fusion")).unwrap() }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn enum_payload_binding_propagates_nominal_identity_into_nested_fields() {
        let p = TempProject::new();
        p.write("main.fusion", "import domain as d\nmain:\n    let result = d.Outcome::Success(d.Person { address: d.Address { city: d.City { code: 7 } } })\n    match result:\n        d.Outcome::Success(person) => print(person.address.city.code)\n        d.Outcome::Failure(message) => print(message)\n");
        p.write("domain.fusion", "pub struct City { code: num }\npub struct Address { city: City }\npub struct Person { address: Address }\npub enum Outcome { Success(Person), Failure(string) }\n");
        let project = p.load();
        let bindings = resolve_hir_pattern_binding_types(&project).unwrap();
        assert!(bindings.iter().any(|binding| binding.name == "person" && binding.definition == crate::hir::HirDefinitionId::new("domain", "Person", HirDefinitionKind::Struct)));
        assert!(!bindings.iter().any(|binding| binding.name == "message"), "builtin payload types should not be recorded as nominal identities");
        let types = propagate_hir_expression_types(&project).unwrap();
        assert!(types.iter().any(|item| item.definition.name == "Address"));
        assert!(types.iter().any(|item| item.definition.name == "City"));
    }

    #[test]
    fn nested_enum_payload_pattern_binds_inner_custom_type() {
        let p = TempProject::new();
        p.write("main.fusion", "import domain as d\nmain:\n    let result = d.Outer::Wrap(d.Inner::Value(d.Person { name: \"Ada\" }))\n    match result:\n        d.Outer::Wrap(d.Inner::Value(person)) => print(person.name)\n");
        p.write("domain.fusion", "pub struct Person { name: string }\npub enum Inner { Value(Person) }\npub enum Outer { Wrap(Inner) }\n");
        let project = p.load();
        let bindings = resolve_hir_pattern_binding_types(&project).unwrap();
        assert!(bindings.iter().any(|binding| binding.name == "person" && binding.definition.name == "Person"));
    }

    #[test]
    fn struct_pattern_field_binding_propagates_field_type() {
        let p = TempProject::new();
        p.write("main.fusion", "import domain as d\nmain:\n    let person = d.Person { address: d.Address { city: d.City { code: 1 } } }\n    match person:\n        d.Person { address: address } => print(address.city.code)\n");
        p.write("domain.fusion", "pub struct City { code: num }\npub struct Address { city: City }\npub struct Person { address: Address }\n");
        let project = p.load();
        let bindings = resolve_hir_pattern_binding_types(&project).unwrap();
        assert!(bindings.iter().any(|binding| binding.name == "address" && binding.definition.name == "Address"));
        let types = propagate_hir_expression_types(&project).unwrap();
        assert!(types.iter().any(|item| item.definition.name == "City"));
    }

    #[test]
    fn nested_pattern_rejects_variant_from_wrong_payload_enum() {
        let p = TempProject::new();
        p.write("main.fusion", "import domain as d\nmain:\n    let result = d.Outer::Wrap(d.Inner::Value(d.Person { name: \"Ada\" }))\n    match result:\n        d.Outer::Wrap(d.Other::Missing(person)) => print(1)\n");
        p.write("domain.fusion", "pub struct Person { name: string }\npub enum Inner { Value(Person) }\npub enum Other { Missing(Person) }\npub enum Outer { Wrap(Inner) }\n");
        let error = resolve_hir_pattern_binding_types(&p.load()).unwrap_err();
        assert!(error.contains("nested enum pattern") || error.contains("belongs to"), "{error}");
    }
}

#[cfg(test)]
mod hir_nominal_value_validation_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-nominal-values-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
        fn write(&self, name: &str, source: &str) {
            let path = self.0.join(name); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, source).unwrap();
        }
        fn load(&self) -> Project { load_project_with_metadata(self.0.join("main.fusion")).unwrap() }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn rejects_struct_field_initialized_with_different_nominal_type() {
        let p = TempProject::new();
        p.write("main.fusion", "import models\nmain:\n    let holder = models.Holder { item: models.Address { city: 1 } }\n");
        p.write("models.fusion", "pub struct City { code: num }\npub struct Address { city: num }\npub struct Holder { item: City }\n");
        let error = validate_hir_nominal_value_types(&p.load()).unwrap_err();
        assert!(error.contains("expects nominal type 'models.City'"), "{error}");
    }

    #[test]
    fn rejects_enum_payload_initialized_with_different_nominal_type() {
        let p = TempProject::new();
        p.write("main.fusion", "import models\nmain:\n    let value = models.Event::Created(models.Address { city: 1 })\n");
        p.write("models.fusion", "pub struct City { code: num }\npub struct Address { city: num }\npub enum Event { Created(City) }\n");
        let error = validate_hir_nominal_value_types(&p.load()).unwrap_err();
        assert!(error.contains("expects nominal type 'models.City'"), "{error}");
    }

    #[test]
    fn rejects_function_return_with_different_nominal_type() {
        let p = TempProject::new();
        p.write("main.fusion", "import models\nmain:\n    print(1)\n");
        p.write("models.fusion", "pub struct City { code: num }\npub struct Address { city: num }\npub fn make_city() -> City:\n    return Address { city: 1 }\n");
        let error = validate_hir_nominal_value_types(&p.load()).unwrap_err();
        assert!(error.contains("function return value expects nominal type 'models.City'"), "{error}");
    }

    #[test]
    fn accepts_matching_nominal_values_across_imports() {
        let p = TempProject::new();
        p.write("main.fusion", "import models as m\nmain:\n    let holder = m.Holder { item: m.City { code: 7 } }\n    let event = m.Event::Created(m.City { code: 8 })\n");
        p.write("models.fusion", "pub struct City { code: num }\npub struct Holder { item: City }\npub enum Event { Created(City) }\n");
        validate_hir_nominal_value_types(&p.load()).unwrap();
    }
}


#[cfg(test)]
mod backend_type_identity_tests {
    use super::*;
    use crate::ast::{ModuleInfo, Program, Statement};
    use crate::span::Span;

    #[test]
    fn backend_type_symbols_restore_distinct_module_identities() {
        let geometry_name = mangle_type("geometry", "Point");
        let graphics_name = mangle_type("graphics", "Point");
        let program = Program {
            statements: vec![
                Statement::Struct { name: geometry_name.clone(), fields: Vec::new(), derives: Vec::new(), span: Span::default() },
                Statement::Struct { name: graphics_name.clone(), fields: Vec::new(), derives: Vec::new(), span: Span::default() },
            ],
            span: Span::default(),
            modules: vec![
                ModuleInfo { name: "geometry".into(), path: "geometry.fusion".into(), statement_start: 0, statement_end: 1, imports: Vec::new(), public_symbols: vec!["Point".into()], private_symbols: Vec::new() },
                ModuleInfo { name: "graphics".into(), path: "graphics.fusion".into(), statement_start: 1, statement_end: 2, imports: Vec::new(), public_symbols: vec!["Point".into()], private_symbols: Vec::new() },
            ],
        };

        let identities = backend_type_definition_ids(&program);
        let geometry = identities.get(&geometry_name).expect("geometry identity");
        let graphics = identities.get(&graphics_name).expect("graphics identity");
        assert_eq!(geometry.module, "geometry");
        assert_eq!(graphics.module, "graphics");
        assert_eq!(geometry.name, "Point");
        assert_eq!(graphics.name, "Point");
        assert_ne!(geometry, graphics);
    }
}

#[cfg(test)]
mod hir_trait_signature_validation_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-trait-signatures-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, source: &str) { fs::write(self.0.join("main.fusion"), source).unwrap(); }
        fn load(&self) -> Project { load_project_with_metadata(self.0.join("main.fusion")).unwrap() }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn trait_impl_accepts_explicit_receiver_as_abi_only_parameter() {
        let project = TempProject::new();
        project.write("struct Item { value: num }\ntrait Readable { fn read() -> num }\nimpl Readable for Item { fn read(self) -> num { return 1 } }\nmain: print(1)\n");
        validate_hir_trait_implementations(&project.load()).expect("explicit self must not count as a trait argument");
    }

    #[test]
    fn trait_impl_rejects_receiver_typed_as_another_nominal_type() {
        let project = TempProject::new();
        project.write("struct Item { value: num }\nstruct Other { value: num }\ntrait Readable { fn read() -> num }\nimpl Readable for Item { fn read(self: Other) -> num { return 1 } }\nmain: print(1)\n");
        let error = validate_hir_trait_implementations(&project.load()).unwrap_err();
        assert!(error.contains("matching non-receiver parameters"), "{error}");
    }
}


#[cfg(test)]
mod hir_structural_type_tests {
    use super::*;

    #[test]
    fn generic_unification_and_substitution_cover_nested_containers() {
        use crate::hir::HirType as T;
        let expected = T::Array(Box::new(T::Generic("Item".into())));
        let actual = T::Array(Box::new(T::Tuple(vec![T::Num, T::String])));
        let mut bindings = HashMap::new();
        assert!(unify_hir_call_type(&expected, &actual, &mut bindings));
        assert_eq!(substitute_hir_generics(expected, &bindings), actual);
    }

    #[test]
    fn generic_unification_rejects_inconsistent_repeated_parameters() {
        use crate::hir::HirType as T;
        let expected = T::Tuple(vec![T::Generic("T".into()), T::Generic("T".into())]);
        let actual = T::Tuple(vec![T::Num, T::String]);
        assert!(!unify_hir_call_type(&expected, &actual, &mut HashMap::new()));
    }

    fn empty_project() -> Project {
        Project {
            entry_module: "main".into(),
            modules: Vec::new(),
            program: crate::ast::Program { statements: Vec::new(), span: crate::span::Span::default(), modules: Vec::new() },
        }
    }

    #[test]
    fn structural_array_inference_checks_every_element() {
        use crate::ast::Expression as E;
        use crate::hir::HirType as T;
        use crate::span::Span;
        let span = Span::default();
        let project = empty_project();
        let environment = HashMap::new();
        let homogeneous = E::Array { elements: vec![E::Number { value: 1, span }, E::Number { value: 2, span }], span };
        assert_eq!(infer_hir_structural_type(&project, "main", &homogeneous, &environment).unwrap(), T::Array(Box::new(T::Num)));

        let heterogeneous = E::Array { elements: vec![E::Number { value: 1, span }, E::String { value: "x".into(), span }], span };
        assert!(infer_hir_structural_type(&project, "main", &heterogeneous, &environment).is_err());
    }

    #[test]
    fn structural_index_inference_preserves_array_element_type() {
        use crate::ast::Expression as E;
        use crate::hir::HirType as T;
        use crate::span::Span;
        let span = Span::default();
        let project = empty_project();
        let environment = HashMap::new();
        let expression = E::Index {
            array: Box::new(E::Array { elements: vec![E::Boolean { value: true, span }], span }),
            index: Box::new(E::Number { value: 0, span }),
            span,
        };
        assert_eq!(infer_hir_structural_type(&project, "main", &expression, &environment).unwrap(), T::Bool);
    }

    #[test]
    fn generic_function_call_return_type_uses_inferred_argument_binding() {
        use crate::ast::{Expression as E, Parameter, Program, Statement as S};
        use crate::hir::HirType as T;
        use crate::span::Span;
        let span = Span::default();
        let function = S::Function {
            name: "identity".into(),
            generic_parameters: vec!["Item".into()],
            parameters: vec![Parameter { name: "value".into(), name_span: span, type_name: Some("Item".into()), span }],
            return_type: Some("Item".into()), body: Vec::new(), is_async: false, span,
        };
        let module = ModuleUnit {
            name: "main".into(), path: PathBuf::new(), imports: Vec::new(),
            public_symbols: HashSet::new(), local_public_symbols: HashSet::new(),
            function_symbols: HashSet::from(["identity".to_string()]),
            private_symbols: HashSet::new(), statements: vec![function], statement_start: 0, statement_end: 1,
        };
        let project = Project {
            entry_module: "main".into(), modules: vec![module],
            program: Program { statements: Vec::new(), span, modules: Vec::new() },
        };
        let call = E::Call {
            name: "identity".into(),
            arguments: vec![E::Number { value: 42, span }],
            generic_arguments: Vec::new(), span,
        };
        assert_eq!(infer_hir_structural_type(&project, "main", &call, &HashMap::new()).unwrap(), T::Num);
    }
}

#[cfg(test)]
mod hir_structural_local_environment_tests {
    use super::*;
    use crate::hir::HirType as T;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("fusion-hir-local-types-{}-{}", std::process::id(), nonce));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
        fn write(&self, source: &str) { fs::write(self.0.join("main.fusion"), source).unwrap(); }
        fn load(&self) -> Project { load_project_with_metadata(self.0.join("main.fusion")).unwrap() }
    }
    impl Drop for TempProject { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn retains_primitive_array_and_index_types_through_local_bindings() {
        let temp = TempProject::new();
        temp.write("main:\n    let seed = 7\n    let values = [seed, 9]\n    let first = values[0]\n    print(first)\n");
        let project = temp.load();
        let locals = resolve_hir_local_types(&project).unwrap();
        let ty = |name: &str| locals.iter().find(|item| item.name == name).map(|item| item.ty.clone()).unwrap();
        assert_eq!(ty("seed"), T::Num);
        assert_eq!(ty("values"), T::Array(Box::new(T::Num)));
        assert_eq!(ty("first"), T::Num);
    }

    #[test]
    fn retains_generic_parameter_and_return_types_in_local_environment() {
        let temp = TempProject::new();
        temp.write("fn identity<T>(value: T) -> T:\n    return value\nmain:\n    let answer = identity(42)\n    print(answer)\n");
        let project = temp.load();
        let locals = resolve_hir_local_types(&project).unwrap();
        assert!(locals.iter().any(|item| item.name == "value" && item.ty == T::Generic("T".into())));
        assert!(locals.iter().any(|item| item.name == "answer" && item.ty == T::Num));
    }

    #[test]
    fn rejects_assignment_that_changes_a_known_structural_local_type() {
        let temp = TempProject::new();
        temp.write("main:\n    let count = 1\n    count = \"one\"\n");
        let error = resolve_hir_local_types(&temp.load()).unwrap_err();
        assert!(error.contains("changes structural type"), "{error}");
    }
}
