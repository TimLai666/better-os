use std::{
    cmp::Ordering,
    collections::VecDeque,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use better_ui::page_heading;
use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::{
    ActiveTheme, Root, Selectable as _, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    chart::AreaChart,
    h_flex,
    table::{Column, ColumnSort, DataTable, TableDelegate, TableState},
    v_flex,
};
use monitor_core::{Incident, MonitorStore, Sample};
use smol::Timer;
use sysinfo::{Disks, Networks, Pid, System};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MAX_DATA_POINTS: usize = 120;
const MIB: f64 = 1024.0 * 1024.0;

// UI patterns were studied from gpui-component's system_monitor example at
// commit 88f102d13654fe25aa2fede076274b6b751a3704. Better Monitor's screen
// composition, navigation, wording, state model, and diagnostics are original.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum MonitorPage {
    #[default]
    Overview,
    Apps,
    Processes,
    Cpu,
    Memory,
    Storage,
    Network,
    History,
    Incidents,
    Diagnostics,
}

impl MonitorPage {
    const PRIMARY: [Self; 7] = [
        Self::Overview,
        Self::Apps,
        Self::Processes,
        Self::Cpu,
        Self::Memory,
        Self::Storage,
        Self::Network,
    ];

    const INVESTIGATE: [Self; 3] = [Self::History, Self::Incidents, Self::Diagnostics];

    const fn id(self) -> &'static str {
        match self {
            Self::Overview => "monitor-overview",
            Self::Apps => "monitor-apps",
            Self::Processes => "monitor-processes",
            Self::Cpu => "monitor-cpu",
            Self::Memory => "monitor-memory",
            Self::Storage => "monitor-storage",
            Self::Network => "monitor-network",
            Self::History => "monitor-history",
            Self::Incidents => "monitor-incidents",
            Self::Diagnostics => "monitor-diagnostics",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Apps => "Apps",
            Self::Processes => "Processes",
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::History => "History",
            Self::Incidents => "Incidents",
            Self::Diagnostics => "Diagnostics",
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Overview => "◫",
            Self::Apps => "▦",
            Self::Processes => "≡",
            Self::Cpu => "◇",
            Self::Memory => "▤",
            Self::Storage => "▱",
            Self::Network => "⌁",
            Self::History => "↗",
            Self::Incidents => "!",
            Self::Diagnostics => "⊙",
        }
    }

    const fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "Current pressure, throughput, and resource health",
            Self::Apps => "Application candidates and future cgroup-backed grouping",
            Self::Processes => "Live process activity with sortable resource columns",
            Self::Cpu => "Utilization, load, clocks, and logical CPU activity",
            Self::Memory => "Available memory, swap, cache context, and pressure coverage",
            Self::Storage => "Filesystem capacity and current process I/O throughput",
            Self::Network => "Per-interface throughput and link activity",
            Self::History => "Recent low-cost samples retained by the monitor session",
            Self::Incidents => "User-marked slowdown moments and their sample positions",
            Self::Diagnostics => "Collector health, coverage, and known blind spots",
        }
    }
}

#[derive(Clone)]
struct MetricPoint {
    time: String,
    cpu: f64,
    memory: f64,
    network_received: f64,
    network_transmitted: f64,
    disk_read: f64,
    disk_written: f64,
}

#[derive(Clone)]
struct ProcessInfo {
    pid: Pid,
    name: String,
    state: String,
    cpu_usage: f32,
    memory: u64,
    read_bytes: u64,
    written_bytes: u64,
}

#[derive(Clone)]
struct DiskInfo {
    name: String,
    mount_point: String,
    file_system: String,
    total: u64,
    available: u64,
}

#[derive(Clone)]
struct NetworkInfo {
    name: String,
    received: u64,
    transmitted: u64,
    total_received: u64,
    total_transmitted: u64,
}

