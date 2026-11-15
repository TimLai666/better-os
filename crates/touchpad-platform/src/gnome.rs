//! The GNOME backend: read the user's dconf database, write through the dconf
//! service, verify by reading again.
//!
//! Reading reuses `defaults-platform`'s GVDB parser, which ticket 27 built and
//! tested against a database `dconf compile` produced. There is one dconf
//! reader in Better OS and this is not a second one.
//!
//! What this backend does **not** own is as important as what it does. GNOME 46
//! — the release Zorin 18 ships — has no scroll-factor key for the touchpad and
//! no smooth-scroll setting. The two scroll factors are carried instead by
//! Better Touchpad's scroll pass-through on the system bus ([`crate::scroll`],
//! ADR 0030), and are available exactly when a touchpad has one running.
//! Smooth scrolling stays unavailable with its reason attached rather than
//! drawn as a switch that does nothing. The tables below are the whole
//! mapping; `engineering/specs/touchpad-sensitivity-mapping.md` records what
//! it can and cannot promise.

use std::path::{Path, PathBuf};

use defaults_platform::gvdb::{GVariantValue, GvdbDatabase};
use touchpad_core::{
    AccelerationProfile, Capabilities, ClickMethod, Reading, ScrollFactor, Sensitivity, SettingId,
    SettingValue, Support,
};

use crate::devices::DeviceCapabilities;
use crate::gvariant::{ChangeValue, Changeset};
use crate::scroll::ScrollChannel;
use crate::{BackendStatus, TouchpadBackend, WriteOutcome};
use touchpad_scroll::factors::Factors;

#[cfg(feature = "dconf-write")]
use crate::dconf::DconfWriter;

/// Every key this backend touches lives under here.
pub const TOUCHPAD_PREFIX: &str = "/org/gnome/desktop/peripherals/touchpad/";

/// How a Better OS value is carried by a GNOME key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyShape {
    /// A `d` in `-1.0..=1.0`, where 0 is the session's neutral speed.
    Speed,
    Toggle,
    /// A `s` from `default`, `adaptive`, `flat`.
    AccelerationProfile,
    /// A `s` from `default`, `none`, `areas`, `fingers`.
    ClickMethod,
}

struct KeyMapping {
    setting: SettingId,
    key: &'static str,
    shape: KeyShape,
}

/// The controls GNOME carries, and the key each one is.
const MAPPED: &[KeyMapping] = &[
    KeyMapping {
        setting: SettingId::PointerSensitivity,
        key: "speed",
        shape: KeyShape::Speed,
    },
    KeyMapping {
        setting: SettingId::AccelerationProfile,
        key: "accel-profile",
        shape: KeyShape::AccelerationProfile,
    },
    KeyMapping {
        setting: SettingId::DisableWhileTyping,
        key: "disable-while-typing",
        shape: KeyShape::Toggle,
    },
    KeyMapping {
        setting: SettingId::NaturalScrolling,
        key: "natural-scroll",
        shape: KeyShape::Toggle,
    },
    KeyMapping {
        setting: SettingId::TwoFingerScrolling,
        key: "two-finger-scrolling-enabled",
        shape: KeyShape::Toggle,
    },
    KeyMapping {
        setting: SettingId::TapToClick,
        key: "tap-to-click",
        shape: KeyShape::Toggle,
    },
    KeyMapping {
        setting: SettingId::TapAndDrag,
        key: "tap-and-drag",
        shape: KeyShape::Toggle,
    },
    KeyMapping {
        setting: SettingId::DragLock,
        key: "tap-and-drag-lock",
        shape: KeyShape::Toggle,
    },
    KeyMapping {
        setting: SettingId::ClickMethod,
        key: "click-method",
        shape: KeyShape::ClickMethod,
    },
    KeyMapping {
        setting: SettingId::MiddleClickEmulation,
        key: "middle-click-emulation",
        shape: KeyShape::Toggle,
    },
];

/// The control nothing can carry, with the wording the screen shows.
///
/// Smooth scrolling is not an input setting: a touchpad's scrolling is already
/// continuous when it leaves libinput, and whether a view animates or glides
/// after the fingers lift is each application's own decision (GTK and the
/// browsers make it themselves). The scroll pass-through can change how far a
/// scroll goes, not how an application draws it, so it does not carry this.
const UNMAPPED: &[(SettingId, &str, &str)] = &[(
    SettingId::SmoothScrolling,
    "gnome.no_smooth_scroll_key",
    "whether scrolling animates is decided by each application, and neither GNOME nor the touchpad has a setting that changes it",
)];

