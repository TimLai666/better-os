//! The host half of the shared application catalog.
//!
//! `app-catalog-core` decides what an application record is. This crate is the
//! only place that touches the host to produce one: it finds the XDG
//! application directories, reads them, resolves executables against the real
//! `PATH`, watches for changes, and starts processes. There is no GPUI
//! dependency here either, so discovery and normalization can run on any
//! thread a consumer chooses.

#[cfg(feature = "dbus-activation")]
pub mod activation;
pub mod discovery;
pub mod launch;
pub mod watch;

use std::path::{Path, PathBuf};

use app_catalog_core::{Catalog, DesktopEnvironments, ExecutableProbe, LaunchError, Locale};
use thiserror::Error;

pub use discovery::{ApplicationDirectories, ApplicationDirectory, discover};
pub use launch::{
    DesktopActivator, LaunchOutcome, Launcher, ProcessSpawner, RecordingActivator,
    RecordingSpawner, SystemSpawner, TerminalCommand,
};
pub use watch::{CatalogChange, CatalogWatcher, WatchBackend};

/// Host-side failures. Every variant renders as a stable machine key.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum PlatformError {
    #[error("{0}")]
    Launch(#[from] LaunchError),
    #[error("catalog.platform.error.spawn_failed:{program}:{reason}")]
    SpawnFailed { program: String, reason: String },
    #[error("catalog.platform.error.watch_failed:{0}")]
    WatchFailed(String),
    #[error("catalog.platform.error.activation_failed:{0}")]
    ActivationFailed(String),
}

/// Resolves program names against the real `PATH`.
///
/// A name is resolved only when a regular, executable file is actually found.
/// Nothing here ever returns a constructed path that was not checked, which is
/// what keeps `ExecutableStatus::Resolved` meaningful.
#[derive(Clone, Debug, Default)]
pub struct HostProbe {
    path_entries: Vec<PathBuf>,
}

impl HostProbe {
    pub fn from_env() -> Self {
        Self::with_path(std::env::var("PATH").ok().as_deref())
    }

    pub fn with_path(path: Option<&str>) -> Self {
        let path_entries = path
            .unwrap_or_default()
            .split(':')
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .filter(|entry| entry.is_absolute())
            .collect();
        Self { path_entries }
    }
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

impl ExecutableProbe for HostProbe {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        if program.is_empty() {
            return None;
        }
        if program.contains('/') {
            let path = Path::new(program);
            // A relative program name is resolved by the shell against a
            // working directory this process does not control, so it is not
            // resolved at all rather than guessed.
            if path.is_absolute() && is_executable_file(path) {
                return Some(path.to_path_buf());
            }
            return None;
        }
        self.path_entries
            .iter()
            .map(|entry| entry.join(program))
            .find(|candidate| is_executable_file(candidate))
    }
}

/// Everything about the session that changes what the catalog answers.
#[derive(Clone, Debug, Default)]
pub struct SessionEnvironment {
    pub directories: ApplicationDirectories,
    pub environments: DesktopEnvironments,
    pub locale: Option<Locale>,
}

impl SessionEnvironment {
    /// Reads the session from the process environment.
    pub fn from_env() -> Self {
        Self {
            directories: ApplicationDirectories::from_env(),
            environments: DesktopEnvironments::parse(
                &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            ),
            locale: std::env::var("LC_MESSAGES")
                .or_else(|_| std::env::var("LANG"))
                .ok()
                .as_deref()
                .and_then(Locale::parse),
        }
    }
}

/// Reads every application directory and returns the assembled catalog.
///
/// This is plain blocking I/O with no GPUI involvement, so a consumer is free
/// to run it on a background thread; nothing here requires or touches a render
/// thread.
pub fn load_catalog(session: &SessionEnvironment, probe: &dyn ExecutableProbe) -> Catalog {
    discover(&session.directories, probe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn resolves_a_program_from_path_only_when_it_is_executable() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("real-tool");
        fs::write(&executable, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let plain = directory.path().join("not-executable");
        fs::write(&plain, b"data").unwrap();

        let probe = HostProbe::with_path(Some(directory.path().to_str().unwrap()));
        assert_eq!(probe.resolve("real-tool"), Some(executable.clone()));
        assert_eq!(probe.resolve("not-executable"), None);
        assert_eq!(probe.resolve("absent-tool"), None);
        assert_eq!(
            probe.resolve(executable.to_str().unwrap()),
            Some(executable)
        );
        assert_eq!(probe.resolve("./real-tool"), None);
        assert_eq!(probe.resolve(""), None);
    }

    #[test]
    fn a_relative_path_entry_is_ignored() {
        let probe = HostProbe::with_path(Some("relative/bin::/nonexistent-absolute"));
        assert_eq!(
            probe.path_entries,
            vec![PathBuf::from("/nonexistent-absolute")]
        );
    }
}
