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
use defaults_core::ApplicationChoice;
use defaults_core::{
    AggregateState, ComponentDefaults, DefaultsOutcome, DefaultsPlan, DefaultsReport, EntryOutcome,
    EntryResult, IntegrationState, IntegrationStatus, KeyOutcome, PlanAction, PlanEntry, PlanKind,
    PlanWarning, RestoreChoice, SkipReason,
};
use defaults_store::{RestoreState, Snapshot, SnapshotEntry, SnapshotStore, SystemIdentity};

use crate::i18n::{Locale, copy};
use crate::layout::{ActionLayout, MIN_CONTENT_WIDTH, action_layout};
use crate::pages::default_apps::model::{
    ApprovedPlan, DefaultsSummary, PrimaryAction, RestoreClass, ResultTone, ReviewModel,
    SecondaryAction, aggregate_label, component_rows, integration_state_label, kind_label,
    last_verified_times, observed_label, outcome_headline, relative_time, result_rows,
    session_effect_label, skip_reason_label, value_label, warning_label,
};

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

fn rows_of(report: &DefaultsReport) -> Vec<crate::pages::default_apps::model::DefaultsRow> {
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

// Ticket 207 review finding 4: a refusal to write a database the session
// would not read says why and what to do, in both languages, instead of
// "needs to be done by hand" with nothing beside it.
#[test]
fn a_dconf_refusal_reads_as_its_sentence_not_as_manual_work() {
    let refused = DefaultsOutcome {
        kind: PlanKind::Restore,
        results: vec![EntryResult {
            component: component("better-launcher"),
            integration: integration("launcher-shortcut"),
            outcome: EntryOutcome::ManualActionRequired {
                reason: "dconf.session_profile_not_in_environment".to_string(),
                detail: Some("this process runs in a betteros:cosmic session".to_string()),
            },
        }],
        baseline_snapshot: None,
        recorded_snapshot: None,
    };
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let row = &result_rows(locale, &refused, &|component| component.to_string())[0];
        assert_eq!(row.label, c.result_not_changed);
        assert_eq!(row.tone, ResultTone::Warning);
        let detail = row.detail.clone().unwrap_or_default();
        assert!(!detail.is_empty());
        assert!(!detail.contains("dconf."), "{detail} leaks a machine key");

        let unknown = ObservedValue::Unknown {
            reason: "dconf.profile_missing:the dconf profile \"betteros\" is not at …".to_string(),
        };
        let said = observed_label(locale, &unknown);
        assert_ne!(said, c.value_unknown);
        assert!(!said.contains("dconf."), "{said}");
        let skipped = skip_reason_label(
            locale,
            &SkipReason::EffectiveValueUnknown {
                reason: "dconf.profile_unreadable".to_string(),
            },
        );
        assert_ne!(skipped, c.skip_value_unknown);
    }
    let en = observed_label(
        Locale::EnUs,
        &ObservedValue::Unknown {
            reason: "dconf.session_profile_not_in_environment".to_string(),
        },
    );
    let zh = observed_label(
        Locale::ZhTw,
        &ObservedValue::Unknown {
            reason: "dconf.session_profile_not_in_environment".to_string(),
        },
    );
    assert_ne!(en, zh);
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
    fn sources(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for file in std::fs::read_dir(directory).expect("the crate's sources") {
            let path = file.expect("a readable entry").path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&source_root, &mut files);
    let mut call_sites = Vec::new();
    for path in files {
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
                    path.strip_prefix(&source_root)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
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
    assert_eq!(call_sites[0].0, "pages/default_apps/jobs.rs");
    let body = std::fs::read_to_string(source_root.join("pages/default_apps/jobs.rs"))
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

fn per_type(pairs: &[(&str, ObservedValue)]) -> ObservedValue {
    ObservedValue::Mixed {
        per_key: pairs
            .iter()
            .map(|(key, observed)| better_core::defaults::KeyObservation {
                key: key.to_string(),
                observed: observed.clone(),
            })
            .collect(),
    }
}

#[test]
fn a_group_whose_types_open_in_different_applications_lists_each_owner() {
    let mixed = per_type(&[
        ("image/png", observed("org.gnome.eog.desktop")),
        ("image/jpeg", observed("org.gnome.gThumb.desktop")),
        ("image/webp", ObservedValue::Unset),
    ]);
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let label = observed_label(locale, &mixed);
        assert!(
            label.contains("image/png: org.gnome.eog.desktop"),
            "{label}"
        );
        assert!(
            label.contains("image/jpeg: org.gnome.gThumb.desktop"),
            "{label}"
        );
        assert!(
            label.contains(&format!("image/webp: {}", copy(locale).value_none)),
            "{label}"
        );
    }

    let mut apply = applying("better-files", "image-viewer");
    apply.current = mixed.clone();
    apply.captured_previous = None;
    let review = review_of(PlanKind::Apply, vec![apply]);
    let entry = &review.components()[0].entries[0];
    assert_eq!(entry.current_owner, observed_label(Locale::EnUs, &mixed));
    assert!(entry.changes_something);

    let mut restore = entry_with_capture(mixed.clone());
    restore.action = PlanAction::Restore { to: mixed.clone() };
    let review = review_of(PlanKind::Restore, vec![restore]);
    let entry = &review.components()[0].entries[0];
    assert_eq!(entry.new_owner, observed_label(Locale::EnUs, &mixed));
    assert!(entry.restorable);
    assert_eq!(entry.restore_class, RestoreClass::Safe);
}

fn entry_with_capture(captured: ObservedValue) -> PlanEntry {
    let mut entry = applying("better-files", "image-viewer");
    entry.current = observed("io.betteros.Files.desktop");
    entry.captured_previous = Some(captured);
    entry
}

#[test]
fn a_group_restored_type_by_type_reports_what_each_type_now_reads() {
    let key = |key: &str, outcome: EntryOutcome| defaults_core::KeyOutcome {
        key: key.to_string(),
        outcome,
    };
    let run = |keys: Vec<defaults_core::KeyOutcome>| DefaultsOutcome {
        kind: PlanKind::Restore,
        results: vec![EntryResult {
            component: component("better-files"),
            integration: integration("image-viewer"),
            outcome: EntryOutcome::PerKey { keys },
        }],
        baseline_snapshot: None,
        recorded_snapshot: Some("snapshot".to_string()),
    };
    let restored = |value: &str| EntryOutcome::Restored {
        value: observed(value),
    };

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let all = run(vec![
            key("image/png", restored("org.gnome.eog.desktop")),
            key("image/jpeg", restored("org.gnome.gThumb.desktop")),
        ]);
        let row = &result_rows(locale, &all, &|component| component.to_string())[0];
        assert_eq!(row.label, c.result_restored);
        assert_eq!(row.tone, ResultTone::Success);
        let detail = row.detail.clone().unwrap_or_default();
        assert!(
            detail.contains("image/png: org.gnome.eog.desktop"),
            "{detail}"
        );
        assert!(
            detail.contains("image/jpeg: org.gnome.gThumb.desktop"),
            "{detail}"
        );

        let manual = run(vec![
            key("image/png", restored("org.gnome.eog.desktop")),
            key(
                "image/webp",
                EntryOutcome::ManualActionRequired {
                    reason: "xdg.clearing_a_default_is_not_supported".to_string(),
                    detail: None,
                },
            ),
        ]);
        let row = &result_rows(locale, &manual, &|component| component.to_string())[0];
        assert_eq!(row.label, c.result_partial);
        assert_eq!(row.tone, ResultTone::Warning);
        let detail = row.detail.clone().unwrap_or_default();
        assert!(
            detail.contains(&format!("image/webp: {}", c.manual_action_required)),
            "{detail}"
        );
        assert!(!detail.contains("xdg."), "{detail} leaks a machine key");

        let failed = run(vec![
            key("image/png", restored("org.gnome.eog.desktop")),
            key(
                "image/jpeg",
                EntryOutcome::NotVerified {
                    observed: observed("io.betteros.Files.desktop"),
                },
            ),
        ]);
        let row = &result_rows(locale, &failed, &|component| component.to_string())[0];
        assert_eq!(row.label, c.result_partial);
        assert_eq!(row.tone, ResultTone::Failure);
        assert_eq!(outcome_headline(locale, &failed), c.result_partial);
    }
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
        dconf_database: None,
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
            c.defaults_subtitle,
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
    assert_eq!(
        crate::i18n::shell(Locale::ZhTw).default_apps,
        "預設應用程式"
    );
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
            // In the narrowest content column the control center has (960
            // less the compact pane and margins) these labels fit on one line
            // until the person scales up to 150%, which is what the wrapping
            // action row exists for.
            if scale >= 1.5 {
                assert_eq!(
                    action_layout(MIN_CONTENT_WIDTH, scale, longest),
                    ActionLayout::Wrapped,
                    "{locale:?} at {scale} laid out unexpectedly at the smallest window"
                );
            }
            assert_eq!(
                action_layout(1920.0, scale, longest),
                ActionLayout::Inline,
                "{locale:?} at {scale} must fit on a normal desktop"
            );
            // A longer translation of the same button must wrap rather than
            // run off the edge.
            assert_eq!(
                action_layout(MIN_CONTENT_WIDTH, scale, longest + 80),
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
    use defaults_core::{AdapterMode, ComponentReadiness, Confirmations, Selection, SystemContext};

    use crate::pages::default_apps::jobs::{DefaultsEvent, DefaultsInputs, DefaultsJob, run_job};

    fn fixture() -> ComponentManifest {
        ComponentManifest::parse_yaml(include_str!(
            "../../../../better-core/tests/fixtures/every-integration-kind.yaml"
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
        // One of the nine is declared for a distribution this is not. It is
        // listed as unavailable on its own row, and left out of the
        // component's state (ticket 89): the other eight are simply not the
        // default yet, which is what the component reads as.
        assert_eq!(before.components[0].aggregate, AggregateState::NotDefault);
        assert_eq!(
            before.components[0]
                .integrations
                .iter()
                .filter(|status| matches!(status.state, IntegrationState::Unavailable { .. }))
                .count(),
            1
        );
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
                decisions: Confirmations::none(),
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
                decisions: Confirmations::none(),
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
                decisions: Confirmations::none(),
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
            // A launcher entry is declared leave-in-place: what Better OS
            // added stays where it is when the rest is put back.
            if restored.kind == better_core::defaults::IntegrationKind::DesktopLauncherEntry {
                assert_eq!(restored.state, IntegrationState::Default);
                continue;
            }
            assert_eq!(
                restored.current, original.current,
                "{} did not come back to what it was",
                restored.integration
            );
        }
    }
}

fn alternatives() -> Vec<ApplicationChoice> {
    vec![
        ApplicationChoice {
            desktop_id: "org.gnome.Loupe.desktop".to_string(),
            name: "Image Viewer".to_string(),
        },
        ApplicationChoice {
            desktop_id: "org.gnome.gThumb.desktop".to_string(),
            name: "gThumb".to_string(),
        },
    ]
}

fn missing_eog() -> SkipReason {
    SkipReason::PreviousApplicationMissing {
        desktop_id: "org.gnome.eog.desktop".to_string(),
        alternatives: alternatives(),
    }
}

#[test]
fn a_missing_previous_application_is_its_own_restore_class_in_both_locales() {
    let skipped = entry(
        "better-files",
        "image-viewer",
        PlanAction::Skip {
            reason: missing_eog(),
        },
    );
    assert_eq!(
        RestoreClass::of(&skipped),
        RestoreClass::PreviousApplicationMissing
    );
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let label = RestoreClass::PreviousApplicationMissing.label(locale);
        assert!(!label.trim().is_empty());
        for other in [
            RestoreClass::Safe,
            RestoreClass::AlreadyRestored,
            RestoreClass::ChangedExternally,
            RestoreClass::NothingCaptured,
            RestoreClass::ManualAction,
            RestoreClass::NotApplicable,
        ] {
            assert_ne!(label, other.label(locale), "{locale:?}");
        }
        assert_eq!(skip_reason_label(locale, &missing_eog()), label);
    }
}

#[test]
fn the_review_names_the_missing_application_and_offers_the_others_or_no_default() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let review = ReviewModel::new(
            locale,
            DefaultsPlan::new(
                PlanKind::Restore,
                vec![
                    skipping("better-files", "image-viewer", missing_eog()),
                    entry(
                        "better-files",
                        "file-manager",
                        PlanAction::Restore {
                            to: observed("org.gnome.Nautilus.desktop"),
                        },
                    ),
                ],
                Vec::new(),
            ),
            &|component| component.to_string(),
        );
        let entry = &review.components()[0].entries[0];
        assert_eq!(entry.missing.len(), 1);
        let missing = &entry.missing[0];
        assert_eq!(missing.key, None);
        assert!(
            missing.headline.contains(c.previous_application_missing),
            "{}",
            missing.headline
        );
        assert!(missing.headline.contains("org.gnome.eog.desktop"));
        let labels: Vec<&str> = missing
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Image Viewer", "gThumb", c.clear_default]);
        assert_eq!(
            missing.options[2].choice,
            RestoreChoice::ClearDefault,
            "clearing is always offered"
        );
        assert!(missing.options.iter().all(|option| !option.chosen));
        // Nothing is restored for it, and the summary of the selected component
        // counts it as something for the person to act on.
        assert!(!entry.changes_something);
        assert_eq!(review.summary().manual_actions, 1);
    }
}

