//! Enabling and starting a component's systemd user units.
//!
//! The packages install their units without enabling them; this is the half
//! that makes an installed component run. None of it needs root: a user unit is
//! owned by the session that starts it, so the controller talks to
//! `systemctl --user` directly and never crosses the privileged boundary.
//!
//! Everything is an argument vector. Nothing here is ever handed to a shell,
//! and a unit name that is not a plain `name.service` — a path, a name with
//! whitespace, a name systemctl would read as an option — is refused before a
//! process is spawned.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use better_core::package_health::InstalledFiles;

/// Where a package puts a user unit. systemd's own user search path is wider
/// (and includes two per-user directories); what this looks at is the set of
/// files dpkg recorded for the package, which is the only set that belongs to
/// the component.
pub const USER_UNIT_DIRECTORY: &str = "/usr/lib/systemd/user";

/// What systemd says about one unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitState {
    pub unit: String,
    pub enabled: bool,
    pub active: bool,
}

/// One unit that could not be enabled, disabled, or queried, with the words
/// systemctl used. The name travels separately from the message because the
/// presentation layer has to name the unit even when the message is terse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitFailure {
    pub unit: String,
    pub message: String,
}

/// Manages the user units of one component's package.
///
/// `enable_now` and `disable_now` mean what `systemctl --user enable --now` and
/// `disable --now` mean: one invocation per unit, so a failure is attributable
/// to the unit that failed rather than to the batch.
pub trait UserUnitController: Send + Sync {
    fn enable_now(&self, units: &[String]) -> Result<(), UnitFailure>;
    fn disable_now(&self, units: &[String]) -> Result<(), UnitFailure>;
    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, UnitFailure>;
}

/// The service files a package installed, as dpkg's own file list records them.
///
/// Only direct children of [`USER_UNIT_DIRECTORY`] whose name is a plain
/// `.service` are units a user manager will load under that name. A system unit
/// (`/usr/lib/systemd/system/...`) and a D-Bus activation file
/// (`/usr/share/dbus-1/services/...`) are deliberately not here: neither is
/// started by the user manager, and asking it about one would fail.
pub fn service_units(files: &InstalledFiles) -> Vec<String> {
    let mut units: Vec<String> = files
        .paths
        .iter()
        .filter(|path| path.parent() == Some(Path::new(USER_UNIT_DIRECTORY)))
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
        .filter(|name| is_plain_unit_name(name))
        .map(str::to_string)
        .collect();
    units.sort();
    units.dedup();
    units
}

/// Whether this is a name that can be handed to systemctl as a unit and nothing
/// else: a plain `name.service` with no path component, no whitespace, no
/// leading option, and no character systemctl would interpret.
pub fn is_plain_unit_name(unit: &str) -> bool {
    !unit.is_empty()
        && unit.len() <= 255
        && unit.ends_with(".service")
        && unit.len() > ".service".len()
        && !unit.starts_with('-')
        && unit.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '@')
        })
}

/// One `systemctl` invocation, as a program and its argument vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemctlInvocation {
    pub program: &'static str,
    pub arguments: Vec<String>,
}

/// The real controller: `systemctl --user`, one unit at a time.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemdUserUnitController;

impl SystemdUserUnitController {
    pub const fn new() -> Self {
        Self
    }

    /// `systemctl --user enable --now <unit>`.
    pub fn enable_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(unit, &["enable", "--now"])
    }

    /// `systemctl --user disable --now <unit>`.
    pub fn disable_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(unit, &["disable", "--now"])
    }

    /// `systemctl --user show --property=UnitFileState --property=ActiveState
    /// <unit>`.
    pub fn show_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(
            unit,
            &["show", "--property=UnitFileState", "--property=ActiveState"],
        )
    }

    fn invocation(unit: &str, arguments: &[&str]) -> Result<SystemctlInvocation, UnitFailure> {
        if !is_plain_unit_name(unit) {
            return Err(UnitFailure {
                unit: unit.to_string(),
                message: "not a plain unit name".to_string(),
            });
        }
        let mut full = std::iter::once("--user")
            .chain(arguments.iter().copied())
            .map(str::to_string)
            .collect::<Vec<_>>();
        full.push(unit.to_string());
        Ok(SystemctlInvocation {
            program: "systemctl",
            arguments: full,
        })
    }

    fn run(invocation: &SystemctlInvocation) -> Result<String, String> {
        let output = Command::new(invocation.program)
            .args(&invocation.arguments)
            .env("LC_ALL", "C")
            .output()
            .map_err(|error| format!("systemctl could not be run: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("systemctl exited with {}", output.status));
            return Err(message);
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl UserUnitController for SystemdUserUnitController {
    fn enable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        for unit in units {
            let invocation = Self::enable_invocation(unit)?;
            Self::run(&invocation).map_err(|message| UnitFailure {
                unit: unit.clone(),
                message,
            })?;
        }
        Ok(())
    }

    fn disable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        for unit in units {
            let invocation = Self::disable_invocation(unit)?;
            Self::run(&invocation).map_err(|message| UnitFailure {
                unit: unit.clone(),
                message,
            })?;
        }
        Ok(())
    }

    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, UnitFailure> {
        let mut states = Vec::with_capacity(units.len());
        for unit in units {
            let invocation = Self::show_invocation(unit)?;
            let output = Self::run(&invocation).map_err(|message| UnitFailure {
                unit: unit.clone(),
                message,
            })?;
            states.push(parse_unit_state(unit, &output));
        }
        Ok(states)
    }
}

/// Reads the two properties `show_invocation` asked for.
///
/// A unit systemd has never heard of is not an error: `show` exits zero and
/// prints an empty `UnitFileState`, so it reads as not enabled and not running,
/// which is what the caller needs to know.
pub fn parse_unit_state(unit: &str, output: &str) -> UnitState {
    let mut enabled = false;
    let mut active = false;
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("UnitFileState=") {
            enabled = unit_file_state_is_enabled(value.trim());
        } else if let Some(value) = line.strip_prefix("ActiveState=") {
            active = matches!(value.trim(), "active" | "reloading");
        }
    }
    UnitState {
        unit: unit.to_string(),
        enabled,
        active,
    }
}

