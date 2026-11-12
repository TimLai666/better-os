//! Check box (with the indeterminate state), radio buttons, and the toggle
//! switch.
//!
//! `CheckBox_themeresources.xaml`: a 20px box with 4px corners in a 32px row,
//! the label 8px after it, a 12px glyph. `RadioButton_themeresources.xaml`: a
//! 20px circle whose checked dot is 12px at rest, 14px under the pointer,
//! and 10px pressed. `ToggleSwitch_themeresources.xaml`: a 40 by 20 track and
//! a 12px knob that grows to 14px under the pointer, with the On or Off
//! words after it.
//!
//! Keys, from the WinUI source: a check box toggles on Space (and Enter,
//! through `ButtonBase`); a toggle switch only on Space
//! (`ToggleSwitch::HandlesKey`); a radio group moves its selection with the
//! arrow keys and does not wrap (`RadioButtons.cpp`).

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce, Rgba,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName};

use super::ValueHandler;
use super::words::{Locale, on_off};
use super::{
    BODY, Interaction, Key, Palette, ShownState, keyed_focus, palette, radius, resolve,
    shows_focus, step_index, text, with_alpha, with_focus_ring,
};

/// A check box's value. `Indeterminate` is WinUI's `IsChecked == null`: some
/// of what it stands for is checked.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CheckState {
    #[default]
    Unchecked,
    Checked,
    Indeterminate,
}

impl CheckState {
    /// The value after a click, exactly as `ToggleButton::OnToggleImpl`:
    /// indeterminate becomes unchecked, unchecked becomes checked, and
    /// checked becomes indeterminate only when the box is three-state.
    pub fn toggled(self, three_state: bool) -> CheckState {
        match self {
            CheckState::Indeterminate => CheckState::Unchecked,
            CheckState::Unchecked => CheckState::Checked,
            CheckState::Checked if three_state => CheckState::Indeterminate,
            CheckState::Checked => CheckState::Unchecked,
        }
    }

    pub fn from_bool(checked: bool) -> CheckState {
        if checked {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }
    }

    /// The state of a box that stands for several others: checked when all
    /// are, unchecked when none are, indeterminate in between.
    pub fn summarize(values: impl IntoIterator<Item = bool>) -> CheckState {
        let (mut any, mut all, mut count) = (false, true, 0);
        for value in values {
            any |= value;
            all &= value;
            count += 1;
        }
        match (count, any, all) {
            (0, ..) | (_, false, _) => CheckState::Unchecked,
            (_, true, true) => CheckState::Checked,
            _ => CheckState::Indeterminate,
        }
    }
}

/// The fill, the stroke, and the glyph color of a check box's square, or of
/// a radio button's circle, in one state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkColors {
    pub fill: Rgba,
    pub stroke: Rgba,
    pub glyph: Rgba,
}

/// `CheckBoxCheckBackgroundFill*`, `*Stroke*`, and `*GlyphForeground*`.
/// Radio buttons use the same table (`RadioButtonOuterEllipse*`).
pub fn mark_colors(on: bool, state: Interaction, p: &Palette) -> MarkColors {
    match (on, state) {
        (false, Interaction::Rest) => MarkColors {
            fill: p.control_alt_fill,
            stroke: p.strong_stroke,
            glyph: p.text_on_accent,
        },
        (false, Interaction::PointerOver) => MarkColors {
            fill: p.control_alt_fill_tertiary,
            stroke: p.strong_stroke,
            glyph: p.text_on_accent,
        },
        (false, Interaction::Pressed) => MarkColors {
            fill: p.control_alt_fill_quarternary,
            stroke: p.strong_stroke_disabled,
            glyph: p.text_on_accent,
        },
        (false, Interaction::Disabled) => MarkColors {
            fill: with_alpha(p.control_alt_fill, 0.0),
            stroke: p.strong_stroke_disabled,
            glyph: p.text_on_accent_disabled,
        },
        (true, Interaction::Rest) => MarkColors {
            fill: p.accent,
            stroke: p.accent,
            glyph: p.text_on_accent,
        },
        (true, Interaction::PointerOver) => MarkColors {
            fill: p.accent_secondary,
            stroke: p.accent_secondary,
            glyph: p.text_on_accent,
        },
        (true, Interaction::Pressed) => MarkColors {
            fill: p.accent_tertiary,
            stroke: p.accent_tertiary,
            glyph: p.text_on_accent_secondary,
        },
        (true, Interaction::Disabled) => MarkColors {
            fill: p.accent_disabled,
            stroke: p.accent_disabled,
            glyph: p.text_on_accent_disabled,
        },
    }
}

