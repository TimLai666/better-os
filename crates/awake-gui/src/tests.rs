//! What this window promises, checked without a display server.
//!
//! Every test here works on the view model, on the copy, or on the layout
//! policy. None of them opens a window, which is what lets them run in CI on a
//! machine with no compositor and still cover the acceptance criteria.

use awake_core::{
    BackendCapabilities, Combine, Condition, ConditionGroup, InterfaceName, ProcessMatchKind,
    ProcessMatcher, ProviderKind, RESOLUTION_STRONGEST_WINS, Reason, Rule, RuleId, SessionOrigin,
    SessionPolicy,
};
use awake_ipc::{
    StatusDocument, WireActiveRule, WireBackend, WireBatteryProtection, WireConflict, WireEnd,
    WireIndicator, WireProvider, WireReason, WireRemaining, WireRuleSummary, WireSession,
};

use crate::{
    i18n::{Locale, copy},
    layout::{ActionLayout, MIN_WINDOW_WIDTH, action_layout},
    model::{
        BatteryView, ConditionControl, ConditionView, ConflictView, ProviderRow, RuleView, Section,
        StatusView,
    },
    settings::{Preferences, PreferencesStore, PresetLength, StoredTheme},
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn backend(available: bool) -> WireBackend {
    WireBackend {
        name: "logind".to_string(),
        available,
        capabilities: BackendCapabilities {
            system_suspend: true,
            idle: true,
            display_sleep: true,
            automatic_lock: false,
        },
        detail: (!available).then(|| "awake.backend.error.unavailable".to_string()),
    }
}

fn session(session_id: u64, origin: SessionOrigin, reason: &str) -> WireSession {
    WireSession {
        session_id,
        reason: reason.to_string(),
        origin,
        policy: SessionPolicy::quick_default(),
        battery_stop_percent: Some(20),
        end: WireEnd::Indefinite,
        started_at_unix_seconds: 1_700_000_000,
        remaining: WireRemaining::UntilEnded,
    }
}

fn status(has_battery: bool) -> StatusDocument {
    StatusDocument {
        indicator: WireIndicator::Inactive,
        effective_policy: SessionPolicy::quick_default(),
        unmet_policy: Vec::new(),
        battery_stop_percent: None,
        sessions: Vec::new(),
        reasons: Vec::new(),
        backend: backend(true),
        attention: None,
        interrupted_previous_session: None,
        reduced_security_confirmed: false,
        active_rules: Vec::new(),
        rule_summary: WireRuleSummary::default(),
        rules_suppression: None,
        conflicts: Vec::new(),
        providers: Vec::new(),
        battery_protection: WireBatteryProtection {
            has_battery,
            percent: has_battery.then_some(74),
            on_ac_power: Some(true),
            stop_below_percent: has_battery.then_some(20),
        },
        now_unix_seconds: 1_700_000_600,
    }
}

/// A machine held awake by a manual session and by one rule at the same time.
fn status_with_two_reasons() -> StatusDocument {
    let mut document = status(true);
    document.indicator = WireIndicator::ActiveManual;
    document.sessions = vec![
        session(7, SessionOrigin::Manual, "Rendering the export"),
        session(8, SessionOrigin::Trigger, "Video call"),
    ];
    document.reasons = vec![
        WireReason {
            session_id: 7,
            origin: SessionOrigin::Manual,
            reason: "Rendering the export".to_string(),
        },
        WireReason {
            session_id: 8,
            origin: SessionOrigin::Trigger,
            reason: "Video call".to_string(),
        },
    ];
    document.active_rules = vec![WireActiveRule {
        rule_id: 3,
        name: "Video call".to_string(),
        session_id: 8,
        priority: 60,
    }];
    document
}

fn unavailable_provider(kind: ProviderKind) -> ProviderRow {
    ProviderRow::from_wire(&WireProvider {
        kind,
        available: false,
        poll_seconds: None,
        explanation: Some("awake.provider.error.no_session_bus".to_string()),
    })
}

fn rule(id: u64, name: &str, condition: Condition) -> Rule {
    Rule::new(
        RuleId(id),
        Reason::new(name).expect("a fixture name must be a valid reason"),
        Combine::All,
        [ConditionGroup::one(condition).expect("a single condition is a valid group")],
    )
    .expect("the fixture rule must be valid")
}

// ---------------------------------------------------------------------------
// Copy
// ---------------------------------------------------------------------------

#[test]
fn every_user_visible_string_exists_in_both_locales_and_the_two_differ() {
    let en = copy(Locale::EnUs);
    let zh = copy(Locale::ZhTw);

    // The section names and their one-line purposes.
    for section in Section::ALL {
        for text in [section.title(en), section.subtitle(en)] {
            assert!(!text.trim().is_empty(), "{:?} is missing English", section);
        }
        for text in [section.title(zh), section.subtitle(zh)] {
            assert!(!text.trim().is_empty(), "{:?} is missing Chinese", section);
        }
        // The application name is a product name and is the same in both, but
        // a section's own purpose must actually have been translated.
        assert_ne!(
            section.subtitle(en),
            section.subtitle(zh),
            "{:?} shows English in the Chinese build",
            section
        );
    }

    // Every provider, since Diagnostics and the rules editor both name them.
    for provider in ProviderKind::ALL {
        let english = crate::model::provider_label(provider, en);
        let chinese = crate::model::provider_label(provider, zh);
        assert!(!english.trim().is_empty());
        assert!(!chinese.trim().is_empty());
        assert_ne!(english, chinese, "{provider:?} was never translated");
    }
}

#[test]
fn every_status_and_action_string_is_translated_in_both_locales() {
    let en = copy(Locale::EnUs);
    let zh = copy(Locale::ZhTw);
    let pairs: Vec<(&str, &str)> = vec![
        (en.service_unreachable, zh.service_unreachable),
        (en.service_unreachable_detail, zh.service_unreachable_detail),
        (en.active_summary, zh.active_summary),
        (en.inactive_summary, zh.inactive_summary),
        (en.attention_summary, zh.attention_summary),
        (en.paused_summary, zh.paused_summary),
        (en.effective_policy, zh.effective_policy),
        (en.prevented, zh.prevented),
        (en.allowed, zh.allowed),
        (en.not_delivered, zh.not_delivered),
        (en.active_reasons, zh.active_reasons),
        (en.no_active_reasons, zh.no_active_reasons),
        (en.end_session, zh.end_session),
        (en.end_this_reason, zh.end_this_reason),
        (en.extend_session, zh.extend_session),
        (en.modify_session, zh.modify_session),
        (en.ending_leaves, zh.ending_leaves),
        (en.ending_leaves_nothing, zh.ending_leaves_nothing),
        (en.inhibitor_health, zh.inhibitor_health),
        (en.conflicts_heading, zh.conflicts_heading),
        (en.conflict_explanation, zh.conflict_explanation),
        (en.resolution_strongest_wins, zh.resolution_strongest_wins),
        (
            en.resolution_earliest_battery_stop,
            zh.resolution_earliest_battery_stop,
        ),
        (en.presets_heading, zh.presets_heading),
        (en.preset_indefinite, zh.preset_indefinite),
        (en.add_preset, zh.add_preset),
        (en.set_as_default, zh.set_as_default),
        (en.restore_defaults, zh.restore_defaults),
        (en.default_session_policy, zh.default_session_policy),
        (en.new_rule, zh.new_rule),
        (en.no_rules, zh.no_rules),
        (en.no_rules_detail, zh.no_rules_detail),
        (en.match_all, zh.match_all),
        (en.match_any, zh.match_any),
        (en.add_group, zh.add_group),
        (en.add_condition, zh.add_condition),
        (en.test_rule, zh.test_rule),
        (en.test_true, zh.test_true),
        (en.test_false, zh.test_false),
        (en.test_unknown, zh.test_unknown),
        (en.would_be_active, zh.would_be_active),
        (en.would_not_be_active, zh.would_not_be_active),
        (en.tested_rule_is_disabled, zh.tested_rule_is_disabled),
        (en.rules_paused_until, zh.rules_paused_until),
        (en.rules_overridden, zh.rules_overridden),
        (en.override_all_rules, zh.override_all_rules),
        (en.condition_unavailable, zh.condition_unavailable),
        (
            en.condition_unavailable_detail,
            zh.condition_unavailable_detail,
        ),
        (en.rule_invalid_input, zh.rule_invalid_input),
        (en.default_policy_heading, zh.default_policy_heading),
        (en.default_battery_threshold, zh.default_battery_threshold),
        (en.battery_stops_at, zh.battery_stops_at),
        (en.battery_stop_off, zh.battery_stop_off),
        (en.reduced_security_warning, zh.reduced_security_warning),
        (en.no_battery, zh.no_battery),
        (en.no_battery_detail, zh.no_battery_detail),
        (en.not_applicable, zh.not_applicable),
        (en.quit_warning, zh.quit_warning),
        (en.quit_warning_detail, zh.quit_warning_detail),
        (en.on_battery_rule, zh.on_battery_rule),
        (en.on_battery_rule_detail, zh.on_battery_rule_detail),
        (en.no_history, zh.no_history),
        (en.retention_note, zh.retention_note),
        (en.showing_of, zh.showing_of),
        (en.history_stop_cause, zh.history_stop_cause),
        (en.cause_user_request, zh.cause_user_request),
        (en.cause_battery_threshold, zh.cause_battery_threshold),
        (en.cause_rules_suppressed, zh.cause_rules_suppressed),
        (en.backend_heading, zh.backend_heading),
        (en.provider_heading, zh.provider_heading),
        (en.cadence_column, zh.cadence_column),
        (en.poll_every_seconds, zh.poll_every_seconds),
        (en.no_polling, zh.no_polling),
        (en.verification_heading, zh.verification_heading),
        (
            en.verified_holds_no_inhibitor,
            zh.verified_holds_no_inhibitor,
        ),
        (en.verified_no_shell_command, zh.verified_no_shell_command),
        (en.tray_host_unavailable, zh.tray_host_unavailable),
        (
            en.tray_host_unavailable_detail,
            zh.tray_host_unavailable_detail,
        ),
        (en.language, zh.language),
        (en.appearance, zh.appearance),
        (en.dark_theme, zh.dark_theme),
        (en.light_theme, zh.light_theme),
        (en.settings_storage_failed, zh.settings_storage_failed),
        (en.keyboard_hint, zh.keyboard_hint),
    ];

    for (english, chinese) in pairs {
        assert!(!english.trim().is_empty(), "an English string is empty");
        assert!(!chinese.trim().is_empty(), "a Chinese string is empty");
        assert_ne!(
            english, chinese,
            "{english:?} was left in English in the Chinese build"
        );
    }
}

#[test]
fn every_end_cause_and_condition_has_words_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for text in [
            c.cause_user_request,
            c.cause_expired,
            c.cause_battery_threshold,
            c.cause_backend_failure,
            c.cause_service_shutdown,
            c.cause_replaced,
            c.cause_trigger_cleared,
            c.cause_rules_suppressed,
            c.cause_unrecognized,
        ] {
            assert!(
                !text.trim().is_empty(),
                "{locale:?} is missing an end cause"
            );
        }

        // Every condition variant must produce a sentence, not an empty line.
        for provider in ProviderKind::ALL {
            let condition = crate::app::AwakeApp::default_condition(provider);
            let sentence = crate::model::condition_summary(&condition, c);
            assert!(
                !sentence.trim().is_empty(),
                "{locale:?} has no sentence for {provider:?}"
            );
            assert!(
                !sentence.contains('{'),
                "{locale:?} left a placeholder unfilled for {provider:?}: {sentence}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Sections and reachability
// ---------------------------------------------------------------------------

#[test]
fn all_eight_sections_are_present_and_each_one_is_reachable() {
    assert_eq!(Section::ALL.len(), 8);

    let mut keys: Vec<&str> = Section::ALL
        .iter()
        .map(|section| section.as_key())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 8, "two sections share an identity");

    let mut shortcuts: Vec<&str> = Section::ALL
        .iter()
        .map(|section| section.shortcut())
        .collect();
    shortcuts.sort_unstable();
    shortcuts.dedup();
    assert_eq!(
        shortcuts.len(),
        8,
        "two sections would answer the same keystroke"
    );

    // Every section is reachable by its own index, which is what the sidebar
    // and the keyboard both walk.
    for (index, section) in Section::ALL.iter().enumerate() {
        assert_eq!(Section::at_index(index), Some(*section));
    }
    assert_eq!(Section::at_index(8), None);

    // And every binding the window installs names a section shortcut.
    assert_eq!(crate::shell::key_bindings().len(), 9);
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[test]
fn a_status_with_two_active_reasons_renders_both_of_them() {
    let view = StatusView::from_status(&status_with_two_reasons());

    assert_eq!(view.reasons.len(), 2);
    assert!(view.is_active());
    assert_eq!(view.reasons[0].display_name(), "Rendering the export");
    // The rule-started session is named by its rule rather than by a session id.
    assert_eq!(view.reasons[1].display_name(), "Video call");
    assert_eq!(view.reasons[1].rule_id, Some(3));
    assert_eq!(view.manual_session_id(), Some(7));
    assert_eq!(
        view.summary(copy(Locale::EnUs)),
        copy(Locale::EnUs).active_summary
    );
}

#[test]
fn ending_one_reason_leaves_the_other_one_explaining_the_machine() {
    let view = StatusView::from_status(&status_with_two_reasons());

    let remaining = view.reasons_after_ending(7);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].display_name(), "Video call");

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let explanation = view.ending_explanation(7, c);
        assert!(
            explanation.contains("Video call"),
            "{locale:?} did not name the reason that survives: {explanation}"
        );
        assert!(
            !explanation.contains('{'),
            "a placeholder was left unfilled"
        );
        assert_ne!(explanation, c.ending_leaves_nothing);
    }

    // Ending the last one is the only case where the machine is released.
    let alone = StatusView::from_status(&{
        let mut document = status_with_two_reasons();
        document.sessions.truncate(1);
        document.reasons.truncate(1);
        document.active_rules.clear();
        document
    });
    assert_eq!(
        alone.ending_explanation(7, copy(Locale::EnUs)),
        copy(Locale::EnUs).ending_leaves_nothing
    );
}

