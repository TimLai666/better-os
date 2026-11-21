//! The rules of a layer-shell surface, with no GPUI in them: what a surface
//! asks the compositor for, which changes GPUI can make to a surface that is
//! already open and which need a new one, which outputs get a surface as
//! outputs come and go, and which part of a surface takes input.
//!
//! GPUI at the pinned revision opens one `zwlr_layer_surface_v1` per
//! `open_window` call, on the output named by `display_id`, and after that
//! changes only the exclusive zone, the exclusive edge, the size, and the
//! input region; namespace, layer, anchor, margin, and keyboard
//! interactivity are fixed until the surface is destroyed
//! (`gpui_linux` `wayland/window.rs` lines 150–194 and 458–499,
//! `gpui` `window.rs` lines 2127–2152). It never drops an output whose
//! global the compositor removed (`wayland/client.rs` line 1286 is a TODO),
//! so an output only reads as gone once the compositor closed the surface on
//! it. Every rule here follows from those facts; `super` carries them out.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::BitOr;

/// The four layer-shell layers, bottom to top.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Layer {
    /// The wallpaper and the desktop's icons.
    Background,
    /// Below windows, above the background.
    Bottom,
    /// Above windows: the top bar, the taskbar.
    #[default]
    Top,
    /// Above everything, fullscreen windows included: the launcher, the
    /// switcher, the overview, notifications.
    Overlay,
}

/// A set of screen edges, for anchors and the exclusive edge.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Edges(u8);

impl Edges {
    /// No edge: the surface is centred.
    pub const NONE: Edges = Edges(0);
    /// The top edge.
    pub const TOP: Edges = Edges(1);
    /// The bottom edge.
    pub const BOTTOM: Edges = Edges(2);
    /// The left edge.
    pub const LEFT: Edges = Edges(4);
    /// The right edge.
    pub const RIGHT: Edges = Edges(8);
    /// Every edge: the surface fills the output.
    pub const ALL: Edges = Edges(15);

    /// The protocol's bit values (`top` 1, `bottom` 2, `left` 4, `right` 8),
    /// which are also GPUI's `layer_shell::Anchor` bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// The edges with these bits; bits above the four edges are dropped.
    pub const fn from_bits(bits: u8) -> Edges {
        Edges(bits & 15)
    }

    /// Whether every edge of `other` is in `self`.
    pub const fn contains(self, other: Edges) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether this is exactly one edge.
    pub const fn is_single(self) -> bool {
        self.0.count_ones() == 1
    }

    /// Both the left and the right edge.
    pub const fn spans_width(self) -> bool {
        self.contains(Edges(4 | 8))
    }

    /// Both the top and the bottom edge.
    pub const fn spans_height(self) -> bool {
        self.contains(Edges(1 | 2))
    }

    /// The single edge a surface anchored here reserves space along: the
    /// layer-shell protocol's rule, an edge anchored without its opposite,
    /// and only when exactly one such edge exists (a bar across the top is
    /// `TOP | LEFT | RIGHT`; a corner has two candidates and none is chosen).
    pub fn implied_exclusive_edge(self) -> Option<Edges> {
        let candidates: Vec<Edges> = [
            (Edges::TOP, Edges::BOTTOM),
            (Edges::BOTTOM, Edges::TOP),
            (Edges::LEFT, Edges::RIGHT),
            (Edges::RIGHT, Edges::LEFT),
        ]
        .into_iter()
        .filter(|(edge, opposite)| self.contains(*edge) && !self.contains(*opposite))
        .map(|(edge, _)| edge)
        .collect();
        match candidates.as_slice() {
            [edge] => Some(*edge),
            _ => None,
        }
    }
}

impl BitOr for Edges {
    type Output = Edges;

    fn bitor(self, other: Edges) -> Edges {
        Edges(self.0 | other.0)
    }
}

/// Whether and how the surface takes the keyboard.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Keyboard {
    /// Never: clicks reach it, keys never do. A bar.
    #[default]
    None,
    /// Like a window: focused when clicked, loses focus to the next click
    /// elsewhere. A notification centre.
    OnDemand,
    /// Takes every key while it is shown on the top or overlay layer. The
    /// launcher, the switcher.
    Exclusive,
}

