//! The time picker (WinUI `TimePicker`): a 32px field showing the hour and
//! the minute as two segments, opening a flyout with an hour column and a
//! minute column and an accept (✓) and a dismiss (✕) button under them.
//!
//! `TimePicker_themeresources.xaml`: the field is drawn like a standard
//! button with its segments split by a 1px divider; the flyout is on the
//! flyout fill, its columns are looping selectors whose rows are 40px tall,
//! and the chosen row sits in a highlight band across the middle filled with
//! the accent colour. Better OS shows the 24-hour clock in both languages
//! (the spec's choice, section 13), so there is no AM/PM column.
//!
//! Keys (`TimePickerFlyoutPresenter`, `LoopingSelector`): a closed picker
//! opens on Enter, Space, Alt+Down, or F4. Open, Up and Down move the focused
//! column one step and wrap at either end, Page Up and Page Down move it by
//! a larger step, Home and End go to its first and last value, Left and Right
//! move between the two columns, Enter accepts the time shown, and Escape
//! dismisses the flyout without a change. Nothing changes until the time is
//! accepted, which is also what the ✓ button does.

use std::rc::Rc;

use gpui::{
    App, ElementId, Entity, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{Icon, IconName};

use super::ValueHandler;
use super::button::{self, ButtonKind};
use super::light_dismiss::{self, PanelEdge};
use super::menu::panel_frame;
use super::{
    BODY, Interaction, Key, ShownState, keyed_focus, paint, palette, radius, resolve, shows_focus,
    size, text, with_focus_ring,
};

/// A time of day on the 24-hour clock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Time {
    pub hour: u8,
    pub minute: u8,
}

impl Time {
    /// `None` for an hour past 23 or a minute past 59.
    pub fn new(hour: u8, minute: u8) -> Option<Self> {
        (hour < 24 && minute < 60).then_some(Self { hour, minute })
    }

    /// Minutes since midnight.
    pub fn minutes(self) -> u32 {
        u32::from(self.hour) * 60 + u32::from(self.minute)
    }

    /// From minutes since midnight, wrapped to one day.
    pub fn from_minutes(minutes: u32) -> Self {
        let minutes = minutes % (24 * 60);
        Self {
            hour: (minutes / 60) as u8,
            minute: (minutes % 60) as u8,
        }
    }

    /// `18:05`: two digits each, as both languages write it.
    pub fn format(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// One of the picker's two columns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Column {
    #[default]
    Hour,
    Minute,
}

/// The picker's settings: how far apart the minutes it offers are
/// (`MinuteIncrement`, 1 by default as in WinUI).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeModel {
    pub minute_increment: u8,
}

impl Default for TimeModel {
    fn default() -> Self {
        Self {
            minute_increment: 1,
        }
    }
}

impl TimeModel {
    pub fn new(minute_increment: u8) -> Self {
        Self {
            minute_increment: minute_increment.clamp(1, 30),
        }
    }

    /// The values a column offers, in order.
    pub fn values(&self, column: Column) -> Vec<u8> {
        match column {
            Column::Hour => (0..24).collect(),
            Column::Minute => (0..60)
                .step_by(usize::from(self.minute_increment))
                .collect(),
        }
    }

    /// A time with its minute pulled down onto the increment, as WinUI does
    /// with a `Time` that is not on it.
    pub fn coerce(&self, time: Time) -> Time {
        let step = self.minute_increment.max(1);
        Time {
            hour: time.hour.min(23),
            minute: (time.minute.min(59) / step) * step,
        }
    }

    /// Moves one column by `steps` of its own values, wrapping at either end
    /// (a looping selector). The other column is left alone.
    pub fn step(&self, time: Time, column: Column, steps: i32) -> Time {
        let time = self.coerce(time);
        let values = self.values(column);
        let current = match column {
            Column::Hour => time.hour,
            Column::Minute => time.minute,
        };
        let len = values.len() as i32;
        let at = values.iter().position(|v| *v == current).unwrap_or(0) as i32;
        let next = values[((at + steps).rem_euclid(len)) as usize];
        match column {
            Column::Hour => Time { hour: next, ..time },
            Column::Minute => Time {
                minute: next,
                ..time
            },
        }
    }

