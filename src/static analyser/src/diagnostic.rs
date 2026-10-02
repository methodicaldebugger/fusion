use std::fmt;

/// Byte-based source range. End is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub rule_id: &'static str,
    pub severity: Severity,
    pub message: String,
    pub span: SourceSpan,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn new(
        rule_id: &'static str,
        severity: Severity,
        message: impl Into<String>,
        span: SourceSpan,
    ) -> Self {
        Self {
            rule_id,
            severity,
            message: message.into(),
            span,
            help: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}[{}] at {}..{}: {}",
            self.severity, self.rule_id, self.span.start, self.span.end, self.message
        )?;
        if let Some(help) = &self.help {
            write!(f, "\n  help: {help}")?;
        }
        Ok(())
    }
}
