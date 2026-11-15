//! `org.betteros.Hardware1`: the two hardware writes that need root (ADR
//! 0017, spec 11.2).
//!
//! One method per control and one polkit action per method. Nothing crosses
//! as free text or a path: the argument is checked against its closed set
//! before anything else happens, the caller is then asked about through
//! polkit, and only then does the daemon find the attribute itself, write
//! it, and read it back. There is no read method; reading is unprivileged and
//! the window reads sysfs itself.
//!
//! The daemon itself cannot write `/sys` (its unit has `ReadOnlyPaths=/sys`,
//! ADR 0029). A write goes through [`AttributeWriter`]: in the shipped binary
//! [`SandboxedWriter`] starts a transient oneshot unit whose `/sys` is an
//! empty read-only tmpfs with exactly the one attribute file bound in, running
//! this program's `--write-attribute`, which checks the file and value again
//! before it writes.

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

/// Writes one attribute and returns what it reads back.
pub trait AttributeWriter: Send + Sync {
    fn write(&self, path: &Path, value: &str) -> Result<String, DaemonError>;
}

/// Writes in this process. The helper uses it inside its sandbox, and tests
/// use it against a fake root; the daemon never does.
pub struct DirectWriter;

impl AttributeWriter for DirectWriter {
    fn write(&self, path: &Path, value: &str) -> Result<String, DaemonError> {
        write_attribute(path, value)
    }
}

/// The attribute files a helper may write, with the values each takes.
/// Checked by the daemon before it starts a helper and by the helper again
/// before it writes.
pub fn helper_accepts(path: &Path, value: &str) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let under_devices = path.is_absolute()
        && path.starts_with("/sys/devices/")
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        });
    let value_ok = match name {
        "charge_control_end_threshold" => value.parse::<u8>().is_ok_and(charge::accepts),
        "pwm1_enable" => FanMode::ALL
            .iter()
            .any(|m| m.pwm_enable().to_string() == value),
        _ => false,
    };
    under_devices && value_ok
}

/// A path as `systemd-run -p BindPaths=` takes it: a colon separates source
/// from destination, so the colons sysfs names carry (`LNXSYSTM:00`) are
/// escaped.
pub fn escape_bind_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace(':', "\\:")
}

/// The transient helper unit's properties: `/sys` is an empty read-only
/// tmpfs with the one file bound in, no capabilities, no network, nothing
/// else writable.
pub fn helper_properties(path: &Path) -> Vec<String> {
    [
        "TemporaryFileSystem=/sys:ro".to_string(),
        format!("BindPaths={}", escape_bind_path(path)),
        "MountAPIVFS=no".to_string(),
        "ProtectSystem=strict".to_string(),
        "ProtectHome=yes".to_string(),
        "PrivateTmp=yes".to_string(),
        "PrivateNetwork=yes".to_string(),
        "PrivateDevices=yes".to_string(),
        "NoNewPrivileges=yes".to_string(),
        "CapabilityBoundingSet=".to_string(),
        "RestrictAddressFamilies=AF_UNIX".to_string(),
        "RestrictNamespaces=yes".to_string(),
        "LockPersonality=yes".to_string(),
        "SystemCallArchitectures=native".to_string(),
    ]
    .to_vec()
}

/// Starts this program's `--write-attribute` in a transient unit and waits.
pub struct SandboxedWriter {
    pub program: PathBuf,
}