    /// Sets a column to its first or last value.
    pub fn end(&self, time: Time, column: Column, last: bool) -> Time {
        let values = self.values(column);
        let value = if last {
            *values.last().unwrap_or(&0)
        } else {
            values[0]
        };
        let time = self.coerce(time);
        match column {
            Column::Hour => Time {
                hour: value,
                ..time
            },
            Column::Minute => Time {
                minute: value,
                ..time
            },
        }
    }

    /// How many steps Page Up and Page Down move: a quarter of a day for the
    /// hours, a quarter of an hour's values for the minutes.
    pub fn page(&self, column: Column) -> i32 {
        (self.values(column).len() / 4).max(1) as i32
    }
}

/// Whether the flyout is open, the time it shows, and its focused column.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PickerOpen {
    pub open: bool,
    pub pending: Time,
    pub column: Column,
}

/// What a key press does to a time picker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PickerOutcome {
    Ignored,
    /// The flyout opened, showing the current time, on the hour column.
    Open,
    /// Open: the time shown or the focused column changed.
    Pending(PickerOpen),
    /// Open: this time was accepted and the flyout closed.
    Accept(Time),
    /// Open: closed without a change.
    Dismiss,
}

/// The keyboard rules of a time picker.
pub fn picker_key(model: &TimeModel, state: PickerOpen, key: Key) -> PickerOutcome {
    if !state.open {
        return match key {
            Key::Enter | Key::Space | Key::OpenDropDown => PickerOutcome::Open,
            _ => PickerOutcome::Ignored,
        };
    }
    let moved = |pending: Time| PickerOutcome::Pending(PickerOpen { pending, ..state });
    let column = state.column;
    match key {
        Key::Up => moved(model.step(state.pending, column, -1)),
        Key::Down => moved(model.step(state.pending, column, 1)),
        Key::PageUp => moved(model.step(state.pending, column, -model.page(column))),
        Key::PageDown => moved(model.step(state.pending, column, model.page(column))),
        Key::Home => moved(model.end(state.pending, column, false)),
        Key::End => moved(model.end(state.pending, column, true)),
        Key::Left | Key::Right => {
            let next = match (column, key) {
                (Column::Hour, Key::Right) => Column::Minute,
                (Column::Minute, Key::Left) => Column::Hour,
                _ => return PickerOutcome::Ignored,
            };
            PickerOutcome::Pending(PickerOpen {
                column: next,
                ..state
            })
        }
        Key::Enter | Key::Space | Key::OpenDropDown => {
            PickerOutcome::Accept(model.coerce(state.pending))
        }
        Key::Escape => PickerOutcome::Dismiss,
        _ => PickerOutcome::Ignored,
    }
}

/// How many rows of each column the flyout shows: the chosen one in the
/// middle and three on either side.
pub const VISIBLE_ROWS: usize = 7;

/// A row's height in the flyout.
pub const ROW_HEIGHT: f32 = 40.0;

/// The values a column draws, top to bottom, around the chosen one, wrapped
/// as a looping selector wraps.
pub fn visible_values(model: &TimeModel, time: Time, column: Column) -> Vec<u8> {
    let half = (VISIBLE_ROWS / 2) as i32;
    (-half..=half)
        .map(|offset| {
            let moved = model.step(time, column, offset);
            match column {
                Column::Hour => moved.hour,
                Column::Minute => moved.minute,
            }
        })
        .collect()
}

/// `TimePicker`: the field, its flyout, and the time it was given.
#[derive(IntoElement)]
pub struct TimePicker {
    id: ElementId,
    time: Time,
    model: TimeModel,
    header: Option<SharedString>,
    disabled: bool,
    shown: ShownState,
    /// Draw the flyout open in the page, on this column, for the gallery.
    show_open: Option<Column>,
    width: Pixels,
    on_change: Option<ValueHandler<Time>>,
}

impl TimePicker {
    pub fn new(id: impl Into<ElementId>, time: Time) -> Self {
        Self {
            id: id.into(),
            time,
            model: TimeModel::default(),
            header: None,
            disabled: false,
            shown: ShownState::Live,
            show_open: None,
            width: px(242.0),
            on_change: None,
        }
    }

