//! The USB-C recovery on the machines whose USB-C interface and xHCI host
//! fail across a suspend (ticket 101, ADR 0046,
//! `engineering/specs/better-patch.md` entries 3 and 4).
//!
//! No I/O. `hardware-platform` reads sysfs and `/proc` into [`UsbPresent`],
//! the kernel log is read by whoever may (the person with `journalctl`, or
//! the daemon) and classified here by [`classify`], and every decision the
//! Fixes page, the daemon, and the after-sleep run take is made here, so all
//! three ask the same place:
//!
//! - which machines and controllers are known to have the fault
//!   ([`MACHINES`], kept as data in `data/usb-recovery-machines.txt`);
//! - what this boot's kernel log says ([`UsbFindings`]), never carrying a
//!   line of the log itself;
//! - whether a reload of `ucsi_acpi` or a rebind of one xHCI function is
//!   called for ([`ucsi_reload`], [`rebind_target`]), and when the only
//!   answer is a restart;
//! - what a recovery restored ([`verify_ucsi`], [`verify_rebind`]);
//! - what the after-sleep run does about a sleep the hung hub work keeps
//!   failing ([`guard`]).

use serde::{Deserialize, Serialize};

/// The machine list, as kept in the repository.
pub const MACHINES: &str = include_str!("../data/usb-recovery-machines.txt");

/// What the kernel log query matches (journalctl's `--grep`, PCRE2,
/// case-sensitive): every line [`classify`] reads, and nothing else, so a
/// long boot's log is not copied out to be thrown away.
pub const KERNEL_LOG_PATTERN: &str = concat!(
    r"^ucsi_acpi .*\(-110\)",
    r"|^xhci_hcd [0-9a-f:.]+: (HC died|PCI post-resume error|WARN: xHC restore state timeout|AMD-Vi: Event logged \[IO_PAGE_FAULT)",
    r"|^PM: suspend (entry|exit)",
    r"|^PM: Some devices failed to suspend",
    r"|^Freezing .* failed",
    r"|^workqueue usb_hub_wq:",
    r"|^Workqueue: usb_hub_wq ",
    r"|^usb-recovery: ",
);

/// The arguments of the kernel log query: this boot's kernel messages, and
/// the daemon's own recovery markers (root's, from its unit's output), as
/// plain message text. `grep` false is the same query for a journalctl
/// built without PCRE2, which refuses `--grep`.
pub fn kernel_log_args(grep: bool) -> Vec<String> {
    let mut args: Vec<String> = ["--boot=0", "--no-pager", "--quiet", "--output=cat"]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    if grep {
        args.push("--case-sensitive=true".to_string());
        args.push(format!("--grep={KERNEL_LOG_PATTERN}"));
    }
    args.extend(
        [
            "_TRANSPORT=kernel",
            "+",
            "_UID=0",
            "SYSLOG_IDENTIFIER=better-manager-daemon",
        ]
        .iter()
        .map(|arg| arg.to_string()),
    );
    args
}

/// The marker the daemon writes to its journal after a reload it verified.
pub const UCSI_RELOADED_MARKER: &str = "usb-recovery: ucsi_acpi reloaded";

/// The marker the daemon writes after a rebind it verified, followed by the
/// PCI address.
pub const REBOUND_MARKER_PREFIX: &str = "usb-recovery: controller rebound ";

/// The marker line for a rebound controller.
pub fn rebound_marker(address: &PciAddress) -> String {
    format!("{REBOUND_MARKER_PREFIX}{address}")
}

/// One PCI function's address, `dddd:bb:dd.f`, lower-case hex, as the
/// kernel prints it. Nothing else parses as one, so an address can never
/// become a path outside `/sys/bus/pci/...`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PciAddress(String);

