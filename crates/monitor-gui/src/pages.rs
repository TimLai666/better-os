//! Every page the window can show.
//!
//! The pages read models and draw them. None of them decides what a number
//! means, and none of them can render a missing reading as a zero, because
//! every value goes through the cell helpers in `tables`.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    switch::Switch,
    table::DataTable,
    *,
};
use monitor_core::{
    ActionRefusal, CollectorHealth, CollectorReport, EntityKind, MetricId, ProcessAction,
};
use monitor_views::format::{
    Cell, bytes, bytes_per_second, cell, count, duration, percent, ratio_percent,
};
use monitor_views::overview::{ResourceSummary, ResourceVerdict, ThrottlingState};
use monitor_views::{Field, ProcessFacts, field};

use crate::app::{ActionReport, MonitorApp, Page};
use crate::i18n::{Copy, copy};
use crate::tables::{cell_element, confidence_label, evidence_label};

fn metric(raw: &str) -> MetricId {
    MetricId::new(raw).expect("a page metric id must be well formed")
}

impl MonitorApp {
    pub(crate) fn render_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.page {
            Page::Overview => self.overview_page(cx),
            Page::Apps => self.apps_page(cx),
            Page::Processes => self.processes_page(cx),
            Page::Cpu => self.cpu_page(cx),
            Page::Memory => self.memory_page(cx),
            Page::Storage => self.storage_page(cx),
            Page::Network => self.network_page(cx),
            Page::Gpu => {
                self.unsupported_page(copy(self.locale).gpu, copy(self.locale).gpu_unsupported, cx)
            }
            Page::Energy => self.unsupported_page(
                copy(self.locale).energy,
                copy(self.locale).energy_unsupported,
                cx,
            ),
            Page::History => self.history_page(cx),
            Page::Incidents => self.incidents_page(cx),
            Page::Inventory => self.inventory_page(cx),
            Page::Diagnostics => self.diagnostics_page(cx),
            Page::Settings => self.settings_page(cx),
        }
    }

    fn verdict_label(&self, verdict: &ResourceVerdict, c: &'static Copy) -> &'static str {
        match verdict {
            ResourceVerdict::Nominal => c.verdict_nominal,
            ResourceVerdict::BusyWithoutContention => c.verdict_busy,
            ResourceVerdict::UnderPressure { .. } => c.verdict_pressure,
            ResourceVerdict::Saturated { .. } => c.verdict_saturated,
            ResourceVerdict::CollectorFailed { .. } => c.verdict_collector_failed,
            ResourceVerdict::Unsupported { .. } => c.verdict_unsupported,
            ResourceVerdict::Unobserved { .. } => c.verdict_unobserved,
        }
    }

    /// One resource card: the verdict first, then the readings behind it.
    ///
    /// The verdict is the headline because "busy" and "waiting" look identical
    /// in a utilization number and are the two things a user most needs kept
    /// apart.
    fn resource_card(
        &self,
        title: &'static str,
        summary: &ResourceSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let accent = match &summary.verdict {
            ResourceVerdict::Saturated { .. } | ResourceVerdict::CollectorFailed { .. } => {
                cx.theme().danger
            }
            ResourceVerdict::UnderPressure { .. } => cx.theme().warning,
            ResourceVerdict::BusyWithoutContention => cx.theme().primary,
            _ => cx.theme().muted_foreground,
        };
        let verdict = self.verdict_label(&summary.verdict, c);
        let detail = match &summary.verdict {
            ResourceVerdict::CollectorFailed { detail }
            | ResourceVerdict::Unsupported { detail }
            | ResourceVerdict::Unobserved { detail } => Some(detail.clone()),
            _ => None,
        };
        let body = v_flex()
            .min_w_0()
            .gap_3()
            .child(
                h_flex()
                    .min_w_0()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .flex_wrap()
                    .child(div().font_semibold().child(title))
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(accent)
                            .text_color(accent)
                            .text_xs()
                            .child(verdict),
                    ),
            )
            .when_some(detail, |element, detail| {
                element.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail),
                )
            })
            .child(
                h_flex()
                    .min_w_0()
                    .gap_4()
                    .flex_wrap()
                    .child(self.stat(
                        c.utilization,
                        cell_element(cell(&summary.utilization, |v| ratio_percent(*v)), c, cx),
                        cx,
                    ))
                    .child(self.stat(
                        c.pressure_some,
                        cell_element(cell(&summary.pressure_some, |v| percent(*v)), c, cx),
                        cx,
                    ))
                    .child(self.stat(
                        c.pressure_full,
                        cell_element(cell(&summary.pressure_full, |v| percent(*v)), c, cx),
                        cx,
                    )),
            );
        div()
            .min_w(px(300.0))
            .flex_1()
            .child(self.surface(body, cx))
            .into_any_element()
    }

    fn overview_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let throttling = match &self.overview.throttling {
            ThrottlingState::NotThrottled => c.throttling_none.to_string(),
            ThrottlingState::ClockHeldDown {
                current_hz,
                maximum_hz,
            } => format!(
                "{} ({:.2} / {:.2} GHz)",
                c.throttling_clock,
                current_hz / 1e9,
                maximum_hz / 1e9
            ),
            ThrottlingState::NotObservable { .. } => c.throttling_unobservable.to_string(),
        };

        let top_apps = {
            let apps = self.apps.read(cx).delegate();
            apps.model
                .top_by_cpu(5)
                .into_iter()
                .map(|row| {
                    (
                        row.group.display_name.clone(),
                        row.cpu_utilization,
                        row.group.members.len(),
                    )
                })
                .collect::<Vec<_>>()
        };

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.overview, c.overview_subtitle))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .flex_wrap()
                    .child(self.resource_card(c.cpu, &self.overview.cpu, cx))
                    .child(self.resource_card(c.memory, &self.overview.memory.resource, cx))
                    .child(self.resource_card(c.storage, &self.overview.io, cx)),
            )
            .child(
                self.surface(
                    h_flex()
                        .min_w_0()
                        .gap_4()
                        .flex_wrap()
                        .child(self.stat(
                            c.load_average,
                            cell_element(
                                cell(&self.overview.load_average_1m, |v| format!("{v:.2}")),
                                c,
                                cx,
                            ),
                            cx,
                        ))
                        .child(self.stat(
                            c.logical_cpus,
                            cell_element(cell(&self.overview.logical_cpus, |v| count(*v)), c, cx),
                            cx,
                        ))
                        .child(self.stat(
                            c.running_processes,
                            cell_element(cell(&self.overview.process_count, |v| count(*v)), c, cx),
                            cx,
                        ))
                        .child(self.stat(
                            c.throttling,
                            div().text_sm().child(throttling).into_any_element(),
                            cx,
                        )),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .flex_wrap()
                    .child(
                        div().min_w(px(300.0)).flex_1().child(
                            self.surface(
                                v_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .child(div().font_semibold().child(c.storage))
                                    .child(
                                        self.stat(
                                            c.read_throughput,
                                            div()
                                                .child(bytes_per_second(
                                                    self.overview.storage.read.total,
                                                ))
                                                .into_any_element(),
                                            cx,
                                        ),
                                    )
                                    .child(
                                        self.stat(
                                            c.write_throughput,
                                            div()
                                                .child(bytes_per_second(
                                                    self.overview.storage.write.total,
                                                ))
                                                .into_any_element(),
                                            cx,
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(match &self.overview.storage.busiest {
                                                Some(device) => {
                                                    format!("{}: {device}", c.busiest_device)
                                                }
                                                None => c.no_devices.to_string(),
                                            }),
                                    ),
                                cx,
                            ),
                        ),
                    )
                    .child(
                        div().min_w(px(300.0)).flex_1().child(
                            self.surface(
                                v_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .child(div().font_semibold().child(c.network))
                                    .child(
                                        self.stat(
                                            c.received,
                                            div()
                                                .child(bytes_per_second(
                                                    self.overview.network.read.total,
                                                ))
                                                .into_any_element(),
                                            cx,
                                        ),
                                    )
                                    .child(
                                        self.stat(
                                            c.sent,
                                            div()
                                                .child(bytes_per_second(
                                                    self.overview.network.write.total,
                                                ))
                                                .into_any_element(),
                                            cx,
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(match &self.overview.network.busiest {
                                                Some(interface) => {
                                                    format!("{}: {interface}", c.busiest_device)
                                                }
                                                None => c.no_devices.to_string(),
                                            }),
                                    ),
                                cx,
                            ),
                        ),
                    ),
            )
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_2()
                        .child(div().font_semibold().child(c.top_applications))
                        .children(top_apps.into_iter().map(|(name, cpu, members)| {
                            h_flex()
                                .min_w_0()
                                .justify_between()
                                .gap_3()
                                .py_1()
                                .child(div().truncate().child(name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{members} {}", c.process_count)),
                                )
                                .child(div().child(if cpu.is_unavailable() {
                                    c.nothing_measured.to_string()
                                } else {
                                    ratio_percent(cpu.total)
                                }))
                        })),
                    cx,
                ),
            )
            .child(self.collector_health_card(cx))
            .into_any_element()
    }

    /// Which collectors are working, and the coverage of the last round.
    fn collector_health_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let coverage = self.overview.coverage;
        let rows = self
            .overview
            .collectors
            .iter()
            .map(|status| {
                let (label, color) = match &status.health {
                    CollectorHealth::Healthy => (c.health_healthy, cx.theme().success),
                    CollectorHealth::Degraded { .. } => (c.health_degraded, cx.theme().warning),
                    CollectorHealth::Failed { .. } => (c.health_failed, cx.theme().danger),
                    CollectorHealth::Unsupported(_) => {
                        (c.health_unsupported, cx.theme().muted_foreground)
                    }
                };
                h_flex()
                    .min_w_0()
                    .justify_between()
                    .gap_3()
                    .py_0p5()
                    .child(div().truncate().text_sm().child(status.collector.clone()))
                    .child(div().text_xs().text_color(color).child(label))
                    .when_some(status.detail(), |element, detail| {
                        element.child(
                            div()
                                .max_w(px(320.0))
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(detail),
                        )
                    })
            })
            .collect::<Vec<_>>();

        self.surface(
            v_flex()
                .min_w_0()
                .gap_2()
                .child(
                    h_flex()
                        .min_w_0()
                        .justify_between()
                        .child(div().font_semibold().child(c.observation_health))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.collector),
                        ),
                )
                .children(rows)
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_4()
                        .flex_wrap()
                        .pt_2()
                        .child(
                            div()
                                .text_xs()
                                .child(format!("{}: {}", c.coverage_value, coverage.value)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(format!("{}: {}", c.coverage_stale, coverage.stale)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(format!("{}: {}", c.coverage_unknown, coverage.unknown)),
                        )
                        .child(div().text_xs().child(format!(
                            "{}: {}",
                            c.coverage_unsupported, coverage.unsupported
                        )))
                        .child(div().text_xs().child(format!(
                            "{}: {}",
                            c.coverage_denied, coverage.permission_denied
                        ))),
                ),
            cx,
        )
    }

    fn apps_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .w_full()
            .h_full()
            .min_w_0()
            .gap_3()
            .child(self.page_heading(c.apps, c.apps_subtitle))
            .child(
                div().flex_1().min_h(px(320.0)).child(
                    DataTable::new(&self.apps)
                        .bordered(true)
                        .stripe(true)
                        .small(),
                ),
            )
            .child(self.process_detail(cx))
            .into_any_element()
    }

    fn processes_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let tree = self.tree_mode(cx);
        let show_commands = self.include_command_lines;
        v_flex()
            .w_full()
            .h_full()
            .min_w_0()
            .gap_3()
            .child(self.page_heading(c.processes, c.processes_subtitle))
            .child(
                h_flex()
                    .min_w_0()
                    .gap_4()
                    .flex_wrap()
                    .child(
                        Switch::new("tree-mode")
                            .checked(tree)
                            .label(c.tree_view)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_tree_mode(cx))),
                    )
                    .child(
                        Switch::new("show-command-lines")
                            .checked(show_commands)
                            .label(c.show_command_lines)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.set_command_lines(*checked, cx)
                            })),
                    ),
            )
            .child(
                div().flex_1().min_h(px(320.0)).child(
                    DataTable::new(&self.processes)
                        .bordered(true)
                        .stripe(true)
                        .small(),
                ),
            )
            .child(self.process_detail(cx))
            .into_any_element()
    }

    /// The detail panel: what is known about the selected process, and the
    /// actions that are available or the reason they are not.
    fn process_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(process) = self.selected_process(cx) else {
            return self.surface(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.select_a_process),
                cx,
            );
        };

        if let Some(pending) = self.pending.clone() {
            let question = match pending.action {
                ProcessAction::ForceStop => c.confirm_force_stop,
                _ => c.confirm_terminate,
            };
            return self.surface(
                v_flex()
                    .min_w_0()
                    .gap_3()
                    .child(
                        div()
                            .font_semibold()
                            .child(format!("{} · {}", pending.name, pending.pid)),
                    )
                    .child(div().text_sm().child(question))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("confirm-action")
                                    .danger()
                                    .label(c.confirm)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.confirm_pending(cx)),
                                    ),
                            )
                            .child(
                                Button::new("cancel-action").label(c.cancel).on_click(
                                    cx.listener(|this, _, _, cx| this.cancel_pending(cx)),
                                ),
                            ),
                    ),
                cx,
            );
        }

        let nice_step = process
            .nice
            .copied()
            .and_then(|nice| i32::try_from(nice).ok())
            .map(|nice| (nice + 5).min(monitor_core::NICE_MAXIMUM))
            .unwrap_or(monitor_core::NICE_MAXIMUM);
        let paused = process.is_paused();
        let offered: Vec<(&'static str, ProcessAction)> = vec![
            (c.terminate, ProcessAction::Terminate),
            (c.force_stop, ProcessAction::ForceStop),
            if paused {
                (c.resume_process, ProcessAction::Resume)
            } else {
                (c.pause_process, ProcessAction::Pause)
            },
            (c.lower_priority, ProcessAction::SetNice(nice_step)),
        ];

        let buttons = offered
            .into_iter()
            .map(|(label, action)| {
                let availability = self.availability(&process, action);
                let refusal = availability
                    .refusal()
                    .map(|refusal| self.refusal_label(refusal, c).to_string());
                let target = process.clone();
                Button::new(SharedString::from(format!(
                    "action-{}-{}",
                    action.key(),
                    process.pid
                )))
                .label(label)
                .when(action.is_destructive(), |button| button.danger())
                .disabled(refusal.is_some())
                .when_some(refusal, |button, reason| {
                    button.tooltip(SharedString::from(reason))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.request_action(&target, action, cx);
                }))
            })
            .collect::<Vec<_>>();

        let report = self
            .last_action
            .clone()
            .map(|report| self.report_line(&report, c));
        let identity = process.display_name();
        let details = self.diagnostic_lines(&process, c, cx);

        self.surface(
            v_flex()
                .min_w_0()
                .gap_3()
                .child(
                    h_flex()
                        .min_w_0()
                        .justify_between()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            div()
                                .font_semibold()
                                .child(format!("{identity} · {}", process.pid)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    self.grouping_summary(process.pid, c, cx)
                                        .unwrap_or_default(),
                                ),
                        ),
                )
                .child(h_flex().min_w_0().gap_4().flex_wrap().children(details))
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.actions),
                        )
                        .child(h_flex().gap_2().flex_wrap().children(buttons)),
                )
                .when_some(report, |element, line| {
                    element.child(div().text_sm().child(line))
                }),
            cx,
        )
    }

    fn diagnostic_lines(
        &self,
        process: &ProcessFacts,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        vec![
            self.stat(
                c.column_user,
                cell_element(cell(&process.user, |v| v.clone()), c, cx),
                cx,
            ),
            self.stat(
                c.column_state,
                cell_element(cell(&process.state, |v| v.clone()), c, cx),
                cx,
            ),
            self.stat(
                c.column_cpu_time,
                cell_element(cell(&process.cpu_time_total, |v| duration(*v)), c, cx),
                cx,
            ),
            self.stat(
                c.column_memory,
                cell_element(cell(&process.memory_resident, |v| bytes(*v as f64)), c, cx),
                cx,
            ),
            self.stat(
                c.column_descriptors,
                cell_element(cell(&process.file_descriptors, |v| count(*v)), c, cx),
                cx,
            ),
            self.stat(
                c.column_cgroup,
                cell_element(cell(&process.cgroup, |v| v.clone()), c, cx),
                cx,
            ),
        ]
    }

    fn grouping_summary(&self, pid: u32, c: &'static Copy, cx: &App) -> Option<String> {
        let delegate = self.apps.read(cx).delegate();
        let group = delegate.model.grouping().group_of(pid)?;
        let member = group.members.iter().find(|member| member.pid == pid)?;
        Some(format!(
            "{}: {} · {}",
            c.grouped_because,
            evidence_label(&member.evidence, c),
            confidence_label(group.confidence(), c)
        ))
    }

    fn refusal_label(&self, refusal: &ActionRefusal, c: &'static Copy) -> &'static str {
        match refusal {
            ActionRefusal::NotOwnedByCurrentUser { .. } => c.refusal_other_user,
            ActionRefusal::OwnershipUnknown => c.refusal_ownership_unknown,
            ActionRefusal::ProtectedProcess { .. } => c.refusal_protected,
            ActionRefusal::RaisingPriorityNeedsPrivilege { .. } => c.refusal_raise_priority,
            ActionRefusal::NiceOutOfRange { .. } => c.refusal_nice_range,
            ActionRefusal::CurrentPriorityUnknown => c.refusal_priority_unknown,
        }
    }

    /// One line saying what the last action actually did, including when it
    /// failed. A signal that was accepted is not reported as an exit.
    pub(crate) fn report_line(&self, report: &ActionReport, c: &'static Copy) -> String {
        match report {
            ActionReport::Succeeded { pid, outcome } => match outcome {
                monitor_core::ActionOutcome::SignalAccepted { .. } => {
                    format!("{pid}: {}", c.outcome_signal_sent)
                }
                monitor_core::ActionOutcome::PriorityChanged { to, .. } => {
                    format!("{pid}: {} ({to})", c.outcome_priority_changed)
                }
            },
            ActionReport::Refused { pid, refusal } => {
                format!("{pid}: {}", self.refusal_label(refusal, c))
            }
            ActionReport::Failed { pid, error } => {
                let reason = match error {
                    monitor_core::ActionError::ProcessDisappeared { .. } => c.failure_disappeared,
                    monitor_core::ActionError::PermissionDenied { .. } => c.failure_denied,
                    monitor_core::ActionError::InvalidRequest { .. } => c.failure_invalid,
                    monitor_core::ActionError::Unsupported { .. } => c.failure_unsupported,
                    monitor_core::ActionError::Failed { .. } => c.failure_other,
                };
                format!("{pid}: {reason}")
            }
        }
    }

    /// A page for hardware the collectors in this build cannot see.
    ///
    /// It exists rather than being hidden, and it says what is missing. A page
    /// of zeros would be a lie about the machine.
    fn unsupported_page(
        &self,
        title: &'static str,
        explanation: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(title, c.unsupported_page_title))
            .child(
                self.surface(
                    h_flex()
                        .min_w_0()
                        .gap_3()
                        .items_start()
                        .child(Icon::new(IconName::TriangleAlert))
                        .child(div().min_w_0().text_sm().child(explanation)),
                    cx,
                ),
            )
            .into_any_element()
    }

    /// The metric set of one collector, if the round carried it.
    fn system_metrics(&self, collector: &str) -> Option<&CollectorReport> {
        self.reports
            .iter()
            .find(|report| report.collector.as_str() == collector)
    }

    fn metric_row(&self, label: String, rendered: Cell, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        h_flex()
            .min_w_0()
            .justify_between()
            .gap_3()
            .py_0p5()
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(cell_element(rendered, c, cx))
            .into_any_element()
    }

    fn cpu_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(report) = self.system_metrics("linux.cpu") else {
            return self.unsupported_page(c.cpu, c.verdict_unobserved, cx);
        };
        let metrics = report.metrics.clone();
        let cores = report
            .entities_of(EntityKind::LogicalCpu)
            .map(|entity| {
                (
                    entity.id.key.clone(),
                    field::number(&entity.metrics, &metric("cpu.utilization.busy")),
                    field::number(&entity.metrics, &metric("cpu.frequency.current")),
                )
            })
            .collect::<Vec<_>>();

        let rows = [
            ("cpu.utilization.busy", c.utilization),
            ("cpu.utilization.user", c.column_cpu),
            ("cpu.utilization.iowait", c.column_state),
            ("cpu.load.average.1m", c.load_average),
            ("cpu.logical.count", c.logical_cpus),
            ("cpu.tasks.runnable", c.running_processes),
        ];

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.cpu, c.overview_subtitle))
            .child(self.resource_card(c.cpu, &self.overview.cpu, cx))
            .child(
                self.surface(
                    v_flex().min_w_0().gap_1().children(
                        rows.into_iter()
                            .map(|(id, label)| {
                                let field = field::number(&metrics, &metric(id));
                                self.metric_row(
                                    format!("{label} · {id}"),
                                    cell(&field, |v| format!("{v:.3}")),
                                    cx,
                                )
                            })
                            .collect::<Vec<_>>(),
                    ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex().min_w_0().gap_1().children(
                        cores
                            .into_iter()
                            .map(|(key, busy, frequency)| {
                                h_flex()
                                    .min_w_0()
                                    .gap_3()
                                    .justify_between()
                                    .child(div().text_sm().child(format!("CPU {key}")))
                                    .child(cell_element(cell(&busy, |v| ratio_percent(*v)), c, cx))
                                    .child(cell_element(
                                        cell(&frequency, |v| format!("{:.2} GHz", v / 1e9)),
                                        c,
                                        cx,
                                    ))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                    ),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn memory_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let memory = self.overview.memory.clone();
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.memory, c.free_is_not_available))
            .child(self.resource_card(c.memory, &self.overview.memory.resource, cx))
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_1()
                        .child(self.byte_row(c.memory, &memory.total, cx))
                        .child(self.byte_row(c.available_memory, &memory.available, cx))
                        .child(self.byte_row(c.cached, &memory.cached, cx))
                        .child(self.byte_row(c.swap_used, &memory.swap_used, cx))
                        .child(self.metric_row(
                            c.major_faults.to_string(),
                            cell(&memory.major_fault_rate, |v| format!("{v:.1}/s")),
                            cx,
                        )),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn byte_row(
        &self,
        label: &'static str,
        field: &Field<u64>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.metric_row(label.to_string(), cell(field, |v| bytes(*v as f64)), cx)
    }

    fn device_page(
        &mut self,
        title: &'static str,
        collector: &str,
        kind: EntityKind,
        columns: [(&'static str, &'static str); 2],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let Some(report) = self.system_metrics(collector) else {
            return self.unsupported_page(title, c.verdict_unobserved, cx);
        };
        let devices = report
            .entities_of(kind)
            .map(|entity| {
                (
                    entity.id.key.clone(),
                    field::number(&entity.metrics, &metric(columns[0].0)),
                    field::number(&entity.metrics, &metric(columns[1].0)),
                )
            })
            .collect::<Vec<_>>();

        let body = if devices.is_empty() {
            v_flex().min_w_0().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.no_devices),
            )
        } else {
            v_flex().min_w_0().gap_2().children(
                devices
                    .into_iter()
                    .map(|(key, read, write)| {
                        h_flex()
                            .min_w_0()
                            .gap_3()
                            .justify_between()
                            .flex_wrap()
                            .child(div().text_sm().font_semibold().truncate().child(key))
                            .child(self.stat(
                                columns[0].1,
                                cell_element(cell(&read, |v| bytes_per_second(*v)), c, cx),
                                cx,
                            ))
                            .child(self.stat(
                                columns[1].1,
                                cell_element(cell(&write, |v| bytes_per_second(*v)), c, cx),
                                cx,
                            ))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            )
        };

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(title, c.overview_subtitle))
            .child(self.surface(body, cx))
            .into_any_element()
    }

    fn storage_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        self.device_page(
            c.storage,
            "linux.storage",
            EntityKind::BlockDevice,
            [
                ("storage.read.bytes.rate", c.read_throughput),
                ("storage.write.bytes.rate", c.write_throughput),
            ],
            cx,
        )
    }

    fn network_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        self.device_page(
            c.network,
            "linux.network",
            EntityKind::NetworkInterface,
            [
                ("network.rx.bytes.rate", c.received),
                ("network.tx.bytes.rate", c.sent),
            ],
            cx,
        )
    }

    fn diagnostics_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.diagnostics, c.coverage_title))
            .child(self.collector_health_card(cx))
            .into_any_element()
    }

    fn settings_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let order = self
            .precedence
            .order()
            .iter()
            .map(|kind| kind.key())
            .collect::<Vec<_>>()
            .join(" › ");

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.settings, c.sampling_description))
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_2()
                        .child(div().font_semibold().child(c.appearance))
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(Button::new("theme-dark").label(c.dark_theme).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.set_theme(gpui_component::ThemeMode::Dark, window, cx)
                                    }),
                                ))
                                .child(Button::new("theme-light").label(c.light_theme).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.set_theme(gpui_component::ThemeMode::Light, window, cx)
                                    }),
                                )),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_2()
                        .child(div().font_semibold().child(c.language))
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(
                                    Button::new("locale-system")
                                        .label(c.system_default)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_locale(crate::i18n::Locale::System, cx)
                                        })),
                                )
                                .child(Button::new("locale-en").label(c.english).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.set_locale(crate::i18n::Locale::EnUs, cx)
                                    }),
                                ))
                                .child(
                                    Button::new("locale-zh")
                                        .label(c.traditional_chinese)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_locale(crate::i18n::Locale::ZhTw, cx)
                                        })),
                                ),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_2()
                        .child(div().font_semibold().child(c.privacy))
                        .child(div().text_sm().child(c.privacy_description))
                        .child(
                            Switch::new("settings-command-lines")
                                .checked(self.include_command_lines)
                                .label(c.show_command_lines)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    this.set_command_lines(*checked, cx)
                                })),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_2()
                        .child(div().font_semibold().child(c.grouping_precedence))
                        .child(div().text_sm().child(c.grouping_precedence_description))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(order),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_2()
                        .child(div().font_semibold().child(c.sampling))
                        .child(div().text_sm().child(c.sampling_description)),
                    cx,
                ),
            )
            .into_any_element()
    }
}
