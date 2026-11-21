//! The bus readers against stand-ins on a private bus: both
//! power-profiles-daemon names, gsd-power's screen and keyboard, and UPower.

use std::process::{Child, Command, Stdio};

use hardware_core::Presence;
use hardware_core::display::KeyboardSource;
use hardware_core::power::Profile;
use hardware_platform::bus;
use hardware_platform::fakes::{self, FakeProfiles};
use zbus::blocking::Connection;

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

macro_rules! bus_or_skip {
    () => {
        test_prerequisites::require!(
            "dbus-daemon",
            PrivateBus::start().ok_or("a private dbus-daemon did not start")
        )
    };
}

#[test]
fn power_profiles_are_read_with_their_holds_and_set() {
    let bus = bus_or_skip!();
    let mut profiles = FakeProfiles::owners_machine();
    profiles.degraded = "lap-detected".into();
    profiles.holds = vec![("org.gnome.Boxes".into(), "performance".into())];
    let _services = fakes::serve_system(bus.builder(), profiles, true, false).unwrap();
    let client = bus.connect();
    let Presence::Present(facts) = bus::power_profiles(&client) else {
        panic!("power-profiles-daemon answers")
    };
    assert_eq!(facts.active, "balanced");
    assert_eq!(facts.profiles, ["power-saver", "balanced", "performance"]);
    assert_eq!(facts.degraded, "lap-detected");
    assert_eq!(facts.holds[0].application, "org.gnome.Boxes");
    assert_eq!(
        bus::power_profiles_endpoint(&client).unwrap().name,
        "org.freedesktop.UPower.PowerProfiles"
    );
    bus::set_power_profile(&client, Profile::Performance).unwrap();
    assert_eq!(
        bus::power_profiles(&client).value().unwrap().active,
        "performance"
    );
    assert_eq!(bus::on_battery(&client), Ok(false));
}

#[test]
fn the_older_name_is_used_when_it_is_the_only_one() {
    let bus = bus_or_skip!();
    let _services =
        fakes::serve_older_profiles_only(bus.builder(), FakeProfiles::owners_machine()).unwrap();
    let client = bus.connect();
    assert_eq!(
        bus::power_profiles_endpoint(&client).unwrap().name,
        "net.hadess.PowerProfiles"
    );
    bus::set_power_profile(&client, Profile::PowerSaver).unwrap();
    assert_eq!(
        bus::power_profiles(&client).value().unwrap().active,
        "power-saver"
    );
}

#[test]
fn nothing_on_the_bus_is_absent() {
    let bus = bus_or_skip!();
    let client = bus.connect();
    assert_eq!(bus::power_profiles(&client), Presence::Absent);
    assert_eq!(bus::screen_brightness(&client), Presence::Absent);
    assert_eq!(bus::keyboard(&client, &client), Presence::Absent);
    assert_eq!(bus::shell_major_version(&client), None);
}

#[test]
fn gnome_brightness_and_keyboard_come_first() {
    let bus = bus_or_skip!();
    let _session = fakes::serve_session(bus.builder(), 60, 33, 4).unwrap();
    let _system =
        fakes::serve_system(bus.builder(), FakeProfiles::owners_machine(), false, true).unwrap();
    let client = bus.connect();
    assert_eq!(bus::screen_brightness(&client), Presence::Present(60));
    bus::set_screen_brightness(&client, 25).unwrap();
    assert_eq!(bus::screen_brightness(&client), Presence::Present(25));
    let Presence::Present(levels) = bus::keyboard(&client, &client) else {
        panic!("gsd-power has a keyboard")
    };
    assert_eq!(levels.source, KeyboardSource::Gnome { steps: 4 });
    assert_eq!((levels.levels, levels.level), (3, 1));
    bus::set_keyboard(&client, &client, &levels, 3).unwrap();
    assert_eq!(bus::keyboard(&client, &client).value().unwrap().level, 3);
}

#[test]
fn upower_keyboard_is_the_fallback() {
    let bus = bus_or_skip!();
    let _system =
        fakes::serve_system(bus.builder(), FakeProfiles::owners_machine(), false, true).unwrap();
    let client = bus.connect();
    let Presence::Present(levels) = bus::keyboard(&client, &client) else {
        panic!("UPower has a keyboard backlight")
    };
    assert_eq!(levels.source, KeyboardSource::UPower { max: 3 });
    assert_eq!(levels.level, 1);
    bus::set_keyboard(&client, &client, &levels, 2).unwrap();
    assert_eq!(bus::keyboard(&client, &client).value().unwrap().level, 2);
    assert_eq!(bus::on_battery(&client), Ok(true));
}

#[test]
fn the_whole_machine_reads_from_a_fixture_and_the_stand_ins() {
    let bus = bus_or_skip!();
    let _session = fakes::serve_session(bus.builder(), 70, 0, 4).unwrap();
    let _system =
        fakes::serve_system(bus.builder(), FakeProfiles::owners_machine(), true, false).unwrap();
    let client = bus.connect();
    let root = hardware_platform::sysfs::SysRoot::at(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/owner"),
    );
    let facts = hardware_platform::read_machine(&root, Some(&client), Some(&client));
    let view = hardware_core::page::system_view(&facts);
    assert!(!view.is_empty());
    assert!(view.power.is_some() && view.display.is_some() && view.cooling.is_some());

    let nothing = hardware_platform::sysfs::SysRoot::at(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nothing"),
    );
    let empty_bus = bus_or_skip!();
    let quiet = empty_bus.connect();
    let facts = hardware_platform::read_machine(&nothing, Some(&quiet), Some(&quiet));
    assert!(hardware_core::page::system_view(&facts).is_empty());
}