impl PciAddress {
    pub fn parse(text: &str) -> Option<PciAddress> {
        let bytes = text.as_bytes();
        let hex = |range: std::ops::Range<usize>| {
            bytes[range]
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        };
        let ok = bytes.len() == 12
            && hex(0..4)
            && bytes[4] == b':'
            && hex(5..7)
            && bytes[7] == b':'
            && hex(8..10)
            && bytes[10] == b'.'
            && (b'0'..=b'7').contains(&bytes[11]);
        ok.then(|| PciAddress(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PciAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for PciAddress {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        PciAddress::parse(&value).ok_or_else(|| "not a PCI address".to_string())
    }
}

impl From<PciAddress> for String {
    fn from(value: PciAddress) -> String {
        value.0
    }
}

/// A known affected machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownModel {
    pub board_name: String,
    pub ports: u32,
}

/// The models and controllers of [`MACHINES`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Machines {
    pub models: Vec<KnownModel>,
    pub controllers: Vec<(u16, u16)>,
}

/// Reads the machine list. A line it cannot read is a mistake in the
/// repository, which the crate's tests catch; it is skipped, never guessed.
pub fn machines(text: &str) -> Machines {
    let mut machines = Machines::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["model", board, ports] => {
                if let Ok(ports) = ports.parse() {
                    machines.models.push(KnownModel {
                        board_name: board.to_string(),
                        ports,
                    });
                }
            }
            ["controller", ids] => {
                let parsed = ids.split_once(':').and_then(|(vendor, device)| {
                    Some((
                        u16::from_str_radix(vendor, 16).ok()?,
                        u16::from_str_radix(device, 16).ok()?,
                    ))
                });
                if let Some(ids) = parsed {
                    machines.controllers.push(ids);
                }
            }
            _ => {}
        }
    }
    machines
}

/// What the firmware says the machine is, from `/sys/class/dmi/id`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MachineModel {
    pub board_name: Option<String>,
    pub product_name: Option<String>,
    pub sys_vendor: Option<String>,
}

impl MachineModel {
    /// The known model this machine is, matched by DMI board name.
    pub fn known(&self) -> Option<KnownModel> {
        let board = self.board_name.as_deref()?.trim();
        machines(MACHINES)
            .models
            .into_iter()
            .find(|model| model.board_name.eq_ignore_ascii_case(board))
    }
}

/// Whether a PCI function is a controller of the kind seen to die.
pub fn affected_controller(vendor: u16, device: u16) -> bool {
    machines(MACHINES).controllers.contains(&(vendor, device))
}

/// The UCSI interface's timeouts this boot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UcsiFindings {
    /// `GET_CONNECTOR_STATUS failed (-110)` and any other `ucsi_acpi` line
    /// ending in `(-110)` but the one below.
    pub timeouts: u32,
    /// `failed to re-enable notifications (-110)`: written about ten
    /// seconds after a resume.
    pub notification_failures: u32,
    /// Verified reloads since boot.
    pub reloads: u32,
    /// A timeout came after the last verified reload (or there was none):
    /// the interface is failing now.
    pub active: bool,
}

/// An xHCI host the kernel gave up on this boot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ControllerDeath {
    pub address: PciAddress,
    /// `AMD-Vi: Event logged [IO_PAGE_FAULT ...]` on it just before.
    pub page_fault: bool,
    /// `WARN: xHC restore state timeout`.
    pub restore_timeout: bool,
    /// `PCI post-resume error -110!`.
    pub post_resume_error: bool,
    /// A verified rebind came after its last death.
    pub rebound: bool,
}

/// This boot's suspends, as the kernel saw them.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SleepFindings {
    pub attempts: u32,
    /// Attempts that failed because `usb_hub_wq` would not freeze.
    pub blocked_by_hub_work: u32,
    /// How many of the last attempts in a row failed for that reason.
    pub streak: u32,
}

impl SleepFindings {
    /// The attempt (counted from 1) the current run of failures began at,
    /// which names the run: a sleep that works ends it, and the next run
    /// starts at a later attempt.
    pub fn run_start(&self) -> u32 {
        (self.attempts + 1).saturating_sub(self.streak)
    }
}

/// What this boot's kernel log says, classified. Never a line of it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsbFindings {
    pub ucsi: UcsiFindings,
    pub controllers: Vec<ControllerDeath>,
    /// A `usb_hub_wq` work item that never ends: a hung-task report naming
    /// `Workqueue: usb_hub_wq`, or a failed freeze listing `usb_hub_wq`.
    pub hub_work_hung: bool,
    pub sleep: SleepFindings,
}

impl UsbFindings {
    /// The controller that died last and has not been rebound since.
    pub fn dead_controller(&self) -> Option<&ControllerDeath> {
        self.controllers.iter().rev().find(|death| !death.rebound)
    }
}

/// The PCI address an `xhci_hcd 0000:63:00.4: ...` line names.
fn xhci_address(line: &str) -> Option<PciAddress> {
    let rest = line.strip_prefix("xhci_hcd ")?;
    let (address, _) = rest.split_once(": ")?;
    PciAddress::parse(address)
}

#[derive(Default)]
struct Pending {
    page_fault: bool,
    restore_timeout: bool,
    post_resume_error: bool,
}

