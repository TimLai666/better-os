//! Ticket 310: a folder on a server connects again when it is opened, from
//! every way of opening it, against a fake connector and a fake GVfs folder.
//! Nothing here reaches a bus, GVfs or a server; the window is not opened.
//!
//! The fake GVfs folder is a scratch directory: a mount's folder is there
//! while it is "mounted" and moved aside while it is not, which is what
//! GVfs's FUSE folder looks like to a `stat`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use files_core::{EntryId, Location};
use files_operations::{EngineConfig, JobEngine};
use files_platform::{MountTable, ReaderConfig, UserDirectories};

use crate::bookmarks::BookmarkStore;
use crate::network::{Answer, Asker, MountError, MountFailure, Mounter, Prompt, SignInFields};
use crate::prefs::{FilesPreferences, PreferenceStore};
use crate::reader::FilesReader;
use crate::remote::{FixedMounts, KnownMount, Mounted, Reason, Remote};
use crate::session::{FilesSession, LinkState, Notice, SessionSetup};

/// What the fake does when asked to connect.
#[derive(Clone, Debug)]
enum Script {
    Mount,
    Fail(MountFailure, &'static str),
    /// Waits until released, then mounts.
    Hold,
    /// Asks for "ann" / "secret" until it gets them, as GVfs does.
    SignIn,
}

/// A server, as GVfs would show it: its files live in `stash/<name>` while
/// it is not mounted and in `gvfs/<name>` while it is.
struct World {
    _root: tempfile::TempDir,
    home: PathBuf,
    gvfs: PathBuf,
    stash: PathBuf,
    mounts: FixedMounts,
    calls: Mutex<Vec<String>>,
    script: Mutex<Vec<Script>>,
    released: (Mutex<bool>, Condvar),
    /// Whether the servers' hosts answer (a look at the host).
    up: std::sync::atomic::AtomicBool,
    /// Which network this computer is on.
    network: Mutex<crate::remote::Network>,
}

/// The network the tests start on.
fn home_network() -> crate::remote::Network {
    crate::remote::Network {
        routes: "wlan0>0101A8C0".into(),
        connection: Some("home-wifi-uuid".into()),
        gateway_hardware: Some("wlan0>aa:bb:cc:00:00:01".into()),
    }
}

/// Another network with the same interface and gateway address (a café's
/// router at 192.168.1.1 too): another profile, another router.
fn cafe_network() -> crate::remote::Network {
    crate::remote::Network {
        routes: "wlan0>0101A8C0".into(),
        connection: Some("cafe-wifi-uuid".into()),
        gateway_hardware: Some("wlan0>aa:bb:cc:00:00:99".into()),
    }
}

impl World {
    fn name(address: &str) -> String {
        let remote = Remote::from_address(address).expect("an address");
        let key = remote.mount_key();
        let mut kind = String::new();
        let mut rest = Vec::new();
        for pair in key.split(',') {
            let (k, v) = pair.split_once('=').unwrap();
            if k == "type" {
                kind = v.to_string();
            } else {
                rest.push(format!("{k}={v}"));
            }
        }
        format!("{kind}:{}", rest.join(","))
    }

    /// A server with `folders` (paths from its root) in it, not mounted.
    fn server(&self, address: &str, folders: &[&str]) -> PathBuf {
        let stored = self.stash.join(Self::name(address));
        for folder in folders {
            fs::create_dir_all(stored.join(folder.trim_start_matches('/'))).unwrap();
        }
        fs::create_dir_all(stored.join("home/ann")).unwrap();
        self.gvfs.join(Self::name(address))
    }

    fn mount(&self, address: &str) -> Mounted {
        let name = Self::name(address);
        let shown = self.gvfs.join(&name);
        if !shown.exists() {
            fs::rename(self.stash.join(&name), &shown).unwrap();
        }
        let remote = Remote::from_gvfs_path(&shown, &self.gvfs).unwrap().at(None);
        let mut listed = self.mounts.0.lock().unwrap().clone().unwrap_or_default();
        listed.retain(|known| known.folder.as_deref() != Some(shown.as_path()));
        listed.push(KnownMount {
            remote,
            folder: Some(shown.clone()),
        });
        self.mounts.set(Some(listed));
        Mounted {
            root: shown.clone(),
            folder: shown.join("home/ann"),
        }
    }

    /// The connection goes: GVfs's folder for it disappears and its mount
    /// leaves the list.
    fn drop_connection(&self, address: &str) {
        let name = Self::name(address);
        let shown = self.gvfs.join(&name);
        if shown.exists() {
            fs::rename(&shown, self.stash.join(&name)).unwrap();
        }
        let mut listed = self.mounts.0.lock().unwrap().clone().unwrap_or_default();
        listed.retain(|known| known.folder.as_deref() != Some(shown.as_path()));
        self.mounts.set(Some(listed));
    }

    /// The server goes away: its mount goes, and its host stops answering.
    fn go_down(&self, address: &str) {
        self.up.store(false, std::sync::atomic::Ordering::Release);
        self.drop_connection(address);
    }

    /// The server answers again.
    fn come_back(&self) {
        self.up.store(true, std::sync::atomic::Ordering::Release);
    }

    /// This computer is on `network` now (`Network::default()`: none).
    fn join(&self, network: crate::remote::Network) {
        *self.network.lock().unwrap() = network;
    }

    fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }

    fn then(&self, steps: &[Script]) {
        let mut script = self.script.lock().unwrap();
        script.clear();
        script.extend(steps.iter().cloned().rev());
    }

    fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

/// The connector the session is given: it plays the world's script.
struct Fake(Arc<World>);

impl Mounter for Fake {
    fn mount(&self, address: &str, asker: &Asker) -> Result<PathBuf, MountError> {
        self.connect(address, asker).map(|mounted| mounted.folder)
    }

