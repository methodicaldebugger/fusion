use crate::{
    diagnostic::{Diagnostic, Severity, SourceSpan},
    rule::AnalysisRule,
    source::SourceFile,
};

/// Conservative textual heuristic for plainly named `let` bindings.
///
/// This is not a substitute for name resolution: shadowing, comments, strings,
/// destructuring and language-specific declaration forms need AST/scope analysis.
pub struct SimpleUnusedBindingRule;

impl AnalysisRule for SimpleUnusedBindingRule {
    fn id(&self) -> &'static str {
        "FUS1002"
    }

    fn analyze(&self, source: &SourceFile) -> Vec<Diagnostic> {
        let mut results = Vec::new();

        for (line_start, line) in line_starts(&source.text) {
            let trimmed = line.trim_start();
            let Some(after_let) = trimmed.strip_prefix("let ") else {
                continue;
            };

            let name_end = after_let
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .unwrap_or(after_let.len());
            let name = &after_let[..name_end];

            if name.is_empty() || name.starts_with('_') {
                continue;
            }

            let name_offset = line.find(name).map(|i| line_start + i).unwrap_or(line_start);
            let occurrences = source.text.match_indices(name).count();
            if occurrences == 1 {
                results.push(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("binding `{name}` appears to be unused"),
                        SourceSpan::new(name_offset, name_offset + name.len()),
                    )
                    .with_help("Remove the binding or prefix its name with `_` if intentional."),
                );
            }
        }

        results
    }
}

fn line_starts(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_inclusive('\n').scan(0usize, |offset, line| {
        let start = *offset;
        *offset += line.len();
        Some((start, line.trim_end_matches('\n')))
    })
}
