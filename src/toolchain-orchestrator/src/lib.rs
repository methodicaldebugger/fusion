//! Toolchain discovery and foreign-build orchestration.
//!
//! The orchestrator owns external compilers, linkers, SDKs and runtimes. It
//! consumes the neutral `fusion-interop-model` rather than putting foreign
//! toolchain details into the Fusion parser or Foundry dependency resolver.

use fusion_interop_model::{
    ArtifactKind, ForeignLanguage, ForeignSource, IntegrationKind, InteropSpec,
};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    CCompiler,
    CppCompiler,
    RustCompiler,
    Linker,
    WasmCompiler,
}

impl Tool {
    fn candidates(self) -> &'static [&'static str] {
        match self {
            Self::CCompiler => &["clang", "cc", "gcc"],
            Self::CppCompiler => &["clang++", "c++", "g++"],
            Self::RustCompiler => &["rustc"],
            Self::Linker => &["clang", "cc", "ld"],
            Self::WasmCompiler => &["clang"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    pub target: String,
    pub c_compiler: Option<PathBuf>,
    pub cpp_compiler: Option<PathBuf>,
    pub rustc: Option<PathBuf>,
    pub linker: Option<PathBuf>,
    pub wasm_compiler: Option<PathBuf>,
}

impl Toolchain {
    pub fn discover(target: Option<&str>) -> Self {
        let target = target
            .map(str::to_owned)
            .or_else(|| env::var("TARGET").ok())
            .unwrap_or_else(|| env::consts::ARCH.to_string());

        Self {
            target,
            c_compiler: find_tool(Tool::CCompiler),
            cpp_compiler: find_tool(Tool::CppCompiler),
            rustc: find_tool(Tool::RustCompiler),
            linker: find_tool(Tool::Linker),
            wasm_compiler: find_tool(Tool::WasmCompiler),
        }
    }

    pub fn compiler_for(&self, language: ForeignLanguage) -> Option<&Path> {
        match language {
            ForeignLanguage::C => self.c_compiler.as_deref(),
            ForeignLanguage::Cpp => self.cpp_compiler.as_deref(),
            ForeignLanguage::Rust => self.rustc.as_deref(),
            _ => None,
        }
    }

    pub fn require(&self, tool: Tool) -> Result<&Path, OrchestratorError> {
        let value = match tool {
            Tool::CCompiler => self.c_compiler.as_deref(),
            Tool::CppCompiler => self.cpp_compiler.as_deref(),
            Tool::RustCompiler => self.rustc.as_deref(),
            Tool::Linker => self.linker.as_deref(),
            Tool::WasmCompiler => self.wasm_compiler.as_deref(),
        };
        value.ok_or_else(|| OrchestratorError::ToolUnavailable {
            tool,
            target: self.target.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRequest {
    pub spec: InteropSpec,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildArtifact {
    pub path: PathBuf,
    pub kind: ArtifactKind,
}

#[derive(Debug)]
pub enum OrchestratorError {
    InvalidSpec(String),
    ToolUnavailable { tool: Tool, target: String },
    UnsupportedLanguage(ForeignLanguage),
    UnsupportedIntegration(IntegrationKind),
    Io(String),
    CommandFailed { program: PathBuf, status: String },
}

impl fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec(e) => write!(f, "invalid interop specification: {e}"),
            Self::ToolUnavailable { tool, target } => write!(f, "required tool {:?} is unavailable for target {target}", tool),
            Self::UnsupportedLanguage(lang) => write!(f, "no foreign build backend for {}", lang.name()),
            Self::UnsupportedIntegration(kind) => write!(f, "integration backend for {kind} is not implemented"),
            Self::Io(e) => write!(f, "toolchain I/O error: {e}"),
            Self::CommandFailed { program, status } => write!(f, "{} failed: {status}", program.display()),
        }
    }
}

impl std::error::Error for OrchestratorError {}

pub struct Orchestrator {
    pub toolchain: Toolchain,
}

impl Orchestrator {
    pub fn new(toolchain: Toolchain) -> Self {
        Self { toolchain }
    }

    /// Validate the integration and execute its foreign build.
    ///
    /// Native ABI C/C++ and Rust source are supported in this bootstrap
    /// implementation. Other integration kinds have an explicit error rather
    /// than silently pretending that a foreign boundary was built.
    pub fn build(&self, request: &BuildRequest) -> Result<Vec<BuildArtifact>, OrchestratorError> {
        request
            .spec
            .validate()
            .map_err(|e| OrchestratorError::InvalidSpec(e.to_string()))?;

        match request.spec.kind {
            IntegrationKind::NativeAbi => self.build_native(request),
            IntegrationKind::Process => self.build_process(request),
            kind => Err(OrchestratorError::UnsupportedIntegration(kind)),
        }
    }

    fn build_native(&self, request: &BuildRequest) -> Result<Vec<BuildArtifact>, OrchestratorError> {
        fs::create_dir_all(&request.output_dir)
            .map_err(|e| OrchestratorError::Io(e.to_string()))?;

        let mut artifacts = Vec::new();
        for source in &request.spec.sources {
            let object = object_path(&request.output_dir, &source.path);
            match source.language {
                ForeignLanguage::C | ForeignLanguage::Cpp => {
                    self.compile_c_like(source, &object, &request.spec)?;
                    artifacts.push(BuildArtifact { path: object, kind: ArtifactKind::Object });
                }
                ForeignLanguage::Rust => {
                    return Err(OrchestratorError::UnsupportedLanguage(source.language));
                }
                language => return Err(OrchestratorError::UnsupportedLanguage(language)),
            }
        }
        Ok(artifacts)
    }

    fn compile_c_like(
        &self,
        source: &ForeignSource,
        output: &Path,
        spec: &InteropSpec,
    ) -> Result<(), OrchestratorError> {
        let tool = match source.language {
            ForeignLanguage::C => Tool::CCompiler,
            ForeignLanguage::Cpp => Tool::CppCompiler,
            _ => return Err(OrchestratorError::UnsupportedLanguage(source.language)),
        };
        let compiler = self.toolchain.require(tool)?;
        let mut command = Command::new(compiler);
        command.arg("-c").arg(&source.path).arg("-o").arg(output);

        let req = &spec.requirements;
        if let Some(target) = &req.target {
            command.arg("--target").arg(target);
        }
        if let Some(sysroot) = &req.sysroot {
            command.arg("--sysroot").arg(sysroot);
        }
        for dir in &req.include_dirs {
            command.arg("-I").arg(dir);
        }
        for define in &req.defines {
            command.arg("-D").arg(define);
        }
        command.args(&req.extra_args);
        run(command)
    }

    fn build_process(&self, request: &BuildRequest) -> Result<Vec<BuildArtifact>, OrchestratorError> {
        if request.spec.sources.len() != 1 {
            return Err(OrchestratorError::InvalidSpec(
                "a process integration must contain exactly one executable source".into(),
            ));
        }
        Ok(vec![BuildArtifact {
            path: request.spec.sources[0].path.clone(),
            kind: ArtifactKind::Executable,
        }])
    }
}

fn run(mut command: Command) -> Result<(), OrchestratorError> {
    let program = command.get_program().to_owned().into();
    let output: Output = command
        .output()
        .map_err(|e| OrchestratorError::Io(format!("could not start {}: {e}", program.display())))?;
    if output.status.success() {
        return Ok(());
    }
    let mut status = output.status.to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        status.push_str(&format!(": {}", stderr.trim()));
    }
    Err(OrchestratorError::CommandFailed { program, status })
}

fn object_path(output_dir: &Path, source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("foreign");
    output_dir.join(format!("{stem}.o"))
}

fn find_tool(tool: Tool) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for candidate in tool.candidates() {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
            #[cfg(windows)]
            {
                let exe = dir.join(format!("{candidate}.exe"));
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_interop_model::{ForeignSource, InteropSpec};

    #[test]
    fn discovers_a_toolchain_without_running_it() {
        let tc = Toolchain::discover(Some("native"));
        assert_eq!(tc.target, "native");
    }

    #[test]
    fn c_native_build_produces_an_object_when_a_c_compiler_exists() {
        let tc = Toolchain::discover(None);
        if tc.c_compiler.is_none() {
            return;
        }

        let base = std::env::temp_dir().join(format!(
            "fusion-orchestrator-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("src/test.c"), "int fusion_test(void) { return 42; }\n").unwrap();

        let mut spec = InteropSpec::new("test-c", IntegrationKind::NativeAbi);
        spec.sources.push(ForeignSource::new(ForeignLanguage::C, base.join("src/test.c")));
        spec.artifacts.push(ArtifactKind::Object);

        let artifacts = Orchestrator::new(tc)
            .build(&BuildRequest { spec, output_dir: base.join("out") })
            .unwrap();

        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].path.is_file());
        let _ = fs::remove_dir_all(base);
    }
}
