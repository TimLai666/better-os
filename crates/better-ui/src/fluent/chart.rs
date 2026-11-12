//! Charts: a history line chart with an area under its first series, the way
//! Windows 11 Task Manager draws a resource over the last minute, and a
//! sparkline small enough for a card or a list row.
//!
//! WinUI 3 has no chart control, so this follows Task Manager's Performance
//! page instead: a thin stroke around the plot, a light grid, the newest
//! reading at the right edge, the series drawn as a line with a tinted area
//! under it, and a second series (write beside read, send beside receive)
//! drawn as a dashed line without a fill. A reading that is missing leaves a
//! gap in the line rather than dropping to zero, because no reading is not the
//! same as a reading of nothing.
//!
//! Colors are categorical, one hue per kind of resource as Task Manager uses
//! them (processor blue, memory purple, disk green, network orange), with a
//! lighter variant for the dark appearance. They are data colors, not the
//! accent: the accent stays reserved for the primary action, the selection,
//! and focus.
//!
//! The geometry is plain functions so it is tested without a window; the
//! elements only paint what those functions return.

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, PathBuilder, Pixels,
    RenderOnce, Rgba, SharedString, Styled, Window, canvas, div, point, px,
};

use super::{CAPTION, Palette, palette, radius, text, with_alpha};

/// A data color.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ChartHue {
    /// Processor.
    Blue,
    /// Memory.
    Purple,
    /// Disk.
    Green,
    /// Network.
    Orange,
    /// Graphics and sensors.
    Teal,
}

impl ChartHue {
    /// The line color in the given appearance.
    pub fn color(self, dark: bool) -> Rgba {
        let rgb = match (self, dark) {
            (ChartHue::Blue, false) => 0x117dbb,
            (ChartHue::Blue, true) => 0x4cb4f5,
            (ChartHue::Purple, false) => 0x8b12ae,
            (ChartHue::Purple, true) => 0xc27ce6,
            (ChartHue::Green, false) => 0x4da60c,
            (ChartHue::Green, true) => 0x7fcf4a,
            (ChartHue::Orange, false) => 0xa74f01,
            (ChartHue::Orange, true) => 0xf0a35a,
            (ChartHue::Teal, false) => 0x00838f,
            (ChartHue::Teal, true) => 0x4fcfd6,
        };
        super::hex(rgb)
    }

    /// The tint under the first series.
    pub fn area(self, dark: bool) -> Rgba {
        with_alpha(self.color(dark), if dark { 0.22 } else { 0.16 })
    }
}

/// One line on a chart: readings oldest first, `None` where there was none.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSeries {
    pub values: Vec<Option<f64>>,
    pub hue: ChartHue,
}

impl ChartSeries {
    pub fn new(values: Vec<Option<f64>>, hue: ChartHue) -> Self {
        Self { values, hue }
    }
}

/// The largest reading in any series, or `None` when nothing was measured.
pub fn peak(series: &[ChartSeries]) -> Option<f64> {
    series
        .iter()
        .flat_map(|series| series.values.iter().flatten().copied())
        .filter(|value| value.is_finite())
        .fold(None, |peak: Option<f64>, value| {
            Some(peak.map_or(value, |peak| peak.max(value)))
        })
}

/// A round top for an axis that scales itself: the smallest of 1, 2, or 5
/// times a power of ten that holds `peak`, and never less than `floor`. A
/// chart of network traffic that tops out at 3.1 KB/s reads against 5 KB/s,
/// not against an arbitrary 3.1.
pub fn nice_maximum(peak: f64, floor: f64) -> f64 {
    let floor = if floor.is_finite() && floor > 0.0 {
        floor
    } else {
        1.0
    };
    if !peak.is_finite() || peak <= floor {
        return floor;
    }
    let magnitude = 10f64.powf(peak.log10().floor());
    for step in [1.0, 2.0, 5.0, 10.0] {
        let candidate = step * magnitude;
        if candidate >= peak {
            return candidate;
        }
    }
    10.0 * magnitude
}

/// Runs of connected points in a unit square, as [`runs`] returns them.
pub type Runs = Vec<Vec<(f32, f32)>>;

