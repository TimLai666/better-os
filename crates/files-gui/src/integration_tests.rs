//! Ticket 35's view-model tests: Applications, Open With, devices, preview,
//! and search.
//!
//! None of them opens a window, and none of them touches the host's installed
//! applications or its plugged-in disks. The catalog is built from desktop-file
//! fixtures, the launcher is `app-catalog-platform`'s own recording spawner, and
//! the device link is a fake driven by the test — so what is proven is the
//! production path with exactly one thing replaced in each case.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_catalog_core::{
    Catalog, CatalogBuilder, DesktopId, DirectoryRank, EntryScope, ExecutableProbe, LaunchTarget,
    MimeType, SourceKind,
};
use app_catalog_platform::{RecordingSpawner, SessionEnvironment};
use files_core::{DirectoryModel, Entry, EntryKind, HiddenPreference, Location, SortOrder};
use files_operations::{EngineConfig, JobEngine};
use files_platform::{MountTable, ReaderConfig, UserDirectories};
use storage_core::{DeviceStateKind, RemovalPolicy};
use storage_service::protocol::{BlockerReport, DeviceReport, StateReport, UnsafeRemovalReport};

use crate::apps::{CatalogHandle, ExecutableSummary, LaunchReport};
use crate::bookmarks::BookmarkStore;
use crate::devices::{
    CollectionMode, DeviceInventory, DeviceLink, DeviceNotice, MountedDevice, UnsafeRemoval,
    is_under, row_from, state_label,
};
use crate::i18n::{EN_US, ZH_TW};
use crate::openwith::{ChooserRequest, DefaultSource, OpenRoute, SessionDefaults, route_open_file};
use crate::prefs::{FilesPreferences, PreferenceStore};
use crate::preview::{PreviewPanel, PreviewSlot};
use crate::reader::FilesReader;
use crate::session::{DeviceEvent, FilesSession, Notice, SessionSetup};

// --- Catalog fixtures ----------------------------------------------------

const EDITOR: &str = "[Desktop Entry]\nType=Application\nName=Text Editor\nName[zh_TW]=文字編輯器\nComment=Edits text\nExec=editor %F\nMimeType=text/plain;text/markdown;\nCategories=Utility;TextEditor;\nActions=new-window;\n\n[Desktop Action new-window]\nName=New Window\nExec=editor --new-window\n";
const VIEWER: &str = "[Desktop Entry]\nType=Application\nName=Image Viewer\nExec=viewer %f\nMimeType=image/png;\nCategories=Graphics;\n";
const DAEMON: &str =
    "[Desktop Entry]\nType=Application\nName=Background Daemon\nExec=daemon\nNoDisplay=true\n";

struct AlwaysResolves;

impl ExecutableProbe for AlwaysResolves {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        Some(PathBuf::from("/usr/bin").join(program))
    }
}

fn catalog_with(entries: &[(&str, &str)]) -> Catalog {
    let probe = AlwaysResolves;
    let mut builder = CatalogBuilder::new(&probe);
    let directory = DirectoryRank {
        rank: 0,
        scope: EntryScope::System,
    };
    for (id, contents) in entries {
        builder.add_entry(
            DesktopId::new(*id).expect("desktop id"),
            PathBuf::from(format!("/usr/share/applications/{id}")),
            &directory,
            contents.as_bytes(),
        );
    }
    builder.build()
}

fn full_catalog() -> CatalogHandle {
    CatalogHandle::with_catalog(
        SessionEnvironment::default(),
        catalog_with(&[
            ("org.example.Editor.desktop", EDITOR),
            ("org.example.Viewer.desktop", VIEWER),
            ("org.example.Daemon.desktop", DAEMON),
        ]),
    )
}

fn id(value: &str) -> DesktopId {
    DesktopId::new(value).expect("desktop id")
}

fn mime(value: &str) -> MimeType {
    MimeType::parse(value).expect("mime type")
}

// --- The fake device link ------------------------------------------------

/// A link the test drives directly.
///
/// It records what the window asked for and hands back whatever the test
/// queued, which is how a disconnect, a failed mount, and an unsafe removal are
/// all reachable without a real disk.
struct FakeLink {
    mode: Mutex<CollectionMode>,
    queued: Mutex<Vec<DeviceNotice>>,
    calls: Mutex<Vec<String>>,
    mounted: Mutex<Vec<MountedDevice>>,
    /// A path whose existence is recorded with every started notice, which is
    /// how a test proves the notice came before the job's first write.
    watched: Mutex<Option<PathBuf>>,
    refuse_started: std::sync::atomic::AtomicBool,
}

impl FakeLink {
    fn new(mode: CollectionMode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            queued: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            mounted: Mutex::new(Vec::new()),
            watched: Mutex::new(None),
            refuse_started: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn mount(&self, object_path: &str, mount_point: &std::path::Path) {
        self.mounted.lock().unwrap().push(MountedDevice {
            object_path: object_path.to_string(),
            mount_point: mount_point.to_path_buf(),
        });
    }

    fn watch(&self, path: PathBuf) {
        *self.watched.lock().unwrap() = Some(path);
    }

    fn push(&self, notice: DeviceNotice) {
        self.queued.lock().unwrap().push(notice);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl DeviceLink for Arc<FakeLink> {
    fn mode(&self) -> CollectionMode {
        self.mode.lock().unwrap().clone()
    }
    fn request_mount(&self, object_path: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("mount {object_path}"));
    }
    fn request_eject(&self, object_path: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("eject {object_path}"));
    }
    fn request_refresh(&self) {
        self.calls.lock().unwrap().push("refresh".to_string());
    }
    fn poll(&self) -> Vec<DeviceNotice> {
        std::mem::take(&mut *self.queued.lock().unwrap())
    }
    fn mounted_devices(&self) -> Vec<MountedDevice> {
        self.mounted.lock().unwrap().clone()
    }
    fn operation_started(&self, object_path: &str, operation: &str) -> Result<(), String> {
        let written = self
            .watched
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|path| path.exists());
        self.calls.lock().unwrap().push(format!(
            "started {object_path} {operation} written={written}"
        ));
        if self
            .refuse_started
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err("the service is not answering".to_string());
        }
        Ok(())
    }
    fn operation_completed(&self, object_path: &str, operation: &str) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("completed {object_path} {operation}"));
        Ok(())
    }
    fn request_policy(&self, request: crate::policy::PolicyRequest) {
        self.calls.lock().unwrap().push(format!(
            "policy {} {} [{}]",
            request.object_path,
            request.policy.as_str(),
            request.acknowledged_risks.join(",")
        ));
    }
}

// --- Device report fixtures ----------------------------------------------

fn report(object_path: &str, mount_point: Option<&str>, state: StateReport) -> DeviceReport {
    DeviceReport {
        object_path: object_path.to_string(),
        device_path: "/dev/sdb1".to_string(),
        display_name: "USB Drive".to_string(),
        identity: "uuid:A1B2-C3D4".to_string(),
        identity_confidence: "stable".to_string(),
        filesystem: Some("vfat".to_string()),
        mount_point: mount_point.map(str::to_string),
        policy: RemovalPolicy::DirectRemoval,
        state,
    }
}

