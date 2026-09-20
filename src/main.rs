
use std::{env, fs, path::PathBuf, process::{Command, ExitCode}};

fn usage() {
    eprintln!("Fusion 0.2");
    eprintln!("usage:");
    eprintln!("  fusion run <file.fusion>");
    eprintln!("  fusion check <file.fusion>");
    eprintln!("  fusion emit-llvm <file.fusion> [-o output.ll]");
    eprintln!("  fusion build <file.fusion> [-o output]");
}

fn read_source(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("could not read '{}': {}", path, e))
}

fn write_or_print(path: Option<&str>, data: &str) -> Result<(), String> {
    match path {
        Some(p) => fs::write(p, data).map_err(|e| format!("could not write '{}': {}", p, e)),
        None => { print!("{}", data); Ok(()) }
    }
}

fn run_interpreter(source: &str) -> Result<(), String> {
    let program = fusion::compile_source(source)?;
    let mut interpreter = fusion::interpreter::Interpreter::new();
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
            let candidate = PathBuf::from(dir).join("bin").join(if cfg!(windows) { "clang.exe" } else { "clang" });
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
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin\clang.exe"),
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

fn build(source: &str, output: &str) -> Result<(), String> {
    let ll = fusion::emit_llvm(source)?;
    let base = std::env::temp_dir().join(format!("fusion-{}-{}.ll", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::write(&base, ll).map_err(|e| format!("temporary LLVM file: {}", e))?;

    let clang = find_clang()?;
    let status = Command::new(&clang).arg("-O2").arg(&base).arg("-o").arg(output).status()
        .map_err(|e| format!("could not start clang at '{}': {}", clang.display(), e))?;
    let _ = fs::remove_file(&base);
    if !status.success() {
        return Err(format!("clang failed while lowering LLVM IR (using '{}')", clang.display()));
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "run".into());
    let path = match args.next() {
        Some(p) => p,
        None => { usage(); return ExitCode::from(2); }
    };
    let mut output = None;
    while let Some(a) = args.next() {
        if a == "-o" { output = args.next(); } else { eprintln!("unknown option '{}'", a); return ExitCode::from(2); }
    }

    let source = match read_source(&path) {
        Ok(s) => s,
        Err(e) => { eprintln!("error: {}", e); return ExitCode::from(1); }
    };

    let result = match command.as_str() {
        "run" => run_interpreter(&source),
        "check" => fusion::compile_source(&source).map(|_| { println!("ok: {}", path); }),
        "emit-llvm" => fusion::emit_llvm(&source).and_then(|ll| write_or_print(output.as_deref(), &ll)),
        "build" => build(&source, output.as_deref().unwrap_or("a.out")),
        _ => { usage(); return ExitCode::from(2); }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("error: {}", e); ExitCode::from(1) }
    }
}
