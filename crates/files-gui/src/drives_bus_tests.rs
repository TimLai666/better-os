//! Ticket 122's drives against a fake UDisks2 on a private bus: a stick
//! appearing, mounting from its sidebar row, filling up, and ejecting; an
//! encrypted stick asking for its passphrase.
//!
//! The bus is a `dbus-daemon` of the test's own with no service files, and
//! the fake owns `org.freedesktop.UDisks2` on it, so nothing here reaches the
//! system bus, the machine's UDisks2 or any real drive. What is proven is the
//! production path, [`DriveSources::with_udisks`] and the window's session,
//! with only the bus, the mount table and the free-space read replaced.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use files_core::Location;
use files_operations::{EngineConfig, JobEngine};
use files_platform::{MountTable, ReaderConfig, UserDirectories};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::bookmarks::BookmarkStore;
use crate::drives::{Capacity, DriveKind, DriveSources};
use crate::prefs::{FilesPreferences, PreferenceStore};
use crate::reader::FilesReader;
use crate::session::{FilesSession, Notice, PendingDialog, SessionSetup};

const ROOT: &str = "/org/freedesktop/UDisks2";
const STICK: &str = "/org/freedesktop/UDisks2/block_devices/sdx1";
const STICK_DRIVE: &str = "/org/freedesktop/UDisks2/drives/Fake_Stick_SN1";
const VAULT: &str = "/org/freedesktop/UDisks2/block_devices/sdy1";
const VAULT_DRIVE: &str = "/org/freedesktop/UDisks2/drives/Fake_Vault_SN2";
const CLEARTEXT: &str = "/org/freedesktop/UDisks2/block_devices/dm_2d9";
const DISC: &str = "/org/freedesktop/UDisks2/block_devices/sr9";
const DISC_DRIVE: &str = "/org/freedesktop/UDisks2/drives/Fake_DVD_SN3";
const PASSPHRASE: &str = "fixture passphrase";
const GIB: u64 = 1024 * 1024 * 1024;

/// A private bus: no service directory, so nothing is ever activated.
struct PrivateBus {
    child: Child,
    address: String,
    directory: PathBuf,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        // One per test: the tests run side by side in one process.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "better-os-files-drives-bus-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).ok()?;
        let config = directory.join("bus.conf");
        fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:dir={}</listen>\
                 <auth>EXTERNAL</auth>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
                directory.display()
            ),
        )
        .ok()?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        use std::io::{BufRead, BufReader};
        let mut address = String::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = BufReader::new(stdout).read_line(&mut address);
        }
        let bus = Self {
            child,
            address: address.trim().to_string(),
            directory,
        };
        (!bus.address.is_empty()).then_some(bus)
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// What the fake was asked to do, in order.
type Calls = Arc<Mutex<Vec<String>>>;

