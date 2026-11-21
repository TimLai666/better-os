//! The USB-C recovery (ticket 101, ADR 0046): `org.betteros.UsbRecovery1`,
//! its two helper units, and the after-sleep run.
//!
//! Every decision is `hardware_core::usb_recovery`'s; this module reads the
//! machine for it and acts:
//!
//! - **Reload `ucsi_acpi`.** In a transient unit that may load and unload
//!   modules and nothing else: `--reload-ucsi` calls `delete_module(2)` on
//!   the fixed name and `finit_module(2)` on the distribution's own file
//!   under `/lib/modules/$(uname -r)`, which the kernel checks against its
//!   signing keys (lockdown under Secure Boot refuses anything unsigned).
//! - **Rebind one xHCI function.** In a transient unit whose `/sys` holds
//!   `xhci_hcd`'s `unbind` and `bind` and the function's own identity files:
//!   `--rebind-xhci ADDRESS`, for the one address the daemon decided from
//!   this boot's log and sysfs, never one a caller sent.
//! - **After a sleep.** The package's `systemd-sleep` hook runs
//!   `--after-sleep PHASE OPERATION`, which queues one transient unit,
//!   ordered after the sleep service, running `--usb-after-sleep`: the same
//!   reload and rebind when the person turned them on, and the overnight
//!   guard (notify, or power off when chosen and safe).
//!
//! Each verified recovery writes a marker to the daemon's journal, which the
//! kernel log query reads back, so "failing now" means a timeout after the
//! last verified reload.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hardware_core::usb_recovery::{
    self as core, GuardWorld, PciAddress, RecoveryReply, Restored, UsbChoice, UsbFindings,
    UsbPresent, UsbRecoverySettings, Worker,
};
use hardware_platform::sysfs::SysRoot;
use zbus::message::Header;
use zbus::{fdo, interface};

use crate::DaemonError;
use crate::authorize::{
    Authorization, Authorizer, USB_C_RELOAD_ACTION, USB_CONTROLLER_REBIND_ACTION,
    USB_RECOVERY_SETTINGS_ACTION,
};
use crate::service::{CallGate, refuse};

/// The person's choices (readable by everyone, written only here).
pub const SETTINGS: &str = "/var/lib/better-os/usb-recovery.json";

/// What the after-sleep run told the person this boot.
pub const RUN_STATE: &str = "/run/better-os/usb-recovery.json";

/// The directory [`RUN_STATE`] is in.
pub const RUN_STATE_DIR: &str = "/run/better-os";

/// journalctl, by its absolute path.
pub const JOURNALCTL: &str = "/usr/bin/journalctl";

/// The one module the reload helper may unload and load.
pub const UCSI_MODULE: &str = "ucsi_acpi";

/// Where the distribution keeps it under `/lib/modules/<release>`.
pub const UCSI_MODULE_DIR: &str = "kernel/drivers/usb/typec/ucsi";

/// The after-sleep run's unit. A fixed name, so a run already queued or
/// running takes the place of a second one.
pub const AFTER_SLEEP_UNIT: &str = "better-os-usb-after-sleep";

/// The helpers' units. Fixed names: two reloads, or two rebinds, never run
/// at once, whoever started them (the window's call or the after-sleep
/// run): systemd refuses a second unit with a name that exists.
pub const UCSI_RELOAD_UNIT: &str = "better-os-ucsi-reload";
pub const XHCI_REBIND_UNIT: &str = "better-os-xhci-rebind";

/// The sleep services the after-sleep run is ordered after.
pub const SLEEP_SERVICES: [&str; 4] = [
    "systemd-suspend.service",
    "systemd-hibernate.service",
    "systemd-hybrid-sleep.service",
    "systemd-suspend-then-hibernate.service",
];

/// The largest kernel log the daemon reads, after `--grep`.
pub const MAX_LOG_BYTES: usize = 8 * 1024 * 1024;

/// The shortest time between two reloads, or two rebinds: a call inside it
/// gets the last answer back (`recent`), not a second run.
pub const RELOAD_COOLDOWN: Duration = Duration::from_secs(30);
pub const REBIND_COOLDOWN: Duration = Duration::from_secs(60);

/// How long the findings a caller asked for are given to the next caller.
pub const FINDINGS_FRESH: Duration = Duration::from_secs(5);

/// How long the recovery waits.
#[derive(Clone, Copy, Debug)]
pub struct Timings {
    /// Between two looks while waiting for the ports or the hubs.
    pub poll: Duration,
    /// How long the ports get to come back after a reload.
    pub ucsi_settle: Duration,
    /// How long the hubs get after a rebind.
    pub rebind_settle: Duration,
    /// Between the two looks at the hub workers.
    pub worker_gap: Duration,
    /// How long the after-sleep run watches for the UCSI timeout, which
    /// the kernel writes about ten seconds after a resume.
    pub ucsi_watch: Duration,
}

impl Timings {
    pub const REAL: Timings = Timings {
        poll: Duration::from_millis(500),
        ucsi_settle: Duration::from_secs(10),
        rebind_settle: Duration::from_secs(15),
        worker_gap: Duration::from_secs(2),
        ucsi_watch: Duration::from_secs(25),
    };

    pub const INSTANT: Timings = Timings {
        poll: Duration::ZERO,
        ucsi_settle: Duration::ZERO,
        rebind_settle: Duration::ZERO,
        worker_gap: Duration::ZERO,
        ucsi_watch: Duration::ZERO,
    };
}

