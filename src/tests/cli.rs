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
