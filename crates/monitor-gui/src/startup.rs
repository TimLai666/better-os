//! Startup apps and the background services this session runs.
//!
//! Two lists, read and changed without root:
//!
//! - **Startup apps** are the XDG autostart entries: `.desktop` files in
//!   `$XDG_CONFIG_DIRS/autostart` (for everyone, usually `/etc/xdg/autostart`)
//!   and `$XDG_CONFIG_HOME/autostart` (this person's own, which win over a
//!   system file of the same name). Turning one off writes this person's own
//!   copy with `Hidden=true` and `X-GNOME-Autostart-enabled=false`, which is
//!   what the XDG specification and GNOME read; turning it back on writes the
//!   same copy with both cleared. A system file is never touched.
//! - **Background services** are the systemd user units, read from
//!   `systemctl --user`. Enabling and disabling one needs no privilege. A unit
//!   a Better OS component installed is managed by Better Manager, never by a
//!   window (AGENTS.md), so it is listed and not offered.
//!
//! Everything here is plain functions over text and paths, so the rules are
//! tested without a desktop or a user manager. The window runs the reads and
//! writes on a background thread.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Who an autostart entry belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Scope {
    /// Only in this person's `~/.config/autostart`.
    Yours,
    /// Installed for everyone; this person has no copy of their own.
    Everyone,
    /// Installed for everyone, and this person's own copy overrides it.
    EveryoneChangedByYou,
}

/// One startup app.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutostartEntry {
    /// The file name, which is the entry's identity across the two folders.
    pub(crate) file_name: String,
    pub(crate) name: String,
    pub(crate) comment: Option<String>,
    pub(crate) command: Option<String>,
    pub(crate) icon: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) scope: Scope,
    /// False when `OnlyShowIn` or `NotShowIn` keep it from starting on this
    /// desktop, whatever its switch says.
    pub(crate) for_this_desktop: bool,
    /// The file that is in force.
    pub(crate) path: PathBuf,
    /// The everyone file this person's copy overrides, if there is one.
    pub(crate) system_path: Option<PathBuf>,
    /// False when the entry's `TryExec` program is not installed, which is
    /// the XDG rule for "this entry does not apply".
    pub(crate) installed: bool,
}

/// Where autostart entries live.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutostartDirs {
    /// `$XDG_CONFIG_HOME/autostart`.
    pub(crate) user: PathBuf,
    /// `$XDG_CONFIG_DIRS/*/autostart`, most important first.
    pub(crate) system: Vec<PathBuf>,
    /// `$XDG_CURRENT_DESKTOP`, split on `:`.
    pub(crate) desktops: Vec<String>,
    /// `$PATH`, for `TryExec`.
    pub(crate) path: Option<std::ffi::OsString>,
}

impl AutostartDirs {
    /// The directories the process's environment names, by the XDG rules.
    pub(crate) fn from_environment() -> Option<Self> {
        let user = better_ui::fluent::theme::config_home(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        )?
        .join("autostart");
        let system = std::env::var("XDG_CONFIG_DIRS")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/etc/xdg".to_string())
            .split(':')
            .filter(|dir| Path::new(dir).is_absolute())
            .map(|dir| PathBuf::from(dir).join("autostart"))
            .collect();
        let desktops = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect();
        Some(Self {
            user,
            system,
            desktops,
            path: std::env::var_os("PATH"),
        })
    }
}

/// The `[Desktop Entry]` group of a desktop file, as ordered key/value lines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DesktopEntry {
    pub(crate) fields: BTreeMap<String, String>,
}

