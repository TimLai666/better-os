//! List rows with a selection fill and bar, grid tiles, and a details table
//! with sortable column headers.
//!
//! `ListViewItem_themeresources.xaml`: rows with 4px corners on a
//! transparent fill, `SubtleFillColorSecondary` under the pointer and when
//! selected, `SubtleFillColorTertiary` pressed, and a 3 by 16 accent
//! indicator with 1.5px corners at the leading edge of the selected row.
//! Selection is a fill and a bar; never an outline.
//!
//! Keyboard: the list is one tab stop; Up, Down, Home, and End move the
//! selection and stop at the ends; Enter invokes the selected row. A sortable
//! header sorts on Enter or Space, and a second press reverses the order.
//!
//! [`ListRow`] is one `ListViewItem` on its own, for a window whose list
//! holds more than text — an application's own icon, a trailing button, a
//! line that opens under the title — and whose keyboard model lives in the
//! window. [`GridTile`] is one `GridViewItem` as the Windows 11 Start menu
//! draws a pinned app: a 32px icon over a two-line Caption label, with a
//! fill under the pointer and no frame. Tiles can be dragged to reorder
//! them; [`move_item`] is the reordering rule.

use std::cmp::Ordering;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Point, Render, RenderOnce, Rgba, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName};

use crate::AppIcon;

use super::ValueHandler;
use super::{
    BODY, BODY_STRONG, CAPTION, Interaction, Key, Palette, ShownState, keyed_focus, palette,
    radius, shows_focus, size, step_index, text, with_alpha, with_focus_ring,
};

/// A row's fill in one state, selected or not.
pub fn row_fill(selected: bool, state: Interaction, p: &Palette) -> Rgba {
    match (selected, state) {
        (_, Interaction::Pressed) => p.subtle_fill_tertiary,
        (true, Interaction::PointerOver) => p.subtle_fill_tertiary,
        (true, _) => p.subtle_fill,
        (false, Interaction::PointerOver) => p.subtle_fill,
        (false, _) => with_alpha(p.subtle_fill, 0.0),
    }
}

/// The 3 by 16 accent bar at a selected row's leading edge.
pub fn selection_bar(p: &Palette, disabled: bool) -> gpui::Div {
    div()
        .w(size::SELECTION_BAR_WIDTH)
        .h(size::SELECTION_BAR_HEIGHT)
        .rounded(px(1.5))
        .bg(if disabled {
            p.accent_disabled
        } else {
            p.accent
        })
}

/// One row of a [`ListView`].
#[derive(Clone)]
pub struct ListItem {
    pub title: SharedString,
    pub caption: Option<SharedString>,
    pub icon: Option<IconName>,
    pub disabled: bool,
    pub shown: ShownState,
}

impl ListItem {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            caption: None,
            icon: None,
            disabled: false,
            shown: ShownState::Live,
        }
    }

    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
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

/// The row a key moves a list's selection to.
pub fn list_step(selected: Option<usize>, items: &[ListItem], key: Key) -> Option<usize> {
    match key {
        Key::Up | Key::Down | Key::Home | Key::End => {
            step_index(selected, items.len(), key, false, |i| items[i].disabled)
        }
        _ => None,
    }
}

type IndexHandler = ValueHandler<usize>;

/// A list of rows, one selected.
#[derive(IntoElement)]
pub struct ListView {
    id: ElementId,
    items: Vec<ListItem>,
    selected: Option<usize>,
    shown: ShownState,
    on_select: Option<IndexHandler>,
    on_invoke: Option<IndexHandler>,
}

impl ListView {
    pub fn new(id: impl Into<ElementId>, items: Vec<ListItem>, selected: Option<usize>) -> Self {
        Self {
            id: id.into(),
            items,
            selected,
            shown: ShownState::Live,
            on_select: None,
            on_invoke: None,
        }
    }

