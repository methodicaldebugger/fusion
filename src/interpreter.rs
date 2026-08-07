//contents of interpreter.rs
use std::collections::HashMap;
use crate::ast::*;
use crate::value::Value;
use crate::environment::Environment;
#[derive(Debug)]
enum Flow {
    Normal,Break,Continue,Return(Value),
}
pub struct Interpreter {
    environment: Environment,
    functions: HashMap<String, Function>,
    loop_depth: usize,
}
#[derive(Clone)]
pub struct Function {
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub body: Vec<Statement>,
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
                println!("{}", value);
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
    if function.return_type.is_some()
    && !self.function_has_return(&function.body)
{
    panic!(
        "Function '{}' expects a return value but has no return statement",
        name
    );
}
    let values: Vec<Value> =
    arguments
    .iter()
    .map(|argument| self.evaluate(argument))
    .collect();
self.environment.push_scope();
for (parameter, value) 
in function.parameters.iter()
.zip(values.iter())
{
    self.environment.set(
        parameter.name.clone(),
        value.clone(),
    );
}
    for statement in &function.body {
    match self.execute_statement(statement) {
        Flow::Return(value) => {
    self.check_return_type(
        &function.return_type,
        &value,
    );
    self.environment.pop_scope();
    return value;
}
        Flow::Break => {
            panic!("break outside loop");
        }
        Flow::Continue => {
            panic!("continue outside loop");
        }
        Flow::Normal => {}
    }
}
    self.environment.pop_scope();
if function.return_type.is_some() {
    panic!("Function '{}' expected a return value", name);
}
Value::None
}
    fn function_has_return(
    &self,
    statements: &[Statement],
) -> bool {
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
                let if_returns =
                    self.function_has_return(body);
                let else_returns =
                    match else_body {
                        Some(body) =>
                            self.function_has_return(body),
                        None => false,
                    };
                if if_returns && else_returns {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
    pub fn new() -> Self {
    Self {
        environment: Environment::new(),
        functions: HashMap::new(),
        loop_depth: 0,
    }
}
    pub fn execute(&mut self, program:&Program) {
    // Pass 1: register functions
for statement in &program.statements {
    if let Statement::Function {
    name,
    parameters,
    body,
    return_type,
} = statement {
    self.functions.insert(
        name.clone(),
        Function {
            parameters: parameters.clone(),
            return_type: return_type.clone(),
            body: body.clone(),
        },
    );
}
    }
    // Pass 2: execute normal code
    for statement in &program.statements {
        match statement {
            Statement::Function { .. } => {
                continue;
            }
            _ => {
                match self.execute_statement(statement) {
    Flow::Return(value) => {
        println!("Program returned: {:?}", value);
        break;
    }
    _ => {}
}
            }
        }
    }
}
    fn execute_statement(
    &mut self,
    statement: &Statement,
) -> Flow {
    match statement {
        Statement::VariableDeclaration {
    name,
    value,
    ..
} => {
    let result = self.evaluate(value);
    self.environment.set(
        name.clone(),
        result,
    );
    Flow::Normal
}
Statement::ConstDeclaration {
    name,
    value,
    ..
} => {
    let result = self.evaluate(value);
    self.environment.set(
        name.clone(),
        result,
    );
    Flow::Normal
}
Statement::Expression(expr) => {
    self.evaluate(expr);
    Flow::Normal
}
        Statement::Break => {
    if self.loop_depth == 0 {
        panic!("break outside loop");
    }
    Flow::Break
}
Statement::Continue => {
    if self.loop_depth == 0 {
        panic!("continue outside loop");
    }
    Flow::Continue
}
        Statement::Assignment {
    name,
    value,
    ..
} => {
            let result = self.evaluate(value);
            self.environment.set(
    name.clone(),
    result,
);
            Flow::Normal
        }
        Statement::Call(expr) => {
            self.evaluate(expr);
            Flow::Normal
        }
       Statement::Return(expr) => {
            let value = self.evaluate(expr);
            Flow::Return(value)
        }
        Statement::For {
    variable,
    start,
    end,
    body,
} => {
    let start_value = self.evaluate(start);
    let end_value = self.evaluate(end);
    let start_number = match start_value {
        Value::Integer(v) => v,
        _ => panic!("For loop start must be integer"),
    };
    let end_number = match end_value {
        Value::Integer(v) => v,
        _ => panic!("For loop end must be integer"),
    };
    self.environment.push_scope();
    self.loop_depth += 1;
    for i in start_number..end_number {
        self.environment.set(
            variable.clone(),
            Value::Integer(i),
        );
        for statement in body {
    match self.execute_statement(statement) {
        Flow::Normal => {}
        Flow::Continue => {
            break;
        }
        Flow::Break => {
            self.loop_depth -= 1;
            self.environment.pop_scope();
            return Flow::Normal;
        }
        Flow::Return(value) => {
            self.loop_depth -= 1;
            self.environment.pop_scope();
            return Flow::Return(value);
        }
    }
}
    }
    self.loop_depth -= 1;
    self.environment.pop_scope();
    Flow::Normal
}
        Statement::While {
    condition,
    body,
} => {
    self.loop_depth += 1;
    loop {
        match self.evaluate(condition) {
            Value::Boolean(true) => {}
            Value::Boolean(false) => {
                break;
            }
            _ => {
                panic!("While condition must be boolean");
            }
        }
        for stmt in body {
            match self.execute_statement(stmt) {
                Flow::Normal => {}
                Flow::Continue => {
                    break;
                }
                Flow::Break => {
                    self.loop_depth -= 1;
                    return Flow::Normal;
                }
                Flow::Return(value) => {
                    self.loop_depth -= 1;
                    return Flow::Return(value);
                }
            }
        }
    }
    self.loop_depth -= 1;
    Flow::Normal
}
        Statement::If {
            condition,
            body,
            else_body,
        } => {
    let value = self.evaluate(condition);
    let statements = match value {
    Value::Boolean(true) =>
        Some(body),
    Value::Boolean(false) =>
        else_body.as_ref(),
    _ =>
        None,
};
    if let Some(statements) = statements {
        for stmt in statements {
            match self.execute_statement(stmt) {
    Flow::Normal => {}
    other => {
        return other;
    }
}
        }
    }
    Flow::Normal
}
        Statement::Function {
    name,
    parameters,
    body,
    return_type,
    ..
} => {
            self.functions.insert(
                name.clone(),
                Function {
    parameters: parameters.clone(),
    return_type: return_type.clone(),
    body: body.clone(),
}
            );
            Flow::Normal
        }
    }
}
    fn evaluate(&mut self, expr: &Expression) -> Value {
        match expr {
            Expression::Integer(v) => Value::Integer(*v),
            Expression::Float(v) => Value::Float(*v),
            Expression::String(v) => Value::String(v.clone()),
            Expression::Boolean(v) => Value::Boolean(*v),
            Expression::Character(value) => {
            Value::Character(*value)
            }
            Expression::Array(values) => {
                let mut result = Vec::new();
                for value in values {
                    result.push(
                    self.evaluate(value)
                    );
                }
                Value::Array(result)
            }
            Expression::Identifier(name) => {
    match self.environment.get(name) {
        Some(value)=>value.clone(),
        None=>{
            panic!(
                "Runtime error: unknown variable {}",
                name
            );
        }
    }
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
            Expression::Unary {
    operator,
    expression,
} => {
    let value = self.evaluate(expression);
    match operator {
        UnaryOperator::Negate => {
            match value {
                Value::Integer(v) =>
                    Value::Integer(-v),
                Value::Float(v) =>
                    Value::Float(-v),
                _ =>
                    Value::None,
            }
        }
        UnaryOperator::Not => {
            match value {
                Value::Boolean(v) =>
                    Value::Boolean(!v),
                _ =>
                    Value::None,
            }
        }
    }
}
            Expression::Call { name, arguments } => {
    self.call_function(name, arguments)
}
Expression::Index {
                array,
                index,
            } => {
                let array_value = self.evaluate(array);
                let index_value = self.evaluate(index);
                match (array_value, index_value) {
                    (Value::Array(values), Value::Integer(i)) => {
                        match values.get(i as usize) {
                            Some(value) => value.clone(),
                            None => panic!("Array index out of bounds"),
                        }
                    }
                    _ => panic!("Invalid array indexing"),
                }
            }
            Expression::Property {
                object,
                name,
            } => {
                panic!(
                    "Property access not implemented: {:?}.{}",
                    object,
                    name
                );
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
            } => {
                panic!(
                    "Method call not implemented: {:?}.{}({:?})",
                    object,
                    method,
                    arguments
                );
            }
        }
    }
    fn check_return_type(
    &self,
    expected: &Option<String>,
    value: &Value,
) {
    let Some(expected) = expected else {
        return;
    };
    let valid = match expected.as_str() {
        "int" => matches!(value, Value::Integer(_)),
        "float" => matches!(value, Value::Float(_)),
        "string" => matches!(value, Value::String(_)),
        "bool" => matches!(value, Value::Boolean(_)),
        _ => true,
    };
    if !valid {
        panic!(
            "Return type error: expected {}, got {:?}",
            expected,
            value
        );
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
                Operator::Divide => {
    if b == 0 {
        panic!("Division by zero");
    }
    Value::Integer(a / b)
}
                Operator::Equal => Value::Boolean(a == b),
                Operator::NotEqual => Value::Boolean(a != b),
                Operator::Greater => Value::Boolean(a > b),
                Operator::Less => Value::Boolean(a < b),
                Operator::GreaterEqual => Value::Boolean(a >= b),
                Operator::LessEqual => Value::Boolean(a <= b),
                Operator::And |
                Operator::Or =>
                Value::None,
            }
        }
        (Value::Float(a), Value::Float(b)) => {
    match operator {
        Operator::Plus =>
            Value::Float(a + b),
        Operator::Minus =>
            Value::Float(a - b),
        Operator::Multiply =>
            Value::Float(a * b),
        Operator::Divide =>
            Value::Float(a / b),
        Operator::And |
        Operator::Or =>
        Value::None,
        Operator::Equal =>
            Value::Boolean(a == b),
        Operator::NotEqual =>
            Value::Boolean(a != b),
        Operator::Greater =>
            Value::Boolean(a > b),
        Operator::Less =>
            Value::Boolean(a < b),
        Operator::GreaterEqual =>
            Value::Boolean(a >= b),
        Operator::LessEqual =>
            Value::Boolean(a <= b),
    }
}
        (Value::Boolean(a),Value::Boolean(b)) => {
    match operator {
        Operator::And =>
            Value::Boolean(a && b),
        Operator::Or =>
            Value::Boolean(a || b),
        Operator::Equal =>
            Value::Boolean(a==b),
        Operator::NotEqual =>
            Value::Boolean(a!=b),
        _ =>
            Value::None,
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