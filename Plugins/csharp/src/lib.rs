//! Fusion C# / .NET interoperability plugin scaffold.

pub const PLUGIN_ID: &str = "language.csharp";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    ManagedRuntime,
    GeneratedAdapter,
}

pub struct IntegrationDescriptor {
    pub id: &'static str,
    pub language: &'static str,
    pub boundary: Boundary,
    pub requires_runtime: bool,
}

pub const DESCRIPTOR: IntegrationDescriptor = IntegrationDescriptor {
    id: PLUGIN_ID,
    language: "C#",
    boundary: Boundary::ManagedRuntime,
    requires_runtime: true,
};

pub fn validate_runtime() -> Result<(), &'static str> {
    Err(".NET runtime discovery is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_managed() {
        assert_eq!(DESCRIPTOR.boundary, Boundary::ManagedRuntime);
        assert!(DESCRIPTOR.requires_runtime);
    }
}