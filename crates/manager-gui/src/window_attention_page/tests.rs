use window_attention_core::{AppId, Choice, Choices, SeenApps};

use super::{
    AppRow, ExtensionInfo, ExtensionState, InstalledApp, NoticeAction, ServiceState, add,
    app_choice_at, app_index, app_items, default_index, default_items, extension_notice,
    extension_state, pick, rows, service_notice, suggestions,
};
use crate::i18n::Locale;

fn app(id: &str) -> AppId {
    AppId::new(id).unwrap()
}

fn installed(id: &str, name: &str) -> InstalledApp {
    InstalledApp {
        id: app(id),
        name: name.to_string(),
        icon: None,
    }
}

#[test]
fn the_default_list_is_the_four_choices_in_both_languages() {
    assert_eq!(
        default_items(Locale::EnUs),
        [
            "Banner and taskbar mark",
            "Taskbar mark only",
            "Nothing",
            "Bring the window forward"
        ]
    );
    assert_eq!(
        default_items(Locale::ZhTw),
        [
            "橫幅與工作列標記",
            "只標記工作列",
            "不做任何事",
            "把視窗帶到最前面"
        ]
    );
    for (index, choice) in Choice::ALL.iter().enumerate() {
        assert_eq!(default_index(*choice), index);
    }
    // The default a fresh install starts with.
    assert_eq!(default_index(Choices::default().default), 1);
}

#[test]
fn an_app_list_starts_with_the_default_and_names_it() {
    let items = app_items(Locale::EnUs, Choice::MarkOnly);
    assert_eq!(items[0], "Use the default (Taskbar mark only)");
    assert_eq!(items.len(), 5);
    assert_eq!(
        app_items(Locale::ZhTw, Choice::Nothing)[0],
        "使用預設（不做任何事）"
    );
    assert_eq!(app_index(None), 0);
    assert_eq!(app_index(Some(Choice::BringForward)), 4);
    assert_eq!(app_choice_at(0), Some(None));
    assert_eq!(app_choice_at(3), Some(Some(Choice::Nothing)));
    assert_eq!(app_choice_at(5), None);
}

#[test]
fn the_list_holds_each_chosen_and_each_seen_app_once_by_its_menu_name() {
    let mut choices = Choices::default();
    choices.set(
        app("com.anthropic.Claude.desktop"),
        "Claude",
        Choice::Nothing,
    );
    choices.set(app("gone.desktop"), "Gone App", Choice::BringForward);
    let mut seen = SeenApps::default();
    seen.record(&app("com.anthropic.Claude.desktop"), "Claude", 1);
    seen.record(&app("org.gnome.Nautilus.desktop"), "nautilus", 2);
    seen.record(&app("nameless.desktop"), "", 3);
    let menu = [installed("org.gnome.Nautilus.desktop", "Files")];

    let listed = rows(&choices, &seen, &menu);
    let names: Vec<(&str, Option<Choice>)> = listed
        .iter()
        .map(|row| (row.name.as_str(), row.own))
        .collect();
    assert_eq!(
        names,
        [
            ("Claude", Some(Choice::Nothing)),
            ("Files", None),
            ("Gone App", Some(Choice::BringForward)),
            ("nameless.desktop", None),
        ]
    );
}

#[test]
fn picking_use_the_default_takes_the_app_s_own_choice_away() {
    let mut choices = Choices::default();
    let row = AppRow {
        app: app("com.anthropic.Claude.desktop"),
        name: "Claude".to_string(),
        own: None,
    };
    assert!(pick(&mut choices, &row, 3));
    assert_eq!(
        choices.for_app(Some(&row.app)),
        Choice::Nothing,
        "an own choice overrides the default"
    );
    let row = AppRow {
        own: Some(Choice::Nothing),
        ..row
    };
    assert!(
        !pick(&mut choices, &row, 3),
        "the same pick changes nothing"
    );
    assert!(pick(&mut choices, &row, 0));
    assert_eq!(choices.for_app(Some(&row.app)), Choice::MarkOnly);
    assert!(!pick(&mut choices, &row, 9));
}

#[test]
fn an_app_added_by_hand_starts_at_the_default() {
    let mut choices = Choices::with_default(Choice::BannerAndMark);
    add(
        &mut choices,
        &installed("org.gnome.Nautilus.desktop", "Files"),
    );
    let own = &choices.apps[&app("org.gnome.Nautilus.desktop")];
    assert_eq!(
        (own.choice, own.name.as_str()),
        (Choice::BannerAndMark, "Files")
    );
}

