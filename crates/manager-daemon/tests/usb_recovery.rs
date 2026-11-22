//! The USB-C recovery (ticket 101, ADR 0046) against a fake machine whose
//! kernel log is an excerpt from the ticket, and `org.betteros.UsbRecovery1`
//! over a private bus. Nothing here loads a module, writes sysfs, or reads
//! this machine's journal.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hardware_core::usb_recovery::{
    self as core, MachineModel, PciAddress, RecoveryReply, Refusal, Restored, UsbCSupply,
    UsbChoice, UsbFindings, UsbPresent, UsbRecoverySettings, Worker, XhciFunction,
};
use manager_daemon::DaemonError;
use manager_daemon::authorize::{
    Authorizer, FakeActionAuthorizer, FakeAuthorizer, USB_C_RELOAD_ACTION,
    USB_CONTROLLER_REBIND_ACTION, USB_RECOVERY_SETTINGS_ACTION,
};
use manager_daemon::service::{CallGate, OBJECT_PATH};
use manager_daemon::usb_recovery::{
    self as recovery, SleepWorld, Timings, UsbMachine, UsbRecoveryService,
};

const UCSI_LOG: &str = "\
ucsi_acpi USBC000:00: ucsi_handle_connector_change: GET_CONNECTOR_STATUS failed (-110)
PM: suspend entry (s2idle)
PM: suspend exit
ucsi_acpi USBC000:00: failed to re-enable notifications (-110)
";

const DIED_LOG: &str = "\
PM: suspend entry (s2idle)
xhci_hcd 0000:63:00.4: AMD-Vi: Event logged [IO_PAGE_FAULT domain=0x0014 address=0x0 flags=0x0000]
xhci_hcd 0000:63:00.4: WARN: xHC restore state timeout
xhci_hcd 0000:63:00.4: PCI post-resume error -110!
xhci_hcd 0000:63:00.4: HC died; cleaning up
PM: suspend exit
";

const BLOCKED: &str = "\
PM: suspend entry (s2idle)
Freezing remaining freezable tasks failed after 20.004 seconds (0 tasks refusing to freeze, wq_busy=1):
workqueue usb_hub_wq: flags=0x104
PM: suspend exit
PM: suspend entry (s2idle)
Freezing remaining freezable tasks failed after 20.000 seconds (0 tasks refusing to freeze, wq_busy=1):
workqueue usb_hub_wq: flags=0x104
PM: suspend exit
";

fn address() -> PciAddress {
    PciAddress::parse("0000:63:00.4").unwrap()
}

/// The owner's M5406WA, with the interface failing (one charger offline)
/// and 63:00.4 dead (no root hubs).
fn broken(board: &str) -> UsbPresent {
    UsbPresent {
        model: MachineModel {
            board_name: Some(board.into()),
            ..MachineModel::default()
        },
        typec_ports: 1,
        supplies: vec![UsbCSupply {
            name: "ucsi-source-psy-USBC000:001".into(),
            online: Some(false),
        }],
        mains_online: Some(true),
        controllers: vec![XhciFunction {
            address: address(),
            vendor: 0x1022,
            device: 0x151e,
            driver: Some("xhci_hcd".into()),
            root_hubs: 0,
            devices: 0,
        }],
    }
}

struct Fake {
    log: Mutex<String>,
    present: Mutex<UsbPresent>,
    workers: Vec<Worker>,
    reloads: Mutex<u32>,
    rebinds: Mutex<Vec<PciAddress>>,
    /// What a reload brings back: both ports and the charger, or one port.
    reload_restores: bool,
    /// Both ports come back and the charger does not.
    charger_stays_hidden: bool,
    /// The helper fails.
    reload_fails: bool,
    /// How many times the journal was read.
    reads: Mutex<u32>,
    /// What the kernel says about loading the module file.
    module_ready: Result<(), String>,
}

impl Fake {
    fn new(log: &str, present: UsbPresent) -> Fake {
        Fake {
            log: Mutex::new(log.to_string()),
            present: Mutex::new(present),
            workers: Vec::new(),
            reloads: Mutex::new(0),
            rebinds: Mutex::new(Vec::new()),
            reload_restores: true,
            charger_stays_hidden: false,
            reload_fails: false,
            reads: Mutex::new(0),
            module_ready: Ok(()),
        }
    }
}

