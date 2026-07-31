use std::cmp::Ordering;

use gpui::*;
use gpui_component::{
    ActiveTheme,
    table::{Column, ColumnSort, TableDelegate, TableState},
};
use sysinfo::Pid;

use crate::{
    linux,
    settings::MonitorSettings,
};

#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: Pid,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub user: String,
    pub state: String,
    pub cpu_usage: f32,
    pub memory: u64,
    pub swap: u64,
    pub read_speed: u64,
    pub read_total: u64,
    pub write_speed: u64,
    pub write_total: u64,
    pub total_cpu_time_ticks: Option<u64>,
    pub user_cpu_time_ticks: Option<u64>,
    pub system_cpu_time_ticks: Option<u64>,
    pub priority: Option<i64>,
    pub nice: Option<i64>,
    pub threads: Option<u64>,
    pub file_descriptors: Option<usize>,
    pub command_line: String,
    pub executable: Option<String>,
    pub working_directory: Option<String>,
    pub cgroup: Option<String>,
    pub app_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessColumn {
    Name,
    Pid,
    User,
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
    TotalCpuTime,
    UserCpuTime,
    SystemCpuTime,
    Priority,
    Swap,
    CombinedMemory,
    CommandLine,
    State,
}

impl ProcessColumn {
    const fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Pid => "pid",
            Self::User => "user",
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
            Self::TotalCpuTime => "total-cpu-time",
            Self::UserCpuTime => "user-cpu-time",
            Self::SystemCpuTime => "system-cpu-time",
            Self::Priority => "priority",
            Self::Swap => "swap",
            Self::CombinedMemory => "combined-memory",
            Self::CommandLine => "command-line",
            Self::State => "state",
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Name => "Process",
            Self::Pid => "PID",
            Self::User => "User",
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
            Self::TotalCpuTime => "CPU time",
            Self::UserCpuTime => "User CPU",
            Self::SystemCpuTime => "System CPU",
            Self::Priority => "Priority",
            Self::Swap => "Swap",
            Self::CombinedMemory => "Memory + swap",
            Self::CommandLine => "Command line",
            Self::State => "State",
        }
    }

    const fn width(self) -> f32 {
        match self {
            Self::Name => 240.0,
            Self::Pid => 78.0,
            Self::User => 112.0,
            Self::Memory | Self::Swap | Self::CombinedMemory | Self::GpuMemory => 112.0,
            Self::Cpu | Self::Gpu | Self::Encoder | Self::Decoder => 86.0,
            Self::ReadSpeed | Self::WriteSpeed => 104.0,
            Self::ReadTotal | Self::WriteTotal => 112.0,
            Self::TotalCpuTime | Self::UserCpuTime | Self::SystemCpuTime => 110.0,
            Self::Priority => 112.0,
            Self::CommandLine => 420.0,
            Self::State => 100.0,
        }
    }
}

pub struct ProcessTableDelegate {
    all_processes: Vec<ProcessInfo>,
    pub processes: Vec<ProcessInfo>,
    columns: Vec<Column>,
    column_kinds: Vec<ProcessColumn>,
    sort_column: ProcessColumn,
    sort_order: ColumnSort,
    query: String,
    settings: MonitorSettings,
}

impl ProcessTableDelegate {
    pub fn new(settings: &MonitorSettings) -> Self {
        let mut this = Self {
            all_processes: Vec::new(),
            processes: Vec::new(),
            columns: Vec::new(),
            column_kinds: Vec::new(),
            sort_column: ProcessColumn::Cpu,
            sort_order: ColumnSort::Descending,
            query: String::new(),
            settings: settings.clone(),
        };
        this.rebuild_columns();
        this
    }

    pub fn set_settings(&mut self, settings: &MonitorSettings) {
        self.settings = settings.clone();
        self.rebuild_columns();
        self.refresh_rows();
    }

    pub fn set_processes(&mut self, processes: Vec<ProcessInfo>) {
        self.all_processes = processes;
        self.refresh_rows();
    }

    pub fn set_filter(&mut self, query: impl Into<String>) {
        self.query = query.into().trim().to_lowercase();
        self.refresh_rows();
    }

    pub fn process_at(&self, row: usize) -> Option<&ProcessInfo> {
        self.processes.get(row)
    }

