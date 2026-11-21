//! What the System page says, without GPUI: the labels of every choice and
//! reading, the descriptions, the About rows, and which state bar a write's
//! outcome draws. `hardware-core` decides what is drawn; this decides the
//! words.

use better_core::CurrentDesktop;
use better_ui::fluent::words;
use hardware_core::battery::BatteryFacts;
use hardware_core::charge::ChargeChoice;
use hardware_core::display::KeyboardLevels;
use hardware_core::fan::FanMode;
use hardware_core::power::{Caution, Degradation, Profile};
use hardware_core::sensors::{SensorLabel, SensorRow, SensorSummary};
use hardware_core::write::WriteOutcome;

use super::copy::{Copy, copy};
use crate::home::battery_look;
use crate::i18n::Locale;
use crate::pages::states::PageState;
use hardware_core::charge_state::{ChargeLimit, ChargeReadings};

/// Sentences one after another: a space between them in English, nothing
/// between them in Chinese, where a full stop already ends each one.
pub(crate) fn sentences(locale: Locale, parts: &[&str]) -> String {
    let separator = match locale.resolved() {
        Locale::ZhTw => "",
        Locale::EnUs | Locale::System => " ",
    };
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(separator)
}

pub(crate) fn profile_label(profile: Profile, c: &Copy) -> &'static str {
    profile.name(c.language)
}

/// A profile as the daemon names it, in words when this version knows it.
fn profile_name(name: &str, c: &Copy) -> String {
    Profile::from_wire(name)
        .map(|p| profile_label(p, c).to_string())
        .unwrap_or_else(|| name.to_string())
}

/// The caution lines under Power mode.
pub(crate) fn caution_line(caution: &Caution, c: &Copy) -> String {
    match caution {
        Caution::Held(hold) => c
            .held
            .replace("{application}", &hold.application)
            .replace("{mode}", &profile_name(&hold.profile, c)),
        Caution::Degraded(Degradation::LapDetected) => c.degraded_lap.to_string(),
        Caution::Degraded(Degradation::HighTemperature) => c.degraded_hot.to_string(),
        Caution::Degraded(Degradation::Other(reason)) => {
            c.degraded_other.replace("{reason}", reason)
        }
    }
}

pub(crate) fn charge_label(choice: ChargeChoice, c: &Copy) -> String {
    match choice {
        ChargeChoice::Limit(100) => c.charge_100.to_string(),
        ChargeChoice::Limit(80) => c.charge_80.to_string(),
        ChargeChoice::Limit(60) => c.charge_60.to_string(),
        ChargeChoice::Limit(n) | ChargeChoice::SetElsewhere(n) => {
            c.charge_other.replace("{n}", &n.to_string())
        }
    }
}

/// `87% · 充電中`, or the state alone when the charge could not be read.
/// The state is the battery icon rule's (`hardware_core::charge_state`),
/// with the limit the page read beside the battery.
pub(crate) fn battery_description(charge: &ChargeReadings, locale: Locale) -> String {
    let status = battery_look(charge, locale).words;
    match charge.percent() {
        Some(percent) => format!("{percent}% · {status}"),
        None => status,
    }
}

/// The sentence the charge limit card adds after its description, while a
/// limit under 100% holds a battery and GNOME's own battery icon cannot show
/// it (ticket 170).
pub(crate) fn charge_limit_lines(
    charge: Option<&ChargeReadings>,
    desktop_icon_note: bool,
    c: &Copy,
) -> Vec<String> {
    let below_full = charge.is_some_and(|charge| {
        charge
            .batteries
            .iter()
            .any(|b| matches!(b.limit, ChargeLimit::Percent { end, .. } if end < 100))
    });
    if desktop_icon_note && below_full {
        vec![c.charge_desktop_icon.to_string()]
    } else {
        Vec::new()
    }
}

/// Whether GNOME's own battery icon draws a battery held at its limit like
/// one running the machine, which is what the charge limit card says.
///
/// GNOME Shell up to 48 adds a mark only for UPower's `charging` and draws
/// `pending-charge` as the plain level icon; 49 draws `-plugged-in` for it
/// when UPower's display device has `ChargeThresholdEnabled`
/// (`engineering/specs/battery-charge-state.md`). So the sentence is true on
/// GNOME when the shell is older than 49, or when UPower publishes no such
/// property; without either reading it is not said.
pub(crate) fn desktop_icon_note(
    on_gnome: bool,
    shell_major: Option<u32>,
    upower_reports_threshold: Option<bool>,
) -> bool {
    on_gnome
        && (upower_reports_threshold == Some(false) || shell_major.is_some_and(|major| major < 49))
}

pub(crate) fn battery_rows(battery: &BatteryFacts, c: &Copy) -> Vec<String> {
    use hardware_core::battery::BatteryRow;
    battery
        .rows()
        .into_iter()
        .map(|row| match row {
            BatteryRow::Health(p) => c.battery_health.replace("{percent}", &p.to_string()),
            BatteryRow::Cycles(n) => c.battery_cycles.replace("{n}", &n.to_string()),
            BatteryRow::Model(m) => c.battery_model.replace("{model}", &m),
        })
        .collect()
}

pub(crate) fn keyboard_value(level: u32, c: &Copy) -> String {
    if level == 0 {
        c.keyboard_off.to_string()
    } else {
        c.keyboard_level.replace("{n}", &level.to_string())
    }
}