const MARK: Pixels = px(20.0);
const GROUP: &str = "fluent-toggle";

fn label_color(disabled: bool, p: &Palette) -> Rgba {
    if disabled {
        p.text_disabled
    } else {
        p.text_primary
    }
}

/// Stacks a mark drawn for rest with copies drawn for pointer over and
/// pressed that only show through the row's group hover and press, so the
/// whole row, label included, is the pointer target.
fn layered(
    id: &'static str,
    draw: impl Fn(Interaction) -> gpui::Div,
    forced: Option<Interaction>,
    group: gpui::SharedString,
) -> gpui::Stateful<gpui::Div> {
    let base = div()
        .id(id)
        .relative()
        .flex_none()
        .child(draw(forced.unwrap_or(Interaction::Rest)));
    if forced.is_some() {
        return base;
    }
    base.child(
        div()
            .id("hover")
            .absolute()
            .top_0()
            .left_0()
            .opacity(0.0)
            .group_hover(group.clone(), |s| s.opacity(1.0))
            .child(draw(Interaction::PointerOver)),
    )
    .child(
        div()
            .id("pressed")
            .absolute()
            .top_0()
            .left_0()
            .opacity(0.0)
            .group_active(group, |s| s.opacity(1.0))
            .child(draw(Interaction::Pressed)),
    )
}

/// Draws the check box square for one state.
fn check_square(check: CheckState, state: Interaction, p: &Palette) -> gpui::Div {
    let colors = mark_colors(check != CheckState::Unchecked, state, p);
    let square = div()
        .size(MARK)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::SM)
        .border_1()
        .border_color(colors.stroke)
        .bg(colors.fill);
    match check {
        CheckState::Unchecked => square,
        CheckState::Checked => square.child(
            Icon::new(IconName::Check)
                .size(px(12.0))
                .text_color(colors.glyph),
        ),
        CheckState::Indeterminate => square.child(
            div()
                .w(px(8.0))
                .h(px(1.5))
                .rounded(px(1.0))
                .bg(colors.glyph),
        ),
    }
}

/// A check box and its label.
#[derive(IntoElement)]
pub struct CheckBox {
    id: ElementId,
    label: Option<SharedString>,
    state: CheckState,
    three_state: bool,
    disabled: bool,
    shown: ShownState,
    on_change: Option<ValueHandler<CheckState>>,
}

impl CheckBox {
    pub fn new(id: impl Into<ElementId>, state: CheckState) -> Self {
        Self {
            id: id.into(),
            label: None,
            state,
            three_state: false,
            disabled: false,
            shown: ShownState::Live,
            on_change: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn three_state(mut self, three_state: bool) -> Self {
        self.three_state = three_state;
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

    /// Called with the value the click or key press produced.
    pub fn on_change(
        mut self,
        handler: impl Fn(CheckState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for CheckBox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let state = self.state;

        let square = layered(
            "square",
            |interaction| check_square(state, interaction, p),
            forced,
            GROUP.into(),
        );
        let square = with_focus_ring(square, ring, radius::SM, p);

        let mut root = text(
            div()
                .id(self.id.clone())
                .group(GROUP)
                .min_h(px(32.0))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.0))
                .text_color(label_color(disabled, p)),
            BODY,
        )
        .child(square)
        .children(self.label.map(|label| div().child(label)));
        if !disabled {
            root = root.track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()));
            if let Some(handler) = self.on_change {
                let three = self.three_state;
                let on_key = handler.clone();
                root = root
                    .on_click(move |event, window, cx| {
                        // Keys are handled on key down, by the WinUI rule.
                        if !event.is_keyboard() {
                            handler(state.toggled(three), window, cx);
                        }
                    })
                    .on_key_down(move |event, window, cx| {
                        if Key::from_keystroke(&event.keystroke).is_some_and(Key::invokes) {
                            cx.stop_propagation();
                            on_key(state.toggled(three), window, cx);
                        }
                    });
            }
        }
        root
    }
}

