//! The progress bar and the progress ring.
//!
//! `ProgressBar_themeresources.xaml`: a 1px track in
//! `ControlStrongStrokeColorDefault` under a 3px accent bar with 1.5px
//! corners; paused draws the bar in `SystemFillColorCaution`, error in
//! `SystemFillColorCritical`. `ProgressRing_themeresources.xaml`: a 4px
//! stroke; the ring is 32px by default.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, ElementId, IntoElement, ParentElement, PathBuilder, Pixels,
    Point, RenderOnce, Rgba, Styled, Window, canvas, div, point, px, relative,
};

use super::{Palette, palette};

/// What a progress control shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
    /// A known fraction between 0 and 1.
    Determinate(f32),
    /// Work of unknown length.
    Indeterminate,
}

/// The state a progress bar reports, beside its value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ProgressStatus {
    #[default]
    Running,
    /// `ShowPaused`.
    Paused,
    /// `ShowError`.
    Error,
}

/// The fraction of `value` between `minimum` and `maximum`, clamped; a range
/// with no width reads as empty.
pub fn fraction(value: f64, minimum: f64, maximum: f64) -> f32 {
    if maximum <= minimum || value.is_nan() {
        return 0.0;
    }
    ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0) as f32
}

/// The bar's color for its status.
pub fn bar_color(status: ProgressStatus, p: &Palette) -> Rgba {
    match status {
        ProgressStatus::Running => p.accent,
        ProgressStatus::Paused => p.caution,
        ProgressStatus::Error => p.critical,
    }
}

/// Where the indeterminate bar's moving segment is at time `t` (0 to 1 over
/// one cycle): its left edge and width as fractions of the track. The
/// segment enters from the left and leaves at the right, like WinUI's.
pub fn indeterminate_segment(t: f32) -> (f32, f32) {
    let t = t.clamp(0.0, 1.0);
    let width = 0.4;
    let left = -width + t * (1.0 + width);
    let start = left.max(0.0);
    let end = (left + width).min(1.0);
    (start, (end - start).max(0.0))
}

/// The indeterminate ring's arc at time `t` (0 to 1 over one cycle): its
/// start angle and its sweep, in degrees. The arc turns twice per cycle and
/// grows and shrinks between 30 and 270 degrees.
pub fn indeterminate_arc(t: f32) -> (f32, f32) {
    let t = t.clamp(0.0, 1.0);
    let grow = (t * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    ((t * 720.0) % 360.0, 30.0 + grow * 240.0)
}

const CYCLE: Duration = Duration::from_millis(2000);

/// `ProgressBar`: as wide as its container.
#[derive(IntoElement)]
pub struct ProgressBar {
    id: ElementId,
    progress: Progress,
    status: ProgressStatus,
    frozen: Option<f32>,
}

impl ProgressBar {
    pub fn new(id: impl Into<ElementId>, progress: Progress) -> Self {
        Self {
            id: id.into(),
            progress,
            status: ProgressStatus::Running,
            frozen: None,
        }
    }

    pub fn status(mut self, status: ProgressStatus) -> Self {
        self.status = status;
        self
    }

    /// Draws the indeterminate segment at this point of its cycle instead
    /// of moving, for a screenshot.
    pub fn frozen(mut self, t: f32) -> Self {
        self.frozen = Some(t);
        self
    }
}

fn bar_frame(p: &Palette) -> gpui::Div {
    div().relative().w_full().h(px(3.0)).child(
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(1.0))
            .h(px(1.0))
            .rounded(px(0.5))
            .bg(p.strong_stroke),
    )
}

fn segment(left: f32, width: f32, color: Rgba) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .left(relative(left))
        .w(relative(width))
        .h(px(3.0))
        .rounded(px(1.5))
        .bg(color)
}

impl RenderOnce for ProgressBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let color = bar_color(self.status, p);
        match (self.progress, self.frozen) {
            (Progress::Determinate(value), _) => bar_frame(p)
                .child(segment(0.0, value.clamp(0.0, 1.0), color))
                .into_any_element(),
            (Progress::Indeterminate, Some(t)) => {
                let (left, width) = indeterminate_segment(t);
                bar_frame(p)
                    .child(segment(left, width, color))
                    .into_any_element()
            }
            (Progress::Indeterminate, None) => bar_frame(p)
                .with_animation(self.id, Animation::new(CYCLE).repeat(), move |frame, t| {
                    let (left, width) = indeterminate_segment(t);
                    frame.child(segment(left, width, color))
                })
                .into_any_element(),
        }
    }
}

/// `ProgressRing`.
#[derive(IntoElement)]
pub struct ProgressRing {
    id: ElementId,
    progress: Progress,
    size: Pixels,
    frozen: Option<f32>,
}

impl ProgressRing {
    pub fn new(id: impl Into<ElementId>, progress: Progress) -> Self {
        Self {
            id: id.into(),
            progress,
            size: px(32.0),
            frozen: None,
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Draws the indeterminate arc at this point of its cycle instead of
    /// moving, for a screenshot.
    pub fn frozen(mut self, t: f32) -> Self {
        self.frozen = Some(t);
        self
    }
}

/// A stroked arc as a polyline, `start` and `sweep` in degrees clockwise
/// from twelve o'clock.
fn arc(
    center: Point<Pixels>,
    radius: Pixels,
    start: f32,
    sweep: f32,
    stroke: Pixels,
    color: Rgba,
    window: &mut Window,
) {
    let steps = ((sweep.abs() / 6.0).ceil() as usize).max(2);
    let mut builder = PathBuilder::stroke(stroke);
    for step in 0..=steps {
        let angle = (start + sweep * step as f32 / steps as f32 - 90.0).to_radians();
        let at = point(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        if step == 0 {
            builder.move_to(at);
        } else {
            builder.line_to(at);
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

fn ring(
    size: Pixels,
    start: f32,
    sweep: f32,
    track: Option<Rgba>,
    color: Rgba,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let stroke = px(4.0) * (f32::from(size) / 32.0).max(0.5);
            let radius = (bounds.size.width.min(bounds.size.height) - stroke) / 2.0;
            let center = bounds.center();
            if let Some(track) = track {
                arc(center, radius, 0.0, 359.9, stroke, track, window);
            }
            if sweep > 0.0 {
                arc(center, radius, start, sweep, stroke, color, window);
            }
        },
    )
    .size(size)
}

impl RenderOnce for ProgressRing {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let size = self.size;
        let color = p.accent;
        match (self.progress, self.frozen) {
            (Progress::Determinate(value), _) => div()
                .size(size)
                .child(ring(size, 0.0, value.clamp(0.0, 1.0) * 360.0, None, color))
                .into_any_element(),
            (Progress::Indeterminate, Some(t)) => {
                let (start, sweep) = indeterminate_arc(t);
                div()
                    .size(size)
                    .child(ring(size, start, sweep, None, color))
                    .into_any_element()
            }
            (Progress::Indeterminate, None) => div()
                .size(size)
                .with_animation(self.id, Animation::new(CYCLE).repeat(), move |frame, t| {
                    let (start, sweep) = indeterminate_arc(t);
                    frame.child(ring(size, start, sweep, None, color))
                })
                .into_any_element(),
        }
    }
}