impl DesktopEntry {
    pub(crate) fn parse(text: &str) -> Self {
        let mut fields = BTreeMap::new();
        let mut in_entry = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                fields
                    .entry(key.trim().to_string())
                    .or_insert_with(|| value.trim().to_string());
            }
        }
        Self { fields }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn flag(&self, key: &str) -> Option<bool> {
        self.get(key)
            .map(|value| value.eq_ignore_ascii_case("true"))
    }

    /// A localized key (`Name[zh_TW]`), falling back to the plain one.
    pub(crate) fn localized(&self, key: &str, chinese: bool) -> Option<&str> {
        if chinese {
            for tag in ["zh_TW", "zh_Hant", "zh_HK", "zh"] {
                if let Some(value) = self.get(&format!("{key}[{tag}]")) {
                    return Some(value);
                }
            }
        }
        self.get(key)
    }

    /// Whether the entry starts at all: neither `Hidden` nor GNOME's own
    /// switch turned off.
    pub(crate) fn starts(&self) -> bool {
        self.flag("Hidden") != Some(true) && self.flag("X-GNOME-Autostart-enabled") != Some(false)
    }

    /// Whether `OnlyShowIn` and `NotShowIn` let it start on these desktops.
    pub(crate) fn for_desktops(&self, desktops: &[String]) -> bool {
        let list = |key: &str| -> Vec<String> {
            self.get(key)
                .unwrap_or_default()
                .split(';')
                .filter(|name| !name.is_empty())
                .map(|name| name.to_ascii_lowercase())
                .collect()
        };
        let current: Vec<String> = desktops.iter().map(|d| d.to_ascii_lowercase()).collect();
        let only = list("OnlyShowIn");
        let not = list("NotShowIn");
        if !only.is_empty() && !only.iter().any(|name| current.contains(name)) {
            return false;
        }
        !not.iter().any(|name| current.contains(name))
    }
}

fn desktop_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.ends_with(".desktop").then(|| (name, entry.path()))
        })
        .collect();
    files.sort();
    files
}

/// Whether a `TryExec` program exists: an absolute path that is an
/// executable file, or a name found in one of `PATH`'s directories.
pub(crate) fn program_installed(program: &str, path: Option<&std::ffi::OsStr>) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let executable = |candidate: &Path| {
        std::fs::metadata(candidate)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    };
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        return executable(candidate);
    }
    if program.contains('/') {
        return false;
    }
    path.map(std::env::split_paths)
        .into_iter()
        .flatten()
        .any(|dir| executable(&dir.join(program)))
}