    /// Draws the list's focus ring, for the gallery.
    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Enter on the selected row, or a double click.
    pub fn on_invoke(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_invoke = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ListView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = shows_focus(&focus, self.shown, window);
        let items = Rc::new(self.items);
        let mut list = div().id(self.id.clone()).flex().flex_col().gap(px(2.0));
        for (index, item) in items.iter().enumerate() {
            let selected = self.selected == Some(index);
            let forced = super::resolve(item.disabled, item.shown);
            let two_line = item.caption.is_some();
            let fill = row_fill(selected, forced.unwrap_or(Interaction::Rest), p);
            let hover = row_fill(selected, Interaction::PointerOver, p);
            let pressed = row_fill(selected, Interaction::Pressed, p);
            let disabled = forced == Some(Interaction::Disabled);
            let (title_color, caption_color) = if disabled {
                (p.text_disabled, p.text_disabled)
            } else {
                (p.text_primary, p.text_secondary)
            };
            let mut row = div()
                .id(ElementId::named_usize("row", index))
                .relative()
                .h(if two_line {
                    size::TWO_LINE_ROW_HEIGHT
                } else {
                    size::ROW_HEIGHT
                })
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .rounded(radius::SM)
                .bg(fill);
            if forced.is_none() {
                row = row
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(pressed));
            }
            if selected {
                row = row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(selection_bar(p, disabled)),
                );
            }
            row = row
                .children(
                    item.icon
                        .clone()
                        .map(|icon| Icon::new(icon).size(size::ICON).text_color(caption_color)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            text(div(), BODY)
                                .text_color(title_color)
                                .truncate()
                                .child(item.title.clone()),
                        )
                        .children(item.caption.clone().map(|caption| {
                            text(div(), CAPTION)
                                .text_color(caption_color)
                                .truncate()
                                .child(caption)
                        })),
                );
            if !disabled {
                let select = self.on_select.clone();
                let invoke = self.on_invoke.clone();
                let focus = focus.clone();
                row = row.on_click(move |event, window, cx| {
                    focus.focus(window, cx);
                    if event.click_count() >= 2 {
                        if let Some(invoke) = invoke.as_ref() {
                            invoke(index, window, cx);
                        }
                    } else if let Some(select) = select.as_ref() {
                        select(index, window, cx);
                    }
                });
            }
            let row = with_focus_ring(row, ring && selected, radius::SM, p);
            list = list.child(row);
        }
        let selected = self.selected;
        let select = self.on_select.clone();
        let invoke = self.on_invoke.clone();
        list.track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()))
            .on_key_down(move |event, window, cx| {
                let Some(key) = Key::from_keystroke(&event.keystroke) else {
                    return;
                };
                if key == Key::Enter {
                    if let (Some(index), Some(invoke)) = (selected, invoke.as_ref()) {
                        cx.stop_propagation();
                        invoke(index, window, cx);
                    }
                } else if let Some(next) = list_step(selected, &items, key)
                    && let Some(select) = select.as_ref()
                {
                    cx.stop_propagation();
                    select(next, window, cx);
                }
            })
    }
}

/// What stands at the leading edge of a [`ListRow`].
#[derive(Clone)]
pub enum Leading {
    /// A 16px glyph in secondary text, as [`ListView`] draws one.
    Glyph(IconName),
    /// An application's own icon, at the row's icon size.
    App(AppIcon),
    /// A 16px glyph in a status color, for a row that reports how something
    /// went (an activity log).
    Status(IconName, super::info_bar::Severity),
}

/// The color a status glyph is drawn in: the severity's own color, which is
/// the color an info bar gives its icon.
pub fn status_glyph_color(severity: super::info_bar::Severity, p: &Palette) -> Rgba {
    super::info_bar::severity_look(severity, p).1
}

/// The size an application icon is drawn at in a row: 24px beside one line,
/// 32px beside a second line of caption text, as File Explorer and Windows
/// 11's Start search draw them.
pub fn row_icon_size(two_line: bool) -> Pixels {
    if two_line { px(32.0) } else { px(24.0) }
}

/// A handler told about a click, with the event (its click count).
pub type ClickHandler = Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;

/// A handler told where a secondary click happened, in window coordinates.
pub type PointHandler = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;

/// The letter tile behind an application with no icon file: accent fill and
/// text on accent, 4px corners.
fn app_icon(icon: &AppIcon, size: Pixels, p: &Palette) -> AnyElement {
    icon.element(size, p.text_on_accent.into(), p.accent.into(), radius::SM)
}

