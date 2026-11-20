//! The window's rounded shape, and how content is kept inside it (ticket
//! 141).
//!
//! GTK 4 and libadwaita windows on GNOME round all four corners with the
//! theme's `border-radius` and clip their content to that shape; a maximized,
//! fullscreen, or tiled window (on any edge: `window.csd.tiled-*` sets
//! `border-radius: 0` in libadwaita 1.5 and in the Zorin themes) has square
//! corners.
//!
//! GPUI clips content to rectangles only: `ContentMask` is a rectangle at the
//! locked revision (`ae394f3`, `crates/gpui/src/window.rs`) and on Zed's `main`
//! of 2026-11-20 (`0a514d7`), and the wgpu shaders discard by the distance to
//! that rectangle (`clip_to_mask`). So the kit keeps content inside the curve
//! by one rule, which costs no width and hides nothing for good:
//!
//! - Every surface that reaches a corner paints its own fill into it, with the
//!   radius that keeps it inside ([`WindowShape::fill_radii`]): the surface's
//!   colour runs to the curve and there is never a band of another colour.
//! - Nothing is inset along a side: content keeps the window's full width.
//! - What scrolls past a bottom corner is painted inside a rectangle whose
//!   bottom is raised by the curve's depth at the content's own side inset
//!   ([`WindowShape::clip`], [`curve_depth`]): rows `inset` in from the side
//!   cross the curve only within that depth. Content padded at least the
//!   radius from the side gives up nothing. The same depth is bottom padding
//!   inside the scroll extent ([`WindowShape::corner_depth`]), so the last
//!   row scrolls fully above the curve and stays clickable, and a scrollbar's
//!   track ends above it.
//!
//! Everything here is plain geometry in logical pixels, tested without GPUI.

use super::{Edge, Insets, RESIZE_HANDLE, WindowState};

/// The radius of each corner, logical pixels; 0 is square.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub const SQUARE: CornerRadii = CornerRadii {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: 0.0,
        bottom_left: 0.0,
    };

    /// Both top corners `top`, both bottom corners `bottom`.
    pub fn new(top: f32, bottom: f32) -> CornerRadii {
        CornerRadii {
            top_left: top,
            top_right: top,
            bottom_right: bottom,
            bottom_left: bottom,
        }
    }

    pub fn uniform(radius: f32) -> CornerRadii {
        CornerRadii::new(radius, radius)
    }

    pub fn is_square(&self) -> bool {
        self.all().iter().all(|r| *r <= 0.0)
    }

    /// In [`Corner::ALL`] order.
    pub fn all(&self) -> [f32; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }

    pub fn get(&self, corner: Corner) -> f32 {
        match corner {
            Corner::TopLeft => self.top_left,
            Corner::TopRight => self.top_right,
            Corner::BottomRight => self.bottom_right,
            Corner::BottomLeft => self.bottom_left,
        }
    }

    fn set(&mut self, corner: Corner, radius: f32) {
        match corner {
            Corner::TopLeft => self.top_left = radius,
            Corner::TopRight => self.top_right = radius,
            Corner::BottomRight => self.bottom_right = radius,
            Corner::BottomLeft => self.bottom_left = radius,
        }
    }

    /// Each rounded corner grown by `by`, as the outline of a rounded
    /// rectangle `by` larger is; a square corner stays square.
    pub fn grow(&self, by: f32) -> CornerRadii {
        let grow = |r: f32| if r > 0.0 { r + by } else { 0.0 };
        CornerRadii {
            top_left: grow(self.top_left),
            top_right: grow(self.top_right),
            bottom_right: grow(self.bottom_right),
            bottom_left: grow(self.bottom_left),
        }
    }
}

/// A window corner.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomRight,
        Corner::BottomLeft,
    ];

    fn is_left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    fn is_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }

    /// The resize edge a press at this corner starts.
    pub fn edge(self) -> Edge {
        match self {
            Corner::TopLeft => Edge::TopLeft,
            Corner::TopRight => Edge::TopRight,
            Corner::BottomRight => Edge::BottomRight,
            Corner::BottomLeft => Edge::BottomLeft,
        }
    }
}

