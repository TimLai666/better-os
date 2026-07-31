use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    tag::Tag,
    *,
};

use crate::{
    app::ManagerApp,
    i18n::copy,
    model::{
        ActivityKind, ComponentInfo, ComponentKind, ComponentState, Modal, Page, RestartRequirement,
    },
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
        value: impl IntoElement,
        detail: &'static str,
        icon: IconName,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .min_w(px(220.0))
            .flex_1()
            .child(
                self.surface(
                    v_flex()
                        .gap_3()
                        .child(
                            h_flex()
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

    pub(crate) fn section_header(
        &self,
        title: &'static str,
        subtitle: Option<&'static str>,
        action: Option<Button>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_end()
            .justify_between()
            .flex_wrap()
            .child(
                v_flex()
                    .min_w(px(240.0))
                    .flex_1()
                    .gap_1()
                    .child(div().text_lg().font_semibold().child(title))
                    .when_some(subtitle, |view, subtitle| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(subtitle),
                        )
                    }),
            )
            .when_some(action, |row, action| row.child(action))
            .into_any_element()
    }

    pub(crate) fn status_tag(&self, state: ComponentState) -> Tag {
        let c = copy(self.locale);
        match state {
            ComponentState::Healthy => Tag::success().small().rounded_full().child(c.healthy),
            ComponentState::UpdateAvailable => Tag::warning()
                .small()
                .rounded_full()
                .child(c.update_available),
            ComponentState::Available => Tag::info().small().rounded_full().child(c.available),
            ComponentState::Planned => Tag::secondary().small().rounded_full().child(c.planned),
            ComponentState::Disabled => Tag::secondary().small().rounded_full().child(c.disabled),
            ComponentState::Incompatible => Tag::danger()
                .outline()
                .small()
                .rounded_full()
                .child(c.incompatible),
            ComponentState::Degraded => Tag::warning().small().rounded_full().child(c.degraded),
            ComponentState::Failed => Tag::danger().small().rounded_full().child(c.failed),
            ComponentState::RestoreAvailable => Tag::info()
                .small()
                .rounded_full()
                .child(c.restore_available_status),
        }
    }

    pub(crate) fn kind_tag(&self, kind: ComponentKind) -> Tag {
        let c = copy(self.locale);
        match kind {
            ComponentKind::Core => Tag::primary()
                .outline()
                .small()
                .rounded_full()
                .child(c.core),
            ComponentKind::Replacement => Tag::info()
                .outline()
                .small()
                .rounded_full()
                .child(c.replacement),
            ComponentKind::Enhancement => Tag::success()
                .outline()
                .small()
                .rounded_full()
                .child(c.enhancement),
            ComponentKind::Diagnostic => Tag::secondary()
                .outline()
                .small()
                .rounded_full()
                .child(c.diagnostic),
        }
    }

    pub(crate) fn restart_tag(&self, requirement: RestartRequirement) -> Tag {
        let c = copy(self.locale);
        match requirement {
            RestartRequirement::None => Tag::secondary().small().rounded_full().child(c.no_restart),
            RestartRequirement::Logout => Tag::warning()
                .small()
                .rounded_full()
                .child(c.logout_required),
            RestartRequirement::Restart => Tag::danger()
                .small()
                .rounded_full()
                .child(c.restart_required),
        }
    }

    pub(crate) fn component_icon(&self, id: &str) -> IconName {
        match id {
            "manager" => IconName::Settings,
            "monitor" => IconName::Inspector,
            "launcher" => IconName::Search,
            "files" => IconName::Folder,
            _ => IconName::Inbox,
        }
    }

    pub(crate) fn component_action_button(
        &self,
        component: ComponentInfo,
        cx: &mut Context<Self>,
    ) -> Button {
        let c = copy(self.locale);
        let id = component.id;
        match component.state {
            ComponentState::Healthy | ComponentState::Disabled | ComponentState::Planned => {
                Button::new(format!("open-{id}"))
                    .label(c.details)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_component(id, cx);
                    }))
            }
            ComponentState::UpdateAvailable | ComponentState::Available => {
                let label = if component.state == ComponentState::Available {
                    c.install
                } else {
                    c.update
                };
                Button::new(format!("change-{id}"))
                    .primary()
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.prepare_component_change(id, cx);
                    }))
            }
            ComponentState::Incompatible => Button::new(format!("incompatible-{id}"))
                .label(c.details)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_component(id, cx);
                })),
            ComponentState::Degraded
            | ComponentState::Failed
            | ComponentState::RestoreAvailable => Button::new(format!("recover-{id}"))
                .danger()
                .label(c.view_recovery)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.navigate(Page::Restore, cx);
                })),
        }
    }

    pub(crate) fn component_card(
        &self,
        component: ComponentInfo,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let identity = h_flex()
            .min_w_0()
            .gap_3()
            .items_center()
            .child(
                div()
                    .size_10()
                    .flex_shrink_0()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().secondary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(self.component_icon(component.id)).small()),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .font_semibold()
                            .child(self.component_name(component.id)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.purpose(component.id)),
                    ),
            );

        let metadata = h_flex()
            .min_w_0()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(component.version_label()),
            )
            .child(self.kind_tag(component.kind))
            .child(self.status_tag(component.state))
            .child(self.restart_tag(component.restart));

        let actions = h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(self.component_action_button(component, cx));

        let content = if compact {
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(identity)
                .child(metadata)
                .child(actions)
                .into_any_element()
        } else {
            h_flex()
                .w_full()
                .min_w_0()
                .gap_4()
                .items_center()
                .justify_between()
                .flex_wrap()
                .child(div().min_w(px(280.0)).flex_1().child(identity))
                .child(metadata)
                .child(actions)
                .into_any_element()
        };

        div()
            .w_full()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(content)
            .into_any_element()
    }

    pub(crate) fn bullet_row(
        &self,
        icon: IconName,
        title: impl IntoElement,
        detail: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_start()
            .child(
                div()
                    .mt_1()
                    .size_7()
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(cx.theme().secondary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(icon).xsmall()),
            )
            .child(
                v_flex()
                    .min_w_0()
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
            .into_any_element()
    }

    pub(crate) fn key_value_row(
        &self,
        key: impl IntoElement,
        value: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_start()
            .justify_between()
            .flex_wrap()
            .child(
                div()
                    .min_w(px(180.0))
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(key),
            )
            .child(div().font_semibold().child(value))
            .into_any_element()
    }

    pub(crate) fn activity_status_tag(&self, kind: ActivityKind) -> Tag {
        let c = copy(self.locale);
        match kind {
            ActivityKind::Success => Tag::success().small().rounded_full().child(c.successful),
            ActivityKind::Warning => Tag::warning().small().rounded_full().child(c.warnings),
            ActivityKind::Failure => Tag::danger().small().rounded_full().child(c.failures),
            ActivityKind::Information => Tag::info().small().rounded_full().child(c.activity),
        }
    }

    pub(crate) fn activity_row(
        &self,
        kind: ActivityKind,
        title: impl IntoElement,
        time: impl IntoElement,
        detail: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                h_flex()
                    .min_w(px(260.0))
                    .flex_1()
                    .min_w_0()
                    .gap_3()
                    .items_start()
                    .child(self.activity_status_tag(kind))
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_1()
                            .child(div().font_semibold().child(title))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(detail),
                            ),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(time),
            )
            .into_any_element()
    }

    pub(crate) fn empty_state(
        &self,
        icon: IconName,
        title: &'static str,
        description: &'static str,
        actions: impl IntoElement,
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
                        .size_12()
                        .rounded_full()
                        .bg(cx.theme().secondary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon)),
                )
                .child(div().text_xl().font_semibold().child(title))
                .child(
                    div()
                        .max_w(px(620.0))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
                .child(actions),
            cx,
        )
    }

    pub(crate) fn open_disable_dialog(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.modal = Modal::ConfirmDisable(id);
        cx.notify();
    }
}
