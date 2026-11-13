//! The rows of the Processes and Details tables, decided without a window.
//!
//! The models in `monitor-views` decide grouping, filtering, and the tree;
//! this module decides what each row says: the cells, their order, and how a
//! reading that has no value is written. Every cell goes through
//! [`reading`], the single place a reading becomes text. A reading with no
//! value is written as the reason it has none, in the secondary tone, and
//! never as `0`.
//!
//! The windows draws the rows with `better_ui::fluent::list::DetailsTable`.

use std::cmp::Ordering;

use better_ui::fluent::list::{CellTone, TableCell};
use better_ui::fluent::words;
use monitor_views::apps::{Aggregate, AppRow};
use monitor_views::format::{Cell, NonValue, cell};
use monitor_views::grouping::{Confidence, GroupingEvidence};
use monitor_views::{AppsModel, ProcessColumn, ProcessFacts};

use crate::i18n::{Copy, Locale, process_state};

/// Bytes as a person reads them, in Windows' units: `1.2 MB`.
pub(crate) fn bytes(value: f64) -> String {
    words::bytes(value.max(0.0) as u64)
}

/// A rate in bytes per second: `1.2 MB/s`.
pub(crate) fn rate(value: f64) -> String {
    format!("{}/s", bytes(value))
}

/// A round top for a chart of bytes or bytes per second, in the binary
/// units [`bytes`] writes, so the axis reads `2 MB/s` rather than `1.9`.
pub(crate) fn byte_axis(peak: f64, floor: f64) -> f64 {
    let peak = peak.max(floor);
    let mut unit = 1.0_f64;
    while peak / unit >= 1024.0 && unit < 1024f64.powi(5) {
        unit *= 1024.0;
    }
    better_ui::fluent::chart::nice_maximum(peak / unit, 1.0) * unit
}

/// A fraction of `0.0..=1.0` as a percentage with one decimal below ten,
/// the way Task Manager writes a process's processor share.
pub(crate) fn share(value: f64) -> String {
    let percent = (value * 100.0).max(0.0);
    if percent < 10.0 {
        format!("{percent:.1}%")
    } else {
        format!("{percent:.0}%")
    }
}