impl AttributeWriter for SandboxedWriter {
    fn write(&self, path: &Path, value: &str) -> Result<String, DaemonError> {
        if !helper_accepts(path, value) {
            return Err(DaemonError::InvalidArgument(format!("{}", path.display())));
        }
        let mut command = std::process::Command::new("systemd-run");
        command.args([
            "--quiet",
            "--wait",
            "--pipe",
            "--collect",
            "--service-type=exec",
        ]);
        command.arg("--description=Better OS hardware write");
        for property in helper_properties(path) {
            command.arg("-p").arg(property);
        }
        command
            .arg(&self.program)
            .arg("--write-attribute")
            .arg(path)
            .arg(value);
        command.stdin(std::process::Stdio::null());
        let output = command
            .output()
            .map_err(|e| DaemonError::WriteFailed(format!("systemd-run: {e}")))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(DaemonError::WriteFailed(format!(
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

/// `--write-attribute PATH VALUE`, inside the helper's sandbox.
pub fn helper_main(path: &Path, value: &str) -> Result<String, DaemonError> {
    if !helper_accepts(path, value) {
        return Err(DaemonError::InvalidArgument(path.display().to_string()));
    }
    write_attribute(path, value)
}

pub struct HardwareService<A: Authorizer + 'static> {
    authorizer: A,
    writer: Arc<dyn AttributeWriter>,
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
    pub fn new(
        authorizer: A,
        writer: Arc<dyn AttributeWriter>,
        root: SysRoot,
        gate: Arc<CallGate>,
    ) -> Self {
        Self {
            authorizer,
            writer,
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
/// The attribute's real path: the class link resolved to the device, which
/// is what the helper's sandbox binds.
fn real(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

pub fn apply_charge_limit(
    root: &SysRoot,
    writer: &dyn AttributeWriter,
    percent: u8,
) -> Result<u8, DaemonError> {
    if !charge::accepts(percent) {
        return Err(DaemonError::InvalidArgument(percent.to_string()));
    }
    let path = real(sysfs::charge_limit_attribute(root).ok_or(DaemonError::HardwareAbsent)?);
    let read = writer.write(&path, &percent.to_string())?;
    read.parse::<u8>()
        .map_err(|_| DaemonError::WriteFailed(format!("{}: reads back {read:?}", path.display())))
}

/// Finds the fan switch, writes the mode, reads it back. A value that reads
/// back as neither mode is reported with what it read.
pub fn apply_fan_mode(
    root: &SysRoot,
    writer: &dyn AttributeWriter,
    mode: FanMode,
) -> Result<String, DaemonError> {
    let path = real(sysfs::fan_mode_attribute(root).ok_or(DaemonError::HardwareAbsent)?);
    let read = writer.write(&path, &mode.pwm_enable().to_string())?;
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
pub fn restore(
    root: &SysRoot,
    writer: &dyn AttributeWriter,
    state_file: &Path,
) -> Result<Restored, DaemonError> {
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
    apply_charge_limit(root, writer, stored.charge_limit).map(Restored::Written)
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
        let writer = self.writer.clone();
        let state_file = self.state_file.clone();
        self.blocking(move || {
            let read = apply_charge_limit(&root, writer.as_ref(), percent)?;
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
        let writer = self.writer.clone();
        self.blocking(move || apply_fan_mode(&root, writer.as_ref(), mode))
            .await
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
                apply_charge_limit(&root, &DirectWriter, refused),
                Err(DaemonError::InvalidArgument(refused.to_string()))
            );
        }
        assert_eq!(std::fs::read_to_string(&attribute).unwrap(), "100\n");
        assert_eq!(apply_charge_limit(&root, &DirectWriter, 80), Ok(80));
        assert_eq!(std::fs::read_to_string(&attribute).unwrap(), "80");
    }

    #[test]
    fn a_helper_writes_only_the_two_attributes_with_their_values() {
        let limit = Path::new(
            "/sys/devices/LNXSYSTM:00/LNXSYBUS:00/PNP0C0A:00/power_supply/BAT1/charge_control_end_threshold",
        );
        let fan = Path::new("/sys/devices/platform/asus-nb-wmi/hwmon/hwmon8/pwm1_enable");
        assert!(helper_accepts(limit, "80"));
        assert!(helper_accepts(limit, "60"));
        assert!(!helper_accepts(limit, "75"));
        assert!(!helper_accepts(limit, "80\n"));
        assert!(helper_accepts(fan, "0"));
        assert!(helper_accepts(fan, "2"));
        assert!(!helper_accepts(fan, "1"));
        for refused in [
            "/sys/kernel/mm/transparent_hugepage/enabled",
            "/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy",
            "/sys/devices/../kernel/pwm1_enable",
            "sys/devices/x/pwm1_enable",
            "/tmp/pwm1_enable",
        ] {
            assert!(!helper_accepts(Path::new(refused), "2"), "{refused}");
        }
    }

    #[test]
    fn the_helper_sees_sys_only_through_the_one_bound_file() {
        let path = Path::new("/sys/devices/LNXSYSTM:00/BAT1/charge_control_end_threshold");
        let properties = helper_properties(path);
        assert_eq!(properties[0], "TemporaryFileSystem=/sys:ro");
        assert_eq!(
            properties[1],
            "BindPaths=/sys/devices/LNXSYSTM\\:00/BAT1/charge_control_end_threshold"
        );
        assert_eq!(
            properties
                .iter()
                .filter(|p| p.starts_with("BindPaths"))
                .count(),
            1
        );
        assert!(properties.contains(&"CapabilityBoundingSet=".to_string()));
        assert!(properties.contains(&"NoNewPrivileges=yes".to_string()));
        assert!(!properties.iter().any(|p| p.starts_with("ReadWritePaths")));
    }

    #[test]
    fn restore_writes_only_a_stored_value_from_the_set() {
        let dir = tempfile::tempdir().unwrap();
        let attribute = battery(dir.path(), "BAT0", "100\n");
        let root = SysRoot::at(dir.path());
        let state = dir.path().join("state/hardware.json");
        assert_eq!(
            restore(&root, &DirectWriter, &state),
            Ok(Restored::NothingStored)
        );
        store(&state, StoredHardware { charge_limit: 60 }).unwrap();
        assert_eq!(
            restore(&root, &DirectWriter, &state),
            Ok(Restored::Written(60))
        );
        assert_eq!(
            restore(&root, &DirectWriter, &state),
            Ok(Restored::AlreadySet(60))
        );
        std::fs::write(&state, r#"{"charge_limit":42}"#).unwrap();
        std::fs::write(&attribute, "100\n").unwrap();
        assert!(matches!(
            restore(&root, &DirectWriter, &state),
            Err(DaemonError::InvalidArgument(_))
        ));
        assert_eq!(std::fs::read_to_string(&attribute).unwrap(), "100\n");
        std::fs::write(&state, r#"{"charge_limit":80,"fan":"full"}"#).unwrap();
        assert!(matches!(
            restore(&root, &DirectWriter, &state),
            Err(DaemonError::Storage(_))
        ));
    }
}
