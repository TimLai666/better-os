//! `NavigationView` in its left mode: a pane of items beside the content,
//! open at 320px or compact at 48px, never hidden.
//!
//! `NavigationView_themeresources.xaml`: items at least 36px tall
//! (`NavigationViewItemOnLeftMinHeight`), 4,2 apart from the pane and each
//! other, with 4px corners, a 40px icon column for a 16px icon, and the text
//! after it; `SubtleFillColorSecondary` under the pointer and while
//! selected, `SubtleFillColorTertiary` pressed, and a 3 by 16 accent
//! indicator with 2px corners at the selected item's leading edge. Section
//! headers are in secondary text (`NavigationViewItemHeaderForeground`). The
//! pane toggle is a subtle button in a 40px column. The pane sits on the
//! window background; the content beside it is `LayerFillColorDefault` with
//! a 1px `CardStrokeColorDefault` edge on its top and leading sides and an
//! 8px top-leading corner (`NavigationViewContentGridCornerRadius`).
//!
//! Keyboard: the pane is one tab stop; Up and Down move through the items,
//! skipping headers and separators and stopping at the ends; Home and End
//! jump; Enter or Space selects. In the compact pane each item names itself
//! in a tooltip.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName};

use super::ValueHandler;
use super::button::{Button, ButtonKind};
use super::words::Locale;
use super::{
    BODY, BODY_STRONG, Interaction, Key, Palette, ShownState, keyed_focus, palette, radius,
    shows_focus, size, step_index, text, with_alpha, with_focus_ring,
};

/// One line of the pane.
#[derive(Clone)]
pub enum NavEntry {
    Item(NavItem),
    /// A section header. In the compact pane it becomes a separator.
    Header(SharedString),
    Separator,
}

/// A destination.
#[derive(Clone)]
pub struct NavItem {
    pub label: SharedString,
    pub icon: IconName,
    pub disabled: bool,
    pub shown: ShownState,
}

impl NavItem {
    pub fn new(label: impl Into<SharedString>, icon: IconName) -> Self {
        Self {
            label: label.into(),
            icon,
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

    pub fn entry(self) -> NavEntry {
        NavEntry::Item(self)
    }
}

impl NavEntry {
    fn selectable(&self) -> bool {
        matches!(self, NavEntry::Item(item) if !item.disabled)
    }
}

/// The pane's width.
pub fn pane_width(compact: bool) -> Pixels {
    if compact {
        size::ICON_PANE_WIDTH
    } else {
        size::LIST_PANE_WIDTH
    }
}

/// Where Up, Down, Home, and End move the pane's keyboard position, over the
/// items followed by the footer items, counted as one list.
pub fn nav_step(current: Option<usize>, entries: &[NavEntry], key: Key) -> Option<usize> {
    match key {
        Key::Up | Key::Down | Key::Home | Key::End => {
            step_index(current, entries.len(), key, false, |i| {
                !entries[i].selectable()
            })
        }
        _ => None,
    }
}

/// An item's fill in one state (`NavigationViewItemBackground*`).
pub fn item_fill(selected: bool, state: Interaction, p: &Palette) -> gpui::Rgba {
    match (selected, state) {
        (true, Interaction::PointerOver) => p.subtle_fill_tertiary,
        (_, Interaction::Pressed) => p.subtle_fill_tertiary,
        (true, _) => p.subtle_fill,
        (false, Interaction::PointerOver) => p.subtle_fill,
        (false, _) => with_alpha(p.subtle_fill, 0.0),
    }
}

/// The toggle's tooltip, which says what it will do.
pub fn toggle_tooltip(locale: Locale, compact: bool) -> &'static str {
    match (locale, compact) {
        (Locale::English, true) => "Open navigation",
        (Locale::English, false) => "Close navigation",
        (Locale::TraditionalChinese, true) => "展開導覽窗格",
        (Locale::TraditionalChinese, false) => "收合導覽窗格",
    }
}

/// The keyboard position in a pane, kept across frames.
#[derive(Default)]
struct Cursor(Option<usize>);

type IndexHandler = ValueHandler<usize>;

/// The pane and the content beside it.
#[derive(IntoElement)]
pub struct NavigationView {
    id: ElementId,
    entries: Vec<NavEntry>,
    footer: Vec<NavItem>,
    selected: Option<usize>,
    compact: bool,
    title: Option<SharedString>,
    locale: Locale,
    content: Option<AnyElement>,
    shown: ShownState,
    on_select: Option<IndexHandler>,
    on_toggle: Option<ValueHandler<bool>>,
}

impl NavigationView {
    /// `selected` indexes the entries and then the footer items, as one list.
    pub fn new(id: impl Into<ElementId>, entries: Vec<NavEntry>, selected: Option<usize>) -> Self {
        Self {
            id: id.into(),
            entries,
            footer: Vec::new(),
            selected,
            compact: false,
            title: None,
            locale: Locale::English,
            content: None,
            shown: ShownState::Live,
            on_select: None,
            on_toggle: None,
        }
    }

