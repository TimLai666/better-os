//! Bounded values, and the refusals that keep an impossible one out.
//!
//! Every number a user can move lives behind a newtype whose constructor is the
//! only way to build it. A value outside its supported range is rejected and
//! named; it is never clamped into something the user did not ask for. Silent
//! clamping is how a touchpad ends up at a setting nobody chose, and how a
//! restore puts back a value that was never captured.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why a value was refused. Every variant names the setting and the bound, so
/// the refusal can be shown without the caller re-deriving it.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum ValueError {
    #[error("{setting} must be between {min} and {max}, not {value}")]
    OutOfRange {
        setting: &'static str,
        value: f64,
        min: f64,
        max: f64,
    },
    #[error("{setting} must be a real number")]
    NotFinite { setting: &'static str },
    #[error("{setting} takes a {expected} value, not a {found} one")]
    WrongKind {
        setting: &'static str,
        expected: &'static str,
        found: &'static str,
    },
}

/// Pointer movement sensitivity on the Better OS scale.
///
/// The scale is `0.0` (slowest the backend offers) to `1.0` (fastest), with
/// `0.5` meaning "leave the pointer where the session had it". It is a Better
/// OS scale on purpose: a backend range is a backend detail, and
/// `docs/touchpad-sensitivity-mapping.md` records how it maps onto GNOME's
/// `-1.0..=1.0` speed and what that mapping cannot promise.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sensitivity(f64);

impl Sensitivity {
    pub const MIN: f64 = 0.0;
    pub const MAX: f64 = 1.0;
    pub const NAME: &'static str = "sensitivity";

    pub fn new(value: f64) -> Result<Self, ValueError> {
        check(Self::NAME, value, Self::MIN, Self::MAX).map(Self)
    }

    /// The neutral position: whatever the session already does.
    pub fn neutral() -> Self {
        Self(0.5)
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// A scroll sensitivity multiplier.
///
/// `1.0` is the session's own scroll distance. The bounds are a starting point
/// recorded in ADR 0010, not a settled curve; they exist so a value that would
/// make the desktop unusable cannot be stored.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScrollFactor(f64);

impl ScrollFactor {
    pub const MIN: f64 = 0.2;
    pub const MAX: f64 = 5.0;
    pub const NAME: &'static str = "scroll factor";

    pub fn new(value: f64) -> Result<Self, ValueError> {
        check(Self::NAME, value, Self::MIN, Self::MAX).map(Self)
    }

    pub fn neutral() -> Self {
        Self(1.0)
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

fn check(setting: &'static str, value: f64, min: f64, max: f64) -> Result<f64, ValueError> {
    if !value.is_finite() {
        return Err(ValueError::NotFinite { setting });
    }
    if value < min || value > max {
        return Err(ValueError::OutOfRange {
            setting,
            value,
            min,
            max,
        });
    }
    Ok(value)
}

/// How the backend turns finger movement into pointer movement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccelerationProfile {
    /// Leave the profile the session chose.
    Default,
    /// Speed-dependent acceleration.
    Adaptive,
    /// No acceleration at all.
    Flat,
}

impl AccelerationProfile {
    pub const ALL: [Self; 3] = [Self::Default, Self::Adaptive, Self::Flat];

    pub fn key(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Adaptive => "adaptive",
            Self::Flat => "flat",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == value)
    }
}

/// How a physical click is decided when several methods exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClickMethod {
    /// Leave the method the session chose.
    Default,
    /// Where on the pad the click happened decides the button.
    Areas,
    /// How many fingers are down decides the button.
    Fingers,
    /// No physical click button at all.
    None,
}

impl ClickMethod {
    pub const ALL: [Self; 4] = [Self::Default, Self::Areas, Self::Fingers, Self::None];

    pub fn key(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Areas => "areas",
            Self::Fingers => "fingers",
            Self::None => "none",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sensitivity_inside_the_range_is_kept_exactly() {
        assert_eq!(Sensitivity::new(0.55).unwrap().get(), 0.55);
        assert_eq!(Sensitivity::new(0.0).unwrap().get(), 0.0);
        assert_eq!(Sensitivity::new(1.0).unwrap().get(), 1.0);
    }

    #[test]
    fn a_sensitivity_outside_the_range_is_refused_rather_than_clamped() {
        let error = Sensitivity::new(1.4).unwrap_err();
        assert_eq!(
            error,
            ValueError::OutOfRange {
                setting: Sensitivity::NAME,
                value: 1.4,
                min: 0.0,
                max: 1.0,
            }
        );
        assert!(Sensitivity::new(-0.001).is_err());
    }

    #[test]
    fn a_value_that_is_not_a_number_is_refused() {
        assert_eq!(
            Sensitivity::new(f64::NAN).unwrap_err(),
            ValueError::NotFinite {
                setting: Sensitivity::NAME
            }
        );
        assert!(ScrollFactor::new(f64::INFINITY).is_err());
        assert!(ScrollFactor::new(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn scroll_factor_bounds_hold_at_both_ends() {
        assert!(ScrollFactor::new(ScrollFactor::MIN).is_ok());
        assert!(ScrollFactor::new(ScrollFactor::MAX).is_ok());
        assert!(ScrollFactor::new(ScrollFactor::MIN - 0.01).is_err());
        assert!(ScrollFactor::new(ScrollFactor::MAX + 0.01).is_err());
    }

    #[test]
    fn the_neutral_values_are_inside_their_own_ranges() {
        assert_eq!(Sensitivity::neutral().get(), 0.5);
        assert_eq!(ScrollFactor::neutral().get(), 1.0);
    }

    #[test]
    fn profile_and_method_keys_round_trip() {
        for profile in AccelerationProfile::ALL {
            assert_eq!(AccelerationProfile::parse(profile.key()), Some(profile));
        }
        for method in ClickMethod::ALL {
            assert_eq!(ClickMethod::parse(method.key()), Some(method));
        }
        assert_eq!(AccelerationProfile::parse("turbo"), None);
        assert_eq!(ClickMethod::parse("elbow"), None);
    }
}
