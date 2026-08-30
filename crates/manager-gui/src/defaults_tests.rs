//! What the Defaults screens are allowed to say, asserted without a window.
//!
//! The data these build is hand-made rather than read from a desktop: the
//! engine's own behaviour is `defaults-core`'s to prove, and what is at stake
//! here is what the screen does with the answer.

use std::collections::BTreeMap;

use better_core::defaults::{
    DefaultsValue, IntegrationId, IntegrationKind, ObservedValue, SessionEffect,
};
use better_core::{ComponentIcon, ComponentId};
use defaults_core::{
    AggregateState, ComponentDefaults, DefaultsOutcome, DefaultsPlan, DefaultsReport, EntryOutcome,
    EntryResult, IntegrationState, IntegrationStatus, PlanAction, PlanEntry, PlanKind, PlanWarning,
    SkipReason,
};
use defaults_store::{RestoreState, Snapshot, SnapshotEntry, SnapshotStore, SystemIdentity};

use crate::defaults_model::{
    ApprovedPlan, DefaultsSummary, PrimaryAction, RestoreClass, ResultTone, ReviewModel,
    SecondaryAction, aggregate_label, component_rows, integration_state_label, kind_label,
    last_verified_times, observed_label, outcome_headline, relative_time, result_rows,
    session_effect_label, skip_reason_label, value_label, warning_label,
};
use crate::i18n::{Locale, copy};
use crate::layout::{ActionLayout, MIN_WINDOW_WIDTH, action_layout};

fn component(value: &str) -> ComponentId {
    ComponentId::new(value).expect("a valid component id")
}

fn integration(value: &str) -> IntegrationId {
    IntegrationId::new(value).expect("a valid integration id")
}

fn desktop(value: &str) -> DefaultsValue {
    DefaultsValue::DesktopEntry(value.to_string())
}

fn observed(value: &str) -> ObservedValue {
    ObservedValue::Set {
        value: desktop(value),
    }
}

fn status(id: &str, state: IntegrationState, current: ObservedValue) -> IntegrationStatus {
    IntegrationStatus {
        integration: integration(id),
        kind: IntegrationKind::ApplicationHandler,
        state,
        current,
        desired: desktop("io.betteros.Files.desktop"),
        session_effect: SessionEffect::Immediate,
        restore_available: false,
        last_verified_value: None,
    }
}

fn report_of(components: Vec<ComponentDefaults>) -> DefaultsReport {
    DefaultsReport {
        components,
        damaged_snapshots: Vec::new(),
    }
}

fn defaults_of(id: &str, integrations: Vec<IntegrationStatus>) -> ComponentDefaults {
    ComponentDefaults {
        component: component(id),
        aggregate: AggregateState::derive(&integrations),
        integrations,
    }
}

fn rows_of(report: &DefaultsReport) -> Vec<crate::defaults_model::DefaultsRow> {
    component_rows(
        Locale::EnUs,
        report,
        &BTreeMap::new(),
        &|component| component.to_string(),
        &|_| ComponentIcon::Generic,
    )
}

fn entry(component_id: &str, integration_id: &str, action: PlanAction) -> PlanEntry {
    PlanEntry {
        component: component(component_id),
        integration: integration(integration_id),
        kind: IntegrationKind::ApplicationHandler,
        adapter: better_core::AdapterId::XdgDefaultApp,
        action,
        current: observed("org.gnome.Nautilus.desktop"),
        desired: desktop("io.betteros.Files.desktop"),
        captured_previous: Some(observed("org.gnome.Nautilus.desktop")),
        session_effect: SessionEffect::Immediate,
        requires_confirmation: false,
        confirmed: false,
        warnings: Vec::new(),
    }
}

fn applying(component_id: &str, integration_id: &str) -> PlanEntry {
    entry(
        component_id,
        integration_id,
        PlanAction::Apply {
            to: desktop("io.betteros.Files.desktop"),
        },
    )
}

fn skipping(component_id: &str, integration_id: &str, reason: SkipReason) -> PlanEntry {
    entry(component_id, integration_id, PlanAction::Skip { reason })
}

fn review_of(kind: PlanKind, entries: Vec<PlanEntry>) -> ReviewModel {
    ReviewModel::new(
        Locale::EnUs,
        DefaultsPlan::new(kind, entries, Vec::new()),
        &|component| component.to_string(),
    )
}

