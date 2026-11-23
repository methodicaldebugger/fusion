//contents of type_checker.rs

/*
Type checker — verifies that operations are type-correct
The type checker determines whether the program uses values in ways allowed
by the language's type rules.

For example:
let x = 10 + 20;

If both integers support addition, this is valid.

The type checker may also need to:
- Resolve variable names and their types.
- Check function arguments and return values.
- Verify that conditions are booleans.
- Check generic types and type constraints.
- Report incompatible types.
*/

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::errors::FusionError;
use crate::span::Span;
use crate::type_expr::TypeExpr;
use crate::types::{EnumDefinition, EnumVariantDefinition, StructDefinition, Type};
use crate::wide_int::{IntKind, WideInt};

#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionType {
    pub parameters: Vec<Type>,
    pub return_type: Type,
    pub generic_parameters: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TypeEnvironment {
    pub scopes: Vec<HashMap<String, VariableInfo>>,
    pub functions: HashMap<String, FunctionType>,
    /// Foreign declarations retained with their validated C ABI signatures.
    pub foreign_functions: HashSet<String>,
    pub foreign_signatures: HashMap<String, crate::foreign::ForeignFunctionDecl>,
    pub structs: HashMap<String, StructDefinition>,
    pub enums: HashMap<String, EnumDefinition>,
    /// Authoritative project definitions indexed by stable HIR declaration ID.
    pub structs_by_id: HashMap<crate::hir::HirDefinitionId, StructDefinition>,
    pub enums_by_id: HashMap<crate::hir::HirDefinitionId, EnumDefinition>,
    pub methods: HashMap<(String, String), FunctionType>,
    /// Project method signatures keyed by the receiver's stable declaration ID.
    /// The string-keyed map above remains a compatibility path for standalone programs.
    pub methods_by_id: HashMap<(crate::hir::HirDefinitionId, String), FunctionType>,
    /// Selected method declaration identity per lowered receiver symbol/name.
    /// Inherent methods win over trait-provided methods, matching HIR resolution.
    pub method_definition_ids: HashMap<(String, String), crate::hir::HirDefinitionId>,
    pub traits: HashMap<String, Vec<TraitMethod>>,
    /// Project trait declarations keyed by stable HIR identity.
    pub traits_by_id: HashMap<crate::hir::HirDefinitionId, Vec<TraitMethod>>,
    pub trait_definition_ids: HashMap<String, crate::hir::HirDefinitionId>,
    /// Traits requested by `#[derive(...)]`; their generated methods share the ordinary method-signature table.
    pub derived_traits: HashMap<String, HashSet<String>>,
    /// Authoritative source identities for project nominal types. The string keys
    /// are compatibility-AST symbols only; equality is decided by these HIR IDs.
    pub nominal_type_ids: HashMap<String, crate::hir::HirDefinitionId>,
}

#[derive(Debug, Clone)]
struct FunctionContext {
    declared_return_type: Option<Type>,
    inferred_return_type: Option<Type>,
    has_return: bool,
}

pub struct TypeChecker {
    pub environment: TypeEnvironment,
    current_function: Option<FunctionContext>,
    loop_depth: usize,
}

