//! What can be asserted without a window.
//!
//! Every decision the screens draw lives in the view model, so all of it is
//! tested here: which controls appear, which are unavailable and why, what the
//! apply flow reports for each outcome, what a restore would put back, and
//! whether both locales fit their columns at 100%, 125%, and 150% scaling.

use touchpad_core::{
    Capabilities, ClickMethod, Reading, RestoreScope, RunState, ScrollFactor, Section, Sensitivity,
    SettingId, SettingValue, Support, TouchpadConfig, TouchpadStore,
};
use touchpad_platform::{
    BackendStatus, DeviceInventory, MockBackend, Roots, Session, TouchpadBackend, devices,
    mock::MockBehavior, session::lookup_from,
};

use crate::i18n::{Locale, copy};
use crate::model::{Control, Page, PointerTrace, RunKind, TouchpadModel};
use crate::{ActionLayout, action_layout, label_fits};

fn fixture(name: &str) -> Roots {
    Roots::at(format!(
        "{}/../touchpad-platform/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn session() -> Session {
    Session::from_lookup(lookup_from(&[
        ("XDG_SESSION_TYPE", "wayland"),
        ("XDG_CURRENT_DESKTOP", "zorin:GNOME"),
        ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
    ]))
}

fn model_with(capabilities: Capabilities, roots: Roots, locale: Locale) -> TouchpadModel {
    TouchpadModel::new(
        TouchpadConfig::default(),
        capabilities,
        session(),
        BackendStatus::reachable("mock", "a fake backend"),
        "mock",
        devices::enumerate(&roots),
        locale,
    )
}

fn model() -> TouchpadModel {
    model_with(
        Capabilities::everything_immediate(),
        fixture("one-touchpad"),
        Locale::EnUs,
    )
}

fn sensitivity(value: f64) -> SettingValue {
    SettingValue::sensitivity(Sensitivity::new(value).unwrap())
}

#[test]
fn every_screen_name_parses_back_to_itself_and_an_unknown_one_does_not() {
    for page in Page::ALL {
        assert_eq!(Page::parse(page.key()), Some(page));
    }
    assert_eq!(Page::parse("telepathy"), None);
    // The headless launch smoke opens this one by name.
    assert_eq!(Page::parse("gestures"), Some(Page::Gestures));
}

#[test]
fn every_setting_appears_on_exactly_one_screen() {
    let model = model();
    let mut seen: Vec<SettingId> = Page::ALL
        .into_iter()
        .filter_map(Page::section)
        .flat_map(|section| {
            model
                .rows(section)
                .into_iter()
                .map(|row| row.setting)
                .collect::<Vec<_>>()
        })
        .collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), SettingId::ALL.len());
}

#[test]
fn a_control_the_backend_cannot_own_shows_the_reason_and_no_switch() {
    let capabilities = Capabilities::everything_immediate().with(
        SettingId::VerticalScrollFactor,
        Support::unavailable(
            "gnome.no_scroll_factor_key",
            "GNOME's touchpad settings have no scroll-factor key",
        ),
    );
    let model = model_with(capabilities, fixture("one-touchpad"), Locale::EnUs);
    let row = model
        .rows(Section::Scrolling)
        .into_iter()
        .find(|row| row.setting == SettingId::VerticalScrollFactor)
        .unwrap();

    assert!(!row.available);
    assert_eq!(
        row.unavailable_detail.as_deref(),
        Some("GNOME's touchpad settings have no scroll-factor key")
    );
    // The row still knows what shape it would have been, so nothing downstream
    // has to guess — but the renderer draws the explanation instead.
    assert!(matches!(row.control, Control::Slider { .. }));
}

#[test]
fn a_row_shows_the_requested_and_the_effective_value_separately() {
    let mut model = model();
    let mut backend = MockBackend::new().behaving(
        SettingId::PointerSensitivity,
        MockBehavior::StoresInstead(sensitivity(0.7)),
    );
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();
    model.apply(&mut backend, None, 0);

    let row = model
        .rows(Section::Pointer)
        .into_iter()
        .find(|row| row.setting == SettingId::PointerSensitivity)
        .unwrap();
    assert_eq!(row.requested_label, "90%");
    assert_eq!(row.effective_label, "70%");
}

