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

mod remote_tabs;
pub use remote_tabs::{LinkState, PasteWhy, SavedView, TabLink};

use files_core::{
    Entry, EntryId, LocalPath, Location, NavigationError, OpenRefusal, Pane, SortKey, SortOrder,
    TabId, TabSet, TrashLocation,
};
use files_operations::{
    ArchiveFormat, ConflictDecision, DeleteConfirmation, DeleteTarget, JobEngine, JobId,
    JobSnapshot, JobSpec, OperationError,
};
use files_platform::{MountTable, UserDirectories};

use app_catalog_core::{DesktopId, MimeType};
use app_catalog_platform::ProcessSpawner;
use storage_core::RemovalPolicy;

use crate::apps::{ApplicationDetails, CatalogHandle, LaunchReport};
use crate::bookmarks::{BookmarkFile, BookmarkStore, PinOutcome};
use crate::commands::{self, ArchiveActions, Clipboard, CommandRefusal, CompressChooser};
use crate::content::{ContentView, SelectionInput};
use crate::devices::{
    CollectionMode, DeviceInventory, DeviceLink, DeviceNotice, DeviceRow, NoDeviceLink, is_under,
};
use crate::drives::DriveText as _;
use crate::i18n::{Copy, Locale};
use crate::keys::{ChooserKey, Command, Focus};
use crate::launch::StartProblem;
use crate::opcenter::{self, JobRow, SessionHistory};
use crate::openwith::{ChooserRequest, DefaultHandlers, DefaultSource, OpenRoute, SessionDefaults};
use crate::policy::{PerformanceConfirmation, PolicyRequest};
use crate::prefs::{FilesPreferences, ItemScale, PreferenceStore, ViewMode};
use crate::preview::PreviewPanel;
use crate::reader::FilesReader;
use crate::search::SearchState;
use crate::settings_store::{FolderView, ViewState, ViewStateStore};
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
    /// What happened to the location the window was started with: why it
    /// could not be opened, and how many further locations were ignored.
    Start {
        problem: Option<StartProblem>,
        ignored: usize,
    },
    /// No terminal is installed that Open Terminal Here knows how to start.
    NoTerminal,
    /// An image is being written to a drive: how far it is.
    WritingImage {
        label: String,
        percent: u8,
    },
    /// A format or image write finished, or why it did not.
    Erased {
        label: String,
        format: bool,
        failure: Option<String>,
    },
    /// A server is being opened.
    Mounting(String),
    /// A server could not be opened.
    /// A server did not open: a plain sentence, with the server's own words
    /// as the detail under it ([`Notice::detail`]).
    MountFailed {
        address: String,
        error: crate::network::MountError,
    },
    /// The folder a view showed was deleted or moved away (its name); the
    /// view went to the folder above it.
    FolderGone(String),
    /// A folder on a server that connected is not there any more; the view
    /// went to the nearest folder above it (ticket 310).
    RemoteFolderGone {
        folder: String,
        server: String,
    },
    /// A paste held for a server folder's connection was not done, and why
    /// (ticket 310).
    PasteNotDone {
        folder: String,
        server: String,
        why: remote_tabs::PasteWhy,
    },
    /// Retry was asked of a job the engine no longer keeps.
    RetryGone,
    /// A drive could not be mounted or unlocked.
    DriveFailed {
        drive: String,
        detail: String,
    },
    /// A drive's "Mount automatically" could not be changed (ticket 127).
    AutoMountUnchanged {
        drive: String,
        outcome: drive_mounts::client::SetOutcome,
    },
    /// The passphrase did not open the drive.
    WrongPassphrase {
        drive: String,
    },
    /// The drive unlocked but what is inside did not mount.
    DriveUnlockedNotMounted {
        drive: String,
        detail: String,
    },
    /// Ejected: safe to remove.
    DriveSafe {
        drive: String,
    },
    /// Unmounted, but not powered off.
    DriveNotPoweredOff {
        drive: String,
        detail: String,
    },
    /// Another volume on the same drive is in use: nothing was powered off.
    DriveInUse {
        drive: String,
        volume: String,
        detail: String,
    },
    /// The eject failed.
    DriveEjectFailed {
        drive: String,
        detail: String,
    },
    /// What an undo (or redo, when `undoing` is false) did, or why it did
    /// not run.
    Undo {
        undoing: bool,
        outcome: Result<crate::undo::ChangeKind, crate::undo::Refusal>,
    },
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
    /// The storage layer refused a removal-policy change, with its reason.
    PolicyRefused {
        label: String,
        detail: String,
    },
}

/// How a notice is drawn: a done thing is a success, news is information,
/// and the rest asks for a look.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoticeTone {
    Success,
    Information,
    Warning,
}

impl Notice {
    pub fn tone(&self) -> NoticeTone {
        match self {
            Notice::Undo { outcome: Ok(_), .. }
            | Notice::Erased { failure: None, .. }
            | Notice::DriveSafe { .. } => NoticeTone::Success,
            Notice::Device(event) => match event.as_ref() {
                DeviceEvent::Ejected { .. } => NoticeTone::Success,
                DeviceEvent::EjectedNotPoweredOff { .. }
                | DeviceEvent::DisconnectedWhileViewing { .. } => NoticeTone::Information,
                _ => NoticeTone::Warning,
            },
            Notice::WritingImage { .. }
            | Notice::Mounting(_)
            | Notice::FolderGone(_)
            | Notice::RemoteFolderGone { .. } => NoticeTone::Information,
            Notice::Launch(
                LaunchReport::Started { .. }
                | LaunchReport::Activated { .. }
                | LaunchReport::FellBackToProcess { .. },
            ) => NoticeTone::Information,
            _ => NoticeTone::Warning,
        }
    }

    /// What goes under the message, in smaller words: the server's own
    /// account of a failure.
    pub fn detail(&self) -> Option<String> {
        match self {
            Notice::MountFailed { error, .. } => {
                Some(error.detail.trim().to_string()).filter(|detail| !detail.is_empty())
            }
            _ => None,
        }
    }

