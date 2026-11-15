//! The program a package ships for Better Manager to run, as the person,
//! before the package is removed (ticket 88).
//!
//! A component that changed something in the person's session that dpkg cannot
//! see, such as a desktop setting it wrote, puts it back from here: nothing it
//! set may outlive it (AGENTS.md). A maintainer script cannot do this, because
//! it runs as root with no session. The program is found the way units are,
//! in dpkg's own file list: exactly
//! `/usr/lib/better-os/before-removal/<package>`, never a path a manifest
//! supplied. It is run with no arguments and a timeout, and a test environment
//! that sets [`SYSTEMCTL_GUARD_VARIABLE`] runs nothing.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use better_core::package_health::InstalledFiles;

use crate::user_units::{SYSTEMCTL_GUARD_VARIABLE, run_with_timeout};

pub const BEFORE_REMOVAL_DIRECTORY: &str = "/usr/lib/better-os/before-removal";

/// How long the program may take.
pub const BEFORE_REMOVAL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookFailure {
    /// No session to run it in (the guard, or no session bus).
    pub no_session: bool,
    pub message: String,
}

/// Runs one before-removal program.
pub trait RemovalHookRunner: Send + Sync {
    fn run(&self, program: &Path) -> Result<(), HookFailure>;
}

/// The package's before-removal program, when its file list has one.
pub fn before_removal_hook(package: &str, files: &InstalledFiles) -> Option<PathBuf> {
    let wanted = Path::new(BEFORE_REMOVAL_DIRECTORY).join(package);
    files.paths.iter().find(|path| **path == wanted).cloned()
}

/// Runs the program as a child of this process, which is the person's.
#[derive(Clone, Debug)]
pub struct ProcessHookRunner {
    timeout: Duration,
    guard: Option<PathBuf>,
}

impl ProcessHookRunner {
    pub fn from_environment() -> Self {
        Self {
            timeout: BEFORE_REMOVAL_TIMEOUT,
            guard: std::env::var_os(SYSTEMCTL_GUARD_VARIABLE)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        }
    }
}

impl RemovalHookRunner for ProcessHookRunner {
    fn run(&self, program: &Path) -> Result<(), HookFailure> {
        if let Some(log) = &self.guard {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .and_then(|mut file| writeln!(file, "{}", program.display()));
            return Err(HookFailure {
                no_session: true,
                message: format!(
                    "{} is not run while {SYSTEMCTL_GUARD_VARIABLE} is set",
                    program.display()
                ),
            });
        }
        let mut command = Command::new(program);
        command.env("LC_ALL", "C");
        let output = match run_with_timeout(command, self.timeout) {
            Ok(Some(output)) => output,
            Ok(None) => {
                return Err(HookFailure {
                    no_session: false,
                    message: format!(
                        "{} did not finish within {} seconds",
                        program.display(),
                        self.timeout.as_secs()
                    ),
                });
            }
            Err(error) => {
                return Err(HookFailure {
                    no_session: false,
                    message: format!("{} could not be run: {error}", program.display()),
                });
            }
        };
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("exited with {}", output.status));
        Err(HookFailure {
            no_session: false,
            message: format!("{}: {message}", program.display()),
        })
    }
}

/// Records each program it was asked to run and answers as told, for tests.
#[derive(Debug, Default)]
pub struct FakeHookRunner {
    pub failure: Option<HookFailure>,
    pub ran: std::sync::Mutex<Vec<PathBuf>>,
}

impl FakeHookRunner {
    pub fn ran(&self) -> Vec<PathBuf> {
        self.ran.lock().expect("not poisoned").clone()
    }
}

impl RemovalHookRunner for FakeHookRunner {
    fn run(&self, program: &Path) -> Result<(), HookFailure> {
        self.ran
            .lock()
            .expect("not poisoned")
            .push(program.to_path_buf());
        match &self.failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> InstalledFiles {
        InstalledFiles {
            paths: paths.iter().map(PathBuf::from).collect(),
            ..InstalledFiles::default()
        }
    }

    #[test]
    fn only_the_package_s_own_exact_path_counts() {
        let list = files(&[
            "/usr/lib/better-os/before-removal",
            "/usr/lib/better-os/before-removal/better-git",
            "/usr/lib/better-os/before-removal/better-touchpad",
            "/usr/lib/better-os/before-removal/better-touchpad/x",
        ]);
        assert_eq!(
            before_removal_hook("better-touchpad", &list),
            Some(PathBuf::from(
                "/usr/lib/better-os/before-removal/better-touchpad"
            ))
        );
        assert_eq!(before_removal_hook("better-awake", &list), None);
    }

    #[test]
    fn the_guard_runs_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("log");
        let runner = ProcessHookRunner {
            timeout: BEFORE_REMOVAL_TIMEOUT,
            guard: Some(log.clone()),
        };
        let failure = runner.run(Path::new("/bin/false")).unwrap_err();
        assert!(failure.no_session);
        assert!(std::fs::read_to_string(log).unwrap().contains("/bin/false"));
    }

    #[test]
    fn a_failing_program_says_why() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("hook");
        std::fs::write(&program, "#!/bin/sh\necho nope >&2\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runner = ProcessHookRunner {
            timeout: BEFORE_REMOVAL_TIMEOUT,
            guard: None,
        };
        let failure = runner.run(&program).unwrap_err();
        assert!(!failure.no_session);
        assert!(failure.message.ends_with("nope"), "{}", failure.message);
    }
}
