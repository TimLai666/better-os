//! `org.betteros.Hardware1`: the two hardware writes that need root (ADR
//! 0017, spec 11.2).
//!
//! One method per control and one polkit action per method. Nothing crosses
//! as free text or a path: the argument is checked against its closed set
//! before anything else happens, the caller is then asked about through
//! polkit, and only then does the daemon find the attribute itself, write
//! it, and read it back. There is no read method; reading is unprivileged and
//! the window reads sysfs itself.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hardware_core::charge;
use hardware_core::fan::{FanMode, FanState};
use hardware_platform::sysfs::{self, SysRoot};
use zbus::message::Header;
use zbus::{fdo, interface};

use crate::DaemonError;
use crate::authorize::{Authorization, Authorizer, CHARGE_LIMIT_ACTION, FAN_MODE_ACTION};
use crate::service::{CallGate, refuse};

pub const INTERFACE: &str = "org.betteros.Hardware1";

/// What the daemon stores between boots: the charge limit only. Fan mode is
/// never restored; it always starts on automatic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredHardware {
    pub charge_limit: u8,
}

pub struct HardwareService<A: Authorizer + 'static> {
    authorizer: A,
    root: SysRoot,
    /// Where the chosen limit is kept for `--restore-hardware`; `None` in
    /// tests that do not ask.
    state_file: Option<PathBuf>,
    gate: Arc<CallGate>,
    /// See `ManagerService::runtime`: zbus polls this on its own executor.
    runtime: tokio::runtime::Handle,
}

impl<A: Authorizer + 'static> HardwareService<A> {
    /// `gate` is the manager service's, so the daemon never leaves in the
    /// middle of a hardware write either.
    pub fn new(authorizer: A, root: SysRoot, gate: Arc<CallGate>) -> Self {
        Self {
            authorizer,
            root,
            state_file: None,
            gate,
            runtime: tokio::runtime::Handle::current(),
        }
    }

    pub fn with_state_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.state_file = Some(path.into());
        self
    }

    async fn authorize(&self, header: &Header<'_>, action: &'static str) -> Result<(), fdo::Error> {
        match self.authorizer.decide(header, action).await {
            Ok(Authorization::Granted) => Ok(()),
            Ok(Authorization::Dismissed) => Err(refuse(DaemonError::AuthCancelled)),
            Ok(Authorization::Denied) => Err(refuse(DaemonError::Unauthorized)),
            Err(error) => Err(refuse(error)),
        }
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, DaemonError> + Send + 'static,
    ) -> Result<T, fdo::Error> {
        self.runtime
            .spawn_blocking(work)
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?
            .map_err(refuse)
    }
}

/// Writes `value` to a sysfs attribute that already exists (never creating
/// one) and reads it back. Opened with `O_TRUNC`, as a shell's `>` does:
/// sysfs ignores it, and a plain file in a test is replaced whole.
pub fn write_attribute(path: &Path, value: &str) -> Result<String, DaemonError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| DaemonError::WriteFailed(format!("{}: {error}", path.display())))?;
    file.write_all(value.as_bytes())
        .map_err(|error| DaemonError::WriteFailed(format!("{}: {error}", path.display())))?;
    drop(file);
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .map_err(|error| DaemonError::WriteFailed(format!("{}: {error}", path.display())))
}

/// Finds the battery, writes the limit, reads it back.
pub fn apply_charge_limit(root: &SysRoot, percent: u8) -> Result<u8, DaemonError> {
    if !charge::accepts(percent) {
        return Err(DaemonError::InvalidArgument(percent.to_string()));
    }
    let path = sysfs::writable_charge_limit_attribute(root).ok_or(DaemonError::HardwareAbsent)?;
    let read = write_attribute(&path, &percent.to_string())?;
    read.parse::<u8>()
        .map_err(|_| DaemonError::WriteFailed(format!("{}: reads back {read:?}", path.display())))
}

/// Finds the fan switch, writes the mode, reads it back. A value that reads
/// back as neither mode is reported with what it read.
pub fn apply_fan_mode(root: &SysRoot, mode: FanMode) -> Result<String, DaemonError> {
    let path = sysfs::fan_mode_attribute(root).ok_or(DaemonError::HardwareAbsent)?;
    let read = write_attribute(&path, &mode.pwm_enable().to_string())?;
    match FanState::from_pwm_enable(&read) {
        Some(FanState::Mode(read_mode)) => Ok(read_mode.wire().to_string()),
        Some(FanState::Other(value)) => Ok(value.to_string()),
        None => Err(DaemonError::WriteFailed(format!(
            "{}: reads back {read:?}",
            path.display()
        ))),
    }
}

fn store(path: &Path, stored: StoredHardware) -> Result<(), DaemonError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DaemonError::Storage(e.to_string()))?;
    }
    let text = serde_json::to_string(&stored).map_err(|e| DaemonError::Storage(e.to_string()))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text).map_err(|e| DaemonError::Storage(e.to_string()))?;
    std::fs::rename(&temporary, path).map_err(|e| DaemonError::Storage(e.to_string()))
}