#[test]
fn staging_a_linked_scroll_factor_moves_both_rows() {
    let mut model = model();
    model
        .stage(
            SettingId::VerticalScrollFactor,
            SettingValue::factor(ScrollFactor::new(2.0).unwrap()),
        )
        .unwrap();
    let rows = model.rows(Section::Scrolling);
    let vertical = rows
        .iter()
        .find(|row| row.setting == SettingId::VerticalScrollFactor)
        .unwrap();
    let horizontal = rows
        .iter()
        .find(|row| row.setting == SettingId::HorizontalScrollFactor)
        .unwrap();
    assert!(vertical.pending && horizontal.pending);
    assert_eq!(vertical.requested_label, horizontal.requested_label);
}

#[test]
fn unlinking_the_axes_lets_one_move_without_the_other() {
    let mut model = model();
    model.stage_linked_axes(false);
    model
        .stage(
            SettingId::VerticalScrollFactor,
            SettingValue::factor(ScrollFactor::new(2.0).unwrap()),
        )
        .unwrap();
    let rows = model.rows(Section::Scrolling);
    assert!(
        !rows
            .iter()
            .find(|row| row.setting == SettingId::HorizontalScrollFactor)
            .unwrap()
            .pending
    );
}

#[test]
fn each_apply_outcome_reaches_its_own_banner() {
    let cases: Vec<(MockBehavior, Option<Support>, RunState)> = vec![
        (MockBehavior::Honest, None, RunState::Applied),
        (
            MockBehavior::Honest,
            Some(Support::sign_out_required()),
            RunState::AwaitingSignOut,
        ),
        (
            MockBehavior::StoresInstead(sensitivity(0.6)),
            None,
            RunState::PartiallySupported,
        ),
        (
            MockBehavior::RefusesWrite {
                reason: "mock.refused".to_string(),
                detail: "the service said no".to_string(),
            },
            None,
            RunState::Failed,
        ),
    ];

    for (behavior, support, expected) in cases {
        let mut capabilities = Capabilities::everything_immediate();
        if let Some(support) = support.clone() {
            capabilities.insert(SettingId::PointerSensitivity, support);
        }
        let mut model = model_with(capabilities.clone(), fixture("one-touchpad"), Locale::EnUs);
        let mut backend = MockBackend::with_capabilities(capabilities)
            .behaving(SettingId::PointerSensitivity, behavior.clone());
        model
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();

        let state = model.apply(&mut backend, None, 0);
        assert_eq!(state, expected, "{behavior:?} produced {state:?}");
        let (banner_state, text) = model.result_banner().expect("a run always has a banner");
        assert_eq!(banner_state, expected);
        assert!(!text.is_empty());
    }
}

#[test]
fn a_run_that_changed_nothing_says_so_rather_than_claiming_success() {
    let mut model = model();
    let mut backend = MockBackend::new();
    model.refresh(&backend);
    let state = model.apply(&mut backend, None, 0);
    assert_eq!(state, RunState::NothingToDo);
    assert_eq!(
        model.result_banner().unwrap().1,
        copy(Locale::EnUs).result_nothing
    );
}

#[test]
fn applying_captures_the_previous_values_before_it_writes_anything() {
    let mut model = model();
    let mut backend = MockBackend::new().holding(SettingId::PointerSensitivity, sensitivity(0.2));
    model.refresh(&backend);
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();
    model.apply(&mut backend, None, 100);

    let row = model
        .rows(Section::Pointer)
        .into_iter()
        .find(|row| row.setting == SettingId::PointerSensitivity)
        .unwrap();
    assert_eq!(row.previous_label.as_deref(), Some("20%"));
}

#[test]
fn the_restore_review_shows_the_captured_values_before_anything_is_put_back() {
    let mut model = model();
    let mut backend = MockBackend::new().holding(SettingId::PointerSensitivity, sensitivity(0.2));
    model.refresh(&backend);
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();
    model.apply(&mut backend, None, 100);

    let rows = model.restore_rows(RestoreScope::All);
    let row = rows
        .iter()
        .find(|row| row.setting == SettingId::PointerSensitivity)
        .expect("the changed setting is in the review");
    assert_eq!(row.captured_label, "20%");
    assert!(row.actionable);
}

