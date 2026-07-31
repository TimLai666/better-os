use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    progress::Progress,
    scroll::ScrollableElement,
    tag::Tag,
    *,
};
use manager_core::DesiredOperation;

use crate::{
    app::ManagerApp,
    i18n::copy,
    model::{InstallStep, Modal, Page},
};

impl ManagerApp {
    pub(crate) fn review_changes_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let planned_steps = self
            .pending_plan
            .as_ref()
            .map(|plan| plan.steps.clone())
            .unwrap_or_default();
        let planned_rows: Vec<(String, String)> = planned_steps
            .iter()
            .map(|step| {
                let operation = match step.operation {
                    DesiredOperation::Install => c.install,
                    DesiredOperation::Update => c.update,
                };
                (
                    self.plan_component_name(&step.component),
                    format!("{operation} · {}", self.plan_version_label(&step.component)),
                )
            })
            .collect();
        let files_touched = self.plan_paths();
        v_flex()
            .gap_5()
            .child(self.page_heading(c.review_title, c.review_subtitle, false, compact))
            .when_some(self.planning_error, |view, error| {
                view.child(self.planning_error_view(error, cx))
            })
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(div().text_lg().font_semibold().child(c.affected_components))
                        .children(planned_rows.into_iter().map(|(component, operation)| {
                            self.key_value_row(component, operation, cx)
                        })),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(
                        div().min_w(px(220.0)).flex_1().child(
                            self.surface(
                                v_flex()
                                    .gap_2()
                                    .child(c.download_size)
                                    .child(div().text_xl().font_semibold().child("25.3 MB")),
                                cx,
                            ),
                        ),
                    )
                    .child(
                        div().min_w(px(220.0)).flex_1().child(
                            self.surface(
                                v_flex()
                                    .gap_2()
                                    .child(c.dependencies)
                                    .child(div().text_xl().font_semibold().child(c.none)),
                                cx,
                            ),
                        ),
                    )
                    .child(
                        div().min_w(px(220.0)).flex_1().child(
                            self.surface(
                                v_flex()
                                    .gap_2()
                                    .child(c.restore_available)
                                    .child(Tag::success().rounded_full().child(c.supported)),
                                cx,
                            ),
                        ),
                    ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(div().font_semibold().child(c.files_touched))
                        .children(if files_touched.is_empty() {
                            vec![div().child(c.none).into_any_element()]
                        } else {
                            files_touched
                                .into_iter()
                                .map(|path| div().child(path).into_any_element())
                                .collect()
                        })
                        .child(
                            div()
                                .pt_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.before_installing),
                        ),
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
                            .label(c.install_updates)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.begin_install(cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn planning_error_view(
        &self,
        error: crate::app::PlanningError,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let detail = match error {
            crate::app::PlanningError::PreviewOnlyComponent => c.preview_only_component,
            crate::app::PlanningError::CorePlanningFailed => c.planning_failed_detail,
        };
        self.surface(
            v_flex()
                .gap_2()
                .child(div().font_semibold().child(c.planning_failed_title))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail),
                ),
            cx,
        )
    }

    fn install_step_label(&self, step: InstallStep) -> &'static str {
        let c = copy(self.locale);
        match step {
            InstallStep::Download => c.downloading,
            InstallStep::InstallFiles => c.installing_files,
            InstallStep::ApplySettings => c.applying_settings,
            InstallStep::Verify => c.checking_works,
        }
    }

    fn install_step_row(
        &self,
        step: InstallStep,
        active: bool,
        complete: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tag = if complete {
            Tag::success().small().rounded_full().child("✓")
        } else if active {
            Tag::info().small().rounded_full().child("•••")
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
                    .child(self.install_step_label(step)),
            )
            .into_any_element()
    }

    pub(crate) fn installing_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let current = self.install_step;
        let installation_rows = self
            .pending_plan
            .as_ref()
            .map(|plan| {
                plan.steps
                    .iter()
                    .map(|step| {
                        (
                            self.plan_component_name(&step.component),
                            self.plan_version_label(&step.component),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let completed = |step: InstallStep| -> bool {
            matches!(
                (current, step),
                (InstallStep::InstallFiles, InstallStep::Download)
                    | (InstallStep::ApplySettings, InstallStep::Download)
                    | (InstallStep::ApplySettings, InstallStep::InstallFiles)
                    | (InstallStep::Verify, InstallStep::Download)
                    | (InstallStep::Verify, InstallStep::InstallFiles)
                    | (InstallStep::Verify, InstallStep::ApplySettings)
            )
        };

        v_flex()
            .gap_5()
            .child(self.page_heading(c.installing_title, c.installing_subtitle, false, compact))
            .child(
                self.surface(
                    v_flex()
                        .gap_4()
                        .child(
                            h_flex()
                                .w_full()
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
                                                .child(self.install_step_label(current)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_2xl()
                                        .font_bold()
                                        .child(format!("{:.0}%", self.install_progress)),
                                ),
                        )
                        .child(Progress::new("install-progress").value(self.install_progress))
                        .child(
                            v_flex()
                                .gap_3()
                                .child(self.install_step_row(
                                    InstallStep::Download,
                                    current == InstallStep::Download,
                                    completed(InstallStep::Download),
                                    cx,
                                ))
                                .child(self.install_step_row(
                                    InstallStep::InstallFiles,
                                    current == InstallStep::InstallFiles,
                                    completed(InstallStep::InstallFiles),
                                    cx,
                                ))
                                .child(self.install_step_row(
                                    InstallStep::ApplySettings,
                                    current == InstallStep::ApplySettings,
                                    completed(InstallStep::ApplySettings),
                                    cx,
                                ))
                                .child(self.install_step_row(
                                    InstallStep::Verify,
                                    current == InstallStep::Verify,
                                    completed(InstallStep::Verify),
                                    cx,
                                )),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(div().font_semibold().child(c.installation_details))
                        .children(installation_rows.into_iter().map(|(component, version)| {
                            self.key_value_row(component, version, cx)
                        })),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .child(
                        Button::new("cancel-install")
                            .danger()
                            .outline()
                            .label(c.cancel_installation)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.modal = Modal::ConfirmCancelInstall;
                                cx.notify();
                            })),
                    )
                    .when(cfg!(debug_assertions), |row| {
                        row.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .flex_wrap()
                                .child(Tag::secondary().small().child(c.debug_preview))
                                .child(
                                    Button::new("preview-next-install-step")
                                        .label(c.preview_next_step)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.advance_install_preview(cx);
                                        })),
                                )
                                .child(
                                    Button::new("preview-install-failure")
                                        .danger()
                                        .label(c.preview_failure)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.fail_install_preview(cx);
                                        })),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn finished_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let finished_rows = self
            .pending_plan
            .as_ref()
            .map(|plan| {
                plan.steps
                    .iter()
                    .map(|step| {
                        (
                            self.plan_component_name(&step.component),
                            self.plan_version_label(&step.component),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
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
            .child(self.page_heading(c.finished_title, c.finished_subtitle, false, compact))
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(div().text_lg().font_semibold().child(c.changes_installed))
                        .children(
                            finished_rows.into_iter().map(|(component, version)| {
                                self.key_value_row(component, version, cx)
                            }),
                        )
                        .child(Tag::warning().rounded_full().child(c.logout_required)),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(Button::new("logout-now").primary().label(c.logout_now))
                    .child(
                        Button::new("logout-later")
                            .label(c.logout_later)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Overview, cx);
                            })),
                    )
                    .child(
                        Button::new("finished-overview")
                            .link()
                            .label(c.back_to_overview)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Overview, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn restore_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
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
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(div().text_lg().font_semibold().child(c.monitor_name))
                        .child(self.key_value_row(
                            c.health_check_label,
                            c.failed_service_detail,
                            cx,
                        ))
                        .child(self.key_value_row(c.previous_version_label, "0.1.0", cx))
                        .child(self.key_value_row(c.restore_available, c.supported, cx)),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        Button::new("restore-previous")
                            .danger()
                            .label(c.restore_previous)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Restored, cx);
                            })),
                    )
                    .child(
                        Button::new("retry-health-check")
                            .label(c.retry_check)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Finished, cx);
                            })),
                    )
                    .child(
                        Button::new("manual-recovery")
                            .link()
                            .label(c.manual_recovery)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.modal = Modal::ManualRecovery;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn restored_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
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
                        .gap_3()
                        .child(self.key_value_row(c.monitor_name, "0.1.0", cx))
                        .child(self.key_value_row(c.health_check_label, c.passed, cx))
                        .child(self.key_value_row(c.service_running, c.ready, cx)),
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

    pub(crate) fn modal_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale);
        let dialog = match self.modal {
            Modal::None => return None,
            Modal::ConfirmDisable(component_id) => {
                let component_name = crate::model::component_by_id(component_id)
                    .map(|component| self.component_name(component.id))
                    .unwrap_or(c.manager_name);
                v_flex()
                    .gap_4()
                    .child(div().text_xl().font_semibold().child(c.disable_title))
                    .child(div().text_sm().child(component_name))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(c.disable_body),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .justify_end()
                            .flex_wrap()
                            .child(
                                Button::new("close-disable-dialog")
                                    .label(c.cancel)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.modal = Modal::None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("confirm-disable")
                                    .danger()
                                    .label(c.disable_component)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.modal = Modal::None;
                                        this.navigate(Page::Components, cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            Modal::ConfirmCancelInstall => v_flex()
                .gap_4()
                .child(
                    div()
                        .text_xl()
                        .font_semibold()
                        .child(c.cancel_install_title),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.cancel_install_body),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .justify_end()
                        .flex_wrap()
                        .child(
                            Button::new("keep-installing")
                                .label(c.keep_installing)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.modal = Modal::None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("stop-and-restore")
                                .danger()
                                .label(c.stop_and_restore)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.modal = Modal::None;
                                    this.navigate(Page::Restored, cx);
                                })),
                        ),
                )
                .into_any_element(),
            Modal::ManualRecovery => v_flex()
                .gap_4()
                .child(div().text_xl().font_semibold().child(c.manual_steps_title))
                .child(self.bullet_row(IconName::Info, "1", c.manual_step_stop_service, cx))
                .child(self.bullet_row(IconName::Info, "2", c.manual_step_restore_package, cx))
                .child(self.bullet_row(IconName::Info, "3", c.manual_step_run_check, cx))
                .child(
                    h_flex().w_full().justify_end().child(
                        Button::new("close-manual-recovery")
                            .primary()
                            .label(c.close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.modal = Modal::None;
                                cx.notify();
                            })),
                    ),
                )
                .into_any_element(),
        };

        Some(
            div()
                .absolute()
                .inset_0()
                .p_4()
                .bg(hsla(0.62, 0.35, 0.12, 0.42))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(560.0))
                        .max_h(px(720.0))
                        .overflow_y_scrollbar()
                        .p_5()
                        .rounded(cx.theme().radius_lg)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .child(dialog),
                )
                .into_any_element(),
        )
    }
}
