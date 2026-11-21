//! The session's environment: what the launcher sets for itself and its
//! children, what it imports into the systemd user manager and D-Bus
//! activation, and how it puts the user manager back when the session ends
//! (engineering/specs/better-os-session.md, "The environment the launcher
//! sets").
//!
//! Nothing the launcher imports outlives the session: every variable it
//! changes in the user manager is set back to the value it had before the
//! session (or unset when it had none), as sway's systemd integration does.
//! Putting the previous value back rather than unsetting matters for
//! `XDG_DATA_DIRS`, which the user manager already holds from its environment
//! generators (Flatpak's and snapd's), so a GNOME login after a Better OS one
//! finds exactly what it would have found.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where the session's defaults for COSMIC's configuration are, as package
/// data. `cosmic-config` takes a configuration's default directory
/// (`cosmic/<id>/v<N>/`) from the first `XDG_DATA_DIRS` entry that has one, so
/// a directory here replaces COSMIC's own for that configuration and leaves
/// every other configuration's defaults in `/usr/share/cosmic/`.
pub const SESSION_DATA_DIR: &str = "/usr/share/better-os/session";

/// `XDG_CURRENT_DESKTOP` in the session: Better OS's name first, so portal
/// configuration (`betteros-portals.conf`) and Better OS's own session
/// matching (`better_core::desktop`) pick the session's rules, and `COSMIC`
/// for COSMIC's portal (`UseIn=COSMIC`) and daemons.
pub const CURRENT_DESKTOP: &str = "BetterOS:COSMIC";

/// The dconf profile `/usr/share/dconf/profile/betteros`: the session's own
/// database first, GNOME's read beneath it.
pub const DCONF_PROFILE: &str = "betteros";

/// `XDG_DATA_DIRS` when nothing set it, as the XDG base directory
/// specification has it.
pub const DEFAULT_DATA_DIRS: &str = "/usr/local/share/:/usr/share/";

/// The variables imported into the user manager and D-Bus activation, in this
/// order. The two the compositor reports come first.
pub const IMPORTED: [&str; 9] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DESKTOP_SESSION",
    "DCONF_PROFILE",
    "XDG_DATA_DIRS",
    "QT_IM_MODULE",
    "XMODIFIERS",
];

/// Variables the session must not have, taken out of the launcher's own
/// environment and, for the length of the session, out of the user
/// manager's. `GTK_IM_MODULE=fcitx` takes Chinese input away from every snap
/// GTK application (AppArmor denies the module's D-Bus call to Fcitx 5);
/// unset, Wayland GTK clients reach Fcitx 5 through text-input-v3 (ticket
/// 201).
pub const WITHHELD: [&str; 1] = ["GTK_IM_MODULE"];

/// What the launcher does to its own environment before it starts anything.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Launch {
    /// Variables set, in order.
    pub set: Vec<(String, String)>,
    /// Variables removed.
    pub remove: Vec<String>,
    /// Things worth a journal line; none stops the session.
    pub warnings: Vec<String>,
}

/// The session's environment over the one GDM started the launcher with.
pub fn launch_environment(inherited: impl Fn(&str) -> Option<String>) -> Launch {
    let mut launch = Launch::default();
    let mut set = |name: &str, value: String| launch.set.push((name.to_string(), value));
    set("XDG_CURRENT_DESKTOP", CURRENT_DESKTOP.to_string());
    set(
        "XDG_SESSION_DESKTOP",
        better_core::desktop::BETTER_OS_SESSION_NAME.to_string(),
    );
    set(
        "DESKTOP_SESSION",
        better_core::desktop::BETTER_OS_SESSION_NAME.to_string(),
    );
    set("DCONF_PROFILE", DCONF_PROFILE.to_string());
    set(
        "XDG_DATA_DIRS",
        session_data_dirs(inherited("XDG_DATA_DIRS").as_deref()),
    );
    // Fcitx 5 for Qt and X11 clients (C4). GTK reaches it without a module.
    set("QT_IM_MODULE", "fcitx".to_string());
    set("XMODIFIERS", "@im=fcitx".to_string());
    launch.remove = WITHHELD.iter().map(|name| name.to_string()).collect();
    // GDM sets the session type; the launcher only checks it.
    match inherited("XDG_SESSION_TYPE").as_deref() {
        Some("wayland") => {}
        Some(other) => launch.warnings.push(format!(
            "XDG_SESSION_TYPE is {other}, not wayland; the session starts anyway"
        )),
        None => launch
            .warnings
            .push("XDG_SESSION_TYPE is not set; GDM normally sets it to wayland".to_string()),
    }
    launch
}

