//! What the Gestures screen can be asserted to do without a window.

use better_actions::{ActionCapabilities, ActionSupport, DesktopAction};
use touchpad_gestures::{
    ConflictResolution, GestureConfig, GestureId, GestureShape, GestureStore, PlanError, PresetId,
    RunState, mac_style,
};
use touchpad_session::MockSessionAdapter;

use crate::gestures_model::{
    Arrow, GestureScreen, PresetStatus, action_label, gesture_label, resolution_choices,
};
use crate::i18n::{Locale, copy};
use crate::{ActionLayout, action_layout, label_fits};

fn id(name: &str) -> GestureId {
    GestureId::new(name).unwrap()
}

fn screen() -> GestureScreen {
    GestureScreen::new(
        GestureConfig::default(),
        None,
        Box::new(MockSessionAdapter::new()),
    )
}

/// The preview, the four conflict decisions, and the confirmation — the whole
/// gate, as a screen would walk it.
fn confirmed_preview(screen: &mut GestureScreen, resolution: ConflictResolution) {
    screen.preview_preset();
    let conflicts: Vec<GestureId> = screen
        .plan()
        .unwrap()
        .conflicts
        .iter()
        .map(|conflict| conflict.gesture.clone())
        .collect();
    for gesture in conflicts {
        screen.resolve(gesture, resolution);
    }
    screen.confirm(true);
}

#[test]
fn the_screen_starts_with_nothing_bound_and_the_preset_not_applied() {
    let screen = screen();
    assert!(screen.rows(copy(Locale::EnUs)).is_empty());
    assert_eq!(screen.preset_status(), PresetStatus::NotApplied);
    let card = screen.preset_card(copy(Locale::EnUs));
    assert!(!card.can_apply);
    assert!(card.changes.is_empty());
}

#[test]
fn previewing_lists_every_change_and_every_conflict_before_anything_happens() {
    let mut screen = screen();
    screen.preview_preset();
    let card = screen.preset_card(copy(Locale::EnUs));

    assert_eq!(card.changes.len(), mac_style().gestures.len());
    assert_eq!(card.conflicts.len(), 4);
    assert!(!card.can_apply);
    // Still nothing bound: a preview changes nothing.
    assert!(screen.config().gestures.is_empty());
}

#[test]
fn nothing_is_applied_until_every_conflict_is_decided_and_the_change_confirmed() {
    let mut screen = screen();
    screen.preview_preset();

    // No decisions, no confirmation.
    assert_eq!(
        screen.apply_preset(None).err(),
        Some(PlanError::UnresolvedConflict("overview".to_string()))
    );
    assert!(screen.config().gestures.is_empty());

    // Decisions, still no confirmation.
    let conflicts: Vec<GestureId> = screen
        .plan()
        .unwrap()
        .conflicts
        .iter()
        .map(|conflict| conflict.gesture.clone())
        .collect();
    for gesture in conflicts {
        screen.resolve(gesture, ConflictResolution::KeepBuiltIn);
    }
    assert!(!screen.preset_card(copy(Locale::EnUs)).can_apply);
    assert_eq!(
        screen.apply_preset(None).err(),
        Some(PlanError::NotConfirmed)
    );
    assert!(screen.config().gestures.is_empty());

    // And the preview is still on screen after both refusals.
    assert!(screen.plan().is_some());

    screen.confirm(true);
    assert!(screen.preset_card(copy(Locale::EnUs)).can_apply);
    assert_eq!(screen.apply_preset(None), Ok(RunState::Applied));
    assert_eq!(screen.config().gestures.len(), 10);
    assert_eq!(screen.preset_status(), PresetStatus::Differs);
}

#[test]
fn a_new_preview_forgets_the_previous_confirmation() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    assert!(screen.preset_card(copy(Locale::EnUs)).can_apply);

    screen.preview_preset();
    let card = screen.preset_card(copy(Locale::EnUs));
    assert!(!card.confirmed);
    assert!(!card.can_apply);
}

