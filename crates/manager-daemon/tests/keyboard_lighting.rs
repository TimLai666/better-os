//! `org.betteros.Hardware1`'s keyboard colour (ticket 136, ADR 0034) over a
//! private bus: the window's own client (`hardware_platform::privileged`)
//! against the daemon's service, a fake sysfs root with the owner's real
//! report descriptor, and a fake LampArray helper. Nothing here opens a
//! device or reaches the machine's buses.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use hardware_core::lighting::{
    Access, Effect, Interface, LightingChoice, LightingReply, Rgb, Speed,
};
use hardware_core::write::WriteOutcome;
use hardware_platform::privileged;
use hardware_platform::sysfs::SysRoot;
use manager_daemon::authorize::{
    Authorizer, FakeActionAuthorizer, FakeAuthorizer, FakeDismissingAuthorizer,
    KEYBOARD_LIGHTING_ACTION,
};
use manager_daemon::hardware::{DirectWriter, HardwareService};
use manager_daemon::keyboard_lighting::{FakeLampHelper, LightingPaths};
use manager_daemon::service::{CallGate, OBJECT_PATH};

const M5406WA: &[u8] = include_bytes!("../../hardware-core/tests/fixtures/m5406wa-ite5570.rdesc");

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

/// The owner's keyboard: `hidraw1` on I2C with the real descriptor.
struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    fn owners() -> Machine {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path().join("sys/class/hidraw/hidraw1/device");
        std::fs::create_dir_all(&device).unwrap();
        std::fs::write(device.join("report_descriptor"), M5406WA).unwrap();
        std::fs::write(device.join("uevent"), "HID_ID=0018:00000B05:00005570\n").unwrap();
        Machine { dir }
    }

    fn state(&self) -> PathBuf {
        self.dir
            .path()
            .join("var/lib/better-os/keyboard-lighting.json")
    }
}

async fn serve<A: Authorizer + 'static>(
    bus: &PrivateBus,
    machine: &Machine,
    authorizer: A,
    helper: Arc<FakeLampHelper>,
) -> zbus::Connection {
    let address: zbus::Address = bus.address.parse().unwrap();
    let service = HardwareService::new(
        authorizer,
        Arc::new(DirectWriter),
        SysRoot::at(machine.dir.path()),
        Arc::new(CallGate::default()),
    )
    .with_lamp_helper(helper)
    .with_lighting_paths(LightingPaths {
        state: machine.state(),
        firmware_marker: machine.dir.path().join("run/keyboard-lighting-firmware"),
    });
    zbus::connection::Builder::address(address)
        .unwrap()
        .serve_at(OBJECT_PATH, service)
        .unwrap()
        .name(privileged::BUS_NAME)
        .unwrap()
        .build()
        .await
        .unwrap()
}

fn client(bus: &PrivateBus) -> zbus::blocking::Connection {
    let address: zbus::Address = bus.address.parse().unwrap();
    zbus::blocking::connection::Builder::address(address)
        .unwrap()
        .build()
        .unwrap()
}

async fn describe(bus: &PrivateBus) -> Result<LightingReply, Access> {
    let system = client(bus);
    tokio::task::spawn_blocking(move || privileged::keyboard_lighting(&system))
        .await
        .unwrap()
}

async fn set(bus: &PrivateBus, choice: LightingChoice) -> (WriteOutcome, Option<LightingReply>) {
    let system = client(bus);
    tokio::task::spawn_blocking(move || privileged::set_keyboard_lighting(&system, &choice))
        .await
        .unwrap()
}

