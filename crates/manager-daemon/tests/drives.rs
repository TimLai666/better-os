//! `org.betteros.Drives1` over a private bus, with a fake UDisks2 on the same
//! bus answering for the drives, fixture `/etc/fstab` and rules files, and
//! recording systemd units (ticket 127). Nothing here touches the machine.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use drive_mounts::client::{SetOutcome, set_mount_automatically};
use manager_daemon::authorize::{
    Authorizer, DRIVE_MOUNTING_ACTION, FakeActionAuthorizer, FakeAuthorizer,
    FakeDismissingAuthorizer,
};
use manager_daemon::drives::fake::RecordingUnits;
use manager_daemon::drives::{DrivePaths, DrivesService, UdisksResolver};
use manager_daemon::service::{CallGate, OBJECT_PATH};
use zbus::zvariant::OwnedObjectPath;

const NAME: &str = "org.betteros.Manager1";

struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        use std::io::{BufRead, BufReader};
        let mut address = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One block device as UDisks2 shows it.
#[derive(Clone)]
struct FakeBlock {
    uuid: String,
    filesystem: String,
    label: String,
    device: String,
    drive: OwnedObjectPath,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl FakeBlock {
    #[zbus(property, name = "IdUUID")]
    fn id_uuid(&self) -> String {
        self.uuid.clone()
    }
    #[zbus(property, name = "IdType")]
    fn id_type(&self) -> String {
        self.filesystem.clone()
    }
    #[zbus(property, name = "IdUsage")]
    fn id_usage(&self) -> String {
        "filesystem".into()
    }
    #[zbus(property, name = "IdLabel")]
    fn id_label(&self) -> String {
        self.label.clone()
    }
    #[zbus(property, name = "Device")]
    fn device(&self) -> Vec<u8> {
        let mut bytes = self.device.clone().into_bytes();
        bytes.push(0);
        bytes
    }
    #[zbus(property, name = "Symlinks")]
    fn symlinks(&self) -> Vec<Vec<u8>> {
        vec![format!("/dev/disk/by-uuid/{}\0", self.uuid).into_bytes()]
    }
    #[zbus(property, name = "Drive")]
    fn drive(&self) -> OwnedObjectPath {
        self.drive.clone()
    }
}

struct FakeFilesystem {
    mounted: Vec<String>,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl FakeFilesystem {
    #[zbus(property, name = "MountPoints")]
    fn mount_points(&self) -> Vec<Vec<u8>> {
        self.mounted
            .iter()
            .map(|p| format!("{p}\0").into_bytes())
            .collect()
    }
}

struct FakeDrive {
    removable: bool,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl FakeDrive {
    #[zbus(property, name = "Removable")]
    fn removable(&self) -> bool {
        self.removable
    }
    #[zbus(property, name = "MediaRemovable")]
    fn media_removable(&self) -> bool {
        false
    }
}

struct FakeManager {
    blocks: Vec<(String, OwnedObjectPath)>,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Manager")]
impl FakeManager {
    fn resolve_device(
        &self,
        devspec: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        _options: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Vec<OwnedObjectPath> {
        let Some(uuid) = devspec
            .get("uuid")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        else {
            return Vec::new();
        };
        self.blocks
            .iter()
            .filter(|(known, _)| *known == uuid)
            .map(|(_, path)| path.clone())
            .collect()
    }
}

async fn fake_udisks(bus: &PrivateBus) -> zbus::Connection {
    let path = |p: &str| OwnedObjectPath::try_from(p).unwrap();
    let nvme = path("/org/freedesktop/UDisks2/drives/nvme");
    let usb = path("/org/freedesktop/UDisks2/drives/usb");
    let windows = FakeBlock {
        uuid: "01D9F00DBEEF1234".into(),
        filesystem: "ntfs".into(),
        label: "Windows".into(),
        device: "/dev/nvme0n1p3".into(),
        drive: nvme.clone(),
    };
    let stick = FakeBlock {
        uuid: "AB12-CD34".into(),
        filesystem: "exfat".into(),
        label: "STICK".into(),
        device: "/dev/sdb1".into(),
        drive: usb.clone(),
    };
    let backup = FakeBlock {
        uuid: "BACC-0001".into(),
        filesystem: "ext4".into(),
        label: "Backup".into(),
        device: "/dev/nvme0n1p4".into(),
        drive: nvme.clone(),
    };
    let manager = FakeManager {
        blocks: vec![
            (windows.uuid.clone(), path("/org/freedesktop/UDisks2/block_devices/nvme0n1p3")),
            (stick.uuid.clone(), path("/org/freedesktop/UDisks2/block_devices/sdb1")),
            (backup.uuid.clone(), path("/org/freedesktop/UDisks2/block_devices/nvme0n1p4")),
        ],
    };
    let address: zbus::Address = bus.address.parse().unwrap();
    zbus::connection::Builder::address(address)
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2/Manager", manager)
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2/block_devices/nvme0n1p3", windows)
        .unwrap()
        .serve_at(
            "/org/freedesktop/UDisks2/block_devices/nvme0n1p3",
            FakeFilesystem { mounted: vec![] },
        )
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2/block_devices/sdb1", stick)
        .unwrap()
        .serve_at(
            "/org/freedesktop/UDisks2/block_devices/sdb1",
            FakeFilesystem {
                mounted: vec!["/media/me/STICK".into()],
            },
        )
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2/block_devices/nvme0n1p4", backup)
        .unwrap()
        .serve_at(nvme.as_str(), FakeDrive { removable: false })
        .unwrap()
        .serve_at(usb.as_str(), FakeDrive { removable: true })
        .unwrap()
        .name("org.freedesktop.UDisks2")
        .unwrap()
        .build()
        .await
        .unwrap()
}

struct Machine {
    _dir: tempfile::TempDir,
    paths: DrivePaths,
}

const SYSTEM: &str = "UUID=1111-2222 / ext4 errors=remount-ro 0 1\n\
/dev/nvme0n1p4 /backup ext4 defaults 0 2\n";

impl Machine {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("etc/udev/rules.d")).unwrap();
        std::fs::write(dir.path().join("etc/fstab"), SYSTEM).unwrap();
        let paths = DrivePaths {
            fstab: dir.path().join("etc/fstab"),
            rules: dir.path().join("etc/udev/rules.d/90-better-os-drives.rules"),
            mount_root: dir.path().to_path_buf(),
        };
        Self { _dir: dir, paths }
    }
    fn fstab(&self) -> String {
        std::fs::read_to_string(&self.paths.fstab).unwrap()
    }
    fn rules(&self) -> Option<String> {
        std::fs::read_to_string(&self.paths.rules).ok()
    }
}

async fn serve<A: Authorizer + 'static>(
    bus: &PrivateBus,
    machine: &Machine,
    authorizer: A,
    units: Arc<RecordingUnits>,
) -> zbus::Connection {
    let address = bus.address.clone();
    let udisks = tokio::task::spawn_blocking(move || {
        zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .unwrap()
    })
    .await
    .unwrap();
    let service = DrivesService::new(
        authorizer,
        Arc::new(UdisksResolver { connection: udisks }),
        units,
        machine.paths.clone(),
        Arc::new(CallGate::default()),
    );
    let address: zbus::Address = bus.address.parse().unwrap();
    zbus::connection::Builder::address(address)
        .unwrap()
        .serve_at(OBJECT_PATH, service)
        .unwrap()
        .name(NAME)
        .unwrap()
        .build()
        .await
        .unwrap()
}

async fn call(bus: &PrivateBus, uuid: &'static str, on: bool) -> SetOutcome {
    let address = bus.address.clone();
    tokio::task::spawn_blocking(move || {
        let connection = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .unwrap();
        set_mount_automatically(&connection, uuid, on)
    })
    .await
    .unwrap()
}

macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None => {
                eprintln!("skipping: dbus-daemon is not available in this environment");
                return;
            }
        }
    };
}

