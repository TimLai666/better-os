//! Better OS's session surfaces (the top bar, the taskbar, the launcher, the
//! switcher, the overview, notifications, the desktop) are GPUI windows opened
//! as `zwlr_layer_shell_v1` surfaces (engineering/specs/better-os-session.md,
//! ticket 200). This module is how every one of them does it:
//!
//! - [`LayerSurfaces`] keeps one window per output, or one window, following
//!   outputs as they come and go and recreating a window when its spec
//!   changes in a way GPUI cannot apply to an open surface.
//! - [`apply_input_region`] keeps a surface's input to what it draws.
//! - [`clock::Motion`] and [`clock::visible_part`] move a panel inside a
//!   fixed surface.
//!
//! The rules are in [`rules`] and [`clock`], which have no GPUI in them.
//!
//! Known limit at the pinned GPUI (`ae394f3`, ticket 200): a surface with an
//! [`Extent::Fill`] dimension is killed by a `wp_viewport` protocol error
//! when the compositor sends its fractional scale before its first configure,
//! which `cosmic-comp` does on every output above 100 %. Fixed-size surfaces
//! are not affected. The fix is the owner's decision (ticket 200's notes);
//! nothing here works around it.
//! Layer-shell exists only on a compositor that offers it: on GNOME, opening
//! one fails with GPUI's `LayerShellNotSupportedError`, which
//! [`LayerSurfaces::last_error`] reports.

pub mod clock;
pub mod rules;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;

use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer as GpuiLayer, LayerShellOptions};
use gpui::{
    App, AppContext as _, Bounds, Context, DisplayId, QuitMode, Render, Size, Subscription, Task,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowId, WindowKind,
    WindowOptions, point, px,
};

pub use rules::{
    Action, Change, Edges, ExclusiveZone, Extent, InPlace, InputRegion, Keyboard, Layer, Margin,
    OutputInfo, Placement, Recreate, Rect, SpecError, SurfaceSet, SurfaceSpec, change,
    input_region,
};

/// How often [`LayerSurfaces`] looks at the output list. GPUI has no event
/// for an output added (it fills `App::displays` as `wl_output`s arrive), so
/// a new output is noticed within this long.
pub const OUTPUT_POLL: Duration = Duration::from_millis(500);

/// GPUI's layer-shell options for a spec.
pub fn layer_shell_options(spec: &SurfaceSpec) -> LayerShellOptions {
    let margin = spec.margin;
    LayerShellOptions {
        namespace: spec.namespace.clone(),
        layer: match spec.layer {
            Layer::Background => GpuiLayer::Background,
            Layer::Bottom => GpuiLayer::Bottom,
            Layer::Top => GpuiLayer::Top,
            Layer::Overlay => GpuiLayer::Overlay,
        },
        anchor: Anchor::from_bits_truncate(u32::from(spec.anchor.bits())),
        exclusive_zone: Some(px(spec.exclusive_zone.protocol_value())),
        exclusive_edge: spec
            .exclusive_edge
            .map(|edge| Anchor::from_bits_truncate(u32::from(edge.bits()))),
        margin: Some((
            px(margin.top),
            px(margin.right),
            px(margin.bottom),
            px(margin.left),
        )),
        keyboard_interactivity: match spec.keyboard {
            Keyboard::None => KeyboardInteractivity::None,
            Keyboard::OnDemand => KeyboardInteractivity::OnDemand,
            Keyboard::Exclusive => KeyboardInteractivity::Exclusive,
        },
    }
}

/// The window options that open `spec` on `display` (or wherever the
/// compositor puts it, with `None`). A layer surface has no title bar and a
/// transparent background, so only what the view paints is seen.
pub fn window_options(
    spec: &SurfaceSpec,
    display: Option<DisplayId>,
    app_id: impl Into<String>,
) -> WindowOptions {
    let (width, height) = spec.requested_size();
    WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: Size::new(px(width), px(height)),
        })),
        focus: spec.keyboard != Keyboard::None,
        show: true,
        kind: WindowKind::LayerShell(layer_shell_options(spec)),
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: display,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(app_id.into()),
        window_decorations: None,
        ..Default::default()
    }
}

