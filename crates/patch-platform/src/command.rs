//! Running a `patch_core::Command`. This module is the only place in
//! Better Patch that starts a process, and it starts only the program and
//! arguments the command itself names: an absolute path from a closed list
//! and fixed arguments, never a shell.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use patch_core::Command;

/// What a finished command printed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Output {
    /// It exited with status 0.
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Why a command did not run to the end.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunError {
    NotInstalled(&'static str),
    TimedOut,
    Failed(String),
}

/// What runs commands. Tests and fixtures replace it.
pub trait CommandRunner: Send {
    fn run(&self, command: &Command) -> Result<Output, RunError>;
}

/// Runs the real programs.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCommands;

/// How often a running command is looked at.
const POLL: Duration = Duration::from_millis(20);

fn drain(mut pipe: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

impl CommandRunner for SystemCommands {
    fn run(&self, command: &Command) -> Result<Output, RunError> {
        let program: &'static str = command.program();
        if !Path::new(program).is_file() {
            return Err(RunError::NotInstalled(program));
        }
        let mut child = std::process::Command::new(program)
            .args(command.args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => RunError::NotInstalled(program),
                _ => RunError::Failed(format!("{program}: {error}")),
            })?;
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        let deadline = Instant::now() + command.timeout();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::TimedOut);
                }
                Ok(None) => std::thread::sleep(POLL),
                Err(error) => return Err(RunError::Failed(format!("{program}: {error}"))),
            }
        };
        let collect = |pipe: Option<std::thread::JoinHandle<String>>| {
            pipe.and_then(|h| h.join().ok()).unwrap_or_default()
        };
        Ok(Output {
            success: status.success(),
            stdout: collect(stdout),
            stderr: collect(stderr),
        })
    }
}

impl Output {
    /// Why a command that exited unsuccessfully failed, in its own words.
    pub fn failure(&self, program: &str) -> String {
        let said = self.stderr.trim();
        if said.is_empty() {
            format!("{program} did not succeed")
        } else {
            format!("{program}: {said}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_names_the_program_and_what_it_said() {
        let output = Output {
            success: false,
            stdout: String::new(),
            stderr: "error: nothing\n".into(),
        };
        assert_eq!(
            output.failure("/usr/bin/flatpak"),
            "/usr/bin/flatpak: error: nothing"
        );
        let silent = Output {
            success: false,
            stdout: String::new(),
            stderr: String::new(),
        };
        assert_eq!(
            silent.failure("/usr/bin/fc-cache"),
            "/usr/bin/fc-cache did not succeed"
        );
    }
}