fn foreign_integer_literal_value(expression: &Expression) -> Option<i128> {
    match expression {
        Expression::Number { value, .. } => Some(*value as i128),
        Expression::WideNumber { value, .. } => i128::try_from(*value).ok(),
        Expression::Unary {
            operator: UnaryOperator::Negate,
            expression,
            ..
        } => foreign_integer_literal_value(expression)?.checked_neg(),
        _ => None,
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut functions = HashMap::new();

        functions.insert(
            "input".to_string(),
            FunctionType {
                parameters: Vec::new(),
                return_type: Type::String,
                generic_parameters: Vec::new(),
            },
        );
        functions.insert(
            "open".to_string(),
            FunctionType {
                parameters: vec![Type::String],
                return_type: Type::File,
                generic_parameters: Vec::new(),
            },
        );
        functions.insert(
            "print".to_string(),
            FunctionType {
                parameters: vec![Type::Unknown],
                return_type: Type::Void,
                generic_parameters: Vec::new(),
            },
        );
        functions.insert(
            "array".to_string(),
            FunctionType {
                parameters: vec![],
                return_type: Type::Array(Box::new(Type::Unknown)),
                generic_parameters: Vec::new(),
            },
        );
        functions.insert(
            "hashmap".to_string(),
            FunctionType {
                parameters: vec![],
                return_type: Type::HashMap(Box::new(Type::Unknown), Box::new(Type::Unknown)),
                generic_parameters: Vec::new(),
            },
        );
        functions.insert(
            "iterator".to_string(),
            FunctionType {
                parameters: vec![Type::Array(Box::new(Type::Unknown))],
                return_type: Type::Iterator(Box::new(Type::Unknown)),
                generic_parameters: Vec::new(),
            },
        );
        let mut builtins: Vec<(&str, Vec<Type>, Type)> = vec![
            ("args", vec![], Type::Array(Box::new(Type::String))),
            ("read_file", vec![Type::String], Type::String),
            ("write_file", vec![Type::String, Type::String], Type::Void),
            ("file_exists", vec![Type::String], Type::Bool),
            ("remove_file", vec![Type::String], Type::Void),
            ("create_dir", vec![Type::String], Type::Void),
            (
                "list_dir",
                vec![Type::String],
                Type::Array(Box::new(Type::String)),
            ),
            ("sleep_ms", vec![Type::Num], Type::Void),
            ("now_ms", vec![], Type::Num),
            ("unix_time", vec![], Type::Num),
            ("date_utc", vec![Type::Num], Type::String),
            ("random_int", vec![Type::Num, Type::Num], Type::Num),
            ("random_float", vec![], Type::Float),
            (
                "process_run",
                vec![Type::String, Type::Array(Box::new(Type::String))],
                Type::Num,
            ),
            ("system", vec![], Type::String),
            ("cwd", vec![], Type::String),
            (
                "env_get",
                vec![Type::String],
                Type::Option(Box::new(Type::String)),
            ),
            (
                "tcp_connect",
                vec![Type::String, Type::Num],
                Type::TcpStream,
            ),
            ("serialize", vec![Type::Unknown], Type::String),
            ("__fusion_derive_debug", vec![Type::Unknown], Type::String),
            (
                "__fusion_derive_eq",
                vec![Type::Unknown, Type::Unknown],
                Type::Bool,
            ),
            ("deserialize", vec![Type::String], Type::Unknown),
            (
                "format",
                vec![Type::String, Type::Array(Box::new(Type::Unknown))],
                Type::String,
            ),
            ("hex_encode", vec![Type::String], Type::String),
            ("hex_decode", vec![Type::String], Type::String),
            ("base64_encode", vec![Type::String], Type::String),
            ("base64_decode", vec![Type::String], Type::String),
            ("log_debug", vec![Type::Unknown], Type::Void),
            ("log_info", vec![Type::Unknown], Type::Void),
            ("log_warn", vec![Type::Unknown], Type::Void),
            ("log_error", vec![Type::Unknown], Type::Void),
        ];
        builtins.extend(crate::stdlib_ext::signatures());
        for (name, parameters, return_type) in builtins {
            functions.insert(
                name.into(),
                FunctionType {
                    parameters,
                    return_type,
                    generic_parameters: Vec::new(),
                },
            );
        }

        Self {
            environment: TypeEnvironment {
                scopes: vec![HashMap::new()],
                functions,
                foreign_functions: HashSet::new(),
                foreign_signatures: HashMap::new(),
                structs: HashMap::new(),
                enums: HashMap::new(),
                structs_by_id: HashMap::new(),
                enums_by_id: HashMap::new(),
                methods: HashMap::new(),
                methods_by_id: HashMap::new(),
                method_definition_ids: HashMap::new(),
                traits: HashMap::new(),
                traits_by_id: HashMap::new(),
                trait_definition_ids: HashMap::new(),
                derived_traits: HashMap::new(),
                nominal_type_ids: HashMap::new(),
            },
            current_function: None,
            loop_depth: 0,
        }
    }

    // ---------------------------------------------------------------------
    // Scope management
    // ---------------------------------------------------------------------

    fn push_scope(&mut self) {
        self.environment.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.environment.scopes.len() > 1 {
            self.environment.scopes.pop();
        }
    }

    fn declare_variable(
        &mut self,
        name: String,
        ty: Type,
        mutable: bool,
        span: Span,
    ) -> Result<(), FusionError> {
        let scope = self
            .environment
            .scopes
            .last_mut()
            .expect("type checker always has a root scope");

        if scope.contains_key(&name) {
            return Err(FusionError::Syntax {
                message: format!("Variable '{}' is already declared in this scope", name),
                span,
            });
        }

        scope.insert(name, VariableInfo { ty, mutable });

        Ok(())
    }

    fn lookup_variable(&self, name: &str) -> Option<&VariableInfo> {
        for scope in self.environment.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }

        None
    }

    // ---------------------------------------------------------------------
    // Error helpers
    // ---------------------------------------------------------------------

    fn unknown_variable(&self, name: &str, span: Span) -> FusionError {
        FusionError::UnknownVariable {
            name: name.to_string(),
            suggestion: None,
            span,
        }
    }

    fn type_mismatch(
        &self,
        expected: impl Into<String>,
        found: impl Into<String>,
        span: Span,
    ) -> FusionError {
        FusionError::TypeMismatch {
            expected: expected.into(),
            found: found.into(),
            span,
        }
    }

    fn invalid_operation(
        &self,
        left: impl Into<String>,
        operator: impl Into<String>,
        right: impl Into<String>,
        span: Span,
    ) -> FusionError {
        FusionError::InvalidOperation {
            left: left.into(),
            operator: operator.into(),
            right: right.into(),
            span,
        }
    }

    // ---------------------------------------------------------------------
    // Type utilities
    // ---------------------------------------------------------------------

    fn struct_definition(&self, symbol: &str) -> Option<&StructDefinition> {
        self.environment
            .nominal_type_ids
            .get(symbol)
            .and_then(|identity| self.environment.structs_by_id.get(identity))
            .or_else(|| self.environment.structs.get(symbol))
    }

    fn enum_definition(&self, symbol: &str) -> Option<&EnumDefinition> {
        self.environment
            .nominal_type_ids
            .get(symbol)
            .and_then(|identity| self.environment.enums_by_id.get(identity))
            .or_else(|| self.environment.enums.get(symbol))
    }

    fn method_signature(&self, receiver_symbol: &str, method_name: &str) -> Option<&FunctionType> {
        if let Some(identity) = self.environment.nominal_type_ids.get(receiver_symbol) {
            // Project code must not fall back to a flat method-name table: a
            // missing identity-keyed entry is a genuine unresolved method.
            return self
                .environment
                .methods_by_id
                .get(&(identity.clone(), method_name.to_string()));
        }
        self.environment
            .methods
            .get(&(receiver_symbol.to_string(), method_name.to_string()))
    }

    fn trait_methods(&self, trait_symbol: &str) -> Option<&Vec<TraitMethod>> {
        if let Some(identity) = self.environment.trait_definition_ids.get(trait_symbol) {
            return self.environment.traits_by_id.get(identity);
        }
        self.environment.traits.get(trait_symbol)
    }

    fn has_struct_definition(&self, symbol: &str) -> bool {
        self.struct_definition(symbol).is_some()
    }

    fn has_enum_definition(&self, symbol: &str) -> bool {
        self.enum_definition(symbol).is_some()
    }

    fn types_compatible(&self, expected: &Type, found: &Type) -> bool {
        // For project nominal types, compare declaration identity before looking
        // at compatibility-AST spellings. This is the semantic authority; the
        // mangled names are retained only for legacy declaration/body lookup.
        match (expected, found) {
            (Type::Struct(expected_name), Type::Struct(found_name))
            | (Type::Enum(expected_name), Type::Enum(found_name)) => {
                match (
                    self.environment.nominal_type_ids.get(expected_name),
                    self.environment.nominal_type_ids.get(found_name),
                ) {
                    (Some(expected_id), Some(found_id)) => return expected_id == found_id,
                    _ => {}
                }
            }
            _ => {}
        }

        if expected == found {
            return true;
        }

        match (expected, found) {
            // Unknown can stand for any concrete type.
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            // Fixed-width numeric annotations share the interpreter's integer / float
            // runtime representation, but retain their declared type for checking.
            (Type::Num, Type::Generic(name)) | (Type::Generic(name), Type::Num)
                if Self::is_fixed_integer_name(name) =>
            {
                true
            }
            (Type::Float, Type::Generic(name)) | (Type::Generic(name), Type::Float)
                if matches!(name.as_str(), "f32" | "f64") =>
            {
                true
            }
            (Type::Generic(a), Type::Generic(b))
                if (Self::is_fixed_integer_name(a) && Self::is_fixed_integer_name(b))
                    || (matches!(a.as_str(), "f32" | "f64")
                        && matches!(b.as_str(), "f32" | "f64")) =>
            {
                true
            }

            // Array element types must be compatible.
            (Type::Tuple(a), Type::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| self.types_compatible(x, y))
            }
            (Type::Array(expected_inner), Type::Array(found_inner)) => {
                self.types_compatible(expected_inner, found_inner)
            }

            // Iterator element types must be compatible.
            (Type::Iterator(expected_inner), Type::Iterator(found_inner)) => {
                self.types_compatible(expected_inner, found_inner)
            }

            // Hash map key/value types must both be compatible.
            (
                Type::HashMap(expected_key, expected_value),
                Type::HashMap(found_key, found_value),
            ) => {
                self.types_compatible(expected_key, found_key)
                    && self.types_compatible(expected_value, found_value)
            }

            // Option element types must be compatible.
            (Type::Option(expected_inner), Type::Option(found_inner)) => {
                self.types_compatible(expected_inner, found_inner)
            }

            // Result<T, E>.
            (Type::Result(expected_ok, expected_err), Type::Result(found_ok, found_err)) => {
                self.types_compatible(expected_ok, found_ok)
                    && self.types_compatible(expected_err, found_err)
            }

            // Task<T>.
            (Type::Task(expected_inner), Type::Task(found_inner)) => {
                self.types_compatible(expected_inner, found_inner)
            }

            _ => false,
        }
    }

    fn numeric_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Num | Type::Float | Type::Unknown)
            || matches!(ty, Type::Generic(name) if Self::is_fixed_integer_name(name) || matches!(name.as_str(), "f32" | "f64"))
    }

    fn is_fixed_integer_name(name: &str) -> bool {
        matches!(
            name,
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "isize"
                | "usize"
        )
    }

    fn integer_type(ty: &Type) -> bool {
        matches!(ty, Type::Num | Type::Unknown)
            || matches!(ty, Type::Generic(name) if Self::is_fixed_integer_name(name))
    }

    fn float_type(ty: &Type) -> bool {
        match ty {
            Type::Float => true,
            Type::Generic(name) => matches!(name.as_str(), "f32" | "f64"),
            _ => false,
        }
    }

    /// Convert a written type to a semantic `Type`.
    ///
    /// All type spellings go through `TypeExpr`, so there is exactly one place that knows the
    /// type grammar and exactly one place (`convert_type_expr`) that gives it meaning.
    fn convert_type(&self, name: &str, span: Span) -> Result<Type, FusionError> {
        self.convert_type_spelling(name, &[], false, span)
    }

    /// Like `convert_type`, additionally resolving the enclosing declaration's generic
    /// parameters and `Self` (the latter stays unchecked, as before).
    fn convert_type_with_generics(
        &self,
        name: &str,
        generic_parameters: &[String],
        span: Span,
    ) -> Result<Type, FusionError> {
        self.convert_type_spelling(name, generic_parameters, true, span)
    }

    fn convert_type_spelling(
        &self,
        name: &str,
        generic_parameters: &[String],
        allow_self: bool,
        span: Span,
    ) -> Result<Type, FusionError> {
        match TypeExpr::parse(name) {
            Ok(expr) => self.convert_type_expr(&expr, generic_parameters, allow_self, span),
            Err(error) => Err(FusionError::Syntax {
                message: format!("Invalid type '{}': {}", name, error),
                span,
            }),
        }
    }

    fn convert_type_expr(
        &self,
        expr: &TypeExpr,
        generic_parameters: &[String],
        allow_self: bool,
        span: Span,
    ) -> Result<Type, FusionError> {
        match expr {
            TypeExpr::OpaquePointer => Ok(Type::Generic("*opaque".to_string())),
            TypeExpr::Array(inner) => Ok(Type::Array(Box::new(self.convert_type_expr(
                inner,
                generic_parameters,
                allow_self,
                span,
            )?))),
            TypeExpr::Tuple(items) => {
                if items.len() < 2 {
                    return Err(FusionError::Syntax {
                        message: "Tuple types require at least two elements".into(),
                        span,
                    });
                }
                let mut converted = Vec::with_capacity(items.len());
                for item in items {
                    converted.push(self.convert_type_expr(
                        item,
                        generic_parameters,
                        allow_self,
                        span,
                    )?);
                }
                Ok(Type::Tuple(converted))
            }
            TypeExpr::Function { .. } => Err(FusionError::Syntax {
                message: format!(
                    "Function type '{}' is not supported yet: the type checker has no function values or closures",
                    expr
                ),
                span,
            }),
            TypeExpr::Named { name, args } => {
                self.convert_named_type(expr, name, args, generic_parameters, allow_self, span)
            }
        }
    }

    fn convert_named_type(
        &self,
        expr: &TypeExpr,
        name: &str,
        args: &[TypeExpr],
        generic_parameters: &[String],
        allow_self: bool,
        span: Span,
    ) -> Result<Type, FusionError> {
        if args.is_empty() {
            if allow_self && name == "Self" {
                return Ok(Type::Unknown);
            }
            if generic_parameters.iter().any(|parameter| parameter == name) {
                return Ok(Type::Generic(name.to_string()));
            }
            return match name {
                "num" => Ok(Type::Num),
                "float" => Ok(Type::Float),
                "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64" | "u128"
                | "isize" | "usize" | "f32" | "f64" => Ok(Type::Generic(name.to_string())),
                "bool" => Ok(Type::Bool),
                "string" => Ok(Type::String),
                "void" => Ok(Type::Void),
                "File" => Ok(Type::File),
                "TcpStream" => Ok(Type::TcpStream),
                "unknown" => Ok(Type::Unknown),
                _ if self.environment.structs.contains_key(name) => {
                    Ok(Type::Struct(name.to_string()))
                }
                _ if self.environment.enums.contains_key(name) => {
                    Ok(Type::Enum(name.to_string()))
                }
                _ => Err(FusionError::Syntax {
                    message: format!("Unknown type '{}'", name),
                    span,
                }),
            };
        }

        let convert_arg = |arg: &TypeExpr| -> Result<Type, FusionError> {
            self.convert_type_expr(arg, generic_parameters, allow_self, span)
        };

        match (name, args.len()) {
            ("Array", 1) => Ok(Type::Array(Box::new(convert_arg(&args[0])?))),
            ("Iterator", 1) => Ok(Type::Iterator(Box::new(convert_arg(&args[0])?))),
            ("Option", 1) => Ok(Type::Option(Box::new(convert_arg(&args[0])?))),
            ("Task", 1) => Ok(Type::Task(Box::new(convert_arg(&args[0])?))),
            ("HashMap", 2) => Ok(Type::HashMap(
                Box::new(convert_arg(&args[0])?),
                Box::new(convert_arg(&args[1])?),
            )),
            ("Result", 2) => Ok(Type::Result(
                Box::new(convert_arg(&args[0])?),
                Box::new(convert_arg(&args[1])?),
            )),
            ("HashMap", _) => Err(FusionError::Syntax {
                message: "HashMap requires two type arguments".into(),
                span,
            }),
            ("Result", _) => Err(FusionError::Syntax {
                message: "Result requires two type arguments".into(),
                span,
            }),
            ("Array" | "Iterator" | "Option" | "Task", _) => Err(FusionError::Syntax {
                message: format!("{} requires one type argument", name),
                span,
            }),
            _ => Err(FusionError::Syntax {
                message: format!("Unknown type '{}'", expr),
                span,
            }),
        }
    }

    fn substitute_generic_type(&self, ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
        match ty {
            Type::Generic(name) => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| ty.clone()),

            Type::Array(inner) => {
                Type::Array(Box::new(self.substitute_generic_type(inner, substitutions)))
            }

            Type::Iterator(inner) => {
                Type::Iterator(Box::new(self.substitute_generic_type(inner, substitutions)))
            }

            Type::HashMap(key, value) => Type::HashMap(
                Box::new(self.substitute_generic_type(key, substitutions)),
                Box::new(self.substitute_generic_type(value, substitutions)),
            ),

            Type::Option(inner) => {
                Type::Option(Box::new(self.substitute_generic_type(inner, substitutions)))
            }

            Type::Result(ok, error) => Type::Result(
                Box::new(self.substitute_generic_type(ok, substitutions)),
                Box::new(self.substitute_generic_type(error, substitutions)),
            ),

            Type::Task(inner) => {
                Type::Task(Box::new(self.substitute_generic_type(inner, substitutions)))
            }

            _ => ty.clone(),
        }
    }

    // ---------------------------------------------------------------------
    // Operator checking
    // ---------------------------------------------------------------------

    fn operator_name(operator: Operator) -> &'static str {
        match operator {
            Operator::Plus => "+",
            Operator::Minus => "-",
            Operator::Multiply => "*",
            Operator::Divide => "/",
            Operator::Equal => "==",
            Operator::NotEqual => "!=",
            Operator::Less => "<",
            Operator::LessEqual => "<=",
            Operator::Greater => ">",
            Operator::GreaterEqual => ">=",
            Operator::And => "<and>",
            Operator::Or => "<or>",
        }
    }

    fn unary_operator_name(operator: UnaryOperator) -> &'static str {
        match operator {
            UnaryOperator::Negate => "-",
            UnaryOperator::Not => "<not>",
            UnaryOperator::Deref => "*",
        }
    }

    fn check_binary_operator(
        &self,
        left: Type,
        operator: Operator,
        right: Type,
        span: Span,
    ) -> Result<Type, FusionError> {
        use Operator::*;

        let overload_name = match operator {
            Plus => Some("add"),
            Minus => Some("sub"),
            Multiply => Some("mul"),
            Divide => Some("div"),
            Equal => Some("eq"),
            NotEqual => Some("ne"),
            Less => Some("lt"),
            LessEqual => Some("le"),
            Greater => Some("gt"),
            GreaterEqual => Some("ge"),
            And | Or => None,
        };
        if let (Type::Struct(type_name), Some(method_name)) = (&left, overload_name) {
            let signature = self.method_signature(type_name, method_name).or_else(|| {
                if matches!(operator, NotEqual) {
                    self.method_signature(type_name, "eq")
                } else {
                    None
                }
            });
            if let Some(signature) = signature {
                if signature.parameters.len() != 2 {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "operator method '{}.{}' must take self and one operand",
                            type_name, method_name
                        ),
                        span,
                    });
                }
                if !self.types_compatible(&signature.parameters[1], &right) {
                    return Err(self.type_mismatch(
                        signature.parameters[1].name(),
                        right.name(),
                        span,
                    ));
                }
                return Ok(signature.return_type.clone());
            }
        }

        match operator {
            Plus => {
                if left == Type::String && right == Type::String {
                    return Ok(Type::String);
                }

                if self.numeric_type(&left) && self.numeric_type(&right) {
                    if Self::float_type(&left) || Self::float_type(&right) {
                        return Ok(Type::Float);
                    }

                    return Ok(Type::Num);
                }

                Err(self.invalid_operation(
                    left.name(),
                    Self::operator_name(operator),
                    right.name(),
                    span,
                ))
            }

            Minus | Multiply | Divide => {
                if !self.numeric_type(&left) || !self.numeric_type(&right) {
                    return Err(self.invalid_operation(
                        left.name(),
                        Self::operator_name(operator),
                        right.name(),
                        span,
                    ));
                }

                if Self::float_type(&left) || Self::float_type(&right) {
                    Ok(Type::Float)
                } else {
                    Ok(Type::Num)
                }
            }

            Equal | NotEqual => {
                if self.types_compatible(&left, &right) {
                    Ok(Type::Bool)
                } else {
                    Err(self.invalid_operation(
                        left.name(),
                        Self::operator_name(operator),
                        right.name(),
                        span,
                    ))
                }
            }

            Less | LessEqual | Greater | GreaterEqual => {
                if self.numeric_type(&left) && self.numeric_type(&right) {
                    Ok(Type::Bool)
                } else {
                    Err(self.invalid_operation(
                        left.name(),
                        Self::operator_name(operator),
                        right.name(),
                        span,
                    ))
                }
            }

            And | Or => {
                if matches!(left, Type::Bool | Type::Unknown)
                    && matches!(right, Type::Bool | Type::Unknown)
                {
                    Ok(Type::Bool)
                } else {
                    Err(self.invalid_operation(
                        left.name(),
                        Self::operator_name(operator),
                        right.name(),
                        span,
                    ))
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // Property checking
    // ---------------------------------------------------------------------

    fn property_type(
        &mut self,
        object: &Expression,
        property: &str,
        span: Span,
    ) -> Result<Type, FusionError> {
        let object_type = self.infer_expression(object)?;

        match object_type {
            Type::Struct(name) => {
                let definition =
                    self.environment
                        .structs
                        .get(&name)
                        .ok_or_else(|| FusionError::Syntax {
                            message: format!("Unknown struct '{}'", name),
                            span,
                        })?;

                definition
                    .fields
                    .iter()
                    .find(|(field_name, _)| field_name == property)
                    .map(|(_, field_type)| field_type.clone())
                    .ok_or_else(|| FusionError::Syntax {
                        message: format!("Struct '{}' has no property '{}'", name, property),
                        span,
                    })
            }

            Type::Unknown => Ok(Type::Unknown),

            other => Err(FusionError::Syntax {
                message: format!("Type '{}' has no properties", other.name()),
                span,
            }),
        }
    }

    fn property_mutable(
        &self,
        object: &Expression,
        property: &str,
        span: Span,
    ) -> Result<bool, FusionError> {
        match object {
            Expression::Identifier { name, .. } => {
                let variable = self
                    .lookup_variable(name)
                    .ok_or_else(|| self.unknown_variable(name, span))?;

                if !variable.mutable {
                    return Ok(false);
                }

                let object_type = &variable.ty;

                match object_type {
                    Type::Struct(struct_name) => {
                        let definition = self.struct_definition(struct_name).ok_or_else(|| {
                            FusionError::Syntax {
                                message: format!("Unknown struct '{}'", struct_name),
                                span,
                            }
                        })?;

                        if definition.fields.iter().any(|(name, _)| name == property) {
                            Ok(true)
                        } else {
                            Err(FusionError::Syntax {
                                message: format!(
                                    "Struct '{}' has no property '{}'",
                                    struct_name, property
                                ),
                                span,
                            })
                        }
                    }

                    Type::Unknown => Ok(true),

                    _ => Err(FusionError::Syntax {
                        message: format!("Type '{}' has no mutable properties", object_type.name()),
                        span,
                    }),
                }
            }

            _ => Ok(false),
        }
    }

    // ---------------------------------------------------------------------
    // Pattern checking
    // ---------------------------------------------------------------------

    fn check_pattern(
        &self,
        pattern: &Pattern,
        expected: &Type,
    ) -> Result<Vec<(String, Type)>, FusionError> {
        let mut bindings = Vec::new();

        match &pattern.kind {
            PatternKind::Wildcard => {}

            PatternKind::At {
                name,
                pattern: nested,
            } => {
                bindings.push((name.clone(), expected.clone()));
                bindings.extend(self.check_pattern(nested, expected)?);
            }
            PatternKind::Struct {
                name,
                fields,
                has_rest,
            } => match expected {
                Type::Struct(expected_name) if expected_name == name => {
                    let definition =
                        self.struct_definition(name)
                            .ok_or_else(|| FusionError::Syntax {
                                message: format!("Unknown struct '{}' in pattern", name),
                                span: pattern.span,
                            })?;
                    if !has_rest && fields.len() != definition.fields.len() {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Struct pattern '{}' must mention every field or include '..'",
                                name
                            ),
                            span: pattern.span,
                        });
                    }
                    let mut seen_fields = HashSet::new();
                    for (field_name, nested) in fields {
                        if !seen_fields.insert(field_name) {
                            return Err(FusionError::Syntax {
                                message: format!(
                                    "Duplicate field '{}' in struct pattern",
                                    field_name
                                ),
                                span: nested.span,
                            });
                        }
                        let (_, field_type) = definition
                            .fields
                            .iter()
                            .find(|(n, _)| n == field_name)
                            .ok_or_else(|| FusionError::Syntax {
                                message: format!("Struct '{}' has no field '{}'", name, field_name),
                                span: nested.span,
                            })?;
                        bindings.extend(self.check_pattern(nested, field_type)?);
                    }
                }
                Type::Unknown => {
                    for (_, nested) in fields {
                        bindings.extend(self.check_pattern(nested, &Type::Unknown)?);
                    }
                }
                other => return Err(self.type_mismatch(name, other.name(), pattern.span)),
            },
            PatternKind::Tuple(patterns) => match expected {
                Type::Tuple(types) if types.len() == patterns.len() => {
                    for (p, ty) in patterns.iter().zip(types) {
                        bindings.extend(self.check_pattern(p, ty)?);
                    }
                }
                Type::Unknown => {
                    for p in patterns {
                        bindings.extend(self.check_pattern(p, &Type::Unknown)?);
                    }
                }
                other => return Err(self.type_mismatch("tuple", other.name(), pattern.span)),
            },
            PatternKind::Or(alternatives) => {
                if let Some(first) = alternatives.first() {
                    bindings.extend(self.check_pattern(first, expected)?);
                }
                for alternative in alternatives.iter().skip(1) {
                    let other = self.check_pattern(alternative, expected)?;
                    let a: Vec<_> = bindings.iter().map(|(n, _)| n).collect();
                    let b: Vec<_> = other.iter().map(|(n, _)| n).collect();
                    let incompatible_types =
                        bindings
                            .iter()
                            .zip(&other)
                            .any(|((_, left_ty), (_, right_ty))| {
                                !self.types_compatible(left_ty, right_ty)
                            });
                    if a != b || incompatible_types {
                        return Err(FusionError::Syntax { message: "Every alternative in an or-pattern must bind the same names in the same order with compatible types".into(), span: alternative.span });
                    }
                }
            }
            PatternKind::Range { .. } => {
                if !Self::integer_type(expected) {
                    return Err(self.type_mismatch("integer", expected.name(), pattern.span));
                }
            }

            PatternKind::Identifier(name) => {
                bindings.push((name.clone(), expected.clone()));
            }

            PatternKind::Number(_) => {
                if !Self::integer_type(expected) {
                    return Err(self.type_mismatch("integer", expected.name(), pattern.span));
                }
            }

            PatternKind::Float(_) => {
                if !matches!(expected, Type::Float | Type::Unknown)
                    && !matches!(expected, Type::Generic(name) if matches!(name.as_str(), "f32" | "f64"))
                {
                    return Err(self.type_mismatch("float", expected.name(), pattern.span));
                }
            }

            PatternKind::String(_) => {
                if !matches!(expected, Type::String | Type::Unknown) {
                    return Err(self.type_mismatch("string", expected.name(), pattern.span));
                }
            }

            PatternKind::Boolean(_) => {
                if !matches!(expected, Type::Bool | Type::Unknown) {
                    return Err(self.type_mismatch("bool", expected.name(), pattern.span));
                }
            }

            PatternKind::Variant {
                name,
                bindings: names,
            } => {
                let enum_name = match expected {
                    Type::Enum(name) => name,
                    Type::Unknown => {
                        for nested in names {
                            bindings.extend(self.check_pattern(nested, &Type::Unknown)?);
                        }
                        return Ok(bindings);
                    }
                    other => {
                        return Err(FusionError::TypeMismatch {
                            expected: "enum".to_string(),
                            found: other.name(),
                            span: pattern.span,
                        });
                    }
                };

                let definition =
                    self.enum_definition(enum_name)
                        .ok_or_else(|| FusionError::Syntax {
                            message: format!("Unknown enum '{}'", enum_name),
                            span: pattern.span,
                        })?;

                // Patterns may be written as `State::Ready(...)`. The AST keeps
                // the qualified spelling so the interpreter can compare it to
                // the runtime enum value, while the enum definition stores only
                // the variant name (`Ready`).
                let (qualified_enum, variant_name) = match name.split_once("::") {
                    Some((qualified_enum, variant_name)) => (Some(qualified_enum), variant_name),
                    None => (None, name.as_str()),
                };

                if let Some(qualified_enum) = qualified_enum {
                    if qualified_enum != enum_name {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Pattern enum '{}' does not match expected enum '{}'",
                                qualified_enum, enum_name
                            ),
                            span: pattern.span,
                        });
                    }
                }

                let variant =
                    definition
                        .variants
                        .get(variant_name)
                        .ok_or_else(|| FusionError::Syntax {
                            message: format!(
                                "Enum '{}' has no variant '{}'",
                                enum_name, variant_name
                            ),
                            span: pattern.span,
                        })?;

                if variant.fields.len() != names.len() {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "Variant '{}' expects {} bindings, found {}",
                            variant_name,
                            variant.fields.len(),
                            names.len()
                        ),
                        span: pattern.span,
                    });
                }

                for (nested_pattern, field_type) in names.iter().zip(&variant.fields) {
                    bindings.extend(self.check_pattern(nested_pattern, field_type)?);
                }
            }
        }

        let mut seen = HashSet::new();

        for (name, _) in &bindings {
            if !seen.insert(name.clone()) {
                return Err(FusionError::Syntax {
                    message: format!("Duplicate pattern binding '{}'", name),
                    span: pattern.span,
                });
            }
        }

        Ok(bindings)
    }

    // ---------------------------------------------------------------------
    // Expression inference
    // ---------------------------------------------------------------------

    pub fn infer_expression(&mut self, expression: &Expression) -> Result<Type, FusionError> {
        match expression {
            Expression::Number { .. } => Ok(Type::Num),

            // Literals above i64::MAX need a wider type. i128 can also hold the
            // negation of any literal up to 2^127; larger ones are u128.
            Expression::WideNumber { value, .. } => {
                let name = if *value <= i128::MAX as u128 {
                    "i128"
                } else {
                    "u128"
                };
                Ok(Type::Generic(name.to_string()))
            }

            Expression::Float { .. } => Ok(Type::Float),

            Expression::Boolean { .. } => Ok(Type::Bool),

            Expression::String { .. } => Ok(Type::String),

            Expression::Identifier { name, span } => self
                .lookup_variable(name)
                .map(|info| info.ty.clone())
                .ok_or_else(|| self.unknown_variable(name, *span)),

            Expression::Tuple { elements, .. } => {
                let mut types = Vec::new();
                for element in elements {
                    types.push(self.infer_expression(element)?);
                }
                if types.len() < 2 {
                    return Err(FusionError::Syntax {
                        message: "Tuple expressions require at least two elements".into(),
                        span: expression.span(),
                    });
                }
                Ok(Type::Tuple(types))
            }

            Expression::Array { elements, span } => {
                if elements.is_empty() {
                    return Ok(Type::Array(Box::new(Type::Unknown)));
                }

                let first_type = self.infer_expression(&elements[0])?;

                for element in &elements[1..] {
                    let element_type = self.infer_expression(element)?;

                    if !self.types_compatible(&first_type, &element_type) {
                        return Err(self.type_mismatch(
                            first_type.name(),
                            element_type.name(),
                            element.span(),
                        ));
                    }
                }

                let _ = span;

                Ok(Type::Array(Box::new(first_type)))
            }

            Expression::Index { array, index, span } => {
                let array_type = self.infer_expression(array)?;
                let index_type = self.infer_expression(index)?;

                if !Self::integer_type(&index_type) {
                    return Err(self.type_mismatch("integer", index_type.name(), index.span()));
                }

                match array_type {
                    Type::Array(inner) => Ok(*inner),

                    Type::Iterator(inner) => Ok(*inner),

                    Type::Tuple(items) => {
                        let Expression::Number { value, .. } = index.as_ref() else {
                            return Err(FusionError::Syntax { message: "Tuple indices must be integer literals because tuple elements may have different types".into(), span: *span });
                        };
                        if *value < 0 || (*value as usize) >= items.len() {
                            return Err(FusionError::Syntax {
                                message: format!(
                                    "Tuple index {} is out of bounds for a tuple of length {}",
                                    value,
                                    items.len()
                                ),
                                span: *span,
                            });
                        }
                        Ok(items[*value as usize].clone())
                    }

                    Type::Struct(type_name) => {
                        let signature =
                            self.method_signature(&type_name, "index").ok_or_else(|| {
                                FusionError::Syntax {
                                    message: format!(
                                        "Type '{}' does not implement Index",
                                        type_name
                                    ),
                                    span: *span,
                                }
                            })?;
                        if signature.parameters.len() != 2
                            || !self.types_compatible(&signature.parameters[1], &index_type)
                        {
                            return Err(self.type_mismatch(
                                signature
                                    .parameters
                                    .get(1)
                                    .map(Type::name)
                                    .unwrap_or_else(|| "index argument".into()),
                                index_type.name(),
                                index.span(),
                            ));
                        }
                        Ok(signature.return_type.clone())
                    }
                    Type::Unknown => Ok(Type::Unknown),

                    other => Err(FusionError::Syntax {
                        message: format!("Cannot index value of type '{}'", other.name()),
                        span: *span,
                    }),
                }
            }

            Expression::Property { object, name, span } => self.property_type(object, name, *span),

            Expression::Await { expression, span } => {
                let awaited = self.infer_expression(expression)?;

                match awaited {
                    Type::Task(inner) => Ok(*inner),
                    Type::Unknown => Ok(Type::Unknown),

                    other => Err(FusionError::TypeMismatch {
                        expected: "Task<T>".to_string(),
                        found: other.name(),
                        span: *span,
                    }),
                }
            }

            Expression::Unary {
                operator,
                expression,
                span,
            } => {
                let operand = self.infer_expression(expression)?;

                match operator {
                    UnaryOperator::Negate => {
                        if self.numeric_type(&operand) {
                            Ok(operand)
                        } else {
                            Err(self.invalid_operation(
                                Self::unary_operator_name(*operator),
                                "",
                                operand.name(),
                                *span,
                            ))
                        }
                    }

                    UnaryOperator::Deref => match operand {
                        Type::Struct(type_name) => self
                            .method_signature(&type_name, "deref")
                            .map(|signature| signature.return_type.clone())
                            .ok_or_else(|| FusionError::Syntax {
                                message: format!("Type '{}' does not implement Deref", type_name),
                                span: *span,
                            }),
                        Type::Unknown => Ok(Type::Unknown),
                        other => Err(FusionError::Syntax {
                            message: format!("Cannot dereference value of type '{}'", other.name()),
                            span: *span,
                        }),
                    },
                    UnaryOperator::Not => {
                        if matches!(operand, Type::Bool | Type::Unknown) {
                            Ok(Type::Bool)
                        } else {
                            Err(self.invalid_operation(
                                Self::unary_operator_name(*operator),
                                "",
                                operand.name(),
                                *span,
                            ))
                        }
                    }
                }
            }

            Expression::Conversion {
                kind,
                expression,
                target_type,
                span,
            } => {
                let source = self.infer_expression(expression)?;
                let target = self.convert_type(target_type, *span)?;
                let primitive_conversion = match (&source, &target) {
                    (Type::Num, Type::Float)
                    | (Type::Float, Type::Num)
                    | (Type::Num, Type::String)
                    | (Type::Float, Type::String)
                    | (Type::Bool, Type::String)
                    | (Type::String, Type::Num)
                    | (Type::String, Type::Float)
                    | (Type::String, Type::Bool)
                    | (Type::Bool, Type::Num)
                    | (Type::Num, Type::Bool)
                    | (Type::Float, Type::Bool)
                    | (Type::Num, Type::Num)
                    | (Type::Float, Type::Float)
                    | (Type::Bool, Type::Bool)
                    | (Type::String, Type::String) => true,
                    (source, target) if self.numeric_type(source) && self.numeric_type(target) => {
                        true
                    }
                    (Type::String, target) if self.numeric_type(target) => true,
                    (source, Type::String) if self.numeric_type(source) => true,
                    (Type::Unknown, Type::Num | Type::Float | Type::Bool | Type::String) => true,
                    _ => false,
                };

                match kind {
                    ConversionKind::As => {
                        if primitive_conversion {
                            Ok(target)
                        } else {
                            Err(FusionError::TypeMismatch {
                                expected: target.name(),
                                found: source.name(),
                                span: *span,
                            })
                        }
                    }
                    ConversionKind::From => {
                        let function_name = format!("from_{}", target.name());
                        if self.environment.functions.contains_key(&function_name)
                            || primitive_conversion
                        {
                            Ok(target)
                        } else {
                            Err(FusionError::Syntax {
                                message: format!(
                                    "no conversion from {} to {}",
                                    source.name(),
                                    target.name()
                                ),
                                span: *span,
                            })
                        }
                    }
                    ConversionKind::Try => {
                        let function_name = format!("try_{}", target.name());
                        if primitive_conversion
                            || self.environment.functions.contains_key(&function_name)
                        {
                            Ok(Type::Option(Box::new(target)))
                        } else {
                            Err(FusionError::TypeMismatch {
                                expected: target.name(),
                                found: source.name(),
                                span: *span,
                            })
                        }
                    }
                }
            }

            Expression::Binary {
                left,
                operator,
                right,
                span,
            } => {
                let left_type = self.infer_expression(left)?;
                let right_type = self.infer_expression(right)?;

                self.check_binary_operator(left_type, *operator, right_type, *span)
            }

            Expression::Call {
                name,
                arguments,
                generic_arguments,
                span,
            } => self.infer_call(name, arguments, generic_arguments, *span),

            Expression::MethodCall {
                object,
                method,
                arguments,
                generic_arguments,
                span,
            } => self.infer_method_call(object, method, arguments, generic_arguments, *span),

            Expression::StructConstructor { name, fields, span } => {
                self.infer_struct_constructor(name, fields, *span)
            }

            Expression::EnumConstructor {
                enum_name,
                variant,
                arguments,
                span,
            } => self.infer_enum_constructor(enum_name, variant, arguments, *span),
        }
    }

    fn validate_foreign_argument(
        &mut self,
        function_name: &str,
        index: usize,
        abi_type: &crate::foreign::ForeignAbiType,
        argument: &Expression,
    ) -> Result<(), FusionError> {
        use crate::foreign::ForeignAbiType as Abi;

        let argument_span = argument.span();
        let invalid = |expected: &str, found: &str| FusionError::Syntax {
            message: format!(
                "Foreign argument {} to '{}' requires {}; found {}. Pass a value explicitly declared with the exact ABI type; implicit numeric conversions are not allowed.",
                index + 1, function_name, expected, found
            ),
            span: argument_span,
        };

        // Integer literals are checked against the exact ABI range before LLVM
        // lowering. Non-literal values must carry the exact fixed-width type;
        // the general Fusion `num` type is deliberately not width-safe.
        if let Some((bits, signed)) = match abi_type {
            Abi::I8 => Some((8, true)),
            Abi::U8 => Some((8, false)),
            Abi::I16 => Some((16, true)),
            Abi::U16 => Some((16, false)),
            Abi::I32 => Some((32, true)),
            Abi::U32 => Some((32, false)),
            Abi::I64 => Some((64, true)),
            Abi::U64 => Some((64, false)),
            _ => None,
        } {
            if let Some(value) = foreign_integer_literal_value(argument) {
                let in_range = if signed {
                    let min = -(1i128 << (bits - 1));
                    let max = (1i128 << (bits - 1)) - 1;
                    value >= min && value <= max
                } else {
                    value >= 0 && (value as u128) <= ((1u128 << bits) - 1)
                };
                if !in_range {
                    return Err(invalid(
                        &format!("an integer in the {}-bit {} range", bits, if signed { "signed" } else { "unsigned" }),
                        &value.to_string(),
                    ));
                }
                return Ok(());
            }

            let found = self.infer_expression(argument)?;
            let expected_name = match abi_type {
                Abi::I8 => "i8", Abi::U8 => "u8",
                Abi::I16 => "i16", Abi::U16 => "u16",
                Abi::I32 => "i32", Abi::U32 => "u32",
                Abi::I64 => "i64", Abi::U64 => "u64",
                _ => unreachable!(),
            };
            if found == Type::Generic(expected_name.to_string()) {
                return Ok(());
            }
            return Err(invalid(expected_name, &found.name()));
        }

        match abi_type {
            Abi::F32 | Abi::F64 => {
                if let Expression::Float { value, .. } = argument {
                    let valid = value.is_finite()
                        && (*abi_type != Abi::F32
                            || (value.abs() <= f32::MAX as f64
                                && (*value == 0.0 || value.abs() >= f32::from_bits(1) as f64)));
                    if valid {
                        return Ok(());
                    }
                    return Err(invalid(
                        if *abi_type == Abi::F32 { "a finite f32 value" } else { "a finite f64 value" },
                        &value.to_string(),
                    ));
                }
                let found = self.infer_expression(argument)?;
                let expected_name = if *abi_type == Abi::F32 { "f32" } else { "f64" };
                if found == Type::Generic(expected_name.to_string()) {
                    Ok(())
                } else {
                    Err(invalid(expected_name, &found.name()))
                }
            }
            Abi::OpaquePointer => {
                let found = self.infer_expression(argument)?;
                if found == Type::Generic("*opaque".to_string()) {
                    Ok(())
                } else {
                    Err(invalid("*opaque", &found.name()))
                }
            }
            Abi::Void => Err(invalid("a non-void C ABI value", "void")),
            _ => unreachable!("integer ABI types are handled above"),
        }
    }

    fn infer_call(
        &mut self,
        name: &str,
        arguments: &[Expression],
        generic_arguments: &[String],
        span: Span,
    ) -> Result<Type, FusionError> {
        // Foreign functions have already been validated and registered with their
        // ABI signatures during declaration checking. Type-check their arguments
        // through the same FunctionType path as ordinary functions; LLVM lowering
        // handles the foreign ABI and link_name separately.
        if let Some(struct_definition) = self.struct_definition(name).cloned() {
            if !generic_arguments.is_empty() {
                return Err(FusionError::Syntax {
                    message: format!("Struct '{}' does not accept generic arguments", name),
                    span,
                });
            }

            if arguments.len() != struct_definition.fields.len() {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Struct '{}' expects {} arguments, found {}",
                        name,
                        struct_definition.fields.len(),
                        arguments.len()
                    ),
                    span,
                });
            }

            for ((field_name, field_type), argument) in
                struct_definition.fields.iter().zip(arguments)
            {
                let argument_type = self.infer_expression(argument)?;

                if !self.types_compatible(field_type, &argument_type) {
                    return Err(self.type_mismatch(
                        field_type.name(),
                        argument_type.name(),
                        argument.span(),
                    ));
                }

                let _ = field_name;
            }

            return Ok(Type::Struct(name.to_string()));
        }

        // Foreign calls use the retained ABI declaration rather than the
        // width-neutral compatibility rules used by ordinary Fusion numerics.
        if let Some(declaration) = self.environment.foreign_signatures.get(name).cloned() {
            if !generic_arguments.is_empty() {
                return Err(FusionError::Syntax {
                    message: format!("Foreign function '{}' does not accept generic arguments", name),
                    span,
                });
            }
            if arguments.len() != declaration.parameters.len() {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Foreign function '{}' expects {} arguments, found {}",
                        name, declaration.parameters.len(), arguments.len()
                    ),
                    span,
                });
            }
            for (index, (abi_type, argument)) in declaration.parameters.iter().zip(arguments).enumerate() {
                self.validate_foreign_argument(name, index, abi_type, argument)?;
            }
        }

        let function = self
            .environment
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| FusionError::Syntax {
                message: format!("Unknown function '{}'", name),
                span,
            })?;

        // Collection constructors use their parameter list only to describe
        // the resulting collection type; it is not a fixed-arity signature.
        // `hashmap` accepts key/value pairs and `array` accepts any number of
        // elements. Keep the hashmap pair invariant here as well as in HIR
        // call collection, otherwise the HIR-aware checker rejects valid calls.
        let variadic_builtin_arity_is_valid = match name {
            "hashmap" => arguments.len() % 2 == 0,
            "array" => true,
            _ => arguments.len() == function.parameters.len(),
        };
        if !variadic_builtin_arity_is_valid {
            let expected = match name {
                "hashmap" => "an even number of key/value arguments".to_string(),
                _ => function.parameters.len().to_string(),
            };
            return Err(FusionError::Syntax {
                message: format!(
                    "Function '{}' expects {}, found {}",
                    name,
                    expected,
                    arguments.len()
                ),
                span,
            });
        }

        if generic_arguments.len() != function.generic_parameters.len() {
            if !function.generic_parameters.is_empty() {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Function '{}' requires {} generic arguments, found {}",
                        name,
                        function.generic_parameters.len(),
                        generic_arguments.len()
                    ),
                    span,
                });
            }

            if !generic_arguments.is_empty() {
                return Err(FusionError::Syntax {
                    message: format!("Function '{}' does not accept generic arguments", name),
                    span,
                });
            }
        }

        let mut substitutions = HashMap::new();

        for (generic_name, generic_argument) in
            function.generic_parameters.iter().zip(generic_arguments)
        {
            let generic_type = self.convert_type(generic_argument, span)?;

            substitutions.insert(generic_name.clone(), generic_type);
        }

        for (parameter_type, argument) in function.parameters.iter().zip(arguments) {
            let expected = self.substitute_generic_type(parameter_type, &substitutions);

            let found = self.infer_expression(argument)?;

            if !self.types_compatible(&expected, &found) {
                return Err(self.type_mismatch(expected.name(), found.name(), argument.span()));
            }
        }

        Ok(self.substitute_generic_type(&function.return_type, &substitutions))
    }

    fn infer_method_call(
        &mut self,
        object: &Expression,
        method: &str,
        arguments: &[Expression],
        generic_arguments: &[String],
        span: Span,
    ) -> Result<Type, FusionError> {
        let object_type = self.infer_expression(object)?;
        if !generic_arguments.is_empty() {
            return Err(FusionError::Syntax {
                message: format!(
                    "Method '{}' does not currently support explicit generic arguments",
                    method
                ),
                span,
            });
        }
        let check_callback = |checker: &mut Self, expr: &Expression| -> Result<(), FusionError> {
            match expr {
                Expression::Identifier { name, .. }
                    if checker.environment.functions.contains_key(name) =>
                {
                    Ok(())
                }
                _ => Err(FusionError::Syntax {
                    message: "expected a function name as callback".into(),
                    span: expr.span(),
                }),
            }
        };

        match object_type {
            Type::TcpStream => match method {
                "send" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "send expects one argument".into(),
                            span,
                        });
                    }
                    let argument_type = self.infer_expression(&arguments[0])?;
                    if !matches!(argument_type, Type::String | Type::Unknown) {
                        let found = argument_type.name();
                        let argument_span = arguments[0].span();
                        return Err(self.type_mismatch("string", found, argument_span));
                    }
                    Ok(Type::Void)
                }
                "receive" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "receive expects no arguments".into(),
                            span,
                        });
                    };
                    Ok(Type::String)
                }
                "close" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "close expects no arguments".into(),
                            span,
                        });
                    };
                    Ok(Type::Void)
                }
                _ => Err(FusionError::Syntax {
                    message: format!("Unknown TcpStream method '{}'", method),
                    span,
                }),
            },
            Type::File => match method {
                "read" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "read expects no arguments".into(),
                            span,
                        });
                    };
                    Ok(Type::String)
                }
                "write" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "write expects one argument".into(),
                            span,
                        });
                    };
                    let t = self.infer_expression(&arguments[0])?;
                    if !self.types_compatible(&Type::String, &t) {
                        return Err(self.type_mismatch("string", t.name(), arguments[0].span()));
                    };
                    Ok(Type::Void)
                }
                "close" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "close expects no arguments".into(),
                            span,
                        });
                    };
                    Ok(Type::Void)
                }
                _ => Err(FusionError::Syntax {
                    message: format!("Unknown File method '{}'", method),
                    span,
                }),
            },
            Type::Array(inner) => match method {
                "add" | "push" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects one argument", method),
                            span,
                        });
                    };
                    let t = self.infer_expression(&arguments[0])?;
                    if !self.types_compatible(&inner, &t) {
                        return Err(self.type_mismatch(
                            inner.name(),
                            t.name(),
                            arguments[0].span(),
                        ));
                    };
                    Ok(Type::Void)
                }
                "remove_last" | "pop" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects no arguments", method),
                            span,
                        });
                    };
                    Ok(Type::Option(inner))
                }
                "access" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "access expects an index".into(),
                            span,
                        });
                    };
                    let t = self.infer_expression(&arguments[0])?;
                    if !Self::integer_type(&t) {
                        return Err(self.type_mismatch("num", t.name(), arguments[0].span()));
                    };
                    Ok(*inner)
                }
                "modify" => {
                    if arguments.len() != 2 {
                        return Err(FusionError::Syntax {
                            message: "modify expects index and value".into(),
                            span,
                        });
                    };
                    let i = self.infer_expression(&arguments[0])?;
                    if !Self::integer_type(&i) {
                        return Err(self.type_mismatch("num", i.name(), arguments[0].span()));
                    };
                    let v = self.infer_expression(&arguments[1])?;
                    if !self.types_compatible(&inner, &v) {
                        return Err(self.type_mismatch(
                            inner.name(),
                            v.name(),
                            arguments[1].span(),
                        ));
                    };
                    Ok(Type::Void)
                }
                "get_length" | "length" => Ok(Type::Num),
                "capacity" | "count" => Ok(Type::Num),
                "is_empty" => Ok(Type::Bool),
                "first" | "last" => Ok(Type::Option(inner.clone())),
                "dedup" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "dedup expects no arguments".into(),
                            span,
                        });
                    }
                    Ok(Type::Void)
                }
                "truncate" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "truncate expects a length".into(),
                            span,
                        });
                    }
                    let t = self.infer_expression(&arguments[0])?;
                    if !Self::integer_type(&t) {
                        return Err(self.type_mismatch("num", t.name(), arguments[0].span()));
                    }
                    Ok(Type::Void)
                }
                "extend" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "extend expects an array or iterator".into(),
                            span,
                        });
                    }
                    let t = self.infer_expression(&arguments[0])?;
                    match t {
                        Type::Array(other) | Type::Iterator(other)
                            if self.types_compatible(&inner, &other) =>
                        {
                            Ok(Type::Void)
                        }
                        Type::Unknown => Ok(Type::Void),
                        other => Err(self.type_mismatch(
                            "array or iterator",
                            other.name(),
                            arguments[0].span(),
                        )),
                    }
                }
                "clear" | "sort" | "reverse" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects no arguments", method),
                            span,
                        });
                    };
                    Ok(Type::Void)
                }
                "insert_at_index" => {
                    if arguments.len() != 2 {
                        return Err(FusionError::Syntax {
                            message: "insert_at_index expects index and value".into(),
                            span,
                        });
                    };
                    let i = self.infer_expression(&arguments[0])?;
                    let v = self.infer_expression(&arguments[1])?;
                    if !Self::integer_type(&i) {
                        return Err(self.type_mismatch("num", i.name(), arguments[0].span()));
                    };
                    if !self.types_compatible(&inner, &v) {
                        return Err(self.type_mismatch(
                            inner.name(),
                            v.name(),
                            arguments[1].span(),
                        ));
                    };
                    Ok(Type::Void)
                }
                "remove_at_index" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "remove_at_index expects index".into(),
                            span,
                        });
                    };
                    Ok(*inner)
                }
                "contains" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "contains expects one argument".into(),
                            span,
                        });
                    };
                    let t = self.infer_expression(&arguments[0])?;
                    if !self.types_compatible(&inner, &t) {
                        return Err(self.type_mismatch(
                            inner.name(),
                            t.name(),
                            arguments[0].span(),
                        ));
                    };
                    Ok(Type::Bool)
                }
                "iterate" => Ok(Type::Iterator(inner)),
                "map" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "map expects callback".into(),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Iterator(Box::new(Type::Unknown)))
                }
                "filter" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "filter expects callback".into(),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Iterator(inner))
                }
                "enumerate" => Ok(Type::Iterator(Box::new(Type::Array(Box::new(
                    Type::Unknown,
                ))))),
                "collect" => Ok(Type::Array(inner)),
                "find" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "find expects callback".into(),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Option(inner))
                }
                "any" | "all" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects callback", method),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Bool)
                }
                "reduce" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "reduce expects callback".into(),
                            span,
                        });
                    }
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Option(inner))
                }
                "count" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "count expects no arguments".into(),
                            span,
                        });
                    }
                    Ok(Type::Num)
                }
                "take" | "skip" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects a count", method),
                            span,
                        });
                    }
                    let count_ty = self.infer_expression(&arguments[0])?;
                    if !self.types_compatible(&Type::Num, &count_ty) {
                        return Err(self.type_mismatch(
                            "num",
                            count_ty.name(),
                            arguments[0].span(),
                        ));
                    }
                    Ok(Type::Iterator(inner))
                }
                "zip" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "zip expects one array or iterator".into(),
                            span,
                        });
                    }
                    let _ = self.infer_expression(&arguments[0])?;
                    Ok(Type::Iterator(Box::new(Type::Array(Box::new(
                        Type::Unknown,
                    )))))
                }
                "flatten" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "flatten expects no arguments".into(),
                            span,
                        });
                    }
                    Ok(Type::Iterator(Box::new(Type::Unknown)))
                }
                "fold" => {
                    if arguments.len() != 2 {
                        return Err(FusionError::Syntax {
                            message: "fold expects initial value and callback".into(),
                            span,
                        });
                    };
                    let _ = self.infer_expression(&arguments[0])?;
                    check_callback(self, &arguments[1])?;
                    Ok(Type::Unknown)
                }
                _ => Err(FusionError::Syntax {
                    message: format!("Unknown array method '{}'", method),
                    span,
                }),
            },
            Type::Iterator(inner) => match method {
                "map" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "map expects callback".into(),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Iterator(Box::new(Type::Unknown)))
                }
                "filter" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "filter expects callback".into(),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Iterator(inner))
                }
                "enumerate" => Ok(Type::Iterator(Box::new(Type::Array(Box::new(
                    Type::Unknown,
                ))))),
                "collect" => Ok(Type::Array(inner)),
                "find" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "find expects callback".into(),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Option(inner))
                }
                "any" | "all" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects callback", method),
                            span,
                        });
                    };
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Bool)
                }
                "reduce" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "reduce expects callback".into(),
                            span,
                        });
                    }
                    check_callback(self, &arguments[0])?;
                    Ok(Type::Option(inner))
                }
                "count" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "count expects no arguments".into(),
                            span,
                        });
                    }
                    Ok(Type::Num)
                }
                "take" | "skip" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects a count", method),
                            span,
                        });
                    }
                    let count_ty = self.infer_expression(&arguments[0])?;
                    if !self.types_compatible(&Type::Num, &count_ty) {
                        return Err(self.type_mismatch(
                            "num",
                            count_ty.name(),
                            arguments[0].span(),
                        ));
                    }
                    Ok(Type::Iterator(inner))
                }
                "zip" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: "zip expects one array or iterator".into(),
                            span,
                        });
                    }
                    let _ = self.infer_expression(&arguments[0])?;
                    Ok(Type::Iterator(Box::new(Type::Array(Box::new(
                        Type::Unknown,
                    )))))
                }
                "flatten" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "flatten expects no arguments".into(),
                            span,
                        });
                    }
                    Ok(Type::Iterator(Box::new(Type::Unknown)))
                }
                "fold" => {
                    if arguments.len() != 2 {
                        return Err(FusionError::Syntax {
                            message: "fold expects initial value and callback".into(),
                            span,
                        });
                    };
                    let _ = self.infer_expression(&arguments[0])?;
                    check_callback(self, &arguments[1])?;
                    Ok(Type::Unknown)
                }
                _ => Err(FusionError::Syntax {
                    message: format!("Unknown iterator method '{}'", method),
                    span,
                }),
            },
            Type::HashMap(key, value) => match method {
                "insert" => {
                    if arguments.len() != 2 {
                        return Err(FusionError::Syntax {
                            message: "insert expects key and value".into(),
                            span,
                        });
                    };
                    let k = self.infer_expression(&arguments[0])?;
                    let v = self.infer_expression(&arguments[1])?;
                    if !self.types_compatible(&key, &k) {
                        return Err(self.type_mismatch(key.name(), k.name(), arguments[0].span()));
                    };
                    if !self.types_compatible(&value, &v) {
                        return Err(self.type_mismatch(
                            value.name(),
                            v.name(),
                            arguments[1].span(),
                        ));
                    };
                    Ok(Type::Void)
                }
                "get" | "remove" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: format!("{} expects key", method),
                            span,
                        });
                    };
                    let k = self.infer_expression(&arguments[0])?;
                    if !self.types_compatible(&key, &k) {
                        return Err(self.type_mismatch(key.name(), k.name(), arguments[0].span()));
                    };
                    Ok(Type::Option(value))
                }
                "contains_key" => Ok(Type::Bool),
                "clear" => Ok(Type::Void),
                "keys" => Ok(Type::Array(key)),
                "values" => Ok(Type::Array(value)),
                "iterate" => Ok(Type::Iterator(Box::new(Type::Array(Box::new(
                    Type::Unknown,
                ))))),
                "get_length" | "length" => Ok(Type::Num),
                "count" => Ok(Type::Num),
                "is_empty" => Ok(Type::Bool),
                _ => Err(FusionError::Syntax {
                    message: format!("Unknown hash map method '{}'", method),
                    span,
                }),
            },
            Type::String => match method {
                "create" => Ok(Type::String),
                "concatenate" | "replace" => {
                    for a in arguments {
                        let t = self.infer_expression(a)?;
                        if !matches!(t, Type::String | Type::Unknown) {
                            return Err(self.type_mismatch("string", t.name(), a.span()));
                        }
                    }
                    Ok(Type::String)
                }
                "substring" => Ok(Type::String),
                "find" => Ok(Type::Option(Box::new(Type::Num))),
                "split" => Ok(Type::Array(Box::new(Type::String))),
                "trim" | "to_uppercase" | "to_lowercase" => Ok(Type::String),
                "starts_with" | "ends_with" | "contains" => Ok(Type::Bool),
                "length" => Ok(Type::Num),
                "iterate" => Ok(Type::Iterator(Box::new(Type::String))),
                _ => Err(FusionError::Syntax {
                    message: format!("Unknown string method '{}'", method),
                    span,
                }),
            },
            Type::Struct(type_name) => {
                let signature = self
                    .method_signature(&type_name, method)
                    .cloned()
                    .ok_or_else(|| FusionError::Syntax {
                        message: format!("Unknown method '{}' for type '{}'", method, type_name),
                        span,
                    })?;
                if signature.parameters.len() != arguments.len() + 1 {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "Method '{}.{}' expects {} explicit arguments",
                            type_name,
                            method,
                            signature.parameters.len().saturating_sub(1)
                        ),
                        span,
                    });
                }
                let receiver_type = Type::Struct(type_name.clone());
                if let Some(first) = signature.parameters.first() {
                    let first_type = first.clone();
                    let _ = receiver_type;
                    if !self.types_compatible(&first_type, &Type::Struct(type_name.clone()))
                        && !matches!(first_type, Type::Unknown | Type::Generic(_))
                    {
                        return Err(self.type_mismatch(first_type.name(), type_name, span));
                    }
                }
                for (param, arg) in signature.parameters.iter().skip(1).zip(arguments) {
                    let found = self.infer_expression(arg)?;
                    if !self.types_compatible(&param, &found) {
                        return Err(self.type_mismatch(param.name(), found.name(), arg.span()));
                    }
                }
                Ok(signature.return_type)
            }
            Type::Unknown => Ok(Type::Unknown),
            other => Err(FusionError::Syntax {
                message: format!("Type '{}' has no method '{}'", other.name(), method),
                span,
            }),
        }
    }

    fn infer_struct_constructor(
        &mut self,
        name: &str,
        fields: &[(String, Expression)],
        span: Span,
    ) -> Result<Type, FusionError> {
        let definition =
            self.environment
                .structs
                .get(name)
                .cloned()
                .ok_or_else(|| FusionError::Syntax {
                    message: format!("Unknown struct '{}'", name),
                    span,
                })?;

        if fields.len() != definition.fields.len() {
            return Err(FusionError::Syntax {
                message: format!(
                    "Struct '{}' expects {} fields, found {}",
                    name,
                    definition.fields.len(),
                    fields.len()
                ),
                span,
            });
        }

        let mut supplied = HashSet::new();

        for (field_name, expression) in fields {
            if !supplied.insert(field_name.clone()) {
                return Err(FusionError::Syntax {
                    message: format!("Duplicate field '{}'", field_name),
                    span,
                });
            }

            let expected = definition
                .fields
                .iter()
                .find(|(name, _)| name == field_name)
                .map(|(_, ty)| ty.clone())
                .ok_or_else(|| FusionError::Syntax {
                    message: format!("Struct '{}' has no field '{}'", name, field_name),
                    span,
                })?;

            let found = self.infer_expression(expression)?;

            if !self.types_compatible(&expected, &found) {
                return Err(self.type_mismatch(expected.name(), found.name(), expression.span()));
            }
        }

        for (field_name, _) in &definition.fields {
            if !supplied.contains(field_name) {
                return Err(FusionError::Syntax {
                    message: format!("Missing field '{}' in struct '{}'", field_name, name),
                    span,
                });
            }
        }

        Ok(Type::Struct(name.to_string()))
    }

    fn infer_enum_constructor(
        &mut self,
        enum_name: &str,
        variant: &str,
        arguments: &[Expression],
        span: Span,
    ) -> Result<Type, FusionError> {
        let definition =
            self.enum_definition(enum_name)
                .cloned()
                .ok_or_else(|| FusionError::Syntax {
                    message: format!("Unknown enum '{}'", enum_name),
                    span,
                })?;

        let variant_definition =
            definition
                .variants
                .get(variant)
                .cloned()
                .ok_or_else(|| FusionError::Syntax {
                    message: format!("Enum '{}' has no variant '{}'", enum_name, variant),
                    span,
                })?;

        if arguments.len() != variant_definition.fields.len() {
            return Err(FusionError::Syntax {
                message: format!(
                    "Variant '{}' expects {} arguments, found {}",
                    variant,
                    variant_definition.fields.len(),
                    arguments.len()
                ),
                span,
            });
        }

        for (expected, argument) in variant_definition.fields.iter().zip(arguments) {
            let found = self.infer_expression(argument)?;

            if !self.types_compatible(expected, &found) {
                return Err(self.type_mismatch(expected.name(), found.name(), argument.span()));
            }
        }

        Ok(Type::Enum(enum_name.to_string()))
    }

    // ---------------------------------------------------------------------
    // Assignment checking
    // ---------------------------------------------------------------------

    fn check_assignment(
        &mut self,
        target: &Expression,
        value: &Expression,
        span: Span,
    ) -> Result<(), FusionError> {
        let value_type = self.infer_expression(value)?;

        match target {
            Expression::Identifier {
                name,
                span: target_span,
            } => {
                if let Some(variable) = self.lookup_variable(name).cloned() {
                    if !variable.mutable {
                        return Err(FusionError::CannotAssignToConst {
                            name: name.clone(),
                            span: *target_span,
                        });
                    }

                    if !self.types_compatible(&variable.ty, &value_type) {
                        return Err(self.type_mismatch(
                            variable.ty.name(),
                            value_type.name(),
                            value.span(),
                        ));
                    }

                    return Ok(());
                }

                // Assignment to an unknown identifier is Fusion's inferred
                // declaration form:
                //
                //     x = 10
                //
                // If x does not exist in the current scope, create it with the
                // type of the assigned expression.
                self.declare_variable(name.clone(), value_type, true, *target_span)
            }

            Expression::Property {
                object,
                name,
                span: property_span,
            } => {
                if !self.property_mutable(object, name, *property_span)? {
                    return Err(FusionError::CannotAssignToConst {
                        name: name.clone(),
                        span: *property_span,
                    });
                }

                let property_type = self.property_type(object, name, *property_span)?;

                if !self.types_compatible(&property_type, &value_type) {
                    return Err(self.type_mismatch(
                        property_type.name(),
                        value_type.name(),
                        value.span(),
                    ));
                }

                Ok(())
            }

            Expression::Index {
                array,
                index,
                span: index_span,
            } => {
                let array_type = self.infer_expression(array)?;
                let index_type = self.infer_expression(index)?;

                if !Self::integer_type(&index_type) {
                    return Err(self.type_mismatch("num", index_type.name(), index.span()));
                }

                let element_type = match array_type {
                    Type::Array(inner) => *inner,
                    Type::Unknown => Type::Unknown,

                    other => {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Cannot assign through value of type '{}'",
                                other.name()
                            ),
                            span: *index_span,
                        });
                    }
                };

                if !self.types_compatible(&element_type, &value_type) {
                    return Err(self.type_mismatch(
                        element_type.name(),
                        value_type.name(),
                        value.span(),
                    ));
                }

                Ok(())
            }

            _ => Err(FusionError::Syntax {
                message: "Invalid assignment target".to_string(),
                span,
            }),
        }
    }

    fn check_fixed_literal_range(
        &self,
        target: &Type,
        expression: &Expression,
        span: Span,
    ) -> Result<(), FusionError> {
        // (is_negative, magnitude) of an integer literal, if the expression is one.
        let literal: Option<(bool, u128)> = match expression {
            Expression::Number { value, .. } => Some((*value < 0, value.unsigned_abs() as u128)),
            Expression::WideNumber { value, .. } => Some((false, *value)),
            Expression::Unary {
                operator: UnaryOperator::Negate,
                expression,
                ..
            } => match expression.as_ref() {
                Expression::Number { value, .. } => {
                    Some((*value > 0, value.unsigned_abs() as u128))
                }
                Expression::WideNumber { value, .. } => Some((true, *value)),
                _ => None,
            },
            _ => None,
        };
        if let (Type::Generic(name), Some((negative, magnitude))) = (target, literal) {
            if let Some(kind) = IntKind::from_name(name) {
                let fits = match WideInt::new(negative, magnitude) {
                    Some(value) => kind.contains(value),
                    None => false,
                };
                if !fits {
                    let text = if negative {
                        format!("-{}", magnitude)
                    } else {
                        magnitude.to_string()
                    };
                    return Err(FusionError::Syntax {
                        message: format!(
                            "Integer literal {} is outside the range of {}",
                            text, name
                        ),
                        span,
                    });
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Statement checking
    // ---------------------------------------------------------------------

    fn check_statement(&mut self, statement: &Statement) -> Result<(), FusionError> {
        match statement {
            Statement::VariableDeclarations { declarations, .. } => {
                for declaration in declarations {
                    let declared_type = match &declaration.declared_type {
                        Some(type_name) => Some(self.convert_type(type_name, declaration.span)?),
                        None => None,
                    };

                    let value_type = match &declaration.value {
                        Some(value) => Some(self.infer_expression(value)?),
                        None => None,
                    };

                    let final_type = match (declared_type, value_type) {
                        (Some(declared), Some(found)) => {
                            if let Some(initializer) = &declaration.value {
                                self.check_fixed_literal_range(
                                    &declared,
                                    initializer,
                                    declaration.span,
                                )?;
                            }
                            if !self.types_compatible(&declared, &found) {
                                return Err(self.type_mismatch(
                                    declared.name(),
                                    found.name(),
                                    declaration.span,
                                ));
                            }

                            declared
                        }

                        (Some(declared), None) => declared,

                        (None, Some(found)) => found,

                        (None, None) => {
                            return Err(FusionError::Syntax {
                                message: format!(
                                    "Variable '{}' requires a type or initializer",
                                    declaration.name
                                ),
                                span: declaration.span,
                            });
                        }
                    };

                    self.declare_variable(
                        declaration.name.clone(),
                        final_type,
                        true,
                        declaration.span,
                    )?;
                }

                Ok(())
            }

            Statement::ConstDeclaration {
                name,
                declared_type,
                value,
                span,
                ..
            } => {
                let value_type = self.infer_expression(value)?;

                let final_type = match declared_type {
                    Some(type_name) => {
                        let declared = self.convert_type(type_name, *span)?;
                        self.check_fixed_literal_range(&declared, value, *span)?;

                        if !self.types_compatible(&declared, &value_type) {
                            return Err(self.type_mismatch(
                                declared.name(),
                                value_type.name(),
                                value.span(),
                            ));
                        }

                        declared
                    }

                    None => value_type,
                };

                self.declare_variable(name.clone(), final_type, false, *span)
            }

            Statement::Assignment {
                target,
                value,
                span,
            } => self.check_assignment(target, value, *span),

            Statement::Function { .. } => Ok(()),

            Statement::Struct { .. } => Ok(()),

            Statement::Enum { .. } => Ok(()),

            Statement::Main { body, .. } => {
                self.push_scope();

                let result = self.check_block(body);

                self.pop_scope();

                result
            }

            Statement::Trait { .. } => Ok(()),

            Statement::Impl { .. } => Ok(()),

            Statement::Match {
                expression,
                arms,
                span,
            } => self.check_match(expression, arms, *span),

            Statement::Defer { expression, .. } => {
                self.infer_expression(expression)?;
                Ok(())
            }

            Statement::If {
                condition,
                body,
                else_body,
                ..
            } => {
                let condition_type = self.infer_expression(condition)?;

                if !matches!(condition_type, Type::Bool | Type::Unknown) {
                    return Err(self.type_mismatch(
                        "bool",
                        condition_type.name(),
                        condition.span(),
                    ));
                }

                self.push_scope();
                let body_result = self.check_block(body);
                self.pop_scope();
                body_result?;

                if let Some(else_body) = else_body {
                    self.push_scope();
                    let result = self.check_block(else_body);
                    self.pop_scope();
                    result?;
                }

                Ok(())
            }

            Statement::While {
                condition, body, ..
            } => {
                let condition_type = self.infer_expression(condition)?;

                if !matches!(condition_type, Type::Bool | Type::Unknown) {
                    return Err(self.type_mismatch(
                        "bool",
                        condition_type.name(),
                        condition.span(),
                    ));
                }

                self.push_scope();
                self.loop_depth += 1;

                let result = self.check_block(body);

                self.loop_depth -= 1;
                self.pop_scope();

                result
            }

            Statement::For {
                variable,
                start,
                end,
                body,
                span,
            } => {
                let start_type = self.infer_expression(start)?;
                let end_type = self.infer_expression(end)?;

                if !Self::integer_type(&start_type) {
                    return Err(self.type_mismatch("num", start_type.name(), start.span()));
                }

                if !Self::integer_type(&end_type) {
                    return Err(self.type_mismatch("num", end_type.name(), end.span()));
                }

                self.push_scope();

                let result = (|| {
                    self.declare_variable(variable.clone(), Type::Num, true, *span)?;

                    self.loop_depth += 1;

                    let result = self.check_block(body);

                    self.loop_depth -= 1;

                    result
                })();

                self.pop_scope();

                result
            }

            Statement::ForEach {
                variable,
                iterable,
                body,
                span,
            } => {
                let iterable_type = self.infer_expression(iterable)?;

                let element_type = match iterable_type {
                    Type::Array(inner) => *inner,

                    Type::Iterator(inner) => *inner,
                    Type::HashMap(_, _) => Type::Array(Box::new(Type::Unknown)),

                    Type::Unknown => Type::Unknown,

                    other => {
                        return Err(self.type_mismatch(
                            "array or iterator",
                            other.name(),
                            iterable.span(),
                        ));
                    }
                };

                self.push_scope();

                let result = (|| {
                    self.declare_variable(variable.clone(), element_type, true, *span)?;

                    self.loop_depth += 1;

                    let result = self.check_block(body);

                    self.loop_depth -= 1;

                    result
                })();

                self.pop_scope();

                result
            }

            Statement::Return { value, span } => self.check_return(value.as_ref(), *span),

            Statement::Break { span } => {
                if self.loop_depth == 0 {
                    return Err(FusionError::Syntax {
                        message: "'break' is only valid inside a loop".to_string(),
                        span: *span,
                    });
                }

                Ok(())
            }

            Statement::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(FusionError::Syntax {
                        message: "'continue' is only valid inside a loop".to_string(),
                        span: *span,
                    });
                }

                Ok(())
            }

            Statement::Expression { expression, .. } => {
                self.infer_expression(expression)?;
                Ok(())
            }

            Statement::Import { .. } => Ok(()),

            Statement::Call { expression, .. } => {
                self.infer_expression(expression)?;
                Ok(())
            }
        }
    }

    fn check_return(&mut self, value: Option<&Expression>, span: Span) -> Result<(), FusionError> {
        let context = self
            .current_function
            .as_ref()
            .ok_or_else(|| FusionError::Syntax {
                message: "'return' is only valid inside a function".to_string(),
                span,
            })?
            .clone();

        let value_type = match value {
            Some(expression) => self.infer_expression(expression)?,

            None => Type::Void,
        };

        if let Some(expected) = context.declared_return_type {
            if let Some(expression) = value {
                self.check_fixed_literal_range(&expected, expression, span)?;
            }
            if !self.types_compatible(&expected, &value_type) {
                return Err(self.type_mismatch(expected.name(), value_type.name(), span));
            }
        } else if let Some(existing) = self
            .current_function
            .as_ref()
            .and_then(|function| function.inferred_return_type.clone())
        {
            if !self.types_compatible(&existing, &value_type) {
                return Err(self.type_mismatch(existing.name(), value_type.name(), span));
            }
        } else if let Some(function) = self.current_function.as_mut() {
            function.inferred_return_type = Some(value_type);
        }

        if let Some(function) = self.current_function.as_mut() {
            function.has_return = true;
        }

        Ok(())
    }

    fn check_block(&mut self, statements: &[Statement]) -> Result<(), FusionError> {
        for statement in statements {
            self.check_statement(statement)?;
        }

        Ok(())
    }

    // ---------------------------------------------------------------------
    // Match checking
    // ---------------------------------------------------------------------

    fn pattern_is_irrefutable(pattern: &Pattern) -> bool {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Identifier(_) => true,
            PatternKind::At { pattern, .. } => Self::pattern_is_irrefutable(pattern),
            PatternKind::Tuple(items) => items.iter().all(Self::pattern_is_irrefutable),
            PatternKind::Struct { fields, .. } => fields
                .iter()
                .all(|(_, item)| Self::pattern_is_irrefutable(item)),
            PatternKind::Or(items) => items.iter().any(Self::pattern_is_irrefutable),
            _ => false,
        }
    }

    fn collect_variant_coverage(pattern: &Pattern, covered: &mut HashSet<String>) {
        match &pattern.kind {
            PatternKind::Variant { name, bindings }
                if bindings.iter().all(Self::pattern_is_irrefutable) =>
            {
                let variant = name.rsplit_once("::").map(|(_, v)| v).unwrap_or(name);
                covered.insert(variant.to_string());
            }
            PatternKind::At { pattern, .. } => Self::collect_variant_coverage(pattern, covered),
            PatternKind::Or(items) => {
                for item in items {
                    Self::collect_variant_coverage(item, covered);
                }
            }
            _ => {}
        }
    }

    fn collect_bool_coverage(pattern: &Pattern, covered: &mut HashSet<bool>) {
        match &pattern.kind {
            PatternKind::Boolean(value) => {
                covered.insert(*value);
            }
            PatternKind::At { pattern, .. } => Self::collect_bool_coverage(pattern, covered),
            PatternKind::Or(items) => {
                for item in items {
                    Self::collect_bool_coverage(item, covered);
                }
            }
            _ => {}
        }
    }

    fn collect_integer_ranges(pattern: &Pattern, ranges: &mut Vec<(i128, i128)>) {
        match &pattern.kind {
            PatternKind::Number(value) => ranges.push((*value as i128, *value as i128 + 1)),
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                let exclusive_end = *end as i128 + if *inclusive { 1 } else { 0 };
                ranges.push((*start as i128, exclusive_end));
            }
            PatternKind::At { pattern, .. } => Self::collect_integer_ranges(pattern, ranges),
            PatternKind::Or(items) => {
                for item in items {
                    Self::collect_integer_ranges(item, ranges);
                }
            }
            _ => {}
        }
    }

    fn check_match(
        &mut self,
        expression: &Expression,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<(), FusionError> {
        let expression_type = self.infer_expression(expression)?;
        let mut has_catch_all = false;
        let mut covered_variants = HashSet::new();
        let mut covered_booleans = HashSet::new();
        let mut seen_literals = HashSet::new();
        let mut integer_ranges: Vec<(i128, i128)> = Vec::new();

        for arm in arms {
            if has_catch_all {
                return Err(FusionError::Syntax {
                    message: "Unreachable match arm after an irrefutable pattern".to_string(),
                    span: arm.pattern.span,
                });
            }

            let bindings = self.check_pattern(&arm.pattern, &expression_type)?;
            if arm.guard.is_none() {
                if Self::pattern_is_irrefutable(&arm.pattern) {
                    has_catch_all = true;
                } else {
                    let mut arm_variants = HashSet::new();
                    Self::collect_variant_coverage(&arm.pattern, &mut arm_variants);
                    if arm_variants
                        .iter()
                        .any(|variant| covered_variants.contains(variant))
                    {
                        return Err(FusionError::Syntax {
                            message: "Duplicate or unreachable enum variant pattern".into(),
                            span: arm.pattern.span,
                        });
                    }
                    covered_variants.extend(arm_variants);
                    let mut bools_in_arm = HashSet::new();
                    Self::collect_bool_coverage(&arm.pattern, &mut bools_in_arm);
                    for value in bools_in_arm {
                        let key = format!("bool:{}", value);
                        if !seen_literals.insert(key) {
                            return Err(FusionError::Syntax {
                                message: "Duplicate boolean match pattern".into(),
                                span: arm.pattern.span,
                            });
                        }
                        covered_booleans.insert(value);
                    }

                    let mut arm_ranges = Vec::new();
                    Self::collect_integer_ranges(&arm.pattern, &mut arm_ranges);
                    for (start, end) in arm_ranges {
                        if start >= end {
                            return Err(FusionError::Syntax {
                                message: "Integer match range is empty or reversed".into(),
                                span: arm.pattern.span,
                            });
                        }
                        if integer_ranges
                            .iter()
                            .any(|(old_start, old_end)| start < *old_end && *old_start < end)
                        {
                            return Err(FusionError::Syntax {
                                message: "Overlapping integer match patterns are unreachable"
                                    .into(),
                                span: arm.pattern.span,
                            });
                        }
                        integer_ranges.push((start, end));
                    }

                    let mut literal_keys = Vec::new();
                    Self::collect_non_integer_literal_keys(&arm.pattern, &mut literal_keys);
                    for key in literal_keys {
                        if !seen_literals.insert(key.clone()) {
                            return Err(FusionError::Syntax {
                                message: format!("Duplicate match pattern '{}'", key),
                                span: arm.pattern.span,
                            });
                        }
                    }

                    // A fully covered finite domain makes every later unguarded
                    // arm unreachable. Nested payload constraints do not count
                    // as covering an entire enum variant.
                    let finite_domain_covered = match &expression_type {
                        Type::Bool => covered_booleans.len() == 2,
                        Type::Enum(enum_name) => self
                            .enum_definition(enum_name)
                            .map(|definition| {
                                !definition.variants.is_empty()
                                    && definition
                                        .variants
                                        .keys()
                                        .all(|name| covered_variants.contains(name))
                            })
                            .unwrap_or(false),
                        _ => false,
                    };
                    if finite_domain_covered {
                        has_catch_all = true;
                    }
                }
            }

            self.push_scope();
            let result = (|| {
                for (name, binding_type) in bindings {
                    self.declare_variable(name, binding_type, true, arm.pattern.span)?;
                }
                if let Some(guard) = &arm.guard {
                    let guard_type = self.infer_expression(guard)?;
                    if !matches!(guard_type, Type::Bool | Type::Unknown) {
                        return Err(self.type_mismatch("bool", guard_type.name(), guard.span()));
                    }
                }
                self.check_block(&arm.body)
            })();
            self.pop_scope();
            result?;
        }

        if has_catch_all {
            return Ok(());
        }
        match expression_type {
            Type::Enum(enum_name) => {
                let definition =
                    self.enum_definition(&enum_name)
                        .ok_or_else(|| FusionError::Syntax {
                            message: format!("Unknown enum '{}'", enum_name),
                            span,
                        })?;
                for variant_name in definition.variants.keys() {
                    if !covered_variants.contains(variant_name) {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Non-exhaustive match: missing variant '{}'",
                                variant_name
                            ),
                            span,
                        });
                    }
                }
            }
            Type::Bool => {
                if covered_booleans.len() != 2 {
                    return Err(FusionError::Syntax {
                        message: "Non-exhaustive boolean match".into(),
                        span,
                    });
                }
            }
            Type::Unknown => {}
            _ => {
                return Err(FusionError::Syntax {
                    message: "Match over this type requires a wildcard or irrefutable arm".into(),
                    span,
                })
            }
        }
        Ok(())
    }

    fn collect_non_integer_literal_keys(pattern: &Pattern, keys: &mut Vec<String>) {
        match &pattern.kind {
            PatternKind::Float(value) => keys.push(format!("float:{:?}", value)),
            PatternKind::String(value) => keys.push(format!("string:{}", value)),
            PatternKind::At { pattern, .. } => {
                Self::collect_non_integer_literal_keys(pattern, keys)
            }
            PatternKind::Or(items) => {
                for item in items {
                    Self::collect_non_integer_literal_keys(item, keys);
                }
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------------
    // Return analysis
    // ---------------------------------------------------------------------

    fn block_returns(&mut self, statements: &[Statement]) -> bool {
        for statement in statements {
            match statement {
                Statement::Return { .. } => return true,
                Statement::If {
                    body,
                    else_body: Some(else_body),
                    ..
                } => {
                    if self.block_returns(body) && self.block_returns(else_body) {
                        return true;
                    }
                }
                Statement::Match {
                    expression, arms, ..
                } => {
                    if arms.is_empty() || !arms.iter().all(|arm| self.block_returns(&arm.body)) {
                        continue;
                    }
                    let has_catch_all = arms.iter().any(|arm| {
                        arm.guard.is_none() && Self::pattern_is_irrefutable(&arm.pattern)
                    });
                    let expression_type =
                        self.infer_expression(expression).unwrap_or(Type::Unknown);
                    let exhaustive = if has_catch_all {
                        true
                    } else {
                        match expression_type {
                            Type::Bool => {
                                let mut covered = HashSet::new();
                                for arm in arms.iter().filter(|arm| arm.guard.is_none()) {
                                    Self::collect_bool_coverage(&arm.pattern, &mut covered);
                                }
                                covered.len() == 2
                            }
                            Type::Enum(ref name) => {
                                let mut covered = HashSet::new();
                                for arm in arms.iter().filter(|arm| arm.guard.is_none()) {
                                    Self::collect_variant_coverage(&arm.pattern, &mut covered);
                                }
                                self.enum_definition(name)
                                    .map(|definition| {
                                        !definition.variants.is_empty()
                                            && definition
                                                .variants
                                                .keys()
                                                .all(|variant| covered.contains(variant))
                                    })
                                    .unwrap_or(false)
                            }
                            _ => false,
                        }
                    };
                    if exhaustive {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    // ---------------------------------------------------------------------
    // Function checking
    // ---------------------------------------------------------------------

    fn check_function_body(
        &mut self,
        name: &str,
        generic_parameters: &[String],
        parameters: &[Parameter],
        return_type: Option<&String>,
        body: &[Statement],
        is_async: bool,
        span: Span,
    ) -> Result<(), FusionError> {
        let mut generic_set = HashSet::new();

        for parameter in generic_parameters {
            if !generic_set.insert(parameter.clone()) {
                return Err(FusionError::Syntax {
                    message: format!("Duplicate generic parameter '{}'", parameter),
                    span,
                });
            }
        }

        let declared_return_type = match return_type {
            Some(type_name) => {
                Some(self.convert_type_with_generics(type_name, generic_parameters, span)?)
            }

            None => None,
        };

        let previous_function = self.current_function.take();

        self.current_function = Some(FunctionContext {
            declared_return_type,
            inferred_return_type: None,
            has_return: false,
        });

        self.push_scope();

        let result = (|| {
            for parameter in parameters {
                let parameter_type = match &parameter.type_name {
                    Some(type_name) => self.convert_type_with_generics(
                        type_name,
                        generic_parameters,
                        parameter.span,
                    )?,

                    None => Type::Unknown,
                };

                self.declare_variable(
                    parameter.name.clone(),
                    parameter_type,
                    true,
                    parameter.span,
                )?;
            }

            self.check_block(body)?;

            let context = self
                .current_function
                .as_ref()
                .expect("function context exists")
                .clone();

            if context.declared_return_type.is_some()
                && context.declared_return_type != Some(Type::Void)
                && !self.block_returns(body)
            {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Function '{}' may reach the end without returning a value",
                        name
                    ),
                    span,
                });
            }

            if context.declared_return_type.is_none()
                && context.has_return
                && !self.block_returns(body)
            {
                return Err(FusionError::Syntax {
                    message: format!("Function '{}' may reach the end without returning", name),
                    span,
                });
            }

            Ok(())
        })();

        // Capture the body-inferred return type before restoring the
        // enclosing function context.
        let inferred_return_type = self
            .current_function
            .as_ref()
            .and_then(|context| context.inferred_return_type.clone())
            .unwrap_or(Type::Void);

        self.pop_scope();
        self.current_function = previous_function;

        if let Err(error) = result {
            return Err(error);
        }

        // Publish inferred return types for functions without an explicit
        // return annotation. The registration pass uses `Unknown` so forward
        // calls can still be resolved before the body is checked.
        if return_type.is_none() {
            let final_return_type = if is_async {
                Type::Task(Box::new(inferred_return_type))
            } else {
                inferred_return_type
            };

            if let Some(function) = self.environment.functions.get_mut(name) {
                function.return_type = final_return_type;
            }
        }

        Ok(())
    }

    fn validate_trait_method(
        &self,
        method: &TraitMethod,
        generic_parameters: &[String],
    ) -> Result<(), FusionError> {
        let mut names = HashSet::new();

        for parameter in &method.parameters {
            if !names.insert(parameter.name.clone()) {
                return Err(FusionError::Syntax {
                    message: format!("Duplicate parameter '{}'", parameter.name),
                    span: parameter.span,
                });
            }

            if let Some(type_name) = &parameter.type_name {
                self.convert_type_with_generics(type_name, generic_parameters, parameter.span)?;
            }
        }

        if let Some(return_type) = &method.return_type {
            self.convert_type_with_generics(return_type, generic_parameters, method.span)?;
        }

        Ok(())
    }

    fn check_impl(
        &mut self,
        trait_name: Option<&String>,
        type_name: &str,
        methods: &[Statement],
        span: Span,
    ) -> Result<(), FusionError> {
        if !self.has_struct_definition(type_name) && !self.has_enum_definition(type_name) {
            return Err(FusionError::Syntax {
                message: format!("Cannot implement methods for unknown type '{}'", type_name),
                span,
            });
        }

        if let Some(trait_name) = trait_name {
            if self.trait_methods(trait_name).is_none() {
                return Err(FusionError::Syntax {
                    message: format!("Unknown trait '{}'", trait_name),
                    span,
                });
            }
        }

        if let Some(trait_name) = trait_name {
            if let Some(required) = self.trait_methods(trait_name).cloned() {
                for required_method in required {
                    let found = methods.iter().find(|m| matches!(m, Statement::Function { name, .. } if name == &required_method.name));
                    if found.is_none() {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Impl of trait '{}' for '{}' is missing method '{}'",
                                trait_name, type_name, required_method.name
                            ),
                            span,
                        });
                    }
                }
            }
        }

        for method in methods {
            match method {
                Statement::Function {
                    name,
                    generic_parameters,
                    parameters,
                    return_type,
                    body,
                    is_async,
                    span,
                    ..
                } => {
                    let parameter_types = parameters
                        .iter()
                        .map(|p| match &p.type_name {
                            Some(t) => {
                                self.convert_type_with_generics(t, generic_parameters, p.span)
                            }
                            None => Ok(Type::Unknown),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let ret = match return_type {
                        Some(t) => self.convert_type_with_generics(t, generic_parameters, *span)?,
                        None => Type::Void,
                    };
                    let signature = FunctionType {
                        parameters: parameter_types,
                        return_type: if *is_async {
                            Type::Task(Box::new(ret))
                        } else {
                            ret
                        },
                        generic_parameters: generic_parameters.clone(),
                    };
                    self.environment
                        .methods
                        .insert((type_name.to_string(), name.clone()), signature.clone());
                    if let Some(receiver_id) =
                        self.environment.nominal_type_ids.get(type_name).cloned()
                    {
                        let preferred = self
                            .environment
                            .method_definition_ids
                            .get(&(type_name.to_string(), name.clone()));
                        let current_scope = match trait_name {
                            None => Some("inherent".to_string()),
                            Some(trait_symbol) => self
                                .environment
                                .trait_definition_ids
                                .get(trait_symbol)
                                .map(|id| format!("{}::{}", id.module, id.name)),
                        };
                        let is_selected = preferred
                            .map(|id| id.scope.first().cloned() == current_scope)
                            .unwrap_or(true);
                        if is_selected {
                            self.environment
                                .methods_by_id
                                .insert((receiver_id, name.clone()), signature);
                        }
                    }
                    self.check_function_body(
                        name,
                        generic_parameters,
                        parameters,
                        return_type.as_ref(),
                        body,
                        *is_async,
                        *span,
                    )?;
                }

                _ => {
                    return Err(FusionError::Syntax {
                        message: "Impl blocks may only contain functions".to_string(),
                        span: method.span(),
                    });
                }
            }
        }

        Ok(())
    }

    // ---------------------------------------------------------------------
    // Program checking
    // ---------------------------------------------------------------------

    /// Check a flattened project program while making the HIR declaration index
    /// authoritative for nominal type equality. Every backend type symbol must
    /// map to an existing HIR registry entry of the same kind.
    pub fn check_with_hir(
        &mut self,
        program: &Program,
        identities: HashMap<String, crate::hir::HirDefinitionId>,
        registry: &crate::hir::HirTypeRegistry,
    ) -> Result<(), FusionError> {
        use crate::hir::HirDefinitionKind as Kind;

        for (symbol, identity) in &identities {
            let definition = registry.get(identity).ok_or_else(|| FusionError::Syntax {
                message: format!(
                    "internal HIR type identity for '{}' is missing from the project type registry ({}::{})",
                    symbol, identity.module, identity.name
                ),
                span: Span::default(),
            })?;
            let kind_matches = matches!(
                (&definition.shape, identity.kind),
                (crate::hir::HirTypeShape::Struct { .. }, Kind::Struct)
                    | (crate::hir::HirTypeShape::Enum { .. }, Kind::Enum)
            );
            if !kind_matches {
                return Err(FusionError::Syntax {
                    message: format!(
                        "HIR type identity kind mismatch for '{}::{}'",
                        identity.module, identity.name
                    ),
                    span: Span::default(),
                });
            }
        }

        // Do not silently fall back to string-only nominal comparison in a
        // project compilation if a declaration was omitted from the identity map.
        for statement in &program.statements {
            let declared = match statement {
                Statement::Struct { name, span, .. } => Some((name, Kind::Struct, *span)),
                Statement::Enum { name, span, .. } => Some((name, Kind::Enum, *span)),
                _ => None,
            };
            if let Some((name, expected_kind, span)) = declared {
                let identity = identities.get(name).ok_or_else(|| FusionError::Syntax {
                    message: format!("project type '{}' has no stable HIR identity", name),
                    span,
                })?;
                if identity.kind != expected_kind {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "project type '{}' has an incorrect HIR identity kind",
                            name
                        ),
                        span,
                    });
                }
            }
        }

        self.environment.nominal_type_ids = identities;
        self.environment.trait_definition_ids =
            crate::module_system::backend_trait_definition_ids(program);
        self.environment.method_definition_ids =
            crate::module_system::backend_method_definition_ids(program);
        self.check(program)
    }

    pub fn check(&mut self, program: &Program) -> Result<(), FusionError> {
        // Pass 1: register struct names.
        for statement in &program.statements {
            if let Statement::Struct {
                name,
                derives,
                span,
                ..
            } = statement
            {
                let mut requested = HashSet::new();
                for trait_name in derives {
                    if !matches!(trait_name.as_str(), "Debug" | "Clone" | "PartialEq") {
                        return Err(FusionError::Syntax {
                            message: format!("Unsupported derive '{}'; supported derives are Debug, Clone, and PartialEq", trait_name),
                            span: *span,
                        });
                    }
                    if !requested.insert(trait_name.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!("Duplicate derive '{}'", trait_name),
                            span: *span,
                        });
                    }
                }
                self.environment
                    .derived_traits
                    .insert(name.clone(), requested);
                if self.environment.structs.contains_key(name) {
                    return Err(FusionError::Syntax {
                        message: format!("Duplicate struct '{}'", name),
                        span: *span,
                    });
                }

                self.environment
                    .structs
                    .insert(name.clone(), StructDefinition { fields: Vec::new() });
            }
        }

        // Pass 2: register enum names.
        for statement in &program.statements {
            if let Statement::Enum { name, span, .. } = statement {
                if self.environment.enums.contains_key(name) {
                    return Err(FusionError::Syntax {
                        message: format!("Duplicate enum '{}'", name),
                        span: *span,
                    });
                }

                self.environment.enums.insert(
                    name.clone(),
                    EnumDefinition {
                        variants: HashMap::new(),
                    },
                );
            }
        }

        // Pass 3: resolve struct fields.
        for statement in &program.statements {
            if let Statement::Struct {
                name, fields, span, ..
            } = statement
            {
                let mut seen = HashSet::new();
                let mut resolved_fields = Vec::new();

                for field in fields {
                    if !seen.insert(field.name.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!("Duplicate field '{}'", field.name),
                            span: field.span,
                        });
                    }

                    let field_type = self.convert_type(&field.type_name, field.span)?;

                    resolved_fields.push((field.name.clone(), field_type));
                }

                let definition =
                    self.environment
                        .structs
                        .get_mut(name)
                        .ok_or_else(|| FusionError::Syntax {
                            message: format!("Unknown struct '{}'", name),
                            span: *span,
                        })?;

                definition.fields = resolved_fields;
            }
        }

        // Pass 4: resolve enum variants.
        for statement in &program.statements {
            if let Statement::Enum {
                name,
                variants,
                span,
            } = statement
            {
                let mut resolved_variants = HashMap::new();

                for variant in variants {
                    if resolved_variants.contains_key(&variant.name) {
                        return Err(FusionError::Syntax {
                            message: format!("Duplicate enum variant '{}'", variant.name),
                            span: variant.span,
                        });
                    }

                    let mut resolved_fields = Vec::new();

                    for field_type_name in &variant.fields {
                        resolved_fields.push(self.convert_type(field_type_name, variant.span)?);
                    }

                    resolved_variants.insert(
                        variant.name.clone(),
                        EnumVariantDefinition {
                            fields: resolved_fields,
                        },
                    );
                }

                let definition =
                    self.environment
                        .enums
                        .get_mut(name)
                        .ok_or_else(|| FusionError::Syntax {
                            message: format!("Unknown enum '{}'", name),
                            span: *span,
                        })?;

                definition.variants = resolved_variants;
            }
        }

        // Materialize the authoritative HIR-keyed declaration tables before
        // checking executable bodies. The string-keyed tables remain as a
        // compatibility bridge for standalone compilation and early declaration
        // registration, but project body lookups now prefer these identity maps.
        for (symbol, definition) in self.environment.structs.clone() {
            if let Some(identity) = self.environment.nominal_type_ids.get(&symbol) {
                self.environment
                    .structs_by_id
                    .insert(identity.clone(), definition);
            }
        }
        for (symbol, definition) in self.environment.enums.clone() {
            if let Some(identity) = self.environment.nominal_type_ids.get(&symbol) {
                self.environment
                    .enums_by_id
                    .insert(identity.clone(), definition);
            }
        }

        // Publish imported public type definitions under module aliases. Local
        // declarations keep their legacy unqualified names; qualified aliases
        // are exposed only across an import edge and only for public symbols.
        for importer in &program.modules {
            for import in &importer.imports {
                let target = program
                    .modules
                    .iter()
                    .find(|candidate| candidate.name == import.path)
                    .or_else(|| {
                        program.modules.iter().find(|candidate| {
                            candidate.name.ends_with(&format!(".{}", import.path))
                        })
                    });
                let Some(target) = target else {
                    continue;
                };
                let alias = import.alias.clone().unwrap_or_else(|| {
                    import
                        .path
                        .rsplit('.')
                        .next()
                        .unwrap_or(&import.path)
                        .to_string()
                });
                for statement in &program.statements[target.statement_start..target.statement_end] {
                    match statement {
                        Statement::Struct { name, .. }
                            if target.public_symbols.iter().any(|symbol| symbol == name) =>
                        {
                            if let Some(definition) = self.struct_definition(name).cloned() {
                                self.environment
                                    .structs
                                    .insert(format!("{}.{}", alias, name), definition);
                            }
                        }
                        Statement::Enum { name, .. }
                            if target.public_symbols.iter().any(|symbol| symbol == name) =>
                        {
                            if let Some(definition) = self.environment.enums.get(name).cloned() {
                                self.environment
                                    .enums
                                    .insert(format!("{}.{}", alias, name), definition);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Register trait declarations before validating impl blocks.
        for statement in &program.statements {
            if let Statement::Trait {
                name,
                methods,
                span,
            } = statement
            {
                if self.environment.traits.contains_key(name) {
                    return Err(FusionError::Syntax {
                        message: format!("Duplicate trait '{}'", name),
                        span: *span,
                    });
                }
                for method in methods {
                    self.validate_trait_method(method, &[])?;
                }
                self.environment
                    .traits
                    .insert(name.clone(), methods.clone());
                if let Some(identity) = self.environment.trait_definition_ids.get(name).cloned() {
                    self.environment
                        .traits_by_id
                        .insert(identity, methods.clone());
                }
            }
        }

        // Pass 5: register function signatures.
        for statement in &program.statements {
            if let Statement::Function {
                name,
                generic_parameters,
                parameters,
                return_type,
                is_async,
                foreign_abi,
                link_name,
                span,
                ..
            } = statement
            {
                if let Some(abi) = foreign_abi {
                    if !generic_parameters.is_empty() {
                        return Err(FusionError::Syntax {
                            message: "generic foreign functions are not supported by the C ABI subset".into(),
                            span: *span,
                        });
                    }
                    if *is_async {
                        return Err(FusionError::Syntax {
                            message: "async foreign functions are not supported by the C ABI subset".into(),
                            span: *span,
                        });
                    }
                    if abi != "C" {
                        return Err(FusionError::Syntax {
                            message: format!("Unsupported foreign ABI '{}'; only C is supported", abi),
                            span: *span,
                        });
                    }

                    let abi_parameters = parameters
                        .iter()
                        .enumerate()
                        .map(|(index, parameter)| {
                            let type_name = parameter.type_name.as_deref().ok_or_else(|| {
                                FusionError::Syntax {
                                    message: format!("Foreign parameter {} requires an explicit ABI type", index + 1),
                                    span: parameter.span,
                                }
                            })?;
                            crate::foreign::ForeignAbiType::from_type_name(type_name).ok_or_else(|| {
                                FusionError::Syntax {
                                    message: format!("Unsupported C ABI parameter type '{}' for '{}'", type_name, name),
                                    span: parameter.span,
                                }
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                    let return_name = return_type.as_deref().unwrap_or("void");
                    let abi_return = crate::foreign::ForeignAbiType::from_type_name(return_name)
                        .ok_or_else(|| FusionError::Syntax {
                            message: format!("Unsupported C ABI return type '{}' for '{}'", return_name, name),
                            span: *span,
                        })?;

                    let declaration = crate::foreign::ForeignFunctionDecl {
                        source_name: name.clone(),
                        link_name: link_name.clone().unwrap_or_else(|| name.clone()),
                        parameters: abi_parameters,
                        return_type: abi_return,
                        calling_convention: crate::foreign::ForeignCallingConvention::C,
                    };
                    declaration.validate().map_err(|message| FusionError::Syntax {
                        message,
                        span: *span,
                    })?;
                    self.environment.foreign_signatures.insert(name.clone(), declaration.clone());
                    self.environment.foreign_functions.insert(name.clone());
                }

                if self.environment.functions.contains_key(name) {
                    return Err(FusionError::Syntax {
                        message: format!("Duplicate function '{}'", name),
                        span: *span,
                    });
                }

                let mut generic_set = HashSet::new();

                for generic in generic_parameters {
                    if !generic_set.insert(generic.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!("Duplicate generic parameter '{}'", generic),
                            span: *span,
                        });
                    }
                }

                let mut parameter_types = Vec::new();
                let mut parameter_names = HashSet::new();

                for parameter in parameters {
                    if !parameter_names.insert(parameter.name.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!("Duplicate parameter '{}'", parameter.name),
                            span: parameter.span,
                        });
                    }

                    let parameter_type = match &parameter.type_name {
                        Some(type_name) => self.convert_type_with_generics(
                            type_name,
                            generic_parameters,
                            parameter.span,
                        )?,

                        None => Type::Unknown,
                    };

                    parameter_types.push(parameter_type);
                }

                // An omitted return type is initially `Unknown`, not `Void`.
                // The body checker infers the actual return type from `return`
                // statements.  A function with no return statements is later
                // normalized to `Void`.
                let base_return_type = match return_type {
                    Some(type_name) if foreign_abi.is_some() && type_name == "void" => Type::Void,
                    Some(type_name) => {
                        self.convert_type_with_generics(type_name, generic_parameters, *span)?
                    }
                    // An omitted foreign return annotation is the C `void` return,
                    // unlike an ordinary Fusion function whose return may be inferred.
                    None if foreign_abi.is_some() => Type::Void,
                    None => Type::Unknown,
                };

                let final_return_type = if *is_async {
                    Type::Task(Box::new(base_return_type))
                } else {
                    base_return_type
                };

                self.environment.functions.insert(
                    name.clone(),
                    FunctionType {
                        parameters: parameter_types,
                        return_type: final_return_type,
                        generic_parameters: generic_parameters.clone(),
                    },
                );
            }
        }

        // Pass 6: validate executable bodies and declarations.
        for statement in &program.statements {
            match statement {
                Statement::Function {
                    name,
                    generic_parameters,
                    parameters,
                    return_type,
                    body,
                    is_async,
                    foreign_abi,
                    span,
                    ..
                } => {
                    if foreign_abi.is_none() {
                        self.check_function_body(
                            name,
                            generic_parameters,
                            parameters,
                            return_type.as_ref(),
                            body,
                            *is_async,
                            *span,
                        )?;
                    }
                }

                Statement::Trait { methods, span, .. } => {
                    for method in methods {
                        self.validate_trait_method(method, &[])?;
                    }

                    let _ = span;
                }

                Statement::Impl {
                    trait_name,
                    type_name,
                    methods,
                    span,
                } => {
                    self.check_impl(trait_name.as_ref(), type_name, methods, *span)?;
                }

                Statement::Struct { .. } | Statement::Enum { .. } | Statement::Import { .. } => {}

                Statement::Main { body, .. } => {
                    self.push_scope();

                    let result = self.check_block(body);

                    self.pop_scope();

                    result?;
                }

                _ => {
                    self.check_statement(statement)?;
                }
            }
        }

        Ok(())
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod hir_nominal_identity_tests {
    use super::*;
    use crate::hir::{
        HirDefinitionId, HirDefinitionKind, HirTypeDefinition, HirTypeRegistry, HirTypeShape,
    };

    #[test]
    fn project_nominal_compatibility_uses_hir_identity_not_symbol_spelling() {
        let mut checker = TypeChecker::new();
        let geometry = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let graphics = HirDefinitionId::new("graphics", "Point", HirDefinitionKind::Struct);
        checker
            .environment
            .nominal_type_ids
            .insert("geometry_Point".into(), geometry.clone());
        checker
            .environment
            .nominal_type_ids
            .insert("graphics_Point".into(), graphics.clone());

        assert!(!checker.types_compatible(
            &Type::Struct("geometry_Point".into()),
            &Type::Struct("graphics_Point".into()),
        ));

        checker
            .environment
            .nominal_type_ids
            .insert("geometry_Point_alias".into(), geometry);
        assert!(checker.types_compatible(
            &Type::Struct("geometry_Point".into()),
            &Type::Struct("geometry_Point_alias".into()),
        ));
    }

    #[test]
    fn nested_nominal_compatibility_also_uses_hir_identity() {
        let mut checker = TypeChecker::new();
        checker.environment.nominal_type_ids.insert(
            "module_a_Item".into(),
            HirDefinitionId::new("module_a", "Item", HirDefinitionKind::Struct),
        );
        checker.environment.nominal_type_ids.insert(
            "module_b_Item".into(),
            HirDefinitionId::new("module_b", "Item", HirDefinitionKind::Struct),
        );

        assert!(!checker.types_compatible(
            &Type::Array(Box::new(Type::Struct("module_a_Item".into()))),
            &Type::Array(Box::new(Type::Struct("module_b_Item".into()))),
        ));
    }

    #[test]
    fn project_method_lookup_prefers_receiver_hir_identity() {
        let mut checker = TypeChecker::new();
        let geometry = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let graphics = HirDefinitionId::new("graphics", "Point", HirDefinitionKind::Struct);
        checker
            .environment
            .nominal_type_ids
            .insert("geometry_Point".into(), geometry.clone());
        checker
            .environment
            .nominal_type_ids
            .insert("graphics_Point".into(), graphics.clone());
        let geometry_sig = FunctionType {
            parameters: vec![Type::Struct("geometry_Point".into())],
            return_type: Type::Num,
            generic_parameters: Vec::new(),
        };
        let graphics_sig = FunctionType {
            parameters: vec![Type::Struct("graphics_Point".into())],
            return_type: Type::String,
            generic_parameters: Vec::new(),
        };
        checker.environment.methods.insert(
            ("geometry_Point".into(), "measure".into()),
            geometry_sig.clone(),
        );
        checker
            .environment
            .methods
            .insert(("graphics_Point".into(), "measure".into()), graphics_sig);
        checker
            .environment
            .methods_by_id
            .insert((geometry.clone(), "measure".into()), geometry_sig);

        assert_eq!(
            checker
                .method_signature("geometry_Point", "measure")
                .unwrap()
                .return_type,
            Type::Num
        );
        // A known project receiver must not fall back to the flat compatibility table.
        assert!(checker
            .method_signature("graphics_Point", "measure")
            .is_none());
    }

    #[test]
    fn project_trait_lookup_prefers_trait_hir_identity() {
        let mut checker = TypeChecker::new();
        let first = HirDefinitionId::new("collections", "Iterable", HirDefinitionKind::Trait);
        let second = HirDefinitionId::new("streams", "Iterable", HirDefinitionKind::Trait);
        checker
            .environment
            .trait_definition_ids
            .insert("collections_Iterable".into(), first.clone());
        checker
            .environment
            .trait_definition_ids
            .insert("streams_Iterable".into(), second.clone());
        checker
            .environment
            .traits
            .insert("collections_Iterable".into(), Vec::new());
        checker
            .environment
            .traits
            .insert("streams_Iterable".into(), Vec::new());
        checker.environment.traits_by_id.insert(
            first,
            vec![TraitMethod {
                name: "next".into(),
                parameters: Vec::new(),
                return_type: None,
                default_body: None,
                span: Span::default(),
            }],
        );
        checker.environment.traits_by_id.insert(second, Vec::new());

        assert_eq!(
            checker.trait_methods("collections_Iterable").unwrap().len(),
            1
        );
        assert!(checker
            .trait_methods("streams_Iterable")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn hir_aware_check_rejects_a_missing_registry_identity() {
        let mut checker = TypeChecker::new();
        let id = HirDefinitionId::new("models", "Point", HirDefinitionKind::Struct);
        let mut identities = HashMap::new();
        identities.insert("models_Point".into(), id);
        let registry = HirTypeRegistry::default();
        let program = Program {
            statements: Vec::new(),
            span: Span::default(),
            modules: Vec::new(),
        };

        assert!(checker
            .check_with_hir(&program, identities, &registry)
            .is_err());
    }

    #[test]
    fn hir_aware_check_accepts_registry_backed_identity() {
        let mut checker = TypeChecker::new();
        let id = HirDefinitionId::new("models", "Point", HirDefinitionKind::Struct);
        let mut identities = HashMap::new();
        identities.insert("models_Point".into(), id.clone());
        let mut registry = HirTypeRegistry::default();
        registry.insert(HirTypeDefinition {
            id,
            is_public: true,
            shape: HirTypeShape::Struct {
                fields: Vec::new(),
                derives: Vec::new(),
            },
        });
        let program = Program {
            statements: vec![Statement::Struct {
                name: "models_Point".into(),
                fields: Vec::new(),
                derives: Vec::new(),
                span: Span::default(),
            }],
            span: Span::default(),
            modules: Vec::new(),
        };

        checker
            .check_with_hir(&program, identities, &registry)
            .unwrap();
        assert_eq!(checker.environment.nominal_type_ids.len(), 1);
        assert!(checker
            .environment
            .structs_by_id
            .contains_key(&HirDefinitionId::new(
                "models",
                "Point",
                HirDefinitionKind::Struct
            )));
    }

    #[test]
    fn boolean_or_pattern_can_prove_exhaustiveness() {
        let mut checker = TypeChecker::new();
        let span = Span::new(0, 0);
        let pattern = Pattern {
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
        };
        let arms = vec![MatchArm {
            pattern,
            guard: None,
            body: vec![],
            span,
        }];
        assert!(checker
            .check_match(&Expression::Boolean { value: true, span }, &arms, span)
            .is_ok());
    }

    #[test]
    fn overlapping_integer_ranges_are_rejected_as_unreachable() {
        let mut checker = TypeChecker::new();
        let span = Span::new(0, 0);
        let arms = vec![
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Range {
                        start: 1,
                        end: 5,
                        inclusive: true,
                    },
                    span,
                },
                guard: None,
                body: vec![],
                span,
            },
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Number(3),
                    span,
                },
                guard: None,
                body: vec![],
                span,
            },
        ];
        let error = checker
            .check_match(&Expression::Number { value: 3, span }, &arms, span)
            .unwrap_err();
        assert!(error.to_string().contains("Overlapping integer"));
    }

    #[test]
    fn enum_variant_with_refutable_payload_does_not_cover_the_whole_variant() {
        let mut checker = TypeChecker::new();
        let span = Span::new(0, 0);
        checker.environment.enums.insert(
            "Choice".into(),
            EnumDefinition {
                variants: HashMap::from([
                    (
                        "Some".into(),
                        EnumVariantDefinition {
                            fields: vec![Type::Num],
                        },
                    ),
                    ("None".into(), EnumVariantDefinition { fields: vec![] }),
                ]),
            },
        );
        checker
            .declare_variable("choice".into(), Type::Enum("Choice".into()), false, span)
            .unwrap();
        let arms = vec![
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Variant {
                        name: "Choice::Some".into(),
                        bindings: vec![Pattern {
                            kind: PatternKind::Number(1),
                            span,
                        }],
                    },
                    span,
                },
                guard: None,
                body: vec![],
                span,
            },
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Variant {
                        name: "Choice::None".into(),
                        bindings: vec![],
                    },
                    span,
                },
                guard: None,
                body: vec![],
                span,
            },
        ];
        let error = checker
            .check_match(
                &Expression::Identifier {
                    name: "choice".into(),
                    span,
                },
                &arms,
                span,
            )
            .unwrap_err();
        assert!(error.to_string().contains("Non-exhaustive match"));
    }

    #[test]
    fn arms_after_an_unconditional_wildcard_are_rejected() {
        let mut checker = TypeChecker::new();
        let span = Span::new(0, 0);
        let arms = vec![
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Wildcard,
                    span,
                },
                guard: None,
                body: vec![],
                span,
            },
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Boolean(true),
                    span,
                },
                guard: None,
                body: vec![],
                span,
            },
        ];
        let error = checker
            .check_match(&Expression::Boolean { value: true, span }, &arms, span)
            .unwrap_err();
        assert!(error.to_string().contains("Unreachable match arm"));
    }

    #[test]
    fn match_with_guard_does_not_prove_function_return() {
        let mut checker = TypeChecker::new();
        let span = Span::new(0, 0);
        let return_stmt = || Statement::Return {
            value: Some(Expression::Number { value: 1, span }),
            span,
        };
        let statements = vec![Statement::Match {
            expression: Expression::Boolean { value: true, span },
            arms: vec![
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Boolean(true),
                        span,
                    },
                    guard: Some(Expression::Boolean { value: true, span }),
                    body: vec![return_stmt()],
                    span,
                },
                MatchArm {
                    pattern: Pattern {
                        kind: PatternKind::Boolean(false),
                        span,
                    },
                    guard: None,
                    body: vec![return_stmt()],
                    span,
                },
            ],
            span,
        }];
        assert!(!checker.block_returns(&statements));
    }
}


