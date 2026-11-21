//! The session's record of what it changed in the user manager, kept where
//! something other than the launcher can put it back.
//!
//! A launcher that dies without running its end (SIGKILL, the OOM killer, a
//! panic) would otherwise leave `DCONF_PROFILE=betteros` and the session's
//! `XDG_DATA_DIRS` in the user manager, and `better-os-session.target` with
//! COSMIC's daemons running, for the next GNOME login to read and write
//! (ticket 203's review, P1). So before it starts the target, the launcher
//! writes this record to `$XDG_RUNTIME_DIR/better-os/session/restore.json`:
//! its own process id and start time, and the import with how to put it back.
//! Three things act on it, and the first to [`take`] it does the work:
//!
//! - `better-os-session-guard.service` (`start --guard`), a unit of the target,
//!   watches the launcher and stops the target when it ends; its
//!   `ExecStopPost=` (`start --restore`) puts the environment back whenever
//!   the session is over ([`session_over`]), and not when the guard is only
//!   restarted.
//! - The launcher's own end, after the units have stopped.
//! - `better-os-session-leftovers.service` (`start --leftovers`), wanted by
//!   `graphical-session-pre.target` in any session of the person, which puts
//!   back a record whose launcher is gone.
//!
//! The record's presence is also what the target and its units require
//! (`ConditionPathExists=`), so none of them starts outside a session.

use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::environment::Import;

/// The record's directory under `XDG_RUNTIME_DIR`.
pub const DIRECTORY: &str = "better-os/session";

/// The record's file name. The units name the same path with `%t`.
pub const FILE: &str = "restore.json";

/// The record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Record {
    /// The launcher's process id.
    pub leader: i32,
    /// The launcher's start time (field 22 of `/proc/<pid>/stat`), so a
    /// process id used again by another program is not taken for it.
    pub leader_start: u64,
    /// What the user manager was told, and how it is put back.
    pub import: Import,
    /// What D-Bus activation is set to at the end
    /// ([`crate::environment::activation_restore`]).
    pub activation: std::collections::BTreeMap<String, String>,
}

/// The record's path in a runtime directory.
pub fn path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(DIRECTORY).join(FILE)
}

/// Writes the record, replacing any, through a file renamed into place in a
/// directory only the person can read.
pub fn write(runtime_dir: &Path, record: &Record) -> io::Result<()> {
    let directory = runtime_dir.join(DIRECTORY);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)?;
    let partial = directory.join(format!("{FILE}.{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&partial)?;
    file.write_all(&serde_json::to_vec(record).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&partial, path(runtime_dir))
}

/// Reads the record without taking it.
pub fn read(runtime_dir: &Path) -> Option<Record> {
    serde_json::from_slice(&fs::read(path(runtime_dir)).ok()?).ok()
}

/// Takes the record: renames it away first, so of several takers at once
/// exactly one gets it, then reads and deletes it.
pub fn take(runtime_dir: &Path) -> Option<Record> {
    let taken = runtime_dir
        .join(DIRECTORY)
        .join(format!("{FILE}.taken.{}", std::process::id()));
    fs::rename(path(runtime_dir), &taken).ok()?;
    let record = fs::read(&taken)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let _ = fs::remove_file(&taken);
    record
}

/// The start time of a process, from its `stat` file under `proc_root`.
pub fn start_time(proc_root: &Path, pid: i32) -> Option<u64> {
    let stat = fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    // The command name is in parentheses and may hold spaces; the fields
    // after the last ')' start at field 3.
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(19)?.parse().ok()
}

/// Whether the record's launcher still runs.
pub fn leader_alive(record: &Record, proc_root: &Path) -> bool {
    start_time(proc_root, record.leader) == Some(record.leader_start)
}

/// Whether the guard's `ExecStopPost=` puts the environment back: when the
/// launcher is gone, or the target is on its way down. A guard stopped while
/// the launcher runs and the target stays is being restarted (an update of
/// the package restarts its running units, ticket 203's VM run took the
/// whole session down that way while the target was bound to the guard) and
/// changes nothing.
pub fn session_over(leader_alive: bool, target_staying: bool) -> bool {
    !leader_alive || !target_staying
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restarted_guard_leaves_a_running_session_alone() {
        assert!(!session_over(true, true), "an update restarts the guard");
        assert!(session_over(true, false), "a logout stops the target");
        assert!(session_over(false, true), "the launcher died");
        assert!(session_over(false, false));
    }

    fn record(leader: i32, leader_start: u64) -> Record {
        let mut import = Import::default();
        import.set.insert("DCONF_PROFILE".into(), "betteros".into());
        import.restore_unset.push("DCONF_PROFILE".into());
        Record {
            leader,
            leader_start,
            import,
            activation: Default::default(),
        }
    }

    #[test]
    fn a_record_is_written_private_and_taken_once() {
        let runtime = tempfile::tempdir().unwrap();
        write(runtime.path(), &record(42, 7)).unwrap();
        let mode = |p: &Path| {
            use std::os::unix::fs::PermissionsExt as _;
            fs::metadata(p).unwrap().permissions().mode() & 0o777
        };
        assert_eq!(mode(&runtime.path().join(DIRECTORY)), 0o700);
        assert_eq!(mode(&path(runtime.path())), 0o600);
        assert_eq!(read(runtime.path()), Some(record(42, 7)));
        assert_eq!(take(runtime.path()), Some(record(42, 7)));
        assert_eq!(take(runtime.path()), None, "a record is taken once");
        assert!(!path(runtime.path()).exists());
        let left: Vec<_> = fs::read_dir(runtime.path().join(DIRECTORY))
            .unwrap()
            .collect();
        assert!(left.is_empty(), "nothing is left behind: {left:?}");
    }

    #[test]
    fn the_leader_is_known_by_its_start_time_not_its_id_alone() {
        let proc_root = tempfile::tempdir().unwrap();
        let stat_dir = proc_root.path().join("1234");
        fs::create_dir_all(&stat_dir).unwrap();
        // Field 22 is the start time; the name holds a space and a ')'.
        fs::write(
            stat_dir.join("stat"),
            "1234 (start x) y) S 1 1234 1234 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 98765 0 0",
        )
        .unwrap();
        assert_eq!(start_time(proc_root.path(), 1234), Some(98765));
        assert!(leader_alive(&record(1234, 98765), proc_root.path()));
        assert!(
            !leader_alive(&record(1234, 11), proc_root.path()),
            "the id was used again"
        );
        assert!(!leader_alive(&record(999, 98765), proc_root.path()), "gone");
    }

    #[test]
    fn this_process_is_alive_by_the_real_proc() {
        let me = std::process::id() as i32;
        let start = start_time(Path::new("/proc"), me).unwrap();
        assert!(leader_alive(&record(me, start), Path::new("/proc")));
    }
}
