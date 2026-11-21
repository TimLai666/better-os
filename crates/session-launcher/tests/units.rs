//! The session's target and units hold to the ordering rules of
//! engineering/specs/better-os-session.md ("How Better OS's existing user
//! units bind to the session"), and every existing Better OS user unit starts
//! and stops with the session through graphical-session.target.

mod common;

use common::{installed_data, read, sections, unit_lines, values};

const SESSION_UNITS: [&str; 5] = [
    "better-os-session-settings-daemon.service",
    "better-os-session-osd.service",
    "better-os-session-idle.service",
    "better-os-session-background.service",
    "better-os-session-input-method.service",
];

fn target() -> Vec<(String, String)> {
    unit_lines(&read("packaging/session/better-os-session.target"))
}

#[test]
fn the_launcher_starts_the_target_the_package_ships() {
    assert!(
        installed_data().iter().any(|(source, installed)| {
            source == "packaging/session/better-os-session.target"
                && installed == &format!("usr/lib/systemd/user/{}", session_launcher::TARGET)
        }),
        "build-deb.sh installs {} under its own name",
        session_launcher::TARGET
    );
}

fn autostart_target() -> Vec<(String, String)> {
    unit_lines(&read(
        "packaging/session/better-os-session-autostart.target",
    ))
}

#[test]
fn the_target_binds_the_graphical_session() {
    let target = target();
    assert_eq!(values(&target, "BindsTo"), vec!["graphical-session.target"]);
    assert!(values(&target, "Before").contains(&"graphical-session.target".to_string()));
    assert!(values(&target, "Wants").contains(&"graphical-session-pre.target".to_string()));
    assert!(values(&target, "After").contains(&"graphical-session-pre.target".to_string()));
    // The launcher starts it; nothing enables it.
    let text = read("packaging/session/better-os-session.target");
    assert!(!sections(&text).contains(&"Install".to_string()));
    assert!(values(&target, "RefuseManualStart").is_empty());
}

/// Ticket 203's second review: a condition skips only its own unit, never
/// what it wants (systemd.unit(5)), so the target's condition did not keep
/// the XDG autostart target and Tracker from starting when something
/// started the target under GNOME. Every unit the target wants carries the
/// condition itself; the two that cannot are wanted by a second target that
/// only the launcher starts, after the first.
#[test]
fn autostart_and_tracker_are_pulled_in_only_by_the_launchers_second_target() {
    let target = target();
    for unconditioned in ["xdg-desktop-autostart.target", "tracker-miner-fs-3.service"] {
        assert!(
            !values(&target, "Wants").contains(&unconditioned.to_string())
                && !values(&target, "Requires").contains(&unconditioned.to_string()),
            "the session's target would start {unconditioned} under GNOME"
        );
    }
    // Everything the target wants skips itself outside the session.
    for wanted in values(&target, "Wants") {
        if wanted == "graphical-session-pre.target" {
            // GNOME's own login starts it anyway; it starts nothing of ours
            // but the leftovers unit, which is meant to run there.
            continue;
        }
        let lines = unit_lines(&read(&format!("packaging/session/{wanted}")));
        assert_eq!(
            values(&lines, "ConditionPathExists"),
            vec![IN_SESSION],
            "{wanted} would start under GNOME with the target"
        );
    }
    let second = autostart_target();
    let wants = values(&second, "Wants");
    for wanted in ["xdg-desktop-autostart.target", "tracker-miner-fs-3.service"] {
        assert!(wants.contains(&wanted.to_string()), "it wants {wanted}");
    }
    assert_eq!(values(&second, "ConditionPathExists"), vec![IN_SESSION]);
    assert_eq!(values(&second, "PartOf"), vec![session_launcher::TARGET]);
    assert!(values(&second, "After").contains(&session_launcher::TARGET.to_string()));
    let text = read("packaging/session/better-os-session-autostart.target");
    assert!(!sections(&text).contains(&"Install".to_string()));
    assert!(installed_data().iter().any(|(source, installed)| {
        source == "packaging/session/better-os-session-autostart.target"
            && installed
                == &format!(
                    "usr/lib/systemd/user/{}",
                    session_launcher::AUTOSTART_TARGET
                )
    }));
    // No unit of the package names the second target: only the launcher
    // starts it ([`session_launcher::lifecycle::begin`]).
    for unit in SESSION_UNITS.iter().chain([
        &session_launcher::GUARD_UNIT,
        &session_launcher::LEFTOVERS_UNIT,
    ]) {
        assert!(
            !read(&format!("packaging/session/{unit}"))
                .contains(session_launcher::AUTOSTART_TARGET),
            "{unit}"
        );
    }
    assert!(
        !read("packaging/session/better-os-session.target")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .any(|line| line.contains(session_launcher::AUTOSTART_TARGET))
    );
}

