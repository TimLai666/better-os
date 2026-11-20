//! The window's rounded shape, and how content is kept inside it (ticket
//! 141).
//!
//! GTK 4 and libadwaita windows on GNOME round all four corners with the
//! theme's `border-radius` and clip their content to that shape; a maximized,
//! fullscreen, or tiled window (on any edge: `window.csd.tiled-*` sets
//! `border-radius: 0` in libadwaita 1.5 and in the Zorin themes) has square
//! corners. GPUI clips content to rectangles only, so the kit keeps content
//! inside the curve itself: a container that reaches a window corner paints
//! its fill with the corner rounded ([`WindowShape::fill_radii`]) and paints
//! whatever may scroll or float into the corner inside a rectangle that stays
//! clear of the curve ([`WindowShape::clip`]). Everything here is plain
//! geometry in logical pixels, so it is tested without GPUI.

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

/// How a rectangle that would reach outside a rounded corner is kept clear
/// of it. Each keeps the rectangle whole away from the corner; they differ in
/// which strip along the window's edge they give up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cut {
    /// Raise the edge along the window's top or bottom: for a list that
    /// scrolls into the corner, whose rows run the whole width.
    Vertical,
    /// Move the edge along the window's side in: for a bar along the bottom
    /// whose contents are padded away from its ends.
    Horizontal,
    /// Move both in to the curve's 45° point, about 30% of the radius: for
    /// contents inset from both edges, such as floating cards and their
    /// shadows.
    Inset,
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

    /// The part of `element` its contents are painted in so that none of it
    /// lies outside the window's rounded corners: `element` inside the
    /// window, with the strip `cut` names given up at every rounded corner
    /// it would otherwise reach past. Edges move only inward, to whole device
    /// pixels at `scale`.
    pub fn clip(&self, element: Rect, cut: Cut, scale: f32) -> Rect {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let snap = |v: f32| (v * scale - 1e-3).ceil() / scale;
        let mut v = element.intersect(&self.rect);
        for corner in Corner::ALL {
            let r = self.radii.get(corner);
            if r <= 0.0 || v.is_empty() {
                continue;
            }
            let (x, y) = element_corner(&v, corner);
            let (ox, oy) = offsets(&self.rect, corner, x, y);
            if needed_radius(r, ox, oy) <= 0.0 {
                continue;
            }
            let diagonal = r * (1.0 - std::f32::consts::FRAC_1_SQRT_2);
            let (new_ox, new_oy) = match cut {
                Cut::Vertical => (ox, snap(curve_depth(r, ox))),
                Cut::Horizontal => (snap(curve_depth(r, oy)), oy),
                Cut::Inset if ox >= diagonal => (ox, snap(curve_depth(r, ox))),
                Cut::Inset if oy >= diagonal => (snap(curve_depth(r, oy)), oy),
                Cut::Inset => (snap(diagonal), snap(diagonal)),
            };
            let (new_ox, new_oy) = (new_ox.max(ox), new_oy.max(oy));
            let mut left = v.x;
            let mut right = v.right();
            let mut top = v.y;
            let mut bottom = v.bottom();
            if corner.is_left() {
                left = left.max(self.rect.x + new_ox);
            } else {
                right = right.min(self.rect.right() - new_ox);
            }
            if corner.is_top() {
                top = top.max(self.rect.y + new_oy);
            } else {
                bottom = bottom.min(self.rect.bottom() - new_oy);
            }
            v = Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0));
        }
        v
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
        Corner, CornerRadii, Cut, Rect, WindowShape, curve_depth, input_region, needed_radius,
        rounded_spans, window_corner_radii,
    };
    use crate::system_frame::{Insets, Tiled, WindowState};

    const R: f32 = 12.0;

    fn shape() -> WindowShape {
        WindowShape::new(Rect::new(40.0, 40.0, 800.0, 600.0), CornerRadii::uniform(R))
    }

    /// Every pixel centre of `rect` at `scale` lies inside the shape.
    fn inside(shape: &WindowShape, rect: Rect, scale: f32) -> bool {
        let cols = (rect.width * scale).round() as i32;
        let rows = (rect.height * scale).round() as i32;
        (0..rows).all(|j| {
            (0..cols).all(|i| {
                shape.contains(
                    rect.x + (i as f32 + 0.5) / scale,
                    rect.y + (j as f32 + 0.5) / scale,
                )
            })
        })
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

    #[test]
    fn a_clipped_rect_never_reaches_outside_the_curve() {
        let s = shape();
        let rects = [
            s.rect,
            Rect::new(40.0, 100.0, 280.0, 540.0),
            Rect::new(320.0, 100.0, 520.0, 540.0),
            Rect::new(40.0, 608.0, 800.0, 32.0),
            Rect::new(44.0, 620.0, 100.0, 40.0),
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
        ];
        for scale in [1.0, 1.25, 2.0] {
            for rect in rects {
                for cut in [Cut::Vertical, Cut::Horizontal, Cut::Inset] {
                    let clipped = s.clip(rect, cut, scale);
                    assert!(
                        inside(&s, clipped, scale),
                        "{rect:?} {cut:?} at {scale}: {clipped:?}"
                    );
                    // Never larger than the element or the window.
                    let bound = rect.intersect(&s.rect);
                    assert!(clipped.x >= bound.x && clipped.y >= bound.y);
                    assert!(clipped.right() <= bound.right());
                    assert!(clipped.bottom() <= bound.bottom());
                }
            }
        }
    }

    #[test]
    fn each_cut_gives_up_its_own_strip() {
        let s = shape();
        let all = s.rect;
        let vertical = s.clip(all, Cut::Vertical, 1.0);
        assert_eq!(vertical, Rect::new(40.0, 52.0, 800.0, 576.0));
        let horizontal = s.clip(all, Cut::Horizontal, 1.0);
        assert_eq!(horizontal, Rect::new(52.0, 40.0, 776.0, 600.0));
        let inset = s.clip(all, Cut::Inset, 1.0);
        assert_eq!(inset, Rect::new(44.0, 44.0, 792.0, 592.0));
        // At scale 2 the 45° point, 3.51px in, still rounds in to 4px.
        let inset = s.clip(all, Cut::Inset, 2.0);
        assert_eq!(inset, Rect::new(44.0, 44.0, 792.0, 592.0));
        // A bar along the bottom, padded: only its ends move in.
        let bar = Rect::new(40.0, 608.0, 800.0, 32.0);
        assert_eq!(
            s.clip(bar, Cut::Horizontal, 1.0),
            Rect::new(52.0, 608.0, 776.0, 32.0)
        );
        // Contents already clear of the curve are not cut.
        let clear = Rect::new(60.0, 100.0, 760.0, 520.0);
        for cut in [Cut::Vertical, Cut::Horizontal, Cut::Inset] {
            assert_eq!(s.clip(clear, cut, 1.0), clear);
        }
        let square = WindowShape::new(all, CornerRadii::SQUARE);
        assert_eq!(square.clip(all, Cut::Vertical, 1.0), all);
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
