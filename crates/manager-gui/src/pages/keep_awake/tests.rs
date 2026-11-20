//! What Keep awake promises, checked without a display server: the view
//! model that moved from the Better Awake window with its tests (ticket 87),
//! and the page's own decisions.

use awake_core::{
    BackendCapabilities, Combine, Condition, ConditionGroup, InterfaceName, ProcessMatchKind,
    ProcessMatcher, ProviderKind, RESOLUTION_STRONGEST_WINS, Reason, Rule, RuleId, SessionOrigin,
    SessionPolicy,
};
use awake_ipc::{
    StatusDocument, WireActiveRule, WireBackend, WireBatteryProtection, WireConflict, WireEnd,
    WireIndicator, WireProvider, WireReason, WireRemaining, WireRuleSummary, WireSession,
};

use super::copy::copy;
use super::model::{
    BatteryView, ConditionControl, ConditionView, ConflictView, ProviderRow, RuleView, StatusView,
};
use crate::i18n::Locale;

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
            lid_switch: true,
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
        lid: awake_ipc::WireLidProtection::default(),
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
        explanation: Some("awake.provider.no_alsa_playback_device".to_string()),
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
fn a_status_with_two_active_reasons_renders_both_of_them() {
    let view = StatusView::from_status(&status_with_two_reasons());

    assert_eq!(view.reasons.len(), 2);
    assert_eq!(view.reasons[0].display_name(), "Rendering the export");
    // The rule-started session is named by its rule rather than by a session id.
    assert_eq!(view.reasons[1].display_name(), "Video call");
    assert_eq!(view.reasons[1].rule_id, Some(3));
    assert_eq!(view.manual_session_id(), Some(7));
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
        .find(|row| row.field == super::model::PolicyRowField::AutomaticLock)
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
fn a_reason_the_table_does_not_know_is_named_as_unknown_with_its_key() {
    let providers = vec![ProviderRow::from_wire(&WireProvider {
        kind: ProviderKind::AudioPlayback,
        available: false,
        poll_seconds: None,
        explanation: Some("awake.provider.from_a_newer_service".to_string()),
    })];
    let condition = Condition::AudioPlayback { playing: true };
    let english = ConditionView::present(&condition, &providers, copy(Locale::EnUs));
    assert_eq!(
        english.explanation(),
        Some("Unknown reason (awake.provider.from_a_newer_service)")
    );
    let chinese = ConditionView::present(&condition, &providers, copy(Locale::ZhTw));
    assert_eq!(
        chinese.explanation(),
        Some("原因不明（awake.provider.from_a_newer_service）")
    );
}

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
        // A sentence in the window's language, never the key (ticket 146).
        let expected = match locale {
            Locale::ZhTw => {
                "找不到播放聲音的裝置。Better Awake 是透過 ALSA 讀取聲音狀態，看不到藍牙和網路喇叭。"
            }
            _ => {
                "No sound playback device was found. Better Awake reads sound through ALSA, so Bluetooth and network speakers are not seen."
            }
        };
        assert_eq!(view.explanation(), Some(expected));
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
            Some(super::copy::fill(c.conflict_overrode, "count", "2"))
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
#[test]
fn the_lid_choice_uses_the_trays_words() {
    // Copied from awake-tray's labels, so the window and the tray say the
    // same thing about the same choice.
    let zh = copy(Locale::ZhTw);
    assert_eq!(zh.keep_awake_lid_closed, "闔上螢幕時也保持清醒");
    assert_eq!(zh.lid_sleep, "闔上螢幕時睡眠");
    assert_eq!(zh.lid_stops_after, "{limit}後停止");
    assert_eq!(zh.lid_never_stops, "不會因此停止");
    let en = copy(Locale::EnUs);
    assert_eq!(en.keep_awake_lid_closed, "Also when the lid is closed");
    assert_eq!(en.lid_sleep, "Sleep when the lid is closed");
    assert_eq!(en.lid_stops_after, "Stops after {limit}");
    assert_eq!(en.lid_never_stops, "Never stops for this");
}

#[test]
fn the_status_shows_the_lid_row_and_the_limit_only_while_the_lid_is_held() {
    let mut document = status(true);
    let view = StatusView::from_status(&document);
    let lid = view
        .policy
        .iter()
        .find(|row| row.field == super::model::PolicyRowField::LidSwitch)
        .expect("the lid row is always drawn");
    assert_eq!(lid.value(copy(Locale::EnUs)), "Allowed");
    assert!(!view.lid.held);

    document.effective_policy = SessionPolicy::manual_default();
    document.lid.battery_limit = awake_core::LidBatteryLimit::Minutes { minutes: 240 };
    let view = StatusView::from_status(&document);
    assert!(view.lid.held);
    let zone = better_core::local_time::TimeZone::UTC;
    assert_eq!(
        view.lid
            .limit_summary(Locale::EnUs, document.now_unix_seconds, zone),
        "Stops after 4 hours"
    );
    assert_eq!(
        view.lid
            .limit_summary(Locale::ZhTw, document.now_unix_seconds, zone),
        "4 小時後停止"
    );
    let lid = view
        .policy
        .iter()
        .find(|row| row.field == super::model::PolicyRowField::LidSwitch)
        .unwrap();
    assert_eq!(lid.value(copy(Locale::ZhTw)), "已擋下");

    // A backend with no lid lock, or a service from before protocol version 3
    // whose status has no lid fields: the row says it is unavailable.
    document.backend.capabilities.lid_switch = false;
    let view = StatusView::from_status(&document);
    assert!(!view.lid.held);
    assert!(!view.lid.available);
    let lid = view
        .policy
        .iter()
        .find(|row| row.field == super::model::PolicyRowField::LidSwitch)
        .unwrap();
    assert_eq!(lid.value(copy(Locale::ZhTw)), "目前無法使用");
    assert_eq!(lid.value(copy(Locale::EnUs)), "Not available right now");
}

#[test]
fn the_safety_note_says_only_what_applies_to_the_session() {
    use super::model::lid_safety_note;
    use awake_core::LidBatteryLimit;
    let note = lid_safety_note(LidBatteryLimit::default(), Some(20), true, Locale::EnUs);
    assert!(note.contains("stops below 20%"), "{note}");
    assert!(note.contains("2 hours"), "{note}");
    assert!(note.contains("goes to sleep right away"), "{note}");

    // No battery stop: nothing claims there is one.
    let unprotected = lid_safety_note(LidBatteryLimit::default(), None, true, Locale::EnUs);
    assert!(!unprotected.contains("stops below"), "{unprotected}");
    assert!(
        unprotected.contains("does not stop for a low battery"),
        "{unprotected}"
    );

    // No closed-lid limit: says the battery can run out.
    let never = lid_safety_note(LidBatteryLimit::Never, None, true, Locale::ZhTw);
    assert!(never.contains("不會因為電量低而停止"), "{never}");
    assert!(never.contains("電池可能會用完"), "{never}");
    assert!(!never.contains("超過"), "{never}");
    assert!(!never.contains('{'), "{never}");

    // A machine with no battery hears nothing about one.
    let desktop = lid_safety_note(LidBatteryLimit::default(), Some(20), false, Locale::ZhTw);
    assert!(
        !desktop.contains("電池") && !desktop.contains("電量"),
        "{desktop}"
    );
    assert!(desktop.contains("電腦會馬上睡眠"), "{desktop}");
}

#[test]
fn a_running_closed_lid_clock_reads_as_a_countdown_and_otherwise_as_the_rule() {
    let mut document = status(true);
    document.effective_policy = SessionPolicy::manual_default();
    let view = StatusView::from_status(&document);
    let zone = better_core::local_time::TimeZone::UTC;
    assert_eq!(
        view.lid
            .limit_summary(Locale::EnUs, document.now_unix_seconds, zone),
        "Stops after 2 hours"
    );
    document.lid.stops_at_unix_seconds = Some(document.now_unix_seconds + 90 * 60);
    let view = StatusView::from_status(&document);
    let at = better_core::local_time::clock_time(document.now_unix_seconds + 90 * 60, zone);
    assert_eq!(
        view.lid
            .limit_summary(Locale::EnUs, document.now_unix_seconds, zone),
        format!("Stops at {at} (90 minutes left)")
    );
    assert_eq!(
        view.lid
            .limit_summary(Locale::ZhTw, document.now_unix_seconds, zone),
        format!("{at} 停止（還有 90 分鐘）")
    );
}

#[test]
fn a_session_the_closed_lid_ended_is_named_in_history() {
    let entry = super::model::HistoryRow {
        session_id: 3,
        started_at_unix_seconds: 1_700_000_000,
        ended_at_unix_seconds: Some(1_700_007_200),
        origin: SessionOrigin::Manual,
        rule_id: None,
        reasons: vec!["Download".to_string()],
        policy: SessionPolicy::manual_default(),
        battery_stop_percent: Some(20),
        end_cause: Some("lid_closed_on_battery".to_string()),
        backend_failure: None,
    };
    assert_eq!(
        entry.cause_label(copy(Locale::EnUs)),
        "The lid stayed closed on battery past the limit"
    );
}

// ---------------------------------------------------------------------------
// Ticket 87: the page's own decisions
// ---------------------------------------------------------------------------

mod page {
    use super::*;
    use crate::attention::{AttentionAction, Snapshot};
    use crate::navigation::Route;
    use crate::pages::keep_awake::copy::page_copy;
    use crate::pages::keep_awake::decide::{
        self, Facts, Headline, Presence, ReadError, ReasonAction, Service, SubPage,
    };
    use crate::pages::{self, NavBadge};
    use awake_ipc::{QuickLength, QuickSessions, RequestBody, WireSuppression};
    use better_core::local_time::TimeZone;
    use better_ui::fluent::time_picker::Time;

    const NOW: u64 = 1_700_000_000; // 2023-11-14 22:13:20 UTC

    fn at(hour: u8, minute: u8) -> Time {
        Time::new(hour, minute).unwrap()
    }

    #[test]
    fn until_a_time_ends_today_or_tomorrow_on_the_local_clock() {
        // 22:13 UTC: 23:00 is later today, 07:30 is tomorrow.
        let later = decide::until(NOW, TimeZone::UTC, at(23, 0));
        assert!(!later.tomorrow);
        assert_eq!(
            later.unix_seconds,
            NOW - (22 * 3600 + 13 * 60 + 20) + 23 * 3600
        );
        let morning = decide::until(NOW, TimeZone::UTC, at(7, 30));
        assert!(morning.tomorrow);
        assert_eq!(
            morning.unix_seconds,
            NOW - (22 * 3600 + 13 * 60 + 20) + 86_400 + 7 * 3600 + 30 * 60
        );
        // In Taipei (UTC+8) it is already 06:13 on the next day, so 07:30 is
        // later today there.
        let taipei = decide::until(NOW, TimeZone::Fixed(8 * 3600), at(7, 30));
        assert!(!taipei.tomorrow);
        assert_eq!(taipei.unix_seconds, morning.unix_seconds - 8 * 3600);
        // The minute it is now is not "later": it means tomorrow.
        let now = decide::until(1_700_000_040 - 40, TimeZone::UTC, at(22, 13));
        assert!(now.tomorrow);
        let w = page_copy(Locale::EnUs);
        assert_eq!(
            decide::until_caption(morning, at(7, 30), w),
            "Tomorrow at 07:30"
        );
        assert_eq!(
            decide::until_caption(taipei, at(7, 30), page_copy(Locale::ZhTw)),
            "今天 07:30"
        );
    }

    /// The ✱ row: a session started from "Until a time" ends at that time,
    /// against the service's own clock seam, through the request the page
    /// sends.
    #[tokio::test]
    async fn a_session_started_until_a_time_ends_at_that_time() {
        use awake_service::backend::{FakeInhibitorBackend, FixedClock};
        let directory = tempfile::tempdir().unwrap();
        let clock = std::sync::Arc::new(FixedClock::at(NOW));
        let engine = awake_service::AwakeEngine::start_in(
            FakeInhibitorBackend::logind_shaped(),
            directory.path(),
            clock.clone(),
        )
        .await;
        let until = decide::until(NOW, TimeZone::UTC, at(23, 30));
        let body = decide::start_with_end(
            &QuickSessions::default(),
            WireEnd::UntilUnixSeconds {
                unix_seconds: until.unix_seconds,
            },
            "Keep awake",
        );
        engine.handle(awake_ipc::AwakeRequest::new(body)).await;
        assert_eq!(engine.status().await.sessions.len(), 1);
        clock.set(until.unix_seconds - 1);
        engine.tick().await;
        assert_eq!(engine.status().await.sessions.len(), 1);
        clock.set(until.unix_seconds);
        let ended = engine.tick().await.ended;
        assert_eq!(ended.len(), 1);
        assert!(engine.status().await.sessions.is_empty());
        assert!(!engine.holds_inhibitor().await);
    }

    #[test]
    fn the_dialog_opens_on_a_whole_hour_more_than_an_hour_away() {
        // 22:13 → 00:00 (23:13 rounded up to the hour).
        assert_eq!(decide::suggested_until(NOW, TimeZone::UTC), at(0, 0));
        assert_eq!(
            decide::suggested_until(NOW - 13 * 60 - 20, TimeZone::UTC),
            at(23, 0)
        );
    }

    fn presence(installed: bool, enabled: bool) -> Presence {
        Presence {
            installed,
            enabled,
            version: installed.then(|| "0.2.14".to_string()),
        }
    }

    #[test]
    fn the_page_state_follows_spec_section_2() {
        let ok: Result<StatusDocument, ReadError> = Ok(status(true));
        let gone: Result<StatusDocument, ReadError> = Err(ReadError::NotOnBus(
            "org.freedesktop.DBus.Error.ServiceUnknown".into(),
        ));
        assert_eq!(
            Service::decide(None, &presence(true, true)),
            Service::Reading
        );
        assert_eq!(
            Service::decide(Some((&ok, Some(4))), &presence(false, false)),
            Service::Ready,
            "a service that answers is there however it was installed"
        );
        assert_eq!(
            Service::decide(Some((&ok, Some(3))), &presence(true, true)),
            Service::TooOld {
                installed: "0.2.14".to_string()
            }
        );
        assert_eq!(
            Service::decide(Some((&gone, None)), &presence(false, false)),
            Service::NotInstalled
        );
        assert!(matches!(
            Service::decide(Some((&gone, None)), &presence(true, true)),
            Service::NotRunning {
                offers_start: true,
                ..
            }
        ));
        assert!(
            matches!(
                Service::decide(Some((&gone, None)), &presence(true, false)),
                Service::NotRunning {
                    offers_start: false,
                    ..
                }
            ),
            "Start service is not offered for a component the person turned off"
        );
    }

    #[test]
    fn the_top_card_says_what_holds_the_pc_and_offers_one_control() {
        let w = page_copy(Locale::EnUs);
        let c = copy(Locale::EnUs);
        let mut idle = status(true);
        idle.sessions.clear();
        idle.reasons.clear();
        idle.active_rules.clear();
        let view = StatusView::from_status(&idle);
        assert_eq!(Headline::of(&idle, &view), Headline::Nothing);
        assert_eq!(
            Headline::Nothing.words(w, c, TimeZone::UTC).0,
            "Nothing is keeping this PC awake"
        );

        let two = status_with_two_reasons();
        let headline = Headline::of(&two, &StatusView::from_status(&two));
        assert!(matches!(headline, Headline::Manual { session_id: 7, .. }));

        let mut rules_only = status_with_two_reasons();
        rules_only
            .sessions
            .retain(|session| session.origin == SessionOrigin::Trigger);
        rules_only
            .reasons
            .retain(|reason| reason.origin == SessionOrigin::Trigger);
        let view = StatusView::from_status(&rules_only);
        let headline = Headline::of(&rules_only, &view);
        assert_eq!(
            headline,
            Headline::RulesOnly {
                rule: "Video call".to_string()
            }
        );
        assert_eq!(
            headline.words(w, c, TimeZone::UTC).1,
            "Started by the rule \"Video call\""
        );

        idle.rules_suppression = Some(WireSuppression::PausedUntil {
            unix_seconds: NOW + 900,
        });
        let paused = Headline::of(&idle, &StatusView::from_status(&idle));
        assert_eq!(
            paused.words(w, c, TimeZone::UTC).0,
            "Automatic rules are paused until 22:28"
        );
        idle.rules_suppression = Some(WireSuppression::Overridden);
        assert_eq!(
            Headline::of(&idle, &StatusView::from_status(&idle)),
            Headline::Overridden
        );
        idle.indicator = WireIndicator::AttentionRequired;
        assert!(matches!(
            Headline::of(&idle, &StatusView::from_status(&idle)),
            Headline::Attention { .. }
        ));
    }

    #[test]
    fn a_reason_s_menu_replaces_the_five_buttons_and_offers_only_what_applies() {
        let view = StatusView::from_status(&status_with_two_reasons());
        let manual = &view.reasons[0];
        let rule = &view.reasons[1];
        let manual_actions = decide::reason_actions(manual);
        match manual.remaining {
            Some(WireRemaining::Seconds { .. }) => assert_eq!(
                manual_actions,
                vec![
                    ReasonAction::Extend(15),
                    ReasonAction::Extend(30),
                    ReasonAction::Extend(60),
                    ReasonAction::Modify,
                    ReasonAction::End
                ]
            ),
            _ => assert_eq!(
                manual_actions,
                vec![ReasonAction::Modify, ReasonAction::End]
            ),
        }
        assert_eq!(decide::reason_actions(rule), vec![ReasonAction::End]);
        let w = page_copy(Locale::ZhTw);
        assert_eq!(
            decide::reason_action_label(ReasonAction::Extend(30), w),
            "延長 30 分鐘"
        );
        assert_eq!(
            decide::reason_action_label(ReasonAction::End, w),
            "結束這個原因"
        );
    }

    #[test]
    fn the_split_button_starts_the_default_length_with_the_defaults() {
        let mut offer = QuickSessions::default();
        offer.defaults.battery_stop_percent = Some(30);
        offer.defaults.policy.prevent_display_sleep = true;
        let RequestBody::StartSession {
            policy,
            battery_stop_percent,
            end,
            security_confirmed,
            ..
        } = decide::start_request(&offer, offer.default_length().unwrap(), "Keep awake")
        else {
            panic!("a start");
        };
        assert_eq!(end, WireEnd::Duration { seconds: 3_600 });
        assert_eq!(battery_stop_percent, Some(30));
        assert!(policy.prevent_display_sleep);
        assert!(
            security_confirmed,
            "the caution is stated beside the switch that asked for it"
        );
    }

    #[test]
    fn the_quick_lengths_rows_and_the_lengths_left_to_add() {
        let c = copy(Locale::EnUs);
        let offer = QuickSessions::default();
        let rows = decide::length_rows(&offer);
        assert_eq!(rows.len(), 5);
        assert!(rows[3].is_default);
        assert!(!rows[0].can_move_up && rows[0].can_move_down);
        assert!(!rows[4].can_move_down);
        assert!(rows.iter().all(|row| row.can_remove));
        let addable = decide::addable_lengths(&offer);
        assert!(!addable.contains(&QuickLength::Minutes { minutes: 15 }));
        assert!(addable.contains(&QuickLength::Minutes { minutes: 45 }));
        assert!(!addable.contains(&QuickLength::Indefinite));
        assert_eq!(
            decide::length_label(QuickLength::Minutes { minutes: 180 }, Locale::EnUs),
            "3 hours"
        );
        assert_eq!(
            decide::length_label(QuickLength::Minutes { minutes: 60 }, Locale::EnUs),
            "1 hour"
        );
        let _ = c;
        assert_eq!(
            decide::length_label(QuickLength::Minutes { minutes: 45 }, Locale::EnUs),
            "45 minutes"
        );
        let full = QuickSessions {
            lengths: (1..=8)
                .map(|minutes| QuickLength::Minutes { minutes })
                .collect(),
            default_index: 0,
            ..QuickSessions::default()
        };
        assert!(decide::addable_lengths(&full).is_empty());
        let one = QuickSessions {
            lengths: vec![QuickLength::Indefinite],
            default_index: 0,
            ..QuickSessions::default()
        };
        assert!(!decide::length_rows(&one)[0].can_remove);
    }

    #[test]
    fn the_import_notice_is_said_in_words() {
        let w = page_copy(Locale::EnUs);
        let document = |notice: Option<&str>| awake_ipc::QuickSessionsDocument {
            quick_sessions: QuickSessions::default(),
            import_notice: notice.map(str::to_string),
            writable: true,
        };
        assert_eq!(decide::import_notice(&document(None), w), None);
        assert!(
            decide::import_notice(
                &document(Some("awake.quick_sessions.import_newer_schema:3")),
                w
            )
            .unwrap()
            .contains("(version 3)")
        );
        assert_eq!(
            decide::import_notice(&document(Some("awake.quick_sessions.import_unreadable")), w)
                .as_deref(),
            Some(w.import_unreadable)
        );
    }

    #[test]
    fn the_sub_pages_have_ctrl_shortcuts_written_as_the_audit_asks() {
        assert_eq!(
            SubPage::ALL.map(SubPage::shortcut),
            ["Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4"].map(str::to_string)
        );
        assert_eq!(
            SubPage::for_key("3", true, false, false),
            Some(SubPage::History)
        );
        assert_eq!(SubPage::for_key("3", false, false, false), None);
        assert_eq!(SubPage::for_key("5", true, false, false), None);
        let w = page_copy(Locale::ZhTw);
        assert_eq!(
            SubPage::ALL.map(|page| page.title(w)),
            ["狀態", "規則", "紀錄", "診斷"]
        );
    }

    #[test]
    fn modify_changes_the_length_from_now_or_to_a_time() {
        let offer = QuickSessions::default();
        let draft = decide::ModifyDraft {
            session_id: 4,
            length: Some(1),
            until: at(23, 0),
            policy: SessionPolicy::manual_default(),
            battery_stop_percent: None,
        };
        let RequestBody::ChangeSession {
            end, session_id, ..
        } = decide::modify_request(&draft, &offer, "Build", NOW, TimeZone::UTC)
        else {
            panic!("a change");
        };
        assert_eq!(session_id, 4);
        assert_eq!(end, WireEnd::Duration { seconds: 900 });
        let until = decide::ModifyDraft {
            length: None,
            ..draft
        };
        let RequestBody::ChangeSession { end, .. } =
            decide::modify_request(&until, &offer, "Build", NOW, TimeZone::UTC)
        else {
            panic!("a change");
        };
        assert_eq!(
            end,
            WireEnd::UntilUnixSeconds {
                unix_seconds: decide::until(NOW, TimeZone::UTC, at(23, 0)).unix_seconds
            }
        );
    }

    fn snapshot(facts: Facts) -> Snapshot {
        Snapshot {
            keep_awake: facts,
            ..Snapshot::empty(Locale::EnUs)
        }
    }

    #[test]
    fn home_hears_about_a_stopped_service_an_active_session_and_a_backend_that_cannot() {
        let module = pages::module(Route::KeepAwake);
        assert!((module.attention)(&snapshot(Facts::Quiet)).is_empty());

        let stopped = (module.attention)(&snapshot(Facts::Stopped { offers_start: true }));
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].header, "The keep-awake service isn't running");
        assert_eq!(stopped[0].action_label, "Start service");
        assert_eq!(stopped[0].action, AttentionAction::StartKeepAwakeService);
        assert_eq!(
            (module.attention)(&snapshot(Facts::Stopped {
                offers_start: false
            }))[0]
                .action,
            AttentionAction::Open(crate::model::Page::KeepAwake)
        );

        let running = Facts::Running {
            active: Some("Build, 45 min left".to_string()),
            manual: true,
            cannot_stop_sleep: Some("x".to_string()),
        };
        let items = (module.attention)(&snapshot(running.clone()));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].header, "Keep awake can't stop sleep");
        assert_eq!(items[0].action, AttentionAction::OpenKeepAwakeDiagnostics);
        assert_eq!(items[1].header, "This PC is being kept awake");
        assert_eq!(items[1].description, "Build, 45 min left");
        // The session is informational; it never takes "Everything's fine" away.
        assert!(crate::attention::everything_fine(&items[1..]));

        assert_eq!((module.badge)(&snapshot(running)), Some(NavBadge::Dot));
        assert_eq!(
            (module.badge)(&snapshot(Facts::Running {
                active: None,
                manual: false,
                cannot_stop_sleep: None
            })),
            None
        );
    }

    #[test]
    fn the_facts_name_the_first_reason_and_its_time() {
        let w = page_copy(Locale::EnUs);
        let c = copy(Locale::EnUs);
        let two = status_with_two_reasons();
        let Facts::Running {
            active,
            manual,
            cannot_stop_sleep,
        } = decide::facts(&Service::Ready, Some(&two), w, c)
        else {
            panic!("running");
        };
        assert!(manual);
        assert!(active.unwrap().starts_with("Rendering the export"));
        assert_eq!(cannot_stop_sleep, None);
        assert_eq!(
            decide::facts(
                &Service::NotRunning {
                    offers_start: true,
                    details: String::new()
                },
                None,
                w,
                c
            ),
            Facts::Stopped { offers_start: true }
        );
        assert_eq!(
            decide::facts(&Service::NotInstalled, None, w, c),
            Facts::Quiet
        );
    }

    #[test]
    fn the_page_is_built_and_its_cards_are_findable_in_both_languages() {
        assert_eq!(
            pages::module(Route::KeepAwake).status,
            pages::PageStatus::Built
        );
        for (locale, query, card) in [
            (Locale::EnUs, "quick session", super::super::card::QUICK),
            (Locale::ZhTw, "快速工作階段", super::super::card::QUICK),
            (Locale::EnUs, "lid closed", super::super::card::LID_LIMIT),
            (Locale::ZhTw, "闔上螢幕", super::super::card::DEFAULT_LID),
            (Locale::EnUs, "history", super::super::card::HISTORY),
        ] {
            let index = pages::search_index(locale);
            let found = crate::search::find(&index, query);
            assert!(
                found
                    .iter()
                    .any(|entry| entry.card == card && entry.route == Route::KeepAwake),
                "{locale:?} {query} → {found:?}"
            );
        }
        assert_eq!(
            super::super::sub_page_of(super::super::card::HISTORY),
            SubPage::History
        );
        assert_eq!(
            super::super::sub_page_of(super::super::card::QUICK),
            SubPage::Status
        );
    }

    #[test]
    fn every_page_word_exists_in_both_languages_and_differs() {
        let en = page_copy(Locale::EnUs);
        let zh = page_copy(Locale::ZhTw);
        for (english, chinese) in [
            (en.keep_awake, zh.keep_awake),
            (en.until_title, zh.until_title),
            (en.quick_header, zh.quick_header),
            (en.defaults_group, zh.defaults_group),
            (en.battery_group, zh.battery_group),
            (en.rules_empty, zh.rules_empty),
            (en.history_empty, zh.history_empty),
            (en.home_stopped_header, zh.home_stopped_header),
            (en.import_newer_schema, zh.import_newer_schema),
        ] {
            assert!(!english.is_empty() && !chinese.is_empty());
            assert_ne!(english, chinese);
        }
        // The spec's exact words.
        assert_eq!(
            zh.quick_description,
            "系統匣選單和「保持清醒」按鈕提供的長度。"
        );
        assert_eq!(en.nothing_holds_header, "Nothing is keeping this PC awake");
        assert_eq!(zh.until_title, "保持清醒到…");
        assert_eq!(
            en.delete_title.replace("{rule}", "Build"),
            "Delete \"Build\"?"
        );
    }

    #[test]
    fn history_rows_read_as_the_table_s_five_columns() {
        let c = copy(Locale::EnUs);
        let entry = awake_ipc::WireHistoryEntry {
            session_id: 1,
            started_at_unix_seconds: NOW,
            ended_at_unix_seconds: Some(NOW + 600),
            origin: SessionOrigin::Manual,
            rule_id: None,
            reasons: vec!["Build".to_string()],
            effective_policy: SessionPolicy::default(),
            battery_stop_percent: None,
            end_cause: Some("expired".to_string()),
            backend_failure: None,
            battery_stop_percent_at_stop: None,
        };
        let rows = vec![super::super::model::HistoryRow::from_wire(&entry)];
        let cells = decide::history_cells(&rows, c, TimeZone::UTC);
        assert_eq!(cells[0][0], "2023-11-14 22:13");
        assert_eq!(cells[0][1], "2023-11-14 22:23");
        assert_eq!(cells[0][3], "Build");
        assert!(!cells[0][4].is_empty());
        assert_eq!(decide::HISTORY_SORTABLE, [0, 1]);
    }

    #[test]
    fn the_default_switches_change_their_own_field() {
        let mut policy = SessionPolicy::default();
        for field in decide::DefaultField::SWITCHES {
            field.set(&mut policy, true);
            assert!(field.get(&policy));
        }
        assert!(decide::DefaultField::DisplaySleep.warns(true));
        assert!(!decide::DefaultField::SystemSleep.warns(true));
        assert!(!decide::DefaultField::AutomaticLock.warns(false));
        assert_eq!(
            decide::lid_limit_index(decide::lid_limit(Some(120))),
            Some(2)
        );
        assert_eq!(decide::lid_limit_index(decide::lid_limit(None)), Some(5));
    }
}