    pub fn message(&self, c: &'static Copy) -> String {
        match self {
            Notice::Path(rejection) => rejection.message(c).to_string(),
            Notice::FolderGone(name) => c.folder_gone.replace("{folder}", name),
            Notice::RemoteFolderGone { folder, server } => c
                .remote_folder_gone
                .replace("{folder}", folder)
                .replace("{server}", server),
            Notice::PasteNotDone {
                folder,
                server,
                why,
            } => match why {
                remote_tabs::PasteWhy::Left => c.remote_paste_left,
                remote_tabs::PasteWhy::Gone => c.remote_paste_gone,
                remote_tabs::PasteWhy::Failed => c.remote_paste_failed,
                remote_tabs::PasteWhy::Cancelled => c.remote_paste_cancelled,
            }
            .replace("{folder}", folder)
            .replace("{server}", server),
            Notice::RetryGone => c.retry_gone.to_string(),
            Notice::DriveFailed { drive, detail } => c
                .drive_failed
                .replace("{drive}", drive)
                .replace("{detail}", &crate::errors::explain(detail, c)),
            Notice::AutoMountUnchanged { drive, outcome } => c
                .auto_mount_unchanged
                .replace("{drive}", drive)
                .replace("{reason}", &auto_mount_reason(outcome, c)),
            Notice::WrongPassphrase { drive } => c.drive_wrong_passphrase.replace("{drive}", drive),
            Notice::DriveUnlockedNotMounted { drive, detail } => c
                .drive_unlocked_not_mounted
                .replace("{drive}", drive)
                .replace("{detail}", &crate::errors::explain(detail, c)),
            Notice::DriveSafe { drive } => c.drive_safe.replace("{drive}", drive),
            Notice::DriveNotPoweredOff { drive, detail } => c
                .drive_not_powered_off
                .replace("{drive}", drive)
                .replace("{detail}", &crate::errors::explain(detail, c)),
            Notice::DriveInUse {
                drive,
                volume,
                detail,
            } => c
                .drive_in_use
                .replace("{drive}", drive)
                .replace("{volume}", volume)
                .replace("{detail}", &crate::errors::explain(detail, c)),
            Notice::DriveEjectFailed { drive, detail } => c
                .drive_eject_failed
                .replace("{drive}", drive)
                .replace("{detail}", &crate::errors::explain(detail, c)),
            Notice::Command(refusal) => match refusal {
                CommandRefusal::NothingToActOn => c.nothing_selected.to_string(),
                CommandRefusal::NotAFilesystemLocation => c.not_writable_here.to_string(),
                CommandRefusal::UnusableName => c.name_not_usable.to_string(),
                CommandRefusal::NotInTrash => c.refusal_in_trash.to_string(),
                CommandRefusal::NotAnArchive => c.not_an_archive.to_string(),
            },
            Notice::Refused(refusal) => crate::i18n::refusal_label(*refusal, c).to_string(),
            Notice::Navigation(error) => match error {
                NavigationError::LastTab => c.last_tab_stays_open.to_string(),
                NavigationError::NothingToRestore => c.nothing_to_reopen.to_string(),
                NavigationError::NoSuchTab(_) => crate::errors::explain(&error.to_string(), c),
            },
            Notice::Undo { undoing, outcome } => match outcome {
                Ok(kind) => (if *undoing { c.undo_done } else { c.redo_done })
                    .replace("{kind}", crate::undo::kind_label(*kind, c)),
                Err(refusal) => refusal.message(*undoing, c),
            },
            Notice::NoTerminal => c.no_terminal.to_string(),
            Notice::WritingImage { label, percent } => c
                .image_writing
                .replace("{device}", label)
                .replace("{percent}", &percent.to_string()),
            Notice::Erased {
                label,
                format,
                failure,
            } => match (format, failure) {
                (true, None) => c.format_done.replace("{device}", label),
                (false, None) => c.image_written.replace("{device}", label),
                (_, Some(reason)) => format!(
                    "{}: {}",
                    c.erase_failed.replace("{device}", label),
                    crate::errors::explain(reason, c)
                ),
            },
            Notice::Mounting(address) => format!("{} {address}", c.server_connecting),
            Notice::MountFailed { address, error } => {
                use crate::network::MountFailure as F;
                let server = crate::network::server_name(address);
                match error.failure {
                    F::Cancelled | F::Failed => c.server_failed,
                    F::NoGvfs => c.server_no_gvfs,
                    F::TimedOut => c.server_timed_out,
                    F::HostNotFound => c.server_not_found,
                    F::Refused => c.server_refused,
                    F::PermissionDenied => c.server_denied,
                    F::ShareGone => c.remote_share_gone,
                    F::NetworkDown => c.remote_no_network,
                    F::HostKeyChanged => c.server_key_changed,
                    F::NotSupported => c.server_not_supported,
                    F::NoLocalFolder => c.server_no_folder,
                    F::UntrustedGvfs => c.server_untrusted_gvfs,
                }
                .replace("{server}", &server)
                .replace(
                    "{share}",
                    &crate::remote::Remote::from_address(address)
                        .map(|remote| remote.folder_name())
                        .unwrap_or_default(),
                )
            }
            Notice::AlreadyPinned => c.already_pinned.to_string(),
            Notice::NotPinnable => c.not_writable_here.to_string(),
            Notice::Rejected(error) => crate::errors::explain(&error.to_string(), c),
            Notice::Key(key) => crate::errors::explain(key, c),
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
                // The reason is the storage layer's own sentence. It is shown
                // as it came, because a paraphrase could hide what was wrong.
                DeviceEvent::PolicyRefused { label, detail } => {
                    format!("{label}: {} ({detail})", c.device_policy_refused)
                }
            },
            Notice::NoMimeType => c.open_with_no_mime_type.to_string(),
            Notice::DefaultApplicationMissing => c.open_with_default_missing.to_string(),
            Notice::Start { problem, ignored } => {
                let reason = problem.map(|problem| match problem {
                    StartProblem::NotLocal => c.start_not_local,
                    StartProblem::Malformed => c.start_malformed,
                    StartProblem::NotFound => c.start_not_found,
                    StartProblem::Unreadable => c.start_unreadable,
                });
                let ignored = (*ignored > 0)
                    .then(|| c.start_ignored.replace("{count}", &ignored.to_string()));
                match (reason, ignored) {
                    (Some(reason), Some(ignored)) => format!("{reason} {ignored}"),
                    (Some(reason), None) => reason.to_string(),
                    (None, Some(ignored)) => ignored,
                    (None, None) => String::new(),
                }
            }
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
    /// Compress, waiting for a format. The request holds the selection it was
    /// opened for, and the chooser which format the keyboard is on.
    Compress(CompressChooser),
    /// An encrypted drive that wants its passphrase before it mounts
    /// (ticket 122): its key in the drive list, and its name.
    UnlockDrive {
        key: String,
        name: String,
    },
    /// Format a drive: the filesystem chosen, the name typed, and the
    /// warning that everything on it goes.
    Format {
        object_path: String,
        /// The identity of the device the dialog was opened for: another
        /// device later at the same object path closes it (ticket 171).
        identity: String,
        device_label: String,
        filesystem: storage_platform::FilesystemKind,
    },
    /// Write a disk image to a whole drive, after a warning that names
    /// every volume on it.
    WriteImage {
        object_path: String,
        identity: String,
        device_label: String,
        image: LocalPath,
        volumes: Vec<String>,
    },
}

/// One window's worth of state.
pub struct FilesSession {
    pub locale: Locale,
    /// The language, the preview pane, the search's own hidden setting, and
    /// a mirror of the view the folder in front of the person has: its mode,
    /// zoom, sort order, and hidden-file visibility. The mirror is filled
    /// from [`ViewState`] every time the folder changes.
    pub preferences: FilesPreferences,
    preference_store: PreferenceStore,
    /// How each folder is shown, and the last folder, under the XDG state
    /// directory.
    view_store: ViewStateStore,
    pub views: ViewState,
    /// Undo and redo of the file changes this window made.
    pub history: crate::undo::History,
    /// The jobs this window submitted that undo will cover once they finish,
    /// with what each was asked to do.
    undoable: Vec<(files_operations::JobId, files_operations::Operation)>,
    /// Server folders (ticket 310): GVfs's folder, its mount list, the
    /// connections being made and the tabs waiting on them.
    pub(crate) remote: remote_tabs::RemoteSide,
    /// The question shown in the content area, with where its answer goes.
    pub server_prompt: Option<crate::network::Ask>,
    /// Counts the questions shown, so the window readies its fields once
    /// for each.
    pub server_prompt_serial: u64,
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
    /// How the sidebar looks at folders: one on a network or FUSE
    /// filesystem never on the window's thread (ticket 310).
    pub probes: crate::sidebar::BackgroundProbe,
    pub operations_open: bool,
    /// Copies and moves an earlier Better Files left unfinished, offered
    /// in the operation center until resumed or dismissed.
    pub interrupted: crate::interrupted::InterruptedJobs,
    pub jobs: Vec<JobSnapshot>,
    pub finished: SessionHistory,
    /// The finished jobs already filed into `finished`, so a job the engine
    /// still lists is filed (and its folders refreshed) once, not every frame.
    /// Pruned to what the engine still lists, so it never grows with the
    /// jobs of the past.
    recorded: std::collections::HashSet<JobId>,
    /// Each pane's folder watch (ticket 119).
    watches: Vec<FolderWatch>,
    /// The bookmarks file's folder, followed so outside edits show.
    bookmark_watch: Option<files_platform::Subscription>,
    /// The preferences as last read or written, against which this window's
    /// own changes are told apart from another window's.
    preferences_baseline: FilesPreferences,
    /// The preferences file's folder, followed so widths are shared.
    preference_watch: Option<files_platform::Subscription>,
    /// The watch hub every folder of this window is followed through: the
    /// process's, or a test thread's own.
    hub: Arc<files_platform::WatchHub>,
    /// Devices being ejected and the folders whose watches were let go of.
    ejecting: Vec<(String, std::path::PathBuf)>,
    /// Every drive and how full it is (ticket 122), as last read.
    drives: Vec<crate::drives::Drive>,
    /// Reads the drives off this thread; `None` until the window gives it
    /// its sources (tests read no real drives).
    drive_scanner: Option<crate::drives::DriveScanner>,
    drive_sources: Option<crate::drives::DriveSources>,
    /// Each finished drive action, by the drive's key.
    drive_outcomes: (
        std::sync::mpsc::Sender<KeyedOutcome>,
        std::sync::mpsc::Receiver<KeyedOutcome>,
    ),
    last_drive_scan: std::time::Instant,
    /// The drives with a mount, unlock or eject still running.
    busy_drives: Vec<String>,
    /// Whether the window is on screen (the drives' timer runs only then).
    window_visible: bool,
    /// This window's claim on the shared engine's finished jobs.
    _engine_window: files_operations::WindowAttachment,

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
    /// The large preview's program when a test names it; `None` is the
    /// companion rule's.
    preview_program: Option<String>,
    /// The folder each server this window mounted was mounted at, by the
    /// address it was asked for, so the sidebar can light that server's row
    /// (ticket 175).
    server_folders: Vec<(String, std::path::PathBuf)>,
    /// The network or FUSE mount the last folder asked about is on, read
    /// from the kernel's mount table once per folder.
    foreign_mount: Option<(std::path::PathBuf, Option<std::path::PathBuf>)>,
    pub search: SearchState,
    /// How much each folder in view holds (ticket 126).
    pub folder_sizes: crate::folder_sizes::FolderSizes,
    /// The application sizes the panes last listed with (ticket 128).
    application_sizes_seen: u64,
    /// The file the embedded chooser is open for, when it is open.
    pub chooser: Option<ChooserRequest>,
    /// The application whose details panel is open.
    pub details: Option<ApplicationDetails>,
    /// The Performance mode confirmation, while it is open.
    pub policy_confirmation: Option<PerformanceConfirmation>,
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
    pub view_store: ViewStateStore,
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
            view_store: ViewStateStore::beside(preference_store.path()),
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
            mut preferences,
            preference_store,
            view_store,
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
        let mut views = view_store.load();
        views.set_last_folder(local_folder(&start).as_deref());
        mirror(
            &mut preferences,
            views.view_for(local_folder(&start).as_deref()),
        );
        reader.set_include_hidden_applications(preferences.show_hidden);
        let view = preferences.view_preferences();
        let gvfs_root = reader.gvfs_root().map(std::path::Path::to_path_buf);
        let bookmarks = load_bookmarks(&bookmark_store, gvfs_root.as_deref(), true);
        let mount_list: Arc<dyn crate::remote::MountsSource> = match &gvfs_root {
            Some(root) => Arc::new(crate::remote::GvfsTracker::session(Some(root.clone()))),
            None => Arc::new(crate::remote::NoMounts),
        };
        // The process's watch hub, or this thread's own (a test's).
        let hub = files_platform::WatchHub::current();
        let preference_watch = preference_store
            .path()
            .parent()
            .filter(|folder| folder.is_dir())
            .map(|folder| hub.subscribe(folder, files_platform::ReaderConfig::new()));
        let bookmark_watch = bookmark_store
            .path()
            .parent()
            .filter(|folder| folder.is_dir())
            .map(|folder| hub.subscribe(folder, files_platform::ReaderConfig::new()));
        let tabs = TabSet::with_preferences(start.clone(), view);
        let first = tabs.active().id();
        let pane = Pane::open(start, view, reader.as_ref());
        let engine_window = engine.attach_window();
        Self {
            locale: Locale::from_preference(preferences.locale),
            preferences,
            preference_store,
            view_store,
            views,
            history: crate::undo::History::default(),
            remote: remote_tabs::RemoteSide::new(
                gvfs_root,
                mount_list,
                Arc::new(crate::network::SystemMounter),
            ),
            server_prompt: None,
            server_prompt_serial: 0,
            undoable: Vec::new(),
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
            probes: crate::sidebar::BackgroundProbe::default(),
            operations_open: false,
            interrupted: crate::interrupted::InterruptedJobs::default(),
            jobs: Vec::new(),
            finished: SessionHistory::default(),
            recorded: std::collections::HashSet::new(),
            watches: Vec::new(),
            bookmark_watch,
            preferences_baseline: preferences,
            preference_watch,
            hub: Arc::clone(&hub),
            ejecting: Vec::new(),
            drives: Vec::new(),
            drive_scanner: None,
            drive_sources: None,
            drive_outcomes: std::sync::mpsc::channel(),
            last_drive_scan: std::time::Instant::now(),
            busy_drives: Vec::new(),
            window_visible: true,
            _engine_window: engine_window,
            catalog,
            defaults,
            spawner,
            collection: link.mode(),
            link,
            devices: DeviceInventory::default(),
            pending_open: None,
            preview,
            preview_program: None,
            server_folders: Vec::new(),
            foreign_mount: None,
            search: SearchState::default(),
            folder_sizes: crate::folder_sizes::FolderSizes::default()
                .watching_with(Arc::clone(&hub)),
            application_sizes_seen: 0,
            chooser: None,
            details: None,
            policy_confirmation: None,
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

    /// What the folder at `location` is, for its icon (ticket 120).
    pub fn folder_glyph(&self, location: &Location) -> crate::icons::FolderGlyph {
        crate::icons::folder_glyph(location, &self.directories, &self.mounts)
    }

    /// Opens a tab. `activate` false is "open in background", which is what
    /// middle-clicking a folder does.
    pub fn open_tab(&mut self, location: Location, activate: bool) -> TabId {
        let id = self.tabs.open(location.clone(), activate);
        let view = self
            .views
            .view_for(local_folder(&location).as_deref())
            .view_preferences();
        let pane = Pane::open(location, view, self.reader.as_ref());
        self.panes.push((id, pane));
        if activate {
            self.adopt_folder_view();
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
                self.adopt_folder_view();
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
                self.adopt_folder_view();
                self.resync_content();
            }
            Err(error) => self.notice = Some(Notice::Navigation(error)),
        }
    }

