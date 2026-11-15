//! Moves a removed component's personal data to the person's trash.
//!
//! Which paths count is `manager_core::personal_data`'s decision; this module
//! only carries it out, under one home directory, into the freedesktop trash
//! `files-platform` already implements. Nothing is unlinked: a person who
//! ticked "Also delete my settings and history" by mistake can take it back
//! from the trash.

use std::path::{Path, PathBuf};

use files_platform::trash::{TrashDirectory, TrashError, move_to_trash};

/// What happened to each path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DataRemoval {
    /// Moved to the trash.
    pub trashed: Vec<PathBuf>,
    /// Not there to begin with.
    pub missing: Vec<PathBuf>,
    /// Could not be moved, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

impl DataRemoval {
    pub fn succeeded(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Moves each of `relative` (paths under `home`, already classified as
/// personal data) to `trash`.
pub fn move_personal_data_to_trash(
    home: &Path,
    relative: &[&str],
    trash: &TrashDirectory,
) -> DataRemoval {
    let mut report = DataRemoval::default();
    for relative in relative {
        let path = home.join(relative);
        if std::fs::symlink_metadata(&path).is_err() {
            report.missing.push(path);
            continue;
        }
        match move_to_trash(trash, &path) {
            Ok(_) => report.trashed.push(path),
            Err(TrashError::NotFound { .. }) => report.missing.push(path),
            Err(error) => report.failed.push((path, error.to_string())),
        }
    }
    report
}

/// The person's home directory and home trash, from the environment, or
/// `None` when there is no home to delete from.
pub fn home_and_trash() -> Option<(PathBuf, TrashDirectory)> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let trash = TrashDirectory::home_from_env()?;
    Some((home, trash))
}

#[cfg(test)]
mod tests {
    use super::move_personal_data_to_trash;
    use files_platform::trash::TrashDirectory;

    #[test]
    fn data_goes_to_the_trash_and_a_missing_path_is_reported() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config/better-os");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("git.json"), "{}").unwrap();
        let trash = TrashDirectory::new(home.path().join(".local/share/Trash"));

        let report = move_personal_data_to_trash(
            home.path(),
            &[
                ".config/better-os/git.json",
                ".local/state/better-os/absent",
            ],
            &trash,
        );

        assert!(report.succeeded());
        assert_eq!(report.trashed, [config.join("git.json")]);
        assert_eq!(
            report.missing,
            [home.path().join(".local/state/better-os/absent")]
        );
        assert!(!config.join("git.json").exists());
        assert!(trash.files_dir().join("git.json").exists());
        assert!(trash.info_dir().join("git.json.trashinfo").exists());
    }

    #[test]
    fn a_whole_directory_is_trashed_as_one_item() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join(".local/state/better-awake");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("awake-rules.json"), "[]").unwrap();
        let trash = TrashDirectory::new(home.path().join(".local/share/Trash"));

        let report =
            move_personal_data_to_trash(home.path(), &[".local/state/better-awake"], &trash);

        assert_eq!(report.trashed, std::slice::from_ref(&state));
        assert!(!state.exists());
        assert!(
            trash
                .files_dir()
                .join("better-awake/awake-rules.json")
                .exists()
        );
    }
}
