//! What Better Files carries over when an update restarts it (ticket 105),
//! with no GPUI in it.
//!
//! The view is the open tabs and which one is in front, the entries selected
//! in it, the search being typed, and whether the operations panel is open.
//! A location is carried as the URI [`files_core::Location::to_uri`] writes,
//! so it reads back through the same parser a saved tab would. A device
//! location cannot be rebuilt from its URI alone and is left out rather than
//! guessed at; the tab it was in is not reopened.
//!
//! A restart waits while a file operation is queued or running: the job
//! engine belongs to this process, and a copy cut off by a restart would be
//! left half done. It also waits while a dialog, the Open With chooser, or a
//! confirmation is open, because what was chosen there has no saved form.

use better_ui::restart::Blocker;
use files_core::{EntryId, Location};
use serde::{Deserialize, Serialize};

use crate::session::FilesSession;

/// The session's part of the window's view.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionView {
    /// Each tab's location, left to right.
    pub tabs: Vec<String>,
    pub active: usize,
    /// The names selected in the front tab, the cursor first.
    pub selected: Vec<String>,
    pub search: String,
    pub operations_open: bool,
}

/// Everything the window puts back.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesView {
    pub session: SessionView,
    /// The path field's text, when it was being edited.
    pub path_draft: Option<String>,
    /// How far the content list was scrolled, in pixels.
    pub scroll: f32,
}

/// A carried location that can be opened again.
fn reopenable(uri: &str) -> Option<Location> {
    match Location::parse_uri(uri) {
        Location::Unsupported(_) | Location::Device(_) => None,
        location => Some(location),
    }
}

static OPEN_WINDOWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One Better Files window counted among the process's open windows, until it
/// is dropped with the window. Several windows share one process (Ctrl+N,
/// "Show in folder"), and a restart replaces the process, so it waits until
/// one window is left; that window carries its view over.
#[derive(Debug)]
pub struct WindowCount(());

impl WindowCount {
    pub fn open() -> WindowCount {
        OPEN_WINDOWS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        WindowCount(())
    }

    /// Whether any window besides one is open.
    pub fn others_open() -> bool {
        OPEN_WINDOWS.load(std::sync::atomic::Ordering::SeqCst) > 1
    }
}

impl Drop for WindowCount {
    fn drop(&mut self) {
        OPEN_WINDOWS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

static SENDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many sends to another machine (Taildrop, LocalSend) are under way in
/// this process. A send runs on its own thread, so a restart would cut it off;
/// it holds the restart like a file operation does. The count is the
/// process's, not a window's: every Better Files window in the process shares
/// the restart, so closing the window that started a send must not let
/// another window restart under it.
pub struct SendCounter;

/// One send under way, counted until it is dropped — when the send's thread
/// ends, however it ends.
pub struct SendGuard(());

impl SendCounter {
    pub fn begin() -> SendGuard {
        SENDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        SendGuard(())
    }

    pub fn in_progress() -> bool {
        SENDS.load(std::sync::atomic::Ordering::SeqCst) > 0
    }
}

impl Drop for SendGuard {
    fn drop(&mut self) {
        SENDS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl FilesSession {
    /// The view as it is now.
    pub fn view(&self) -> SessionView {
        let model = self.pane().model();
        let selection = model.selection();
        let name = |id: &EntryId| match id {
            EntryId::Name(name) => Some(name.clone()),
            _ => None,
        };
        let mut selected: Vec<String> = selection.cursor().and_then(name).into_iter().collect();
        for id in selection.ids() {
            if let Some(entry) = name(id)
                && !selected.contains(&entry)
            {
                selected.push(entry);
            }
        }
        let active = self.active_tab();
        let tabs = self
            .tabs()
            .tabs()
            .iter()
            .map(|tab| {
                if tab.id() == active {
                    self.location().to_uri()
                } else {
                    tab.location().to_uri()
                }
            })
            .collect();
        SessionView {
            tabs,
            active: self.tabs().active_index(),
            selected,
            search: self.search.text.clone(),
            operations_open: self.operations_open,
        }
    }

    /// Opens the carried tabs in place of the one the session started with.
    /// Returns false, and changes nothing, when none of them can be opened.
    pub fn restore_view(&mut self, view: &SessionView) -> bool {
        let tabs: Vec<(usize, Location)> = view
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, uri)| reopenable(uri).map(|location| (index, location)))
            .collect();
        if tabs.is_empty() {
            return false;
        }
        let started = self.active_tab();
        let mut front = None;
        for (position, (index, location)) in tabs.into_iter().enumerate() {
            let id = if position == 0 {
                self.navigate_to(location);
                started
            } else {
                // Each opens after the one before it: a tab opened in the
                // background goes next to the front one instead.
                self.open_tab(location, true)
            };
            if index == view.active {
                front = Some(id);
            }
        }
        self.activate_tab(front.unwrap_or(started));
        let mut names = view.selected.iter();
        if let Some(cursor) = names.next() {
            self.select_by_name(cursor);
            for name in names {
                self.pane_mut()
                    .model_mut()
                    .selection_mut()
                    .toggle(EntryId::Name(name.clone()));
            }
            // The cursor goes back on the first name.
            self.pane_mut()
                .model_mut()
                .selection_mut()
                .move_cursor(Some(EntryId::Name(cursor.clone())));
        }
        if !view.search.is_empty() {
            self.set_search_text(view.search.clone());
        }
        self.operations_open = view.operations_open;
        true
    }

    /// What a restart would lose, if anything: a file operation not yet
    /// finished, or a dialog, the Open With chooser, or a confirmation.
    pub fn restart_blocked(&self) -> Option<Blocker> {
        if self.jobs.iter().any(|job| !job.state.is_terminal()) {
            Some(Blocker::Task)
        } else if self.dialog.is_some()
            || self.chooser.is_some()
            || self.policy_confirmation.is_some()
        {
            Some(Blocker::Dialog)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SendCounter, WindowCount};

    #[test]
    fn a_restart_waits_until_one_window_is_left() {
        // The only test that opens windows, so the count starts at zero.
        let first = WindowCount::open();
        assert!(!WindowCount::others_open());
        let second = WindowCount::open();
        assert!(WindowCount::others_open());
        drop(first);
        assert!(!WindowCount::others_open(), "the one left may restart");
        drop(second);
    }

    /// The only test that starts sends, so the process's count starts at
    /// zero. The send outlives anything that could stand for the window that
    /// started it: only the send's own thread ending lets a restart through.
    #[test]
    fn a_send_is_counted_for_the_whole_process_until_its_thread_ends() {
        assert!(!SendCounter::in_progress());
        let first = SendCounter::begin();
        // A window that starts a send and is then closed: what it owned is
        // gone, the send's guard is not.
        let window = std::rc::Rc::new(());
        let second = SendCounter::begin();
        drop(window);
        assert!(SendCounter::in_progress());
        drop(first);
        assert!(SendCounter::in_progress(), "one is still going");
        let worker = std::thread::spawn(move || {
            let _guard = second;
            panic!("the send failed badly");
        });
        assert!(worker.join().is_err());
        assert!(
            !SendCounter::in_progress(),
            "a send that panicked is not counted"
        );
    }
}