#[test]
fn choosing_instead_is_kept_per_entry_and_per_type_and_becomes_the_plan_decision() {
    let mut review = review_of(
        PlanKind::Restore,
        vec![skipping("better-files", "image-viewer", missing_eog())],
    );
    let loupe = RestoreChoice::Application("org.gnome.Loupe.desktop".to_string());
    review.choose_restore(
        &component("better-files"),
        &integration("image-viewer"),
        None,
        loupe.clone(),
    );
    review.choose_restore(
        &component("better-files"),
        &integration("image-viewer"),
        Some("image/png"),
        RestoreChoice::ClearDefault,
    );
    let decisions = review.decisions();
    assert_eq!(
        decisions.restore_choice(
            &component("better-files"),
            &integration("image-viewer"),
            None
        ),
        Some(&loupe)
    );
    assert_eq!(
        decisions.restore_choice(
            &component("better-files"),
            &integration("image-viewer"),
            Some("image/png")
        ),
        Some(&RestoreChoice::ClearDefault)
    );
    let entry = &review.components()[0].entries[0];
    assert!(entry.missing[0].options[0].chosen);
    assert!(!entry.missing[0].options[1].chosen);

    // Choosing the same thing again takes the choice back.
    review.choose_restore(
        &component("better-files"),
        &integration("image-viewer"),
        None,
        loupe,
    );
    assert_eq!(
        review.decisions().restore_choice(
            &component("better-files"),
            &integration("image-viewer"),
            None
        ),
        None
    );
}