/// One dimension of a surface's size, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extent {
    /// As wide (or tall) as the output minus the margins. The protocol's
    /// size 0, which is valid only when the surface is anchored to both
    /// opposite edges.
    Fill,
    /// A fixed size.
    Logical(f32),
}

/// How a surface treats the space other surfaces reserve, and whether it
/// reserves some itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ExclusiveZone {
    /// Reserves nothing and is moved out of others' reserved space (the
    /// protocol's 0).
    #[default]
    None,
    /// Reserves this many logical pixels from its exclusive edge, which
    /// windows (maximized ones included) then leave free.
    Reserve(f32),
    /// Reserves nothing and is not moved for others' reserved space: a
    /// fullscreen overlay (the protocol's -1).
    IgnoreOthers,
}

impl ExclusiveZone {
    /// The protocol's value.
    pub fn protocol_value(self) -> f32 {
        match self {
            ExclusiveZone::None => 0.0,
            ExclusiveZone::Reserve(pixels) => pixels,
            ExclusiveZone::IgnoreOthers => -1.0,
        }
    }
}

/// Space between the surface and the edges it is anchored to, in logical
/// pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margin {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Everything a layer surface asks the compositor for when it is created.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceSpec {
    /// The compositor's name for the surface's role (`better-top-bar`);
    /// compositors match rules against it.
    pub namespace: String,
    pub layer: Layer,
    pub anchor: Edges,
    pub width: Extent,
    pub height: Extent,
    pub exclusive_zone: ExclusiveZone,
    /// Needed only when the anchor does not imply one edge (a corner).
    pub exclusive_edge: Option<Edges>,
    pub margin: Margin,
    pub keyboard: Keyboard,
}

/// A spec the compositor would reject, or would silently do something else
/// with. Each is a protocol error or an ignored request in
/// `wlr-layer-shell-unstable-v1`.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecError {
    /// The namespace is empty.
    EmptyNamespace,
    /// A width of [`Extent::Fill`] without both side anchors: the protocol's
    /// `invalid_size` error.
    FillWidthNeedsLeftAndRight,
    /// A height of [`Extent::Fill`] without both top and bottom anchors.
    FillHeightNeedsTopAndBottom,
    /// A size or margin that is negative or not a number.
    NotASize(&'static str),
    /// An exclusive edge that is not one edge: the protocol's
    /// `invalid_exclusive_edge` error, which GPUI avoids by ignoring it.
    ExclusiveEdgeNotSingle,
    /// An exclusive edge the surface is not anchored to.
    ExclusiveEdgeNotAnchored,
    /// Space is reserved but no edge says where: a corner or centred
    /// surface without an exclusive edge. The compositor would reserve
    /// nothing.
    ReserveWithoutEdge,
}

impl SurfaceSpec {
    /// A strip across one edge of the output, `thickness` deep, reserving
    /// that much space and never taking the keyboard: the shape of a top bar
    /// or a taskbar.
    pub fn strip(namespace: impl Into<String>, edge: Edges, thickness: f32) -> SurfaceSpec {
        let (anchor, width, height) = if edge == Edges::TOP || edge == Edges::BOTTOM {
            (
                edge | Edges::LEFT | Edges::RIGHT,
                Extent::Fill,
                Extent::Logical(thickness),
            )
        } else {
            (
                edge | Edges::TOP | Edges::BOTTOM,
                Extent::Logical(thickness),
                Extent::Fill,
            )
        };
        SurfaceSpec {
            namespace: namespace.into(),
            layer: Layer::Top,
            anchor,
            width,
            height,
            exclusive_zone: ExclusiveZone::Reserve(thickness),
            exclusive_edge: None,
            margin: Margin::default(),
            keyboard: Keyboard::None,
        }
    }

