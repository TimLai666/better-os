//! Enabling and starting a component's systemd user units.
//!
//! The packages install their units without enabling them; this is the half
//! that makes an installed component run. None of it needs root: a user unit is
//! owned by the session that starts it, so the controller talks to
//! `systemctl --user` directly and never crosses the privileged boundary.
//!
//! Everything is an argument vector. Nothing here is ever handed to a shell,
//! and a unit name that is not a plain `name.service` — a path, a name with
//! whitespace, a name systemctl would read as an option, a template with no
//! instance — is refused before a process is spawned.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use better_core::package_health::InstalledFiles;

/// Where a package puts a user unit. systemd's own user search path is wider
/// (and includes two per-user directories); what this looks at is the set of
/// files dpkg recorded for the package, which is the only set that belongs to
/// the component.
pub const USER_UNIT_DIRECTORY: &str = "/usr/lib/systemd/user";

/// Set in a test environment to a file path, it stops the real controller from
/// ever running `systemctl`: each invocation is appended to that file instead
/// and answered as if no user session existed. A test that drives the real
/// binaries in their real mode sets it, so nothing it does can reach the user
/// manager of the person running the tests.
pub const SYSTEMCTL_GUARD_VARIABLE: &str = "BETTER_OS_TEST_SYSTEMCTL_GUARD";

/// How long one `systemctl` call may take before it is abandoned. A user
/// manager that is wedged must not hang a window or a command.
pub const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(10);

/// What a unit is doing, as systemd's `ActiveState` and the main process's
/// exit describe it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnitActivity {
    /// `active` or `reloading`.
    Running,
    /// `activating`, which includes a service waiting to be restarted after it
    /// crashed.
    Starting,
    /// `inactive` after its main process ran and exited 0 with
    /// `Result=success`. A service that has nothing to do on this machine ends
    /// like this on purpose, and it is not a problem.
    Exited,
    /// `inactive` for any other reason, including never having been started,
    /// and `deactivating`.
    Stopped,
    /// `failed`.
    Failed,
}

/// What systemd says about one unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitState {
    pub unit: String,
    pub enabled: bool,
    /// `UnitFileState=masked`: the unit was deliberately blocked, and enabling
    /// or starting it fails until someone unmasks it.
    pub masked: bool,
    pub activity: UnitActivity,
}

impl UnitState {
    pub fn new(unit: &str, enabled: bool, activity: UnitActivity) -> Self {
        Self {
            unit: unit.to_string(),
            enabled,
            masked: false,
            activity,
        }
    }

    /// Enabled and running.
    pub fn running(unit: &str) -> Self {
        Self::new(unit, true, UnitActivity::Running)
    }

    /// Neither enabled nor running.
    pub fn stopped(unit: &str) -> Self {
        Self::new(unit, false, UnitActivity::Stopped)
    }

    /// Masked, and therefore neither enabled nor running.
    pub fn masked(unit: &str) -> Self {
        Self {
            masked: true,
            ..Self::new(unit, false, UnitActivity::Stopped)
        }
    }

    /// Whether the unit is running now.
    pub fn is_active(&self) -> bool {
        self.activity == UnitActivity::Running
    }

    /// Enabled, and either running or finished on its own with success. This
    /// is what an enabled component's unit has to be.
    pub fn is_satisfied(&self) -> bool {
        self.enabled && matches!(self.activity, UnitActivity::Running | UnitActivity::Exited)
    }
}

/// Why a unit could not be enabled, disabled, or queried.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UnitFailureKind {
    /// systemd answered, and the answer was a failure.
    #[default]
    Failed,
    /// There is no systemd user manager to ask: the session bus cannot be
    /// reached or `systemctl` is not installed. Nothing about the unit is
    /// known, so nothing about the component may be concluded from it.
    NoSession,
}

/// One unit that could not be enabled, disabled, or queried, with the words
/// systemctl used. The name travels separately from the message because the
/// presentation layer has to name the unit even when the message is terse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitFailure {
    pub unit: String,
    pub message: String,
    pub kind: UnitFailureKind,
}

impl UnitFailure {
    pub fn new(unit: &str, message: impl Into<String>) -> Self {
        Self {
            unit: unit.to_string(),
            message: message.into(),
            kind: UnitFailureKind::Failed,
        }
    }