    /// Items pinned to the bottom of the pane, such as Settings.
    pub fn footer(mut self, footer: Vec<NavItem>) -> Self {
        self.footer = footer;
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// The pane's title, beside the toggle when the pane is open.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    /// Draws the pane's focus ring, for the gallery.
    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Called with the compact state the toggle asks for.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

struct RowContext<'a> {
    compact: bool,
    focus: gpui::FocusHandle,
    on_select: Option<IndexHandler>,
    p: &'a Palette,
}

fn item_row(
    index: usize,
    item: &NavItem,
    selected: bool,
    ring: bool,
    c: &RowContext,
) -> AnyElement {
    let p = c.p;
    let forced = super::resolve(item.disabled, item.shown);
    let disabled = forced == Some(Interaction::Disabled);
    let state = forced.unwrap_or(Interaction::Rest);
    let foreground = match state {
        Interaction::Disabled => p.text_disabled,
        Interaction::Pressed => p.text_secondary,
        _ => p.text_primary,
    };
    let hover = item_fill(selected, Interaction::PointerOver, p);
    let pressed = item_fill(selected, Interaction::Pressed, p);
    let pressed_text = p.text_secondary;
    let mut row = text(
        div()
            .id(ElementId::named_usize("nav-item", index))
            .relative()
            .h(px(36.0))
            .mx(px(4.0))
            .my(px(2.0))
            .flex()
            .flex_none()
            .items_center()
            .rounded(radius::SM)
            .bg(item_fill(selected, state, p))
            .text_color(foreground),
        BODY,
    )
    .child(
        div()
            .w(px(40.0))
            .flex_none()
            .flex()
            .justify_center()
            .child(Icon::new(item.icon.clone()).size(size::ICON)),
    )
    .when(!c.compact, |row| {
        row.child(
            div()
                .ml(px(4.0))
                .mr(px(8.0))
                .flex_1()
                .min_w_0()
                .truncate()
                .child(item.label.clone()),
        )
    });
    if selected {
        row = row.child(
            div()
                .absolute()
                .left_0()
                .top(px(10.0))
                .w(size::SELECTION_BAR_WIDTH)
                .h(size::SELECTION_BAR_HEIGHT)
                .rounded(px(2.0))
                .bg(if disabled {
                    p.accent_disabled
                } else {
                    p.accent
                }),
        );
    }
    if forced.is_none() {
        row = row
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(pressed).text_color(pressed_text));
    }
    if c.compact {
        row = row.tooltip(super::dialog::tooltip_builder(item.label.clone()));
    }
    if !disabled && let Some(handler) = c.on_select.clone() {
        let focus = c.focus.clone();
        row = row.on_click(move |_, window, cx| {
            focus.focus(window, cx);
            handler(index, window, cx)
        });
    }
    with_focus_ring(row, ring, radius::SM, p).into_any_element()
}

fn divider(p: &Palette) -> AnyElement {
    div()
        .mx(px(4.0))
        .my(px(4.0))
        .h(px(1.0))
        .bg(p.divider)
        .into_any_element()
}

impl RenderOnce for NavigationView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = shows_focus(&focus, self.shown, window);
        let cursor = window.use_keyed_state(super::child_id(&self.id, "cursor"), cx, |_, _| {
            Cursor::default()
        });
        let at = cursor.read(cx).0.or(self.selected);
        let compact = self.compact;
        let all: Vec<NavEntry> = self
            .entries
            .iter()
            .cloned()
            .chain(self.footer.iter().cloned().map(NavEntry::Item))
            .collect();
        let main_len = self.entries.len();
        let row_context = RowContext {
            compact,
            focus: focus.clone(),
            on_select: self.on_select.clone(),
            p,
        };

