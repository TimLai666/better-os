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

    /// Draw one read-only status indicator.
    ///
    /// A status is tinted and outlined, never filled in an action color: this
    /// window's `primary` fill belongs to its buttons, and a pill wearing it
    /// promises something to click. `better_ui::Affordance` states the rule,
    /// and `better_ui::badge` — which this draws through — registers no hover
    /// and no click handler. `gpui_component::Tag`, which these replaced,
    /// could do neither.
    pub(crate) fn pill(&self, pill: better_ui::StatusPill, cx: &mut Context<Self>) -> AnyElement {
        let accent = match pill.tone {
            better_ui::StatusTone::Neutral => {
                return pill
                    .render(better_ui::BadgeStyle {
                        foreground: cx.theme().muted_foreground,
                        background: cx.theme().secondary,
                        border: cx.theme().border,
                    })
                    .into_any_element();
            }
            better_ui::StatusTone::Info => cx.theme().info,
            better_ui::StatusTone::Success => cx.theme().success,
            better_ui::StatusTone::Warning => cx.theme().warning,
            better_ui::StatusTone::Danger => cx.theme().danger,
        };
        pill.render(better_ui::BadgeStyle {
            foreground: accent,
            background: accent.opacity(0.12),
            border: accent.opacity(0.4),
        })
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

/// An action icon on one of gpui-component's buttons (ticket 147). Those
/// buttons take a toolkit `Icon` before their label, and give the button its
/// accessible name from the label alone, so a labelled button keeps both by
/// taking this icon: the action's main file drawn as the mask it is, in the
/// colour its placement gives it (on a primary button the text on accent
/// colour), dimmed when the button is disabled. A toolkit `Icon` draws one
/// layer, so only an action drawn in one layer goes here.
pub(crate) fn action_icon(
    picture: better_ui::icon_set::Picture,
    primary: bool,
    disabled: bool,
    cx: &gpui::App,
) -> gpui_component::Icon {
    debug_assert!(
        picture.second_layer().is_none(),
        "{} has a second layer an Icon cannot draw",
        picture.name()
    );
    let colour = labelled_icon_colour(picture, primary, disabled, better_ui::fluent::palette(cx));
    gpui_component::Icon::empty()
        .path(picture.path())
        .text_color(colour)
}

/// The colour [`action_icon`] draws in: the placement's body colour, the
/// disabled text on accent on a disabled primary button, and the body at
/// the disabled opacity on any other disabled button.
pub(crate) fn labelled_icon_colour(
    picture: better_ui::icon_set::Picture,
    primary: bool,
    disabled: bool,
    palette: &better_ui::fluent::Palette,
) -> gpui::Rgba {
    use better_ui::icon_set::{DISABLED_OPACITY, Placement, action_colours};
    let placement = Placement::for_control(primary, false);
    let (body, _) = action_colours(picture, placement, palette, disabled);
    if disabled && !primary {
        better_ui::fluent::with_alpha(body, DISABLED_OPACITY)
    } else {
        body
    }
}

/// An action alone on a gpui-component button with no words: the picture,
/// both layers, at 18px, dimmed when the button is disabled. The caller
/// makes the button square with [`square_icon_button`].
pub(crate) fn action_alone(
    picture: better_ui::icon_set::Picture,
    primary: bool,
    disabled: bool,
    cx: &gpui::App,
) -> gpui::AnyElement {
    use better_ui::icon_set::Placement;
    use gpui::IntoElement as _;
    better_ui::icon_set::action(
        picture,
        Placement::for_control(primary, true),
        better_ui::fluent::palette(cx),
        disabled,
    )
    .into_any_element()
}

/// A gpui-component button holding only an icon child, sized as that kit's
/// own icon button (32px square): it sizes a button as an icon button only
/// when it has no children, and skips its own sizing once padding is set.
pub(crate) fn square_icon_button(
    button: gpui_component::button::Button,
) -> gpui_component::button::Button {
    use gpui::Styled as _;
    button.size_8().p_0()
}

#[cfg(test)]
mod action_button_tests {
    use better_ui::fluent::{LIGHT, with_alpha};
    use better_ui::icon_set::{DISABLED_OPACITY, Picture};

    use super::labelled_icon_colour;

    /// A disabled button's icon is dimmed with it: the quiet grey at the
    /// disabled opacity, and on a primary button the disabled text on accent.
    #[test]
    fn a_disabled_button_dims_its_icon() {
        let live = labelled_icon_colour(Picture::Add, false, false, &LIGHT);
        assert_eq!(live, LIGHT.icon_quiet);
        assert_eq!(
            labelled_icon_colour(Picture::Add, false, true, &LIGHT),
            with_alpha(LIGHT.icon_quiet, DISABLED_OPACITY)
        );
        assert_eq!(
            labelled_icon_colour(Picture::Add, true, true, &LIGHT),
            LIGHT.text_on_accent_disabled
        );
        assert_eq!(
            labelled_icon_colour(Picture::Add, true, false, &LIGHT),
            LIGHT.text_on_accent
        );
    }

    /// Every button with an action icon keeps its words in `label`, where
    /// gpui-component takes the accessible name from, and every button with
    /// an action alone is made square as that kit's icon buttons are.
    #[test]
    fn action_buttons_keep_their_name_and_their_shape() {
        let mut faults = Vec::new();
        for (name, text) in [
            ("shell.rs", include_str!("shell.rs")),
            ("pages_status.rs", include_str!("pages_status.rs")),
            ("pages_rules.rs", include_str!("pages_rules.rs")),
        ] {
            for (at, _) in text.match_indices("components::action_icon(") {
                let rest = &text[at..];
                let chain = &rest[..rest.find(".on_click(").unwrap_or(rest.len())];
                if !chain.contains(".label(") {
                    faults.push(format!("{name}: an action icon on a button without .label"));
                }
            }
            for (at, _) in text.match_indices("components::action_alone(") {
                let before = &text[at.saturating_sub(240)..at];
                if !before.contains("square_icon_button(") {
                    faults.push(format!(
                        "{name}: an action alone on a button not made square"
                    ));
                }
            }
        }
        assert!(faults.is_empty(), "{faults:#?}");
    }
}
