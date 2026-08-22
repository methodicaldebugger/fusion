//contents of type_checker.rs
use std::collections::HashMap;
use crate::ast::*;
use crate::errors::*;
use crate::types::{Type, StructDefinition};

pub struct TypeEnvironment {
    pub scopes: Vec<HashMap<String, VariableInfo>>,
    pub functions: HashMap<String, FunctionType>,
    pub structs: HashMap<String, StructDefinition>,
}
struct FunctionContext {
    return_type: Option<Type>,
}
#[derive(Clone)]
pub struct VariableInfo {
    pub ty: Type,
    pub mutable: bool,
}
#[derive(Clone)]
pub struct FunctionType {
    pub parameters: Vec<Type>,
    pub return_type: Option<Type>,
}
pub struct TypeChecker {
    environment: TypeEnvironment,
    current_function: Option<FunctionContext>,
}
impl TypeChecker {

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
                    FusionError::UnknownVariable(struct_name.clone())
                })?;

            definition
                .fields
                .get(name)
                .cloned()
                .ok_or_else(|| {
                    FusionError::UnknownVariable(format!(
                        "Unknown field '{}' on struct '{}'",
                        name,
                        struct_name
                    ))
                })
        }

        other => Err(FusionError::TypeMismatch {
            expected: "struct".into(),
            found: format!("{:?}", other),
        }),
    }
}

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
) -> Result<(), FusionError> {
    let scope = self.environment.scopes.last_mut().unwrap();

    if scope.contains_key(&name) {
        return Err(FusionError::UnknownVariable(
            format!("Variable '{}' is already declared", name)
        ));
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

    

    fn property_root_name(
    expression: &Expression,
) -> Option<String> {
    match expression {
        Expression::Identifier(name) => {
            Some(name.clone())
        }

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
    let root_name = Self::property_root_name(expression)
        .ok_or_else(|| {
            FusionError::UnknownVariable(
                "Invalid property assignment target".into()
            )
        })?;

    let info = self
        .lookup_variable_info(&root_name)
        .ok_or_else(|| {
            FusionError::UnknownVariable(root_name.clone())
        })?;

    Ok(info.mutable)
}

    fn require_bool(&self,found: Type)
        -> Result<(), FusionError>
    {
        if found == Type::Bool {
            Ok(())
        }
        else {
            Err(
                FusionError::TypeMismatch {
                expected:
                "Bool".into(),
                found:
                format!("{:?}", found),
                }
            )
        }
    }

    fn operator_name(&self,operator: &Operator) -> String {
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

    pub fn new()->Self {
        let mut functions = HashMap::new();
        functions.insert(
            "print".into(),
            FunctionType {
                parameters: vec![Type::Unknown],
                return_type: Some(Type::Void),
            },
        );
        Self {
            environment: TypeEnvironment {
            scopes: vec![HashMap::new()],
            functions,
            structs: HashMap::new(),
            },
            current_function: None,
        }
    }

    fn convert_type(
        &self,
        name: &String,
        ) -> Result<Type, FusionError> {
            match name.as_str() {
            "num" => Ok(Type::Num),
            "float" => Ok(Type::Float),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),

        _ if self.environment.structs.contains_key(name) => {
            Ok(Type::Struct(name.clone()))
        }

        _ => Err(FusionError::TypeMismatch {
            expected: "known type".into(),
            found: name.clone(),
            }),
        }
    }

    fn block_returns(statements:&Vec<Statement>) -> bool {
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
                if let Some(else_body)=else_body {
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
    
    fn infer_expression(&self,expression:&Expression)-> Result<Type,FusionError>
    {
        match expression {
            Expression::StructConstructor {
                name,
                fields,
                } => {
                    let definition = self
                    .environment
                    .structs
                    .get(name)
                    .ok_or_else(|| {
                        FusionError::UnknownVariable(name.clone())
                    })?;

                // Check every supplied field.
                for (field_name, expression) in fields {
                    let expected_type = definition
                    .fields
                    .get(field_name)
                    .ok_or_else(|| {
                        FusionError::UnknownVariable(
                            format!(
                            "Unknown field '{}' on struct '{}'",
                            field_name,
                            name
                            )
                        )
                    })?;

                    let actual_type =
                    self.infer_expression(expression)?;

                    if *expected_type != actual_type {
                        return Err(
                            FusionError::TypeMismatch {
                                expected: format!(
                                "{:?}",
                                expected_type
                                ),
                            found: format!(
                                "{:?}",
                                actual_type
                                ),
                            }
                        );
                    }
                }

            // Make sure required fields were supplied.
            for field_name in definition.fields.keys() {
                if !fields
                    .iter()
                    .any(|(name, _)| name == field_name)
                    {
                        return Err(
                            FusionError::UnknownVariable(
                            format!(
                            "Missing field '{}' in struct '{}'",
                            field_name,
                            name
                            )
                            )
                        );
                    }
            }
            Ok(Type::Struct(name.clone()))
        }
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
                    Operator::Plus |
                    Operator::Minus |
                    Operator::Multiply |
                    Operator::Divide =>
                    {
                        if left_type == Type::Num&& right_type == Type::Num
                        {
                            Ok(Type::Num)
                        }
                        else if left_type == Type::Float&& right_type == Type::Float
                        {
                            Ok(Type::Float)
                        }
                        else if operator == &Operator::Plus&& left_type == Type::String&& right_type == Type::String
                        {
                            Ok(Type::String)
                        }
                        else {
                            Err(FusionError::InvalidOperation {
                                left:
                                format!("{:?}",left_type),
                                operator:
                                self.operator_name(operator),
                                right:
                                format!("{:?}",right_type),
                            }
                        )
                    }
                }
                Operator::Equal |
                Operator::NotEqual =>
                {
                    if left_type == right_type {
                        Ok(Type::Bool)
                    }
                    else {
                        Err(
                        FusionError::InvalidOperation {
                        left: format!("{:?}", left_type),
                        operator: self.operator_name(operator),
                        right: format!("{:?}", right_type),
                        }
                        )
                    }
                }
                Operator::Less |
                Operator::LessEqual |
                Operator::Greater |
                Operator::GreaterEqual =>
                {
                if (left_type == Type::Num && right_type == Type::Num)
                || (left_type == Type::Float && right_type == Type::Float)
                {
                    Ok(Type::Bool)
                }
                else
                {
                    Err(
                    FusionError::InvalidOperation {
                        left:
                        format!("{:?}", left_type),
                        operator:
                        self.operator_name(operator),
                        right:
                        format!("{:?}", right_type),
                    }
                    )
                }
            }
            Operator::And |
            Operator::Or =>
            {
                if left_type == Type::Bool
                    && right_type == Type::Bool
        {
            Ok(Type::Bool)
        }
        else
        {
            Err(
                FusionError::InvalidOperation {
                    left:
                        format!("{:?}", left_type),
                    operator:
                        self.operator_name(operator),
                    right:
                        format!("{:?}", right_type),
                }
            )
        }
    }
    }
}
Expression::Number(_) =>Ok(Type::Num),
Expression::Float(_) =>Ok(Type::Float),
Expression::String(_) =>Ok(Type::String),
Expression::Boolean(_) =>Ok(Type::Bool),
Expression::Identifier(name)=>{
    match self.lookup_variable(name)
    {
        Some(t)=>Ok(t.clone()),
        None=>
        Err(FusionError::UnknownVariable(name.clone()))
    }
}
Expression::Unary {
    operator,
    expression,
} => {
    let inner_type =
        self.infer_expression(expression)?;
    match operator {
        UnaryOperator::Negate => {
            if inner_type == Type::Num
            || inner_type == Type::Float
            {
                Ok(inner_type)
            }
            else {
                Err(
                    FusionError::InvalidOperation {
                        left:format!("{:?}", inner_type),
                        operator:"-".into(),
                        right:"".into(),
                    }
                )
            }
        }
        UnaryOperator::Not => {
            if inner_type == Type::Bool
            {
                Ok(Type::Bool)
            }
            else {
                Err(
                    FusionError::InvalidOperation {
                        left:format!("{:?}", inner_type),
                        operator:"!".into(),
                        right:"".into(),
                    }
                )
            }
        }
    }
}
Expression::Call {
    name,
    arguments,
    ..
} => {
    let function =
        match self.environment.functions.get(name)
        {
            Some(f) => f,
            None =>
            return Err(FusionError::UnknownVariable(name.clone())),
        };
    if arguments.len() != function.parameters.len()
    {
        return Err(
            FusionError::TypeMismatch {
                expected:
                format!("{} arguments",function.parameters.len()),
                found:
                format!("{} arguments",arguments.len()),
            }
        );
    }
    for (argument, expected_type)
    in arguments.iter().zip(function.parameters.iter())
    {
        let actual =
        self.infer_expression(argument)?;
        if *expected_type != Type::Unknown
&& &actual != expected_type {
            return Err(
                FusionError::TypeMismatch {
                    expected:
                    format!("{:?}", expected_type),
                    found:
                    format!("{:?}", actual),
                }
            );
        }
    }
    match &function.return_type {
    Some(t) => Ok(t.clone()),
    None => Ok(Type::Void),
}
}
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
            return Err(
                FusionError::TypeMismatch {
                    expected:format!("{:?}", first_type),
                    found:format!("{:?}", value_type),
                }
            );
        }
    }
    Ok(
        Type::Array(Box::new(first_type))
    )
}
Expression::Index {
    array,
    index,
} => {
    let array_type =
        self.infer_expression(array)?;
    let index_type =
        self.infer_expression(index)?;
    if index_type != Type::Num {
        return Err(
            FusionError::TypeMismatch {
                expected:"num index".into(),
                found:format!("{:?}", index_type),
            }
        );
    }
    match array_type {
        Type::Array(inner) =>Ok(*inner),
        other =>
            Err(
                FusionError::TypeMismatch {
                    expected:"array".into(),
                    found:format!("{:?}", other),
                }
            )
    }
}
    Expression::Property { object, name } => {
    self.lookup_property_type(object, name)
}

    Expression::MethodCall {
    object,
    method,
    arguments,
} => {
    let object_type = self.infer_expression(object)?;

    match object_type {
        Type::Array(element_type) => {
            match method.as_str() {
                "push" => {
                    if arguments.len() != 1 {
                        return Err(FusionError::TypeMismatch {
                            expected: "1 argument".into(),
                            found: format!(
                                "{} arguments",
                                arguments.len()
                            ),
                        });
                    }

                    let argument_type =
                        self.infer_expression(&arguments[0])?;

                    if *element_type != Type::Unknown
                        && argument_type != *element_type
                    {
                        return Err(FusionError::TypeMismatch {
                            expected: format!("{:?}", element_type),
                            found: format!("{:?}", argument_type),
                        });
                    }

                    Ok(Type::Void)
                }

                "pop" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::TypeMismatch {
                            expected: "0 arguments".into(),
                            found: format!(
                                "{} arguments",
                                arguments.len()
                            ),
                        });
                    }

                    Ok(*element_type)
                }

                "length" => {
                    if !arguments.is_empty() {
                        return Err(FusionError::TypeMismatch {
                            expected: "0 arguments".into(),
                            found: format!(
                                "{} arguments",
                                arguments.len()
                            ),
                        });
                    }

                    Ok(Type::Num)
                }

                _ => {
                    Err(FusionError::UnknownVariable(format!(
                        "Unknown array method '{}'",
                        method
                    )))
                }
            }
        }

        other => {
            Err(FusionError::TypeMismatch {
                expected: "array".into(),
                found: format!("{:?}", other),
            })
        }
    }
}

}
}








