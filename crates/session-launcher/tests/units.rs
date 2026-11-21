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

#[test]
fn the_target_binds_the_graphical_session_and_pulls_in_autostart() {
    let target = target();
    assert_eq!(
        values(&target, "BindsTo"),
        vec!["graphical-session.target", session_launcher::GUARD_UNIT]
    );
    assert!(values(&target, "Before").contains(&"graphical-session.target".to_string()));
    let wants = values(&target, "Wants");
    for wanted in [
        "graphical-session-pre.target",
        "xdg-desktop-autostart.target",
        "tracker-miner-fs-3.service",
    ] {
        assert!(
            wants.contains(&wanted.to_string()),
            "the target wants {wanted}"
        );
    }
    assert!(values(&target, "After").contains(&"graphical-session-pre.target".to_string()));
    // The launcher starts it; nothing enables it.
    let text = read("packaging/session/better-os-session.target");
    assert!(!sections(&text).contains(&"Install".to_string()));
    assert!(values(&target, "RefuseManualStart").is_empty());
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
    assert!(values(&target, "BindsTo").contains(&session_launcher::GUARD_UNIT.to_string()));
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
    // Any session of the person, GNOME's too: before its graphical session.
    assert_eq!(
        values(&lines, "WantedBy"),
        vec!["graphical-session-pre.target"]
    );
    assert!(!text.contains("WantedBy=graphical-session.target"));
    assert_eq!(values(&lines, "ConditionPathExists"), vec![IN_SESSION]);
    assert_eq!(
        values(&lines, "ExecStart"),
        vec![session_launcher::INSTALLED_PATH, "--leftovers"]
    );
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