#[derive(Clone)]
struct IncidentMarker {
    sequence: usize,
    sample_index: usize,
    recorded_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ProcessSortField {
    Name,
    Pid,
    #[default]
    Cpu,
    Memory,
    Read,
    Write,
    State,
}

struct ProcessTableDelegate {
    processes: Vec<ProcessInfo>,
    columns: Vec<Column>,
    sort_field: ProcessSortField,
    sort_order: ColumnSort,
}

impl ProcessTableDelegate {
    fn new() -> Self {
        Self {
            processes: Vec::new(),
            columns: vec![
                Column::new("name", "Process").width(260.).sortable(),
                Column::new("pid", "PID").width(78.).sortable(),
                Column::new("cpu", "CPU %")
                    .width(86.)
                    .sortable()
                    .sort(ColumnSort::Descending),
                Column::new("memory", "Memory").width(108.).sortable(),
                Column::new("read", "Read/s").width(100.).sortable(),
                Column::new("write", "Write/s").width(100.).sortable(),
                Column::new("state", "State").width(100.).sortable(),
            ],
            sort_field: ProcessSortField::Cpu,
            sort_order: ColumnSort::Descending,
        }
    }

    fn set_processes(&mut self, processes: Vec<ProcessInfo>) {
        self.processes = processes;
        self.sort_processes();
        self.processes.truncate(400);
    }

    fn sort_processes(&mut self) {
        let descending = matches!(self.sort_order, ColumnSort::Descending);
        self.processes.sort_by(|a, b| {
            let ordering = match self.sort_field {
                ProcessSortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                ProcessSortField::Pid => a.pid.as_u32().cmp(&b.pid.as_u32()),
                ProcessSortField::Cpu => a
                    .cpu_usage
                    .partial_cmp(&b.cpu_usage)
                    .unwrap_or(Ordering::Equal),
                ProcessSortField::Memory => a.memory.cmp(&b.memory),
                ProcessSortField::Read => a.read_bytes.cmp(&b.read_bytes),
                ProcessSortField::Write => a.written_bytes.cmp(&b.written_bytes),
                ProcessSortField::State => a.state.cmp(&b.state),
            };
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
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

        match col_ix {
            0 => div()
                .text_sm()
                .text_color(cx.theme().foreground)
                .truncate()
                .child(process.name.clone())
                .into_any_element(),
            1 => div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(process.pid.to_string())
                .into_any_element(),
            2 => div()
                .text_xs()
                .text_color(if process.cpu_usage >= 50.0 {
                    cx.theme().red
                } else if process.cpu_usage >= 20.0 {
                    cx.theme().yellow
                } else {
                    cx.theme().blue
                })
                .child(format!("{:.1}%", process.cpu_usage))
                .into_any_element(),
            3 => div()
                .text_xs()
                .text_color(cx.theme().green)
                .child(format_bytes(process.memory))
                .into_any_element(),
            4 => div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format_bytes(process.read_bytes))
                .into_any_element(),
            5 => div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format_bytes(process.written_bytes))
                .into_any_element(),
            6 => div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(process.state.clone())
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.sort_order = sort;
        self.sort_field = match col_ix {
            0 => ProcessSortField::Name,
            1 => ProcessSortField::Pid,
            2 => ProcessSortField::Cpu,
            3 => ProcessSortField::Memory,
            4 => ProcessSortField::Read,
            5 => ProcessSortField::Write,
            6 => ProcessSortField::State,
            _ => ProcessSortField::Cpu,
        };
        self.sort_processes();
    }
}

struct MonitorWindow {
    system: System,
    disks: Disks,
    networks: Networks,
    history: VecDeque<MetricPoint>,
    frozen_history: Vec<MetricPoint>,
    top_processes: Vec<ProcessInfo>,
    disk_info: Vec<DiskInfo>,
    network_info: Vec<NetworkInfo>,
    active_page: MonitorPage,
    charts_paused: bool,
    sample_index: usize,
    process_table: Entity<TableState<ProcessTableDelegate>>,
    incidents: Vec<IncidentMarker>,
    store: MonitorStore,
}

impl MonitorWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        let process_table = cx.new(|cx| {
            TableState::new(ProcessTableDelegate::new(), window, cx)
                .col_selectable(false)
                .col_movable(false)
        });