#[cfg(test)]
mod foreign_declaration_typecheck_tests {
    use super::*;
    use crate::lexer::{BlockMode, Lexer};
    use crate::parser::Parser;

    #[test]
    fn registers_a_valid_c_foreign_signature() {
        let source = "extern \"C\" {\n    fn fusion_helper(value: i32) -> i32;\n}\nmain:\n    print(1)\n";
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("source should lex");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("source should parse");

        let mut checker = TypeChecker::new();
        checker.check(&program).expect("foreign signature should type-check");

        let signature = checker.environment.functions.get("fusion_helper")
            .expect("foreign function should be registered");
        assert_eq!(signature.parameters, vec![Type::Generic("i32".into())]);
        assert_eq!(signature.return_type, Type::Generic("i32".into()));
    }

    #[test]
    fn rejects_void_foreign_parameters() {
        let source = "extern \"C\" {\n    fn invalid(value: void);\n}\nmain:\n    print(1)\n";
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("source should lex");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("source should parse");

        let mut checker = TypeChecker::new();
        assert!(checker.check(&program).is_err());
    }

    fn check_foreign_source(source: &str) -> Result<TypeEnvironment, String> {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().map_err(|error| error.to_string())?;
        let mut parser = Parser::new(tokens);
        let program = parser.parse().map_err(|error| error.to_string())?;
        let mut checker = TypeChecker::new();
        checker.check(&program).map_err(|error| error.to_string())?;
        Ok(checker.environment)
    }