/// `XDG_DATA_DIRS` with [`SESSION_DATA_DIR`] first and the inherited entries
/// after it, in their order, without a second copy of the session's entry.
/// An unset or empty value stands for the specification's default.
pub fn session_data_dirs(inherited: Option<&str>) -> String {
    let inherited = match inherited {
        Some(value) if value.split(':').any(|entry| !entry.is_empty()) => value,
        _ => DEFAULT_DATA_DIRS,
    };
    let mut entries = vec![SESSION_DATA_DIR.to_string()];
    for entry in inherited.split(':') {
        if entry.is_empty() || entry.trim_end_matches('/') == SESSION_DATA_DIR {
            continue;
        }
        entries.push(entry.to_string());
    }
    entries.join(":")
}

/// What the user manager and D-Bus activation are told at the start of the
/// session, and what puts the user manager back at its end.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Import {
    /// Set at the start.
    pub set: BTreeMap<String, String>,
    /// Unset at the start: a withheld variable the user manager had, or an
    /// imported one the session has no value for (no XWayland, no `DISPLAY`).
    pub unset: Vec<String>,
    /// Set back at the end: each changed variable's value before the session.
    pub restore_set: BTreeMap<String, String>,
    /// Unset at the end: each variable the session set that had no value
    /// before it.
    pub restore_unset: Vec<String>,
    /// Set when the user manager's environment could not be read before the
    /// session: at the end `XDG_DATA_DIRS` is read again and the session's
    /// entry taken out of it, rather than set back to a value nobody knows.
    #[serde(default)]
    pub strip_data_dir: bool,
}

impl Import {
    /// `set` as the `NAME=value` assignments systemd takes.
    pub fn assignments(&self) -> Vec<String> {
        assignments(&self.set)
    }

    /// `restore_set` as assignments.
    pub fn restore_assignments(&self) -> Vec<String> {
        assignments(&self.restore_set)
    }

    /// The import with every name outside [`IMPORTED`] and [`WITHHELD`]
    /// dropped. A record is read from a file in the runtime directory, and
    /// whatever it holds besides the session's own names (`LD_PRELOAD`,
    /// `PATH`) must never reach the user manager.
    pub fn known_only(mut self) -> Import {
        self.set.retain(|name, _| is_known(name));
        self.unset.retain(|name| is_known(name));
        self.restore_set.retain(|name, _| is_known(name));
        self.restore_unset.retain(|name| is_known(name));
        self
    }
}

/// Whether the session ever imports or withholds `name`.
pub fn is_known(name: &str) -> bool {
    IMPORTED.contains(&name) || WITHHELD.contains(&name)
}

/// `NAME=value` for each entry.
pub fn assignments(values: &BTreeMap<String, String>) -> Vec<String> {
    values
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect()
}

