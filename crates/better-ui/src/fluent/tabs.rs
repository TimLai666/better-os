//! The tab view and the selector bar.
//!
//! `TabView_themeresources.xaml`: tabs 32px tall, 100 to 240px wide, 12px
//! text (`TabViewItemHeaderFontSize`) with 8,3,4,3 padding, a 16px icon 10px
//! before the text, and a 32 by 24 close button with a 12px glyph. The
//! selected tab is `SolidBackgroundFillColorTertiary` with 8px top corners and
//! a 1px `CardStrokeColorDefault` edge on three sides, joined to the content
//! below it; the others are transparent with secondary text,
//! `LayerOnMicaBaseAltFillColorSecondary` under the pointer, and a 1px divider
//! between unselected neighbours.
//!
//! `SelectorBar_themeresources.xaml`: items 12,10,12,7 padded, 8px apart,
//! primary text that turns secondary under the pointer and tertiary pressed,
//! and the selected item marked by an accent pill 3px tall under its text.
//!
//! Keyboard: Ctrl+Tab and Ctrl+Shift+Tab move between tabs and wrap, Ctrl+W
//! and Ctrl+F4 close the selected tab, and Left and Right move the
//! selection while the strip has focus. A selector bar moves with Left and
//! Right.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{Icon, IconName};

use super::{
    BODY, CAPTION, Interaction, Key, ShownState, keyed_focus, palette, radius, shows_focus,
    step_index, text, with_alpha, with_focus_ring,
};
use super::{Handler, ValueHandler};

/// What a key does to a tab strip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabOutcome {
    Ignored,
    Select(usize),
    Close(usize),
}

/// The keyboard rules of a tab view.
pub fn tab_key(selected: Option<usize>, len: usize, key: Key) -> TabOutcome {
    let pick = |key, wrap| match step_index(selected, len, key, wrap, |_| false) {
        Some(next) => TabOutcome::Select(next),
        None => TabOutcome::Ignored,
    };
    match key {
        Key::NextTab => pick(Key::Right, true),
        Key::PreviousTab => pick(Key::Left, true),
        Key::Left | Key::Right | Key::Home | Key::End => pick(key, false),
        Key::CloseTab => match selected {
            Some(index) if index < len => TabOutcome::Close(index),
            _ => TabOutcome::Ignored,
        },
        _ => TabOutcome::Ignored,
    }
}

/// The selection after tab `closed` of `len` is removed: the tab that slides
/// into its place, or the one before it when it was the last; unchanged
/// indices shift down when they were after it.
pub fn after_close(selected: Option<usize>, closed: usize, len: usize) -> Option<usize> {
    let remaining = len.checked_sub(1)?;
    if remaining == 0 {
        return None;
    }
    match selected {
        Some(index) if index == closed => Some(closed.min(remaining - 1)),
        Some(index) if index > closed => Some(index - 1),
        other => other,
    }
}

/// One tab.
#[derive(Clone)]
pub struct Tab {
    pub title: SharedString,
    pub icon: Option<IconName>,
    pub closable: bool,
    pub shown: ShownState,
}

impl Tab {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            icon: None,
            closable: true,
            shown: ShownState::Live,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }
}

type IndexHandler = ValueHandler<usize>;

/// `TabView`'s strip. The window draws the selected tab's content under it
/// on [`super::Palette::tab_selected`].
#[derive(IntoElement)]
pub struct TabView {
    id: ElementId,
    tabs: Vec<Tab>,
    selected: Option<usize>,
    shown: ShownState,
    on_select: Option<IndexHandler>,
    on_close: Option<IndexHandler>,
    on_add: Option<Handler>,
    close_tooltip: Option<SharedString>,
    add_tooltip: Option<SharedString>,
}

impl TabView {
    pub fn new(id: impl Into<ElementId>, tabs: Vec<Tab>, selected: Option<usize>) -> Self {
        Self {
            id: id.into(),
            tabs,
            selected,
            shown: ShownState::Live,
            on_select: None,
            on_close: None,
            on_add: None,
            close_tooltip: None,
            add_tooltip: None,
        }
    }

