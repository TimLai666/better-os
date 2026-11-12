//! The text box, password box, search box, and number box.
//!
//! The text editing is gpui-component's `InputState`, drawn without its own
//! frame; the frame is WinUI's (`TextBox_themeresources.xaml`): 32px tall,
//! 4px corners, `ControlFillColorDefault` at rest, `ControlFillColorSecondary`
//! under the pointer, `ControlFillColorInputActive` while focused, a 1px
//! `ControlStrokeColorDefault` edge whose bottom 1px is
//! `ControlStrongStrokeColorDefault` at rest and a 2px accent line while
//! focused (`TextControlElevationBorderFocusedBrush`). That 2px bottom edge
//! is the text box's focus indicator; WinUI draws no focus ring around it.
//!
//! The number box follows `NumberBox.cpp`: Up and Down step by the small
//! change, Page Up and Page Down by the large one, Enter validates the
//! typed text, and Escape puts back the last value; a value outside the
//! range is pulled back into it.

use std::rc::Rc;

use gpui::{
    App, ElementId, Entity, Focusable as _, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, RenderOnce, Rgba, SharedString, StatefulInteractiveElement, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputState};
use gpui_component::{Icon, IconName};

use super::{BODY, Interaction, Key, Palette, ShownState, palette, radius, resolve, size, text};
use super::{Handler, ValueHandler};

/// The frame colors of a text field in one state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldColors {
    pub background: Rgba,
    pub border: Rgba,
    /// The bottom edge, and how thick it is.
    pub bottom: Rgba,
    pub bottom_width: Pixels,
    pub foreground: Rgba,
}

/// `TextControlBackground*`, `TextControlBorderBrush*`.
pub fn field_colors(state: Interaction, focused: bool, p: &Palette) -> FieldColors {
    match (state, focused) {
        (Interaction::Disabled, _) => FieldColors {
            background: p.control_fill_disabled,
            border: p.stroke,
            bottom: p.stroke,
            bottom_width: px(1.0),
            foreground: p.text_disabled,
        },
        (_, true) => FieldColors {
            background: p.control_fill_input_active,
            border: p.stroke,
            bottom: p.accent,
            bottom_width: px(2.0),
            foreground: p.text_primary,
        },
        (Interaction::PointerOver | Interaction::Pressed, false) => FieldColors {
            background: p.control_fill_secondary,
            border: p.stroke,
            bottom: p.strong_stroke,
            bottom_width: px(1.0),
            foreground: p.text_primary,
        },
        (Interaction::Rest, false) => FieldColors {
            background: p.control_fill,
            border: p.stroke,
            bottom: p.strong_stroke,
            bottom_width: px(1.0),
            foreground: p.text_primary,
        },
    }
}

/// What the field is for, which decides the buttons inside it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FieldKind {
    #[default]
    Text,
    /// The text is masked; a reveal button shows it while held down, as
    /// WinUI's `PasswordRevealMode="Peek"`.
    Password,
    /// A search glyph at the end, as `AutoSuggestBox`'s query button.
    Search,
}

/// A single-line text field around an `InputState` the window owns.
#[derive(IntoElement)]
pub struct TextBox {
    id: ElementId,
    state: Entity<InputState>,
    kind: FieldKind,
    header: Option<SharedString>,
    description: Option<SharedString>,
    disabled: bool,
    shown: ShownState,
    width: Option<Pixels>,
    suffix: Option<gpui::AnyElement>,
    clearable: bool,
    on_search: Option<Handler>,
}

impl TextBox {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            kind: FieldKind::Text,
            header: None,
            description: None,
            disabled: false,
            shown: ShownState::Live,
            width: None,
            suffix: None,
            clearable: true,
            on_search: None,
        }
    }

    /// A password box. The window creates its `InputState` with
    /// `.masked(true)`.
    pub fn password(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self::new(id, state).kind(FieldKind::Password)
    }

    pub fn search(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self::new(id, state).kind(FieldKind::Search)
    }

    pub fn kind(mut self, kind: FieldKind) -> Self {
        self.kind = kind;
        self
    }

    /// The label above the field (`Header`), 8px above it.
    pub fn header(mut self, header: impl Into<SharedString>) -> Self {
        self.header = Some(header.into());
        self
    }

    /// Caption text under the field.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
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
        self.width = Some(width);
        self
    }

    fn suffix(mut self, suffix: impl IntoElement) -> Self {
        self.suffix = Some(suffix.into_any_element());
        self
    }

    /// Runs when the search glyph is clicked.
    pub fn on_search(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_search = Some(Rc::new(handler));
        self
    }
}

