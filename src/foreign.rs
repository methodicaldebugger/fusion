
//! Project-level foreign dependency model.
//!
//! Fusion deliberately keeps foreign source out of `.fusion` files. This module
//! is the first bootstrap implementation of that boundary for C. The parser
//! does not need to understand C; the build system owns these files.

use std::path::{Path, PathBuf};
use std::process::Command;

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
            return Err(format!("foreign library '{}' has no source files", self.name));
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

        let status = cmd.status().map_err(|e| format!("could not start clang: {}", e))?;
        if !status.success() {
            return Err(format!("clang failed while compiling foreign library '{}'", self.name));
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
