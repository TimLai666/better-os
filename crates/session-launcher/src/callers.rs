//! Who may end the session over the bus.
//!
//! The launcher owns `org.betteros.Session1` with one method, `Logout`, which
//! stops the target and ends the compositor. It is not a privilege boundary:
//! anything running as the person can end their own session through logind.
//! The check keeps an arbitrary program from logging the person out by
//! accident or on a page's request: only the session's own programs, named by
//! the file they run, of the same person.
//!
//! The launcher also answers `com.system76.CosmicSession.Exit`, the call
//! `cosmic-osd log-out` makes after its confirmation (COSMIC's `LogOut`
//! action, Super+Shift+Escape); only `cosmic-osd` may make it.

use std::path::{Path, PathBuf};

/// The programs that may call `org.betteros.Session1.Logout`: Better
/// Manager's Log out now (ticket 205). The top bar's end-session dialog
/// (ticket 160) adds its program here when it ships.
pub const LOGOUT_CALLERS: &[&str] = &["/usr/bin/better-manager"];

/// The programs that may call `com.system76.CosmicSession.Exit`.
pub const COSMIC_EXIT_CALLERS: &[&str] = &["/usr/bin/cosmic-osd"];

/// The session interface's bus name and object path.
pub const SESSION_NAME: &str = "org.betteros.Session1";
pub const SESSION_PATH: &str = "/org/betteros/Session1";

/// COSMIC's session name and path, as `cosmic-osd` calls them.
pub const COSMIC_SESSION_NAME: &str = "com.system76.CosmicSession";
pub const COSMIC_SESSION_PATH: &str = "/com/system76/CosmicSession";

/// Why a call was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// Another person's process.
    OtherUser(u32),
    /// The caller's program could not be read.
    UnknownProgram,
    /// A program not on the list.
    NotAllowed(PathBuf),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::OtherUser(uid) => {
                write!(
                    formatter,
                    "the caller runs as user {uid}, not as this session's person"
                )
            }
            Refusal::UnknownProgram => write!(formatter, "the caller's program could not be read"),
            Refusal::NotAllowed(program) => {
                write!(formatter, "{} may not end the session", program.display())
            }
        }
    }
}

/// The program a `/proc/<pid>/exe` link names. A program replaced by an
/// update reads as `<path> (deleted)` until it restarts itself, and is still
/// that program.
pub fn running_program(link: &Path) -> PathBuf {
    let text = link.as_os_str().to_string_lossy();
    match text.strip_suffix(" (deleted)") {
        Some(path) => PathBuf::from(path),
        None => link.to_path_buf(),
    }
}

/// The rule.
pub fn check(
    caller_uid: u32,
    own_uid: u32,
    program: Option<&Path>,
    allowed: &[&str],
) -> Result<(), Refusal> {
    if caller_uid != own_uid {
        return Err(Refusal::OtherUser(caller_uid));
    }
    let program = program
        .map(running_program)
        .ok_or(Refusal::UnknownProgram)?;
    if allowed.iter().any(|allowed| Path::new(allowed) == program) {
        Ok(())
    } else {
        Err(Refusal::NotAllowed(program))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn better_manager_of_the_same_person_may_log_out() {
        assert_eq!(
            check(
                1000,
                1000,
                Some(Path::new("/usr/bin/better-manager")),
                LOGOUT_CALLERS
            ),
            Ok(())
        );
        assert_eq!(
            check(
                1000,
                1000,
                Some(Path::new("/usr/bin/better-manager (deleted)")),
                LOGOUT_CALLERS
            ),
            Ok(())
        );
    }

    #[test]
    fn anything_else_is_refused_with_the_reason() {
        assert_eq!(
            check(
                1001,
                1000,
                Some(Path::new("/usr/bin/better-manager")),
                LOGOUT_CALLERS
            ),
            Err(Refusal::OtherUser(1001))
        );
        assert_eq!(
            check(1000, 1000, None, LOGOUT_CALLERS),
            Err(Refusal::UnknownProgram)
        );
        assert_eq!(
            check(
                1000,
                1000,
                Some(Path::new("/usr/bin/busctl")),
                LOGOUT_CALLERS
            ),
            Err(Refusal::NotAllowed(PathBuf::from("/usr/bin/busctl")))
        );
        // A copy of an allowed program elsewhere is not that program.
        assert!(
            check(
                1000,
                1000,
                Some(Path::new("/tmp/better-manager")),
                LOGOUT_CALLERS
            )
            .is_err()
        );
        // Each interface has its own list.
        assert!(
            check(
                1000,
                1000,
                Some(Path::new("/usr/bin/cosmic-osd")),
                LOGOUT_CALLERS
            )
            .is_err()
        );
        assert_eq!(
            check(
                1000,
                1000,
                Some(Path::new("/usr/bin/cosmic-osd")),
                COSMIC_EXIT_CALLERS
            ),
            Ok(())
        );
    }

    #[test]
    fn the_refusals_read_as_sentences() {
        assert!(
            Refusal::NotAllowed(PathBuf::from("/usr/bin/x"))
                .to_string()
                .contains("/usr/bin/x")
        );
        assert!(Refusal::OtherUser(7).to_string().contains('7'));
    }
}