pub fn check_statement(&mut self,statement:&Statement)
    ->Result<(),FusionError>
    {
        match statement {
            Statement::Assignment {
    target,
    value,
} => {
    let inferred = self.infer_expression(value)?;

    match target {
        Expression::Identifier(name) => {
    match self.lookup_variable_info(name) {
        Some(info) => {
            if !info.mutable {
                return Err(
                    FusionError::CannotAssignToConst(name.clone())
                );
            }

            if info.ty != inferred {
                return Err(
                    FusionError::TypeMismatch {
                        expected: format!("{:?}", info.ty),
                        found: format!("{:?}", inferred),
                    }
                );
            }

            Ok(())
        }

        None => {
            self.declare_variable(
                name.clone(),
                inferred,
                true,
            )?;

            Ok(())
        }
    }
}

        Expression::Property { object, name } => {
    // The root variable of the entire property chain
    // must be mutable.
    if !self.property_target_is_mutable(object)? {
        let variable_name =
            Self::property_root_name(object)
                .unwrap_or_else(|| "<unknown>".into());

        return Err(
            FusionError::CannotAssignToConst(variable_name)
        );
    }

    let field_type =
        self.lookup_property_type(object, name)?;

    if field_type != inferred {
        return Err(FusionError::TypeMismatch {
            expected: format!("{:?}", field_type),
            found: format!("{:?}", inferred),
        });
    }

    Ok(())
}

        _ => {
            Err(FusionError::UnknownVariable(
                "invalid assignment target".into()
            ))
        }
    }
}
Statement::Call(expression) => {
    self.infer_expression(expression)?;
    Ok(())
}

    Statement::VariableDeclaration {
    name,
    declared_type,
    value,
} => {
    let inferred = self.infer_expression(value)?;

    let ty = match declared_type {
        Some(type_name) => {
            let declared =
                self.convert_type(type_name)?;

            if declared != inferred {
                return Err(
                    FusionError::TypeMismatch {
                        expected: format!("{:?}", declared),
                        found: format!("{:?}", inferred),
                    }
                );
            }

            declared
        }

        None => inferred,
    };

    self.declare_variable(
    name.clone(),
    ty,
    true,
    )?;

    Ok(())
}

Statement::If {
    condition,
    body,
    else_body,
} => {
    let condition_type =
        self.infer_expression(condition)?;
    self.require_bool(condition_type)?;

    // IF body gets its own scope.
    self.push_scope();

    for statement in body {
        if let Err(error) = self.check_statement(statement) {
        self.pop_scope();
        return Err(error);
    }
    }

    self.pop_scope();

    // ELSE body gets its own independent scope.
    if let Some(else_statements) = else_body {
    self.push_scope();

    for statement in else_statements {
        if let Err(error) = self.check_statement(statement) {
            self.pop_scope();
            return Err(error);
        }
    }

    self.pop_scope();
}

    Ok(())
}

    Statement::ConstDeclaration {
    name,
    declared_type,
    value,
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
                        expected: format!("{:?}", declared),
                        found: format!("{:?}", inferred),
                    }
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
    )?;

    Ok(())
}