fn path_bytes(path: &str) -> Vec<u8> {
    let mut bytes = path.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

struct FakeBlock {
    device: String,
    label: String,
    kind: String,
    size: u64,
    drive: String,
    backing: String,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl FakeBlock {
    #[zbus(property)]
    fn device(&self) -> Vec<u8> {
        path_bytes(&self.device)
    }
    #[zbus(property)]
    fn drive(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(self.drive.as_str()).expect("an object path")
    }
    #[zbus(property, name = "IdLabel")]
    fn id_label(&self) -> String {
        self.label.clone()
    }
    #[zbus(property, name = "IdType")]
    fn id_type(&self) -> String {
        self.kind.clone()
    }
    #[zbus(property, name = "IdUUID")]
    fn id_uuid(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn size(&self) -> u64 {
        self.size
    }
    #[zbus(property)]
    fn crypto_backing_device(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(self.backing.as_str()).expect("an object path")
    }
}

struct FakeFilesystem {
    /// Where a mount puts it.
    target: PathBuf,
    mounted: Option<PathBuf>,
    calls: Calls,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl FakeFilesystem {
    #[zbus(property)]
    fn mount_points(&self) -> Vec<Vec<u8>> {
        self.mounted
            .iter()
            .map(|path| path_bytes(&path.to_string_lossy()))
            .collect()
    }

    async fn mount(
        &mut self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        fs::create_dir_all(&self.target)
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        self.mounted = Some(self.target.clone());
        self.calls
            .lock()
            .unwrap()
            .push(format!("mount {}", emitter.path()));
        let _ = self.mount_points_changed(&emitter).await;
        Ok(self.target.to_string_lossy().into_owned())
    }

    async fn unmount(
        &mut self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if self.mounted.take().is_none() {
            return Err(zbus::fdo::Error::Failed("not mounted".into()));
        }
        self.calls
            .lock()
            .unwrap()
            .push(format!("unmount {}", emitter.path()));
        let _ = self.mount_points_changed(&emitter).await;
        Ok(())
    }
}

struct FakeDrive {
    calls: Calls,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl FakeDrive {
    #[zbus(property)]
    fn removable(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn media_removable(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn connection_bus(&self) -> String {
        "usb".into()
    }
    #[zbus(property)]
    fn ejectable(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_power_off(&self) -> bool {
        true
    }
    fn power_off(
        &self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("power-off {}", emitter.path()));
    }
    fn eject(
        &self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("tray {}", emitter.path()));
    }
}

struct FakeEncrypted {
    calls: Calls,
    root: PathBuf,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Encrypted")]
impl FakeEncrypted {
    async fn unlock(
        &self,
        passphrase: String,
        _options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        if passphrase != PASSPHRASE {
            self.calls.lock().unwrap().push("unlock refused".into());
            return Err(zbus::fdo::Error::AccessDenied("wrong passphrase".into()));
        }
        self.calls.lock().unwrap().push("unlock".into());
        server
            .at(
                CLEARTEXT,
                FakeBlock {
                    device: "/dev/dm-9".into(),
                    label: "Vault".into(),
                    kind: "ext4".into(),
                    size: 16 * GIB,
                    drive: "/".into(),
                    backing: VAULT.into(),
                },
            )
            .await?;
        server
            .at(
                CLEARTEXT,
                FakeFilesystem {
                    target: self.root.join("media/Vault"),
                    mounted: None,
                    calls: Arc::clone(&self.calls),
                },
            )
            .await?;
        Ok(OwnedObjectPath::try_from(CLEARTEXT).expect("an object path"))
    }

    fn lock(&self, _options: HashMap<String, OwnedValue>) {
        self.calls.lock().unwrap().push("lock".into());
    }
}

/// Owns `org.freedesktop.UDisks2` on the private bus, with nothing plugged
/// in yet.
fn fake_udisks(bus: &PrivateBus) -> zbus::Connection {
    smol::block_on(async {
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .serve_at(ROOT, zbus::fdo::ObjectManager)
            .unwrap()
            .name("org.freedesktop.UDisks2")
            .unwrap()
            .build()
            .await
            .expect("the fake UDisks2 is on the bus")
    })
}

fn plug_stick(udisks: &zbus::Connection, root: &Path, calls: &Calls) {
    smol::block_on(async {
        let server = udisks.object_server();
        server
            .at(
                STICK_DRIVE,
                FakeDrive {
                    calls: Arc::clone(calls),
                },
            )
            .await
            .unwrap();
        server
            .at(
                STICK,
                FakeBlock {
                    device: "/dev/sdx1".into(),
                    label: "FIELD DATA".into(),
                    kind: "exfat".into(),
                    size: 32 * GIB,
                    drive: STICK_DRIVE.into(),
                    backing: "/".into(),
                },
            )
            .await
            .unwrap();
        server
            .at(
                STICK,
                FakeFilesystem {
                    target: root.join("media/FIELD DATA"),
                    mounted: None,
                    calls: Arc::clone(calls),
                },
            )
            .await
            .unwrap();
    });
}

fn unplug(udisks: &zbus::Connection, block: &str, drive: &str) {
    smol::block_on(async {
        let server = udisks.object_server();
        let _ = server.remove::<FakeFilesystem, _>(block).await;
        let _ = server.remove::<FakeBlock, _>(block).await;
        let _ = server.remove::<FakeDrive, _>(drive).await;
    });
}

struct Fixture {
    root: tempfile::TempDir,
    /// What each mount point reads as, as `statvfs` would.
    space: Arc<Mutex<HashMap<PathBuf, Capacity>>>,
}

fn session_on(bus: &PrivateBus) -> (Fixture, FilesSession) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let engine = Arc::new(JobEngine::new(EngineConfig {
        store: None,
        displaced_trash: Some(root.path().join("Trash")),
        ..EngineConfig::default()
    }));
    let reader = Arc::new(FilesReader::new(
        ReaderConfig::new(),
        None,
        crate::apps::CatalogHandle::empty(Default::default()),
    ));
    let mut session = FilesSession::new(SessionSetup {
        start: Location::local(&home).unwrap(),
        preferences: FilesPreferences::default(),
        preference_store: PreferenceStore::at_path(root.path().join("prefs.json")),
        view_store: crate::settings_store::ViewStateStore::at_path(root.path().join("views.json")),
        bookmark_store: BookmarkStore::at_path(root.path().join("bookmarks")),
        directories: UserDirectories::from_values(Some(&home), None),
        mounts: MountTable::new(Vec::new()),
        reader: reader.clone(),
        engine,
        catalog: reader.catalog().clone(),
        defaults: Box::new(crate::openwith::SessionDefaults::fixed([])),
        spawner: Box::new(app_catalog_platform::RecordingSpawner::new()),
        link: Box::new(crate::devices::NoDeviceLink),
        preview: crate::preview::PreviewPanel::default(),
    });
    let space: Arc<Mutex<HashMap<PathBuf, Capacity>>> = Arc::default();
    let mut sources = DriveSources::with_udisks(Some(bus.address.clone()));
    // The machine's own mount table and GVfs folder stay out of it.
    sources.mounts = Arc::new(|| MountTable::new(Vec::new()));
    sources.gvfs = Arc::new(Vec::new);
    let read = Arc::clone(&space);
    sources.capacity = Arc::new(move |path| read.lock().unwrap().get(path).copied());
    session.set_drive_sources(sources);
    (Fixture { root, space }, session)
}

/// Pumps the session until `done` holds, or fails after a few seconds.
fn until(session: &mut FilesSession, what: &str, done: impl Fn(&FilesSession) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        session.pump();
        if done(session) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{what}: never happened; drives are {:#?}", session.drives());
}

fn stick(session: &FilesSession) -> Option<&crate::drives::Drive> {
    session.drives().iter().find(|drive| drive.key == STICK)
}

#[test]
fn a_stick_appears_mounts_fills_up_and_ejects() {
    let Some(bus) = PrivateBus::start() else {
        eprintln!("skipping: dbus-daemon is not available in this environment");
        return;
    };
    let udisks = fake_udisks(&bus);
    let calls: Calls = Arc::default();
    let (fixture, mut session) = session_on(&bus);
    session.pump();
    assert!(stick(&session).is_none());

    // Plugged in: UDisks2's InterfacesAdded alone brings it to the sidebar,
    // not mounted, named by its label, with its size and an Eject.
    plug_stick(&udisks, fixture.root.path(), &calls);
    until(&mut session, "the stick appears", |session| {
        stick(session).is_some()
    });
    let drive = stick(&session).unwrap().clone();
    assert_eq!(drive.kind, DriveKind::Removable);
    assert!(drive.ejectable);
    assert_eq!(drive.mount_point, None);
    assert_eq!(drive.capacity, None);
    let c = &crate::i18n::EN_US;
    assert_eq!(drive.name(c), "FIELD DATA");
    assert_eq!(drive.caption(c).as_deref(), Some("32.0 GB · Not mounted"));

    // Clicked: mounted through UDisks2 off the window's thread, and the
    // window goes there; the bar follows.
    let target = fixture.root.path().join("media/FIELD DATA");
    fixture.space.lock().unwrap().insert(
        target.clone(),
        Capacity {
            total: 32 * GIB,
            free: 20 * GIB,
        },
    );
    session.open_drive(STICK);
    until(&mut session, "the window opens the stick", |session| {
        session.location() == &Location::local(&target).unwrap()
    });
    until(&mut session, "the stick reads as mounted", |session| {
        stick(session).is_some_and(|drive| drive.capacity.is_some())
    });
    let drive = stick(&session).unwrap();
    assert_eq!(drive.mount_point.as_deref(), Some(target.as_path()));
    assert!(!drive.capacity.unwrap().is_nearly_full());
    assert_eq!(drive.caption(c).as_deref(), Some("20.0 GB free of 32.0 GB"));

    // Filled: the next read (a copy finishing asks for one) turns the bar
    // to the caution colour.
    fixture.space.lock().unwrap().insert(
        target.clone(),
        Capacity {
            total: 32 * GIB,
            free: GIB,
        },
    );
    session.refresh_drives();
    until(&mut session, "the stick reads as nearly full", |session| {
        stick(session).is_some_and(|drive| drive.capacity.is_some_and(|c| c.is_nearly_full()))
    });

    // Ejected: unmounted, then powered off; the person pulls it and it goes.
    session.eject_drive(STICK);
    until(&mut session, "the eject finishes", |session| {
        !session.is_drive_busy()
    });
    assert!(
        !matches!(session.notice, Some(Notice::DriveFailed { .. })),
        "{:?}",
        session.notice
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [
            format!("mount {STICK}"),
            format!("unmount {STICK}"),
            format!("power-off {STICK_DRIVE}"),
        ]
    );
    unplug(&udisks, STICK, STICK_DRIVE);
    until(&mut session, "the stick goes", |session| {
        stick(session).is_none()
    });
}

#[test]
fn an_encrypted_stick_asks_for_its_passphrase_and_locks_again_on_eject() {
    let Some(bus) = PrivateBus::start() else {
        eprintln!("skipping: dbus-daemon is not available in this environment");
        return;
    };
    let udisks = fake_udisks(&bus);
    let calls: Calls = Arc::default();
    let (fixture, mut session) = session_on(&bus);
    smol::block_on(async {
        let server = udisks.object_server();
        server
            .at(
                VAULT_DRIVE,
                FakeDrive {
                    calls: Arc::clone(&calls),
                },
            )
            .await
            .unwrap();
        server
            .at(
                VAULT,
                FakeBlock {
                    device: "/dev/sdy1".into(),
                    label: String::new(),
                    kind: "crypto_LUKS".into(),
                    size: 16 * GIB,
                    drive: VAULT_DRIVE.into(),
                    backing: "/".into(),
                },
            )
            .await
            .unwrap();
        server
            .at(
                VAULT,
                FakeEncrypted {
                    calls: Arc::clone(&calls),
                    root: fixture.root.path().to_path_buf(),
                },
            )
            .await
            .unwrap();
    });
    until(&mut session, "the locked stick appears", |session| {
        session
            .drives()
            .iter()
            .any(|drive| drive.key == VAULT && drive.locked)
    });

    // Clicked: the window asks for the passphrase instead of mounting.
    session.open_drive(VAULT);
    assert!(matches!(
        &session.dialog,
        Some(PendingDialog::UnlockDrive { key, .. }) if key == VAULT
    ));

    // A wrong one says so and asks again.
    session.dialog = None;
    session.unlock_drive(VAULT, "not it".into());
    until(&mut session, "a wrong passphrase is refused", |session| {
        !session.is_drive_busy()
    });
    assert!(matches!(session.notice, Some(Notice::DriveFailed { .. })));
    assert!(matches!(
        &session.dialog,
        Some(PendingDialog::UnlockDrive { key, .. }) if key == VAULT
    ));

    // The right one unlocks it, mounts what is inside, and goes there; the
    // sidebar shows the volume in place of the locked container.
    session.dialog = None;
    let target = fixture.root.path().join("media/Vault");
    session.unlock_drive(VAULT, PASSPHRASE.into());
    until(
        &mut session,
        "the window opens the unlocked stick",
        |session| session.location() == &Location::local(&target).unwrap(),
    );
    until(
        &mut session,
        "the volume replaces the container",
        |session| {
            session.drives().iter().any(|drive| drive.key == CLEARTEXT)
                && !session.drives().iter().any(|drive| drive.key == VAULT)
        },
    );
    let volume = session
        .drives()
        .iter()
        .find(|drive| drive.key == CLEARTEXT)
        .unwrap();
    assert_eq!(volume.kind, DriveKind::Removable);
    assert!(volume.ejectable);

    // Eject unmounts it, locks the container again and powers it off.
    session.eject_drive(CLEARTEXT);
    until(&mut session, "the eject finishes", |session| {
        !session.is_drive_busy()
    });
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "unlock refused".to_string(),
            "unlock".to_string(),
            format!("mount {CLEARTEXT}"),
            format!("unmount {CLEARTEXT}"),
            "lock".to_string(),
            format!("power-off {VAULT_DRIVE}"),
        ]
    );
}

#[test]
fn a_disc_is_ejected_from_its_tray_rather_than_powered_off() {
    let Some(bus) = PrivateBus::start() else {
        eprintln!("skipping: dbus-daemon is not available in this environment");
        return;
    };
    let udisks = fake_udisks(&bus);
    let calls: Calls = Arc::default();
    let (fixture, mut session) = session_on(&bus);
    let target = fixture.root.path().join("media/HOLIDAY");
    fs::create_dir_all(&target).unwrap();
    smol::block_on(async {
        let server = udisks.object_server();
        server
            .at(
                DISC_DRIVE,
                FakeDrive {
                    calls: Arc::clone(&calls),
                },
            )
            .await
            .unwrap();
        server
            .at(
                DISC,
                FakeBlock {
                    device: "/dev/sr9".into(),
                    label: "HOLIDAY".into(),
                    kind: "iso9660".into(),
                    size: 4 * GIB,
                    drive: DISC_DRIVE.into(),
                    backing: "/".into(),
                },
            )
            .await
            .unwrap();
        server
            .at(
                DISC,
                FakeFilesystem {
                    target: target.clone(),
                    mounted: Some(target.clone()),
                    calls: Arc::clone(&calls),
                },
            )
            .await
            .unwrap();
    });
    until(&mut session, "the disc appears", |session| {
        session
            .drives()
            .iter()
            .any(|drive| drive.key == DISC && drive.kind == DriveKind::Optical && drive.ejectable)
    });
    session.eject_drive(DISC);
    until(&mut session, "the eject finishes", |session| {
        !session.is_drive_busy()
    });
    assert_eq!(
        *calls.lock().unwrap(),
        [format!("unmount {DISC}"), format!("tray {DISC_DRIVE}")]
    );
}
