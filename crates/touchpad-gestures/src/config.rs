//! The gesture half of the configuration: versioned, migratable, and kept in
//! its own file.
//!
//! It is separate from `touchpad-core`'s pointer, scrolling, and clicking
//! configuration on purpose, and that separation is a safety property rather
//! than tidiness. A gesture adapter that fails, or a gesture configuration that
//! will not parse, must not be able to take pointer movement or two-finger
//! scrolling down with it. Different file, different capture, different restore
//! — so the worst a broken gesture configuration can do is leave the machine
//! with no gestures.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::definition::{GestureDefinition, GestureError, GestureId};

/// The schema this build writes. There has never been an older one; the version
/// is here so that the file that follows this one can be read by the build that
/// wrote this one, or refused with a reason.
pub const GESTURE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ConfigError {
    #[error("gestures.config.not_json:{0}")]
    NotJson(String),
    #[error("gestures.config.no_schema_version")]
    NoSchemaVersion,
    #[error("gestures.config.newer_schema:{found}:{known}")]
    NewerSchema { found: u32, known: u32 },
    #[error("gestures.config.unknown_schema:{0}")]
    UnknownSchema(u32),
    #[error(transparent)]
    Gesture(#[from] GestureError),
}

/// Which shipped preset a configuration came from, if any.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresetId {
    /// No preset: either the shipped empty configuration or one the user built.
    #[default]
    Custom,
    /// Issue #3's Mac-style gestures.
    MacStyle,
}

impl PresetId {
    pub fn key(self) -> &'static str {
        match self {
            Self::Custom => "custom",
            Self::MacStyle => "mac-style",
        }
    }
}

/// Every configured gesture, plus whether gestures are on at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GestureConfig {
    pub schema_version: u32,
    /// The master switch. Turning it off is what "disable gestures" means, and
    /// it restores the captured configuration rather than deleting it.
    pub enabled: bool,
    pub preset: PresetId,
    pub gestures: Vec<GestureDefinition>,
}

impl Default for GestureConfig {
    /// The shipped configuration before a preset is applied: gestures are on as
    /// a subsystem and nothing is bound. Better Touchpad therefore changes no
    /// gesture until somebody asks it to, which is what Issue #3 requires of
    /// installation.
    fn default() -> Self {
        Self {
            schema_version: GESTURE_SCHEMA_VERSION,
            enabled: true,
            preset: PresetId::Custom,
            gestures: Vec::new(),
        }
    }
}

impl GestureConfig {
    pub fn with_gestures(gestures: Vec<GestureDefinition>, preset: PresetId) -> Self {
        Self {
            preset,
            gestures,
            ..Self::default()
        }
    }

    pub fn get(&self, id: &GestureId) -> Option<&GestureDefinition> {
        self.gestures.iter().find(|gesture| &gesture.id == id)
    }

    pub fn get_mut(&mut self, id: &GestureId) -> Option<&mut GestureDefinition> {
        self.gestures.iter_mut().find(|gesture| &gesture.id == id)
    }

    /// The gestures a recognizer should be listening for.
    pub fn active(&self) -> Vec<GestureDefinition> {
        if !self.enabled {
            return Vec::new();
        }
        self.gestures
            .iter()
            .filter(|gesture| gesture.enabled)
            .cloned()
            .collect()
    }

    /// Refuses a configuration that could not be recognized: a duplicate
    /// identity, or a gesture that does not validate on its own.
    pub fn validate(&self) -> Result<(), GestureError> {
        let mut seen: Vec<&GestureId> = Vec::new();
        for gesture in &self.gestures {
            gesture.validate()?;
            if seen.contains(&&gesture.id) {
                return Err(GestureError::DuplicateId(gesture.id.to_string()));
            }
            seen.push(&gesture.id);
        }
        Ok(())
    }

