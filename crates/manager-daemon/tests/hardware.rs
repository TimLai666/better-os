//! `org.betteros.Hardware1` over a private bus, against a fake sysfs root and
//! the fake authorizers (ticket 86). Nothing here touches the machine.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use hardware_platform::sysfs::SysRoot;
use manager_daemon::authorize::{
    Authorizer, CHARGE_LIMIT_ACTION, FAN_MODE_ACTION, FakeActionAuthorizer, FakeAuthorizer,
    FakeDismissingAuthorizer,
};
use manager_daemon::hardware::HardwareService;
use manager_daemon::service::{CallGate, OBJECT_PATH};

const NAME: &str = "org.betteros.Manager1HardwareTest";

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

/// A machine with the owner's two attributes: `BAT1`'s limit at 100 and the
/// `asus-nb-wmi` fan switch on automatic.
struct Machine {
    dir: tempfile::TempDir,
    limit: PathBuf,
    fan: PathBuf,
}

impl Machine {
    fn new() -> Machine {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path().join("sys");
        let bat = sys.join("class/power_supply/BAT1");
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("type"), "Battery\n").unwrap();
        std::fs::write(bat.join("capacity"), "87\n").unwrap();
        let limit = bat.join("charge_control_end_threshold");
        std::fs::write(&limit, "100\n").unwrap();
        let platform = sys.join("devices/platform/asus-nb-wmi");
        let chip = platform.join("hwmon/hwmon8");
        std::fs::create_dir_all(&chip).unwrap();
        std::fs::write(chip.join("name"), "asus\n").unwrap();
        let fan = chip.join("pwm1_enable");
        std::fs::write(&fan, "2\n").unwrap();
        std::os::unix::fs::symlink(&platform, chip.join("device")).unwrap();
        std::fs::create_dir_all(sys.join("class/hwmon")).unwrap();
        std::os::unix::fs::symlink(&chip, sys.join("class/hwmon/hwmon8")).unwrap();
        Machine { dir, limit, fan }
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("var/lib/better-os/hardware.json")
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap().trim().to_string()
    }
}

