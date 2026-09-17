//contents of type_checker.rs
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::errors::FusionError;
use crate::span::Span;
use crate::types::{EnumDefinition, EnumVariantDefinition, StructDefinition, Type};

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
    pub structs: HashMap<String, StructDefinition>,
    pub enums: HashMap<String, EnumDefinition>,
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

impl TypeChecker {
    pub fn new() -> Self {
        let mut functions = HashMap::new();

        functions.insert(
            "print".to_string(),
            FunctionType {
                parameters: vec![Type::Unknown],
                return_type: Type::Void,
                generic_parameters: Vec::new(),
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

    fn types_compatible(&self, expected: &Type, found: &Type) -> bool {
        if expected == found {
            return true;
        }

        matches!(expected, Type::Unknown)
            || matches!(found, Type::Unknown)
            || matches!((expected, found), (Type::Generic(_), _))
            || matches!((expected, found), (_, Type::Generic(_)))
    }

    fn numeric_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Num | Type::Float | Type::Unknown)
    }

    fn convert_type(&self, name: &str, span: Span) -> Result<Type, FusionError> {
        match name {
            "num" => Ok(Type::Num),
            "float" => Ok(Type::Float),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),
            "void" => Ok(Type::Void),
            "File" => Ok(Type::File),

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
        }
    }

    fn convert_type_with_generics(
        &self,
        name: &str,
        generic_parameters: &[String],
        span: Span,
    ) -> Result<Type, FusionError> {
        if generic_parameters.iter().any(|parameter| parameter == name) {
            return Ok(Type::Generic(name.to_string()));
        }

        if let Some(inner) = name.strip_suffix("[]") {
            return Ok(Type::Array(Box::new(
                self.convert_type_with_generics(inner, generic_parameters, span)?,
            )));
        }

        if name.starts_with("Option<") && name.ends_with('>') {
            let inner = &name[7..name.len() - 1];

            return Ok(Type::Option(Box::new(
                self.convert_type_with_generics(inner, generic_parameters, span)?,
            )));
        }

        if name.starts_with("Result<") && name.ends_with('>') {
            let inner = &name[7..name.len() - 1];
            let parts = Self::split_generic_arguments(inner);

            if parts.len() != 2 {
                return Err(FusionError::Syntax {
                    message: "Result requires two type arguments".to_string(),
                    span,
                });
            }

            return Ok(Type::Result(
                Box::new(self.convert_type_with_generics(
                    parts[0],
                    generic_parameters,
                    span,
                )?),
                Box::new(self.convert_type_with_generics(
                    parts[1],
                    generic_parameters,
                    span,
                )?),
            ));
        }

        if name.starts_with("Iterator<") && name.ends_with('>') {
            let inner = &name[9..name.len() - 1];

            return Ok(Type::Iterator(Box::new(
                self.convert_type_with_generics(inner, generic_parameters, span)?,
            )));
        }

        if name.starts_with("Task<") && name.ends_with('>') {
            let inner = &name[5..name.len() - 1];

            return Ok(Type::Task(Box::new(
                self.convert_type_with_generics(inner, generic_parameters, span)?,
            )));
        }

        self.convert_type(name, span)
    }

    fn split_generic_arguments(value: &str) -> Vec<&str> {
        let mut result = Vec::new();
        let mut start = 0;
        let mut depth = 0;

        for (index, character) in value.char_indices() {
            match character {
                '<' => depth += 1,
                '>' => depth -= 1,
                ',' if depth == 0 => {
                    result.push(value[start..index].trim());
                    start = index + 1;
                }
                _ => {}
            }
        }

        let last = value[start..].trim();

        if !last.is_empty() {
            result.push(last);
        }

        result
    }