/// Sets a window's input region from the rule's answer.
pub fn apply_input_region(window: &Window, region: &InputRegion) {
    match region {
        InputRegion::Whole => window.set_input_region(None),
        InputRegion::Nothing => window.set_input_region(Some(&[])),
        InputRegion::Rects(rects) => {
            let bounds: Vec<Bounds<gpui::Pixels>> = rects
                .iter()
                .map(|rect| Bounds {
                    origin: point(px(rect.x), px(rect.y)),
                    size: Size::new(px(rect.width), px(rect.height)),
                })
                .collect();
            window.set_input_region(Some(&bounds));
        }
    }
}

/// Sends the in-place requests a changed spec needs to an open surface.
pub fn apply_in_place(window: &mut Window, spec: &SurfaceSpec, changes: &[InPlace]) {
    for change in changes {
        match change {
            InPlace::ExclusiveZone => {
                window.set_exclusive_zone(px(spec.exclusive_zone.protocol_value()));
            }
            InPlace::ExclusiveEdge => {
                // GPUI has no request to clear an exclusive edge; a spec that
                // drops it falls back to the edge the anchor implies, which
                // is the same edge whenever validation passed.
                if let Some(edge) = spec.reserving_edge() {
                    window.set_exclusive_edge(Anchor::from_bits_truncate(u32::from(edge.bits())));
                }
            }
            InPlace::Size => {
                let (width, height) = spec.requested_size();
                window.resize(Size::new(px(width), px(height)));
            }
        }
    }
}

/// The outputs as GPUI lists them now.
pub fn outputs(cx: &App) -> Vec<OutputInfo> {
    let mut outputs: Vec<OutputInfo> = cx
        .displays()
        .iter()
        .map(|display| OutputInfo {
            id: u64::from(display.id()),
            key: display.uuid().ok().map(|uuid| uuid.to_string()),
            // GPUI exposes no scale for a display.
            scale: None,
        })
        .collect();
    outputs.sort_by_key(|output| output.id);
    outputs
}

type Build<V> = Rc<dyn Fn(&OutputInfo, &mut Window, &mut Context<V>) -> V>;

struct Inner<V: 'static> {
    app_id: String,
    spec: SurfaceSpec,
    placement: Placement,
    set: SurfaceSet,
    windows: BTreeMap<u64, WindowHandle<V>>,
    /// Windows Better OS closed itself, so their close is not read as the
    /// compositor taking the output away.
    closing: BTreeSet<WindowId>,
    build: Build<V>,
    last_error: Option<String>,
}

/// The surfaces of one role: one window per output ([`Placement::EveryOutput`])
/// or one ([`Placement::One`]), kept in step with the outputs and the spec.
/// Dropping it stops following outputs; the windows stay open until closed.
pub struct LayerSurfaces<V: 'static> {
    inner: Rc<RefCell<Inner<V>>>,
    _closed: Subscription,
    _poll: Task<()>,
}

