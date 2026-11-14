//! Light dismiss: how a floating panel (a flyout, a combo box's list, a
//! menu, the suggestion list, the floating navigation pane) closes.
//!
//! WinUI closes a light-dismiss panel on any press outside it, and the press
//! that closes it does nothing else. So a second press on the control that
//! opened the panel closes it and does not open it again; the third press
//! opens it. GPUI's `on_mouse_down_out` closes the panel on the press, but
//! the control's click handler then runs on the release and opens the panel
//! again. The kit stops that with a shield: while the panel is open, an
//! empty element covers the control the panel hangs from and takes its
//! pointer input, so the closing press never becomes the control's click.
//!
//! The shield covers only the control, not the window, so a press on any
//! other control still closes the panel and reaches that control, as it did
//! before.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Anchor, AnyElement, App, Div, Global, Hitbox, HitboxBehavior, InteractiveElement as _,
    IntoElement, ParentElement as _, Styled as _, Window, WindowId, anchored, canvas, deferred,
    div, px, relative,
};

/// Where a pointer press lands while a panel is, or is not, open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PressAt {
    /// Inside the panel.
    Panel,
    /// On the control the panel hangs from.
    OwnControl,
    /// Inside a panel opened from this one (a split button's menu in a
    /// flyout, a submenu), even where it reaches past this panel's edge.
    ChildPanel,
    /// Anywhere else in the window.
    Elsewhere,
}

/// What one press does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Press {
    /// Whether the panel is open after the press.
    pub open: bool,
    /// Whether the press reaches what it landed on (its click handler runs).
    pub reaches_target: bool,
}

/// One press on a panel's window. A control's own click toggles its panel;
/// the shield keeps that click from running while the panel is open.
pub fn press(open: bool, at: PressAt) -> Press {
    match (open, at) {
        (true, PressAt::Panel | PressAt::ChildPanel) => Press {
            open: true,
            reaches_target: true,
        },
        // The press closes the panel; the shield takes it from the control.
        (true, PressAt::OwnControl) => Press {
            open: false,
            reaches_target: false,
        },
        (true, PressAt::Elsewhere) => Press {
            open: false,
            reaches_target: true,
        },
        (false, PressAt::OwnControl) => Press {
            open: true,
            reaches_target: true,
        },
        (false, PressAt::Panel | PressAt::ChildPanel | PressAt::Elsewhere) => Press {
            open: false,
            reaches_target: true,
        },
    }
}

/// The empty element that covers a control while its panel is open. It is
/// drawn after the control, inside the control's positioned parent, so it
/// is on top of it; `occlude` keeps the press from the control, and the
/// panel's own `on_mouse_down_out` closes the panel.
pub fn control_shield() -> Div {
    div().absolute().top_0().left_0().size_full().occlude()
}

/// Which edge of the control a floating panel lines up with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PanelEdge {
    #[default]
    Left,
    Right,
}

/// A panel floating 4px under the control it belongs to, kept inside the
/// window, with the control shielded while it is open. Draw it as the last
/// child of the control's `relative()` parent.
pub fn anchored_panel(panel: impl IntoElement, edge: PanelEdge) -> AnyElement {
    let corner = match edge {
        PanelEdge::Left => Anchor::TopLeft,
        PanelEdge::Right => Anchor::TopRight,
    };
    let floating = div().absolute().top(relative(1.0)).child(
        deferred(
            anchored()
                .anchor(corner)
                .snap_to_window_with_margin(px(8.0))
                .child(div().mt(px(4.0)).child(panel)),
        )
        .with_priority(1),
    );
    let floating = match edge {
        PanelEdge::Left => floating.left_0(),
        PanelEdge::Right => floating.right_0(),
    };
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(control_shield())
        .child(floating)
        .into_any_element()
}

/// The controls with a light-dismiss panel, and where the control that
/// opens each one sits, for the tests that walk every one of them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Opener {
    /// A button beside the panel, covered by the shield while it is open
    /// ([`anchored_panel`]): flyouts, combo boxes, menus, split and
    /// drop-down buttons, the command bar's More button, the breadcrumb's
    /// overflow.
    Shielded,
    /// The toggle is inside the panel itself: the floating navigation pane,
    /// whose ☰ button is the pane's first row.
    InsidePanel,
    /// A text field whose list is open while it has focus and text: a press
    /// in the field closes the list and the next press opens it again
    /// ([`super::auto_suggest`]).
    Field,
}

/// Three presses on a control: open, close, open again. Returns whether the
/// panel is open after each.
pub fn three_presses(opener: Opener) -> [bool; 3] {
    let mut open = false;
    let mut after = [false; 3];
    for slot in after.iter_mut() {
        open = match opener {
            Opener::Shielded | Opener::Field => press(open, PressAt::OwnControl).open,
            // A press on the toggle is a press in the panel: it reaches the
            // toggle, and the toggle flips the pane.
            Opener::InsidePanel => {
                let result = press(
                    open,
                    if open {
                        PressAt::Panel
                    } else {
                        PressAt::OwnControl
                    },
                );
                if open && result.reaches_target {
                    false
                } else {
                    result.open
                }
            }
        };
        *slot = open;
    }
    after
}

/// The panels drawn in the last frame, in the order they were drawn, so a
/// panel can tell a press in a panel opened from it (drawn after it, on top)
/// from a press outside. A panel's own `on_mouse_down_out` sees a press in
/// its child panel as outside: the child floats past its edge, and closing
/// the parent on that press took the child's item away before its click.
#[derive(Default)]
struct DrawnPanels {
    next: u64,
    panels: Vec<(WindowId, u64, Hitbox)>,
}

impl Global for DrawnPanels {}

/// How many drawn panels are remembered. A hitbox from an earlier frame is
/// never hovered again, so old entries only cost memory.
const REMEMBERED: usize = 64;

/// Where a panel was drawn in the order of this frame's panels, set when it
/// is drawn and read by its press handler.
#[derive(Clone, Default)]
pub struct PanelOrder(Rc<Cell<u64>>);

/// An empty element that records the panel it fills as drawn. Put it in
/// every light-dismiss panel, before the panel's other children.
pub fn panel_marker(order: &PanelOrder) -> impl IntoElement {
    let order = order.clone();
    canvas(
        move |bounds, window, cx| {
            let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            let drawn = cx.default_global::<DrawnPanels>();
            drawn.next += 1;
            order.0.set(drawn.next);
            drawn
                .panels
                .push((window.window_handle().window_id(), drawn.next, hitbox));
            let extra = drawn.panels.len().saturating_sub(REMEMBERED);
            drawn.panels.drain(..extra);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Whether the pointer is in a panel drawn after the one with `order`: a
/// panel opened from it, whose presses are not presses outside it.
pub fn pointer_in_child_panel(order: &PanelOrder, window: &Window, cx: &App) -> bool {
    let own = order.0.get();
    let here = window.window_handle().window_id();
    cx.try_global::<DrawnPanels>().is_some_and(|drawn| {
        drawn
            .panels
            .iter()
            .any(|(id, seq, hitbox)| *id == here && *seq > own && hitbox.is_hovered(window))
    })
}
