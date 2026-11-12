//! What can be asserted about the window without opening one.
//!
//! The window's decisions live in modules that know nothing of GPUI, and
//! these tests call them directly: that both languages have every word and
//! show no internal name, that nothing a table or a page title says is cut
//! off at 960 pixels, what the live charts keep and how they leave gaps, how
//! the Overview's sentence is chosen, how startup apps and services are read
//! and switched, and the safety rules the specification is strict about — a
//! missing reading never drawn as a number, a destructive action never run
//! without a confirmation, an action on another user's process never
//! offered.

use std::path::PathBuf;

use crate::charts::{AdapterKind, CAPACITY, LiveHistory, Readings, SeriesKey};
use crate::i18n::{self, Copy, EN, Locale, ZH, copy};
use crate::layout::{
    card_columns, commands_that_fit, content_width, flexible_width, heading_fits, label_width,
    navigation_compact,
};
use crate::startup::{
    self, Activity, AutostartDirs, DesktopEntry, Scope, ServiceError, ServiceLock, StartupState,
    Systemctl,
};
use crate::tables;
use better_ui::fluent::list::CellTone;
use monitor_core::action::testing::RecordingController;
use monitor_core::{
    ActionRefusal, CollectorId, CollectorReport, Entity, EntityId, EntityKind, MetricId, MetricSet,
    Observation, ProcessAction, ProcessController, ProcessTarget, Timestamp, UnknownReason,
    UnsupportedReason,
};
use monitor_views::format::{Cell, NonValue, cell};
use monitor_views::overview::{OverviewModel, ResourceVerdict};
use monitor_views::{Field, ProcessColumn, ProcessFacts, ProcessTableModel};

const LOCALES: [Locale; 2] = [Locale::EnUs, Locale::ZhTw];

/// The width the content has in a 960-pixel window, where the pane folds.
fn content_at_960() -> f32 {
    content_width(960.0, navigation_compact(960.0))
}

fn every_visible_string(c: &'static Copy) -> Vec<&'static str> {
    vec![
        c.app_name,
        c.overview,
        c.processes,
        c.details,
        c.performance,
        c.startup_apps,
        c.services,
        c.history,
        c.events,
        c.system_info,
        c.settings,
        c.search_placeholder,
        c.pause_updates,
        c.resume_updates,
        c.paused_title,
        c.paused_message,
        c.end_task,
        c.end_process_tree,
        c.properties,
        c.tree_view,
        c.enable,
        c.disable,
        c.refresh,
        c.open_file_location,
        c.go_to_details,
        c.pause_process,
        c.resume_process,
        c.lower_priority,
        c.mark_incident,
        c.cpu,
        c.memory,
        c.disk,
        c.network,
        c.gpu,
        c.battery,
        c.temperatures,
        c.using_the_most,
        c.waiting_for_first_reading,
        c.no_chart,
        c.not_available,
        c.not_read,
        c.gpu_not_found,
        c.battery_not_read,
        c.apps_section,
        c.background_processes,
        c.nothing_matches,
        c.column_name,
        c.column_user,
        c.column_state,
        c.column_cpu_time,
        c.column_memory,
        c.column_swap,
        c.column_disk,
        c.column_read,
        c.column_write,
        c.column_threads,
        c.column_descriptors,
        c.column_running_for,
        c.column_nice,
        c.column_cgroup,
        c.column_command_line,
        c.column_status,
        c.column_starts_for,
        c.column_command,
        c.column_service_name,
        c.column_at_login,
        c.state_column_tooltip,
        c.partial_total,
        c.seconds_old,
        c.not_yet_sampled,
        c.read_failed,
        c.entity_disappeared,
        c.interface_missing,
        c.not_reported,
        c.policy_withheld,
        c.permission_denied,
        c.not_collected,
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
        c.outcome_signal_sent,
        c.outcome_priority_changed,
        c.failure_disappeared,
        c.failure_denied,
        c.failure_invalid,
        c.failure_unsupported,
        c.failure_other,
        c.refusal_other_user,
        c.refusal_ownership_unknown,
        c.refusal_protected,
        c.refusal_raise_priority,
        c.refusal_nice_range,
        c.refusal_priority_unknown,
        c.no_location,
        c.switched_on,
        c.switched_off,
        c.switch_failed,
        c.not_reported_yet,
        c.sixty_seconds,
        c.overall_utilization,
        c.logical_processors,
        c.utilization,
        c.utilization_percent,
        c.utilization_per_processor,
        c.speed,
        c.maximum_speed,
        c.processes_count,
        c.threads,
        c.up_time,
        c.waiting_to_run,
        c.waiting_to_run_hint,
        c.temperature,
        c.memory_usage,
        c.memory_composition,
        c.memory_hint,
        c.in_use,
        c.modified,
        c.cached,
        c.free,
        c.available,
        c.committed,
        c.swap,
        c.swap_off,
        c.active_time,
        c.transfer_rate,
        c.read_solid_write_dashed,
        c.response_time,
        c.read_speed,
        c.write_speed,
        c.capacity,
        c.kind,
        c.ssd,
        c.hdd,
        c.throughput,
        c.receive_solid_send_dashed,
        c.send,
        c.receive,
        c.connection,
        c.link_speed,
        c.wired,
        c.other_connection,
        c.not_read_yet_title,
        c.gpu_not_read,
        c.graphics_cards,
        c.processor_temperature,
        c.temperature_not_found,
        c.fans_not_read,
        c.startup_hint,
        c.enabled,
        c.disabled,
        c.not_on_this_desktop,
        c.no_startup_apps,
        c.loading,
        c.services_hint,
        c.no_services,
        c.no_service_manager,
        c.no_service_manager_advice,
        c.appearance_and_language,
        c.appearance,
        c.appearance_description,
        c.dark_theme,
        c.light_theme,
        c.system_theme,
        c.language,
        c.language_description,
        c.system_default,
        c.privacy,
        c.show_command_lines,
        c.privacy_description,
        c.how_it_works,
        c.grouping_title,
        c.grouping_description,
        c.data_sources,
        c.readings_this_second,
        c.sampling,
        c.sampling_description,
        c.health_healthy,
        c.health_degraded,
        c.health_failed,
        c.health_unsupported,
        c.collection_connecting,
        c.collection_service,
        c.collection_in_process_title,
        c.collection_in_process,
        c.collection_unavailable_title,
        c.collection_unavailable,
        c.request_failed,
        c.history_subtitle,
        c.span_15_minutes,
        c.span_1_hour,
        c.span_6_hours,
        c.no_history_yet,
        c.no_history_explained,
        c.history_truncated,
        c.history_processor,
        c.history_memory,
        c.history_disk,
        c.history_network,
        c.history_waiting,
        c.stored_samples,
        c.one_sample_every,
        c.kept_for,
        c.disk_budget,
        c.gaps_title,
        c.no_gaps,
        c.gap_service_stopped,
        c.gap_missed_cadence,
        c.gap_interrupted_write,
        c.gap_retention,
        c.events_intro_title,
        c.events_intro,
        c.mark_unavailable,
        c.no_events_yet,
        c.no_events_explained,
        c.marked_moments,
        c.what_changed,
        c.baseline_none,
        c.busiest_then,
        c.inventory_never_captured,
        c.inventory_first_capture,
        c.inventory_no_changes,
        c.changes_title,
        c.changes_since_last,
        c.stays_on_this_computer,
    ]
}

/// Text that looks like an internal name: a dotted key such as
/// `cpu.utilization.busy` or `linux.cpu`.
fn looks_like_a_key(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || "()「」,:;\"".contains(c))
        .any(|word| {
            let parts: Vec<&str> = word.trim_end_matches('.').split('.').collect();
            parts.len() >= 2
                && parts.iter().all(|part| {
                    !part.is_empty()
                        && part
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                })
                && parts
                    .iter()
                    .any(|part| part.chars().any(|c| c.is_ascii_lowercase()))
        })
}

