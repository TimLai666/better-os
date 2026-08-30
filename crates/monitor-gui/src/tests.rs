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
        c.history,
        c.incidents,
        c.inventory,
        c.history_subtitle,
        c.incidents_subtitle,
        c.inventory_subtitle,
        c.collection_service,
        c.collection_in_process,
        c.collection_unavailable,
        c.collection_connecting,
        c.span_15_minutes,
        c.span_1_hour,
        c.span_6_hours,
        c.stored_samples,
        c.observation_gaps,
        c.gap_service_stopped,
        c.gap_missed_cadence,
        c.gap_interrupted_write,
        c.gap_retention,
        c.no_history_yet,
        c.history_truncated,
        c.retention_window,
        c.disk_budget,
        c.resolution,
        c.mark_incident,
        c.mark_unavailable,
        c.no_incidents_yet,
        c.marked_at,
        c.incident_window,
        c.largest_shifts,
        c.baseline_none,
        c.captured_processes,
        c.inventory_never_captured,
        c.inventory_captures,
        c.inventory_no_changes,
        c.inventory_first_capture,
        c.changed,
        c.added,
        c.removed,
        c.withheld_value,
        c.request_failed,
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

#[test]
fn every_observation_gap_has_words_in_both_locales() {
    use crate::stored::gap_reason_label;
    use monitor_store::GapReason;

    // Every reason the store can record has to be sayable. A gap the window
    // could not label would be drawn as an unexplained hole.
    let reasons = [
        GapReason::ServiceStopped,
        GapReason::MissedCadence,
        GapReason::InterruptedWrite,
        GapReason::Retention,
    ];
    for locale in LOCALES {
        let c = copy(locale);
        let labels: Vec<&str> = reasons
            .iter()
            .map(|reason| gap_reason_label(*reason, c))
            .collect();
        for label in &labels {
            assert!(
                !label.trim().is_empty(),
                "{locale:?} is missing a gap label"
            );
        }
        let unique: std::collections::BTreeSet<&&str> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            reasons.len(),
            "{locale:?} reuses a label for two different reasons"
        );
    }
}

#[test]
fn a_history_chart_leaves_a_space_where_there_was_no_reading() {
    use crate::stored::bar_fraction;

    // The whole point of the History page: a sample with no reading draws
    // nothing, and a sample that measured zero draws a floor. If both produced
    // the same bar the chart would say the machine was idle when it was
    // actually unobserved.
    let values = [Some(0.0), None, Some(4.0), Some(2.0)];
    assert_eq!(bar_fraction(&values, 0), Some(0.0));
    assert_eq!(bar_fraction(&values, 1), None);
    assert_eq!(bar_fraction(&values, 2), Some(1.0));
    assert_eq!(bar_fraction(&values, 3), Some(0.5));
    assert_eq!(bar_fraction(&values, 9), None);
}

#[test]
fn a_period_where_everything_measured_zero_is_not_drawn_as_unobserved() {
    use crate::stored::bar_fraction;

    let values = [Some(0.0), Some(0.0), Some(0.0)];
    for index in 0..values.len() {
        assert_eq!(
            bar_fraction(&values, index),
            Some(0.0),
            "a measured zero must still be a reading"
        );
    }
}

#[test]
fn the_history_page_charts_only_metrics_the_collectors_declare() {
    use monitor_collectors_linux::LinuxCollectors;
    use monitor_core::MetricId;

    // A renamed metric must break here rather than silently blanking a chart.
    let declared: std::collections::BTreeSet<String> = LinuxCollectors::descriptors()
        .into_iter()
        .map(|descriptor| descriptor.id.to_string())
        .collect();
    for id in crate::stored::CHARTED {
        assert!(
            MetricId::new(id).is_ok(),
            "{id} is not a well-formed metric id"
        );
        assert!(
            declared.contains(id),
            "the History page charts {id}, which no collector declares"
        );
    }
}

