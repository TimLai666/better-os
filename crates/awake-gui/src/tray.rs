//! Bringing the tray up when the window opens.
//!
//! The tray is the `better-awake-tray` user unit, started with the graphical
//! session and started again when it fails. A tray that someone quit stays gone
//! until the next login, and the owner once found Better Awake with no taskbar
//! icon for that reason. Opening Better Awake is the moment a person is looking
//! for it, so the window brings the tray back when no tray is on the bus.
//!
//! The window never runs `systemctl`: a component's units are Better Manager's
//! to manage. It asks the bus for the tray's name, and the bus starts the tray
//! through its unit (`org.betteros.AwakeTray1.service` names
//! `SystemdService=better-awake-tray.service`), so the tray it brings back is
//! the one an update restarts and an uninstall stops. It asks only while the
//! unit is enabled, read from the enablement link `systemctl --user enable`
//! leaves in the user's configuration: a component switched off in Better
//! Manager stays off. Only where no unit is installed (a development build) is
//! the program spawned directly, with no argument and no shell.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use awake_ipc::notification::TRAY_BUS_NAME;

/// The program the tray's user unit runs.
pub(crate) const TRAY_PROGRAM: &str = "awake-tray";

/// The tray's user unit, and where the package installs it.
pub(crate) const TRAY_UNIT: &str = "better-awake-tray.service";
pub(crate) const TRAY_UNIT_PATH: &str = "/usr/lib/systemd/user/better-awake-tray.service";

/// How long the window waits for the bus to answer its request for the tray.
const ACTIVATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The target whose `.wants` directory holds the unit's enablement link.
const WANTED_BY: &str = "graphical-session.target";

/// Whether to start the tray, from whether its bus name has an owner.
/// `None` means the bus could not be asked; nothing is started then, because
/// a tray started with no session bus would only fail.
pub(crate) fn should_start(name_has_owner: Option<bool>) -> bool {
    name_has_owner == Some(false)
}

/// Where `systemctl --user enable` puts the unit's link for this user:
/// `$XDG_CONFIG_HOME/systemd/user/graphical-session.target.wants/`, with
/// `~/.config` when `XDG_CONFIG_HOME` is unset or not absolute.
pub(crate) fn enablement_link(
    xdg_config_home: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    let config = match xdg_config_home.filter(|path| Path::new(path).is_absolute()) {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(home.filter(|path| Path::new(path).is_absolute())?).join(".config"),
    };
    Some(
        config
            .join("systemd/user")
            .join(format!("{WANTED_BY}.wants"))
            .join(TRAY_UNIT),
    )
}

/// What bringing the tray back does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrayStart {
    /// A tray is on the bus, or the bus could not be asked.
    NotNeeded,
    /// Ask the bus for the tray's name, which starts the unit.
    Activate,
    /// The unit is installed and not enabled, so the component is off.
    ComponentOff,
    /// No unit is installed: spawn the program.
    Spawn,
}

/// Decides how to bring the tray back. Runs nothing.
pub(crate) fn plan(name_has_owner: Option<bool>, unit_installed: bool, enabled: bool) -> TrayStart {
    if !should_start(name_has_owner) {
        TrayStart::NotNeeded
    } else if !unit_installed {
        TrayStart::Spawn
    } else if enabled {
        TrayStart::Activate
    } else {
        TrayStart::ComponentOff
    }
}

/// Asks the session bus whether a tray is running.
fn tray_is_running(bus: &zbus::blocking::fdo::DBusProxy<'_>) -> Option<bool> {
    let name = zbus::names::BusName::try_from(TRAY_BUS_NAME).ok()?;
    bus.name_has_owner(name).ok()
}