#[test]
fn every_visible_string_exists_in_both_locales_and_names_no_internal_key() {
    for c in [&EN, &ZH] {
        for text in every_visible_string(c) {
            assert!(!text.trim().is_empty());
            assert!(!looks_like_a_key(text), "{text:?} shows an internal name");
        }
    }
    let differing = every_visible_string(&EN)
        .into_iter()
        .zip(every_visible_string(&ZH))
        .filter(|(en, zh)| en != zh)
        .count();
    // Only the few words both languages share (PID, CPU, SSD, Wi‑Fi) match.
    assert!(differing >= every_visible_string(&EN).len() - 6);
    assert!(looks_like_a_key("cpu.utilization.busy"));
    assert!(!looks_like_a_key("Asked it to close."));
}

#[test]
fn the_language_choice_is_stored_and_read_back() {
    for locale in Locale::ALL {
        assert_eq!(Locale::parse(locale.as_str()), Some(locale));
    }
    assert_eq!(Locale::parse("klingon"), None);
    assert!(Locale::ZhTw.is_chinese());
    assert!(!Locale::EnUs.is_chinese());
    assert_eq!(
        Locale::ZhTw.words(),
        better_ui::fluent::words::Locale::TraditionalChinese
    );
}

#[test]
fn every_process_state_is_translated_and_explained() {
    // Every name the Linux collector can report (`state_name`).
    let states = [
        "running",
        "sleeping",
        "uninterruptible",
        "zombie",
        "stopped",
        "tracing-stop",
        "dead",
        "idle",
        "paging",
        "wakekill",
        "parked",
        "unknown",
    ];
    for state in states {
        let (en, en_why) = i18n::process_state(state, Locale::EnUs);
        let (zh, zh_why) = i18n::process_state(state, Locale::ZhTw);
        assert_ne!(en, state, "{state} shown raw");
        assert!(zh.chars().any(|c| c as u32 > 0x2E80), "{state}: {zh}");
        assert!(!en_why.is_empty() && !zh_why.is_empty());
    }
    assert_eq!(i18n::process_state("sleeping", Locale::ZhTw).0, "等待中");
    assert_eq!(i18n::process_state("stopped", Locale::EnUs).0, "Suspended");
}

#[test]
fn the_navigation_folds_to_icons_below_1008_pixels_and_never_disappears() {
    assert!(navigation_compact(960.0));
    assert!(navigation_compact(800.0));
    assert!(!navigation_compact(1008.0));
    assert!(!navigation_compact(1600.0));
    // At the narrowest the window allows, the content still has room.
    assert!(content_width(crate::layout::MIN_WINDOW_WIDTH, true) > 700.0);
    assert_eq!(card_columns(content_at_960()), 3);
    assert_eq!(card_columns(content_width(1600.0, false)), 3);
    assert_eq!(card_columns(500.0), 1);
}

#[test]
fn no_page_title_or_command_is_cut_off_at_960_pixels() {
    // A command that does not fit moves to the overflow menu, so what is
    // asserted is that the title keeps its room and at least the first
    // command stays on the bar in both languages.
    for locale in LOCALES {
        let c = copy(locale);
        for (title, commands) in [
            (
                c.details,
                vec![
                    c.end_task,
                    c.end_process_tree,
                    c.properties,
                    c.tree_view,
                    c.pause_updates,
                ],
            ),
            (c.processes, vec![c.end_task, c.properties, c.pause_updates]),
            (c.startup_apps, vec![c.enable, c.disable, c.refresh]),
            (c.services, vec![c.enable, c.disable, c.refresh]),
            (c.events, vec![c.mark_incident]),
        ] {
            let fit = commands_that_fit(content_at_960(), title, &commands);
            assert!(fit >= 1, "{locale:?} {title}: no command fits");
            assert!(label_width(title) * 2.0 < content_at_960() / 2.0, "{title}");
        }
    }
}

#[test]
fn every_details_heading_fits_its_column_at_960_pixels_in_both_languages() {
    for locale in LOCALES {
        let c = copy(locale);
        let mut fixed = Vec::new();
        for column in tables::detail_columns(false) {
            let title = tables::column_title(column, c);
            if let Some(width) = tables::detail_width(column) {
                assert!(
                    heading_fits(title, width),
                    "{locale:?} heading {title:?} is cut off in {width}px"
                );
                fixed.push(width);
            }
        }
        // The name keeps a usable column at 960.
        assert!(flexible_width(content_at_960(), &fixed) >= 180.0);
    }
}

#[test]
fn every_other_table_heading_fits_its_column() {
    for locale in LOCALES {
        let c = copy(locale);
        for (title, width) in [
            (c.column_cpu, 96.0),
            (c.column_memory, 120.0),
            (c.column_disk, 120.0),
            (c.column_status, 180.0),
            (c.column_starts_for, 180.0),
            (c.column_status, 120.0),
            (c.column_at_login, 170.0),
        ] {
            assert!(
                heading_fits(title, width),
                "{locale:?} {title:?} in {width}"
            );
        }
        // The short words that sit in those fixed columns fit too.
        for word in [
            c.enabled,
            c.disabled,
            c.not_on_this_desktop,
            c.not_installed,
            i18n::scope_word(Scope::EveryoneChangedByYou, locale),
        ] {
            assert!(heading_fits(word, 180.0 + 16.0), "{locale:?} {word:?}");
        }
        for state in [
            StartupState::Enabled,
            StartupState::Disabled,
            StartupState::OnDemand,
            StartupState::Masked,
            StartupState::Temporary,
        ] {
            let service = startup::ServiceUnit {
                unit: "x.service".into(),
                description: String::new(),
                activity: Activity::Running,
                startup: state,
            };
            let (word, _) = i18n::startup_word(&service, locale);
            assert!(heading_fits(word, 170.0 + 16.0), "{locale:?} {word:?}");
        }
        let better = startup::ServiceUnit {
            unit: "better-monitor.service".into(),
            description: String::new(),
            activity: Activity::Running,
            startup: StartupState::Enabled,
        };
        let (word, _) = i18n::startup_word(&better, locale);
        assert!(heading_fits(word, 170.0 + 16.0), "{locale:?} {word:?}");
    }
}