/// A small subtle button inside a field: the clear, reveal, search, and spin
/// buttons (`TextControlButton*`, `TextBoxInnerButtonMargin` 0,4,4,4).
fn inner_button(
    id: impl Into<ElementId>,
    icon: IconName,
    p: &Palette,
) -> gpui::Stateful<gpui::Div> {
    let hover = p.subtle_fill;
    let pressed = p.subtle_fill_tertiary;
    let pressed_text = p.text_tertiary;
    div()
        .id(id)
        .w(px(30.0))
        .h(px(24.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(radius::SM)
        .text_color(p.text_secondary)
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed).text_color(pressed_text))
        .child(Icon::new(icon).size(px(12.0)))
}

impl RenderOnce for TextBox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let input = self.state.read(cx);
        let really_focused = input.focus_handle(cx).is_focused(window);
        let focused =
            !disabled && (self.shown.shows_focus() || (self.shown.is_live() && really_focused));
        let has_text = !input.value().is_empty();
        let colors = field_colors(forced.unwrap_or(Interaction::Rest), focused, p);
        let hover = field_colors(Interaction::PointerOver, false, p);

        let mut field = div()
            .id(self.id.clone())
            .relative()
            .h(size::CONTROL_HEIGHT)
            .when_some(self.width, |f, w| f.w(w))
            .when(self.width.is_none(), |f| f.w_full())
            .min_w(px(64.0))
            .flex()
            .items_center()
            .rounded(radius::SM)
            .border_1()
            .border_color(colors.border)
            .bg(colors.background)
            .overflow_hidden()
            .text_color(colors.foreground);
        if forced.is_none() && !focused {
            field = field.hover(move |s| s.bg(hover.background));
        }
        let editor = text(
            Input::new(&self.state)
                .appearance(false)
                .disabled(disabled)
                .h_full()
                .pl(px(10.0))
                .pr(px(6.0))
                .flex_1(),
            BODY,
        );
        field = field.child(editor);

        let mut buttons = div().flex().items_center().pr(px(4.0)).gap(px(2.0));
        if self.clearable && !disabled && focused && has_text && self.kind != FieldKind::Password {
            let state = self.state.clone();
            buttons = buttons.child(inner_button("clear", IconName::Close, p).on_click(
                move |_, window, cx| {
                    state.update(cx, |input, cx| {
                        input.set_value("", window, cx);
                        input.focus(window, cx);
                    })
                },
            ));
        }
        match self.kind {
            FieldKind::Password if !disabled && focused && has_text => {
                let down = self.state.clone();
                let up = self.state.clone();
                buttons = buttons.child(
                    inner_button("reveal", IconName::Eye, p)
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            down.update(cx, |input, cx| input.set_masked(false, window, cx));
                        })
                        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                            up.update(cx, |input, cx| input.set_masked(true, window, cx));
                        }),
                );
            }
            FieldKind::Search => {
                let mut search = inner_button("search", IconName::Search, p);
                if let Some(handler) = self.on_search.clone() {
                    search = search.on_click(move |_, window, cx| handler(window, cx));
                }
                buttons = buttons.child(search);
            }
            _ => {}
        }
        field = field.child(buttons).children(self.suffix);
        field = field.child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h(colors.bottom_width)
                .bg(colors.bottom),
        );

        let label_color = if disabled {
            p.text_disabled
        } else {
            p.text_primary
        };
        div()
            .flex()
            .flex_col()
            .when(self.width.is_none(), |c| c.w_full())
            .children(self.header.map(|header| {
                text(div().mb(px(8.0)), BODY)
                    .text_color(label_color)
                    .child(header)
            }))
            .child(field)
            .children(self.description.map(|description| {
                text(div().mt(px(4.0)), super::CAPTION)
                    .text_color(p.text_secondary)
                    .child(description)
            }))
    }
}

/// The range and steps of a number box, and its keyboard and validation
/// rules, without any drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberModel {
    pub minimum: f64,
    pub maximum: f64,
    /// `SmallChange`: Up, Down, and the spin buttons.
    pub small_change: f64,
    /// `LargeChange`: Page Up and Page Down.
    pub large_change: f64,
    /// `IsWrapEnabled`: stepping past one end lands on the other.
    pub wrap: bool,
}

impl Default for NumberModel {
    fn default() -> Self {
        Self {
            minimum: f64::MIN,
            maximum: f64::MAX,
            small_change: 1.0,
            large_change: 10.0,
            wrap: false,
        }
    }
}

impl NumberModel {
    pub fn new(minimum: f64, maximum: f64) -> Self {
        Self {
            minimum,
            maximum,
            ..Self::default()
        }
    }

    /// Pulls a value into the range (`InvalidInputOverwritten`).
    pub fn coerce(&self, value: f64) -> f64 {
        value.clamp(self.minimum, self.maximum)
    }