    fn rebuild_columns(&mut self) {
        let columns = &self.settings.process_columns;
        let mut kinds = vec![ProcessColumn::Name];
        if columns.pid {
            kinds.push(ProcessColumn::Pid);
        }
        if columns.user {
            kinds.push(ProcessColumn::User);
        }
        if columns.memory {
            kinds.push(ProcessColumn::Memory);
        }
        if columns.cpu {
            kinds.push(ProcessColumn::Cpu);
        }
        if columns.read_speed {
            kinds.push(ProcessColumn::ReadSpeed);
        }
        if columns.read_total {
            kinds.push(ProcessColumn::ReadTotal);
        }
        if columns.write_speed {
            kinds.push(ProcessColumn::WriteSpeed);
        }
        if columns.write_total {
            kinds.push(ProcessColumn::WriteTotal);
        }
        if columns.gpu {
            kinds.push(ProcessColumn::Gpu);
        }
        if columns.gpu_memory {
            kinds.push(ProcessColumn::GpuMemory);
        }
        if columns.encoder {
            kinds.push(ProcessColumn::Encoder);
        }
        if columns.decoder {
            kinds.push(ProcessColumn::Decoder);
        }
        if columns.total_cpu_time {
            kinds.push(ProcessColumn::TotalCpuTime);
        }
        if columns.user_cpu_time {
            kinds.push(ProcessColumn::UserCpuTime);
        }
        if columns.system_cpu_time {
            kinds.push(ProcessColumn::SystemCpuTime);
        }
        if columns.priority {
            kinds.push(ProcessColumn::Priority);
        }
        if columns.swap {
            kinds.push(ProcessColumn::Swap);
        }
        if columns.combined_memory {
            kinds.push(ProcessColumn::CombinedMemory);
        }
        if columns.command_line {
            kinds.push(ProcessColumn::CommandLine);
        }
        kinds.push(ProcessColumn::State);

        self.columns = kinds
            .iter()
            .map(|kind| {
                let column = Column::new(kind.id(), kind.title())
                    .width(kind.width())
                    .sortable();
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
            self.processes.clone_from(&self.all_processes);
        } else {
            let terms = self
                .query
                .split('|')
                .map(str::trim)
                .filter(|term| !term.is_empty())
                .collect::<Vec<_>>();
            self.processes = self
                .all_processes
                .iter()
                .filter(|process| {
                    let haystack = format!(
                        "{} {} {} {} {} {} {}",
                        process.name,
                        process.pid,
                        process.user,
                        process.command_line,
                        process.app_id.as_deref().unwrap_or_default(),
                        process.cgroup.as_deref().unwrap_or_default(),
                        process.state
                    )
                    .to_lowercase();
                    terms.iter().all(|term| haystack.contains(term))
                })
                .cloned()
                .collect();
        }
        self.sort_processes();
    }

    fn sort_processes(&mut self) {
        let descending = matches!(self.sort_order, ColumnSort::Descending);
        let column = self.sort_column;
        self.processes.sort_by(|a, b| {
            let ordering = match column {
                ProcessColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                ProcessColumn::Pid => a.pid.as_u32().cmp(&b.pid.as_u32()),
                ProcessColumn::User => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
                ProcessColumn::Memory => a.memory.cmp(&b.memory),
                ProcessColumn::Cpu => a
                    .cpu_usage
                    .partial_cmp(&b.cpu_usage)
                    .unwrap_or(Ordering::Equal),
                ProcessColumn::ReadSpeed => a.read_speed.cmp(&b.read_speed),
                ProcessColumn::ReadTotal => a.read_total.cmp(&b.read_total),
                ProcessColumn::WriteSpeed => a.write_speed.cmp(&b.write_speed),
                ProcessColumn::WriteTotal => a.write_total.cmp(&b.write_total),
                ProcessColumn::TotalCpuTime => a.total_cpu_time_ticks.cmp(&b.total_cpu_time_ticks),
                ProcessColumn::UserCpuTime => a.user_cpu_time_ticks.cmp(&b.user_cpu_time_ticks),
                ProcessColumn::SystemCpuTime => {
                    a.system_cpu_time_ticks.cmp(&b.system_cpu_time_ticks)
                }
                ProcessColumn::Priority => a.nice.cmp(&b.nice),
                ProcessColumn::Swap => a.swap.cmp(&b.swap),
                ProcessColumn::CombinedMemory => a
                    .memory
                    .saturating_add(a.swap)
                    .cmp(&b.memory.saturating_add(b.swap)),
                ProcessColumn::CommandLine => a.command_line.cmp(&b.command_line),
                ProcessColumn::State => a.state.cmp(&b.state),
                ProcessColumn::Gpu
                | ProcessColumn::GpuMemory
                | ProcessColumn::Encoder
                | ProcessColumn::Decoder => Ordering::Equal,
            };
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    fn cell_value(&self, process: &ProcessInfo, column: ProcessColumn) -> String {
        match column {
            ProcessColumn::Name => process.name.clone(),
            ProcessColumn::Pid => process.pid.to_string(),
            ProcessColumn::User => process.user.clone(),
            ProcessColumn::Memory => linux::format_bytes(process.memory, self.settings.unit_base),
            ProcessColumn::Cpu => format!("{:.1}%", process.cpu_usage),
            ProcessColumn::ReadSpeed => {
                linux::format_rate(process.read_speed, false, self.settings.unit_base)
            }
            ProcessColumn::ReadTotal => {
                linux::format_bytes(process.read_total, self.settings.unit_base)
            }
            ProcessColumn::WriteSpeed => {
                linux::format_rate(process.write_speed, false, self.settings.unit_base)
            }
            ProcessColumn::WriteTotal => {
                linux::format_bytes(process.write_total, self.settings.unit_base)
            }
            ProcessColumn::Gpu
            | ProcessColumn::GpuMemory
            | ProcessColumn::Encoder
            | ProcessColumn::Decoder => "N/A".to_string(),
            ProcessColumn::TotalCpuTime => format_ticks(process.total_cpu_time_ticks),
            ProcessColumn::UserCpuTime => format_ticks(process.user_cpu_time_ticks),
            ProcessColumn::SystemCpuTime => format_ticks(process.system_cpu_time_ticks),
            ProcessColumn::Priority => format_priority(process.nice, self.settings.detailed_priority),
            ProcessColumn::Swap => linux::format_bytes(process.swap, self.settings.unit_base),
            ProcessColumn::CombinedMemory => linux::format_bytes(
                process.memory.saturating_add(process.swap),
                self.settings.unit_base,
            ),
            ProcessColumn::CommandLine => {
                if process.command_line.is_empty() {
                    "N/A".to_string()
                } else {
                    process.command_line.clone()
                }
            }
            ProcessColumn::State => process.state.clone(),
        }
    }
}

impl TableDelegate for ProcessTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.processes.len()
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
        let Some(process) = self.processes.get(row_ix) else {
            return div().into_any_element();
        };
        let Some(column) = self.column_kinds.get(col_ix).copied() else {
            return div().into_any_element();
        };
        let value = self.cell_value(process, column);
        let color = match column {
            ProcessColumn::Cpu if process.cpu_usage >= 50.0 => cx.theme().red,
            ProcessColumn::Cpu if process.cpu_usage >= 20.0 => cx.theme().yellow,
            ProcessColumn::Cpu => cx.theme().blue,
            ProcessColumn::Memory | ProcessColumn::CombinedMemory => cx.theme().green,
            ProcessColumn::Gpu
            | ProcessColumn::GpuMemory
            | ProcessColumn::Encoder
            | ProcessColumn::Decoder => cx.theme().muted_foreground,
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
        self.sort_column = column;
        self.sort_order = sort;
        self.sort_processes();
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        let Some(process) = self.processes.get(row_ix) else {
            return String::new();
        };
        let Some(column) = self.column_kinds.get(col_ix).copied() else {
            return String::new();
        };
        self.cell_value(process, column)
    }
}

fn format_ticks(ticks: Option<u64>) -> String {
    let Some(ticks) = ticks else {
        return "N/A".to_string();
    };
    // Linux commonly exposes USER_HZ as 100. The raw source remains available
    // in process details until a typed sysconf adapter is added.
    let seconds = ticks / 100;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_priority(nice: Option<i64>, detailed: bool) -> String {
    let Some(nice) = nice else {
        return "N/A".to_string();
    };
    let label = match nice {
        i64::MIN..=-8 => "Very High",
        -7..=-3 => "High",
        -2..=2 => "Normal",
        3..=6 => "Low",
        _ => "Very Low",
    };
    if detailed {
        format!("{label} ({nice:+})")
    } else {
        label.to_string()
    }
}