    /// Checks the spec against the protocol before a surface is asked for.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.namespace.trim().is_empty() {
            return Err(SpecError::EmptyNamespace);
        }
        for (name, extent) in [("width", self.width), ("height", self.height)] {
            if let Extent::Logical(value) = extent
                && !(value.is_finite() && value >= 0.0)
            {
                return Err(SpecError::NotASize(name));
            }
        }
        let margin = self.margin;
        if [margin.top, margin.right, margin.bottom, margin.left]
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(SpecError::NotASize("margin"));
        }
        if let ExclusiveZone::Reserve(value) = self.exclusive_zone
            && !(value.is_finite() && value >= 0.0)
        {
            return Err(SpecError::NotASize("exclusive zone"));
        }
        if self.width == Extent::Fill && !self.anchor.spans_width() {
            return Err(SpecError::FillWidthNeedsLeftAndRight);
        }
        if self.height == Extent::Fill && !self.anchor.spans_height() {
            return Err(SpecError::FillHeightNeedsTopAndBottom);
        }
        if let Some(edge) = self.exclusive_edge {
            if !edge.is_single() {
                return Err(SpecError::ExclusiveEdgeNotSingle);
            }
            if !self.anchor.contains(edge) {
                return Err(SpecError::ExclusiveEdgeNotAnchored);
            }
        }
        if matches!(self.exclusive_zone, ExclusiveZone::Reserve(value) if value > 0.0)
            && self.reserving_edge().is_none()
        {
            return Err(SpecError::ReserveWithoutEdge);
        }
        Ok(())
    }

    /// The edge the reserved space is taken from: the one asked for, else
    /// the one the anchor implies.
    pub fn reserving_edge(&self) -> Option<Edges> {
        self.exclusive_edge
            .or_else(|| self.anchor.implied_exclusive_edge())
    }

    /// The size GPUI is asked for, with [`Extent::Fill`] as the protocol's 0.
    pub fn requested_size(&self) -> (f32, f32) {
        let value = |extent| match extent {
            Extent::Fill => 0.0,
            Extent::Logical(pixels) => pixels,
        };
        (value(self.width), value(self.height))
    }
}

/// A change GPUI can make to an open surface.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InPlace {
    /// `Window::set_exclusive_zone`.
    ExclusiveZone,
    /// `Window::set_exclusive_edge`.
    ExclusiveEdge,
    /// `Window::resize`, which sets the layer surface's size. Only when
    /// neither dimension is [`Extent::Fill`]: GPUI turns a 0 into 1 before it
    /// sends `set_size` (`gpui_linux` `wayland/window.rs` line 1501), so a
    /// filled strip resized in place becomes one pixel wide.
    Size,
}

/// A change only a new surface can make, because GPUI exposes no request
/// for it after creation (and the protocol has none for namespace or layer
/// before version 2).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Recreate {
    Namespace,
    Layer,
    Anchor,
    Margin,
    Keyboard,
    /// A size change where the new size fills an axis (see [`InPlace::Size`]).
    FilledSize,
}

/// What it takes to turn one spec into another on an open surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    /// Nothing differs.
    Same,
    /// Only requests GPUI can send to the open surface.
    InPlace(Vec<InPlace>),
    /// The surface has to be closed and opened again; anything in-place
    /// comes with the new one.
    Recreate(Vec<Recreate>),
}

/// Compares two specs.
pub fn change(old: &SurfaceSpec, new: &SurfaceSpec) -> Change {
    let mut recreate = Vec::new();
    if old.namespace != new.namespace {
        recreate.push(Recreate::Namespace);
    }
    if old.layer != new.layer {
        recreate.push(Recreate::Layer);
    }
    if old.anchor != new.anchor {
        recreate.push(Recreate::Anchor);
    }
    if old.margin != new.margin {
        recreate.push(Recreate::Margin);
    }
    if old.keyboard != new.keyboard {
        recreate.push(Recreate::Keyboard);
    }
    let resized = old.width != new.width || old.height != new.height;
    if resized && (new.width == Extent::Fill || new.height == Extent::Fill) {
        recreate.push(Recreate::FilledSize);
    }
    if !recreate.is_empty() {
        return Change::Recreate(recreate);
    }
    let mut in_place = Vec::new();
    if old.exclusive_zone != new.exclusive_zone {
        in_place.push(InPlace::ExclusiveZone);
    }
    if old.exclusive_edge != new.exclusive_edge {
        in_place.push(InPlace::ExclusiveEdge);
    }
    if resized {
        in_place.push(InPlace::Size);
    }
    if in_place.is_empty() {
        Change::Same
    } else {
        Change::InPlace(in_place)
    }
}