fn red() -> LightingChoice {
    LightingChoice {
        effect: Effect::Static,
        colour: Rgb::new(255, 0, 0),
        intensity: 60,
        speed: Speed::Normal,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_window_describes_sets_and_gives_back_the_owners_keyboard() {
    let bus = bus_or_skip!();
    let machine = Machine::owners();
    let helper = Arc::new(FakeLampHelper::new("hidraw1", M5406WA));
    let _server = serve(&bus, &machine, FakeAuthorizer(true), helper.clone()).await;

    let reply = describe(&bus).await.unwrap();
    assert_eq!(reply.interface, Interface::LampArray);
    assert_eq!(reply.stored, None);
    assert_eq!(reply.lamp_array.unwrap().lamps, 1);

    let (outcome, reply) = set(&bus, red()).await;
    assert_eq!(outcome, WriteOutcome::Done);
    assert_eq!(reply.unwrap().stored, Some(red()));
    // A level count of 255 is off and 255 levels (HID Usage Tables 1.4,
    // 26.9): 60% of it is 153.
    assert_eq!(helper.device.lock().unwrap().shown, Some([255, 0, 0, 153]));
    // The saved choice names its keyboard: bus, vendor, product.
    let saved = std::fs::read_to_string(machine.state()).unwrap();
    assert!(
        saved.contains(r#""bus":24,"vendor":2821,"product":21872"#),
        "{saved}"
    );
    assert!(machine.state().exists());
    assert_eq!(describe(&bus).await.unwrap().stored, Some(red()));

    let (outcome, reply) = set(&bus, LightingChoice::starting(Effect::Firmware)).await;
    assert_eq!(outcome, WriteOutcome::Done);
    assert_eq!(reply.unwrap().stored, None);
    assert!(helper.device.lock().unwrap().autonomous);
    assert!(!machine.state().exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_effect_the_keyboard_cannot_do_is_refused_and_nothing_is_written() {
    let bus = bus_or_skip!();
    let machine = Machine::owners();
    let helper = Arc::new(FakeLampHelper::new("hidraw1", M5406WA));
    let _server = serve(&bus, &machine, FakeAuthorizer(true), helper.clone()).await;
    for effect in [Effect::Breathing, Effect::Cycle, Effect::Strobe] {
        let (outcome, _) = set(&bus, LightingChoice::starting(effect)).await;
        assert!(
            matches!(&outcome, WriteOutcome::Failed { details } if details.contains("daemon.error.invalid_argument")),
            "{effect:?}: {outcome:?}"
        );
    }
    assert!(!machine.state().exists());
    assert!(helper.device.lock().unwrap().autonomous);

    // Raw calls outside the closed set: an unknown effect, a speed of 3, an
    // intensity over 100.
    let system = client(&bus);
    let refused = tokio::task::spawn_blocking(move || {
        let call = |effect: &str, intensity: u8, speed: u8| {
            system
                .call_method(
                    Some(privileged::BUS_NAME),
                    OBJECT_PATH,
                    Some(privileged::INTERFACE),
                    "SetKeyboardLighting",
                    &(effect, 1u8, 2u8, 3u8, intensity, speed),
                )
                .is_err()
        };
        [
            call("rainbow", 50, 1),
            call("static", 50, 3),
            call("static", 101, 1),
            call("STATIC", 50, 1),
        ]
    })
    .await
    .unwrap();
    assert_eq!(refused, [true; 4]);
    assert!(!machine.state().exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_or_cancelled_caller_reads_and_changes_nothing() {
    let bus = bus_or_skip!();
    let machine = Machine::owners();
    let helper = Arc::new(FakeLampHelper::new("hidraw1", M5406WA));
    let _server = serve(&bus, &machine, FakeAuthorizer(false), helper.clone()).await;
    assert_eq!(describe(&bus).await, Err(Access::NotAllowed));
    assert_eq!(set(&bus, red()).await.0, WriteOutcome::NotAllowed);
    assert_eq!(helper.runs.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(_server);

    let bus = bus_or_skip!();
    let _server = serve(&bus, &machine, FakeDismissingAuthorizer, helper.clone()).await;
    assert_eq!(describe(&bus).await, Err(Access::Cancelled));
    assert_eq!(set(&bus, red()).await.0, WriteOutcome::Cancelled);
    drop(_server);

    // The charge limit's action does not open the keyboard.
    let bus = bus_or_skip!();
    let _server = serve(
        &bus,
        &machine,
        FakeActionAuthorizer("org.betteros.manager.set-charge-limit"),
        helper.clone(),
    )
    .await;
    assert_eq!(describe(&bus).await, Err(Access::NotAllowed));
    drop(_server);
    let bus = bus_or_skip!();
    let _server = serve(
        &bus,
        &machine,
        FakeActionAuthorizer(KEYBOARD_LIGHTING_ACTION),
        helper.clone(),
    )
    .await;
    assert!(describe(&bus).await.is_ok());
    assert_eq!(helper.runs.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_keyboard_is_absent_and_no_daemon_is_no_service() {
    let bus = bus_or_skip!();
    let empty = Machine {
        dir: tempfile::tempdir().unwrap(),
    };
    let helper = Arc::new(FakeLampHelper::new("hidraw1", M5406WA));
    let _server = serve(&bus, &empty, FakeAuthorizer(true), helper).await;
    assert_eq!(describe(&bus).await, Err(Access::Absent));
    drop(_server);

    let quiet = bus_or_skip!();
    assert_eq!(describe(&quiet).await, Err(Access::NoService));
    let (outcome, reply) = set(&quiet, red()).await;
    assert!(
        matches!(outcome, WriteOutcome::Failed { .. }),
        "{outcome:?}"
    );
    assert_eq!(reply, None);
}

/// A daemon older than the window answers without these methods: the card
/// says it needs updating, not that it is missing (review finding 12).
#[tokio::test(flavor = "multi_thread")]
async fn an_older_daemon_is_outdated_not_missing() {
    struct Older;
    #[zbus::interface(name = "org.betteros.Hardware1")]
    impl Older {
        fn set_charge_limit(&self, percent: u8) -> u8 {
            percent
        }
    }
    let bus = bus_or_skip!();
    let address: zbus::Address = bus.address.parse().unwrap();
    let _server = zbus::connection::Builder::address(address)
        .unwrap()
        .serve_at(OBJECT_PATH, Older)
        .unwrap()
        .name(privileged::BUS_NAME)
        .unwrap()
        .build()
        .await
        .unwrap();
    assert_eq!(describe(&bus).await, Err(Access::Outdated));
}

/// A caller in a loop (review finding 8): calls coalesce, the newest is
/// what the keyboard shows, and far fewer helpers run than calls were made.
#[tokio::test(flavor = "multi_thread")]
async fn a_burst_of_changes_writes_the_newest_and_coalesces_the_rest() {
    let bus = bus_or_skip!();
    let machine = Machine::owners();
    let helper = Arc::new(FakeLampHelper::new("hidraw1", M5406WA));
    let _server = serve(&bus, &machine, FakeAuthorizer(true), helper.clone()).await;
    // Read once so the description is cached.
    describe(&bus).await.unwrap();
    let before = helper.runs.load(std::sync::atomic::Ordering::SeqCst);
    let mut calls = Vec::new();
    for red_level in 1..=8u8 {
        let system = client(&bus);
        calls.push(tokio::task::spawn_blocking(move || {
            privileged::set_keyboard_lighting(
                &system,
                &LightingChoice {
                    colour: Rgb::new(red_level, 0, 0),
                    ..red()
                },
            )
        }));
    }
    let mut done = 0;
    for call in calls {
        let (outcome, _) = call.await.unwrap();
        if outcome == WriteOutcome::Done {
            done += 1;
        }
    }
    assert_eq!(done, 8, "every caller got an answer");
    let writes = helper.runs.load(std::sync::atomic::Ordering::SeqCst) - before;
    assert!(writes < 8, "{writes} helper runs for 8 calls");
    // Whatever was written last is what is saved.
    let shown = helper.device.lock().unwrap().shown.unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(machine.state()).unwrap()).unwrap();
    assert_eq!(
        saved["choice"]["colour"]["red"].as_u64().unwrap(),
        u64::from(shown[0])
    );
}
