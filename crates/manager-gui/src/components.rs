use better_core::ComponentIcon;
use better_ui::{BadgeStyle, StatusPill, StatusTone};
use gpui::*;
use gpui_component::{ActiveTheme, Icon, IconName, *};
use manager_core::{ComponentStatus, DesiredOperation, RestartRequirement};

use crate::{
    app::ManagerApp,
    i18n::{Locale, copy},
    model::ComponentKind,
};

impl ManagerApp {
    pub(crate) fn surface(&self, child: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        better_ui::surface(
            child,
            cx.theme().border,
            cx.theme().background,
            cx.theme().radius,
        )
        .into_any_element()
    }

    pub(crate) fn metric_card(
        &self,
        label: &'static str,
        value: String,
        detail: &'static str,
        icon: IconName,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .min_w(px(210.0))
            .flex_1()
            .child(
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_3()
                        .child(
                            h_flex()
                                .min_w_0()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(label),
                                )
                                .child(Icon::new(icon).small().text_color(cx.theme().primary)),
                        )
                        .child(div().text_2xl().font_bold().child(value))
                        .child(
                            div()
                                .min_w_0()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(detail),
                        ),
                    cx,
                ),
            )
            .into_any_element()
    }

    pub(crate) fn key_value_row(
        &self,
        label: impl IntoElement,
        value: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .items_start()
            .justify_between()
            .flex_wrap()
            .py_2()
            .child(
                div()
                    .min_w(px(180.0))
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().min_w_0().text_sm().font_medium().child(value))
            .into_any_element()
    }

    pub(crate) fn empty_state(
        &self,
        icon: IconName,
        title: &'static str,
        detail: &'static str,
        action: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.surface(
            v_flex()
                .w_full()
                .items_center()
                .text_center()
                .gap_3()
                .py_8()
                .child(
                    div()
                        .size_11()
                        .rounded_full()
                        .bg(cx.theme().secondary)
                        .text_color(cx.theme().primary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).large()),
                )
                .child(div().text_lg().font_semibold().child(title))
                .child(
                    div()
                        .max_w(px(560.0))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail),
                )
                .child(action),
            cx,
        )
    }

    /// The colors one status tone is drawn in.
    ///
    /// Deliberately *not* the fill a button of the same tone would use. A
    /// status is tinted and outlined — the border and the text carry the tone,
    /// the background only hints at it — so a `danger` status and a `danger`
    /// action can never be told apart by shape alone. `better_ui::Affordance`
    /// states the rule this implements.
    fn status_pill_style(&self, tone: StatusTone, cx: &mut Context<Self>) -> BadgeStyle {
        let accent = match tone {
            StatusTone::Neutral => {
                return BadgeStyle {
                    foreground: cx.theme().muted_foreground,
                    background: cx.theme().secondary,
                    border: cx.theme().border,
                };
            }
            StatusTone::Info => cx.theme().info,
            StatusTone::Success => cx.theme().success,
            StatusTone::Warning => cx.theme().warning,
            StatusTone::Danger => cx.theme().danger,
        };
        BadgeStyle {
            foreground: accent,
            background: accent.opacity(0.12),
            border: accent.opacity(0.4),
        }
    }

    /// Draw one read-only status indicator.
    ///
    /// Every status in this window goes through here, which is what keeps the
    /// promise that none of them responds to a pointer.
    pub(crate) fn pill(&self, pill: StatusPill, cx: &mut Context<Self>) -> AnyElement {
        let style = self.status_pill_style(pill.tone, cx);
        pill.render(style).into_any_element()
    }

    /// How one component's state reads. An associated function taking a
    /// locale rather than a method, so a test can enumerate every state
    /// without opening a window.
    pub(crate) fn status_pill(
        locale: Locale,
        status: ComponentStatus,
        pending: bool,
    ) -> StatusPill {
        let c = copy(locale);
        if pending {
            return StatusPill::new(c.ready_to_install, StatusTone::Info);
        }
        let (label, tone) = match status {
            ComponentStatus::Available => (c.available, StatusTone::Info),
            ComponentStatus::Downloading => (c.downloading, StatusTone::Info),
            ComponentStatus::ReadyToInstall => (c.ready_to_install, StatusTone::Info),
            ComponentStatus::Installing => (c.installing_files, StatusTone::Info),
            ComponentStatus::Verifying => (c.checking_works, StatusTone::Info),
            ComponentStatus::Healthy => (c.healthy, StatusTone::Success),
            ComponentStatus::UpdateAvailable => (c.update_available, StatusTone::Warning),
            ComponentStatus::Disabled => (c.disabled, StatusTone::Neutral),
            ComponentStatus::Incompatible => (c.incompatible, StatusTone::Danger),
            ComponentStatus::Degraded => (c.degraded, StatusTone::Warning),
            ComponentStatus::Failed => (c.failed, StatusTone::Danger),
            ComponentStatus::RestoreAvailable => (c.restore_available_status, StatusTone::Danger),
        };
        StatusPill::new(label, tone)
    }

    pub(crate) fn kind_pill(locale: Locale, kind: ComponentKind) -> StatusPill {
        let c = copy(locale);
        let (label, tone) = match kind {
            ComponentKind::Replacement => (c.replacement, StatusTone::Info),
            ComponentKind::Enhancement => (c.enhancement, StatusTone::Success),
            ComponentKind::Diagnostic => (c.diagnostic, StatusTone::Neutral),
        };
        StatusPill::new(label, tone)
    }

    #[cfg(test)]
    pub(crate) fn health_pill(locale: Locale, health: manager_core::HealthState) -> StatusPill {
        let c = copy(locale);
        let (label, tone) = match health {
            manager_core::HealthState::Healthy => (c.healthy, StatusTone::Success),
            manager_core::HealthState::Degraded => (c.degraded, StatusTone::Warning),
            manager_core::HealthState::Failed => (c.failed, StatusTone::Danger),
        };
        StatusPill::new(label, tone)
    }

    /// Maps the icon a manifest declares onto a shipped glyph. The manifest
    /// chooses from a closed set, so this never guesses from a component ID.
    pub(crate) fn component_icon(&self, icon: ComponentIcon) -> IconName {
        match icon {
            ComponentIcon::Manager => IconName::Settings,
            ComponentIcon::Monitor => IconName::Inspector,
            ComponentIcon::Files => IconName::Folder,
            ComponentIcon::Launcher => IconName::LayoutDashboard,
            ComponentIcon::Touchpad => IconName::Frame,
            ComponentIcon::Generic => IconName::Inbox,
        }
    }

    /// What happens after `operation`, for the "Afterwards" row; `None`
    /// hides the row. See [`restart_effect`].
    pub(crate) fn restart_requirement_label(
        &self,
        operation: DesiredOperation,
        requirement: RestartRequirement,
    ) -> Option<&'static str> {
        restart_effect(self.locale, operation, requirement)
    }

    pub(crate) fn error_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.planning_error.map(|_| {
            self.surface(
                h_flex()
                    .min_w_0()
                    .gap_3()
                    .items_start()
                    .child(Icon::new(IconName::Info).small().text_color(cx.theme().red))
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.error_message()),
                    ),
                cx,
            )
        })
    }
}