/// Every aggregate state, so a test cannot silently cover seven of them.
fn every_aggregate_state() -> Vec<AggregateState> {
    vec![
        AggregateState::Default,
        AggregateState::NotDefault,
        AggregateState::PartiallyDefault,
        AggregateState::ChangedExternally,
        AggregateState::Unavailable {
            reason: "defaults.requires_administrator".to_string(),
        },
        AggregateState::Conflict {
            claimant: component("better-monitor"),
        },
        AggregateState::Unknown {
            reason: "defaults.effective_value_unknown".to_string(),
        },
        AggregateState::NeedsSignOut,
    ]
}

#[test]
fn all_eight_aggregate_states_render_distinctly_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let labels: Vec<&str> = every_aggregate_state()
            .iter()
            .map(|state| aggregate_label(locale, state))
            .collect();
        for label in &labels {
            assert!(!label.trim().is_empty(), "{locale:?} is missing a state");
        }
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            labels.len(),
            "{locale:?} renders two aggregate states the same way"
        );
    }
}

#[test]
fn the_primary_action_never_offers_a_meaningless_switch() {
    assert_eq!(
        PrimaryAction::of(&AggregateState::Default),
        PrimaryAction::AlreadyDefault
    );
    for state in [
        AggregateState::NotDefault,
        AggregateState::PartiallyDefault,
        AggregateState::ChangedExternally,
    ] {
        assert_eq!(PrimaryAction::of(&state), PrimaryAction::MakeDefault);
    }
    for state in [
        AggregateState::Unavailable {
            reason: "r".to_string(),
        },
        AggregateState::Conflict {
            claimant: component("better-monitor"),
        },
        AggregateState::Unknown {
            reason: "r".to_string(),
        },
        AggregateState::NeedsSignOut,
    ] {
        assert_eq!(PrimaryAction::of(&state), PrimaryAction::Verify);
    }
}

#[test]
fn a_partial_state_keeps_both_integrations_visible() {
    let report = report_of(vec![defaults_of(
        "better-files",
        vec![
            status(
                "default-file-manager",
                IntegrationState::Default,
                observed("io.betteros.Files.desktop"),
            ),
            status(
                "archive-handler-group",
                IntegrationState::NotDefault,
                observed("org.gnome.FileRoller.desktop"),
            ),
        ],
    )]);
    let rows = rows_of(&report);
    let row = &rows[0];

    assert_eq!(row.aggregate, AggregateState::PartiallyDefault);
    assert_eq!(row.integrations.len(), 2);
    assert_ne!(
        row.integrations[0].current_owner,
        row.integrations[1].current_owner
    );
    // The row above them must not pick one of the two owners and present it as
    // the answer.
    assert_eq!(row.current_owner, copy(Locale::EnUs).value_mixed);
}

#[test]
fn a_row_offers_restore_only_when_a_previous_value_was_saved() {
    let mut with_snapshot = status(
        "default-file-manager",
        IntegrationState::Default,
        observed("io.betteros.Files.desktop"),
    );
    with_snapshot.restore_available = true;
    let report = report_of(vec![
        defaults_of("with-snapshot", vec![with_snapshot]),
        defaults_of(
            "without-snapshot",
            vec![status(
                "default-file-manager",
                IntegrationState::NotDefault,
                observed("org.gnome.Nautilus.desktop"),
            )],
        ),
    ]);
    let rows = rows_of(&report);

    assert!(rows[0].restore_available);
    assert!(
        rows[0]
            .secondary
            .contains(&SecondaryAction::RestorePreviousDefault)
    );
    assert!(!rows[1].restore_available);
    assert!(
        !rows[1]
            .secondary
            .contains(&SecondaryAction::RestorePreviousDefault)
    );
}

#[test]
fn the_summary_counts_defaults_changeable_and_externally_changed() {
    let report = report_of(vec![
        defaults_of(
            "a",
            vec![status(
                "i",
                IntegrationState::Default,
                observed("io.betteros.Files.desktop"),
            )],
        ),
        defaults_of(
            "b",
            vec![status(
                "i",
                IntegrationState::NotDefault,
                observed("org.gnome.Nautilus.desktop"),
            )],
        ),
        defaults_of(
            "c",
            vec![status(
                "i",
                IntegrationState::ChangedExternally { last_known: None },
                observed("org.gnome.Nautilus.desktop"),
            )],
        ),
    ]);

    let summary = DefaultsSummary::of(&report);
    assert_eq!(summary.total, 3);
    assert_eq!(summary.are_default, 1);
    assert_eq!(summary.can_change, 2);
    assert_eq!(summary.changed_externally, 1);
}