/// The user manager's `Environment` property (`NAME=value` strings) as a map.
/// An entry without `=` is not an assignment and is left out.
pub fn parse_assignments(entries: &[String]) -> BTreeMap<String, String> {
    entries
        .iter()
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// The import for a session whose own environment is `session`, over a user
/// manager whose environment is `before`. A variable whose value would not
/// change is left alone and is not put back either.
pub fn plan_import(
    session: impl Fn(&str) -> Option<String>,
    before: &BTreeMap<String, String>,
) -> Import {
    let mut import = Import::default();
    let mut changed = Vec::new();
    for name in IMPORTED {
        // The compositor's displays name a display that ends with the
        // session, whatever was there before: cosmic-comp imports both into
        // the user manager itself as it comes up, before it tells the
        // launcher, so "before" already holds them. They are set and, at the
        // end, unset, as COSMIC's own cosmic-comp.service does
        // (ExecStopPost=systemctl --user unset-environment DISPLAY
        // WAYLAND_DISPLAY). Ticket 203's VM found them left behind otherwise.
        if crate::compositor::REPORTED.contains(&name) {
            if let Some(value) = session(name).filter(|value| !value.is_empty()) {
                import.set.insert(name.to_string(), value);
            } else if before.contains_key(name) {
                import.unset.push(name.to_string());
            }
            import.restore_unset.push(name.to_string());
            continue;
        }
        match session(name).filter(|value| !value.is_empty()) {
            Some(value) => {
                if before.get(name) != Some(&value) {
                    import.set.insert(name.to_string(), value);
                    changed.push(name);
                }
            }
            None => {
                if before.contains_key(name) {
                    import.unset.push(name.to_string());
                    changed.push(name);
                }
            }
        }
    }
    for name in WITHHELD {
        if before.contains_key(name) {
            import.unset.push(name.to_string());
            changed.push(name);
        }
    }
    for name in changed {
        match before.get(name) {
            Some(value) => {
                import.restore_set.insert(name.to_string(), value.clone());
            }
            None => import.restore_unset.push(name.to_string()),
        }
    }
    import
}

/// Whether `value` of `name` is one only the Better OS session sets. Such a
/// value in the user manager before the session starts was left by an
/// earlier session that ended without its launcher, never set by the person.
pub fn is_session_value(name: &str, value: &str) -> bool {
    match name {
        "DCONF_PROFILE" => crate::dconf::is_session_profile(value),
        "XDG_CURRENT_DESKTOP" => value == CURRENT_DESKTOP,
        "XDG_SESSION_DESKTOP" | "DESKTOP_SESSION" => {
            value == better_core::desktop::BETTER_OS_SESSION_NAME
        }
        // Anywhere in the list, not only first: a GNOME login after a
        // session that left its value puts its own entries in front of it
        // (ticket 203's second review, VM).
        "XDG_DATA_DIRS" => value
            .split(':')
            .any(|entry| entry.trim_end_matches('/') == SESSION_DATA_DIR),
        _ => false,
    }
}

/// `XDG_DATA_DIRS` without the session's entry, or `None` when nothing else
/// is left in it.
pub fn without_session_data_dir(value: &str) -> Option<String> {
    let rest: Vec<&str> = value
        .split(':')
        .filter(|entry| !entry.is_empty() && entry.trim_end_matches('/') != SESSION_DATA_DIR)
        .collect();
    (!rest.is_empty()).then(|| rest.join(":"))
}

/// The user manager's environment with an earlier session's leftovers taken
/// out, as if that session had put it back (ticket 203's review, P1). The
/// session-only values ([`is_session_value`]) go; `XDG_DATA_DIRS` loses the
/// session's entry and keeps the rest. `QT_IM_MODULE` and `XMODIFIERS` are
/// values a person may set for GNOME too, so they go only beside a leftover
/// `DCONF_PROFILE=betteros`, which nobody but the session sets.
pub fn without_leftovers(before: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let leaked = before
        .get("DCONF_PROFILE")
        .is_some_and(|value| is_session_value("DCONF_PROFILE", value));
    let mut clean = BTreeMap::new();
    for (name, value) in before {
        if name == "XDG_DATA_DIRS" && is_session_value(name, value) {
            if let Some(rest) = without_session_data_dir(value) {
                clean.insert(name.clone(), rest);
            }
            continue;
        }
        if is_session_value(name, value) {
            continue;
        }
        if leaked
            && ((name == "QT_IM_MODULE" && value == "fcitx")
                || (name == "XMODIFIERS" && value == "@im=fcitx"))
        {
            continue;
        }
        clean.insert(name.clone(), value.clone());
    }
    clean
}

/// The import when the user manager's environment could not be read: the
/// session's variables are set, and at the end only what provably belongs to
/// the session is taken back. The session-only values and the displays are
/// unset, `XDG_DATA_DIRS` loses the session's entry ([`Import::strip_data_dir`]),
/// and `QT_IM_MODULE` and `XMODIFIERS`, which may have been the person's,
/// stay as they are.
pub fn plan_import_blind(session: impl Fn(&str) -> Option<String>) -> Import {
    let mut import = Import::default();
    for name in IMPORTED {
        if let Some(value) = session(name).filter(|value| !value.is_empty()) {
            if is_session_value(name, &value) && name != "XDG_DATA_DIRS"
                || crate::compositor::REPORTED.contains(&name)
            {
                import.restore_unset.push(name.to_string());
            }
            import.set.insert(name.to_string(), value);
        }
    }
    import.strip_data_dir = true;
    import
}

/// What D-Bus activation's environment is set to at the end of the session.
/// `dbus-daemon` cannot unset a variable, so one the session added keeps its
/// value there for as long as the session bus lives (a quick login after,
/// linger, an ssh session), and a GNOME application D-Bus starts directly
/// would then read and write the session's dconf database. Each variable is
/// therefore set back to its value before the session, and where it had
/// none, to a value that behaves as unset does: `DCONF_PROFILE` to
/// `dconf_default` ([`crate::dconf::default_profile`]) and `XDG_DATA_DIRS` to
/// the specification's default. The other session-only names are left to the
/// next session, which sets its own.
pub fn activation_restore(import: &Import, dconf_default: &str) -> BTreeMap<String, String> {
    let mut values = import.restore_set.clone();
    for name in &import.restore_unset {
        match name.as_str() {
            "DCONF_PROFILE" => {
                values.insert(name.clone(), dconf_default.to_string());
            }
            "XDG_DATA_DIRS" => {
                values.insert(name.clone(), DEFAULT_DATA_DIRS.to_string());
            }
            _ => {}
        }
    }
    values
}

/// What putting back a record left by a session whose launcher is gone may
/// touch, given the user manager's environment `now`. Another session may
/// have started since (the leftovers unit runs inside a GNOME login, after
/// gnome-session imported its own values), so a variable is put back only
/// while its value is still the session's: one only the session sets
/// ([`is_session_value`]), or the session's input method value beside its
/// leftover `DCONF_PROFILE`. Every other value may be someone else's and
/// stays, a display equal to the session's included (ticket 203's second
/// review, P2). `XDG_DATA_DIRS` still holding the session's entry, wherever
/// in the list, loses only that entry. In D-Bus activation, which cannot be read, only
/// `DCONF_PROFILE` is set, to the record's value that behaves as unset:
/// nothing but the session sets it, and a GNOME login sets its own
/// `XDG_DATA_DIRS` there.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Leftovers {
    pub set: BTreeMap<String, String>,
    pub unset: Vec<String>,
    pub activation: BTreeMap<String, String>,
}

