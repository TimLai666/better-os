//! Application metadata: where it comes from, and how the overlay learns it
//! changed.
//!
//! Both halves are delegation. Discovery is `app-catalog-platform::discover`
//! and watching is its `CatalogWatcher`, which uses inotify and reports which
//! backend it actually got rather than claiming to be event-driven. What this
//! module contributes is the pairing the overlay needs: a catalog and the
//! index built over it, produced together so the two can never describe
//! different sets of applications.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app_catalog_core::{Catalog, Locale};
use app_catalog_platform::{
    CatalogChange, CatalogWatcher, HostProbe, SessionEnvironment, WatchBackend, load_catalog,
};
use launcher_core::{IndexOptions, SearchIndex};

use crate::PlatformError;

/// How long a burst of filesystem events is allowed to settle into one reload.
/// Installing a package writes many files; the library should be redrawn once.
pub const SETTLE: Duration = Duration::from_millis(150);

/// One consistent view of the installed applications.
///
/// The catalog and the index are built from the same read. Launching looks a
/// record up in `catalog`; the overlay draws `index`. Handing them out
/// separately would let a click resolve against a different set of
/// applications than the one on screen.
#[derive(Clone, Debug)]
pub struct LauncherSnapshot {
    pub catalog: Arc<Catalog>,
    pub index: Arc<SearchIndex>,
}

impl LauncherSnapshot {
    /// The number of applications the overlay will show, which is the visible
    /// count and not the catalog's total.
    pub fn visible(&self) -> usize {
        self.index.len()
    }
}

/// Reads every application directory and builds the index over it.
///
/// Plain blocking I/O with no GPUI involvement, so the caller runs it on a
/// background thread. Hidden and desktop-incompatible entries are excluded by
/// the shared catalog and the index's own visibility check, never re-filtered
/// here.
pub fn load_snapshot(session: &SessionEnvironment, locale: Option<Locale>) -> LauncherSnapshot {
    let probe = HostProbe::from_env();
    let catalog = load_catalog(session, &probe);
    let index = SearchIndex::from_catalog(
        &catalog,
        &IndexOptions::new()
            .with_locale(locale)
            .with_environments(session.environments.clone()),
    );
    LauncherSnapshot {
        catalog: Arc::new(catalog),
        index: Arc::new(index),
    }
}

/// Watches the application directories the session actually has.
///
/// A thin wrapper so the overlay depends on one type instead of assembling a
/// watcher, and so the backend it got is a question the overlay can answer:
/// "no idle polling" is a claim that needs evidence.
pub struct MetadataWatch {
    watcher: CatalogWatcher,
}

impl MetadataWatch {
    pub fn start(session: &SessionEnvironment) -> Result<Self, PlatformError> {
        Ok(Self {
            watcher: CatalogWatcher::new(&session.directories)?,
        })
    }

    /// The directories being watched. Empty is possible and is not a failure:
    /// a session with no readable application directory has nothing to watch.
    pub fn watched(&self) -> &[PathBuf] {
        self.watcher.watched()
    }

    pub fn backend(&self) -> WatchBackend {
        self.watcher.backend()
    }

    /// Whether changes arrive as events rather than by re-reading on a timer.
    pub fn is_event_driven(&self) -> bool {
        self.backend() == WatchBackend::EventDriven
    }

    /// Blocks until something changed or `timeout` elapsed, collapsing a burst
    /// into one answer. Meant for a background thread; the overlay is told
    /// about the reload, it does not wait for it.
    pub fn next_change(&self, timeout: Duration) -> Option<CatalogChange> {
        self.watcher.next_change(timeout, SETTLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_catalog_platform::ApplicationDirectories;
    use std::fs;

    fn session_at(root: &std::path::Path) -> SessionEnvironment {
        SessionEnvironment {
            directories: ApplicationDirectories::from_values(
                Some(&root.join("home")),
                None,
                Some(root.join("system").to_str().unwrap()),
            ),
            ..SessionEnvironment::default()
        }
    }

    #[test]
    fn a_snapshot_indexes_exactly_what_the_catalog_shows() {
        let directory = tempfile::tempdir().unwrap();
        let applications = directory.path().join("system/applications");
        fs::create_dir_all(&applications).unwrap();
        fs::write(
            applications.join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor\n",
        )
        .unwrap();
        fs::write(
            applications.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=hidden\nHidden=true\n",
        )
        .unwrap();

        let snapshot = load_snapshot(&session_at(directory.path()), None);
        assert_eq!(snapshot.visible(), 1);
        assert_eq!(
            snapshot.index.browse().applications()[0].display_name,
            "Editor"
        );
        assert_eq!(
            snapshot.catalog.hidden().len(),
            1,
            "the hidden entry is excluded by the shared catalog, not re-filtered here"
        );
    }

    #[test]
    fn watching_reports_the_backend_rather_than_claiming_to_be_event_driven() {
        let directory = tempfile::tempdir().unwrap();
        let applications = directory.path().join("system/applications");
        fs::create_dir_all(&applications).unwrap();

        let watch = MetadataWatch::start(&session_at(directory.path())).unwrap();
        assert_eq!(watch.watched(), [applications.as_path()]);
        assert_eq!(
            watch.is_event_driven(),
            watch.backend() == WatchBackend::EventDriven
        );
    }

    #[test]
    fn installing_an_application_is_noticed_and_changes_the_next_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let applications = directory.path().join("system/applications");
        fs::create_dir_all(&applications).unwrap();
        let session = session_at(directory.path());

        let watch = MetadataWatch::start(&session).unwrap();
        assert_eq!(load_snapshot(&session, None).visible(), 0);

        fs::write(
            applications.join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor\n",
        )
        .unwrap();

        let change = watch.next_change(Duration::from_secs(5));
        assert!(change.is_some(), "the watcher must notice a new entry");
        assert_eq!(load_snapshot(&session, None).visible(), 1);
    }

    #[test]
    fn a_directory_that_does_not_exist_is_skipped_rather_than_failing_the_watch() {
        let directory = tempfile::tempdir().unwrap();
        let watch = MetadataWatch::start(&session_at(directory.path())).unwrap();
        assert!(watch.watched().is_empty());
    }
}