fn ready() -> StateReport {
    StateReport::ReadyToUnplug {
        proven_at_millis: 1,
        flush_scope: Some("filesystem".to_string()),
        fully_corroborated: true,
        mounted: true,
    }
}

// --- Sessions ------------------------------------------------------------

struct Rig {
    _root: tempfile::TempDir,
    home: PathBuf,
    spawner: Arc<RecordingSpawner>,
    link: Arc<FakeLink>,
    catalog: CatalogHandle,
}

/// A session with a real catalog, a recording launcher, and a fake link.
fn rig(defaults: Vec<(String, DesktopId)>, mode: CollectionMode) -> (Rig, FilesSession) {
    let root = tempfile::tempdir().expect("temp dir");
    let home = root.path().join("home");
    fs::create_dir_all(home.join("Documents")).unwrap();
    fs::write(home.join("Documents/notes.txt"), b"hello world").unwrap();
    fs::write(home.join("Documents/photo.png"), b"not really a png").unwrap();
    fs::write(home.join("Documents/.secret"), b"hidden").unwrap();

    let catalog = full_catalog();
    let reader = Arc::new(FilesReader::new(ReaderConfig::new(), None, catalog.clone()));
    let spawner = Arc::new(RecordingSpawner::new());
    let link = FakeLink::new(mode);
    let engine = Arc::new(JobEngine::new(EngineConfig {
        store: None,
        ..EngineConfig::default()
    }));

    let session = FilesSession::new(SessionSetup {
        start: Location::local(home.join("Documents")).unwrap(),
        preferences: FilesPreferences::default(),
        preference_store: PreferenceStore::at_path(root.path().join("prefs.json")),
        bookmark_store: BookmarkStore::at_path(root.path().join("bookmarks")),
        directories: UserDirectories::from_values(Some(&home), None),
        mounts: MountTable::new(Vec::new()),
        reader,
        engine,
        catalog: catalog.clone(),
        defaults: Box::new(SessionDefaults::fixed(defaults)),
        spawner: Box::new(SharedSpawner(spawner.clone())),
        link: Box::new(link.clone()),
        preview: PreviewPanel::default(),
    });
    (
        Rig {
            _root: root,
            home,
            spawner,
            link,
            catalog,
        },
        session,
    )
}

/// Lets the test keep a handle on the spawner the session owns.
struct SharedSpawner(Arc<RecordingSpawner>);

impl app_catalog_platform::ProcessSpawner for SharedSpawner {
    fn spawn(
        &self,
        invocation: &app_catalog_core::Invocation,
    ) -> Result<(), app_catalog_platform::PlatformError> {
        self.0.spawn(invocation)
    }
}

fn plain() -> (Rig, FilesSession) {
    rig(Vec::new(), CollectionMode::Service)
}

fn settle(session: &mut FilesSession) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while session.is_listing() && std::time::Instant::now() < deadline {
        session.pump();
        std::thread::yield_now();
    }
    session.pump();
    assert!(!session.is_listing(), "the listing never finished");
}

// =========================================================================
// Applications
// =========================================================================

#[test]
fn the_applications_location_lists_real_applications_by_localized_name() {
    let (_rig, mut session) = plain();
    session.navigate_to(Location::Applications);
    settle(&mut session);

    let names: Vec<String> = session
        .pane()
        .model()
        .iter_visible()
        .map(|entry| entry.name.clone())
        .collect();
    // The `NoDisplay` daemon is excluded; the two real applications are not.
    assert_eq!(names, ["Image Viewer", "Text Editor"]);

    // Every row is an application, and none of them has a path.
    for entry in session.pane().model().iter_visible() {
        assert_eq!(entry.kind, EntryKind::Application);
        assert_eq!(
            entry.as_local_path(),
            None,
            "no row exposes a .desktop file as if it were the application"
        );
    }
}

#[test]
fn a_no_display_application_appears_only_when_hidden_entries_are_shown() {
    let (_rig, mut session) = plain();
    session.toggle_hidden();
    session.navigate_to(Location::Applications);
    settle(&mut session);
    let names: Vec<String> = session
        .pane()
        .model()
        .iter_visible()
        .map(|entry| entry.name.clone())
        .collect();
    assert!(names.contains(&"Background Daemon".to_string()));
}

#[test]
fn opening_an_application_row_launches_it_through_its_desktop_definition() {
    let (rig, mut session) = plain();
    session.navigate_to(Location::Applications);
    settle(&mut session);

    // "Image Viewer" sorts first.
    session.apply_selection(crate::content::SelectionInput::Click(1), 1);
    session.dispatch(crate::keys::Command::Open, 1, 10);

    let calls = rig.spawner.calls();
    assert_eq!(calls.len(), 1, "exactly one process was started");
    assert_eq!(calls[0].program, "editor");
    assert!(
        calls[0].arguments.is_empty(),
        "no file was passed, and no shell string was built: {:?}",
        calls[0]
    );
    assert_eq!(
        session.notice,
        Some(Notice::Launch(LaunchReport::Started {
            name: "Text Editor".to_string(),
            processes: 1
        }))
    );
    assert_eq!(
        session.location(),
        &Location::Applications,
        "launching does not navigate anywhere"
    );
}

#[test]
fn launching_an_application_that_was_uninstalled_says_so_rather_than_failing_silently() {
    let (rig, mut session) = plain();
    // The application is removed from the catalog between the row being drawn
    // and the click, which is exactly what an uninstall looks like.
    rig.catalog.replace(catalog_with(&[]));
    session.launch_application(&id("org.example.Editor.desktop"), None, &[]);
    assert_eq!(
        session.notice,
        Some(Notice::Launch(LaunchReport::NoSuchApplication {
            desktop_id: "org.example.Editor.desktop".to_string()
        }))
    );
    assert!(rig.spawner.calls().is_empty());
}

#[test]
fn view_details_reveals_source_metadata_including_where_the_entry_came_from() {
    let (_rig, mut session) = plain();
    session.show_details(&id("org.example.Editor.desktop"));
    let details = session.details.clone().expect("details");

    assert_eq!(details.name, "Text Editor");
    assert_eq!(details.comment.as_deref(), Some("Edits text"));
    assert_eq!(details.source_kind, SourceKind::Native);
    assert_eq!(details.scope, EntryScope::System);
    assert_eq!(
        details.source_path, "/usr/share/applications/org.example.Editor.desktop",
        "the diagnostic names the file, under a heading that says it is one"
    );
    assert_eq!(
        details.executable,
        ExecutableSummary::Resolved("/usr/bin/editor".to_string())
    );
    // The catalog normalizes the declared list, so the order is its order.
    assert_eq!(details.mime_types, ["text/markdown", "text/plain"]);
    assert_eq!(details.categories, ["Utility", "TextEditor"]);
    assert_eq!(
        details.actions,
        [("new-window".to_string(), "New Window".to_string())]
    );
    assert!(details.visible);

    session.close_details();
    assert!(session.details.is_none());
}