// ---------------------------------------------------------------------------
// The page's client against a real service on a private bus
// ---------------------------------------------------------------------------

mod bus {

    use crate::pages::keep_awake::bus::{self, Outcome, Target};
    use awake_ipc::{QuickLength, RequestBody};
    use awake_service::backend::{FakeInhibitorBackend, FixedClock};
    use std::process::{Child, Command, Stdio};

    const NAME: &str = "org.betteros.Awake1ManagerTest";

    struct PrivateBus {
        child: Child,
        address: String,
    }

    impl PrivateBus {
        fn start() -> Option<Self> {
            let mut child = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            use std::io::{BufRead, BufReader};
            let mut address = String::new();
            BufReader::new(child.stdout.take()?)
                .read_line(&mut address)
                .ok()?;
            let address = address.trim().to_string();
            (!address.is_empty()).then_some(Self { child, address })
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn target(address: &str, name: &str) -> Target {
        let address: zbus::Address = address.parse().unwrap();
        Target {
            connection: zbus::blocking::connection::Builder::address(address)
                .unwrap()
                .build()
                .unwrap(),
            name: name.to_string(),
        }
    }

    /// The page reads the service, changes the offer, and sees the change;
    /// a name nobody owns reads as not on the bus.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_page_reads_the_service_and_changes_the_offer() {
        let bus = test_prerequisites::require!(
            "dbus-daemon",
            PrivateBus::start().ok_or("a private dbus-daemon did not start")
        );
        let directory = tempfile::tempdir().unwrap();
        let engine = std::sync::Arc::new(
            awake_service::AwakeEngine::start_in(
                FakeInhibitorBackend::logind_shaped(),
                directory.path(),
                std::sync::Arc::new(FixedClock::at(1_700_000_000)),
            )
            .await,
        );
        let address: zbus::Address = bus.address.parse().unwrap();
        let _service = awake_service::service::serve(
            zbus::connection::Builder::address(address).unwrap(),
            NAME,
            awake_service::AwakeDbusService::new(engine.clone()),
        )
        .await
        .unwrap();

        let address = bus.address.clone();
        let read = tokio::task::spawn_blocking(move || {
            let target = target(&address, NAME);
            let first = bus::read(&target);
            let mut offer = first.offer.clone().unwrap().quick_sessions;
            assert!(offer.add_length(QuickLength::Minutes { minutes: 45 }));
            let changed = bus::perform(
                &target,
                RequestBody::SetQuickSessions {
                    quick_sessions: offer.clone(),
                },
            );
            let second = bus::read(&target);
            let refused = bus::perform(
                &target,
                RequestBody::ExtendSession {
                    session_id: 99,
                    by_seconds: 60,
                },
            );
            let missing = bus::read(&self::target(&address, "org.betteros.Awake1Nobody"));
            (first, changed, second, offer, refused, missing)
        })
        .await
        .unwrap();
        let (first, changed, second, offer, refused, missing) = read;
        assert_eq!(first.version, Some(awake_ipc::PROTOCOL_VERSION));
        assert!(first.status.is_ok());
        assert!(first.rules.is_some() && first.history.is_some());
        assert_eq!(changed, Outcome::Done);
        assert_eq!(second.offer.unwrap().quick_sessions, offer);
        assert!(matches!(refused, Outcome::Refused(key) if key.starts_with("awake.error.")));
        assert!(
            matches!(
                missing.status,
                Err(crate::pages::keep_awake::decide::ReadError::NotOnBus(_))
            ),
            "{:?}",
            missing.status
        );
    }
}

/// Ticket 172: the page shows the service's keys as sentences, and an
/// answer of the wrong kind as a sentence rather than its debug shape.
mod no_key_on_screen {
    use super::super::copy::{copy, page_copy};
    use super::super::decide::Headline;
    use crate::i18n::Locale;
    use better_core::local_time::TimeZone;

