//! The combo box (drop-down list) and the slider.
//!
//! `ComboBox_themeresources.xaml`: a 32px field drawn like a standard button,
//! text 12px from the left edge, a secondary chevron at the end; a drop-down
//! on the flyout fill with 4px vertical margin, items 11,5,11,7 padded with
//! 3px corners, and the selected item marked by a subtle fill and a 3 by 16
//! accent pill (`ComboBoxItemPill*`), so the choice is always visible.
//!
//! `Slider_themeresources.xaml`: a 32px tall control with a 4px track
//! rounded 2px, `ControlStrongFillColorDefault` behind the accent value fill,
//! an 18px thumb on `ControlSolidFillColorDefault` whose accent center is
//! 12px at rest, 14px under the pointer, and 10px pressed.
//!
//! Keys: a closed combo box changes its selection with Up and Down, and
//! opens on Enter, Space, Alt+Down, or F4; open, the arrows move the
//! highlight, Enter or Space choose it, and Escape closes without a change.
//! A slider steps with the arrows, jumps by the large change with Page Up
//! and Page Down, and goes to its ends with Home and End (WinUI's
//! `RangeBase`).

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, ElementId, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, anchored, canvas,
    deferred, div, prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{Icon, IconName};

use super::ValueHandler;
use super::button::{self, ButtonKind};
use super::menu::panel_frame;
use super::{
    BODY, Interaction, Key, Palette, ShownState, keyed_focus, paint, palette, radius, resolve,
    shows_focus, size, step_index, text, with_alpha, with_focus_ring,
};

/// What a key press does to a combo box.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComboOutcome {
    Ignored,
    /// Closed: the selection changed directly.
    Select(usize),
    /// The drop-down opened with this item highlighted.
    Open(Option<usize>),
    /// Open: the highlight moved.
    Highlight(usize),
    /// Open: this item was chosen and the drop-down closed.
    Commit(usize),
    /// Open: closed without a change.
    Close,
}

/// The keyboard rules of a combo box. `skip` marks disabled items.
pub fn combo_key(
    open: bool,
    selected: Option<usize>,
    highlight: Option<usize>,
    len: usize,
    key: Key,
    skip: impl Fn(usize) -> bool,
) -> ComboOutcome {
    match (open, key) {
        (false, Key::Enter | Key::Space | Key::OpenDropDown) => ComboOutcome::Open(selected),
        (false, Key::Up | Key::Down | Key::Home | Key::End) => {
            match step_index(selected, len, key, false, skip) {
                Some(next) => ComboOutcome::Select(next),
                None => ComboOutcome::Ignored,
            }
        }
        (true, Key::Up | Key::Down | Key::Home | Key::End) => {
            match step_index(highlight, len, key, false, skip) {
                Some(next) => ComboOutcome::Highlight(next),
                None => ComboOutcome::Ignored,
            }
        }
        (true, Key::Enter | Key::Space | Key::OpenDropDown) => match highlight {
            Some(index) => ComboOutcome::Commit(index),
            None => ComboOutcome::Close,
        },
        (true, Key::Escape) => ComboOutcome::Close,
        _ => ComboOutcome::Ignored,
    }
}

/// Whether a combo box is open, and its highlighted item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboOpen {
    pub open: bool,
    pub highlight: Option<usize>,
}

/// One choice in a combo box.
#[derive(Clone)]
pub struct ComboItem {
    pub label: SharedString,
    pub disabled: bool,
}

impl ComboItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

type SelectHandler = ValueHandler<usize>;

/// `ComboBox`: a field showing the chosen item, opening a drop-down list.
#[derive(IntoElement)]
pub struct ComboBox {
    id: ElementId,
    items: Vec<ComboItem>,
    selected: Option<usize>,
    placeholder: Option<SharedString>,
    header: Option<SharedString>,
    disabled: bool,
    shown: ShownState,
    /// Draw the drop-down open in the page, for the gallery.
    show_open: Option<Option<usize>>,
    min_width: Pixels,
    on_select: Option<SelectHandler>,
}