#[test]
fn a_group_with_one_missing_type_shows_that_type_and_is_not_called_safe() {
    let mut group = entry(
        "better-files",
        "image-viewer",
        PlanAction::Restore {
            to: per_type(&[
                ("image/png", observed("org.gnome.eog.desktop")),
                ("image/jpeg", observed("org.gnome.gThumb.desktop")),
            ]),
        },
    );
    group
        .warnings
        .push(PlanWarning::PreviousApplicationMissing {
            key: Some("image/png".to_string()),
            desktop_id: "org.gnome.eog.desktop".to_string(),
            alternatives: alternatives(),
            chosen: None,
        });
    assert_eq!(
        RestoreClass::of(&group),
        RestoreClass::PreviousApplicationMissing
    );
    let review = review_of(PlanKind::Restore, vec![group.clone()]);
    let shown = &review.components()[0].entries[0];
    assert!(shown.changes_something, "the other type is still restored");
    assert_eq!(shown.missing.len(), 1);
    assert_eq!(shown.missing[0].key.as_deref(), Some("image/png"));
    assert!(shown.missing[0].headline.starts_with("image/png"));
    assert!(
        shown.warnings.is_empty(),
        "the missing type is shown once, as its own row"
    );

    // Once a choice is in the plan the type is safe to restore again.
    group.warnings = vec![PlanWarning::PreviousApplicationMissing {
        key: Some("image/png".to_string()),
        desktop_id: "org.gnome.eog.desktop".to_string(),
        alternatives: alternatives(),
        chosen: Some(RestoreChoice::ClearDefault),
    }];
    assert_eq!(RestoreClass::of(&group), RestoreClass::Safe);
}

