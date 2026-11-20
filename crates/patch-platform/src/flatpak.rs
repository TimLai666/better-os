//! Installed Flatpak applications and the person's override files, read
//! from disk the way Flatpak lays them out, and the one write a fix makes.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use patch_core::ChangeError;
use patch_core::ReadError;
use patch_core::flatpak::{AppId, Installation, InstalledApp, OverrideLayers};

use crate::paths::Paths;

fn read_optional(path: &Path) -> Result<Option<String>, ReadError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ReadError::Failed(format!("{}: {error}", path.display()))),
    }
}

/// The applications of one installation, in directory order sorted by ID.
fn apps_in(
    installation_dir: &Path,
    installation: Installation,
    paths: &Paths,
) -> Result<Vec<InstalledApp>, ReadError> {
    let app_dir = installation_dir.join("app");
    let entries = match std::fs::read_dir(&app_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ReadError::Failed(format!("{}: {error}", app_dir.display())));
        }
    };
    let mut ids: Vec<AppId> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| AppId::parse(name).ok())
        })
        .collect();
    ids.sort();
    let mut apps = Vec::new();
    for id in ids {
        let active = app_dir.join(id.as_str()).join("current/active");
        // An app with no deployed metadata is half installed or being
        // removed; Flatpak would not run it either.
        let Some(metadata) = read_optional(&active.join("metadata"))? else {
            continue;
        };
        let desktop_name = format!("{}.desktop", id.as_str());
        let desktop_entry =
            match read_optional(&active.join("export/share/applications").join(&desktop_name))? {
                Some(entry) => Some(entry),
                None => read_optional(
                    &installation_dir
                        .join("exports/share/applications")
                        .join(&desktop_name),
                )?,
            };
        let overrides = OverrideLayers {
            system_global: read_optional(&paths.system_flatpak.join("overrides/global"))?,
            system_app: read_optional(&paths.system_flatpak.join("overrides").join(id.as_str()))?,
            user_global: read_optional(&paths.user_flatpak.join("overrides/global"))?,
            user_app: read_optional(&paths.user_override(&id))?,
        };
        let electron_version = electron_version(&active.join("files"));
        apps.push(InstalledApp {
            id,
            installation,
            metadata,
            desktop_entry,
            overrides,
            electron_version,
        });
    }
    Ok(apps)
}

/// The Electron an app ships: the `version` file Electron's release puts
/// beside its binary and its `resources.pak`, looked for in the app's files
/// and up to two folders down (`/app/<name>/`, `/app/lib/<name>/`), in name
/// order. `None` when there is none, or it does not hold a version.
pub fn electron_version(files: &Path) -> Option<String> {
    let mut candidates = vec![files.to_path_buf()];
    let mut level = vec![files.to_path_buf()];
    for _ in 0..2 {
        let mut next = Vec::new();
        for directory in &level {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect();
            found.sort();
            next.extend(found);
        }
        candidates.extend(next.iter().cloned());
        level = next;
    }
    candidates.into_iter().find_map(|directory| {
        if !directory.join("resources.pak").is_file() {
            return None;
        }
        let text = read_small(&directory.join("version"))?;
        let version = text.lines().next()?.trim().to_string();
        patch_core::flatpak::electron_major(&version).map(|_| version)
    })
}

/// A file of at most a few bytes, as text.
fn read_small(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(64)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// Every installed application, the person's installation first, so an
/// app installed twice is the one Flatpak runs by default for them.
pub fn installed_apps(paths: &Paths) -> Result<Vec<InstalledApp>, ReadError> {
    let mut apps = apps_in(&paths.user_flatpak, Installation::User, paths)?;
    apps.extend(apps_in(&paths.system_flatpak, Installation::System, paths)?);
    Ok(apps)
}

/// The person's override file for an app; `None` when there is none.
pub fn read_user_override(paths: &Paths, app: &AppId) -> Result<Option<String>, ReadError> {
    read_optional(&paths.user_override(app))
}

fn temporary_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.better-os-{}.tmp", std::process::id()))
}

fn failed(path: &Path, error: std::io::Error) -> ChangeError {
    ChangeError::Failed(format!("{}: {error}", path.display()))
}

