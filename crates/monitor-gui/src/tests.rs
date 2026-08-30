//! What can be asserted about the window without opening one.
//!
//! Three things are proved here: that both languages have every string, that
//! neither language's labels overflow the layout at any supported scale, and
//! that the presentation rules the specification is strict about — a missing
//! reading never drawn as a zero, a destructive action never run without a
//! confirmation, an action on another user's process never offered — hold in
//! the code the window actually calls.

use crate::i18n::{Copy, EN, Locale, ZH, copy};
use crate::layout::{
    ActionLayout, MIN_WINDOW_WIDTH, TableLayout, action_layout, header_fits, label_width,
    process_table_width, table_layout,
};
use crate::tables::{ProcessColumnLayout, evidence_label, non_value_label};
use monitor_core::action::testing::RecordingController;
use monitor_core::{
    ActionRefusal, ProcessAction, ProcessController, ProcessTarget, UnknownReason,
    UnsupportedReason,
};
use monitor_views::format::{Cell, NonValue, cell};
use monitor_views::grouping::{AppKind, GroupingEvidence, GroupingPrecedence, group_processes};
use monitor_views::{Field, ProcessColumn, ProcessFacts, ProcessTableModel};

const LOCALES: [Locale; 2] = [Locale::EnUs, Locale::ZhTw];
const SCALES: [f32; 3] = [1.0, 1.25, 1.5];