    pub fn no_session(unit: &str, message: impl Into<String>) -> Self {
        Self {
            unit: unit.to_string(),
            message: message.into(),
            kind: UnitFailureKind::NoSession,
        }
    }

    pub fn is_no_session(&self) -> bool {
        self.kind == UnitFailureKind::NoSession
    }
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
    /// Enables each unit and restarts it, which starts it when it was not
    /// running. An update or a restore replaces the program a running unit
    /// executes, and only a restart makes the new one run.
    fn enable_and_restart(&self, units: &[String]) -> Result<(), UnitFailure>;
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
/// leading option, and no character systemctl would interpret. A template
/// (`name@.service`) is only a unit once it has an instance.
pub fn is_plain_unit_name(unit: &str) -> bool {
    let Some(stem) = unit.strip_suffix(".service") else {
        return false;
    };
    !stem.is_empty()
        && unit.len() <= 255
        && !unit.starts_with('-')
        && !stem.starts_with('@')
        && !stem.ends_with('@')
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

/// Why one `systemctl` invocation produced no answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemctlError {
    pub kind: UnitFailureKind,
    pub message: String,
}

/// Runs one invocation and returns its standard output.
pub trait SystemctlRunner: Send + Sync {
    fn run(&self, invocation: &SystemctlInvocation) -> Result<String, SystemctlError>;
}

/// Whether systemctl's own words mean there was no user manager to talk to,
/// rather than a user manager that said no.
pub fn is_no_session_message(message: &str) -> bool {
    [
        "Failed to connect to bus",
        "Failed to connect to user scope bus",
        "$DBUS_SESSION_BUS_ADDRESS",
        "$XDG_RUNTIME_DIR",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

/// Runs a command with its output captured, and gives up on it after
/// `timeout`. `Ok(None)` is a timeout: the child has been killed and reaped.
///
/// Both pipes are drained on their own threads while the child runs, so a
/// child that writes more than a pipe holds cannot deadlock the wait.
pub fn run_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> std::io::Result<Option<Output>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut buffer);
        }
        buffer
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut buffer);
        }
        buffer
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(status.map(|status| Output {
        status,
        stdout,
        stderr,
    }))
}

/// Runs `systemctl` as a child process, with `LC_ALL=C` so its messages can be
/// read, and a timeout so a wedged user manager cannot hang the caller.
#[derive(Clone, Debug)]
pub struct ProcessRunner {
    timeout: Duration,
    guard: Option<PathBuf>,
}

impl ProcessRunner {
    /// The runner the product uses: [`SYSTEMCTL_TIMEOUT`], and refusing to run
    /// anything when [`SYSTEMCTL_GUARD_VARIABLE`] is set.
    pub fn from_environment() -> Self {
        Self {
            timeout: SYSTEMCTL_TIMEOUT,
            guard: std::env::var_os(SYSTEMCTL_GUARD_VARIABLE)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        }
    }