pub fn leftover_restore(
    import: &Import,
    activation: &BTreeMap<String, String>,
    now: &BTreeMap<String, String>,
) -> Leftovers {
    let mut leftovers = Leftovers::default();
    // A value nobody but the session sets is the session's. The input method
    // variables are the session's while they still hold its value and the
    // session's DCONF_PROFILE is still there beside them (the rule of
    // [`without_leftovers`]): GNOME's login does not set them, so they would
    // otherwise reach GNOME Shell. A display (`:0`) is never taken back:
    // the next session's compositor sets its own.
    let leaked = now
        .get("DCONF_PROFILE")
        .is_some_and(|value| is_session_value("DCONF_PROFILE", value));
    let still_ours = |name: &str| {
        now.get(name).is_some_and(|value| {
            is_session_value(name, value)
                || (leaked
                    && matches!(name, "QT_IM_MODULE" | "XMODIFIERS")
                    && import.set.get(name) == Some(value))
        })
    };
    let names = import
        .restore_set
        .keys()
        .chain(import.restore_unset.iter())
        .chain(import.set.keys());
    let mut seen = std::collections::BTreeSet::new();
    for name in names {
        if !seen.insert(name.as_str()) || !still_ours(name) {
            continue;
        }
        if name == "XDG_DATA_DIRS" {
            match now
                .get(name)
                .and_then(|value| without_session_data_dir(value))
            {
                Some(rest) => {
                    leftovers.set.insert(name.clone(), rest);
                }
                None => leftovers.unset.push(name.clone()),
            }
            continue;
        }
        match import.restore_set.get(name) {
            Some(before) if !is_session_value(name, before) => {
                leftovers.set.insert(name.clone(), before.clone());
            }
            _ => leftovers.unset.push(name.clone()),
        }
    }
    if let Some(profile) = activation.get("DCONF_PROFILE") {
        leftovers
            .activation
            .insert("DCONF_PROFILE".to_string(), profile.clone());
    }
    leftovers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn value<'a>(launch: &'a Launch, name: &str) -> Option<&'a str> {
        launch
            .set
            .iter()
            .find(|(set, _)| set == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn the_launcher_sets_the_spec_table() {
        let launch = launch_environment(lookup(&[
            ("XDG_SESSION_TYPE", "wayland"),
            (
                "XDG_DATA_DIRS",
                "/usr/local/share/:/usr/share/:/var/lib/snapd/desktop",
            ),
            ("GTK_IM_MODULE", "fcitx"),
        ]));
        assert_eq!(
            value(&launch, "XDG_CURRENT_DESKTOP"),
            Some("BetterOS:COSMIC")
        );
        assert_eq!(value(&launch, "XDG_SESSION_DESKTOP"), Some("better-os"));
        assert_eq!(value(&launch, "DESKTOP_SESSION"), Some("better-os"));
        assert_eq!(value(&launch, "DCONF_PROFILE"), Some("betteros"));
        assert_eq!(
            value(&launch, "XDG_DATA_DIRS"),
            Some(
                "/usr/share/better-os/session:/usr/local/share/:/usr/share/:/var/lib/snapd/desktop"
            )
        );
        assert_eq!(value(&launch, "QT_IM_MODULE"), Some("fcitx"));
        assert_eq!(value(&launch, "XMODIFIERS"), Some("@im=fcitx"));
        assert_eq!(
            value(&launch, "GTK_IM_MODULE"),
            None,
            "GTK_IM_MODULE is never set"
        );
        assert_eq!(launch.remove, vec!["GTK_IM_MODULE".to_string()]);
        assert!(launch.warnings.is_empty());
        // XDG_SESSION_TYPE is GDM's: checked, not set.
        assert_eq!(value(&launch, "XDG_SESSION_TYPE"), None);
    }

    #[test]
    fn the_desktop_names_are_the_ones_better_core_reads() {
        let desktop = better_core::desktop::CurrentDesktop::parse(CURRENT_DESKTOP);
        assert!(desktop.is_better_os_session());
        assert!(desktop.includes("COSMIC"));
        assert_eq!(desktop.names()[0], better_core::desktop::BETTER_OS_DESKTOP);
    }

    #[test]
    fn a_session_type_other_than_wayland_is_said_and_does_not_stop_the_session() {
        let launch = launch_environment(lookup(&[("XDG_SESSION_TYPE", "x11")]));
        assert_eq!(launch.warnings.len(), 1);
        assert!(launch.warnings[0].contains("x11"));
        let launch = launch_environment(lookup(&[]));
        assert_eq!(launch.warnings.len(), 1);
    }

    #[test]
    fn the_data_dirs_put_the_session_first_once() {
        assert_eq!(
            session_data_dirs(None),
            "/usr/share/better-os/session:/usr/local/share/:/usr/share/"
        );
        assert_eq!(
            session_data_dirs(Some("")),
            "/usr/share/better-os/session:/usr/local/share/:/usr/share/"
        );
        assert_eq!(
            session_data_dirs(Some("::")),
            "/usr/share/better-os/session:/usr/local/share/:/usr/share/"
        );
        assert_eq!(
            session_data_dirs(Some("/usr/share/better-os/session/:/usr/share::/opt/x")),
            "/usr/share/better-os/session:/usr/share:/opt/x"
        );
    }

    #[test]
    fn the_import_list_holds_what_the_spec_imports() {
        for name in [
            "WAYLAND_DISPLAY",
            "DISPLAY",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
            "DCONF_PROFILE",
        ] {
            assert!(
                IMPORTED.contains(&name),
                "{name} is in the spec's import list"
            );
        }
        // The units the target starts read their defaults through it.
        assert!(IMPORTED.contains(&"XDG_DATA_DIRS"));
        assert!(!IMPORTED.contains(&"GTK_IM_MODULE"));
        assert!(!IMPORTED.contains(&"XDG_SESSION_TYPE"));
    }

    #[test]
    fn every_variable_the_launcher_sets_is_imported() {
        let launch = launch_environment(lookup(&[("XDG_SESSION_TYPE", "wayland")]));
        for (name, _) in &launch.set {
            assert!(
                IMPORTED.contains(&name.as_str()),
                "{name} reaches the user manager"
            );
        }
    }

    #[test]
    fn the_import_sets_the_session_and_remembers_what_was_there() {
        let before = parse_assignments(&[
            "HOME=/home/tester".to_string(),
            "XDG_DATA_DIRS=/usr/local/share/:/usr/share/:/var/lib/snapd/desktop".to_string(),
            "XDG_CURRENT_DESKTOP=ubuntu:GNOME".to_string(),
            "GTK_IM_MODULE=ibus".to_string(),
            "DISPLAY=:1".to_string(),
        ]);
        let session = lookup(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("XDG_CURRENT_DESKTOP", "BetterOS:COSMIC"),
            ("XDG_SESSION_DESKTOP", "better-os"),
            ("DESKTOP_SESSION", "better-os"),
            ("DCONF_PROFILE", "betteros"),
            (
                "XDG_DATA_DIRS",
                "/usr/share/better-os/session:/usr/local/share/:/usr/share/:/var/lib/snapd/desktop",
            ),
            ("QT_IM_MODULE", "fcitx"),
            ("XMODIFIERS", "@im=fcitx"),
        ]);
        let import = plan_import(session, &before);
        assert_eq!(
            import.set.get("WAYLAND_DISPLAY").map(String::as_str),
            Some("wayland-1")
        );
        assert_eq!(
            import.set.get("DCONF_PROFILE").map(String::as_str),
            Some("betteros")
        );
        // No XWayland display this time: a stale DISPLAY is not left behind.
        assert!(import.unset.contains(&"DISPLAY".to_string()));
        assert!(import.unset.contains(&"GTK_IM_MODULE".to_string()));
        assert!(!import.set.contains_key("HOME"));
        // Put back: the previous values, and nothing where there was nothing.
        assert_eq!(
            import.restore_set.get("XDG_DATA_DIRS").map(String::as_str),
            Some("/usr/local/share/:/usr/share/:/var/lib/snapd/desktop")
        );
        assert_eq!(
            import
                .restore_set
                .get("XDG_CURRENT_DESKTOP")
                .map(String::as_str),
            Some("ubuntu:GNOME")
        );
        assert_eq!(
            import.restore_set.get("GTK_IM_MODULE").map(String::as_str),
            Some("ibus")
        );
        // A display of an earlier session is gone with it; nothing brings
        // it back.
        assert!(!import.restore_set.contains_key("DISPLAY"));
        assert!(import.restore_unset.contains(&"DISPLAY".to_string()));
        for name in [
            "WAYLAND_DISPLAY",
            "DCONF_PROFILE",
            "XDG_SESSION_DESKTOP",
            "QT_IM_MODULE",
        ] {
            assert!(
                import.restore_unset.contains(&name.to_string()),
                "{name} goes again"
            );
        }
        assert!(!import.restore_unset.contains(&"HOME".to_string()));
    }

    #[test]
    fn after_the_restore_the_user_manager_is_exactly_as_it_was() {
        let before = parse_assignments(&[
            "XDG_DATA_DIRS=/usr/share/".to_string(),
            "QT_IM_MODULE=fcitx".to_string(),
            "LANG=zh_TW.UTF-8".to_string(),
        ]);
        let session = lookup(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("DISPLAY", ":0"),
            ("XDG_DATA_DIRS", "/usr/share/better-os/session:/usr/share/"),
            ("QT_IM_MODULE", "fcitx"),
            ("DCONF_PROFILE", "betteros"),
        ]);
        let import = plan_import(session, &before);
        // Unchanged values are not touched at either end.
        assert!(!import.set.contains_key("QT_IM_MODULE"));
        assert!(!import.restore_set.contains_key("QT_IM_MODULE"));

        let mut manager = before.clone();
        for name in &import.unset {
            manager.remove(name);
        }
        manager.extend(import.set.clone());
        assert_eq!(
            manager.get("DCONF_PROFILE").map(String::as_str),
            Some("betteros")
        );
        manager.extend(import.restore_set.clone());
        for name in &import.restore_unset {
            manager.remove(name);
        }
        assert_eq!(manager, before);
    }

    #[test]
    fn the_compositors_displays_go_at_the_end_even_when_it_imported_them_first() {
        // What the user manager held when the launcher read it in ticket
        // 203's VM: cosmic-comp had already imported its own two.
        let before = parse_assignments(&[
            "WAYLAND_DISPLAY=wayland-1".to_string(),
            "DISPLAY=:0".to_string(),
            "XDG_DATA_DIRS=/usr/share/".to_string(),
        ]);
        let session = lookup(&[("WAYLAND_DISPLAY", "wayland-1"), ("DISPLAY", ":0")]);
        let import = plan_import(session, &before);
        assert_eq!(
            import.set.get("WAYLAND_DISPLAY").map(String::as_str),
            Some("wayland-1")
        );
        assert_eq!(import.set.get("DISPLAY").map(String::as_str), Some(":0"));
        for name in ["WAYLAND_DISPLAY", "DISPLAY"] {
            assert!(
                import.restore_unset.contains(&name.to_string()),
                "{name} is unset at the end"
            );
            assert!(!import.restore_set.contains_key(name));
        }
    }

    #[test]
    fn assignments_round_trip_and_skip_what_is_not_one() {
        let parsed = parse_assignments(&[
            "A=1".to_string(),
            "B=x=y".to_string(),
            "junk".to_string(),
            "=nothing".to_string(),
        ]);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.get("B").map(String::as_str), Some("x=y"));
        assert_eq!(
            assignments(&parsed),
            vec!["A=1".to_string(), "B=x=y".to_string()]
        );
    }

    /// The import of a session that started on a fresh user manager.
    fn fresh_session_import() -> Import {
        let before = parse_assignments(&[
            "XDG_DATA_DIRS=/usr/local/share/:/usr/share/:/var/lib/snapd/desktop".to_string(),
        ]);
        let session = lookup(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("DISPLAY", ":0"),
            ("XDG_CURRENT_DESKTOP", "BetterOS:COSMIC"),
            ("XDG_SESSION_DESKTOP", "better-os"),
            ("DESKTOP_SESSION", "better-os"),
            ("DCONF_PROFILE", "betteros"),
            (
                "XDG_DATA_DIRS",
                "/usr/share/better-os/session:/usr/local/share/:/usr/share/:/var/lib/snapd/desktop",
            ),
            ("QT_IM_MODULE", "fcitx"),
            ("XMODIFIERS", "@im=fcitx"),
        ]);
        plan_import(session, &before)
    }

    #[test]
    fn leftovers_put_back_under_a_starting_gnome_leave_gnomes_values_alone() {
        let import = fresh_session_import();
        let activation = activation_restore(&import, "/usr/share/dconf/profile/betteros-default");
        // gnome-session has imported its own values; the session's
        // DCONF_PROFILE and its fcitx variables are still there.
        let now = parse_assignments(&[
            "XDG_CURRENT_DESKTOP=ubuntu:GNOME".to_string(),
            "XDG_SESSION_DESKTOP=ubuntu".to_string(),
            "DESKTOP_SESSION=ubuntu".to_string(),
            "WAYLAND_DISPLAY=wayland-0".to_string(),
            "DISPLAY=:0".to_string(),
            // As the VM had it: GNOME's entries in front of the session's
            // leftover value.
            "XDG_DATA_DIRS=/usr/share/ubuntu:/usr/share/gnome:/usr/share/better-os/session:/usr/local/share/:/usr/share/:/var/lib/snapd/desktop"
                .to_string(),
            "QT_IM_MODULE=ibus".to_string(),
            "XMODIFIERS=@im=fcitx".to_string(),
            "DCONF_PROFILE=betteros".to_string(),
        ]);
        let left = leftover_restore(&import, &activation, &now);
        assert_eq!(
            left.unset,
            vec!["DCONF_PROFILE".to_string(), "XMODIFIERS".to_string()]
        );
        assert_eq!(
            left.set.get("XDG_DATA_DIRS").map(String::as_str),
            Some(
                "/usr/share/ubuntu:/usr/share/gnome:/usr/local/share/:/usr/share/:/var/lib/snapd/desktop"
            ),
            "only the session's entry leaves GNOME's list"
        );
        assert_eq!(left.set.len(), 1, "{:?}", left.set);
        // DISPLAY=:0 and XMODIFIERS=@im=fcitx equal the session's values
        // but may as well be GNOME's own: they are left.
        for gnome in [
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
            "WAYLAND_DISPLAY",
            "DISPLAY",
            "QT_IM_MODULE",
        ] {
            assert!(
                !left.unset.contains(&gnome.to_string()),
                "{gnome} is GNOME's"
            );
            assert!(!left.set.contains_key(gnome), "{gnome} is GNOME's");
        }
        assert_eq!(
            left.activation.get("DCONF_PROFILE").map(String::as_str),
            Some("/usr/share/dconf/profile/betteros-default")
        );
        assert!(!left.activation.contains_key("XDG_DATA_DIRS"));
    }

    #[test]
    fn leftovers_with_nobody_else_there_are_all_put_back() {
        let import = fresh_session_import();
        let now: BTreeMap<String, String> = import.set.clone();
        let left = leftover_restore(&import, &BTreeMap::new(), &now);
        assert_eq!(
            left.set.get("XDG_DATA_DIRS").map(String::as_str),
            Some("/usr/local/share/:/usr/share/:/var/lib/snapd/desktop")
        );
        for name in [
            "DCONF_PROFILE",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
        ] {
            assert!(left.unset.contains(&name.to_string()), "{name}");
        }
        // Beside the session's DCONF_PROFILE, its input method values go.
        assert!(left.unset.contains(&"QT_IM_MODULE".to_string()));
        assert!(left.unset.contains(&"XMODIFIERS".to_string()));
        // A display is the next session's to set.
        assert!(!left.unset.contains(&"WAYLAND_DISPLAY".to_string()));
    }

    #[test]
    fn input_method_values_without_the_sessions_profile_are_someone_elses() {
        let import = fresh_session_import();
        let now = parse_assignments(&[
            "QT_IM_MODULE=fcitx".to_string(),
            "XMODIFIERS=@im=fcitx".to_string(),
        ]);
        let left = leftover_restore(&import, &BTreeMap::new(), &now);
        assert!(left.unset.is_empty(), "{:?}", left.unset);
    }

    #[test]
    fn the_sessions_data_dir_anywhere_in_the_list_is_the_sessions() {
        assert!(is_session_value(
            "XDG_DATA_DIRS",
            "/usr/share/gnome:/usr/share/better-os/session/:/usr/share/"
        ));
        assert!(!is_session_value(
            "XDG_DATA_DIRS",
            "/usr/share/gnome:/usr/share/"
        ));
    }
}