    fn connect(&self, address: &str, asker: &Asker) -> Result<Mounted, MountError> {
        let world = &self.0;
        world.calls.lock().unwrap().push(address.to_string());
        let step = world.script.lock().unwrap().pop().unwrap_or(Script::Mount);
        let up = world.up.load(std::sync::atomic::Ordering::Acquire);
        match step {
            // A server that is away refuses, as a stopped sshd does.
            Script::Mount if !up => Err(MountError::new(
                MountFailure::Refused,
                "Connection refused by server",
            )),
            Script::Mount => Ok(world.mount(address)),
            Script::Fail(failure, detail) => Err(MountError::new(failure, detail)),
            Script::Hold => {
                let mut released = world.released.0.lock().unwrap();
                while !*released {
                    let (next, timeout) = world
                        .released
                        .1
                        .wait_timeout(released, Duration::from_secs(10))
                        .unwrap();
                    released = next;
                    if timeout.timed_out() {
                        return Err(MountError::new(MountFailure::TimedOut, ""));
                    }
                }
                Ok(world.mount(address))
            }
            Script::SignIn => {
                let mut again = false;
                loop {
                    let answer = smol::block_on(asker.ask(Prompt::SignIn {
                        message: String::new(),
                        user: "ann".into(),
                        domain: String::new(),
                        fields: SignInFields::from_bits(1 | 2 | 8),
                        again,
                    }));
                    match answer {
                        Answer::SignIn { user, password, .. }
                            if user == "ann" && password.expose() == "secret" =>
                        {
                            return Ok(world.mount(address));
                        }
                        Answer::SignIn { .. } => again = true,
                        _ => return Err(MountError::new(MountFailure::Cancelled, "")),
                    }
                }
            }
        }
    }
}

fn world() -> Arc<World> {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let gvfs = root.path().join("run/gvfs");
    let stash = root.path().join("stash");
    for dir in [&home.join("Documents"), &gvfs, &stash] {
        fs::create_dir_all(dir).unwrap();
    }
    Arc::new(World {
        home,
        gvfs,
        stash,
        _root: root,
        mounts: FixedMounts::default(),
        calls: Mutex::new(Vec::new()),
        script: Mutex::new(Vec::new()),
        released: (Mutex::new(false), Condvar::new()),
        up: std::sync::atomic::AtomicBool::new(true),
        network: Mutex::new(home_network()),
    })
}

fn bookmarks_path(world: &World) -> PathBuf {
    world.home.join(".config/gtk-3.0/bookmarks")
}

fn session_at(world: &Arc<World>, start: Location) -> FilesSession {
    files_platform::WatchHub::isolate_this_thread();
    let engine = Arc::new(JobEngine::new(EngineConfig {
        store: None,
        displaced_trash: Some(world.home.join(".trash-for-tests")),
        ..EngineConfig::default()
    }));
    let reader = Arc::new(
        FilesReader::new(
            ReaderConfig::new(),
            None,
            crate::apps::CatalogHandle::empty(Default::default()),
        )
        .with_gvfs_root(Some(world.gvfs.clone())),
    );
    let mut session = FilesSession::new(SessionSetup {
        start,
        preferences: FilesPreferences::default(),
        preference_store: PreferenceStore::at_path(world.home.join("prefs.json")),
        view_store: crate::settings_store::ViewStateStore::at_path(world.home.join("views.json")),
        bookmark_store: BookmarkStore::at_path(bookmarks_path(world)),
        directories: UserDirectories::from_values(Some(&world.home), None),
        mounts: MountTable::new(Vec::new()),
        reader: reader.clone(),
        engine,
        catalog: reader.catalog().clone(),
        defaults: Box::new(crate::openwith::SessionDefaults::fixed([])),
        spawner: Box::new(app_catalog_platform::RecordingSpawner::new()),
        link: Box::new(crate::devices::NoDeviceLink),
        preview: crate::preview::PreviewPanel::default(),
    });
    // Before anything is pumped: the fake world, never the session bus.
    session.set_remote(
        Some(world.gvfs.clone()),
        Arc::new(world.mounts.clone()),
        Arc::new(Fake(Arc::clone(world))),
    );
    session.set_network_up(Arc::new(|| true));
    // The fake servers' hosts: there while the world says they are up.
    let up = Arc::clone(world);
    session.set_host_check(Arc::new(move |_| {
        if up.up.load(std::sync::atomic::Ordering::Acquire) {
            crate::remote::HostState::Reachable
        } else {
            crate::remote::HostState::Unreachable
        }
    }));
    let network = Arc::clone(world);
    session.set_network(Arc::new(move || network.network.lock().unwrap().clone()));
    session
}

fn session(world: &Arc<World>) -> FilesSession {
    let start = Location::local(world.home.join("Documents")).unwrap();
    session_at(world, start)
}

fn pump_until(session: &mut FilesSession, done: impl Fn(&FilesSession) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        session.pump();
        if done(session) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn listed(session: &FilesSession) -> Vec<String> {
    session
        .pane()
        .model()
        .iter_visible()
        .map(|entry| entry.name.clone())
        .collect()
}

/// The tab shows `folder`, listed, with no wait left.
fn shows(session: &FilesSession, folder: &Path) -> bool {
    session.location() == &Location::local(folder).unwrap()
        && session.link().is_none()
        && !session.is_listing()
        && matches!(
            session.pane().model().status(),
            files_core::ListingStatus::Complete
        )
}

const NAS: &str = "sftp://nas/";

#[test]
fn a_pinned_server_folder_connects_when_it_is_clicked() {
    let world = world();
    let shown = world.server(NAS, &["srv/site"]);
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    fs::write(bookmarks_path(&world), "sftp://nas/srv/site Site\n").unwrap();
    let mut session = session(&world);
    let favourite = session.bookmarks.get(0).unwrap().location().clone();
    assert_eq!(session.bookmarks.get(0).unwrap().display_name(), "Site");
    session.navigate_to(favourite);
    assert!(
        matches!(
            session.link().map(|link| &link.state),
            Some(LinkState::Connecting { again: false })
        ),
        "the tab says it is connecting at once"
    );
    assert!(pump_until(&mut session, |s| shows(
        s,
        &shown.join("srv/site")
    )));
    assert_eq!(world.calls(), 1);
    // The address GVfs was asked for is the mount's, never with a password.
    assert_eq!(world.calls.lock().unwrap()[0], "sftp://nas");
}

#[test]
fn a_favourite_an_earlier_version_stored_as_a_gvfs_folder_is_its_address_now() {
    let world = world();
    world.server("sftp://ann@nas:2222/", &["srv"]);
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    let old = format!(
        "file://{}/sftp:host=nas,port=2222,user=ann/srv Work\nfile:///home/ann/Music\n",
        world.gvfs.display()
    );
    fs::write(bookmarks_path(&world), &old).unwrap();
    let session = session(&world);
    assert_eq!(
        session.bookmarks.get(0).unwrap().location().to_uri(),
        "sftp://ann@nas:2222/srv"
    );
    // Written once in the new form; the label and the other lines stay.
    assert_eq!(
        fs::read_to_string(bookmarks_path(&world)).unwrap(),
        "sftp://ann@nas:2222/srv Work\nfile:///home/ann/Music\n"
    );
}

#[test]
fn a_gvfs_favourite_whose_mount_is_not_understood_asks_for_its_address_once() {
    let world = world();
    let odd = world.gvfs.join("dav+sd:host=Cloud._webdav._tcp.local/data");
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    fs::write(
        bookmarks_path(&world),
        format!("file://{} Data\n", odd.display()),
    )
    .unwrap();
    world.mounts.set(Some(Vec::new()));
    let shown = world.server(NAS, &["data"]);
    let mut session = session(&world);
    // Its line is left alone until an address is typed.
    assert!(matches!(
        session.bookmarks.get(0).unwrap().location(),
        Location::Local(_)
    ));
    session.navigate_to(Location::local(&odd).unwrap());
    assert!(pump_until(&mut session, |s| s
        .link()
        .is_some_and(|link| link.state == LinkState::NeedsAddress)));
    assert_eq!(world.calls(), 0, "nothing to connect to yet");
    session.submit_path(
        "sftp://nas/data",
        None,
        &crate::toolbar::FilesystemValidator,
    );
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("data"))));
    assert_eq!(
        fs::read_to_string(bookmarks_path(&world)).unwrap(),
        "sftp://nas/data Data\n",
        "the favourite keeps the address typed for it"
    );
}