/// Classifies this boot's kernel log (message text, one per line, oldest
/// first, as [`kernel_log_args`] prints it).
pub fn classify(log: &str) -> UsbFindings {
    let mut findings = UsbFindings::default();
    let mut pending: std::collections::BTreeMap<PciAddress, Pending> = Default::default();
    // The attempt in progress: (failed, failed because of the hub work).
    let mut attempt: Option<(bool, bool)> = None;
    let close = |attempt: &mut Option<(bool, bool)>, sleep: &mut SleepFindings| {
        if let Some((failed, hub)) = attempt.take() {
            if failed && hub {
                sleep.blocked_by_hub_work += 1;
                sleep.streak += 1;
            } else {
                sleep.streak = 0;
            }
        }
    };
    for line in log.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("ucsi_acpi ") {
            if !rest.ends_with("(-110)") {
                continue;
            }
            if rest.contains("failed to re-enable notifications") {
                findings.ucsi.notification_failures += 1;
            } else {
                findings.ucsi.timeouts += 1;
            }
            findings.ucsi.active = true;
        } else if line == UCSI_RELOADED_MARKER {
            findings.ucsi.reloads += 1;
            findings.ucsi.active = false;
        } else if let Some(address) = line
            .strip_prefix(REBOUND_MARKER_PREFIX)
            .and_then(PciAddress::parse)
        {
            for death in findings.controllers.iter_mut() {
                if death.address == address {
                    death.rebound = true;
                }
            }
        } else if let Some(address) = xhci_address(line) {
            let entry = pending.entry(address.clone()).or_default();
            if line.contains("[IO_PAGE_FAULT") {
                entry.page_fault = true;
            } else if line.contains("xHC restore state timeout") {
                entry.restore_timeout = true;
            } else if line.contains("PCI post-resume error") {
                entry.post_resume_error = true;
            } else if line.contains("HC died") {
                let seen = pending.remove(&address).unwrap_or_default();
                findings
                    .controllers
                    .retain(|death| death.address != address);
                findings.controllers.push(ControllerDeath {
                    address,
                    page_fault: seen.page_fault,
                    restore_timeout: seen.restore_timeout,
                    post_resume_error: seen.post_resume_error,
                    rebound: false,
                });
            }
        } else if line.starts_with("PM: suspend entry") {
            close(&mut attempt, &mut findings.sleep);
            findings.sleep.attempts += 1;
            attempt = Some((false, false));
        } else if line.starts_with("PM: suspend exit") {
            close(&mut attempt, &mut findings.sleep);
        } else if (line.starts_with("Freezing ") && line.contains(" failed"))
            || line.starts_with("PM: Some devices failed to suspend")
        {
            if let Some(attempt) = attempt.as_mut() {
                attempt.0 = true;
            }
        } else if line.starts_with("workqueue usb_hub_wq:") {
            // Listed after a failed freeze: the hub work is what would not
            // freeze.
            if let Some(attempt) = attempt.as_mut()
                && attempt.0
            {
                attempt.1 = true;
                findings.hub_work_hung = true;
            }
        } else if line.starts_with("Workqueue: usb_hub_wq ") {
            findings.hub_work_hung = true;
        }
    }
    close(&mut attempt, &mut findings.sleep);
    findings
}

/// One USB-C power supply the UCSI driver registered.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsbCSupply {
    pub name: String,
    /// `None` when its `online` could not be read.
    pub online: Option<bool>,
}

/// One xHCI PCI function, as sysfs lists it. Nothing here was read from a
/// USB device's own attributes: on a host whose hub work hangs, reading
/// one blocks (ticket 101), so only directory entries and the PCI
/// function's own `vendor`, `device`, and `class` are read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XhciFunction {
    pub address: PciAddress,
    pub vendor: u16,
    pub device: u16,
    /// The driver bound to it, `xhci_hcd` when it is working.
    pub driver: Option<String>,
    /// `usbN` root hubs under it.
    pub root_hubs: u32,
    /// USB devices under it, root hubs not counted.
    pub devices: u32,
}

/// What the machine is and shows now.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsbPresent {
    pub model: MachineModel,
    /// `/sys/class/typec/portN`, partners and cables not counted.
    pub typec_ports: u32,
    pub supplies: Vec<UsbCSupply>,
    /// The mains adapter's `online` (`ACAD`, `AC`, any `Mains` supply):
    /// whether a charger is plugged in at all.
    pub mains_online: Option<bool>,
    pub controllers: Vec<XhciFunction>,
}

