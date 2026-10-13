use crate::{
    diagnostic::{Diagnostic, Severity, SourceSpan},
    rule::AnalysisRule,
    source::SourceFile,
};

/// Heuristic rule that flags function-like blocks exceeding a line threshold.
///
/// This intentionally uses a lightweight brace scan. Replace it with AST-based
/// function spans when Fusion's parser exposes a stable library interface.
pub struct LargeFunctionRule {
    pub line_threshold: usize,
}

impl AnalysisRule for LargeFunctionRule {
    fn id(&self) -> &'static str {
        "FUS1001"
    }

    fn analyze(&self, source: &SourceFile) -> Vec<Diagnostic> {
        let mut results = Vec::new();
        let mut search_from = 0;

        while let Some(relative_fn) = source.text[search_from..].find("fn ") {
            let start = search_from + relative_fn;
            let open = match source.text[start..].find('{') {
                Some(offset) => start + offset,
                None => break,
            };

            let mut depth = 0usize;
            let mut end = None;
            for (offset, ch) in source.text[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = Some(open + offset + ch.len_utf8());
                            break;
                        }
                    }
                    _ => {}
                }
            }

            let Some(end) = end else { break };
            let block = &source.text[start..end];
            let lines = block.lines().count();
            if lines > self.line_threshold {
                results.push(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!(
                            "function-like block spans {lines} lines (threshold: {})",
                            self.line_threshold
                        ),
                        SourceSpan::new(start, end),
                    )
                    .with_help("Consider splitting this function into smaller units."),
                );
            }
            search_from = end;
        }

        results
    }
}
