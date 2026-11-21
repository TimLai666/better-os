//! The power mode beside the icon (ticket 295), against a stand-in
//! power-profiles-daemon on a private bus: read at once, followed when
//! something else changes it, nothing on a machine without it, and the
//! second item a panel reads. Nothing here touches the machine's own
//! system bus or its power mode.

use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use hardware_core::Presence;
use hardware_core::power::{Profile, mode_in_force};
use hardware_platform::bus;
use hardware_platform::fakes::{self, FakeProfiles};
use monitor_tray::menu::{IconChoice, Locale, PowerFacts, TrayReadings};
use monitor_tray::{POWER_ITEM_PATH, Tray, TrayState};

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

    fn builder(&self) -> zbus::blocking::connection::Builder<'static> {
        zbus::blocking::connection::Builder::address(self.address.clone().as_str()).unwrap()
    }

    fn connect(&self) -> zbus::blocking::Connection {
        self.builder().build().unwrap()
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

/// Follows the daemon on `bus` from a thread, as the item does.
fn following(bus: &PrivateBus) -> mpsc::Receiver<PowerFacts> {
    let (sender, readings) = mpsc::channel();
    let system = bus.connect();
    std::thread::spawn(move || {
        monitor_tray::power::follow(&system, |facts| sender.send(facts).is_ok());
    });
    readings
}

fn mode_of(facts: &PowerFacts) -> Option<Profile> {
    match facts {
        Presence::Present(facts) => mode_in_force(facts),
        _ => None,
    }
}

/// The next reading whose mode is `want`, within a few seconds.
fn until_mode(readings: &mpsc::Receiver<PowerFacts>, want: Option<Profile>) -> PowerFacts {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let facts = readings
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("no reading with {want:?} arrived"));
        if mode_of(&facts) == want {
            return facts;
        }
    }
}

#[test]
fn the_mode_is_read_at_once_and_followed_when_something_else_changes_it() {
    let bus = bus_or_skip!();
    let _daemon =
        fakes::serve_system(bus.builder(), FakeProfiles::owners_machine(), true, false).unwrap();
    let readings = following(&bus);
    until_mode(&readings, Some(Profile::Balanced));

    // The System page, the charger switch, or any other program.
    let other = bus.connect();
    bus::set_power_profile(&other, Profile::Performance).unwrap();
    until_mode(&readings, Some(Profile::Performance));
    bus::set_power_profile(&other, Profile::PowerSaver).unwrap();
    let facts = until_mode(&readings, Some(Profile::PowerSaver));

    let mut state = TrayState::new(Locale::English, IconChoice::default(), None);
    state.adopt(Some(TrayReadings::default()));
    state.adopt_power(facts);
    assert!(
        state
            .menu
            .labels()
            .contains(&"Power mode: Best power efficiency"),
        "{:?}",
        state.menu.labels()
    );
}

#[test]
fn a_machine_without_the_daemon_shows_nothing_until_one_starts() {
    let bus = bus_or_skip!();
    let readings = following(&bus);
    let first = readings
        .recv_timeout(Duration::from_secs(5))
        .expect("a first reading");
    assert_eq!(first, Presence::Absent);
    let state = {
        let mut state = TrayState::new(Locale::English, IconChoice::default(), None);
        state.adopt_power(first);
        state
    };
    assert_eq!(state.power_item.status(), "Passive");
    assert!(
        !state
            .menu
            .labels()
            .iter()
            .any(|l| l.starts_with("Power mode:"))
    );

    // The daemon starts late: its name appearing is followed.
    let _daemon =
        fakes::serve_system(bus.builder(), FakeProfiles::owners_machine(), false, false).unwrap();
    until_mode(&readings, Some(Profile::Balanced));
}

#[test]
fn a_daemon_that_offers_no_modes_shows_nothing() {
    let bus = bus_or_skip!();
    let profiles = FakeProfiles {
        active: String::new(),
        profiles: Vec::new(),
        degraded: String::new(),
        holds: Vec::new(),
    };
    let _daemon = fakes::serve_system(bus.builder(), profiles, false, false).unwrap();
    let readings = following(&bus);
    let first = readings
        .recv_timeout(Duration::from_secs(5))
        .expect("a first reading");
    assert!(matches!(first, Presence::Present(_)), "{first:?}");
    let mut state = TrayState::new(Locale::English, IconChoice::default(), None);
    state.adopt_power(first);
    assert_eq!(state.power_item.status(), "Passive");
    assert_eq!(state.power_item.icon_name(), "");
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierItem",
    default_path = "/PowerModeItem"
)]
trait PowerItem {
    #[zbus(property)]
    fn status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn icon_name(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn menu(&self) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    #[zbus(signal)]
    fn new_status(&self, status: String) -> zbus::Result<()>;
}

#[tokio::test]
async fn a_panel_reads_the_mode_item_and_hears_it_change() {
    use zbus::export::futures_core::Stream;
    let bus = bus_or_skip!();
    let tray = Tray::new(TrayState::new(Locale::English, IconChoice::default(), None));
    let connection = monitor_tray::serve(
        zbus::connection::Builder::address(bus.address.as_str()).unwrap(),
        tray.clone(),
    )
    .await
    .unwrap();
    let unique = connection.unique_name().unwrap().to_string();
    let panel = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let item = PowerItemProxy::builder(&panel)
        .destination(unique.as_str())
        .unwrap()
        .path(POWER_ITEM_PATH)
        .unwrap()
        // A StatusNotifierItem says a change with its own signals, not
        // PropertiesChanged, so a panel reads each property afresh.
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .unwrap();
    assert_eq!(item.status().await.unwrap(), "Passive");
    assert_eq!(item.id().await.unwrap(), "better-monitor-power-mode");
    assert_eq!(item.menu().await.unwrap().as_str(), monitor_tray::MENU_PATH);
    let mut statuses = item.receive_new_status().await.unwrap();

    let facts = Presence::Present(hardware_core::power::PowerProfilesFacts {
        active: "performance".into(),
        profiles: vec![
            "power-saver".into(),
            "balanced".into(),
            "performance".into(),
        ],
        ..Default::default()
    });
    let changes = tray.state.lock().await.adopt_power(facts);
    tray.announce(changes).await;
    let signal = tokio::time::timeout(
        Duration::from_secs(5),
        std::future::poll_fn(|context| std::pin::Pin::new(&mut statuses).poll_next(context)),
    )
    .await
    .expect("NewStatus arrives")
    .expect("a signal");
    assert_eq!(signal.args().unwrap().status, "Active");
    assert_eq!(item.status().await.unwrap(), "Active");
    assert_eq!(
        item.icon_name().await.unwrap(),
        "power-profile-performance-symbolic"
    );
}
