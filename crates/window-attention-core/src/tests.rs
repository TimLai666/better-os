use std::collections::BTreeMap;

use crate::store::{Store, StoreError};
use crate::{
    AppId, AttentionRequest, Choice, Choices, DEFAULT_CHOICE, Decision, MAX_APP_NAME_CHARS,
    MAX_SEEN_APPS, RequestKind, SeenApps, ShellAction, clean_name, decide,
};

fn app(id: &str) -> AppId {
    AppId::new(id).unwrap()
}

fn request(app_id: &str) -> AttentionRequest {
    AttentionRequest {
        window: 7,
        app_id: app_id.to_string(),
        app_name: "Claude".to_string(),
        kind: RequestKind::DemandsAttention,
        focused: false,
        skip_taskbar: false,
    }
}

#[test]
fn the_default_is_the_taskbar_mark_only() {
    assert_eq!(DEFAULT_CHOICE, Choice::MarkOnly);
    assert_eq!(Choices::default().default, Choice::MarkOnly);
    assert_eq!(Choice::default(), Choice::MarkOnly);
}

#[test]
fn each_choice_asks_the_shell_for_exactly_one_thing() {
    let mut choices = Choices::default();
    let cases = [
        (Choice::BannerAndMark, ShellAction::ShowBanner(7)),
        (
            Choice::MarkOnly,
            ShellAction::MarkTaskbar(app("com.anthropic.Claude.desktop")),
        ),
        (Choice::Nothing, ShellAction::Clear(7)),
        (Choice::BringForward, ShellAction::Activate(7)),
    ];
    for (choice, action) in cases {
        choices.default = choice;
        assert_eq!(
            decide(&choices, &request("com.anthropic.Claude.desktop")).action,
            action,
            "{choice:?}"
        );
    }
}

#[test]
fn an_application_s_own_choice_overrides_the_default_and_removing_it_falls_back() {
    let mut choices = Choices::default();
    let claude = app("com.anthropic.Claude.desktop");
    choices.set(claude.clone(), "Claude", Choice::Nothing);
    assert_eq!(
        decide(&choices, &request("com.anthropic.Claude.desktop")).action,
        ShellAction::Clear(7)
    );
    // Another application still follows the default.
    assert_eq!(
        decide(&choices, &request("org.gnome.Nautilus.desktop")).action,
        ShellAction::MarkTaskbar(app("org.gnome.Nautilus.desktop"))
    );
    assert!(choices.remove(&claude));
    assert!(!choices.remove(&claude));
    assert_eq!(
        decide(&choices, &request("com.anthropic.Claude.desktop")).action,
        ShellAction::MarkTaskbar(claude)
    );
    // With the default on the mark, a window with no application keeps its
    // request and there is no icon to mark.
    assert_eq!(
        decide(&Choices::default(), &request("window:3")).action,
        ShellAction::Leave
    );
}

#[test]
fn a_focused_window_or_one_outside_the_taskbar_is_left_alone_and_not_recorded() {
    let choices = Choices::with_default(Choice::BringForward);
    let mut focused = request("org.gimp.GIMP.desktop");
    focused.focused = true;
    assert_eq!(
        decide(&choices, &focused),
        Decision {
            action: ShellAction::Leave,
            record: None
        }
    );
    let mut toolbar = request("org.gimp.GIMP.desktop");
    toolbar.skip_taskbar = true;
    assert_eq!(decide(&choices, &toolbar).action, ShellAction::Leave);
}

#[test]
fn a_window_the_tracker_could_not_match_follows_the_default_and_is_not_recorded() {
    let choices = Choices::with_default(Choice::BannerAndMark);
    let decision = decide(&choices, &request("window:42"));
    assert_eq!(decision.action, ShellAction::ShowBanner(7));
    assert_eq!(decision.record, None);
    let decision = decide(&choices, &request("com.anthropic.Claude.desktop"));
    assert_eq!(
        decision.record,
        Some((app("com.anthropic.Claude.desktop"), "Claude".to_string()))
    );
}

