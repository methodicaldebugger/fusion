//! Fusion Zig interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.zig";

pub const LANGUAGE: &str = "Zig";

pub fn validate_target(_target: &str) -> Result<(), &'static str> {
    Err("Zig plugin scaffold: target validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_correct() {
        assert_eq!(PLUGIN_ID, "language.zig");
    }
}