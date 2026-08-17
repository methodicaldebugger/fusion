
//contents of value.rs
#[derive(Debug, Clone)]
pub enum Value {
    Number(i64),Float(f64),String(String),Boolean(bool),
    Array(Vec<Value>),None,
}
impl std::fmt::Display for Value {
    fn fmt(&self,f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Value::Number(v) =>write!(f, "{}", v),
            Value::Float(v) =>write!(f, "{}", v),
            Value::String(v) =>write!(f, "{}", v),
            Value::Boolean(v) =>write!(f, "{}", v),
            Value::Array(values) =>write!(f, "{:?}", values),
            Value::None =>write!(f, "none"),
        }
    }
}