/// The words for a reading that has no value.
pub(crate) fn non_value_label(reason: &NonValue, c: &'static Copy) -> &'static str {
    match reason {
        NonValue::NotYetSampled => c.not_yet_sampled,
        NonValue::IntervalTooShort => c.interval_too_short,
        NonValue::ReadFailed { .. } => c.read_failed,
        NonValue::Malformed { .. } => c.malformed,
        NonValue::EntityDisappeared => c.entity_disappeared,
        NonValue::InterfaceMissing { .. } => c.interface_missing,
        NonValue::NotReported { .. } => c.not_reported,
        NonValue::PolicyWithheld { .. } => c.policy_withheld,
        NonValue::PermissionDenied { .. } => c.permission_denied,
        NonValue::NotCollected => c.not_collected,
    }
}

/// One reading as a table cell.
///
/// A measured value is plain text. A stale one carries its age. A missing one
/// is a dash in the secondary tone with the reason on hover, which is
/// visibly and semantically different from a zero.
pub(crate) fn reading(rendered: Cell, c: &'static Copy) -> TableCell {
    match rendered {
        Cell::Value(text) => TableCell::new(text),
        Cell::Stale { text, age_seconds } => {
            TableCell::new(format!("{text} ({age_seconds} {})", c.seconds_old))
                .tone(CellTone::Caution)
        }
        // A dash keeps a narrow column readable; the reason is one pointer
        // rest away, and the properties sheet writes it out in full.
        Cell::Missing(reason) => {
            let label = non_value_label(&reason, c);
            let why = match reason.detail() {
                Some(detail) if !detail.is_empty() => format!("{label}: {detail}"),
                _ => label.to_string(),
            };
            TableCell::new("—").tone(CellTone::Secondary).tooltip(why)
        }
    }
}

/// A name cell, carrying its full text on hover when it is long enough to
/// be cut off in a narrow window.
pub(crate) fn named(name: String) -> TableCell {
    if name.chars().count() > 20 {
        TableCell::new(name.clone()).tooltip(name)
    } else {
        TableCell::new(name)
    }
}

/// Whether the process tables carry a GPU column: some program has a GPU
/// reading. A card whose driver does not report use per program (or a
/// card that is asleep) leaves every row empty, and a column of dashes would
/// only suggest that nothing is using it.
pub(crate) fn shows_gpu_column(processes: &[ProcessFacts]) -> bool {
    processes
        .iter()
        .any(|process| process.gpu_utilization.any_value().is_some())
}

/// The columns the Details table shows, in order. The command line is
/// private and only appears once the person has turned it on.
///
/// The GPU column only appears when [`shows_gpu_column`] says so.
pub(crate) fn detail_columns(show_command_line: bool, has_gpu: bool) -> Vec<ProcessColumn> {
    let mut columns = vec![
        ProcessColumn::Name,
        ProcessColumn::Pid,
        ProcessColumn::State,
        ProcessColumn::User,
        ProcessColumn::CpuUtilization,
        ProcessColumn::Memory,
        ProcessColumn::Disk,
        ProcessColumn::Network,
    ];
    if has_gpu {
        columns.push(ProcessColumn::Gpu);
    }
    if show_command_line {
        columns.push(ProcessColumn::CommandLine);
    }
    columns
}

/// A column's fixed width, or `None` for the columns that share what is left.
pub(crate) fn detail_width(column: ProcessColumn) -> Option<f32> {
    match column {
        ProcessColumn::Name | ProcessColumn::CommandLine => None,
        ProcessColumn::Pid => Some(80.0),
        ProcessColumn::State => Some(96.0),
        ProcessColumn::User => Some(88.0),
        ProcessColumn::CpuUtilization => Some(72.0),
        ProcessColumn::Memory => Some(88.0),
        ProcessColumn::ReadRate | ProcessColumn::WriteRate => Some(88.0),
        ProcessColumn::Disk => Some(88.0),
        ProcessColumn::Network => Some(96.0),
        ProcessColumn::Gpu => Some(72.0),
        _ => Some(110.0),
    }
}

/// Whether a column holds numbers, which read best aligned to the end.
pub(crate) fn numeric(column: ProcessColumn) -> bool {
    matches!(
        column,
        ProcessColumn::Pid
            | ProcessColumn::CpuUtilization
            | ProcessColumn::Memory
            | ProcessColumn::ReadRate
            | ProcessColumn::WriteRate
            | ProcessColumn::Disk
            | ProcessColumn::Network
            | ProcessColumn::Gpu
            | ProcessColumn::GpuMemory
            | ProcessColumn::Power
            | ProcessColumn::Swap
            | ProcessColumn::Threads
            | ProcessColumn::FileDescriptors
    )
}

pub(crate) fn column_title(column: ProcessColumn, c: &'static Copy) -> &'static str {
    match column {
        ProcessColumn::Name => c.column_name,
        ProcessColumn::Pid => c.column_pid,
        ProcessColumn::ParentPid => c.column_parent_pid,
        ProcessColumn::User => c.column_user,
        ProcessColumn::State => c.column_state,
        ProcessColumn::CpuUtilization => c.column_cpu,
        ProcessColumn::CpuTime => c.column_cpu_time,
        ProcessColumn::Memory => c.column_memory,
        ProcessColumn::Swap => c.column_swap,
        ProcessColumn::ReadRate => c.column_read,
        ProcessColumn::WriteRate => c.column_write,
        ProcessColumn::Disk => c.column_disk,
        ProcessColumn::Network => c.column_network,
        ProcessColumn::Gpu => c.column_gpu,
        ProcessColumn::GpuMemory => c.column_gpu_memory,
        ProcessColumn::Power => c.column_power,
        ProcessColumn::Threads => c.column_threads,
        ProcessColumn::FileDescriptors => c.column_descriptors,
        ProcessColumn::StartTime => c.column_running_for,
        ProcessColumn::Nice => c.column_nice,
        ProcessColumn::Cgroup => c.column_cgroup,
        ProcessColumn::CommandLine => c.column_command_line,
    }
}

/// One process column as a cell.
///
/// `processors` is the number of logical processors: a process's processor
/// time is shown as a share of the whole machine, as Task Manager shows it,
/// so the column adds up to the Processor page rather than to 2,400%.
pub(crate) fn process_cell(
    process: &ProcessFacts,
    column: ProcessColumn,
    locale: Locale,
    processors: Option<f64>,
) -> TableCell {
    let c = crate::i18n::copy(locale);
    match column {
        ProcessColumn::Name => named(process.display_name()),
        ProcessColumn::Pid => TableCell::new(process.pid.to_string()),
        ProcessColumn::ParentPid => reading(cell(&process.parent_pid, |v| v.to_string()), c),
        ProcessColumn::User => reading(cell(&process.user, |v| v.clone()), c),
        ProcessColumn::State => match process.state.any_value() {
            Some(raw) => {
                let (label, explanation) = process_state(raw, locale);
                TableCell::new(label).tooltip(explanation)
            }
            None => reading(cell(&process.state, |v| v.clone()), c),
        },
        ProcessColumn::CpuUtilization => match processors {
            Some(count) => reading(cell(&process.cpu_utilization, |v| share(*v / count)), c),
            None => counting_processors(c),
        },
        ProcessColumn::CpuTime => reading(
            cell(&process.cpu_time_total, |v| {
                words::duration(locale.words(), *v as u64)
            }),
            c,
        ),
        ProcessColumn::Memory => reading(cell(&process.memory_resident, |v| bytes(*v as f64)), c),
        ProcessColumn::Swap => reading(cell(&process.memory_swap, |v| bytes(*v as f64)), c),
        ProcessColumn::ReadRate => reading(cell(&process.read_rate, |v| rate(*v)), c),
        ProcessColumn::WriteRate => reading(cell(&process.write_rate, |v| rate(*v)), c),
        ProcessColumn::Disk => match process.disk_rate() {
            Some(value) => TableCell::new(rate(value)),
            None => reading(cell(&process.read_rate, |v| rate(*v)), c),
        },
        ProcessColumn::Network => match process.network_rate() {
            Some(value) => TableCell::new(rate(value)),
            None => reading(cell(&process.network_receive_rate, |v| rate(*v)), c),
        },
        ProcessColumn::Gpu => reading(cell(&process.gpu_utilization, |v| share(*v)), c),
        ProcessColumn::GpuMemory => reading(cell(&process.gpu_memory, |v| bytes(*v as f64)), c),
        ProcessColumn::Power => reading(cell(&process.energy_power, |v| watts(*v)), c),
        ProcessColumn::Threads => reading(cell(&process.threads, |v| v.to_string()), c),
        ProcessColumn::FileDescriptors => {
            reading(cell(&process.file_descriptors, |v| v.to_string()), c)
        }
        ProcessColumn::StartTime => reading(
            cell(&process.runtime, |v| {
                words::duration(locale.words(), *v as u64)
            }),
            c,
        ),
        ProcessColumn::Nice => reading(cell(&process.nice, |v| v.to_string()), c),
        ProcessColumn::Cgroup => reading(cell(&process.cgroup, |v| v.clone()), c),
        ProcessColumn::CommandLine => reading(cell(&process.command_line, |v| v.clone()), c),
    }
}

/// Which column the Processes page is sorted by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppColumn {
    Name,
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
}

impl AppColumn {
    pub(crate) const ALL: [AppColumn; 6] = [
        AppColumn::Name,
        AppColumn::Cpu,
        AppColumn::Memory,
        AppColumn::Disk,
        AppColumn::Network,
        AppColumn::Gpu,
    ];
}

/// What is selected on the Processes page: an application, or one of its
/// processes. A group is named by its key, which survives a refresh.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AppSelection {
    Group(String),
    Process(u32),
}

