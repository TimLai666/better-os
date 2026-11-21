//! What a battery icon shows: the one rule every Better OS battery icon and
//! battery line follows (ticket 170, `engineering/specs/battery-charge-state.md`).
//!
//! The kernel's `status` alone cannot tell a battery held at its charge limit
//! from one running the machine: at the limit it says `Not charging`, and so
//! does a charger that has stopped for a fault. This module combines the
//! charger, each battery's status, charge and limit into one [`ChargeState`],
//! and gives each state its picture and its words in both languages, so no
//! window keeps a mapping of its own. It also owns the two related questions
//! every window asks: whether a charger is online ([`charger_online`]) and
//! whether the machine draws from its battery ([`ChargeState::on_battery`]).
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
/// counts as held at it, in percentage points, when it reports no usable
/// start threshold.
///
/// Firmware stops at the limit and starts again only after the charge has
/// fallen a little, so a held battery reads anywhere from the limit down to
/// a few points under it. Five is a recorded starting value: the owner's
/// machine (ASUS, `BAT1`) reports no start threshold, and on 2026-11-21 it read
/// exactly its limit, 80 of 80.
pub const HOLD_TOLERANCE: u8 = 5;

/// The widest hold window a start threshold may open, in points under the
/// limit.
///
/// `charge_control_start_threshold` is where charging starts again, so a
/// battery anywhere between it and the limit may rest there for hours. The
/// tools that set both write them close together: UPower and TLP default to 75
/// and 80, Dell's firmware asks for at least 5 points between them. A window
/// wider than 20 points stops meaning "held": ThinkPads ship with a start of
/// 0, and a battery at 30% that is not charging with the charger in is a fault
/// to show, not a hold. A start threshold of 0, at or over the limit, or more
/// than this under it is ignored and [`HOLD_TOLERANCE`] is used instead.
pub const START_WINDOW_MAX: u8 = 20;

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

    /// The lowest charge that still counts as held at this limit, and the
    /// limit: `None` when the limit could not be read.
    fn window(self) -> Option<(u8, u8)> {
        let (end, start) = match self {
            ChargeLimit::NotSupported => (100, None),
            ChargeLimit::Percent { end, start } => (end.min(100), start),
            ChargeLimit::Unreadable => return None,
        };
        let lowest = match start {
            Some(start) if start > 0 && start < end && end - start <= START_WINDOW_MAX => start,
            _ => end.saturating_sub(HOLD_TOLERANCE),
        };
        Some((lowest, end))
    }
}

/// One battery, read.
#[derive(Clone, Debug, PartialEq)]
pub struct BatteryPart {
    /// The kernel's `status`.
    pub status: Option<BatteryStatus>,
    /// `capacity`, 0 to 100.
    pub percent: Option<u8>,
    /// Energy now and when full, in any one unit, for weighting the charge of
    /// several batteries.
    pub energy_now: Option<f64>,
    pub energy_full: Option<f64>,
    pub limit: ChargeLimit,
}

impl BatteryPart {
    pub fn new(status: Option<BatteryStatus>, percent: Option<u8>, limit: ChargeLimit) -> Self {
        BatteryPart {
            status,
            percent,
            energy_now: None,
            energy_full: None,
            limit,
        }
    }
}

/// Everything the rule reads: whether a charger is online and every system
/// battery (a ThinkPad may have two). Each `None` is a reading that was not
/// there.
#[derive(Clone, Debug, PartialEq)]
pub struct ChargeReadings {
    /// See [`charger_online`].
    pub charger_online: Option<bool>,
    pub batteries: Vec<BatteryPart>,
}

impl ChargeReadings {
    /// A machine with one battery.
    pub fn single(
        charger_online: Option<bool>,
        status: Option<BatteryStatus>,
        percent: Option<u8>,
        limit: ChargeLimit,
    ) -> Self {
        ChargeReadings {
            charger_online,
            batteries: vec![BatteryPart::new(status, percent, limit)],
        }
    }

    /// The charge of all the batteries together: one battery's own, or the
    /// energy-weighted charge of several when each reports its energy, or
    /// their average when one does not.
    pub fn percent(&self) -> Option<u8> {
        match self.batteries.as_slice() {
            [] => None,
            [one] => one.percent,
            many => {
                let energy: Option<(f64, f64)> =
                    many.iter().try_fold((0.0, 0.0), |(now, full), part| {
                        let part_full = part.energy_full.filter(|f| *f > 0.0)?;
                        Some((now + part.energy_now?, full + part_full))
                    });
                if let Some((now, full)) = energy.filter(|(_, full)| *full > 0.0) {
                    return Some((now / full * 100.0).round().clamp(0.0, 100.0) as u8);
                }
                let sum: Option<u32> = many.iter().map(|part| part.percent.map(u32::from)).sum();
                sum.map(|sum| (sum as f64 / many.len() as f64).round() as u8)
            }
        }
    }
}

