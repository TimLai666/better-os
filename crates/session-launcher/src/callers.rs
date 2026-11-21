//! Who may end the session over the bus.
//!
//! The launcher owns `org.betteros.Session1` with one method, `Logout`, which
//! stops the target and ends the compositor. It is not a privilege boundary:
//! anything running as the person can end their own session through logind.
//! The check keeps an arbitrary program from logging the person out by
//! accident or on a page's request: only the session's own programs, of the
//! same person. A program is the file it runs, compared by device and inode
//! (`stat` of `/proc/<pid>/exe`, which reaches the running file itself) with
//! the installed file, not by the path the link reads: a process in another
//! mount namespace (a Flatpak whose runtime has a file at that path) is not
//! the installed program.
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

/// A file by its device and inode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileId {
    pub device: u64,
    pub inode: u64,
}

/// The file `path` leads to, following links; for `/proc/<pid>/exe`, the
/// file the process runs.
pub fn file_id(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::metadata(path).ok()?;
    Some(FileId {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// The caller as the launcher saw it: the path its `exe` link reads (for
/// the journal) and the file it runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Caller {
    pub link: PathBuf,
    pub file: FileId,
}

/// The rule. `installed` gives the file at an allowed path on this machine.
/// A program replaced by an update runs a file no longer at its path and is
/// refused until it restarts itself, which AGENTS.md has every window do
/// within seconds.
pub fn check(
    caller_uid: u32,
    own_uid: u32,
    caller: Option<&Caller>,
    allowed: &[&str],
    installed: impl Fn(&Path) -> Option<FileId>,
) -> Result<(), Refusal> {
    if caller_uid != own_uid {
        return Err(Refusal::OtherUser(caller_uid));
    }
    let caller = caller.ok_or(Refusal::UnknownProgram)?;
    if allowed
        .iter()
        .any(|allowed| installed(Path::new(allowed)) == Some(caller.file))
    {
        Ok(())
    } else {
        Err(Refusal::NotAllowed(running_program(&caller.link)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two files standing for installed programs, and the rule over them.
    struct Machine {
        _dir: tempfile::TempDir,
        manager: PathBuf,
        osd: PathBuf,
    }

    impl Machine {
        fn new() -> Machine {
            let dir = tempfile::tempdir().unwrap();
            let manager = dir.path().join("better-manager");
            let osd = dir.path().join("cosmic-osd");
            std::fs::write(&manager, "m").unwrap();
            std::fs::write(&osd, "o").unwrap();
            Machine {
                _dir: dir,
                manager,
                osd,
            }
        }
        /// Maps the installed paths of the lists to the scratch files.
        fn installed(&self) -> impl Fn(&Path) -> Option<FileId> + '_ {
            move |path| match path.to_str() {
                Some("/usr/bin/better-manager") => file_id(&self.manager),
                Some("/usr/bin/cosmic-osd") => file_id(&self.osd),
                _ => None,
            }
        }
        fn caller(&self, link: &str, file: &Path) -> Caller {
            Caller {
                link: PathBuf::from(link),
                file: file_id(file).unwrap(),
            }
        }
    }

    #[test]
    fn better_manager_of_the_same_person_may_log_out() {
        let machine = Machine::new();
        let caller = machine.caller("/usr/bin/better-manager", &machine.manager);
        assert_eq!(
            check(
                1000,
                1000,
                Some(&caller),
                LOGOUT_CALLERS,
                machine.installed()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_file_at_the_same_path_that_is_not_the_installed_one_is_refused() {
        let machine = Machine::new();
        // Another mount namespace's /usr/bin/better-manager: the same path,
        // another file.
        let elsewhere = machine._dir.path().join("flatpak-better-manager");
        std::fs::write(&elsewhere, "m").unwrap();
        let caller = machine.caller("/usr/bin/better-manager", &elsewhere);
        assert_eq!(
            check(
                1000,
                1000,
                Some(&caller),
                LOGOUT_CALLERS,
                machine.installed()
            ),
            Err(Refusal::NotAllowed(PathBuf::from(
                "/usr/bin/better-manager"
            )))
        );
        // A hard link is the same file.
        let linked = machine._dir.path().join("hard-link");
        std::fs::hard_link(&machine.manager, &linked).unwrap();
        let caller = machine.caller("/opt/x", &linked);
        assert!(
            check(
                1000,
                1000,
                Some(&caller),
                LOGOUT_CALLERS,
                machine.installed()
            )
            .is_ok()
        );
    }

    #[test]
    fn anything_else_is_refused_with_the_reason() {
        let machine = Machine::new();
        let manager = machine.caller("/usr/bin/better-manager", &machine.manager);
        assert_eq!(
            check(
                1001,
                1000,
                Some(&manager),
                LOGOUT_CALLERS,
                machine.installed()
            ),
            Err(Refusal::OtherUser(1001))
        );
        assert_eq!(
            check(1000, 1000, None, LOGOUT_CALLERS, machine.installed()),
            Err(Refusal::UnknownProgram)
        );
        // Each interface has its own list.
        let osd = machine.caller("/usr/bin/cosmic-osd (deleted)", &machine.osd);
        assert_eq!(
            check(1000, 1000, Some(&osd), LOGOUT_CALLERS, machine.installed()),
            Err(Refusal::NotAllowed(PathBuf::from("/usr/bin/cosmic-osd")))
        );
        assert_eq!(
            check(
                1000,
                1000,
                Some(&osd),
                COSMIC_EXIT_CALLERS,
                machine.installed()
            ),
            Ok(())
        );
        // A program that is not installed is never allowed.
        assert!(check(1000, 1000, Some(&manager), LOGOUT_CALLERS, |_: &Path| None).is_err());
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
