//! Fusion Kotlin interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.kotlin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Jvm,
    Native,
}

pub const LANGUAGE: &str = "Kotlin";

pub fn validate_backend(_backend: Backend) -> Result<(), &'static str> {
    Err("Kotlin backend validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_correct() {
        assert_eq!(PLUGIN_ID, "language.kotlin");
        assert_eq!(LANGUAGE, "Kotlin");
    }
}