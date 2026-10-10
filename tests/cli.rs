use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    dir: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fusion-cli-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temporary test directory");
        let path = dir.join("main.fusion");
        fs::write(&path, source).expect("write Fusion test program");
        Self { dir, source: path }
    }

    fn run(&self, command: &str) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_fusion"))
            .arg(command)
            .arg(&self.source)
            .output()
            .expect("launch Fusion CLI")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn run_executes_a_fusion_file_through_the_cli() {
    let fixture = Fixture::new(
        "main:\n    answer = 40 + 2\n    print(answer)\n",
    );

    let output = fixture.run("run");

    assert!(
        output.status.success(),
        "Fusion run failed. stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}

#[test]
fn check_accepts_a_valid_fusion_file_through_the_cli() {
    let fixture = Fixture::new("main:\n    print(42)\n");

    let output = fixture.run("check");

    assert!(
        output.status.success(),
        "Fusion check failed. stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ok:"),
        "expected check command to report success; stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn run_reports_invalid_fusion_source_with_a_failure_exit_code() {
    let fixture = Fixture::new("main:\n    print(missing_name)\n");

    let output = fixture.run("run");

    assert!(
        !output.status.success(),
        "invalid source unexpectedly succeeded; stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("error:"),
        "expected a diagnostic on stderr; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
