//! The desktop's own services, over connections the caller supplies: the
//! system bus for power-profiles-daemon and UPower, the session bus for
//! gsd-power and GNOME Shell. Plain blocking calls, made off the UI thread.

use std::collections::HashMap;

use hardware_core::Presence;
use hardware_core::display::{KeyboardLevels, KeyboardSource, brightness};
use hardware_core::power::{Hold, PowerProfilesFacts, Profile};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

/// One service object: its bus name, path, and interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub name: &'static str,
    pub path: &'static str,
    pub interface: &'static str,
}

/// power-profiles-daemon under its current name, then the older one it still
/// owns (spec 11.1: 0.21 owns both).
pub const POWER_PROFILES: [Endpoint; 2] = [
    Endpoint {
        name: "org.freedesktop.UPower.PowerProfiles",
        path: "/org/freedesktop/UPower/PowerProfiles",
        interface: "org.freedesktop.UPower.PowerProfiles",
    },
    Endpoint {
        name: "net.hadess.PowerProfiles",
        path: "/net/hadess/PowerProfiles",
        interface: "net.hadess.PowerProfiles",
    },
];

pub const GSD_SCREEN: Endpoint = Endpoint {
    name: "org.gnome.SettingsDaemon.Power",
    path: "/org/gnome/SettingsDaemon/Power",
    interface: "org.gnome.SettingsDaemon.Power.Screen",
};

pub const GSD_KEYBOARD: Endpoint = Endpoint {
    name: "org.gnome.SettingsDaemon.Power",
    path: "/org/gnome/SettingsDaemon/Power",
    interface: "org.gnome.SettingsDaemon.Power.Keyboard",
};

pub const UPOWER_KEYBOARD: Endpoint = Endpoint {
    name: "org.freedesktop.UPower",
    path: "/org/freedesktop/UPower/KbdBacklight",
    interface: "org.freedesktop.UPower.KbdBacklight",
};

pub const UPOWER: Endpoint = Endpoint {
    name: "org.freedesktop.UPower",
    path: "/org/freedesktop/UPower",
    interface: "org.freedesktop.UPower",
};

pub const SHELL: Endpoint = Endpoint {
    name: "org.gnome.Shell",
    path: "/org/gnome/Shell",
    interface: "org.gnome.Shell",
};

/// Why a read did not produce a value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BusError {
    /// Nobody answers for it: the thing is not on this machine.
    Absent(String),
    /// Someone answers and the read failed.
    Failed(String),
}

impl BusError {
    fn from_zbus(error: zbus::Error) -> BusError {
        let text = error.to_string();
        // dbus-daemon passes systemd's own error on when a unit it was asked
        // to activate cannot start (`dbus_activation_systemd_failure` in
        // bus/activation.c hands the ActivationFailure code through; systemd
        // names them in bus-common-errors.h): a masked or missing unit is a
        // service that is not on this machine. dbus-broker answers the same
        // failure with NameHasNoOwner (`driver_name_activation_failed`).
        let absent = [
            "ServiceUnknown",
            "NameHasNoOwner",
            "org.freedesktop.systemd1.UnitMasked",
            "org.freedesktop.systemd1.NoSuchUnit",
            "org.freedesktop.systemd1.LoadFailed",
            "UnknownObject",
            "UnknownInterface",
            "UnknownProperty",
            "UnknownMethod",
            "InvalidArgs: No such interface",
        ];
        if absent.iter().any(|name| text.contains(name)) {
            BusError::Absent(text)
        } else {
            BusError::Failed(text)
        }
    }

    pub fn details(&self) -> &str {
        match self {
            BusError::Absent(text) | BusError::Failed(text) => text,
        }
    }
}

impl<T> From<BusError> for Presence<T> {
    fn from(error: BusError) -> Self {
        match error {
            BusError::Absent(_) => Presence::Absent,
            BusError::Failed(text) => Presence::Unreadable(text),
        }
    }
}

