use std::cmp::Ordering;

use gpui::*;
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    dialog::{
        AlertDialog, DialogAction, DialogClose, DialogDescription, DialogFooter, DialogHeader,
        DialogTitle,
    },
    h_flex,
    table::{Column, ColumnSort, TableDelegate, TableState},
};
use sysinfo::Signal;

use crate::{
    app::MonitorWindow,
    linux::{self, AppGroup},
    settings::MonitorSettings,
    sort_preferences,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppColumn {
    Name,
    Memory,
    Cpu,
    ReadSpeed,
    ReadTotal,
    WriteSpeed,
    WriteTotal,
    Gpu,
    GpuMemory,
    Encoder,
    Decoder,
    Swap,
    CombinedMemory,
    Actions,
}

impl AppColumn {
    const fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Memory => "memory",
            Self::Cpu => "cpu",
            Self::ReadSpeed => "read-speed",
            Self::ReadTotal => "read-total",
            Self::WriteSpeed => "write-speed",
            Self::WriteTotal => "write-total",
            Self::Gpu => "gpu",
            Self::GpuMemory => "gpu-memory",
            Self::Encoder => "encoder",
            Self::Decoder => "decoder",
            Self::Swap => "swap",
            Self::CombinedMemory => "combined-memory",
            Self::Actions => "actions",
        }
    }

    fn from_id(value: &str) -> Option<Self> {
        Some(match value {
            "name" => Self::Name,
            "memory" => Self::Memory,
            "cpu" => Self::Cpu,
            "read-speed" => Self::ReadSpeed,
            "read-total" => Self::ReadTotal,
            "write-speed" => Self::WriteSpeed,
            "write-total" => Self::WriteTotal,
            "gpu" => Self::Gpu,
            "gpu-memory" => Self::GpuMemory,
            "encoder" => Self::Encoder,
            "decoder" => Self::Decoder,
            "swap" => Self::Swap,
            "combined-memory" => Self::CombinedMemory,
            "actions" => Self::Actions,
            _ => return None,
        })
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Name => "App",
            Self::Memory => "Memory",
            Self::Cpu => "CPU %",
            Self::ReadSpeed => "Read/s",
            Self::ReadTotal => "Read total",
            Self::WriteSpeed => "Write/s",
            Self::WriteTotal => "Write total",
            Self::Gpu => "GPU %",
            Self::GpuMemory => "GPU memory",
            Self::Encoder => "Encoder %",
            Self::Decoder => "Decoder %",
            Self::Swap => "Swap",
            Self::CombinedMemory => "Memory + swap",
            Self::Actions => "Actions",
        }
    }

    const fn width(self) -> f32 {
        match self {
            Self::Name => 280.0,
            Self::Memory | Self::Swap => 112.0,
            Self::Cpu | Self::Gpu => 86.0,
            Self::ReadSpeed | Self::WriteSpeed => 106.0,
            Self::ReadTotal | Self::WriteTotal | Self::GpuMemory => 118.0,
            Self::Encoder | Self::Decoder => 98.0,
            Self::CombinedMemory => 132.0,
            Self::Actions => 410.0,
        }
    }

    const fn sortable(self) -> bool {
        !matches!(
            self,
            Self::Gpu | Self::GpuMemory | Self::Encoder | Self::Decoder | Self::Actions
        )
    }
}

pub struct AppTableDelegate {
    all_groups: Vec<AppGroup>,
    pub groups: Vec<AppGroup>,
    columns: Vec<Column>,
    column_kinds: Vec<AppColumn>,
    sort_column: AppColumn,
    sort_order: ColumnSort,
    query: String,
    settings: MonitorSettings,
    monitor: WeakEntity<MonitorWindow>,
}

impl AppTableDelegate {
    pub fn new(settings: &MonitorSettings, monitor: WeakEntity<MonitorWindow>) -> Self {
        let preference = sort_preferences::load_app();
        let mut this = Self {
            all_groups: Vec::new(),
            groups: Vec::new(),
            columns: Vec::new(),
            column_kinds: Vec::new(),
            sort_column: AppColumn::from_id(&preference.column).unwrap_or(AppColumn::Cpu),
            sort_order: if preference.descending {
                ColumnSort::Descending
            } else {
                ColumnSort::Ascending
            },
            query: String::new(),
            settings: settings.clone(),
            monitor,
        };
        this.rebuild_columns();
        this
    }

