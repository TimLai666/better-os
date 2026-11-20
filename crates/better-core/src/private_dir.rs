//! A scratch directory only this person can reach, for a demo or stand-in
//! mode that writes files (the Fixes page's example PC, the Touchpad page's
//! demo store).
//!
//! It lives under the session's private runtime directory
//! (`XDG_RUNTIME_DIR`) and nowhere else. A shared place such as `/tmp` with
//! a fixed or pid-made name is refused by design: another local account can
//! create it first, lay links in it, and have this person's writes land in
//! a file of theirs.

use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

/// This process's user id.
fn my_uid() -> Result<u32, String> {
    std::fs::metadata("/proc/self")
        .map(|metadata| metadata.uid())
        .map_err(|error| format!("/proc/self: {error}"))
}

fn private(path: &Path, me: u32, forbidden: u32) -> Result<(), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.file_type().is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    if metadata.uid() != me {
        return Err(format!("{} is not yours", path.display()));
    }
    if metadata.mode() & forbidden != 0 {
        return Err(format!("{} is open to others", path.display()));
    }
    Ok(())
}

/// `<runtime>/<parts...>`, made 0700 where missing. The runtime directory
/// must be a real directory (not a link) this person owns with no access
/// for anyone else, and each part below it a real directory they own that
/// nobody else can write. `runtime` is `XDG_RUNTIME_DIR`; without one there
/// is no private place, and the caller offers nothing rather than falling
/// back to a shared one.
pub fn private_runtime_dir(runtime: Option<&Path>, parts: &[&str]) -> Result<PathBuf, String> {
    let runtime = runtime
        .filter(|path| path.is_absolute())
        .ok_or_else(|| "there is no private runtime directory (XDG_RUNTIME_DIR)".to_string())?;
    let me = my_uid()?;
    private(runtime, me, 0o077)?;
    let mut path = runtime.to_path_buf();
    for part in parts {
        if part.is_empty() || part.contains('/') || *part == "." || *part == ".." {
            return Err(format!("{part:?} is not a directory name"));
        }
        path.push(part);
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
        private(&path, me, 0o022)?;
    }
    Ok(path)
}

/// The same under this session's own `XDG_RUNTIME_DIR`.
pub fn private_session_dir(parts: &[&str]) -> Result<PathBuf, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    private_runtime_dir(runtime.as_deref(), parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn runtime() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn a_private_directory_is_made_0700_under_the_runtime_directory() {
        let dir = runtime();
        let made = private_runtime_dir(Some(dir.path()), &["better-os", "demo"]).unwrap();
        assert_eq!(made, dir.path().join("better-os/demo"));
        let mode = std::fs::metadata(&made).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        // Again: the same directory.
        assert_eq!(
            private_runtime_dir(Some(dir.path()), &["better-os", "demo"]).unwrap(),
            made
        );
    }

    #[test]
    fn no_private_runtime_directory_means_none() {
        assert!(private_runtime_dir(None, &["x"]).is_err());
        assert!(private_runtime_dir(Some(Path::new("relative")), &["x"]).is_err());
        let open = tempfile::tempdir().unwrap();
        std::fs::set_permissions(open.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_runtime_dir(Some(open.path()), &["x"]).is_err());
        let dir = runtime();
        assert!(private_runtime_dir(Some(dir.path()), &["../x"]).is_err());
    }

    #[test]
    fn a_link_laid_in_advance_is_refused_and_nothing_is_written_through_it() {
        let dir = runtime();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("better-os")).unwrap();
        assert!(private_runtime_dir(Some(dir.path()), &["better-os", "demo"]).is_err());
        assert!(
            std::fs::read_dir(elsewhere.path())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn a_part_others_can_write_is_refused() {
        let dir = runtime();
        let shared = dir.path().join("better-os");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(private_runtime_dir(Some(dir.path()), &["better-os", "demo"]).is_err());
    }
}