#[test]
fn a_missing_application_result_says_so_with_its_name_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let run = |outcome: EntryOutcome| DefaultsOutcome {
            kind: PlanKind::Restore,
            results: vec![EntryResult {
                component: component("better-files"),
                integration: integration("image-viewer"),
                outcome,
            }],
            baseline_snapshot: None,
            recorded_snapshot: None,
        };
        let single = run(EntryOutcome::Skipped {
            reason: missing_eog(),
        });
        let row = &result_rows(locale, &single, &|component| component.to_string())[0];
        assert_eq!(row.label, c.previous_application_missing);
        assert_eq!(row.tone, ResultTone::Warning);
        assert_eq!(row.detail.as_deref(), Some("org.gnome.eog.desktop"));

        let group = run(EntryOutcome::PerKey {
            keys: vec![
                KeyOutcome {
                    key: "image/png".to_string(),
                    outcome: EntryOutcome::Skipped {
                        reason: missing_eog(),
                    },
                },
                KeyOutcome {
                    key: "image/jpeg".to_string(),
                    outcome: EntryOutcome::Restored {
                        value: observed("org.gnome.gThumb.desktop"),
                    },
                },
            ],
        });
        let row = &result_rows(locale, &group, &|component| component.to_string())[0];
        assert_eq!(row.label, c.result_partial);
        assert_eq!(row.tone, ResultTone::Warning);
        let detail = row.detail.clone().unwrap_or_default();
        assert!(
            detail.contains(&format!(
                "image/png: {} (org.gnome.eog.desktop)",
                c.previous_application_missing
            )),
            "{detail}"
        );
        assert!(
            detail.contains("image/jpeg: org.gnome.gThumb.desktop"),
            "{detail}"
        );
    }
}