/// Replaces (or, with `contents` `None`, removes) the person's override
/// file for an app, only while it still holds `expected`. The new text is
/// written beside it and renamed over it, so Flatpak never reads half a
/// file. A link where the file should be is refused: writing through it
/// would change a file somewhere else.
pub fn write_user_override(
    paths: &Paths,
    app: &AppId,
    expected: Option<&str>,
    contents: Option<&str>,
) -> Result<(), ChangeError> {
    let path = paths.user_override(app);
    if let Ok(metadata) = std::fs::symlink_metadata(&path)
        && !metadata.file_type().is_file()
    {
        return Err(ChangeError::Refused(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let now = read_optional(&path).map_err(|error| ChangeError::Failed(error.to_string()))?;
    if now.as_deref() != expected {
        return Err(ChangeError::ChangedSinceRead);
    }
    match contents {
        None => match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(failed(&path, error)),
        },
        Some(text) => {
            let directory = path.parent().expect("an override has a directory");
            std::fs::create_dir_all(directory).map_err(|error| failed(directory, error))?;
            let temporary = temporary_beside(&path);
            // The file keeps the permissions it had.
            let permissions = std::fs::metadata(&path).ok().map(|m| m.permissions());
            let written = (|| {
                let mut file = std::fs::File::create(&temporary)?;
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                if let Some(permissions) = permissions {
                    std::fs::set_permissions(&temporary, permissions)?;
                }
                std::fs::rename(&temporary, &path)
            })();
            if let Err(error) = written {
                let _ = std::fs::remove_file(&temporary);
                return Err(failed(&path, error));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppId {
        AppId::parse(name).unwrap()
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn apps_are_read_from_both_installations_with_their_layers() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let system = paths
            .system_flatpak
            .join("app/org.example.Electron/current/active");
        put(
            &system.join("metadata"),
            "[Application]\nname=org.example.Electron\n",
        );
        put(
            &paths
                .system_flatpak
                .join("exports/share/applications/org.example.Electron.desktop"),
            "[Desktop Entry]\nName=Example\n",
        );
        put(
            &paths.system_flatpak.join("overrides/global"),
            "[Context]\nsockets=x11;\n",
        );
        put(
            &paths.user_override(&app("org.example.Electron")),
            "[Environment]\nA=1\n",
        );
        let user = paths
            .user_flatpak
            .join("app/org.example.Mine/current/active");
        put(
            &user.join("metadata"),
            "[Application]\nname=org.example.Mine\n",
        );
        // Not an app ID: skipped, never read.
        put(
            &paths
                .user_flatpak
                .join("app/not-an-id/current/active/metadata"),
            "x",
        );
        // Half installed: no metadata.
        std::fs::create_dir_all(paths.user_flatpak.join("app/org.example.Half")).unwrap();

        let apps = installed_apps(&paths).unwrap();
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["org.example.Mine", "org.example.Electron"]);
        assert_eq!(apps[0].installation, Installation::User);
        let electron = &apps[1];
        assert_eq!(
            electron.desktop_entry.as_deref(),
            Some("[Desktop Entry]\nName=Example\n")
        );
        assert_eq!(
            electron.overrides.system_global.as_deref(),
            Some("[Context]\nsockets=x11;\n")
        );
        assert_eq!(
            electron.overrides.user_app.as_deref(),
            Some("[Environment]\nA=1\n")
        );
        assert_eq!(electron.overrides.system_app, None);
    }

    #[test]
    fn the_electron_version_is_read_beside_its_resources() {
        let root = tempfile::tempdir().unwrap();
        let files = root.path().join("files");
        put(&files.join("github-desktop/version"), "32.3.0");
        put(&files.join("github-desktop/resources.pak"), "");
        // A version file with no Electron beside it is not Electron's.
        put(&files.join("aaa/version"), "1.0");
        assert_eq!(electron_version(&files).as_deref(), Some("32.3.0"));
        put(&files.join("lib/app/version"), "38.1.2\n");
        put(&files.join("lib/app/resources.pak"), "");
        std::fs::remove_dir_all(files.join("github-desktop")).unwrap();
        assert_eq!(electron_version(&files).as_deref(), Some("38.1.2"));
        put(&files.join("lib/app/version"), "not a version");
        assert_eq!(electron_version(&files), None);
        assert_eq!(electron_version(&root.path().join("missing")), None);
    }

    #[test]
    fn no_flatpak_at_all_is_no_apps() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            installed_apps(&Paths::under(root.path()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_write_happens_only_while_the_file_holds_what_was_read() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let id = app("org.example.App");
        write_user_override(&paths, &id, None, Some("[Context]\nsockets=x11;\n")).unwrap();
        assert_eq!(
            read_user_override(&paths, &id).unwrap().as_deref(),
            Some("[Context]\nsockets=x11;\n")
        );
        // Someone wrote it in between.
        assert_eq!(
            write_user_override(&paths, &id, None, Some("other")),
            Err(ChangeError::ChangedSinceRead)
        );
        write_user_override(&paths, &id, Some("[Context]\nsockets=x11;\n"), None).unwrap();
        assert_eq!(read_user_override(&paths, &id).unwrap(), None);
        // Nothing is left beside it.
        let left: Vec<_> = std::fs::read_dir(paths.user_flatpak.join("overrides"))
            .unwrap()
            .collect();
        assert!(left.is_empty());
    }

    #[test]
    fn a_rewrite_keeps_the_file_s_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let id = app("org.example.App");
        write_user_override(&paths, &id, None, Some("[A]\nk=1\n")).unwrap();
        let path = paths.user_override(&id);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_user_override(&paths, &id, Some("[A]\nk=1\n"), Some("[A]\nk=2\n")).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_link_where_the_file_should_be_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        let id = app("org.example.App");
        let elsewhere = root.path().join("elsewhere");
        std::fs::write(&elsewhere, "keep").unwrap();
        let link = paths.user_override(&id);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        assert!(matches!(
            write_user_override(&paths, &id, Some("keep"), Some("x")),
            Err(ChangeError::Refused(_))
        ));
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "keep");
    }
}