impl UsbPresent {
    pub fn controller(&self, address: &PciAddress) -> Option<&XhciFunction> {
        self.controllers
            .iter()
            .find(|function| &function.address == address)
    }
}

/// A kernel worker, from `/proc/<pid>/comm` and the state in
/// `/proc/<pid>/stat`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Worker {
    pub pid: u32,
    pub comm: String,
    pub state: char,
}

/// The `usb_hub_wq` workers that are inside a work item and blocked in
/// the kernel (state `D`). The kernel names a worker `kworker/N:M+WQ` while
/// it runs a work item of `WQ` and `-WQ` after (`wq_worker_comm`,
/// `kernel/workqueue.c`).
pub fn busy_hub_workers(workers: &[Worker]) -> Vec<u32> {
    workers
        .iter()
        .filter(|worker| worker.comm.ends_with("+usb_hub_wq") && worker.state == 'D')
        .map(|worker| worker.pid)
        .collect()
}

/// The hub work is stuck: the same worker is blocked inside a
/// `usb_hub_wq` work item in two looks taken seconds apart. A hub event
/// normally ends in milliseconds; one stuck behind a dead host never does,
/// and unbinding that host would block behind it.
pub fn hub_work_stuck(first: &[Worker], second: &[Worker]) -> bool {
    let first = busy_hub_workers(first);
    busy_hub_workers(second)
        .iter()
        .any(|pid| first.contains(pid))
}

/// The person's choices, kept by the daemon in
/// `/var/lib/better-os/usb-recovery.json` (readable by everyone).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsbRecoverySettings {
    pub version: u32,
    /// Reload `ucsi_acpi` after a sleep when it timed out (entry 3).
    #[serde(default)]
    pub ucsi_automatic: bool,
    /// Rebind a controller that died across a sleep, and say when the hung
    /// hub work keeps sleep from working (entry 4).
    #[serde(default)]
    pub controller_automatic: bool,
    /// With entry 4 on: also turn the PC off when sleep keeps failing with
    /// the lid closed. Off unless the person chose it.
    #[serde(default)]
    pub power_off_when_sleep_blocked: bool,
}

/// The settings file's version this build writes.
pub const SETTINGS_VERSION: u32 = 1;

impl UsbRecoverySettings {
    pub fn get(&self, choice: UsbChoice) -> bool {
        match choice {
            UsbChoice::UcsiAutomatic => self.ucsi_automatic,
            UsbChoice::ControllerAutomatic => self.controller_automatic,
            UsbChoice::PowerOffWhenSleepBlocked => self.power_off_when_sleep_blocked,
        }
    }

    /// The settings with one choice changed. Power-off needs entry 4 on, so
    /// turning entry 4 off turns it off too.
    pub fn with(mut self, choice: UsbChoice, on: bool) -> Self {
        self.version = SETTINGS_VERSION;
        match choice {
            UsbChoice::UcsiAutomatic => self.ucsi_automatic = on,
            UsbChoice::ControllerAutomatic => {
                self.controller_automatic = on;
                if !on {
                    self.power_off_when_sleep_blocked = false;
                }
            }
            UsbChoice::PowerOffWhenSleepBlocked => {
                self.power_off_when_sleep_blocked = on && self.controller_automatic;
            }
        }
        self
    }

    /// Whether the after-sleep hook has anything to do.
    pub fn any(&self) -> bool {
        self.ucsi_automatic || self.controller_automatic
    }
}

/// One of the person's choices, by its wire name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsbChoice {
    UcsiAutomatic,
    ControllerAutomatic,
    PowerOffWhenSleepBlocked,
}

impl UsbChoice {
    pub const ALL: [UsbChoice; 3] = [
        UsbChoice::UcsiAutomatic,
        UsbChoice::ControllerAutomatic,
        UsbChoice::PowerOffWhenSleepBlocked,
    ];

    pub fn wire(self) -> &'static str {
        match self {
            UsbChoice::UcsiAutomatic => "ucsi-automatic",
            UsbChoice::ControllerAutomatic => "controller-automatic",
            UsbChoice::PowerOffWhenSleepBlocked => "power-off-when-sleep-blocked",
        }
    }

    pub fn from_wire(text: &str) -> Option<UsbChoice> {
        UsbChoice::ALL
            .into_iter()
            .find(|choice| choice.wire() == text)
    }
}