    pub fn activate_tab(&mut self, id: TabId) {
        if self.tabs.activate(id).is_ok() {
            self.adopt_folder_view();
            self.resync_content();
            self.refresh_if_polled(id);
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
        self.forget_open_folder();
        let reader = self.reader.clone();
        self.pane_mut().navigate_to(location, reader.as_ref());
        self.after_navigation();
    }

    pub fn go_back(&mut self) {
        self.forget_open_folder();
        let reader = self.reader.clone();
        self.pane_mut().go_back(reader.as_ref());
        self.after_navigation();
    }

    pub fn go_forward(&mut self) {
        self.forget_open_folder();
        let reader = self.reader.clone();
        self.pane_mut().go_forward(reader.as_ref());
        self.after_navigation();
    }

    pub fn go_to_parent(&mut self) {
        self.forget_open_folder();
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
        // A filter belongs to the folder it was typed in.
        self.pane_mut().model_mut().set_name_filter("");
        self.adopt_folder_view();
        let folder = local_folder(self.location());
        if folder.is_some() && self.views.last_folder() != folder.as_deref() {
            self.views.set_last_folder(folder.as_deref());
            self.persist_views();
        }
        self.content.clear_type_ahead();
        self.resync_content();
        // A search belongs to the location it was started in. Carrying it into
        // the next folder would show results from a place the user has left.
        self.search.clear();
        self.details = None;
        // A folder on a server whose connection the rule already knows is
        // not there starts connecting now (ticket 310).
        self.link_after_navigation();
        self.refresh_preview();
        // The new folder's watch is asked for now, in the frame its listing
        // started, not on the next pump.
        self.pump_watches();
    }

    /// What a tab that came back from its server needs, as a navigation
    /// does: the cursor, the preview, the watch.
    fn after_landing(&mut self) {
        let id = self.active_tab();
        self.sync_history(id);
        self.adopt_folder_view();
        self.resync_content();
        self.refresh_preview();
        self.pump_watches();
    }

    /// Takes what the user typed in the path field.
    pub fn submit_path(
        &mut self,
        text: &str,
        home: Option<&std::path::Path>,
        validator: &dyn PathValidator,
    ) {
        if let Some(address) = crate::network::server_address(text) {
            self.note_address_typed(&address);
            self.mount_server(address);
            return;
        }
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
    /// immediate and starts no listing; the choice is remembered for this
    /// folder.
    pub fn toggle_hidden(&mut self) {
        let shown = self.pane_mut().toggle_hidden();
        self.preferences.show_hidden = shown;
        // The Applications location has its own hidden rule — an excluded or
        // `NoDisplay` entry — and it follows the same key.
        self.reader.set_include_hidden_applications(shown);
        self.sync_active_tab_view();
        self.record_view();
        // Applications leaves its hidden entries out when it reads the
        // catalog, so showing them means reading it again.
        if matches!(self.location(), Location::Applications) {
            let reader = self.reader.clone();
            self.pane_mut().reload(reader.as_ref());
        }
        self.resync_content();
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.preferences.view_mode = mode;
        self.content.mode = mode;
        self.record_view();
    }

    pub fn toggle_view_mode(&mut self) {
        self.set_view_mode(self.preferences.view_mode.toggled());
    }

    pub fn set_scale(&mut self, scale: ItemScale) {
        self.preferences.scale = scale;
        self.content.scale = scale;
        self.record_view();
    }

    pub fn set_order(&mut self, order: SortOrder) {
        self.preferences.set_order(order);
        let order = self.preferences.order();
        self.pane_mut().set_order(order);
        self.sync_active_tab_view();
        self.record_view();
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

    /// `S` and `Shift+S`: the next or previous sort, through name, type,
    /// newest, oldest and size, the way folio steps through its sort menu.
    pub fn step_sort(&mut self, forward: bool) {
        let next = crate::context_menu::step_sort(self.preferences.order(), forward);
        self.set_order(next);
    }

    /// "Use this view for every folder": the folder's view becomes the
    /// default and every other folder's own view is forgotten.
    pub fn use_view_everywhere(&mut self) {
        self.views.apply_everywhere(self.current_view());
        self.persist_views();
    }

    /// Whether the folder in front of the person has a view of its own.
    pub fn folder_has_own_view(&self) -> bool {
        local_folder(self.location()).is_some_and(|path| self.views.has_override(&path))
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        self.preferences.locale = locale.to_preference();
        self.persist_preferences();
    }

    /// The view the person is looking at, as stored.
    fn current_view(&self) -> FolderView {
        FolderView::new(
            self.preferences.view_mode,
            self.preferences.scale,
            self.preferences.show_hidden,
            self.preferences.order(),
        )
    }

    /// Takes the stored view of the folder in front of the person and shows
    /// it: the mode and zoom, and the pane's sort and hidden filter.
    fn adopt_folder_view(&mut self) {
        let mut view = self
            .views
            .view_for(local_folder(self.location()).as_deref());
        // Recents reads newest first, whatever the folders are sorted by.
        if matches!(self.location(), Location::Recent) {
            view.set_order(SortOrder::new(
                SortKey::Modified,
                files_core::SortDirection::Descending,
            ));
        }
        mirror(&mut self.preferences, view);
        self.content.mode = view.mode;
        self.content.scale = view.scale;
        let order = view.order();
        let hidden = view.hidden();
        let pane = self.pane_mut();
        if pane.preferences().order != order {
            pane.set_order(order);
        }
        if pane.preferences().hidden != hidden {
            pane.set_hidden_preference(hidden);
        }
        self.reader
            .set_include_hidden_applications(view.show_hidden);
        self.sync_active_tab_view();
    }

    /// Keeps the tab's own copy of the view in step with its pane, so a tab
    /// closed and reopened comes back the way it looked.
    fn sync_active_tab_view(&mut self) {
        let id = self.active_tab();
        let order = self.preferences.order();
        let hidden = self.preferences.hidden();
        if let Some(tab) = self.tabs.get_mut(id) {
            let preferences = tab.preferences_mut();
            preferences.order = order;
            preferences.hidden = hidden;
        }
    }

    /// Remembers the current view for the folder in front of the person.
    fn record_view(&mut self) {
        let folder = local_folder(self.location());
        self.views.set_view(folder.as_deref(), self.current_view());
        self.persist_views();
    }

    fn persist_views(&mut self) {
        if let Err(error) = self.view_store.save(&self.views) {
            self.notice = Some(Notice::Key(format!("files.prefs.error.unwritable:{error}")));
        }
    }

    /// Stores the preferences now; for a change the window made itself, such
    /// as a pane's width after its edge was dragged.
    pub fn save_preferences(&mut self) {
        self.persist_preferences();
    }

    /// Writes the fields this window changed over what is on disk now, so
    /// another window's changes since are kept.
    fn persist_preferences(&mut self) {
        let disk = self.preference_store.load().preferences;
        let merged =
            crate::prefs::merge_preferences(&self.preferences_baseline, &self.preferences, &disk);
        if let Err(error) = self.preference_store.save(&merged) {
            self.notice = Some(Notice::Key(format!("files.prefs.error.unwritable:{error}")));
            return;
        }
        self.preferences = merged;
        self.preferences_baseline = merged;
    }

    /// Takes the pane widths another window stored (ticket 118: one width
    /// for every window), when the preferences file changed. The other
    /// fields this window applied when it opened are left as they are: a
    /// view mode or a hidden-files choice changes what a window shows, and
    /// it is changed by the window the person is using.
    fn pump_preferences(&mut self) -> bool {
        let Some(subscription) = &self.preference_watch else {
            return false;
        };
        let name = self
            .preference_store
            .path()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let touched = subscription.poll().into_iter().any(|update| match update {
            files_platform::HubUpdate::Rows(rows) => rows.iter().any(|row| match row {
                files_core::RefreshEvent::Added(entry)
                | files_core::RefreshEvent::Modified(entry) => {
                    name.as_deref() == Some(entry.name.as_str())
                }
                _ => false,
            }),
            files_platform::HubUpdate::Resynchronize | files_platform::HubUpdate::Ready { .. } => {
                true
            }
            _ => false,
        });
        if !touched {
            return false;
        }
        let disk = self.preference_store.load().preferences;
        let mut changed = false;
        for (ours, base, theirs) in [
            (
                &mut self.preferences.sidebar_width,
                &mut self.preferences_baseline.sidebar_width,
                disk.sidebar_width,
            ),
            (
                &mut self.preferences.preview_width,
                &mut self.preferences_baseline.preview_width,
                disk.preview_width,
            ),
            (
                &mut self.preferences.operations_width,
                &mut self.preferences_baseline.operations_width,
                disk.operations_width,
            ),
        ] {
            // A width this window is changing right now is its own.
            if *ours == *base && *ours != theirs {
                *ours = theirs;
                changed = true;
            }
            *base = theirs;
        }
        changed
    }

    // --- Selection and opening --------------------------------------------

    /// Rubber-band selection: exactly the content-area rows given, or those
    /// added to what was selected when `extend` (Ctrl held).
    pub fn select_rows(&mut self, rows: &[usize], base: &[files_core::EntryId]) {
        let ids: Vec<files_core::EntryId> = rows
            .iter()
            .filter_map(|row| self.model_row(*row))
            .filter_map(|row| self.pane().model().visible(row).map(Entry::id))
            .chain(base.iter().cloned())
            .collect();
        let selection = self.pane_mut().model_mut().selection_mut();
        selection.select_all(&ids);
        self.resync_content();
        self.refresh_preview();
    }

    pub fn apply_selection(&mut self, input: SelectionInput, columns: usize) -> Option<usize> {
        let mut content = std::mem::take(&mut self.content);
        let scrolled = content.apply(self.pane_mut().model_mut(), input, columns);
        self.content = content;
        // The side preview follows the selection at once: a click changes
        // nothing the frame pump watches, so waiting for it left the pane
        // saying "select something" with something selected.
        self.refresh_preview();
        scrolled
    }

    pub fn type_ahead(&mut self, character: char, now: std::time::Instant) -> Option<usize> {
        let mut content = std::mem::take(&mut self.content);
        let scrolled = content.type_ahead_key(self.pane_mut().model_mut(), character, now);
        self.content = content;
        self.refresh_preview();
        scrolled
    }

    /// Selects the entry with this display name in the active pane, whether or
    /// not the listing has delivered it yet.
    ///
    /// The selection names an entry rather than an index, so it survives the
    /// listing streaming in around it, and the cursor lands on the entry the
    /// moment it arrives.
    pub fn select_by_name(&mut self, name: &str) {
        self.pane_mut()
            .model_mut()
            .selection_mut()
            .select_only(EntryId::Name(name.to_string()));
        self.resync_content();
        self.refresh_preview();
    }

    /// The entries currently selected, in visible order.
    pub fn selected_entries(&self) -> Vec<&Entry> {
        let model = self.pane().model();
        model
            .iter_visible()
            .filter(|entry| model.selection().contains(&entry.id()))
            .collect()
    }

    /// The size the Applications view has for one application, for its
    /// details panel (ticket 128).
    pub fn application_size(&self, desktop_id: &str) -> Option<&files_core::ApplicationSize> {
        let entry = self
            .pane()
            .model()
            .get(&files_core::EntryId::Application(desktop_id.to_string()))?;
        match &entry.body {
            files_core::EntryBody::Application(facts) => Some(&facts.size),
            _ => None,
        }
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
                // A file on a server that is not connected (a recent one):
                // its folder opens, connecting first, with it selected
                // (ticket 310).
                if let Some(folder) = path.parent().map(Location::Local)
                    && matches!(
                        crate::remote::need(
                            &folder,
                            self.remote.gvfs_root.as_deref(),
                            &self.remote.watch.current()
                        ),
                        crate::remote::Need::Connect(_)
                    )
                {
                    self.notice = None;
                    self.navigate_to(folder);
                    self.select_by_name(&path.file_name());
                    return;
                }
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
    /// Format… from a drive's menu: opens the dialog that names the drive,
    /// offers the filesystems and says that everything on it goes.
    pub fn request_format(&mut self, object_path: &str) {
        if self.refuse_if_system(object_path) {
            return;
        }
        let Some(identity) = self
            .devices
            .get(object_path)
            .map(|row| row.identity.clone())
        else {
            return;
        };
        self.dialog = Some(PendingDialog::Format {
            object_path: object_path.to_string(),
            identity,
            device_label: self.device_label(object_path),
            filesystem: storage_platform::FilesystemKind::Exfat,
        });
    }

    /// Write to Drive… for a disk image: opens the confirmation, which
    /// names the drive and every volume on it.
    pub fn request_write_image(&mut self, image: LocalPath, object_path: &str) {
        let Some(target) = self
            .image_targets()
            .into_iter()
            .find(|target| target.object_path == object_path)
        else {
            return;
        };
        let Some(identity) = self
            .devices
            .get(&target.object_path)
            .map(|row| row.identity.clone())
        else {
            return;
        };
        self.dialog = Some(PendingDialog::WriteImage {
            identity,
            object_path: target.object_path,
            device_label: target.name,
            image,
            volumes: target.volumes,
        });
    }

    /// The person confirmed a Format or an image write: only now does the
    /// request go to the storage layer, which refuses an unconfirmed one.
    pub fn confirm_erase(&mut self, label: &str) {
        let request = match self.dialog.take() {
            Some(PendingDialog::Format {
                object_path,
                identity,
                filesystem,
                ..
            }) => {
                let format = storage_platform::FormatRequest {
                    kind: filesystem,
                    label: label.to_string(),
                };
                if let Err(reason) = format.validate() {
                    self.notice = Some(Notice::Key(format!("storage.format.label:{reason}")));
                    return;
                }
                crate::devices::EraseRequest::Format {
                    object_path,
                    identity,
                    filesystem,
                    label: label.trim().to_string(),
                }
            }
            Some(PendingDialog::WriteImage {
                object_path,
                identity,
                image,
                ..
            }) => crate::devices::EraseRequest::WriteImage {
                object_path,
                identity,
                image: image.as_path().to_path_buf(),
            },
            other => {
                self.dialog = other;
                return;
            }
        };
        // The device at that path must still be the one the person
        // confirmed: a stick pulled and another plugged in can take the
        // same object path (ticket 171).
        let (object_path, identity) = match &request {
            crate::devices::EraseRequest::Format {
                object_path,
                identity,
                ..
            }
            | crate::devices::EraseRequest::WriteImage {
                object_path,
                identity,
                ..
            } => (object_path.clone(), identity.clone()),
        };
        if self
            .devices
            .get(&object_path)
            .is_none_or(|row| row.identity != identity)
        {
            self.notice = Some(Notice::Key(DEVICE_CHANGED.to_string()));
            return;
        }
        // Asked again: the drive may have become the system's while the
        // dialog was open.
        if self.refuse_if_system(request.object_path()) {
            return;
        }
        self.link.request_erase(request);
    }

    /// Closes a Format or Write to Drive dialog whose device is gone, or is
    /// now another device at the same object path, and says so.
    fn close_erase_for_changed_device(&mut self) {
        let (object_path, identity) = match &self.dialog {
            Some(PendingDialog::Format {
                object_path,
                identity,
                ..
            })
            | Some(PendingDialog::WriteImage {
                object_path,
                identity,
                ..
            }) => (object_path.clone(), identity.clone()),
            _ => return,
        };
        if self
            .devices
            .get(&object_path)
            .is_none_or(|row| row.identity != identity)
        {
            self.dialog = None;
            self.notice = Some(Notice::Key(DEVICE_CHANGED.to_string()));
        }
    }

    /// The sentence a Format or Write to Drive confirmation shows for a
    /// device that cannot be told from another one like it (ticket 171).
    pub fn erase_caution(&self) -> Option<String> {
        let object_path = match &self.dialog {
            Some(PendingDialog::Format { object_path, .. })
            | Some(PendingDialog::WriteImage { object_path, .. }) => object_path,
            _ => return None,
        };
        let row = self.devices.get(object_path)?;
        crate::devices::erase_caution(row, crate::i18n::copy(self.locale))
    }

    /// What of the running system the drive of the storage service's
    /// device holds (ticket 171): the drive listing's answer and the
    /// service's together, the stronger winning. A device the window does
    /// not know is unknown.
    pub fn device_system_use(&self, object_path: &str) -> crate::drives::SystemUse {
        let row = self.devices.get(object_path);
        let drive = row.and_then(|row| {
            self.drives
                .iter()
                .find(|drive| crate::drives::is_device(drive, row))
        });
        crate::drives::system_use_of(drive, row)
    }

    /// Says why, and is true, when the device's drive holds the running
    /// system or cannot be told apart from one: nothing is ejected or
    /// erased then, whatever asked for it.
    fn refuse_if_system(&mut self, object_path: &str) -> bool {
        match self.device_system_use(object_path).refusal_detail() {
            Some(reason) => {
                self.notice = Some(Notice::Key(reason));
                true
            }
            None => false,
        }
    }

    /// Picks the filesystem in an open Format dialog.
    pub fn choose_filesystem(&mut self, kind: storage_platform::FilesystemKind) {
        if let Some(PendingDialog::Format { filesystem, .. }) = &mut self.dialog {
            *filesystem = kind;
        }
    }

    /// The drives an image could be written to (whole drives, never a
    /// partition), each with the volumes writing to it erases.
    pub fn image_targets(&self) -> Vec<crate::devices::ImageTarget> {
        crate::devices::image_targets(self.devices.rows())
    }

    /// Mount from a drive's menu: asks the storage layer to mount it,
    /// without going there.
    pub fn mount_device(&mut self, object_path: &str) {
        self.link.request_mount(object_path);
    }

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
        if self.refuse_if_system(object_path) {
            return;
        }
        if let Some(folder) = self
            .devices
            .get(object_path)
            .and_then(|row| row.location())
            .and_then(|location| {
                location
                    .as_local_path()
                    .map(|path| path.as_path().to_path_buf())
            })
            // `/` is never let go of: that would stop every watch there is.
            .filter(|folder| folder != std::path::Path::new("/"))
        {
            // Every window's watches under the device go first; they come
            // back if the eject does not happen.
            self.hub.suspend_under(&folder);
            self.ejecting.push((object_path.to_string(), folder));
        }
        self.link.request_eject(object_path);
    }

    /// An eject ended, done or not: the watches it let go of are asked for
    /// again (an unmounted folder answers that it is gone).
    fn end_eject(&mut self, object_path: &str) {
        let (ending, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.ejecting)
            .into_iter()
            .partition(|(path, _)| path == object_path);
        self.ejecting = rest;
        for (_, folder) in ending {
            self.hub.resume_under(&folder);
        }
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
        // A device that came, went, mounted or ejected changes the drives.
        self.refresh_drives();
        for notice in notices {
            self.apply_device_notice(notice);
        }
        true
    }

    fn apply_device_notice(&mut self, notice: DeviceNotice) {
        match notice {
            DeviceNotice::Mode(mode) => self.collection = mode,
            DeviceNotice::ImageProgress {
                object_path,
                written,
                total,
            } => {
                self.notice = Some(Notice::WritingImage {
                    label: self.drive_name(&object_path),
                    percent: (written.saturating_mul(100))
                        .checked_div(total)
                        .unwrap_or(0)
                        .min(100) as u8,
                });
            }
            DeviceNotice::Erased {
                object_path,
                format,
                outcome,
            } => {
                self.notice = Some(Notice::Erased {
                    label: if format {
                        self.device_label(&object_path)
                    } else {
                        self.drive_name(&object_path)
                    },
                    format,
                    failure: outcome.err(),
                });
                self.link.request_refresh();
            }
            DeviceNotice::Inventory(reports) => {
                self.devices.apply_inventory(reports);
                self.close_erase_for_changed_device();
            }
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
                self.end_eject(&object_path);
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
                self.end_eject(&object_path);
                self.notice = Some(Notice::Device(Box::new(DeviceEvent::EjectFailed {
                    label: self.device_label(&object_path),
                    detail,
                })));
            }
            DeviceNotice::Disconnected {
                object_path,
                unsafe_removal,
            } => {
                self.handle_disconnect(&object_path, unsafe_removal);
                self.close_erase_for_changed_device();
            }
            // Nothing to draw yet: the row changes when an inventory reports
            // the new policy, not when a request for one was accepted.
            DeviceNotice::PolicyApplied { .. } => {}
            DeviceNotice::PolicyRefused {
                object_path,
                detail,
            } => {
                self.notice = Some(Notice::Device(Box::new(DeviceEvent::PolicyRefused {
                    label: self.device_label(&object_path),
                    detail,
                })));
            }
        }
    }

    // --- Drives (ticket 122) -----------------------------------------------

    /// Starts reading the drives from `sources`, now and whenever they may
    /// have changed.
    pub fn set_drive_sources(&mut self, sources: crate::drives::DriveSources) {
        self.drive_scanner = Some(crate::drives::DriveScanner::start(sources.clone()));
        self.drive_sources = Some(sources);
        self.last_drive_scan = std::time::Instant::now();
    }

    /// Every drive, as last read.
    pub fn drives(&self) -> &[crate::drives::Drive] {
        &self.drives
    }

    /// Reads the drives again, off this thread.
    pub fn refresh_drives(&mut self) {
        if let Some(scanner) = &self.drive_scanner {
            scanner.refresh();
            self.last_drive_scan = std::time::Instant::now();
        }
    }

    /// Takes a new list of drives and the outcome of a mount or eject;
    /// reads the drives again now and then while the window is visible.
    /// Whether the window has to redraw.
    fn pump_drives(&mut self) -> bool {
        let mut changed = false;
        if let Some((drives, system)) = self
            .drive_scanner
            .as_ref()
            .and_then(|scanner| scanner.take())
        {
            changed |= drives != self.drives;
            self.drives = drives;
            // The storage service's rows are judged by the same reading
            // (ticket 171).
            let before: Vec<_> = self
                .devices
                .rows()
                .iter()
                .map(|r| r.system_use.clone())
                .collect();
            self.devices.set_system(system);
            changed |= before
                != self
                    .devices
                    .rows()
                    .iter()
                    .map(|r| r.system_use.clone())
                    .collect::<Vec<_>>();
        }
        if self.window_visible && self.last_drive_scan.elapsed() >= crate::drives::DRIVES_REFRESH {
            self.refresh_drives();
        }
        while let Ok((key, outcome)) = self.drive_outcomes.1.try_recv() {
            changed = true;
            self.busy_drives.retain(|busy| *busy != key);
            self.refresh_drives();
            self.apply_drive_outcome(key, outcome);
        }
        changed
    }

    fn apply_drive_outcome(&mut self, key: String, outcome: DriveOutcome) {
        use crate::drives::{EjectError, Ejected, UnlockError};
        match outcome {
            DriveOutcome::Mounted(path) => {
                if let Ok(location) = Location::local(path) {
                    self.notice = None;
                    self.navigate_to(location);
                }
            }
            DriveOutcome::MountFailed { drive, detail } => {
                self.notice = Some(Notice::DriveFailed { drive, detail });
            }
            DriveOutcome::AutoMount {
                drive,
                outcome,
                after,
            } => {
                use drive_mounts::client::SetOutcome;
                self.notice = match (outcome, after) {
                    (SetOutcome::Cancelled, _) => None,
                    (SetOutcome::Done { .. }, None) => None,
                    (SetOutcome::Done { .. }, Some(detail)) => {
                        Some(Notice::DriveFailed { drive, detail })
                    }
                    (outcome, _) => Some(Notice::AutoMountUnchanged { drive, outcome }),
                };
            }
            DriveOutcome::Ejected {
                drive,
                folder,
                result,
            } => {
                if let Some(folder) = &folder {
                    self.hub.resume_under(folder);
                }
                self.notice = Some(match result {
                    Ok(Ejected::Safe) => Notice::DriveSafe { drive },
                    Ok(Ejected::NotPoweredOff(detail)) => {
                        Notice::DriveNotPoweredOff { drive, detail }
                    }
                    Err(EjectError::InUse { volume, detail }) => Notice::DriveInUse {
                        drive,
                        volume,
                        detail,
                    },
                    Err(EjectError::Failed(detail)) => Notice::DriveEjectFailed { drive, detail },
                });
            }
            DriveOutcome::Unlocked { drive, result } => match result {
                Ok(path) => self.apply_drive_outcome(key, DriveOutcome::Mounted(path)),
                Err(UnlockError::Dismissed) => {}
                Err(UnlockError::WrongPassphrase) => {
                    self.notice = Some(Notice::WrongPassphrase {
                        drive: drive.clone(),
                    });
                    // Asked again only while nothing else is open, and only
                    // while the container is still there and locked.
                    let still_locked = self
                        .drives
                        .iter()
                        .any(|known| known.key == key && known.locked);
                    if self.dialog.is_none() && still_locked {
                        self.dialog = Some(PendingDialog::UnlockDrive { key, name: drive });
                    }
                }
                Err(UnlockError::Unavailable) => {
                    self.notice = Some(Notice::DriveFailed {
                        drive,
                        detail: "UDisks2 is not available".to_string(),
                    });
                }
                Err(UnlockError::Failed(detail)) => {
                    self.notice = Some(Notice::DriveFailed { drive, detail });
                }
                // Unlocked, and the container's key has left the list: the
                // name kept from the start says which drive.
                Err(UnlockError::MountFailed(detail)) => {
                    self.notice = Some(Notice::DriveUnlockedNotMounted { drive, detail });
                }
            },
        }
    }

    /// The drives the sidebar and "This PC" list: the mounted ones, and the
    /// ones that should be and are not (ticket 127); every drive while
    /// "Show other drives" is on.
    pub fn shown_drives(&self) -> Vec<crate::drives::Drive> {
        self.drives
            .iter()
            .filter(|drive| {
                self.preferences.show_other_drives || drive_mounts::choice::shown(drive)
            })
            .cloned()
            .collect()
    }

    /// "Show other drives" in the window's `…` menu, remembered with the
    /// other preferences.
    pub fn toggle_show_other_drives(&mut self) {
        self.preferences.show_other_drives = !self.preferences.show_other_drives;
        self.persist_preferences();
    }

    /// Turns a drive's "Mount automatically" on or off through
    /// `manager-daemon` (polkit asks once), then mounts a removable one
    /// turned on, or unmounts one turned off that is still mounted, through
    /// UDisks2 as the person may. Off this thread.
    pub fn set_auto_mount(&mut self, key: &str, on: bool) {
        if self.is_drive_busy(key) {
            return;
        }
        let Some(drive) = self.drives.iter().find(|drive| drive.key == key).cloned() else {
            return;
        };
        let (Some(uuid), Some(auto)) = (drive.uuid.clone(), drive.auto_mount.clone()) else {
            return;
        };
        if auto.locked.is_some() || drive.kind == crate::drives::DriveKind::System {
            return;
        }
        let Some(sources) = self.drive_sources.clone() else {
            return;
        };
        let name = drive.name(crate::i18n::copy(self.locale));
        let busy = drive.key.clone();
        if !on && let Some(folder) = &drive.mount_point {
            self.hub.suspend_under(folder);
        }
        self.spawn_drive_action(&busy, "files-drive-auto-mount", move || {
            use drive_mounts::client::SetOutcome;
            let outcome = (sources.set_auto_mount)(&uuid, on);
            let after = match (&outcome, drive_mounts::choice::follow_up(&drive, on)) {
                (SetOutcome::Done { .. }, drive_mounts::choice::FollowUp::Mount) => {
                    (sources.mount)(&drive).err()
                }
                (SetOutcome::Done { .. }, drive_mounts::choice::FollowUp::Unmount) => {
                    (sources.unmount)(&drive).err()
                }
                _ => None,
            };
            DriveOutcome::AutoMount {
                drive: name,
                outcome,
                after,
            }
        });
    }

    /// A drive that is on and did not mount, tried again: an internal one
    /// through the daemon (which starts its mount unit), a removable one
    /// through UDisks2.
    pub fn retry_mount(&mut self, key: &str) {
        let Some(drive) = self.drives.iter().find(|drive| drive.key == key) else {
            return;
        };
        if drive.kind == crate::drives::DriveKind::Internal
            && drive
                .auto_mount
                .as_ref()
                .is_some_and(|auto| auto.on && auto.locked.is_none())
        {
            self.set_auto_mount(key, true);
        } else {
            self.open_drive(key);
        }
    }

    /// Whether a mount, unlock or eject started from this drive is still
    /// running: its row ignores clicks until it ends.
    pub fn is_drive_busy(&self, key: &str) -> bool {
        self.busy_drives.iter().any(|busy| busy == key)
    }

    /// Whether any drive action is still running.
    pub fn any_drive_busy(&self) -> bool {
        !self.busy_drives.is_empty()
    }

    /// Opens a drive: goes there when it is mounted; asks for the
    /// passphrase of an encrypted one; otherwise mounts it (through the
    /// storage service when it reports the device, through UDisks2
    /// otherwise, off this thread) and goes there once it is. Whether it
    /// went there now.
    pub fn open_drive(&mut self, key: &str) -> bool {
        if self.is_drive_busy(key) {
            return false;
        }
        let Some(drive) = self.drives.iter().find(|drive| drive.key == key).cloned() else {
            return false;
        };
        if let Some(point) = &drive.mount_point {
            let Ok(location) = Location::local(point) else {
                return false;
            };
            self.navigate_to(location);
            return true;
        }
        let c = crate::i18n::copy(self.locale);
        if drive.locked {
            self.dialog = Some(PendingDialog::UnlockDrive {
                key: drive.key.clone(),
                name: drive.name(c),
            });
            return false;
        }
        let Some(object_path) = drive.object_path.clone() else {
            return false;
        };
        if self.devices.get(&object_path).is_some() {
            self.open_device(&object_path);
            return false;
        }
        let Some(sources) = self.drive_sources.clone() else {
            return false;
        };
        let name = drive.name(c);
        let busy = drive.key.clone();
        self.spawn_drive_action(&busy, "files-drive-mount", move || {
            match (sources.mount)(&drive) {
                Ok(path) => DriveOutcome::Mounted(path),
                Err(detail) => DriveOutcome::MountFailed {
                    drive: name,
                    detail,
                },
            }
        });
        false
    }

    /// Unlocks an encrypted drive with the passphrase typed into its dialog
    /// and goes there once what is inside is mounted. The passphrase goes to
    /// UDisks2 and is overwritten when the call returns.
    pub fn unlock_drive(&mut self, key: &str, passphrase: crate::secret::Secret) {
        if self.is_drive_busy(key) {
            return;
        }
        let Some(drive) = self.drives.iter().find(|drive| drive.key == key).cloned() else {
            return;
        };
        let Some(sources) = self.drive_sources.clone() else {
            return;
        };
        let name = drive.name(crate::i18n::copy(self.locale));
        let busy = drive.key.clone();
        self.spawn_drive_action(&busy, "files-drive-unlock", move || {
            let result = (sources.unlock)(&drive, &passphrase);
            drop(passphrase);
            DriveOutcome::Unlocked {
                drive: name,
                result,
            }
        });
    }

    /// Ejects a drive: through the storage service when it reports the
    /// device (which says what happened), through UDisks2 otherwise. Every
    /// window's watches under it go first. The system drive is never
    /// ejected, and `/` is never let go of.
    pub fn eject_drive(&mut self, key: &str) {
        if self.is_drive_busy(key) {
            return;
        }
        let Some(drive) = self.drives.iter().find(|drive| drive.key == key).cloned() else {
            return;
        };
        let Some(object_path) = drive.object_path.clone() else {
            return;
        };
        let system = crate::drives::system_use_of(Some(&drive), self.devices.get(&object_path));
        if let Some(reason) = system.refusal_detail() {
            self.notice = Some(Notice::Key(reason));
            return;
        }
        if !drive.ejectable
            || drive.kind == crate::drives::DriveKind::System
            || drive.mount_point.as_deref() == Some(std::path::Path::new("/"))
        {
            return;
        }
        if self.devices.get(&object_path).is_some() {
            self.eject_device(&object_path);
            return;
        }
        let Some(sources) = self.drive_sources.clone() else {
            return;
        };
        let folder = drive.mount_point.clone();
        if let Some(folder) = &folder {
            self.hub.suspend_under(folder);
        }
        let name = drive.name(crate::i18n::copy(self.locale));
        let busy = drive.key.clone();
        self.spawn_drive_action(&busy, "files-drive-eject", move || DriveOutcome::Ejected {
            drive: name,
            folder,
            result: (sources.eject)(&drive),
        });
    }

    /// Runs a mount, unlock or eject on a thread of its own; its outcome
    /// comes back through [`Self::pump`]. The drive is busy until then.
    fn spawn_drive_action(
        &mut self,
        key: &str,
        name: &str,
        action: impl FnOnce() -> DriveOutcome + Send + 'static,
    ) {
        let outcomes = self.drive_outcomes.0.clone();
        let key = key.to_string();
        self.busy_drives.push(key.clone());
        let spawned = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let _ = outcomes.send((key, action()));
            });
        if spawned.is_err() {
            self.busy_drives.pop();
        }
    }