    #[test]
    fn accepts_every_supported_c_abi_parameter_type() {
        let source = "extern \"C\" {\n    fn all_types(a: i8, b: u8, c: i16, d: u16, e: i32, f: u32, g: i64, h: u64, i: f32, j: f64, k: *opaque);\n}\nmain:\n    print(1)\n";
        check_foreign_source(source).expect("all supported C ABI parameter types should be accepted");
    }

    #[test]
    fn accepts_supported_c_abi_return_types_and_void() {
        let source = "extern \"C\" {\n    fn returns_void();\n    fn returns_i8() -> i8;\n    fn returns_u64() -> u64;\n    fn returns_f32() -> f32;\n    fn returns_f64() -> f64;\n    fn returns_handle() -> *opaque;\n}\nmain:\n    print(1)\n";
        let environment = check_foreign_source(source).expect("supported C ABI return types should be accepted");
        assert_eq!(environment.functions.get("returns_void").unwrap().return_type, Type::Void);
    }

    #[test]
    fn checks_integer_literals_against_exact_foreign_abi_widths() {
        for (ty, value) in [
            ("i8", "127"),
            ("i8", "-128"),
            ("u8", "255"),
            ("i16", "32767"),
            ("u16", "65535"),
            ("i32", "2147483647"),
            ("u32", "4294967295"),
            ("i64", "9223372036854775807"),
            ("u64", "18446744073709551615"),
        ] {
            let source = format!(
                "extern \"C\" {{\n    fn take(value: {});\n}}\nmain:\n    take({})\n",
                ty, value
            );
            check_foreign_source(&source)
                .unwrap_or_else(|error| panic!("{}({}) should be accepted: {}", ty, value, error));
        }
    }