    /// A runner that never spawns anything and records each invocation in
    /// `log`, which is what the guard variable selects.
    pub fn guarded(log: impl Into<PathBuf>) -> Self {
        Self {
            timeout: SYSTEMCTL_TIMEOUT,
            guard: Some(log.into()),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl SystemctlRunner for ProcessRunner {
    fn run(&self, invocation: &SystemctlInvocation) -> Result<String, SystemctlError> {
        if let Some(log) = &self.guard {
            let line = std::iter::once(invocation.program.to_string())
                .chain(invocation.arguments.iter().cloned())
                .collect::<Vec<_>>()
                .join(" ");
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .and_then(|mut file| writeln!(file, "{line}"));
            return Err(SystemctlError {
                kind: UnitFailureKind::NoSession,
                message: format!("systemctl is not run while {SYSTEMCTL_GUARD_VARIABLE} is set"),
            });
        }

        let mut command = Command::new(invocation.program);
        command.args(&invocation.arguments).env("LC_ALL", "C");
        let output = match run_with_timeout(command, self.timeout) {
            Ok(Some(output)) => output,
            Ok(None) => {
                return Err(SystemctlError {
                    kind: UnitFailureKind::Failed,
                    message: format!(
                        "systemctl did not answer within {} seconds",
                        self.timeout.as_secs_f32()
                    ),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(SystemctlError {
                    kind: UnitFailureKind::NoSession,
                    message: "systemctl is not installed".to_string(),
                });
            }
            Err(error) => {
                return Err(SystemctlError {
                    kind: UnitFailureKind::Failed,
                    message: format!("systemctl could not be run: {error}"),
                });
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("systemctl exited with {}", output.status));
            let kind = if is_no_session_message(&stderr) {
                UnitFailureKind::NoSession
            } else {
                UnitFailureKind::Failed
            };
            return Err(SystemctlError { kind, message });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// How long a unit that was just started is watched before its state counts.
///
/// A service that crashes as it starts is `active` for a moment and then
/// `failed`, or `activating` while systemd waits to restart it. Reading the
/// state once, straight after `systemctl` returned, sees neither.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettleSchedule {
    pub attempts: u32,
    pub interval: Duration,
}

impl Default for SettleSchedule {
    /// Every 200 ms for up to two seconds.
    fn default() -> Self {
        Self {
            attempts: 10,
            interval: Duration::from_millis(200),
        }
    }
}

type Sleeper = Arc<dyn Fn(Duration) + Send + Sync>;

/// The real controller: `systemctl --user`, one unit at a time.
#[derive(Clone)]
pub struct SystemdUserUnitController {
    runner: Arc<dyn SystemctlRunner>,
    settle: SettleSchedule,
    sleep: Sleeper,
}

impl fmt::Debug for SystemdUserUnitController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemdUserUnitController")
            .field("settle", &self.settle)
            .finish_non_exhaustive()
    }
}

impl Default for SystemdUserUnitController {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemdUserUnitController {
    /// The controller the product uses: [`ProcessRunner::from_environment`],
    /// the default settle schedule, and a real sleep.
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessRunner::from_environment()),
            settle: SettleSchedule::default(),
            sleep: Arc::new(std::thread::sleep),
        }
    }

    /// Replaces what runs each invocation.
    pub fn with_runner(mut self, runner: Arc<dyn SystemctlRunner>) -> Self {
        self.runner = runner;
        self
    }

    /// Replaces how a started unit is watched, and what waits between looks.
    pub fn with_settle(
        mut self,
        settle: SettleSchedule,
        sleep: impl Fn(Duration) + Send + Sync + 'static,
    ) -> Self {
        self.settle = settle;
        self.sleep = Arc::new(sleep);
        self
    }

    /// `systemctl --user enable --now <unit>`.
    pub fn enable_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(unit, &["enable", "--now"])
    }

    /// `systemctl --user enable <unit>`, which links it and starts nothing.
    pub fn enable_only_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(unit, &["enable"])
    }

    /// `systemctl --user restart <unit>`, which starts a unit that is not
    /// running and stops and starts one that is.
    pub fn restart_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(unit, &["restart"])
    }

