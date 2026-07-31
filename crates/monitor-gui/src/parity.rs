use super::*;

use gpui_component::{Icon, IconName, input::Input, switch::Switch};
use sysinfo::Signal;

impl MonitorWindow {
    pub(super) fn render_resources_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let point = self.current_point();
        let memory_detail = linux::format_bytes(self.system.used_memory(), self.settings.unit_base);

        v_flex()
            .w(px(292.0))
            .h_full()
            .flex_shrink_0()
            .gap_2()
            .p_3()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py_2()
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(div().font_bold().text_lg().child("Better Monitor"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("GNOME Resources parity mode"),
                            ),
                    )
                    .child(
                        Button::new("sidebar-settings")
                            .ghost()
                            .small()
                            .label("Settings")
                            .selected(self.active_page == MonitorPage::Settings)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_page = MonitorPage::Settings;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().flex_1().min_h(px(0.0)).overflow_y_scrollbar().child(
                    v_flex()
                        .gap_1()
                        .child(self.sidebar_group_label("Applications", cx))
                        .child(self.sidebar_resource_row(
                            "sidebar-apps",
                            "Apps".to_string(),
                            format!("{} grouped apps", self.app_groups.len()),
                            self.app_groups.first().map(|app| app.cpu_usage as f64),
                            self.active_page == MonitorPage::Apps,
                            cx.listener(|this, _, _, cx| {
                                this.active_page = MonitorPage::Apps;
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.sidebar_resource_row(
                            "sidebar-processes",
                            "Processes".to_string(),
                            format!("{} running", self.system.processes().len()),
                            Some(point.cpu),
                            self.active_page == MonitorPage::Processes,
                            cx.listener(|this, _, _, cx| {
                                this.active_page = MonitorPage::Processes;
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.sidebar_group_label("System", cx))
                        .child(
                            self.sidebar_resource_row(
                                "sidebar-cpu",
                                "Processor".to_string(),
                                self.cpu_details
                                    .model_name
                                    .clone()
                                    .unwrap_or_else(|| "CPU".to_string()),
                                Some(point.cpu),
                                self.active_page == MonitorPage::Cpu,
                                cx.listener(|this, _, _, cx| {
                                    this.active_page = MonitorPage::Cpu;
                                    cx.notify();
                                }),
                                cx,
                            ),
                        )
                        .child(self.sidebar_resource_row(
                            "sidebar-memory",
                            "Memory".to_string(),
                            memory_detail,
                            Some(point.memory),
                            self.active_page == MonitorPage::Memory,
                            cx.listener(|this, _, _, cx| {
                                this.active_page = MonitorPage::Memory;
                                cx.notify();
                            }),
                            cx,
                        ))
                        .children(self.gpus.iter().enumerate().map(|(index, gpu)| {
                            self.sidebar_resource_row(
                                ("sidebar-gpu", index),
                                "GPU".to_string(),
                                gpu.name.clone(),
                                gpu.usage_percent,
                                self.active_page == MonitorPage::Gpu && self.selected_gpu == index,
                                cx.listener(move |this, _, _, cx| {
                                    this.selected_gpu = index;
                                    this.active_page = MonitorPage::Gpu;
                                    cx.notify();
                                }),
                                cx,
                            )
                        }))
                        .children(self.npus.iter().enumerate().map(|(index, npu)| {
                            self.sidebar_resource_row(
                                ("sidebar-npu", index),
                                "NPU".to_string(),
                                npu.name.clone(),
                                npu.usage_percent,
                                self.active_page == MonitorPage::Npu && self.selected_npu == index,
                                cx.listener(move |this, _, _, cx| {
                                    this.selected_npu = index;
                                    this.active_page = MonitorPage::Npu;
                                    cx.notify();
                                }),
                                cx,
                            )
                        }))
                        .children(self.visible_disks().enumerate().map(|(index, disk)| {
                            let used = disk.total.saturating_sub(disk.available);
                            let usage =
                                (disk.total > 0).then_some(used as f64 / disk.total as f64 * 100.0);
                            self.sidebar_resource_row(
                                ("sidebar-drive", index),
                                "Drive".to_string(),
                                disk.metadata
                                    .model
                                    .clone()
                                    .unwrap_or_else(|| disk.metadata.device.clone()),
                                usage,
                                self.active_page == MonitorPage::Storage
                                    && self.selected_disk == index,
                                cx.listener(move |this, _, _, cx| {
                                    this.selected_disk = index;
                                    this.active_page = MonitorPage::Storage;
                                    cx.notify();
                                }),
                                cx,
                            )
                        }))
                        .children(
                            self.visible_networks()
                                .enumerate()
                                .map(|(index, interface)| {
                                    let usage = interface
                                        .metadata
                                        .link_speed_mbps
                                        .filter(|speed| *speed > 0)
                                        .map(|speed| {
                                            let bytes_per_second = speed as f64 * 1_000_000.0 / 8.0;
                                            ((interface.received + interface.transmitted) as f64
                                                / bytes_per_second
                                                * 100.0)
                                                .clamp(0.0, 100.0)
                                        });
                                    self.sidebar_resource_row(
                                        ("sidebar-network", index),
                                        interface.metadata.interface_type.clone(),
                                        interface.name.clone(),
                                        usage,
                                        self.active_page == MonitorPage::Network
                                            && self.selected_network == index,
                                        cx.listener(move |this, _, _, cx| {
                                            this.selected_network = index;
                                            this.active_page = MonitorPage::Network;
                                            cx.notify();
                                        }),
                                        cx,
                                    )
                                }),
                        )
                        .children(self.batteries.iter().enumerate().map(|(index, battery)| {
                            self.sidebar_resource_row(
                                ("sidebar-battery", index),
                                "Battery".to_string(),
                                battery.name.clone(),
                                battery.charge_percent,
                                self.active_page == MonitorPage::Battery
                                    && self.selected_battery == index,
                                cx.listener(move |this, _, _, cx| {
                                    this.selected_battery = index;
                                    this.active_page = MonitorPage::Battery;
                                    cx.notify();
                                }),
                                cx,
                            )
                        }))
                        .child(self.sidebar_group_label("Better Monitor", cx))
                        .children(
                            MonitorPage::INVESTIGATE
                                .into_iter()
                                .map(|page| self.nav_button(page, cx)),
                        ),
                ),
            )
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
                            .child(div().size_2().rounded(px(99.0)).bg(cx.theme().green))
                            .child(div().text_sm().font_bold().child("Recording")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} samples · {}",
                                self.store.samples().len(),
                                self.settings.refresh_speed.label()
                            )),
                    ),
            )
    }

    fn sidebar_group_label(&self, label: &'static str, cx: &Context<Self>) -> Div {
        div()
            .mt_3()
            .px_2()
            .py_1()
            .text_xs()
            .font_bold()
            .text_color(cx.theme().muted_foreground)
            .child(label)
    }

    fn sidebar_resource_row(
        &self,
        id: impl Into<ElementId>,
        label: String,
        detail: String,
        usage: Option<f64>,
        selected: bool,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &Context<Self>,
    ) -> Div {
        div().child(
            Button::new(id)
                .ghost()
                .w_full()
                .selected(selected)
                .on_click(listener)
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_3()
                        .py_1()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().font_bold().truncate().child(label))
                                .when(self.settings.sidebar_description, |this| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(detail),
                                    )
                                }),
                        )
                        .when(self.settings.sidebar_details, |this| {
                            this.child(self.sidebar_meter(usage, cx))
                        }),
                ),
        )
    }

    fn sidebar_meter(&self, usage: Option<f64>, cx: &Context<Self>) -> Div {
        let usage = usage.unwrap_or_default().clamp(0.0, 100.0);
        match self.settings.sidebar_meter_type {
            SidebarMeterType::ProgressBar => v_flex()
                .w(px(74.0))
                .gap_1()
                .items_end()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{usage:.0}%")),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(5.0))
                        .rounded(px(99.0))
                        .bg(cx.theme().border)
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(px((usage as f32 * 0.72).max(1.0)))
                                .rounded(px(99.0))
                                .bg(cx.theme().blue),
                        ),
                ),
            SidebarMeterType::Graph => v_flex()
                .w(px(74.0))
                .items_end()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().blue)
                        .child(sparkline_for_usage(usage)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{usage:.0}%")),
                ),
        }
    }

    pub(super) fn render_apps_parity(&self, cx: &mut Context<Self>) -> Div {
        let query = self.search_query.trim().to_lowercase();
        let groups = self
            .app_groups
            .iter()
            .filter(|group| {
                query.is_empty()
                    || group.display_name.to_lowercase().contains(&query)
                    || group.id.to_lowercase().contains(&query)
                    || group.grouping_reason.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();

        v_flex()
            .gap_4()
            .child(self.render_search_toolbar("Search apps…", cx))
            .child(
                div()
                    .overflow_x_scrollbar()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(
                        v_flex()
                            .min_w(px(self.app_table_width()))
                            .overflow_hidden()
                            .child(self.render_app_header(cx))
                            .when(groups.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .items_center()
                                        .justify_center()
                                        .min_h(px(260.0))
                                        .child(
                                            div().font_bold().child("No matching applications"),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(
                                                    "Clear the search or wait for application groups.",
                                                ),
                                        ),
                                )
                            })
                            .children(groups.into_iter().enumerate().map(|(index, group)| {
                                self.render_app_row(index, group, cx)
                            })),
                    ),
            )
            .when_some(self.last_action.clone(), |this, message| {
                this.child(self.action_result_banner(message, cx))
            })
    }

    fn app_table_width(&self) -> f32 {
        let columns = &self.settings.app_columns;
        250.0
            + if columns.memory { 110.0 } else { 0.0 }
            + if columns.cpu { 86.0 } else { 0.0 }
            + if columns.read_speed { 108.0 } else { 0.0 }
            + if columns.read_total { 116.0 } else { 0.0 }
            + if columns.write_speed { 108.0 } else { 0.0 }
            + if columns.write_total { 116.0 } else { 0.0 }
            + if columns.gpu { 86.0 } else { 0.0 }
            + if columns.gpu_memory { 116.0 } else { 0.0 }
            + if columns.encoder { 96.0 } else { 0.0 }
            + if columns.decoder { 96.0 } else { 0.0 }
            + if columns.swap { 110.0 } else { 0.0 }
            + if columns.combined_memory { 130.0 } else { 0.0 }
            + 270.0
    }

    fn render_app_header(&self, cx: &Context<Self>) -> Div {
        let columns = &self.settings.app_columns;
        h_flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.app_header_cell("App", 250.0))
            .when(columns.memory, |this| {
                this.child(self.app_header_cell("Memory", 110.0))
            })
            .when(columns.cpu, |this| {
                this.child(self.app_header_cell("CPU", 86.0))
            })
            .when(columns.read_speed, |this| {
                this.child(self.app_header_cell("Read/s", 108.0))
            })
            .when(columns.read_total, |this| {
                this.child(self.app_header_cell("Read total", 116.0))
            })
            .when(columns.write_speed, |this| {
                this.child(self.app_header_cell("Write/s", 108.0))
            })
            .when(columns.write_total, |this| {
                this.child(self.app_header_cell("Write total", 116.0))
            })
            .when(columns.gpu, |this| {
                this.child(self.app_header_cell("GPU", 86.0))
            })
            .when(columns.gpu_memory, |this| {
                this.child(self.app_header_cell("GPU memory", 116.0))
            })
            .when(columns.encoder, |this| {
                this.child(self.app_header_cell("Encoder", 96.0))
            })
            .when(columns.decoder, |this| {
                this.child(self.app_header_cell("Decoder", 96.0))
            })
            .when(columns.swap, |this| {
                this.child(self.app_header_cell("Swap", 110.0))
            })
            .when(columns.combined_memory, |this| {
                this.child(self.app_header_cell("Memory + swap", 130.0))
            })
            .child(self.app_header_cell("Actions", 270.0))
    }

    fn app_header_cell(&self, label: &'static str, width: f32) -> Div {
        div().w(px(width)).flex_shrink_0().font_bold().child(label)
    }

    fn render_app_row(&self, index: usize, group: &AppGroup, cx: &mut Context<Self>) -> Div {
        let columns = &self.settings.app_columns;
        let term_pids = group.pids.clone();
        let kill_pids = group.pids.clone();
        let stop_pids = group.pids.clone();
        let continue_pids = group.pids.clone();

        h_flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .w(px(250.0))
                    .flex_shrink_0()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_bold()
                            .truncate()
                            .child(group.display_name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} processes · {}",
                                group.process_count, group.grouping_reason
                            )),
                    ),
            )
            .when(columns.memory, |this| {
                this.child(self.app_value_cell(
                    linux::format_bytes(group.memory, self.settings.unit_base),
                    110.0,
                    cx.theme().foreground,
                ))
            })
            .when(columns.cpu, |this| {
                this.child(self.app_value_cell(
                    format!("{:.1}%", group.cpu_usage),
                    86.0,
                    cx.theme().blue,
                ))
            })
            .when(columns.read_speed, |this| {
                this.child(self.app_value_cell(
                    linux::format_rate(group.read_speed, false, self.settings.unit_base),
                    108.0,
                    cx.theme().foreground,
                ))
            })
            .when(columns.read_total, |this| {
                this.child(self.app_value_cell(
                    linux::format_bytes(group.read_total, self.settings.unit_base),
                    116.0,
                    cx.theme().foreground,
                ))
            })
            .when(columns.write_speed, |this| {
                this.child(self.app_value_cell(
                    linux::format_rate(group.write_speed, false, self.settings.unit_base),
                    108.0,
                    cx.theme().foreground,
                ))
            })
            .when(columns.write_total, |this| {
                this.child(self.app_value_cell(
                    linux::format_bytes(group.write_total, self.settings.unit_base),
                    116.0,
                    cx.theme().foreground,
                ))
            })
            .when(columns.gpu, |this| {
                this.child(self.app_value_cell(
                    "N/A".to_string(),
                    86.0,
                    cx.theme().muted_foreground,
                ))
            })
            .when(columns.gpu_memory, |this| {
                this.child(self.app_value_cell(
                    "N/A".to_string(),
                    116.0,
                    cx.theme().muted_foreground,
                ))
            })
            .when(columns.encoder, |this| {
                this.child(self.app_value_cell(
                    "N/A".to_string(),
                    96.0,
                    cx.theme().muted_foreground,
                ))
            })
            .when(columns.decoder, |this| {
                this.child(self.app_value_cell(
                    "N/A".to_string(),
                    96.0,
                    cx.theme().muted_foreground,
                ))
            })
            .when(columns.swap, |this| {
                this.child(self.app_value_cell(
                    linux::format_bytes(group.swap, self.settings.unit_base),
                    110.0,
                    cx.theme().foreground,
                ))
            })
            .when(columns.combined_memory, |this| {
                this.child(self.app_value_cell(
                    linux::format_bytes(
                        group.memory.saturating_add(group.swap),
                        self.settings.unit_base,
                    ),
                    130.0,
                    cx.theme().foreground,
                ))
            })
            .child(
                h_flex()
                    .w(px(270.0))
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        Button::new(("app-term", index))
                            .outline()
                            .small()
                            .label("End")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.signal_pids(&term_pids, Signal::Term);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("app-kill", index))
                            .warning()
                            .small()
                            .label("Force")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.signal_pids(&kill_pids, Signal::Kill);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("app-stop", index))
                            .ghost()
                            .small()
                            .label("Pause")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.signal_pids(&stop_pids, Signal::Stop);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("app-cont", index))
                            .ghost()
                            .small()
                            .label("Resume")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.signal_pids(&continue_pids, Signal::Continue);
                                cx.notify();
                            })),
                    ),
            )
    }

    fn app_value_cell(&self, value: String, width: f32, color: Hsla) -> Div {
        div()
            .w(px(width))
            .flex_shrink_0()
            .text_sm()
            .text_color(color)
            .truncate()
            .child(value)
    }

    pub(super) fn render_processes_parity(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.selected_process(cx);
        let visible_count = self.process_table.read(cx).delegate().processes.len();

        v_flex()
            .gap_4()
            .child(
                self.render_search_toolbar("Search processes… Use | to require multiple terms", cx),
            )
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .p_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(match selected.as_ref() {
                                Some(process) => format!(
                                    "Selected: {} · PID {} · {}",
                                    process.name, process.pid, process.user
                                ),
                                None => "Select a process row to enable actions".to_string(),
                            }),
                    )
                    .child(
                        self.process_action_buttons(
                            selected.as_ref().map(|process| process.pid),
                            cx,
                        ),
                    ),
            )
            .child(
                v_flex()
                    .min_h(px(520.0))
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
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(div().font_bold().child("Processes"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{visible_count} visible · {} total",
                                        self.system.processes().len()
                                    )),
                            ),
                    )
                    .child(
                        div().flex_1().child(
                            DataTable::new(&self.process_table)
                                .bordered(false)
                                .stripe(true)
                                .small(),
                        ),
                    ),
            )
            .when_some(selected, |this, process| {
                this.child(self.render_process_details(&process, cx))
            })
            .when_some(self.last_action.clone(), |this, message| {
                this.child(self.action_result_banner(message, cx))
            })
    }

    fn render_search_toolbar(&self, hint: &'static str, cx: &Context<Self>) -> Div {
        h_flex()
            .items_center()
            .gap_3()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .p_3()
            .child(
                div().flex_1().child(
                    Input::new(&self.search_input)
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).small()),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
    }

    fn process_action_buttons(&self, pid: Option<Pid>, cx: &mut Context<Self>) -> Div {
        let enabled = pid.is_some();
        h_flex()
            .gap_1()
            .child(
                Button::new("process-term")
                    .outline()
                    .small()
                    .disabled(!enabled)
                    .label("End")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(pid) = pid {
                            this.signal_pids(&[pid], Signal::Term);
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new("process-kill")
                    .warning()
                    .small()
                    .disabled(!enabled)
                    .label("Force stop")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(pid) = pid {
                            this.signal_pids(&[pid], Signal::Kill);
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new("process-stop")
                    .ghost()
                    .small()
                    .disabled(!enabled)
                    .label("Pause")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(pid) = pid {
                            this.signal_pids(&[pid], Signal::Stop);
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new("process-continue")
                    .ghost()
                    .small()
                    .disabled(!enabled)
                    .label("Resume")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(pid) = pid {
                            this.signal_pids(&[pid], Signal::Continue);
                            cx.notify();
                        }
                    })),
            )
    }

    fn render_process_details(&self, process: &ProcessInfo, cx: &Context<Self>) -> Div {
        self.section_card(
            "Process information",
            "Resources-style details with explicit support states",
            v_flex()
                .gap_2()
                .child(self.property_row("Name", process.name.clone(), cx))
                .child(self.property_row("PID", process.pid.to_string(), cx))
                .child(
                    self.property_row(
                        "Parent PID",
                        process
                            .parent_pid
                            .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
                        cx,
                    ),
                )
                .child(self.property_row("User", process.user.clone(), cx))
                .child(self.property_row("State", process.state.clone(), cx))
                .child(
                    self.property_row(
                        "Threads",
                        process
                            .threads
                            .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
                        cx,
                    ),
                )
                .child(
                    self.property_row(
                        "File descriptors",
                        process
                            .file_descriptors
                            .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
                        cx,
                    ),
                )
                .child(self.property_row(
                    "Priority / nice",
                    match (process.priority, process.nice) {
                        (Some(priority), Some(nice)) => format!("{priority} / {nice:+}"),
                        _ => "N/A".to_string(),
                    },
                    cx,
                ))
                .child(self.property_row(
                    "Application identity",
                    process.app_id.clone().unwrap_or_else(|| "N/A".to_string()),
                    cx,
                ))
                .child(self.property_row(
                    "Cgroup",
                    process.cgroup.clone().unwrap_or_else(|| "N/A".to_string()),
                    cx,
                ))
                .child(
                    self.property_row(
                        "Executable",
                        process
                            .executable
                            .clone()
                            .unwrap_or_else(|| "N/A".to_string()),
                        cx,
                    ),
                )
                .child(
                    self.property_row(
                        "Working directory",
                        process
                            .working_directory
                            .clone()
                            .unwrap_or_else(|| "N/A".to_string()),
                        cx,
                    ),
                )
                .child(self.property_row(
                    "Command line",
                    if process.command_line.is_empty() {
                        "N/A".to_string()
                    } else {
                        process.command_line.clone()
                    },
                    cx,
                )),
            cx,
        )
    }

    pub(super) fn render_gpu_parity(&self, cx: &Context<Self>) -> Div {
        let Some(gpu) = self.gpus.get(self.selected_gpu) else {
            return self.unavailable_page(
                "No supported GPU was detected",
                "Better Monitor scans DRM/sysfs adapters. Missing metrics remain unavailable.",
                cx,
            );
        };
        let memory = match (gpu.memory_used, gpu.memory_total) {
            (Some(used), Some(total)) => format!(
                "{} / {}",
                linux::format_bytes(used, self.settings.unit_base),
                linux::format_bytes(total, self.settings.unit_base)
            ),
            (Some(used), None) => linux::format_bytes(used, self.settings.unit_base),
            _ => "N/A".to_string(),
        };

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "Total Usage",
                        option_percent(gpu.usage_percent),
                        gpu.name.clone(),
                        cx.theme().red,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Video Memory Usage",
                        memory,
                        "Dedicated memory when exposed by the driver".to_string(),
                        cx.theme().red,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Temperature",
                        gpu.temperature_c.map_or_else(
                            || "N/A".to_string(),
                            |value| {
                                linux::format_temperature(value, self.settings.temperature_unit)
                            },
                        ),
                        "Highest current hwmon sensor".to_string(),
                        cx.theme().yellow,
                        cx,
                    ))
                    .child(
                        self.metric_card(
                            "Power Usage",
                            gpu.power_watts
                                .map_or_else(|| "N/A".to_string(), |value| format!("{value:.1} W")),
                            gpu.max_power_watts.map_or_else(
                                || "No verified power cap".to_string(),
                                |value| format!("Maximum {value:.1} W"),
                            ),
                            cx.theme().green,
                            cx,
                        ),
                    ),
            )
            .child(
                self.section_card(
                    "Media engines",
                    "Encoder and decoder activity appear only when the driver exposes them",
                    v_flex()
                        .gap_2()
                        .child(self.property_row(
                            "Video Encoder Usage",
                            option_percent(gpu.encode_percent),
                            cx,
                        ))
                        .child(self.property_row(
                            "Video Decoder Usage",
                            option_percent(gpu.decode_percent),
                            cx,
                        )),
                    cx,
                ),
            )
            .child(
                self.section_card(
                    "Properties",
                    "Hardware identity and driver metadata",
                    v_flex()
                        .gap_2()
                        .child(self.property_row("Manufacturer", gpu.manufacturer.clone(), cx))
                        .child(self.property_row("PCI Slot", gpu.pci_slot.clone(), cx))
                        .child(self.property_row("Driver Used", gpu.driver.clone(), cx))
                        .child(self.property_row(
                            "GPU Clock Speed",
                            option_mhz(gpu.gpu_clock_mhz),
                            cx,
                        ))
                        .child(self.property_row(
                            "Video Memory Clock Speed",
                            option_mhz(gpu.memory_clock_mhz),
                            cx,
                        ))
                        .child(
                            self.property_row(
                                "Maximum Power Cap",
                                gpu.max_power_watts.map_or_else(
                                    || "N/A".to_string(),
                                    |value| format!("{value:.1} W"),
                                ),
                                cx,
                            ),
                        )
                        .child(self.property_row(
                            "Link",
                            gpu.link.clone().unwrap_or_else(|| "N/A".to_string()),
                            cx,
                        )),
                    cx,
                ),
            )
    }

    pub(super) fn render_npu_parity(&self, cx: &Context<Self>) -> Div {
        let Some(npu) = self.npus.get(self.selected_npu) else {
            return self.unavailable_page(
                "No supported NPU was detected",
                "Better Monitor scans /sys/class/accel and known NPU/VPU devices.",
                cx,
            );
        };
        let memory = match (npu.memory_used, npu.memory_total) {
            (Some(used), Some(total)) => format!(
                "{} / {}",
                linux::format_bytes(used, self.settings.unit_base),
                linux::format_bytes(total, self.settings.unit_base)
            ),
            (Some(used), None) => linux::format_bytes(used, self.settings.unit_base),
            _ => "N/A".to_string(),
        };

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(self.metric_card(
                        "Total Usage",
                        option_percent(npu.usage_percent),
                        npu.name.clone(),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Memory Usage",
                        memory,
                        "NPU-local memory when exposed".to_string(),
                        cx.theme().blue,
                        cx,
                    ))
                    .child(self.metric_card(
                        "Temperature",
                        npu.temperature_c.map_or_else(
                            || "N/A".to_string(),
                            |value| {
                                linux::format_temperature(value, self.settings.temperature_unit)
                            },
                        ),
                        "Driver sensor".to_string(),
                        cx.theme().yellow,
                        cx,
                    ))
                    .child(
                        self.metric_card(
                            "Power Usage",
                            npu.power_watts
                                .map_or_else(|| "N/A".to_string(), |value| format!("{value:.1} W")),
                            npu.max_power_watts.map_or_else(
                                || "No verified power cap".to_string(),
                                |value| format!("Maximum {value:.1} W"),
                            ),
                            cx.theme().green,
                            cx,
                        ),
                    ),
            )
            .child(
                self.section_card(
                    "Properties",
                    "Hardware identity and driver metadata",
                    v_flex()
                        .gap_2()
                        .child(self.property_row("Manufacturer", npu.manufacturer.clone(), cx))
                        .child(self.property_row("PCI Slot", npu.pci_slot.clone(), cx))
                        .child(self.property_row("Driver Used", npu.driver.clone(), cx))
                        .child(self.property_row("NPU Clock Speed", option_mhz(npu.clock_mhz), cx))
                        .child(self.property_row(
                            "Memory Clock Speed",
                            option_mhz(npu.memory_clock_mhz),
                            cx,
                        ))
                        .child(
                            self.property_row(
                                "Maximum Power Cap",
                                npu.max_power_watts.map_or_else(
                                    || "N/A".to_string(),
                                    |value| format!("{value:.1} W"),
                                ),
                                cx,
                            ),
                        )
                        .child(self.property_row(
                            "Link",
                            npu.link.clone().unwrap_or_else(|| "N/A".to_string()),
                            cx,
                        )),
                    cx,
                ),
            )
    }

    pub(super) fn render_battery_parity(&self, cx: &Context<Self>) -> Div {
        let Some(battery) = self.batteries.get(self.selected_battery) else {
            return self.unavailable_page(
                "No battery was detected",
                "Battery pages appear dynamically for power-supply devices with type Battery.",
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
                        "Battery Charge",
                        option_percent(battery.charge_percent),
                        battery.state.clone().unwrap_or_else(|| "N/A".to_string()),
                        cx.theme().green,
                        cx,
                    ))
                    .child(
                        self.metric_card(
                            "Power Usage",
                            battery
                                .power_watts
                                .map_or_else(|| "N/A".to_string(), |value| format!("{value:.2} W")),
                            "Current charge or discharge rate".to_string(),
                            cx.theme().green,
                            cx,
                        ),
                    )
                    .child(self.metric_card(
                        "Health",
                        option_percent(battery.health_percent),
                        "Full capacity compared with design capacity".to_string(),
                        cx.theme().blue,
                        cx,
                    )),
            )
            .child(
                self.section_card(
                    "Properties",
                    "Power-supply identity and lifetime information",
                    v_flex()
                        .gap_2()
                        .child(self.property_row(
                            "Design Capacity",
                            battery.design_capacity_wh.map_or_else(
                                || "N/A".to_string(),
                                |value| format!("{value:.1} Wh"),
                            ),
                            cx,
                        ))
                        .child(
                            self.property_row(
                                "Charge Cycles",
                                battery
                                    .charge_cycles
                                    .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
                                cx,
                            ),
                        )
                        .child(
                            self.property_row(
                                "Technology",
                                battery
                                    .technology
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(
                            self.property_row(
                                "Manufacturer",
                                battery
                                    .manufacturer
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(
                            self.property_row(
                                "Model Name",
                                battery
                                    .model_name
                                    .clone()
                                    .unwrap_or_else(|| "N/A".to_string()),
                                cx,
                            ),
                        )
                        .child(self.property_row("Device", battery.device.clone(), cx)),
                    cx,
                ),
            )
    }

    pub(super) fn render_settings_parity(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_4()
            .child(
                self.section_card(
                    "General",
                    "Settings compatible with GNOME Resources v1.10.2 behavior",
                    v_flex()
                        .gap_2()
                        .child(
                            self.setting_row(
                                "Refresh speed",
                                "Controls live collector and UI refresh cadence",
                                Button::new("setting-refresh-speed")
                                    .outline()
                                    .small()
                                    .label(self.settings.refresh_speed.label())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.refresh_speed =
                                            match this.settings.refresh_speed {
                                                RefreshSpeed::VerySlow => RefreshSpeed::Slow,
                                                RefreshSpeed::Slow => RefreshSpeed::Normal,
                                                RefreshSpeed::Normal => RefreshSpeed::Fast,
                                                RefreshSpeed::Fast => RefreshSpeed::VeryFast,
                                                RefreshSpeed::VeryFast => RefreshSpeed::VerySlow,
                                            };
                                        this.persist_settings();
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        )
                        .child(
                            self.setting_row(
                                "Data units",
                                "Choose decimal or binary storage units",
                                Button::new("setting-unit-base")
                                    .outline()
                                    .small()
                                    .label(self.settings.unit_base.label())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.unit_base = match this.settings.unit_base {
                                            UnitBase::Decimal => UnitBase::Binary,
                                            UnitBase::Binary => UnitBase::Decimal,
                                        };
                                        this.sync_table_settings(cx);
                                    })),
                                cx,
                            ),
                        )
                        .child(
                            self.setting_row(
                                "Temperature unit",
                                "Used by CPU, GPU, NPU, and thermal pages",
                                Button::new("setting-temperature-unit")
                                    .outline()
                                    .small()
                                    .label(self.settings.temperature_unit.label())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.temperature_unit = match this
                                            .settings
                                            .temperature_unit
                                        {
                                            TemperatureUnit::Celsius => TemperatureUnit::Fahrenheit,
                                            TemperatureUnit::Fahrenheit => TemperatureUnit::Kelvin,
                                            TemperatureUnit::Kelvin => TemperatureUnit::Celsius,
                                        };
                                        this.persist_settings();
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        )
                        .child(
                            self.setting_row(
                                "Graph data points",
                                "Recent values retained by each live graph",
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("graph-points-minus")
                                            .outline()
                                            .small()
                                            .label("−")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.settings.graph_data_points = this
                                                    .settings
                                                    .graph_data_points
                                                    .saturating_sub(30)
                                                    .max(30);
                                                this.trim_history();
                                                this.persist_settings();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div().w(px(70.0)).text_center().child(
                                            self.settings.clamped_graph_points().to_string(),
                                        ),
                                    )
                                    .child(
                                        Button::new("graph-points-plus")
                                            .outline()
                                            .small()
                                            .label("+")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.settings.graph_data_points =
                                                    (this.settings.graph_data_points + 30).min(600);
                                                this.persist_settings();
                                                cx.notify();
                                            })),
                                    ),
                                cx,
                            ),
                        ),
                    cx,
                ),
            )
            .child(
                self.section_card(
                    "Sidebar and graphs",
                    "Resources-style navigation density and graph controls",
                    v_flex()
                        .gap_2()
                        .child(
                            self.setting_row(
                                "Sidebar meter",
                                "Display progress bars or compact mini graphs",
                                Button::new("setting-sidebar-meter")
                                    .outline()
                                    .small()
                                    .label(self.settings.sidebar_meter_type.label())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.sidebar_meter_type =
                                            match this.settings.sidebar_meter_type {
                                                SidebarMeterType::ProgressBar => {
                                                    SidebarMeterType::Graph
                                                }
                                                SidebarMeterType::Graph => {
                                                    SidebarMeterType::ProgressBar
                                                }
                                            };
                                        this.persist_settings();
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        )
                        .child(self.setting_switch(
                            "setting-sidebar-details",
                            "Sidebar details",
                            "Show a live meter and current value",
                            self.settings.sidebar_details,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.sidebar_details = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-sidebar-description",
                            "Sidebar descriptions",
                            "Show device model or interface name",
                            self.settings.sidebar_description,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.sidebar_description = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-logical-cpus",
                            "Show logical CPUs",
                            "Switch between total CPU and individual thread tiles",
                            self.settings.show_logical_cpus,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.show_logical_cpus = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-graph-grids",
                            "Show graph grids",
                            "Keep chart grid guides visible",
                            self.settings.show_graph_grids,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.show_graph_grids = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-normalize-cpu",
                            "Normalize CPU usage",
                            "Keep total CPU within 0–100%",
                            self.settings.normalize_cpu_usage,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.normalize_cpu_usage = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        )),
                    cx,
                ),
            )
            .child(
                self.section_card(
                    "Devices and network",
                    "Visibility and unit controls for dynamic hardware pages",
                    v_flex()
                        .gap_2()
                        .child(self.setting_switch(
                            "setting-virtual-drives",
                            "Show virtual drives",
                            "Include loop, device-mapper, and other virtual block devices",
                            self.settings.show_virtual_drives,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.show_virtual_drives = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-virtual-network",
                            "Show virtual network interfaces",
                            "Include loopback, bridges, tunnels, containers, and VPN devices",
                            self.settings.show_virtual_network_interfaces,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.show_virtual_network_interfaces = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-network-bits",
                            "Network speed in bits",
                            "Display bit/s instead of bytes/s",
                            self.settings.network_bits,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.network_bits = *checked;
                                this.persist_settings();
                                cx.notify();
                            }),
                            cx,
                        )),
                    cx,
                ),
            )
            .child(self.render_column_settings(cx))
    }

    fn render_column_settings(&self, cx: &mut Context<Self>) -> Div {
        let apps = &self.settings.app_columns;
        let processes = &self.settings.process_columns;

        v_flex()
            .gap_4()
            .child(
                self.section_card(
                    "App columns",
                    "Choose the columns shown on the Apps page",
                    v_flex()
                        .gap_2()
                        .child(self.app_column_switch(
                            "Memory",
                            "app-col-memory",
                            apps.memory,
                            AppColumn::Memory,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "CPU",
                            "app-col-cpu",
                            apps.cpu,
                            AppColumn::Cpu,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Drive read speed",
                            "app-col-read-speed",
                            apps.read_speed,
                            AppColumn::ReadSpeed,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Drive read total",
                            "app-col-read-total",
                            apps.read_total,
                            AppColumn::ReadTotal,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Drive write speed",
                            "app-col-write-speed",
                            apps.write_speed,
                            AppColumn::WriteSpeed,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Drive write total",
                            "app-col-write-total",
                            apps.write_total,
                            AppColumn::WriteTotal,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "GPU",
                            "app-col-gpu",
                            apps.gpu,
                            AppColumn::Gpu,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "GPU memory",
                            "app-col-gpu-memory",
                            apps.gpu_memory,
                            AppColumn::GpuMemory,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Encoder",
                            "app-col-encoder",
                            apps.encoder,
                            AppColumn::Encoder,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Decoder",
                            "app-col-decoder",
                            apps.decoder,
                            AppColumn::Decoder,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Swap",
                            "app-col-swap",
                            apps.swap,
                            AppColumn::Swap,
                            cx,
                        ))
                        .child(self.app_column_switch(
                            "Combined memory",
                            "app-col-combined",
                            apps.combined_memory,
                            AppColumn::CombinedMemory,
                            cx,
                        )),
                    cx,
                ),
            )
            .child(
                self.section_card(
                    "Process columns",
                    "Choose the sortable columns shown on the Processes page",
                    v_flex()
                        .gap_2()
                        .child(self.process_column_switch(
                            "PID",
                            "proc-col-pid",
                            processes.pid,
                            ProcessColumnSetting::Pid,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "User",
                            "proc-col-user",
                            processes.user,
                            ProcessColumnSetting::User,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Memory",
                            "proc-col-memory",
                            processes.memory,
                            ProcessColumnSetting::Memory,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "CPU",
                            "proc-col-cpu",
                            processes.cpu,
                            ProcessColumnSetting::Cpu,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Drive read speed",
                            "proc-col-read-speed",
                            processes.read_speed,
                            ProcessColumnSetting::ReadSpeed,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Drive read total",
                            "proc-col-read-total",
                            processes.read_total,
                            ProcessColumnSetting::ReadTotal,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Drive write speed",
                            "proc-col-write-speed",
                            processes.write_speed,
                            ProcessColumnSetting::WriteSpeed,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Drive write total",
                            "proc-col-write-total",
                            processes.write_total,
                            ProcessColumnSetting::WriteTotal,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "GPU",
                            "proc-col-gpu",
                            processes.gpu,
                            ProcessColumnSetting::Gpu,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "GPU memory",
                            "proc-col-gpu-memory",
                            processes.gpu_memory,
                            ProcessColumnSetting::GpuMemory,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Encoder",
                            "proc-col-encoder",
                            processes.encoder,
                            ProcessColumnSetting::Encoder,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Decoder",
                            "proc-col-decoder",
                            processes.decoder,
                            ProcessColumnSetting::Decoder,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Total CPU time",
                            "proc-col-total-cpu",
                            processes.total_cpu_time,
                            ProcessColumnSetting::TotalCpuTime,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "User CPU time",
                            "proc-col-user-cpu",
                            processes.user_cpu_time,
                            ProcessColumnSetting::UserCpuTime,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "System CPU time",
                            "proc-col-system-cpu",
                            processes.system_cpu_time,
                            ProcessColumnSetting::SystemCpuTime,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Priority",
                            "proc-col-priority",
                            processes.priority,
                            ProcessColumnSetting::Priority,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Swap",
                            "proc-col-swap",
                            processes.swap,
                            ProcessColumnSetting::Swap,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Combined memory",
                            "proc-col-combined",
                            processes.combined_memory,
                            ProcessColumnSetting::CombinedMemory,
                            cx,
                        ))
                        .child(self.process_column_switch(
                            "Command line",
                            "proc-col-command",
                            processes.command_line,
                            ProcessColumnSetting::CommandLine,
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-detailed-priority",
                            "Detailed priority",
                            "Show numeric nice value beside the human-readable priority",
                            self.settings.detailed_priority,
                            cx.listener(|this, checked, _, cx| {
                                this.settings.detailed_priority = *checked;
                                this.sync_table_settings(cx);
                            }),
                            cx,
                        )),
                    cx,
                ),
            )
    }

    fn setting_row(
        &self,
        title: &'static str,
        subtitle: &'static str,
        control: impl IntoElement,
        cx: &Context<Self>,
    ) -> Div {
        h_flex()
            .items_center()
            .justify_between()
            .gap_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().font_bold().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    ),
            )
            .child(control)
    }

    fn setting_switch(
        &self,
        id: impl Into<ElementId>,
        title: &'static str,
        subtitle: &'static str,
        checked: bool,
        listener: impl Fn(&bool, &mut Window, &mut App) + 'static,
        cx: &Context<Self>,
    ) -> Div {
        self.setting_row(
            title,
            subtitle,
            Switch::new(id).checked(checked).on_click(listener),
            cx,
        )
    }

    fn app_column_switch(
        &self,
        label: &'static str,
        id: &'static str,
        checked: bool,
        column: AppColumn,
        cx: &mut Context<Self>,
    ) -> Div {
        self.setting_switch(
            id,
            label,
            "GNOME Resources parity column",
            checked,
            cx.listener(move |this, checked, _, cx| {
                column.set(&mut this.settings, *checked);
                this.persist_settings();
                cx.notify();
            }),
            cx,
        )
    }

    fn process_column_switch(
        &self,
        label: &'static str,
        id: &'static str,
        checked: bool,
        column: ProcessColumnSetting,
        cx: &mut Context<Self>,
    ) -> Div {
        self.setting_switch(
            id,
            label,
            "GNOME Resources parity column",
            checked,
            cx.listener(move |this, checked, _, cx| {
                column.set(&mut this.settings, *checked);
                this.sync_table_settings(cx);
            }),
            cx,
        )
    }

    fn property_row(&self, label: &'static str, value: String, cx: &Context<Self>) -> Div {
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

    fn unavailable_page(
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
                    .max_w(px(620.0))
                    .text_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
    }

    fn action_result_banner(&self, message: String, cx: &Context<Self>) -> Div {
        h_flex()
            .items_center()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .p_3()
            .child(div().size_2().rounded(px(99.0)).bg(cx.theme().blue))
            .child(div().text_sm().child(message))
    }

    fn selected_process(&self, cx: &Context<Self>) -> Option<ProcessInfo> {
        let pid = self.selected_pid?;
        self.process_table
            .read(cx)
            .delegate()
            .processes
            .iter()
            .find(|process| process.pid == pid)
            .cloned()
    }

    fn signal_pids(&mut self, pids: &[Pid], signal: Signal) {
        let mut succeeded = 0;
        let mut unavailable = 0;
        for pid in pids {
            match self
                .system
                .process(*pid)
                .and_then(|process| process.kill_with(signal))
            {
                Some(true) => succeeded += 1,
                _ => unavailable += 1,
            }
        }
        self.last_action = Some(format!(
            "Signal {signal:?}: {succeeded} succeeded, {unavailable} unavailable or denied"
        ));
    }

    fn persist_settings(&mut self) {
        self.last_action = match self.settings.save() {
            Ok(()) => Some("Settings saved".to_string()),
            Err(error) => Some(format!("Could not save settings: {error}")),
        };
    }

    fn sync_table_settings(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        self.process_table.update(cx, |table, cx| {
            table.delegate_mut().set_settings(&settings);
            table.refresh(cx);
            cx.notify();
        });
        self.persist_settings();
        cx.notify();
    }

    fn trim_history(&mut self) {
        let maximum = self.settings.clamped_graph_points();
        while self.history.len() > maximum {
            self.history.pop_front();
        }
    }

    fn visible_disks(&self) -> impl Iterator<Item = &DiskInfo> {
        self.disk_info
            .iter()
            .filter(|disk| self.settings.show_virtual_drives || !disk.metadata.is_virtual)
    }

    fn visible_networks(&self) -> impl Iterator<Item = &NetworkInfo> {
        self.network_info.iter().filter(|interface| {
            self.settings.show_virtual_network_interfaces || !interface.metadata.is_virtual
        })
    }
}