/// The machine, as the recovery reads and changes it.
pub trait UsbMachine: Send + Sync {
    /// This boot's kernel log, as `core::kernel_log_args` prints it.
    fn kernel_log(&self) -> Result<String, DaemonError>;
    fn present(&self) -> UsbPresent;
    fn workers(&self) -> Vec<Worker>;
    fn pause(&self, duration: Duration);
    /// Runs the reload helper.
    fn reload_ucsi(&self) -> Result<(), DaemonError>;
    /// Runs the rebind helper for one function.
    fn rebind(&self, address: &PciAddress) -> Result<(), DaemonError>;
    /// Writes a marker line to the daemon's journal.
    fn mark(&self, line: &str);
}

/// The real machine: sysfs and `/proc` at `/`, journalctl, and the helpers
/// in transient units started with systemd-run.
pub struct SystemUsbMachine {
    pub root: SysRoot,
    /// This program, which the helper units run.
    pub program: PathBuf,
}

impl SystemUsbMachine {
    pub fn new(program: PathBuf) -> SystemUsbMachine {
        SystemUsbMachine {
            root: SysRoot::system(),
            program,
        }
    }
}

/// Runs journalctl with the kernel log query; without `--grep` when the
/// journal was built without PCRE2.
pub fn read_kernel_log(program: &str) -> Result<String, DaemonError> {
    let run = |grep: bool| {
        std::process::Command::new(program)
            .args(core::kernel_log_args(grep))
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .output()
    };
    let output = run(true).map_err(|e| DaemonError::KernelLogUnreadable(e.to_string()))?;
    let output = if output.status.success() {
        output
    } else {
        run(false).map_err(|e| DaemonError::KernelLogUnreadable(e.to_string()))?
    };
    if !output.status.success() {
        return Err(DaemonError::KernelLogUnreadable(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let mut bytes = output.stdout;
    if bytes.len() > MAX_LOG_BYTES {
        // The newest lines decide what is failing now.
        bytes.drain(..bytes.len() - MAX_LOG_BYTES);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn run_helper(
    unit: &str,
    description: &str,
    properties: Vec<String>,
    program: &Path,
    args: &[&str],
) -> Result<(), DaemonError> {
    let mut command = std::process::Command::new("systemd-run");
    command.args([
        "--quiet",
        "--wait",
        "--pipe",
        "--collect",
        "--service-type=exec",
    ]);
    command.arg(format!("--unit={unit}"));
    command.arg(format!("--description={description}"));
    for property in properties {
        command.arg("-p").arg(property);
    }
    command.arg(program).args(args);
    command.stdin(std::process::Stdio::null());
    let output = command
        .output()
        .map_err(|e| DaemonError::UsbRecoveryFailed(format!("systemd-run: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    // A unit with that name exists: another recovery of the same kind is
    // running right now.
    if stderr.contains("already exists") || stderr.contains("already loaded") {
        return Err(DaemonError::Busy);
    }
    Err(DaemonError::UsbRecoveryFailed(stderr))
}

impl UsbMachine for SystemUsbMachine {
    fn kernel_log(&self) -> Result<String, DaemonError> {
        read_kernel_log(JOURNALCTL)
    }

    fn present(&self) -> UsbPresent {
        hardware_platform::usb::present(&self.root)
    }

    fn workers(&self) -> Vec<Worker> {
        hardware_platform::usb::workers(&self.root)
    }

    fn pause(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn reload_ucsi(&self) -> Result<(), DaemonError> {
        run_helper(
            UCSI_RELOAD_UNIT,
            "Better OS USB-C interface reload",
            ucsi_helper_properties(),
            &self.program,
            &["--reload-ucsi"],
        )
    }

    fn rebind(&self, address: &PciAddress) -> Result<(), DaemonError> {
        run_helper(
            XHCI_REBIND_UNIT,
            "Better OS USB controller rebind",
            xhci_helper_properties(address),
            &self.program,
            &["--rebind-xhci", address.as_str()],
        )
    }

    fn mark(&self, line: &str) {
        // The unit's output is its journal, under this program's name and
        // root's uid: what the kernel log query reads markers from.
        eprintln!("{line}");
    }
}

/// The reload helper's unit: the hardware helpers' sandbox
/// (`hardware::helper_sandbox`, ADR 0029 and 0034), with module loading
/// let in and nothing else: `ProtectKernelModules=` (which takes away the
/// module system calls, `CAP_SYS_MODULE`, and `/lib/modules`) left out, the
/// `@module` system calls allowed, and `CAP_SYS_MODULE` as the only
/// capability.
pub fn ucsi_helper_properties() -> Vec<String> {
    let mut properties: Vec<String> = crate::hardware::helper_sandbox()
        .into_iter()
        .filter(|p| p != "ProtectKernelModules=yes" && !p.starts_with("RuntimeMaxSec="))
        .collect();
    properties.push("SystemCallFilter=@module".to_string());
    properties.push("CapabilityBoundingSet=CAP_SYS_MODULE".to_string());
    // A stuck interface can hold an unload for its own command timeout.
    properties.push("RuntimeMaxSec=30s".to_string());
    properties
}

/// The rebind helper's unit: the same sandbox, with `/sys` holding the
/// driver's `unbind` and `bind` and, read-only, the function's `vendor`,
/// `device`, and `class`, and no capability at all.
pub fn xhci_helper_properties(address: &PciAddress) -> Vec<String> {
    let mut properties: Vec<String> = crate::hardware::helper_sandbox()
        .into_iter()
        .filter(|p| !p.starts_with("RuntimeMaxSec="))
        .collect();
    let mut at = 1;
    for file in ["unbind", "bind"] {
        let path = PathBuf::from(format!("/sys/bus/pci/drivers/xhci_hcd/{file}"));
        properties.insert(
            at,
            format!("BindPaths={}", crate::hardware::escape_bind_path(&path)),
        );
        at += 1;
    }
    for file in ["vendor", "device", "class"] {
        let path = PathBuf::from(format!("/sys/bus/pci/devices/{address}/{file}"));
        properties.insert(
            at,
            format!(
                "BindReadOnlyPaths={}",
                crate::hardware::escape_bind_path(&path)
            ),
        );
        at += 1;
    }
    properties.push("CapabilityBoundingSet=".to_string());
    properties.push("RuntimeMaxSec=30s".to_string());
    properties
}

/// The after-sleep run's unit: not a hardware helper, so `/sys` and the
/// journal are read whole, but nothing is writable except `/run/better-os`,
/// no module can be loaded, no network reached, and its only capabilities
/// are the two it needs to tell a person's session as that person.
pub fn after_sleep_properties() -> Vec<String> {
    let mut properties: Vec<String> = SLEEP_SERVICES
        .iter()
        .map(|service| format!("After={service}"))
        .collect();
    properties.extend(
        [
            "ProtectSystem=strict",
            "ReadWritePaths=-/run/better-os",
            "ProtectHome=read-only",
            "PrivateTmp=yes",
            "IPAddressDeny=any",
            "RestrictAddressFamilies=AF_UNIX",
            "ProtectKernelModules=yes",
            "ProtectKernelTunables=yes",
            "ProtectKernelLogs=yes",
            "ProtectClock=yes",
            "ProtectHostname=yes",
            "ProtectControlGroups=yes",
            "NoNewPrivileges=yes",
            "RestrictSUIDSGID=yes",
            "RestrictRealtime=yes",
            "RestrictNamespaces=yes",
            "LockPersonality=yes",
            "MemoryDenyWriteExecute=yes",
            "SystemCallArchitectures=native",
            "SystemCallFilter=@system-service",
            "CapabilityBoundingSet=CAP_SETUID CAP_SETGID",
            "RuntimeMaxSec=120s",
        ]
        .iter()
        .map(|p| p.to_string()),
    );
    properties
}

/// This boot's findings.
pub fn findings(machine: &dyn UsbMachine) -> Result<UsbFindings, DaemonError> {
    Ok(core::classify(&machine.kernel_log()?))
}

/// Looks until `done` holds or `settle` has passed; the last look.
fn settle(
    machine: &dyn UsbMachine,
    timings: Timings,
    settle: Duration,
    done: impl Fn(&UsbPresent) -> bool,
) -> UsbPresent {
    let started = Instant::now();
    loop {
        let present = machine.present();
        if done(&present) || started.elapsed() >= settle {
            return present;
        }
        machine.pause(timings.poll);
    }
}

/// Reloads `ucsi_acpi` when this boot's log says the interface is failing
/// now, and reports what came back. A reload that brought every port back
/// is marked in the journal.
pub fn reload(machine: &dyn UsbMachine, timings: Timings) -> Result<Restored, DaemonError> {
    let findings = findings(machine)?;
    core::ucsi_reload(&findings, &machine.present()).map_err(DaemonError::UsbRefused)?;
    machine.reload_ucsi()?;
    let after = settle(machine, timings, timings.ucsi_settle, |present| {
        core::verify_ucsi(present).complete()
    });
    let restored = core::verify_ucsi(&after);
    if let Restored::Ucsi {
        ports,
        expected_ports,
        ..
    } = &restored
        && ports >= expected_ports
    {
        machine.mark(core::UCSI_RELOADED_MARKER);
    }
    eprintln!("usb-recovery: reload result {restored:?}");
    Ok(restored)
}

/// Whether the hub work is stuck: two looks at the workers, apart.
pub fn hub_stuck(machine: &dyn UsbMachine, timings: Timings) -> bool {
    let first = machine.workers();
    if core::busy_hub_workers(&first).is_empty() {
        return false;
    }
    machine.pause(timings.worker_gap);
    core::hub_work_stuck(&first, &machine.workers())
}

/// Rebinds the one controller this boot's log names as dead, when sysfs
/// shows it is of the affected kind and the hub work is not stuck; a stuck
/// hub is a restart, and nothing is touched.
pub fn rebind(machine: &dyn UsbMachine, timings: Timings) -> Result<Restored, DaemonError> {
    let findings = findings(machine)?;
    let stuck = hub_stuck(machine, timings);
    let address = core::rebind_target(&findings, &machine.present(), stuck)
        .map_err(DaemonError::UsbRefused)?;
    machine.rebind(&address)?;
    let after = settle(machine, timings, timings.rebind_settle, |present| {
        core::verify_rebind(&address, present).complete()
    });
    let restored = core::verify_rebind(&address, &after);
    if restored.complete() {
        machine.mark(&core::rebound_marker(&address));
    }
    eprintln!("usb-recovery: rebind result {restored:?}");
    Ok(restored)
}

/// The person's choices; none until they made one.
pub fn load_settings(path: &Path) -> UsbRecoverySettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_whole(path: &Path, text: &str) -> Result<(), DaemonError> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let dir = path
        .parent()
        .ok_or_else(|| DaemonError::WriteFailed(path.display().to_string()))?;
    std::fs::create_dir_all(dir).map_err(|e| DaemonError::WriteFailed(e.to_string()))?;
    let temporary = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("usb")
    ));
    let _ = std::fs::remove_file(&temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&temporary)
        .map_err(|e| DaemonError::WriteFailed(e.to_string()))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| DaemonError::WriteFailed(e.to_string()))?;
    std::fs::rename(&temporary, path).map_err(|e| DaemonError::WriteFailed(e.to_string()))?;
    if let Ok(dir) = std::fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Changes one choice and keeps the settings, or removes the file when
/// nothing is on, so a machine with nothing chosen holds nothing.
pub fn set_choice(
    path: &Path,
    choice: UsbChoice,
    on: bool,
) -> Result<UsbRecoverySettings, DaemonError> {
    let settings = load_settings(path).with(choice, on);
    if settings.any() || settings.power_off_when_sleep_blocked {
        let text = serde_json::to_string_pretty(&settings)
            .map_err(|e| DaemonError::WriteFailed(e.to_string()))?;
        write_whole(path, &text)?;
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(DaemonError::WriteFailed(error.to_string())),
        }
    }
    Ok(load_settings(path))
}

/// Forgets every choice: Better Manager's or the daemon package's removal.
pub fn release(settings: &Path, run_state: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for path in [settings, run_state] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => problems.push(format!("{}: {error}", path.display())),
        }
    }
    problems
}

/// One recovery at a time per kind, and no second run inside the
/// cooldown: a caller in a loop gets the last answer back.
#[derive(Default)]
pub struct Limiter {
    last: tokio::sync::Mutex<Option<(Instant, Restored)>>,
}

impl Limiter {
    /// Runs `work` unless one ran inside `cooldown`; waits while one runs,
    /// on an async lock, so a caller in a loop holds no thread.
    pub async fn run<F>(
        &self,
        cooldown: Duration,
        runtime: &tokio::runtime::Handle,
        work: F,
    ) -> Result<RecoveryReply, DaemonError>
    where
        F: FnOnce() -> Result<Restored, DaemonError> + Send + 'static,
    {
        let mut last = self.last.lock().await;
        if let Some((at, restored)) = last.as_ref()
            && at.elapsed() < cooldown
        {
            return Ok(RecoveryReply {
                restored: restored.clone(),
                recent: true,
            });
        }
        let restored = runtime
            .spawn_blocking(work)
            .await
            .map_err(|error| DaemonError::UsbRecoveryFailed(error.to_string()))??;
        *last = Some((Instant::now(), restored.clone()));
        Ok(RecoveryReply {
            restored,
            recent: false,
        })
    }
}

/// Recent findings, given to the next caller instead of a second read.
#[derive(Default)]
struct FindingsCache {
    lock: tokio::sync::Mutex<Option<(Instant, UsbFindings)>>,
    waiting: std::sync::atomic::AtomicUsize,
}

/// How many findings callers may wait before one is refused as busy.
pub const FINDINGS_WAITERS: usize = 8;

/// `org.betteros.UsbRecovery1`.
pub struct UsbRecoveryService<A: Authorizer + 'static> {
    authorizer: A,
    machine: Arc<dyn UsbMachine>,
    settings: PathBuf,
    gate: Arc<CallGate>,
    runtime: tokio::runtime::Handle,
    timings: Timings,
    reloads: Limiter,
    rebinds: Limiter,
    cache: FindingsCache,
}

impl<A: Authorizer + 'static> UsbRecoveryService<A> {
    pub fn new(
        authorizer: A,
        machine: Arc<dyn UsbMachine>,
        settings: impl Into<PathBuf>,
        gate: Arc<CallGate>,
    ) -> Self {
        Self {
            authorizer,
            machine,
            settings: settings.into(),
            gate,
            runtime: tokio::runtime::Handle::current(),
            timings: Timings::REAL,
            reloads: Limiter::default(),
            rebinds: Limiter::default(),
            cache: FindingsCache::default(),
        }
    }

    pub fn with_timings(mut self, timings: Timings) -> Self {
        self.timings = timings;
        self
    }

    async fn authorize(&self, header: &Header<'_>, action: &'static str) -> Result<(), fdo::Error> {
        match self.authorizer.decide(header, action).await {
            Ok(Authorization::Granted) => Ok(()),
            Ok(Authorization::Dismissed) => Err(refuse(DaemonError::AuthCancelled)),
            Ok(Authorization::Denied) => Err(refuse(DaemonError::Unauthorized)),
            Err(error) => Err(refuse(error)),
        }
    }

    fn reply(reply: Result<RecoveryReply, DaemonError>) -> Result<String, fdo::Error> {
        let reply = reply.map_err(refuse)?;
        serde_json::to_string(&reply).map_err(|e| fdo::Error::Failed(e.to_string()))
    }
}

#[interface(name = "org.betteros.UsbRecovery1")]
impl<A: Authorizer + 'static> UsbRecoveryService<A> {
    /// This boot's kernel log, classified (`UsbFindings` as JSON): never a
    /// line of the log. For a person whose account cannot read the
    /// system journal. Ungated, like `GetStatus`; callers share one read
    /// for a few seconds, and past eight waiting a call is busy.
    async fn findings(&self) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        let before = self
            .cache
            .waiting
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        struct Leave<'a>(&'a std::sync::atomic::AtomicUsize);
        impl Drop for Leave<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _leave = Leave(&self.cache.waiting);
        if before >= FINDINGS_WAITERS {
            return Err(refuse(DaemonError::Busy));
        }
        let mut cached = self.cache.lock.lock().await;
        let findings = match cached.as_ref() {
            Some((at, findings)) if at.elapsed() < FINDINGS_FRESH => findings.clone(),
            _ => {
                let machine = self.machine.clone();
                let read = self
                    .runtime
                    .spawn_blocking(move || findings(machine.as_ref()))
                    .await
                    .map_err(|e| fdo::Error::Failed(e.to_string()))?
                    .map_err(refuse)?;
                *cached = Some((Instant::now(), read.clone()));
                read
            }
        };
        serde_json::to_string(&findings).map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// The person's choices (`UsbRecoverySettings` as JSON). Ungated: the
    /// file is readable by everyone.
    async fn settings(&self) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        serde_json::to_string(&load_settings(&self.settings))
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Reloads `ucsi_acpi` when the interface is failing now; takes no
    /// argument. Returns what came back (`RecoveryReply` as JSON).
    async fn reload_ucsi(&self, #[zbus(header)] header: Header<'_>) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        self.authorize(&header, USB_C_RELOAD_ACTION).await?;
        let machine = self.machine.clone();
        let timings = self.timings;
        let reply = self
            .reloads
            .run(RELOAD_COOLDOWN, &self.runtime, move || {
                reload(machine.as_ref(), timings)
            })
            .await;
        *self.cache.lock.lock().await = None;
        Self::reply(reply)
    }

    /// Rebinds the one controller this boot's log names as dead; takes no
    /// argument. A stuck hub is refused as `usb_restart_needed`.
    async fn rebind_controller(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        self.authorize(&header, USB_CONTROLLER_REBIND_ACTION)
            .await?;
        let machine = self.machine.clone();
        let timings = self.timings;
        let reply = self
            .rebinds
            .run(REBIND_COOLDOWN, &self.runtime, move || {
                rebind(machine.as_ref(), timings)
            })
            .await;
        *self.cache.lock.lock().await = None;
        Self::reply(reply)
    }

    /// Turns one choice on or off (`ucsi-automatic`,
    /// `controller-automatic`, `power-off-when-sleep-blocked`) and returns
    /// the settings as read back. The choice is not echoed.
    async fn set_choice(
        &self,
        #[zbus(header)] header: Header<'_>,
        choice: &str,
        on: bool,
    ) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        let Some(choice) = UsbChoice::from_wire(choice) else {
            return Err(refuse(DaemonError::InvalidArgument("choice".to_string())));
        };
        self.authorize(&header, USB_RECOVERY_SETTINGS_ACTION)
            .await?;
        let path = self.settings.clone();
        let settings = self
            .runtime
            .spawn_blocking(move || set_choice(&path, choice, on))
            .await
            .map_err(|e| fdo::Error::Failed(e.to_string()))?
            .map_err(refuse)?;
        serde_json::to_string(&settings).map_err(|e| fdo::Error::Failed(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// The helpers, run inside their units.
// ---------------------------------------------------------------------------

/// The module file of `ucsi_acpi` for a kernel release, under `modules`
/// (`/lib/modules` on a real machine): the distribution's own, compressed
/// or not. A release that could leave the directory is refused.
pub fn ucsi_module_file(modules: &Path, release: &str) -> Option<(PathBuf, bool)> {
    if release.is_empty() || release.contains('/') || release.starts_with('.') {
        return None;
    }
    let dir = modules.join(release).join(UCSI_MODULE_DIR);
    for (suffix, compressed) in [
        (".ko.zst", true),
        (".ko.xz", true),
        (".ko.gz", true),
        (".ko", false),
    ] {
        let path = dir.join(format!("{UCSI_MODULE}{suffix}"));
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_file() {
            return Some((path, compressed));
        }
    }
    None
}

fn kernel_release() -> Option<String> {
    // SAFETY: `uname` fills a zeroed struct it is given.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut name) } != 0 {
        return None;
    }
    // SAFETY: the kernel writes a NUL-terminated string.
    let release = unsafe { std::ffi::CStr::from_ptr(name.release.as_ptr()) };
    release.to_str().ok().map(str::to_string)
}

/// `finit_module(2)`'s flag for a compressed file the kernel decompresses
/// (`MODULE_INIT_COMPRESSED_FILE`, Linux 5.17).
const MODULE_INIT_COMPRESSED_FILE: libc::c_int = 4;

/// The reload helper: `delete_module(2)` on `ucsi_acpi`, then
/// `finit_module(2)` on the distribution's file. Takes nothing from
/// outside: the name is fixed, the file is found under `/lib/modules` for
/// the running kernel, and the kernel checks its signature.
pub fn reload_ucsi_main() -> Result<String, String> {
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::unix::io::AsRawFd as _;
    let name = std::ffi::CString::new(UCSI_MODULE).expect("no NUL in a fixed name");
    // SAFETY: a NUL-terminated name and flags, as delete_module(2) takes.
    let removed = unsafe {
        libc::syscall(
            libc::SYS_delete_module,
            name.as_ptr(),
            libc::O_NONBLOCK as libc::c_long,
        )
    };
    if removed != 0 {
        let error = std::io::Error::last_os_error();
        // Not loaded: loading it is all there is to do.
        if error.raw_os_error() != Some(libc::ENOENT) {
            return Err(format!("{UCSI_MODULE} could not be unloaded: {error}"));
        }
    }
    let release = kernel_release().ok_or("the kernel release is unknown")?;
    let (path, compressed) = ucsi_module_file(Path::new("/lib/modules"), &release)
        .ok_or_else(|| format!("{UCSI_MODULE} is not under /lib/modules/{release}"))?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return Err(format!("{} is not root's own file", path.display()));
    }
    let params = std::ffi::CString::new("").expect("empty");
    let flags = if compressed {
        MODULE_INIT_COMPRESSED_FILE
    } else {
        0
    };
    let mut last = None;
    for _ in 0..2 {
        // SAFETY: an open descriptor, an empty NUL-terminated parameter
        // string, and flags, as finit_module(2) takes.
        let loaded = unsafe {
            libc::syscall(
                libc::SYS_finit_module,
                file.as_raw_fd(),
                params.as_ptr(),
                flags as libc::c_long,
            )
        };
        if loaded == 0 {
            return Ok("reloaded".to_string());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EEXIST) {
            return Ok("loaded".to_string());
        }
        last = Some(error);
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "{UCSI_MODULE} was unloaded and could not be loaded again: {}",
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn read_hex(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

fn write_sysfs(path: &Path, value: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.write_all(value.as_bytes())
}

/// The rebind helper, under `root` (`/` in its unit, a fixture in a test):
/// checks again that the address is an xHCI function of the affected kind,
/// unbinds it from `xhci_hcd` (one that is already unbound is left
/// unbound), and binds it again.
pub fn rebind_xhci_main(root: &Path, address: &str) -> Result<String, String> {
    let address = PciAddress::parse(address).ok_or("not a PCI address")?;
    let device = root.join(format!("sys/bus/pci/devices/{address}"));
    let ids = (
        read_hex(&device.join("vendor")),
        read_hex(&device.join("device")),
        read_hex(&device.join("class")),
    );
    let (Some(vendor), Some(product), Some(class)) = ids else {
        return Err(format!("{address} is not readable here"));
    };
    if class != hardware_platform::usb::XHCI_CLASS
        || !core::affected_controller(vendor as u16, product as u16)
    {
        return Err(format!(
            "{address} is not a controller of the affected kind"
        ));
    }
    let driver = root.join("sys/bus/pci/drivers/xhci_hcd");
    match write_sysfs(&driver.join("unbind"), address.as_str()) {
        Ok(()) => {}
        // Not bound: there is nothing to unbind.
        Err(error) if error.raw_os_error() == Some(libc::ENODEV) => {}
        Err(error) => return Err(format!("unbind {address}: {error}")),
    }
    write_sysfs(&driver.join("bind"), address.as_str())
        .map_err(|error| format!("bind {address}: {error}"))?;
    Ok("rebound".to_string())
}

// ---------------------------------------------------------------------------
// After a sleep.
// ---------------------------------------------------------------------------

/// Whether the hook's two arguments are systemd-sleep's own.
pub fn sleep_hook_arguments(phase: &str, operation: &str) -> bool {
    matches!(phase, "pre" | "post")
        && matches!(
            operation,
            "suspend" | "hibernate" | "hybrid-sleep" | "suspend-then-hibernate"
        )
}

/// The hook: with anything turned on, queue the after-sleep run, ordered
/// after the sleep service so it runs once the attempt is over, and return
/// at once, never holding the sleep or failing it. A run already queued is
/// the run.
pub fn after_sleep_hook(program: &Path, settings: &Path, phase: &str, operation: &str) {
    if !sleep_hook_arguments(phase, operation) || !load_settings(settings).any() {
        return;
    }
    // Where the run keeps what it told the person: Better Manager's
    // tmpfiles entry makes it at boot, and a machine with only this
    // package gets it here, as that entry would (root, 0755).
    if std::fs::create_dir_all(RUN_STATE_DIR).is_ok() {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(RUN_STATE_DIR, std::fs::Permissions::from_mode(0o755));
    }
    let mut command = std::process::Command::new("systemd-run");
    command.args(["--no-block", "--quiet", "--collect", "--service-type=exec"]);
    command.arg(format!("--unit={AFTER_SLEEP_UNIT}"));
    command.arg("--description=Better OS USB-C recovery after sleep");
    for property in after_sleep_properties() {
        command.arg("-p").arg(property);
    }
    command.arg(program).arg("--usb-after-sleep");
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());
    let _ = command.status();
}

/// What the after-sleep run reads from logind and does through it.
pub trait SleepWorld {
    fn lid_closed(&self) -> Option<bool>;
    fn shutdown_blocked(&self) -> bool;
    /// The uids of the active local graphical sessions, and whether anyone
    /// else has a session.
    fn people(&self) -> (Vec<u32>, bool);
    fn notify(&self, uid: u32);
    fn power_off(&self) -> Result<(), String>;
}

/// What the after-sleep run keeps under `/run/better-os` this boot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunState {
    /// The run of failures the person was told about.
    pub notified_run: Option<u32>,
    /// A power-off was asked for in this run.
    #[serde(default)]
    pub powered_off_run: Option<u32>,
}

/// What the after-sleep run did, for its journal line and the tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AfterSleep {
    pub reload: Option<Result<Restored, DaemonError>>,
    pub rebind: Option<Result<Restored, DaemonError>>,
    pub notified: Vec<u32>,
    pub powered_off: bool,
}

