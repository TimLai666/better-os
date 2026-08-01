use super::*;

use gpui_component::{
    Icon, IconName,
    dialog::{
        AlertDialog, DialogAction, DialogClose, DialogDescription, DialogFooter, DialogHeader,
        DialogTitle,
    },
    input::Input,
    switch::Switch,
};
use sysinfo::Signal;

impl MonitorWindow {
    pub(super) fn render_resources_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let point = self.current_point();
        let locale = self.settings.locale.resolved();
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
                            .label(match locale {
                                Locale::ZhTw => "設定",
                                _ => "Settings",
                            })
                            .selected(self.active_page == MonitorPage::Settings)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_active_page(MonitorPage::Settings);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().flex_1().min_h(px(0.0)).overflow_y_scrollbar().child(
                    v_flex()
                        .gap_1()
                        .child(self.sidebar_group_label(
                            match locale {
                                Locale::ZhTw => "應用程式",
                                _ => "Applications",
                            },
                            cx,
                        ))
                        .child(self.sidebar_resource_row(
                            "sidebar-apps",
                            MonitorPage::Apps.label(locale).to_string(),
                            format!("{} grouped apps", self.app_groups.len()),
                            self.app_groups.first().map(|app| app.cpu_usage as f64),
                            self.active_page == MonitorPage::Apps,
                            cx.listener(|this, _, _, cx| {
                                this.set_active_page(MonitorPage::Apps);
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.sidebar_resource_row(
                            "sidebar-processes",
                            MonitorPage::Processes.label(locale).to_string(),
                            format!("{} running", self.system.processes().len()),
                            Some(point.cpu),
                            self.active_page == MonitorPage::Processes,
                            cx.listener(|this, _, _, cx| {
                                this.set_active_page(MonitorPage::Processes);
                                cx.notify();
                            }),
                            cx,
                        ))
                        .child(self.sidebar_group_label(
                            match locale {
                                Locale::ZhTw => "系統",
                                _ => "System",
                            },
                            cx,
                        ))
                        .child(
                            self.sidebar_resource_row(
                                "sidebar-cpu",
                                MonitorPage::Cpu.label(locale).to_string(),
                                self.cpu_details
                                    .model_name
                                    .clone()
                                    .unwrap_or_else(|| "CPU".to_string()),
                                Some(point.cpu),
                                self.active_page == MonitorPage::Cpu,
                                cx.listener(|this, _, _, cx| {
                                    this.set_active_page(MonitorPage::Cpu);
                                    cx.notify();
                                }),
                                cx,
                            ),
                        )
                        .child(self.sidebar_resource_row(
                            "sidebar-memory",
                            MonitorPage::Memory.label(locale).to_string(),
                            memory_detail,
                            Some(point.memory),
                            self.active_page == MonitorPage::Memory,
                            cx.listener(|this, _, _, cx| {
                                this.set_active_page(MonitorPage::Memory);
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
                                    this.set_active_page(MonitorPage::Gpu);
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
                                    this.set_active_page(MonitorPage::Npu);
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
                                MonitorPage::Storage.label(locale).to_string(),
                                disk.metadata
                                    .model
                                    .clone()
                                    .unwrap_or_else(|| disk.metadata.device.clone()),
                                usage,
                                self.active_page == MonitorPage::Storage
                                    && self.selected_disk == index,
                                cx.listener(move |this, _, _, cx| {
                                    this.selected_disk = index;
                                    this.set_active_page(MonitorPage::Storage);
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
                                            this.set_active_page(MonitorPage::Network);
                                            cx.notify();
                                        }),
                                        cx,
                                    )
                                }),
                        )
                        .children(self.batteries.iter().enumerate().map(|(index, battery)| {
                            self.sidebar_resource_row(
                                ("sidebar-battery", index),
                                MonitorPage::Battery.label(locale).to_string(),
                                battery.name.clone(),
                                battery.charge_percent,
                                self.active_page == MonitorPage::Battery
                                    && self.selected_battery == index,
                                cx.listener(move |this, _, _, cx| {
                                    this.selected_battery = index;
                                    this.set_active_page(MonitorPage::Battery);
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
                            .child(div().text_sm().font_bold().child(match locale {
                                Locale::ZhTw => "正在記錄",
                                _ => "Recording",
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} {} · {}",
                                self.store.samples().len(),
                                match locale {
                                    Locale::ZhTw => "筆樣本",
                                    _ => "samples",
                                },
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

    #[expect(
        clippy::too_many_arguments,
        reason = "a dynamic sidebar row carries its full resource presentation and navigation contract"
    )]
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
        let visible_count = self.app_table.read(cx).delegate().groups.len();

        v_flex()
            .gap_4()
            .child(self.render_search_toolbar("Search apps… Use | to require multiple terms", cx))
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
                            .child(div().font_bold().child("Applications"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{visible_count} visible · {} grouped",
                                        self.app_groups.len()
                                    )),
                            ),
                    )
                    .when(visible_count == 0, |this| {
                        this.child(
                            v_flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .child(div().font_bold().child("No matching applications"))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Clear the search or wait for application groups."),
                                ),
                        )
                    })
                    .when(visible_count > 0, |this| {
                        this.child(
                            div().flex_1().child(
                                DataTable::new(&self.app_table)
                                    .bordered(false)
                                    .stripe(true)
                                    .small(),
                            ),
                        )
                    }),
            )
            .when_some(self.last_action.clone(), |this, message| {
                this.child(self.action_result_banner(message, cx))
            })
    }

    pub(super) fn render_processes_parity(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.selected_process(cx);
        let (visible_count, batch_pids) = {
            let table = self.process_table.read(cx);
            (
                table.delegate().processes.len(),
                table.delegate().selected_pids(),
            )
        };
        let action_pids = if batch_pids.is_empty() {
            selected
                .as_ref()
                .map(|process| vec![process.pid])
                .unwrap_or_default()
        } else {
            batch_pids.clone()
        };
        let batch_count = batch_pids.len();

        v_flex()
            .gap_4()
            .child(
                self.render_search_toolbar("Search processes… Use | to require multiple terms", cx),
            )
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .p_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_bold()
                                    .child(if batch_count > 0 {
                                        format!("{batch_count} processes selected for batch actions")
                                    } else {
                                        match selected.as_ref() {
                                            Some(process) => format!(
                                                "Focused: {} · PID {} · {}",
                                                process.name, process.pid, process.user
                                            ),
                                            None => "Focus a row or use the selection switches".to_string(),
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "Row focus is used for details. Selection switches are used for batch actions.",
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                Button::new("process-select-visible")
                                    .ghost()
                                    .small()
                                    .disabled(visible_count == 0)
                                    .label("Select visible")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.process_table.update(cx, |table, cx| {
                                            table.delegate_mut().select_all_visible();
                                            table.refresh(cx);
                                            cx.notify();
                                        });
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("process-clear-batch")
                                    .ghost()
                                    .small()
                                    .disabled(batch_count == 0)
                                    .label("Clear batch")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_process_batch(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(self.process_information_dialog(selected.clone(), cx))
                            .child(self.process_action_buttons(action_pids, cx)),
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
                                        "{visible_count} visible · {} total · {batch_count} batch selected",
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

    fn process_information_dialog(
        &self,
        process: Option<ProcessInfo>,
        cx: &mut Context<Self>,
    ) -> AlertDialog {
        let enabled = process.is_some();
        let process = process.unwrap_or(ProcessInfo {
            pid: Pid::from_u32(0),
            parent_pid: None,
            name: "No process selected".to_string(),
            user: "N/A".to_string(),
            state: "N/A".to_string(),
            cpu_usage: 0.0,
            memory: 0,
            swap: 0,
            read_speed: 0,
            read_total: 0,
            write_speed: 0,
            write_total: 0,
            total_cpu_time_ticks: None,
            user_cpu_time_ticks: None,
            system_cpu_time_ticks: None,
            priority: None,
            nice: None,
            threads: None,
            file_descriptors: None,
            command_line: String::new(),
            executable: None,
            working_directory: None,
            cgroup: None,
            app_id: None,
        });
        let description = format!(
            "PID {} · Parent {} · User {} · State {}
Threads: {} · File descriptors: {}
Executable: {}
Working directory: {}
Cgroup: {}
Command: {}",
            process.pid,
            process
                .parent_pid
                .map_or_else(|| "N/A".to_string(), |pid| pid.to_string()),
            process.user,
            process.state,
            process
                .threads
                .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
            process
                .file_descriptors
                .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
            process.executable.unwrap_or_else(|| "N/A".to_string()),
            process
                .working_directory
                .unwrap_or_else(|| "N/A".to_string()),
            process.cgroup.unwrap_or_else(|| "N/A".to_string()),
            if process.command_line.is_empty() {
                "N/A".to_string()
            } else {
                process.command_line
            },
        );

        AlertDialog::new(cx)
            .trigger(
                Button::new("process-information")
                    .outline()
                    .small()
                    .disabled(!enabled)
                    .label("Information"),
            )
            .content(move |content, _, _| {
                content
                    .child(
                        DialogHeader::new()
                            .child(DialogTitle::new().child("Process Information"))
                            .child(DialogDescription::new().child(description.clone())),
                    )
                    .child(
                        DialogFooter::new().child(
                            DialogClose::new().child(
                                Button::new("process-information-close")
                                    .outline()
                                    .label("Close"),
                            ),
                        ),
                    )
            })
    }

    fn process_action_buttons(&self, pids: Vec<Pid>, cx: &mut Context<Self>) -> Div {
        let enabled = !pids.is_empty();
        let count = pids.len();
        let term_pids = pids.clone();
        let kill_pids = pids.clone();
        let stop_pids = pids.clone();
        let continue_pids = pids;
        let term_target = cx.entity().downgrade();
        let kill_target = cx.entity().downgrade();

        h_flex()
            .flex_wrap()
            .gap_1()
            .child(
                AlertDialog::new(cx)
                    .trigger(
                        Button::new("process-term")
                            .outline()
                            .small()
                            .disabled(!enabled)
                            .label(if count > 1 {
                                format!("End {count}")
                            } else {
                                "End".to_string()
                            }),
                    )
                    .on_ok(move |_, _, cx| {
                        let _ = term_target.update(cx, |this, cx| {
                            this.signal_pids(&term_pids, Signal::Term);
                            this.clear_process_batch(cx);
                            cx.notify();
                        });
                        true
                    })
                    .content(move |content, _, _| {
                        content
                            .child(
                                DialogHeader::new()
                                    .child(DialogTitle::new().child("End selected processes?"))
                                    .child(DialogDescription::new().child(format!(
                                        "A graceful termination signal will be sent to {count} process{}.",
                                        if count == 1 { "" } else { "es" }
                                    ))),
                            )
                            .child(
                                DialogFooter::new()
                                    .child(DialogClose::new().child(
                                        Button::new("process-term-cancel")
                                            .outline()
                                            .label("Cancel"),
                                    ))
                                    .child(DialogAction::new().child(
                                        Button::new("process-term-confirm")
                                            .warning()
                                            .label("End"),
                                    )),
                            )
                    }),
            )
            .child(
                AlertDialog::new(cx)
                    .trigger(
                        Button::new("process-kill")
                            .warning()
                            .small()
                            .disabled(!enabled)
                            .label(if count > 1 {
                                format!("Force stop {count}")
                            } else {
                                "Force stop".to_string()
                            }),
                    )
                    .on_ok(move |_, _, cx| {
                        let _ = kill_target.update(cx, |this, cx| {
                            this.signal_pids(&kill_pids, Signal::Kill);
                            this.clear_process_batch(cx);
                            cx.notify();
                        });
                        true
                    })
                    .content(move |content, _, _| {
                        content
                            .child(
                                DialogHeader::new()
                                    .child(DialogTitle::new().child("Force stop selected processes?"))
                                    .child(DialogDescription::new().child(format!(
                                        "{count} process{} will be stopped immediately and cannot clean up first.",
                                        if count == 1 { "" } else { "es" }
                                    ))),
                            )
                            .child(
                                DialogFooter::new()
                                    .child(DialogClose::new().child(
                                        Button::new("process-kill-cancel")
                                            .outline()
                                            .label("Cancel"),
                                    ))
                                    .child(DialogAction::new().child(
                                        Button::new("process-kill-confirm")
                                            .warning()
                                            .label("Force stop"),
                                    )),
                            )
                    }),
            )
            .child(
                Button::new("process-stop")
                    .ghost()
                    .small()
                    .disabled(!enabled)
                    .label("Pause")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.signal_pids(&stop_pids, Signal::Stop);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("process-continue")
                    .ghost()
                    .small()
                    .disabled(!enabled)
                    .label("Resume")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.signal_pids(&continue_pids, Signal::Continue);
                        cx.notify();
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
                                match self.settings.locale.resolved() {
                                    Locale::ZhTw => "語言",
                                    _ => "Language",
                                },
                                match self.settings.locale.resolved() {
                                    Locale::ZhTw => "切換後立即套用，不需要重新啟動",
                                    _ => "Changes apply immediately without restarting",
                                },
                                Button::new("setting-locale")
                                    .outline()
                                    .small()
                                    .label(self.settings.locale.label())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.locale = this.settings.locale.next();
                                        this.persist_settings();
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        )
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

    fn support_state_palette(&self, cx: &Context<Self>) -> SupportStatePalette {
        SupportStatePalette {
            border: cx.theme().border,
            background: cx.theme().background,
            foreground: cx.theme().foreground,
            muted_foreground: cx.theme().muted_foreground,
            success: cx.theme().green,
            warning: cx.theme().yellow,
            danger: cx.theme().red,
            info: cx.theme().blue,
            radius: cx.theme().radius,
        }
    }

    fn unavailable_page(
        &self,
        title: &'static str,
        description: &'static str,
        cx: &Context<Self>,
    ) -> Div {
        let state = SupportState::new(SupportStateKind::Unavailable, title, description);
        v_flex()
            .items_center()
            .justify_center()
            .min_h(px(420.0))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .p_5()
            .child(
                div()
                    .w_full()
                    .max_w(px(680.0))
                    .child(support_state_panel(state, self.support_state_palette(cx))),
            )
    }

    fn action_result_banner(&self, state: SupportState, cx: &Context<Self>) -> impl IntoElement {
        support_state_panel(state, self.support_state_palette(cx))
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

    fn clear_process_batch(&mut self, cx: &mut Context<Self>) {
        self.process_table.update(cx, |table, cx| {
            table.delegate_mut().clear_selected();
            table.refresh(cx);
            cx.notify();
        });
    }

    pub(crate) fn signal_pids(&mut self, pids: &[Pid], signal: Signal) {
        let mut succeeded = 0;
        let mut stale = 0;
        let mut denied = 0;
        let mut unsupported = 0;
        for pid in pids {
            let Some(process) = self.system.process(*pid) else {
                stale += 1;
                continue;
            };
            match process.kill_with(signal) {
                Some(true) => succeeded += 1,
                Some(false) => denied += 1,
                None => unsupported += 1,
            }
        }

        let kind = if denied > 0 {
            SupportStateKind::PermissionDenied
        } else if unsupported > 0 {
            SupportStateKind::Unavailable
        } else if stale > 0 {
            SupportStateKind::Stale
        } else {
            SupportStateKind::Success
        };
        let locale = self.settings.locale.resolved();
        let title = match (locale, kind) {
            (Locale::ZhTw, SupportStateKind::PermissionDenied) => "需要額外權限",
            (Locale::ZhTw, SupportStateKind::Unavailable) => "此操作無法使用",
            (Locale::ZhTw, SupportStateKind::Stale) => "程序已經結束",
            (Locale::ZhTw, _) => "程序操作完成",
            (_, SupportStateKind::PermissionDenied) => "Permission required",
            (_, SupportStateKind::Unavailable) => "Action unavailable",
            (_, SupportStateKind::Stale) => "Process already ended",
            (_, _) => "Process action finished",
        };
        let detail = match locale {
            Locale::ZhTw => format!(
                "{signal:?}：成功 {succeeded} 個、已結束 {stale} 個、權限不足 {denied} 個、不支援 {unsupported} 個"
            ),
            _ => format!(
                "Signal {signal:?}: {succeeded} succeeded, {stale} stale, {denied} denied, {unsupported} unsupported"
            ),
        };
        self.last_action = Some(SupportState::new(kind, title, detail));
    }

    fn persist_settings(&mut self) {
        let locale = self.settings.locale.resolved();
        self.last_action = match self.settings.save() {
            Ok(()) => Some(SupportState::new(
                SupportStateKind::Success,
                match locale {
                    Locale::ZhTw => "設定已儲存",
                    _ => "Settings saved",
                },
                match locale {
                    Locale::ZhTw => "新的偏好設定已寫入使用者設定檔。",
                    _ => "The updated preferences were written to the user configuration.",
                },
            )),
            Err(error) => Some(SupportState::new(
                SupportStateKind::CollectorError,
                match locale {
                    Locale::ZhTw => "無法儲存設定",
                    _ => "Could not save settings",
                },
                error.to_string(),
            )),
        };
    }

    fn sync_table_settings(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let app_settings = settings.clone();
        self.app_table.update(cx, |table, cx| {
            table.delegate_mut().set_settings(&app_settings);
            table.refresh(cx);
            cx.notify();
        });
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
