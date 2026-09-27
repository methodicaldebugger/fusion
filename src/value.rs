//contents of value.rs
use std::collections::HashMap;
use std::cell::RefCell;
use std::fs::File as StdFile;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Value {
    Number(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Array(Vec<Value>),
    HashMap(Vec<(Value, Value)>),
    Iterator(Vec<Value>),

    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },

    Enum {
        enum_name: String,
        variant: String,
        values: Vec<Value>,
    },

    Option(Option<Box<Value>>),

    File(Rc<RefCell<Option<StdFile>>>),

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

            Value::HashMap(entries) => {
                write!(f, "{{")?;
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", key, value)?;
                }
                write!(f, "}}")
            }

            Value::Iterator(values) => {
                write!(f, "Iterator[")?;
                for (i, value) in values.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
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

            Value::Option(Some(value)) => write!(f, "Some({})", value),
            Value::Option(None) => write!(f, "None"),

            Value::File(_) => write!(f, "<file>"),

            Value::None => write!(f, "none"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::HashMap(a), Value::HashMap(b)) => a == b,
            (Value::Iterator(a), Value::Iterator(b)) => a == b,
            (Value::Struct { name: an, fields: af }, Value::Struct { name: bn, fields: bf }) => an == bn && af == bf,
            (Value::Enum { enum_name: ae, variant: av, values: ax }, Value::Enum { enum_name: be, variant: bv, values: bx }) => ae == be && av == bv && ax == bx,
            (Value::Option(a), Value::Option(b)) => a == b,
            (Value::None, Value::None) => true,
            (Value::File(a), Value::File(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}
