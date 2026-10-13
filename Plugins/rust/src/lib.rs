//! Fusion Rust interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.rust";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    GeneratedAdapter,
    NativeAbi,
}

pub const LANGUAGE: &str = "Rust";

pub fn validate_target(_target: &str) -> Result<(), &'static str> {
    Err("Rust plugin scaffold: target validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_identity_is_stable() {
        assert_eq!(PLUGIN_ID, "language.rust");
        assert_eq!(LANGUAGE, "Rust");
    }
}