/// Whether a `UnitFileState` means "systemd will start this at the next
/// opportunity". `masked` is not among them, and neither is `disabled`.
fn unit_file_state_is_enabled(state: &str) -> bool {
    matches!(
        state,
        "enabled"
            | "enabled-runtime"
            | "linked"
            | "linked-runtime"
            | "static"
            | "indirect"
            | "generated"
            | "transient"
    )
}

/// What the fake controller was asked to do, in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FakeUnitCall {
    Enable(Vec<String>),
    Disable(Vec<String>),
    States(Vec<String>),
}

#[derive(Debug, Default)]
struct FakeUnitState {
    units: BTreeMap<String, UnitState>,
    failing_enable: BTreeMap<String, String>,
    failing_disable: BTreeMap<String, String>,
    unreadable_states: Option<String>,
    calls: Vec<FakeUnitCall>,
}

/// A controller that answers from memory, for tests.
///
/// A unit it has never been told about starts out stopped, so a test only has
/// to name the units whose starting state matters. Enabling marks a unit
/// enabled and active; disabling marks it neither, which is what the two
/// `systemctl` verbs do.
#[derive(Debug, Default)]
pub struct FakeUserUnitController {
    state: Mutex<FakeUnitState>,
}

impl FakeUserUnitController {
    pub fn new(states: &[UnitState]) -> Self {
        let mut units = BTreeMap::new();
        for state in states {
            units.insert(state.unit.clone(), state.clone());
        }
        Self {
            state: Mutex::new(FakeUnitState {
                units,
                ..FakeUnitState::default()
            }),
        }
    }

    pub fn with_failing_enable(self, unit: &str, message: &str) -> Self {
        self.state
            .lock()
            .expect("the fake is not poisoned")
            .failing_enable
            .insert(unit.to_string(), message.to_string());
        self
    }

    pub fn with_failing_disable(self, unit: &str, message: &str) -> Self {
        self.state
            .lock()
            .expect("the fake is not poisoned")
            .failing_disable
            .insert(unit.to_string(), message.to_string());
        self
    }

    pub fn with_unreadable_states(self, message: &str) -> Self {
        self.state
            .lock()
            .expect("the fake is not poisoned")
            .unreadable_states = Some(message.to_string());
        self
    }

    pub fn calls(&self) -> Vec<FakeUnitCall> {
        self.state
            .lock()
            .expect("the fake is not poisoned")
            .calls
            .clone()
    }

    pub fn state(&self, unit: &str) -> Option<UnitState> {
        self.state
            .lock()
            .expect("the fake is not poisoned")
            .units
            .get(unit)
            .cloned()
    }
}