impl UsbMachine for Fake {
    fn kernel_log(&self) -> Result<String, DaemonError> {
        *self.reads.lock().unwrap() += 1;
        // As journalctl prints it: JSON, the marker from the daemon's
        // stream and everything else from the kernel.
        Ok(core::as_journal(&self.log.lock().unwrap()))
    }
    fn present(&self) -> UsbPresent {
        self.present.lock().unwrap().clone()
    }
    fn workers(&self) -> Vec<Worker> {
        self.workers.clone()
    }
    fn pause(&self, _duration: Duration) {}
    fn ucsi_module_ready(&self) -> Result<(), String> {
        self.module_ready.clone()
    }
    fn reload_ucsi(&self) -> Result<(), DaemonError> {
        *self.reloads.lock().unwrap() += 1;
        if self.reload_fails {
            return Err(DaemonError::UsbRecoveryFailed("helper failed".into()));
        }
        let mut present = self.present.lock().unwrap();
        if self.reload_restores {
            present.typec_ports = 2;
            present.supplies[0].online = Some(!self.charger_stays_hidden);
        }
        Ok(())
    }
    fn rebind(&self, address: &PciAddress) -> Result<(), DaemonError> {
        self.rebinds.lock().unwrap().push(address.clone());
        let mut present = self.present.lock().unwrap();
        present.controllers[0].root_hubs = 2;
        Ok(())
    }
    fn mark(&self, line: &str) {
        let mut log = self.log.lock().unwrap();
        log.push_str(line);
        log.push('\n');
    }
}

#[test]
fn a_failing_interface_is_reloaded_verified_and_marked() {
    let fake = Fake::new(UCSI_LOG, broken("M5406WA"));
    let restored = recovery::reload(&fake, Timings::INSTANT).unwrap();
    assert!(restored.complete(), "{restored:?}");
    assert_eq!(*fake.reloads.lock().unwrap(), 1);
    assert!(
        fake.log
            .lock()
            .unwrap()
            .ends_with("usb-recovery: ucsi_acpi reloaded\n")
    );
    // Marked: nothing is failing now, and a second reload is refused.
    assert_eq!(
        recovery::reload(&fake, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::NothingToReset))
    );
    assert_eq!(*fake.reloads.lock().unwrap(), 1);
}

#[test]
fn a_reload_that_did_not_bring_the_ports_back_is_said_and_not_marked() {
    let mut fake = Fake::new(UCSI_LOG, broken("M5406WA"));
    fake.reload_restores = false;
    let restored = recovery::reload(&fake, Timings::INSTANT).unwrap();
    assert_eq!(
        restored,
        Restored::Ucsi {
            ports: 1,
            expected_ports: 2,
            charger: core::ChargerShown::NotShown
        }
    );
    assert!(!fake.log.lock().unwrap().contains("reloaded"));
}

#[test]
fn nothing_runs_on_a_machine_that_is_not_affected_or_not_failing() {
    let other = Fake::new(UCSI_LOG, broken("UX3405MA"));
    assert_eq!(
        recovery::reload(&other, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::NotAffected))
    );
    assert_eq!(
        recovery::rebind(&other, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::NotAffected))
    );
    let healthy = Fake::new(
        "PM: suspend entry (s2idle)\nPM: suspend exit\n",
        broken("M5406WA"),
    );
    assert_eq!(
        recovery::reload(&healthy, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::NothingToReset))
    );
    assert_eq!(
        recovery::rebind(&healthy, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::NothingToReset))
    );
    assert_eq!(
        *other.reloads.lock().unwrap() + *healthy.reloads.lock().unwrap(),
        0
    );
    assert!(other.rebinds.lock().unwrap().is_empty());
}

#[test]
fn the_dead_controller_is_rebound_by_the_address_the_log_names() {
    let fake = Fake::new(DIED_LOG, broken("M5406WA"));
    let restored = recovery::rebind(&fake, Timings::INSTANT).unwrap();
    assert!(restored.complete());
    assert_eq!(*fake.rebinds.lock().unwrap(), vec![address()]);
    assert!(
        fake.log
            .lock()
            .unwrap()
            .ends_with("usb-recovery: controller rebound 0000:63:00.4\n")
    );
}

#[test]
fn stuck_hub_work_is_a_restart_and_nothing_is_unbound() {
    let mut fake = Fake::new(DIED_LOG, broken("M5406WA"));
    fake.workers = vec![Worker {
        pid: 1115178,
        comm: "kworker/12:2+usb_hub_wq".into(),
        state: 'D',
    }];
    assert_eq!(
        recovery::rebind(&fake, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::RestartNeeded))
    );
    let logged = Fake::new(&format!("{DIED_LOG}{BLOCKED}"), broken("M5406WA"));
    assert_eq!(
        recovery::rebind(&logged, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::RestartNeeded))
    );
    assert!(fake.rebinds.lock().unwrap().is_empty());
    assert!(logged.rebinds.lock().unwrap().is_empty());
}

#[derive(Default)]
struct World {
    lid_closed: Option<bool>,
    docked: Option<bool>,
    blocked: bool,
    busy: bool,
    others: bool,
    lingering: bool,
    /// The daemon starts a transaction while the notifications go out.
    busy_after_notify: bool,
    became_busy: Mutex<bool>,
    notified: Mutex<Vec<u32>>,
    powered_off: Mutex<u32>,
}