/// Every startup app, by name. `chinese` picks the zh-TW names where a file
/// carries them.
pub(crate) fn read_autostart(dirs: &AutostartDirs, chinese: bool) -> Vec<AutostartEntry> {
    let mut system: BTreeMap<String, PathBuf> = BTreeMap::new();
    // The first directory in XDG_CONFIG_DIRS wins, so read them in reverse
    // and let earlier ones overwrite.
    for dir in dirs.system.iter().rev() {
        for (name, path) in desktop_files(dir) {
            system.insert(name, path);
        }
    }
    let user: BTreeMap<String, PathBuf> = desktop_files(&dirs.user).into_iter().collect();

    let mut names: Vec<&String> = system.keys().chain(user.keys()).collect();
    names.sort();
    names.dedup();
    let mut entries: Vec<AutostartEntry> = names
        .into_iter()
        .filter_map(|file_name| {
            let system_path = system.get(file_name).cloned();
            let (path, scope) = match (user.get(file_name), system.get(file_name)) {
                (Some(path), Some(_)) => (path.clone(), Scope::EveryoneChangedByYou),
                (Some(path), None) => (path.clone(), Scope::Yours),
                (None, Some(path)) => (path.clone(), Scope::Everyone),
                (None, None) => return None,
            };
            let entry = DesktopEntry::parse(&std::fs::read_to_string(&path).ok()?);
            if entry.get("Type").is_some_and(|kind| kind != "Application") {
                return None;
            }
            let name = entry
                .localized("Name", chinese)
                .map(str::to_string)
                .unwrap_or_else(|| file_name.trim_end_matches(".desktop").to_string());
            Some(AutostartEntry {
                file_name: file_name.clone(),
                name,
                comment: entry.localized("Comment", chinese).map(str::to_string),
                command: entry.get("Exec").map(str::to_string),
                icon: entry.get("Icon").map(str::to_string),
                enabled: entry.starts(),
                scope,
                for_this_desktop: entry.for_desktops(&dirs.desktops),
                installed: entry
                    .get("TryExec")
                    .is_none_or(|program| program_installed(program, dirs.path.as_deref())),
                path,
                system_path,
            })
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

/// The text of this person's copy of an entry, switched on or off.
///
/// `source` is the file in force now. Every line is kept except the two
/// switches, which are rewritten inside `[Desktop Entry]`, so a translated
/// name or an action group survives the change.
pub(crate) fn switched(source: &str, enabled: bool) -> String {
    let mut out = String::new();
    let mut in_entry = false;
    let mut wrote = false;
    let switches = |out: &mut String| {
        out.push_str(&format!(
            "Hidden={}\nX-GNOME-Autostart-enabled={}\n",
            !enabled, enabled
        ));
    };
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_entry && !wrote {
                switches(&mut out);
                wrote = true;
            }
            in_entry = trimmed == "[Desktop Entry]";
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_entry {
            let key = trimmed.split_once('=').map(|(key, _)| key.trim());
            if matches!(key, Some("Hidden") | Some("X-GNOME-Autostart-enabled")) {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_entry && !wrote {
        switches(&mut out);
        wrote = true;
    }
    if !wrote {
        out.push_str("[Desktop Entry]\n");
        switches(&mut out);
    }
    out
}

/// Turn a startup app on or off by writing this person's own copy of it.
/// Written to a temporary file and renamed into place.
///
/// Turning an everyone entry back on removes this person's copy instead,
/// when the copy differs from the everyone file only in the two switches and
/// the everyone file starts on its own: the default is then in force again,
/// and a later change to the installed file reaches this person too.
pub(crate) fn set_autostart(
    dirs: &AutostartDirs,
    entry: &AutostartEntry,
    enabled: bool,
) -> io::Result<()> {
    let target = dirs.user.join(&entry.file_name);
    if enabled
        && entry.scope == Scope::EveryoneChangedByYou
        && let Some(system_path) = &entry.system_path
    {
        let system = DesktopEntry::parse(&std::fs::read_to_string(system_path)?);
        let mine = DesktopEntry::parse(&std::fs::read_to_string(&entry.path)?);
        if system.starts() && same_apart_from_switches(&system, &mine) {
            return std::fs::remove_file(&target);
        }
    }
    let source = std::fs::read_to_string(&entry.path)?;
    let text = switched(&source, enabled);
    std::fs::create_dir_all(&dirs.user)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&dirs.user)?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(&target).map_err(|error| error.error)?;
    Ok(())
}

/// Whether two entries say the same thing once the switches are set aside.
pub(crate) fn same_apart_from_switches(a: &DesktopEntry, b: &DesktopEntry) -> bool {
    let strip = |entry: &DesktopEntry| {
        let mut fields = entry.fields.clone();
        fields.remove("Hidden");
        fields.remove("X-GNOME-Autostart-enabled");
        fields
    };
    strip(a) == strip(b)
}

/// What a user unit is doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Activity {
    Running,
    Starting,
    Stopping,
    Stopped,
    Failed,
    /// Ran and finished on purpose (`active (exited)` or a clean `inactive
    /// (dead)` with a one-shot).
    Finished,
}

/// Whether a unit starts with the session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupState {
    Enabled,
    Disabled,
    /// Started only when something else needs it; there is nothing to turn
    /// on or off.
    OnDemand,
    /// Blocked on purpose; it cannot be started until someone unblocks it.
    Masked,
    /// Created by a program for this session only.
    Temporary,
    Unknown,
}

/// One background service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServiceUnit {
    pub(crate) unit: String,
    pub(crate) description: String,
    pub(crate) activity: Activity,
    pub(crate) startup: StartupState,
}

/// Why a service cannot be switched from here, when it cannot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServiceLock {
    /// A Better OS component's unit: Better Manager owns it.
    BetterOs,
    /// Started on demand; enabling or disabling it changes nothing.
    OnDemand,
    Masked,
    Temporary,
    Unknown,
}

impl ServiceUnit {
    /// Whether enable and disable are offered, and why not.
    pub(crate) fn lock(&self) -> Option<ServiceLock> {
        if is_better_os_unit(&self.unit) {
            return Some(ServiceLock::BetterOs);
        }
        match self.startup {
            StartupState::Enabled | StartupState::Disabled => None,
            StartupState::OnDemand => Some(ServiceLock::OnDemand),
            StartupState::Masked => Some(ServiceLock::Masked),
            StartupState::Temporary => Some(ServiceLock::Temporary),
            StartupState::Unknown => Some(ServiceLock::Unknown),
        }
    }

