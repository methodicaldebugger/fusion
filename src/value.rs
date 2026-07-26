#[derive(Debug, Clone)]
pub enum Value {

    Integer(i64),

    Float(f64),

    String(String),

    Boolean(bool),

    None,
}