    pub fn model(mut self, model: TimeModel) -> Self {
        self.model = model;
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

    /// WinUI's default is 242px; a narrower one still fits "23 | 59".
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    /// Draws the flyout open, in the page, with this column focused.
    pub fn show_open(mut self, column: Column) -> Self {
        self.show_open = Some(column);
        self
    }

    /// Called with the accepted time; never while the flyout is only
    /// showing one.
    pub fn on_change(mut self, handler: impl Fn(Time, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

fn close(state: &Entity<PickerOpen>, cx: &mut App) {
    state.update(cx, |s, cx| {
        *s = PickerOpen::default();
        cx.notify();
    });
}

fn set(state: &Entity<PickerOpen>, value: PickerOpen, cx: &mut App) {
    state.update(cx, |s, cx| {
        *s = value;
        cx.notify();
    });
}

/// The flyout: two looping columns and the accept and dismiss buttons.
fn flyout(
    id: &ElementId,
    model: TimeModel,
    open: PickerOpen,
    width: Pixels,
    interactive: Option<(Entity<PickerOpen>, Option<ValueHandler<Time>>)>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let p = palette(cx);
    let band_top = px(ROW_HEIGHT * (VISIBLE_ROWS / 2) as f32);
    let column_view = |column: Column| {
        let values = visible_values(&model, open.pending, column);
        let focused = open.column == column;
        let mut col = div()
            .id(super::child_id(
                id,
                match column {
                    Column::Hour => "hours",
                    Column::Minute => "minutes",
                },
            ))
            .flex_1()
            .flex()
            .flex_col()
            .relative();
        for (row, value) in values.into_iter().enumerate() {
            let middle = row == VISIBLE_ROWS / 2;
            let color = if middle {
                p.text_on_accent
            } else {
                p.text_primary
            };
            let mut cell = text(
                div()
                    .id(ElementId::named_usize(
                        match column {
                            Column::Hour => "hour",
                            Column::Minute => "minute",
                        },
                        row,
                    ))
                    .h(px(ROW_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(color),
                BODY,
            )
            .child(format!("{value:02}"));
            if !middle && let Some((state, _)) = interactive.clone() {
                let hover = p.subtle_fill;
                let picked = match column {
                    Column::Hour => Time {
                        hour: value,
                        ..open.pending
                    },
                    Column::Minute => Time {
                        minute: value,
                        ..open.pending
                    },
                };
                cell =
                    cell.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(move |_, _, cx| {
                            set(
                                &state,
                                PickerOpen {
                                    open: true,
                                    pending: picked,
                                    column,
                                },
                                cx,
                            )
                        });
            }
            col = col.child(cell);
        }
        if focused {
            // The focused column's band carries a stronger edge, which is
            // how the keyboard user sees which column the arrows move.
            col = col.child(
                div()
                    .absolute()
                    .left(px(2.0))
                    .right(px(2.0))
                    .top(band_top)
                    .h(px(ROW_HEIGHT))
                    .rounded(radius::SM)
                    .border_2()
                    .border_color(p.focus_outer),
            );
        }
        col
    };
    let band = div()
        .absolute()
        .left(px(4.0))
        .right(px(4.0))
        .top(band_top)
        .h(px(ROW_HEIGHT))
        .rounded(radius::SM)
        .bg(p.accent);
    let columns = div()
        .relative()
        .flex()
        .h(px(ROW_HEIGHT * VISIBLE_ROWS as f32))
        .child(band)
        .child(column_view(Column::Hour))
        .child(div().w(px(1.0)).h_full().bg(p.divider))
        .child(column_view(Column::Minute));

    let footer_button = |part: &'static str, icon: IconName| {
        let look = div()
            .id(super::child_id(id, part))
            .flex_1()
            .h(px(40.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::SM)
            .text_color(p.text_primary)
            .child(Icon::new(icon).size(px(16.0)));
        let hover = p.subtle_fill;
        let pressed = p.subtle_fill_tertiary;
        look.hover(move |s| s.bg(hover))
            .active(move |s| s.bg(pressed))
    };
    let mut accept = footer_button("accept", IconName::Check);
    let mut dismiss = footer_button("dismiss", IconName::Close);
    if let Some((state, handler)) = interactive {
        let accept_state = state.clone();
        accept = accept.on_click(move |_, window, cx| {
            let time = model.coerce(accept_state.read(cx).pending);
            close(&accept_state, cx);
            if let Some(handler) = handler.as_ref() {
                handler(time, window, cx);
            }
        });
        dismiss = dismiss.on_click(move |_, _, cx| close(&state, cx));
    }
    panel_frame(div().id(super::child_id(id, "flyout")), p)
        .occlude()
        .w(width)
        .flex()
        .flex_col()
        .child(columns)
        .child(div().h(px(1.0)).bg(p.divider))
        .child(
            div()
                .flex()
                .gap(px(4.0))
                .p(px(4.0))
                .child(accept)
                .child(dismiss),
        )
}

impl RenderOnce for TimePicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let state = window.use_keyed_state(super::child_id(&self.id, "open"), cx, |_, _| {
            PickerOpen::default()
        });
        let open = *state.read(cx);
        let model = self.model;
        let time = model.coerce(self.time);
        let colors = button::colors(ButtonKind::Standard, false, p);
        let text_color = if disabled {
            p.text_disabled
        } else {
            p.text_primary
        };
        let segment = |value: u8| {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(text_color)
                .child(format!("{value:02}"))
        };
        let mut field = div()
            .id(self.id.clone())
            .h(size::CONTROL_HEIGHT)
            .w(self.width)
            .flex()
            .flex_none()
            .items_center();
        field = text(field, BODY);
        if !disabled {
            field = field.track_focus(&focus.clone().tab_index(0).tab_stop(self.shown.is_live()));
        }
        let mut field = paint(field, colors, forced, radius::SM, p.is_dark())
            .child(segment(time.hour))
            .child(div().w(px(1.0)).h(px(20.0)).bg(p.divider))
            .child(segment(time.minute));
        if !disabled {
            let click_state = state.clone();
            field = field.on_click(move |event, _, cx| {
                if event.is_keyboard() {
                    return;
                }
                let now = *click_state.read(cx);
                set(
                    &click_state,
                    if now.open {
                        PickerOpen::default()
                    } else {
                        PickerOpen {
                            open: true,
                            pending: time,
                            column: Column::Hour,
                        }
                    },
                    cx,
                );
            });
            let key_state = state.clone();
            let handler = self.on_change.clone();
            field = field.on_key_down(move |event, window, cx| {
                let Some(key) = Key::from_keystroke(&event.keystroke) else {
                    return;
                };
                let now = *key_state.read(cx);
                match picker_key(&model, now, key) {
                    PickerOutcome::Ignored => return,
                    PickerOutcome::Open => set(
                        &key_state,
                        PickerOpen {
                            open: true,
                            pending: time,
                            column: Column::Hour,
                        },
                        cx,
                    ),
                    PickerOutcome::Pending(next) => set(&key_state, next, cx),
                    PickerOutcome::Accept(accepted) => {
                        close(&key_state, cx);
                        if let Some(handler) = handler.as_ref() {
                            handler(accepted, window, cx);
                        }
                    }
                    PickerOutcome::Dismiss => close(&key_state, cx),
                }
                cx.stop_propagation();
            });
        }
        let field = with_focus_ring(field, ring, radius::SM, p);

        let inline = self.show_open.map(|column| {
            div().mt(px(4.0)).child(flyout(
                &self.id,
                model,
                PickerOpen {
                    open: true,
                    pending: time,
                    column,
                },
                self.width,
                None,
                cx,
            ))
        });
        let overlay = if self.show_open.is_some() || disabled || !open.open {
            None
        } else {
            let out_state = state.clone();
            let order = light_dismiss::PanelOrder::default();
            let out_order = order.clone();
            let panel = flyout(
                &self.id,
                model,
                open,
                self.width,
                Some((state.clone(), self.on_change.clone())),
                cx,
            )
            .child(light_dismiss::panel_marker(&order))
            .on_mouse_down_out(move |_, window, cx| {
                if !light_dismiss::pointer_in_child_panel(&out_order, window, cx) {
                    close(&out_state, cx)
                }
            });
            Some(light_dismiss::anchored_panel(panel, PanelEdge::Left))
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
            .children(inline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u8, minute: u8) -> Time {
        Time::new(hour, minute).unwrap()
    }

    fn opened(time: Time, column: Column) -> PickerOpen {
        PickerOpen {
            open: true,
            pending: time,
            column,
        }
    }

    #[test]
    fn a_time_is_written_with_two_digits_on_the_24_hour_clock() {
        assert_eq!(at(18, 5).format(), "18:05");
        assert_eq!(at(0, 0).format(), "00:00");
        assert_eq!(Time::new(24, 0), None);
        assert_eq!(Time::new(1, 60), None);
        assert_eq!(Time::from_minutes(25 * 60 + 3), at(1, 3));
        assert_eq!(at(23, 59).minutes(), 1439);
    }

    #[test]
    fn a_closed_picker_opens_on_the_drop_down_keys_and_ignores_the_arrows() {
        let model = TimeModel::default();
        let closed = PickerOpen::default();
        for key in [Key::Enter, Key::Space, Key::OpenDropDown] {
            assert_eq!(picker_key(&model, closed, key), PickerOutcome::Open);
        }
        for key in [Key::Up, Key::Down, Key::Escape, Key::Left] {
            assert_eq!(picker_key(&model, closed, key), PickerOutcome::Ignored);
        }
    }

    #[test]
    fn the_arrows_move_the_focused_column_and_wrap_like_a_looping_selector() {
        let model = TimeModel::default();
        let state = opened(at(23, 59), Column::Hour);
        assert_eq!(
            picker_key(&model, state, Key::Down),
            PickerOutcome::Pending(opened(at(0, 59), Column::Hour))
        );
        let state = opened(at(0, 0), Column::Minute);
        assert_eq!(
            picker_key(&model, state, Key::Up),
            PickerOutcome::Pending(opened(at(0, 59), Column::Minute)),
            "the minutes wrap without touching the hour"
        );
        assert_eq!(
            picker_key(&model, opened(at(9, 30), Column::Hour), Key::PageDown),
            PickerOutcome::Pending(opened(at(15, 30), Column::Hour))
        );
        assert_eq!(
            picker_key(&model, opened(at(9, 30), Column::Minute), Key::End),
            PickerOutcome::Pending(opened(at(9, 59), Column::Minute))
        );
        assert_eq!(
            picker_key(&model, opened(at(9, 30), Column::Hour), Key::Home),
            PickerOutcome::Pending(opened(at(0, 30), Column::Hour))
        );
    }

    #[test]
    fn left_and_right_move_between_the_columns_and_stop_at_the_edges() {
        let model = TimeModel::default();
        assert_eq!(
            picker_key(&model, opened(at(1, 2), Column::Hour), Key::Right),
            PickerOutcome::Pending(opened(at(1, 2), Column::Minute))
        );
        assert_eq!(
            picker_key(&model, opened(at(1, 2), Column::Minute), Key::Left),
            PickerOutcome::Pending(opened(at(1, 2), Column::Hour))
        );
        assert_eq!(
            picker_key(&model, opened(at(1, 2), Column::Minute), Key::Right),
            PickerOutcome::Ignored
        );
    }

    #[test]
    fn enter_accepts_what_is_shown_and_escape_changes_nothing() {
        let model = TimeModel::default();
        assert_eq!(
            picker_key(&model, opened(at(7, 45), Column::Minute), Key::Enter),
            PickerOutcome::Accept(at(7, 45))
        );
        assert_eq!(
            picker_key(&model, opened(at(7, 45), Column::Minute), Key::Escape),
            PickerOutcome::Dismiss
        );
    }

    #[test]
    fn a_minute_increment_offers_only_its_minutes() {
        let model = TimeModel::new(15);
        assert_eq!(model.values(Column::Minute), vec![0, 15, 30, 45]);
        assert_eq!(model.coerce(at(8, 44)), at(8, 30));
        assert_eq!(model.step(at(8, 45), Column::Minute, 1), at(8, 0));
        assert_eq!(model.step(at(8, 50), Column::Minute, -1), at(8, 30));
        assert_eq!(TimeModel::new(0).minute_increment, 1);
    }

    #[test]
    fn the_flyout_shows_the_chosen_value_in_the_middle_of_its_neighbours() {
        let model = TimeModel::default();
        assert_eq!(
            visible_values(&model, at(1, 58), Column::Hour),
            vec![22, 23, 0, 1, 2, 3, 4]
        );
        assert_eq!(
            visible_values(&model, at(1, 58), Column::Minute),
            vec![55, 56, 57, 58, 59, 0, 1]
        );
        assert_eq!(
            visible_values(&model, at(1, 58), Column::Hour).len(),
            VISIBLE_ROWS
        );
    }
}
