//! The light-dismiss panels and menu rows driven through GPUI's own test
//! window: real presses at real positions, laid out and hit-tested by GPUI.
//! The model tests in `tests.rs` say what should happen; these say it does.

use gpui::{
    App, Bounds, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, VisualTestContext, Window, div, point, prelude::FluentBuilder as _, px,
};

use super::dialog::Flyout;
use super::menu::{MenuEntry, MenuFlyout, MenuItem, MenuOpen, open_menu};

/// A window with a control at (20, 20), 120 by 32, that opens a flyout; in
/// the flyout, a 40 by 32 control that opens either a nested flyout with a
/// 400px-wide item or a menu, both reaching far past the flyout's edge.
struct Harness {
    flyout: bool,
    nested: bool,
    menu: Entity<MenuOpen>,
    inner_menu: Entity<MenuOpen>,
    chosen: Vec<&'static str>,
}

impl Harness {
    fn new(cx: &mut Context<Self>) -> Self {
        Harness {
            flyout: false,
            nested: false,
            menu: super::menu::new_state(cx),
            inner_menu: super::menu::new_state(cx),
            chosen: Vec::new(),
        }
    }
}

fn record(
    view: &gpui::WeakEntity<Harness>,
    what: &'static str,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, cx| {
        let _ = view.update(cx, |h, cx| {
            h.chosen.push(what);
            cx.notify();
        });
    }
}

fn entries(view: &gpui::WeakEntity<Harness>, prefix: &'static [&'static str; 2]) -> Vec<MenuEntry> {
    vec![
        MenuItem::new(prefix[0])
            .on_select(record(view, prefix[0]))
            .entry(),
        MenuItem::new(prefix[1])
            .on_select(record(view, prefix[1]))
            .entry(),
    ]
}