#[test]
fn every_eligible_component_starts_selected_and_can_be_unchecked() {
    let review = review_of(
        PlanKind::Apply,
        vec![
            applying("better-files", "one"),
            applying("better-monitor", "two"),
        ],
    );
    assert!(review.is_selected(&component("better-files")));
    assert!(review.is_selected(&component("better-monitor")));

    let mut narrowed = review.clone();
    narrowed.toggle(&component("better-monitor"));
    let approved = narrowed.approve().expect("one component is still selected");
    let touched: Vec<&ComponentId> = approved
        .plan()
        .entries
        .iter()
        .map(|entry| &entry.component)
        .collect();
    assert_eq!(touched, vec![&component("better-files")]);
    assert_eq!(narrowed.summary().components_selected, 1);
}

#[test]
fn a_component_with_nothing_to_do_is_listed_but_not_preselected() {
    let review = review_of(
        PlanKind::Apply,
        vec![
            applying("better-files", "one"),
            skipping("better-monitor", "two", SkipReason::AlreadyDefault),
        ],
    );
    let components = review.components();

    assert_eq!(components.len(), 2);
    assert!(components.iter().any(|entry| entry.changes == 0));
    assert!(!review.is_selected(&component("better-monitor")));
}

#[test]
fn deselecting_everything_leaves_nothing_that_could_be_applied() {
    let mut review = review_of(PlanKind::Apply, vec![applying("better-files", "one")]);
    review.toggle(&component("better-files"));

    assert!(review.approve().is_none());
    assert_eq!(review.summary().components_selected, 0);
}

#[test]
fn a_plan_with_nothing_to_change_cannot_be_approved() {
    let review = review_of(
        PlanKind::Apply,
        vec![skipping("better-files", "one", SkipReason::AlreadyDefault)],
    );
    assert!(review.approve().is_none());
}

/// The milestone signal: nothing runs a plan except the one function that takes
/// a plan a review screen produced.
///
/// The type system carries most of this — [`ApprovedPlan`]'s field is private
/// and [`ReviewModel::approve`] is its only constructor — but the type system
/// cannot say that no second call site exists, so the source is checked too.
#[test]
fn no_code_path_executes_a_plan_without_a_review() {
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut call_sites = Vec::new();
    for file in std::fs::read_dir(&source_root).expect("the crate's sources") {
        let path = file.expect("a readable entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        // The scan cannot count itself: this file quotes the pattern it looks
        // for, and so does the crate's other test module if it ever does.
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("_tests.rs") || name == "tests.rs")
        {
            continue;
        }
        let body = std::fs::read_to_string(&path).expect("a readable source file");
        for line in body.lines() {
            if line.contains(".execute(") {
                call_sites.push((
                    path.file_name().unwrap().to_string_lossy().to_string(),
                    line.trim().to_string(),
                ));
            }
        }
    }

    assert_eq!(
        call_sites.len(),
        1,
        "a defaults plan is executed from more than one place: {call_sites:?}"
    );
    assert_eq!(call_sites[0].0, "defaults_app.rs");
    let body = std::fs::read_to_string(source_root.join("defaults_app.rs"))
        .expect("a readable source file");
    let guarded = body
        .split("impl ApprovedPlan {")
        .nth(1)
        .expect("the execution path must hang off ApprovedPlan");
    assert!(
        guarded.contains(".execute("),
        "the execution path moved out of ApprovedPlan"
    );
}

#[test]
fn an_approved_plan_carries_only_the_selected_components() {
    let mut review = review_of(
        PlanKind::Restore,
        vec![
            entry(
                "better-files",
                "one",
                PlanAction::Restore {
                    to: observed("org.gnome.Nautilus.desktop"),
                },
            ),
            entry(
                "better-monitor",
                "two",
                PlanAction::Restore {
                    to: observed("gnome-system-monitor.desktop"),
                },
            ),
        ],
    );
    review.toggle(&component("better-files"));
    let approved: ApprovedPlan = review.approve().expect("one component is still selected");

    assert_eq!(approved.plan().kind, PlanKind::Restore);
    assert_eq!(approved.plan().entries.len(), 1);
    assert_eq!(
        approved.plan().entries[0].component,
        component("better-monitor")
    );
}

