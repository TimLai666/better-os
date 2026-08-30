//! Change watching for application directories.
//!
//! The catalog must notice an installed or removed application without asking
//! the filesystem every few seconds. `notify`'s recommended backend on Linux
//! is inotify, which delivers events; the watcher reports which backend it got
//! so a caller can prove it is not polling rather than assume it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind};

use crate::PlatformError;
use crate::discovery::ApplicationDirectories;

/// How the watcher learns about changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchBackend {
    /// The kernel delivers events. Nothing runs while the directories are
    /// idle.
    EventDriven,
    /// No event source was available and the backend re-reads directories on a
    /// timer. Reported, never chosen silently.
    Polling,
}

/// What changed. The watcher deliberately does not say which record changed:
/// precedence means one file appearing can change or reveal a different
/// application, so the answer is always to reload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogChange {
    pub paths: Vec<PathBuf>,
}

/// Watches every application directory that exists.
pub struct CatalogWatcher {
    watcher: RecommendedWatcher,
    receiver: Receiver<CatalogChange>,
    watched: Vec<PathBuf>,
}

impl CatalogWatcher {
    /// Starts watching. Directories that do not exist yet are skipped; a user
    /// application directory is usually absent until something writes one.
    pub fn new(directories: &ApplicationDirectories) -> Result<Self, PlatformError> {
        let (sender, receiver) = channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            forward(&sender, event);
        })
        .map_err(|error| PlatformError::WatchFailed(error.to_string()))?;
        let mut watched = Vec::new();
        for directory in directories.directories() {
            if !directory.path.is_dir() {
                continue;
            }
            watcher
                .watch(&directory.path, RecursiveMode::Recursive)
                .map_err(|error| PlatformError::WatchFailed(error.to_string()))?;
            watched.push(directory.path.clone());
        }
        Ok(Self {
            watcher,
            receiver,
            watched,
        })
    }

    /// The backend `notify` selected on this host.
    pub fn backend(&self) -> WatchBackend {
        match RecommendedWatcher::kind() {
            WatcherKind::PollWatcher => WatchBackend::Polling,
            _ => WatchBackend::EventDriven,
        }
    }

    /// Directories actually being watched.
    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }

    /// Starts watching a directory that appeared after the watcher was built.
    pub fn watch_additional(&mut self, path: &Path) -> Result<(), PlatformError> {
        if self.watched.iter().any(|existing| existing == path) {
            return Ok(());
        }
        self.watcher
            .watch(path, RecursiveMode::Recursive)
            .map_err(|error| PlatformError::WatchFailed(error.to_string()))?;
        self.watched.push(path.to_path_buf());
        Ok(())
    }

    /// The change channel, for a caller that wants to select on it.
    pub fn changes(&self) -> &Receiver<CatalogChange> {
        &self.receiver
    }

    /// Waits for the next change, collapsing everything that arrives inside
    /// `settle` into one result. Installing a package writes many files; a
    /// consumer wants one reload, not forty.
    pub fn next_change(&self, timeout: Duration, settle: Duration) -> Option<CatalogChange> {
        let first = match self.receiver.recv_timeout(timeout) {
            Ok(change) => change,
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return None,
        };
        let mut paths = first.paths;
        while let Ok(next) = self.receiver.recv_timeout(settle) {
            paths.extend(next.paths);
        }
        paths.sort();
        paths.dedup();
        Some(CatalogChange { paths })
    }
}

/// Forwards only events that can change the catalog. An access-time event on
/// an unrelated file must not wake a consumer.
fn forward(sender: &Sender<CatalogChange>, event: notify::Result<Event>) {
    let Ok(event) = event else {
        return;
    };
    let interesting = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    );
    if !interesting {
        return;
    }
    let paths: Vec<PathBuf> = event
        .paths
        .into_iter()
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("desktop")
                || path.extension().is_none()
        })
        .collect();
    if paths.is_empty() {
        return;
    }
    let _ = sender.send(CatalogChange { paths });
}