    /// The name a person reads: the description where there is one,
    /// otherwise the unit without its `.service`.
    pub(crate) fn title(&self) -> &str {
        if self.description.is_empty() {
            self.unit.trim_end_matches(".service")
        } else {
            &self.description
        }
    }
}

/// Units that a Better OS package installs. Their state is the component's,
/// recorded by Better Manager, and a window must not change it behind the
/// manager's back.
pub(crate) fn is_better_os_unit(unit: &str) -> bool {
    unit.starts_with("better-") || unit.starts_with("org.betteros.")
}

fn activity(active: &str, sub: &str) -> Activity {
    match (active, sub) {
        ("active", "exited") => Activity::Finished,
        ("active", _) | ("reloading", _) => Activity::Running,
        ("activating", _) => Activity::Starting,
        ("deactivating", _) => Activity::Stopping,
        ("failed", _) => Activity::Failed,
        _ => Activity::Stopped,
    }
}

fn startup_state(state: &str) -> StartupState {
    match state {
        "enabled" | "enabled-runtime" | "linked" | "linked-runtime" | "alias" => {
            StartupState::Enabled
        }
        "disabled" => StartupState::Disabled,
        "static" | "indirect" | "generated" => StartupState::OnDemand,
        "masked" | "masked-runtime" => StartupState::Masked,
        "transient" => StartupState::Temporary,
        _ => StartupState::Unknown,
    }
}

/// Parse `systemctl --user list-units --type=service --all --plain
/// --no-legend --no-pager`: unit, load, active, sub, and a description that
/// may contain spaces.
pub(crate) fn parse_units(text: &str) -> Vec<(String, String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start_matches(['●', '*', ' ']);
            let mut parts = line.split_whitespace();
            let unit = parts.next()?.to_string();
            let load = parts.next()?;
            let active = parts.next()?.to_string();
            let sub = parts.next()?.to_string();
            if !unit.ends_with(".service") || load == "not-found" {
                return None;
            }
            let description = parts.collect::<Vec<_>>().join(" ");
            Some((unit, active, sub, description))
        })
        .collect()
}

/// Parse `systemctl --user list-unit-files --type=service --no-legend
/// --no-pager`: unit and its file state.
pub(crate) fn parse_unit_files(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let unit = parts.next()?;
            let state = parts.next()?;
            unit.ends_with(".service")
                .then(|| (unit.to_string(), state.to_string()))
        })
        .collect()
}

/// Both lists merged: every loaded unit with its state, and every installed
/// unit file that is not loaded, as stopped. Templates (`name@.service`) are
/// left out because there is nothing to run until an instance is named.
pub(crate) fn merge_services(units: &str, files: &str) -> Vec<ServiceUnit> {
    let files = parse_unit_files(files);
    let mut services: BTreeMap<String, ServiceUnit> = BTreeMap::new();
    for (unit, active, sub, description) in parse_units(units) {
        let startup = files
            .get(&unit)
            .map(|state| startup_state(state))
            .unwrap_or(StartupState::Temporary);
        services.insert(
            unit.clone(),
            ServiceUnit {
                unit,
                description,
                activity: activity(&active, &sub),
                startup,
            },
        );
    }
    for (unit, state) in &files {
        if unit.ends_with("@.service") || services.contains_key(unit) {
            continue;
        }
        services.insert(
            unit.clone(),
            ServiceUnit {
                unit: unit.clone(),
                description: String::new(),
                activity: Activity::Stopped,
                startup: startup_state(state),
            },
        );
    }
    let mut list: Vec<ServiceUnit> = services.into_values().collect();
    list.sort_by_key(|service| service.title().to_lowercase());
    list
}

/// Set in a test environment, it keeps this window from ever running
/// `systemctl`: the same variable the manager's unit controller honours.
pub(crate) const SYSTEMCTL_GUARD_VARIABLE: &str = "BETTER_OS_TEST_SYSTEMCTL_GUARD";