#[test]
fn the_bottom_summary_counts_what_the_screen_shows() {
    let mut sign_out = applying("better-files", "one");
    sign_out.session_effect = SessionEffect::SignOut;
    let mut uncaptured = applying("better-files", "two");
    uncaptured.captured_previous = None;
    let review = review_of(
        PlanKind::Apply,
        vec![
            sign_out,
            uncaptured,
            skipping(
                "better-files",
                "three",
                SkipReason::NoProductionAdapter {
                    adapter: better_core::AdapterId::GnomeKeybinding,
                },
            ),
            skipping(
                "better-files",
                "four",
                SkipReason::ChangedExternallyWithoutConfirmation {
                    current: observed("org.gnome.Nautilus.desktop"),
                },
            ),
        ],
    );

    let summary = review.summary();
    assert_eq!(summary.components_selected, 1);
    assert_eq!(summary.settings_affected, 2);
    assert_eq!(summary.needs_sign_out, 1);
    assert_eq!(summary.will_capture, 1);
    assert_eq!(summary.manual_actions, 1);
    assert_eq!(summary.awaiting_confirmation, 1);
}

#[test]
fn an_entry_changed_elsewhere_needs_its_own_confirmation() {
    let mut held_back = skipping(
        "better-files",
        "one",
        SkipReason::ChangedExternallyWithoutConfirmation {
            current: observed("org.gnome.Nautilus.desktop"),
        },
    );
    held_back.requires_confirmation = true;
    let mut review = review_of(PlanKind::Restore, vec![held_back]);

    let entries = &review.components()[0].entries;
    assert!(entries[0].requires_confirmation);
    assert!(!entries[0].confirmed);
    assert_eq!(entries[0].restore_class, RestoreClass::ChangedExternally);
    assert!(
        review.approve().is_none(),
        "a held-back entry changes nothing"
    );

    review.toggle_confirmation(&component("better-files"), &integration("one"));
    assert!(review.is_confirmed(&component("better-files"), &integration("one")));
    assert_eq!(
        review.confirmed_entries(),
        vec![(component("better-files"), integration("one"))]
    );
    // Confirming one entry confirms nothing else.
    assert!(!review.is_confirmed(&component("better-files"), &integration("two")));
}

#[test]
fn the_restore_screen_keeps_its_five_outcomes_apart() {
    let classes = [
        (
            PlanAction::Restore {
                to: observed("org.gnome.Nautilus.desktop"),
            },
            RestoreClass::Safe,
        ),
        (
            PlanAction::Skip {
                reason: SkipReason::AlreadyRestored,
            },
            RestoreClass::AlreadyRestored,
        ),
        (
            PlanAction::Skip {
                reason: SkipReason::ChangedExternallyWithoutConfirmation {
                    current: observed("org.gnome.Nautilus.desktop"),
                },
            },
            RestoreClass::ChangedExternally,
        ),
        (
            PlanAction::Skip {
                reason: SkipReason::NothingCaptured,
            },
            RestoreClass::NothingCaptured,
        ),
        (
            PlanAction::Skip {
                reason: SkipReason::RequiresAdministrator,
            },
            RestoreClass::ManualAction,
        ),
    ];
    for (action, expected) in classes {
        assert_eq!(
            RestoreClass::of(&entry("better-files", "one", action)),
            expected
        );
    }

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let mut labels: Vec<&str> = [
            RestoreClass::Safe,
            RestoreClass::AlreadyRestored,
            RestoreClass::ChangedExternally,
            RestoreClass::NothingCaptured,
            RestoreClass::ManualAction,
        ]
        .into_iter()
        .map(|class| class.label(locale))
        .collect();
        let total = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), total, "{locale:?} renders two classes alike");
    }
}

#[test]
fn the_restore_screen_shows_the_exact_captured_value() {
    let review = review_of(
        PlanKind::Restore,
        vec![entry(
            "better-files",
            "one",
            PlanAction::Restore {
                to: observed("org.gnome.Nautilus.desktop"),
            },
        )],
    );
    let entries = &review.components()[0].entries;

    assert_eq!(entries[0].new_owner, "org.gnome.Nautilus.desktop");
    assert_eq!(
        entries[0].captured_previous.as_deref(),
        Some("org.gnome.Nautilus.desktop")
    );
}