impl SleepWorld for World {
    fn lid_closed(&self) -> Option<bool> {
        self.lid_closed
    }
    fn docked(&self) -> Option<bool> {
        self.docked
    }
    fn shutdown_blocked(&self) -> bool {
        self.blocked
    }
    fn daemon_busy(&self) -> bool {
        self.busy || *self.became_busy.lock().unwrap()
    }
    fn lingering(&self) -> bool {
        self.lingering
    }
    fn people(&self) -> (Vec<u32>, bool) {
        (vec![1000], self.others)
    }
    fn notify(&self, uid: u32) {
        self.notified.lock().unwrap().push(uid);
        if self.busy_after_notify {
            *self.became_busy.lock().unwrap() = true;
        }
    }
    fn power_off(&self) -> Result<(), String> {
        *self.powered_off.lock().unwrap() += 1;
        Ok(())
    }
}

fn settings(dir: &Path, choices: &[UsbChoice]) -> PathBuf {
    let path = dir.join("usb-recovery.json");
    for choice in choices {
        recovery::set_choice(&path, *choice, true).unwrap();
    }
    path
}

#[test]
fn after_a_sleep_nothing_happens_until_the_person_turned_it_on() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(&format!("{UCSI_LOG}{DIED_LOG}{BLOCKED}"), broken("M5406WA"));
    let world = World {
        lid_closed: Some(true),
        ..World::default()
    };
    let done = recovery::after_sleep(
        &fake,
        &world,
        &dir.path().join("usb-recovery.json"),
        &dir.path().join("run.json"),
        Timings::INSTANT,
    );
    assert_eq!(done, recovery::AfterSleep::default());
    assert_eq!(*fake.reloads.lock().unwrap(), 0);
    assert!(world.notified.lock().unwrap().is_empty());
}

#[test]
fn after_a_sleep_the_interface_is_reloaded_when_it_failed() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings(dir.path(), &[UsbChoice::UcsiAutomatic]);
    let fake = Fake::new(UCSI_LOG, broken("M5406WA"));
    let done = recovery::after_sleep(
        &fake,
        &World::default(),
        &path,
        &dir.path().join("run.json"),
        Timings::INSTANT,
    );
    assert!(matches!(done.reload, Some(Ok(ref restored)) if restored.complete()));
    assert_eq!(done.rebind, None);
}

#[test]
fn the_overnight_loop_is_told_once_and_powered_off_only_when_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().join("run.json");
    let path = settings(dir.path(), &[UsbChoice::ControllerAutomatic]);
    let fake = Fake::new(&format!("{DIED_LOG}{BLOCKED}"), broken("M5406WA"));
    let world = World {
        lid_closed: Some(true),
        docked: Some(false),
        ..World::default()
    };
    let done = recovery::after_sleep(&fake, &world, &path, &run, Timings::INSTANT);
    assert_eq!(
        done.rebind,
        Some(Err(DaemonError::UsbRefused(Refusal::RestartNeeded)))
    );
    assert_eq!(done.notified, vec![1000]);
    assert!(!done.powered_off, "notify only is the default");
    // The next failed attempt in the same run tells nobody again.
    let again = recovery::after_sleep(&fake, &world, &path, &run, Timings::INSTANT);
    assert!(again.notified.is_empty());
    assert_eq!(*world.notified.lock().unwrap(), vec![1000]);

    // Chosen: off with the lid closed, once.
    recovery::set_choice(&path, UsbChoice::PowerOffWhenSleepBlocked, true).unwrap();
    let off = recovery::after_sleep(&fake, &world, &path, &run, Timings::INSTANT);
    assert!(off.powered_off);
    let twice = recovery::after_sleep(&fake, &world, &path, &run, Timings::INSTANT);
    assert!(!twice.powered_off);
    assert_eq!(*world.powered_off.lock().unwrap(), 1);

    // Never with the lid open, another session open, a block held, docked
    // or not known to be undocked, or the daemon in a transaction.
    let closed = || World {
        lid_closed: Some(true),
        docked: Some(false),
        ..World::default()
    };
    for world in [
        World {
            lid_closed: Some(false),
            ..closed()
        },
        World::default(),
        World {
            others: true,
            ..closed()
        },
        World {
            blocked: true,
            ..closed()
        },
        World {
            docked: Some(true),
            ..closed()
        },
        World {
            docked: None,
            ..closed()
        },
        World {
            busy: true,
            ..closed()
        },
        // An account lingering with no session (its services run).
        World {
            lingering: true,
            ..closed()
        },
    ] {
        let run = dir.path().join("fresh.json");
        let _ = std::fs::remove_file(&run);
        let done = recovery::after_sleep(&fake, &world, &path, &run, Timings::INSTANT);
        assert!(!done.powered_off);
        assert_eq!(*world.powered_off.lock().unwrap(), 0);
    }
}