impl UserUnitController for FakeUserUnitController {
    fn enable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        let mut state = self.state.lock().expect("the fake is not poisoned");
        state.calls.push(FakeUnitCall::Enable(units.to_vec()));
        for unit in units {
            if let Some(message) = state.failing_enable.get(unit) {
                return Err(UnitFailure {
                    unit: unit.clone(),
                    message: message.clone(),
                });
            }
        }
        for unit in units {
            state.units.insert(
                unit.clone(),
                UnitState {
                    unit: unit.clone(),
                    enabled: true,
                    active: true,
                },
            );
        }
        Ok(())
    }

    fn disable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        let mut state = self.state.lock().expect("the fake is not poisoned");
        state.calls.push(FakeUnitCall::Disable(units.to_vec()));
        for unit in units {
            if let Some(message) = state.failing_disable.get(unit) {
                return Err(UnitFailure {
                    unit: unit.clone(),
                    message: message.clone(),
                });
            }
        }
        for unit in units {
            state.units.insert(
                unit.clone(),
                UnitState {
                    unit: unit.clone(),
                    enabled: false,
                    active: false,
                },
            );
        }
        Ok(())
    }

    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, UnitFailure> {
        let mut state = self.state.lock().expect("the fake is not poisoned");
        state.calls.push(FakeUnitCall::States(units.to_vec()));
        if let Some(message) = &state.unreadable_states {
            return Err(UnitFailure {
                unit: units.first().cloned().unwrap_or_default(),
                message: message.clone(),
            });
        }
        Ok(units
            .iter()
            .map(|unit| {
                state.units.get(unit).cloned().unwrap_or_else(|| UnitState {
                    unit: unit.clone(),
                    enabled: false,
                    active: false,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_service_name_can_become_a_unit() {
        assert!(is_plain_unit_name("better-awake.service"));
        assert!(is_plain_unit_name("better-touchpad-gestures.service"));
        assert!(is_plain_unit_name("getty@tty1.service"));
        for name in [
            "",
            "better-awake",
            "better-awake.service/../x",
            "better awake.service",
            "-better-awake.service",
            "better-awake.SERVICE",
            "better/awake.service",
            "better;rm.service",
            ".service",
        ] {
            assert!(!is_plain_unit_name(name), "{name} must be refused");
        }
    }

    #[test]
    fn every_invocation_is_an_argument_vector_and_never_a_shell() {
        let enable = SystemdUserUnitController::enable_invocation("better-awake.service").unwrap();
        assert_eq!(enable.program, "systemctl");
        assert_eq!(
            enable.arguments,
            ["--user", "enable", "--now", "better-awake.service"]
        );

        let disable =
            SystemdUserUnitController::disable_invocation("better-awake.service").unwrap();
        assert_eq!(
            disable.arguments,
            ["--user", "disable", "--now", "better-awake.service"]
        );

        let show = SystemdUserUnitController::show_invocation("better-awake.service").unwrap();
        assert_eq!(
            show.arguments,
            [
                "--user",
                "show",
                "--property=UnitFileState",
                "--property=ActiveState",
                "better-awake.service",
            ]
        );

        for invocation in [&enable, &disable, &show] {
            assert!(
                !invocation
                    .arguments
                    .iter()
                    .any(|argument| argument == "-c" || argument == "sh" || argument == "bash"),
                "{:?} looks like a shell",
                invocation.arguments
            );
        }
    }

    #[test]
    fn an_unsafe_unit_name_never_produces_an_invocation() {
        for unit in [
            "../etc/passwd.service",
            "better awake.service",
            "-x.service",
        ] {
            assert!(matches!(
                SystemdUserUnitController::enable_invocation(unit),
                Err(UnitFailure { .. })
            ));
            assert!(matches!(
                SystemdUserUnitController::show_invocation(unit),
                Err(UnitFailure { .. })
            ));
        }
    }

    #[test]
    fn the_two_properties_are_read_into_enabled_and_active() {
        // The shape `systemctl --user show` prints on this host.
        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=enabled\nActiveState=active\n",
        );
        assert!(state.enabled && state.active);

        // `static` is enabled by definition and has nothing to link.
        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=static\nActiveState=active\n",
        );
        assert!(state.enabled && state.active);

        // A unit systemd has never heard of prints an empty state and exits 0.
        let state = parse_unit_state(
            "better-awake.service",
            "LoadState=not-found\nActiveState=inactive\nUnitFileState=\n",
        );
        assert!(!state.enabled && !state.active);

        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=enabled\nActiveState=failed\n",
        );
        assert!(state.enabled && !state.active);
    }

    #[test]
    fn the_fake_starts_unknown_units_stopped_and_records_what_it_was_asked() {
        let units = FakeUserUnitController::new(&[]);
        assert_eq!(
            units.states(&["better-awake.service".to_string()]).unwrap(),
            vec![UnitState {
                unit: "better-awake.service".to_string(),
                enabled: false,
                active: false,
            }]
        );
        units
            .enable_now(&["better-awake.service".to_string()])
            .unwrap();
        assert_eq!(
            units.state("better-awake.service"),
            Some(UnitState {
                unit: "better-awake.service".to_string(),
                enabled: true,
                active: true,
            })
        );
        assert_eq!(
            units.calls(),
            vec![
                FakeUnitCall::States(vec!["better-awake.service".to_string()]),
                FakeUnitCall::Enable(vec!["better-awake.service".to_string()]),
            ]
        );
    }

    #[test]
    fn a_packages_user_units_are_the_service_files_in_its_own_directory() {
        let files = InstalledFiles {
            paths: [
                "/usr/lib/systemd/user",
                "/usr/lib/systemd/user/better-awake.service",
                "/usr/lib/systemd/user/better-awake.service.d/override.conf",
                "/usr/lib/systemd/system/better-manager-daemon.service",
                "/usr/share/dbus-1/services/org.betteros.Storage1.service",
                "/usr/bin/better-awake-service",
            ]
            .iter()
            .map(std::path::PathBuf::from)
            .collect(),
            ..InstalledFiles::default()
        };
        assert_eq!(service_units(&files), ["better-awake.service"]);
    }
}