#[test]
fn no_change_in_a_plan_ever_asks_for_administrator_access() {
    let review = review_of(
        PlanKind::Apply,
        vec![
            applying("better-files", "one"),
            skipping("better-files", "two", SkipReason::RequiresAdministrator),
        ],
    );
    let elevation = review.elevation();

    assert!(
        !elevation.requested,
        "no adapter escalates, so the preview must not say one might"
    );
    assert_eq!(elevation.excluded_needing_administrator, 1);
}

#[test]
fn every_result_outcome_has_its_own_words_in_both_locales() {
    let outcomes = vec![
        EntryOutcome::Applied {
            value: desktop("io.betteros.Files.desktop"),
        },
        EntryOutcome::AppliedNeedsSignOut {
            value: desktop("io.betteros.Files.desktop"),
        },
        EntryOutcome::Restored {
            value: observed("org.gnome.Nautilus.desktop"),
        },
        EntryOutcome::AlreadyCorrect,
        EntryOutcome::NotVerified {
            observed: observed("org.gnome.Nautilus.desktop"),
        },
        EntryOutcome::VerificationInconclusive {
            observed: ObservedValue::Unknown {
                reason: "test".to_string(),
            },
        },
        EntryOutcome::Skipped {
            reason: SkipReason::AlreadyDefault,
        },
        EntryOutcome::ManualActionRequired {
            reason: "dconf.write_not_supported".to_string(),
            detail: None,
        },
        EntryOutcome::Failed {
            reason: "xdg.write_failed".to_string(),
            detail: None,
        },
    ];
    let expected_tones = [
        ResultTone::Success,
        ResultTone::Pending,
        ResultTone::Success,
        ResultTone::Success,
        ResultTone::Failure,
        ResultTone::Warning,
        ResultTone::Neutral,
        ResultTone::Warning,
        ResultTone::Failure,
    ];
    let outcome = DefaultsOutcome {
        kind: PlanKind::Apply,
        results: outcomes
            .iter()
            .enumerate()
            .map(|(index, entry)| EntryResult {
                component: component("better-files"),
                integration: integration(&format!("integration-{index}")),
                outcome: entry.clone(),
            })
            .collect(),
        baseline_snapshot: None,
        recorded_snapshot: None,
    };

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let rows = result_rows(locale, &outcome, &|component| component.to_string());
        assert_eq!(rows.len(), outcomes.len());
        for (row, tone) in rows.iter().zip(expected_tones) {
            assert!(!row.label.trim().is_empty());
            assert_eq!(row.tone, tone);
        }
        // A machine key from an adapter must never reach the screen.
        assert!(
            !rows
                .iter()
                .any(|row| row.label.contains('.') && row.label.contains('_'))
        );
    }
}

#[test]
fn a_partly_successful_run_says_so_rather_than_claiming_success() {
    let outcome = DefaultsOutcome {
        kind: PlanKind::Apply,
        results: vec![
            EntryResult {
                component: component("better-files"),
                integration: integration("one"),
                outcome: EntryOutcome::Applied {
                    value: desktop("io.betteros.Files.desktop"),
                },
            },
            EntryResult {
                component: component("better-files"),
                integration: integration("two"),
                outcome: EntryOutcome::NotVerified {
                    observed: observed("org.gnome.Nautilus.desktop"),
                },
            },
        ],
        baseline_snapshot: Some("snapshot".to_string()),
        recorded_snapshot: None,
    };

    assert!(outcome.has_failures());
    assert_eq!(
        outcome_headline(Locale::EnUs, &outcome),
        copy(Locale::EnUs).result_partial
    );
}

#[test]
fn the_last_verified_time_comes_from_the_newest_snapshot_that_confirmed_it() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let store = SnapshotStore::at_path(directory.path());
    let identity = SystemIdentity {
        distribution: "zorin".to_string(),
        desktop_session: "gnome".to_string(),
    };
    let record = SnapshotEntry {
        component_id: component("better-files"),
        integration_id: integration("default-file-manager"),
        previous_value: observed("org.gnome.Nautilus.desktop"),
        better_value: desktop("io.betteros.Files.desktop"),
        applied_value: Some(desktop("io.betteros.Files.desktop")),
        last_verified_value: Some(desktop("io.betteros.Files.desktop")),
        restore_state: RestoreState::Available,
    };
    let never_verified = SnapshotEntry {
        integration_id: integration("archive-handler-group"),
        last_verified_value: None,
        ..record.clone()
    };
    store
        .write(&Snapshot::new(identity, vec![record, never_verified]))
        .expect("the snapshot must be writable");

    let history = store.history().expect("the history must be readable");
    let times = last_verified_times(&history);
    assert!(times.contains_key(&(
        component("better-files"),
        integration("default-file-manager")
    )));
    assert!(
        !times.contains_key(&(
            component("better-files"),
            integration("archive-handler-group")
        )),
        "an integration nothing ever confirmed must not claim a time"
    );
}

