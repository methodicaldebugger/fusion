#[cfg(test)]
mod tests {
    use crate::{Analyzer, SourceFile};

    #[test]
    fn reports_simple_unused_binding() {
        let source = SourceFile::new("test.fus", "fn main() {\n let unused = 1;\n}\n");
        let findings = Analyzer::default().analyze(&source);
        assert!(findings.iter().any(|d| d.rule_id == "FUS1002"));
    }

    #[test]
    fn does_not_report_used_binding() {
        let source = SourceFile::new("test.fus", "fn main() {\n let count = 1;\n print(count);\n}\n");
        let findings = Analyzer::default().analyze(&source);
        assert!(!findings.iter().any(|d| d.rule_id == "FUS1002"));
    }
}