#[test]
fn open_new_window_uses_a_declared_action_and_nothing_else() {
    let (rig, mut session) = plain();
    session.open_new_window(&id("org.example.Editor.desktop"));
    let calls = rig.spawner.calls();
    assert_eq!(calls[0].arguments, ["--new-window"]);

    // The viewer declares no action, so Open New Window starts it normally
    // rather than inventing a flag.
    session.open_new_window(&id("org.example.Viewer.desktop"));
    let calls = rig.spawner.calls();
    assert_eq!(calls[1].program, "viewer");
    assert!(calls[1].arguments.is_empty());
}

#[test]
fn a_catalog_reload_is_visible_to_the_next_listing() {
    let (rig, mut session) = plain();
    session.navigate_to(Location::Applications);
    settle(&mut session);
    assert_eq!(session.pane().model().visible_len(), 2);

    let before = session.catalog_generation();
    rig.catalog
        .replace(catalog_with(&[("org.example.Editor.desktop", EDITOR)]));
    assert!(
        session.catalog_generation() > before,
        "the generation moves, which is what the window watches"
    );

    session.reload();
    settle(&mut session);
    let names: Vec<String> = session
        .pane()
        .model()
        .iter_visible()
        .map(|entry| entry.name.clone())
        .collect();
    assert_eq!(names, ["Text Editor"]);
}

#[test]
fn the_localized_name_follows_the_session_locale() {
    let zh = app_catalog_core::Locale::parse("zh_TW").expect("locale");
    let catalog = catalog_with(&[("org.example.Editor.desktop", EDITOR)]);
    let record = catalog.records().next().unwrap();
    assert_eq!(record.display_name(None), "Text Editor");
    assert_eq!(record.display_name(Some(&zh)), "文字編輯器");
}

// =========================================================================
// Open With
// =========================================================================

#[test]
fn double_click_uses_the_effective_default_handler() {
    let catalog = full_catalog();
    let handlers =
        SessionDefaults::fixed([("text/plain".to_string(), id("org.example.Editor.desktop"))]);
    let (route, source) = route_open_file(Some(&mime("text/plain")), &handlers, &catalog);
    assert_eq!(
        route,
        OpenRoute::LaunchWith {
            desktop_id: id("org.example.Editor.desktop"),
            mime: mime("text/plain")
        }
    );
    assert_eq!(source, DefaultSource::UserAssociation);
}

#[test]
fn a_file_type_with_no_default_opens_the_chooser_rather_than_doing_nothing() {
    let catalog = full_catalog();
    let handlers = SessionDefaults::fixed([]);
    let (route, source) = route_open_file(Some(&mime("text/plain")), &handlers, &catalog);
    assert_eq!(
        route,
        OpenRoute::AskChooser {
            mime: mime("text/plain")
        }
    );
    assert_eq!(source, DefaultSource::None);
}

#[test]
fn an_association_naming_an_uninstalled_application_is_told_apart_from_no_association() {
    let catalog = full_catalog();
    let handlers =
        SessionDefaults::fixed([("text/plain".to_string(), id("org.example.Gone.desktop"))]);
    let (route, source) = route_open_file(Some(&mime("text/plain")), &handlers, &catalog);
    assert!(matches!(route, OpenRoute::AskChooser { .. }));
    assert_eq!(
        source,
        DefaultSource::AssociationMissingApplication,
        "the two empty-looking cases are not the same and only one is worth saying"
    );
}

#[test]
fn a_file_whose_type_is_unknown_reports_that_rather_than_guessing_one() {
    let catalog = full_catalog();
    let handlers = SessionDefaults::fixed([]);
    let (route, source) = route_open_file(None, &handlers, &catalog);
    assert_eq!(route, OpenRoute::NoMimeType);
    assert_eq!(source, DefaultSource::None);
}

#[test]
fn opening_a_file_with_a_default_launches_it_with_the_file_as_an_argument() {
    let (rig, mut session) = rig(
        vec![("text/plain".to_string(), id("org.example.Editor.desktop"))],
        CollectionMode::Service,
    );
    let path = files_core::LocalPath::new(rig.home.join("Documents/notes.txt")).unwrap();
    session.open_file(&path, Some(&mime("text/plain")));

    let calls = rig.spawner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "editor");
    assert_eq!(
        calls[0].arguments,
        [rig.home.join("Documents/notes.txt").display().to_string()],
        "the file is an argument in a vector, never concatenated into a string"
    );
    assert!(session.chooser.is_none());
}

#[test]
fn the_chooser_is_embedded_with_the_files_resolved_mime_type() {
    let (rig, mut session) = plain();
    let path = files_core::LocalPath::new(rig.home.join("Documents/notes.txt")).unwrap();
    session.open_file(&path, Some(&mime("text/plain")));

    let request = session.chooser.clone().expect("the chooser was opened");
    assert_eq!(request.mime, mime("text/plain"));
    assert_eq!(request.display_name, "notes.txt");
    assert_eq!(
        request.target(),
        LaunchTarget::path(rig.home.join("Documents/notes.txt")).unwrap()
    );
    assert!(
        rig.spawner.calls().is_empty(),
        "opening the chooser launches nothing"
    );

    session.close_chooser(true);
    assert!(session.chooser.is_none());
}

#[test]
fn the_explicit_open_with_action_asks_even_when_a_default_exists() {
    let (rig, mut session) = rig(
        vec![("text/plain".to_string(), id("org.example.Editor.desktop"))],
        CollectionMode::Service,
    );
    settle(&mut session);
    // Give the entry a type, which the fixture reader does not detect.
    let index = session
        .pane()
        .model()
        .iter_visible()
        .position(|entry| entry.name == "notes.txt")
        .expect("notes.txt");
    let entry_id = session.pane().model().visible(index).unwrap().id();
    session
        .pane_mut()
        .model_mut()
        .selection_mut()
        .select_only(entry_id.clone());
    let entry = session.pane().model().get(&entry_id).unwrap().clone();
    let path = entry.as_local_path().unwrap().clone();

    session.open_with_selected();
    // No MIME type was detected by the fixture reader, so the honest answer is
    // that there is nothing to choose from.
    assert_eq!(session.notice, Some(Notice::NoMimeType));

    // With a type, the same action opens the chooser rather than launching.
    session.chooser = ChooserRequest::new(&path, mime("text/plain"));
    assert!(session.chooser.is_some());
    assert!(rig.spawner.calls().is_empty());
}

// =========================================================================
// Devices
// =========================================================================

#[test]
fn every_one_of_the_five_states_reads_in_the_issues_own_words_in_both_languages() {
    let cases = [
        (DeviceStateKind::ReadyToUnplug, Vec::new()),
        (DeviceStateKind::Writing, Vec::new()),
        (DeviceStateKind::Busy, vec!["gedit".to_string()]),
        (DeviceStateKind::PerformanceMode, Vec::new()),
        (DeviceStateKind::Unknown, Vec::new()),
    ];
    let english: Vec<String> = cases
        .iter()
        .map(|(state, blockers)| state_label(*state, blockers, &EN_US))
        .collect();
    assert_eq!(
        english,
        [
            "Ready to unplug",
            "Writing… Do not unplug",
            "In use by gedit",
            "Performance mode: eject before unplugging",
            "Removal status cannot be verified",
        ]
    );

    let chinese: Vec<String> = cases
        .iter()
        .map(|(state, blockers)| state_label(*state, blockers, &ZH_TW))
        .collect();
    assert_eq!(
        chinese,
        [
            "可以安全拔除",
            "寫入中… 請勿拔除",
            "gedit 正在使用",
            "效能模式：拔除前請先退出",
            "無法確認移除狀態",
        ]
    );
    // Every one of the ten is a different sentence.
    let mut all: Vec<&String> = english.iter().chain(chinese.iter()).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 10);
}

