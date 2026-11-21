//! Failed passwords in a row, per account, and the wait they impose
//! (design/login-screen-spec.md section 3.9; the security review's "What an
//! attacker gains").
//!
//! The doubling wait is the longest limit on guessing at the login screen
//! (Ubuntu's `/etc/pam.d` has no `pam_faillock`), so it is kept per account
//! and survives everything a person at the keyboard can do: switching
//! account and back, choosing "Other account…" and typing the same name (in
//! any case the system's name service treats as the same account), and a
//! restart of the program.
//!
//! **What is kept, and where.** A name is counted under the account it
//! resolves to ([`AccountResolver`], the account database's own `pw_name`),
//! so `Tim` and `tim` share one count where they are one account. Only
//! resolved accounts are written to the record; a name that resolves to no
//! account is counted in memory only, because it may be anything a person
//! typed into the name field, a password among them, and the record must
//! never hold typed text. In memory both kinds wait alike, so the screen
//! shows no difference between a name that exists and one that does not.
//! The record is in each program's own runtime directory, written only by it:
//!
//! - the greeter: [`GREETER_RECORD`], under `/run/better-os-greeter/`, owned
//!   by `_better-greeter` with mode 0700 (ticket 264 creates the directory so
//!   that it survives a restart of the greeter's unit);
//! - the lock program: [`LOCK_RECORD_NAME`] under the person's
//!   `$XDG_RUNTIME_DIR/better-os/` (0700, ticket 262).
//!
//! `/run` is emptied at boot, so the times are `CLOCK_BOOTTIME` milliseconds
//! ([`crate::view::boot_time_now`]), which keep counting through sleep. A
//! record read back is held to what this program could have written: no
//! wait longer than the longest from now, no count beyond [`MAX_FAILURES`],
//! no name longer than [`LOGIN_NAME_MAX`], at most [`MAX_ACCOUNTS`] entries,
//! and [`Attempts::save`] never writes more than [`Attempts::load`] reads.
//! The person's own programs can delete or rewrite the lock program's
//! record, as they can unlock the session through logind anyway (the threat
//! model).

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::view::Millis;
use crate::wait::{LONGEST_WAIT, wait_after_failures};

/// The greeter's record.
pub const GREETER_RECORD: &str = "/run/better-os-greeter/attempts.json";
/// The lock program's record, inside `$XDG_RUNTIME_DIR/better-os/`.
pub const LOCK_RECORD_NAME: &str = "lock-attempts.json";
/// The most entries kept, in the record and in memory each. Past this the
/// entry whose wait ended longest ago goes.
pub const MAX_ACCOUNTS: usize = 64;
/// The largest record read, and so the largest written.
pub const MAX_RECORD_BYTES: u64 = 64 * 1024;
/// The longest account name Linux allows (`LOGIN_NAME_MAX`, bytes with the
/// terminating zero). A longer typed name is counted under its first
/// `LOGIN_NAME_MAX` bytes, in memory only.
pub const LOGIN_NAME_MAX: usize = 256;
/// The highest count kept; from the seventh failure in a row every wait is
/// the longest anyway.
pub const MAX_FAILURES: u32 = 1000;

/// Turns a name typed or listed into the account it names.
pub trait AccountResolver {
    /// The account database's own name for `name` (`pw_name`), or `None`
    /// when no account has it.
    fn account_name(&self, name: &str) -> Option<String>;
}

/// Resolves no name: every typed name is counted in memory only. Listed
/// accounts (from AccountsService) are accounts already.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoResolver;

impl AccountResolver for NoResolver {
    fn account_name(&self, _name: &str) -> Option<String> {
        None
    }
}

/// The system's name service (`getpwnam_r`), which also answers for domain
/// accounts through `sssd` or `winbind`, with their case rules.
#[derive(Clone, Copy, Debug, Default)]
pub struct NameService;