/// The corner radii a window has now: the theme's, measured from GTK's
/// drawing (or the fallback's), on every corner of a window that draws its
/// own frame and is neither maximized, fullscreen, nor tiled on any edge;
/// square otherwise, as libadwaita's `window.csd.tiled*`, `.maximized`, and
/// `.fullscreen` rules make a GTK window's.
pub fn window_corner_radii(state: WindowState, top: f32, bottom: f32) -> CornerRadii {
    if !state.client_decorated || state.maximized || state.tiled.any() {
        CornerRadii::SQUARE
    } else {
        CornerRadii::new(top.max(0.0), bottom.max(0.0))
    }
}

/// A rectangle, logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
    }

    /// The window inside a surface `width` by `height` whose frame reaches
    /// `insets` past it.
    pub fn inside(width: f32, height: f32, insets: Insets) -> Rect {
        Rect::new(
            insets.left,
            insets.top,
            (width - insets.left - insets.right).max(0.0),
            (height - insets.top - insets.bottom).max(0.0),
        )
    }

    /// As `(x, y, width, height)`.
    pub fn tuple(&self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.width, self.height)
    }
}

/// The window's visible shape: its rectangle in the surface and its corners.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowShape {
    pub rect: Rect,
    pub radii: CornerRadii,
}

/// How far a point is from the two window edges that meet at `corner`,
/// positive inside the window.
fn offsets(rect: &Rect, corner: Corner, x: f32, y: f32) -> (f32, f32) {
    let dx = if corner.is_left() {
        x - rect.x
    } else {
        rect.right() - x
    };
    let dy = if corner.is_top() {
        y - rect.y
    } else {
        rect.bottom() - y
    };
    (dx, dy)
}

/// The corner of `element` nearest the window corner `corner`.
fn element_corner(element: &Rect, corner: Corner) -> (f32, f32) {
    let x = if corner.is_left() {
        element.x
    } else {
        element.right()
    };
    let y = if corner.is_top() {
        element.y
    } else {
        element.bottom()
    };
    (x, y)
}

/// How far in from the window's top or bottom edge the curve of radius `r`
/// is, `along` in from its side; 0 past the curve.
pub fn curve_depth(r: f32, along: f32) -> f32 {
    if r <= 0.0 || along >= r {
        return 0.0;
    }
    let along = along.max(0.0);
    r - (r * r - (r - along) * (r - along)).max(0.0).sqrt()
}

/// The smallest radius a rectangle's corner needs, `ox` and `oy` in from the
/// two edges of a window corner of radius `r`, to lie inside that corner:
/// its own arc inside the window's. 0 when its square corner already does.
pub fn needed_radius(r: f32, ox: f32, oy: f32) -> f32 {
    if r <= 0.0 {
        return 0.0;
    }
    let (ox, oy) = (ox.max(0.0), oy.max(0.0));
    if ox >= r || oy >= r {
        return 0.0;
    }
    // The arc of radius ρ centred (ox + ρ, oy + ρ) lies inside the window's
    // arc of radius r centred (r, r) when the centres are at most r - ρ
    // apart; the smallest such ρ is a root of that equality.
    (r - ox - oy - (2.0 * ox * oy).sqrt()).max(0.0)
}

impl WindowShape {
    pub fn new(rect: Rect, radii: CornerRadii) -> WindowShape {
        WindowShape { rect, radii }
    }