#[derive(Clone, Copy)]
enum AppColumn {
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
}

impl AppColumn {
    fn set(self, settings: &mut MonitorSettings, value: bool) {
        match self {
            Self::Memory => settings.app_columns.memory = value,
            Self::Cpu => settings.app_columns.cpu = value,
            Self::ReadSpeed => settings.app_columns.read_speed = value,
            Self::ReadTotal => settings.app_columns.read_total = value,
            Self::WriteSpeed => settings.app_columns.write_speed = value,
            Self::WriteTotal => settings.app_columns.write_total = value,
            Self::Gpu => settings.app_columns.gpu = value,
            Self::GpuMemory => settings.app_columns.gpu_memory = value,
            Self::Encoder => settings.app_columns.encoder = value,
            Self::Decoder => settings.app_columns.decoder = value,
            Self::Swap => settings.app_columns.swap = value,
            Self::CombinedMemory => settings.app_columns.combined_memory = value,
        }
    }
}

#[derive(Clone, Copy)]
enum ProcessColumnSetting {
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
}

impl ProcessColumnSetting {
    fn set(self, settings: &mut MonitorSettings, value: bool) {
        match self {
            Self::Pid => settings.process_columns.pid = value,
            Self::User => settings.process_columns.user = value,
            Self::Memory => settings.process_columns.memory = value,
            Self::Cpu => settings.process_columns.cpu = value,
            Self::ReadSpeed => settings.process_columns.read_speed = value,
            Self::ReadTotal => settings.process_columns.read_total = value,
            Self::WriteSpeed => settings.process_columns.write_speed = value,
            Self::WriteTotal => settings.process_columns.write_total = value,
            Self::Gpu => settings.process_columns.gpu = value,
            Self::GpuMemory => settings.process_columns.gpu_memory = value,
            Self::Encoder => settings.process_columns.encoder = value,
            Self::Decoder => settings.process_columns.decoder = value,
            Self::TotalCpuTime => settings.process_columns.total_cpu_time = value,
            Self::UserCpuTime => settings.process_columns.user_cpu_time = value,
            Self::SystemCpuTime => settings.process_columns.system_cpu_time = value,
            Self::Priority => settings.process_columns.priority = value,
            Self::Swap => settings.process_columns.swap = value,
            Self::CombinedMemory => settings.process_columns.combined_memory = value,
            Self::CommandLine => settings.process_columns.command_line = value,
        }
    }
}

fn sparkline_for_usage(usage: f64) -> String {
    let level = (usage / 12.5).round().clamp(0.0, 8.0) as usize;
    let blocks = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "█"];
    let current = blocks[level];
    format!("▁▂▃{current}{current}▅▃▂")
}

fn option_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "N/A".to_string(), |value| format!("{value:.0}%"))
}

fn option_mhz(value: Option<f64>) -> String {
    value.map_or_else(|| "N/A".to_string(), |value| format!("{value:.0} MHz"))
}
