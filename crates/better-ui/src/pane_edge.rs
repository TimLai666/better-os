//! A pane whose edge a person can drag: the width it starts at, how narrow it
//! may get, and how much of the window it may take, so every such edge
//! clamps, remembers and resets alike.
//!
//! Better Files' policy from its `layout.rs` (tickets 118 and 119), moved
//! here in ticket 123: Better Files and Better Git both take their edges from
//! this module, so there is one rule.

/// One draggable pane edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneEdge {
    /// The width it has until dragged, and after a double-click.
    pub default: f32,
    /// Narrower than this and its labels stop being readable.
    pub min: f32,
    /// The most of the window it may take, so the content beside it keeps a
    /// usable width.
    pub max_fraction: f32,
}

impl PaneEdge {
    /// The widest the pane may be in a window this wide; never below `min`.
    pub fn max(&self, window_width: f32) -> f32 {
        (window_width * self.max_fraction).max(self.min)
    }

    /// `width`, kept between the edge's limits for this window. Anything that
    /// is not a width (not a number, below zero) is the default.
    pub fn clamp(&self, width: f32, window_width: f32) -> f32 {
        let width = if width.is_finite() && width > 0.0 {
            width
        } else {
            self.default
        };
        width.clamp(self.min, self.max(window_width))
    }

    /// The width to draw from what was stored: the stored width when there is
    /// one, the default otherwise, either clamped for this window.
    pub fn resolve(&self, stored: Option<u32>, window_width: f32) -> f32 {
        self.clamp(
            stored.map_or(self.default, |width| width as f32),
            window_width,
        )
    }

    /// What a double-click on the edge stores: no width at all, so the pane is
    /// drawn at its default whenever the window is wide enough, not at
    /// whatever a narrow window clamped the default to.
    pub fn reset(&self) -> Option<u32> {
        None
    }

    /// What to store after a drag: the clamped width in whole pixels, or
    /// nothing when it is the default.
    pub fn store(&self, width: f32, window_width: f32) -> Option<u32> {
        let width = self.clamp(width, window_width).round();
        (width != self.default).then_some(width as u32)
    }
}

/// How wide the strip a person grabs to drag a pane's edge is: wider than the
/// 1px divider drawn in it, as in Windows 11 File Explorer.
pub const PANE_EDGE_HIT_WIDTH: f32 = 8.0;

#[cfg(test)]
mod tests {
    use super::*;

    const EDGE: PaneEdge = PaneEdge {
        default: 320.0,
        min: 240.0,
        max_fraction: 0.4,
    };

    #[test]
    fn a_stored_width_is_kept_between_the_limits() {
        assert_eq!(EDGE.resolve(None, 1280.0), 320.0);
        assert_eq!(EDGE.resolve(Some(400), 1280.0), 400.0);
        // Below the minimum is the minimum; not a width at all is the default.
        assert_eq!(EDGE.resolve(Some(100), 1280.0), 240.0);
        assert_eq!(EDGE.clamp(f32::NAN, 1280.0), 320.0);
        // A window too narrow for the default takes the minimum, not less.
        assert_eq!(EDGE.resolve(Some(320), 500.0), 240.0);
        // And the pane never takes more than its share.
        assert_eq!(EDGE.resolve(Some(900), 1000.0), 400.0);
        assert_eq!(EDGE.max(1000.0), 400.0);
        // The share can never squeeze the pane below its own minimum.
        assert_eq!(EDGE.max(100.0), 240.0);
    }

    #[test]
    fn a_drag_stores_whole_pixels_and_a_double_click_stores_nothing() {
        assert_eq!(EDGE.store(360.4, 1280.0), Some(360));
        assert_eq!(EDGE.store(100.0, 1280.0), Some(240));
        assert_eq!(EDGE.store(900.0, 1280.0), Some(512));
        // Back at the default is stored as nothing, so a later wide window
        // shows the default and not the width this one clamped to.
        assert_eq!(EDGE.store(320.0, 1280.0), None);
        assert_eq!(EDGE.reset(), None);
    }
}
