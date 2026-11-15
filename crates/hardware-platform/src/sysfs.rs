//! sysfs, read only, under a root.

use std::path::{Path, PathBuf};

use hardware_core::Presence;
use hardware_core::battery::{BatteryFacts, BatteryStatus};
use hardware_core::fan::FanState;
#[cfg(feature = "sensors")]
use hardware_core::sensors::{SensorInput, SensorKind};
#[cfg(feature = "sensors")]
use monitor_collectors_linux::Roots;
#[cfg(feature = "sensors")]
use monitor_collectors_linux::hwmon::{SensorKind as MonitorKind, scan_hwmon};

/// The platform devices whose `pwm1_enable` the daemon recognises as a fan
/// mode switch, first match wins (spec 11.2). `better-manager-daemon.service`
/// opens each one's directory with `ReadWritePaths=`; a test holds the two
/// lists together.
pub const FAN_PLATFORM_DEVICES: [&str; 1] = ["asus-nb-wmi"];

/// The names a system battery is published under that the daemon writes.
/// Each is a `ReadWritePaths=` line in the daemon's unit, which resolves the
/// class symlink to the device directory; a battery under any other name is
/// read and shown, and the daemon reports it absent.
pub const BATTERY_NAMES: [&str; 4] = ["BAT0", "BAT1", "BAT2", "BATT"];

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

/// The same attribute, only under a name the daemon's unit opens. This is
/// what the daemon writes.
pub fn writable_charge_limit_attribute(root: &SysRoot) -> Option<PathBuf> {
    let path = charge_limit_attribute(root)?;
    let name = path.parent()?.file_name()?.to_str()?;
    BATTERY_NAMES.contains(&name).then_some(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_writable_battery_names_are_ordinary_names() {
        for name in BATTERY_NAMES {
            assert!(name.chars().all(|c| c.is_ascii_alphanumeric()));
        }
    }
}