    pub fn set_settings(&mut self, settings: &MonitorSettings) {
        self.settings = settings.clone();
        self.rebuild_columns();
        self.refresh_rows();
    }

    pub fn set_groups(&mut self, groups: Vec<AppGroup>) {
        self.all_groups = groups;
        self.refresh_rows();
    }

    pub fn set_filter(&mut self, query: impl Into<String>) {
        self.query = query.into().trim().to_lowercase();
        self.refresh_rows();
    }

    fn rebuild_columns(&mut self) {
        let columns = &self.settings.app_columns;
        let mut kinds = vec![AppColumn::Name];
        if columns.memory {
            kinds.push(AppColumn::Memory);
        }
        if columns.cpu {
            kinds.push(AppColumn::Cpu);
        }
        if columns.read_speed {
            kinds.push(AppColumn::ReadSpeed);
        }
        if columns.read_total {
            kinds.push(AppColumn::ReadTotal);
        }
        if columns.write_speed {
            kinds.push(AppColumn::WriteSpeed);
        }
        if columns.write_total {
            kinds.push(AppColumn::WriteTotal);
        }
        if columns.gpu {
            kinds.push(AppColumn::Gpu);
        }
        if columns.gpu_memory {
            kinds.push(AppColumn::GpuMemory);
        }
        if columns.encoder {
            kinds.push(AppColumn::Encoder);
        }
        if columns.decoder {
            kinds.push(AppColumn::Decoder);
        }
        if columns.swap {
            kinds.push(AppColumn::Swap);
        }
        if columns.combined_memory {
            kinds.push(AppColumn::CombinedMemory);
        }
        kinds.push(AppColumn::Actions);

        self.columns = kinds
            .iter()
            .map(|kind| {
                let column = Column::new(kind.id(), kind.title()).width(kind.width());
                let column = if kind.sortable() {
                    column.sortable()
                } else {
                    column
                };
                if *kind == self.sort_column {
                    column.sort(self.sort_order)
                } else {
                    column
                }
            })
            .collect();
        self.column_kinds = kinds;
    }

    fn refresh_rows(&mut self) {
        if self.query.is_empty() {
            self.groups.clone_from(&self.all_groups);
        } else {
            let terms = self
                .query
                .split('|')
                .map(str::trim)
                .filter(|term| !term.is_empty())
                .collect::<Vec<_>>();
            self.groups = self
                .all_groups
                .iter()
                .filter(|group| {
                    let haystack = format!(
                        "{} {} {} {}",
                        group.display_name, group.id, group.grouping_reason, group.process_count
                    )
                    .to_lowercase();
                    terms.iter().all(|term| haystack.contains(term))
                })
                .cloned()
                .collect();
        }
        self.sort_groups();
    }