/// Ticket 310's second review: the address typed on the Address-needed page
/// is of a server already mounted; the favourite still takes it.
#[test]
fn an_address_typed_for_a_server_already_mounted_still_rewrites_the_favourite() {
    let world = world();
    let odd = world.gvfs.join("dav+sd:host=Cloud._webdav._tcp.local/data");
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    fs::write(
        bookmarks_path(&world),
        format!("file://{} Data\n", odd.display()),
    )
    .unwrap();
    let shown = world.server(NAS, &["data"]);
    world.mount(NAS);
    let mut session = session(&world);
    session.navigate_to(Location::local(&odd).unwrap());
    assert!(pump_until(&mut session, |s| s
        .link()
        .is_some_and(|link| link.state == LinkState::NeedsAddress)));
    session.submit_path(
        "sftp://nas/data",
        None,
        &crate::toolbar::FilesystemValidator,
    );
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("data"))));
    assert_eq!(world.calls(), 0, "it was mounted already");
    assert_eq!(
        fs::read_to_string(bookmarks_path(&world)).unwrap(),
        "sftp://nas/data Data\n"
    );
}

#[test]
fn a_gtk_bookmark_another_program_wrote_is_a_favourite_that_connects() {
    let world = world();
    let shown = world.server("smb://nas/media", &["photos"]);
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    fs::write(bookmarks_path(&world), "smb://nas/media/photos\n").unwrap();
    let mut session = session(&world);
    let favourite = session.bookmarks.get(0).unwrap();
    assert_eq!(favourite.display_name(), "photos");
    let favourite = favourite.location().clone();
    session.navigate_to(favourite);
    assert!(pump_until(&mut session, |s| shows(
        s,
        &shown.join("photos")
    )));
}

#[test]
fn the_address_bar_connects_and_back_returns_to_where_it_was() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.submit_path("sftp://nas/srv", None, &crate::toolbar::FilesystemValidator);
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    session.go_back();
    assert_eq!(
        session.location(),
        &Location::local(world.home.join("Documents")).unwrap()
    );
}

#[test]
fn back_and_forward_onto_a_server_folder_connect_it_again() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    session.go_back();
    world.drop_connection(NAS);
    session.go_forward();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    assert_eq!(world.calls(), 2);
}

#[test]
fn a_tab_restored_at_start_and_after_a_restart_connects_and_keeps_its_selection() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    fs::write(world.stash.join(World::name(NAS)).join("srv/plan.txt"), "x").unwrap();
    world.mounts.set(Some(Vec::new()));
    // A window started on the folder (the last folder, a restored tab).
    let mut first = session_at(&world, Location::local(shown.join("srv")).unwrap());
    assert!(pump_until(&mut first, |s| shows(s, &shown.join("srv"))));
    assert_eq!(listed(&first), ["plan.txt"]);

    // An update restart hands the tab over; the connection is gone by then.
    let view = crate::restart::SessionView {
        tabs: vec![Location::local(shown.join("srv")).unwrap().to_uri()],
        active: 0,
        selected: vec!["plan.txt".into()],
        ..Default::default()
    };
    world.drop_connection(NAS);
    let mut restarted = session(&world);
    assert!(restarted.restore_view(&view));
    assert!(pump_until(&mut restarted, |s| shows(s, &shown.join("srv"))));
    assert!(
        restarted
            .pane()
            .model()
            .selection()
            .contains(&EntryId::Name("plan.txt".into()))
    );
}

#[test]
fn a_location_handed_in_on_the_command_line_or_over_the_bus_connects_first() {
    // Neither looks at the folder: a probe that is asked fails the test.
    struct NeverAsked;
    impl crate::launch::TargetProbe for NeverAsked {
        fn kind(&self, path: &Path) -> crate::launch::TargetKind {
            panic!("{} was looked at before the window opened", path.display())
        }
    }
    let request = crate::launch::StartRequest {
        target: Some("sftp://nas/srv".into()),
        ignored: 0,
    };
    let plan = crate::launch::plan_start(&request, None, &NeverAsked);
    let location = plan.location(Location::Recent);
    assert_eq!(location.to_uri(), "sftp://nas/srv");

    let items = crate::filemanager1::targets(&["sftp://nas/srv/plan.txt".into()], true);
    assert_eq!(
        items,
        [crate::launch::StartTarget::Remote {
            location: Location::parse_uri("sftp://nas/srv"),
            select: Some("plan.txt".into()),
        }]
    );

    let world = world();
    let shown = world.server(NAS, &["srv"]);
    fs::write(world.stash.join(World::name(NAS)).join("srv/plan.txt"), "x").unwrap();
    let mut session = session_at(&world, location);
    session.select_by_name("plan.txt");
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    assert!(
        session
            .pane()
            .model()
            .selection()
            .contains(&EntryId::Name("plan.txt".into())),
        "the file asked for is selected once it is there"
    );
}

#[test]
fn a_recent_file_on_a_server_that_is_away_opens_its_folder_connecting() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    fs::write(world.stash.join(World::name(NAS)).join("srv/plan.txt"), "x").unwrap();
    world.mounts.set(Some(Vec::new()));
    let mut session = session(&world);
    session.pump();
    assert!(pump_until(&mut session, |s| s
        .connected(&Location::parse_uri(NAS))
        == Some(false)));
    let entry = files_core::Entry::file(
        "plan.txt",
        files_core::LocalPath::new(shown.join("srv/plan.txt")).unwrap(),
        files_core::EntryKind::File,
    );
    session.open_entry_for_tests(&entry);
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    assert!(
        session
            .pane()
            .model()
            .selection()
            .contains(&EntryId::Name("plan.txt".into()))
    );
}

