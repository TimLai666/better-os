//! The two udev properties the clone has to carry over from the original.
//!
//! libinput pairs a touchpad with the built-in keyboard for disable-while-
//! typing only when the touchpad's `ID_INPUT_TOUCHPAD_INTEGRATION` is
//! `internal`, and pairs it with a trackpoint (for the top software buttons and
//! for palm handling) by `LIBINPUT_DEVICE_GROUP`. A uinput clone has neither of
//! its own: its udev properties would be guessed from a virtual device. So the
//! service reads them from the original's udev database entry
//! (`/run/udev/data/c13:<minor>`) before it drops privilege, writes them into
//! the clone's `phys`, and the udev rule hands them back to the clone with
//! `better-touchpad-scrolld --clone-properties`. The bus type stands in for the
//! integration only when the database has none.

use std::path::Path;

use crate::caps::PHYS_PREFIX;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UdevFacts {
    pub integration: Option<String>,
    pub device_group: Option<String>,
}

/// The longest device group carried. The clone's `phys` is read back through
/// `EVIOCGPHYS` with 256 bytes, so the whole string has to fit.
const MAX_GROUP: usize = 200;

fn valid_integration(value: &str) -> bool {
    matches!(value, "internal" | "external")
}

fn valid_group(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_GROUP
        && value != "-"
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":._/-".contains(&b))
}

/// Read a udev database entry. Values that are not what udev writes for these
/// two properties are dropped rather than carried.
pub fn parse_db(text: &str) -> UdevFacts {
    let mut facts = UdevFacts::default();
    for line in text.lines() {
        let Some(property) = line.strip_prefix("E:") else {
            continue;
        };
        match property.split_once('=') {
            Some(("ID_INPUT_TOUCHPAD_INTEGRATION", value)) if valid_integration(value) => {
                facts.integration = Some(value.to_string());
            }
            Some(("LIBINPUT_DEVICE_GROUP", value)) if valid_group(value) => {
                facts.device_group = Some(value.to_string());
            }
            _ => {}
        }
    }
    facts
}

pub fn read_db(major: u32, minor: u32) -> UdevFacts {
    std::fs::read_to_string(format!("/run/udev/data/c{major}:{minor}"))
        .map(|text| parse_db(&text))
        .unwrap_or_default()
}

/// The clone's `phys`: the prefix, the integration, and the device group or
/// `-`.
pub fn clone_phys(integration: &str, device_group: Option<&str>) -> String {
    format!(
        "{PHYS_PREFIX}{integration}/{}",
        device_group
            .filter(|group| valid_group(group))
            .unwrap_or("-")
    )
}

/// The properties a clone's `phys` names, or `None` when it is not exactly
/// the shape [`clone_phys`] writes.
pub fn parse_clone_phys(phys: &str) -> Option<UdevFacts> {
    let rest = phys.strip_prefix(PHYS_PREFIX)?;
    let (integration, group) = rest.split_once('/')?;
    if !valid_integration(integration) {
        return None;
    }
    let device_group = match group {
        "-" => None,
        group if valid_group(group) => Some(group.to_string()),
        _ => return None,
    };
    Some(UdevFacts {
        integration: Some(integration.to_string()),
        device_group,
    })
}

/// `--clone-properties <syspath>`, run by udev for a clone: the properties as
/// `KEY=VALUE` lines for `IMPORT{program}`.
///
/// The `phys` alone is not trusted: anything that can write `/dev/uinput` can
/// give a device that `phys`. The properties are printed only when a real
/// touchpad is present right now (not a clone, accepted by
/// [`crate::caps::classify_capabilities`]) with the clone's bus type, vendor,
/// product, and name, and with exactly the integration and device group the
/// `phys` claims (its udev entry's own, or the bus type's integration when the
/// entry has none). Otherwise nothing is printed, and the clone gets what udev
/// and libinput would give any virtual device.
pub fn clone_properties(syspath: &str) -> Option<String> {
    if !syspath.starts_with("/sys/") {
        return None;
    }
    clone_properties_at(
        syspath,
        &crate::sysfs::class_root(),
        Path::new("/run/udev/data"),
    )
}