/// One `ListViewItem`: the selection fill and bar of [`ListView`], with an
/// application icon or a glyph, an optional trailing element, and an
/// optional block under the title that grows the row. The window keeps the
/// list's keyboard model; `highlighted` draws the keyboard focus visual on
/// the row the window's keyboard is on.
#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    title: SharedString,
    caption: Option<SharedString>,
    leading: Option<Leading>,
    trailing: Option<AnyElement>,
    detail: Option<AnyElement>,
    selected: bool,
    highlighted: bool,
    disabled: bool,
    shown: ShownState,
    on_click: Option<ClickHandler>,
    on_secondary: Option<PointHandler>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            caption: None,
            leading: None,
            trailing: None,
            detail: None,
            selected: false,
            highlighted: false,
            disabled: false,
            shown: ShownState::Live,
            on_click: None,
            on_secondary: None,
        }
    }

    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    pub fn glyph(mut self, icon: IconName) -> Self {
        self.leading = Some(Leading::Glyph(icon));
        self
    }

    pub fn app_icon(mut self, icon: AppIcon) -> Self {
        self.leading = Some(Leading::App(icon));
        self
    }

    /// A glyph in a status color at the leading edge.
    pub fn status_glyph(mut self, icon: IconName, severity: super::info_bar::Severity) -> Self {
        self.leading = Some(Leading::Status(icon, severity));
        self
    }

    /// Something at the trailing edge, such as an icon button.
    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
        self
    }

    /// A block under the title and caption, inside the row.
    pub fn detail(mut self, detail: impl IntoElement) -> Self {
        self.detail = Some(detail.into_any_element());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Draws the keyboard focus visual: the row the window's keyboard is on.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
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

    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// A right click, with where it happened, for a context menu.
    pub fn on_secondary(
        mut self,
        handler: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ListRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = super::resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let two_line = self.caption.is_some();
        let fill = row_fill(self.selected, forced.unwrap_or(Interaction::Rest), p);
        let hover = row_fill(self.selected, Interaction::PointerOver, p);
        let pressed = row_fill(self.selected, Interaction::Pressed, p);
        let (title_color, caption_color) = if disabled {
            (p.text_disabled, p.text_disabled)
        } else {
            (p.text_primary, p.text_secondary)
        };
        let has_detail = self.detail.is_some();
        let mut row = div()
            .id(self.id)
            .relative()
            .w_full()
            .min_h(if two_line {
                size::TWO_LINE_ROW_HEIGHT
            } else {
                size::ROW_HEIGHT
            })
            .px(px(12.0))
            .when(has_detail, |row| row.py(px(8.0)))
            .flex()
            .items_center()
            .gap(px(12.0))
            .rounded(radius::SM)
            .bg(fill);
        if forced.is_none() && !disabled {
            row = row
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed));
        }
        if self.selected {
            row = row.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(selection_bar(p, disabled)),
            );
        }
        let leading: Option<AnyElement> = self.leading.map(|leading| match leading {
            Leading::Glyph(icon) => Icon::new(icon)
                .size(size::ICON)
                .text_color(caption_color)
                .into_any_element(),
            Leading::App(icon) => app_icon(&icon, row_icon_size(two_line), p),
            Leading::Status(icon, severity) => Icon::new(icon)
                .size(size::ICON)
                .text_color(if disabled {
                    p.text_disabled
                } else {
                    status_glyph_color(severity, p)
                })
                .into_any_element(),
        });
        row = row
            .children(leading.map(|leading| div().flex_none().child(leading)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        text(div(), BODY)
                            .text_color(title_color)
                            .truncate()
                            .child(self.title),
                    )
                    .children(self.caption.map(|caption| {
                        text(div(), CAPTION)
                            .text_color(caption_color)
                            .truncate()
                            .child(caption)
                    }))
                    .children(self.detail),
            )
            .children(self.trailing.map(|t| div().flex_none().child(t)));
        if !disabled {
            if let Some(handler) = self.on_click {
                row = row.on_click(move |event, window, cx| handler(event, window, cx));
            }
            if let Some(handler) = self.on_secondary {
                row = row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                    cx.stop_propagation();
                    handler(event.position, window, cx);
                });
            }
        }
        with_focus_ring(row, self.highlighted && !disabled, radius::SM, p)
    }
}

/// Moves the item at `from` to sit at `to`, the way a dragged tile lands
/// where it is dropped: everything between shifts by one. Out-of-range
/// indices change nothing. Returns whether the order changed.
pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from >= items.len() || to >= items.len() || from == to {
        return false;
    }
    let item = items.remove(from);
    items.insert(to, item);
    true
}

/// A Start-menu tile's size: `GridViewItem` at Windows 11's pinned-app
/// metrics, a 32px icon 16px from the top over two lines of Caption.
pub const TILE_WIDTH: Pixels = px(96.0);
pub const TILE_HEIGHT: Pixels = px(84.0);

/// The body inside a tile's 1px edge.
pub const TILE_BODY_WIDTH: Pixels = px(94.0);
pub const TILE_BODY_HEIGHT: Pixels = px(82.0);
/// The icon inside a tile.
pub const TILE_ICON: Pixels = px(32.0);

/// What a dragged [`GridTile`] carries. `group` keeps tiles of two grids
/// from landing in each other.
#[derive(Clone, Debug)]
pub struct DraggedTile {
    pub group: SharedString,
    pub index: usize,
    pub label: SharedString,
    pub icon: AppIcon,
}

/// The tile drawn under the pointer while it is dragged: the tile itself on
/// the flyout fill, so it reads as lifted off the page.
pub struct TileDragPreview {
    tile: DraggedTile,
}

impl Render for TileDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        tile_body(
            div()
                .bg(p.flyout)
                .border_1()
                .border_color(p.flyout_stroke)
                .rounded(radius::SM)
                .shadow_md(),
            &self.tile.icon,
            self.tile.label.clone(),
            p.text_primary,
            p,
        )
    }
}