pub(crate) fn keyboard_description(levels: &KeyboardLevels, c: &Copy) -> String {
    c.keyboard_description
        .replace("{levels}", &levels.levels.to_string())
}

pub(crate) fn fan_label(mode: FanMode, c: &Copy) -> &'static str {
    match mode {
        FanMode::Auto => c.fan_auto,
        FanMode::Full => c.fan_full,
    }
}

pub(crate) fn sensor_label(label: &SensorLabel, c: &Copy) -> String {
    match label {
        SensorLabel::Processor => c.processor.to_string(),
        SensorLabel::Graphics => c.graphics.to_string(),
        SensorLabel::Ssd => c.ssd.to_string(),
        SensorLabel::Motherboard => c.motherboard.to_string(),
        SensorLabel::WiFi => c.wifi.to_string(),
        SensorLabel::CpuFan => c.cpu_fan.to_string(),
        SensorLabel::Raw(raw) => raw.clone(),
    }
}

pub(crate) fn sensor_value(row: &SensorRow, c: &Copy) -> String {
    use hardware_core::sensors::SensorKind;
    match (row.kind, row.value) {
        (_, None) => c.sensors_unreadable.to_string(),
        (SensorKind::Temperature, Some(v)) => words::celsius(v),
        (SensorKind::Fan, Some(v)) => words::rpm(v),
    }
}

/// `處理器 52 °C · 風扇 4,800 RPM`; either half alone when the other has
/// nothing to say, and nothing when neither does.
pub(crate) fn sensors_description(summary: &SensorSummary, c: &Copy) -> String {
    let hottest = summary
        .hottest
        .as_ref()
        .map(|(label, v)| format!("{} {}", sensor_label(label, c), words::celsius(*v)));
    let fan = summary.fan_rpm.map(words::rpm);
    match (hottest, fan) {
        (Some(hottest), Some(fan)) => c
            .sensors_description
            .replace("{hottest}", &hottest)
            .replace("{rpm}", &fan),
        (Some(hottest), None) => hottest,
        (None, Some(fan)) => c.sensors_fan_only.replace("{rpm}", &fan),
        (None, None) => String::new(),
    }
}

/// The sentence after "That didn't change." for a failed write, read from
/// the technical chain; empty when nothing more can be said in words.
pub(crate) fn failure_reason(details: &str, c: &Copy) -> &'static str {
    use hardware_core::write::keys;
    if details.contains(keys::HARDWARE_ABSENT) {
        c.reason_absent
    } else if details.contains(keys::WRITE_FAILED) {
        c.reason_write
    } else if [
        "ServiceUnknown",
        "NameHasNoOwner",
        "UnknownInterface",
        "UnknownMethod",
        "not reachable",
    ]
    .iter()
    .any(|name| details.contains(name))
    {
        c.reason_no_service
    } else {
        ""
    }
}

/// The state a write's outcome draws, or `None` when it needs no bar.
pub(crate) fn outcome_state(outcome: &WriteOutcome, locale: Locale) -> Option<PageState> {
    match outcome {
        WriteOutcome::Done => None,
        WriteOutcome::ReadBack(value) => Some(PageState::ReadBackDifferent {
            value: value.clone(),
        }),
        WriteOutcome::Cancelled => Some(PageState::NoPermission),
        WriteOutcome::NotAllowed => Some(PageState::NotAllowed),
        WriteOutcome::Failed { details } => Some(PageState::WriteFailed {
            reason: failure_reason(details, copy(locale)).to_string(),
        }),
    }
}

/// What About this PC reads beyond the files.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AboutFacts {
    pub(crate) product: Option<String>,
    pub(crate) system: Option<String>,
    pub(crate) architecture: Option<String>,
    pub(crate) kernel: Option<String>,
    pub(crate) desktop: Option<String>,
}

/// `GNOME 46 · Wayland` from the session's desktops (`zorin:GNOME`, read by
/// `better_core::desktop`), the shell's own major version, and
/// `XDG_SESSION_TYPE`. The line names the desktop itself, which a distribution
/// lists after its own name, so it is the last entry. Whatever is missing is
/// left out; nothing missing is filled in.
pub(crate) fn desktop_line(
    current_desktop: &CurrentDesktop,
    shell_major: Option<&str>,
    session_type: Option<&str>,
) -> Option<String> {
    let desktop = current_desktop
        .names()
        .last()
        .map(|d| match (d.as_str(), shell_major) {
            ("GNOME", Some(major)) => format!("GNOME {major}"),
            (other, _) => other.to_string(),
        });
    let session = session_type.and_then(|s| match s.trim() {
        "wayland" => Some("Wayland".to_string()),
        "x11" => Some("X11".to_string()),
        "" | "tty" => None,
        other => Some(other.to_string()),
    });
    match (desktop, session) {
        (Some(d), Some(s)) => Some(format!("{d} · {s}")),
        (Some(d), None) => Some(d),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

/// The About rows: name and value, a value that could not be read said so.
pub(crate) fn about_rows(about: &AboutFacts, locale: Locale) -> Vec<(&'static str, String)> {
    let c = copy(locale);
    let value = |v: &Option<String>| v.clone().unwrap_or_else(|| c.about_unknown.to_string());
    vec![
        (c.about_device, value(&about.product)),
        (c.about_system, value(&about.system)),
        (c.about_architecture, value(&about.architecture)),
        (c.about_kernel, value(&about.kernel)),
        (c.about_desktop, value(&about.desktop)),
    ]
}
