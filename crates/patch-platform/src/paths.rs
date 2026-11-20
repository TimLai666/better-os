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

    /// Everything under one directory, for a fixture machine. The root is
    /// laid out as a home (`.local/share/flatpak`, `.local/state/better-os`),
    /// so a sandbox that makes it the window's HOME shows the paths a
    /// person would see.
    pub fn under(root: &Path) -> Self {
        Self {
            system_flatpak: root.join("system-flatpak"),
            ..Self::default_for_home(root)
        }
    }

    /// The person's override file for an app.
    pub fn user_override(&self, app: &patch_core::flatpak::AppId) -> PathBuf {
        self.user_flatpak.join("overrides").join(app.as_str())
    }

    /// Where everything is for a home with no XDG variables set: what a
    /// maintainer script, which cannot see them, finds on its own.
    pub fn default_for_home(home: &Path) -> Self {
        Self {
            user_flatpak: home.join(".local/share/flatpak"),
            system_flatpak: PathBuf::from("/var/lib/flatpak"),
            records: default_state(home).join(RECORDS_FILE),
        }
    }
}

/// `~/.local/state/better-os`, where the files a removal reads without the
/// person's environment live.
pub fn default_state(home: &Path) -> PathBuf {
    home.join(".local/state/better-os")
}

/// The file that says where the record and the Flatpak overrides really
/// are, kept at the default location while either is somewhere else.
pub const LOCATION_FILE: &str = "fixes-location.json";

/// Writes or removes the location file for `paths`, so a removal run with
/// no XDG variables finds a record kept under a custom `XDG_STATE_HOME` and
/// overrides under a custom `XDG_DATA_HOME`. Removed again when everything
/// is where the defaults say.
pub fn keep_location(home: &Path, paths: &Paths) -> Result<(), String> {
    let file = default_state(home).join(LOCATION_FILE);
    let defaults = Paths::default_for_home(home);
    if paths.records == defaults.records && paths.user_flatpak == defaults.user_flatpak {
        return match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("{}: {error}", file.display())),
        };
    }
    let text = serde_json::json!({
        "records": paths.records,
        "user_flatpak": paths.user_flatpak,
    })
    .to_string();
    let failed = |error: std::io::Error| format!("{}: {error}", file.display());
    std::fs::create_dir_all(default_state(home)).map_err(failed)?;
    let temporary = file.with_extension("json.tmp");
    std::fs::write(&temporary, text).map_err(failed)?;
    std::fs::rename(&temporary, &file).map_err(failed)
}

/// Whether a path the location file names may be followed: absolute, with
/// no `..`, and the deepest part of it that exists (links followed) is the
/// person's own, either inside their home or owned by them. The removal runs
/// as that person, so it can reach nothing they could not; a custom
/// `XDG_DATA_HOME` such as `/data/tim/share` is theirs and is followed, a
/// path into a directory someone else owns is not.
pub fn followable(path: &Path, home: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;
    if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return false;
    }
    let mut existing = path.to_path_buf();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent.to_path_buf(),
            None => return false,
        }
    }
    let Ok(real) = existing.canonicalize() else {
        return false;
    };
    if home.canonicalize().is_ok_and(|home| real.starts_with(home)) {
        return true;
    }
    let me = std::fs::metadata("/proc/self").map(|m| m.uid());
    match (std::fs::metadata(&real), me) {
        (Ok(metadata), Ok(me)) => metadata.uid() == me && me != 0,
        _ => false,
    }
}

/// Where a removal finds one person's files: the defaults, or what the
/// location file says when everything it names stays inside the home.
/// A location file that cannot be read, or that points out of the home, is
/// reported and the defaults are used.
pub fn for_removal(home: &Path) -> (Paths, Option<String>) {
    let defaults = Paths::default_for_home(home);
    let file = default_state(home).join(LOCATION_FILE);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (defaults, None),
        Err(error) => return (defaults, Some(format!("{}: {error}", file.display()))),
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => return (defaults, Some(format!("{}: {error}", file.display()))),
    };
    let named = |key: &str| value.get(key).and_then(|v| v.as_str()).map(PathBuf::from);
    match (named("records"), named("user_flatpak")) {
        (Some(records), Some(user_flatpak))
            if followable(&records, home) && followable(&user_flatpak, home) =>
        {
            (
                Paths {
                    records,
                    user_flatpak,
                    system_flatpak: defaults.system_flatpak.clone(),
                },
                None,
            )
        }
        _ => (
            defaults,
            Some(format!(
                "{} names a place that is not yours; not followed",
                file.display()
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_file_is_written_only_for_places_other_than_the_defaults() {
        let home = tempfile::tempdir().unwrap();
        let defaults = Paths::default_for_home(home.path());
        keep_location(home.path(), &defaults).unwrap();
        assert!(!default_state(home.path()).join(LOCATION_FILE).exists());
        let custom = Paths {
            records: home.path().join("my-state/better-os/fixes.json"),
            user_flatpak: home.path().join("my-data/flatpak"),
            system_flatpak: PathBuf::from("/var/lib/flatpak"),
        };
        keep_location(home.path(), &custom).unwrap();
        assert_eq!(for_removal(home.path()), (custom.clone(), None));
        keep_location(home.path(), &defaults).unwrap();
        assert_eq!(for_removal(home.path()), (defaults, None));
    }

    #[test]
    fn a_location_that_is_not_the_person_s_is_not_followed() {
        let home = tempfile::tempdir().unwrap();
        let defaults = Paths::default_for_home(home.path());
        for records in [
            // Root's, not the person's.
            PathBuf::from("/etc/better-os/fixes.json"),
            home.path().join("../escape/fixes.json"),
            PathBuf::from("relative/fixes.json"),
        ] {
            let custom = Paths {
                records,
                ..defaults.clone()
            };
            keep_location(home.path(), &custom).unwrap();
            let (paths, warning) = for_removal(home.path());
            assert_eq!(paths, defaults);
            assert!(warning.is_some());
        }
        // A link inside the home that leads to root's directory is not
        // followed.
        std::os::unix::fs::symlink("/etc", home.path().join("link")).unwrap();
        assert!(!followable(
            &home.path().join("link/fixes.json"),
            home.path()
        ));
        assert!(followable(&home.path().join("a/b/fixes.json"), home.path()));
    }

    #[test]
    fn a_location_outside_the_home_that_is_the_person_s_is_followed() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let custom = Paths {
            records: data.path().join("state/better-os/fixes.json"),
            user_flatpak: data.path().join("share/flatpak"),
            system_flatpak: PathBuf::from("/var/lib/flatpak"),
        };
        keep_location(home.path(), &custom).unwrap();
        assert_eq!(for_removal(home.path()), (custom, None));
    }

    #[test]
    fn a_fixture_keeps_everything_under_its_root() {
        let paths = Paths::under(Path::new("/scratch"));
        assert!(paths.user_flatpak.starts_with("/scratch"));
        assert!(paths.system_flatpak.starts_with("/scratch"));
        assert!(paths.records.starts_with("/scratch"));
        let app = patch_core::flatpak::AppId::parse("org.example.App").unwrap();
        assert_eq!(
            paths.user_override(&app),
            Path::new("/scratch/.local/share/flatpak/overrides/org.example.App")
        );
    }
}
