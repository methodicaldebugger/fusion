//contents of process.rs

use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, ExitStatus, Stdio},
};
use thiserror::Error;

#[derive(Debug)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("could not start '{program}': {source}")]
    Start {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("'{program}' exited unsuccessfully: {status}\n{stderr}")]
    Failed {
        program: String,
        status: String,
        stderr: String,
    },
}

pub fn run_capture<I, S>(
    program: &OsStr,
    args: I,
    cwd: &Path,
) -> Result<CommandOutput, ProcessError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(program);

    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = command.output().map_err(|source| ProcessError::Start {
        program: program.to_string_lossy().into_owned(),
        source,
    })?;

    let result = CommandOutput {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    };

    if !result.status.success() {
        return Err(ProcessError::Failed {
            program: program.to_string_lossy().into_owned(),
            status: result.status.to_string(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }

    Ok(result)
}
