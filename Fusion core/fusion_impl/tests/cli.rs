use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn write_fusion_file(source: &str) -> std::path::PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "fusion_cli_test_{}_{}.fusion",
        std::process::id(),
        id
    ));
    fs::write(&path, source).expect("write temporary Fusion source");
    path
}

fn fusion() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fusion"))
}

#[test]
fn run_executes_a_fusion_file_through_the_cli() {
    let path = write_fusion_file("main:\n    answer = 40 + 2\n    print(answer)\n");

    let output = fusion()
        .arg("run")
        .arg(&path)
        .output()
        .expect("run Fusion CLI");

    let _ = fs::remove_file(&path);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}

#[test]
fn check_accepts_valid_fusion_source() {
    let path = write_fusion_file("main:\n    print(42)\n");

    let output = fusion()
        .arg("check")
        .arg(&path)
        .output()
        .expect("run Fusion CLI");

    let _ = fs::remove_file(&path);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ok:"),
        "expected an 'ok:' message, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn invalid_fusion_source_returns_failure_and_reports_error() {
    let path = write_fusion_file("main:\n    print(missing_name)\n");

    let output = fusion()
        .arg("run")
        .arg(&path)
        .output()
        .expect("run Fusion CLI");

    let _ = fs::remove_file(&path);

    assert!(
        !output.status.success(),
        "invalid source unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("error:"),
        "expected an error message, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_source_file_returns_failure_and_reports_error() {
    let path =
        std::env::temp_dir().join(format!("fusion_cli_missing_{}.fusion", std::process::id()));
    let _ = fs::remove_file(&path);

    let output = fusion()
        .arg("run")
        .arg(&path)
        .output()
        .expect("run Fusion CLI");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("error:"),
        "expected an error message, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn native_output_path(label: &str) -> std::path::PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let filename = if cfg!(windows) {
        format!("fusion_native_{}_{}_{}.exe", std::process::id(), label, id)
    } else {
        format!("fusion_native_{}_{}_{}", std::process::id(), label, id)
    };
    std::env::temp_dir().join(filename)
}

fn assert_native_matches_interpreter(source: &str, label: &str) {
    let source_path = write_fusion_file(source);
    let native_path = native_output_path(label);

    // Establish the interpreter's output first.
    let interpreted = fusion()
        .arg("run")
        .arg(&source_path)
        .output()
        .expect("run Fusion interpreter");

    assert!(
        interpreted.status.success(),
        "interpreter failed: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );

    // Build a native executable using Fusion's LLVM/Clang pipeline.
    let build = fusion()
        .arg("build")
        .arg(&source_path)
        .arg("-o")
        .arg(&native_path)
        .output()
        .expect("run Fusion native build");

    assert!(
        build.status.success(),
        "native build failed. This test requires Clang. Install LLVM/Clang or configure FUSION_LLVM_DIR.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let native = Command::new(&native_path)
        .output()
        .expect("run generated native executable");

    let _ = fs::remove_file(&source_path);
    let _ = fs::remove_file(&native_path);

    assert!(
        native.status.success(),
        "native executable failed: {}",
        String::from_utf8_lossy(&native.stderr)
    );

    let native_stdout = String::from_utf8_lossy(&native.stdout).replace("\r\n", "\n");
    let interpreted_stdout = String::from_utf8_lossy(&interpreted.stdout).replace("\r\n", "\n");

    assert_eq!(
        native_stdout, interpreted_stdout,
        "native output differs from interpreter output"
    );
}

#[test]
fn native_build_matches_interpreter_for_arithmetic() {
    assert_native_matches_interpreter(
        "main:\n    x = 10\n    y = 32\n    print(x + y)\n",
        "arithmetic",
    );
}

#[test]
fn native_build_matches_interpreter_for_functions_and_loops() {
    assert_native_matches_interpreter(
        "fn sum_to(n: num) -> num:\n    total = 0\n    for i in 0..n:\n        total = total + i\n    return total\n\nmain:\n    print(sum_to(10))\n",
        "control",
    );
}
