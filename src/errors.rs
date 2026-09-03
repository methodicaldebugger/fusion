//contents of errors.rs
use crate::span::Span;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.message, self.span.start)
    }
}

impl std::error::Error for ParseError {}

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

            FusionError::Syntax { message, span } => {
    write!(f, "{} at {}", message, span.start)
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