#[test]
fn a_paste_into_a_server_folder_waits_for_its_connection() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let source = world.home.join("Documents/report.txt");
    fs::write(&source, "q3").unwrap();
    let mut session = session(&world);
    session.clipboard =
        crate::commands::Clipboard::Copy(vec![files_core::LocalPath::new(&source).unwrap()]);
    world.then(&[Script::Hold]);
    session.mount_server("sftp://nas/srv".into());
    session.paste();
    assert!(session.notice.is_none(), "{:?}", session.notice);
    assert!(
        session.jobs.is_empty(),
        "nothing is copied before it is connected"
    );
    world.release();
    assert!(pump_until(&mut session, |_| shown
        .join("srv/report.txt")
        .exists()));
}

#[test]
fn a_tailscale_machine_opens_its_files_over_sftp_by_the_same_rule() {
    let world = world();
    let machine = crate::share::Machine {
        name: "laptop".into(),
        address: "laptop.tail0.ts.net".into(),
        online: true,
        os: String::new(),
    };
    let address = crate::share::sftp_address(&machine);
    let shown = world.server(&address, &[]);
    let mut session = session(&world);
    session.mount_server(address.clone());
    // No folder in the address: the server's own default folder.
    assert!(pump_until(&mut session, |s| shows(
        s,
        &shown.join("home/ann")
    )));
    assert_eq!(session.server_folder(&address), Some(shown.as_path()));
    world.drop_connection(&address);
    session.mount_server(address);
    assert!(pump_until(&mut session, |s| shows(
        s,
        &shown.join("home/ann")
    )));
}

#[test]
fn a_second_click_and_a_second_tab_join_the_attempt_in_flight() {
    let world = world();
    let shown = world.server(NAS, &["srv", "docs"]);
    world.then(&[Script::Hold]);
    let mut session = session(&world);
    let site = Location::parse_uri("sftp://nas/srv");
    session.navigate_to(site.clone());
    session.navigate_to(site.clone());
    let second = session.open_tab(Location::parse_uri("sftp://nas/docs"), false);
    for _ in 0..20 {
        session.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.link_of(second).is_some(), "the other tab waits too");
    assert_eq!(world.calls(), 1, "one connection for one server");
    world.release();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    session.activate_tab(second);
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("docs"))));
    assert_eq!(world.calls(), 1);
}

#[test]
fn the_window_stays_usable_and_other_tabs_are_not_held_while_it_connects() {
    let world = world();
    world.server(NAS, &["srv"]);
    world.then(&[Script::Hold]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    let other = session.open_tab(Location::local(world.home.join("Documents")).unwrap(), true);
    fs::write(world.home.join("Documents/one.txt"), "1").unwrap();
    session.reload();
    assert!(pump_until(&mut session, |s| s.active_tab() == other
        && !s.is_listing()
        && listed(s).contains(&"one.txt".to_string())));
    assert!(session.link().is_none(), "this tab is not waiting");
    assert!(session.is_mounting());
    world.release();
}

#[test]
fn cancel_leaves_not_connected_and_connect_tries_again() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    world.then(&[Script::Hold, Script::Mount]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    session.cancel_connecting();
    assert_eq!(
        session.link().map(|link| &link.state),
        Some(&LinkState::Cancelled)
    );
    assert!(!session.is_mounting());
    world.release();
    // What the cancelled attempt brings is not used.
    for _ in 0..20 {
        session.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        session.link().map(|link| &link.state),
        Some(&LinkState::Cancelled)
    );
    session.retry_connecting();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
}

#[test]
fn a_sign_in_is_asked_in_the_tab_that_waits_and_cancelling_it_is_a_cancel() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    world.then(&[Script::SignIn, Script::SignIn]);
    let mut session = session(&world);
    let first = session.active_tab();
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| s.server_prompt.is_some()));
    // Another tab in front: the question waits for its own tab.
    let other = session.open_tab(Location::local(world.home.join("Documents")).unwrap(), true);
    assert!(pump_until(&mut session, |s| s.server_prompt.is_none()));
    session.activate_tab(first);
    assert!(pump_until(&mut session, |s| s.server_prompt.is_some()));
    session.cancel_server_prompt();
    assert!(pump_until(&mut session, |s| s
        .link()
        .is_some_and(|link| link.state == LinkState::Cancelled)));
    session.retry_connecting();
    assert!(pump_until(&mut session, |s| s.server_prompt.is_some()));
    session.answer_server(Answer::SignIn {
        user: "ann".into(),
        domain: String::new(),
        password: crate::secret::Secret::new("secret"),
        anonymous: false,
        remember: crate::network::Remember::Forever,
    });
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    let _ = other;
}

#[test]
fn each_failure_says_why_and_try_again_connects() {
    let cases = [
        (MountFailure::TimedOut, "", true, Reason::NoAnswer),
        (MountFailure::Refused, "", true, Reason::Refused),
        (MountFailure::HostNotFound, "", true, Reason::NameNotFound),
        (
            MountFailure::PermissionDenied,
            "",
            true,
            Reason::SignInRefused,
        ),
        (MountFailure::ShareGone, "", true, Reason::ShareGone),
        (MountFailure::NetworkDown, "", true, Reason::NoNetwork),
        (MountFailure::TimedOut, "", false, Reason::NoNetwork),
        (
            MountFailure::HostKeyChanged,
            "Host key verification failed",
            true,
            Reason::Mount(MountFailure::HostKeyChanged),
        ),
    ];
    for (failure, detail, network, reason) in cases {
        let world = world();
        let shown = world.server(NAS, &["srv"]);
        world.then(&[Script::Fail(failure, detail), Script::Mount]);
        let mut session = session(&world);
        session.set_network_up(Arc::new(move || network));
        session.mount_server("sftp://nas/srv".into());
        assert!(
            pump_until(&mut session, |s| matches!(
                s.link().map(|link| &link.state),
                Some(LinkState::Failed { reason: shown, .. }) if *shown == reason
            )),
            "{failure:?} -> {reason:?}: {:?}",
            session.link().map(|link| &link.state)
        );
        session.retry_connecting();
        assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    }
}

#[test]
fn every_page_has_its_own_sentence_in_both_languages() {
    for c in [&crate::i18n::EN_US, &crate::i18n::ZH_TW] {
        let sentences = [
            c.remote_no_answer,
            c.remote_refused,
            c.remote_name_not_found,
            c.remote_sign_in_refused,
            c.remote_share_gone,
            c.remote_no_network,
        ];
        for (index, sentence) in sentences.iter().enumerate() {
            assert!(!sentence.is_empty());
            assert!(sentences[index + 1..].iter().all(|other| other != sentence));
            assert!(
                *sentence != c.server_failed,
                "never only \"cannot connect\""
            );
        }
        for title in [
            c.remote_connecting_title,
            c.remote_again_title,
            c.remote_cancelled_title,
            c.remote_failed_title,
        ] {
            assert!(title.contains("{server}"));
        }
    }
}

