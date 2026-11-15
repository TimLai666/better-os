//! The widths of Better Git's two list panes, through the shared edge policy
//! `better_ui::pane_edge::PaneEdge` (moved from Better Files in ticket 123).
//!
//! Changes and History share one width, because they are the same pane with
//! different lists in it; the Graph is wider by default because a commit row
//! carries lanes, labels, a subject and three columns, and has a width of its
//! own so dragging it does not move the file list. Both remember what the
//! person dragged them to, both clamp for the window they are drawn in, and
//! both go back to their default on a double-click.

use better_ui::pane_edge::PaneEdge;

use crate::graph_pane::GRAPH_SHARE;

/// Which list pane an edge belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneKind {
    /// Changes and History.
    List,
    /// The Graph tab's commit list.
    Graph,
}

/// Changes and History: 320px as the window has always drawn it, from 260px
/// (a file's name, its folder and its badge) to two fifths of the window.
pub const LIST_EDGE: PaneEdge = PaneEdge {
    default: 320.0,
    min: 260.0,
    max_fraction: 0.4,
};

/// The narrowest the Graph's list may get and still show a lane, a subject
/// and the sha column.
pub const GRAPH_MIN: f32 = 380.0;
/// The most of the window the Graph's list may take, so the commit's details
/// and its diff keep a usable width.
pub const GRAPH_MAX_FRACTION: f32 = 0.65;

/// The Graph's policy in a window this wide: [`GRAPH_SHARE`] of it by default,
/// which is what the tab has always drawn, and never below [`GRAPH_MIN`].
pub fn graph_edge(window_width: f32) -> PaneEdge {
    PaneEdge {
        default: (window_width * GRAPH_SHARE).round().max(GRAPH_MIN),
        min: GRAPH_MIN,
        max_fraction: GRAPH_MAX_FRACTION,
    }
}

/// The policy for a kind in a window this wide.
pub fn edge(kind: PaneKind, window_width: f32) -> PaneEdge {
    match kind {
        PaneKind::List => LIST_EDGE,
        PaneKind::Graph => graph_edge(window_width),
    }
}

/// The width to draw: the stored one when there is one, the default
/// otherwise, clamped for this window.
pub fn width(kind: PaneKind, stored: Option<u32>, window_width: f32) -> f32 {
    edge(kind, window_width).resolve(stored, window_width)
}

/// What a drag at `width` stores for this kind.
pub fn store(kind: PaneKind, width: f32, window_width: f32) -> Option<u32> {
    edge(kind, window_width).store(width, window_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_pane_is_320px_until_dragged() {
        assert_eq!(width(PaneKind::List, None, 1280.0), 320.0);
        assert_eq!(width(PaneKind::List, Some(380), 1280.0), 380.0);
        // Its minimum keeps a file's name and badge readable.
        assert_eq!(width(PaneKind::List, Some(10), 1280.0), 260.0);
        // It never takes more than two fifths of the window.
        assert_eq!(width(PaneKind::List, Some(1000), 1280.0), 512.0);
        // A window narrower than the share takes the minimum, and the default
        // is what comes back when it is wide again (nothing was stored).
        assert_eq!(width(PaneKind::List, None, 500.0), 260.0);
    }

    #[test]
    fn the_graph_keeps_its_share_until_dragged() {
        assert_eq!(width(PaneKind::Graph, None, 1280.0), 742.0);
        assert_eq!(width(PaneKind::Graph, None, 960.0), 557.0);
        assert_eq!(width(PaneKind::Graph, Some(600), 1280.0), 600.0);
        assert_eq!(width(PaneKind::Graph, Some(10), 1280.0), GRAPH_MIN);
        assert_eq!(width(PaneKind::Graph, Some(1200), 1280.0), 832.0);
        // A narrow window never draws it below its own minimum.
        assert_eq!(width(PaneKind::Graph, None, 600.0), GRAPH_MIN);
    }

    #[test]
    fn a_drag_is_stored_per_kind_and_a_double_click_stores_nothing() {
        assert_eq!(store(PaneKind::List, 380.6, 1280.0), Some(381));
        assert_eq!(store(PaneKind::List, 320.0, 1280.0), None);
        // The Graph's default is the share it is drawn at, so dragging back to
        // it stores nothing and the next window size gets its own share.
        assert_eq!(store(PaneKind::Graph, 742.0, 1280.0), None);
        assert_eq!(store(PaneKind::Graph, 600.0, 1280.0), Some(600));
        assert_eq!(LIST_EDGE.reset(), None);
        assert_eq!(graph_edge(1280.0).reset(), None);
    }
}
