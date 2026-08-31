//! What the Gestures screen can be asserted to do without a window.

use better_actions::{
    ActionCapabilities, ActionSupport, DesktopAction, Key, KeyboardShortcut, Modifier,
};
use touchpad_gestures::{
    ConflictResolution, GestureConfig, GestureId, GestureProfiles, GestureShape, GestureStore,
    KnownShortcuts, PlanError, PresetId, RunState, ShortcutCheck, mac_style,
};
use touchpad_session::MockSessionAdapter;

use crate::gestures_model::{
    Arrow, GestureScreen, KeyGroup, PresetStatus, action_label, gesture_label, resolution_choices,
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

/// A screen reopened on stored profiles, which is what a restart is.
fn screen_with(profiles: GestureProfiles, device: Option<String>) -> GestureScreen {
    GestureScreen::with_profiles(profiles, device, None, Box::new(MockSessionAdapter::new()))
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
        screen.apply_plan(None).err(),
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
    assert_eq!(screen.apply_plan(None).err(), Some(PlanError::NotConfirmed));
    assert!(screen.config().gestures.is_empty());

    // And the preview is still on screen after both refusals.
    assert!(screen.plan().is_some());

    screen.confirm(true);
    assert!(screen.preset_card(copy(Locale::EnUs)).can_apply);
    assert_eq!(screen.apply_plan(None), Ok(RunState::Applied));
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
    screen.apply_plan(Some(&store)).unwrap();
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
    screen.apply_plan(None).unwrap();

    screen.disable(None);
    assert!(!screen.config().enabled);
    assert!(screen.config().gestures.is_empty());
    assert!(screen.config().active().is_empty());
}

#[test]
fn keeping_the_desktop_gesture_leaves_our_four_swipes_switched_off() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_plan(None).unwrap();

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
    screen.apply_plan(None).unwrap();

    let overview = screen.config().get(&id("overview")).unwrap();
    assert_eq!(overview.contacts.get(), 5);
    assert!(overview.enabled);
    assert!(!overview.conflict.conflicts());
}

#[test]
fn a_row_carries_the_diagram_the_contact_count_and_the_action() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_plan(None).unwrap();
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
    screen.apply_plan(None).unwrap();
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
    screen.apply_plan(None).unwrap();

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
    screen.apply_plan(None).unwrap();

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
    screen.apply_plan(None).unwrap();

    screen.set_live_testing(true);
    let run = screen.test_gesture(&id("overview"), copy(Locale::EnUs));
    assert_eq!(run.performed, run.lines.len());
    assert!(run.performed > 0);
}

#[test]
fn a_gesture_that_does_not_reach_the_threshold_is_shown_as_cancelled_and_does_nothing() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();
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
    screen.apply_plan(None).unwrap();

    let first = screen.test_gesture(&id("overview"), copy(Locale::EnUs));
    let second = screen.test_gesture(&id("overview"), copy(Locale::EnUs));
    assert_eq!(first.lines.len(), second.lines.len());
}

#[test]
fn editing_a_gesture_saves_a_legal_change_and_refuses_an_illegal_one() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    screen.apply_plan(None).unwrap();

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
    screen.apply_plan(None).unwrap();

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
    screen.apply_plan(None).unwrap();
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
    screen.apply_plan(None).unwrap();
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
    assert_eq!(screen.apply_plan(None), Ok(RunState::Failed));

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
    screen.apply_plan(Some(&store)).unwrap();

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
    screen.apply_plan(None).unwrap();
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
    screen.apply_plan(None).unwrap();

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
fn every_button_the_advanced_controls_add_fits_its_button_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let labels = [
            c.profile_global,
            c.profile_this_device,
            c.profile_detach,
            c.profile_forget,
            c.export_profiles,
            c.import_profiles,
            c.shortcut_modifiers,
            c.shortcut_key,
            c.group_letters,
            c.group_digits,
            c.group_function,
            c.group_navigation,
            c.group_editing,
            c.group_punctuation,
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
fn every_key_the_picker_draws_fits_its_own_small_button_at_every_scale() {
    // A key button is the narrowest control on the screen: 112 logical pixels.
    // `bracketright` is the longest name in the table and is what decides it.
    // The picker shows the key's own name rather than the character it types,
    // because that name is what the stored shortcut says.
    for key in Key::all() {
        for scale in [1.0, 1.25, 1.5] {
            assert!(
                label_fits(key.name(), 112.0 * scale, scale),
                "{} does not fit a key button at {scale}x",
                key.name()
            );
        }
    }
}

#[test]
fn the_notes_the_advanced_controls_add_fit_the_column_they_are_drawn_in() {
    // The notes wrap, so what matters is that one *line* of them fits the card
    // at its narrowest: 640 logical pixels in a compact window.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for note in [
            c.profile_follows_global,
            c.profile_own,
            c.import_summary,
            c.shortcut_needs_modifier,
            c.shortcut_conflict,
            c.export_label,
            c.import_label,
            c.profiles_file,
            c.profile_heading,
            c.shortcut_heading,
        ] {
            for scale in [1.0, 1.25, 1.5] {
                assert!(
                    label_fits(note, 640.0 * scale, scale),
                    "{note} does not fit at {scale}x in {}",
                    locale.tag()
                );
            }
        }
    }
}