#[test]
fn a_missing_reading_is_a_dash_with_its_reason_never_a_number() {
    let mut process = ProcessFacts::synthetic(4242, "unreadable");
    process.cpu_utilization = Field::Unknown(UnknownReason::NotYetSampled);
    process.memory_resident = Field::Unsupported(UnsupportedReason::NotReported {
        detail: "kernel thread".into(),
    });
    process.file_descriptors = Field::PermissionDenied {
        path: "/proc/4242/fd".into(),
    };
    process.threads = Field::NotCollected;
    for locale in LOCALES {
        for column in [
            ProcessColumn::CpuUtilization,
            ProcessColumn::Memory,
            ProcessColumn::FileDescriptors,
            ProcessColumn::Threads,
        ] {
            let shown = tables::process_cell(&process, column, locale, Some(1.0));
            assert_eq!(shown.text.as_ref(), "—", "{column:?}");
            assert_eq!(shown.tone, CellTone::Secondary);
            let why = shown.tooltip.expect("a reason on hover");
            assert!(!why.chars().any(|c| c.is_ascii_digit()) || why.contains("/proc"));
        }
        // The properties sheet writes the reason out in full.
        let rows = i18n::properties(&process, locale, Some(1.0));
        let memory = rows
            .iter()
            .find(|(name, _, _)| name == copy(locale).column_memory)
            .unwrap();
        assert!(memory.2, "flagged as not a reading");
        assert!(memory.1.contains("kernel thread"));
    }
    // And a measured zero is still a number.
    assert_eq!(
        tables::process_cell(&process, ProcessColumn::Swap, Locale::EnUs, Some(1.0))
            .text
            .as_ref(),
        "0 B"
    );
    assert!(matches!(
        cell(&process.threads, |v| v.to_string()),
        Cell::Missing(NonValue::NotCollected)
    ));
}

#[test]
fn process_states_reach_the_table_translated_with_their_explanation() {
    let mut process = ProcessFacts::synthetic(7, "worker");
    process.state = Field::Value("uninterruptible".into());
    let shown = tables::process_cell(&process, ProcessColumn::State, Locale::ZhTw, Some(1.0));
    assert_eq!(shown.text.as_ref(), "等候磁碟");
    assert!(shown.tooltip.is_some());
}

#[test]
fn every_number_carries_a_unit() {
    assert_eq!(tables::bytes(1536.0), "1.5 KB");
    assert_eq!(tables::rate(2.0 * 1024.0 * 1024.0), "2.0 MB/s");
    assert_eq!(tables::share(0.034), "3.4%");
    assert_eq!(tables::share(0.5), "50%");
    assert_eq!(i18n::bits_per_second(866_700_000.0), "867 Mbps");
    assert_eq!(i18n::bits_per_second(1e9), "1.0 Gbps");
    assert_eq!(
        i18n::uptime(3 * 86_400 + 5 * 3_600, Locale::EnUs),
        "3 days 5 h"
    );
    assert_eq!(
        i18n::uptime(3 * 86_400 + 5 * 3_600, Locale::ZhTw),
        "3 天 5 小時"
    );
    assert_eq!(i18n::uptime(200, Locale::EnUs), "3 min 20 s");
}

#[test]
fn ending_a_tree_ends_the_children_before_their_parents() {
    let mut processes = Vec::new();
    for (pid, parent) in [(10, 1), (11, 10), (12, 10), (13, 11), (20, 1)] {
        let mut process = ProcessFacts::synthetic(pid, "p");
        process.parent_pid = Field::Value(parent as u64);
        processes.push(process);
    }
    let order = tables::tree_of(10, &processes);
    assert_eq!(order.len(), 4);
    assert_eq!(*order.last().unwrap(), 10, "the root goes last");
    let at = |pid| order.iter().position(|p| *p == pid).unwrap();
    assert!(at(13) < at(11), "a grandchild before its parent");
    assert!(!order.contains(&20), "an unrelated process is not touched");
    // A process that names itself as its parent does not loop.
    let mut own = ProcessFacts::synthetic(5, "loop");
    own.parent_pid = Field::Value(5);
    assert_eq!(tables::tree_of(5, &[own]), vec![5]);
}

#[test]
fn the_details_table_hides_command_lines_until_they_are_turned_on() {
    assert!(!tables::detail_columns(false).contains(&ProcessColumn::CommandLine));
    assert!(tables::detail_columns(true).contains(&ProcessColumn::CommandLine));
    let mut model = ProcessTableModel::new(vec![ProcessFacts::synthetic(1, "systemd")]);
    assert!(!model.columns().contains(&ProcessColumn::CommandLine));
    model.set_show_command_line(true);
    assert!(model.columns().contains(&ProcessColumn::CommandLine));
}

#[test]
fn applications_sort_by_any_column_with_unmeasured_ones_last() {
    use monitor_views::AppsModel;
    use monitor_views::grouping::GroupingPrecedence;
    let mut a = ProcessFacts::synthetic(100, "alpha");
    a.cgroup = Field::Value(
        "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-alpha-1.scope".into(),
    );
    a.cpu_utilization = Field::Value(0.10);
    a.memory_resident = Field::Value(10);
    let mut b = ProcessFacts::synthetic(200, "beta");
    b.cgroup = Field::Value(
        "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-beta-1.scope".into(),
    );
    b.cpu_utilization = Field::Value(0.50);
    b.memory_resident = Field::NotCollected;
    let model = AppsModel::new(vec![a, b], GroupingPrecedence::default());
    let rows = model.applications();
    let names = |order: Vec<usize>| -> Vec<String> {
        order
            .into_iter()
            .map(|i| rows[i].group.display_name.to_lowercase())
            .collect()
    };
    assert_eq!(
        names(tables::app_order(rows, tables::AppColumn::Cpu, false))[0],
        "beta"
    );
    // Memory: beta measured nothing, so it is last in both directions.
    assert_eq!(
        names(tables::app_order(rows, tables::AppColumn::Memory, false))[1],
        "beta"
    );
    assert_eq!(
        names(tables::app_order(rows, tables::AppColumn::Memory, true))[1],
        "beta"
    );
    let lines = tables::app_lines(&model, tables::AppColumn::Name, true);
    assert!(matches!(
        lines[0],
        tables::AppLine::Section {
            services: false,
            count: 2
        }
    ));
}

fn report(collector: &str, metrics: MetricSet, entities: Vec<Entity>) -> CollectorReport {
    let mut report = CollectorReport::new(
        CollectorId::new(collector).unwrap(),
        Timestamp {
            unix_ms: 1_000,
            monotonic_ns: 1,
        },
    );
    report.metrics = metrics;
    report.entities = entities;
    report
}

fn set(values: &[(&str, Observation)]) -> MetricSet {
    let mut set = MetricSet::new();
    for (id, observation) in values {
        set.insert(MetricId::new(*id).unwrap(), observation.clone());
    }
    set
}

fn entity(kind: EntityKind, key: &str, metrics: MetricSet) -> Entity {
    Entity::new(EntityId::new(kind, key), metrics)
}

