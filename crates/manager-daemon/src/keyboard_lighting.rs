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

use hardware_core::lamparray::{self, Description, FeatureDevice, LampArrayKind, Layout};
use hardware_core::lighting::{
    self, Effect, Interface, LightingChoice, LightingReply, Rgb, asus_wmi,
};
use hardware_platform::sysfs::{self, SysRoot};

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

/// The transient helper unit's properties: the device cgroup allows the one
/// node; `/sys` is an empty read-only tmpfs; no capabilities, no network,
/// nothing writable.
pub fn lamp_helper_properties(node: &str) -> Vec<String> {
    vec![
        "DevicePolicy=closed".to_string(),
        format!("DeviceAllow=/dev/{node} rw"),
        "TemporaryFileSystem=/sys:ro".to_string(),
        "MountAPIVFS=no".to_string(),
        "ProtectSystem=strict".to_string(),
        "ProtectHome=yes".to_string(),
        "PrivateTmp=yes".to_string(),
        "PrivateNetwork=yes".to_string(),
        "NoNewPrivileges=yes".to_string(),
        "CapabilityBoundingSet=".to_string(),
        "RestrictAddressFamilies=AF_UNIX".to_string(),
        "RestrictNamespaces=yes".to_string(),
        "LockPersonality=yes".to_string(),
        "SystemCallArchitectures=native".to_string(),
    ]
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
            let size = usize::try_from(size)
                .ok()
                .filter(|s| (1..=4096).contains(s))
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
    match run_on(&mut device, &descriptor, command)? {
        Some(description) => {
            serde_json::to_string(&description).map_err(|e| DaemonError::WriteFailed(e.to_string()))
        }
        None => Ok("ok".to_string()),
    }
}

/// Whether a LampArray is the built-in keyboard: it says it is a keyboard,
/// or says nothing (or "undefined") and sits on the I2C bus, where a
/// laptop's keyboard is. A LampArray mouse or a light strip is never taken.
pub fn is_keyboard(kind: Option<LampArrayKind>, bus: Option<u16>) -> bool {
    match kind {
        Some(LampArrayKind::Keyboard) => true,
        None | Some(LampArrayKind::Undefined) => bus == Some(sysfs::BUS_I2C),
        Some(_) => false,
    }
}

/// Descriptions already read, by node and descriptor: a LampArray's
/// attributes do not change while it is plugged in.
#[derive(Default)]
pub struct LampCache(Mutex<Vec<(String, Vec<u8>, Description)>>);

/// The built-in keyboard's LampArray: its node and description.
pub fn find_keyboard(
    root: &SysRoot,
    helper: &dyn LampHelper,
    cache: &LampCache,
) -> Result<Option<(String, Description)>, DaemonError> {
    let mut last_error = None;
    for candidate in sysfs::lamp_array_nodes(root) {
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
        if is_keyboard(description.kind, candidate.bus) {
            return Ok(Some((candidate.node, description)));
        }
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

/// The stored choice, or `None` when Better OS has set nothing.
pub fn load(state: &Path) -> Result<Option<LightingChoice>, DaemonError> {
    match std::fs::read_to_string(state) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| DaemonError::Storage(e.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(DaemonError::Storage(error.to_string())),
    }
}

/// Keeps a choice for the boot-time restore. The keyboard's own default is
/// no choice at all: the file goes, so nothing is written at boot.
pub fn store(state: &Path, choice: &LightingChoice) -> Result<(), DaemonError> {
    if choice.effect == Effect::Firmware {
        return match std::fs::remove_file(state) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DaemonError::Storage(error.to_string())),
        };
    }
    if let Some(parent) = state.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DaemonError::Storage(e.to_string()))?;
    }
    let text = serde_json::to_string(choice).map_err(|e| DaemonError::Storage(e.to_string()))?;
    let temporary = state.with_extension("json.tmp");
    std::fs::write(&temporary, text).map_err(|e| DaemonError::Storage(e.to_string()))?;
    std::fs::rename(&temporary, state).map_err(|e| DaemonError::Storage(e.to_string()))
}

