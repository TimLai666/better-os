//! The session's key bindings and system actions name only programs the
//! session's packages install (ticket 203), and the keys the spec names point
//! at Better OS's programs.

mod common;

use std::collections::BTreeMap;

use common::{SESSION_SHORTCUTS, programs_by_package, read, session_depends};

/// A program the session's system actions run that is not Better OS's, and
/// the package of the session's `Depends` that installs it.
const PROVIDERS: &[(&str, &str)] = &[
    ("busctl", "systemd"),
    ("loginctl", "systemd"),
    ("systemctl", "systemd"),
    ("cosmic-osd", "cosmic-osd"),
    ("wpctl", "wireplumber"),
    ("playerctl", "playerctl"),
    ("xdg-open", "xdg-utils"),
    (
        "x-terminal-emulator",
        "gnome-terminal | x-terminal-emulator",
    ),
];

/// COSMIC's actions with no Better OS program yet, each with what it waits
/// for. None is bound to a key and none has a command; the ticket that ships
/// the program adds both and takes the line out of this list.
const WAITING: &[(&str, &str)] = &[
    ("AppLibrary", "the owner's decision S3, ticket 210"),
    ("WindowSwitcher", "ticket 211"),
    ("WindowSwitcherPrevious", "ticket 211"),
    ("WorkspaceOverview", "ticket 212"),
];

/// The programs COSMIC's own defaults name that the closure does not install
/// (ticket 201).
const NOT_INSTALLED: [&str; 5] = [
    "cosmic-app-library",
    "cosmic-launcher",
    "cosmic-screenshot",
    "cosmic-term",
    "cosmic-workspaces",
];

fn without_comments(text: &str) -> String {
    // Whole-line comments only: a command may hold "//" (xdg-open http://).
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `Name: "command",` entries.
fn system_actions() -> BTreeMap<String, String> {
    let text = without_comments(&read(&format!(
        "packaging/session/{SESSION_SHORTCUTS}/system_actions"
    )));
    let mut actions = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line == "{" || line == "}" {
            continue;
        }
        let (name, command) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("not an entry: {line}"));
        let command = command.trim().trim_end_matches(',');
        assert!(
            command.starts_with('"') && command.ends_with('"'),
            "a quoted command: {line}"
        );
        let previous = actions.insert(
            name.trim().to_string(),
            command[1..command.len() - 1].to_string(),
        );
        assert!(previous.is_none(), "{name} once");
    }
    actions
}

/// `(binding): Action,` entries, the binding as written.
fn bindings() -> Vec<(String, String)> {
    let text = without_comments(&read(&format!(
        "packaging/session/{SESSION_SHORTCUTS}/defaults"
    )));
    let mut bindings = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line == "{" || line == "}" {
            continue;
        }
        let (binding, action) = line
            .rsplit_once("):")
            .unwrap_or_else(|| panic!("not a binding: {line}"));
        bindings.push((
            format!("{binding})"),
            action.trim().trim_end_matches(',').to_string(),
        ));
    }
    bindings
}

fn system_action(action: &str) -> Option<&str> {
    action.strip_prefix("System(")?.strip_suffix(')')
}

#[test]
fn every_bound_system_action_has_a_command() {
    let actions = system_actions();
    for (binding, action) in bindings() {
        if let Some(name) = system_action(&action) {
            assert!(
                actions.contains_key(name),
                "{binding} runs System({name}), which has no command"
            );
        }
    }
}

#[test]
fn every_command_runs_a_program_the_session_installs() {
    // A Better OS program counts only when the session package ships it or
    // depends on the package that does: any other package may be missing.
    let better_os = programs_by_package();
    let depends = session_depends();
    // The system actions' commands, and the keys that spawn a command.
    let spawned = bindings().into_iter().filter_map(|(binding, action)| {
        let command = action.strip_prefix("Spawn(\"")?.strip_suffix("\")")?;
        Some((binding, command.to_string()))
    });
    for (name, command) in system_actions().into_iter().chain(spawned) {
        let program = command.split_whitespace().next().unwrap();
        assert!(
            !NOT_INSTALLED.contains(&program),
            "{name} runs {program}, which is not installed"
        );
        if let Some((package, _)) = better_os.iter().find(|(_, shipped)| shipped == program) {
            assert!(
                package == "better-os-session" || depends.contains(package),
                "{name} runs {program} from {package}, which the session does not depend on"
            );
            continue;
        }
        let (_, package) = PROVIDERS
            .iter()
            .find(|(provided, _)| *provided == program)
            .unwrap_or_else(|| {
                panic!("{name} runs {program}, which no package of the session installs")
            });
        assert!(
            depends.contains(&package.to_string()),
            "{name} runs {program} from {package}, which the session does not depend on ({depends:?})"
        );
    }
}

