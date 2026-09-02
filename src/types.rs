//contents of types.rs
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Num,
    Float,
    Bool,
    String,
    Array(Box<Type>),
    Struct(String),
    Enum(String),
    Generic(String),
    Unknown,
    Void,
}

#[derive(Debug, Clone)]
pub struct StructDefinition {
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub variants: HashMap<String, EnumVariantDefinition>,
}

#[derive(Debug, Clone)]
pub struct EnumVariantDefinition {
    pub fields: Vec<Type>,
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Num => "num".into(),
            Type::Float => "float".into(),
            Type::Bool => "bool".into(),
            Type::String => "string".into(),
            Type::Array(inner) => format!("{}[]", inner.name()),
            Type::Struct(name) => name.clone(),
            Type::Enum(name) => name.clone(),
            Type::Generic(name) => name.clone(),
            Type::Void => "void".into(),
            Type::Unknown => "unknown".into(),
        }
    }
}