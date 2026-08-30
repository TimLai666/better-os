//! Everything Better Files does, with no GPUI in it.
//!
//! The window in [`crate::app`] owns one of these and draws it. Every command
//! — a keystroke, a click, a menu item — lands on a method here, so the
//! behaviour of the file manager is testable without a display server, and the
//! renderer has no decisions of its own to get wrong.
//!
//! Three things are deliberate about the shape.
//!
//! The job engine is an `Arc` handed in, not built here. One engine serves the
//! process, so closing a window drops a session and leaves every copy running
//! — which is Issue #6's "do not tie file operations to one window's lifetime"
//! held by ownership rather than by care.
//!
//! Navigation lives in `files_core::Pane`, one per tab. This module holds no
//! history of its own, starts no listing of its own, and cancels nothing of its
//! own; it calls the pane, which already cancels the abandoned listing before
//! it starts the next one.
//!
//! Nothing here blocks. [`FilesSession::pump`] drains whatever the reader has
//! produced and returns whether a redraw is needed, and job state is read from
//! the engine's snapshots rather than waited on.

use std::sync::Arc;

use files_core::{
    Entry, EntryId, LocalPath, Location, NavigationError, OpenRefusal, Pane, SortKey, SortOrder,
    TabId, TabSet, TrashLocation,
};
use files_operations::{
    ConflictDecision, DeleteConfirmation, DeleteTarget, JobEngine, JobId, JobSnapshot, JobSpec,
    OperationError,
};
use files_platform::{MountTable, UserDirectories};

use crate::bookmarks::{BookmarkFile, BookmarkStore, PinOutcome};
use crate::commands::{self, Clipboard, CommandRefusal};
use crate::content::{ContentView, NoHandlerReason, OpenOutcome, SelectionInput};
use crate::i18n::{Copy, Locale};
use crate::keys::{Command, Focus};
use crate::opcenter::{self, JobRow, SessionHistory};
use crate::prefs::{FilesPreferences, ItemScale, PreferenceStore, ViewMode};
use crate::reader::FilesReader;
use crate::toolbar::{PathRejection, PathValidator, resolve_path_input};

/// Something the window says once, and stops saying when the next thing
/// happens.
///
/// A typed value rather than a formatted string, so switching language
/// re-renders the message that is on screen instead of leaving the previous
/// language's sentence there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    Path(PathRejection),
    Command(CommandRefusal),
    NoHandler(NoHandlerReason),
    Refused(OpenRefusal),
    Navigation(NavigationError),
    AlreadyPinned,
    NotPinnable,
    /// A job could not even be accepted — a spec that does not validate.
    Rejected(OperationError),
    /// Something that is already a machine key, such as an unreadable
    /// preferences file.
    Key(String),
}

impl Notice {
    pub fn message(&self, c: &'static Copy) -> String {
        match self {
            Notice::Path(rejection) => rejection.message(c).to_string(),
            Notice::Command(refusal) => match refusal {
                CommandRefusal::NothingToActOn => c.nothing_selected.to_string(),
                CommandRefusal::NotAFilesystemLocation => c.not_writable_here.to_string(),
                CommandRefusal::UnusableName => c.name_not_usable.to_string(),
                CommandRefusal::NotInTrash => c.refusal_in_trash.to_string(),
            },
            Notice::NoHandler(reason) => reason.message(c),
            Notice::Refused(refusal) => crate::i18n::refusal_label(*refusal, c).to_string(),
            Notice::Navigation(error) => match error {
                NavigationError::LastTab => c.last_tab_stays_open.to_string(),
                NavigationError::NothingToRestore => c.nothing_to_reopen.to_string(),
                NavigationError::NoSuchTab(_) => error.to_string(),
            },
            Notice::AlreadyPinned => c.already_pinned.to_string(),
            Notice::NotPinnable => c.not_writable_here.to_string(),
            Notice::Rejected(error) => error.to_string(),
            Notice::Key(key) => key.clone(),
        }
    }
}