#[test]
fn restoring_puts_the_captured_value_back_and_verifies_it() {
    let mut model = model();
    let mut backend = MockBackend::new().holding(SettingId::PointerSensitivity, sensitivity(0.2));
    model.refresh(&backend);
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();
    model.apply(&mut backend, None, 100);
    assert_eq!(
        backend.read_one(SettingId::PointerSensitivity),
        Reading::value(sensitivity(0.9))
    );

    let state = model.restore(&mut backend, RestoreScope::All, None);
    assert_eq!(state, Some(RunState::Applied));
    assert_eq!(
        backend.read_one(SettingId::PointerSensitivity),
        Reading::value(sensitivity(0.2))
    );
    assert_eq!(
        model.result_banner().unwrap().1,
        copy(Locale::EnUs).restored
    );
    assert_eq!(model.last_run().unwrap().0, RunKind::Restore);
}

#[test]
fn restoring_one_section_leaves_the_others_alone() {
    let mut model = model();
    let mut backend = MockBackend::new()
        .holding(SettingId::PointerSensitivity, sensitivity(0.2))
        .holding(SettingId::TapToClick, SettingValue::toggle(true));
    model.refresh(&backend);
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();
    model
        .stage(SettingId::TapToClick, SettingValue::toggle(false))
        .unwrap();
    model.apply(&mut backend, None, 100);
    assert_eq!(
        backend.read_one(SettingId::TapToClick),
        Reading::value(SettingValue::toggle(false))
    );

    model.restore(
        &mut backend,
        RestoreScope::Section {
            section: Section::Pointer,
        },
        None,
    );
    assert_eq!(
        backend.read_one(SettingId::PointerSensitivity),
        Reading::value(sensitivity(0.2))
    );
    // The clicking section was not in the scope, so it still holds what the
    // apply left there rather than what was captured.
    assert_eq!(
        backend.read_one(SettingId::TapToClick),
        Reading::value(SettingValue::toggle(false))
    );
}

#[test]
fn there_is_nothing_to_restore_before_anything_has_been_changed() {
    let model = model();
    assert!(model.restore_rows(RestoreScope::All).is_empty());
}

#[test]
fn safe_mode_stops_a_write_and_says_why() {
    let mut model = model();
    let mut backend = MockBackend::new();
    model.refresh(&backend);
    model.set_safe_mode(true);
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();

    let state = model.apply(&mut backend, None, 0);
    assert_eq!(state, RunState::NothingToDo);
    assert!(backend.writes.is_empty());
    assert!(
        model
            .health()
            .check("touchpad.safe_mode")
            .is_some_and(|check| check.state == touchpad_core::HealthState::Degraded)
    );
}

#[test]
fn the_overview_reports_the_session_the_device_and_what_is_unavailable() {
    let capabilities = Capabilities::everything_immediate().with(
        SettingId::SmoothScrolling,
        Support::unavailable("gnome.no_smooth_scroll_key", "no such key"),
    );
    let model = model_with(capabilities, fixture("one-touchpad"), Locale::EnUs);
    let overview = model.overview();

    assert!(overview.device.contains("ASCF1200"));
    assert_eq!(overview.session, "wayland / zorin, gnome");
    assert_eq!(overview.backend, "mock");
    assert_eq!(overview.unavailable_count, 1);
    assert_eq!(overview.pending_count, 0);
}

#[test]
fn a_machine_with_no_touchpad_says_so_rather_than_showing_an_empty_screen() {
    let model = model_with(
        Capabilities::everything_immediate(),
        Roots::at("/nonexistent"),
        Locale::EnUs,
    );
    assert_eq!(model.overview().device, copy(Locale::EnUs).no_devices);
    assert_eq!(
        model.health().check("touchpad.device").unwrap().state,
        touchpad_core::HealthState::Failed
    );
}

#[test]
fn a_setting_never_read_is_shown_as_not_read_rather_than_as_a_value() {
    let model = model();
    let row = model
        .rows(Section::Pointer)
        .into_iter()
        .find(|row| row.setting == SettingId::PointerSensitivity)
        .unwrap();
    assert_eq!(row.effective_label, copy(Locale::EnUs).value_not_read);
}

#[test]
fn a_key_with_no_user_value_reads_as_the_session_default_in_both_locales() {
    let mut model = model();
    let backend = MockBackend::new();
    model.refresh(&backend);
    assert_eq!(
        model.rows(Section::Clicking)[0].effective_label,
        copy(Locale::EnUs).value_session_default
    );
    model.set_locale(Locale::ZhTw);
    assert_eq!(
        model.rows(Section::Clicking)[0].effective_label,
        copy(Locale::ZhTw).value_session_default
    );
}

