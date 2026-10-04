//! Fusion Python interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.python";

pub const LANGUAGE: &str = "Python";

pub fn validate_target(_target: &str) -> Result<(), &'static str> {
    Err("Python plugin scaffold: target validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_correct() {
        assert_eq!(PLUGIN_ID, "language.python");
        assert_eq!(LANGUAGE, "Python");
    }
}
