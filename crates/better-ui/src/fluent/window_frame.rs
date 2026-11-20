//! The window frame: corners, edges, shadow, and resizing, as the desktop
//! draws them (ticket 108).
//!
//! [`root`] wraps a window's view in [`FramedView`], which paints the frame GTK
//! drew (see [`crate::system_frame`]) around the content, gives the compositor
//! the inset the shadow needs, takes input only where GTK does, and starts a
//! resize from every edge and corner. The title bar's header and buttons are
//! painted by [`super::chrome::TitleBar`] from the same drawing, so the two
//! always agree. When GTK cannot be loaded both fall back to Adwaita's look.
//!
//! Every corner is rounded with the theme's radius while the window floats
//! (ticket 141), and square while it is maximized, fullscreen, or tiled. GPUI
//! clips content to rectangles only, so the window's shape is published
//! ([`window_shape`]) for the containers that reach a corner: [`CornerFill`]
//! paints a fill (and its border and shadow) with the corner rounded, and
//! [`corner_clip`] paints contents that may scroll or float into a corner
//! inside a rectangle that stays clear of the curve.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Bounds, BoxShadow, ContentMask, Context, Corners,
    CursorStyle, Decorations, Edges, Element, ElementId, Global, GlobalElementId, Hsla,
    InspectorElementId, InteractiveElement, IntoElement, LayoutId, MouseButton, ParentElement,
    Pixels, Render, RenderImage, RenderOnce, ResizeEdge, Rgba, Styled, Task, Window, WindowId,
    canvas, div, point, px, size,
};
use gpui_component::{ActiveTheme as _, Root};

use crate::system_frame::service::{FrameService, Rendered};
use crate::system_frame::{
    self, Bitmap, Color, CornerRadii, Cut, DesktopSettings, Edge, FallbackLook, Insets, Rect,
    Tiled, WindowShape, WindowState,
};

/// How long a window's first frame waits for the desktop's settings and
/// GTK's drawing, together, before painting the fallback instead.
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(1000);

/// The root view of a framed window: `cx.new(|cx| root(view, window, cx))`
/// where the window would otherwise use `gpui_component::Root::new`.
pub fn root(view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Root>) -> Root {
    let view = view.into();
    let framed = cx.new(|cx| FramedView::new(view, window, cx));
    Root::new(framed, window, cx)
        .bordered(false)
        .bg(gpui::transparent_black())
}

/// Repaints `cx`'s view whenever the desktop's settings or the frame
/// drawings change. The task ends with the view.
pub fn watch_desktop<V: 'static>(cx: &mut Context<V>) -> Task<()> {
    let changes = FrameService::global().subscribe();
    cx.spawn(async move |this, cx| {
        while changes.recv().await.is_ok() {
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }
    })
}

/// Wraps the window's own view in the frame.
pub struct FramedView {
    view: AnyView,
    /// The first frame's deadline, shared by every wait it makes.
    first: Option<Instant>,
    /// The inset asked for so far; it only grows (see
    /// [`system_frame::sticky_inset`]).
    inset: Option<f32>,
    /// The input region last given to the compositor.
    input: Option<system_frame::InputRegion>,
    /// X11 cannot narrow a window's input region.
    x11: bool,
    _watch: Task<()>,
}

impl FramedView {
    pub fn new(view: AnyView, _: &mut Window, cx: &mut Context<Self>) -> Self {
        FrameService::warm_up();
        FramedView {
            view,
            first: None,
            inset: None,
            input: None,
            x11: std::env::var_os("WAYLAND_DISPLAY").is_none(),
            _watch: watch_desktop(cx),
        }
    }
}