impl Render for Harness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        let dismiss_view = view.clone();
        let nested_view = view.clone();
        let menu = self.menu.clone();
        let inner = self.inner_menu.clone();
        let flyout_open = self.flyout;
        let nested_open = self.nested;
        let menu_open = self.menu.read(cx).is_open();
        let inner_open = self.inner_menu.read(cx).is_open();
        let outer_content = div()
            .w(px(40.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .relative()
                    .w(px(40.0))
                    .h(px(32.0))
                    .child(
                        div()
                            .id("nested-opener")
                            .debug_selector(|| "nested-opener".into())
                            .size_full()
                            .on_click(cx.listener(|h, _, _, cx| {
                                h.nested = !h.nested;
                                cx.notify();
                            })),
                    )
                    .when(nested_open, |d| {
                        d.child(
                            Flyout::new("nested")
                                .on_dismiss(move |_, cx| {
                                    let _ = nested_view.update(cx, |h, cx| {
                                        h.nested = false;
                                        cx.notify();
                                    });
                                })
                                .child(
                                    div()
                                        .id("far-item")
                                        .debug_selector(|| "far-item".into())
                                        .w(px(400.0))
                                        .h(px(40.0))
                                        .on_click(cx.listener(|h, _, _, cx| {
                                            h.chosen.push("far");
                                            cx.notify();
                                        })),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .w(px(40.0))
                    .h(px(32.0))
                    .child(
                        div()
                            .id("inner-menu-opener")
                            .debug_selector(|| "inner-menu-opener".into())
                            .size_full()
                            .on_click(move |_, window, cx| open_menu(&inner, None, window, cx)),
                    )
                    .when(inner_open, |d| {
                        d.child(
                            MenuFlyout::new(
                                "inner-menu",
                                entries(&view, &["Clone", "New repository"]),
                            )
                            .state(self.inner_menu.clone()),
                        )
                    }),
            );
        div()
            .size_full()
            .child(
                div()
                    .absolute()
                    .left(px(20.0))
                    .top(px(20.0))
                    .w(px(120.0))
                    .h(px(32.0))
                    .child(
                        div()
                            .id("opener")
                            .debug_selector(|| "opener".into())
                            .size_full()
                            .on_click(cx.listener(|h, _, _, cx| {
                                h.flyout = !h.flyout;
                                cx.notify();
                            })),
                    )
                    .when(flyout_open, |d| {
                        d.child(
                            Flyout::new("outer")
                                .on_dismiss(move |_, cx| {
                                    let _ = dismiss_view.update(cx, |h, cx| {
                                        h.flyout = false;
                                        cx.notify();
                                    });
                                })
                                .child(outer_content),
                        )
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(400.0))
                    .top(px(20.0))
                    .w(px(120.0))
                    .h(px(32.0))
                    .child(
                        div()
                            .id("menu-opener")
                            .debug_selector(|| "menu-opener".into())
                            .size_full()
                            .on_click(move |_, window, cx| open_menu(&menu, None, window, cx)),
                    )
                    .when(menu_open, |d| {
                        d.child(
                            MenuFlyout::new("menu", entries(&view, &["Cut", "Copy"]))
                                .state(self.menu.clone()),
                        )
                    }),
            )
    }
}

fn harness(cx: &mut TestAppContext) -> (Entity<Harness>, &mut VisualTestContext) {
    cx.update(gpui_component::init);
    cx.add_window_view(|_, cx| Harness::new(cx))
}

fn centre(bounds: Bounds<Pixels>) -> Point<Pixels> {
    bounds.center()
}

fn press(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_click(at, Modifiers::default());
    cx.run_until_parked();
}

fn press_on(cx: &mut VisualTestContext, selector: &'static str) {
    let at = centre(bounds(cx, selector));
    press(cx, at);
}

fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
}

#[gpui::test]
fn a_flyout_opens_closes_and_opens_again_on_its_own_control(cx: &mut TestAppContext) {
    let (view, cx) = harness(cx);
    let opener = centre(bounds(cx, "opener"));
    let mut seen = Vec::new();
    for _ in 0..3 {
        press(cx, opener);
        seen.push(view.read_with(cx, |h, _| h.flyout));
    }
    assert_eq!(seen, [true, false, true]);
}

#[gpui::test]
fn a_menu_flyout_opens_closes_and_opens_again_on_its_own_control(cx: &mut TestAppContext) {
    let (view, cx) = harness(cx);
    let opener = centre(bounds(cx, "menu-opener"));
    let mut seen = Vec::new();
    for _ in 0..3 {
        press(cx, opener);
        seen.push(view.read_with(cx, |h, cx| h.menu.read(cx).is_open()));
    }
    assert_eq!(seen, [true, false, true]);
}

/// The pixels between two menu items belong to an item: a press on the last
/// line of the first item's hit area, below its drawn shape, invokes it.
#[gpui::test]
fn a_press_between_two_menu_items_invokes_one(cx: &mut TestAppContext) {
    let (view, cx) = harness(cx);
    press_on(cx, "menu-opener");
    let first = bounds(cx, "menu-item-0");
    let second = bounds(cx, "menu-item-1");
    assert_eq!(
        first.bottom(),
        second.top(),
        "the two items' hit areas meet: no line between them belongs to neither"
    );
    // The inset shape ends 2px above the hit area's bottom.
    press(cx, point(first.right() - px(2.0), first.bottom() - px(1.0)));
    view.read_with(cx, |h, cx| {
        assert_eq!(h.chosen, ["Cut"]);
        assert!(!h.menu.read(cx).is_open());
    });
}

/// The owner's "New repository does nothing": a menu opened from a flyout
/// reaches past the flyout's edge, and a press there used to close the
/// flyout (and the menu with it) before the item's click arrived.
#[gpui::test]
fn a_press_in_a_menu_that_reaches_past_its_flyout_reaches_the_item(cx: &mut TestAppContext) {
    let (view, cx) = harness(cx);
    press_on(cx, "opener");
    press_on(cx, "inner-menu-opener");
    let item = bounds(cx, "menu-item-1");
    let flyout_right = bounds(cx, "inner-menu-opener").right() + px(16.0);
    let at = point(item.right() - px(4.0), item.center().y);
    assert!(at.x > flyout_right, "the press is outside the flyout");
    press(cx, at);
    view.read_with(cx, |h, _| assert_eq!(h.chosen, ["New repository"]));
}

#[gpui::test]
fn a_press_in_a_nested_flyout_past_the_outer_edge_reaches_its_content(cx: &mut TestAppContext) {
    let (view, cx) = harness(cx);
    press_on(cx, "opener");
    press_on(cx, "nested-opener");
    let far = bounds(cx, "far-item");
    press(cx, point(far.right() - px(4.0), far.center().y));
    view.read_with(cx, |h, _| {
        assert_eq!(h.chosen, ["far"]);
        assert!(h.flyout, "the outer flyout stays open");
    });
    // A press outside both closes both.
    press(cx, point(px(600.0), px(500.0)));
    view.read_with(cx, |h, _| assert!(!h.flyout && !h.nested));
}

/// A toolbar cell 52px tall, as Better Git's sync cell is: a primary part
/// and, after a divider, a [`MenuSegment`] that fills the cell's height,
/// with the cell drawing the menu lined up on its right edge. Below it, a
/// [`SplitButton`].
struct SplitHarness {
    menu: Entity<MenuOpen>,
    primary: usize,
}

fn plain_entries() -> Vec<MenuEntry> {
    vec![MenuItem::new("A").entry(), MenuItem::new("B").entry()]
}

impl Render for SplitHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        let open = self.menu.read(cx).is_open();
        div()
            .size_full()
            .child(
                div()
                    .id("cell")
                    .absolute()
                    .left(px(20.0))
                    .top(px(20.0))
                    .h(px(52.0))
                    .flex()
                    .child(
                        div()
                            .id("cell-primary")
                            .w(px(160.0))
                            .h_full()
                            .on_click(cx.listener(|h, _, _, cx| {
                                h.primary += 1;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .debug_selector(|| "segment".into())
                            .h_full()
                            .border_l_1()
                            .child(
                                super::button::MenuSegment::new("segment", self.menu.clone())
                                    .tooltip("More"),
                            ),
                    )
                    .when(open, |d| {
                        d.child(
                            MenuFlyout::new("segment-menu", plain_entries())
                                .align_right()
                                .state(self.menu.clone()),
                        )
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(20.0))
                    .top(px(160.0))
                    .debug_selector(|| "split".into())
                    .child(
                        super::button::SplitButton::new("split", "Save")
                            .entries(plain_entries())
                            .on_click(move |_, cx| {
                                let _ = view.update(cx, |h, cx| {
                                    h.primary += 1;
                                    cx.notify();
                                });
                            }),
                    ),
            )
    }
}

fn split_harness(cx: &mut TestAppContext) -> (Entity<SplitHarness>, &mut VisualTestContext) {
    cx.update(gpui_component::init);
    cx.add_window_view(|_, cx| SplitHarness {
        menu: super::menu::new_state(cx),
        primary: 0,
    })
}

/// The owner's "only the arrow glyph opens the menu" (ticket 130): a press
/// in a corner of the arrow segment, far from its glyph, opens the menu, a
/// second press there closes it, and the primary part is never invoked.
#[gpui::test]
fn a_press_in_the_corner_of_a_toolbar_arrow_segment_opens_its_menu(cx: &mut TestAppContext) {
    let (view, cx) = split_harness(cx);
    let segment = bounds(cx, "segment");
    assert_eq!(
        segment.size.height,
        px(52.0),
        "the segment fills the cell's height"
    );
    assert!(segment.size.width >= px(36.0));
    let corners = [
        point(segment.left() + px(2.0), segment.top() + px(1.0)),
        point(segment.right() - px(1.0), segment.bottom() - px(1.0)),
    ];
    for corner in corners {
        press(cx, corner);
        view.read_with(cx, |h, cx| {
            assert!(
                h.menu.read(cx).is_open(),
                "a press at {corner:?} opens the menu"
            );
            assert_eq!(h.primary, 0);
        });
        assert!(
            cx.debug_bounds("menu-item-0").is_some(),
            "its items are drawn"
        );
        press(cx, corner);
        view.read_with(cx, |h, cx| {
            assert!(!h.menu.read(cx).is_open(), "a second press closes it")
        });
    }
}

/// The same rule for the kit's split button: the corner of its chevron
/// part, not only the chevron, opens the menu.
#[gpui::test]
fn a_press_in_the_corner_of_a_split_buttons_chevron_opens_its_menu(cx: &mut TestAppContext) {
    let (view, cx) = split_harness(cx);
    let split = bounds(cx, "split");
    assert!(cx.debug_bounds("menu-item-0").is_none());
    press(cx, point(split.right() - px(2.0), split.bottom() - px(2.0)));
    assert!(
        cx.debug_bounds("menu-item-0").is_some(),
        "a press in the chevron part's corner opens the menu"
    );
    view.read_with(cx, |h, _| assert_eq!(h.primary, 0));
}

/// A details table whose first row is a group that opens to two children,
/// as Better Monitor's Processes page draws an application (ticket 145).
struct TableHarness {
    open: bool,
    selected: Option<usize>,
    invoked: Vec<usize>,
    expanded: Vec<(usize, bool)>,
}

impl Render for TableHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use super::list::{Column, DetailsTable, TableCell};
        let mut rows = vec![vec![TableCell::new("Files (2)").expander(self.open)]];
        if self.open {
            rows.push(vec![TableCell::new("files").indent(2)]);
            rows.push(vec![TableCell::new("files-helper").indent(2)]);
        }
        rows.push(vec![TableCell::new("Terminal").indent(1)]);
        let view = cx.entity().downgrade();
        let (select, invoke, expand) = (view.clone(), view.clone(), view);
        div().size_full().p(px(20.0)).child(
            DetailsTable::new("tree", vec![Column::new("Name")], rows)
                .presorted()
                .selected(self.selected)
                .on_select(move |index, _, cx| {
                    let _ = select.update(cx, |h, cx| {
                        h.selected = Some(index);
                        cx.notify();
                    });
                })
                .on_invoke(move |index, _, cx| {
                    let _ = invoke.update(cx, |h, cx| {
                        h.invoked.push(index);
                        cx.notify();
                    });
                })
                .on_expand(move |index, open, _, cx| {
                    let _ = expand.update(cx, |h, cx| {
                        h.expanded.push((index, open));
                        if index == 0 {
                            h.open = open;
                        }
                        cx.notify();
                    });
                }),
        )
    }
}

fn table_harness(cx: &mut TestAppContext) -> (Entity<TableHarness>, &mut VisualTestContext) {
    cx.update(gpui_component::init);
    cx.add_window_view(|_, _| TableHarness {
        open: false,
        selected: None,
        invoked: Vec::new(),
        expanded: Vec::new(),
    })
}

fn double_press(cx: &mut VisualTestContext, at: Point<Pixels>) {
    use gpui::{MouseButton, MouseDownEvent, MouseUpEvent};
    cx.simulate_mouse_move(at, None, Modifiers::default());
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
        });
    }
    cx.run_until_parked();
}