#[test]
fn a_policy_the_backend_cannot_deliver_is_not_shown_as_in_force() {
    let mut document = status_with_two_reasons();
    document.effective_policy.prevent_automatic_lock = true;
    let view = StatusView::from_status(&document);
    let c = copy(Locale::EnUs);

    let lock = view
        .policy
        .iter()
        .find(|row| row.field == crate::model::PolicyRowField::AutomaticLock)
        .expect("automatic lock must be one of the rows");
    // The fixture backend cannot hold off automatic locking.
    assert!(lock.prevented);
    assert!(!lock.delivered);
    assert_eq!(lock.value(c), c.not_delivered);
    assert_ne!(lock.value(c), c.prevented);
}

// ---------------------------------------------------------------------------
// Providers and the rules editor
// ---------------------------------------------------------------------------

#[test]
fn an_unavailable_provider_renders_its_explanation_and_not_an_enabled_control() {
    let providers = vec![unavailable_provider(ProviderKind::AudioPlayback)];
    let condition = Condition::AudioPlayback { playing: true };

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let view = ConditionView::present(&condition, &providers, c);

        assert!(
            !view.is_editable(),
            "{locale:?} would have drawn an editable control for a provider that cannot be read"
        );
        assert!(matches!(view.control, ConditionControl::Unavailable { .. }));
        assert_eq!(
            view.explanation(),
            Some("awake.provider.error.no_session_bus")
        );
        assert!(!c.condition_unavailable.trim().is_empty());
        assert!(!c.condition_unavailable_detail.trim().is_empty());
    }

    // The same condition against a provider that reports itself available is
    // editable, so the refusal is about the reading and not about the variant.
    let available = vec![ProviderRow::from_wire(&WireProvider {
        kind: ProviderKind::AudioPlayback,
        available: true,
        poll_seconds: Some(2),
        explanation: None,
    })];
    let view = ConditionView::present(&condition, &available, copy(Locale::EnUs));
    assert!(view.is_editable());
    assert_eq!(view.explanation(), None);
}