Statement::Return(expression) => {
    let actual =
        self.infer_expression(expression)?;
    match &self.current_function {
        Some(context) => {
            match &context.return_type {
                Some(expected) => {
                    if expected != &actual {
                        return Err(
                            FusionError::TypeMismatch {
                                expected:format!("{:?}", expected),
                                found:format!("{:?}", actual),
                            }
                        );
                    }
                }
                None => {
                    return Err(
                        FusionError::TypeMismatch {
                            expected:"no return value".into(),
                            found:format!("{:?}", actual),
                        }
                    );
                }
            }
        }
        None => {
            return Err(
                FusionError::TypeMismatch {
                    expected:"inside function".into(),
                    found:"return".into(),
                }
            );
        }
    }
    Ok(())
}
    Statement::While {
    condition,
    body,
} => {
    let condition_type =
        self.infer_expression(condition)?;

    self.require_bool(condition_type)?;

    self.push_scope();

    for statement in body {
        if let Err(error) = self.check_statement(statement) {
        self.pop_scope();
        return Err(error);
    }
    }

    self.pop_scope();

    Ok(())
}
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
        return Err(
            FusionError::TypeMismatch {
                expected:"num range".into(),
                found:format!("{:?}..{:?}",start_type,end_type),
            }
        );
    }
    self.push_scope();
    self.declare_variable(
        variable.clone(),
        Type::Num,
      true,)?;
    for statement in body {
        if let Err(error) = self.check_statement(statement) {
        self.pop_scope();
        return Err(error);
    }
    }
    self.pop_scope();
    Ok(())
}
Statement::Expression(expression) => {
    self.infer_expression(expression)?;
    Ok(())
}

