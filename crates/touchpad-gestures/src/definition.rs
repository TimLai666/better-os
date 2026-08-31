//! What a gesture is.
//!
//! Every field Issue #3 requires per gesture lives here, and each of them is a
//! bounded type rather than a number someone remembered to check: a contact
//! count outside what a touchpad reports, a threshold outside `0..=1`, an
//! activation that sits below its own cancellation, or a cooldown long enough
//! to make the gesture feel broken are all refused at construction and named in
//! the refusal. That is the same rule `touchpad-core::value` applies to
//! sensitivity, for the same reason: a clamped value is a setting nobody chose.

use std::fmt;

use better_actions::DesktopAction;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum GestureError {
    #[error("gestures.contacts_out_of_range:{0}")]
    ContactsOutOfRange(u8),
    #[error("gestures.threshold_out_of_range:{name}:{value}")]
    ThresholdOutOfRange { name: &'static str, value: String },
    #[error("gestures.cancellation_not_below_activation:{activation}:{cancellation}")]
    CancellationNotBelowActivation {
        activation: String,
        cancellation: String,
    },
    #[error("gestures.cooldown_out_of_range:{0}")]
    CooldownOutOfRange(u64),
    #[error("gestures.direction_required:{0}")]
    DirectionRequired(&'static str),
    #[error("gestures.direction_not_allowed:{shape}:{direction}")]
    DirectionNotAllowed {
        shape: &'static str,
        direction: &'static str,
    },
    #[error("gestures.duplicate_id:{0}")]
    DuplicateId(String),
    #[error("gestures.unknown_id:{0}")]
    UnknownId(String),
    #[error("gestures.id_not_well_formed:{0}")]
    MalformedId(String),
    /// The keys chosen for a custom shortcut are not a shortcut. Carried here
    /// rather than as a second error type so that one refusal reaches the
    /// editor whether the gesture or its action is what was wrong.
    #[error("gestures.shortcut_not_usable:{0}")]
    ShortcutNotUsable(String),
}

/// The primitives a gesture can be built from.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GestureShape {
    /// Contacts travel together in one direction.
    Swipe,
    /// Contacts draw together.
    Pinch,
    /// Contacts spread apart.
    Spread,
    /// Contacts stay still for long enough.
    Hold,
    /// Contacts land and lift quickly without travelling.
    Tap,
    /// Contacts turn about their own centre.
    Rotate,
}

impl GestureShape {
    pub const ALL: [Self; 6] = [
        Self::Swipe,
        Self::Pinch,
        Self::Spread,
        Self::Hold,
        Self::Tap,
        Self::Rotate,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Swipe => "swipe",
            Self::Pinch => "pinch",
            Self::Spread => "spread",
            Self::Hold => "hold",
            Self::Tap => "tap",
            Self::Rotate => "rotate",
        }
    }

    /// Whether this shape means anything without a direction.
    pub fn needs_direction(self) -> bool {
        matches!(self, Self::Swipe | Self::Rotate)
    }

    /// Which directions this shape can carry.
    pub fn allowed_directions(self) -> &'static [Direction] {
        match self {
            Self::Swipe => &[
                Direction::Up,
                Direction::Down,
                Direction::Left,
                Direction::Right,
            ],
            Self::Rotate => &[Direction::Clockwise, Direction::CounterClockwise],
            _ => &[],
        }
    }

    /// Whether the shape draws contacts together. The launcher's own event
    /// vocabulary has exactly two directions, inward and outward, and this is
    /// how a Better Touchpad shape answers that question.
    pub fn is_inward(self) -> bool {
        matches!(self, Self::Pinch)
    }
}

/// Where a gesture goes.
///
/// Direction is configurable per gesture because natural-direction preferences
/// and workspace orientation differ, which Issue #3 requires and which is why
/// the preset stores a direction rather than hard-coding one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    Clockwise,
    CounterClockwise,
}

impl Direction {
    pub const ALL: [Self; 6] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Clockwise,
        Self::CounterClockwise,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Clockwise => "clockwise",
            Self::CounterClockwise => "counter-clockwise",
        }
    }

    /// The direction that undoes this one.
    pub fn reversed(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Clockwise => Self::CounterClockwise,
            Self::CounterClockwise => Self::Clockwise,
        }
    }
}

/// How many contact points a gesture needs.
///
/// The upper bound is five because that is the most a touchpad reports and the
/// most a hand has. Five-finger gestures are available as custom mappings and
/// are not the preset's answer for the launcher or for Show Desktop, which is
/// Issue #3's decision, not this type's.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct ContactCount(u8);

impl ContactCount {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 5;