    /// Whether the window is on screen; the drives are read on a timer only
    /// while it is.
    pub fn set_window_visible(&mut self, visible: bool) {
        self.window_visible = visible;
    }

    // --- Removal policy (ticket 52) ----------------------------------------

    /// A policy was chosen for a device from its sidebar row.
    ///
    /// Performance mode opens the confirmation and sends nothing yet. Direct
    /// Removal is sent at once: it gives nothing up, so there is nothing to
    /// confirm. Choosing the policy the device already reports does nothing.
    pub fn choose_policy(&mut self, object_path: &str, policy: RemovalPolicy) {
        let Some(row) = self.devices.get(object_path) else {
            return;
        };
        if row.policy == policy {
            return;
        }
        match policy {
            RemovalPolicy::Performance => {
                self.policy_confirmation = Some(PerformanceConfirmation::new(
                    object_path,
                    self.device_label(object_path),
                ));
            }
            RemovalPolicy::DirectRemoval => {
                self.policy_confirmation = None;
                self.link
                    .request_policy(PolicyRequest::direct_removal(object_path));
            }
        }
    }

    pub fn toggle_policy_risk(&mut self, key: &str) {
        if let Some(confirmation) = self.policy_confirmation.as_mut() {
            confirmation.toggle(key);
        }
    }

