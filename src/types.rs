//contents of types.rs
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Num,Float,Bool,
    String,Unknown,Void,Array(Box<Type>)
}
impl Type {
    pub fn name(&self)->String {
        match self {
            Type::Num =>"num".into(),
            Type::Array(inner)=>
            format!("{}[]",inner.name()),
            Type::Void => "void".into(),
            Type::Float =>"float".into(),
            Type::Bool =>"bool".into(),
            Type::String =>"string".into(),
            Type::Unknown =>"unknown".into(),
        }
    }
}