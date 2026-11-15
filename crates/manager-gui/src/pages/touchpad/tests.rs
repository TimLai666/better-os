//! The Touchpad page's own decisions, asserted without a window. The
//! controls' decisions are `touchpad-views`' and are tested there, with the
//! capture-before-first-change and per-section restore tests the older
//! window had.

use touchpad_core::{Capabilities, HealthState, SettingId, TouchpadConfig};
use touchpad_platform::{BackendStatus, Roots, Session, devices, session::lookup_from};
use touchpad_session::AdapterDescription;
use touchpad_views::TouchpadModel;

use super::copy::{event_words, test_reach};
use super::view::*;
use super::{InFlight, TouchpadView};
use crate::attention::{AttentionAction, Severity, Snapshot};
use crate::i18n::Locale;
use crate::pages::states::PageState;
use crate::search;

fn fixture(name: &str) -> Roots {
    Roots::at(format!(
        "{}/../touchpad-platform/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn session(desktop: &str) -> Session {
    Session::from_lookup(lookup_from(&[
        ("XDG_SESSION_TYPE", "wayland"),
        ("XDG_CURRENT_DESKTOP", desktop),
        ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
    ]))
}

fn model(locale: Locale, roots: Roots) -> TouchpadModel {
    let mut model = TouchpadModel::new(
        TouchpadConfig::default(),
        Capabilities::everything_immediate(),
        session("zorin:GNOME"),
        BackendStatus::reachable("gnome", "a fake backend"),
        "gnome",
        devices::enumerate(&roots),
        views_locale(locale),
    );
    model.refresh(&touchpad_platform::MockBackend::new());
    model
}

#[test]
fn the_selector_bar_is_the_six_sub_pages_in_the_spec_s_order_and_words() {
    let zh: Vec<&str> = SubPage::ALL
        .iter()
        .map(|page| page.label(Locale::ZhTw))
        .collect();
    assert_eq!(zh, ["游標", "捲動", "點按", "手勢", "裝置", "診斷"]);
    let en: Vec<&str> = SubPage::ALL
        .iter()
        .map(|page| page.label(Locale::EnUs))
        .collect();
    assert_eq!(
        en,
        [
            "Pointer",
            "Scrolling",
            "Clicking",
            "Gestures",
            "Devices",
            "Diagnostics"
        ]
    );
    for page in SubPage::ALL {
        assert_eq!(SubPage::from_key(page.key()), page);
        assert_eq!(SubPage::ALL[page.index()], page);
    }
    // `--page touchpad` and today's Overview open Pointer.
    assert_eq!(SubPage::from_key("overview"), SubPage::Pointer);
    assert_eq!(SubPage::from_key("nonsense"), SubPage::Pointer);
}

#[test]
fn not_installed_and_too_old_are_the_shared_states() {
    assert_eq!(
        availability(None),
        Availability::Blocked(PageState::NotInstalled {
            component: "Better Touchpad".to_string()
        })
    );
    assert_eq!(
        availability(Some("0.2.0")),
        Availability::Blocked(PageState::TooOld {
            component: "Better Touchpad".to_string(),
            installed: "0.2.0".to_string(),
            required: REQUIRED_VERSION.to_string(),
        })
    );
    assert_eq!(availability(Some("0.2.1")), Availability::Ready);
    assert_eq!(availability(Some("0.2.15")), Availability::Ready);
    // A version semver cannot order is not judged too old.
    assert_eq!(availability(Some("0.2.8~rc1")), Availability::Ready);
    let copy = crate::pages::states::describe(
        Locale::ZhTw,
        &PageState::NotInstalled {
            component: COMPONENT_NAME.to_string(),
        },
    );
    assert_eq!(copy.text, "需要安裝 Better Touchpad 才能使用這一頁。");
    assert_eq!(copy.action, Some("安裝"));
}

#[test]
fn not_gnome_and_safe_mode_are_bars_in_that_order() {
    assert!(notices(&session("zorin:GNOME"), false).is_empty());
    assert_eq!(notices(&session("KDE"), false), vec![Notice::NotGnome]);
    assert_eq!(
        notices(&session("KDE"), true),
        vec![Notice::NotGnome, Notice::SafeMode]
    );
    assert_eq!(notices(&session("GNOME"), true), vec![Notice::SafeMode]);
    assert_eq!(
        super::copy::page_copy(Locale::ZhTw).not_gnome,
        "這些設定目前只支援 GNOME 桌面。"
    );
    let c = touchpad_views::i18n::copy(touchpad_views::Locale::ZhTw);
    assert_eq!(
        c.safe_mode_banner,
        "安全模式已開啟。只會顯示設定，不會做任何變更。"
    );
    assert_eq!(c.safe_mode_off, "關閉安全模式");
    assert_eq!(c.no_devices, "這台電腦沒有找到觸控板。");
    let e = touchpad_views::i18n::copy(touchpad_views::Locale::EnUs);
    assert_eq!(e.no_devices, "No touchpad was found on this PC.");
}

#[test]
fn the_apply_bar_counts_what_is_staged_and_is_gone_when_nothing_is() {
    assert_eq!(apply_bar_text(Locale::ZhTw, 0), None);
    assert_eq!(
        apply_bar_text(Locale::ZhTw, 3).as_deref(),
        Some("有 3 項變更還沒套用。")
    );
    assert_eq!(
        apply_bar_text(Locale::EnUs, 3).as_deref(),
        Some("3 changes haven't been applied.")
    );
    assert_eq!(
        apply_bar_text(Locale::EnUs, 1).as_deref(),
        Some("1 change hasn't been applied.")
    );
    let mut model = model(Locale::EnUs, fixture("one-touchpad"));
    assert_eq!(staged_count(&model), 0);
    model
        .stage(
            SettingId::TapToClick,
            touchpad_core::SettingValue::toggle(false),
        )
        .unwrap();
    assert_eq!(staged_count(&model), 1);
    model.discard();
    assert_eq!(staged_count(&model), 0);
}

#[test]
fn this_touchpad_says_everything_in_words_and_no_identifier() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let model = model(locale, fixture("one-touchpad"));
        let facts = this_touchpad(&model, locale);
        let device = model.selected_device().unwrap();
        assert_eq!(facts.device, friendly_name(&device.name));
        // Audit T4: no vendor, product, or bus on this card.
        for text in [&facts.device, &facts.session] {
            assert!(!text.contains(&device.vendor), "{text}");
            assert!(!text.contains(&device.identity), "{text}");
        }
        // The health is a word in this language, never the enum's name.
        assert!(!["Ok", "Degraded", "Failed"].contains(&facts.health));
        assert_eq!(facts.session, "GNOME · Wayland");
    }
    let model = model(Locale::ZhTw, fixture("one-touchpad"));
    let facts = this_touchpad(&model, Locale::ZhTw);
    assert_eq!(facts.backend, "GNOME 設定");
    assert_eq!(
        touchpad_views::i18n::health_words(HealthState::Degraded, touchpad_views::Locale::ZhTw),
        "部分功能無法使用"
    );
    assert_eq!(
        touchpad_views::i18n::health_words(HealthState::Degraded, touchpad_views::Locale::EnUs),
        "Partly unavailable"
    );
}