/// What a battery is doing, as a person would say it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChargeState {
    /// The machine runs on its battery.
    OnBattery,
    Charging,
    /// The charger is in and the battery still drains: a charger too weak for
    /// the load. Never called "on battery", which the person can see is false.
    PluggedInDraining,
    /// Plugged in and not charging because the battery is at, a little under,
    /// or above the limit Better OS set or the firmware reports. `limit` is
    /// that limit and `percent` the charge, so the words can say which.
    Holding {
        limit: u8,
        percent: Option<u8>,
    },
    /// Plugged in and full: at the top of a battery with no limit or a limit
    /// of 100%.
    PluggedInFull,
    /// Plugged in, not charging, and below the limit: a charger that cannot
    /// supply enough, a fault, or a pause the firmware chose. Never called
    /// holding.
    PluggedInNotCharging,
    /// A reading the decision needs is missing.
    Unknown,
}

/// Whether a `power_supply` of this `type` supplies this machine.
///
/// The kernel's types are "Battery", "UPS", "Mains", "USB" and "Wireless"
/// (`Documentation/ABI/testing/sysfs-class-power`, `type`). Mains is an AC
/// adapter, USB a USB or USB-C port (a dock included), Wireless a charging pad:
/// each powers the machine when online. Kernels before 4.17 spelled USB-C and
/// USB charger kinds as types of their own (`USB_C`, `USB_PD`, `USB_DCP`), so
/// any type that starts with `USB` counts. A Battery is what is charged, and a
/// UPS is a battery the kernel reports beside the machine's, not something
/// that powers it, so neither counts.
pub fn counts_as_charger(kind: &str) -> bool {
    let kind = kind.trim();
    kind.eq_ignore_ascii_case("mains")
        || kind.eq_ignore_ascii_case("wireless")
        || kind
            .get(..3)
            .is_some_and(|start| start.eq_ignore_ascii_case("usb"))
}

/// Whether any charger is online, from each power source's `type` and
/// `online`: online when one that counts ([`counts_as_charger`]) says so,
/// offline when every one that counts says it is not, and unknown when none
/// counts or one could not be read.
pub fn charger_online<'a>(
    sources: impl IntoIterator<Item = (&'a str, Option<bool>)>,
) -> Option<bool> {
    let mut any = false;
    let mut unread = false;
    for (kind, online) in sources {
        if !counts_as_charger(kind) {
            continue;
        }
        any = true;
        match online {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unread = true,
        }
    }
    (any && !unread).then_some(false)
}

/// What one battery that is neither charging nor discharging is doing.
enum Resting {
    Held { limit: u8 },
    Full,
    Below,
    Unknown,
}

fn resting(part: &BatteryPart) -> Resting {
    let Some(percent) = part.percent else {
        return Resting::Unknown;
    };
    // `Unknown` is what kernels before 5.19 reported for a battery stopped by
    // its limit (`drivers/acpi/battery.c`, commit 185d20694a8a): inside the
    // hold window it is held, anywhere else it stays unknown.
    let kernel_unknown = match part.status {
        Some(BatteryStatus::Full) | Some(BatteryStatus::NotCharging) => false,
        Some(BatteryStatus::Unknown) => true,
        _ => return Resting::Unknown,
    };
    let Some((lowest, end)) = part.limit.window() else {
        // Without the limit only a battery at 100% is known to be full.
        return if percent >= 100 {
            Resting::Full
        } else {
            Resting::Unknown
        };
    };
    if percent >= lowest {
        if end >= 100 {
            Resting::Full
        } else {
            Resting::Held { limit: end }
        }
    } else if kernel_unknown {
        Resting::Unknown
    } else if part.status == Some(BatteryStatus::Full) {
        // The kernel calls a battery full below the window only when its
        // idea of full has moved (a worn battery); it is still full.
        Resting::Full
    } else {
        Resting::Below
    }
}