fn tile_body(
    element: gpui::Div,
    icon: &AppIcon,
    label: SharedString,
    color: Rgba,
    p: &Palette,
) -> gpui::Div {
    // Both a tile and its drag preview wear a 1px edge around this body, so
    // the whole is exactly `TILE_WIDTH` by `TILE_HEIGHT`.
    element
        .w(TILE_BODY_WIDTH)
        .h(TILE_BODY_HEIGHT)
        .flex()
        .flex_col()
        .items_center()
        .pt(px(11.0))
        .gap(px(6.0))
        .child(app_icon(icon, TILE_ICON, p))
        .child(
            text(div(), CAPTION)
                .w_full()
                .px(px(4.0))
                .text_center()
                .text_color(color)
                .line_clamp(2)
                .child(label),
        )
}

type ReorderHandler = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;

/// One `GridViewItem`, as a pinned app in the Windows 11 Start menu.
#[derive(IntoElement)]
pub struct GridTile {
    id: ElementId,
    icon: AppIcon,
    label: SharedString,
    highlighted: bool,
    shown: ShownState,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
    on_secondary: Option<PointHandler>,
    reorder: Option<(SharedString, usize, ReorderHandler)>,
}

impl GridTile {
    pub fn new(id: impl Into<ElementId>, icon: AppIcon, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            highlighted: false,
            shown: ShownState::Live,
            tooltip: None,
            on_click: None,
            on_secondary: None,
            reorder: None,
        }
    }

    /// Draws the keyboard focus visual: the tile the window's keyboard is on.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    /// The full name, for a label two lines cannot hold.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn on_secondary(
        mut self,
        handler: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary = Some(Rc::new(handler));
        self
    }

    /// Makes the tile draggable within `group`; a tile of the same group
    /// dropped on this one calls `handler(from, to)`.
    pub fn reorderable(
        mut self,
        group: impl Into<SharedString>,
        index: usize,
        handler: impl Fn(usize, usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.reorder = Some((group.into(), index, Rc::new(handler)));
        self
    }
}

impl RenderOnce for GridTile {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = self.shown.forced();
        let fill = row_fill(false, forced.unwrap_or(Interaction::Rest), p);
        let hover = row_fill(false, Interaction::PointerOver, p);
        let pressed = row_fill(false, Interaction::Pressed, p);
        // The 1px edge is always there, transparent until a dragged tile is
        // over this one, so showing it never moves the tile.
        let mut tile = div()
            .id(self.id)
            .relative()
            .flex_none()
            .rounded(radius::SM)
            .border_1()
            .border_color(with_alpha(p.accent, 0.0))
            .bg(if self.highlighted { hover } else { fill });
        if forced.is_none() {
            tile = tile
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed));
        }
        let body = tile_body(div(), &self.icon, self.label.clone(), p.text_primary, p);
        tile = tile.child(body);
        if let Some(tooltip) = self.tooltip {
            tile = tile.tooltip(super::dialog::tooltip_builder(tooltip));
        }
        if let Some(handler) = self.on_click {
            tile = tile.on_click(move |event, window, cx| handler(event, window, cx));
        }
        if let Some(handler) = self.on_secondary {
            tile = tile.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                cx.stop_propagation();
                handler(event.position, window, cx);
            });
        }
        if let Some((group, index, handler)) = self.reorder {
            let dragged = DraggedTile {
                group: group.clone(),
                index,
                label: self.label.clone(),
                icon: self.icon.clone(),
            };
            let accent = p.accent;
            let over_group = group.clone();
            tile = tile
                .on_drag(dragged, |tile, _, _, cx| {
                    cx.new(|_| TileDragPreview { tile: tile.clone() })
                })
                .drag_over::<DraggedTile>(move |style, dragged, _, _| {
                    if dragged.group == over_group && dragged.index != index {
                        style.border_color(accent)
                    } else {
                        style
                    }
                })
                .on_drop(move |dragged: &DraggedTile, window, cx| {
                    if dragged.group == group {
                        handler(dragged.index, index, window, cx);
                    }
                });
        }
        with_focus_ring(tile, self.highlighted, radius::SM, p)
    }
}

/// Which column a table is sorted by, and which way.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SortState {
    pub column: usize,
    pub ascending: bool,
}

impl SortState {
    /// The sort after a click on `column`: a new column sorts ascending, the
    /// same column reverses.
    pub fn clicked(current: Option<SortState>, column: usize) -> SortState {
        match current {
            Some(sort) if sort.column == column => SortState {
                column,
                ascending: !sort.ascending,
            },
            _ => SortState {
                column,
                ascending: true,
            },
        }
    }
}

