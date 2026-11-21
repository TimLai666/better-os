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
        assert_eq!(
            import.restore_set.get("DISPLAY").map(String::as_str),
            Some(":1")
        );
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
}
