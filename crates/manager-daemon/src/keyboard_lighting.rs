//! The keyboard's colour (ticket 136, ADR 0034): a HID LampArray through a
//! helper unit that can open exactly one hidraw node, or asus-wmi's
//! `kbd_rgb_mode` through ADR 0029's attribute helper.
//!
//! The daemon never opens a hidraw node itself (its unit has
//! `DevicePolicy=closed`). [`SandboxedLampHelper`] starts this program's
//! `--lamp-array NODE COMMAND` in a transient unit whose device cgroup allows
//! `/dev/NODE` and nothing else. The helper reads the report descriptor from
//! the opened node itself, refuses anything that is not a LampArray, and can
//! do three things: describe, set every lamp to one colour, give the lighting
//! back to the firmware. It never reads an input report and can build no
//! report but the five LampArray ones (`hardware_core::lamparray`).

use std::os::unix::fs::{FileTypeExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hardware_core::lamparray::{self, Description, FeatureDevice, Layout};
use hardware_core::lighting::{
    self, Effect, Interface, LightingChoice, LightingDevice, LightingReply, Rgb, StoredLighting,
    asus_wmi,
};
use hardware_platform::sysfs::{self, LampArrayNode, SysRoot};

use crate::DaemonError;
use crate::hardware::AttributeWriter;

/// What the helper is asked to do. Nothing the caller sent crosses as text:
/// the daemon builds this from checked values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LampCommand {
    Describe,
    Set { colour: Rgb, intensity: u8 },
    GiveBack,
}

impl LampCommand {
    /// The helper's arguments after the node.
    pub fn args(&self) -> Vec<String> {
        match self {
            LampCommand::Describe => vec!["describe".into()],
            LampCommand::Set { colour, intensity } => vec![
                "set".into(),
                format!("{:02x}{:02x}{:02x}", colour.red, colour.green, colour.blue),
                intensity.to_string(),
            ],
            LampCommand::GiveBack => vec!["give-back".into()],
        }
    }

    /// Exactly what [`LampCommand::args`] writes, and nothing else.
    pub fn parse(args: &[String]) -> Option<LampCommand> {
        match args {
            [d] if d == "describe" => Some(LampCommand::Describe),
            [g] if g == "give-back" => Some(LampCommand::GiveBack),
            [s, hex, text] if s == "set" => {
                if hex.len() != 6 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                    return None;
                }
                let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
                let intensity: u8 = text.parse().ok().filter(|i| *i <= 100)?;
                // Only the canonical spelling: no sign, no leading zero.
                if intensity.to_string() != *text {
                    return None;
                }
                Some(LampCommand::Set {
                    colour: Rgb::new(channel(0)?, channel(2)?, channel(4)?),
                    intensity,
                })
            }
            _ => None,
        }
    }
}

/// Runs one command against one LampArray node.
pub trait LampHelper: Send + Sync {
    /// The description for [`LampCommand::Describe`]; `None` otherwise.
    fn run(&self, node: &str, command: LampCommand) -> Result<Option<Description>, DaemonError>;
}

/// The account the helper runs as once the node is open.
pub const HELPER_UID: libc::uid_t = 65534;
pub const HELPER_GID: libc::gid_t = 65534;

/// The transient helper unit's properties: ADR 0029's shared helper
/// sandbox (`hardware::helper_sandbox`), a private `/dev` holding the
/// pseudo-devices and this one node, a device cgroup that allows only it,
/// and of all capabilities only the two that let the helper drop to
/// `nobody` once the node is open (`drop_to_nobody`).
pub fn lamp_helper_properties(node: &str) -> Vec<String> {
    let mut properties = crate::hardware::helper_sandbox();
    properties.extend([
        // `-`: a node gone since the daemon found it is the helper's own
        // clean "No such file" rather than a unit that cannot set up.
        format!("BindPaths=-/dev/{node}"),
        format!("DeviceAllow=/dev/{node} rw"),
        "CapabilityBoundingSet=CAP_SETUID CAP_SETGID".to_string(),
    ]);
    properties
}

/// Starts this program's `--lamp-array` in a transient unit and waits.
pub struct SandboxedLampHelper {
    pub program: PathBuf,
}

