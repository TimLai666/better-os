use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use better_ui::page_heading;
use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::{
    ActiveTheme, Disableable, Root, Selectable as _, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    chart::AreaChart,
    h_flex,
    input::{InputEvent, InputState},
    scroll::ScrollableElement,
    table::{DataTable, TableEvent, TableState},
    v_flex,
};
use monitor_core::{Incident, MonitorStore, Sample};
use smol::Timer;
use sysinfo::{Disks, Networks, Pid, System};

use crate::{
    app_table::AppTableDelegate,
    linux::{
        self, AppGroup, AppProcessSample, BatteryDevice, BlockCounters, CpuDetails, GpuDevice,
        NetworkMetadata, NpuDevice,
    },
    process_table::{ProcessInfo, ProcessTableDelegate},
    settings::{MonitorSettings, RefreshSpeed, SidebarMeterType, TemperatureUnit, UnitBase},
};

#[path = "parity.rs"]
mod parity;

const MIB: f64 = 1024.0 * 1024.0;
const SECTOR_SIZE: u64 = 512;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum MonitorPage {
    #[default]
    Overview,
    Apps,
    Processes,
    Cpu,
    Memory,
    Gpu,
    Npu,
    Storage,
    Network,
    Battery,
    History,
    Incidents,
    Diagnostics,
    Settings,
}

impl MonitorPage {
    const INVESTIGATE: [Self; 3] = [Self::History, Self::Incidents, Self::Diagnostics];

    const fn id(self) -> &'static str {
        match self {
            Self::Overview => "monitor-overview",
            Self::Apps => "monitor-apps",
            Self::Processes => "monitor-processes",
            Self::Cpu => "monitor-cpu",
            Self::Memory => "monitor-memory",
            Self::Gpu => "monitor-gpu",
            Self::Npu => "monitor-npu",
            Self::Storage => "monitor-storage",
            Self::Network => "monitor-network",
            Self::Battery => "monitor-battery",
            Self::History => "monitor-history",
            Self::Incidents => "monitor-incidents",
            Self::Diagnostics => "monitor-diagnostics",
            Self::Settings => "monitor-settings",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Apps => "Apps",
            Self::Processes => "Processes",
            Self::Cpu => "Processor",
            Self::Memory => "Memory",
            Self::Gpu => "GPU",
            Self::Npu => "NPU",
            Self::Storage => "Drive",
            Self::Network => "Network Interface",
            Self::Battery => "Battery",
            Self::History => "History",
            Self::Incidents => "Incidents",
            Self::Diagnostics => "Diagnostics",
            Self::Settings => "Settings",
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Overview => "◫",
            Self::Apps => "▦",
            Self::Processes => "≡",
            Self::Cpu => "◇",
            Self::Memory => "▤",
            Self::Gpu => "▰",
            Self::Npu => "◇",
            Self::Storage => "▱",
            Self::Network => "⌁",
            Self::Battery => "▥",
            Self::History => "↗",
            Self::Incidents => "!",
            Self::Diagnostics => "⊙",
            Self::Settings => "⚙",
        }
    }

    const fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "Current resource activity and observation coverage",
            Self::Apps => "Application groups, resource columns, search, details, and controls",
            Self::Processes => "Sortable process metrics, search, details, and controls",
            Self::Cpu => "Total or logical CPU usage, clocks, temperature, topology, and uptime",
            Self::Memory => "Memory, swap, availability, and hardware-property coverage",
            Self::Gpu => "Usage, media engines, memory, thermals, power, clocks, and driver",
            Self::Npu => "Usage, memory, thermals, power, clocks, and driver",
            Self::Storage => "Per-drive activity, throughput, totals, capacity, and properties",
            Self::Network => "Per-interface traffic, totals, link, driver, and identity",
            Self::Battery => "Charge, power, health, capacity, cycles, and identity",
            Self::History => "Recent bounded samples and Better Monitor incident markers",
            Self::Incidents => "User-marked slowdown moments and evidence capture boundaries",
            Self::Diagnostics => "Collector health, support states, and observation blind spots",
            Self::Settings => "Refresh, units, sidebar, graphs, devices, and table columns",
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
struct DiskInfo {
    mount_point: String,
    file_system: String,
    total: u64,
    available: u64,
    metadata: linux::BlockMetadata,
    activity_percent: Option<f64>,
    read_speed: u64,
    write_speed: u64,
    total_read: u64,
    total_written: u64,
}

#[derive(Clone)]
struct NetworkInfo {
    name: String,
    received: u64,
    transmitted: u64,
    total_received: u64,
    total_transmitted: u64,
    metadata: NetworkMetadata,
}

#[derive(Clone)]
struct IncidentMarker {
    sequence: usize,
    sample_index: usize,
    recorded_at_ms: u64,
}

