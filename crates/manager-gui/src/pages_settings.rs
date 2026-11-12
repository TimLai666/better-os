use better_ui::{StatusPill, StatusTone};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName,
    button::{Button, ButtonVariants},
    *,
};
use manager_core::{DoctorCheck, DoctorCheckKind, DoctorCheckStatus};

use crate::{
    app::ManagerApp,
    i18n::copy,
    model::{Page, package_system_notice, repair_result_line, unorderable_detail},
};

impl ManagerApp {
    pub(crate) fn health_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let checks = self.doctor_checks();
        let failed = checks
            .iter()
            .filter(|check| check.status == DoctorCheckStatus::Failed)
            .count();
        let warnings = checks
            .iter()
            .filter(|check| check.status == DoctorCheckStatus::Warning)
            .count();
        let heading = if failed > 0 {
            c.one_check_failed
        } else {
            c.all_checks_passed
        };
        v_flex()
            .gap_5()
            .child(self.page_heading(c.health_title, c.health_subtitle, false, compact))
            .when_some(self.error_banner(cx), |view, error| view.child(error))
            // An interrupted dpkg run is why every install would fail, so it
            // goes above the checks rather than among them.
            .when_some(self.package_system_card(cx), |view, card| view.child(card))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(self.metric_card(
                        c.checks_run,
                        checks.len().to_string(),
                        heading,
                        IconName::CircleCheck,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.issue_found,
                        failed.to_string(),
                        c.failed,
                        IconName::Info,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.warnings,
                        warnings.to_string(),
                        c.needs_action,
                        IconName::Info,
                        cx,
                    )),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_lg()
                                .font_semibold()
                                .child(c.component_health_checks),
                        )
                        .children(checks.iter().map(|check| self.doctor_row(check, cx))),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        Button::new("run-doctor")
                            .primary()
                            .icon(IconName::CircleCheck)
                            .label(c.run_doctor)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::DoctorResults, cx);
                            })),
                    )
                    .when(failed > 0, |row| {
                        row.child(
                            Button::new("health-recovery")
                                .label(c.view_recovery)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.navigate(Page::Restore, cx);
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn doctor_results_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let checks = self.doctor_checks();
        let failed = checks
            .iter()
            .filter(|check| check.status == DoctorCheckStatus::Failed)
            .count();
        v_flex()
            .gap_5()
            .child(self.page_heading(c.doctor_title, c.doctor_subtitle, false, compact))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(self.metric_card(
                        c.checks_run,
                        checks.len().to_string(),
                        c.component_health_checks,
                        IconName::CircleCheck,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.issue_found,
                        failed.to_string(),
                        if failed == 0 {
                            c.all_checks_passed
                        } else {
                            c.needs_action
                        },
                        IconName::Info,
                        cx,
                    )),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_2()
                        .children(checks.iter().map(|check| self.doctor_row(check, cx))),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        Button::new("doctor-run-again")
                            .primary()
                            .label(c.run_again)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::DoctorResults, cx);
                            })),
                    )
                    .child(
                        Button::new("doctor-back-health")
                            .label(c.back_to_health)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Page::Health, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// What dpkg left unfinished, and the one control that finishes it.
    ///
    /// Drawn while the privileged service reports an interrupted run, and after
    /// a repair to say how it ended. The heading and lines are status; the
    /// button is the only thing on the card that does anything, and it asks
    /// `manager-core`, which asks the service. Nothing here runs dpkg.
    pub(crate) fn package_system_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale);
        let notice = self
            .package_system
            .as_ref()
            .and_then(|state| package_system_notice(self.locale, state));
        let result = self
            .repair_result
            .as_ref()
            .map(|result| repair_result_line(self.locale, result));
        if notice.is_none() && result.is_none() {
            return None;
        }
        let repairing = self.repair_task.is_some();
        let busy = repairing || self.running.is_some();
        Some(
            self.surface(
                v_flex()
                    .gap_2()
                    .when_some(notice, |view, notice| {
                        view.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .flex_wrap()
                                .child(
                                    Icon::new(IconName::TriangleAlert)
                                        .small()
                                        .text_color(cx.theme().yellow),
                                )
                                .child(div().text_lg().font_semibold().child(notice.title)),
                        )
                        .children(notice.lines.into_iter().map(|line| {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(line)
                        }))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(notice.consequence),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.repair_needs_password),
                        )
                        .child(
                            h_flex().child(
                                Button::new("package-system-repair")
                                    .primary()
                                    .label(if repairing {
                                        c.repairing
                                    } else {
                                        c.repair_package_system
                                    })
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.repair_package_system(cx);
                                    })),
                            ),
                        )
                    })
                    .when_some(result, |view, result| {
                        view.child(div().text_sm().child(result))
                    }),
                cx,
            ),
        )
    }

    /// How one doctor check result reads. It reports what a check found, so it
    /// is a pill: nothing about it can be clicked.
    pub(crate) fn doctor_pill(
        c: &'static crate::i18n::Copy,
        status: DoctorCheckStatus,
    ) -> StatusPill {
        let (label, tone) = match status {
            DoctorCheckStatus::Passed => (c.passed, StatusTone::Success),
            DoctorCheckStatus::Warning => (c.warnings, StatusTone::Warning),
            DoctorCheckStatus::Failed => (c.failed, StatusTone::Danger),
        };
        StatusPill::new(label, tone)
    }

    fn doctor_row(&self, check: &DoctorCheck, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let label = match check.kind {
            DoctorCheckKind::Catalog => c.check_catalog,
            DoctorCheckKind::Compatibility => c.compatibility,
            DoctorCheckKind::ComponentHealth => c.component_health_checks,
            DoctorCheckKind::RestoreData => c.restore_available,
            DoctorCheckKind::HostReconciliation => c.check_host_reconciliation,
            DoctorCheckKind::UnorderableHostVersion => c.check_unorderable_version,
        };
        let detail = unorderable_detail(self.locale, check);
        let status = self.pill(Self::doctor_pill(c, check.status), cx);
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_center()
            .justify_between()
            .flex_wrap()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .min_w(px(220.0))
                    .flex_1()
                    .gap_1()
                    .child(div().font_semibold().child(label))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.component_name_for_core(check.component.as_ref())),
                    )
                    .when_some(detail, |view, detail| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(detail),
                        )
                    }),
            )
            .child(status)
            .into_any_element()
    }
}