async fn serve<A: Authorizer + 'static>(
    bus: &PrivateBus,
    machine: &Machine,
    authorizer: A,
) -> zbus::Connection {
    let address: zbus::Address = bus.address.parse().unwrap();
    let service = HardwareService::new(
        authorizer,
        Arc::new(manager_daemon::hardware::DirectWriter),
        SysRoot::at(machine.dir.path()),
        Arc::new(CallGate::default()),
    )
    .with_state_file(machine.state());
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

async fn connect(bus: &PrivateBus) -> zbus::Connection {
    let address: zbus::Address = bus.address.parse().unwrap();
    zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap()
}

async fn set_limit(client: &zbus::Connection, percent: u8) -> Result<u8, String> {
    match client
        .call_method(
            Some(NAME),
            OBJECT_PATH,
            Some("org.betteros.Hardware1"),
            "SetChargeLimit",
            &(percent,),
        )
        .await
    {
        Ok(reply) => Ok(reply.body().deserialize::<u8>().unwrap()),
        Err(zbus::Error::MethodError(name, message, _)) => {
            Err(format!("{} {}", name.as_str(), message.unwrap_or_default()))
        }
        Err(other) => Err(other.to_string()),
    }
}

async fn set_fan(client: &zbus::Connection, mode: &str) -> Result<String, String> {
    match client
        .call_method(
            Some(NAME),
            OBJECT_PATH,
            Some("org.betteros.Hardware1"),
            "SetFanMode",
            &(mode,),
        )
        .await
    {
        Ok(reply) => Ok(reply.body().deserialize::<String>().unwrap()),
        Err(zbus::Error::MethodError(name, message, _)) => {
            Err(format!("{} {}", name.as_str(), message.unwrap_or_default()))
        }
        Err(other) => Err(other.to_string()),
    }
}

#[tokio::test]
async fn a_limit_outside_sixty_eighty_one_hundred_is_refused_before_anything_is_touched() {
    let bus = bus_or_skip!();
    let machine = Machine::new();
    // Granted, so only the argument check can be what refuses.
    let _server = serve(&bus, &machine, FakeAuthorizer(true)).await;
    let client = connect(&bus).await;
    for refused in [0u8, 50, 59, 61, 79, 90, 99, 101, 255] {
        let error = set_limit(&client, refused).await.unwrap_err();
        assert!(error.contains("InvalidArgs"), "{error}");
        assert!(error.contains("daemon.error.invalid_argument"), "{error}");
    }
    assert_eq!(Machine::read(&machine.limit), "100");
    assert!(!machine.state().exists());
}

#[tokio::test]
async fn a_fan_mode_other_than_auto_or_full_is_refused_before_anything_is_touched() {
    let bus = bus_or_skip!();
    let machine = Machine::new();
    let _server = serve(&bus, &machine, FakeAuthorizer(true)).await;
    let client = connect(&bus).await;
    for refused in ["", "manual", "Auto", "FULL", "1", "auto ", "../pwm1"] {
        let error = set_fan(&client, refused).await.unwrap_err();
        assert!(
            error.contains("daemon.error.invalid_argument"),
            "{refused:?}: {error}"
        );
    }
    assert_eq!(Machine::read(&machine.fan), "2");
}

#[tokio::test]
async fn without_polkit_nothing_is_written() {
    let bus = bus_or_skip!();
    let machine = Machine::new();
    let _server = serve(&bus, &machine, FakeAuthorizer(false)).await;
    let client = connect(&bus).await;
    let error = set_limit(&client, 80).await.unwrap_err();
    assert!(
        error.contains("AccessDenied") && error.contains("daemon.error.unauthorized"),
        "{error}"
    );
    let error = set_fan(&client, "full").await.unwrap_err();
    assert!(error.contains("daemon.error.unauthorized"), "{error}");
    assert_eq!(Machine::read(&machine.limit), "100");
    assert_eq!(Machine::read(&machine.fan), "2");
}

#[tokio::test]
async fn a_cancelled_prompt_leaves_the_value_and_says_so() {
    let bus = bus_or_skip!();
    let machine = Machine::new();
    let _server = serve(&bus, &machine, FakeDismissingAuthorizer).await;
    let client = connect(&bus).await;
    let error = set_limit(&client, 60).await.unwrap_err();
    assert!(error.contains("daemon.error.auth_cancelled"), "{error}");
    assert_eq!(Machine::read(&machine.limit), "100");
    // What the window makes of it: "Nothing changed: authentication wasn't
    // completed."
    let (name, message) = error.split_once(' ').unwrap();
    assert_eq!(
        hardware_core::write::classify_error(name, message),
        hardware_core::write::WriteOutcome::Cancelled
    );
}

#[tokio::test]
async fn each_method_asks_for_its_own_action() {
    let bus = bus_or_skip!();
    let machine = Machine::new();
    let _server = serve(&bus, &machine, FakeActionAuthorizer(CHARGE_LIMIT_ACTION)).await;
    let client = connect(&bus).await;
    assert_eq!(set_limit(&client, 80).await, Ok(80));
    assert!(
        set_fan(&client, "full")
            .await
            .unwrap_err()
            .contains("unauthorized")
    );
    drop(_server);

    let bus = bus_or_skip!();
    let _server = serve(&bus, &machine, FakeActionAuthorizer(FAN_MODE_ACTION)).await;
    let second = connect(&bus).await;
    let client = second;
    assert_eq!(set_fan(&client, "full").await, Ok("full".to_string()));
    assert!(
        set_limit(&client, 60)
            .await
            .unwrap_err()
            .contains("unauthorized")
    );
}

#[tokio::test]
async fn an_authorized_write_is_read_back_and_the_limit_is_kept_for_boot() {
    let bus = bus_or_skip!();
    let machine = Machine::new();
    let _server = serve(&bus, &machine, FakeAuthorizer(true)).await;
    let client = connect(&bus).await;
    assert_eq!(set_limit(&client, 80).await, Ok(80));
    assert_eq!(Machine::read(&machine.limit), "80");
    assert_eq!(Machine::read(&machine.state()), r#"{"charge_limit":80}"#);
    assert_eq!(set_fan(&client, "full").await, Ok("full".to_string()));
    assert_eq!(Machine::read(&machine.fan), "0");
    assert_eq!(set_fan(&client, "auto").await, Ok("auto".to_string()));
    assert_eq!(Machine::read(&machine.fan), "2");
}

#[tokio::test]
async fn a_machine_without_the_hardware_says_absent() {
    let bus = bus_or_skip!();
    let machine = Machine::new();
    std::fs::remove_file(&machine.limit).unwrap();
    std::fs::remove_file(&machine.fan).unwrap();
    let _server = serve(&bus, &machine, FakeAuthorizer(true)).await;
    let client = connect(&bus).await;
    assert!(
        set_limit(&client, 80)
            .await
            .unwrap_err()
            .contains("daemon.error.hardware_absent")
    );
    assert!(
        set_fan(&client, "auto")
            .await
            .unwrap_err()
            .contains("daemon.error.hardware_absent")
    );
}
