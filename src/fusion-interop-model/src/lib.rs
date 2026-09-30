//! Fusion interoperability model.
//!
//! This crate defines the language/toolchain-neutral vocabulary for crossing
//! the Fusion/foreign boundary. It deliberately contains no compiler process
//! management. Foundry resolves *what* a project needs; the toolchain
//! orchestrator decides *how/with which tools* to build it.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntegrationKind {
    NativeAbi,
    ManagedRuntime,
    EmbeddedRuntime,
    Process,
    WebAssembly,
}

impl IntegrationKind {
    pub const ALL: [Self; 5] = [
        Self::NativeAbi,
        Self::ManagedRuntime,
        Self::EmbeddedRuntime,
        Self::Process,
        Self::WebAssembly,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::NativeAbi => "native-abi",
            Self::ManagedRuntime => "managed-runtime",
            Self::EmbeddedRuntime => "embedded-runtime",
            Self::Process => "process",
            Self::WebAssembly => "wasm",
        }
    }

    pub fn is_in_process(self) -> bool {
        matches!(self, Self::NativeAbi | Self::ManagedRuntime | Self::EmbeddedRuntime | Self::WebAssembly)
    }
}

impl fmt::Display for IntegrationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForeignLanguage {
    C,
    Cpp,
    Rust,
    Zig,
    Go,
    Swift,
    Java,
    Kotlin,
    CSharp,
    Python,
    Other,
}

impl ForeignLanguage {
    pub fn name(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Rust => "rust",
            Self::Zig => "zig",
            Self::Go => "go",
            Self::Swift => "swift",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::CSharp => "csharp",
            Self::Python => "python",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    StaticLibrary,
    SharedLibrary,
    Object,
    Executable,
    WasmModule,
    ForeignPackage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignSource {
    pub language: ForeignLanguage,
    pub path: PathBuf,
}

impl ForeignSource {
    pub fn new(language: ForeignLanguage, path: impl Into<PathBuf>) -> Self {
        Self { language, path: path.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InteropRequirements {
    pub target: Option<String>,
    pub sysroot: Option<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub library_dirs: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub defines: Vec<String>,
    pub extra_args: Vec<String>,
}

impl InteropRequirements {
    pub fn for_target(target: impl Into<String>) -> Self {
        Self { target: Some(target.into()), ..Self::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteropSpec {
    pub name: String,
    pub kind: IntegrationKind,
    pub sources: Vec<ForeignSource>,
    pub artifacts: Vec<ArtifactKind>,
    pub requirements: InteropRequirements,
}

impl InteropSpec {
    pub fn new(name: impl Into<String>, kind: IntegrationKind) -> Self {
        Self {
            name: name.into(),
            kind,
            sources: Vec::new(),
            artifacts: Vec::new(),
            requirements: InteropRequirements::default(),
        }
    }

    pub fn validate(&self) -> Result<(), InteropError> {
        if self.name.trim().is_empty() {
            return Err(InteropError::Invalid("integration name cannot be empty".into()));
        }
        if self.sources.is_empty() && self.kind != IntegrationKind::Process {
            return Err(InteropError::Invalid(format!(
                "integration '{}' has no foreign sources",
                self.name
            )));
        }
        if self.kind == IntegrationKind::Process
            && self.artifacts.iter().any(|a| *a != ArtifactKind::Executable)
        {
            return Err(InteropError::Invalid(
                "process integrations may only declare executable artifacts".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteropError {
    Invalid(String),
}

impl fmt::Display for InteropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for InteropError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_bootstrap_levels_are_stable() {
        assert_eq!(IntegrationKind::ALL.len(), 5);
        assert_eq!(IntegrationKind::NativeAbi.name(), "native-abi");
        assert_eq!(IntegrationKind::WebAssembly.name(), "wasm");
    }

    #[test]
    fn c_native_abi_spec_validates() {
        let mut spec = InteropSpec::new("physics", IntegrationKind::NativeAbi);
        spec.sources.push(ForeignSource::new(ForeignLanguage::C, "foreign/physics.c"));
        spec.artifacts.push(ArtifactKind::StaticLibrary);
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn empty_non_process_spec_is_rejected() {
        let spec = InteropSpec::new("broken", IntegrationKind::NativeAbi);
        assert!(spec.validate().is_err());
    }
}