    /// Whether the point is inside the window's rounded shape.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let r = &self.rect;
        if x < r.x || x > r.right() || y < r.y || y > r.bottom() {
            return false;
        }
        Corner::ALL.iter().all(|corner| {
            let radius = self.radii.get(*corner);
            let (dx, dy) = offsets(r, *corner, x, y);
            if radius <= 0.0 || dx >= radius || dy >= radius {
                return true;
            }
            let (cx, cy) = (radius - dx, radius - dy);
            cx * cx + cy * cy <= radius * radius + 1e-3
        })
    }

    /// The corner radii a fill covering `element` needs to stay inside the
    /// window: `base` where that is enough, more where the element reaches a
    /// rounded window corner. A fill flush with the window's corner takes the
    /// window's radius.
    pub fn fill_radii(&self, element: Rect, base: CornerRadii) -> CornerRadii {
        let mut radii = base;
        let limit = (element.width.min(element.height) / 2.0).max(0.0);
        for corner in Corner::ALL {
            let window = self.radii.get(corner);
            if window <= 0.0 {
                continue;
            }
            let (x, y) = element_corner(&element, corner);
            let (ox, oy) = offsets(&self.rect, corner, x, y);
            let needed = needed_radius(window, ox, oy);
            radii.set(corner, base.get(corner).max(needed).min(limit));
        }
        radii
    }

    /// How far from the window's bottom edge (and top edge, for the top
    /// corners) content `inset` in from a side can cross a rounded corner:
    /// the deeper of the two bottom corners' [`curve_depth`], rounded up to a
    /// whole device pixel at `scale`. 0 when the corners are square or the
    /// inset is at least the radius. A scrolling surface that reaches a bottom
    /// corner takes this as bottom padding inside its scroll extent.
    pub fn corner_depth(&self, inset: f32, scale: f32) -> f32 {
        let depth = curve_depth(self.radii.bottom_left, inset)
            .max(curve_depth(self.radii.bottom_right, inset));
        snap_up(depth, scale)
    }

    /// The part of `element` its contents are painted in so that content
    /// `inset` or more in from the element's sides never crosses the window's
    /// rounded corners: `element` inside the window, with its bottom (or top)
    /// raised by the curve's depth at that inset wherever it reaches a rounded
    /// corner. Its sides never move. Edges move only inward, to whole device
    /// pixels at `scale`.
    pub fn clip(&self, element: Rect, inset: f32, scale: f32) -> Rect {
        let mut v = element.intersect(&self.rect);
        let inset = inset.max(0.0);
        for corner in Corner::ALL {
            let r = self.radii.get(corner);
            if r <= 0.0 || v.is_empty() {
                continue;
            }
            // How far the content's nearest column is from the window's side
            // at this corner.
            let (x, _) = element_corner(&v, corner);
            let (ox, _) = offsets(&self.rect, corner, x, v.y);
            let depth = snap_up(curve_depth(r, ox.max(0.0) + inset), scale);
            if depth <= 0.0 {
                continue;
            }
            if corner.is_top() {
                let top = v.y.max(self.rect.y + depth);
                v = Rect::new(v.x, top, v.width, (v.bottom() - top).max(0.0));
            } else {
                let bottom = v.bottom().min(self.rect.bottom() - depth);
                v = Rect::new(v.x, v.y, v.width, (bottom - v.y).max(0.0));
            }
        }
        v
    }

    /// Where a floating box (a tooltip, a menu, a panel) may lie so that
    /// none of it crosses the window's edge or a rounded corner: the window's
    /// rectangle less, on every side, the distance at which a square corner
    /// meets the curve (`r (1 - 1/√2)`, 4.4px for 15px), and never less than
    /// 4px.
    pub fn overlay_area(&self) -> Rect {
        let m = self
            .radii
            .all()
            .iter()
            .map(|r| (r * (1.0 - std::f32::consts::FRAC_1_SQRT_2)).ceil())
            .fold(4.0f32, f32::max);
        let r = &self.rect;
        Rect::new(
            r.x + m,
            r.y + m,
            (r.width - 2.0 * m).max(0.0),
            (r.height - 2.0 * m).max(0.0),
        )
    }

    /// How far to move a floating box so it lies in [`Self::overlay_area`]:
    /// as little as possible, and where it is larger than the area its
    /// leading and top edges win.
    pub fn shift_inside(&self, b: Rect) -> (f32, f32) {
        let area = self.overlay_area();
        let along = |start: f32, len: f32, lo: f32, hi: f32| {
            let mut d = 0.0;
            if start + len > hi {
                d = hi - (start + len);
            }
            if start + d < lo {
                d = lo - start;
            }
            d
        };
        (
            along(b.x, b.width, area.x, area.right()),
            along(b.y, b.height, area.y, area.bottom()),
        )
    }

    /// Where a submenu `width` wide goes beside its parent row, which spans
    /// `row_left` to `row_right`: after the row when it fits in
    /// [`Self::overlay_area`], else before it (on the parent's other side,
    /// as a menu at the window's trailing edge opens its submenus), else
    /// wherever it overlaps the parent least while inside the area.
    pub fn submenu_x(&self, row_left: f32, row_right: f32, width: f32) -> f32 {
        let area = self.overlay_area();
        if row_right + width <= area.right() {
            row_right
        } else if row_left - width >= area.x {
            row_left - width
        } else {
            (area.right() - width).max(area.x)
        }
    }

    /// The parts of the window's rectangle outside its rounded corners, one
    /// logical pixel row at a time: `(corner, row)`. A row holds every pixel
    /// whose centre is outside the curve.
    pub fn outside_rows(&self) -> Vec<(Corner, Rect)> {
        let mut rows = Vec::new();
        let r = &self.rect;
        for corner in Corner::ALL {
            let radius = self.radii.get(corner);
            if radius <= 0.0 {
                continue;
            }
            let count = radius.ceil() as u32;
            for i in 0..count {
                let depth = i as f32 + 0.5;
                let width = curve_depth(radius, depth);
                // Pixels whose centre is outside: the first `n` columns.
                let columns = (width + 0.5 - 1e-3).floor().max(0.0);
                if columns <= 0.0 {
                    continue;
                }
                let x = if corner.is_left() {
                    r.x
                } else {
                    r.right() - columns
                };
                let y = if corner.is_top() {
                    r.y + i as f32
                } else {
                    r.bottom() - i as f32 - 1.0
                };
                rows.push((corner, Rect::new(x, y, columns, 1.0)));
            }
        }
        rows
    }

    /// The corner a point inside the window's rectangle but outside its
    /// rounded shape belongs to.
    pub fn corner_outside(&self, x: f32, y: f32) -> Option<Corner> {
        let r = &self.rect;
        if x < r.x || x > r.right() || y < r.y || y > r.bottom() || self.contains(x, y) {
            return None;
        }
        Corner::ALL.into_iter().find(|corner| {
            let radius = self.radii.get(*corner);
            let (dx, dy) = offsets(r, *corner, x, y);
            radius > 0.0 && dx < radius && dy < radius
        })
    }
}