    /// Replaces one gesture, refusing a change that would break the
    /// configuration. The previous definition is put back on refusal, so a
    /// rejected edit leaves nothing half-applied.
    pub fn replace(&mut self, gesture: GestureDefinition) -> Result<(), GestureError> {
        let Some(slot) = self
            .gestures
            .iter()
            .position(|existing| existing.id == gesture.id)
        else {
            return Err(GestureError::UnknownId(gesture.id.to_string()));
        };
        let previous = std::mem::replace(&mut self.gestures[slot], gesture);
        match self.validate() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.gestures[slot] = previous;
                Err(error)
            }
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a gesture configuration always serializes")
    }

    pub fn from_json(text: &str) -> Result<Self, ConfigError> {
        let value: Value =
            serde_json::from_str(text).map_err(|error| ConfigError::NotJson(error.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or(ConfigError::NoSchemaVersion)? as u32;
        match version {
            GESTURE_SCHEMA_VERSION => {
                let config: Self = serde_json::from_value(value)
                    .map_err(|error| ConfigError::NotJson(error.to_string()))?;
                config.validate()?;
                Ok(config)
            }
            found if found > GESTURE_SCHEMA_VERSION => Err(ConfigError::NewerSchema {
                found,
                known: GESTURE_SCHEMA_VERSION,
            }),
            found => Err(ConfigError::UnknownSchema(found)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{Direction, GestureShape};
    use better_actions::DesktopAction;

    fn gesture(id: &str) -> GestureDefinition {
        GestureDefinition::new(
            id,
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )
        .unwrap()
    }

    #[test]
    fn the_shipped_configuration_binds_nothing_so_installing_changes_no_gesture() {
        let config = GestureConfig::default();
        assert!(config.gestures.is_empty());
        assert_eq!(config.preset, PresetId::Custom);
        assert!(config.enabled);
        assert!(config.active().is_empty());
    }

    #[test]
    fn a_configuration_round_trips_through_json() {
        let config = GestureConfig::with_gestures(vec![gesture("overview")], PresetId::MacStyle);
        assert_eq!(GestureConfig::from_json(&config.to_json()).unwrap(), config);
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_read_leniently() {
        assert_eq!(
            GestureConfig::from_json(r#"{"schema_version":99,"enabled":true}"#),
            Err(ConfigError::NewerSchema {
                found: 99,
                known: GESTURE_SCHEMA_VERSION
            })
        );
        assert_eq!(
            GestureConfig::from_json(r#"{"enabled":true}"#),
            Err(ConfigError::NoSchemaVersion)
        );
        assert_eq!(
            GestureConfig::from_json(r#"{"schema_version":0}"#),
            Err(ConfigError::UnknownSchema(0))
        );
    }

    #[test]
    fn two_gestures_with_the_same_identity_are_refused() {
        let config = GestureConfig::with_gestures(
            vec![gesture("overview"), gesture("overview")],
            PresetId::Custom,
        );
        assert!(matches!(
            config.validate(),
            Err(GestureError::DuplicateId(_))
        ));
        assert!(GestureConfig::from_json(&config.to_json()).is_err());
    }

    #[test]
    fn turning_gestures_off_stops_the_recognizer_without_forgetting_the_bindings() {
        let mut config =
            GestureConfig::with_gestures(vec![gesture("overview")], PresetId::MacStyle);
        assert_eq!(config.active().len(), 1);
        config.enabled = false;
        assert!(config.active().is_empty());
        // The definition is still there to turn back on.
        assert_eq!(config.gestures.len(), 1);
    }

    #[test]
    fn a_disabled_gesture_is_kept_and_not_recognized() {
        let mut config = GestureConfig::with_gestures(vec![gesture("overview")], PresetId::Custom);
        config.gestures[0].enabled = false;
        assert!(config.active().is_empty());
        assert_eq!(config.gestures.len(), 1);
    }

    #[test]
    fn a_rejected_edit_leaves_the_configuration_exactly_as_it_was() {
        let mut config = GestureConfig::with_gestures(
            vec![gesture("overview"), gesture("windows")],
            PresetId::Custom,
        );
        let before = config.clone();
        let mut broken = gesture("windows");
        broken.direction = None;
        assert!(config.replace(broken).is_err());
        assert_eq!(config, before);
    }

    #[test]
    fn replacing_a_gesture_that_is_not_there_is_an_error_rather_than_an_insert() {
        let mut config = GestureConfig::default();
        assert!(matches!(
            config.replace(gesture("overview")),
            Err(GestureError::UnknownId(_))
        ));
        assert!(config.gestures.is_empty());
    }
}
