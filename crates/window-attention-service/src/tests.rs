use window_attention_core::store::Store;
use window_attention_core::{AppId, AttentionRequest, Choice, Choices, RequestKind, ShellAction};

use crate::{Relay, Shell, parse_request};

#[derive(Default)]
struct FakeShell {
    calls: Vec<String>,
    fail: bool,
}

impl FakeShell {
    fn result(&mut self, call: String) -> Result<(), String> {
        self.calls.push(call);
        if self.fail {
            Err("the shell refused".to_string())
        } else {
            Ok(())
        }
    }
}

impl Shell for FakeShell {
    fn suppress_built_in(&mut self, suppress: bool) -> Result<(), String> {
        self.result(format!("suppress {suppress}"))
    }
    fn mark_taskbar(&mut self, app: &AppId) -> Result<(), String> {
        self.result(format!("mark {app}"))
    }
    fn unmark_taskbar(&mut self, app: &AppId) -> Result<(), String> {
        self.result(format!("unmark {app}"))
    }
    fn show_banner(&mut self, window: u64) -> Result<(), String> {
        self.result(format!("banner {window}"))
    }
    fn activate(&mut self, window: u64) -> Result<(), String> {
        self.result(format!("activate {window}"))
    }
    fn clear(&mut self, window: u64) -> Result<(), String> {
        self.result(format!("clear {window}"))
    }
}

fn request(app_id: &str, window: u64) -> AttentionRequest {
    AttentionRequest {
        window,
        app_id: app_id.to_string(),
        app_name: "Claude".to_string(),
        kind: RequestKind::DemandsAttention,
        focused: false,
        skip_taskbar: false,
    }
}

const CLAUDE: &str = "com.anthropic.Claude.desktop";

#[test]
fn with_nothing_chosen_a_request_keeps_only_the_taskbar_mark_and_is_recorded() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let mut relay = Relay::with_clock(store.clone(), || 1_000);
    let mut shell = FakeShell::default();

    let handled = relay.handle(&mut shell, &request(CLAUDE, 3));
    assert_eq!(
        handled.action,
        ShellAction::MarkTaskbar(AppId::new(CLAUDE).unwrap())
    );
    assert_eq!(shell.calls, [format!("mark {CLAUDE}")]);
    assert!(handled.recorded);
    let seen = store.load_seen().unwrap();
    let entry = &seen.apps[&AppId::new(CLAUDE).unwrap()];
    assert_eq!((entry.name.as_str(), entry.first_seen), ("Claude", 1_000));

    // Asking again writes nothing new.
    let again = relay.handle(&mut shell, &request(CLAUDE, 3));
    assert!(!again.recorded);
}

#[test]
fn each_choice_reaches_the_shell_as_its_one_method() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    for (choice, call) in [
        (Choice::BannerAndMark, Some("banner 9")),
        (Choice::MarkOnly, Some("mark com.anthropic.Claude.desktop")),
        (Choice::Nothing, Some("clear 9")),
        (Choice::BringForward, Some("activate 9")),
    ] {
        let choices = Choices::with_default(choice);
        store.save_choices(&choices).unwrap();
        let mut relay = Relay::new(store.clone());
        let mut shell = FakeShell::default();
        relay.handle(&mut shell, &request(CLAUDE, 9));
        assert_eq!(
            shell.calls,
            call.map(str::to_string).into_iter().collect::<Vec<_>>(),
            "{choice:?}"
        );
    }
}

/// A choice saved in Better Manager applies to the next request, with no
/// reload: the file is read for each one.
#[test]
fn a_per_app_choice_applies_to_the_next_request_and_removing_it_falls_back() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let mut relay = Relay::new(store.clone());
    let mut choices = Choices::with_default(Choice::BannerAndMark);
    store.save_choices(&choices).unwrap();

    let mut shell = FakeShell::default();
    relay.handle(&mut shell, &request(CLAUDE, 1));
    choices.set(AppId::new(CLAUDE).unwrap(), "Claude", Choice::Nothing);
    store.save_choices(&choices).unwrap();
    relay.handle(&mut shell, &request(CLAUDE, 1));
    relay.handle(&mut shell, &request("org.gnome.Nautilus.desktop", 2));
    choices.remove(&AppId::new(CLAUDE).unwrap());
    store.save_choices(&choices).unwrap();
    relay.handle(&mut shell, &request(CLAUDE, 1));
    assert_eq!(shell.calls, ["banner 1", "clear 1", "banner 2", "banner 1"]);
}

#[test]
fn an_unreadable_choices_file_uses_the_default_and_says_so() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    std::fs::write(store.choices_path(), "{").unwrap();
    let mut relay = Relay::new(store);
    let mut shell = FakeShell::default();
    let handled = relay.handle(&mut shell, &request(CLAUDE, 1));
    assert_eq!(
        handled.action,
        ShellAction::MarkTaskbar(AppId::new(CLAUDE).unwrap())
    );
    assert!(handled.choices_problem.is_some());
}