fn round(cpu: Option<f64>) -> Vec<CollectorReport> {
    let mut system = vec![("cpu.logical.count", Observation::unsigned(2))];
    if let Some(cpu) = cpu {
        system.push(("cpu.utilization.busy", Observation::float(cpu)));
    }
    system.push(("cpu.boot_time", Observation::float(100.0)));
    vec![
        report(
            "linux.cpu",
            set(&system),
            vec![
                entity(
                    EntityKind::LogicalCpu,
                    "1",
                    set(&[("cpu.utilization.busy", Observation::float(0.2))]),
                ),
                entity(
                    EntityKind::LogicalCpu,
                    "0",
                    set(&[
                        ("cpu.utilization.busy", Observation::float(0.4)),
                        ("cpu.frequency.current", Observation::float(2e9)),
                    ]),
                ),
            ],
        ),
        report(
            "linux.memory",
            set(&[
                ("memory.total", Observation::unsigned(1000)),
                ("memory.available", Observation::unsigned(250)),
                ("memory.cached", Observation::unsigned(100)),
                ("memory.buffers", Observation::unsigned(20)),
            ]),
            Vec::new(),
        ),
        report(
            "linux.network",
            MetricSet::new(),
            vec![
                entity(
                    EntityKind::NetworkInterface,
                    "lo",
                    set(&[("network.link.type", Observation::text("loopback"))]),
                ),
                entity(
                    EntityKind::NetworkInterface,
                    "eth0",
                    set(&[
                        ("network.link.type", Observation::text("ethernet")),
                        ("network.link.state", Observation::text("down")),
                    ]),
                ),
                entity(
                    EntityKind::NetworkInterface,
                    "wlan0",
                    set(&[
                        ("network.link.type", Observation::text("wifi")),
                        ("network.link.state", Observation::text("up")),
                        ("network.rx.bytes.rate", Observation::float(300.0)),
                        ("network.tx.bytes.rate", Observation::float(100.0)),
                    ]),
                ),
            ],
        ),
    ]
}

#[test]
fn a_round_becomes_the_figures_a_person_reads() {
    let readings = Readings::from_reports(&round(Some(0.3)), 1_100.0);
    assert_eq!(readings.cpu.utilization, Some(0.3));
    assert_eq!(readings.cpu.cores.len(), 2);
    assert_eq!(readings.cpu.cores[0].index, 0, "processors in kernel order");
    assert_eq!(readings.cpu.speed_hz, Some(2e9));
    assert_eq!(readings.cpu.uptime_seconds, Some(1_000.0));
    assert_eq!(readings.memory.in_use(), Some(750));
    assert_eq!(readings.memory.in_use_fraction(), Some(0.75));
    assert_eq!(
        readings.memory.cached,
        Some(120),
        "cache and buffers together"
    );
    assert!(
        !readings.has_storage,
        "no disk report is not an empty disk list"
    );
    // The loopback adapter is left out, and the connected one comes first.
    let names: Vec<&str> = readings.adapters.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["wlan0", "eth0"]);
    assert_eq!(readings.adapters[0].kind, AdapterKind::Wireless);
    assert_eq!(readings.adapters[1].connected, Some(false));
    assert_eq!(readings.network_total(), (Some(300.0), Some(100.0)));
    assert_eq!(readings.disk_total(), (None, None));
}

#[test]
fn the_live_charts_keep_a_minute_and_leave_a_gap_for_a_missing_reading() {
    let mut history = LiveHistory::default();
    history.record(&Readings::from_reports(&round(Some(0.5)), 0.0));
    history.record(&Readings::from_reports(&round(None), 0.0));
    history.record(&Readings::from_reports(&round(Some(0.25)), 0.0));
    assert_eq!(
        history.values(&SeriesKey::Cpu),
        vec![Some(50.0), None, Some(25.0)]
    );
    assert_eq!(history.values(&SeriesKey::Memory), vec![Some(750.0); 3]);
    for _ in 0..(CAPACITY + 5) {
        history.record(&Readings::from_reports(&round(Some(0.1)), 0.0));
    }
    assert_eq!(history.values(&SeriesKey::Cpu).len(), CAPACITY);
    assert_eq!(history.rounds(), CAPACITY as u64 + 8);
    // A round without the network report: the adapter's line gets a gap,
    // and after a whole chart's width of silence it is forgotten.
    let without_network: Vec<CollectorReport> = round(Some(0.1))
        .into_iter()
        .filter(|r| r.collector.as_str() != "linux.network")
        .collect();
    history.record(&Readings::from_reports(&without_network, 0.0));
    assert_eq!(
        history.values(&SeriesKey::Receive("wlan0".into())).last(),
        Some(&None)
    );
    for _ in 0..CAPACITY {
        history.record(&Readings::from_reports(&without_network, 0.0));
    }
    assert!(
        history
            .values(&SeriesKey::Receive("wlan0".into()))
            .is_empty()
    );
}

#[test]
fn the_overview_sentence_keeps_busy_and_waiting_apart() {
    let readings = Readings::from_reports(&round(Some(0.3)), 0.0);
    let mut overview = OverviewModel::from_reports(&[]);
    overview.cpu.verdict = ResourceVerdict::Nominal;
    overview.memory.resource.verdict = ResourceVerdict::Nominal;
    overview.io.verdict = ResourceVerdict::Nominal;
    let (tone, text, _) = i18n::health_sentence(&overview, &readings, None, Locale::EnUs);
    assert_eq!(tone, i18n::Tone::Good);
    assert_eq!(text, "Your computer is running smoothly.");

    overview.cpu.verdict = ResourceVerdict::BusyWithoutContention;
    let (tone, text, _) =
        i18n::health_sentence(&overview, &readings, Some("Firefox"), Locale::ZhTw);
    assert_eq!(tone, i18n::Tone::Busy);
    assert!(text.contains("沒有程式需要等待"));

    overview.memory.resource.verdict = ResourceVerdict::UnderPressure { some_percent: 30.0 };
    overview.io.verdict = ResourceVerdict::Saturated { full_percent: 20.0 };
    let (tone, text, detail) =
        i18n::health_sentence(&overview, &readings, Some("Firefox"), Locale::EnUs);
    assert_eq!(tone, i18n::Tone::Strained);
    assert_eq!(
        text,
        "Your computer is slowing down: programs are waiting for memory and the disk."
    );
    assert_eq!(
        detail.as_deref(),
        Some("Firefox is using the most processor time right now.")
    );

    let nothing = Readings::default();
    let (tone, _, _) = i18n::health_sentence(&overview, &nothing, None, Locale::EnUs);
    assert_eq!(tone, i18n::Tone::Unknown, "no readings is not a verdict");
}

#[test]
fn every_resource_state_has_words_in_both_languages() {
    for verdict in [
        ResourceVerdict::Nominal,
        ResourceVerdict::BusyWithoutContention,
        ResourceVerdict::UnderPressure { some_percent: 1.0 },
        ResourceVerdict::Saturated { full_percent: 1.0 },
        ResourceVerdict::CollectorFailed { detail: "x".into() },
        ResourceVerdict::Unsupported { detail: "x".into() },
        ResourceVerdict::Unobserved { detail: "x".into() },
    ] {
        assert_ne!(
            i18n::verdict_word(&verdict, Locale::EnUs),
            i18n::verdict_word(&verdict, Locale::ZhTw)
        );
    }
}