    pub fn new(count: u8) -> Result<Self, GestureError> {
        if (Self::MIN..=Self::MAX).contains(&count) {
            Ok(Self(count))
        } else {
            Err(GestureError::ContactsOutOfRange(count))
        }
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for ContactCount {
    type Error = GestureError;

    fn try_from(count: u8) -> Result<Self, Self::Error> {
        Self::new(count)
    }
}

impl From<ContactCount> for u8 {
    fn from(count: ContactCount) -> Self {
        count.0
    }
}

impl fmt::Display for ContactCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A fraction of the way through a gesture.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct Threshold(f32);

impl Threshold {
    pub const MIN: f32 = 0.0;
    pub const MAX: f32 = 1.0;

    pub fn new(value: f32) -> Result<Self, GestureError> {
        if value.is_finite() && (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(GestureError::ThresholdOutOfRange {
                name: "threshold",
                value: value.to_string(),
            })
        }
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

impl TryFrom<f32> for Threshold {
    type Error = GestureError;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Threshold> for f32 {
    fn from(threshold: Threshold) -> Self {
        threshold.0
    }
}

/// How long after a gesture completes the same gesture is ignored.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Cooldown(u64);

impl Cooldown {
    /// No cooldown is a legitimate choice for a gesture that cannot flap, such
    /// as a volume step.
    pub const MIN_MS: u64 = 0;
    /// Two seconds. Longer than this and a deliberate second gesture is
    /// swallowed, which reads as the gesture being broken.
    pub const MAX_MS: u64 = 2_000;

    pub fn from_millis(milliseconds: u64) -> Result<Self, GestureError> {
        if milliseconds <= Self::MAX_MS {
            Ok(Self(milliseconds))
        } else {
            Err(GestureError::CooldownOutOfRange(milliseconds))
        }
    }

    pub fn as_millis(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for Cooldown {
    type Error = GestureError;

    fn try_from(milliseconds: u64) -> Result<Self, Self::Error> {
        Self::from_millis(milliseconds)
    }
}

impl From<Cooldown> for u64 {
    fn from(cooldown: Cooldown) -> Self {
        cooldown.0
    }
}

/// Which backend is producing the events for a gesture.
///
/// The list is short because the honest list is short: this build has a mock
/// source and no production one. ADR 0012 chooses what gets added, and adding a
/// variant before the adapter exists would make the Diagnostics screen claim a
/// backend that is not there.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GestureBackend {
    /// Whatever the session offers, re-evaluated at start. Today that resolves
    /// to nothing on a real desktop.
    #[default]
    Auto,
    /// The recording adapter. Test mode and every test run on this.
    Mock,
    /// Recognize nothing. This is what a disabled integration and safe mode
    /// select.
    None,
}

impl GestureBackend {
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Mock => "mock",
            Self::None => "none",
        }
    }
}

/// Whether a gesture can be animated as it happens.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnimationProgress {
    /// The gesture should follow the fingers where the adapter and the action
    /// both allow it, and fall back to discrete activation where they do not.
    #[default]
    WhenAvailable,
    /// Discrete activation only, even where progress is available.
    Never,
}

/// Whether a gesture collides with something the desktop already does.
///
/// A gesture is `Unknown` until conflict detection has run against it, which is
/// a different state from "no conflict" and is shown differently.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "conflict", rename_all = "snake_case")]
pub enum ConflictState {
    #[default]
    Unknown,
    Clear,
    Conflicts {
        /// The machine key of the built-in gesture it collides with.
        with: String,
        detail: String,
    },
}

impl ConflictState {
    pub fn conflicts(&self) -> bool {
        matches!(self, Self::Conflicts { .. })
    }
}

/// What the last verification of this gesture's binding said.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verification", rename_all = "snake_case")]
pub enum VerificationRecord {
    /// Never applied, so never verified. Not the same as failing.
    #[default]
    NotRun,
    Verified {
        continuous_progress: bool,
    },
    Failed {
        reason: String,
        detail: String,
    },
    Unsupported {
        reason: String,
        detail: String,
    },
}

/// A gesture identity. Kebab-case, so it can be a key in a file, a report, and
/// a log without quoting.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GestureId(String);

impl GestureId {
    pub fn new(id: impl Into<String>) -> Result<Self, GestureError> {
        let id = id.into();
        let well_formed = !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
            && !id.starts_with('-')
            && !id.ends_with('-');
        if well_formed {
            Ok(Self(id))
        } else {
            Err(GestureError::MalformedId(id))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for GestureId {
    type Error = GestureError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

impl From<GestureId> for String {
    fn from(id: GestureId) -> Self {
        id.0
    }
}

impl fmt::Display for GestureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One configured gesture, with every field Issue #3 requires.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GestureDefinition {
    pub id: GestureId,
    pub shape: GestureShape,
    pub contacts: ContactCount,
    /// Whether one of the contacts must be the thumb. This is what makes
    /// "thumb and three fingers" a different gesture from "four fingers",
    /// even though both are four contact points.
    pub thumb_required: bool,
    pub direction: Option<Direction>,
    pub action: DesktopAction,
    pub activation_threshold: Threshold,
    pub cancellation_threshold: Threshold,
    pub cooldown: Cooldown,
    pub enabled: bool,
    pub animation_progress: AnimationProgress,
    pub backend: GestureBackend,
    #[serde(default)]
    pub conflict: ConflictState,
    #[serde(default)]
    pub last_verification: VerificationRecord,
}

impl GestureDefinition {
    /// Builds a gesture with the recorded starting thresholds.
    ///
    /// The numbers are ADR 0012's recorded starting values, not a tuned curve.
    /// They are here rather than scattered through the preset so that changing
    /// them is one edit and one test.
    pub fn new(
        id: &str,
        shape: GestureShape,
        contacts: u8,
        thumb_required: bool,
        direction: Option<Direction>,
        action: DesktopAction,
    ) -> Result<Self, GestureError> {
        let definition = Self {
            id: GestureId::new(id)?,
            shape,
            contacts: ContactCount::new(contacts)?,
            thumb_required,
            direction,
            action,
            activation_threshold: Threshold::new(Self::DEFAULT_ACTIVATION)?,
            cancellation_threshold: Threshold::new(Self::DEFAULT_CANCELLATION)?,
            cooldown: Cooldown::from_millis(Self::DEFAULT_COOLDOWN_MS)?,
            enabled: true,
            animation_progress: AnimationProgress::WhenAvailable,
            backend: GestureBackend::Auto,
            conflict: ConflictState::Unknown,
            last_verification: VerificationRecord::NotRun,
        };
        definition.validate()?;
        Ok(definition)
    }

    /// Deliberately past halfway: a gesture that commits at the halfway point
    /// is one a hesitating hand triggers. This is the same number
    /// `launcher-platform` uses, so the two sides commit at the same place.
    pub const DEFAULT_ACTIVATION: f32 = 0.6;
    /// Far enough back that a small tremble does not cancel, near enough that a
    /// deliberate reversal does.
    pub const DEFAULT_CANCELLATION: f32 = 0.25;
    /// Also the launcher's number.
    pub const DEFAULT_COOLDOWN_MS: u64 = 350;

    /// Refuses a gesture that cannot mean anything.
    pub fn validate(&self) -> Result<(), GestureError> {
        match (self.shape.needs_direction(), self.direction) {
            (true, None) => return Err(GestureError::DirectionRequired(self.shape.key())),
            (_, Some(direction)) if !self.shape.allowed_directions().contains(&direction) => {
                return Err(GestureError::DirectionNotAllowed {
                    shape: self.shape.key(),
                    direction: direction.key(),
                });
            }
            _ => {}
        }
        if self.cancellation_threshold.get() >= self.activation_threshold.get() {
            return Err(GestureError::CancellationNotBelowActivation {
                activation: self.activation_threshold.get().to_string(),
                cancellation: self.cancellation_threshold.get().to_string(),
            });
        }
        Ok(())
    }

    /// Whether this gesture would ever animate: the action has to be able to
    /// follow progress, and the gesture has to be allowed to.
    pub fn can_animate(&self) -> bool {
        self.animation_progress == AnimationProgress::WhenAvailable
            && self.action.follows_progress()
    }

    /// A one-line summary for a plan preview, in machine keys.
    pub fn summary(&self) -> String {
        let direction = self
            .direction
            .map(|direction| format!(" {}", direction.key()))
            .unwrap_or_default();
        let thumb = if self.thumb_required { " +thumb" } else { "" };
        format!(
            "{}{direction} ×{}{thumb} → {}{}",
            self.shape.key(),
            self.contacts,
            self.action.key(),
            if self.enabled { "" } else { " (off)" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swipe() -> GestureDefinition {
        GestureDefinition::new(
            "overview",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )
        .unwrap()
    }

    #[test]
    fn a_swipe_without_a_direction_is_refused_because_it_could_not_be_recognized() {
        assert_eq!(
            GestureDefinition::new(
                "overview",
                GestureShape::Swipe,
                4,
                false,
                None,
                DesktopAction::ShowOverview,
            ),
            Err(GestureError::DirectionRequired("swipe"))
        );
    }

    #[test]
    fn a_direction_a_shape_cannot_carry_is_refused() {
        assert_eq!(
            GestureDefinition::new(
                "launcher",
                GestureShape::Pinch,
                4,
                true,
                Some(Direction::Up),
                DesktopAction::LauncherOpen,
            ),
            Err(GestureError::DirectionNotAllowed {
                shape: "pinch",
                direction: "up"
            })
        );
        assert_eq!(
            GestureDefinition::new(
                "spin",
                GestureShape::Rotate,
                2,
                false,
                Some(Direction::Left),
                DesktopAction::ApplicationRotate,
            ),
            Err(GestureError::DirectionNotAllowed {
                shape: "rotate",
                direction: "left"
            })
        );
    }

    #[test]
    fn a_contact_count_outside_what_a_hand_or_a_touchpad_has_is_refused() {
        assert_eq!(
            ContactCount::new(0),
            Err(GestureError::ContactsOutOfRange(0))
        );
        assert_eq!(
            ContactCount::new(6),
            Err(GestureError::ContactsOutOfRange(6))
        );
        for count in 1..=5 {
            assert_eq!(ContactCount::new(count).unwrap().get(), count);
        }
    }

    #[test]
    fn a_five_finger_gesture_is_a_legitimate_custom_mapping() {
        let five = GestureDefinition::new(
            "custom-five",
            GestureShape::Pinch,
            5,
            false,
            None,
            DesktopAction::LauncherOpen,
        )
        .unwrap();
        assert_eq!(five.contacts.get(), 5);
    }

    #[test]
    fn a_cancellation_threshold_at_or_above_activation_is_refused() {
        let mut gesture = swipe();
        gesture.cancellation_threshold = Threshold::new(0.6).unwrap();
        assert!(matches!(
            gesture.validate(),
            Err(GestureError::CancellationNotBelowActivation { .. })
        ));
        gesture.cancellation_threshold = Threshold::new(0.9).unwrap();
        assert!(gesture.validate().is_err());
    }

    #[test]
    fn a_threshold_outside_zero_to_one_never_exists() {
        assert!(Threshold::new(1.1).is_err());
        assert!(Threshold::new(-0.1).is_err());
        assert!(Threshold::new(f32::NAN).is_err());
        assert_eq!(Threshold::new(1.0).unwrap().get(), 1.0);
    }

    #[test]
    fn a_cooldown_long_enough_to_swallow_a_deliberate_gesture_is_refused() {
        assert!(Cooldown::from_millis(Cooldown::MAX_MS).is_ok());
        assert_eq!(
            Cooldown::from_millis(5_000),
            Err(GestureError::CooldownOutOfRange(5_000))
        );
        assert_eq!(Cooldown::from_millis(0).unwrap().as_millis(), 0);
    }

    #[test]
    fn an_identity_that_could_not_be_a_key_in_a_file_is_refused() {
        for bad in ["", "Launcher", "show desktop", "-lead", "trail-", "a/b"] {
            assert!(GestureId::new(bad).is_err(), "{bad} was accepted");
        }
        assert_eq!(
            GestureId::new("show-desktop").unwrap().as_str(),
            "show-desktop"
        );
    }

    #[test]
    fn a_definition_round_trips_through_json_with_every_field_intact() {
        let gesture = swipe();
        let text = serde_json::to_string(&gesture).unwrap();
        assert_eq!(
            serde_json::from_str::<GestureDefinition>(&text).unwrap(),
            gesture
        );
    }

    #[test]
    fn a_stored_definition_with_an_impossible_value_is_refused_not_clamped() {
        let text = serde_json::to_string(&swipe())
            .unwrap()
            .replace("\"contacts\":4", "\"contacts\":9");
        assert!(serde_json::from_str::<GestureDefinition>(&text).is_err());
    }

    #[test]
    fn animation_needs_both_the_action_and_the_gesture_to_allow_it() {
        let mut gesture = swipe();
        assert!(gesture.can_animate());
        gesture.animation_progress = AnimationProgress::Never;
        assert!(!gesture.can_animate());

        let mute = GestureDefinition::new(
            "mute",
            GestureShape::Tap,
            3,
            false,
            None,
            DesktopAction::VolumeMute,
        )
        .unwrap();
        assert!(!mute.can_animate());
    }

    #[test]
    fn a_conflict_state_starts_unknown_rather_than_clear() {
        assert_eq!(swipe().conflict, ConflictState::Unknown);
        assert!(!ConflictState::Unknown.conflicts());
        assert_eq!(swipe().last_verification, VerificationRecord::NotRun);
    }
}