/// What `--restore-hardware` did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Restored {
    /// Nothing was stored.
    NothingStored,
    /// The battery already reads the stored limit.
    AlreadySet(u8),
    Written(u8),
}

/// Writes the stored charge limit back at boot, and nothing else. A stored
/// value outside the closed set is refused like a bus argument would be.
pub fn restore(root: &SysRoot, state_file: &Path) -> Result<Restored, DaemonError> {
    let text = match std::fs::read_to_string(state_file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Restored::NothingStored);
        }
        Err(error) => return Err(DaemonError::Storage(error.to_string())),
    };
    let stored: StoredHardware =
        serde_json::from_str(&text).map_err(|e| DaemonError::Storage(e.to_string()))?;
    if !charge::accepts(stored.charge_limit) {
        return Err(DaemonError::InvalidArgument(
            stored.charge_limit.to_string(),
        ));
    }
    if sysfs::charge_limit(root).value() == Some(&stored.charge_limit) {
        return Ok(Restored::AlreadySet(stored.charge_limit));
    }
    apply_charge_limit(root, stored.charge_limit).map(Restored::Written)
}

#[interface(name = "org.betteros.Hardware1")]
impl<A: Authorizer + 'static> HardwareService<A> {
    /// Sets the battery charge limit to 60, 80, or 100 and returns the value
    /// the battery reads back.
    async fn set_charge_limit(
        &self,
        #[zbus(header)] header: Header<'_>,
        percent: u8,
    ) -> Result<u8, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        if !charge::accepts(percent) {
            return Err(refuse(DaemonError::InvalidArgument(percent.to_string())));
        }
        self.authorize(&header, CHARGE_LIMIT_ACTION).await?;
        let root = self.root.clone();
        let state_file = self.state_file.clone();
        self.blocking(move || {
            let read = apply_charge_limit(&root, percent)?;
            if let Some(path) = state_file {
                store(&path, StoredHardware { charge_limit: read })?;
            }
            Ok(read)
        })
        .await
    }

    /// Sets the fan mode to `auto` or `full` and returns what it reads back:
    /// `auto`, `full`, or the raw value when it is neither.
    async fn set_fan_mode(
        &self,
        #[zbus(header)] header: Header<'_>,
        mode: &str,
    ) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        let Some(mode) = FanMode::from_wire(mode) else {
            // The argument is not echoed: it is free text from the caller.
            return Err(refuse(DaemonError::InvalidArgument("fan mode".to_string())));
        };
        self.authorize(&header, FAN_MODE_ACTION).await?;
        let root = self.root.clone();
        self.blocking(move || apply_fan_mode(&root, mode)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(dir: &Path, name: &str, limit: &str) -> PathBuf {
        let bat = dir.join("sys/class/power_supply").join(name);
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("type"), "Battery\n").unwrap();
        std::fs::write(bat.join("capacity"), "50\n").unwrap();
        let attribute = bat.join("charge_control_end_threshold");
        std::fs::write(&attribute, limit).unwrap();
        attribute
    }

    #[test]
    fn a_limit_outside_the_set_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let attribute = battery(dir.path(), "BAT1", "100\n");
        let root = SysRoot::at(dir.path());
        for refused in [0, 1, 59, 61, 79, 81, 99, 101, 255] {
            assert_eq!(
                apply_charge_limit(&root, refused),
                Err(DaemonError::InvalidArgument(refused.to_string()))
            );
        }
        assert_eq!(std::fs::read_to_string(&attribute).unwrap(), "100\n");
        assert_eq!(apply_charge_limit(&root, 80), Ok(80));
        assert_eq!(std::fs::read_to_string(&attribute).unwrap(), "80");
    }

    #[test]
    fn a_battery_the_unit_does_not_open_is_absent_for_writing() {
        let dir = tempfile::tempdir().unwrap();
        battery(dir.path(), "CMB0", "100\n");
        assert_eq!(
            apply_charge_limit(&SysRoot::at(dir.path()), 80),
            Err(DaemonError::HardwareAbsent)
        );
    }

    #[test]
    fn restore_writes_only_a_stored_value_from_the_set() {
        let dir = tempfile::tempdir().unwrap();
        let attribute = battery(dir.path(), "BAT0", "100\n");
        let root = SysRoot::at(dir.path());
        let state = dir.path().join("state/hardware.json");
        assert_eq!(restore(&root, &state), Ok(Restored::NothingStored));
        store(&state, StoredHardware { charge_limit: 60 }).unwrap();
        assert_eq!(restore(&root, &state), Ok(Restored::Written(60)));
        assert_eq!(restore(&root, &state), Ok(Restored::AlreadySet(60)));
        std::fs::write(&state, r#"{"charge_limit":42}"#).unwrap();
        std::fs::write(&attribute, "100\n").unwrap();
        assert!(matches!(
            restore(&root, &state),
            Err(DaemonError::InvalidArgument(_))
        ));
        assert_eq!(std::fs::read_to_string(&attribute).unwrap(), "100\n");
        std::fs::write(&state, r#"{"charge_limit":80,"fan":"full"}"#).unwrap();
        assert!(matches!(
            restore(&root, &state),
            Err(DaemonError::Storage(_))
        ));
    }
}