#[test]
fn a_rule_carrying_an_unreadable_condition_is_marked_before_it_is_opened() {
    let providers = vec![unavailable_provider(ProviderKind::ProcessRunning)];
    let source = rule(
        4,
        "Compile",
        Condition::ProcessRunning {
            matcher: ProcessMatcher::new(ProcessMatchKind::ExecutableName, "cargo")
                .expect("a plain executable name is valid"),
        },
    );

    let view = RuleView::present(&source, &[4], &providers, copy(Locale::EnUs));
    assert!(view.has_unavailable_condition);
    assert!(view.matching_now);
    assert_eq!(view.groups.len(), 1);
    assert!(!view.groups[0].conditions[0].is_editable());
}

#[test]
fn a_provider_the_service_never_mentioned_stays_editable_rather_than_being_guessed_at() {
    // Reporting nothing about a provider is not the same as reporting it
    // broken, and treating the two alike would disable a working control.
    let view = ConditionView::present(
        &Condition::NetworkInterfaceUp {
            interface: InterfaceName::new("eth0").expect("a plain name is valid"),
        },
        &[],
        copy(Locale::EnUs),
    );
    assert!(view.is_editable());
}

#[test]
fn a_provider_reported_unavailable_without_a_reason_still_gets_an_explanation() {
    let providers = vec![ProviderRow::from_wire(&WireProvider {
        kind: ProviderKind::Fullscreen,
        available: false,
        poll_seconds: None,
        explanation: None,
    })];
    let c = copy(Locale::ZhTw);
    let view = ConditionView::present(&Condition::Fullscreen { active: true }, &providers, c);

    assert!(!view.is_editable());
    assert_eq!(view.explanation(), Some(c.unknown));
}

