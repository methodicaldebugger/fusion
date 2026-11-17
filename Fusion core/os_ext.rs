//! File system, path and process utilities.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Paths (purely lexical, no file system access)
// ---------------------------------------------------------------------------

pub fn path_join(a: &str, b: &str) -> String {
    Path::new(a).join(b).to_string_lossy().into_owned()
}

pub fn path_parent(p: &str) -> Option<String> {
    Path::new(p)
        .parent()
        .map(|x| x.to_string_lossy().into_owned())
}

pub fn path_file_name(p: &str) -> Option<String> {
    Path::new(p)
        .file_name()
        .map(|x| x.to_string_lossy().into_owned())
}

pub fn path_extension(p: &str) -> Option<String> {
    Path::new(p)
        .extension()
        .map(|x| x.to_string_lossy().into_owned())
}

pub fn path_stem(p: &str) -> Option<String> {
    Path::new(p)
        .file_stem()
        .map(|x| x.to_string_lossy().into_owned())
}

pub fn path_is_absolute(p: &str) -> bool {
    Path::new(p).is_absolute()
}

/// Removes `.` components and resolves `..` where possible, without touching the file system.
pub fn path_normalize(p: &str) -> String {
    let mut out = PathBuf::new();
    for c in Path::new(p).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if can_pop {
                    out.pop();
                } else if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir) | Some(Component::Prefix(_))
                ) {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        ".".to_string()
    } else {
        out.to_string_lossy().into_owned()
    }
}

// ---------------------------------------------------------------------------
// File system
// ---------------------------------------------------------------------------

fn io<T>(what: &str, path: &str, r: std::io::Result<T>) -> Result<T, String> {
    r.map_err(|e| format!("{} '{}': {}", what, path, e))
}

pub fn is_file(p: &str) -> bool {
    Path::new(p).is_file()
}

pub fn is_dir(p: &str) -> bool {
    Path::new(p).is_dir()
}

pub fn file_size(p: &str) -> Result<i64, String> {
    Ok(io("file_size", p, std::fs::metadata(p))?.len() as i64)
}

pub fn modified_time(p: &str) -> Result<i64, String> {
    let t = io(
        "modified_time",
        p,
        io("metadata", p, std::fs::metadata(p))?.modified(),
    )?;
    Ok(match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    })
}

pub fn copy_file(from: &str, to: &str) -> Result<i64, String> {
    Ok(io("copy_file", from, std::fs::copy(from, to))? as i64)
}

pub fn rename_path(from: &str, to: &str) -> Result<(), String> {
    io("rename", from, std::fs::rename(from, to))
}

/// Removes a directory and everything inside it.
pub fn remove_dir(p: &str) -> Result<(), String> {
    io("remove_dir", p, std::fs::remove_dir_all(p))
}

pub fn append_file(p: &str, text: &str) -> Result<(), String> {
    let mut f = io(
        "append_file",
        p,
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p),
    )?;
    io("append_file", p, f.write_all(text.as_bytes()))
}

pub fn read_lines(p: &str) -> Result<Vec<String>, String> {
    let text = io("read_lines", p, std::fs::read_to_string(p))?;
    Ok(text.lines().map(|l| l.to_string()).collect())
}

/// Recursively lists everything below `root` as paths relative to it (directories included),
/// in sorted order. Symbolic links are listed but not followed.
pub fn walk_dir(root: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    fn visit(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("walk_dir '{}': {}", dir.display(), e))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            out.push(rel);
            let is_real_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_real_dir {
                visit(base, &path, out)?;
            }
        }
        Ok(())
    }
    let base = Path::new(root);
    visit(base, base, &mut out)?;
    Ok(out)
}

pub fn is_readonly(p: &str) -> Result<bool, String> {
    Ok(io("is_readonly", p, std::fs::metadata(p))?
        .permissions()
        .readonly())
}

pub fn set_readonly(p: &str, readonly: bool) -> Result<(), String> {
    let mut perms = io("set_readonly", p, std::fs::metadata(p))?.permissions();
    perms.set_readonly(readonly);
    io("set_readonly", p, std::fs::set_permissions(p, perms))
}

/// Unix permission bits (e.g. 493 == 0o755); -1 on platforms without them.
pub fn file_mode(p: &str) -> Result<i64, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok((io("file_mode", p, std::fs::metadata(p))?
            .permissions()
            .mode()
            & 0o7777) as i64)
    }
    #[cfg(not(unix))]
    {
        io("file_mode", p, std::fs::metadata(p))?;
        Ok(-1)
    }
}

pub fn set_file_mode(p: &str, mode: i64) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        io(
            "set_file_mode",
            p,
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode as u32)),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (p, mode);
        Err("set_file_mode is only available on Unix-like systems; use set_readonly".to_string())
    }
}

pub fn temp_dir() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

pub fn home_dir() -> Option<String> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var(var).ok()
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

static NEXT_PROCESS: AtomicI64 = AtomicI64::new(1);

fn processes() -> &'static Mutex<HashMap<i64, Child>> {
    static PROCESSES: OnceLock<Mutex<HashMap<i64, Child>>> = OnceLock::new();
    PROCESSES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct ProcessResult {
    pub status: i64,
    pub stdout: String,
    pub stderr: String,
}

