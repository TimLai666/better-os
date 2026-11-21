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
//! that sets [`better_core::test_guard::GUARD_VARIABLE`] runs nothing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use better_core::package_health::InstalledFiles;
use better_core::test_guard::TestGuard;

use crate::user_units::run_with_timeout;

pub const BEFORE_REMOVAL_DIRECTORY: &str = "/usr/lib/better-os/before-removal";

/// How long the program may take.
pub const BEFORE_REMOVAL_TIMEOUT: Duration = Duration::from_secs(30);

/// What a before-removal program did. None of it stops a removal: removing
/// a component must always be possible (review of ticket 88), so what could
/// not be put back is said, not enforced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HookOutcome {
    /// Exit 0: put back, or nothing to put back.
    Restored,
    /// Exit 3, any other failure, or a program that was not run: the settings
    /// stay as the component left them, and the person is told why.
    NotRestored(String),
    /// Exit 4: no session to write in right now; the program left a pending
    /// restore that Better Manager finishes at the person's next start.
    Deferred,
}

/// The program's exit status for "could not put the settings back".
pub const EXIT_NOT_RESTORED: i32 = 3;
/// The program's exit status for "deferred to the next login".
pub const EXIT_DEFERRED: i32 = 4;
/// The one argument a program is ever given: undo the run before it, because
/// the removal it prepared for did not happen.
pub const UNDO_ARGUMENT: &str = "--undo";

/// Runs one before-removal program, or its undo.
pub trait RemovalHookRunner: Send + Sync {
    fn run(&self, program: &Path, undo: bool) -> HookOutcome;
}

/// The package's before-removal program, when its file list has one.
pub fn before_removal_hook(package: &str, files: &InstalledFiles) -> Option<PathBuf> {
    let wanted = Path::new(BEFORE_REMOVAL_DIRECTORY).join(package);
    files.paths.iter().find(|path| **path == wanted).cloned()
}

/// Why `program` must not be run, if it must not: anything but a regular
/// file (a link included) owned by `owner` that neither group nor others may
/// write. The program runs as the person, from a path dpkg recorded, but a
/// file someone else could have replaced is not dpkg's file any more.
pub fn refuse_program(program: &Path, owner: u32) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = match std::fs::symlink_metadata(program) {
        Ok(metadata) => metadata,
        Err(error) => return Some(format!("{} could not be read: {error}", program.display())),
    };
    if !metadata.file_type().is_file() {
        return Some(format!("{} is not a regular file", program.display()));
    }
    if metadata.uid() != owner {
        return Some(format!("{} is not owned by root", program.display()));
    }
    if metadata.mode() & 0o022 != 0 {
        return Some(format!("{} can be changed by others", program.display()));
    }
    None
}

/// Runs the program as a child of this process, which is the person's.
#[derive(Clone, Debug)]
pub struct ProcessHookRunner {
    timeout: Duration,
    guard: Option<TestGuard>,
    owner: u32,
}

impl ProcessHookRunner {
    pub fn from_environment() -> Self {
        Self {
            timeout: BEFORE_REMOVAL_TIMEOUT,
            guard: TestGuard::from_environment(),
            owner: 0,
        }
    }

    /// A runner that accepts programs owned by `owner` rather than root, for
    /// a test that cannot make a root-owned file.
    pub fn owned_by(owner: u32) -> Self {
        Self {
            timeout: BEFORE_REMOVAL_TIMEOUT,
            guard: None,
            owner,
        }
    }
}

impl RemovalHookRunner for ProcessHookRunner {
    fn run(&self, program: &Path, undo: bool) -> HookOutcome {
        if let Some(guard) = &self.guard {
            guard.record(&program.display().to_string());
            return HookOutcome::NotRestored(guard.reason(&program.display().to_string()));
        }
        if let Some(reason) = refuse_program(program, self.owner) {
            return HookOutcome::NotRestored(format!("not run: {reason}"));
        }
        let mut command = Command::new(program);
        command.env("LC_ALL", "C");
        if undo {
            command.arg(UNDO_ARGUMENT);
        }
        let output = match run_with_timeout(command, self.timeout) {
            Ok(Some(output)) => output,
            Ok(None) => {
                return HookOutcome::NotRestored(format!(
                    "{} did not finish within {} seconds",
                    program.display(),
                    self.timeout.as_secs()
                ));
            }
            Err(error) => {
                return HookOutcome::NotRestored(format!(
                    "{} could not be run: {error}",
                    program.display()
                ));
            }
        };
        let stderr = String::from_utf8_lossy(&output.stderr);
        let said = stderr
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("exited with {}", output.status));
        outcome_for(output.status.code(), said)
    }
}

