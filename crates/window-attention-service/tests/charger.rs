//! "Switch power mode with the charger" against a fake UPower and a fake
//! power-profiles-daemon on a private bus (ticket 86).

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use hardware_core::follow::ChargerFollow;
use hardware_core::power::Profile;
use window_attention_service::charger::{self, Event};
use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

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

    fn connect(&self) -> Connection {
        self.builder().build().unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct FakeUPower {
    on_battery: bool,
}

#[zbus::interface(name = "org.freedesktop.UPower")]
impl FakeUPower {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        self.on_battery
    }
}

struct FakeProfiles {
    active: String,
}

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl FakeProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.active.clone()
    }

    #[zbus(property)]
    fn set_active_profile(&mut self, profile: String) {
        self.active = profile;
    }

    #[zbus(property)]
    fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
        ["power-saver", "balanced", "performance"]
            .iter()
            .map(|name| {
                HashMap::from([(
                    "Profile".to_string(),
                    OwnedValue::try_from(zbus::zvariant::Value::from(*name)).unwrap(),
                )])
            })
            .collect()
    }

    #[zbus(property)]
    fn performance_degraded(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedValue>> {
        Vec::new()
    }
}

struct World {
    _bus: PrivateBus,
    services: Connection,
    settings: tempfile::TempDir,
    events: mpsc::Receiver<Event>,
}

impl World {
    fn start(follow: Option<ChargerFollow>, on_battery: bool) -> Option<World> {
        let bus = PrivateBus::start()?;
        let services = bus
            .builder()
            .serve_at("/org/freedesktop/UPower", FakeUPower { on_battery })
            .unwrap()
            .serve_at(
                "/org/freedesktop/UPower/PowerProfiles",
                FakeProfiles {
                    active: "balanced".into(),
                },
            )
            .unwrap()
            .name("org.freedesktop.UPower")
            .unwrap()
            .name("org.freedesktop.UPower.PowerProfiles")
            .unwrap()
            .build()
            .unwrap();
        let settings = tempfile::tempdir().unwrap();
        let path = settings.path().join("power-follow.json");
        if let Some(follow) = follow {
            hardware_platform::follow_store::save(&path, &follow).unwrap();
        }
        let (sender, events) = mpsc::channel();
        let system = bus.connect();
        std::thread::spawn(move || charger::follow(system, path, move |e| sender.send(e).is_ok()));
        // Let the follower subscribe before anything changes.
        std::thread::sleep(Duration::from_millis(300));
        Some(World {
            _bus: bus,
            services,
            settings,
            events,
        })
    }

    fn plug(&self, on_battery: bool) {
        let server = self.services.object_server();
        let iface = server
            .interface::<_, FakeUPower>("/org/freedesktop/UPower")
            .unwrap();
        iface.get_mut().on_battery = on_battery;
        zbus::block_on(iface.get().on_battery_changed(iface.signal_emitter())).unwrap();
    }

    fn active(&self) -> String {
        let server = self.services.object_server();
        let iface = server
            .interface::<_, FakeProfiles>("/org/freedesktop/UPower/PowerProfiles")
            .unwrap();
        iface.get().active.clone()
    }

    fn set_by_hand(&self, profile: &str) {
        let server = self.services.object_server();
        let iface = server
            .interface::<_, FakeProfiles>("/org/freedesktop/UPower/PowerProfiles")
            .unwrap();
        iface.get_mut().active = profile.into();
    }

    fn next(&self) -> Option<Event> {
        self.events.recv_timeout(Duration::from_secs(5)).ok()
    }
}

macro_rules! world_or_skip {
    ($follow:expr, $on_battery:expr) => {
        test_prerequisites::require!(
            "dbus-daemon",
            World::start($follow, $on_battery).ok_or("a private dbus-daemon did not start")
        )
    };
}

fn on() -> ChargerFollow {
    ChargerFollow {
        enabled: true,
        ..Default::default()
    }
}

#[test]
fn plugging_in_sets_performance_and_unplugging_sets_power_saver() {
    let world = world_or_skip!(Some(on()), true);
    world.plug(false);
    assert_eq!(world.next(), Some(Event::Set(Profile::Performance)));
    assert_eq!(world.active(), "performance");
    world.plug(true);
    assert_eq!(world.next(), Some(Event::Set(Profile::PowerSaver)));
    assert_eq!(world.active(), "power-saver");
}

#[test]
fn a_mode_picked_by_hand_stays_until_the_next_plug_event() {
    let world = world_or_skip!(Some(on()), true);
    world.plug(false);
    assert_eq!(world.next(), Some(Event::Set(Profile::Performance)));
    world.set_by_hand("balanced");
    // Another UPower change that is not a plug event does nothing.
    world.plug(false);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(world.active(), "balanced");
    world.plug(true);
    assert_eq!(world.next(), Some(Event::Set(Profile::PowerSaver)));
}

#[test]
fn the_chosen_modes_are_used() {
    let world = world_or_skip!(
        Some(ChargerFollow {
            enabled: true,
            plugged_in: Profile::Balanced,
            on_battery: Profile::Balanced,
        }),
        false
    );
    world.set_by_hand("performance");
    world.plug(true);
    assert_eq!(world.next(), Some(Event::Set(Profile::Balanced)));
    assert_eq!(world.active(), "balanced");
}

#[test]
fn off_by_default_and_turned_off_leaves_the_mode() {
    let world = world_or_skip!(None, true);
    world.plug(false);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(world.active(), "balanced");
    // Turned on, then off again: the mode it set stays.
    let path = world.settings.path().join("power-follow.json");
    hardware_platform::follow_store::save(&path, &on()).unwrap();
    world.plug(true);
    assert_eq!(world.next(), Some(Event::Set(Profile::PowerSaver)));
    hardware_platform::follow_store::save(&path, &ChargerFollow::default()).unwrap();
    world.plug(false);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(world.active(), "power-saver");
}