#[test]
fn every_session_unit_is_the_targets_and_never_graphical_sessions() {
    let wants = values(&target(), "Wants");
    let shipped: Vec<String> = installed_data()
        .into_iter()
        .filter(|(source, installed)| {
            source.starts_with("packaging/session/")
                && installed.starts_with("usr/lib/systemd/user/")
                && installed.ends_with(".service")
        })
        .map(|(_, installed)| installed.rsplit('/').next().unwrap().to_string())
        .collect();
    let mut expected: Vec<String> = SESSION_UNITS
        .iter()
        .chain([
            &session_launcher::GUARD_UNIT,
            &session_launcher::LEFTOVERS_UNIT,
        ])
        .map(|unit| unit.to_string())
        .collect();
    expected.sort();
    let mut shipped_sorted = shipped.clone();
    shipped_sorted.sort();
    assert_eq!(
        shipped_sorted, expected,
        "the package ships exactly the session's units"
    );
    for unit in SESSION_UNITS {
        let text = read(&format!("packaging/session/{unit}"));
        let lines = unit_lines(&text);
        assert!(
            wants.contains(&unit.to_string()),
            "the target starts {unit}"
        );
        assert_eq!(
            values(&lines, "PartOf"),
            vec![session_launcher::TARGET],
            "{unit} stops with the target"
        );
        // Ordered after the target, so stopping the target waits for it.
        assert!(
            values(&lines, "After").contains(&session_launcher::TARGET.to_string()),
            "{unit}"
        );
        assert!(
            !sections(&text).contains(&"Install".to_string()),
            "{unit} needs no enable step"
        );
        assert!(
            !text.contains("graphical-session.target"),
            "{unit} would start under GNOME"
        );
        let exec = values(&lines, "ExecStart");
        assert!(
            exec[0].starts_with("/usr/bin/"),
            "{unit} names an absolute program"
        );
    }
}

#[test]
fn the_target_wants_nothing_it_would_be_ordered_after_in_a_cycle() {
    // A target is After= every unit it Wants= unless that unit is already
    // ordered after it. Autostart entries are After=graphical-session.target,
    // which is after this target: without Before= the implicit order is a
    // cycle, and systemd broke it on Zorin OS 18.1 by dropping every
    // autostart entry. The target that wants them is ordered before them.
    assert!(
        values(&autostart_target(), "Before").contains(&"xdg-desktop-autostart.target".to_string())
    );
    let target = target();
    assert!(
        values(&target, "Wants")
            .iter()
            .all(|wanted| wanted != "xdg-desktop-autostart.target")
    );
    // Each unit of the session it wants is ordered after it.
    for unit in SESSION_UNITS.iter().chain([&session_launcher::GUARD_UNIT]) {
        if *unit == session_launcher::GUARD_UNIT {
            // The guard is the one the target is after, on purpose: it has
            // no ordering of its own against the target.
            let guard = unit_lines(&read(&format!("packaging/session/{unit}")));
            assert!(values(&guard, "After").is_empty() && values(&guard, "Before").is_empty());
            continue;
        }
        let lines = unit_lines(&read(&format!("packaging/session/{unit}")));
        assert!(
            values(&lines, "After").contains(&session_launcher::TARGET.to_string()),
            "{unit}"
        );
    }
}

/// The condition every unit that may run only in the session carries.
const IN_SESSION: &str = "%t/better-os/session/restore.json";