    #[test]
    fn rejects_out_of_range_foreign_integer_literals() {
        for (ty, value) in [
            ("i8", "128"),
            ("i8", "-129"),
            ("u8", "256"),
            ("u8", "-1"),
            ("i16", "32768"),
            ("u16", "65536"),
            ("i32", "2147483648"),
            ("u32", "4294967296"),
            ("i64", "9223372036854775808"),
        ] {
            let source = format!(
                "extern \"C\" {{\n    fn take(value: {});\n}}\nmain:\n    take({})\n",
                ty, value
            );
            assert!(
                check_foreign_source(&source).is_err(),
                "{}({}) must be rejected as out of range",
                ty,
                value
            );
        }
    }

    #[test]
    fn rejects_untyped_numeric_values_at_foreign_boundaries() {
        let source = "extern \"C\" {\n    fn take(value: i8);\n}\nmain:\n    let value = 12\n    take(value)\n";
        let result = check_foreign_source(source);
        assert!(
            result.is_err(),
            "width-neutral Fusion num values must not silently narrow to i8"
        );
    }

    #[test]
    fn rejects_types_outside_the_initial_c_abi_subset() {
        for ty in ["i128", "u128", "isize", "usize", "string", "bool", "MyStruct"] {
            let source = format!("extern \"C\" {{\n    fn invalid(value: {});\n}}\nmain:\n    print(1)\n", ty);
            assert!(check_foreign_source(&source).is_err(), "type {} must be rejected", ty);
        }
    }