#[test]
fn applying_captures_what_was_configured_first_and_restore_puts_it_back() {
    let directory = tempfile::tempdir().unwrap();
    let store = GestureStore::at(directory.path().join("touchpad"));
    let mut screen = screen();
    let before = screen.config().clone();

    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(Some(&store)).unwrap();
    assert_eq!(screen.captured(), Some(&before));
    assert!(store.has_capture());
    assert_eq!(store.load_config().unwrap().gestures.len(), 10);

    screen.restore(Some(&store)).unwrap();
    assert_eq!(screen.config(), &before);
    assert_eq!(store.load_config().unwrap(), before);
}

#[test]
fn turning_gestures_off_restores_the_capture_and_leaves_the_subsystem_off() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();

    screen.disable(None);
    assert!(!screen.config().enabled);
    assert!(screen.config().gestures.is_empty());
    assert!(screen.config().active().is_empty());
}

#[test]
fn keeping_the_desktop_gesture_leaves_our_four_swipes_switched_off() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();

    let rows = screen.rows(copy(Locale::EnUs));
    for conflicting in [
        "overview",
        "current-app-windows",
        "workspace-next",
        "workspace-previous",
    ] {
        let row = rows
            .iter()
            .find(|row| row.id.as_str() == conflicting)
            .unwrap();
        assert!(!row.enabled, "{conflicting} was left on");
    }
    assert!(
        rows.iter()
            .find(|row| row.id.as_str() == "launcher")
            .unwrap()
            .enabled
    );
}

#[test]
fn a_conflict_offers_three_decisions_and_the_remap_moves_the_gesture_out_of_reach() {
    let mut screen = screen();
    screen.preview_preset();
    let card = screen.preset_card(copy(Locale::EnUs));
    let conflict = &card.conflicts[0];
    assert_eq!(conflict.resolution, None);

    let choices = resolution_choices(Some(touchpad_gestures::Direction::Up));
    assert_eq!(choices.len(), 3);
    assert!(matches!(
        choices[2],
        ConflictResolution::RemapOurs { contacts: 5, .. }
    ));

    let conflicts: Vec<GestureId> = card
        .conflicts
        .iter()
        .map(|conflict| conflict.gesture.clone())
        .collect();
    for gesture in conflicts {
        let direction = screen
            .config()
            .get(&gesture)
            .and_then(|gesture| gesture.direction)
            .or(screen
                .plan()
                .unwrap()
                .proposed
                .get(&gesture)
                .and_then(|gesture| gesture.direction))
            .unwrap();
        screen.resolve(
            gesture,
            ConflictResolution::RemapOurs {
                contacts: 5,
                direction,
            },
        );
    }
    screen.confirm(true);
    screen.apply_preset(None).unwrap();

    let overview = screen.config().get(&id("overview")).unwrap();
    assert_eq!(overview.contacts.get(), 5);
    assert!(overview.enabled);
    assert!(!overview.conflict.conflicts());
}

#[test]
fn a_row_carries_the_diagram_the_contact_count_and_the_action() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();
    let c = copy(Locale::EnUs);
    let rows = screen.rows(c);

    let launcher = rows
        .iter()
        .find(|row| row.id.as_str() == "launcher")
        .unwrap();
    assert_eq!(launcher.glyph.dots, 4);
    assert!(launcher.glyph.thumb);
    assert_eq!(launcher.glyph.arrow, Arrow::In);
    assert_eq!(launcher.action_label, c.action_launcher_open);
    assert_eq!(launcher.contacts, 4);
    assert_eq!(launcher.verification, c.verification_verified);

    let desktop = rows
        .iter()
        .find(|row| row.id.as_str() == "show-desktop")
        .unwrap();
    assert_eq!(desktop.glyph.arrow, Arrow::Out);

    let overview = rows
        .iter()
        .find(|row| row.id.as_str() == "overview")
        .unwrap();
    assert_eq!(overview.glyph.arrow, Arrow::Up);
    assert!(!overview.glyph.thumb);
    assert_eq!(overview.direction_label, Some(c.direction_up));
}