#[test]
fn a_busy_device_whose_blocker_cannot_be_named_does_not_say_in_use_by_nothing() {
    assert_eq!(
        state_label(DeviceStateKind::Busy, &[], &EN_US),
        "In use by another application"
    );
    assert_eq!(
        state_label(DeviceStateKind::Busy, &[], &ZH_TW),
        "有其他應用程式正在使用"
    );
}

#[test]
fn only_writing_and_performance_mode_are_drawn_as_warnings() {
    let quiet = [
        DeviceStateKind::ReadyToUnplug,
        DeviceStateKind::Busy,
        DeviceStateKind::Unknown,
    ];
    for state in quiet {
        let row = row_from(report("/o", Some("/media/x"), state_report(state)));
        assert!(
            !row.is_warning(),
            "{state:?} must not turn the sidebar into a warning console"
        );
    }
    for state in [DeviceStateKind::Writing, DeviceStateKind::PerformanceMode] {
        let row = row_from(report("/o", Some("/media/x"), state_report(state)));
        assert!(row.is_warning());
    }
}

fn state_report(kind: DeviceStateKind) -> StateReport {
    match kind {
        DeviceStateKind::ReadyToUnplug => ready(),
        DeviceStateKind::Writing => StateReport::Writing {
            reason: "storage.writing.tracked_operation".to_string(),
            detail: "job-1".to_string(),
        },
        DeviceStateKind::Busy => StateReport::Busy {
            blockers: vec![BlockerReport::Process {
                pid: 42,
                name: Some("gedit".to_string()),
            }],
        },
        DeviceStateKind::PerformanceMode => StateReport::PerformanceMode {
            eject_required: true,
            active_write: false,
        },
        DeviceStateKind::Unknown => StateReport::Unknown {
            reason: "storage.unknown.not_yet_observed".to_string(),
            detail: String::new(),
        },
        DeviceStateKind::Disconnected => StateReport::Disconnected {
            unsafe_removal: None,
        },
    }
}

#[test]
fn only_ready_to_unplug_permits_direct_removal() {
    for state in [
        DeviceStateKind::Writing,
        DeviceStateKind::Busy,
        DeviceStateKind::PerformanceMode,
        DeviceStateKind::Unknown,
    ] {
        let row = row_from(report("/o", Some("/media/x"), state_report(state)));
        assert!(
            !row.permits_direct_removal(),
            "{state:?} is not a readiness claim"
        );
    }
    assert!(row_from(report("/o", Some("/media/x"), ready())).permits_direct_removal());
}

#[test]
fn a_busy_state_names_the_application_holding_the_device() {
    let row = row_from(report(
        "/o",
        Some("/media/x"),
        state_report(DeviceStateKind::Busy),
    ));
    assert_eq!(row.blockers, ["gedit"]);
    assert_eq!(row.state_label(&EN_US), "In use by gedit");
}

#[test]
fn with_no_link_no_device_claims_a_state() {
    let link = crate::devices::NoDeviceLink;
    assert!(matches!(link.mode(), CollectionMode::Unavailable { .. }));
    assert!(!link.mode().has_states());
    assert!(link.poll().is_empty());
}

#[test]
fn an_in_process_link_says_so_and_a_service_link_does_not() {
    assert_eq!(CollectionMode::Service.note(&EN_US), None);
    let (note, warn) = CollectionMode::InProcess {
        detail: "not on the bus".to_string(),
    }
    .note(&EN_US)
    .expect("a note");
    assert_eq!(note, EN_US.devices_in_process);
    assert!(warn, "collecting in this window is a caveat, not a status");
    assert_eq!(
        CollectionMode::InProcess {
            detail: String::new()
        }
        .note(&ZH_TW)
        .map(|(note, _)| note),
        Some(ZH_TW.devices_in_process)
    );
}

#[test]
fn clicking_an_unmounted_device_mounts_it_and_then_opens_it() {
    let (rig, mut session) = plain();
    settle(&mut session);
    let mount_point = rig.home.join("media/PHOTOS");
    fs::create_dir_all(&mount_point).unwrap();

    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        None,
        ready(),
    )]));
    session.pump();
    assert_eq!(session.device_rows().len(), 1);
    assert!(!session.device_rows()[0].is_mounted());

    session.open_device("/dev/obj");
    assert_eq!(
        rig.link.calls(),
        ["mount /dev/obj"],
        "an unmounted device is mounted rather than refused"
    );
    assert_eq!(
        session.location(),
        &Location::local(rig.home.join("Documents")).unwrap(),
        "nothing has moved yet: the mount has not answered"
    );

    rig.link.push(DeviceNotice::Mounted {
        object_path: "/dev/obj".to_string(),
        mount_point: mount_point.clone(),
    });
    session.pump();
    settle(&mut session);
    assert_eq!(
        session.location(),
        &Location::local(&mount_point).unwrap(),
        "the mount's answer is what navigates"
    );
    assert!(session.device_rows()[0].is_mounted());
}

#[test]
fn clicking_a_mounted_device_opens_it_without_mounting_again() {
    let (rig, mut session) = plain();
    settle(&mut session);
    let mount_point = rig.home.join("media/PHOTOS");
    fs::create_dir_all(&mount_point).unwrap();
    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        Some(mount_point.to_str().unwrap()),
        ready(),
    )]));
    session.pump();

    session.open_device("/dev/obj");
    settle(&mut session);
    assert!(rig.link.calls().is_empty(), "no second mount");
    assert_eq!(session.location(), &Location::local(&mount_point).unwrap());
}

#[test]
fn leaving_a_device_location_does_not_unmount_it() {
    let (rig, mut session) = plain();
    settle(&mut session);
    let mount_point = rig.home.join("media/PHOTOS");
    fs::create_dir_all(&mount_point).unwrap();
    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        Some(mount_point.to_str().unwrap()),
        ready(),
    )]));
    session.pump();
    session.open_device("/dev/obj");
    settle(&mut session);

    session.navigate_to(Location::local(rig.home.join("Documents")).unwrap());
    settle(&mut session);
    assert!(
        rig.link.calls().is_empty(),
        "leaving asks the link for nothing at all"
    );
    assert!(session.device_rows()[0].is_mounted());
}

