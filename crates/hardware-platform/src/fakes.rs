//! Stand-ins for power-profiles-daemon, UPower, and gsd-power, for tests on
//! a private bus and for screenshots in a sandbox. Never built into a
//! shipped binary: only the `test-support` feature compiles this.

use std::collections::HashMap;

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

fn dict(pairs: &[(&str, &str)]) -> HashMap<String, OwnedValue> {
    pairs
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                OwnedValue::try_from(Value::from(*v)).expect("a string is a value"),
            )
        })
        .collect()
}

/// power-profiles-daemon.
pub struct FakeProfiles {
    pub active: String,
    pub profiles: Vec<String>,
    pub degraded: String,
    /// (application, profile)
    pub holds: Vec<(String, String)>,
}

impl FakeProfiles {
    pub fn owners_machine() -> FakeProfiles {
        FakeProfiles {
            active: "balanced".into(),
            profiles: vec![
                "power-saver".into(),
                "balanced".into(),
                "performance".into(),
            ],
            degraded: String::new(),
            holds: Vec::new(),
        }
    }
}

macro_rules! profiles_interface {
    ($ty:ident, $name:literal) => {
        pub struct $ty(pub FakeProfiles);

        #[zbus::interface(name = $name)]
        impl $ty {
            #[zbus(property)]
            fn active_profile(&self) -> String {
                self.0.active.clone()
            }

            #[zbus(property)]
            fn set_active_profile(&mut self, profile: String) {
                self.0.active = profile;
            }

            #[zbus(property)]
            fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
                self.0
                    .profiles
                    .iter()
                    .map(|p| dict(&[("Profile", p), ("Driver", "platform_profile")]))
                    .collect()
            }

            #[zbus(property)]
            fn performance_degraded(&self) -> String {
                self.0.degraded.clone()
            }

            #[zbus(property)]
            fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedValue>> {
                self.0
                    .holds
                    .iter()
                    .map(|(app, p)| dict(&[("ApplicationId", app), ("Profile", p), ("Reason", "")]))
                    .collect()
            }
        }
    };
}

profiles_interface!(CurrentProfiles, "org.freedesktop.UPower.PowerProfiles");
profiles_interface!(OlderProfiles, "net.hadess.PowerProfiles");

/// UPower's `OnBattery`.
pub struct FakeUPower {
    pub on_battery: bool,
}

#[zbus::interface(name = "org.freedesktop.UPower")]
impl FakeUPower {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        self.on_battery
    }
}

/// UPower's `KbdBacklight`.
pub struct FakeKbdBacklight {
    pub level: i32,
    pub max: i32,
}

#[zbus::interface(name = "org.freedesktop.UPower.KbdBacklight")]
impl FakeKbdBacklight {
    fn get_brightness(&self) -> i32 {
        self.level
    }

    fn get_max_brightness(&self) -> i32 {
        self.max
    }

    fn set_brightness(&mut self, value: i32) {
        self.level = value.clamp(0, self.max);
    }
}

/// gsd-power's `Screen`.
pub struct FakeScreen {
    pub brightness: i32,
}

#[zbus::interface(name = "org.gnome.SettingsDaemon.Power.Screen")]
impl FakeScreen {
    #[zbus(property)]
    fn brightness(&self) -> i32 {
        self.brightness
    }

    #[zbus(property)]
    fn set_brightness(&mut self, value: i32) {
        self.brightness = value.clamp(0, 100);
    }
}

/// gsd-power's `Keyboard`.
pub struct FakeKeyboard {
    pub brightness: i32,
    pub steps: i32,
}

#[zbus::interface(name = "org.gnome.SettingsDaemon.Power.Keyboard")]
impl FakeKeyboard {
    #[zbus(property)]
    fn brightness(&self) -> i32 {
        self.brightness
    }

    #[zbus(property)]
    fn set_brightness(&mut self, value: i32) {
        self.brightness = value.clamp(0, 100);
    }

    #[zbus(property)]
    fn steps(&self) -> i32 {
        self.steps
    }
}

/// Serves power-profiles-daemon under its current name (and, with `older`,
/// the older one too), UPower with a keyboard backlight, on `builder`'s bus.
pub fn serve_system(
    builder: zbus::blocking::connection::Builder<'_>,
    profiles: FakeProfiles,
    older: bool,
    on_battery: bool,
) -> zbus::Result<Connection> {
    let older_copy = FakeProfiles {
        active: profiles.active.clone(),
        profiles: profiles.profiles.clone(),
        degraded: profiles.degraded.clone(),
        holds: profiles.holds.clone(),
    };
    let mut builder = builder
        .serve_at(
            "/org/freedesktop/UPower/PowerProfiles",
            CurrentProfiles(profiles),
        )?
        .serve_at("/org/freedesktop/UPower", FakeUPower { on_battery })?
        .serve_at(
            "/org/freedesktop/UPower/KbdBacklight",
            FakeKbdBacklight { level: 1, max: 3 },
        )?
        .name("org.freedesktop.UPower.PowerProfiles")?
        .name("org.freedesktop.UPower")?;
    if older {
        builder = builder
            .serve_at("/net/hadess/PowerProfiles", OlderProfiles(older_copy))?
            .name("net.hadess.PowerProfiles")?;
    }
    builder.build()
}

/// Serves only the older power-profiles-daemon name, as 0.10-era daemons do.
pub fn serve_older_profiles_only(
    builder: zbus::blocking::connection::Builder<'_>,
    profiles: FakeProfiles,
) -> zbus::Result<Connection> {
    builder
        .serve_at("/net/hadess/PowerProfiles", OlderProfiles(profiles))?
        .name("net.hadess.PowerProfiles")?
        .build()
}

/// Serves gsd-power's screen and keyboard on `builder`'s bus.
pub fn serve_session(
    builder: zbus::blocking::connection::Builder<'_>,
    brightness: i32,
    keyboard_percent: i32,
    steps: i32,
) -> zbus::Result<Connection> {
    builder
        .serve_at("/org/gnome/SettingsDaemon/Power", FakeScreen { brightness })?
        .serve_at(
            "/org/gnome/SettingsDaemon/Power",
            FakeKeyboard {
                brightness: keyboard_percent,
                steps,
            },
        )?
        .name("org.gnome.SettingsDaemon.Power")?
        .build()
}
