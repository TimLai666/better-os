//! What the Touchpad page decides, with no GPUI in it: its sub-pages, which
//! state it is in, the "This touchpad" card, the Apply bar's sentence, what
//! the search can find on it, and what Home shows about it (spec sections 2,
//! 3, and 5). The controls themselves are decided by `touchpad-views`.

use better_core::ComponentId;
use touchpad_core::{HealthState, SettingId};
use touchpad_platform::{Session, SessionKind};
use touchpad_views::i18n::{self as tp, copy as tp_copy};
use touchpad_views::{Page as ViewPage, TouchpadModel};

use super::copy::page_copy;
use crate::attention::{AttentionAction, AttentionItem, Severity, Snapshot, row};
use crate::i18n::{Locale, shell};
use crate::navigation::Route;
use crate::pages::states::PageState;
use crate::search::SearchEntry;

/// The component this page is the window of.
pub(crate) const COMPONENT_ID: &str = "better-touchpad";
pub(crate) const COMPONENT_NAME: &str = "Better Touchpad";

/// The oldest Better Touchpad whose files this page reads and writes: gesture
/// profiles at schema version 2 (one per device) first shipped in 0.2.1, and
/// a gesture service older than that would read the page's file as damaged.
pub(crate) const REQUIRED_VERSION: &str = "0.2.1";

/// The six sub-pages, in the selector bar's order. Today's Overview is not
/// one: its facts are the "This touchpad" card above the selector.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) enum SubPage {
    #[default]
    Pointer,
    Scrolling,
    Clicking,
    Gestures,
    Devices,
    Diagnostics,
}

impl SubPage {
    pub(crate) const ALL: [Self; 6] = [
        Self::Pointer,
        Self::Scrolling,
        Self::Clicking,
        Self::Gestures,
        Self::Devices,
        Self::Diagnostics,
    ];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|page| *page == self).unwrap_or(0)
    }

    /// The screen of `touchpad-views` this sub-page draws.
    pub(crate) fn view(self) -> ViewPage {
        match self {
            Self::Pointer => ViewPage::Pointer,
            Self::Scrolling => ViewPage::Scrolling,
            Self::Clicking => ViewPage::Clicking,
            Self::Gestures => ViewPage::Gestures,
            Self::Devices => ViewPage::Devices,
            Self::Diagnostics => ViewPage::Diagnostics,
        }
    }

    /// A sub-page from the screen key `--page` and the restart hand-off use.
    /// Today's Overview opens Pointer, as `--page touchpad` does.
    pub(crate) fn from_key(key: &str) -> Self {
        match ViewPage::parse(key) {
            Some(ViewPage::Scrolling) => Self::Scrolling,
            Some(ViewPage::Clicking) => Self::Clicking,
            Some(ViewPage::Gestures) => Self::Gestures,
            Some(ViewPage::Devices) => Self::Devices,
            Some(ViewPage::Diagnostics) => Self::Diagnostics,
            _ => Self::Pointer,
        }
    }

    pub(crate) fn key(self) -> &'static str {
        self.view().key()
    }

    pub(crate) fn label(self, locale: Locale) -> &'static str {
        self.view().label(tp_copy(views_locale(locale)))
    }

    /// The sub-page a search result's card lives on.
    pub(crate) fn of_card(card: &str) -> Option<Self> {
        if let Some(setting) = SettingId::ALL.iter().find(|setting| setting.key() == card) {
            return Some(match setting.section() {
                touchpad_core::Section::Pointer => Self::Pointer,
                touchpad_core::Section::Scrolling => Self::Scrolling,
                touchpad_core::Section::Clicking => Self::Clicking,
            });
        }
        Some(match card {
            card::POINTER_TEST => Self::Pointer,
            card::LINKED_AXES | card::SCROLL_TEST => Self::Scrolling,
            card::GESTURE_PRESET
            | card::GESTURE_PROFILE
            | card::GESTURE_TEST
            | card::GESTURE_FILE
            | card::GESTURES_OFF => Self::Gestures,
            card::DEVICES => Self::Devices,
            card::EFFECTIVE | card::RESTORE | card::SAFE_MODE => Self::Diagnostics,
            _ => return None,
        })
    }
}