/// Draws the radio circle for one state.
fn radio_circle(selected: bool, state: Interaction, p: &Palette) -> gpui::Div {
    let colors = mark_colors(selected, state, p);
    let dot = match (selected, state) {
        (true, Interaction::PointerOver) => Some(px(14.0)),
        (true, Interaction::Pressed) => Some(px(10.0)),
        (true, _) => Some(px(12.0)),
        (false, _) => None,
    };
    div()
        .size(MARK)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(colors.stroke)
        .bg(colors.fill)
        .children(dot.map(|size| div().size(size).rounded_full().bg(colors.glyph)))
}

/// One option of a [`RadioButtons`] group.
#[derive(Clone)]
pub struct RadioOption {
    pub label: SharedString,
    pub disabled: bool,
    pub shown: ShownState,
}

impl RadioOption {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            disabled: false,
            shown: ShownState::Live,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }
}

/// Where the selection goes on a key press in a radio group, or `None`.
pub fn radio_step(selected: Option<usize>, options: &[RadioOption], key: Key) -> Option<usize> {
    match key {
        Key::Up | Key::Down | Key::Left | Key::Right => {
            step_index(selected, options.len(), key, false, |i| options[i].disabled)
        }
        _ => None,
    }
}

/// WinUI's `RadioButtons`: a header and a column of options with exactly
/// one selected, one tab stop for the whole group, and the arrow keys
/// moving the selection. The selected option always shows its accent dot,
/// so a choice is never invisible.
#[derive(IntoElement)]
pub struct RadioButtons {
    id: ElementId,
    header: Option<SharedString>,
    options: Vec<RadioOption>,
    selected: Option<usize>,
    disabled: bool,
    horizontal: bool,
    shown: ShownState,
    on_select: Option<ValueHandler<usize>>,
}

impl RadioButtons {
    pub fn new(
        id: impl Into<ElementId>,
        options: Vec<RadioOption>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            header: None,
            options,
            selected,
            disabled: false,
            horizontal: false,
            shown: ShownState::Live,
            on_select: None,
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

    /// Options in a row rather than a column (WinUI's `MaxColumns`).
    pub fn horizontal(mut self) -> Self {
        self.horizontal = true;
        self
    }

    /// Draws the group's focus ring, for the gallery.
    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for RadioButtons {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let group_disabled = self.disabled || self.shown.is_disabled();
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !group_disabled && shows_focus(&focus, self.shown, window);
        let options = Rc::new(self.options);

        let mut list = div()
            .flex()
            .when(self.horizontal, |l| l.flex_row().gap(px(24.0)))
            .when(!self.horizontal, |l| l.flex_col());
        for (index, option) in options.iter().enumerate() {
            let disabled = group_disabled || option.disabled;
            let selected = self.selected == Some(index);
            let forced = resolve(disabled, option.shown);
            let group: SharedString = format!("fluent-radio-{index}").into();
            let circle = layered(
                "circle",
                |interaction| radio_circle(selected, interaction, p),
                forced,
                group.clone(),
            );
            // The group's one focus ring sits on its selected option.
            let circle = with_focus_ring(
                circle,
                ring && (selected || (self.selected.is_none() && index == 0)),
                px(10.0),
                p,
            );
            let mut row = text(
                div()
                    .id(ElementId::named_usize("option", index))
                    .group(group)
                    .min_h(px(32.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(label_color(disabled, p)),
                BODY,
            )
            .child(circle)
            .child(div().child(option.label.clone()));
            if !disabled && let Some(handler) = self.on_select.clone() {
                let focus = focus.clone();
                row = row.on_click(move |_, window, cx| {
                    focus.focus(window, cx);
                    handler(index, window, cx);
                });
            }
            list = list.child(row);
        }

        let mut root = div()
            .id(self.id.clone())
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(self.header.map(|header| {
                text(div(), BODY)
                    .text_color(label_color(group_disabled, p))
                    .child(header)
            }))
            .child(list);
        if !group_disabled {
            root = root.track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()));
            if let Some(handler) = self.on_select {
                let selected = self.selected;
                root = root.on_key_down(move |event, window, cx| {
                    if let Some(next) = Key::from_keystroke(&event.keystroke)
                        .and_then(|key| radio_step(selected, &options, key))
                    {
                        cx.stop_propagation();
                        handler(next, window, cx);
                    }
                });
            }
        }
        root
    }
}

/// The track and knob colors of a toggle switch in one state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchColors {
    pub track: Rgba,
    pub stroke: Rgba,
    pub knob: Rgba,
}