impl LampHelper for SandboxedLampHelper {
    fn run(&self, node: &str, command: LampCommand) -> Result<Option<Description>, DaemonError> {
        if !sysfs::is_hidraw_name(node) {
            return Err(DaemonError::InvalidArgument("node".to_string()));
        }
        let mut systemd_run = std::process::Command::new("systemd-run");
        systemd_run.args([
            "--quiet",
            "--wait",
            "--pipe",
            "--collect",
            "--service-type=exec",
            "--description=Better OS keyboard lighting",
        ]);
        for property in lamp_helper_properties(node) {
            systemd_run.arg("-p").arg(property);
        }
        systemd_run
            .arg(&self.program)
            .arg("--lamp-array")
            .arg(node)
            .args(command.args())
            .stdin(std::process::Stdio::null());
        let output = systemd_run
            .output()
            .map_err(|e| DaemonError::WriteFailed(format!("systemd-run: {e}")))?;
        if !output.status.success() {
            return Err(DaemonError::WriteFailed(format!(
                "/dev/{node}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        read_helper_output(command, &String::from_utf8_lossy(&output.stdout))
    }
}

/// What the helper printed, as the command expects it.
pub fn read_helper_output(
    command: LampCommand,
    stdout: &str,
) -> Result<Option<Description>, DaemonError> {
    match command {
        LampCommand::Describe => serde_json::from_str(stdout.trim())
            .map(Some)
            .map_err(|e| DaemonError::WriteFailed(format!("helper output: {e}"))),
        _ if stdout.trim() == "ok" => Ok(None),
        _ => Err(DaemonError::WriteFailed(format!(
            "helper output: {:?}",
            stdout.trim()
        ))),
    }
}

/// Runs a command on any [`FeatureDevice`] whose descriptor is `descriptor`:
/// the helper's own work, also what tests drive a fake through.
pub fn run_on(
    device: &mut dyn FeatureDevice,
    descriptor: &[u8],
    command: LampCommand,
) -> Result<Option<Description>, DaemonError> {
    let layout = Layout::from_descriptor(descriptor)
        .map_err(|e| DaemonError::InvalidArgument(format!("not a LampArray: {e}")))?;
    let failed = |e: lamparray::LampError| DaemonError::WriteFailed(e.to_string());
    match command {
        LampCommand::Describe => lamparray::describe(device, &layout)
            .map(Some)
            .map_err(failed),
        LampCommand::Set { colour, intensity } => {
            let description = lamparray::describe(device, &layout).map_err(failed)?;
            lamparray::set_all(device, &layout, &description, colour, intensity)
                .map(|_| None)
                .map_err(failed)
        }
        LampCommand::GiveBack => lamparray::give_back(device, &layout)
            .map(|_| None)
            .map_err(failed),
    }
}

/// hidraw's ioctls (`include/uapi/linux/hidraw.h`).
mod hidraw {
    use std::os::fd::AsRawFd as _;

    const IOC_WRITE: u64 = 1;
    const IOC_READ: u64 = 2;

    const fn ioc(dir: u64, nr: u64, size: usize) -> u64 {
        (dir << 30) | ((size as u64) << 16) | ((b'H' as u64) << 8) | nr
    }

    pub const HIDIOCGRDESCSIZE: u64 = ioc(IOC_READ, 0x01, 4);
    pub const HIDIOCGRDESC: u64 = ioc(IOC_READ, 0x02, 4 + 4096);
    pub const HIDIOCGRAWINFO: u64 = ioc(IOC_READ, 0x03, 8);

    pub const fn hidiocsfeature(len: usize) -> u64 {
        ioc(IOC_WRITE | IOC_READ, 0x06, len)
    }

    pub const fn hidiocgfeature(len: usize) -> u64 {
        ioc(IOC_WRITE | IOC_READ, 0x07, len)
    }

    #[repr(C)]
    struct ReportDescriptor {
        size: u32,
        value: [u8; 4096],
    }

    fn errno(what: &str) -> String {
        format!("{what}: {}", std::io::Error::last_os_error())
    }

    /// One opened hidraw node. Only feature reports are moved; nothing reads
    /// the node, so no input report (a key, a button) is ever seen.
    pub struct Hidraw(pub std::fs::File);

    impl Hidraw {
        /// The node's own report descriptor, from the kernel.
        pub fn descriptor(&self) -> Result<Vec<u8>, String> {
            let fd = self.0.as_raw_fd();
            let mut info = [0u8; 8];
            // SAFETY: the kernel writes 8 bytes into an 8-byte buffer.
            if unsafe { libc::ioctl(fd, HIDIOCGRAWINFO as _, info.as_mut_ptr()) } < 0 {
                return Err(errno("not a hidraw node"));
            }
            let mut size: libc::c_int = 0;
            // SAFETY: the kernel writes one int.
            if unsafe { libc::ioctl(fd, HIDIOCGRDESCSIZE as _, &mut size) } < 0 {
                return Err(errno("HIDIOCGRDESCSIZE"));
            }
            // The kernel refuses HIDIOCGRDESC for a size of
            // HID_MAX_DESCRIPTOR_SIZE (4096) or more (`hidraw_ioctl`).
            let size = usize::try_from(size)
                .ok()
                .filter(|s| (1..=4095).contains(s))
                .ok_or_else(|| format!("descriptor size {size}"))?;
            let mut descriptor = Box::new(ReportDescriptor {
                size: size as u32,
                value: [0; 4096],
            });
            // SAFETY: the struct is hidraw_report_descriptor's layout, and
            // the kernel writes at most `size` bytes into `value`.
            if unsafe { libc::ioctl(fd, HIDIOCGRDESC as _, &mut *descriptor) } < 0 {
                return Err(errno("HIDIOCGRDESC"));
            }
            Ok(descriptor.value[..size].to_vec())
        }
    }

    impl hardware_core::lamparray::FeatureDevice for Hidraw {
        fn get_feature(&mut self, report: &mut [u8]) -> Result<usize, String> {
            if report.is_empty() || report.len() > 4096 {
                return Err("feature report size".into());
            }
            // SAFETY: the kernel writes at most `report.len()` bytes.
            let n = unsafe {
                libc::ioctl(
                    self.0.as_raw_fd(),
                    hidiocgfeature(report.len()) as _,
                    report.as_mut_ptr(),
                )
            };
            usize::try_from(n).map_err(|_| errno("HIDIOCGFEATURE"))
        }

        fn set_feature(&mut self, report: &[u8]) -> Result<(), String> {
            if report.is_empty() || report.len() > 4096 {
                return Err("feature report size".into());
            }
            // SAFETY: the kernel reads `report.len()` bytes.
            let n = unsafe {
                libc::ioctl(
                    self.0.as_raw_fd(),
                    hidiocsfeature(report.len()) as _,
                    report.as_ptr(),
                )
            };
            if n < 0 {
                Err(errno("HIDIOCSFEATURE"))
            } else {
                Ok(())
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The numbers `include/uapi/linux/hidraw.h` gives on x86-64 and
        /// arm64, which share the generic `_IOC` layout.
        #[test]
        fn the_ioctl_numbers_are_the_kernels() {
            assert_eq!(HIDIOCGRDESCSIZE, 0x8004_4801);
            assert_eq!(HIDIOCGRDESC, 0x9004_4802);
            assert_eq!(HIDIOCGRAWINFO, 0x8008_4803);
            assert_eq!(hidiocsfeature(10), 0xC00A_4806);
            assert_eq!(hidiocgfeature(23), 0xC017_4807);
        }
    }
}

/// Gives up root for good: no supplementary groups, then `nobody`'s group
/// and user, real, effective, and saved. Root's capabilities go with the
/// user ID (no `PR_SET_KEEPCAPS`). Fails closed: a helper that is still
/// root afterwards stops.
fn drop_to_nobody() -> Result<(), DaemonError> {
    let failed = |what: &str| {
        DaemonError::WriteFailed(format!("{what}: {}", std::io::Error::last_os_error()))
    };
    // SAFETY: plain system calls with no pointers but a null list of zero
    // groups.
    unsafe {
        if libc::setgroups(0, std::ptr::null()) != 0 {
            return Err(failed("setgroups"));
        }
        if libc::setresgid(HELPER_GID, HELPER_GID, HELPER_GID) != 0 {
            return Err(failed("setresgid"));
        }
        if libc::setresuid(HELPER_UID, HELPER_UID, HELPER_UID) != 0 {
            return Err(failed("setresuid"));
        }
        if libc::getuid() != HELPER_UID || libc::geteuid() != HELPER_UID || libc::setuid(0) == 0 {
            return Err(DaemonError::WriteFailed("still root".to_string()));
        }
    }
    Ok(())
}

/// `--lamp-array NODE COMMAND...`, inside the helper's sandbox: opens
/// `/dev/NODE`, reads its descriptor from the kernel, and runs the command.
/// Prints a description as JSON, or `ok`.
pub fn lamp_helper_main(node: &str, args: &[String]) -> Result<String, DaemonError> {
    if !sysfs::is_hidraw_name(node) {
        return Err(DaemonError::InvalidArgument("node".to_string()));
    }
    let command = LampCommand::parse(args)
        .ok_or_else(|| DaemonError::InvalidArgument("lamp command".to_string()))?;
    let path = Path::new("/dev").join(node);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NOCTTY)
        .open(&path)
        .map_err(|e| DaemonError::WriteFailed(format!("{}: {e}", path.display())))?;
    let metadata = file
        .metadata()
        .map_err(|e| DaemonError::WriteFailed(format!("{}: {e}", path.display())))?;
    if !metadata.file_type().is_char_device() {
        return Err(DaemonError::InvalidArgument(format!(
            "{} is not a device",
            path.display()
        )));
    }
    let mut device = hidraw::Hidraw(file);
    let descriptor = device
        .descriptor()
        .map_err(|e| DaemonError::WriteFailed(format!("{}: {e}", path.display())))?;
    // Root was needed to open the node, and nothing after. Everything the
    // device answers is parsed as `nobody`, with no capability.
    drop_to_nobody()?;
    match run_on(&mut device, &descriptor, command)? {
        Some(description) => {
            serde_json::to_string(&description).map_err(|e| DaemonError::WriteFailed(e.to_string()))
        }
        None => Ok("ok".to_string()),
    }
}

/// Descriptions already read, by node and descriptor: a LampArray's
/// attributes do not change while it is plugged in.
#[derive(Default)]
pub struct LampCache(Mutex<Vec<(String, Vec<u8>, Description)>>);

/// The keyboard the System page sets (ADR 0034): the laptop's own, never
/// one plugged in by USB or Bluetooth.
#[derive(Clone, Debug, PartialEq)]
pub enum Keyboard {
    /// A built-in (I2C) LampArray that says it is a keyboard, or says
    /// nothing.
    LampArray {
        node: LampArrayNode,
        description: Description,
    },
    /// asus-wmi's `kbd_rgb_mode`, at its real path under `/sys/devices`.
    AsusWmi(PathBuf),
    /// A built-in LampArray that says it is something else (chassis,
    /// peripheral, ...). Reported as found; never driven.
    NotAKeyboard {
        node: LampArrayNode,
        description: Description,
    },
}

impl Keyboard {
    pub fn device(&self) -> LightingDevice {
        match self {
            Keyboard::LampArray { node, .. } | Keyboard::NotAKeyboard { node, .. } => node.device(),
            Keyboard::AsusWmi(_) => LightingDevice::ASUS_WMI,
        }
    }
}

/// The journal line a LampArray's description gets the first time the
/// daemon reads it: what the owner's check wants to know, with no root
/// command (lamp count, kind, levels).
pub fn description_line(node: &LampArrayNode, description: &Description) -> String {
    let l = description.levels;
    format!(
        "keyboard lighting: /dev/{} ({}): {} lamps, kind {}, levels red {} green {} blue {} intensity {}, programmable {}, update interval {}",
        node.node,
        node.device().label(),
        description.lamps,
        description
            .kind
            .map(|k| format!("{k:?}"))
            .unwrap_or_else(|| "not reported".to_string()),
        l.red,
        l.green,
        l.blue,
        l.intensity,
        if description.programmable {
            "yes"
        } else {
            "no"
        },
        description
            .min_update_interval_us
            .map(|us| format!("{us} µs"))
            .unwrap_or_else(|| "not reported".to_string()),
    )
}

/// The built-in keyboard: a built-in LampArray that is a keyboard first,
/// then `kbd_rgb_mode`, then a built-in lighting device that is not a
/// keyboard (so the page can say what it found). External LampArrays are
/// never read.
pub fn find_keyboard(
    root: &SysRoot,
    helper: &dyn LampHelper,
    cache: &LampCache,
) -> Result<Option<Keyboard>, DaemonError> {
    let mut last_error = None;
    let mut not_keyboard = None;
    for candidate in sysfs::built_in_lamp_arrays(root) {
        let Some(descriptor) = sysfs::report_descriptor(root, &candidate.node) else {
            continue;
        };
        let cached = cache
            .0
            .lock()
            .map_err(|_| DaemonError::WriteFailed("lamp cache".into()))?
            .iter()
            .find(|(node, d, _)| *node == candidate.node && *d == descriptor)
            .map(|(_, _, description)| description.clone());
        let description = match cached {
            Some(description) => description,
            None => match helper.run(&candidate.node, LampCommand::Describe) {
                Ok(Some(description)) => {
                    eprintln!(
                        "better-manager-daemon: {}",
                        description_line(&candidate, &description)
                    );
                    let mut entries = cache
                        .0
                        .lock()
                        .map_err(|_| DaemonError::WriteFailed("lamp cache".into()))?;
                    entries.retain(|(node, _, _)| *node != candidate.node);
                    entries.push((candidate.node.clone(), descriptor, description.clone()));
                    description
                }
                Ok(None) => continue,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            },
        };
        if lamparray::is_keyboard_kind(description.kind) {
            return Ok(Some(Keyboard::LampArray {
                node: candidate,
                description,
            }));
        }
        not_keyboard.get_or_insert(Keyboard::NotAKeyboard {
            node: candidate,
            description,
        });
    }
    if let Some(path) = sysfs::keyboard_rgb_mode_attribute(root) {
        return Ok(Some(Keyboard::AsusWmi(real(path))));
    }
    if let Some(found) = not_keyboard {
        return Ok(Some(found));
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

/// Where the daemon keeps the keyboard's colour.
#[derive(Clone, Debug)]
pub struct LightingPaths {
    /// The choice and its keyboard, for the boot-time restore
    /// (`/var/lib/better-os/keyboard-lighting.json`).
    pub state: PathBuf,
    /// Present while the keyboard's own lighting was chosen on
    /// `kbd_rgb_mode` and the PC has not started again
    /// (`/run/better-os/keyboard-lighting-firmware`); `/run` is emptied at
    /// boot, which is exactly when the firmware's own setting returns.
    pub firmware_marker: PathBuf,
}

impl LightingPaths {
    pub fn system() -> LightingPaths {
        LightingPaths {
            state: PathBuf::from(crate::LIGHTING_STATE),
            firmware_marker: PathBuf::from(crate::LIGHTING_FIRMWARE_MARKER),
        }
    }

    /// Both under one directory, for tests.
    pub fn under(dir: &Path) -> LightingPaths {
        LightingPaths {
            state: dir.join("keyboard-lighting.json"),
            firmware_marker: dir.join("keyboard-lighting-firmware"),
        }
    }
}

/// The stored choice, or `None` when Better OS has set nothing. A file that
/// is empty or does not parse (power lost before an older write reached the
/// disk, say) is nothing saved, with a journal line, never a failure that
/// would fail the boot-time restore every time.
pub fn load(state: &Path) -> Result<Option<StoredLighting>, DaemonError> {
    match std::fs::read_to_string(state) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(stored) => Ok(Some(stored)),
            Err(error) => {
                eprintln!(
                    "better-manager-daemon: keyboard lighting: {} is not a saved choice ({error}); nothing is restored",
                    state.display()
                );
                Ok(None)
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(DaemonError::Storage(error.to_string())),
    }
}

fn remove(path: &Path) -> Result<(), DaemonError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DaemonError::Storage(error.to_string())),
    }
}

/// Writes `text` to `path` whole or not at all: a temporary file, synced,
/// renamed over the old one, and the directory synced, so a power loss
/// leaves the old file or the new one and never an empty one.
fn write_durably(path: &Path, text: &str) -> Result<(), DaemonError> {
    use std::io::Write as _;
    let storage = |e: std::io::Error| DaemonError::Storage(e.to_string());
    let parent = path
        .parent()
        .ok_or_else(|| DaemonError::Storage("no directory".into()))?;
    std::fs::create_dir_all(parent).map_err(storage)?;
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(storage)?;
    file.write_all(text.as_bytes()).map_err(storage)?;
    file.sync_all().map_err(storage)?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(storage)?;
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(storage)
}

/// Keeps a choice for the boot-time restore, with the keyboard it is for.
/// The keyboard's own lighting is no choice at all: the file goes, so
/// nothing is written at boot; on `kbd_rgb_mode` the marker says the
/// firmware's own setting returns at the next start.
pub fn store(
    paths: &LightingPaths,
    device: LightingDevice,
    choice: &LightingChoice,
) -> Result<(), DaemonError> {
    if choice.effect == Effect::Firmware {
        remove(&paths.state)?;
        if device.interface == Interface::AsusWmi {
            write_durably(&paths.firmware_marker, "")?;
        }
        return Ok(());
    }
    remove(&paths.firmware_marker)?;
    let stored = StoredLighting {
        device,
        choice: *choice,
    };
    let text = serde_json::to_string(&stored).map_err(|e| DaemonError::Storage(e.to_string()))?;
    write_durably(&paths.state, &text)
}

/// What the machine offers and what Better OS set on that keyboard.
pub fn describe(
    root: &SysRoot,
    helper: &dyn LampHelper,
    cache: &LampCache,
    paths: &LightingPaths,
) -> Result<LightingReply, DaemonError> {
    let keyboard = find_keyboard(root, helper, cache)?.ok_or(DaemonError::HardwareAbsent)?;
    let device = keyboard.device();
    let stored = load(&paths.state)?
        .filter(|stored| stored.device == device)
        .map(|stored| stored.choice);
    Ok(match keyboard {
        Keyboard::LampArray { description, .. } | Keyboard::NotAKeyboard { description, .. } => {
            LightingReply {
                interface: Interface::LampArray,
                device,
                lamp_array: Some(description),
                stored,
                firmware_at_next_start: false,
            }
        }
        Keyboard::AsusWmi(_) => LightingReply {
            interface: Interface::AsusWmi,
            device,
            lamp_array: None,
            stored,
            firmware_at_next_start: paths.firmware_marker.exists(),
        },
    })
}

/// The attribute's real path, the class link resolved to the device.
fn real(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Shows `choice` on `keyboard`. A choice the interface does not take
/// touches nothing. The keyboard's own lighting on `kbd_rgb_mode` writes
/// nothing: there is no write that brings the firmware's saved setting back
/// before the next start (cmd 0 never saved over it).
pub fn apply_to(
    keyboard: &Keyboard,
    helper: &dyn LampHelper,
    writer: &dyn AttributeWriter,
    choice: &LightingChoice,
) -> Result<LightingDevice, DaemonError> {
    match keyboard {
        Keyboard::LampArray { node, .. } => {
            if !lighting::accepts(Interface::LampArray, choice) {
                return Err(DaemonError::InvalidArgument("effect".to_string()));
            }
            let command = match choice.effect {
                Effect::Firmware => LampCommand::GiveBack,
                _ => LampCommand::Set {
                    colour: choice.colour,
                    intensity: choice.intensity,
                },
            };
            helper.run(&node.node, command)?;
        }
        Keyboard::AsusWmi(path) => {
            if !lighting::accepts(Interface::AsusWmi, choice) {
                return Err(DaemonError::InvalidArgument("effect".to_string()));
            }
            if let Some(value) = asus_wmi::value(choice) {
                writer.write(path, &value)?;
            }
        }
        Keyboard::NotAKeyboard { .. } => return Err(DaemonError::HardwareAbsent),
    }
    Ok(keyboard.device())
}

/// Finds the keyboard and shows `choice` on it.
pub fn apply(
    root: &SysRoot,
    helper: &dyn LampHelper,
    writer: &dyn AttributeWriter,
    cache: &LampCache,
    choice: &LightingChoice,
) -> Result<LightingDevice, DaemonError> {
    let keyboard = find_keyboard(root, helper, cache)?.ok_or(DaemonError::HardwareAbsent)?;
    apply_to(&keyboard, helper, writer, choice)
}

/// What `--restore-hardware` did for the keyboard.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoredLighting {
    NothingStored,
    Written(LightingDevice),
    /// The keyboard the choice was made for is not here (yet): an I2C
    /// keyboard can bind late, so the restore asks again for a while, then
    /// skips with a journal line. Never a failed unit.
    DeviceAbsent(LightingDevice),
    /// The choice no longer fits its keyboard; skipped with a journal line.
    Skipped(String),
}

/// Shows the stored choice again at boot, on the keyboard it was made for
/// and no other.
pub fn restore(
    root: &SysRoot,
    helper: &dyn LampHelper,
    writer: &dyn AttributeWriter,
    state: &Path,
) -> Result<RestoredLighting, DaemonError> {
    let Some(stored) = load(state)? else {
        return Ok(RestoredLighting::NothingStored);
    };
    let keyboard = match find_keyboard(root, helper, &LampCache::default())? {
        Some(keyboard) if keyboard.device() == stored.device => keyboard,
        _ => return Ok(RestoredLighting::DeviceAbsent(stored.device)),
    };
    match apply_to(&keyboard, helper, writer, &stored.choice) {
        Ok(device) => Ok(RestoredLighting::Written(device)),
        Err(DaemonError::InvalidArgument(_)) | Err(DaemonError::HardwareAbsent) => {
            Ok(RestoredLighting::Skipped(format!(
                "the saved {} does not fit {}",
                stored.choice.effect.wire(),
                stored.device.label()
            )))
        }
        Err(error) => Err(error),
    }
}

/// Gives the keyboard back to its firmware and forgets the choice: the
/// daemon package's prerm on removal, and Better Manager's own prerm (the
/// page that set it is going). A LampArray Better OS set is put back in
/// autonomous mode; `kbd_rgb_mode` needs no write, because Better OS never
/// saved to the firmware, whose own setting returns at the next start.
/// Nothing is touched when nothing was set. Returns what could not be done;
/// it never fails a removal.
pub fn release(root: &SysRoot, helper: &dyn LampHelper, paths: &LightingPaths) -> Vec<String> {
    let mut problems = Vec::new();
    match load(&paths.state) {
        Ok(None) => {}
        Ok(Some(stored)) => match find_keyboard(root, helper, &LampCache::default()) {
            Ok(Some(Keyboard::LampArray { node, .. })) if node.device() == stored.device => {
                if let Err(error) = helper.run(&node.node, LampCommand::GiveBack) {
                    problems.push(format!("/dev/{}: {error}", node.node));
                }
            }
            Ok(_) => {}
            Err(error) => problems.push(error.to_string()),
        },
        Err(error) => problems.push(error.to_string()),
    }
    for path in [&paths.state, &paths.firmware_marker] {
        if let Err(error) = remove(path) {
            problems.push(format!("{}: {error}", path.display()));
        }
    }
    problems
}

/// A helper that drives in-memory LampArrays: tests and sandbox stand-ins.
/// Never in the shipped daemon. `device` is the one at `node`; `others`
/// are more nodes, each its own LampArray.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeLampHelper {
    pub node: String,
    pub descriptor: Vec<u8>,
    pub device: Mutex<lamparray::fake::FakeLampArray>,
    pub others: Mutex<Vec<(String, lamparray::fake::FakeLampArray)>>,
    /// How many times the helper ran.
    pub runs: std::sync::atomic::AtomicUsize,
}

#[cfg(any(test, feature = "test-support"))]
impl FakeLampHelper {
    pub fn new(node: &str, descriptor: &[u8]) -> FakeLampHelper {
        FakeLampHelper {
            node: node.to_string(),
            descriptor: descriptor.to_vec(),
            device: Mutex::new(lamparray::fake::FakeLampArray::one_zone(descriptor)),
            others: Mutex::new(Vec::new()),
            runs: Default::default(),
        }
    }

    /// Another LampArray at `node`, of `kind`.
    pub fn add(&self, node: &str, kind: u32) {
        let mut device = lamparray::fake::FakeLampArray::one_zone(&self.descriptor);
        device.kind = kind;
        self.others.lock().unwrap().push((node.to_string(), device));
    }

    pub fn runs(&self) -> usize {
        self.runs.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl LampHelper for FakeLampHelper {
    fn run(&self, node: &str, command: LampCommand) -> Result<Option<Description>, DaemonError> {
        self.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // The helper is a separate process: it sees only its argument list.
        let parsed = LampCommand::parse(&command.args())
            .ok_or_else(|| DaemonError::InvalidArgument("lamp command".into()))?;
        if node == self.node {
            let mut device = self.device.lock().unwrap();
            return run_on(&mut *device, &self.descriptor, parsed);
        }
        let mut others = self.others.lock().unwrap();
        match others.iter_mut().find(|(n, _)| n == node) {
            Some((_, device)) => run_on(device, &self.descriptor, parsed),
            None => Err(DaemonError::WriteFailed(format!("/dev/{node}: ENOENT"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::DirectWriter;
    use hardware_core::lighting::Speed;

    const M5406WA: &[u8] =
        include_bytes!("../../hardware-core/tests/fixtures/m5406wa-ite5570.rdesc");

    const OWNERS: LightingDevice = LightingDevice {
        interface: Interface::LampArray,
        bus: Some(0x18),
        vendor: Some(0x0B05),
        product: Some(0x5570),
    };

    /// A hidraw node with the real descriptor, on `bus`.
    fn lamp_node(dir: &Path, node: &str, bus: &str, product: &str) {
        let device = dir.join("sys/class/hidraw").join(node).join("device");
        std::fs::create_dir_all(&device).unwrap();
        std::fs::write(device.join("report_descriptor"), M5406WA).unwrap();
        std::fs::write(
            device.join("uevent"),
            format!("HID_ID={bus}:00000B05:0000{product}\n"),
        )
        .unwrap();
    }

    fn backlight(dir: &Path, rgb: bool) -> PathBuf {
        let led = dir.join("sys/class/leds/asus::kbd_backlight");
        std::fs::create_dir_all(&led).unwrap();
        std::fs::write(led.join("max_brightness"), "3\n").unwrap();
        let attribute = led.join("kbd_rgb_mode");
        if rgb {
            std::fs::write(&attribute, "").unwrap();
        }
        attribute
    }

    /// The owner's machine: the I2C LampArray at hidraw1 and a backlight.
    fn owners(dir: &Path) {
        lamp_node(dir, "hidraw1", "0018", "5570");
        backlight(dir, false);
    }

    fn red() -> LightingChoice {
        LightingChoice {
            effect: Effect::Static,
            colour: Rgb::new(255, 0, 0),
            intensity: 100,
            speed: Speed::Normal,
        }
    }

    #[test]
    fn the_helpers_arguments_are_a_closed_set() {
        for command in [
            LampCommand::Describe,
            LampCommand::GiveBack,
            LampCommand::Set {
                colour: Rgb::new(255, 0, 10),
                intensity: 100,
            },
            LampCommand::Set {
                colour: Rgb::new(0, 0, 0),
                intensity: 0,
            },
        ] {
            assert_eq!(LampCommand::parse(&command.args()), Some(command));
        }
        let refuse = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            assert_eq!(LampCommand::parse(&args), None, "{args:?}");
        };
        refuse(&[]);
        refuse(&["write", "5f00"]);
        refuse(&["set", "FF0000", "100"]);
        refuse(&["set", "ff00", "100"]);
        refuse(&["set", "ff0000", "101"]);
        refuse(&["set", "ff0000", "050"]);
        refuse(&["set", "ff0000", "+5"]);
        refuse(&["set", "ff0000"]);
        refuse(&["describe", "x"]);
        refuse(&["give-back", "now"]);
    }

    /// The review's finding 1: the helper's sandbox, asserted property by
    /// property. Untested on a real system manager (see ADR 0034).
    #[test]
    fn the_lamp_helper_sees_one_node_and_drops_to_nobody() {
        let properties = lamp_helper_properties("hidraw1");
        let has = |p: &str| properties.iter().any(|q| q == p);
        // The shared sandbox, in full.
        for shared in crate::hardware::helper_sandbox() {
            assert!(has(&shared), "{shared}");
        }
        // /dev is private: the pseudo-devices and this node, bound in, and
        // the device cgroup allows only it.
        assert!(has("PrivateDevices=yes"));
        assert!(has("BindPaths=-/dev/hidraw1"));
        assert_eq!(
            properties
                .iter()
                .filter(|p| p.starts_with("DeviceAllow=") || p.starts_with("BindPaths="))
                .collect::<Vec<_>>(),
            vec!["BindPaths=-/dev/hidraw1", "DeviceAllow=/dev/hidraw1 rw"]
        );
        // Only the two capabilities the drop to nobody needs.
        assert_eq!(
            properties
                .iter()
                .filter(|p| p.starts_with("CapabilityBoundingSet="))
                .collect::<Vec<_>>(),
            vec!["CapabilityBoundingSet=CAP_SETUID CAP_SETGID"]
        );
        assert_eq!((HELPER_UID, HELPER_GID), (65534, 65534));
    }

    #[test]
    fn a_name_that_is_not_a_node_never_reaches_systemd_run() {
        let helper = SandboxedLampHelper {
            program: PathBuf::from("/nonexistent"),
        };
        for node in ["../sda", "hidraw1 rw", "sda", "hidraw", "hidraw1/../../sda"] {
            assert_eq!(
                helper.run(node, LampCommand::Describe),
                Err(DaemonError::InvalidArgument("node".into())),
                "{node}"
            );
            assert!(matches!(
                lamp_helper_main(node, &["describe".to_string()]),
                Err(DaemonError::InvalidArgument(_))
            ));
        }
        assert!(matches!(
            lamp_helper_main("hidraw1", &["write".to_string()]),
            Err(DaemonError::InvalidArgument(_))
        ));
    }

    #[test]
    fn the_owners_keyboard_is_found_set_stored_restored_and_released() {
        let dir = tempfile::tempdir().unwrap();
        owners(dir.path());
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw1", M5406WA);
        let cache = LampCache::default();
        let paths = LightingPaths::under(&dir.path().join("var"));

        let reply = describe(&root, &helper, &cache, &paths).unwrap();
        assert_eq!(reply.interface, Interface::LampArray);
        assert_eq!(reply.device, OWNERS);
        assert_eq!(reply.lamp_array.as_ref().unwrap().lamps, 1);
        // The description is read once.
        describe(&root, &helper, &cache, &paths).unwrap();
        assert_eq!(helper.runs(), 1);

        assert_eq!(
            apply(&root, &helper, &DirectWriter, &cache, &red()),
            Ok(OWNERS)
        );
        assert_eq!(helper.device.lock().unwrap().shown, Some([255, 0, 0, 255]));
        assert!(!helper.device.lock().unwrap().autonomous);
        store(&paths, OWNERS, &red()).unwrap();
        assert_eq!(
            load(&paths.state).unwrap(),
            Some(StoredLighting {
                device: OWNERS,
                choice: red()
            })
        );
        assert_eq!(
            describe(&root, &helper, &cache, &paths).unwrap().stored,
            Some(red())
        );

        // Boot: the firmware is back in control; the restore takes it again.
        helper.device.lock().unwrap().autonomous = true;
        assert_eq!(
            restore(&root, &helper, &DirectWriter, &paths.state),
            Ok(RestoredLighting::Written(OWNERS))
        );
        assert!(!helper.device.lock().unwrap().autonomous);

        // Removal gives the keyboard back and forgets the choice.
        assert!(release(&root, &helper, &paths).is_empty());
        assert!(helper.device.lock().unwrap().autonomous);
        assert!(!paths.state.exists());

        // The keyboard's own default stores nothing.
        store(&paths, OWNERS, &red()).unwrap();
        store(&paths, OWNERS, &LightingChoice::starting(Effect::Firmware)).unwrap();
        assert!(!paths.state.exists());
        assert!(!paths.firmware_marker.exists(), "no marker on a LampArray");
        assert_eq!(
            restore(&root, &helper, &DirectWriter, &paths.state),
            Ok(RestoredLighting::NothingStored)
        );
        let before = helper.runs();
        assert!(release(&root, &helper, &paths).is_empty());
        // With nothing stored, removal never touches the keyboard.
        assert_eq!(helper.runs(), before);
    }

    #[test]
    fn an_effect_a_lamp_array_cannot_do_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        owners(dir.path());
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw1", M5406WA);
        let cache = LampCache::default();
        for effect in [Effect::Breathing, Effect::Cycle, Effect::Strobe] {
            assert_eq!(
                apply(
                    &root,
                    &helper,
                    &DirectWriter,
                    &cache,
                    &LightingChoice::starting(effect)
                ),
                Err(DaemonError::InvalidArgument("effect".into()))
            );
        }
        let device = helper.device.lock().unwrap();
        // Only the description was read; nothing was set.
        assert!(device.sent.iter().all(|r| r[0] == 0x42));
        assert!(device.autonomous);
    }

    /// The review's finding 3: a USB Dynamic Lighting keyboard is never
    /// the keyboard, beside a built-in one or alone.
    #[test]
    fn an_external_lamp_array_is_never_the_keyboard() {
        // A TUF laptop with a USB LampArray keyboard plugged in: the card
        // is for kbd_rgb_mode, and the USB keyboard is not even read.
        let dir = tempfile::tempdir().unwrap();
        lamp_node(dir.path(), "hidraw0", "0003", "1234");
        let attribute = backlight(dir.path(), true);
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw0", M5406WA);
        let cache = LampCache::default();
        let reply = describe(&root, &helper, &cache, &LightingPaths::under(dir.path())).unwrap();
        assert_eq!(reply.interface, Interface::AsusWmi);
        assert_eq!(reply.device, LightingDevice::ASUS_WMI);
        assert_eq!(helper.runs(), 0, "the USB keyboard is never opened");
        let breathing = LightingChoice {
            effect: Effect::Breathing,
            ..red()
        };
        assert_eq!(
            apply(&root, &helper, &DirectWriter, &cache, &breathing),
            Ok(LightingDevice::ASUS_WMI)
        );
        assert_eq!(
            std::fs::read_to_string(&attribute).unwrap(),
            "0 1 255 0 0 1"
        );

        // The M5406WA with a USB LampArray keyboard that sorts first: the
        // built-in one is still the one set.
        let dir = tempfile::tempdir().unwrap();
        lamp_node(dir.path(), "hidraw0", "0003", "1234");
        owners(dir.path());
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw1", M5406WA);
        helper.add("hidraw0", 1);
        assert_eq!(
            apply(&root, &helper, &DirectWriter, &LampCache::default(), &red()),
            Ok(OWNERS)
        );
        assert!(helper.others.lock().unwrap()[0].1.sent.is_empty());

        // A USB LampArray alone: no keyboard at all.
        let dir = tempfile::tempdir().unwrap();
        lamp_node(dir.path(), "hidraw0", "0003", "1234");
        let helper = FakeLampHelper::new("hidraw0", M5406WA);
        assert_eq!(
            describe(
                &SysRoot::at(dir.path()),
                &helper,
                &LampCache::default(),
                &LightingPaths::under(dir.path())
            ),
            Err(DaemonError::HardwareAbsent)
        );
    }

    /// The review's finding 4: a built-in lighting device that says it is
    /// not a keyboard is described for what it is and never driven.
    #[test]
    fn a_built_in_lamp_array_that_is_not_a_keyboard_is_described_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        owners(dir.path());
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw1", M5406WA);
        // Chassis.
        helper.device.lock().unwrap().kind = 7;
        let reply = describe(
            &root,
            &helper,
            &LampCache::default(),
            &LightingPaths::under(dir.path()),
        )
        .unwrap();
        assert_eq!(
            reply.lamp_array.unwrap().kind,
            Some(lamparray::LampArrayKind::Chassis)
        );
        assert_eq!(
            apply(&root, &helper, &DirectWriter, &LampCache::default(), &red()),
            Err(DaemonError::HardwareAbsent)
        );
        assert!(helper.device.lock().unwrap().autonomous);
        // Undefined, on the built-in bus, is taken as the keyboard.
        helper.device.lock().unwrap().kind = 0;
        assert_eq!(
            apply(&root, &helper, &DirectWriter, &LampCache::default(), &red()),
            Ok(OWNERS)
        );
    }

    /// The review's finding 3: a saved choice goes back to its own keyboard
    /// only, and a missing keyboard is a skip, not a failed unit.
    #[test]
    fn a_restore_writes_only_to_the_keyboard_it_was_saved_for() {
        let dir = tempfile::tempdir().unwrap();
        let paths = LightingPaths::under(&dir.path().join("var"));
        // Breathing saved on a TUF keyboard...
        let breathing = LightingChoice {
            effect: Effect::Breathing,
            ..red()
        };
        store(&paths, LightingDevice::ASUS_WMI, &breathing).unwrap();
        // ...restored on a machine whose keyboard is a LampArray.
        owners(dir.path());
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw1", M5406WA);
        assert_eq!(
            restore(&root, &helper, &DirectWriter, &paths.state),
            Ok(RestoredLighting::DeviceAbsent(LightingDevice::ASUS_WMI))
        );
        assert!(
            helper
                .device
                .lock()
                .unwrap()
                .sent
                .iter()
                .all(|r| r[0] == 0x42)
        );
        // Saved on the LampArray, which is not there at boot (yet).
        store(&paths, OWNERS, &red()).unwrap();
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            restore(
                &SysRoot::at(empty.path()),
                &helper,
                &DirectWriter,
                &paths.state
            ),
            Ok(RestoredLighting::DeviceAbsent(OWNERS))
        );
        // An empty or broken file (power lost mid-write) is nothing saved.
        std::fs::write(&paths.state, "").unwrap();
        assert_eq!(
            restore(&root, &helper, &DirectWriter, &paths.state),
            Ok(RestoredLighting::NothingStored)
        );
        std::fs::write(&paths.state, r#"{"effect":"static","report":"5f00"}"#).unwrap();
        assert_eq!(load(&paths.state), Ok(None));
    }

    /// The review's finding 2: on kbd_rgb_mode the keyboard's own lighting
    /// is the way back, in one action.
    #[test]
    fn the_keyboards_own_lighting_on_kbd_rgb_mode_forgets_the_choice() {
        let dir = tempfile::tempdir().unwrap();
        let attribute = backlight(dir.path(), true);
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw9", M5406WA);
        let cache = LampCache::default();
        let paths = LightingPaths::under(&dir.path().join("var"));
        let strobe = LightingChoice {
            effect: Effect::Strobe,
            ..red()
        };
        apply(&root, &helper, &DirectWriter, &cache, &strobe).unwrap();
        store(&paths, LightingDevice::ASUS_WMI, &strobe).unwrap();
        let firmware = LightingChoice::starting(Effect::Firmware);
        assert_eq!(
            apply(&root, &helper, &DirectWriter, &cache, &firmware),
            Ok(LightingDevice::ASUS_WMI)
        );
        // No write: the strobe stays until the next start.
        assert_eq!(
            std::fs::read_to_string(&attribute).unwrap(),
            "0 10 255 0 0 1"
        );
        store(&paths, LightingDevice::ASUS_WMI, &firmware).unwrap();
        assert!(!paths.state.exists(), "nothing is restored at boot");
        let reply = describe(&root, &helper, &cache, &paths).unwrap();
        assert_eq!(reply.stored, None);
        assert!(reply.firmware_at_next_start);
        assert_eq!(
            restore(&root, &helper, &DirectWriter, &paths.state),
            Ok(RestoredLighting::NothingStored)
        );
        // A colour set again clears the marker.
        store(&paths, LightingDevice::ASUS_WMI, &strobe).unwrap();
        assert!(!paths.firmware_marker.exists());
    }

    #[test]
    fn the_saved_choice_is_written_whole() {
        let dir = tempfile::tempdir().unwrap();
        let paths = LightingPaths::under(&dir.path().join("deep/var"));
        store(&paths, OWNERS, &red()).unwrap();
        assert!(!paths.state.with_extension("json.tmp").exists());
        let text = std::fs::read_to_string(&paths.state).unwrap();
        assert!(serde_json::from_str::<StoredLighting>(&text).is_ok());
    }

    #[test]
    fn the_journal_line_names_what_the_owners_check_wants() {
        let node = LampArrayNode {
            node: "hidraw1".into(),
            bus: Some(0x18),
            vendor: Some(0x0B05),
            product: Some(0x5570),
        };
        let mut device = lamparray::fake::FakeLampArray::one_zone(M5406WA);
        let layout = device.layout.clone();
        let description = lamparray::describe(&mut device, &layout).unwrap();
        let line = description_line(&node, &description);
        for part in [
            "/dev/hidraw1",
            "0018:0B05:5570",
            "1 lamps",
            "kind Keyboard",
            "levels red 255 green 255 blue 255 intensity 255",
            "programmable yes",
        ] {
            assert!(line.contains(part), "{part}: {line}");
        }
    }

    #[test]
    fn helper_output_is_read_strictly() {
        assert_eq!(read_helper_output(LampCommand::GiveBack, "ok\n"), Ok(None));
        assert!(read_helper_output(LampCommand::GiveBack, "done").is_err());
        assert!(read_helper_output(LampCommand::Describe, "{}").is_err());
    }
}