/// `value` rounded up to a whole device pixel at `scale`.
fn snap_up(value: f32, scale: f32) -> f32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    if value <= 0.0 {
        0.0
    } else {
        (value * scale - 1e-3).ceil() / scale
    }
}

/// A rounded rectangle as whole-pixel horizontal spans: every pixel whose
/// centre is inside it, rows with the same span merged. What a Wayland input
/// region, which takes rectangles, can hold of a rounded shape.
pub fn rounded_spans(rect: Rect, radii: CornerRadii) -> Vec<Rect> {
    if rect.is_empty() {
        return Vec::new();
    }
    let shape = WindowShape::new(rect, radii);
    let top_band = radii.top_left.max(radii.top_right).ceil();
    let bottom_band = radii.bottom_left.max(radii.bottom_right).ceil();
    let rows_total = rect.height.round().max(0.0) as i64;
    let top_rows = (top_band as i64).min(rows_total);
    let bottom_rows = (bottom_band as i64).min(rows_total - top_rows);
    let span = |row: i64| -> Option<(f32, f32)> {
        let y = rect.y + row as f32 + 0.5;
        let start = (rect.x.floor() as i64..rect.right().ceil() as i64)
            .find(|x| shape.contains(*x as f32 + 0.5, y))?;
        let end = (start..rect.right().ceil() as i64)
            .rev()
            .find(|x| shape.contains(*x as f32 + 0.5, y))?;
        Some((start as f32, (end + 1) as f32))
    };
    let mut spans: Vec<Rect> = Vec::new();
    let mut push = |row_y: f32, height: f32, s: Option<(f32, f32)>| {
        let Some((x0, x1)) = s else { return };
        if let Some(last) = spans.last_mut()
            && last.x == x0
            && last.right() == x1
            && (last.bottom() - row_y).abs() < 1e-3
        {
            last.height += height;
            return;
        }
        spans.push(Rect::new(x0, row_y, x1 - x0, height));
    };
    for row in 0..top_rows {
        push(rect.y + row as f32, 1.0, span(row));
    }
    let middle = rows_total - top_rows - bottom_rows;
    if middle > 0 {
        push(
            rect.y + top_rows as f32,
            middle as f32,
            Some((rect.x, rect.right())),
        );
    }
    for row in (rows_total - bottom_rows)..rows_total {
        push(rect.y + row as f32, 1.0, span(row));
    }
    spans
}