/// The after-sleep run: the recoveries the person turned on, then the
/// overnight guard.
pub fn after_sleep(
    machine: &dyn UsbMachine,
    world: &dyn SleepWorld,
    settings_path: &Path,
    run_state: &Path,
    timings: Timings,
) -> AfterSleep {
    let settings = load_settings(settings_path);
    let mut done = AfterSleep::default();
    if settings.ucsi_automatic {
        // The timeout after a resume is written about ten seconds after
        // it, so watch for it a while.
        let started = Instant::now();
        loop {
            match findings(machine) {
                Ok(found) if found.ucsi.active => {
                    done.reload = Some(reload(machine, timings));
                    break;
                }
                Ok(_) if started.elapsed() < timings.ucsi_watch => machine.pause(timings.poll * 4),
                Ok(_) => break,
                Err(error) => {
                    done.reload = Some(Err(error));
                    break;
                }
            }
        }
    }
    if settings.controller_automatic {
        let Ok(found) = findings(machine) else {
            return done;
        };
        if found.dead_controller().is_some() {
            done.rebind = Some(rebind(machine, timings));
        }
        let mut state: RunState = std::fs::read_to_string(run_state)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let (people, other_people) = world.people();
        let action = core::guard(
            &found,
            &settings,
            GuardWorld {
                lid_closed: world.lid_closed(),
                shutdown_blocked: world.shutdown_blocked(),
                other_people,
                notified_run: state.notified_run,
            },
        );
        let run = found.sleep.run_start();
        let notify = action.notify;
        let power_off = action.power_off && state.powered_off_run != Some(run);
        if notify {
            state.notified_run = Some(run);
        }
        if power_off {
            state.powered_off_run = Some(run);
        }
        // What is done once per run is done only once it is kept: a run
        // that cannot remember would tell the person every forty seconds.
        if (notify || power_off)
            && let Err(error) = serde_json::to_string(&state)
                .map_err(|e| DaemonError::WriteFailed(e.to_string()))
                .and_then(|text| write_whole(run_state, &text))
        {
            eprintln!(
                "usb-recovery: {}: {error}; nothing told or done",
                run_state.display()
            );
            return done;
        }
        if notify {
            for uid in &people {
                world.notify(*uid);
            }
            done.notified = people;
        }
        if power_off {
            match world.power_off() {
                Ok(()) => done.powered_off = true,
                Err(error) => eprintln!("usb-recovery: power-off refused: {error}"),
            }
        }
    }
    done
}