    /// What the close buttons' and the `+` button's tooltips say.
    pub fn tooltips(
        mut self,
        close: impl Into<SharedString>,
        add: impl Into<SharedString>,
    ) -> Self {
        self.close_tooltip = Some(close.into());
        self.add_tooltip = Some(add.into());
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn on_close(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    /// Shows the `+` button after the last tab.
    pub fn on_add(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_add = Some(Rc::new(handler));
        self
    }
}

fn small_button(
    id: impl Into<ElementId>,
    icon: IconName,
    p: &super::Palette,
) -> gpui::Stateful<gpui::Div> {
    let hover = p.subtle_fill;
    let pressed = p.subtle_fill_tertiary;
    div()
        .id(id)
        .w(px(32.0))
        .h(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::SM)
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .child(Icon::new(icon).size(px(12.0)))
}

impl RenderOnce for TabView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = shows_focus(&focus, self.shown, window);
        let len = self.tabs.len();
        // The tabs scroll sideways when they do not fit, the selected one is
        // scrolled into view when the selection changes, and `+` stays
        // pinned after them.
        let scroll = window
            .use_keyed_state(super::child_id(&self.id, "scroll"), cx, |_, _| {
                gpui::ScrollHandle::new()
            })
            .read(cx)
            .clone();
        let shown_selection =
            window.use_keyed_state(super::child_id(&self.id, "scrolled-to"), cx, |_, _| {
                None::<usize>
            });
        if *shown_selection.read(cx) != self.selected {
            if let Some(index) = self.selected {
                scroll.scroll_to_item(index);
            }
            shown_selection.update(cx, |s, _| *s = self.selected);
        }
        let mut row = div()
            .id(super::child_id(&self.id, "tabs"))
            .flex()
            .items_end()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll()
            .track_scroll(&scroll);
        let mut strip = div()
            .id(self.id.clone())
            .flex()
            .items_end()
            .h(px(40.0))
            .pt(px(8.0))
            .pl(px(8.0))
            .border_b_1()
            .border_color(p.card_stroke);
        for (index, tab) in self.tabs.iter().enumerate() {
            let selected = self.selected == Some(index);
            let state = tab.shown.forced().unwrap_or(Interaction::Rest);
            let background = match (selected, state) {
                (true, _) => p.tab_selected,
                (false, Interaction::PointerOver) => p.tab_hover,
                (false, Interaction::Pressed) => p.tab_pressed,
                _ => with_alpha(p.tab_hover, 0.0),
            };
            let foreground = match (selected, state) {
                (_, Interaction::Disabled) => p.text_disabled,
                (true, _) => p.text_primary,
                (false, Interaction::Pressed) => p.text_tertiary,
                (false, _) => p.text_secondary,
            };
            let hover = p.tab_hover;
            let pressed = p.tab_pressed;
            let pressed_text = p.text_tertiary;
            let mut item = text(
                div()
                    .id(ElementId::named_usize("tab", index))
                    .relative()
                    .h(px(32.0))
                    .min_w(px(100.0))
                    .max_w(px(240.0))
                    .flex()
                    .items_center()
                    .pl(px(8.0))
                    .pr(px(4.0))
                    .rounded_t(radius::MD)
                    .bg(background)
                    .text_color(foreground),
                CAPTION,
            );
            if selected {
                // The selected tab sits on the strip's bottom edge, hiding it.
                item = item
                    .border_1()
                    .border_b_0()
                    .border_color(p.card_stroke)
                    .mb(px(-1.0))
                    .h(px(33.0));
            } else if tab.shown.is_live() {
                item = item
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(pressed).text_color(pressed_text));
            }
            item = item
                .children(tab.icon.clone().map(|icon| {
                    div()
                        .mr(px(10.0))
                        .child(Icon::new(icon).size(super::size::ICON))
                }))
                .child(div().flex_1().min_w_0().truncate().child(tab.title.clone()));
            if tab.closable {
                let mut close =
                    small_button(ElementId::named_usize("close", index), IconName::Close, p)
                        .ml(px(4.0));
                if let Some(tooltip) = self.close_tooltip.clone() {
                    close = close.tooltip(super::dialog::tooltip_builder(tooltip));
                }
                if let Some(handler) = self.on_close.clone() {
                    close = close.on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        handler(index, window, cx)
                    });
                }
                item = item.child(close);
            } else {
                item = item.pr(px(8.0));
            }
            if let Some(handler) = self.on_select.clone() {
                item = item.on_click(move |_, window, cx| handler(index, window, cx));
            }
            // A middle click closes a closable tab, as in every tab strip.
            if tab.closable
                && let Some(handler) = self.on_close.clone()
            {
                item = item.on_mouse_down(gpui::MouseButton::Middle, move |_, window, cx| {
                    cx.stop_propagation();
                    handler(index, window, cx)
                });
            }
            let next_selected = self.selected == Some(index + 1);
            let item = with_focus_ring(item, ring && selected, radius::SM, p);
            // One child per tab, so the scroll handle's item `index` is tab
            // `index`.
            let mut cell = div().flex().flex_none().items_end().child(item);
            if !selected && !next_selected && index + 1 < len {
                cell = cell.child(div().w(px(1.0)).h(px(16.0)).mb(px(8.0)).bg(p.divider));
            }
            row = row.child(cell);
        }
        strip = strip.child(row);
        if let Some(handler) = self.on_add.clone() {
            let mut add = small_button("add", IconName::Plus, p)
                .on_click(move |_, window, cx| handler(window, cx));
            if let Some(tooltip) = self.add_tooltip.clone() {
                add = add.tooltip(super::dialog::tooltip_builder(tooltip));
            }
            strip = strip.child(
                div()
                    .flex_none()
                    .pb(px(4.0))
                    .px(px(4.0))
                    .text_color(p.text_primary)
                    .child(add),
            );
        }
        let selected = self.selected;
        let select = self.on_select.clone();
        let close = self.on_close.clone();
        strip
            .track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()))
            .on_key_down(move |event, window, cx| {
                let Some(key) = Key::from_keystroke(&event.keystroke) else {
                    return;
                };
                match tab_key(selected, len, key) {
                    TabOutcome::Select(index) => {
                        if let Some(select) = select.as_ref() {
                            cx.stop_propagation();
                            select(index, window, cx);
                        }
                    }
                    TabOutcome::Close(index) => {
                        if let Some(close) = close.as_ref() {
                            cx.stop_propagation();
                            close(index, window, cx);
                        }
                    }
                    TabOutcome::Ignored => {}
                }
            })
    }
}

