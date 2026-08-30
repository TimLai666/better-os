//! Starting an application, by delegation.
//!
//! There is one launch path in Better OS and it lives in
//! `app-catalog-platform`: it plans from the registered desktop definition,
//! passes an argument vector rather than a command string, honors D-Bus
//! activation, and wraps terminal entries in the terminal's own vector.
//! Nothing in this module reimplements any of that.
//!
//! What this module adds is the shape the overlay needs: the overlay holds a
//! desktop ID, not a record, and a failure has to reach the screen instead of
//! a log. [`ApplicationStarter`] is that shape, and it is a trait so a launch
//! failure can be tested without a machine that fails.

use std::sync::Arc;
use std::sync::Mutex;

use app_catalog_core::{Catalog, DesktopId, Locale};
use app_catalog_platform::launch::{Launcher, SystemSpawner, TerminalCommand};
use app_catalog_platform::{LaunchOutcome, ProcessSpawner};

use crate::PlatformError;

/// Starts one application by its desktop ID.
pub trait ApplicationStarter {
    fn start(&self, desktop_id: &DesktopId) -> Result<LaunchOutcome, PlatformError>;
}

/// The production starter: looks the record up in the catalog the overlay is
/// showing and hands it to the shared launch path.
#[derive(Clone, Debug)]
pub struct CatalogLauncher {
    catalog: Arc<Catalog>,
    locale: Option<Locale>,
    terminal: TerminalCommand,
}

impl CatalogLauncher {
    pub fn new(catalog: Arc<Catalog>, locale: Option<Locale>) -> Self {
        Self {
            catalog,
            locale,
            terminal: TerminalCommand::default(),
        }
    }

    pub fn with_terminal(mut self, terminal: TerminalCommand) -> Self {
        self.terminal = terminal;
        self
    }

    /// The same launch against an injected spawner, so the argument vector can
    /// be asserted without starting a process.
    pub fn start_with(
        &self,
        desktop_id: &DesktopId,
        spawner: &dyn ProcessSpawner,
    ) -> Result<LaunchOutcome, PlatformError> {
        let record = self
            .catalog
            .get(desktop_id)
            .ok_or_else(|| PlatformError::UnknownApplication(desktop_id.as_str().to_string()))?;
        Launcher::new(spawner)
            .with_terminal(self.terminal.clone())
            // The launcher opens applications, never files, so there are no
            // targets. An entry that wants a file gets none, which is what
            // opening it from a launcher means.
            .launch(record, None, &[], self.locale.as_ref())
            .map_err(PlatformError::from)
    }
}

impl ApplicationStarter for CatalogLauncher {
    fn start(&self, desktop_id: &DesktopId) -> Result<LaunchOutcome, PlatformError> {
        self.start_with(desktop_id, &SystemSpawner)
    }
}

/// Records what it was asked to start and answers with a fixed result. This is
/// how the overlay's launch-failure path is tested: the failure is arranged,
/// not waited for.
#[derive(Debug)]
pub struct RecordingStarter {
    started: Mutex<Vec<DesktopId>>,
    outcome: Result<LaunchOutcome, PlatformError>,
}

impl RecordingStarter {
    pub fn succeeding() -> Self {
        Self {
            started: Mutex::new(Vec::new()),
            outcome: Ok(LaunchOutcome::Started { processes: 1 }),
        }
    }

    pub fn failing(error: PlatformError) -> Self {
        Self {
            started: Mutex::new(Vec::new()),
            outcome: Err(error),
        }
    }

    pub fn started(&self) -> Vec<DesktopId> {
        self.started.lock().expect("starter lock").clone()
    }
}

impl ApplicationStarter for RecordingStarter {
    fn start(&self, desktop_id: &DesktopId) -> Result<LaunchOutcome, PlatformError> {
        self.started
            .lock()
            .expect("starter lock")
            .push(desktop_id.clone());
        self.outcome.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_catalog_core::{CatalogBuilder, DirectoryRank, EntryScope, NoProbe};
    use app_catalog_platform::launch::RecordingSpawner;
    use std::path::PathBuf;

    fn catalog() -> Arc<Catalog> {
        let mut builder = CatalogBuilder::new(&NoProbe);
        builder.add_entry(
            DesktopId::new("editor.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/editor.desktop"),
            &DirectoryRank {
                rank: 0,
                scope: EntryScope::System,
            },
            b"[Desktop Entry]\nType=Application\nName=Editor\nExec=editor --wait %F\n",
        );
        Arc::new(builder.build())
    }

    #[test]
    fn launching_delegates_to_the_shared_path_with_no_targets_and_no_shell() {
        let launcher = CatalogLauncher::new(catalog(), None);
        let spawner = RecordingSpawner::new();
        let outcome = launcher
            .start_with(&DesktopId::new("editor.desktop").unwrap(), &spawner)
            .unwrap();

        assert_eq!(outcome, LaunchOutcome::Started { processes: 1 });
        let calls = spawner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "editor");
        assert_eq!(calls[0].arguments, vec!["--wait"]);
    }

    #[test]
    fn an_application_removed_since_the_last_frame_is_reported_not_guessed() {
        let launcher = CatalogLauncher::new(catalog(), None);
        let spawner = RecordingSpawner::new();
        let error = launcher
            .start_with(&DesktopId::new("gone.desktop").unwrap(), &spawner)
            .unwrap_err();
        assert_eq!(
            error,
            PlatformError::UnknownApplication("gone.desktop".to_string())
        );
        assert!(spawner.calls().is_empty());
    }

    #[test]
    fn the_recording_starter_reports_the_failure_it_was_given() {
        let starter = RecordingStarter::failing(PlatformError::UnknownApplication(
            "editor.desktop".to_string(),
        ));
        let id = DesktopId::new("editor.desktop").unwrap();
        assert!(starter.start(&id).is_err());
        assert_eq!(starter.started(), vec![id]);
    }
}
