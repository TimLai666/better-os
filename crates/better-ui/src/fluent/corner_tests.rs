//! Ticket 141's rule driven through GPUI's own test window: a window with a
//! rounded shape (given as its frame would give it; the test platform has no
//! client decorations), real layout, real hit testing, real scrolling.

use std::sync::Arc;

use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _, Pixels,
    Render, ScrollDelta, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _,
    Styled as _, TestAppContext, TouchPhase, UniformListScrollHandle, VisualTestContext, Window,
    div, point, px, size, uniform_list,
};

use super::navigation::{NavItem, NavigationView};
use super::window_frame::{
    FrameLook, corner_clip, corner_depth, corner_scrollbar, cut, float_at, outline_visible,
    set_window_shape,
};
use crate::system_frame::service::Rendered;
use crate::system_frame::{
    CornerRadii, DesktopSettings, Rect, Tiled, WindowShape, WindowState, fallback_look,
};

const W: f32 = 480.0;
const H: f32 = 360.0;
const R: f32 = 15.0;

fn shape() -> WindowShape {
    WindowShape::new(Rect::new(0.0, 0.0, W, H), CornerRadii::uniform(R))
}

#[derive(Clone, Copy)]
enum Kind {
    Overlay,
    Nav,
    Diff,
    Scrollbar,
}

struct Harness {
    kind: Kind,
    scroll: ScrollHandle,
    list: UniformListScrollHandle,
    clicked: Vec<usize>,
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        set_window_shape(window, cx, shape());
        let this = cx.entity().downgrade();
        let record = move |index: usize| {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |h, cx| {
                    h.clicked.push(index);
                    cx.notify();
                });
            }
        };
        match self.kind {
            // A tooltip asked for at the pointer in the bottom right corner.
            Kind::Overlay => div()
                .size_full()
                .child(float_at(
                    point(px(W - 10.0), px(H - 5.0)),
                    div()
                        .id("tip")
                        .debug_selector(|| "tip".into())
                        .w(px(120.0))
                        .h(px(30.0))
                        .on_click(record(1)),
                ))
                .into_any_element(),
            // A pane with no footer, so its list reaches the bottom corner.
            Kind::Nav => {
                let this = cx.entity().downgrade();
                let entries = (0..40)
                    .map(|i| {
                        NavItem::new(format!("Item {i}"), crate::icon_set::Picture::Folder).entry()
                    })
                    .collect();
                NavigationView::new("nav", entries, None)
                    .on_select(move |index, _, cx| {
                        let _ = this.update(cx, |h, cx| {
                            h.clicked.push(index);
                            cx.notify();
                        });
                    })
                    .content(div())
                    .into_any_element()
            }
            // Git's diff: full-width rows, clipped at inset 0, with the
            // corner's depth as padding at the end (`git-gui` render.rs).
            Kind::Diff => {
                let record = record.clone();
                corner_clip(
                    px(0.0),
                    uniform_list("diff", 60, move |range, _, _| {
                        range
                            .map(|i| {
                                div()
                                    .id(i)
                                    .debug_selector(move || format!("row-{i}"))
                                    .h(px(20.0))
                                    .w_full()
                                    .on_click(record(i))
                                    .into_any_element()
                            })
                            .collect()
                    })
                    .track_scroll(&self.list)
                    .pb(corner_depth(px(0.0), window, cx))
                    .size_full(),
                )
                .into_any_element()
            }
            Kind::Scrollbar => div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("page")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(div().h(px(2000.0))),
                )
                .child(corner_scrollbar(&self.scroll, window, cx))
                .into_any_element(),
        }
    }
}

fn harness(cx: &mut TestAppContext, kind: Kind) -> (Entity<Harness>, &mut VisualTestContext) {
    cx.update(gpui_component::init);
    let (view, cx) = cx.add_window_view(move |_, _| Harness {
        kind,
        scroll: ScrollHandle::new(),
        list: UniformListScrollHandle::new(),
        clicked: Vec::new(),
    });
    cx.simulate_resize(size(px(W), px(H)));
    cx.run_until_parked();
    (view, cx)
}

