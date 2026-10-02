/// Configuration shared by analyser rules.
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    pub enabled: bool,
    pub warn_on_large_functions: bool,
    pub large_function_line_threshold: usize,
    pub warn_on_simple_unused_bindings: bool,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            warn_on_large_functions: true,
            large_function_line_threshold: 80,
            warn_on_simple_unused_bindings: true,
        }
    }
}