#[test]
fn a_setting_that_was_never_checked_says_so_rather_than_showing_a_time() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        assert_eq!(
            relative_time(locale, None, 1_000_000),
            copy(locale).never_verified
        );
        assert_eq!(
            relative_time(locale, Some(1_000_000), 1_000_010),
            copy(locale).time_just_now
        );
        assert!(relative_time(locale, Some(1_000_000), 1_007_200).contains('2'));
    }
}

#[test]
fn a_reading_the_system_could_not_give_is_words_rather_than_a_machine_key() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        for observed in [
            ObservedValue::Unset,
            ObservedValue::Unknown {
                reason: "xdg.mimeapps_unreadable".to_string(),
            },
            ObservedValue::Unsupported {
                reason: "dconf.no_such_key".to_string(),
            },
            ObservedValue::PermissionDenied {
                reason: "dconf.denied".to_string(),
            },
        ] {
            let label = observed_label(locale, &observed);
            assert!(!label.trim().is_empty());
            assert!(!label.contains('.'), "{label} leaks a machine key");
        }
        assert_eq!(
            value_label(locale, &DefaultsValue::Boolean(true)),
            copy(locale).value_on
        );
        assert_eq!(
            value_label(
                locale,
                &DefaultsValue::TextList(vec!["<Super>e".to_string(), "<Super>f".to_string()])
            ),
            "<Super>e, <Super>f"
        );
    }
}

#[test]
fn every_defaults_word_the_screens_use_exists_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for text in [
            c.defaults,
            c.defaults_title,
            c.defaults_subtitle,
            c.defaults_are_default,
            c.defaults_can_change,
            c.defaults_changed_externally,
            c.defaults_empty_title,
            c.defaults_empty_detail,
            c.defaults_working,
            c.defaults_read_failed,
            c.use_better_defaults,
            c.restore_previous_defaults,
            c.make_default,
            c.verify_again,
            c.apply_selected_defaults,
            c.restore_selected_defaults,
            c.current_owner,
            c.better_target,
            c.integrations_label,
            c.last_verified,
            c.never_verified,
            c.saved_previous_value,
            c.no_saved_previous_value,
            c.defaults_review_title,
            c.defaults_review_subtitle,
            c.restore_review_title,
            c.restore_review_subtitle,
            c.current_value,
            c.new_value,
            c.can_be_restored,
            c.cannot_be_restored,
            c.confirm_overwrite,
            c.overwrites_external_change,
            c.previous_value_indeterminate,
            c.components_selected,
            c.settings_affected,
            c.snapshot_label,
            c.snapshot_will_capture,
            c.snapshot_nothing_to_capture,
            c.manual_action_required,
            c.awaiting_confirmation,
            c.no_elevated_access,
            c.elevated_excluded,
            c.damaged_snapshots,
            c.nothing_to_change,
            c.restore_safe,
            c.defaults_results_title,
            c.defaults_results_subtitle,
            c.result_partial,
        ] {
            assert!(
                !text.trim().is_empty(),
                "{locale:?} is missing defaults copy"
            );
        }
        for kind in IntegrationKind::ALL {
            assert!(!kind_label(locale, kind).trim().is_empty());
        }
        for effect in [
            SessionEffect::Immediate,
            SessionEffect::SignOut,
            SessionEffect::Restart,
        ] {
            assert!(!session_effect_label(locale, effect).trim().is_empty());
        }
        for state in [
            IntegrationState::Default,
            IntegrationState::NotDefault,
            IntegrationState::ChangedExternally { last_known: None },
            IntegrationState::Unavailable {
                reason: "r".to_string(),
            },
            IntegrationState::Conflict {
                claimant: component("better-monitor"),
            },
            IntegrationState::Unknown {
                reason: "r".to_string(),
            },
            IntegrationState::NeedsSignOut,
        ] {
            assert!(!integration_state_label(locale, &state).trim().is_empty());
        }
        for warning in [
            PlanWarning::NeedsSignOut,
            PlanWarning::NeedsRestart,
            PlanWarning::PreviousValueIndeterminate,
            PlanWarning::OverwritesExternalChange {
                current: observed("org.gnome.Nautilus.desktop"),
            },
        ] {
            assert!(!warning_label(locale, &warning).trim().is_empty());
        }
        for reason in [
            SkipReason::AlreadyDefault,
            SkipReason::AlreadyRestored,
            SkipReason::NotApplicableHere,
            SkipReason::PrerequisiteNotMet {
                prerequisite: better_core::HealthPrerequisite::Installed,
            },
            SkipReason::RequiresAdministrator,
            SkipReason::NoProductionAdapter {
                adapter: better_core::AdapterId::GnomeKeybinding,
            },
            SkipReason::NothingCaptured,
            SkipReason::EffectiveValueUnknown {
                reason: "r".to_string(),
            },
            SkipReason::ChangedExternallyWithoutConfirmation {
                current: ObservedValue::Unset,
            },
            SkipReason::Conflict {
                claimant: component("better-monitor"),
            },
        ] {
            assert!(!skip_reason_label(locale, &reason).trim().is_empty());
        }
    }
}