/// A question the window is waiting on, other than a job conflict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingDialog {
    NewFolder,
    NewFile,
    Rename(LocalPath),
    RenameBookmark(usize),
    /// A permanent delete, which Issue #6 requires an explicit confirmation
    /// for. The targets are held here so the confirmation dialog cannot be
    /// answered for a different selection than the one it named.
    ConfirmDelete {
        targets: Vec<DeleteTarget>,
    },
}

/// One window's worth of state.
pub struct FilesSession {
    pub locale: Locale,
    pub preferences: FilesPreferences,
    preference_store: PreferenceStore,
    bookmark_store: BookmarkStore,
    pub bookmarks: BookmarkFile,
    pub directories: UserDirectories,
    pub mounts: MountTable,
    reader: Arc<FilesReader>,
    engine: Arc<JobEngine>,

    tabs: TabSet,
    /// One pane per tab, in no particular order; the tab strip's order comes
    /// from `tabs`.
    panes: Vec<(TabId, Pane)>,

    pub content: ContentView,
    pub clipboard: Clipboard,
    pub notice: Option<Notice>,
    pub dialog: Option<PendingDialog>,
    pub focus: Focus,
    /// Which favourite the keyboard is on, for the reorder and remove keys.
    pub sidebar_cursor: Option<usize>,
    pub operations_open: bool,
    pub jobs: Vec<JobSnapshot>,
    pub finished: SessionHistory,
}

/// Everything a session is built from.
///
/// A struct rather than a long argument list, so a caller cannot silently swap
/// two stores that happen to have the same shape, and so the window and the
/// tests build a session the same way.
pub struct SessionSetup {
    pub start: Location,
    pub preferences: FilesPreferences,
    pub preference_store: PreferenceStore,
    pub bookmark_store: BookmarkStore,
    pub directories: UserDirectories,
    pub mounts: MountTable,
    pub reader: Arc<FilesReader>,
    /// The process's one job engine. Handed in rather than built, which is
    /// what makes closing a window harmless to a running copy.
    pub engine: Arc<JobEngine>,
}

impl FilesSession {
    /// Builds a session at a starting location.
    pub fn new(setup: SessionSetup) -> Self {
        let SessionSetup {
            start,
            preferences,
            preference_store,
            bookmark_store,
            directories,
            mounts,
            reader,
            engine,
        } = setup;
        let view = preferences.view_preferences();
        let bookmarks = bookmark_store.load();
        let tabs = TabSet::with_preferences(start.clone(), view);
        let first = tabs.active().id();
        let pane = Pane::open(start, view, reader.as_ref());
        Self {
            locale: Locale::from_preference(preferences.locale),
            preferences,
            preference_store,
            bookmark_store,
            bookmarks,
            directories,
            mounts,
            reader,
            engine,
            tabs,
            panes: vec![(first, pane)],
            content: ContentView::new(preferences.view_mode, preferences.scale),
            clipboard: Clipboard::Empty,
            notice: None,
            dialog: None,
            focus: Focus::Content,
            sidebar_cursor: None,
            operations_open: false,
            jobs: Vec::new(),
            finished: SessionHistory::default(),
        }
    }

    // --- Tabs ------------------------------------------------------------

    pub fn tabs(&self) -> &TabSet {
        &self.tabs
    }

    pub fn active_tab(&self) -> TabId {
        self.tabs.active().id()
    }

    pub fn pane(&self) -> &Pane {
        let id = self.active_tab();
        &self
            .panes
            .iter()
            .find(|(tab, _)| *tab == id)
            .expect("every tab has a pane")
            .1
    }

    pub fn pane_mut(&mut self) -> &mut Pane {
        let id = self.active_tab();
        &mut self
            .panes
            .iter_mut()
            .find(|(tab, _)| *tab == id)
            .expect("every tab has a pane")
            .1
    }

