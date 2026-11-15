//! Turning a component's GNOME Shell extension on and off.
//!
//! A package installs its extension under [`EXTENSION_DIRECTORY`] and does not
//! enable it; this is the half that makes the installed component work, the
//! same way [`crate::user_units`] is for its services (ticket 88). Which
//! extensions belong to a component comes from dpkg's own file list, never
//! from a table by component id.
//!
//! Enabling is adding the extension's UUID to the person's
//! `org.gnome.shell enabled-extensions` (and taking it out of
//! `disabled-extensions`); disabling is taking it out again. Every other entry
//! in either list is left exactly as it was. GNOME Shell loads an extension
//! installed after it started only at the next login, so writing the setting
//! is what can be done now, and the next login does the rest.
//!
//! None of it needs root. Everything is an argument vector, a UUID that is not
//! a plain extension UUID is refused before a process is spawned, and a list
//! this module cannot read back exactly is never rewritten.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use better_core::package_health::InstalledFiles;

use crate::user_units::{SYSTEMCTL_GUARD_VARIABLE, run_with_timeout};

/// Where a package puts a GNOME Shell extension: one directory per UUID.
pub const EXTENSION_DIRECTORY: &str = "/usr/share/gnome-shell/extensions";

pub const SHELL_SCHEMA: &str = "org.gnome.shell";
pub const ENABLED_KEY: &str = "enabled-extensions";
pub const DISABLED_KEY: &str = "disabled-extensions";
/// GNOME 46 loads no extension but its session mode's own while this is true
/// (`extensionSystem.js`, `_getEnabledExtensions`), whatever the lists say.
pub const ALL_OFF_KEY: &str = "disable-user-extensions";

/// How many times a list is read, changed, and written before giving up
/// because something else keeps changing it at the same moment.
pub const WRITE_ATTEMPTS: usize = 3;

/// How long one `gsettings` call may take.
pub const GSETTINGS_TIMEOUT: Duration = Duration::from_secs(10);

/// Why an extension could not be turned on or off, or read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionFailureKind {
    /// This session has no GNOME Shell settings at all: another desktop, or
    /// no GNOME installed. There is nothing to turn on, and nothing is wrong.
    NoShell,
    /// The person's settings could not be reached (no session bus).
    NoSession,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionFailure {
    pub kind: ExtensionFailureKind,
    pub message: String,
}

impl ExtensionFailure {
    pub fn new(kind: ExtensionFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn is_no_shell(&self) -> bool {
        self.kind == ExtensionFailureKind::NoShell
    }

    pub fn is_no_session(&self) -> bool {
        self.kind == ExtensionFailureKind::NoSession
    }
}

/// Turns extensions on and off in the person's GNOME Shell settings.
pub trait ShellExtensionController: Send + Sync {
    fn enable(&self, uuids: &[String]) -> Result<(), ExtensionFailure>;
    fn disable(&self, uuids: &[String]) -> Result<(), ExtensionFailure>;
    /// The UUIDs of `uuids` the settings have enabled.
    fn enabled(&self, uuids: &[String]) -> Result<Vec<String>, ExtensionFailure>;
    /// Whether GNOME's own "turn off all extensions" switch is on, in which
    /// case an enabled extension is still not loaded.
    fn all_turned_off(&self) -> Result<bool, ExtensionFailure>;
}

/// The extensions a package installed, as dpkg's own file list records them:
/// every direct child directory of [`EXTENSION_DIRECTORY`] that holds a
/// `metadata.json`, named by a plain UUID.
pub fn shell_extensions(files: &InstalledFiles) -> Vec<String> {
    let root = Path::new(EXTENSION_DIRECTORY);
    let mut uuids: Vec<String> = files
        .paths
        .iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("metadata.json"))
        .filter_map(|path| path.parent())
        .filter(|directory| directory.parent() == Some(root))
        .filter_map(|directory| directory.file_name().and_then(|name| name.to_str()))
        .filter(|uuid| is_plain_uuid(uuid))
        .map(str::to_string)
        .collect();
    uuids.sort();
    uuids.dedup();
    uuids
}