    #[test]
    fn attention_reads_as_a_sentence_in_both_languages() {
        let headline = Headline::Attention {
            reason: "awake.backend.denied:no".to_string(),
        };
        let english = headline.words(page_copy(Locale::EnUs), copy(Locale::EnUs), TimeZone::UTC);
        assert_eq!(
            english.1,
            "The system refused to keep the computer awake. (no)"
        );
        let chinese = headline.words(page_copy(Locale::ZhTw), copy(Locale::ZhTw), TimeZone::UTC);
        assert_eq!(chinese.1, "系統拒絕讓電腦保持清醒。（no）");
        let empty = Headline::Attention {
            reason: String::new(),
        };
        assert_eq!(
            empty
                .words(page_copy(Locale::EnUs), copy(Locale::EnUs), TimeZone::UTC)
                .1,
            ""
        );
    }

    #[test]
    fn a_failure_s_details_are_a_sentence_when_they_are_a_key() {
        let c = copy(Locale::EnUs);
        assert_eq!(
            c.details(awake_ipc::UNEXPECTED_REPLY),
            "The keep-awake service answered with something other than what was asked for."
        );
        assert_eq!(
            copy(Locale::ZhTw).details("awake.error.too_many_sessions"),
            "同時執行的工作階段太多了，請先結束一個再開始新的。"
        );
        // The system's own message stays as it is: the error report is where
        // someone diagnosing the failure reads it.
        assert_eq!(
            c.details("org.freedesktop.DBus.Error.NoReply: timed out"),
            "org.freedesktop.DBus.Error.NoReply: timed out"
        );
    }
}