#[test]
fn the_rebind_helper_writes_only_unbind_and_bind_for_the_affected_kind() {
    let dir = tempfile::tempdir().unwrap();
    let device = dir.path().join("sys/bus/pci/devices/0000:63:00.4");
    std::fs::create_dir_all(&device).unwrap();
    for (file, value) in [
        ("vendor", "0x1022"),
        ("device", "0x151e"),
        ("class", "0x0c0330"),
    ] {
        std::fs::write(device.join(file), value).unwrap();
    }
    let driver = dir.path().join("sys/bus/pci/drivers/xhci_hcd");
    std::fs::create_dir_all(&driver).unwrap();
    std::fs::write(driver.join("unbind"), "").unwrap();
    std::fs::write(driver.join("bind"), "").unwrap();
    assert_eq!(
        recovery::rebind_xhci_main(dir.path(), "0000:63:00.4"),
        Ok("rebound".to_string())
    );
    assert_eq!(
        std::fs::read_to_string(driver.join("unbind")).unwrap(),
        "0000:63:00.4"
    );
    assert_eq!(
        std::fs::read_to_string(driver.join("bind")).unwrap(),
        "0000:63:00.4"
    );

    std::fs::write(driver.join("unbind"), "").unwrap();
    std::fs::write(device.join("device"), "0x151f").unwrap();
    assert!(recovery::rebind_xhci_main(dir.path(), "0000:63:00.4").is_err());
    assert!(recovery::rebind_xhci_main(dir.path(), "../../../etc").is_err());
    assert!(recovery::rebind_xhci_main(dir.path(), "0000:63:00.5").is_err());
    assert_eq!(std::fs::read_to_string(driver.join("unbind")).unwrap(), "");
}

#[test]
fn the_printed_helper_properties_are_the_daemons_own() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_better-manager-daemon"))
            .args(args)
            .output()
            .unwrap()
    };
    let lines = |out: std::process::Output| -> Vec<String> {
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    };
    assert_eq!(
        lines(run(&["--helper-properties", "ucsi-reload"])),
        recovery::ucsi_helper_properties()
    );
    assert_eq!(
        lines(run(&["--helper-properties", "xhci-rebind", "0000:63:00.4"])),
        recovery::xhci_helper_properties(&address())
    );
    assert_eq!(
        lines(run(&["--helper-properties", "after-sleep"])),
        recovery::after_sleep_properties()
    );
    for refused in [
        &["--helper-properties", "xhci-rebind", "/sys/../x"][..],
        &["--reload-ucsi", "x"][..],
        &["--rebind-xhci"][..],
        &["--notify-sleep-blocked", "fr"][..],
    ] {
        assert!(!run(refused).status.success(), "{refused:?}");
    }
    // The hook never fails a sleep, whatever it is given. (Only arguments
    // it refuses before reading anything: a valid pair would read this
    // machine's settings.)
    for args in [
        &["--after-sleep", "post", "reboot"][..],
        &["--after-sleep", "nonsense"][..],
    ] {
        assert!(run(args).status.success(), "{args:?}");
    }
}

// --- Over a private bus ---------------------------------------------------

const NAME: &str = "org.betteros.Manager1UsbTest";

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
        test_prerequisites::require!(
            "dbus-daemon",
            PrivateBus::start().ok_or("a private dbus-daemon did not start")
        )
    };
}

async fn serve<A: Authorizer + 'static>(
    bus: &PrivateBus,
    fake: Arc<Fake>,
    settings: &Path,
    authorizer: A,
) -> zbus::Connection {
    serve_fresh(bus, fake, settings, authorizer, recovery::FINDINGS_FRESH).await
}