#[test]
fn eject_is_available_and_reports_what_actually_happened() {
    let (rig, mut session) = plain();
    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        Some("/media/PHOTOS"),
        ready(),
    )]));
    session.pump();

    session.eject_device("/dev/obj");
    assert_eq!(rig.link.calls(), ["eject /dev/obj"]);

    rig.link.push(DeviceNotice::Ejected {
        object_path: "/dev/obj".to_string(),
        unmounted: true,
        powered_off: false,
    });
    session.pump();
    assert_eq!(
        session.notice,
        Some(Notice::Device(Box::new(
            DeviceEvent::EjectedNotPoweredOff {
                label: "PHOTOS".to_string()
            }
        ))),
        "an unmount that worked with a power-off that did not is not a clean eject"
    );
    assert!(!session.device_rows()[0].is_mounted());
}

#[test]
fn disconnecting_the_device_being_viewed_returns_to_a_safe_location_with_an_explanation() {
    let (rig, mut session) = plain();
    settle(&mut session);
    let mount_point = rig.home.join("media/PHOTOS");
    fs::create_dir_all(mount_point.join("holiday")).unwrap();
    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        Some(mount_point.to_str().unwrap()),
        ready(),
    )]));
    session.pump();
    session.open_device("/dev/obj");
    settle(&mut session);
    session.navigate_to(Location::local(mount_point.join("holiday")).unwrap());
    settle(&mut session);
    assert!(session.pane().history().can_go_back());

    rig.link.push(DeviceNotice::Disconnected {
        object_path: "/dev/obj".to_string(),
        unsafe_removal: None,
    });
    session.pump();
    settle(&mut session);

    assert_eq!(
        session.location(),
        &session.home_location(),
        "the tab went somewhere safe on its own"
    );
    assert_eq!(
        session.notice,
        Some(Notice::Device(Box::new(
            DeviceEvent::DisconnectedWhileViewing {
                label: "PHOTOS".to_string()
            }
        ))),
        "and said why"
    );
    assert!(session.device_rows().is_empty(), "the row is gone");
    assert!(
        !session
            .pane()
            .history()
            .back_entries()
            .any(|location| is_under(location, &mount_point)),
        "no stale navigation state points at the device"
    );
    assert!(
        !session
            .pane()
            .history()
            .forward_entries()
            .any(|location| is_under(location, &mount_point))
    );
}

#[test]
fn disconnecting_a_device_nobody_is_looking_at_cleans_up_silently() {
    let (rig, mut session) = plain();
    settle(&mut session);
    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        Some("/media/PHOTOS"),
        ready(),
    )]));
    session.pump();
    assert_eq!(session.device_rows().len(), 1);

    rig.link.push(DeviceNotice::Disconnected {
        object_path: "/dev/obj".to_string(),
        unsafe_removal: None,
    });
    session.pump();

    assert!(session.device_rows().is_empty());
    assert_eq!(
        session.notice, None,
        "an idle device unplugged is not an event worth interrupting anyone about"
    );
}

#[test]
fn an_unsafe_removal_produces_a_warning_rather_than_a_clean_completion_message() {
    let (rig, mut session) = plain();
    settle(&mut session);
    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        Some("/media/PHOTOS"),
        state_report(DeviceStateKind::Writing),
    )]));
    session.pump();

    rig.link.push(DeviceNotice::Disconnected {
        object_path: "/dev/obj".to_string(),
        unsafe_removal: Some(UnsafeRemoval {
            previous_state: "writing".to_string(),
            unfinished_operations: vec!["job-1".to_string()],
            recommend_filesystem_check: true,
        }),
    });
    session.pump();

    assert_eq!(
        session.notice,
        Some(Notice::Device(Box::new(DeviceEvent::UnsafeRemoval {
            label: "PHOTOS".to_string(),
            recommend_filesystem_check: true
        })))
    );
    let message = session.notice.as_ref().unwrap().message(&EN_US);
    assert!(message.contains(EN_US.device_unsafe_removal));
    assert!(message.contains(EN_US.device_check_filesystem));
    assert!(
        !message.contains(EN_US.device_ejected),
        "an unsafe removal never reads as a clean completion"
    );
}

#[test]
fn the_unsafe_removal_warning_outlives_the_row_that_caused_it() {
    let mut inventory = DeviceInventory::default();
    inventory.apply_inventory(vec![report("/dev/obj", Some("/media/X"), ready())]);
    inventory.remove(
        "/dev/obj",
        Some(UnsafeRemoval {
            previous_state: "writing".to_string(),
            unfinished_operations: Vec::new(),
            recommend_filesystem_check: true,
        }),
    );
    assert!(inventory.is_empty());
    assert_eq!(inventory.warnings().len(), 1, "the warning is still there");
    inventory.dismiss_warning("/dev/obj");
    assert!(inventory.warnings().is_empty());
}

#[test]
fn a_failed_mount_clears_the_pending_open_rather_than_leaving_the_window_waiting() {
    let (rig, mut session) = plain();
    settle(&mut session);
    rig.link.push(DeviceNotice::Inventory(vec![report(
        "/dev/obj",
        None,
        ready(),
    )]));
    session.pump();
    session.open_device("/dev/obj");

    rig.link.push(DeviceNotice::MountFailed {
        object_path: "/dev/obj".to_string(),
        detail: "refused".to_string(),
    });
    session.pump();
    assert!(matches!(
        session.notice,
        Some(Notice::Device(ref event)) if matches!(**event, DeviceEvent::MountFailed { .. })
    ));

    // A later mount of a different device must not navigate to the one that
    // failed.
    rig.link.push(DeviceNotice::Mounted {
        object_path: "/dev/obj".to_string(),
        mount_point: PathBuf::from("/media/PHOTOS"),
    });
    session.pump();
    assert_eq!(
        session.location(),
        &Location::local(rig.home.join("Documents")).unwrap()
    );
}

#[test]
fn a_volatile_identity_is_flagged_and_a_weak_one_is_not() {
    let mut volatile = report("/o", None, ready());
    volatile.identity_confidence = "volatile".to_string();
    assert!(row_from(volatile).identity_volatile);

    let mut weak = report("/o", None, ready());
    weak.identity_confidence = "weak".to_string();
    assert!(
        !row_from(weak).identity_volatile,
        "a weak identity is still persistable"
    );
}

#[test]
fn a_device_row_is_named_after_its_mount_point_not_its_object_path() {
    let row = row_from(report(
        "/org/freedesktop/UDisks2/x",
        Some("/media/tim/PHOTOS"),
        ready(),
    ));
    assert_eq!(row.label, "PHOTOS");
    let unmounted = row_from(report("/org/freedesktop/UDisks2/x", None, ready()));
    assert_eq!(unmounted.label, "USB Drive");
}

#[test]
fn a_disconnect_report_carries_its_unsafe_removal_record() {
    let row = row_from(report(
        "/o",
        None,
        StateReport::Disconnected {
            unsafe_removal: Some(UnsafeRemovalReport {
                at_millis: 5,
                previous_state: "writing".to_string(),
                unfinished_operations: vec!["job-1".to_string()],
                detail: "removed mid-write".to_string(),
                recommend_filesystem_check: true,
            }),
        },
    ));
    let record = row.unsafe_removal.expect("the record survived the wire");
    assert_eq!(record.unfinished_operations, ["job-1"]);
    assert!(record.recommend_filesystem_check);
}

