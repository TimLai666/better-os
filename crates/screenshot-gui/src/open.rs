//! Opening a saved picture or recording from its notification (spec S7, R8).
//!
//! The application is the person's default for the file's type, read from
//! `mimeapps.list` by `app-chooser-core` and launched from the shared
//! application catalog by `app-catalog-platform`'s launcher: the same reader
//! and launcher Better Files and the application chooser use, so there is no
//! second rule here. With no default (or one that is not installed), the file
//! goes to the portal's `OpenFile` with its descriptor, which asks. The
//! portal's `OpenURI` is never given a `file:` URI: xdg-desktop-portal 1.18
//! refuses one there (`src/open-uri.c` 441–444), without a word.

use std::path::Path;

use app_catalog_core::{DesktopId, LaunchTarget, Locale as EntryLocale, MimeType};
use app_catalog_platform::{HostProbe, Launcher, SessionEnvironment, SystemSpawner, load_catalog};
use app_chooser_core::AssociationStore;
use screenshot_core::words::Locale;
use zbus::blocking::Connection;

/// The default application for `mime`, as `mimeapps.list` names it.
pub fn default_application(mime: &MimeType) -> Option<DesktopId> {
    default_in(&AssociationStore::for_user().ok()?, mime)
}

fn default_in(store: &AssociationStore, mime: &MimeType) -> Option<DesktopId> {
    store.load().ok()?.associations().default_for(mime).cloned()
}

/// The desktop-entry locale the launched application's name is read in.
fn entry_locale(locale: Locale) -> Option<EntryLocale> {
    EntryLocale::parse(match locale {
        Locale::TraditionalChinese => "zh_TW",
        Locale::English => "en_US",
    })
}

/// Opens `path` (of type `mime`) with the default application, or through
/// the portal's `OpenFile` when there is none to launch.
pub fn open_file(
    connection: &Connection,
    path: &Path,
    mime: &str,
    locale: Locale,
) -> Result<(), String> {
    let launched = MimeType::parse(mime)
        .ok_or_else(|| format!("{mime} is not a type"))
        .and_then(|mime| {
            default_application(&mime).ok_or_else(|| format!("nothing opens {}", mime.as_str()))
        })
        .and_then(|id| {
            let catalog = load_catalog(&SessionEnvironment::from_env(), &HostProbe::from_env());
            let record = catalog
                .get(&id)
                .ok_or_else(|| format!("{} is not installed", id.as_str()))?;
            let target = LaunchTarget::path(path).map_err(|error| error.to_string())?;
            Launcher::new(&SystemSpawner)
                .launch(record, None, &[target], entry_locale(locale).as_ref())
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    match launched {
        Ok(()) => Ok(()),
        Err(reason) => {
            eprintln!("better-screenshot: {reason}; asking the portal to open the file");
            crate::service::open_through_portal(connection, path, "OpenFile")
        }
    }
}

/// The type a saved file of Better Screenshot's has.
pub fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("webm") => "video/webm",
        _ => "image/png",
    }
}

#[cfg(test)]
mod tests {
    use super::{default_in, mime_of};
    use app_catalog_core::MimeType;
    use app_chooser_core::AssociationStore;
    use std::path::Path;

    #[test]
    fn a_saved_file_has_the_type_it_was_written_as() {
        assert_eq!(mime_of(Path::new("/p/Screenshot.png")), "image/png");
        assert_eq!(mime_of(Path::new("/v/Screencast.webm")), "video/webm");
    }

    /// The default comes from the person's `mimeapps.list` through
    /// `app-chooser-core`, not from a table here.
    #[test]
    fn the_default_is_what_mimeapps_list_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("mimeapps.list"),
            "[Default Applications]\nimage/png=fixture-viewer.desktop\n",
        )
        .unwrap();
        let store = AssociationStore::new(
            dir.path().join("mimeapps.list"),
            dir.path().join("rollback"),
        );
        let png = MimeType::parse("image/png").unwrap();
        assert_eq!(
            default_in(&store, &png).map(|id| id.as_str().to_string()),
            Some("fixture-viewer.desktop".to_string())
        );
        let webm = MimeType::parse("video/webm").unwrap();
        assert_eq!(default_in(&store, &webm), None);
    }
}
