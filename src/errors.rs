//contents of errors.rs
use crate::span::Span;

#[derive(Debug, Clone)]
pub enum FusionError {
    Lexer {
        message: String,
        span: Span,
    },

    Syntax {
        message: String,
        span: Span,
    },

    UnknownVariable {
        name: String,
        span: Span,
    },

    TypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },

    CannotAssignToConst {
        name: String,
        span: Span,
    },

    InvalidOperation {
        left: String,
        operator: String,
        right: String,
        span: Span,
    },
}

impl std::fmt::Display for FusionError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter,
    ) -> std::fmt::Result {
        match self {
            FusionError::Lexer { message, .. } => {
                write!(f, "{}", message)
            }

            FusionError::Syntax { message, .. } => {
                write!(f, "{}", message)
            }

            FusionError::UnknownVariable { name, .. } => {
                write!(f, "Unknown variable '{}'", name)
            }

            FusionError::TypeMismatch {
                expected,
                found,
                ..
            } => {
                write!(
                    f,
                    "Type mismatch: expected {}, found {}",
                    expected,
                    found
                )
            }

            FusionError::CannotAssignToConst { name, .. } => {
                write!(
                    f,
                    "Cannot assign to constant '{}'",
                    name
                )
            }

            FusionError::InvalidOperation {
                left,
                operator,
                right,
                ..
            } => {
                write!(
                    f,
                    "Invalid operation: {} {} {}",
                    left,
                    operator,
                    right
                )
            }
        }
    }
}

impl std::error::Error for FusionError {}