fn get(connection: &Connection, at: &Endpoint, property: &str) -> Result<OwnedValue, BusError> {
    let reply = connection
        .call_method(
            Some(at.name),
            at.path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(at.interface, property),
        )
        .map_err(BusError::from_zbus)?;
    reply
        .body()
        .deserialize::<OwnedValue>()
        .map_err(|error| BusError::Failed(error.to_string()))
}

fn set(
    connection: &Connection,
    at: &Endpoint,
    property: &str,
    value: Value<'_>,
) -> Result<(), BusError> {
    connection
        .call_method(
            Some(at.name),
            at.path,
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &(at.interface, property, value),
        )
        .map(|_| ())
        .map_err(BusError::from_zbus)
}

fn call<B>(
    connection: &Connection,
    at: &Endpoint,
    method: &str,
    body: &B,
) -> Result<zbus::Message, BusError>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    connection
        .call_method(Some(at.name), at.path, Some(at.interface), method, body)
        .map_err(BusError::from_zbus)
}

fn string(value: OwnedValue) -> Result<String, BusError> {
    String::try_from(value).map_err(|error| BusError::Failed(error.to_string()))
}

fn int(value: OwnedValue) -> Result<i32, BusError> {
    i32::try_from(&value)
        .or_else(|_| u32::try_from(&value).map(|v| v as i32))
        .map_err(|error| BusError::Failed(error.to_string()))
}

type Dicts = Vec<HashMap<String, OwnedValue>>;

fn dicts(value: OwnedValue) -> Result<Dicts, BusError> {
    Dicts::try_from(value).map_err(|error| BusError::Failed(error.to_string()))
}

fn entry(dict: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    dict.get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| String::try_from(v).ok())
}

/// Whether a name has an owner now, asked of the bus itself, which never
/// starts anything.
fn has_owner(system: &Connection, name: &str) -> Result<bool, BusError> {
    let reply = system
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(name,),
        )
        .map_err(BusError::from_zbus)?;
    reply
        .body()
        .deserialize::<bool>()
        .map_err(|error| BusError::Failed(error.to_string()))
}

/// The first power-profiles-daemon name that answers.
///
/// Only a daemon that is running is asked: a name with no owner is absent,
/// never started through D-Bus activation. Better Monitor's taskbar item
/// follows the daemon all day, and asking a stopped one would start it again
/// the moment someone stopped it, or its package's removal stopped it
/// (ticket 295 review).
pub fn power_profiles_endpoint(system: &Connection) -> Result<Endpoint, BusError> {
    let mut last = BusError::Absent("power-profiles-daemon".into());
    for endpoint in POWER_PROFILES {
        if !has_owner(system, endpoint.name)? {
            continue;
        }
        match get(system, &endpoint, "ActiveProfile") {
            Ok(_) => return Ok(endpoint),
            Err(error @ BusError::Failed(_)) => return Err(error),
            Err(error) => last = error,
        }
    }
    Err(last)
}

pub fn power_profiles(system: &Connection) -> Presence<PowerProfilesFacts> {
    let read = || -> Result<PowerProfilesFacts, BusError> {
        let at = power_profiles_endpoint(system)?;
        let active = string(get(system, &at, "ActiveProfile")?)?;
        let profiles = dicts(get(system, &at, "Profiles")?)?
            .iter()
            .filter_map(|d| entry(d, "Profile"))
            .collect();
        let degraded = get(system, &at, "PerformanceDegraded")
            .and_then(string)
            .unwrap_or_default();
        let holds = get(system, &at, "ActiveProfileHolds")
            .and_then(dicts)
            .unwrap_or_default()
            .iter()
            .filter_map(|d| {
                Some(Hold {
                    application: entry(d, "ApplicationId")?,
                    profile: entry(d, "Profile")?,
                })
            })
            .collect();
        Ok(PowerProfilesFacts {
            active,
            profiles,
            degraded,
            holds,
        })
    };
    read().map_or_else(Presence::from, Presence::Present)
}