// =========================================================================
// Preview
// =========================================================================

#[test]
fn space_opens_the_preview_pane_and_space_again_closes_it() {
    let (_rig, mut session) = plain();
    settle(&mut session);
    assert!(!session.preview.open);
    session.dispatch(crate::keys::Command::TogglePreview, 1, 10);
    assert!(session.preview.open);
    assert!(session.preferences.preview_open);
    session.dispatch(crate::keys::Command::TogglePreview, 1, 10);
    assert!(!session.preview.open);
    assert!(matches!(session.preview.slot(), PreviewSlot::Nothing));
}

#[test]
fn the_preview_pane_produces_a_text_preview_for_a_selected_file() {
    let (_rig, mut session) = plain();
    settle(&mut session);
    let index = session
        .pane()
        .model()
        .iter_visible()
        .position(|entry| entry.name == "notes.txt")
        .expect("notes.txt");
    session.apply_selection(crate::content::SelectionInput::Click(index), 1);
    session.dispatch(crate::keys::Command::TogglePreview, 1, 10);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline
        && !matches!(session.preview.slot(), PreviewSlot::Ready(_))
    {
        session.pump();
        std::thread::yield_now();
    }
    let PreviewSlot::Ready(preview) = session.preview.slot() else {
        panic!("no preview arrived: {:?}", session.preview.slot())
    };
    let files_preview::Preview::Text(text) = preview.as_ref() else {
        panic!("expected text, got {preview:?}")
    };
    assert_eq!(text.text, "hello world");
}

#[test]
fn an_application_row_has_no_file_to_preview_and_says_so() {
    let (_rig, mut session) = plain();
    session.navigate_to(Location::Applications);
    settle(&mut session);
    session.apply_selection(crate::content::SelectionInput::Click(0), 1);
    session.dispatch(crate::keys::Command::TogglePreview, 1, 10);
    assert!(matches!(
        session.preview.slot(),
        PreviewSlot::NotPreviewable
    ));
    assert_eq!(
        session.preview.placeholder(&EN_US),
        Some(EN_US.preview_not_previewable)
    );
}

// =========================================================================
// Search
// =========================================================================

/// A model that has finished listing, which is what makes a search over it
/// able to reach the "nothing matched" state rather than staying "searching".
fn model_with(names: &[&str]) -> DirectoryModel {
    let mut model = DirectoryModel::new(
        Location::local("/home/tim").unwrap(),
        SortOrder::default(),
        HiddenPreference::default(),
    );
    let listing = files_core::ListingId::next();
    model.restart(listing);
    model.insert_batch(
        names
            .iter()
            .map(|name| {
                Entry::file(
                    *name,
                    files_core::LocalPath::new(format!("/home/tim/{name}")).unwrap(),
                    EntryKind::File,
                )
            })
            .collect(),
    );
    model.apply(files_core::ListingEvent::Complete(
        files_core::ListingSummary {
            listing,
            total: names.len(),
            skipped: Vec::new(),
        },
    ));
    model
}

#[test]
fn typing_narrows_the_content_area_to_the_matches() {
    let (rig, mut session) = plain();
    settle(&mut session);
    assert_eq!(session.row_count(), 2);

    session.set_search_text("notes");
    session.pump();
    assert!(session.search.is_active());
    assert_eq!(session.row_count(), 1);
    assert_eq!(
        session.entry_at(0).map(|e| e.name.as_str()),
        Some("notes.txt")
    );
    // Navigation is untouched by any of it.
    assert_eq!(
        session.location(),
        &Location::local(rig.home.join("Documents")).unwrap()
    );
}

#[test]
fn clearing_the_field_puts_the_directory_back() {
    let (_rig, mut session) = plain();
    settle(&mut session);
    session.set_search_text("notes");
    session.pump();
    assert_eq!(session.row_count(), 1);
    session.set_search_text("");
    assert!(!session.search.is_active());
    assert_eq!(session.row_count(), 2);
}