/// The ids of the page's cards that are not a setting (a setting's card is
/// its `SettingId::key`). Search scrolls to them.
pub(crate) mod card {
    pub(crate) const THIS_TOUCHPAD: &str = "touchpad-this";
    pub(crate) const POINTER_TEST: &str = "touchpad-pointer-test";
    pub(crate) const LINKED_AXES: &str = "touchpad-linked-axes";
    pub(crate) const SCROLL_TEST: &str = "touchpad-scroll-test";
    pub(crate) const GESTURE_PRESET: &str = "touchpad-gesture-preset";
    pub(crate) const GESTURE_PROFILE: &str = "touchpad-gesture-profile";
    pub(crate) const GESTURE_TEST: &str = "touchpad-gesture-test";
    pub(crate) const GESTURE_FILE: &str = "touchpad-gesture-file";
    pub(crate) const GESTURES_OFF: &str = "touchpad-gestures-off";
    pub(crate) const DEVICES: &str = "touchpad-devices";
    pub(crate) const EFFECTIVE: &str = "touchpad-effective";
    pub(crate) const RESTORE: &str = "touchpad-restore";
    pub(crate) const SAFE_MODE: &str = "touchpad-safe-mode";
}

/// The control center's language as `touchpad-views` spells it.
pub(crate) fn views_locale(locale: Locale) -> touchpad_views::Locale {
    match locale.resolved() {
        Locale::ZhTw => touchpad_views::Locale::ZhTw,
        _ => touchpad_views::Locale::EnUs,
    }
}

/// What Home and the page know about Better Touchpad without opening it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TouchpadFacts {
    /// What the manager's record says is installed; `None` is not installed.
    pub(crate) installed: Option<String>,
    pub(crate) safe_mode: bool,
    /// The gesture pipeline turned gestures off after failing (its marker
    /// file is there).
    pub(crate) gestures_turned_off: bool,
}

impl TouchpadFacts {
    /// The two markers, read from the touchpad's store directory: two
    /// `stat` calls, cheap enough for every time Home is shown.
    pub(crate) fn read_markers(&mut self, directory: &std::path::Path) {
        let store = touchpad_core::TouchpadStore::new(directory);
        self.safe_mode = store.safe_mode_enabled();
        self.gestures_turned_off =
            touchpad_gestures::GestureStore::at(directory).turned_off_by_itself();
    }
}

/// Whether the page can be used, before any of its controls is drawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Availability {
    Ready,
    /// Spec section 2's "not installed" or "too old" card.
    Blocked(PageState),
}

pub(crate) fn availability(installed: Option<&str>) -> Availability {
    let component = || COMPONENT_NAME.to_string();
    let Some(installed) = installed else {
        return Availability::Blocked(PageState::NotInstalled {
            component: component(),
        });
    };
    let required = semver::Version::parse(REQUIRED_VERSION).expect("a valid version");
    // A version semver cannot read (a tilde pre-release dpkg holds) is not
    // judged too old: the Apps page already says it cannot be ordered, and
    // refusing the page over it would invent a fact.
    match semver::Version::parse(installed) {
        Ok(version) if version < required => Availability::Blocked(PageState::TooOld {
            component: component(),
            installed: installed.to_string(),
            required: REQUIRED_VERSION.to_string(),
        }),
        _ => Availability::Ready,
    }
}

/// The bars above the selector, in this order (spec section 5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Notice {
    /// `InfoBar` Informational.
    NotGnome,
    /// `InfoBar` Warning with "Turn off safe mode".
    SafeMode,
}

pub(crate) fn notices(session: &Session, safe_mode: bool) -> Vec<Notice> {
    let mut notices = Vec::new();
    if !session.is_gnome() {
        notices.push(Notice::NotGnome);
    }
    if safe_mode {
        notices.push(Notice::SafeMode);
    }
    notices
}

