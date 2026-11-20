//! What a battery icon shows: the one rule every Better OS battery icon and
//! battery line follows (ticket 170, `engineering/specs/battery-charge-state.md`).
//!
//! The kernel's `status` alone cannot tell a battery held at its charge limit
//! from one running the machine: at the limit it says `Not charging`, and so
//! does a charger that has stopped for a fault. This module combines the
//! charger, the status, the charge and the limit into one [`ChargeState`],
//! and gives each state its icon and its words in both languages, so no
//! window keeps a mapping of its own.
//!
//! It lives here, in `hardware-core`, because every place that draws a
//! battery can depend on this crate without pulling GPUI or any I/O: it needs
//! nothing but `serde`. Better Monitor's window and tray, Better Manager's
//! Home and System page, Better Awake's window, and the top bar (ticket 157)
//! all ask [`battery_look`]. The words live here too, against this crate's
//! "no words" habit, because a mapping repeated per window is what the
//! ticket removes.

use crate::Presence;
use crate::battery::BatteryStatus;

/// How far below the charge limit a battery that is not charging still
/// counts as held at it, in percentage points.
///
/// Firmware stops at the limit and starts again only after the charge has
/// fallen a little, so a held battery reads anywhere from the limit down to
/// a few points under it. When the battery also reports
/// `charge_control_start_threshold`, that is the lower edge instead. Five is a
/// recorded starting value: the owner's machine (ASUS, `BAT1`) reports no
/// start threshold, and on 2026-11-21 it read exactly its limit, 80 of 80.
pub const HOLD_TOLERANCE: u8 = 5;

/// The charge limit, as the machine reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChargeLimit {
    /// The battery has no `charge_control_end_threshold`: it charges to full.
    NotSupported,
    /// `charge_control_end_threshold`, and `charge_control_start_threshold`
    /// when the battery has one.
    Percent { end: u8, start: Option<u8> },
    /// The battery has a limit and it could not be read.
    Unreadable,
}

impl ChargeLimit {
    /// The limit as the System page reads it.
    pub fn from_presence(end: &Presence<u8>, start: Option<u8>) -> ChargeLimit {
        match end {
            Presence::Absent => ChargeLimit::NotSupported,
            Presence::Unreadable(_) => ChargeLimit::Unreadable,
            Presence::Present(end) => ChargeLimit::Percent { end: *end, start },
        }
    }
}

/// Everything the rule reads. Each `None` is a reading that was not there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChargeReadings {
    /// Whether a mains adapter or a USB-C port supplies power
    /// (`online` of a non-battery `power_supply`); see [`charger_online`].
    pub charger_online: Option<bool>,
    /// The kernel's `status` of the system battery.
    pub status: Option<BatteryStatus>,
    /// `capacity`, 0 to 100.
    pub percent: Option<u8>,
    pub limit: ChargeLimit,
}

/// What a battery is doing, as a person would say it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChargeState {
    /// The machine runs on its battery.
    OnBattery,
    Charging,
    /// Plugged in and not charging because the battery reached the limit
    /// Better OS set or the firmware reports. `limit` is that limit.
    HoldingAtLimit {
        limit: u8,
    },
    /// Plugged in and full: at 100%, or at the top of a battery with no limit.
    PluggedInFull,
    /// Plugged in, not charging, and below the limit: a charger that cannot
    /// supply enough, a fault, or a pause the firmware chose. Never called
    /// holding.
    PluggedInNotCharging,
    /// A reading the decision needs is missing.
    Unknown,
}

/// Whether any charger is online, from each power source's `online`: online
/// when one says so, offline when every one says it is not, and unknown when
/// there is none or one could not be read.
pub fn charger_online(sources: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut any = false;
    let mut unread = false;
    for online in sources {
        any = true;
        match online {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unread = true,
        }
    }
    (any && !unread).then_some(false)
}

