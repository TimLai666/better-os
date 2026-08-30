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

use app_catalog_core::{DesktopId, MimeType};
use app_catalog_platform::ProcessSpawner;

use crate::apps::{ApplicationDetails, CatalogHandle, LaunchReport};
use crate::bookmarks::{BookmarkFile, BookmarkStore, PinOutcome};
use crate::commands::{self, Clipboard, CommandRefusal};
use crate::content::{ContentView, SelectionInput};
use crate::devices::{
    CollectionMode, DeviceInventory, DeviceLink, DeviceNotice, DeviceRow, NoDeviceLink, is_under,
};
use crate::i18n::{Copy, Locale};
use crate::keys::{Command, Focus};
use crate::opcenter::{self, JobRow, SessionHistory};
use crate::openwith::{ChooserRequest, DefaultHandlers, DefaultSource, OpenRoute, SessionDefaults};
use crate::prefs::{FilesPreferences, ItemScale, PreferenceStore, ViewMode};
use crate::preview::PreviewPanel;
use crate::reader::FilesReader;
use crate::search::SearchState;
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
    Refused(OpenRefusal),
    Navigation(NavigationError),
    AlreadyPinned,
    NotPinnable,
    /// A job could not even be accepted — a spec that does not validate.
    Rejected(OperationError),
    /// Something that is already a machine key, such as an unreadable
    /// preferences file.
    Key(String),
    /// The result of starting an application.
    Launch(LaunchReport),
    /// Something a device did.
    Device(Box<DeviceEvent>),
    /// A file whose type could not be resolved, so no chooser can be opened
    /// for it.
    NoMimeType,
    /// The application set for this type is not installed any more, so the
    /// chooser was opened instead of a launch happening.
    DefaultApplicationMissing,
}

/// A device event worth telling the user about.
///
/// Only the ones that need words. A device appearing, being mounted, and being
/// navigated into produce no notice at all, because the sidebar row and the
/// content area already say what happened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    MountFailed {
        label: String,
        detail: String,
    },
    EjectFailed {
        label: String,
        detail: String,
    },
    Ejected {
        label: String,
    },
    EjectedNotPoweredOff {
        label: String,
    },
    /// The device being viewed went away and the tab was moved somewhere safe.
    DisconnectedWhileViewing {
        label: String,
    },
    /// It went away while data was still being written.
    UnsafeRemoval {
        label: String,
        recommend_filesystem_check: bool,
    },
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
            Notice::Launch(report) => match report {
                LaunchReport::Started { name, .. } => format!("{} {}", c.app_launched, name),
                LaunchReport::Activated { name } => format!("{} {}", c.app_activated, name),
                LaunchReport::FellBackToProcess { name, .. } => {
                    format!("{} {}", c.app_activation_fell_back, name)
                }
                LaunchReport::NoSuchApplication { .. } => c.app_no_such_application.to_string(),
                LaunchReport::Failed { name, .. } => format!("{}: {name}", c.app_launch_failed),
            },
            Notice::Device(event) => match event.as_ref() {
                DeviceEvent::MountFailed { label, .. } => {
                    format!("{}: {label}", c.device_mount_failed)
                }
                DeviceEvent::EjectFailed { label, .. } => {
                    format!("{}: {label}", c.device_eject_failed)
                }
                DeviceEvent::Ejected { label } => format!("{label}: {}", c.device_ejected),
                DeviceEvent::EjectedNotPoweredOff { label } => {
                    format!("{label}: {}", c.device_ejected_not_powered_off)
                }
                DeviceEvent::DisconnectedWhileViewing { .. } => {
                    c.device_disconnected_returned_home.to_string()
                }
                DeviceEvent::UnsafeRemoval {
                    label,
                    recommend_filesystem_check,
                } => {
                    let mut message = format!("{label}: {}", c.device_unsafe_removal);
                    if *recommend_filesystem_check {
                        message.push(' ');
                        message.push_str(c.device_check_filesystem);
                    }
                    message
                }
            },
            Notice::NoMimeType => c.open_with_no_mime_type.to_string(),
            Notice::DefaultApplicationMissing => c.open_with_default_missing.to_string(),
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

    // --- ticket 35 --------------------------------------------------------
    /// The shared application catalog. The same handle the reader lists the
    /// Applications location from, so a reload is visible to both at once.
    catalog: CatalogHandle,
    /// The effective default handler per MIME type, read from the session's
    /// `mimeapps.list`.
    defaults: Box<dyn DefaultHandlers>,
    /// Where a launch actually happens. The platform crate's own trait, so a
    /// test that records launches exercises the production path.
    spawner: Box<dyn ProcessSpawner>,
    link: Box<dyn DeviceLink>,
    pub devices: DeviceInventory,
    pub collection: CollectionMode,
    /// The device whose mount is being waited on so the window can navigate
    /// into it. Set by a click on an unmounted row and cleared by the mount.
    pending_open: Option<String>,
    pub preview: PreviewPanel,
    pub search: SearchState,
    /// The file the embedded chooser is open for, when it is open.
    pub chooser: Option<ChooserRequest>,
    /// The application whose details panel is open.
    pub details: Option<ApplicationDetails>,
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
    /// The shared application catalog.
    pub catalog: CatalogHandle,
    pub defaults: Box<dyn DefaultHandlers>,
    pub spawner: Box<dyn ProcessSpawner>,
    pub link: Box<dyn DeviceLink>,
    pub preview: PreviewPanel,
}