#[test]
fn a_touchpad_s_name_loses_its_identifiers() {
    assert_eq!(friendly_name("ASCF1200:00 2808:0233 Touchpad"), "Touchpad");
    assert_eq!(
        friendly_name("SynPS/2 Synaptics TouchPad"),
        "SynPS/2 Synaptics TouchPad"
    );
    assert_eq!(friendly_name("ELAN0001:00"), "ELAN0001:00");
}

#[test]
fn no_touchpad_is_named_on_the_card() {
    let empty = tempfile::tempdir().unwrap();
    let model = model(Locale::ZhTw, Roots::at(empty.path()));
    assert!(model.devices().is_empty());
    assert_eq!(this_touchpad(&model, Locale::ZhTw).device, "沒有觸控板");
}

/// Audit T1: the adapter's own name never reaches the screen.
#[test]
fn the_gesture_route_is_described_in_words() {
    let describe = |name: &str, performs| AdapterDescription {
        name: name.to_string(),
        continuous_progress: false,
        performs_system_actions: performs,
    };
    for locale in [Locale::EnUs, Locale::ZhTw] {
        for description in [
            describe("mock", false),
            describe("gnome-shell (46.0)", true),
            describe("launcher-activation (io.betteros.Launcher)", true),
        ] {
            let words = adapter_words(&description, locale);
            assert!(!words.contains("mock"));
            assert!(!words.contains("gnome-shell"));
        }
    }
    assert_eq!(
        adapter_words(&describe("mock", false), Locale::ZhTw),
        "沒有連上桌面"
    );
    assert_eq!(
        adapter_words(
            &describe("launcher-activation (x) + gnome-shell (46.0)", true),
            Locale::EnUs
        ),
        "Connected to the desktop"
    );
    assert_eq!(
        backend_words("mock", true, Locale::EnUs),
        "Demo: nothing is written"
    );
}

