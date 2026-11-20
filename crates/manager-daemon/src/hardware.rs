//! `org.betteros.Hardware1`: the hardware writes that need root (ADR 0017,
//! spec 11.2): the charge limit, the fan mode, and the keyboard's colour
//! (ADR 0034, `keyboard_lighting`).
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
use crate::authorize::{
    Authorization, Authorizer, CHARGE_LIMIT_ACTION, FAN_MODE_ACTION, KEYBOARD_LIGHTING_ACTION,
};
use crate::keyboard_lighting::{self, LampCache, LampHelper, LightingPaths};
use crate::service::{CallGate, refuse};
use hardware_core::lighting::{Effect, LightingChoice, Rgb, Speed, asus_wmi};

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
        // asus-wmi's keyboard colour (ADR 0034): only a value the closed set
        // produces, `cmd` 0 (never saved to the firmware).
        asus_wmi::ATTRIBUTE => asus_wmi::accepts_value(value),
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

/// The sandbox every hardware helper unit runs in (ADR 0029, narrowed by
/// ADR 0034's security review): `/sys` an empty read-only tmpfs, a private
/// `/dev` of pseudo-devices, `/proc/sys` read-only, no network, nothing
/// writable, a system-call allow-list, no realtime, no new privileges, and
/// ten seconds to finish (a hung I2C ioctl would otherwise hold the
/// keyboard's lock, and a removal's prerm, for ever). Each helper adds the
/// one file or node it needs and its capability bounding set.
///
/// Three hardening options are deliberately absent: `ProtectKernelTunables=`,
/// `ProtectControlGroups=`, and `ProtectProc=` each bring the API file
/// systems back (`MountAPIVFS=`), and with them the whole of `/sys` over the
/// one-file tmpfs. Seen on systemd 255 (transient units in a user manager,
/// 2026-11-21: `ls /sys` lists 11 entries with any of them, 1 without), the
/// same duplicate-mount rule ADR 0029 found. What they protect is covered
/// otherwise: `/proc/sys` and `/proc/sysrq-trigger` are bound read-only, the
/// cgroup tree lives under the empty `/sys`, and the LampArray helper runs
/// as `nobody` once its node is open.
pub fn helper_sandbox() -> Vec<String> {
    [
        "TemporaryFileSystem=/sys:ro",
        "MountAPIVFS=no",
        "ProtectSystem=strict",
        "ProtectHome=yes",
        "PrivateTmp=yes",
        "PrivateNetwork=yes",
        "PrivateDevices=yes",
        "ReadOnlyPaths=/proc/sys",
        "ReadOnlyPaths=/proc/sysrq-trigger",
        "ProtectKernelModules=yes",
        "ProtectKernelLogs=yes",
        "ProtectClock=yes",
        "ProtectHostname=yes",
        "NoNewPrivileges=yes",
        "RestrictSUIDSGID=yes",
        "RestrictRealtime=yes",
        "RestrictAddressFamilies=AF_UNIX",
        "RestrictNamespaces=yes",
        "LockPersonality=yes",
        "MemoryDenyWriteExecute=yes",
        "SystemCallArchitectures=native",
        "SystemCallFilter=@system-service",
        "RuntimeMaxSec=10s",
    ]
    .iter()
    .map(|p| p.to_string())
    .collect()
}