/// `ToggleSwitchFill*`, `ToggleSwitchStroke*`, `ToggleSwitchKnobFill*`.
pub fn switch_colors(on: bool, state: Interaction, p: &Palette) -> SwitchColors {
    let mark = mark_colors(on, state, p);
    let knob = match (on, state) {
        (false, Interaction::Disabled) => p.text_disabled,
        (false, _) => p.text_secondary,
        (true, Interaction::Disabled) => p.text_on_accent_disabled,
        (true, _) => p.text_on_accent,
    };
    let stroke = match (on, state) {
        // `ToggleSwitchStrokeOffPressed` stays the strong stroke.
        (false, Interaction::Pressed) => p.strong_stroke,
        _ => mark.stroke,
    };
    SwitchColors {
        track: mark.fill,
        stroke,
        knob,
    }
}

fn switch_track(on: bool, state: Interaction, p: &Palette) -> gpui::Div {
    let colors = switch_colors(on, state, p);
    let (width, height) = match state {
        Interaction::PointerOver => (px(14.0), px(14.0)),
        Interaction::Pressed => (px(17.0), px(14.0)),
        _ => (px(12.0), px(12.0)),
    };
    div()
        .w(px(40.0))
        .h(px(20.0))
        .flex_none()
        .flex()
        .items_center()
        .px(px(3.0))
        .when(on, |track| track.justify_end())
        .rounded(px(10.0))
        .border_1()
        .border_color(colors.stroke)
        .bg(colors.track)
        .child(div().w(width).h(height).rounded(px(7.0)).bg(colors.knob))
}

/// What a key does to a focused toggle switch: Space flips it.
pub fn switch_toggles(key: Key) -> bool {
    key == Key::Space
}

/// `ToggleSwitch`: a header above, the switch, and the On or Off words in
/// the window's language after it.
#[derive(IntoElement)]
pub struct ToggleSwitch {
    id: ElementId,
    on: bool,
    header: Option<SharedString>,
    locale: Locale,
    show_words: bool,
    disabled: bool,
    shown: ShownState,
    on_change: Option<ValueHandler<bool>>,
}

impl ToggleSwitch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            header: None,
            locale: Locale::English,
            show_words: true,
            disabled: false,
            shown: ShownState::Live,
            on_change: None,
        }
    }

    pub fn header(mut self, header: impl Into<SharedString>) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// Leaves out the On or Off words, as a settings card does, where the
    /// card's own title says what the switch is.
    pub fn without_words(mut self) -> Self {
        self.show_words = false;
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

    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ToggleSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let on = self.on;

        let track = layered(
            "track",
            |interaction| switch_track(on, interaction, p),
            forced,
            GROUP.into(),
        );
        let track = with_focus_ring(track, ring, px(10.0), p);

        let mut row = text(
            div()
                .id(self.id.clone())
                .group(GROUP)
                .min_h(px(32.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .text_color(label_color(disabled, p)),
            BODY,
        )
        .child(track)
        .when(self.show_words, |row| {
            row.child(div().whitespace_nowrap().child(on_off(self.locale, on)))
        });
        if !disabled {
            row = row.track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()));
            if let Some(handler) = self.on_change {
                let on_key = handler.clone();
                row = row
                    .on_click(move |event, window, cx| {
                        // Keys are handled on key down, by the WinUI rule.
                        if !event.is_keyboard() {
                            handler(!on, window, cx);
                        }
                    })
                    .on_key_down(move |event, window, cx| {
                        if Key::from_keystroke(&event.keystroke).is_some_and(switch_toggles) {
                            cx.stop_propagation();
                            on_key(!on, window, cx);
                        }
                    });
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(self.header.map(|header| {
                text(div(), BODY)
                    .text_color(label_color(disabled, p))
                    .child(header)
            }))
            .child(row)
    }
}