#[test]
fn the_audit_s_copy_changes_are_in() {
    let zh = touchpad_views::i18n::copy(touchpad_views::Locale::ZhTw);
    let en = touchpad_views::i18n::copy(touchpad_views::Locale::EnUs);
    assert_eq!(
        (zh.requested_value, zh.effective_value),
        ("你選的", "目前生效")
    );
    assert_eq!(
        (en.requested_value, en.effective_value),
        ("You chose", "In effect")
    );
    assert_eq!(zh.tap_to_click, "輕點即按一下");
    assert_eq!(en.tap_to_click, "Tap to click");
    assert_eq!(zh.pending_badge, "尚未套用");
    assert_eq!(en.pending_badge, "Not applied yet");
    assert_eq!(zh.apply, "套用");
    assert_eq!(zh.discard, "捨棄變更");
    assert_eq!(en.apply, "Apply");
    assert_eq!(en.discard, "Discard");
}

/// Audit T2: an unavailable control's reason is in this language.
#[test]
fn an_unavailable_control_says_why_in_this_language() {
    let mut capabilities = Capabilities::everything_immediate();
    capabilities.insert(
        SettingId::VerticalScrollFactor,
        touchpad_core::Support::unavailable(
            "gnome.no_scroll_factor_key",
            "GNOME's touchpad settings have no scroll-factor key",
        ),
    );
    let model = TouchpadModel::new(
        TouchpadConfig::default(),
        capabilities,
        session("GNOME"),
        BackendStatus::reachable("gnome", "x"),
        "gnome",
        devices::enumerate(&fixture("one-touchpad")),
        touchpad_views::Locale::ZhTw,
    );
    let row = model
        .all_rows()
        .into_iter()
        .find(|row| row.setting == SettingId::VerticalScrollFactor)
        .unwrap();
    assert!(!row.available);
    assert_eq!(
        row.unavailable_reason.as_deref(),
        Some("GNOME 沒有這個設定，捲動距離會跟著游標速度。")
    );
}

#[test]
fn every_card_is_findable_and_opens_its_own_sub_page() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let entries = search_entries(locale);
        for setting in SettingId::ALL {
            assert!(
                entries.iter().any(|entry| entry.card == setting.key()),
                "{setting:?}"
            );
        }
        for entry in &entries {
            if entry.card != card::THIS_TOUCHPAD {
                assert!(SubPage::of_card(entry.card).is_some(), "{entry:?}");
                assert_eq!(
                    entry.group,
                    Some(SubPage::of_card(entry.card).unwrap().label(locale))
                );
            }
        }
    }
    assert_eq!(
        SubPage::of_card(SettingId::TapToClick.key()),
        Some(SubPage::Clicking)
    );
    assert_eq!(
        SubPage::of_card(card::SAFE_MODE),
        Some(SubPage::Diagnostics)
    );
    assert_eq!(
        SubPage::of_card(card::GESTURE_FILE),
        Some(SubPage::Gestures)
    );
    // "捲動" finds the scrolling cards; "natural" finds natural scrolling.
    let zh = search_entries(Locale::ZhTw);
    let found = search::find(&zh, "自然");
    assert_eq!(found[0].card, SettingId::NaturalScrolling.key());
    let en = search_entries(Locale::EnUs);
    let found = search::find(&en, "tap");
    assert!(
        found
            .iter()
            .any(|entry| entry.card == SettingId::TapToClick.key())
    );
}

#[test]
fn home_shows_safe_mode_and_gestures_that_turned_themselves_off() {
    let snapshot = |locale, installed: Option<&str>, safe_mode, turned_off| Snapshot {
        touchpad: TouchpadFacts {
            installed: installed.map(str::to_string),
            safe_mode,
            gestures_turned_off: turned_off,
        },
        ..Snapshot::empty(locale)
    };
    assert!(attention(&snapshot(Locale::EnUs, Some("0.2.15"), false, false)).is_empty());
    // Not installed: nothing to say, whatever stale files are around.
    assert!(attention(&snapshot(Locale::EnUs, None, true, true)).is_empty());

    let items = attention(&snapshot(Locale::ZhTw, Some("0.2.15"), true, false));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].header, "觸控板安全模式已開啟");
    assert_eq!(
        items[0].description,
        "Better OS 不會變更觸控板設定，也不會處理手勢。"
    );
    assert_eq!(items[0].action_label, "關閉安全模式");
    assert_eq!(items[0].action, AttentionAction::TurnOffTouchpadSafeMode);
    assert_eq!(items[0].severity, Severity::Warning);

    let items = attention(&snapshot(Locale::EnUs, Some("0.2.15"), false, true));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].header, "Gestures turned themselves off");
    assert_eq!(
        items[0].description,
        "Three gestures in a row didn't work, so they were turned off."
    );
    assert_eq!(items[0].action_label, "View");
    assert_eq!(
        items[0].action,
        AttentionAction::OpenTouchpad(SubPage::Gestures)
    );

    // Safe mode already says gestures are off: one card, not two.
    assert_eq!(
        attention(&snapshot(Locale::EnUs, Some("0.2.15"), true, true)).len(),
        1
    );
    // And Home orders them with everything else.
    let all = crate::pages::attention(&snapshot(Locale::EnUs, Some("0.2.15"), true, false));
    assert!(
        all.iter()
            .any(|item| item.action == AttentionAction::TurnOffTouchpadSafeMode)
    );
}

