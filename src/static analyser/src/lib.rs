//! Static analysis foundation for the Fusion language.

pub mod analyzer;
pub mod config;
pub mod diagnostic;
pub mod rule;
pub mod rules;
pub mod source;

pub use analyzer::Analyzer;
pub use config::AnalyzerConfig;
pub use diagnostic::{Diagnostic, Severity, SourceSpan};
pub use source::SourceFile;

#[cfg(test)]
mod tests;
