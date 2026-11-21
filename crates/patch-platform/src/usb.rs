//! What the USB-C entries read and ask for (ticket 101, ADR 0046): the
//! machine's sysfs through `hardware_platform::usb`, the daemon's choice
//! file, this boot's kernel log, and the daemon's typed actions.
//!
//! The kernel log is read as the person when their account can read the
//! system journal (members of `adm` and `systemd-journal` on Ubuntu, whose
//! journal files carry those groups' read access): `journalctl` through
//! [`crate::command`], classified here. Anyone else asks the daemon's
//! `Findings`, which answers with the classified findings and never a
//! line of the log. `dmesg` is not used: Ubuntu sets
//! `kernel.dmesg_restrict=1`, so it needs `CAP_SYSLOG`.

use std::path::{Path, PathBuf};

use hardware_core::usb_recovery::{
    self as core, RecoveryReply, Refusal, UsbFindings, UsbRecoverySettings,
};
use hardware_platform::sysfs::SysRoot;
use patch_core::{ChangeError, Privileged, ReadError, UsbMachineView};

/// The daemon's choice file, under the machine root.
pub const SETTINGS: &str = "var/lib/better-os/usb-recovery.json";

/// The daemon's USB-C recovery interface.
pub trait UsbDaemon: Send {
    /// This boot's findings, classified by the daemon.
    fn findings(&self) -> Result<UsbFindings, ReadError>;
    /// One typed action; what a recovery restored, when it is one.
    fn act(&self, action: Privileged) -> Result<Option<RecoveryReply>, ChangeError>;
}

/// Whether the person's account can read the system journal: the
/// journal's own `system.journal` opens for reading, in the persistent
/// journal or the volatile one, for this machine's ID.
pub fn journal_readable(root: &Path) -> bool {
    let Ok(id) = std::fs::read_to_string(root.join("etc/machine-id")) else {
        return false;
    };
    let id = id.trim();
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    ["var/log/journal", "run/log/journal"]
        .iter()
        .any(|dir| std::fs::File::open(root.join(dir).join(id).join("system.journal")).is_ok())
}

/// The machine and the choices.
pub fn machine(root: &Path) -> UsbMachineView {
    let settings = std::fs::read_to_string(root.join(SETTINGS))
        .ok()
        .and_then(|text| serde_json::from_str::<UsbRecoverySettings>(&text).ok())
        .unwrap_or_default();
    UsbMachineView {
        present: hardware_platform::usb::present(&SysRoot::at(root)),
        settings,
    }
}

/// The daemon's key in a refusal, as a change error the page words.
fn change_error(name: &str, message: &str) -> ChangeError {
    if let Some(refusal) = Refusal::from_key(message) {
        return ChangeError::Refused(refusal.key().to_string());
    }
    for key in [
        "daemon.error.auth_cancelled",
        "daemon.error.unauthorized",
        "daemon.error.busy",
        "daemon.error.shutting_down",
    ] {
        if message.contains(key) {
            return ChangeError::Refused(key.to_string());
        }
    }
    if name == "org.freedesktop.DBus.Error.ServiceUnknown" {
        return ChangeError::NotInstalled("better-manager-daemon");
    }
    ChangeError::Failed(message.to_string())
}

/// `org.betteros.UsbRecovery1` on the system bus.
pub struct SystemUsbDaemon;

const DESTINATION: &str = "org.betteros.Manager1";
const PATH: &str = "/org/betteros/Manager1";
const INTERFACE: &str = "org.betteros.UsbRecovery1";

fn call<B>(method: &str, body: &B) -> Result<String, (String, String)>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let bus = zbus::blocking::Connection::system()
        .map_err(|e| (String::new(), format!("the system bus: {e}")))?;
    let reply = bus.call_method(Some(DESTINATION), PATH, Some(INTERFACE), method, body);
    match reply {
        Ok(message) => message
            .body()
            .deserialize::<String>()
            .map_err(|e| (String::new(), e.to_string())),
        Err(zbus::Error::MethodError(name, message, _)) => {
            Err((name.to_string(), message.unwrap_or_default()))
        }
        Err(other) => Err((String::new(), other.to_string())),
    }
}

impl UsbDaemon for SystemUsbDaemon {
    fn findings(&self) -> Result<UsbFindings, ReadError> {
        let text = call("Findings", &()).map_err(|(_, message)| ReadError::Failed(message))?;
        serde_json::from_str(&text).map_err(|e| ReadError::Failed(e.to_string()))
    }

    fn act(&self, action: Privileged) -> Result<Option<RecoveryReply>, ChangeError> {
        let answer = match action {
            Privileged::ReloadUcsi => call("ReloadUcsi", &()),
            Privileged::RebindController => call("RebindController", &()),
            Privileged::SetChoice { choice, on } => call("SetChoice", &(choice.wire(), on)),
        };
        let text = answer.map_err(|(name, message)| change_error(&name, &message))?;
        match action {
            Privileged::SetChoice { .. } => Ok(None),
            _ => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| ChangeError::Failed(e.to_string())),
        }
    }
}

/// A fixture machine's daemon: the same decisions over the fixture's files
/// (`<root>/kernel.log`, the choice file, the sysfs tree), and what a
/// reload or a rebind would change there, so a demo and a screenshot show
/// the whole flow and touch nothing.
pub struct FixtureUsbDaemon {
    pub root: PathBuf,
}