pub(crate) struct MonitorWindow {
    system: System,
    disks: Disks,
    networks: Networks,
    history: VecDeque<MetricPoint>,
    frozen_history: Vec<MetricPoint>,
    top_processes: Vec<ProcessInfo>,
    app_groups: Vec<AppGroup>,
    disk_info: Vec<DiskInfo>,
    network_info: Vec<NetworkInfo>,
    cpu_details: CpuDetails,
    gpus: Vec<GpuDevice>,
    npus: Vec<NpuDevice>,
    batteries: Vec<BatteryDevice>,
    users: HashMap<u32, String>,
    settings: MonitorSettings,
    search_input: Entity<InputState>,
    search_query: String,
    selected_pid: Option<Pid>,
    selected_gpu: usize,
    selected_npu: usize,
    selected_disk: usize,
    selected_network: usize,
    selected_battery: usize,
    last_action: Option<String>,
    _subscriptions: Vec<Subscription>,
    active_page: MonitorPage,
    charts_paused: bool,
    sample_index: usize,
    app_table: Entity<TableState<AppTableDelegate>>,
    process_table: Entity<TableState<ProcessTableDelegate>>,
    incidents: Vec<IncidentMarker>,
    store: MonitorStore,
    previous_block_counters: HashMap<String, BlockCounters>,
    last_disk_sample: Instant,
}

