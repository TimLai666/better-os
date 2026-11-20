//! System (ticket 86, spec sections 4 and 11): the hardware controls, and
//! which drives mount automatically (ticket 127). Power
//! mode, the charger switch, the battery charge limit, the battery, display
//! brightness, the keyboard backlight and its colour (ticket 136), fan mode,
//! temperatures and fans, and About this PC, each drawn only where the
//! machine has it.
//!
//! What is drawn is decided by `hardware-core`; `decide` gives it words,
//! `state` reads and writes off the UI thread, and `render` draws it with
//! the Fluent kit. Every root write goes to `manager-daemon` over D-Bus.

pub(crate) mod copy;
pub(crate) mod decide;
pub(crate) mod drives;
pub(crate) mod lighting;
pub(crate) mod lighting_card;
pub(crate) mod render;
pub(crate) mod state;
#[cfg(test)]
mod tests;

use crate::attention::{AttentionAction, AttentionItem, Severity, Snapshot, row};
use crate::i18n::{Locale, shell};
use crate::navigation::Route;
use crate::pages::{PageModule, PageStatus};
use crate::search::SearchEntry;

/// The ids of the page's cards, which the search scrolls to.
pub(crate) mod card {
    pub(crate) const POWER_MODE: &str = "system-power-mode";
    pub(crate) const FOLLOW: &str = "system-follow-charger";
    pub(crate) const CHARGE_LIMIT: &str = "system-charge-limit";
    pub(crate) const BATTERY: &str = "system-battery";
    pub(crate) const BRIGHTNESS: &str = "system-brightness";
    pub(crate) const KEYBOARD: &str = "system-keyboard";
    pub(crate) const LIGHTING: &str = "system-keyboard-colour";
    pub(crate) const FAN_MODE: &str = "system-fan-mode";
    pub(crate) const SENSORS: &str = "system-sensors";
    pub(crate) const ABOUT: &str = "system-about";
}

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::System,
    name: "system",
    status: PageStatus::Built,
    title: |s| s.system,
    search_entries,
    attention,
    badge: |_| None,
};

fn search_entries(locale: Locale) -> Vec<SearchEntry> {
    let c = copy::copy(locale);
    let page = shell(locale).system;
    let entry = |header, description, group, card| SearchEntry {
        header,
        description,
        page,
        group: Some(group),
        route: Route::System,
        card,
    };
    vec![
        entry(
            c.power_mode,
            c.power_mode_description,
            c.group_power,
            card::POWER_MODE,
        ),
        entry(
            c.follow_header,
            c.follow_description,
            c.group_power,
            card::FOLLOW,
        ),
        entry(
            c.charge_limit,
            c.charge_limit_description,
            c.group_power,
            card::CHARGE_LIMIT,
        ),
        entry(c.battery, c.home_worn_header, c.group_power, card::BATTERY),
        entry(
            c.brightness,
            c.brightness_description,
            c.group_display,
            card::BRIGHTNESS,
        ),
        entry(c.keyboard, "", c.group_display, card::KEYBOARD),
        {
            let l = lighting::copy(locale);
            entry(l.header, l.search, c.group_display, card::LIGHTING)
        },
        entry(
            c.fan_mode,
            c.fan_mode_description,
            c.group_cooling,
            card::FAN_MODE,
        ),
        entry(c.sensors, "", c.group_cooling, card::SENSORS),
        {
            let d = drives::copy(locale);
            entry(d.mount_automatically, d.description, d.group, drives::CARD)
        },
        entry(c.about, "", c.group_about, card::ABOUT),
    ]
}

/// Home's System item (spec section 3): the battery holds under 80% of what
/// it was designed to.
fn attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    let c = copy::copy(snapshot.locale);
    match snapshot.battery_health {
        Some(percent) if percent < hardware_core::battery::WEARING_OUT_BELOW_PERCENT => {
            vec![AttentionItem {
                source: Route::System,
                severity: Severity::Warning,
                row: row::SYSTEM_BATTERY_WORN,
                header: c.home_worn_header.to_string(),
                description: c
                    .home_worn_description
                    .replace("{percent}", &percent.to_string()),
                action_label: c.view,
                action: AttentionAction::OpenSystemBattery,
            }]
        }
        _ => Vec::new(),
    }
}