fn scroll_down(cx: &mut VisualTestContext, at: gpui::Point<Pixels>) {
    for _ in 0..40 {
        cx.simulate_event(ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-400.0))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
    }
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestContext, at: gpui::Point<Pixels>) {
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_click(at, Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn an_overlay_at_the_bottom_corner_is_moved_inside_and_takes_its_presses(cx: &mut TestAppContext) {
    let (view, cx) = harness(cx, Kind::Overlay);
    let tip = cx.debug_bounds("tip").expect("the tooltip is drawn");
    let area = shape().overlay_area();
    assert!(
        f32::from(tip.left()) >= area.x && f32::from(tip.right()) <= area.right(),
        "{tip:?}"
    );
    assert!(
        f32::from(tip.top()) >= area.y && f32::from(tip.bottom()) <= area.bottom(),
        "{tip:?}"
    );
    // Its hitbox moved with it.
    click(cx, tip.center());
    assert_eq!(view.read_with(cx, |h, _| h.clicked.clone()), [1]);
}

#[gpui::test]
fn a_pane_list_scrolled_to_its_end_shows_its_last_row_whole_and_clickable(cx: &mut TestAppContext) {
    let (view, cx) = harness(cx, Kind::Nav);
    scroll_down(cx, point(px(100.0), px(H / 2.0)));
    let last = cx
        .debug_bounds("nav-item-39")
        .expect("the last row is drawn");
    // Rows are 4px in from the side: the curve reaches 5px up there.
    let depth = shape().corner_depth(4.0, 1.0);
    assert_eq!(depth, 5.0);
    assert!(f32::from(last.bottom()) <= H - depth, "{last:?}");
    // Its bottom pixel row takes a press.
    click(cx, point(last.center().x, last.bottom() - px(1.0)));
    assert_eq!(view.read_with(cx, |h, _| h.clicked.clone()), [39]);
}

#[gpui::test]
fn the_keyboard_brings_the_last_row_into_view_above_the_curve(cx: &mut TestAppContext) {
    let (_view, cx) = harness(cx, Kind::Nav);
    let first = cx
        .debug_bounds("nav-item-0")
        .expect("the first row is drawn");
    // A click selects and focuses the pane; Down walks to the end.
    click(cx, first.center());
    for _ in 0..39 {
        cx.simulate_keystrokes("down");
    }
    cx.run_until_parked();
    let last = cx
        .debug_bounds("nav-item-39")
        .expect("the last row is drawn");
    let depth = shape().corner_depth(4.0, 1.0);
    assert!(f32::from(last.top()) >= 0.0, "{last:?}");
    assert!(f32::from(last.bottom()) <= H - depth, "{last:?}");
}

#[gpui::test]
fn a_full_width_list_scrolled_to_its_end_shows_its_last_row_above_the_band(
    cx: &mut TestAppContext,
) {
    let (view, cx) = harness(cx, Kind::Diff);
    scroll_down(cx, point(px(W / 2.0), px(H / 2.0)));
    let last = cx.debug_bounds("row-59").expect("the last row is drawn");
    // Full-width rows give up the whole radius.
    assert_eq!(shape().corner_depth(0.0, 1.0), R);
    assert!(f32::from(last.bottom()) <= H - R, "{last:?}");
    click(cx, point(last.center().x, last.bottom() - px(1.0)));
    assert_eq!(view.read_with(cx, |h, _| h.clicked.clone()), [59]);
}

#[gpui::test]
fn a_corner_scrollbar_ends_above_the_curve(cx: &mut TestAppContext) {
    let (_view, cx) = harness(cx, Kind::Scrollbar);
    let bar = cx
        .debug_bounds("corner-scrollbar")
        .expect("the scrollbar is drawn");
    assert_eq!(f32::from(bar.bottom()), H - shape().corner_depth(4.0, 1.0));
    assert_eq!(f32::from(bar.right()), W);
}

#[test]
fn the_outline_shows_while_floating_and_never_maximized_or_tiled() {
    let floating = WindowState::new(Some(Tiled::default()), false, false, true);
    let mut rendered = Rendered::placeholder(floating.key(&DesktopSettings::default(), true, 1.0));
    rendered.has_outline = true;
    let images = Arc::new(cut(&rendered));
    let adwaita = FrameLook::Gtk(Arc::new(rendered.clone()), images);
    rendered.has_outline = false;
    let zorin = FrameLook::Gtk(Arc::new(rendered.clone()), Arc::new(cut(&rendered)));
    let fallback = FrameLook::Fallback(fallback_look(true));
    assert!(outline_visible(&adwaita, floating));
    assert!(outline_visible(&fallback, floating));
    // A theme with `outline: none` draws none.
    assert!(!outline_visible(&zorin, floating));
    let left = Tiled {
        left: true,
        ..Tiled::default()
    };
    for state in [
        WindowState::new(Some(Tiled::default()), true, false, true),
        WindowState::new(Some(Tiled::default()), false, true, true),
        WindowState::new(Some(left), false, false, true),
        WindowState::new(None, false, false, true),
    ] {
        assert!(!outline_visible(&adwaita, state), "{state:?}");
        assert!(!outline_visible(&fallback, state), "{state:?}");
    }
}
