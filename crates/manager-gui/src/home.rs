//! Home (spec section 3): the page the control center opens on, answering
//! "does anything here need me?". Everything Home decides is here, without
//! GPUI: the device banner read from DMI and the battery, which attention
//! items replace "Everything's fine", the quick settings, and which activity
//! entries are recent. The window only draws what this says.

use std::path::PathBuf;

use manager_core::{ActivityKind, ActivityRecord};

use crate::i18n::{Locale, shell};
use crate::navigation::Route;
use crate::pages::{PageModule, PageStatus};
use crate::search::SearchEntry;

/// How many activity entries Home lists.
pub(crate) const RECENT_ENTRIES: usize = 5;

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::Home,
    name: "home",
    status: PageStatus::Built,
    title: |s| s.home,
    // Home's cards repeat other pages' settings; the search finds them on
    // the page that owns them.
    search_entries: |_: Locale| Vec::<SearchEntry>::new(),
    attention: |_| Vec::new(),
    badge: |_| None,
};

/// What the banner says about this computer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Device {
    /// The product name, with a repeated model suffix removed.
    pub(crate) product: Option<String>,
    /// The distribution's own name for itself, `Zorin OS 18.1`.
    pub(crate) distribution: Option<String>,
    pub(crate) battery: Option<Battery>,
}

/// The battery's charge and what it is doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Battery {
    pub(crate) percent: u8,
    /// What the battery icon rule reads (`hardware_core::charge_state`).
    pub(crate) charge: ChargeReadings,
}

/// The kernel's `status` words for a battery, read in one place for Home and
/// System (`hardware-core`).
#[cfg(test)]
pub(crate) use hardware_core::battery::BatteryStatus;
use hardware_core::charge_state::{BatteryLook, ChargeReadings, Language};

/// What a battery icon shows, from the one rule every battery icon in Better
/// OS follows (ticket 170): Home's banner, the System page's battery and
/// charge limit cards, and the Keep awake page.
pub(crate) fn battery_look(readings: &ChargeReadings, locale: Locale) -> BatteryLook {
    hardware_core::charge_state::battery_look(
        readings,
        match locale.resolved() {
            Locale::ZhTw => Language::TraditionalChinese,
            Locale::EnUs | Locale::System => Language::English,
        },
    )
}

/// The picture for a look, from `better-ui`'s icon set. Every battery icon
/// the rule names has a drawing there (a test checks it); the empty battery
/// stands for one that did not.
pub(crate) fn battery_picture(look: &BatteryLook) -> better_ui::icon_set::Picture {
    better_ui::icon_set::Picture::from_path(look.icon.asset_path())
        .unwrap_or(better_ui::icon_set::Picture::Battery)
}

/// Values firmware writes when nobody filled the field in.
const PLACEHOLDER_NAMES: [&str; 6] = [
    "to be filled by o.e.m.",
    "system product name",
    "default string",
    "not applicable",
    "none",
    "",
];

/// The product name DMI reports, as a person would write it.
///
/// Some vendors repeat the model after an underscore
/// (`ASUS Vivobook S 14 M5406WA_M5406WA`); the repeat is removed. A name the
/// firmware left as a placeholder is no name.
pub(crate) fn product_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    if PLACEHOLDER_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
        return None;
    }
    let name = match name.rsplit_once('_') {
        Some((head, tail))
            if !tail.is_empty()
                && head
                    .split_whitespace()
                    .last()
                    .is_some_and(|word| word.eq_ignore_ascii_case(tail)) =>
        {
            head.trim_end()
        }
        _ => name,
    };
    Some(name.to_string())
}

/// `PRETTY_NAME` from an `os-release` file, unquoted by the one field
/// reader `better_core::host` has.
pub(crate) fn pretty_name(os_release: &str) -> Option<String> {
    better_core::host::os_release_field(os_release, "PRETTY_NAME")
}

/// Where the banner reads from. `/` on a real machine; a fixture tree in a
/// test.
#[derive(Clone, Debug)]
pub(crate) struct HostRoot(pub(crate) PathBuf);

impl Default for HostRoot {
    fn default() -> Self {
        HostRoot(PathBuf::from("/"))
    }
}