_ => Ok(())

}
}
pub fn check(
    &mut self,
    program: &Program,
) -> Result<(), FusionError> {

    // Pass 1: register struct names
    for statement in &program.statements {
        if let Statement::Struct { name, .. } = statement {
            if self.environment.structs.contains_key(name) {
                return Err(
                    FusionError::UnknownVariable(
                        format!(
                            "Struct '{}' is already defined",
                            name
                        )
                    )
                );
            }

            self.environment.structs.insert(
                name.clone(),
                StructDefinition {
                    fields: HashMap::new(),
                },
            );
        }
    }

    // Pass 2: resolve struct fields
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
                        FusionError::UnknownVariable(
                            format!(
                                "Duplicate field '{}' in struct '{}'",
                                field.name,
                                name
                            )
                        )
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
                .unwrap()
                .fields = field_map;
        }
    }

    // Pass 3: register functions
    for statement in &program.statements {
    if let Statement::Function {
        name,
        parameters,
        return_type,
        ..
    } = statement
    {
        if self.environment.functions.contains_key(name) {
            return Err(FusionError::UnknownVariable(
                format!("Function '{}' is already defined", name)
            ));
        }

        let mut parameter_types = Vec::new();

        for parameter in parameters {
            let ty = match &parameter.type_name {
                Some(type_name) => self.convert_type(type_name)?,
                None => Type::Unknown,
            };

            parameter_types.push(ty);
        }

        let return_ty = match return_type {
            Some(type_name) => Some(self.convert_type(type_name)?),
            None => None,
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

// Pass 4: check everything
for statement in &program.statements {
    match statement {
        Statement::Struct { .. } => {
            // Structs were already checked above.
        }

        Statement::Function {
            parameters,
            return_type,
            body,
            ..
        } => {
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
                )?;
            }

            let old_function = self.current_function.take();

            self.current_function = Some(FunctionContext {
                return_type: match return_type {
                    Some(type_name) => {
                        Some(self.convert_type(type_name)?)
                    }
                    None => None,
                },
            });

            for statement in body {
                if let Err(error) = self.check_statement(statement) {
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
                });
            }

            self.current_function = old_function;
            self.pop_scope();
        }

        _ => {
            self.check_statement(statement)?;
        }
    }
}

    Ok(())
}




}