    /// `NumberBox::StepValue`: adds the change, wraps if wrapping is on,
    /// and keeps the result in range.
    pub fn step(&self, value: f64, change: f64) -> f64 {
        if value.is_nan() {
            return value;
        }
        let mut next = value + change;
        if self.wrap {
            if next > self.maximum {
                next = self.minimum;
            } else if next < self.minimum {
                next = self.maximum;
            }
        }
        self.coerce(next)
    }

    /// The value a key moves to, or `None` when the key does not step.
    pub fn key(&self, value: f64, key: Key) -> Option<f64> {
        let change = match key {
            Key::Up => self.small_change,
            Key::Down => -self.small_change,
            Key::PageUp => self.large_change,
            Key::PageDown => -self.large_change,
            _ => return None,
        };
        Some(self.step(value, change))
    }

    /// The value Enter commits for the typed text: the parsed number in
    /// range, or the current value when the text is not a number (WinUI
    /// overwrites invalid input with the last good value).
    pub fn validate(&self, typed: &str, current: f64) -> f64 {
        match typed.trim().parse::<f64>() {
            Ok(value) if value.is_finite() => self.coerce(value),
            _ => current,
        }
    }

    /// The text for a value: no trailing zeros, no `-0`.
    pub fn format(value: f64) -> String {
        if value.is_nan() {
            return String::new();
        }
        let value = if value == 0.0 { 0.0 } else { value };
        let text = format!("{value:.6}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        text.to_string()
    }
}

/// `NumberBox` with inline spin buttons. The window owns the `InputState`
/// holding the text and the value; `on_change` reports each new value, and
/// the box writes its formatted text back into the field.
#[derive(IntoElement)]
pub struct NumberBox {
    id: ElementId,
    state: Entity<InputState>,
    value: f64,
    model: NumberModel,
    header: Option<SharedString>,
    disabled: bool,
    shown: ShownState,
    width: Option<Pixels>,
    on_change: Option<ValueHandler<f64>>,
}

impl NumberBox {
    pub fn new(
        id: impl Into<ElementId>,
        state: &Entity<InputState>,
        value: f64,
        model: NumberModel,
    ) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            value,
            model,
            header: None,
            disabled: false,
            shown: ShownState::Live,
            width: None,
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
        self.width = Some(width);
        self
    }

    pub fn on_change(mut self, handler: impl Fn(f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

fn commit(
    state: &Entity<InputState>,
    value: f64,
    handler: &Option<ValueHandler<f64>>,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |input, cx| {
        input.set_value(NumberModel::format(value), window, cx)
    });
    if let Some(handler) = handler {
        handler(value, window, cx);
    }
}

impl RenderOnce for NumberBox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let disabled = self.disabled || self.shown.is_disabled();
        let model = self.model;
        let value = self.value;
        let mut spin = div().flex().items_center().gap(px(2.0)).pr(px(4.0));
        if !disabled {
            for (id, icon, change) in [
                ("spin-up", IconName::ChevronUp, model.small_change),
                ("spin-down", IconName::ChevronDown, -model.small_change),
            ] {
                let state = self.state.clone();
                let handler = self.on_change.clone();
                spin = spin.child(inner_button(id, icon, p).on_click(move |_, window, cx| {
                    let typed = state.read(cx).value().to_string();
                    let current = model.validate(&typed, value);
                    commit(&state, model.step(current, change), &handler, window, cx);
                }));
            }
        }
        let field = TextBox::new(self.id.clone(), &self.state)
            .disabled(self.disabled)
            .shown(self.shown)
            .when_some(self.header, |f, h| f.header(h))
            .when_some(self.width, |f, w| f.width(w))
            .suffix(spin);
        let field = TextBox {
            clearable: false,
            ..field
        };
        let state = self.state.clone();
        let handler = self.on_change.clone();
        div()
            .id(super::child_id(&self.id, "number"))
            .when(self.width.is_none(), |d| d.w_full())
            .when(!disabled, |d| {
                d.capture_key_down(move |event, window, cx| {
                    let Some(key) = Key::from_keystroke(&event.keystroke) else {
                        return;
                    };
                    let typed = state.read(cx).value().to_string();
                    let current = model.validate(&typed, value);
                    match key {
                        Key::Enter => {
                            cx.stop_propagation();
                            commit(&state, current, &handler, window, cx);
                        }
                        Key::Escape => {
                            cx.stop_propagation();
                            state.update(cx, |input, cx| {
                                input.set_value(NumberModel::format(value), window, cx)
                            });
                        }
                        _ => {
                            if let Some(next) = model.key(current, key) {
                                cx.stop_propagation();
                                commit(&state, next, &handler, window, cx);
                            }
                        }
                    }
                })
            })
            .child(field)
    }
}