    /// `systemctl --user disable --now <unit>`.
    pub fn disable_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(unit, &["disable", "--now"])
    }

    /// `systemctl --user show` with the properties [`parse_unit_state`] reads.
    pub fn show_invocation(unit: &str) -> Result<SystemctlInvocation, UnitFailure> {
        Self::invocation(
            unit,
            &[
                "show",
                "--property=UnitFileState",
                "--property=ActiveState",
                "--property=Result",
                "--property=ExecMainCode",
                "--property=ExecMainStatus",
            ],
        )
    }

    fn invocation(unit: &str, arguments: &[&str]) -> Result<SystemctlInvocation, UnitFailure> {
        if !is_plain_unit_name(unit) {
            return Err(UnitFailure::new(unit, "not a plain unit name"));
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

    fn call(&self, unit: &str, invocation: &SystemctlInvocation) -> Result<String, UnitFailure> {
        self.runner.run(invocation).map_err(|error| UnitFailure {
            unit: unit.to_string(),
            message: error.message,
            kind: error.kind,
        })
    }

    fn state_of(&self, unit: &str) -> Result<UnitState, UnitFailure> {
        let invocation = Self::show_invocation(unit)?;
        let output = self.call(unit, &invocation)?;
        Ok(parse_unit_state(unit, &output))
    }

    /// Watches a unit that was just started until it has either come up or
    /// visibly failed. `activating` is not an answer yet; still `activating`
    /// when the schedule runs out is a failure, because a service systemd
    /// keeps restarting is not one that started.
    fn settle(&self, unit: &str) -> Result<(), UnitFailure> {
        if self.settle.attempts == 0 {
            return Ok(());
        }
        for _ in 0..self.settle.attempts {
            (self.sleep)(self.settle.interval);
            let state = self.state_of(unit)?;
            match state.activity {
                UnitActivity::Running | UnitActivity::Exited => return Ok(()),
                UnitActivity::Starting => continue,
                UnitActivity::Failed => {
                    return Err(UnitFailure::new(
                        unit,
                        "the service failed right after it started",
                    ));
                }
                UnitActivity::Stopped => {
                    return Err(UnitFailure::new(
                        unit,
                        "the service stopped right after it started",
                    ));
                }
            }
        }
        let waited = self.settle.interval * self.settle.attempts;
        Err(UnitFailure::new(
            unit,
            format!(
                "the service was still activating after {:.1} seconds; it may be crashing and being restarted",
                waited.as_secs_f32()
            ),
        ))
    }
}

impl UserUnitController for SystemdUserUnitController {
    fn enable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        for unit in units {
            let invocation = Self::enable_invocation(unit)?;
            self.call(unit, &invocation)?;
            self.settle(unit)?;
        }
        Ok(())
    }

    fn disable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        for unit in units {
            let invocation = Self::disable_invocation(unit)?;
            self.call(unit, &invocation)?;
        }
        Ok(())
    }

    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, UnitFailure> {
        units.iter().map(|unit| self.state_of(unit)).collect()
    }

    fn enable_and_restart(&self, units: &[String]) -> Result<(), UnitFailure> {
        for unit in units {
            let enable = Self::enable_only_invocation(unit)?;
            self.call(unit, &enable)?;
            let restart = Self::restart_invocation(unit)?;
            self.call(unit, &restart)?;
            self.settle(unit)?;
        }
        Ok(())
    }
}

/// Reads the properties `show_invocation` asked for.
///
/// A unit systemd has never heard of is not an error: `show` exits zero and
/// prints an empty `UnitFileState`, so it reads as not enabled and not running,
/// which is what the caller needs to know.
///
/// `Result=success` alone does not mean "ran and finished": a unit that was
/// never started reports it too, with `ExecMainCode=0`. Only a main process
/// that exited (`ExecMainCode=1`, the kernel's `CLD_EXITED`) with status 0
/// counts as a service that finished on its own.
pub fn parse_unit_state(unit: &str, output: &str) -> UnitState {
    let mut unit_file_state = "";
    let mut active_state = "";
    let mut result = "";
    let mut exec_main_code = "";
    let mut exec_main_status = "";
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            "UnitFileState" => unit_file_state = value,
            "ActiveState" => active_state = value,
            "Result" => result = value,
            "ExecMainCode" => exec_main_code = value,
            "ExecMainStatus" => exec_main_status = value,
            _ => {}
        }
    }
    let activity = match active_state {
        "active" | "reloading" => UnitActivity::Running,
        "activating" => UnitActivity::Starting,
        "failed" => UnitActivity::Failed,
        "inactive" if result == "success" && exec_main_code == "1" && exec_main_status == "0" => {
            UnitActivity::Exited
        }
        _ => UnitActivity::Stopped,
    };
    UnitState {
        unit: unit.to_string(),
        enabled: unit_file_state_is_enabled(unit_file_state),
        masked: matches!(unit_file_state, "masked" | "masked-runtime"),
        activity,
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
    EnableAndRestart(Vec<String>),
}

#[derive(Debug, Default)]
struct FakeUnitState {
    units: BTreeMap<String, UnitState>,
    failing_enable: BTreeMap<String, String>,
    failing_disable: BTreeMap<String, String>,
    unreadable_states: Option<String>,
    no_session: Option<String>,
    restarted: BTreeSet<String>,
    calls: Vec<FakeUnitCall>,
}