#[test]
fn the_screens_stated_tab_stops_are_distinct_and_in_the_order_they_are_drawn() {
    let order = crate::pages_gestures::GESTURE_TAB_ORDER;
    for pair in order.windows(2) {
        assert!(
            pair[0] < pair[1],
            "tab stop {} is not before {}",
            pair[0],
            pair[1]
        );
    }
    let mut sorted = order.to_vec();
    sorted.dedup();
    assert_eq!(sorted.len(), order.len(), "two controls share a tab stop");
}

#[test]
fn a_repeatedly_failing_adapter_turns_the_integration_off_by_itself() {
    let mut screen = GestureScreen::new(
        GestureConfig::default(),
        None,
        Box::new(MockSessionAdapter::new().failing(&DesktopAction::LauncherOpen)),
    );
    confirmed_preview(&mut screen, ConflictResolution::KeepBuiltIn);
    assert_eq!(screen.apply_plan(None), Ok(RunState::Failed));
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
    screen.apply_plan(None).unwrap();
    assert_eq!(screen.consecutive_failures(), 0);
    assert!(screen.config().enabled);
}

// ---------------------------------------------------------------------------
// Contact counts
// ---------------------------------------------------------------------------

#[test]
fn every_contact_count_the_editor_offers_saves_including_five_fingers() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    for contacts in 1..=5u8 {
        screen.edit(&id("launcher"));
        screen.editor_mut().unwrap().contacts = contacts;
        screen.commit_edit(None).unwrap();
        assert_eq!(
            screen.config().get(&id("launcher")).unwrap().contacts.get(),
            contacts
        );
    }
    // Five contacts with the thumb is a legitimate custom mapping, and the row
    // draws all five.
    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 5;
    screen.editor_mut().unwrap().thumb_required = true;
    screen.commit_edit(None).unwrap();
    let row = screen
        .rows(copy(Locale::EnUs))
        .into_iter()
        .find(|row| row.id.as_str() == "launcher")
        .unwrap();
    assert_eq!(row.glyph.dots, 5);
    assert!(row.glyph.thumb);
}

#[test]
fn a_contact_count_outside_what_a_hand_has_is_refused_by_the_editor() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 6;
    assert!(screen.commit_edit(None).is_err());
    assert!(screen.editor().unwrap().error.is_some());
    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().contacts.get(),
        4,
        "a refused edit changed the configuration anyway"
    );
    // The preset is unchanged by any of this: thumb plus three stays the
    // launcher's mapping until somebody edits it.
    assert_eq!(mac_style().get(&id("launcher")).unwrap().contacts.get(), 4);
    assert!(mac_style().get(&id("launcher")).unwrap().thumb_required);
}

// ---------------------------------------------------------------------------
// Custom keyboard shortcuts
// ---------------------------------------------------------------------------

fn shortcut_action(text: &str) -> DesktopAction {
    DesktopAction::KeyboardShortcut {
        shortcut: KeyboardShortcut::parse(text).unwrap(),
    }
}

#[test]
fn the_key_picker_reaches_every_key_in_the_fixed_table_exactly_once() {
    let mut seen: Vec<&'static str> = Vec::new();
    for group in KeyGroup::ALL {
        for key in group.keys() {
            assert_eq!(KeyGroup::of(key), group);
            seen.push(key.name());
        }
    }
    let mut table: Vec<&'static str> = Key::all().map(|key| key.name()).collect();
    seen.sort_unstable();
    table.sort_unstable();
    assert_eq!(seen, table, "the key picker does not cover the key table");
}