/// The frame as it is drawn right now for a window.
#[derive(Clone)]
pub enum FrameLook {
    /// GTK's drawing, with the GPUI images cut from it.
    Gtk(Arc<Rendered>, Arc<FrameImages>),
    /// GTK is unavailable; Adwaita's look, drawn here.
    Fallback(&'static FallbackLook),
}

/// The images GPUI paints, cut once from a drawing.
pub struct FrameImages {
    /// Per frame state, `[focused, backdrop]`: top-left, top, top-right,
    /// right, bottom-right, bottom, bottom-left, left.
    pub frame: [[Arc<RenderImage>; 8]; 2],
    /// Header left corner, middle column, right corner; `[focused, backdrop]`.
    pub header: [[Arc<RenderImage>; 3]; 2],
    /// `(button, restore)` → `[state][focused, backdrop]`.
    pub buttons: HashMap<(system_frame::WindowButton, bool), [[Arc<RenderImage>; 2]; 3]>,
}

impl FrameImages {
    /// Every image, to take them out of the GPU's atlas when the drawing
    /// they were cut from is gone.
    pub fn all(&self) -> Vec<Arc<RenderImage>> {
        let mut all: Vec<Arc<RenderImage>> = self.frame.iter().flatten().cloned().collect();
        all.extend(self.header.iter().flatten().cloned());
        all.extend(self.buttons.values().flatten().flatten().cloned());
        all
    }
}

fn image(bitmap: &Bitmap) -> Arc<RenderImage> {
    let mut bgra = bitmap.rgba.clone();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(bitmap.width.max(1), bitmap.height.max(1), bgra)
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// Cuts a drawing into the pieces GPUI paints: the frame's four corners
/// (each `2 * frame` square around the window's corner) and four one-pixel
/// edges made into strips, the header's two corners and middle strip, and
/// every button.
#[cfg(target_os = "linux")]
pub fn cut(rendered: &Rendered) -> FrameImages {
    let f = rendered.frame_px;
    let frame = |bitmap: &Bitmap| -> [Arc<RenderImage>; 8] {
        let s = bitmap.width;
        let n = system_frame::STRIP_TEXELS;
        [
            image(&bitmap.crop(0, 0, 2 * f, 2 * f)),
            image(&bitmap.crop(2 * f, 0, 1, f).repeat(n, true)),
            image(&bitmap.crop(s - 2 * f, 0, 2 * f, 2 * f)),
            image(&bitmap.crop(s - f, 2 * f, f, 1).repeat(n, false)),
            image(&bitmap.crop(s - 2 * f, s - 2 * f, 2 * f, 2 * f)),
            image(&bitmap.crop(2 * f, s - f, 1, f).repeat(n, true)),
            image(&bitmap.crop(0, s - 2 * f, 2 * f, 2 * f)),
            image(&bitmap.crop(0, 2 * f, f, 1).repeat(n, false)),
        ]
    };
    let scale = rendered.scale;
    let r = rendered.corner_radius as u32 * scale;
    let header = |bitmap: &Bitmap| -> [Arc<RenderImage>; 3] {
        let h = bitmap.height;
        [
            image(&bitmap.crop(0, 0, r.max(1), h)),
            image(
                &bitmap
                    .crop(r, 0, 1, h)
                    .repeat(system_frame::STRIP_TEXELS, true),
            ),
            image(&bitmap.crop(bitmap.width.saturating_sub(r.max(1)), 0, r.max(1), h)),
        ]
    };
    let buttons = rendered
        .buttons
        .iter()
        .map(|set| {
            (
                (set.button, set.restore),
                set.images.clone().map(|pair| pair.map(|b| image(&b))),
            )
        })
        .collect();
    FrameImages {
        frame: [frame(&rendered.frame[0]), frame(&rendered.frame[1])],
        header: [header(&rendered.header[0]), header(&rendered.header[1])],
        buttons,
    }
}

/// GPUI images cut from each drawing, kept while the drawing is.
#[derive(Default)]
struct ImageCache(Vec<(Weak<Rendered>, Arc<FrameImages>)>);

impl Global for ImageCache {}

/// The last GTK drawing each window painted, per appearance: shown while a
/// new one (maximized, tiled, another scale) is being drawn, rather than the
/// fallback, so the frame and the header's height never flicker.
#[derive(Default)]
struct LastLooks(HashMap<(WindowId, bool), (Arc<Rendered>, Arc<FrameImages>)>);

impl Global for LastLooks {}

/// Each framed window's shape as its frame last laid it out.
#[derive(Default)]
struct WindowShapes(HashMap<WindowId, WindowShape>);

impl Global for WindowShapes {}

/// The rounded shape of the window being drawn, in window coordinates, as
/// its frame laid it out this frame: `None` for a window with no frame of the
/// kit's (Better Launcher's overlay), and square corners while it is
/// maximized, fullscreen, or tiled.
pub fn window_shape(window: &Window, cx: &App) -> Option<WindowShape> {
    cx.try_global::<WindowShapes>()?
        .0
        .get(&window.window_handle().window_id())
        .copied()
}

/// What a window's frame state is right now.
pub fn window_state(window: &Window) -> WindowState {
    let tiling = match window.window_decorations() {
        Decorations::Server => None,
        Decorations::Client { tiling } => Some(Tiled {
            top: tiling.top,
            right: tiling.right,
            bottom: tiling.bottom,
            left: tiling.left,
        }),
    };
    WindowState::new(
        tiling,
        window.is_maximized(),
        window.is_fullscreen(),
        window.is_window_active(),
    )
}

/// The frame for this window, and the desktop settings it was chosen from.
/// `dark` overrides the window's appearance (the gallery shows both).
pub fn frame_look(
    window: &mut Window,
    cx: &mut App,
    dark: Option<bool>,
) -> (FrameLook, DesktopSettings) {
    let state = window_state(window);
    frame_look_for(window, cx, dark, None, state)
}

/// [`frame_look`] for a given window state, such as the restored window's
/// while it is maximized, waiting until `deadline` (a window's first frame)
/// for the settings and the drawing.
pub fn frame_look_for(
    window: &mut Window,
    cx: &mut App,
    dark: Option<bool>,
    deadline: Option<Instant>,
    state: WindowState,
) -> (FrameLook, DesktopSettings) {
    let service = FrameService::global();
    let settings = match deadline {
        Some(deadline) => service.settings_by(deadline),
        None => service.settings(),
    };
    let dark = dark.unwrap_or_else(|| cx.theme().is_dark());
    let key = state.key(&settings, dark, window.scale_factor());
    let rendered = match deadline {
        Some(deadline) => service.get_by(&key, deadline),
        None => service.get(&key),
    };
    let last = (window.window_handle().window_id(), dark);
    let look = match rendered {
        #[cfg(target_os = "linux")]
        Some(rendered) => {
            let images = images_for(&rendered, window, cx);
            follow_drawn_accent(Some(&rendered), cx);
            if state == window_state(window) {
                let looks = cx.default_global::<LastLooks>();
                looks.0.insert(last, (rendered.clone(), images.clone()));
            }
            FrameLook::Gtk(rendered, images)
        }
        #[cfg(not(target_os = "linux"))]
        Some(_) => FrameLook::Fallback(system_frame::fallback_look(dark)),
        None if service.gave_up() => {
            follow_drawn_accent(None, cx);
            FrameLook::Fallback(system_frame::fallback_look(dark))
        }
        None => match cx.default_global::<LastLooks>().0.get(&last) {
            Some((rendered, images)) => FrameLook::Gtk(rendered.clone(), images.clone()),
            None => FrameLook::Fallback(system_frame::fallback_look(dark)),
        },
    };
    forget_closed_windows(cx);
    (look, settings)
}

/// The images cut from `rendered`, cutting them on first use, and taking the
/// images of drawings that are gone out of every window's atlas.
#[cfg(target_os = "linux")]
fn images_for(rendered: &Arc<Rendered>, window: &mut Window, cx: &mut App) -> Arc<FrameImages> {
    let cache = cx.default_global::<ImageCache>();
    let mut gone = Vec::new();
    cache.0.retain(|(weak, images)| {
        let alive = weak.strong_count() > 0;
        if !alive {
            gone.push(images.clone());
        }
        alive
    });
    let found = cache
        .0
        .iter()
        .find(|(weak, _)| weak.upgrade().is_some_and(|r| Arc::ptr_eq(&r, rendered)))
        .map(|(_, images)| images.clone());
    let images = found.unwrap_or_else(|| {
        let images = Arc::new(cut(rendered));
        cache.0.push((Arc::downgrade(rendered), images.clone()));
        images
    });
    for set in gone {
        for image in set.all() {
            cx.drop_image(image, Some(window));
        }
    }
    images
}

/// Drops the last looks of windows that have closed, so their drawings and
/// images can go.
fn forget_closed_windows(cx: &mut App) {
    let open: Vec<WindowId> = cx.windows().iter().map(|w| w.window_id()).collect();
    if let Some(looks) = cx.try_global::<LastLooks>()
        && looks.0.keys().any(|(id, _)| !open.contains(id))
    {
        cx.default_global::<LastLooks>()
            .0
            .retain(|(id, _), _| open.contains(id));
    }
    if let Some(shapes) = cx.try_global::<WindowShapes>()
        && shapes.0.keys().any(|id| !open.contains(id))
    {
        cx.default_global::<WindowShapes>()
            .0
            .retain(|id, _| open.contains(id));
    }
}

/// Makes the Fluent accent the drawing's theme accent, or Windows 11's blue
/// when the theme has none or GTK is unavailable.
fn follow_drawn_accent(rendered: Option<&Rendered>, cx: &mut App) {
    #[cfg(target_os = "linux")]
    let accent = rendered
        .and_then(|r| r.accent)
        .map(|(bg, fg)| (rgba(bg), rgba(fg)));
    #[cfg(not(target_os = "linux"))]
    let accent = {
        let _ = rendered;
        None
    };
    super::follow_system_accent(accent, cx);
}

/// Follows the desktop's accent in a window without a frame (Better
/// Launcher's overlay). Call it while rendering, with [`watch_desktop`]
/// repainting the view when the drawings change.
pub fn follow_desktop_accent(window: &mut Window, cx: &mut App) {
    let service = FrameService::global();
    let state = WindowState::new(Some(Tiled::default()), false, false, true);
    let key = state.key(
        &service.settings(),
        cx.theme().is_dark(),
        window.scale_factor(),
    );
    match service.get(&key) {
        Some(rendered) => follow_drawn_accent(Some(&rendered), cx),
        None if service.gave_up() => follow_drawn_accent(None, cx),
        None => {}
    }
}

pub fn rgba(color: Color) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

impl FrameLook {
    /// The room the window keeps past its edge for the shadow.
    pub fn shadow(&self) -> Insets {
        match self {
            FrameLook::Gtk(rendered, _) => rendered.thickness,
            FrameLook::Fallback(look) => look.shadow,
        }
    }

    /// The top corners' radius the theme gives a floating window.
    pub fn corner_radius(&self) -> f32 {
        match self {
            FrameLook::Gtk(rendered, _) => rendered.corner_radius,
            FrameLook::Fallback(look) => look.corner_radius,
        }
    }

    /// The bottom corners' radius, measured apart from the top.
    pub fn bottom_corner_radius(&self) -> f32 {
        match self {
            FrameLook::Gtk(rendered, _) => rendered.bottom_corner_radius,
            FrameLook::Fallback(look) => look.corner_radius,
        }
    }

    /// The four corners a window in `state` has with this look.
    pub fn corner_radii(&self, state: WindowState) -> CornerRadii {
        system_frame::window_corner_radii(state, self.corner_radius(), self.bottom_corner_radius())
    }
}

fn corners(radii: CornerRadii) -> Corners<Pixels> {
    Corners {
        top_left: px(radii.top_left),
        top_right: px(radii.top_right),
        bottom_right: px(radii.bottom_right),
        bottom_left: px(radii.bottom_left),
    }
}

fn rect(bounds: Bounds<Pixels>) -> Rect {
    Rect::new(
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    )
}

fn bounds_of(rect: Rect) -> Bounds<Pixels> {
    Bounds::new(
        point(px(rect.x), px(rect.y)),
        size(px(rect.width), px(rect.height)),
    )
}

fn gpui_edge(edge: Edge) -> ResizeEdge {
    match edge {
        Edge::Top => ResizeEdge::Top,
        Edge::TopRight => ResizeEdge::TopRight,
        Edge::Right => ResizeEdge::Right,
        Edge::BottomRight => ResizeEdge::BottomRight,
        Edge::Bottom => ResizeEdge::Bottom,
        Edge::BottomLeft => ResizeEdge::BottomLeft,
        Edge::Left => ResizeEdge::Left,
        Edge::TopLeft => ResizeEdge::TopLeft,
    }
}

fn cursor(edge: Edge) -> CursorStyle {
    match edge {
        Edge::Top | Edge::Bottom => CursorStyle::ResizeUpDown,
        Edge::Left | Edge::Right => CursorStyle::ResizeLeftRight,
        Edge::TopLeft | Edge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        Edge::TopRight | Edge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

impl FramedView {
    /// Gives the compositor `input` as the window's input region, when it
    /// differs from the one given last. `None` is the whole surface.
    fn set_input(&mut self, input: system_frame::InputRegion, window: &mut Window) {
        if !system_frame::input_region_changes(self.input.as_ref(), &input) {
            return;
        }
        match &input {
            Some(rects) => {
                let rects: Vec<Bounds<Pixels>> = rects
                    .iter()
                    .map(|(x, y, rw, rh)| {
                        Bounds::new(point(px(*x), px(*y)), size(px(*rw), px(*rh)))
                    })
                    .collect();
                window.set_input_region(Some(&rects))
            }
            None => window.set_input_region(None),
        }
        self.input = Some(input);
    }
}

impl Render for FramedView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // What `gpui_component::Root` painted before the frame took over: the
        // Fluent window background in a Fluent window, the library's own in an
        // older one. App roots no longer paint one.
        let background = cx.theme().tokens.background;
        let state = window_state(window);
        if !state.client_decorated {
            // A window that started client-decorated and is now drawn by the
            // compositor (sway agrees to server-side decorations on its first
            // configure) keeps whatever input region its first frame set:
            // that frame's smaller size, so every press outside it went to
            // nothing (ticket 85: the right half of Better Manager took no
            // click). The compositor's frame needs the whole surface.
            self.set_input(None, window);
            cx.default_global::<WindowShapes>()
                .0
                .remove(&window.window_handle().window_id());
            return div()
                .size_full()
                .bg(background)
                .child(self.view.clone())
                .into_any_element();
        }
        // One deadline for every wait the first frame makes.
        let deadline = match self.first {
            None => {
                let deadline = Instant::now() + FIRST_FRAME_WAIT;
                self.first = Some(deadline);
                Some(deadline)
            }
            Some(_) => None,
        };
        let (look, _) = frame_look_for(window, cx, None, deadline, state);
        // GPUI takes one inset for every side; the narrower sides of the
        // shadow leave transparent space. It is the restored window's even
        // while maximized or tiled (GPUI drops it for tiled sides), and it
        // never shrinks, so neither restoring nor GTK's drawing arriving
        // after the fallback changes the window's size.
        let shadow = if state.maximized || state.tiled.any() {
            frame_look_for(window, cx, None, deadline, state.restored())
                .0
                .shadow()
        } else {
            look.shadow()
        };
        let inset =
            system_frame::sticky_inset(self.inset, system_frame::client_inset(shadow), self.x11);
        if self.inset != Some(inset) {
            self.inset = Some(inset);
            window.set_client_inset(px(inset));
        }
        let insets = system_frame::frame_insets(inset, state.tiled, state.maximized);
        let surface = window.viewport_size();
        let (w, h) = (f32::from(surface.width), f32::from(surface.height));
        let radii = look.corner_radii(state);
        let input = (!(state.maximized || state.tiled.all()))
            .then(|| system_frame::input_region(w, h, insets, radii));
        self.set_input(input, window);
        let shape = WindowShape::new(Rect::inside(w, h, insets), radii);
        cx.default_global::<WindowShapes>()
            .0
            .insert(window.window_handle().window_id(), shape);
        let active = state.active;
        let paint_look = look.clone();
        let frame = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                paint_frame(&paint_look, bounds, insets, radii, active, state, window)
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let zones = system_frame::resize_zones(w, h, insets, state.tiled, state.maximized, radii)
            .into_iter()
            .map(|(edge, (x, y, zw, zh))| {
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(zw))
                    .h(px(zh))
                    .cursor(cursor(edge))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        let state = window_state(window);
                        let size = window.viewport_size();
                        let edge = system_frame::resize_edge(
                            f32::from(event.position.x),
                            f32::from(event.position.y),
                            f32::from(size.width),
                            f32::from(size.height),
                            insets,
                            state.tiled,
                            state.maximized,
                            radii,
                        );
                        if let Some(edge) = edge {
                            cx.stop_propagation();
                            window.start_window_resize(gpui_edge(edge));
                        }
                    })
            });
        div()
            .id("better-window-frame")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .pt(px(insets.top))
            .pr(px(insets.right))
            .pb(px(insets.bottom))
            .pl(px(insets.left))
            .child(frame)
            .child(
                div()
                    .id("better-window-content")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .bg(background)
                    .rounded_tl(px(radii.top_left))
                    .rounded_tr(px(radii.top_right))
                    .rounded_br(px(radii.bottom_right))
                    .rounded_bl(px(radii.bottom_left))
                    .cursor(CursorStyle::Arrow)
                    .child(self.view.clone()),
            )
            .children(zones)
            .into_any_element()
    }
}

