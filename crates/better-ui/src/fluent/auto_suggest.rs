//! `AutoSuggestBox`: a search field with a list of suggestions under it.
//!
//! The field is the kit's search [`TextBox`]; the list floats under it on the
//! flyout fill, as WinUI's suggestion popup does, each suggestion a Body line
//! with an optional Caption line under it. WinUI's keyboard rules
//! (`AutoSuggestBox`): Down and Up move the highlight through the list and
//! past either end back to the text, Enter submits the highlighted suggestion
//! or, with none highlighted, the text itself, and Escape closes the list
//! without changing anything. The list is open while the field has focus and
//! text, and stays closed after Escape until the text changes.
//!
//! What the suggestions are is the window's business: it reads the text from
//! its own `InputState`, works out the matches, and hands them in. The box
//! only draws them and reports which one was chosen.

use std::rc::Rc;

use gpui::{
    App, ElementId, Entity, Focusable as _, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, RenderOnce, SharedString, Styled, Window, anchored, deferred, div,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::input::{self, InputState};

use super::hit_area::{RowInset, group_look, hit_row};
use super::menu::panel_frame;
use super::text_box::TextBox;
use super::{BODY, CAPTION, Handler, Key, ValueHandler, palette, text, with_alpha};

/// One line in the suggestion list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    pub title: SharedString,
    pub caption: Option<SharedString>,
}

impl Suggestion {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            caption: None,
        }
    }

    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }
}

/// What a key press does to the box.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SuggestOutcome {
    /// The key is the text field's.
    Ignored,
    /// The highlight moved; `None` is back in the text.
    Highlight(Option<usize>),
    /// This suggestion was chosen.
    Choose(usize),
    /// Enter with nothing highlighted: the text itself was submitted.
    Submit,
    /// The list closed without a choice.
    Close,
}

/// WinUI's `AutoSuggestBox` keyboard rules, for a list of `len` suggestions.
pub fn suggest_key(open: bool, highlight: Option<usize>, len: usize, key: Key) -> SuggestOutcome {
    match (open, key) {
        (_, Key::Enter) => match highlight.filter(|&index| open && index < len) {
            Some(index) => SuggestOutcome::Choose(index),
            None => SuggestOutcome::Submit,
        },
        (true, Key::Escape) => SuggestOutcome::Close,
        (true, Key::Down) if len > 0 => SuggestOutcome::Highlight(match highlight {
            None => Some(0),
            Some(index) if index + 1 >= len => None,
            Some(index) => Some(index + 1),
        }),
        (true, Key::Up) if len > 0 => SuggestOutcome::Highlight(match highlight {
            None => Some(len - 1),
            Some(0) => None,
            Some(index) => Some(index - 1),
        }),
        _ => SuggestOutcome::Ignored,
    }
}

/// Whether the list is showing: the field has focus and text, and the person
/// has not closed the list for exactly this text.
pub fn list_open(focused: bool, value: &str, dismissed_for: Option<&str>) -> bool {
    focused && !value.trim().is_empty() && dismissed_for != Some(value)
}

/// What a press in the field does to the list's memory: a press while the
/// list is open closes it for this text, as a second press on any
/// light-dismiss control does; the next press opens it again.
pub fn field_pressed(open: bool, value: &str, dismissed_for: Option<&str>) -> Option<String> {
    if open {
        Some(value.to_string())
    } else if dismissed_for == Some(value) {
        None
    } else {
        dismissed_for.map(str::to_string)
    }
}

/// The box's own memory between frames.
#[derive(Default)]
struct SuggestState {
    highlight: Option<usize>,
    /// The text the list was closed for. A different text opens it again.
    dismissed_for: Option<String>,
    /// The text the highlight belongs to. A different text clears it.
    highlight_for: String,
}

/// The search field and its suggestion list.
#[derive(IntoElement)]
pub struct AutoSuggestBox {
    id: ElementId,
    state: Entity<InputState>,
    suggestions: Vec<Suggestion>,
    empty_text: Option<SharedString>,
    width: Option<Pixels>,
    on_choose: Option<ValueHandler<usize>>,
    on_submit: Option<Handler>,
}