#[test]
fn the_missing_application_words_exist_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for text in [
            c.previous_application_missing,
            c.restore_instead,
            c.clear_default,
            c.no_alternative_application,
        ] {
            assert!(!text.trim().is_empty(), "{locale:?} is missing copy");
        }
        assert!(
            !warning_label(
                locale,
                &PlanWarning::PreviousApplicationMissing {
                    key: None,
                    desktop_id: "a.desktop".to_string(),
                    alternatives: Vec::new(),
                    chosen: None,
                }
            )
            .trim()
            .is_empty()
        );
    }
    assert_ne!(
        copy(Locale::EnUs).clear_default,
        copy(Locale::ZhTw).clear_default
    );
}

#[test]
fn a_chosen_application_that_went_missing_is_not_called_the_previous_one() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let reason = SkipReason::ChosenApplicationMissing {
            desktop_id: "org.gnome.Loupe.desktop".to_string(),
            alternatives: Vec::new(),
        };
        assert_eq!(
            skip_reason_label(locale, &reason),
            c.chosen_application_missing
        );
        assert_ne!(c.chosen_application_missing, c.previous_application_missing);
        let outcome = DefaultsOutcome {
            kind: PlanKind::Restore,
            results: vec![EntryResult {
                component: component("better-files"),
                integration: integration("image-viewer"),
                outcome: EntryOutcome::Skipped { reason },
            }],
            baseline_snapshot: None,
            recorded_snapshot: None,
        };
        let row = &result_rows(locale, &outcome, &|component| component.to_string())[0];
        assert_eq!(row.label, c.chosen_application_missing);
        assert_eq!(row.tone, ResultTone::Warning);
        assert_eq!(row.detail.as_deref(), Some("org.gnome.Loupe.desktop"));
    }
}

/// What the control center's page adds on top of today's Defaults screens
/// (ticket 89): the summary line, the overflow menu, the filter a details
/// page opens, the integration rows, the Home card, and the search.
mod control_center {
    use super::*;
    use crate::attention::{AttentionAction, Severity, Snapshot};
    use crate::model::Page;
    use crate::navigation::Route;
    use crate::pages::default_apps::model::{
        ComponentMenu, changed_elsewhere, changes_label, holds_restart, integration_line,
        outcome_tone, summary_line, visible_rows,
    };

    fn mixed_report() -> DefaultsReport {
        report_of(vec![
            defaults_of(
                "better-files",
                vec![status(
                    "file-manager",
                    IntegrationState::Default,
                    observed("io.betteros.Files.desktop"),
                )],
            ),
            defaults_of(
                "better-git",
                vec![
                    status(
                        "git-gui",
                        IntegrationState::ChangedExternally { last_known: None },
                        observed("org.gnome.gitg.desktop"),
                    ),
                    status(
                        "git-links",
                        IntegrationState::ChangedExternally { last_known: None },
                        observed("org.gnome.gitg.desktop"),
                    ),
                ],
            ),
            defaults_of(
                "better-monitor",
                vec![status(
                    "monitor",
                    IntegrationState::NotDefault,
                    observed("org.gnome.SystemMonitor.desktop"),
                )],
            ),
        ])
    }