/// The notification, in the person's language.
pub fn sleep_blocked_notice(zh_tw: bool) -> (&'static str, &'static str) {
    if zh_tw {
        (
            "這台電腦現在無法進入睡眠",
            "睡眠之後，一側的 USB 停止運作，電腦也因此一直無法進入睡眠。重新開機就能修好。",
        )
    } else {
        (
            "This PC can't go to sleep right now",
            "USB on one side stopped working after sleep, and that keeps the PC from sleeping. Restarting the PC fixes it.",
        )
    }
}

/// Whether a locale names Traditional Chinese for Taiwan.
pub fn is_zh_tw(locale: &str) -> bool {
    let locale = locale.trim().to_ascii_lowercase();
    locale.starts_with("zh_tw") || locale.starts_with("zh-tw") || locale.starts_with("zh_hant")
}

/// `--notify-sleep-blocked LANG`, run as the person with their session bus
/// in `DBUS_SESSION_BUS_ADDRESS`: one desktop notification, fixed words.
pub fn notify_main(language: &str) -> Result<(), String> {
    let zh_tw = match language {
        "zh-TW" => true,
        "en" => false,
        _ => return Err("--notify-sleep-blocked takes zh-TW or en".into()),
    };
    let (summary, body) = sleep_blocked_notice(zh_tw);
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
        [("urgency", zbus::zvariant::Value::U8(2))]
            .into_iter()
            .collect();
    bus.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            "Better Manager",
            0u32,
            "better-manager",
            summary,
            body,
            Vec::<&str>::new(),
            hints,
            -1i32,
        ),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// One row of logind's ListInhibitors: what, who, why, mode, uid, pid.