impl AutoSuggestBox {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            suggestions: Vec::new(),
            empty_text: None,
            width: None,
            on_choose: None,
            on_submit: None,
        }
    }

    /// The suggestions for the text the field holds now.
    pub fn suggestions(mut self, suggestions: Vec<Suggestion>) -> Self {
        self.suggestions = suggestions;
        self
    }

    /// What the open list says when there is no suggestion.
    pub fn empty_text(mut self, text: impl Into<SharedString>) -> Self {
        self.empty_text = Some(text.into());
        self
    }

    /// The field's width, and the list's. Without one the field fills its
    /// container and the list is at least 240px.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// A suggestion was chosen, by a click or by Enter on the highlight.
    pub fn on_choose(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_choose = Some(Rc::new(handler));
        self
    }

    /// Enter with no suggestion highlighted.
    pub fn on_submit(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(handler));
        self
    }
}

/// The handlers one key press can reach, shared by the action and key
/// listeners.
#[derive(Clone)]
struct Wiring {
    memory: Entity<SuggestState>,
    input: Entity<InputState>,
    len: usize,
    on_choose: Option<ValueHandler<usize>>,
    on_submit: Option<Handler>,
}

impl Wiring {
    /// Runs one key through the rules. Returns whether it was used.
    fn key(&self, key: Key, window: &mut Window, cx: &mut App) -> bool {
        let input = self.input.read(cx);
        let value = input.value().to_string();
        let focused = input.focus_handle(cx).is_focused(window);
        let memory = self.memory.read(cx);
        let open = list_open(focused, &value, memory.dismissed_for.as_deref());
        let highlight = memory.highlight.filter(|_| memory.highlight_for == value);
        match suggest_key(open, highlight, self.len, key) {
            SuggestOutcome::Ignored => false,
            SuggestOutcome::Highlight(next) => {
                self.memory.update(cx, |m, cx| {
                    m.highlight = next;
                    m.highlight_for = value;
                    cx.notify();
                });
                true
            }
            SuggestOutcome::Choose(index) => {
                self.close(value, cx);
                if let Some(handler) = self.on_choose.as_ref() {
                    handler(index, window, cx);
                }
                true
            }
            SuggestOutcome::Submit => {
                self.close(value, cx);
                if let Some(handler) = self.on_submit.as_ref() {
                    handler(window, cx);
                }
                true
            }
            SuggestOutcome::Close => {
                self.close(value, cx);
                true
            }
        }
    }

    fn close(&self, value: String, cx: &mut App) {
        self.memory.update(cx, |m, cx| {
            m.highlight = None;
            m.dismissed_for = Some(value);
            cx.notify();
        });
    }
}

