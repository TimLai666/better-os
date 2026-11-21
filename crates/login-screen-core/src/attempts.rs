//! Failed passwords in a row, per account, and the wait they impose
//! (design/login-screen-spec.md section 3.9; the security review's "What an
//! attacker gains").
//!
//! The doubling wait is the longest limit on guessing at the login screen
//! (Ubuntu's `/etc/pam.d` has no `pam_faillock`), so it is kept per user name
//! and survives everything a person at the keyboard can do: switching
//! account and back, choosing "Other account…" and typing the same name, and
//! a restart of the program. Each program keeps the record in its own runtime
//! directory, written only by it:
//!
//! - the greeter: [`GREETER_RECORD`], under `/run/better-os-greeter/`, owned
//!   by `_better-greeter` with mode 0700 (ticket 264 creates the directory);
//! - the lock program: [`LOCK_RECORD_NAME`] under the person's
//!   `$XDG_RUNTIME_DIR/better-os/` (0700, ticket 262).
//!
//! `/run` is emptied at boot, so the times are `CLOCK_BOOTTIME` milliseconds
//! ([`crate::view::boot_time_now`]), which keep counting through sleep. The
//! person's own programs can delete the lock program's record, as they can
//! unlock the session through logind anyway (the threat model); the record
//! holds account names and counts, never anything typed.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::view::Millis;
use crate::wait::wait_after_failures;

/// The greeter's record.
pub const GREETER_RECORD: &str = "/run/better-os-greeter/attempts.json";
/// The lock program's record, inside `$XDG_RUNTIME_DIR/better-os/`.
pub const LOCK_RECORD_NAME: &str = "lock-attempts.json";
/// The most accounts kept. "Other account…" takes any name, so a record
/// that grew without bound would let anyone at the keyboard fill the disk;
/// past this the entry whose wait ended longest ago goes.
pub const MAX_ACCOUNTS: usize = 64;
/// The largest record read.
pub const MAX_RECORD_BYTES: u64 = 64 * 1024;

/// One account's failures in a row and when its wait ends.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Attempt {
    pub failures: u32,
    /// `CLOCK_BOOTTIME` milliseconds; 0 for no wait.
    pub wait_until: Millis,
}

/// Every account's [`Attempt`], by user name.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Attempts {
    accounts: BTreeMap<String, Attempt>,
}

impl Attempts {
    pub fn get(&self, user: &str) -> Attempt {
        self.accounts.get(user).copied().unwrap_or_default()
    }

    /// The end of the user's wait, when it is still running at `now`.
    pub fn waiting_until(&self, user: &str, now: Millis) -> Option<Millis> {
        Some(self.get(user).wait_until).filter(|until| *until > now)
    }

