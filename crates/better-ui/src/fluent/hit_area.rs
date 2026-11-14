//! The highlighted area is the click area.
//!
//! WinUI draws a menu item, a list item, or a navigation item inset inside
//! its row: a 4px corner shape a few pixels in from the panel's sides and
//! from the next row. Drawn with margins, those few pixels belong to no row:
//! the pointer over them still sees the last row highlighted, and a click
//! there lands on the panel and does nothing. At 200% scale that is 4 to 8
//! physical pixels between every two rows.
//!
//! So a kit row is two elements: the hit area, which fills the row edge to
//! edge with no gap to the next one and carries the click and the hover,
//! and inside it the drawn shape, inset by padding and highlighted through
//! the hit area's group. [`RowInset`] is the one place each row's inset is
//! written; the renderers and the tests read the same numbers.

use gpui::{
    Div, ElementId, InteractiveElement as _, Stateful, StatefulInteractiveElement as _,
    Styled as _, div, px,
};

/// The group name a row's hit area gives its drawn shape, so the shape
/// highlights while the pointer is anywhere in the row.
pub const ROW_GROUP: &str = "fluent-hit-row";

/// How far a row's drawn shape sits inside its hit area, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowInset {
    pub x: f32,
    pub y: f32,
}

impl RowInset {
    /// `MenuFlyoutItem`: 4px from the panel's sides, 2px above and below.
    pub const MENU: Self = Self { x: 4.0, y: 2.0 };
    /// `ComboBoxItem`: 5px from the sides, 2px above and below.
    pub const COMBO: Self = Self { x: 5.0, y: 2.0 };
    /// The suggestion list, as the menu.
    pub const SUGGESTION: Self = Self { x: 4.0, y: 2.0 };
    /// `NavigationViewItem`: 4px from the pane's sides, 2px above and below.
    pub const NAVIGATION: Self = Self { x: 4.0, y: 2.0 };
    /// `ListViewItem`: rows 2px apart, edge to edge across the list.
    pub const LIST: Self = Self { x: 0.0, y: 1.0 };
}

/// A row's hit area: the click and hover target, filling the row. Put the
/// drawn shape inside it and highlight that with [`group_look`].
pub fn hit_row(id: impl Into<ElementId>, inset: RowInset) -> Stateful<Div> {
    div()
        .id(id)
        .group(ROW_GROUP)
        .flex()
        .flex_col()
        .flex_none()
        .px(px(inset.x))
        .py(px(inset.y))
}

/// Gives a row's drawn shape its hover and pressed fills from the row's hit
/// area rather than from its own bounds.
/// The shape needs an id of its own: GPUI keeps the pressed state per
/// element.
pub fn group_look(look: Stateful<Div>, hover: gpui::Rgba, pressed: gpui::Rgba) -> Stateful<Div> {
    look.group_hover(ROW_GROUP, move |s| s.bg(hover))
        .group_active(ROW_GROUP, move |s| s.bg(pressed))
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }
}

/// One row as the pointer sees it: where it takes clicks, and where its
/// highlight is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowGeometry {
    pub hit: Rect,
    pub look: Rect,
}

/// Rows stacked down a panel `width` wide, each drawn `look_heights[i]`
/// tall inside its hit area, as [`hit_row`] lays them out.
pub fn stack(inset: RowInset, look_heights: &[f32], width: f32) -> Vec<RowGeometry> {
    let mut y = 0.0;
    look_heights
        .iter()
        .map(|&height| {
            let hit = Rect {
                x: 0.0,
                y,
                width,
                height: height + 2.0 * inset.y,
            };
            y += hit.height;
            RowGeometry {
                hit,
                look: Rect {
                    x: inset.x,
                    y: hit.y + inset.y,
                    width: width - 2.0 * inset.x,
                    height,
                },
            }
        })
        .collect()
}

/// Which row a point falls in.
pub fn row_at(rows: &[RowGeometry], x: f32, y: f32) -> Option<usize> {
    rows.iter().position(|row| row.hit.contains(x, y))
}

/// The points a click test presses in one row, in logical pixels, for a
/// display at `scale`: the left edge, the centre, the right edge, and, when
/// the row has a label and trailing text, the empty space between them.
/// Each point is the centre of a physical pixel, which is where a pointer
/// at that scale can actually be.
pub fn probes(row: &RowGeometry, label_gap: Option<(f32, f32)>, scale: f32) -> Vec<(f32, f32)> {
    let pixel = 1.0 / scale;
    let mid_y = row.look.y + row.look.height / 2.0;
    let mut points = vec![
        (row.hit.x + pixel / 2.0, mid_y),
        (row.hit.x + row.hit.width / 2.0, mid_y),
        (row.hit.x + row.hit.width - pixel / 2.0, mid_y),
    ];
    if let Some((label_end, trailing_start)) = label_gap {
        points.push((row.look.x + (label_end + trailing_start) / 2.0, mid_y));
    }
    points
}

/// Every physical pixel row down a stack at `scale`, as logical y values
/// at the pixel centres: the pointer positions between and inside rows.
pub fn pixel_rows(rows: &[RowGeometry], scale: f32) -> Vec<f32> {
    let Some(last) = rows.last() else {
        return Vec::new();
    };
    let bottom = last.hit.y + last.hit.height;
    let count = (bottom * scale).round() as usize;
    (0..count).map(|i| (i as f32 + 0.5) / scale).collect()
}

/// The click test every kit row goes through: at `scale`, a press at the
/// row's left edge, centre, right edge, and in the gap between its label and
/// trailing text lands in that row; every physical pixel down the stack
/// belongs to some row; and each row's highlight is drawn inside its own
/// hit area, so what looks clickable is clickable. Returns what failed.
pub fn check_rows(
    rows: &[RowGeometry],
    label_gap: Option<(f32, f32)>,
    scale: f32,
) -> Result<(), String> {
    for (index, row) in rows.iter().enumerate() {
        if !row.hit.contains_rect(&row.look) {
            return Err(format!(
                "row {index} draws its highlight outside its hit area"
            ));
        }
        for (x, y) in probes(row, label_gap, scale) {
            if row_at(rows, x, y) != Some(index) {
                return Err(format!(
                    "a press at ({x}, {y}) at scale {scale} misses row {index}"
                ));
            }
        }
    }
    let x = rows.first().map(|r| r.hit.width / 2.0).unwrap_or(0.0);
    for y in pixel_rows(rows, scale) {
        if row_at(rows, x, y).is_none() {
            return Err(format!(
                "the pixel line at y {y} at scale {scale} is in no row"
            ));
        }
    }
    Ok(())
}