#[test]
fn the_traditional_chinese_terms_the_issue_fixed_are_the_ones_shown() {
    let c = copy(Locale::ZhTw);
    assert_eq!(c.defaults, "預設值");
    assert_eq!(c.make_default, "設為預設");
    assert_eq!(c.use_better_defaults, "套用 Better OS 預設值");
    assert_eq!(c.restore_previous_defaults, "恢復先前的預設值");
    assert_eq!(c.state_partially_default, "部分為預設");
    assert_eq!(c.state_changed_externally, "已由其他程式變更");
    assert_eq!(c.state_needs_sign_out, "需要登出後生效");
}

#[test]
fn no_defaults_copy_uses_the_words_the_backend_uses() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for text in [
            c.defaults_review_title,
            c.defaults_review_subtitle,
            c.apply_selected_defaults,
            c.restore_selected_defaults,
            c.snapshot_label,
            c.snapshot_will_capture,
            c.defaults_results_title,
        ] {
            let lowered = text.to_ascii_lowercase();
            for jargon in ["commit", "transaction", "adapter", "snapshot", "plan entry"] {
                assert!(!lowered.contains(jargon), "{text} uses backend words");
            }
        }
    }
}

#[test]
fn the_defaults_actions_wrap_at_every_supported_scale() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let longest = [
            c.use_better_defaults,
            c.restore_previous_defaults,
            c.apply_selected_defaults,
            c.restore_selected_defaults,
            c.make_default,
            c.state_needs_sign_out,
        ]
        .iter()
        .map(|label| label.chars().count())
        .max()
        .expect("the defaults actions are not empty");

        for scale in [1.0, 1.25, 1.5] {
            // At the smallest supported window these labels fit on one line
            // unscaled, and stop fitting as soon as the user scales up, which
            // is what the wrapping action row exists for.
            let expected = if scale == 1.0 {
                ActionLayout::Inline
            } else {
                ActionLayout::Wrapped
            };
            assert_eq!(
                action_layout(MIN_WINDOW_WIDTH, scale, longest),
                expected,
                "{locale:?} at {scale} laid out unexpectedly at the smallest window"
            );
            assert_eq!(
                action_layout(1920.0, scale, longest),
                ActionLayout::Inline,
                "{locale:?} at {scale} must fit on a normal desktop"
            );
            // A longer translation of the same button must wrap rather than
            // run off the edge.
            assert_eq!(
                action_layout(MIN_WINDOW_WIDTH, scale, longest + 40),
                ActionLayout::Wrapped
            );
        }
    }
}

/// The whole path the Defaults screens use, with a window's work left out.
///
/// This runs the same job function the background task runs: read, plan,
/// approve, apply, read again. It uses the schema-coverage fixture rather than
/// the shipped catalog, because no shipped manifest declares an integration
/// yet, and it uses a simulated desktop, so it changes nothing outside its own
/// temporary directory.
mod through_the_engine {
    use super::*;
    use better_core::ComponentManifest;
    use defaults_core::{AdapterMode, ComponentReadiness, Selection, SystemContext};

    use crate::defaults_app::{DefaultsEvent, DefaultsInputs, DefaultsJob, run_job};

    fn fixture() -> ComponentManifest {
        ComponentManifest::parse_yaml(include_str!(
            "../../better-core/tests/fixtures/every-integration-kind.yaml"
        ))
        .expect("the schema-coverage fixture must stay valid")
    }