#[test]
fn the_actions_waiting_for_a_program_are_bound_to_nothing() {
    let actions = system_actions();
    let bound: Vec<String> = bindings()
        .into_iter()
        .filter_map(|(_, action)| system_action(&action).map(str::to_string))
        .collect();
    for (name, waiting_for) in WAITING {
        assert!(
            !actions.contains_key(*name),
            "{name} waits for {waiting_for}"
        );
        assert!(
            !bound.contains(&name.to_string()),
            "no key runs {name} before {waiting_for}"
        );
    }
}

#[test]
fn super_and_super_space_open_better_launcher() {
    assert_eq!(
        system_actions().get("Launcher").map(String::as_str),
        Some("better-launcher")
    );
    let bindings = bindings();
    for binding in [
        "(modifiers: [Super])",
        "(modifiers: [Super], key: \"space\")",
        "(modifiers: [Super], key: \"slash\")",
    ] {
        assert!(
            bindings
                .iter()
                .any(|(bound, action)| bound == binding && action == "System(Launcher)"),
            "{binding} opens the launcher"
        );
    }
}

#[test]
fn the_workspace_keys_are_the_compositors() {
    let bindings = bindings();
    for number in 1..=9 {
        let binding = format!("(modifiers: [Super], key: \"{number}\")");
        assert!(
            bindings
                .iter()
                .any(|(bound, action)| *bound == binding
                    && *action == format!("Workspace({number})")),
            "{binding}"
        );
    }
    for (binding, action) in [
        (
            "(modifiers: [Super, Ctrl], key: \"Left\")",
            "PreviousWorkspace",
        ),
        (
            "(modifiers: [Super, Ctrl], key: \"Right\")",
            "NextWorkspace",
        ),
    ] {
        assert!(
            bindings
                .iter()
                .any(|(bound, act)| bound == binding && act == action),
            "{binding}"
        );
    }
}

#[test]
fn no_key_is_bound_twice() {
    let mut seen = std::collections::BTreeSet::new();
    for (binding, _) in bindings() {
        assert!(seen.insert(binding.clone()), "{binding} is bound twice");
    }
}

#[test]
fn logging_out_goes_through_the_launcher() {
    // cosmic-osd log-out calls com.system76.CosmicSession.Exit, which only
    // cosmic-osd may call and the launcher answers.
    assert_eq!(
        system_actions().get("LogOut").map(String::as_str),
        Some("cosmic-osd log-out")
    );
    assert!(session_launcher::callers::COSMIC_EXIT_CALLERS.contains(&"/usr/bin/cosmic-osd"));
}

#[test]
fn gnomes_screenshot_keys_reach_better_screenshot() {
    // Ticket 253: Print opens the interface, Shift+Print the whole screen,
    // Alt+Print the focused window, Ctrl+Shift+Alt+R a recording.
    assert_eq!(
        system_actions().get("Screenshot").map(String::as_str),
        Some("better-screenshot")
    );
    let bindings = bindings();
    for (binding, action) in [
        ("(modifiers: [], key: \"Print\")", "System(Screenshot)"),
        (
            "(modifiers: [Shift], key: \"Print\")",
            "Spawn(\"better-screenshot --screen\")",
        ),
        (
            "(modifiers: [Alt], key: \"Print\")",
            "Spawn(\"better-screenshot --window\")",
        ),
        (
            "(modifiers: [Ctrl, Shift, Alt], key: \"r\")",
            "Spawn(\"better-screenshot --record\")",
        ),
    ] {
        assert!(
            bindings
                .iter()
                .any(|(bound, act)| bound == binding && act == action),
            "{binding} runs {action}"
        );
    }
    let portals = read("packaging/session/betteros-portals.conf");
    assert!(
        portals
            .lines()
            .any(|line| line == "org.freedesktop.impl.portal.Screenshot=betteros-screenshot;"),
        "the Screenshot portal is Better Screenshot's"
    );
}

#[test]
fn the_upstream_files_the_session_replaces_are_recorded() {
    // packaging/session/cosmic/upstream.tsv: one line per session file, with
    // the installed upstream file, its package, version, source, licence and
    // hash, which the VM check compares with the installed closure.
    let record = read("packaging/session/cosmic/upstream.tsv");
    let mut files = Vec::new();
    for line in record
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 7, "seven fields: {line}");
        assert!(
            fields[1].starts_with("/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/")
        );
        assert!(
            fields[4].contains(&fields[3].rsplit('~').next().unwrap()[..7]),
            "{line}"
        );
        assert_eq!(fields[6].len(), 64, "a SHA-256: {line}");
        files.push(fields[0].to_string());
        // The session's file says what it derives from.
        let session = read(&format!(
            "packaging/session/{SESSION_SHORTCUTS}/{}",
            fields[0]
        ));
        assert!(
            session.contains(fields[2]),
            "{} names {}",
            fields[0],
            fields[2]
        );
    }
    files.sort();
    assert_eq!(files, vec!["defaults", "system_actions"]);
}
