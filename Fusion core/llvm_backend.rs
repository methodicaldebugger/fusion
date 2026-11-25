use crate::ast::{
    Expression, MatchArm, Operator, Pattern, PatternKind, Program, Statement, UnaryOperator,
};
use crate::errors::FusionError;
use crate::types::{StructDefinition, Type};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LlType {
    I64,
    I1,
    F64,
    Ptr,
    Void,
}

impl LlType {
    fn text(self) -> &'static str {
        match self {
            LlType::I64 => "i64",
            LlType::I1 => "i1",
            LlType::F64 => "double",
            LlType::Ptr => "ptr",
            LlType::Void => "void",
        }
    }
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<LlType>,
    ret: LlType,
}

#[derive(Debug, Clone)]
struct ForeignCallSig {
    link_name: String,
    parameters: Vec<crate::foreign::ForeignAbiType>,
    return_type: crate::foreign::ForeignAbiType,
}

#[derive(Debug, Clone)]
struct LoopTargets {
    break_label: String,
    continue_label: String,
}

pub struct LLVMBackend {
    module: String,
    globals: Vec<String>,
    string_ids: HashMap<String, usize>,
    functions: HashMap<String, FunctionSig>,
    /// Foreign C declarations retain their exact ABI widths separately from
    /// Fusion's native scalar representation (i64/double/pointer).
    foreign_functions: HashMap<String, ForeignCallSig>,
    /// Compatibility symbol to authoritative HIR function declaration.
    function_definition_ids: HashMap<String, crate::hir::HirDefinitionId>,
    /// HIR declaration to the backend symbol assigned during lowering.
    function_symbols_by_id: HashMap<crate::hir::HirDefinitionId, String>,
    /// Resolved function-call source spans mapped to identity-selected LLVM symbols.
    function_call_targets: HashMap<(String, usize, usize), String>,
    /// Resolved method-call source spans mapped to identity-selected LLVM symbols.
    method_call_targets: HashMap<(String, usize, usize), String>,
    method_call_return_types: HashMap<(String, usize, usize), crate::hir::HirDefinitionId>,
    /// Nominal return identities for resolved free-function calls.
    function_call_return_types: HashMap<(String, usize, usize), crate::hir::HirDefinitionId>,
    /// Module context follows flattened program statement ranges, preventing
    /// identical source offsets in different modules from colliding.
    current_module: Option<String>,
    /// Backend-oriented HIR bodies available for incremental direct emission.
    hir_bodies: Vec<crate::hir::HirBody>,
    /// Compatibility layouts for standalone programs without project metadata.
    structs: HashMap<String, StructDefinition>,
    /// Project struct layouts keyed by stable declaration identity.
    struct_layouts_by_id: HashMap<crate::hir::HirDefinitionId, StructDefinition>,
    /// Enum payload layouts keyed by stable variant declaration identity.
    enum_payloads_by_id: HashMap<crate::hir::HirDefinitionId, Vec<Type>>,
    /// Concrete LLVM named-type declarations assigned from stable HIR identities.
    llvm_type_names_by_id: HashMap<crate::hir::HirDefinitionId, String>,
    /// LLVM payload-record names keyed by stable enum-variant identity.
    enum_variant_layout_names_by_id: HashMap<crate::hir::HirDefinitionId, String>,
    /// Source field declaration identity to zero-based LLVM struct field index.
    struct_field_indices_by_id: HashMap<crate::hir::HirDefinitionId, usize>,
    /// Source enum-variant identity to its stable declaration-order discriminant.
    enum_variant_discriminants_by_id: HashMap<crate::hir::HirDefinitionId, u32>,
    /// Stable source declaration identities retained beside lowered LLVM symbols.
    type_definition_ids: HashMap<String, crate::hir::HirDefinitionId>,
    /// HIR identities for impl methods, keyed by lowered receiver symbol/name.
    method_definition_ids: HashMap<(String, String), crate::hir::HirDefinitionId>,
    trait_definition_ids: HashMap<String, crate::hir::HirDefinitionId>,
    constant_definition_ids: HashMap<String, crate::hir::HirDefinitionId>,
    /// Module constants lowered to package-owned LLVM globals.
    global_constants: HashMap<String, (LlType, String)>,
    unsupported_global_constants: HashSet<String>,
    struct_field_definition_ids: HashMap<(String, String), crate::hir::HirDefinitionId>,
    enum_variant_definition_ids: HashMap<(String, String), crate::hir::HirDefinitionId>,
    tuple_types: HashMap<String, Vec<Type>>,
    array_types: HashMap<String, Type>,
    next_temp: u32,
    next_block: u32,
    locals: HashMap<String, (LlType, String)>,
    /// Nominal HIR identities carried by local pointer values.
    nominal_types_by_local: HashMap<String, crate::hir::HirDefinitionId>,
    /// Innermost loop first; break and continue always target the nearest loop.
    loop_targets: Vec<LoopTargets>,
    current_ret: LlType,
    current_is_main: bool,
}

impl LLVMBackend {
    pub fn new() -> Self {
        Self {
            module: String::new(),
            globals: Vec::new(),
            string_ids: HashMap::new(),
            functions: HashMap::new(),
            foreign_functions: HashMap::new(),
            function_definition_ids: HashMap::new(),
            function_symbols_by_id: HashMap::new(),
            function_call_targets: HashMap::new(),
            method_call_targets: HashMap::new(),
            method_call_return_types: HashMap::new(),
            function_call_return_types: HashMap::new(),
            current_module: None,
            hir_bodies: Vec::new(),
            structs: HashMap::new(),
            struct_layouts_by_id: HashMap::new(),
            enum_payloads_by_id: HashMap::new(),
            llvm_type_names_by_id: HashMap::new(),
            enum_variant_layout_names_by_id: HashMap::new(),
            struct_field_indices_by_id: HashMap::new(),
            enum_variant_discriminants_by_id: HashMap::new(),
            type_definition_ids: HashMap::new(),
            method_definition_ids: HashMap::new(),
            trait_definition_ids: HashMap::new(),
            constant_definition_ids: HashMap::new(),
            global_constants: HashMap::new(),
            unsupported_global_constants: HashSet::new(),
            struct_field_definition_ids: HashMap::new(),
            enum_variant_definition_ids: HashMap::new(),
            tuple_types: HashMap::new(),
            array_types: HashMap::new(),
            next_temp: 0,
            next_block: 0,
            locals: HashMap::new(),
            nominal_types_by_local: HashMap::new(),
            loop_targets: Vec::new(),
            current_ret: LlType::Void,
            current_is_main: false,
        }
    }

    /// Register the backend symbol for a top-level HIR function. Calls use this
    /// identity map to select their LLVM target; source spellings are not used
    /// as declaration identity once this registration is present.
    pub fn register_hir_function_symbol(
        &mut self,
        identity: crate::hir::HirDefinitionId,
        backend_symbol: impl Into<String>,
    ) {
        let backend_symbol = backend_symbol.into();
        self.function_definition_ids
            .insert(backend_symbol.clone(), identity.clone());
        self.function_symbols_by_id.insert(identity, backend_symbol);
    }

