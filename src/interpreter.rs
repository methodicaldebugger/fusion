//contents of interpreter.rs
use std::collections::HashMap;

use crate::ast::*;
use crate::environment::Environment;
use crate::types::{EnumDefinition, StructDefinition};
use crate::value::Value;

#[derive(Debug)]
enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
}

pub struct Interpreter {
    enums: HashMap<String, EnumDefinition>,
    environment: Environment,
    functions: HashMap<String, Function>,
    structs: HashMap<String, StructDefinition>,
    loop_depth: usize,
    output: Vec<String>,
}

#[derive(Clone)]
pub struct Function {
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub body: Vec<Statement>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            loop_depth: 0,
            output: Vec::new(),
        }
    }

    pub fn output(&self) -> &[String] {
    &self.output
}

    // -------------------------------------------------------------------------
    // Scope / defer handling
    // -------------------------------------------------------------------------

    fn exit_scope(&mut self) {
        let deferred = self.environment.take_deferred();

        // Defer is LIFO, like Zig-style defer semantics.
        for expression in deferred.into_iter().rev() {
            self.evaluate(&expression);
        }

        self.environment.pop_scope();
    }

    fn execute_scoped_block(&mut self, statements: &[Statement]) -> Flow {
        self.environment.push_scope();

        let mut flow = Flow::Normal;

        for statement in statements {
            flow = self.execute_statement(statement);

            if !matches!(flow, Flow::Normal) {
                break;
            }
        }

        // IMPORTANT:
        //
        // Defers execute before the flow leaves this scope.
        //
        // Therefore:
        //
        //   defer x()
        //   return 123
        //
        // executes x() before the Return reaches the caller.
        self.exit_scope();

        flow
    }

    // -------------------------------------------------------------------------
    // Property assignment
    // -------------------------------------------------------------------------

    fn assign_property(
        &mut self,
        object: &Expression,
        name: &str,
        value: Value,
    ) {
        match object {
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

                        if let Err(error) = self.environment.assign(
                            variable_name,
                            Value::Struct {
                                name: struct_name,
                                fields,
                            },
                        ) {
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

                        fields.insert(name.to_string(), value);

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
                panic!("Invalid property assignment target");
            }
        }
    }

    // -------------------------------------------------------------------------
    // Function calls
    // -------------------------------------------------------------------------

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

            struct_name => match value {
                Value::Struct { name, .. } => name == struct_name,
                _ => false,
            },
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
        // ---------------------------------------------------------------------
        // Built-ins
        // ---------------------------------------------------------------------

        match name {
            "print" => {
                for argument in arguments {
                    let value = self.evaluate(argument);

                    self.output.push(format!("{}", value));
                }

            return Value::None;
            }

            _ => {}
        }

        // ---------------------------------------------------------------------
        // Find user-defined function
        // ---------------------------------------------------------------------

        let function = match self.functions.get(name) {
            Some(function) => function.clone(),

            None => {
                panic!("Unknown function '{}'", name);
            }
        };

        if arguments.len() != function.parameters.len() {
            panic!(
                "Function '{}' expects {} arguments, got {}",
                name,
                function.parameters.len(),
                arguments.len()
            );
        }

        // ---------------------------------------------------------------------
        // Evaluate arguments before entering the function scope
        // ---------------------------------------------------------------------

        let values: Vec<Value> = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect();

        // ---------------------------------------------------------------------
        // Check parameter types
        // ---------------------------------------------------------------------

        for (parameter, value) in
            function.parameters.iter().zip(values.iter())
        {
            self.check_parameter_type(
                name,
                parameter,
                value,
            );
        }

        // ---------------------------------------------------------------------
        // A function starts a new control-flow context.
        //
        // This is particularly important for:
        //
        //     while (...) {
        //         foo();
        //     }
        //
        // `break` inside foo() must NOT break the caller's loop.
        // ---------------------------------------------------------------------

        let previous_loop_depth = self.loop_depth;
        self.loop_depth = 0;

        self.environment.push_scope();

        // ---------------------------------------------------------------------
        // Bind parameters
        // ---------------------------------------------------------------------

        for (parameter, value) in
            function.parameters.iter().zip(values.iter())
        {
            self.environment.declare(
                parameter.name.clone(),
                value.clone(),
                true,
            );
        }

        // ---------------------------------------------------------------------
        // Execute function body
        // ---------------------------------------------------------------------

        let mut flow = Flow::Normal;

        for statement in &function.body {
            flow = self.execute_statement(statement);

            if !matches!(flow, Flow::Normal) {
                break;
            }
        }

        // ---------------------------------------------------------------------
        // Leaving the function scope always executes its defers.
        // ---------------------------------------------------------------------

        let returned_value = match flow {
            Flow::Return(value) => {
                value
            }

            Flow::Normal => {
                if function.return_type.is_some() {
                    self.exit_scope();
                    self.loop_depth = previous_loop_depth;

                    panic!(
                        "Function '{}' expected a return value",
                        name
                    );
                }

                Value::None
            }

            Flow::Break => {
                self.exit_scope();
                self.loop_depth = previous_loop_depth;

                panic!(
                    "break outside loop in function '{}'",
                    name
                );
            }

            Flow::Continue => {
                self.exit_scope();
                self.loop_depth = previous_loop_depth;

                panic!(
                    "continue outside loop in function '{}'",
                    name
                );
            }
        };

        // IMPORTANT:
        //
        // Return value has already been evaluated, but the function's
        // deferred expressions must execute before the function actually
        // returns to its caller.
        self.exit_scope();

        self.loop_depth = previous_loop_depth;

        self.check_return_type(
            name,
            &function.return_type,
            &returned_value,
        );

        returned_value
    }

    // -------------------------------------------------------------------------
    // Program execution
    // -------------------------------------------------------------------------

    pub fn execute(&mut self, program: &Program) {
        // ---------------------------------------------------------------------
        // Pass 1: register structs
        // ---------------------------------------------------------------------

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

                        other => {
                            crate::types::Type::Struct(
                                other.to_string()
                            )
                        }
                    };

                    field_map.insert(
                        field.name.clone(),
                        field_type,
                    );
                }

                self.structs.insert(
                    name.clone(),
                    StructDefinition {
                        fields: field_map,
                    },
                );
            }
        }

        // ---------------------------------------------------------------------
        // Pass 2: register functions
        // ---------------------------------------------------------------------

        for statement in &program.statements {
            if let Statement::Function {
                name,
                parameters,
                body,
                return_type,
                ..
            } = statement
            {
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

        // ---------------------------------------------------------------------
        // Pass 3: execute top-level statements
        // ---------------------------------------------------------------------

        for statement in &program.statements {
            if matches!(statement, Statement::Function { .. }) {
                continue;
            }

            match self.execute_statement(statement) {
                Flow::Normal => {}

                Flow::Return(_) => {
                    panic!("return outside function");
                }

                Flow::Break => {
                    panic!("break outside loop");
                }

                Flow::Continue => {
                    panic!("continue outside loop");
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Statement execution
    // -------------------------------------------------------------------------

    fn execute_statement(
        &mut self,
        statement: &Statement,
    ) -> Flow {
        match statement {
            // -----------------------------------------------------------------
            // Variables
            // -----------------------------------------------------------------

            Statement::VariableDeclarations {
                declarations,
                ..
            } => {
                for declaration in declarations {
                    let result =
                        self.evaluate(&declaration.value);

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

            // -----------------------------------------------------------------
            // Assignment
            // -----------------------------------------------------------------

            Statement::Assignment {
                target,
                value,
            } => {
                let result = self.evaluate(value);

                match target {
                    Expression::Identifier(name) => {
                        if self.environment.get(name).is_some() {
                            if let Err(error) =
                                self.environment.assign(
                                    name,
                                    result,
                                )
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

                    Expression::Property {
                        object,
                        name,
                    } => {
                        self.assign_property(
                            object,
                            name,
                            result,
                        );
                    }

                    _ => {
                        panic!(
                            "Invalid assignment target"
                        );
                    }
                }

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Expression / call
            // -----------------------------------------------------------------

            Statement::Expression(expr) => {
                self.evaluate(expr);
                Flow::Normal
            }

            Statement::Call(expr) => {
                self.evaluate(expr);
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Defer
            // -----------------------------------------------------------------

            Statement::Defer(expression) => {
                self.environment
                    .add_defer(expression.clone());

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Return
            // -----------------------------------------------------------------

            Statement::Return(expression) => {
                let value = self.evaluate(expression);

                Flow::Return(value)
            }

            // -----------------------------------------------------------------
            // Break
            // -----------------------------------------------------------------

            Statement::Break => {
                if self.loop_depth == 0 {
                    panic!("break outside loop");
                }

                Flow::Break
            }

            // -----------------------------------------------------------------
            // Continue
            // -----------------------------------------------------------------

            Statement::Continue => {
                if self.loop_depth == 0 {
                    panic!("continue outside loop");
                }

                Flow::Continue
            }

            // -----------------------------------------------------------------
            // Main
            // -----------------------------------------------------------------

            Statement::Main { body } => {
                let previous_loop_depth =
                    self.loop_depth;

                self.loop_depth = 0;

                let flow =
                    self.execute_scoped_block(body);

                self.loop_depth =
                    previous_loop_depth;

                match flow {
                    Flow::Normal => Flow::Normal,

                    Flow::Return(_) => {
                        panic!(
                            "return outside function"
                        );
                    }

                    Flow::Break => {
                        panic!(
                            "break outside loop"
                        );
                    }

                    Flow::Continue => {
                        panic!(
                            "continue outside loop"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // If
            // -----------------------------------------------------------------

            Statement::If {
                condition,
                body,
                else_body,
            } => {
                let value = self.evaluate(condition);

                match value {
                    Value::Boolean(true) => {
                        self.execute_scoped_block(body)
                    }

                    Value::Boolean(false) => {
                        match else_body {
                            Some(statements) => {
                                self.execute_scoped_block(
                                    statements,
                                )
                            }

                            None => Flow::Normal,
                        }
                    }

                    _ => {
                        panic!(
                            "If condition must be boolean"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // While
            // -----------------------------------------------------------------

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
                self.exit_scope();
                panic!("While condition must be boolean");
            }
        }

        // Fresh scope for THIS iteration.
        self.environment.push_scope();

        let mut flow = Flow::Normal;

        for statement in body {
            flow = self.execute_statement(statement);

            match flow {
                Flow::Normal => {}

                Flow::Continue
                | Flow::Break
                | Flow::Return(_) => {
                    break;
                }
            }
        }

        // Always unwind iteration scope first.
        self.exit_scope();

        match flow {
            Flow::Normal => {}

            Flow::Continue => {
                continue;
            }

            Flow::Break => {
                self.loop_depth -= 1;
                self.exit_scope();
                return Flow::Normal;
            }

            Flow::Return(value) => {
                self.loop_depth -= 1;
                self.exit_scope();
                return Flow::Return(value);
            }
        }
    }

    self.loop_depth -= 1;
    self.exit_scope();

    Flow::Normal
}

            // -----------------------------------------------------------------
            // For
            // -----------------------------------------------------------------

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
        // Every iteration gets a fresh scope.
        self.environment.push_scope();

        self.environment.set(
            variable.clone(),
            Value::Number(i),
        );

        let mut flow = Flow::Normal;

        for statement in body {
            flow = self.execute_statement(statement);

            match flow {
                Flow::Normal => {}

                Flow::Continue => {
                    break;
                }

                Flow::Break | Flow::Return(_) => {
                    break;
                }
            }
        }

        // IMPORTANT:
        // This runs iteration defers before continuing,
        // breaking, or returning.
        self.exit_scope();

        match flow {
            Flow::Normal => {}

            Flow::Continue => {
                continue;
            }

            Flow::Break => {
                self.loop_depth -= 1;
                self.exit_scope();
                return Flow::Normal;
            }

            Flow::Return(value) => {
                self.loop_depth -= 1;
                self.exit_scope();
                return Flow::Return(value);
            }
        }
    }

    self.loop_depth -= 1;
    self.exit_scope();

    Flow::Normal
}

            // -----------------------------------------------------------------
            // Match
            // -----------------------------------------------------------------

            Statement::Match {
                expression,
                arms,
            } => {
                let value =
                    self.evaluate(expression);

                for arm in arms {
                    if let Some(bindings) =
                        self.pattern_matches(
                            &arm.pattern,
                            &value,
                        )
                    {
                        self.environment.push_scope();

                        for (name, value) in bindings {
                            self.environment.declare(
                                name,
                                value,
                                true,
                            );
                        }

                        let flow =
                            self.execute_statement_list(
                                &arm.body
                            );

                        self.exit_scope();

                        return flow;
                    }
                }

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Enum
            // -----------------------------------------------------------------

            Statement::Enum {
                name,
                variants,
            } => {
                let mut variant_map =
                    HashMap::new();

                for variant in variants {
                    let fields = variant
                        .fields
                        .iter()
                        .map(|field| {
                            match field.as_str() {
                                "num" => {
                                    crate::types::Type::Num
                                }

                                "float" => {
                                    crate::types::Type::Float
                                }

                                "bool" => {
                                    crate::types::Type::Bool
                                }

                                "string" => {
                                    crate::types::Type::String
                                }

                                other => {
                                    crate::types::Type::Struct(
                                        other.to_string()
                                    )
                                }
                            }
                        })
                        .collect();

                    variant_map.insert(
                        variant.name.clone(),
                        crate::types::EnumVariantDefinition {
                            fields,
                        },
                    );
                }

                self.enums.insert(
                    name.clone(),
                    EnumDefinition {
                        variants: variant_map,
                    },
                );

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Function declaration
            // -----------------------------------------------------------------

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
                    },
                );

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Struct declaration
            // -----------------------------------------------------------------

            Statement::Struct { .. } => {
                // Structs were registered during the first pass.
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Trait / impl
            //
            // These remain declarations for now.
            // -----------------------------------------------------------------

            Statement::Trait { .. } => {
                Flow::Normal
            }

            Statement::Impl { .. } => {
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Unsupported / currently declaration-only statements
            // -----------------------------------------------------------------

            Statement::For { .. }
            | Statement::While { .. }
            | Statement::If { .. }
            | Statement::Match { .. }
            | Statement::Main { .. }
            | Statement::Return(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Defer(_)
            | Statement::Assignment { .. }
            | Statement::VariableDeclarations { .. }
            | Statement::ConstDeclaration { .. }
            | Statement::Expression(_)
            | Statement::Call(_) => {
                unreachable!(
                    "statement variant handled above"
                );
            }
        }
    }

    fn execute_statement_list(
        &mut self,
        statements: &[Statement],
    ) -> Flow {
        for statement in statements {
            let flow =
                self.execute_statement(statement);

            if !matches!(flow, Flow::Normal) {
                return flow;
            }
        }

        Flow::Normal
    }

    // -------------------------------------------------------------------------
    // Pattern matching
    // -------------------------------------------------------------------------

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
                Some(vec![
                    (name.clone(), value.clone())
                ])
            }

            (Pattern::Number(a), Value::Number(b))
                if a == b =>
            {
                Some(Vec::new())
            }

            (Pattern::Float(a), Value::Float(b))
                if a == b =>
            {
                Some(Vec::new())
            }

            (Pattern::String(a), Value::String(b))
                if a == b =>
            {
                Some(Vec::new())
            }

            (Pattern::Boolean(a), Value::Boolean(b))
                if a == b =>
            {
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
                let expected =
                    format!("{}::{}", enum_name, variant);

                if name != &expected {
                    return None;
                }

                if bindings.len() != values.len() {
                    return None;
                }

                Some(
                    bindings
                        .iter()
                        .cloned()
                        .zip(values.iter().cloned())
                        .collect(),
                )
            }

            _ => None,
        }
    }

    // -------------------------------------------------------------------------
    // Expression evaluation
    // -------------------------------------------------------------------------

    fn evaluate(
        &mut self,
        expr: &Expression,
    ) -> Value {
        match expr {
            Expression::Number(value) => {
                Value::Number(*value)
            }

            Expression::Float(value) => {
                Value::Float(*value)
            }

            Expression::Boolean(value) => {
                Value::Boolean(*value)
            }

            Expression::String(value) => {
                Value::String(value.clone())
            }

            Expression::Identifier(name) => {
                match self.environment.get(name) {
                    Some(value) => value.clone(),

                    None => {
                        panic!(
                            "Runtime error: unknown variable {}",
                            name
                        );
                    }
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

            Expression::StructConstructor {
                name,
                fields,
            } => {
                let mut result =
                    HashMap::new();

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
                    .map(|argument| {
                        self.evaluate(argument)
                    })
                    .collect();

                Value::Enum {
                    enum_name: enum_name.clone(),
                    variant: variant.clone(),
                    values,
                }
            }

            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let left_value =
                    self.evaluate(left);

                let right_value =
                    self.evaluate(right);

                self.evaluate_binary(
                    left_value,
                    operator,
                    right_value,
                )
            }

            Expression::Unary {
                operator,
                expression,
            } => {
                let value =
                    self.evaluate(expression);

                match operator {
                    UnaryOperator::Negate => {
                        match value {
                            Value::Number(v) =>
                                Value::Number(-v),

                            Value::Float(v) =>
                                Value::Float(-v),

                            _ => {
                                panic!(
                                    "Unary '-' requires a numeric value"
                                );
                            }
                        }
                    }

                    UnaryOperator::Not => {
                        match value {
                            Value::Boolean(v) =>
                                Value::Boolean(!v),

                            _ => {
                                panic!(
                                    "Unary '!' requires a boolean value"
                                );
                            }
                        }
                    }
                }
            }

            Expression::Call {
                name,
                arguments,
                ..
            } => {
                self.call_function(
                    name,
                    arguments,
                )
            }

            Expression::Index {
                array,
                index,
            } => {
                let array_value =
                    self.evaluate(array);

                let index_value =
                    self.evaluate(index);

                match (
                    array_value,
                    index_value,
                ) {
                    (
                        Value::Array(values),
                        Value::Number(index),
                    ) => {
                        if index < 0 {
                            panic!(
                                "Array index out of bounds"
                            );
                        }

                        match values.get(
                            index as usize
                        ) {
                            Some(value) =>
                                value.clone(),

                            None => {
                                panic!(
                                    "Array index out of bounds"
                                );
                            }
                        }
                    }

                    _ => {
                        panic!(
                            "Invalid array indexing"
                        );
                    }
                }
            }

            Expression::Property {
                object,
                name,
            } => {
                let value =
                    self.evaluate(object);

                match value {
                    Value::Struct {
                        fields,
                        ..
                    } => {
                        match fields.get(name) {
                            Some(value) =>
                                value.clone(),

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
                let object_value =
                    self.evaluate(object);

                match method.as_str() {
                    "push" => {
                        if arguments.len() != 1 {
                            panic!(
                                "push() expects exactly 1 argument"
                            );
                        }

                        let value =
                            self.evaluate(
                                &arguments[0]
                            );

                        match object_value {
                            Value::Array(
                                mut values
                            ) => {
                                values.push(value);

                                if let Expression::Identifier(
                                    name
                                ) = object.as_ref()
                                {
                                    if let Err(error) =
                                        self.environment.assign(
                                            name,
                                            Value::Array(
                                                values
                                            ),
                                        )
                                    {
                                        panic!("{}", error);
                                    }
                                } else {
                                    panic!(
                                        "push() requires an array variable"
                                    );
                                }

                                Value::None
                            }

                            _ => {
                                panic!(
                                    "push() can only be called on an array"
                                );
                            }
                        }
                    }

                    "pop" => {
                        if !arguments.is_empty() {
                            panic!(
                                "pop() expects no arguments"
                            );
                        }

                        match object.as_ref() {
                            Expression::Identifier(
                                name
                            ) => {
                                let array =
                                    self.environment
                                        .get_mut(name)
                                        .unwrap_or_else(
                                            || {
                                                panic!(
                                                    "Unknown array variable '{}'",
                                                    name
                                                )
                                            }
                                        );

                                match array {
                                    Value::Array(
                                        values
                                    ) => {
                                        values
                                            .pop()
                                            .unwrap_or_else(
                                                || {
                                                    panic!(
                                                        "Cannot pop from an empty array"
                                                    )
                                                }
                                            )
                                    }

                                    _ => {
                                        panic!(
                                            "pop() can only be called on an array"
                                        );
                                    }
                                }
                            }

                            _ => {
                                panic!(
                                    "pop() requires an array variable"
                                );
                            }
                        }
                    }

                    "length" => {
                        if !arguments.is_empty() {
                            panic!(
                                "length() expects no arguments"
                            );
                        }

                        match object_value {
                            Value::Array(values) => {
                                Value::Number(
                                    values.len()
                                        as i64
                                )
                            }

                            _ => {
                                panic!(
                                    "length() can only be called on an array"
                                );
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

    // -------------------------------------------------------------------------
    // Return type checking
    // -------------------------------------------------------------------------

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
            "num" => {
                matches!(value, Value::Number(_))
            }

            "float" => {
                matches!(value, Value::Float(_))
            }

            "string" => {
                matches!(value, Value::String(_))
            }

            "bool" => {
                matches!(value, Value::Boolean(_))
            }

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

    // -------------------------------------------------------------------------
    // Binary operations
    // -------------------------------------------------------------------------

    fn evaluate_binary(
        &self,
        left: Value,
        operator: &Operator,
        right: Value,
    ) -> Value {
        match (left, right) {
            (
                Value::Number(a),
                Value::Number(b),
            ) => {
                match operator {
                    Operator::Plus =>
                        Value::Number(a + b),

                    Operator::Minus =>
                        Value::Number(a - b),

                    Operator::Multiply =>
                        Value::Number(a * b),

                    Operator::Divide => {
                        if b == 0 {
                            panic!(
                                "Division by zero"
                            );
                        }

                        Value::Number(a / b)
                    }

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

                    Operator::And
                    | Operator::Or => {
                        panic!(
                            "Logical operators require boolean operands"
                        );
                    }
                }
            }

            (
                Value::Float(a),
                Value::Float(b),
            ) => {
                match operator {
                    Operator::Plus =>
                        Value::Float(a + b),

                    Operator::Minus =>
                        Value::Float(a - b),

                    Operator::Multiply =>
                        Value::Float(a * b),

                    Operator::Divide => {
                        if b == 0.0 {
                            panic!(
                                "Division by zero"
                            );
                        }

                        Value::Float(a / b)
                    }

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

                    Operator::And
                    | Operator::Or => {
                        panic!(
                            "Logical operators require boolean operands"
                        );
                    }
                }
            }

            (
                Value::Boolean(a),
                Value::Boolean(b),
            ) => {
                match operator {
                    Operator::And =>
                        Value::Boolean(a && b),

                    Operator::Or =>
                        Value::Boolean(a || b),

                    Operator::Equal =>
                        Value::Boolean(a == b),

                    Operator::NotEqual =>
                        Value::Boolean(a != b),

                    _ => {
                        panic!(
                            "Invalid boolean operation"
                        );
                    }
                }
            }

            (
                Value::String(a),
                Value::String(b),
            ) => {
                match operator {
                    Operator::Plus =>
                        Value::String(
                            format!("{}{}", a, b)
                        ),

                    Operator::Equal =>
                        Value::Boolean(a == b),

                    Operator::NotEqual =>
                        Value::Boolean(a != b),

                    _ => {
                        panic!(
                            "Invalid string operation"
                        );
                    }
                }
            }

            _ => {
                panic!(
                    "Invalid operation between incompatible values"
                );
            }
        }
    }
}