    #[test]
    fn the_summary_line_reads_the_spec_s_words_in_both_languages() {
        let summary = DefaultsSummary::of(&mixed_report());
        assert_eq!(
            summary_line(Locale::EnUs, &summary),
            "1 are defaults · 2 can change · 1 changed elsewhere"
        );
        assert_eq!(
            summary_line(Locale::ZhTw, &summary),
            "1 個已是預設 · 2 個可以變更 · 1 個被其他程式變更"
        );
    }

    /// Home counts settings, not components: one component with two changed
    /// settings is two.
    #[test]
    fn home_counts_the_settings_changed_elsewhere() {
        assert_eq!(changed_elsewhere(&mixed_report()), 2);
        assert_eq!(changed_elsewhere(&report_of(Vec::new())), 0);
    }

    #[test]
    fn home_says_a_default_was_changed_elsewhere_and_opens_the_page() {
        let module = crate::pages::module(Route::DefaultApps);
        let quiet = Snapshot::empty(Locale::EnUs);
        assert!((module.attention)(&quiet).is_empty());

        let one = Snapshot {
            default_apps_changed: 1,
            ..Snapshot::empty(Locale::EnUs)
        };
        let items = (module.attention)(&one);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].header, "A default was changed by another app");
        assert_eq!(
            items[0].description,
            "1 setting no longer matches what Better OS set."
        );
        assert_eq!(items[0].action_label, "View");
        assert_eq!(items[0].action, AttentionAction::Open(Page::Defaults));
        assert_eq!(items[0].severity, Severity::Warning);
        assert_eq!(items[0].source, Route::DefaultApps);

        let three = Snapshot {
            default_apps_changed: 3,
            ..Snapshot::empty(Locale::ZhTw)
        };
        let items = (module.attention)(&three);
        assert_eq!(items[0].header, "有預設值被其他程式變更");
        assert_eq!(items[0].description, "3 項設定和 Better OS 套用的不同。");
        assert_eq!(items[0].action_label, "查看");
        // It takes "Everything's fine" away, as a warning does.
        assert!(!crate::attention::everything_fine(&items));
    }

    #[test]
    fn the_overflow_menu_offers_only_what_can_happen() {
        let rows = rows_of(&mixed_report());
        let menu = |id: &str| {
            ComponentMenu::of(
                rows.iter()
                    .find(|row| row.component.as_str() == id)
                    .expect("the component is listed"),
            )
        };
        // Already the default, nothing saved: Check again only.
        assert_eq!(
            menu("better-files"),
            ComponentMenu {
                make_default: false,
                restore: false
            }
        );
        assert!(menu("better-git").make_default);
        assert!(menu("better-monitor").make_default);

        let mut saved = status(
            "file-manager",
            IntegrationState::Default,
            observed("io.betteros.Files.desktop"),
        );
        saved.restore_available = true;
        let rows = rows_of(&report_of(vec![defaults_of("better-files", vec![saved])]));
        assert_eq!(
            ComponentMenu::of(&rows[0]),
            ComponentMenu {
                make_default: false,
                restore: true
            }
        );
    }

    #[test]
    fn a_details_page_link_shows_only_its_component() {
        let rows = rows_of(&mixed_report());
        assert_eq!(visible_rows(rows.clone(), None).len(), 3);
        let only = visible_rows(rows.clone(), Some(&component("better-git")));
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].component.as_str(), "better-git");
        assert!(visible_rows(rows, Some(&component("better-awake"))).is_empty());
    }

    #[test]
    fn an_integration_row_names_owner_target_check_saved_value_and_effect() {
        let rows = rows_of(&mixed_report());
        let monitor = rows
            .iter()
            .find(|row| row.component.as_str() == "better-monitor")
            .unwrap();
        let line = integration_line(Locale::EnUs, &monitor.integrations[0], 1_000);
        assert_eq!(
            line,
            "Opens with: org.gnome.SystemMonitor.desktop → Better OS would use: \
             io.betteros.Files.desktop · Last checked: Not checked yet · \
             No previous value saved · Takes effect immediately"
        );
        let line = integration_line(Locale::ZhTw, &monitor.integrations[0], 1_000);
        let c = copy(Locale::ZhTw);
        for part in [
            c.current_owner,
            c.better_target,
            c.last_verified,
            c.never_verified,
            c.no_saved_previous_value,
        ] {
            assert!(line.contains(part), "{line} lacks {part}");
        }
    }

    #[test]
    fn a_review_row_counts_its_settings_in_both_languages() {
        assert_eq!(changes_label(Locale::EnUs, 1), "1 setting affected");
        assert_eq!(changes_label(Locale::EnUs, 3), "3 settings affected");
        assert_eq!(changes_label(Locale::ZhTw, 2), "影響 2 項設定");
    }

    /// An update waits while a plan is written and while a review holds
    /// choices a restart would lose.
    #[test]
    fn writing_or_reviewing_holds_an_update_restart() {
        assert!(!holds_restart(false, false));
        assert!(holds_restart(true, false));
        assert!(holds_restart(false, true));
    }

    #[test]
    fn the_results_headline_is_drawn_as_honestly_as_it_reads() {
        let run = |outcomes: Vec<EntryOutcome>| DefaultsOutcome {
            kind: PlanKind::Apply,
            results: outcomes
                .into_iter()
                .enumerate()
                .map(|(index, outcome)| EntryResult {
                    component: component("better-files"),
                    integration: integration(&format!("i-{index}")),
                    outcome,
                })
                .collect(),
            baseline_snapshot: None,
            recorded_snapshot: None,
        };
        let applied = || EntryOutcome::Applied {
            value: desktop("io.betteros.Files.desktop"),
        };
        let not_verified = || EntryOutcome::NotVerified {
            observed: observed("org.gnome.Nautilus.desktop"),
        };
        assert_eq!(outcome_tone(&run(vec![applied()])), ResultTone::Success);
        assert_eq!(
            outcome_tone(&run(vec![applied(), not_verified()])),
            ResultTone::Warning
        );
        assert_eq!(
            outcome_tone(&run(vec![not_verified()])),
            ResultTone::Failure
        );
        // The words and the drawing agree.
        let partial = run(vec![applied(), not_verified()]);
        assert_eq!(
            outcome_headline(Locale::EnUs, &partial),
            copy(Locale::EnUs).result_partial
        );
    }

    #[test]
    fn the_page_is_built_and_findable_in_both_languages() {
        let module = crate::pages::module(Route::DefaultApps);
        assert_eq!(module.status, crate::pages::PageStatus::Built);
        assert_eq!(module.name, "default-apps");
        for (locale, query, card) in [
            (
                Locale::EnUs,
                "use better",
                crate::pages::default_apps::card::COMMANDS,
            ),
            (
                Locale::EnUs,
                "changed else",
                crate::pages::default_apps::card::SUMMARY,
            ),
            (
                Locale::EnUs,
                "default apps",
                crate::pages::default_apps::card::SUMMARY,
            ),
            (
                Locale::ZhTw,
                "預設應用程式",
                crate::pages::default_apps::card::SUMMARY,
            ),
            (
                Locale::ZhTw,
                "恢復先前",
                crate::pages::default_apps::card::COMMANDS,
            ),
            (
                Locale::ZhTw,
                "要求的設定",
                crate::pages::default_apps::card::COMPONENTS,
            ),
        ] {
            let index = crate::pages::search_index(locale);
            let found = crate::search::find(&index, query);
            assert!(
                found
                    .iter()
                    .any(|entry| entry.route == Route::DefaultApps && entry.card == card),
                "{locale:?} {query:?} found {found:?}"
            );
        }
    }

    /// Every card id the search scrolls to is one the page draws.
    #[test]
    fn every_search_card_is_drawn_by_the_page() {
        let source = include_str!("render.rs");
        for card in ["card::SUMMARY", "card::COMMANDS", "card::COMPONENTS"] {
            assert!(source.contains(card), "the page never draws {card}");
        }
    }

    #[test]
    fn the_page_s_own_words_exist_in_both_languages() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let w = crate::pages::default_apps::copy::page_copy(locale);
            for text in [
                w.description,
                w.summary,
                w.restore,
                w.more_actions,
                w.review_summary,
                w.review_changes_one,
                w.review_changes,
                w.home_changed_header,
                w.home_changed_description_one,
                w.home_changed_description,
                w.view,
            ] {
                assert!(!text.trim().is_empty(), "{locale:?}");
            }
        }
        assert_eq!(
            crate::pages::default_apps::copy::page_copy(Locale::ZhTw).description,
            "讓 Better OS 元件成為開啟檔案、連結與功能的預設程式，並可隨時恢復。"
        );
    }

    /// The first time Better OS changes a setting it captures what was there
    /// before it writes, so the review must not say that value is lost; only
    /// a value that cannot be read clearly is.
    #[test]
    fn a_first_change_says_the_value_it_replaces_can_be_put_back() {
        let mut first = applying("better-files", "file-manager");
        first.captured_previous = None;
        let review = review_of(PlanKind::Apply, vec![first]);
        assert!(review.components()[0].entries[0].restorable);

        let mut unreadable = applying("better-files", "file-manager");
        unreadable.captured_previous = None;
        unreadable.current = ObservedValue::Unknown {
            reason: "xdg.unreadable".to_string(),
        };
        let review = review_of(PlanKind::Apply, vec![unreadable]);
        assert!(!review.components()[0].entries[0].restorable);

        // A setting this plan leaves alone puts nothing back.
        let skipped = skipping("better-files", "file-manager", SkipReason::AlreadyDefault);
        let mut skipped = skipped;
        skipped.captured_previous = None;
        let review = review_of(PlanKind::Apply, vec![skipped]);
        assert!(!review.components()[0].entries[0].restorable);
    }

    /// "1 minutes ago" read wrong on the page; one of a unit is singular.
    #[test]
    fn one_minute_hour_or_day_is_said_in_the_singular() {
        let now = 1_000_000;
        assert_eq!(
            relative_time(Locale::EnUs, Some(now - 60), now),
            "1 minute ago"
        );
        assert_eq!(
            relative_time(Locale::EnUs, Some(now - 120), now),
            "2 minutes ago"
        );
        assert_eq!(
            relative_time(Locale::EnUs, Some(now - 3600), now),
            "1 hour ago"
        );
        assert_eq!(
            relative_time(Locale::EnUs, Some(now - 86_400), now),
            "1 day ago"
        );
        assert_eq!(
            relative_time(Locale::EnUs, Some(now - 3 * 86_400), now),
            "3 days ago"
        );
        assert_eq!(relative_time(Locale::ZhTw, Some(now - 60), now), "1 分鐘前");
    }

    /// A setting this desktop does not have says exactly that, in both
    /// languages, instead of the generic "not available".
    #[test]
    fn a_setting_this_desktop_lacks_says_so() {
        let unsupported = ObservedValue::Unsupported {
            reason: defaults_core::NO_SUCH_SETTING.to_string(),
        };
        let state = IntegrationState::Unavailable {
            reason: defaults_core::NO_SUCH_SETTING.to_string(),
        };
        assert_eq!(
            observed_label(Locale::EnUs, &unsupported),
            "This desktop has no such setting, so nothing reads it"
        );
        assert_eq!(
            integration_state_label(Locale::EnUs, &state),
            "Not used on this desktop"
        );
        assert_eq!(
            integration_state_label(Locale::ZhTw, &state),
            "這個桌面不使用"
        );
        assert_eq!(
            observed_label(Locale::ZhTw, &unsupported),
            "這個桌面沒有這項設定，所以不會有程式讀取它"
        );
        // Any other unsupported reading keeps the general words.
        assert_eq!(
            observed_label(
                Locale::EnUs,
                &ObservedValue::Unsupported {
                    reason: "other".to_string()
                }
            ),
            copy(Locale::EnUs).value_unsupported
        );
    }

    /// A dock entry Better OS added stays when defaults are put back, and the
    /// review says so rather than "not used on this system".
    #[test]
    fn a_launcher_entry_left_in_place_reads_as_staying() {
        let entry = skipping("better-files", "overview-entry", SkipReason::LeftInPlace);
        assert_eq!(RestoreClass::of(&entry), RestoreClass::LeftInPlace);
        assert_eq!(
            skip_reason_label(Locale::EnUs, &SkipReason::LeftInPlace),
            "Stays: putting things back leaves this in place"
        );
        assert_eq!(
            RestoreClass::LeftInPlace.label(Locale::ZhTw),
            "保留：恢復時不會移除這一項"
        );
    }
}
