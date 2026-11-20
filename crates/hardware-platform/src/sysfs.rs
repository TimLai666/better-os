//! sysfs, read only, under a root.

use std::path::{Path, PathBuf};

use hardware_core::Presence;
use hardware_core::battery::{BatteryFacts, BatteryStatus};
use hardware_core::charge_state::{BatteryPart, ChargeLimit, ChargeReadings};
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

/// The variable a debug build of a window reads its sysfs root from.
pub use hardware_core::DEBUG_SYS_ROOT_VARIABLE as DEBUG_ROOT_VARIABLE;

/// Where the machine is: `/` on a real one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SysRoot(PathBuf);

impl SysRoot {
    pub fn system() -> SysRoot {
        SysRoot(PathBuf::from("/"))
    }

    /// The root a window reads: `/`, or in a debug build the fixture tree
    /// `BETTER_OS_DEBUG_SYS_ROOT` names, for screenshots in a sandbox. A
    /// release build never reads the variable.
    pub fn for_window() -> SysRoot {
        if cfg!(debug_assertions)
            && let Some(root) = std::env::var_os(DEBUG_ROOT_VARIABLE)
        {
            return SysRoot::at(root);
        }
        SysRoot::system()
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

/// Whether `dir` is one of the machine's own batteries: a `power_supply` of
/// type `Battery` whose scope is not `Device` (a mouse reports its battery as
/// one) and that is present.
fn is_system_battery(dir: &Path) -> bool {
    read_text(&dir.join("type")).is_ok_and(|t| t == "Battery")
        && read_text(&dir.join("scope")).map_or(true, |s| s != "Device")
        && read_text(&dir.join("present")).map_or(true, |p| p != "0")
}

/// Every system battery, in name order (`BAT0` before `BAT1`).
pub fn system_batteries(root: &SysRoot) -> Vec<PathBuf> {
    sorted_entries(&root.path("sys/class/power_supply"))
        .into_iter()
        .filter(|dir| is_system_battery(dir))
        .collect()
}

/// The machine's first battery, whose facts and limit the System page shows.
pub fn system_battery(root: &SysRoot) -> Option<PathBuf> {
    system_batteries(root).into_iter().next()
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

/// Whether a charger supplies power: each other `power_supply`'s `type` and
/// `online`, judged by `hardware_core::charge_state::charger_online`. `None`
/// when no charger is listed or one could not be read.
pub fn charger_online(root: &SysRoot) -> Option<bool> {
    let sources: Vec<(String, Option<bool>)> = sorted_entries(&root.path("sys/class/power_supply"))
        .into_iter()
        .filter_map(|dir| {
            let kind = read_text(&dir.join("type")).ok()?;
            let online = match read_text(&dir.join("online")).as_deref() {
                Ok(text) => text.parse::<u32>().ok().map(|v| v > 0),
                Err(_) => None,
            };
            Some((kind, online))
        })
        .collect();
    hardware_core::charge_state::charger_online(
        sources
            .iter()
            .map(|(kind, online)| (kind.as_str(), *online)),
    )
}

/// One battery's part of the battery icon's reading, its own limit included.
fn battery_part(dir: &Path) -> BatteryPart {
    let number = |name: &str| read_text(&dir.join(name)).ok()?.parse::<f64>().ok();
    let end = dir.join("charge_control_end_threshold");
    let limit = if !end.exists() {
        ChargeLimit::NotSupported
    } else {
        match number("charge_control_end_threshold") {
            Some(end) if (1.0..=100.0).contains(&end) => ChargeLimit::Percent {
                end: end.round() as u8,
                start: number("charge_control_start_threshold")
                    .filter(|start| (0.0..100.0).contains(start))
                    .map(|start| start.round() as u8),
            },
            _ => ChargeLimit::Unreadable,
        }
    };
    BatteryPart {
        status: read_text(&dir.join("status"))
            .ok()
            .map(|s| BatteryStatus::parse(&s)),
        percent: number("capacity").map(|c| c.clamp(0.0, 100.0).round() as u8),
        energy_now: number("energy_now").or_else(|| number("charge_now")),
        energy_full: number("energy_full").or_else(|| number("charge_full")),
        limit,
    }
}

/// What every battery icon reads: the charger and every system battery with
/// its own limit (`hardware_core::charge_state`). `None` without a system
/// battery. A charge-reporting battery's `charge_*` stands in for energy: the
/// weighting needs one unit across the batteries, and both of a ThinkPad's
/// report the same kind.
pub fn charge_readings(root: &SysRoot) -> Option<ChargeReadings> {
    let batteries: Vec<BatteryPart> = system_batteries(root)
        .iter()
        .map(|dir| battery_part(dir))
        .collect();
    (!batteries.is_empty()).then(|| ChargeReadings {
        charger_online: charger_online(root),
        batteries,
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
    pub vendor: Option<u16>,
    pub product: Option<u16>,
}

impl LampArrayNode {
    /// On the I2C bus: a laptop's own keyboard, not one plugged in by USB
    /// or Bluetooth.
    pub fn built_in(&self) -> bool {
        self.bus == Some(BUS_I2C)
    }

    /// The identity a stored choice is tied to (ADR 0034).
    pub fn device(&self) -> hardware_core::lighting::LightingDevice {
        hardware_core::lighting::LightingDevice {
            interface: hardware_core::lighting::Interface::LampArray,
            bus: self.bus,
            vendor: self.vendor,
            product: self.product,
        }
    }
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

/// Bus, vendor, and product from a HID `uevent`'s
/// `HID_ID=0018:00000B05:00005570`; each `None` when it does not read.
fn hid_id(uevent: &str) -> (Option<u16>, Option<u16>, Option<u16>) {
    let Some(id) = uevent.lines().find_map(|l| l.strip_prefix("HID_ID=")) else {
        return (None, None, None);
    };
    let mut parts = id.split(':').map(|part| {
        u32::from_str_radix(part.trim(), 16)
            .ok()
            .and_then(|v| u16::try_from(v).ok())
    });
    let mut next = || parts.next().flatten();
    (next(), next(), next())
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

/// Every LampArray, built-in ones (I2C) first, then by number. Only
/// [`built_in_lamp_arrays`] are ever the keyboard the System page sets.
pub fn lamp_array_nodes(root: &SysRoot) -> Vec<LampArrayNode> {
    let mut nodes: Vec<LampArrayNode> = sorted_entries(&root.path("sys/class/hidraw"))
        .into_iter()
        .filter_map(|dir| {
            let node = dir.file_name()?.to_str()?.to_string();
            let descriptor = report_descriptor(root, &node)?;
            if !hardware_core::lamparray::is_lamp_array(&descriptor) {
                return None;
            }
            let (bus, vendor, product) = std::fs::read_to_string(dir.join("device/uevent"))
                .map(|text| hid_id(&text))
                .unwrap_or((None, None, None));
            Some(LampArrayNode {
                node,
                bus,
                vendor,
                product,
            })
        })
        .collect();
    nodes.sort_by_key(|n| (n.bus != Some(BUS_I2C), n.node.len(), n.node.clone()));
    nodes
}

/// The built-in LampArrays (I2C), by number. A Dynamic Lighting keyboard
/// plugged in by USB or Bluetooth is never one of them: the System page
/// sets the laptop's own keyboard (ADR 0034).
pub fn built_in_lamp_arrays(root: &SysRoot) -> Vec<LampArrayNode> {
    lamp_array_nodes(root)
        .into_iter()
        .filter(LampArrayNode::built_in)
        .collect()
}

/// asus-wmi's `kbd_rgb_mode` on the keyboard backlight LED, on the TUF and
/// ROG models whose firmware has the RGB device.
pub fn keyboard_rgb_mode_attribute(root: &SysRoot) -> Option<PathBuf> {
    let path = keyboard_led(root)?.join(asus_wmi::ATTRIBUTE);
    path.exists().then_some(path)
}

/// What sysfs says about the built-in keyboard's colour: a built-in
/// LampArray first, then `kbd_rgb_mode`, and whether there is a backlight
/// at all. External LampArrays play no part.
pub fn lighting_hint(root: &SysRoot) -> Presence<LightingHint> {
    let interface = match built_in_lamp_arrays(root).into_iter().next() {
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