#[tokio::test(flavor = "multi_thread")]
async fn an_internal_drive_is_found_through_udisks_and_written_into_fstab() {
    let bus = bus_or_skip!();
    let _udisks = fake_udisks(&bus).await;
    let machine = Machine::new();
    let units = Arc::new(RecordingUnits::default());
    let _server = serve(&bus, &machine, FakeAuthorizer(true), units.clone()).await;

    assert_eq!(
        call(&bus, "01D9F00DBEEF1234", true).await,
        SetOutcome::Done {
            mount_point: "/mnt/better-os/01D9F00DBEEF1234".into()
        }
    );
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let written = machine.fstab();
    assert!(written.starts_with(SYSTEM));
    assert!(
        written.contains(&format!(
            "UUID=01D9F00DBEEF1234 /mnt/better-os/01D9F00DBEEF1234 ntfs nofail,x-gvfs-show,x-systemd.device-timeout=10s,uid={uid},"
        )),
        "{written}"
    );
    assert_eq!(
        units.calls(),
        ["reload", "start mnt-better\\x2dos-01D9F00DBEEF1234.mount"]
    );
    assert_eq!(
        call(&bus, "01D9F00DBEEF1234", false).await,
        SetOutcome::Done {
            mount_point: String::new()
        }
    );
    assert_eq!(machine.fstab(), SYSTEM);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removable_drive_is_a_rule_and_an_administrator_s_line_is_left_alone() {
    let bus = bus_or_skip!();
    let _udisks = fake_udisks(&bus).await;
    let machine = Machine::new();
    let units = Arc::new(RecordingUnits::default());
    let _server = serve(&bus, &machine, FakeAuthorizer(true), units.clone()).await;

    assert_eq!(
        call(&bus, "AB12-CD34", false).await,
        SetOutcome::Done {
            mount_point: String::new()
        }
    );
    assert!(machine.rules().unwrap().contains("AB12-CD34"));
    assert_eq!(machine.fstab(), SYSTEM);
    // /dev/nvme0n1p4 is in the administrator's own line.
    assert_eq!(
        call(&bus, "BACC-0001", true).await,
        SetOutcome::Refused("system_table".into())
    );
    assert_eq!(call(&bus, "FFFF-0000", true).await, SetOutcome::Absent);
    assert!(matches!(
        call(&bus, "x; rm -rf /", true).await,
        SetOutcome::Failed(message) if message.contains("invalid_argument:uuid")
    ));
    assert_eq!(machine.fstab(), SYSTEM);
}

#[tokio::test(flavor = "multi_thread")]
async fn polkit_decides_before_anything_is_looked_up() {
    let bus = bus_or_skip!();
    let _udisks = fake_udisks(&bus).await;
    let machine = Machine::new();
    let units = Arc::new(RecordingUnits::default());
    {
        let _server = serve(&bus, &machine, FakeAuthorizer(false), units.clone()).await;
        assert_eq!(
            call(&bus, "01D9F00DBEEF1234", true).await,
            SetOutcome::NotAllowed
        );
    }
    {
        let _server = serve(&bus, &machine, FakeDismissingAuthorizer, units.clone()).await;
        assert_eq!(
            call(&bus, "01D9F00DBEEF1234", true).await,
            SetOutcome::Cancelled
        );
    }
    {
        // Another action's grant is not this one's.
        let _server = serve(
            &bus,
            &machine,
            FakeActionAuthorizer("org.betteros.manager.set-fan-mode"),
            units.clone(),
        )
        .await;
        assert_eq!(
            call(&bus, "01D9F00DBEEF1234", true).await,
            SetOutcome::NotAllowed
        );
    }
    assert_eq!(machine.fstab(), SYSTEM);
    assert!(units.calls().is_empty());
    let _server = serve(
        &bus,
        &machine,
        FakeActionAuthorizer(DRIVE_MOUNTING_ACTION),
        units.clone(),
    )
    .await;
    assert!(matches!(
        call(&bus, "01D9F00DBEEF1234", true).await,
        SetOutcome::Done { .. }
    ));
}