#[test]
fn the_add_box_suggests_installed_apps_not_yet_listed() {
    let menu = [
        installed("org.gnome.Nautilus.desktop", "Files"),
        installed("org.gnome.Settings.desktop", "Settings"),
        installed("com.anthropic.Claude.desktop", "Claude"),
        installed("firefox_firefox.desktop", "Firefox"),
    ];
    let listed = [AppRow {
        app: app("com.anthropic.Claude.desktop"),
        name: "Claude".to_string(),
        own: None,
    }];
    let names = |query| -> Vec<String> {
        suggestions(&menu, &listed, query)
            .into_iter()
            .map(|found| found.name)
            .collect()
    };
    assert!(names("").is_empty());
    assert!(names("claude").is_empty(), "already listed");
    assert_eq!(names("FI"), ["Files", "Firefox"]);
    assert_eq!(names("gnome"), ["Files", "Settings"]);
    // The name or the id, never the ".desktop" every id ends with.
    assert_eq!(names("es"), ["Files"]);
    assert!(names("desktop").is_empty());
}

#[test]
fn the_extension_state_follows_what_the_shell_reports() {
    let info = |state, enabled| ExtensionInfo {
        state,
        enabled,
        error: "TypeError: x".to_string(),
    };
    assert_eq!(
        extension_state(Some(&info(1, true)), true, true),
        ExtensionState::Active
    );
    assert_eq!(
        extension_state(Some(&info(2, false)), true, true),
        ExtensionState::Off {
            extensions_off: false
        }
    );
    // Enabled in the list, but every extension is off for the session.
    assert_eq!(
        extension_state(Some(&info(1, true)), false, true),
        ExtensionState::Off {
            extensions_off: true
        }
    );
    assert_eq!(
        extension_state(Some(&info(3, true)), true, true),
        ExtensionState::Failed("TypeError: x".to_string())
    );
    assert_eq!(
        extension_state(Some(&info(4, true)), true, true),
        ExtensionState::Unsupported
    );
    assert_eq!(
        extension_state(None, true, true),
        ExtensionState::NeedsSignOut
    );
    assert_eq!(
        extension_state(None, true, false),
        ExtensionState::NotInstalled
    );
}

/// The settings are shown only while the extension is on; otherwise the page
/// offers to turn it on, or says why it cannot.
#[test]
fn the_page_offers_to_turn_the_extension_on_and_hides_the_settings_until_then() {
    assert_eq!(
        extension_notice(Locale::EnUs, &ExtensionState::Active),
        None
    );
    let off = extension_notice(
        Locale::EnUs,
        &ExtensionState::Off {
            extensions_off: false,
        },
    )
    .unwrap();
    assert_eq!(off.title, "Turn on the Better OS shell adapter");
    assert_eq!(off.action, Some(NoticeAction::TurnOn));
    assert!(!off.settings_shown);
    let all_off = extension_notice(
        Locale::ZhTw,
        &ExtensionState::Off {
            extensions_off: true,
        },
    )
    .unwrap();
    assert!(all_off.body.contains("一併重新開啟擴充功能"));
    for state in [
        ExtensionState::Checking,
        ExtensionState::NeedsSignOut,
        ExtensionState::NotInstalled,
        ExtensionState::Failed("e".to_string()),
        ExtensionState::Unsupported,
        ExtensionState::NoShell,
    ] {
        let notice = extension_notice(Locale::EnUs, &state).unwrap();
        assert!(!notice.settings_shown, "{state:?}");
        assert_eq!(notice.action, None, "{state:?}");
    }
}

#[test]
fn a_service_that_is_not_answering_is_a_warning_with_a_way_to_start_it() {
    assert_eq!(service_notice(Locale::EnUs, &ServiceState::Checking), None);
    let healthy = ServiceState::Running {
        adapter_present: true,
        suppressed: true,
        problem: String::new(),
    };
    assert_eq!(service_notice(Locale::EnUs, &healthy), None);
    let off = service_notice(
        Locale::EnUs,
        &ServiceState::Unreachable("ServiceUnknown".to_string()),
    )
    .unwrap();
    assert_eq!(off.action, Some(NoticeAction::StartService));
    assert!(off.settings_shown);
    let problem = service_notice(
        Locale::ZhTw,
        &ServiceState::Running {
            adapter_present: true,
            suppressed: true,
            problem: "choices.json is broken".to_string(),
        },
    )
    .unwrap();
    assert_eq!(problem.title, "視窗提醒服務回報了問題");
    assert_eq!(problem.body, "choices.json is broken");
}
