//! "Switch power mode with the charger" (ticket 86, owner 2026-11-13).
//!
//! It runs here because this is the session service the `better-manager`
//! package already ships and Better Manager already keeps running; a new
//! daemon for one decision would be one more process to start and stop. It
//! reacts to UPower's `OnBattery` changing and sets power-profiles-daemon's
//! mode, which needs no root. The decision is
//! `hardware_core::follow::on_power_change`; the person's choice is read
//! from its file at each plug event, so the window never has to tell this
//! service anything and a choice changed a second ago is the one used.

use std::path::{Path, PathBuf};

use hardware_core::follow::on_power_change;
use hardware_core::power::Profile;
use hardware_platform::bus::{self, UPOWER};
use hardware_platform::follow_store;
use zbus::blocking::Connection;

/// What one plug event did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Set(Profile),
    Left,
    Failed(String),
}

/// One `OnBattery` reading after `before`: decides, and sets the mode.
pub fn react(system: &Connection, settings: &Path, before: Option<bool>, now: bool) -> Event {
    let follow = match follow_store::load(settings) {
        Ok(follow) => follow,
        Err(error) => return Event::Failed(error),
    };
    if !follow.enabled || before.is_none_or(|b| b == now) {
        return Event::Left;
    }
    let offered: Vec<Profile> = match bus::power_profiles(system) {
        hardware_core::Presence::Present(facts) => facts
            .profiles
            .iter()
            .filter_map(|p| Profile::from_wire(p))
            .collect(),
        _ => return Event::Failed("power-profiles-daemon does not answer".to_string()),
    };
    match on_power_change(&follow, before, now, &offered) {
        Some(profile) => match bus::set_power_profile(system, profile) {
            Ok(()) => Event::Set(profile),
            Err(error) => Event::Failed(error.details().to_string()),
        },
        None => Event::Left,
    }
}

/// Follows UPower until the connection closes or `report` returns `false`.
/// Each event is reported, so `main` can log it and a test can wait for it.
pub fn follow(system: Connection, settings: PathBuf, mut report: impl FnMut(Event) -> bool) {
    let mut before = bus::on_battery(&system).ok();
    let watcher = system.clone();
    bus::follow(&watcher, &[UPOWER], || {
        let Ok(now) = bus::on_battery(&system) else {
            return true;
        };
        let event = react(&system, &settings, before, now);
        before = Some(now);
        if event == Event::Left {
            return true;
        }
        report(event)
    });
}