/// One line of the Processes table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppLine {
    /// A heading: apps, then background processes.
    Section { services: bool, count: usize },
    /// An application, by its index in the model's list.
    Group { services: bool, index: usize },
    /// One of an expanded application's processes, by its index in the
    /// model's process list.
    Member { process: usize },
}

fn disk(row: &AppRow) -> Aggregate {
    Aggregate {
        total: row.read_rate.total + row.write_rate.total,
        counted: row.read_rate.counted.min(row.write_rate.counted),
        missing: row.read_rate.missing.max(row.write_rate.missing),
    }
}

fn network(row: &AppRow) -> Aggregate {
    let (receive, send) = (&row.network_receive_rate, &row.network_send_rate);
    Aggregate {
        total: receive.total + send.total,
        counted: receive.counted.min(send.counted),
        missing: receive.missing.max(send.missing),
    }
}

/// Watts, the way a person reads a small power: `1.2 W`, `350 mW`.
pub(crate) fn watts(value: f64) -> String {
    if value.abs() < 1.0 {
        format!("{:.0} mW", value * 1000.0)
    } else {
        format!("{value:.1} W")
    }
}

/// Order two aggregates, with "nothing measured" last in either direction.
fn by_total(a: &Aggregate, b: &Aggregate, ascending: bool) -> Ordering {
    match (a.is_unavailable(), b.is_unavailable()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let order = a.total.partial_cmp(&b.total).unwrap_or(Ordering::Equal);
            if ascending { order } else { order.reverse() }
        }
    }
}