/// The rule.
///
/// - `Charging` and `Discharging` are the kernel's own answer and need
///   nothing else: a discharging battery runs the machine even with a charger
///   that cannot keep up plugged in.
/// - Otherwise the charger decides. Offline, the battery is running the
///   machine whatever its status says. Unknown, nothing can be said.
/// - Online and `Not charging` or `Full`, the charge is compared with the
///   limit: from [`HOLD_TOLERANCE`] under it (or from the start threshold)
///   upwards it is held at the limit, or full when the limit is 100 or there
///   is none. Below that, `Not charging` is [`ChargeState::PluggedInNotCharging`].
/// - Online with the kernel's `Unknown` status, or with no status, is unknown.
pub fn charge_state(readings: &ChargeReadings) -> ChargeState {
    match readings.status {
        Some(BatteryStatus::Charging) => return ChargeState::Charging,
        Some(BatteryStatus::Discharging) => return ChargeState::OnBattery,
        _ => {}
    }
    match readings.charger_online {
        None => return ChargeState::Unknown,
        Some(false) => return ChargeState::OnBattery,
        Some(true) => {}
    }
    let full = match readings.status {
        Some(BatteryStatus::Full) => true,
        Some(BatteryStatus::NotCharging) => false,
        _ => return ChargeState::Unknown,
    };
    let Some(percent) = readings.percent else {
        return ChargeState::Unknown;
    };
    let (end, start) = match readings.limit {
        ChargeLimit::NotSupported => (100, None),
        ChargeLimit::Percent { end, start } => (end.min(100), start),
        // Without the limit only a battery at 100% is known to be full.
        ChargeLimit::Unreadable if percent >= 100 => return ChargeState::PluggedInFull,
        ChargeLimit::Unreadable => return ChargeState::Unknown,
    };
    let lowest = match start {
        Some(start) if start < end => start,
        _ => end.saturating_sub(HOLD_TOLERANCE),
    };
    if percent >= lowest {
        if end >= 100 {
            ChargeState::PluggedInFull
        } else {
            ChargeState::HoldingAtLimit { limit: end }
        }
    } else if full {
        // The kernel calls a battery full below the window only when its
        // idea of full has moved (a worn battery); it is still full.
        ChargeState::PluggedInFull
    } else {
        ChargeState::PluggedInNotCharging
    }
}

/// The battery pictures, by the file stem each has in `better-ui`'s icon set
/// (`crates/better-ui/assets/icons/object/`). A window draws
/// `better_ui::icon_set::Picture::from_path(icon.asset_path())`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BatteryIcon {
    /// An empty battery: nothing is known about it.
    Unknown,
    Low,
    Medium,
    Full,
    Charging,
    /// A battery with a plug, partly filled: held at its limit.
    Plugged,
    /// A full battery with a plug.
    FullPlugged,
}

impl BatteryIcon {
    pub const ALL: [BatteryIcon; 7] = [
        BatteryIcon::Unknown,
        BatteryIcon::Low,
        BatteryIcon::Medium,
        BatteryIcon::Full,
        BatteryIcon::Charging,
        BatteryIcon::Plugged,
        BatteryIcon::FullPlugged,
    ];

    /// The file stem in `better-ui`'s icon set.
    pub const fn file(self) -> &'static str {
        match self {
            BatteryIcon::Unknown => "battery",
            BatteryIcon::Low => "battery-low",
            BatteryIcon::Medium => "battery-medium",
            BatteryIcon::Full => "battery-full",
            BatteryIcon::Charging => "battery-charging",
            BatteryIcon::Plugged => "battery-plugged",
            BatteryIcon::FullPlugged => "battery-full-plugged",
        }
    }

    /// The path `better-ui` serves the picture under.
    pub const fn asset_path(self) -> &'static str {
        match self {
            BatteryIcon::Unknown => "better-ui/icons/object/battery.svg",
            BatteryIcon::Low => "better-ui/icons/object/battery-low.svg",
            BatteryIcon::Medium => "better-ui/icons/object/battery-medium.svg",
            BatteryIcon::Full => "better-ui/icons/object/battery-full.svg",
            BatteryIcon::Charging => "better-ui/icons/object/battery-charging.svg",
            BatteryIcon::Plugged => "better-ui/icons/object/battery-plugged.svg",
            BatteryIcon::FullPlugged => "better-ui/icons/object/battery-full-plugged.svg",
        }
    }
}

/// A level picture for a battery that is not plugged in: full from 90%,
/// medium from 40%, low below.
fn level(percent: Option<u8>) -> BatteryIcon {
    match percent {
        Some(p) if p >= 90 => BatteryIcon::Full,
        Some(p) if p >= 40 => BatteryIcon::Medium,
        Some(_) => BatteryIcon::Low,
        None => BatteryIcon::Unknown,
    }
}