/// One output as GPUI lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputInfo {
    /// GPUI's `DisplayId` on Wayland: the `wl_output` object's protocol id.
    /// An output unplugged and plugged back in gets a new one.
    pub id: u64,
    /// A key that stays the same when the output is unplugged and plugged
    /// back in: GPUI's `PlatformDisplay::uuid`, a version 5 UUID of the
    /// compositor's connector name (`eDP-1`, `HEADLESS-2`), when it sent one.
    /// GPUI exposes no other form of the name.
    pub key: Option<String>,
    /// The output's scale, when the caller knows it. GPUI does not expose
    /// one: it reads `wl_output.scale` once, at bind, and never again, so a
    /// rescale is seen on the surface (`Window::scale_factor`), not here.
    pub scale: Option<f32>,
}

/// Which outputs get a surface.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Placement {
    /// One surface on every output: the top bar, the desktop.
    #[default]
    EveryOutput,
    /// One surface, on the output with this [`OutputInfo::key`] when there is
    /// one, else where it already is, else on the output listed with the
    /// lowest id: the launcher, the switcher.
    One { preferred: Option<String> },
}

/// What [`SurfaceSet::plan`] asks the GPUI side to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Open a surface on this output.
    Open { output: u64 },
    /// Close the surface on this output.
    Close { output: u64 },
    /// Close the surface on this output and open it again with the new spec.
    Recreate { output: u64, reasons: Vec<Recreate> },
    /// Send these requests to the open surface on this output.
    Update { output: u64, changes: Vec<InPlace> },
}

/// The surfaces of one role (one per output, or one), and the outputs the
/// compositor took away.
#[derive(Clone, Debug, Default)]
pub struct SurfaceSet {
    open: BTreeMap<u64, SurfaceSpec>,
    gone: BTreeSet<u64>,
}

impl SurfaceSet {
    /// An empty set.
    pub fn new() -> SurfaceSet {
        SurfaceSet::default()
    }

    /// The outputs that have a surface now.
    pub fn outputs(&self) -> impl Iterator<Item = u64> + '_ {
        self.open.keys().copied()
    }

    /// The spec the surface on `output` was opened or last updated with.
    pub fn spec(&self, output: u64) -> Option<&SurfaceSpec> {
        self.open.get(&output)
    }

    /// Whether the compositor closed the surface on `output`, which is how
    /// an output that GPUI still lists is known to be gone.
    pub fn is_gone(&self, output: u64) -> bool {
        self.gone.contains(&output)
    }

    /// The actions that bring the open surfaces to `wanted` on the outputs
    /// `placement` picks from `outputs`. Outputs the compositor closed a
    /// surface on are skipped. A rescale is never an action: GPUI follows the
    /// surface's preferred scale itself and the spec is in logical pixels.
    pub fn plan(
        &self,
        outputs: &[OutputInfo],
        placement: &Placement,
        wanted: &SurfaceSpec,
    ) -> Vec<Action> {
        let live: Vec<&OutputInfo> = outputs
            .iter()
            .filter(|output| !self.gone.contains(&output.id))
            .collect();
        let targets: BTreeSet<u64> = match placement {
            Placement::EveryOutput => live.iter().map(|output| output.id).collect(),
            Placement::One { preferred } => {
                let named = preferred.as_ref().and_then(|name| {
                    live.iter()
                        .find(|output| output.key.as_deref() == Some(name.as_str()))
                });
                // Keep the surface where it is while its output lives, so a
                // new output plugged in does not move the launcher.
                let current = self
                    .open
                    .keys()
                    .find(|id| live.iter().any(|output| output.id == **id));
                named
                    .map(|output| output.id)
                    .or(current.copied())
                    .or_else(|| live.iter().map(|output| output.id).min())
                    .into_iter()
                    .collect()
            }
        };

        let mut actions = Vec::new();
        for output in self.open.keys() {
            if !targets.contains(output) {
                actions.push(Action::Close { output: *output });
            }
        }
        for output in &targets {
            match self.open.get(output) {
                None => actions.push(Action::Open { output: *output }),
                Some(spec) => match change(spec, wanted) {
                    Change::Same => {}
                    Change::InPlace(changes) => actions.push(Action::Update {
                        output: *output,
                        changes,
                    }),
                    Change::Recreate(reasons) => actions.push(Action::Recreate {
                        output: *output,
                        reasons,
                    }),
                },
            }
        }
        actions
    }

    /// Records that a surface with `spec` is open on `output` (after an
    /// open, a recreate, or an update).
    pub fn opened(&mut self, output: u64, spec: SurfaceSpec) {
        self.gone.remove(&output);
        self.open.insert(output, spec);
    }

    /// Records that Better OS closed the surface on `output` itself.
    pub fn closed(&mut self, output: u64) {
        self.open.remove(&output);
    }

    /// Records that the compositor closed the surface on `output`
    /// (`zwlr_layer_surface_v1.closed`): the output went away, or the
    /// compositor refused the surface. It is not reopened there; an output
    /// plugged back in has a new id.
    pub fn closed_by_compositor(&mut self, output: u64) {
        self.open.remove(&output);
        self.gone.insert(output);
    }
}

