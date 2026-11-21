//! The order of the session's start and end, against the user manager and the
//! bus behind one seam ([`UserSession`]), so the order is tested with a fake.
//!
//! Start ([`begin`]): read the user manager's environment, unset what the
//! session must not have, set the session's variables, give the same values
//! to D-Bus activation, and only then start [`crate::TARGET`], so no unit of
//! the session starts without them.
//!
//! End: [`stop_units`] stops the target and waits until
//! `graphical-session.target` has stopped too (it stops when nothing needs it,
//! and every existing Better OS unit is `PartOf=` it); the launcher then ends
//! its children and the compositor; [`restore`] puts the user manager's
//! environment back last.
//!
//! D-Bus activation cannot be unset: `org.freedesktop.DBus` has no call for
//! it. A variable that had a value before the session is set back to it
//! there; `DCONF_PROFILE` and `XDG_DATA_DIRS`, which decide where a GNOME
//! application D-Bus starts directly reads and writes its settings, are set
//! to values that behave as unset ([`environment::activation_restore`]).

use std::collections::BTreeMap;
use std::time::Duration;

use crate::environment::{self, Import};

/// How long the end of a session waits for its units to stop.
pub const STOP_PATIENCE: Duration = Duration::from_secs(20);

/// How often it looks while it waits.
pub const STOP_POLL: Duration = Duration::from_millis(100);