/// Why a recovery does not run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Refusal {
    /// The machine is not a known affected model.
    NotAffected,
    /// Nothing is failing: no timeout since the last reload, no controller
    /// dead.
    NothingToReset,
    /// The controller the log names is not in sysfs, not of the affected
    /// kind, or not on `xhci_hcd`.
    ControllerNotRecognised,
    /// The hub work is stuck behind the dead host; only a restart frees it.
    RestartNeeded,
}

impl Refusal {
    /// The daemon's error key.
    pub fn key(self) -> &'static str {
        match self {
            Refusal::NotAffected => "daemon.error.usb_not_affected",
            Refusal::NothingToReset => "daemon.error.usb_nothing_to_reset",
            Refusal::ControllerNotRecognised => "daemon.error.usb_controller_not_recognised",
            Refusal::RestartNeeded => "daemon.error.usb_restart_needed",
        }
    }

    pub fn from_key(key: &str) -> Option<Refusal> {
        [
            Refusal::NotAffected,
            Refusal::NothingToReset,
            Refusal::ControllerNotRecognised,
            Refusal::RestartNeeded,
        ]
        .into_iter()
        .find(|refusal| key.contains(refusal.key()))
    }
}

/// Whether `ucsi_acpi` should be reloaded now.
pub fn ucsi_reload(findings: &UsbFindings, present: &UsbPresent) -> Result<(), Refusal> {
    if present.model.known().is_none() {
        return Err(Refusal::NotAffected);
    }
    if !findings.ucsi.active {
        return Err(Refusal::NothingToReset);
    }
    Ok(())
}

/// The one xHCI function a rebind may act on, decided from the log, sysfs,
/// and whether the hub work is stuck; or why none.
pub fn rebind_target(
    findings: &UsbFindings,
    present: &UsbPresent,
    hub_stuck: bool,
) -> Result<PciAddress, Refusal> {
    if present.model.known().is_none() {
        return Err(Refusal::NotAffected);
    }
    let Some(death) = findings.dead_controller() else {
        return Err(Refusal::NothingToReset);
    };
    let Some(function) = present.controller(&death.address) else {
        return Err(Refusal::ControllerNotRecognised);
    };
    if !affected_controller(function.vendor, function.device)
        || function.driver.as_deref() != Some("xhci_hcd")
    {
        return Err(Refusal::ControllerNotRecognised);
    }
    if findings.hub_work_hung || hub_stuck {
        return Err(Refusal::RestartNeeded);
    }
    Ok(death.address.clone())
}

/// Whether a USB-C charger shows up.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChargerShown {
    /// A USB-C supply reports `online=1`.
    Online,
    /// No charger is plugged in (the mains adapter is offline), so there is
    /// nothing to show.
    NoneConnected,
    /// A charger is plugged in and no USB-C supply shows it.
    NotShown,
    /// It could not be read.
    Unknown,
}

/// What a recovery restored, shown on the page and in the journal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Restored {
    Ucsi {
        ports: u32,
        expected_ports: u32,
        charger: ChargerShown,
    },
    Controller {
        address: PciAddress,
        bound: bool,
        root_hubs: u32,
        devices: u32,
    },
}

impl Restored {
    /// Everything it checks came back.
    pub fn complete(&self) -> bool {
        match self {
            Restored::Ucsi {
                ports,
                expected_ports,
                charger,
            } => ports >= expected_ports && *charger != ChargerShown::NotShown,
            Restored::Controller {
                bound, root_hubs, ..
            } => *bound && *root_hubs > 0,
        }
    }
}

/// What the daemon answers a recovery with.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoveryReply {
    pub restored: Restored,
    /// The answer of a recovery that ran moments ago, not a new one: a
    /// second call inside the cooldown is not run again.
    #[serde(default)]
    pub recent: bool,
}

fn charger(present: &UsbPresent) -> ChargerShown {
    if present.supplies.iter().any(|s| s.online == Some(true)) {
        return ChargerShown::Online;
    }
    match present.mains_online {
        Some(false) => ChargerShown::NoneConnected,
        Some(true) => ChargerShown::NotShown,
        None => ChargerShown::Unknown,
    }
}

/// What a reload restored, from a fresh look at the ports and supplies.
pub fn verify_ucsi(after: &UsbPresent) -> Restored {
    Restored::Ucsi {
        ports: after.typec_ports,
        expected_ports: after.model.known().map_or(1, |model| model.ports),
        charger: charger(after),
    }
}

