//! `HyperlinkButton`: a link drawn as a button, for a place to go rather than
//! a thing to do.
//!
//! WinUI's `HyperlinkButton` is accent text (`AccentTextFillColorPrimary`) on
//! a transparent fill, with `SubtleFillColorSecondary` under the pointer and
//! `SubtleFillColorTertiary` pressed, 4px corners and a standard button's
//! padding. A link that leaves the window carries the external-link glyph
//! after its text, so nobody is surprised when a browser opens. Enter and
//! Space follow it, by the button rule.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName};

use super::{
    BODY, Handler, Interaction, Key, ShownState, keyed_focus, palette, radius, resolve,
    shows_focus, size, text, with_alpha, with_focus_ring,
};

/// The text color of a link in one state.
pub fn link_color(state: Interaction, p: &super::Palette) -> gpui::Rgba {
    match state {
        Interaction::Disabled => p.text_disabled,
        Interaction::Pressed => with_alpha(p.accent_text, 0.8),
        _ => p.accent_text,
    }
}

#[derive(IntoElement)]
pub struct HyperlinkButton {
    id: ElementId,
    label: SharedString,
    external: bool,
    disabled: bool,
    shown: ShownState,
    on_click: Option<Handler>,
}

impl HyperlinkButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            external: false,
            disabled: false,
            shown: ShownState::Live,
            on_click: None,
        }
    }

    /// The link leaves the window (a web page, a file in another program).
    pub fn external(mut self, external: bool) -> Self {
        self.external = external;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for HyperlinkButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let state = forced.unwrap_or(Interaction::Rest);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let fill = match state {
            Interaction::PointerOver => p.subtle_fill,
            Interaction::Pressed => p.subtle_fill_tertiary,
            _ => with_alpha(p.subtle_fill, 0.0),
        };
        let hover = p.subtle_fill;
        let pressed = p.subtle_fill_tertiary;
        let pressed_text = link_color(Interaction::Pressed, p);

        let mut link = text(
            div()
                .id(self.id.clone())
                .relative()
                .h(size::CONTROL_HEIGHT)
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.0))
                .px(px(11.0))
                .rounded(radius::SM)
                .bg(fill)
                .text_color(link_color(state, p)),
            BODY,
        )
        .child(div().whitespace_nowrap().child(self.label))
        .when(self.external, |link| {
            link.child(Icon::new(IconName::ExternalLink).size(px(12.0)))
        });
        if forced.is_none() {
            link = link
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed).text_color(pressed_text));
        }
        if !disabled {
            link = link.track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()));
            if let Some(handler) = self.on_click {
                let on_key = handler.clone();
                link = link
                    .on_click(move |event, window, cx| {
                        if !event.is_keyboard() {
                            handler(window, cx);
                        }
                    })
                    .on_key_down(move |event, window, cx| {
                        if Key::from_keystroke(&event.keystroke).is_some_and(Key::invokes) {
                            cx.stop_propagation();
                            on_key(window, cx);
                        }
                    });
            }
        }
        with_focus_ring(link, ring, radius::SM, p)
    }
}