#[test]
fn every_non_value_reason_and_grouping_has_words() {
    for locale in LOCALES {
        let c = copy(locale);
        for reason in [
            NonValue::NotYetSampled,
            NonValue::IntervalTooShort,
            NonValue::ReadFailed { detail: "x".into() },
            NonValue::Malformed { detail: "x".into() },
            NonValue::EntityDisappeared,
            NonValue::InterfaceMissing { path: "x".into() },
            NonValue::NotReported { detail: "x".into() },
            NonValue::PolicyWithheld { policy: "x".into() },
            NonValue::PermissionDenied { path: "x".into() },
            NonValue::NotCollected,
        ] {
            assert!(!tables::non_value_label(&reason, c).is_empty());
        }
        for kind in monitor_views::grouping::GroupingPrecedence::default().order() {
            assert!(!i18n::evidence_kind_word(*kind, locale).is_empty());
            assert!(!i18n::evidence_kind_explanation(*kind, locale).is_empty());
        }
    }
}

#[test]
fn the_inventory_is_shown_in_words_and_unknown_keys_are_left_out() {
    use monitor_store::{Inventory, InventoryEntry};
    let mut inventory = Inventory::new(0);
    for (key, value) in [
        ("os.name", "Zorin OS"),
        ("kernel.release", "6.8.0"),
        ("memory.total_bytes", "1073741824"),
        ("storage.nvme0n1.rotational", "0"),
        ("graphics.card1.driver", "amdgpu"),
        ("filesystem..type", "ext4"),
        ("filesystem.home.source", "/dev/nvme0n1p3"),
        ("betteros.component.better-awake.version", "0.2.9"),
        ("collector.linux.cpu.supported_metrics", "27"),
        ("os.id", "zorin"),
    ] {
        inventory
            .entries
            .insert(key.to_string(), InventoryEntry::public(value));
    }
    for locale in LOCALES {
        let sections = i18n::inventory_sections(&inventory, locale);
        let rows: Vec<&(String, String, monitor_store::Sensitivity)> =
            sections.iter().flat_map(|(_, rows)| rows).collect();
        assert_eq!(rows.len(), 8, "the two unknown keys are left out");
        for (name, value, _) in rows {
            assert!(!looks_like_a_key(name), "{name}");
            assert!(!looks_like_a_key(value), "{value}");
        }
        // The operating system's name comes before its kernel.
        assert!(sections[0].1[0].1.contains("Zorin"));
    }
    let english = i18n::inventory_sections(&inventory, Locale::EnUs);
    let all: Vec<&(String, String, monitor_store::Sensitivity)> =
        english.iter().flat_map(|(_, rows)| rows).collect();
    assert!(
        all.iter()
            .any(|(n, v, _)| n == "Installed memory" && v == "1.0 GB")
    );
    assert!(
        all.iter()
            .any(|(n, v, _)| n == "/ file system" && v == "ext4")
    );
    assert!(all.iter().any(|(n, _, _)| n == "/home device"));
    assert!(
        all.iter()
            .any(|(n, v, _)| n == "nvme0n1 type" && v == "SSD")
    );
    assert_eq!(
        i18n::graphics_cards(Some(&inventory), Locale::EnUs),
        vec![("card1".to_string(), "Driver amdgpu".to_string())]
    );
}

#[test]
fn an_incident_compares_only_readings_it_has_words_for() {
    assert_eq!(
        i18n::shift_sentence("cpu.utilization.busy", 0.1, 0.9, Locale::EnUs),
        Some(("Processor use".to_string(), "10% → 90%".to_string()))
    );
    assert!(i18n::shift_sentence("cpu.something.new", 1.0, 2.0, Locale::EnUs).is_none());
    assert_eq!(i18n::ago(600_000, 0, Locale::EnUs), "10 minutes ago");
    assert_eq!(i18n::ago(600_000, 0, Locale::ZhTw), "10 分鐘前");
    assert_eq!(i18n::ago(0, 5, Locale::EnUs), "just now");
    assert_eq!(i18n::ago(7_200_000, 0, Locale::EnUs), "2 hours ago");
}

#[test]
fn a_link_failure_is_said_plainly_and_never_shows_its_key() {
    for detail in [
        "monitor.gui.link.privacy_owned_by_service",
        "monitor.ipc.error.invalid_window",
        "org.freedesktop.DBus.Error.ServiceUnknown",
    ] {
        for locale in LOCALES {
            let said = i18n::failure_sentence(detail, locale);
            assert!(!said.contains("monitor."), "{said}");
            assert!(!looks_like_a_key(&said), "{said}");
        }
    }
}

// History.

fn sample_at(wall_unix_ms: u64, cpu: Option<f64>) -> monitor_store::Sample {
    let mut metrics = vec![("memory.total", Observation::unsigned(100))];
    if let Some(cpu) = cpu {
        metrics.push(("cpu.utilization.busy", Observation::float(cpu)));
    }
    let mut sample =
        monitor_store::Sample::from_reports(&[report("linux.cpu", set(&metrics), Vec::new())], 0);
    sample.wall_unix_ms = wall_unix_ms;
    sample
}

#[test]
fn a_history_chart_is_placed_on_the_clock_and_leaves_gaps_where_nothing_was_recorded() {
    use crate::stored::{HistoryChart, binned};
    // A 60-second span in 5-second slots, ending at 60 s: readings at 5 s and
    // 7 s share a slot, a sample with no reading leaves its slot empty, and
    // a reading older than the span is not drawn.
    let samples = vec![
        sample_at(0, Some(0.9)),
        sample_at(5_000, Some(0.2)),
        sample_at(7_000, Some(0.4)),
        sample_at(30_000, None),
        sample_at(59_000, Some(0.0)),
    ];
    let values = binned(&samples, HistoryChart::Processor, 60_000, 60, 5);
    assert_eq!(values.len(), 12);
    assert!((values[1].unwrap() - 30.0).abs() < 1e-9, "{values:?}");
    assert_eq!(values[6], None, "the sample with no reading is a gap");
    assert_eq!(values[11], Some(0.0), "a measured zero is still a reading");
    assert_eq!(
        values[0],
        Some(90.0),
        "the reading at the start of the span"
    );
    assert_eq!(
        binned(&[], HistoryChart::Processor, 0, 21_600, 5).len(),
        720
    );
}

#[test]
fn the_history_page_charts_only_metrics_the_collectors_declare() {
    use monitor_collectors_linux::LinuxCollectors;
    let declared: std::collections::BTreeSet<String> = LinuxCollectors::descriptors()
        .into_iter()
        .map(|descriptor| descriptor.id.to_string())
        .collect();
    for chart in crate::stored::HistoryChart::ALL {
        for id in chart.metrics() {
            assert!(
                declared.contains(*id),
                "{chart:?} reads {id}, which nobody declares"
            );
        }
    }
}

#[test]
fn the_history_page_never_asks_for_more_than_the_protocol_allows() {
    const _: () = assert!(crate::link::MAX_HISTORY_SAMPLES <= monitor_ipc::MAX_SAMPLES_PER_REPLY);
    const _: () = assert!(crate::link::MAX_HISTORY_SAMPLES > 0);
}