/// The order of one section's applications under a sort.
pub(crate) fn app_order(rows: &[AppRow], column: AppColumn, ascending: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&x, &y| {
        let (a, b) = (&rows[x], &rows[y]);
        match column {
            AppColumn::Name => {
                let order = a
                    .group
                    .display_name
                    .to_lowercase()
                    .cmp(&b.group.display_name.to_lowercase());
                if ascending { order } else { order.reverse() }
            }
            AppColumn::Cpu => by_total(&a.cpu_utilization, &b.cpu_utilization, ascending),
            AppColumn::Memory => by_total(&a.memory_resident, &b.memory_resident, ascending),
            AppColumn::Disk => by_total(&disk(a), &disk(b), ascending),
            AppColumn::Network => by_total(&network(a), &network(b), ascending),
            AppColumn::Gpu => by_total(&a.gpu_utilization, &b.gpu_utilization, ascending),
        }
        .then_with(|| a.group.key.cmp(&b.group.key))
    });
    order
}

/// Every line of the Processes table: a heading, then each application in
/// order, with an expanded one's processes under it.
pub(crate) fn app_lines(model: &AppsModel, column: AppColumn, ascending: bool) -> Vec<AppLine> {
    let mut lines = Vec::new();
    for services in [false, true] {
        let rows = if services {
            model.services()
        } else {
            model.applications()
        };
        if rows.is_empty() {
            continue;
        }
        lines.push(AppLine::Section {
            services,
            count: rows.len(),
        });
        for index in app_order(rows, column, ascending) {
            lines.push(AppLine::Group { services, index });
            if rows[index].process_count() > 1 && model.is_expanded(&rows[index].group.key) {
                lines.extend(
                    rows[index]
                        .member_indices
                        .iter()
                        .map(|process| AppLine::Member { process: *process }),
                );
            }
        }
    }
    lines
}

pub(crate) fn app_row(model: &AppsModel, services: bool, index: usize) -> Option<&AppRow> {
    if services {
        model.services().get(index)
    } else {
        model.applications().get(index)
    }
}

/// An aggregate as a cell: a complete total, a floor marked as one when some
/// processes did not report, or the fact that nothing was measured.
pub(crate) fn aggregate(
    value: &Aggregate,
    c: &'static Copy,
    render: impl Fn(f64) -> String,
) -> TableCell {
    if value.is_unavailable() {
        return reading(Cell::Missing(NonValue::NotCollected), c);
    }
    let text = render(value.total);
    if value.is_partial() {
        TableCell::new(format!("≥ {text}")).tooltip(format!(
            "{} {}/{}",
            c.partial_total,
            value.counted,
            value.counted + value.missing
        ))
    } else {
        TableCell::new(text)
    }
}

/// A share of the whole machine cannot be worked out before the processor
/// count is known; until then the cell says it is still measuring.
pub(crate) fn counting_processors(c: &'static Copy) -> TableCell {
    TableCell::new("—")
        .tone(CellTone::Secondary)
        .tooltip(c.not_yet_sampled)
}

/// An application's processor time as a share of every logical processor,
/// Task Manager's rule: 100% is the whole machine.
pub(crate) fn cpu_aggregate(
    value: &Aggregate,
    c: &'static Copy,
    processors: Option<f64>,
) -> TableCell {
    match processors {
        Some(count) => aggregate(value, c, |v| share(v / count)),
        None => counting_processors(c),
    }
}