impl FakeUnitState {
    fn refuse_start(&self, units: &[String]) -> Result<(), UnitFailure> {
        for unit in units {
            if let Some(message) = &self.no_session {
                return Err(UnitFailure::no_session(unit, message.clone()));
            }
            if self.units.get(unit).is_some_and(|state| state.masked) {
                return Err(UnitFailure::new(
                    unit,
                    format!("Unit file {unit} is masked."),
                ));
            }
            if let Some(message) = self.failing_enable.get(unit) {
                return Err(UnitFailure::new(unit, message.clone()));
            }
        }
        Ok(())
    }
}

/// A controller that answers from memory, for tests.
///
/// A unit it has never been told about starts out stopped, so a test only has
/// to name the units whose starting state matters. Enabling marks a unit
/// enabled and running; disabling marks it neither, which is what the two
/// `systemctl` verbs do. A masked unit refuses to be enabled, as systemd does.
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
        self.lock()
            .failing_enable
            .insert(unit.to_string(), message.to_string());
        self
    }

    pub fn with_failing_disable(self, unit: &str, message: &str) -> Self {
        self.lock()
            .failing_disable
            .insert(unit.to_string(), message.to_string());
        self
    }

    pub fn with_unreadable_states(self, message: &str) -> Self {
        self.lock().unreadable_states = Some(message.to_string());
        self
    }

    /// Every call fails the way it does when there is no user session.
    pub fn with_no_session(self, message: &str) -> Self {
        self.lock().no_session = Some(message.to_string());
        self
    }

    pub fn calls(&self) -> Vec<FakeUnitCall> {
        self.lock().calls.clone()
    }

    pub fn state(&self, unit: &str) -> Option<UnitState> {
        self.lock().units.get(unit).cloned()
    }

    /// Whether `enable_and_restart` found this unit running and restarted it,
    /// as opposed to starting a unit that was not running.
    pub fn was_restarted(&self, unit: &str) -> bool {
        self.lock().restarted.contains(unit)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeUnitState> {
        self.state.lock().expect("the fake is not poisoned")
    }
}