/// The rule.
///
/// - Any battery charging is charging. Any discharging is the machine drawing
///   from its battery: on battery, or plugged in and draining when a charger
///   is online.
/// - Otherwise the charger decides. Offline, the battery is running the
///   machine whatever its status says. Unknown, nothing can be said.
/// - Online, each battery is compared with its limit: inside the hold window
///   (from the start threshold, or [`HOLD_TOLERANCE`] under the limit, upwards)
///   it is held at the limit, or full when the limit is 100 or there is none.
///   One below its window is plugged in and not charging; one whose state
///   cannot be told makes the whole unknown; all held or full is holding (or
///   full).
pub fn charge_state(readings: &ChargeReadings) -> ChargeState {
    let parts = &readings.batteries;
    if parts.is_empty() {
        return match readings.charger_online {
            Some(false) => ChargeState::OnBattery,
            _ => ChargeState::Unknown,
        };
    }
    if parts
        .iter()
        .any(|p| p.status == Some(BatteryStatus::Charging))
    {
        return ChargeState::Charging;
    }
    if parts
        .iter()
        .any(|p| p.status == Some(BatteryStatus::Discharging))
    {
        return match readings.charger_online {
            Some(true) => ChargeState::PluggedInDraining,
            _ => ChargeState::OnBattery,
        };
    }
    match readings.charger_online {
        None => return ChargeState::Unknown,
        Some(false) => return ChargeState::OnBattery,
        Some(true) => {}
    }
    let resting: Vec<Resting> = parts.iter().map(resting).collect();
    if resting.iter().any(|r| matches!(r, Resting::Below)) {
        return ChargeState::PluggedInNotCharging;
    }
    if resting.iter().any(|r| matches!(r, Resting::Unknown)) {
        return ChargeState::Unknown;
    }
    match resting
        .iter()
        .filter_map(|r| match r {
            Resting::Held { limit } => Some(*limit),
            _ => None,
        })
        .max()
    {
        Some(limit) => ChargeState::Holding {
            limit,
            percent: readings.percent(),
        },
        None => ChargeState::PluggedInFull,
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
    /// A battery with a plug in it, partly filled: held at its limit.
    Plugged,
    /// A full battery with a plug in it.
    FullPlugged,
    /// A battery with a plug in it and a caution mark where the charge would
    /// be: plugged in and not charging below the limit, or still draining.
    PluggedCaution,
}

impl BatteryIcon {
    pub const ALL: [BatteryIcon; 8] = [
        BatteryIcon::Unknown,
        BatteryIcon::Low,
        BatteryIcon::Medium,
        BatteryIcon::Full,
        BatteryIcon::Charging,
        BatteryIcon::Plugged,
        BatteryIcon::FullPlugged,
        BatteryIcon::PluggedCaution,
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
            BatteryIcon::PluggedCaution => "battery-plugged-caution",
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
            BatteryIcon::PluggedCaution => "better-ui/icons/object/battery-plugged-caution.svg",
        }
    }
}

