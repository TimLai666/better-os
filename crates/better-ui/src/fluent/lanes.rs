//! A branch graph's cell: the coloured lines and the node one commit row of
//! a commit graph draws, the way `git log --graph`, VS Code's Git Graph, and
//! Fork draw them.
//!
//! WinUI has no such control. The cell follows the kit's chart rules: the
//! lines are data colors ([`ChartHue`], one per lane, cycling), never the
//! accent, and a 1.5px stroke as a history chart's line. A line that changes
//! column is a cubic curve that leaves and arrives vertically, so it meets
//! the straight line in the row above or below without a corner.
//!
//! The geometry is plain functions so it is tested without a window; the
//! element only paints what [`segment`] returns. What runs where is decided
//! by the caller (Better Git's lane layout); this module only draws it.

use gpui::{
    App, IntoElement, ParentElement, PathBuilder, Pixels, RenderOnce, Styled, Window, canvas, div,
    point, px,
};

use super::chart::ChartHue;
use super::palette;

/// The width of one lane.
pub const LANE_WIDTH: Pixels = px(16.0);
/// The stroke of every line.
pub const LINE_WIDTH: Pixels = px(1.5);
/// The node's radius.
pub const NODE_RADIUS: f32 = 4.0;

/// How a line crosses the cell.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LineKind {
    /// Top to bottom in its own column.
    Pass,
    /// From the top at its column into the node.
    In,
    /// From the node to the bottom at its column.
    Out,
}

/// One line in a cell.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LaneLine {
    pub kind: LineKind,
    pub column: u16,
    /// The lane's number; its colour is [`lane_hue`] of it.
    pub lane: u32,
}

/// How the node is drawn.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum NodeStyle {
    /// A filled dot.
    #[default]
    Commit,
    /// A merge commit: a smaller dot, so the lines meeting it stay visible
    /// to its edge.
    Merge,
    /// The commit HEAD is on: a hollow ring, larger.
    Head,
}

/// The colour of a lane: the chart hues in order, cycling.
pub fn lane_hue(lane: u32) -> ChartHue {
    ChartHue::SEQUENCE[lane as usize % ChartHue::SEQUENCE.len()]
}

/// Where a column's lines run, from the cell's left edge.
pub fn column_x(column: u16, lane_width: f32) -> f32 {
    lane_width * column as f32 + lane_width / 2.0
}

/// One line's path in a cell `lane_width` per column and `height` tall,
/// with the node in `node_column`: where it starts and ends, and the two
/// control points when it is a curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub from: (f32, f32),
    pub to: (f32, f32),
    /// `None` for a straight line.
    pub controls: Option<((f32, f32), (f32, f32))>,
}

/// The path [`LaneLine`] takes.
///
/// Expected results:
/// - `Pass` is straight down its column, top to bottom.
/// - `In` runs from the top of its column to the node at mid-height, and
///   `Out` from the node to the bottom of its column.
/// - A line that stays in the node's column is straight; one that changes
///   column is a cubic curve with both control points at the midpoint of its
///   height, so it leaves and arrives vertically.
pub fn segment(line: LaneLine, node_column: u16, lane_width: f32, height: f32) -> Segment {
    let x = column_x(line.column, lane_width);
    let node = (column_x(node_column, lane_width), height / 2.0);
    let (from, to) = match line.kind {
        LineKind::Pass => ((x, 0.0), (x, height)),
        LineKind::In => ((x, 0.0), node),
        LineKind::Out => (node, (x, height)),
    };
    let controls = (from.0 != to.0).then(|| {
        let middle = (from.1 + to.1) / 2.0;
        ((from.0, middle), (to.0, middle))
    });
    Segment { from, to, controls }
}

/// What a cell shows only `visible` columns of has to say about the rest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Clipping {
    /// Where the node is drawn: its own column, or the overflow column just
    /// past the visible ones when its own is cut off, so a node is never
    /// invisible and never sits on a lane that is not its own.
    pub node_column: u16,
    /// The node's own column is cut off.
    pub node_clipped: bool,
    /// Some line or the node lies beyond the visible columns: the cell
    /// draws a marker in the overflow column.
    pub beyond: bool,
}

/// Clipping a row to its first `visible` columns.
///
/// Expected results:
/// - With everything inside, nothing is clipped and no marker is drawn.
/// - A line in a column at or past `visible` puts a marker in the overflow
///   column.
/// - A node past the edge is drawn in the overflow column (`visible`),
///   which no lane is drawn in, and marked.
pub fn clipping(node_column: u16, lines: &[LaneLine], visible: u16) -> Clipping {
    let visible = visible.max(1);
    let node_clipped = node_column >= visible;
    Clipping {
        node_column: if node_clipped { visible } else { node_column },
        node_clipped,
        beyond: node_clipped || lines.iter().any(|l| l.column >= visible),
    }
}

