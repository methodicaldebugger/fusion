//! Fusion C interoperability plugin.
//!
//! This is currently an architectural scaffold.
//! It does not invoke a C compiler or generate bindings yet.

pub const PLUGIN_ID: &str = "language.c";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    NativeAbi,
}

#[derive(Debug, Clone, Copy)]
pub struct IntegrationDescriptor {
    pub id: &'static str,
    pub language: &'static str,
    pub boundary: Boundary,
    pub requires_toolchain: bool,
    pub requires_runtime: bool,
}

pub const DESCRIPTOR: IntegrationDescriptor = IntegrationDescriptor {
    id: PLUGIN_ID,
    language: "C",
    boundary: Boundary::NativeAbi,
    requires_toolchain: true,
    requires_runtime: false,
};

pub fn validate_target(_target: &str) -> Result<(), &'static str> {
    Err("C plugin scaffold: target validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_correct() {
        assert_eq!(DESCRIPTOR.id, PLUGIN_ID);
        assert_eq!(DESCRIPTOR.language, "C");
        assert!(!DESCRIPTOR.requires_runtime);
    }
}