impl SessionSetup {
    /// The parts of a session that have a sensible offline default, so a test
    /// names only what it is actually testing.
    ///
    /// The device link is [`NoDeviceLink`] and the launcher is a recording
    /// spawner, which is the honest default for a session with no host behind
    /// it: no device claims a state, and nothing is executed.
    pub fn offline(
        start: Location,
        reader: Arc<FilesReader>,
        engine: Arc<JobEngine>,
        preferences: FilesPreferences,
        preference_store: PreferenceStore,
        bookmark_store: BookmarkStore,
    ) -> Self {
        let catalog = reader.catalog().clone();
        Self {
            start,
            preferences,
            preference_store,
            bookmark_store,
            directories: UserDirectories::default(),
            mounts: MountTable::default(),
            reader,
            engine,
            catalog,
            defaults: Box::new(SessionDefaults::fixed([])),
            spawner: Box::new(app_catalog_platform::RecordingSpawner::new()),
            link: Box::new(NoDeviceLink),
            preview: PreviewPanel::default(),
        }
    }
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
            catalog,
            defaults,
            spawner,
            link,
            preview,
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
            catalog,
            defaults,
            spawner,
            collection: link.mode(),
            link,
            devices: DeviceInventory::default(),
            pending_open: None,
            preview,
            search: SearchState::default(),
            chooser: None,
            details: None,
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
        // A search belongs to the location it was started in. Carrying it into
        // the next folder would show results from a place the user has left.
        self.search.clear();
        self.details = None;
        self.refresh_preview();
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
        // The Applications location has its own hidden rule — an excluded or
        // `NoDisplay` entry — and it follows the same key.
        self.reader.set_include_hidden_applications(shown);
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

    // --- The content area's rows ------------------------------------------
    //
    // A row on screen is not always an index into the model's visible list: a
    // running search draws its hits instead. These three are the only place
    // that translation happens, so a click, a double-click, and the row the
    // renderer formats can never disagree about which entry row 7 is.

    /// How many rows the content area draws.
    pub fn row_count(&self) -> usize {
        if self.search.is_active() {
            self.search.hits().len()
        } else {
            self.pane().model().visible_len()
        }
    }

    /// The entry a content-area row shows.
    pub fn entry_at(&self, index: usize) -> Option<&Entry> {
        if self.search.is_active() {
            let hit = self.search.hits().get(index)?;
            self.pane().model().get(&hit.id)
        } else {
            self.pane().model().visible(index)
        }
    }

    /// The model index the selection machinery works in, for a content-area
    /// row.
    ///
    /// `None` for a search hit the view is currently hiding — a dotfile found
    /// by a search that includes hidden files while the view does not. It can
    /// be opened; it has no place in the visible selection, and pretending it
    /// did would move the cursor to a different entry.
    pub fn model_row(&self, index: usize) -> Option<usize> {
        if !self.search.is_active() {
            return (index < self.pane().model().visible_len()).then_some(index);
        }
        let id = self.entry_at(index)?.id();
        self.pane()
            .model()
            .iter_visible()
            .position(|entry| entry.id() == id)
    }

    /// Opens the entry a content-area row shows.
    pub fn open_row(&mut self, index: usize) {
        let Some(entry) = self.entry_at(index).cloned() else {
            return;
        };
        self.open_entry(&entry);
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
        // The three intents are three different actions, which is why
        // `files-core` makes them a closed enum rather than a path.
        match files_core::open_intent(entry) {
            files_core::OpenIntent::Navigate(location) => {
                self.notice = None;
                self.navigate_to(*location);
            }
            files_core::OpenIntent::Launch { desktop_id, action } => {
                self.launch_application(&desktop_id, action.as_deref(), &[]);
            }
            files_core::OpenIntent::OpenFile { path, mime } => {
                self.open_file(&path, mime.as_ref());
            }
            files_core::OpenIntent::Refused(refusal) => {
                self.notice = Some(Notice::Refused(refusal))
            }
        }
    }