/// The Apply bar's sentence, or nothing when nothing is staged.
pub(crate) fn apply_bar_text(locale: Locale, staged: usize) -> Option<String> {
    let c = page_copy(locale);
    match staged {
        0 => None,
        1 => Some(c.apply_bar_one.to_string()),
        n => Some(c.apply_bar_many.replace("{n}", &n.to_string())),
    }
}

/// How many settings are staged and not applied.
pub(crate) fn staged_count(model: &TouchpadModel) -> usize {
    model.overview().pending_count
}

pub(crate) fn session_words(session: &Session, locale: Locale) -> String {
    let c = page_copy(locale);
    let kind = match session.kind {
        SessionKind::Wayland => c.session_wayland,
        SessionKind::X11 => c.session_x11,
        SessionKind::Unknown => c.session_unknown,
    };
    if session.is_gnome() {
        format!("GNOME · {kind}")
    } else {
        kind.to_string()
    }
}

/// The backend, named for a person: GNOME's settings, or the demo's memory.
pub(crate) fn backend_words(name: &str, reachable: bool, locale: Locale) -> &'static str {
    let c = page_copy(locale);
    match (name, reachable) {
        ("mock", _) => c.backend_memory,
        (_, true) => c.backend_gnome,
        (_, false) => c.backend_unreachable,
    }
}

/// Whether gestures reach the desktop, in words. The adapter's own name
/// (`mock`, `gnome-shell (46.0)`) is never on screen (audit T1).
pub(crate) fn adapter_words(
    description: &touchpad_session::AdapterDescription,
    locale: Locale,
) -> &'static str {
    let c = page_copy(locale);
    if description.name.contains("gnome-shell") {
        c.adapter_desktop
    } else if description.performs_system_actions {
        c.adapter_launcher
    } else {
        c.adapter_none
    }
}

/// The "This touchpad" card: what today's Overview showed, in words, with
/// no hardware identifier (audit T4: those are under Devices › Details).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ThisTouchpad {
    /// The touchpad's name, or "No touchpad".
    pub(crate) device: String,
    pub(crate) session: String,
    pub(crate) backend: &'static str,
    pub(crate) health: &'static str,
    pub(crate) health_state: HealthState,
    pub(crate) pointer: String,
    pub(crate) scroll: String,
}

pub(crate) fn this_touchpad(model: &TouchpadModel, locale: Locale) -> ThisTouchpad {
    let overview = model.overview();
    ThisTouchpad {
        device: model
            .selected_device()
            .map(|device| friendly_name(&device.name))
            .unwrap_or_else(|| page_copy(locale).this_touchpad_none.to_string()),
        session: session_words(model.session(), locale),
        backend: backend_words(
            model.backend_status().name,
            overview.backend_reachable,
            locale,
        ),
        health: tp::health_words(overview.health, views_locale(locale)),
        health_state: overview.health,
        pointer: overview.pointer_summary,
        scroll: overview.scroll_summary,
    }
}