/// An address naming a folder the server does not have: the nearest folder
/// above it opens, and stays open (seen in the sandbox: the gone folder's
/// watch later said "gone" and the tab started connecting again).
#[test]
fn an_address_whose_folder_is_not_there_opens_the_folder_above_and_stays() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv/2025".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    assert!(matches!(
        session.notice,
        Some(Notice::RemoteFolderGone { .. })
    ));
    let until = Instant::now() + Duration::from_secs(4);
    while Instant::now() < until {
        session.pump();
        assert!(session.link().is_none(), "{:?}", session.link());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(shows(&session, &shown.join("srv")));
}

#[test]
fn a_folder_gone_on_a_server_that_connected_opens_the_nearest_folder_above() {
    let world = world();
    let shown = world.server(NAS, &["srv/old"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv/old".into());
    assert!(pump_until(&mut session, |s| shows(
        s,
        &shown.join("srv/old")
    )));
    session.go_back();
    fs::remove_dir(shown.join("srv/old")).unwrap();
    session.go_forward();
    assert!(
        pump_until(&mut session, |s| shows(s, &shown.join("srv"))),
        "{:?} at {:?} status {:?} notice {:?}",
        session.link(),
        session.location(),
        session.pane().model().status(),
        session.notice
    );
    assert_eq!(
        session.notice,
        Some(Notice::RemoteFolderGone {
            folder: "old".into(),
            server: "nas".into()
        })
    );
    assert_eq!(world.calls(), 1, "the server is there; nothing to connect");
}

#[test]
fn a_connection_that_drops_comes_back_to_the_same_folder_selection_and_scroll() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(
            world.stash.join(World::name(NAS)).join("srv").join(name),
            name,
        )
        .unwrap();
    }
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    session.select_by_name("b.txt");
    session.note_scroll(120.0);
    session.pump();

    world.go_down(NAS);
    assert!(
        pump_until(&mut session, |s| matches!(
            s.link().map(|link| &link.state),
            Some(LinkState::Connecting { again: true })
        )),
        "it says it is connecting again, in place: {:?}",
        session.link()
    );
    // The first look at the host is over: it was away.
    pump_for(&mut session, Duration::from_secs(1), |_| false);
    world.come_back();
    assert!(pump_for(&mut session, RETRY_WAIT, |s| shows(
        s,
        &shown.join("srv")
    ) && s
        .pane()
        .model()
        .visible_len()
        == 3));
    assert_eq!(
        session.pane().model().selection().cursor(),
        Some(&EntryId::Name("b.txt".into()))
    );
    assert_eq!(session.take_scroll_to_restore(), Some(120.0));
    assert!(
        session.pane().history().can_go_back(),
        "the tab's history is the same"
    );
}

/// How long a test waits for a retry: one interval and some.
const RETRY_WAIT: Duration = Duration::from_secs(12);

fn pump_for(
    session: &mut FilesSession,
    limit: Duration,
    done: impl Fn(&FilesSession) -> bool,
) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        session.pump();
        if done(session) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn a_dropped_connection_keeps_trying_until_the_server_is_back() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    world.go_down(NAS);
    // A look from here that does not reach the host cannot tell it is gone
    // (an alias, a jump host): GVfs itself is asked, once per 30 s.
    pump_for(&mut session, Duration::from_secs(7), |_| false);
    assert_eq!(
        world.calls(),
        2,
        "GVfs is asked once, not on every look at the host"
    );
    assert!(matches!(
        session.link().map(|link| &link.state),
        Some(LinkState::Connecting { again: true })
    ));
    world.come_back();
    assert!(
        pump_for(&mut session, RETRY_WAIT, |s| shows(s, &shown.join("srv"))),
        "it never came back: {:?}",
        session.link()
    );
    assert_eq!(world.calls(), 3);
}

/// Ticket 310's second review: GVfs says nothing that tells a mount let go
/// elsewhere from a server that went away, and Better Files unmounts no
/// server itself, so an open folder whose mount left comes back by itself,
/// as the owner asked.
#[test]
fn a_mount_let_go_elsewhere_comes_back_by_itself() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    // `gio mount -u` elsewhere: the mount leaves, the server still answers.
    world.drop_connection(NAS);
    assert!(pump_until(&mut session, |s| s.link().is_some()));
    assert!(pump_until(&mut session, |s| s.link().is_none()
        && shows(s, &shown.join("srv"))));
    assert_eq!(world.calls(), 2);
}

/// Ticket 310's second review: a Wi-Fi drop. The network goes (no default
/// route), the connection drops, and the same network comes back: the
/// folder comes back by itself, and the time with no network does not
/// count against the retry limit.
#[test]
fn a_network_that_goes_and_comes_back_reconnects_by_itself() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.set_retry_limit(Duration::from_secs(1));
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    world.join(crate::remote::Network::default());
    world.drop_connection(NAS);
    assert!(pump_until(&mut session, |s| s.link().is_some()));
    // Longer than the limit, with no network: nothing is tried, nothing
    // gives up.
    pump_for(&mut session, Duration::from_secs(3), |_| false);
    assert_eq!(world.calls(), 1, "nothing is sent with no network");
    assert!(matches!(
        session.link().map(|link| &link.state),
        Some(LinkState::Connecting { again: true })
    ));
    world.join(home_network());
    assert!(
        pump_for(&mut session, RETRY_WAIT, |s| shows(s, &shown.join("srv"))),
        "the same network came back and the folder did not: {:?}",
        session.link()
    );
    assert_eq!(world.calls(), 2);
}

/// Ticket 310's second review: a computer that slept. The network went
/// before the drop was noticed, and the server is there all along: when the
/// network the folder was open on is back, so is the folder.
#[test]
fn a_folder_comes_back_after_sleep_on_the_same_network() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    // Asleep: no network, and the mount is gone when it wakes.
    world.join(crate::remote::Network::default());
    pump_for(&mut session, Duration::from_secs(1), |_| false);
    world.drop_connection(NAS);
    assert!(pump_until(&mut session, |s| s.link().is_some()));
    pump_for(&mut session, Duration::from_secs(1), |_| false);
    assert_eq!(world.calls(), 1);
    // Awake: the same Wi-Fi again.
    world.join(home_network());
    assert!(pump_for(&mut session, RETRY_WAIT, |s| s.link().is_none()
        && shows(s, &shown.join("srv"))));
}