/// The two controls the scroll pass-through carries.
const SCROLL_FACTORS: [SettingId; 2] = [
    SettingId::VerticalScrollFactor,
    SettingId::HorizontalScrollFactor,
];

fn is_scroll_factor(setting: SettingId) -> bool {
    SCROLL_FACTORS.contains(&setting)
}

/// Why the scroll factors are unavailable when no pass-through answers.
pub const SCROLL_ABSENT: (&str, &str) = (
    "gnome.scroll_passthrough_absent",
    "no touchpad has Better Touchpad's scroll service running, so scroll distance follows the pointer speed; the service starts for every touchpad once Better Touchpad is installed",
);

/// Whether the pass-through can carry the factors, and if not why.
fn scroll_support(scroll: Option<&dyn ScrollChannel>) -> Result<(), (String, String)> {
    let Some(scroll) = scroll else {
        return Err((SCROLL_ABSENT.0.to_string(), SCROLL_ABSENT.1.to_string()));
    };
    match scroll.factors() {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err((SCROLL_ABSENT.0.to_string(), SCROLL_ABSENT.1.to_string())),
        Err(error) => Err(("gnome.scroll_passthrough_unreachable".to_string(), error)),
    }
}

fn mapping(setting: SettingId) -> Option<&'static KeyMapping> {
    MAPPED.iter().find(|entry| entry.setting == setting)
}

fn unmapped_reason(setting: SettingId) -> Option<(&'static str, &'static str)> {
    UNMAPPED
        .iter()
        .find(|(id, _, _)| *id == setting)
        .map(|(_, reason, detail)| (*reason, *detail))
}

/// Better OS sensitivity (`0.0..=1.0`) as GNOME speed (`-1.0..=1.0`).
fn to_gnome_speed(value: Sensitivity) -> f64 {
    value.get() * 2.0 - 1.0
}

/// GNOME speed back to Better OS sensitivity. Out of range is refused, not
/// clamped: a database holding 3.0 is not a pad running at maximum, it is a
/// value nothing this application wrote.
fn from_gnome_speed(speed: f64) -> Option<Sensitivity> {
    Sensitivity::new((speed + 1.0) / 2.0).ok()
}

pub struct GnomeBackend {
    database: PathBuf,
    #[cfg(feature = "dconf-write")]
    writer: Option<DconfWriter>,
    capabilities: Capabilities,
    status: BackendStatus,
    scroll: Option<Box<dyn ScrollChannel>>,
}