#[test]
fn a_conflicting_row_carries_the_badge_and_a_clear_one_does_not() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();
    let rows = screen.rows(copy(Locale::EnUs));

    assert!(
        rows.iter()
            .find(|row| row.id.as_str() == "overview")
            .unwrap()
            .conflict
            .is_some()
    );
    assert!(
        rows.iter()
            .find(|row| row.id.as_str() == "launcher")
            .unwrap()
            .conflict
            .is_none()
    );
}

#[test]
fn an_action_no_adapter_can_perform_is_shown_as_unavailable_rather_than_as_working() {
    let capabilities = ActionCapabilities::everything().with(
        &DesktopAction::ApplicationZoom,
        ActionSupport::unsupported(
            "session.no_application_gestures",
            "no adapter in this build reaches an application's own gestures",
        ),
    );
    let mut screen = GestureScreen::new(
        GestureConfig::default(),
        None,
        Box::new(MockSessionAdapter::with_capabilities(capabilities)),
    );
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();

    let row = screen
        .rows(copy(Locale::EnUs))
        .into_iter()
        .find(|row| row.id.as_str() == "app-zoom")
        .unwrap();
    assert!(!row.supported);
    assert_eq!(
        row.support_detail.as_deref(),
        Some("no adapter in this build reaches an application's own gestures")
    );
}

#[test]
fn test_mode_shows_the_recognition_and_performs_no_action_by_default() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();

    assert!(!screen.live_testing(), "live testing must default to off");
    let run = screen.test_gesture(&id("overview"), copy(Locale::EnUs));
    assert!(!run.lines.is_empty(), "nothing was recognized");
    assert_eq!(run.lines.first().unwrap().kind, "begin");
    assert_eq!(run.lines.last().unwrap().kind, "complete");
    assert_eq!(run.performed, 0, "test mode performed a system action");
}

#[test]
fn live_testing_hands_every_event_to_the_adapter() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();

    screen.set_live_testing(true);
    let run = screen.test_gesture(&id("overview"), copy(Locale::EnUs));
    assert_eq!(run.performed, run.lines.len());
    assert!(run.performed > 0);
}

#[test]
fn a_gesture_that_does_not_reach_the_threshold_is_shown_as_cancelled_and_does_nothing() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();
    screen.set_live_testing(true);

    let run = screen.test_gesture_with(&id("overview"), 0.4, copy(Locale::EnUs));
    assert_eq!(run.lines.last().unwrap().kind, "cancel");
    // Live testing invoked the adapter with the cancel phase; nothing
    // completed, which is what the log shows.
    assert!(!run.lines.iter().any(|line| line.kind == "complete"));
}

#[test]
fn testing_one_gesture_twice_is_not_swallowed_by_the_previous_runs_cooldown() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();

    let first = screen.test_gesture(&id("overview"), copy(Locale::EnUs));
    let second = screen.test_gesture(&id("overview"), copy(Locale::EnUs));
    assert_eq!(first.lines.len(), second.lines.len());
}

#[test]
fn editing_a_gesture_saves_a_legal_change_and_refuses_an_illegal_one() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();

    screen.edit(&id("overview"));
    screen.editor_mut().unwrap().contacts = 5;
    screen.editor_mut().unwrap().enabled = true;
    screen.commit_edit(None).unwrap();
    assert!(screen.editor().is_none());
    assert_eq!(
        screen.config().get(&id("overview")).unwrap().contacts.get(),
        5
    );

    // A cancellation threshold at or above activation cannot be saved, and the
    // editor stays open carrying the reason.
    screen.edit(&id("overview"));
    screen.editor_mut().unwrap().cancellation = 0.9;
    assert!(screen.commit_edit(None).is_err());
    assert!(screen.editor().unwrap().error.is_some());
    assert_eq!(
        screen
            .config()
            .get(&id("overview"))
            .unwrap()
            .cancellation_threshold
            .get(),
        0.25,
        "a refused edit changed the configuration anyway"
    );
}