impl ComboBox {
    pub fn new(id: impl Into<ElementId>, items: Vec<ComboItem>, selected: Option<usize>) -> Self {
        Self {
            id: id.into(),
            items,
            selected,
            placeholder: None,
            header: None,
            disabled: false,
            shown: ShownState::Live,
            show_open: None,
            min_width: px(120.0),
            on_select: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn header(mut self, header: impl Into<SharedString>) -> Self {
        self.header = Some(header.into());
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

    /// The narrowest the field may be. It grows to fit its text.
    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self
    }

    /// Draws the drop-down open, in the page, with this item highlighted.
    pub fn show_open(mut self, highlight: Option<usize>) -> Self {
        self.show_open = Some(highlight);
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

fn close_combo(state: &Entity<ComboOpen>, cx: &mut App) {
    state.update(cx, |s, cx| {
        *s = ComboOpen::default();
        cx.notify();
    });
}

fn combo_list(
    items: &[ComboItem],
    selected: Option<usize>,
    highlight: Option<usize>,
    min_width: Pixels,
    interactive: Option<(Entity<ComboOpen>, Option<SelectHandler>, gpui::FocusHandle)>,
    p: &Palette,
) -> gpui::Stateful<gpui::Div> {
    let mut list = panel_frame(div().id("combo-list"), p)
        .occlude()
        .min_w(min_width)
        .py(px(4.0));
    for (index, item) in items.iter().enumerate() {
        let is_selected = selected == Some(index);
        let is_highlighted = highlight == Some(index);
        let background = if is_highlighted || is_selected {
            p.subtle_fill
        } else {
            with_alpha(p.subtle_fill, 0.0)
        };
        let foreground = if item.disabled {
            p.text_disabled
        } else {
            p.text_primary
        };
        let hover = p.subtle_fill;
        let pressed = p.subtle_fill_tertiary;
        let mut row = text(
            div()
                .id(ElementId::named_usize("combo-item", index))
                .relative()
                .mx(px(5.0))
                .my(px(2.0))
                .pt(px(5.0))
                .pb(px(7.0))
                .px(px(11.0))
                .rounded(px(3.0))
                .bg(background)
                .text_color(foreground)
                .whitespace_nowrap(),
            BODY,
        )
        .child(item.label.clone());
        if is_selected {
            row = row.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(8.0))
                    .w(size::SELECTION_BAR_WIDTH)
                    .h(size::SELECTION_BAR_HEIGHT)
                    .rounded(px(1.5))
                    .bg(p.accent),
            );
        }
        if !item.disabled
            && let Some((state, handler, focus)) = interactive.clone()
        {
            row = row
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .on_click(move |_, window, cx| {
                    close_combo(&state, cx);
                    focus.focus(window, cx);
                    if let Some(handler) = handler.as_ref() {
                        handler(index, window, cx);
                    }
                });
        }
        list = list.child(row);
    }
    list
}

impl RenderOnce for ComboBox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let state = window.use_keyed_state(super::child_id(&self.id, "open"), cx, |_, _| {
            ComboOpen::default()
        });
        let open = *state.read(cx);
        let colors = button::colors(ButtonKind::Standard, false, p);
        let items = Rc::new(self.items);

        let (shown_text, text_color) = match self.selected.and_then(|i| items.get(i)) {
            Some(item) => (item.label.clone(), None),
            None => (
                self.placeholder.clone().unwrap_or_default(),
                Some(if disabled {
                    p.text_disabled
                } else {
                    p.text_secondary
                }),
            ),
        };
        let chevron_color = if disabled {
            p.text_disabled
        } else {
            p.text_secondary
        };
        let mut field = div()
            .id(self.id.clone())
            .h(size::CONTROL_HEIGHT)
            .min_w(self.min_width)
            .flex()
            .flex_none()
            .items_center()
            .pl(px(12.0))
            .pr(px(11.0))
            .gap(px(8.0));
        field = text(field, BODY);
        if !disabled {
            field = field.track_focus(&focus.clone().tab_index(0).tab_stop(self.shown.is_live()));
        }
        let mut field = paint(field, colors, forced, radius::SM, p.is_dark())
            .child(
                div()
                    .flex_1()
                    .whitespace_nowrap()
                    .when_some(text_color, |d, c| d.text_color(c))
                    .child(shown_text),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(px(12.0))
                    .text_color(chevron_color),
            );
        if !disabled {
            let click_state = state.clone();
            let selected = self.selected;
            field = field.on_click(move |event, window, cx| {
                if event.is_keyboard() {
                    return;
                }
                click_state.update(cx, |s, cx| {
                    *s = if s.open {
                        ComboOpen::default()
                    } else {
                        ComboOpen {
                            open: true,
                            highlight: selected,
                        }
                    };
                    cx.notify();
                });
                let _ = window;
            });
            let key_state = state.clone();
            let key_items = items.clone();
            let handler = self.on_select.clone();
            let selected = self.selected;
            field = field.on_key_down(move |event, window, cx| {
                let Some(key) = Key::from_keystroke(&event.keystroke) else {
                    return;
                };
                let now = *key_state.read(cx);
                let outcome = combo_key(
                    now.open,
                    selected,
                    now.highlight,
                    key_items.len(),
                    key,
                    |i| key_items[i].disabled,
                );
                match outcome {
                    ComboOutcome::Ignored => return,
                    ComboOutcome::Select(index) => {
                        if let Some(handler) = handler.as_ref() {
                            handler(index, window, cx);
                        }
                    }
                    ComboOutcome::Open(highlight) => key_state.update(cx, |s, cx| {
                        *s = ComboOpen {
                            open: true,
                            highlight,
                        };
                        cx.notify();
                    }),
                    ComboOutcome::Highlight(index) => key_state.update(cx, |s, cx| {
                        s.highlight = Some(index);
                        cx.notify();
                    }),
                    ComboOutcome::Commit(index) => {
                        close_combo(&key_state, cx);
                        if let Some(handler) = handler.as_ref() {
                            handler(index, window, cx);
                        }
                    }
                    ComboOutcome::Close => close_combo(&key_state, cx),
                }
                cx.stop_propagation();
            });
        }
        let field = with_focus_ring(field, ring, radius::SM, p);

        let inline_dropdown = self.show_open.map(|highlight| {
            div().mt(px(4.0)).child(combo_list(
                &items,
                self.selected,
                highlight,
                self.min_width,
                None,
                p,
            ))
        });
        let overlay = if self.show_open.is_some() {
            None
        } else if open.open && !disabled {
            let out_state = state.clone();
            let list = combo_list(
                &items,
                self.selected,
                open.highlight,
                self.min_width,
                Some((state.clone(), self.on_select.clone(), focus.clone())),
                p,
            )
            .on_mouse_down_out(move |_, _, cx| close_combo(&out_state, cx));
            Some(
                div()
                    .absolute()
                    .top(relative(1.0))
                    .left_0()
                    .child(
                        deferred(
                            anchored()
                                .snap_to_window_with_margin(px(8.0))
                                .child(div().mt(px(4.0)).child(list)),
                        )
                        .with_priority(1),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let header_color = if disabled {
            p.text_disabled
        } else {
            p.text_primary
        };
        div()
            .flex()
            .flex_col()
            .children(self.header.map(|header| {
                text(div().mb(px(8.0)), BODY)
                    .text_color(header_color)
                    .child(header)
            }))
            .child(div().relative().flex().child(field).children(overlay))
            .children(inline_dropdown)
    }
}

/// The range and steps of a slider, and its keyboard and pointer rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderModel {
    pub minimum: f64,
    pub maximum: f64,
    /// `StepFrequency` and `SmallChange`: the arrows, and where a pointer
    /// lands.
    pub step: f64,
    /// `LargeChange`: Page Up and Page Down.
    pub large_change: f64,
}

impl SliderModel {
    pub fn new(minimum: f64, maximum: f64) -> Self {
        Self {
            minimum,
            maximum,
            step: 1.0,
            large_change: 10.0,
        }
    }

    /// Snaps to the step grid that starts at the minimum, and keeps the
    /// value in range.
    pub fn snap(&self, value: f64) -> f64 {
        let clamped = value.clamp(self.minimum, self.maximum);
        if self.step <= 0.0 {
            return clamped;
        }
        let steps = ((clamped - self.minimum) / self.step).round();
        (self.minimum + steps * self.step).clamp(self.minimum, self.maximum)
    }

    /// Where the value sits along the track, from 0 to 1.
    pub fn fraction(&self, value: f64) -> f32 {
        let span = self.maximum - self.minimum;
        if span <= 0.0 {
            return 0.0;
        }
        (((value - self.minimum) / span).clamp(0.0, 1.0)) as f32
    }

    /// The value at a point along the track, from 0 to 1.
    pub fn value_at(&self, fraction: f32) -> f64 {
        let fraction = f64::from(fraction.clamp(0.0, 1.0));
        self.snap(self.minimum + fraction * (self.maximum - self.minimum))
    }

    /// The value a key moves to, or `None` when it does not move the slider.
    pub fn key(&self, value: f64, key: Key) -> Option<f64> {
        let next = match key {
            Key::Right | Key::Up => value + self.step,
            Key::Left | Key::Down => value - self.step,
            Key::PageUp => value + self.large_change,
            Key::PageDown => value - self.large_change,
            Key::Home => self.minimum,
            Key::End => self.maximum,
            _ => return None,
        };
        let next = self.snap(next);
        (next != value).then_some(next)
    }
}

/// The thumb's center size and the fills, for one state.
fn slider_parts(state: Interaction, p: &Palette) -> (Pixels, gpui::Rgba, gpui::Rgba) {
    match state {
        Interaction::Rest => (px(12.0), p.accent, p.control_strong_fill),
        Interaction::PointerOver => (px(14.0), p.accent_secondary, p.control_strong_fill),
        Interaction::Pressed => (px(10.0), p.accent_tertiary, p.control_strong_fill),
        Interaction::Disabled => (px(12.0), p.accent_disabled, p.control_strong_fill_disabled),
    }
}

/// Where a slider's track was last laid out, so a pointer position can be
/// turned into a value.
#[derive(Default)]
pub struct SliderState {
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    dragging: bool,
}

/// `Slider`, horizontal.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f64,
    model: SliderModel,
    header: Option<SharedString>,
    disabled: bool,
    shown: ShownState,
    width: Pixels,
    on_change: Option<ValueHandler<f64>>,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>, value: f64, model: SliderModel) -> Self {
        Self {
            id: id.into(),
            value,
            model,
            header: None,
            disabled: false,
            shown: ShownState::Live,
            width: px(200.0),
            on_change: None,
        }
    }