impl<V: Render + 'static> LayerSurfaces<V> {
    /// Opens the surfaces `spec` and `placement` call for, and keeps them in
    /// step with the outputs. `build` makes the view for each output's window.
    /// It sets the application's quit mode to [`QuitMode::Explicit`]: the
    /// process ends when it calls `App::quit`, not when its last surface
    /// closes.
    pub fn open(
        app_id: impl Into<String>,
        spec: SurfaceSpec,
        placement: Placement,
        build: impl Fn(&OutputInfo, &mut Window, &mut Context<V>) -> V + 'static,
        cx: &mut App,
    ) -> Result<LayerSurfaces<V>, SpecError> {
        spec.validate()?;
        // A session surface outlives its windows: with every output gone (a
        // laptop lid closed on a docked machine) it has none, and GPUI's
        // default on Linux quits when the last window closes (the check
        // program did exactly that when a recreation closed its only one).
        cx.set_quit_mode(QuitMode::Explicit);
        let inner = Rc::new(RefCell::new(Inner {
            app_id: app_id.into(),
            spec,
            placement,
            set: SurfaceSet::new(),
            windows: BTreeMap::new(),
            closing: BTreeSet::new(),
            build: Rc::new(build),
            last_error: None,
        }));

        let weak = Rc::downgrade(&inner);
        let closed = cx.on_window_closed(move |_cx, window_id| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(mut inner) = inner.try_borrow_mut() else {
                return;
            };
            if inner.closing.remove(&window_id) {
                return;
            }
            let output = inner
                .windows
                .iter()
                .find(|(_, handle)| handle.window_id() == window_id)
                .map(|(output, _)| *output);
            if let Some(output) = output {
                inner.windows.remove(&output);
                inner.set.closed_by_compositor(output);
            }
        });

        let weak = Rc::downgrade(&inner);
        let poll = cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(OUTPUT_POLL).await;
                let Some(inner) = weak.upgrade() else { break };
                cx.update(|cx| sync(&inner, cx));
            }
        });

        sync(&inner, cx);
        Ok(LayerSurfaces {
            inner,
            _closed: closed,
            _poll: poll,
        })
    }

    /// Changes the spec of every surface: in place where GPUI can, by a new
    /// surface where it cannot (anchor, margin, layer, keyboard, namespace).
    pub fn set_spec(&self, spec: SurfaceSpec, cx: &mut App) -> Result<(), SpecError> {
        spec.validate()?;
        self.inner.borrow_mut().spec = spec;
        sync(&self.inner, cx);
        Ok(())
    }

    /// Changes which outputs get a surface.
    pub fn set_placement(&self, placement: Placement, cx: &mut App) {
        self.inner.borrow_mut().placement = placement;
        sync(&self.inner, cx);
    }

    /// Looks at the outputs now rather than at the next poll.
    pub fn refresh(&self, cx: &mut App) {
        sync(&self.inner, cx);
    }

    /// The open windows, by output id.
    pub fn windows(&self) -> Vec<(u64, WindowHandle<V>)> {
        self.inner
            .borrow()
            .windows
            .iter()
            .map(|(output, handle)| (*output, *handle))
            .collect()
    }

    /// The spec the surfaces are kept at.
    pub fn spec(&self) -> SurfaceSpec {
        self.inner.borrow().spec.clone()
    }

    /// Why the last surface could not be opened, if one could not: on GNOME,
    /// GPUI's "Compositor doesn't support zwlr_layer_shell_v1".
    pub fn last_error(&self) -> Option<String> {
        self.inner.borrow().last_error.clone()
    }

    /// Closes every surface.
    pub fn close_all(&self, cx: &mut App) {
        let windows: Vec<(u64, WindowHandle<V>)> = self.windows();
        for (output, handle) in windows {
            close_window(&self.inner, output, handle, cx);
        }
    }
}

fn close_window<V: 'static>(
    inner: &Rc<RefCell<Inner<V>>>,
    output: u64,
    handle: WindowHandle<V>,
    cx: &mut App,
) {
    {
        let mut inner = inner.borrow_mut();
        inner.closing.insert(handle.window_id());
        inner.windows.remove(&output);
        inner.set.closed(output);
    }
    let _ = handle.update(cx, |_, window, _| window.remove_window());
}

fn sync<V: Render + 'static>(inner: &Rc<RefCell<Inner<V>>>, cx: &mut App) {
    let outputs = outputs(cx);
    let (actions, spec) = {
        let Ok(state) = inner.try_borrow() else {
            return;
        };
        (
            state.set.plan(&outputs, &state.placement, &state.spec),
            state.spec.clone(),
        )
    };
    for action in actions {
        match action {
            Action::Open { output } => open_on(inner, output, &outputs, &spec, cx),
            Action::Close { output } => {
                let handle = inner.borrow().windows.get(&output).copied();
                match handle {
                    Some(handle) => close_window(inner, output, handle, cx),
                    None => inner.borrow_mut().set.closed(output),
                }
            }
            Action::Recreate { output, .. } => {
                // The new surface first, then the old one goes: with the old
                // one closed first the bar blinks out for the second a new
                // GPUI window takes to show (seen on cosmic-comp, ticket 200).
                let old = inner.borrow().windows.get(&output).copied();
                open_on(inner, output, &outputs, &spec, cx);
                if let Some(old) = old {
                    inner.borrow_mut().closing.insert(old.window_id());
                    let _ = old.update(cx, |_, window, _| window.remove_window());
                }
            }
            Action::Update { output, changes } => {
                let handle = inner.borrow().windows.get(&output).copied();
                if let Some(handle) = handle {
                    let applied = handle
                        .update(cx, |_, window, _| apply_in_place(window, &spec, &changes))
                        .is_ok();
                    if applied {
                        inner.borrow_mut().set.opened(output, spec.clone());
                    }
                }
            }
        }
    }
}

