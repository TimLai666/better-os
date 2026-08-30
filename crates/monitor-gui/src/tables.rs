//! The two virtualized tables.
//!
//! Both are `gpui-component` `TableState` delegates, which render only the
//! rows the viewport can show. The models behind them decide order, filtering,
//! and tree structure; these types only turn a row into elements.
//!
//! Every cell goes through [`cell_element`], which is the single place a
//! reading becomes something visible. A reading with no value renders as the
//! reason it has none, in the muted style, and never as `0`.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme,
    table::{Column, ColumnSort, TableDelegate, TableState},
    *,
};
use monitor_views::apps::{Aggregate, AppRow};
use monitor_views::format::{
    Cell, NonValue, bytes, bytes_per_second, cell, count, duration, ratio_percent,
};
use monitor_views::grouping::{Confidence, GroupingEvidence};
use monitor_views::{AppsModel, ProcessColumn, ProcessFacts, ProcessTableModel, SortDirection};

use crate::i18n::{Copy, Locale, copy};

/// Column widths, owned here so the layout tests and the renderer cannot
/// disagree about how much room a header has.
pub(crate) struct ProcessColumnLayout;

impl ProcessColumnLayout {
    pub(crate) fn width_of(column: ProcessColumn) -> f32 {
        match column {
            ProcessColumn::Name => 240.0,
            ProcessColumn::Pid => 88.0,
            ProcessColumn::ParentPid => 96.0,
            ProcessColumn::User => 130.0,
            ProcessColumn::State => 150.0,
            ProcessColumn::CpuUtilization => 130.0,
            ProcessColumn::CpuTime => 130.0,
            ProcessColumn::Memory => 130.0,
            ProcessColumn::Swap => 120.0,
            ProcessColumn::ReadRate => 120.0,
            ProcessColumn::WriteRate => 120.0,
            ProcessColumn::Threads => 110.0,
            ProcessColumn::FileDescriptors => 110.0,
            ProcessColumn::StartTime => 130.0,
            ProcessColumn::Nice => 100.0,
            ProcessColumn::Cgroup => 260.0,
            ProcessColumn::CommandLine => 320.0,
        }
    }