impl AccountResolver for NameService {
    fn account_name(&self, name: &str) -> Option<String> {
        if name.is_empty() || name.len() >= LOGIN_NAME_MAX {
            return None;
        }
        let name = CString::new(name).ok()?;
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut buffer = vec![0 as libc::c_char; 16 * 1024];
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: every pointer names memory this frame owns, with its size.
        let status = unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if status != 0 || result.is_null() || entry.pw_name.is_null() {
            return None;
        }
        // SAFETY: getpwnam_r filled `entry` with strings inside `buffer`.
        let resolved = unsafe { CStr::from_ptr(entry.pw_name) };
        resolved.to_str().ok().map(str::to_string)
    }
}

/// Which count a name falls under.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AttemptKey {
    /// An account the system knows, by its own name: written to the record.
    Account(String),
    /// A name no account has: counted in memory only.
    Unresolved(String),
}

impl AttemptKey {
    /// The key for a name, resolved through `resolver`.
    pub fn for_name(name: &str, resolver: &dyn AccountResolver) -> Self {
        match resolver.account_name(name) {
            Some(account) if !account.is_empty() && account.len() < LOGIN_NAME_MAX => {
                Self::Account(account)
            }
            _ => Self::Unresolved(within_limit(name).to_string()),
        }
    }
}

/// The first [`LOGIN_NAME_MAX`] bytes of `name`, cut at a character.
fn within_limit(name: &str) -> &str {
    if name.len() <= LOGIN_NAME_MAX {
        return name;
    }
    let mut end = LOGIN_NAME_MAX;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// One account's failures in a row and when its wait ends.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Attempt {
    pub failures: u32,
    /// `CLOCK_BOOTTIME` milliseconds; 0 for no wait.
    pub wait_until: Millis,
}

fn longest_wait() -> Millis {
    u64::try_from(LONGEST_WAIT.as_millis()).unwrap_or(u64::MAX)
}

/// Every account's [`Attempt`].
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Attempts {
    accounts: BTreeMap<String, Attempt>,
    #[serde(skip)]
    unresolved: BTreeMap<String, Attempt>,
}

fn trim(map: &mut BTreeMap<String, Attempt>, keep: usize) {
    while map.len() > keep {
        let oldest = map
            .iter()
            .min_by_key(|(name, attempt)| (attempt.wait_until, attempt.failures, (*name).clone()))
            .map(|(name, _)| name.clone());
        match oldest {
            Some(name) => {
                map.remove(&name);
            }
            None => break,
        }
    }
}