/// One item of a [`SelectorBar`].
#[derive(Clone)]
pub struct SelectorItem {
    pub label: SharedString,
    pub icon: Option<IconName>,
    pub disabled: bool,
    pub shown: ShownState,
}

impl SelectorItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            disabled: false,
            shown: ShownState::Live,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
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
}

/// The item a key moves a selector bar to.
pub fn selector_step(selected: Option<usize>, items: &[SelectorItem], key: Key) -> Option<usize> {
    match key {
        Key::Left | Key::Right | Key::Home | Key::End => {
            step_index(selected, items.len(), key, false, |i| items[i].disabled)
        }
        _ => None,
    }
}

/// `SelectorBar`: a row of views of the same content (Recent, Shared,
/// Favorites), exactly one selected and marked with an accent pill. It is
/// the control for switching between a few peers on one page, where a radio
/// group would look like a form.
#[derive(IntoElement)]
pub struct SelectorBar {
    id: ElementId,
    items: Vec<SelectorItem>,
    selected: Option<usize>,
    shown: ShownState,
    on_select: Option<IndexHandler>,
}

impl SelectorBar {
    pub fn new(
        id: impl Into<ElementId>,
        items: Vec<SelectorItem>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            items,
            selected,
            shown: ShownState::Live,
            on_select: None,
        }
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SelectorBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = shows_focus(&focus, self.shown, window);
        let items = Rc::new(self.items);
        let mut bar = div().id(self.id.clone()).flex().gap(px(8.0)).py(px(4.0));
        for (index, item) in items.iter().enumerate() {
            let selected = self.selected == Some(index);
            let forced = super::resolve(item.disabled, item.shown);
            let color = |state: Interaction| match state {
                Interaction::Rest => p.text_primary,
                Interaction::PointerOver => p.text_secondary,
                Interaction::Pressed => p.text_tertiary,
                Interaction::Disabled => p.text_disabled,
            };
            let hover = color(Interaction::PointerOver);
            let pressed = color(Interaction::Pressed);
            let mut cell = text(
                div()
                    .id(ElementId::named_usize("selector", index))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .pt(px(10.0))
                    .pb(px(7.0))
                    .px(px(12.0))
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(with_alpha(p.stroke, 0.0))
                    .text_color(color(forced.unwrap_or(Interaction::Rest))),
                BODY,
            )
            .children(item.icon.clone().map(|icon| Icon::new(icon).size(px(13.0))))
            .child(div().whitespace_nowrap().child(item.label.clone()));
            if forced.is_none() {
                cell = cell
                    .hover(move |s| s.text_color(hover))
                    .active(move |s| s.text_color(pressed));
            }
            if selected {
                let pill = if forced == Some(Interaction::Disabled) {
                    p.accent_disabled
                } else {
                    p.accent
                };
                cell = cell.child(
                    div()
                        .absolute()
                        .bottom(px(0.0))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(div().w(px(16.0)).h(px(3.0)).rounded(px(1.5)).bg(pill)),
                );
            }
            if forced != Some(Interaction::Disabled)
                && let Some(handler) = self.on_select.clone()
            {
                let focus = focus.clone();
                cell = cell.on_click(move |_, window, cx| {
                    focus.focus(window, cx);
                    handler(index, window, cx)
                });
            }
            bar = bar.child(with_focus_ring(cell, ring && selected, radius::SM, p));
        }
        let selected = self.selected;
        let handler = self.on_select.clone();
        bar.track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()))
            .on_key_down(move |event, window, cx| {
                if let Some(next) = Key::from_keystroke(&event.keystroke)
                    .and_then(|key| selector_step(selected, &items, key))
                    && let Some(handler) = handler.as_ref()
                {
                    cx.stop_propagation();
                    handler(next, window, cx);
                }
            })
    }
}