    // --- Applications -----------------------------------------------------

    pub fn catalog(&self) -> &CatalogHandle {
        &self.catalog
    }

    /// Starts an application through its registered desktop definition.
    ///
    /// Nothing builds a command line here or anywhere below: `apps::launch`
    /// hands the record to the shared platform launcher, which produces an
    /// argument vector from the entry itself.
    pub fn launch_application(
        &mut self,
        desktop_id: &DesktopId,
        action: Option<&str>,
        targets: &[app_catalog_core::LaunchTarget],
    ) {
        let report = crate::apps::launch(
            &self.catalog,
            desktop_id,
            action,
            targets,
            self.spawner.as_ref(),
        );
        // A successful launch is not silent: the window has not changed, so
        // without a line the user cannot tell a slow application from a click
        // that did nothing.
        self.notice = Some(Notice::Launch(report));
    }

    /// Opens the details panel for an application row.
    pub fn show_details(&mut self, desktop_id: &DesktopId) {
        self.details = crate::apps::details(&self.catalog, desktop_id);
        if self.details.is_none() {
            self.notice = Some(Notice::Launch(LaunchReport::NoSuchApplication {
                desktop_id: desktop_id.as_str().to_string(),
            }));
        }
    }

    pub fn close_details(&mut self) {
        self.details = None;
    }

    /// The desktop id of whatever is focused, when it is an application.
    pub fn focused_application(&self) -> Option<DesktopId> {
        let entry = self.focused_entry()?;
        match &entry.body {
            files_core::EntryBody::Application(facts) => Some(facts.desktop_id.clone()),
            _ => None,
        }
    }

    /// Opens a second window of an application, when its entry declares a way.
    pub fn open_new_window(&mut self, desktop_id: &DesktopId) {
        let snapshot = self.catalog.snapshot();
        let action = snapshot
            .get(desktop_id)
            .and_then(crate::apps::new_window_action);
        self.launch_application(desktop_id, action.as_deref(), &[]);
    }

    /// Re-reads the catalog. Called from the watcher thread's change signal.
    pub fn reload_catalog(&mut self) {
        self.catalog.reload_from_env();
        if matches!(self.location(), Location::Applications) {
            self.reload();
        }
    }

    /// The catalog generation the window last drew, so a change can be noticed
    /// without comparing two catalogs.
    pub fn catalog_generation(&self) -> u64 {
        self.catalog.generation()
    }

    // --- Open With --------------------------------------------------------

    /// Opens a file with the effective default handler, or asks.
    pub fn open_file(&mut self, path: &LocalPath, mime: Option<&MimeType>) {
        let (route, source) =
            crate::openwith::route_open_file(mime, self.defaults.as_ref(), &self.catalog);
        match route {
            OpenRoute::LaunchWith { desktop_id, .. } => {
                let Some(target) = crate::apps::target_for(path) else {
                    self.notice = Some(Notice::Command(CommandRefusal::UnusableName));
                    return;
                };
                self.launch_application(&desktop_id, None, &[target]);
            }
            OpenRoute::AskChooser { mime } => {
                self.chooser = ChooserRequest::new(path, mime);
                if self.chooser.is_none() {
                    self.notice = Some(Notice::Command(CommandRefusal::UnusableName));
                } else if source == DefaultSource::AssociationMissingApplication {
                    // The two empty-looking cases are not the same, and the
                    // one where something *was* set is worth saying.
                    self.notice = Some(Notice::DefaultApplicationMissing);
                } else {
                    self.notice = None;
                }
            }
            OpenRoute::NoMimeType => self.notice = Some(Notice::NoMimeType),
        }
    }

    /// The explicit Open With action, which always asks even when a default
    /// exists.
    pub fn open_with_selected(&mut self) {
        let Some(entry) = self.focused_entry().cloned() else {
            self.notice = Some(Notice::Command(CommandRefusal::NothingToActOn));
            return;
        };
        let Some(path) = entry.as_local_path().cloned() else {
            self.notice = Some(Notice::Command(CommandRefusal::NotAFilesystemLocation));
            return;
        };
        let Some(mime) = entry.mime.clone() else {
            self.notice = Some(Notice::NoMimeType);
            return;
        };
        self.chooser = ChooserRequest::new(&path, mime);
        if self.chooser.is_none() {
            self.notice = Some(Notice::Command(CommandRefusal::UnusableName));
        } else {
            self.notice = None;
        }
    }