#[test]
fn the_provider_table_states_a_poll_cadence_for_every_provider() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let polled = ProviderRow::from_wire(&WireProvider {
            kind: ProviderKind::CpuUtilization,
            available: true,
            poll_seconds: Some(5),
            explanation: None,
        });
        assert!(polled.cadence_label(c).contains('5'));
        assert_ne!(polled.cadence_label(c), c.no_polling);

        let free = ProviderRow::from_wire(&WireProvider {
            kind: ProviderKind::AcPower,
            available: true,
            poll_seconds: None,
            explanation: None,
        });
        assert_eq!(free.cadence_label(c), c.no_polling);
    }
}

// ---------------------------------------------------------------------------
// Conflicts
// ---------------------------------------------------------------------------

#[test]
fn the_conflict_explanation_names_the_winning_rule() {
    let conflict = ConflictView::from_wire(&WireConflict {
        field: "prevent_display_sleep".to_string(),
        winner_rule_id: 3,
        winner_name: "Video call".to_string(),
        overridden_rule_ids: vec![5, 6],
        resolution_key: RESOLUTION_STRONGEST_WINS.to_string(),
    });

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let explanation = conflict.explanation(c);
        assert!(
            explanation.contains("Video call"),
            "{locale:?} did not name the winning rule: {explanation}"
        );
        assert!(
            explanation.contains(c.field_display_sleep),
            "{locale:?} did not name the field the rules disagreed about"
        );
        assert!(!explanation.contains('{'));
        assert_eq!(conflict.resolution_label(c), c.resolution_strongest_wins);
        assert_eq!(
            conflict.overridden_note(c),
            Some(crate::i18n::fill(c.conflict_overrode, "count", "2"))
        );
    }
}

