//! The closed set of settings Better Touchpad owns, and what a backend can say
//! about one.
//!
//! A setting is an identity, not a string: a backend maps it onto whatever it
//! actually stores, and nothing above this layer knows that mapping exists.
//! A reading keeps "holds this value", "the session default applies", "this
//! backend cannot do it", "cannot be determined", and "not allowed to look"
//! apart, for the same reason `better-core` keeps them apart for defaults —
//! only one of them can be safely overwritten, and only one of them can be
//! restored to.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::value::{AccelerationProfile, ClickMethod, ScrollFactor, Sensitivity, ValueError};

/// The screens a setting belongs to, and the unit a restore can be scoped to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Section {
    Pointer,
    Scrolling,
    Clicking,
}

impl Section {
    pub const ALL: [Self; 3] = [Self::Pointer, Self::Scrolling, Self::Clicking];

    pub fn key(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Scrolling => "scrolling",
            Self::Clicking => "clicking",
        }
    }
}

/// What sort of value a setting carries. A backend that offered a boolean for
/// a sensitivity would be caught here rather than in a slider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueKind {
    Sensitivity,
    Factor,
    Toggle,
    Acceleration,
    Click,
}

impl ValueKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sensitivity => "sensitivity",
            Self::Factor => "factor",
            Self::Toggle => "toggle",
            Self::Acceleration => "acceleration profile",
            Self::Click => "click method",
        }
    }
}

/// Every control Better Touchpad phase 1 owns.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingId {
    PointerSensitivity,
    AccelerationProfile,
    DisableWhileTyping,
    VerticalScrollFactor,
    HorizontalScrollFactor,
    NaturalScrolling,
    TwoFingerScrolling,
    SmoothScrolling,
    TapToClick,
    TapAndDrag,
    DragLock,
    ClickMethod,
    MiddleClickEmulation,
}

impl SettingId {
    pub const ALL: [Self; 13] = [
        Self::PointerSensitivity,
        Self::AccelerationProfile,
        Self::DisableWhileTyping,
        Self::VerticalScrollFactor,
        Self::HorizontalScrollFactor,
        Self::NaturalScrolling,
        Self::TwoFingerScrolling,
        Self::SmoothScrolling,
        Self::TapToClick,
        Self::TapAndDrag,
        Self::DragLock,
        Self::ClickMethod,
        Self::MiddleClickEmulation,
    ];

    /// A stable machine key. Presentation layers own the wording; this is what
    /// a log, a report, or a saved backup carries.
    pub fn key(self) -> &'static str {
        match self {
            Self::PointerSensitivity => "pointer.sensitivity",
            Self::AccelerationProfile => "pointer.acceleration-profile",
            Self::DisableWhileTyping => "pointer.disable-while-typing",
            Self::VerticalScrollFactor => "scrolling.vertical-factor",
            Self::HorizontalScrollFactor => "scrolling.horizontal-factor",
            Self::NaturalScrolling => "scrolling.natural",
            Self::TwoFingerScrolling => "scrolling.two-finger",
            Self::SmoothScrolling => "scrolling.smooth",
            Self::TapToClick => "clicking.tap-to-click",
            Self::TapAndDrag => "clicking.tap-and-drag",
            Self::DragLock => "clicking.drag-lock",
            Self::ClickMethod => "clicking.click-method",
            Self::MiddleClickEmulation => "clicking.middle-click-emulation",
        }
    }

    pub fn section(self) -> Section {
        match self {
            Self::PointerSensitivity | Self::AccelerationProfile | Self::DisableWhileTyping => {
                Section::Pointer
            }
            Self::VerticalScrollFactor
            | Self::HorizontalScrollFactor
            | Self::NaturalScrolling
            | Self::TwoFingerScrolling
            | Self::SmoothScrolling => Section::Scrolling,
            Self::TapToClick
            | Self::TapAndDrag
            | Self::DragLock
            | Self::ClickMethod
            | Self::MiddleClickEmulation => Section::Clicking,
        }
    }

    pub fn kind(self) -> ValueKind {
        match self {
            Self::PointerSensitivity => ValueKind::Sensitivity,
            Self::AccelerationProfile => ValueKind::Acceleration,
            Self::VerticalScrollFactor | Self::HorizontalScrollFactor => ValueKind::Factor,
            Self::ClickMethod => ValueKind::Click,
            Self::DisableWhileTyping
            | Self::NaturalScrolling
            | Self::TwoFingerScrolling
            | Self::SmoothScrolling
            | Self::TapToClick
            | Self::TapAndDrag
            | Self::DragLock
            | Self::MiddleClickEmulation => ValueKind::Toggle,
        }
    }

    /// The settings in one section, in the order the screens show them.
    pub fn in_section(section: Section) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|setting| setting.section() == section)
            .collect()
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.key() == key)
    }
}