    pub(crate) fn header_of(column: ProcessColumn, c: &'static Copy) -> &'static str {
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
            ProcessColumn::Threads => c.column_threads,
            ProcessColumn::FileDescriptors => c.column_descriptors,
            ProcessColumn::StartTime => c.column_start_time,
            ProcessColumn::Nice => c.column_nice,
            ProcessColumn::Cgroup => c.column_cgroup,
            ProcessColumn::CommandLine => c.column_command_line,
        }
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

/// Draw one reading.
///
/// A measured value is plain text. A stale one carries its age. A missing one
/// is the reason, in muted italics, with the detail on hover — visually and
/// semantically distinct from a zero, which is what the specification asks
/// for.
pub(crate) fn cell_element(rendered: Cell, c: &'static Copy, cx: &App) -> AnyElement {
    match rendered {
        Cell::Value(text) => div().truncate().child(text).into_any_element(),
        Cell::Stale { text, age_seconds } => div()
            .truncate()
            .text_color(cx.theme().warning_foreground)
            .child(format!("{text} · {age_seconds}s {}", c.stale_suffix))
            .into_any_element(),
        Cell::Missing(reason) => {
            let label = non_value_label(&reason, c);
            div()
                .truncate()
                .italic()
                .text_color(cx.theme().muted_foreground)
                .child(format!("— {label}"))
                .into_any_element()
        }
    }
}

/// Render one process column.
pub(crate) fn process_cell(
    process: &ProcessFacts,
    column: ProcessColumn,
    c: &'static Copy,
    cx: &App,
) -> AnyElement {
    match column {
        ProcessColumn::Name => cell_element(Cell::Value(process.display_name()), c, cx),
        ProcessColumn::Pid => cell_element(Cell::Value(process.pid.to_string()), c, cx),
        ProcessColumn::ParentPid => {
            cell_element(cell(&process.parent_pid, |value| count(*value)), c, cx)
        }
        ProcessColumn::User => cell_element(cell(&process.user, |value| value.clone()), c, cx),
        ProcessColumn::State => cell_element(cell(&process.state, |value| value.clone()), c, cx),
        ProcessColumn::CpuUtilization => cell_element(
            cell(&process.cpu_utilization, |value| ratio_percent(*value)),
            c,
            cx,
        ),
        ProcessColumn::CpuTime => cell_element(
            cell(&process.cpu_time_total, |value| duration(*value)),
            c,
            cx,
        ),
        ProcessColumn::Memory => cell_element(
            cell(&process.memory_resident, |value| bytes(*value as f64)),
            c,
            cx,
        ),
        ProcessColumn::Swap => cell_element(
            cell(&process.memory_swap, |value| bytes(*value as f64)),
            c,
            cx,
        ),
        ProcessColumn::ReadRate => cell_element(
            cell(&process.read_rate, |value| bytes_per_second(*value)),
            c,
            cx,
        ),
        ProcessColumn::WriteRate => cell_element(
            cell(&process.write_rate, |value| bytes_per_second(*value)),
            c,
            cx,
        ),
        ProcessColumn::Threads => {
            cell_element(cell(&process.threads, |value| count(*value)), c, cx)
        }
        ProcessColumn::FileDescriptors => cell_element(
            cell(&process.file_descriptors, |value| count(*value)),
            c,
            cx,
        ),
        ProcessColumn::StartTime => {
            cell_element(cell(&process.runtime, |value| duration(*value)), c, cx)
        }
        ProcessColumn::Nice => cell_element(cell(&process.nice, |value| value.to_string()), c, cx),
        ProcessColumn::Cgroup => cell_element(cell(&process.cgroup, |value| value.clone()), c, cx),
        ProcessColumn::CommandLine => {
            cell_element(cell(&process.command_line, |value| value.clone()), c, cx)
        }
    }
}

/// The Processes table.
pub(crate) struct ProcessTableDelegate {
    pub(crate) model: ProcessTableModel,
    pub(crate) locale: Locale,
    columns: Vec<Column>,
    visible_columns: Vec<ProcessColumn>,
}

impl ProcessTableDelegate {
    pub(crate) fn new(locale: Locale) -> Self {
        let mut delegate = Self {
            model: ProcessTableModel::new(Vec::new()),
            locale,
            columns: Vec::new(),
            visible_columns: Vec::new(),
        };
        delegate.rebuild_columns();
        delegate
    }

    /// Rebuild the column definitions, which changes when the language or the
    /// command-line privacy setting changes.
    pub(crate) fn rebuild_columns(&mut self) {
        let c = copy(self.locale);
        let (sorted, direction) = self.model.sort();
        self.visible_columns = self.model.columns();
        self.columns = self
            .visible_columns
            .iter()
            .map(|column| {
                let width = ProcessColumnLayout::width_of(*column);
                let definition =
                    Column::new(column.key(), ProcessColumnLayout::header_of(*column, c))
                        .width(width)
                        .min_width(72.0)
                        .sortable();
                if *column == sorted {
                    definition.sort(match direction {
                        SortDirection::Ascending => ColumnSort::Ascending,
                        SortDirection::Descending => ColumnSort::Descending,
                    })
                } else {
                    definition
                }
            })
            .collect();
    }

    pub(crate) fn process_at(&self, row: usize) -> Option<&ProcessFacts> {
        self.model.process_at(row)
    }
}

impl TableDelegate for ProcessTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.model.visible_rows().len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        let Some(column) = self.visible_columns.get(col_ix).copied() else {
            return;
        };
        let direction = match sort {
            ColumnSort::Ascending => SortDirection::Ascending,
            _ => SortDirection::Descending,
        };
        self.model.set_sort(column, direction);
        self.rebuild_columns();
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let c = copy(self.locale);
        let Some(row) = self.model.visible_rows().get(row_ix).copied() else {
            return div().into_any_element();
        };
        let Some(process) = self.model.row(row.index) else {
            return div().into_any_element();
        };
        let Some(column) = self.visible_columns.get(col_ix).copied() else {
            return div().into_any_element();
        };
        // Tree depth is drawn as indentation on the name column only, so the
        // numeric columns stay aligned and comparable down the table.
        let indent = if column == ProcessColumn::Name {
            row.depth as f32 * 14.0
        } else {
            0.0
        };
        h_flex()
            .min_w_0()
            .w_full()
            .items_center()
            .gap_1()
            .pl(px(indent))
            .when(
                column == ProcessColumn::Name && row.has_children,
                |element| {
                    element.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("▾"),
                    )
                },
            )
            .child(process_cell(process, column, c, cx))
            .into_any_element()
    }
}