    fn substitute_generic_type(
        &self,
        ty: &Type,
        substitutions: &HashMap<String, Type>,
    ) -> Type {
        match ty {
            Type::Generic(name) => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| ty.clone()),

            Type::Array(inner) => Type::Array(Box::new(
                self.substitute_generic_type(inner, substitutions),
            )),

            Type::Iterator(inner) => Type::Iterator(Box::new(
                self.substitute_generic_type(inner, substitutions),
            )),

            Type::HashMap(key, value) => Type::HashMap(
                Box::new(self.substitute_generic_type(key, substitutions)),
                Box::new(self.substitute_generic_type(value, substitutions)),
            ),

            Type::Option(inner) => Type::Option(Box::new(
                self.substitute_generic_type(inner, substitutions),
            )),

            Type::Result(ok, error) => Type::Result(
                Box::new(self.substitute_generic_type(ok, substitutions)),
                Box::new(self.substitute_generic_type(error, substitutions)),
            ),

            Type::Task(inner) => Type::Task(Box::new(
                self.substitute_generic_type(inner, substitutions),
            )),

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

        match operator {
            Plus => {
                if left == Type::String && right == Type::String {
                    return Ok(Type::String);
                }

                if self.numeric_type(&left) && self.numeric_type(&right) {
                    if left == Type::Float || right == Type::Float {
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

                if left == Type::Float || right == Type::Float {
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
                let definition = self.environment.structs.get(&name).ok_or_else(|| {
                    FusionError::Syntax {
                        message: format!("Unknown struct '{}'", name),
                        span,
                    }
                })?;

                definition
                    .fields
                    .iter()
                    .find(|(field_name, _)| field_name == property)
                    .map(|(_, field_type)| field_type.clone())
                    .ok_or_else(|| FusionError::Syntax {
                        message: format!(
                            "Struct '{}' has no property '{}'",
                            name, property
                        ),
                        span,
                    })
            }

            Type::Unknown => Ok(Type::Unknown),

            other => Err(FusionError::Syntax {
                message: format!(
                    "Type '{}' has no properties",
                    other.name()
                ),
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
                        let definition =
                            self.environment.structs.get(struct_name).ok_or_else(|| {
                                FusionError::Syntax {
                                    message: format!(
                                        "Unknown struct '{}'",
                                        struct_name
                                    ),
                                    span,
                                }
                            })?;

                        if definition
                            .fields
                            .iter()
                            .any(|(name, _)| name == property)
                        {
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
                        message: format!(
                            "Type '{}' has no mutable properties",
                            object_type.name()
                        ),
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

            PatternKind::Identifier(name) => {
                bindings.push((name.clone(), expected.clone()));
            }

            PatternKind::Number(_) => {
                if !matches!(expected, Type::Num | Type::Unknown) {
                    return Err(self.type_mismatch(
                        "num",
                        expected.name(),
                        pattern.span,
                    ));
                }
            }

            PatternKind::Float(_) => {
                if !matches!(expected, Type::Float | Type::Unknown) {
                    return Err(self.type_mismatch(
                        "float",
                        expected.name(),
                        pattern.span,
                    ));
                }
            }

            PatternKind::String(_) => {
                if !matches!(expected, Type::String | Type::Unknown) {
                    return Err(self.type_mismatch(
                        "string",
                        expected.name(),
                        pattern.span,
                    ));
                }
            }

            PatternKind::Boolean(_) => {
                if !matches!(expected, Type::Bool | Type::Unknown) {
                    return Err(self.type_mismatch(
                        "bool",
                        expected.name(),
                        pattern.span,
                    ));
                }
            }

            PatternKind::Variant { name, bindings: names } => {
                let enum_name = match expected {
                    Type::Enum(name) => name,
                    Type::Unknown => {
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
                    self.environment.enums.get(enum_name).ok_or_else(|| {
                        FusionError::Syntax {
                            message: format!(
                                "Unknown enum '{}'",
                                enum_name
                            ),
                            span: pattern.span,
                        }
                    })?;

                let variant =
                    definition.variants.get(name).ok_or_else(|| {
                        FusionError::Syntax {
                            message: format!(
                                "Enum '{}' has no variant '{}'",
                                enum_name, name
                            ),
                            span: pattern.span,
                        }
                    })?;

                if variant.fields.len() != names.len() {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "Variant '{}' expects {} bindings, found {}",
                            name,
                            variant.fields.len(),
                            names.len()
                        ),
                        span: pattern.span,
                    });
                }

                for (binding, field_type) in names.iter().zip(&variant.fields) {
                    bindings.push((binding.clone(), field_type.clone()));
                }
            }
        }

        let mut seen = HashSet::new();

        for (name, _) in &bindings {
            if !seen.insert(name.clone()) {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Duplicate pattern binding '{}'",
                        name
                    ),
                    span: pattern.span,
                });
            }
        }

        Ok(bindings)
    }

    // ---------------------------------------------------------------------
    // Expression inference
    // ---------------------------------------------------------------------

    pub fn infer_expression(
        &mut self,
        expression: &Expression,
    ) -> Result<Type, FusionError> {
        match expression {
            Expression::Number { .. } => Ok(Type::Num),

            Expression::Float { .. } => Ok(Type::Float),

            Expression::Boolean { .. } => Ok(Type::Bool),

            Expression::String { .. } => Ok(Type::String),

            Expression::Identifier { name, span } => self
                .lookup_variable(name)
                .map(|info| info.ty.clone())
                .ok_or_else(|| self.unknown_variable(name, *span)),

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

            Expression::Index {
                array,
                index,
                span,
            } => {
                let array_type = self.infer_expression(array)?;
                let index_type = self.infer_expression(index)?;

                if !matches!(index_type, Type::Num | Type::Unknown) {
                    return Err(self.type_mismatch(
                        "num",
                        index_type.name(),
                        index.span(),
                    ));
                }

                match array_type {
                    Type::Array(inner) => Ok(*inner),

                    Type::Iterator(inner) => Ok(*inner),

                    Type::Unknown => Ok(Type::Unknown),

                    other => Err(FusionError::Syntax {
                        message: format!(
                            "Cannot index value of type '{}'",
                            other.name()
                        ),
                        span: *span,
                    }),
                }
            }

            Expression::Property {
                object,
                name,
                span,
            } => self.property_type(object, name, *span),

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

            Expression::Binary {
                left,
                operator,
                right,
                span,
            } => {
                let left_type = self.infer_expression(left)?;
                let right_type = self.infer_expression(right)?;

                self.check_binary_operator(
                    left_type,
                    *operator,
                    right_type,
                    *span,
                )
            }

            Expression::Call {
                name,
                arguments,
                generic_arguments,
                span,
            } => self.infer_call(
                name,
                arguments,
                generic_arguments,
                *span,
            ),

            Expression::MethodCall {
                object,
                method,
                arguments,
                generic_arguments,
                span,
            } => self.infer_method_call(
                object,
                method,
                arguments,
                generic_arguments,
                *span,
            ),

            Expression::StructConstructor {
                name,
                fields,
                span,
            } => self.infer_struct_constructor(name, fields, *span),

            Expression::EnumConstructor {
                enum_name,
                variant,
                arguments,
                span,
            } => self.infer_enum_constructor(
                enum_name,
                variant,
                arguments,
                *span,
            ),
        }
    }

    fn infer_call(
        &mut self,
        name: &str,
        arguments: &[Expression],
        generic_arguments: &[String],
        span: Span,
    ) -> Result<Type, FusionError> {
        if let Some(struct_definition) = self.environment.structs.get(name).cloned() {
            if !generic_arguments.is_empty() {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Struct '{}' does not accept generic arguments",
                        name
                    ),
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

        let function = self
            .environment
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| FusionError::Syntax {
                message: format!("Unknown function '{}'", name),
                span,
            })?;

        if arguments.len() != function.parameters.len() {
            return Err(FusionError::Syntax {
                message: format!(
                    "Function '{}' expects {} arguments, found {}",
                    name,
                    function.parameters.len(),
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
                    message: format!(
                        "Function '{}' does not accept generic arguments",
                        name
                    ),
                    span,
                });
            }
        }

        let mut substitutions = HashMap::new();

        for (generic_name, generic_argument) in function
            .generic_parameters
            .iter()
            .zip(generic_arguments)
        {
            let generic_type = self.convert_type(
                generic_argument,
                span,
            )?;

            substitutions.insert(
                generic_name.clone(),
                generic_type,
            );
        }

        for (parameter_type, argument) in
            function.parameters.iter().zip(arguments)
        {
            let expected =
                self.substitute_generic_type(parameter_type, &substitutions);

            let found = self.infer_expression(argument)?;

            if !self.types_compatible(&expected, &found) {
                return Err(self.type_mismatch(
                    expected.name(),
                    found.name(),
                    argument.span(),
                ));
            }
        }

        Ok(self.substitute_generic_type(
            &function.return_type,
            &substitutions,
        ))
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

        match object_type {
            Type::Array(inner) => {
                match method {
                    "push" => {
                        if arguments.len() != 1 {
                            return Err(FusionError::Syntax {
                                message: "push expects one argument".to_string(),
                                span,
                            });
                        }

                        let argument_type =
                            self.infer_expression(&arguments[0])?;

                        if !self.types_compatible(
                            &inner,
                            &argument_type,
                        ) {
                            return Err(self.type_mismatch(
                                inner.name(),
                                argument_type.name(),
                                arguments[0].span(),
                            ));
                        }

                        Ok(Type::Void)
                    }

                    "pop" => {
                        if !arguments.is_empty() {
                            return Err(FusionError::Syntax {
                                message: "pop expects no arguments".to_string(),
                                span,
                            });
                        }

                        Ok(Type::Option(inner))
                    }

                    "clear" => {
                        if !arguments.is_empty() {
                            return Err(FusionError::Syntax {
                                message: "clear expects no arguments".to_string(),
                                span,
                            });
                        }

                        Ok(Type::Void)
                    }

                    "length" => {
                        if !arguments.is_empty() {
                            return Err(FusionError::Syntax {
                                message: "length expects no arguments".to_string(),
                                span,
                            });
                        }

                        Ok(Type::Num)
                    }

                    "contains" => {
                        if arguments.len() != 1 {
                            return Err(FusionError::Syntax {
                                message: "contains expects one argument".to_string(),
                                span,
                            });
                        }

                        let argument_type =
                            self.infer_expression(&arguments[0])?;

                        if !self.types_compatible(
                            &inner,
                            &argument_type,
                        ) {
                            return Err(self.type_mismatch(
                                inner.name(),
                                argument_type.name(),
                                arguments[0].span(),
                            ));
                        }

                        Ok(Type::Bool)
                    }

                    "sort" | "reverse" => {
                        if !arguments.is_empty() {
                            return Err(FusionError::Syntax {
                                message: format!(
                                    "{} expects no arguments",
                                    method
                                ),
                                span,
                            });
                        }

                        Ok(Type::Void)
                    }

                    "find" => Ok(Type::Option(inner)),

                    "any" | "all" => Ok(Type::Bool),

                    "count" => Ok(Type::Num),

                    "take" | "skip" => {
                        if arguments.len() != 1 {
                            return Err(FusionError::Syntax {
                                message: format!(
                                    "{} expects one argument",
                                    method
                                ),
                                span,
                            });
                        }

                        let amount_type =
                            self.infer_expression(&arguments[0])?;

                        if !matches!(
                            amount_type,
                            Type::Num | Type::Unknown
                        ) {
                            return Err(self.type_mismatch(
                                "num",
                                amount_type.name(),
                                arguments[0].span(),
                            ));
                        }

                        Ok(Type::Array(inner))
                    }

                    "map" | "filter" | "reduce" | "fold" | "collect"
                    | "zip" | "flatten" => {
                        // The AST currently has no lambda/closure expression,
                        // so these operations cannot be fully type-checked yet.
                        //
                        // Keep them represented in the type system without
                        // pretending we know the callback's type.
                        for argument in arguments {
                            let _ = self.infer_expression(argument)?;
                        }

                        Ok(Type::Unknown)
                    }

                    _ => Err(FusionError::Syntax {
                        message: format!(
                            "Unknown array method '{}'",
                            method
                        ),
                        span,
                    }),
                }
            }

            Type::Iterator(inner) => match method {
                "take" | "skip" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "{} expects one argument",
                                method
                            ),
                            span,
                        });
                    }

                    let amount_type =
                        self.infer_expression(&arguments[0])?;

                    if !matches!(
                        amount_type,
                        Type::Num | Type::Unknown
                    ) {
                        return Err(self.type_mismatch(
                            "num",
                            amount_type.name(),
                            arguments[0].span(),
                        ));
                    }

                    Ok(Type::Iterator(inner))
                }