#[test]
fn a_conflict_field_this_build_does_not_know_is_named_rather_than_dropped() {
    let conflict = ConflictView::from_wire(&WireConflict {
        field: "prevent_something_newer".to_string(),
        winner_rule_id: 1,
        winner_name: "Newer rule".to_string(),
        overridden_rule_ids: Vec::new(),
        resolution_key: "awake.conflict.something_newer".to_string(),
    });
    let c = copy(Locale::EnUs);

    assert!(conflict.explanation(c).contains("Newer rule"));
    assert_eq!(conflict.field_label(c), c.unknown);
    assert_eq!(conflict.overridden_note(c), None);
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

#[test]
fn a_machine_with_no_battery_shows_not_applicable_rather_than_a_threshold() {
    let view = StatusView::from_status(&status(false));

    assert_eq!(view.battery, BatteryView::NotApplicable);
    assert!(
        !view.battery.offers_threshold(),
        "a desktop was offered a threshold that could never fire"
    );
    assert_eq!(view.battery.threshold_percent(), None);

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        assert_eq!(view.battery.summary(c), c.not_applicable);
        assert_ne!(view.battery.summary(c), c.battery_stop_off);
    }
}

#[test]
fn a_machine_with_a_battery_states_the_threshold_in_force() {
    let view = StatusView::from_status(&status(true));

    assert!(view.battery.offers_threshold());
    assert_eq!(view.battery.threshold_percent(), Some(20));
    let summary = view.battery.summary(copy(Locale::EnUs));
    assert!(summary.contains("20"), "{summary}");
    assert!(!summary.contains('{'));
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// The longest labels an action row has to carry, per locale.
fn longest_action_label(locale: Locale) -> usize {
    let c = copy(locale);
    [
        c.end_session,
        c.modify_session,
        c.restore_defaults,
        c.override_all_rules,
        c.set_as_default,
        c.pause_rules_short,
    ]
    .iter()
    .map(|label| label.chars().count())
    .max()
    .expect("the label list is not empty")
}

#[test]
fn the_shipped_action_labels_wrap_once_the_smallest_window_is_scaled_up() {
    // The shipped labels in both locales are short enough to sit on one line in
    // the smallest supported window at 100%. They are not short enough to
    // survive that window being scaled, because scaling shrinks the logical
    // width the row has to work with: 760px at 125% is 608 logical pixels, which
    // is below the 680 this policy will lay a row out inline in.
    //
    // Asserting Wrapped at every scale would read better and be false, so this
    // states the breakpoint that actually exists.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let longest = longest_action_label(locale);
        assert_eq!(
            action_layout(MIN_WINDOW_WIDTH, 1.0, longest),
            ActionLayout::Inline,
            "{locale:?} fits the smallest window unscaled"
        );
        for scale in [1.25, 1.5] {
            assert_eq!(
                action_layout(MIN_WINDOW_WIDTH, scale, longest),
                ActionLayout::Wrapped,
                "{locale:?} at {scale}x would have run its actions off the window"
            );
        }
        // The same row stays on one line on a normal desktop at every scale.
        for scale in [1.0, 1.25, 1.5] {
            assert_eq!(
                action_layout(1920.0, scale, longest),
                ActionLayout::Inline,
                "{locale:?} at {scale}x wrapped a row that fits at 1920"
            );
        }
    }
}

