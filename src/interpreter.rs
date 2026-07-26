use std::collections::HashMap;
use crate::ast::*;

pub struct Interpreter {
    variables: HashMap<String, Value>,
    functions: HashMap<String, Function>,
}

#[derive(Clone)]
pub struct Function {
    pub parameters: Vec<String>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    None,
}

impl Interpreter {

    fn call_function(
    &mut self,
    name: &str,
    arguments: &[Expression],
) -> Value {

    // Built-in functions
    match name {

        "print" => {

            for argument in arguments {

                let value = self.evaluate(argument);

                println!("{:?}", value);

            }

            return Value::None;
        }


        _ => {}
    }


    // User-defined functions
    let function = match self.functions.get(name) {

        Some(f) => f.clone(),

        None => {
            panic!("Unknown function '{}'", name);
        }
    };


    let old_variables = self.variables.clone();


    for (parameter, argument) in function.parameters.iter()
        .zip(arguments.iter())
    {

        let value = self.evaluate(argument);

        self.variables.insert(
            parameter.clone(),
            value,
        );
    }


    for statement in &function.body {

        if let Some(value) =
            self.execute_statement(statement)
        {

            self.variables = old_variables;

            return value;
        }
    }


    self.variables = old_variables;

    Value::None
}
    
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    pub fn execute(&mut self, program: &Program) {

    for statement in &program.statements {

        if let Some(value) = self.execute_statement(statement) {

            println!("Program returned: {:?}", value);
            break;

        }

    }
}

    fn execute_statement(
    &mut self,
    statement: &Statement,
) -> Option<Value> {

    match statement {

        Statement::Assignment { name, value } => {

            let result = self.evaluate(value);

            self.variables.insert(
                name.clone(),
                result,
            );

            None
        }


        Statement::Call(expr) => {

            self.evaluate(expr);

            None
        }


        Statement::Return(expr) => {

            let value = self.evaluate(expr);

            Some(value)
        }


        Statement::If {
    condition,
    body,
    else_body,
} => {

    let value = self.evaluate(condition);

    let statements = match value {
        Value::Boolean(true) => body,
        _ => else_body,
    };


    for stmt in statements {

        if let Some(result) =
            self.execute_statement(stmt)
        {
            return Some(result);
        }

    }

    None
}


        Statement::Function {
            name,
            parameters,
            body,
            ..
        } => {

            self.functions.insert(
                name.clone(),
                Function {
                    parameters: parameters.clone(),
                    body: body.clone(),
                },
            );

            None
        }
    }

}

    fn evaluate(&mut self, expr: &Expression) -> Value {
        match expr {
            Expression::Integer(v) => Value::Integer(*v),

            Expression::Float(v) => Value::Float(*v),

            Expression::String(v) => Value::String(v.clone()),

            Expression::Boolean(v) => Value::Boolean(*v),

            Expression::Identifier(name) => {
                self.variables
                    .get(name)
                    .cloned()
                    .unwrap_or(Value::None)
            }

            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left);
                let right = self.evaluate(right);

                self.evaluate_binary(left, operator, right)
            }
            Expression::Call { name, arguments } => {
    self.call_function(name, arguments)
}

            _ => Value::None,
        }
    }

    fn evaluate_binary(
    &self,
    left: Value,
    operator: &Operator,
    right: Value,
) -> Value {

    match (left, right) {

        (Value::Integer(a), Value::Integer(b)) => {

            match operator {

                Operator::Plus => Value::Integer(a + b),
                Operator::Minus => Value::Integer(a - b),
                Operator::Multiply => Value::Integer(a * b),
                Operator::Divide => Value::Integer(a / b),

                Operator::Equal => Value::Boolean(a == b),
                Operator::NotEqual => Value::Boolean(a != b),
                Operator::Greater => Value::Boolean(a > b),
                Operator::Less => Value::Boolean(a < b),
                Operator::GreaterEqual => Value::Boolean(a >= b),
                Operator::LessEqual => Value::Boolean(a <= b),
            }
        }


        (Value::String(a), Value::String(b)) => {

            match operator {

                Operator::Plus =>
                    Value::String(format!("{}{}", a, b)),

                Operator::Equal =>
                    Value::Boolean(a == b),

                Operator::NotEqual =>
                    Value::Boolean(a != b),

                _ => Value::None,
            }
        }


        _ => Value::None,
    }
}
}