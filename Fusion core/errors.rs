use crate::span::Span;
use std::fmt;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Help,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Help => write!(f, "help"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticLabel {
    pub span: Span,
    pub message: String,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub message: String,
    pub replacement: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<DiagnosticLabel>,
    pub suggestions: Vec<Suggestion>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            labels: Vec::new(),
            suggestions: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn with_label(mut self, span: Span, message: impl Into<String>, is_primary: bool) -> Self {
        self.labels.push(DiagnosticLabel {
            span,
            message: message.into(),
            is_primary,
        });
        self
    }

    pub fn with_suggestion(mut self, message: impl Into<String>, replacement: impl Into<String>, span: Span) -> Self {
        self.suggestions.push(Suggestion {
            message: message.into(),
            replacement: replacement.into(),
            span,
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn emit(&self, source_code: &str) -> String {
        let lines: Vec<&str> = source_code.lines().collect();
        let mut output = String::new();

        output.push_str(&format!("{}[{}]: {}
", self.severity, self.code, self.message));

        if let Some(primary) = self.labels.iter().find(|l| l.is_primary) {
    let mut line_num: usize = 1;
    let mut col_num: usize = 1;
    for (idx, ch) in source_code.char_indices() {
        if idx >= primary.span.start {
            break;
        }
        if ch == '\n' {
            line_num += 1;
            col_num = 1;
        } else {
            col_num += 1;
        }
    }
            let line_idx = line_num - 1;
            let line_str = lines.get(line_idx).copied().unwrap_or("");
            
            output.push_str(&format!("  --> line {}, col {}
", line_num, col_num));
            output.push_str(" |
");
            output.push_str(&format!("{:>3} | {}
", line_num, line_str));
            
            let indent = " ".repeat(col_num.saturating_sub(1));
            let len = std::cmp::max(1, primary.span.end.saturating_sub(primary.span.start));
            let carets = "^".repeat(len);
            output.push_str(&format!("    | {}{}", indent, carets));
            if !primary.message.is_empty() {
                output.push_str(&format!(" {}", primary.message));
            }
            output.push('\n');
            output.push_str(" |
");
        }

        for suggestion in &self.suggestions {
            output.push_str(&format!("  = help: {}; consider replacing with `{}`
", suggestion.message, suggestion.replacement));
        }

        for note in &self.notes {
            output.push_str(&format!("  = note: {}
", note));
        }

        output
    }
}

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
        suggestion: Option<String>,
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

impl FusionError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            FusionError::Lexer { message, span } => {
                Diagnostic::error("E0001", message.clone())
                    .with_label(*span, "invalid token", true)
            }
            FusionError::Syntax { message, span } => {
                Diagnostic::error("E0002", message.clone())
                    .with_label(*span, "syntax error here", true)
            }
            FusionError::UnknownVariable { name, suggestion, span } => {
                let mut diag = Diagnostic::error("E0425", format!("cannot find value `{}` in this scope", name))
                    .with_label(*span, "not found in scope", true);
                if let Some(sugg) = suggestion {
                    diag = diag.with_suggestion(format!("did you mean `{}`?", sugg), sugg.clone(), *span);
                }
                diag
            }
            FusionError::TypeMismatch { expected, found, span } => {
                Diagnostic::error("E0308", "mismatched types")
                    .with_label(*span, format!("expected `{}`, found `{}`", expected, found), true)
            }
            FusionError::CannotAssignToConst { name, span } => {
                Diagnostic::error("E0384", format!("cannot assign to immutable/constant variable `{}`", name))
                    .with_label(*span, "cannot assign", true)
            }
            FusionError::InvalidOperation { left, operator, right, span } => {
                Diagnostic::error("E0369", format!("cannot operate `{}` on `{}` and `{}`", operator, left, right))
                    .with_label(*span, "invalid operation", true)
            }
        }
    }
}

impl std::fmt::Display for FusionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FusionError::Lexer { message, .. } => {
                write!(f, "{}", message)
            }

            FusionError::Syntax { message, span } => {
                write!(f, "{} at {}", message, span.start)
            }

            FusionError::UnknownVariable { name, suggestion, .. } => {
                if let Some(sugg) = suggestion {
                    write!(f, "Unknown variable '{}'. Did you mean '{}'?", name, sugg)
                } else {
                    write!(f, "Unknown variable '{}'", name)
                }
            }

            FusionError::TypeMismatch {
                expected, found, ..
            } => {
                write!(f, "Type mismatch: expected {}, found {}", expected, found)
            }

            FusionError::CannotAssignToConst { name, .. } => {
                write!(f, "Cannot assign to constant '{}'", name)
            }

            FusionError::InvalidOperation {
                left,
                operator,
                right,
                ..
            } => {
                write!(f, "Invalid operation: {} {} {}", left, operator, right)
            }
        }
    }
}

impl std::error::Error for FusionError {}
