//! Where the window pictures go in window mode (spec C12): spread over their
//! output in rows, as the overview spreads them, inside GNOME's margins
//! (`.screenshot-ui-window-selector-window-container`: 100 px each side,
//! 200 px at the bottom of the output with the panel), never larger than
//! the window itself.
//!
//! GNOME uses its overview's layout (`Workspace.WorkspaceLayout`); this is
//! the same idea in its simplest exact form: try every number of rows, fill
//! the rows in order, scale each row to fit, and keep the arrangement whose
//! smallest picture is largest.

use crate::geometry::Rect;

/// GNOME's margin around the pictures.
pub const MARGIN: i32 = 100;
/// The bottom margin on the output that shows the panel.
pub const PANEL_MARGIN: i32 = 200;
/// The space between pictures.
pub const SPACING: f64 = 24.0;

/// The area pictures are laid out in on an output.
pub fn layout_area(output: Rect, has_panel: bool) -> Rect {
    let bottom = if has_panel { PANEL_MARGIN } else { MARGIN };
    Rect::new(
        output.x + MARGIN,
        output.y + MARGIN,
        (output.width - 2 * MARGIN).max(1),
        (output.height - MARGIN - bottom).max(1),
    )
}

/// Places windows of the given logical sizes inside `area`. The result has
/// one rectangle per window, in the same order.
pub fn arrange(sizes: &[(i32, i32)], area: Rect) -> Vec<Rect> {
    if sizes.is_empty() {
        return Vec::new();
    }
    let count = sizes.len();
    let mut best: Option<(f64, Vec<Rect>)> = None;
    for rows in 1..=count {
        let per_row = count.div_ceil(rows);
        let chunks: Vec<&[(i32, i32)]> = sizes.chunks(per_row).collect();
        let row_height = (f64::from(area.height) - SPACING * (chunks.len() as f64 - 1.0))
            / chunks.len() as f64;
        if row_height <= 0.0 {
            break;
        }
        let mut placed = Vec::with_capacity(count);
        let mut smallest = f64::INFINITY;
        let mut rows_out: Vec<(f64, Vec<(f64, f64)>)> = Vec::new();
        for chunk in &chunks {
            let total_width: f64 = chunk.iter().map(|(w, _)| f64::from((*w).max(1))).sum();
            let tallest = chunk
                .iter()
                .map(|(_, h)| f64::from((*h).max(1)))
                .fold(1.0, f64::max);
            let width_room = f64::from(area.width) - SPACING * (chunk.len() as f64 - 1.0);
            let scale = (width_room / total_width)
                .min(row_height / tallest)
                .min(1.0);
            smallest = smallest.min(scale);
            let items: Vec<(f64, f64)> = chunk
                .iter()
                .map(|(w, h)| (f64::from((*w).max(1)) * scale, f64::from((*h).max(1)) * scale))
                .collect();
            let height = items.iter().map(|(_, h)| *h).fold(0.0, f64::max);
            rows_out.push((height, items));
        }
        // Centre the rows vertically and each row horizontally.
        let used: f64 = rows_out.iter().map(|(h, _)| *h).sum::<f64>()
            + SPACING * (rows_out.len() as f64 - 1.0);
        let mut y = f64::from(area.y) + (f64::from(area.height) - used) / 2.0;
        for (height, items) in &rows_out {
            let width: f64 =
                items.iter().map(|(w, _)| *w).sum::<f64>() + SPACING * (items.len() as f64 - 1.0);
            let mut x = f64::from(area.x) + (f64::from(area.width) - width) / 2.0;
            for (w, h) in items {
                let top = y + (height - h) / 2.0;
                placed.push(Rect::new(
                    x.round() as i32,
                    top.round() as i32,
                    w.round().max(1.0) as i32,
                    h.round().max(1.0) as i32,
                ));
                x += w + SPACING;
            }
            y += height + SPACING;
        }
        if best.as_ref().is_none_or(|(score, _)| smallest > *score) {
            best = Some((smallest, placed));
        }
    }
    best.map(|(_, placed)| placed).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{arrange, layout_area};
    use crate::geometry::Rect;

    #[test]
    fn the_area_leaves_gnomes_margins() {
        let output = Rect::new(0, 0, 1280, 800);
        assert_eq!(layout_area(output, false), Rect::new(100, 100, 1080, 600));
        assert_eq!(layout_area(output, true), Rect::new(100, 100, 1080, 500));
    }

    #[test]
    fn one_small_window_keeps_its_size_and_is_centred() {
        let area = Rect::new(100, 100, 1080, 600);
        let placed = arrange(&[(400, 300)], area);
        assert_eq!(placed, vec![Rect::new(440, 250, 400, 300)]);
    }

    #[test]
    fn pictures_never_overlap_and_stay_inside() {
        let area = Rect::new(100, 100, 1080, 500);
        let sizes = [(1280, 800), (800, 600), (640, 480), (300, 900), (1000, 200)];
        let placed = arrange(&sizes, area);
        assert_eq!(placed.len(), sizes.len());
        for (i, a) in placed.iter().enumerate() {
            assert!(area.contains_rect(a), "{a:?} outside {area:?}");
            for b in &placed[i + 1..] {
                assert!(a.intersection(b).is_none(), "{a:?} overlaps {b:?}");
            }
        }
        // Each keeps its shape.
        for (rect, (w, h)) in placed.iter().zip(sizes) {
            let want = f64::from(w) / f64::from(h);
            let got = f64::from(rect.width) / f64::from(rect.height);
            assert!((want - got).abs() / want < 0.05, "{rect:?} for {w}x{h}");
        }
    }

    #[test]
    fn many_windows_take_several_rows() {
        let area = Rect::new(0, 0, 1000, 1000);
        let placed = arrange(&[(800, 800); 4], area);
        let rows: std::collections::BTreeSet<i32> = placed.iter().map(|r| r.y).collect();
        assert_eq!(rows.len(), 2);
        assert!(arrange(&[], area).is_empty());
    }
}