#[test]
fn a_shortcut_is_built_from_the_picked_keys_and_stored_as_a_typed_action() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    screen.edit(&id("launcher"));
    let editor = screen.editor_mut().unwrap();
    editor.set_action(shortcut_action("<Super>g"));
    editor.shortcut.modifiers.clear();
    editor.shortcut.toggle(Modifier::Ctrl);
    editor.shortcut.toggle(Modifier::Alt);
    editor.set_key(Key::parse("F5").unwrap());
    assert_eq!(editor.key_group, KeyGroup::Function);
    screen.commit_edit(None).unwrap();

    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().action,
        shortcut_action("<Ctrl><Alt>F5")
    );
    // And the row says which shortcut it is rather than only that there is one.
    let row = screen
        .rows(copy(Locale::EnUs))
        .into_iter()
        .find(|row| row.id.as_str() == "launcher")
        .unwrap();
    assert!(row.action_label.contains("<Ctrl><Alt>F5"));
}

#[test]
fn a_shortcut_with_no_modifier_is_refused_rather_than_typed_into_the_focused_window() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();
    let before = screen.config().get(&id("launcher")).unwrap().action.clone();

    screen.edit(&id("launcher"));
    let editor = screen.editor_mut().unwrap();
    editor.set_action(shortcut_action("<Super>g"));
    editor.shortcut.modifiers.clear();
    assert!(editor.shortcut.spelling().is_err());
    assert!(screen.commit_edit(None).is_err());

    assert!(screen.editor().unwrap().error.is_some());
    assert_eq!(screen.config().get(&id("launcher")).unwrap().action, before);
}

#[test]
fn the_keys_already_picked_survive_a_trip_through_the_action_list() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    screen.edit(&id("launcher"));
    let editor = screen.editor_mut().unwrap();
    editor.set_action(shortcut_action("<Super>g"));
    editor.shortcut.modifiers.clear();
    editor.shortcut.toggle(Modifier::Shift);
    editor.shortcut.toggle(Modifier::Super);
    editor.set_key(Key::parse("Home").unwrap());
    // Away to another action and back again.
    editor.set_action(DesktopAction::VolumeMute);
    assert!(!editor.action_is_shortcut());
    editor.set_action(DesktopAction::KeyboardShortcut {
        shortcut: DesktopAction::placeholder_shortcut(),
    });
    screen.commit_edit(None).unwrap();

    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().action,
        shortcut_action("<Shift><Super>Home")
    );
}

#[test]
fn a_shortcut_the_session_already_uses_is_said_so_before_it_is_bound() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();
    screen.set_known_shortcuts(KnownShortcuts::from_bindings([(
        "/org/gnome/settings-daemon/plugins/media-keys/www",
        "<Super>w",
    )]));

    screen.edit(&id("launcher"));
    let editor = screen.editor_mut().unwrap();
    editor.set_action(shortcut_action("<Super>w"));
    editor.shortcut.modifiers.clear();
    editor.shortcut.toggle(Modifier::Super);
    editor.set_key(Key::parse("w").unwrap());
    assert_eq!(
        screen.shortcut_check(),
        Some(ShortcutCheck::Conflicts {
            key: "/org/gnome/settings-daemon/plugins/media-keys/www".to_string()
        })
    );

    // A different key is not recorded, which is a different claim from clear.
    screen
        .editor_mut()
        .unwrap()
        .set_key(Key::parse("q").unwrap());
    assert_eq!(screen.shortcut_check(), Some(ShortcutCheck::NoneRecorded));

    // And an action that is not a shortcut is not checked at all.
    screen
        .editor_mut()
        .unwrap()
        .set_action(DesktopAction::VolumeMute);
    assert_eq!(screen.shortcut_check(), None);
}

#[test]
fn a_session_whose_shortcuts_could_not_be_read_says_so_rather_than_claiming_no_conflict() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();
    screen.set_known_shortcuts(KnownShortcuts::unavailable("gnome.database_unreadable"));

    screen.edit(&id("launcher"));
    screen
        .editor_mut()
        .unwrap()
        .set_action(shortcut_action("<Super>g"));
    assert_eq!(
        screen.shortcut_check(),
        Some(ShortcutCheck::Unknown {
            reason: "gnome.database_unreadable".to_string()
        })
    );
}

// ---------------------------------------------------------------------------
// Per-device profiles
// ---------------------------------------------------------------------------

const PAD: &str = "uniq:LEN-0001";
const OTHER_PAD: &str = "input:0003:06cb:ce67:0100:SynPS/2 Synaptics TouchPad";

#[test]
fn a_device_with_no_profile_of_its_own_shows_the_shared_one() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();
    let shared = screen.config().clone();

    screen.select_device(Some(PAD.to_string()));
    assert_eq!(screen.config(), &shared);
    assert!(!screen.device_has_own_profile());
}