#[test]
fn a_keyboard_led_with_no_service_is_present_and_unreadable() {
    let bus = bus_or_skip!();
    let quiet = bus.connect();
    let root = hardware_platform::sysfs::SysRoot::at(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/owner"),
    );
    let facts = hardware_platform::read_machine(&root, Some(&quiet), Some(&quiet));
    assert!(matches!(facts.keyboard, Presence::Unreadable(_)));
}

/// A UPower whose display device has `ChargeThresholdEnabled`, as UPower
/// releases after 1.90.3 publish it.
struct ThresholdDevice;

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl ThresholdDevice {
    #[zbus(property)]
    fn charge_threshold_enabled(&self) -> bool {
        true
    }
}

/// Whether UPower tells the desktop about a charge limit decides the System
/// page's sentence about GNOME's own icon (ticket 170). UPower 1.90.3's
/// display device has no such property; without UPower nothing does.
#[test]
fn upower_reports_a_charge_threshold_only_when_its_device_has_the_property() {
    let bus = bus_or_skip!();
    let client = bus.connect();
    assert_eq!(bus::upower_reports_charge_threshold(&client), Some(false));

    let _services =
        fakes::serve_system(bus.builder(), FakeProfiles::owners_machine(), false, false).unwrap();
    assert_eq!(bus::upower_reports_charge_threshold(&client), Some(false));

    let newer = bus_or_skip!();
    let _device = newer
        .builder()
        .serve_at(
            "/org/freedesktop/UPower/devices/DisplayDevice",
            ThresholdDevice,
        )
        .unwrap()
        .name("org.freedesktop.UPower")
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        bus::upower_reports_charge_threshold(&newer.connect()),
        Some(true)
    );
}

/// A bus that can start power-profiles-daemon on demand, as the system bus
/// can: its only service directory holds activation files for both of the
/// daemon's names, whose program leaves a mark when it runs.
fn activatable_bus(dir: &std::path::Path) -> Option<(Child, String, std::path::PathBuf)> {
    let services = dir.join("services");
    std::fs::create_dir_all(&services).ok()?;
    let mark = dir.join("started");
    for name in [
        "org.freedesktop.UPower.PowerProfiles",
        "net.hadess.PowerProfiles",
    ] {
        std::fs::write(
            services.join(format!("{name}.service")),
            format!(
                "[D-BUS Service]\nName={name}\nExec=/bin/sh -c 'touch {}'\n",
                mark.display()
            ),
        )
        .ok()?;
    }
    let config = dir.join("bus.conf");
    std::fs::write(
        &config,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>unix:dir={}</listen>\
             <servicedir>{}</servicedir><policy context=\"default\">\
             <allow send_destination=\"*\"/><allow receive_type=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
            dir.display(),
            services.display()
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
    BufReader::new(child.stdout.take()?)
        .read_line(&mut address)
        .ok()?;
    Some((child, address.trim().to_string(), mark))
}

/// Following the daemon never starts it (ticket 295 review): a stopped
/// daemon, or one its package's removal just stopped, stays stopped and
/// reads as absent, for the System page and the taskbar item alike.
#[test]
fn reading_and_following_the_daemon_never_start_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, address, mark) = test_prerequisites::require!(
        "dbus-daemon",
        activatable_bus(dir.path()).ok_or("a private dbus-daemon did not start")
    );
    let client = zbus::blocking::connection::Builder::address(address.as_str())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(bus::power_profiles(&client), Presence::Absent);
    assert!(bus::set_power_profile(&client, Profile::Balanced).is_err());
    let mut readings = Vec::new();
    let end = bus::watch(&client, &bus::POWER_PROFILES, || {
        readings.push(bus::power_profiles(&client));
        false
    });
    assert_eq!(end, bus::WatchEnd::Stopped);
    assert_eq!(readings, vec![Presence::Absent]);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!mark.exists(), "power-profiles-daemon was started");
    let _ = child.kill();
    let _ = child.wait();
}

/// A follow whose bus goes away says so last, so nothing keeps showing the
/// mode it read before.
#[test]
fn a_follow_whose_bus_closes_ends_unreadable() {
    let bus = bus_or_skip!();
    let _services =
        fakes::serve_system(bus.builder(), FakeProfiles::owners_machine(), false, false).unwrap();
    let client = bus.connect();
    let (sender, readings) = std::sync::mpsc::channel();
    let follower = std::thread::spawn(move || {
        bus::follow_power_profiles(&client, |facts| sender.send(facts).is_ok())
    });
    let first = readings
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(matches!(first, Presence::Present(_)), "{first:?}");
    drop(bus);
    let last = loop {
        match readings.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Presence::Unreadable(why)) => break why,
            Ok(_) => continue,
            Err(error) => panic!("no last word: {error}"),
        }
    };
    assert!(last.contains("closed"), "{last}");
    follower.join().unwrap();
}