#[test]
fn a_synthetic_long_translation_wraps_in_the_smallest_window_at_every_scale() {
    // A future translation is allowed to be longer than both shipped ones. The
    // policy has to hold for it too, or the first long string breaks the row.
    let synthetic =
        "End the running session and let this machine follow its own power settings again";
    for scale in [1.0, 1.25, 1.5] {
        assert_eq!(
            action_layout(MIN_WINDOW_WIDTH, scale, synthetic.chars().count()),
            ActionLayout::Wrapped,
            "a label this long cannot share the smallest window with a session summary"
        );
    }
    // It does fit a wide screen, and claiming otherwise would be asserting a
    // limit this policy does not have.
    assert_eq!(
        action_layout(1920.0, 1.0, synthetic.chars().count()),
        ActionLayout::Inline
    );
}

// ---------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------

#[test]
fn an_unconfigured_window_opens_dark() {
    assert_eq!(Preferences::default().theme, StoredTheme::Dark);
    assert_eq!(Preferences::default().locale(), Locale::System);
}

#[test]
fn reordering_a_preset_keeps_the_default_pointing_at_the_same_preset() {
    let mut preferences = Preferences::default();
    let default_before = preferences.presets[preferences.default_preset];

    assert!(preferences.move_preset(1, -1));
    assert_eq!(
        preferences.presets[preferences.default_preset],
        default_before
    );
    assert_eq!(preferences.default_preset, 0);

    // Moving past either end is refused rather than silently clamped.
    assert!(!preferences.move_preset(0, -1));
    assert!(!preferences.move_preset(preferences.presets.len() - 1, 1));
}