/// Paints the drawn frame around the content box, as Chromium's
/// `PaintWindowFrame` does: four corners at their size, four edges stretched
/// along the window.
fn paint_frame(
    look: &FrameLook,
    bounds: Bounds<Pixels>,
    insets: Insets,
    radii: CornerRadii,
    active: bool,
    state: WindowState,
    window: &mut Window,
) {
    if state.maximized {
        return;
    }
    let left = bounds.origin.x + px(insets.left);
    let top = bounds.origin.y + px(insets.top);
    let right = bounds.origin.x + bounds.size.width - px(insets.right);
    let bottom = bounds.origin.y + bounds.size.height - px(insets.bottom);
    match look {
        FrameLook::Gtk(rendered, images) => {
            let f = px(rendered.frame_px as f32 / rendered.scale as f32);
            let set = &images.frame[if active { 0 } else { 1 }];
            let width = right - left;
            let height = bottom - top;
            let cw = f.min(width / 2.0);
            let ch = f.min(height / 2.0);
            // Each corner image is 2f square with the window's corner at its
            // middle; only the part outside the window and the first `cw` by
            // `ch` inside it is painted (the content covers the rest).
            let corners = [
                (
                    0usize,
                    left,
                    top,
                    Bounds::new(point(left - f, top - f), size(f + cw, f + ch)),
                ),
                (
                    2,
                    right,
                    top,
                    Bounds::new(point(right - cw, top - f), size(cw + f, f + ch)),
                ),
                (
                    4,
                    right,
                    bottom,
                    Bounds::new(point(right - cw, bottom - ch), size(cw + f, ch + f)),
                ),
                (
                    6,
                    left,
                    bottom,
                    Bounds::new(point(left - f, bottom - ch), size(f + cw, ch + f)),
                ),
            ];
            for (index, x, y, visible) in corners {
                let image_bounds = Bounds::new(point(x - f, y - f), size(f * 2.0, f * 2.0));
                let _ = window.paint_image(
                    visible,
                    image_bounds,
                    Corners::default(),
                    set[index].clone(),
                    0,
                    false,
                );
            }
            let edge_w = width - cw * 2.0;
            let edge_h = height - ch * 2.0;
            let edges = [
                (1usize, left + cw, top - f, edge_w, f, true),
                (5, left + cw, bottom, edge_w, f, true),
                (7, left - f, top + ch, f, edge_h, false),
                (3, right, top + ch, f, edge_h, false),
            ];
            for (index, x, y, w, h, along_x) in edges {
                paint_strip(
                    window,
                    &set[index],
                    Bounds::new(point(x, y), size(w, h)),
                    along_x,
                );
            }
        }
        FrameLook::Fallback(look) => {
            // Adwaita's window shadow, border, and 12px corners. GPUI does
            // not grow paint bounds for blur, so the shadow stays inside the
            // inset.
            let opacity = if active { 1.0 } else { 0.6 };
            let shadows = [
                (2.0, 8.0, 2.0, 0.13),
                (3.0, 20.0, 6.0, 0.09),
                (6.0, 28.0, 8.0, 0.04),
            ];
            let content = Bounds::new(point(left, top), size(right - left, bottom - top));
            let corners = corners(radii);
            let list: Vec<gpui::BoxShadow> = shadows
                .iter()
                .map(|(y, blur, spread, alpha)| gpui::BoxShadow {
                    color: gpui::hsla(0.0, 0.0, 0.0, alpha * opacity),
                    offset: point(px(0.0), px(*y)),
                    blur_radius: px(*blur),
                    spread_radius: px(*spread),
                    inset: false,
                })
                .collect();
            window.paint_drop_shadows(content, corners, &list);
            let border = rgba(look.border);
            window.paint_quad(gpui::quad(
                Bounds::new(
                    point(left - px(1.0), top - px(1.0)),
                    size(right - left + px(2.0), bottom - top + px(2.0)),
                ),
                self::corners(radii.grow(1.0)),
                gpui::transparent_black(),
                px(1.0),
                border,
                gpui::BorderStyle::Solid,
            ));
        }
    }
}