#[test]
fn nothing_of_the_session_starts_outside_it() {
    // The record's path in the units is the one the launcher writes.
    assert_eq!(
        IN_SESSION,
        format!(
            "%t/{}/{}",
            session_launcher::record::DIRECTORY,
            session_launcher::record::FILE
        )
    );
    assert_eq!(values(&target(), "ConditionPathExists"), vec![IN_SESSION]);
    assert_eq!(
        values(&autostart_target(), "ConditionPathExists"),
        vec![IN_SESSION]
    );
    for unit in SESSION_UNITS.iter().chain([&session_launcher::GUARD_UNIT]) {
        let lines = unit_lines(&read(&format!("packaging/session/{unit}")));
        assert_eq!(
            values(&lines, "ConditionPathExists"),
            vec![IN_SESSION],
            "{unit} refuses to start without a session's record"
        );
    }
}

#[test]
fn the_guard_holds_the_target_and_puts_the_environment_back_however_it_stops() {
    let target = target();
    assert!(values(&target, "Wants").contains(&session_launcher::GUARD_UNIT.to_string()));
    assert!(values(&target, "After").contains(&session_launcher::GUARD_UNIT.to_string()));
    let guard = unit_lines(&read(&format!(
        "packaging/session/{}",
        session_launcher::GUARD_UNIT
    )));
    assert_eq!(values(&guard, "PartOf"), vec![session_launcher::TARGET]);
    assert_eq!(
        values(&guard, "ExecStart"),
        vec![session_launcher::INSTALLED_PATH, "--guard"]
    );
    // ExecStopPost runs however the service stops, a failure included.
    assert_eq!(
        values(&guard, "ExecStopPost"),
        vec![session_launcher::INSTALLED_PATH, "--restore"]
    );
}

#[test]
fn the_leftovers_unit_is_the_one_that_runs_in_any_session() {
    let text = read(&format!(
        "packaging/session/{}",
        session_launcher::LEFTOVERS_UNIT
    ));
    let lines = unit_lines(&text);
    // Any session of the person, GNOME's too: before its graphical session,
    // and before GNOME's own early target, which every GNOME login starts
    // even when a dead session left graphical-session-pre.target active
    // (ticket 203's second review, VM).
    let targets = vec!["graphical-session-pre.target", "gnome-session-pre.target"];
    assert_eq!(values(&lines, "WantedBy"), targets);
    assert_eq!(values(&lines, "Before"), targets);
    assert!(!text.contains("WantedBy=graphical-session.target"));
    assert_eq!(values(&lines, "ConditionPathExists"), vec![IN_SESSION]);
    assert_eq!(
        values(&lines, "ExecStart"),
        vec![session_launcher::INSTALLED_PATH, "--leftovers"]
    );
    // graphical-session-pre.target waits for it: a hung call must not hold a
    // GNOME login up for ever (a oneshot has no start limit by default).
    assert_eq!(values(&lines, "TimeoutStartSec"), vec!["15s"]);
    assert!(
        common::read("packaging/build-deb.sh")
            .contains("PACKAGE_ALWAYS_ON_UNITS=\"better-os-session-leftovers.service\""),
        "enabled for every person at install, disabled at removal"
    );
}

#[test]
fn the_locker_is_not_a_unit() {
    // cosmic-greeter finds its logind session by its own process id and exits
    // as a user unit (ticket 201); the launcher runs it.
    for unit in SESSION_UNITS {
        assert!(
            !read(&format!("packaging/session/{unit}")).contains("cosmic-greeter"),
            "{unit}"
        );
    }
    assert_eq!(
        session_launcher::children::LOCKER.program,
        "/usr/bin/cosmic-greeter"
    );
}

#[test]
fn the_daemons_the_spec_names_each_have_a_unit() {
    let programs: Vec<String> = SESSION_UNITS
        .iter()
        .map(|unit| {
            values(
                &unit_lines(&read(&format!("packaging/session/{unit}"))),
                "ExecStart",
            )[0]
            .clone()
        })
        .collect();
    for program in [
        "/usr/bin/cosmic-settings-daemon",
        "/usr/bin/cosmic-osd",
        "/usr/bin/cosmic-idle",
        "/usr/bin/cosmic-bg",
        "/usr/bin/fcitx5",
    ] {
        assert!(
            programs.contains(&program.to_string()),
            "{program} runs in the session"
        );
    }
}