/// One line of the Apps table: either an application, or one of its processes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppsRow {
    /// A section heading: applications, then background services.
    Section { services: bool },
    /// Index into the applications or services list.
    Group { services: bool, index: usize },
    /// A member process, by its index in the model's process list.
    Member { process: usize },
}

/// The Apps table, flattened so the same virtualized table can draw group rows
/// and the member rows of an expanded group.
pub(crate) struct AppsTableDelegate {
    pub(crate) model: AppsModel,
    pub(crate) locale: Locale,
    rows: Vec<AppsRow>,
    columns: Vec<Column>,
}

impl AppsTableDelegate {
    pub(crate) fn new(locale: Locale, model: AppsModel) -> Self {
        let mut delegate = Self {
            model,
            locale,
            rows: Vec::new(),
            columns: Vec::new(),
        };
        delegate.rebuild();
        delegate
    }

    pub(crate) fn rebuild(&mut self) {
        let c = copy(self.locale);
        self.columns = vec![
            Column::new("name", c.applications)
                .width(280.0)
                .min_width(140.0),
            Column::new("count", c.process_count).width(110.0),
            Column::new("cpu", c.column_cpu).width(130.0),
            Column::new("memory", c.column_memory).width(140.0),
            Column::new("read", c.column_read).width(130.0),
            Column::new("write", c.column_write).width(130.0),
            Column::new("evidence", c.grouped_because).width(320.0),
        ];

        let mut rows = Vec::new();
        for services in [false, true] {
            let groups = if services {
                self.model.services()
            } else {
                self.model.applications()
            };
            if groups.is_empty() {
                continue;
            }
            rows.push(AppsRow::Section { services });
            for (index, row) in groups.iter().enumerate() {
                rows.push(AppsRow::Group { services, index });
                if self.model.is_expanded(&row.group.key) {
                    rows.extend(
                        row.member_indices
                            .iter()
                            .map(|process| AppsRow::Member { process: *process }),
                    );
                }
            }
        }
        self.rows = rows;
    }

    pub(crate) fn row_at(&self, row: usize) -> Option<AppsRow> {
        self.rows.get(row).copied()
    }

    pub(crate) fn app_row(&self, services: bool, index: usize) -> Option<&AppRow> {
        if services {
            self.model.services().get(index)
        } else {
            self.model.applications().get(index)
        }
    }
}

/// An aggregate as a cell: a complete total, an explicit floor when some
/// members did not report, or the fact that nothing was measured at all.
pub(crate) fn aggregate_cell(
    value: &Aggregate,
    c: &'static Copy,
    render: impl Fn(f64) -> String,
    cx: &App,
) -> AnyElement {
    if value.is_unavailable() {
        return cell_element(Cell::Missing(NonValue::NotCollected), c, cx);
    }
    let text = render(value.total);
    if value.is_partial() {
        // A floor, not a total: some members did not report, and the row says
        // how many rather than presenting the partial sum as the answer.
        div()
            .truncate()
            .text_color(cx.theme().warning_foreground)
            .child(format!(
                "{} {text} ({}/{})",
                c.partial_total,
                value.counted,
                value.counted + value.missing
            ))
            .into_any_element()
    } else {
        div().truncate().child(text).into_any_element()
    }
}