    pub fn location(&self) -> &Location {
        self.pane().location()
    }

    /// Opens a tab. `activate` false is "open in background", which is what
    /// middle-clicking a folder does.
    pub fn open_tab(&mut self, location: Location, activate: bool) -> TabId {
        let id = self.tabs.open(location.clone(), activate);
        let view = self.preferences.view_preferences();
        let pane = Pane::open(location, view, self.reader.as_ref());
        self.panes.push((id, pane));
        if activate {
            self.resync_content();
        }
        id
    }

    pub fn close_tab(&mut self, id: TabId) {
        // The tab keeps the pane's history, so reopening it restores Back and
        // Forward rather than dropping the user at the folder with no history.
        self.sync_history(id);
        match self.tabs.close(id) {
            Ok(()) => {
                self.panes.retain(|(tab, _)| *tab != id);
                self.resync_content();
            }
            Err(error) => self.notice = Some(Notice::Navigation(error)),
        }
    }

    /// Reopens the most recently closed tab, with its history.
    pub fn restore_tab(&mut self) {
        match self.tabs.restore_closed() {
            Ok(id) => {
                let tab = self.tabs.get(id).expect("the tab just restored");
                let history = tab.history().clone();
                let view = tab.preferences();
                let pane = Pane::resume(history, view, self.reader.as_ref());
                self.panes.push((id, pane));
                self.resync_content();
            }
            Err(error) => self.notice = Some(Notice::Navigation(error)),
        }
    }

    pub fn activate_tab(&mut self, id: TabId) {
        if self.tabs.activate(id).is_ok() {
            self.resync_content();
        }
    }

    pub fn next_tab(&mut self) {
        let index = (self.tabs.active_index() + 1) % self.tabs.len();
        let id = self.tabs.tabs()[index].id();
        self.activate_tab(id);
    }

    pub fn previous_tab(&mut self) {
        let index = (self.tabs.active_index() + self.tabs.len() - 1) % self.tabs.len();
        let id = self.tabs.tabs()[index].id();
        self.activate_tab(id);
    }

    /// Re-derives the content cursor from the active pane's selection.
    ///
    /// Taking the view out of `self` first is what lets it borrow the pane
    /// immutably while it is itself borrowed mutably; the alternative would be
    /// an `Rc` around one `usize`.
    fn resync_content(&mut self) {
        let mut content = std::mem::take(&mut self.content);
        content.resync(self.pane().model());
        self.content = content;
    }

    /// Copies the pane's history into its tab, which is what makes closing and
    /// reopening a tab preserve where it had been.
    fn sync_history(&mut self, id: TabId) {
        let Some(history) = self
            .panes
            .iter()
            .find(|(tab, _)| *tab == id)
            .map(|(_, pane)| pane.history().clone())
        else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(id) {
            *tab.history_mut() = history;
        }
    }

    // --- Navigation ------------------------------------------------------

    pub fn navigate_to(&mut self, location: Location) {
        let reader = self.reader.clone();
        self.pane_mut().navigate_to(location, reader.as_ref());
        self.after_navigation();
    }

    pub fn go_back(&mut self) {
        let reader = self.reader.clone();
        self.pane_mut().go_back(reader.as_ref());
        self.after_navigation();
    }

    pub fn go_forward(&mut self) {
        let reader = self.reader.clone();
        self.pane_mut().go_forward(reader.as_ref());
        self.after_navigation();
    }

    pub fn go_to_parent(&mut self) {
        let reader = self.reader.clone();
        self.pane_mut().go_to_parent(reader.as_ref());
        self.after_navigation();
    }

    pub fn reload(&mut self) {
        let reader = self.reader.clone();
        self.pane_mut().reload(reader.as_ref());
        self.after_navigation();
    }

    fn after_navigation(&mut self) {
        let id = self.active_tab();
        self.sync_history(id);
        self.content.clear_type_ahead();
        self.resync_content();
    }

