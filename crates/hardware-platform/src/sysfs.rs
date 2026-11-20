//! sysfs, read only, under a root.

use std::path::{Path, PathBuf};

use hardware_core::Presence;
use hardware_core::battery::{BatteryFacts, BatteryStatus};
use hardware_core::fan::FanState;
use hardware_core::lighting::{HintInterface, LightingHint, asus_wmi};
#[cfg(feature = "sensors")]
use hardware_core::sensors::{SensorInput, SensorKind};
#[cfg(feature = "sensors")]
use monitor_collectors_linux::Roots;
#[cfg(feature = "sensors")]
use monitor_collectors_linux::hwmon::{SensorKind as MonitorKind, scan_hwmon};

/// The platform devices whose `pwm1_enable` the daemon recognises as a fan
/// mode switch, first match wins (spec 11.2).
pub const FAN_PLATFORM_DEVICES: [&str; 1] = ["asus-nb-wmi"];

/// Where the machine is: `/` on a real one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SysRoot(PathBuf);

impl SysRoot {
    pub fn system() -> SysRoot {
        SysRoot(PathBuf::from("/"))
    }

    /// A tree laid out as `<root>/sys`, `<root>/proc`, `<root>/etc`.
    pub fn at(root: impl Into<PathBuf>) -> SysRoot {
        SysRoot(root.into())
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative.trim_start_matches('/'))
    }

    #[cfg(feature = "sensors")]
    fn roots(&self) -> Roots {
        Roots::new(self.path("proc"), self.path("sys"), self.path("etc/passwd"))
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

/// The machine's own battery: a `power_supply` of type `Battery` whose scope
/// is not `Device` (a mouse reports its battery as one) and that is present.
pub fn system_battery(root: &SysRoot) -> Option<PathBuf> {
    sorted_entries(&root.path("sys/class/power_supply"))
        .into_iter()
        .find(|dir| {
            read_text(&dir.join("type")).is_ok_and(|t| t == "Battery")
                && read_text(&dir.join("scope")).map_or(true, |s| s != "Device")
                && read_text(&dir.join("present")).map_or(true, |p| p != "0")
        })
}

/// The battery's facts. Absent when there is no system battery; unreadable
/// when it is there and its capacity cannot be read.
pub fn battery(root: &SysRoot) -> Presence<BatteryFacts> {
    let Some(dir) = system_battery(root) else {
        return Presence::Absent;
    };
    let number = |name: &str| read_text(&dir.join(name)).ok()?.parse::<f64>().ok();
    let capacity = match read_text(&dir.join("capacity")) {
        Ok(text) => match text.parse::<f64>() {
            Ok(value) => Some(value.clamp(0.0, 100.0).round() as u8),
            Err(_) => return Presence::Unreadable(format!("capacity: not a number: {text:?}")),
        },
        Err(error) => return Presence::Unreadable(error),
    };
    Presence::Present(BatteryFacts {
        capacity,
        status: read_text(&dir.join("status"))
            .map(|s| BatteryStatus::parse(&s))
            .unwrap_or(BatteryStatus::Unknown),
        full: number("charge_full").or_else(|| number("energy_full")),
        full_design: number("charge_full")
            .and_then(|_| number("charge_full_design"))
            .or_else(|| number("energy_full_design")),
        cycles: number("cycle_count").map(|c| c.max(0.0) as u64),
        model: read_text(&dir.join("model_name"))
            .ok()
            .filter(|m| !m.is_empty()),
    })
}

/// `charge_control_end_threshold` of the system battery, when it has one.
pub fn charge_limit_attribute(root: &SysRoot) -> Option<PathBuf> {
    let path = system_battery(root)?.join("charge_control_end_threshold");
    path.exists().then_some(path)
}

pub fn charge_limit(root: &SysRoot) -> Presence<u8> {
    let Some(path) = charge_limit_attribute(root) else {
        return Presence::Absent;
    };
    match read_text(&path) {
        Ok(text) => match text.parse::<u8>() {
            Ok(value) if (1..=100).contains(&value) => Presence::Present(value),
            _ => Presence::Unreadable(format!("{}: {text:?}", path.display())),
        },
        Err(error) => Presence::Unreadable(error),
    }
}

/// The `pwm1_enable` of a recognised platform device that has no writable
/// `pwm1`, in [`FAN_PLATFORM_DEVICES`] order.
pub fn fan_mode_attribute(root: &SysRoot) -> Option<PathBuf> {
    let chips = sorted_entries(&root.path("sys/class/hwmon"));
    FAN_PLATFORM_DEVICES.iter().find_map(|wanted| {
        chips.iter().find_map(|chip| {
            let device = std::fs::read_link(chip.join("device")).ok()?;
            let name = device.file_name()?.to_str()?;
            let attribute = chip.join("pwm1_enable");
            let pwm = chip.join("pwm1");
            let pwm_writable = std::fs::metadata(&pwm).is_ok_and(|m| {
                std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o222 != 0
            });
            (name == *wanted && attribute.exists() && !pwm_writable).then_some(attribute)
        })
    })
}

pub fn fan_mode(root: &SysRoot) -> Presence<FanState> {
    let Some(path) = fan_mode_attribute(root) else {
        return Presence::Absent;
    };
    match read_text(&path) {
        Ok(text) => match FanState::from_pwm_enable(&text) {
            Some(state) => Presence::Present(state),
            None => Presence::Unreadable(format!("{}: {text:?}", path.display())),
        },
        Err(error) => Presence::Unreadable(error),
    }
}

/// Every temperature and fan input, through Better Monitor's hwmon reader.
#[cfg(feature = "sensors")]
pub fn sensors(root: &SysRoot) -> Vec<SensorInput> {
    let Ok(readings) = scan_hwmon(&root.roots()) else {
        return Vec::new();
    };
    readings
        .into_iter()
        .filter_map(|reading| {
            let kind = match reading.kind {
                MonitorKind::Temperature => SensorKind::Temperature,
                MonitorKind::Fan => SensorKind::Fan,
                _ => return None,
            };
            Some(SensorInput {
                chip: reading.chip,
                label: reading.label,
                kind,
                value: reading.value.ok(),
            })
        })
        .collect()
}

/// The keyboard backlight LED's maximum, as a hint that a backlight exists
/// when neither bus answers. Only read; it is never written here.
pub fn keyboard_led(root: &SysRoot) -> Option<PathBuf> {
    sorted_entries(&root.path("sys/class/leds"))
        .into_iter()
        .find(|dir| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("::kbd_backlight"))
        })
}