/// A commit row's graph cell.
#[derive(IntoElement)]
pub struct LaneCell {
    node_column: u16,
    node_lane: u32,
    node: NodeStyle,
    lines: Vec<LaneLine>,
    columns: u16,
    visible: Option<u16>,
    lane_width: Pixels,
}

impl LaneCell {
    /// A cell `columns` lanes wide; lines beyond it are clipped.
    pub fn new(node_column: u16, node_lane: u32, lines: Vec<LaneLine>, columns: u16) -> Self {
        Self {
            node_column,
            node_lane,
            node: NodeStyle::Commit,
            lines,
            columns: columns.max(1),
            visible: None,
            lane_width: LANE_WIDTH,
        }
    }

    /// Shows only the first `visible` columns of lanes, and when there are
    /// more, one overflow column after them: what lies beyond is marked
    /// there ([`clipping`]), and a node beyond it is drawn there rather than
    /// left out.
    pub fn visible(mut self, visible: u16) -> Self {
        self.visible = Some(visible.max(1));
        self
    }

    pub fn node(mut self, node: NodeStyle) -> Self {
        self.node = node;
        self
    }

    pub fn lane_width(mut self, width: Pixels) -> Self {
        self.lane_width = width;
        self
    }
}

impl RenderOnce for LaneCell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let dark = p.is_dark();
        let lane_width = f32::from(self.lane_width);
        let shown = self.visible.map_or(self.columns, |v| v.min(self.columns));
        let overflow = self.columns > shown;
        let cell_width = self.lane_width * (shown + u16::from(overflow)) as f32;
        let lanes_width = self.lane_width * shown as f32;
        let clip = clipping(self.node_column, &self.lines, shown);
        let marker = p.text_secondary;
        let LaneCell {
            node_column,
            node_lane,
            node,
            lines,
            ..
        } = self;
        let plot = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let origin = bounds.origin;
                let height = f32::from(bounds.size.height);
                let at = |(x, y): (f32, f32)| point(origin.x + px(x), origin.y + px(y));
                // Lanes stay in their columns; the overflow column holds only
                // clipped nodes and the marker.
                let mask = gpui::ContentMask {
                    bounds: gpui::Bounds::new(origin, gpui::size(lanes_width, bounds.size.height)),
                };
                window.with_content_mask(Some(mask), |window| {
                    for line in &lines {
                        let s = segment(*line, node_column, lane_width, height);
                        let mut path = PathBuilder::stroke(LINE_WIDTH);
                        path.move_to(at(s.from));
                        match s.controls {
                            Some((c1, c2)) => path.cubic_bezier_to(at(s.to), at(c1), at(c2)),
                            None => path.line_to(at(s.to)),
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, lane_hue(line.lane).color(dark));
                        }
                    }
                });
                let color = lane_hue(node_lane).color(dark);
                if clip.beyond {
                    // A small arrow at the start of the overflow column: more
                    // lanes that way.
                    let edge = f32::from(lanes_width);
                    let mid = height / 2.0;
                    let mut arrow = PathBuilder::fill();
                    arrow.add_polygon(
                        &[
                            (edge + 1.0, mid - 3.0),
                            (edge + 4.0, mid),
                            (edge + 1.0, mid + 3.0),
                        ]
                        .map(at),
                        true,
                    );
                    if let Ok(path) = arrow.build() {
                        window.paint_path(path, if clip.node_clipped { color } else { marker });
                    }
                }
                let center = (column_x(clip.node_column, lane_width), height / 2.0);
                let path = match node {
                    NodeStyle::Commit | NodeStyle::Merge => {
                        let radius = if node == NodeStyle::Merge {
                            NODE_RADIUS - 1.0
                        } else {
                            NODE_RADIUS
                        };
                        let mut path = PathBuilder::fill();
                        path.add_polygon(&circle_points(center, radius).map(at), true);
                        path
                    }
                    NodeStyle::Head => {
                        let mut path = PathBuilder::stroke(px(2.0));
                        path.add_polygon(&circle_points(center, NODE_RADIUS + 0.5).map(at), true);
                        path
                    }
                };
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            },
        )
        .size_full();
        div()
            .flex_none()
            .h_full()
            .w(cell_width)
            .overflow_hidden()
            .child(plot)
    }
}

/// A circle as a polygon of 24 points, for a filled path.
fn circle_points(center: (f32, f32), radius: f32) -> [(f32, f32); 24] {
    std::array::from_fn(|i| {
        let angle = i as f32 / 24.0 * std::f32::consts::TAU;
        (
            center.0 + radius * angle.cos(),
            center.1 + radius * angle.sin(),
        )
    })
}