    /// Takes what the user typed in the path field.
    pub fn submit_path(
        &mut self,
        text: &str,
        home: Option<&std::path::Path>,
        validator: &dyn PathValidator,
    ) {
        match resolve_path_input(text, home, validator) {
            Ok(location) => {
                self.notice = None;
                self.navigate_to(location);
            }
            Err(rejection) => self.notice = Some(Notice::Path(rejection)),
        }
    }

    // --- View preferences -------------------------------------------------

    /// `Ctrl+H`. The model re-filters what it already holds, so this is
    /// immediate and starts no listing; the preference is then persisted.
    pub fn toggle_hidden(&mut self) {
        let shown = self.pane_mut().toggle_hidden();
        self.preferences.show_hidden = shown;
        self.apply_preferences_to_all_tabs();
        self.persist_preferences();
        self.resync_content();
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.preferences.view_mode = mode;
        self.content.mode = mode;
        self.persist_preferences();
    }

    pub fn toggle_view_mode(&mut self) {
        self.set_view_mode(self.preferences.view_mode.toggled());
    }

    pub fn set_scale(&mut self, scale: ItemScale) {
        self.preferences.scale = scale;
        self.content.scale = scale;
        self.persist_preferences();
    }

    pub fn set_order(&mut self, order: SortOrder) {
        self.preferences.set_order(order);
        self.apply_preferences_to_all_tabs();
        self.persist_preferences();
        self.resync_content();
    }

    pub fn set_sort_key(&mut self, key: SortKey) {
        let order = self.preferences.order();
        self.set_order(
            SortOrder::new(key, order.direction).with_folders_first(order.folders_first),
        );
    }

    pub fn toggle_sort_direction(&mut self) {
        let order = self.preferences.order();
        self.set_order(
            SortOrder::new(order.key, crate::content::reversed(order.direction))
                .with_folders_first(order.folders_first),
        );
    }

    pub fn toggle_folders_first(&mut self) {
        let order = self.preferences.order();
        self.set_order(order.with_folders_first(!order.folders_first));
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        self.preferences.locale = locale.to_preference();
        self.persist_preferences();
    }

    /// The global policy in one place: every tab gets the same order and the
    /// same hidden preference.
    fn apply_preferences_to_all_tabs(&mut self) {
        let order = self.preferences.order();
        let hidden = self.preferences.hidden();
        for (_, pane) in &mut self.panes {
            pane.set_order(order);
            pane.set_hidden_preference(hidden);
        }
        for tab in 0..self.tabs.len() {
            let id = self.tabs.tabs()[tab].id();
            if let Some(tab) = self.tabs.get_mut(id) {
                let preferences = tab.preferences_mut();
                preferences.order = order;
                preferences.hidden = hidden;
            }
        }
    }

    fn persist_preferences(&mut self) {
        if let Err(error) = self.preference_store.save(&self.preferences) {
            self.notice = Some(Notice::Key(format!("files.prefs.error.unwritable:{error}")));
        }
    }

    // --- Selection and opening --------------------------------------------

    pub fn apply_selection(&mut self, input: SelectionInput, columns: usize) -> Option<usize> {
        let mut content = std::mem::take(&mut self.content);
        let scrolled = content.apply(self.pane_mut().model_mut(), input, columns);
        self.content = content;
        scrolled
    }

    pub fn type_ahead(&mut self, character: char, now: std::time::Instant) -> Option<usize> {
        let mut content = std::mem::take(&mut self.content);
        let scrolled = content.type_ahead_key(self.pane_mut().model_mut(), character, now);
        self.content = content;
        scrolled
    }

    /// The entries currently selected, in visible order.
    pub fn selected_entries(&self) -> Vec<&Entry> {
        let model = self.pane().model();
        model
            .iter_visible()
            .filter(|entry| model.selection().contains(&entry.id()))
            .collect()
    }