/// A rectangle in a surface's logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// A rectangle.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}

/// The part of a surface that takes pointer and touch input.
#[derive(Clone, Debug, PartialEq)]
pub enum InputRegion {
    /// All of it (the protocol's null region).
    Whole,
    /// These rectangles, in whole logical pixels.
    Rects(Vec<Rect>),
    /// None of it: every click goes to what is underneath.
    Nothing,
}

/// The input region of a surface `size` big of which only `drawn` is
/// painted. Each rectangle is clipped to the surface and widened to whole
/// pixels (GPUI truncates to integers, which would drop a pixel at a
/// fractional edge); empty ones are dropped. Nothing drawn takes no input,
/// and a rectangle covering the surface makes it [`InputRegion::Whole`].
pub fn input_region(size: (f32, f32), drawn: &[Rect]) -> InputRegion {
    let (surface_width, surface_height) = size;
    let mut rects = Vec::new();
    for rect in drawn {
        if ![rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|value| value.is_finite())
        {
            continue;
        }
        let left = rect.x.max(0.0).floor();
        let top = rect.y.max(0.0).floor();
        let right = (rect.x + rect.width).min(surface_width).ceil();
        let bottom = (rect.y + rect.height).min(surface_height).ceil();
        if right <= left || bottom <= top {
            continue;
        }
        if left <= 0.0 && top <= 0.0 && right >= surface_width && bottom >= surface_height {
            return InputRegion::Whole;
        }
        rects.push(Rect::new(left, top, right - left, bottom - top));
    }
    if rects.is_empty() {
        InputRegion::Nothing
    } else {
        InputRegion::Rects(rects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> SurfaceSpec {
        SurfaceSpec::strip("better-top-bar", Edges::TOP, 32.0)
    }

    fn output(id: u64, name: &str) -> OutputInfo {
        OutputInfo {
            id,
            key: Some(name.to_string()),
            scale: Some(1.0),
        }
    }

    #[test]
    fn a_strip_is_valid_and_reserves_its_own_edge() {
        let spec = bar();
        assert_eq!(spec.validate(), Ok(()));
        assert_eq!(spec.anchor, Edges::TOP | Edges::LEFT | Edges::RIGHT);
        assert_eq!(spec.reserving_edge(), Some(Edges::TOP));
        assert_eq!(spec.requested_size(), (0.0, 32.0));
        let side = SurfaceSpec::strip("dock", Edges::LEFT, 64.0);
        assert_eq!(side.validate(), Ok(()));
        assert_eq!(side.reserving_edge(), Some(Edges::LEFT));
        assert_eq!(side.requested_size(), (64.0, 0.0));
    }

    #[test]
    fn the_implied_edge_follows_the_protocol() {
        assert_eq!(Edges::TOP.implied_exclusive_edge(), Some(Edges::TOP));
        assert_eq!(
            (Edges::BOTTOM | Edges::LEFT | Edges::RIGHT).implied_exclusive_edge(),
            Some(Edges::BOTTOM)
        );
        // A corner has two candidates, a fill has none.
        assert_eq!((Edges::TOP | Edges::LEFT).implied_exclusive_edge(), None);
        assert_eq!(Edges::ALL.implied_exclusive_edge(), None);
        assert_eq!(Edges::NONE.implied_exclusive_edge(), None);
        assert_eq!(
            (Edges::LEFT | Edges::RIGHT).implied_exclusive_edge(),
            None,
            "anchored to both sides and neither top nor bottom"
        );
    }

    #[test]
    fn fill_needs_both_opposite_anchors() {
        let mut spec = bar();
        spec.anchor = Edges::TOP | Edges::LEFT;
        assert_eq!(spec.validate(), Err(SpecError::FillWidthNeedsLeftAndRight));
        let mut spec = SurfaceSpec::strip("dock", Edges::RIGHT, 48.0);
        spec.anchor = Edges::RIGHT | Edges::TOP;
        assert_eq!(spec.validate(), Err(SpecError::FillHeightNeedsTopAndBottom));
    }

    #[test]
    fn an_exclusive_edge_must_be_one_anchored_edge() {
        let mut corner = bar();
        corner.anchor = Edges::TOP | Edges::LEFT;
        corner.width = Extent::Logical(200.0);
        assert_eq!(corner.validate(), Err(SpecError::ReserveWithoutEdge));
        corner.exclusive_edge = Some(Edges::TOP);
        assert_eq!(corner.validate(), Ok(()));
        assert_eq!(corner.reserving_edge(), Some(Edges::TOP));
        corner.exclusive_edge = Some(Edges::TOP | Edges::LEFT);
        assert_eq!(corner.validate(), Err(SpecError::ExclusiveEdgeNotSingle));
        corner.exclusive_edge = Some(Edges::BOTTOM);
        assert_eq!(corner.validate(), Err(SpecError::ExclusiveEdgeNotAnchored));
        // A corner that reserves nothing needs no edge.
        corner.exclusive_edge = None;
        corner.exclusive_zone = ExclusiveZone::None;
        assert_eq!(corner.validate(), Ok(()));
    }

    #[test]
    fn sizes_and_margins_must_be_numbers() {
        let mut spec = bar();
        spec.height = Extent::Logical(f32::NAN);
        assert_eq!(spec.validate(), Err(SpecError::NotASize("height")));
        let mut spec = bar();
        spec.height = Extent::Logical(-1.0);
        assert_eq!(spec.validate(), Err(SpecError::NotASize("height")));
        let mut spec = bar();
        spec.margin.left = f32::INFINITY;
        assert_eq!(spec.validate(), Err(SpecError::NotASize("margin")));
        let mut spec = bar();
        spec.exclusive_zone = ExclusiveZone::Reserve(-4.0);
        assert_eq!(spec.validate(), Err(SpecError::NotASize("exclusive zone")));
        let mut spec = bar();
        spec.namespace = "  ".into();
        assert_eq!(spec.validate(), Err(SpecError::EmptyNamespace));
    }

    #[test]
    fn exclusive_zones_map_to_the_protocol_values() {
        assert_eq!(ExclusiveZone::None.protocol_value(), 0.0);
        assert_eq!(ExclusiveZone::Reserve(32.0).protocol_value(), 32.0);
        assert_eq!(ExclusiveZone::IgnoreOthers.protocol_value(), -1.0);
    }

    #[test]
    fn zone_edge_and_size_change_in_place() {
        let old = bar();
        let mut new = bar();
        new.exclusive_zone = ExclusiveZone::Reserve(48.0);
        assert_eq!(
            change(&old, &new),
            Change::InPlace(vec![InPlace::ExclusiveZone])
        );
        let mut corner = bar();
        corner.anchor = Edges::TOP | Edges::LEFT;
        corner.width = Extent::Logical(200.0);
        corner.exclusive_zone = ExclusiveZone::None;
        let mut bigger = corner.clone();
        bigger.width = Extent::Logical(300.0);
        bigger.height = Extent::Logical(48.0);
        assert_eq!(
            change(&corner, &bigger),
            Change::InPlace(vec![InPlace::Size]),
            "a fixed size resizes in place"
        );
        let mut edge = bar();
        edge.exclusive_edge = Some(Edges::TOP);
        assert_eq!(
            change(&old, &edge),
            Change::InPlace(vec![InPlace::ExclusiveEdge])
        );
        assert_eq!(change(&old, &bar()), Change::Same);
    }

    #[test]
    fn anchor_margin_layer_keyboard_and_namespace_need_a_new_surface() {
        let old = bar();
        let mut moved = SurfaceSpec::strip("better-top-bar", Edges::BOTTOM, 32.0);
        moved.exclusive_zone = ExclusiveZone::Reserve(40.0);
        // An anchor change wins over the in-place zone change that rides along.
        assert_eq!(
            change(&old, &moved),
            Change::Recreate(vec![Recreate::Anchor])
        );

        let mut new = bar();
        new.margin.top = 8.0;
        new.layer = Layer::Overlay;
        new.keyboard = Keyboard::OnDemand;
        new.namespace = "other".into();
        assert_eq!(
            change(&old, &new),
            Change::Recreate(vec![
                Recreate::Namespace,
                Recreate::Layer,
                Recreate::Margin,
                Recreate::Keyboard,
            ])
        );
    }

    #[test]
    fn a_filled_strip_changing_thickness_needs_a_new_surface() {
        // Seen on cosmic-comp: resized in place, GPUI sent set_size(1, 64)
        // and the strip vanished.
        let old = bar();
        let mut thicker = bar();
        thicker.height = Extent::Logical(64.0);
        thicker.exclusive_zone = ExclusiveZone::Reserve(64.0);
        assert_eq!(
            change(&old, &thicker),
            Change::Recreate(vec![Recreate::FilledSize])
        );
    }

    #[test]
    fn every_output_gets_a_surface_and_a_new_output_gets_one_too() {
        let spec = bar();
        let mut set = SurfaceSet::new();
        let outputs = vec![output(10, "HEADLESS-1")];
        assert_eq!(
            set.plan(&outputs, &Placement::EveryOutput, &spec),
            vec![Action::Open { output: 10 }]
        );
        set.opened(10, spec.clone());
        assert!(
            set.plan(&outputs, &Placement::EveryOutput, &spec)
                .is_empty()
        );

        let added = vec![output(10, "HEADLESS-1"), output(22, "HEADLESS-2")];
        assert_eq!(
            set.plan(&added, &Placement::EveryOutput, &spec),
            vec![Action::Open { output: 22 }]
        );
    }

    #[test]
    fn a_removed_output_loses_its_surface_whether_or_not_gpui_still_lists_it() {
        let spec = bar();
        let mut set = SurfaceSet::new();
        set.opened(10, spec.clone());
        set.opened(22, spec.clone());

        // An output that disappears from the list is closed.
        let one = vec![output(10, "HEADLESS-1")];
        assert_eq!(
            set.plan(&one, &Placement::EveryOutput, &spec),
            vec![Action::Close { output: 22 }]
        );

        // GPUI keeps listing a removed output; the compositor's `closed` on
        // its surface is what says it is gone, and it is not reopened.
        let stale = vec![output(10, "HEADLESS-1"), output(22, "HEADLESS-2")];
        set.closed_by_compositor(22);
        assert!(set.is_gone(22));
        assert!(set.plan(&stale, &Placement::EveryOutput, &spec).is_empty());

        // Plugged back in, it is a new wl_output with a new id.
        let replugged = vec![
            output(10, "HEADLESS-1"),
            output(22, "HEADLESS-2"),
            output(31, "HEADLESS-2"),
        ];
        assert_eq!(
            set.plan(&replugged, &Placement::EveryOutput, &spec),
            vec![Action::Open { output: 31 }]
        );
    }

    #[test]
    fn a_rescaled_output_needs_nothing() {
        let spec = bar();
        let mut set = SurfaceSet::new();
        set.opened(10, spec.clone());
        let mut rescaled = output(10, "HEADLESS-1");
        rescaled.scale = Some(2.0);
        assert!(
            set.plan(&[rescaled], &Placement::EveryOutput, &spec)
                .is_empty(),
            "GPUI follows the preferred scale; the spec is in logical pixels"
        );
    }

    #[test]
    fn a_changed_spec_updates_or_recreates_every_surface() {
        let spec = bar();
        let mut set = SurfaceSet::new();
        set.opened(10, spec.clone());
        set.opened(22, spec.clone());
        let outputs = vec![output(10, "A"), output(22, "B")];

        let mut taller = bar();
        taller.exclusive_zone = ExclusiveZone::Reserve(40.0);
        assert_eq!(
            set.plan(&outputs, &Placement::EveryOutput, &taller),
            vec![
                Action::Update {
                    output: 10,
                    changes: vec![InPlace::ExclusiveZone]
                },
                Action::Update {
                    output: 22,
                    changes: vec![InPlace::ExclusiveZone]
                },
            ]
        );

        let bottom = SurfaceSpec::strip("better-top-bar", Edges::BOTTOM, 32.0);
        assert_eq!(
            set.plan(&outputs, &Placement::EveryOutput, &bottom),
            vec![
                Action::Recreate {
                    output: 10,
                    reasons: vec![Recreate::Anchor]
                },
                Action::Recreate {
                    output: 22,
                    reasons: vec![Recreate::Anchor]
                },
            ]
        );
    }

    #[test]
    fn one_surface_prefers_its_named_output_and_otherwise_stays_put() {
        let spec = bar();
        let mut set = SurfaceSet::new();
        let outputs = vec![output(22, "HDMI-1"), output(10, "eDP-1")];
        let first = Placement::One { preferred: None };
        assert_eq!(
            set.plan(&outputs, &first, &spec),
            vec![Action::Open { output: 10 }],
            "the lowest id without a preference"
        );
        let named = Placement::One {
            preferred: Some("HDMI-1".into()),
        };
        assert_eq!(
            set.plan(&outputs, &named, &spec),
            vec![Action::Open { output: 22 }]
        );

        set.opened(22, spec.clone());
        // A new output does not move it.
        let more = vec![output(22, "HDMI-1"), output(10, "eDP-1"), output(5, "DP-3")];
        assert!(set.plan(&more, &first, &spec).is_empty());
        // Its output gone, it moves to the lowest id left.
        set.closed_by_compositor(22);
        assert_eq!(
            set.plan(&more, &first, &spec),
            vec![Action::Open { output: 5 }]
        );
    }

    #[test]
    fn closing_by_hand_allows_reopening() {
        let spec = bar();
        let mut set = SurfaceSet::new();
        set.opened(10, spec.clone());
        set.closed(10);
        assert!(!set.is_gone(10));
        assert_eq!(
            set.plan(&[output(10, "A")], &Placement::EveryOutput, &spec),
            vec![Action::Open { output: 10 }]
        );
    }

    #[test]
    fn no_output_means_no_surface() {
        let set = SurfaceSet::new();
        assert!(set.plan(&[], &Placement::EveryOutput, &bar()).is_empty());
        assert!(
            set.plan(&[], &Placement::One { preferred: None }, &bar())
                .is_empty()
        );
    }

    #[test]
    fn the_input_region_is_what_is_drawn_in_whole_pixels() {
        let size = (1280.0, 32.0);
        assert_eq!(input_region(size, &[]), InputRegion::Nothing);
        assert_eq!(
            input_region(size, &[Rect::new(0.0, 0.0, 1280.0, 32.0)]),
            InputRegion::Whole
        );
        assert_eq!(
            input_region(size, &[Rect::new(-10.0, -5.0, 2000.0, 100.0)]),
            InputRegion::Whole,
            "clipped to the surface first"
        );
        assert_eq!(
            input_region(size, &[Rect::new(10.4, 0.0, 100.2, 31.5)]),
            InputRegion::Rects(vec![Rect::new(10.0, 0.0, 101.0, 32.0)]),
            "widened outward, never narrowed"
        );
        assert_eq!(
            input_region(
                size,
                &[
                    Rect::new(1300.0, 0.0, 10.0, 10.0),
                    Rect::new(0.0, 0.0, 0.0, 32.0),
                    Rect::new(f32::NAN, 0.0, 1.0, 1.0),
                ]
            ),
            InputRegion::Nothing,
            "outside, empty, or not a number"
        );
        assert_eq!(
            input_region(
                size,
                &[
                    Rect::new(0.0, 0.0, 200.0, 32.0),
                    Rect::new(1180.0, 0.0, 100.0, 32.0)
                ]
            ),
            InputRegion::Rects(vec![
                Rect::new(0.0, 0.0, 200.0, 32.0),
                Rect::new(1180.0, 0.0, 100.0, 32.0)
            ])
        );
    }

    #[test]
    fn edges_round_trip_through_their_bits() {
        for bits in 0..16u8 {
            assert_eq!(Edges::from_bits(bits).bits(), bits);
        }
        assert_eq!(Edges::from_bits(0xff), Edges::ALL);
        assert!(Edges::TOP.is_single());
        assert!(!(Edges::TOP | Edges::LEFT).is_single());
        assert!(!Edges::NONE.is_single());
    }
}
