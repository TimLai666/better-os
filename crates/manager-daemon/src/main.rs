//! The Better Manager privileged daemon.
//!
//! Started on demand by D-Bus activation, runs as root, and exits once it has
//! been idle for a while. It is never enabled as a permanently running service:
//! a package manager that is not managing packages has no reason to be alive.

use std::sync::Arc;
use std::time::Duration;

use better_core::self_update::{OnDisk, ProgramFile};
use manager_daemon::apt::AptGetDriver;
use manager_daemon::authorize::PolkitAuthorizer;
use manager_daemon::executor::Executor;
use manager_daemon::hardware::HardwareService;
use manager_daemon::health::SystemHealthProbe;
use manager_daemon::host::SystemHostProbe;
use manager_daemon::service::{BUS_NAME, ManagerService, serve, wait_until_idle_or_replaced};
use manager_daemon::store::{ArtifactStore, Journal};
use manager_daemon::{ARCHIVE_DIR, STATE_DIR};

/// How long the daemon waits with nothing running before exiting.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// How long the daemon stays after its last transaction once an update has
/// replaced its program: enough for the caller to read the outcome.
const REPLACED_GRACE: Duration = Duration::from_secs(10);

/// How often a leaving daemon looks whether the calls inside have finished.
const DRAIN_POLL: Duration = Duration::from_millis(50);

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `better-hardware-restore.service` at boot (ticket 86): write the stored
    // charge limit and nothing else, then leave. No bus, no polkit: it runs
    // as root from a unit only the package installs.
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    // The hardware helper (ADR 0029): started by the daemon or the restore in
    // a transient unit whose /sys holds only this one file. It checks the
    // file and the value again, writes, and prints what it reads back.
    if arguments.first().map(String::as_str) == Some("--write-attribute") {
        let (Some(path), Some(value), None) =
            (arguments.get(1), arguments.get(2), arguments.get(3))
        else {
            return Err("--write-attribute takes a path and a value".into());
        };
        let read = manager_daemon::hardware::helper_main(std::path::Path::new(path), value)
            .map_err(|error| error.to_string())?;
        println!("{read}");
        return Ok(());
    }
    // The keyboard's LampArray helper (ADR 0034): started by the daemon or
    // the restore in a transient unit whose device cgroup allows this one
    // hidraw node. It reads the node's descriptor, refuses anything that is
    // not a LampArray, runs the one command, and prints JSON or `ok`.
    if arguments.first().map(String::as_str) == Some("--lamp-array") {
        let Some(node) = arguments.get(1) else {
            return Err("--lamp-array takes a node and a command".into());
        };
        let out = manager_daemon::keyboard_lighting::lamp_helper_main(node, &arguments[2..])
            .map_err(|error| error.to_string())?;
        println!("{out}");
        return Ok(());
    }
    // The USB-C recovery's helpers and after-sleep run (ticket 101, ADR
    // 0046). Each takes exactly what is listed and nothing from a caller.
    match arguments.first().map(String::as_str) {
        // In a transient unit that may load modules: unload and load
        // ucsi_acpi, the distribution's own signed file.
        Some("--reload-ucsi") => {
            if arguments.len() > 1 {
                return Err("--reload-ucsi takes no value".into());
            }
            println!("{}", manager_daemon::usb_recovery::reload_ucsi_main()?);
            return Ok(());
        }
        // In the same kind of unit, where /usr/lib/modules is readable (the
        // daemon's own unit hides it): only look at the file a reload would
        // load.
        Some("--check-ucsi") => {
            if arguments.len() > 1 {
                return Err("--check-ucsi takes no value".into());
            }
            println!("{}", manager_daemon::usb_recovery::check_ucsi_main()?);
            return Ok(());
        }
        // In a transient unit whose /sys holds xhci_hcd's unbind and bind:
        // the one function the daemon decided on.
        Some("--rebind-xhci") => {
            let (Some(address), None) = (arguments.get(1), arguments.get(2)) else {
                return Err("--rebind-xhci takes one PCI address".into());
            };
            println!(
                "{}",
                manager_daemon::usb_recovery::rebind_xhci_main(std::path::Path::new("/"), address)?
            );
            return Ok(());
        }
        // The systemd-sleep hook: queue the after-sleep run when anything
        // is on, and return at once. Never fails the sleep.
        Some("--after-sleep") => {
            if let (Some(phase), Some(operation), None) =
                (arguments.get(1), arguments.get(2), arguments.get(3))
            {
                manager_daemon::usb_recovery::after_sleep_hook(
                    &std::env::current_exe()?,
                    std::path::Path::new(manager_daemon::usb_recovery::SETTINGS),
                    phase,
                    operation,
                );
            }
            return Ok(());
        }
        // The after-sleep run itself, in the unit the hook queued.
        Some("--usb-after-sleep") => {
            if arguments.len() > 1 {
                return Err("--usb-after-sleep takes no value".into());
            }
            let program = std::env::current_exe()?;
            let machine = manager_daemon::usb_recovery::SystemUsbMachine::new(program.clone());
            let world = manager_daemon::usb_recovery::LogindWorld::new(program);
            let done = manager_daemon::usb_recovery::after_sleep(
                &machine,
                &world,
                std::path::Path::new(manager_daemon::usb_recovery::SETTINGS),
                std::path::Path::new(manager_daemon::usb_recovery::RUN_STATE),
                manager_daemon::usb_recovery::Timings::REAL,
            );
            eprintln!("usb-recovery: after sleep: {done:?}");
            return Ok(());
        }
        // As the person, from the after-sleep run: one notification.
        Some("--notify-sleep-blocked") => {
            let (Some(language), None) = (arguments.get(1), arguments.get(2)) else {
                return Err("--notify-sleep-blocked takes zh-TW or en".into());
            };
            manager_daemon::usb_recovery::notify_main(language)?;
            return Ok(());
        }
        _ => {}
    }
    // The prerm on removal of this package and of better-manager (ADR
    // 0034): a keyboard Better OS took over goes back to its firmware, the
    // battery gets back the charge limit it had before Better OS, and both
    // choices are forgotten, so neither they nor their boot-time restore
    // outlive the page that set them. Never fails the removal.
    if arguments.first().map(String::as_str) == Some("--release-hardware") {
        if arguments.len() > 1 {
            return Err("--release-hardware takes no value".into());
        }
        let program = std::env::current_exe()?;
        let root = hardware_platform::sysfs::SysRoot::system();
        let helper = manager_daemon::keyboard_lighting::SandboxedLampHelper {
            program: program.clone(),
        };
        for problem in manager_daemon::keyboard_lighting::release(
            &root,
            &helper,
            &manager_daemon::keyboard_lighting::LightingPaths::system(),
        ) {
            eprintln!("better-manager-daemon: keyboard lighting: {problem}");
        }
        // The USB-C recovery's choices (ADR 0046): with them gone the sleep
        // hook does nothing, so no recovery outlives the page that set it.
        for problem in manager_daemon::usb_recovery::release(
            std::path::Path::new(manager_daemon::usb_recovery::SETTINGS),
            std::path::Path::new(manager_daemon::usb_recovery::RUN_STATE),
        ) {
            eprintln!("better-manager-daemon: usb recovery: {problem}");
        }
        let writer = manager_daemon::hardware::SandboxedWriter { program };
        for line in manager_daemon::hardware::release_charge_limit(
            &root,
            &writer,
            std::path::Path::new(manager_daemon::HARDWARE_STATE),
        ) {
            eprintln!("better-manager-daemon: {line}");
        }
        return Ok(());
    }
    // The helpers' unit properties, one per line, exactly as the daemon
    // passes them to systemd-run: what packaging/hardware-unit/check.sh
    // starts its probes with, so the check cannot drift from the code.
    if arguments.first().map(String::as_str) == Some("--helper-properties") {
        let properties = match (
            arguments.get(1).map(String::as_str),
            arguments.get(2),
            arguments.get(3),
        ) {
            (Some("attribute"), Some(path), None) => {
                manager_daemon::hardware::helper_properties(std::path::Path::new(path))
            }
            (Some("lamp-array"), Some(node), None)
                if hardware_platform::sysfs::is_hidraw_name(node) =>
            {
                manager_daemon::keyboard_lighting::lamp_helper_properties(node)
            }
            (Some("ucsi-reload"), None, None) => {
                manager_daemon::usb_recovery::ucsi_helper_properties()
            }
            (Some("xhci-rebind"), Some(address), None) => {
                let Some(address) = hardware_core::usb_recovery::PciAddress::parse(address) else {
                    return Err("--helper-properties xhci-rebind takes a PCI address".into());
                };
                manager_daemon::usb_recovery::xhci_helper_properties(&address)
            }
            (Some("after-sleep"), None, None) => {
                manager_daemon::usb_recovery::after_sleep_properties()
            }
            _ => {
                return Err(
                    "--helper-properties takes attribute PATH, lamp-array hidrawN, ucsi-reload, xhci-rebind ADDRESS, or after-sleep".into(),
                );
            }
        };
        for property in properties {
            println!("{property}");
        }
        return Ok(());
    }
    // The LampArray helper started from here, as the daemon starts it, for
    // the packaged build's own diagnostics: `describe` prints what the
    // keyboard says about itself (the owner's check, ticket 136). Root only,
    // and only the read: no colour can be set this way.
    if arguments.first().map(String::as_str) == Some("--lamp-array-via-helper") {
        let (Some(node), Some(command), None) =
            (arguments.get(1), arguments.get(2), arguments.get(3))
        else {
            return Err("--lamp-array-via-helper takes a node and describe".into());
        };
        if command != "describe" {
            return Err("--lamp-array-via-helper only describes".into());
        }
        use manager_daemon::keyboard_lighting::LampHelper as _;
        let helper = manager_daemon::keyboard_lighting::SandboxedLampHelper {
            program: std::env::current_exe()?,
        };
        let description = helper
            .run(
                node,
                manager_daemon::keyboard_lighting::LampCommand::Describe,
            )
            .map_err(|error| error.to_string())?;
        println!("{}", serde_json::to_string_pretty(&description)?);
        return Ok(());
    }
    // The same helper started from here, as the daemon starts it: used by
    // packaging/hardware-unit/check.sh to show the path works from inside the
    // daemon's own sandbox. Root only, and the same checks as the helper.
    if arguments.first().map(String::as_str) == Some("--write-via-helper") {
        let (Some(path), Some(value), None) =
            (arguments.get(1), arguments.get(2), arguments.get(3))
        else {
            return Err("--write-via-helper takes a path and a value".into());
        };
        use manager_daemon::hardware::AttributeWriter as _;
        let writer = manager_daemon::hardware::SandboxedWriter {
            program: std::env::current_exe()?,
        };
        let read = writer
            .write(std::path::Path::new(path), value)
            .map_err(|error| error.to_string())?;
        println!("{read}");
        return Ok(());
    }
    // The package's prerm on removal (ADR 0032): every fstab line and udev
    // rule Better OS wrote goes, and nothing else. It never fails the
    // removal; what it could not do is printed.
    if arguments.first().map(String::as_str) == Some("--remove-drive-entries") {
        for problem in manager_daemon::drives::DriveChange::remove_all(
            &manager_daemon::drives::SystemctlUnits,
            &manager_daemon::drives::DrivePaths::system(),
        ) {
            eprintln!("better-manager-daemon: drive entries: {problem}");
        }
        return Ok(());
    }
    // `better-greeter-watch.service` at boot (ADR 0033, ticket 140): watch
    // for the login screen once, restart GDM once if it never appears, and
    // record what happened. Never fails the unit over what it found.
    if arguments.first().map(String::as_str) == Some("--watch-login-screen") {
        if arguments.len() > 1 {
            return Err("--watch-login-screen takes no value".into());
        }
        let host = manager_daemon::boot_watch::SystemWatchHost::new();
        let outcome = manager_daemon::boot_watch::run(
            &host,
            &manager_ipc::boot_watch::Records::system(),
            manager_daemon::boot_watch::Timings::BOOT,
        );
        eprintln!("better-greeter-watch: {outcome:?}");
        return Ok(());
    }
    if let Some(argument) = arguments.first() {
        if argument == "--restore-hardware" {
            let root = hardware_platform::sysfs::SysRoot::system();
            let state = std::path::Path::new(manager_daemon::HARDWARE_STATE);
            // The threshold file belongs to a platform driver that can bind
            // after this unit starts; wait for it a little before saying the
            // battery has none.
            let writer = manager_daemon::hardware::SandboxedWriter {
                program: std::env::current_exe()?,
            };
            let mut result = manager_daemon::hardware::restore(&root, &writer, state);
            for _ in 0..30 {
                if result != Err(manager_daemon::DaemonError::HardwareAbsent) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                result = manager_daemon::hardware::restore(&root, &writer, state);
            }
            // The keyboard's colour (ADR 0034), on the keyboard it was set
            // for and no other. An I2C keyboard can bind after this unit
            // starts: the same wait. A keyboard still absent after it is a
            // skip with this line, not a failed unit.
            use manager_daemon::keyboard_lighting::RestoredLighting;
            let helper = manager_daemon::keyboard_lighting::SandboxedLampHelper {
                program: std::env::current_exe()?,
            };
            let lighting = std::path::Path::new(manager_daemon::LIGHTING_STATE);
            let restore_lighting =
                || manager_daemon::keyboard_lighting::restore(&root, &helper, &writer, lighting);
            let mut lit = restore_lighting();
            for _ in 0..30 {
                if !matches!(lit, Ok(RestoredLighting::DeviceAbsent(_))) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                lit = restore_lighting();
            }
            match &lit {
                Ok(RestoredLighting::DeviceAbsent(device)) => eprintln!(
                    "better-manager-daemon: keyboard restore: skipped, {} is not here",
                    device.label()
                ),
                Ok(restored) => eprintln!("better-manager-daemon: keyboard restore: {restored:?}"),
                Err(error) => eprintln!("better-manager-daemon: keyboard restore: {error}"),
            }
            return match (result, lit) {
                (Ok(restored), Ok(_)) => {
                    eprintln!("better-manager-daemon: hardware restore: {restored:?}");
                    Ok(())
                }
                (Err(error), _) | (_, Err(error)) => Err(error.into()),
            };
        }
        return Err(format!(
            "better-manager-daemon takes no argument but --restore-hardware, --watch-login-screen, --remove-drive-entries, --release-hardware, --helper-properties, --write-attribute, --lamp-array, --lamp-array-via-helper, --write-via-helper, --reload-ucsi, --rebind-xhci, --after-sleep, --usb-after-sleep, or --notify-sleep-blocked; got {argument:?}"
        )
        .into());
    }
    let artifacts = Arc::new(ArtifactStore::new(ARCHIVE_DIR));
    let journal = Arc::new(Journal::new(STATE_DIR));

    // A transaction left mid-flight by a daemon that died is never resumed. An
    // interrupted dpkg run needs a person to look at it, and continuing would
    // be guessing at what state the packages are in.
    for interrupted in journal.interrupted()? {
        eprintln!(
            "better-manager-daemon: transaction {} was interrupted and needs manual recovery",
            interrupted.transaction_id
        );
    }

    let executor = Arc::new(Executor {
        apt: Arc::new(AptGetDriver),
        host: Arc::new(SystemHostProbe),
        health: Arc::new(SystemHealthProbe),
        artifacts: artifacts.clone(),
        journal: journal.clone(),
    });

    // polkit is asked on a connection of its own. The served connection has
    // to be built with the service already in it (see `serve`), and the
    // service holds the authorizer, so the authorizer cannot use that one.
    // polkit identifies the caller from the call's sender, not from the
    // connection that asks.
    let polkit = zbus::connection::Builder::system()?.build().await?;
    let service = ManagerService::new(PolkitAuthorizer::new(polkit), executor, artifacts, journal)
        // The daemon package's postinst restarts a running daemon after an
        // upgrade only while this file is absent (ticket 105). /run is emptied
        // at boot, and a file left by a daemon that died is removed here.
        .with_busy_marker(manager_daemon::BUSY_MARKER);

    let activity = service.activity();
    let gate = service.gate();
    let hardware = HardwareService::new(
        PolkitAuthorizer::new(zbus::connection::Builder::system()?.build().await?),
        Arc::new(manager_daemon::hardware::SandboxedWriter {
            program: std::env::current_exe()?,
        }),
        hardware_platform::sysfs::SysRoot::system(),
        gate.clone(),
    )
    .with_state_file(manager_daemon::HARDWARE_STATE)
    .with_lamp_helper(Arc::new(
        manager_daemon::keyboard_lighting::SandboxedLampHelper {
            program: std::env::current_exe()?,
        },
    ))
    .with_lighting_paths(manager_daemon::keyboard_lighting::LightingPaths::system());
    let udisks = zbus::blocking::Connection::system()?;
    let drives = manager_daemon::drives::DrivesService::new(
        PolkitAuthorizer::new(zbus::connection::Builder::system()?.build().await?),
        Arc::new(manager_daemon::drives::UdisksResolver { connection: udisks }),
        Arc::new(manager_daemon::drives::SystemctlUnits),
        manager_daemon::drives::DrivePaths::system(),
        gate.clone(),
    );
    let boot_watch = manager_daemon::boot_watch::BootWatchService::new(
        PolkitAuthorizer::new(zbus::connection::Builder::system()?.build().await?),
        manager_ipc::boot_watch::Records::system(),
        gate.clone(),
    );
    let usb_recovery = manager_daemon::usb_recovery::UsbRecoveryService::new(
        PolkitAuthorizer::new(zbus::connection::Builder::system()?.build().await?),
        Arc::new(manager_daemon::usb_recovery::SystemUsbMachine::new(
            std::env::current_exe()?,
        )),
        manager_daemon::usb_recovery::SETTINGS,
        gate.clone(),
    );
    let connection = serve(
        zbus::connection::Builder::system()?,
        BUS_NAME,
        service,
        hardware,
        drives,
        boot_watch,
        usb_recovery,
    )
    .await?;

    // Nothing to do but wait to be called, and never exit in the middle of a
    // transaction or a repair. D-Bus activation starts us again on the next
    // request — as the new program, when an update replaced this one.
    let program = ProgramFile::current().ok();
    let replaced = move || {
        program
            .as_ref()
            .is_some_and(|program| program.on_disk() != OnDisk::Same)
    };
    wait_until_idle_or_replaced(activity.clone(), IDLE_TIMEOUT, REPLACED_GRACE, &replaced).await;
    // Leaving: close the door to new calls (clients retry and reach a new
    // daemon), wait for the calls already inside, and only then give the name
    // up. Releasing it first would make systemd stop this `Type=dbus` unit at
    // once, in the middle of whatever was still running.
    gate.close(DRAIN_POLL).await;
    let _ = connection.release_name(BUS_NAME).await;
    Ok(())
}