    /// Register top-level functions from a lowered project. Impl-method symbols
    /// are registered separately once their backend ABI names are assigned.
    pub fn register_hir_project_functions(&mut self, project: &crate::hir::HirProject) {
        for module in &project.modules {
            for statement in &module.statements {
                match statement {
                    crate::hir::HirStatement::Function(function) => {
                        if let Some(identity) = &function.identity {
                            let symbol =
                                crate::module_system::mangle_function(&module.name, &function.name);
                            self.register_hir_function_symbol(identity.clone(), symbol);
                        }
                    }
                    crate::hir::HirStatement::Impl { methods, .. } => {
                        for method in methods {
                            if let Some(identity) = &method.identity {
                                let symbol = hir_method_backend_symbol(identity);
                                self.register_hir_function_symbol(identity.clone(), symbol);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Bind a compatibility call-site spelling to a resolved HIR declaration
    /// and its final backend symbol. This is the bridge used while the AST
    /// expression walker is being replaced by direct HIR call emission.
    pub fn register_hir_call_target(
        &mut self,
        identity: crate::hir::HirDefinitionId,
        compatibility_symbol: impl Into<String>,
        backend_symbol: impl Into<String>,
    ) {
        let compatibility_symbol = compatibility_symbol.into();
        let backend_symbol = backend_symbol.into();
        self.function_definition_ids
            .insert(compatibility_symbol, identity.clone());
        self.function_symbols_by_id.insert(identity, backend_symbol);
    }

    pub fn function_symbol_for_id(&self, identity: &crate::hir::HirDefinitionId) -> Option<&str> {
        self.function_symbols_by_id
            .get(identity)
            .map(String::as_str)
    }

    fn resolved_backend_function_symbol(&self, compatibility_symbol: &str) -> String {
        self.function_definition_ids
            .get(compatibility_symbol)
            .and_then(|identity| self.function_symbols_by_id.get(identity))
            .cloned()
            .unwrap_or_else(|| compatibility_symbol.to_string())
    }

    /// Returns the stable HIR identity for a lowered nominal type symbol.
    pub fn type_definition_id(&self, backend_symbol: &str) -> Option<&crate::hir::HirDefinitionId> {
        self.type_definition_ids.get(backend_symbol)
    }

    /// Returns a project's struct layout by declaration identity. Project code
    /// should use this API instead of looking up a nominal layout by source name.
    pub fn struct_layout_by_id(
        &self,
        identity: &crate::hir::HirDefinitionId,
    ) -> Option<&StructDefinition> {
        self.struct_layouts_by_id.get(identity)
    }

    /// Returns an enum variant's payload types by stable variant identity.
    pub fn enum_payload_layout_by_id(
        &self,
        identity: &crate::hir::HirDefinitionId,
    ) -> Option<&[Type]> {
        self.enum_payloads_by_id.get(identity).map(Vec::as_slice)
    }

    /// Returns the emitted LLVM named type associated with a nominal HIR type.
    pub fn llvm_type_name_by_id(&self, identity: &crate::hir::HirDefinitionId) -> Option<&str> {
        self.llvm_type_names_by_id.get(identity).map(String::as_str)
    }

    /// Returns the emitted LLVM payload record type for an enum variant.
    pub fn enum_variant_layout_name_by_id(
        &self,
        identity: &crate::hir::HirDefinitionId,
    ) -> Option<&str> {
        self.enum_variant_layout_names_by_id
            .get(identity)
            .map(String::as_str)
    }

    /// Returns the zero-based field slot for a stable struct-field identity.
    pub fn struct_field_index_by_id(
        &self,
        identity: &crate::hir::HirDefinitionId,
    ) -> Option<usize> {
        self.struct_field_indices_by_id.get(identity).copied()
    }

    /// Returns the declaration-order tag value for a stable enum-variant identity.
    pub fn enum_variant_discriminant_by_id(
        &self,
        identity: &crate::hir::HirDefinitionId,
    ) -> Option<u32> {
        self.enum_variant_discriminants_by_id.get(identity).copied()
    }

    /// Resolve a compatibility-AST type symbol through its HIR identity to the
    /// concrete LLVM named type. This is the intended bridge for instruction lowering.
    pub fn llvm_type_name_for_symbol(&self, backend_symbol: &str) -> Option<&str> {
        let identity = self.type_definition_ids.get(backend_symbol)?;
        self.llvm_type_name_by_id(identity)
    }

    fn nominal_identity_for_type_name(&self, name: &str) -> Option<crate::hir::HirDefinitionId> {
        self.type_definition_ids.get(name).cloned()
    }

    fn nominal_identity_for_expression(
        &self,
        expression: &Expression,
    ) -> Option<crate::hir::HirDefinitionId> {
        match expression {
            Expression::StructConstructor { name, .. } => self.nominal_identity_for_type_name(name),
            Expression::EnumConstructor { enum_name, .. } => {
                self.nominal_identity_for_type_name(enum_name)
            }
            Expression::Identifier { name, .. } => self.nominal_types_by_local.get(name).cloned(),
            Expression::Call { span, .. } => self
                .function_call_return_types
                .get(&(
                    self.current_module.clone().unwrap_or_default(),
                    span.start,
                    span.end,
                ))
                .cloned(),
            Expression::MethodCall { span, .. } => self
                .method_call_return_types
                .get(&(
                    self.current_module.clone().unwrap_or_default(),
                    span.start,
                    span.end,
                ))
                .cloned(),
            Expression::Property { object, name, .. } => {
                let owner = self.nominal_identity_for_expression(object)?;
                let (_, field_type) = self.struct_field_by_owner_and_name(&owner, name)?;
                match field_type {
                    Type::Struct(type_name) | Type::Enum(type_name) => {
                        self.nominal_identity_for_type_name(&type_name)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn struct_field_by_owner_and_name(
        &self,
        owner: &crate::hir::HirDefinitionId,
        field_name: &str,
    ) -> Option<(crate::hir::HirDefinitionId, Type)> {
        for ((owner_symbol, candidate), field_id) in &self.struct_field_definition_ids {
            if candidate != field_name || self.type_definition_ids.get(owner_symbol) != Some(owner)
            {
                continue;
            }
            let index = self.struct_field_index_by_id(field_id)?;
            let field_type = self
                .struct_layouts_by_id
                .get(owner)?
                .fields
                .get(index)?
                .1
                .clone();
            return Some((field_id.clone(), field_type));
        }
        None
    }

    fn emit_struct_field_address(
        &mut self,
        object: &Expression,
        field_name: &str,
        out: &mut String,
    ) -> Result<(String, LlType, Type), FusionError> {
        let owner = self.nominal_identity_for_expression(object).ok_or_else(|| {
            self.unsupported(format!("cannot resolve HIR identity for field access '{}'; receiver must have a known nominal type", field_name))
        })?;
        let (field_id, field_type) = self
            .struct_field_by_owner_and_name(&owner, field_name)
            .ok_or_else(|| {
                self.unsupported(format!(
                    "unknown field '{}' on {}::{}",
                    field_name, owner.module, owner.name
                ))
            })?;
        let llvm_owner = self
            .llvm_type_name_by_id(&owner)
            .ok_or_else(|| {
                self.unsupported(format!(
                    "LLVM layout missing for {}::{}",
                    owner.module, owner.name
                ))
            })?
            .to_string();
        let index = self
            .struct_field_index_by_id(&field_id)
            .ok_or_else(|| self.unsupported("HIR field identity has no LLVM field index"))?;
        let object_value = self.emit_expr(object, out)?;
        let address = self.temp();
        writeln!(
            out,
            "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 {}",
            address, llvm_owner, object_value, index
        )
        .unwrap();
        let native_type = self.native_type_for_fusion_type(&field_type);
        Ok((address, native_type, field_type))
    }

    fn emit_heap_allocation_for_named_type(
        &mut self,
        llvm_type_name: &str,
        out: &mut String,
    ) -> String {
        let end = self.temp();
        writeln!(
            out,
            "  {} = getelementptr {}, ptr null, i32 1",
            end, llvm_type_name
        )
        .unwrap();
        let size = self.temp();
        writeln!(out, "  {} = ptrtoint ptr {} to i64", size, end).unwrap();
        let pointer = self.temp();
        writeln!(
            out,
            "  {} = call ptr @fusion_heap_alloc(i64 {})",
            pointer, size
        )
        .unwrap();
        pointer
    }

    /// Resolve a lowered owner/field pair through its field HIR identity.
    pub fn struct_field_index_for_symbol(
        &self,
        owner_symbol: &str,
        field_name: &str,
    ) -> Option<usize> {
        let identity = self
            .struct_field_definition_ids
            .get(&(owner_symbol.to_string(), field_name.to_string()))?;
        self.struct_field_index_by_id(identity)
    }

    /// Resolve a lowered enum/variant pair through its variant HIR identity.
    pub fn enum_variant_discriminant_for_symbol(
        &self,
        enum_symbol: &str,
        variant_name: &str,
    ) -> Option<u32> {
        let identity = self
            .enum_variant_definition_ids
            .get(&(enum_symbol.to_string(), variant_name.to_string()))?;
        self.enum_variant_discriminant_by_id(identity)
    }

    /// Project-aware entry point. It builds the HIR call-resolution table,
    /// registers declaration identities, then emits the flattened compatibility
    /// program with identity-directed function targets.
    pub fn emit_project(
        mut self,
        project: &crate::module_system::Project,
    ) -> Result<String, String> {
        let hir = crate::hir_lower::HirLowerer::lower_project(project)
            .map_err(|error| format!("HIR call resolution: {}", error))?;
        self.register_hir_project_functions(&hir);
        self.hir_bodies = hir.bodies.clone();
        for call in &hir.resolved_calls {
            let key = (call.module.clone(), call.span.start, call.span.end);
            let symbol = self
                .function_symbol_for_id(&call.declaration)
                .map(str::to_string);
            // The nominal return identity is useful independently of whether
            // this backend has a callable symbol for the declaration (for
            // example, while emitting a package-scoped translation unit).
            // Prefer the legacy nominal field, but retain the structural HIR
            // result as the authoritative fallback for nominal return types.
            let nominal_return = call.return_type.clone().or_else(|| {
                match &call.return_hir_type {
                    crate::hir::HirType::Nominal(identity) => Some(identity.clone()),
                    _ => None,
                }
            });
            match call.kind {
                crate::hir::HirCallKind::Function => {
                    if let Some(symbol) = symbol {
                        self.function_call_targets.insert(key.clone(), symbol);
                    }
                    if let Some(return_type) = nominal_return {
                        self.function_call_return_types.insert(key, return_type);
                    }
                }
                crate::hir::HirCallKind::Method => {
                    if let Some(symbol) = symbol {
                        self.method_call_targets.insert(key.clone(), symbol);
                    }
                    if let Some(return_type) = nominal_return {
                        self.method_call_return_types.insert(key, return_type);
                    }
                }
            }
        }
        self.emit_program(&project.program)
            .map_err(|error| error.to_string())
    }

    pub fn emit_program(self, program: &Program) -> Result<String, FusionError> {
        self.emit_program_inner(program, true)
    }

    /// Emit a library translation unit without requiring an application `main`.
    /// Package-scoped emission uses this before selecting definitions by module identity.
    pub fn emit_library_program(self, program: &Program) -> Result<String, FusionError> {
        self.emit_program_inner(program, false)
    }

    fn emit_program_inner(
        mut self,
        program: &Program,
        require_main: bool,
    ) -> Result<String, FusionError> {
        self.collect_declarations(program)?;
        self.emit_preamble();
        self.emit_foreign_declarations()?;
        for (statement_index, stmt) in program.statements.iter().enumerate() {
            self.current_module = program
                .modules
                .iter()
                .find(|module| {
                    statement_index >= module.statement_start
                        && statement_index < module.statement_end
                })
                .map(|module| module.name.clone());
            if let Statement::Function {
                name,
                foreign_abi,
                ..
            } = stmt
            {
                if foreign_abi.is_none() {
                    self.emit_function(stmt, name, None)?;
                }
            } else if let Statement::Impl {
                type_name, methods, ..
            } = stmt
            {
                for method in methods {
                    if let Statement::Function { name, .. } = method {
                        let identity = self
                            .method_definition_ids
                            .get(&(type_name.clone(), name.clone()))
                            .cloned();
                        if let Some(identity) = identity {
                            let symbol = self
                                .function_symbol_for_id(&identity)
                                .map(str::to_string)
                                .unwrap_or_else(|| hir_method_backend_symbol(&identity));
                            self.emit_function(method, &symbol, Some(&identity))?;
                        }
                    }
                }
            } else if let Statement::Main { body, .. } = stmt {
                self.emit_main(body)?;
            }
        }
        if require_main && !self.functions.contains_key("main") {
            // Application emission requires an entry point; library emission does not.
            return Err(self.unsupported("program has no main"));
        }
        let mut out = String::new();
        out.push_str(&self.module);
        for g in &self.globals {
            out.push_str(g);
            out.push('\n');
        }
        out.push('\n');
        out.push_str("declare i32 @printf(ptr, ...)\n");
        out.push_str("declare i32 @puts(ptr)\n");
        out.push_str("declare i32 @strcmp(ptr, ptr)\n");
        out.push_str("declare ptr @malloc(i64)\n");
        out.push_str("declare void @free(ptr)\n");
        out.push_str("declare void @llvm.trap()\n");
        out.push_str(
            r#"
@fusion_tuple_head = global ptr null
@fusion_heap_head = global ptr null

define ptr @fusion_heap_alloc(i64 %bytes) {
entry:
  %nonzero = icmp ugt i64 %bytes, 0
  %payload_bytes = select i1 %nonzero, i64 %bytes, i64 1
  %total = add i64 %payload_bytes, 16
  %raw = call ptr @malloc(i64 %total)
  %next_slot = getelementptr inbounds i64, ptr %raw, i64 1
  %old_head = load ptr, ptr @fusion_heap_head
  %old_int = ptrtoint ptr %old_head to i64
  store i64 %old_int, ptr %next_slot, align 8
  store ptr %raw, ptr @fusion_heap_head
  %data = getelementptr inbounds i8, ptr %raw, i64 16
  ret ptr %data
}

define void @fusion_heap_cleanup() {
entry:
  br label %loop
loop:
  %raw = load ptr, ptr @fusion_heap_head
  %done = icmp eq ptr %raw, null
  br i1 %done, label %exit, label %body
body:
  %next_slot = getelementptr inbounds i64, ptr %raw, i64 1
  %next_int = load i64, ptr %next_slot, align 8
  %next = inttoptr i64 %next_int to ptr
  store ptr %next, ptr @fusion_heap_head
  call void @free(ptr %raw)
  br label %loop
exit:
  ret void
}

define ptr @fusion_tuple_new(i64 %count) {
entry:
  %slots = add i64 %count, 2
  %bytes = mul i64 %slots, 8
  %raw = call ptr @malloc(i64 %bytes)
  %next_slot = getelementptr inbounds i64, ptr %raw, i64 1
  %old_head = load ptr, ptr @fusion_tuple_head
  %old_int = ptrtoint ptr %old_head to i64
  store i64 %old_int, ptr %next_slot, align 8
  store ptr %raw, ptr @fusion_tuple_head
  %count_slot = getelementptr inbounds i64, ptr %raw, i64 0
  store i64 %count, ptr %count_slot, align 8
  %data = getelementptr inbounds i64, ptr %raw, i64 2
  ret ptr %data
}

define void @fusion_tuple_cleanup() {
entry:
  br label %loop
loop:
  %raw = load ptr, ptr @fusion_tuple_head
  %done = icmp eq ptr %raw, null
  br i1 %done, label %exit, label %body
body:
  %next_slot = getelementptr inbounds i64, ptr %raw, i64 1
  %next_int = load i64, ptr %next_slot, align 8
  %next = inttoptr i64 %next_int to ptr
  store ptr %next, ptr @fusion_tuple_head
  call void @free(ptr %raw)
  br label %loop
exit:
  ret void
}

define i64 @fusion_tuple_len(ptr %data) {
entry:
  %header = getelementptr inbounds i64, ptr %data, i64 -2
  %length = load i64, ptr %header, align 8
  ret i64 %length
}
"#,
        );
        out.push_str("\n");
        Ok(out)
    }

    fn output_ends_in_terminator(out: &str) -> bool {
        out.lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(|line| {
                let line = line.trim_start();
                line == "unreachable"
                    || line.starts_with("ret ")
                    || line.starts_with("br ")
                    || line.starts_with("switch ")
                    || line.starts_with("resume ")
                    || line.starts_with("unreachable")
            })
            .unwrap_or(false)
    }

    fn unsupported(&self, msg: impl Into<String>) -> FusionError {
        FusionError::Syntax {
            message: format!("LLVM backend: {}", msg.into()),
            span: crate::span::Span::new(0, 0),
        }
    }

    fn collect_declarations(&mut self, program: &Program) -> Result<(), FusionError> {
        self.type_definition_ids = crate::module_system::backend_type_definition_ids(program);
        self.method_definition_ids = crate::module_system::backend_method_definition_ids(program);
        self.trait_definition_ids = crate::module_system::backend_trait_definition_ids(program);
        self.constant_definition_ids =
            crate::module_system::backend_constant_definition_ids(program);
        self.global_constants.clear();
        self.unsupported_global_constants.clear();
        for statement in &program.statements {
            let Statement::ConstDeclaration {
                name,
                declared_type,
                value,
                ..
            } = statement
            else {
                continue;
            };
            let Some(identity) = self.constant_definition_ids.get(name).cloned() else {
                continue;
            };
            let (ty, initializer) = match value {
                Expression::Number { value, .. } => (LlType::I64, Some(value.to_string())),
                Expression::Float { value, .. } => (LlType::F64, Some(format!("{:.17e}", value))),
                Expression::Boolean { value, .. } => (
                    LlType::I1,
                    Some((if *value { "true" } else { "false" }).to_string()),
                ),
                Expression::String { value, .. } => {
                    let pointer = self.string_ptr(value);
                    (LlType::Ptr, Some(pointer))
                }
                _ => (
                    declared_type
                        .as_deref()
                        .map(|name| self.llvm_type_name(name))
                        .unwrap_or(LlType::I64),
                    None,
                ),
            };
            if let Some(initializer) = initializer {
                let symbol = format!(
                    "_F{}",
                    crate::module_system::mangle_function(
                        &identity.module,
                        &format!("$data${}", identity.name)
                    )
                );
                self.globals.push(format!(
                    "@{} = constant {} {}, align 8",
                    symbol,
                    ty.text(),
                    initializer
                ));
                self.global_constants.insert(name.clone(), (ty, symbol));
            } else {
                self.unsupported_global_constants.insert(name.clone());
            }
        }
        let (field_ids, variant_ids) = crate::module_system::backend_member_definition_ids(program);
        self.struct_field_definition_ids = field_ids;
        self.enum_variant_definition_ids = variant_ids;
        for s in &program.statements {
            match s {
                Statement::Function {
                    name,
                    parameters,
                    return_type,
                    body,
                    foreign_abi,
                    ..
                } => {
                    if foreign_abi.is_some() {
                        let abi_parameters = parameters.iter().map(|parameter| {
                            parameter.type_name.as_deref()
                                .and_then(crate::foreign::ForeignAbiType::from_type_name)
                                .ok_or_else(|| self.unsupported(format!(
                                    "foreign parameter '{}' has no supported ABI type", parameter.name
                                )))
                        }).collect::<Result<Vec<_>, _>>()?;
                        let abi_return = return_type.as_deref().unwrap_or("void");
                        let abi_return = crate::foreign::ForeignAbiType::from_type_name(abi_return)
                            .ok_or_else(|| self.unsupported(format!(
                                "foreign function '{}' has unsupported return type '{}'", name, abi_return
                            )))?;
                        let link_name = match s {
                            Statement::Function { link_name, .. } => link_name.clone().unwrap_or_else(|| name.clone()),
                            _ => name.clone(),
                        };
                        let signature = ForeignCallSig {
                            link_name,
                            parameters: abi_parameters,
                            return_type: abi_return,
                        };
                        let native_params = signature.parameters.iter()
                            .map(Self::foreign_native_llvm_type).collect();
                        let native_ret = Self::foreign_native_llvm_type(&signature.return_type);
                        self.functions.insert(name.clone(), FunctionSig { params: native_params, ret: native_ret });
                        self.foreign_functions.insert(name.clone(), signature);
                        continue;
                    }
                    if self.functions.contains_key(name) {
                        continue;
                    }
                    let params = parameters
                        .iter()
                        .map(|p| {
                            p.type_name
                                .as_deref()
                                .map(|p| self.llvm_type_name(p))
                                .unwrap_or(LlType::I64)
                        })
                        .collect();
                    let ret = match return_type.as_deref() {
                        Some("void") => LlType::Void,
                        Some(t) => self.llvm_type_name(t),
                        None => self.infer_return_type(body),
                    };
                    self.functions
                        .insert(name.clone(), FunctionSig { params, ret });
                }
                Statement::Impl {
                    type_name, methods, ..
                } => {
                    for method in methods {
                        if let Statement::Function {
                            name,
                            parameters,
                            return_type,
                            body,
                            ..
                        } = method
                        {
                            let identity = self
                                .method_definition_ids
                                .get(&(type_name.clone(), name.clone()))
                                .cloned();
                            let Some(identity) = identity else {
                                continue;
                            };
                            let symbol = self
                                .function_symbol_for_id(&identity)
                                .map(str::to_string)
                                .unwrap_or_else(|| hir_method_backend_symbol(&identity));
                            let has_explicit_self =
                                parameters.first().map(|parameter| parameter.name.as_str())
                                    == Some("self");
                            let mut params = Vec::with_capacity(
                                parameters.len() + usize::from(!has_explicit_self),
                            );
                            // The receiver is always the first ABI argument, whether `self`
                            // is written explicitly in source or supplied implicitly.
                            params.push(LlType::Ptr);
                            let user_parameters = if has_explicit_self {
                                &parameters[1..]
                            } else {
                                parameters.as_slice()
                            };
                            params.extend(user_parameters.iter().map(|parameter| {
                                parameter
                                    .type_name
                                    .as_deref()
                                    .map(|ty| self.llvm_type_name(ty))
                                    .unwrap_or(LlType::I64)
                            }));
                            let ret = match return_type.as_deref() {
                                Some("void") => LlType::Void,
                                Some(ty) => self.llvm_type_name(ty),
                                None => self.infer_return_type(body),
                            };
                            self.functions.insert(symbol, FunctionSig { params, ret });
                        }
                    }
                }
                Statement::Struct { name, fields, .. } => {
                    let layout = StructDefinition {
                        fields: fields
                            .iter()
                            .map(|f| (f.name.clone(), self.type_from_name(&f.type_name)))
                            .collect(),
                    };
                    if let Some(identity) = self.type_definition_ids.get(name).cloned() {
                        self.struct_layouts_by_id.insert(identity, layout);
                    } else {
                        self.structs.insert(name.clone(), layout);
                    }
                }
                Statement::Enum { name, variants, .. } => {
                    for variant in variants {
                        let identity = self
                            .enum_variant_definition_ids
                            .get(&(name.clone(), variant.name.clone()))
                            .cloned();
                        if let Some(identity) = identity {
                            let payload = variant
                                .fields
                                .iter()
                                .map(|field| self.type_from_name(field))
                                .collect();
                            self.enum_payloads_by_id.insert(identity, payload);
                        }
                    }
                }
                _ => {}
            }
        }
        // Assign stable LLVM type names directly from HIR declaration identities.
        // These are real named LLVM layout declarations, not traversal-order
        // counters, so package subsets keep identical names for the same type.
        let mut nominal_ids: Vec<_> = self.type_definition_ids.values().cloned().collect();
        nominal_ids.sort();
        nominal_ids.dedup();
        for identity in nominal_ids {
            let llvm_name = match identity.kind {
                crate::hir::HirDefinitionKind::Struct | crate::hir::HirDefinitionKind::Enum => {
                    // Stable across package subsets and unrelated graph changes:
                    // the same nominal declaration always receives the same LLVM
                    // identified-type name, regardless of discovery order.
                    format!(
                        "%fusion.type.{}",
                        crate::module_system::mangle_type(&identity.module, &identity.name)
                    )
                }
                _ => continue,
            };
            self.llvm_type_names_by_id.insert(identity, llvm_name);
        }

        // Map each field declaration to its actual position in the source
        // struct declaration. The identity is scoped beneath the owning type.
        for statement in &program.statements {
            if let Statement::Struct { name, fields, .. } = statement {
                for (index, field) in fields.iter().enumerate() {
                    if let Some(identity) = self
                        .struct_field_definition_ids
                        .get(&(name.clone(), field.name.clone()))
                    {
                        self.struct_field_indices_by_id
                            .insert(identity.clone(), index);
                    }
                }
            }
        }

        for statement in &program.statements {
            if let Statement::Enum { name, variants, .. } = statement {
                for (discriminant, variant) in variants.iter().enumerate() {
                    if let Some(identity) = self
                        .enum_variant_definition_ids
                        .get(&(name.clone(), variant.name.clone()))
                    {
                        self.enum_variant_discriminants_by_id
                            .insert(identity.clone(), discriminant as u32);
                    }
                }
            }
        }

        let mut variant_ids: Vec<_> = self.enum_variant_definition_ids.values().cloned().collect();
        variant_ids.sort();
        variant_ids.dedup();
        for identity in variant_ids {
            let owner = identity.scope.join("::");
            let variant_key = format!("{}::{}", owner, identity.name);
            let stable = crate::module_system::mangle_type(&identity.module, &variant_key);
            self.enum_variant_layout_names_by_id
                .insert(identity, format!("%fusion.variant.{}", stable));
        }

        self.functions.insert(
            "print".into(),
            FunctionSig {
                params: vec![LlType::I64],
                ret: LlType::Void,
            },
        );
        Ok(())
    }

    fn foreign_abi_llvm_type(ty: &crate::foreign::ForeignAbiType) -> &'static str {
        use crate::foreign::ForeignAbiType as A;
        match ty {
            A::Void => "void",
            A::I8 | A::U8 => "i8",
            A::I16 | A::U16 => "i16",
            A::I32 | A::U32 => "i32",
            A::I64 | A::U64 => "i64",
            A::F32 => "float",
            A::F64 => "double",
            A::OpaquePointer => "ptr",
        }
    }

    fn foreign_native_llvm_type(ty: &crate::foreign::ForeignAbiType) -> LlType {
        use crate::foreign::ForeignAbiType as A;
        match ty {
            A::Void => LlType::Void,
            A::F32 | A::F64 => LlType::F64,
            A::OpaquePointer => LlType::Ptr,
            A::I8 | A::U8 | A::I16 | A::U16 | A::I32 | A::U32 | A::I64 | A::U64 => LlType::I64,
        }
    }

    /// Preserve foreign linker symbols verbatim. Fusion's internal symbol
    /// sanitizer deliberately prefixes `_F`, but doing that to an `extern "C"`
    /// symbol would make the generated declaration disagree with the C object.
    fn foreign_llvm_symbol(symbol: &str) -> String {
        let mut chars = symbol.chars();
        let valid_start = chars
            .next()
            .map(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '$'))
            .unwrap_or(false);
        let valid_rest = chars.all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$')
        });
        if valid_start && valid_rest {
            symbol.to_string()
        } else {
            format!("\"{}\"", llvm_escape(symbol))
        }
    }

    fn emit_foreign_declarations(&mut self) -> Result<(), FusionError> {
        let mut declarations: Vec<_> = self.foreign_functions.iter().collect();
        declarations.sort_by(|(left, _), (right, _)| left.cmp(right));
        let mut emitted: HashMap<String, (Vec<String>, String)> = HashMap::new();
        for (source_name, signature) in declarations {
            let parameters: Vec<String> = signature.parameters.iter()
                .map(Self::foreign_abi_llvm_type).map(str::to_string).collect();
            let result = Self::foreign_abi_llvm_type(&signature.return_type).to_string();
            if let Some((existing_parameters, existing_result)) = emitted.get(&signature.link_name) {
                if existing_parameters != &parameters || existing_result != &result {
                    return Err(self.unsupported(format!(
                        "foreign symbol '{}' is declared with conflicting ABI signatures (including '{}')",
                        signature.link_name, source_name
                    )));
                }
                continue;
            }
            emitted.insert(signature.link_name.clone(), (parameters.clone(), result.clone()));
            writeln!(self.module, "declare ccc {} @{}({})", result,
                Self::foreign_llvm_symbol(&signature.link_name), parameters.join(", ")).unwrap();
        }
        if !emitted.is_empty() { self.module.push('\n'); }
        Ok(())
    }

    fn emit_foreign_call(
        &mut self,
        source_name: &str,
        arguments: &[Expression],
        out: &mut String,
    ) -> Result<String, FusionError> {
        let signature = self.foreign_functions.get(source_name).cloned()
            .ok_or_else(|| self.unsupported(format!("missing foreign ABI for '{}'", source_name)))?;
        if arguments.len() != signature.parameters.len() {
            return Err(self.unsupported(format!(
                "foreign function '{}' expects {} arguments, found {}",
                source_name, signature.parameters.len(), arguments.len()
            )));
        }

        let mut lowered_arguments = Vec::with_capacity(arguments.len());
        for (argument, abi_type) in arguments.iter().zip(&signature.parameters) {
            let value = self.emit_expr(argument, out)?;
            use crate::foreign::ForeignAbiType as A;
            let lowered = match abi_type {
                A::I8 | A::U8 | A::I16 | A::U16 | A::I32 | A::U32 => {
                    let width = Self::foreign_abi_llvm_type(abi_type);
                    let temp = self.temp();
                    writeln!(out, "  {} = trunc i64 {} to {}", temp, value, width).unwrap();
                    temp
                }
                A::I64 | A::U64 | A::F64 | A::OpaquePointer => value,
                A::F32 => {
                    let temp = self.temp();
                    writeln!(out, "  {} = fptrunc double {} to float", temp, value).unwrap();
                    temp
                }
                A::Void => return Err(self.unsupported("void is not a valid foreign parameter type")),
            };
            lowered_arguments.push(format!("{} {}", Self::foreign_abi_llvm_type(abi_type), lowered));
        }

        let result_type = Self::foreign_abi_llvm_type(&signature.return_type);
        let call = format!("call ccc {} @{}({})", result_type,
            Self::foreign_llvm_symbol(&signature.link_name), lowered_arguments.join(", "));
        use crate::foreign::ForeignAbiType as A;
        match signature.return_type {
            A::Void => {
                writeln!(out, "  {}", call).unwrap();
                Ok("0".into())
            }
            A::I8 | A::I16 | A::I32 => {
                let raw = self.temp();
                writeln!(out, "  {} = {}", raw, call).unwrap();
                let widened = self.temp();
                writeln!(out, "  {} = sext {} {} to i64", widened, result_type, raw).unwrap();
                Ok(widened)
            }
            A::U8 | A::U16 | A::U32 => {
                let raw = self.temp();
                writeln!(out, "  {} = {}", raw, call).unwrap();
                let widened = self.temp();
                writeln!(out, "  {} = zext {} {} to i64", widened, result_type, raw).unwrap();
                Ok(widened)
            }
            A::I64 | A::U64 | A::F64 | A::OpaquePointer => {
                let raw = self.temp();
                writeln!(out, "  {} = {}", raw, call).unwrap();
                Ok(raw)
            }
            A::F32 => {
                let raw = self.temp();
                writeln!(out, "  {} = {}", raw, call).unwrap();
                let widened = self.temp();
                writeln!(out, "  {} = fpext float {} to double", widened, raw).unwrap();
                Ok(widened)
            }
        }
    }

    fn type_from_name(&self, n: &str) -> Type {
        match n {
            "num" | "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "isize" | "usize" => Type::Num,
            "float" | "f32" | "f64" => Type::Float,
            "bool" => Type::Bool,
            "string" => Type::String,
            x => Type::Struct(x.into()),
        }
    }

    fn llvm_storage_type(&self, ty: &Type) -> String {
        match ty {
            Type::Num => "i64".into(),
            Type::Float => "double".into(),
            Type::Bool => "i1".into(),
            Type::String
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Iterator(_)
            | Type::HashMap(_, _)
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::File
            | Type::TcpStream
            | Type::Task(_)
            | Type::Function(_, _) => "ptr".into(),
            Type::Generic(name) if matches!(name.as_str(), "f32" | "f64") => "double".into(),
            Type::Generic(name) if name == "*opaque" => "ptr".into(),
            Type::Generic(_) | Type::Unknown => "i64".into(),
            Type::Void => "i8".into(),
        }
    }

    fn llvm_type_name(&self, n: &str) -> LlType {
        // The current native runtime represents Fusion integers as i64 and
        // floating-point values as double. Fixed-width source annotations are
        // recognized here; ABI-width lowering remains a separate backend step.
        match n {
            "float" | "f32" | "f64" => LlType::F64,
            "bool" => LlType::I1,
            "string" | "*opaque" => LlType::Ptr,
            "void" => LlType::Void,
            _ if self.type_definition_ids.contains_key(n) => LlType::Ptr,
            _ => LlType::I64,
        }
    }

    fn infer_return_type(&self, body: &[Statement]) -> LlType {
        for s in body {
            if let Statement::Return { value: Some(e), .. } = s {
                return self.expr_type(e);
            }
            if let Statement::If {
                body: b, else_body, ..
            } = s
            {
                let t = self.infer_return_type(b);
                if t != LlType::Void {
                    return t;
                }
                if let Some(e) = else_body {
                    let t = self.infer_return_type(e);
                    if t != LlType::Void {
                        return t;
                    }
                }
            }
        }
        LlType::Void
    }

    fn emit_preamble(&mut self) {
        self.module.push_str("; Fusion LLVM IR\n");
        // Keep source declaration identities visible in generated artifacts.
        // This deterministic manifest is also consumed by downstream tooling
        // that needs to map backend symbols back to module-qualified types.
        let mut identities: Vec<_> = self.type_definition_ids.iter().collect();
        identities.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (symbol, identity) in identities {
            self.module.push_str(&format!(
                "; fusion.type {}::{} [{}] = {}\n",
                identity.module,
                identity.name,
                match identity.kind {
                    crate::hir::HirDefinitionKind::Struct => "struct",
                    crate::hir::HirDefinitionKind::Enum => "enum",
                    _ => "nominal",
                },
                symbol,
            ));
        }
        let mut methods: Vec<_> = self.method_definition_ids.iter().collect();
        methods.sort_by(|(left, _), (right, _)| left.cmp(right));
        for ((receiver, method), identity) in methods {
            self.module.push_str(&format!(
                "; fusion.method {}::{} [{}] = {}::{}\n",
                identity.module,
                identity.name,
                identity.scope.join("/"),
                receiver,
                method,
            ));
        }
        let mut traits: Vec<_> = self.trait_definition_ids.iter().collect();
        traits.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (symbol, identity) in traits {
            self.module.push_str(&format!(
                "; fusion.trait {}::{} = {}\n",
                identity.module, identity.name, symbol
            ));
        }
        let mut constants: Vec<_> = self.constant_definition_ids.iter().collect();
        constants.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (symbol, identity) in constants {
            self.module.push_str(&format!(
                "; fusion.constant {}::{} = {}\n",
                identity.module, identity.name, symbol
            ));
        }
        let mut fields: Vec<_> = self.struct_field_definition_ids.iter().collect();
        fields.sort_by(|(left, _), (right, _)| left.cmp(right));
        for ((owner, field), identity) in fields {
            self.module.push_str(&format!(
                "; fusion.field {}::{}::{} = {}.{}\n",
                identity.module,
                identity.scope.join("::"),
                identity.name,
                owner,
                field
            ));
        }
        let mut variants: Vec<_> = self.enum_variant_definition_ids.iter().collect();
        variants.sort_by(|(left, _), (right, _)| left.cmp(right));
        for ((owner, variant), identity) in variants {
            self.module.push_str(&format!(
                "; fusion.variant {}::{}::{} = {}::{}\n",
                identity.module,
                identity.scope.join("::"),
                identity.name,
                owner,
                variant
            ));
        }

        let mut layouts: Vec<_> = self.llvm_type_names_by_id.iter().collect();
        layouts.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (identity, llvm_name) in layouts {
            match identity.kind {
                crate::hir::HirDefinitionKind::Struct => {
                    if let Some(layout) = self.struct_layouts_by_id.get(identity) {
                        let fields = layout
                            .fields
                            .iter()
                            .map(|(_, ty)| self.llvm_storage_type(ty))
                            .collect::<Vec<_>>()
                            .join(", ");
                        self.module
                            .push_str(&format!("{} = type {{ {} }}\n", llvm_name, fields));
                    }
                }
                crate::hir::HirDefinitionKind::Enum => {
                    // A tagged enum stores its discriminant and a pointer to a
                    // separately typed payload record. This keeps payload
                    // alignment/type information explicit without forcing a
                    // maximum-sized inline union into every enum value.
                    self.module
                        .push_str(&format!("{} = type {{ i32, ptr }}\n", llvm_name));
                }
                _ => {}
            }
        }
        let mut variant_layouts: Vec<_> = self.enum_variant_layout_names_by_id.iter().collect();
        variant_layouts.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (identity, llvm_name) in variant_layouts {
            let fields = self
                .enum_payloads_by_id
                .get(identity)
                .map(|payload| {
                    payload
                        .iter()
                        .map(|ty| self.llvm_storage_type(ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            self.module
                .push_str(&format!("{} = type {{ {} }}\n", llvm_name, fields));
        }
        let mut field_slots: Vec<_> = self.struct_field_indices_by_id.iter().collect();
        field_slots.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (identity, index) in field_slots {
            self.module.push_str(&format!(
                "; fusion.field-index {}::{}::{} = {}\n",
                identity.module,
                identity.scope.join("::"),
                identity.name,
                index
            ));
        }
        let mut discriminants: Vec<_> = self.enum_variant_discriminants_by_id.iter().collect();
        discriminants.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (identity, discriminant) in discriminants {
            self.module.push_str(&format!(
                "; fusion.variant-discriminant {}::{}::{} = {}\n",
                identity.module,
                identity.scope.join("::"),
                identity.name,
                discriminant
            ));
        }
        self.module.push_str("\n");
    }

    fn reset_function_state(&mut self, ret: LlType) {
        self.current_is_main = false;
        self.next_temp = 0;
        self.next_block = 0;
        self.locals.clear();
        self.nominal_types_by_local.clear();
        self.tuple_types.clear();
        self.array_types.clear();
        self.current_ret = ret;
    }

    fn temp(&mut self) -> String {
        let s = format!("%t{}", self.next_temp);
        self.next_temp += 1;
        s
    }
    fn block(&mut self, prefix: &str) -> String {
        let s = format!("{}.{}", prefix, self.next_block);
        self.next_block += 1;
        s
    }
    fn string_ptr(&mut self, value: &str) -> String {
        if let Some(id) = self.string_ids.get(value) {
            return format!(
                "getelementptr inbounds ([{} x i8], ptr @.str{}, i64 0, i64 0)",
                value.as_bytes().len() + 1,
                id
            );
        }
        let id = self.string_ids.len();
        self.string_ids.insert(value.to_string(), id);
        let escaped = llvm_escape(value);
        self.globals.push(format!(
            "@.str{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
            id,
            value.as_bytes().len() + 1,
            escaped
        ));
        format!(
            "getelementptr inbounds ([{} x i8], ptr @.str{}, i64 0, i64 0)",
            value.as_bytes().len() + 1,
            id
        )
    }

    fn emit_main(&mut self, body: &[Statement]) -> Result<(), FusionError> {
        self.reset_function_state(LlType::I64);
        self.current_is_main = true;
        self.functions.insert(
            "main".into(),
            FunctionSig {
                params: vec![],
                ret: LlType::I64,
            },
        );
        let mut b = String::new();
        writeln!(b, "define i32 @main() {{").unwrap();
        writeln!(b, "entry:").unwrap();
        self.emit_statements(body, &mut b)?;
        writeln!(b, "  call void @fusion_tuple_cleanup()").unwrap();
        writeln!(b, "  call void @fusion_heap_cleanup()").unwrap();
        writeln!(b, "  ret i32 0").unwrap();
        writeln!(b, "}}").unwrap();
        self.module.push_str(&b);
        Ok(())
    }

    /// Directly emits the supported scalar/control-flow subset of a backend HIR body.
    /// This deliberately rejects unsupported constructs so the project emitter can
    /// keep using its compatibility-AST path until each feature is migrated.
    pub fn emit_hir_body(
        &mut self,
        body: &crate::hir::HirBody,
        symbol: &str,
        parameter_types: &[crate::hir::HirType],
        return_type: &crate::hir::HirType,
    ) -> Result<String, FusionError> {
        use crate::hir::{HirExpression as E, HirStatement as S, HirTerminator as T};
        let return_type = hir_scalar_llvm_type(return_type)
            .ok_or_else(|| self.unsupported("unsupported HIR function return type"))?;
        let parameter_types: Vec<LlType> = parameter_types
            .iter()
            .map(|ty| {
                hir_scalar_llvm_type(ty)
                    .ok_or_else(|| self.unsupported("unsupported HIR parameter type"))
            })
            .collect::<Result<_, _>>()?;
        self.reset_function_state(return_type);
        let mut slots = HashMap::new();
        let mut loop_steps: HashMap<u32, String> = HashMap::new();
        // The HIR CFG represents foreach binding as a local plus an explicit
        // loop terminator. Materialize the current element on entry to its body.
        let mut foreach_bindings: HashMap<u32, (crate::hir::HirLocalId, String, String, LlType)> =
            HashMap::new();
        let mut out = String::new();
        write!(
            out,
            "define {} @{}(",
            return_type.text(),
            sanitize_symbol(symbol)
        )
        .unwrap();
        for (i, ty) in parameter_types.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write!(out, "{} %arg{}", ty.text(), i).unwrap();
        }
        out.push_str(") {\nentry:\n");
        for local in &body.locals {
            let ty = hir_scalar_llvm_type(&local.ty).ok_or_else(|| {
                self.unsupported(format!("unsupported HIR local type for {}", local.name))
            })?;
            let slot = format!("%hir_local_{}", local.id.0);
            writeln!(out, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
            slots.insert(local.id, (ty, slot));
        }
        let mut parameter_index = 0;
        for local in body.locals.iter().filter(|local| local.parameter) {
            let (ty, slot) = slots.get(&local.id).cloned().unwrap();
            if parameter_types.get(parameter_index) != Some(&ty) {
                return Err(self.unsupported("HIR parameter signature mismatch"));
            }
            writeln!(
                out,
                "  store {} %arg{}, ptr {}",
                ty.text(),
                parameter_index,
                slot
            )
            .unwrap();
            parameter_index += 1;
        }
        if parameter_index != parameter_types.len() {
            return Err(self.unsupported("HIR parameter count mismatch"));
        }
        writeln!(out, "  br label %hir_bb_{}", body.entry).unwrap();
        for block in &body.blocks {
            writeln!(out, "hir_bb_{}:", block.id).unwrap();
            if let Some((local, collection_slot, index_slot, element_ty)) =
                foreach_bindings.get(&block.id).cloned()
            {
                let (_, local_slot) = slots
                    .get(&local)
                    .cloned()
                    .ok_or_else(|| self.unsupported("HIR foreach binding local slot missing"))?;
                let collection = self.temp();
                writeln!(out, "  {} = load ptr, ptr {}", collection, collection_slot).unwrap();
                let index = self.temp();
                writeln!(out, "  {} = load i64, ptr {}", index, index_slot).unwrap();
                let element_ptr = self.temp();
                writeln!(
                    out,
                    "  {} = getelementptr inbounds i64, ptr {}, i64 {}",
                    element_ptr, collection, index
                )
                .unwrap();
                let encoded = self.temp();
                writeln!(out, "  {} = load i64, ptr {}", encoded, element_ptr).unwrap();
                let decoded = match element_ty {
                    LlType::I64 => encoded,
                    LlType::I1 => {
                        let value = self.temp();
                        writeln!(out, "  {} = trunc i64 {} to i1", value, encoded).unwrap();
                        value
                    }
                    LlType::F64 => {
                        let value = self.temp();
                        writeln!(out, "  {} = bitcast i64 {} to double", value, encoded).unwrap();
                        value
                    }
                    LlType::Ptr => {
                        let value = self.temp();
                        writeln!(out, "  {} = inttoptr i64 {} to ptr", value, encoded).unwrap();
                        value
                    }
                    LlType::Void => {
                        return Err(self.unsupported("cannot bind a void foreach element"))
                    }
                };
                writeln!(
                    out,
                    "  store {} {}, ptr {}",
                    element_ty.text(),
                    decoded,
                    local_slot
                )
                .unwrap();
            }
            for operation in &block.operations {
                match &operation.statement {
                    S::VariableDeclaration {
                        name, value, span, ..
                    }
                    | S::ConstDeclaration {
                        name, value, span, ..
                    } => {
                        let local = body
                            .locals
                            .iter()
                            .find(|l| l.name == *name && l.declaration_span == *span)
                            .ok_or_else(|| self.unsupported("HIR declaration local missing"))?;
                        let (ty, slot) = slots.get(&local.id).cloned().unwrap();
                        let value = self.emit_hir_scalar_expr(
                            value,
                            &[0],
                            &operation.local_references,
                            &operation.call_references,
                            &slots,
                            &mut out,
                        )?;
                        writeln!(out, "  store {} {}, ptr {}", ty.text(), value, slot).unwrap();
                    }
                    S::Assignment { value, .. } => {
                        let reference = operation
                            .local_references
                            .iter()
                            .find(|r| r.path == vec![0])
                            .ok_or_else(|| self.unsupported("assignment target is not a local"))?;
                        let (ty, slot) = slots
                            .get(&reference.local)
                            .cloned()
                            .ok_or_else(|| self.unsupported("assignment local missing"))?;
                        let value = self.emit_hir_scalar_expr(
                            value,
                            &[1],
                            &operation.local_references,
                            &operation.call_references,
                            &slots,
                            &mut out,
                        )?;
                        writeln!(out, "  store {} {}, ptr {}", ty.text(), value, slot).unwrap();
                    }
                    S::Expression { expression, .. } => {
                        let _ = self.emit_hir_scalar_expr(
                            expression,
                            &[0],
                            &operation.local_references,
                            &operation.call_references,
                            &slots,
                            &mut out,
                        )?;
                    }
                    _ => return Err(self.unsupported("unsupported direct HIR operation")),
                }
            }
            match &block.terminator {
                T::Goto(target) => {
                    if let Some(step) = loop_steps.get(target) {
                        writeln!(out, "  br label %{}", step).unwrap();
                    } else {
                        writeln!(out, "  br label %hir_bb_{}", target).unwrap();
                    }
                }
                T::Branch {
                    condition,
                    then_block,
                    else_block,
                } => {
                    let value = self.emit_hir_scalar_expr(
                        condition,
                        &[0],
                        &block.terminator_local_references,
                        &block.terminator_call_references,
                        &slots,
                        &mut out,
                    )?;
                    writeln!(
                        out,
                        "  br i1 {}, label %hir_bb_{}, label %hir_bb_{}",
                        value, then_block, else_block
                    )
                    .unwrap();
                }
                T::Return(Some(value)) => {
                    let value = self.emit_hir_scalar_expr(
                        value,
                        &[0],
                        &block.terminator_local_references,
                        &block.terminator_call_references,
                        &slots,
                        &mut out,
                    )?;
                    writeln!(out, "  ret {} {}", return_type.text(), value).unwrap();
                }
                T::Return(None) => match return_type {
                    LlType::Void => out.push_str("  ret void\n"),
                    LlType::I64 => out.push_str("  ret i64 0\n"),
                    LlType::I1 => out.push_str("  ret i1 false\n"),
                    LlType::F64 => out.push_str("  ret double 0.0\n"),
                    LlType::Ptr => out.push_str("  ret ptr null\n"),
                },
                T::For {
                    local,
                    start,
                    end,
                    body: body_target,
                    exit,
                    ..
                } => {
                    let (local_ty, slot) = slots
                        .get(local)
                        .cloned()
                        .ok_or_else(|| self.unsupported("HIR counted-loop local slot missing"))?;
                    if local_ty != LlType::I64 {
                        return Err(self.unsupported(
                            "direct HIR counted loops currently require integer variables",
                        ));
                    }
                    let start_value = self.emit_hir_scalar_expr(
                        start,
                        &[0],
                        &block.terminator_local_references,
                        &block.terminator_call_references,
                        &slots,
                        &mut out,
                    )?;
                    let end_value = self.emit_hir_scalar_expr(
                        end,
                        &[1],
                        &block.terminator_local_references,
                        &block.terminator_call_references,
                        &slots,
                        &mut out,
                    )?;
                    writeln!(out, "  store i64 {}, ptr {}", start_value, slot).unwrap();
                    let cond_label = format!("hir_for_cond_{}", block.id);
                    let step_label = format!("hir_for_step_{}", block.id);
                    loop_steps.insert(block.id, step_label.clone());
                    writeln!(out, "  br label %{}", cond_label).unwrap();
                    writeln!(out, "{}:", cond_label).unwrap();
                    let current = self.temp();
                    writeln!(out, "  {} = load i64, ptr {}", current, slot).unwrap();
                    let condition = self.temp();
                    writeln!(
                        out,
                        "  {} = icmp slt i64 {}, {}",
                        condition, current, end_value
                    )
                    .unwrap();
                    writeln!(
                        out,
                        "  br i1 {}, label %hir_bb_{}, label %hir_bb_{}",
                        condition, body_target, exit
                    )
                    .unwrap();
                    writeln!(out, "{}:", step_label).unwrap();
                    let current = self.temp();
                    writeln!(out, "  {} = load i64, ptr {}", current, slot).unwrap();
                    let incremented = self.temp();
                    writeln!(out, "  {} = add i64 {}, 1", incremented, current).unwrap();
                    writeln!(out, "  store i64 {}, ptr {}", incremented, slot).unwrap();
                    writeln!(out, "  br label %{}", cond_label).unwrap();
                }
                T::Match {
                    value,
                    arms,
                    fallback,
                } => {
                    let scrutinee = self.emit_hir_scalar_expr(
                        value,
                        &[0],
                        &block.terminator_local_references,
                        &block.terminator_call_references,
                        &slots,
                        &mut out,
                    )?;
                    let value_ty = hir_expr_scalar_type(
                        value,
                        &[0],
                        &block.terminator_local_references,
                        &block.terminator_call_references,
                        &slots,
                    )
                    .ok_or_else(|| {
                        self.unsupported("direct HIR match requires a scalar scrutinee")
                    })?;
                    for (index, arm) in arms.iter().enumerate() {
                        let next = if index + 1 == arms.len() {
                            format!("hir_match_fallback_{}", block.id)
                        } else {
                            format!("hir_match_{}_{}", block.id, index + 1)
                        };
                        let condition = self.emit_hir_pattern_condition(
                            &arm.pattern,
                            value_ty,
                            &scrutinee,
                            &mut out,
                        )?;
                        if let Some(condition) = condition {
                            writeln!(
                                out,
                                "  br i1 {}, label %hir_match_arm_{}_{}, label %{}",
                                condition, block.id, index, next
                            )
                            .unwrap();
                        } else {
                            writeln!(out, "  br label %hir_match_arm_{}_{}", block.id, index)
                                .unwrap();
                        }
                        writeln!(out, "hir_match_arm_{}_{}:", block.id, index).unwrap();
                        for binding in &arm.bindings {
                            if !binding.path.is_empty() {
                                return Err(self.unsupported("direct HIR match binding currently supports whole-scrutinee scalar bindings"));
                            }
                            let (binding_ty, slot) =
                                slots.get(&binding.local).cloned().ok_or_else(|| {
                                    self.unsupported("HIR match binding local slot missing")
                                })?;
                            if binding_ty != value_ty {
                                return Err(self.unsupported(
                                    "HIR match binding type differs from scalar scrutinee type",
                                ));
                            }
                            writeln!(
                                out,
                                "  store {} {}, ptr {}",
                                binding_ty.text(),
                                scrutinee,
                                slot
                            )
                            .unwrap();
                        }
                        if let Some(guard) = &arm.guard {
                            let guard_value = self.emit_hir_scalar_expr(
                                guard,
                                &[0],
                                &arm.guard_local_references,
                                &arm.guard_call_references,
                                &slots,
                                &mut out,
                            )?;
                            let guard_next = if index + 1 == arms.len() {
                                format!("hir_match_fallback_{}", block.id)
                            } else {
                                format!("hir_match_{}_{}", block.id, index + 1)
                            };
                            writeln!(
                                out,
                                "  br i1 {}, label %hir_bb_{}, label %{}",
                                guard_value, arm.block, guard_next
                            )
                            .unwrap();
                        } else {
                            writeln!(out, "  br label %hir_bb_{}", arm.block).unwrap();
                        }
                        if index + 1 < arms.len() {
                            writeln!(out, "hir_match_{}_{}:", block.id, index + 1).unwrap();
                        }
                    }
                    writeln!(out, "hir_match_fallback_{}:", block.id).unwrap();
                    writeln!(out, "  br label %hir_bb_{}", fallback).unwrap();
                }
                T::Break(Some(target)) => writeln!(out, "  br label %hir_bb_{}", target).unwrap(),
                T::Continue(Some(target)) => {
                    if let Some(step) = loop_steps.get(target) {
                        writeln!(out, "  br label %{}", step).unwrap();
                    } else {
                        writeln!(out, "  br label %hir_bb_{}", target).unwrap();
                    }
                }
                T::Break(None) | T::Continue(None) => {
                    return Err(self.unsupported("unresolved HIR loop-control target"))
                }
                T::Unreachable => out.push_str("  unreachable\n"),
                T::ForEach {
                    local,
                    iterable,
                    body: body_target,
                    exit,
                    ..
                } => {
                    let element_hir_type = match block.terminator_expression_types.iter().find(|typed| typed.path == vec![0]).map(|typed| &typed.ty) {
                        Some(crate::hir::HirType::Array(element)) => (**element).clone(),
                        _ => return Err(self.unsupported("direct HIR foreach currently requires an array with a resolved element type")),
                    };
                    let element_ty = hir_scalar_llvm_type(&element_hir_type).ok_or_else(|| {
                        self.unsupported("direct HIR foreach requires scalar array elements")
                    })?;
                    let (local_ty, _) = slots
                        .get(local)
                        .cloned()
                        .ok_or_else(|| self.unsupported("HIR foreach local slot missing"))?;
                    if local_ty != element_ty {
                        return Err(self.unsupported(
                            "HIR foreach local type does not match array element type",
                        ));
                    }
                    let collection = self.emit_hir_scalar_expr(
                        iterable,
                        &[0],
                        &block.terminator_local_references,
                        &block.terminator_call_references,
                        &slots,
                        &mut out,
                    )?;
                    let collection_slot = format!("%hir_foreach_collection_{}", block.id);
                    let index_slot = format!("%hir_foreach_index_{}", block.id);
                    let length_slot = format!("%hir_foreach_length_{}", block.id);
                    writeln!(out, "  {} = alloca ptr, align 8", collection_slot).unwrap();
                    writeln!(out, "  store ptr {}, ptr {}", collection, collection_slot).unwrap();
                    writeln!(out, "  {} = alloca i64, align 8", index_slot).unwrap();
                    writeln!(out, "  store i64 0, ptr {}", index_slot).unwrap();
                    writeln!(out, "  {} = alloca i64, align 8", length_slot).unwrap();
                    let length = self.temp();
                    writeln!(
                        out,
                        "  {} = call i64 @fusion_tuple_len(ptr {})",
                        length, collection
                    )
                    .unwrap();
                    writeln!(out, "  store i64 {}, ptr {}", length, length_slot).unwrap();
                    let cond_label = format!("hir_foreach_cond_{}", block.id);
                    let step_label = format!("hir_foreach_step_{}", block.id);
                    loop_steps.insert(block.id, step_label.clone());
                    foreach_bindings.insert(
                        *body_target,
                        (*local, collection_slot, index_slot.clone(), element_ty),
                    );
                    writeln!(out, "  br label %{}", cond_label).unwrap();
                    writeln!(out, "{}:", cond_label).unwrap();
                    let index = self.temp();
                    writeln!(out, "  {} = load i64, ptr {}", index, index_slot).unwrap();
                    let limit = self.temp();
                    writeln!(out, "  {} = load i64, ptr {}", limit, length_slot).unwrap();
                    let in_bounds = self.temp();
                    writeln!(out, "  {} = icmp slt i64 {}, {}", in_bounds, index, limit).unwrap();
                    writeln!(
                        out,
                        "  br i1 {}, label %hir_bb_{}, label %hir_bb_{}",
                        in_bounds, body_target, exit
                    )
                    .unwrap();
                    writeln!(out, "{}:", step_label).unwrap();
                    let current = self.temp();
                    writeln!(out, "  {} = load i64, ptr {}", current, index_slot).unwrap();
                    let next = self.temp();
                    writeln!(out, "  {} = add i64 {}, 1", next, current).unwrap();
                    writeln!(out, "  store i64 {}, ptr {}", next, index_slot).unwrap();
                    writeln!(out, "  br label %{}", cond_label).unwrap();
                }
                T::Open => return Err(self.unsupported("unterminated HIR basic block")),
            }
        }
        out.push_str("}\n");
        Ok(out)
    }

    fn emit_hir_pattern_condition(
        &mut self,
        pattern: &crate::hir::HirPattern,
        value_ty: LlType,
        value: &str,
        out: &mut String,
    ) -> Result<Option<String>, FusionError> {
        use crate::hir::HirPattern as P;
        match pattern {
            P::Wildcard | P::Identifier(_) => Ok(None),
            P::At { pattern, .. } => self.emit_hir_pattern_condition(pattern, value_ty, value, out),
            P::Number(expected) if value_ty == LlType::I64 => {
                let temp = self.temp(); writeln!(out, "  {} = icmp eq i64 {}, {}", temp, value, expected).unwrap(); Ok(Some(temp))
            }
            P::Boolean(expected) if value_ty == LlType::I1 => {
                let literal = if *expected { "true" } else { "false" };
                let temp = self.temp(); writeln!(out, "  {} = icmp eq i1 {}, {}", temp, value, literal).unwrap(); Ok(Some(temp))
            }
            P::Range { start, end, inclusive } if value_ty == LlType::I64 => {
                let lower = self.temp(); writeln!(out, "  {} = icmp sge i64 {}, {}", lower, value, start).unwrap();
                let upper = self.temp(); writeln!(out, "  {} = icmp {} i64 {}, {}", upper, if *inclusive { "sle" } else { "slt" }, value, end).unwrap();
                let both = self.temp(); writeln!(out, "  {} = and i1 {}, {}", both, lower, upper).unwrap(); Ok(Some(both))
            }
            P::Or(alternatives) => {
                let mut combined: Option<String> = None;
                for alternative in alternatives {
                    let Some(next) = self.emit_hir_pattern_condition(alternative, value_ty, value, out)? else { return Ok(None); };
                    combined = Some(if let Some(previous) = combined {
                        let joined = self.temp(); writeln!(out, "  {} = or i1 {}, {}", joined, previous, next).unwrap(); joined
                    } else { next });
                }
                Ok(combined)
            }
            _ => Err(self.unsupported("direct HIR match currently supports wildcard, integer, boolean, range, and or-pattern tests")),
        }
    }

    fn emit_hir_scalar_expr(
        &mut self,
        expression: &crate::hir::HirExpression,
        path: &[u32],
        refs: &[crate::hir::HirLocalReference],
        calls: &[crate::hir::HirCallReference],
        slots: &HashMap<crate::hir::HirLocalId, (LlType, String)>,
        out: &mut String,
    ) -> Result<String, FusionError> {
        use crate::hir::{HirExpression as E, HirOperator as O, HirUnaryOperator as U};
        match expression {
            E::Number(n) => Ok(n.to_string()),
            E::Float(value) => Ok(format!("{:.17e}", value)),
            E::Boolean(v) => Ok(if *v { "true" } else { "false" }.into()),
            E::String(value) => Ok(self.string_ptr(value)),
            E::Array(elements) | E::Tuple(elements) => {
                let collection = self.temp();
                writeln!(
                    out,
                    "  {} = call ptr @fusion_tuple_new(i64 {})",
                    collection,
                    elements.len()
                )
                .unwrap();
                let mut element_type: Option<LlType> = None;
                for (index, element) in elements.iter().enumerate() {
                    let mut child_path = path.to_vec();
                    child_path.push(index as u32);
                    let value =
                        self.emit_hir_scalar_expr(element, &child_path, refs, calls, slots, out)?;
                    let ty = hir_expr_scalar_type(element, &child_path, refs, calls, slots)
                        .ok_or_else(|| self.unsupported("HIR array element type is unresolved"))?;
                    if let Some(previous) = element_type {
                        if previous != ty {
                            return Err(self
                                .unsupported("heterogeneous HIR array elements are unsupported"));
                        }
                    } else {
                        element_type = Some(ty);
                    }
                    let encoded = match ty {
                        LlType::I64 => value,
                        LlType::I1 => {
                            let widened = self.temp();
                            writeln!(out, "  {} = zext i1 {} to i64", widened, value).unwrap();
                            widened
                        }
                        LlType::F64 => {
                            let bits = self.temp();
                            writeln!(out, "  {} = bitcast double {} to i64", bits, value).unwrap();
                            bits
                        }
                        LlType::Ptr => {
                            let bits = self.temp();
                            writeln!(out, "  {} = ptrtoint ptr {} to i64", bits, value).unwrap();
                            bits
                        }
                        LlType::Void => {
                            return Err(
                                self.unsupported("void values cannot be stored in HIR arrays")
                            )
                        }
                    };
                    let element_slot = self.temp();
                    writeln!(
                        out,
                        "  {} = getelementptr inbounds i64, ptr {}, i64 {}",
                        element_slot, collection, index
                    )
                    .unwrap();
                    writeln!(out, "  store i64 {}, ptr {}", encoded, element_slot).unwrap();
                }
                Ok(collection)
            }
            E::Identifier(_) => {
                let r = refs
                    .iter()
                    .find(|r| r.path == path)
                    .ok_or_else(|| self.unsupported("unresolved HIR local read"))?;
                let (ty, slot) = slots
                    .get(&r.local)
                    .cloned()
                    .ok_or_else(|| self.unsupported("HIR local slot missing"))?;
                let temp = self.temp();
                writeln!(out, "  {} = load {}, ptr {}", temp, ty.text(), slot).unwrap();
                Ok(temp)
            }
            E::Call { arguments, .. } => {
                self.emit_hir_call_expression(path, false, None, arguments, refs, calls, slots, out)
            }
            E::MethodCall {
                object, arguments, ..
            } => self.emit_hir_call_expression(
                path,
                true,
                Some(object),
                arguments,
                refs,
                calls,
                slots,
                out,
            ),
            E::Binary {
                left,
                operator,
                right,
            } => {
                let mut lp = path.to_vec();
                lp.push(0);
                let mut rp = path.to_vec();
                rp.push(1);
                let l = self.emit_hir_scalar_expr(left, &lp, refs, calls, slots, out)?;
                let r = self.emit_hir_scalar_expr(right, &rp, refs, calls, slots, out)?;
                let ty = hir_expr_scalar_type(left, &lp, refs, calls, slots).unwrap_or(LlType::I64);
                let op = match (operator, ty) {
                    (O::Plus, LlType::I64) => format!("add i64 {}, {}", l, r),
                    (O::Minus, LlType::I64) => format!("sub i64 {}, {}", l, r),
                    (O::Multiply, LlType::I64) => format!("mul i64 {}, {}", l, r),
                    (O::Divide, LlType::I64) => format!("sdiv i64 {}, {}", l, r),
                    (O::Equal, LlType::I64) => format!("icmp eq i64 {}, {}", l, r),
                    (O::NotEqual, LlType::I64) => format!("icmp ne i64 {}, {}", l, r),
                    (O::Less, LlType::I64) => format!("icmp slt i64 {}, {}", l, r),
                    (O::LessEqual, LlType::I64) => format!("icmp sle i64 {}, {}", l, r),
                    (O::Greater, LlType::I64) => format!("icmp sgt i64 {}, {}", l, r),
                    (O::GreaterEqual, LlType::I64) => format!("icmp sge i64 {}, {}", l, r),
                    (O::And, LlType::I1) => format!("and i1 {}, {}", l, r),
                    (O::Or, LlType::I1) => format!("or i1 {}, {}", l, r),
                    _ => return Err(self.unsupported("unsupported HIR scalar operator/type")),
                };
                let temp = self.temp();
                writeln!(out, "  {} = {}", temp, op).unwrap();
                Ok(temp)
            }
            E::Unary {
                operator,
                expression,
            } => {
                let mut child = path.to_vec();
                child.push(0);
                let value =
                    self.emit_hir_scalar_expr(expression, &child, refs, calls, slots, out)?;
                let temp = self.temp();
                match operator {
                    U::Negate => writeln!(out, "  {} = sub i64 0, {}", temp, value).unwrap(),
                    U::Not => writeln!(out, "  {} = xor i1 {}, true", temp, value).unwrap(),
                    _ => return Err(self.unsupported("unsupported HIR unary operator")),
                };
                Ok(temp)
            }
            _ => Err(self.unsupported("unsupported HIR scalar expression")),
        }
    }

    fn emit_hir_call_expression(
        &mut self,
        path: &[u32],
        is_method: bool,
        receiver: Option<&crate::hir::HirExpression>,
        arguments: &[crate::hir::HirExpression],
        refs: &[crate::hir::HirLocalReference],
        calls: &[crate::hir::HirCallReference],
        slots: &HashMap<crate::hir::HirLocalId, (LlType, String)>,
        out: &mut String,
    ) -> Result<String, FusionError> {
        let reference = calls
            .iter()
            .find(|reference| reference.path == path)
            .ok_or_else(|| self.unsupported("HIR call has no resolved declaration identity"))?;
        let call = &reference.call;
        if (call.kind == crate::hir::HirCallKind::Method) != is_method {
            return Err(self.unsupported("HIR call kind does not match expression"));
        }
        if call.is_async {
            return Err(
                self.unsupported("async HIR calls remain on the compatibility backend path")
            );
        }
        let symbol = self
            .function_symbols_by_id
            .get(&call.declaration)
            .cloned()
            .ok_or_else(|| {
                self.unsupported(format!(
                    "no LLVM symbol registered for HIR declaration {}::{}",
                    call.declaration.module, call.declaration.name
                ))
            })?;
        let return_ty = hir_scalar_llvm_type(&call.return_hir_type)
            .filter(|ty| *ty != LlType::Void)
            .ok_or_else(|| {
                self.unsupported("direct HIR calls currently require a scalar non-void return type")
            })?;
        let mut argument_values: Vec<(LlType, String)> = Vec::new();
        if let Some(receiver) = receiver {
            let receiver_ty = call
                .receiver_hir_type
                .as_ref()
                .and_then(hir_scalar_llvm_type)
                .ok_or_else(|| {
                    self.unsupported("direct HIR method calls require a supported receiver type")
                })?;
            let mut receiver_path = path.to_vec();
            receiver_path.push(0);
            let value =
                self.emit_hir_scalar_expr(receiver, &receiver_path, refs, calls, slots, out)?;
            argument_values.push((receiver_ty, value));
        }
        if arguments.len() != call.argument_hir_types.len()
            || arguments.len() != call.parameter_hir_types.len()
        {
            return Err(self.unsupported("resolved HIR call argument signature is incomplete"));
        }
        for (index, argument) in arguments.iter().enumerate() {
            let expected = hir_scalar_llvm_type(&call.parameter_hir_types[index])
                .or_else(|| hir_scalar_llvm_type(&call.argument_hir_types[index]))
                .ok_or_else(|| self.unsupported("direct HIR call argument type is not scalar"))?;
            let actual = hir_scalar_llvm_type(&call.argument_hir_types[index])
                .ok_or_else(|| self.unsupported("direct HIR call argument type is unresolved"))?;
            if expected != actual {
                return Err(self.unsupported(
                    "direct HIR call argument type does not match resolved parameter type",
                ));
            }
            let child_index = index as u32 + if is_method { 1 } else { 0 };
            let mut argument_path = path.to_vec();
            argument_path.push(child_index);
            let value =
                self.emit_hir_scalar_expr(argument, &argument_path, refs, calls, slots, out)?;
            argument_values.push((expected, value));
        }
        let target_signature = self.functions.get(&symbol).cloned().ok_or_else(|| {
            self.unsupported("resolved HIR call target has no registered LLVM signature")
        })?;
        if target_signature.ret != return_ty
            || target_signature.params.len() != argument_values.len()
            || target_signature
                .params
                .iter()
                .zip(&argument_values)
                .any(|(expected, (actual, _))| expected != actual)
        {
            return Err(self.unsupported(
                "resolved HIR call signature does not match the registered LLVM function ABI",
            ));
        }
        let args = argument_values
            .iter()
            .map(|(ty, value)| format!("{} {}", ty.text(), value))
            .collect::<Vec<_>>()
            .join(", ");
        let result = self.temp();
        writeln!(
            out,
            "  {} = call {} @{}({})",
            result,
            return_ty.text(),
            sanitize_symbol(&symbol),
            args
        )
        .unwrap();
        Ok(result)
    }

    fn emit_function(
        &mut self,
        stmt: &Statement,
        name: &str,
        owner_identity: Option<&crate::hir::HirDefinitionId>,
    ) -> Result<(), FusionError> {
        let Statement::Function {
            parameters, body, ..
        } = stmt
        else {
            return Ok(());
        };
        let sig = self.functions.get(name).cloned().unwrap();
        if let Some(body) = self
            .hir_bodies
            .iter()
            .find(|body| {
                if let Some(identity) = owner_identity {
                    body.owner.as_ref() == Some(identity)
                } else {
                    body.name == name
                        && self
                            .current_module
                            .as_deref()
                            .map(|module| module == body.module)
                            .unwrap_or(true)
                        && body
                            .owner
                            .as_ref()
                            .map(|owner| owner.kind == crate::hir::HirDefinitionKind::Function)
                            .unwrap_or(false)
                }
            })
            .cloned()
        {
            let parameter_types: Vec<_> = body
                .locals
                .iter()
                .filter(|local| local.parameter)
                .map(|local| local.ty.clone())
                .collect();
            let return_type = match sig.ret {
                LlType::I64 => crate::hir::HirType::Num,
                LlType::I1 => crate::hir::HirType::Bool,
                LlType::F64 => crate::hir::HirType::Float,
                LlType::Ptr => crate::hir::HirType::String,
                LlType::Void => crate::hir::HirType::Void,
            };
            let signature_matches = parameter_types.len() == sig.params.len()
                && parameter_types
                    .iter()
                    .zip(&sig.params)
                    .all(|(hir, llvm)| hir_scalar_llvm_type(hir) == Some(*llvm));
            if signature_matches {
                if let Ok(llvm_body) =
                    self.emit_hir_body(&body, name, &parameter_types, &return_type)
                {
                    self.module.push_str(&llvm_body);
                    return Ok(());
                }
            }
        }
        self.reset_function_state(sig.ret);
        let mut b = String::new();
        write!(b, "define {} @{}(", sig.ret.text(), sanitize_symbol(name)).unwrap();
        for (i, p) in sig.params.iter().enumerate() {
            if i > 0 {
                b.push_str(", ");
            }
            write!(b, "{} %arg{}", p.text(), i).unwrap();
        }
        b.push_str(") {\nentry:\n");
        let parameter_offset = sig.params.len().saturating_sub(parameters.len());
        for (i, p) in parameters.iter().enumerate() {
            let argument_index = i + parameter_offset;
            let ty = sig.params[argument_index];
            let slot = format!("%local_{}", sanitize_symbol(&p.name));
            writeln!(b, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
            writeln!(
                b,
                "  store {} %arg{}, ptr {}",
                ty.text(),
                argument_index,
                slot
            )
            .unwrap();
            self.locals.insert(p.name.clone(), (ty, slot));
            if let Some(identity) = p
                .type_name
                .as_deref()
                .and_then(|name| self.nominal_identity_for_type_name(name))
            {
                self.nominal_types_by_local.insert(p.name.clone(), identity);
            }
        }
        self.emit_statements(body, &mut b)?;
        if !Self::output_ends_in_terminator(&b) {
            match sig.ret {
                LlType::Void => writeln!(b, "  ret void").unwrap(),
                LlType::I64 => writeln!(b, "  ret i64 0").unwrap(),
                LlType::I1 => writeln!(b, "  ret i1 false").unwrap(),
                LlType::F64 => writeln!(b, "  ret double 0.0").unwrap(),
                LlType::Ptr => writeln!(b, "  ret ptr null").unwrap(),
            }
        }
        writeln!(b, "}}").unwrap();
        self.module.push_str(&b);
        Ok(())
    }

    fn emit_statements(&mut self, body: &[Statement], out: &mut String) -> Result<(), FusionError> {
        for statement in body {
            self.emit_statement(statement, out)?;
            if Self::output_ends_in_terminator(out) {
                break;
            }
        }
        Ok(())
    }

    fn emit_statement(&mut self, s: &Statement, out: &mut String) -> Result<(), FusionError> {
        match s {
            Statement::VariableDeclarations { declarations, .. } => {
                for d in declarations {
                    let value = if let Some(v) = &d.value {
                        self.emit_expr(v, out)?
                    } else {
                        self.zero_for(
                            self.llvm_type_name(d.declared_type.as_deref().unwrap_or("num")),
                        )
                    };
                    let ty = self.expr_type(d.value.as_ref().unwrap_or(&Expression::Number {
                        value: 0,
                        span: crate::span::Span::new(0, 0),
                    }));
                    let ty = d
                        .declared_type
                        .as_deref()
                        .map(|x| self.llvm_type_name(x))
                        .unwrap_or(ty);
                    let slot = format!("%local_{}_{}", sanitize_symbol(&d.name), self.next_temp);
                    writeln!(out, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
                    writeln!(out, "  store {} {}, ptr {}", ty.text(), value, slot).unwrap();
                    self.locals.insert(d.name.clone(), (ty, slot));
                    if let Some(identity) = d
                        .declared_type
                        .as_deref()
                        .and_then(|name| self.nominal_identity_for_type_name(name))
                        .or_else(|| {
                            d.value
                                .as_ref()
                                .and_then(|value| self.nominal_identity_for_expression(value))
                        })
                    {
                        self.nominal_types_by_local.insert(d.name.clone(), identity);
                    }
                    if let Some(Expression::Tuple { elements, .. }) = d.value.as_ref() {
                        let tuple_types = elements
                            .iter()
                            .map(|e| self.fusion_type_of_expr(e))
                            .collect();
                        self.tuple_types.insert(d.name.clone(), tuple_types);
                    }
                    if let Some(Expression::Array { elements, .. }) = d.value.as_ref() {
                        let element_type = elements
                            .first()
                            .map(|e| self.fusion_type_of_expr(e))
                            .unwrap_or(Type::Unknown);
                        self.array_types.insert(d.name.clone(), element_type);
                    }
                }
            }
            Statement::ConstDeclaration {
                name,
                value,
                declared_type,
                ..
            } => {
                let tuple_types = if let Expression::Tuple { elements, .. } = value {
                    Some(
                        elements
                            .iter()
                            .map(|e| self.fusion_type_of_expr(e))
                            .collect(),
                    )
                } else {
                    None
                };
                let ty = declared_type
                    .as_deref()
                    .map(|x| self.llvm_type_name(x))
                    .unwrap_or_else(|| self.expr_type(value));
                let emitted_value = self.emit_expr(value, out)?;
                let slot = format!("%local_{}_{}", sanitize_symbol(name), self.next_temp);
                writeln!(out, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
                writeln!(out, "  store {} {}, ptr {}", ty.text(), emitted_value, slot).unwrap();
                self.locals.insert(name.clone(), (ty, slot));
                if let Some(identity) = declared_type
                    .as_deref()
                    .and_then(|name| self.nominal_identity_for_type_name(name))
                    .or_else(|| self.nominal_identity_for_expression(value))
                {
                    self.nominal_types_by_local.insert(name.clone(), identity);
                }
                if let Some(tuple_types) = tuple_types {
                    self.tuple_types.insert(name.clone(), tuple_types);
                }
                if let Expression::Array { elements, .. } = value {
                    let element_type = elements
                        .first()
                        .map(|e| self.fusion_type_of_expr(e))
                        .unwrap_or(Type::Unknown);
                    self.array_types.insert(name.clone(), element_type);
                }
            }
            Statement::Assignment { target, value, .. } => {
                if let Expression::Property {
                    object,
                    name: field_name,
                    ..
                } = target
                {
                    let (address, native_type, _) =
                        self.emit_struct_field_address(object, field_name, out)?;
                    let emitted = self.emit_expr(value, out)?;
                    writeln!(
                        out,
                        "  store {} {}, ptr {}",
                        native_type.text(),
                        emitted,
                        address
                    )
                    .unwrap();
                    return Ok(());
                }
                let Expression::Identifier { name, .. } = target else {
                    return Err(self.unsupported(
                        "assignment target must be a local or a resolved struct field",
                    ));
                };

                if let Some((ty, slot)) = self.locals.get(name).cloned() {
                    // Existing variable: evaluate the new value and store it.
                    let v = self.emit_expr(value, out)?;
                    writeln!(out, "  store {} {}, ptr {}", ty.text(), v, slot).unwrap();
                    if let Some(identity) = self.nominal_identity_for_expression(value) {
                        self.nominal_types_by_local.insert(name.clone(), identity);
                    }
                    if let Expression::Tuple { elements, .. } = value {
                        let tuple_types = elements
                            .iter()
                            .map(|e| self.fusion_type_of_expr(e))
                            .collect();
                        self.tuple_types.insert(name.clone(), tuple_types);
                    }
                    if let Expression::Array { elements, .. } = value {
                        let element_type = elements
                            .first()
                            .map(|e| self.fusion_type_of_expr(e))
                            .unwrap_or(Type::Unknown);
                        self.array_types.insert(name.clone(), element_type);
                    }
                } else {
                    // New variable: Fusion treats `x = value` as an inferred declaration.
                    let ty = self.expr_type(value);
                    let v = self.emit_expr(value, out)?;
                    let slot = format!("%local_{}_{}", sanitize_symbol(name), self.next_temp);

                    writeln!(out, "  {} = alloca {}, align 8", slot, ty.text()).unwrap();
                    writeln!(out, "  store {} {}, ptr {}", ty.text(), v, slot).unwrap();

                    self.locals.insert(name.clone(), (ty, slot));
                    if let Some(identity) = self.nominal_identity_for_expression(value) {
                        self.nominal_types_by_local.insert(name.clone(), identity);
                    }
                    if let Expression::Tuple { elements, .. } = value {
                        let tuple_types = elements
                            .iter()
                            .map(|e| self.fusion_type_of_expr(e))
                            .collect();
                        self.tuple_types.insert(name.clone(), tuple_types);
                    }
                    if let Expression::Array { elements, .. } = value {
                        let element_type = elements
                            .first()
                            .map(|e| self.fusion_type_of_expr(e))
                            .unwrap_or(Type::Unknown);
                        self.array_types.insert(name.clone(), element_type);
                    }
                }
            }
            Statement::Expression { expression, .. } | Statement::Call { expression, .. } => {
                let _ = self.emit_expr(expression, out)?;
            }
            Statement::Return { value, .. } => {
                if self.current_is_main {
                    return Err(self.unsupported(
                        "main cannot contain return; process termination is owned by the runtime",
                    ));
                }
                if let Some(v) = value {
                    let x = self.emit_expr(v, out)?;
                    writeln!(out, "  ret {} {}", self.current_ret.text(), x).unwrap();
                } else {
                    writeln!(out, "  ret void").unwrap();
                }
            }
            Statement::If {
                condition,
                body,
                else_body,
                ..
            } => self.emit_if(condition, body, else_body.as_deref(), out)?,
            Statement::While {
                condition, body, ..
            } => self.emit_while(condition, body, out)?,
            Statement::For {
                variable,
                start,
                end,
                body,
                ..
            } => self.emit_for(variable, start, end, body, out)?,
            Statement::Break { .. } => {
                let target = self
                    .loop_targets
                    .last()
                    .map(|targets| targets.break_label.clone())
                    .ok_or_else(|| self.unsupported("break used outside a loop"))?;
                writeln!(out, "  br label %{}", target).unwrap();
            }
            Statement::Continue { .. } => {
                let target = self
                    .loop_targets
                    .last()
                    .map(|targets| targets.continue_label.clone())
                    .ok_or_else(|| self.unsupported("continue used outside a loop"))?;
                writeln!(out, "  br label %{}", target).unwrap();
            }
            Statement::Defer { .. } => {
                return Err(self.unsupported("defer requires cleanup CFG lowering"));
            }
            Statement::ForEach {
                variable,
                iterable,
                body,
                ..
            } => {
                self.emit_for_each(variable, iterable, body, out)?;
            }
            Statement::Match {
                expression, arms, ..
            } => {
                self.emit_nominal_match(expression, arms, out)?;
            }
            Statement::Struct { .. }
            | Statement::Enum { .. }
            | Statement::Trait { .. }
            | Statement::Impl { .. }
            | Statement::Main { .. }
            | Statement::Function { .. }
            | Statement::Import { .. } => {}
        }
        Ok(())
    }

    /// Lower match statements through the HIR-aware pattern pipeline. Nominal
    /// children resolve through stable declaration identities; scalar, tuple,
    /// range, and or-patterns share the same short-circuit decision CFG.
    fn emit_nominal_match(
        &mut self,
        expression: &Expression,
        arms: &[MatchArm],
        out: &mut String,
    ) -> Result<(), FusionError> {
        let nominal_identity = self.nominal_identity_for_expression(expression);
        let scrutinee_type = if let Some(identity) = nominal_identity.as_ref() {
            let symbol = self
                .type_definition_ids
                .iter()
                .find_map(|(symbol, candidate)| (candidate == identity).then(|| symbol.clone()))
                .ok_or_else(|| {
                    self.unsupported("nominal HIR identity has no compatibility type symbol")
                })?;
            match identity.kind {
                crate::hir::HirDefinitionKind::Enum => Type::Enum(symbol),
                crate::hir::HirDefinitionKind::Struct => Type::Struct(symbol),
                _ => {
                    return Err(self.unsupported(format!(
                        "match CFG lowering cannot match nominal {}::{}",
                        identity.module, identity.name
                    )))
                }
            }
        } else {
            self.fusion_type_of_expr(expression)
        };
        let emitted_scrutinee = self.emit_expr(expression, out)?;
        let scrutinee_is_object = matches!(
            &scrutinee_type,
            Type::Struct(_) | Type::Enum(_) | Type::Tuple(_) | Type::String | Type::Array(_)
        );
        let scrutinee_value = if scrutinee_is_object {
            emitted_scrutinee
        } else {
            let slot = self.temp();
            let native = self.native_type_for_fusion_type(&scrutinee_type);
            writeln!(out, "  {} = alloca {}, align 8", slot, native.text()).unwrap();
            writeln!(
                out,
                "  store {} {}, ptr {}",
                native.text(),
                emitted_scrutinee,
                slot
            )
            .unwrap();
            slot
        };

        let end_block = self.block("match.end");
        let saved_locals = self.locals.clone();
        let saved_nominal = self.nominal_types_by_local.clone();
        let saved_tuples = self.tuple_types.clone();
        let saved_arrays = self.array_types.clone();
        let mut every_arm_terminates = !arms.is_empty();

        for (arm_index, arm) in arms.iter().enumerate() {
            let arm_block = self.block(&format!("match.arm.{}", arm_index));
            let next_block = self.block(&format!("match.next.{}", arm_index));
            self.emit_hir_pattern_test(
                &arm.pattern,
                &scrutinee_type,
                &scrutinee_value,
                scrutinee_is_object,
                &arm_block,
                &next_block,
                out,
            )?;
            writeln!(out, "{}:", arm_block).unwrap();

            if let Some(guard) = &arm.guard {
                let guard_value = self.emit_expr(guard, out)?;
                let body_block = self.block(&format!("match.guard.{}", arm_index));
                writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    guard_value, body_block, next_block
                )
                .unwrap();
                writeln!(out, "{}:", body_block).unwrap();
            }

            self.emit_statements(&arm.body, out)?;
            let arm_terminates = Self::output_ends_in_terminator(out);
            every_arm_terminates &= arm_terminates;
            if !arm_terminates {
                writeln!(out, "  br label %{}", end_block).unwrap();
            }
            self.locals = saved_locals.clone();
            self.nominal_types_by_local = saved_nominal.clone();
            self.tuple_types = saved_tuples.clone();
            self.array_types = saved_arrays.clone();
            writeln!(out, "{}:", next_block).unwrap();
        }
        let exhaustive = self.match_is_exhaustive(&scrutinee_type, arms);
        if exhaustive {
            writeln!(out, "  unreachable").unwrap();
        } else {
            writeln!(out, "  br label %{}", end_block).unwrap();
        }
        writeln!(out, "{}:", end_block).unwrap();
        if every_arm_terminates && exhaustive {
            writeln!(out, "  unreachable").unwrap();
        }
        self.locals = saved_locals;
        self.nominal_types_by_local = saved_nominal;
        self.tuple_types = saved_tuples;
        self.array_types = saved_arrays;
        Ok(())
    }

    /// Emits a conditional pattern test. `value` is either the address of a
    /// scalar slot (`is_object == false`) or the opaque pointer to a nominal
    /// struct/enum object (`is_object == true`). The method branches to exactly
    /// one of `success` or `failure` and recursively handles nominal children.
    fn emit_hir_pattern_test(
        &mut self,
        pattern: &Pattern,
        value_type: &Type,
        value: &str,
        is_object: bool,
        success: &str,
        failure: &str,
        out: &mut String,
    ) -> Result<(), FusionError> {
        match &pattern.kind {
            PatternKind::Wildcard => {
                writeln!(out, "  br label %{}", success).unwrap();
            }
            PatternKind::Identifier(binding) => {
                self.emit_pattern_binding(binding, value_type, value, is_object, out)?;
                writeln!(out, "  br label %{}", success).unwrap();
            }
            PatternKind::At {
                name,
                pattern: nested,
            } => {
                self.emit_pattern_binding(name, value_type, value, is_object, out)?;
                self.emit_hir_pattern_test(
                    nested, value_type, value, is_object, success, failure, out,
                )?;
            }
            PatternKind::Variant { name, bindings } => {
                let Type::Enum(enum_symbol) = value_type else {
                    return Err(self.unsupported(format!(
                        "enum variant pattern '{}' applied to non-enum value",
                        name
                    )));
                };
                let (pattern_enum_symbol, variant_name) =
                    name.rsplit_once("::").ok_or_else(|| {
                        self.unsupported(format!(
                            "enum pattern '{}' must be qualified as Type::Variant",
                            name
                        ))
                    })?;
                let expected_enum_id = self
                    .type_definition_ids
                    .get(enum_symbol)
                    .cloned()
                    .ok_or_else(|| {
                        self.unsupported(format!("enum type '{}' has no HIR identity", enum_symbol))
                    })?;
                let pattern_enum_id = self
                    .type_definition_ids
                    .get(pattern_enum_symbol)
                    .cloned()
                    .ok_or_else(|| {
                        self.unsupported(format!(
                            "enum pattern '{}' has no HIR type identity",
                            name
                        ))
                    })?;
                if expected_enum_id != pattern_enum_id {
                    return Err(self.unsupported(format!(
                        "enum pattern '{}' resolves to {}::{}, but value is {}::{}",
                        name,
                        pattern_enum_id.module,
                        pattern_enum_id.name,
                        expected_enum_id.module,
                        expected_enum_id.name
                    )));
                }
                let variant_id = self
                    .enum_variant_definition_ids
                    .get(&(pattern_enum_symbol.to_string(), variant_name.to_string()))
                    .cloned()
                    .ok_or_else(|| {
                        self.unsupported(format!("unknown HIR enum variant '{}'", name))
                    })?;
                let discriminant = self
                    .enum_variant_discriminant_by_id(&variant_id)
                    .ok_or_else(|| {
                        self.unsupported(format!("enum variant '{}' has no discriminant", name))
                    })?;
                let enum_layout = self
                    .llvm_type_name_by_id(&expected_enum_id)
                    .ok_or_else(|| {
                        self.unsupported(format!(
                            "LLVM enum layout missing for {}::{}",
                            expected_enum_id.module, expected_enum_id.name
                        ))
                    })?
                    .to_string();
                let tag_address = self.temp();
                writeln!(
                    out,
                    "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 0",
                    tag_address, enum_layout, value
                )
                .unwrap();
                let tag = self.temp();
                writeln!(out, "  {} = load i32, ptr {}", tag, tag_address).unwrap();
                let compare = self.temp();
                writeln!(out, "  {} = icmp eq i32 {}, {}", compare, tag, discriminant).unwrap();
                let matched = self.block("pattern.variant.payload");
                writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    compare, matched, failure
                )
                .unwrap();
                writeln!(out, "{}:", matched).unwrap();

                let payload_types = self
                    .enum_payloads_by_id
                    .get(&variant_id)
                    .cloned()
                    .ok_or_else(|| {
                        self.unsupported(format!(
                            "payload layout missing for enum variant '{}'",
                            name
                        ))
                    })?;
                if payload_types.len() != bindings.len() {
                    return Err(self.unsupported(format!(
                        "enum pattern '{}' has {} binding(s), but its payload has {} field(s)",
                        name,
                        bindings.len(),
                        payload_types.len()
                    )));
                }
                let payload_layout = self
                    .enum_variant_layout_names_by_id
                    .get(&variant_id)
                    .cloned()
                    .ok_or_else(|| {
                        self.unsupported(format!("payload LLVM layout missing for '{}'", name))
                    })?;
                let payload_address = self.temp();
                writeln!(
                    out,
                    "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 1",
                    payload_address, enum_layout, value
                )
                .unwrap();
                let payload = self.temp();
                writeln!(out, "  {} = load ptr, ptr {}", payload, payload_address).unwrap();

                for (index, (child_pattern, child_type)) in
                    bindings.iter().zip(payload_types.iter()).enumerate()
                {
                    let field_address = self.temp();
                    writeln!(
                        out,
                        "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 {}",
                        field_address, payload_layout, payload, index
                    )
                    .unwrap();
                    let child_is_nominal = matches!(child_type, Type::Struct(_) | Type::Enum(_));
                    let child_is_object = child_is_nominal
                        || matches!(child_type, Type::Tuple(_) | Type::String | Type::Array(_));
                    let child_value = if child_is_object {
                        let loaded = self.temp();
                        writeln!(out, "  {} = load ptr, ptr {}", loaded, field_address).unwrap();
                        loaded
                    } else {
                        field_address
                    };
                    let child_success = self.block("pattern.payload.next");
                    self.emit_hir_pattern_test(
                        child_pattern,
                        child_type,
                        &child_value,
                        child_is_object,
                        &child_success,
                        failure,
                        out,
                    )?;
                    writeln!(out, "{}:", child_success).unwrap();
                }
                writeln!(out, "  br label %{}", success).unwrap();
            }
            PatternKind::Struct { name, fields, .. } => {
                let Type::Struct(struct_symbol) = value_type else {
                    return Err(self.unsupported(format!(
                        "struct pattern '{}' applied to non-struct value",
                        name
                    )));
                };
                let expected_id = self
                    .type_definition_ids
                    .get(struct_symbol)
                    .cloned()
                    .ok_or_else(|| {
                        self.unsupported(format!(
                            "struct type '{}' has no HIR identity",
                            struct_symbol
                        ))
                    })?;
                let pattern_id = self.type_definition_ids.get(name).cloned().ok_or_else(|| {
                    self.unsupported(format!(
                        "struct pattern '{}' has no HIR type identity",
                        name
                    ))
                })?;
                if expected_id != pattern_id {
                    return Err(self.unsupported(format!(
                        "struct pattern '{}' resolves to {}::{}, but value is {}::{}",
                        name,
                        pattern_id.module,
                        pattern_id.name,
                        expected_id.module,
                        expected_id.name
                    )));
                }
                let layout = self
                    .llvm_type_name_by_id(&expected_id)
                    .ok_or_else(|| {
                        self.unsupported(format!(
                            "LLVM struct layout missing for {}::{}",
                            expected_id.module, expected_id.name
                        ))
                    })?
                    .to_string();
                for (field_name, child_pattern) in fields {
                    let (field_id, field_type) = self
                        .struct_field_by_owner_and_name(&expected_id, field_name)
                        .ok_or_else(|| {
                            self.unsupported(format!(
                                "unknown HIR field '{}' on {}::{}",
                                field_name, expected_id.module, expected_id.name
                            ))
                        })?;
                    let index = self.struct_field_index_by_id(&field_id).ok_or_else(|| {
                        self.unsupported(format!("field '{}' has no LLVM index", field_name))
                    })?;
                    let address = self.temp();
                    writeln!(
                        out,
                        "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 {}",
                        address, layout, value, index
                    )
                    .unwrap();
                    let child_is_nominal = matches!(&field_type, Type::Struct(_) | Type::Enum(_));
                    let child_is_object = child_is_nominal
                        || matches!(&field_type, Type::Tuple(_) | Type::String | Type::Array(_));
                    let child_value = if child_is_object {
                        let loaded = self.temp();
                        writeln!(out, "  {} = load ptr, ptr {}", loaded, address).unwrap();
                        loaded
                    } else {
                        address
                    };
                    let child_success = self.block("pattern.struct.next");
                    self.emit_hir_pattern_test(
                        child_pattern,
                        &field_type,
                        &child_value,
                        child_is_object,
                        &child_success,
                        failure,
                        out,
                    )?;
                    writeln!(out, "{}:", child_success).unwrap();
                }
                writeln!(out, "  br label %{}", success).unwrap();
            }
            PatternKind::Number(expected) => {
                let native = self.native_type_for_fusion_type(value_type);
                if native != LlType::I64 {
                    return Err(self.unsupported("integer pattern applied to a non-integer value"));
                }
                let loaded = self.temp();
                writeln!(out, "  {} = load i64, ptr {}", loaded, value).unwrap();
                let compare = self.temp();
                writeln!(out, "  {} = icmp eq i64 {}, {}", compare, loaded, expected).unwrap();
                writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    compare, success, failure
                )
                .unwrap();
            }
            PatternKind::Float(expected) => {
                let loaded = self.temp();
                writeln!(out, "  {} = load double, ptr {}", loaded, value).unwrap();
                let compare = self.temp();
                writeln!(
                    out,
                    "  {} = fcmp oeq double {}, {:.17e}",
                    compare, loaded, expected
                )
                .unwrap();
                writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    compare, success, failure
                )
                .unwrap();
            }
            PatternKind::Boolean(expected) => {
                let loaded = self.temp();
                writeln!(out, "  {} = load i1, ptr {}", loaded, value).unwrap();
                let literal = if *expected { "true" } else { "false" };
                let compare = self.temp();
                writeln!(out, "  {} = icmp eq i1 {}, {}", compare, loaded, literal).unwrap();
                writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    compare, success, failure
                )
                .unwrap();
            }
            PatternKind::String(expected) => {
                if !matches!(value_type, Type::String | Type::Unknown) {
                    return Err(self.unsupported("string pattern applied to a non-string value"));
                }
                let literal = self.string_ptr(expected);
                let compare_result = self.temp();
                writeln!(
                    out,
                    "  {} = call i32 @strcmp(ptr {}, ptr {})",
                    compare_result, value, literal
                )
                .unwrap();
                let compare = self.temp();
                writeln!(out, "  {} = icmp eq i32 {}, 0", compare, compare_result).unwrap();
                writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    compare, success, failure
                )
                .unwrap();
            }
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                let loaded = self.temp();
                writeln!(out, "  {} = load i64, ptr {}", loaded, value).unwrap();
                let lower = self.temp();
                writeln!(out, "  {} = icmp sge i64 {}, {}", lower, loaded, start).unwrap();
                let upper = self.temp();
                let predicate = if *inclusive { "sle" } else { "slt" };
                writeln!(
                    out,
                    "  {} = icmp {} i64 {}, {}",
                    upper, predicate, loaded, end
                )
                .unwrap();
                let compare = self.temp();
                writeln!(out, "  {} = and i1 {}, {}", compare, lower, upper).unwrap();
                writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    compare, success, failure
                )
                .unwrap();
            }
            PatternKind::Tuple(patterns) => {
                let Type::Tuple(element_types) = value_type else {
                    return Err(self.unsupported("tuple pattern applied to a non-tuple value"));
                };
                if patterns.len() != element_types.len() {
                    return Err(self.unsupported(format!(
                        "tuple pattern has {} element(s), value has {}",
                        patterns.len(),
                        element_types.len()
                    )));
                }
                for (index, (child_pattern, child_type)) in
                    patterns.iter().zip(element_types.iter()).enumerate()
                {
                    let encoded_address = self.temp();
                    writeln!(
                        out,
                        "  {} = getelementptr inbounds i64, ptr {}, i64 {}",
                        encoded_address, value, index
                    )
                    .unwrap();
                    let encoded = self.temp();
                    writeln!(out, "  {} = load i64, ptr {}", encoded, encoded_address).unwrap();
                    let native = self.native_type_for_fusion_type(child_type);
                    let decoded = match native {
                        LlType::I64 => encoded.clone(),
                        LlType::I1 => {
                            let v = self.temp();
                            writeln!(out, "  {} = trunc i64 {} to i1", v, encoded).unwrap();
                            v
                        }
                        LlType::F64 => {
                            let v = self.temp();
                            writeln!(out, "  {} = bitcast i64 {} to double", v, encoded).unwrap();
                            v
                        }
                        LlType::Ptr => {
                            let v = self.temp();
                            writeln!(out, "  {} = inttoptr i64 {} to ptr", v, encoded).unwrap();
                            v
                        }
                        LlType::Void => {
                            return Err(
                                self.unsupported("void tuple elements cannot be pattern matched")
                            )
                        }
                    };
                    let child_is_object = matches!(
                        child_type,
                        Type::Struct(_)
                            | Type::Enum(_)
                            | Type::Tuple(_)
                            | Type::String
                            | Type::Array(_)
                    );
                    let child_value = if child_is_object {
                        decoded
                    } else {
                        let slot = self.temp();
                        writeln!(out, "  {} = alloca {}, align 8", slot, native.text()).unwrap();
                        writeln!(out, "  store {} {}, ptr {}", native.text(), decoded, slot)
                            .unwrap();
                        slot
                    };
                    let child_success = self.block("pattern.tuple.next");
                    self.emit_hir_pattern_test(
                        child_pattern,
                        child_type,
                        &child_value,
                        child_is_object,
                        &child_success,
                        failure,
                        out,
                    )?;
                    writeln!(out, "{}:", child_success).unwrap();
                }
                writeln!(out, "  br label %{}", success).unwrap();
            }
            PatternKind::Or(alternatives) => {
                if alternatives.is_empty() {
                    return Err(self.unsupported("empty or-pattern"));
                }
                let base_locals = self.locals.clone();
                let base_nominal = self.nominal_types_by_local.clone();
                let base_tuples = self.tuple_types.clone();
                let base_arrays = self.array_types.clone();
                let mut first_bindings = None;
                for (index, alternative) in alternatives.iter().enumerate() {
                    self.locals = base_locals.clone();
                    self.nominal_types_by_local = base_nominal.clone();
                    self.tuple_types = base_tuples.clone();
                    self.array_types = base_arrays.clone();
                    let alternative_success = self.block("pattern.or.success");
                    let alternative_failure = if index + 1 == alternatives.len() {
                        failure.to_string()
                    } else {
                        self.block("pattern.or.next")
                    };
                    self.emit_hir_pattern_test(
                        alternative,
                        value_type,
                        value,
                        is_object,
                        &alternative_success,
                        &alternative_failure,
                        out,
                    )?;
                    writeln!(out, "{}:", alternative_success).unwrap();
                    if first_bindings.is_none() {
                        first_bindings = Some((
                            self.locals.clone(),
                            self.nominal_types_by_local.clone(),
                            self.tuple_types.clone(),
                            self.array_types.clone(),
                        ));
                    }
                    writeln!(out, "  br label %{}", success).unwrap();
                    if index + 1 != alternatives.len() {
                        writeln!(out, "{}:", alternative_failure).unwrap();
                    }
                }
                if let Some((locals, nominal, tuples, arrays)) = first_bindings {
                    self.locals = locals;
                    self.nominal_types_by_local = nominal;
                    self.tuple_types = tuples;
                    self.array_types = arrays;
                }
            }
            _ => return Err(self.unsupported("unsupported pattern kind in LLVM match lowering")),
        }
        Ok(())
    }

    fn pattern_is_irrefutable(pattern: &Pattern) -> bool {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Identifier(_) => true,
            PatternKind::At { pattern, .. } => Self::pattern_is_irrefutable(pattern),
            PatternKind::Or(alternatives) => alternatives.iter().any(Self::pattern_is_irrefutable),
            PatternKind::Tuple(items) => items.iter().all(Self::pattern_is_irrefutable),
            PatternKind::Struct { fields, .. } => {
                fields.iter().all(|(_, p)| Self::pattern_is_irrefutable(p))
            }
            _ => false,
        }
    }

    fn match_is_exhaustive(&self, value_type: &Type, arms: &[MatchArm]) -> bool {
        if arms
            .iter()
            .any(|arm| arm.guard.is_none() && Self::pattern_is_irrefutable(&arm.pattern))
        {
            return true;
        }
        match value_type {
            Type::Bool => {
                let mut values = HashSet::new();
                for arm in arms.iter().filter(|arm| arm.guard.is_none()) {
                    Self::collect_bool_patterns(&arm.pattern, &mut values);
                }
                values.len() == 2
            }
            Type::Enum(symbol) => {
                let Some(owner) = self.type_definition_ids.get(symbol) else {
                    return false;
                };
                let mut expected_scope = owner.scope.clone();
                expected_scope.push(owner.name.clone());
                let all: HashSet<_> = self
                    .enum_variant_discriminants_by_id
                    .keys()
                    .filter(|id| id.module == owner.module && id.scope == expected_scope)
                    .cloned()
                    .collect();
                let mut covered = HashSet::new();
                for arm in arms.iter().filter(|arm| arm.guard.is_none()) {
                    self.collect_enum_variant_patterns(&arm.pattern, &mut covered);
                }
                !all.is_empty() && all.is_subset(&covered)
            }
            _ => false,
        }
    }

    fn collect_bool_patterns(pattern: &Pattern, values: &mut HashSet<bool>) {
        match &pattern.kind {
            PatternKind::Boolean(value) => {
                values.insert(*value);
            }
            PatternKind::At { pattern, .. } => Self::collect_bool_patterns(pattern, values),
            PatternKind::Or(items) => {
                for item in items {
                    Self::collect_bool_patterns(item, values);
                }
            }
            _ => {}
        }
    }

    fn collect_enum_variant_patterns(
        &self,
        pattern: &Pattern,
        covered: &mut HashSet<crate::hir::HirDefinitionId>,
    ) {
        match &pattern.kind {
            PatternKind::Variant { name, bindings }
                if bindings.iter().all(Self::pattern_is_irrefutable) =>
            {
                if let Some((owner, variant)) = name.rsplit_once("::") {
                    if let Some(id) = self
                        .enum_variant_definition_ids
                        .get(&(owner.to_string(), variant.to_string()))
                    {
                        covered.insert(id.clone());
                    }
                }
            }
            PatternKind::At { pattern, .. } => self.collect_enum_variant_patterns(pattern, covered),
            PatternKind::Or(items) => {
                for item in items {
                    self.collect_enum_variant_patterns(item, covered);
                }
            }
            _ => {}
        }
    }

    fn emit_pattern_binding(
        &mut self,
        name: &str,
        value_type: &Type,
        value: &str,
        is_object: bool,
        out: &mut String,
    ) -> Result<(), FusionError> {
        let native = if is_object {
            LlType::Ptr
        } else {
            self.native_type_for_fusion_type(value_type)
        };
        let stored_value = if is_object {
            value.to_string()
        } else {
            let loaded = self.temp();
            writeln!(out, "  {} = load {}, ptr {}", loaded, native.text(), value).unwrap();
            loaded
        };
        let slot = format!("%local_{}_{}", sanitize_symbol(name), self.next_temp);
        self.next_temp += 1;
        writeln!(out, "  {} = alloca {}, align 8", slot, native.text()).unwrap();
        writeln!(
            out,
            "  store {} {}, ptr {}",
            native.text(),
            stored_value,
            slot
        )
        .unwrap();
        self.locals.insert(name.to_string(), (native, slot));
        if let Type::Struct(type_name) | Type::Enum(type_name) = value_type {
            if let Some(identity) = self.nominal_identity_for_type_name(type_name) {
                self.nominal_types_by_local
                    .insert(name.to_string(), identity);
            }
        }
        if let Type::Tuple(items) = value_type {
            self.tuple_types.insert(name.to_string(), items.clone());
        }
        if let Type::Array(element) = value_type {
            self.array_types
                .insert(name.to_string(), (**element).clone());
        }
        Ok(())
    }

    fn emit_if(
        &mut self,
        condition: &Expression,
        body: &[Statement],
        else_body: Option<&[Statement]>,
        out: &mut String,
    ) -> Result<(), FusionError> {
        let c = self.emit_expr(condition, out)?;
        let then_b = self.block("then");
        let else_b = self.block("else");
        let end_b = self.block("endif");
        writeln!(out, "  br i1 {}, label %{}, label %{}", c, then_b, else_b).unwrap();
        writeln!(out, "{}:", then_b).unwrap();
        self.emit_statements(body, out)?;
        let then_terminates = Self::output_ends_in_terminator(out);
        if !then_terminates {
            writeln!(out, "  br label %{}", end_b).unwrap();
        }
        writeln!(out, "{}:", else_b).unwrap();
        if let Some(other) = else_body {
            self.emit_statements(other, out)?;
        }
        let else_terminates = Self::output_ends_in_terminator(out);
        if !else_terminates {
            writeln!(out, "  br label %{}", end_b).unwrap();
        }
        writeln!(out, "{}:", end_b).unwrap();
        if then_terminates && else_terminates {
            writeln!(out, "  unreachable").unwrap();
        }
        Ok(())
    }

    fn emit_while(
        &mut self,
        condition: &Expression,
        body: &[Statement],
        out: &mut String,
    ) -> Result<(), FusionError> {
        let cond_b = self.block("while.cond");
        let body_b = self.block("while.body");
        let end_b = self.block("while.end");
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", cond_b).unwrap();
        let c = self.emit_expr(condition, out)?;
        writeln!(out, "  br i1 {}, label %{}, label %{}", c, body_b, end_b).unwrap();
        writeln!(out, "{}:", body_b).unwrap();
        self.loop_targets.push(LoopTargets {
            break_label: end_b.clone(),
            continue_label: cond_b.clone(),
        });
        let body_result = self.emit_statements(body, out);
        self.loop_targets.pop();
        body_result?;
        if !Self::output_ends_in_terminator(out) {
            writeln!(out, "  br label %{}", cond_b).unwrap();
        }
        writeln!(out, "{}:", end_b).unwrap();
        Ok(())
    }

    fn emit_for(
        &mut self,
        variable: &str,
        start: &Expression,
        end: &Expression,
        body: &[Statement],
        out: &mut String,
    ) -> Result<(), FusionError> {
        let startv = self.emit_expr(start, out)?;
        let endv = self.emit_expr(end, out)?;
        let slot_id = self.next_temp;
        self.next_temp += 1;
        let slot = format!("%local_{}_{}", sanitize_symbol(variable), slot_id);
        writeln!(out, "  {} = alloca i64, align 8", slot).unwrap();
        writeln!(out, "  store i64 {}, ptr {}", startv, slot).unwrap();
        let previous_local = self
            .locals
            .insert(variable.to_string(), (LlType::I64, slot.clone()));
        let previous_nominal = self.nominal_types_by_local.remove(variable);
        let previous_tuple = self.tuple_types.remove(variable);
        let previous_array = self.array_types.remove(variable);
        let cond_b = self.block("for.cond");
        let body_b = self.block("for.body");
        let step_b = self.block("for.step");
        let end_b = self.block("for.end");
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", cond_b).unwrap();
        let cur = self.temp();
        writeln!(out, "  {} = load i64, ptr {}", cur, slot).unwrap();
        let cmp = self.temp();
        writeln!(out, "  {} = icmp slt i64 {}, {}", cmp, cur, endv).unwrap();
        writeln!(out, "  br i1 {}, label %{}, label %{}", cmp, body_b, end_b).unwrap();
        writeln!(out, "{}:", body_b).unwrap();
        self.loop_targets.push(LoopTargets {
            break_label: end_b.clone(),
            continue_label: step_b.clone(),
        });
        let body_result = self.emit_statements(body, out);
        self.loop_targets.pop();
        body_result?;
        if !Self::output_ends_in_terminator(out) {
            writeln!(out, "  br label %{}", step_b).unwrap();
        }
        writeln!(out, "{}:", step_b).unwrap();
        let cur2 = self.temp();
        writeln!(out, "  {} = load i64, ptr {}", cur2, slot).unwrap();
        let inc = self.temp();
        writeln!(out, "  {} = add i64 {}, 1", inc, cur2).unwrap();
        writeln!(out, "  store i64 {}, ptr {}", inc, slot).unwrap();
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", end_b).unwrap();
        self.locals.remove(variable);
        if let Some(previous) = previous_local {
            self.locals.insert(variable.to_string(), previous);
        }
        if let Some(previous) = previous_nominal {
            self.nominal_types_by_local
                .insert(variable.to_string(), previous);
        }
        if let Some(previous) = previous_tuple {
            self.tuple_types.insert(variable.to_string(), previous);
        }
        if let Some(previous) = previous_array {
            self.array_types.insert(variable.to_string(), previous);
        }
        Ok(())
    }

    fn emit_for_each(
        &mut self,
        variable: &str,
        iterable: &Expression,
        body: &[Statement],
        out: &mut String,
    ) -> Result<(), FusionError> {
        let element_type = match self.fusion_type_of_expr(iterable) {
            Type::Array(element) => *element,
            _ => {
                return Err(self.unsupported(
                    "native for-each currently requires an array with a known element type",
                ))
            }
        };
        let collection = self.emit_expr(iterable, out)?;
        let length = self.temp();
        writeln!(
            out,
            "  {} = call i64 @fusion_tuple_len(ptr {})",
            length, collection
        )
        .unwrap();
        let slot_id = self.next_temp;
        self.next_temp += 1;
        let index_slot = format!("%foreach_index_{}", slot_id);
        writeln!(out, "  {} = alloca i64, align 8", index_slot).unwrap();
        writeln!(out, "  store i64 0, ptr {}", index_slot).unwrap();
        let variable_slot = format!("%local_{}_{}", sanitize_symbol(variable), slot_id);
        let native_element_type = self.native_type_for_fusion_type(&element_type);
        writeln!(
            out,
            "  {} = alloca {}, align 8",
            variable_slot,
            native_element_type.text()
        )
        .unwrap();
        let cond_b = self.block("foreach.cond");
        let body_b = self.block("foreach.body");
        let step_b = self.block("foreach.step");
        let end_b = self.block("foreach.end");
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", cond_b).unwrap();
        let index = self.temp();
        writeln!(out, "  {} = load i64, ptr {}", index, index_slot).unwrap();
        let in_bounds = self.temp();
        writeln!(out, "  {} = icmp slt i64 {}, {}", in_bounds, index, length).unwrap();
        writeln!(
            out,
            "  br i1 {}, label %{}, label %{}",
            in_bounds, body_b, end_b
        )
        .unwrap();
        writeln!(out, "{}:", body_b).unwrap();
        let element_ptr = self.temp();
        writeln!(
            out,
            "  {} = getelementptr inbounds i64, ptr {}, i64 {}",
            element_ptr, collection, index
        )
        .unwrap();
        let encoded = self.temp();
        writeln!(out, "  {} = load i64, ptr {}", encoded, element_ptr).unwrap();
        let decoded = match native_element_type {
            LlType::I64 => encoded,
            LlType::I1 => {
                let v = self.temp();
                writeln!(out, "  {} = trunc i64 {} to i1", v, encoded).unwrap();
                v
            }
            LlType::F64 => {
                let v = self.temp();
                writeln!(out, "  {} = bitcast i64 {} to double", v, encoded).unwrap();
                v
            }
            LlType::Ptr => {
                let v = self.temp();
                writeln!(out, "  {} = inttoptr i64 {} to ptr", v, encoded).unwrap();
                v
            }
            LlType::Void => return Err(self.unsupported("cannot iterate over void values")),
        };
        writeln!(
            out,
            "  store {} {}, ptr {}",
            native_element_type.text(),
            decoded,
            variable_slot
        )
        .unwrap();
        let previous_local = self
            .locals
            .insert(variable.to_string(), (native_element_type, variable_slot));
        let previous_nominal = self.nominal_types_by_local.remove(variable);
        let previous_tuple = self.tuple_types.remove(variable);
        let previous_array = self.array_types.remove(variable);
        match &element_type {
            Type::Tuple(items) => {
                self.tuple_types.insert(variable.to_string(), items.clone());
            }
            Type::Array(inner) => {
                self.array_types
                    .insert(variable.to_string(), (**inner).clone());
            }
            _ => {}
        }
        self.loop_targets.push(LoopTargets {
            break_label: end_b.clone(),
            continue_label: step_b.clone(),
        });
        let body_result = self.emit_statements(body, out);
        self.loop_targets.pop();
        body_result?;
        if !Self::output_ends_in_terminator(out) {
            writeln!(out, "  br label %{}", step_b).unwrap();
        }
        writeln!(out, "{}:", step_b).unwrap();
        let current = self.temp();
        writeln!(out, "  {} = load i64, ptr {}", current, index_slot).unwrap();
        let next = self.temp();
        writeln!(out, "  {} = add i64 {}, 1", next, current).unwrap();
        writeln!(out, "  store i64 {}, ptr {}", next, index_slot).unwrap();
        writeln!(out, "  br label %{}", cond_b).unwrap();
        writeln!(out, "{}:", end_b).unwrap();
        self.locals.remove(variable);
        if let Some(previous) = previous_local {
            self.locals.insert(variable.to_string(), previous);
        }
        if let Some(previous) = previous_nominal {
            self.nominal_types_by_local
                .insert(variable.to_string(), previous);
        }
        if let Some(previous) = previous_tuple {
            self.tuple_types.insert(variable.to_string(), previous);
        }
        if let Some(previous) = previous_array {
            self.array_types.insert(variable.to_string(), previous);
        }
        Ok(())
    }

    fn emit_expr(&mut self, e: &Expression, out: &mut String) -> Result<String, FusionError> {
        match e {
            Expression::Number { value, .. } => Ok(value.to_string()),
            Expression::WideNumber { .. } => Err(self.unsupported(
                "integer literals above i64::MAX need exact-width native lowering (u64 above i64::MAX, i128, u128), which is not implemented yet; use the interpreter",
            )),
            Expression::Float { value, .. } => Ok(format!("{:.17e}", value)),
            Expression::Boolean { value, .. } => Ok(if *value {
                "true".into()
            } else {
                "false".into()
            }),
            Expression::String { value, .. } => Ok(self.string_ptr(value)),
            Expression::Tuple { elements, .. } | Expression::Array { elements, .. } => {
                // Tuples and arrays share tracked heap storage; their static types determine
                // whether indexing is positional or dynamic.
                let tuple_ptr = self.temp();
                writeln!(out, "  {} = call ptr @fusion_tuple_new(i64 {})", tuple_ptr, elements.len()).unwrap();
                for (index, element) in elements.iter().enumerate() {
                    let value = self.emit_expr(element, out)?;
                    let encoded = match self.expr_type(element) {
                        LlType::I64 => value,
                        LlType::I1 => {
                            let widened = self.temp();
                            writeln!(out, "  {} = zext i1 {} to i64", widened, value).unwrap();
                            widened
                        }
                        LlType::F64 => {
                            let bits = self.temp();
                            writeln!(out, "  {} = bitcast double {} to i64", bits, value).unwrap();
                            bits
                        }
                        LlType::Ptr => {
                            let bits = self.temp();
                            writeln!(out, "  {} = ptrtoint ptr {} to i64", bits, value).unwrap();
                            bits
                        }
                        LlType::Void => return Err(self.unsupported("void value cannot be stored in a tuple")),
                    };
                    let slot_ptr = self.temp();
                    writeln!(out, "  {} = getelementptr inbounds i64, ptr {}, i64 {}", slot_ptr, tuple_ptr, index).unwrap();
                    writeln!(out, "  store i64 {}, ptr {}, align 8", encoded, slot_ptr).unwrap();
                }
                Ok(tuple_ptr)
            }
            Expression::StructConstructor { name, fields, .. } => {
                let identity = self.nominal_identity_for_type_name(name).ok_or_else(|| {
                    self.unsupported(format!("struct constructor '{}' has no HIR declaration identity", name))
                })?;
                let llvm_name = self.llvm_type_name_by_id(&identity).ok_or_else(|| {
                    self.unsupported(format!("LLVM layout missing for struct {}::{}", identity.module, identity.name))
                })?.to_string();
                let layout = self.struct_layouts_by_id.get(&identity).cloned().ok_or_else(|| {
                    self.unsupported(format!("struct layout missing for {}::{}", identity.module, identity.name))
                })?;
                if fields.len() != layout.fields.len() {
                    return Err(self.unsupported(format!("struct constructor '{}' supplies {} fields but the declaration requires {}", name, fields.len(), layout.fields.len())));
                }
                let value_ptr = self.emit_heap_allocation_for_named_type(&llvm_name, out);
                let mut initialized_fields = HashSet::new();
                for (field_name, field_value) in fields {
                    if !initialized_fields.insert(field_name.as_str()) {
                        return Err(self.unsupported(format!("struct constructor '{}' initializes field '{}' more than once", name, field_name)));
                    }
                    let (field_id, _) = self.struct_field_by_owner_and_name(&identity, field_name)
                        .ok_or_else(|| self.unsupported(format!("unknown field '{}' on {}::{}", field_name, identity.module, identity.name)))?;
                    let index = self.struct_field_index_by_id(&field_id).ok_or_else(|| self.unsupported("HIR field identity has no LLVM index"))?;
                    let field_type = layout.fields.get(index).map(|(_, ty)| ty.clone()).ok_or_else(|| self.unsupported("HIR field index is outside struct layout"))?;
                    let emitted = self.emit_expr(field_value, out)?;
                    let native = self.native_type_for_fusion_type(&field_type);
                    let address = self.temp();
                    writeln!(out, "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 {}", address, llvm_name, value_ptr, index).unwrap();
                    writeln!(out, "  store {} {}, ptr {}", native.text(), emitted, address).unwrap();
                }
                Ok(value_ptr)
            }
            Expression::EnumConstructor { enum_name, variant, arguments, .. } => {
                let enum_identity = self.nominal_identity_for_type_name(enum_name).ok_or_else(|| {
                    self.unsupported(format!("enum constructor '{}' has no HIR declaration identity", enum_name))
                })?;
                let enum_llvm_name = self.llvm_type_name_by_id(&enum_identity).ok_or_else(|| self.unsupported("LLVM enum layout is missing"))?.to_string();
                let variant_identity = self.enum_variant_definition_ids.get(&(enum_name.clone(), variant.clone())).cloned()
                    .ok_or_else(|| self.unsupported(format!("enum variant '{}::{}' has no HIR identity", enum_name, variant)))?;
                let discriminant = self.enum_variant_discriminant_by_id(&variant_identity).ok_or_else(|| self.unsupported("enum variant has no discriminant"))?;
                let payload_types = self.enum_payloads_by_id.get(&variant_identity).cloned()
                    .ok_or_else(|| self.unsupported(format!("enum payload layout missing for '{}::{}'", enum_name, variant)))?;
                if payload_types.len() != arguments.len() {
                    return Err(self.unsupported(format!("enum variant '{}::{}' expects {} payload values", enum_name, variant, payload_types.len())));
                }
                let payload_ptr = if arguments.is_empty() {
                    "null".to_string()
                } else {
                    let payload_name = self.enum_variant_layout_names_by_id.get(&variant_identity).cloned().ok_or_else(|| self.unsupported("enum payload LLVM layout is missing"))?;
                    let ptr = self.emit_heap_allocation_for_named_type(&payload_name, out);
                    for (index, (argument, payload_type)) in arguments.iter().zip(payload_types.iter()).enumerate() {
                        let emitted = self.emit_expr(argument, out)?;
                        let native = self.native_type_for_fusion_type(payload_type);
                        let address = self.temp();
                        writeln!(out, "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 {}", address, payload_name, ptr, index).unwrap();
                        writeln!(out, "  store {} {}, ptr {}", native.text(), emitted, address).unwrap();
                    }
                    ptr
                };
                let enum_ptr = self.emit_heap_allocation_for_named_type(&enum_llvm_name, out);
                let tag_ptr = self.temp();
                writeln!(out, "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 0", tag_ptr, enum_llvm_name, enum_ptr).unwrap();
                writeln!(out, "  store i32 {}, ptr {}", discriminant, tag_ptr).unwrap();
                let data_ptr = self.temp();
                writeln!(out, "  {} = getelementptr inbounds {}, ptr {}, i32 0, i32 1", data_ptr, enum_llvm_name, enum_ptr).unwrap();
                writeln!(out, "  store ptr {}, ptr {}", payload_ptr, data_ptr).unwrap();
                Ok(enum_ptr)
            }
            Expression::Property { object, name, .. } => {
                let (address, native_type, _) = self.emit_struct_field_address(object, name, out)?;
                let value = self.temp();
                writeln!(out, "  {} = load {}, ptr {}", value, native_type.text(), address).unwrap();
                Ok(value)
            }
            Expression::Index { array, index, .. } => {
                let is_array = matches!(array.as_ref(), Expression::Array { .. })
                    || matches!(self.fusion_type_of_expr(array), Type::Array(_));
                let element_type = match index.as_ref() {
                    Expression::Number { value, .. } if *value >= 0 => self.tuple_element_type(array, *value as usize),
                    _ if is_array => match self.fusion_type_of_expr(array) { Type::Array(element) => Some(*element), _ => None },
                    _ => None,
                }.ok_or_else(|| self.unsupported("tuple index is out of bounds or array element type is unknown"))?;
                let native_element_type = self.native_type_for_fusion_type(&element_type);
                let collection = self.emit_expr(array, out)?;
                let index_value = match index.as_ref() {
                    Expression::Number { value, .. } if *value >= 0 => value.to_string(),
                    _ if is_array => self.emit_expr(index, out)?,
                    _ => return Err(self.unsupported("native tuple indexing requires a literal index; arrays accept dynamic integer indexes")),
                };
                if is_array {
                    let length = self.temp();
                    writeln!(out, "  {} = call i64 @fusion_tuple_len(ptr {})", length, collection).unwrap();
                    let valid = self.temp();
                    writeln!(out, "  {} = icmp ult i64 {}, {}", valid, index_value, length).unwrap();
                    let good = self.block("index.valid");
                    let bad = self.block("index.oob");
                    writeln!(out, "  br i1 {}, label %{}, label %{}", valid, good, bad).unwrap();
                    writeln!(out, "{}:", bad).unwrap();
                    writeln!(out, "  call void @llvm.trap()").unwrap();
                    writeln!(out, "  unreachable").unwrap();
                    writeln!(out, "{}:", good).unwrap();
                }
                let slot = self.temp();
                writeln!(out, "  {} = getelementptr inbounds i64, ptr {}, i64 {}", slot, collection, index_value).unwrap();
                let encoded = self.temp();
                writeln!(out, "  {} = load i64, ptr {}, align 8", encoded, slot).unwrap();
                match native_element_type {
                    LlType::I64 => Ok(encoded),
                    LlType::I1 => { let value = self.temp(); writeln!(out, "  {} = trunc i64 {} to i1", value, encoded).unwrap(); Ok(value) }
                    LlType::F64 => { let value = self.temp(); writeln!(out, "  {} = bitcast i64 {} to double", value, encoded).unwrap(); Ok(value) }
                    LlType::Ptr => { let value = self.temp(); writeln!(out, "  {} = inttoptr i64 {} to ptr", value, encoded).unwrap(); Ok(value) }
                    LlType::Void => Err(self.unsupported("cannot index a collection element of type void")),
                }
            }
            Expression::Identifier { name, .. } => {
                if let Some((ty, symbol)) = self.global_constants.get(name).cloned() {
                    let value = self.temp();
                    writeln!(out, "  {} = load {}, ptr @{}", value, ty.text(), symbol).unwrap();
                    return Ok(value);
                }
                if self.unsupported_global_constants.contains(name) {
                    return Err(self.unsupported(format!("package constant '{}' has a non-literal initializer and cannot yet be emitted as a cross-package global", name)));
                }
                let (ty, slot) = self.locals.get(name).cloned()
                    .ok_or_else(|| self.unsupported(format!("unknown local '{}'", name)))?;
                let t = self.temp();
                writeln!(out, "  {} = load {}, ptr {}", t, ty.text(), slot).unwrap();
                Ok(t)
            }
            Expression::Binary {
                left,
                operator,
                right,
                ..
            } => {
                let l = self.emit_expr(left, out)?;
                let r = self.emit_expr(right, out)?;
                let ty = self.expr_type(left);
                let t = self.temp();
                match (operator, ty) {
                    (Operator::Plus, LlType::I64) => {
                        writeln!(out, "  {} = add i64 {}, {}", t, l, r)
                    }
                    (Operator::Minus, LlType::I64) => {
                        writeln!(out, "  {} = sub i64 {}, {}", t, l, r)
                    }
                    (Operator::Multiply, LlType::I64) => {
                        writeln!(out, "  {} = mul i64 {}, {}", t, l, r)
                    }
                    (Operator::Divide, LlType::I64) => {
                        writeln!(out, "  {} = sdiv i64 {}, {}", t, l, r)
                    }
                    (Operator::Plus, LlType::F64) => {
                        writeln!(out, "  {} = fadd double {}, {}", t, l, r)
                    }
                    (Operator::Minus, LlType::F64) => {
                        writeln!(out, "  {} = fsub double {}, {}", t, l, r)
                    }
                    (Operator::Multiply, LlType::F64) => {
                        writeln!(out, "  {} = fmul double {}, {}", t, l, r)
                    }
                    (Operator::Divide, LlType::F64) => {
                        writeln!(out, "  {} = fdiv double {}, {}", t, l, r)
                    }
                    (Operator::Equal, LlType::I64) => {
                        writeln!(out, "  {} = icmp eq i64 {}, {}", t, l, r)
                    }
                    (Operator::NotEqual, LlType::I64) => {
                        writeln!(out, "  {} = icmp ne i64 {}, {}", t, l, r)
                    }
                    (Operator::Less, LlType::I64) => {
                        writeln!(out, "  {} = icmp slt i64 {}, {}", t, l, r)
                    }
                    (Operator::LessEqual, LlType::I64) => {
                        writeln!(out, "  {} = icmp sle i64 {}, {}", t, l, r)
                    }
                    (Operator::Greater, LlType::I64) => {
                        writeln!(out, "  {} = icmp sgt i64 {}, {}", t, l, r)
                    }
                    (Operator::GreaterEqual, LlType::I64) => {
                        writeln!(out, "  {} = icmp sge i64 {}, {}", t, l, r)
                    }
                    (Operator::And, LlType::I1) => writeln!(out, "  {} = and i1 {}, {}", t, l, r),
                    (Operator::Or, LlType::I1) => writeln!(out, "  {} = or i1 {}, {}", t, l, r),
                    (Operator::Equal, LlType::F64) => {
                        writeln!(out, "  {} = fcmp oeq double {}, {}", t, l, r)
                    }
                    (Operator::NotEqual, LlType::F64) => {
                        writeln!(out, "  {} = fcmp one double {}, {}", t, l, r)
                    }
                    (Operator::Less, LlType::F64) => {
                        writeln!(out, "  {} = fcmp olt double {}, {}", t, l, r)
                    }
                    (Operator::LessEqual, LlType::F64) => {
                        writeln!(out, "  {} = fcmp ole double {}, {}", t, l, r)
                    }
                    (Operator::Greater, LlType::F64) => {
                        writeln!(out, "  {} = fcmp ogt double {}, {}", t, l, r)
                    }
                    (Operator::GreaterEqual, LlType::F64) => {
                        writeln!(out, "  {} = fcmp oge double {}, {}", t, l, r)
                    }
                    _ => return Err(self.unsupported("invalid LLVM binary operation")),
                }
                .map_err(|e| self.unsupported(e.to_string()))?;
                Ok(t)
            }
            Expression::Conversion { .. } => {
                return Err(
                    self.unsupported("type conversions are not yet lowered by the LLVM backend")
                );
            }
            Expression::Unary {
                operator,
                expression,
                ..
            } => {
                let x = self.emit_expr(expression, out)?;
                let ty = self.expr_type(expression);
                let t = self.temp();
                match (operator, ty) {
                    (UnaryOperator::Negate, LlType::I64) => {
                        writeln!(out, "  {} = sub i64 0, {}", t, x)
                    }
                    (UnaryOperator::Negate, LlType::F64) => {
                        writeln!(out, "  {} = fneg double {}", t, x)
                    }
                    (UnaryOperator::Not, LlType::I1) => {
                        writeln!(out, "  {} = xor i1 {}, true", t, x)
                    }
                    _ => return Err(self.unsupported("invalid unary operation")),
                }
                .map_err(|e| self.unsupported(e.to_string()))?;
                Ok(t)
            }
            Expression::MethodCall { object, arguments, span, .. } => {
                let target_symbol = self.method_call_targets.get(&(self.current_module.clone().unwrap_or_default(), span.start, span.end)).cloned()
                    .ok_or_else(|| self.unsupported("method call has no resolved HIR target; compile through emit_project"))?;
                let sig = self.functions.get(&target_symbol).cloned().ok_or_else(||
                    self.unsupported(format!("resolved method target '{}' has no emitted ABI", target_symbol)))?;
                if sig.params.len() != arguments.len() + 1 {
                    return Err(self.unsupported(format!("method '{}' ABI expects {} total parameters (receiver plus arguments), got {}", target_symbol, sig.params.len(), arguments.len() + 1)));
                }
                let receiver = self.emit_expr(object, out)?;
                let mut args = vec![format!("{} {}", sig.params[0].text(), receiver)];
                for (index, argument) in arguments.iter().enumerate() {
                    let value = self.emit_expr(argument, out)?;
                    args.push(format!("{} {}", sig.params[index + 1].text(), value));
                }
                if sig.ret == LlType::Void {
                    writeln!(out, "  call void @{}({})", sanitize_symbol(&target_symbol), args.join(", ")).unwrap();
                    Ok("0".into())
                } else {
                    let result = self.temp();
                    writeln!(out, "  {} = call {} @{}({})", result, sig.ret.text(), sanitize_symbol(&target_symbol), args.join(", ")).unwrap();
                    Ok(result)
                }
            }
            Expression::Call {
                name, arguments, span, ..
            } => {
                if self.foreign_functions.contains_key(name) {
                    self.emit_foreign_call(name, arguments, out)
                } else if name == "print" {
                    if arguments.len() != 1 {
                        return Err(self.unsupported("print currently takes exactly one argument"));
                    }
                    let e = &arguments[0];
                    let v = self.emit_expr(e, out);
                    let ty = self.expr_type(e);
                    match ty {
                        LlType::I64 => {
                            let fmt = self.string_ptr("%ld\n");
                            let f = self.temp();
                            writeln!(
                                out,
                                "  {} = call i32 (ptr, ...) @printf(ptr {}, i64 {})",
                                f,
                                fmt,
                                v.unwrap()
                            )
                            .unwrap();
                        }
                        LlType::F64 => {
                            let fmt = self.string_ptr("%f\n");
                            let f = self.temp();
                            writeln!(
                                out,
                                "  {} = call i32 (ptr, ...) @printf(ptr {}, double {})",
                                f,
                                fmt,
                                v.unwrap()
                            )
                            .unwrap();
                        }
                        LlType::I1 => {
                            let truep = self.string_ptr("true\n");
                            let falsep = self.string_ptr("false\n");
                            let sel = self.temp();
                            writeln!(
                                out,
                                "  {} = select i1 {}, ptr {}, ptr {}",
                                sel,
                                v.unwrap(),
                                truep,
                                falsep
                            )
                            .unwrap();
                            let f = self.temp();
                            writeln!(out, "  {} = call i32 @puts(ptr {})", f, sel).unwrap();
                        }
                        LlType::Ptr => {
                            let f = self.temp();
                            writeln!(out, "  {} = call i32 @puts(ptr {})", f, v.unwrap()).unwrap();
                        }
                        LlType::Void => return Err(self.unsupported("cannot print void")),
                    }
                    Ok("0".into())
                } else {
                    // Project calls are selected by their resolved HIR call-site
                    // identity. The compatibility-name lookup remains only for
                    // standalone programs and unresolved legacy intrinsics.
                    let target_symbol = self.function_call_targets
                        .get(&(self.current_module.clone().unwrap_or_default(), span.start, span.end)).cloned()
                        .unwrap_or_else(|| self.resolved_backend_function_symbol(name));
                    let sig = self.functions.get(&target_symbol).or_else(|| self.functions.get(name)).cloned().ok_or_else(|| {
                        self.unsupported(format!("unknown function '{}' (resolved target '{}')", name, target_symbol))
                    })?;
                    if sig.params.len() != arguments.len() {
                        return Err(self.unsupported(format!(
                            "function '{}' expects {} arguments",
                            name,
                            sig.params.len()
                        )));
                    }
                    let mut args = Vec::new();
                    for (i, a) in arguments.iter().enumerate() {
                        let v = self.emit_expr(a, out)?;
                        args.push(format!("{} {}", sig.params[i].text(), v));
                    }
                    if sig.ret == LlType::Void {
                        writeln!(
                            out,
                            "  call void @{}({})",
                            sanitize_symbol(&target_symbol),
                            args.join(", ")
                        )
                        .unwrap();
                        Ok("0".into())
                    } else {
                        let t = self.temp();
                        writeln!(
                            out,
                            "  {} = call {} @{}({})",
                            t,
                            sig.ret.text(),
                            sanitize_symbol(&target_symbol),
                            args.join(", ")
                        )
                        .unwrap();
                        Ok(t)
                    }
                }
            }
            _ => Err(self.unsupported("expression is not yet lowered by the LLVM backend")),
        }
    }

    fn tuple_element_type(&self, tuple: &Expression, index: usize) -> Option<Type> {
        match tuple {
            Expression::Tuple { elements, .. } => {
                elements.get(index).map(|e| self.fusion_type_of_expr(e))
            }
            Expression::Array { elements, .. } => {
                elements.first().map(|e| self.fusion_type_of_expr(e))
            }
            Expression::Identifier { name, .. } => self
                .tuple_types
                .get(name)
                .and_then(|items| items.get(index))
                .cloned()
                .or_else(|| self.array_types.get(name).cloned()),
            Expression::Index {
                array,
                index: inner_index,
                ..
            } => {
                let Expression::Number { value, .. } = inner_index.as_ref() else {
                    return None;
                };
                if *value < 0 {
                    return None;
                }
                match self.tuple_element_type(array, *value as usize)? {
                    Type::Tuple(items) => items.get(index).cloned(),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn fusion_type_of_expr(&self, e: &Expression) -> Type {
        match e {
            Expression::Tuple { elements, .. } => Type::Tuple(
                elements
                    .iter()
                    .map(|e| self.fusion_type_of_expr(e))
                    .collect(),
            ),
            Expression::Array { elements, .. } => Type::Array(Box::new(
                elements
                    .first()
                    .map(|e| self.fusion_type_of_expr(e))
                    .unwrap_or(Type::Unknown),
            )),
            Expression::Number { .. } => Type::Num,
            Expression::Float { .. } => Type::Float,
            Expression::Boolean { .. } => Type::Bool,
            Expression::String { .. } => Type::String,
            Expression::Identifier { name, .. } => {
                if let Some(items) = self.tuple_types.get(name) {
                    return Type::Tuple(items.clone());
                }
                if let Some(element) = self.array_types.get(name) {
                    return Type::Array(Box::new(element.clone()));
                }
                match self.locals.get(name).map(|entry| entry.0) {
                    Some(LlType::I64) => Type::Num,
                    Some(LlType::I1) => Type::Bool,
                    Some(LlType::F64) => Type::Float,
                    Some(LlType::Ptr) => Type::String,
                    _ => Type::Unknown,
                }
            }
            Expression::Index { array, index, .. } => {
                if let Expression::Number { value, .. } = index.as_ref() {
                    if *value >= 0 {
                        if let Some(ty) = self.tuple_element_type(array, *value as usize) {
                            return ty;
                        }
                    }
                }
                Type::Unknown
            }
            _ => Type::Unknown,
        }
    }

    fn native_type_for_fusion_type(&self, ty: &Type) -> LlType {
        match ty {
            Type::Num => LlType::I64,
            Type::Float => LlType::F64,
            Type::Generic(name) if matches!(name.as_str(), "f32" | "f64") => LlType::F64,
            Type::Generic(name) if name == "*opaque" => LlType::Ptr,
            Type::Generic(_) => LlType::I64,
            Type::Bool => LlType::I1,
            Type::String | Type::Tuple(_) | Type::Array(_) | Type::Struct(_) | Type::Enum(_) => {
                LlType::Ptr
            }
            Type::Void => LlType::Void,
            _ => LlType::I64,
        }
    }

    fn expr_type(&self, e: &Expression) -> LlType {
        match e {
            Expression::Number { .. } => LlType::I64,
            Expression::Float { .. } => LlType::F64,
            Expression::Boolean { .. } => LlType::I1,
            Expression::String { .. } => LlType::Ptr,
            Expression::Binary { left, operator, .. } => match operator {
                Operator::Equal
                | Operator::NotEqual
                | Operator::Less
                | Operator::LessEqual
                | Operator::Greater
                | Operator::GreaterEqual
                | Operator::And
                | Operator::Or => LlType::I1,
                _ => self.expr_type(left),
            },
            Expression::Conversion { target_type, .. } => self.llvm_type_name(target_type),
            Expression::Unary {
                operator,
                expression,
                ..
            } => match operator {
                UnaryOperator::Not => LlType::I1,
                _ => self.expr_type(expression),
            },
            Expression::Identifier { name, .. } => self
                .locals
                .get(name)
                .map(|x| x.0)
                .or_else(|| self.global_constants.get(name).map(|entry| entry.0))
                .unwrap_or(LlType::I64),
            Expression::Index { array, index, .. } => {
                if let Expression::Number { value, .. } = index.as_ref() {
                    if *value >= 0 {
                        if let Some(ty) = self.tuple_element_type(array, *value as usize) {
                            return self.native_type_for_fusion_type(&ty);
                        }
                    }
                }
                if matches!(self.fusion_type_of_expr(array), Type::Array(_)) {
                    if let Type::Array(element) = self.fusion_type_of_expr(array) {
                        return self.native_type_for_fusion_type(&element);
                    }
                }
                LlType::I64
            }
            Expression::Tuple { .. }
            | Expression::Array { .. }
            | Expression::StructConstructor { .. }
            | Expression::EnumConstructor { .. } => LlType::Ptr,
            Expression::Property { object, name, .. } => self
                .nominal_identity_for_expression(object)
                .and_then(|owner| self.struct_field_by_owner_and_name(&owner, name))
                .map(|(_, field_type)| self.native_type_for_fusion_type(&field_type))
                .unwrap_or(LlType::I64),
            Expression::Call { name, .. } => self
                .functions
                .get(name)
                .map(|s| s.ret)
                .unwrap_or(LlType::I64),
            Expression::MethodCall { span, .. } => self
                .method_call_targets
                .get(&(
                    self.current_module.clone().unwrap_or_default(),
                    span.start,
                    span.end,
                ))
                .and_then(|symbol| self.functions.get(symbol))
                .map(|signature| signature.ret)
                .unwrap_or(LlType::I64),
            _ => LlType::I64,
        }
    }

    fn zero_for(&self, t: LlType) -> String {
        match t {
            LlType::I64 => "0".into(),
            LlType::I1 => "false".into(),
            LlType::F64 => "0.0".into(),
            LlType::Ptr => "null".into(),
            LlType::Void => "0".into(),
        }
    }
}

fn hir_method_backend_symbol(identity: &crate::hir::HirDefinitionId) -> String {
    fn encode(component: &str) -> String {
        let hex = component
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join("");
        format!("{}x{}", component.len(), hex)
    }
    let scopes = identity
        .scope
        .iter()
        .map(|scope| encode(scope))
        .collect::<Vec<_>>()
        .join("_");
    format!(
        "__fusion_method_m{}_c{}_{}_n{}",
        encode(&identity.module),
        identity.scope.len(),
        scopes,
        encode(&identity.name)
    )
}

fn hir_scalar_llvm_type(ty: &crate::hir::HirType) -> Option<LlType> {
    match ty {
        crate::hir::HirType::Num => Some(LlType::I64),
        crate::hir::HirType::Bool => Some(LlType::I1),
        crate::hir::HirType::Float => Some(LlType::F64),
        crate::hir::HirType::String
        | crate::hir::HirType::Nominal(_)
        | crate::hir::HirType::Array(_)
        | crate::hir::HirType::Tuple(_) => Some(LlType::Ptr),
        crate::hir::HirType::Void => Some(LlType::Void),
        _ => None,
    }
}

fn hir_expr_scalar_type(
    expression: &crate::hir::HirExpression,
    path: &[u32],
    refs: &[crate::hir::HirLocalReference],
    calls: &[crate::hir::HirCallReference],
    slots: &HashMap<crate::hir::HirLocalId, (LlType, String)>,
) -> Option<LlType> {
    match expression {
        crate::hir::HirExpression::Number(_) => Some(LlType::I64),
        crate::hir::HirExpression::Boolean(_) => Some(LlType::I1),
        crate::hir::HirExpression::Float(_) => Some(LlType::F64),
        crate::hir::HirExpression::String(_)
        | crate::hir::HirExpression::Array(_)
        | crate::hir::HirExpression::Tuple(_) => Some(LlType::Ptr),
        crate::hir::HirExpression::Identifier(_) => refs
            .iter()
            .find(|r| r.path == path)
            .and_then(|r| slots.get(&r.local).map(|(ty, _)| *ty)),
        crate::hir::HirExpression::Call { .. } | crate::hir::HirExpression::MethodCall { .. } => {
            calls
                .iter()
                .find(|call| call.path == path)
                .and_then(|call| hir_scalar_llvm_type(&call.call.return_hir_type))
        }
        _ => None,
    }
}

fn sanitize_symbol(s: &str) -> String {
    let mut out = String::from("_F");
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

fn llvm_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'\\' => out.push_str("\\5C"),
            b'"' => out.push_str("\\22"),
            b'\n' => out.push_str("\\0A"),
            b'\r' => out.push_str("\\0D"),
            b'\t' => out.push_str("\\09"),
            0x20..=0x7e => out.push(*b as char),
            b => write!(&mut out, "\\{:02X}", b).unwrap(),
        }
    }
    out
}

#[cfg(test)]
mod hir_layout_tests {
    use super::*;
    use crate::hir::{HirDefinitionId, HirDefinitionKind};

    #[test]
    fn emits_c_abi_declaration_and_width_converting_call() {
        let source = "extern \"C\" {\n    fn fusion_helper(value: i32) -> i32;\n}\nmain:\n    print(fusion_helper(21))\n";
        let mut lexer = crate::lexer::Lexer::new(source, crate::lexer::BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("source should lex");
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse().expect("source should parse");
        let ir = LLVMBackend::new().emit_program(&program).expect("foreign call should lower");
        assert!(ir.contains("declare ccc i32 @fusion_helper(i32)"));
        assert!(ir.contains("trunc i64 21 to i32"));
        assert!(ir.contains("call ccc i32 @fusion_helper(i32"));
        assert!(ir.contains("sext i32"));
    }

    #[test]
    fn emits_c_abi_float_pointer_and_void_signatures() {
        let source = "extern \"C\" {\n    fn consume(value: f32, handle: *opaque);\n    fn get_handle() -> *opaque;\n    fn get_unsigned() -> u32;\n    fn get_float() -> f32;\n}\nmain:\n    consume(1.5, get_handle())\n    print(get_unsigned())\n    print(get_float())\n";
        let mut lexer = crate::lexer::Lexer::new(source, crate::lexer::BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("source should lex");
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse().expect("source should parse");
        let ir = LLVMBackend::new().emit_program(&program).expect("foreign calls should lower");
        assert!(ir.contains("declare ccc void @consume(float, ptr)"));
        assert!(ir.contains("declare ccc ptr @get_handle()"));
        assert!(ir.contains("declare ccc i32 @get_unsigned()"));
        assert!(ir.contains("declare ccc float @get_float()"));
        assert!(ir.contains("fptrunc double 1.50000000000000000e0 to float"));
        assert!(ir.contains("call ccc void @consume(float"));
        assert!(ir.contains("zext i32"));
        assert!(ir.contains("fpext float"));
    }

    #[test]
    fn llvm_emits_named_struct_and_enum_layouts_from_hir_ids() {
        let mut backend = LLVMBackend::new();
        let point = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let color = HirDefinitionId::new("graphics", "Color", HirDefinitionKind::Enum);
        let red = HirDefinitionId::scoped(
            "graphics",
            ["Color".into()],
            "Red",
            HirDefinitionKind::Variant,
        );
        let x =
            HirDefinitionId::scoped("geometry", ["Point".into()], "x", HirDefinitionKind::Field);
        backend
            .llvm_type_names_by_id
            .insert(point.clone(), "%fusion.struct.0".into());
        backend
            .llvm_type_names_by_id
            .insert(color.clone(), "%fusion.enum.0".into());
        backend.struct_layouts_by_id.insert(
            point,
            StructDefinition {
                fields: vec![("x".into(), Type::Num), ("label".into(), Type::String)],
            },
        );
        backend
            .enum_payloads_by_id
            .insert(red.clone(), vec![Type::Num, Type::Bool]);
        backend
            .enum_variant_layout_names_by_id
            .insert(red, "%fusion.variant.0".into());
        backend.struct_field_indices_by_id.insert(x.clone(), 0);
        backend
            .struct_field_definition_ids
            .insert(("geometry_Point".into(), "x".into()), x);
        backend.enum_variant_discriminants_by_id.insert(
            HirDefinitionId::scoped(
                "graphics",
                ["Color".into()],
                "Red",
                HirDefinitionKind::Variant,
            ),
            0,
        );
        backend.type_definition_ids.insert(
            "geometry_Point".into(),
            HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct),
        );
        backend.enum_variant_definition_ids.insert(
            ("graphics_Color".into(), "Red".into()),
            HirDefinitionId::scoped(
                "graphics",
                ["Color".into()],
                "Red",
                HirDefinitionKind::Variant,
            ),
        );

        backend.emit_preamble();
        assert!(backend
            .module
            .contains("%fusion.struct.0 = type { i64, ptr }"));
        assert!(backend
            .module
            .contains("%fusion.enum.0 = type { i32, ptr }"));
        assert!(backend
            .module
            .contains("%fusion.variant.0 = type { i64, i1 }"));
        assert!(backend
            .module
            .contains("fusion.field-index geometry::Point::x = 0"));
        assert!(backend
            .module
            .contains("fusion.variant-discriminant graphics::Color::Red = 0"));
        assert_eq!(
            backend.llvm_type_name_for_symbol("geometry_Point"),
            Some("%fusion.struct.0")
        );
        assert_eq!(
            backend.struct_field_index_for_symbol("geometry_Point", "x"),
            Some(0)
        );
        assert_eq!(
            backend.enum_variant_discriminant_for_symbol("graphics_Color", "Red"),
            Some(0)
        );
    }

    #[test]
    fn struct_constructor_and_field_read_emit_hir_keyed_gep_operations() {
        let mut backend = LLVMBackend::new();
        let point = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let x =
            HirDefinitionId::scoped("geometry", ["Point".into()], "x", HirDefinitionKind::Field);
        backend
            .type_definition_ids
            .insert("geometry_Point".into(), point.clone());
        backend
            .llvm_type_names_by_id
            .insert(point.clone(), "%fusion.struct.0".into());
        backend.struct_layouts_by_id.insert(
            point.clone(),
            StructDefinition {
                fields: vec![("x".into(), Type::Num)],
            },
        );
        backend
            .struct_field_definition_ids
            .insert(("geometry_Point".into(), "x".into()), x.clone());
        backend.struct_field_indices_by_id.insert(x, 0);

        let constructor = Expression::StructConstructor {
            name: "geometry_Point".into(),
            fields: vec![(
                "x".into(),
                Expression::Number {
                    value: 7,
                    span: crate::span::Span::new(0, 0),
                },
            )],
            span: crate::span::Span::new(0, 0),
        };
        let mut constructor_ir = String::new();
        let value = backend
            .emit_expr(&constructor, &mut constructor_ir)
            .unwrap();
        assert!(constructor_ir.contains("call ptr @fusion_heap_alloc"));
        assert!(constructor_ir.contains("getelementptr inbounds %fusion.struct.0"));
        assert!(constructor_ir.contains("store i64 7"));

        backend
            .locals
            .insert("point".into(), (LlType::Ptr, "%point.slot".into()));
        backend.nominal_types_by_local.insert("point".into(), point);
        let property = Expression::Property {
            object: Box::new(Expression::Identifier {
                name: "point".into(),
                span: crate::span::Span::new(0, 0),
            }),
            name: "x".into(),
            span: crate::span::Span::new(0, 0),
        };
        let mut property_ir = String::new();
        backend.emit_expr(&property, &mut property_ir).unwrap();
        assert!(property_ir.contains("getelementptr inbounds %fusion.struct.0"));
        assert!(property_ir.contains("load i64"));
        assert!(value.starts_with("%t"));
    }

    #[test]
    fn enum_constructor_emits_hir_variant_tag_and_payload_layout() {
        let mut backend = LLVMBackend::new();
        let result = HirDefinitionId::new("network", "Result", HirDefinitionKind::Enum);
        let ok = HirDefinitionId::scoped(
            "network",
            ["Result".into()],
            "Ok",
            HirDefinitionKind::Variant,
        );
        backend
            .type_definition_ids
            .insert("network_Result".into(), result.clone());
        backend
            .llvm_type_names_by_id
            .insert(result, "%fusion.enum.0".into());
        backend
            .enum_variant_definition_ids
            .insert(("network_Result".into(), "Ok".into()), ok.clone());
        backend
            .enum_variant_layout_names_by_id
            .insert(ok.clone(), "%fusion.variant.0".into());
        backend
            .enum_variant_discriminants_by_id
            .insert(ok.clone(), 1);
        backend.enum_payloads_by_id.insert(ok, vec![Type::Num]);

        let expression = Expression::EnumConstructor {
            enum_name: "network_Result".into(),
            variant: "Ok".into(),
            arguments: vec![Expression::Number {
                value: 42,
                span: crate::span::Span::new(0, 0),
            }],
            span: crate::span::Span::new(0, 0),
        };
        let mut ir = String::new();
        backend.emit_expr(&expression, &mut ir).unwrap();
        assert!(ir.contains("getelementptr inbounds %fusion.variant.0"));
        assert!(ir.contains("store i64 42"));
        assert!(ir.contains("store i32 1"));
        assert!(ir.contains("store ptr"));
    }

    #[test]
    fn enum_match_uses_hir_variant_discriminants_and_extracts_payload_bindings() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::hir::{HirDefinitionId, HirDefinitionKind};
        use crate::span::Span;

        let mut backend = LLVMBackend::new();
        let result = HirDefinitionId::new("network", "Result", HirDefinitionKind::Enum);
        let ok = HirDefinitionId::scoped(
            "network",
            ["Result".into()],
            "Ok",
            HirDefinitionKind::Variant,
        );
        let err = HirDefinitionId::scoped(
            "network",
            ["Result".into()],
            "Err",
            HirDefinitionKind::Variant,
        );
        backend
            .type_definition_ids
            .insert("network_Result".into(), result.clone());
        backend
            .llvm_type_names_by_id
            .insert(result.clone(), "%fusion.enum.0".into());
        backend
            .enum_variant_definition_ids
            .insert(("network_Result".into(), "Ok".into()), ok.clone());
        backend
            .enum_variant_definition_ids
            .insert(("network_Result".into(), "Err".into()), err.clone());
        backend
            .enum_variant_layout_names_by_id
            .insert(ok.clone(), "%fusion.variant.0".into());
        backend
            .enum_variant_layout_names_by_id
            .insert(err.clone(), "%fusion.variant.1".into());
        backend
            .enum_variant_discriminants_by_id
            .insert(ok.clone(), 0);
        backend
            .enum_variant_discriminants_by_id
            .insert(err.clone(), 1);
        backend.enum_payloads_by_id.insert(ok, vec![Type::Num]);
        backend.enum_payloads_by_id.insert(err, vec![Type::String]);
        backend
            .locals
            .insert("result".into(), (LlType::Ptr, "%result.slot".into()));
        backend
            .nominal_types_by_local
            .insert("result".into(), result);

        let span = Span::new(0, 0);
        let match_statement = Statement::Match {
            expression: Expression::Identifier {
                name: "result".into(),
                span,
            },
            arms: vec![
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Variant {
                            name: "network_Result::Ok".into(),
                            bindings: vec![Pattern {
                                kind: PatternKind::Identifier("value".into()),
                                span,
                            }],
                        },
                        span,
                    },
                    guard: None,
                    body: vec![Statement::Expression {
                        expression: Expression::Identifier {
                            name: "value".into(),
                            span,
                        },
                        span,
                    }],
                    span,
                },
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Variant {
                            name: "network_Result::Err".into(),
                            bindings: vec![Pattern {
                                kind: PatternKind::Wildcard,
                                span,
                            }],
                        },
                        span,
                    },
                    guard: None,
                    body: vec![Statement::Expression {
                        expression: Expression::Number { value: 0, span },
                        span,
                    }],
                    span,
                },
            ],
            span,
        };
        let mut ir = String::new();
        backend.emit_statement(&match_statement, &mut ir).unwrap();
        assert!(ir.contains("load i32"));
        assert!(ir.contains("icmp eq i32"));
        assert!(ir.contains("getelementptr inbounds %fusion.variant.0"));
        assert!(ir.contains("load i64"));
        assert!(ir.contains("br i1"));
        assert!(ir.contains("match.end"));
        assert!(
            !backend.locals.contains_key("value"),
            "match bindings must not escape the arm scope"
        );
    }

    #[test]
    fn enum_match_rejects_variant_from_a_different_hir_enum() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::hir::{HirDefinitionId, HirDefinitionKind};
        use crate::span::Span;

        let mut backend = LLVMBackend::new();
        let actual = HirDefinitionId::new("network", "Result", HirDefinitionKind::Enum);
        let foreign = HirDefinitionId::new("storage", "Result", HirDefinitionKind::Enum);
        let foreign_variant = HirDefinitionId::scoped(
            "storage",
            ["Result".into()],
            "Ok",
            HirDefinitionKind::Variant,
        );
        backend
            .type_definition_ids
            .insert("network_Result".into(), actual.clone());
        backend
            .type_definition_ids
            .insert("storage_Result".into(), foreign.clone());
        backend
            .llvm_type_names_by_id
            .insert(actual.clone(), "%fusion.enum.0".into());
        backend
            .enum_variant_definition_ids
            .insert(("storage_Result".into(), "Ok".into()), foreign_variant);
        backend
            .locals
            .insert("value".into(), (LlType::Ptr, "%value.slot".into()));
        backend
            .nominal_types_by_local
            .insert("value".into(), actual);
        let span = Span::new(0, 0);
        let statement = Statement::Match {
            expression: Expression::Identifier {
                name: "value".into(),
                span,
            },
            arms: vec![MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Variant {
                        name: "storage_Result::Ok".into(),
                        bindings: vec![],
                    },
                    span,
                },
                guard: None,
                body: vec![],
                span,
            }],
            span,
        };
        let error = backend
            .emit_statement(&statement, &mut String::new())
            .unwrap_err();
        assert!(
            error.to_string().contains("different") || error.to_string().contains("resolves to")
        );
    }

    #[test]
    fn nested_enum_payload_patterns_dispatch_by_nested_hir_identity() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::hir::{HirDefinitionId, HirDefinitionKind};
        use crate::span::Span;

        let mut backend = LLVMBackend::new();
        let outer = HirDefinitionId::new("app", "Event", HirDefinitionKind::Enum);
        let inner = HirDefinitionId::new("state", "State", HirDefinitionKind::Enum);
        let outer_variant = HirDefinitionId::scoped(
            "app",
            ["Event".into()],
            "Nested",
            HirDefinitionKind::Variant,
        );
        let ready = HirDefinitionId::scoped(
            "state",
            ["State".into()],
            "Ready",
            HirDefinitionKind::Variant,
        );
        backend
            .type_definition_ids
            .insert("app_Event".into(), outer.clone());
        backend
            .type_definition_ids
            .insert("state_State".into(), inner.clone());
        backend
            .llvm_type_names_by_id
            .insert(outer.clone(), "%fusion.event".into());
        backend
            .llvm_type_names_by_id
            .insert(inner.clone(), "%fusion.state".into());
        backend
            .enum_variant_definition_ids
            .insert(("app_Event".into(), "Nested".into()), outer_variant.clone());
        backend
            .enum_variant_definition_ids
            .insert(("state_State".into(), "Ready".into()), ready.clone());
        backend
            .enum_variant_layout_names_by_id
            .insert(outer_variant.clone(), "%fusion.event.nested".into());
        backend
            .enum_variant_layout_names_by_id
            .insert(ready.clone(), "%fusion.state.ready".into());
        backend
            .enum_variant_discriminants_by_id
            .insert(outer_variant.clone(), 2);
        backend
            .enum_variant_discriminants_by_id
            .insert(ready.clone(), 1);
        backend
            .enum_payloads_by_id
            .insert(outer_variant, vec![Type::Enum("state_State".into())]);
        backend.enum_payloads_by_id.insert(ready, vec![Type::Num]);
        backend
            .locals
            .insert("event".into(), (LlType::Ptr, "%event.slot".into()));
        backend.nominal_types_by_local.insert("event".into(), outer);

        let span = Span::new(0, 0);
        let pattern = Pattern {
            kind: PatternKind::Variant {
                name: "app_Event::Nested".into(),
                bindings: vec![Pattern {
                    kind: PatternKind::Variant {
                        name: "state_State::Ready".into(),
                        bindings: vec![Pattern {
                            kind: PatternKind::Identifier("code".into()),
                            span,
                        }],
                    },
                    span,
                }],
            },
            span,
        };
        let statement = Statement::Match {
            expression: Expression::Identifier {
                name: "event".into(),
                span,
            },
            arms: vec![MatchArm {
                pattern,
                guard: None,
                body: vec![],
                span,
            }],
            span,
        };
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("%fusion.event.nested"));
        assert!(ir.contains("%fusion.state.ready"));
        assert!(
            ir.matches("icmp eq i32").count() >= 2,
            "both enum levels need discriminant checks"
        );
        assert!(ir.contains("load i64"));
        assert!(
            !backend.locals.contains_key("code"),
            "nested payload bindings must not escape the arm"
        );
    }

    #[test]
    fn struct_patterns_in_match_arms_use_hir_field_indices() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::hir::{HirDefinitionId, HirDefinitionKind};
        use crate::span::Span;

        let mut backend = LLVMBackend::new();
        let point = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let x =
            HirDefinitionId::scoped("geometry", ["Point".into()], "x", HirDefinitionKind::Field);
        let y =
            HirDefinitionId::scoped("geometry", ["Point".into()], "y", HirDefinitionKind::Field);
        backend
            .type_definition_ids
            .insert("geometry_Point".into(), point.clone());
        backend
            .llvm_type_names_by_id
            .insert(point.clone(), "%fusion.point".into());
        backend.struct_layouts_by_id.insert(
            point.clone(),
            StructDefinition {
                fields: vec![("x".into(), Type::Num), ("y".into(), Type::Num)],
            },
        );
        backend
            .struct_field_definition_ids
            .insert(("geometry_Point".into(), "x".into()), x.clone());
        backend
            .struct_field_definition_ids
            .insert(("geometry_Point".into(), "y".into()), y.clone());
        backend.struct_field_indices_by_id.insert(x, 0);
        backend.struct_field_indices_by_id.insert(y, 1);
        backend
            .locals
            .insert("point".into(), (LlType::Ptr, "%point.slot".into()));
        backend.nominal_types_by_local.insert("point".into(), point);

        let span = Span::new(0, 0);
        let statement = Statement::Match {
            expression: Expression::Identifier {
                name: "point".into(),
                span,
            },
            arms: vec![MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Struct {
                        name: "geometry_Point".into(),
                        fields: vec![(
                            "x".into(),
                            Pattern {
                                kind: PatternKind::Identifier("x_value".into()),
                                span,
                            },
                        )],
                        has_rest: true,
                    },
                    span,
                },
                guard: None,
                body: vec![],
                span,
            }],
            span,
        };
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("getelementptr inbounds %fusion.point"));
        assert!(ir.contains("load i64"));
        assert!(
            !backend.locals.contains_key("x_value"),
            "struct pattern bindings must not escape the arm"
        );
    }

    #[test]
    fn nested_struct_patterns_can_match_fields_inside_enum_payloads() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::hir::{HirDefinitionId, HirDefinitionKind};
        use crate::span::Span;

        let mut backend = LLVMBackend::new();
        let event = HirDefinitionId::new("app", "Event", HirDefinitionKind::Enum);
        let address = HirDefinitionId::new("geo", "Address", HirDefinitionKind::Struct);
        let located = HirDefinitionId::scoped(
            "app",
            ["Event".into()],
            "Located",
            HirDefinitionKind::Variant,
        );
        let city =
            HirDefinitionId::scoped("geo", ["Address".into()], "city", HirDefinitionKind::Field);
        backend
            .type_definition_ids
            .insert("app_Event".into(), event.clone());
        backend
            .type_definition_ids
            .insert("geo_Address".into(), address.clone());
        backend
            .llvm_type_names_by_id
            .insert(event.clone(), "%fusion.event".into());
        backend
            .llvm_type_names_by_id
            .insert(address.clone(), "%fusion.address".into());
        backend
            .enum_variant_definition_ids
            .insert(("app_Event".into(), "Located".into()), located.clone());
        backend
            .enum_variant_layout_names_by_id
            .insert(located.clone(), "%fusion.event.located".into());
        backend
            .enum_variant_discriminants_by_id
            .insert(located.clone(), 0);
        backend
            .enum_payloads_by_id
            .insert(located, vec![Type::Struct("geo_Address".into())]);
        backend.struct_layouts_by_id.insert(
            address,
            StructDefinition {
                fields: vec![("city".into(), Type::String)],
            },
        );
        backend
            .struct_field_definition_ids
            .insert(("geo_Address".into(), "city".into()), city.clone());
        backend.struct_field_indices_by_id.insert(city, 0);
        backend
            .locals
            .insert("event".into(), (LlType::Ptr, "%event.slot".into()));
        backend.nominal_types_by_local.insert("event".into(), event);

        let span = Span::new(0, 0);
        let pattern = Pattern {
            kind: PatternKind::Variant {
                name: "app_Event::Located".into(),
                bindings: vec![Pattern {
                    kind: PatternKind::Struct {
                        name: "geo_Address".into(),
                        fields: vec![(
                            "city".into(),
                            Pattern {
                                kind: PatternKind::Identifier("city_name".into()),
                                span,
                            },
                        )],
                        has_rest: false,
                    },
                    span,
                }],
            },
            span,
        };
        let statement = Statement::Match {
            expression: Expression::Identifier {
                name: "event".into(),
                span,
            },
            arms: vec![MatchArm {
                pattern,
                guard: None,
                body: vec![],
                span,
            }],
            span,
        };
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("%fusion.event.located"));
        assert!(ir.contains("getelementptr inbounds %fusion.address"));
        assert!(ir.contains("load ptr"));
        assert!(ir.contains("load ptr"));
        assert!(!backend.locals.contains_key("city_name"));
    }

    #[test]
    fn scalar_boolean_or_pattern_lowers_to_alternative_tests() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::span::Span;
        let mut backend = LLVMBackend::new();
        backend
            .locals
            .insert("flag".into(), (LlType::I1, "%flag.slot".into()));
        let span = Span::new(0, 0);
        let statement = Statement::Match {
            expression: Expression::Identifier {
                name: "flag".into(),
                span,
            },
            arms: vec![MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Or(vec![
                        Pattern {
                            kind: PatternKind::Boolean(true),
                            span,
                        },
                        Pattern {
                            kind: PatternKind::Boolean(false),
                            span,
                        },
                    ]),
                    span,
                },
                guard: None,
                body: vec![],
                span,
            }],
            span,
        };
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.matches("icmp eq i1").count() == 2);
        assert!(ir.contains("pattern.or.next"));
        assert!(
            ir.contains("unreachable"),
            "exhaustive boolean or-pattern must terminate unmatched flow"
        );
    }

    #[test]
    fn integer_range_pattern_emits_both_bounds() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::span::Span;
        let mut backend = LLVMBackend::new();
        let span = Span::new(0, 0);
        let statement = Statement::Match {
            expression: Expression::Number { value: 4, span },
            arms: vec![
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Range {
                            start: 1,
                            end: 5,
                            inclusive: false,
                        },
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Wildcard,
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
            ],
            span,
        };
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("icmp sge i64"));
        assert!(ir.contains("icmp slt i64"));
    }

    #[test]
    fn tuple_patterns_decode_each_element_using_its_static_type() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::span::Span;
        let mut backend = LLVMBackend::new();
        let span = Span::new(0, 0);
        let statement = Statement::Match {
            expression: Expression::Tuple {
                elements: vec![
                    Expression::Number { value: 7, span },
                    Expression::Boolean { value: true, span },
                ],
                span,
            },
            arms: vec![
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Tuple(vec![
                            Pattern {
                                kind: PatternKind::Number(7),
                                span,
                            },
                            Pattern {
                                kind: PatternKind::Boolean(true),
                                span,
                            },
                        ]),
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Wildcard,
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
            ],
            span,
        };
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("getelementptr inbounds i64"));
        assert!(ir.contains("trunc i64"));
        assert!(ir.contains("icmp eq i64"));
        assert!(ir.contains("icmp eq i1"));
    }

    #[test]
    fn return_in_both_if_branches_produces_terminated_merge_block() {
        use crate::ast::Statement;
        use crate::span::Span;
        let mut backend = LLVMBackend::new();
        let span = Span::new(0, 0);
        let ret = || Statement::Return {
            value: Some(Expression::Number { value: 1, span }),
            span,
        };
        let mut ir = String::new();
        backend.current_ret = LlType::I64;
        backend
            .emit_if(
                &Expression::Boolean { value: true, span },
                &[ret()],
                Some(&[ret()]),
                &mut ir,
            )
            .unwrap();
        assert!(ir.contains("then"));
        assert!(ir.contains("else"));
        assert!(ir.contains("endif"));
        assert!(ir.contains("unreachable"));
        assert!(
            !ir.contains("br label %endif"),
            "both terminating branches must not branch into the merge"
        );
    }

    #[test]
    fn float_and_string_literal_patterns_use_typed_comparisons() {
        use crate::ast::{MatchArm, Pattern, PatternKind};
        use crate::span::Span;
        let span = Span::new(0, 0);
        let mut backend = LLVMBackend::new();
        let float_statement = Statement::Match {
            expression: Expression::Float { value: 2.5, span },
            arms: vec![
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Float(2.5),
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Wildcard,
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
            ],
            span,
        };
        let mut float_ir = String::new();
        backend
            .emit_statement(&float_statement, &mut float_ir)
            .unwrap();
        assert!(float_ir.contains("fcmp oeq double"));

        let string_statement = Statement::Match {
            expression: Expression::String {
                value: "ready".into(),
                span,
            },
            arms: vec![
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::String("ready".into()),
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Wildcard,
                        span,
                    },
                    guard: None,
                    body: vec![],
                    span,
                },
            ],
            span,
        };
        let mut string_ir = String::new();
        backend
            .emit_statement(&string_statement, &mut string_ir)
            .unwrap();
        assert!(string_ir.contains("call i32 @strcmp(ptr"));
    }

    #[test]
    fn while_break_and_continue_branch_to_nearest_loop_targets() {
        let span = crate::span::Span::new(0, 0);
        let statement = Statement::While {
            condition: Expression::Boolean { value: true, span },
            body: vec![Statement::If {
                condition: Expression::Boolean { value: true, span },
                body: vec![Statement::Continue { span }],
                else_body: Some(vec![Statement::Break { span }]),
                span,
            }],
            span,
        };
        let mut backend = LLVMBackend::new();
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("br label %while.cond."));
        assert!(ir.contains("br label %while.end."));
        assert!(ir.contains("unreachable"));
    }

    #[test]
    fn for_continue_runs_increment_and_break_skips_it() {
        let span = crate::span::Span::new(0, 0);
        let statement = Statement::For {
            variable: "i".into(),
            start: Expression::Number { value: 0, span },
            end: Expression::Number { value: 4, span },
            body: vec![Statement::If {
                condition: Expression::Boolean { value: true, span },
                body: vec![Statement::Continue { span }],
                else_body: Some(vec![Statement::Break { span }]),
                span,
            }],
            span,
        };
        let mut backend = LLVMBackend::new();
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("for.step."));
        assert!(ir.contains("br label %for.step."));
        assert!(ir.contains("br label %for.end."));
        assert!(ir.contains("add i64"));
    }

    #[test]
    fn foreach_continue_uses_step_block_and_break_exits_loop() {
        let span = crate::span::Span::new(0, 0);
        let statement = Statement::ForEach {
            variable: "item".into(),
            iterable: Expression::Array {
                elements: vec![
                    Expression::Number { value: 1, span },
                    Expression::Number { value: 2, span },
                ],
                span,
            },
            body: vec![Statement::If {
                condition: Expression::Boolean { value: true, span },
                body: vec![Statement::Continue { span }],
                else_body: Some(vec![Statement::Break { span }]),
                span,
            }],
            span,
        };
        let mut backend = LLVMBackend::new();
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        assert!(ir.contains("foreach.step."));
        assert!(ir.contains("br label %foreach.step."));
        assert!(ir.contains("br label %foreach.end."));
    }

    #[test]
    fn break_and_continue_outside_loops_are_diagnostics() {
        let span = crate::span::Span::new(0, 0);
        let mut backend = LLVMBackend::new();
        let error = backend
            .emit_statement(&Statement::Break { span }, &mut String::new())
            .unwrap_err();
        assert!(error.to_string().contains("break used outside a loop"));
        let error = backend
            .emit_statement(&Statement::Continue { span }, &mut String::new())
            .unwrap_err();
        assert!(error.to_string().contains("continue used outside a loop"));
    }

    #[test]
    fn nested_loops_use_innermost_break_target() {
        let span = crate::span::Span::new(0, 0);
        let statement = Statement::While {
            condition: Expression::Boolean { value: true, span },
            body: vec![Statement::While {
                condition: Expression::Boolean { value: true, span },
                body: vec![Statement::Break { span }],
                span,
            }],
            span,
        };
        let mut backend = LLVMBackend::new();
        let mut ir = String::new();
        backend.emit_statement(&statement, &mut ir).unwrap();
        let inner_end = ir.find("while.end.5:").unwrap();
        let branch = ir[..inner_end].rfind("br label %while.end.5").unwrap();
        assert!(branch < inner_end);
        assert!(ir.contains("while.end.2:"));
    }

    #[test]
    fn llvm_method_call_emission_uses_resolved_hir_target_and_receiver_abi() {
        let mut backend = LLVMBackend::new();
        let span = crate::span::Span::new(10, 20);
        let target = "__fusion_method_test_Point_read".to_string();
        backend
            .method_call_targets
            .insert((String::new(), span.start, span.end), target.clone());
        backend.functions.insert(
            target.clone(),
            FunctionSig {
                params: vec![LlType::Ptr, LlType::I64],
                ret: LlType::I64,
            },
        );
        backend
            .locals
            .insert("point".into(), (LlType::Ptr, "%point_slot".into()));
        let call = Expression::MethodCall {
            object: Box::new(Expression::Identifier {
                name: "point".into(),
                span,
            }),
            method: "read".into(),
            arguments: vec![Expression::Number { value: 7, span }],
            generic_arguments: vec![],
            span,
        };
        let mut ir = String::new();
        let result = backend.emit_expr(&call, &mut ir).unwrap();
        assert!(ir.contains("call i64 @"));
        assert!(ir.contains("ptr "));
        assert!(ir.contains("i64 7"));
        assert!(backend.functions.contains_key(&target));
        assert!(result.starts_with("%t"));
    }

    #[test]
    fn llvm_project_call_emission_prefers_hir_call_site_target_over_legacy_name() {
        let mut backend = LLVMBackend::new();
        let span = crate::span::Span::new(31, 42);
        backend.function_call_targets.insert(
            (String::new(), span.start, span.end),
            "module_scoped_target".into(),
        );
        backend.functions.insert(
            "module_scoped_target".into(),
            FunctionSig {
                params: vec![],
                ret: LlType::I64,
            },
        );
        let call = Expression::Call {
            name: "legacy_or_ambiguous_name".into(),
            arguments: vec![],
            generic_arguments: vec![],
            span,
        };
        let mut ir = String::new();
        backend.emit_expr(&call, &mut ir).unwrap();
        assert!(ir.contains("@_Fmodule_scoped_target("));
        assert!(!ir.contains("legacy_or_ambiguous_name"));
    }

    #[test]
    fn llvm_call_emission_uses_registered_hir_function_identity() {
        let mut backend = LLVMBackend::new();
        let identity = crate::hir::HirDefinitionId::new(
            "math",
            "sum",
            crate::hir::HirDefinitionKind::Function,
        );
        backend.register_hir_call_target(identity.clone(), "legacy_sum", "math_sum");
        backend.functions.insert(
            "math_sum".into(),
            FunctionSig {
                params: vec![],
                ret: LlType::I64,
            },
        );
        let call = Expression::Call {
            name: "legacy_sum".into(),
            arguments: vec![],
            generic_arguments: vec![],
            span: crate::span::Span::new(1, 2),
        };
        let mut ir = String::new();
        backend.emit_expr(&call, &mut ir).unwrap();
        assert!(ir.contains("@_Fmath_sum("));
        assert!(!ir.contains("@_Flegacy_sum("));
        assert_eq!(backend.function_symbol_for_id(&identity), Some("math_sum"));
    }

    #[test]
    fn direct_hir_emitter_lowers_counted_loop_control_flow() {
        use crate::hir::{
            HirBasicBlock, HirBody, HirBodyLocal, HirExpression as E, HirLocalId,
            HirLocalReference, HirTerminator as T, HirType,
        };
        let loop_local = HirLocalId(0);
        let body = HirBody {
            module: "loops".into(),
            name: "sum".into(),
            owner: None,
            entry: 0,
            exit: 2,
            locals: vec![HirBodyLocal {
                id: loop_local,
                name: "i".into(),
                ty: HirType::Num,
                declaration_span: crate::span::Span::new(0, 1),
                parameter: false,
            }],
            blocks: vec![
                HirBasicBlock {
                    id: 0,
                    operations: vec![],
                    terminator: T::For {
                        variable: "i".into(),
                        local: loop_local,
                        start: E::Number(0),
                        end: E::Number(3),
                        body: 1,
                        exit: 2,
                    },
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 1,
                    operations: vec![],
                    terminator: T::Goto(0),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 2,
                    operations: vec![],
                    terminator: T::Return(Some(E::Identifier("i".into()))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![HirLocalReference {
                        path: vec![0],
                        local: loop_local,
                    }],
                    terminator_call_references: vec![],
                },
            ],
        };
        let mut backend = LLVMBackend::new();
        let ir = backend
            .emit_hir_body(&body, "loop_sum", &[], &HirType::Num)
            .unwrap();
        assert!(ir.contains("icmp slt i64"));
        assert!(ir.contains("hir_for_step_0:"));
        assert!(ir.contains("add i64"));
        assert!(ir.contains("br label %hir_for_step_0"));
    }

    #[test]
    fn direct_hir_emitter_lowers_scalar_match_dispatch() {
        use crate::hir::{
            HirBasicBlock, HirBody, HirExpression as E, HirMatchTarget, HirPattern as P,
            HirTerminator as T, HirType,
        };
        let body = HirBody {
            module: "matches".into(),
            name: "classify".into(),
            owner: None,
            entry: 0,
            exit: 3,
            locals: vec![],
            blocks: vec![
                HirBasicBlock {
                    id: 0,
                    operations: vec![],
                    terminator: T::Match {
                        value: E::Number(2),
                        arms: vec![
                            HirMatchTarget {
                                pattern: P::Number(1),
                                bindings: vec![],
                                guard: None,
                                guard_expression_types: vec![],
                                guard_local_references: vec![],
                                guard_call_references: vec![],
                                block: 1,
                            },
                            HirMatchTarget {
                                pattern: P::Wildcard,
                                bindings: vec![],
                                guard: None,
                                guard_expression_types: vec![],
                                guard_local_references: vec![],
                                guard_call_references: vec![],
                                block: 2,
                            },
                        ],
                        fallback: 3,
                    },
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 1,
                    operations: vec![],
                    terminator: T::Return(Some(E::Number(10))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 2,
                    operations: vec![],
                    terminator: T::Return(Some(E::Number(20))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 3,
                    operations: vec![],
                    terminator: T::Return(Some(E::Number(0))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
            ],
        };
        let mut backend = LLVMBackend::new();
        let ir = backend
            .emit_hir_body(&body, "classify", &[], &HirType::Num)
            .unwrap();
        assert!(ir.contains("icmp eq i64 2, 1"));
        assert!(ir.contains("hir_match_arm_0_0:"));
        assert!(ir.contains("hir_match_fallback_0:"));
    }

    #[test]
    fn direct_hir_emitter_lowers_array_foreach_and_binds_each_element() {
        use crate::hir::{
            HirBasicBlock, HirBody, HirBodyLocal, HirExpression as E, HirLocalId,
            HirLocalReference, HirTerminator as T, HirType, HirTypedExpression,
        };
        let values = HirLocalId(0);
        let item = HirLocalId(1);
        let body = HirBody {
            module: "collections".into(),
            name: "first_or_zero".into(),
            owner: None,
            entry: 0,
            exit: 2,
            locals: vec![
                HirBodyLocal {
                    id: values,
                    name: "values".into(),
                    ty: HirType::Array(Box::new(HirType::Num)),
                    declaration_span: crate::span::Span::new(0, 1),
                    parameter: true,
                },
                HirBodyLocal {
                    id: item,
                    name: "item".into(),
                    ty: HirType::Num,
                    declaration_span: crate::span::Span::new(2, 3),
                    parameter: false,
                },
            ],
            blocks: vec![
                HirBasicBlock {
                    id: 0,
                    operations: vec![],
                    terminator: T::ForEach {
                        variable: "item".into(),
                        local: item,
                        iterable: E::Identifier("values".into()),
                        body: 1,
                        exit: 2,
                    },
                    terminator_expression_types: vec![HirTypedExpression {
                        path: vec![0],
                        ty: HirType::Array(Box::new(HirType::Num)),
                    }],
                    terminator_local_references: vec![HirLocalReference {
                        path: vec![0],
                        local: values,
                    }],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 1,
                    operations: vec![],
                    terminator: T::Return(Some(E::Identifier("item".into()))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![HirLocalReference {
                        path: vec![0],
                        local: item,
                    }],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 2,
                    operations: vec![],
                    terminator: T::Return(Some(E::Number(0))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
            ],
        };
        let mut backend = LLVMBackend::new();
        let ir = backend
            .emit_hir_body(
                &body,
                "first_or_zero",
                &[HirType::Array(Box::new(HirType::Num))],
                &HirType::Num,
            )
            .unwrap();
        assert!(ir.contains("call i64 @fusion_tuple_len(ptr"));
        assert!(ir.contains("hir_foreach_cond_0:"));
        assert!(ir.contains("hir_foreach_step_0:"));
        assert!(ir.contains("store i64"));
        // The body terminates with `return`, so it must not branch to the
        // increment block after its return terminator.
        assert!(ir.contains("ret i64"));
    }

    #[test]
    fn direct_hir_match_binds_identifier_before_guard_evaluation() {
        use crate::hir::{
            HirBasicBlock, HirBody, HirBodyLocal, HirCallReference, HirExpression as E, HirLocalId,
            HirLocalReference, HirMatchTarget, HirPattern as P, HirPatternBinding,
            HirTerminator as T, HirType,
        };
        let input = HirLocalId(0);
        let bound = HirLocalId(1);
        let guard = E::Binary {
            left: Box::new(E::Identifier("captured".into())),
            operator: crate::hir::HirOperator::Greater,
            right: Box::new(E::Number(0)),
        };
        let body = HirBody {
            module: "patterns".into(),
            name: "positive_or_zero".into(),
            owner: None,
            entry: 0,
            exit: 2,
            locals: vec![
                HirBodyLocal {
                    id: input,
                    name: "input".into(),
                    ty: HirType::Num,
                    declaration_span: crate::span::Span::new(0, 1),
                    parameter: true,
                },
                HirBodyLocal {
                    id: bound,
                    name: "captured".into(),
                    ty: HirType::Num,
                    declaration_span: crate::span::Span::new(2, 3),
                    parameter: false,
                },
            ],
            blocks: vec![
                HirBasicBlock {
                    id: 0,
                    operations: vec![],
                    terminator: T::Match {
                        value: E::Identifier("input".into()),
                        arms: vec![HirMatchTarget {
                            pattern: P::Identifier("captured".into()),
                            bindings: vec![HirPatternBinding {
                                name: "captured".into(),
                                local: bound,
                                path: vec![],
                            }],
                            guard: Some(guard),
                            guard_expression_types: vec![],
                            guard_local_references: vec![HirLocalReference {
                                path: vec![0, 0],
                                local: bound,
                            }],
                            guard_call_references: Vec::<HirCallReference>::new(),
                            block: 1,
                        }],
                        fallback: 2,
                    },
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![HirLocalReference {
                        path: vec![0],
                        local: input,
                    }],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 1,
                    operations: vec![],
                    terminator: T::Return(Some(E::Identifier("captured".into()))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![HirLocalReference {
                        path: vec![0],
                        local: bound,
                    }],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 2,
                    operations: vec![],
                    terminator: T::Return(Some(E::Number(0))),
                    terminator_expression_types: vec![],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
            ],
        };
        let mut backend = LLVMBackend::new();
        let ir = backend
            .emit_hir_body(&body, "positive_or_zero", &[HirType::Num], &HirType::Num)
            .unwrap();
        let binding_store = ir.find("store i64 %t").unwrap();
        let guard_load = ir.find("load i64, ptr %hir_local_1").unwrap();
        assert!(
            binding_store < guard_load,
            "pattern binding must be stored before guard evaluation"
        );
    }

    #[test]
    fn direct_hir_body_emission_uses_local_ids_and_explicit_branch_blocks() {
        use crate::hir::{
            HirBasicBlock, HirBody, HirBodyLocal, HirExpression, HirLocalId, HirLocalReference,
            HirTerminator, HirType, HirTypedExpression,
        };
        let local = HirLocalId(0);
        let body = HirBody {
            module: "math".into(),
            name: "increment".into(),
            owner: None,
            entry: 0,
            exit: 1,
            locals: vec![HirBodyLocal {
                id: local,
                name: "value".into(),
                ty: HirType::Num,
                declaration_span: crate::span::Span::new(0, 1),
                parameter: true,
            }],
            blocks: vec![
                HirBasicBlock {
                    id: 0,
                    operations: vec![],
                    terminator: HirTerminator::Branch {
                        condition: HirExpression::Boolean(true),
                        then_block: 1,
                        else_block: 2,
                    },
                    terminator_expression_types: vec![HirTypedExpression {
                        path: vec![0],
                        ty: HirType::Bool,
                    }],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 1,
                    operations: vec![],
                    terminator: HirTerminator::Return(Some(HirExpression::Binary {
                        left: Box::new(HirExpression::Identifier("value".into())),
                        operator: crate::hir::HirOperator::Plus,
                        right: Box::new(HirExpression::Number(1)),
                    })),
                    terminator_expression_types: vec![HirTypedExpression {
                        path: vec![0],
                        ty: HirType::Num,
                    }],
                    terminator_local_references: vec![HirLocalReference {
                        path: vec![0, 0],
                        local,
                    }],
                    terminator_call_references: vec![],
                },
                HirBasicBlock {
                    id: 2,
                    operations: vec![],
                    terminator: HirTerminator::Return(Some(HirExpression::Number(0))),
                    terminator_expression_types: vec![HirTypedExpression {
                        path: vec![0],
                        ty: HirType::Num,
                    }],
                    terminator_local_references: vec![],
                    terminator_call_references: vec![],
                },
            ],
        };
        let mut backend = LLVMBackend::new();
        let ir = backend
            .emit_hir_body(&body, "math_increment", &[HirType::Num], &HirType::Num)
            .unwrap();
        assert!(ir.contains("%hir_local_0 = alloca i64"));
        assert!(ir.contains("load i64, ptr %hir_local_0"));
        assert!(ir.contains("add i64"));
        assert!(ir.contains("ret i64"));
        assert!(ir.contains("br i1 true, label %hir_bb_1, label %hir_bb_2"));
    }

    #[test]
    fn direct_hir_function_calls_use_resolved_declaration_identity() {
        use crate::hir::{
            HirBasicBlock, HirBody, HirBodyLocal, HirCallKind, HirCallReference, HirDefinitionId,
            HirDefinitionKind, HirExpression as E, HirLocalId, HirLocalReference, HirResolvedCall,
            HirTerminator as T, HirType,
        };
        let local = HirLocalId(0);
        let declaration = HirDefinitionId::new("math", "double", HirDefinitionKind::Function);
        let call = HirResolvedCall {
            module: "app".into(),
            span: crate::span::Span::new(10, 20),
            kind: HirCallKind::Function,
            declaration: declaration.clone(),
            receiver_type: None,
            parameter_types: vec![None],
            argument_types: vec![None],
            return_type: None,
            generic_parameters: vec![],
            is_async: false,
            receiver_hir_type: None,
            parameter_hir_types: vec![HirType::Num],
            argument_hir_types: vec![HirType::Num],
            return_hir_type: HirType::Num,
        };
        let expression = E::Call {
            name: "possibly_ambiguous_double".into(),
            arguments: vec![E::Identifier("value".into())],
            generic_arguments: vec![],
            span: call.span,
        };
        let body = HirBody {
            module: "app".into(),
            name: "caller".into(),
            owner: None,
            entry: 0,
            exit: 1,
            locals: vec![HirBodyLocal {
                id: local,
                name: "value".into(),
                ty: HirType::Num,
                declaration_span: crate::span::Span::new(0, 1),
                parameter: true,
            }],
            blocks: vec![HirBasicBlock {
                id: 0,
                operations: vec![],
                terminator: T::Return(Some(expression)),
                terminator_expression_types: vec![],
                terminator_local_references: vec![HirLocalReference {
                    path: vec![0, 0],
                    local,
                }],
                terminator_call_references: vec![HirCallReference {
                    path: vec![0],
                    call,
                }],
            }],
        };
        let mut backend = LLVMBackend::new();
        backend.register_hir_function_symbol(declaration, "math_double");
        backend.functions.insert(
            "math_double".into(),
            FunctionSig {
                params: vec![LlType::I64],
                ret: LlType::I64,
            },
        );
        let ir = backend
            .emit_hir_body(&body, "caller", &[HirType::Num], &HirType::Num)
            .unwrap();
        assert!(ir.contains("call i64 @_Fmath_double(i64"));
        assert!(ir.contains("ret i64 %t"));
    }

    #[test]
    fn direct_hir_method_calls_pass_receiver_before_user_arguments() {
        use crate::hir::{
            HirBasicBlock, HirBody, HirBodyLocal, HirCallKind, HirCallReference, HirDefinitionId,
            HirDefinitionKind, HirExpression as E, HirLocalId, HirLocalReference, HirResolvedCall,
            HirTerminator as T, HirType,
        };
        let receiver_local = HirLocalId(0);
        let receiver_id = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let method_id = HirDefinitionId::scoped(
            "geometry",
            ["inherent".to_string(), "Point".to_string()],
            "shift",
            HirDefinitionKind::Method,
        );
        let call = HirResolvedCall {
            module: "app".into(),
            span: crate::span::Span::new(30, 44),
            kind: HirCallKind::Method,
            declaration: method_id.clone(),
            receiver_type: Some(receiver_id.clone()),
            parameter_types: vec![None],
            argument_types: vec![None],
            return_type: None,
            generic_parameters: vec![],
            is_async: false,
            receiver_hir_type: Some(HirType::Nominal(receiver_id.clone())),
            parameter_hir_types: vec![HirType::Num],
            argument_hir_types: vec![HirType::Num],
            return_hir_type: HirType::Num,
        };
        let expression = E::MethodCall {
            object: Box::new(E::Identifier("point".into())),
            method: "shift".into(),
            arguments: vec![E::Number(2)],
            generic_arguments: vec![],
            span: call.span,
        };
        let body = HirBody {
            module: "app".into(),
            name: "caller".into(),
            owner: None,
            entry: 0,
            exit: 1,
            locals: vec![HirBodyLocal {
                id: receiver_local,
                name: "point".into(),
                ty: HirType::Nominal(receiver_id.clone()),
                declaration_span: crate::span::Span::new(0, 1),
                parameter: true,
            }],
            blocks: vec![HirBasicBlock {
                id: 0,
                operations: vec![],
                terminator: T::Return(Some(expression)),
                terminator_expression_types: vec![],
                terminator_local_references: vec![HirLocalReference {
                    path: vec![0, 0],
                    local: receiver_local,
                }],
                terminator_call_references: vec![HirCallReference {
                    path: vec![0],
                    call,
                }],
            }],
        };
        let symbol = hir_method_backend_symbol(&method_id);
        let mut backend = LLVMBackend::new();
        backend.register_hir_function_symbol(method_id, symbol.clone());
        backend.functions.insert(
            symbol.clone(),
            FunctionSig {
                params: vec![LlType::Ptr, LlType::I64],
                ret: LlType::I64,
            },
        );
        let ir = backend
            .emit_hir_body(
                &body,
                "caller",
                &[HirType::Nominal(receiver_id)],
                &HirType::Num,
            )
            .unwrap();
        assert!(ir.contains(&format!("call i64 @{}(ptr", sanitize_symbol(&symbol))));
        assert!(ir.contains(", i64 2)"));
    }
}
