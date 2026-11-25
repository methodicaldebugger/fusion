//contents of value.rs
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File as StdFile;
use std::net::TcpStream;
use std::rc::Rc;

use crate::wide_int::WideInt;

#[derive(Debug, Clone)]
pub enum Value {
    Number(i64),
    /// Integer outside the `i64` range (for example a `u64` above `i64::MAX`, or a
    /// 128-bit value). Canonical form: values that fit `i64` are always `Number`.
    Wide(WideInt),
    Float(f64),
    String(String),
    Boolean(bool),
    Array(Vec<Value>),
    Tuple(Vec<Value>),
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
    TcpStream(Rc<RefCell<Option<TcpStream>>>),
    Task(Box<Value>),

    /// A function value: the (possibly internal) symbol of a user function.
    Function(String),

    None,
}

impl Value {
    /// Canonical integer constructor: values that fit `i64` become `Number`,
    /// everything else becomes `Wide`. Always use this when building an integer
    /// from a `WideInt` so equal integers have equal representations.
    pub fn from_wide(value: WideInt) -> Value {
        match value.to_i64() {
            Some(number) => Value::Number(number),
            None => Value::Wide(value),
        }
    }

    /// Returns the exact integer value of `Number` and `Wide`, `None` otherwise.
    pub fn as_wide(&self) -> Option<WideInt> {
        match self {
            Value::Number(number) => Some(WideInt::from_i64(*number)),
            Value::Wide(wide) => Some(*wide),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(v) => write!(f, "{}", v),

            Value::Wide(v) => write!(f, "{}", v),

            Value::Float(v) => write!(f, "{}", v),

            Value::String(v) => write!(f, "{}", v),

            Value::Boolean(v) => write!(f, "{}", v),

            Value::Tuple(values) => {
                write!(f, "(")?;
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", value)?;
                }
                if values.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }

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
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, value)?;
                }
                write!(f, "}}")
            }

            Value::Iterator(values) => {
                write!(f, "Iterator[")?;
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

            Value::Option(Some(value)) => write!(f, "Some({})", value),
            Value::Option(None) => write!(f, "None"),

            Value::File(_) => write!(f, "<file>"),
            Value::TcpStream(_) => write!(f, "<tcp-stream>"),
            Value::Task(value) => write!(f, "Task({})", value),

            Value::Function(symbol) => {
                write!(f, "<function {}>", crate::function_values::display_name(symbol))
            }

            Value::None => write!(f, "none"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Wide(a), Value::Wide(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            (Value::HashMap(a), Value::HashMap(b)) => a == b,
            (Value::Iterator(a), Value::Iterator(b)) => a == b,
            (
                Value::Struct {
                    name: an,
                    fields: af,
                },
                Value::Struct {
                    name: bn,
                    fields: bf,
                },
            ) => an == bn && af == bf,
            (
                Value::Enum {
                    enum_name: ae,
                    variant: av,
                    values: ax,
                },
                Value::Enum {
                    enum_name: be,
                    variant: bv,
                    values: bx,
                },
            ) => ae == be && av == bv && ax == bx,
            (Value::Option(a), Value::Option(b)) => a == b,
            (Value::None, Value::None) => true,
            (Value::File(a), Value::File(b)) => Rc::ptr_eq(a, b),
            (Value::TcpStream(a), Value::TcpStream(b)) => Rc::ptr_eq(a, b),
            (Value::Task(a), Value::Task(b)) => a == b,
            _ => false,
        }
    }
}