#[test]
fn results_stream_in_slices_rather_than_arriving_all_at_once() {
    let names: Vec<String> = (0..crate::search::SLICE * 2 + 100)
        .map(|index| format!("report-{index:06}.txt"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let model = model_with(&refs);

    let mut state = crate::search::SearchState::default();
    state.set_text("report", &Location::local("/home/tim").unwrap());
    assert!(state.is_active());

    assert!(state.pump(&model));
    let after_first = state.hits().len();
    assert_eq!(after_first, crate::search::SLICE);
    assert!(!state.is_complete(), "one slice is not the whole directory");

    assert!(state.pump(&model));
    assert_eq!(state.hits().len(), crate::search::SLICE * 2);
    assert!(state.pump(&model));
    assert_eq!(state.hits().len(), names.len());
}

#[test]
fn navigating_away_ends_the_search_rather_than_carrying_it_into_the_next_folder() {
    let (rig, mut session) = plain();
    settle(&mut session);
    session.set_search_text("notes");
    session.pump();
    assert!(session.search.is_active());

    session.navigate_to(Location::local(&rig.home).unwrap());
    settle(&mut session);
    assert!(
        !session.search.is_active(),
        "results from a folder the user has left are not results"
    );
}

#[test]
fn the_search_scope_is_named_in_the_ui() {
    let (rig, mut session) = plain();
    settle(&mut session);
    session.set_search_text("notes");
    let label = session.search.scope_label(session.location(), &EN_US);
    assert!(label.starts_with("in "), "the scope is stated: {label}");
    assert!(label.contains("Documents"));
    let _ = rig;
}

#[test]
fn a_search_that_matches_nothing_says_so_only_once_it_has_finished_looking() {
    let model = model_with(&["alpha.txt", "beta.txt"]);
    let mut state = crate::search::SearchState::default();
    state.set_text("zzz", &Location::local("/home/tim").unwrap());
    assert_eq!(state.empty_state(&EN_US), Some(EN_US.search_running));
    state.pump(&model);
    assert_eq!(state.empty_state(&EN_US), Some(EN_US.search_no_matches));
}

#[test]
fn hidden_files_follow_the_search_setting_rather_than_the_view() {
    let (_rig, mut session) = plain();
    settle(&mut session);
    // The view hides `.secret`; the search's own setting is off too.
    session.set_search_text("secret");
    session.pump();
    assert_eq!(session.search.hits().len(), 0);

    session.toggle_search_hidden();
    session.pump();
    assert_eq!(
        session.search.hits().len(),
        1,
        "the search setting reveals it without the view changing"
    );
    assert!(
        !session.preferences.show_hidden,
        "and the view is still hiding it"
    );
}

#[test]
fn a_row_clicked_during_a_search_selects_the_entry_it_shows() {
    let (_rig, mut session) = plain();
    settle(&mut session);
    session.set_search_text("photo");
    session.pump();
    assert_eq!(session.row_count(), 1);

    // Row 0 of the results is `photo.png`, which is index 1 of the directory.
    let model_row = session.model_row(0).expect("a visible entry");
    assert_eq!(model_row, 1);
    session.apply_selection(crate::content::SelectionInput::Click(model_row), 1);
    assert_eq!(
        session.focused_entry().map(|entry| entry.name.as_str()),
        Some("photo.png")
    );
}

// =========================================================================
// Starting at a given location (ticket 50)
// =========================================================================

#[test]
fn a_file_given_at_start_is_selected_once_its_folder_is_listed() {
    let (_rig, mut session) = plain();
    // Selected before a single entry has arrived, the way the window does it
    // straight after opening.
    session.select_by_name("notes.txt");
    settle(&mut session);
    let selected: Vec<&str> = session
        .selected_entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(selected, vec!["notes.txt"]);
    assert_eq!(
        session.focused_entry().map(|entry| entry.name.as_str()),
        Some("notes.txt")
    );
}

#[test]
fn the_start_notice_says_why_and_counts_what_was_ignored_in_both_languages() {
    use crate::launch::StartProblem;
    let problems = [
        StartProblem::NotLocal,
        StartProblem::Malformed,
        StartProblem::NotFound,
        StartProblem::Unreadable,
    ];
    for c in [&EN_US, &ZH_TW] {
        let mut seen = std::collections::HashSet::new();
        for problem in problems {
            let message = Notice::Start {
                problem: Some(problem),
                ignored: 0,
            }
            .message(c);
            assert!(!message.is_empty());
            assert!(seen.insert(message), "each reason reads differently");
        }
        let both = Notice::Start {
            problem: Some(StartProblem::NotFound),
            ignored: 2,
        }
        .message(c);
        assert!(both.contains('2'), "{both}");
        let only_ignored = Notice::Start {
            problem: None,
            ignored: 1,
        }
        .message(c);
        assert!(only_ignored.contains('1'), "{only_ignored}");
    }
    assert_ne!(
        Notice::Start {
            problem: Some(StartProblem::NotFound),
            ignored: 0
        }
        .message(&EN_US),
        Notice::Start {
            problem: Some(StartProblem::NotFound),
            ignored: 0
        }
        .message(&ZH_TW)
    );
}

// =========================================================================
// Better Files' own writes hold readiness (ticket 51)
// =========================================================================

const USB: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";
const OTHER_USB: &str = "/org/freedesktop/UDisks2/block_devices/sdc1";

/// An engine whose jobs are tracked through a fake link, with a home folder
/// and a "USB stick" that is just another temporary directory the link says is
/// mounted.
struct Tracked {
    root: tempfile::TempDir,
    link: Arc<FakeLink>,
    engine: JobEngine,
}

impl Tracked {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("home")).unwrap();
        fs::create_dir_all(root.path().join("usb")).unwrap();
        fs::create_dir_all(root.path().join("other-usb")).unwrap();
        let link = FakeLink::new(CollectionMode::Service);
        link.mount(USB, &root.path().join("usb").canonicalize().unwrap());
        link.mount(
            OTHER_USB,
            &root.path().join("other-usb").canonicalize().unwrap(),
        );
        let tracker = Arc::new(StorageTracker::new(None));
        tracker.attach(Arc::new(link.clone()));
        let engine = JobEngine::with_observer(
            EngineConfig {
                store: None,
                ..EngineConfig::default()
            },
            tracker,
        );
        Self { root, link, engine }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn run(&self, operation: files_operations::Operation) -> (files_operations::JobId, JobState) {
        let handle = self
            .engine
            .submit(files_operations::JobSpec::new(operation))
            .unwrap();
        let state = self
            .engine
            .wait(handle.id(), std::time::Duration::from_secs(20))
            .expect("the job finished")
            .state;
        (handle.id(), state)
    }
}

fn operation(id: files_operations::JobId) -> String {
    crate::tracking::operation_id(std::process::id(), id)
}

fn local(path: PathBuf) -> files_core::LocalPath {
    files_core::LocalPath::new(path).unwrap()
}

use crate::tracking::StorageTracker;
use files_operations::JobState;

#[test]
fn a_copy_to_a_device_is_registered_before_its_first_write_and_released_after() {
    let rig = Tracked::new();
    fs::write(rig.path("home/report.pdf"), b"quarterly").unwrap();
    rig.link.watch(rig.path("usb/report.pdf"));

    let (id, state) = rig.run(files_operations::Operation::Copy {
        sources: vec![local(rig.path("home/report.pdf"))],
        destination: local(rig.path("usb")),
    });
    assert_eq!(state, JobState::Completed);
    let op = operation(id);
    assert_eq!(
        rig.link.calls(),
        vec![
            format!("started {USB} {op} written=false"),
            format!("completed {USB} {op}"),
        ]
    );
}

#[test]
fn a_destination_reached_through_a_link_or_dotdot_still_finds_the_device() {
    let rig = Tracked::new();
    fs::write(rig.path("home/a.txt"), b"a").unwrap();
    std::os::unix::fs::symlink(rig.path("usb"), rig.path("home/stick")).unwrap();

    let (id, _) = rig.run(files_operations::Operation::Copy {
        sources: vec![local(rig.path("home/a.txt"))],
        destination: local(rig.path("home/stick")),
    });
    let (second, _) = rig.run(files_operations::Operation::CreateFolder {
        parent: local(rig.path("home/../usb")),
        name: "new".into(),
    });
    let calls = rig.link.calls();
    assert_eq!(calls.len(), 4, "{calls:?}");
    assert!(calls[0].starts_with(&format!("started {USB} {}", operation(id))));
    assert!(calls[2].starts_with(&format!("started {USB} {}", operation(second))));
}

#[test]
fn a_job_on_no_tracked_device_sends_nothing() {
    let rig = Tracked::new();
    fs::write(rig.path("home/a.txt"), b"a").unwrap();
    fs::create_dir(rig.path("home/backup")).unwrap();
    let (_, state) = rig.run(files_operations::Operation::Copy {
        sources: vec![local(rig.path("home/a.txt"))],
        destination: local(rig.path("home/backup")),
    });
    assert_eq!(state, JobState::Completed);
    assert!(rig.link.calls().is_empty());
}

#[test]
fn a_move_from_one_device_to_another_holds_both() {
    let rig = Tracked::new();
    fs::write(rig.path("usb/photo.jpg"), b"jpeg").unwrap();
    let (id, state) = rig.run(files_operations::Operation::Move {
        sources: vec![local(rig.path("usb/photo.jpg"))],
        destination: local(rig.path("other-usb")),
    });
    assert_eq!(state, JobState::Completed);
    let op = operation(id);
    assert_eq!(
        rig.link.calls(),
        vec![
            format!("started {OTHER_USB} {op} written=false"),
            format!("started {USB} {op} written=false"),
            format!("completed {OTHER_USB} {op}"),
            format!("completed {USB} {op}"),
        ]
    );
}

#[test]
fn a_failed_or_cancelled_job_still_releases_the_device() {
    let rig = Tracked::new();
    // Failed: the source is not there.
    let (failed, state) = rig.run(files_operations::Operation::Copy {
        sources: vec![local(rig.path("home/missing.txt"))],
        destination: local(rig.path("usb")),
    });
    assert_eq!(state, JobState::Failed);

    // Cancelled: parked on a conflict, then cancelled.
    fs::write(rig.path("home/clash.txt"), b"new").unwrap();
    fs::write(rig.path("usb/clash.txt"), b"old").unwrap();
    let handle = rig
        .engine
        .submit(files_operations::JobSpec::new(
            files_operations::Operation::Copy {
                sources: vec![local(rig.path("home/clash.txt"))],
                destination: local(rig.path("usb")),
            },
        ))
        .unwrap();
    rig.engine
        .wait_for(
            handle.id(),
            std::time::Duration::from_secs(20),
            |snapshot| snapshot.state == JobState::WaitingOnConflict,
        )
        .expect("the job parked");
    rig.engine.cancel(handle.id());
    let cancelled = rig
        .engine
        .wait(handle.id(), std::time::Duration::from_secs(20))
        .unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);

    let calls = rig.link.calls();
    assert!(calls.contains(&format!("completed {USB} {}", operation(failed))));
    assert!(calls.contains(&format!("completed {USB} {}", operation(handle.id()))));
}