/// The two languages Better OS ships.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    English,
    TraditionalChinese,
}

impl ChargeState {
    /// The state's picture. A battery that is plugged in and not charging
    /// below its limit keeps its level picture: it is not being kept topped
    /// up, and a plug would read as "all is well".
    pub fn icon(self, percent: Option<u8>) -> BatteryIcon {
        match self {
            ChargeState::OnBattery | ChargeState::PluggedInNotCharging => level(percent),
            ChargeState::Charging => BatteryIcon::Charging,
            ChargeState::HoldingAtLimit { .. } => BatteryIcon::Plugged,
            ChargeState::PluggedInFull => BatteryIcon::FullPlugged,
            ChargeState::Unknown => BatteryIcon::Unknown,
        }
    }

    /// The state in words.
    pub fn words(self, language: Language) -> String {
        let zh = language == Language::TraditionalChinese;
        match self {
            ChargeState::OnBattery => pick(zh, "On battery", "使用電池"),
            ChargeState::Charging => pick(zh, "Charging", "充電中"),
            ChargeState::HoldingAtLimit { limit } => {
                if zh {
                    format!("已接上電源，維持在 {limit}%")
                } else {
                    format!("Plugged in, holding at {limit}%")
                }
            }
            ChargeState::PluggedInFull => {
                pick(zh, "Plugged in, fully charged", "已接上電源，已充滿")
            }
            ChargeState::PluggedInNotCharging => {
                pick(zh, "Plugged in, not charging", "已接上電源，沒有充電")
            }
            ChargeState::Unknown => pick(zh, "Battery state unknown", "無法判讀電池狀態"),
        }
    }

    /// Whether the machine runs on its battery now: `None` when the state is
    /// unknown.
    pub fn on_battery(self) -> Option<bool> {
        match self {
            ChargeState::OnBattery => Some(true),
            ChargeState::Unknown => None,
            _ => Some(false),
        }
    }
}

fn pick(zh: bool, english: &str, chinese: &str) -> String {
    if zh { chinese } else { english }.to_string()
}

/// What a battery icon shows: its state, its picture and its words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatteryLook {
    pub state: ChargeState,
    pub icon: BatteryIcon,
    pub words: String,
}