    /// Counts one more failure and starts its wait; returns the attempt.
    pub fn record_failure(&mut self, user: &str, now: Millis) -> Attempt {
        let mut attempt = self.get(user);
        attempt.failures = attempt.failures.saturating_add(1);
        let wait = wait_after_failures(attempt.failures);
        attempt.wait_until = if wait.is_zero() {
            0
        } else {
            now.saturating_add(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX))
        };
        self.accounts.insert(user.to_string(), attempt);
        self.trim();
        attempt
    }

    /// A success ends the row.
    pub fn record_success(&mut self, user: &str) {
        self.accounts.remove(user);
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    fn trim(&mut self) {
        while self.accounts.len() > MAX_ACCOUNTS {
            let oldest = self
                .accounts
                .iter()
                .min_by_key(|(name, attempt)| {
                    (attempt.wait_until, attempt.failures, (*name).clone())
                })
                .map(|(name, _)| name.clone());
            match oldest {
                Some(name) => {
                    self.accounts.remove(&name);
                }
                None => break,
            }
        }
    }

    /// Reads a record. A missing file is an empty record; one that cannot be
    /// read or parsed is an error, which the program logs and then starts
    /// from an empty record (it holds no secret, and refusing every login
    /// over a damaged file would lock everyone out).
    pub fn load(path: &Path) -> Result<Self, AttemptsError> {
        use std::io::Read;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(AttemptsError::Io(error)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(AttemptsError::TooLarge);
        }
        let mut attempts: Self = serde_json::from_slice(&bytes)?;
        attempts.trim();
        Ok(attempts)
    }

    /// Writes the record: a new file beside it, created with mode 0600 and
    /// without following a link, then renamed over the old one.
    pub fn save(&self, path: &Path) -> Result<(), AttemptsError> {
        use std::os::unix::fs::OpenOptionsExt;
        let directory = path.parent().ok_or(AttemptsError::NoDirectory)?;
        let name = path.file_name().ok_or(AttemptsError::NoDirectory)?;
        let temporary = directory.join(format!(".{}.new", name.to_string_lossy()));
        let _ = std::fs::remove_file(&temporary);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AttemptsError {
    #[error("the failure record could not be read or written: {0}")]
    Io(#[from] std::io::Error),
    #[error("the failure record is longer than {MAX_RECORD_BYTES} bytes")]
    TooLarge,
    #[error("the failure record is not one this program wrote: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("the failure record's path has no directory")]
    NoDirectory,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Millis = 1000;

    #[test]
    fn failures_count_per_account() {
        let mut attempts = Attempts::default();
        assert_eq!(attempts.record_failure("lin", 0).wait_until, 0);
        assert_eq!(attempts.record_failure("lin", 0).wait_until, SECOND);
        assert_eq!(attempts.record_failure("wang", 0).failures, 1);
        assert_eq!(attempts.waiting_until("lin", 0), Some(SECOND));
        assert_eq!(attempts.waiting_until("lin", SECOND), None);
        assert_eq!(attempts.waiting_until("wang", 0), None);
        assert_eq!(
            attempts.record_failure("lin", 5 * SECOND).wait_until,
            7 * SECOND
        );
        attempts.record_success("lin");
        assert_eq!(attempts.get("lin"), Attempt::default());
        assert_eq!(attempts.get("wang").failures, 1);
    }

    #[test]
    fn the_record_is_bounded() {
        let mut attempts = Attempts::default();
        for index in 0..(MAX_ACCOUNTS + 10) {
            attempts.record_failure(&format!("user{index}"), index as Millis);
        }
        assert_eq!(attempts.len(), MAX_ACCOUNTS);
    }

    #[test]
    fn the_record_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        assert!(Attempts::load(&path).unwrap().is_empty());
        let mut attempts = Attempts::default();
        attempts.record_failure("lin", 0);
        attempts.record_failure("lin", 0);
        attempts.save(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let loaded = Attempts::load(&path).unwrap();
        assert_eq!(loaded, attempts);
        assert_eq!(loaded.waiting_until("lin", 0), Some(SECOND));
        // Saving again replaces it.
        let mut later = loaded.clone();
        later.record_success("lin");
        later.save(&path).unwrap();
        assert!(Attempts::load(&path).unwrap().is_empty());
    }

    #[test]
    fn a_damaged_or_huge_record_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, b"{not json").unwrap();
        assert!(matches!(
            Attempts::load(&bad),
            Err(AttemptsError::Malformed(_))
        ));
        let huge = dir.path().join("huge.json");
        std::fs::write(&huge, vec![b' '; MAX_RECORD_BYTES as usize + 1]).unwrap();
        assert!(matches!(
            Attempts::load(&huge),
            Err(AttemptsError::TooLarge)
        ));
    }

    #[test]
    fn saving_does_not_follow_a_planted_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, b"keep").unwrap();
        let path = dir.path().join("attempts.json");
        std::os::unix::fs::symlink(&target, dir.path().join(".attempts.json.new")).unwrap();
        Attempts::default().save(&path).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    }
}