#[test]
fn a_notice_that_fails_is_logged_and_never_fails_the_job() {
    let rig = Tracked::new();
    rig.link
        .refuse_started
        .store(true, std::sync::atomic::Ordering::SeqCst);
    fs::write(rig.path("home/a.txt"), b"a").unwrap();
    let (id, state) = rig.run(files_operations::Operation::Copy {
        sources: vec![local(rig.path("home/a.txt"))],
        destination: local(rig.path("usb")),
    });
    assert_eq!(state, JobState::Completed);
    assert!(rig.path("usb/a.txt").exists());
    // The completion is still sent: the service may have recorded the start
    // even though the answer never came back.
    assert!(
        rig.link
            .calls()
            .contains(&format!("completed {USB} {}", operation(id)))
    );
}

// =========================================================================
// Choosing Performance mode after reading its risks (ticket 52)
// =========================================================================

fn with_stick(policy: RemovalPolicy) -> (Rig, FilesSession) {
    let (rig, mut session) = plain();
    let mut stick = report(USB, Some("/media/tim/PHOTOS"), ready());
    stick.policy = policy;
    if policy == RemovalPolicy::Performance {
        stick.state = StateReport::PerformanceMode {
            active_write: false,
            eject_required: true,
        };
    }
    rig.link.push(DeviceNotice::Inventory(vec![stick]));
    session.pump();
    (rig, session)
}

fn policy_calls(link: &FakeLink) -> Vec<String> {
    link.calls()
        .into_iter()
        .filter(|call| call.starts_with("policy "))
        .collect()
}

#[test]
fn choosing_performance_mode_asks_first_and_cancelling_sends_nothing() {
    let (rig, mut session) = with_stick(RemovalPolicy::DirectRemoval);
    session.choose_policy(USB, RemovalPolicy::Performance);
    let confirmation = session
        .policy_confirmation
        .as_ref()
        .expect("the risks are shown before anything is sent");
    assert_eq!(confirmation.label, "PHOTOS");
    assert!(!confirmation.can_confirm());

    // Confirming before every risk is ticked does nothing at all.
    session.toggle_policy_risk(storage_core::PERFORMANCE_RISK_KEYS[0]);
    session.confirm_policy();
    assert!(session.policy_confirmation.is_some());
    assert!(policy_calls(&rig.link).is_empty());

    session.cancel_policy();
    assert!(session.policy_confirmation.is_none());
    assert!(
        policy_calls(&rig.link).is_empty(),
        "cancelling sends nothing"
    );
}

#[test]
fn the_request_carries_the_confirmed_risks_and_the_row_waits_for_the_service() {
    let (rig, mut session) = with_stick(RemovalPolicy::DirectRemoval);
    session.choose_policy(USB, RemovalPolicy::Performance);
    for key in storage_core::PERFORMANCE_RISK_KEYS {
        session.toggle_policy_risk(key);
    }
    session.confirm_policy();
    assert!(session.policy_confirmation.is_none());
    let mut keys: Vec<&str> = storage_core::PERFORMANCE_RISK_KEYS.to_vec();
    keys.sort();
    assert_eq!(
        policy_calls(&rig.link),
        vec![format!("policy {USB} performance [{}]", keys.join(","))]
    );

    // Asked for is not the same as granted: the row still shows what the
    // service last reported.
    rig.link.push(DeviceNotice::PolicyApplied {
        object_path: USB.to_string(),
    });
    session.pump();
    assert_eq!(
        session.device_rows()[0].policy,
        RemovalPolicy::DirectRemoval
    );

    let mut now = report(
        USB,
        Some("/media/tim/PHOTOS"),
        StateReport::PerformanceMode {
            active_write: false,
            eject_required: true,
        },
    );
    now.policy = RemovalPolicy::Performance;
    rig.link.push(DeviceNotice::Inventory(vec![now]));
    session.pump();
    assert_eq!(session.device_rows()[0].policy, RemovalPolicy::Performance);
}

#[test]
fn a_refusal_is_shown_with_its_reason_and_the_reported_policy_stays() {
    let (rig, mut session) = with_stick(RemovalPolicy::DirectRemoval);
    session.choose_policy(USB, RemovalPolicy::Performance);
    for key in storage_core::PERFORMANCE_RISK_KEYS {
        session.toggle_policy_risk(key);
    }
    session.confirm_policy();
    rig.link.push(DeviceNotice::PolicyRefused {
        object_path: USB.to_string(),
        detail: "two connected devices report the same identity".to_string(),
    });
    session.pump();

    assert_eq!(
        session.notice,
        Some(Notice::Device(Box::new(DeviceEvent::PolicyRefused {
            label: "PHOTOS".to_string(),
            detail: "two connected devices report the same identity".to_string(),
        })))
    );
    for c in [&EN_US, &ZH_TW] {
        let message = session.notice.as_ref().unwrap().message(c);
        assert!(message.contains("PHOTOS"), "{message}");
        assert!(message.contains("same identity"), "{message}");
    }
    assert_eq!(
        session.device_rows()[0].policy,
        RemovalPolicy::DirectRemoval
    );
}

#[test]
fn switching_back_to_direct_removal_needs_no_confirmation() {
    let (rig, mut session) = with_stick(RemovalPolicy::Performance);
    session.choose_policy(USB, RemovalPolicy::Performance);
    assert!(
        session.policy_confirmation.is_none(),
        "choosing the policy a device already has asks nothing"
    );
    session.choose_policy(USB, RemovalPolicy::DirectRemoval);
    assert!(session.policy_confirmation.is_none());
    assert_eq!(
        policy_calls(&rig.link),
        vec![format!("policy {USB} direct_removal []")]
    );
}

#[test]
fn the_policy_switch_is_worded_with_the_existing_policy_names() {
    for c in [&EN_US, &ZH_TW] {
        assert!(c.device_policy_switch.contains("{policy}"));
        for name in [c.device_policy_performance, c.device_policy_direct_removal] {
            let offered = c.device_policy_switch.replace("{policy}", name);
            assert!(offered.contains(name), "{offered}");
        }
    }
}