#[test]
fn a_change_survives_a_restart_of_the_application() {
    let directory = tempfile::tempdir().unwrap();
    let store = TouchpadStore::new(directory.path().join("touchpad"));
    let mut model = model();
    let mut backend = MockBackend::new();
    model.refresh(&backend);
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();
    model.apply(&mut backend, Some(&store), 100);

    let reopened = store.load_config().unwrap();
    assert_eq!(reopened.pointer.sensitivity.get(), 0.9);
    assert!(store.load_backup().unwrap().is_some());
}

#[test]
fn the_capture_written_before_the_first_change_is_never_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let store = TouchpadStore::new(directory.path().join("touchpad"));
    let mut model = model();
    let mut backend = MockBackend::new().holding(SettingId::PointerSensitivity, sensitivity(0.2));
    model.refresh(&backend);

    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.9))
        .unwrap();
    model.apply(&mut backend, Some(&store), 100);
    model
        .stage(SettingId::PointerSensitivity, sensitivity(0.3))
        .unwrap();
    model.apply(&mut backend, Some(&store), 200);

    let backup = store.load_backup().unwrap().unwrap();
    assert_eq!(
        backup.reading(SettingId::PointerSensitivity),
        Some(&Reading::value(sensitivity(0.2)))
    );
}

#[test]
fn the_pointer_test_surface_reports_a_fraction_and_never_leaves_its_box() {
    assert_eq!(
        PointerTrace::at(50.0, 25.0, 100.0, 50.0),
        PointerTrace {
            x: 0.5,
            y: 0.5,
            inside: true
        }
    );
    let outside = PointerTrace::at(-10.0, 80.0, 100.0, 50.0);
    assert!(!outside.inside);
    assert_eq!(outside.x, 0.0);
    assert_eq!(outside.y, 1.0);
    // A surface with no size yet cannot be divided by.
    assert!(!PointerTrace::at(10.0, 10.0, 0.0, 0.0).inside);
    assert!(!PointerTrace::idle().inside);
}

#[test]
fn every_navigation_label_fits_its_sidebar_column_in_both_locales_at_every_scale() {
    // The sidebar is 232px wide with an icon and padding, leaving 150px for the
    // label itself.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for page in Page::ALL {
            for scale in [1.0, 1.25, 1.5] {
                assert!(
                    label_fits(page.label(c), 150.0 * scale, scale),
                    "{} does not fit at {scale}x in {}",
                    page.label(c),
                    locale.tag()
                );
            }
        }
    }
}

#[test]
fn every_setting_label_fits_its_row_in_both_locales_at_every_scale() {
    // A settings row gives the label the full content width less the value
    // column; 320 logical pixels is the narrowest it gets in a compact window.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for setting in SettingId::ALL {
            let label = crate::model::label_for(setting, c);
            for scale in [1.0, 1.25, 1.5] {
                assert!(
                    label_fits(label, 320.0 * scale, scale),
                    "{label} does not fit at {scale}x in {}",
                    locale.tag()
                );
            }
        }
    }
}

#[test]
fn the_action_row_fits_the_default_window_at_every_scale_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let labels = [c.switch_language, c.refresh, c.discard, c.apply];
        for scale in [1.0, 1.25, 1.5] {
            assert_eq!(
                action_layout(DEFAULT_WINDOW_WIDTH, scale, &labels),
                ActionLayout::Inline,
                "{} does not fit at {scale}x",
                locale.tag()
            );
        }
        // At the minimum window width it still fits unscaled.
        assert_eq!(
            action_layout(crate::MIN_WINDOW_WIDTH, 1.0, &labels),
            ActionLayout::Inline,
            "{} does not fit the minimum window",
            locale.tag()
        );
    }
}

#[test]
fn the_action_row_wraps_rather_than_pushing_a_button_out_of_view() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let labels = [c.switch_language, c.refresh, c.discard, c.apply];
        // Narrow enough that the four buttons genuinely cannot share a line.
        assert_eq!(
            action_layout(640.0, 1.5, &labels),
            ActionLayout::Wrapped,
            "{} claimed four buttons fit in 427 logical pixels",
            locale.tag()
        );
    }
}

/// The size the window opens at.
const DEFAULT_WINDOW_WIDTH: f32 = 1040.0;