    pub fn header(mut self, header: impl Into<SharedString>) -> Self {
        self.header = Some(header.into());
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

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

const THUMB: Pixels = px(18.0);

fn slider_body(fraction: f32, state: Interaction, width: Pixels, p: &Palette) -> gpui::Div {
    let (center, fill, track) = slider_parts(state, p);
    // The track runs between the thumb's centers at either end.
    let travel = width - THUMB;
    let x = travel * fraction;
    div()
        .relative()
        .w(width)
        .h(size::CONTROL_HEIGHT)
        .child(
            div()
                .absolute()
                .left(THUMB / 2.0)
                .right(THUMB / 2.0)
                .top(px(14.0))
                .h(px(4.0))
                .rounded(px(2.0))
                .bg(track),
        )
        .child(
            div()
                .absolute()
                .left(THUMB / 2.0)
                .w(x)
                .top(px(14.0))
                .h(px(4.0))
                .rounded(px(2.0))
                .bg(fill),
        )
        .child(
            div()
                .absolute()
                .left(x)
                .top(px(7.0))
                .size(THUMB)
                .rounded_full()
                .bg(p.control_solid_fill)
                .border_1()
                .border_color(p.stroke)
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(center).rounded_full().bg(fill)),
        )
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let state = window.use_keyed_state(super::child_id(&self.id, "slider"), cx, |_, _| {
            SliderState::default()
        });
        let dragging = state.read(cx).dragging;
        let bounds = state.read(cx).bounds.clone();
        let fraction = self.model.fraction(self.value);
        let width = self.width;
        let group: SharedString = "fluent-slider".into();

        let shown_state = match forced {
            Some(s) => s,
            None if dragging => Interaction::Pressed,
            None => Interaction::Rest,
        };
        let mut body = div()
            .id(self.id.clone())
            .group(group.clone())
            .relative()
            .w(width)
            .h(size::CONTROL_HEIGHT)
            .child(slider_body(fraction, shown_state, width, p))
            .child({
                let bounds = bounds.clone();
                canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
            });
        if forced.is_none() && !dragging {
            body = body.child(
                div()
                    .id("hover")
                    .absolute()
                    .top_0()
                    .left_0()
                    .opacity(0.0)
                    .group_hover(group, |s| s.opacity(1.0))
                    .child(slider_body(fraction, Interaction::PointerOver, width, p)),
            );
        }
        if !disabled {
            let model = self.model;
            let value = self.value;
            let to_value = {
                let bounds = bounds.clone();
                move |x: Pixels| -> Option<f64> {
                    let b = bounds.get()?;
                    let travel = b.size.width - THUMB;
                    if travel <= px(0.0) {
                        return None;
                    }
                    let fraction = (x - b.origin.x - THUMB / 2.0) / travel;
                    Some(model.value_at(fraction))
                }
            };
            body = body.track_focus(&focus.clone().tab_index(0).tab_stop(self.shown.is_live()));
            if let Some(handler) = self.on_change.clone() {
                let down_state = state.clone();
                let down_handler = handler.clone();
                let down_value = to_value.clone();
                let move_state = state.clone();
                let move_handler = handler.clone();
                let move_value = to_value.clone();
                let up_state = state.clone();
                let key_handler = handler.clone();
                let focus_down = focus.clone();
                body = body
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        focus_down.focus(window, cx);
                        down_state.update(cx, |s, cx| {
                            s.dragging = true;
                            cx.notify();
                        });
                        if let Some(v) = down_value(event.position.x) {
                            down_handler(v, window, cx);
                        }
                    })
                    .on_mouse_move(move |event, window, cx| {
                        if move_state.read(cx).dragging
                            && event.pressed_button == Some(MouseButton::Left)
                            && let Some(v) = move_value(event.position.x)
                        {
                            move_handler(v, window, cx);
                        }
                    })
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        up_state.update(cx, |s, cx| {
                            s.dragging = false;
                            cx.notify();
                        });
                    })
                    .on_key_down(move |event, window, cx| {
                        if let Some(next) = Key::from_keystroke(&event.keystroke)
                            .and_then(|key| model.key(value, key))
                        {
                            cx.stop_propagation();
                            key_handler(next, window, cx);
                        }
                    });
            }
            let out_state = state.clone();
            body = body.on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                out_state.update(cx, |s, cx| {
                    s.dragging = false;
                    cx.notify();
                });
            });
        }
        let body = with_focus_ring(body, ring, radius::SM, p);
        let header_color = if disabled {
            p.text_disabled
        } else {
            p.text_primary
        };
        div()
            .flex()
            .flex_col()
            .children(self.header.map(|header| {
                text(div().mb(px(4.0)), BODY)
                    .text_color(header_color)
                    .child(header)
            }))
            .child(body)
    }
}