impl HostRoot {
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(path.trim_start_matches('/'))).ok()
    }

    /// Reads the banner's facts. Every read is of a world-readable file and
    /// changes nothing; anything missing is left out rather than guessed.
    pub(crate) fn device(&self) -> Device {
        Device {
            product: self
                .read("/sys/class/dmi/id/product_name")
                .and_then(|raw| product_name(&raw)),
            // Read by better_core::host, the one reader of the file.
            distribution: better_core::host::read_os_release_under(&self.0)
                .ok()
                .and_then(|text| pretty_name(&text)),
            battery: self.battery(),
        }
    }

    /// The system battery, as the System page reads it
    /// (`hardware_platform::sysfs::battery`): not a mouse's, and only when its
    /// charge can be read.
    pub(crate) fn battery(&self) -> Option<Battery> {
        let root = hardware_platform::sysfs::SysRoot::at(&self.0);
        let charge = hardware_platform::sysfs::charge_readings(&root)?;
        Some(Battery {
            percent: charge.percent?.min(100),
            charge,
        })
    }
}

/// The banner's second line: `{distribution} · Better OS {version}`, or the
/// version alone when the distribution could not be read.
pub(crate) fn banner_line(locale: Locale, distribution: Option<&str>, version: &str) -> String {
    match distribution {
        Some(distribution) => shell(locale)
            .banner_line
            .replace("{distribution}", distribution)
            .replace("{version}", version),
        None => format!("Better OS {version}"),
    }
}

/// The battery's words at the banner's trailing edge: `87% · 充電中`.
pub(crate) fn battery_line(locale: Locale, battery: Battery) -> String {
    format!(
        "{}% · {}",
        battery.percent,
        battery_look(&battery.charge, locale).words
    )
}

/// Whether the first-run info bar is shown: until it is closed once.
pub(crate) fn shows_welcome(onboarding_complete: bool) -> bool {
    !onboarding_complete
}

/// The quick settings Home repeats, in the spec's order. A page ticket adds
/// its card here (Power mode, ticket 86; Keep awake, ticket 87) and Home
/// leaves out a card whose hardware or component is missing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickSetting {
    /// System's Power mode combo box (ticket 86), drawn only where
    /// power-profiles-daemon answers.
    PowerMode,
    /// Keep awake's toggle (ticket 87), drawn only while its service runs.
    KeepAwake,
    Appearance,
}

pub(crate) fn quick_settings() -> Vec<QuickSetting> {
    vec![
        QuickSetting::PowerMode,
        QuickSetting::KeepAwake,
        QuickSetting::Appearance,
    ]
}

/// The tone an activity entry is drawn in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tone {
    Information,
    Success,
    Warning,
    Failure,
}

pub(crate) fn tone(kind: ActivityKind) -> Tone {
    match kind {
        ActivityKind::Information => Tone::Information,
        ActivityKind::Success | ActivityKind::RecoverySuccess => Tone::Success,
        ActivityKind::Warning | ActivityKind::RecoveryPartial | ActivityKind::ManualRecovery => {
            Tone::Warning
        }
        ActivityKind::Failure => Tone::Failure,
    }
}

/// The entries Home lists, newest first.
pub(crate) fn recent(activity: &[ActivityRecord]) -> Vec<&ActivityRecord> {
    activity.iter().rev().take(RECENT_ENTRIES).collect()
}

