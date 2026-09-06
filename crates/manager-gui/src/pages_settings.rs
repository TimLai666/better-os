use better_ui::{StatusPill, StatusTone};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName,
    button::{Button, ButtonVariants},
    switch::Switch,
    *,
};
use manager_core::{DoctorCheck, DoctorCheckKind, DoctorCheckStatus, ReleaseChannel, StoredTheme};

use crate::{
    app::ManagerApp,
    i18n::{Locale, copy},
    model::{AboutInfo, Page},
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
        };
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
                    ),
            )
            .child(status)
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
        let settings = &self.state.settings;
        v_flex()
            .gap_5()
            .child(self.page_heading(c.settings_title, c.settings_subtitle, false, compact))
            .when_some(self.error_banner(cx), |view, error| view.child(error))
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
                        .child(div().text_lg().font_semibold().child(c.appearance))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.appearance_description),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(self.theme_button(
                                    "theme-dark",
                                    StoredTheme::Dark,
                                    c.dark_theme,
                                    window,
                                    cx,
                                ))
                                .child(self.theme_button(
                                    "theme-light",
                                    StoredTheme::Light,
                                    c.light_theme,
                                    window,
                                    cx,
                                ))
                                .child(self.theme_button(
                                    "theme-system",
                                    StoredTheme::System,
                                    c.system_default,
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
                        .child(div().text_sm().font_medium().child(c.release_channel))
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(self.release_channel_button(
                                    "channel-stable",
                                    ReleaseChannel::Stable,
                                    c.stable_channel,
                                    cx,
                                ))
                                .child(self.release_channel_button(
                                    "channel-preview",
                                    ReleaseChannel::Preview,
                                    c.preview_channel,
                                    cx,
                                )),
                        )
                        .child(self.setting_switch(
                            "setting-check-updates",
                            c.update_checks,
                            settings.check_updates,
                            |this, cx| this.toggle_check_updates(cx),
                            cx,
                        ))
                        .child(self.setting_switch(
                            "setting-auto-download",
                            c.download_automatically,
                            settings.auto_download,
                            |this, cx| this.toggle_auto_download(cx),
                            cx,
                        )),
                    cx,
                ),
            )
            .child(
                self.surface(
                    v_flex()
                        .gap_4()
                        .child(div().text_lg().font_semibold().child(c.diagnostics_section))
                        .child(self.setting_switch(
                            "setting-diagnostic-logs",
                            c.diagnostic_logs,
                            settings.diagnostic_logs,
                            |this, cx| this.toggle_diagnostic_logs(cx),
                            cx,
                        ))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.privacy_note),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(c.mock_lifecycle_detail),
                        ),
                    cx,
                ),
            )
            .child(self.about_section(cx))
            .into_any_element()
    }

    /// What this build is, and which machine it decided it is running on.
    ///
    /// The version comes from the package this binary was built from, and the
    /// platform line is the profile the manager actually planned against — the
    /// same values the sidebar footer shows, not a second reading of the host.
    fn about_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let about = AboutInfo::present(self.locale, self.manager.profile());
        self.surface(
            v_flex()
                .gap_4()
                .child(div().text_lg().font_semibold().child(c.about_section))
                .child(div().font_medium().child(about.name))
                .child(self.key_value_row(c.about_version, about.version, cx))
                .child(self.key_value_row(c.about_platform, about.platform, cx))
                .child(self.key_value_row(c.about_repository, about.repository, cx)),
            cx,
        )
    }

    fn theme_button(
        &self,
        id: &'static str,
        theme: StoredTheme,
        label: &'static str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.state.settings.theme == theme)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_theme(theme, window, cx);
            }))
    }

    fn release_channel_button(
        &self,
        id: &'static str,
        channel: ReleaseChannel,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(self.state.settings.release_channel == channel)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_release_channel(channel, cx);
            }))
    }

    fn setting_switch(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        on_change: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .items_center()
            .justify_between()
            .flex_wrap()
            .child(div().min_w(px(220.0)).flex_1().child(label))
            .child(
                Switch::new(id)
                    .checked(checked)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        on_change(this, cx);
                    })),
            )
            .into_any_element()
    }
}