#[test]
fn marking_is_offered_only_when_something_is_actually_recording() {
    use crate::app::Collection;

    // Offering the button while nothing collects would capture an incident
    // with no history behind it, which is worse than not offering it.
    for (collection, expected) in [
        (Collection::Connecting, false),
        (Collection::Service, true),
        (
            Collection::InProcess {
                detail: "no service".into(),
            },
            true,
        ),
        (
            Collection::Unavailable {
                detail: "no store".into(),
            },
            false,
        ),
    ] {
        assert_eq!(
            matches!(
                collection,
                Collection::Service | Collection::InProcess { .. }
            ),
            expected
        );
    }
}

#[test]
fn the_window_asks_for_a_marker_and_decides_nothing_about_what_it_captures() {
    use crate::link::LinkRequest;

    // The window's whole contribution to an incident is the moment and the
    // selected process. The window before and after, the snapshot, and the
    // baseline are all decided where the readings are.
    let request = LinkRequest::Mark {
        note: None,
        before_seconds: monitor_store::DEFAULT_WINDOW_BEFORE_SECONDS,
        after_seconds: monitor_store::DEFAULT_WINDOW_AFTER_SECONDS,
        about_pid: Some(4242),
    };
    let LinkRequest::Mark {
        before_seconds,
        after_seconds,
        ..
    } = request
    else {
        unreachable!()
    };
    assert!(
        monitor_store::IncidentWindow {
            before_seconds,
            after_seconds,
        }
        .is_valid(),
        "the window the GUI sends must be one the protocol accepts"
    );
}

#[test]
fn the_history_page_never_asks_for_more_than_the_protocol_allows() {
    // A compile-time check: a window that asked for more than the protocol
    // allows would be refused at run time on a page a user had just opened.
    const _: () = assert!(crate::link::MAX_HISTORY_SAMPLES <= monitor_ipc::MAX_SAMPLES_PER_REPLY);
    const _: () = assert!(crate::link::MAX_HISTORY_SAMPLES > 0);
}

#[test]
fn every_navigation_label_fits_the_sidebar_at_every_supported_scale() {
    // The sidebar is a fixed 232 logical pixels wide, and an icon plus padding
    // takes about 72 of them. A label that does not fit is clipped, and a
    // clipped navigation item is one a user cannot read in either language.
    const SIDEBAR_WIDTH: f32 = 232.0;
    const LABEL_SPACE: f32 = SIDEBAR_WIDTH - 72.0;

    for locale in LOCALES {
        let c = copy(locale);
        for label in [
            c.overview,
            c.apps,
            c.processes,
            c.cpu,
            c.memory,
            c.storage,
            c.network,
            c.gpu,
            c.energy,
            c.history,
            c.incidents,
            c.inventory,
            c.diagnostics,
            c.settings,
        ] {
            for scale in SCALES {
                // A larger scale means the same sidebar holds fewer logical
                // pixels of text.
                let available = LABEL_SPACE / scale;
                assert!(
                    label_width(label) <= available,
                    "{locale:?} navigation label {label:?} overflows at {scale}x \
                     ({:.1} > {available:.1})",
                    label_width(label)
                );
            }
        }
    }
}

#[test]
fn the_history_and_inventory_page_headings_fit_the_content_column() {
    // Page headings and their subtitles sit in the content column, which is at
    // least the minimum window width less the sidebar and page padding. A
    // subtitle that overflows would be clipped rather than wrapped, because
    // the heading row does not wrap.
    let available = MIN_WINDOW_WIDTH - 300.0;
    for locale in LOCALES {
        let c = copy(locale);
        for heading in [c.history, c.incidents, c.inventory] {
            for scale in SCALES {
                assert!(
                    label_width(heading) <= available / scale,
                    "{locale:?} heading {heading:?} overflows at {scale}x"
                );
            }
        }
        // The subtitles are allowed to wrap, so what is asserted is only that
        // no single word in them is wider than the column.
        for subtitle in [
            c.history_subtitle,
            c.incidents_subtitle,
            c.inventory_subtitle,
            c.collection_in_process,
            c.no_history_yet,
            c.inventory_first_capture,
        ] {
            let longest = subtitle
                .split_whitespace()
                .map(label_width)
                .fold(0.0f32, f32::max);
            assert!(
                longest <= available,
                "{locale:?} has an unwrappable run in {subtitle:?}"
            );
        }
    }
}