                "count" => Ok(Type::Num),

                "find" => Ok(Type::Option(inner)),

                "any" | "all" => Ok(Type::Bool),

                "collect" => Ok(Type::Array(inner)),

                _ => Err(FusionError::Syntax {
                    message: format!(
                        "Unknown iterator method '{}'",
                        method
                    ),
                    span,
                }),
            },

            Type::Unknown => Ok(Type::Unknown),

            other => Err(FusionError::Syntax {
                message: format!(
                    "Type '{}' has no method '{}'",
                    other.name(),
                    method
                ),
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
        let definition = self
            .environment
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
                    message: format!(
                        "Duplicate field '{}'",
                        field_name
                    ),
                    span,
                });
            }

            let expected = definition
                .fields
                .iter()
                .find(|(name, _)| name == field_name)
                .map(|(_, ty)| ty.clone())
                .ok_or_else(|| FusionError::Syntax {
                    message: format!(
                        "Struct '{}' has no field '{}'",
                        name, field_name
                    ),
                    span,
                })?;

            let found = self.infer_expression(expression)?;

            if !self.types_compatible(&expected, &found) {
                return Err(self.type_mismatch(
                    expected.name(),
                    found.name(),
                    expression.span(),
                ));
            }
        }

        for (field_name, _) in &definition.fields {
            if !supplied.contains(field_name) {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Missing field '{}' in struct '{}'",
                        field_name, name
                    ),
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
        let definition = self
            .environment
            .enums
            .get(enum_name)
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
                    message: format!(
                        "Enum '{}' has no variant '{}'",
                        enum_name, variant
                    ),
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

        for (expected, argument) in
            variant_definition.fields.iter().zip(arguments)
        {
            let found = self.infer_expression(argument)?;

            if !self.types_compatible(expected, &found) {
                return Err(self.type_mismatch(
                    expected.name(),
                    found.name(),
                    argument.span(),
                ));
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
            Expression::Identifier { name, span: target_span } => {
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
    self.declare_variable(
        name.clone(),
        value_type,
        true,
        *target_span,
    )
}

            Expression::Property {
                object,
                name,
                span: property_span,
            } => {
                if !self.property_mutable(
                    object,
                    name,
                    *property_span,
                )? {
                    return Err(FusionError::CannotAssignToConst {
                        name: name.clone(),
                        span: *property_span,
                    });
                }

                let property_type =
                    self.property_type(object, name, *property_span)?;

                if !self.types_compatible(
                    &property_type,
                    &value_type,
                ) {
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

                if !matches!(
                    index_type,
                    Type::Num | Type::Unknown
                ) {
                    return Err(self.type_mismatch(
                        "num",
                        index_type.name(),
                        index.span(),
                    ));
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

                if !self.types_compatible(
                    &element_type,
                    &value_type,
                ) {
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

    // ---------------------------------------------------------------------
    // Statement checking
    // ---------------------------------------------------------------------

    fn check_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<(), FusionError> {
        match statement {
            Statement::VariableDeclarations {
                declarations,
                ..
            } => {
                for declaration in declarations {
                    let declared_type = match &declaration.declared_type {
                        Some(type_name) => Some(
                            self.convert_type(
                                type_name,
                                declaration.span,
                            )?,
                        ),
                        None => None,
                    };

                    let value_type = match &declaration.value {
                        Some(value) => {
                            Some(self.infer_expression(value)?)
                        }
                        None => None,
                    };

                    let final_type = match (declared_type, value_type) {
                        (Some(declared), Some(found)) => {
                            if !self.types_compatible(
                                &declared,
                                &found,
                            ) {
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
                        let declared =
                            self.convert_type(type_name, *span)?;

                        if !self.types_compatible(
                            &declared,
                            &value_type,
                        ) {
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

                self.declare_variable(
                    name.clone(),
                    final_type,
                    false,
                    *span,
                )
            }

            Statement::Assignment {
                target,
                value,
                span,
            } => self.check_assignment(
                target,
                value,
                *span,
            ),

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
            } => self.check_match(
                expression,
                arms,
                *span,
            ),

            Statement::Defer {
                expression,
                ..
            } => {
                self.infer_expression(expression)?;
                Ok(())
            }

            Statement::If {
                condition,
                body,
                else_body,
                ..
            } => {
                let condition_type =
                    self.infer_expression(condition)?;

                if !matches!(
                    condition_type,
                    Type::Bool | Type::Unknown
                ) {
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
                condition,
                body,
                ..
            } => {
                let condition_type =
                    self.infer_expression(condition)?;

                if !matches!(
                    condition_type,
                    Type::Bool | Type::Unknown
                ) {
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

                if !matches!(
                    start_type,
                    Type::Num | Type::Unknown
                ) {
                    return Err(self.type_mismatch(
                        "num",
                        start_type.name(),
                        start.span(),
                    ));
                }

                if !matches!(
                    end_type,
                    Type::Num | Type::Unknown
                ) {
                    return Err(self.type_mismatch(
                        "num",
                        end_type.name(),
                        end.span(),
                    ));
                }

                self.push_scope();

                let result = (|| {
                    self.declare_variable(
                        variable.clone(),
                        Type::Num,
                        true,
                        *span,
                    )?;

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
                let iterable_type =
                    self.infer_expression(iterable)?;

                let element_type = match iterable_type {
                    Type::Array(inner) => *inner,

                    Type::Iterator(inner) => *inner,

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
                    self.declare_variable(
                        variable.clone(),
                        element_type,
                        true,
                        *span,
                    )?;

                    self.loop_depth += 1;

                    let result = self.check_block(body);

                    self.loop_depth -= 1;

                    result
                })();

                self.pop_scope();

                result
            }

            Statement::Return {
                value,
                span,
            } => self.check_return(value.as_ref(), *span),

            Statement::Break { span } => {
                if self.loop_depth == 0 {
                    return Err(FusionError::Syntax {
                        message: "'break' is only valid inside a loop"
                            .to_string(),
                        span: *span,
                    });
                }

                Ok(())
            }

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

            Statement::Expression { expression, .. } => {
                self.infer_expression(expression)?;
                Ok(())
            }

            Statement::Call { expression, .. } => {
                self.infer_expression(expression)?;
                Ok(())
            }
        }
    }

    fn check_return(
        &mut self,
        value: Option<&Expression>,
        span: Span,
    ) -> Result<(), FusionError> {
        let context = self
            .current_function
            .as_ref()
            .ok_or_else(|| FusionError::Syntax {
                message: "'return' is only valid inside a function"
                    .to_string(),
                span,
            })?
            .clone();

        let value_type = match value {
            Some(expression) => {
                self.infer_expression(expression)?
            }

            None => Type::Void,
        };

        if let Some(expected) = context.declared_return_type {
            if !self.types_compatible(
                &expected,
                &value_type,
            ) {
                return Err(self.type_mismatch(
                    expected.name(),
                    value_type.name(),
                    span,
                ));
            }
        } else if let Some(existing) =
            self.current_function
                .as_ref()
                .and_then(|function| {
                    function.inferred_return_type.clone()
                })
        {
            if !self.types_compatible(
                &existing,
                &value_type,
            ) {
                return Err(self.type_mismatch(
                    existing.name(),
                    value_type.name(),
                    span,
                ));
            }
        } else if let Some(function) =
            self.current_function.as_mut()
        {
            function.inferred_return_type =
                Some(value_type);
        }

        if let Some(function) = self.current_function.as_mut() {
            function.has_return = true;
        }

        Ok(())
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

    // ---------------------------------------------------------------------
    // Match checking
    // ---------------------------------------------------------------------

    fn check_match(
        &mut self,
        expression: &Expression,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<(), FusionError> {
        let expression_type =
            self.infer_expression(expression)?;

        let mut has_wildcard = false;
        let mut covered_variants = HashSet::new();
        let mut seen_patterns = HashSet::new();

        for arm in arms {
            match &arm.pattern.kind {
                PatternKind::Wildcard => {
                    has_wildcard = true;
                }

                PatternKind::Variant { name, .. } => {
                    if !seen_patterns.insert(name.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Duplicate match pattern '{}'",
                                name
                            ),
                            span: arm.pattern.span,
                        });
                    }

                    covered_variants.insert(name.clone());
                }

                PatternKind::Boolean(value) => {
                    let key = value.to_string();

                    if !seen_patterns.insert(key) {
                        return Err(FusionError::Syntax {
                            message:
                                "Duplicate boolean match pattern"
                                    .to_string(),
                            span: arm.pattern.span,
                        });
                    }
                }

                PatternKind::Number(value) => {
                    let key = format!("num:{}", value);

                    if !seen_patterns.insert(key) {
                        return Err(FusionError::Syntax {
                            message:
                                "Duplicate numeric match pattern"
                                    .to_string(),
                            span: arm.pattern.span,
                        });
                    }
                }

                PatternKind::Float(value) => {
                    let key = format!("float:{:?}", value);

                    if !seen_patterns.insert(key) {
                        return Err(FusionError::Syntax {
                            message:
                                "Duplicate float match pattern"
                                    .to_string(),
                            span: arm.pattern.span,
                        });
                    }
                }

                PatternKind::String(value) => {
                    let key = format!("string:{}", value);

                    if !seen_patterns.insert(key) {
                        return Err(FusionError::Syntax {
                            message:
                                "Duplicate string match pattern"
                                    .to_string(),
                            span: arm.pattern.span,
                        });
                    }
                }

                PatternKind::Identifier(_) => {}
            }

            let bindings =
                self.check_pattern(&arm.pattern, &expression_type)?;

            self.push_scope();

            let result = (|| {
                for (name, binding_type) in bindings {
                    self.declare_variable(
                        name,
                        binding_type,
                        true,
                        arm.pattern.span,
                    )?;
                }

                self.check_block(&arm.body)
            })();

            self.pop_scope();

            result?;
        }

        if has_wildcard {
            return Ok(());
        }

        match expression_type {
            Type::Enum(enum_name) => {
                let definition =
                    self.environment.enums.get(&enum_name).ok_or_else(
                        || FusionError::Syntax {
                            message: format!(
                                "Unknown enum '{}'",
                                enum_name
                            ),
                            span,
                        },
                    )?;

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
                let has_true = seen_patterns.contains("true");
                let has_false = seen_patterns.contains("false");

                if !has_true || !has_false {
                    return Err(FusionError::Syntax {
                        message:
                            "Non-exhaustive boolean match"
                                .to_string(),
                        span,
                    });
                }
            }

            Type::Unknown => {}

            _ => {
                return Err(FusionError::Syntax {
                    message:
                        "Match over this type requires a wildcard arm"
                            .to_string(),
                    span,
                });
            }
        }

        Ok(())
    }

    // ---------------------------------------------------------------------
    // Return analysis
    // ---------------------------------------------------------------------

    fn block_returns(&self, statements: &[Statement]) -> bool {
        for statement in statements {
            match statement {
                Statement::Return { .. } => return true,

                Statement::If {
                    body,
                    else_body: Some(else_body),
                    ..
                } => {
                    if self.block_returns(body)
                        && self.block_returns(else_body)
                    {
                        return true;
                    }
                }

                Statement::Match { arms, .. } => {
                    if !arms.is_empty()
                        && arms
                            .iter()
                            .all(|arm| self.block_returns(&arm.body))
                    {
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
                    message: format!(
                        "Duplicate generic parameter '{}'",
                        parameter
                    ),
                    span,
                });
            }
        }

        let declared_return_type = match return_type {
            Some(type_name) => Some(
                self.convert_type_with_generics(
                    type_name,
                    generic_parameters,
                    span,
                )?,
            ),

            None => None,
        };

        let previous_function =
            self.current_function.take();

        self.current_function = Some(FunctionContext {
            declared_return_type,
            inferred_return_type: None,
            has_return: false,
        });

        self.push_scope();

        let result = (|| {
            for parameter in parameters {
                let parameter_type = match &parameter.type_name {
                    Some(type_name) => self
                        .convert_type_with_generics(
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
                && context.declared_return_type
                    != Some(Type::Void)
                && !context.has_return
            {
                return Err(FusionError::Syntax {
                    message: format!(
                        "Function '{}' must return a value",
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
                    message: format!(
                        "Function '{}' may reach the end without returning",
                        name
                    ),
                    span,
                });
            }

            Ok(())
        })();

        self.pop_scope();
        self.current_function = previous_function;

        if result.is_err() {
            return result;
        }

        let _ = is_async;

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
                    message: format!(
                        "Duplicate parameter '{}'",
                        parameter.name
                    ),
                    span: parameter.span,
                });
            }

            if let Some(type_name) = &parameter.type_name {
                self.convert_type_with_generics(
                    type_name,
                    generic_parameters,
                    parameter.span,
                )?;
            }
        }

        if let Some(return_type) = &method.return_type {
            self.convert_type_with_generics(
                return_type,
                generic_parameters,
                method.span,
            )?;
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
        if !self.environment.structs.contains_key(type_name)
            && !self.environment.enums.contains_key(type_name)
        {
            return Err(FusionError::Syntax {
                message: format!(
                    "Cannot implement methods for unknown type '{}'",
                    type_name
                ),
                span,
            });
        }

        if let Some(trait_name) = trait_name {
            if !self.environment.functions.contains_key(trait_name)
                && !self.environment.structs.contains_key(trait_name)
                && !self.environment.enums.contains_key(trait_name)
            {
                // Traits are not stored as a separate environment entry
                // in the current TypeEnvironment, so trait existence is
                // validated during the main AST pass.
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
                } => {
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
                        message:
                            "Impl blocks may only contain functions"
                                .to_string(),
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

    pub fn check(
        &mut self,
        program: &Program,
    ) -> Result<(), FusionError> {
        // Pass 1: register struct names.
        for statement in &program.statements {
            if let Statement::Struct {
                name,
                span,
                ..
            } = statement
            {
                if self.environment.structs.contains_key(name) {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "Duplicate struct '{}'",
                            name
                        ),
                        span: *span,
                    });
                }

                self.environment.structs.insert(
                    name.clone(),
                    StructDefinition {
                        fields: Vec::new(),
                    },
                );
            }
        }

        // Pass 2: register enum names.
        for statement in &program.statements {
            if let Statement::Enum {
                name,
                span,
                ..
            } = statement
            {
                if self.environment.enums.contains_key(name) {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "Duplicate enum '{}'",
                            name
                        ),
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
                name,
                fields,
                span,
            } = statement
            {
                let mut seen = HashSet::new();
                let mut resolved_fields = Vec::new();

                for field in fields {
                    if !seen.insert(field.name.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Duplicate field '{}'",
                                field.name
                            ),
                            span: field.span,
                        });
                    }

                    let field_type =
                        self.convert_type(&field.type_name, field.span)?;

                    resolved_fields.push((
                        field.name.clone(),
                        field_type,
                    ));
                }

                let definition =
                    self.environment.structs.get_mut(name).ok_or_else(
                        || FusionError::Syntax {
                            message: format!(
                                "Unknown struct '{}'",
                                name
                            ),
                            span: *span,
                        },
                    )?;

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
                            message: format!(
                                "Duplicate enum variant '{}'",
                                variant.name
                            ),
                            span: variant.span,
                        });
                    }

                    let mut resolved_fields = Vec::new();

                    for field_type_name in &variant.fields {
                        resolved_fields.push(
                            self.convert_type(
                                field_type_name,
                                variant.span,
                            )?,
                        );
                    }

                    resolved_variants.insert(
                        variant.name.clone(),
                        EnumVariantDefinition {
                            fields: resolved_fields,
                        },
                    );
                }

                let definition =
                    self.environment.enums.get_mut(name).ok_or_else(
                        || FusionError::Syntax {
                            message: format!(
                                "Unknown enum '{}'",
                                name
                            ),
                            span: *span,
                        },
                    )?;

                definition.variants = resolved_variants;
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
                span,
                ..
            } = statement
            {
                if self.environment.functions.contains_key(name) {
                    return Err(FusionError::Syntax {
                        message: format!(
                            "Duplicate function '{}'",
                            name
                        ),
                        span: *span,
                    });
                }

                let mut generic_set = HashSet::new();

                for generic in generic_parameters {
                    if !generic_set.insert(generic.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Duplicate generic parameter '{}'",
                                generic
                            ),
                            span: *span,
                        });
                    }
                }

                let mut parameter_types = Vec::new();
                let mut parameter_names = HashSet::new();

                for parameter in parameters {
                    if !parameter_names.insert(parameter.name.clone()) {
                        return Err(FusionError::Syntax {
                            message: format!(
                                "Duplicate parameter '{}'",
                                parameter.name
                            ),
                            span: parameter.span,
                        });
                    }

                    let parameter_type =
                        match &parameter.type_name {
                            Some(type_name) => self
                                .convert_type_with_generics(
                                    type_name,
                                    generic_parameters,
                                    parameter.span,
                                )?,

                            None => Type::Unknown,
                        };

                    parameter_types.push(parameter_type);
                }

                let base_return_type =
                    match return_type {
                        Some(type_name) => self
                            .convert_type_with_generics(
                                type_name,
                                generic_parameters,
                                *span,
                            )?,

                        None => Type::Void,
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
                    span,
                } => {
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

                Statement::Trait {
                    methods,
                    span,
                    ..
                } => {
                    for method in methods {
                        self.validate_trait_method(
                            method,
                            &[],
                        )?;
                    }

                    let _ = span;
                }

                Statement::Impl {
                    trait_name,
                    type_name,
                    methods,
                    span,
                } => {
                    self.check_impl(
                        trait_name.as_ref(),
                        type_name,
                        methods,
                        *span,
                    )?;
                }

                Statement::Struct { .. }
                | Statement::Enum { .. } => {}

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