#[test]
fn changing_the_shape_keeps_the_direction_legal() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();

    screen.edit(&id("overview"));
    screen.editor_mut().unwrap().set_shape(GestureShape::Pinch);
    assert_eq!(screen.editor().unwrap().direction, None);
    screen.editor_mut().unwrap().set_shape(GestureShape::Rotate);
    assert_eq!(
        screen.editor().unwrap().direction,
        Some(touchpad_gestures::Direction::Clockwise)
    );
    screen.commit_edit(None).unwrap();
    assert_eq!(
        screen.config().get(&id("overview")).unwrap().shape,
        GestureShape::Rotate
    );
}

#[test]
fn an_edited_gesture_is_no_longer_claimed_to_be_the_shipped_preset() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();
    assert_eq!(screen.preset_status(), PresetStatus::Applied);
    assert_eq!(screen.config().preset, PresetId::MacStyle);

    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 5;
    screen.commit_edit(None).unwrap();
    assert_eq!(screen.preset_status(), PresetStatus::Differs);
}

#[test]
fn the_diagnostics_log_carries_the_recognized_events_and_the_last_run() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();
    screen.test_gesture(&id("overview"), copy(Locale::EnUs));

    let lines = screen.diagnostics_lines(copy(Locale::EnUs));
    assert!(lines.iter().any(|line| line.contains("complete")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("gnome.overview.swipe")),
        "the built-in half of a resolution is not reported: {lines:?}"
    );
}

#[test]
fn a_failing_adapter_makes_the_run_a_failure_and_the_row_says_so() {
    let mut screen = GestureScreen::new(
        GestureConfig::default(),
        None,
        Box::new(MockSessionAdapter::new().failing(&DesktopAction::LauncherOpen)),
    );
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    assert_eq!(screen.apply_preset(None), Ok(RunState::Failed));

    let row = screen
        .rows(copy(Locale::EnUs))
        .into_iter()
        .find(|row| row.id.as_str() == "launcher")
        .unwrap();
    assert_eq!(row.verification, copy(Locale::EnUs).verification_failed);
}

#[test]
fn a_gesture_failure_never_touches_the_pointer_and_scrolling_state() {
    let directory = tempfile::tempdir().unwrap();
    let store = GestureStore::at(directory.path().join("touchpad"));
    let settings = store.settings();
    settings
        .save_config(&touchpad_core::TouchpadConfig::default())
        .unwrap();
    let before = std::fs::read_to_string(settings.config_path()).unwrap();

    let mut screen = GestureScreen::new(
        GestureConfig::default(),
        None,
        Box::new(MockSessionAdapter::new().failing(&DesktopAction::LauncherOpen)),
    );
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(Some(&store)).unwrap();

    assert_eq!(
        std::fs::read_to_string(settings.config_path()).unwrap(),
        before
    );
    assert!(!settings.backup_path().exists());
}

#[test]
fn no_adapter_in_this_build_claims_to_change_the_desktop() {
    // The Test gestures panel says so out loud, and the assertion is here so a
    // future adapter that does change the desktop has to change this test on
    // purpose.
    assert!(!screen().adapter().describe().performs_system_actions);
}

#[test]
fn the_fixed_chinese_terms_are_the_ones_the_gesture_rows_show() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();
    let c = copy(Locale::ZhTw);

    let actions: Vec<String> = screen
        .rows(c)
        .iter()
        .map(|row| row.action_label.clone())
        .collect();
    assert!(actions.contains(&"顯示桌面".to_string()));
    assert!(actions.contains(&"顯示所有 App".to_string()));
    assert!(actions.contains(&"工作區總覽".to_string()));
    assert_eq!(c.nav_gestures, "手勢");
}