/// The owner's "its disclosure arrow does nothing when clicked": one press
/// in the corner of the arrow's hit area, away from the glyph, opens the
/// row, and another closes it, without selecting or invoking it.
#[gpui::test]
fn one_press_on_a_tree_rows_arrow_opens_and_closes_it(cx: &mut TestAppContext) {
    let (view, cx) = table_harness(cx);
    let arrow = bounds(cx, "table-expander-0");
    let row = bounds(cx, "table-row-0");
    assert_eq!(arrow.size.height, row.size.height, "the full row height");
    assert!(arrow.size.width >= px(24.0), "wider than the 12px glyph");
    let corner = point(arrow.left() + px(1.0), arrow.top() + px(1.0));
    press(cx, corner);
    view.read_with(cx, |h, _| {
        assert!(h.open);
        assert_eq!(h.expanded, [(0, true)]);
        assert_eq!(h.selected, None, "the arrow does not select the row");
        assert!(h.invoked.is_empty());
    });
    assert!(
        cx.debug_bounds("table-row-1").is_some(),
        "its children are drawn"
    );
    let corner = point(arrow.right() - px(1.0), arrow.bottom() - px(1.0));
    press(cx, corner);
    view.read_with(cx, |h, _| {
        assert!(!h.open);
        assert_eq!(h.expanded, [(0, true), (0, false)]);
        assert!(h.invoked.is_empty());
    });
}