/// The attribute helper's properties: the shared sandbox with exactly the
/// one attribute file bound writable, and no capability at all. It stays
/// uid 0, which owns the file; its only input is its two arguments, checked
/// against closed sets, not a device's replies.
pub fn helper_properties(path: &Path) -> Vec<String> {
    let mut properties = helper_sandbox();
    properties.insert(1, format!("BindPaths={}", escape_bind_path(path)));
    properties.push("CapabilityBoundingSet=".to_string());
    properties
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
    /// The keyboard's LampArray helper (ADR 0034); `None` drives no
    /// LampArray.
    lamp_helper: Option<Arc<dyn LampHelper>>,
    /// Where the keyboard's colour is kept for `--restore-hardware`.
    lighting_paths: Option<LightingPaths>,
    lamp_cache: Arc<LampCache>,
    /// One keyboard change at a time, the newest only, at most ten a
    /// second (ADR 0034).
    lighting: Arc<Coalescer<LightingChoice>>,
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
            lamp_helper: None,
            lighting_paths: None,
            lamp_cache: Arc::new(LampCache::default()),
            lighting: Arc::new(Coalescer::new(LIGHTING_WAITERS, LIGHTING_INTERVAL)),
        }
    }

    pub fn with_lamp_helper(mut self, helper: Arc<dyn LampHelper>) -> Self {
        self.lamp_helper = Some(helper);
        self
    }

    pub fn with_lighting_paths(mut self, paths: LightingPaths) -> Self {
        self.lighting_paths = Some(paths);
        self
    }

    /// Where the keyboard's choice is read when no paths were given: a path
    /// that holds nothing, so nothing is ever read as stored.
    fn lighting_paths(&self) -> LightingPaths {
        self.lighting_paths.clone().unwrap_or_else(|| {
            LightingPaths::under(std::path::Path::new("/nonexistent/better-os-keyboard"))
        })
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
    // `kbd_rgb_mode` is write-only (`DEVICE_ATTR_WO`): there is nothing to
    // read back, and the write's own success is the answer.
    let write_only = path.file_name().and_then(|n| n.to_str()) == Some(asus_wmi::ATTRIBUTE);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| DaemonError::WriteFailed(format!("{}: {error}", path.display())))?;
    file.write_all(value.as_bytes())
        .map_err(|error| DaemonError::WriteFailed(format!("{}: {error}", path.display())))?;
    drop(file);
    if write_only {
        return Ok(value.to_string());
    }
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

    /// What the keyboard's colour interface is, what its lamps can show, and
    /// what Better OS set, as JSON (`hardware_core::lighting::LightingReply`).
    /// Reading a LampArray opens the device, so it is asked for like a write.
    async fn keyboard_lighting(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        self.authorize(&header, KEYBOARD_LIGHTING_ACTION).await?;
        // In line behind any change, so a helper never meets another one.
        let _turn = self.lighting.queue().await;
        let root = self.root.clone();
        let helper = self.lamp_helper.clone();
        let cache = self.lamp_cache.clone();
        let paths = self.lighting_paths();
        self.blocking(move || {
            let helper = helper_or_none(helper.as_deref());
            let reply = keyboard_lighting::describe(&root, helper, &cache, &paths)?;
            serde_json::to_string(&reply).map_err(|e| DaemonError::WriteFailed(e.to_string()))
        })
        .await
    }

    /// Sets the keyboard's effect (`firmware`, `static`, `breathing`,
    /// `cycle`, `strobe`), colour, intensity (0-100), and speed (0-2), and
    /// returns what `KeyboardLighting` then answers. An effect the keyboard
    /// does not do itself is refused before anything is touched.
    ///
    /// A caller in a loop is coalesced: calls wait their turn without a
    /// thread each, only the newest choice waiting is written, at most one
    /// write every 100 ms, and past eight waiting a call is refused as busy.
    #[allow(clippy::too_many_arguments)]
    async fn set_keyboard_lighting(
        &self,
        #[zbus(header)] header: Header<'_>,
        effect: &str,
        red: u8,
        green: u8,
        blue: u8,
        intensity: u8,
        speed: u8,
    ) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        // Neither is echoed: they are the caller's.
        let Some(effect) = Effect::from_wire(effect) else {
            return Err(refuse(DaemonError::InvalidArgument("effect".to_string())));
        };
        let Some(speed) = Speed::from_wire(speed) else {
            return Err(refuse(DaemonError::InvalidArgument("speed".to_string())));
        };
        if intensity > 100 {
            return Err(refuse(DaemonError::InvalidArgument(
                "intensity".to_string(),
            )));
        }
        let choice = LightingChoice {
            effect,
            colour: Rgb::new(red, green, blue),
            intensity,
            speed,
        };
        self.authorize(&header, KEYBOARD_LIGHTING_ACTION).await?;
        let turn = self.lighting.submit(choice).await.map_err(refuse)?;
        let root = self.root.clone();
        let helper = self.lamp_helper.clone();
        let writer = self.writer.clone();
        let cache = self.lamp_cache.clone();
        let paths = self.lighting_paths.clone();
        let read_paths = self.lighting_paths();
        let newest = turn.value;
        let wait = turn.wait;
        let reply = self
            .blocking(move || {
                let helper = helper_or_none(helper.as_deref());
                // A newer choice than this call's may be the one written:
                // every caller waiting gets the newest one's answer.
                if let Some(choice) = newest {
                    std::thread::sleep(wait);
                    let device =
                        keyboard_lighting::apply(&root, helper, writer.as_ref(), &cache, &choice)?;
                    if let Some(paths) = &paths {
                        keyboard_lighting::store(paths, device, &choice)?;
                    }
                }
                let reply = keyboard_lighting::describe(&root, helper, &cache, &read_paths)?;
                serde_json::to_string(&reply).map_err(|e| DaemonError::WriteFailed(e.to_string()))
            })
            .await;
        if newest.is_some() {
            turn.written();
        }
        reply
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

