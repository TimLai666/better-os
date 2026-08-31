//! Per-device gesture profiles, and the document they are exported as.
//!
//! ADR 0010 shipped one global profile because GNOME's touchpad *settings*
//! schema is per-session: there is one `speed` key however many pads are
//! attached, so a per-device pointer configuration would be a Better OS
//! structure the backend cannot honour. Gestures are not that. A gesture is
//! recognized from contact frames this application reads itself, so which pad
//! produced them is knowable, and a gesture profile per pad is a thing Better
//! OS can actually keep rather than a promise about a key it cannot write.
//!
//! So this is the shape stored in `gestures.json` from schema version 2 on:
//! one global profile, zero or more profiles keyed by the stable device
//! identity `touchpad-platform` derives, and the identity currently selected.
//! A device with no profile of its own resolves to the global one — the
//! fallback is what makes attaching an unfamiliar pad do something sensible
//! rather than nothing.
//!
//! The same document is what export writes and import reads. That is
//! deliberate: an exported file is the configuration schema, not a second
//! format that has to be kept in step with it, so a file from another machine
//! goes through exactly the validation and the version migration a local file
//! does. An imported document is untrusted — every bound value is re-checked,
//! every identity is re-checked, and the sizes are bounded, because "it came
//! from a file" is the whole threat model here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{ConfigError, GestureConfig};

/// The schema `gestures.json` now carries.
///
/// Version 1 was a bare [`GestureConfig`] — one global profile written straight
/// to the file. Version 2 is this document. The version line is the file's, not
/// a profile's: a profile body inside a version 2 document still declares
/// version 1, because that is the schema a profile body is.
pub const PROFILE_SCHEMA_VERSION: u32 = 2;

/// The most device profiles one document may carry.
///
/// A machine with more than this many touchpads it has ever configured is not a
/// machine; it is a file trying to make the reader allocate. The bound is here
/// rather than in a comment because import treats the file as hostile.
pub const MAX_DEVICE_PROFILES: usize = 32;

/// The most gestures one profile may carry. The shipped preset has ten.
pub const MAX_GESTURES_PER_PROFILE: usize = 64;

/// The longest a device identity may be. `touchpad-platform` builds identities
/// from a `Uniq`, or from bus, vendor, product, version and name, and the name
/// is the only unbounded part.
pub const MAX_IDENTITY_LENGTH: usize = 160;

/// Every gesture profile on this machine, plus which one is in force.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GestureProfiles {
    pub schema_version: u32,
    /// The device whose profile is in force, or `None` for the global one.
    #[serde(default)]
    pub active_device: Option<String>,
    /// The fallback. Every device with no profile of its own uses this.
    pub global: GestureConfig,
    #[serde(default)]
    pub devices: BTreeMap<String, GestureConfig>,
}

impl Default for GestureProfiles {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            active_device: None,
            global: GestureConfig::default(),
            devices: BTreeMap::new(),
        }
    }
}

impl GestureProfiles {
    /// A document holding one profile as the global one, which is what a
    /// version 1 file was and what a single-profile export is.
    pub fn global_only(config: GestureConfig) -> Self {
        Self {
            global: config,
            ..Self::default()
        }
    }

    /// The profile in force for `device`: its own if it has one, the global one
    /// otherwise. The fallback is the whole reason a new pad works at all.
    pub fn resolve(&self, device: Option<&str>) -> &GestureConfig {
        device
            .and_then(|identity| self.devices.get(identity))
            .unwrap_or(&self.global)
    }

    /// The profile in force, mutable — the device's own if it has one, the
    /// global one otherwise.
    ///
    /// This deliberately does **not** create a device profile. Editing a pad
    /// that is following the global profile edits the global profile, which is
    /// what "following" means; giving a pad a profile of its own is a separate
    /// act, [`Self::detach`].
    pub fn resolve_mut(&mut self, device: Option<&str>) -> &mut GestureConfig {
        match device.filter(|identity| self.devices.contains_key(*identity)) {
            Some(identity) => self
                .devices
                .get_mut(identity)
                .expect("the identity was just found"),
            None => &mut self.global,
        }
    }

    /// Gives a device a profile of its own, copied from the global one.
    ///
    /// Starting it as a copy rather than as an empty profile is the behaviour
    /// that matches the fallback: until this moment the device *was* using the
    /// global profile, so that is what it now diverges from.
    pub fn detach(&mut self, device: &str) -> &mut GestureConfig {
        self.profile_mut(Some(device))
    }