impl MonitorWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let settings = MonitorSettings::load();

        let monitor_target = cx.entity().downgrade();
        let app_settings = settings.clone();
        let app_table = cx.new(|cx| {
            TableState::new(
                AppTableDelegate::new(&app_settings, monitor_target.clone()),
                window,
                cx,
            )
            .col_selectable(false)
            .col_movable(false)
            .row_selectable(true)
        });
        let process_table = cx.new(|cx| {
            TableState::new(ProcessTableDelegate::new(&settings), window, cx)
                .col_selectable(false)
                .col_movable(false)
                .row_selectable(true)
        });
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search apps and processes…")
                .clean_on_escape()
        });

        let table_subscription =
            cx.subscribe(&process_table, |this, table, event, cx| match event {
                TableEvent::SelectRow(row) | TableEvent::DoubleClickedRow(row) => {
                    this.selected_pid = table
                        .read(cx)
                        .delegate()
                        .process_at(*row)
                        .map(|process| process.pid);
                    cx.notify();
                }
                TableEvent::ClearSelection => {
                    this.selected_pid = None;
                    cx.notify();
                }
                _ => {}
            });
        let search_subscription =
            cx.subscribe_in(&search_input, window, |this, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search_query = input.read(cx).value().to_string();
                    let query = this.search_query.clone();
                    let app_query = query.clone();
                    this.app_table.update(cx, |table, cx| {
                        table.delegate_mut().set_filter(app_query);
                        table.refresh(cx);
                        cx.notify();
                    });
                    this.process_table.update(cx, |table, cx| {
                        table.delegate_mut().set_filter(query);
                        table.refresh(cx);
                        cx.notify();
                    });
                    cx.notify();
                }
            });

        let cpu_details = linux::cpu_details(&system);
        let previous_block_counters = linux::block_counters();
        let mut monitor = Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            history: VecDeque::with_capacity(settings.clamped_graph_points()),
            frozen_history: Vec::new(),
            top_processes: Vec::new(),
            app_groups: Vec::new(),
            disk_info: Vec::new(),
            network_info: Vec::new(),
            cpu_details,
            gpus: linux::scan_gpus(),
            npus: linux::scan_npus(),
            batteries: linux::scan_batteries(),
            users: linux::users_by_id(),
            settings,
            search_input,
            search_query: String::new(),
            selected_pid: None,
            selected_gpu: 0,
            selected_npu: 0,
            selected_disk: 0,
            selected_network: 0,
            selected_battery: 0,
            last_action: None,
            _subscriptions: vec![table_subscription, search_subscription],
            active_page: MonitorPage::Overview,
            charts_paused: false,
            sample_index: 0,
            app_table,
            process_table,
            incidents: Vec::new(),
            store: MonitorStore::default(),
            previous_block_counters,
            last_disk_sample: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        };

        monitor.collect_metrics(cx);
        cx.spawn(async move |this, cx| {
            loop {
                let delay = match this.update(cx, |this, _| this.settings.refresh_interval()) {
                    Ok(delay) => delay,
                    Err(_) => break,
                };
                Timer::after(delay).await;
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

        let network_received_bytes = self
            .networks
            .values()
            .map(|data| data.received())
            .sum::<u64>();
        let network_transmitted_bytes = self
            .networks
            .values()
            .map(|data| data.transmitted())
            .sum::<u64>();
        let process_disk_read_bytes = self
            .system
            .processes()
            .values()
            .map(|process| process.disk_usage().read_bytes)
            .sum::<u64>();
        let process_disk_written_bytes = self
            .system
            .processes()
            .values()
            .map(|process| process.disk_usage().written_bytes)
            .sum::<u64>();

        let point = MetricPoint {
            time: format!("{}s", self.sample_index),
            cpu,
            memory,
            network_received: network_received_bytes as f64 / MIB,
            network_transmitted: network_transmitted_bytes as f64 / MIB,
            disk_read: process_disk_read_bytes as f64 / MIB,
            disk_written: process_disk_written_bytes as f64 / MIB,
        };
        if self.history.len() >= self.settings.clamped_graph_points() {
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

        let mut app_samples = Vec::new();
        let mut processes = self
            .system
            .processes()
            .iter()
            .map(|(pid, process)| {
                let disk = process.disk_usage();
                let extra = linux::process_extra(*pid, &self.users);
                let name = process.name().to_string_lossy().to_string();
                let swap = extra.swap_bytes.unwrap_or_default();
                app_samples.push(AppProcessSample {
                    pid: *pid,
                    name: name.clone(),
                    cpu_usage: process.cpu_usage(),
                    memory: process.memory(),
                    swap,
                    read_speed: disk.read_bytes,
                    read_total: disk.total_read_bytes,
                    write_speed: disk.written_bytes,
                    write_total: disk.total_written_bytes,
                    app_id: extra.app_id.clone(),
                    cgroup: extra.cgroup.clone(),
                });
                ProcessInfo {
                    pid: *pid,
                    parent_pid: extra.parent_pid,
                    name,
                    user: extra.user.unwrap_or_else(|| {
                        extra
                            .uid
                            .map_or_else(|| "N/A".to_string(), |uid| uid.to_string())
                    }),
                    state: format!("{:?}", process.status()),
                    cpu_usage: process.cpu_usage(),
                    memory: process.memory(),
                    swap,
                    read_speed: disk.read_bytes,
                    read_total: disk.total_read_bytes,
                    write_speed: disk.written_bytes,
                    write_total: disk.total_written_bytes,
                    total_cpu_time_ticks: extra.total_cpu_time_ticks,
                    user_cpu_time_ticks: extra.user_cpu_time_ticks,
                    system_cpu_time_ticks: extra.system_cpu_time_ticks,
                    priority: extra.priority,
                    nice: extra.nice,
                    threads: extra.threads,
                    file_descriptors: extra.file_descriptors,
                    command_line: extra.command_line,
                    executable: extra.executable,
                    working_directory: extra.working_directory,
                    cgroup: extra.cgroup,
                    app_id: extra.app_id,
                }
            })
            .collect::<Vec<_>>();
        self.app_groups = linux::group_apps(&app_samples);
        let app_groups = self.app_groups.clone();
        let app_query = self.search_query.clone();
        self.app_table.update(cx, |table, cx| {
            table.delegate_mut().set_groups(app_groups);
            table.delegate_mut().set_filter(app_query);
            table.refresh(cx);
            cx.notify();
        });
        processes.sort_by(|a, b| {
            b.cpu_usage
                .partial_cmp(&a.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.top_processes = processes.iter().take(8).cloned().collect();
        if self
            .selected_pid
            .is_some_and(|pid| !processes.iter().any(|process| process.pid == pid))
        {
            self.selected_pid = None;
        }
        let query = self.search_query.clone();
        self.process_table.update(cx, |table, cx| {
            table.delegate_mut().set_processes(processes);
            table.delegate_mut().set_filter(query);
            table.refresh(cx);
            cx.notify();
        });

        self.refresh_disk_info();
        self.network_info = self
            .networks
            .iter()
            .map(|(name, data)| NetworkInfo {
                name: name.clone(),
                received: data.received(),
                transmitted: data.transmitted(),
                total_received: data.total_received(),
                total_transmitted: data.total_transmitted(),
                metadata: linux::network_metadata(name),
            })
            .collect();
        self.network_info.sort_by(|a, b| a.name.cmp(&b.name));

        if self.sample_index % 2 == 0 {
            self.cpu_details = linux::cpu_details(&self.system);
            self.gpus = linux::scan_gpus();
            self.npus = linux::scan_npus();
            self.batteries = linux::scan_batteries();
            self.selected_gpu = self.selected_gpu.min(self.gpus.len().saturating_sub(1));
            self.selected_npu = self.selected_npu.min(self.npus.len().saturating_sub(1));
            self.selected_battery = self
                .selected_battery
                .min(self.batteries.len().saturating_sub(1));
        }
    }

    fn refresh_disk_info(&mut self) {
        let now = Instant::now();
        let elapsed = now
            .saturating_duration_since(self.last_disk_sample)
            .as_secs_f64()
            .max(0.001);
        let current_counters = linux::block_counters();
        self.disk_info = self
            .disks
            .iter()
            .map(|disk| {
                let device_name = disk.name().to_string_lossy().to_string();
                let metadata = linux::block_metadata(&device_name);
                let current = current_counters.get(&metadata.device).copied();
                let previous = self.previous_block_counters.get(&metadata.device).copied();
                let (activity_percent, read_speed, write_speed, total_read, total_written) =
                    match (current, previous) {
                        (Some(current), Some(previous)) => {
                            let read_sectors =
                                current.read_sectors.saturating_sub(previous.read_sectors);
                            let write_sectors =
                                current.write_sectors.saturating_sub(previous.write_sectors);
                            let io_ticks = current.io_ticks_ms.saturating_sub(previous.io_ticks_ms);
                            (
                                Some(
                                    (io_ticks as f64 / (elapsed * 1_000.0) * 100.0)
                                        .clamp(0.0, 100.0),
                                ),
                                (read_sectors.saturating_mul(SECTOR_SIZE) as f64 / elapsed) as u64,
                                (write_sectors.saturating_mul(SECTOR_SIZE) as f64 / elapsed) as u64,
                                current.read_sectors.saturating_mul(SECTOR_SIZE),
                                current.write_sectors.saturating_mul(SECTOR_SIZE),
                            )
                        }
                        (Some(current), None) => (
                            None,
                            0,
                            0,
                            current.read_sectors.saturating_mul(SECTOR_SIZE),
                            current.write_sectors.saturating_mul(SECTOR_SIZE),
                        ),
                        _ => (None, 0, 0, 0, 0),
                    };
                DiskInfo {
                    mount_point: disk.mount_point().to_string_lossy().to_string(),
                    file_system: disk.file_system().to_string_lossy().to_string(),
                    total: disk.total_space(),
                    available: disk.available_space(),
                    metadata,
                    activity_percent,
                    read_speed,
                    write_speed,
                    total_read,
                    total_written,
                }
            })
            .collect();
        self.previous_block_counters = current_counters;
        self.last_disk_sample = now;
        let visible_count = self
            .disk_info
            .iter()
            .filter(|disk| self.settings.show_virtual_drives || !disk.metadata.is_virtual)
            .count();
        self.selected_disk = self.selected_disk.min(visible_count.saturating_sub(1));
    }

    fn current_point(&self) -> MetricPoint {
        self.history.back().cloned().unwrap_or(MetricPoint {
            time: "0s".to_string(),
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
                        Button::new("overview-page")
                            .ghost()
                            .small()
                            .label("Overview")
                            .selected(self.active_page == MonitorPage::Overview)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_page = MonitorPage::Overview;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("pause-charts")
                            .outline()
                            .small()
                            .label(if self.charts_paused {
                                "Resume graphs"
                            } else {
                                "Pause graphs"
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
                                this.record_incident();
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
                "CPU {:.1}%  •  Memory {:.1}%  •  ↓ {}  ↑ {}",
                point.cpu,
                point.memory,
                linux::format_rate(
                    (point.network_received * MIB) as u64,
                    self.settings.network_bits,
                    self.settings.unit_base,
                ),
                linux::format_rate(
                    (point.network_transmitted * MIB) as u64,
                    self.settings.network_bits,
                    self.settings.unit_base,
                )
            ))
            .child(if self.charts_paused {
                "Graphs paused • collection continues".to_string()
            } else {
                format!("Live • {}", self.settings.refresh_speed.label())
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
            .bg(cx.theme().background)
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
            .bg(cx.theme().background)
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
                        0.0,
                        linear_color_stop(color.opacity(0.34), 1.0),
                        linear_color_stop(cx.theme().background.opacity(0.05), 0.0),
                    ))
                    .tick_margin(if self.settings.show_graph_grids {
                        16
                    } else {
                        0
                    }),
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
            .bg(cx.theme().background)
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
            .h(px(7.0))
            .rounded(px(99.0))
            .bg(cx.theme().muted)
            .overflow_hidden()
            .child(div().h_full().w(px(width)).rounded(px(99.0)).bg(color))
    }

    fn render_overview(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let history = self.chart_data();
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        self.metric_card(
                            "Processor",
                            format!("{:.1}%", point.cpu),
                            self.cpu_details
                                .model_name
                                .clone()
                                .unwrap_or_else(|| "N/A".to_string()),
                            cx.theme().blue,
                            cx,
                        ),
                    )
                    .child(self.metric_card(
                        "Memory",
                        format!("{:.1}%", point.memory),
                        format!(
                            "{} available",
                            linux::format_bytes(
                                self.system.available_memory(),
                                self.settings.unit_base
                            )
                        ),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Drive activity",
                        format!("{:.2} MiB/s", point.disk_read + point.disk_written),
                        format!("{} detected drives", self.disk_info.len()),
                        cx.theme().yellow,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Network",
                        linux::format_rate(
                            ((point.network_received + point.network_transmitted) * MIB) as u64,
                            self.settings.network_bits,
                            self.settings.unit_base,
                        ),
                        format!("{} interfaces", self.network_info.len()),
                        cx.theme().blue,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.chart_card(
                        "Processor usage",
                        format!("{:.1}%", point.cpu),
                        history.clone(),
                        |point| point.cpu,
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.chart_card(
                        "Memory usage",
                        format!("{:.1}%", point.memory),
                        history,
                        |point| point.memory,
                        cx.theme().green,
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
                                        .child(format!(
                                            "PID {} • {} • {}",
                                            process.pid, process.user, process.state
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().blue)
                                .child(format!("{:.1}%", process.cpu_usage)),
                        )
                        .child(
                            div()
                                .w(px(92.0))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(linux::format_bytes(
                                    process.memory,
                                    self.settings.unit_base,
                                )),
                        )
                })),
            cx,
        )
    }

    fn render_observation_health(&self, cx: &Context<Self>) -> Div {
        self.section_card(
            "Observation health",
            "Unavailable data stays unavailable rather than becoming a fake zero",
            v_flex()
                .gap_3()
                .child(self.health_row("CPU / memory / processes", "Active", cx.theme().green, cx))
                .child(self.health_row(
                    "cgroup application identity",
                    "Active with explicit fallback",
                    cx.theme().green,
                    cx,
                ))
                .child(self.health_row(
                    "GPU devices",
                    if self.gpus.is_empty() {
                        "Unavailable"
                    } else {
                        "Detected"
                    },
                    if self.gpus.is_empty() {
                        cx.theme().yellow
                    } else {
                        cx.theme().green
                    },
                    cx,
                ))
                .child(self.health_row(
                    "NPU devices",
                    if self.npus.is_empty() {
                        "Unavailable"
                    } else {
                        "Detected"
                    },
                    if self.npus.is_empty() {
                        cx.theme().yellow
                    } else {
                        cx.theme().green
                    },
                    cx,
                ))
                .child(self.health_row("Linux PSI", "Not connected", cx.theme().yellow, cx)),
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
                    .child(div().size_2().rounded(px(99.0)).bg(color))
                    .child(div().text_sm().child(name)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(state),
            )
    }

    fn render_cpu(&self, cx: &Context<Self>) -> Div {
        let point = self.current_point();
        let load = System::load_average();
        let total_view = v_flex()
            .gap_4()
            .child(self.chart_card(
                "Total Usage",
                format!("{:.1}%", point.cpu),
                self.chart_data(),
                |point| point.cpu,
                cx.theme().blue,
                cx,
            ))
            .child(
                self.section_card(
                    "Properties",
                    "Processor identity, topology, and platform capabilities",
                    v_flex()
                        .gap_2()
                        .child(
                            self.simple_property_row(
                                "Maximum Speed",
                                self.cpu_details.max_speed_mhz.map_or_else(
                                    || "N/A".to_string(),
                                    |value| format!("{value} MHz"),
                                ),
                                cx,
                            ),
                        )
                        .child(self.simple_property_row(
                            "Logical CPUs",
                            self.cpu_details.logical_cpus.to_string(),
                            cx,
                        ))
                        .child(
                            self.simple_property_row(
                                "Physical CPUs",
                                self.cpu_details
                                    .physical_cpus
                                    .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
                                cx,
                            ),
                        )
                        .child(
                            self.simple_property_row(
                                "Sockets",
                                self.cpu_details
                                    .sockets
                                    .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
                                cx,
                            ),
                        )
                        .child(self.simple_property_row(
                            "Uptime",
                            format_duration(System::uptime()),
                            cx,
                        ))
                        .child(
                            self.simple_property_row(
                                "Virtualization",
                                self.cpu_details
                                    .virtualization
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(self.simple_property_row(
                            "Architecture",
                            self.cpu_details.architecture.clone(),
                            cx,
                        )),
                    cx,
                ),
            );

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        self.metric_card(
                            "Processor",
                            self.cpu_details
                                .model_name
                                .clone()
                                .unwrap_or_else(|| "N/A".to_string()),
                            format!(
                                "Load {:.2} / {:.2} / {:.2}",
                                load.one, load.five, load.fifteen
                            ),
                            cx.theme().blue,
                            cx,
                        ),
                    )
                    .child(self.metric_card(
                        "Temperature",
                        self.cpu_details.temperature_c.map_or_else(
                            || "N/A".to_string(),
                            |value| {
                                linux::format_temperature(value, self.settings.temperature_unit)
                            },
                        ),
                        "Highest available thermal sensor".to_string(),
                        cx.theme().yellow,
                        cx,
                    )),
            )
            .when(!self.settings.show_logical_cpus, |this| {
                this.child(total_view)
            })
            .when(self.settings.show_logical_cpus, |this| {
                this.child(h_flex().flex_wrap().gap_3().children(
                    self.system.cpus().iter().enumerate().map(|(index, cpu)| {
                        v_flex()
                            .min_w(px(190.0))
                            .flex_1()
                            .gap_2()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .p_3()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(div().font_bold().child(format!("CPU {}", index + 1)))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("{} MHz", cpu.frequency())),
                                    ),
                            )
                            .child(self.utilization_bar(cpu.cpu_usage(), cx.theme().blue, cx))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{:.1}%", cpu.cpu_usage())),
                            )
                    }),
                ))
            })
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
                        "Memory",
                        format!(
                            "{} / {}",
                            linux::format_bytes(used, self.settings.unit_base),
                            linux::format_bytes(total, self.settings.unit_base)
                        ),
                        format!("{:.1}% used", point.memory),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Available",
                        linux::format_bytes(available, self.settings.unit_base),
                        "Available memory includes reclaimable cache".to_string(),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Swap",
                        if swap_total == 0 {
                            "N/A".to_string()
                        } else {
                            format!(
                                "{} / {}",
                                linux::format_bytes(swap_used, self.settings.unit_base),
                                linux::format_bytes(swap_total, self.settings.unit_base)
                            )
                        },
                        "System swap usage".to_string(),
                        cx.theme().yellow,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.chart_card(
                        "Memory",
                        format!("{:.1}%", point.memory),
                        self.chart_data(),
                        |point| point.memory,
                        cx.theme().green,
                        cx,
                    ))
                    .when(swap_total > 0, |this| {
                        let swap_percent = swap_used as f64 / swap_total as f64 * 100.0;
                        this.child(self.metric_card(
                            "Swap history",
                            format!("{swap_percent:.1}%"),
                            "Live swap graph requires a dedicated sampled series".to_string(),
                            cx.theme().yellow,
                            cx,
                        ))
                    }),
            )
            .child(self.section_card(
                "Memory properties",
                "Slots, speed, form factor, type, and type detail require DMI access",
                v_flex()
                    .gap_2()
                    .child(self.simple_property_row("Slots Used", "Permission required".to_string(), cx))
                    .child(self.simple_property_row("Speed", "Permission required".to_string(), cx))
                    .child(self.simple_property_row("Form Factor", "Permission required".to_string(), cx))
                    .child(self.simple_property_row("Type", "Permission required".to_string(), cx))
                    .child(self.simple_property_row("Type Detail", "Permission required".to_string(), cx))
                    .child(
                        div()
                            .mt_2()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().yellow)
                            .p_3()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "A narrow Polkit-reviewed DMI helper is still required. The GPUI app will not run dmidecode as root itself.",
                            ),
                    ),
                cx,
            ))
    }

    fn render_storage(&self, cx: &Context<Self>) -> Div {
        let disks = self
            .disk_info
            .iter()
            .filter(|disk| self.settings.show_virtual_drives || !disk.metadata.is_virtual)
            .collect::<Vec<_>>();
        let Some(disk) = disks.get(self.selected_disk).copied() else {
            return self.empty_hardware_page(
                "No drive is available",
                "Virtual drives may be hidden in Settings.",
                cx,
            );
        };
        let used = disk.total.saturating_sub(disk.available);
        let capacity_percent = if disk.total > 0 {
            used as f64 / disk.total as f64 * 100.0
        } else {
            0.0
        };
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        self.metric_card(
                            "Drive Activity",
                            disk.activity_percent
                                .map_or_else(|| "N/A".to_string(), |value| format!("{value:.0}%")),
                            disk.metadata
                                .model
                                .clone()
                                .unwrap_or_else(|| disk.metadata.device.clone()),
                            cx.theme().yellow,
                            cx,
                        ),
                    )
                    .child(self.metric_card(
                        "Read Speed",
                        linux::format_rate(disk.read_speed, false, self.settings.unit_base),
                        format!(
                            "Total {}",
                            linux::format_bytes(disk.total_read, self.settings.unit_base)
                        ),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Write Speed",
                        linux::format_rate(disk.write_speed, false, self.settings.unit_base),
                        format!(
                            "Total {}",
                            linux::format_bytes(disk.total_written, self.settings.unit_base)
                        ),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Capacity",
                        linux::format_bytes(disk.total, self.settings.unit_base),
                        format!("{capacity_percent:.1}% used at {}", disk.mount_point),
                        cx.theme().blue,
                        cx,
                    )),
            )
            .child(
                self.section_card(
                    "Properties",
                    "Drive identity, mount, and hardware characteristics",
                    v_flex()
                        .gap_2()
                        .child(self.simple_property_row(
                            "Drive Type",
                            disk.metadata.drive_type.clone(),
                            cx,
                        ))
                        .child(self.simple_property_row("Device", disk.metadata.device.clone(), cx))
                        .child(self.simple_property_row(
                            "Mount Point",
                            disk.mount_point.clone(),
                            cx,
                        ))
                        .child(self.simple_property_row("Filesystem", disk.file_system.clone(), cx))
                        .child(self.simple_property_row(
                            "Writable",
                            option_yes_no(disk.metadata.writable),
                            cx,
                        ))
                        .child(self.simple_property_row(
                            "Removable",
                            option_yes_no(disk.metadata.removable),
                            cx,
                        ))
                        .child(
                            self.simple_property_row(
                                "Link",
                                disk.metadata
                                    .link
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        ),
                    cx,
                ),
            )
    }

    fn render_network(&self, cx: &Context<Self>) -> Div {
        let interfaces = self
            .network_info
            .iter()
            .filter(|interface| {
                self.settings.show_virtual_network_interfaces || !interface.metadata.is_virtual
            })
            .collect::<Vec<_>>();
        let Some(interface) = interfaces.get(self.selected_network).copied() else {
            return self.empty_hardware_page(
                "No network interface is available",
                "Virtual interfaces may be hidden in Settings.",
                cx,
            );
        };
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "Receiving",
                        linux::format_rate(
                            interface.received,
                            self.settings.network_bits,
                            self.settings.unit_base,
                        ),
                        format!(
                            "Total {}",
                            linux::format_bytes(interface.total_received, self.settings.unit_base)
                        ),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Sending",
                        linux::format_rate(
                            interface.transmitted,
                            self.settings.network_bits,
                            self.settings.unit_base,
                        ),
                        format!(
                            "Total {}",
                            linux::format_bytes(
                                interface.total_transmitted,
                                self.settings.unit_base
                            )
                        ),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(
                        self.metric_card(
                            "Link Speed",
                            interface.metadata.link_speed_mbps.map_or_else(
                                || "N/A".to_string(),
                                |value| format!("{value} Mbit/s"),
                            ),
                            interface
                                .metadata
                                .state
                                .clone()
                                .unwrap_or_else(|| "N/A".to_string()),
                            cx.theme().yellow,
                            cx,
                        ),
                    ),
            )
            .child(
                self.section_card(
                    "Properties",
                    "Interface identity and connection metadata",
                    v_flex()
                        .gap_2()
                        .child(
                            self.simple_property_row(
                                "Manufacturer",
                                interface
                                    .metadata
                                    .manufacturer
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(
                            self.simple_property_row(
                                "Driver",
                                interface
                                    .metadata
                                    .driver
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(self.simple_property_row("Interface", interface.name.clone(), cx))
                        .child(
                            self.simple_property_row(
                                "Hardware Address",
                                interface
                                    .metadata
                                    .hardware_address
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(
                            self.simple_property_row(
                                "Network Name",
                                interface
                                    .metadata
                                    .network_name
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(
                            self.simple_property_row(
                                "Link",
                                interface
                                    .metadata
                                    .link
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        ),
                    cx,
                ),
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
                        "Current GUI session".to_string(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Graph window",
                        format!("{} points", self.history.len()),
                        format!("Maximum {}", self.settings.clamped_graph_points()),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Incident markers",
                        self.incidents.len().to_string(),
                        "Markers preserve sample positions".to_string(),
                        cx.theme().yellow,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.chart_card(
                        "Processor history",
                        format!("{:.1}%", point.cpu),
                        history.clone(),
                        |point| point.cpu,
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.chart_card(
                        "Memory history",
                        format!("{:.1}%", point.memory),
                        history,
                        |point| point.memory,
                        cx.theme().green,
                        cx,
                    )),
            )
            .child(self.section_card(
                "Persistence boundary",
                "Resources parity is live; Better Monitor history is an additional product layer",
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "A restart-safe service, time-series storage, downsampling, retention budgets, migrations, and recovery remain separate from this GUI parity slice.",
                    ),
                cx,
            ))
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
                    .bg(cx.theme().background)
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
                                        "Records the current sample position for the future before-and-after capture service.",
                                    ),
                            ),
                    )
                    .child(
                        Button::new("record-incident-page")
                            .warning()
                            .label("Record incident")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.record_incident();
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
                                .min_h(px(260.0))
                                .rounded(cx.theme().radius_lg)
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().background)
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
                            .bg(cx.theme().background)
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
                                                "Sample {} • unix {} ms",
                                                incident.sample_index, incident.recorded_at_ms
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().yellow)
                                    .child("Deep capture pending"),
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
                        "Healthy".to_string(),
                        self.settings.refresh_speed.label().to_string(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Processes",
                        self.system.processes().len().to_string(),
                        "sysinfo plus /proc enrichment".to_string(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Application groups",
                        self.app_groups.len().to_string(),
                        "cgroup identity with explicit fallback".to_string(),
                        cx.theme().green,
                        cx,
                    ))
                    .child(
                        self.metric_card(
                            "Dynamic device pages",
                            (self.gpus.len()
                                + self.npus.len()
                                + self.disk_info.len()
                                + self.network_info.len()
                                + self.batteries.len())
                            .to_string(),
                            "GPU, NPU, drive, network, and battery".to_string(),
                            cx.theme().blue,
                            cx,
                        ),
                    ),
            )
            .child(
                self.section_card(
                    "Collector matrix",
                    "Support state is part of every metric",
                    v_flex()
                        .gap_3()
                        .child(self.health_row(
                            "CPU / memory / process baseline",
                            "Active via sysinfo and /proc",
                            cx.theme().green,
                            cx,
                        ))
                        .child(self.health_row(
                            "Application grouping",
                            "Active via cgroup v2 with named fallback",
                            cx.theme().green,
                            cx,
                        ))
                        .child(self.health_row(
                            "GPU / NPU adapters",
                            "DRM, accel, and driver sysfs where exposed",
                            cx.theme().green,
                            cx,
                        ))
                        .child(self.health_row(
                            "Drive and network metadata",
                            "Active via sysfs and kernel counters",
                            cx.theme().green,
                            cx,
                        ))
                        .child(self.health_row(
                            "Memory hardware properties",
                            "Awaiting narrow Polkit DMI helper",
                            cx.theme().yellow,
                            cx,
                        ))
                        .child(self.health_row(
                            "CPU affinity and priority mutation",
                            "Not connected yet",
                            cx.theme().yellow,
                            cx,
                        ))
                        .child(self.health_row(
                            "Linux PSI and persistent history",
                            "Not connected yet",
                            cx.theme().yellow,
                            cx,
                        )),
                    cx,
                ),
            )
    }

    fn simple_property_row(&self, label: &'static str, value: String, cx: &Context<Self>) -> Div {
        h_flex()
            .items_start()
            .justify_between()
            .gap_5()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .w(px(190.0))
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().flex_1().text_sm().child(value))
    }

    fn empty_hardware_page(
        &self,
        title: &'static str,
        description: &'static str,
        cx: &Context<Self>,
    ) -> Div {
        v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .min_h(px(420.0))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(div().text_lg().font_bold().child(title))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
    }

    fn record_incident(&mut self) {
        let sequence = self.incidents.len() + 1;
        let recorded_at_ms = unix_time_ms();
        self.store.record_incident(Incident {
            timestamp_unix_ms: recorded_at_ms,
            title: format!("Slowdown marker #{sequence}"),
            note: Some(format!("Sample {}", self.sample_index)),
        });
        self.incidents.push(IncidentMarker {
            sequence,
            sample_index: self.sample_index,
            recorded_at_ms,
        });
    }

    fn render_page(&self, cx: &mut Context<Self>) -> Div {
        match self.active_page {
            MonitorPage::Overview => self.render_overview(cx),
            MonitorPage::Apps => self.render_apps_parity(cx),
            MonitorPage::Processes => self.render_processes_parity(cx),
            MonitorPage::Cpu => self.render_cpu(cx),
            MonitorPage::Memory => self.render_memory(cx),
            MonitorPage::Gpu => self.render_gpu_parity(cx),
            MonitorPage::Npu => self.render_npu_parity(cx),
            MonitorPage::Storage => self.render_storage(cx),
            MonitorPage::Network => self.render_network(cx),
            MonitorPage::Battery => self.render_battery_parity(cx),
            MonitorPage::History => self.render_history(cx),
            MonitorPage::Incidents => self.render_incidents(cx),
            MonitorPage::Diagnostics => self.render_diagnostics(cx),
            MonitorPage::Settings => self.render_settings_parity(cx),
        }
    }
}

impl Render for MonitorWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_resources_sidebar(cx))
            .child(
                v_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .child(self.render_header(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scrollbar()
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

fn option_yes_no(value: Option<bool>) -> String {
    value.map_or_else(
        || "N/A".to_string(),
        |value| if value { "Yes" } else { "No" }.to_string(),
    )
}

pub fn run() {
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1360.0), px(860.0)), cx)),
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