pub fn clone_properties_at(syspath: &str, class_root: &Path, udev_data: &Path) -> Option<String> {
    if syspath.split('/').any(|part| part == "..") {
        return None;
    }
    let clone = crate::sysfs::describe(Path::new(syspath))?;
    let claimed = parse_clone_phys(&clone.phys)?;
    let matches_an_original = std::fs::read_dir(class_root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| crate::sysfs::is_event_name(name))
        .filter_map(|name| {
            let mut original = crate::sysfs::describe(&class_root.join(&name))?;
            let dev = std::fs::read_to_string(class_root.join(&name).join("dev")).ok()?;
            let (major, minor) = dev.trim().split_once(':')?;
            original.udev = std::fs::read_to_string(udev_data.join(format!("c{major}:{minor}")))
                .map(|text| parse_db(&text))
                .unwrap_or_default();
            Some(original)
        })
        .filter(|original| crate::caps::classify_capabilities(original).is_ok())
        .any(|original| {
            original.id == clone.id
                && original.name == clone.name
                && Some(original.integration()) == claimed.integration.as_deref()
                && original.udev.device_group == claimed.device_group
        });
    if !matches_an_original {
        return None;
    }
    let mut out = String::new();
    if let Some(integration) = claimed.integration {
        out.push_str(&format!("ID_INPUT_TOUCHPAD_INTEGRATION={integration}\n"));
    }
    if let Some(group) = claimed.device_group {
        out.push_str(&format!("LIBINPUT_DEVICE_GROUP={group}\n"));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DB: &str = "S:input/by-path/platform-AMDI0010:01-event-mouse\n\
        I:5123456\n\
        E:ID_INPUT=1\n\
        E:ID_INPUT_TOUCHPAD=1\n\
        E:ID_INPUT_TOUCHPAD_INTEGRATION=internal\n\
        E:LIBINPUT_DEVICE_GROUP=18/6cb/ceb0:i2c-SYNA2BA6:00\n\
        G:seat\n";

    #[test]
    fn the_originals_integration_and_group_are_read_from_its_database_entry() {
        assert_eq!(
            parse_db(DB),
            UdevFacts {
                integration: Some("internal".to_string()),
                device_group: Some("18/6cb/ceb0:i2c-SYNA2BA6:00".to_string()),
            }
        );
    }

    #[test]
    fn values_udev_would_not_write_are_dropped() {
        let facts =
            parse_db("E:ID_INPUT_TOUCHPAD_INTEGRATION=sideways\nE:LIBINPUT_DEVICE_GROUP=a b;rm\n");
        assert_eq!(facts, UdevFacts::default());
        let long = format!("E:LIBINPUT_DEVICE_GROUP={}\n", "a".repeat(MAX_GROUP + 1));
        assert_eq!(parse_db(&long), UdevFacts::default());
    }

    #[test]
    fn the_phys_round_trips_and_nothing_else_parses() {
        let phys = clone_phys("internal", Some("18/6cb/ceb0:i2c-SYNA2BA6:00"));
        assert_eq!(
            phys,
            "better-touchpad-scroll/internal/18/6cb/ceb0:i2c-SYNA2BA6:00"
        );
        assert!(phys.len() < 256);
        assert_eq!(parse_clone_phys(&phys), Some(parse_db(DB)));
        assert_eq!(
            parse_clone_phys("better-touchpad-scroll/external/-"),
            Some(UdevFacts {
                integration: Some("external".to_string()),
                device_group: None,
            })
        );
        for bad in [
            "i2c-SYNA2BA6:00",
            "better-touchpad-scroll/internal",
            "better-touchpad-scroll/inside/-",
            "better-touchpad-scroll/internal/a b",
            "better-touchpad-scroll/internal/x\nLIBINPUT_DEVICE_GROUP=evil",
        ] {
            assert_eq!(parse_clone_phys(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn clone_properties_reads_only_under_sys() {
        assert_eq!(clone_properties("/etc/passwd"), None);
        assert_eq!(clone_properties("/sys/../etc"), None);
    }

    /// One input device in a scratch sysfs: the touchpad's capabilities.
    fn write_device(dir: &Path, name: &str, phys: &str, product: &str, dev: Option<&str>) {
        let device = dir.join("device");
        std::fs::create_dir_all(device.join("capabilities")).unwrap();
        std::fs::create_dir_all(device.join("id")).unwrap();
        std::fs::write(device.join("name"), format!("{name}\n")).unwrap();
        std::fs::write(device.join("phys"), format!("{phys}\n")).unwrap();
        for (file, value) in [
            ("bustype", "0018"),
            ("vendor", "06cb"),
            ("product", product),
            ("version", "0100"),
        ] {
            std::fs::write(device.join("id").join(file), format!("{value}\n")).unwrap();
        }
        std::fs::write(device.join("properties"), "5\n").unwrap();
        let caps = device.join("capabilities");
        for (file, value) in [
            ("ev", "1b"),
            ("key", "e520 10000 0 0 0 0"),
            ("abs", "6e18000 3"),
            ("rel", "0"),
            ("msc", "20"),
        ] {
            std::fs::write(caps.join(file), format!("{value}\n")).unwrap();
        }
        if let Some(dev) = dev {
            std::fs::write(dir.join("dev"), format!("{dev}\n")).unwrap();
        }
    }

    #[test]
    fn a_clone_gets_its_properties_only_when_a_matching_touchpad_is_there() {
        let root = tempfile::tempdir().unwrap();
        let class = root.path().join("class");
        let udev = root.path().join("udev");
        std::fs::create_dir_all(&udev).unwrap();
        write_device(
            &class.join("event7"),
            "SYNA Touchpad",
            "i2c-SYNA:00",
            "ceb0",
            Some("13:71"),
        );
        std::fs::write(udev.join("c13:71"), DB).unwrap();
        let group = "18/6cb/ceb0:i2c-SYNA2BA6:00";
        let clone_dir = root.path().join("virtual/event40");
        let phys = clone_phys("internal", Some(group));

        write_device(&clone_dir, "SYNA Touchpad", &phys, "ceb0", None);
        let printed = clone_properties_at(clone_dir.to_str().unwrap(), &class, &udev).unwrap();
        assert_eq!(
            printed,
            format!("ID_INPUT_TOUCHPAD_INTEGRATION=internal\nLIBINPUT_DEVICE_GROUP={group}\n")
        );

        // A device that only claims the phys, with other ids: nothing.
        write_device(&clone_dir, "SYNA Touchpad", &phys, "beef", None);
        assert_eq!(
            clone_properties_at(clone_dir.to_str().unwrap(), &class, &udev),
            None
        );

        // The right ids but a group the original does not have: nothing.
        let forged = clone_phys("internal", Some("3/1/2:usb-evil"));
        write_device(&clone_dir, "SYNA Touchpad", &forged, "ceb0", None);
        assert_eq!(
            clone_properties_at(clone_dir.to_str().unwrap(), &class, &udev),
            None
        );

        // No original at all: nothing.
        std::fs::remove_dir_all(&class).unwrap();
        std::fs::create_dir_all(&class).unwrap();
        write_device(&clone_dir, "SYNA Touchpad", &phys, "ceb0", None);
        assert_eq!(
            clone_properties_at(clone_dir.to_str().unwrap(), &class, &udev),
            None
        );
    }
}
