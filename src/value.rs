//contents of value.rs
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Array(Vec<Value>),

    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },

    Enum {
        enum_name: String,
        variant: String,
        values: Vec<Value>,
    },

    None,
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(v) => write!(f, "{}", v),

            Value::Float(v) => write!(f, "{}", v),

            Value::String(v) => write!(f, "{}", v),

            Value::Boolean(v) => write!(f, "{}", v),

            Value::Array(values) => {
                write!(f, "[")?;

                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }

                    write!(f, "{}", value)?;
                }

                write!(f, "]")
            }

            Value::Struct { name, fields } => {
                write!(f, "{} {{ ", name)?;

                let mut entries: Vec<_> = fields.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));

                for (i, (field, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }

                    write!(f, "{}: {}", field, value)?;
                }

                write!(f, " }}")
            }

            Value::Enum {
                enum_name,
                variant,
                values,
            } => {
                write!(f, "{}::{}", enum_name, variant)?;

                if !values.is_empty() {
                    write!(f, "(")?;

                    for (i, value) in values.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }

                        write!(f, "{}", value)?;
                    }

                    write!(f, ")")?;
                }

                Ok(())
            }

            Value::None => write!(f, "none"),
        }
    }
}