/// The settings cards the search finds on this page, from the same tables the
/// page draws them from.
pub(crate) fn search_entries(locale: Locale) -> Vec<SearchEntry> {
    let c = tp_copy(views_locale(locale));
    let p = page_copy(locale);
    let page = shell(locale).touchpad;
    let entry = |header, description, group: SubPage, card| SearchEntry {
        header,
        description,
        page,
        group: Some(group.label(locale)),
        route: Route::Touchpad,
        card,
    };
    let mut entries = vec![SearchEntry {
        header: p.this_touchpad,
        description: "",
        page,
        group: None,
        route: Route::Touchpad,
        card: card::THIS_TOUCHPAD,
    }];
    for setting in SettingId::ALL {
        let group = SubPage::of_card(setting.key()).unwrap_or_default();
        let header = touchpad_views::model::label_for(setting, c);
        // The two scroll factors are "Vertical" and "Horizontal" under the
        // scroll-sensitivity group, so the search also says which.
        let description = match setting {
            SettingId::VerticalScrollFactor | SettingId::HorizontalScrollFactor => {
                c.scroll_sensitivity
            }
            _ => "",
        };
        entries.push(entry(header, description, group, setting.key()));
    }
    entries.extend([
        entry(
            c.pointer_test_title,
            c.pointer_test_hint,
            SubPage::Pointer,
            card::POINTER_TEST,
        ),
        entry(c.linked_axes, "", SubPage::Scrolling, card::LINKED_AXES),
        entry(
            c.scroll_test_title,
            c.scroll_test_hint,
            SubPage::Scrolling,
            card::SCROLL_TEST,
        ),
        entry(
            c.preset_title,
            c.preset_description,
            SubPage::Gestures,
            card::GESTURE_PRESET,
        ),
        entry(
            c.profile_heading,
            "",
            SubPage::Gestures,
            card::GESTURE_PROFILE,
        ),
        entry(
            c.test_gestures,
            c.test_gestures_hint,
            SubPage::Gestures,
            card::GESTURE_TEST,
        ),
        entry(c.profiles_file, "", SubPage::Gestures, card::GESTURE_FILE),
        entry(
            c.disable_gestures,
            "",
            SubPage::Gestures,
            card::GESTURES_OFF,
        ),
        entry(
            c.devices_title,
            c.devices_subtitle,
            SubPage::Devices,
            card::DEVICES,
        ),
        entry(
            c.effective_values,
            "",
            SubPage::Diagnostics,
            card::EFFECTIVE,
        ),
        entry(c.restore_actions, "", SubPage::Diagnostics, card::RESTORE),
        entry(
            p.safe_mode_card,
            p.safe_mode_description,
            SubPage::Diagnostics,
            card::SAFE_MODE,
        ),
    ]);
    entries
}

/// Home's two Touchpad rows (spec section 3).
pub(crate) fn attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    let facts = &snapshot.touchpad;
    if facts.installed.is_none() {
        return Vec::new();
    }
    let c = page_copy(snapshot.locale);
    let mut items = Vec::new();
    if facts.safe_mode {
        items.push(AttentionItem {
            source: Route::Touchpad,
            severity: Severity::Warning,
            row: row::TOUCHPAD_SAFE_MODE,
            header: c.home_safe_mode_header.to_string(),
            description: c.home_safe_mode_description.to_string(),
            action_label: tp_copy(views_locale(snapshot.locale)).safe_mode_off,
            action: AttentionAction::TurnOffTouchpadSafeMode,
        });
    }
    // Safe mode already says gestures are not handled; saying it twice
    // would be two cards for one state.
    if facts.gestures_turned_off && !facts.safe_mode {
        items.push(AttentionItem {
            source: Route::Touchpad,
            severity: Severity::Warning,
            row: row::TOUCHPAD_GESTURES_OFF,
            header: c.home_turned_off_header.to_string(),
            description: c.home_turned_off_description.to_string(),
            action_label: c.view,
            action: AttentionAction::OpenTouchpad(SubPage::Gestures),
        });
    }
    items
}

/// The component id, for the "not installed" card's Install button.
pub(crate) fn component_id() -> ComponentId {
    ComponentId::new(COMPONENT_ID).expect("a valid component id")
}

/// A touchpad's name without the bus and USB/I2C identifiers the kernel puts
/// in front of it (`ASCF1200:00 2808:0233 Touchpad` is "Touchpad"): those are
/// hardware identifiers, shown only under Devices › Details (audit T4). A
/// name that is nothing but identifiers is kept whole rather than emptied.
pub(crate) fn friendly_name(name: &str) -> String {
    let looks_like_id = |word: &str| {
        word.contains(':')
            && word
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c.is_ascii_uppercase())
    };
    let kept: Vec<&str> = name
        .split_whitespace()
        .filter(|word| !looks_like_id(word))
        .collect();
    if kept.is_empty() {
        name.to_string()
    } else {
        kept.join(" ")
    }
}