fn result_of(out: std::process::Output) -> ProcessResult {
    ProcessResult {
        status: out.status.code().unwrap_or(-1) as i64,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Runs a program to completion and captures its output.
pub fn process_output(
    program: &str,
    args: &[String],
    input: Option<&str>,
) -> Result<ProcessResult, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start '{}': {}", program, e))?;
    if let Some(text) = input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
    }
    child
        .wait_with_output()
        .map(result_of)
        .map_err(|e| format!("waiting for '{}': {}", program, e))
}

/// Starts a program with piped stdin/stdout/stderr and returns a process handle.
pub fn process_spawn(program: &str, args: &[String]) -> Result<i64, String> {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start '{}': {}", program, e))?;
    let id = NEXT_PROCESS.fetch_add(1, Ordering::SeqCst);
    processes().lock().unwrap().insert(id, child);
    Ok(id)
}

pub fn process_write(id: i64, text: &str) -> Result<(), String> {
    let mut map = processes().lock().unwrap();
    let child = map
        .get_mut(&id)
        .ok_or_else(|| format!("process {} does not exist", id))?;
    let stdin = child.stdin.as_mut().ok_or("process stdin is closed")?;
    stdin
        .write_all(text.as_bytes())
        .map_err(|e| format!("process_write: {}", e))
}

pub fn process_close_stdin(id: i64) -> Result<(), String> {
    let mut map = processes().lock().unwrap();
    let child = map
        .get_mut(&id)
        .ok_or_else(|| format!("process {} does not exist", id))?;
    child.stdin.take();
    Ok(())
}

/// Non-blocking check: `Some(exit code)` if the process has finished.
pub fn process_try_wait(id: i64) -> Result<Option<i64>, String> {
    let mut map = processes().lock().unwrap();
    let child = map
        .get_mut(&id)
        .ok_or_else(|| format!("process {} does not exist", id))?;
    let status = child
        .try_wait()
        .map_err(|e| format!("process_try_wait: {}", e))?;
    Ok(status.map(|s| s.code().unwrap_or(-1) as i64))
}

pub fn process_kill(id: i64) -> Result<(), String> {
    let mut map = processes().lock().unwrap();
    let child = map
        .get_mut(&id)
        .ok_or_else(|| format!("process {} does not exist", id))?;
    match child.kill() {
        Ok(()) => Ok(()),
        // Already exited.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
        Err(e) => Err(format!("process_kill: {}", e)),
    }
}

/// Closes stdin, waits for the process to finish and returns its exit code and output.
pub fn process_wait(id: i64) -> Result<ProcessResult, String> {
    let child = processes().lock().unwrap().remove(&id);
    let mut child =
        child.ok_or_else(|| format!("process {} does not exist or was already waited for", id))?;
    child.stdin.take();
    child
        .wait_with_output()
        .map(result_of)
        .map_err(|e| format!("process_wait: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_functions() {
        assert_eq!(path_file_name("a/b/c.txt").as_deref(), Some("c.txt"));
        assert_eq!(path_extension("a/b/c.tar.gz").as_deref(), Some("gz"));
        assert_eq!(path_stem("a/b/c.txt").as_deref(), Some("c"));
        assert_eq!(path_parent("a/b/c.txt").as_deref(), Some("a/b"));
        assert_eq!(
            path_normalize("a/./b/../c"),
            Path::new("a").join("c").to_string_lossy()
        );
        assert_eq!(
            path_normalize("../x"),
            Path::new("..").join("x").to_string_lossy()
        );
        assert_eq!(path_normalize("."), ".");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fusion_os_ext_{}_{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn file_operations() {
        let dir = scratch("files");
        let a = dir.join("a.txt").to_string_lossy().into_owned();
        let b = dir.join("sub").join("b.txt").to_string_lossy().into_owned();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        append_file(&a, "one\n").unwrap();
        append_file(&a, "two\n").unwrap();
        assert_eq!(read_lines(&a).unwrap(), vec!["one", "two"]);
        assert_eq!(file_size(&a).unwrap(), 8);
        copy_file(&a, &b).unwrap();
        let walked = walk_dir(&dir.to_string_lossy()).unwrap();
        assert_eq!(walked.len(), 3);
        set_readonly(&a, true).unwrap();
        assert!(is_readonly(&a).unwrap());
        set_readonly(&a, false).unwrap();
        assert!(!is_readonly(&a).unwrap());
        rename_path(&a, &dir.join("c.txt").to_string_lossy()).unwrap();
        assert!(!is_file(&a));
        remove_dir(&dir.to_string_lossy()).unwrap();
        assert!(!is_dir(&dir.to_string_lossy()));
    }

    #[test]
    fn processes_capture_output() {
        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            ("cmd", vec!["/C".into(), "echo hello".into()])
        } else {
            ("sh", vec!["-c".into(), "echo hello".into()])
        };
        let r = process_output(&program, &args, None).unwrap();
        assert_eq!(r.status, 0);
        assert_eq!(r.stdout.trim(), "hello");
        assert!(process_output("definitely-not-a-real-program-xyz", &[], None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn process_pipes_and_kill() {
        let id = process_spawn("cat", &[]).unwrap();
        process_write(id, "ping").unwrap();
        let r = process_wait(id).unwrap();
        assert_eq!(r.stdout, "ping");
        let sleeper = process_spawn("sleep", &["30".to_string()]).unwrap();
        assert_eq!(process_try_wait(sleeper).unwrap(), None);
        process_kill(sleeper).unwrap();
        let r = process_wait(sleeper).unwrap();
        assert_ne!(r.status, 0);
    }
}