/// Brings the tray back when none is running, off the caller's thread. A
/// spawned tray is reaped when it exits so no zombie is left behind.
pub(crate) fn ensure_running() {
    std::thread::spawn(|| {
        // Its own connection with a short call timeout: asking for the tray's
        // name is answered only once a tray takes it, and a tray with no
        // notification service never does, so without one this thread would
        // wait out the bus's own activation limit.
        let Ok(builder) = zbus::blocking::connection::Builder::session() else {
            return;
        };
        let Ok(connection) = builder.method_timeout(ACTIVATION_TIMEOUT).build() else {
            return;
        };
        let Ok(bus) = zbus::blocking::fdo::DBusProxy::new(&connection) else {
            return;
        };
        let link = enablement_link(
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
            std::env::var("HOME").ok().as_deref(),
        );
        let outcome = plan(
            tray_is_running(&bus),
            Path::new(TRAY_UNIT_PATH).exists(),
            // A symlink, so `exists` follows it: a dangling link is not enabled.
            link.is_some_and(|link| link.exists()),
        );
        match outcome {
            TrayStart::NotNeeded | TrayStart::ComponentOff => {}
            TrayStart::Activate => {
                // The bus answers once the tray takes the name. A tray with no
                // notification service never does, and the bus gives up after
                // its own timeout; the unit was started either way.
                if let Ok(name) = zbus::names::WellKnownName::try_from(TRAY_BUS_NAME)
                    && let Err(error) = bus.start_service_by_name(name, 0)
                {
                    eprintln!("awake-gui: asking the bus for the tray did not finish: {error}");
                }
            }
            TrayStart::Spawn => match Command::new(TRAY_PROGRAM)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
            {
                Ok(mut child) => {
                    let _ = child.wait();
                }
                Err(error) => eprintln!("awake-gui: the tray could not be started: {error}"),
            },
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_is_started_only_when_the_bus_says_none_is_running() {
        assert!(should_start(Some(false)));
        assert!(!should_start(Some(true)));
        assert!(!should_start(None));
    }

    #[test]
    fn the_tray_program_is_the_one_its_unit_runs() {
        let unit = include_str!("../../../packaging/awake/better-awake-tray.service");
        assert!(
            unit.lines()
                .any(|line| line == format!("ExecStart=/usr/bin/{TRAY_PROGRAM}"))
        );
        assert!(
            unit.lines()
                .any(|line| line == format!("WantedBy={WANTED_BY}")),
            "the enablement link is looked for under the target the unit is wanted by"
        );
    }

    #[test]
    fn asking_for_the_trays_name_starts_the_trays_unit() {
        let activation = include_str!("../../../packaging/awake/org.betteros.AwakeTray1.service");
        assert!(
            activation
                .lines()
                .any(|line| line == format!("Name={TRAY_BUS_NAME}"))
        );
        assert!(
            activation
                .lines()
                .any(|line| line == format!("SystemdService={TRAY_UNIT}"))
        );
    }

    #[test]
    fn the_unit_path_is_where_the_package_installs_it() {
        let build = include_str!("../../../packaging/build-deb.sh");
        assert!(build.contains(&format!(
            "packaging/awake/{TRAY_UNIT}:{}",
            TRAY_UNIT_PATH.trim_start_matches('/')
        )));
    }

    #[test]
    fn an_installed_and_enabled_tray_is_brought_back_by_its_unit() {
        assert_eq!(plan(Some(false), true, true), TrayStart::Activate);
    }

    #[test]
    fn a_component_switched_off_is_not_started_from_the_window() {
        assert_eq!(plan(Some(false), true, false), TrayStart::ComponentOff);
    }

    #[test]
    fn a_running_tray_or_an_unreachable_bus_starts_nothing() {
        for enabled in [true, false] {
            assert_eq!(plan(Some(true), true, enabled), TrayStart::NotNeeded);
            assert_eq!(plan(None, true, enabled), TrayStart::NotNeeded);
        }
    }

    #[test]
    fn only_a_build_with_no_unit_spawns_the_program() {
        assert_eq!(plan(Some(false), false, false), TrayStart::Spawn);
    }

    #[test]
    fn the_enablement_link_is_where_systemctl_user_enable_puts_it() {
        assert_eq!(
            enablement_link(Some("/home/a/.cfg"), Some("/home/a")),
            Some(PathBuf::from(
                "/home/a/.cfg/systemd/user/graphical-session.target.wants/better-awake-tray.service"
            ))
        );
        assert_eq!(
            enablement_link(None, Some("/home/a")),
            Some(PathBuf::from(
                "/home/a/.config/systemd/user/graphical-session.target.wants/better-awake-tray.service"
            ))
        );
        // A relative XDG_CONFIG_HOME is ignored, as the base-directory spec says.
        assert_eq!(
            enablement_link(Some("cfg"), Some("/home/a")),
            enablement_link(None, Some("/home/a"))
        );
        assert_eq!(enablement_link(None, None), None);
    }

    #[test]
    fn an_enablement_link_on_disk_is_read_as_enabled_and_a_dangling_one_is_not() {
        let home = tempfile::tempdir().unwrap();
        let link = enablement_link(None, home.path().to_str()).unwrap();
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        let unit = home.path().join("better-awake-tray.service");
        std::os::unix::fs::symlink(&unit, &link).unwrap();
        assert!(!link.exists(), "a link to nothing is not enabled");
        std::fs::write(&unit, "[Unit]\n").unwrap();
        assert!(link.exists());
    }
}