#[test]
fn tracker_stops_with_the_session() {
    let dropin = unit_lines(&read(
        "packaging/session/tracker-miner-fs-3-better-os-session.conf",
    ));
    assert_eq!(values(&dropin, "PartOf"), vec![session_launcher::TARGET]);
    assert!(installed_data().iter().any(|(source, installed)| {
        source == "packaging/session/tracker-miner-fs-3-better-os-session.conf"
            && installed
                == "usr/lib/systemd/user/tracker-miner-fs-3.service.d/better-os-session.conf"
    }));
}

#[test]
fn every_existing_better_os_user_unit_starts_and_stops_with_any_graphical_session() {
    let units: Vec<(String, String)> = installed_data()
        .into_iter()
        .filter(|(source, installed)| {
            !source.starts_with("packaging/session/")
                && installed.starts_with("usr/lib/systemd/user/")
                && installed.ends_with(".service")
        })
        .collect();
    assert!(
        units.len() >= 7,
        "the existing user units are found ({units:?})"
    );
    for (source, _) in units {
        let lines = unit_lines(&read(&source));
        assert_eq!(
            values(&lines, "PartOf"),
            vec!["graphical-session.target"],
            "{source}"
        );
        assert!(
            values(&lines, "After").contains(&"graphical-session.target".to_string()),
            "{source}"
        );
        assert_eq!(
            values(&lines, "WantedBy"),
            vec!["graphical-session.target"],
            "{source}"
        );
    }
}

#[test]
fn every_file_under_packaging_session_is_installed() {
    let installed: Vec<String> = installed_data()
        .into_iter()
        .map(|(source, _)| source)
        .collect();
    let root = common::repository().canonicalize().unwrap();
    let mut stack = vec![root.join("packaging/session")];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .to_string();
            // The record of what the shortcut directory replaces is for the
            // checks, not the package.
            if relative == "packaging/session/cosmic/upstream.tsv" {
                continue;
            }
            assert!(
                installed.contains(&relative),
                "build-deb.sh installs {relative}"
            );
        }
    }
}

#[test]
fn the_session_file_runs_the_launcher() {
    let entry = unit_lines(&read("packaging/session/better-os.desktop"));
    assert_eq!(
        values(&entry, "Exec"),
        vec![session_launcher::INSTALLED_PATH]
    );
    assert_eq!(values(&entry, "DesktopNames"), vec!["BetterOS;COSMIC"]);
    assert!(installed_data().iter().any(|(source, installed)| {
        source == "packaging/session/better-os.desktop"
            && installed
                == &format!(
                    "usr/share/wayland-sessions/{}.desktop",
                    better_core::desktop::BETTER_OS_SESSION_NAME
                )
    }));
    assert!(common::installed_binaries().contains(&session_launcher::INSTALLED_PATH.to_string()));
}

#[test]
fn the_dconf_profile_and_the_portals_are_the_specs() {
    assert_eq!(
        read("packaging/session/betteros.dconf-profile"),
        "user-db:betteros\nuser-db:user\n"
    );
    assert!(installed_data().iter().any(|(_, installed)| {
        installed
            == &format!(
                "usr/share/dconf/profile/{}",
                session_launcher::environment::DCONF_PROFILE
            )
    }));
    let portals = unit_lines(&read("packaging/session/betteros-portals.conf"));
    assert_eq!(values(&portals, "default"), vec!["cosmic;gtk;"]);
    assert_eq!(
        values(&portals, "org.freedesktop.impl.portal.Secret"),
        vec!["gnome-keyring;"]
    );
    // The file name is what xdg-desktop-portal looks for with
    // XDG_CURRENT_DESKTOP's first entry, lowercased.
    let first = session_launcher::environment::CURRENT_DESKTOP
        .split(':')
        .next()
        .unwrap()
        .to_lowercase();
    assert!(installed_data().iter().any(|(_, installed)| {
        installed == &format!("usr/share/xdg-desktop-portal/{first}-portals.conf")
    }));
}