/// Paints a strip image (see [`system_frame::STRIP_TEXELS`]) stretched over
/// `bounds`, sampling only its middle so nothing beside it in the texture
/// atlas bleeds in.
pub fn paint_strip(
    window: &mut Window,
    image: &Arc<RenderImage>,
    bounds: Bounds<Pixels>,
    along_x: bool,
) {
    if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
        return;
    }
    let image_bounds = if along_x {
        let (start, length) = system_frame::strip_image_span(
            f32::from(bounds.origin.x),
            f32::from(bounds.size.width),
        );
        Bounds::new(
            point(px(start), bounds.origin.y),
            size(px(length), bounds.size.height),
        )
    } else {
        let (start, length) = system_frame::strip_image_span(
            f32::from(bounds.origin.y),
            f32::from(bounds.size.height),
        );
        Bounds::new(
            point(bounds.origin.x, px(start)),
            size(bounds.size.width, px(length)),
        )
    };
    let _ = window.paint_image(
        bounds,
        image_bounds,
        Corners::default(),
        image.clone(),
        0,
        false,
    );
}

/// A fill that stays inside the window's rounded corners (ticket 141): a
/// container that reaches a window corner draws its background, border, and
/// shadow with this instead of `.bg()`, `.border_*()`, and `.shadow_*()`,
/// because a GPUI quad is square wherever its own style says so. The corner
/// that meets a rounded window corner gets the radius that keeps it inside
/// ([`WindowShape::fill_radii`]); the rest keep their own. Put it first in a
/// `relative()` element; it covers the element.
#[derive(IntoElement)]
pub struct CornerFill {
    color: Hsla,
    base: CornerRadii,
    border: Option<(Edges<Pixels>, Hsla)>,
    shadows: Vec<BoxShadow>,
}