/// What the machine offers and what Better OS set.
pub fn describe(
    root: &SysRoot,
    helper: &dyn LampHelper,
    cache: &LampCache,
    stored: Option<LightingChoice>,
) -> Result<LightingReply, DaemonError> {
    if let Some((_, description)) = find_keyboard(root, helper, cache)? {
        return Ok(LightingReply {
            interface: Interface::LampArray,
            lamp_array: Some(description),
            stored,
        });
    }
    if sysfs::keyboard_rgb_mode_attribute(root).is_some() {
        return Ok(LightingReply {
            interface: Interface::AsusWmi,
            lamp_array: None,
            stored,
        });
    }
    Err(DaemonError::HardwareAbsent)
}

/// The attribute's real path, the class link resolved to the device.
fn real(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Shows `choice` on the keyboard. A choice the interface does not take
/// touches nothing.
pub fn apply(
    root: &SysRoot,
    helper: &dyn LampHelper,
    writer: &dyn AttributeWriter,
    cache: &LampCache,
    choice: &LightingChoice,
) -> Result<Interface, DaemonError> {
    if let Some((node, _)) = find_keyboard(root, helper, cache)? {
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
        helper.run(&node, command)?;
        return Ok(Interface::LampArray);
    }
    if let Some(path) = sysfs::keyboard_rgb_mode_attribute(root) {
        if !lighting::accepts(Interface::AsusWmi, choice) {
            return Err(DaemonError::InvalidArgument("effect".to_string()));
        }
        let value = asus_wmi::value(choice)
            .ok_or_else(|| DaemonError::InvalidArgument("effect".to_string()))?;
        writer.write(&real(path), &value)?;
        return Ok(Interface::AsusWmi);
    }
    Err(DaemonError::HardwareAbsent)
}

/// What `--restore-hardware` did for the keyboard.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoredLighting {
    NothingStored,
    Written(Interface),
}

/// Shows the stored choice again at boot.
pub fn restore(
    root: &SysRoot,
    helper: &dyn LampHelper,
    writer: &dyn AttributeWriter,
    state: &Path,
) -> Result<RestoredLighting, DaemonError> {
    let Some(choice) = load(state)? else {
        return Ok(RestoredLighting::NothingStored);
    };
    apply(root, helper, writer, &LampCache::default(), &choice).map(RestoredLighting::Written)
}

/// The package's prerm on removal: a keyboard Better OS took over goes back
/// to its firmware. `kbd_rgb_mode` needs nothing: Better OS never saved to
/// the firmware, whose own setting comes back at the next start. Returns
/// what could not be done; it never fails the removal.
pub fn release(root: &SysRoot, helper: &dyn LampHelper, state: &Path) -> Vec<String> {
    match load(state) {
        Ok(None) => return Vec::new(),
        Ok(Some(_)) | Err(_) => {}
    }
    match find_keyboard(root, helper, &LampCache::default()) {
        Ok(Some((node, _))) => match helper.run(&node, LampCommand::GiveBack) {
            Ok(_) => Vec::new(),
            Err(error) => vec![format!("/dev/{node}: {error}")],
        },
        Ok(None) => Vec::new(),
        Err(error) => vec![error.to_string()],
    }
}

