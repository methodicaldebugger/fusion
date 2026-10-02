use crate::{diagnostic::Diagnostic, source::SourceFile};

/// A static analysis rule. Rules should report findings, not mutate source code.
pub trait AnalysisRule: Send + Sync {
    fn id(&self) -> &'static str;
    fn analyze(&self, source: &SourceFile) -> Vec<Diagnostic>;
}