/// Reads power-profiles-daemon now and again after every change it
/// announces, or its name getting or losing an owner, handing each reading
/// to `send` until it returns `false` or the bus closes. The watch is in
/// place before the first read, so no change is missed. Better Monitor's
/// taskbar item and its popup follow the mode in force with it (ticket 295).
///
/// When the watch ends because the bus closed, or could never be set up, a
/// last `Unreadable` says so, so nothing goes on showing a stale mode.
pub fn follow_power_profiles(
    system: &Connection,
    mut send: impl FnMut(Presence<PowerProfilesFacts>) -> bool,
) {
    let end = watch(system, &POWER_PROFILES, || send(power_profiles(system)));
    let why = match end {
        WatchEnd::Stopped => return,
        WatchEnd::Closed => "the system bus connection closed",
        WatchEnd::NotWatched => "power-profiles-daemon's changes cannot be watched",
    };
    send(Presence::Unreadable(why.to_string()));
}

/// Sets the active profile; no root (the daemon's own polkit action is
/// `allow_active=yes`).
pub fn set_power_profile(system: &Connection, profile: Profile) -> Result<(), BusError> {
    let at = power_profiles_endpoint(system)?;
    set(system, &at, "ActiveProfile", Value::from(profile.wire()))
}

pub fn screen_brightness(session: &Connection) -> Presence<u8> {
    match get(session, &GSD_SCREEN, "Brightness").and_then(int) {
        Ok(raw) => match brightness(raw) {
            Some(value) => Presence::Present(value),
            // gsd-power answers -1 when it has no backlight to drive.
            None => Presence::Absent,
        },
        Err(error) => error.into(),
    }
}

pub fn set_screen_brightness(session: &Connection, value: u8) -> Result<(), BusError> {
    set(
        session,
        &GSD_SCREEN,
        "Brightness",
        Value::from(i32::from(value.min(100))),
    )
}

/// gsd-power's keyboard first, UPower's after.
pub fn keyboard(session: &Connection, system: &Connection) -> Presence<KeyboardLevels> {
    let gnome = get(session, &GSD_KEYBOARD, "Brightness")
        .and_then(int)
        .and_then(|percent| {
            let steps = get(session, &GSD_KEYBOARD, "Steps").and_then(int)?;
            Ok((percent, steps))
        });
    match gnome {
        Ok((percent, _)) if percent < 0 => {}
        Ok((percent, steps)) => {
            if let Some(levels) = KeyboardLevels::from_gnome(percent, steps.max(0) as u32) {
                return Presence::Present(levels);
            }
        }
        Err(BusError::Failed(text)) => return Presence::Unreadable(text),
        Err(BusError::Absent(_)) => {}
    }
    let upower = call(system, &UPOWER_KEYBOARD, "GetMaxBrightness", &())
        .and_then(|m| {
            m.body()
                .deserialize::<i32>()
                .map_err(|e| BusError::Failed(e.to_string()))
        })
        .and_then(|max| {
            let level = call(system, &UPOWER_KEYBOARD, "GetBrightness", &())?
                .body()
                .deserialize::<i32>()
                .map_err(|e| BusError::Failed(e.to_string()))?;
            Ok((level, max))
        });
    match upower {
        Ok((level, max)) => {
            KeyboardLevels::from_upower(level, max).map_or(Presence::Absent, Presence::Present)
        }
        Err(error) => error.into(),
    }
}

pub fn set_keyboard(
    session: &Connection,
    system: &Connection,
    levels: &KeyboardLevels,
    level: u32,
) -> Result<(), BusError> {
    let value = levels.wire_value(level);
    match levels.source {
        KeyboardSource::Gnome { .. } => {
            set(session, &GSD_KEYBOARD, "Brightness", Value::from(value))
        }
        KeyboardSource::UPower { .. } => {
            call(system, &UPOWER_KEYBOARD, "SetBrightness", &(value,)).map(|_| ())
        }
    }
}

/// UPower's display device: the battery the desktop's own indicator draws.
pub const UPOWER_DISPLAY_DEVICE: Endpoint = Endpoint {
    name: "org.freedesktop.UPower",
    path: "/org/freedesktop/UPower/devices/DisplayDevice",
    interface: "org.freedesktop.UPower.Device",
};