fn every_visible_string(c: &'static Copy) -> Vec<&'static str> {
    vec![
        c.brand_name,
        c.monitor,
        c.navigation,
        c.overview,
        c.apps,
        c.processes,
        c.cpu,
        c.memory,
        c.storage,
        c.network,
        c.gpu,
        c.energy,
        c.diagnostics,
        c.settings,
        c.search_placeholder,
        c.pause_updates,
        c.resume_updates,
        c.paused_banner,
        c.overview_subtitle,
        c.utilization,
        c.pressure_some,
        c.pressure_full,
        c.load_average,
        c.logical_cpus,
        c.running_processes,
        c.throttling,
        c.top_applications,
        c.observation_health,
        c.read_throughput,
        c.write_throughput,
        c.received,
        c.sent,
        c.busiest_device,
        c.available_memory,
        c.free_is_not_available,
        c.swap_used,
        c.cached,
        c.major_faults,
        c.verdict_nominal,
        c.verdict_busy,
        c.verdict_pressure,
        c.verdict_saturated,
        c.verdict_collector_failed,
        c.verdict_unsupported,
        c.verdict_unobserved,
        c.throttling_none,
        c.throttling_clock,
        c.throttling_unobservable,
        c.not_yet_sampled,
        c.interval_too_short,
        c.read_failed,
        c.malformed,
        c.entity_disappeared,
        c.interface_missing,
        c.not_reported,
        c.policy_withheld,
        c.permission_denied,
        c.not_collected,
        c.stale_suffix,
        c.processes_subtitle,
        c.column_name,
        c.column_pid,
        c.column_parent_pid,
        c.column_user,
        c.column_state,
        c.column_cpu,
        c.column_cpu_time,
        c.column_memory,
        c.column_swap,
        c.column_read,
        c.column_write,
        c.column_threads,
        c.column_descriptors,
        c.column_start_time,
        c.column_nice,
        c.column_cgroup,
        c.column_command_line,
        c.tree_view,
        c.show_command_lines,
        c.select_a_process,
        c.apps_subtitle,
        c.applications,
        c.background_services,
        c.grouped_because,
        c.evidence_systemd_unit,
        c.evidence_flatpak,
        c.evidence_snap,
        c.evidence_desktop,
        c.evidence_ancestry,
        c.evidence_executable,
        c.evidence_unattributed,
        c.confidence_high,
        c.confidence_medium,
        c.confidence_low,
        c.partial_total,
        c.nothing_measured,
        c.process_count,
        c.actions,
        c.terminate,
        c.force_stop,
        c.pause_process,
        c.resume_process,
        c.lower_priority,
        c.confirm_terminate,
        c.confirm_force_stop,
        c.confirm,
        c.cancel,
        c.refusal_other_user,
        c.refusal_ownership_unknown,
        c.refusal_protected,
        c.refusal_raise_priority,
        c.refusal_nice_range,
        c.refusal_priority_unknown,
        c.outcome_signal_sent,
        c.outcome_priority_changed,
        c.failure_disappeared,
        c.failure_denied,
        c.failure_invalid,
        c.failure_unsupported,
        c.failure_other,
        c.unsupported_page_title,
        c.gpu_unsupported,
        c.energy_unsupported,
        c.collector,
        c.health_healthy,
        c.health_degraded,
        c.health_failed,
        c.health_unsupported,
        c.coverage_title,
        c.coverage_value,
        c.coverage_stale,
        c.coverage_unknown,
        c.coverage_unsupported,
        c.coverage_denied,
        c.no_devices,
        c.appearance,
        c.dark_theme,
        c.light_theme,
        c.system_default,
        c.language,
        c.english,
        c.traditional_chinese,
        c.privacy,
        c.privacy_description,
        c.sampling,
        c.sampling_description,
        c.grouping_precedence,
        c.grouping_precedence_description,
    ]
}

#[test]
fn every_visible_string_exists_in_both_locales() {
    for locale in LOCALES {
        for value in every_visible_string(copy(locale)) {
            assert!(!value.trim().is_empty(), "{locale:?} is missing a string");
        }
    }
    assert_eq!(
        every_visible_string(&EN).len(),
        every_visible_string(&ZH).len()
    );
}

#[test]
fn the_two_locales_are_actually_different_translations() {
    // A copied-through English string in the Chinese table is the failure this
    // catches: it compiles, it is non-empty, and it is wrong.
    let shared = ["Better OS", "PID", "English"];
    let identical = every_visible_string(&EN)
        .into_iter()
        .zip(every_visible_string(&ZH))
        .filter(|(english, chinese)| english == chinese && !shared.contains(english))
        .count();
    assert_eq!(identical, 0, "{identical} strings were not translated");
}

#[test]
fn every_column_header_fits_its_column_in_both_locales() {
    for locale in LOCALES {
        let c = copy(locale);
        for column in ProcessColumn::ALL {
            let label = ProcessColumnLayout::header_of(column, c);
            let width = ProcessColumnLayout::width_of(column);
            assert!(
                header_fits(label, width),
                "{locale:?} header {label:?} needs {:.0}px but has {width}px",
                label_width(label)
            );
        }
    }
}

#[test]
fn the_process_table_scrolls_sideways_rather_than_clipping_at_every_scale() {
    let all = ProcessColumn::ALL.to_vec();
    let total = process_table_width(&all);
    for scale in SCALES {
        assert_eq!(
            table_layout(MIN_WINDOW_WIDTH, scale, total),
            TableLayout::HorizontalScroll,
            "the full column set cannot fit the minimum window; it must scroll"
        );
        // A wide window fits the columns that matter without scrolling.
        let essential = vec![
            ProcessColumn::Name,
            ProcessColumn::Pid,
            ProcessColumn::User,
            ProcessColumn::CpuUtilization,
            ProcessColumn::Memory,
        ];
        assert_eq!(
            table_layout(2560.0, scale, process_table_width(&essential)),
            TableLayout::Fits
        );
    }
}

#[test]
fn the_action_row_reflows_at_the_minimum_window_in_both_locales() {
    // At the minimum supported window both locales' action rows fit on one
    // line at 100%. At 125% and 150% the same window holds 688 and 573 logical
    // pixels, and the row has to wrap in both languages — which is the case
    // this test exists to hold, because it is the one a scaled desktop hits.
    for locale in LOCALES {
        let c = copy(locale);
        let labels = [
            c.terminate,
            c.force_stop,
            c.pause_process,
            c.resume_process,
            c.lower_priority,
        ];
        assert_eq!(
            action_layout(MIN_WINDOW_WIDTH, 1.0, &labels),
            ActionLayout::Inline,
            "{locale:?} fits at 100%"
        );
        for scale in [1.25, 1.5] {
            assert_eq!(
                action_layout(MIN_WINDOW_WIDTH, scale, &labels),
                ActionLayout::Wrapped,
                "{locale:?} at {scale}x must wrap at the minimum window width"
            );
        }
        // A full-size window keeps the row inline at every supported scale.
        for scale in SCALES {
            assert_eq!(
                action_layout(1920.0, scale, &labels),
                ActionLayout::Inline,
                "{locale:?} at {scale}x on a 1920px window"
            );
        }
    }
}

#[test]
fn a_synthetic_over_long_translation_still_wraps_rather_than_overflowing() {
    let synthetic = "Terminate the selected process and every helper it started";
    for scale in SCALES {
        assert_eq!(
            action_layout(MIN_WINDOW_WIDTH, scale, &[synthetic, synthetic]),
            ActionLayout::Wrapped
        );
    }
}

#[test]
fn wide_script_labels_are_measured_as_wide() {
    // Four ideographs must not be measured as four Latin letters, or the
    // column-fit check would pass on a label that clips.
    assert!(label_width("背景服務") > label_width("abcd") * 1.9);
}

#[test]
fn every_non_value_reason_has_words_in_both_locales() {
    let reasons = [
        NonValue::NotYetSampled,
        NonValue::IntervalTooShort,
        NonValue::ReadFailed {
            detail: String::new(),
        },
        NonValue::Malformed {
            detail: String::new(),
        },
        NonValue::EntityDisappeared,
        NonValue::InterfaceMissing {
            path: String::new(),
        },
        NonValue::NotReported {
            detail: String::new(),
        },
        NonValue::PolicyWithheld {
            policy: String::new(),
        },
        NonValue::PermissionDenied {
            path: String::new(),
        },
        NonValue::NotCollected,
    ];
    for locale in LOCALES {
        let c = copy(locale);
        let labels: std::collections::BTreeSet<&str> = reasons
            .iter()
            .map(|reason| non_value_label(reason, c))
            .collect();
        assert_eq!(
            labels.len(),
            reasons.len(),
            "{locale:?} reuses a label for two different reasons"
        );
    }
}

#[test]
fn every_grouping_evidence_has_words_in_both_locales() {
    let evidence = [
        GroupingEvidence::SystemdUnit {
            unit: String::new(),
            path: String::new(),
        },
        GroupingEvidence::Flatpak {
            app_id: String::new(),
            unit: String::new(),
        },
        GroupingEvidence::Snap {
            snap: String::new(),
            app: None,
            unit: String::new(),
        },
        GroupingEvidence::DesktopApplication {
            app_id: String::new(),
            unit: String::new(),
        },
        GroupingEvidence::Ancestry {
            parent_pid: 1,
            root_pid: 1,
        },
        GroupingEvidence::ExecutableIdentity {
            executable: String::new(),
        },
        GroupingEvidence::Unattributed {
            detail: String::new(),
        },
    ];
    for locale in LOCALES {
        let c = copy(locale);
        let labels: std::collections::BTreeSet<&str> = evidence
            .iter()
            .map(|item| evidence_label(item, c))
            .collect();
        assert_eq!(labels.len(), evidence.len(), "{locale:?}");
    }
}

#[test]
fn no_column_renders_a_missing_reading_as_a_number() {
    // The process the collector could barely read: every column that can be
    // missing, is.
    let mut process = ProcessFacts::synthetic(4242, "unreadable");
    process.cpu_utilization = Field::Unknown(UnknownReason::NotYetSampled);
    process.memory_resident = Field::Unsupported(UnsupportedReason::NotReported {
        detail: "kernel thread".into(),
    });
    process.file_descriptors = Field::PermissionDenied {
        path: "/proc/4242/fd".into(),
    };
    process.threads = Field::NotCollected;

    for rendered in [
        cell(&process.cpu_utilization, |v| format!("{v}")),
        cell(&process.memory_resident, |v| format!("{v}")),
        cell(&process.file_descriptors, |v| format!("{v}")),
        cell(&process.threads, |v| format!("{v}")),
    ] {
        assert!(rendered.is_missing(), "{rendered:?}");
        assert_eq!(rendered.text(), None);
    }
    // And a real zero is still a number.
    assert_eq!(
        cell(&process.memory_swap, |v| format!("{v}")),
        Cell::Value("0".into())
    );
}

#[test]
fn the_destructive_actions_are_exactly_the_ones_that_ask_first() {
    let confirmed: Vec<&str> = [
        ProcessAction::Terminate,
        ProcessAction::ForceStop,
        ProcessAction::Pause,
        ProcessAction::Resume,
        ProcessAction::SetNice(10),
    ]
    .into_iter()
    .filter(|action| action.requires_confirmation())
    .map(|action| action.key())
    .collect();
    assert_eq!(confirmed, vec!["terminate", "force-stop"]);
}

#[test]
fn the_window_never_offers_an_action_on_another_users_process() {
    // The window asks the controller, and this is the answer it gets.
    let controller = RecordingController::new(1000, 55);
    let theirs = ProcessTarget::new(900, "systemd-resolved")
        .owned_by(101)
        .with_nice(0);
    for action in [
        ProcessAction::Terminate,
        ProcessAction::ForceStop,
        ProcessAction::Pause,
        ProcessAction::SetNice(10),
    ] {
        assert_eq!(
            controller.availability(&theirs, action).refusal(),
            Some(&ActionRefusal::NotOwnedByCurrentUser {
                owner_uid: Some(101)
            }),
            "{action:?}"
        );
    }
}

#[test]
fn the_processes_page_hides_command_lines_until_they_are_turned_on() {
    let mut model = ProcessTableModel::new(vec![ProcessFacts::synthetic(1, "systemd")]);
    assert!(!model.columns().contains(&ProcessColumn::CommandLine));
    model.set_show_command_line(true);
    assert!(model.columns().contains(&ProcessColumn::CommandLine));
}

#[test]
fn the_apps_page_separates_what_a_person_launched_from_what_the_system_runs() {
    let mut launched = ProcessFacts::synthetic(100, "gedit");
    launched.cgroup = Field::Value(
        "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.gedit-1.scope"
            .into(),
    );
    let mut service = ProcessFacts::synthetic(200, "NetworkManager");
    service.cgroup = Field::Value("/system.slice/NetworkManager.service".into());

    let grouping = group_processes(&[launched, service], &GroupingPrecedence::default());
    assert_eq!(grouping.applications.len(), 1);
    assert_eq!(grouping.services.len(), 1);
    assert_eq!(grouping.applications[0].kind, AppKind::UserApplication);
    assert_eq!(grouping.services[0].kind, AppKind::BackgroundService);
}