#[test]
fn an_app_id_names_a_desktop_file_and_nothing_else() {
    for good in [
        "org.gnome.Nautilus.desktop",
        "firefox_firefox.desktop",
        "code-url-handler.desktop",
    ] {
        assert!(AppId::new(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        ".desktop",
        "window:42",
        "../etc/passwd.desktop",
        "a b.desktop",
        "org.gnome.Nautilus",
        "naïve.desktop",
    ] {
        assert!(AppId::new(bad).is_err(), "{bad}");
    }
    let long = format!("{}.desktop", "a".repeat(250));
    assert!(AppId::new(long).is_err());
}

#[test]
fn a_name_loses_control_characters_and_is_cut() {
    assert_eq!(clean_name("  Claude\n\u{7}  "), "Claude");
    assert_eq!(
        clean_name(&"字".repeat(400)).chars().count(),
        MAX_APP_NAME_CHARS
    );
}

#[test]
fn the_seen_list_records_once_follows_a_rename_and_forgets_the_oldest_when_full() {
    let mut seen = SeenApps::default();
    let claude = app("com.anthropic.Claude.desktop");
    assert!(seen.record(&claude, "Claude", 10));
    assert!(!seen.record(&claude, "Claude", 20));
    assert!(!seen.record(&claude, "", 20));
    assert!(seen.record(&claude, "Claude Desktop", 30));
    assert_eq!(seen.apps[&claude].first_seen, 10);
    assert_eq!(seen.apps[&claude].name, "Claude Desktop");

    let mut full = SeenApps::default();
    for index in 0..MAX_SEEN_APPS {
        full.record(
            &app(&format!("app{index}.desktop")),
            "x",
            100 + index as u64,
        );
    }
    assert!(full.record(&app("newest.desktop"), "x", 10_000));
    assert_eq!(full.apps.len(), MAX_SEEN_APPS);
    assert!(!full.apps.contains_key(&app("app0.desktop")));
    assert!(full.apps.contains_key(&app("newest.desktop")));
}

#[test]
fn the_store_reads_defaults_when_empty_and_round_trips_both_files() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path().join("window-attention"));
    assert_eq!(store.load_choices().unwrap(), Choices::default());
    assert!(store.load_seen().unwrap().apps.is_empty());

    let mut choices = Choices::with_default(Choice::Nothing);
    choices.set(
        app("com.anthropic.Claude.desktop"),
        "Claude",
        Choice::BringForward,
    );
    store.save_choices(&choices).unwrap();
    assert_eq!(store.load_choices().unwrap(), choices);

    let mut seen = SeenApps::default();
    seen.record(&app("org.gnome.Nautilus.desktop"), "Files", 5);
    store.save_seen(&seen).unwrap();
    assert_eq!(store.load_seen().unwrap().apps, seen.apps);

    // No temporary file is left beside them.
    let mut names: Vec<String> = std::fs::read_dir(store.directory())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["choices.json", "seen.json"]);
}

#[test]
fn the_file_format_is_the_words_a_person_reading_it_would_expect() {
    let mut choices = Choices::default();
    choices.set(app("a.desktop"), "A", Choice::BannerAndMark);
    let text = serde_json::to_string(&choices).unwrap();
    assert_eq!(
        text,
        r#"{"version":1,"default":"mark_only","apps":{"a.desktop":{"choice":"banner_and_mark","name":"A"}}}"#
    );
    let parsed: Choices =
        serde_json::from_str(r#"{"version":1,"default":"bring_forward"}"#).unwrap();
    assert_eq!(parsed.default, Choice::BringForward);
    assert_eq!(parsed.apps, BTreeMap::new());
}

#[test]
fn a_broken_or_newer_file_is_an_error_and_never_silently_the_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    std::fs::write(store.choices_path(), "{ not json").unwrap();
    assert!(matches!(
        store.load_choices(),
        Err(StoreError::Parse { .. })
    ));
    std::fs::write(store.choices_path(), r#"{"version":9,"default":"nothing"}"#).unwrap();
    assert!(matches!(
        store.load_choices(),
        Err(StoreError::Newer { found: 9, .. })
    ));
    // A bad id in the file is refused, not loaded.
    std::fs::write(
        store.choices_path(),
        r#"{"version":1,"default":"nothing","apps":{"window:1":{"choice":"nothing"}}}"#,
    )
    .unwrap();
    assert!(store.load_choices().is_err());
    std::fs::write(store.seen_path(), "[]").unwrap();
    assert!(store.load_seen().is_err());
}

#[test]
fn the_wire_kinds_are_the_two_display_signals() {
    assert_eq!(
        RequestKind::from_wire(0),
        Some(RequestKind::DemandsAttention)
    );
    assert_eq!(RequestKind::from_wire(1), Some(RequestKind::MarkedUrgent));
    assert_eq!(RequestKind::from_wire(2), None);
}