/// How long one `systemctl` call may take. A wedged user manager must not
/// leave a page waiting forever.
pub(crate) const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the services could not be read or changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ServiceError {
    /// There is no user service manager to ask (no session bus, or a test).
    NoManager(String),
    /// `systemctl` ran and said no.
    Refused(String),
    /// Not offered for this unit, and why (`ServiceUnit::lock`).
    Locked(ServiceLock),
    /// The name is not a plain `name.service`, so it is never passed on.
    InvalidName(String),
}

/// Something that runs `systemctl --user` with arguments.
pub(crate) trait Systemctl {
    fn run(&self, arguments: &[&str]) -> Result<String, ServiceError>;
}

/// The real `systemctl`, found on `PATH`.
pub(crate) struct RealSystemctl {
    program: std::ffi::OsString,
    /// Set when the test guard is: nothing is run at all.
    guarded: bool,
}

impl RealSystemctl {
    /// `systemctl` from `PATH`, guarded when the environment carries the
    /// test guard.
    pub(crate) fn from_environment() -> Self {
        Self {
            program: "systemctl".into(),
            guarded: std::env::var_os(SYSTEMCTL_GUARD_VARIABLE).is_some(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(program: impl Into<std::ffi::OsString>, guarded: bool) -> Self {
        Self {
            program: program.into(),
            guarded,
        }
    }
}

/// Read a pipe to its end on a thread of its own, so a child that writes
/// more than the pipe holds is never left blocked while it is waited on.
fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

impl Systemctl for RealSystemctl {
    fn run(&self, arguments: &[&str]) -> Result<String, ServiceError> {
        if self.guarded {
            return Err(ServiceError::NoManager(format!(
                "{SYSTEMCTL_GUARD_VARIABLE} is set"
            )));
        }
        let mut child = Command::new(&self.program)
            .arg("--user")
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| ServiceError::NoManager(error.to_string()))?;
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() > SYSTEMCTL_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ServiceError::NoManager("systemctl timed out".into()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(error) => return Err(ServiceError::NoManager(error.to_string())),
            }
        };
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        if status.success() {
            Ok(stdout)
        } else if stderr.contains("Failed to connect to bus")
            || stderr.contains("No medium found")
            || stderr.contains("$DBUS_SESSION_BUS_ADDRESS")
        {
            Err(ServiceError::NoManager(stderr.trim().to_string()))
        } else {
            Err(ServiceError::Refused(stderr.trim().to_string()))
        }
    }
}

/// Every background service the session's user manager knows.
pub(crate) fn read_services(systemctl: &dyn Systemctl) -> Result<Vec<ServiceUnit>, ServiceError> {
    let units = systemctl.run(&[
        "list-units",
        "--type=service",
        "--all",
        "--plain",
        "--no-legend",
        "--no-pager",
    ])?;
    let files = systemctl.run(&[
        "list-unit-files",
        "--type=service",
        "--no-legend",
        "--no-pager",
    ])?;
    Ok(merge_services(&units, &files))
}

/// Turn a service on or off at login. Refused here, before `systemctl` is
/// asked, for a unit that is locked.
pub(crate) fn set_service(
    systemctl: &dyn Systemctl,
    service: &ServiceUnit,
    enabled: bool,
) -> Result<(), ServiceError> {
    if let Some(lock) = service.lock() {
        return Err(ServiceError::Locked(lock));
    }
    if !plain_unit_name(&service.unit) {
        return Err(ServiceError::InvalidName(service.unit.clone()));
    }
    let verb = if enabled { "enable" } else { "disable" };
    // `--` ends the options, so no unit name can ever be read as one.
    systemctl.run(&[verb, "--", &service.unit]).map(|_| ())
}

/// A unit name systemctl will only read as a name: letters, digits, and
/// `:_.@\-` (systemd's own set), not starting with `-`, ending `.service`.
pub(crate) fn plain_unit_name(unit: &str) -> bool {
    let Some(stem) = unit.strip_suffix(".service") else {
        return false;
    };
    !stem.is_empty()
        && !unit.starts_with('-')
        && unit.len() <= 255
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ":_.@\\-".contains(c))
}