type Inhibitor = (String, String, String, String, u32, u32);

/// One row of logind's ListSessions: id, uid, user, seat, path.
type SessionListed = (String, u32, String, String, zbus::zvariant::OwnedObjectPath);

/// logind, on the system bus, for the after-sleep run.
pub struct LogindWorld {
    bus: Option<zbus::blocking::Connection>,
    program: PathBuf,
}

impl LogindWorld {
    pub fn new(program: PathBuf) -> LogindWorld {
        LogindWorld {
            bus: zbus::blocking::Connection::system().ok(),
            program,
        }
    }

    fn manager(&self) -> Option<zbus::blocking::Proxy<'_>> {
        zbus::blocking::Proxy::new(
            self.bus.as_ref()?,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .ok()
    }

    /// The person's language, from AccountsService, else the system's.
    fn language(&self, uid: u32) -> bool {
        let from_accounts = self.bus.as_ref().and_then(|bus| {
            let proxy = zbus::blocking::Proxy::new(
                bus,
                "org.freedesktop.Accounts",
                format!("/org/freedesktop/Accounts/User{uid}"),
                "org.freedesktop.Accounts.User",
            )
            .ok()?;
            proxy
                .get_property::<String>("Language")
                .ok()
                .filter(|l| !l.is_empty())
        });
        let system = || {
            std::fs::read_to_string("/etc/default/locale")
                .ok()?
                .lines()
                .find_map(|line| {
                    line.strip_prefix("LANG=")
                        .map(|v| v.trim_matches('"').to_string())
                })
        };
        from_accounts.or_else(system).is_some_and(|l| is_zh_tw(&l))
    }
}