#[test]
fn switching_language_changes_every_visible_string() {
    let mut model = model();
    let english: Vec<&'static str> = model
        .rows(Section::Clicking)
        .iter()
        .map(|row| row.label)
        .collect();
    model.set_locale(Locale::ZhTw);
    let chinese: Vec<&'static str> = model
        .rows(Section::Clicking)
        .iter()
        .map(|row| row.label)
        .collect();
    assert_eq!(english.len(), chinese.len());
    assert!(english.iter().zip(chinese.iter()).all(|(a, b)| a != b));
}

#[test]
fn the_fixed_chinese_terms_are_the_ones_the_screens_show() {
    let mut model = model();
    model.set_locale(Locale::ZhTw);
    let labels: Vec<&'static str> = model.all_rows().iter().map(|row| row.label).collect();
    assert!(labels.contains(&"游標靈敏度"));
    assert!(labels.contains(&"自然捲動"));
    assert!(labels.contains(&"點按來按一下"));
    assert_eq!(copy(Locale::ZhTw).scroll_sensitivity, "捲動靈敏度");
}

#[test]
fn a_device_that_cannot_do_something_is_reported_from_the_hardware_not_the_backend() {
    let inventory: DeviceInventory = devices::enumerate(&fixture("semi-mt"));
    let limits = inventory.devices[0].capabilities.limits();
    assert!(
        limits
            .iter()
            .any(|(setting, _, _)| *setting == SettingId::TwoFingerScrolling)
    );
}

#[test]
fn the_shipped_source_builds_no_backend_command() {
    // ADR 0005 and Issue #3 both require that the GUI never runs a
    // backend-specific command. Asserting it over this crate's own source is
    // cheaper than reviewing every future change to it.
    let source_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // `std::process::exit` is fine and `--safe-mode` uses it; running a
    // command is what must never happen.
    let forbidden = [
        "gsettings",
        "xinput",
        "dconf ",
        "Command::new",
        "process::Command",
    ];
    for entry in std::fs::read_dir(&source_directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        // This file is `#[cfg(test)]` and names the forbidden tokens on
        // purpose; it is not shipped source.
        if path.file_name().and_then(|name| name.to_str()) == Some("tests.rs") {
            continue;
        }
        // Comments are allowed to name what the code must not do; that is how
        // the rule is explained.
        let text: String = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{} names {needle}, which the GUI must never do",
                path.display()
            );
        }
    }
}

#[test]
fn the_control_for_a_setting_follows_its_value_kind_and_nothing_else() {
    for setting in SettingId::ALL {
        let control = crate::model::control_for(setting);
        match setting.kind() {
            touchpad_core::ValueKind::Sensitivity | touchpad_core::ValueKind::Factor => {
                assert!(matches!(control, Control::Slider { .. }), "{setting}")
            }
            touchpad_core::ValueKind::Toggle => {
                assert_eq!(control, Control::Switch, "{setting}")
            }
            touchpad_core::ValueKind::Acceleration | touchpad_core::ValueKind::Click => {
                assert_eq!(control, Control::Choice, "{setting}")
            }
        }
    }
}

#[test]
fn a_slider_range_is_the_supported_range_so_an_impossible_value_cannot_be_dragged_to() {
    let Control::Slider { min, max, .. } = crate::model::control_for(SettingId::PointerSensitivity)
    else {
        panic!("sensitivity is a slider");
    };
    assert_eq!(min, Sensitivity::MIN);
    assert_eq!(max, Sensitivity::MAX);
    assert!(Sensitivity::new(max + 0.01).is_err());

    let Control::Slider { min, max, .. } =
        crate::model::control_for(SettingId::VerticalScrollFactor)
    else {
        panic!("a scroll factor is a slider");
    };
    assert_eq!(min, ScrollFactor::MIN);
    assert_eq!(max, ScrollFactor::MAX);
}

#[test]
fn a_click_method_choice_offers_every_method_and_no_others() {
    assert_eq!(ClickMethod::ALL.len(), 4);
    let model = model();
    let row = model
        .rows(Section::Clicking)
        .into_iter()
        .find(|row| row.setting == SettingId::ClickMethod)
        .unwrap();
    assert_eq!(row.control, Control::Choice);
    assert_eq!(row.requested_label, copy(Locale::EnUs).method_default);
}