    /// Sends the confirmed request, if every risk has been ticked. Until then
    /// it does nothing, and the confirmation stays open.
    pub fn confirm_policy(&mut self) {
        let Some(request) = self
            .policy_confirmation
            .as_ref()
            .and_then(PerformanceConfirmation::request)
        else {
            return;
        };
        self.policy_confirmation = None;
        self.link.request_policy(request);
    }

    /// Closes the confirmation and sends nothing.
    pub fn cancel_policy(&mut self) {
        self.policy_confirmation = None;
    }

    /// What a device is called in a toast or a dialog: what its sidebar row
    /// says. A device the drive listing also has is drawn as that drive and
    /// named by `Drive::naming` (the running system's drive on an external
    /// SSD is "Zorin OS", ticket 138); any other by the service's own label.
    pub(crate) fn device_label(&self, object_path: &str) -> String {
        use crate::drives::DriveText as _;
        let Some(row) = self.devices.get(object_path) else {
            return object_path.to_string();
        };
        match self
            .drives
            .iter()
            .find(|drive| crate::drives::is_device(drive, row))
        {
            Some(drive) => drive.name(crate::i18n::copy(self.locale)),
            None => row.label.clone(),
        }
    }

    /// A drive by its vendor and model, for an image write.
    fn drive_name(&self, object_path: &str) -> String {
        self.devices
            .get(object_path)
            .and_then(|row| row.drive_name.clone())
            .unwrap_or_else(|| self.device_label(object_path))
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
        // A server folder is pinned by its address (ticket 310).
        let location = &self.stored_form(location);
        let outcome = self.edit_bookmarks(|file| {
            let outcome = file.pin(location);
            (outcome == PinOutcome::Pinned, outcome)
        });
        match outcome {
            // A successful pin shows itself: the row appears in Favorites.
            // A notice on top of that would be noise.
            PinOutcome::Pinned => self.notice = None,
            PinOutcome::AlreadyPinned => self.notice = Some(Notice::AlreadyPinned),
            PinOutcome::NotPinnable => self.notice = Some(Notice::NotPinnable),
        }
    }

    pub fn pin_current(&mut self) {
        let location = self.location().clone();
        self.pin(&location);
    }

    /// Edits the shared bookmarks file as it is now, not as this window last
    /// read it: the file is read again, the edit applied (by location, since
    /// the rows may have moved), and written. A favourite added meanwhile in
    /// Nautilus, another window or the other half of a split is kept.
    /// `edit` says whether it changed anything.
    fn edit_bookmarks<R>(&mut self, edit: impl FnOnce(&mut BookmarkFile) -> (bool, R)) -> R {
        let mut file = load_bookmarks(
            &self.bookmark_store,
            self.remote.gvfs_root.as_deref(),
            false,
        );
        let (changed, result) = edit(&mut file);
        if changed && let Err(error) = self.bookmark_store.save(&file) {
            self.notice = Some(Notice::Key(format!(
                "files.bookmarks.error.unwritable:{error}"
            )));
        }
        self.bookmarks = file;
        result
    }

    /// The location of the favourite this window shows at `index`.
    fn bookmark_location(&self, index: usize) -> Option<Location> {
        self.bookmarks
            .get(index)
            .map(|bookmark| bookmark.location().clone())
    }

    pub fn remove_bookmark(&mut self, index: usize) {
        let Some(location) = self.bookmark_location(index) else {
            return;
        };
        let removed = self.edit_bookmarks(|file| {
            let removed = file
                .position_of(&location)
                .and_then(|at| file.remove(at))
                .is_some();
            (removed, removed)
        });
        if removed
            && let Some(cursor) = self.sidebar_cursor
            && cursor >= self.bookmarks.len()
        {
            self.sidebar_cursor = self.bookmarks.len().checked_sub(1);
        }
    }

    pub fn move_bookmark_up(&mut self, index: usize) {
        if index > 0 {
            self.move_bookmark_to(index, index - 1);
        }
    }

    pub fn move_bookmark_down(&mut self, index: usize) {
        self.move_bookmark_to(index, index + 1);
    }

    /// Pins a folder dropped on the Favourites at `position` (ticket 121),
    /// rather than at the end; a folder already pinned moves there instead.
    pub fn pin_at(&mut self, location: &Location, position: usize) {
        let location = &self.stored_form(location);
        if !matches!(location, Location::Local(_) | Location::Network(_)) {
            self.notice = Some(Notice::NotPinnable);
            return;
        }
        let target = self.bookmark_location(position);
        self.edit_bookmarks(|file| {
            let to = target
                .as_ref()
                .and_then(|target| file.position_of(target))
                .unwrap_or(file.len());
            match file.position_of(location) {
                Some(from) => {
                    let to = to.min(file.len().saturating_sub(1));
                    (file.move_to(from, to), ())
                }
                None => {
                    file.pin(location);
                    let from = file.len() - 1;
                    file.move_to(from, to.min(from));
                    (true, ())
                }
            }
        });
        self.notice = None;
    }

    /// A drop between two favourites: the one at `from` lands where the one
    /// at `to` is.
    pub fn move_bookmark_to(&mut self, from: usize, to: usize) {
        let (Some(moved), Some(target)) =
            (self.bookmark_location(from), self.bookmark_location(to))
        else {
            return;
        };
        let landed = self.edit_bookmarks(|file| {
            match (file.position_of(&moved), file.position_of(&target)) {
                (Some(from), Some(to)) => (file.move_to(from, to), Some(to)),
                _ => (false, None),
            }
        });
        if let Some(to) = landed {
            self.sidebar_cursor = Some(to);
        }
    }

    pub fn set_bookmark_label(&mut self, index: usize, label: &str) {
        let Some(location) = self.bookmark_location(index) else {
            return;
        };
        self.edit_bookmarks(|file| {
            let changed = file
                .position_of(&location)
                .is_some_and(|at| file.set_label(at, label));
            (changed, ())
        });
    }

    /// Reads the bookmarks file again when it changed on disk (another
    /// program, window or pane), so the sidebar follows it.
    fn pump_bookmarks(&mut self) -> bool {
        let Some(subscription) = &self.bookmark_watch else {
            return false;
        };
        let name = self
            .bookmark_store
            .path()
            .file_name()
            .map(|name| name.to_os_string());
        let touched = subscription.poll().into_iter().any(|update| match update {
            files_platform::HubUpdate::Rows(rows) => rows.iter().any(|row| match row {
                files_core::RefreshEvent::Removed(files_core::EntryId::Name(changed)) => {
                    name.as_deref() == Some(std::ffi::OsStr::new(changed))
                }
                files_core::RefreshEvent::Added(entry)
                | files_core::RefreshEvent::Modified(entry) => {
                    name.as_deref() == Some(std::ffi::OsStr::new(&entry.name))
                }
                _ => false,
            }),
            // What changed before the watch was in place is unknown.
            files_platform::HubUpdate::Resynchronize | files_platform::HubUpdate::Ready { .. } => {
                true
            }
            _ => false,
        });
        if !touched {
            return false;
        }
        let fresh = load_bookmarks(
            &self.bookmark_store,
            self.remote.gvfs_root.as_deref(),
            false,
        );
        if fresh == self.bookmarks {
            return false;
        }
        self.bookmarks = fresh;
        if let Some(cursor) = self.sidebar_cursor
            && cursor >= self.bookmarks.len()
        {
            self.sidebar_cursor = self.bookmarks.len().checked_sub(1);
        }
        true
    }