    #[test]
    fn rejects_generic_and_async_foreign_functions() {
        for declaration in ["fn generic<T>(value: i32) -> i32;", "async fn asynchronous(value: i32) -> i32;"] {
            let source = format!("extern \"C\" {{\n    {}\n}}\nmain:\n    print(1)\n", declaration);
            assert!(check_foreign_source(&source).is_err(), "declaration must be rejected: {}", declaration);
        }
    }

    #[test]
fn accepts_foreign_invocation_for_native_call_lowering() {
    let source = "extern \"C\" {\n    fn fusion_helper(value: i32) -> i32;\n}\nmain:\n    print(fusion_helper(21))\n";

    let mut lexer = Lexer::new(source, BlockMode::Unknown);
    let tokens = lexer.tokenize().expect("source should lex");
    let mut parser = Parser::new(tokens);
    let program = parser.parse().expect("source should parse");

    let mut checker = TypeChecker::new();
    checker
        .check(&program)
        .expect("valid foreign function invocation should pass type checking");
}

}

#[cfg(test)]
mod type_expression_conversion_tests {
    use super::*;
    use crate::lexer::{BlockMode, Lexer};
    use crate::parser::Parser;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn convert(name: &str) -> Result<Type, String> {
        TypeChecker::new()
            .convert_type(name, span())
            .map_err(|error| error.to_string())
    }

