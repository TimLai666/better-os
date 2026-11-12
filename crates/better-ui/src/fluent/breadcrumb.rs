//! The breadcrumb bar (WinUI `BreadcrumbBar`).
//!
//! `BreadcrumbBar_themeresources.xaml`: transparent items in primary text,
//! secondary under the pointer and tertiary pressed; a 12px chevron with 2px
//! padding between items; the current (last) item is not a link. When the
//! path does not fit, the earliest items fold into a leading `…` that opens a
//! menu of them.
//!
//! Keyboard: the bar is one tab stop; Left and Right move between items,
//! Home and End to the ends, Enter or Space invokes.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{Icon, IconName};

use super::ValueHandler;
use super::menu::{MenuEntry, MenuFlyout, MenuItem, MenuOpen, open_menu};
use super::{
    BODY, Interaction, Key, Palette, TypeStyle, keyed_focus, palette, radius, shows_focus,
    step_index, text, with_focus_ring,
};

/// Which items to draw when at most `max_visible` fit: `(hidden, shown)`.
/// Hidden items are the earliest ones, behind the `…`; the current item is
/// always shown. `max_visible` counts the `…` as one.
pub fn fold(len: usize, max_visible: usize) -> (Vec<usize>, Vec<usize>) {
    if len <= max_visible.max(1) {
        return (Vec::new(), (0..len).collect());
    }
    let keep = max_visible.saturating_sub(1).max(1);
    let split = len - keep;
    ((0..split).collect(), (split..len).collect())
}

/// The path left after a person clicks item `index`: everything up to and
/// including it.
pub fn clicked<T: Clone>(items: &[T], index: usize) -> Vec<T> {
    items[..(index + 1).min(items.len())].to_vec()
}

/// A breadcrumb item's text color in one state.
pub fn item_color(state: Interaction, p: &Palette) -> gpui::Rgba {
    match state {
        Interaction::Rest => p.text_primary,
        Interaction::PointerOver => p.text_secondary,
        Interaction::Pressed => p.text_tertiary,
        Interaction::Disabled => p.text_disabled,
    }
}

/// The bar's keyboard position, kept across frames.
#[derive(Default)]
struct Cursor(Option<usize>);

/// `BreadcrumbBar`.
#[derive(IntoElement)]
pub struct BreadcrumbBar {
    id: ElementId,
    items: Vec<SharedString>,
    max_visible: usize,
    style: TypeStyle,
    on_click: Option<ValueHandler<usize>>,
}

impl BreadcrumbBar {
    pub fn new(id: impl Into<ElementId>, items: Vec<SharedString>) -> Self {
        Self {
            id: id.into(),
            items,
            max_visible: usize::MAX,
            style: BODY,
            on_click: None,
        }
    }

    /// How many items fit, counting the `…`.
    pub fn max_visible(mut self, max_visible: usize) -> Self {
        self.max_visible = max_visible;
        self
    }

    /// The type style: Body by default; a page header uses Subtitle or
    /// Title, as Settings does.
    pub fn style(mut self, style: TypeStyle) -> Self {
        self.style = style;
        self
    }

    /// Called with the index of the item clicked. The current item is not
    /// clickable.
    pub fn on_click(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for BreadcrumbBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = shows_focus(&focus, super::ShownState::Live, window);
        let cursor = window.use_keyed_state(super::child_id(&self.id, "cursor"), cx, |_, _| {
            Cursor::default()
        });
        let at = cursor.read(cx).0;
        let menu = MenuOpen::keyed(super::child_id(&self.id, "overflow"), window, cx);
        let menu_open = menu.read(cx).is_open();
        let len = self.items.len();
        let (hidden, shown) = fold(len, self.max_visible);
        // Keyboard positions: the `…` first when there is one, then the shown
        // items. The current item takes focus too, but does nothing.
        let positions: Vec<Option<usize>> = (!hidden.is_empty())
            .then_some(None)
            .into_iter()
            .chain(shown.iter().map(|&i| Some(i)))
            .collect();

        let count_last = positions.len().checked_sub(1);
        let mut bar = text(
            div().id(self.id.clone()).flex().items_center().flex_wrap(),
            self.style,
        );
        let chevron = || {
            div()
                .px(px(2.0))
                .text_color(p.text_secondary)
                .child(Icon::new(IconName::ChevronRight).size(px(12.0)))
        };
        for (position, target) in positions.iter().enumerate() {
            let is_current = *target == Some(len - 1);
            let label: SharedString = match target {
                Some(index) => self.items[*index].clone(),
                None => "…".into(),
            };
            let hover = item_color(Interaction::PointerOver, p);
            let pressed = item_color(Interaction::Pressed, p);
            let mut item = div()
                .id(ElementId::named_usize("crumb", position))
                .relative()
                .px(px(4.0))
                .rounded(radius::SM)
                .text_color(item_color(Interaction::Rest, p))
                .child(label);
            if !is_current {
                item = item
                    .hover(move |s| s.text_color(hover))
                    .active(move |s| s.text_color(pressed));
                match target {
                    Some(index) => {
                        if let Some(handler) = self.on_click.clone() {
                            let index = *index;
                            item = item.on_click(move |_, window, cx| handler(index, window, cx));
                        }
                    }
                    None => {
                        let menu = menu.clone();
                        item =
                            item.on_click(move |_, window, cx| open_menu(&menu, None, window, cx));
                    }
                }
            }
            let item = with_focus_ring(
                item,
                ring && at.or(count_last) == Some(position),
                radius::SM,
                p,
            );
            if position > 0 {
                bar = bar.child(chevron());
            }
            bar = bar.child(item);
        }

        let overflow = menu_open.then(|| {
            let entries = hidden
                .iter()
                .map(|&index| {
                    let handler = self.on_click.clone();
                    MenuItem::new(self.items[index].clone())
                        .on_select(move |window, cx| {
                            if let Some(handler) = handler.as_ref() {
                                handler(index, window, cx);
                            }
                        })
                        .entry()
                })
                .collect::<Vec<MenuEntry>>();
            MenuFlyout::new(super::child_id(&self.id, "flyout"), entries)
                .state(menu.clone())
                .return_focus(focus.clone())
        });

        let handler = self.on_click.clone();
        let count = positions.len();
        let key_cursor = cursor.clone();
        let key_menu = menu.clone();
        bar = bar
            .track_focus(&focus.tab_index(0).tab_stop(true))
            .on_key_down(move |event, window, cx| {
                let Some(key) = Key::from_keystroke(&event.keystroke) else {
                    return;
                };
                let now = key_cursor.read(cx).0.or(count.checked_sub(1));
                match key {
                    Key::Left | Key::Right | Key::Home | Key::End => {
                        if let Some(next) = step_index(now, count, key, false, |_| false) {
                            cx.stop_propagation();
                            key_cursor.update(cx, |c, cx| {
                                c.0 = Some(next);
                                cx.notify();
                            });
                        }
                    }
                    Key::Enter | Key::Space => {
                        let Some(position) = now else { return };
                        cx.stop_propagation();
                        match positions[position] {
                            None => open_menu(&key_menu, Some(0), window, cx),
                            Some(index) if index + 1 < len => {
                                if let Some(handler) = handler.as_ref() {
                                    handler(index, window, cx);
                                }
                            }
                            Some(_) => {}
                        }
                    }
                    _ => {}
                }
            });
        div().relative().child(bar).children(overflow)
    }
}