/// A helper that drives an in-memory LampArray: tests and sandbox
/// stand-ins. Never in the shipped daemon.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeLampHelper {
    pub node: String,
    pub descriptor: Vec<u8>,
    pub device: Mutex<lamparray::fake::FakeLampArray>,
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
            runs: Default::default(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl LampHelper for FakeLampHelper {
    fn run(&self, node: &str, command: LampCommand) -> Result<Option<Description>, DaemonError> {
        self.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if node != self.node {
            return Err(DaemonError::WriteFailed(format!("/dev/{node}: ENOENT")));
        }
        // The helper is a separate process: it sees only its argument list.
        let parsed = LampCommand::parse(&command.args())
            .ok_or_else(|| DaemonError::InvalidArgument("lamp command".into()))?;
        let mut device = self.device.lock().unwrap();
        run_on(&mut *device, &self.descriptor, parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::DirectWriter;

    const M5406WA: &[u8] =
        include_bytes!("../../hardware-core/tests/fixtures/m5406wa-ite5570.rdesc");

    fn machine(dir: &Path, bus: &str) {
        let device = dir.join("sys/class/hidraw/hidraw1/device");
        std::fs::create_dir_all(&device).unwrap();
        std::fs::write(device.join("report_descriptor"), M5406WA).unwrap();
        std::fs::write(
            device.join("uevent"),
            format!("HID_ID={bus}:00000B05:00005570\n"),
        )
        .unwrap();
        let led = dir.join("sys/class/leds/asus::kbd_backlight");
        std::fs::create_dir_all(&led).unwrap();
        std::fs::write(led.join("max_brightness"), "3\n").unwrap();
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

    #[test]
    fn the_helper_unit_can_open_only_its_one_node() {
        let properties = lamp_helper_properties("hidraw1");
        assert!(properties.contains(&"DevicePolicy=closed".to_string()));
        assert_eq!(
            properties
                .iter()
                .filter(|p| p.starts_with("DeviceAllow="))
                .collect::<Vec<_>>(),
            vec!["DeviceAllow=/dev/hidraw1 rw"]
        );
        assert!(properties.contains(&"TemporaryFileSystem=/sys:ro".to_string()));
        assert!(properties.contains(&"CapabilityBoundingSet=".to_string()));
        assert!(properties.contains(&"NoNewPrivileges=yes".to_string()));
        assert!(properties.contains(&"PrivateNetwork=yes".to_string()));
        assert!(!properties.iter().any(|p| p.starts_with("BindPaths")));
        assert!(!properties.iter().any(|p| p.starts_with("PrivateDevices")));
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
        machine(dir.path(), "0018");
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw1", M5406WA);
        let cache = LampCache::default();
        let state = dir.path().join("var/keyboard-lighting.json");

        let reply = describe(&root, &helper, &cache, None).unwrap();
        assert_eq!(reply.interface, Interface::LampArray);
        assert_eq!(reply.lamp_array.as_ref().unwrap().lamps, 1);
        // The description is read once.
        describe(&root, &helper, &cache, None).unwrap();
        assert_eq!(helper.runs.load(std::sync::atomic::Ordering::SeqCst), 1);

        let choice = LightingChoice {
            effect: Effect::Static,
            colour: Rgb::new(255, 0, 0),
            intensity: 100,
            speed: lighting::Speed::Normal,
        };
        assert_eq!(
            apply(&root, &helper, &DirectWriter, &cache, &choice),
            Ok(Interface::LampArray)
        );
        assert_eq!(helper.device.lock().unwrap().shown, Some([254, 0, 0, 254]));
        assert!(!helper.device.lock().unwrap().autonomous);
        store(&state, &choice).unwrap();
        assert_eq!(load(&state).unwrap(), Some(choice));

        // Boot: the firmware is back in control; the restore takes it again.
        helper.device.lock().unwrap().autonomous = true;
        assert_eq!(
            restore(&root, &helper, &DirectWriter, &state),
            Ok(RestoredLighting::Written(Interface::LampArray))
        );
        assert!(!helper.device.lock().unwrap().autonomous);

        // Removal gives the keyboard back.
        assert!(release(&root, &helper, &state).is_empty());
        assert!(helper.device.lock().unwrap().autonomous);

        // The keyboard's own default stores nothing.
        store(&state, &LightingChoice::starting(Effect::Firmware)).unwrap();
        assert!(!state.exists());
        assert_eq!(
            restore(&root, &helper, &DirectWriter, &state),
            Ok(RestoredLighting::NothingStored)
        );
        let before = helper.runs.load(std::sync::atomic::Ordering::SeqCst);
        assert!(release(&root, &helper, &state).is_empty());
        // With nothing stored, removal never touches the keyboard.
        assert_eq!(
            helper.runs.load(std::sync::atomic::Ordering::SeqCst),
            before
        );
    }

    #[test]
    fn an_effect_a_lamp_array_cannot_do_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        machine(dir.path(), "0018");
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

    #[test]
    fn a_lamp_array_that_is_not_a_keyboard_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        // A USB LampArray that says it is a mouse.
        machine(dir.path(), "0003");
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw1", M5406WA);
        helper.device.lock().unwrap().kind = 2;
        assert_eq!(
            describe(&root, &helper, &LampCache::default(), None),
            Err(DaemonError::HardwareAbsent)
        );
        // Kind undefined: a keyboard only on the built-in bus.
        helper.device.lock().unwrap().kind = 0;
        assert_eq!(
            describe(&root, &helper, &LampCache::default(), None),
            Err(DaemonError::HardwareAbsent)
        );
        assert!(is_keyboard(Some(LampArrayKind::Undefined), Some(0x18)));
        assert!(is_keyboard(None, Some(0x18)));
        assert!(!is_keyboard(Some(LampArrayKind::Mouse), Some(0x18)));
        assert!(is_keyboard(Some(LampArrayKind::Keyboard), Some(0x03)));
    }

    #[test]
    fn kbd_rgb_mode_gets_the_effect_through_the_attribute_helper() {
        let dir = tempfile::tempdir().unwrap();
        let led = dir.path().join("sys/class/leds/asus::kbd_backlight");
        std::fs::create_dir_all(&led).unwrap();
        let attribute = led.join("kbd_rgb_mode");
        std::fs::write(&attribute, "").unwrap();
        let root = SysRoot::at(dir.path());
        let helper = FakeLampHelper::new("hidraw9", M5406WA);
        let cache = LampCache::default();
        assert_eq!(
            describe(&root, &helper, &cache, None).unwrap().interface,
            Interface::AsusWmi
        );
        let choice = LightingChoice {
            effect: Effect::Breathing,
            colour: Rgb::new(0, 128, 255),
            intensity: 100,
            speed: lighting::Speed::Slow,
        };
        assert_eq!(
            apply(&root, &helper, &DirectWriter, &cache, &choice),
            Ok(Interface::AsusWmi)
        );
        assert_eq!(
            std::fs::read_to_string(&attribute).unwrap(),
            "0 1 0 128 255 0"
        );
        assert_eq!(
            apply(
                &root,
                &helper,
                &DirectWriter,
                &cache,
                &LightingChoice::starting(Effect::Firmware)
            ),
            Err(DaemonError::InvalidArgument("effect".into()))
        );
        assert_eq!(
            std::fs::read_to_string(&attribute).unwrap(),
            "0 1 0 128 255 0"
        );
        // No keyboard at all.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            describe(&SysRoot::at(empty.path()), &helper, &cache, None),
            Err(DaemonError::HardwareAbsent)
        );
    }

    #[test]
    fn a_stored_file_that_is_not_a_choice_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("keyboard-lighting.json");
        std::fs::write(&state, r#"{"effect":"static","report":"5f00"}"#).unwrap();
        assert!(matches!(load(&state), Err(DaemonError::Storage(_))));
    }

    #[test]
    fn helper_output_is_read_strictly() {
        assert_eq!(read_helper_output(LampCommand::GiveBack, "ok\n"), Ok(None));
        assert!(read_helper_output(LampCommand::GiveBack, "done").is_err());
        assert!(read_helper_output(LampCommand::Describe, "{}").is_err());
    }
}
