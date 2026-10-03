//contents of interpreter.rs
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    methods: HashMap<(String, String), Function>,
    loop_depth: usize,
    output: Vec<String>,
}

#[derive(Clone)]
pub struct Function {
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub body: Vec<Statement>,
    pub generic_parameters: Vec<String>,
    pub is_async: bool,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            enums: HashMap::new(),
            environment: Environment::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            methods: HashMap::new(),
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
            "File" => Type::File,
            "TcpStream" => Type::TcpStream,
            other if other.ends_with("[]") => {
                Type::Array(Box::new(self.type_from_name(&other[..other.len() - 2])))
            }
            other if other.starts_with("Array<") && other.ends_with('>') => {
                Type::Array(Box::new(self.type_from_name(&other[6..other.len() - 1])))
            }
            other if other.starts_with("Iterator<") && other.ends_with('>') => {
                Type::Iterator(Box::new(self.type_from_name(&other[9..other.len() - 1])))
            }
            other if other.starts_with("Option<") && other.ends_with('>') => {
                Type::Option(Box::new(self.type_from_name(&other[7..other.len() - 1])))
            }
            other => Type::Struct(other.to_string()),
        }
    }

    fn value_matches_type(&self, value: &Value, expected: &str) -> bool {
        match expected {
            "num" => matches!(value, Value::Number(_)),
            "float" => matches!(value, Value::Float(_)),
            "bool" => matches!(value, Value::Boolean(_)),
            "string" => matches!(value, Value::String(_)),
            "File" => matches!(value, Value::File(_)),
            "TcpStream" => matches!(value, Value::TcpStream(_)),

            // T[] syntax
            other if other.ends_with("[]") => {
                let inner = &other[..other.len() - 2];

                match value {
                    Value::Array(elements) => elements
                        .iter()
                        .all(|element| self.value_matches_type(element, inner)),
                    _ => false,
                }
            }

            // Array<T> syntax
            other if other.starts_with("Array<") && other.ends_with('>') => {
                let inner = &other[6..other.len() - 1];

                match value {
                    Value::Array(elements) => elements
                        .iter()
                        .all(|element| self.value_matches_type(element, inner)),
                    _ => false,
                }
            }

            other if other.starts_with("Iterator<") && other.ends_with('>') => {
                matches!(value, Value::Iterator(_))
            }

            other if other.starts_with("HashMap<") && other.ends_with('>') => {
                matches!(value, Value::HashMap(_))
            }

            other if other.starts_with("Task<") && other.ends_with('>') => {
                matches!(value, Value::Task(_))
            }

            other if other.starts_with("Option<") && other.ends_with('>') => {
                let inner = &other[7..other.len() - 1];

                match value {
                    Value::Option(Some(v)) => self.value_matches_type(v, inner),
                    Value::Option(None) => true,
                    _ => false,
                }
            }

            struct_name => match value {
                Value::Struct { name, .. } => name == struct_name,
                _ => false,
            },
        }
    }

    fn check_value_type(&self, context: &str, expected: Option<&String>, value: &Value) {
        let Some(expected) = expected else {
            return;
        };

        if !self.value_matches_type(value, expected) {
            panic!("{}: expected {}, got {:?}", context, expected, value);
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

    fn assign_property(&mut self, object: &Expression, name: &str, value: Value) {
        match object {
            Expression::Identifier {
                name: variable_name,
                ..
            } => {
                let object_value =
                    self.environment
                        .get(variable_name)
                        .cloned()
                        .unwrap_or_else(|| {
                            panic!("Runtime error: unknown variable '{}'", variable_name)
                        });

                match object_value {
                    Value::Struct {
                        name: struct_name,
                        mut fields,
                    } => {
                        if !fields.contains_key(name) {
                            panic!("Unknown field '{}' on struct '{}'", name, struct_name);
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
                            panic!("Unknown field '{}' on struct '{}'", name, struct_name);
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
        generic_parameters: &[String],
    ) {
        let Some(expected) = &parameter.type_name else {
            return;
        };

        // Generic types are compile-time only.
        if generic_parameters.iter().any(|name| name == expected) {
            return;
        }

        let valid = match expected.as_str() {
            "num" => matches!(value, Value::Number(_)),
            "float" => matches!(value, Value::Float(_)),
            "string" => matches!(value, Value::String(_)),
            "bool" => matches!(value, Value::Boolean(_)),
            "File" => matches!(value, Value::File(_)),
            "TcpStream" => matches!(value, Value::TcpStream(_)),

            struct_name => match value {
                Value::Struct { name, .. } => name == struct_name,
                _ => false,
            },
        };

        if !valid {
            panic!(
                "Function '{}' parameter '{}' expects {}, got {:?}",
                function_name, parameter.name, expected, value
            );
        }
    }

    fn call_function(
        &mut self,
        name: &str,
        arguments: &[Expression],
        _generic_arguments: &[String],
    ) -> Value {
        match name {
            "input" => {
                if !arguments.is_empty() {
                    panic!("input() expects no arguments");
                }
                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .expect("failed to read input");
                return Value::String(input.trim_end_matches(&['\r', '\n'][..]).to_string());
            }
            "open" => {
                if arguments.len() != 1 {
                    panic!("open() expects one path argument");
                }
                let path = self.evaluate(&arguments[0]);
                let Value::String(path) = path else {
                    panic!("open() expects a string path");
                };
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&path)
                    .unwrap_or_else(|e| panic!("cannot open '{}': {}", path, e));
                return Value::File(Rc::new(RefCell::new(Some(file))));
            }
            "array" => {
                return Value::Array(arguments.iter().map(|a| self.evaluate(a)).collect());
            }
            "hashmap" => {
                if arguments.len() % 2 != 0 {
                    panic!("hashmap() expects an even number of key/value arguments");
                }
                let mut entries = Vec::new();
                let values: Vec<Value> = arguments.iter().map(|a| self.evaluate(a)).collect();
                for pair in values.chunks(2) {
                    entries.push((pair[0].clone(), pair[1].clone()));
                }
                return Value::HashMap(entries);
            }
            "iterator" => {
                if arguments.len() != 1 {
                    panic!("iterator() expects one array argument");
                }
                match self.evaluate(&arguments[0]) {
                    Value::Array(values) => return Value::Iterator(values),
                    Value::Iterator(values) => return Value::Iterator(values),
                    _ => panic!("iterator() expects an array or iterator"),
                }
            }
            "args" => {
                if !arguments.is_empty() {
                    panic!("args() expects no arguments");
                }
                return Value::Array(env::args().map(Value::String).collect());
            }
            "read_file" => {
                if arguments.len() != 1 {
                    panic!("read_file() expects one path");
                }
                let Value::String(path) = self.evaluate(&arguments[0]) else {
                    panic!("read_file() expects string path");
                };
                return Value::String(
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| panic!("read_file '{}': {}", path, e)),
                );
            }
            "write_file" => {
                if arguments.len() != 2 {
                    panic!("write_file() expects path and contents");
                }
                let Value::String(path) = self.evaluate(&arguments[0]) else {
                    panic!("write_file() expects string path");
                };
                let Value::String(data) = self.evaluate(&arguments[1]) else {
                    panic!("write_file() expects string contents");
                };
                std::fs::write(&path, data)
                    .unwrap_or_else(|e| panic!("write_file '{}': {}", path, e));
                return Value::None;
            }
            "file_exists" => {
                if arguments.len() != 1 {
                    panic!("file_exists() expects one path");
                }
                let Value::String(path) = self.evaluate(&arguments[0]) else {
                    panic!("file_exists() expects string path");
                };
                return Value::Boolean(Path::new(&path).exists());
            }
            "remove_file" => {
                if arguments.len() != 1 {
                    panic!("remove_file() expects one path");
                }
                let Value::String(path) = self.evaluate(&arguments[0]) else {
                    panic!("remove_file() expects string path");
                };
                std::fs::remove_file(&path)
                    .unwrap_or_else(|e| panic!("remove_file '{}': {}", path, e));
                return Value::None;
            }
            "create_dir" => {
                if arguments.len() != 1 {
                    panic!("create_dir() expects one path");
                }
                let Value::String(path) = self.evaluate(&arguments[0]) else {
                    panic!("create_dir() expects string path");
                };
                std::fs::create_dir_all(&path)
                    .unwrap_or_else(|e| panic!("create_dir '{}': {}", path, e));
                return Value::None;
            }
            "list_dir" => {
                if arguments.len() != 1 {
                    panic!("list_dir() expects one path");
                }
                let Value::String(path) = self.evaluate(&arguments[0]) else {
                    panic!("list_dir() expects string path");
                };
                let mut out = Vec::new();
                for entry in std::fs::read_dir(&path)
                    .unwrap_or_else(|e| panic!("list_dir '{}': {}", path, e))
                {
                    let e = entry.unwrap();
                    out.push(Value::String(e.file_name().to_string_lossy().into_owned()));
                }
                return Value::Array(out);
            }
            "sleep_ms" => {
                let ms = match self.evaluate(
                    arguments
                        .get(0)
                        .unwrap_or_else(|| panic!("sleep_ms() expects milliseconds")),
                ) {
                    Value::Number(ms) => ms,
                    other => panic!("sleep_ms() expects a number of milliseconds, got {}", other),
                };
                if arguments.len() != 1 {
                    panic!("sleep_ms() expects one argument");
                }
                std::thread::sleep(Duration::from_millis(ms.max(0) as u64));
                return Value::None;
            }
            "now_ms" => {
                if !arguments.is_empty() {
                    panic!("now_ms() expects no arguments");
                }
                return Value::Number(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64,
                );
            }
            "unix_time" => {
                if !arguments.is_empty() {
                    panic!("unix_time() expects no arguments");
                }
                return Value::Number(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                );
            }
            "date_utc" => {
                if arguments.len() != 1 {
                    panic!("date_utc() expects unix seconds");
                }
                let Value::Number(secs) = self.evaluate(&arguments[0]) else {
                    panic!("date_utc() expects num");
                };
                return Value::String(format_utc_date(secs));
            }
            "random_int" => {
                if arguments.len() != 2 {
                    panic!("random_int() expects min and max");
                }
                let Value::Number(a) = self.evaluate(&arguments[0]) else {
                    panic!("random_int() expects numbers");
                };
                let Value::Number(b) = self.evaluate(&arguments[1]) else {
                    panic!("random_int() expects numbers");
                };
                return Value::Number(random_int(a, b));
            }
            "random_float" => {
                if !arguments.is_empty() {
                    panic!("random_float() expects no arguments");
                }
                return Value::Float(random_float());
            }
            "process_run" => {
                if arguments.len() != 2 {
                    panic!("process_run() expects program and args array");
                }
                let Value::String(program) = self.evaluate(&arguments[0]) else {
                    panic!("process_run() expects program string");
                };
                let Value::Array(args) = self.evaluate(&arguments[1]) else {
                    panic!("process_run() expects string array");
                };
                let mut cmd = Command::new(program);
                for arg in args {
                    let Value::String(a) = arg else {
                        panic!("process_run() args must be strings");
                    };
                    cmd.arg(a);
                }
                let status = cmd
                    .status()
                    .unwrap_or_else(|e| panic!("process_run: {}", e));
                return Value::Number(status.code().unwrap_or(-1) as i64);
            }
            "system" => {
                if !arguments.is_empty() {
                    panic!("system() expects no arguments");
                }
                return Value::String(env::consts::OS.to_string());
            }
            "tcp_connect" => {
                if arguments.len() != 2 {
                    panic!("tcp_connect() expects host and port");
                }
                let Value::String(host) = self.evaluate(&arguments[0]) else {
                    panic!("tcp_connect() host must be string");
                };
                let Value::Number(port) = self.evaluate(&arguments[1]) else {
                    panic!("tcp_connect() port must be num");
                };
                let stream = TcpStream::connect((host.as_str(), port as u16))
                    .unwrap_or_else(|e| panic!("tcp_connect: {}", e));
                return Value::TcpStream(Rc::new(RefCell::new(Some(stream))));
            }
            "serialize" => {
                if arguments.len() != 1 {
                    panic!("serialize() expects one value");
                }
                return Value::String(serialize_value(&self.evaluate(&arguments[0])));
            }
            "deserialize" => {
                if arguments.len() != 1 {
                    panic!("deserialize() expects one string");
                }

                let Value::String(serialized) = self.evaluate(&arguments[0]) else {
                    panic!("deserialize() expects string");
                };

                return parse_serialized(&serialized)
                    .unwrap_or_else(|| panic!("deserialize() could not parse serialized value"));
            }
            "format" => {
                if arguments.len() != 2 {
                    panic!("format() expects template and values array");
                }
                let Value::String(mut template) = self.evaluate(&arguments[0]) else {
                    panic!("format() template must be string");
                };
                let Value::Array(values) = self.evaluate(&arguments[1]) else {
                    panic!("format() values must be array");
                };
                for (i, v) in values.iter().enumerate() {
                    template = template.replace(&format!("{{{}}}", i), &v.to_string());
                }
                return Value::String(template);
            }
            "hex_encode" => {
                if arguments.len() != 1 {
                    panic!("hex_encode() expects string");
                }
                let Value::String(s) = self.evaluate(&arguments[0]) else {
                    panic!("hex_encode() expects string");
                };
                return Value::String(s.as_bytes().iter().map(|b| format!("{:02x}", b)).collect());
            }
            "hex_decode" => {
                if arguments.len() != 1 {
                    panic!("hex_decode() expects string");
                }
                let Value::String(s) = self.evaluate(&arguments[0]) else {
                    panic!("hex_decode() expects string");
                };
                return Value::String(hex_decode(&s));
            }
            "base64_encode" => {
                if arguments.len() != 1 {
                    panic!("base64_encode() expects string");
                }
                let Value::String(s) = self.evaluate(&arguments[0]) else {
                    panic!("base64_encode() expects string");
                };
                return Value::String(base64_encode(s.as_bytes()));
            }
            "base64_decode" => {
                if arguments.len() != 1 {
                    panic!("base64_decode() expects string");
                }
                let Value::String(s) = self.evaluate(&arguments[0]) else {
                    panic!("base64_decode() expects string");
                };
                return Value::String(base64_decode(&s));
            }
            "log_debug" | "log_info" | "log_warn" | "log_error" => {
                if arguments.len() != 1 {
                    panic!("log_*() expects one message");
                }
                let value = self.evaluate(&arguments[0]);
                self.output
                    .push(format!("[{}] {}", &name[4..].to_uppercase(), value));
                return Value::None;
            }
            "print" => {
                for argument in arguments {
                    let value = self.evaluate(argument);
                    self.output.push(format!("{}", value));
                }
                return Value::None;
            }
            _ => {}
        }

        let function = self
            .functions
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("Unknown function '{}'", name));
        let values: Vec<Value> = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect();
        self.invoke_function(name, &function, values)
    }

    fn invoke_function(&mut self, name: &str, function: &Function, values: Vec<Value>) -> Value {
        if values.len() != function.parameters.len() {
            panic!(
                "Function '{}' expects {} arguments, got {}",
                name,
                function.parameters.len(),
                values.len()
            );
        }
        for (parameter, value) in function.parameters.iter().zip(values.iter()) {
            self.check_parameter_type(name, parameter, value, &function.generic_parameters);
        }
        let previous_loop_depth = self.loop_depth;
        self.loop_depth = 0;
        self.environment.push_scope();
        for (parameter, value) in function.parameters.iter().zip(values.into_iter()) {
            self.environment
                .declare(parameter.name.clone(), value, true);
        }
        let mut flow = Flow::Normal;
        for statement in &function.body {
            flow = self.execute_statement(statement);
            if !matches!(flow, Flow::Normal) {
                break;
            }
        }
        let returned_value = match flow {
            Flow::Return(value) => value,
            Flow::Normal => {
                if function.return_type.is_some() {
                    self.exit_scope();
                    self.loop_depth = previous_loop_depth;
                    panic!("Function '{}' expected a return value", name);
                }
                Value::None
            }
            Flow::Break => {
                self.exit_scope();
                self.loop_depth = previous_loop_depth;
                panic!("break escaped function '{}'", name);
            }
            Flow::Continue => {
                self.exit_scope();
                self.loop_depth = previous_loop_depth;
                panic!("continue escaped function '{}'", name);
            }
        };
        self.exit_scope();
        self.loop_depth = previous_loop_depth;

        // The declared return type of an async function is its
        // source-level result type (e.g. `num`), while the runtime
        // value returned by calling it is `Task<num>`.
        self.check_return_type(
            name,
            &function.return_type,
            &returned_value,
            &function.generic_parameters,
        );

        if function.is_async {
            Value::Task(Box::new(returned_value))
        } else {
            returned_value
        }
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
            if let Statement::Struct { name, fields, .. } = statement {
                if self.structs.contains_key(name) {
                    panic!("Duplicate struct '{}'", name);
                }

                let mut field_list = Vec::new();

                for field in fields {
                    if field_list
                        .iter()
                        .any(|(field_name, _)| field_name == &field.name)
                    {
                        panic!("Duplicate field '{}' in struct '{}'", field.name, name);
                    }

                    field_list.push((field.name.clone(), self.type_from_name(&field.type_name)));
                }

                self.structs
                    .insert(name.clone(), StructDefinition { fields: field_list });
            }
        }

        // ---------------------------------------------------------------------
        // Pass 2: register enums
        // ---------------------------------------------------------------------

        for statement in &program.statements {
            if let Statement::Enum { name, variants, .. } = statement {
                if self.enums.contains_key(name) {
                    panic!("Duplicate enum '{}'", name);
                }

                let mut variant_map = HashMap::new();

                for variant in variants {
                    if variant_map.contains_key(&variant.name) {
                        panic!("Duplicate variant '{}' in enum '{}'", variant.name, name);
                    }

                    let fields = variant
                        .fields
                        .iter()
                        .map(|field| self.type_from_name(field))
                        .collect();

                    variant_map.insert(variant.name.clone(), EnumVariantDefinition { fields });
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
                generic_parameters,
                is_async,
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
                        generic_parameters: generic_parameters.clone(),
                        is_async: *is_async,
                    },
                );
            }
        }

        // ---------------------------------------------------------------------
        // Pass 4: register impl methods
        // ---------------------------------------------------------------------
        for statement in &program.statements {
            if let Statement::Impl {
                type_name, methods, ..
            } = statement
            {
                for method in methods {
                    if let Statement::Function {
                        name,
                        parameters,
                        return_type,
                        body,
                        generic_parameters,
                        is_async,
                        ..
                    } = method
                    {
                        self.methods.insert(
                            (type_name.clone(), name.clone()),
                            Function {
                                parameters: parameters.clone(),
                                return_type: return_type.clone(),
                                body: body.clone(),
                                generic_parameters: generic_parameters.clone(),
                                is_async: *is_async,
                            },
                        );
                    }
                }
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
                | Statement::Impl { .. }
                | Statement::Import { .. } => {}

                _ => match self.execute_statement(statement) {
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
                },
            }
        }
    }

    // =========================================================================
    // Statement execution
    // =========================================================================

    fn execute_statement(&mut self, statement: &Statement) -> Flow {
        match statement {
            // -----------------------------------------------------------------
            // Variable declarations
            // -----------------------------------------------------------------
            Statement::VariableDeclarations { declarations, .. } => {
                for declaration in declarations {
                    if let Some(expression) = &declaration.value {
                        let value = self.evaluate(expression);

                        self.check_value_type(
                            &format!("Variable '{}' type error", declaration.name),
                            declaration.declared_type.as_ref(),
                            &value,
                        );

                        self.environment
                            .declare(declaration.name.clone(), value, true);
                    }
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

                self.environment.declare(name.clone(), result, false);

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Assignment
            // -----------------------------------------------------------------
            Statement::Assignment { target, value, .. } => {
                let result = self.evaluate(value);

                match target {
                    Expression::Identifier { name, .. } => {
                        if self.environment.get(name).is_some() {
                            if let Err(error) = self.environment.assign(name, result) {
                                panic!("{}", error);
                            }
                        } else {
                            // Preserve the existing language behavior:
                            // assigning an unknown name creates a variable.
                            self.environment.declare(name.clone(), result, true);
                        }
                    }

                    Expression::Property { object, name, .. } => {
                        self.assign_property(object, name, result);
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
            Statement::Expression { expression, .. } => {
                self.evaluate(expression);
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Call statement
            // -----------------------------------------------------------------
            Statement::Call { expression, .. } => {
                self.evaluate(expression);
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Defer
            // -----------------------------------------------------------------
            Statement::Defer { expression, .. } => {
                self.environment.add_defer(expression.clone());
                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Return
            // -----------------------------------------------------------------
            Statement::Return { value, .. } => {
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
            Statement::Main { body, .. } => {
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
                    Value::Boolean(true) => self.execute_scoped_block(body),

                    Value::Boolean(false) => match else_body {
                        Some(statements) => self.execute_scoped_block(statements),

                        None => Flow::Normal,
                    },

                    _ => {
                        panic!("If condition must be boolean");
                    }
                }
            }

            // -----------------------------------------------------------------
            // While
            // -----------------------------------------------------------------
            Statement::While {
                condition, body, ..
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

                            panic!("While condition must be boolean");
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
                        panic!("For loop start must be an integer");
                    }
                };

                let end_number = match end_value {
                    Value::Number(value) => value,
                    _ => {
                        panic!("For loop end must be an integer");
                    }
                };

                self.environment.push_scope();
                self.loop_depth += 1;

                for i in start_number..end_number {
                    // Fresh scope per iteration.
                    self.environment.push_scope();

                    self.environment.set(variable.clone(), Value::Number(i));

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
            // ForEach
            // -----------------------------------------------------------------
            Statement::ForEach {
                variable,
                iterable,
                body,
                ..
            } => {
                let iterable_value = self.evaluate(iterable);

                let values = match iterable_value {
                    Value::Array(values) | Value::Iterator(values) => values,
                    Value::HashMap(entries) => entries
                        .into_iter()
                        .map(|(k, v)| Value::Array(vec![k, v]))
                        .collect(),
                    _ => {
                        panic!("ForEach iterable must be an array, iterator, or hash map");
                    }
                };

                self.environment.push_scope();
                self.loop_depth += 1;

                for value in values {
                    // Fresh scope per iteration.
                    self.environment.push_scope();

                    self.environment.declare(variable.clone(), value, true);

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
            // Match
            // -----------------------------------------------------------------
            Statement::Match {
                expression, arms, ..
            } => {
                let value = self.evaluate(expression);

                for arm in arms {
                    if let Some(bindings) = self.pattern_matches(&arm.pattern, &value) {
                        self.environment.push_scope();

                        for (name, binding_value) in bindings {
                            self.environment.declare(name, binding_value, true);
                        }

                        let flow = self.execute_statement_list(&arm.body);

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
                generic_parameters,
                is_async,
                ..
            } => {
                self.functions.insert(
                    name.clone(),
                    Function {
                        parameters: parameters.clone(),
                        return_type: return_type.clone(),
                        body: body.clone(),
                        generic_parameters: generic_parameters.clone(),
                        is_async: *is_async,
                    },
                );

                Flow::Normal
            }

            // -----------------------------------------------------------------
            // Struct declaration
            // -----------------------------------------------------------------
            Statement::Struct { .. } => Flow::Normal,

            // -----------------------------------------------------------------
            // Enum declaration
            // -----------------------------------------------------------------
            Statement::Enum { name, variants, .. } => {
                let mut variant_map = HashMap::new();

                for variant in variants {
                    let fields = variant
                        .fields
                        .iter()
                        .map(|field| self.type_from_name(field))
                        .collect();

                    variant_map.insert(variant.name.clone(), EnumVariantDefinition { fields });
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
            Statement::Trait { .. } => Flow::Normal,

            Statement::Impl { .. } => Flow::Normal,
            Statement::Import { .. } => Flow::Normal,
        }
    }

    fn execute_statement_list(&mut self, statements: &[Statement]) -> Flow {
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

    fn pattern_matches(&self, pattern: &Pattern, value: &Value) -> Option<Vec<(String, Value)>> {
        match (&pattern.kind, value) {
            // -----------------------------------------------------------------
            // Wildcard
            // -----------------------------------------------------------------
            (PatternKind::Wildcard, _) => Some(Vec::new()),

            // -----------------------------------------------------------------
            // Identifier binding
            // -----------------------------------------------------------------
            (PatternKind::Identifier(name), value) => Some(vec![(name.clone(), value.clone())]),

            // -----------------------------------------------------------------
            // Number
            // -----------------------------------------------------------------
            (PatternKind::Number(expected), Value::Number(actual)) if expected == actual => {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // Float
            // -----------------------------------------------------------------
            (PatternKind::Float(expected), Value::Float(actual)) if expected == actual => {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // String
            // -----------------------------------------------------------------
            (PatternKind::String(expected), Value::String(actual)) if expected == actual => {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // Boolean
            // -----------------------------------------------------------------
            (PatternKind::Boolean(expected), Value::Boolean(actual)) if expected == actual => {
                Some(Vec::new())
            }

            // -----------------------------------------------------------------
            // Enum variant
            // -----------------------------------------------------------------
            (
                PatternKind::Variant { name, bindings },
                Value::Enum {
                    enum_name,
                    variant,
                    values,
                },
            ) => {
                let expected_name = format!("{}::{}", enum_name, variant);

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
            Expression::Number { value, .. } => Value::Number(*value),

            Expression::Float { value, .. } => Value::Float(*value),

            Expression::Boolean { value, .. } => Value::Boolean(*value),

            Expression::String { value, .. } => Value::String(value.clone()),

            // -----------------------------------------------------------------
            // Identifier
            // -----------------------------------------------------------------
            Expression::Identifier { name, .. } => self
                .environment
                .get(name)
                .cloned()
                .unwrap_or_else(|| panic!("Runtime error: unknown variable '{}'", name)),

            // -----------------------------------------------------------------
            // Array
            // -----------------------------------------------------------------
            Expression::Array { elements, .. } => {
                let values = elements
                    .iter()
                    .map(|element| self.evaluate(element))
                    .collect();

                Value::Array(values)
            }

            // -----------------------------------------------------------------
            // Indexing
            // -----------------------------------------------------------------
            Expression::Index { array, index, .. } => {
                let array_value = self.evaluate(array);
                let index_value = self.evaluate(index);

                match (array_value, index_value) {
                    (Value::Array(values), Value::Number(index)) => {
                        if index < 0 {
                            panic!("Array index out of bounds");
                        }

                        values
                            .get(index as usize)
                            .cloned()
                            .unwrap_or_else(|| panic!("Array index out of bounds"))
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
            Expression::Property { object, name, .. } => {
                let value = self.evaluate(object);

                match value {
                    Value::Struct { fields, .. } => fields
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| panic!("Unknown field '{}'", name)),

                    _ => {
                        panic!("Property access requires a struct");
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
            } => self.evaluate_method_call(object, method, arguments),

            // -----------------------------------------------------------------
            // Await
            // -----------------------------------------------------------------
            // Async runtime semantics are not implemented yet.
            //
            // For the 0.2 interpreter, await simply evaluates the wrapped
            // expression and returns its value. The real Task<T> runtime
            // behavior will be introduced with async/await support.
            Expression::Await { expression, .. } => match self.evaluate(expression) {
                Value::Task(value) => *value,
                other => panic!("await expects Task<T>, got {}", other),
            },

            // -----------------------------------------------------------------
            // Function calls
            // -----------------------------------------------------------------
            Expression::Call {
                name, arguments, ..
            } => {
                // Struct constructor.
                if let Some(struct_definition) = self.structs.get(name).cloned() {
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

                        if !self.value_matches_type_value(&value, expected_type) {
                            panic!(
                                "Invalid value for field '{}.{}': expected {:?}, got {:?}",
                                name, field_name, expected_type, value
                            );
                        }

                        fields.insert(field_name.clone(), value);
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
            Expression::StructConstructor { name, fields, .. } => {
                let definition = self.structs.get(name).cloned();

                if definition.is_none() {
                    panic!("Unknown struct '{}'", name);
                }

                let definition = definition.unwrap();

                let mut result = HashMap::new();

                for (field_name, expression) in fields {
                    let expected_type = definition
                        .fields
                        .iter()
                        .find(|(name, _)| name == field_name)
                        .map(|(_, ty)| ty);

                    let Some(expected_type) = expected_type else {
                        panic!("Unknown field '{}' on struct '{}'", field_name, name);
                    };

                    let value = self.evaluate(expression);

                    if !self.value_matches_type_value(&value, expected_type) {
                        panic!(
                            "Invalid value for field '{}.{}': expected {:?}, got {:?}",
                            name, field_name, expected_type, value
                        );
                    }

                    result.insert(field_name.clone(), value);
                }

                // Require all declared fields.
                // Require all declared fields.
                for (field_name, _) in &definition.fields {
                    if !result.contains_key(field_name) {
                        panic!(
                            "Missing field '{}' in struct constructor '{}'",
                            field_name, name
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
                let enum_definition = self.enums.get(enum_name).cloned();

                let Some(enum_definition) = enum_definition else {
                    panic!("Unknown enum '{}'", enum_name);
                };

                let variant_definition = enum_definition.variants.get(variant).cloned();

                let Some(variant_definition) = variant_definition else {
                    panic!("Unknown variant '{}::{}'", enum_name, variant);
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

                for (argument, expected_type) in
                    arguments.iter().zip(variant_definition.fields.iter())
                {
                    let value = self.evaluate(argument);

                    if !self.value_matches_type_value(&value, expected_type) {
                        panic!(
                            "Invalid value in enum variant '{}::{}': expected {:?}, got {:?}",
                            enum_name, variant, expected_type, value
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
            // Type conversion
            // -----------------------------------------------------------------
            Expression::Conversion {
                kind,
                expression,
                target_type,
                ..
            } => {
                let value = self.evaluate(expression);
                self.convert_value(value, kind, target_type)
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

                self.evaluate_binary(left_value, operator, right_value)
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
                    UnaryOperator::Negate => match value {
                        Value::Number(value) => Value::Number(-value),

                        Value::Float(value) => Value::Float(-value),

                        _ => {
                            panic!("Unary '-' requires a numeric value");
                        }
                    },

                    UnaryOperator::Not => match value {
                        Value::Boolean(value) => Value::Boolean(!value),

                        _ => {
                            panic!("Unary 'not' requires a boolean value");
                        }
                    },
                }
            }
        }
    }

    // =========================================================================
    // Value / Type compatibility
    // =========================================================================

    fn value_matches_type_value(&self, value: &Value, expected: &Type) -> bool {
        match expected {
            Type::Num => matches!(value, Value::Number(_)),
            Type::Float => matches!(value, Value::Float(_)),
            Type::Bool => matches!(value, Value::Boolean(_)),
            Type::String => matches!(value, Value::String(_)),
            Type::File => matches!(value, Value::File(_)),
            Type::Array(inner) => match value {
                Value::Array(values) => values
                    .iter()
                    .all(|v| self.value_matches_type_value(v, inner)),
                _ => false,
            },
            Type::Iterator(inner) => match value {
                Value::Iterator(values) => values
                    .iter()
                    .all(|v| self.value_matches_type_value(v, inner)),
                _ => false,
            },
            Type::HashMap(_, _) => matches!(value, Value::HashMap(_)),
            Type::Option(inner) => match value {
                Value::Option(Some(v)) => self.value_matches_type_value(v, inner),
                Value::Option(None) => true,
                _ => false,
            },

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

    fn convert_value(&mut self, value: Value, kind: &ConversionKind, target: &str) -> Value {
        let explicit = |value: Value, target: &str| -> Result<Value, String> {
            match (value, target) {
                (Value::Number(v), "float") => Ok(Value::Float(v as f64)),
                (Value::Float(v), "num") => Ok(Value::Number(v as i64)),
                (Value::Number(v), "string") => Ok(Value::String(v.to_string())),
                (Value::Float(v), "string") => Ok(Value::String(v.to_string())),
                (Value::Boolean(v), "string") => Ok(Value::String(v.to_string())),
                (Value::String(v), "num") => v
                    .parse::<i64>()
                    .map(Value::Number)
                    .map_err(|e| e.to_string()),
                (Value::String(v), "float") => v
                    .parse::<f64>()
                    .map(Value::Float)
                    .map_err(|e| e.to_string()),
                (Value::String(v), "bool") => match v.as_str() {
                    "true" => Ok(Value::Boolean(true)),
                    "false" => Ok(Value::Boolean(false)),
                    _ => Err("expected 'true' or 'false'".into()),
                },
                (Value::Boolean(v), "num") => Ok(Value::Number(if v { 1 } else { 0 })),
                (Value::Number(v), "bool") => Ok(Value::Boolean(v != 0)),
                (Value::Float(v), "bool") => Ok(Value::Boolean(v != 0.0)),
                (v, t) if self.value_matches_type(&v, t) => Ok(v),
                (_, t) => Err(format!("cannot convert to {}", t)),
            }
        };
        match kind {
            ConversionKind::As => {
                explicit(value, target).unwrap_or_else(|e| panic!("invalid 'as' conversion: {}", e))
            }
            ConversionKind::Try => {
                let function = format!("try_{}", target);
                if self.functions.contains_key(&function) {
                    let saved = value.clone();
                    self.environment.push_scope();
                    self.environment
                        .declare("__conversion_value".into(), saved, true);
                    let result = self.call_function(
                        &function,
                        &[Expression::Identifier {
                            name: "__conversion_value".into(),
                            span: crate::span::Span::point(0),
                        }],
                        &[],
                    );
                    self.environment.pop_scope();
                    result
                } else {
                    Value::Option(explicit(value, target).ok().map(Box::new))
                }
            }
            ConversionKind::From => {
                let function = format!("from_{}", target);
                if self.functions.contains_key(&function) {
                    let saved = value.clone();
                    self.environment.push_scope();
                    self.environment
                        .declare("__conversion_value".into(), saved, true);
                    let result = self.call_function(
                        &function,
                        &[Expression::Identifier {
                            name: "__conversion_value".into(),
                            span: crate::span::Span::point(0),
                        }],
                        &[],
                    );
                    self.environment.pop_scope();
                    return result;
                }
                explicit(value, target)
                    .unwrap_or_else(|e| panic!("invalid 'from' conversion: {}", e))
            }
        }
    }

    fn evaluate_method_call(
        &mut self,
        object: &Expression,
        method: &str,
        arguments: &[Expression],
    ) -> Value {
        let object_value = self.evaluate(object);

        match object_value.clone() {
            Value::TcpStream(stream) => match method {
                "send" => {
                    if arguments.len() != 1 {
                        panic!("send() expects one string");
                    }
                    let Value::String(text) = self.evaluate(&arguments[0]) else {
                        panic!("send() expects string");
                    };
                    let mut b = stream.borrow_mut();
                    let Some(s) = b.as_mut() else {
                        panic!("stream is closed");
                    };
                    s.write_all(text.as_bytes()).unwrap();
                    Value::None
                }
                "receive" => {
                    if !arguments.is_empty() {
                        panic!("receive() expects no arguments");
                    }
                    let mut b = stream.borrow_mut();
                    let Some(s) = b.as_mut() else {
                        panic!("stream is closed");
                    };
                    let mut buf = String::new();
                    s.read_to_string(&mut buf).unwrap();
                    Value::String(buf)
                }
                "close" => {
                    if !arguments.is_empty() {
                        panic!("close() expects no arguments");
                    }
                    stream.borrow_mut().take();
                    Value::None
                }
                _ => panic!("Unknown TcpStream method '{}'", method),
            },
            Value::File(file) => match method {
                "read" => {
                    if !arguments.is_empty() {
                        panic!("read() expects no arguments");
                    }
                    let mut borrowed = file.borrow_mut();
                    let Some(f) = borrowed.as_mut() else {
                        panic!("file is closed");
                    };
                    f.seek(std::io::SeekFrom::Start(0))
                        .expect("failed to seek file");
                    let mut text = String::new();
                    f.read_to_string(&mut text).expect("failed to read file");
                    Value::String(text)
                }
                "write" => {
                    if arguments.len() != 1 {
                        panic!("write() expects one argument");
                    }
                    let text = self.evaluate(&arguments[0]);
                    let Value::String(text) = text else {
                        panic!("write() expects a string");
                    };
                    let mut borrowed = file.borrow_mut();
                    let Some(f) = borrowed.as_mut() else {
                        panic!("file is closed");
                    };
                    f.write_all(text.as_bytes()).expect("failed to write file");
                    f.flush().expect("failed to write file");
                    Value::None
                }
                "close" => {
                    if !arguments.is_empty() {
                        panic!("close() expects no arguments");
                    }
                    file.borrow_mut().take();
                    Value::None
                }
                _ => panic!("Unknown File method '{}'", method),
            },
            Value::Array(values) => self.evaluate_array_method(object, values, method, arguments),
            Value::Iterator(values) => self.evaluate_iterator_method(values, method, arguments),
            Value::HashMap(entries) => {
                self.evaluate_hashmap_method(object, entries, method, arguments)
            }
            Value::String(text) => self.evaluate_string_method(text, method, arguments),
            Value::Struct { name, .. } => {
                let function = self
                    .methods
                    .get(&(name.clone(), method.to_string()))
                    .cloned()
                    .unwrap_or_else(|| panic!("Unknown method '{}.{}'", name, method));
                let mut values = Vec::with_capacity(arguments.len() + 1);
                values.push(object_value);
                values.extend(arguments.iter().map(|a| self.evaluate(a)));
                self.invoke_function(&format!("{}.{}", name, method), &function, values)
            }
            _ => panic!("Type has no method '{}'", method),
        }
    }

    fn evaluate_array_method(
        &mut self,
        object: &Expression,
        values: Vec<Value>,
        method: &str,
        arguments: &[Expression],
    ) -> Value {
        match method {
            "add" | "push" => {
                if arguments.len() != 1 {
                    panic!("{}() expects one argument", method);
                }
                let value = self.evaluate(&arguments[0]);
                match object {
                    Expression::Identifier { name, .. } => {
                        let array = self
                            .environment
                            .get_mut(name)
                            .unwrap_or_else(|| panic!("Unknown array '{}'", name));
                        if let Value::Array(items) = array {
                            items.push(value);
                            Value::None
                        } else {
                            panic!("{}() requires an array", method)
                        }
                    }
                    _ => panic!("{}() requires an array variable", method),
                }
            }
            "remove_last" | "pop" => {
                if !arguments.is_empty() {
                    panic!("{}() expects no arguments", method);
                }
                match object {
                    Expression::Identifier { name, .. } => match self.environment.get_mut(name) {
                        Some(Value::Array(items)) => items.pop().unwrap_or(Value::None),
                        _ => panic!("{}() requires an array variable", method),
                    },
                    _ => panic!("{}() requires an array variable", method),
                }
            }
            "access" => {
                if arguments.len() != 1 {
                    panic!("access() expects one index");
                }
                let Value::Number(i) = self.evaluate(&arguments[0]) else {
                    panic!("access() expects a numeric index");
                };
                if i < 0 {
                    panic!("array index out of bounds");
                }
                values
                    .get(i as usize)
                    .cloned()
                    .unwrap_or_else(|| panic!("array index out of bounds"))
            }
            "modify" => {
                if arguments.len() != 2 {
                    panic!("modify() expects index and value");
                }
                let Value::Number(i) = self.evaluate(&arguments[0]) else {
                    panic!("modify() expects a numeric index");
                };
                let value = self.evaluate(&arguments[1]);
                if i < 0 {
                    panic!("array index out of bounds");
                }
                match object {
                    Expression::Identifier { name, .. } => match self.environment.get_mut(name) {
                        Some(Value::Array(items)) => {
                            let slot = items
                                .get_mut(i as usize)
                                .unwrap_or_else(|| panic!("array index out of bounds"));
                            *slot = value;
                            Value::None
                        }
                        _ => panic!("modify() requires an array variable"),
                    },
                    _ => panic!("modify() requires an array variable"),
                }
            }
            "get_length" | "length" => {
                if !arguments.is_empty() {
                    panic!("{}() expects no arguments", method)
                };
                Value::Number(values.len() as i64)
            }
            "clear" => {
                if !arguments.is_empty() {
                    panic!("clear() expects no arguments")
                };
                match object {
                    Expression::Identifier { name, .. } => {
                        if let Some(Value::Array(items)) = self.environment.get_mut(name) {
                            items.clear();
                            Value::None
                        } else {
                            panic!("clear() requires an array variable")
                        }
                    }
                    _ => panic!("clear() requires an array variable"),
                }
            }
            "insert_at_index" => {
                if arguments.len() != 2 {
                    panic!("insert_at_index() expects index and value")
                };
                let Value::Number(i) = self.evaluate(&arguments[0]) else {
                    panic!("index must be num")
                };
                let value = self.evaluate(&arguments[1]);
                if i < 0 {
                    panic!("index out of bounds")
                };
                match object {
                    Expression::Identifier { name, .. } => match self.environment.get_mut(name) {
                        Some(Value::Array(items)) => {
                            if i as usize > items.len() {
                                panic!("index out of bounds")
                            };
                            items.insert(i as usize, value);
                            Value::None
                        }
                        _ => panic!("insert_at_index() requires array"),
                    },
                    _ => panic!("insert_at_index() requires array"),
                }
            }
            "remove_at_index" => {
                if arguments.len() != 1 {
                    panic!("remove_at_index() expects index")
                };
                let Value::Number(i) = self.evaluate(&arguments[0]) else {
                    panic!("index must be num")
                };
                if i < 0 {
                    panic!("index out of bounds")
                };
                match object {
                    Expression::Identifier { name, .. } => match self.environment.get_mut(name) {
                        Some(Value::Array(items)) => items.remove(i as usize),
                        _ => panic!("remove_at_index() requires array"),
                    },
                    _ => panic!("remove_at_index() requires array"),
                }
            }
            "contains" => {
                if arguments.len() != 1 {
                    panic!("contains() expects one argument")
                };
                let v = self.evaluate(&arguments[0]);
                Value::Boolean(values.iter().any(|x| x == &v))
            }
            "sort" => {
                if !arguments.is_empty() {
                    panic!("sort() expects no arguments")
                };
                match object {
                    Expression::Identifier { name, .. } => {
                        if let Some(Value::Array(items)) = self.environment.get_mut(name) {
                            items.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
                            Value::None
                        } else {
                            panic!("sort() requires array")
                        }
                    }
                    _ => panic!("sort() requires array"),
                }
            }
            "reverse" => {
                if !arguments.is_empty() {
                    panic!("reverse() expects no arguments")
                };
                match object {
                    Expression::Identifier { name, .. } => {
                        if let Some(Value::Array(items)) = self.environment.get_mut(name) {
                            items.reverse();
                            Value::None
                        } else {
                            panic!("reverse() requires array")
                        }
                    }
                    _ => panic!("reverse() requires array"),
                }
            }
            "iterate" => Value::Iterator(values),
            "map" | "filter" | "enumerate" | "collect" | "find" | "any" | "all" | "fold" => {
                self.evaluate_iterator_method(values, method, arguments)
            }
            _ => panic!("Unknown array method '{}'", method),
        }
    }

    fn callback_name(arguments: &[Expression], index: usize) -> String {
        match arguments.get(index) {
            Some(Expression::Identifier { name, .. }) => name.clone(),
            _ => panic!("callback must be a function name"),
        }
    }

    fn evaluate_iterator_method(
        &mut self,
        values: Vec<Value>,
        method: &str,
        arguments: &[Expression],
    ) -> Value {
        match method {
            "map" => {
                if arguments.len() != 1 {
                    panic!("map() expects one callback")
                };
                let name = Self::callback_name(arguments, 0);
                let f = self
                    .functions
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| panic!("Unknown callback '{}'", name));
                Value::Iterator(
                    values
                        .into_iter()
                        .map(|v| self.invoke_function(&name, &f, vec![v]))
                        .collect(),
                )
            }
            "filter" => {
                if arguments.len() != 1 {
                    panic!("filter() expects one callback")
                };
                let name = Self::callback_name(arguments, 0);
                let f = self
                    .functions
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| panic!("Unknown callback '{}'", name));
                Value::Iterator(
                    values
                        .into_iter()
                        .filter(|v| {
                            matches!(
                                self.invoke_function(&name, &f, vec![v.clone()]),
                                Value::Boolean(true)
                            )
                        })
                        .collect(),
                )
            }
            "enumerate" => Value::Iterator(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| Value::Array(vec![Value::Number(i as i64), v]))
                    .collect(),
            ),
            "collect" => Value::Array(values),
            "find" => {
                if arguments.len() != 1 {
                    panic!("find() expects one callback")
                };
                let name = Self::callback_name(arguments, 0);
                let f = self
                    .functions
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| panic!("Unknown callback '{}'", name));
                Value::Option(
                    values
                        .into_iter()
                        .find(|v| {
                            matches!(
                                self.invoke_function(&name, &f, vec![v.clone()]),
                                Value::Boolean(true)
                            )
                        })
                        .map(Box::new),
                )
            }
            "any" | "all" => {
                if arguments.len() != 1 {
                    panic!("{}() expects one callback", method)
                };
                let name = Self::callback_name(arguments, 0);
                let f = self
                    .functions
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| panic!("Unknown callback '{}'", name));
                let mut iter = values.into_iter().map(|v| {
                    matches!(
                        self.invoke_function(&name, &f, vec![v]),
                        Value::Boolean(true)
                    )
                });
                Value::Boolean(if method == "any" {
                    iter.any(|x| x)
                } else {
                    iter.all(|x| x)
                })
            }
            "fold" => {
                if arguments.len() != 2 {
                    panic!("fold() expects initial value and callback")
                };
                let mut acc = self.evaluate(&arguments[0]);
                let name = Self::callback_name(arguments, 1);
                let f = self
                    .functions
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| panic!("Unknown callback '{}'", name));
                for v in values {
                    acc = self.invoke_function(&name, &f, vec![acc, v]);
                }
                acc
            }
            _ => panic!("Unknown iterator method '{}'", method),
        }
    }

    fn evaluate_hashmap_method(
        &mut self,
        object: &Expression,
        mut entries: Vec<(Value, Value)>,
        method: &str,
        arguments: &[Expression],
    ) -> Value {
        match method {
            "insert" => {
                if arguments.len() != 2 {
                    panic!("insert() expects key and value")
                };
                let k = self.evaluate(&arguments[0]);
                let v = self.evaluate(&arguments[1]);
                if let Some((_, old)) = entries.iter_mut().find(|(key, _)| key == &k) {
                    *old = v.clone();
                } else {
                    entries.push((k, v));
                }
                self.replace_hashmap(object, entries);
                Value::None
            }
            "get" => {
                if arguments.len() != 1 {
                    panic!("get() expects key")
                };
                let k = self.evaluate(&arguments[0]);
                Value::Option(
                    entries
                        .iter()
                        .find(|(key, _)| key == &k)
                        .map(|(_, v)| Box::new(v.clone())),
                )
            }
            "remove" => {
                if arguments.len() != 1 {
                    panic!("remove() expects key")
                };
                let k = self.evaluate(&arguments[0]);
                let pos = entries.iter().position(|(key, _)| key == &k);
                let out = pos.map(|i| entries.remove(i).1);
                self.replace_hashmap(object, entries);
                out.map_or(Value::Option(None), |v| Value::Option(Some(Box::new(v))))
            }
            "contains_key" => {
                if arguments.len() != 1 {
                    panic!("contains_key() expects key")
                };
                let k = self.evaluate(&arguments[0]);
                Value::Boolean(entries.iter().any(|(key, _)| key == &k))
            }
            "get_length" | "length" => Value::Number(entries.len() as i64),
            "clear" => {
                self.replace_hashmap(object, Vec::new());
                Value::None
            }
            "keys" => Value::Array(entries.into_iter().map(|(k, _)| k).collect()),
            "values" => Value::Array(entries.into_iter().map(|(_, v)| v).collect()),
            "iterate" => Value::Iterator(
                entries
                    .into_iter()
                    .map(|(k, v)| Value::Array(vec![k, v]))
                    .collect(),
            ),
            _ => panic!("Unknown hash map method '{}'", method),
        }
    }

    fn replace_hashmap(&mut self, object: &Expression, entries: Vec<(Value, Value)>) {
        match object {
            Expression::Identifier { name, .. } => match self.environment.get_mut(name) {
                Some(slot) => *slot = Value::HashMap(entries),
                None => panic!("Unknown hash map variable '{}'", name),
            },
            _ => panic!("hash map mutation requires a variable"),
        }
    }

    fn evaluate_string_method(
        &mut self,
        text: String,
        method: &str,
        arguments: &[Expression],
    ) -> Value {
        match method {
            "create" => Value::String(text),
            "concatenate" => {
                if arguments.len() != 1 {
                    panic!("concatenate() expects one argument")
                };
                let v = self.evaluate(&arguments[0]);
                let Value::String(v) = v else {
                    panic!("concatenate() expects string")
                };
                Value::String(text + &v)
            }
            "substring" => {
                if arguments.len() != 2 {
                    panic!("substring() expects start and end")
                };
                let Value::Number(a) = self.evaluate(&arguments[0]) else {
                    panic!("start must be num")
                };
                let Value::Number(b) = self.evaluate(&arguments[1]) else {
                    panic!("end must be num")
                };
                if a < 0 || b < a {
                    panic!("invalid substring range")
                };
                Value::String(
                    text.chars()
                        .skip(a as usize)
                        .take((b - a) as usize)
                        .collect(),
                )
            }
            "find" => {
                if arguments.len() != 1 {
                    panic!("find() expects one string")
                };
                let Value::String(q) = self.evaluate(&arguments[0]) else {
                    panic!("find() expects string")
                };
                Value::Option(text.find(&q).map(|i| Box::new(Value::Number(i as i64))))
            }
            "replace" => {
                if arguments.len() != 2 {
                    panic!("replace() expects two strings")
                };
                let Value::String(a) = self.evaluate(&arguments[0]) else {
                    panic!("replace() expects strings")
                };
                let Value::String(b) = self.evaluate(&arguments[1]) else {
                    panic!("replace() expects strings")
                };
                Value::String(text.replace(&a, &b))
            }
            "split" => {
                if arguments.len() != 1 {
                    panic!("split() expects separator")
                };
                let Value::String(sep) = self.evaluate(&arguments[0]) else {
                    panic!("split() expects string")
                };
                Value::Array(
                    text.split(&sep)
                        .map(|s| Value::String(s.to_string()))
                        .collect(),
                )
            }
            "trim" => Value::String(text.trim().to_string()),
            "to_uppercase" => Value::String(text.to_uppercase()),
            "to_lowercase" => Value::String(text.to_lowercase()),
            "starts_with" => {
                let Value::String(q) = self.evaluate(&arguments[0]) else {
                    panic!("starts_with() expects string")
                };
                Value::Boolean(text.starts_with(&q))
            }
            "ends_with" => {
                let Value::String(q) = self.evaluate(&arguments[0]) else {
                    panic!("ends_with() expects string")
                };
                Value::Boolean(text.ends_with(&q))
            }
            "contains" => {
                let Value::String(q) = self.evaluate(&arguments[0]) else {
                    panic!("contains() expects string")
                };
                Value::Boolean(text.contains(&q))
            }
            "length" | "get_length" => Value::Number(text.chars().count() as i64),
            "iterate" => {
                Value::Iterator(text.chars().map(|c| Value::String(c.to_string())).collect())
            }
            _ => panic!("Unknown string method '{}'", method),
        }
    }

    // =========================================================================
    // Return type checking
    // =========================================================================

    fn check_return_type(
        &self,
        function_name: &str,
        return_type: &Option<String>,
        value: &Value,
        generic_parameters: &[String],
    ) {
        let Some(expected) = return_type else {
            return;
        };

        // Generic return types are compile-time information.
        if generic_parameters.iter().any(|name| name == expected) {
            return;
        }

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
                "Function '{}' return type error: expected {}, got {:?}",
                function_name, expected, value
            );
        }
    }

    // =========================================================================
    // Binary operations
    // =========================================================================

    fn evaluate_binary(&self, left: Value, operator: &Operator, right: Value) -> Value {
        match (left, right) {
            // -----------------------------------------------------------------
            // Numbers
            // -----------------------------------------------------------------
            (Value::Number(a), Value::Number(b)) => match operator {
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

                Operator::And | Operator::Or => {
                    panic!("Logical operators require boolean operands");
                }
            },

            // -----------------------------------------------------------------
            // Floats
            // -----------------------------------------------------------------
            (Value::Float(a), Value::Float(b)) => match operator {
                Operator::Plus => Value::Float(a + b),

                Operator::Minus => Value::Float(a - b),

                Operator::Multiply => Value::Float(a * b),

                Operator::Divide => {
                    if b == 0.0 {
                        panic!("Division by zero");
                    }

                    Value::Float(a / b)
                }

                Operator::Equal => Value::Boolean(a == b),

                Operator::NotEqual => Value::Boolean(a != b),

                Operator::Greater => Value::Boolean(a > b),

                Operator::Less => Value::Boolean(a < b),

                Operator::GreaterEqual => Value::Boolean(a >= b),

                Operator::LessEqual => Value::Boolean(a <= b),

                Operator::And | Operator::Or => {
                    panic!("Logical operators require boolean operands");
                }
            },

            // -----------------------------------------------------------------
            // Booleans
            // -----------------------------------------------------------------
            (Value::Boolean(a), Value::Boolean(b)) => match operator {
                Operator::And => Value::Boolean(a && b),

                Operator::Or => Value::Boolean(a || b),

                Operator::Equal => Value::Boolean(a == b),

                Operator::NotEqual => Value::Boolean(a != b),

                _ => {
                    panic!("Invalid boolean operation");
                }
            },

            // -----------------------------------------------------------------
            // Strings
            // -----------------------------------------------------------------
            (Value::String(a), Value::String(b)) => match operator {
                Operator::Plus => Value::String(format!("{}{}", a, b)),

                Operator::Equal => Value::Boolean(a == b),

                Operator::NotEqual => Value::Boolean(a != b),

                _ => {
                    panic!("Invalid string operation");
                }
            },

            // -----------------------------------------------------------------
            // Everything else
            // -----------------------------------------------------------------
            _ => {
                panic!("Invalid operation between incompatible values");
            }
        }
    }
}

static RNG_STATE: AtomicU64 = AtomicU64::new(0x9E3779B97F4A7C15);
fn next_random_u64() -> u64 {
    let mut x = RNG_STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, Ordering::Relaxed);
    x
}
fn random_float() -> f64 {
    (next_random_u64() as f64) / (u64::MAX as f64)
}
fn random_int(a: i64, b: i64) -> i64 {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    if lo == hi {
        return lo;
    }
    lo + (next_random_u64() % ((hi - lo + 1) as u64)) as i64
}
fn format_utc_date(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}
fn serialize_value(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),

        Value::Float(x) => x.to_string(),

        Value::Boolean(b) => b.to_string(),

        Value::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }

        Value::Array(a) => {
            format!(
                "[{}]",
                a.iter().map(serialize_value).collect::<Vec<_>>().join(", ")
            )
        }

        Value::HashMap(h) => {
            format!(
                "{{{}}}",
                h.iter()
                    .map(|(k, v)| { format!("{}:{}", serialize_value(k), serialize_value(v)) })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }

        Value::Struct { name, fields } => {
            format!(
                "{{\"__type\":\"{}\",{}}}",
                name,
                fields
                    .iter()
                    .map(|(k, v)| { format!("\"{}\":{}", k, serialize_value(v)) })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }

        Value::Enum {
            enum_name,
            variant,
            values,
        } => {
            format!(
                "{{\"__enum\":\"{}\",\"variant\":\"{}\",\"values\":[{}]}}",
                enum_name,
                variant,
                values
                    .iter()
                    .map(serialize_value)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }

        Value::Option(Some(x)) => {
            format!("{{\"Some\":{}}}", serialize_value(x))
        }

        Value::Option(None) => "null".into(),

        Value::Task(x) => serialize_value(x),

        Value::File(_) | Value::TcpStream(_) | Value::Iterator(_) | Value::None => v.to_string(),
    }
}
fn parse_serialized(s: &str) -> Option<Value> {
    let s = s.trim();
    if s == "null" {
        return Some(Value::Option(None));
    }
    if s == "true" {
        return Some(Value::Boolean(true));
    }
    if s == "false" {
        return Some(Value::Boolean(false));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Some(Value::Number(n));
    }
    if let Ok(f) = s.parse::<f64>() {
        if s.contains('.') {
            return Some(Value::Float(f));
        }
    }
    if s.starts_with('"') && s.ends_with('"') {
        return Some(Value::String(
            s[1..s.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\\\", "\\"),
        ));
    }
    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        if inner.trim().is_empty() {
            return Some(Value::Array(Vec::new()));
        }
        let parts = split_serialized(inner);
        let mut vals = Vec::new();
        for p in parts {
            vals.push(parse_serialized(&p)?);
        }
        return Some(Value::Array(vals));
    }
    None
}
fn split_serialized(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let mut quoted = false;
    let mut esc = false;
    for (i, c) in s.char_indices() {
        if quoted {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                quoted = false;
            }
        } else {
            match c {
                '"' => quoted = true,
                '[' | '{' => depth += 1,
                ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    out.push(s[start..i].trim().to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    out.push(s[start..].trim().to_string());
    out
}
fn hex_decode(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let a = data[i];
        let b = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let c = if i + 2 < data.len() { data[i + 2] } else { 0 };
        out.push(B64[(a >> 2) as usize] as char);
        out.push(B64[((a & 3) << 4 | b >> 4) as usize] as char);
        out.push(if i + 1 < data.len() {
            B64[((b & 15) << 2 | c >> 6) as usize] as char
        } else {
            '='
        });
        out.push(if i + 2 < data.len() {
            B64[(c & 63) as usize] as char
        } else {
            '='
        });
        i += 3;
    }
    out
}
fn base64_decode(s: &str) -> String {
    let mut vals = Vec::new();
    for ch in s.bytes() {
        if ch == b'=' {
            break;
        }
        if let Some(i) = B64.iter().position(|&x| x == ch) {
            vals.push(i as u8);
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < vals.len() {
        let a = vals[i];
        let b = vals[i + 1];
        out.push((a << 2) | (b >> 4));
        if i + 2 < vals.len() {
            let c = vals[i + 2];
            out.push((b << 4) | (c >> 2));
            if i + 3 < vals.len() {
                let d = vals[i + 3];
                out.push((c << 6) | d);
            }
        }
        i += 4;
    }
    String::from_utf8_lossy(&out).into_owned()
}
