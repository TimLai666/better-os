//! Stand-ins for what the System page talks to, on private buses, so a
//! sandbox desktop can show every state without touching the machine:
//!
//!   hardware_standin session
//!     gsd-power's screen and keyboard on $DBUS_SESSION_BUS_ADDRESS.
//!   hardware_standin system ADDRESS grant|deny|dismiss [lap]
//!     power-profiles-daemon, UPower, and this daemon's Hardware1 (writing
//!     into a temporary copy of a battery and a fan switch) on ADDRESS.
//!
//!     With `BETTER_FILES_TEST_DRIVES`, `BETTER_OS_DEBUG_FSTAB` and
//!     `BETTER_OS_DEBUG_DRIVE_RULES` set, also this daemon's Drives1 (ticket
//!     127), answering for the fixture's drives and writing the two fixture
//!     files the windows read.
//!
//! Runs until killed. Built only with the `test-support` feature.

use std::sync::Arc;

use hardware_platform::fakes::{self, FakeProfiles};
use hardware_platform::sysfs::SysRoot;
use manager_daemon::authorize::{FakeAuthorizer, FakeDismissingAuthorizer};
use manager_daemon::hardware::{DirectWriter, HardwareService};
use manager_daemon::service::{CallGate, OBJECT_PATH};

fn stand_in_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let sys = dir.path().join("sys");
    let bat = sys.join("class/power_supply/BAT1");
    std::fs::create_dir_all(&bat).unwrap();
    std::fs::write(bat.join("type"), "Battery\n").unwrap();
    std::fs::write(bat.join("capacity"), "87\n").unwrap();
    std::fs::write(bat.join("charge_control_end_threshold"), "100\n").unwrap();
    let platform = sys.join("devices/platform/asus-nb-wmi");
    let chip = platform.join("hwmon/hwmon8");
    std::fs::create_dir_all(&chip).unwrap();
    std::fs::write(chip.join("name"), "asus\n").unwrap();
    std::fs::write(chip.join("pwm1_enable"), "2\n").unwrap();
    std::os::unix::fs::symlink(&platform, chip.join("device")).unwrap();
    std::fs::create_dir_all(sys.join("class/hwmon")).unwrap();
    std::os::unix::fs::symlink(&chip, sys.join("class/hwmon/hwmon8")).unwrap();
    dir
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("session") => {
            let builder = zbus::blocking::connection::Builder::session().unwrap();
            let _connection = fakes::serve_session(builder, 60, 33, 4).unwrap();
            std::future::pending::<()>().await;
        }
        Some("system") => {
            let address = args.get(1).expect("a bus address");
            let auth = args.get(2).map(String::as_str).unwrap_or("dismiss");
            let mut profiles = FakeProfiles::owners_machine();
            if args.get(3).map(String::as_str) == Some("lap") {
                profiles.active = "performance".into();
                profiles.degraded = "lap-detected".into();
                profiles.holds = vec![("GNOME Boxes".into(), "performance".into())];
            }
            let builder = zbus::blocking::connection::Builder::address(address.as_str()).unwrap();
            let _fakes = fakes::serve_system(builder, profiles, true, false).unwrap();
            let root = stand_in_root();
            let gate = Arc::new(CallGate::default());
            let parsed: zbus::Address = address.parse().unwrap();
            let builder = zbus::connection::Builder::address(parsed).unwrap();
            let sys = SysRoot::at(root.path());
            let _daemon = match auth {
                "grant" => builder
                    .serve_at(
                        OBJECT_PATH,
                        HardwareService::new(
                            FakeAuthorizer(true),
                            Arc::new(DirectWriter),
                            sys,
                            gate,
                        ),
                    )
                    .unwrap(),
                "deny" => builder
                    .serve_at(
                        OBJECT_PATH,
                        HardwareService::new(
                            FakeAuthorizer(false),
                            Arc::new(DirectWriter),
                            sys,
                            gate,
                        ),
                    )
                    .unwrap(),
                _ => builder
                    .serve_at(
                        OBJECT_PATH,
                        HardwareService::new(
                            FakeDismissingAuthorizer,
                            Arc::new(DirectWriter),
                            sys,
                            gate,
                        ),
                    )
                    .unwrap(),
            }
            .serve_at(OBJECT_PATH, drives_standin(auth))
            .unwrap()
            .name("org.betteros.Manager1")
            .unwrap()
            .build()
            .await
            .unwrap();
            eprintln!("stand-ins ready on {address} ({auth})");
            std::future::pending::<()>().await;
        }
        _ => {
            eprintln!("usage: hardware_standin session | system ADDRESS grant|deny|dismiss [lap]");
            std::process::exit(2);
        }
    }
}

/// The fixture's drives, as UDisks2 would describe them to the daemon.
#[derive(serde::Deserialize)]
struct FixtureDrive {
    uuid: Option<String>,
    kind: String,
    label: Option<String>,
    mount_point: Option<String>,
    filesystem: Option<String>,
}

fn drives_standin(
    auth: &str,
) -> manager_daemon::drives::DrivesService<manager_daemon::authorize::FakeAuthorizer> {
    use manager_daemon::drives::fake::{RecordingUnits, TableResolver};
    use manager_daemon::drives::{DrivePaths, DrivesService, ResolvedDrive};
    let var = |name: &str| std::env::var_os(name).map(std::path::PathBuf::from);
    let fixture: Vec<FixtureDrive> = var("BETTER_FILES_TEST_DRIVES")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let mut table = std::collections::HashMap::new();
    for drive in fixture {
        let Some(uuid) = drive.uuid else { continue };
        table.insert(
            uuid.clone(),
            ResolvedDrive {
                filesystem: drive.filesystem.unwrap_or_else(|| "ext4".into()),
                usage: "filesystem".into(),
                removable: matches!(drive.kind.as_str(), "removable" | "optical"),
                mount_points: drive.mount_point.into_iter().map(Into::into).collect(),
                names: drive_mounts::fstab::DriveNames {
                    uuid,
                    label: drive.label,
                    ..Default::default()
                },
            },
        );
    }
    let scratch = std::env::temp_dir();
    let paths = DrivePaths {
        fstab: var("BETTER_OS_DEBUG_FSTAB").unwrap_or_else(|| scratch.join("fstab")),
        rules: var("BETTER_OS_DEBUG_DRIVE_RULES").unwrap_or_else(|| scratch.join("drive.rules")),
        lock: scratch.join("better-os-fstab.lock"),
        mount_root: scratch,
    };
    DrivesService::new(
        manager_daemon::authorize::FakeAuthorizer(auth == "grant"),
        Arc::new(TableResolver(table)),
        Arc::new(RecordingUnits::default()),
        paths,
        Arc::new(CallGate::default()),
    )
}
