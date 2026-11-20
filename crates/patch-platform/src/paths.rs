//! Where the files Better Patch reads and writes are.

use std::path::{Path, PathBuf};

/// The directories one person's fixes involve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paths {
    /// The person's Flatpak installation, `$XDG_DATA_HOME/flatpak` (or
    /// `$FLATPAK_USER_DIR`). Its `overrides/` is the only place a fix
    /// writes.
    pub user_flatpak: PathBuf,
    /// The machine's installation, `/var/lib/flatpak` (or
    /// `$FLATPAK_SYSTEM_DIR`). Read only.
    pub system_flatpak: PathBuf,
    /// The record of applied fixes,
    /// `$XDG_STATE_HOME/better-os/fixes.json`.
    pub records: PathBuf,
}

/// The records file's name.
pub const RECORDS_FILE: &str = "fixes.json";

fn absolute(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

impl Paths {
    /// The person's own, as Flatpak and the XDG base directories find
    /// them. `None` without a home directory, rather than a guessed one.
    pub fn for_user() -> Option<Self> {
        let home = absolute("HOME");
        let data =
            absolute("XDG_DATA_HOME").or_else(|| Some(home.clone()?.join(".local/share")))?;
        let state =
            absolute("XDG_STATE_HOME").or_else(|| Some(home.clone()?.join(".local/state")))?;
        Some(Self {
            user_flatpak: absolute("FLATPAK_USER_DIR").unwrap_or_else(|| data.join("flatpak")),
            system_flatpak: absolute("FLATPAK_SYSTEM_DIR")
                .unwrap_or_else(|| PathBuf::from("/var/lib/flatpak")),
            records: state.join("better-os").join(RECORDS_FILE),
        })
    }

    /// Everything under one directory, for a fixture machine.
    pub fn under(root: &Path) -> Self {
        Self {
            user_flatpak: root.join("data/flatpak"),
            system_flatpak: root.join("system-flatpak"),
            records: root.join("state/better-os").join(RECORDS_FILE),
        }
    }

    /// The person's override file for an app.
    pub fn user_override(&self, app: &patch_core::flatpak::AppId) -> PathBuf {
        self.user_flatpak.join("overrides").join(app.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixture_keeps_everything_under_its_root() {
        let paths = Paths::under(Path::new("/scratch"));
        assert!(paths.user_flatpak.starts_with("/scratch"));
        assert!(paths.system_flatpak.starts_with("/scratch"));
        assert!(paths.records.starts_with("/scratch"));
        let app = patch_core::flatpak::AppId::parse("org.example.App").unwrap();
        assert_eq!(
            paths.user_override(&app),
            Path::new("/scratch/data/flatpak/overrides/org.example.App")
        );
    }
}