    fn inputs(directory: &std::path::Path) -> DefaultsInputs {
        let manifest = fixture();
        DefaultsInputs {
            readiness: vec![(manifest.id.clone(), ComponentReadiness::ready())],
            manifests: vec![manifest],
            system: SystemContext::new("zorin", "gnome"),
            snapshot_directory: directory.join("snapshots"),
            mode: AdapterMode::Simulated {
                desktop_path: Some(directory.join("desktop.json")),
            },
        }
    }

    fn report(inputs: &DefaultsInputs) -> DefaultsReport {
        match run_job(inputs, DefaultsJob::Inspect) {
            DefaultsEvent::Report(reading) => reading.report,
            _ => panic!("inspecting must produce a reading"),
        }
    }

    #[test]
    fn reviewing_changes_nothing_and_applying_the_review_changes_everything_it_showed() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let inputs = inputs(directory.path());

        let before = report(&inputs);
        assert_eq!(before.components.len(), 1);
        assert_eq!(
            before.components[0].integrations.len(),
            9,
            "every declared integration is listed separately"
        );
        // One of the nine is declared for a distribution this is not, so the
        // component reads as unavailable while the other eight are simply not
        // the default yet. That is the aggregate hiding nothing.
        assert!(matches!(
            before.components[0].aggregate,
            AggregateState::Unavailable { .. }
        ));
        assert_eq!(
            before.components[0]
                .integrations
                .iter()
                .filter(|status| status.state == IntegrationState::NotDefault)
                .count(),
            8
        );

        let plan = match run_job(
            &inputs,
            DefaultsJob::Plan {
                kind: PlanKind::Apply,
                selection: Selection::All,
                confirmed: Vec::new(),
            },
        ) {
            DefaultsEvent::Planned(plan) => *plan,
            _ => panic!("planning must produce a plan"),
        };
        assert!(plan.changes().count() > 0);

        // Building the preview is not a mutation.
        assert_eq!(report(&inputs), before);

        let review = ReviewModel::new(Locale::EnUs, plan, &|component| component.to_string());
        assert!(!review.elevation().requested);
        let approved = review.approve().expect("the fixture has changes to make");

        let outcome = match run_job(&inputs, DefaultsJob::Run(approved)) {
            DefaultsEvent::Finished(outcome) => *outcome,
            _ => panic!("an approved plan must run"),
        };
        assert!(outcome.succeeded() > 0);
        assert!(
            outcome.baseline_snapshot.is_some(),
            "the previous values are saved before the first change"
        );

        let after = report(&inputs);
        assert_ne!(after, before);
        assert!(
            after.components[0]
                .integrations
                .iter()
                .any(|status| status.state == IntegrationState::Default)
        );
    }

    #[test]
    fn restoring_puts_back_exactly_what_was_saved() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let inputs = inputs(directory.path());
        let before = report(&inputs);

        let plan = match run_job(
            &inputs,
            DefaultsJob::Plan {
                kind: PlanKind::Apply,
                selection: Selection::All,
                confirmed: Vec::new(),
            },
        ) {
            DefaultsEvent::Planned(plan) => *plan,
            _ => panic!("planning must produce a plan"),
        };
        let approved = ReviewModel::new(Locale::EnUs, plan, &|component| component.to_string())
            .approve()
            .expect("the fixture has changes to make");
        run_job(&inputs, DefaultsJob::Run(approved));

        let restore = match run_job(
            &inputs,
            DefaultsJob::Plan {
                kind: PlanKind::Restore,
                selection: Selection::All,
                confirmed: Vec::new(),
            },
        ) {
            DefaultsEvent::Planned(plan) => *plan,
            _ => panic!("planning must produce a plan"),
        };
        let review = ReviewModel::new(Locale::EnUs, restore, &|component| component.to_string());
        let approved = review.approve().expect("something was applied to put back");
        match run_job(&inputs, DefaultsJob::Run(approved)) {
            DefaultsEvent::Finished(outcome) => assert!(outcome.succeeded() > 0),
            _ => panic!("an approved plan must run"),
        }

        let after = report(&inputs);
        for (restored, original) in after.components[0]
            .integrations
            .iter()
            .zip(&before.components[0].integrations)
        {
            assert_eq!(
                restored.current, original.current,
                "{} did not come back to what it was",
                restored.integration
            );
        }
    }
}