    pub fn focused_entry(&self) -> Option<&Entry> {
        let id = self.pane().model().selection().cursor()?;
        self.pane().model().get(id)
    }

    /// Opens whatever the keyboard is on, or the entry at an index.
    pub fn open_index(&mut self, index: usize) {
        let Some(entry) = self.pane().model().visible(index).cloned() else {
            return;
        };
        self.open_entry(&entry);
    }

    pub fn open_focused(&mut self) {
        let Some(entry) = self.focused_entry().cloned() else {
            return;
        };
        self.open_entry(&entry);
    }

    fn open_entry(&mut self, entry: &Entry) {
        match crate::content::route_open(entry) {
            OpenOutcome::Navigate(location) => {
                self.notice = None;
                self.navigate_to(*location);
            }
            OpenOutcome::NoHandler(reason) => self.notice = Some(Notice::NoHandler(reason)),
            OpenOutcome::Refused(refusal) => self.notice = Some(Notice::Refused(refusal)),
        }
    }

    /// Opens an entry in a new tab, which only makes sense for a directory.
    pub fn open_in_new_tab(&mut self, entry_id: &EntryId) {
        let Some(entry) = self.pane().model().get(entry_id) else {
            return;
        };
        if let OpenOutcome::Navigate(location) = crate::content::route_open(entry) {
            self.open_tab(*location, true);
        }
    }

    // --- Favourites -------------------------------------------------------

    /// Pins a location. The directory is never moved: this writes a line to
    /// the bookmarks file and nothing else.
    pub fn pin(&mut self, location: &Location) {
        match self.bookmarks.pin(location) {
            PinOutcome::Pinned => {
                // A successful pin shows itself: the row appears in Favorites.
                // A notice on top of that would be noise.
                self.notice = None;
                self.persist_bookmarks();
            }
            PinOutcome::AlreadyPinned => self.notice = Some(Notice::AlreadyPinned),
            PinOutcome::NotPinnable => self.notice = Some(Notice::NotPinnable),
        }
    }

    pub fn pin_current(&mut self) {
        let location = self.location().clone();
        self.pin(&location);
    }

    pub fn remove_bookmark(&mut self, index: usize) {
        if self.bookmarks.remove(index).is_some() {
            if let Some(cursor) = self.sidebar_cursor
                && cursor >= self.bookmarks.len()
            {
                self.sidebar_cursor = self.bookmarks.len().checked_sub(1);
            }
            self.persist_bookmarks();
        }
    }

    pub fn move_bookmark_up(&mut self, index: usize) {
        if self.bookmarks.move_up(index) {
            self.sidebar_cursor = Some(index - 1);
            self.persist_bookmarks();
        }
    }

    pub fn move_bookmark_down(&mut self, index: usize) {
        if self.bookmarks.move_down(index) {
            self.sidebar_cursor = Some(index + 1);
            self.persist_bookmarks();
        }
    }

    /// A drop between two favourites.
    pub fn move_bookmark_to(&mut self, from: usize, to: usize) {
        if self.bookmarks.move_to(from, to) {
            self.sidebar_cursor = Some(to);
            self.persist_bookmarks();
        }
    }

    pub fn set_bookmark_label(&mut self, index: usize, label: &str) {
        if self.bookmarks.set_label(index, label) {
            self.persist_bookmarks();
        }
    }

    fn persist_bookmarks(&mut self) {
        if let Err(error) = self.bookmark_store.save(&self.bookmarks) {
            self.notice = Some(Notice::Key(format!(
                "files.bookmarks.error.unwritable:{error}"
            )));
        }
    }

    // --- Operations -------------------------------------------------------

    pub fn engine(&self) -> &Arc<JobEngine> {
        &self.engine
    }

