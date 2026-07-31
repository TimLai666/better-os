use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName,
    button::{Button, ButtonVariants},
    switch::Switch,
    tag::Tag,
    *,
};

use crate::{
    app::ManagerApp,
    i18n::{Locale, copy},
    model::Page,
};

impl ManagerApp {
    pub(crate) fn health_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .gap_5()
            .child(self.page_heading(c.health_title, c.health_subtitle, false, compact))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(
                        div().min_w(px(300.0)).flex_1().child(
                            self.surface(
                                v_flex()
                                    .gap_3()
                                    .child(Tag::success().rounded_full().child(c.healthy))
                                    .child(
                                        div().text_lg().font_semibold().child(c.all_checks_passed),
                                    )
                                    .child(c.health_passed_detail),
                                cx,
                            ),
                        ),
                    )
                    .child(
                        div().min_w(px(300.0)).flex_1().child(
                            self.surface(
                                v_flex()
                                    .gap_3()
                                    .child(Tag::warning().rounded_full().child(c.update_available))
                                    .child(
                                        div().text_lg().font_semibold().child(c.one_check_failed),
                                    )
                                    .child(c.warning_detail)
                                    .child(
                                        Button::new("health-view-update")
                                            .label(c.view_recovery)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.navigate(Page::Updates, cx);
                                            })),
                                    ),
                                cx,
                            ),
                        ),
                    ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_lg()
                                .font_semibold()
                                .child(c.component_health_checks),
                        )
                        .child(self.bullet_row(
                            IconName::Check,
                            c.check_catalog,
                            c.passed_detail,
                            cx,
                        ))
                        .child(self.bullet_row(
                            IconName::Check,
                            c.check_services,
                            c.passed_detail,
                            cx,
                        ))
                        .child(self.bullet_row(
                            IconName::Check,
                            c.check_dependencies,
                            c.passed_detail,
                            cx,
                        ))
                        .child(self.bullet_row(
                            IconName::Check,
                            c.check_restore_data,
                            c.passed_detail,
                            cx,
                        )),
                    cx,
                ),
            )
            .child(
                Button::new("run-doctor")
                    .primary()
                    .icon(IconName::CircleCheck)
                    .label(c.run_doctor)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Page::DoctorResults, cx);
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn doctor_results_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let checks = [
            (c.check_catalog, c.passed_detail, true),
            (c.check_services, c.passed_detail, true),
            (c.check_dependencies, c.warning_detail, false),
            (c.check_permissions, c.passed_detail, true),
            (c.check_disk_space, c.passed_detail, true),
            (c.check_restore_data, c.passed_detail, true),
        ];

        v_flex()
            .gap_5()
            .child(self.page_heading(c.doctor_title, c.doctor_subtitle, false, compact))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .flex_wrap()
                    .child(self.metric_card(
                        c.checks_run,
                        "6",
                        c.all_checks_passed,
                        IconName::CircleCheck,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.issue_found,
                        "1",
                        c.warning_detail,
                        IconName::Info,
                        cx,
                    ))
                    .child(self.metric_card(
                        c.fixed_automatically,
                        "0",
                        c.needs_action,
                        IconName::Settings,
                        cx,
                    )),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_2()
                        .children(checks.into_iter().map(|(title, detail, passed)| {
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_3()
                                .items_start()
                                .justify_between()
                                .flex_wrap()
                                .py_3()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(
                                    v_flex()
                                        .min_w(px(280.0))
                                        .flex_1()
                                        .gap_1()
                                        .child(div().font_semibold().child(title))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(detail),
                                        ),
                                )
                                .child(if passed {
                                    Tag::success().small().rounded_full().child(c.passed)
                                } else {
                                    Tag::warning().small().rounded_full().child(c.needs_action)
                                })
                                .into_any_element()
                        })),
                    cx,
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(Button::new("doctor-run-again").primary().label(c.run_again))
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

    fn locale_button(
        &self,
        id: &'static str,
        locale: Locale,
        label: &'static str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.locale == locale)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_locale(locale, window, cx);
            }))
    }

    pub(crate) fn settings_page(
        &self,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .gap_5()
            .child(self.page_heading(c.settings_title, c.settings_subtitle, false, compact))
            .child(
                self.surface(
                    v_flex()
                        .gap_4()
                        .child(div().text_lg().font_semibold().child(c.language))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.language_description),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .flex_wrap()
                                .child(self.locale_button(
                                    "locale-system",
                                    Locale::System,
                                    c.system_default,
                                    window,
                                    cx,
                                ))
                                .child(self.locale_button(
                                    "locale-english",
                                    Locale::EnUs,
                                    c.english,
                                    window,
                                    cx,
                                ))
                                .child(self.locale_button(
                                    "locale-zh-tw",
                                    Locale::ZhTw,
                                    c.traditional_chinese,
                                    window,
                                    cx,
                                )),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_4()
                        .child(div().text_lg().font_semibold().child(c.updates_section))
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_4()
                                .items_center()
                                .justify_between()
                                .flex_wrap()
                                .child(div().min_w(px(260.0)).flex_1().child(c.update_checks))
                                .child(
                                    Switch::new("setting-check-updates")
                                        .checked(self.check_updates)
                                        .on_click(cx.listener(|this, checked, _, cx| {
                                            this.check_updates = *checked;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_4()
                                .items_center()
                                .justify_between()
                                .flex_wrap()
                                .child(
                                    div()
                                        .min_w(px(260.0))
                                        .flex_1()
                                        .child(c.download_automatically),
                                )
                                .child(
                                    Switch::new("setting-auto-download")
                                        .checked(self.auto_download)
                                        .on_click(cx.listener(|this, checked, _, cx| {
                                            this.auto_download = *checked;
                                            cx.notify();
                                        })),
                                ),
                        ),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_4()
                        .child(div().text_lg().font_semibold().child(c.diagnostics_section))
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_4()
                                .items_center()
                                .justify_between()
                                .flex_wrap()
                                .child(div().min_w(px(260.0)).flex_1().child(c.diagnostic_logs))
                                .child(
                                    Switch::new("setting-diagnostic-logs")
                                        .checked(self.diagnostic_logs)
                                        .on_click(cx.listener(|this, checked, _, cx| {
                                            this.diagnostic_logs = *checked;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.privacy_note),
                        )
                        .when(cfg!(debug_assertions), |view| {
                            view.child(
                                Button::new("open-edge-previews")
                                    .label(c.open_edge_previews)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.navigate(Page::EdgeStates, cx);
                                    })),
                            )
                        }),
                    cx,
                ),
            )
            .into_any_element()
    }

    pub(crate) fn edge_states_page(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        v_flex()
            .gap_5()
            .child(self.page_heading(c.edge_states_title, c.edge_states_subtitle, false, compact))
            .child(
                self.empty_state(
                    IconName::Info,
                    c.offline_title,
                    c.offline_subtitle,
                    h_flex()
                        .gap_2()
                        .items_center()
                        .justify_center()
                        .flex_wrap()
                        .child(
                            Button::new("offline-retry")
                                .primary()
                                .label(c.retry_connection),
                        )
                        .child(Button::new("offline-cache").label(c.cached_catalog)),
                    cx,
                ),
            )
            .child(
                self.empty_state(
                    IconName::Inbox,
                    c.empty_components_title,
                    c.empty_components_subtitle,
                    Button::new("empty-back-settings")
                        .label(c.back)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate(Page::Settings, cx);
                        })),
                    cx,
                ),
            )
            .child(
                self.empty_state(
                    IconName::Check,
                    c.no_updates_title,
                    c.no_updates_subtitle,
                    Button::new("edge-preview-no-updates")
                        .label(c.view)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_no_updates = true;
                            this.navigate(Page::Updates, cx);
                        })),
                    cx,
                ),
            )
            .child(
                self.empty_state(
                    IconName::WindowRestore,
                    c.no_activity_title,
                    c.no_activity_subtitle,
                    Button::new("edge-preview-no-activity")
                        .label(c.view)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.activity_cleared = true;
                            this.navigate(Page::Activity, cx);
                        })),
                    cx,
                ),
            )
            .into_any_element()
    }
}
