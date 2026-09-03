//contents of interpreter.rs
use std::collections::HashMap;

use crate::ast::*;
use crate::environment::Environment;
use crate::types::{EnumDefinition, EnumVariantDefinition, StructDefinition, Type};
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
            enums: HashMap::new(),
            environment: Environment::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            loop_depth: 0,
            output: Vec::new(),
        }
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    // =========================================================================
    // Type helpers
    // =========================================================================

    fn type_from_name(&self, name: &str) -> Type {
        match name {
            "num" => Type::Num,
            "float" => Type::Float,
            "bool" => Type::Bool,
            "string" => Type::String,
            other => Type::Struct(other.to_string()),
        }
    }

    fn value_matches_type(&self, value: &Value, expected: &str) -> bool {
        match expected {
            "num" => matches!(value, Value::Number(_)),
            "float" => matches!(value, Value::Float(_)),
            "bool" => matches!(value, Value::Boolean(_)),
            "string" => matches!(value, Value::String(_)),

            struct_name => match value {
                Value::Struct { name, .. } => name == struct_name,
                _ => false,
            },
        }
    }

    fn check_value_type(
        &self,
        context: &str,
        expected: Option<&String>,
        value: &Value,
    ) {
        let Some(expected) = expected else {
            return;
        };

        if !self.value_matches_type(value, expected) {
            panic!(
                "{}: expected {}, got {:?}",
                context,
                expected,
                value
            );
        }
    }

    // =========================================================================
    // Scope / defer handling
    // =========================================================================

    fn exit_scope(&mut self) {
        let deferred = self.environment.take_deferred();

        // LIFO defer semantics.
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

        self.exit_scope();

        flow
    }

    // =========================================================================
    // Property assignment
    // =========================================================================

    fn assign_property(
        &mut self,
        object: &Expression,
        name: &str,
        value: Value,
    ) {
        match object {
            Expression::Identifier {
                name: variable_name,
                ..
            } => {
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
                        panic!("Property assignment requires a struct");
                    }
                }
            }

            Expression::Property {
                object: parent,
                name: parent_field,
                ..
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
                        panic!("Property assignment requires a struct");
                    }
                }
            }

            _ => {
                panic!("Invalid property assignment target");
            }
        }
    }

    // =========================================================================
    // Function handling
    // =========================================================================

    fn check_parameter_type(
        &self,
        function_name: &str,
        parameter: &Parameter,
        value: &Value,
    ) {
        let Some(expected) = &parameter.type_name else {
            return;
        };

        if !self.value_matches_type(value, expected) {
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
        _generic_arguments: &[String],
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
        // User-defined function
        // ---------------------------------------------------------------------

        let function = match self.functions.get(name) {
            Some(function) => function.clone(),
            None => panic!("Unknown function '{}'", name),
        };

        if arguments.len() != function.parameters.len() {
            panic!(
                "Function '{}' expects {} arguments, got {}",
                name,
                function.parameters.len(),
                arguments.len()
            );
        }

        // Arguments are evaluated in the caller's scope.
        let values: Vec<Value> = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect();

        // Type-check before entering function scope.
        for (parameter, value) in
            function.parameters.iter().zip(values.iter())
        {
            self.check_parameter_type(name, parameter, value);
        }

        // A function gets its own loop context.
        let previous_loop_depth = self.loop_depth;
        self.loop_depth = 0;

        self.environment.push_scope();

        // Bind parameters.
        for (parameter, value) in
            function.parameters.iter().zip(values.into_iter())
        {
            self.environment.declare(
                parameter.name.clone(),
                value,
                true,
            );
        }

        // Execute body.
        let mut flow = Flow::Normal;

        for statement in &function.body {
            flow = self.execute_statement(statement);

            if !matches!(flow, Flow::Normal) {
                break;
            }
        }

        // A return value has already been evaluated.
        //
        // Defers must execute before the function scope disappears.
        let returned_value = match flow {
            Flow::Return(value) => value,

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
                    "break escaped function '{}'",
                    name
                );
            }

            Flow::Continue => {
                self.exit_scope();
                self.loop_depth = previous_loop_depth;

                panic!(
                    "continue escaped function '{}'",
                    name
                );
            }
        };

        self.exit_scope();

        self.loop_depth = previous_loop_depth;

        self.check_return_type(
            name,
            &function.return_type,
            &returned_value,
        );

        returned_value
    }

    // =========================================================================
    // Program execution
    // =========================================================================

    pub fn execute(&mut self, program: &Program) {
        // ---------------------------------------------------------------------
        // Pass 1: register structs
        // ---------------------------------------------------------------------

                // ---------------------------------------------------------------------
        // Pass 1: register structs
        // ---------------------------------------------------------------------

        for statement in &program.statements {
            if let Statement::Struct {
                name,
                fields,
                ..
            } = statement
            {
                if self.structs.contains_key(name) {
                    panic!("Duplicate struct '{}'", name);
                }

                let mut field_list = Vec::new();

                for field in fields {
                    if field_list
                        .iter()
                        .any(|(field_name, _)| field_name == &field.name)
                    {
                        panic!(
                            "Duplicate field '{}' in struct '{}'",
                            field.name,
                            name
                        );
                    }

                    field_list.push((
                        field.name.clone(),
                        self.type_from_name(&field.type_name),
                    ));
                }

                self.structs.insert(
                    name.clone(),
                    StructDefinition {
                        fields: field_list,
                    },
                );
            }
        }

        // ---------------------------------------------------------------------
        // Pass 2: register enums
        // ---------------------------------------------------------------------

        for statement in &program.statements {
            if let Statement::Enum {
                name,
                variants,
                ..
            } = statement
            {
                if self.enums.contains_key(name) {
                    panic!("Duplicate enum '{}'", name);
                }

                let mut variant_map = HashMap::new();

                for variant in variants {
                    if variant_map.contains_key(&variant.name) {
                        panic!(
                            "Duplicate variant '{}' in enum '{}'",
                            variant.name,
                            name
                        );
                    }

                    let fields = variant
                        .fields
                        .iter()
                        .map(|field| self.type_from_name(field))
                        .collect();

                    variant_map.insert(
                        variant.name.clone(),
                        EnumVariantDefinition {
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
            }
        }

        // ---------------------------------------------------------------------
        // Pass 3: register functions
        // ---------------------------------------------------------------------

        for statement in &program.statements {
            if let Statement::Function {
                name,
                parameters,
                return_type,
                body,
                ..
            } = statement
            {
                if self.functions.contains_key(name) {
                    panic!("Duplicate function '{}'", name);
                }

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
        // Pass 4: execute top-level declarations/statements
        // ---------------------------------------------------------------------

        for statement in &program.statements {
            match statement {
                // Declarations are already registered.
                Statement::Function { .. }
                | Statement::Struct { .. }
                | Statement::Enum { .. }
                | Statement::Trait { .. }
                | Statement::Impl { .. } => {}

                _ => {
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
        }
    }

    // =========================================================================
    // Statement execution
    // =========================================================================

    fn execute_statement(
        &mut self,
        statement: &Statement,
    ) -> Flow {
        match statement {
            // -----------------------------------------------------------------
            // Variable declarations
            // -----------------------------------------------------------------

            Statement::VariableDeclarations {
                declarations,
                ..
            } => {
                for declaration in declarations {
                    let value = self.evaluate(&declaration.value);

                    self.check_value_type(
                        &format!(
                            "Variable '{}' type error",
                            declaration.name
                        ),
                        declaration.declared_type.as_ref(),
                        &value,
                    );

                    self.environment.declare(
                        declaration.name.clone(),
                        value,
                        true,
                    );
                }

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Constant declaration
            // -----------------------------------------------------------------

            Statement::ConstDeclaration {
                name,
                declared_type,
                value,
                ..
            } => {
                let result = self.evaluate(value);

                self.check_value_type(
                    &format!("Constant '{}' type error", name),
                    declared_type.as_ref(),
                    &result,
                );

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
                ..
            } => {
                let result = self.evaluate(value);

                match target {
                    Expression::Identifier { name, .. } => {
                        if self.environment.get(name).is_some() {
                            if let Err(error) =
                                self.environment.assign(name, result)
                            {
                                panic!("{}", error);
                            }
                        } else {
                            // Preserve the existing language behavior:
                            // assigning an unknown name creates a variable.
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
                        ..
                    } => {
                        self.assign_property(
                            object,
                            name,
                            result,
                        );
                    }

                    _ => {
                        panic!("Invalid assignment target");
                    }
                }

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Expression statement
            // -----------------------------------------------------------------

            Statement::Expression {
                expression,
                ..
            } => {
                self.evaluate(expression);
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Call statement
            // -----------------------------------------------------------------

            Statement::Call {
                expression,
                ..
            } => {
                self.evaluate(expression);
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Defer
            // -----------------------------------------------------------------

            Statement::Defer {
                expression,
                ..
            } => {
                self.environment.add_defer(expression.clone());
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Return
            // -----------------------------------------------------------------

            Statement::Return {
    value,
    ..
} => {
    let result = match value {
        Some(expr) => self.evaluate(expr),
        None => Value::None,
    };

    Flow::Return(result)
}

            // -----------------------------------------------------------------
            // Break
            // -----------------------------------------------------------------

            Statement::Break { .. } => {
                if self.loop_depth == 0 {
                    panic!("break outside loop");
                }

                Flow::Break
            }

            // -----------------------------------------------------------------
            // Continue
            // -----------------------------------------------------------------

            Statement::Continue { .. } => {
                if self.loop_depth == 0 {
                    panic!("continue outside loop");
                }

                Flow::Continue
            }

            // -----------------------------------------------------------------
            // Main
            // -----------------------------------------------------------------

            Statement::Main {
                body,
                ..
            } => {
                let previous_loop_depth = self.loop_depth;
                self.loop_depth = 0;

                let flow = self.execute_scoped_block(body);

                self.loop_depth = previous_loop_depth;

                match flow {
                    Flow::Normal => Flow::Normal,

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

            // -----------------------------------------------------------------
            // If
            // -----------------------------------------------------------------

            Statement::If {
                condition,
                body,
                else_body,
                ..
            } => {
                let value = self.evaluate(condition);

                match value {
                    Value::Boolean(true) => {
                        self.execute_scoped_block(body)
                    }

                    Value::Boolean(false) => {
                        match else_body {
                            Some(statements) => {
                                self.execute_scoped_block(statements)
                            }

                            None => Flow::Normal,
                        }
                    }

                    _ => {
                        panic!("If condition must be boolean");
                    }
                }
            }

            // -----------------------------------------------------------------
            // While
            // -----------------------------------------------------------------

            Statement::While {
                condition,
                body,
                ..
            } => {
                self.environment.push_scope();
                self.loop_depth += 1;

                loop {
                    let condition_value = self.evaluate(condition);

                    match condition_value {
                        Value::Boolean(true) => {}

                        Value::Boolean(false) => {
                            break;
                        }

                        _ => {
                            self.loop_depth -= 1;
                            self.exit_scope();

                            panic!(
                                "While condition must be boolean"
                            );
                        }
                    }

                    // Every iteration receives a fresh scope.
                    self.environment.push_scope();

                    let mut flow = Flow::Normal;

                    for statement in body {
                        flow = self.execute_statement(statement);

                        if !matches!(flow, Flow::Normal) {
                            break;
                        }
                    }

                    // Iteration defers execute before control leaves
                    // this iteration.
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
                ..
            } => {
                let start_value = self.evaluate(start);
                let end_value = self.evaluate(end);

                let start_number = match start_value {
                    Value::Number(value) => value,
                    _ => {
                        panic!(
                            "For loop start must be an integer"
                        );
                    }
                };

                let end_number = match end_value {
                    Value::Number(value) => value,
                    _ => {
                        panic!(
                            "For loop end must be an integer"
                        );
                    }
                };

                self.environment.push_scope();
                self.loop_depth += 1;

                for i in start_number..end_number {
                    // Fresh scope per iteration.
                    self.environment.push_scope();

                    self.environment.set(
                        variable.clone(),
                        Value::Number(i),
                    );

                    let mut flow = Flow::Normal;

                    for statement in body {
                        flow = self.execute_statement(statement);

                        if !matches!(flow, Flow::Normal) {
                            break;
                        }
                    }

                    // Iteration defers execute here.
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
                ..
            } => {
                let value = self.evaluate(expression);

                for arm in arms {
                    if let Some(bindings) =
                        self.pattern_matches(&arm.pattern, &value)
                    {
                        self.environment.push_scope();

                        for (name, binding_value) in bindings {
                            self.environment.declare(
                                name,
                                binding_value,
                                true,
                            );
                        }

                        let flow =
                            self.execute_statement_list(&arm.body);

                        self.exit_scope();

                        return flow;
                    }
                }

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Function declaration
            // -----------------------------------------------------------------

            Statement::Function {
                name,
                parameters,
                return_type,
                body,
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
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Enum declaration
            // -----------------------------------------------------------------

            Statement::Enum {
                name,
                variants,
                ..
            } => {
                let mut variant_map = HashMap::new();

                for variant in variants {
                    let fields = variant
                        .fields
                        .iter()
                        .map(|field| self.type_from_name(field))
                        .collect();

                    variant_map.insert(
                        variant.name.clone(),
                        EnumVariantDefinition {
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
            // Trait / impl
            // -----------------------------------------------------------------

            Statement::Trait { .. } => {
                Flow::Normal
            }

            Statement::Impl { .. } => {
                Flow::Normal
            }
        }
    }

    fn execute_statement_list(
        &mut self,
        statements: &[Statement],
    ) -> Flow {
        for statement in statements {
            let flow = self.execute_statement(statement);

            if !matches!(flow, Flow::Normal) {
                return flow;
            }
        }

        Flow::Normal
    }

    // =========================================================================
    // Pattern matching
    // =========================================================================

    fn pattern_matches(
        &self,
        pattern: &Pattern,
        value: &Value,
    ) -> Option<Vec<(String, Value)>> {
        match (&pattern.kind, value) {
            // -----------------------------------------------------------------
            // Wildcard
            // -----------------------------------------------------------------

            (PatternKind::Wildcard, _) => {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // Identifier binding
            // -----------------------------------------------------------------

            (PatternKind::Identifier(name), value) => {
                Some(vec![
                    (name.clone(), value.clone())
                ])
            }

            // -----------------------------------------------------------------
            // Number
            // -----------------------------------------------------------------

            (PatternKind::Number(expected), Value::Number(actual))
                if expected == actual =>
            {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // Float
            // -----------------------------------------------------------------

            (PatternKind::Float(expected), Value::Float(actual))
                if expected == actual =>
            {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // String
            // -----------------------------------------------------------------

            (PatternKind::String(expected), Value::String(actual))
                if expected == actual =>
            {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // Boolean
            // -----------------------------------------------------------------

            (PatternKind::Boolean(expected), Value::Boolean(actual))
                if expected == actual =>
            {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // Enum variant
            // -----------------------------------------------------------------

            (
                PatternKind::Variant {
                    name,
                    bindings,
                },
                Value::Enum {
                    enum_name,
                    variant,
                    values,
                },
            ) => {
                let expected_name =
                    format!("{}::{}", enum_name, variant);

                if name != &expected_name {
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

    // =========================================================================
    // Expression evaluation
    // =========================================================================

    fn evaluate(&mut self, expr: &Expression) -> Value {
        match expr {
            // -----------------------------------------------------------------
            // Literals
            // -----------------------------------------------------------------

            Expression::Number { value, .. } => {
                Value::Number(*value)
            }

            Expression::Float { value, .. } => {
                Value::Float(*value)
            }

            Expression::Boolean { value, .. } => {
                Value::Boolean(*value)
            }

            Expression::String { value, .. } => {
                Value::String(value.clone())
            }

            // -----------------------------------------------------------------
            // Identifier
            // -----------------------------------------------------------------

            Expression::Identifier { name, .. } => {
                self.environment
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| {
                        panic!(
                            "Runtime error: unknown variable '{}'",
                            name
                        )
                    })
            }

            // -----------------------------------------------------------------
            // Array
            // -----------------------------------------------------------------

            Expression::Array {
                elements,
                ..
            } => {
                let values = elements
                    .iter()
                    .map(|element| self.evaluate(element))
                    .collect();

                Value::Array(values)
            }

            // -----------------------------------------------------------------
            // Indexing
            // -----------------------------------------------------------------

            Expression::Index {
                array,
                index,
                ..
            } => {
                let array_value = self.evaluate(array);
                let index_value = self.evaluate(index);

                match (array_value, index_value) {
                    (
                        Value::Array(values),
                        Value::Number(index),
                    ) => {
                        if index < 0 {
                            panic!("Array index out of bounds");
                        }

                        values
                            .get(index as usize)
                            .cloned()
                            .unwrap_or_else(|| {
                                panic!(
                                    "Array index out of bounds"
                                )
                            })
                    }

                    _ => {
                        panic!(
                            "Invalid array indexing: index must be a number and target must be an array"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // Property access
            // -----------------------------------------------------------------

            Expression::Property {
                object,
                name,
                ..
            } => {
                let value = self.evaluate(object);

                match value {
                    Value::Struct {
                        fields,
                        ..
                    } => {
                        fields
                            .get(name)
                            .cloned()
                            .unwrap_or_else(|| {
                                panic!(
                                    "Unknown field '{}'",
                                    name
                                )
                            })
                    }

                    _ => {
                        panic!(
                            "Property access requires a struct"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // Method calls
            // -----------------------------------------------------------------

            Expression::MethodCall {
                object,
                method,
                arguments,
                ..
            } => {
                self.evaluate_method_call(
                    object,
                    method,
                    arguments,
                )
            }

            // -----------------------------------------------------------------
            // Function calls
            // -----------------------------------------------------------------

            Expression::Call {
    name,
    arguments,
    ..
} => {
    // Struct constructor.
    if let Some(struct_definition) =
        self.structs.get(name).cloned()
    {
        if arguments.len() != struct_definition.fields.len() {
            panic!(
                "Struct '{}' expects {} arguments, got {}",
                name,
                struct_definition.fields.len(),
                arguments.len()
            );
        }

        let mut fields = HashMap::new();

        for ((field_name, expected_type), argument) in
            struct_definition.fields.iter().zip(arguments.iter())
        {
            let value = self.evaluate(argument);

            if !self.value_matches_type_value(
                &value,
                expected_type,
            ) {
                panic!(
                    "Invalid value for field '{}.{}': expected {:?}, got {:?}",
                    name,
                    field_name,
                    expected_type,
                    value
                );
            }

            fields.insert(
                field_name.clone(),
                value,
            );
        }

        return Value::Struct {
            name: name.clone(),
            fields,
        };
    }

    self.call_function(name, arguments, &[])
}

            // -----------------------------------------------------------------
            // Struct constructor
            // -----------------------------------------------------------------

            Expression::StructConstructor {
                name,
                fields,
                ..
            } => {
                let definition = self.structs.get(name).cloned();

                if definition.is_none() {
                    panic!(
                        "Unknown struct '{}'",
                        name
                    );
                }

                let definition = definition.unwrap();

                let mut result = HashMap::new();

                for (field_name, expression) in fields {
    let expected_type =
        definition
            .fields
            .iter()
            .find(|(name, _)| name == field_name)
            .map(|(_, ty)| ty);

    let Some(expected_type) = expected_type else {
        panic!(
            "Unknown field '{}' on struct '{}'",
            field_name,
            name
        );
    };

    let value = self.evaluate(expression);

    if !self.value_matches_type_value(
        &value,
        expected_type,
    ) {
        panic!(
            "Invalid value for field '{}.{}': expected {:?}, got {:?}",
            name,
            field_name,
            expected_type,
            value
        );
    }

    result.insert(
        field_name.clone(),
        value,
    );
}

                // Require all declared fields.
                // Require all declared fields.
for (field_name, _) in &definition.fields {
    if !result.contains_key(field_name) {
        panic!(
            "Missing field '{}' in struct constructor '{}'",
            field_name,
            name
        );
    }
}

                Value::Struct {
                    name: name.clone(),
                    fields: result,
                }
            }

            // -----------------------------------------------------------------
            // Enum constructor
            // -----------------------------------------------------------------

            Expression::EnumConstructor {
                enum_name,
                variant,
                arguments,
                ..
            } => {
                let enum_definition =
                    self.enums.get(enum_name).cloned();

                let Some(enum_definition) = enum_definition else {
                    panic!(
                        "Unknown enum '{}'",
                        enum_name
                    );
                };

                let variant_definition =
                    enum_definition.variants.get(variant).cloned();

                let Some(variant_definition) = variant_definition else {
                    panic!(
                        "Unknown variant '{}::{}'",
                        enum_name,
                        variant
                    );
                };

                if arguments.len() != variant_definition.fields.len() {
                    panic!(
                        "Enum variant '{}::{}' expects {} arguments, got {}",
                        enum_name,
                        variant,
                        variant_definition.fields.len(),
                        arguments.len()
                    );
                }

                let mut values = Vec::new();

                for (argument, expected_type) in arguments
                    .iter()
                    .zip(variant_definition.fields.iter())
                {
                    let value = self.evaluate(argument);

                    if !self.value_matches_type_value(
                        &value,
                        expected_type,
                    ) {
                        panic!(
                            "Invalid value in enum variant '{}::{}': expected {:?}, got {:?}",
                            enum_name,
                            variant,
                            expected_type,
                            value
                        );
                    }

                    values.push(value);
                }

                Value::Enum {
                    enum_name: enum_name.clone(),
                    variant: variant.clone(),
                    values,
                }
            }

            // -----------------------------------------------------------------
            // Binary expression
            // -----------------------------------------------------------------

            Expression::Binary {
                left,
                operator,
                right,
                ..
            } => {
                let left_value = self.evaluate(left);

                let right_value = self.evaluate(right);

                self.evaluate_binary(
                    left_value,
                    operator,
                    right_value,
                )
            }

            // -----------------------------------------------------------------
            // Unary expression
            // -----------------------------------------------------------------

            Expression::Unary {
                operator,
                expression,
                ..
            } => {
                let value = self.evaluate(expression);

                match operator {
                    UnaryOperator::Negate => {
                        match value {
                            Value::Number(value) => {
                                Value::Number(-value)
                            }

                            Value::Float(value) => {
                                Value::Float(-value)
                            }

                            _ => {
                                panic!(
                                    "Unary '-' requires a numeric value"
                                );
                            }
                        }
                    }

                    UnaryOperator::Not => {
                        match value {
                            Value::Boolean(value) => {
                                Value::Boolean(!value)
                            }

                            _ => {
                                panic!(
                                    "Unary 'not' requires a boolean value"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // =========================================================================
    // Value / Type compatibility
    // =========================================================================

    fn value_matches_type_value(
        &self,
        value: &Value,
        expected: &Type,
    ) -> bool {
        match expected {
            Type::Num => matches!(value, Value::Number(_)),
            Type::Float => matches!(value, Value::Float(_)),
            Type::Bool => matches!(value, Value::Boolean(_)),
            Type::String => matches!(value, Value::String(_)),

            Type::Struct(expected_name) => {
                matches!(
                    value,
                    Value::Struct { name, .. }
                        if name == expected_name
                )
            }

            _ => false,
        }
    }

    // =========================================================================
    // Method calls
    // =========================================================================

    fn evaluate_method_call(
        &mut self,
        object: &Expression,
        method: &str,
        arguments: &[Expression],
    ) -> Value {
        match method {
            // -----------------------------------------------------------------
            // Array.push(value)
            // -----------------------------------------------------------------

            "push" => {
                if arguments.len() != 1 {
                    panic!(
                        "push() expects exactly 1 argument"
                    );
                }

                let value = self.evaluate(&arguments[0]);

                match object {
                    Expression::Identifier { name, .. } => {
                        let array = self
                            .environment
                            .get_mut(name)
                            .unwrap_or_else(|| {
                                panic!(
                                    "Unknown variable '{}'",
                                    name
                                )
                            });

                        match array {
                            Value::Array(values) => {
                                values.push(value);
                                Value::None
                            }

                            _ => {
                                panic!(
                                    "push() can only be called on an array"
                                );
                            }
                        }
                    }

                    _ => {
                        panic!(
                            "push() requires an array variable"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // Array.pop()
            // -----------------------------------------------------------------

            "pop" => {
                if !arguments.is_empty() {
                    panic!(
                        "pop() expects no arguments"
                    );
                }

                match object {
                    Expression::Identifier { name, .. } => {
                        let array = self
                            .environment
                            .get_mut(name)
                            .unwrap_or_else(|| {
                                panic!(
                                    "Unknown array variable '{}'",
                                    name
                                )
                            });

                        match array {
                            Value::Array(values) => {
                                values.pop().unwrap_or_else(|| {
                                    panic!(
                                        "Cannot pop from an empty array"
                                    )
                                })
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

            // -----------------------------------------------------------------
            // Array.length()
            // -----------------------------------------------------------------

            "length" => {
                if !arguments.is_empty() {
                    panic!(
                        "length() expects no arguments"
                    );
                }

                let object_value = self.evaluate(object);

                match object_value {
                    Value::Array(values) => {
                        Value::Number(values.len() as i64)
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

    // =========================================================================
    // Return type checking
    // =========================================================================

    fn check_return_type(
        &self,
        function_name: &str,
        expected: &Option<String>,
        value: &Value,
    ) {
        let Some(expected) = expected else {
            return;
        };

        if !self.value_matches_type(value, expected) {
            panic!(
                "Function '{}' return type error: expected {}, got {:?}",
                function_name,
                expected,
                value
            );
        }
    }

    // =========================================================================
    // Binary operations
    // =========================================================================

    fn evaluate_binary(
        &self,
        left: Value,
        operator: &Operator,
        right: Value,
    ) -> Value {
        match (left, right) {
            // -----------------------------------------------------------------
            // Numbers
            // -----------------------------------------------------------------

            (
                Value::Number(a),
                Value::Number(b),
            ) => {
                match operator {
                    Operator::Plus => {
                        Value::Number(a + b)
                    }

                    Operator::Minus => {
                        Value::Number(a - b)
                    }

                    Operator::Multiply => {
                        Value::Number(a * b)
                    }

                    Operator::Divide => {
                        if b == 0 {
                            panic!("Division by zero");
                        }

                        Value::Number(a / b)
                    }

                    Operator::Equal => {
                        Value::Boolean(a == b)
                    }

                    Operator::NotEqual => {
                        Value::Boolean(a != b)
                    }

                    Operator::Greater => {
                        Value::Boolean(a > b)
                    }

                    Operator::Less => {
                        Value::Boolean(a < b)
                    }

                    Operator::GreaterEqual => {
                        Value::Boolean(a >= b)
                    }

                    Operator::LessEqual => {
                        Value::Boolean(a <= b)
                    }

                    Operator::And
                    | Operator::Or => {
                        panic!(
                            "Logical operators require boolean operands"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // Floats
            // -----------------------------------------------------------------

            (
                Value::Float(a),
                Value::Float(b),
            ) => {
                match operator {
                    Operator::Plus => {
                        Value::Float(a + b)
                    }

                    Operator::Minus => {
                        Value::Float(a - b)
                    }

                    Operator::Multiply => {
                        Value::Float(a * b)
                    }

                    Operator::Divide => {
                        if b == 0.0 {
                            panic!("Division by zero");
                        }

                        Value::Float(a / b)
                    }

                    Operator::Equal => {
                        Value::Boolean(a == b)
                    }

                    Operator::NotEqual => {
                        Value::Boolean(a != b)
                    }

                    Operator::Greater => {
                        Value::Boolean(a > b)
                    }

                    Operator::Less => {
                        Value::Boolean(a < b)
                    }

                    Operator::GreaterEqual => {
                        Value::Boolean(a >= b)
                    }

                    Operator::LessEqual => {
                        Value::Boolean(a <= b)
                    }

                    Operator::And
                    | Operator::Or => {
                        panic!(
                            "Logical operators require boolean operands"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // Booleans
            // -----------------------------------------------------------------

            (
                Value::Boolean(a),
                Value::Boolean(b),
            ) => {
                match operator {
                    Operator::And => {
                        Value::Boolean(a && b)
                    }

                    Operator::Or => {
                        Value::Boolean(a || b)
                    }

                    Operator::Equal => {
                        Value::Boolean(a == b)
                    }

                    Operator::NotEqual => {
                        Value::Boolean(a != b)
                    }

                    _ => {
                        panic!(
                            "Invalid boolean operation"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // Strings
            // -----------------------------------------------------------------

            (
                Value::String(a),
                Value::String(b),
            ) => {
                match operator {
                    Operator::Plus => {
                        Value::String(
                            format!("{}{}", a, b)
                        )
                    }

                    Operator::Equal => {
                        Value::Boolean(a == b)
                    }

                    Operator::NotEqual => {
                        Value::Boolean(a != b)
                    }

                    _ => {
                        panic!(
                            "Invalid string operation"
                        );
                    }
                }
            }

            // -----------------------------------------------------------------
            // Everything else
            // -----------------------------------------------------------------

            _ => {
                panic!(
                    "Invalid operation between incompatible values"
                );
            }
        }
    }
}