/// The sentence that explains a group.
pub(crate) fn evidence_label(evidence: &GroupingEvidence, c: &'static Copy) -> &'static str {
    match evidence {
        GroupingEvidence::SystemdUnit { .. } => c.evidence_systemd_unit,
        GroupingEvidence::Flatpak { .. } => c.evidence_flatpak,
        GroupingEvidence::Snap { .. } => c.evidence_snap,
        GroupingEvidence::DesktopApplication { .. } => c.evidence_desktop,
        GroupingEvidence::Ancestry { .. } => c.evidence_ancestry,
        GroupingEvidence::ExecutableIdentity { .. } => c.evidence_executable,
        GroupingEvidence::Unattributed { .. } => c.evidence_unattributed,
    }
}

pub(crate) fn confidence_label(confidence: Confidence, c: &'static Copy) -> &'static str {
    match confidence {
        Confidence::High => c.confidence_high,
        Confidence::Medium => c.confidence_medium,
        Confidence::Low => c.confidence_low,
    }
}

impl TableDelegate for AppsTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let c = copy(self.locale);
        let Some(row) = self.rows.get(row_ix).copied() else {
            return div().into_any_element();
        };
        match row {
            AppsRow::Section { services } => {
                if col_ix != 0 {
                    return div().into_any_element();
                }
                div()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(if services {
                        c.background_services
                    } else {
                        c.applications
                    })
                    .into_any_element()
            }
            AppsRow::Group { services, index } => {
                let Some(app) = self.app_row(services, index) else {
                    return div().into_any_element();
                };
                let expanded = self.model.is_expanded(&app.group.key);
                match col_ix {
                    0 => h_flex()
                        .min_w_0()
                        .gap_1()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if expanded { "▾" } else { "▸" }),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_semibold()
                                .child(app.group.display_name.clone()),
                        )
                        .into_any_element(),
                    1 => div()
                        .child(format!("{} {}", app.process_count(), c.process_count))
                        .into_any_element(),
                    2 => aggregate_cell(&app.cpu_utilization, c, ratio_percent, cx),
                    3 => aggregate_cell(&app.memory_resident, c, bytes, cx),
                    4 => aggregate_cell(&app.read_rate, c, bytes_per_second, cx),
                    5 => aggregate_cell(&app.write_rate, c, bytes_per_second, cx),
                    6 => v_flex()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .child(evidence_label(&app.group.evidence, c)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(confidence_label(app.group.confidence(), c)),
                        )
                        .into_any_element(),
                    _ => div().into_any_element(),
                }
            }
            AppsRow::Member { process } => {
                let Some(member) = self.model.process(process) else {
                    return div().into_any_element();
                };
                match col_ix {
                    0 => div()
                        .pl(px(28.0))
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{} · {}", member.display_name(), member.pid))
                        .into_any_element(),
                    2 => process_cell(member, ProcessColumn::CpuUtilization, c, cx),
                    3 => process_cell(member, ProcessColumn::Memory, c, cx),
                    4 => process_cell(member, ProcessColumn::ReadRate, c, cx),
                    5 => process_cell(member, ProcessColumn::WriteRate, c, cx),
                    6 => {
                        let evidence = self
                            .model
                            .grouping()
                            .group_of(member.pid)
                            .and_then(|group| {
                                group
                                    .members
                                    .iter()
                                    .find(|entry| entry.pid == member.pid)
                                    .map(|entry| evidence_label(&entry.evidence, c))
                            })
                            .unwrap_or(c.evidence_unattributed);
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(evidence)
                            .into_any_element()
                    }
                    _ => div().into_any_element(),
                }
            }
        }
    }
}