/// Compares two cells as a person expects: numbers by value (byte units
/// such as `KB` and `MB` scaled, thousands separators ignored), everything
/// else without regard to case, and runs of digits inside text by value, so
/// `file2` sorts before `file10`.
pub fn compare_cells(a: &str, b: &str) -> Ordering {
    /// A cell that is a number, or a number and a byte unit (`640 KB`,
    /// `1.2 MB`), as its value in bytes. Anything else is text.
    fn number(cell: &str) -> Option<f64> {
        let cell = cell.trim();
        let end = cell
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-')))
            .map_or(cell.len(), |(at, _)| at);
        let digits: String = cell[..end].chars().filter(|c| *c != ',').collect();
        let value: f64 = digits.parse().ok()?;
        let scale = match cell[end..].trim().to_ascii_uppercase().as_str() {
            "" => 1.0,
            "B" => 1.0,
            "KB" | "KIB" => 1024.0,
            "MB" | "MIB" => 1024.0_f64.powi(2),
            "GB" | "GIB" => 1024.0_f64.powi(3),
            "TB" | "TIB" => 1024.0_f64.powi(4),
            _ => return None,
        };
        Some(value * scale)
    }
    if let (Some(x), Some(y)) = (number(a), number(b)) {
        return x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    }
    let (mut left, mut right) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut run = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        run.push(c);
                        chars.next();
                    }
                    run
                };
                let (rx, ry) = (take(&mut left), take(&mut right));
                let (tx, ty) = (rx.trim_start_matches('0'), ry.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                left.next();
                right.next();
            }
        }
    }
}

/// The order to draw rows in for a sort: indices into `rows`. The sort is
/// stable, so rows that compare equal keep their order.
pub fn sorted_order(rows: &[Vec<SharedString>], sort: Option<SortState>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    if let Some(sort) = sort {
        order.sort_by(|&a, &b| {
            let x = rows[a].get(sort.column).map(|c| c.as_ref()).unwrap_or("");
            let y = rows[b].get(sort.column).map(|c| c.as_ref()).unwrap_or("");
            let order = compare_cells(x, y);
            if sort.ascending {
                order
            } else {
                order.reverse()
            }
        });
    }
    order
}

/// One column of a [`DetailsTable`].
#[derive(Clone)]
pub struct Column {
    pub title: SharedString,
    /// A fixed width, or `None` to share what is left.
    pub width: Option<Pixels>,
    pub sortable: bool,
    /// Numbers read best aligned to the end.
    pub align_end: bool,
    /// Said when the pointer rests on the header, for a column whose short
    /// title needs explaining.
    pub tooltip: Option<SharedString>,
}

impl Column {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            width: None,
            sortable: true,
            align_end: false,
            tooltip: None,
        }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn align_end(mut self) -> Self {
        self.align_end = true;
        self
    }

    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

/// How a cell's text is colored. Status tones always travel with words, never
/// alone.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CellTone {
    #[default]
    Primary,
    /// A value that is not a reading (`Not collected`), or metadata.
    Secondary,
    Caution,
    Critical,
}

/// One cell of a [`DetailsTable`]: text, and how it is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableCell {
    pub text: SharedString,
    pub tone: CellTone,
    /// Body Strong, for a row's name.
    pub strong: bool,
    /// Levels of indentation, 16px each, for the children of a group.
    pub indent: u8,
    /// `Some(expanded)` draws a chevron before the text, for a row that
    /// opens to show its children.
    pub expander: Option<bool>,
    /// Said when the pointer rests on the cell.
    pub tooltip: Option<SharedString>,
}

