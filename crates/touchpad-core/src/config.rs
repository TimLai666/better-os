//! The versioned, migratable configuration.
//!
//! Two rules shape this file:
//!
//! - **A version is read before the content is.** A file written by a newer
//!   Better Touchpad is refused and left alone rather than parsed leniently and
//!   rewritten, the same rule `manager-store` follows for lifecycle state.
//! - **Linked axes are a configuration rule, not a screen rule.** Setting one
//!   scroll factor while the axes are linked sets both, here, so the CLI, the
//!   GUI, and a restored file cannot disagree about what "linked" meant.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::settings::{Section, SettingId, SettingValue};
use crate::value::{AccelerationProfile, ClickMethod, ScrollFactor, Sensitivity, ValueError};

/// The schema this build writes. Version 1 was the pre-Better-Touchpad sketch
/// with a single scroll factor; version 2 is the shipped phase-1 shape.
pub const CONFIG_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ConfigError {
    #[error("the configuration is not valid JSON: {0}")]
    NotJson(String),
    #[error("the configuration declares no schema version")]
    NoSchemaVersion,
    #[error(
        "schema version {found} was written by a newer Better Touchpad than this one ({known})"
    )]
    NewerSchema { found: u32, known: u32 },
    #[error("schema version {0} is not a version Better Touchpad ever wrote")]
    UnknownSchema(u32),
    #[error("the configuration is missing {0}")]
    Missing(&'static str),
    #[error(transparent)]
    Value(#[from] ValueError),
}

/// Which touchpad the configuration is about.
///
/// A device is named by the stable identity `touchpad-platform` derives, never
/// by an event node number, which changes when a device is re-plugged or when
/// the kernel enumerates in a different order.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "selection", rename_all = "snake_case")]
pub enum DeviceSelection {
    /// Whichever touchpad the platform picks, re-evaluated every start.
    #[default]
    Auto,
    /// One specific device by stable identity, with the global profile as the
    /// fallback when it is not connected.
    Device { identity: String },
}

/// Which backend applies changes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendSelection {
    #[default]
    Auto,
    Gnome,
    /// Read and show, change nothing. This is what safe mode selects.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerConfig {
    pub sensitivity: Sensitivity,
    pub acceleration_profile: AccelerationProfile,
    pub disable_while_typing: bool,
}