/// How many keyboard changes may wait for their turn before a call is
/// refused as busy.
pub const LIGHTING_WAITERS: usize = 8;

/// The shortest time between two keyboard writes.
pub const LIGHTING_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// Keeps only the newest value waiting and spaces the work out (ADR 0034's
/// review, finding 8). Callers wait on an async lock, so a caller in a loop
/// costs no thread per call; the first caller through the lock takes the
/// newest value submitted so far, and a caller that finds nothing left
/// (a later one's value was already taken) has nothing to write. The lock is
/// `tokio::sync::Mutex`, which needs no runtime to poll, so it is safe on
/// zbus's own executor.
pub struct Coalescer<T> {
    latest: std::sync::Mutex<Option<T>>,
    waiting: std::sync::atomic::AtomicUsize,
    gate: tokio::sync::Mutex<Option<std::time::Instant>>,
    limit: usize,
    interval: std::time::Duration,
}

/// A caller's turn: the value to write, if any is left, and how long to
/// wait first so writes stay `interval` apart. Holding it holds the lock.
pub struct Turn<'a, T> {
    pub value: Option<T>,
    pub wait: std::time::Duration,
    guard: tokio::sync::MutexGuard<'a, Option<std::time::Instant>>,
}

impl<T> Turn<'_, T> {
    /// The value was written now.
    pub fn written(mut self) {
        *self.guard = Some(std::time::Instant::now());
    }
}

struct Waiting<'a>(&'a std::sync::atomic::AtomicUsize);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl<T> Coalescer<T> {
    pub fn new(limit: usize, interval: std::time::Duration) -> Coalescer<T> {
        Coalescer {
            latest: std::sync::Mutex::new(None),
            waiting: std::sync::atomic::AtomicUsize::new(0),
            gate: tokio::sync::Mutex::new(None),
            limit,
            interval,
        }
    }

    /// Hands `value` in as the newest and waits for a turn.
    pub async fn submit(&self, value: T) -> Result<Turn<'_, T>, DaemonError> {
        let before = self
            .waiting
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _waiting = Waiting(&self.waiting);
        if before >= self.limit {
            return Err(DaemonError::Busy);
        }
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(value);
        }
        let guard = self.gate.lock().await;
        let value = self.latest.lock().ok().and_then(|mut latest| latest.take());
        let wait = guard
            .map(|last| self.interval.saturating_sub(last.elapsed()))
            .unwrap_or_default();
        Ok(Turn { value, wait, guard })
    }

    /// Waits for a turn without a value: a read in line behind the writes.
    pub async fn queue(&self) -> tokio::sync::MutexGuard<'_, Option<std::time::Instant>> {
        self.gate.lock().await
    }
}

