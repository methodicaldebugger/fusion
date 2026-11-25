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

/// Ownership behavior for an opaque pointer crossing the C ABI boundary.
///
/// `Unspecified` is deliberately distinct from `Borrowed`: legacy declarations
/// may omit a contract, but safety-sensitive consumers must not infer that an
/// unspecified pointer is safe to retain or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignPointerOwnership {
    Unspecified,
    Borrowed,
    /// Ownership of a returned pointer is transferred to Fusion.
    Owned,
    /// Ownership of a parameter is transferred from Fusion to the callee.
    Consumed,
}

/// Contract for one opaque pointer. Nullability is independent of ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignPointerContract {
    pub ownership: ForeignPointerOwnership,
    pub nullable: bool,
    /// Required for an `Owned` return value; names the C release function.
    pub destructor: Option<String>,
}

impl ForeignPointerContract {
    pub fn borrowed(nullable: bool) -> Self {
        Self { ownership: ForeignPointerOwnership::Borrowed, nullable, destructor: None }
    }

    pub fn owned(nullable: bool, destructor: impl Into<String>) -> Self {
        Self { ownership: ForeignPointerOwnership::Owned, nullable, destructor: Some(destructor.into()) }
    }

    pub fn consumed(nullable: bool) -> Self {
        Self { ownership: ForeignPointerOwnership::Consumed, nullable, destructor: None }
    }
}

/// Language-level declaration metadata shared by the type checker and native
/// backend. Pointer contracts are kept alongside (not encoded into) the C ABI
/// types so the LLVM signature remains ABI-compatible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignFunctionDecl {
    pub source_name: String,
    pub link_name: String,
    pub parameters: Vec<ForeignAbiType>,
    pub return_type: ForeignAbiType,
    pub calling_convention: ForeignCallingConvention,
    /// One entry per parameter. `None` means the source declaration has not yet
    /// supplied an ownership contract; non-pointer parameters must use `None`.
    pub parameter_pointer_contracts: Vec<Option<ForeignPointerContract>>,
    /// Present only when the return type is an opaque pointer.
    pub return_pointer_contract: Option<ForeignPointerContract>,
}

impl ForeignFunctionDecl {
    /// Initialize conservative, explicitly unspecified pointer contracts.
    pub fn with_default_pointer_contracts(mut self) -> Self {
        self.parameter_pointer_contracts = self.parameters.iter().map(|ty| {
            matches!(ty, ForeignAbiType::OpaquePointer).then(|| ForeignPointerContract {
                ownership: ForeignPointerOwnership::Unspecified,
                nullable: false,
                destructor: None,
            })
        }).collect();
        self.return_pointer_contract = matches!(self.return_type, ForeignAbiType::OpaquePointer)
            .then(|| ForeignPointerContract {
                ownership: ForeignPointerOwnership::Unspecified,
                nullable: false,
                destructor: None,
            });
        self
    }

    pub fn set_parameter_pointer_contract(
        &mut self,
        index: usize,
        contract: ForeignPointerContract,
    ) -> Result<(), String> {
        let Some(parameter) = self.parameters.get(index) else {
            return Err(format!("foreign parameter index {} is out of range", index));
        };
        if !matches!(parameter, ForeignAbiType::OpaquePointer) {
            return Err(format!("foreign parameter {} is not an opaque pointer", index + 1));
        }
        if contract.ownership == ForeignPointerOwnership::Owned {
            return Err("a foreign parameter cannot transfer ownership to Fusion as an owned input; use Consumed to transfer ownership to the callee".into());
        }
        if contract.destructor.is_some() {
            return Err("parameter pointer contracts cannot declare a destructor".into());
        }
        if self.parameter_pointer_contracts.len() != self.parameters.len() {
            self.parameter_pointer_contracts = self.parameters.iter().map(|_| None).collect();
        }
        self.parameter_pointer_contracts[index] = Some(contract);
        Ok(())
    }