impl GnomeBackend {
    /// The default per-user database location.
    pub fn user_database_path() -> PathBuf {
        let config = match std::env::var("XDG_CONFIG_HOME") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
        };
        config.join("dconf").join("user")
    }

    /// The backend as it runs on a real session: the user's own database, and
    /// a connection to the dconf service if there is one.
    ///
    /// Failing to reach the service is not an error here. It makes every
    /// control unavailable with that reason, which is the honest state and the
    /// one the GUI can render.
    #[cfg(feature = "dconf-write")]
    pub fn connect(device: Option<&DeviceCapabilities>) -> Self {
        let database = Self::user_database_path();
        let scroll = crate::scroll::SystemBusScroll::connect()
            .ok()
            .map(|scroll| Box::new(scroll) as Box<dyn ScrollChannel>);
        match DconfWriter::connect().and_then(|writer| writer.probe().map(|()| writer)) {
            Ok(writer) => {
                let status = BackendStatus::reachable(
                    "gnome",
                    format!(
                        "the dconf service answered, and {} is the database being read",
                        database.display()
                    ),
                );
                let capabilities = capabilities_for(true, device, scroll.as_deref());
                Self {
                    database,
                    writer: Some(writer),
                    capabilities,
                    status,
                    scroll,
                }
            }
            Err(error) => {
                let backend = Self::read_only_with_reason(
                    database,
                    device,
                    "gnome.dconf_service_unreachable",
                    error.to_string(),
                );
                match scroll {
                    Some(scroll) => backend.with_scroll(scroll, device),
                    None => backend,
                }
            }
        }
    }

    /// A backend that reads a database and can change nothing.
    pub fn read_only(database: impl Into<PathBuf>, device: Option<&DeviceCapabilities>) -> Self {
        Self::read_only_with_reason(
            database.into(),
            device,
            "gnome.read_only_backend",
            "this backend was built without a connection to the dconf service".to_string(),
        )
    }

    fn read_only_with_reason(
        database: PathBuf,
        device: Option<&DeviceCapabilities>,
        reason: &str,
        detail: String,
    ) -> Self {
        let status = BackendStatus::unreachable("gnome", reason, detail.clone());
        let mut capabilities = Capabilities::new();
        for setting in SettingId::ALL {
            capabilities.insert(setting, Support::unavailable(reason, detail.clone()));
        }
        let _ = device;
        Self {
            database,
            #[cfg(feature = "dconf-write")]
            writer: None,
            capabilities,
            status,
            scroll: None,
        }
    }

    /// A backend reading `database` and writing through `writer`. This is what
    /// the live apply test builds against a private bus.
    #[cfg(feature = "dconf-write")]
    pub fn with_writer(
        database: impl Into<PathBuf>,
        writer: DconfWriter,
        device: Option<&DeviceCapabilities>,
    ) -> Self {
        let database = database.into();
        Self {
            status: BackendStatus::reachable("gnome", format!("writing through {writer:?}")),
            capabilities: capabilities_for(true, device, None),
            database,
            writer: Some(writer),
            scroll: None,
        }
    }

    /// Carry the scroll factors through `scroll`. This is how a test gives the
    /// backend a fake pass-through, and how `connect` gives it the real one.
    pub fn with_scroll(
        mut self,
        scroll: Box<dyn ScrollChannel>,
        device: Option<&DeviceCapabilities>,
    ) -> Self {
        let writable = self.status.reachable;
        self.capabilities = if writable {
            capabilities_for(true, device, Some(scroll.as_ref()))
        } else {
            let mut capabilities = self.capabilities.clone();
            match scroll_support(Some(scroll.as_ref())) {
                Ok(()) => {
                    for setting in SCROLL_FACTORS {
                        capabilities.insert(setting, Support::immediate());
                    }
                }
                Err((reason, detail)) => {
                    for setting in SCROLL_FACTORS {
                        capabilities.insert(setting, Support::unavailable(&reason, &detail));
                    }
                }
            }
            capabilities
        };
        self.scroll = Some(scroll);
        self
    }

    pub fn database_path(&self) -> &Path {
        &self.database
    }

    pub fn status(&self) -> &BackendStatus {
        &self.status
    }

    /// The full dconf path of a setting, for a diagnostics screen that has to
    /// say exactly which key was read.
    pub fn key_path(setting: SettingId) -> Option<String> {
        mapping(setting).map(|entry| format!("{TOUCHPAD_PREFIX}{}", entry.key))
    }

    fn load(&self) -> Result<GvdbDatabase, Reading> {
        match std::fs::read(&self.database) {
            Ok(bytes) => GvdbDatabase::parse(&bytes)
                .map_err(|error| Reading::unknown(format!("gnome.database_unreadable:{error}"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // No database at all means the user has never changed a
                // peripheral setting. Every key is at its session default,
                // which is a definite state and a restorable one.
                Ok(GvdbDatabase::default())
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                Err(Reading::PermissionDenied {
                    reason: "gnome.database_not_readable".to_string(),
                })
            }
            Err(error) => Err(Reading::unknown(format!(
                "gnome.database_unreadable:{error}"
            ))),
        }
    }
}

/// The capability table, given whether the service can be written to and what
/// the selected pad can physically do.
fn capabilities_for(
    writable: bool,
    device: Option<&DeviceCapabilities>,
    scroll: Option<&dyn ScrollChannel>,
) -> Capabilities {
    let mut capabilities = Capabilities::new();
    // The factors are applied by the pass-through, not by dconf, so whether
    // dconf answers does not decide them.
    let scroll = scroll_support(scroll);
    for setting in SettingId::ALL {
        let support = match unmapped_reason(setting) {
            Some((reason, detail)) => Support::unavailable(reason, detail),
            None if is_scroll_factor(setting) => match &scroll {
                // The service applies a factor at its next frame.
                Ok(()) => Support::immediate(),
                Err((reason, detail)) => Support::unavailable(reason, detail),
            },
            None if !writable => Support::unavailable(
                "gnome.dconf_service_unreachable",
                "the dconf service is not answering, so no change could be applied or verified",
            ),
            // Every mapped GNOME peripheral key is picked up by the running
            // session as soon as the service emits its change notification.
            None => Support::immediate(),
        };
        capabilities.insert(setting, support);
    }
    if let Some(device) = device {
        for (setting, reason, detail) in device.limits() {
            capabilities.insert(setting, Support::unavailable(reason, detail));
        }
    }
    capabilities
}

fn to_reading(shape: KeyShape, value: Option<&GVariantValue>) -> Reading {
    let Some(value) = value else {
        return Reading::session_default("gnome.no_user_scope_value");
    };
    match (shape, value) {
        (KeyShape::Speed, GVariantValue::Double(speed)) => match from_gnome_speed(*speed) {
            Some(sensitivity) => Reading::value(SettingValue::sensitivity(sensitivity)),
            None => Reading::unknown(format!("gnome.speed_outside_its_range:{speed}")),
        },
        (KeyShape::Toggle, GVariantValue::Boolean(value)) => {
            Reading::value(SettingValue::toggle(*value))
        }
        (KeyShape::AccelerationProfile, GVariantValue::Text(text)) => {
            match AccelerationProfile::parse(text) {
                Some(profile) => Reading::value(SettingValue::acceleration(profile)),
                None => Reading::unsupported(format!("gnome.unknown_accel_profile:{text}")),
            }
        }
        (KeyShape::ClickMethod, GVariantValue::Text(text)) => match ClickMethod::parse(text) {
            Some(method) => Reading::value(SettingValue::click(method)),
            None => Reading::unsupported(format!("gnome.unknown_click_method:{text}")),
        },
        (_, GVariantValue::Malformed { signature }) => {
            Reading::unknown(format!("gnome.malformed_value:{signature}"))
        }
        // The key holds a type this setting is not. That is a database somebody
        // else wrote, and guessing what they meant is exactly the wrong move.
        (shape, value) => Reading::unknown(format!(
            "gnome.value_type_does_not_match:{shape:?}:{value:?}"
        )),
    }
}

fn to_change_value(shape: KeyShape, value: SettingValue) -> Option<ChangeValue> {
    match (shape, value) {
        (KeyShape::Speed, SettingValue::Sensitivity { value }) => {
            Some(ChangeValue::Double(to_gnome_speed(value)))
        }
        (KeyShape::Toggle, SettingValue::Toggle { value }) => Some(ChangeValue::Boolean(value)),
        (KeyShape::AccelerationProfile, SettingValue::Acceleration { value }) => {
            Some(ChangeValue::Text(value.key().to_string()))
        }
        (KeyShape::ClickMethod, SettingValue::Click { value }) => {
            Some(ChangeValue::Text(value.key().to_string()))
        }
        _ => None,
    }
}

fn factor_reading(factors: Factors, setting: SettingId) -> Reading {
    let value = if setting == SettingId::VerticalScrollFactor {
        factors.vertical
    } else {
        factors.horizontal
    };
    match ScrollFactor::new(value) {
        Ok(factor) => Reading::value(SettingValue::factor(factor)),
        Err(_) => Reading::unknown(format!("gnome.scroll_factor_out_of_range:{value}")),
    }
}

impl TouchpadBackend for GnomeBackend {
    fn name(&self) -> &'static str {
        "gnome"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// The recorded status, which names the database and the service rather
    /// than only counting controls.
    fn status(&self) -> crate::BackendStatus {
        self.status.clone()
    }

    fn read(&self, settings: &[SettingId]) -> Vec<(SettingId, Reading)> {
        let database = match self.load() {
            Ok(database) => database,
            Err(reading) => {
                return settings
                    .iter()
                    .map(|setting| (*setting, reading.clone()))
                    .collect();
            }
        };
        settings
            .iter()
            .map(|setting| {
                if is_scroll_factor(*setting) {
                    return (*setting, self.read_factor(*setting));
                }
                let reading = match mapping(*setting) {
                    None => {
                        let (reason, detail) = unmapped_reason(*setting)
                            .unwrap_or(("gnome.no_key", "this GNOME has no such key"));
                        let _ = detail;
                        Reading::unsupported(reason)
                    }
                    Some(entry) => to_reading(
                        entry.shape,
                        database.get(&format!("{TOUCHPAD_PREFIX}{}", entry.key)),
                    ),
                };
                (*setting, reading)
            })
            .collect()
    }

    fn write_all(
        &mut self,
        writes: &[(SettingId, Option<SettingValue>)],
    ) -> Vec<(SettingId, WriteOutcome)> {
        let mut outcomes = Vec::with_capacity(writes.len());
        let mut changeset = match Changeset::new(TOUCHPAD_PREFIX) {
            Ok(changeset) => changeset,
            Err(error) => {
                return writes
                    .iter()
                    .map(|(setting, _)| {
                        (
                            *setting,
                            WriteOutcome::failed("gnome.changeset_rejected", error.to_string()),
                        )
                    })
                    .collect();
            }
        };
        let mut queued = Vec::new();

        let factor_writes: Vec<(SettingId, Option<SettingValue>)> = writes
            .iter()
            .filter(|(setting, _)| is_scroll_factor(*setting))
            .copied()
            .collect();
        if !factor_writes.is_empty() {
            outcomes.extend(self.write_factors(&factor_writes));
        }

        for (setting, value) in writes {
            if is_scroll_factor(*setting) {
                continue;
            }
            let Some(entry) = mapping(*setting) else {
                let (reason, detail) = unmapped_reason(*setting)
                    .unwrap_or(("gnome.no_key", "this GNOME has no such key"));
                outcomes.push((*setting, WriteOutcome::unsupported(reason, detail)));
                continue;
            };
            if !self.capabilities.is_available(*setting) {
                let support = self.capabilities.support(*setting);
                let (reason, detail) = match support {
                    Support::Unavailable { reason, detail } => (reason, detail),
                    Support::Full { .. } => unreachable!("checked unavailable"),
                };
                outcomes.push((*setting, WriteOutcome::unsupported(reason, detail)));
                continue;
            }
            let staged = match value {
                None => changeset.reset(entry.key),
                Some(value) => match to_change_value(entry.shape, *value) {
                    Some(change) => changeset.set(entry.key, change),
                    None => {
                        outcomes.push((
                            *setting,
                            WriteOutcome::failed(
                                "gnome.value_kind_does_not_fit_the_key",
                                format!("{setting} cannot be written to {}", entry.key),
                            ),
                        ));
                        continue;
                    }
                },
            };
            match staged {
                Ok(()) => queued.push(*setting),
                Err(error) => outcomes.push((
                    *setting,
                    WriteOutcome::failed("gnome.changeset_rejected", error.to_string()),
                )),
            }
        }

        let result = self.send(&changeset);
        for setting in queued {
            outcomes.push((
                setting,
                match &result {
                    Ok(()) => WriteOutcome::Written,
                    Err(error) => {
                        WriteOutcome::failed("gnome.dconf_write_failed", error.to_string())
                    }
                },
            ));
        }
        outcomes
    }
}

impl GnomeBackend {
    fn read_factor(&self, setting: SettingId) -> Reading {
        let Some(scroll) = &self.scroll else {
            return Reading::unsupported(SCROLL_ABSENT.0);
        };
        match scroll.factors() {
            Ok(Some(factors)) => factor_reading(factors, setting),
            Ok(None) => Reading::unsupported(SCROLL_ABSENT.0),
            Err(error) => Reading::unknown(format!("gnome.scroll_passthrough_unreachable:{error}")),
        }
    }

    /// Both factors go in one call, so a change to one keeps the other at
    /// what is in force. Resetting a factor (`None`) is setting it to 1, the
    /// value at which the pass-through holds nothing.
    fn write_factors(
        &mut self,
        writes: &[(SettingId, Option<SettingValue>)],
    ) -> Vec<(SettingId, WriteOutcome)> {
        let refuse = |reason: &str, detail: &str| {
            writes
                .iter()
                .map(|(setting, _)| (*setting, WriteOutcome::unsupported(reason, detail)))
                .collect::<Vec<_>>()
        };
        if let Some((setting, _)) = writes
            .iter()
            .find(|(setting, _)| !self.capabilities.is_available(*setting))
            && let Support::Unavailable { reason, detail } = self.capabilities.support(*setting)
        {
            return refuse(&reason, &detail);
        }
        let Some(scroll) = &self.scroll else {
            return refuse(SCROLL_ABSENT.0, SCROLL_ABSENT.1);
        };
        let failed = |detail: String| {
            writes
                .iter()
                .map(|(setting, _)| {
                    (
                        *setting,
                        WriteOutcome::failed(
                            "gnome.scroll_passthrough_write_failed",
                            detail.clone(),
                        ),
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut factors = match scroll.factors() {
            Ok(Some(factors)) => factors,
            Ok(None) => return refuse(SCROLL_ABSENT.0, SCROLL_ABSENT.1),
            Err(error) => return failed(error),
        };
        for (setting, value) in writes {
            let factor = match value {
                None => 1.0,
                Some(SettingValue::Factor { value }) => value.get(),
                Some(_) => {
                    return failed(format!("{setting} takes a scroll factor"));
                }
            };
            if *setting == SettingId::VerticalScrollFactor {
                factors.vertical = factor;
            } else {
                factors.horizontal = factor;
            }
        }
        match scroll.set(factors) {
            Ok(()) => writes
                .iter()
                .map(|(setting, _)| (*setting, WriteOutcome::Written))
                .collect(),
            Err(error) => failed(error),
        }
    }

    #[cfg(feature = "dconf-write")]
    fn send(&self, changeset: &Changeset) -> Result<(), crate::PlatformError> {
        if changeset.is_empty() {
            return Ok(());
        }
        match &self.writer {
            Some(writer) => writer
                .change(changeset)
                .map(|_tag| ())
                .map_err(crate::PlatformError::from),
            None => Err(crate::PlatformError::NoWriteSupport),
        }
    }

    #[cfg(not(feature = "dconf-write"))]
    fn send(&self, changeset: &Changeset) -> Result<(), crate::PlatformError> {
        if changeset.is_empty() {
            return Ok(());
        }
        Err(crate::PlatformError::NoWriteSupport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use touchpad_core::{ApplyPlan, ApplyStep, RunState, SessionEffect, StepOutcome};

    fn fixture() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dconf/user"
        ))
    }

    fn backend() -> GnomeBackend {
        GnomeBackend::read_only(fixture(), None)
    }

    #[test]
    fn the_speed_key_is_read_onto_the_better_os_scale() {
        // The fixture holds speed = 0.5 on GNOME's -1..1 scale.
        assert_eq!(
            backend().read_one(SettingId::PointerSensitivity),
            Reading::value(SettingValue::sensitivity(Sensitivity::new(0.75).unwrap()))
        );
    }

    #[test]
    fn the_scale_conversion_round_trips_across_the_whole_range() {
        for step in 0..=100 {
            let value = Sensitivity::new(f64::from(step) / 100.0).unwrap();
            let back = from_gnome_speed(to_gnome_speed(value)).expect("stays in range");
            assert!(
                (back.get() - value.get()).abs() <= crate::VALUE_TOLERANCE,
                "{} became {}",
                value.get(),
                back.get()
            );
        }
        assert_eq!(to_gnome_speed(Sensitivity::neutral()), 0.0);
        assert_eq!(to_gnome_speed(Sensitivity::new(0.0).unwrap()), -1.0);
        assert_eq!(to_gnome_speed(Sensitivity::new(1.0).unwrap()), 1.0);
    }

    #[test]
    fn a_speed_outside_gnomes_own_range_is_reported_rather_than_clamped() {
        assert!(from_gnome_speed(3.0).is_none());
        assert_eq!(
            to_reading(KeyShape::Speed, Some(&GVariantValue::Double(3.0))),
            Reading::unknown("gnome.speed_outside_its_range:3")
        );
    }

    #[test]
    fn booleans_and_enumerations_are_read_as_their_typed_values() {
        let backend = backend();
        assert_eq!(
            backend.read_one(SettingId::TapToClick),
            Reading::value(SettingValue::toggle(true))
        );
        assert_eq!(
            backend.read_one(SettingId::NaturalScrolling),
            Reading::value(SettingValue::toggle(false))
        );
        assert_eq!(
            backend.read_one(SettingId::ClickMethod),
            Reading::value(SettingValue::click(ClickMethod::Fingers))
        );
        assert_eq!(
            backend.read_one(SettingId::AccelerationProfile),
            Reading::value(SettingValue::acceleration(AccelerationProfile::Flat))
        );
    }

    #[test]
    fn a_key_the_user_database_does_not_hold_is_the_session_default_not_unknown() {
        // Restoring to "nothing was set" is a real, reproducible state, so it
        // must not be collapsed into "cannot be determined".
        let reading = backend().read_one(SettingId::MiddleClickEmulation);
        assert_eq!(
            reading,
            Reading::session_default("gnome.no_user_scope_value")
        );
        assert!(reading.is_determinate());
    }

    #[test]
    fn a_key_holding_the_wrong_type_is_unknown_rather_than_coerced() {
        assert!(matches!(
            to_reading(KeyShape::Toggle, Some(&GVariantValue::Double(1.0))),
            Reading::Unknown { .. }
        ));
    }

    #[test]
    fn a_click_method_gnome_grew_later_is_unsupported_rather_than_guessed() {
        assert_eq!(
            to_reading(
                KeyShape::ClickMethod,
                Some(&GVariantValue::Text("clickfinger-two".to_string()))
            ),
            Reading::unsupported("gnome.unknown_click_method:clickfinger-two")
        );
    }

    #[test]
    fn the_three_controls_gnome_has_no_key_for_are_unavailable_with_a_reason() {
        let backend = backend();
        for setting in [
            SettingId::VerticalScrollFactor,
            SettingId::HorizontalScrollFactor,
            SettingId::SmoothScrolling,
        ] {
            assert!(matches!(
                backend.read_one(setting),
                Reading::Unsupported { .. }
            ));
            assert_eq!(GnomeBackend::key_path(setting), None);
        }
        let capabilities = capabilities_for(true, None, None);
        assert!(!capabilities.is_available(SettingId::VerticalScrollFactor));
        let Support::Unavailable { reason, detail } =
            capabilities.support(SettingId::VerticalScrollFactor)
        else {
            panic!("a factor with no pass-through must not be available");
        };
        assert_eq!(reason, SCROLL_ABSENT.0);
        assert!(detail.contains("scroll service"));
        let Support::Unavailable { reason, .. } = capabilities.support(SettingId::SmoothScrolling)
        else {
            panic!("smooth scrolling has no carrier");
        };
        assert_eq!(reason, "gnome.no_smooth_scroll_key");
    }

    /// A pass-through that remembers what it was told.
    struct FakeScroll {
        factors: std::sync::Arc<std::sync::Mutex<Option<Factors>>>,
        fail: bool,
    }

    impl ScrollChannel for FakeScroll {
        fn factors(&self) -> Result<Option<Factors>, String> {
            if self.fail {
                return Err("the system bus is gone".to_string());
            }
            Ok(*self.factors.lock().unwrap())
        }

        fn set(&self, factors: Factors) -> Result<(), String> {
            *self.factors.lock().unwrap() = Some(factors);
            Ok(())
        }
    }

    fn with_fake_scroll(
        initial: Option<Factors>,
    ) -> (
        GnomeBackend,
        std::sync::Arc<std::sync::Mutex<Option<Factors>>>,
    ) {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(initial));
        let backend = backend().with_scroll(
            Box::new(FakeScroll {
                factors: shared.clone(),
                fail: false,
            }),
            None,
        );
        (backend, shared)
    }

    #[test]
    fn the_scroll_factors_are_live_when_a_pass_through_answers() {
        let (backend, _) = with_fake_scroll(Some(Factors::new(2.0, 0.5).unwrap()));
        for setting in SCROLL_FACTORS {
            assert_eq!(
                backend.capabilities().support(setting).effect(),
                Some(SessionEffect::Immediate)
            );
        }
        assert_eq!(
            backend.read_one(SettingId::VerticalScrollFactor),
            Reading::value(SettingValue::factor(ScrollFactor::new(2.0).unwrap()))
        );
        assert_eq!(
            backend.read_one(SettingId::HorizontalScrollFactor),
            Reading::value(SettingValue::factor(ScrollFactor::new(0.5).unwrap()))
        );
        // Smooth scrolling is still not carried by anything.
        assert!(
            !backend
                .capabilities()
                .is_available(SettingId::SmoothScrolling)
        );
    }

    #[test]
    fn applying_a_factor_writes_both_and_verifies_by_reading_back() {
        let (mut backend, shared) = with_fake_scroll(Some(Factors::NEUTRAL));
        let report = backend.apply(&ApplyPlan {
            steps: vec![ApplyStep {
                setting: SettingId::VerticalScrollFactor,
                requested: SettingValue::factor(ScrollFactor::new(3.0).unwrap()),
                captured: Reading::value(SettingValue::factor(ScrollFactor::neutral())),
                effect: SessionEffect::Immediate,
            }],
            skipped: Vec::new(),
        });
        assert_eq!(report.state(), RunState::Applied, "{report:?}");
        // The other axis kept the value in force.
        assert_eq!(
            *shared.lock().unwrap(),
            Some(Factors::new(3.0, 1.0).unwrap())
        );
    }

    #[test]
    fn resetting_a_factor_returns_it_to_one() {
        let (mut backend, shared) = with_fake_scroll(Some(Factors::new(3.0, 2.0).unwrap()));
        let outcomes = backend.write_all(&[(SettingId::HorizontalScrollFactor, None)]);
        assert_eq!(
            outcomes,
            vec![(SettingId::HorizontalScrollFactor, WriteOutcome::Written)]
        );
        assert_eq!(
            *shared.lock().unwrap(),
            Some(Factors::new(3.0, 1.0).unwrap())
        );
    }

    #[test]
    fn no_touchpad_with_a_pass_through_means_no_factor() {
        let (mut backend, _) = with_fake_scroll(None);
        assert!(
            !backend
                .capabilities()
                .is_available(SettingId::VerticalScrollFactor)
        );
        assert_eq!(
            backend.read_one(SettingId::VerticalScrollFactor),
            Reading::unsupported(SCROLL_ABSENT.0)
        );
        let outcomes = backend.write_all(&[(
            SettingId::VerticalScrollFactor,
            Some(SettingValue::factor(ScrollFactor::new(2.0).unwrap())),
        )]);
        assert!(matches!(outcomes[0].1, WriteOutcome::Unsupported { .. }));
    }

    #[test]
    fn an_unreachable_pass_through_is_unknown_not_a_default() {
        let backend = backend().with_scroll(
            Box::new(FakeScroll {
                factors: Default::default(),
                fail: true,
            }),
            None,
        );
        let Support::Unavailable { reason, detail } = backend
            .capabilities()
            .support(SettingId::VerticalScrollFactor)
        else {
            panic!("an unreachable pass-through offers nothing");
        };
        assert_eq!(reason, "gnome.scroll_passthrough_unreachable");
        assert!(detail.contains("system bus"));
        assert!(matches!(
            backend.read_one(SettingId::VerticalScrollFactor),
            Reading::Unknown { .. }
        ));
    }

    #[test]
    fn the_ten_controls_gnome_does_carry_are_available_and_immediate() {
        let capabilities = capabilities_for(true, None, None);
        assert_eq!(capabilities.available().len(), 10);
        for setting in capabilities.available() {
            assert_eq!(
                capabilities.support(setting).effect(),
                Some(SessionEffect::Immediate),
                "{setting} is not immediate"
            );
            assert!(
                GnomeBackend::key_path(setting)
                    .unwrap()
                    .starts_with(TOUCHPAD_PREFIX)
            );
        }
    }

    #[test]
    fn nothing_is_offered_when_the_dconf_service_cannot_be_reached() {
        let capabilities = capabilities_for(false, None, None);
        assert!(capabilities.available().is_empty());
        let backend = backend();
        assert!(!backend.status().reachable);
        assert!(backend.capabilities().available().is_empty());
    }

    #[test]
    fn a_pad_that_cannot_do_something_overrides_a_backend_that_can() {
        let device = DeviceCapabilities {
            pointer: true,
            buttonpad: false,
            semi_multitouch: true,
            multitouch: false,
            max_contacts: 1,
            physical_middle_button: true,
        };
        let capabilities = capabilities_for(true, Some(&device), None);
        assert!(!capabilities.is_available(SettingId::TwoFingerScrolling));
        assert!(!capabilities.is_available(SettingId::MiddleClickEmulation));
        assert!(!capabilities.is_available(SettingId::ClickMethod));
        assert!(capabilities.is_available(SettingId::TapToClick));
    }

    #[test]
    fn a_missing_database_reads_as_session_defaults_rather_than_as_an_error() {
        let backend = GnomeBackend::read_only("/nonexistent/dconf/user", None);
        assert!(matches!(
            backend.read_one(SettingId::TapToClick),
            Reading::SessionDefault { .. }
        ));
    }

    #[test]
    fn a_database_that_is_not_a_database_is_unknown_for_every_setting() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("user");
        std::fs::write(&path, b"this is not a GVDB file").unwrap();
        let backend = GnomeBackend::read_only(&path, None);
        for (_, reading) in backend.read_all() {
            assert!(matches!(reading, Reading::Unknown { .. }));
        }
    }

    #[test]
    fn a_read_only_backend_writes_nothing_and_says_so() {
        let mut backend = backend();
        let before = std::fs::read(fixture()).unwrap();
        let report = backend.apply(&ApplyPlan {
            steps: vec![ApplyStep {
                setting: SettingId::TapToClick,
                requested: SettingValue::toggle(false),
                captured: Reading::value(SettingValue::toggle(true)),
                effect: SessionEffect::Immediate,
            }],
            skipped: Vec::new(),
        });
        assert_eq!(report.state(), RunState::PartiallySupported);
        assert!(matches!(
            report.outcome(SettingId::TapToClick),
            Some(StepOutcome::Unsupported { .. })
        ));
        assert_eq!(std::fs::read(fixture()).unwrap(), before);
    }

    #[test]
    fn a_change_set_names_the_keys_it_would_write_and_nothing_else() {
        let mut changeset = Changeset::new(TOUCHPAD_PREFIX).unwrap();
        changeset
            .set(
                "speed",
                to_change_value(
                    KeyShape::Speed,
                    SettingValue::sensitivity(Sensitivity::new(0.75).unwrap()),
                )
                .unwrap(),
            )
            .unwrap();
        changeset.reset("tap-to-click").unwrap();
        assert_eq!(
            changeset.paths(),
            vec![
                "/org/gnome/desktop/peripherals/touchpad/speed".to_string(),
                "/org/gnome/desktop/peripherals/touchpad/tap-to-click".to_string(),
            ]
        );
    }

    #[test]
    fn a_value_of_the_wrong_kind_for_a_key_is_refused_before_it_is_encoded() {
        assert_eq!(
            to_change_value(KeyShape::Speed, SettingValue::toggle(true)),
            None
        );
        assert_eq!(
            to_change_value(
                KeyShape::ClickMethod,
                SettingValue::acceleration(AccelerationProfile::Flat)
            ),
            None
        );
    }
}