/// What a window that shows the battery follows to learn that a charger
/// came or went or the battery's state moved, without polling (ticket 170):
/// UPower's `OnBattery` and its display device. UPower reads the kernel's
/// `power_supply` uevents, so a change there reaches these signals.
pub const POWER_SUPPLY_CHANGES: [Endpoint; 2] = [UPOWER, UPOWER_DISPLAY_DEVICE];

/// Whether UPower tells the desktop that a charge limit holds the battery:
/// its display device has `ChargeThresholdEnabled` (UPower 1.90.3, on Ubuntu
/// 24.04, does not; GNOME Shell 49 draws a plugged-in battery from it). `Some(false)` when
/// UPower has no such property or is not running, `None` when the read
/// failed for another reason. A read only.
pub fn upower_reports_charge_threshold(system: &Connection) -> Option<bool> {
    match get(system, &UPOWER_DISPLAY_DEVICE, "ChargeThresholdEnabled") {
        Ok(_) => Some(true),
        Err(BusError::Absent(_)) => Some(false),
        Err(BusError::Failed(text)) if text.contains("No such property") => Some(false),
        Err(BusError::Failed(_)) => None,
    }
}

pub fn on_battery(system: &Connection) -> Result<bool, BusError> {
    bool::try_from(get(system, &UPOWER, "OnBattery")?).map_err(|e| BusError::Failed(e.to_string()))
}

/// `46.0` → `46`.
pub fn shell_major_version(session: &Connection) -> Option<String> {
    let version = string(get(session, &SHELL, "ShellVersion").ok()?).ok()?;
    version
        .split('.')
        .next()
        .map(str::to_string)
        .filter(|v| !v.is_empty())
}

/// Calls `changed` whenever a `PropertiesChanged` arrives from any of
/// `endpoints`, or one of their names gets or loses an owner. Runs until the
/// connection closes or `changed` returns `false`.
pub fn follow(
    connection: &Connection,
    endpoints: &[Endpoint],
    changed: impl FnMut() -> bool,
) -> WatchEnd {
    follow_after(connection, endpoints, false, changed)
}

/// Why a [`follow`] or a [`watch`] ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchEnd {
    /// `changed` returned `false`.
    Stopped,
    /// The connection closed.
    Closed,
    /// Not one match rule could be added, so no change would ever arrive.
    NotWatched,
}

/// [`follow`], calling `changed` once as soon as the watch is in place, so
/// a reader that reads there misses no change between its first reading and
/// the watch (the taskbar item's power mode, ticket 295).
pub fn watch(
    connection: &Connection,
    endpoints: &[Endpoint],
    changed: impl FnMut() -> bool,
) -> WatchEnd {
    follow_after(connection, endpoints, true, changed)
}

fn follow_after(
    connection: &Connection,
    endpoints: &[Endpoint],
    now: bool,
    mut changed: impl FnMut() -> bool,
) -> WatchEnd {
    let (merged, events) = std::sync::mpsc::channel::<()>();
    let mut rules = Vec::new();
    for at in endpoints {
        rules.push(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.freedesktop.DBus.Properties")
                .and_then(|r| r.member("PropertiesChanged"))
                .and_then(|r| r.path(at.path))
                .map(|r| r.build()),
        );
        rules.push(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")
                .and_then(|r| r.interface("org.freedesktop.DBus"))
                .and_then(|r| r.member("NameOwnerChanged"))
                .and_then(|r| r.add_arg(at.name))
                .map(|r| r.build()),
        );
    }
    let mut watched = 0;
    for rule in rules.into_iter().flatten() {
        if let Ok(stream) =
            zbus::blocking::MessageIterator::for_match_rule(rule, connection, Some(16))
        {
            watched += 1;
            let merged = merged.clone();
            std::thread::spawn(move || {
                for message in stream {
                    if message.is_err() || merged.send(()).is_err() {
                        return;
                    }
                }
            });
        }
    }
    drop(merged);
    if now && !changed() {
        return WatchEnd::Stopped;
    }
    if watched == 0 {
        return WatchEnd::NotWatched;
    }
    while events.recv().is_ok() {
        if !changed() {
            return WatchEnd::Stopped;
        }
    }
    WatchEnd::Closed
}