    fn sort_groups(&mut self) {
        let descending = matches!(self.sort_order, ColumnSort::Descending);
        let column = self.sort_column;
        self.groups.sort_by(|a, b| {
            let ordering = match column {
                AppColumn::Name => a
                    .display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase()),
                AppColumn::Memory => a.memory.cmp(&b.memory),
                AppColumn::Cpu => a
                    .cpu_usage
                    .partial_cmp(&b.cpu_usage)
                    .unwrap_or(Ordering::Equal),
                AppColumn::ReadSpeed => a.read_speed.cmp(&b.read_speed),
                AppColumn::ReadTotal => a.read_total.cmp(&b.read_total),
                AppColumn::WriteSpeed => a.write_speed.cmp(&b.write_speed),
                AppColumn::WriteTotal => a.write_total.cmp(&b.write_total),
                AppColumn::Swap => a.swap.cmp(&b.swap),
                AppColumn::CombinedMemory => a
                    .memory
                    .saturating_add(a.swap)
                    .cmp(&b.memory.saturating_add(b.swap)),
                AppColumn::Gpu
                | AppColumn::GpuMemory
                | AppColumn::Encoder
                | AppColumn::Decoder
                | AppColumn::Actions => Ordering::Equal,
            };
            let ordering = if descending {
                ordering.reverse()
            } else {
                ordering
            };
            ordering.then_with(|| a.display_name.cmp(&b.display_name))
        });
    }

    fn cell_value(&self, group: &AppGroup, column: AppColumn) -> String {
        match column {
            AppColumn::Name => {
                format!("{} · {} processes", group.display_name, group.process_count)
            }
            AppColumn::Memory => linux::format_bytes(group.memory, self.settings.unit_base),
            AppColumn::Cpu => format!("{:.1}%", group.cpu_usage),
            AppColumn::ReadSpeed => {
                linux::format_rate(group.read_speed, false, self.settings.unit_base)
            }
            AppColumn::ReadTotal => linux::format_bytes(group.read_total, self.settings.unit_base),
            AppColumn::WriteSpeed => {
                linux::format_rate(group.write_speed, false, self.settings.unit_base)
            }
            AppColumn::WriteTotal => {
                linux::format_bytes(group.write_total, self.settings.unit_base)
            }
            AppColumn::Gpu | AppColumn::GpuMemory | AppColumn::Encoder | AppColumn::Decoder => {
                "N/A".to_string()
            }
            AppColumn::Swap => linux::format_bytes(group.swap, self.settings.unit_base),
            AppColumn::CombinedMemory => linux::format_bytes(
                group.memory.saturating_add(group.swap),
                self.settings.unit_base,
            ),
            AppColumn::Actions => "Application actions".to_string(),
        }
    }

    fn render_actions(
        &self,
        row_ix: usize,
        group: AppGroup,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        let term_target = self.monitor.clone();
        let kill_target = self.monitor.clone();
        let stop_target = self.monitor.clone();
        let continue_target = self.monitor.clone();
        let term_pids = group.pids.clone();
        let kill_pids = group.pids.clone();
        let stop_pids = group.pids.clone();
        let continue_pids = group.pids.clone();
        let app_name = group.display_name.clone();
        let info_name = group.display_name.clone();
        let info_description = format!(
            "Identity: {}\nGrouped processes: {}\nGrouping evidence: {}\nPIDs: {}\nCPU: {:.1}%\nMemory: {}\nSwap: {}\nRead: {} total, {}/s\nWritten: {} total, {}/s",
            group.id,
            group.process_count,
            group.grouping_reason,
            group
                .pids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            group.cpu_usage,
            linux::format_bytes(group.memory, self.settings.unit_base),
            linux::format_bytes(group.swap, self.settings.unit_base),
            linux::format_bytes(group.read_total, self.settings.unit_base),
            linux::format_bytes(group.read_speed, self.settings.unit_base),
            linux::format_bytes(group.write_total, self.settings.unit_base),
            linux::format_bytes(group.write_speed, self.settings.unit_base),
        );

        h_flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .child(
                AlertDialog::new(cx)
                    .trigger(
                        Button::new(("app-information", row_ix))
                            .outline()
                            .small()
                            .label("Info"),
                    )
                    .content(move |content, _, _| {
                        content
                            .child(
                                DialogHeader::new()
                                    .child(
                                        DialogTitle::new()
                                            .child(format!("{info_name} Information")),
                                    )
                                    .child(
                                        DialogDescription::new()
                                            .child(info_description.clone()),
                                    ),
                            )
                            .child(
                                DialogFooter::new().child(
                                    DialogClose::new().child(
                                        Button::new(("app-information-close", row_ix))
                                            .outline()
                                            .label("Close"),
                                    ),
                                ),
                            )
                    }),
            )
            .child(
                AlertDialog::new(cx)
                    .trigger(
                        Button::new(("app-term", row_ix))
                            .outline()
                            .small()
                            .label("End"),
                    )
                    .on_ok(move |_, _, cx| {
                        let _ = term_target.update(cx, |monitor, cx| {
                            monitor.signal_pids(&term_pids, Signal::Term);
                            cx.notify();
                        });
                        true
                    })
                    .content(move |content, _, _| {
                        content
                            .child(
                                DialogHeader::new()
                                    .child(
                                        DialogTitle::new().child(format!("End {app_name}?")),
                                    )
                                    .child(DialogDescription::new().child(
                                        "All processes in this application group will receive a graceful termination signal.",
                                    )),
                            )
                            .child(
                                DialogFooter::new()
                                    .child(DialogClose::new().child(
                                        Button::new(("app-term-cancel", row_ix))
                                            .outline()
                                            .label("Cancel"),
                                    ))
                                    .child(DialogAction::new().child(
                                        Button::new(("app-term-confirm", row_ix))
                                            .warning()
                                            .label("End Application"),
                                    )),
                            )
                    }),
            )
            .child(
                AlertDialog::new(cx)
                    .trigger(
                        Button::new(("app-kill", row_ix))
                            .warning()
                            .small()
                            .label("Force"),
                    )
                    .on_ok(move |_, _, cx| {
                        let _ = kill_target.update(cx, |monitor, cx| {
                            monitor.signal_pids(&kill_pids, Signal::Kill);
                            cx.notify();
                        });
                        true
                    })
                    .content(move |content, _, _| {
                        content
                            .child(
                                DialogHeader::new()
                                    .child(
                                        DialogTitle::new().child("Force stop application?"),
                                    )
                                    .child(DialogDescription::new().child(
                                        "Every process in this application group will stop immediately without cleanup.",
                                    )),
                            )
                            .child(
                                DialogFooter::new()
                                    .child(DialogClose::new().child(
                                        Button::new(("app-kill-cancel", row_ix))
                                            .outline()
                                            .label("Cancel"),
                                    ))
                                    .child(DialogAction::new().child(
                                        Button::new(("app-kill-confirm", row_ix))
                                            .warning()
                                            .label("Force stop"),
                                    )),
                            )
                    }),
            )
            .child(
                Button::new(("app-stop", row_ix))
                    .ghost()
                    .small()
                    .label("Pause")
                    .on_click(move |_, _, cx| {
                        let _ = stop_target.update(cx, |monitor, cx| {
                            monitor.signal_pids(&stop_pids, Signal::Stop);
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new(("app-continue", row_ix))
                    .ghost()
                    .small()
                    .label("Resume")
                    .on_click(move |_, _, cx| {
                        let _ = continue_target.update(cx, |monitor, cx| {
                            monitor.signal_pids(&continue_pids, Signal::Continue);
                            cx.notify();
                        });
                    }),
            )
            .into_any_element()
    }
}

impl TableDelegate for AppTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.groups.len()
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
        let Some(group) = self.groups.get(row_ix) else {
            return div().into_any_element();
        };
        let Some(column) = self.column_kinds.get(col_ix).copied() else {
            return div().into_any_element();
        };

        if column == AppColumn::Actions {
            return self.render_actions(row_ix, group.clone(), cx);
        }

        let value = self.cell_value(group, column);
        let color = match column {
            AppColumn::Cpu if group.cpu_usage >= 50.0 => cx.theme().red,
            AppColumn::Cpu if group.cpu_usage >= 20.0 => cx.theme().yellow,
            AppColumn::Cpu => cx.theme().blue,
            AppColumn::Memory | AppColumn::CombinedMemory => cx.theme().green,
            AppColumn::Gpu | AppColumn::GpuMemory | AppColumn::Encoder | AppColumn::Decoder => {
                cx.theme().muted_foreground
            }
            _ => cx.theme().foreground,
        };
        div()
            .text_sm()
            .text_color(color)
            .truncate()
            .child(value)
            .into_any_element()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        let Some(column) = self.column_kinds.get(col_ix).copied() else {
            return;
        };
        if !column.sortable() {
            return;
        }
        self.sort_column = column;
        self.sort_order = sort;
        self.sort_groups();
        if let Err(error) =
            sort_preferences::save_app(column.id(), matches!(sort, ColumnSort::Descending))
        {
            eprintln!("could not save Apps sort preference: {error}");
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        let Some(group) = self.groups.get(row_ix) else {
            return String::new();
        };
        let Some(column) = self.column_kinds.get(col_ix).copied() else {
            return String::new();
        };
        self.cell_value(group, column)
    }
}
