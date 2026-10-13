//! Fusion Java/JVM interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.java";

pub const LANGUAGE: &str = "Java";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    Jvm,
    GeneratedAdapter,
}

pub fn validate_runtime() -> Result<(), &'static str> {
    Err("JVM discovery is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_correct() {
        assert_eq!(PLUGIN_ID, "language.java");
        assert_eq!(LANGUAGE, "Java");
    }
}