    /// Submits a spec, keeping only the failure. The handle is dropped on
    /// purpose: the engine owns the job, and holding a receipt would be the
    /// beginning of tying the job to this window.
    pub fn submit(&mut self, spec: JobSpec) {
        match self.engine.submit(spec) {
            Ok(_handle) => {
                self.notice = None;
                self.operations_open = true;
            }
            Err(error) => self.notice = Some(Notice::Rejected(error)),
        }
        self.refresh_jobs();
    }

    fn submit_or_notice(&mut self, built: Result<JobSpec, CommandRefusal>) {
        match built {
            Ok(spec) => self.submit(spec),
            Err(refusal) => self.notice = Some(Notice::Command(refusal)),
        }
    }

    pub fn copy_selection(&mut self) {
        let paths = commands::selected_paths(&self.selected_entries());
        if paths.is_empty() {
            self.notice = Some(Notice::Command(CommandRefusal::NothingToActOn));
            return;
        }
        self.clipboard = Clipboard::Copy(paths);
        self.notice = None;
    }

    pub fn cut_selection(&mut self) {
        let paths = commands::selected_paths(&self.selected_entries());
        if paths.is_empty() {
            self.notice = Some(Notice::Command(CommandRefusal::NothingToActOn));
            return;
        }
        self.clipboard = Clipboard::Cut(paths);
        self.notice = None;
    }

    pub fn paste(&mut self) {
        let destination = self.location().clone();
        let built = commands::paste(&self.clipboard, &destination);
        let was_cut = matches!(self.clipboard, Clipboard::Cut(_));
        self.submit_or_notice(built);
        if was_cut && self.notice.is_none() {
            // A move consumes the clipboard: pasting a cut twice would move
            // files that are no longer where the clipboard says they are.
            self.clipboard = Clipboard::Empty;
        }
    }

    pub fn duplicate_selection(&mut self) {
        let paths = commands::selected_paths(&self.selected_entries());
        let built = commands::duplicate(paths);
        self.submit_or_notice(built);
    }

    pub fn trash_selection(&mut self) {
        let paths = commands::selected_paths(&self.selected_entries());
        let built = commands::move_to_trash(paths, None);
        self.submit_or_notice(built);
    }

    /// Asks for a permanent delete. Nothing is deleted here: the targets are
    /// held in a dialog until the confirmation is answered.
    pub fn request_permanent_delete(&mut self) {
        // Deleting a file by path needs no trash directory. Only emptying an
        // item out of the trash does, and that case is handled by producing no
        // targets rather than by refusing every delete.
        let trash_root = self.trash_root();
        let location = self.location().clone();
        let targets =
            commands::delete_targets(&location, &self.selected_entries(), trash_root.as_deref());
        if targets.is_empty() {
            self.notice = Some(Notice::Command(CommandRefusal::NothingToActOn));
            return;
        }
        self.dialog = Some(PendingDialog::ConfirmDelete { targets });
    }

    /// Answers the confirmation. This is the only place a
    /// [`DeleteConfirmation`] is constructed, and it is constructed from a
    /// person answering, never from stored state.
    pub fn confirm_permanent_delete(&mut self) {
        let Some(PendingDialog::ConfirmDelete { targets }) = self.dialog.take() else {
            return;
        };
        let built = commands::delete_permanently(targets, DeleteConfirmation::explicit());
        self.submit_or_notice(built);
    }

    pub fn restore_selection_from_trash(&mut self) {
        let Some(trash_root) = self.trash_root() else {
            self.notice = Some(Notice::Command(CommandRefusal::NotInTrash));
            return;
        };
        let location = self.location().clone();
        let items = commands::selected_trash_items(&self.selected_entries(), &trash_root);
        let built = commands::restore_from_trash(&location, items);
        self.submit_or_notice(built);
    }

    pub fn create_folder(&mut self, name: &str) {
        let location = self.location().clone();
        let built = commands::new_folder(&location, name);
        self.submit_or_notice(built);
    }

