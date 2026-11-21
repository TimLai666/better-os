//! The test guard: one switch that keeps a test or a sandbox run away from
//! the person's own session.
//!
//! [`GUARD_VARIABLE`] is a flag. Any non-empty value turns it on, and then
//! nothing that would reach the person's user manager, shell settings,
//! removal hooks or installed programs is run (ticket 178). The flag writes
//! nothing by itself; it used to be read as a log file path in some places
//! and as a flag in others, so `=1` left a file named `1` in whatever folder
//! a window was started from.
//!
//! A test that wants to see what was held back names a file in
//! [`LOG_VARIABLE`]: each command the guard stopped is appended to it, one
//! line each. Only an absolute path is accepted; a relative one is refused
//! with a message rather than resolved against the working directory.
//!
//! Every caller reads the guard through [`TestGuard::from_environment`] or
//! [`is_on`]; nothing else reads either variable.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set to any non-empty value, it turns the guard on.
pub const GUARD_VARIABLE: &str = "BETTER_OS_TEST_SYSTEMCTL_GUARD";

/// An absolute file path the guard appends each command it held back to.
/// Read only while [`GUARD_VARIABLE`] is on.
pub const LOG_VARIABLE: &str = "BETTER_OS_TEST_GUARD_LOG";

/// Where the commands the guard held back are written.
#[derive(Clone, Debug, Eq, PartialEq)]
enum GuardLog {
    /// Nowhere: the flag alone writes nothing.
    None,
    /// Appended to this absolute path.
    File(PathBuf),
    /// [`LOG_VARIABLE`] named something that is not an absolute path, and
    /// this says so.
    Refused(String),
}

/// The guard, on. A caller holds `Option<TestGuard>`: `None` is off.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestGuard {
    log: GuardLog,
}

/// Whether the guard is on in this process's environment.
pub fn is_on() -> bool {
    flag_is_on(std::env::var_os(GUARD_VARIABLE).as_deref())
}

fn flag_is_on(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

impl TestGuard {
    /// The guard as this process's environment sets it, or `None` when it is
    /// off.
    pub fn from_environment() -> Option<Self> {
        Self::from_values(
            std::env::var_os(GUARD_VARIABLE).as_deref(),
            std::env::var_os(LOG_VARIABLE).as_deref(),
        )
    }

    /// The guard the two variables' values describe, for a test that does not
    /// change its own environment.
    pub fn from_values(flag: Option<&OsStr>, log: Option<&OsStr>) -> Option<Self> {
        if !flag_is_on(flag) {
            return None;
        }
        let log = match log.filter(|value| !value.is_empty()) {
            None => GuardLog::None,
            Some(value) => {
                let path = Path::new(value);
                if path.is_absolute() {
                    GuardLog::File(path.to_path_buf())
                } else {
                    GuardLog::Refused(format!(
                        "{LOG_VARIABLE} must be an absolute path; `{}` is not, so nothing is recorded",
                        path.display()
                    ))
                }
            }
        };
        Some(Self { log })
    }

    /// A guard that records to `log`, which must be absolute.
    pub fn recording_to(log: impl AsRef<Path>) -> Self {
        Self::from_values(Some(OsStr::new("1")), Some(log.as_ref().as_os_str()))
            .expect("the flag is on")
    }

    /// A guard that records nothing.
    pub fn silent() -> Self {
        Self {
            log: GuardLog::None,
        }
    }

    /// The file the guard records to, when there is one.
    pub fn log(&self) -> Option<&Path> {
        match &self.log {
            GuardLog::File(path) => Some(path),
            _ => None,
        }
    }

    /// Why [`LOG_VARIABLE`] was not used, when it named a relative path.
    pub fn refusal(&self) -> Option<&str> {
        match &self.log {
            GuardLog::Refused(message) => Some(message),
            _ => None,
        }
    }

    /// The words for something the guard did not run: "`what` is not run
    /// while … is set", with the log's refusal after it when there is one.
    pub fn reason(&self, what: &str) -> String {
        let mut reason = format!("{what} is not run while {GUARD_VARIABLE} is set");
        if let Some(refusal) = self.refusal() {
            reason.push_str(" (");
            reason.push_str(refusal);
            reason.push(')');
        }
        reason
    }

    /// Records one command the guard held back. Writes only to the log file
    /// [`LOG_VARIABLE`] names; a refused value is said once on stderr.
    pub fn record(&self, line: &str) {
        match &self.log {
            GuardLog::None => {}
            GuardLog::File(path) => {
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut file| writeln!(file, "{line}"));
            }
            GuardLog::Refused(message) => {
                static SAID: AtomicBool = AtomicBool::new(false);
                if !SAID.swap(true, Ordering::Relaxed) {
                    eprintln!("{message}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(flag: Option<&str>, log: Option<&str>) -> Option<TestGuard> {
        TestGuard::from_values(flag.map(OsStr::new), log.map(OsStr::new))
    }

    #[test]
    fn any_non_empty_value_turns_the_guard_on_and_empty_or_unset_does_not() {
        assert!(values(Some("1"), None).is_some());
        assert!(values(Some("yes"), None).is_some());
        assert!(values(Some("/tmp/anything"), None).is_some());
        assert!(values(Some(""), None).is_none());
        assert!(values(None, None).is_none());
        // The log alone is not the guard.
        assert!(values(None, Some("/tmp/log")).is_none());
    }

    #[test]
    fn the_flag_alone_writes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        // The value the old reading took as a file name.
        let guard = values(Some("1"), None).unwrap();
        // A relative file would land in the working directory; the guard
        // must write nowhere at all, so look at the directory a relative
        // `1` would have been resolved against as well as the scratch one.
        let before = std::path::Path::new("1").exists();
        guard.record("systemctl --user daemon-reload");
        assert_eq!(guard.log(), None);
        assert_eq!(std::path::Path::new("1").exists(), before);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_log_variable_writes_where_it_names() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("held-back.log");
        let guard = values(Some("1"), Some(log.to_str().unwrap())).unwrap();

        guard.record("systemctl --user daemon-reload");
        guard.record("gsettings get org.gnome.shell enabled-extensions");

        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "systemctl --user daemon-reload\ngsettings get org.gnome.shell enabled-extensions\n"
        );
        assert_eq!(guard.refusal(), None);
    }

    #[test]
    fn a_relative_log_path_is_refused_with_a_message() {
        let guard = values(Some("1"), Some("held-back.log")).unwrap();

        assert_eq!(guard.log(), None);
        let refusal = guard.refusal().expect("a relative path is refused");
        assert!(refusal.contains(LOG_VARIABLE), "{refusal}");
        assert!(refusal.contains("absolute"), "{refusal}");
        assert!(refusal.contains("held-back.log"), "{refusal}");
        let reason = guard.reason("systemctl");
        assert!(reason.starts_with("systemctl is not run while"), "{reason}");
        assert!(reason.contains(refusal), "{reason}");

        guard.record("systemctl --user daemon-reload");
        assert!(!std::path::Path::new("held-back.log").exists());
    }
}
