use crate::{
    config::AnalyzerConfig,
    diagnostic::Diagnostic,
    rule::AnalysisRule,
    rules::{LargeFunctionRule, SimpleUnusedBindingRule},
    source::SourceFile,
};

/// Runs enabled static analysis rules over Fusion source.
pub struct Analyzer {
    pub config: AnalyzerConfig,
    rules: Vec<Box<dyn AnalysisRule>>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new(AnalyzerConfig::default())
    }
}

impl Analyzer {
    pub fn new(config: AnalyzerConfig) -> Self {
        let mut analyzer = Self {
            config: config.clone(),
            rules: Vec::new(),
        };

        if config.enabled {
            if config.warn_on_large_functions {
                analyzer.register(LargeFunctionRule {
                    line_threshold: config.large_function_line_threshold,
                });
            }
            if config.warn_on_simple_unused_bindings {
                analyzer.register(SimpleUnusedBindingRule);
            }
        }
        analyzer
    }

    pub fn register<R: AnalysisRule + 'static>(&mut self, rule: R) {
        self.rules.push(Box::new(rule));
    }

    pub fn analyze(&self, source: &SourceFile) -> Vec<Diagnostic> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut diagnostics = self
            .rules
            .iter()
            .flat_map(|rule| rule.analyze(source))
            .collect::<Vec<_>>();

        diagnostics.sort_by_key(|d| (d.span.start, d.span.end, d.rule_id));
        diagnostics
    }
}