    /// Closes the chooser. The chooser itself performs the launch and the
    /// association write, so there is nothing to apply here — which is the
    /// point: one association write path, in `app-chooser-core`.
    pub fn close_chooser(&mut self, cancelled: bool) {
        self.chooser = None;
        if cancelled {
            self.notice = None;
        }
    }

    // --- Devices ----------------------------------------------------------

    pub fn device_rows(&self) -> &[DeviceRow] {
        self.devices.rows()
    }

    /// Clicking a device row. A mounted device is opened; an unmounted one is
    /// mounted and then opened, which is Issue #6's "clicking mounts and opens
    /// automatically".
    pub fn open_device(&mut self, object_path: &str) {
        let Some(row) = self.devices.get(object_path) else {
            return;
        };
        match row.location() {
            Some(location) => {
                self.notice = None;
                self.navigate_to(location);
            }
            None => {
                // Remembered so the mount's answer knows where it was going.
                // Leaving the location does *not* unmount, so nothing here
                // pairs with a later unmount.
                self.pending_open = Some(object_path.to_string());
                self.link.request_mount(object_path);
            }
        }
    }

    /// The Eject action. Available in every state, because a user who wants to
    /// stop using a device should never have to argue with the file manager
    /// about it.
    pub fn eject_device(&mut self, object_path: &str) {
        self.link.request_eject(object_path);
    }

    pub fn refresh_devices(&mut self) {
        self.link.request_refresh();
    }

    /// Drains the device link. Returns whether the window has to redraw.
    pub fn pump_devices(&mut self) -> bool {
        let notices = self.link.poll();
        if notices.is_empty() {
            return false;
        }
        for notice in notices {
            self.apply_device_notice(notice);
        }
        true
    }

    fn apply_device_notice(&mut self, notice: DeviceNotice) {
        match notice {
            DeviceNotice::Mode(mode) => self.collection = mode,
            DeviceNotice::Inventory(reports) => self.devices.apply_inventory(reports),
            DeviceNotice::Mounted {
                object_path,
                mount_point,
            } => {
                self.devices
                    .set_mount_point(&object_path, mount_point.clone());
                if self.pending_open.as_deref() == Some(object_path.as_str())
                    && let Ok(path) = LocalPath::new(mount_point)
                {
                    self.pending_open = None;
                    self.notice = None;
                    self.navigate_to(Location::Local(path));
                }
            }
            DeviceNotice::MountFailed {
                object_path,
                detail,
            } => {
                if self.pending_open.as_deref() == Some(object_path.as_str()) {
                    self.pending_open = None;
                }
                self.notice = Some(Notice::Device(Box::new(DeviceEvent::MountFailed {
                    label: self.device_label(&object_path),
                    detail,
                })));
            }
            DeviceNotice::Ejected {
                object_path,
                unmounted,
                powered_off,
            } => {
                self.devices.clear_mount_point(&object_path);
                let label = self.device_label(&object_path);
                self.notice = Some(Notice::Device(Box::new(if unmounted && !powered_off {
                    // An unmount that worked and a power-off that did not
                    // is not a clean eject and is not reported as one.
                    DeviceEvent::EjectedNotPoweredOff { label }
                } else {
                    DeviceEvent::Ejected { label }
                })));
            }
            DeviceNotice::EjectFailed {
                object_path,
                detail,
            } => {
                self.notice = Some(Notice::Device(Box::new(DeviceEvent::EjectFailed {
                    label: self.device_label(&object_path),
                    detail,
                })));
            }
            DeviceNotice::Disconnected {
                object_path,
                unsafe_removal,
            } => self.handle_disconnect(&object_path, unsafe_removal),
        }
    }

    fn device_label(&self, object_path: &str) -> String {
        self.devices
            .get(object_path)
            .map(|row| row.label.clone())
            .unwrap_or_else(|| object_path.to_string())
    }

