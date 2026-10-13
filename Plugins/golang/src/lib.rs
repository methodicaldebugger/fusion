//! Fusion Go interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.golang";

pub const LANGUAGE: &str = "Go";

pub fn validate_target(_target: &str) -> Result<(), &'static str> {
    Err("Go plugin scaffold: target validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_correct() {
        assert_eq!(PLUGIN_ID, "language.golang");
    }
}