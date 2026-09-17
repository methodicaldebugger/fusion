use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Num,
    Float,
    Bool,
    String,
    Array(Box<Type>),
    Iterator(Box<Type>),
    HashMap(Box<Type>, Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Struct(String),
    Enum(String),
    Generic(String),
    File,
    Task(Box<Type>),
    Void,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct StructDefinition { pub fields: Vec<(String, Type)> }
#[derive(Debug, Clone)]
pub struct EnumDefinition { pub variants: HashMap<String, EnumVariantDefinition> }
#[derive(Debug, Clone)]
pub struct EnumVariantDefinition { pub fields: Vec<Type> }

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Num => "num".into(), Type::Float => "float".into(), Type::Bool => "bool".into(),
            Type::String => "string".into(), Type::Array(i) => format!("{}[]", i.name()),
            Type::Iterator(i) => format!("Iterator<{}>", i.name()),
            Type::HashMap(k,v) => format!("HashMap<{}, {}>", k.name(), v.name()),
            Type::Option(i) => format!("Option<{}>", i.name()),
            Type::Result(o,e) => format!("Result<{}, {}>", o.name(), e.name()),
            Type::Struct(n) => n.clone(), Type::Enum(n) => n.clone(), Type::Generic(n) => n.clone(),
            Type::File => "File".into(), Type::Task(i) => format!("Task<{}>", i.name()),
            Type::Void => "void".into(), Type::Unknown => "unknown".into(),
        }
    }
}