        let mut monitor = Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            history: VecDeque::with_capacity(MAX_DATA_POINTS),
            frozen_history: Vec::new(),
            top_processes: Vec::new(),
            disk_info: Vec::new(),
            network_info: Vec::new(),
            active_page: MonitorPage::Overview,
            charts_paused: false,
            sample_index: 0,
            process_table,
            incidents: Vec::new(),
            store: MonitorStore::default(),
        };

        monitor.collect_metrics(cx);
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(REFRESH_INTERVAL).await;
                if this
                    .update(cx, |this, cx| {
                        this.collect_metrics(cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        monitor
    }

    fn collect_metrics(&mut self, cx: &mut Context<Self>) {
        self.system.refresh_all();
        self.disks.refresh(true);
        self.networks.refresh(true);

        let cpu = self.system.global_cpu_usage() as f64;
        let total_memory = self.system.total_memory() as f64;
        let used_memory = self.system.used_memory() as f64;
        let memory = if total_memory > 0.0 {
            (used_memory / total_memory * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let network_received = self
            .networks
            .iter()
            .map(|(_, data)| data.received())
            .sum::<u64>() as f64
            / MIB;
        let network_transmitted = self
            .networks
            .iter()
            .map(|(_, data)| data.transmitted())
            .sum::<u64>() as f64
            / MIB;

        let disk_read = self
            .system
            .processes()
            .values()
            .map(|process| process.disk_usage().read_bytes)
            .sum::<u64>() as f64
            / MIB;
        let disk_written = self
            .system
            .processes()
            .values()
            .map(|process| process.disk_usage().written_bytes)
            .sum::<u64>() as f64
            / MIB;

        let point = MetricPoint {
            time: format!("{}s", self.sample_index),
            cpu,
            memory,
            network_received,
            network_transmitted,
            disk_read,
            disk_written,
        };

        if self.history.len() >= MAX_DATA_POINTS {
            self.history.pop_front();
        }
        self.history.push_back(point);
        self.store.record_sample(Sample {
            timestamp_unix_ms: unix_time_ms(),
            cpu_percent: cpu as f32,
            memory_percent: memory as f32,
            psi_some_percent: None,
        });
        self.sample_index += 1;

        let mut processes = self
            .system
            .processes()
            .iter()
            .map(|(pid, process)| {
                let disk = process.disk_usage();
                ProcessInfo {
                    pid: *pid,
                    name: process.name().to_string_lossy().to_string(),
                    state: format!("{:?}", process.status()),
                    cpu_usage: process.cpu_usage(),
                    memory: process.memory(),
                    read_bytes: disk.read_bytes,
                    written_bytes: disk.written_bytes,
                }
            })
            .collect::<Vec<_>>();
        processes.sort_by(|a, b| {
            b.cpu_usage
                .partial_cmp(&a.cpu_usage)
                .unwrap_or(Ordering::Equal)
        });
        self.top_processes = processes.iter().take(7).cloned().collect();
        self.process_table.update(cx, |table, cx| {
            table.delegate_mut().set_processes(processes);
            cx.notify();
        });

        self.disk_info = self
            .disks
            .iter()
            .map(|disk| DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                file_system: disk.file_system().to_string_lossy().to_string(),
                total: disk.total_space(),
                available: disk.available_space(),
            })
            .collect();

        self.network_info = self
            .networks
            .iter()
            .map(|(name, data)| NetworkInfo {
                name: name.clone(),
                received: data.received(),
                transmitted: data.transmitted(),
                total_received: data.total_received(),
                total_transmitted: data.total_transmitted(),
            })
            .collect();
        self.network_info.sort_by(|a, b| a.name.cmp(&b.name));
    }

    fn current_point(&self) -> MetricPoint {
        self.history.back().cloned().unwrap_or(MetricPoint {
            time: "0s".into(),
            cpu: 0.0,
            memory: 0.0,
            network_received: 0.0,
            network_transmitted: 0.0,
            disk_read: 0.0,
            disk_written: 0.0,
        })
    }

    fn chart_data(&self) -> Vec<MetricPoint> {
        if self.charts_paused && !self.frozen_history.is_empty() {
            self.frozen_history.clone()
        } else {
            self.history.iter().cloned().collect()
        }
    }

    fn nav_button(&self, page: MonitorPage, cx: &mut Context<Self>) -> Button {
        Button::new(page.id())
            .ghost()
            .small()
            .w_full()
            .label(format!("{}   {}", page.marker(), page.label()))
            .selected(self.active_page == page)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_page = page;
                cx.notify();
            }))
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .w(px(224.))
            .h_full()
            .flex_shrink_0()
            .gap_4()
            .p_3()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted)
            .child(
                v_flex()
                    .gap_1()
                    .px_2()
                    .py_2()
                    .child(div().font_bold().text_lg().child("Better Monitor"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Live system evidence"),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(self.sidebar_label("Monitor", cx))
                    .children(
                        MonitorPage::PRIMARY
                            .into_iter()
                            .map(|page| self.nav_button(page, cx)),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(self.sidebar_label("Investigate", cx))
                    .children(
                        MonitorPage::INVESTIGATE
                            .into_iter()
                            .map(|page| self.nav_button(page, cx)),
                    ),
            )
            .child(div().flex_1())
            .child(
                v_flex()
                    .gap_1()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .p_3()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(div().size_2().rounded(px(99.)).bg(cx.theme().green))
                            .child(div().text_sm().font_bold().child("Recording")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} samples this session",
                                self.store.samples().len()
                            )),
                    ),
            )
    }

    fn sidebar_label(&self, label: &'static str, cx: &Context<Self>) -> Div {
        div()
            .px_2()
            .py_1()
            .text_xs()
            .font_bold()
            .text_color(cx.theme().muted_foreground)
            .child(label)
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .items_center()
            .justify_between()
            .gap_4()
            .px_5()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .font_bold()
                    .text_lg()
                    .child(page_heading(self.active_page.label()))
                    .child(
                        div()
                            .font_normal()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.active_page.subtitle()),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("pause-charts")
                            .outline()
                            .small()
                            .label(if self.charts_paused {
                                "Resume charts"
                            } else {
                                "Pause charts"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.charts_paused {
                                    this.charts_paused = false;
                                    this.frozen_history.clear();
                                } else {
                                    this.frozen_history =
                                        this.history.iter().cloned().collect::<Vec<_>>();
                                    this.charts_paused = true;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("record-slowdown")
                            .warning()
                            .small()
                            .label("The system was just slow")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let sequence = this.incidents.len() + 1;
                                let recorded_at_ms = unix_time_ms();
                                this.store.record_incident(Incident {
                                    timestamp_unix_ms: recorded_at_ms,
                                    title: format!("Slowdown marker #{sequence}"),
                                    note: Some(format!("Sample {}", this.sample_index)),
                                });
                                this.incidents.push(IncidentMarker {
                                    sequence,
                                    sample_index: this.sample_index,
                                    recorded_at_ms,
                                });
                                this.active_page = MonitorPage::Incidents;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_status_bar(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        h_flex()
            .h_7()
            .items_center()
            .justify_between()
            .gap_4()
            .px_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(format!(
                "CPU {:.1}%  •  Memory {:.1}%  •  ↓ {:.2} MiB/s  ↑ {:.2} MiB/s",
                point.cpu, point.memory, point.network_received, point.network_transmitted
            ))
            .child(if self.charts_paused {
                "Charts paused • history still recording"
            } else {
                "Live • 1 second refresh"
            })
    }

    fn metric_card(
        &self,
        title: &'static str,
        value: String,
        detail: String,
        color: Hsla,
        cx: &Context<Self>,
    ) -> Div {
        v_flex()
            .flex_1()
            .min_w(px(180.))
            .gap_2()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().list)
            .p_4()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(title),
                    )
                    .child(div().size_2().rounded(px(99.)).bg(color)),
            )
            .child(div().text_lg().font_bold().child(value))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(detail),
            )
    }

    fn chart_card(
        &self,
        title: &'static str,
        value: String,
        data: Vec<MetricPoint>,
        value_fn: impl Fn(&MetricPoint) -> f64 + 'static,
        color: Hsla,
        cx: &Context<Self>,
    ) -> Div {
        v_flex()
            .flex_1()
            .min_w(px(330.))
            .min_h(px(220.))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().list)
            .overflow_hidden()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .child(div().text_sm().font_bold().child(title))
                    .child(div().text_sm().text_color(color).child(value)),
            )
            .child(
                AreaChart::new(data)
                    .x(|point| point.time.clone())
                    .y(value_fn)
                    .stroke(color)
                    .fill(linear_gradient(
                        0.,
                        linear_color_stop(color.opacity(0.34), 1.),
                        linear_color_stop(cx.theme().background.opacity(0.05), 0.),
                    ))
                    .tick_margin(16),
            )
    }

    fn section_card(
        &self,
        title: &'static str,
        subtitle: &'static str,
        content: impl IntoElement,
        cx: &Context<Self>,
    ) -> Div {
        v_flex()
            .flex_1()
            .min_w(px(320.))
            .gap_3()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().list)
            .p_4()
            .child(
                v_flex()
                    .gap_1()
                    .child(div().font_bold().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    ),
            )
            .child(content)
    }

    fn utilization_bar(&self, value: f32, color: Hsla, cx: &Context<Self>) -> Div {
        let width = value.clamp(0.0, 100.0) * 2.2;
        div()
            .w_full()
            .h(px(7.))
            .rounded(px(99.))
            .bg(cx.theme().muted)
            .overflow_hidden()
            .child(div().h_full().w(px(width)).rounded(px(99.)).bg(color))
    }

    fn render_overview(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let load = System::load_average();
        let history = self.chart_data();

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "CPU",
                        format!("{:.1}%", point.cpu),
                        format!(
                            "Load {:.2} / {:.2} / {:.2}",
                            load.one, load.five, load.fifteen
                        ),
                        cx.theme().red,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Memory",
                        format!("{:.1}%", point.memory),
                        format!("{} available", format_bytes(self.system.available_memory())),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Storage activity",
                        format!("{:.2} MiB/s", point.disk_read + point.disk_written),
                        format!(
                            "Read {:.2} • Write {:.2}",
                            point.disk_read, point.disk_written
                        ),
                        cx.theme().yellow,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Network",
                        format!(
                            "{:.2} MiB/s",
                            point.network_received + point.network_transmitted
                        ),
                        format!(
                            "Receive {:.2} • Send {:.2}",
                            point.network_received, point.network_transmitted
                        ),
                        cx.theme().green,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.chart_card(
                        "CPU usage",
                        format!("{:.1}%", point.cpu),
                        history.clone(),
                        |point| point.cpu,
                        cx.theme().red,
                        cx,
                    ))
                    .child(self.chart_card(
                        "Memory usage",
                        format!("{:.1}%", point.memory),
                        history,
                        |point| point.memory,
                        cx.theme().blue,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .items_start()
                    .flex_wrap()
                    .gap_4()
                    .child(self.render_top_processes(cx))
                    .child(self.render_observation_health(cx)),
            )
    }

    fn render_top_processes(&self, cx: &Context<Self>) -> Div {
        self.section_card(
            "Top processes",
            "Highest current CPU consumers",
            v_flex()
                .gap_1()
                .children(self.top_processes.iter().map(|process| {
                    h_flex()
                        .items_center()
                        .gap_3()
                        .py_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().truncate().child(process.name.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("PID {} • {}", process.pid, process.state)),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().red)
                                .child(format!("{:.1}%", process.cpu_usage)),
                        )
                        .child(
                            div()
                                .w(px(82.))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format_bytes(process.memory)),
                        )
                })),
            cx,
        )
    }

    fn render_observation_health(&self, cx: &Context<Self>) -> Div {
        self.section_card(
            "Observation health",
            "Coverage is explicit; missing data is never shown as zero",
            v_flex()
                .gap_3()
                .child(self.health_row("Portable system metrics", "Active", cx.theme().green, cx))
                .child(self.health_row(
                    "Process CPU / memory / I/O",
                    "Active",
                    cx.theme().green,
                    cx,
                ))
                .child(self.health_row("PSI pressure", "Not connected", cx.theme().yellow, cx))
                .child(self.health_row(
                    "cgroup app grouping",
                    "Not connected",
                    cx.theme().yellow,
                    cx,
                ))
                .child(self.health_row("GPU adapters", "Not connected", cx.theme().yellow, cx)),
            cx,
        )
    }

    fn health_row(
        &self,
        name: &'static str,
        state: &'static str,
        color: Hsla,
        cx: &Context<Self>,
    ) -> Div {
        h_flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().size_2().rounded(px(99.)).bg(color))
                    .child(div().text_sm().child(name)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(state),
            )
    }

    fn render_apps(&self, cx: &Context<Self>) -> Div {
        v_flex()
            .gap_4()
            .child(
                v_flex()
                    .gap_2()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().yellow)
                    .bg(cx.theme().list)
                    .p_4()
                    .child(div().font_bold().child("Application grouping is not active yet"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "This prototype shows process candidates individually. Production grouping must use cgroup, systemd unit, Flatpak, Snap, and desktop identity evidence rather than executable names alone.",
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .children(self.top_processes.iter().map(|process| {
                        v_flex()
                            .flex_1()
                            .min_w(px(220.))
                            .gap_3()
                            .rounded(cx.theme().radius_lg)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().list)
                            .p_4()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_bold()
                                            .truncate()
                                            .child(process.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("PID {}", process.pid)),
                                    ),
                            )
                            .child(self.utilization_bar(process.cpu_usage, cx.theme().red, cx))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("CPU {:.1}%", process.cpu_usage))
                                    .child(format_bytes(process.memory)),
                            )
                    })),
            )
    }

    fn render_processes(&self, cx: &Context<Self>) -> Div {
        v_flex()
            .min_h(px(560.))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().list)
            .overflow_hidden()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().font_bold().child("Live processes"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} processes", self.system.processes().len())),
                    ),
            )
            .child(
                div().flex_1().child(
                    DataTable::new(&self.process_table)
                        .bordered(false)
                        .stripe(true)
                        .small(),
                ),
            )
    }

    fn render_cpu(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let load = System::load_average();
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "Total CPU",
                        format!("{:.1}%", point.cpu),
                        format!("{} logical CPUs", self.system.cpus().len()),
                        cx.theme().red,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Load average",
                        format!("{:.2}", load.one),
                        format!("5 min {:.2} • 15 min {:.2}", load.five, load.fifteen),
                        cx.theme().yellow,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Uptime",
                        format_duration(System::uptime()),
                        "System-reported uptime".into(),
                        cx.theme().blue,
                        cx,
                    )),
            )
            .child(self.chart_card(
                "CPU utilization history",
                format!("{:.1}%", point.cpu),
                self.chart_data(),
                |point| point.cpu,
                cx.theme().red,
                cx,
            ))
            .child(h_flex().flex_wrap().gap_3().children(
                self.system.cpus().iter().enumerate().map(|(index, cpu)| {
                    v_flex()
                        .min_w(px(160.))
                        .flex_1()
                        .gap_2()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().list)
                        .p_3()
                        .child(
                            h_flex()
                                .justify_between()
                                .child(div().font_bold().child(format!("CPU {index}")))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} MHz", cpu.frequency())),
                                ),
                        )
                        .child(self.utilization_bar(cpu.cpu_usage(), cx.theme().red, cx))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{:.1}%", cpu.cpu_usage())),
                        )
                }),
            ))
    }

    fn render_memory(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let total = self.system.total_memory();
        let used = self.system.used_memory();
        let available = self.system.available_memory();
        let swap_total = self.system.total_swap();
        let swap_used = self.system.used_swap();

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "Used",
                        format_bytes(used),
                        format!("{:.1}% of {}", point.memory, format_bytes(total)),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Available",
                        format_bytes(available),
                        "Available is more useful than raw free memory".into(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Swap",
                        format_bytes(swap_used),
                        format!("{} total", format_bytes(swap_total)),
                        cx.theme().yellow,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Memory PSI",
                        "Unavailable".into(),
                        "Linux PSI collector is not connected yet".into(),
                        cx.theme().yellow,
                        cx,
                    )),
            )
            .child(self.chart_card(
                "Memory utilization history",
                format!("{:.1}%", point.memory),
                self.chart_data(),
                |point| point.memory,
                cx.theme().blue,
                cx,
            ))
    }

    fn render_storage(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let history = self.chart_data();
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.chart_card(
                        "Process read throughput",
                        format!("{:.2} MiB/s", point.disk_read),
                        history.clone(),
                        |point| point.disk_read,
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.chart_card(
                        "Process write throughput",
                        format!("{:.2} MiB/s", point.disk_written),
                        history,
                        |point| point.disk_written,
                        cx.theme().yellow,
                        cx,
                    )),
            )
            .child(v_flex().gap_3().children(self.disk_info.iter().map(|disk| {
                let used = disk.total.saturating_sub(disk.available);
                let percent = if disk.total > 0 {
                    used as f32 / disk.total as f32 * 100.0
                } else {
                    0.0
                };
                v_flex()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().list)
                    .p_4()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .child(div().font_bold().child(if disk.name.is_empty() {
                                        disk.mount_point.clone()
                                    } else {
                                        disk.name.clone()
                                    }))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{} • {}",
                                                disk.mount_point, disk.file_system
                                            )),
                                    ),
                            )
                            .child(div().text_sm().child(format!(
                                "{} / {}",
                                format_bytes(used),
                                format_bytes(disk.total)
                            ))),
                    )
                    .child(self.utilization_bar(percent, cx.theme().blue, cx))
            })))
    }

    fn render_network(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let history = self.chart_data();
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.chart_card(
                        "Receive throughput",
                        format!("{:.2} MiB/s", point.network_received),
                        history.clone(),
                        |point| point.network_received,
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.chart_card(
                        "Send throughput",
                        format!("{:.2} MiB/s", point.network_transmitted),
                        history,
                        |point| point.network_transmitted,
                        cx.theme().blue,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .gap_3()
                    .children(self.network_info.iter().map(|interface| {
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .rounded(cx.theme().radius_lg)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().list)
                            .p_4()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(div().font_bold().child(interface.name.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "Total ↓ {} • ↑ {}",
                                                format_bytes(interface.total_received),
                                                format_bytes(interface.total_transmitted)
                                            )),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_4()
                                    .child(
                                        div().text_sm().text_color(cx.theme().green).child(
                                            format!("↓ {}/s", format_bytes(interface.received)),
                                        ),
                                    )
                                    .child(div().text_sm().text_color(cx.theme().blue).child(
                                        format!("↑ {}/s", format_bytes(interface.transmitted)),
                                    )),
                            )
                    })),
            )
    }

    fn render_history(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let history = self.chart_data();
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "Samples",
                        self.store.samples().len().to_string(),
                        "One-second in-memory prototype retention".into(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Window",
                        format!("{} seconds", self.history.len()),
                        format!("Maximum {} recent points", MAX_DATA_POINTS),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Incident markers",
                        self.incidents.len().to_string(),
                        "Markers keep their sample position".into(),
                        cx.theme().yellow,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.chart_card(
                        "CPU history",
                        format!("{:.1}%", point.cpu),
                        history.clone(),
                        |point| point.cpu,
                        cx.theme().red,
                        cx,
                    ))
                    .child(self.chart_card(
                        "Memory history",
                        format!("{:.1}%", point.memory),
                        history,
                        |point| point.memory,
                        cx.theme().blue,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .gap_2()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().list)
                    .p_4()
                    .child(div().font_bold().child("Persistence boundary"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "This slice keeps history in memory. A restart-safe monitor service, downsampling, retention budgets, schema migration, and corrupted-tail recovery remain separate implementation work.",
                            ),
                    ),
            )
    }

    fn render_incidents(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().list)
                    .p_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().font_bold().child("Mark a slowdown moment"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "The marker records the current sample position so a future history service can capture the window before and after it.",
                                    ),
                            ),
                    )
                    .child(
                        Button::new("record-incident-page")
                            .warning()
                            .label("Record incident")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let sequence = this.incidents.len() + 1;
                                let recorded_at_ms = unix_time_ms();
                                this.store.record_incident(Incident {
                                    timestamp_unix_ms: recorded_at_ms,
                                    title: format!("Slowdown marker #{sequence}"),
                                    note: Some(format!("Sample {}", this.sample_index)),
                                });
                                this.incidents.push(IncidentMarker {
                                    sequence,
                                    sample_index: this.sample_index,
                                    recorded_at_ms,
                                });
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .when(self.incidents.is_empty(), |this| {
                        this.child(
                            v_flex()
                                .items_center()
                                .justify_center()
                                .min_h(px(260.))
                                .rounded(cx.theme().radius_lg)
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().list)
                                .child(div().font_bold().child("No incidents recorded"))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Use the marker when the system feels slow."),
                                ),
                        )
                    })
                    .children(self.incidents.iter().rev().map(|incident| {
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .rounded(cx.theme().radius_lg)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().list)
                            .p_4()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .font_bold()
                                            .child(format!("Slowdown marker #{}", incident.sequence)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "Recorded at sample {} • unix {} ms",
                                                incident.sample_index, incident.recorded_at_ms
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().yellow)
                                    .child("Capture pending"),
                            )
                    })),
            )
    }

    fn render_diagnostics(&self, cx: &Context<Self>) -> Div {
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "Collector loop",
                        "Healthy".into(),
                        "One-second event loop is updating".into(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Process coverage",
                        self.system.processes().len().to_string(),
                        "Portable sysinfo process records".into(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Disk coverage",
                        self.disk_info.len().to_string(),
                        "Mounted disk records".into(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Network coverage",
                        self.network_info.len().to_string(),
                        "Detected interfaces".into(),
                        cx.theme().green,
                        cx,
                    )),
            )
            .child(
                self.section_card(
                    "Collector matrix",
                    "Support state is part of the metric, not an afterthought",
                    v_flex()
                        .gap_3()
                        .child(self.health_row(
                            "CPU / memory / process baseline",
                            "Active via sysinfo",
                            cx.theme().green,
                            cx,
                        ))
                        .child(self.health_row(
                            "Disk capacity",
                            "Active via sysinfo",
                            cx.theme().green,
                            cx,
                        ))
                        .child(self.health_row(
                            "Interface throughput",
                            "Active via sysinfo",
                            cx.theme().green,
                            cx,
                        ))
                        .child(self.health_row(
                            "Linux PSI",
                            "Unsupported in this slice",
                            cx.theme().yellow,
                            cx,
                        ))
                        .child(self.health_row(
                            "cgroup v2 app identity",
                            "Unsupported in this slice",
                            cx.theme().yellow,
                            cx,
                        ))
                        .child(self.health_row(
                            "GPU engines and process attribution",
                            "No adapter selected",
                            cx.theme().yellow,
                            cx,
                        ))
                        .child(self.health_row(
                            "SMART / storage latency",
                            "No UDisks2 adapter selected",
                            cx.theme().yellow,
                            cx,
                        ))
                        .child(self.health_row(
                            "Persistent historical store",
                            "In-memory prototype only",
                            cx.theme().yellow,
                            cx,
                        )),
                    cx,
                ),
            )
    }

    fn render_page(&self, cx: &mut Context<Self>) -> Div {
        match self.active_page {
            MonitorPage::Overview => self.render_overview(cx),
            MonitorPage::Apps => self.render_apps(cx),
            MonitorPage::Processes => self.render_processes(cx),
            MonitorPage::Cpu => self.render_cpu(cx),
            MonitorPage::Memory => self.render_memory(cx),
            MonitorPage::Storage => self.render_storage(cx),
            MonitorPage::Network => self.render_network(cx),
            MonitorPage::History => self.render_history(cx),
            MonitorPage::Incidents => self.render_incidents(cx),
            MonitorPage::Diagnostics => self.render_diagnostics(cx),
        }
    }
}

impl Render for MonitorWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(cx))
            .child(
                v_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .child(self.render_header(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .p_5()
                            .child(self.render_page(cx)),
                    )
                    .child(self.render_status_bar(cx)),
            )
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB_U64: u64 = KIB * 1024;
    const GIB: u64 = MIB_U64 * 1024;
    const TIB: u64 = GIB * 1024;

    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB_U64 {
        format!("{:.1} MiB", bytes as f64 / MIB_U64 as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn main() {
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1320.), px(820.)), cx)),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| MonitorWindow::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open Better Monitor window");
        })
        .detach();
    });
}
