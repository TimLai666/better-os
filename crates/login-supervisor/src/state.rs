//! `/run/better-os/login-path.json`: read back at start, written whole and
//! atomically after every step that changed it.

use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use crate::read::read_bounded;
use crate::rules::{Loaded, State};

/// Larger than any state the rules produce (a few hundred notes at most).
pub const MAX_STATE_BYTES: u64 = 256 * 1024;

/// What the file holds: nothing, a state, or something that is not one.
pub fn load(path: &Path) -> Loaded {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Loaded::Absent,
        Err(_) => return Loaded::Unreadable,
        Ok(_) => {}
    }
    let Some(bytes) = read_bounded(path, MAX_STATE_BYTES) else {
        return Loaded::Unreadable;
    };
    match serde_json::from_slice::<State>(&bytes) {
        Ok(state) if state.version == crate::rules::STATE_VERSION => Loaded::Found(Box::new(state)),
        _ => Loaded::Unreadable,
    }
}

/// Writes `state` to a new temporary file (never through a link) and
/// renames it into place, 0644: `better-os-login-status` reads it as anyone.
pub fn save(path: &Path, state: &State) -> std::io::Result<()> {
    write_atomic(
        path,
        &serde_json::to_vec(state).map_err(std::io::Error::other)?,
    )
}

/// The one way the supervisor writes a file of its own under `/run`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let temporary = dir.join(format!(".{name}.tmp"));
    match std::fs::remove_file(&temporary) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .mode(0o644)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Phase, State};

    #[test]
    fn a_state_round_trips_and_anything_else_is_unreadable_not_new() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("login-path.json");
        assert_eq!(load(&path), Loaded::Absent);
        let mut state = State::fresh(
            Some("b".repeat(32)),
            crate::rules::UnitStart {
                began: Some(5_000_000),
                restarts: Some(0),
            },
            6_000_000,
        );
        state.phase = Phase::Attempt {
            number: 2,
            started: 30_000_000,
            baseline: Some(7),
            session_seen: true,
            heartbeat_seen: false,
        };
        save(&path, &state).unwrap();
        assert_eq!(load(&path), Loaded::Found(Box::new(state.clone())));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        std::fs::write(&path, "{\"version\":1").unwrap();
        assert_eq!(load(&path), Loaded::Unreadable);
        let mut other = serde_json::to_value(&state).unwrap();
        other["version"] = 99.into();
        std::fs::write(&path, other.to_string()).unwrap();
        assert_eq!(load(&path), Loaded::Unreadable);
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), &path).unwrap();
        assert_eq!(load(&path), Loaded::Unreadable);
    }
}
