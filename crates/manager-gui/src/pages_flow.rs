use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    progress::Progress,
    tag::Tag,
    *,
};
use manager_core::{DesiredOperation, OperationStage, RecoveryStatus};

use crate::{app::ManagerApp, i18n::copy, model::Page};

impl ManagerApp {
    pub(crate) fn review_changes_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let steps = self.pending_steps();
        let approval_label = if steps
            .iter()
            .all(|step| step.operation == DesiredOperation::Update)
        {
            c.install_updates
        } else {
            c.apply_changes
        };

        v_flex()
            .gap_5()
            .child(self.page_heading(c.review_title, c.review_subtitle, false, compact))
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .when(steps.is_empty(), |view| {
                view.child(
                    self.empty_state(
                        IconName::Info,
                        c.planning_failed_title,
                        c.planning_failed_detail,
                        Button::new("review-back-updates")
                            .label(c.back)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Updates, cx);
                            })),
                        cx,
                    ),
                )
            })
            .when(!steps.is_empty(), |view| {
                view.child(
                    self.surface(
                        v_flex()
                            .gap_3()
                            .child(div().text_lg().font_semibold().child(c.affected_components))
                            .children(steps.iter().map(|step| {
                                self.key_value_row(
                                    self.plan_component_name(&step.component),
                                    self.operation_label(step.operation),
                                    cx,
                                )
                            })),
                        cx,
                    ),
                )
                .child(
                    self.surface(
                        v_flex()
                            .gap_1()
                            .child(self.key_value_row(
                                c.required_disk_space,
                                self.disk_space_label(self.pending_disk_space()),
                                cx,
                            ))
                            .child(self.key_value_row(
                                c.restart_requirement,
                                self.restart_requirement_label(self.pending_restart_requirement()),
                                cx,
                            )),
                        cx,
                    ),
                )
                .children(steps.iter().map(|step| self.review_step(step, cx)))
                .child(
                    self.surface(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.mock_lifecycle_detail),
                        cx,
                    ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .items_center()
                        .justify_end()
                        .flex_wrap()
                        .child(
                            Button::new("cancel-review")
                                .label(c.cancel)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.navigate(Page::Updates, cx);
                                })),
                        )
                        .child(
                            Button::new("approve-install")
                                .primary()
                                .label(approval_label)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.begin_install(cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }

    fn review_step(&self, step: &manager_core::PlanStep, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let dependencies = if step.dependencies.is_empty() {
            c.none.to_string()
        } else {
            step.dependencies
                .iter()
                .map(|id| self.plan_component_name(id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let conflicts = if step.conflicts.is_empty() {
            c.none.to_string()
        } else {
            step.conflicts
                .iter()
                .map(|id| self.plan_component_name(id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let paths = if step.paths.is_empty() {
            c.none.to_string()
        } else {
            step.paths.join(", ")
        };
        let download = self.byte_count_label(step.estimated_download_bytes);
        let required_disk = self.byte_count_label(step.required_disk_bytes);
        let release_notes = if step.release_notes.is_empty() {
            c.no_release_notes.to_string()
        } else {
            step.release_notes.join("\n")
        };
        self.surface(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .child(self.plan_component_name(&step.component)),
                )
                .child(
                    self.key_value_row(
                        c.before_version,
                        step.before_version
                            .clone()
                            .unwrap_or_else(|| c.none.to_string()),
                        cx,
                    ),
                )
                .child(
                    self.key_value_row(
                        c.after_version,
                        step.after_version
                            .clone()
                            .unwrap_or_else(|| c.none.to_string()),
                        cx,
                    ),
                )
                .child(self.key_value_row(c.dependencies, dependencies, cx))
                .child(self.key_value_row(c.conflicts, conflicts, cx))
                .child(self.key_value_row(
                    c.replaces_label,
                    if step.replaces.is_empty() {
                        c.none.to_string()
                    } else {
                        step.replaces.join(", ")
                    },
                    cx,
                ))
                .child(self.key_value_row(
                    c.enhances_label,
                    if step.enhances.is_empty() {
                        c.none.to_string()
                    } else {
                        step.enhances.join(", ")
                    },
                    cx,
                ))
                .child(self.key_value_row(c.files_touched, paths, cx))
                .child(self.key_value_row(
                    c.restart_requirement,
                    self.restart_requirement_label(step.restart_requirement),
                    cx,
                ))
                .child(self.key_value_row(c.download_size, download, cx))
                .child(self.key_value_row(c.required_disk_space, required_disk, cx))
                .child(self.key_value_row(c.release_notes, release_notes, cx))
                .child(self.key_value_row(
                    c.rollback,
                    if step.rollback_available {
                        c.restore_available
                    } else {
                        c.none
                    },
                    cx,
                )),
            cx,
        )
    }

    pub(crate) fn installing_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let current = self.stage();
        let current_label = current
            .map(|stage| self.stage_label(stage))
            .unwrap_or(c.none);
        let progress = current.map(stage_progress).unwrap_or(0.0);
        let active_steps = self.active_steps();
        v_flex()
            .gap_5()
            .child(self.page_heading(c.installing_title, c.mock_lifecycle_detail, false, compact))
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .when(current.is_none(), |view| {
                view.child(
                    self.empty_state(
                        IconName::Info,
                        c.planning_failed_title,
                        c.planning_failed_detail,
                        Button::new("installing-updates")
                            .label(c.back)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Updates, cx);
                            })),
                        cx,
                    ),
                )
            })
            .when(current.is_some(), |view| {
                view.child(
                    self.surface(
                        v_flex()
                            .gap_4()
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_3()
                                    .items_center()
                                    .justify_between()
                                    .flex_wrap()
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(c.current_step),
                                            )
                                            .child(
                                                div()
                                                    .text_lg()
                                                    .font_semibold()
                                                    .child(current_label),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_bold()
                                            .child(format!("{:.0}%", progress * 100.0)),
                                    ),
                            )
                            .child(Progress::new("install-progress").value(progress))
                            .children(
                                OperationStage::ALL
                                    .into_iter()
                                    .map(|stage| self.install_stage_row(stage, current, cx)),
                            ),
                        cx,
                    ),
                )
                .child(
                    self.surface(
                        v_flex()
                            .gap_2()
                            .child(div().font_semibold().child(c.installation_details))
                            .children(active_steps.iter().map(|step| {
                                self.key_value_row(
                                    self.plan_component_name(&step.component),
                                    self.operation_label(step.operation),
                                    cx,
                                )
                            })),
                        cx,
                    ),
                )
                // What the transaction is actually moving right now. Only a
                // real run has bytes to report; a simulation moves nothing.
                .when_some(self.transfer.clone(), |this, transfer| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(match transfer.total_bytes {
                                Some(total) => format!(
                                    "{} {} — {} / {}",
                                    c.downloading_progress,
                                    transfer.component,
                                    transfer.received_bytes,
                                    total
                                ),
                                None => format!(
                                    "{} {} — {}",
                                    c.downloading_progress,
                                    transfer.component,
                                    transfer.received_bytes
                                ),
                            }),
                    )
                })
                .child(
                    h_flex()
                        .gap_3()
                        .flex_wrap()
                        // Cancelling is only offered while it can still be
                        // honored. Past that point the machine may already have
                        // changed, and the button would promise a restoration
                        // nothing performed.
                        .when(self.can_cancel_now(), |this| {
                            this.child(
                                Button::new("cancel-install")
                                    .danger()
                                    .outline()
                                    .label(c.cancel_installation)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_install(cx);
                                    })),
                            )
                        })
                        // A real transaction advances itself as the work
                        // completes; only the simulation is stepped by hand.
                        .when(self.is_demo(), |this| {
                            this.child(
                                Button::new("continue-install")
                                    .primary()
                                    .label(c.continue_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.advance_install(cx);
                                    })),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn install_stage_row(
        &self,
        stage: OperationStage,
        current: Option<OperationStage>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = current == Some(stage);
        let complete =
            current.is_some_and(|current| stage_progress(current) > stage_progress(stage));
        let tag = if complete {
            Tag::success().small().rounded_full().child("✓")
        } else if active {
            Tag::info().small().rounded_full().child("…")
        } else {
            Tag::secondary().small().rounded_full().child("○")
        };
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_center()
            .child(tag)
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .font_semibold()
                    .text_color(if active {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(self.stage_label(stage)),
            )
            .into_any_element()
    }

    pub(crate) fn finished_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let steps = self.pending_steps();
        v_flex()
            .gap_5()
            .items_start()
            .child(
                div()
                    .size(px(56.0))
                    .rounded_full()
                    .bg(cx.theme().green_light)
                    .text_color(cx.theme().green)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(IconName::Check).large()),
            )
            .child(self.page_heading(c.finished, c.finished_subtitle, false, compact))
            .child(
                self.surface(
                    v_flex()
                        .gap_2()
                        .child(div().font_semibold().child(c.finished_subtitle))
                        .children(steps.iter().map(|step| {
                            self.key_value_row(
                                self.plan_component_name(&step.component),
                                self.operation_label(step.operation),
                                cx,
                            )
                        }))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.no_privileged_actions),
                        ),
                    cx,
                ),
            )
            .child(
                Button::new("finished-overview")
                    .primary()
                    .label(c.back_to_overview)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Page::Overview, cx);
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn restore_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some((component, failure)) = self.current_failure() else {
            return v_flex()
                .gap_5()
                .child(self.page_heading(c.restore_title, c.restore_subtitle, false, compact))
                .child(
                    self.empty_state(
                        IconName::Check,
                        c.all_checks_passed,
                        c.no_updates_subtitle,
                        Button::new("restore-overview")
                            .label(c.back_to_overview)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Overview, cx);
                            })),
                        cx,
                    ),
                )
                .into_any_element();
        };
        let restore_available = self
            .state
            .component(&component)
            .and_then(|record| record.restore_snapshot.as_ref())
            .is_some();
        let recovery_detail = match failure.recovery {
            Some(RecoveryStatus::PartiallyRestored) => c.recovery_partial,
            Some(RecoveryStatus::ManualRecoveryRequired) => c.manual_recovery_required,
            _ => c.restore_available,
        };
        v_flex()
            .gap_5()
            .items_start()
            .child(
                div()
                    .size(px(56.0))
                    .rounded_full()
                    .bg(cx.theme().red_light)
                    .text_color(cx.theme().red)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(IconName::Info).large()),
            )
            .child(self.page_heading(c.restore_title, c.restore_subtitle, false, compact))
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            .child(
                self.surface(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_lg()
                                .font_semibold()
                                .child(self.plan_component_name(&component)),
                        )
                        .child(self.key_value_row(
                            c.failed_stage,
                            self.stage_label(failure.stage),
                            cx,
                        ))
                        .child(self.key_value_row(
                            c.failure_evidence,
                            self.evidence_label(Some(&failure.evidence)),
                            cx,
                        ))
                        .child(self.key_value_row(c.restore_available, recovery_detail, cx)),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .when(restore_available, |row| {
                        let component = component.clone();
                        row.child(
                            Button::new("restore-previous")
                                .danger()
                                .label(c.restore_previous)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.prepare_recovery(
                                        component.clone(),
                                        DesiredOperation::Restore,
                                        cx,
                                    );
                                })),
                        )
                    })
                    .child(
                        Button::new("retry-health-check")
                            .label(c.retry_check)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.prepare_recovery(
                                    component.clone(),
                                    DesiredOperation::Verify,
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn restored_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let steps = self.pending_steps();
        v_flex()
            .gap_5()
            .items_start()
            .child(
                div()
                    .size(px(56.0))
                    .rounded_full()
                    .bg(cx.theme().green_light)
                    .text_color(cx.theme().green)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(IconName::Check).large()),
            )
            .child(self.page_heading(c.restored_title, c.restored_subtitle, false, compact))
            .child(
                self.surface(
                    v_flex()
                        .gap_2()
                        .children(steps.iter().map(|step| {
                            self.key_value_row(
                                self.plan_component_name(&step.component),
                                step.after_version
                                    .clone()
                                    .unwrap_or_else(|| c.none.to_string()),
                                cx,
                            )
                        }))
                        .child(self.key_value_row(c.health_check_label, c.passed, cx)),
                    cx,
                ),
            )
            .child(
                Button::new("restored-overview")
                    .primary()
                    .label(c.back_to_overview)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Page::Overview, cx);
                    })),
            )
            .into_any_element()
    }
}

fn stage_progress(stage: OperationStage) -> f32 {
    match stage {
        OperationStage::Downloading => 0.25,
        OperationStage::Installing => 0.5,
        OperationStage::ApplyingSettings => 0.75,
        OperationStage::CheckingHealth => 1.0,
    }
}
