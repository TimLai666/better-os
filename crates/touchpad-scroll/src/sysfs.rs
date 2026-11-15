//! A device described from sysfs, without opening its node.
//!
//! The maintainer script and the `--eligible` listing judge devices this way:
//! reading `/sys/class/input/eventN/device/capabilities/*` needs no privilege
//! and no access to `/dev/input`, so the list of touchpads can be made where
//! the device nodes themselves are closed (Better Manager's daemon sandbox has
//! `DevicePolicy=closed`). The service then judges the node it opened again,
//! from the ioctls, with the axis ranges sysfs does not have.

use std::path::{Path, PathBuf};

use crate::caps::{Bits, DeviceDescriptor, InputId, classify_capabilities};

/// Parse a sysfs capability bitmap: hexadecimal words, most significant first,
/// each as wide as the kernel's `unsigned long` (64 bits on every architecture
/// Better OS ships for).
pub fn parse_bitmap(text: &str) -> Option<Bits> {
    let mut bits = Bits::new();
    for (index, word) in text.split_whitespace().rev().enumerate() {
        let word = u64::from_str_radix(word, 16).ok()?;
        for bit in 0..64 {
            if word & (1 << bit) != 0 {
                bits.set(u16::try_from(index * 64 + bit).ok()?);
            }
        }
    }
    Some(bits)
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim_end_matches('\n').to_string())
}

fn hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(read(path)?.trim(), 16).ok()
}

/// The device behind `/sys/class/input/<event>`, or `None` when sysfs does not
/// describe it completely. An incomplete description is never eligible.
pub fn describe(class_dir: &Path) -> Option<DeviceDescriptor> {
    let device = class_dir.join("device");
    let capabilities = device.join("capabilities");
    let bitmap = |name: &str| parse_bitmap(&read(&capabilities.join(name))?);
    Some(DeviceDescriptor {
        name: read(&device.join("name"))?,
        phys: read(&device.join("phys")).unwrap_or_default(),
        id: InputId {
            bustype: hex(&device.join("id/bustype"))?,
            vendor: hex(&device.join("id/vendor"))?,
            product: hex(&device.join("id/product"))?,
            version: hex(&device.join("id/version"))?,
        },
        properties: parse_bitmap(&read(&device.join("properties"))?)?,
        events: bitmap("ev")?,
        keys: bitmap("key")?,
        relative: bitmap("rel")?,
        absolute: bitmap("abs")?,
        misc: bitmap("msc")?,
        abs_info: Default::default(),
    })
}

/// Whether a kernel name is an event node's name and nothing else, so it can
/// be joined to `/dev/input` without leaving it.
pub fn is_event_name(name: &str) -> bool {
    name.strip_prefix("event")
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The event nodes under `class_root` (normally `/sys/class/input`) whose
/// capabilities make them touchpads the pass-through may hold, sorted.
pub fn eligible(class_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(class_root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_event_name(name))
        .filter(|name| {
            describe(&class_root.join(name))
                .is_some_and(|device| classify_capabilities(&device).is_ok())
        })
        .collect();
    names.sort();
    names
}

pub fn class_root() -> PathBuf {
    PathBuf::from("/sys/class/input")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::*;

    /// Writes a sysfs tree for one device, the way the kernel lays it out.
    fn write_device(root: &Path, event: &str, ev: &str, key: &str, abs: &str, props: &str) {
        let device = root.join(event).join("device");
        std::fs::create_dir_all(device.join("capabilities")).unwrap();
        std::fs::create_dir_all(device.join("id")).unwrap();
        std::fs::write(device.join("name"), "Test device\n").unwrap();
        std::fs::write(device.join("phys"), "i2c-TEST:00\n").unwrap();
        for (file, value) in [
            ("bustype", "0018"),
            ("vendor", "06cb"),
            ("product", "ceb0"),
            ("version", "0100"),
        ] {
            std::fs::write(device.join("id").join(file), format!("{value}\n")).unwrap();
        }
        std::fs::write(device.join("properties"), format!("{props}\n")).unwrap();
        let capabilities = device.join("capabilities");
        std::fs::write(capabilities.join("ev"), format!("{ev}\n")).unwrap();
        std::fs::write(capabilities.join("key"), format!("{key}\n")).unwrap();
        std::fs::write(capabilities.join("abs"), format!("{abs}\n")).unwrap();
        std::fs::write(capabilities.join("rel"), "0\n").unwrap();
        std::fs::write(capabilities.join("msc"), "20\n").unwrap();
    }

    #[test]
    fn a_bitmap_is_read_most_significant_word_first() {
        // A touchpad's key bitmap: BTN_LEFT, the finger tools, and BTN_TOUCH
        // sit in the fifth 64-bit word.
        let bits = parse_bitmap("e520 10000 0 0 0 0").unwrap();
        assert!(bits.has(BTN_LEFT));
        assert!(bits.has(BTN_TOOL_FINGER));
        assert!(bits.has(BTN_TOUCH));
        assert!(bits.has(BTN_TOOL_DOUBLETAP));
        assert!(!bits.has(30));
        assert!(parse_bitmap("zz").is_none());
    }

    #[test]
    fn only_the_touchpad_is_eligible() {
        let root = tempfile::tempdir().unwrap();
        // A clickpad, as a real Synaptics I2C pad reports it.
        write_device(
            root.path(),
            "event7",
            "1b",
            "e520 10000 0 0 0 0",
            "6e18000 3",
            "5",
        );
        // A keyboard: keys 1..=248, autorepeat and LEDs.
        write_device(
            root.path(),
            "event3",
            "120013",
            "1000000000007 ff9f207ac14057ff febeffdfffefffff fffffffffffffffe",
            "0",
            "0",
        );
        // A touchpad node that also reports KEY_A.
        write_device(
            root.path(),
            "event9",
            "1b",
            "e520 10000 0 0 0 40000000",
            "6e18000 3",
            "5",
        );
        // Not an event node.
        write_device(
            root.path(),
            "mouse0",
            "1b",
            "e520 10000 0 0 0 0",
            "6e18000 3",
            "5",
        );
        assert_eq!(eligible(root.path()), vec!["event7".to_string()]);
    }

    #[test]
    fn an_incomplete_description_is_never_eligible() {
        let root = tempfile::tempdir().unwrap();
        write_device(
            root.path(),
            "event7",
            "1b",
            "e520 10000 0 0 0 0",
            "6e18000 3",
            "5",
        );
        std::fs::remove_file(root.path().join("event7/device/capabilities/key")).unwrap();
        assert!(eligible(root.path()).is_empty());
    }

    #[test]
    fn event_names_cannot_leave_dev_input() {
        assert!(is_event_name("event12"));
        for bad in [
            "event",
            "event1/../../sda",
            "../event1",
            "mouse0",
            "event-1",
            "event1 ",
        ] {
            assert!(!is_event_name(bad), "{bad}");
        }
    }
}