        let mut toggle = Button::new(super::child_id(&self.id, "toggle"), ButtonKind::Subtle)
            .icon(IconName::Menu)
            .tooltip(toggle_tooltip(self.locale, compact));
        if let Some(handler) = self.on_toggle.clone() {
            toggle = toggle.on_click(move |window, cx| handler(!compact, window, cx));
        }
        let top = div()
            .h(px(40.0))
            .flex()
            .flex_none()
            .items_center()
            .px(px(4.0))
            .gap(px(4.0))
            .child(div().w(px(40.0)).flex().justify_center().child(toggle))
            .when(!compact, |top| {
                top.children(self.title.clone().map(|title| {
                    text(div(), BODY)
                        .text_color(p.text_primary)
                        .truncate()
                        .child(title)
                }))
            });

        let mut list = div().flex().flex_col().flex_1().min_h_0();
        for (index, entry) in self.entries.iter().enumerate() {
            list = list.child(match entry {
                NavEntry::Item(item) => item_row(
                    index,
                    item,
                    self.selected == Some(index),
                    ring && at == Some(index),
                    &row_context,
                ),
                NavEntry::Header(label) if !compact => text(div(), BODY_STRONG)
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .px(px(16.0))
                    .text_color(p.text_secondary)
                    .truncate()
                    .child(label.clone())
                    .into_any_element(),
                NavEntry::Header(_) | NavEntry::Separator => divider(p),
            });
        }
        let mut footer = div().flex().flex_col().flex_none().pb(px(4.0));
        if !self.footer.is_empty() {
            footer = footer.child(divider(p));
        }
        for (offset, item) in self.footer.iter().enumerate() {
            let index = main_len + offset;
            footer = footer.child(item_row(
                index,
                item,
                self.selected == Some(index),
                ring && at == Some(index),
                &row_context,
            ));
        }

        let handler = self.on_select.clone();
        let key_cursor = cursor.clone();
        let selected = self.selected;
        let pane = div()
            .id(self.id.clone())
            .w(pane_width(compact))
            .h_full()
            .flex()
            .flex_col()
            .flex_none()
            .overflow_hidden()
            .child(top)
            .child(list)
            .child(footer)
            .track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()))
            .on_key_down(move |event, window, cx| {
                let Some(key) = Key::from_keystroke(&event.keystroke) else {
                    return;
                };
                let now = key_cursor.read(cx).0.or(selected);
                if key.invokes() {
                    if let (Some(index), Some(handler)) = (now, handler.as_ref())
                        && all.get(index).is_some_and(NavEntry::selectable)
                    {
                        cx.stop_propagation();
                        handler(index, window, cx);
                    }
                } else if let Some(next) = nav_step(now, &all, key) {
                    cx.stop_propagation();
                    key_cursor.update(cx, |c, cx| {
                        c.0 = Some(next);
                        cx.notify();
                    });
                }
            });

        let content = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(p.layer)
            .border_t_1()
            .border_l_1()
            .border_color(p.card_stroke)
            .rounded_tl(radius::MD)
            .overflow_hidden()
            .children(self.content);
        div()
            .flex()
            .size_full()
            .min_h_0()
            .bg(p.window_background)
            .child(pane)
            .child(content)
    }
}
