//! Where "Switch power mode with the charger" is kept: one small file in the
//! person's own config directory, written by the window and read by the
//! session service at each plug event.

use std::path::{Path, PathBuf};

use hardware_core::follow::{ChargerFollow, FILE_NAME};

/// `$XDG_CONFIG_HOME/better-os/power-follow.json`, or under `~/.config`.
/// `None` when neither is set: then nothing is stored and the switch is off.
pub fn path_for_user() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("better-os").join(FILE_NAME))
}

/// The stored choice. A missing file is the default (off); a file that does
/// not parse is reported, so the window can say so instead of showing "off".
pub fn load(path: &Path) -> Result<ChargerFollow, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ChargerFollow::default()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Writes the choice through a temporary file and a rename, so a reader
/// never sees half a file.
pub fn save(path: &Path, value: &ChargerFollow) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{}: no parent", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hardware_core::power::Profile;

    #[test]
    fn missing_is_off_and_a_saved_choice_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("better-os").join(FILE_NAME);
        assert_eq!(load(&path), Ok(ChargerFollow::default()));
        let chosen = ChargerFollow {
            enabled: true,
            plugged_in: Profile::Balanced,
            on_battery: Profile::PowerSaver,
        };
        save(&path, &chosen).unwrap();
        assert_eq!(load(&path), Ok(chosen));
    }

    #[test]
    fn a_broken_file_is_an_error_not_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, "{").unwrap();
        assert!(load(&path).is_err());
    }
}