/// A [`CornerFill`] of `color`.
pub fn corner_fill(color: impl Into<Hsla>) -> CornerFill {
    CornerFill {
        color: color.into(),
        base: CornerRadii::SQUARE,
        border: None,
        shadows: Vec::new(),
    }
}

impl CornerFill {
    /// The element's own corner radii, kept wherever they already stay
    /// inside the window.
    pub fn rounded(mut self, radii: CornerRadii) -> Self {
        self.base = radii;
        self
    }

    /// A border, drawn with the fill's corners.
    pub fn border(mut self, widths: Edges<Pixels>, color: impl Into<Hsla>) -> Self {
        self.border = Some((widths, color.into()));
        self
    }

    /// A drop shadow, drawn with the fill's corners and kept inside the
    /// window's curve.
    pub fn shadow(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.shadows = shadows;
        self
    }
}

impl RenderOnce for CornerFill {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let shape = window_shape(window, cx);
                let radii = shape
                    .map(|shape| shape.fill_radii(rect(bounds), self.base))
                    .unwrap_or(self.base);
                if !self.shadows.is_empty() {
                    // A shadow is drawn under the fill too, so it would show
                    // past the fill's rounded corner; it is kept to the part
                    // of the window clear of every curve.
                    let mask = shape.map(|shape| ContentMask {
                        bounds: bounds_of(shape.clip(
                            shape.rect,
                            Cut::Inset,
                            window.scale_factor(),
                        )),
                    });
                    window.with_content_mask(mask, |window| {
                        window.paint_drop_shadows(bounds, corners(radii), &self.shadows)
                    });
                }
                let (widths, border) = self
                    .border
                    .unwrap_or((Edges::default(), gpui::transparent_black()));
                window.paint_quad(gpui::quad(
                    bounds,
                    corners(radii),
                    self.color,
                    widths,
                    border,
                    gpui::BorderStyle::Solid,
                ));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

/// Paints `child` inside the part of its bounds clear of the window's rounded
/// corners (ticket 141), for contents that can scroll or float into a corner:
/// a list's rows, a bar's contents, toasts. `cut` says which strip along the
/// window's edge is given up where the child reaches a curve
/// ([`WindowShape::clip`]); nothing is given up away from the corners, or
/// while the window's corners are square. Hit testing follows the same
/// rectangle, so a press outside the curve never reaches the child.
pub fn corner_clip(cut: Cut, child: impl IntoElement) -> CornerClip {
    CornerClip {
        cut,
        child: child.into_any_element(),
    }
}

/// A panel that reaches a window corner, such as a sidebar: `fill` under it,
/// its contents in [`corner_clip`]. Style the returned element as the panel's
/// place in the layout needs (its size follows `child`).
pub fn corner_panel(fill: CornerFill, cut: Cut, child: impl IntoElement) -> gpui::Div {
    div().relative().child(fill).child(corner_clip(cut, child))
}

/// See [`corner_clip`].
pub struct CornerClip {
    cut: Cut,
    child: AnyElement,
}

impl IntoElement for CornerClip {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl CornerClip {
    fn mask(
        &self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &App,
    ) -> Option<ContentMask<Pixels>> {
        let shape = window_shape(window, cx)?;
        if shape.radii.is_square() {
            return None;
        }
        // What of the child is visible at all: a scrolled list's rows are
        // clipped by their viewport before they reach the window's edge.
        let visible = rect(bounds).intersect(&rect(window.content_mask().bounds));
        Some(ContentMask {
            bounds: bounds_of(shape.clip(visible, self.cut, window.scale_factor())),
        })
    }
}

impl Element for CornerClip {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let mask = self.mask(bounds, window, cx);
        window.with_content_mask(mask, |window| {
            self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = self.mask(bounds, window, cx);
        window.with_content_mask(mask, |window| self.child.paint(window, cx));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{FrameLook, cut};
    use crate::system_frame::service::Rendered;
    use crate::system_frame::{CornerRadii, DesktopSettings, Tiled, WindowState, fallback_look};
    use std::sync::Arc;

    /// Ticket 141: all four corners take the radius measured from GTK's
    /// drawing (or the fallback's) while the window floats, and every corner
    /// is square maximized, fullscreen, or tiled on any edge.
    #[test]
    fn every_corner_takes_the_measured_radius_and_is_square_when_not_floating() {
        let floating = WindowState::new(Some(Tiled::default()), false, false, true);
        let key = floating.key(&DesktopSettings::default(), true, 1.0);
        let mut rendered = Rendered::placeholder(key);
        rendered.corner_radius = 15.0;
        rendered.bottom_corner_radius = 15.0;
        let images = Arc::new(cut(&rendered));
        let gtk = FrameLook::Gtk(Arc::new(rendered), images);
        let fallback = FrameLook::Fallback(fallback_look(true));
        assert_eq!(gtk.corner_radii(floating), CornerRadii::uniform(15.0));
        assert_eq!(fallback.corner_radii(floating), CornerRadii::uniform(12.0));
        let left = Tiled {
            left: true,
            ..Tiled::default()
        };
        let bottom = Tiled {
            bottom: true,
            ..Tiled::default()
        };
        for state in [
            WindowState::new(Some(Tiled::default()), true, false, true),
            WindowState::new(Some(Tiled::default()), false, true, true),
            WindowState::new(Some(left), false, false, true),
            WindowState::new(Some(bottom), false, false, true),
            WindowState::new(Some(Tiled::ALL), false, false, true),
            WindowState::new(None, false, false, true),
        ] {
            assert!(gtk.corner_radii(state).is_square(), "{state:?}");
            assert!(fallback.corner_radii(state).is_square(), "{state:?}");
        }
    }

    fn size(image: &gpui::RenderImage) -> (i32, i32) {
        let size = image.size(0);
        (size.width.0, size.height.0)
    }

    #[test]
    fn a_drawing_is_cut_into_corners_strips_and_buttons() {
        for scale in [1.0, 2.0] {
            let key = WindowState::new(Some(Tiled::default()), false, false, true).key(
                &DesktopSettings::default(),
                false,
                scale,
            );
            let rendered = Rendered::placeholder(key);
            let f = rendered.frame_px as i32;
            let n = crate::system_frame::STRIP_TEXELS as i32;
            let images = cut(&rendered);
            for set in &images.frame {
                for corner in [0, 2, 4, 6] {
                    assert_eq!(size(&set[corner]), (2 * f, 2 * f));
                }
                assert_eq!(size(&set[1]), (n, f), "top edge");
                assert_eq!(size(&set[5]), (n, f), "bottom edge");
                assert_eq!(size(&set[3]), (f, n), "right edge");
                assert_eq!(size(&set[7]), (f, n), "left edge");
            }
            let r = rendered.corner_radius as i32 * rendered.scale as i32;
            let h = rendered.header[0].height as i32;
            for set in &images.header {
                assert_eq!(size(&set[0]), (r, h));
                assert_eq!(size(&set[1]), (n, h));
                assert_eq!(size(&set[2]), (r, h));
            }
            assert_eq!(images.buttons.len(), 4);
            assert_eq!(images.all().len(), 16 + 6 + 4 * 6);
        }
    }
}