#[test]
fn the_markers_are_read_from_the_store_directory() {
    let directory = tempfile::tempdir().unwrap();
    let mut facts = TouchpadFacts::default();
    facts.read_markers(directory.path());
    assert!(!facts.safe_mode && !facts.gestures_turned_off);
    touchpad_core::TouchpadStore::new(directory.path())
        .enable_safe_mode()
        .unwrap();
    touchpad_gestures::GestureStore::at(directory.path())
        .record_turned_off(3)
        .unwrap();
    facts.read_markers(directory.path());
    assert!(facts.safe_mode && facts.gestures_turned_off);
}

#[test]
fn a_restart_waits_for_what_has_no_saved_form() {
    use better_ui::restart::Blocker;
    assert_eq!(InFlight::default().blocker(), None);
    let one = |set: fn(&mut InFlight)| {
        let mut flight = InFlight::default();
        set(&mut flight);
        flight.blocker()
    };
    assert_eq!(one(|f| f.applying = true), Some(Blocker::Task));
    assert_eq!(one(|f| f.live_test = true), Some(Blocker::Task));
    assert_eq!(one(|f| f.editor = true), Some(Blocker::Dialog));
    assert_eq!(one(|f| f.choosing_file = true), Some(Blocker::Dialog));
    assert_eq!(
        one(|f| f.unapplied_settings = true),
        Some(Blocker::UnsavedChanges)
    );
    assert_eq!(one(|f| f.preview = true), Some(Blocker::UnsavedChanges));
    assert_eq!(one(|f| f.import = true), Some(Blocker::UnsavedChanges));

    let view = TouchpadView {
        sub_page: "gestures".to_string(),
        gesture_device: Some("usb:1:2".to_string()),
        export_path: Some("/tmp/a.json".to_string()),
        import_path: None,
    };
    let read: TouchpadView = serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
    assert_eq!(read, view);
    assert_eq!(SubPage::from_key(&read.sub_page), SubPage::Gestures);
    // An older hand-off without the field still reads.
    let old: crate::restart::ManagerView = serde_json::from_str("{}").unwrap();
    assert_eq!(old.touchpad, None);
}

#[test]
fn a_gesture_test_says_whether_it_reached_the_desktop() {
    assert_eq!(
        test_reach(Locale::ZhTw, false, 0, 0),
        "只重播了辨識過程，沒有送到桌面。"
    );
    assert_eq!(
        test_reach(Locale::EnUs, true, 4, 4),
        "It reached the desktop."
    );
    assert_eq!(
        test_reach(Locale::ZhTw, true, 4, 1),
        "有 3 個動作沒有送到桌面。"
    );
    assert_eq!(
        event_words("threshold-crossed", Locale::ZhTw),
        "超過觸發門檻"
    );
    assert_eq!(event_words("complete", Locale::EnUs), "Completed");
}

/// ADR 0005 and Issue #3: the page never runs a backend-specific command.
/// Asserted over this directory's own source, as the older window's test
/// does over its crate.
#[test]
fn the_page_builds_no_backend_command() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pages/touchpad");
    let forbidden = [
        "gsettings",
        "xinput",
        "dconf ",
        "Command::new",
        "process::Command",
    ];
    let mut read = 0;
    for entry in std::fs::read_dir(&directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path.file_name().and_then(|name| name.to_str()) == Some("tests.rs")
        {
            continue;
        }
        read += 1;
        let text: String = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{} names {needle}, which the page must never do",
                path.display()
            );
        }
    }
    assert!(read >= 6, "the scan found {read} files");
}

/// The demo's in-memory page stages a change the way the window does.
#[test]
fn the_in_memory_page_stages_a_toggle() {
    let directory = tempfile::tempdir().unwrap();
    let startup = touchpad_views::Startup::run(touchpad_views::StartupOptions {
        roots: fixture("one-touchpad"),
        store_directory: directory.path().to_path_buf(),
        locale: touchpad_views::Locale::EnUs,
        offline: false,
        in_memory: true,
        page: touchpad_views::Page::Pointer,
    });
    let mut model = startup.model;
    let row = model
        .all_rows()
        .into_iter()
        .find(|row| row.setting == SettingId::NaturalScrolling)
        .unwrap();
    assert!(row.available, "{row:?}");
    let result = model.stage(
        SettingId::NaturalScrolling,
        touchpad_core::SettingValue::toggle(!row.requested.as_bool().unwrap()),
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(staged_count(&model), 1);
}
