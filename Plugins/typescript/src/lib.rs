//! Fusion TypeScript interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.typescript";

pub const LANGUAGE: &str = "TypeScript";

pub fn validate_target(_target: &str) -> Result<(), &'static str> {
    Err("TypeScript plugin scaffold: target validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_correct() {
        assert_eq!(PLUGIN_ID, "language.typescript");
        assert_eq!(LANGUAGE, "TypeScript");
    }
}