fn primary_gid(uid: u32) -> Option<u32> {
    // SAFETY: getpwuid_r with a buffer it may fill, read only on success.
    let mut buffer = vec![0u8; 16 * 1024];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    (status == 0 && !result.is_null()).then_some(entry.pw_gid)
}

impl SleepWorld for LogindWorld {
    fn lid_closed(&self) -> Option<bool> {
        self.manager()?.get_property::<bool>("LidClosed").ok()
    }

    fn shutdown_blocked(&self) -> bool {
        let Some(manager) = self.manager() else {
            // Not knowing is never read as safe.
            return true;
        };
        let listed: Result<Vec<Inhibitor>, _> = manager.call("ListInhibitors", &());
        match listed {
            Ok(list) => list.iter().any(|(what, _, _, mode, _, _)| {
                mode == "block" && what.split(':').any(|w| w == "shutdown")
            }),
            Err(_) => true,
        }
    }

    fn people(&self) -> (Vec<u32>, bool) {
        let Some(manager) = self.manager() else {
            // Nobody can be told, and nobody is known to be alone.
            return (Vec::new(), true);
        };
        let listed: Result<Vec<SessionListed>, _> = manager.call("ListSessions", &());
        let Ok(listed) = listed else {
            return (Vec::new(), true);
        };
        let mut people = Vec::new();
        let mut everyone = Vec::new();
        for (_, uid, _, _, path) in listed {
            let Some(bus) = self.bus.as_ref() else { break };
            let Ok(session) = zbus::blocking::Proxy::new(
                bus,
                "org.freedesktop.login1",
                path,
                "org.freedesktop.login1.Session",
            ) else {
                continue;
            };
            let class = session.get_property::<String>("Class").unwrap_or_default();
            if class != "user" {
                continue;
            }
            everyone.push(uid);
            let active = session.get_property::<bool>("Active").unwrap_or(false);
            let remote = session.get_property::<bool>("Remote").unwrap_or(true);
            let kind = session.get_property::<String>("Type").unwrap_or_default();
            if active
                && !remote
                && matches!(kind.as_str(), "wayland" | "x11")
                && !people.contains(&uid)
            {
                people.push(uid);
            }
        }
        let others = everyone.iter().any(|uid| !people.contains(uid));
        (people, others)
    }