/// The fixture's kernel log, as journalctl would print it.
pub const FIXTURE_LOG: &str = "kernel.log";

impl FixtureUsbDaemon {
    fn log(&self) -> String {
        std::fs::read_to_string(self.root.join(FIXTURE_LOG)).unwrap_or_default()
    }

    fn append(&self, line: &str) -> Result<(), ChangeError> {
        let mut log = self.log();
        log.push_str(line);
        log.push('\n');
        std::fs::write(self.root.join(FIXTURE_LOG), log)
            .map_err(|e| ChangeError::Failed(e.to_string()))
    }
}

impl UsbDaemon for FixtureUsbDaemon {
    fn findings(&self) -> Result<UsbFindings, ReadError> {
        Ok(core::classify(&self.log()))
    }

    fn act(&self, action: Privileged) -> Result<Option<RecoveryReply>, ChangeError> {
        let refused = |refusal: Refusal| ChangeError::Refused(refusal.key().to_string());
        let findings = core::classify(&self.log());
        let present = hardware_platform::usb::present(&SysRoot::at(&self.root));
        match action {
            Privileged::SetChoice { choice, on } => {
                let path = self.root.join(SETTINGS);
                let settings = machine(&self.root).settings.with(choice, on);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| ChangeError::Failed(e.to_string()))?;
                }
                let text = serde_json::to_string_pretty(&settings)
                    .map_err(|e| ChangeError::Failed(e.to_string()))?;
                std::fs::write(&path, text).map_err(|e| ChangeError::Failed(e.to_string()))?;
                Ok(None)
            }
            Privileged::ReloadUcsi => {
                core::ucsi_reload(&findings, &present).map_err(refused)?;
                let typec = self.root.join("sys/class/typec");
                for port in ["port0", "port1"] {
                    std::fs::create_dir_all(typec.join(port))
                        .map_err(|e| ChangeError::Failed(e.to_string()))?;
                }
                // The charger that was not shown is shown again.
                let charger = self
                    .root
                    .join("sys/class/power_supply/ucsi-source-psy-USBC000:001/online");
                if charger.is_file() {
                    std::fs::write(&charger, "1")
                        .map_err(|e| ChangeError::Failed(e.to_string()))?;
                }
                self.append(core::UCSI_RELOADED_MARKER)?;
                let after = hardware_platform::usb::present(&SysRoot::at(&self.root));
                Ok(Some(RecoveryReply {
                    restored: core::verify_ucsi(&after),
                    recent: false,
                }))
            }
            Privileged::RebindController => {
                let address = core::rebind_target(&findings, &present, false).map_err(refused)?;
                // The controller's root hubs come back, as after a real
                // rebind.
                let failed = |e: std::io::Error| ChangeError::Failed(e.to_string());
                let function = std::fs::canonicalize(
                    self.root.join("sys/bus/pci/devices").join(address.as_str()),
                )
                .map_err(failed)?;
                let usb = self.root.join("sys/bus/usb/devices");
                std::fs::create_dir_all(&usb).map_err(failed)?;
                for hub in ["usb1", "usb2"] {
                    std::fs::create_dir_all(function.join(hub)).map_err(failed)?;
                    let link = usb.join(hub);
                    if !link.exists() {
                        std::os::unix::fs::symlink(function.join(hub), link).map_err(failed)?;
                    }
                }
                self.append(&core::rebound_marker(&address))?;
                let after = hardware_platform::usb::present(&SysRoot::at(&self.root));
                Ok(Some(RecoveryReply {
                    restored: core::verify_rebind(&address, &after),
                    recent: false,
                }))
            }
        }
    }
}

/// A daemon nobody set up: every read and change says so.
pub struct NoUsbDaemon;

impl UsbDaemon for NoUsbDaemon {
    fn findings(&self) -> Result<UsbFindings, ReadError> {
        Err(ReadError::NotOnThisDesktop)
    }

    fn act(&self, _: Privileged) -> Result<Option<RecoveryReply>, ChangeError> {
        Err(ChangeError::NotInstalled("better-manager-daemon"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_journal_is_readable_only_when_its_file_opens() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!journal_readable(dir.path()));
        std::fs::create_dir_all(dir.path().join("etc")).unwrap();
        std::fs::write(dir.path().join("etc/machine-id"), "0123abcd\n").unwrap();
        assert!(!journal_readable(dir.path()));
        let journal = dir.path().join("var/log/journal/0123abcd");
        std::fs::create_dir_all(&journal).unwrap();
        std::fs::write(journal.join("system.journal"), b"").unwrap();
        assert!(journal_readable(dir.path()));
        std::fs::write(dir.path().join("etc/machine-id"), "../../x\n").unwrap();
        assert!(!journal_readable(dir.path()));
    }

    #[test]
    fn a_refusal_from_the_daemon_keeps_its_key() {
        assert_eq!(
            change_error(
                "org.freedesktop.DBus.Error.Failed",
                "daemon.error.usb_restart_needed"
            ),
            ChangeError::Refused("daemon.error.usb_restart_needed".into())
        );
        assert_eq!(
            change_error(
                "org.freedesktop.DBus.Error.AccessDenied",
                "daemon.error.auth_cancelled"
            ),
            ChangeError::Refused("daemon.error.auth_cancelled".into())
        );
        assert_eq!(
            change_error("org.freedesktop.DBus.Error.ServiceUnknown", "no"),
            ChangeError::NotInstalled("better-manager-daemon")
        );
    }
}