    pub fn set_return_pointer_contract(
        &mut self,
        contract: ForeignPointerContract,
    ) -> Result<(), String> {
        if !matches!(self.return_type, ForeignAbiType::OpaquePointer) {
            return Err("a return pointer contract requires an opaque-pointer return type".into());
        }
        self.return_pointer_contract = Some(contract);
        Ok(())
    }
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
        if self.parameter_pointer_contracts.len() != self.parameters.len() {
            return Err("foreign pointer contract count must match the parameter count".into());
        }
        for (index, (ty, contract)) in self.parameters.iter().zip(&self.parameter_pointer_contracts).enumerate() {
            match (ty, contract) {
                (ForeignAbiType::OpaquePointer, Some(contract)) => {
                    if contract.ownership == ForeignPointerOwnership::Owned {
                        return Err(format!("foreign parameter {} cannot use Owned; use Consumed for ownership transfer to the callee", index + 1));
                    }
                    if contract.destructor.is_some() {
                        return Err(format!("foreign parameter {} cannot declare a destructor", index + 1));
                    }
                }
                (ForeignAbiType::OpaquePointer, None) | (_, None) => {}
                (_, Some(_)) => return Err(format!("foreign parameter {} has a pointer contract but is not an opaque pointer", index + 1)),
            }
        }
        match (&self.return_type, &self.return_pointer_contract) {
            (ForeignAbiType::OpaquePointer, Some(contract)) => {
                match contract.ownership {
                    ForeignPointerOwnership::Owned => {
                        let destructor = contract.destructor.as_deref().unwrap_or("").trim();
                        if destructor.is_empty() {
                            return Err("owned foreign pointer return requires a non-empty destructor symbol".into());
                        }
                    }
                    _ if contract.destructor.is_some() => {
                        return Err("only an Owned foreign pointer return may declare a destructor".into());
                    }
                    _ => {}
                }
            }
            (ForeignAbiType::OpaquePointer, None) | (_, None) => {}
            (_, Some(_)) => return Err("foreign return pointer contract requires an opaque-pointer return type".into()),
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


#[cfg(test)]
mod pointer_contract_tests {
    use super::*;

    fn declaration() -> ForeignFunctionDecl {
        ForeignFunctionDecl {
            source_name: "open_handle".into(),
            link_name: "open_handle".into(),
            parameters: vec![ForeignAbiType::OpaquePointer],
            return_type: ForeignAbiType::OpaquePointer,
            calling_convention: ForeignCallingConvention::C,
            parameter_pointer_contracts: vec![Some(ForeignPointerContract::borrowed(false))],
            return_pointer_contract: Some(ForeignPointerContract::owned(false, "close_handle")),
        }
    }

    #[test]
    fn owned_pointer_return_requires_destructor() {
        let mut decl = declaration();
        decl.return_pointer_contract = Some(ForeignPointerContract {
            ownership: ForeignPointerOwnership::Owned,
            nullable: false,
            destructor: None,
        });
        assert!(decl.validate().unwrap_err().contains("requires a non-empty destructor"));
    }

    #[test]
    fn nullability_is_independent_of_ownership() {
        let mut decl = declaration();
        decl.return_pointer_contract = Some(ForeignPointerContract::owned(true, "close_handle"));
        assert!(decl.validate().is_ok());
        decl.parameter_pointer_contracts[0] = Some(ForeignPointerContract::borrowed(true));
        assert!(decl.validate().is_ok());
    }

    #[test]
    fn consumed_parameter_models_transfer_to_callee() {
        let mut decl = declaration();
        decl.parameter_pointer_contracts[0] = Some(ForeignPointerContract::consumed(false));
        assert!(decl.validate().is_ok());
    }

    #[test]
    fn owned_parameter_is_rejected_as_ambiguous_direction() {
        let mut decl = declaration();
        decl.parameter_pointer_contracts[0] = Some(ForeignPointerContract {
            ownership: ForeignPointerOwnership::Owned,
            nullable: false,
            destructor: None,
        });
        assert!(decl.validate().unwrap_err().contains("cannot use Owned"));
    }

    #[test]
    fn pointer_contract_cannot_be_attached_to_scalar() {
        let mut decl = declaration();
        decl.parameters[0] = ForeignAbiType::I32;
        assert!(decl.validate().unwrap_err().contains("not an opaque pointer"));
    }
}