    fn notify(&self, uid: u32) {
        let Some(gid) = primary_gid(uid) else { return };
        let language = if self.language(uid) { "zh-TW" } else { "en" };
        use std::os::unix::process::CommandExt as _;
        // Run as the person: their uid and primary group, no supplementary
        // groups (std drops them when root sets a uid), and nothing of this
        // unit's environment but their own bus.
        let status = std::process::Command::new(&self.program)
            .arg("--notify-sleep-blocked")
            .arg(language)
            .env_clear()
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path=/run/user/{uid}/bus"),
            )
            .env("LC_ALL", "C.UTF-8")
            .uid(uid)
            .gid(gid)
            .stdin(std::process::Stdio::null())
            .status();
        if !matches!(status, Ok(status) if status.success()) {
            eprintln!("usb-recovery: the notification for {uid} was not shown: {status:?}");
        }
    }

    fn power_off(&self) -> Result<(), String> {
        let manager = self.manager().ok_or("logind is not reachable")?;
        manager
            .call::<_, _, ()>("PowerOff", &(false,))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_module_is_found_only_under_its_own_release() {
        let dir = tempfile::tempdir().unwrap();
        let ko = dir.path().join("7.0.0-43-generic").join(UCSI_MODULE_DIR);
        std::fs::create_dir_all(&ko).unwrap();
        std::fs::write(ko.join("ucsi_acpi.ko.zst"), b"x").unwrap();
        assert_eq!(
            ucsi_module_file(dir.path(), "7.0.0-43-generic"),
            Some((ko.join("ucsi_acpi.ko.zst"), true))
        );
        for bad in ["", "../x", "a/b", ".hidden"] {
            assert_eq!(ucsi_module_file(dir.path(), bad), None, "{bad}");
        }
        assert_eq!(ucsi_module_file(dir.path(), "6.8.0-1-generic"), None);
        // A link is not the distribution's file.
        let other = dir.path().join("5.15.0-1-generic").join(UCSI_MODULE_DIR);
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", other.join("ucsi_acpi.ko")).unwrap();
        assert_eq!(ucsi_module_file(dir.path(), "5.15.0-1-generic"), None);
    }

    #[test]
    fn the_hook_takes_only_systemd_sleeps_arguments() {
        assert!(sleep_hook_arguments("post", "suspend"));
        assert!(sleep_hook_arguments("pre", "suspend-then-hibernate"));
        assert!(!sleep_hook_arguments("post", "reboot"));
        assert!(!sleep_hook_arguments("during", "suspend"));
    }

    #[test]
    fn the_language_follows_the_locale() {
        assert!(is_zh_tw("zh_TW.UTF-8"));
        assert!(!is_zh_tw("zh_CN.UTF-8"));
        assert!(!is_zh_tw("en_US.UTF-8"));
        let (title, body) = sleep_blocked_notice(true);
        assert!(title.contains("睡眠") && body.contains("重新開機"));
        let (title, body) = sleep_blocked_notice(false);
        assert!(title.contains("sleep") && body.contains("Restarting"));
    }

    #[test]
    fn the_helper_sandboxes_open_only_what_each_needs() {
        let ucsi = ucsi_helper_properties();
        assert!(!ucsi.contains(&"ProtectKernelModules=yes".to_string()));
        assert!(ucsi.contains(&"CapabilityBoundingSet=CAP_SYS_MODULE".to_string()));
        assert!(ucsi.contains(&"SystemCallFilter=@module".to_string()));
        assert!(ucsi.contains(&"TemporaryFileSystem=/sys:ro".to_string()));
        assert!(!ucsi.iter().any(|p| p.starts_with("BindPaths=")));
        assert_eq!(
            ucsi.iter()
                .filter(|p| p.starts_with("RuntimeMaxSec="))
                .count(),
            1
        );

        let address = PciAddress::parse("0000:63:00.4").unwrap();
        let xhci = xhci_helper_properties(&address);
        assert!(xhci.contains(&"ProtectKernelModules=yes".to_string()));
        assert!(xhci.contains(&"CapabilityBoundingSet=".to_string()));
        let binds: Vec<&String> = xhci.iter().filter(|p| p.starts_with("Bind")).collect();
        assert_eq!(
            binds,
            [
                "BindPaths=/sys/bus/pci/drivers/xhci_hcd/unbind",
                "BindPaths=/sys/bus/pci/drivers/xhci_hcd/bind",
                "BindReadOnlyPaths=/sys/bus/pci/devices/0000\\:63\\:00.4/vendor",
                "BindReadOnlyPaths=/sys/bus/pci/devices/0000\\:63\\:00.4/device",
                "BindReadOnlyPaths=/sys/bus/pci/devices/0000\\:63\\:00.4/class",
            ]
        );
        let after = after_sleep_properties();
        assert!(after.contains(&"ProtectKernelModules=yes".to_string()));
        assert!(after.contains(&"After=systemd-suspend.service".to_string()));
    }

    #[test]
    fn settings_are_kept_only_while_something_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usb-recovery.json");
        let on = set_choice(&path, UsbChoice::UcsiAutomatic, true).unwrap();
        assert!(on.ucsi_automatic);
        assert!(path.exists());
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let off = set_choice(&path, UsbChoice::UcsiAutomatic, false).unwrap();
        assert_eq!(off, UsbRecoverySettings::default());
        assert!(!path.exists());
        assert!(release(&path, &dir.path().join("run.json")).is_empty());
    }
}