/// Ticket 310's second review: after a drop this computer is on another
/// network, here one with the same interface and gateway address but
/// another NetworkManager profile and router. Nothing is sent there by
/// itself; the page says why, and a click connects.
#[test]
fn a_drop_onto_another_network_waits_for_a_click() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    world.join(crate::remote::Network::default());
    world.drop_connection(NAS);
    assert!(pump_until(&mut session, |s| s.link().is_some()));
    world.join(cafe_network());
    assert!(pump_for(&mut session, RETRY_WAIT, |s| s
        .link()
        .is_some_and(|link| link.state == LinkState::OtherNetwork)));
    pump_for(&mut session, Duration::from_secs(6), |_| false);
    assert_eq!(world.calls(), 1, "nothing is tried on another network");
    // The favourite clicked, or Connect on the page: it connects.
    session.retry_connecting();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    assert_eq!(world.calls(), 2);
}

/// Ticket 310's review: retries stop at their limit (time with a network),
/// and then only a click tries again.
#[test]
fn retries_stop_at_their_limit() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.set_retry_limit(Duration::from_secs(1));
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    world.go_down(NAS);
    assert!(pump_for(&mut session, RETRY_WAIT, |s| s
        .link()
        .is_some_and(|link| matches!(
            link.state,
            LinkState::Failed {
                reason: Reason::NoAnswer,
                ..
            }
        ))));
    // One more interval: the limit is past, and it stops.
    pump_for(&mut session, RETRY_WAIT, |_| false);
    let calls = world.calls();
    world.come_back();
    pump_for(&mut session, RETRY_WAIT, |_| false);
    assert_eq!(
        world.calls(),
        calls,
        "past its limit only a click tries again"
    );
    session.retry_connecting();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
}

/// Ticket 310's review: a dropped connection whose server shows another host
/// key says so at once and is never tried again by itself.
#[test]
fn a_changed_host_key_after_a_drop_is_said_at_once_and_not_retried() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    world.then(&[Script::Fail(
        MountFailure::HostKeyChanged,
        "Host key verification failed",
    )]);
    // A restarted server with another key: it answers, and its key is not
    // the one known.
    world.drop_connection(NAS);
    assert!(pump_for(&mut session, RETRY_WAIT, |s| matches!(
        s.link().map(|link| &link.state),
        Some(LinkState::Failed {
            reason: Reason::Mount(MountFailure::HostKeyChanged),
            ..
        })
    )));
    let calls = world.calls();
    pump_for(&mut session, RETRY_WAIT, |_| false);
    assert_eq!(world.calls(), calls, "never tried again by itself");
}

/// A tracker that never answers.
struct Hanging;

impl crate::remote::MountsSource for Hanging {
    fn list(&self) -> Option<Vec<KnownMount>> {
        std::thread::sleep(Duration::from_secs(30));
        None
    }
}

#[test]
fn nothing_on_the_window_s_thread_waits_on_a_mount_list_or_a_folder_that_does_not_answer() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    fs::write(bookmarks_path(&world), "sftp://nas/srv\n").unwrap();
    let mut session = session(&world);
    session.set_remote(
        Some(world.gvfs.clone()),
        Arc::new(Hanging),
        Arc::new(Fake(Arc::clone(&world))),
    );
    for _ in 0..50 {
        let started = Instant::now();
        session.pump();
        let rows = crate::sidebar::build_rows(
            &crate::sidebar::SidebarInputs {
                directories: &session.directories,
                mounts: &session.mounts,
                bookmarks: &session.bookmarks,
                states: &crate::sidebar::NoDeviceStates,
                probe: &session.probes,
                remote: &session,
            },
            &crate::i18n::EN_US,
        );
        assert!(started.elapsed() < Duration::from_millis(200));
        let favourite = rows
            .iter()
            .find(|row| row.bookmark_index == Some(0))
            .unwrap();
        assert!(
            favourite.availability.is_available(),
            "a click still connects"
        );
    }
    // A folder whose probe is still out is known not to answer at once.
    let reader = FilesReader::new(
        ReaderConfig::new(),
        None,
        crate::apps::CatalogHandle::empty(Default::default()),
    )
    .with_gvfs_root(Some(world.gvfs.clone()))
    .with_probe_deadline(Duration::from_millis(300));
    world.mount(NAS);
    reader.hold_probe_for_tests(&shown);
    let started = Instant::now();
    let mut pane = files_core::Pane::open(
        Location::local(shown.join("srv")).unwrap(),
        files_core::ViewPreferences::default(),
        &reader,
    );
    while pane.is_listing() {
        assert!(started.elapsed() < Duration::from_secs(2));
        pane.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(
        pane.model().status(),
        files_core::ListingStatus::Failed(files_core::ListingError::NotAnswering { .. })
    ));
}

/// The address bar does not look at a server's folder on the window's
/// thread either: a typed path there is taken as a folder (the listing
/// connects and tells), and Tab completes nothing under GVfs's folder.
#[test]
fn the_address_bar_never_reads_a_server_s_folder_on_the_window_s_thread() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let gvfs = Some(world.gvfs.as_path());
    assert!(crate::toolbar::on_a_server_under(&shown.join("srv"), gvfs));
    assert!(!crate::toolbar::on_a_server_under(&world.gvfs, gvfs));
    assert!(!crate::toolbar::on_a_server_under(&world.home, gvfs));
    // Not mounted: the folder is not there to read, and is not read.
    let typed = format!("{}/s", shown.display());
    assert_eq!(
        crate::pathbar::complete_path_beside(&typed, None, gvfs),
        None
    );
    world.mount(NAS);
    assert_eq!(
        crate::pathbar::complete_path_beside(&typed, None, gvfs),
        None,
        "even mounted, a server's folder is not read here"
    );
    assert!(crate::pathbar::complete_path_beside(&typed, None, None).is_some());
}

#[test]
fn a_folder_that_stops_answering_says_connecting_again_and_lists_again() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    fs::write(shown.join("srv/a.txt"), "a").unwrap();
    fs::write(shown.join("srv/b.txt"), "b").unwrap();
    session.reload();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))
        && listed(s).len() == 2));
    session.select_by_name("b.txt");
    session.pump();
    session.hold_probe_for_tests(&shown);
    session.reload();
    assert!(pump_until(&mut session, |s| matches!(
        s.link().map(|link| &link.state),
        Some(LinkState::Connecting { again: true })
    )));
    session.release_probe_for_tests(&shown);
    let deadline = Instant::now() + crate::remote::RETRY_EVERY + Duration::from_secs(5);
    while !shows(&session, &shown.join("srv")) {
        assert!(Instant::now() < deadline, "it never listed again");
        session.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        world.calls(),
        1,
        "the mount was there; only the folder was asked again"
    );
    assert_eq!(
        session.pane().model().selection().cursor(),
        Some(&EntryId::Name("b.txt".into())),
        "the same selection as before it stopped answering"
    );
}

