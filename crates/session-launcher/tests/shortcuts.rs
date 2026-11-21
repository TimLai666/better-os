//! The session's key bindings and system actions name only programs the
//! session's packages install (ticket 203), and the keys the spec names point
//! at Better OS's programs.

mod common;

use std::collections::BTreeMap;

use common::{SESSION_SHORTCUTS, installed_binaries, read, session_depends};

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
    ("Screenshot", "ticket 253"),
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
    let better_os: Vec<String> = installed_binaries()
        .into_iter()
        .filter_map(|path| path.strip_prefix("/usr/bin/").map(str::to_string))
        .collect();
    let depends = session_depends();
    for (name, command) in system_actions() {
        let program = command.split_whitespace().next().unwrap();
        assert!(
            !NOT_INSTALLED.contains(&program),
            "{name} runs {program}, which is not installed"
        );
        if better_os.contains(&program.to_string()) {
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
