//contents of interpreter.rs
use std::collections::HashMap;
use crate::ast::*;
use crate::value::Value;
use crate::environment::Environment;
use crate::types::StructDefinition;


#[derive(Debug)]
enum Flow {
    Normal,Break,Continue,Return(Value),
}
pub struct Interpreter {
    enums: HashMap<String, Vec<String>>,
    environment: Environment,
    functions: HashMap<String, Function>,
    structs: HashMap<String, StructDefinition>,
    loop_depth: usize,
}
#[derive(Clone)]
pub struct Function {
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub body: Vec<Statement>,
}




impl Interpreter { // stores many functions


    fn assign_property(
    &mut self,
    object: &Expression,
    name: &str,
    value: Value,
) {
    match object {
        // Simple case:
        //
        // person.age = 31
        //
        // We replace the whole struct value stored in `person`.
        Expression::Identifier(variable_name) => {
            let object_value = self
                .environment
                .get(variable_name)
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "Runtime error: unknown variable '{}'",
                        variable_name
                    )
                });

            match object_value {
                Value::Struct {
                    name: struct_name,
                    mut fields,
                } => {
                    if !fields.contains_key(name) {
                        panic!(
                            "Unknown field '{}' on struct '{}'",
                            name,
                            struct_name
                        );
                    }

                    fields.insert(name.to_string(), value);

                    if let Err(error) =
                        self.environment.assign(
                            variable_name,
                            Value::Struct {
                                name: struct_name,
                                fields,
                            },
                        )
                    {
                        panic!("{}", error);
                    }
                }

                _ => {
                    panic!(
                        "Property assignment requires a struct"
                    );
                }
            }
        }

        // Nested case:
        //
        // person.address.city = "London"
        //
        // `object` here is:
        //
        // person.address
        //
        // We first evaluate that struct, modify it,
        // then recursively assign the modified struct
        // back into its parent.
        Expression::Property {
            object: parent,
            name: parent_field,
        } => {
            let object_value = self.evaluate(object);

            match object_value {
                Value::Struct {
                    name: struct_name,
                    mut fields,
                } => {
                    if !fields.contains_key(name) {
                        panic!(
                            "Unknown field '{}' on struct '{}'",
                            name,
                            struct_name
                        );
                    }

                    fields.insert(
                        name.to_string(),
                        value,
                    );

                    self.assign_property(
                        parent,
                        parent_field,
                        Value::Struct {
                            name: struct_name,
                            fields,
                        },
                    );
                }

                _ => {
                    panic!(
                        "Property assignment requires a struct"
                    );
                }
            }
        }

        _ => {
            panic!(
                "Invalid property assignment target"
            );
        }
    }
}




    fn check_parameter_type(
        &self,
        function_name: &str,
        parameter: &Parameter,
        value: &Value,
        ) {
            let Some(expected) = &parameter.type_name else {
                return;
            };

        let valid = match expected.as_str() {
            "num" => matches!(value, Value::Number(_)),
            "float" => matches!(value, Value::Float(_)),
            "string" => matches!(value, Value::String(_)),
            "bool" => matches!(value, Value::Boolean(_)),

            struct_name => {
                match value {
                    Value::Struct { name, .. } => {
                        name == struct_name
                }
                    _ => false,
                }
            }
        };

        if !valid {
            panic!(
                "Function '{}' parameter '{}' expects {}, got {:?}",
                function_name,
                parameter.name,
                expected,
                value
            );
        }
    }




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

        // User-defined function
        let function = match self.functions.get(name) {
            Some(f) => f.clone(),
            None => {
                panic!("Unknown function '{}'", name);
            }
        };

        // Check argument count.
        if arguments.len() != function.parameters.len() {
            panic!(
                "Function '{}' expects {} arguments, got {}",
                name,
                function.parameters.len(),
                arguments.len()
            );
        }

        // Evaluate arguments.
        let values: Vec<Value> = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect();

        // Check parameter types.
            for (parameter, value) in
                function.parameters.iter().zip(values.iter())
            {
                self.check_parameter_type(
                    name,
                    parameter,
                    value,
            );
        }

        // Create function scope.
        self.environment.push_scope();

        // Bind parameters.
        for (parameter, value) in
            function.parameters.iter().zip(values.iter())
        {
            self.environment.declare(
                parameter.name.clone(),
                value.clone(),
                true,
            );
        }

        // Execute body.
        for statement in &function.body {
            match self.execute_statement(statement) {
                Flow::Return(value) => {
                    self.check_return_type(
                        name,
                        &function.return_type,
                        &value,
                    );
                    self.environment.pop_scope();
                    return value;
                }
                Flow::Break => {
                    self.environment.pop_scope();
                    panic!(
                        "break outside loop in function '{}'",
                        name
                    );
                }
                Flow::Continue => {
                    self.environment.pop_scope();
                    panic!(
                        "continue outside loop in function '{}'",
                        name
                    );
                }
                Flow::Normal => {}
            }
        }

        // Function reached its end.
        self.environment.pop_scope();

        if function.return_type.is_some() {
            panic!(
                "Function '{}' expected a return value",
                name
            );
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
            structs: HashMap::new(),
            enums: HashMap::new(),
            loop_depth: 0,
        }
    }



    pub fn execute(&mut self, program:&Program) {
        // Pass 1: register functions
        // Register structs
        for statement in &program.statements {
        if let Statement::Struct {
            name,
            fields,
            } = statement
        {
            let mut field_map = HashMap::new();

            for field in fields {
                let field_type = match field.type_name.as_str() {
                    "num" => crate::types::Type::Num,
                    "float" => crate::types::Type::Float,
                    "bool" => crate::types::Type::Bool,
                    "string" => crate::types::Type::String,
                    other => crate::types::Type::Struct(other.to_string()),
                };

                field_map.insert(field.name.clone(), field_type);
            }

            self.structs.insert(
                name.clone(),
                StructDefinition {
                    fields: field_map,
                },
            );
        }
    }
    for statement in &program.statements {
        if let Statement::Function {
            name,
            parameters,
            body,
            return_type,
            ..
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
                    Flow::Return(_) => {
                        panic!("return outside function");
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
            }
        }
    }



    fn execute_statement(
    &mut self,
    statement: &Statement,
    ) -> Flow {
        match statement {
            Statement::VariableDeclarations { declarations } => {
    for declaration in declarations {
        let result = self.evaluate(&declaration.value);

        self.environment.declare(
            declaration.name.clone(),
            result,
            true,
        );
    }

    Flow::Normal
}
        Statement::ConstDeclaration {
            name,
            value,
            ..
            } => {
            let result = self.evaluate(value);

            self.environment.declare(
                name.clone(),
                result,
                false,
                );

            Flow::Normal
            }
        Statement::Enum {
    name,
    variants,
} => {
    self.enums.insert(
        name.clone(),
        variants
            .iter()
            .map(|variant| variant.name.clone())
            .collect(),
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
    target,
    value,
} => {
    let result = self.evaluate(value);

    match target {
        
        Expression::Identifier(name) => {
            if self.environment.get(name).is_some() {
                if let Err(error) =
                    self.environment.assign(name, result)
                {
                    panic!("{}", error);
                }
            } else {
                self.environment.declare(
                    name.clone(),
                    result,
                    true,
                );
            }
        }

        Expression::Property { object, name } => {
            self.assign_property(object, name, result);
        }

        _ => {
            panic!("Invalid assignment target");
        }
    }

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
                Value::Number(v) => v,
                _ => panic!("For loop start must be integer"),
            };
            let end_number = match end_value {
                Value::Number(v) => v,
                _ => panic!("For loop end must be integer"),
            };
        self.environment.push_scope();
        self.loop_depth += 1;
        for i in start_number..end_number {
            self.environment.set(
                variable.clone(),
                Value::Number(i),
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
            self.environment.push_scope();
            self.loop_depth += 1;

            loop {
                match self.evaluate(condition) {
                    Value::Boolean(true) => {}

                    Value::Boolean(false) => {
                        break;
                    }

                    _ => {
                        self.loop_depth -= 1;
                        self.environment.pop_scope();
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
    Statement::Match {
    expression,
    arms,
} => {
    let value = self.evaluate(expression);

    for arm in arms {
        if let Some(bindings) =
            self.pattern_matches(&arm.pattern, &value)
        {
            self.environment.push_scope();

            for (name, value) in bindings {
                self.environment.declare(
                    name,
                    value,
                    true,
                );
            }

            for statement in &arm.body {
                match self.execute_statement(statement) {
                    Flow::Normal => {}

                    Flow::Break => {
                        self.environment.pop_scope();
                        return Flow::Break;
                    }

                    Flow::Continue => {
                        self.environment.pop_scope();
                        return Flow::Continue;
                    }

                    Flow::Return(value) => {
                        self.environment.pop_scope();
                        return Flow::Return(value);
                    }
                }
            }

            self.environment.pop_scope();

            break;
        }
    }

    Flow::Normal
}
    
    Statement::If {
        condition,
        body,
        else_body,
    } => {
    let value = self.evaluate(condition);

    match value {
        Value::Boolean(true) => {
            self.environment.push_scope();

            for stmt in body {
                match self.execute_statement(stmt) {
                    Flow::Normal => {}
                    other => {
                        self.environment.pop_scope();
                        return other;
                    }
                }
            }

            self.environment.pop_scope();
        }

        Value::Boolean(false) => {
            if let Some(else_statements) = else_body {
                self.environment.push_scope();

                for stmt in else_statements {
                    match self.execute_statement(stmt) {
                        Flow::Normal => {}
                        other => {
                            self.environment.pop_scope();
                            return other;
                        }
                    }
                }

                self.environment.pop_scope();
            }
        }

        _ => {
            panic!("If condition must be boolean");
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
        _ => {
            Flow::Normal
            }
        }
        
    }


    fn pattern_matches(
    &self,
    pattern: &Pattern,
    value: &Value,
) -> Option<Vec<(String, Value)>> {
    match (pattern, value) {
        (Pattern::Wildcard, _) => {
            Some(Vec::new())
        }

        (Pattern::Identifier(name), value) => {
            Some(vec![(name.clone(), value.clone())])
        }

        (Pattern::Number(a), Value::Number(b)) if a == b => {
            Some(Vec::new())
        }

        (Pattern::Float(a), Value::Float(b)) if a == b => {
            Some(Vec::new())
        }

        (Pattern::String(a), Value::String(b)) if a == b => {
            Some(Vec::new())
        }

        (Pattern::Boolean(a), Value::Boolean(b)) if a == b => {
            Some(Vec::new())
        }

        (
            Pattern::Variant {
                name,
                bindings,
            },
            Value::Enum {
                enum_name,
                variant,
                values,
            },
        ) => {
            let expected = format!("{}::{}", enum_name, variant);

            if name != &expected {
                return None;
            }

            if bindings.len() != values.len() {
                return None;
            }

            let result = bindings
                .iter()
                .cloned()
                .zip(values.iter().cloned())
                .collect();

            Some(result)
        }

        _ => None,
    }
}


    fn evaluate(&mut self, expr: &Expression) -> Value {
        match expr {
            Expression::Number(v) => Value::Number(*v),
            Expression::Float(v) => Value::Float(*v),
            Expression::String(v) => Value::String(v.clone()),
            Expression::Boolean(v) => Value::Boolean(*v),
            Expression::StructConstructor { 
                name, fields } => {
                let mut result = HashMap::new();
                for (field_name, expression) in fields {
                        result.insert(
                    field_name.clone(),
                    self.evaluate(expression),
                    );
                }

                Value::Struct {
                    name: name.clone(),
                    fields: result,
                    }
                }
            
            Expression::EnumConstructor {
    enum_name,
    variant,
    arguments,
} => {
    let values = arguments
        .iter()
        .map(|argument| self.evaluate(argument))
        .collect();

    Value::Enum {
        enum_name: enum_name.clone(),
        variant: variant.clone(),
        values,
    }
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
                Value::Number(v) =>
                    Value::Number(-v),
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
            Expression::Call {
    name,
    arguments,
    ..
} => {
    self.call_function(name, arguments)
}
Expression::Index {
                array,
                index,
            } => {
                let array_value = self.evaluate(array);
                let index_value = self.evaluate(index);
                match (array_value, index_value) {
                    (Value::Array(values), Value::Number(i)) => {
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
    let value = self.evaluate(object);

    match value {
        Value::Struct { fields, .. } => {
            match fields.get(name) {
                Some(value) => value.clone(),

                None => {
                    panic!(
                        "Unknown field '{}'",
                        name
                    );
                }
            }
        }

        _ => {
            panic!(
                "Property access requires a struct"
            );
        }
    }
}
            Expression::MethodCall {
    object,
    method,
    arguments,
} => {
    let object_value = self.evaluate(object);

    match method.as_str() {
        "push" => {
            if arguments.len() != 1 {
                panic!("push() expects exactly 1 argument");
            }

            let value = self.evaluate(&arguments[0]);

            match object_value {
                Value::Array(mut values) => {
                    values.push(value);

                    // We need to put the mutated array back
                    // into the variable it came from.
                    if let Expression::Identifier(name) = object.as_ref() {
                        self.environment.assign(
                            name,
                            Value::Array(values),
                        ).unwrap();
                    } else {
                        panic!("push() requires an array variable");
                    }

                    Value::None
                }

                _ => {
                    panic!("push() can only be called on an array");
                }
            }
        }

        "pop" => {
    if !arguments.is_empty() {
        panic!("pop() expects no arguments");
    }

    match object.as_ref() {
        Expression::Identifier(name) => {
            let array = self
                .environment
                .get_mut(name)
                .expect("Unknown array variable");

            match array {
                Value::Array(values) => {
                    values.pop().unwrap_or_else(|| {
                        panic!("Cannot pop from an empty array")
                    })
                }

                _ => {
                    panic!("pop() can only be called on an array");
                }
            }
        }

        _ => {
            panic!("pop() requires an array variable");
        }
    }
}

        "length" => {
            if !arguments.is_empty() {
                panic!("length() expects no arguments");
            }

            match object_value {
                Value::Array(values) => {
                    Value::Number(values.len() as i64)
                }

                _ => {
                    panic!("length() can only be called on an array");
                }
            }
        }

        _ => {
            panic!(
                "Unknown method '{}'",
                method
            );
        }
    }
}
        }
    }






    fn check_return_type(
    &self,
    function_name: &str,
    expected: &Option<String>,
    value: &Value,
) {
    let Some(expected) = expected else {
        return;
    };

    let valid = match expected.as_str() {
        "num" => matches!(value, Value::Number(_)),
        "float" => matches!(value, Value::Float(_)),
        "string" => matches!(value, Value::String(_)),
        "bool" => matches!(value, Value::Boolean(_)),

        struct_name => {
            match value {
                Value::Struct { name, .. } => {
                    name == struct_name
                }

                _ => false,
            }
        }
    };

    if !valid {
        panic!(
            "Function '{}' return type error: expected {}, got {:?}",
            function_name,
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
        (Value::Number(a), Value::Number(b)) => {
            match operator {
                Operator::Plus => Value::Number(a + b),
                Operator::Minus => Value::Number(a - b),
                Operator::Multiply => Value::Number(a * b),
                Operator::Divide => {
    if b == 0 {
        panic!("Division by zero");
    }
    Value::Number(a / b)
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