#[test]
fn a_server_folder_is_pinned_by_its_address_and_shows_whether_it_is_connected() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    session.pin_current();
    assert_eq!(
        fs::read_to_string(bookmarks_path(&world))
            .unwrap()
            .trim_end(),
        "sftp://nas/srv"
    );
    assert!(session.is_pinned(&Location::local(shown.join("srv")).unwrap()));
    let favourite = session.bookmarks.get(0).unwrap().location().clone();
    assert!(pump_until(&mut session, |s| s.connected(&favourite) == Some(true)));
    assert_eq!(
        session.shown_at(&favourite),
        Some(Location::local(shown.join("srv")).unwrap())
    );
    world.drop_connection(NAS);
    assert!(pump_until(&mut session, |s| s.connected(&favourite) == Some(false)));
}

fn copy_of(path: &Path) -> crate::commands::Clipboard {
    crate::commands::Clipboard::Copy(vec![files_core::LocalPath::new(path).unwrap()])
}

/// Ticket 310's review: a paste held while a folder stopped answering goes in
/// when its listing comes back, not lost with the wait.
#[test]
fn a_paste_held_while_a_folder_stopped_answering_goes_in_when_it_answers() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let source = world.home.join("Documents/report.txt");
    fs::write(&source, "q3").unwrap();
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    session.hold_probe_for_tests(&shown);
    session.reload();
    assert!(pump_until(&mut session, |s| s.link().is_some()));
    session.clipboard = copy_of(&source);
    session.paste();
    assert!(session.jobs.is_empty());
    session.release_probe_for_tests(&shown);
    assert!(
        pump_for(&mut session, RETRY_WAIT, |_| shown
            .join("srv/report.txt")
            .exists()),
        "the paste went in once the folder answered: {:?}",
        session.notice
    );
}

/// Ticket 310's review: a held paste whose folder was deleted on the server
/// while it waited is not done, nothing is made again, and the page says so.
#[test]
fn a_held_paste_into_a_folder_deleted_meanwhile_is_not_done_and_says_so() {
    let world = world();
    let shown = world.server(NAS, &["srv/drop"]);
    let source = world.home.join("Documents/report.txt");
    fs::write(&source, "q3").unwrap();
    let mut session = session(&world);
    world.then(&[Script::Hold]);
    session.mount_server("sftp://nas/srv/drop".into());
    session.clipboard = copy_of(&source);
    session.paste();
    fs::remove_dir(world.stash.join(World::name(NAS)).join("srv/drop")).unwrap();
    world.release();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    pump_for(&mut session, Duration::from_millis(500), |_| false);
    assert!(
        !shown.join("srv/drop").exists(),
        "the deleted folder is not made again"
    );
    assert!(session.jobs.is_empty(), "nothing was submitted");
    assert_eq!(
        session.notice,
        Some(Notice::PasteNotDone {
            folder: "drop".into(),
            server: "nas".into(),
            why: crate::session::PasteWhy::Gone,
        })
    );
}

/// Ticket 310's second review: a paste held for a folder the tab then
/// leaves says so at once, and is never run later by surprise.
#[test]
fn a_held_paste_whose_tab_goes_elsewhere_says_so_and_never_runs() {
    let world = world();
    let shown = world.server(NAS, &["srv/drop"]);
    let source = world.home.join("Documents/report.txt");
    fs::write(&source, "q3").unwrap();
    let mut session = session(&world);
    world.then(&[Script::Hold]);
    session.mount_server("sftp://nas/srv/drop".into());
    session.clipboard = copy_of(&source);
    session.paste();
    session.navigate_to(Location::local(world.home.join("Documents")).unwrap());
    session.pump();
    assert_eq!(
        session.notice,
        Some(Notice::PasteNotDone {
            folder: "drop".into(),
            server: "nas".into(),
            why: crate::session::PasteWhy::Left,
        })
    );
    world.release();
    session.mount_server("sftp://nas/srv/drop".into());
    assert!(pump_until(&mut session, |s| shows(
        s,
        &shown.join("srv/drop")
    )));
    pump_for(&mut session, Duration::from_millis(500), |_| false);
    assert!(session.jobs.is_empty(), "nothing was pasted later");
}

/// Ticket 310's second review: a paste whose connection came and whose
/// folder was not listed yet, when the connection is then cancelled, says
/// so and never runs on a later Connect.
#[test]
fn a_paste_waiting_for_its_listing_is_said_not_done_when_the_connection_is_cancelled() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let source = world.home.join("Documents/report.txt");
    fs::write(&source, "q3").unwrap();
    let mut session = session(&world);
    session.mount_server("sftp://nas/srv".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    // The folder stops answering, and a paste whose connection had come is
    // waiting for its listing (the state between landing and the listing,
    // set directly: its timing is GVfs's).
    session.hold_probe_for_tests(&shown);
    session.reload();
    assert!(pump_until(&mut session, |s| s.link().is_some()));
    let folder = session.location().clone();
    session.pend_paste_for_tests(&folder, copy_of(&source));
    assert!(session.jobs.is_empty());
    session.cancel_connecting();
    assert_eq!(
        session.notice,
        Some(Notice::PasteNotDone {
            folder: "srv".into(),
            server: "nas".into(),
            why: crate::session::PasteWhy::Cancelled,
        })
    );
    session.release_probe_for_tests(&shown);
    session.retry_connecting();
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))
        && s.link().is_none()));
    pump_for(&mut session, Duration::from_millis(500), |_| false);
    assert!(session.jobs.is_empty(), "nothing was pasted later");
}

/// Ticket 310's review: a held cut that lands leaves a newer clipboard alone.
#[test]
fn a_held_cut_leaves_a_newer_clipboard_alone() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let first = world.home.join("Documents/first.txt");
    let second = world.home.join("Documents/second.txt");
    fs::write(&first, "1").unwrap();
    fs::write(&second, "2").unwrap();
    let mut session = session(&world);
    world.then(&[Script::Hold]);
    session.mount_server("sftp://nas/srv".into());
    session.clipboard =
        crate::commands::Clipboard::Cut(vec![files_core::LocalPath::new(&first).unwrap()]);
    session.paste();
    session.clipboard = copy_of(&second);
    world.release();
    assert!(pump_until(&mut session, |_| shown
        .join("srv/first.txt")
        .exists()));
    assert_eq!(session.clipboard, copy_of(&second));
}

