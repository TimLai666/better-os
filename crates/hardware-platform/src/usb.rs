//! What the USB-C recovery reads from sysfs and `/proc` (ticket 101), under
//! a root a test points at a fixture tree. The daemon and the Fixes page
//! both read through here.
//!
//! Only directory entries, links, and the PCI function's own `vendor`,
//! `device`, and `class` are read under `bus/pci`, and nothing of a USB
//! device's own: on a host whose hub work hangs, reading a USB device's
//! attribute blocks for good (ticket 101 saw `usb1/product` do it).

use std::path::Path;

use hardware_core::usb_recovery::{
    MachineModel, PciAddress, UsbCSupply, UsbPresent, Worker, XhciFunction,
};

use crate::sysfs::SysRoot;

/// The class code of an xHCI controller (`PCI_CLASS_SERIAL_USB_XHCI`).
pub const XHCI_CLASS: u32 = 0x0c0330;

fn text(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn hex(path: &Path) -> Option<u32> {
    let text = text(path)?;
    u32::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The DMI identity.
pub fn model(root: &SysRoot) -> MachineModel {
    let dmi = root.path("sys/class/dmi/id");
    MachineModel {
        board_name: text(&dmi.join("board_name")),
        product_name: text(&dmi.join("product_name")),
        sys_vendor: text(&dmi.join("sys_vendor")),
    }
}

/// `portN` entries of `class/typec`.
pub fn typec_ports(root: &SysRoot) -> u32 {
    names(&root.path("sys/class/typec"))
        .iter()
        .filter(|name| {
            name.strip_prefix("port")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .count() as u32
}

/// The PCI functions bound to, or carrying the class of, an xHCI
/// controller.
pub fn xhci_functions(root: &SysRoot) -> Vec<XhciFunction> {
    let devices = root.path("sys/bus/pci/devices");
    let usb = root.path("sys/bus/usb/devices");
    // Every USB device's real path, once, to count those under each
    // function. Only the links are read.
    let usb_devices: Vec<(String, std::path::PathBuf)> = names(&usb)
        .into_iter()
        .filter_map(|name| {
            let real = std::fs::canonicalize(usb.join(&name)).ok()?;
            Some((name, real))
        })
        .collect();
    names(&devices)
        .into_iter()
        .filter_map(|name| {
            let address = PciAddress::parse(&name)?;
            let dir = devices.join(&name);
            if hex(&dir.join("class"))? != XHCI_CLASS {
                return None;
            }
            let vendor = hex(&dir.join("vendor"))? as u16;
            let device = hex(&dir.join("device"))? as u16;
            let driver = std::fs::read_link(dir.join("driver"))
                .ok()
                .and_then(|link| link.file_name()?.to_str().map(str::to_string));
            let real = std::fs::canonicalize(&dir).ok()?;
            let mut root_hubs = 0;
            let mut usb_count = 0;
            for (usb_name, path) in &usb_devices {
                if !path.starts_with(&real) {
                    continue;
                }
                if usb_name.starts_with("usb") {
                    root_hubs += 1;
                } else if !usb_name.contains(':') {
                    // `1-1` is a device; `1-1:1.0` is one of its interfaces.
                    usb_count += 1;
                }
            }
            Some(XhciFunction {
                address,
                vendor,
                device,
                driver,
                root_hubs,
                devices: usb_count,
            })
        })
        .collect()
}

/// Everything the recovery decides from in sysfs.
pub fn present(root: &SysRoot) -> UsbPresent {
    let supplies_dir = root.path("sys/class/power_supply");
    let mut supplies = Vec::new();
    let mut mains: Option<bool> = None;
    for name in names(&supplies_dir) {
        let dir = supplies_dir.join(&name);
        let online = text(&dir.join("online")).map(|v| v != "0");
        if name.starts_with("ucsi-source-psy-") {
            supplies.push(UsbCSupply { name, online });
        } else if text(&dir.join("type")).as_deref() == Some("Mains") {
            mains = match (mains, online) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (None, None) => None,
                _ => Some(false),
            };
        }
    }
    UsbPresent {
        model: model(root),
        typec_ports: typec_ports(root),
        supplies,
        mains_online: mains,
        controllers: xhci_functions(root),
    }
}

/// The kernel's workqueue workers (`kworker/…`) with their state, from
/// `<root>/proc`.
pub fn workers(root: &SysRoot) -> Vec<Worker> {
    let proc_dir = root.path("proc");
    names(&proc_dir)
        .into_iter()
        .filter_map(|name| {
            let pid: u32 = name.parse().ok()?;
            let dir = proc_dir.join(&name);
            let comm = text(&dir.join("comm"))?;
            if !comm.starts_with("kworker/") {
                return None;
            }
            // The state follows the last ')' of `pid (comm) S ...`.
            let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
            let state = stat.rsplit_once(')')?.1.trim_start().chars().next()?;
            Some(Worker { pid, comm, state })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The owner's machine as sysfs showed it on 2026-11-21, read without
    /// privilege: two ports, one USB-C supply online, the four xHCI
    /// functions (only 63:00.4 matters here).
    fn owner_machine(dir: &Path, root_hubs: bool) {
        let sys = dir.join("sys");
        write(&sys.join("class/dmi/id/board_name"), "M5406WA\n");
        write(
            &sys.join("class/dmi/id/product_name"),
            "ASUS Vivobook S 14 M5406WA_M5406WA\n",
        );
        for port in ["port0", "port0-partner", "port1"] {
            std::fs::create_dir_all(sys.join("class/typec").join(port)).unwrap();
        }
        for (name, kind, online) in [
            ("ACAD", "Mains", "1"),
            ("BAT1", "Battery", ""),
            ("ucsi-source-psy-USBC000:001", "USB", "1"),
            ("ucsi-source-psy-USBC000:002", "USB", "0"),
        ] {
            let d = sys.join("class/power_supply").join(name);
            write(&d.join("type"), kind);
            if !online.is_empty() {
                write(&d.join("online"), online);
            }
        }
        let real = sys.join("devices/pci0000:00/0000:00:08.1/0000:63:00.4");
        write(&real.join("vendor"), "0x1022\n");
        write(&real.join("device"), "0x151e\n");
        write(&real.join("class"), "0x0c0330\n");
        std::fs::create_dir_all(sys.join("bus/pci/drivers/xhci_hcd")).unwrap();
        std::os::unix::fs::symlink(sys.join("bus/pci/drivers/xhci_hcd"), real.join("driver"))
            .unwrap();
        std::fs::create_dir_all(sys.join("bus/pci/devices")).unwrap();
        std::os::unix::fs::symlink(&real, sys.join("bus/pci/devices/0000:63:00.4")).unwrap();
        let usb = sys.join("bus/usb/devices");
        std::fs::create_dir_all(&usb).unwrap();
        if root_hubs {
            for name in ["usb1", "usb2", "1-1", "1-1:1.0"] {
                let target = if name.starts_with("usb") {
                    real.join(name)
                } else {
                    real.join("usb1").join(name)
                };
                std::fs::create_dir_all(&target).unwrap();
                std::os::unix::fs::symlink(&target, usb.join(name)).unwrap();
            }
        }
    }

    #[test]
    fn the_owners_machine_reads_as_it_was_seen() {
        let dir = tempfile::tempdir().unwrap();
        owner_machine(dir.path(), true);
        let present = present(&SysRoot::at(dir.path()));
        assert_eq!(present.model.board_name.as_deref(), Some("M5406WA"));
        assert!(present.model.known().is_some());
        assert_eq!(present.typec_ports, 2);
        assert_eq!(present.mains_online, Some(true));
        assert_eq!(
            present.supplies,
            vec![
                UsbCSupply {
                    name: "ucsi-source-psy-USBC000:001".into(),
                    online: Some(true)
                },
                UsbCSupply {
                    name: "ucsi-source-psy-USBC000:002".into(),
                    online: Some(false)
                },
            ]
        );
        assert_eq!(
            present.controllers,
            vec![XhciFunction {
                address: PciAddress::parse("0000:63:00.4").unwrap(),
                vendor: 0x1022,
                device: 0x151e,
                driver: Some("xhci_hcd".into()),
                root_hubs: 2,
                devices: 1,
            }]
        );
    }

    #[test]
    fn a_dead_host_has_no_root_hubs_and_an_empty_tree_has_nothing() {
        let dir = tempfile::tempdir().unwrap();
        owner_machine(dir.path(), false);
        let present = present(&SysRoot::at(dir.path()));
        assert_eq!(present.controllers[0].root_hubs, 0);
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            super::present(&SysRoot::at(empty.path())),
            UsbPresent::default()
        );
    }

    #[test]
    fn workers_are_read_with_their_state() {
        let dir = tempfile::tempdir().unwrap();
        let proc_dir = dir.path().join("proc");
        write(&proc_dir.join("42/comm"), "kworker/12:2+usb_hub_wq\n");
        write(
            &proc_dir.join("42/stat"),
            "42 (kworker/12:2+usb_hub_wq) D 2 0 0 0 -1 69238880\n",
        );
        write(&proc_dir.join("7/comm"), "bash\n");
        write(&proc_dir.join("7/stat"), "7 (bash) S 1\n");
        write(&proc_dir.join("self/comm"), "x\n");
        assert_eq!(
            workers(&SysRoot::at(dir.path())),
            vec![Worker {
                pid: 42,
                comm: "kworker/12:2+usb_hub_wq".into(),
                state: 'D'
            }]
        );
    }
}