#[test]
fn a_shell_that_refuses_is_reported_and_the_request_is_still_recorded() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let choices = Choices::with_default(Choice::BringForward);
    store.save_choices(&choices).unwrap();
    let mut relay = Relay::new(store);
    let mut shell = FakeShell {
        fail: true,
        ..FakeShell::default()
    };
    let handled = relay.handle(&mut shell, &request(CLAUDE, 1));
    assert_eq!(handled.failure.as_deref(), Some("the shell refused"));
    assert!(handled.recorded);
}

#[test]
fn a_seen_list_from_a_newer_version_is_left_alone() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let newer = r#"{"version":5,"apps":{}}"#;
    std::fs::write(store.seen_path(), newer).unwrap();
    let mut relay = Relay::new(store.clone());
    let handled = relay.handle(&mut FakeShell::default(), &request(CLAUDE, 1));
    assert!(handled.record_problem.is_some());
    assert_eq!(std::fs::read_to_string(store.seen_path()).unwrap(), newer);
}

#[test]
fn the_adapter_appearing_takes_gnome_s_handler_off() {
    let directory = tempfile::tempdir().unwrap();
    let relay = Relay::new(Store::new(directory.path()));
    let mut shell = FakeShell::default();
    relay.adapter_appeared(&mut shell).unwrap();
    assert_eq!(shell.calls, ["suppress true"]);
}

#[test]
fn only_the_two_wire_kinds_are_requests() {
    let body = |kind| {
        (
            4_u64,
            CLAUDE.to_string(),
            "Claude".to_string(),
            kind,
            false,
            true,
        )
    };
    let parsed = parse_request(body(1)).unwrap();
    assert_eq!(parsed.kind, RequestKind::MarkedUrgent);
    assert!(parsed.skip_taskbar);
    assert!(parse_request(body(7)).is_none());
}

/// A mark is taken back when its window's request ends, and only once no
/// other window of the same application is still marked.
#[test]
fn a_taskbar_mark_is_taken_back_when_the_last_marked_window_stops_asking() {
    let directory = tempfile::tempdir().unwrap();
    let mut relay = Relay::new(Store::new(directory.path()));
    let mut shell = FakeShell::default();
    relay.handle(&mut shell, &request(CLAUDE, 1));
    relay.handle(&mut shell, &request(CLAUDE, 2));
    assert_eq!(relay.marked().len(), 2);
    assert_eq!(relay.ended(&mut shell, 1), None);
    assert_eq!(shell.calls.len(), 2, "window 2 still holds the mark");
    relay.ended(&mut shell, 2);
    assert_eq!(shell.calls.last().unwrap(), &format!("unmark {CLAUDE}"));
    assert!(relay.marked().is_empty());
    // An end for a window that was never marked does nothing.
    let before = shell.calls.len();
    relay.ended(&mut shell, 99);
    assert_eq!(shell.calls.len(), before);
}

#[test]
fn a_changed_choice_takes_back_the_marks_it_no_longer_asks_for() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let mut relay = Relay::new(store.clone());
    let mut shell = FakeShell::default();
    relay.handle(&mut shell, &request(CLAUDE, 1));
    relay.handle(&mut shell, &request("org.gnome.Nautilus.desktop", 2));

    let mut choices = Choices::default();
    choices.set(AppId::new(CLAUDE).unwrap(), "Claude", Choice::Nothing);
    store.save_choices(&choices).unwrap();
    assert!(relay.choices_changed(&mut shell).is_empty());
    assert_eq!(shell.calls.last().unwrap(), &format!("unmark {CLAUDE}"));
    assert_eq!(
        relay.marked().len(),
        1,
        "Files still follows the mark-only default"
    );

    // The same window asking again under the new choice gives nothing back
    // twice and is cleared.
    let before = shell.calls.len();
    relay.handle(&mut shell, &request(CLAUDE, 1));
    assert_eq!(&shell.calls[before..], ["clear 1"]);
}

#[test]
fn the_extension_leaving_takes_every_mark_back() {
    let directory = tempfile::tempdir().unwrap();
    let mut relay = Relay::new(Store::new(directory.path()));
    let mut shell = FakeShell::default();
    relay.handle(&mut shell, &request(CLAUDE, 1));
    relay.handle(&mut shell, &request("org.gnome.Nautilus.desktop", 2));
    relay.adapter_gone(&mut shell);
    assert!(relay.marked().is_empty());
    assert!(shell.calls.contains(&format!("unmark {CLAUDE}")));
    assert!(
        shell
            .calls
            .contains(&"unmark org.gnome.Nautilus.desktop".to_string())
    );
}

#[test]
fn the_takeover_retries_with_backoff_for_up_to_thirty_seconds() {
    let waits = crate::bus::takeover_backoff();
    let total: std::time::Duration = waits.iter().sum();
    assert!(total <= crate::bus::TAKEOVER_PATIENCE);
    assert!(total >= std::time::Duration::from_secs(25), "{total:?}");
    assert_eq!(waits[0], std::time::Duration::from_millis(100));
    assert!(waits.windows(2).all(|pair| pair[1] >= pair[0]));
    assert!(
        waits
            .iter()
            .all(|wait| *wait <= std::time::Duration::from_secs(5))
    );
}
