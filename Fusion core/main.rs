#[path = "linker_config.rs"]
mod linker_config;

use std::{
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

fn usage() {
    eprintln!("Fusion {}", env!("CARGO_PKG_VERSION"));
    eprintln!("usage:");
    eprintln!("  fusion run <file.fusion> [--concurrent-async]");
    eprintln!("  fusion check <file.fusion>");
    eprintln!("  fusion emit-llvm <file.fusion> [-o output.ll]");
    eprintln!("  fusion emit-package-llvm <file.fusion> --package-source-root <src-dir> [-o package.ll]");
    eprintln!("  fusion emit-entrypoint-llvm <file.fusion> [-o entrypoint.ll]");
    eprintln!("  fusion package-interface <file.fusion> --package-source-root <src-dir> [-o output.fpi]");
    eprintln!("  fusion package-declarations <input.fpi> [-o declarations.ll]");
    eprintln!("  fusion link-llvm <input.ll> [additional.ll ...] [-o output]");
    eprintln!("  fusion build <file.fusion> [-o output] [--package-root alias=src-path]");
    eprintln!("  fusion test <file.fusion> [--filter text] [--ignored]");
    eprintln!("  fusion bench <file.fusion> [--filter text]");
    eprintln!("  fusion doc <file.fusion|directory> [--html] [-o output]");
}

fn write_or_print(path: Option<&str>, data: &str) -> Result<(), String> {
    match path {
        Some(p) => fs::write(p, data).map_err(|e| format!("could not write '{}': {}", p, e)),
        None => {
            print!("{}", data);
            Ok(())
        }
    }
}

fn run_interpreter(path: &str, concurrent_async: bool, package_roots: &std::collections::BTreeMap<String, PathBuf>) -> Result<(), String> {
    let program = fusion::compile_project_with_package_roots(path, package_roots.clone())?;
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.set_concurrent_async(concurrent_async || std::env::var("FUSION_CONCURRENT_ASYNC").map_or(false, |v| v == "1"));
    interpreter.execute(&program);

    for line in interpreter.output() {
        println!("{}", line);
    }

    Ok(())
}

fn find_on_path(command: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_clang() -> Result<PathBuf, String> {
    // Explicit configuration wins. This is useful on Windows where LLVM is
    // often installed but its bin directory is not on PATH.
    for variable in ["FUSION_LLVM_DIR", "LLVM_HOME"] {
        if let Ok(dir) = env::var(variable) {
            let candidate = PathBuf::from(dir).join("bin").join(if cfg!(windows) {
                "clang.exe"
            } else {
                "clang"
            });
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    let command = if cfg!(windows) { "clang.exe" } else { "clang" };
    if let Some(path) = find_on_path(command) {
        return Ok(path);
    }

    // Common Windows installations. Visual Studio can install LLVM/Clang
    // independently of the standalone LLVM installer.
    if cfg!(windows) {
        let candidates = [
            PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files (x86)\LLVM\bin\clang.exe"),
            PathBuf::from(
                r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin\clang.exe",
            ),
            PathBuf::from(
                r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\Llvm\x64\bin\clang.exe",
            ),
            PathBuf::from(
                r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\Llvm\x64\bin\clang.exe",
            ),
            PathBuf::from(
                r"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin\clang.exe",
            ),
        ];
        for candidate in candidates {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(format!(
        "could not find clang. Install LLVM/Clang or set FUSION_LLVM_DIR to the LLVM installation directory. Searched PATH and common Windows locations."
    ))
}

fn link_llvm(inputs: &[String], output: &str) -> Result<(), String> {
    if inputs.is_empty() {
        return Err("at least one LLVM input is required".into());
    }
    for input in inputs {
        if !std::path::Path::new(input).is_file() {
            return Err(format!("LLVM input '{}' does not exist", input));
        }
    }
    let clang = find_clang()?;
    let mut command = Command::new(&clang);
    command.args(linker_config::FLAGS).args(inputs).arg("-o").arg(output);
    let status = command
        .status()
        .map_err(|e| format!("could not start clang at '{}': {}", clang.display(), e))?;
    if !status.success() {
        return Err(format!("clang failed while linking LLVM IR (using '{}')", clang.display()));
    }
    Ok(())
}

fn build(source: &str, output: &str, package_roots: &std::collections::BTreeMap<String, PathBuf>) -> Result<(), String> {
    let ll = fusion::emit_llvm_project_with_package_roots(source, package_roots.clone())?;
    let base = std::env::temp_dir().join(format!(
        "fusion-{}-{}.ll",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&base, ll).map_err(|e| format!("temporary LLVM file: {}", e))?;
    let result = link_llvm(&[base.to_string_lossy().into_owned()], output);
    let _ = fs::remove_file(&base);
    result
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "run".into());
    if command == "--version" || command == "-V" || command == "version" {
        println!("fusion {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let path = match args.next() {
        Some(p) => p,
        None => {
            usage();
            return ExitCode::from(2);
        }
    };
    let mut output = None;
    let mut html = false;
    let mut filter: Option<String> = None;
    let mut include_ignored = false;
    let mut concurrent_async = false;
    let mut package_roots = std::collections::BTreeMap::new();
    let mut llvm_inputs = vec![path.clone()];
    let mut package_source_root: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => output = args.next(),
            "--html" => html = true,
            "--filter" => filter = args.next(),
            "--ignored" => include_ignored = true,
            "--concurrent-async" => concurrent_async = true,
            "--package-source-root" => {
                let value = args.next().unwrap_or_default();
                if value.is_empty() || package_source_root.replace(PathBuf::from(value)).is_some() {
                    eprintln!("invalid or duplicate --package-source-root");
                    return ExitCode::from(2);
                }
            },
            "--package-root" => {
                let spec = args.next().unwrap_or_default();
                let Some((alias, path)) = spec.split_once('=') else {
                    eprintln!("--package-root expects alias=path");
                    return ExitCode::from(2);
                };
                if alias.is_empty() || path.is_empty() || package_roots.insert(alias.to_string(), PathBuf::from(path)).is_some() {
                    eprintln!("invalid or duplicate --package-root alias");
                    return ExitCode::from(2);
                }
            },
            _ if command == "link-llvm" && !a.starts_with('-') => llvm_inputs.push(a.clone()),
            _ => {
                eprintln!("unknown option '{}'", a);
                return ExitCode::from(2);
            }
        }
    }

    let result = match command.as_str() {
        "run" => run_interpreter(&path, concurrent_async, &package_roots),
        "check" => fusion::compile_project_with_package_roots(&path, package_roots.clone()).map(|_| {
            println!("ok: {}", path);
        }),
        "emit-llvm" => {
            fusion::emit_llvm_project_with_package_roots(&path, package_roots.clone()).and_then(|ll| write_or_print(output.as_deref(), &ll))
        }
        "emit-package-llvm" => {
            match package_source_root.as_deref() {
                Some(root) => fusion::emit_llvm_package_with_package_roots(&path, package_roots.clone(), root)
                    .and_then(|ir| write_or_print(output.as_deref(), &ir)),
                None => Err("emit-package-llvm requires --package-source-root".into()),
            }
        }
        "emit-entrypoint-llvm" => fusion::emit_llvm_entrypoint_with_package_roots(&path, package_roots.clone())
            .and_then(|ir| write_or_print(output.as_deref(), &ir)),
        "package-interface" => {
            match package_source_root.as_deref() {
                Some(root) => fusion::emit_package_interface_with_package_roots(&path, package_roots.clone(), root)
                    .and_then(|interface| write_or_print(output.as_deref(), &interface)),
                None => Err("package-interface requires --package-source-root".into()),
            }
        }
        "package-declarations" => fs::read_to_string(&path)
            .map_err(|error| format!("could not read package interface '{}': {error}", path))
            .and_then(|interface| fusion::emit_package_external_declarations(&interface))
            .and_then(|declarations| write_or_print(output.as_deref(), &declarations)),
        "link-llvm" => link_llvm(&llvm_inputs, output.as_deref().unwrap_or("a.out")),
        "build" => build(&path, output.as_deref().unwrap_or("a.out"), &package_roots),
        "test" => {
            let options = fusion::test_runner::Options { filter, include_ignored };
            fusion::test_runner::run_tests(&path, &options).and_then(|summary| {
                if summary.failed == 0 {
                    Ok(())
                } else {
                    Err(format!("{} test(s) failed", summary.failed))
                }
            })
        }
        "bench" => {
            let options = fusion::test_runner::Options { filter, include_ignored };
            fusion::test_runner::run_benchmarks(&path, &options).and_then(|failed| {
                if failed == 0 {
                    Ok(())
                } else {
                    Err(format!("{} benchmark(s) failed", failed))
                }
            })
        }
        "doc" => {
            let format = if html { fusion::doc_gen::Format::Html } else { fusion::doc_gen::Format::Markdown };
            fusion::doc_gen::generate(&path, format).and_then(|text| write_or_print(output.as_deref(), &text))
        }
        _ => {
            usage();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {}", e);
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn link_llvm_validates_every_input_before_looking_for_clang() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let existing = std::env::temp_dir().join(format!("fusion-existing-ir-{}-{nonce}.ll", std::process::id()));
        std::fs::write(&existing, "; placeholder\n").unwrap();
        let missing = existing.with_extension("missing.ll");
        let error = link_llvm(&[
            existing.to_string_lossy().into_owned(),
            missing.to_string_lossy().into_owned(),
        ], "unused-output").unwrap_err();
        assert!(error.contains("LLVM input"));
        assert!(error.contains("does not exist"));
        let _ = std::fs::remove_file(existing);
    }

    #[test]
    fn link_llvm_rejects_missing_input_before_looking_for_clang() {
        let missing = std::env::temp_dir().join(format!(
            "fusion-missing-ir-{}-{}.ll",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let error = link_llvm(&[missing.to_string_lossy().into_owned()], "unused-output").unwrap_err();
        assert!(error.contains("LLVM input"));
        assert!(error.contains("does not exist"));
    }
}