/// The cells of one Processes line, in the order of [`AppColumn::ALL`].
pub(crate) fn app_cells(
    model: &AppsModel,
    line: AppLine,
    locale: Locale,
    processors: Option<f64>,
) -> Vec<TableCell> {
    let c = crate::i18n::copy(locale);
    match line {
        AppLine::Section { services, count } => vec![
            TableCell::new(format!(
                "{} ({count})",
                if services {
                    c.background_processes
                } else {
                    c.apps_section
                }
            ))
            .strong()
            .tone(CellTone::Secondary),
            TableCell::default(),
            TableCell::default(),
            TableCell::default(),
            TableCell::default(),
            TableCell::default(),
        ],
        AppLine::Group { services, index } => {
            let Some(row) = app_row(model, services, index) else {
                return vec![TableCell::default(); AppColumn::ALL.len()];
            };
            let count = row.process_count();
            let mut name = if count > 1 {
                TableCell::new(format!("{} ({count})", row.group.display_name))
                    .expander(model.is_expanded(&row.group.key))
            } else {
                TableCell::new(row.group.display_name.clone()).indent(1)
            };
            name = name.tooltip(format!(
                "{}\n{}",
                row.group.display_name,
                grouping_sentence(&row.group.evidence, row.group.confidence(), c)
            ));
            vec![
                name,
                cpu_aggregate(&row.cpu_utilization, c, processors),
                aggregate(&row.memory_resident, c, bytes),
                aggregate(&disk(row), c, rate),
                aggregate(&network(row), c, rate),
                gpu_aggregate(&row.gpu_utilization, c),
            ]
        }
        AppLine::Member { process } => {
            let Some(member) = model.process(process) else {
                return vec![TableCell::default(); AppColumn::ALL.len()];
            };
            let disk = match (member.read_rate.value(), member.write_rate.value()) {
                (Some(read), Some(write)) => TableCell::new(rate(read + write)),
                _ => reading(cell(&member.read_rate, |v| rate(*v)), c),
            };
            vec![
                named(member.display_name())
                    .indent(2)
                    .tone(CellTone::Primary),
                process_cell(member, ProcessColumn::CpuUtilization, locale, processors),
                process_cell(member, ProcessColumn::Memory, locale, processors),
                disk,
                process_cell(member, ProcessColumn::Network, locale, processors),
                process_cell(member, ProcessColumn::Gpu, locale, processors),
            ]
        }
    }
}

/// An application's graphics share: the sum of its processes' shares of the
/// busiest engine, capped at the whole card.
pub(crate) fn gpu_aggregate(value: &Aggregate, c: &'static Copy) -> TableCell {
    aggregate(value, c, |v| share(v.min(1.0)))
}

/// The sentence that says why processes were put together.
pub(crate) fn grouping_sentence(
    evidence: &GroupingEvidence,
    confidence: Confidence,
    c: &'static Copy,
) -> String {
    let because = match evidence {
        GroupingEvidence::SystemdUnit { .. } => c.evidence_systemd_unit,
        GroupingEvidence::Flatpak { .. } => c.evidence_flatpak,
        GroupingEvidence::Snap { .. } => c.evidence_snap,
        GroupingEvidence::DesktopApplication { .. } => c.evidence_desktop,
        GroupingEvidence::Ancestry { .. } => c.evidence_ancestry,
        GroupingEvidence::ExecutableIdentity { .. } => c.evidence_executable,
        GroupingEvidence::Unattributed { .. } => c.evidence_unattributed,
    };
    let sure = match confidence {
        Confidence::High => c.confidence_high,
        Confidence::Medium => c.confidence_medium,
        Confidence::Low => c.confidence_low,
    };
    format!("{because} {sure}")
}

/// Whether ending is offered: only with something selected, and never while
/// the list is paused, because a frozen list can name a process that has
/// closed and whose PID now belongs to another.
pub(crate) fn end_offered(list_paused: bool, has_target: bool) -> bool {
    has_target && !list_paused
}

/// A process and everything under it, children before their parents, so a
/// tree is ended from the leaves up and nothing is re-parented to the init
/// process halfway through.
pub(crate) fn tree_of(pid: u32, processes: &[ProcessFacts]) -> Vec<u32> {
    let mut order = Vec::new();
    let mut stack = vec![(pid, false)];
    let mut seen = std::collections::HashSet::new();
    while let Some((current, children_done)) = stack.pop() {
        if children_done {
            order.push(current);
            continue;
        }
        if !seen.insert(current) {
            continue;
        }
        stack.push((current, true));
        for child in processes
            .iter()
            .filter(|process| process.parent() == Some(current) && process.pid != current)
        {
            stack.push((child.pid, false));
        }
    }
    order
}
