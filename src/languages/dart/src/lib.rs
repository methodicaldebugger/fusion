//! Fusion Dart interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.dart";

pub const LANGUAGE: &str = "Dart";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Jit,
    Aot,
    Standalone,
}

pub fn validate_runtime() -> Result<(), &'static str> {
    Err("Dart runtime discovery is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_correct() {
        assert_eq!(PLUGIN_ID, "language.dart");
    }
}