    pub fn create_file(&mut self, name: &str) {
        let location = self.location().clone();
        let built = commands::new_file(&location, name);
        self.submit_or_notice(built);
    }

    pub fn rename(&mut self, path: &LocalPath, new_name: &str) {
        let built = commands::rename(path, new_name);
        self.submit_or_notice(built);
    }

    /// The trash this session writes to.
    pub fn trash_root(&self) -> Option<std::path::PathBuf> {
        self.reader.trash().map(|trash| trash.root().to_path_buf())
    }

    pub fn pause_job(&mut self, id: JobId) {
        self.engine.pause(id);
        self.refresh_jobs();
    }

    pub fn resume_job(&mut self, id: JobId) {
        self.engine.resume(id);
        self.refresh_jobs();
    }

    pub fn cancel_job(&mut self, id: JobId) {
        self.engine.cancel(id);
        self.refresh_jobs();
    }

    pub fn retry_job(&mut self, id: JobId) {
        self.engine.retry_failed(id);
        self.refresh_jobs();
    }

    pub fn resolve_conflict(&mut self, id: JobId, decision: ConflictDecision) {
        self.engine.resolve(id, decision);
        self.refresh_jobs();
    }

    /// Re-reads every job from the engine and files the finished ones into the
    /// session history.
    pub fn refresh_jobs(&mut self) -> bool {
        let jobs = self.engine.jobs();
        let changed = jobs != self.jobs;
        let c = crate::i18n::copy(self.locale);
        for snapshot in &jobs {
            if snapshot.state.is_terminal() {
                self.finished.record(opcenter::job_row(snapshot, c));
            }
        }
        self.jobs = jobs;
        changed
    }

    /// The running jobs as rows.
    pub fn job_rows(&self) -> Vec<JobRow> {
        let c = crate::i18n::copy(self.locale);
        opcenter::job_rows(&self.jobs, c)
    }

    // --- Frame ------------------------------------------------------------