    /// A device left. Removes the row, clears every navigation state that
    /// pointed at it, and moves any tab standing on it somewhere safe.
    ///
    /// All of it without the user doing anything, which is the acceptance
    /// criterion. The one thing that is *not* silent is an unsafe removal:
    /// data may not have been written, and that is not a fact to clean up
    /// quietly.
    fn handle_disconnect(
        &mut self,
        object_path: &str,
        unsafe_removal: Option<crate::devices::UnsafeRemoval>,
    ) {
        let label = self.device_label(object_path);
        let mount_point = self.devices.remove(object_path, unsafe_removal.clone());
        if self.pending_open.as_deref() == Some(object_path) {
            self.pending_open = None;
        }

        let mut stranded_active = false;
        if let Some(mount_point) = mount_point.as_ref() {
            let home = self.home_location();
            let reader = self.reader.clone();
            let active = self.active_tab();
            let mut stranded: Vec<TabId> = Vec::new();
            for (tab, pane) in &mut self.panes {
                if pane.forget_locations(|location| !is_under(location, mount_point)) {
                    stranded.push(*tab);
                }
            }
            for tab in stranded {
                if tab == active {
                    stranded_active = true;
                }
                if let Some((_, pane)) = self.panes.iter_mut().find(|(id, _)| *id == tab) {
                    pane.navigate_to(home.clone(), reader.as_ref());
                    // Navigating pushes where the pane *was* onto the back
                    // stack, which is the very entry that was just forgotten.
                    // Forgetting again is what makes the cleanup actually
                    // leave nothing behind.
                    pane.forget_locations(|location| !is_under(location, mount_point));
                }
                self.sync_history(tab);
            }
            // A tab's own stored history is pruned too, so reopening a closed
            // tab does not bring the stale entries back.
            for index in 0..self.tabs.len() {
                let id = self.tabs.tabs()[index].id();
                if let Some(tab) = self.tabs.get_mut(id) {
                    tab.history_mut()
                        .forget(|location| !is_under(location, mount_point));
                }
            }
            self.resync_content();
        }

        if let Some(record) = unsafe_removal {
            self.notice = Some(Notice::Device(Box::new(DeviceEvent::UnsafeRemoval {
                label,
                recommend_filesystem_check: record.recommend_filesystem_check,
            })));
        } else if stranded_active {
            self.notice = Some(Notice::Device(Box::new(
                DeviceEvent::DisconnectedWhileViewing { label },
            )));
        }
    }

    // --- Preview ----------------------------------------------------------

    /// Space. Opens or closes the preview pane.
    pub fn toggle_preview(&mut self) {
        if self.preview.toggle() {
            let entry = self.focused_entry().cloned();
            let location = self.location().clone();
            self.preview.request_for(entry.as_ref(), &location);
        }
        self.preferences.preview_open = self.preview.open;
        self.persist_preferences();
    }

    fn refresh_preview(&mut self) {
        if !self.preview.open {
            return;
        }
        let entry = self.focused_entry().cloned();
        let location = self.location().clone();
        self.preview.request_for(entry.as_ref(), &location);
    }

    // --- Search -----------------------------------------------------------

    /// Types in the search field.
    pub fn set_search_text(&mut self, text: impl Into<String>) {
        let location = self.location().clone();
        self.search.set_text(text, &location);
    }

    pub fn close_search(&mut self) {
        self.search.close();
    }

    pub fn toggle_search_hidden(&mut self) {
        let location = self.location().clone();
        let include = !self.search.include_hidden;
        self.search.set_include_hidden(include, &location);
    }

    /// The entries the content area draws: the search results when a search is
    /// running, and the directory otherwise.
    pub fn visible_entries(&self) -> Vec<Entry> {
        if !self.search.is_active() {
            return self.pane().model().iter_visible().cloned().collect();
        }
        let model = self.pane().model();
        self.search
            .hits()
            .iter()
            .filter_map(|hit| model.get(&hit.id).cloned())
            .collect()
    }

    /// Opens an entry in a new tab, which only makes sense for a directory.
    pub fn open_in_new_tab(&mut self, entry_id: &EntryId) {
        let Some(entry) = self.pane().model().get(entry_id) else {
            return;
        };
        // Only a directory opens in a tab. An application launches and a file
        // opens in something else; neither has a tab to be put in.
        if let files_core::OpenIntent::Navigate(location) = files_core::open_intent(entry) {
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
        changed |= self.pump_devices();
        // Search is fed after the pane, so entries that arrived this frame are
        // considered this frame rather than next.
        let mut search = std::mem::take(&mut self.search);
        changed |= search.pump(self.pane().model());
        self.search = search;
        changed |= self.preview.pump();
        if changed {
            self.refresh_preview();
        }
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
            Command::TogglePreview => self.toggle_preview(),
            Command::OpenWith => self.open_with_selected(),
            Command::ViewDetails => match self.focused_application() {
                Some(desktop_id) => self.show_details(&desktop_id),
                // Details for a file is the properties panel, which is not in
                // this ticket. Saying nothing is better than opening an empty
                // application panel for a file.
                None => self.notice = Some(Notice::Command(CommandRefusal::NothingToActOn)),
            },
            // The window owns the search field, so focusing it is handled
            // there, exactly as `FocusPathField` is.
            Command::FocusSearch => {}
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