impl Attempts {
    fn map<'a>(&'a self, key: &'a AttemptKey) -> (&'a BTreeMap<String, Attempt>, &'a str) {
        match key {
            AttemptKey::Account(name) => (&self.accounts, name),
            AttemptKey::Unresolved(name) => (&self.unresolved, name),
        }
    }

    pub fn get(&self, key: &AttemptKey) -> Attempt {
        let (map, name) = self.map(key);
        map.get(name).copied().unwrap_or_default()
    }

    /// The end of the wait, when it is still running at `now`.
    pub fn waiting_until(&self, key: &AttemptKey, now: Millis) -> Option<Millis> {
        Some(self.get(key).wait_until).filter(|until| *until > now)
    }

    /// Counts one more failure and starts its wait; returns the attempt.
    pub fn record_failure(&mut self, key: &AttemptKey, now: Millis) -> Attempt {
        let mut attempt = self.get(key);
        attempt.failures = attempt.failures.saturating_add(1).min(MAX_FAILURES);
        let wait = wait_after_failures(attempt.failures);
        attempt.wait_until = if wait.is_zero() {
            0
        } else {
            now.saturating_add(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX))
        };
        let (map, name) = match key {
            AttemptKey::Account(name) => (&mut self.accounts, name),
            AttemptKey::Unresolved(name) => (&mut self.unresolved, name),
        };
        map.insert(name.clone(), attempt);
        trim(map, MAX_ACCOUNTS);
        attempt
    }

    /// A success ends the row.
    pub fn record_success(&mut self, key: &AttemptKey) {
        match key {
            AttemptKey::Account(name) => self.accounts.remove(name),
            AttemptKey::Unresolved(name) => self.unresolved.remove(name),
        };
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty() && self.unresolved.is_empty()
    }

    /// Entries the record holds (resolved accounts).
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    /// Holds every entry to what this program writes: a name within
    /// [`LOGIN_NAME_MAX`], a count within [`MAX_FAILURES`], a wait ending no
    /// later than the longest wait from `now`, at most [`MAX_ACCOUNTS`].
    fn bound(&mut self, now: Millis) {
        let latest = now.saturating_add(longest_wait());
        self.accounts
            .retain(|name, _| !name.is_empty() && name.len() < LOGIN_NAME_MAX);
        for attempt in self.accounts.values_mut() {
            attempt.failures = attempt.failures.min(MAX_FAILURES);
            attempt.wait_until = attempt.wait_until.min(latest);
        }
        trim(&mut self.accounts, MAX_ACCOUNTS);
    }

    /// Reads a record at `now`. A missing file is an empty record; one that
    /// cannot be read or parsed is an error, which the program logs (by its
    /// kind only: [`AttemptsError`] never quotes the file) and then starts
    /// from an empty record, because refusing every login over a damaged
    /// file would lock everyone out.
    pub fn load(path: &Path, now: Millis) -> Result<Self, AttemptsError> {
        use std::io::Read;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(AttemptsError::Io(error.kind())),
        };
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| AttemptsError::Io(error.kind()))?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(AttemptsError::TooLarge);
        }
        let mut attempts: Self =
            serde_json::from_slice(&bytes).map_err(|_| AttemptsError::Malformed)?;
        attempts.bound(now);
        Ok(attempts)
    }

    /// The bytes [`Attempts::save`] writes: the resolved accounts, fewer of
    /// them when needed so that they never exceed [`MAX_RECORD_BYTES`].
    fn record_bytes(&self) -> Vec<u8> {
        let mut kept = self.accounts.clone();
        loop {
            let record = Attempts {
                accounts: kept.clone(),
                unresolved: BTreeMap::new(),
            };
            let bytes = serde_json::to_vec(&record).unwrap_or_default();
            if bytes.len() as u64 <= MAX_RECORD_BYTES || kept.is_empty() {
                return bytes;
            }
            let keep = kept.len() - 1;
            trim(&mut kept, keep);
        }
    }

    /// Writes the record: a new file beside it, created with mode 0600 and
    /// without following a link, then renamed over the old one. Names that
    /// resolve to no account are never written.
    pub fn save(&self, path: &Path) -> Result<(), AttemptsError> {
        use std::os::unix::fs::OpenOptionsExt;
        let io = |error: std::io::Error| AttemptsError::Io(error.kind());
        let directory = path.parent().ok_or(AttemptsError::NoDirectory)?;
        let name = path.file_name().ok_or(AttemptsError::NoDirectory)?;
        let temporary = directory.join(format!(".{}.new", name.to_string_lossy()));
        let _ = std::fs::remove_file(&temporary);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(io)?;
        file.write_all(&self.record_bytes()).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(io)?;
        Ok(())
    }
}