    /// Drains whatever arrived and returns whether the window has to redraw.
    ///
    /// The active pane is pumped every frame; background tabs are pumped too,
    /// because a tab opened in the background is loading and must be ready
    /// when it is switched to rather than starting over.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        for (_, pane) in &mut self.panes {
            changed |= pane.pump();
        }
        if changed {
            // Entries arriving move indices around. The cursor is re-derived
            // from the selection's identity, which is what keeps the focused
            // entry focused while a large directory streams in.
            self.resync_content();
        }
        changed |= self.refresh_jobs();
        changed
    }

    pub fn is_listing(&self) -> bool {
        self.panes.iter().any(|(_, pane)| pane.is_listing())
    }

    // --- Commands ---------------------------------------------------------

    /// Carries out one keyboard command. `rows` is how many rows a page is and
    /// `columns` how many tiles fit across, both from the current viewport.
    pub fn dispatch(&mut self, command: Command, columns: usize, rows: usize) {
        match command {
            Command::GoBack => self.go_back(),
            Command::GoForward => self.go_forward(),
            Command::GoToParent => self.go_to_parent(),
            Command::Reload => self.reload(),
            // The window owns the text field, so focusing it is handled there.
            Command::FocusPathField => {}
            Command::NewTab => {
                let location = self.home_location();
                self.open_tab(location, true);
            }
            Command::CloseTab => {
                let id = self.active_tab();
                self.close_tab(id);
            }
            Command::RestoreClosedTab => self.restore_tab(),
            Command::NextTab => self.next_tab(),
            Command::PreviousTab => self.previous_tab(),
            Command::ToggleHidden => self.toggle_hidden(),
            Command::ToggleViewMode => self.toggle_view_mode(),
            Command::LargerItems => self.set_scale(self.preferences.scale.larger()),
            Command::SmallerItems => self.set_scale(self.preferences.scale.smaller()),
            Command::MoveUp => {
                self.apply_selection(SelectionInput::Up, columns);
            }
            Command::MoveDown => {
                self.apply_selection(SelectionInput::Down, columns);
            }
            Command::MoveLeft => {
                self.apply_selection(SelectionInput::Left, columns);
            }
            Command::MoveRight => {
                self.apply_selection(SelectionInput::Right, columns);
            }
            Command::PageUp => {
                self.apply_selection(SelectionInput::PageUp(rows), columns);
            }
            Command::PageDown => {
                self.apply_selection(SelectionInput::PageDown(rows), columns);
            }
            Command::MoveToStart => {
                self.apply_selection(SelectionInput::Home, columns);
            }
            Command::MoveToEnd => {
                self.apply_selection(SelectionInput::End, columns);
            }
            Command::SelectAll => {
                self.apply_selection(SelectionInput::SelectAll, columns);
            }
            Command::ClearSelection => {
                self.dialog = None;
                self.notice = None;
                self.apply_selection(SelectionInput::Clear, columns);
            }
            Command::ExtendUp | Command::ExtendDown => {
                let direction = if command == Command::ExtendUp {
                    SelectionInput::Up
                } else {
                    SelectionInput::Down
                };
                // Move the cursor, then extend the range to where it landed.
                if let Some(index) = self.apply_selection(direction, columns) {
                    self.apply_selection(SelectionInput::RangeClick(index), columns);
                }
            }
            Command::Open => match self.focus {
                Focus::Content => self.open_focused(),
                Focus::Sidebar => self.open_focused_bookmark(),
            },
            Command::NewFolder => self.dialog = Some(PendingDialog::NewFolder),
            Command::Rename => self.begin_rename(),
            Command::Copy => self.copy_selection(),
            Command::Cut => self.cut_selection(),
            Command::Paste => self.paste(),
            Command::Duplicate => self.duplicate_selection(),
            Command::MoveToTrash => self.trash_selection(),
            Command::DeletePermanently => self.request_permanent_delete(),
            Command::RestoreFromTrash => self.restore_selection_from_trash(),
            Command::ToggleOperations => self.operations_open = !self.operations_open,
            Command::MoveBookmarkUp => {
                if let Some(index) = self.sidebar_cursor {
                    self.move_bookmark_up(index);
                }
            }
            Command::MoveBookmarkDown => {
                if let Some(index) = self.sidebar_cursor {
                    self.move_bookmark_down(index);
                }
            }
            Command::RemoveBookmark => {
                if let Some(index) = self.sidebar_cursor {
                    self.remove_bookmark(index);
                }
            }
            Command::TypeAhead(character) => {
                self.type_ahead(character, std::time::Instant::now());
            }
        }
    }

    fn begin_rename(&mut self) {
        match self.focus {
            Focus::Sidebar => {
                if let Some(index) = self.sidebar_cursor {
                    self.dialog = Some(PendingDialog::RenameBookmark(index));
                }
            }
            Focus::Content => {
                let Some(path) = self.focused_entry().and_then(Entry::as_local_path).cloned()
                else {
                    self.notice = Some(Notice::Command(CommandRefusal::NothingToActOn));
                    return;
                };
                self.dialog = Some(PendingDialog::Rename(path));
            }
        }
    }

    fn open_focused_bookmark(&mut self) {
        let Some(index) = self.sidebar_cursor else {
            return;
        };
        let Some(location) = self.bookmarks.get(index).map(|b| b.location().clone()) else {
            return;
        };
        self.navigate_to(location);
    }

    /// Where a new tab opens: the session's home, or the filesystem root when
    /// there is no home to speak of.
    pub fn home_location(&self) -> Location {
        self.directories
            .home()
            .cloned()
            .unwrap_or_else(|| Location::Local(LocalPath::root()))
    }

    /// Whether the current location is the Trash, which is what makes Put Back
    /// available.
    pub fn viewing_trash(&self) -> bool {
        matches!(self.location(), Location::Trash(TrashLocation::Root))
    }
}
