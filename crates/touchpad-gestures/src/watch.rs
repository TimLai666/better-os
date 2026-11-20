//! Following the gesture configuration as other processes write it (ticket
//! 131).
//!
//! The gesture service and every window that shows gestures follow
//! `gestures.json` and the safe mode marker through this one watch, so a
//! change made anywhere reaches everything that holds a copy. A file watch
//! rather than a "reload" call between them, because the file is where every
//! writer meets: Better Touchpad's window, Better Manager's Touchpad page, an
//! import, safe mode from a text console, and a person with an editor. A call
//! would have to be added to each, and the next writer could forget it.
//!
//! Two things can take the watch away, and neither may cost the gestures:
//!
//! - It cannot be made at all, for instance when the person's
//!   `fs.inotify.max_user_instances` is used up. The owner of the watch keeps
//!   working on what it read, hears [`WatchState::Unavailable`] with the
//!   reason, and the watch is tried again on a slow timer.
//! - The directory is deleted (and perhaps recreated). The kernel drops a
//!   watch on a deleted directory, so the watch is made again on the
//!   directory as it is now, and the owner is told to read again.
//!
//! The writes are atomic renames, so the directory is watched and events for
//! other names are dropped.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// How long a watch that could not be made waits before it is tried again.
/// Slow on purpose: what usually stops it is a per-user kernel limit, and
/// asking every second would not free one any sooner.
pub const RETRY: Duration = Duration::from_secs(60);

/// Whether the watch is working.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WatchState {
    Watching,
    /// Not watching, with the reason. Changes are read when it comes back.
    Unavailable(String),
}

/// A watch on files in one directory, kept alive by a thread of its own and
/// ended when this is dropped.
pub struct ConfigurationWatch {
    stop: Arc<AtomicBool>,
    wake: mpsc::Sender<()>,
}

impl Drop for ConfigurationWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.wake.send(());
    }
}

impl ConfigurationWatch {
    /// Starts watching `files`, which must all be in one directory.
    ///
    /// `on_change` is called from the watch's thread on every event that
    /// touches one of them, and once each time the watch is (re)made, since
    /// something may have changed while it was not there. `on_state` hears
    /// every change of [`WatchState`]. `retry` is how long a watch that could
    /// not be made waits before it is tried again ([`RETRY`] in a service).
    pub fn start(
        files: &[PathBuf],
        retry: Duration,
        on_change: impl Fn() + Send + Sync + 'static,
        on_state: impl Fn(WatchState) + Send + 'static,
    ) -> Self {
        let directory = files
            .first()
            .and_then(|file| file.parent())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let names: Vec<OsString> = files
            .iter()
            .filter_map(|file| file.file_name().map(|name| name.to_os_string()))
            .collect();
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, woken) = mpsc::channel();
        let on_change = Arc::new(on_change);
        let thread_stop = stop.clone();
        let rearm = wake.clone();
        std::thread::spawn(move || {
            let mut last: Option<WatchState> = None;
            let mut tell = |state: WatchState| {
                if last.as_ref() != Some(&state) {
                    on_state(state.clone());
                    last = Some(state);
                }
            };
            while !thread_stop.load(Ordering::SeqCst) {
                match make(&directory, names.clone(), on_change.clone(), rearm.clone()) {
                    Ok(watcher) => {
                        tell(WatchState::Watching);
                        on_change();
                        // Until the directory goes away or the watch is
                        // dropped.
                        let _ = woken.recv();
                        drop(watcher);
                        if thread_stop.load(Ordering::SeqCst) {
                            return;
                        }
                        tell(WatchState::Unavailable(format!(
                            "{} was removed; watching it again",
                            directory.display()
                        )));
                    }
                    Err(error) => {
                        tell(WatchState::Unavailable(error));
                        // A slow timer, woken early only by a drop.
                        let _ = woken.recv_timeout(retry);
                    }
                }
            }
        });
        Self { stop, wake }
    }
}

/// Makes the watch, or says why it could not be made.
fn make(
    directory: &Path,
    names: Vec<OsString>,
    on_change: Arc<dyn Fn() + Send + Sync>,
    rearm: mpsc::Sender<()>,
) -> Result<notify::RecommendedWatcher, String> {
    use notify::{RecursiveMode, Watcher as _};
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let watched = directory.to_path_buf();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            // An error from the watcher is a reason to read again.
            Err(_) => {
                on_change();
                return;
            }
        };
        // The directory itself went away or was moved: the kernel has
        // dropped the watch with it (IN_DELETE_SELF, IN_IGNORED).
        let gone = event.paths.iter().any(|path| path == &watched)
            && matches!(
                event.kind,
                notify::EventKind::Remove(_)
                    | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
            );
        if gone {
            let _ = rearm.send(());
            return;
        }
        let touches = event.need_rescan()
            || event.paths.iter().any(|path| {
                path.file_name()
                    .is_some_and(|name| names.iter().any(|known| known == name))
            });
        if touches && !event.kind.is_access() {
            on_change();
        }
    })
    .map_err(|error| format!("the settings cannot be watched: {error}"))?;
    watcher
        .watch(directory, RecursiveMode::NonRecursive)
        .map_err(|error| format!("the settings cannot be watched: {error}"))?;
    Ok(watcher)
}