/// A level picture for a battery running the machine: full from 90%, medium
/// from 40%, low below.
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
    /// Every state, with sample values, for the tests that compare them.
    pub const KINDS: [ChargeState; 7] = [
        ChargeState::OnBattery,
        ChargeState::Charging,
        ChargeState::PluggedInDraining,
        ChargeState::Holding {
            limit: 80,
            percent: Some(80),
        },
        ChargeState::PluggedInFull,
        ChargeState::PluggedInNotCharging,
        ChargeState::Unknown,
    ];

    /// The state's picture. Only a battery running the machine shows its
    /// level; every state with the charger in carries a plug or a bolt.
    pub fn icon(self, percent: Option<u8>) -> BatteryIcon {
        match self {
            ChargeState::OnBattery => level(percent),
            ChargeState::Charging => BatteryIcon::Charging,
            ChargeState::Holding { .. } => BatteryIcon::Plugged,
            ChargeState::PluggedInFull => BatteryIcon::FullPlugged,
            ChargeState::PluggedInNotCharging | ChargeState::PluggedInDraining => {
                BatteryIcon::PluggedCaution
            }
            ChargeState::Unknown => BatteryIcon::Unknown,
        }
    }

    /// The state in words, for a line or a card.
    pub fn words(self, language: Language) -> String {
        let zh = language == Language::TraditionalChinese;
        match self {
            ChargeState::OnBattery => pick(zh, "On battery", "使用電池"),
            ChargeState::Charging => pick(zh, "Charging", "充電中"),
            ChargeState::PluggedInDraining => {
                pick(zh, "Plugged in, still draining", "已接上電源，仍在耗電")
            }
            ChargeState::Holding { limit, percent } => match percent {
                Some(p) if p > limit => {
                    if zh {
                        format!("已接上電源，高於 {limit}% 上限，不充電")
                    } else {
                        format!("Plugged in, not charging above the {limit}% limit")
                    }
                }
                Some(p) if p < limit => {
                    if zh {
                        format!("已接上電源，維持在 {p}%（上限 {limit}%）")
                    } else {
                        format!("Plugged in, holding at {p}% (limit {limit}%)")
                    }
                }
                _ => {
                    if zh {
                        format!("已接上電源，維持在 {limit}%")
                    } else {
                        format!("Plugged in, holding at {limit}%")
                    }
                }
            },
            ChargeState::PluggedInFull => {
                pick(zh, "Plugged in, fully charged", "已接上電源，已充滿")
            }
            ChargeState::PluggedInNotCharging => {
                pick(zh, "Plugged in, not charging", "已接上電源，沒有充電")
            }
            ChargeState::Unknown => pick(zh, "Battery state unknown", "無法判讀電池狀態"),
        }
    }

    /// The state in a few words, for a narrow place: a list row, a header
    /// beside a figure that already gives the charge.
    pub fn short_words(self, language: Language) -> String {
        let zh = language == Language::TraditionalChinese;
        match self {
            ChargeState::OnBattery => pick(zh, "On battery", "使用電池"),
            ChargeState::Charging => pick(zh, "Charging", "充電中"),
            ChargeState::PluggedInDraining => pick(zh, "Draining", "接電仍耗電"),
            ChargeState::Holding { limit, percent } => match percent {
                Some(p) if p > limit => {
                    if zh {
                        format!("高於上限 {limit}%")
                    } else {
                        format!("Above {limit}% limit")
                    }
                }
                _ => {
                    if zh {
                        format!("維持在上限 {limit}%")
                    } else {
                        format!("Held at {limit}% limit")
                    }
                }
            },
            ChargeState::PluggedInFull => pick(zh, "Fully charged", "已充滿"),
            ChargeState::PluggedInNotCharging => pick(zh, "Not charging", "沒有充電"),
            ChargeState::Unknown => pick(zh, "Unknown", "無法判讀"),
        }
    }

    /// Whether the machine draws from its battery now (on battery, or plugged
    /// in and still draining): `None` when the state is unknown.
    pub fn on_battery(self) -> Option<bool> {
        match self {
            ChargeState::OnBattery | ChargeState::PluggedInDraining => Some(true),
            ChargeState::Unknown => None,
            _ => Some(false),
        }
    }

    /// Whether a time until empty or until full means something: only while
    /// the charge moves.
    pub fn has_time(self) -> bool {
        matches!(
            self,
            ChargeState::OnBattery | ChargeState::PluggedInDraining | ChargeState::Charging
        )
    }
}

fn pick(zh: bool, english: &str, chinese: &str) -> String {
    if zh { chinese } else { english }.to_string()
}

/// A battery icon's accessible name, for a status button that draws only
/// the picture and the charge figure (the login and lock screens' status
/// row): the charge, then the state's full words, 「電池 87%，充電中」 /
/// "Battery 87%: Charging". With no charge read, the words alone after the
/// noun; an unknown state's words already name the battery.
fn spoken(state: ChargeState, percent: Option<u8>, language: Language) -> String {
    let words = state.words(language);
    if state == ChargeState::Unknown {
        return words;
    }
    let zh = language == Language::TraditionalChinese;
    match percent {
        Some(p) if zh => format!("電池 {p}%，{words}"),
        Some(p) => format!("Battery {p}%: {words}"),
        None if zh => format!("電池：{words}"),
        None => format!("Battery: {words}"),
    }
}

/// What a battery icon shows: its state, its picture, and its words in full,
/// in short, and as an accessible name with the charge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatteryLook {
    pub state: ChargeState,
    pub icon: BatteryIcon,
    pub words: String,
    pub short: String,
    pub spoken: String,
}