/// A hidraw node whose report descriptor declares a LampArray Better OS
/// can drive (ticket 136), found without opening the device: sysfs's
/// `report_descriptor` is readable by everyone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LampArrayNode {
    /// `hidraw` and digits, nothing else.
    pub node: String,
    /// The HID bus type from the device's `uevent` (`0x18` is I2C).
    pub bus: Option<u16>,
}

/// HID's bus type for I2C, where a laptop's built-in keyboard sits.
pub const BUS_I2C: u16 = 0x18;

/// Whether `name` is `hidraw` followed by one to four digits.
pub fn is_hidraw_name(name: &str) -> bool {
    name.strip_prefix("hidraw").is_some_and(|digits| {
        (1..=4).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// Reads at most one byte more than any descriptor can be, so a file that
/// is not one cannot make this read without bound.
pub fn read_bounded(path: &Path, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

/// The bus type in a HID `uevent`'s `HID_ID=0018:00000B05:00005570`.
fn hid_bus(uevent: &str) -> Option<u16> {
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    u16::from_str_radix(id.split(':').next()?, 16).ok()
}

/// A hidraw node's report descriptor, read from sysfs.
pub fn report_descriptor(root: &SysRoot, node: &str) -> Option<Vec<u8>> {
    if !is_hidraw_name(node) {
        return None;
    }
    read_bounded(
        &root
            .path("sys/class/hidraw")
            .join(node)
            .join("device/report_descriptor"),
        hardware_core::lamparray::MAX_DESCRIPTOR_LEN,
    )
}

/// Every LampArray, built-in ones (I2C) first, then by number.
pub fn lamp_array_nodes(root: &SysRoot) -> Vec<LampArrayNode> {
    let mut nodes: Vec<LampArrayNode> = sorted_entries(&root.path("sys/class/hidraw"))
        .into_iter()
        .filter_map(|dir| {
            let node = dir.file_name()?.to_str()?.to_string();
            let descriptor = report_descriptor(root, &node)?;
            hardware_core::lamparray::is_lamp_array(&descriptor).then(|| LampArrayNode {
                bus: std::fs::read_to_string(dir.join("device/uevent"))
                    .ok()
                    .as_deref()
                    .and_then(hid_bus),
                node,
            })
        })
        .collect();
    nodes.sort_by_key(|n| (n.bus != Some(BUS_I2C), n.node.len(), n.node.clone()));
    nodes
}

/// asus-wmi's `kbd_rgb_mode` on the keyboard backlight LED, on the TUF and
/// ROG models whose firmware has the RGB device.
pub fn keyboard_rgb_mode_attribute(root: &SysRoot) -> Option<PathBuf> {
    let path = keyboard_led(root)?.join(asus_wmi::ATTRIBUTE);
    path.exists().then_some(path)
}

/// What sysfs says about the keyboard's colour: a LampArray first, then
/// `kbd_rgb_mode`, and whether there is a backlight at all.
pub fn lighting_hint(root: &SysRoot) -> Presence<LightingHint> {
    let interface = match lamp_array_nodes(root).into_iter().next() {
        Some(lamp) => Some(HintInterface::LampArray { node: lamp.node }),
        None => keyboard_rgb_mode_attribute(root).map(|_| HintInterface::AsusWmi),
    };
    let backlight = keyboard_led(root).is_some();
    if interface.is_none() && !backlight {
        return Presence::Absent;
    }
    Presence::Present(LightingHint {
        interface,
        backlight,
    })
}

/// The built-in display's backlight, as a hint that brightness exists when
/// the desktop's service cannot be asked.
pub fn display_backlight(root: &SysRoot) -> Option<PathBuf> {
    sorted_entries(&root.path("sys/class/backlight"))
        .into_iter()
        .next()
}

/// The facts About this PC shows that come from files.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AboutFiles {
    pub product_name: Option<String>,
    pub kernel: Option<String>,
}

pub fn about(root: &SysRoot) -> AboutFiles {
    AboutFiles {
        product_name: read_text(&root.path("sys/class/dmi/id/product_name")).ok(),
        kernel: read_text(&root.path("proc/sys/kernel/osrelease")).ok(),
    }
}