    // --- Operations -------------------------------------------------------

    pub fn engine(&self) -> &Arc<JobEngine> {
        &self.engine
    }

    /// Submits a spec, keeping only the failure. The handle is dropped on
    /// purpose: the engine owns the job, and holding a receipt would be the
    /// beginning of tying the job to this window.
    pub fn submit(&mut self, spec: JobSpec) {
        let operation = spec.operation.clone();
        match self.engine.submit(spec) {
            Ok(handle) => {
                self.notice = None;
                self.operations_open = true;
                self.undoable.push((handle.id(), operation));
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
        // Into a server folder that is not connected: the paste waits for
        // the connection (ticket 310).
        if self.hold_paste() {
            self.notice = None;
            return;
        }
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
        let location = self.location().clone();
        let targets = commands::delete_targets(&location, &self.selected_entries());
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

    /// Which of Compress and Extract the current selection offers.
    ///
    /// The toolbar asks every frame, so this looks up the selected entries by
    /// identity rather than walking the whole listing the way
    /// [`Self::selected_entries`] does to keep them in visible order.
    pub fn archive_actions(&self) -> ArchiveActions {
        let model = self.pane().model();
        let selected: Vec<&Entry> = model
            .selection()
            .ids()
            .filter_map(|id| model.get(id))
            .collect();
        commands::archive_actions(self.location(), &selected)
    }

    /// Opens the format chooser for the selection. Nothing is written until a
    /// format is picked.
    pub fn request_compress(&mut self) {
        let location = self.location().clone();
        match commands::compress_request(&location, &self.selected_entries()) {
            Ok(request) => {
                self.notice = None;
                self.dialog = Some(PendingDialog::Compress(CompressChooser::new(request)));
            }
            Err(refusal) => self.notice = Some(Notice::Command(refusal)),
        }
    }

    /// Answers the format chooser.
    pub fn compress(&mut self, format: ArchiveFormat) {
        let Some(PendingDialog::Compress(chooser)) = self.dialog.take() else {
            return;
        };
        self.submit_or_notice(chooser.request.spec(format));
    }

    /// One keystroke in the format chooser. Does nothing when it is not open.
    pub fn chooser_key(&mut self, key: ChooserKey) {
        let Some(PendingDialog::Compress(chooser)) = &mut self.dialog else {
            return;
        };
        match key {
            ChooserKey::Previous => chooser.previous(),
            ChooserKey::Next => chooser.next(),
            ChooserKey::Confirm => {
                let format = chooser.focused();
                self.compress(format);
            }
            ChooserKey::Cancel => self.dialog = None,
        }
    }

    pub fn extract_selection(&mut self) {
        let location = self.location().clone();
        let built = commands::extract(&location, &self.selected_entries());
        self.submit_or_notice(built);
    }

    pub fn restore_selection_from_trash(&mut self) {
        let location = self.location().clone();
        let items = commands::selected_trash_items(&self.selected_entries());
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
        if self.engine.snapshot(id).is_none() {
            // Let go of after more than `FINISHED_KEPT` other jobs finished.
            self.notice = Some(Notice::RetryGone);
            return;
        }
        if self.engine.retry_failed(id) {
            // Running again: it leaves the finished list and is filed anew
            // when it ends.
            self.finished.dismiss(id);
            self.recorded.remove(&id);
        }
        self.refresh_jobs();
    }

    /// Starts an interrupted copy or move again.
    pub fn resume_interrupted(&mut self, id: u64) {
        if let Some(Err(error)) = self.interrupted.resume(id, &self.engine) {
            self.notice = Some(Notice::Key(error.to_string()));
        }
        self.refresh_jobs();
    }

    /// Forgets an interrupted copy or move.
    pub fn dismiss_interrupted(&mut self, id: u64) {
        self.interrupted.dismiss(id);
    }

    pub fn resolve_conflict(&mut self, id: JobId, decision: ConflictDecision) {
        self.engine.resolve(id, decision);
        self.refresh_jobs();
    }

    /// Re-reads every job from the engine and files the finished ones into the
    /// session history.
    ///
    /// A job is filed once, the first frame it is seen finished, and that is
    /// when every pane is brought up to date ([`Self::refresh_panes`]), so the
    /// result of the person's own action shows at once rather than when a
    /// watcher gets to it. The engine lets go of old finished jobs itself
    /// ([`files_operations::FINISHED_KEPT`]), so this does work for the jobs
    /// it still lists, never for every job of the session.
    pub fn refresh_jobs(&mut self) -> bool {
        let jobs = self.engine.jobs();
        let mut changed = jobs != self.jobs;
        let c = crate::i18n::copy(self.locale);
        let mut finished_now = false;
        for snapshot in &jobs {
            if snapshot.state.is_terminal() && self.recorded.insert(snapshot.id) {
                finished_now = true;
                self.engine.mark_recorded(snapshot.id);
                self.finished.record(opcenter::job_row(snapshot, c));
                if let Some(at) = self.undoable.iter().position(|(id, _)| *id == snapshot.id) {
                    let (_, operation) = self.undoable.remove(at);
                    if let Some(change) = crate::undo::change_from_job(&operation, snapshot) {
                        self.history.record(change);
                    }
                }
            }
        }
        if self.recorded.len() > jobs.len() {
            let listed: std::collections::HashSet<JobId> = jobs.iter().map(|job| job.id).collect();
            self.recorded.retain(|id| listed.contains(id));
        }
        self.jobs = jobs;
        if finished_now {
            self.refresh_panes();
            // A copy or a delete changes how full a drive is.
            self.refresh_drives();
            changed = true;
        }
        changed
    }

    /// Dismisses one finished job from the panel. A failed one is let go of
    /// by the engine too, which ends its Retry.
    pub fn dismiss_finished(&mut self, id: JobId) {
        if self.finished.dismiss(id) {
            self.engine.forget(id);
            self.jobs.retain(|job| job.id != id);
        }
    }

    /// The panel's Clear: every finished job leaves the list.
    pub fn clear_finished(&mut self) {
        for id in self.finished.clear() {
            self.engine.forget(id);
            self.jobs.retain(|job| job.id != id);
        }
    }

    /// Brings every open pane up to date in place, keeping selection and
    /// scroll (ticket 119). Used when a file operation or an undo finishes:
    /// which folders it touched is not worth working out when a reading of
    /// an unchanged folder changes nothing on screen.
    pub fn refresh_panes(&mut self) {
        let reader = self.reader.clone();
        for (_, pane) in &mut self.panes {
            if matches!(pane.location(), Location::Applications) {
                continue;
            }
            pane.refresh(reader.as_ref());
        }
    }

    /// Follows every pane's folder through the process's one watch
    /// ([`files_platform::WatchHub`]): a watch is asked for as soon as a pane
    /// shows a folder, rows the hub read off this thread are applied, and a
    /// folder whose changes are not announced is read again now and then.
    /// Returns whether the window has to redraw.
    ///
    /// Rows are applied only to a list that is settled. While the folder is
    /// being listed or re-read, a change asks for one more reading instead:
    /// applied now, it could be undone by a reading taken before it, or a
    /// removal could come before the batch that carries the file.
    pub(crate) fn pump_watches(&mut self) -> bool {
        let reader = self.reader.clone();
        let hub = Arc::clone(&self.hub);
        let active = self.active_tab();
        let panes = &self.panes;
        self.watches
            .retain(|watch| panes.iter().any(|(tab, _)| *tab == watch.tab));
        let mut changed = false;
        let mut gone: Vec<(TabId, Location, Option<std::path::PathBuf>)> = Vec::new();
        for (tab, pane) in &mut self.panes {
            let index = match self.watches.iter().position(|watch| watch.tab == *tab) {
                Some(index) if self.watches[index].folder == *pane.location() => index,
                Some(index) => {
                    // The old folder's subscription goes before the new one
                    // is asked for.
                    self.watches[index] = FolderWatch::new(*tab, pane.location(), &reader, &hub);
                    index
                }
                None => {
                    self.watches
                        .push(FolderWatch::new(*tab, pane.location(), &reader, &hub));
                    self.watches.len() - 1
                }
            };
            let watch = &mut self.watches[index];
            let updates = watch
                .subscription
                .as_ref()
                .map(files_platform::Subscription::poll)
                .unwrap_or_default();
            for update in updates {
                match update {
                    files_platform::HubUpdate::Ready { polling } => {
                        watch.polling = polling;
                        watch.ready = true;
                        // What changed between the listing and the watch is
                        // unknown; one more reading closes that gap.
                        pane.refresh(reader.as_ref());
                    }
                    files_platform::HubUpdate::Rows(rows) => {
                        if let Some(folder) = pane.location().as_local_path() {
                            let folder = folder.as_path();
                            let paths: Vec<std::path::PathBuf> = rows
                                .iter()
                                .filter_map(|row| match row {
                                    files_core::RefreshEvent::Added(entry)
                                    | files_core::RefreshEvent::Modified(entry) => entry
                                        .as_local_path()
                                        .map(|path| path.as_path().to_path_buf()),
                                    files_core::RefreshEvent::Removed(
                                        files_core::EntryId::Name(name),
                                    ) => Some(folder.join(name)),
                                    _ => None,
                                })
                                .collect();
                            if !paths.is_empty() {
                                self.folder_sizes.invalidate(folder, &paths);
                            }
                        }
                        if pane.is_listing() || pane.is_refreshing() {
                            pane.refresh(reader.as_ref());
                        } else {
                            changed |= apply_rows(pane, rows);
                        }
                    }
                    files_platform::HubUpdate::Resynchronize => {
                        if let Some(folder) = pane.location().as_local_path() {
                            self.folder_sizes.invalidate(folder.as_path(), &[]);
                        }
                        pane.refresh(reader.as_ref())
                    }
                    files_platform::HubUpdate::Gone { nearest } => {
                        gone.push((*tab, watch.folder.clone(), nearest))
                    }
                    files_platform::HubUpdate::Suspended => {}
                }
            }
            let every = if *tab == active {
                UNANNOUNCED_REFRESH
            } else {
                BACKGROUND_REFRESH
            };
            if watch.polling
                && watch.last_read.elapsed() >= every
                && !pane.is_listing()
                && !pane.is_refreshing()
            {
                watch.last_read = std::time::Instant::now();
                pane.refresh(reader.as_ref());
            }
        }
        for (tab, folder, nearest) in gone {
            // A folder the tab has already left (a server folder that was
            // not there, found again above) is not this tab's news.
            let still_there = self
                .panes
                .iter()
                .any(|(id, pane)| *id == tab && *pane.location() == folder);
            if still_there {
                changed |= self.folder_gone(tab, nearest);
            }
        }
        changed
    }

    /// The folder a pane shows was deleted or moved away: the pane goes to
    /// the nearest folder above it that is still there, and says why.
    fn folder_gone(&mut self, tab: TabId, nearest: Option<std::path::PathBuf>) -> bool {
        // On a server, "gone" is most often the connection (ticket 310): the
        // tab connects again in place, and goes to the folder above only if
        // the server, once there, says the folder is gone.
        if self.remote_folder_gone(tab) {
            self.watches.retain(|watch| watch.tab != tab);
            return true;
        }
        let reader = self.reader.clone();
        let home = self.home_location();
        let Some((_, pane)) = self.panes.iter_mut().find(|(id, _)| *id == tab) else {
            return false;
        };
        let name = pane
            .location()
            .as_local_path()
            .and_then(|path| path.as_path().file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = nearest
            .and_then(|path| Location::local(path).ok())
            .unwrap_or(home);
        pane.navigate_to(target, reader.as_ref());
        // The old subscription is spent; the folder above gets a new one,
        // and a folder made again at the same path will too.
        self.watches.retain(|watch| watch.tab != tab);
        self.sync_history(tab);
        if tab == self.active_tab() {
            self.notice = Some(Notice::FolderGone(name));
            self.resync_content();
        }
        true
    }

    /// Reads a polled folder again when its tab comes forward.
    fn refresh_if_polled(&mut self, id: TabId) {
        let polled = self
            .watches
            .iter_mut()
            .find(|watch| watch.tab == id && watch.polling);
        let Some(watch) = polled else {
            return;
        };
        watch.last_read = std::time::Instant::now();
        let reader = self.reader.clone();
        if let Some((_, pane)) = self.panes.iter_mut().find(|(tab, _)| *tab == id) {
            pane.refresh(reader.as_ref());
        }
    }

    /// The watch hub this window's folders are followed through, for tests.
    #[cfg(test)]
    pub(crate) fn watch_hub(&self) -> &files_platform::WatchHub {
        &self.hub
    }

    /// The folders the hub holds a kernel watch on for this window's panes,
    /// for tests.
    #[cfg(test)]
    pub(crate) fn watched_folders(&self) -> Vec<std::path::PathBuf> {
        self.watches
            .iter()
            .filter(|watch| watch.ready)
            .filter_map(|watch| watch.subscription.as_ref())
            .map(|subscription| subscription.directory().to_path_buf())
            .filter(|folder| self.hub.is_watching(folder))
            .collect()
    }

    /// The panes, for tests that look past the active one.
    #[cfg(test)]
    pub(crate) fn panes_for_tests(&self) -> impl Iterator<Item = &Pane> {
        self.panes.iter().map(|(_, pane)| pane)
    }

    /// Whether any pane is reading its folder again.
    pub fn is_refreshing(&self) -> bool {
        self.panes.iter().any(|(_, pane)| pane.is_refreshing())
    }

    /// The running jobs as rows.
    pub fn job_rows(&self) -> Vec<JobRow> {
        let c = crate::i18n::copy(self.locale);
        opcenter::job_rows(&self.jobs, c)
    }

    // --- Frame ------------------------------------------------------------

    /// Lists the Applications view again when sizes it was waiting for are
    /// in. The refresh keeps selection and scroll, and its rows carry the
    /// sizes, so a size sort settles on them.
    fn pump_application_sizes(&mut self) {
        let generation = self.reader.application_sizes_generation();
        if generation == self.application_sizes_seen {
            return;
        }
        self.application_sizes_seen = generation;
        let reader = self.reader.clone();
        for (_, pane) in &mut self.panes {
            if matches!(pane.location(), Location::Applications) {
                pane.refresh(reader.as_ref());
            }
        }
    }

    /// Drains whatever arrived and returns whether the window has to redraw.
    ///
    /// The active pane is pumped every frame; background tabs are pumped too,
    /// because a tab opened in the background is loading and must be ready
    /// when it is switched to rather than starting over.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        let reader = self.reader.clone();
        for (_, pane) in &mut self.panes {
            changed |= pane.pump_with(reader.as_ref());
        }
        changed |= self.pump_watches();
        changed |= self.pump_folder_sizes(changed);
        self.pump_application_sizes();
        if changed {
            // Entries arriving move indices around. The cursor is re-derived
            // from the selection's identity, which is what keeps the focused
            // entry focused while a large directory streams in.
            self.resync_content();
        }
        changed |= self.refresh_jobs();
        changed |= self.pump_devices();
        changed |= self.pump_bookmarks();
        changed |= self.pump_preferences();
        changed |= self.pump_drives();
        changed |= self.pump_remote();
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

    /// Folder sizes for the active pane: started once its listing is in,
    /// cancelled when it leaves the folder.
    fn pump_folder_sizes(&mut self, scan: bool) -> bool {
        if !self.preferences.folder_sizes {
            self.folder_sizes.stop();
            return false;
        }
        let network = self.preferences.folder_sizes_on_network;
        let id = self.active_tab();
        let Some((_, pane)) = self.panes.iter_mut().find(|(tab, _)| *tab == id) else {
            return false;
        };
        let listed = !pane.is_listing();
        self.folder_sizes
            .pump(pane.model_mut(), listed, network, scan)
    }

    /// Turns folder sizes on or off. Off takes the sizes back out of every
    /// pane's model, so sorting by size orders folders as it did before.
    pub fn toggle_folder_sizes(&mut self) {
        self.preferences.folder_sizes = !self.preferences.folder_sizes;
        if !self.preferences.folder_sizes {
            self.folder_sizes.stop();
            for (_, pane) in &mut self.panes {
                crate::folder_sizes::FolderSizes::clear_model(pane.model_mut());
            }
        }
        self.resync_content();
        self.persist_preferences();
    }

    /// Turns measuring network and FUSE folders on or off. The visit starts
    /// over, so the change is seen at once.
    pub fn toggle_folder_sizes_on_network(&mut self) {
        self.preferences.folder_sizes_on_network = !self.preferences.folder_sizes_on_network;
        self.folder_sizes.stop();
        self.persist_preferences();
    }

    /// What the cell of `entry`'s size says.
    pub fn size_label(&self, entry: &Entry) -> crate::folder_sizes::SizeLabel {
        // An application's size is its own record, whatever the folder-size
        // preference (ticket 128).
        if let files_core::EntryBody::Application(facts) = &entry.body {
            let c = crate::i18n::copy(self.locale);
            return crate::folder_sizes::SizeLabel {
                text: crate::app_sizes::size_text(&facts.size, c),
                tooltip: crate::app_sizes::size_note(&facts.size, c),
                is_value: facts.size.bytes().is_some(),
            };
        }
        self.folder_sizes.label(
            entry,
            self.preferences.folder_sizes,
            crate::i18n::copy(self.locale),
        )
    }

    /// The selection's total for the status bar.
    pub fn selection_size(&self) -> Option<String> {
        let selected = self.selected_entries();
        if selected.is_empty() {
            return None;
        }
        crate::folder_sizes::selection_total(
            &selected,
            &self.folder_sizes,
            self.preferences.folder_sizes,
            crate::i18n::copy(self.locale),
        )
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
            Command::ShowGrid => self.set_view_mode(ViewMode::Grid),
            Command::ShowList => self.set_view_mode(ViewMode::List),
            Command::ShowColumns => self.set_view_mode(ViewMode::Columns),
            Command::NormalSize => self.set_scale(ItemScale::default()),
            Command::GoHome => {
                let home = self.home_location();
                self.navigate_to(home);
            }
            Command::TogglePin => self.toggle_pin(),
            // The window owns the system clipboard, the menu's position and
            // the shortcut sheet; it reads what to do from the session.
            Command::CopyPath | Command::ContextMenu | Command::ShowShortcuts => {}
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
                // Escape steps back one thing at a time: a dialog or notice,
                // then the selection, then (as in folio) the work under way.
                let had_something = self.dialog.is_some()
                    || self.notice.is_some()
                    || !self.pane().model().selection().is_empty();
                self.dialog = None;
                self.notice = None;
                self.apply_selection(SelectionInput::Clear, columns);
                if !had_something {
                    self.cancel_latest_job();
                }
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
            Command::Undo => self.undo(),
            Command::PreviewWindow => self.open_preview_window(),
            Command::CyclePane => {
                self.focus = match self.focus {
                    Focus::Content => {
                        if self.sidebar_cursor.is_none() && !self.bookmarks.is_empty() {
                            self.sidebar_cursor = Some(0);
                        }
                        Focus::Sidebar
                    }
                    Focus::Sidebar => Focus::Content,
                };
            }
            // The window owns the filter field and opens windows.
            Command::FilterFolder | Command::NewWindow | Command::SplitView => {}
            Command::Redo => self.redo(),
            Command::ToggleOperations => self.operations_open = !self.operations_open,
            Command::Compress => self.request_compress(),
            Command::Extract => self.extract_selection(),
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

    /// The text `Ctrl+Shift+C` puts on the clipboard: the selected entries'
    /// paths, one per line, or the folder's own path when nothing is
    /// selected. `None` for a place with no path (Trash, Applications).
    pub fn path_text(&self) -> Option<String> {
        let selected: Vec<String> = self
            .selected_entries()
            .into_iter()
            .filter_map(Entry::as_local_path)
            .map(|path| path.as_path().to_string_lossy().into_owned())
            .collect();
        if !selected.is_empty() {
            return Some(selected.join("\n"));
        }
        local_folder(self.location()).map(|path| path.to_string_lossy().into_owned())
    }

    /// `Ctrl+P`: pins the selected folder, or the folder in front of the
    /// person, and unpins it when it is already in the sidebar.
    pub fn toggle_pin(&mut self) {
        let target = self
            .focused_entry()
            .filter(|entry| entry.kind == files_core::EntryKind::Directory)
            .and_then(Entry::as_local_path)
            .cloned()
            .map(Location::Local)
            .unwrap_or_else(|| self.location().clone());
        self.toggle_pin_of(&target);
    }

    /// Pins `location`, or unpins it when it is pinned already.
    pub fn toggle_pin_of(&mut self, location: &Location) {
        let location = &self.stored_form(location);
        let pinned = (0..self.bookmarks.len()).find(|index| {
            self.bookmarks
                .get(*index)
                .is_some_and(|bookmark| bookmark.location() == location)
        });
        match pinned {
            Some(index) => self.remove_bookmark(index),
            None => self.pin(location),
        }
    }

    /// What the content-area menus need to know, read from the selection
    /// and the folder.
    pub fn selection_facts(&self) -> crate::context_menu::SelectionFacts {
        let selected = self.selected_entries();
        let single_folder = selected.len() == 1
            && selected[0].kind == files_core::EntryKind::Directory
            && selected[0].as_local_path().is_some();
        let single_folder_pinned = single_folder
            && selected[0]
                .as_local_path()
                .is_some_and(|path| self.is_pinned(&Location::Local(path.clone())));
        let all_local =
            !selected.is_empty() && selected.iter().all(|entry| entry.as_local_path().is_some());
        crate::context_menu::SelectionFacts {
            count: selected.len(),
            single_folder,
            single_folder_pinned,
            all_local,
            archive: self.archive_actions(),
            writable: local_folder(self.location()).is_some_and(|path| {
                std::fs::metadata(&path).is_ok_and(|meta| !meta.permissions().readonly())
            }),
            in_trash: self.viewing_trash(),
            can_paste: !self.clipboard.is_empty(),
            showing_hidden: self.preferences.show_hidden,
            has_items: self.row_count() > 0,
        }
    }

    /// Carries out one context-menu line. The sidebar lines act on
    /// `sidebar_target`, the row the menu was opened on.
    pub fn run_menu_command(
        &mut self,
        command: crate::context_menu::MenuCommand,
        sidebar_target: Option<&crate::sidebar::SidebarRow>,
        columns: usize,
    ) {
        use crate::context_menu::MenuCommand as M;
        if let Some(row) = sidebar_target {
            match command {
                M::Open => self.navigate_to(row.location.clone()),
                M::OpenInNewTab => {
                    self.open_tab(row.location.clone(), true);
                }
                M::Pin => self.pin(&row.location),
                M::RenameBookmark => {
                    if let Some(index) = row.bookmark_index {
                        self.dialog = Some(PendingDialog::RenameBookmark(index));
                    }
                }
                M::MoveBookmarkUp => {
                    if let Some(index) = row.bookmark_index {
                        self.move_bookmark_up(index);
                    }
                }
                M::MoveBookmarkDown => {
                    if let Some(index) = row.bookmark_index {
                        self.move_bookmark_down(index);
                    }
                }
                M::MoveBookmarkToTop => {
                    if let Some(index) = row.bookmark_index {
                        self.move_bookmark_to(index, 0);
                    }
                }
                M::MoveBookmarkToBottom => {
                    if let Some(index) = row.bookmark_index {
                        let last = self.bookmarks.len().saturating_sub(1);
                        self.move_bookmark_to(index, last);
                    }
                }
                M::RemoveBookmark | M::Unpin => {
                    if let Some(index) = row.bookmark_index {
                        self.remove_bookmark(index);
                    }
                }
                _ => {}
            }
            return;
        }
        match command {
            M::Open => self.open_focused(),
            M::OpenInNewTab => {
                if let Some(id) = self.focused_entry().map(Entry::id) {
                    self.open_in_new_tab(&id);
                }
            }
            M::OpenWith => self.open_with_selected(),
            M::Preview => {
                if !self.preview.open {
                    self.toggle_preview();
                }
            }
            M::Pin | M::Unpin => self.toggle_pin(),
            M::Rename => {
                self.focus = Focus::Content;
                self.begin_rename();
            }
            M::Duplicate => self.duplicate_selection(),
            M::Copy => self.copy_selection(),
            M::Cut => self.cut_selection(),
            M::Paste => self.paste(),
            M::NewFolder => self.dialog = Some(PendingDialog::NewFolder),
            M::NewFile => self.dialog = Some(PendingDialog::NewFile),
            M::Compress => self.request_compress(),
            M::Extract => self.extract_selection(),
            M::ToggleHidden => self.toggle_hidden(),
            M::SelectAll => {
                self.apply_selection(SelectionInput::SelectAll, columns);
            }
            M::MoveToTrash => self.trash_selection(),
            M::DeletePermanently => self.request_permanent_delete(),
            M::Restore => self.restore_selection_from_trash(),
            M::EmptyTrash => self.empty_trash(columns),
            M::OpenTerminal => self.open_terminal_here(&crate::terminal::on_path),
            // The window puts the text on the clipboard.
            M::CopyPath => {}
            M::RenameBookmark
            | M::MoveBookmarkUp
            | M::MoveBookmarkDown
            | M::MoveBookmarkToTop
            | M::MoveBookmarkToBottom
            | M::RemoveBookmark
            | M::Eject
            | M::Mount
            | M::Format
            | M::MountAutomatically
            | M::RetryMount => {}
        }
    }

    /// Type-to-filter: shows only the names in this folder that contain
    /// `text`, ignoring case. It never reloads and never looks below this
    /// folder (that is what search is for), and it is cleared when the
    /// folder changes.
    pub fn set_filter(&mut self, text: &str) {
        self.pane_mut().model_mut().set_name_filter(text);
        self.resync_content();
    }

    pub fn filter(&self) -> &str {
        self.pane().model().name_filter()
    }

    /// "Showing 3 of 120", or `None` while nothing is filtered.
    pub fn filter_status(&self) -> Option<(usize, usize)> {
        let model = self.pane().model();
        (!model.name_filter().is_empty()).then(|| (model.visible_len(), model.total_len()))
    }

    /// Empty Trash: everything in the Trash, deleted for good, after the
    /// same confirmation a permanent delete asks for.
    pub fn empty_trash(&mut self, columns: usize) {
        if !self.viewing_trash() {
            self.navigate_to(Location::Trash(TrashLocation::Root));
            return;
        }
        self.apply_selection(SelectionInput::SelectAll, columns);
        self.request_permanent_delete();
    }

    /// Open Terminal Here: the person's terminal, started in this folder.
    /// `installed` says which programs are on the system.
    pub fn open_terminal_here(&mut self, installed: &dyn Fn(&str) -> bool) {
        let Some(folder) = local_folder(self.location()) else {
            self.notice = Some(Notice::Command(CommandRefusal::NotAFilesystemLocation));
            return;
        };
        match crate::terminal::terminal_invocation(&folder, installed) {
            Some(invocation) => {
                if self.spawner.spawn(&invocation).is_err() {
                    self.notice = Some(Notice::NoTerminal);
                }
            }
            None => self.notice = Some(Notice::NoTerminal),
        }
    }

    /// The folder a server this window mounted was mounted at, by the
    /// address it was asked for.
    pub fn server_folder(&self, address: &str) -> Option<&std::path::Path> {
        // Any address of the same mount: `sftp://nas`, `sftp://nas/`.
        let wanted = crate::remote::Remote::from_address(address)
            .map(|remote| remote.mount_address())
            .unwrap_or_else(|| address.to_string());
        self.server_folders
            .iter()
            .find(|(known, _)| *known == wanted)
            .map(|(_, folder)| folder.as_path())
    }

    /// The mount point of the network or FUSE filesystem `path` is on, or
    /// `None` for a local disk. Read from the kernel's mount table, once for
    /// each folder asked about.
    pub fn foreign_mount(&mut self, path: &std::path::Path) -> Option<std::path::PathBuf> {
        if let Some((known, foreign)) = &self.foreign_mount
            && known == path
        {
            return foreign.clone();
        }
        let table = files_platform::MountTable::from_env();
        let foreign = match table.reach(path) {
            files_platform::Reach::Local => None,
            files_platform::Reach::Network | files_platform::Reach::Fuse => {
                table.mount_for(path).map(|mount| mount.mount_point.clone())
            }
        };
        self.foreign_mount = Some((path.to_path_buf(), foreign.clone()));
        foreign
    }

    /// The name of the application an entry opens with, as the details pane
    /// shows it ("Opens with Text Editor"). `None` for a folder, or a type
    /// nothing handles.
    pub fn opens_with(&self, entry: &Entry) -> Option<String> {
        if entry.kind != files_core::EntryKind::File {
            return None;
        }
        let (route, _) = crate::openwith::route_open_file(
            entry.mime.as_ref(),
            self.defaults.as_ref(),
            &self.catalog,
        );
        match route {
            OpenRoute::LaunchWith { desktop_id, .. } => self
                .catalog
                .snapshot()
                .get(&desktop_id)
                .map(|app| app.display_name(None).to_string()),
            _ => None,
        }
    }

    /// Space: the selected file or folder in the large preview, which is a
    /// program of its own (`better-files-preview`, beside this one), so a
    /// file that upsets a decoder never takes the file manager down.
    pub fn open_preview_window(&mut self) {
        let Some(path) = self
            .focused_entry()
            .and_then(Entry::as_local_path)
            .map(|path| path.as_path().to_string_lossy().into_owned())
        else {
            self.notice = Some(Notice::Command(CommandRefusal::NothingToActOn));
            return;
        };
        let program = match self.preview_program.clone() {
            Some(program) => Ok(program),
            None => preview_program(),
        };
        let started = program.ok().is_some_and(|program| {
            let invocation = app_catalog_core::Invocation {
                program,
                arguments: vec![path],
            };
            self.spawner.spawn(&invocation).is_ok()
        });
        if !started {
            // No separate preview: the side pane shows what it can.
            if !self.preview.open {
                self.toggle_preview();
            }
        }
    }

    /// Cancels the newest job still running, if there is one. Returns
    /// whether one was.
    pub fn cancel_latest_job(&mut self) -> bool {
        let latest = self
            .jobs
            .iter()
            .filter(|job| !job.state.is_terminal())
            .map(|job| job.id)
            .max_by_key(|id| id.value());
        match latest {
            Some(id) => {
                self.engine.cancel(id);
                self.refresh_jobs();
                true
            }
            None => false,
        }
    }

    /// Starts `program` for the large preview instead of the one the
    /// companion rule finds. For tests, whose executable has no preview
    /// program beside it.
    pub fn set_preview_program(&mut self, program: impl Into<String>) {
        self.preview_program = Some(program.into());
    }

    /// Uses `root` as the home trash for what undo puts away, instead of the
    /// session's own. For tests.
    pub fn set_undo_trash(&mut self, root: std::path::PathBuf) {
        self.history = crate::undo::History::with_trash(Some(root));
    }

    /// `Ctrl+Z`: undoes the latest change and says what it did, or why not.
    pub fn undo(&mut self) {
        self.notice = Some(Notice::Undo {
            undoing: true,
            outcome: self.history.undo(),
        });
        self.refresh_panes();
    }

    /// `Ctrl+Shift+Z` or `Ctrl+Y`.
    pub fn redo(&mut self) {
        self.notice = Some(Notice::Undo {
            undoing: false,
            outcome: self.history.redo(),
        });
        self.refresh_panes();
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

/// Reads the bookmarks file, with a favourite an earlier version stored as a
/// folder under GVfs's folder read as its server's address (ticket 310).
/// With `write`, the migrated file is written: once a process (the first
/// window's start), only if the file still holds what was read, and with the
/// mode it had. Nothing else rewrites the shared file without the person
/// doing something to it.
fn load_bookmarks(
    store: &BookmarkStore,
    gvfs_root: Option<&std::path::Path>,
    write: bool,
) -> BookmarkFile {
    // Once a process for each bookmarks file.
    static WRITTEN: std::sync::Mutex<std::collections::BTreeSet<std::path::PathBuf>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let text = store.text();
    let mut file = text.as_deref().map(BookmarkFile::parse).unwrap_or_default();
    if let Some(root) = gvfs_root
        && crate::remote::migrate_bookmarks(&mut file, root)
        && write
        && WRITTEN
            .lock()
            .map(|mut written| written.insert(store.path().to_path_buf()))
            .unwrap_or(false)
        && let Some(text) = &text
    {
        let _ = store.save_if_unchanged(&file, text);
    }
    file
}

/// The folder behind a location, when it is one on this machine.
pub(crate) fn local_folder(location: &Location) -> Option<std::path::PathBuf> {
    match location {
        Location::Local(path) => Some(path.as_path().to_path_buf()),
        _ => None,
    }
}

/// Copies a stored folder view into the preferences mirror.
fn mirror(preferences: &mut FilesPreferences, view: FolderView) {
    preferences.view_mode = view.mode;
    preferences.scale = view.scale;
    preferences.show_hidden = view.show_hidden;
    preferences.set_order(view.order());
}

/// The preview program by the companion rule (`better_core::companion`,
/// ticket 179): `better-files-preview` beside this executable, then where the
/// package installs it for an installed Better Files, and never a name looked
/// up through `$PATH`, which would give a build the installed preview.
pub fn preview_program() -> Result<String, better_core::companion::Refusal> {
    better_core::companion::locate(&better_core::companion::FILES_PREVIEW)
        .map(|program| program.to_string_lossy().into_owned())
}

/// How often a visible folder whose changes are not announced (a network or
/// FUSE mount, the Trash, Recents, or one no watch could be made for) is read
/// again. Five seconds is soon enough that a file someone else added shows
/// while the person still looks.
/// The refusal said when the device a Format or Write to Drive dialog was
/// opened for is gone or is another device now (ticket 171).
pub const DEVICE_CHANGED: &str = "storage.erase.device_changed";

pub const UNANNOUNCED_REFRESH: std::time::Duration = std::time::Duration::from_secs(5);

/// The same, for a tab in the background: slower, and the tab is read again
/// the moment it is switched to.
pub const BACKGROUND_REFRESH: std::time::Duration = std::time::Duration::from_secs(30);

/// One pane's watch on the folder it shows (ticket 119).
struct FolderWatch {
    tab: TabId,
    /// The location the watch was made for; a pane elsewhere gets a new one.
    folder: Location,
    /// The hub's subscription, for a local folder.
    subscription: Option<files_platform::Subscription>,
    /// Whether the folder is read again now and then instead of trusted to
    /// a watch.
    polling: bool,
    /// The hub's `Ready` has been taken (and its reading asked for).
    ready: bool,
    last_read: std::time::Instant,
}

impl FolderWatch {
    /// Asks the hub for a watch; nothing here touches the disk.
    fn new(
        tab: TabId,
        location: &Location,
        reader: &FilesReader,
        hub: &files_platform::WatchHub,
    ) -> Self {
        let subscription = location
            .as_local_path()
            .map(|path| hub.subscribe(path.as_path(), reader.local_config().clone()));
        Self {
            tab,
            folder: location.clone(),
            polling: matches!(location, Location::Trash(_) | Location::Recent),
            subscription,
            ready: false,
            last_read: std::time::Instant::now(),
        }
    }
}

/// Applies rows the hub read to a settled list. Returns whether it changed.
fn apply_rows(pane: &mut Pane, rows: Vec<files_core::RefreshEvent>) -> bool {
    let mut removed = Vec::new();
    let mut entries = Vec::new();
    for row in rows {
        match row {
            files_core::RefreshEvent::Removed(id) => removed.push(id),
            files_core::RefreshEvent::Added(entry) | files_core::RefreshEvent::Modified(entry) => {
                entries.push(*entry)
            }
            files_core::RefreshEvent::Resynchronize => {}
        }
    }
    let model = pane.model_mut();
    let mut changed = model.remove_many(&removed) > 0;
    if !entries.is_empty() {
        model.insert_batch(entries);
        changed = true;
    }
    changed
}

/// A drive action's outcome with the key of the drive it was for.
type KeyedOutcome = (String, DriveOutcome);

/// What a mount, unlock or eject started from a drive row came to.
#[derive(Debug)]
enum DriveOutcome {
    Mounted(std::path::PathBuf),
    /// A drive's choice was changed, or why not.
    AutoMount {
        drive: String,
        outcome: drive_mounts::client::SetOutcome,
        /// Mounting or unmounting it now through UDisks2 afterwards failed.
        after: Option<String>,
    },
    MountFailed {
        drive: String,
        detail: String,
    },
    Unlocked {
        drive: String,
        result: Result<std::path::PathBuf, crate::drives::UnlockError>,
    },
    Ejected {
        drive: String,
        /// The folder whose watches to take back.
        folder: Option<std::path::PathBuf>,
        result: Result<crate::drives::Ejected, crate::drives::EjectError>,
    },
}

/// Why a drive's choice was not changed, in the window's words.
pub(crate) fn auto_mount_reason(
    outcome: &drive_mounts::client::SetOutcome,
    c: &'static Copy,
) -> String {
    use drive_mounts::client::SetOutcome;
    match outcome {
        SetOutcome::Done { .. } | SetOutcome::Cancelled => String::new(),
        SetOutcome::NotAllowed => c.auto_reason_not_allowed.to_string(),
        SetOutcome::Absent => c.auto_reason_absent.to_string(),
        SetOutcome::Refused(why) => match why.as_str() {
            "system_table" => c.auto_reason_system_table,
            "system_drive" => c.auto_reason_system_drive,
            _ => c.auto_reason_not_filesystem,
        }
        .to_string(),
        SetOutcome::Busy(_) => c.auto_reason_busy.to_string(),
        SetOutcome::Malformed(_) => c.auto_reason_malformed.to_string(),
        SetOutcome::TableBusy => c.auto_reason_table_busy.to_string(),
        SetOutcome::MountFailed(detail) => c
            .auto_reason_mount_failed
            .replace("{detail}", &crate::errors::explain(detail, c)),
        SetOutcome::Failed(detail) => crate::errors::explain(detail, c),
    }
}