impl TableCell {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn tone(mut self, tone: CellTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn strong(mut self) -> Self {
        self.strong = true;
        self
    }

    pub fn indent(mut self, levels: u8) -> Self {
        self.indent = levels;
        self
    }

    pub fn expander(mut self, expanded: bool) -> Self {
        self.expander = Some(expanded);
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

impl From<SharedString> for TableCell {
    fn from(text: SharedString) -> Self {
        TableCell::new(text)
    }
}

impl From<&'static str> for TableCell {
    fn from(text: &'static str) -> Self {
        TableCell::new(text)
    }
}

impl From<String> for TableCell {
    fn from(text: String) -> Self {
        TableCell::new(text)
    }
}

/// The row a key moves a table's selection to, by position on screen.
/// `None` when the key does not move it.
pub fn table_step(selected_position: Option<usize>, rows: usize, key: Key) -> Option<usize> {
    match key {
        Key::Up | Key::Down | Key::Home | Key::End => {
            step_index(selected_position, rows, key, false, |_| false)
        }
        _ => None,
    }
}

/// How far in from a row's leading edge a keyboard-opened menu appears.
const MENU_INSET: Pixels = px(48.0);

/// Where the Menu key opens a row's context menu: under the row at
/// `position`, given the rows' viewport and how far it is scrolled
/// (`scroll_y` is zero or negative, as GPUI reports it). A row scrolled out
/// of view puts the menu at the nearest edge of the viewport.
pub fn menu_anchor(
    viewport: gpui::Bounds<Pixels>,
    scroll_y: Pixels,
    position: usize,
) -> gpui::Point<Pixels> {
    let below_row = viewport.origin.y + scroll_y + size::ROW_HEIGHT * (position as f32 + 1.0);
    let y = below_row.clamp(viewport.origin.y, viewport.bottom());
    gpui::point(viewport.origin.x + MENU_INSET, y)
}

type ContextHandler = Rc<dyn Fn(usize, gpui::Point<Pixels>, &mut Window, &mut App)>;

/// A details table: column headers that sort, and rows of cells with the
/// list's selection fill and bar.
///
/// By default the table sorts its own rows by their text and draws all of
/// them. A window whose rows come from a model it sorts itself calls
/// [`DetailsTable::presorted`], and one with more rows than fit calls
/// [`DetailsTable::fill`], which lets the table take the height it is given
/// and draws only the rows in view, scrolling the rest.
///
/// Keyboard: each sortable header is a tab stop that sorts on Enter or Space;
/// the rows are one tab stop where Up, Down, Home, and End move the selection
/// and Enter invokes the selected row. A right click, or the Menu key or
/// Shift+F10 on the selected row, asks for its context menu.
#[derive(IntoElement)]
pub struct DetailsTable {
    id: ElementId,
    columns: Vec<Column>,
    rows: Vec<Vec<TableCell>>,
    sort: Option<SortState>,
    presorted: bool,
    fill: bool,
    selected: Option<usize>,
    empty: Option<SharedString>,
    on_sort: Option<ValueHandler<SortState>>,
    on_select: Option<IndexHandler>,
    on_invoke: Option<IndexHandler>,
    on_context_menu: Option<ContextHandler>,
}

impl DetailsTable {
    pub fn new<C: Into<TableCell>>(
        id: impl Into<ElementId>,
        columns: Vec<Column>,
        rows: Vec<Vec<C>>,
    ) -> Self {
        Self {
            id: id.into(),
            columns,
            rows: rows
                .into_iter()
                .map(|row| row.into_iter().map(Into::into).collect())
                .collect(),
            sort: None,
            presorted: false,
            fill: false,
            selected: None,
            empty: None,
            on_sort: None,
            on_select: None,
            on_invoke: None,
            on_context_menu: None,
        }
    }

    /// The sort the headers show. Without [`DetailsTable::presorted`] the
    /// table also orders its rows by it.
    pub fn sort(mut self, sort: Option<SortState>) -> Self {
        self.sort = sort;
        self
    }

    /// The rows are already in the order to draw them; the headers still show
    /// the sort and report clicks.
    pub fn presorted(mut self) -> Self {
        self.presorted = true;
        self
    }

    /// Take the height the parent gives, draw only the rows in view, and
    /// scroll the rest under a header that stays put.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// The selected row, as an index into the rows given (not the sorted
    /// order).
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// What to say when there are no rows.
    pub fn empty(mut self, message: impl Into<SharedString>) -> Self {
        self.empty = Some(message.into());
        self
    }

    pub fn on_sort(mut self, handler: impl Fn(SortState, &mut Window, &mut App) + 'static) -> Self {
        self.on_sort = Some(Rc::new(handler));
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// A double click, or Enter on the selected row.
    pub fn on_invoke(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_invoke = Some(Rc::new(handler));
        self
    }

    /// A right click on a row, with the pointer's window position, or the
    /// Menu key on the selected row, with the row's position. The row is
    /// selected first.
    pub fn on_context_menu(
        mut self,
        handler: impl Fn(usize, gpui::Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_context_menu = Some(Rc::new(handler));
        self
    }
}

fn cell(column: &Column) -> gpui::Div {
    let cell = div().flex().items_center().px(px(12.0)).min_w_0();
    let cell = match column.width {
        Some(width) => cell.w(width).flex_none(),
        None => cell.flex_1(),
    };
    if column.align_end {
        cell.justify_end()
    } else {
        cell
    }
}

/// The text color of a cell's tone.
pub fn tone_color(tone: CellTone, p: &Palette) -> Rgba {
    match tone {
        CellTone::Primary => p.text_primary,
        CellTone::Secondary => p.text_secondary,
        CellTone::Caution => p.caution,
        CellTone::Critical => p.critical,
    }
}

struct RowParts {
    columns: Rc<Vec<Column>>,
    rows: Rc<Vec<Vec<TableCell>>>,
    order: Rc<Vec<usize>>,
    selected: Option<usize>,
    ring: bool,
    focus: gpui::FocusHandle,
    on_select: Option<IndexHandler>,
    on_invoke: Option<IndexHandler>,
    on_context_menu: Option<ContextHandler>,
}

fn table_row(position: usize, parts: &RowParts, p: &Palette) -> gpui::AnyElement {
    let row_index = parts.order[position];
    let selected = parts.selected == Some(row_index);
    let fill = row_fill(selected, Interaction::Rest, p);
    let hover = row_fill(selected, Interaction::PointerOver, p);
    let pressed = row_fill(selected, Interaction::Pressed, p);
    let mut row = div()
        .id(ElementId::named_usize("table-row", row_index))
        .relative()
        .flex()
        .flex_none()
        .w_full()
        .h(size::ROW_HEIGHT)
        .rounded(radius::SM)
        .bg(fill)
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed));
    if selected {
        row = row.child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .child(selection_bar(p, false)),
        );
    }
    for (index, column) in parts.columns.iter().enumerate() {
        let value = parts.rows[row_index]
            .get(index)
            .cloned()
            .unwrap_or_default();
        let color = tone_color(value.tone, p);
        let mut content = cell(column).gap(px(4.0));
        if value.indent > 0 {
            content = content.pl(px(12.0 + 16.0 * value.indent as f32));
        }
        if let Some(expanded) = value.expander {
            content = content.child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(12.0))
                .flex_none()
                .text_color(p.text_secondary),
            );
        }
        let label = text(div(), if value.strong { BODY_STRONG } else { BODY })
            .text_color(color)
            .min_w_0()
            .truncate()
            .child(value.text.clone());
        let label = match value.tooltip.clone() {
            Some(tip) => div()
                .id(ElementId::named_usize("cell-tip", row_index * 64 + index))
                .min_w_0()
                .flex()
                .child(label)
                .tooltip(super::dialog::tooltip_builder(tip))
                .into_any_element(),
            None => label.into_any_element(),
        };
        row = row.child(content.child(label));
    }
    {
        let select = parts.on_select.clone();
        let invoke = parts.on_invoke.clone();
        let focus = parts.focus.clone();
        row = row.on_click(move |event, window, cx| {
            if event.is_keyboard() {
                return;
            }
            focus.focus(window, cx);
            if event.click_count() >= 2 {
                if let Some(invoke) = invoke.as_ref() {
                    invoke(row_index, window, cx);
                }
            } else if let Some(select) = select.as_ref() {
                select(row_index, window, cx);
            }
        });
    }
    if let Some(menu) = parts.on_context_menu.clone() {
        let select = parts.on_select.clone();
        let focus = parts.focus.clone();
        row = row.on_mouse_down(gpui::MouseButton::Right, move |event, window, cx| {
            cx.stop_propagation();
            focus.focus(window, cx);
            if let Some(select) = select.as_ref() {
                select(row_index, window, cx);
            }
            menu(row_index, event.position, window, cx);
        });
    }
    with_focus_ring(row, parts.ring && selected, radius::SM, p).into_any_element()
}