/// The outcome an exit status means. Anything but 0 and 4 is "not put back",
/// a signal included.
pub fn outcome_for(code: Option<i32>, said: String) -> HookOutcome {
    match code {
        Some(0) => HookOutcome::Restored,
        Some(EXIT_DEFERRED) => HookOutcome::Deferred,
        _ => HookOutcome::NotRestored(said),
    }
}

/// Records each program it was asked to run and answers as told, for tests.
#[derive(Debug)]
pub struct FakeHookRunner {
    pub outcome: HookOutcome,
    pub ran: std::sync::Mutex<Vec<(PathBuf, bool)>>,
}

impl Default for FakeHookRunner {
    fn default() -> Self {
        Self::answering(HookOutcome::Restored)
    }
}

impl FakeHookRunner {
    pub fn answering(outcome: HookOutcome) -> Self {
        Self {
            outcome,
            ran: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Every run, with whether it was the undo.
    pub fn ran(&self) -> Vec<(PathBuf, bool)> {
        self.ran.lock().expect("not poisoned").clone()
    }
}

impl RemovalHookRunner for FakeHookRunner {
    fn run(&self, program: &Path, undo: bool) -> HookOutcome {
        self.ran
            .lock()
            .expect("not poisoned")
            .push((program.to_path_buf(), undo));
        if undo {
            HookOutcome::Restored
        } else {
            self.outcome.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn files(paths: &[&str]) -> InstalledFiles {
        InstalledFiles {
            paths: paths.iter().map(PathBuf::from).collect(),
            ..InstalledFiles::default()
        }
    }

    fn me() -> u32 {
        use std::os::unix::fs::MetadataExt;
        tempfile::tempdir()
            .unwrap()
            .path()
            .metadata()
            .unwrap()
            .uid()
    }

    /// Writes a file to check, never to run (AGENTS.md: a test does not
    /// execute a file it has just written).
    fn script(directory: &Path, body: &str) -> PathBuf {
        let path = directory.join("hook");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
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
            guard: Some(TestGuard::recording_to(&log)),
            owner: 0,
        };
        assert!(matches!(
            runner.run(Path::new("/bin/false"), false),
            HookOutcome::NotRestored(_)
        ));
        assert!(std::fs::read_to_string(log).unwrap().contains("/bin/false"));
    }

    #[test]
    fn a_program_someone_else_could_change_is_not_run() {
        let directory = tempfile::tempdir().unwrap();
        let program = script(directory.path(), "exit 0");
        // Not root's: refused under the product's owner rule.
        assert!(
            refuse_program(&program, 0)
                .unwrap()
                .contains("not owned by root")
        );
        // Writable by others: refused even for its owner.
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o757)).unwrap();
        assert!(
            refuse_program(&program, me())
                .unwrap()
                .contains("changed by others")
        );
        // A link to an acceptable file is still a link.
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&program, &link).unwrap();
        assert!(
            refuse_program(&link, me())
                .unwrap()
                .contains("not a regular file")
        );
        assert_eq!(refuse_program(&program, me()), None);
    }

    /// Better Touchpad's program cannot depend on this crate, so its copy of
    /// the exit statuses is held to this one by its source.
    #[test]
    fn the_touchpad_program_speaks_the_same_exit_statuses() {
        let source = include_str!("../../touchpad-platform/src/bin/before_removal.rs");
        assert!(source.contains(&format!("const NOT_RESTORED: u8 = {EXIT_NOT_RESTORED};")));
        assert!(source.contains(&format!("const DEFERRED: u8 = {EXIT_DEFERRED};")));
        assert!(source.contains(&format!("\"{UNDO_ARGUMENT}\"")));
    }

    #[test]
    fn exit_statuses_map_to_outcomes() {
        assert_eq!(outcome_for(Some(0), String::new()), HookOutcome::Restored);
        assert_eq!(outcome_for(Some(4), String::new()), HookOutcome::Deferred);
        for code in [Some(1), Some(3), Some(2), None] {
            assert_eq!(
                outcome_for(code, "why".into()),
                HookOutcome::NotRestored("why".into())
            );
        }
    }

    #[test]
    fn root_owned_system_programs_run_and_report() {
        let runner = ProcessHookRunner {
            timeout: BEFORE_REMOVAL_TIMEOUT,
            guard: None,
            owner: 0,
        };
        assert_eq!(
            runner.run(Path::new("/bin/true"), false),
            HookOutcome::Restored
        );
        assert!(matches!(
            runner.run(Path::new("/bin/false"), true),
            HookOutcome::NotRestored(_)
        ));
    }
}