/// A double click on the row, away from its arrow, still invokes it, and
/// the table leaves the opening to the window, as before.
#[gpui::test]
fn a_double_click_on_a_tree_row_still_invokes_it(cx: &mut TestAppContext) {
    let (view, cx) = table_harness(cx);
    let row = bounds(cx, "table-row-0");
    double_press(cx, point(row.right() - px(20.0), row.center().y));
    view.read_with(cx, |h, _| {
        assert_eq!(h.invoked, [0]);
        assert_eq!(h.selected, Some(0));
        assert!(h.expanded.is_empty(), "the arrow was not pressed");
    });
}

/// Right opens the selected row and Left closes it; on a row with no
/// children neither does anything.
#[gpui::test]
fn right_and_left_open_and_close_the_selected_tree_row(cx: &mut TestAppContext) {
    let (view, cx) = table_harness(cx);
    let row = bounds(cx, "table-row-0");
    press(cx, point(row.right() - px(20.0), row.center().y));
    view.read_with(cx, |h, _| assert_eq!(h.selected, Some(0)));
    cx.simulate_keystrokes("right");
    view.read_with(cx, |h, _| assert!(h.open));
    cx.simulate_keystrokes("right");
    view.read_with(cx, |h, _| {
        assert_eq!(h.expanded, [(0, true)], "already open")
    });
    cx.simulate_keystrokes("left");
    view.read_with(cx, |h, _| assert!(!h.open));
    cx.simulate_keystrokes("down");
    view.read_with(cx, |h, _| assert_eq!(h.selected, Some(1)));
    cx.simulate_keystrokes("right left");
    view.read_with(cx, |h, _| {
        assert_eq!(
            h.expanded,
            [(0, true), (0, false)],
            "a leaf has nothing to open"
        )
    });
}