#[test]
fn marking_is_offered_only_when_something_is_actually_recording() {
    use crate::app::Collection;
    for (collection, expected) in [
        (Collection::Connecting, false),
        (Collection::Service, true),
        (Collection::InProcess { detail: "x".into() }, true),
        (Collection::Unavailable { detail: "x".into() }, false),
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

// Actions.

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
fn every_end_question_names_what_it_ends_and_warns_about_unsaved_work() {
    use crate::app::EndTarget;
    for locale in LOCALES {
        for target in [
            EndTarget::Process {
                pid: 1,
                name: "gedit".into(),
            },
            EndTarget::Tree {
                pid: 1,
                name: "gedit".into(),
            },
            EndTarget::App {
                name: "gedit".into(),
                pids: vec![1, 2],
            },
        ] {
            let (title, body) = i18n::end_question(&target, 2, locale);
            assert!(title.contains("gedit"));
            assert!(body.contains("儲存") || body.contains("saved"), "{body}");
        }
    }
}

// Startup apps.

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("monitor-gui-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_desktop_entry_says_whether_it_starts_and_where() {
    let entry = DesktopEntry::parse(
        "# comment\n[Desktop Entry]\nName=Mail\nName[zh_TW]=郵件\nX-GNOME-Autostart-enabled=false\nOnlyShowIn=GNOME;Unity;\n[Desktop Action new]\nName=New\n",
    );
    assert!(!entry.starts());
    assert_eq!(entry.localized("Name", true), Some("郵件"));
    assert_eq!(entry.localized("Name", false), Some("Mail"));
    assert!(entry.for_desktops(&["GNOME".into()]));
    assert!(!entry.for_desktops(&["KDE".into()]));
    assert!(DesktopEntry::parse("[Desktop Entry]\nName=A\n").starts());
    assert!(!DesktopEntry::parse("[Desktop Entry]\nHidden=true\n").starts());
    let not = DesktopEntry::parse("[Desktop Entry]\nNotShowIn=GNOME;\n");
    assert!(!not.for_desktops(&["ubuntu".into(), "GNOME".into()]));
}

#[test]
fn switching_an_entry_rewrites_only_its_two_switches() {
    let source =
        "[Desktop Entry]\nName=Mail\nHidden=false\nExec=mail\n[Desktop Action new]\nHidden=keep\n";
    let off = startup::switched(source, false);
    let parsed = DesktopEntry::parse(&off);
    assert!(!parsed.starts());
    assert!(off.contains("Name=Mail") && off.contains("Exec=mail"));
    assert!(off.contains("[Desktop Action new]\nHidden=keep"), "{off}");
    assert_eq!(off.matches("Hidden=true").count(), 1);
    let on = startup::switched(&off, true);
    assert!(DesktopEntry::parse(&on).starts());
    assert!(
        DesktopEntry::parse(&startup::switched("", false))
            .fields
            .contains_key("Hidden")
    );
}

#[test]
fn turning_off_a_system_startup_app_writes_your_own_copy_and_never_the_system_file() {
    let root = scratch("autostart");
    let dirs = AutostartDirs {
        user: root.join("home/autostart"),
        system: vec![root.join("etc-a/autostart"), root.join("etc-b/autostart")],
        desktops: vec!["GNOME".into()],
        path: None,
    };
    for dir in &dirs.system {
        std::fs::create_dir_all(dir).unwrap();
    }
    let system_file = dirs.system[1].join("mail.desktop");
    let original = "[Desktop Entry]\nType=Application\nName=Mail\nExec=mail\n";
    std::fs::write(&system_file, original).unwrap();
    // The first directory in XDG_CONFIG_DIRS wins over a later one.
    std::fs::write(
        dirs.system[0].join("chat.desktop"),
        "[Desktop Entry]\nName=Chat A\n",
    )
    .unwrap();
    std::fs::write(
        dirs.system[1].join("chat.desktop"),
        "[Desktop Entry]\nName=Chat B\n",
    )
    .unwrap();
    std::fs::write(
        dirs.system[1].join("link.desktop"),
        "[Desktop Entry]\nType=Link\nName=Web\n",
    )
    .unwrap();

    let entries = startup::read_autostart(&dirs, false);
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Chat A", "Mail"], "links are not startup apps");
    let mail = entries.iter().find(|e| e.name == "Mail").unwrap().clone();
    assert!(mail.enabled);
    assert_eq!(mail.scope, Scope::Everyone);

    startup::set_autostart(&dirs, &mail, false).unwrap();
    assert_eq!(std::fs::read_to_string(&system_file).unwrap(), original);
    let entries = startup::read_autostart(&dirs, false);
    let mail = entries.iter().find(|e| e.name == "Mail").unwrap().clone();
    assert!(!mail.enabled);
    assert_eq!(mail.scope, Scope::EveryoneChangedByYou);

    startup::set_autostart(&dirs, &mail, true).unwrap();
    let mail = startup::read_autostart(&dirs, false)
        .into_iter()
        .find(|e| e.name == "Mail")
        .unwrap();
    assert!(mail.enabled);
    assert_eq!(std::fs::read_to_string(&system_file).unwrap(), original);
    let _ = std::fs::remove_dir_all(root);
}

// Services.

const UNITS: &str = "\
● tracker-miner-fs-3.service loaded failed failed Tracker file system data miner
  pipewire.service         loaded active running PipeWire Multimedia Service
  better-monitor.service   loaded active running Better Monitor
  oneshot.service          loaded active exited  Set things up
  gone.service             not-found inactive dead gone.service
";
const FILES: &str = "\
pipewire.service            enabled  enabled
tracker-miner-fs-3.service  enabled  enabled
better-monitor.service      enabled  enabled
oneshot.service             static   -
syncthing.service           disabled enabled
blocked.service             masked   enabled
template@.service           disabled enabled
";

#[test]
fn services_are_read_from_both_lists_and_switchable_only_where_that_means_something() {
    let services = startup::merge_services(UNITS, FILES);
    let units: Vec<&str> = services.iter().map(|s| s.unit.as_str()).collect();
    assert!(
        !units.contains(&"gone.service"),
        "not-found units are left out"
    );
    assert!(
        !units.contains(&"template@.service"),
        "templates are left out"
    );
    let find = |unit: &str| services.iter().find(|s| s.unit == unit).unwrap();
    assert_eq!(
        find("tracker-miner-fs-3.service").activity,
        Activity::Failed
    );
    assert_eq!(find("oneshot.service").activity, Activity::Finished);
    assert_eq!(find("syncthing.service").activity, Activity::Stopped);
    assert_eq!(find("syncthing.service").startup, StartupState::Disabled);
    assert_eq!(find("pipewire.service").lock(), None);
    assert_eq!(find("syncthing.service").lock(), None);
    assert_eq!(find("oneshot.service").lock(), Some(ServiceLock::OnDemand));
    assert_eq!(find("blocked.service").lock(), Some(ServiceLock::Masked));
    assert_eq!(
        find("better-monitor.service").lock(),
        Some(ServiceLock::BetterOs)
    );
    assert_eq!(
        find("pipewire.service").title(),
        "PipeWire Multimedia Service"
    );
    assert_eq!(find("syncthing.service").title(), "syncthing");
}

struct FakeSystemctl {
    calls: std::cell::RefCell<Vec<String>>,
}

impl Systemctl for FakeSystemctl {
    fn run(&self, arguments: &[&str]) -> Result<String, ServiceError> {
        self.calls.borrow_mut().push(arguments.join(" "));
        Ok(match arguments[0] {
            "list-units" => UNITS.to_string(),
            "list-unit-files" => FILES.to_string(),
            _ => String::new(),
        })
    }
}

#[test]
fn a_service_is_switched_with_systemctl_user_and_a_locked_one_never_reaches_it() {
    let fake = FakeSystemctl {
        calls: Default::default(),
    };
    let services = startup::read_services(&fake).unwrap();
    let syncthing = services
        .iter()
        .find(|s| s.unit == "syncthing.service")
        .unwrap();
    startup::set_service(&fake, syncthing, true).unwrap();
    let better = services
        .iter()
        .find(|s| s.unit == "better-monitor.service")
        .unwrap();
    assert!(startup::set_service(&fake, better, false).is_err());
    let calls = fake.calls.borrow();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[2], "enable -- syncthing.service",
        "`--` keeps a unit name from ever being read as an option"
    );
    assert!(!calls.iter().any(|call| call.contains("better-monitor")));
}

#[test]
fn the_real_systemctl_is_never_run_under_the_test_guard() {
    // The guard is passed in rather than set in the environment, so no test
    // mutates process-wide state another test thread could be reading.
    let result = startup::RealSystemctl::new("systemctl", true).run(&["list-units"]);
    assert!(
        matches!(result, Err(ServiceError::NoManager(_))),
        "{result:?}"
    );
}

#[test]
fn every_service_word_exists_in_both_languages() {
    for activity in [
        Activity::Running,
        Activity::Starting,
        Activity::Stopping,
        Activity::Stopped,
        Activity::Failed,
        Activity::Finished,
    ] {
        assert_ne!(
            i18n::activity_word(activity, Locale::EnUs).0,
            i18n::activity_word(activity, Locale::ZhTw).0
        );
    }
    for lock in [
        ServiceLock::BetterOs,
        ServiceLock::OnDemand,
        ServiceLock::Masked,
        ServiceLock::Temporary,
        ServiceLock::Unknown,
    ] {
        assert_ne!(
            i18n::lock_sentence(lock, Locale::EnUs),
            i18n::lock_sentence(lock, Locale::ZhTw)
        );
    }
    for scope in [Scope::Yours, Scope::Everyone, Scope::EveryoneChangedByYou] {
        assert_ne!(
            i18n::scope_word(scope, Locale::EnUs),
            i18n::scope_word(scope, Locale::ZhTw)
        );
    }
}

#[test]
fn a_chart_of_bytes_tops_out_at_a_round_binary_figure() {
    let mib = 1024.0 * 1024.0;
    assert_eq!(tables::byte_axis(190.0 * mib, 1024.0), 200.0 * mib);
    assert_eq!(
        tables::rate(tables::byte_axis(190.0 * mib, 1024.0)),
        "200 MB/s"
    );
    assert_eq!(tables::byte_axis(0.0, 1024.0), 1024.0);
    assert_eq!(tables::byte_axis(3_000.0, 1024.0), 5.0 * 1024.0);
}

#[test]
fn a_process_share_is_of_the_whole_machine_like_the_processor_page() {
    let mut process = ProcessFacts::synthetic(1, "busy");
    // One whole logical processor, on a machine with 24 of them.
    process.cpu_utilization = Field::Value(1.0);
    let shown = tables::process_cell(
        &process,
        ProcessColumn::CpuUtilization,
        Locale::EnUs,
        Some(24.0),
    );
    assert_eq!(shown.text.as_ref(), "4.2%");
    // Before the processor has reported its count, nothing is shown rather
    // than a share of one processor that would read 24 times too high.
    let early = tables::process_cell(&process, ProcessColumn::CpuUtilization, Locale::EnUs, None);
    assert_eq!(early.text.as_ref(), "—");
    assert_eq!(early.tone, CellTone::Secondary);
    // The Slow moments page follows the same rule.
    assert_eq!(
        crate::stored::machine_share(Some(1.0), Some(24.0)).as_deref(),
        Some("4.2%")
    );
    assert_eq!(crate::stored::machine_share(Some(1.0), None), None);
    let mut moment = sample_at(0, Some(0.5));
    moment.metrics.insert(
        MetricId::new("cpu.logical.count").unwrap(),
        Observation::unsigned(8),
    );
    assert_eq!(crate::stored::snapshot_processors(&moment), Some(8.0));
}

#[test]
fn a_failed_switch_reads_as_a_sentence_with_or_without_the_systems_words() {
    assert_eq!(
        i18n::switch_failed_sentence("", Locale::EnUs),
        "That couldn't be changed."
    );
    assert_eq!(
        i18n::switch_failed_sentence("Unit file is masked.", Locale::EnUs),
        "That couldn't be changed: Unit file is masked."
    );
    assert_eq!(
        i18n::switch_failed_sentence(" ", Locale::ZhTw),
        "無法變更。"
    );
}

// Review fixes.

#[test]
fn ending_is_never_offered_from_a_paused_list() {
    assert!(tables::end_offered(false, true));
    assert!(
        !tables::end_offered(true, true),
        "a frozen list may name a reused PID"
    );
    assert!(!tables::end_offered(false, false));
    for locale in LOCALES {
        assert!(!copy(locale).list_paused.is_empty());
        assert!(!copy(locale).refusal_protected_tree.is_empty());
    }
}

#[test]
fn the_tree_confirmation_says_how_many_processes_it_ends() {
    use crate::app::EndTarget;
    let target = EndTarget::Tree {
        pid: 1,
        name: "make".into(),
    };
    assert!(
        i18n::end_question(&target, 7, Locale::EnUs)
            .1
            .contains("7 processes")
    );
    assert!(
        i18n::end_question(&target, 7, Locale::ZhTw)
            .1
            .contains("7 個處理程序")
    );
}

#[test]
fn history_charts_are_worked_out_once_against_the_fetch_time() {
    use crate::stored::{HistoryChart, HistoryView};
    let document = monitor_ipc::HistoryDocument {
        slice: monitor_store::HistorySlice {
            samples: vec![sample_at(55_000, Some(0.5)), sample_at(59_000, Some(0.25))],
            ..Default::default()
        },
        resolution_seconds: 5,
    };
    let view = HistoryView::build(document, 60_000, 60);
    let (chart, values) = &view.charts[0];
    assert_eq!(*chart, HistoryChart::Processor);
    assert_eq!(values.len(), 12);
    // Both readings fall in the last slot of a span ending at the fetch.
    assert_eq!(values[11], Some(37.5));
    assert!(values[..11].iter().all(Option::is_none));
    assert_eq!(view.charts.len(), HistoryChart::ALL.len());
    assert_eq!(view.memory_total, Some(100.0));
}

#[test]
fn a_round_built_off_the_window_thread_keeps_the_tables_settings() {
    use crate::charts::{BuiltRound, TableSettings};
    use monitor_views::SortDirection;
    let mut a = ProcessFacts::synthetic(100, "alpha");
    a.cgroup = Field::Value(
        "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-alpha-1.scope".into(),
    );
    let mut b = ProcessFacts::synthetic(101, "alpha-helper");
    b.cgroup = a.cgroup.clone();
    let c = ProcessFacts::synthetic(200, "beta");
    let model = monitor_views::AppsModel::new(
        vec![a.clone(), b.clone(), c.clone()],
        monitor_views::grouping::GroupingPrecedence::default(),
    );
    let key = model.applications()[0].group.key.clone();
    let settings = TableSettings {
        sort: (ProcessColumn::Name, SortDirection::Ascending),
        filter: "alpha".into(),
        tree: false,
        command_lines: false,
        precedence: monitor_views::grouping::GroupingPrecedence::default(),
        expanded: vec![key.clone()],
    };
    // A round that carries no process report still builds, with empty
    // tables that show the same settings.
    let built = BuiltRound::build(round(Some(0.2)), 0.0, settings.clone());
    assert_eq!(built.processes.sort(), settings.sort);
    assert_eq!(built.processes.filter(), "alpha");
    assert_eq!(built.readings.cpu.utilization, Some(0.2));
    // Applied to a model with the group in it, the open group stays open.
    let mut apps = model;
    settings.apply_apps(&mut apps);
    assert!(apps.is_expanded(&key));
    let mut table = ProcessTableModel::new(vec![a, b, c]);
    settings.apply_processes(&mut table);
    assert_eq!(table.visible_rows().len(), 2, "the filter is applied");
}

#[test]
fn turning_a_system_default_back_on_removes_your_copy() {
    let root = scratch("reenable");
    let dirs = AutostartDirs {
        user: root.join("home/autostart"),
        system: vec![root.join("etc/autostart")],
        desktops: vec![],
        path: None,
    };
    std::fs::create_dir_all(&dirs.system[0]).unwrap();
    std::fs::write(
        dirs.system[0].join("mail.desktop"),
        "[Desktop Entry]\nType=Application\nName=Mail\nExec=mail\n",
    )
    .unwrap();
    let mail = || {
        startup::read_autostart(&dirs, false)
            .into_iter()
            .find(|e| e.name == "Mail")
            .unwrap()
    };
    startup::set_autostart(&dirs, &mail(), false).unwrap();
    assert!(dirs.user.join("mail.desktop").exists());
    startup::set_autostart(&dirs, &mail(), true).unwrap();
    assert!(
        !dirs.user.join("mail.desktop").exists(),
        "a copy that only switched it off is removed, so the default is in force again"
    );
    assert_eq!(mail().scope, Scope::Everyone);
    assert!(mail().enabled);
    // No temporary file is left behind.
    assert_eq!(std::fs::read_dir(&dirs.user).unwrap().count(), 0);
    // A copy the person changed in other ways is kept and switched instead.
    startup::set_autostart(&dirs, &mail(), false).unwrap();
    let copy = dirs.user.join("mail.desktop");
    let text = std::fs::read_to_string(&copy)
        .unwrap()
        .replace("Exec=mail", "Exec=mail --quiet");
    std::fs::write(&copy, text).unwrap();
    startup::set_autostart(&dirs, &mail(), true).unwrap();
    assert!(copy.exists());
    assert!(mail().enabled);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_entry_whose_program_is_gone_is_shown_as_not_installed() {
    let root = scratch("tryexec");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("tool");
    std::fs::write(&tool, "#!/bin/sh\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::ffi::OsString::from(bin.as_os_str());
    assert!(startup::program_installed("tool", Some(&path)));
    assert!(startup::program_installed(tool.to_str().unwrap(), None));
    assert!(!startup::program_installed("missing-tool", Some(&path)));
    assert!(!startup::program_installed("/nowhere/tool", Some(&path)));
    let dirs = AutostartDirs {
        user: root.join("autostart"),
        system: vec![],
        desktops: vec![],
        path: Some(path),
    };
    std::fs::create_dir_all(&dirs.user).unwrap();
    std::fs::write(
        dirs.user.join("a.desktop"),
        "[Desktop Entry]\nName=Has it\nTryExec=tool\n",
    )
    .unwrap();
    std::fs::write(
        dirs.user.join("b.desktop"),
        "[Desktop Entry]\nName=Lost it\nTryExec=missing-tool\n",
    )
    .unwrap();
    let entries = startup::read_autostart(&dirs, false);
    assert!(
        entries
            .iter()
            .find(|e| e.name == "Has it")
            .unwrap()
            .installed
    );
    assert!(
        !entries
            .iter()
            .find(|e| e.name == "Lost it")
            .unwrap()
            .installed
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_systemctl_that_writes_more_than_a_pipe_holds_does_not_stall() {
    let root = scratch("bigoutput");
    let script = root.join("systemctl");
    // 200 KiB on stdout and some on stderr, then exit 0.
    std::fs::write(
        &script,
        "#!/bin/sh\nhead -c 204800 /dev/zero | tr '\\0' x\necho done >&2\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let started = std::time::Instant::now();
    let output = startup::RealSystemctl::new(script.as_os_str(), false)
        .run(&["list-units"])
        .expect("the output is read while the child runs");
    assert_eq!(output.len(), 204_800);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "no stall until the timeout"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn only_a_plain_service_name_reaches_systemctl() {
    for good in [
        "pipewire.service",
        "app-gnome-x@1.service",
        "dbus-:1.2-org.a.service",
    ] {
        assert!(startup::plain_unit_name(good), "{good}");
    }
    for bad in [
        "--now.service",
        "-x.service",
        ".service",
        "a b.service",
        "x.timer",
        "a;rm.service",
    ] {
        assert!(!startup::plain_unit_name(bad), "{bad}");
    }
    let fake = FakeSystemctl {
        calls: Default::default(),
    };
    let odd = startup::ServiceUnit {
        unit: "--global.service".into(),
        description: String::new(),
        activity: Activity::Stopped,
        startup: StartupState::Disabled,
    };
    assert_eq!(
        startup::set_service(&fake, &odd, true),
        Err(ServiceError::InvalidName("--global.service".into()))
    );
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn a_failed_switch_never_leads_with_systemctl_or_a_debug_string() {
    for locale in LOCALES {
        let locked = i18n::service_error_detail(&ServiceError::Locked(ServiceLock::Masked), locale);
        assert_eq!(locked, i18n::lock_sentence(ServiceLock::Masked, locale));
        assert!(!locked.contains("Masked"), "no enum name");
        assert!(i18n::switch_failed_title("Syncthing", locale).contains("Syncthing"));
    }
    assert_eq!(
        i18n::service_error_detail(
            &ServiceError::Refused(" Unit is masked.\n".into()),
            Locale::EnUs
        ),
        "Unit is masked."
    );
}