impl fmt::Display for SettingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

/// One setting's value, still typed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingValue {
    Sensitivity { value: Sensitivity },
    Factor { value: ScrollFactor },
    Toggle { value: bool },
    Acceleration { value: AccelerationProfile },
    Click { value: ClickMethod },
}

impl SettingValue {
    pub fn sensitivity(value: Sensitivity) -> Self {
        Self::Sensitivity { value }
    }

    pub fn factor(value: ScrollFactor) -> Self {
        Self::Factor { value }
    }

    pub fn toggle(value: bool) -> Self {
        Self::Toggle { value }
    }

    pub fn acceleration(value: AccelerationProfile) -> Self {
        Self::Acceleration { value }
    }

    pub fn click(value: ClickMethod) -> Self {
        Self::Click { value }
    }

    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Sensitivity { .. } => ValueKind::Sensitivity,
            Self::Factor { .. } => ValueKind::Factor,
            Self::Toggle { .. } => ValueKind::Toggle,
            Self::Acceleration { .. } => ValueKind::Acceleration,
            Self::Click { .. } => ValueKind::Click,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Sensitivity { value } => Some(value.get()),
            Self::Factor { value } => Some(value.get()),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Toggle { value } => Some(*value),
            _ => None,
        }
    }

    /// Refuses a value whose kind does not match the setting it is offered for.
    pub fn validate_for(&self, setting: SettingId) -> Result<(), ValueError> {
        if self.kind() == setting.kind() {
            return Ok(());
        }
        Err(ValueError::WrongKind {
            setting: setting.key(),
            expected: setting.kind().name(),
            found: self.kind().name(),
        })
    }
}

/// What a backend saw when it looked.
///
/// `SessionDefault` is its own answer and not a kind of unknown: it means the
/// user's own database holds nothing for this setting, so the session's default
/// applies and Better OS has never changed it. Restoring to that state means
/// removing the entry again, which is a different write from putting a value
/// back, and collapsing the two would make restore quietly wrong.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reading", rename_all = "snake_case")]
pub enum Reading {
    Value { value: SettingValue },
    SessionDefault { reason: String },
    Unsupported { reason: String },
    Unknown { reason: String },
    PermissionDenied { reason: String },
}

impl Reading {
    pub fn value(value: SettingValue) -> Self {
        Self::Value { value }
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
        }
    }

    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    pub fn session_default(reason: impl Into<String>) -> Self {
        Self::SessionDefault {
            reason: reason.into(),
        }
    }

    /// Whether this reading is definite enough to compare against or restore
    /// to. `SessionDefault` is definite: "nothing was set" is a state that can
    /// be reproduced exactly.
    pub fn is_determinate(&self) -> bool {
        matches!(self, Self::Value { .. } | Self::SessionDefault { .. })
    }

    pub fn as_value(&self) -> Option<SettingValue> {
        match self {
            Self::Value { value } => Some(*value),
            _ => None,
        }
    }

    /// The machine key behind a reading that is not a value.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Value { .. } => None,
            Self::SessionDefault { reason }
            | Self::Unsupported { reason }
            | Self::Unknown { reason }
            | Self::PermissionDenied { reason } => Some(reason),
        }
    }
}

/// When a change takes effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEffect {
    Immediate,
    SignOutRequired,
}

/// Whether a backend can actually own a setting.
///
/// There is no third state. A backend either reads, applies, and verifies a
/// setting, or it says why it cannot — which is what the GUI renders instead of
/// a switch that does nothing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "support", rename_all = "snake_case")]
pub enum Support {
    Full { effect: SessionEffect },
    Unavailable { reason: String, detail: String },
}

impl Support {
    pub fn immediate() -> Self {
        Self::Full {
            effect: SessionEffect::Immediate,
        }
    }

    pub fn sign_out_required() -> Self {
        Self::Full {
            effect: SessionEffect::SignOutRequired,
        }
    }

    pub fn unavailable(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
            detail: detail.into(),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Full { .. })
    }

    pub fn effect(&self) -> Option<SessionEffect> {
        match self {
            Self::Full { effect } => Some(*effect),
            Self::Unavailable { .. } => None,
        }
    }
}

/// What a backend says it can do, setting by setting.
///
/// A setting missing from the map is unavailable, not assumed working. Building
/// one from a partial list is therefore safe by default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capabilities {
    entries: BTreeMap<SettingId, Support>,
}

