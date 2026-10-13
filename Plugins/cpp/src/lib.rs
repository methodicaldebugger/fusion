//! Fusion C++ interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.cpp";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    GeneratedAdapter,
    NativeAbiWrapper,
}

pub struct IntegrationDescriptor {
    pub id: &'static str,
    pub language: &'static str,
    pub requires_toolchain: bool,
}

pub const DESCRIPTOR: IntegrationDescriptor = IntegrationDescriptor {
    id: PLUGIN_ID,
    language: "C++",
    requires_toolchain: true,
};

pub fn validate_target(_target: &str) -> Result<(), &'static str> {
    Err("C++ plugin scaffold: target validation is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_has_expected_identity() {
        assert_eq!(DESCRIPTOR.id, PLUGIN_ID);
        assert_eq!(DESCRIPTOR.language, "C++");
    }
}