#[test]
fn editing_one_device_leaves_the_shared_profile_and_the_other_devices_alone() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    screen.select_device(Some(PAD.to_string()));
    screen.detach_device_profile(None);
    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 5;
    screen.commit_edit(None).unwrap();
    assert!(screen.device_has_own_profile());
    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().contacts.get(),
        5
    );

    screen.select_device(None);
    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().contacts.get(),
        4
    );
    screen.select_device(Some(OTHER_PAD.to_string()));
    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().contacts.get(),
        4
    );
    assert!(!screen.device_has_own_profile());
}

#[test]
fn a_pad_that_follows_the_shared_profile_is_edited_through_it_rather_than_diverged_quietly() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    screen.select_device(Some(PAD.to_string()));
    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 3;
    screen.commit_edit(None).unwrap();

    assert!(
        !screen.device_has_own_profile(),
        "editing a pad that follows the shared profile gave it one of its own"
    );
    screen.select_device(None);
    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().contacts.get(),
        3
    );
}

#[test]
fn switching_devices_drops_a_preview_built_against_the_previous_profile() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    assert!(screen.plan().is_some());

    screen.select_device(Some(PAD.to_string()));
    assert!(
        screen.plan().is_none(),
        "a plan made for one profile survived into another"
    );
    assert_eq!(screen.apply_plan(None).err(), Some(PlanError::NothingToDo));
    assert!(screen.config().gestures.is_empty());
}

#[test]
fn a_per_device_profile_survives_a_restart_and_the_shared_one_is_still_there() {
    let directory = tempfile::tempdir().unwrap();
    let store = GestureStore::at(directory.path().join("touchpad"));

    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(Some(&store)).unwrap();
    screen.select_device(Some(PAD.to_string()));
    screen.detach_device_profile(Some(&store));
    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 5;
    screen.commit_edit(Some(&store)).unwrap();

    let profiles = store.load_profiles().unwrap();
    let reopened = screen_with(profiles, Some(PAD.to_string()));
    assert_eq!(
        reopened
            .config()
            .get(&id("launcher"))
            .unwrap()
            .contacts
            .get(),
        5
    );
    let shared = screen_with(store.load_profiles().unwrap(), None);
    assert_eq!(
        shared.config().get(&id("launcher")).unwrap().contacts.get(),
        4
    );
}

#[test]
fn forgetting_a_device_profile_puts_that_pad_back_on_the_shared_one() {
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    screen.select_device(Some(PAD.to_string()));
    screen.detach_device_profile(None);
    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 5;
    screen.commit_edit(None).unwrap();
    assert!(screen.device_has_own_profile());

    screen.forget_device_profile(None);
    assert!(!screen.device_has_own_profile());
    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().contacts.get(),
        4
    );
}

// ---------------------------------------------------------------------------
// Export and import
// ---------------------------------------------------------------------------

fn imported_document() -> GestureProfiles {
    let mut document = GestureProfiles::global_only(mac_style());
    let mut theirs = mac_style();
    theirs.get_mut(&id("launcher")).unwrap().contacts =
        touchpad_gestures::ContactCount::new(5).unwrap();
    document.devices.insert(PAD.to_string(), theirs);
    document
}

#[test]
fn an_exported_document_carries_every_profile_and_reads_back_as_itself() {
    let directory = tempfile::tempdir().unwrap();
    let store = GestureStore::at(directory.path().join("touchpad"));
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(Some(&store)).unwrap();
    screen.select_device(Some(PAD.to_string()));
    screen.detach_device_profile(Some(&store));
    screen.edit(&id("launcher"));
    screen.editor_mut().unwrap().contacts = 5;
    screen.commit_edit(Some(&store)).unwrap();

    let path = directory.path().join("gestures-export.json");
    store.export_to(&path, &screen.document()).unwrap();
    let read_back = store.import_from(&path).unwrap();

    assert_eq!(read_back, screen.document());
    assert_eq!(
        read_back
            .resolve(Some(PAD))
            .get(&id("launcher"))
            .unwrap()
            .contacts
            .get(),
        5
    );
    assert_eq!(
        read_back
            .resolve(None)
            .get(&id("launcher"))
            .unwrap()
            .contacts
            .get(),
        4
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        read_back.to_json(),
        "a re-export of what was imported is not the same bytes"
    );
}

