//! The handful of shapes every section reuses.
//!
//! Each one is a thin wrapper over a `better-ui` primitive with this window's
//! theme colors filled in, so the manager, the monitor, and this window cannot
//! drift apart visually by each inventing its own card.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    *,
};

use crate::app::AwakeApp;

impl AwakeApp {
    pub(crate) fn surface(&self, child: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        better_ui::surface(
            child,
            cx.theme().border,
            cx.theme().background,
            cx.theme().radius,
        )
        .into_any_element()
    }

    pub(crate) fn section_heading(
        &self,
        title: &'static str,
        subtitle: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(div().text_2xl().font_bold().child(title))
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle),
            )
            .into_any_element()
    }

    pub(crate) fn card_title(&self, title: impl Into<SharedString>) -> AnyElement {
        div()
            .text_lg()
            .font_semibold()
            .child(title.into())
            .into_any_element()
    }

    /// One labelled value. Wraps rather than truncating, because a long
    /// localized label that is cut in half explains nothing.
    pub(crate) fn key_value(
        &self,
        label: impl Into<SharedString>,
        value: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .py_1p5()
            .items_start()
            .justify_between()
            .flex_wrap()
            .child(
                div()
                    .min_w(px(160.0))
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.into()),
            )
            .child(
                div()
                    .min_w(px(120.0))
                    .flex_1()
                    .text_sm()
                    .font_semibold()
                    .child(value.into()),
            )
            .into_any_element()
    }

    /// A line of explanation, used wherever the window has to say why something
    /// is not offered rather than offering it and doing nothing.
    pub(crate) fn explanation(
        &self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        better_ui::notice(
            message.into(),
            cx.theme().muted_foreground,
            cx.theme().muted,
            cx.theme().radius,
        )
        .into_any_element()
    }

    pub(crate) fn warning(
        &self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        better_ui::notice(
            message.into(),
            cx.theme().warning_foreground,
            cx.theme().warning,
            cx.theme().radius,
        )
        .into_any_element()
    }

    pub(crate) fn danger(
        &self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        better_ui::notice(
            message.into(),
            cx.theme().danger_foreground,
            cx.theme().danger,
            cx.theme().radius,
        )
        .into_any_element()
    }

    pub(crate) fn state_message(
        &self,
        title: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        better_ui::state_message(
            title.into(),
            detail.into(),
            cx.theme().foreground,
            cx.theme().muted_foreground,
        )
        .into_any_element()
    }

    /// The banner every section shows when the service cannot be read. It says
    /// the same thing everywhere, because the answer is the same everywhere.
    pub(crate) fn connection_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let detail = self.connection_error.as_ref()?;
        let c = crate::i18n::copy(self.locale);
        Some(
            v_flex()
                .w_full()
                .gap_2()
                .child(self.danger(c.service_unreachable, cx))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.service_unreachable_detail),
                )
                // The reason is a stable key. Showing it beats showing nothing
                // to someone who has to diagnose this.
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail.clone()),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn action_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let detail = self.action_error.as_ref()?;
        Some(self.warning(detail.clone(), cx))
    }

    /// A number with a decrement and an increment beside it. Used for the
    /// battery threshold and the rule priority, neither of which needs a
    /// free-text field.
    pub(crate) fn stepper(
        &self,
        id: &'static str,
        value: String,
        decrease: impl Fn(&mut AwakeApp, &mut Window, &mut Context<AwakeApp>) + 'static,
        increase: impl Fn(&mut AwakeApp, &mut Window, &mut Context<AwakeApp>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new(SharedString::from(format!("{id}-decrease")))
                    .icon(IconName::Minus)
                    .on_click(cx.listener(move |this, _, window, cx| decrease(this, window, cx))),
            )
            .child(div().min_w(px(64.0)).text_center().child(value))
            .child(
                Button::new(SharedString::from(format!("{id}-increase")))
                    .icon(IconName::Plus)
                    .on_click(cx.listener(move |this, _, window, cx| increase(this, window, cx))),
            )
            .into_any_element()
    }

    /// A two-state choice drawn as two buttons rather than a switch, so the
    /// unselected option is always readable and always reachable by keyboard.
    pub(crate) fn choice(
        &self,
        id: &'static str,
        label: &'static str,
        selected: bool,
        on_click: impl Fn(&mut AwakeApp, &mut Window, &mut Context<AwakeApp>) + 'static,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .selected(selected)
            .when(selected, |button| button.primary())
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
    }

    pub(crate) fn badge_row(
        &self,
        icon: IconName,
        text: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_2()
            .items_center()
            .min_w_0()
            .child(Icon::new(icon).small().text_color(cx.theme().primary))
            .child(div().min_w_0().text_sm().child(text.into()))
            .into_any_element()
    }
}