    fn convert_generic(name: &str, generics: &[&str]) -> Result<Type, String> {
        let generics: Vec<String> = generics.iter().map(|g| g.to_string()).collect();
        TypeChecker::new()
            .convert_type_with_generics(name, &generics, span())
            .map_err(|error| error.to_string())
    }

    fn parse(source: &str) -> Result<Program, String> {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().map_err(|error| error.to_string())?;
        Parser::new(tokens)
            .parse()
            .map_err(|error| error.to_string())
    }

    #[test]
    fn legacy_spellings_convert_exactly_as_before() {
        assert_eq!(convert("num"), Ok(Type::Num));
        assert_eq!(convert("num[]"), Ok(Type::Array(Box::new(Type::Num))));
        assert_eq!(convert("Array<num>"), Ok(Type::Array(Box::new(Type::Num))));
        assert_eq!(
            convert("Option<string>"),
            Ok(Type::Option(Box::new(Type::String)))
        );
        assert_eq!(
            convert("Result<num, string>"),
            Ok(Type::Result(Box::new(Type::Num), Box::new(Type::String)))
        );
        assert_eq!(
            convert("HashMap<string, num>"),
            Ok(Type::HashMap(Box::new(Type::String), Box::new(Type::Num)))
        );
        assert_eq!(
            convert("(num, string)"),
            Ok(Type::Tuple(vec![Type::Num, Type::String]))
        );
        assert_eq!(convert("i32"), Ok(Type::Generic("i32".into())));
        assert_eq!(convert("*opaque"), Ok(Type::Generic("*opaque".into())));
    }

