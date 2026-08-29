//contents of type_checker.rs
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::errors::*;
use crate::span::Span;
use crate::types::{
    EnumDefinition,
    EnumVariantDefinition,
    StructDefinition,
    Type,
};

fn dummy_span() -> Span {
    Span { start: 0, end: 0 }
}

/// Expressions currently do not carry source spans in ast.rs.
/// Keep all expression-related diagnostics at a valid fallback span
/// until spans are added to Expression.
fn expression_span(_expression: &Expression) -> Span {
    dummy_span()
}

pub struct TypeEnvironment {
    pub scopes: Vec<HashMap<String, VariableInfo>>,
    pub functions: HashMap<String, FunctionType>,
    pub structs: HashMap<String, StructDefinition>,
    pub enums: HashMap<String, EnumDefinition>,
}

struct FunctionContext {
    declared_return_type: Option<Type>,
    inferred_return_type: Option<Type>,
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
}

impl TypeChecker {
    // ---------------------------------------------------------
    // Construction
    // ---------------------------------------------------------

    pub fn new() -> Self {
        let mut functions = HashMap::new();

        functions.insert(
    "print".into(),
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
        }
    }

    // ---------------------------------------------------------
    // Scopes
    // ---------------------------------------------------------

    fn push_scope(&mut self) {
        self.environment.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.environment.scopes.pop();
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
            .expect("type checker always has at least one scope");

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
        for scope in self.environment.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.ty.clone());
            }
        }

        None
    }

    fn lookup_variable_info(&self, name: &str) -> Option<VariableInfo> {
        for scope in self.environment.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }

        None
    }

    // ---------------------------------------------------------
    // Errors
    // ---------------------------------------------------------

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

    // ---------------------------------------------------------
    // Type conversion
    // ---------------------------------------------------------

    fn convert_type(&self, name: &str) -> Result<Type, FusionError> {
        match name {
            "num" => Ok(Type::Num),
            "float" => Ok(Type::Float),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),

            _ if self.environment.structs.contains_key(name) => {
                Ok(Type::Struct(name.to_string()))
            }

            _ if self.environment.enums.contains_key(name) => {
                Ok(Type::Enum(name.to_string()))
            }

            _ => Err(FusionError::TypeMismatch {
                expected: "known type".into(),
                found: name.to_string(),
                span: dummy_span(),
            }),
        }
    }

    // ---------------------------------------------------------
    // Operators
    // ---------------------------------------------------------

    fn operator_name(&self, operator: &Operator) -> String {
        match operator {
            Operator::Plus => "+".into(),
            Operator::Minus => "-".into(),
            Operator::Multiply => "*".into(),
            Operator::Divide => "/".into(),
            Operator::Equal => "==".into(),
            Operator::NotEqual => "!=".into(),
            Operator::Less => "<".into(),
            Operator::LessEqual => "<=".into(),
            Operator::Greater => ">".into(),
            Operator::GreaterEqual => ">=".into(),
            Operator::And => "and".into(),
            Operator::Or => "or".into(),
        }
    }

    fn require_bool(
        &self,
        found: Type,
        span: Span,
    ) -> Result<(), FusionError> {
        if found == Type::Bool {
            Ok(())
        } else {
            Err(FusionError::TypeMismatch {
                expected: "bool".into(),
                found: format!("{:?}", found),
                span,
            })
        }
    }

    // ---------------------------------------------------------
    // Property helpers
    // ---------------------------------------------------------

    fn property_root_name(
        expression: &Expression,
    ) -> Option<String> {
        match expression {
            Expression::Identifier(name) => Some(name.clone()),

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
                FusionError::UnknownVariable {
                    name: "Invalid property assignment target".into(),
                    span: expression_span(expression),
                }
            })?;

        let info =
            self.lookup_variable_info(&root_name).ok_or_else(|| {
                FusionError::UnknownVariable {
                    name: root_name.clone(),
                    span: expression_span(expression),
                }
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
                    .ok_or_else(|| FusionError::UnknownVariable {
                        name: struct_name.clone(),
                        span: expression_span(object),
                    })?;

                definition
                    .fields
                    .get(name)
                    .cloned()
                    .ok_or_else(|| FusionError::UnknownVariable {
                        name: format!(
                            "Unknown field '{}' on struct '{}'",
                            name, struct_name
                        ),
                        span: expression_span(object),
                    })
            }

            other => Err(FusionError::TypeMismatch {
                expected: "struct".into(),
                found: format!("{:?}", other),
                span: expression_span(object),
            }),
        }
    }

    // ---------------------------------------------------------
    // Pattern checking
    // ---------------------------------------------------------

    fn check_pattern(
        &mut self,
        pattern: &Pattern,
        expected_type: &Type,
    ) -> Result<(), FusionError> {
        match pattern {
            Pattern::Wildcard => Ok(()),

            Pattern::Identifier(name) => {
                if name == "_" {
                    return Ok(());
                }

                self.declare_variable(
                    name.clone(),
                    expected_type.clone(),
                    true,
                    dummy_span(),
                )?;

                Ok(())
            }

            Pattern::Number(_) => {
                if *expected_type != Type::Num {
                    return Err(FusionError::TypeMismatch {
                        expected: "num".into(),
                        found: format!("{:?}", expected_type),
                        span: dummy_span(),
                    });
                }

                Ok(())
            }

            Pattern::Float(_) => {
                if *expected_type != Type::Float {
                    return Err(FusionError::TypeMismatch {
                        expected: "float".into(),
                        found: format!("{:?}", expected_type),
                        span: dummy_span(),
                    });
                }

                Ok(())
            }

            Pattern::String(_) => {
                if *expected_type != Type::String {
                    return Err(FusionError::TypeMismatch {
                        expected: "string".into(),
                        found: format!("{:?}", expected_type),
                        span: dummy_span(),
                    });
                }

                Ok(())
            }

            Pattern::Boolean(_) => {
                if *expected_type != Type::Bool {
                    return Err(FusionError::TypeMismatch {
                        expected: "bool".into(),
                        found: format!("{:?}", expected_type),
                        span: dummy_span(),
                    });
                }

                Ok(())
            }

            Pattern::Variant {
                name,
                bindings,
            } => {
                let enum_name = match expected_type {
                    Type::Enum(name) => name,
                    _ => {
                        return Err(FusionError::TypeMismatch {
                            expected: "enum".into(),
                            found: format!("{:?}", expected_type),
                            span: dummy_span(),
                        });
                    }
                };

                let parts: Vec<&str> = name.split("::").collect();

                if parts.len() != 2 {
                    return Err(FusionError::UnknownVariable {
                        name: format!(
                            "Invalid enum pattern '{}'",
                            name
                        ),
                        span: dummy_span(),
                    });
                }

                let pattern_enum = parts[0];
                let variant_name = parts[1];

                if pattern_enum != enum_name {
                    return Err(FusionError::TypeMismatch {
                        expected: enum_name.clone(),
                        found: pattern_enum.to_string(),
                        span: dummy_span(),
                    });
                }

                let field_types = {
                    let enum_definition = self
                        .environment
                        .enums
                        .get(enum_name)
                        .ok_or_else(|| {
                            FusionError::UnknownVariable {
                                name: enum_name.clone(),
                                span: dummy_span(),
                            }
                        })?;

                    let variant =
                        enum_definition.variants.get(variant_name).ok_or_else(
                            || FusionError::UnknownVariable {
                                name: format!(
                                    "Unknown variant '{}::{}'",
                                    enum_name, variant_name
                                ),
                                span: dummy_span(),
                            },
                        )?;

                    if bindings.len() != variant.fields.len() {
                        return Err(FusionError::TypeMismatch {
                            expected: format!(
                                "{} bindings",
                                variant.fields.len()
                            ),
                            found: format!(
                                "{} bindings",
                                bindings.len()
                            ),
                            span: dummy_span(),
                        });
                    }

                    variant.fields.clone()
                };

                for (binding, field_type) in
                    bindings.iter().zip(field_types.iter())
                {
                    if binding != "_" {
                        self.declare_variable(
                            binding.clone(),
                            field_type.clone(),
                            true,
                            dummy_span(),
                        )?;
                    }
                }

                Ok(())
            }
        }
    }

    // ---------------------------------------------------------
    // Return analysis
    // ---------------------------------------------------------

    fn block_returns(statements: &[Statement]) -> bool {
        for statement in statements {
            match statement {
                Statement::Return(_) => {
                    return true;
                }

                Statement::If {
                    body,
                    else_body,
                    ..
                } => {
                    if let Some(else_body) = else_body {
                        if Self::block_returns(body)
                            && Self::block_returns(else_body)
                        {
                            return true;
                        }
                    }
                }

                _ => {}
            }
        }

        false
    }

    // ---------------------------------------------------------
    // Expression inference
    // ---------------------------------------------------------

    fn infer_expression(
        &self,
        expression: &Expression,
    ) -> Result<Type, FusionError> {
        match expression {
            // ---------------------------------------------
            // Literals
            // ---------------------------------------------

            Expression::Number(_) => Ok(Type::Num),

            Expression::Float(_) => Ok(Type::Float),

            Expression::String(_) => Ok(Type::String),

            Expression::Boolean(_) => Ok(Type::Bool),

            // ---------------------------------------------
            // Identifier
            // ---------------------------------------------

            Expression::Identifier(name) => {
                match self.lookup_variable(name) {
                    Some(ty) => Ok(ty),

                    None => Err(FusionError::UnknownVariable {
                        name: name.clone(),
                        span: expression_span(expression),
                    }),
                }
            }

            // ---------------------------------------------
            // Unary
            // ---------------------------------------------

            Expression::Unary {
                operator,
                expression: inner,
            } => {
                let inner_type =
                    self.infer_expression(inner)?;

                match operator {
                    UnaryOperator::Negate => {
                        if inner_type == Type::Num
                            || inner_type == Type::Float
                        {
                            Ok(inner_type)
                        } else {
                            Err(FusionError::InvalidOperation {
                                left: format!("{:?}", inner_type),
                                operator: "-".into(),
                                right: "".into(),
                                span: expression_span(inner),
                            })
                        }
                    }

                    UnaryOperator::Not => {
                        if inner_type == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            Err(FusionError::InvalidOperation {
                                left: format!("{:?}", inner_type),
                                operator: "!".into(),
                                right: "".into(),
                                span: expression_span(inner),
                            })
                        }
                    }
                }
            }

            // ---------------------------------------------
            // Binary
            // ---------------------------------------------

            Expression::Binary {
                left,
                operator,
                right,
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
            Ok(Type::Unknown)
        } else if left_type == Type::Num
            && right_type == Type::Num
        {
            Ok(Type::Num)
        } else if left_type == Type::Float
            && right_type == Type::Float
        {
            Ok(Type::Float)
        } else if *operator == Operator::Plus
            && left_type == Type::String
            && right_type == Type::String
        {
            Ok(Type::String)
        } else {
            Err(FusionError::InvalidOperation {
                left: format!("{:?}", left_type),
                operator: self.operator_name(operator),
                right: format!("{:?}", right_type),
                span: expression_span(expression),
            })
        }
    }

                    Operator::Equal | Operator::NotEqual => {
    if left_type == Type::Unknown
        || right_type == Type::Unknown
    {
        Ok(Type::Bool)
    } else if left_type == right_type {
        Ok(Type::Bool)
    } else {
        Err(FusionError::InvalidOperation {
            left: format!("{:?}", left_type),
            operator: self.operator_name(operator),
            right: format!("{:?}", right_type),
            span: expression_span(expression),
        })
    }
}

                    Operator::Less
                    | Operator::LessEqual
                    | Operator::Greater
                    | Operator::GreaterEqual => {
                        if (left_type == Type::Num
                            && right_type == Type::Num)
                            || (left_type == Type::Float
                                && right_type == Type::Float)
                        {
                            Ok(Type::Bool)
                        } else {
                            Err(FusionError::InvalidOperation {
                                left: format!("{:?}", left_type),
                                operator: self
                                    .operator_name(operator),
                                right: format!("{:?}", right_type),
                                span: expression_span(expression),
                            })
                        }
                    }

                    Operator::And | Operator::Or => {
    if left_type == Type::Unknown
        || right_type == Type::Unknown
    {
        Ok(Type::Bool)
    } else if left_type == Type::Bool
        && right_type == Type::Bool
    {
        Ok(Type::Bool)
    } else {
        Err(FusionError::InvalidOperation {
            left: format!("{:?}", left_type),
            operator: self.operator_name(operator),
            right: format!("{:?}", right_type),
            span: expression_span(expression),
        })
    }
}
                }
            }

            // ---------------------------------------------
            // Function call
            // ---------------------------------------------

            Expression::Call {
                name,
                arguments,
                ..
            } => {
                let function =
                    self.environment.functions.get(name).ok_or_else(
                        || FusionError::UnknownVariable {
                            name: name.clone(),
                            span: expression_span(expression),
                        },
                    )?;

                if arguments.len() != function.parameters.len() {
                    return Err(FusionError::TypeMismatch {
                        expected: format!(
                            "{} arguments",
                            function.parameters.len()
                        ),
                        found: format!(
                            "{} arguments",
                            arguments.len()
                        ),
                        span: expression_span(expression),
                    });
                }

                for (argument, expected_type) in arguments
                    .iter()
                    .zip(function.parameters.iter())
                {
                    let actual_type =
                        self.infer_expression(argument)?;

                    if *expected_type != Type::Unknown
                        && actual_type != *expected_type
                    {
                        return Err(FusionError::TypeMismatch {
                            expected: format!(
                                "{:?}",
                                expected_type
                            ),
                            found: format!("{:?}", actual_type),
                            span: expression_span(argument),
                        });
                    }
                }

                Ok(function.return_type.clone())
            }

            // ---------------------------------------------
            // Arrays
            // ---------------------------------------------

            Expression::Array(values) => {
                if values.is_empty() {
                    return Ok(Type::Array(Box::new(Type::Unknown)));
                }

                let first_type =
                    self.infer_expression(&values[0])?;

                for value in values.iter().skip(1) {
                    let value_type =
                        self.infer_expression(value)?;

                    if value_type != first_type {
                        return Err(FusionError::TypeMismatch {
                            expected: format!(
                                "{:?}",
                                first_type
                            ),
                            found: format!(
                                "{:?}",
                                value_type
                            ),
                            span: expression_span(value),
                        });
                    }
                }

                Ok(Type::Array(Box::new(first_type)))
            }

            // ---------------------------------------------
            // Array indexing
            // ---------------------------------------------

            Expression::Index { array, index } => {
                let array_type =
                    self.infer_expression(array)?;

                let index_type =
                    self.infer_expression(index)?;

                if index_type != Type::Num {
                    return Err(FusionError::TypeMismatch {
                        expected: "num index".into(),
                        found: format!("{:?}", index_type),
                        span: expression_span(index),
                    });
                }

                match array_type {
                    Type::Array(inner) => Ok(*inner),

                    other => Err(FusionError::TypeMismatch {
                        expected: "array".into(),
                        found: format!("{:?}", other),
                        span: expression_span(array),
                    }),
                }
            }

            // ---------------------------------------------
            // Struct property
            // ---------------------------------------------

            Expression::Property { object, name } => {
                self.lookup_property_type(object, name)
            }

            // ---------------------------------------------
            // Array methods
            // ---------------------------------------------

            Expression::MethodCall {
                object,
                method,
                arguments,
            } => {
                let object_type =
                    self.infer_expression(object)?;

                match object_type {
                    Type::Array(element_type) => {
                        match method.as_str() {
                            "push" => {
                                if arguments.len() != 1 {
                                    return Err(
                                        FusionError::TypeMismatch {
                                            expected:
                                                "1 argument".into(),
                                            found: format!(
                                                "{} arguments",
                                                arguments.len()
                                            ),
                                            span: expression_span(
                                                expression,
                                            ),
                                        },
                                    );
                                }

                                let argument_type =
                                    self.infer_expression(
                                        &arguments[0],
                                    )?;

                                if *element_type != Type::Unknown
                                    && argument_type != *element_type
                                {
                                    return Err(
                                        FusionError::TypeMismatch {
                                            expected: format!(
                                                "{:?}",
                                                element_type
                                            ),
                                            found: format!(
                                                "{:?}",
                                                argument_type
                                            ),
                                            span: expression_span(
                                                &arguments[0],
                                            ),
                                        },
                                    );
                                }

                                Ok(Type::Void)
                            }

                            "pop" => {
                                if !arguments.is_empty() {
                                    return Err(
                                        FusionError::TypeMismatch {
                                            expected:
                                                "0 arguments".into(),
                                            found: format!(
                                                "{} arguments",
                                                arguments.len()
                                            ),
                                            span: expression_span(
                                                expression,
                                            ),
                                        },
                                    );
                                }

                                Ok(*element_type)
                            }

                            "length" => {
                                if !arguments.is_empty() {
                                    return Err(
                                        FusionError::TypeMismatch {
                                            expected:
                                                "0 arguments".into(),
                                            found: format!(
                                                "{} arguments",
                                                arguments.len()
                                            ),
                                            span: expression_span(
                                                expression,
                                            ),
                                        },
                                    );
                                }

                                Ok(Type::Num)
                            }

                            _ => {
                                Err(FusionError::UnknownVariable {
                                    name: format!(
                                        "Unknown array method '{}'",
                                        method
                                    ),
                                    span: expression_span(expression),
                                })
                            }
                        }
                    }

                    other => {
                        Err(FusionError::TypeMismatch {
                            expected: "array".into(),
                            found: format!("{:?}", other),
                            span: expression_span(object),
                        })
                    }
                }
            }

            // ---------------------------------------------
            // Struct constructor
            // ---------------------------------------------

            Expression::StructConstructor {
                name,
                fields,
            } => {
                let definition = self
                    .environment
                    .structs
                    .get(name)
                    .ok_or_else(|| {
                        FusionError::UnknownVariable {
                            name: name.clone(),
                            span: expression_span(expression),
                        }
                    })?;

                // Reject duplicate fields and check supplied fields.
                let mut supplied_fields = HashSet::new();

                for (field_name, field_expression) in fields {
                    if !supplied_fields.insert(field_name.clone()) {
                        return Err(FusionError::UnknownVariable {
                            name: format!(
                                "Field '{}' is specified more than once",
                                field_name
                            ),
                            span: expression_span(
                                field_expression,
                            ),
                        });
                    }

                    let expected_type =
                        definition.fields.get(field_name).ok_or_else(
                            || FusionError::UnknownVariable {
                                name: format!(
                                    "Unknown field '{}' on struct '{}'",
                                    field_name, name
                                ),
                                span: expression_span(
                                    field_expression,
                                ),
                            },
                        )?;

                    let actual_type =
                        self.infer_expression(field_expression)?;

                    if *expected_type != actual_type {
                        return Err(FusionError::TypeMismatch {
                            expected: format!(
                                "{:?}",
                                expected_type
                            ),
                            found: format!("{:?}", actual_type),
                            span: expression_span(
                                field_expression,
                            ),
                        });
                    }
                }

                // Make sure every required field was supplied.
                for field_name in definition.fields.keys() {
                    if !supplied_fields.contains(field_name) {
                        return Err(FusionError::UnknownVariable {
                            name: format!(
                                "Missing field '{}' in struct '{}'",
                                field_name, name
                            ),
                            span: expression_span(expression),
                        });
                    }
                }

                Ok(Type::Struct(name.clone()))
            }

            // ---------------------------------------------
            // Enum constructor
            // ---------------------------------------------

            Expression::EnumConstructor {
                enum_name,
                variant,
                arguments,
            } => {
                let definition = self
                    .environment
                    .enums
                    .get(enum_name)
                    .ok_or_else(|| {
                        FusionError::UnknownVariable {
                            name: enum_name.clone(),
                            span: expression_span(expression),
                        }
                    })?;

                let variant_definition = definition
                    .variants
                    .get(variant)
                    .ok_or_else(|| {
                        FusionError::UnknownVariable {
                            name: format!(
                                "Unknown variant '{}::{}'",
                                enum_name, variant
                            ),
                            span: expression_span(expression),
                        }
                    })?;

                if arguments.len() != variant_definition.fields.len()
                {
                    return Err(FusionError::TypeMismatch {
                        expected: format!(
                            "{} arguments",
                            variant_definition.fields.len()
                        ),
                        found: format!(
                            "{} arguments",
                            arguments.len()
                        ),
                        span: expression_span(expression),
                    });
                }

                for (argument, expected_type) in arguments
                    .iter()
                    .zip(variant_definition.fields.iter())
                {
                    let actual_type =
                        self.infer_expression(argument)?;

                    if actual_type != *expected_type {
                        return Err(FusionError::TypeMismatch {
                            expected: format!(
                                "{:?}",
                                expected_type
                            ),
                            found: format!("{:?}", actual_type),
                            span: expression_span(argument),
                        });
                    }
                }

                Ok(Type::Enum(enum_name.clone()))
            }
        }
    }

    // ---------------------------------------------------------
    // Statement checking
    // ---------------------------------------------------------

    pub fn check_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<(), FusionError> {
        match statement {
            // ---------------------------------------------
            // Variable declarations
            // ---------------------------------------------

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

                            if declared != inferred {
                                return Err(
                                    FusionError::TypeMismatch {
                                        expected: format!(
                                            "{:?}",
                                            declared
                                        ),
                                        found: format!(
                                            "{:?}",
                                            inferred
                                        ),
                                        span: declaration.span,
                                    },
                                );
                            }

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

            // ---------------------------------------------
            // Constants
            // ---------------------------------------------

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

                        if declared != inferred {
                            return Err(
                                FusionError::TypeMismatch {
                                    expected: format!(
                                        "{:?}",
                                        declared
                                    ),
                                    found: format!(
                                        "{:?}",
                                        inferred
                                    ),
                                    span: *name_span,
                                },
                            );
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
                )?;

                Ok(())
            }

            // ---------------------------------------------
            // Assignment
            // ---------------------------------------------

            Statement::Assignment {
                target,
                value,
            } => {
                let inferred =
                    self.infer_expression(value)?;

                match target {
                    Expression::Identifier(name) => {
                        match self.lookup_variable_info(name) {
                            Some(info) => {
                                if !info.mutable {
                                    return Err(
                                        FusionError::CannotAssignToConst {
                                            name: name.clone(),
                                            span: expression_span(
                                                target,
                                            ),
                                        },
                                    );
                                }

                                if info.ty != inferred {
                                    return Err(
                                        FusionError::TypeMismatch {
                                            expected: format!(
                                                "{:?}",
                                                info.ty
                                            ),
                                            found: format!(
                                                "{:?}",
                                                inferred
                                            ),
                                            span: expression_span(
                                                target,
                                            ),
                                        },
                                    );
                                }

                                Ok(())
                            }

                            None => {
                                self.declare_variable(
                                    name.clone(),
                                    inferred,
                                    true,
                                    expression_span(target),
                                )?;

                                Ok(())
                            }
                        }
                    }

                    Expression::Property { object, name } => {
                        if !self.property_target_is_mutable(object)? {
                            let variable_name =
                                Self::property_root_name(object)
                                    .unwrap_or_else(|| {
                                        "<unknown>".into()
                                    });

                            return Err(
                                FusionError::CannotAssignToConst {
                                    name: variable_name,
                                    span: expression_span(object),
                                },
                            );
                        }

                        let field_type =
                            self.lookup_property_type(object, name)?;

                        if field_type != inferred {
                            return Err(
                                FusionError::TypeMismatch {
                                    expected: format!(
                                        "{:?}",
                                        field_type
                                    ),
                                    found: format!(
                                        "{:?}",
                                        inferred
                                    ),
                                    span: expression_span(target),
                                },
                            );
                        }

                        Ok(())
                    }

                    _ => Err(FusionError::UnknownVariable {
                        name: "invalid assignment target".into(),
                        span: expression_span(target),
                    }),
                }
            }

            // ---------------------------------------------
            // Function-call statement
            // ---------------------------------------------

            Statement::Call(expression) => {
                self.infer_expression(expression)?;
                Ok(())
            }

            // ---------------------------------------------
            // Expression statement
            // ---------------------------------------------

            Statement::Expression(expression) => {
                self.infer_expression(expression)?;
                Ok(())
            }

            // ---------------------------------------------
            // If
            // ---------------------------------------------

            Statement::If {
                condition,
                body,
                else_body,
            } => {
                let condition_type =
                    self.infer_expression(condition)?;

                self.require_bool(
                    condition_type,
                    expression_span(condition),
                )?;

                self.push_scope();

                for statement in body {
                    if let Err(error) =
                        self.check_statement(statement)
                    {
                        self.pop_scope();
                        return Err(error);
                    }
                }

                self.pop_scope();

                if let Some(else_statements) = else_body {
                    self.push_scope();

                    for statement in else_statements {
                        if let Err(error) =
                            self.check_statement(statement)
                        {
                            self.pop_scope();
                            return Err(error);
                        }
                    }

                    self.pop_scope();
                }

                Ok(())
            }

            // ---------------------------------------------
            // While
            // ---------------------------------------------

            Statement::While {
                condition,
                body,
            } => {
                let condition_type =
                    self.infer_expression(condition)?;

                self.require_bool(
                    condition_type,
                    expression_span(condition),
                )?;

                self.push_scope();

                for statement in body {
                    if let Err(error) =
                        self.check_statement(statement)
                    {
                        self.pop_scope();
                        return Err(error);
                    }
                }

                self.pop_scope();

                Ok(())
            }

            // ---------------------------------------------
            // For
            // ---------------------------------------------

            Statement::For {
                variable,
                start,
                end,
                body,
            } => {
                let start_type =
                    self.infer_expression(start)?;

                let end_type =
                    self.infer_expression(end)?;

                if start_type != Type::Num
                    || end_type != Type::Num
                {
                    return Err(FusionError::TypeMismatch {
                        expected: "num range".into(),
                        found: format!(
                            "{:?}..{:?}",
                            start_type, end_type
                        ),
                        span: expression_span(start),
                    });
                }

                self.push_scope();

                self.declare_variable(
                    variable.clone(),
                    Type::Num,
                    true,
                    dummy_span(),
                )?;

                for statement in body {
                    if let Err(error) =
                        self.check_statement(statement)
                    {
                        self.pop_scope();
                        return Err(error);
                    }
                }

                self.pop_scope();

                Ok(())
            }

            // ---------------------------------------------
            // Match
            // ---------------------------------------------

            Statement::Match {
                expression,
                arms,
            } => {
                let expression_type =
                    self.infer_expression(expression)?;

                let mut matched_variants =
                    HashSet::new();

                let mut has_wildcard = false;

                for arm in arms {
                    if matches!(&arm.pattern, Pattern::Wildcard)
                        || matches!(
                            &arm.pattern,
                            Pattern::Identifier(name)
                                if name == "_"
                        )
                    {
                        has_wildcard = true;
                    }

                    if let Pattern::Variant { name, .. } =
                        &arm.pattern
                    {
                        matched_variants.insert(name.clone());
                    }

                    self.push_scope();

                    if let Err(error) = self.check_pattern(
                        &arm.pattern,
                        &expression_type,
                    ) {
                        self.pop_scope();
                        return Err(error);
                    }

                    for statement in &arm.body {
                        if let Err(error) =
                            self.check_statement(statement)
                        {
                            self.pop_scope();
                            return Err(error);
                        }
                    }

                    self.pop_scope();
                }

                match &expression_type {
                    Type::Enum(enum_name) => {
                        if !has_wildcard {
                            let enum_definition = self
                                .environment
                                .enums
                                .get(enum_name)
                                .ok_or_else(|| {
                                    FusionError::UnknownVariable {
                                        name: enum_name.clone(),
                                        span: expression_span(
                                            expression,
                                        ),
                                    }
                                })?;

                            for variant_name in
                                enum_definition.variants.keys()
                            {
                                let full_name = format!(
                                    "{}::{}",
                                    enum_name, variant_name
                                );

                                if !matched_variants
                                    .contains(&full_name)
                                {
                                    return Err(
                                        FusionError::UnknownVariable {
                                            name: format!(
                                                "Non-exhaustive match: missing variant '{}'",
                                                full_name
                                            ),
                                            span: expression_span(
                                                expression,
                                            ),
                                        },
                                    );
                                }
                            }
                        }
                    }

                    Type::Bool => {
                        if !has_wildcard {
                            let mut has_true = false;
                            let mut has_false = false;

                            for arm in arms {
                                match &arm.pattern {
                                    Pattern::Boolean(true) => {
                                        has_true = true;
                                    }

                                    Pattern::Boolean(false) => {
                                        has_false = true;
                                    }

                                    _ => {}
                                }
                            }

                            if !has_true {
                                return Err(
                                    FusionError::UnknownVariable {
                                        name: "Non-exhaustive match: missing 'true'"
                                            .into(),
                                        span: expression_span(
                                            expression,
                                        ),
                                    },
                                );
                            }

                            if !has_false {
                                return Err(
                                    FusionError::UnknownVariable {
                                        name: "Non-exhaustive match: missing 'false'"
                                            .into(),
                                        span: expression_span(
                                            expression,
                                        ),
                                    },
                                );
                            }
                        }
                    }

                    Type::Num
                    | Type::Float
                    | Type::String => {
                        if !has_wildcard {
                            return Err(
                                FusionError::UnknownVariable {
                                    name: "Non-exhaustive match: missing wildcard '_'"
                                        .into(),
                                    span: expression_span(
                                        expression,
                                    ),
                                },
                            );
                        }
                    }

                    _ => {
                        if !has_wildcard {
                            return Err(
                                FusionError::UnknownVariable {
                                    name: "Non-exhaustive match: missing wildcard '_'"
                                        .into(),
                                    span: expression_span(
                                        expression,
                                    ),
                                },
                            );
                        }
                    }
                }

                Ok(())
            }

            // ---------------------------------------------
            // Return
            // ---------------------------------------------

            Statement::Return(expression) => {
    let actual = self.infer_expression(expression)?;

    let context = self.current_function.as_mut().ok_or_else(|| {
        FusionError::TypeMismatch {
            expected: "inside function".into(),
            found: "return".into(),
            span: expression_span(expression),
        }
    })?;

    if let Some(expected) = &context.declared_return_type {
        if expected != &actual && actual != Type::Unknown {
            return Err(FusionError::TypeMismatch {
                expected: format!("{:?}", expected),
                found: format!("{:?}", actual),
                span: expression_span(expression),
            });
        }
    } else {
        match &context.inferred_return_type {
            None => {
                context.inferred_return_type = Some(actual);
            }

            Some(previous) => {
                if *previous != actual
                    && actual != Type::Unknown
                    && *previous != Type::Unknown
                {
                    return Err(FusionError::TypeMismatch {
                        expected: format!("{:?}", previous),
                        found: format!("{:?}", actual),
                        span: expression_span(expression),
                    });
                }
            }
        }
    }

    Ok(())
}

            // ---------------------------------------------
            // Defer
            // ---------------------------------------------

            Statement::Defer(expression) => {
                self.infer_expression(expression)?;
                Ok(())
            }

            // ---------------------------------------------
            // Break / Continue
            //
            // The current AST/type-checker does not track loop
            // context, so these are accepted here.
            // ---------------------------------------------

            Statement::Break | Statement::Continue => Ok(()),

            // ---------------------------------------------
            // Function
            //
            // Functions are handled by check().
            // ---------------------------------------------

            Statement::Function { .. } => Ok(()),

            // ---------------------------------------------
            // Struct / Enum
            //
            // These are registered and checked by check().
            // ---------------------------------------------

            Statement::Struct { .. }
            | Statement::Enum { .. } => Ok(()),

            // ---------------------------------------------
            // Main
            // ---------------------------------------------

            Statement::Main { body } => {
                self.push_scope();

                for statement in body {
                    if let Err(error) =
                        self.check_statement(statement)
                    {
                        self.pop_scope();
                        return Err(error);
                    }
                }

                self.pop_scope();

                Ok(())
            }

            // ---------------------------------------------
            // Trait
            //
            // Trait method signatures are not executable.
            // Their types are validated during registration.
            // ---------------------------------------------

            Statement::Trait { methods, .. } => {
                for method in methods {
                    for parameter in &method.parameters {
                        if let Some(type_name) =
                            &parameter.type_name
                        {
                            self.convert_type(type_name)?;
                        }
                    }

                    if let Some(return_type) =
                        &method.return_type
                    {
                        self.convert_type(return_type)?;
                    }
                }

                Ok(())
            }

            // ---------------------------------------------
            // Impl
            //
            // Check method bodies if they are represented as
            // Statement::Function.
            // ---------------------------------------------

            Statement::Impl { methods, .. } => {
                for method in methods {
                    if let Statement::Function {
                        name,
                        parameters,
                        return_type,
                        body,
                        ..
                    } = method
                    {
                        self.check_function_body(
                            name,
                            parameters,
                            return_type,
                            body,
                        )?;
                    }
                }

                Ok(())
            }
        }
    }

    // ---------------------------------------------------------
    // Function body checking
    // ---------------------------------------------------------

    fn check_function_body(
    &mut self,
    function_name: &str,
    parameters: &[Parameter],
    return_type: &Option<String>,
    body: &[Statement],
) -> Result<(), FusionError> {
        self.push_scope();

        for parameter in parameters {
            let ty = match &parameter.type_name {
                Some(type_name) => {
                    self.convert_type(type_name)?
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

        let old_function =
            self.current_function.take();

        self.current_function = Some(FunctionContext {
    declared_return_type: match return_type {
        Some(type_name) => Some(self.convert_type(type_name)?),
        None => None,
    },
    inferred_return_type: None,
});

        for statement in body {
            if let Err(error) =
                self.check_statement(statement)
            {
                self.current_function = old_function;
                self.pop_scope();
                return Err(error);
            }
        }

        if return_type.is_some()
            && !Self::block_returns(body)
        {
            self.current_function = old_function;
            self.pop_scope();

            return Err(FusionError::TypeMismatch {
                expected: "function return value".into(),
                found: "no return".into(),
                span: dummy_span(),
            });
        }

        self.current_function = old_function;
        self.pop_scope();

        Ok(())
    }

    // ---------------------------------------------------------
    // Whole-program checking
    // ---------------------------------------------------------

    pub fn check(
        &mut self,
        program: &Program,
    ) -> Result<(), FusionError> {
        // =====================================================
        // Pass 1:
        // Register all struct names.
        // =====================================================

        for statement in &program.statements {
            if let Statement::Struct { name, .. } = statement {
                if self.environment.structs.contains_key(name) {
                    return Err(FusionError::UnknownVariable {
                        name: format!(
                            "Struct '{}' is already defined",
                            name
                        ),
                        span: dummy_span(),
                    });
                }

                self.environment.structs.insert(
                    name.clone(),
                    StructDefinition {
                        fields: HashMap::new(),
                    },
                );
            }
        }

        // =====================================================
        // Pass 2:
        // Register all enum names.
        //
        // This happens before resolving fields so structs and
        // enums may refer to one another.
        // =====================================================

        for statement in &program.statements {
            if let Statement::Enum { name, .. } = statement {
                if self.environment.enums.contains_key(name) {
                    return Err(FusionError::UnknownVariable {
                        name: format!(
                            "Enum '{}' is already defined",
                            name
                        ),
                        span: dummy_span(),
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

        // =====================================================
        // Pass 3:
        // Resolve struct fields.
        // =====================================================

        for statement in &program.statements {
            if let Statement::Struct {
                name,
                fields,
            } = statement
            {
                let mut field_map = HashMap::new();

                for field in fields {
                    if field_map.contains_key(&field.name) {
                        return Err(
                            FusionError::UnknownVariable {
                                name: format!(
                                    "Duplicate field '{}' in struct '{}'",
                                    field.name, name
                                ),
                                span: field.name_span,
                            },
                        );
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

        // =====================================================
        // Pass 4:
        // Resolve enum variants.
        // =====================================================

        for statement in &program.statements {
            if let Statement::Enum {
                name,
                variants,
            } = statement
            {
                let mut variant_map = HashMap::new();

                for variant in variants {
                    if variant_map.contains_key(&variant.name) {
                        return Err(
                            FusionError::UnknownVariable {
                                name: format!(
                                    "Duplicate variant '{}' in enum '{}'",
                                    variant.name, name
                                ),
                                span: variant.name_span,
                            },
                        );
                    }

                    let mut field_types = Vec::new();

                    for field in &variant.fields {
                        let field_type =
                            self.convert_type(field)?;

                        field_types.push(field_type);
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

        // =====================================================
        // Pass 5:
        // Register functions.
        // =====================================================

        for statement in &program.statements {
            if let Statement::Function {
                name,
                parameters,
                return_type,
                ..
            } = statement
            {
                if self.environment.functions.contains_key(name) {
                    return Err(FusionError::UnknownVariable {
                        name: format!(
                            "Function '{}' is already defined",
                            name
                        ),
                        span: dummy_span(),
                    });
                }

                let mut parameter_types = Vec::new();

                for parameter in parameters {
                    let ty = match &parameter.type_name {
                        Some(type_name) => {
                            self.convert_type(type_name)?
                        }

                        None => Type::Unknown,
                    };

                    parameter_types.push(ty);
                }

                let return_ty = match return_type {
    Some(type_name) => self.convert_type(type_name)?,
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

        // =====================================================
        // Pass 6:
        // Check all executable statements.
        // =====================================================

        for statement in &program.statements {
            match statement {
                // Definitions were handled in earlier passes.
                Statement::Struct { .. }
                | Statement::Enum { .. } => {}

                Statement::Function {
                    name,
                    parameters,
                    return_type,
                    body,
                    ..
                } => {
                    self.check_function_body(
                        name,
                        parameters,
                        return_type,
                        body,
                    )?;
                }

                _ => {
                    self.check_statement(statement)?;
                }
            }
        }

        Ok(())
    }
}