/// A UUID GNOME Shell would accept as a directory name, and nothing that
/// could be read as a path or an option: `name@domain` in letters, digits,
/// `.`, `_`, and `-`.
pub fn is_plain_uuid(uuid: &str) -> bool {
    uuid.len() <= 255
        && !uuid.starts_with(['-', '.', '@'])
        && uuid.matches('@').count() == 1
        && !uuid.ends_with('@')
        && uuid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

/// Where the two lists live. The real one is `gsettings`; tests use
/// [`MemoryShellSettings`].
pub trait ShellSettings: Send + Sync {
    fn get(&self, key: &str) -> Result<Vec<String>, ExtensionFailure>;
    fn set(&self, key: &str, value: &[String]) -> Result<(), ExtensionFailure>;
    fn get_bool(&self, key: &str) -> Result<bool, ExtensionFailure>;
}

/// The controller over any [`ShellSettings`].
pub struct ShellExtensions<S> {
    settings: S,
}

impl<S: ShellSettings> ShellExtensions<S> {
    pub fn new(settings: S) -> Self {
        Self { settings }
    }

    pub fn settings(&self) -> &S {
        &self.settings
    }

    /// Adds (`present`) or takes out every UUID of `uuids` in the list at
    /// `key`, leaving every other entry where it was.
    ///
    /// The list is shared with GNOME's own Extensions app and anything else
    /// the person runs, and `gsettings` has no compare-and-set. So the list is
    /// read again just before the write and the attempt starts over when it
    /// moved; and after the write only our own UUIDs are checked, because
    /// `gsettings set` exits 0 even when dconf refused the change, while an
    /// entry someone else added a moment later is theirs to keep.
    fn update(&self, key: &str, uuids: &[String], present: bool) -> Result<(), ExtensionFailure> {
        let ours_right = |list: &[String]| uuids.iter().all(|uuid| list.contains(uuid) == present);
        for _ in 0..WRITE_ATTEMPTS {
            let before = self.settings.get(key)?;
            if ours_right(&before) {
                return Ok(());
            }
            let mut after: Vec<String> = before
                .iter()
                .filter(|entry| !uuids.contains(entry))
                .cloned()
                .collect();
            if present {
                // Kept in the order they were, then ours at the end.
                after = before.clone();
                after.extend(uuids.iter().filter(|uuid| !before.contains(uuid)).cloned());
            }
            if self.settings.get(key)? != before {
                continue;
            }
            self.settings.set(key, &after)?;
            if ours_right(&self.settings.get(key)?) {
                return Ok(());
            }
        }
        Err(ExtensionFailure::new(
            ExtensionFailureKind::Failed,
            format!("{SHELL_SCHEMA} {key} did not take the change"),
        ))
    }
}

fn refuse_unplain(uuids: &[String]) -> Result<(), ExtensionFailure> {
    match uuids.iter().find(|uuid| !is_plain_uuid(uuid)) {
        Some(uuid) => Err(ExtensionFailure::new(
            ExtensionFailureKind::Failed,
            format!("{uuid:?} is not an extension UUID"),
        )),
        None => Ok(()),
    }
}

impl<S: ShellSettings> ShellExtensionController for ShellExtensions<S> {
    fn enable(&self, uuids: &[String]) -> Result<(), ExtensionFailure> {
        refuse_unplain(uuids)?;
        if uuids.is_empty() {
            return Ok(());
        }
        self.update(DISABLED_KEY, uuids, false)?;
        self.update(ENABLED_KEY, uuids, true)
    }

    fn disable(&self, uuids: &[String]) -> Result<(), ExtensionFailure> {
        refuse_unplain(uuids)?;
        if uuids.is_empty() {
            return Ok(());
        }
        self.update(ENABLED_KEY, uuids, false)
    }

    fn enabled(&self, uuids: &[String]) -> Result<Vec<String>, ExtensionFailure> {
        refuse_unplain(uuids)?;
        if uuids.is_empty() {
            return Ok(Vec::new());
        }
        let enabled = self.settings.get(ENABLED_KEY)?;
        let disabled = self.settings.get(DISABLED_KEY)?;
        Ok(uuids
            .iter()
            .filter(|uuid| enabled.contains(uuid) && !disabled.contains(uuid))
            .cloned()
            .collect())
    }

    fn all_turned_off(&self) -> Result<bool, ExtensionFailure> {
        self.settings.get_bool(ALL_OFF_KEY)
    }
}

/// The product controller: `gsettings` over the dconf backend.
pub type GsettingsShellExtensions = ShellExtensions<GsettingsShellSettings>;

impl GsettingsShellExtensions {
    /// Refuses to run anything when [`SYSTEMCTL_GUARD_VARIABLE`] is set, the
    /// same switch that keeps tests away from the person's user manager.
    pub fn from_environment() -> Self {
        ShellExtensions::new(GsettingsShellSettings::from_environment())
    }
}

/// Reads and writes the two lists with `gsettings`.
#[derive(Clone, Debug)]
pub struct GsettingsShellSettings {
    timeout: Duration,
    guard: Option<PathBuf>,
}

impl GsettingsShellSettings {
    pub fn from_environment() -> Self {
        Self {
            timeout: GSETTINGS_TIMEOUT,
            guard: std::env::var_os(SYSTEMCTL_GUARD_VARIABLE)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        }
    }

    fn run(&self, arguments: &[&str]) -> Result<String, ExtensionFailure> {
        if let Some(log) = &self.guard {
            let line = std::iter::once("gsettings")
                .chain(arguments.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .and_then(|mut file| writeln!(file, "{line}"));
            return Err(ExtensionFailure::new(
                ExtensionFailureKind::NoSession,
                format!("gsettings is not run while {SYSTEMCTL_GUARD_VARIABLE} is set"),
            ));
        }
        let mut command = Command::new("gsettings");
        command
            .args(arguments)
            .env("LC_ALL", "C")
            // Never the memory backend, which would accept a write and keep it
            // nowhere.
            .env("GSETTINGS_BACKEND", "dconf");
        let output = match run_with_timeout(command, self.timeout) {
            Ok(Some(output)) => output,
            Ok(None) => {
                return Err(ExtensionFailure::new(
                    ExtensionFailureKind::Failed,
                    format!(
                        "gsettings did not answer within {} seconds",
                        self.timeout.as_secs_f32()
                    ),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ExtensionFailure::new(
                    ExtensionFailureKind::NoShell,
                    "gsettings is not installed",
                ));
            }
            Err(error) => {
                return Err(ExtensionFailure::new(
                    ExtensionFailureKind::Failed,
                    format!("gsettings could not be run: {error}"),
                ));
            }
        };
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            let message = stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("gsettings exited with {}", output.status));
            let kind = if message.contains("No such schema") || message.contains("No such key") {
                ExtensionFailureKind::NoShell
            } else if is_no_session_message(&stderr) {
                ExtensionFailureKind::NoSession
            } else {
                ExtensionFailureKind::Failed
            };
            return Err(ExtensionFailure::new(kind, message));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

fn is_no_session_message(message: &str) -> bool {
    [
        "D-Bus",
        "DBUS_SESSION_BUS_ADDRESS",
        "$DISPLAY",
        "autolaunch",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

impl ShellSettings for GsettingsShellSettings {
    fn get(&self, key: &str) -> Result<Vec<String>, ExtensionFailure> {
        let output = self.run(&["get", SHELL_SCHEMA, key])?;
        parse_string_list(&output).ok_or_else(|| {
            ExtensionFailure::new(
                ExtensionFailureKind::Failed,
                format!("{SHELL_SCHEMA} {key} could not be read: {}", output.trim()),
            )
        })
    }

    fn set(&self, key: &str, value: &[String]) -> Result<(), ExtensionFailure> {
        let text = format_string_list(value);
        self.run(&["set", SHELL_SCHEMA, key, &text]).map(|_| ())
    }

    fn get_bool(&self, key: &str) -> Result<bool, ExtensionFailure> {
        match self.run(&["get", SHELL_SCHEMA, key])?.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(ExtensionFailure::new(
                ExtensionFailureKind::Failed,
                format!("{SHELL_SCHEMA} {key} could not be read: {other}"),
            )),
        }
    }
}

/// Parses `gsettings get`'s printing of an `as` value: `@as []` or
/// `['a', "b'c"]`. Anything else is `None`, and a list that cannot be read is
/// never written back.
pub fn parse_string_list(text: &str) -> Option<Vec<String>> {
    let text = text.trim();
    let text = text
        .strip_prefix("@as")
        .map(str::trim_start)
        .unwrap_or(text);
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    let mut items = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(quote) = chars.next() else {
            break;
        };
        if quote != '\'' && quote != '"' {
            return None;
        }
        let mut item = String::new();
        loop {
            match chars.next()? {
                '\\' => match chars.next()? {
                    // The only escapes gsettings prints in an extension list.
                    // Anything else (`\n`, `\u…`) is a list this parser does
                    // not fully understand, and such a list is never written
                    // back.
                    c @ ('\\' | '\'' | '"') => item.push(c),
                    _ => return None,
                },
                c if c == quote => break,
                c => item.push(c),
            }
        }
        items.push(item);
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        match chars.next() {
            None => break,
            Some(',') => {}
            Some(_) => return None,
        }
    }
    Some(items)
}

/// The GVariant text of an `as` value, every item single-quoted with `\` and
/// `'` escaped.
pub fn format_string_list(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".to_string();
    }
    let quoted: Vec<String> = items
        .iter()
        .map(|item| {
            let escaped = item.replace('\\', "\\\\").replace('\'', "\\'");
            format!("'{escaped}'")
        })
        .collect();
    format!("[{}]", quoted.join(", "))
}

/// The two lists in memory, for tests.
#[derive(Debug, Default)]
pub struct MemoryShellSettings {
    state: Mutex<MemoryState>,
}

#[derive(Debug, Default)]
struct MemoryState {
    enabled: Vec<String>,
    disabled: Vec<String>,
    failure: Option<ExtensionFailure>,
    ignore_writes: bool,
    writes: usize,
    all_off: bool,
    /// Entries "another program" adds to the enabled list, one per read,
    /// between our read and our write.
    interference: Vec<String>,
    reads: usize,
}

impl MemoryShellSettings {
    pub fn new(enabled: &[&str], disabled: &[&str]) -> Self {
        let settings = Self::default();
        {
            let mut state = settings.lock();
            state.enabled = enabled.iter().map(|s| s.to_string()).collect();
            state.disabled = disabled.iter().map(|s| s.to_string()).collect();
        }
        settings
    }

    /// Every call fails with this.
    pub fn failing(self, failure: ExtensionFailure) -> Self {
        self.lock().failure = Some(failure);
        self
    }

    /// Writes are accepted and kept nowhere, which is what dconf does when it
    /// refuses a change behind a successful `gsettings set`.
    pub fn ignoring_writes(self) -> Self {
        self.lock().ignore_writes = true;
        self
    }

    /// GNOME's "turn off all extensions" switch is on.
    pub fn all_off(self) -> Self {
        self.lock().all_off = true;
        self
    }

    /// Each of these is added to the enabled list by "someone else" right
    /// after one of our reads of it, as a concurrent write would be.
    pub fn interfering(self, entries: &[&str]) -> Self {
        self.lock().interference = entries.iter().rev().map(|s| s.to_string()).collect();
        self
    }

    pub fn enabled_list(&self) -> Vec<String> {
        self.lock().enabled.clone()
    }

    pub fn disabled_list(&self) -> Vec<String> {
        self.lock().disabled.clone()
    }

    pub fn writes(&self) -> usize {
        self.lock().writes
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        self.state.lock().expect("the fake is not poisoned")
    }
}

impl ShellSettings for MemoryShellSettings {
    fn get(&self, key: &str) -> Result<Vec<String>, ExtensionFailure> {
        let state = self.lock();
        if let Some(failure) = &state.failure {
            return Err(failure.clone());
        }
        let mut state = state;
        let list = match key {
            ENABLED_KEY => state.enabled.clone(),
            DISABLED_KEY => state.disabled.clone(),
            _ => Vec::new(),
        };
        if key == ENABLED_KEY {
            state.reads += 1;
            if let Some(entry) = state.interference.pop() {
                state.enabled.push(entry);
            }
        }
        Ok(list)
    }

    fn get_bool(&self, key: &str) -> Result<bool, ExtensionFailure> {
        let state = self.lock();
        if let Some(failure) = &state.failure {
            return Err(failure.clone());
        }
        Ok(key == ALL_OFF_KEY && state.all_off)
    }

    fn set(&self, key: &str, value: &[String]) -> Result<(), ExtensionFailure> {
        let mut state = self.lock();
        if let Some(failure) = &state.failure {
            return Err(failure.clone());
        }
        state.writes += 1;
        if state.ignore_writes {
            return Ok(());
        }
        match key {
            ENABLED_KEY => state.enabled = value.to_vec(),
            DISABLED_KEY => state.disabled = value.to_vec(),
            _ => {}
        }
        Ok(())
    }
}

/// A controller over in-memory settings, for tests.
pub type FakeShellExtensions = ShellExtensions<MemoryShellSettings>;

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &str = "touchpad-adapter@betteros.org";

    fn ours() -> Vec<String> {
        vec![OURS.to_string()]
    }

    fn files(paths: &[&str]) -> InstalledFiles {
        InstalledFiles {
            paths: paths.iter().map(PathBuf::from).collect(),
            ..InstalledFiles::default()
        }
    }

    #[test]
    fn extensions_come_from_the_dpkg_file_list() {
        let list = files(&[
            "/usr/share/gnome-shell/extensions",
            "/usr/share/gnome-shell/extensions/touchpad-adapter@betteros.org",
            "/usr/share/gnome-shell/extensions/touchpad-adapter@betteros.org/metadata.json",
            "/usr/share/gnome-shell/extensions/touchpad-adapter@betteros.org/extension.js",
            // Not a direct child of the extension directory.
            "/usr/share/gnome-shell/extensions/a@b/schemas/metadata.json",
            // A per-user location is never the package's.
            "/home/x/.local/share/gnome-shell/extensions/c@d/metadata.json",
            "/usr/bin/better-touchpad",
        ]);
        assert_eq!(shell_extensions(&list), ours());
    }

    #[test]
    fn only_plain_uuids_are_accepted() {
        assert!(is_plain_uuid(OURS));
        for bad in ["", "no-at", "a@b@c", "-a@b", "a b@c", "../x@y", "a@", "@a"] {
            assert!(!is_plain_uuid(bad), "{bad:?}");
        }
    }

    #[test]
    fn enabling_appends_and_keeps_every_other_entry() {
        let controller =
            ShellExtensions::new(MemoryShellSettings::new(&["x11gestures@a", "dock@b"], &[]));
        controller.enable(&ours()).unwrap();
        assert_eq!(
            controller.settings().enabled_list(),
            ["x11gestures@a", "dock@b", OURS]
        );
        // Already on: nothing written.
        let writes = controller.settings().writes();
        controller.enable(&ours()).unwrap();
        assert_eq!(controller.settings().writes(), writes);
    }

    #[test]
    fn enabling_takes_it_out_of_the_disabled_list() {
        let controller = ShellExtensions::new(MemoryShellSettings::new(&[], &["x@y", OURS]));
        controller.enable(&ours()).unwrap();
        assert_eq!(controller.settings().disabled_list(), ["x@y"]);
        assert_eq!(controller.enabled(&ours()).unwrap(), ours());
    }

    #[test]
    fn disabling_removes_only_ours() {
        let controller = ShellExtensions::new(MemoryShellSettings::new(&["a@b", OURS, "c@d"], &[]));
        controller.disable(&ours()).unwrap();
        assert_eq!(controller.settings().enabled_list(), ["a@b", "c@d"]);
        assert!(controller.enabled(&ours()).unwrap().is_empty());
    }

    #[test]
    fn a_list_someone_else_changed_meanwhile_is_read_again_and_their_entry_kept() {
        let controller =
            ShellExtensions::new(MemoryShellSettings::new(&["a@b"], &[]).interfering(&["late@x"]));
        controller.enable(&ours()).unwrap();
        let list = controller.settings().enabled_list();
        assert!(list.contains(&"late@x".to_string()), "{list:?}");
        assert!(list.contains(&OURS.to_string()), "{list:?}");
        assert!(list.contains(&"a@b".to_string()), "{list:?}");
    }

    #[test]
    fn a_list_that_never_stops_moving_fails_after_the_attempts() {
        let controller = ShellExtensions::new(
            MemoryShellSettings::new(&[], &[])
                .interfering(&["1@x", "2@x", "3@x", "4@x", "5@x", "6@x"]),
        );
        assert_eq!(
            controller.enable(&ours()).unwrap_err().kind,
            ExtensionFailureKind::Failed
        );
        assert!(
            !controller
                .settings()
                .enabled_list()
                .contains(&OURS.to_string())
        );
    }

    #[test]
    fn the_all_off_switch_is_reported() {
        let controller = ShellExtensions::new(MemoryShellSettings::new(&[OURS], &[]).all_off());
        assert!(controller.all_turned_off().unwrap());
        let controller = ShellExtensions::new(MemoryShellSettings::new(&[OURS], &[]));
        assert!(!controller.all_turned_off().unwrap());
    }

    #[test]
    fn a_write_dconf_dropped_is_a_failure() {
        let controller = ShellExtensions::new(MemoryShellSettings::new(&[], &[]).ignoring_writes());
        let failure = controller.enable(&ours()).unwrap_err();
        assert_eq!(failure.kind, ExtensionFailureKind::Failed);
    }

    #[test]
    fn an_unplain_uuid_is_refused_before_anything_is_read() {
        let controller = ShellExtensions::new(MemoryShellSettings::new(&[], &[]));
        assert!(controller.enable(&["../evil".to_string()]).is_err());
        assert_eq!(controller.settings().writes(), 0);
    }

    #[test]
    fn the_gvariant_list_round_trips() {
        assert_eq!(parse_string_list("@as []\n"), Some(vec![]));
        assert_eq!(parse_string_list("[]"), Some(vec![]));
        assert_eq!(
            parse_string_list("['a@b', \"it's@x\", 'q\\'z@y']\n"),
            Some(vec!["a@b".into(), "it's@x".into(), "q'z@y".into()])
        );
        for bad in [
            "",
            "['a'",
            "['a' 'b']",
            "[a]",
            "['a',]x",
            "['a\\nb']",
            "['\\u0041@b']",
        ] {
            assert_eq!(parse_string_list(bad), None, "{bad:?}");
        }
        let items = vec!["a@b".to_string(), "it's\\x".to_string()];
        assert_eq!(parse_string_list(&format_string_list(&items)), Some(items));
        assert_eq!(format_string_list(&[]), "@as []");
    }

    #[test]
    fn the_guard_runs_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("log");
        let settings = GsettingsShellSettings {
            timeout: GSETTINGS_TIMEOUT,
            guard: Some(log.clone()),
        };
        let failure = settings.get(ENABLED_KEY).unwrap_err();
        assert!(failure.is_no_session());
        assert!(
            std::fs::read_to_string(log)
                .unwrap()
                .contains("gsettings get org.gnome.shell")
        );
    }
}