/// A service built without a LampArray helper finds none to drive.
struct NoLampHelper;

impl LampHelper for NoLampHelper {
    fn run(
        &self,
        _node: &str,
        _command: keyboard_lighting::LampCommand,
    ) -> Result<Option<hardware_core::lamparray::Description>, DaemonError> {
        Err(DaemonError::WriteFailed(
            "this daemon was built without the keyboard helper".to_string(),
        ))
    }
}

fn helper_or_none(helper: Option<&dyn LampHelper>) -> &dyn LampHelper {
    helper.unwrap_or(&NoLampHelper)
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
        let rgb =
            Path::new("/sys/devices/platform/asus-nb-wmi/leds/asus::kbd_backlight/kbd_rgb_mode");
        assert!(helper_accepts(rgb, "0 1 255 0 0 2"));
        assert!(!helper_accepts(rgb, "1 1 255 0 0 2"));
        assert!(!helper_accepts(rgb, "0 4 255 0 0 2"));
        assert!(!helper_accepts(
            Path::new("/sys/kernel/kbd_rgb_mode"),
            "0 0 1 2 3 1"
        ));
        assert!(!helper_accepts(
            Path::new("/sys/devices/platform/asus-nb-wmi/leds/asus::kbd_backlight/kbd_rgb_state"),
            "0 1 1 1 1"
        ));
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
        // ADR 0034's review: the shared hardening, every line of it.
        for wanted in [
            "PrivateDevices=yes",
            "ReadOnlyPaths=/proc/sys",
            "ReadOnlyPaths=/proc/sysrq-trigger",
            "ProtectKernelModules=yes",
            "ProtectKernelLogs=yes",
            "ProtectClock=yes",
            "RestrictSUIDSGID=yes",
            "SystemCallFilter=@system-service",
            "MemoryDenyWriteExecute=yes",
            "RuntimeMaxSec=10s",
        ] {
            assert!(properties.contains(&wanted.to_string()), "{wanted}");
        }
        // These would bring the whole of /sys back over the one-file tmpfs.
        for refused in [
            "ProtectKernelTunables",
            "ProtectControlGroups",
            "ProtectProc",
            "MountAPIVFS=yes",
        ] {
            assert!(
                !properties.iter().any(|p| p.starts_with(refused)),
                "{refused}"
            );
        }
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

    /// ADR 0034's review, finding 8: a caller in a loop writes the newest
    /// value only, spaced out, and too many waiting are refused.
    #[test]
    fn the_newest_change_is_written_and_the_rest_are_coalesced() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let coalescer =
                std::sync::Arc::new(Coalescer::new(3, std::time::Duration::from_millis(100)));
            let first = coalescer.submit(1).await.unwrap();
            assert_eq!(first.value, Some(1));
            assert_eq!(first.wait, std::time::Duration::ZERO);
            // Three more arrive while the first is being written.
            let mut waiting = Vec::new();
            for value in [2, 3, 4] {
                let c = coalescer.clone();
                waiting.push(tokio::spawn(async move {
                    let turn = c.submit(value).await;
                    turn.map(|t| {
                        let value = t.value;
                        let wait = t.wait;
                        if value.is_some() {
                            t.written();
                        }
                        (value, wait)
                    })
                }));
            }
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }
            // A fourth waiting is one too many.
            assert!(matches!(
                coalescer.submit(5).await.map(|t| t.value),
                Err(DaemonError::Busy)
            ));
            first.written();
            let mut written = Vec::new();
            for handle in waiting {
                let (value, wait) = handle.await.unwrap().unwrap();
                if let Some(value) = value {
                    written.push(value);
                    assert!(wait > std::time::Duration::from_millis(50), "{wait:?}");
                }
            }
            // Only the newest of the three was written.
            assert_eq!(written, vec![4]);
        });
    }
}