/// Ticket 310's review: a phone's favourite is not a server's. It never asks
/// for an address, keeps its availability, and the next connection in its
/// tab leaves the shared bookmarks file byte for byte as it was.
#[test]
fn a_device_favourite_is_never_taken_for_a_server_s() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    let phone = world.gvfs.join("mtp:host=Phone/DCIM");
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    let text = format!("file://{} Phone\nsftp://nas/srv\n", phone.display());
    fs::write(bookmarks_path(&world), &text).unwrap();
    world.mounts.set(Some(Vec::new()));
    let mut session = session(&world);
    let rows = crate::sidebar::build_rows(
        &crate::sidebar::SidebarInputs {
            directories: &session.directories,
            mounts: &session.mounts,
            bookmarks: &session.bookmarks,
            states: &crate::sidebar::NoDeviceStates,
            probe: &crate::sidebar::FixedProbe::default(),
            remote: &session,
        },
        &crate::i18n::EN_US,
    );
    let row = rows
        .iter()
        .find(|row| row.bookmark_index == Some(0))
        .unwrap();
    assert!(
        !row.availability.is_available(),
        "an unplugged phone is unavailable"
    );
    assert_eq!(row.connected, None);
    session.navigate_to(Location::local(&phone).unwrap());
    pump_for(&mut session, Duration::from_millis(500), |_| false);
    assert!(
        session.link().is_none(),
        "no address is asked for: {:?}",
        session.link()
    );
    // The next connection in that tab, from a favourite click.
    session.navigate_to(Location::parse_uri("sftp://nas/srv"));
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("srv"))));
    assert_eq!(fs::read_to_string(bookmarks_path(&world)).unwrap(), text);
}

/// Ticket 310's review: leaving the Address-needed page for any other
/// connection rewrites no favourite.
#[test]
fn only_an_address_typed_on_the_address_needed_page_rewrites_its_favourite() {
    let world = world();
    let odd = world.gvfs.join("dav+sd:host=Cloud._webdav._tcp.local/data");
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    let text = format!("file://{} Data\n", odd.display());
    fs::write(bookmarks_path(&world), &text).unwrap();
    world.mounts.set(Some(Vec::new()));
    let shown = world.server(NAS, &["data"]);
    let mut session = session(&world);
    session.navigate_to(Location::local(&odd).unwrap());
    assert!(pump_until(&mut session, |s| s
        .link()
        .is_some_and(|link| link.state == LinkState::NeedsAddress)));
    session.mount_server("sftp://nas/data".into());
    assert!(pump_until(&mut session, |s| shows(s, &shown.join("data"))));
    assert_eq!(fs::read_to_string(bookmarks_path(&world)).unwrap(), text);
}

/// Ticket 310's review: a password typed in an address reaches neither the
/// bookmarks file, the history, the restart's hand-off, nor the screen.
#[test]
fn a_typed_password_reaches_none_of_the_four_places() {
    let world = world();
    world.server("sftp://ann@nas", &["srv"]);
    world.then(&[Script::Hold]);
    let mut session = session(&world);
    session.submit_path(
        "sftp://ann:hunter2@nas/srv",
        None,
        &crate::toolbar::FilesystemValidator,
    );
    session.pin_current();
    session.pump();
    let bookmarks = fs::read_to_string(bookmarks_path(&world)).unwrap();
    assert!(!bookmarks.contains("hunter2"), "{bookmarks}");
    assert!(bookmarks.contains("sftp://ann@nas/srv"));
    let history: Vec<String> = session
        .pane()
        .history()
        .back_entries()
        .chain(std::iter::once(session.location()))
        .map(Location::to_uri)
        .collect();
    assert!(
        history.iter().all(|uri| !uri.contains("hunter2")),
        "{history:?}"
    );
    let handed = serde_json::to_string(&session.view()).unwrap();
    assert!(!handed.contains("hunter2"), "{handed}");
    let shown = session
        .link()
        .and_then(|link| link.remote.clone())
        .unwrap()
        .address();
    assert!(!shown.contains("hunter2"), "{shown}");
    assert!(
        session
            .path_text()
            .is_none_or(|text| !text.contains("hunter2"))
    );
    assert!(
        world
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|asked| !asked.contains("hunter2")),
        "nor is it sent: the server asks"
    );
    world.release();
}

/// Ticket 310's review: a favourite of a mount's root opens the root.
#[test]
fn a_mount_root_favourite_opens_the_root_not_the_home() {
    let world = world();
    let shown = world.server(NAS, &["srv"]);
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    fs::write(
        bookmarks_path(&world),
        format!("file://{}\n", shown.display()),
    )
    .unwrap();
    let mut session = session(&world);
    let favourite = session.bookmarks.get(0).unwrap().location().clone();
    assert_eq!(favourite.to_uri(), "sftp://nas/");
    session.navigate_to(favourite);
    assert!(pump_until(&mut session, |s| shows(s, &shown)));
}

/// Ticket 310's review: the bookmarks file is migrated once, only while it
/// still holds what was read, keeping its mode; a change another program
/// makes is never rewritten by Better Files on its own.
#[test]
fn the_bookmarks_file_is_migrated_once_and_only_as_it_was_read() {
    use std::os::unix::fs::PermissionsExt;
    let world = world();
    fs::create_dir_all(bookmarks_path(&world).parent().unwrap()).unwrap();
    let old = format!("file://{}/sftp:host=nas/srv\n", world.gvfs.display());
    fs::write(bookmarks_path(&world), &old).unwrap();
    fs::set_permissions(bookmarks_path(&world), fs::Permissions::from_mode(0o600)).unwrap();
    let mut session = session(&world);
    assert_eq!(
        fs::read_to_string(bookmarks_path(&world)).unwrap(),
        "sftp://nas/srv\n"
    );
    let mode = fs::metadata(bookmarks_path(&world))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "the file keeps its mode");
    // A write from a file read before another program's change is refused.
    let store = BookmarkStore::at_path(bookmarks_path(&world));
    let mut file = store.load();
    file.pin(&Location::local("/srv/x").unwrap());
    let theirs = format!(
        "sftp://nas/srv\nfile://{}/sftp:host=nas/other\n",
        world.gvfs.display()
    );
    fs::write(bookmarks_path(&world), &theirs).unwrap();
    assert!(
        !store.save_if_unchanged(&file, "sftp://nas/srv\n").unwrap(),
        "changed since it was read"
    );
    // Their old-style line is read as an address and left as they wrote it.
    pump_for(&mut session, Duration::from_millis(800), |_| false);
    assert_eq!(fs::read_to_string(bookmarks_path(&world)).unwrap(), theirs);
}