impl Default for PointerConfig {
    fn default() -> Self {
        Self {
            sensitivity: Sensitivity::neutral(),
            acceleration_profile: AccelerationProfile::Default,
            disable_while_typing: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollingConfig {
    pub vertical_factor: ScrollFactor,
    pub horizontal_factor: ScrollFactor,
    pub linked_axes: bool,
    pub natural: bool,
    pub two_finger: bool,
    pub smooth: bool,
}

impl Default for ScrollingConfig {
    fn default() -> Self {
        Self {
            vertical_factor: ScrollFactor::neutral(),
            horizontal_factor: ScrollFactor::neutral(),
            linked_axes: true,
            natural: true,
            two_finger: true,
            smooth: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClickingConfig {
    pub tap_to_click: bool,
    pub tap_and_drag: bool,
    pub drag_lock: bool,
    pub click_method: ClickMethod,
    pub middle_click_emulation: bool,
}

impl Default for ClickingConfig {
    fn default() -> Self {
        Self {
            tap_to_click: true,
            tap_and_drag: true,
            drag_lock: false,
            click_method: ClickMethod::Default,
            middle_click_emulation: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TouchpadConfig {
    pub schema_version: u32,
    pub enabled: bool,
    pub selected_device: DeviceSelection,
    pub backend: BackendSelection,
    pub pointer: PointerConfig,
    pub scrolling: ScrollingConfig,
    pub clicking: ClickingConfig,
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: true,
            selected_device: DeviceSelection::Auto,
            backend: BackendSelection::Auto,
            pointer: PointerConfig::default(),
            scrolling: ScrollingConfig::default(),
            clicking: ClickingConfig::default(),
        }
    }
}

impl TouchpadConfig {
    /// The configured value of one setting.
    pub fn value(&self, setting: SettingId) -> SettingValue {
        match setting {
            SettingId::PointerSensitivity => SettingValue::sensitivity(self.pointer.sensitivity),
            SettingId::AccelerationProfile => {
                SettingValue::acceleration(self.pointer.acceleration_profile)
            }
            SettingId::DisableWhileTyping => {
                SettingValue::toggle(self.pointer.disable_while_typing)
            }
            SettingId::VerticalScrollFactor => SettingValue::factor(self.scrolling.vertical_factor),
            SettingId::HorizontalScrollFactor => {
                SettingValue::factor(self.scrolling.horizontal_factor)
            }
            SettingId::NaturalScrolling => SettingValue::toggle(self.scrolling.natural),
            SettingId::TwoFingerScrolling => SettingValue::toggle(self.scrolling.two_finger),
            SettingId::SmoothScrolling => SettingValue::toggle(self.scrolling.smooth),
            SettingId::TapToClick => SettingValue::toggle(self.clicking.tap_to_click),
            SettingId::TapAndDrag => SettingValue::toggle(self.clicking.tap_and_drag),
            SettingId::DragLock => SettingValue::toggle(self.clicking.drag_lock),
            SettingId::ClickMethod => SettingValue::click(self.clicking.click_method),
            SettingId::MiddleClickEmulation => {
                SettingValue::toggle(self.clicking.middle_click_emulation)
            }
        }
    }

    /// Stores a value, refusing one of the wrong kind.
    ///
    /// While the axes are linked, writing either scroll factor writes both.
    /// That is the whole meaning of the linked-axis switch, and keeping it here
    /// means no caller can implement half of it.
    pub fn set(&mut self, setting: SettingId, value: SettingValue) -> Result<(), ValueError> {
        value.validate_for(setting)?;
        match (setting, value) {
            (SettingId::PointerSensitivity, SettingValue::Sensitivity { value }) => {
                self.pointer.sensitivity = value;
            }
            (SettingId::AccelerationProfile, SettingValue::Acceleration { value }) => {
                self.pointer.acceleration_profile = value;
            }
            (SettingId::DisableWhileTyping, SettingValue::Toggle { value }) => {
                self.pointer.disable_while_typing = value;
            }
            (SettingId::VerticalScrollFactor, SettingValue::Factor { value }) => {
                self.scrolling.vertical_factor = value;
                if self.scrolling.linked_axes {
                    self.scrolling.horizontal_factor = value;
                }
            }
            (SettingId::HorizontalScrollFactor, SettingValue::Factor { value }) => {
                self.scrolling.horizontal_factor = value;
                if self.scrolling.linked_axes {
                    self.scrolling.vertical_factor = value;
                }
            }
            (SettingId::NaturalScrolling, SettingValue::Toggle { value }) => {
                self.scrolling.natural = value;
            }
            (SettingId::TwoFingerScrolling, SettingValue::Toggle { value }) => {
                self.scrolling.two_finger = value;
            }
            (SettingId::SmoothScrolling, SettingValue::Toggle { value }) => {
                self.scrolling.smooth = value;
            }
            (SettingId::TapToClick, SettingValue::Toggle { value }) => {
                self.clicking.tap_to_click = value;
            }
            (SettingId::TapAndDrag, SettingValue::Toggle { value }) => {
                self.clicking.tap_and_drag = value;
            }
            (SettingId::DragLock, SettingValue::Toggle { value }) => {
                self.clicking.drag_lock = value;
            }
            (SettingId::ClickMethod, SettingValue::Click { value }) => {
                self.clicking.click_method = value;
            }
            (SettingId::MiddleClickEmulation, SettingValue::Toggle { value }) => {
                self.clicking.middle_click_emulation = value;
            }
            // `validate_for` already refused every mismatched pair.
            _ => unreachable!("value kind matched the setting but no arm did"),
        }
        Ok(())
    }

    /// Links or unlinks the scroll axes. Linking them adopts the vertical
    /// factor for both, because the vertical one is the value the user was
    /// looking at when they linked.
    pub fn set_linked_axes(&mut self, linked: bool) {
        self.scrolling.linked_axes = linked;
        if linked {
            self.scrolling.horizontal_factor = self.scrolling.vertical_factor;
        }
    }

    pub fn section_values(&self, section: Section) -> Vec<(SettingId, SettingValue)> {
        SettingId::in_section(section)
            .into_iter()
            .map(|setting| (setting, self.value(setting)))
            .collect()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a configuration always serializes")
    }

    /// Reads a configuration of any schema version this build knows, migrating
    /// it forward. A newer schema is an error rather than a best effort.
    pub fn from_json(text: &str) -> Result<Self, ConfigError> {
        let value: Value =
            serde_json::from_str(text).map_err(|error| ConfigError::NotJson(error.to_string()))?;
        Self::from_value(value)
    }

    pub fn from_value(value: Value) -> Result<Self, ConfigError> {
        let version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or(ConfigError::NoSchemaVersion)? as u32;
        match version {
            1 => migrate_v1(&value),
            2 => serde_json::from_value(value)
                .map_err(|error| ConfigError::NotJson(error.to_string())),
            version if version > CONFIG_SCHEMA_VERSION => Err(ConfigError::NewerSchema {
                found: version,
                known: CONFIG_SCHEMA_VERSION,
            }),
            version => Err(ConfigError::UnknownSchema(version)),
        }
    }
}

/// Version 1 held one scroll factor and no click detail. Migrating it means
/// giving both axes that factor and linking them, which is the behavior a
/// single factor described, and taking the shipped defaults for the controls
/// version 1 had no opinion about.
fn migrate_v1(value: &Value) -> Result<TouchpadConfig, ConfigError> {
    let mut config = TouchpadConfig {
        enabled: value
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or(ConfigError::Missing("enabled"))?,
        ..TouchpadConfig::default()
    };

    if let Some(selection) = value.get("selected_device").and_then(Value::as_str) {
        config.selected_device = match selection {
            "auto" => DeviceSelection::Auto,
            identity => DeviceSelection::Device {
                identity: identity.to_string(),
            },
        };
    }
    if let Some(backend) = value.get("backend").and_then(Value::as_str) {
        config.backend = match backend {
            "gnome" => BackendSelection::Gnome,
            "none" => BackendSelection::None,
            _ => BackendSelection::Auto,
        };
    }

    let pointer = value
        .get("pointer")
        .ok_or(ConfigError::Missing("pointer"))?;
    config.pointer.sensitivity = Sensitivity::new(
        pointer
            .get("sensitivity")
            .and_then(Value::as_f64)
            .ok_or(ConfigError::Missing("pointer.sensitivity"))?,
    )?;
    if let Some(profile) = pointer.get("acceleration_profile").and_then(Value::as_str) {
        config.pointer.acceleration_profile =
            AccelerationProfile::parse(profile).unwrap_or(AccelerationProfile::Default);
    }

    let scrolling = value
        .get("scrolling")
        .ok_or(ConfigError::Missing("scrolling"))?;
    let factor = ScrollFactor::new(
        scrolling
            .get("factor")
            .and_then(Value::as_f64)
            .ok_or(ConfigError::Missing("scrolling.factor"))?,
    )?;
    config.scrolling.vertical_factor = factor;
    config.scrolling.horizontal_factor = factor;
    config.scrolling.linked_axes = true;
    if let Some(natural) = scrolling.get("natural").and_then(Value::as_bool) {
        config.scrolling.natural = natural;
    }

    if let Some(clicking) = value.get("clicking") {
        if let Some(tap) = clicking.get("tap_to_click").and_then(Value::as_bool) {
            config.clicking.tap_to_click = tap;
        }
    }

    config.schema_version = CONFIG_SCHEMA_VERSION;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_configuration_round_trips_through_json() {
        let config = TouchpadConfig::default();
        let parsed = TouchpadConfig::from_json(&config.to_json()).unwrap();
        assert_eq!(parsed, config);
        assert_eq!(parsed.schema_version, CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn every_setting_reads_back_the_value_that_was_written() {
        let mut config = TouchpadConfig::default();
        config.scrolling.linked_axes = false;
        for setting in SettingId::ALL {
            let value = match setting.kind() {
                crate::settings::ValueKind::Sensitivity => {
                    SettingValue::sensitivity(Sensitivity::new(0.8).unwrap())
                }
                crate::settings::ValueKind::Factor => {
                    SettingValue::factor(ScrollFactor::new(2.0).unwrap())
                }
                crate::settings::ValueKind::Toggle => {
                    SettingValue::toggle(!config.value(setting).as_bool().unwrap())
                }
                crate::settings::ValueKind::Acceleration => {
                    SettingValue::acceleration(AccelerationProfile::Flat)
                }
                crate::settings::ValueKind::Click => SettingValue::click(ClickMethod::Areas),
            };
            config.set(setting, value).unwrap();
            assert_eq!(config.value(setting), value, "{setting} did not stick");
        }
    }

    #[test]
    fn linked_axes_move_together_and_unlinked_axes_do_not() {
        let mut config = TouchpadConfig::default();
        assert!(config.scrolling.linked_axes);
        config
            .set(
                SettingId::VerticalScrollFactor,
                SettingValue::factor(ScrollFactor::new(1.8).unwrap()),
            )
            .unwrap();
        assert_eq!(config.scrolling.horizontal_factor.get(), 1.8);

        config
            .set(
                SettingId::HorizontalScrollFactor,
                SettingValue::factor(ScrollFactor::new(0.6).unwrap()),
            )
            .unwrap();
        assert_eq!(config.scrolling.vertical_factor.get(), 0.6);

        config.set_linked_axes(false);
        config
            .set(
                SettingId::HorizontalScrollFactor,
                SettingValue::factor(ScrollFactor::new(2.5).unwrap()),
            )
            .unwrap();
        assert_eq!(config.scrolling.vertical_factor.get(), 0.6);
        assert_eq!(config.scrolling.horizontal_factor.get(), 2.5);
    }

    #[test]
    fn linking_the_axes_adopts_the_vertical_factor_for_both() {
        let mut config = TouchpadConfig::default();
        config.set_linked_axes(false);
        config
            .set(
                SettingId::VerticalScrollFactor,
                SettingValue::factor(ScrollFactor::new(1.5).unwrap()),
            )
            .unwrap();
        config
            .set(
                SettingId::HorizontalScrollFactor,
                SettingValue::factor(ScrollFactor::new(0.4).unwrap()),
            )
            .unwrap();
        config.set_linked_axes(true);
        assert_eq!(config.scrolling.horizontal_factor.get(), 1.5);
    }

    #[test]
    fn a_value_of_the_wrong_kind_never_reaches_the_configuration() {
        let mut config = TouchpadConfig::default();
        let before = config.clone();
        assert!(
            config
                .set(
                    SettingId::TapToClick,
                    SettingValue::click(ClickMethod::Areas)
                )
                .is_err()
        );
        assert_eq!(config, before);
    }

    const V1: &str = r#"{
        "schema_version": 1,
        "enabled": true,
        "selected_device": "auto",
        "backend": "gnome",
        "pointer": { "sensitivity": 0.55, "acceleration_profile": "adaptive" },
        "scrolling": { "factor": 0.65, "natural": true },
        "clicking": { "tap_to_click": false }
    }"#;

    #[test]
    fn a_version_one_file_migrates_to_the_shipped_schema() {
        let config = TouchpadConfig::from_json(V1).unwrap();
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.pointer.sensitivity.get(), 0.55);
        assert_eq!(
            config.pointer.acceleration_profile,
            AccelerationProfile::Adaptive
        );
        // One factor became two linked ones, which is what one factor meant.
        assert_eq!(config.scrolling.vertical_factor.get(), 0.65);
        assert_eq!(config.scrolling.horizontal_factor.get(), 0.65);
        assert!(config.scrolling.linked_axes);
        assert!(config.scrolling.natural);
        assert!(!config.clicking.tap_to_click);
        // Controls version 1 had no opinion about take the shipped defaults.
        assert_eq!(config.clicking.click_method, ClickMethod::Default);
        assert!(config.pointer.disable_while_typing);
        assert_eq!(config.backend, BackendSelection::Gnome);
    }

    #[test]
    fn a_migrated_file_survives_being_written_and_read_again() {
        let migrated = TouchpadConfig::from_json(V1).unwrap();
        let round_tripped = TouchpadConfig::from_json(&migrated.to_json()).unwrap();
        assert_eq!(round_tripped, migrated);
    }

    #[test]
    fn a_version_one_file_with_an_impossible_value_is_refused_not_clamped() {
        let text = V1.replace("0.55", "9.0");
        assert!(matches!(
            TouchpadConfig::from_json(&text),
            Err(ConfigError::Value(ValueError::OutOfRange { .. }))
        ));
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_read_leniently() {
        let text = r#"{"schema_version": 99, "enabled": true}"#;
        assert_eq!(
            TouchpadConfig::from_json(text),
            Err(ConfigError::NewerSchema {
                found: 99,
                known: CONFIG_SCHEMA_VERSION
            })
        );
    }

    #[test]
    fn a_file_with_no_version_is_refused() {
        assert_eq!(
            TouchpadConfig::from_json(r#"{"enabled": true}"#),
            Err(ConfigError::NoSchemaVersion)
        );
        assert!(matches!(
            TouchpadConfig::from_json("not json"),
            Err(ConfigError::NotJson(_))
        ));
    }

    #[test]
    fn a_version_zero_file_is_refused_as_a_version_nothing_ever_wrote() {
        assert_eq!(
            TouchpadConfig::from_json(r#"{"schema_version": 0}"#),
            Err(ConfigError::UnknownSchema(0))
        );
    }
}