impl RenderOnce for DetailsTable {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let mut header = div()
            .flex()
            .flex_none()
            .h(size::CONTROL_HEIGHT)
            .border_b_1()
            .border_color(p.divider);
        for (index, column) in self.columns.iter().enumerate() {
            let sorted = self.sort.filter(|sort| sort.column == index);
            let id = ElementId::named_usize("header", index);
            let header_focus = keyed_focus(
                ElementId::named_usize(SharedString::from(format!("{}-header", self.id)), index),
                window,
                cx,
            );
            let ring = shows_focus(&header_focus, ShownState::Live, window);
            let hover = p.subtle_fill;
            let pressed = p.subtle_fill_tertiary;
            let mut head = cell(column)
                .id(id)
                .relative()
                .gap(px(4.0))
                .rounded(radius::SM)
                .child(
                    text(div(), BODY_STRONG)
                        .text_color(p.text_secondary)
                        .min_w_0()
                        .truncate()
                        .child(column.title.clone()),
                )
                .children(sorted.map(|sort| {
                    Icon::new(if sort.ascending {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .size(px(12.0))
                    .flex_none()
                    .text_color(p.text_secondary)
                }));
            if let Some(tip) = column.tooltip.clone() {
                head = head.tooltip(super::dialog::tooltip_builder(tip));
            }
            if column.sortable
                && let Some(handler) = self.on_sort.clone()
            {
                let current = self.sort;
                let on_key = handler.clone();
                head = head
                    .track_focus(&header_focus.tab_index(0).tab_stop(true))
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(pressed))
                    .on_click(move |event, window, cx| {
                        if event.is_keyboard() {
                            return;
                        }
                        handler(SortState::clicked(current, index), window, cx)
                    })
                    .on_key_down(move |event, window, cx| {
                        if Key::from_keystroke(&event.keystroke).is_some_and(Key::invokes) {
                            cx.stop_propagation();
                            on_key(SortState::clicked(current, index), window, cx);
                        }
                    });
            }
            header = header.child(with_focus_ring(head, ring, radius::SM, p));
        }

        let order: Vec<usize> = if self.presorted {
            (0..self.rows.len()).collect()
        } else {
            let texts: Vec<Vec<SharedString>> = self
                .rows
                .iter()
                .map(|row| row.iter().map(|cell| cell.text.clone()).collect())
                .collect();
            sorted_order(&texts, self.sort)
        };
        let focus = keyed_focus(super::child_id(&self.id, "rows"), window, cx);
        let ring = shows_focus(&focus, ShownState::Live, window);
        let scroll = window
            .use_keyed_state(super::child_id(&self.id, "scroll"), cx, |_, _| {
                gpui::UniformListScrollHandle::new()
            })
            .read(cx)
            .clone();
        let rows_handle = window
            .use_keyed_state(super::child_id(&self.id, "rows-handle"), cx, |_, _| {
                gpui::ScrollHandle::new()
            })
            .read(cx)
            .clone();
        let count = order.len();
        let parts = Rc::new(RowParts {
            columns: Rc::new(self.columns.clone()),
            rows: Rc::new(self.rows),
            order: Rc::new(order),
            selected: self.selected,
            ring,
            focus: focus.clone(),
            on_select: self.on_select.clone(),
            on_invoke: self.on_invoke.clone(),
            on_context_menu: self.on_context_menu.clone(),
        });

        let body: gpui::AnyElement = if count == 0 {
            text(div(), BODY)
                .p(px(12.0))
                .text_color(p.text_secondary)
                .child(self.empty.clone().unwrap_or_default())
                .into_any_element()
        } else if self.fill {
            let row_parts = parts.clone();
            gpui::uniform_list(
                super::child_id(&self.id, "list"),
                count,
                move |range, _window, cx| {
                    let p = palette(cx);
                    range
                        .map(|position| table_row(position, &row_parts, p))
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&scroll)
            .flex_1()
            .min_h_0()
            .into_any_element()
        } else {
            let mut body = div()
                .id(super::child_id(&self.id, "body"))
                .flex()
                .flex_col()
                .track_scroll(&rows_handle);
            for position in 0..count {
                body = body.child(table_row(position, &parts, p));
            }
            body.into_any_element()
        };

        let key_parts = parts.clone();
        let key_scroll = scroll.clone();
        let key_rows = rows_handle.clone();
        let fill = self.fill;
        let rows = div()
            .id(super::child_id(&self.id, "rows"))
            .flex()
            .flex_col()
            .min_h_0()
            .when(self.fill, |rows| rows.flex_1())
            .child(body)
            .track_focus(&focus.tab_index(0).tab_stop(count > 0))
            .on_key_down(move |event, window, cx| {
                let Some(key) = Key::from_keystroke(&event.keystroke) else {
                    return;
                };
                let parts = &key_parts;
                let position = parts
                    .selected
                    .and_then(|selected| parts.order.iter().position(|row| *row == selected));
                let menu_key = event.keystroke.key == "menu"
                    || (event.keystroke.key == "f10" && event.keystroke.modifiers.shift);
                if menu_key {
                    if let (Some(position), Some(menu)) = (position, parts.on_context_menu.as_ref())
                    {
                        cx.stop_propagation();
                        // Beside the selected row, as a right click on it
                        // would put the menu.
                        let at = if fill {
                            let state = key_scroll.0.borrow();
                            menu_anchor(
                                state.base_handle.bounds(),
                                state.base_handle.offset().y,
                                position,
                            )
                        } else {
                            match key_rows.bounds_for_item(position) {
                                Some(row) => gpui::point(row.origin.x + MENU_INSET, row.bottom()),
                                None => menu_anchor(key_rows.bounds(), px(0.0), position),
                            }
                        };
                        menu(parts.order[position], at, window, cx);
                    }
                } else if key == Key::Enter {
                    if let (Some(row), Some(invoke)) = (parts.selected, parts.on_invoke.as_ref()) {
                        cx.stop_propagation();
                        invoke(row, window, cx);
                    }
                } else if let Some(next) = table_step(position, parts.order.len(), key)
                    && let Some(select) = parts.on_select.as_ref()
                {
                    cx.stop_propagation();
                    key_scroll.scroll_to_item(next, gpui::ScrollStrategy::Center);
                    select(parts.order[next], window, cx);
                }
            });

        div()
            .id(self.id)
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .when(self.fill, |table| table.h_full().min_h_0())
            .child(header)
            .child(rows)
    }
}
