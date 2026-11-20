//! Project-level foreign dependency model.
//!
//! Fusion deliberately keeps foreign source out of `.fusion` files. This module
//! is the first bootstrap implementation of that boundary for C. The parser
//! does not need to understand C; the build system owns these files.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Explicit ABI contract for a Phase 4 foreign declaration. This model is
/// intentionally independent of Fusion's general type system until lowering is
/// implemented in the next milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForeignAbiType {
    Void,
    I8, U8, I16, U16, I32, U32, I64, U64,
    F32, F64,
    /// A raw pointer whose pointee is not interpreted by Fusion.
    OpaquePointer,
}

impl ForeignAbiType {
    /// Map the Phase 4 C declaration spelling to the supported ABI subset.
    pub fn from_type_name(name: &str) -> Option<Self> {
        Some(match name {
            "void" => Self::Void,
            "i8" => Self::I8,
            "u8" => Self::U8,
            "i16" => Self::I16,
            "u16" => Self::U16,
            "i32" => Self::I32,
            "u32" => Self::U32,
            "i64" => Self::I64,
            "u64" => Self::U64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            "*opaque" => Self::OpaquePointer,
            _ => return None,
        })
    }

    /// Whether this type is allowed as a parameter in the initial C ABI subset.
    /// `void` is deliberately excluded: C functions cannot accept a void value.
    pub fn is_parameter_supported(&self) -> bool {
        matches!(
            self,
            Self::I8 | Self::U8 | Self::I16 | Self::U16
                | Self::I32 | Self::U32 | Self::I64 | Self::U64
                | Self::F32 | Self::F64 | Self::OpaquePointer
        )
    }

    /// Whether this type is allowed as a return type in the initial C ABI subset.
    /// All supported scalar/pointer types and `void` are valid return types.
    pub fn is_return_supported(&self) -> bool {
        matches!(
            self,
            Self::Void | Self::I8 | Self::U8 | Self::I16 | Self::U16
                | Self::I32 | Self::U32 | Self::I64 | Self::U64
                | Self::F32 | Self::F64 | Self::OpaquePointer
        )
    }
}

/// Language-level declaration metadata. Phase 4.2 defines and validates the
/// declaration shape; invocation lowering is intentionally not implemented yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignFunctionDecl {
    pub source_name: String,
    pub link_name: String,
    pub parameters: Vec<ForeignAbiType>,
    pub return_type: ForeignAbiType,
    pub calling_convention: ForeignCallingConvention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignCallingConvention {
    /// The platform's C ABI (the only supported convention in Phase 4).
    C,
}

impl ForeignFunctionDecl {
    pub fn validate(&self) -> Result<(), String> {
        if self.source_name.trim().is_empty() {
            return Err("foreign function name cannot be empty".into());
        }
        if self.link_name.trim().is_empty() {
            return Err("foreign symbol name cannot be empty".into());
        }
        if let Some(index) = self.parameters.iter().position(|p| !p.is_parameter_supported()) {
            return Err(format!("foreign parameter {} uses an unsupported ABI type", index + 1));
        }
        if !self.return_type.is_return_supported() {
            return Err("foreign function uses an unsupported ABI return type".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum ForeignKind {
    C,
    Cpp,
    Rust,
    Zig,
    Wasm,
    Process,
}

#[derive(Debug, Clone)]
pub struct ForeignLibrary {
    pub name: String,
    pub kind: ForeignKind,
    pub sources: Vec<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub library_dirs: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub extra_args: Vec<String>,
}

impl ForeignLibrary {
    pub fn c(name: impl Into<String>, sources: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            name: name.into(),
            kind: ForeignKind::C,
            sources: sources.into_iter().collect(),
            include_dirs: Vec::new(),
            library_dirs: Vec::new(),
            libraries: Vec::new(),
            extra_args: Vec::new(),
        }
    }

    /// Compile the foreign C sources into one object file.
    ///
    /// This intentionally uses clang's C ABI rather than embedding a C
    /// compiler in Fusion. Later backends can replace this implementation
    /// with a cached artifact builder without changing the language boundary.
    pub fn compile_c(&self, out: &Path) -> Result<(), String> {
        if !matches!(self.kind, ForeignKind::C) {
            return Err("compile_c called for a non-C foreign library".into());
        }
        if self.sources.is_empty() {
            return Err(format!(
                "foreign library '{}' has no source files",
                self.name
            ));
        }

        let mut cmd = Command::new("clang");
        cmd.arg("-c");
        for dir in &self.include_dirs {
            cmd.arg("-I").arg(dir);
        }
        for src in &self.sources {
            cmd.arg(src);
        }
        for arg in &self.extra_args {
            cmd.arg(arg);
        }
        cmd.arg("-o").arg(out);

        let status = cmd
            .status()
            .map_err(|e| format!("could not start clang: {}", e))?;
        if !status.success() {
            return Err(format!(
                "clang failed while compiling foreign library '{}'",
                self.name
            ));
        }
        Ok(())
    }

    pub fn link_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        for dir in &self.library_dirs {
            args.push("-L".into());
            args.push(dir.display().to_string());
        }
        for lib in &self.libraries {
            args.push(format!("-l{}", lib));
        }
        args
    }
}
