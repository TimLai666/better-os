//! Better Files: the window, and the view models behind it.
//!
//! The crate is split so that almost none of it needs a display server. The
//! modules listed first hold every decision — what a sidebar row is, what a
//! keystroke means, which controls a job offers, what a bookmark file says —
//! and none of them mentions GPUI. [`app`] and its rendering siblings are the
//! thin part on top.
//!
//! Two rules from Issue #6 are held by construction rather than by discipline.
//!
//! **The GUI never runs as root.** [`refuse_root`] is called before a window is
//! opened and the process exits if it is running as uid 0. There is no code
//! path that elevates.
//!
//! **Operations are not tied to a window.** [`shared_engine`] hands out one
//! `Arc<JobEngine>` for the whole process. Closing a window drops a
//! [`session::FilesSession`]; the engine, its worker threads, and its running
//! jobs are untouched.

pub mod apps;
pub mod bookmarks;
pub mod columns;
pub mod commands;
pub mod content;
pub mod context_menu;
pub mod devicelink;
pub mod devices;
pub mod errors;
pub mod filemanager1;
pub mod format;
pub mod i18n;
pub mod icons;
pub mod interrupted;
pub mod keys;
pub mod launch;
pub mod layout;
pub mod localsend;
pub mod network;
pub mod opcenter;
pub mod openwith;
pub mod pathbar;
pub mod plugin_host;
pub mod plugins;
pub mod policy;
pub mod prefs;
pub mod preview;
pub mod reader;
pub mod restart;
pub mod search;
pub mod session;
pub mod settings_store;
pub mod share;
pub mod shortcuts;
pub mod sidebar;
pub mod terminal;
pub mod toolbar;
pub mod tracking;
pub mod undo;

mod app;
mod panels;
mod plugin_page;
mod render;
mod shell;
mod views;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use files_operations::{EngineConfig, JobEngine, JobStore};

/// The one job engine this process runs.
///
/// It is created on first use and never dropped, so a window closing cannot
/// take a running copy with it. Two windows submitting at once share the same
/// queue and the same worker pool, which is also what makes the operation
/// center in either window show every job rather than only its own.
///
/// Every job it runs is reported to [`shared_tracker`], which is how a copy to
/// an external disk reaches the storage layer as a tracked operation.
pub fn shared_engine() -> Arc<JobEngine> {
    ENGINE.get_or_init(start_engine).0.clone()
}

/// The copies and moves an earlier Better Files left unfinished, read once
/// when the engine starts (before it writes a record of its own, so none
/// of this process's jobs can be mistaken for an interrupted one).
pub fn interrupted_jobs() -> interrupted::InterruptedJobs {
    ENGINE.get_or_init(start_engine).1.clone()
}

static ENGINE: OnceLock<(Arc<JobEngine>, interrupted::InterruptedJobs)> = OnceLock::new();

fn start_engine() -> (Arc<JobEngine>, interrupted::InterruptedJobs) {
    let store = JobStore::new(job_store_root());
    let interrupted =
        interrupted::InterruptedJobs::from_recovery(store.recover(), Some(store.clone()));
    let engine = Arc::new(JobEngine::with_observer(
        EngineConfig {
            store: Some(store),
            ..EngineConfig::default()
        },
        shared_tracker(),
    ));
    (engine, interrupted)
}

/// The one job tracker this process runs, beside the one engine. The window
/// attaches its storage link to it when it opens.
pub fn shared_tracker() -> Arc<tracking::StorageTracker> {
    static TRACKER: OnceLock<Arc<tracking::StorageTracker>> = OnceLock::new();
    TRACKER
        .get_or_init(|| Arc::new(tracking::StorageTracker::from_env()))
        .clone()
}

/// Where job records live: `$XDG_DATA_HOME/better-os/files/jobs`, the same
/// convention `defaults-store` uses for its snapshots.
fn job_store_root() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        })
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    data.join("better-os/files/jobs")
}

/// Refuses to run as root.
///
/// Issue #6 states it flatly: the GUI must not run as root. A file manager
/// running as uid 0 turns every accidental drag into a system change, and
/// there is nothing it could do with those privileges that belongs in a GUI
/// rather than behind a narrow service boundary.
pub fn refuse_root() -> Result<(), &'static str> {
    // Reading the effective uid needs no dependency: the kernel publishes it.
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let effective = rest
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u32>().ok());
            if effective == Some(0) {
                return Err("better-files must not run as root");
            }
        }
    }
    Ok(())
}

pub use app::run;