/// What happens after `operation`, given the manifest's restart requirement.
///
/// `manager-core` attaches the manifest's requirement to every operation that
/// changes a component, but ticket 105's automatic reopening only happens
/// when a running app is *updated*: the window notices its own file was
/// replaced and starts the new program. So only an update says it reopens by
/// itself, and only "if it is open"; installing, enabling, or restoring takes
/// effect the next time the app opens; disabling or removing an app that
/// only needed restarting has nothing left to say. Logging out and
/// restarting the computer are the person's to do, whatever the operation.
pub(crate) fn restart_effect(
    locale: Locale,
    operation: DesiredOperation,
    requirement: RestartRequirement,
) -> Option<&'static str> {
    let c = copy(locale);
    match requirement {
        RestartRequirement::NotDeclared => Some(c.not_declared),
        RestartRequirement::NotRequired => Some(c.restart_not_required),
        RestartRequirement::LogOut => Some(c.restart_log_out),
        RestartRequirement::Reboot => Some(c.restart_reboot),
        RestartRequirement::RestartApplication => match operation {
            DesiredOperation::Update => Some(c.restart_application),
            DesiredOperation::Install | DesiredOperation::Enable | DesiredOperation::Restore => {
                Some(c.restart_next_open)
            }
            DesiredOperation::Disable | DesiredOperation::Remove | DesiredOperation::Verify => None,
        },
    }
}