    /// The profile for a device, creating one from the global profile if it has
    /// none. Used where a device profile is being written on purpose.
    pub fn profile_mut(&mut self, device: Option<&str>) -> &mut GestureConfig {
        match device {
            Some(identity) => {
                let global = self.global.clone();
                self.devices.entry(identity.to_string()).or_insert(global)
            }
            None => &mut self.global,
        }
    }

    /// Whether this device has diverged from the global profile.
    pub fn has_profile(&self, device: &str) -> bool {
        self.devices.contains_key(device)
    }

    /// Drops a device's own profile, so it follows the global one again.
    pub fn forget(&mut self, device: &str) -> bool {
        self.devices.remove(device).is_some()
    }

    pub fn identities(&self) -> impl Iterator<Item = &str> {
        self.devices.keys().map(String::as_str)
    }

    /// Refuses a document that could not be used.
    ///
    /// Every check here is one an imported file has to survive: the profiles
    /// themselves validate, no identity is a shape that could not have come
    /// from the kernel, the sizes are bounded, and the selected device is one
    /// the document actually holds.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.devices.len() > MAX_DEVICE_PROFILES {
            return Err(ConfigError::TooManyDevices(self.devices.len()));
        }
        self.check_profile(&self.global)?;
        for (identity, profile) in &self.devices {
            check_identity(identity)?;
            self.check_profile(profile)?;
        }
        if let Some(active) = &self.active_device {
            check_identity(active)?;
            // A selection naming a profile that is not here would silently fall
            // back to the global one, which is a different configuration from
            // the one the file claims to describe.
            if !self.devices.contains_key(active) {
                return Err(ConfigError::UnknownActiveDevice(active.clone()));
            }
        }
        Ok(())
    }

    fn check_profile(&self, profile: &GestureConfig) -> Result<(), ConfigError> {
        if profile.gestures.len() > MAX_GESTURES_PER_PROFILE {
            return Err(ConfigError::TooManyGestures(profile.gestures.len()));
        }
        profile.validate()?;
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a profile document always serializes")
    }

    /// Reads a document of any schema version this build knows, migrating it
    /// forward, and refuses everything else. This is the only way a file
    /// becomes profiles — there is no lenient path for a local file and a
    /// strict one for an imported file, because that is how the two drift.
    pub fn from_json(text: &str) -> Result<Self, ConfigError> {
        let value: Value =
            serde_json::from_str(text).map_err(|error| ConfigError::NotJson(error.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or(ConfigError::NoSchemaVersion)? as u32;
        match version {
            // A version 1 file is a single global profile, written before there
            // was anything else to write.
            1 => {
                let config: GestureConfig = serde_json::from_value(value)
                    .map_err(|error| ConfigError::NotJson(error.to_string()))?;
                config.validate()?;
                let profiles = Self::global_only(config);
                profiles.validate()?;
                Ok(profiles)
            }
            PROFILE_SCHEMA_VERSION => {
                let profiles: Self = serde_json::from_value(value)
                    .map_err(|error| ConfigError::NotJson(error.to_string()))?;
                profiles.validate()?;
                Ok(profiles)
            }
            found if found > PROFILE_SCHEMA_VERSION => Err(ConfigError::NewerSchema {
                found,
                known: PROFILE_SCHEMA_VERSION,
            }),
            found => Err(ConfigError::UnknownSchema(found)),
        }
    }
}

/// A device identity that could have come from `touchpad-platform`.
///
/// The rule is a positive one rather than a list of forbidden characters:
/// `touchpad-platform` builds exactly two shapes, `uniq:<id>` and
/// `input:<bus>:<vendor>:<product>:<version>:<name>`, so anything without one
/// of those prefixes is not an identity this build could have written. That is
/// what keeps `../../etc/passwd` out while letting `SynPS/2 Synaptics TouchPad`
/// — a real kernel device name, slash and all — through. Control characters are
/// refused separately, because an identity is also a label on a screen.
fn check_identity(identity: &str) -> Result<(), ConfigError> {
    let named = identity
        .strip_prefix("uniq:")
        .or_else(|| identity.strip_prefix("input:"));
    let usable = matches!(named, Some(rest) if !rest.is_empty())
        && identity.len() <= MAX_IDENTITY_LENGTH
        && !identity.chars().any(char::is_control);
    if usable {
        Ok(())
    } else {
        Err(ConfigError::MalformedDeviceIdentity(identity.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PresetId;
    use crate::definition::{Direction, GestureDefinition, GestureShape};
    use crate::preset::mac_style;
    use better_actions::DesktopAction;

    const PAD: &str = "uniq:LEN-0001";
    const OTHER: &str = "input:0003:06cb:ce67:0100:SynPS/2 Synaptics TouchPad";

    fn gesture(id: &str, contacts: u8) -> GestureDefinition {
        GestureDefinition::new(
            id,
            GestureShape::Swipe,
            contacts,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )
        .unwrap()
    }

    fn profiles() -> GestureProfiles {
        let mut profiles = GestureProfiles::global_only(GestureConfig::with_gestures(
            vec![gesture("overview", 4)],
            PresetId::Custom,
        ));
        profiles.devices.insert(
            PAD.to_string(),
            GestureConfig::with_gestures(vec![gesture("overview", 5)], PresetId::Custom),
        );
        profiles
    }

    #[test]
    fn a_device_with_no_profile_of_its_own_falls_back_to_the_global_one() {
        let profiles = profiles();
        assert_eq!(profiles.resolve(Some(PAD)).gestures[0].contacts.get(), 5);
        assert_eq!(profiles.resolve(Some(OTHER)).gestures[0].contacts.get(), 4);
        assert_eq!(profiles.resolve(None).gestures[0].contacts.get(), 4);
        assert!(!profiles.has_profile(OTHER));
    }

    #[test]
    fn editing_a_device_that_follows_the_global_profile_edits_the_global_profile() {
        // Following is not a private copy. A pad with no profile of its own is
        // using the global one, so editing it is editing that.
        let mut profiles = profiles();
        profiles.resolve_mut(Some(OTHER)).enabled = false;
        assert!(!profiles.global.enabled);
        assert!(!profiles.has_profile(OTHER));
        // The pad that does have its own profile is untouched.
        assert!(profiles.resolve(Some(PAD)).enabled);
    }

    #[test]
    fn giving_a_device_its_own_profile_starts_from_what_it_was_already_using() {
        let mut profiles = profiles();
        let profile = profiles.detach(OTHER);
        assert_eq!(
            profile.gestures[0].contacts.get(),
            4,
            "not a copy of global"
        );
        profile.enabled = false;

        assert!(profiles.has_profile(OTHER));
        // And the global profile is not what was edited.
        assert!(profiles.global.enabled);
        assert!(profiles.resolve(Some(PAD)).enabled);
    }

    #[test]
    fn forgetting_a_device_profile_puts_it_back_on_the_global_one() {
        let mut profiles = profiles();
        assert!(profiles.forget(PAD));
        assert!(!profiles.forget(PAD));
        assert_eq!(profiles.resolve(Some(PAD)).gestures[0].contacts.get(), 4);
    }

    #[test]
    fn a_document_round_trips_through_json_byte_for_byte() {
        let mut profiles = profiles();
        profiles.active_device = Some(PAD.to_string());
        let text = profiles.to_json();
        let parsed = GestureProfiles::from_json(&text).unwrap();
        assert_eq!(parsed, profiles);
        assert_eq!(parsed.to_json(), text, "a re-export is not the same bytes");
    }

    #[test]
    fn a_version_one_file_becomes_the_global_profile_of_a_version_two_document() {
        let v1 = mac_style().to_json();
        assert!(v1.contains("\"schema_version\": 1"));

        let profiles = GestureProfiles::from_json(&v1).unwrap();
        assert_eq!(profiles.schema_version, PROFILE_SCHEMA_VERSION);
        assert_eq!(profiles.global, mac_style());
        assert!(profiles.devices.is_empty());
        assert_eq!(profiles.active_device, None);
        // And the migrated document survives being written and read again.
        assert_eq!(
            GestureProfiles::from_json(&profiles.to_json()).unwrap(),
            profiles
        );
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_read_leniently() {
        assert_eq!(
            GestureProfiles::from_json(r#"{"schema_version":99}"#),
            Err(ConfigError::NewerSchema {
                found: 99,
                known: PROFILE_SCHEMA_VERSION
            })
        );
        assert_eq!(
            GestureProfiles::from_json(r#"{"schema_version":0}"#),
            Err(ConfigError::UnknownSchema(0))
        );
        assert_eq!(
            GestureProfiles::from_json(r#"{"global":{}}"#),
            Err(ConfigError::NoSchemaVersion)
        );
        assert!(matches!(
            GestureProfiles::from_json("{ not json"),
            Err(ConfigError::NotJson(_))
        ));
    }

    #[test]
    fn a_selected_device_the_document_does_not_hold_is_refused() {
        let mut profiles = profiles();
        profiles.active_device = Some(OTHER.to_string());
        assert_eq!(
            GestureProfiles::from_json(&profiles.to_json()),
            Err(ConfigError::UnknownActiveDevice(OTHER.to_string()))
        );
    }

    #[test]
    fn an_identity_that_could_not_have_come_from_the_kernel_is_refused() {
        // A real kernel device name carries a slash, so the rule cannot be a
        // list of forbidden characters.
        assert!(check_identity(OTHER).is_ok());
        assert!(check_identity(PAD).is_ok());

        for identity in [
            "",
            "uniq:",
            "input:",
            "../../etc/passwd",
            "uniq:\u{0}LEN",
            "input:pad\nname",
            "back\\slash",
        ] {
            let mut profiles = GestureProfiles::default();
            profiles
                .devices
                .insert(identity.to_string(), GestureConfig::default());
            assert!(
                matches!(
                    GestureProfiles::from_json(&profiles.to_json()),
                    Err(ConfigError::MalformedDeviceIdentity(_))
                ),
                "{identity:?} was accepted as a device identity"
            );
        }
        let long = format!("uniq:{}", "u".repeat(MAX_IDENTITY_LENGTH));
        let mut profiles = GestureProfiles::default();
        profiles.devices.insert(long, GestureConfig::default());
        assert!(GestureProfiles::from_json(&profiles.to_json()).is_err());
    }

    #[test]
    fn a_document_larger_than_a_machine_could_have_produced_is_refused() {
        let mut profiles = GestureProfiles::default();
        for index in 0..=MAX_DEVICE_PROFILES {
            profiles
                .devices
                .insert(format!("uniq:pad-{index}"), GestureConfig::default());
        }
        assert_eq!(
            GestureProfiles::from_json(&profiles.to_json()),
            Err(ConfigError::TooManyDevices(MAX_DEVICE_PROFILES + 1))
        );

        let mut profiles = GestureProfiles::default();
        profiles.global.gestures = (0..=MAX_GESTURES_PER_PROFILE)
            .map(|index| gesture(&format!("g-{index}"), 4))
            .collect();
        assert_eq!(
            GestureProfiles::from_json(&profiles.to_json()),
            Err(ConfigError::TooManyGestures(MAX_GESTURES_PER_PROFILE + 1))
        );
    }

    #[test]
    fn nothing_in_an_imported_document_can_carry_a_command_or_an_impossible_value() {
        let base = profiles().to_json();
        for (original, smuggled) in [
            // A shell string wherever an action could go.
            (
                r#""action": "show-overview""#,
                r#""action": "shell", "command": "rm -rf ~""#,
            ),
            (
                r#""action": "show-overview""#,
                r#""action": "keyboard-shortcut", "shortcut": "sh -c id""#,
            ),
            // Values the bounded types refuse.
            (r#""contacts": 5"#, r#""contacts": 99"#),
            (
                r#""activation_threshold": 0.6"#,
                r#""activation_threshold": 4.0"#,
            ),
            (r#""cooldown": 350"#, r#""cooldown": 99999"#),
        ] {
            let hostile = base.replace(original, smuggled);
            assert_ne!(hostile, base, "{original} is not in the document any more");
            assert!(
                GestureProfiles::from_json(&hostile).is_err(),
                "a hostile document was accepted: {hostile}"
            );
        }
    }

    #[test]
    fn a_document_whose_profile_could_not_be_recognized_is_refused() {
        let mut profiles = GestureProfiles::default();
        profiles.global.gestures = vec![gesture("overview", 4), gesture("overview", 5)];
        assert!(matches!(
            GestureProfiles::from_json(&profiles.to_json()),
            Err(ConfigError::Gesture(_))
        ));
    }
}