impl UserUnitController for FakeUserUnitController {
    fn enable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        let mut state = self.lock();
        state.calls.push(FakeUnitCall::Enable(units.to_vec()));
        state.refuse_start(units)?;
        for unit in units {
            state.units.insert(unit.clone(), UnitState::running(unit));
        }
        Ok(())
    }

    fn disable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        let mut state = self.lock();
        state.calls.push(FakeUnitCall::Disable(units.to_vec()));
        for unit in units {
            if let Some(message) = &state.no_session {
                return Err(UnitFailure::no_session(unit, message.clone()));
            }
            if let Some(message) = state.failing_disable.get(unit) {
                return Err(UnitFailure::new(unit, message.clone()));
            }
        }
        for unit in units {
            let masked = state.units.get(unit).is_some_and(|state| state.masked);
            let mut stopped = UnitState::stopped(unit);
            stopped.masked = masked;
            state.units.insert(unit.clone(), stopped);
        }
        Ok(())
    }

    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, UnitFailure> {
        let mut state = self.lock();
        state.calls.push(FakeUnitCall::States(units.to_vec()));
        let first = units.first().cloned().unwrap_or_default();
        if let Some(message) = &state.no_session {
            return Err(UnitFailure::no_session(&first, message.clone()));
        }
        if let Some(message) = &state.unreadable_states {
            return Err(UnitFailure::new(&first, message.clone()));
        }
        Ok(units
            .iter()
            .map(|unit| {
                state
                    .units
                    .get(unit)
                    .cloned()
                    .unwrap_or_else(|| UnitState::stopped(unit))
            })
            .collect())
    }

    fn enable_and_restart(&self, units: &[String]) -> Result<(), UnitFailure> {
        let mut state = self.lock();
        state
            .calls
            .push(FakeUnitCall::EnableAndRestart(units.to_vec()));
        state.refuse_start(units)?;
        for unit in units {
            if state.units.get(unit).is_some_and(UnitState::is_active) {
                state.restarted.insert(unit.clone());
            }
            state.units.insert(unit.clone(), UnitState::running(unit));
        }
        Ok(())
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

    /// A template with no instance names no unit systemctl can start: `enable`
    /// would need an instance and `show` reports the template itself.
    #[test]
    fn a_template_with_no_instance_is_not_a_unit() {
        assert!(is_plain_unit_name("getty@tty1.service"));
        for name in ["foo@.service", "@.service", "@foo.service"] {
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

        let enable_only =
            SystemdUserUnitController::enable_only_invocation("better-awake.service").unwrap();
        assert_eq!(
            enable_only.arguments,
            ["--user", "enable", "better-awake.service"]
        );

        let restart =
            SystemdUserUnitController::restart_invocation("better-awake.service").unwrap();
        assert_eq!(
            restart.arguments,
            ["--user", "restart", "better-awake.service"]
        );

        let show = SystemdUserUnitController::show_invocation("better-awake.service").unwrap();
        assert_eq!(
            show.arguments,
            [
                "--user",
                "show",
                "--property=UnitFileState",
                "--property=ActiveState",
                "--property=Result",
                "--property=ExecMainCode",
                "--property=ExecMainStatus",
                "better-awake.service",
            ]
        );

        for invocation in [&enable, &disable, &enable_only, &restart, &show] {
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
            "foo@.service",
        ] {
            for invocation in [
                SystemdUserUnitController::enable_invocation(unit),
                SystemdUserUnitController::restart_invocation(unit),
                SystemdUserUnitController::show_invocation(unit),
            ] {
                assert!(matches!(invocation, Err(UnitFailure { .. })), "{unit}");
            }
        }
    }

    #[test]
    fn the_properties_are_read_into_enabled_and_activity() {
        // The shape `systemctl --user show` prints on this host.
        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=enabled\nActiveState=active\n",
        );
        assert!(state.enabled && state.is_active());

        // `static` is enabled by definition and has nothing to link.
        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=static\nActiveState=active\n",
        );
        assert!(state.enabled && state.is_active());

        // A unit systemd has never heard of prints an empty state and exits 0.
        let state = parse_unit_state(
            "better-awake.service",
            "LoadState=not-found\nActiveState=inactive\nUnitFileState=\n",
        );
        assert!(!state.enabled && !state.is_active());

        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=enabled\nActiveState=failed\nResult=exit-code\n",
        );
        assert!(state.enabled && state.activity == UnitActivity::Failed);

        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=enabled\nActiveState=activating\n",
        );
        assert_eq!(state.activity, UnitActivity::Starting);

        let state = parse_unit_state(
            "better-awake.service",
            "UnitFileState=masked\nActiveState=inactive\n",
        );
        assert!(state.masked && !state.enabled);
    }

    /// The touchpad gesture service exits 0 on purpose when it has nothing to
    /// do, and its unit is `Restart=on-failure`, so systemd leaves it
    /// `inactive` with `Result=success`. That is an enabled component working
    /// as designed. A unit that was never started reads `Result=success` too,
    /// and it is told apart by `ExecMainCode=0`: no main process ever exited.
    /// Both shapes are what `systemctl --user show` printed on this host.
    #[test]
    fn a_service_that_finished_successfully_is_satisfied_and_a_never_started_one_is_not() {
        let finished = parse_unit_state(
            "better-touchpad-gestures.service",
            "Result=success\nExecMainCode=1\nExecMainStatus=0\nActiveState=inactive\nUnitFileState=enabled\n",
        );
        assert_eq!(finished.activity, UnitActivity::Exited);
        assert!(finished.is_satisfied());

        let never_started = parse_unit_state(
            "better-touchpad-gestures.service",
            "Result=success\nExecMainCode=0\nExecMainStatus=0\nActiveState=inactive\nUnitFileState=enabled\n",
        );
        assert_eq!(never_started.activity, UnitActivity::Stopped);
        assert!(!never_started.is_satisfied());

        let exited_with_an_error = parse_unit_state(
            "better-touchpad-gestures.service",
            "Result=success\nExecMainCode=1\nExecMainStatus=3\nActiveState=inactive\nUnitFileState=enabled\n",
        );
        assert!(!exited_with_an_error.is_satisfied());
    }

    #[test]
    fn the_fake_starts_unknown_units_stopped_and_records_what_it_was_asked() {
        let units = FakeUserUnitController::new(&[]);
        assert_eq!(
            units.states(&["better-awake.service".to_string()]).unwrap(),
            vec![UnitState::stopped("better-awake.service")]
        );
        units
            .enable_now(&["better-awake.service".to_string()])
            .unwrap();
        assert_eq!(
            units.state("better-awake.service"),
            Some(UnitState::running("better-awake.service"))
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
    fn the_fake_tells_a_restart_from_a_first_start() {
        let units = FakeUserUnitController::new(&[UnitState::running("a.service")]);
        units
            .enable_and_restart(&["a.service".to_string(), "b.service".to_string()])
            .unwrap();
        assert!(units.was_restarted("a.service"));
        assert!(!units.was_restarted("b.service"));
        assert_eq!(
            units.state("b.service"),
            Some(UnitState::running("b.service"))
        );
    }

    #[test]
    fn a_packages_user_units_are_the_service_files_in_its_own_directory() {
        let files = InstalledFiles {
            paths: [
                "/usr/lib/systemd/user",
                "/usr/lib/systemd/user/better-awake.service",
                "/usr/lib/systemd/user/better-awake.service.d/override.conf",
                "/usr/lib/systemd/user/better-template@.service",
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

    /// Replays scripted `systemctl` answers, in order, and records what it was
    /// asked. Nothing is spawned.
    struct ScriptedRunner {
        answers: Mutex<Vec<Result<String, SystemctlError>>>,
        asked: Mutex<Vec<Vec<String>>>,
    }

    impl ScriptedRunner {
        fn new(answers: Vec<Result<&str, SystemctlError>>) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(
                    answers
                        .into_iter()
                        .rev()
                        .map(|answer| answer.map(str::to_string))
                        .collect(),
                ),
                asked: Mutex::new(Vec::new()),
            })
        }

        fn verbs(&self) -> Vec<String> {
            self.asked
                .lock()
                .unwrap()
                .iter()
                .map(|arguments| arguments[1].clone())
                .collect()
        }
    }

    impl SystemctlRunner for ScriptedRunner {
        fn run(&self, invocation: &SystemctlInvocation) -> Result<String, SystemctlError> {
            self.asked
                .lock()
                .unwrap()
                .push(invocation.arguments.clone());
            self.answers
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| panic!("no answer scripted for {:?}", invocation.arguments))
        }
    }

    const ACTIVATING: &str = "UnitFileState=enabled\nActiveState=activating\n";
    const ACTIVE: &str = "UnitFileState=enabled\nActiveState=active\n";
    const FAILED: &str = "UnitFileState=enabled\nActiveState=failed\nResult=exit-code\n";

    /// A controller over `runner` whose waits are counted, never slept.
    fn watched(runner: Arc<ScriptedRunner>) -> (SystemdUserUnitController, Arc<Mutex<u32>>) {
        let sleeps = Arc::new(Mutex::new(0));
        let counter = sleeps.clone();
        let controller = SystemdUserUnitController::new()
            .with_runner(runner)
            .with_settle(SettleSchedule::default(), move |interval| {
                assert_eq!(interval, Duration::from_millis(200));
                *counter.lock().unwrap() += 1;
            });
        (controller, sleeps)
    }

    #[test]
    fn a_unit_that_crashes_as_it_starts_fails_the_start() {
        let runner = ScriptedRunner::new(vec![Ok(""), Ok(ACTIVATING), Ok(FAILED)]);
        let (controller, sleeps) = watched(runner.clone());

        let failure = controller
            .enable_now(&["better-awake.service".to_string()])
            .expect_err("a unit that failed after starting did not start");

        assert_eq!(failure.unit, "better-awake.service");
        assert!(failure.message.contains("failed"), "{}", failure.message);
        assert_eq!(runner.verbs(), ["enable", "show", "show"]);
        assert_eq!(*sleeps.lock().unwrap(), 2);
    }

    #[test]
    fn a_unit_still_activating_after_two_seconds_is_reported_as_activating() {
        let mut answers = vec![Ok("")];
        answers.extend(std::iter::repeat_n(Ok(ACTIVATING), 10));
        let runner = ScriptedRunner::new(answers);
        let (controller, sleeps) = watched(runner.clone());

        let failure = controller
            .enable_now(&["better-awake.service".to_string()])
            .expect_err("a unit systemd keeps restarting did not start");

        assert!(
            failure.message.contains("activating") && failure.message.contains("2.0 seconds"),
            "{}",
            failure.message
        );
        assert_eq!(*sleeps.lock().unwrap(), 10, "two seconds at 200 ms");
    }

    #[test]
    fn a_unit_that_comes_up_after_activating_starts() {
        let runner = ScriptedRunner::new(vec![Ok(""), Ok(ACTIVATING), Ok(ACTIVE)]);
        let (controller, sleeps) = watched(runner.clone());

        controller
            .enable_now(&["better-awake.service".to_string()])
            .expect("the unit came up");

        assert_eq!(*sleeps.lock().unwrap(), 2);
    }

    #[test]
    fn restarting_enables_then_restarts_then_watches() {
        let runner = ScriptedRunner::new(vec![Ok(""), Ok(""), Ok(ACTIVE)]);
        let (controller, _) = watched(runner.clone());

        controller
            .enable_and_restart(&["better-awake.service".to_string()])
            .expect("the unit restarted");

        assert_eq!(runner.verbs(), ["enable", "restart", "show"]);
    }

    #[test]
    fn an_unreachable_user_bus_is_no_session_and_not_a_failed_unit() {
        assert!(is_no_session_message(
            "Failed to connect to bus: No medium found"
        ));
        assert!(is_no_session_message(
            "Failed to connect to user scope bus via local transport: $DBUS_SESSION_BUS_ADDRESS and $XDG_RUNTIME_DIR not defined"
        ));
        assert!(!is_no_session_message(
            "Job for better-awake.service failed because the control process exited with error code."
        ));
    }

    /// The guard is how a test drives a real binary in its real mode without
    /// it ever reaching the user manager of whoever runs the tests.
    #[test]
    fn a_guarded_runner_spawns_nothing_and_records_what_it_was_asked() {
        let log = std::env::temp_dir().join(format!(
            "better-os-systemctl-guard-{}-{:?}",
            std::process::id(),
            Instant::now()
        ));
        let controller =
            SystemdUserUnitController::new().with_runner(Arc::new(ProcessRunner::guarded(&log)));

        let failure = controller
            .enable_now(&["better-awake.service".to_string()])
            .expect_err("a guarded runner runs nothing");

        assert!(failure.is_no_session(), "{failure:?}");
        let recorded = std::fs::read_to_string(&log).unwrap();
        let _ = std::fs::remove_file(&log);
        assert_eq!(
            recorded,
            "systemctl --user enable --now better-awake.service\n"
        );
    }

    #[test]
    fn a_command_that_outlives_its_timeout_is_killed_and_reported() {
        let mut command = Command::new("sleep");
        command.arg("5");
        let started = Instant::now();

        let outcome = run_with_timeout(command, Duration::from_millis(100)).unwrap();

        assert!(outcome.is_none(), "the command must time out");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_command_that_finishes_in_time_returns_its_output() {
        let mut command = Command::new("printf");
        command.arg("ActiveState=active");

        let output = run_with_timeout(command, Duration::from_secs(5))
            .unwrap()
            .expect("printf finishes");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"ActiveState=active");
    }

    /// The real runner, pointed at a program that does not exist: a machine
    /// with no `systemctl` has no user manager to ask.
    #[test]
    fn a_missing_systemctl_is_no_session() {
        let runner = ProcessRunner {
            timeout: Duration::from_secs(1),
            guard: None,
        };
        let error = runner
            .run(&SystemctlInvocation {
                program: "better-os-no-such-systemctl",
                arguments: vec!["--user".to_string()],
            })
            .expect_err("nothing answered");
        assert_eq!(error.kind, UnitFailureKind::NoSession, "{error:?}");
    }

    /// The real runner, pointed at a program that outlives the timeout.
    #[test]
    fn the_real_runner_gives_up_on_a_systemctl_that_does_not_answer() {
        let runner = ProcessRunner {
            timeout: Duration::from_millis(100),
            guard: None,
        };
        let error = runner
            .run(&SystemctlInvocation {
                program: "sleep",
                arguments: vec!["5".to_string()],
            })
            .expect_err("the call timed out");
        assert_eq!(error.kind, UnitFailureKind::Failed);
        assert!(
            error.message.contains("did not answer"),
            "{}",
            error.message
        );
    }
}