async fn serve_fresh<A: Authorizer + 'static>(
    bus: &PrivateBus,
    fake: Arc<Fake>,
    settings: &Path,
    authorizer: A,
    fresh: Duration,
) -> zbus::Connection {
    let address: zbus::Address = bus.address.parse().unwrap();
    let service =
        UsbRecoveryService::new(authorizer, fake, settings, Arc::new(CallGate::default()))
            .with_timings(Timings::INSTANT)
            .with_findings_fresh(fresh);
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

async fn call<B>(bus: &PrivateBus, method: &str, body: &B) -> Result<String, String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let address: zbus::Address = bus.address.parse().unwrap();
    let client = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    match client
        .call_method(
            Some(NAME),
            OBJECT_PATH,
            Some("org.betteros.UsbRecovery1"),
            method,
            body,
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

#[tokio::test(flavor = "multi_thread")]
async fn each_method_asks_for_its_own_action() {
    let bus = bus_or_skip!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usb-recovery.json");
    let fake = Arc::new(Fake::new(
        &format!("{UCSI_LOG}{DIED_LOG}"),
        broken("M5406WA"),
    ));
    let _server = serve(
        &bus,
        fake.clone(),
        &path,
        FakeActionAuthorizer(USB_C_RELOAD_ACTION),
    )
    .await;
    assert!(call(&bus, "ReloadUcsi", &()).await.is_ok());
    assert!(
        call(&bus, "RebindController", &())
            .await
            .unwrap_err()
            .contains("unauthorized")
    );
    assert!(
        call(&bus, "SetChoice", &("ucsi-automatic", true))
            .await
            .unwrap_err()
            .contains("unauthorized")
    );
    assert!(fake.rebinds.lock().unwrap().is_empty());
    assert!(!path.exists());
    drop(_server);

    for (action, method) in [
        (USB_CONTROLLER_REBIND_ACTION, "RebindController"),
        (USB_RECOVERY_SETTINGS_ACTION, "SetChoice"),
    ] {
        let fake = Arc::new(Fake::new(
            &format!("{UCSI_LOG}{DIED_LOG}"),
            broken("M5406WA"),
        ));
        let _server = serve(&bus, fake.clone(), &path, FakeActionAuthorizer(action)).await;
        assert!(
            call(&bus, "ReloadUcsi", &())
                .await
                .unwrap_err()
                .contains("unauthorized")
        );
        let answer = if method == "SetChoice" {
            call(&bus, method, &("controller-automatic", true)).await
        } else {
            call(&bus, method, &()).await
        };
        assert!(answer.is_ok(), "{method}: {answer:?}");
        assert_eq!(*fake.reloads.lock().unwrap(), 0);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_reset_inside_the_cooldown_is_the_first_ones_answer() {
    let bus = bus_or_skip!();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake::new(UCSI_LOG, broken("M5406WA")));
    let _server = serve(
        &bus,
        fake.clone(),
        &dir.path().join("s.json"),
        FakeAuthorizer(true),
    )
    .await;
    let first: RecoveryReply =
        serde_json::from_str(&call(&bus, "ReloadUcsi", &()).await.unwrap()).unwrap();
    assert!(!first.recent && first.restored.complete());
    let second: RecoveryReply =
        serde_json::from_str(&call(&bus, "ReloadUcsi", &()).await.unwrap()).unwrap();
    assert!(second.recent);
    assert_eq!(second.restored, first.restored);
    assert_eq!(*fake.reloads.lock().unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn findings_need_no_authorization_and_carry_no_log_text() {
    let bus = bus_or_skip!();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake::new(
        &format!("{UCSI_LOG}{DIED_LOG}{BLOCKED}"),
        broken("M5406WA"),
    ));
    let _server = serve(
        &bus,
        fake,
        &dir.path().join("s.json"),
        FakeAuthorizer(false),
    )
    .await;
    let wire = call(&bus, "Findings", &()).await.unwrap();
    for text in ["USBC000", "AMD-Vi", "hub_event", "Freezing"] {
        assert!(!wire.contains(text), "{wire}");
    }
    let findings: UsbFindings = serde_json::from_str(&wire).unwrap();
    assert!(findings.ucsi.active);
    assert_eq!(findings.sleep.streak, 2);
    let settings: UsbRecoverySettings =
        serde_json::from_str(&call(&bus, "Settings", &()).await.unwrap()).unwrap();
    assert_eq!(settings, UsbRecoverySettings::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_choice_outside_the_set_is_refused_before_anything() {
    let bus = bus_or_skip!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.json");
    let fake = Arc::new(Fake::new(UCSI_LOG, broken("M5406WA")));
    let _server = serve(&bus, fake, &path, FakeAuthorizer(true)).await;
    let refused = call(&bus, "SetChoice", &("/etc/passwd", true))
        .await
        .unwrap_err();
    assert!(
        refused.contains("daemon.error.invalid_argument:choice"),
        "{refused}"
    );
    assert!(!refused.contains("passwd"));
    assert!(!path.exists());
    let set: UsbRecoverySettings = serde_json::from_str(
        &call(&bus, "SetChoice", &("ucsi-automatic", true))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(set.ucsi_automatic);
}

/// Review finding 4: both ports back and a plugged-in charger still not
/// shown is the fault itself; no "reloaded" marker, so it stays failing
/// and may be tried again.
#[test]
fn a_reload_that_left_the_charger_hidden_is_not_marked() {
    let mut fake = Fake::new(UCSI_LOG, broken("M5406WA"));
    fake.charger_stays_hidden = true;
    let restored = recovery::reload(&fake, Timings::INSTANT).unwrap();
    assert_eq!(
        restored,
        Restored::Ucsi {
            ports: 2,
            expected_ports: 2,
            charger: core::ChargerShown::NotShown
        }
    );
    assert!(!fake.log.lock().unwrap().contains("reloaded"));
    assert!(recovery::findings(&fake).unwrap().ucsi.active);
}

/// Review finding 2: a line written into the daemon's journal stream by
/// anything else (here a forged HC death naming a working controller) is
/// not read, so no rebind follows from it.
#[test]
fn a_forged_line_in_the_daemons_stream_moves_nothing() {
    struct Forged(Fake);
    impl UsbMachine for Forged {
        fn kernel_log(&self) -> Result<String, DaemonError> {
            let forged = serde_json::json!({
                "MESSAGE": "xhci_hcd 0000:63:00.4: HC died; cleaning up",
                "_TRANSPORT": "stdout",
            });
            Ok(format!("{}{forged}\n", self.0.kernel_log()?))
        }
        fn present(&self) -> UsbPresent {
            self.0.present()
        }
        fn workers(&self) -> Vec<Worker> {
            self.0.workers()
        }
        fn pause(&self, duration: Duration) {
            self.0.pause(duration)
        }
        fn ucsi_module_ready(&self) -> Result<(), String> {
            self.0.ucsi_module_ready()
        }
        fn reload_ucsi(&self) -> Result<(), DaemonError> {
            self.0.reload_ucsi()
        }
        fn rebind(&self, address: &PciAddress) -> Result<(), DaemonError> {
            self.0.rebind(address)
        }
        fn mark(&self, line: &str) {
            self.0.mark(line)
        }
    }
    let forged = Forged(Fake::new(
        "PM: suspend entry (s2idle)\nPM: suspend exit\n",
        broken("M5406WA"),
    ));
    assert_eq!(
        recovery::rebind(&forged, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::NothingToReset))
    );
    assert!(forged.0.rebinds.lock().unwrap().is_empty());
}

/// Review finding 5 and second review 2: a run that reached the helper
/// holds the cooldown, success and failure alike, so a caller in a loop
/// does not unload the module again inside it. `Busy` is never kept.
#[tokio::test(flavor = "multi_thread")]
async fn only_a_run_that_reached_the_helper_holds_the_cooldown() {
    let runtime = tokio::runtime::Handle::current();
    let mut failing = Fake::new(UCSI_LOG, broken("M5406WA"));
    failing.reload_fails = true;
    let failing = Arc::new(failing);
    let limiter = recovery::Limiter::default();
    for _ in 0..3 {
        let machine = failing.clone();
        let read = recovery::findings(failing.as_ref());
        let answer = limiter
            .run(
                Duration::from_secs(30),
                &runtime,
                async move { read },
                move |found| recovery::reload_with(machine.as_ref(), &found, Timings::INSTANT),
            )
            .await;
        assert!(matches!(answer, Err(DaemonError::UsbRecoveryFailed(_))));
    }
    assert_eq!(*failing.reloads.lock().unwrap(), 1);

    // Busy (another reload's unit exists) is said and not kept.
    let limiter = recovery::Limiter::default();
    let busy = limiter
        .run(
            Duration::from_secs(30),
            &runtime,
            async { Ok(core::UsbFindings::default()) },
            |_| Err(DaemonError::Busy),
        )
        .await;
    assert_eq!(busy, Err(DaemonError::Busy));
    let fake = Arc::new(Fake::new(UCSI_LOG, broken("M5406WA")));
    let machine = fake.clone();
    let read = recovery::findings(fake.as_ref());
    let next = limiter
        .run(
            Duration::from_secs(30),
            &runtime,
            async move { read },
            move |found| recovery::reload_with(machine.as_ref(), &found, Timings::INSTANT),
        )
        .await;
    assert!(matches!(next, Ok(ref reply) if !reply.recent), "{next:?}");
    assert_eq!(*fake.reloads.lock().unwrap(), 1);
}

/// Second review 2: a refusal is not kept for the cooldown. A fault that
/// appears just after a refused Reset is repaired by the next one; refused
/// calls in a loop read the journal no more often than findings are fresh.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_is_decided_afresh_from_recent_findings() {
    let bus = bus_or_skip!();
    let dir = tempfile::tempdir().unwrap();
    let healthy = "PM: suspend entry (s2idle)\nPM: suspend exit\n";
    let fake = Arc::new(Fake::new(healthy, broken("M5406WA")));
    let _server = serve_fresh(
        &bus,
        fake.clone(),
        &dir.path().join("s.json"),
        FakeAuthorizer(true),
        Duration::ZERO,
    )
    .await;
    let refused = call(&bus, "ReloadUcsi", &()).await.unwrap_err();
    assert!(refused.contains("usb_nothing_to_reset"), "{refused}");
    *fake.log.lock().unwrap() = format!("{healthy}{UCSI_LOG}");
    let reply: RecoveryReply =
        serde_json::from_str(&call(&bus, "ReloadUcsi", &()).await.unwrap()).unwrap();
    assert!(!reply.recent && reply.restored.complete());
    assert_eq!(*fake.reloads.lock().unwrap(), 1);
    drop(_server);

    // With findings five seconds fresh, three refused calls read once.
    let fake = Arc::new(Fake::new(healthy, broken("M5406WA")));
    let _server = serve(
        &bus,
        fake.clone(),
        &dir.path().join("s.json"),
        FakeAuthorizer(true),
    )
    .await;
    for _ in 0..3 {
        assert!(call(&bus, "ReloadUcsi", &()).await.is_err());
    }
    assert_eq!(*fake.reads.lock().unwrap(), 1);
}

/// Second review 6: a kernel that cannot load the file is refused before
/// the helper starts, with a key the page says in words; nothing unloads.
#[test]
fn a_module_the_kernel_cannot_load_is_refused_before_the_helper() {
    let mut fake = Fake::new(UCSI_LOG, broken("M5406WA"));
    fake.module_ready = Err("no /boot/config".into());
    assert_eq!(
        recovery::reload(&fake, Timings::INSTANT),
        Err(DaemonError::UsbRefused(Refusal::ModuleCannotLoad))
    );
    assert_eq!(*fake.reloads.lock().unwrap(), 0);
    assert_eq!(
        DaemonError::UsbRefused(Refusal::ModuleCannotLoad).to_string(),
        "daemon.error.usb_module_cannot_load"
    );
}

/// Second review 6: the kernel's own `/sys/module/compression` decides
/// when it is there; the build config only when it is not; neither is a no.
#[test]
fn the_kernel_says_which_compressed_modules_it_loads() {
    let dir = tempfile::tempdir().unwrap();
    let modules = dir.path().join("modules");
    let release = "7.0.0-43-generic";
    let ko_dir = modules.join(release).join(recovery::UCSI_MODULE_DIR);
    std::fs::create_dir_all(&ko_dir).unwrap();
    std::fs::write(ko_dir.join("ucsi_acpi.ko.zst"), b"m").unwrap();
    // The file check refuses one that group or others can write, whatever
    // this process's umask is.
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(
            ko_dir.join("ucsi_acpi.ko.zst"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }
    let attribute = dir.path().join("compression");
    let config = dir.path().join("config");
    use std::os::unix::fs::MetadataExt as _;
    let owner = std::fs::metadata(dir.path()).unwrap().uid();
    let check =
        || recovery::prepare_module(&attribute, &modules, &config, release, owner).map(|_| ());
    // Neither source: refused.
    assert!(check().is_err());
    // The config alone.
    std::fs::write(
        &config,
        "CONFIG_MODULE_DECOMPRESS=y\nCONFIG_MODULE_COMPRESS_ZSTD=y\n",
    )
    .unwrap();
    assert!(check().is_ok());
    // The kernel's own word wins over the config.
    std::fs::write(&attribute, "xz\n").unwrap();
    assert!(check().is_err());
    std::fs::write(&attribute, "zstd\n").unwrap();
    std::fs::remove_file(&config).unwrap();
    assert!(check().is_ok());
    // An uncompressed module needs neither.
    std::fs::remove_file(ko_dir.join("ucsi_acpi.ko.zst")).unwrap();
    std::fs::write(ko_dir.join("ucsi_acpi.ko"), b"m").unwrap();
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(
            ko_dir.join("ucsi_acpi.ko"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }
    std::fs::remove_file(&attribute).unwrap();
    assert_eq!(check(), Ok(()));
    // No file at all.
    std::fs::remove_file(ko_dir.join("ucsi_acpi.ko")).unwrap();
    assert!(check().is_err());
}

/// Second review 3: what the power-off depends on is read again right
/// before it, after the notifications, which can take seconds each.
#[test]
fn a_transaction_started_while_people_are_told_stops_the_power_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings(
        dir.path(),
        &[
            UsbChoice::ControllerAutomatic,
            UsbChoice::PowerOffWhenSleepBlocked,
        ],
    );
    let fake = Fake::new(&format!("{DIED_LOG}{BLOCKED}"), broken("M5406WA"));
    let world = World {
        lid_closed: Some(true),
        docked: Some(false),
        busy_after_notify: true,
        ..World::default()
    };
    let run = dir.path().join("run.json");
    let done = recovery::after_sleep(&fake, &world, &path, &run, Timings::INSTANT);
    assert_eq!(done.notified, vec![1000]);
    assert!(!done.powered_off);
    assert_eq!(*world.powered_off.lock().unwrap(), 0);
    // Not spent: once the transaction is over, the same run may still.
    *world.became_busy.lock().unwrap() = false;
    let world = World {
        lid_closed: Some(true),
        docked: Some(false),
        ..World::default()
    };
    let again = recovery::after_sleep(&fake, &world, &path, &run, Timings::INSTANT);
    assert!(again.powered_off);
}

/// Second review 4 and 5: a lingering account blocks the power-off, an
/// unreadable one too; every logind class that is a person's login counts,
/// and an unknown class counts as one.
#[test]
fn lingering_accounts_and_every_login_class_count() {
    use recovery::{class_is_login, users_block};
    assert!(!users_block(&[Some((false, "active".into()))]));
    assert!(users_block(&[
        Some((false, "active".into())),
        Some((false, "lingering".into()))
    ]));
    assert!(users_block(&[Some((true, "active".into()))]));
    assert!(users_block(&[None]));
    for class in [
        "user",
        "user-early",
        "user-incomplete",
        "user-light",
        "user-early-light",
    ] {
        assert_eq!(class_is_login(class), Some(true), "{class}");
    }
    for class in [
        "greeter",
        "lock-screen",
        "background",
        "background-light",
        "manager",
        "manager-early",
        "none",
    ] {
        assert_eq!(class_is_login(class), Some(false), "{class}");
    }
    assert_eq!(class_is_login("user-future"), None);
}

/// Review finding 1: everything about the file is checked before the
/// module is unloaded; a file that is missing, a link, writable by others,
/// or compressed for a kernel that cannot decompress it leaves the loaded
/// module alone.
#[test]
fn the_module_stays_loaded_unless_its_file_is_ready() {
    use std::os::unix::fs::PermissionsExt as _;
    #[derive(Default)]
    struct Calls {
        unloads: Mutex<u32>,
        loads: Mutex<u32>,
        load_fails: bool,
    }
    impl recovery::ModuleCalls for Calls {
        fn unload(&self) -> std::io::Result<bool> {
            *self.unloads.lock().unwrap() += 1;
            Ok(true)
        }
        fn load(&self, _: &recovery::ModuleFile) -> std::io::Result<()> {
            *self.loads.lock().unwrap() += 1;
            if self.load_fails {
                Err(std::io::Error::from_raw_os_error(libc::EKEYREJECTED))
            } else {
                Ok(())
            }
        }
        fn pause(&self, _: Duration) {}
    }
    let dir = tempfile::tempdir().unwrap();
    let modules = dir.path().join("modules");
    let release = "7.0.0-43-generic";
    let ko_dir = modules.join(release).join(recovery::UCSI_MODULE_DIR);
    std::fs::create_dir_all(&ko_dir).unwrap();
    let config = dir.path().join("config");
    // SAFETY: getuid has no preconditions.
    let me = unsafe { libc::getuid() };
    let no_attribute = dir.path().join("no-compression-attribute");
    let prepare = || recovery::prepare_module(&no_attribute, &modules, &config, release, me);
    let reload = |calls: &Calls| recovery::reload_module(calls, prepare());

    // Missing.
    let calls = Calls::default();
    assert!(reload(&calls).is_err());
    assert_eq!(*calls.unloads.lock().unwrap(), 0);

    // Compressed, and the kernel config is missing, then says no.
    let ko = ko_dir.join("ucsi_acpi.ko.zst");
    std::fs::write(&ko, b"module").unwrap();
    std::fs::set_permissions(&ko, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(reload(&calls).is_err());
    std::fs::write(
        &config,
        "CONFIG_MODULE_DECOMPRESS=y\n# CONFIG_MODULE_COMPRESS_ZSTD is not set\n",
    )
    .unwrap();
    assert!(reload(&calls).is_err());
    assert_eq!(*calls.unloads.lock().unwrap(), 0);

    // Writable by others.
    std::fs::write(
        &config,
        "CONFIG_MODULE_DECOMPRESS=y\nCONFIG_MODULE_COMPRESS_ZSTD=y\n",
    )
    .unwrap();
    std::fs::set_permissions(&ko, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(reload(&calls).is_err());
    assert_eq!(*calls.unloads.lock().unwrap(), 0);

    // Ready: unloaded, then loaded.
    std::fs::set_permissions(&ko, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(reload(&calls), Ok("reloaded".to_string()));
    assert_eq!(
        (*calls.unloads.lock().unwrap(), *calls.loads.lock().unwrap()),
        (1, 1)
    );

    // A load the kernel refuses is tried twice and said plainly.
    let refusing = Calls {
        load_fails: true,
        ..Calls::default()
    };
    let error = reload(&refusing).unwrap_err();
    assert!(error.contains("could not be loaded again"), "{error}");
    assert_eq!(*refusing.loads.lock().unwrap(), 2);

    // A link in the file's place is not the distribution's file.
    std::fs::remove_file(&ko).unwrap();
    std::os::unix::fs::symlink(&config, &ko).unwrap();
    let calls = Calls::default();
    assert!(reload(&calls).is_err());
    assert_eq!(*calls.unloads.lock().unwrap(), 0);
}

/// Review finding 6: any session but the active local graphical ones is
/// another session, the same person's SSH or tty login included, and one
/// that could not be read too.
#[test]
fn every_other_session_blocks_the_power_off() {
    use recovery::{SessionSeen, sessions_people};
    let desk = || SessionSeen {
        uid: 1000,
        active: true,
        remote: false,
        kind: "wayland".into(),
    };
    assert_eq!(sessions_people(&[Some(desk())]), (vec![1000], false));
    for other in [
        Some(SessionSeen {
            kind: "tty".into(),
            ..desk()
        }),
        Some(SessionSeen {
            remote: true,
            ..desk()
        }),
        Some(SessionSeen {
            active: false,
            ..desk()
        }),
        Some(SessionSeen {
            uid: 1001,
            kind: "tty".into(),
            ..desk()
        }),
        None,
    ] {
        assert_eq!(
            sessions_people(&[Some(desk()), other.clone()]),
            (vec![1000], true),
            "{other:?}"
        );
    }
}

/// Review finding 7: a child that does not finish is ended at the deadline.
#[test]
fn a_notification_that_hangs_is_ended_at_its_deadline() {
    let mut child = test_prerequisites::require!(
        "sleep",
        Command::new("sleep")
            .arg("30")
            .spawn()
            .map_err(|_| "sleep is not installed")
    );
    let started = std::time::Instant::now();
    let status = recovery::wait_with_deadline(&mut child, Duration::from_millis(200)).unwrap();
    assert_eq!(status, None);
    assert!(started.elapsed() < Duration::from_secs(5));
}
