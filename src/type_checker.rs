//contents of type_checker.rs
use std::collections::HashMap;
use crate::ast::*;
use crate::types::Type;
use crate::errors::*;
struct FunctionContext {
    return_type: Option<Type>,
}
pub struct TypeEnvironment {
    scopes: Vec<HashMap<String, Type>>,
    functions: HashMap<String, FunctionType>,
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
) {
    self.environment
        .scopes
        .last_mut()
        .unwrap()
        .insert(name, ty);
}
fn lookup_variable(&self,name: &str,) -> Option<Type> {
    for scope in self.environment.scopes.iter().rev() {
        if let Some(ty) = scope.get(name) {
            return Some(ty.clone());
        }
    }
    None
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
        },
        current_function: None,
    }
}
fn convert_type(&self,name:&String)-> Result<Type,FusionError>
{
    match name.as_str()
    {
        "int" =>Ok(Type::Int),
        "float" =>Ok(Type::Float),
        "char" => Ok(Type::Char),
        "bool" =>Ok(Type::Bool),
        "string" =>Ok(Type::String),
        _ =>
        Err(FusionError::TypeMismatch {
            expected:"known type".into(),
            found:name.clone(),})
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
                    if left_type == Type::Int&& right_type == Type::Int
                    {
                        Ok(Type::Int)
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
    if (left_type == Type::Int && right_type == Type::Int)
        ||
       (left_type == Type::Float && right_type == Type::Float)
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
Expression::Integer(_) =>Ok(Type::Int),
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
            if inner_type == Type::Int
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
Expression::Character(_) =>
Ok(Type::Char),
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
    if index_type != Type::Int {
        return Err(
            FusionError::TypeMismatch {
                expected:"int index".into(),
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
Expression::Property {object: _,name: _,} => {
    Err(FusionError::UnknownVariable("property access not implemented".into()))
}
Expression::MethodCall {object: _,method: _,arguments: _,} => {
    Err(FusionError::UnknownVariable("method call not implemented".into()))
}
}
}



pub fn check_statement(&mut self,statement:&Statement)
    ->Result<(),FusionError>
    {
        match statement {
            Statement::Assignment {
                name,
                declared_type,
                value
                }=>{
                let inferred =
                self.infer_expression(value)?;
                let final_type =
                match declared_type {
    Some(type_name) => {
        let declared =
        self.convert_type(type_name)?;
        if declared != inferred {
            return Err(
                FusionError::TypeMismatch {
                    expected:
                    format!("{:?}", declared),
                    found:
                    format!("{:?}", inferred),
                }
            );
        }
        declared
    }
    None => inferred,
};
    if let Some(old_type) = self.lookup_variable(name) {
    if old_type != final_type {
        return Err(
            FusionError::TypeMismatch {
                expected: format!("{:?}", old_type),
                found: format!("{:?}", final_type),
            }
        );
    }
}
self.declare_variable(name.clone(),final_type,);
Ok(())
}
Statement::Call(expression) => {
    self.infer_expression(expression)?;
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

for statement in body {
    self.check_statement(statement)?;
}

if let Some(else_statements) = else_body {
    for statement in else_statements {
        self.check_statement(statement)?;
    }
}
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
    if start_type != Type::Int
        || end_type != Type::Int
    {
        return Err(
            FusionError::TypeMismatch {
                expected:"int range".into(),
                found:format!("{:?}..{:?}",start_type,end_type),
            }
        );
    }
    self.push_scope();
    self.declare_variable(
        variable.clone(),
        Type::Int,);
    for statement in body {
        self.check_statement(statement)?;
    }
    self.pop_scope();
    Ok(())
}
_ => Ok(())

}
}
pub fn check(
    &mut self,
    program: &Program,
) -> Result<(), FusionError> {
    // Pass 1: register functions
    for statement in &program.statements {
        if let Statement::Function {
            name,
            parameters,
            return_type,
            ..
        } = statement {
            let mut params = Vec::new();
            for parameter in parameters {
                let ty = match &parameter.type_name {
                    Some(name) =>
                        self.convert_type(name)?,
                    None =>Type::Unknown,
                };
                params.push(ty);
            }
            let ret = match return_type {
    Some(name) =>
        Some(self.convert_type(name)?),
    None =>
        None,
};
            self.environment.functions.insert(
                name.clone(),
                FunctionType {
                    parameters: params,
                    return_type: ret,
                },
            );
        }
    }
    // Pass 2: check function bodies and normal statements
    for statement in &program.statements {
        match statement {
            Statement::Function {
                parameters,
                return_type,
                body,
                ..
            } => {
                    self.push_scope();
                // insert parameters into function scope
                for parameter in parameters {
                    let ty = match &parameter.type_name {
                        Some(name) =>
                            self.convert_type(name)?,
                        None =>
                            Type::Unknown,
                    };
                    self.declare_variable(
                    parameter.name.clone(),
                    ty,
                    );
                }
                let old_function = self.current_function.take();
self.current_function = Some(FunctionContext {
    return_type: match return_type {
        Some(name) => Some(self.convert_type(name)?),
        None => None,
    }
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
&& !TypeChecker::block_returns(body)
{
    self.current_function = old_function;
    self.pop_scope();
    return Err(
        FusionError::TypeMismatch {
            expected:
            "function return value".into(),
            found:
            "no return".into(),
        }
    );
}
// restore function context
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