/// Where the window takes input: the visible window and GTK 4's resize
/// handle around it ([`RESIZE_HANDLE`], never past the surface), with each
/// rounded corner following the window's curve grown by the handle, so a
/// press on the shadow beyond it reaches whatever is behind the window.
/// Rectangles `(x, y, width, height)` in the surface.
pub fn input_region(
    width: f32,
    height: f32,
    inset: Insets,
    radii: CornerRadii,
) -> Vec<(f32, f32, f32, f32)> {
    let grow = |side: f32| side.clamp(0.0, RESIZE_HANDLE);
    let (gl, gt, gr, gb) = (
        grow(inset.left),
        grow(inset.top),
        grow(inset.right),
        grow(inset.bottom),
    );
    let left = inset.left - gl;
    let top = inset.top - gt;
    let right = width - inset.right + gr;
    let bottom = height - inset.bottom + gb;
    let rect = Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0));
    if radii.is_square() {
        return vec![rect.tuple()];
    }
    let by = gl.min(gt).min(gr).min(gb);
    rounded_spans(rect, radii.grow(by))
        .into_iter()
        .map(|r| r.tuple())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        Corner, CornerRadii, Rect, WindowShape, curve_depth, input_region, needed_radius,
        rounded_spans, window_corner_radii,
    };
    use crate::system_frame::{Insets, Tiled, WindowState};

    const R: f32 = 12.0;

    fn shape() -> WindowShape {
        WindowShape::new(Rect::new(40.0, 40.0, 800.0, 600.0), CornerRadii::uniform(R))
    }

    #[test]
    fn every_corner_takes_the_measured_radius_while_the_window_floats() {
        let floating = WindowState::new(Some(Tiled::default()), false, false, true);
        let radii = window_corner_radii(floating, 15.0, 15.0);
        assert_eq!(radii.all(), [15.0; 4]);
        // The top and bottom are measured apart; each pair takes its own.
        let radii = window_corner_radii(floating, 12.0, 9.0);
        assert_eq!(radii.all(), [12.0, 12.0, 9.0, 9.0]);
    }

    #[test]
    fn maximized_fullscreen_and_tiled_windows_are_square() {
        let maximized = WindowState::new(Some(Tiled::default()), true, false, true);
        let fullscreen = WindowState::new(Some(Tiled::default()), false, true, true);
        let server = WindowState::new(None, false, false, true);
        assert!(window_corner_radii(maximized, 12.0, 12.0).is_square());
        assert!(window_corner_radii(fullscreen, 12.0, 12.0).is_square());
        assert!(window_corner_radii(server, 12.0, 12.0).is_square());
        // Any one tiled edge squares every corner, as `window.csd.tiled-top`
        // and its siblings do in libadwaita.
        for tiled in [
            Tiled {
                top: true,
                ..Tiled::default()
            },
            Tiled {
                right: true,
                ..Tiled::default()
            },
            Tiled {
                bottom: true,
                ..Tiled::default()
            },
            Tiled {
                left: true,
                ..Tiled::default()
            },
            Tiled::ALL,
        ] {
            let state = WindowState::new(Some(tiled), false, false, true);
            assert!(
                window_corner_radii(state, 12.0, 12.0).is_square(),
                "{tiled:?}"
            );
        }
    }

    #[test]
    fn the_curve_is_as_deep_as_the_radius_at_the_edge_and_flat_past_it() {
        assert_eq!(curve_depth(R, 0.0), R);
        assert_eq!(curve_depth(R, R), 0.0);
        assert_eq!(curve_depth(R, 20.0), 0.0);
        let mid = curve_depth(R, 4.0);
        assert!((mid - (R - (R * R - 64.0f32).sqrt())).abs() < 1e-4);
        assert_eq!(curve_depth(0.0, 0.0), 0.0);
    }

    #[test]
    fn the_window_contains_its_rounded_shape_and_not_the_square_corner() {
        let s = shape();
        assert!(s.contains(440.0, 340.0));
        // The corner pixel itself is outside, at every corner.
        assert!(!s.contains(40.5, 40.5));
        assert!(!s.contains(839.5, 40.5));
        assert!(!s.contains(839.5, 639.5));
        assert!(!s.contains(40.5, 639.5));
        // Along the edges past the curve it is inside.
        assert!(s.contains(40.5, 60.0));
        assert!(s.contains(60.0, 639.5));
        // The curve's 45° point.
        let d = R * (1.0 - std::f32::consts::FRAC_1_SQRT_2) + 0.01;
        assert!(s.contains(40.0 + d, 640.0 - d));
        assert!(!s.contains(40.0 + d - 0.2, 640.0 - d + 0.2));
        let square = WindowShape::new(s.rect, CornerRadii::SQUARE);
        assert!(square.contains(40.5, 639.5));
    }

    #[test]
    fn a_fill_flush_with_a_corner_takes_the_windows_radius() {
        let s = shape();
        // A sidebar down the left edge to the bottom.
        let sidebar = Rect::new(40.0, 90.0, 280.0, 550.0);
        let radii = s.fill_radii(sidebar, CornerRadii::SQUARE);
        assert_eq!(radii.bottom_left, R);
        assert_eq!(radii.bottom_right, 0.0);
        assert_eq!(radii.top_left, 0.0);
        // The page beside it reaches the bottom right corner.
        let page = Rect::new(320.0, 90.0, 520.0, 550.0);
        assert_eq!(s.fill_radii(page, CornerRadii::SQUARE).bottom_right, R);
        // The whole window takes all four.
        assert_eq!(s.fill_radii(s.rect, CornerRadii::SQUARE).all(), [R; 4]);
        // A card away from the corners keeps its own radius.
        let card = Rect::new(60.0, 100.0, 200.0, 100.0);
        assert_eq!(
            s.fill_radii(card, CornerRadii::uniform(8.0)),
            CornerRadii::uniform(8.0)
        );
        // A square window rounds nothing.
        let square = WindowShape::new(s.rect, CornerRadii::SQUARE);
        assert!(square.fill_radii(sidebar, CornerRadii::SQUARE).is_square());
    }

    #[test]
    fn a_fill_near_a_corner_gets_just_the_radius_that_keeps_it_inside() {
        // A row 4px in from the side and touching the bottom: its arc of
        // radius 8 is tangent to the window's.
        assert!((needed_radius(R, 4.0, 0.0) - 8.0).abs() < 1e-4);
        assert_eq!(needed_radius(R, 0.0, 0.0), R);
        assert_eq!(needed_radius(R, R, 0.0), 0.0);
        // Past the 45° point the square corner is already inside.
        assert_eq!(needed_radius(R, 4.0, 4.0), 0.0);
        let s = shape();
        for (ox, oy) in [(0.0, 0.0), (2.0, 0.0), (4.0, 1.0), (0.0, 6.0), (3.0, 3.0)] {
            let rect = Rect::new(40.0 + ox, 540.0, 100.0, 100.0 - oy);
            let rho = s.fill_radii(rect, CornerRadii::SQUARE).bottom_left;
            // The rounded corner's arc, sampled, lies inside the window.
            for step in 0..=32 {
                let t = step as f32 / 32.0 * std::f32::consts::FRAC_PI_2;
                let x = rect.x + rho - rho * t.cos();
                let y = rect.bottom() - rho + rho * t.sin();
                assert!(s.contains(x, y), "({ox}, {oy}) at {t}");
            }
        }
    }

    /// Every pixel centre of `rect` at `scale` that is at least `inset` in
    /// from the sides of `content` lies inside the shape.
    fn inside_from(shape: &WindowShape, rect: Rect, content: Rect, inset: f32, scale: f32) -> bool {
        let cols = (rect.width * scale).round() as i32;
        let rows = (rect.height * scale).round() as i32;
        (0..rows).all(|j| {
            (0..cols).all(|i| {
                let x = rect.x + (i as f32 + 0.5) / scale;
                let y = rect.y + (j as f32 + 0.5) / scale;
                x < content.x + inset || x > content.right() - inset || shape.contains(x, y)
            })
        })
    }

    #[test]
    fn content_clipped_at_its_inset_never_crosses_the_curve() {
        for radius in [12.0, 15.0] {
            let s = WindowShape::new(
                Rect::new(40.0, 40.0, 800.0, 600.0),
                CornerRadii::uniform(radius),
            );
            let rects = [
                s.rect,
                // A pane down the left edge, a page beside it, a bar along the
                // bottom, a list inset from the side.
                Rect::new(40.0, 100.0, 280.0, 540.0),
                Rect::new(320.0, 100.0, 520.0, 540.0),
                Rect::new(40.0, 608.0, 800.0, 32.0),
                Rect::new(44.0, 620.0, 100.0, 40.0),
                Rect::new(0.0, 0.0, 1000.0, 1000.0),
            ];
            for scale in [1.0, 1.25, 2.0] {
                for rect in rects {
                    for inset in [0.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0] {
                        let clipped = s.clip(rect, inset, scale);
                        let bound = rect.intersect(&s.rect);
                        assert!(
                            inside_from(&s, clipped, bound, inset, scale),
                            "r {radius} {rect:?} inset {inset} at {scale}: {clipped:?}"
                        );
                        // The sides never move: content keeps its full width.
                        assert_eq!(clipped.x, bound.x);
                        assert_eq!(clipped.width, bound.width);
                        assert!(clipped.y >= bound.y && clipped.bottom() <= bound.bottom());
                    }
                }
            }
        }
    }

    #[test]
    fn the_band_is_the_curves_depth_at_the_contents_inset() {
        let s = WindowShape::new(
            Rect::new(40.0, 40.0, 800.0, 600.0),
            CornerRadii::uniform(15.0),
        );
        // A full-width list: the whole radius, top and bottom.
        assert_eq!(
            s.clip(s.rect, 0.0, 1.0),
            Rect::new(40.0, 55.0, 800.0, 570.0)
        );
        // Rows 4px in from the side cross the curve only in its last 4.8px:
        // 15 - sqrt(15² - 11²) = 4.80, five whole pixels at scale 1 and ten
        // device pixels at scale 2.
        let page = Rect::new(320.0, 100.0, 520.0, 540.0);
        assert_eq!(
            s.clip(page, 4.0, 1.0),
            Rect::new(320.0, 100.0, 520.0, 535.0)
        );
        assert_eq!(
            s.clip(page, 4.0, 2.0),
            Rect::new(320.0, 100.0, 520.0, 535.0)
        );
        // 12px in: 15 - sqrt(225 - 9) = 0.30, one pixel.
        assert_eq!(s.clip(page, 12.0, 1.0).bottom(), 639.0);
        // A page padded the radius or more gives up nothing.
        assert_eq!(s.clip(page, 24.0, 1.0), page);
        assert_eq!(s.clip(page, 15.0, 1.0), page);
        // An element whose side is away from the window's side counts that
        // distance too: a pane 20px in gives up nothing.
        let away = Rect::new(60.0, 100.0, 200.0, 540.0);
        assert_eq!(s.clip(away, 0.0, 1.0), away);
        // Adwaita's 12px corner, 4px in: 12 - sqrt(144 - 64) = 3.06, four
        // pixels at scale 1, 3.5 at scale 2.
        let adw = WindowShape::new(s.rect, CornerRadii::uniform(12.0));
        assert_eq!(adw.corner_depth(4.0, 1.0), 4.0);
        assert_eq!(adw.corner_depth(4.0, 2.0), 3.5);
        assert_eq!(s.corner_depth(4.0, 1.0), 5.0);
        assert_eq!(s.corner_depth(0.0, 1.0), 15.0);
        assert_eq!(s.corner_depth(24.0, 1.0), 0.0);
        // Square corners give up nothing.
        let square = WindowShape::new(s.rect, CornerRadii::SQUARE);
        assert_eq!(square.clip(s.rect, 0.0, 1.0), s.rect);
        assert_eq!(square.corner_depth(0.0, 1.0), 0.0);
    }

    #[test]
    fn a_floating_box_is_moved_inside_the_window_and_clear_of_its_curves() {
        let s = WindowShape::new(
            Rect::new(40.0, 40.0, 800.0, 600.0),
            CornerRadii::uniform(15.0),
        );
        // 15 (1 - 1/√2) = 4.39: five pixels in on every side.
        assert_eq!(s.overlay_area(), Rect::new(45.0, 45.0, 790.0, 590.0));
        // A tooltip under the pointer at the bottom right, reaching into the
        // shadow: moved up and left until its square corner is inside.
        let tip = Rect::new(800.0, 630.0, 120.0, 30.0);
        let (dx, dy) = s.shift_inside(tip);
        assert_eq!((dx, dy), (-85.0, -25.0));
        let moved = Rect::new(tip.x + dx, tip.y + dy, tip.width, tip.height);
        for (x, y) in [
            (moved.x, moved.y),
            (moved.right(), moved.y),
            (moved.x, moved.bottom()),
            (moved.right(), moved.bottom()),
        ] {
            assert!(s.contains(x, y), "({x}, {y})");
        }
        // One already inside does not move; one wider than the window keeps
        // its leading edge.
        assert_eq!(
            s.shift_inside(Rect::new(100.0, 100.0, 50.0, 20.0)),
            (0.0, 0.0)
        );
        assert_eq!(s.shift_inside(Rect::new(0.0, 100.0, 900.0, 20.0)).0, 45.0);
        // A square window keeps 4px.
        let square = WindowShape::new(s.rect, CornerRadii::SQUARE);
        assert_eq!(square.overlay_area(), Rect::new(44.0, 44.0, 792.0, 592.0));
    }

    #[test]
    fn a_submenu_that_does_not_fit_opens_on_its_parents_other_side() {
        let s = WindowShape::new(
            Rect::new(40.0, 40.0, 800.0, 600.0),
            CornerRadii::uniform(15.0),
        );
        // The area runs 45 to 835.
        // Room after the row: there.
        assert_eq!(s.submenu_x(300.0, 500.0, 200.0), 500.0);
        // No room after a row at the trailing edge: before it, clear of the
        // parent's shortcut column and arrow.
        assert_eq!(s.submenu_x(600.0, 800.0, 200.0), 400.0);
        // Room on neither side: inside the area, as far right as it fits.
        assert_eq!(s.submenu_x(100.0, 800.0, 200.0), 635.0);
        // Wider than the window: its leading edge stays in.
        assert_eq!(s.submenu_x(100.0, 800.0, 900.0), 45.0);
    }

    #[test]
    fn the_rows_outside_the_curve_are_exactly_the_pixels_outside_it() {
        let s = shape();
        let rows = s.outside_rows();
        for corner in Corner::ALL {
            assert!(rows.iter().any(|(c, _)| *c == corner), "{corner:?}");
        }
        for (_, row) in &rows {
            for i in 0..row.width as i32 {
                assert!(!s.contains(row.x + i as f32 + 0.5, row.y + 0.5));
            }
        }
        // Every pixel of the bottom left 12x12 square is either in a row or
        // inside the shape.
        for j in 0..12 {
            for i in 0..12 {
                let (x, y) = (40.0 + i as f32 + 0.5, 628.0 + j as f32 + 0.5);
                let in_row = rows
                    .iter()
                    .any(|(_, r)| x >= r.x && x < r.right() && y >= r.y && y < r.bottom());
                assert_ne!(in_row, s.contains(x, y), "({i}, {j})");
            }
        }
        assert!(
            WindowShape::new(s.rect, CornerRadii::SQUARE)
                .outside_rows()
                .is_empty()
        );
    }

    #[test]
    fn a_point_outside_the_curve_names_its_corner() {
        let s = shape();
        assert_eq!(s.corner_outside(40.5, 639.5), Some(Corner::BottomLeft));
        assert_eq!(s.corner_outside(839.5, 639.5), Some(Corner::BottomRight));
        assert_eq!(s.corner_outside(40.5, 40.5), Some(Corner::TopLeft));
        assert_eq!(s.corner_outside(839.5, 40.5), Some(Corner::TopRight));
        assert_eq!(s.corner_outside(440.0, 340.0), None);
        assert_eq!(s.corner_outside(10.0, 10.0), None);
    }

    #[test]
    fn spans_cover_the_rounded_shape_pixel_for_pixel() {
        let rect = Rect::new(28.0, 28.0, 400.0, 300.0);
        let radii = CornerRadii::uniform(24.0);
        let s = WindowShape::new(rect, radii);
        let spans = rounded_spans(rect, radii);
        for y in 28..328 {
            for x in 28..428 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let covered = spans
                    .iter()
                    .any(|r| px >= r.x && px < r.right() && py >= r.y && py < r.bottom());
                assert_eq!(covered, s.contains(px, py), "({x}, {y})");
            }
        }
        // The straight middle is one rectangle.
        assert!(spans.len() < 2 * 24 + 2);
        assert_eq!(rounded_spans(rect, CornerRadii::SQUARE), vec![rect]);
    }

    #[test]
    fn input_follows_the_window_curve_grown_by_the_handle() {
        let inset = Insets::uniform(40.0);
        let square = input_region(1000.0, 1000.0, inset, CornerRadii::SQUARE);
        assert_eq!(square, vec![(28.0, 28.0, 944.0, 944.0)]);
        let round = input_region(1000.0, 1000.0, inset, CornerRadii::uniform(R));
        let covered = |x: f32, y: f32| {
            round
                .iter()
                .any(|(rx, ry, rw, rh)| x >= *rx && x < rx + rw && y >= *ry && y < ry + rh)
        };
        // The window's own corner pixel resizes: it is inside the grown curve.
        assert!(covered(40.5, 959.5));
        // The handle's square corner, past the grown curve, does not take
        // input: it goes to the window below.
        assert!(!covered(28.5, 971.5));
        assert!(!covered(28.5, 28.5));
        // Along the edges the whole handle takes input.
        assert!(covered(28.5, 500.0));
        assert!(covered(500.0, 971.5));
        // A handle clamped by a narrow inset never reaches past the surface.
        let narrow = input_region(100.0, 100.0, Insets::uniform(5.0), CornerRadii::uniform(R));
        for (x, y, w, h) in narrow {
            assert!(x >= 0.0 && y >= 0.0 && x + w <= 100.0 && y + h <= 100.0);
        }
    }
}