/// The one function every battery icon and battery line in Better OS asks.
pub fn battery_look(readings: &ChargeReadings, language: Language) -> BatteryLook {
    let state = charge_state(readings);
    let percent = readings.percent();
    BatteryLook {
        state,
        icon: state.icon(percent),
        words: state.words(language),
        short: state.short_words(language),
        spoken: spoken(state, percent, language),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(end: u8) -> ChargeLimit {
        ChargeLimit::Percent { end, start: None }
    }

    /// The owner's machine on 2026-11-21: charger online, `Not charging`,
    /// 80% with the limit at 80.
    fn owner() -> ChargeReadings {
        ChargeReadings::single(
            Some(true),
            Some(BatteryStatus::NotCharging),
            Some(80),
            limit(80),
        )
    }

    fn one(
        charger: Option<bool>,
        status: Option<BatteryStatus>,
        percent: Option<u8>,
        limit: ChargeLimit,
    ) -> ChargeState {
        charge_state(&ChargeReadings::single(charger, status, percent, limit))
    }

    fn held(limit: u8, percent: u8) -> ChargeState {
        ChargeState::Holding {
            limit,
            percent: Some(percent),
        }
    }

    use BatteryStatus::{Charging, Discharging, Full, NotCharging};

    #[test]
    fn the_owners_machine_at_its_limit_is_held_there() {
        let look = battery_look(&owner(), Language::TraditionalChinese);
        assert_eq!(look.state, held(80, 80));
        assert_eq!(look.icon, BatteryIcon::Plugged);
        assert_eq!(look.words, "已接上電源，維持在 80%");
        assert_eq!(look.short, "維持在上限 80%");
        let en = battery_look(&owner(), Language::English);
        assert_eq!(en.words, "Plugged in, holding at 80%");
        assert_eq!(en.short, "Held at 80% limit");
    }

    #[test]
    fn held_is_never_the_on_battery_picture_or_words() {
        let held = battery_look(&owner(), Language::English);
        let on_battery = battery_look(
            &ChargeReadings::single(Some(false), Some(Discharging), Some(80), limit(80)),
            Language::English,
        );
        assert_ne!(held.icon, on_battery.icon);
        assert_ne!(held.words, on_battery.words);
        assert_eq!(on_battery.state, ChargeState::OnBattery);
        assert_eq!(on_battery.icon, BatteryIcon::Medium);
        assert_eq!(on_battery.words, "On battery");
    }

    /// No two states draw the same picture at any charge, so a glance tells
    /// them apart. The two caution states share theirs and differ in words.
    /// (With no charge read at all, on battery draws the empty battery that
    /// unknown draws: there is no level to show.)
    #[test]
    fn no_two_states_share_a_picture_at_the_same_charge() {
        for percent in (0..=100).map(Some) {
            for (i, a) in ChargeState::KINDS.iter().enumerate() {
                for b in &ChargeState::KINDS[i + 1..] {
                    let caution = [
                        ChargeState::PluggedInNotCharging,
                        ChargeState::PluggedInDraining,
                    ];
                    if caution.contains(a) && caution.contains(b) {
                        assert_ne!(a.words(Language::English), b.words(Language::English));
                        continue;
                    }
                    assert_ne!(
                        a.icon(percent),
                        b.icon(percent),
                        "{a:?} and {b:?} at {percent:?}"
                    );
                }
            }
        }
        // Nothing with the charger in shows a level picture.
        for state in ChargeState::KINDS {
            if state != ChargeState::OnBattery && state != ChargeState::Unknown {
                for percent in 0..=100 {
                    assert!(
                        ![BatteryIcon::Low, BatteryIcon::Medium, BatteryIcon::Full]
                            .contains(&state.icon(Some(percent))),
                        "{state:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_firmwares_hysteresis_under_the_limit_is_still_held() {
        for percent in 75..=80 {
            assert_eq!(
                one(Some(true), Some(NotCharging), Some(percent), limit(80)),
                held(80, percent),
                "{percent}"
            );
        }
        let under = battery_look(
            &ChargeReadings::single(Some(true), Some(NotCharging), Some(79), limit(80)),
            Language::English,
        );
        assert_eq!(under.words, "Plugged in, holding at 79% (limit 80%)");
        assert_eq!(under.short, "Held at 80% limit");
        let zh = battery_look(
            &ChargeReadings::single(Some(true), Some(NotCharging), Some(76), limit(80)),
            Language::TraditionalChinese,
        );
        assert_eq!(zh.words, "已接上電源，維持在 76%（上限 80%）");
    }

    /// A limit lowered under the charge: the firmware stops charging and does
    /// not drain, so the words say the charge is above the limit.
    #[test]
    fn above_the_limit_says_so() {
        let state = one(Some(true), Some(NotCharging), Some(95), limit(80));
        assert_eq!(state, held(80, 95));
        assert_eq!(
            state.words(Language::English),
            "Plugged in, not charging above the 80% limit"
        );
        assert_eq!(
            state.words(Language::TraditionalChinese),
            "已接上電源，高於 80% 上限，不充電"
        );
        assert_eq!(state.short_words(Language::English), "Above 80% limit");
        assert_eq!(
            state.short_words(Language::TraditionalChinese),
            "高於上限 80%"
        );
        assert_eq!(state.icon(Some(95)), BatteryIcon::Plugged);
    }

    #[test]
    fn online_not_charging_below_the_limit_is_its_own_state() {
        assert_eq!(
            one(Some(true), Some(NotCharging), Some(74), limit(80)),
            ChargeState::PluggedInNotCharging
        );
        let far = battery_look(
            &ChargeReadings::single(Some(true), Some(NotCharging), Some(31), limit(80)),
            Language::TraditionalChinese,
        );
        assert_eq!(far.state, ChargeState::PluggedInNotCharging);
        assert_eq!(far.words, "已接上電源，沒有充電");
        assert_eq!(far.icon, BatteryIcon::PluggedCaution);
        assert!(!far.words.contains("維持"));
        assert_eq!(
            ChargeState::PluggedInNotCharging.words(Language::English),
            "Plugged in, not charging"
        );
    }

    #[test]
    fn a_plausible_start_threshold_is_the_lower_edge_of_the_hold() {
        let window = ChargeLimit::Percent {
            end: 80,
            start: Some(60),
        };
        let at = |percent| one(Some(true), Some(NotCharging), Some(percent), window);
        assert_eq!(at(61), held(80, 61));
        assert_eq!(at(60), held(80, 60));
        assert_eq!(at(59), ChargeState::PluggedInNotCharging);
    }

    /// ThinkPads ship with a start threshold of 0: at 30% and not charging
    /// with the charger in, that is a fault, not a hold.
    #[test]
    fn an_implausible_start_threshold_falls_back_to_the_tolerance() {
        for start in [0, 59, 80, 90] {
            let window = ChargeLimit::Percent {
                end: 80,
                start: Some(start),
            };
            assert_eq!(
                one(Some(true), Some(NotCharging), Some(30), window),
                ChargeState::PluggedInNotCharging,
                "start {start}"
            );
            assert_eq!(
                one(Some(true), Some(NotCharging), Some(76), window),
                held(80, 76),
                "start {start}"
            );
            assert_eq!(
                one(Some(true), Some(NotCharging), Some(74), window),
                ChargeState::PluggedInNotCharging,
                "start {start}"
            );
        }
    }

    #[test]
    fn charging_is_the_kernels_own_answer() {
        for charger in [Some(true), Some(false), None] {
            let look = battery_look(
                &ChargeReadings::single(charger, Some(Charging), Some(42), limit(80)),
                Language::TraditionalChinese,
            );
            assert_eq!(look.state, ChargeState::Charging);
            assert_eq!(look.icon, BatteryIcon::Charging);
            assert_eq!(look.words, "充電中");
        }
    }

    /// A charger too weak for the load: the person can see it is plugged in,
    /// so "on battery" would be false.
    #[test]
    fn discharging_with_the_charger_in_is_draining_never_on_battery() {
        let look = battery_look(
            &ChargeReadings::single(Some(true), Some(Discharging), Some(64), limit(80)),
            Language::English,
        );
        assert_eq!(look.state, ChargeState::PluggedInDraining);
        assert_eq!(look.words, "Plugged in, still draining");
        assert_eq!(look.icon, BatteryIcon::PluggedCaution);
        assert_eq!(look.state.on_battery(), Some(true));
        assert_eq!(
            ChargeState::PluggedInDraining.words(Language::TraditionalChinese),
            "已接上電源，仍在耗電"
        );
        for charger in [Some(false), None] {
            assert_eq!(
                one(charger, Some(Discharging), Some(64), limit(80)),
                ChargeState::OnBattery
            );
        }
    }

    #[test]
    fn plugged_in_and_full() {
        let look = battery_look(
            &ChargeReadings::single(Some(true), Some(Full), Some(100), ChargeLimit::NotSupported),
            Language::English,
        );
        assert_eq!(look.state, ChargeState::PluggedInFull);
        assert_eq!(look.icon, BatteryIcon::FullPlugged);
        assert_eq!(look.words, "Plugged in, fully charged");
        // A limit of 100 is no limit.
        assert_eq!(
            one(Some(true), Some(NotCharging), Some(97), limit(100)),
            ChargeState::PluggedInFull
        );
        let none = ChargeLimit::NotSupported;
        assert_eq!(
            one(Some(true), Some(NotCharging), Some(96), none),
            ChargeState::PluggedInFull
        );
        assert_eq!(
            one(Some(true), Some(NotCharging), Some(94), none),
            ChargeState::PluggedInNotCharging
        );
    }

    #[test]
    fn firmware_that_says_full_at_the_limit_is_held() {
        assert_eq!(
            one(Some(true), Some(Full), Some(80), limit(80)),
            held(80, 80)
        );
        // Full below the window is still full: the kernel's idea of full.
        assert_eq!(
            one(Some(true), Some(Full), Some(60), limit(80)),
            ChargeState::PluggedInFull
        );
    }

    /// Before Linux 5.19 (commit 185d20694a8a) the ACPI battery driver said
    /// `Unknown` for a battery stopped at its threshold. Inside the hold
    /// window that is held; anywhere else it stays unknown.
    #[test]
    fn the_older_kernels_unknown_at_the_limit_is_held() {
        let unknown = Some(BatteryStatus::Unknown);
        assert_eq!(one(Some(true), unknown, Some(80), limit(80)), held(80, 80));
        assert_eq!(one(Some(true), unknown, Some(77), limit(80)), held(80, 77));
        assert_eq!(
            one(Some(true), unknown, Some(100), ChargeLimit::NotSupported),
            ChargeState::PluggedInFull
        );
        assert_eq!(
            one(Some(true), unknown, Some(50), limit(80)),
            ChargeState::Unknown
        );
        assert_eq!(
            one(Some(true), unknown, Some(80), ChargeLimit::Unreadable),
            ChargeState::Unknown
        );
        assert_eq!(
            one(None, unknown, Some(80), limit(80)),
            ChargeState::Unknown
        );
    }

    #[test]
    fn a_charger_that_is_offline_means_the_battery_runs_the_machine() {
        for status in [
            Some(NotCharging),
            Some(Full),
            Some(BatteryStatus::Unknown),
            None,
        ] {
            assert_eq!(
                one(Some(false), status, Some(80), limit(80)),
                ChargeState::OnBattery,
                "{status:?}"
            );
        }
    }

    #[test]
    fn a_missing_reading_is_unknown_never_a_guess() {
        assert_eq!(
            one(None, Some(NotCharging), Some(80), limit(80)),
            ChargeState::Unknown
        );
        assert_eq!(
            one(Some(true), Some(NotCharging), None, limit(80)),
            ChargeState::Unknown
        );
        assert_eq!(
            one(Some(true), None, Some(80), limit(80)),
            ChargeState::Unknown
        );
        assert_eq!(
            one(
                Some(true),
                Some(NotCharging),
                Some(80),
                ChargeLimit::Unreadable
            ),
            ChargeState::Unknown
        );
        // Only at 100% does an unreadable limit not matter.
        assert_eq!(
            one(
                Some(true),
                Some(NotCharging),
                Some(100),
                ChargeLimit::Unreadable
            ),
            ChargeState::PluggedInFull
        );
        let look = battery_look(
            &ChargeReadings::single(None, Some(NotCharging), None, limit(80)),
            Language::English,
        );
        assert_eq!(look.icon, BatteryIcon::Unknown);
        assert_eq!(look.words, "Battery state unknown");
        assert_eq!(look.state.on_battery(), None);
        // No battery reading at all: only "the charger is out" can be said.
        let none = |charger| {
            charge_state(&ChargeReadings {
                charger_online: charger,
                batteries: Vec::new(),
            })
        };
        assert_eq!(none(Some(false)), ChargeState::OnBattery);
        assert_eq!(none(Some(true)), ChargeState::Unknown);
    }

    fn part(status: BatteryStatus, percent: u8, now: f64, full: f64, end: u8) -> BatteryPart {
        BatteryPart {
            status: Some(status),
            percent: Some(percent),
            energy_now: Some(now),
            energy_full: Some(full),
            limit: limit(end),
        }
    }

    /// A ThinkPad with two batteries: the firmware charges one at a time, so
    /// the other's `Not charging` is not a fault.
    #[test]
    fn two_batteries_are_one_state() {
        let both = |a: BatteryPart, b: BatteryPart| ChargeReadings {
            charger_online: Some(true),
            batteries: vec![a, b],
        };
        let charging_one = both(
            part(NotCharging, 60, 14.0, 23.0, 80),
            part(Charging, 40, 9.0, 23.0, 80),
        );
        assert_eq!(charge_state(&charging_one), ChargeState::Charging);
        // Energy-weighted: 23 Wh of 46.
        assert_eq!(charging_one.percent(), Some(50));

        let both_held = both(
            part(NotCharging, 80, 18.4, 23.0, 80),
            part(NotCharging, 78, 39.0, 50.0, 80),
        );
        assert_eq!(charge_state(&both_held), held(80, 79));

        let one_low = both(
            part(NotCharging, 80, 18.4, 23.0, 80),
            part(NotCharging, 30, 15.0, 50.0, 80),
        );
        assert_eq!(charge_state(&one_low), ChargeState::PluggedInNotCharging);

        let one_draining = ChargeReadings {
            charger_online: Some(true),
            batteries: vec![
                part(NotCharging, 80, 18.4, 23.0, 80),
                part(Discharging, 70, 35.0, 50.0, 80),
            ],
        };
        assert_eq!(charge_state(&one_draining), ChargeState::PluggedInDraining);

        // Without energies the charge is the plain average.
        let plain = ChargeReadings {
            charger_online: Some(true),
            batteries: vec![
                BatteryPart::new(Some(NotCharging), Some(80), limit(80)),
                BatteryPart::new(Some(NotCharging), Some(77), limit(80)),
            ],
        };
        assert_eq!(plain.percent(), Some(79));
        assert_eq!(charge_state(&plain), held(80, 79));
    }

    #[test]
    fn what_counts_as_a_charger() {
        for kind in ["Mains", "USB", "Wireless", "USB_PD", "USB_C", "usb"] {
            assert!(counts_as_charger(kind), "{kind}");
        }
        for kind in ["Battery", "UPS", "", "Something"] {
            assert!(!counts_as_charger(kind), "{kind}");
        }
        assert_eq!(
            charger_online([("Mains", Some(false)), ("USB", Some(true))]),
            Some(true)
        );
        assert_eq!(charger_online([("UPS", Some(true))]), None);
        assert_eq!(
            charger_online([("Mains", Some(false)), ("UPS", Some(true))]),
            Some(false)
        );
        assert_eq!(
            charger_online([("Mains", None), ("USB", Some(true))]),
            Some(true)
        );
        assert_eq!(
            charger_online([("Mains", Some(false)), ("USB", None)]),
            None
        );
        assert_eq!(charger_online(std::iter::empty()), None);
    }

    #[test]
    fn levels_on_battery() {
        let at = |percent| {
            battery_look(
                &ChargeReadings::single(
                    Some(false),
                    Some(Discharging),
                    percent,
                    ChargeLimit::NotSupported,
                ),
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
    fn time_means_something_only_while_the_charge_moves() {
        assert!(ChargeState::OnBattery.has_time());
        assert!(ChargeState::Charging.has_time());
        assert!(ChargeState::PluggedInDraining.has_time());
        assert!(!held(80, 80).has_time());
        assert!(!ChargeState::PluggedInFull.has_time());
        assert!(!ChargeState::PluggedInNotCharging.has_time());
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
    fn the_spoken_name_gives_the_charge_and_the_state() {
        let held = battery_look(&owner(), Language::TraditionalChinese);
        assert_eq!(held.spoken, "電池 80%，已接上電源，維持在 80%");
        assert_eq!(
            battery_look(&owner(), Language::English).spoken,
            "Battery 80%: Plugged in, holding at 80%"
        );
        let charging = battery_look(
            &ChargeReadings::single(Some(true), Some(Charging), Some(87), limit(100)),
            Language::TraditionalChinese,
        );
        assert_eq!(charging.spoken, "電池 87%，充電中");
        let no_figure = battery_look(
            &ChargeReadings::single(Some(true), Some(Charging), None, limit(100)),
            Language::English,
        );
        assert_eq!(no_figure.spoken, "Battery: Charging");
        let unknown = battery_look(
            &ChargeReadings::single(None, None, Some(50), limit(80)),
            Language::English,
        );
        assert_eq!(unknown.state, ChargeState::Unknown);
        assert_eq!(unknown.spoken, "Battery state unknown");
        // Every state's name carries its full words, in both languages.
        for state in ChargeState::KINDS {
            for language in [Language::English, Language::TraditionalChinese] {
                assert!(spoken(state, Some(42), language).contains(&state.words(language)));
            }
        }
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
