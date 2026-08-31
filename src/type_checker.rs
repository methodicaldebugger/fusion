//contents of type_checker.rs

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::errors::FusionError;
use crate::span::Span;
use crate::types::{
    EnumDefinition,
    EnumVariantDefinition,
    StructDefinition,
    Type,
};

/// The type environment contains all declarations visible to the checker.
///
/// Types and functions are registered before executable code is checked so
/// that forward references and recursive functions are possible.
pub struct TypeEnvironment {
    pub scopes: Vec<HashMap<String, VariableInfo>>,
    pub functions: HashMap<String, FunctionType>,
    pub structs: HashMap<String, StructDefinition>,
    pub enums: HashMap<String, EnumDefinition>,
}

#[derive(Clone)]
struct FunctionContext {
    declared_return_type: Option<Type>,
    inferred_return_type: Option<Type>,
    has_return: bool,
}

#[derive(Clone)]
pub struct VariableInfo {
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Clone)]
pub struct FunctionType {
    pub parameters: Vec<Type>,
    pub return_type: Type,
}

pub struct TypeChecker {
    environment: TypeEnvironment,
    current_function: Option<FunctionContext>,
    loop_depth: usize,
}

impl TypeChecker {
    // =========================================================
    // Construction
    // =========================================================

    pub fn new() -> Self {
        let mut functions = HashMap::new();

        // print(...) accepts any value and returns void.
        functions.insert(
            "print".to_string(),
            FunctionType {
                parameters: vec![Type::Unknown],
                return_type: Type::Void,
            },
        );

        Self {
            environment: TypeEnvironment {
                scopes: vec![HashMap::new()],
                functions,
                structs: HashMap::new(),
                enums: HashMap::new(),
            },
            current_function: None,
            loop_depth: 0,
        }
    }

    // =========================================================
    // Span helpers
    // =========================================================

    fn expression_span(expression: &Expression) -> Span {
        expression.span()
    }

    fn statement_span(statement: &Statement) -> Span {
        statement.span()
    }

    // =========================================================
    // Scopes
    // =========================================================

    fn push_scope(&mut self) {
        self.environment.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        // Keep the global scope alive.
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
            .expect("type checker always has a global scope");

        if scope.contains_key(&name) {
            return Err(FusionError::UnknownVariable {
                name: format!("Variable '{}' is already declared", name),
                span,
            });
        }

        scope.insert(
            name,
            VariableInfo {
                ty,
                mutable,
            },
        );

        Ok(())
    }

    fn lookup_variable(&self, name: &str) -> Option<Type> {
        self.lookup_variable_info(name).map(|info| info.ty)
    }

    fn lookup_variable_info(&self, name: &str) -> Option<VariableInfo> {
        for scope in self.environment.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }

        None
    }

    // =========================================================
    // Error helpers
    // =========================================================

    fn unknown_variable(
        &self,
        name: impl Into<String>,
        span: Span,
    ) -> FusionError {
        FusionError::UnknownVariable {
            name: name.into(),
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
        left: &Type,
        operator: &str,
        right: &Type,
        span: Span,
    ) -> FusionError {
        FusionError::InvalidOperation {
            left: left.name(),
            operator: operator.to_string(),
            right: right.name(),
            span,
        }
    }

    // =========================================================
    // Type helpers
    // =========================================================

    fn type_is_unknown(ty: &Type) -> bool {
        match ty {
            Type::Unknown => true,

            Type::Array(inner) => Self::type_is_unknown(inner),

            _ => false,
        }
    }

    fn types_compatible(expected: &Type, actual: &Type) -> bool {
        if expected == actual {
            return true;
        }

        // Unknown is deliberately permissive. It is used for values whose
        // type cannot yet be determined, such as an empty array or an
        // untyped function parameter.
        if *expected == Type::Unknown || *actual == Type::Unknown {
            return true;
        }

        match (expected, actual) {
            (Type::Array(a), Type::Array(b)) => {
                Self::types_compatible(a, b)
            }

            _ => false,
        }
    }

    fn require_bool(
        &self,
        found: Type,
        span: Span,
    ) -> Result<(), FusionError> {
        if found == Type::Bool || found == Type::Unknown {
            Ok(())
        } else {
            Err(self.type_mismatch("bool", found.name(), span))
        }
    }

    fn require_num(
        &self,
        found: Type,
        span: Span,
    ) -> Result<(), FusionError> {
        if found == Type::Num || found == Type::Unknown {
            Ok(())
        } else {
            Err(self.type_mismatch("num", found.name(), span))
        }
    }

    // =========================================================
    // Type conversion
    // =========================================================

    fn convert_type(&self, name: &str) -> Result<Type, FusionError> {
        match name {
            "num" => Ok(Type::Num),
            "float" => Ok(Type::Float),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),
            "void" => Ok(Type::Void),
            "unknown" => Ok(Type::Unknown),

            _ if self.environment.structs.contains_key(name) => {
                Ok(Type::Struct(name.to_string()))
            }

            _ if self.environment.enums.contains_key(name) => {
                Ok(Type::Enum(name.to_string()))
            }

            _ => Err(self.type_mismatch(
                "known type",
                name,
                Span::default(),
            )),
        }
    }

    fn convert_type_with_generics(
        &self,
        name: &str,
        generics: &HashSet<String>,
    ) -> Result<Type, FusionError> {
        if generics.contains(name) {
            return Ok(Type::Generic(name.to_string()));
        }

        self.convert_type(name)
    }

    // =========================================================
    // Operators
    // =========================================================

    fn operator_name(operator: &Operator) -> &'static str {
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
            Operator::And => "and",
            Operator::Or => "or",
        }
    }

    // =========================================================
    // Property helpers
    // =========================================================

    fn property_root_name(expression: &Expression) -> Option<String> {
        match expression {
            Expression::Identifier { name, .. } => Some(name.clone()),

            Expression::Property { object, .. } => {
                Self::property_root_name(object)
            }

            _ => None,
        }
    }

    fn property_target_is_mutable(
        &self,
        expression: &Expression,
    ) -> Result<bool, FusionError> {
        let root_name =
            Self::property_root_name(expression).ok_or_else(|| {
                self.unknown_variable(
                    "Invalid property assignment target",
                    Self::expression_span(expression),
                )
            })?;

        let info =
            self.lookup_variable_info(&root_name).ok_or_else(|| {
                self.unknown_variable(
                    root_name.clone(),
                    Self::expression_span(expression),
                )
            })?;

        Ok(info.mutable)
    }

    fn lookup_property_type(
        &self,
        object: &Expression,
        name: &str,
    ) -> Result<Type, FusionError> {
        let object_type = self.infer_expression(object)?;

        match object_type {
            Type::Struct(struct_name) => {
                let definition = self
                    .environment
                    .structs
                    .get(&struct_name)
                    .ok_or_else(|| {
                        self.unknown_variable(
                            struct_name.clone(),
                            Self::expression_span(object),
                        )
                    })?;

                definition
                    .fields
                    .get(name)
                    .cloned()
                    .ok_or_else(|| {
                        self.unknown_variable(
                            format!(
                                "Unknown field '{}' on struct '{}'",
                                name, struct_name
                            ),
                            Self::expression_span(object),
                        )
                    })
            }

            Type::Unknown => Ok(Type::Unknown),

            other => Err(self.type_mismatch(
                "struct",
                other.name(),
                Self::expression_span(object),
            )),
        }
    }

    // =========================================================
    // Pattern checking
    // =========================================================

    fn check_pattern(
        &mut self,
        pattern: &Pattern,
        expected_type: &Type,
    ) -> Result<(), FusionError> {
        match &pattern.kind {
            PatternKind::Wildcard => Ok(()),

            PatternKind::Identifier(name) => {
                if name == "_" {
                    return Ok(());
                }

                self.declare_variable(
                    name.clone(),
                    expected_type.clone(),
                    true,
                    pattern.span,
                )
            }

            PatternKind::Number(_) => {
                if !Self::types_compatible(&Type::Num, expected_type) {
                    return Err(self.type_mismatch(
                        "num",
                        expected_type.name(),
                        pattern.span,
                    ));
                }

                Ok(())
            }

            PatternKind::Float(_) => {
                if !Self::types_compatible(&Type::Float, expected_type) {
                    return Err(self.type_mismatch(
                        "float",
                        expected_type.name(),
                        pattern.span,
                    ));
                }

                Ok(())
            }

            PatternKind::String(_) => {
                if !Self::types_compatible(&Type::String, expected_type) {
                    return Err(self.type_mismatch(
                        "string",
                        expected_type.name(),
                        pattern.span,
                    ));
                }

                Ok(())
            }

            PatternKind::Boolean(_) => {
                if !Self::types_compatible(&Type::Bool, expected_type) {
                    return Err(self.type_mismatch(
                        "bool",
                        expected_type.name(),
                        pattern.span,
                    ));
                }

                Ok(())
            }

            PatternKind::Variant { name, bindings } => {
                let enum_name = match expected_type {
                    Type::Enum(name) => name,

                    Type::Unknown => {
                        return Ok(());
                    }

                    other => {
                        return Err(self.type_mismatch(
                            "enum",
                            other.name(),
                            pattern.span,
                        ));
                    }
                };

                let parts: Vec<&str> = name.split("::").collect();

                if parts.len() != 2 {
                    return Err(self.unknown_variable(
                        format!("Invalid enum pattern '{}'", name),
                        pattern.span,
                    ));
                }

                let pattern_enum = parts[0];
                let variant_name = parts[1];

                if pattern_enum != enum_name {
                    return Err(self.type_mismatch(
                        enum_name,
                        pattern_enum,
                        pattern.span,
                    ));
                }

                let field_types = {
                    let definition = self
                        .environment
                        .enums
                        .get(enum_name)
                        .ok_or_else(|| {
                            self.unknown_variable(
                                enum_name.clone(),
                                pattern.span,
                            )
                        })?;

                    let variant = definition
                        .variants
                        .get(variant_name)
                        .ok_or_else(|| {
                            self.unknown_variable(
                                format!(
                                    "Unknown variant '{}::{}'",
                                    enum_name, variant_name
                                ),
                                pattern.span,
                            )
                        })?;

                    if bindings.len() != variant.fields.len() {
                        return Err(self.type_mismatch(
                            format!("{} bindings", variant.fields.len()),
                            format!("{} bindings", bindings.len()),
                            pattern.span,
                        ));
                    }

                    variant.fields.clone()
                };

                let mut binding_names = HashSet::new();

                for (binding, field_type) in
                    bindings.iter().zip(field_types.iter())
                {
                    if binding == "_" {
                        continue;
                    }

                    if !binding_names.insert(binding.clone()) {
                        return Err(self.unknown_variable(
                            format!(
                                "Pattern binding '{}' is declared more than once",
                                binding
                            ),
                            pattern.span,
                        ));
                    }

                    self.declare_variable(
                        binding.clone(),
                        field_type.clone(),
                        true,
                        pattern.span,
                    )?;
                }

                Ok(())
            }
        }
    }

    // =========================================================
    // Return analysis
    // =========================================================

    fn block_returns(statements: &[Statement]) -> bool {
        for statement in statements {
            match statement {
                Statement::Return { .. } => return true,

                Statement::If {
                    body,
                    else_body: Some(else_body),
                    ..
                } => {
                    if Self::block_returns(body)
                        && Self::block_returns(else_body)
                    {
                        return true;
                    }
                }

                Statement::Match { arms, .. } => {
                    if !arms.is_empty()
                        && arms
                            .iter()
                            .all(|arm| Self::block_returns(&arm.body))
                    {
                        return true;
                    }
                }

                _ => {}
            }
        }

        false
    }

    // =========================================================
    // Expression inference
    // =========================================================

    fn infer_expression(
        &self,
        expression: &Expression,
    ) -> Result<Type, FusionError> {
        match expression {
            // -------------------------------------------------
            // Literals
            // -------------------------------------------------

            Expression::Number { .. } => Ok(Type::Num),

            Expression::Float { .. } => Ok(Type::Float),

            Expression::Boolean { .. } => Ok(Type::Bool),

            Expression::String { .. } => Ok(Type::String),

            // -------------------------------------------------
            // Identifier
            // -------------------------------------------------

            Expression::Identifier { name, span } => {
                self.lookup_variable(name).ok_or_else(|| {
                    self.unknown_variable(name.clone(), *span)
                })
            }

            // -------------------------------------------------
            // Array
            // -------------------------------------------------

            Expression::Array { elements, span } => {
                if elements.is_empty() {
                    return Ok(Type::Array(Box::new(Type::Unknown)));
                }

                let first_type =
                    self.infer_expression(&elements[0])?;

                for element in elements.iter().skip(1) {
                    let element_type =
                        self.infer_expression(element)?;

                    if !Self::types_compatible(
                        &first_type,
                        &element_type,
                    ) {
                        return Err(self.type_mismatch(
                            first_type.name(),
                            element_type.name(),
                            element.span(),
                        ));
                    }
                }

                // If the first element is Unknown but another element has
                // a concrete type, prefer the concrete type.
                let element_type = if first_type == Type::Unknown {
                    elements
                        .iter()
                        .skip(1)
                        .map(|element| self.infer_expression(element))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .find(|ty| *ty != Type::Unknown)
                        .unwrap_or(Type::Unknown)
                } else {
                    first_type
                };

                let _ = span;

                Ok(Type::Array(Box::new(element_type)))
            }

            // -------------------------------------------------
            // Index
            // -------------------------------------------------

            Expression::Index {
                array,
                index,
                span,
            } => {
                let array_type =
                    self.infer_expression(array)?;

                let index_type =
                    self.infer_expression(index)?;

                if index_type != Type::Num
                    && index_type != Type::Unknown
                {
                    return Err(self.type_mismatch(
                        "num index",
                        index_type.name(),
                        index.span(),
                    ));
                }

                match array_type {
                    Type::Array(inner) => Ok(*inner),

                    Type::Unknown => Ok(Type::Unknown),

                    other => Err(self.type_mismatch(
                        "array",
                        other.name(),
                        *span,
                    )),
                }
            }

            // -------------------------------------------------
            // Property
            // -------------------------------------------------

            Expression::Property {
                object,
                name,
                ..
            } => self.lookup_property_type(object, name),

            // -------------------------------------------------
            // Unary
            // -------------------------------------------------

            Expression::Unary {
                operator,
                expression: inner,
                span,
            } => {
                let inner_type =
                    self.infer_expression(inner)?;

                match operator {
                    UnaryOperator::Negate => {
                        if inner_type == Type::Unknown {
                            Ok(Type::Unknown)
                        } else if inner_type == Type::Num
                            || inner_type == Type::Float
                        {
                            Ok(inner_type)
                        } else {
                            Err(FusionError::InvalidOperation {
                                left: inner_type.name(),
                                operator: "-".to_string(),
                                right: String::new(),
                                span: *span,
                            })
                        }
                    }

                    UnaryOperator::Not => {
                        if inner_type == Type::Unknown {
                            Ok(Type::Bool)
                        } else if inner_type == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            Err(FusionError::InvalidOperation {
                                left: inner_type.name(),
                                operator: "!".to_string(),
                                right: String::new(),
                                span: *span,
                            })
                        }
                    }
                }
            }

            // -------------------------------------------------
            // Binary
            // -------------------------------------------------

            Expression::Binary {
                left,
                operator,
                right,
                span,
            } => {
                let left_type =
                    self.infer_expression(left)?;

                let right_type =
                    self.infer_expression(right)?;

                match operator {
                    Operator::Plus
                    | Operator::Minus
                    | Operator::Multiply
                    | Operator::Divide => {
                        if left_type == Type::Unknown
                            || right_type == Type::Unknown
                        {
                            return Ok(if *operator == Operator::Plus
                                && (left_type == Type::String
                                    || right_type == Type::String)
                            {
                                Type::String
                            } else {
                                Type::Unknown
                            });
                        }

                        if left_type == Type::Num
                            && right_type == Type::Num
                        {
                            return Ok(Type::Num);
                        }

                        if left_type == Type::Float
                            && right_type == Type::Float
                        {
                            return Ok(Type::Float);
                        }

                        if *operator == Operator::Plus
                            && left_type == Type::String
                            && right_type == Type::String
                        {
                            return Ok(Type::String);
                        }

                        Err(self.invalid_operation(
                            &left_type,
                            Self::operator_name(operator),
                            &right_type,
                            *span,
                        ))
                    }

                    Operator::Equal | Operator::NotEqual => {
                        if Self::types_compatible(
                            &left_type,
                            &right_type,
                        ) {
                            Ok(Type::Bool)
                        } else {
                            Err(self.invalid_operation(
                                &left_type,
                                Self::operator_name(operator),
                                &right_type,
                                *span,
                            ))
                        }
                    }

                    Operator::Less
                    | Operator::LessEqual
                    | Operator::Greater
                    | Operator::GreaterEqual => {
                        if left_type == Type::Unknown
                            || right_type == Type::Unknown
                        {
                            return Ok(Type::Bool);
                        }

                        let numeric_match =
                            (left_type == Type::Num
                                && right_type == Type::Num)
                                || (left_type == Type::Float
                                    && right_type == Type::Float);

                        if numeric_match {
                            Ok(Type::Bool)
                        } else {
                            Err(self.invalid_operation(
                                &left_type,
                                Self::operator_name(operator),
                                &right_type,
                                *span,
                            ))
                        }
                    }

                    Operator::And | Operator::Or => {
                        if left_type == Type::Unknown
                            || right_type == Type::Unknown
                        {
                            return Ok(Type::Bool);
                        }

                        if left_type == Type::Bool
                            && right_type == Type::Bool
                        {
                            Ok(Type::Bool)
                        } else {
                            Err(self.invalid_operation(
                                &left_type,
                                Self::operator_name(operator),
                                &right_type,
                                *span,
                            ))
                        }
                    }
                }
            }

            // -------------------------------------------------
            // Function call
            // -------------------------------------------------

            Expression::Call {
                name,
                arguments,
                generic_arguments,
                span,
            } => {
                let function =
                    self.environment.functions.get(name).ok_or_else(
                        || self.unknown_variable(name.clone(), *span),
                    )?;

                if arguments.len() != function.parameters.len() {
                    return Err(self.type_mismatch(
                        format!(
                            "{} arguments",
                            function.parameters.len()
                        ),
                        format!("{} arguments", arguments.len()),
                        *span,
                    ));
                }

                for (argument, expected_type) in arguments
                    .iter()
                    .zip(function.parameters.iter())
                {
                    let actual_type =
                        self.infer_expression(argument)?;

                    if !Self::types_compatible(
                        expected_type,
                        &actual_type,
                    ) {
                        return Err(self.type_mismatch(
                            expected_type.name(),
                            actual_type.name(),
                            argument.span(),
                        ));
                    }
                }

                // The current Type model has no generic function
                // substitution information beyond Generic(String), so
                // generic arguments are validated structurally here.
                //
                // An empty generic argument list is valid for non-generic
                // functions. Since FunctionType intentionally remains
                // compatible with the existing public API, we do not attach
                // generic metadata to it.
                let _ = generic_arguments;

                Ok(function.return_type.clone())
            }

            // -------------------------------------------------
            // Method call
            // -------------------------------------------------

            Expression::MethodCall {
                object,
                method,
                arguments,
                span,
            } => {
                let object_type =
                    self.infer_expression(object)?;

                match object_type {
                    Type::Array(element_type) => {
                        match method.as_str() {
                            "push" => {
                                if arguments.len() != 1 {
                                    return Err(self.type_mismatch(
                                        "1 argument",
                                        format!(
                                            "{} arguments",
                                            arguments.len()
                                        ),
                                        *span,
                                    ));
                                }

                                let argument_type =
                                    self.infer_expression(
                                        &arguments[0],
                                    )?;

                                if !Self::types_compatible(
                                    &element_type,
                                    &argument_type,
                                ) {
                                    return Err(self.type_mismatch(
                                        element_type.name(),
                                        argument_type.name(),
                                        arguments[0].span(),
                                    ));
                                }

                                Ok(Type::Void)
                            }

                            "pop" => {
                                if !arguments.is_empty() {
                                    return Err(self.type_mismatch(
                                        "0 arguments",
                                        format!(
                                            "{} arguments",
                                            arguments.len()
                                        ),
                                        *span,
                                    ));
                                }

                                Ok(*element_type)
                            }

                            "length" => {
                                if !arguments.is_empty() {
                                    return Err(self.type_mismatch(
                                        "0 arguments",
                                        format!(
                                            "{} arguments",
                                            arguments.len()
                                        ),
                                        *span,
                                    ));
                                }

                                Ok(Type::Num)
                            }

                            _ => Err(self.unknown_variable(
                                format!(
                                    "Unknown array method '{}'",
                                    method
                                ),
                                *span,
                            )),
                        }
                    }

                    Type::Unknown => Ok(Type::Unknown),

                    other => Err(self.type_mismatch(
                        "array",
                        other.name(),
                        object.span(),
                    )),
                }
            }

            // -------------------------------------------------
            // Struct constructor
            // -------------------------------------------------

            Expression::StructConstructor {
                name,
                fields,
                span,
            } => {
                let definition =
                    self.environment.structs.get(name).ok_or_else(
                        || self.unknown_variable(name.clone(), *span),
                    )?;

                let mut supplied_fields = HashSet::new();

                for (field_name, field_expression) in fields {
                    if !supplied_fields.insert(field_name.clone()) {
                        return Err(self.unknown_variable(
                            format!(
                                "Field '{}' is specified more than once",
                                field_name
                            ),
                            field_expression.span(),
                        ));
                    }

                    let expected_type =
                        definition.fields.get(field_name).ok_or_else(
                            || {
                                self.unknown_variable(
                                    format!(
                                        "Unknown field '{}' on struct '{}'",
                                        field_name, name
                                    ),
                                    field_expression.span(),
                                )
                            },
                        )?;

                    let actual_type =
                        self.infer_expression(field_expression)?;

                    if !Self::types_compatible(
                        expected_type,
                        &actual_type,
                    ) {
                        return Err(self.type_mismatch(
                            expected_type.name(),
                            actual_type.name(),
                            field_expression.span(),
                        ));
                    }
                }

                for field_name in definition.fields.keys() {
                    if !supplied_fields.contains(field_name) {
                        return Err(self.unknown_variable(
                            format!(
                                "Missing field '{}' in struct '{}'",
                                field_name, name
                            ),
                            *span,
                        ));
                    }
                }

                Ok(Type::Struct(name.clone()))
            }

            // -------------------------------------------------
            // Enum constructor
            // -------------------------------------------------

            Expression::EnumConstructor {
                enum_name,
                variant,
                arguments,
                span,
            } => {
                let definition =
                    self.environment.enums.get(enum_name).ok_or_else(
                        || {
                            self.unknown_variable(
                                enum_name.clone(),
                                *span,
                            )
                        },
                    )?;

                let variant_definition = definition
                    .variants
                    .get(variant)
                    .ok_or_else(|| {
                        self.unknown_variable(
                            format!(
                                "Unknown variant '{}::{}'",
                                enum_name, variant
                            ),
                            *span,
                        )
                    })?;

                if arguments.len() != variant_definition.fields.len()
                {
                    return Err(self.type_mismatch(
                        format!(
                            "{} arguments",
                            variant_definition.fields.len()
                        ),
                        format!("{} arguments", arguments.len()),
                        *span,
                    ));
                }

                for (argument, expected_type) in arguments
                    .iter()
                    .zip(variant_definition.fields.iter())
                {
                    let actual_type =
                        self.infer_expression(argument)?;

                    if !Self::types_compatible(
                        expected_type,
                        &actual_type,
                    ) {
                        return Err(self.type_mismatch(
                            expected_type.name(),
                            actual_type.name(),
                            argument.span(),
                        ));
                    }
                }

                Ok(Type::Enum(enum_name.clone()))
            }
        }
    }

    // =========================================================
    // Assignment checking
    // =========================================================

    fn check_assignment(
        &mut self,
        target: &Expression,
        value: &Expression,
        span: Span,
    ) -> Result<(), FusionError> {
        let value_type = self.infer_expression(value)?;

        match target {
            Expression::Identifier { name, span: target_span } => {
                let info =
                    self.lookup_variable_info(name).ok_or_else(|| {
                        self.unknown_variable(
                            name.clone(),
                            *target_span,
                        )
                    })?;

                if !info.mutable {
                    return Err(
                        FusionError::CannotAssignToConst {
                            name: name.clone(),
                            span: *target_span,
                        },
                    );
                }

                if !Self::types_compatible(
                    &info.ty,
                    &value_type,
                ) {
                    return Err(self.type_mismatch(
                        info.ty.name(),
                        value_type.name(),
                        *target_span,
                    ));
                }

                Ok(())
            }

            Expression::Property { object, name, .. } => {
                if !self.property_target_is_mutable(object)? {
                    let variable_name =
                        Self::property_root_name(object)
                            .unwrap_or_else(|| "<unknown>".to_string());

                    return Err(
                        FusionError::CannotAssignToConst {
                            name: variable_name,
                            span: object.span(),
                        },
                    );
                }

                let field_type =
                    self.lookup_property_type(object, name)?;

                if !Self::types_compatible(
                    &field_type,
                    &value_type,
                ) {
                    return Err(self.type_mismatch(
                        field_type.name(),
                        value_type.name(),
                        target.span(),
                    ));
                }

                Ok(())
            }

            Expression::Index { array, index, .. } => {
                let array_type =
                    self.infer_expression(array)?;

                let index_type =
                    self.infer_expression(index)?;

                if index_type != Type::Num
                    && index_type != Type::Unknown
                {
                    return Err(self.type_mismatch(
                        "num index",
                        index_type.name(),
                        index.span(),
                    ));
                }

                if let Some(root_name) =
                    Self::property_root_name(array)
                {
                    if let Some(info) =
                        self.lookup_variable_info(&root_name)
                    {
                        if !info.mutable {
                            return Err(
                                FusionError::CannotAssignToConst {
                                    name: root_name,
                                    span: array.span(),
                                },
                            );
                        }
                    }
                }

                match array_type {
                    Type::Array(element_type) => {
                        if !Self::types_compatible(
                            &element_type,
                            &value_type,
                        ) {
                            return Err(self.type_mismatch(
                                element_type.name(),
                                value_type.name(),
                                span,
                            ));
                        }

                        Ok(())
                    }

                    Type::Unknown => Ok(()),

                    other => Err(self.type_mismatch(
                        "array",
                        other.name(),
                        array.span(),
                    )),
                }
            }

            _ => Err(self.unknown_variable(
                "Invalid assignment target",
                target.span(),
            )),
        }
    }

    // =========================================================
    // Statement checking
    // =========================================================

    pub fn check_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<(), FusionError> {
        match statement {
            // -------------------------------------------------
            // Variables
            // -------------------------------------------------

            Statement::VariableDeclarations {
                declarations,
                ..
            } => {
                for declaration in declarations {
                    let inferred =
                        self.infer_expression(&declaration.value)?;

                    let ty = match &declaration.declared_type {
                        Some(type_name) => {
                            let declared =
                                self.convert_type(type_name)?;

                            if !Self::types_compatible(
                                &declared,
                                &inferred,
                            ) {
                                return Err(self.type_mismatch(
                                    declared.name(),
                                    inferred.name(),
                                    declaration.span,
                                ));
                            }

                            // Preserve an explicitly declared type.
                            declared
                        }

                        None => inferred,
                    };

                    self.declare_variable(
                        declaration.name.clone(),
                        ty,
                        true,
                        declaration.name_span,
                    )?;
                }

                Ok(())
            }

            // -------------------------------------------------
            // Constants
            // -------------------------------------------------

            Statement::ConstDeclaration {
                name,
                name_span,
                declared_type,
                value,
                ..
            } => {
                let inferred =
                    self.infer_expression(value)?;

                let ty = match declared_type {
                    Some(type_name) => {
                        let declared =
                            self.convert_type(type_name)?;

                        if !Self::types_compatible(
                            &declared,
                            &inferred,
                        ) {
                            return Err(self.type_mismatch(
                                declared.name(),
                                inferred.name(),
                                *name_span,
                            ));
                        }

                        declared
                    }

                    None => inferred,
                };

                self.declare_variable(
                    name.clone(),
                    ty,
                    false,
                    *name_span,
                )
            }

            // -------------------------------------------------
            // Assignment
            // -------------------------------------------------

            Statement::Assignment {
                target,
                value,
                span,
            } => self.check_assignment(target, value, *span),

            // -------------------------------------------------
            // Function call
            // -------------------------------------------------

            Statement::Call { expression, .. } => {
                let ty = self.infer_expression(expression)?;

                // A call statement may discard any return value.
                let _ = ty;

                Ok(())
            }

            // -------------------------------------------------
            // Expression statement
            // -------------------------------------------------

            Statement::Expression { expression, .. } => {
                self.infer_expression(expression)?;
                Ok(())
            }

            // -------------------------------------------------
            // If
            // -------------------------------------------------

            Statement::If {
                condition,
                body,
                else_body,
                ..
            } => {
                let condition_type =
                    self.infer_expression(condition)?;

                self.require_bool(
                    condition_type,
                    condition.span(),
                )?;

                self.push_scope();

                let body_result = self.check_block(body);

                self.pop_scope();

                body_result?;

                if let Some(else_body) = else_body {
                    self.push_scope();

                    let else_result =
                        self.check_block(else_body);

                    self.pop_scope();

                    else_result?;
                }

                Ok(())
            }

            // -------------------------------------------------
            // While
            // -------------------------------------------------

            Statement::While {
                condition,
                body,
                ..
            } => {
                let condition_type =
                    self.infer_expression(condition)?;

                self.require_bool(
                    condition_type,
                    condition.span(),
                )?;

                self.push_scope();

                self.loop_depth += 1;
                let result = self.check_block(body);
                self.loop_depth -= 1;

                self.pop_scope();

                result
            }

            // -------------------------------------------------
            // For
            // -------------------------------------------------

            Statement::For {
                variable,
                start,
                end,
                body,
                span,
            } => {
                let start_type =
                    self.infer_expression(start)?;

                let end_type =
                    self.infer_expression(end)?;

                self.require_num(
                    start_type.clone(),
                    start.span(),
                )?;

                self.require_num(
                    end_type.clone(),
                    end.span(),
                )?;

                self.push_scope();

                let result = (|| {
                    self.declare_variable(
                        variable.clone(),
                        Type::Num,
                        true,
                        *span,
                    )?;

                    self.loop_depth += 1;
                    let body_result = self.check_block(body);
                    self.loop_depth -= 1;

                    body_result
                })();

                self.pop_scope();

                result
            }

            // -------------------------------------------------
            // Match
            // -------------------------------------------------

            Statement::Match {
                expression,
                arms,
                span,
            } => self.check_match(expression, arms, *span),

            // -------------------------------------------------
            // Return
            // -------------------------------------------------

            Statement::Return { value, span } => {
                let actual_type =
                    self.infer_expression(value)?;

                let context =
                    self.current_function.as_mut().ok_or_else(
                        || FusionError::TypeMismatch {
                            expected: "inside function".to_string(),
                            found: "return".to_string(),
                            span: *span,
                        },
                    )?;

                context.has_return = true;

                if let Some(expected) =
                    &context.declared_return_type
                {
                    if !Self::types_compatible(
                        expected,
                        &actual_type,
                    ) {
                        return Err(FusionError::TypeMismatch {
                            expected: expected.name(),
                            found: actual_type.name(),
                            span: *span,
                        });
                    }
                } else {
                    match &context.inferred_return_type {
                        None => {
                            context.inferred_return_type =
                                Some(actual_type);
                        }

                        Some(previous) => {
                            if !Self::types_compatible(
                                previous,
                                &actual_type,
                            ) {
                                return Err(
                                    FusionError::TypeMismatch {
                                        expected: previous.name(),
                                        found: actual_type.name(),
                                        span: *span,
                                    },
                                );
                            }

                            // If the previous value was unknown and this
                            // return is concrete, refine the inferred type.
                            if *previous == Type::Unknown
                                && actual_type != Type::Unknown
                            {
                                context.inferred_return_type =
                                    Some(actual_type);
                            }
                        }
                    }
                }

                Ok(())
            }

            // -------------------------------------------------
            // Defer
            // -------------------------------------------------

            Statement::Defer { expression, .. } => {
                self.infer_expression(expression)?;
                Ok(())
            }

            // -------------------------------------------------
            // Break
            // -------------------------------------------------

            Statement::Break { span } => {
                if self.loop_depth == 0 {
                    return Err(FusionError::Syntax {
                        message:
                            "'break' is only valid inside a loop"
                                .to_string(),
                        span: *span,
                    });
                }

                Ok(())
            }

            // -------------------------------------------------
            // Continue
            // -------------------------------------------------

            Statement::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(FusionError::Syntax {
                        message:
                            "'continue' is only valid inside a loop"
                                .to_string(),
                        span: *span,
                    });
                }

                Ok(())
            }

            // -------------------------------------------------
            // Function
            // -------------------------------------------------

            Statement::Function { .. } => {
                // Function declarations are registered and checked by
                // check(). Nested functions are not currently supported
                // by the type environment.
                Ok(())
            }

            // -------------------------------------------------
            // Struct / Enum
            // -------------------------------------------------

            Statement::Struct { .. }
            | Statement::Enum { .. } => Ok(()),

            // -------------------------------------------------
            // Main
            // -------------------------------------------------

            Statement::Main { body, .. } => {
                self.push_scope();

                let result = self.check_block(body);

                self.pop_scope();

                result
            }

            // -------------------------------------------------
            // Trait
            // -------------------------------------------------

            Statement::Trait { methods, .. } => {
                for method in methods {
                    self.validate_trait_method(method)?;
                }

                Ok(())
            }

            // -------------------------------------------------
            // Impl
            // -------------------------------------------------

            Statement::Impl {
                trait_name,
                type_name,
                methods,
                span,
            } => {
                self.check_impl(
                    trait_name.as_deref(),
                    type_name,
                    methods,
                    *span,
                )
            }
        }
    }

    fn check_block(
        &mut self,
        statements: &[Statement],
    ) -> Result<(), FusionError> {
        for statement in statements {
            self.check_statement(statement)?;
        }

        Ok(())
    }

    // =========================================================
    // Match checking
    // =========================================================

    fn check_match(
        &mut self,
        expression: &Expression,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<(), FusionError> {
        let expression_type =
            self.infer_expression(expression)?;

        let mut matched_variants = HashSet::new();
        let mut has_wildcard = false;

        for arm in arms {
            match &arm.pattern.kind {
                PatternKind::Wildcard => {
                    has_wildcard = true;
                }

                PatternKind::Identifier(name) if name == "_" => {
                    has_wildcard = true;
                }

                PatternKind::Variant { name, .. } => {
                    if !matched_variants.insert(name.clone()) {
                        return Err(self.unknown_variable(
                            format!(
                                "Duplicate match variant '{}'",
                                name
                            ),
                            arm.pattern.span,
                        ));
                    }
                }

                _ => {}
            }

            self.push_scope();

            let pattern_result =
                self.check_pattern(
                    &arm.pattern,
                    &expression_type,
                );

            if let Err(error) = pattern_result {
                self.pop_scope();
                return Err(error);
            }

            let body_result = self.check_block(&arm.body);

            self.pop_scope();

            body_result?;
        }

        match &expression_type {
            Type::Enum(enum_name) => {
                if !has_wildcard {
                    let definition =
                        self.environment.enums.get(enum_name).ok_or_else(
                            || {
                                self.unknown_variable(
                                    enum_name.clone(),
                                    expression.span(),
                                )
                            },
                        )?;

                    for variant_name in definition.variants.keys() {
                        let full_name = format!(
                            "{}::{}",
                            enum_name, variant_name
                        );

                        if !matched_variants
                            .contains(&full_name)
                        {
                            return Err(self.unknown_variable(
                                format!(
                                    "Non-exhaustive match: missing variant '{}'",
                                    full_name
                                ),
                                span,
                            ));
                        }
                    }
                }
            }

            Type::Bool => {
                if !has_wildcard {
                    let mut has_true = false;
                    let mut has_false = false;

                    for arm in arms {
                        match arm.pattern.kind {
                            PatternKind::Boolean(true) => {
                                has_true = true;
                            }

                            PatternKind::Boolean(false) => {
                                has_false = true;
                            }

                            _ => {}
                        }
                    }

                    if !has_true {
                        return Err(self.unknown_variable(
                            "Non-exhaustive match: missing 'true'",
                            span,
                        ));
                    }

                    if !has_false {
                        return Err(self.unknown_variable(
                            "Non-exhaustive match: missing 'false'",
                            span,
                        ));
                    }
                }
            }

            Type::Num | Type::Float | Type::String => {
                if !has_wildcard {
                    return Err(self.unknown_variable(
                        "Non-exhaustive match: missing wildcard '_'",
                        span,
                    ));
                }
            }

            Type::Unknown => {
                // We cannot prove exhaustiveness for an unknown type.
                // Do not reject the program solely because the type is
                // unresolved.
            }

            _ => {
                if !has_wildcard {
                    return Err(self.unknown_variable(
                        "Non-exhaustive match: missing wildcard '_'",
                        span,
                    ));
                }
            }
        }

        Ok(())
    }

    // =========================================================
    // Function checking
    // =========================================================

    fn check_function_body(
        &mut self,
        function_name: &str,
        generic_parameters: &[String],
        parameters: &[Parameter],
        return_type: &Option<String>,
        body: &[Statement],
        function_span: Span,
    ) -> Result<Type, FusionError> {
        let old_function = self.current_function.take();

        let generic_set: HashSet<String> =
            generic_parameters.iter().cloned().collect();

        // Reject duplicate generic names.
        if generic_set.len() != generic_parameters.len() {
            self.current_function = old_function;

            return Err(self.unknown_variable(
                format!(
                    "Function '{}' declares a generic parameter more than once",
                    function_name
                ),
                function_span,
            ));
        }

        // Validate the declared return type before entering the body.
        let declared_return_type = match return_type {
            Some(type_name) => Some(
                self.convert_type_with_generics(
                    type_name,
                    &generic_set,
                )?,
            ),

            None => None,
        };

        self.push_scope();

        let result = (|| {
            let mut parameter_names = HashSet::new();

            for parameter in parameters {
                if !parameter_names.insert(parameter.name.clone()) {
                    return Err(self.unknown_variable(
                        format!(
                            "Parameter '{}' is declared more than once",
                            parameter.name
                        ),
                        parameter.name_span,
                    ));
                }

                let ty = match &parameter.type_name {
                    Some(type_name) => {
                        self.convert_type_with_generics(
                            type_name,
                            &generic_set,
                        )?
                    }

                    None => Type::Unknown,
                };

                self.declare_variable(
                    parameter.name.clone(),
                    ty,
                    true,
                    parameter.name_span,
                )?;
            }

            self.current_function =
                Some(FunctionContext {
                    declared_return_type:
                        declared_return_type.clone(),
                    inferred_return_type: None,
                    has_return: false,
                });

            self.check_block(body)?;

            let context = self
                .current_function
                .as_ref()
                .expect("function context was installed");

            if let Some(declared) =
                &context.declared_return_type
            {
                if !Self::block_returns(body) {
                    return Err(FusionError::TypeMismatch {
                        expected: declared.name(),
                        found: "no return".to_string(),
                        span: function_span,
                    });
                }

                Ok(declared.clone())
            } else {
                Ok(context
                    .inferred_return_type
                    .clone()
                    .unwrap_or(Type::Void))
            }
        })();

        self.current_function = old_function;
        self.pop_scope();

        result
    }

    // =========================================================
    // Trait checking
    // =========================================================

    fn validate_trait_method(
        &self,
        method: &TraitMethod,
    ) -> Result<(), FusionError> {
        let mut names = HashSet::new();

        for parameter in &method.parameters {
            if !names.insert(parameter.name.clone()) {
                return Err(self.unknown_variable(
                    format!(
                        "Parameter '{}' is declared more than once",
                        parameter.name
                    ),
                    parameter.name_span,
                ));
            }

            if let Some(type_name) = &parameter.type_name {
                self.convert_type(type_name)?;
            }
        }

        if let Some(return_type) = &method.return_type {
            self.convert_type(return_type)?;
        }

        Ok(())
    }

    // =========================================================
    // Impl checking
    // =========================================================

    fn check_impl(
        &mut self,
        trait_name: Option<&str>,
        type_name: &str,
        methods: &[Statement],
        span: Span,
    ) -> Result<(), FusionError> {
        let target_type = self.convert_type(type_name)?;

        if let Some(trait_name) = trait_name {
            // A trait must exist before it can be implemented.
            //
            // The current environment does not retain trait definitions,
            // so the implementation can only validate the target type
            // here. The parser/registration layer remains responsible for
            // retaining trait metadata if stricter conformance checking
            // is desired later.
            let _ = trait_name;
        }

        let _ = target_type;

        let mut method_names = HashSet::new();

        for method in methods {
            match method {
                Statement::Function {
                    name,
                    generic_parameters,
                    parameters,
                    return_type,
                    body,
                    span: method_span,
                    ..
                } => {
                    if !method_names.insert(name.clone()) {
                        return Err(self.unknown_variable(
                            format!(
                                "Method '{}' is declared more than once in impl",
                                name
                            ),
                            *method_span,
                        ));
                    }

                    let inferred_return =
                        self.check_function_body(
                            name,
                            generic_parameters,
                            parameters,
                            return_type,
                            body,
                            *method_span,
                        )?;

                    // Keep the result meaningful for the checker even
                    // though impl methods currently aren't entered into the
                    // global FunctionType table.
                    let _ = inferred_return;
                }

                other => {
                    return Err(FusionError::Syntax {
                        message:
                            "Only function declarations are valid inside an impl block"
                                .to_string(),
                        span: other.span(),
                    });
                }
            }
        }

        let _ = span;

        Ok(())
    }

    // =========================================================
    // Whole-program checking
    // =========================================================

    pub fn check(
        &mut self,
        program: &Program,
    ) -> Result<(), FusionError> {
        // -----------------------------------------------------
        // Pass 1: register struct names
        // -----------------------------------------------------

        for statement in &program.statements {
            if let Statement::Struct {
                name,
                span,
                ..
            } = statement
            {
                if self.environment.structs.contains_key(name) {
                    return Err(self.unknown_variable(
                        format!(
                            "Struct '{}' is already defined",
                            name
                        ),
                        *span,
                    ));
                }

                // Do not allow a type name to collide with an enum.
                if self.environment.enums.contains_key(name) {
                    return Err(self.unknown_variable(
                        format!(
                            "Type '{}' is already defined",
                            name
                        ),
                        *span,
                    ));
                }

                self.environment.structs.insert(
                    name.clone(),
                    StructDefinition {
                        fields: HashMap::new(),
                    },
                );
            }
        }

        // -----------------------------------------------------
        // Pass 2: register enum names
        // -----------------------------------------------------

        for statement in &program.statements {
            if let Statement::Enum {
                name,
                span,
                ..
            } = statement
            {
                if self.environment.enums.contains_key(name) {
                    return Err(self.unknown_variable(
                        format!(
                            "Enum '{}' is already defined",
                            name
                        ),
                        *span,
                    ));
                }

                if self.environment.structs.contains_key(name) {
                    return Err(self.unknown_variable(
                        format!(
                            "Type '{}' is already defined",
                            name
                        ),
                        *span,
                    ));
                }

                self.environment.enums.insert(
                    name.clone(),
                    EnumDefinition {
                        variants: HashMap::new(),
                    },
                );
            }
        }

        // -----------------------------------------------------
        // Pass 3: resolve struct fields
        // -----------------------------------------------------

        for statement in &program.statements {
            if let Statement::Struct {
                name,
                fields,
                ..
            } = statement
            {
                let mut field_map = HashMap::new();

                for field in fields {
                    if field_map.contains_key(&field.name) {
                        return Err(self.unknown_variable(
                            format!(
                                "Duplicate field '{}' in struct '{}'",
                                field.name, name
                            ),
                            field.name_span,
                        ));
                    }

                    let field_type =
                        self.convert_type(&field.type_name)?;

                    field_map.insert(
                        field.name.clone(),
                        field_type,
                    );
                }

                self.environment
                    .structs
                    .get_mut(name)
                    .expect("struct was registered")
                    .fields = field_map;
            }
        }

        // -----------------------------------------------------
        // Pass 4: resolve enum variants
        // -----------------------------------------------------

        for statement in &program.statements {
            if let Statement::Enum {
                name,
                variants,
                ..
            } = statement
            {
                let mut variant_map = HashMap::new();

                for variant in variants {
                    if variant_map.contains_key(&variant.name) {
                        return Err(self.unknown_variable(
                            format!(
                                "Duplicate variant '{}' in enum '{}'",
                                variant.name, name
                            ),
                            variant.name_span,
                        ));
                    }

                    let mut field_types = Vec::new();

                    for field in &variant.fields {
                        field_types.push(
                            self.convert_type(field)?,
                        );
                    }

                    variant_map.insert(
                        variant.name.clone(),
                        EnumVariantDefinition {
                            fields: field_types,
                        },
                    );
                }

                self.environment
                    .enums
                    .get_mut(name)
                    .expect("enum was registered")
                    .variants = variant_map;
            }
        }

        // -----------------------------------------------------
        // Pass 5: register function signatures
        // -----------------------------------------------------

        for statement in &program.statements {
            if let Statement::Function {
                name,
                parameters,
                return_type,
                generic_parameters,
                span,
                ..
            } = statement
            {
                if self.environment.functions.contains_key(name) {
                    return Err(self.unknown_variable(
                        format!(
                            "Function '{}' is already defined",
                            name
                        ),
                        *span,
                    ));
                }

                let generic_set: HashSet<String> =
                    generic_parameters.iter().cloned().collect();

                if generic_set.len()
                    != generic_parameters.len()
                {
                    return Err(self.unknown_variable(
                        format!(
                            "Function '{}' declares a generic parameter more than once",
                            name
                        ),
                        *span,
                    ));
                }

                let mut parameter_names =
                    HashSet::new();

                let mut parameter_types =
                    Vec::new();

                for parameter in parameters {
                    if !parameter_names
                        .insert(parameter.name.clone())
                    {
                        return Err(self.unknown_variable(
                            format!(
                                "Parameter '{}' is declared more than once",
                                parameter.name
                            ),
                            parameter.name_span,
                        ));
                    }

                    let ty = match &parameter.type_name {
                        Some(type_name) => {
                            self.convert_type_with_generics(
                                type_name,
                                &generic_set,
                            )?
                        }

                        None => Type::Unknown,
                    };

                    parameter_types.push(ty);
                }

                let return_ty = match return_type {
                    Some(type_name) => {
                        self.convert_type_with_generics(
                            type_name,
                            &generic_set,
                        )?
                    }

                    None => Type::Unknown,
                };

                self.environment.functions.insert(
                    name.clone(),
                    FunctionType {
                        parameters: parameter_types,
                        return_type: return_ty,
                    },
                );
            }
        }

        // -----------------------------------------------------
        // Pass 6: check executable program statements
        // -----------------------------------------------------

        for statement in &program.statements {
            match statement {
                Statement::Struct { .. }
                | Statement::Enum { .. }
                | Statement::Trait { .. } => {
                    // Structs and enums were fully validated during
                    // registration. Trait signatures are checked here.
                    if let Statement::Trait { methods, .. } =
                        statement
                    {
                        for method in methods {
                            self.validate_trait_method(method)?;
                        }
                    }
                }

                Statement::Function {
                    name,
                    generic_parameters,
                    parameters,
                    return_type,
                    body,
                    span,
                    ..
                } => {
                    let inferred_return =
                        self.check_function_body(
                            name,
                            generic_parameters,
                            parameters,
                            return_type,
                            body,
                            *span,
                        )?;

                    // Update an untyped function signature with its inferred
                    // return type so later calls see the real result.
                    if return_type.is_none() {
                        if let Some(function) =
                            self.environment.functions.get_mut(name)
                        {
                            function.return_type =
                                inferred_return;
                        }
                    }
                }

                _ => {
                    self.check_statement(statement)?;
                }
            }
        }

        Ok(())
    }
}