/// How long ago something happened, in the coarsest unit that still says
/// something. A clock that ran backwards reads as "just now".
pub(crate) fn ago(locale: Locale, now_unix_seconds: u64, then_unix_seconds: u64) -> String {
    let s = shell(locale);
    let seconds = now_unix_seconds.saturating_sub(then_unix_seconds);
    let (template, value) = match seconds {
        0..=59 => return s.ago_just_now.to_string(),
        60..=3599 => (s.ago_minutes, seconds / 60),
        3600..=86_399 => (s.ago_hours, seconds / 3600),
        _ => (s.ago_days, seconds / 86_400),
    };
    template.replace("{n}", &value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_repeated_model_suffix_is_dropped_from_the_product_name() {
        assert_eq!(
            product_name("ASUS Vivobook S 14 M5406WA_M5406WA\n").as_deref(),
            Some("ASUS Vivobook S 14 M5406WA")
        );
        assert_eq!(product_name("XPS 13 9310").as_deref(), Some("XPS 13 9310"));
        // An underscore that does not repeat the model stays.
        assert_eq!(
            product_name("Thing_Model X").as_deref(),
            Some("Thing_Model X")
        );
        assert_eq!(product_name("To Be Filled By O.E.M."), None);
        assert_eq!(product_name("System Product Name"), None);
        assert_eq!(product_name("   "), None);
    }

    #[test]
    fn the_distribution_is_its_pretty_name() {
        let zorin =
            "NAME=\"Zorin OS\"\nVERSION=\"18.1\"\nPRETTY_NAME=\"Zorin OS 18.1\"\nID=zorin\n";
        assert_eq!(pretty_name(zorin).as_deref(), Some("Zorin OS 18.1"));
        assert_eq!(
            pretty_name("PRETTY_NAME=Ubuntu 24.04.1 LTS").as_deref(),
            Some("Ubuntu 24.04.1 LTS")
        );
        assert_eq!(pretty_name("NAME=x\n"), None);
        assert_eq!(pretty_name("PRETTY_NAME=\"\""), None);
    }

    #[test]
    fn the_banner_falls_back_to_the_vendor_os_release_like_every_other_reader() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            "usr/lib/os-release",
            "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n",
        );
        let device = HostRoot(root.path().to_path_buf()).device();
        assert_eq!(device.distribution.as_deref(), Some("Ubuntu 24.04.1 LTS"));
    }

    #[test]
    fn the_banner_line_names_the_distribution_and_the_version() {
        assert_eq!(
            banner_line(Locale::ZhTw, Some("Zorin OS 18.1"), "0.2.10"),
            "Zorin OS 18.1 · Better OS 0.2.10"
        );
        assert_eq!(
            banner_line(Locale::EnUs, None, "0.2.10"),
            "Better OS 0.2.10"
        );
    }

    fn battery(percent: u8, status: BatteryStatus, charger: bool, limit: u8) -> Battery {
        Battery {
            percent,
            charge: ChargeReadings {
                charger_online: Some(charger),
                status: Some(status),
                percent: Some(percent),
                limit: hardware_core::charge_state::ChargeLimit::Percent {
                    end: limit,
                    start: None,
                },
            },
        }
    }

    #[test]
    fn the_battery_says_its_charge_and_what_it_is_doing_in_both_languages() {
        assert_eq!(
            battery_line(
                Locale::ZhTw,
                battery(87, BatteryStatus::Charging, true, 100)
            ),
            "87% · 充電中"
        );
        assert_eq!(
            battery_line(
                Locale::EnUs,
                battery(87, BatteryStatus::Discharging, false, 100)
            ),
            "87% · On battery"
        );
        assert_eq!(
            battery_line(Locale::EnUs, battery(100, BatteryStatus::Full, true, 100)),
            "100% · Plugged in, fully charged"
        );
        assert_eq!(
            battery_line(Locale::ZhTw, battery(100, BatteryStatus::Full, true, 100)),
            "100% · 已接上電源，已充滿"
        );
        // The owner's machine on 2026-11-21 (ticket 170).
        let held = battery(80, BatteryStatus::NotCharging, true, 80);
        assert_eq!(
            battery_line(Locale::ZhTw, held),
            "80% · 已接上電源，維持在 80%"
        );
        assert_eq!(
            battery_line(Locale::EnUs, held),
            "80% · Plugged in, holding at 80%"
        );
        assert_eq!(
            battery_picture(&battery_look(&held.charge, Locale::EnUs)),
            better_ui::icon_set::Picture::BatteryPlugged
        );
        assert_eq!(
            BatteryStatus::parse("Not charging\n"),
            BatteryStatus::NotCharging
        );
        assert_eq!(BatteryStatus::parse("Bogus"), BatteryStatus::Unknown);
    }

    fn write(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// The owner's machine as read on 2026-11-12, plus a Bluetooth mouse that
    /// reports a battery of its own.
    #[test]
    fn the_banner_reads_dmi_os_release_and_the_system_battery() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        write(
            r,
            "sys/class/dmi/id/product_name",
            "ASUS Vivobook S 14 M5406WA_M5406WA\n",
        );
        write(r, "etc/os-release", "PRETTY_NAME=\"Zorin OS 18.1\"\n");
        write(r, "sys/class/power_supply/ACAD/type", "Mains\n");
        write(r, "sys/class/power_supply/BAT1/type", "Battery\n");
        write(r, "sys/class/power_supply/BAT1/scope", "System\n");
        write(r, "sys/class/power_supply/BAT1/capacity", "100\n");
        write(r, "sys/class/power_supply/BAT1/status", "Full\n");
        write(r, "sys/class/power_supply/AAA-mouse/type", "Battery\n");
        write(r, "sys/class/power_supply/AAA-mouse/scope", "Device\n");
        write(r, "sys/class/power_supply/AAA-mouse/capacity", "20\n");
        let device = HostRoot(r.to_path_buf()).device();
        assert_eq!(
            device.product.as_deref(),
            Some("ASUS Vivobook S 14 M5406WA")
        );
        assert_eq!(device.distribution.as_deref(), Some("Zorin OS 18.1"));
        let battery = device.battery.expect("BAT1 is the system battery");
        assert_eq!(battery.percent, 100);
        assert_eq!(battery.charge.status, Some(BatteryStatus::Full));
        // ACAD has no `online` here, so whether it is plugged in is unknown.
        assert_eq!(battery.charge.charger_online, None);
    }

    #[test]
    fn every_battery_icon_the_rule_names_has_a_drawing() {
        for icon in hardware_core::charge_state::BatteryIcon::ALL {
            assert!(
                better_ui::icon_set::Picture::from_path(icon.asset_path()).is_some(),
                "{icon:?}"
            );
        }
    }

    #[test]
    fn a_desktop_with_nothing_readable_has_an_empty_banner() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            HostRoot(root.path().to_path_buf()).device(),
            Device::default()
        );
    }

    #[test]
    fn recent_activity_is_the_five_newest_newest_first() {
        assert!(recent(&[]).is_empty());
        let records: Vec<ActivityRecord> = (1..=7)
            .map(|sequence| ActivityRecord {
                sequence,
                kind: ActivityKind::Success,
                component: None,
                operation: None,
                stage: None,
                evidence: None,
                recorded_at_unix_seconds: None,
            })
            .collect();
        let shown: Vec<u64> = recent(&records).iter().map(|r| r.sequence).collect();
        assert_eq!(shown, [7, 6, 5, 4, 3]);
    }

    #[test]
    fn each_activity_kind_has_a_tone() {
        assert_eq!(tone(ActivityKind::Success), Tone::Success);
        assert_eq!(tone(ActivityKind::RecoverySuccess), Tone::Success);
        assert_eq!(tone(ActivityKind::ManualRecovery), Tone::Warning);
        assert_eq!(tone(ActivityKind::Failure), Tone::Failure);
        assert_eq!(tone(ActivityKind::Information), Tone::Information);
    }

    #[test]
    fn ago_uses_the_coarsest_unit_that_still_says_something() {
        assert_eq!(ago(Locale::EnUs, 1_000, 990), "Just now");
        assert_eq!(ago(Locale::ZhTw, 1_000, 2_000), "剛剛");
        assert_eq!(ago(Locale::ZhTw, 10_000, 10_000 - 180), "3 分鐘前");
        assert_eq!(ago(Locale::EnUs, 100_000, 100_000 - 7_200), "2 h ago");
        assert_eq!(
            ago(Locale::ZhTw, 1_000_000, 1_000_000 - 3 * 86_400),
            "3 天前"
        );
    }

    #[test]
    fn the_welcome_bar_shows_until_it_is_closed() {
        assert!(shows_welcome(false));
        assert!(!shows_welcome(true));
        // Power mode, Keep awake, then Appearance, as the spec orders them.
        assert_eq!(
            quick_settings(),
            [
                QuickSetting::PowerMode,
                QuickSetting::KeepAwake,
                QuickSetting::Appearance
            ]
        );
    }
}