/// The one function every battery icon and battery line in Better OS asks.
pub fn battery_look(readings: &ChargeReadings, language: Language) -> BatteryLook {
    let state = charge_state(readings);
    BatteryLook {
        state,
        icon: state.icon(readings.percent),
        words: state.words(language),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's machine on 2026-11-21: charger online, `Not charging`,
    /// 80% with the limit at 80.
    fn owner() -> ChargeReadings {
        ChargeReadings {
            charger_online: Some(true),
            status: Some(BatteryStatus::NotCharging),
            percent: Some(80),
            limit: ChargeLimit::Percent {
                end: 80,
                start: None,
            },
        }
    }

    #[test]
    fn the_owners_machine_at_its_limit_is_held_there() {
        let look = battery_look(&owner(), Language::TraditionalChinese);
        assert_eq!(look.state, ChargeState::HoldingAtLimit { limit: 80 });
        assert_eq!(look.icon, BatteryIcon::Plugged);
        assert_eq!(look.words, "已接上電源，維持在 80%");
        assert_eq!(
            battery_look(&owner(), Language::English).words,
            "Plugged in, holding at 80%"
        );
    }

    #[test]
    fn held_is_never_the_on_battery_picture_or_words() {
        let held = battery_look(&owner(), Language::English);
        let on_battery = battery_look(
            &ChargeReadings {
                charger_online: Some(false),
                status: Some(BatteryStatus::Discharging),
                ..owner()
            },
            Language::English,
        );
        assert_ne!(held.icon, on_battery.icon);
        assert_ne!(held.words, on_battery.words);
        assert_eq!(on_battery.state, ChargeState::OnBattery);
        assert_eq!(on_battery.icon, BatteryIcon::Medium);
        assert_eq!(on_battery.words, "On battery");
    }

    #[test]
    fn the_firmwares_hysteresis_under_the_limit_is_still_held() {
        for percent in 75..=80 {
            let r = ChargeReadings {
                percent: Some(percent),
                ..owner()
            };
            assert_eq!(
                charge_state(&r),
                ChargeState::HoldingAtLimit { limit: 80 },
                "{percent}"
            );
        }
        // A limit lowered under the charge: the firmware holds it there.
        let above = ChargeReadings {
            percent: Some(86),
            ..owner()
        };
        assert_eq!(
            charge_state(&above),
            ChargeState::HoldingAtLimit { limit: 80 }
        );
    }

    #[test]
    fn online_not_charging_below_the_limit_is_its_own_state() {
        let below = ChargeReadings {
            percent: Some(74),
            ..owner()
        };
        assert_eq!(charge_state(&below), ChargeState::PluggedInNotCharging);
        let far = ChargeReadings {
            percent: Some(31),
            ..owner()
        };
        let look = battery_look(&far, Language::TraditionalChinese);
        assert_eq!(look.state, ChargeState::PluggedInNotCharging);
        assert_eq!(look.words, "已接上電源，沒有充電");
        assert_eq!(look.icon, BatteryIcon::Low);
        assert!(!look.words.contains("維持"));
        assert_eq!(
            battery_look(&far, Language::English).words,
            "Plugged in, not charging"
        );
    }

    #[test]
    fn a_start_threshold_is_the_lower_edge_of_the_hold() {
        let limit = ChargeLimit::Percent {
            end: 80,
            start: Some(60),
        };
        let at = |percent| {
            charge_state(&ChargeReadings {
                percent: Some(percent),
                limit,
                ..owner()
            })
        };
        assert_eq!(at(61), ChargeState::HoldingAtLimit { limit: 80 });
        assert_eq!(at(60), ChargeState::HoldingAtLimit { limit: 80 });
        assert_eq!(at(59), ChargeState::PluggedInNotCharging);
        // A start threshold at or over the end is ignored.
        let odd = ChargeLimit::Percent {
            end: 80,
            start: Some(90),
        };
        let r = ChargeReadings {
            percent: Some(76),
            limit: odd,
            ..owner()
        };
        assert_eq!(charge_state(&r), ChargeState::HoldingAtLimit { limit: 80 });
    }

    #[test]
    fn charging_and_discharging_are_the_kernels_own_answer() {
        for charger in [Some(true), Some(false), None] {
            let charging = ChargeReadings {
                charger_online: charger,
                status: Some(BatteryStatus::Charging),
                percent: Some(42),
                ..owner()
            };
            let look = battery_look(&charging, Language::TraditionalChinese);
            assert_eq!(look.state, ChargeState::Charging);
            assert_eq!(look.icon, BatteryIcon::Charging);
            assert_eq!(look.words, "充電中");
            let discharging = ChargeReadings {
                status: Some(BatteryStatus::Discharging),
                ..charging
            };
            assert_eq!(charge_state(&discharging), ChargeState::OnBattery);
        }
    }

    #[test]
    fn plugged_in_and_full() {
        let full = ChargeReadings {
            charger_online: Some(true),
            status: Some(BatteryStatus::Full),
            percent: Some(100),
            limit: ChargeLimit::NotSupported,
        };
        let look = battery_look(&full, Language::English);
        assert_eq!(look.state, ChargeState::PluggedInFull);
        assert_eq!(look.icon, BatteryIcon::FullPlugged);
        assert_eq!(look.words, "Plugged in, fully charged");
        // A limit of 100 is no limit.
        let hundred = ChargeReadings {
            status: Some(BatteryStatus::NotCharging),
            percent: Some(97),
            limit: ChargeLimit::Percent {
                end: 100,
                start: None,
            },
            ..full
        };
        assert_eq!(charge_state(&hundred), ChargeState::PluggedInFull);
        // No limit and not charging at the top is full too.
        let top = ChargeReadings {
            status: Some(BatteryStatus::NotCharging),
            percent: Some(96),
            ..full
        };
        assert_eq!(charge_state(&top), ChargeState::PluggedInFull);
        let low = ChargeReadings {
            percent: Some(94),
            ..top
        };
        assert_eq!(charge_state(&low), ChargeState::PluggedInNotCharging);
    }

    #[test]
    fn firmware_that_says_full_at_the_limit_is_held() {
        let r = ChargeReadings {
            status: Some(BatteryStatus::Full),
            ..owner()
        };
        assert_eq!(charge_state(&r), ChargeState::HoldingAtLimit { limit: 80 });
        // Full below the window is still full: the kernel's idea of full.
        let worn = ChargeReadings {
            status: Some(BatteryStatus::Full),
            percent: Some(60),
            ..owner()
        };
        assert_eq!(charge_state(&worn), ChargeState::PluggedInFull);
    }

    #[test]
    fn a_charger_that_is_offline_means_the_battery_runs_the_machine() {
        for status in [
            Some(BatteryStatus::NotCharging),
            Some(BatteryStatus::Full),
            Some(BatteryStatus::Unknown),
            None,
        ] {
            let r = ChargeReadings {
                charger_online: Some(false),
                status,
                ..owner()
            };
            assert_eq!(charge_state(&r), ChargeState::OnBattery, "{status:?}");
        }
    }

    #[test]
    fn a_missing_reading_is_unknown_never_a_guess() {
        let unknown = |r: ChargeReadings| charge_state(&r);
        assert_eq!(
            unknown(ChargeReadings {
                charger_online: None,
                ..owner()
            }),
            ChargeState::Unknown
        );
        assert_eq!(
            unknown(ChargeReadings {
                percent: None,
                ..owner()
            }),
            ChargeState::Unknown
        );
        assert_eq!(
            unknown(ChargeReadings {
                status: None,
                ..owner()
            }),
            ChargeState::Unknown
        );
        assert_eq!(
            unknown(ChargeReadings {
                status: Some(BatteryStatus::Unknown),
                ..owner()
            }),
            ChargeState::Unknown
        );
        assert_eq!(
            unknown(ChargeReadings {
                limit: ChargeLimit::Unreadable,
                ..owner()
            }),
            ChargeState::Unknown
        );
        // Only at 100% does an unreadable limit not matter.
        assert_eq!(
            unknown(ChargeReadings {
                limit: ChargeLimit::Unreadable,
                percent: Some(100),
                ..owner()
            }),
            ChargeState::PluggedInFull
        );
        let look = battery_look(
            &ChargeReadings {
                charger_online: None,
                percent: None,
                ..owner()
            },
            Language::English,
        );
        assert_eq!(look.icon, BatteryIcon::Unknown);
        assert_eq!(look.words, "Battery state unknown");
        assert_eq!(look.state.on_battery(), None);
    }

    #[test]
    fn charger_online_needs_one_yes_or_every_no() {
        assert_eq!(charger_online([Some(false), Some(true)]), Some(true));
        assert_eq!(charger_online([None, Some(true)]), Some(true));
        assert_eq!(charger_online([Some(false), Some(false)]), Some(false));
        assert_eq!(charger_online([Some(false), None]), None);
        assert_eq!(charger_online(std::iter::empty()), None);
    }

    #[test]
    fn levels_on_battery() {
        let at = |percent| {
            battery_look(
                &ChargeReadings {
                    charger_online: Some(false),
                    status: Some(BatteryStatus::Discharging),
                    percent,
                    limit: ChargeLimit::NotSupported,
                },
                Language::English,
            )
            .icon
        };
        assert_eq!(at(Some(90)), BatteryIcon::Full);
        assert_eq!(at(Some(89)), BatteryIcon::Medium);
        assert_eq!(at(Some(40)), BatteryIcon::Medium);
        assert_eq!(at(Some(39)), BatteryIcon::Low);
        assert_eq!(at(None), BatteryIcon::Unknown);
    }

    #[test]
    fn a_limit_from_the_system_page() {
        assert_eq!(
            ChargeLimit::from_presence(&Presence::Absent, None),
            ChargeLimit::NotSupported
        );
        assert_eq!(
            ChargeLimit::from_presence(&Presence::Unreadable("EIO".into()), None),
            ChargeLimit::Unreadable
        );
        assert_eq!(
            ChargeLimit::from_presence(&Presence::Present(80), Some(70)),
            ChargeLimit::Percent {
                end: 80,
                start: Some(70)
            }
        );
    }

    #[test]
    fn every_icon_names_its_own_file() {
        for icon in BatteryIcon::ALL {
            assert_eq!(
                icon.asset_path(),
                format!("better-ui/icons/object/{}.svg", icon.file())
            );
        }
    }
}