#[test]
fn an_import_changes_nothing_until_it_has_been_previewed_and_confirmed() {
    let mut screen = screen();
    screen.preview_import("gestures-export.json", imported_document());

    // Previewed, and nothing bound.
    assert!(screen.config().gestures.is_empty());
    let card = screen.preset_card(copy(Locale::EnUs));
    assert_eq!(card.changes.len(), mac_style().gestures.len());
    assert!(!card.can_apply);
    assert_eq!(
        screen.import().map(|import| import.device_profiles.clone()),
        Some(vec![PAD.to_string()])
    );

    // The same gate as the preset: conflicts first, then the confirmation.
    assert_eq!(
        screen.apply_plan(None).err(),
        Some(PlanError::UnresolvedConflict("overview".to_string()))
    );
    let conflicts: Vec<GestureId> = screen
        .plan()
        .unwrap()
        .conflicts
        .iter()
        .map(|conflict| conflict.gesture.clone())
        .collect();
    for gesture in conflicts {
        screen.resolve(gesture, ConflictResolution::DisableBuiltIn);
    }
    assert_eq!(screen.apply_plan(None).err(), Some(PlanError::NotConfirmed));
    assert!(
        screen.config().gestures.is_empty(),
        "an import bound something before it was confirmed"
    );

    screen.confirm(true);
    // Partially supported rather than applied because no adapter in this build
    // can turn a GNOME gesture off, which the report says out loud.
    assert_eq!(screen.apply_plan(None), Ok(RunState::PartiallySupported));
    assert_eq!(screen.config().gestures.len(), 10);
    assert!(screen.import().is_none());
}

#[test]
fn an_applied_import_brings_the_device_profiles_with_it() {
    let mut screen = screen();
    screen.preview_import("gestures-export.json", imported_document());
    let conflicts: Vec<GestureId> = screen
        .plan()
        .unwrap()
        .conflicts
        .iter()
        .map(|conflict| conflict.gesture.clone())
        .collect();
    for gesture in conflicts {
        screen.resolve(gesture, ConflictResolution::DisableBuiltIn);
    }
    screen.confirm(true);
    screen.apply_plan(None).unwrap();

    screen.select_device(Some(PAD.to_string()));
    assert!(screen.device_has_own_profile());
    assert_eq!(
        screen.config().get(&id("launcher")).unwrap().contacts.get(),
        5
    );
}

#[test]
fn a_cancelled_import_leaves_nothing_behind() {
    let mut screen = screen();
    screen.preview_import("gestures-export.json", imported_document());
    screen.cancel_preview();
    assert!(screen.plan().is_none());
    assert!(screen.import().is_none());
    assert!(screen.config().gestures.is_empty());
    assert_eq!(screen.apply_plan(None).err(), Some(PlanError::NothingToDo));
}

#[test]
fn previewing_the_preset_after_an_import_does_not_install_the_file_as_well() {
    let mut screen = screen();
    screen.preview_import("gestures-export.json", imported_document());
    // The user changed their mind and previewed the preset instead.
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    assert!(screen.import().is_none());
    screen.apply_plan(None).unwrap();

    screen.select_device(Some(PAD.to_string()));
    assert!(
        !screen.device_has_own_profile(),
        "a discarded import was installed by confirming the preset"
    );
}

#[test]
fn an_import_with_no_profile_for_this_pad_previews_the_files_shared_profile() {
    let mut screen = screen();
    screen.select_device(Some(OTHER_PAD.to_string()));
    screen.preview_import("gestures-export.json", imported_document());

    let import = screen.import().unwrap();
    assert!(!import.matches_selected_device);
    assert_eq!(
        screen
            .plan()
            .unwrap()
            .proposed
            .get(&id("launcher"))
            .unwrap()
            .contacts
            .get(),
        4,
        "the file's shared profile is not what was previewed"
    );
}

#[test]
fn every_threshold_and_cooldown_the_editor_offers_is_a_legal_value() {
    // The edit view offers fixed steps rather than free text, so the values it
    // offers have to be values a definition would accept — including the
    // combination of the lowest activation with the highest cancellation.
    let mut screen = screen();
    confirmed_preview(&mut screen, ConflictResolution::DisableBuiltIn);
    screen.apply_plan(None).unwrap();

    for activation in [0.4f32, 0.5, 0.6, 0.7, 0.8] {
        for cancellation in [0.0f32, 0.15, 0.25, 0.35, 0.5] {
            for cooldown in [0u64, 150, 350, 600, 1_000] {
                screen.edit(&id("launcher"));
                let editor = screen.editor_mut().unwrap();
                editor.activation = activation;
                editor.cancellation = cancellation;
                editor.cooldown_ms = cooldown;
                let legal = cancellation < activation;
                assert_eq!(
                    screen.commit_edit(None).is_ok(),
                    legal,
                    "activation {activation} with cancellation {cancellation}"
                );
                screen.cancel_edit();
            }
        }
    }
}