#[test]
fn the_last_preset_cannot_be_removed_and_defaults_can_be_restored() {
    let mut preferences = Preferences::default();
    while preferences.presets.len() > 1 {
        assert!(preferences.remove_preset(0));
    }
    assert!(
        !preferences.remove_preset(0),
        "the tray would have been left with a submenu that starts nothing"
    );

    preferences.restore_default_presets();
    assert_eq!(preferences.presets, Preferences::shipped_presets());
    assert!(preferences.default_preset < preferences.presets.len());
}

#[test]
fn a_duplicate_preset_length_is_refused() {
    let mut preferences = Preferences::default();
    let existing = preferences.presets[0];
    assert!(!preferences.add_preset(existing));
    assert!(preferences.add_preset(PresetLength::Minutes { minutes: 45 }));
}

#[test]
fn preferences_survive_a_round_trip_through_the_store() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let store = PreferencesStore::at_path(directory.path().join("preferences.json"));
    assert!(store.path().ends_with("preferences.json"));

    let preferences = Preferences {
        theme: StoredTheme::Light,
        locale: Locale::ZhTw.as_key().to_string(),
        ..Preferences::default()
    };
    store
        .save(&preferences)
        .expect("the store must be writable");

    let (loaded, readable) = store.load();
    assert!(readable);
    assert_eq!(loaded, preferences);
    assert_eq!(loaded.locale(), Locale::ZhTw);
}

#[test]
fn a_preferences_file_this_build_cannot_understand_is_reported_rather_than_trusted() {
    let directory = std::env::temp_dir().join(format!(
        "awake-gui-preferences-newer-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("the temporary directory must be creatable");
    let path = directory.join("preferences.json");
    std::fs::write(&path, br#"{"schema_version":99}"#).expect("the fixture must be writable");

    let (loaded, readable) = PreferencesStore::at_path(&path).load();
    assert!(
        !readable,
        "a file written by a newer build was silently accepted"
    );
    assert_eq!(loaded, Preferences::default());

    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_preset_length_reads_as_hours_once_it_passes_an_hour() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let hours = PresetLength::Minutes { minutes: 180 }.label(c);
        assert!(hours.contains('3'), "{locale:?}: {hours}");
        assert!(!hours.contains("180"), "{locale:?}: {hours}");
        assert!(!hours.contains('{'));

        let minutes = PresetLength::Minutes { minutes: 15 }.label(c);
        assert!(minutes.contains("15"));
        assert_eq!(PresetLength::Indefinite.label(c), c.preset_indefinite);
    }
}