impl Capabilities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, setting: SettingId, support: Support) -> Self {
        self.entries.insert(setting, support);
        self
    }

    pub fn insert(&mut self, setting: SettingId, support: Support) {
        self.entries.insert(setting, support);
    }

    /// Every setting, immediate, with nothing unavailable. Only a backend that
    /// genuinely owns all thirteen may build one.
    pub fn everything_immediate() -> Self {
        let mut set = Self::new();
        for setting in SettingId::ALL {
            set.insert(setting, Support::immediate());
        }
        set
    }

    pub fn support(&self, setting: SettingId) -> Support {
        self.entries.get(&setting).cloned().unwrap_or_else(|| {
            Support::unavailable(
                "touchpad.backend_declares_no_support",
                "the active backend did not declare this setting at all",
            )
        })
    }

    pub fn is_available(&self, setting: SettingId) -> bool {
        self.support(setting).is_available()
    }

    pub fn available(&self) -> Vec<SettingId> {
        SettingId::ALL
            .into_iter()
            .filter(|setting| self.is_available(*setting))
            .collect()
    }

    pub fn unavailable(&self) -> Vec<SettingId> {
        SettingId::ALL
            .into_iter()
            .filter(|setting| !self.is_available(*setting))
            .collect()
    }

    /// Whether anything in this section takes effect only after a sign-out.
    pub fn section_needs_sign_out(&self, section: Section) -> bool {
        SettingId::in_section(section)
            .into_iter()
            .any(|setting| self.support(setting).effect() == Some(SessionEffect::SignOutRequired))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_belongs_to_exactly_one_section_and_has_a_unique_key() {
        let mut keys: Vec<&str> = SettingId::ALL.iter().map(|setting| setting.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SettingId::ALL.len());

        let counted: usize = Section::ALL
            .into_iter()
            .map(|section| SettingId::in_section(section).len())
            .sum();
        assert_eq!(counted, SettingId::ALL.len());
    }

    #[test]
    fn every_setting_key_parses_back_to_itself() {
        for setting in SettingId::ALL {
            assert_eq!(SettingId::parse(setting.key()), Some(setting));
        }
        assert_eq!(SettingId::parse("pointer.telepathy"), None);
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_refused_for_the_setting() {
        let error = SettingValue::toggle(true)
            .validate_for(SettingId::PointerSensitivity)
            .unwrap_err();
        assert_eq!(
            error,
            ValueError::WrongKind {
                setting: "pointer.sensitivity",
                expected: "sensitivity",
                found: "toggle",
            }
        );
    }

    #[test]
    fn every_setting_accepts_a_value_of_its_own_kind() {
        for setting in SettingId::ALL {
            let value = match setting.kind() {
                ValueKind::Sensitivity => SettingValue::sensitivity(Sensitivity::neutral()),
                ValueKind::Factor => SettingValue::factor(ScrollFactor::neutral()),
                ValueKind::Toggle => SettingValue::toggle(true),
                ValueKind::Acceleration => {
                    SettingValue::acceleration(AccelerationProfile::Adaptive)
                }
                ValueKind::Click => SettingValue::click(ClickMethod::Fingers),
            };
            assert!(value.validate_for(setting).is_ok(), "{setting} refused");
        }
    }

    #[test]
    fn a_setting_the_backend_never_mentioned_is_unavailable_rather_than_assumed() {
        let capabilities = Capabilities::new().with(SettingId::TapToClick, Support::immediate());
        assert!(capabilities.is_available(SettingId::TapToClick));
        assert!(!capabilities.is_available(SettingId::PointerSensitivity));
        assert_eq!(capabilities.available(), vec![SettingId::TapToClick]);
        assert_eq!(capabilities.unavailable().len(), SettingId::ALL.len() - 1);
    }

    #[test]
    fn a_sign_out_setting_marks_its_whole_section() {
        let capabilities = Capabilities::everything_immediate()
            .with(SettingId::SmoothScrolling, Support::sign_out_required());
        assert!(capabilities.section_needs_sign_out(Section::Scrolling));
        assert!(!capabilities.section_needs_sign_out(Section::Pointer));
    }

    #[test]
    fn the_session_default_reading_is_definite_but_holds_no_value() {
        let reading = Reading::session_default("gnome.no_user_scope_value");
        assert!(reading.is_determinate());
        assert_eq!(reading.as_value(), None);
        assert_eq!(reading.reason(), Some("gnome.no_user_scope_value"));

        assert!(!Reading::unknown("x").is_determinate());
        assert!(!Reading::unsupported("x").is_determinate());
        assert!(
            !Reading::PermissionDenied {
                reason: "x".to_string()
            }
            .is_determinate()
        );
    }
}