/// Where each reading sits in a unit square, as runs of connected points.
///
/// `x` runs from 0 at the left to 1 at the right and `y` from 0 at the bottom
/// to 1 at the top. The chart holds `capacity` readings; the newest is at the
/// right edge and a short history starts part of the way across, so the line
/// grows in from the right as Task Manager's does. A reading above the
/// maximum is drawn at the top. A missing reading ends a run, so the line has
/// a gap there; a run of one point is kept and drawn as a short tick.
pub fn runs(values: &[Option<f64>], capacity: usize, maximum: f64) -> Runs {
    let capacity = capacity.max(2);
    let shown = &values[values.len().saturating_sub(capacity)..];
    let offset = capacity - shown.len();
    let span = (capacity - 1) as f32;
    let maximum = if maximum.is_finite() && maximum > 0.0 {
        maximum
    } else {
        1.0
    };
    let mut runs = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    for (index, value) in shown.iter().enumerate() {
        match value.filter(|value| value.is_finite()) {
            Some(value) => {
                let x = (offset + index) as f32 / span;
                let y = (value / maximum).clamp(0.0, 1.0) as f32;
                current.push((x, y));
            }
            None => {
                if !current.is_empty() {
                    runs.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

/// A history chart.
#[derive(IntoElement)]
pub struct LineChart {
    id: ElementId,
    series: Vec<ChartSeries>,
    capacity: usize,
    maximum: f64,
    height: Pixels,
    grid: bool,
    top_left: Option<SharedString>,
    top_right: Option<SharedString>,
    bottom_left: Option<SharedString>,
    bottom_right: Option<SharedString>,
}

impl LineChart {
    /// A chart that holds `capacity` readings against a top of `maximum`.
    pub fn new(
        id: impl Into<ElementId>,
        series: Vec<ChartSeries>,
        capacity: usize,
        maximum: f64,
    ) -> Self {
        Self {
            id: id.into(),
            series,
            capacity,
            maximum,
            height: px(160.0),
            grid: true,
            top_left: None,
            top_right: None,
            bottom_left: None,
            bottom_right: None,
        }
    }

    /// A sparkline: no grid and no labels, for a card or a list row.
    pub fn sparkline(
        id: impl Into<ElementId>,
        series: Vec<ChartSeries>,
        capacity: usize,
        maximum: f64,
    ) -> Self {
        Self::new(id, series, capacity, maximum)
            .height(px(40.0))
            .grid(false)
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn grid(mut self, grid: bool) -> Self {
        self.grid = grid;
        self
    }

    /// What is plotted, above the chart at the left (`% utilization`).
    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.top_left = Some(caption.into());
        self
    }

    /// The top of the axis, above the chart at the right (`100%`).
    pub fn maximum_label(mut self, label: impl Into<SharedString>) -> Self {
        self.top_right = Some(label.into());
        self
    }

    /// The time span, below the chart at the left (`60 seconds`).
    pub fn span_label(mut self, label: impl Into<SharedString>) -> Self {
        self.bottom_left = Some(label.into());
        self
    }

    /// The bottom of the axis, below the chart at the right (`0`).
    pub fn minimum_label(mut self, label: impl Into<SharedString>) -> Self {
        self.bottom_right = Some(label.into());
        self
    }
}

fn caption_row(
    left: Option<SharedString>,
    right: Option<SharedString>,
    p: &Palette,
) -> Option<gpui::Div> {
    if left.is_none() && right.is_none() {
        return None;
    }
    Some(
        div()
            .flex()
            .justify_between()
            .gap(px(8.0))
            .min_w_0()
            .child(
                text(div(), CAPTION)
                    .min_w_0()
                    .truncate()
                    .text_color(p.text_secondary)
                    .child(left.unwrap_or_default()),
            )
            .child(
                text(div(), CAPTION)
                    .flex_none()
                    .text_color(p.text_secondary)
                    .child(right.unwrap_or_default()),
            ),
    )
}

impl RenderOnce for LineChart {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let dark = p.is_dark();
        let grid_color = p.divider;
        let grid = self.grid;
        let capacity = self.capacity;
        let maximum = self.maximum;
        let plotted: Vec<(Runs, ChartHue)> = self
            .series
            .iter()
            .map(|series| (runs(&series.values, capacity, maximum), series.hue))
            .collect();
        let plot = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let origin = bounds.origin;
                let width = bounds.size.width;
                let height = bounds.size.height;
                let at =
                    |(x, y): (f32, f32)| point(origin.x + width * x, origin.y + height * (1.0 - y));
                if grid {
                    for step in 1..4 {
                        let y = step as f32 / 4.0;
                        let mut line = PathBuilder::stroke(px(1.0));
                        line.move_to(at((0.0, y)));
                        line.line_to(at((1.0, y)));
                        if let Ok(path) = line.build() {
                            window.paint_path(path, grid_color);
                        }
                    }
                    for step in 1..6 {
                        let x = step as f32 / 6.0;
                        let mut line = PathBuilder::stroke(px(1.0));
                        line.move_to(at((x, 0.0)));
                        line.line_to(at((x, 1.0)));
                        if let Ok(path) = line.build() {
                            window.paint_path(path, grid_color);
                        }
                    }
                }
                for (position, (runs, hue)) in plotted.iter().enumerate() {
                    let first = position == 0;
                    for run in runs {
                        if first && run.len() > 1 {
                            let mut area = PathBuilder::fill();
                            area.move_to(at((run[0].0, 0.0)));
                            for &point in run {
                                area.line_to(at(point));
                            }
                            area.line_to(at((run[run.len() - 1].0, 0.0)));
                            area.close();
                            if let Ok(path) = area.build() {
                                window.paint_path(path, hue.area(dark));
                            }
                        }
                        let mut line = PathBuilder::stroke(px(if grid { 1.5 } else { 1.25 }));
                        if !first {
                            line = line.dash_array(&[px(4.0), px(3.0)]);
                        }
                        line.move_to(at(run[0]));
                        if run.len() == 1 {
                            // A lone reading between two gaps: a short tick
                            // so it is visible at all.
                            let (x, y) = run[0];
                            line.line_to(point(at((x, y)).x - px(2.0), at((x, y)).y));
                        }
                        for &point in &run[1..] {
                            line.line_to(at(point));
                        }
                        if let Ok(path) = line.build() {
                            window.paint_path(path, hue.color(dark));
                        }
                    }
                }
            },
        )
        .size_full();
        let frame = div()
            .id(self.id)
            .relative()
            .w_full()
            .h(self.height)
            .rounded(radius::SM)
            .border_1()
            .border_color(if grid {
                p.card_stroke
            } else {
                with_alpha(p.card_stroke, 0.0)
            })
            .overflow_hidden()
            .child(plot);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w_full()
            .min_w_0()
            .children(caption_row(self.top_left, self.top_right, p))
            .child(frame)
            .children(caption_row(self.bottom_left, self.bottom_right, p))
    }
}

/// A horizontal bar split into parts, for a composition such as memory in
/// use, cached, and free. Each part is a fraction of the whole; what the parts
/// leave over is drawn as empty track.
#[derive(IntoElement)]
pub struct CompositionBar {
    parts: Vec<(f32, Rgba)>,
    height: Pixels,
}

impl CompositionBar {
    pub fn new(parts: Vec<(f32, Rgba)>) -> Self {
        Self {
            parts,
            height: px(40.0),
        }
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }
}

/// The widths of a composition's parts, clamped so they never add up to
/// more than the whole: a part that would overflow is cut to what is left.
pub fn composition(parts: &[f32]) -> Vec<f32> {
    let mut left = 1.0_f32;
    parts
        .iter()
        .map(|part| {
            let part = if part.is_finite() {
                part.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let width = part.min(left);
            left -= width;
            width
        })
        .collect()
}

impl RenderOnce for CompositionBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let widths = composition(&self.parts.iter().map(|(f, _)| *f).collect::<Vec<_>>());
        let mut bar = div()
            .flex()
            .w_full()
            .h(self.height)
            .rounded(radius::SM)
            .border_1()
            .border_color(p.card_stroke)
            .overflow_hidden();
        for ((_, color), width) in self.parts.iter().zip(widths) {
            if width > 0.0 {
                bar = bar.child(
                    div()
                        .h_full()
                        .flex_none()
                        .w(gpui::relative(width))
                        .bg(*color)
                        .border_r_1()
                        .border_color(p.card_stroke),
                );
            }
        }
        bar
    }
}