/// The systemd user manager and the session bus, as the launcher uses them.
pub trait UserSession {
    /// The user manager's `Environment` property.
    fn environment(&self) -> Result<Vec<String>, String>;
    fn set_environment(&self, assignments: &[String]) -> Result<(), String>;
    fn unset_environment(&self, names: &[String]) -> Result<(), String>;
    /// `org.freedesktop.DBus.UpdateActivationEnvironment`.
    fn update_activation_environment(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<(), String>;
    fn start_unit(&self, unit: &str) -> Result<(), String>;
    fn stop_unit(&self, unit: &str) -> Result<(), String>;
    /// Whether the unit is active or on its way (`active`, `activating`,
    /// `deactivating`, `reloading`); a unit systemd does not know is not.
    fn is_running(&self, unit: &str) -> Result<bool, String>;
}

/// Imports the session's environment, has `record` write down how to put it
/// back ([`crate::record`]), and only then starts the target. Problems are
/// returned for the journal; the session goes on with what worked, because a
/// person at GDM is better served by a session that started than by none.
///
/// What an earlier session left behind is not taken for the person's own
/// ([`environment::without_leftovers`]), and when the user manager's
/// environment cannot be read at all, only what provably belongs to the
/// session is taken back at the end ([`environment::plan_import_blind`]).
pub fn begin(
    session: &dyn UserSession,
    environment: &BTreeMap<String, String>,
    record: &mut dyn FnMut(&Import) -> Result<(), String>,
) -> (Import, Vec<String>) {
    let mut problems = Vec::new();
    let lookup = |name: &str| environment.get(name).cloned();
    let import = match session.environment() {
        Ok(entries) => {
            let before = environment::without_leftovers(&environment::parse_assignments(&entries));
            environment::plan_import(lookup, &before)
        }
        Err(error) => {
            problems.push(format!(
                "the user manager's environment could not be read ({error}); only what is the \
                 session's own is taken back at the end"
            ));
            environment::plan_import_blind(lookup)
        }
    };
    if !import.unset.is_empty()
        && let Err(error) = session.unset_environment(&import.unset)
    {
        problems.push(format!(
            "unsetting {:?} in the user manager: {error}",
            import.unset
        ));
    }
    if !import.set.is_empty()
        && let Err(error) = session.set_environment(&import.assignments())
    {
        problems.push(format!("importing the session's environment: {error}"));
    }
    if !import.set.is_empty()
        && let Err(error) = session.update_activation_environment(&import.set)
    {
        problems.push(format!("updating D-Bus activation's environment: {error}"));
    }
    // The units require the record (ConditionPathExists=), so without it the
    // target would start nothing; it is said and the session goes on.
    if let Err(error) = record(&import) {
        problems.push(format!(
            "the session's record could not be written: {error}"
        ));
    }
    if let Err(error) = session.start_unit(crate::TARGET) {
        problems.push(format!("starting {}: {error}", crate::TARGET));
    }
    (import, problems)
}

/// Stops the target and waits, up to [`STOP_PATIENCE`], for it and
/// `graphical-session.target` to stop. `sleep` is the clock, so a test waits
/// for nothing.
pub fn stop_units(session: &dyn UserSession, sleep: &dyn Fn(Duration)) -> Vec<String> {
    let mut problems = Vec::new();
    if let Err(error) = session.stop_unit(crate::TARGET) {
        problems.push(format!("stopping {}: {error}", crate::TARGET));
    }
    let mut waited = Duration::ZERO;
    loop {
        let running = [crate::TARGET, crate::GRAPHICAL_SESSION_TARGET]
            .into_iter()
            .filter(|unit| session.is_running(unit).unwrap_or(false))
            .collect::<Vec<_>>();
        if running.is_empty() {
            break;
        }
        if waited >= STOP_PATIENCE {
            problems.push(format!(
                "{} still running after {} s; the session ends anyway",
                running.join(" and "),
                STOP_PATIENCE.as_secs()
            ));
            break;
        }
        sleep(STOP_POLL);
        waited += STOP_POLL;
    }
    problems
}

/// Puts the user manager's environment back as it was before the session,
/// and D-Bus activation's to `activation`
/// ([`environment::activation_restore`]): its previous values, and for what
/// it had none of, values that behave as unset.
pub fn restore(
    session: &dyn UserSession,
    import: &Import,
    activation: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    if !import.restore_set.is_empty()
        && let Err(error) = session.set_environment(&import.restore_assignments())
    {
        problems.push(format!(
            "setting the user manager's environment back: {error}"
        ));
    }
    if !activation.is_empty()
        && let Err(error) = session.update_activation_environment(activation)
    {
        problems.push(format!(
            "setting D-Bus activation's environment back: {error}"
        ));
    }
    if !import.restore_unset.is_empty()
        && let Err(error) = session.unset_environment(&import.restore_unset)
    {
        problems.push(format!("unsetting the session's variables: {error}"));
    }
    if import.strip_data_dir {
        match session.environment() {
            Ok(entries) => {
                let now = environment::parse_assignments(&entries);
                if let Some(value) = now.get("XDG_DATA_DIRS")
                    && environment::is_session_value("XDG_DATA_DIRS", value)
                {
                    let result = match environment::without_session_data_dir(value) {
                        Some(rest) => session.set_environment(&[format!("XDG_DATA_DIRS={rest}")]),
                        None => session.unset_environment(&["XDG_DATA_DIRS".to_string()]),
                    };
                    if let Err(error) = result {
                        problems.push(format!("taking the session out of XDG_DATA_DIRS: {error}"));
                    }
                }
            }
            Err(error) => problems.push(format!(
                "XDG_DATA_DIRS could not be read to take the session out of it: {error}"
            )),
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        manager: RefCell<BTreeMap<String, String>>,
        activation: RefCell<BTreeMap<String, String>>,
        calls: RefCell<Vec<String>>,
        running: RefCell<Vec<String>>,
        /// How many polls the units take to stop.
        stops_after: Cell<u32>,
        failing_start: bool,
        failing_read: bool,
    }

    impl Fake {
        fn with(manager: &[(&str, &str)]) -> Fake {
            let fake = Fake::default();
            *fake.manager.borrow_mut() = manager
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
            fake
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    /// Writes the record by noting it among the calls.
    fn recorder(fake: &Fake) -> impl FnMut(&Import) -> Result<(), String> + '_ {
        move |_| {
            fake.calls.borrow_mut().push("record".into());
            Ok(())
        }
    }

    impl UserSession for Fake {
        fn environment(&self) -> Result<Vec<String>, String> {
            self.calls.borrow_mut().push("read".into());
            if self.failing_read {
                return Err("no reply".into());
            }
            Ok(environment::assignments(&self.manager.borrow()))
        }
        fn set_environment(&self, assignments: &[String]) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("set {}", assignments.join(" ")));
            let parsed = environment::parse_assignments(assignments);
            self.manager.borrow_mut().extend(parsed);
            Ok(())
        }
        fn unset_environment(&self, names: &[String]) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("unset {}", names.join(" ")));
            for name in names {
                self.manager.borrow_mut().remove(name);
            }
            Ok(())
        }
        fn update_activation_environment(
            &self,
            values: &BTreeMap<String, String>,
        ) -> Result<(), String> {
            self.calls.borrow_mut().push("activation".into());
            self.activation.borrow_mut().extend(values.clone());
            Ok(())
        }
        fn start_unit(&self, unit: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("start {unit}"));
            if self.failing_start {
                return Err("refused".into());
            }
            *self.running.borrow_mut() = vec![
                unit.to_string(),
                crate::GRAPHICAL_SESSION_TARGET.to_string(),
            ];
            Ok(())
        }
        fn stop_unit(&self, unit: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("stop {unit}"));
            Ok(())
        }
        fn is_running(&self, unit: &str) -> Result<bool, String> {
            if self.stops_after.get() == 0 {
                self.running.borrow_mut().clear();
            }
            Ok(self.running.borrow().iter().any(|running| running == unit))
        }
    }

    fn session_environment() -> BTreeMap<String, String> {
        [
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("DISPLAY", ":0"),
            ("XDG_CURRENT_DESKTOP", "BetterOS:COSMIC"),
            ("XDG_SESSION_DESKTOP", "better-os"),
            ("DESKTOP_SESSION", "better-os"),
            ("DCONF_PROFILE", "betteros"),
            ("XDG_DATA_DIRS", "/usr/share/better-os/session:/usr/share/"),
            ("QT_IM_MODULE", "fcitx"),
            ("XMODIFIERS", "@im=fcitx"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
    }

    #[test]
    fn the_environment_is_in_place_before_the_target_starts() {
        let fake = Fake::with(&[("GTK_IM_MODULE", "ibus"), ("XDG_DATA_DIRS", "/usr/share/")]);
        let (import, problems) = begin(&fake, &session_environment(), &mut recorder(&fake));
        assert!(problems.is_empty(), "{problems:?}");
        let calls = fake.calls();
        let position = |prefix: &str| {
            calls
                .iter()
                .position(|call| call.starts_with(prefix))
                .unwrap()
        };
        assert!(position("read") < position("unset"));
        assert!(position("unset") < position("set "));
        assert!(position("set ") < position("activation"));
        assert!(position("activation") < position("record"));
        assert!(position("record") < position("start better-os-session.target"));
        assert_eq!(
            fake.manager
                .borrow()
                .get("DCONF_PROFILE")
                .map(String::as_str),
            Some("betteros")
        );
        assert!(!fake.manager.borrow().contains_key("GTK_IM_MODULE"));
        assert_eq!(
            fake.activation
                .borrow()
                .get("WAYLAND_DISPLAY")
                .map(String::as_str),
            Some("wayland-1")
        );
        assert_eq!(
            import.restore_set.get("GTK_IM_MODULE").map(String::as_str),
            Some("ibus")
        );
    }

    #[test]
    fn a_target_that_does_not_start_is_reported_and_the_import_stands() {
        let fake = Fake {
            failing_start: true,
            ..Fake::default()
        };
        let (import, problems) = begin(&fake, &session_environment(), &mut recorder(&fake));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("better-os-session.target"));
        assert!(!import.set.is_empty());
    }

    #[test]
    fn the_end_stops_the_target_waits_for_the_graphical_session_and_puts_everything_back() {
        let original = [
            ("XDG_DATA_DIRS", "/usr/local/share/:/usr/share/"),
            ("GTK_IM_MODULE", "ibus"),
            ("LANG", "zh_TW.UTF-8"),
        ];
        let fake = Fake::with(&original);
        let before = fake.manager.borrow().clone();
        let (import, _) = begin(&fake, &session_environment(), &mut recorder(&fake));
        fake.stops_after.set(3);
        let slept = Cell::new(0);
        let sleep = |_| {
            slept.set(slept.get() + 1);
            fake.stops_after
                .set(fake.stops_after.get().saturating_sub(1));
        };
        let problems = stop_units(&fake, &sleep);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(slept.get(), 3, "it waited until both targets stopped");
        assert!(
            fake.calls()
                .contains(&"stop better-os-session.target".to_string())
        );
        let problems = restore(
            &fake,
            &import,
            &environment::activation_restore(&import, "/default"),
        );
        assert!(problems.is_empty());
        assert_eq!(
            *fake.manager.borrow(),
            before,
            "the user manager is as it was"
        );
        // D-Bus activation gets back what had a value before.
        assert_eq!(
            fake.activation
                .borrow()
                .get("XDG_DATA_DIRS")
                .map(String::as_str),
            Some("/usr/local/share/:/usr/share/")
        );
    }

    /// What a session that ended without its launcher left in the user
    /// manager: ticket 203's review, P1.
    const LEFTOVERS: [(&str, &str); 6] = [
        ("DCONF_PROFILE", "betteros"),
        ("XDG_CURRENT_DESKTOP", "BetterOS:COSMIC"),
        ("XDG_SESSION_DESKTOP", "better-os"),
        (
            "XDG_DATA_DIRS",
            "/usr/share/better-os/session:/usr/local/share/:/usr/share/",
        ),
        ("QT_IM_MODULE", "fcitx"),
        ("XMODIFIERS", "@im=fcitx"),
    ];

    #[test]
    fn a_clean_end_after_a_crashed_session_removes_its_leftovers_too() {
        let mut manager = vec![("LANG", "zh_TW.UTF-8")];
        manager.extend(LEFTOVERS);
        let fake = Fake::with(&manager);
        let (import, problems) = begin(&fake, &session_environment(), &mut recorder(&fake));
        assert!(problems.is_empty(), "{problems:?}");
        let activation = environment::activation_restore(&import, "/default");
        restore(&fake, &import, &activation);
        let after = fake.manager.borrow().clone();
        assert_eq!(
            after.get("DCONF_PROFILE"),
            None,
            "the leftover profile goes"
        );
        assert_eq!(after.get("XDG_CURRENT_DESKTOP"), None);
        assert_eq!(after.get("QT_IM_MODULE"), None);
        assert_eq!(
            after.get("XDG_DATA_DIRS").map(String::as_str),
            Some("/usr/local/share/:/usr/share/"),
            "the generators' value is what was under the session's entry"
        );
        assert_eq!(after.get("LANG").map(String::as_str), Some("zh_TW.UTF-8"));
        // D-Bus activation: dconf's default, never the session's database.
        assert_eq!(
            fake.activation
                .borrow()
                .get("DCONF_PROFILE")
                .map(String::as_str),
            Some("/default")
        );
    }

    #[test]
    fn an_unreadable_user_manager_loses_nothing_that_was_not_the_sessions() {
        let fake = Fake::with(&[
            (
                "XDG_DATA_DIRS",
                "/usr/local/share/:/usr/share/:/var/lib/snapd/desktop",
            ),
            ("QT_IM_MODULE", "ibus"),
            ("XMODIFIERS", "@im=ibus"),
        ]);
        let fake = Fake {
            failing_read: true,
            ..fake
        };
        let (import, problems) = begin(&fake, &session_environment(), &mut recorder(&fake));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("could not be read"));
        // The session still has its environment.
        assert_eq!(
            fake.manager
                .borrow()
                .get("DCONF_PROFILE")
                .map(String::as_str),
            Some("betteros")
        );
        let fake = Fake {
            failing_read: false,
            ..fake
        };
        restore(
            &fake,
            &import,
            &environment::activation_restore(&import, "/default"),
        );
        let after = fake.manager.borrow().clone();
        assert_eq!(after.get("DCONF_PROFILE"), None);
        assert_eq!(after.get("WAYLAND_DISPLAY"), None);
        assert_eq!(
            after.get("XDG_DATA_DIRS").map(String::as_str),
            Some("/usr/share/"),
            "only the session's entry is taken out of what is there"
        );
        // Not provably the session's: left as they are.
        assert!(after.contains_key("QT_IM_MODULE"));
        assert!(after.contains_key("XMODIFIERS"));
    }

    #[test]
    fn units_that_do_not_stop_are_reported_after_the_patience() {
        let fake = Fake::default();
        begin(&fake, &session_environment(), &mut recorder(&fake));
        fake.stops_after.set(u32::MAX);
        let slept = Cell::new(Duration::ZERO);
        let sleep = |step| slept.set(slept.get() + step);
        let problems = stop_units(&fake, &sleep);
        assert_eq!(slept.get(), STOP_PATIENCE);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("graphical-session.target"));
    }
}