impl RenderOnce for AutoSuggestBox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let memory = window.use_keyed_state(super::child_id(&self.id, "suggest"), cx, |_, _| {
            SuggestState::default()
        });
        let input = self.state.read(cx);
        let value = input.value().to_string();
        let focused = input.focus_handle(cx).is_focused(window);
        let remembered = memory.read(cx);
        let open = list_open(focused, &value, remembered.dismissed_for.as_deref());
        let highlight = remembered
            .highlight
            .filter(|_| remembered.highlight_for == value);
        let wiring = Wiring {
            memory: memory.clone(),
            input: self.state.clone(),
            len: self.suggestions.len(),
            on_choose: self.on_choose.clone(),
            on_submit: self.on_submit.clone(),
        };

        let mut field = TextBox::search(super::child_id(&self.id, "field"), &self.state);
        if let Some(width) = self.width {
            field = field.width(width);
        }
        if let Some(handler) = self.on_submit.clone() {
            field = field.on_search(move |window, cx| handler(window, cx));
        }

        let list =
            (open && (!self.suggestions.is_empty() || self.empty_text.is_some())).then(|| {
                let mut list = panel_frame(div().id("suggestions"), p)
                    .child(super::light_dismiss::panel_marker(
                        &super::light_dismiss::PanelOrder::default(),
                    ))
                    .py(px(4.0))
                    .when_some(self.width, |l, w| l.w(w))
                    .when(self.width.is_none(), |l| l.min_w(px(240.0)));
                if self.suggestions.is_empty() {
                    list = list.child(
                        text(div(), BODY)
                            .px(px(12.0))
                            .py(px(8.0))
                            .text_color(p.text_secondary)
                            .whitespace_normal()
                            .children(self.empty_text.clone()),
                    );
                }
                for (index, suggestion) in self.suggestions.iter().enumerate() {
                    let hover = p.subtle_fill;
                    let pressed = p.subtle_fill_tertiary;
                    let fill = if highlight == Some(index) {
                        p.subtle_fill
                    } else {
                        with_alpha(p.subtle_fill, 0.0)
                    };
                    let choose = wiring.clone();
                    let value_now = value.clone();
                    // The inset shape is drawn inside a hit area that fills
                    // the row (`hit_area`): no dead pixels between two.
                    let look = div()
                        .id("look")
                        .px(px(12.0))
                        .py(px(6.0))
                        .rounded(super::radius::SM)
                        .bg(fill)
                        .flex()
                        .flex_col()
                        .child(
                            text(div(), BODY)
                                .text_color(p.text_primary)
                                .whitespace_normal()
                                .child(suggestion.title.clone()),
                        )
                        .children(suggestion.caption.clone().map(|caption| {
                            text(div(), CAPTION)
                                .text_color(p.text_secondary)
                                .whitespace_normal()
                                .child(caption)
                        }));
                    list = list.child(
                        hit_row(
                            ElementId::named_usize("suggestion", index),
                            RowInset::SUGGESTION,
                        )
                        .child(group_look(look, hover, pressed))
                        // A press, not a click: the field keeps its focus
                        // until the choice is made, so the list is still
                        // there when the button comes up.
                        .on_mouse_down(
                            MouseButton::Left,
                            move |_, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                choose.close(value_now.clone(), cx);
                                if let Some(handler) = choose.on_choose.as_ref() {
                                    handler(index, window, cx);
                                }
                            },
                        ),
                    );
                }
                div().absolute().top(relative(1.0)).left_0().child(
                    deferred(
                        anchored()
                            .snap_to_window_with_margin(px(8.0))
                            .child(div().mt(px(4.0)).child(list)),
                    )
                    .with_priority(1),
                )
            });

        let press_memory = wiring.memory.clone();
        let press_value = value.clone();
        let field = div()
            .id(super::child_id(&self.id, "press"))
            .capture_any_mouse_down(move |event, _, cx| {
                if event.button != MouseButton::Left {
                    return;
                }
                press_memory.update(cx, |memory, cx| {
                    memory.dismissed_for =
                        field_pressed(open, &press_value, memory.dismissed_for.as_deref());
                    cx.notify();
                });
            })
            .child(field);
        let down = wiring.clone();
        let up = wiring.clone();
        let enter = wiring.clone();
        let escape = wiring;
        div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_col()
            .when(self.width.is_none(), |d| d.w_full())
            .capture_action(move |_: &input::MoveDown, window, cx| {
                if down.key(Key::Down, window, cx) {
                    cx.stop_propagation();
                }
            })
            .capture_action(move |_: &input::MoveUp, window, cx| {
                if up.key(Key::Up, window, cx) {
                    cx.stop_propagation();
                }
            })
            .capture_action(move |_: &input::Enter, window, cx| {
                if enter.key(Key::Enter, window, cx) {
                    cx.stop_propagation();
                }
            })
            .on_key_down(move |event, window, cx| {
                if Key::from_keystroke(&event.keystroke) == Some(Key::Escape)
                    && escape.key(Key::Escape, window, cx)
                {
                    cx.stop_propagation();
                }
            })
            .child(field)
            .children(list)
    }
}
