//! `ColorSwatch`: one colour of a colour picker's palette, as WinUI's
//! `ColorPicker` draws its swatches. A tab stop that Enter and Space
//! choose, named by a tooltip (the kit's way of naming a control that
//! shows no text, as `Button::icon_button` is), with the chosen one ringed
//! in the accent colour.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use super::button::invoked_by;
use super::{Handler, Key, keyed_focus, palette, radius, shows_focus, with_focus_ring};

#[derive(IntoElement)]
pub struct ColorSwatch {
    id: ElementId,
    /// `0xRRGGBB`.
    colour: u32,
    name: SharedString,
    selected: bool,
    disabled: bool,
    on_click: Option<Handler>,
}

impl ColorSwatch {
    /// A swatch of `colour` (`0xRRGGBB`) called `name`, which its tooltip
    /// shows.
    pub fn new(id: impl Into<ElementId>, colour: u32, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            colour,
            name: name.into(),
            selected: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ColorSwatch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !self.disabled && shows_focus(&focus, super::ShownState::Live, window);
        let mut swatch = div()
            .id(self.id.clone())
            .size(px(28.0))
            .flex_none()
            .rounded(radius::SM)
            .border_2()
            .border_color(if self.selected { p.accent } else { p.card })
            .p(px(2.0))
            .tooltip(super::dialog::tooltip_builder(self.name.clone()))
            .child(
                div()
                    .size_full()
                    .rounded(px(2.0))
                    .border_1()
                    .border_color(p.card_stroke)
                    .bg(rgb(self.colour)),
            );
        if !self.disabled {
            swatch = swatch
                .cursor_pointer()
                .track_focus(&focus.tab_index(0).tab_stop(true));
            if let Some(handler) = self.on_click {
                let on_key = handler.clone();
                swatch = swatch
                    .on_click(move |event, window, cx| {
                        // Keys are handled on key down, by the WinUI rule.
                        if !event.is_keyboard() {
                            handler(window, cx);
                        }
                    })
                    .on_key_down(move |event, window, cx| {
                        if Key::from_keystroke(&event.keystroke).is_some_and(invoked_by) {
                            cx.stop_propagation();
                            on_key(window, cx);
                        }
                    });
            }
        }
        with_focus_ring(swatch, ring, radius::SM, p)
    }
}