#[test]
fn switching_language_changes_every_gesture_row() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_preset(None).unwrap();

    let english: Vec<String> = screen
        .rows(copy(Locale::EnUs))
        .iter()
        .map(|row| row.label.clone())
        .collect();
    let chinese: Vec<String> = screen
        .rows(copy(Locale::ZhTw))
        .iter()
        .map(|row| row.label.clone())
        .collect();
    assert_eq!(english.len(), chinese.len());
    assert!(english.iter().zip(chinese.iter()).all(|(a, b)| a != b));
}

#[test]
fn every_gesture_and_action_label_fits_its_row_in_both_locales_at_every_scale() {
    // A gesture row gives the name column 200 logical pixels and the action
    // line the full 420 it has left in a compact window.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for gesture in mac_style().gestures {
            let label = gesture_label(&gesture.id, c);
            let action = action_label(&gesture.action, c);
            for scale in [1.0, 1.25, 1.5] {
                assert!(
                    label_fits(&label, 200.0 * scale, scale),
                    "{label} does not fit at {scale}x in {}",
                    locale.tag()
                );
                assert!(
                    label_fits(&action, 420.0 * scale, scale),
                    "{action} does not fit at {scale}x in {}",
                    locale.tag()
                );
            }
        }
    }
}

#[test]
fn the_preview_buttons_stay_on_one_line_at_every_scale_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let previewing = [c.apply_preset, c.cancel_preview];
        for scale in [1.0, 1.25, 1.5] {
            assert_eq!(
                action_layout(1040.0, scale, &previewing),
                ActionLayout::Inline,
                "the preview buttons do not fit at {scale}x in {}",
                locale.tag()
            );
        }
    }
}

#[test]
fn every_button_label_on_the_screen_fits_its_own_button_in_both_locales() {
    // The rows these buttons sit in carry `flex_wrap`, so four of them
    // wrapping is fine; a single label wider than the button it is in is not.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let labels = [
            c.test_run,
            c.edit_gesture,
            c.save_gesture,
            c.restore_gestures,
            c.disable_gestures,
            c.preview_changes,
            c.apply_preset,
            c.cancel_preview,
            c.resolution_keep_built_in,
            c.resolution_disable_built_in,
            c.resolution_remap,
        ];
        for label in labels {
            for scale in [1.0, 1.25, 1.5] {
                assert!(
                    label_fits(label, 280.0 * scale, scale),
                    "{label} does not fit its button at {scale}x in {}",
                    locale.tag()
                );
            }
        }
    }
}

#[test]
fn a_repeatedly_failing_adapter_turns_the_integration_off_by_itself() {
    let mut screen = GestureScreen::new(
        GestureConfig::default(),
        None,
        Box::new(MockSessionAdapter::new().failing(&DesktopAction::LauncherOpen)),
    );
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    assert_eq!(screen.apply_preset(None), Ok(RunState::Failed));
    assert_eq!(screen.consecutive_failures(), 1);
    // One failure is not enough: a session that was still starting must not
    // cost the user every gesture.
    assert!(screen.config().enabled);

    for _ in 1..GestureScreen::FAILURES_BEFORE_DISABLE {
        assert_eq!(screen.verify_all(), RunState::Failed);
    }
    assert!(!screen.config().enabled);
    assert!(
        screen
            .problem()
            .unwrap()
            .starts_with("gestures.adapter_disabled_after_failures")
    );
    assert!(screen.config().active().is_empty());
    // The bindings are still there to turn back on.
    assert_eq!(screen.config().gestures.len(), 10);
}

#[test]
fn a_run_that_succeeds_clears_the_failure_count() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_preset(None).unwrap();
    assert_eq!(screen.consecutive_failures(), 0);
    assert!(screen.config().enabled);
}