/// What a rebind restored.
pub fn verify_rebind(address: &PciAddress, after: &UsbPresent) -> Restored {
    let function = after.controller(address);
    Restored::Controller {
        address: address.clone(),
        bound: function.and_then(|f| f.driver.as_deref()) == Some("xhci_hcd"),
        root_hubs: function.map_or(0, |f| f.root_hubs),
        devices: function.map_or(0, |f| f.devices),
    }
}

/// What the after-sleep run sees around a blocked sleep.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GuardWorld {
    /// logind's `LidClosed`; `None` when it could not say.
    pub lid_closed: Option<bool>,
    /// A `block` inhibitor on `shutdown` is held (an upgrade, a burn).
    pub shutdown_blocked: bool,
    /// Someone other than the people being told has a session.
    pub other_people: bool,
    /// The run of failures this boot's notification was already sent for,
    /// by [`SleepFindings::run_start`], if any.
    pub notified_run: Option<u32>,
}

/// What the after-sleep run does about the blocked sleep.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GuardAction {
    pub notify: bool,
    pub power_off: bool,
}

/// How many failed attempts in a row it takes.
pub const BLOCKED_STREAK: u32 = 2;

/// The overnight loop (ticket 101): once a sleep has failed twice in a row
/// because `usb_hub_wq` would not freeze, tell the person, once per run of
/// failures. Turn the PC off only when they chose it, the lid is closed
/// (logind says so, not a guess), nothing holds a block on shutdown, and
/// nobody else has a session whose work it would end.
pub fn guard(
    findings: &UsbFindings,
    settings: &UsbRecoverySettings,
    world: GuardWorld,
) -> GuardAction {
    if !settings.controller_automatic || findings.sleep.streak < BLOCKED_STREAK {
        return GuardAction::default();
    }
    let notify = world.notified_run != Some(findings.sleep.run_start());
    let power_off = settings.power_off_when_sleep_blocked
        && world.lid_closed == Some(true)
        && !world.shutdown_blocked
        && !world.other_people;
    GuardAction { notify, power_off }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_machine_list_reads_every_line() {
        let list = machines(MACHINES);
        assert_eq!(
            list.models,
            vec![
                KnownModel {
                    board_name: "M5406WA".into(),
                    ports: 2
                },
                KnownModel {
                    board_name: "M5506WA".into(),
                    ports: 2
                },
            ]
        );
        assert_eq!(list.controllers, vec![(0x1022, 0x151e)]);
        let lines = MACHINES
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .count();
        assert_eq!(lines, list.models.len() + list.controllers.len());
    }

    #[test]
    fn a_pci_address_is_exactly_the_kernels_form() {
        assert!(PciAddress::parse("0000:63:00.4").is_some());
        for bad in [
            "0000:63:00.8",
            "0000:63:00",
            "0000:63:00.4/",
            "../63:00.4xx",
            "0000:6G:00.4",
            "0000:63:00.4\n",
            "",
        ] {
            assert!(PciAddress::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_model_is_known_by_board_name_only() {
        let model = |board: &str| MachineModel {
            board_name: Some(board.into()),
            ..MachineModel::default()
        };
        assert!(model("M5406WA").known().is_some());
        assert!(model("m5506wa\n").known().is_some());
        assert!(model("M5406").known().is_none());
        assert!(MachineModel::default().known().is_none());
    }

    #[test]
    fn settings_keep_power_off_behind_entry_four() {
        let none = UsbRecoverySettings::default();
        assert!(
            !none
                .with(UsbChoice::PowerOffWhenSleepBlocked, true)
                .power_off_when_sleep_blocked
        );
        let on = none
            .with(UsbChoice::ControllerAutomatic, true)
            .with(UsbChoice::PowerOffWhenSleepBlocked, true);
        assert!(on.power_off_when_sleep_blocked);
        assert!(
            !on.with(UsbChoice::ControllerAutomatic, false)
                .power_off_when_sleep_blocked
        );
        for choice in UsbChoice::ALL {
            assert_eq!(UsbChoice::from_wire(choice.wire()), Some(choice));
        }
        assert_eq!(UsbChoice::from_wire("anything"), None);
    }

    #[test]
    fn a_refusal_key_is_found_inside_a_dbus_message() {
        for refusal in [
            Refusal::NotAffected,
            Refusal::NothingToReset,
            Refusal::ControllerNotRecognised,
            Refusal::RestartNeeded,
        ] {
            assert_eq!(
                Refusal::from_key(&format!(
                    "org.freedesktop.DBus.Error.AccessDenied: {}",
                    refusal.key()
                )),
                Some(refusal)
            );
        }
    }
}