/// Why a record could not be read or written. Each says only what kind of
/// failure it was, never anything the file held.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AttemptsError {
    #[error("the failure record could not be read or written: {0}")]
    Io(std::io::ErrorKind),
    #[error("the failure record is longer than {MAX_RECORD_BYTES} bytes")]
    TooLarge,
    #[error("the failure record is not one this program wrote")]
    Malformed,
    #[error("the failure record's path has no directory")]
    NoDirectory,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Millis = 1000;

    fn account(name: &str) -> AttemptKey {
        AttemptKey::Account(name.into())
    }

    /// Resolves names case-insensitively, as `sssd` with
    /// `case_sensitive = false` does for a domain account.
    struct Domain;

    impl AccountResolver for Domain {
        fn account_name(&self, name: &str) -> Option<String> {
            (name.eq_ignore_ascii_case("tim") || name.eq_ignore_ascii_case("tim@example"))
                .then(|| "tim".to_string())
        }
    }

    #[test]
    fn failures_count_per_account() {
        let mut attempts = Attempts::default();
        let lin = account("lin");
        assert_eq!(attempts.record_failure(&lin, 0).wait_until, 0);
        assert_eq!(attempts.record_failure(&lin, 0).wait_until, SECOND);
        assert_eq!(attempts.record_failure(&account("wang"), 0).failures, 1);
        assert_eq!(attempts.waiting_until(&lin, 0), Some(SECOND));
        assert_eq!(attempts.waiting_until(&lin, SECOND), None);
        assert_eq!(attempts.waiting_until(&account("wang"), 0), None);
        assert_eq!(
            attempts.record_failure(&lin, 5 * SECOND).wait_until,
            7 * SECOND
        );
        attempts.record_success(&lin);
        assert_eq!(attempts.get(&lin), Attempt::default());
        assert_eq!(attempts.get(&account("wang")).failures, 1);
    }

    #[test]
    fn names_of_one_account_share_one_count() {
        assert_eq!(AttemptKey::for_name("Tim", &Domain), account("tim"));
        assert_eq!(AttemptKey::for_name("TIM@example", &Domain), account("tim"));
        assert_eq!(
            AttemptKey::for_name("nobody", &Domain),
            AttemptKey::Unresolved("nobody".into())
        );
        let mut attempts = Attempts::default();
        attempts.record_failure(&AttemptKey::for_name("Tim", &Domain), 0);
        attempts.record_failure(&AttemptKey::for_name("tim", &Domain), 0);
        assert_eq!(attempts.get(&account("tim")).failures, 2);
    }

    #[test]
    fn a_name_that_resolves_to_no_account_is_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        let mut attempts = Attempts::default();
        // A password typed into the name field by mistake.
        let typed = AttemptKey::for_name("correct horse battery", &NoResolver);
        attempts.record_failure(&typed, 0);
        attempts.record_failure(&typed, 0);
        // It waits like any account, in memory.
        assert_eq!(attempts.waiting_until(&typed, 0), Some(SECOND));
        attempts.record_failure(&account("lin"), 0);
        attempts.save(&path).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains("correct horse"), "{written}");
        assert!(written.contains("lin"));
        let loaded = Attempts::load(&path, 0).unwrap();
        assert_eq!(loaded.get(&typed), Attempt::default());
        assert_eq!(loaded.get(&account("lin")).failures, 1);
    }

    #[test]
    fn an_over_long_name_is_kept_short_and_in_memory() {
        let long = "x".repeat(5000);
        let key = AttemptKey::for_name(&long, &NoResolver);
        assert_eq!(key, AttemptKey::Unresolved("x".repeat(LOGIN_NAME_MAX)));
        // A resolver that answers with an impossible name is not believed.
        struct Liar;
        impl AccountResolver for Liar {
            fn account_name(&self, _: &str) -> Option<String> {
                Some("y".repeat(LOGIN_NAME_MAX))
            }
        }
        assert!(matches!(
            AttemptKey::for_name("a", &Liar),
            AttemptKey::Unresolved(_)
        ));
        // Cut at a character, not inside one.
        let wide = "字".repeat(200);
        let AttemptKey::Unresolved(cut) = AttemptKey::for_name(&wide, &NoResolver) else {
            panic!("unresolved");
        };
        assert!(cut.len() <= LOGIN_NAME_MAX && cut.chars().all(|c| c == '字'));
    }

    #[test]
    fn the_record_is_bounded() {
        let mut attempts = Attempts::default();
        for index in 0..(MAX_ACCOUNTS + 10) {
            attempts.record_failure(&account(&format!("user{index}")), index as Millis);
            attempts.record_failure(
                &AttemptKey::Unresolved(format!("typed{index}")),
                index as Millis,
            );
        }
        assert_eq!(attempts.len(), MAX_ACCOUNTS);
        assert_eq!(attempts.unresolved.len(), MAX_ACCOUNTS);
    }

    #[test]
    fn sixty_four_longest_names_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        let mut attempts = Attempts::default();
        for index in 0..MAX_ACCOUNTS {
            let name = format!(
                "{index:03}{}",
                "\u{10FFFF}".repeat((LOGIN_NAME_MAX - 8) / 4)
            );
            assert!(name.len() < LOGIN_NAME_MAX);
            attempts.record_failure(&account(&name), 0);
            attempts.record_failure(&account(&name), 0);
        }
        attempts.save(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size <= MAX_RECORD_BYTES, "{size}");
        let loaded = Attempts::load(&path, 0).unwrap();
        assert_eq!(loaded.len(), MAX_ACCOUNTS);
    }

    #[test]
    fn save_never_writes_more_than_load_reads() {
        // A record that would not fit (names past the limit, built by hand)
        // is cut down when written, never written whole.
        let mut attempts = Attempts::default();
        for index in 0..MAX_ACCOUNTS {
            attempts
                .accounts
                .insert(format!("{index}{}", "z".repeat(4000)), Attempt::default());
        }
        let bytes = attempts.record_bytes();
        assert!(bytes.len() as u64 <= MAX_RECORD_BYTES);
        assert!(serde_json::from_slice::<Attempts>(&bytes).is_ok());
    }

    #[test]
    fn a_record_read_back_is_held_to_what_could_have_been_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        let forged = format!(
            r#"{{"accounts":{{"tim":{{"failures":4294967295,"wait_until":18446744073709551615}},"{}":{{"failures":1,"wait_until":0}}}}}}"#,
            "n".repeat(LOGIN_NAME_MAX)
        );
        std::fs::write(&path, forged).unwrap();
        let now = 100 * SECOND;
        let loaded = Attempts::load(&path, now).unwrap();
        let tim = loaded.get(&account("tim"));
        assert_eq!(tim.failures, MAX_FAILURES);
        assert_eq!(tim.wait_until, now + 30 * SECOND);
        assert_eq!(loaded.len(), 1, "a name past LOGIN_NAME_MAX is dropped");
    }

    #[test]
    fn the_record_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        assert!(Attempts::load(&path, 0).unwrap().is_empty());
        let mut attempts = Attempts::default();
        attempts.record_failure(&account("lin"), 0);
        attempts.record_failure(&account("lin"), 0);
        attempts.save(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let loaded = Attempts::load(&path, 0).unwrap();
        assert_eq!(loaded, attempts);
        assert_eq!(loaded.waiting_until(&account("lin"), 0), Some(SECOND));
        let mut later = loaded.clone();
        later.record_success(&account("lin"));
        later.save(&path).unwrap();
        assert!(Attempts::load(&path, 0).unwrap().is_empty());
    }

    #[test]
    fn a_damaged_or_huge_record_is_an_error_that_quotes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, br#"{"accounts":{"hunter2-typed": 5}}"#).unwrap();
        let error = Attempts::load(&bad, 0).unwrap_err();
        assert_eq!(error, AttemptsError::Malformed);
        assert!(!error.to_string().contains("hunter2"));
        let huge = dir.path().join("huge.json");
        std::fs::write(&huge, vec![b' '; MAX_RECORD_BYTES as usize + 1]).unwrap();
        assert_eq!(Attempts::load(&huge, 0), Err(AttemptsError::TooLarge));
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

    #[test]
    fn the_name_service_resolves_this_process_owner() {
        // SAFETY: getuid has no preconditions; getpwuid's answer is read at once.
        let own = unsafe {
            let entry = libc::getpwuid(libc::getuid());
            assert!(!entry.is_null());
            CStr::from_ptr((*entry).pw_name)
                .to_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(NameService.account_name(&own), Some(own.clone()));
        assert_eq!(NameService.account_name("no-such-account-260"), None);
        assert_eq!(NameService.account_name(""), None);
        assert_eq!(NameService.account_name("a\0b"), None);
    }
}