    #[test]
    fn generics_and_arrays_now_nest_in_any_order() {
        assert_eq!(
            convert("Option<num>[]"),
            Ok(Type::Array(Box::new(Type::Option(Box::new(Type::Num)))))
        );
        assert_eq!(
            convert("num[][]"),
            Ok(Type::Array(Box::new(Type::Array(Box::new(Type::Num)))))
        );
        assert_eq!(
            convert("HashMap<string, num[]>[]"),
            Ok(Type::Array(Box::new(Type::HashMap(
                Box::new(Type::String),
                Box::new(Type::Array(Box::new(Type::Num)))
            ))))
        );
        assert_eq!(
            convert("(num, string)[]"),
            Ok(Type::Array(Box::new(Type::Tuple(vec![
                Type::Num,
                Type::String
            ]))))
        );
    }

    #[test]
    fn wrong_generic_arity_is_reported() {
        assert!(convert("HashMap<num>").unwrap_err().contains("HashMap requires two type arguments"));
        assert!(convert("Result<num>").unwrap_err().contains("Result requires two type arguments"));
        assert!(convert("Option<num, num>").unwrap_err().contains("Option requires one type argument"));
        assert!(convert("Widget<num>").unwrap_err().contains("Unknown type 'Widget<num>'"));
        assert!(convert("Widget").unwrap_err().contains("Unknown type 'Widget'"));
    }

    #[test]
    fn generic_parameters_resolve_at_any_depth_including_tuples() {
        assert_eq!(
            convert_generic("T[]", &["T"]),
            Ok(Type::Array(Box::new(Type::Generic("T".into()))))
        );
        assert_eq!(
            convert_generic("Option<T>[]", &["T"]),
            Ok(Type::Array(Box::new(Type::Option(Box::new(Type::Generic("T".into()))))))
        );
        // The old checker lost the generic scope inside tuples.
        assert_eq!(
            convert_generic("(T, num)", &["T"]),
            Ok(Type::Tuple(vec![Type::Generic("T".into()), Type::Num]))
        );
        assert!(convert("T").is_err());
    }

    #[test]
    fn self_is_only_accepted_where_generics_are_in_scope() {
        assert_eq!(convert_generic("Self", &[]), Ok(Type::Unknown));
        assert!(convert("Self").is_err());
    }

    #[test]
    fn function_types_parse_but_are_rejected_with_a_clear_message() {
        let program = parse("fn apply(f: fn(num) -> num, x: num) -> num:\n    return x\n\nmain:\n    print(1)\n")
            .expect("function type annotations should parse");
        match &program.statements[0] {
            Statement::Function { parameters, .. } => {
                assert_eq!(
                    parameters[0].type_name.as_deref(),
                    Some("fn(num) -> num")
                );
            }
            other => panic!("expected a function, found {other:?}"),
        }
        let error = TypeChecker::new()
            .check(&program)
            .expect_err("function types need closures, which do not exist yet")
            .to_string();
        assert!(error.contains("not supported yet"), "{error}");
        assert!(convert("fn(num) -> num").unwrap_err().contains("not supported yet"));
    }

    #[test]
    fn parser_accepts_nested_array_and_generic_annotations() {
        let program = parse(
            "fn grid(rows: num[][], tags: Option<string>[]) -> HashMap<string, num>[]:\n    return 0\n\nmain:\n    print(1)\n",
        )
        .expect("nested annotations should parse");
        match &program.statements[0] {
            Statement::Function {
                parameters,
                return_type,
                ..
            } => {
                assert_eq!(parameters[0].type_name.as_deref(), Some("num[][]"));
                assert_eq!(parameters[1].type_name.as_deref(), Some("Option<string>[]"));
                assert_eq!(return_type.as_deref(), Some("HashMap<string, num>[]"));
            }
            other => panic!("expected a function, found {other:?}"),
        }
    }

    #[test]
    fn malformed_annotations_are_syntax_errors() {
        assert!(parse("fn f(x: Option<num) -> num:\n    return 1\n").is_err());
        assert!(parse("fn f(x: num[) -> num:\n    return 1\n").is_err());
        assert!(parse("fn f(x: (num)) -> num:\n    return 1\n").is_err());
    }
}