fn open_on<V: Render + 'static>(
    inner: &Rc<RefCell<Inner<V>>>,
    output: u64,
    outputs: &[OutputInfo],
    spec: &SurfaceSpec,
    cx: &mut App,
) {
    let Some(info) = outputs.iter().find(|info| info.id == output).cloned() else {
        return;
    };
    let (app_id, build) = {
        let inner = inner.borrow();
        (inner.app_id.clone(), inner.build.clone())
    };
    let options = window_options(spec, Some(DisplayId::new(output)), app_id);
    match cx.open_window(options, |window, cx| cx.new(|cx| build(&info, window, cx))) {
        Ok(handle) => {
            let mut inner = inner.borrow_mut();
            inner.windows.insert(output, handle);
            inner.set.opened(output, spec.clone());
            inner.last_error = None;
        }
        Err(error) => {
            let mut inner = inner.borrow_mut();
            inner.last_error = Some(format!("{error:#}"));
            // Not retried every poll: a compositor without layer-shell would
            // refuse again. A new output or a new spec tries again.
            inner.set.closed_by_compositor(output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Edges, Keyboard, Layer, SurfaceSpec, layer_shell_options, window_options};
    use gpui::WindowKind;
    use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer as GpuiLayer};

    #[test]
    fn a_strip_becomes_gpui_options_with_the_protocol_values() {
        let spec = SurfaceSpec::strip("better-top-bar", Edges::TOP, 32.0);
        let options = layer_shell_options(&spec);
        assert_eq!(options.namespace, "better-top-bar");
        assert_eq!(options.layer, GpuiLayer::Top);
        assert_eq!(options.anchor, Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        assert_eq!(options.exclusive_zone, Some(gpui::px(32.0)));
        assert_eq!(options.exclusive_edge, None);
        assert_eq!(options.keyboard_interactivity, KeyboardInteractivity::None);

        let window = window_options(&spec, None, "io.betteros.TopBar");
        assert!(!window.focus, "a bar never asks for focus");
        assert!(window.titlebar.is_none());
        assert_eq!(window.app_id.as_deref(), Some("io.betteros.TopBar"));
        match window.kind {
            WindowKind::LayerShell(ref options) => assert_eq!(options.namespace, "better-top-bar"),
            _ => panic!("not a layer-shell window"),
        }
        let size = match window.window_bounds {
            Some(gpui::WindowBounds::Windowed(bounds)) => bounds.size,
            _ => panic!("no bounds"),
        };
        assert_eq!(size.width, gpui::px(0.0), "fill is the protocol's 0");
        assert_eq!(size.height, gpui::px(32.0));
    }

    #[test]
    fn every_layer_and_keyboard_mode_maps_across() {
        let mut spec = SurfaceSpec::strip("x", Edges::BOTTOM, 10.0);
        for (layer, expected) in [
            (Layer::Background, GpuiLayer::Background),
            (Layer::Bottom, GpuiLayer::Bottom),
            (Layer::Top, GpuiLayer::Top),
            (Layer::Overlay, GpuiLayer::Overlay),
        ] {
            spec.layer = layer;
            assert_eq!(layer_shell_options(&spec).layer, expected);
        }
        for (keyboard, expected) in [
            (Keyboard::None, KeyboardInteractivity::None),
            (Keyboard::OnDemand, KeyboardInteractivity::OnDemand),
            (Keyboard::Exclusive, KeyboardInteractivity::Exclusive),
        ] {
            spec.keyboard = keyboard;
            assert_eq!(layer_shell_options(&spec).keyboard_interactivity, expected);
            assert_eq!(
                window_options(&spec, None, "x").focus,
                keyboard != Keyboard::None
            );
        }
        spec.anchor = Edges::ALL;
        assert_eq!(layer_shell_options(&spec).anchor, Anchor::all());
    }
}
