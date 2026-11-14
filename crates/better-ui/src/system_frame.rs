//! The window frame the desktop draws for everyone else: corners, edges,
//! shadow, and the minimize, maximize, and close buttons.
//!
//! GNOME's compositor gives a Wayland client no decorations, so every
//! application draws its own, and GTK applications, Chromium, and Electron
//! (Claude Desktop among them) look like the desktop because GTK draws theirs
//! from the current theme. The owner decided on 2026-11-14 that Better OS
//! does the same (ticket 108): GTK is loaded at run time, asked to draw each
//! part offscreen, and the images are painted in GPUI. The method is
//! Chromium's (`ui/gtk/window_frame_provider_gtk.cc` and
//! `ui/gtk/nav_button_provider_gtk.cc`, read for the method only).
//!
//! This module holds every decision that does not need GTK or GPUI, so it is
//! tested on its own: reading the desktop's settings
//! (`org.gnome.desktop.wm.preferences` and `org.gnome.desktop.interface`),
//! what a click on the title bar does, which edge a pointer resizes, the
//! measurements taken from a rendered frame, and the Adwaita look used when GTK
//! cannot be loaded. [`gtk`] does the drawing and [`service`] keeps the
//! images current; `crate::fluent::window_frame` paints them.

#[cfg(target_os = "linux")]
pub mod gtk;
pub mod service;

#[cfg(test)]
mod tests;

/// A window button GTK knows by name in `button-layout`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowButton {
    Minimize,
    /// Maximize, or restore while the window is maximized.
    Maximize,
    Close,
}

impl WindowButton {
    pub const ALL: [WindowButton; 3] = [
        WindowButton::Minimize,
        WindowButton::Maximize,
        WindowButton::Close,
    ];

    /// The style class GTK gives the button (`button.titlebutton.close`).
    pub fn css_class(self) -> &'static str {
        match self {
            WindowButton::Minimize => "minimize",
            WindowButton::Maximize => "maximize",
            WindowButton::Close => "close",
        }
    }

    /// The symbolic icon the button shows, from the current icon theme.
    pub fn icon_name(self, maximized: bool) -> &'static str {
        match (self, maximized) {
            (WindowButton::Minimize, _) => "window-minimize-symbolic",
            (WindowButton::Maximize, false) => "window-maximize-symbolic",
            (WindowButton::Maximize, true) => "window-restore-symbolic",
            (WindowButton::Close, _) => "window-close-symbolic",
        }
    }
}

/// `org.gnome.desktop.wm.preferences button-layout`: which buttons sit at the
/// title bar's start and which at its end.
///
/// The value is `start:end`, each side a comma-separated list of names, as in
/// GNOME's default `appmenu:close` or Zorin's
/// `appmenu:minimize,maximize,close`. Names other than `minimize`,
/// `maximize`, and `close` (`appmenu`, `icon`, `menu`, `spacer`) draw nothing
/// in a GTK 4 window either, so they are skipped. A button named twice is
/// drawn once, where it is first named; a value without a colon puts every
/// button at the start, as GTK does.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ButtonLayout {
    pub start: Vec<WindowButton>,
    pub end: Vec<WindowButton>,
}

impl Default for ButtonLayout {
    /// GNOME's default, `appmenu:close`.
    fn default() -> Self {
        ButtonLayout {
            start: Vec::new(),
            end: vec![WindowButton::Close],
        }
    }
}

impl ButtonLayout {
    pub fn parse(value: &str) -> ButtonLayout {
        let mut seen = Vec::new();
        let mut side = |text: &str| -> Vec<WindowButton> {
            text.split(',')
                .filter_map(|name| match name.trim() {
                    "minimize" => Some(WindowButton::Minimize),
                    "maximize" => Some(WindowButton::Maximize),
                    "close" => Some(WindowButton::Close),
                    _ => None,
                })
                .filter(|button| {
                    let fresh = !seen.contains(button);
                    seen.push(*button);
                    fresh
                })
                .collect()
        };
        let (start, end) = value.split_once(':').unwrap_or((value, ""));
        let start = side(start);
        let end = side(end);
        ButtonLayout { start, end }
    }

    /// The layout for a dialog window: its close button alone, on the side
    /// the desktop puts close (the end when the layout has no close at all).
    pub fn dialog(&self) -> ButtonLayout {
        if self.start.contains(&WindowButton::Close) {
            ButtonLayout {
                start: vec![WindowButton::Close],
                end: Vec::new(),
            }
        } else {
            ButtonLayout {
                start: Vec::new(),
                end: vec![WindowButton::Close],
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start.is_empty() && self.end.is_empty()
    }
}

/// What a click on empty title bar space does, as
/// `action-double-click-titlebar`, `action-middle-click-titlebar`, and
/// `action-right-click-titlebar` name it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TitlebarAction {
    ToggleMaximize,
    ToggleMaximizeHorizontally,
    ToggleMaximizeVertically,
    Minimize,
    Lower,
    Menu,
    None,
}

impl TitlebarAction {
    pub fn parse(value: &str) -> Option<TitlebarAction> {
        Some(match value {
            "toggle-maximize" => TitlebarAction::ToggleMaximize,
            "toggle-maximize-horizontally" => TitlebarAction::ToggleMaximizeHorizontally,
            "toggle-maximize-vertically" => TitlebarAction::ToggleMaximizeVertically,
            "minimize" => TitlebarAction::Minimize,
            "lower" => TitlebarAction::Lower,
            "menu" => TitlebarAction::Menu,
            "none" => TitlebarAction::None,
            // `toggle-shade` is a removed X11 action GNOME still lists.
            "toggle-shade" => TitlebarAction::None,
            _ => return None,
        })
    }

    /// What the window does. A Wayland client can only ask for the whole
    /// window to maximize, so the one-direction maximizes toggle maximize, as
    /// GTK 4 does; Wayland has no way to lower a window, so `lower` does
    /// nothing, as in a GTK 4 window.
    pub fn response(self) -> TitlebarResponse {
        match self {
            TitlebarAction::ToggleMaximize
            | TitlebarAction::ToggleMaximizeHorizontally
            | TitlebarAction::ToggleMaximizeVertically => TitlebarResponse::ToggleMaximize,
            TitlebarAction::Minimize => TitlebarResponse::Minimize,
            TitlebarAction::Menu => TitlebarResponse::WindowMenu,
            TitlebarAction::Lower | TitlebarAction::None => TitlebarResponse::Nothing,
        }
    }
}

/// What the window does for a title bar click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TitlebarResponse {
    ToggleMaximize,
    Minimize,
    /// The compositor's window menu, at the pointer.
    WindowMenu,
    Nothing,
}

/// A click on empty title bar space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TitlebarClick {
    Double,
    Middle,
    Right,
}

/// `org.gnome.desktop.interface color-scheme`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ColorScheme {
    #[default]
    Default,
    PreferDark,
    PreferLight,
}

impl ColorScheme {
    pub fn parse(value: &str) -> Option<ColorScheme> {
        Some(match value {
            "default" => ColorScheme::Default,
            "prefer-dark" => ColorScheme::PreferDark,
            "prefer-light" => ColorScheme::PreferLight,
            _ => return None,
        })
    }

    /// Whether the desktop asks for dark. GNOME's `default` is light.
    pub fn is_dark(self) -> bool {
        self == ColorScheme::PreferDark
    }
}

/// Everything the frame reads from the desktop.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DesktopSettings {
    /// `gtk-theme`, when set.
    pub gtk_theme: Option<String>,
    /// `icon-theme`, when set.
    pub icon_theme: Option<String>,
    pub color_scheme: ColorScheme,
    pub button_layout: ButtonLayout,
    pub double_click: TitlebarAction,
    pub middle_click: TitlebarAction,
    pub right_click: TitlebarAction,
}

impl Default for DesktopSettings {
    /// GNOME's defaults, for a desktop whose settings cannot be read.
    fn default() -> Self {
        DesktopSettings {
            gtk_theme: None,
            icon_theme: None,
            color_scheme: ColorScheme::Default,
            button_layout: ButtonLayout::default(),
            double_click: TitlebarAction::ToggleMaximize,
            middle_click: TitlebarAction::None,
            right_click: TitlebarAction::Menu,
        }
    }
}

/// The schema holding the title bar settings.
pub const WM_SCHEMA: &str = "org.gnome.desktop.wm.preferences";
/// The schema holding the theme and colour scheme.
pub const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";

/// The keys read, schema first.
pub const KEYS: [(&str, &str); 7] = [
    (INTERFACE_SCHEMA, "gtk-theme"),
    (INTERFACE_SCHEMA, "icon-theme"),
    (INTERFACE_SCHEMA, "color-scheme"),
    (WM_SCHEMA, "button-layout"),
    (WM_SCHEMA, "action-double-click-titlebar"),
    (WM_SCHEMA, "action-middle-click-titlebar"),
    (WM_SCHEMA, "action-right-click-titlebar"),
];

/// Where the settings come from. Only ever read, never written.
pub trait SettingsSource: Send + Sync {
    /// The value of `key` in `schema` as `gsettings get` prints it, or `None`
    /// when the schema or key does not exist.
    fn get(&self, schema: &str, key: &str) -> Option<String>;
}

/// GNOME's settings, read through `gsettings get`, the GLib tool that reads
/// whichever backend the session uses. Read only.
pub struct GSettings;

impl SettingsSource for GSettings {
    fn get(&self, schema: &str, key: &str) -> Option<String> {
        let output = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// GNOME's settings, read with one `gsettings list-recursively` per schema
/// (two processes rather than one per key). Read only.
pub struct GSettingsListing {
    values: Vec<(String, String, String)>,
}

impl GSettingsListing {
    /// Runs `gsettings list-recursively` on both schemas the frame reads.
    pub fn read() -> GSettingsListing {
        let mut text = String::new();
        for schema in [INTERFACE_SCHEMA, WM_SCHEMA] {
            let Ok(output) = std::process::Command::new("gsettings")
                .args(["list-recursively", schema])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
            else {
                continue;
            };
            if output.status.success() {
                text.push_str(&String::from_utf8_lossy(&output.stdout));
                text.push('\n');
            }
        }
        GSettingsListing::parse(&text)
    }

    /// What `gsettings list-recursively` prints: `schema key value`, one key
    /// a line.
    pub fn parse(text: &str) -> GSettingsListing {
        let values = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, ' ');
                let schema = parts.next()?.trim();
                let key = parts.next()?.trim();
                let value = parts.next()?.trim();
                (!schema.is_empty() && !key.is_empty())
                    .then(|| (schema.to_string(), key.to_string(), value.to_string()))
            })
            .collect();
        GSettingsListing { values }
    }
}

impl SettingsSource for GSettingsListing {
    fn get(&self, schema: &str, key: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(s, k, _)| s == schema && k == key)
            .map(|(_, _, v)| v.clone())
    }
}

/// Settings fixed in advance, for tests: `(schema, key, printed value)`.
#[derive(Default)]
pub struct FixedSettings(pub Vec<(String, String, String)>);

impl FixedSettings {
    pub fn with(mut self, schema: &str, key: &str, value: &str) -> Self {
        self.0
            .push((schema.to_string(), key.to_string(), value.to_string()));
        self
    }
}

impl SettingsSource for FixedSettings {
    fn get(&self, schema: &str, key: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(s, k, _)| s == schema && k == key)
            .map(|(_, _, v)| v.clone())
    }
}

/// What `gsettings get` prints for a string, `'appmenu:close'`, unquoted.
/// Anything not quoted as a string is not one.
pub fn gsettings_string(printed: &str) -> Option<String> {
    let text = printed.trim();
    let inner = text
        .strip_prefix('\'')
        .and_then(|t| t.strip_suffix('\''))
        .or_else(|| text.strip_prefix('"').and_then(|t| t.strip_suffix('"')))?;
    Some(inner.to_string())
}

impl DesktopSettings {
    /// Reads every setting, keeping GNOME's default for any that is missing
    /// or holds a value GNOME does not define.
    pub fn read(source: &dyn SettingsSource) -> DesktopSettings {
        let string = |schema, key| source.get(schema, key).and_then(|v| gsettings_string(&v));
        let defaults = DesktopSettings::default();
        let name = |value: Option<String>| {
            value.filter(|v| !v.is_empty() && !v.contains('/') && v != "." && v != "..")
        };
        let action = |key, default| {
            string(WM_SCHEMA, key)
                .and_then(|v| TitlebarAction::parse(&v))
                .unwrap_or(default)
        };
        DesktopSettings {
            gtk_theme: name(string(INTERFACE_SCHEMA, "gtk-theme")),
            icon_theme: name(string(INTERFACE_SCHEMA, "icon-theme")),
            color_scheme: string(INTERFACE_SCHEMA, "color-scheme")
                .and_then(|v| ColorScheme::parse(&v))
                .unwrap_or(defaults.color_scheme),
            button_layout: string(WM_SCHEMA, "button-layout")
                .map(|v| ButtonLayout::parse(&v))
                .unwrap_or(defaults.button_layout),
            double_click: action("action-double-click-titlebar", defaults.double_click),
            middle_click: action("action-middle-click-titlebar", defaults.middle_click),
            right_click: action("action-right-click-titlebar", defaults.right_click),
        }
    }

    /// What a click on empty title bar space does.
    pub fn titlebar_response(&self, click: TitlebarClick) -> TitlebarResponse {
        match click {
            TitlebarClick::Double => self.double_click,
            TitlebarClick::Middle => self.middle_click,
            TitlebarClick::Right => self.right_click,
        }
        .response()
    }
}

/// One line of `gsettings monitor SCHEMA` output names a key that changed:
/// `button-layout: 'close:'`. Returns the key when it is one the frame reads.
pub fn monitored_key(line: &str) -> Option<&'static str> {
    let key = line.split_once(':')?.0.trim();
    KEYS.iter().map(|(_, k)| *k).find(|k| *k == key)
}

/// A rectangle's four sides.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const ZERO: Insets = Insets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub fn uniform(value: f32) -> Insets {
        Insets {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn max_side(&self) -> f32 {
        self.top.max(self.right).max(self.bottom).max(self.left)
    }
}

/// Which sides of the window touch a screen edge or another window, as the
/// compositor reports it. A maximized window is tiled on every side.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Tiled {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

impl Tiled {
    pub const ALL: Tiled = Tiled {
        top: true,
        right: true,
        bottom: true,
        left: true,
    };

    pub fn any(&self) -> bool {
        self.top || self.right || self.bottom || self.left
    }

    pub fn all(&self) -> bool {
        self.top && self.right && self.bottom && self.left
    }
}

/// A window edge or corner a press resizes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Edge {
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    TopLeft,
}

/// How far outside the visible window a press still resizes it: GTK 4's
/// `RESIZE_HANDLE_SIZE` (`gtk/gtkwindow.c`, 4.14).
pub const RESIZE_HANDLE: f32 = 12.0;
/// How far along an edge from a corner a press resizes from the corner: GTK
/// 4's `RESIZE_HANDLE_CORNER_SIZE`.
pub const RESIZE_CORNER: f32 = 24.0;

/// The edge a press at (`x`, `y`) resizes, in a surface `width` by `height`
/// whose visible window sits `inset` inside it, or `None` when the press is
/// on the window itself or beyond the resize handles. This is GTK 4's
/// `get_edge_for_coordinates`: a handle reaches [`RESIZE_HANDLE`] outside
/// the window (never past the surface), a corner covers
/// [`RESIZE_CORNER`] of each edge next to it, and a side that touches the
/// screen or another window does not resize. A maximized window does not
/// resize at all.
pub fn resize_edge(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    inset: Insets,
    tiled: Tiled,
    maximized: bool,
) -> Option<Edge> {
    if maximized || tiled.all() {
        return None;
    }
    let left = inset.left;
    let top = inset.top;
    let right = width - inset.right;
    let bottom = height - inset.bottom;
    let handle = |side: f32| side.min(RESIZE_HANDLE);
    let in_left = x < left && x >= left - handle(inset.left);
    let in_right = x > right && x <= right + handle(inset.right);
    let in_top = y < top && y >= top - handle(inset.top);
    let in_bottom = y > bottom && y <= bottom + handle(inset.bottom);
    let near_top = y < top + RESIZE_CORNER && y >= top - handle(inset.top);
    let near_bottom = y > bottom - RESIZE_CORNER && y <= bottom + handle(inset.bottom);
    let near_left = x < left + RESIZE_CORNER && x >= left - handle(inset.left);
    let near_right = x > right - RESIZE_CORNER && x <= right + handle(inset.right);
    let edge = if in_left {
        if near_top {
            Edge::TopLeft
        } else if near_bottom {
            Edge::BottomLeft
        } else {
            Edge::Left
        }
    } else if in_right {
        if near_top {
            Edge::TopRight
        } else if near_bottom {
            Edge::BottomRight
        } else {
            Edge::Right
        }
    } else if in_top {
        if near_left {
            Edge::TopLeft
        } else if near_right {
            Edge::TopRight
        } else {
            Edge::Top
        }
    } else if in_bottom {
        if near_left {
            Edge::BottomLeft
        } else if near_right {
            Edge::BottomRight
        } else {
            Edge::Bottom
        }
    } else {
        return None;
    };
    let blocked = match edge {
        Edge::Top => tiled.top,
        Edge::Right => tiled.right,
        Edge::Bottom => tiled.bottom,
        Edge::Left => tiled.left,
        Edge::TopLeft => tiled.top || tiled.left,
        Edge::TopRight => tiled.top || tiled.right,
        Edge::BottomLeft => tiled.bottom || tiled.left,
        Edge::BottomRight => tiled.bottom || tiled.right,
    };
    (!blocked).then_some(edge)
}

/// The areas that resize the window, each with its edge, as
/// `(edge, (x, y, width, height))` in the surface. They are exactly the
/// points [`resize_edge`] maps to that edge, drawn so the pointer can show the
/// matching resize cursor; corners come after edges, so a corner wins where
/// they overlap.
pub fn resize_zones(
    width: f32,
    height: f32,
    inset: Insets,
    tiled: Tiled,
    maximized: bool,
) -> Vec<(Edge, (f32, f32, f32, f32))> {
    if maximized || tiled.all() {
        return Vec::new();
    }
    let h = |side: f32| side.min(RESIZE_HANDLE);
    let (hl, ht, hr, hb) = (h(inset.left), h(inset.top), h(inset.right), h(inset.bottom));
    let left = inset.left;
    let top = inset.top;
    let right = width - inset.right;
    let bottom = height - inset.bottom;
    let full_x = (left - hl, right - left + hl + hr);
    let full_y = (top - ht, bottom - top + ht + hb);
    let mut zones = vec![
        (Edge::Top, (full_x.0, top - ht, full_x.1, ht)),
        (Edge::Bottom, (full_x.0, bottom, full_x.1, hb)),
        (Edge::Left, (left - hl, full_y.0, hl, full_y.1)),
        (Edge::Right, (right, full_y.0, hr, full_y.1)),
    ];
    let corner = |x0: f32, y0: f32, w: f32, h: f32| (x0, y0, w, h);
    // Each corner is the L of its two edges' last RESIZE_CORNER pixels.
    zones.extend([
        (
            Edge::TopLeft,
            corner(left - hl, top - ht, hl + RESIZE_CORNER, ht),
        ),
        (
            Edge::TopLeft,
            corner(left - hl, top - ht, hl, ht + RESIZE_CORNER),
        ),
        (
            Edge::TopRight,
            corner(right - RESIZE_CORNER, top - ht, RESIZE_CORNER + hr, ht),
        ),
        (
            Edge::TopRight,
            corner(right, top - ht, hr, ht + RESIZE_CORNER),
        ),
        (
            Edge::BottomLeft,
            corner(left - hl, bottom, hl + RESIZE_CORNER, hb),
        ),
        (
            Edge::BottomLeft,
            corner(left - hl, bottom - RESIZE_CORNER, hl, RESIZE_CORNER + hb),
        ),
        (
            Edge::BottomRight,
            corner(right - RESIZE_CORNER, bottom, RESIZE_CORNER + hr, hb),
        ),
        (
            Edge::BottomRight,
            corner(right, bottom - RESIZE_CORNER, hr, RESIZE_CORNER + hb),
        ),
    ]);
    zones
        .into_iter()
        .filter(|(edge, (_, _, w, h))| {
            *w > 0.0
                && *h > 0.0
                && match edge {
                    Edge::Top => !tiled.top,
                    Edge::Right => !tiled.right,
                    Edge::Bottom => !tiled.bottom,
                    Edge::Left => !tiled.left,
                    Edge::TopLeft => !(tiled.top || tiled.left),
                    Edge::TopRight => !(tiled.top || tiled.right),
                    Edge::BottomLeft => !(tiled.bottom || tiled.left),
                    Edge::BottomRight => !(tiled.bottom || tiled.right),
                }
        })
        .collect()
}

/// Where the window takes input: the visible window and the resize handles
/// around it, as GTK 4 sets its input region, so a press on the outer shadow
/// reaches whatever is behind the window. `(x, y, width, height)` in the
/// surface.
pub fn input_region(width: f32, height: f32, inset: Insets) -> (f32, f32, f32, f32) {
    let left = (inset.left - RESIZE_HANDLE).max(0.0);
    let top = (inset.top - RESIZE_HANDLE).max(0.0);
    let right = (width - inset.right + RESIZE_HANDLE).min(width);
    let bottom = (height - inset.bottom + RESIZE_HANDLE).min(height);
    (left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

/// How far a window's frame reaches on each side: the inset on every side
/// that is neither tiled nor part of a maximized (or fullscreen) window.
pub fn frame_insets(inset: f32, tiled: Tiled, maximized: bool) -> Insets {
    let side = |tiled: bool| if tiled || maximized { 0.0 } else { inset };
    Insets {
        top: side(tiled.top),
        right: side(tiled.right),
        bottom: side(tiled.bottom),
        left: side(tiled.left),
    }
}

/// The widest shadow a window ever asks room for: the frame image's reach
/// (Chromium's `kMaxFrameSizeDip`). The fallback asks for it too, so GTK's
/// drawing arriving later never has to grow the window's inset.
pub const MAX_SHADOW: f32 = 64.0;

/// On X11 a client cannot narrow its input region, so the transparent
/// shadow would swallow clicks meant for the window behind; the shadow stays
/// within this, as gpui-component's frame did.
pub const X11_MAX_INSET: f32 = 20.0;

/// The inset a window keeps: never smaller than one it already asked for,
/// so a later, narrower drawing does not resize it, and capped on X11.
pub fn sticky_inset(previous: Option<f32>, wanted: f32, x11: bool) -> f32 {
    let inset = previous.map_or(wanted, |previous| previous.max(wanted));
    if x11 { inset.min(X11_MAX_INSET) } else { inset }
}

/// How far the pointer moves with the button down before a press on the
/// title bar becomes a window move: GTK's default `gtk-dnd-drag-threshold`.
pub const DRAG_THRESHOLD: f32 = 8.0;

/// Whether a press at `from` that is now at `to` has become a drag.
pub fn is_drag(from: (f32, f32), to: (f32, f32)) -> bool {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD
}

/// The client inset a window asks the compositor for: the widest side of
/// GTK's shadow, never less than the resize handle, and nothing while the
/// window is maximized. GPUI takes one value for all four sides, so the
/// narrower sides of the shadow leave transparent space instead.
pub fn client_inset(shadow: Insets) -> f32 {
    shadow.max_side().max(RESIZE_HANDLE).ceil()
}

/// An image in straight-alpha RGBA, as GPUI paints it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Bitmap {
    pub fn new(width: u32, height: u32) -> Bitmap {
        Bitmap {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        }
    }

    /// From Cairo's `ARGB32`: premultiplied, one native-endian `u32` a
    /// pixel, rows `stride` bytes apart.
    pub fn from_cairo_argb32(data: &[u8], width: u32, height: u32, stride: usize) -> Bitmap {
        let mut bitmap = Bitmap::new(width, height);
        for y in 0..height as usize {
            for x in 0..width as usize {
                let at = y * stride + x * 4;
                let Some(bytes) = data.get(at..at + 4) else {
                    continue;
                };
                let pixel = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let a = (pixel >> 24) & 0xff;
                let un =
                    |c: u32| -> u8 { (c * 255 + a / 2).checked_div(a).unwrap_or(0).min(255) as u8 };
                let out = (y * width as usize + x) * 4;
                bitmap.rgba[out] = un((pixel >> 16) & 0xff);
                bitmap.rgba[out + 1] = un((pixel >> 8) & 0xff);
                bitmap.rgba[out + 2] = un(pixel & 0xff);
                bitmap.rgba[out + 3] = a as u8;
            }
        }
        bitmap
    }

    pub fn alpha(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.rgba[((y * self.width + x) * 4 + 3) as usize]
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let at = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }

    /// A one-pixel row or column repeated `times` across, so a stretched
    /// strip can be sampled away from its ends: a GPU sampling a one-texel
    /// image blends in whatever sits beside it in the texture atlas.
    pub fn repeat(&self, times: u32, along_x: bool) -> Bitmap {
        let (width, height) = if along_x {
            (self.width * times, self.height)
        } else {
            (self.width, self.height * times)
        };
        let mut out = Bitmap::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = if along_x {
                    (x % self.width.max(1), y)
                } else {
                    (x, y % self.height.max(1))
                };
                let at = ((y * width + x) * 4) as usize;
                out.rgba[at..at + 4].copy_from_slice(&self.pixel(sx, sy));
            }
        }
        out
    }

    /// A rectangle cut out of this image.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Bitmap {
        let mut out = Bitmap::new(width, height);
        for row in 0..height {
            for col in 0..width {
                let at = ((row * width + col) * 4) as usize;
                out.rgba[at..at + 4].copy_from_slice(&self.pixel(x + col, y + row));
            }
        }
        out
    }
}

/// How far the drawn frame reaches outside the window on each side, in
/// pixels, measured from a frame rendered around a window `frame` pixels
/// from each edge of a square image `4 * frame` wide, as Chromium does: the
/// first pixel with any alpha, walking in from the image edge along the
/// middle of each side.
pub fn frame_thickness(image: &Bitmap, frame: u32) -> Insets {
    let size = image.width;
    let middle = 2 * frame;
    let walk = |pixel: &dyn Fn(u32) -> u8| -> f32 {
        (0..frame)
            .find(|i| pixel(*i) > 0)
            .map(|i| (frame - i) as f32)
            .unwrap_or(0.0)
    };
    Insets {
        top: walk(&|i| image.alpha(middle, i)),
        left: walk(&|i| image.alpha(i, middle)),
        bottom: walk(&|i| image.alpha(middle, size - 1 - i)),
        right: walk(&|i| image.alpha(size - 1 - i, middle)),
    }
}

/// The top corner radius of a rendered sample: a window drawn opaque, with
/// only its top-left corner rounded, into the image's top-left. The radius
/// is the first diagonal step at which both the top row and the left column
/// are opaque, as Chromium measures it.
pub fn top_corner_radius(sample: &Bitmap) -> u32 {
    let size = sample.width.min(sample.height);
    (0..size)
        .find(|i| sample.alpha(0, *i) == 255 && sample.alpha(*i, 0) == 255)
        .unwrap_or(size)
}

/// A colour, straight alpha, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(hex: u32) -> Color {
        Color {
            r: ((hex >> 16) & 0xff) as f32 / 255.0,
            g: ((hex >> 8) & 0xff) as f32 / 255.0,
            b: (hex & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const fn alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }
}

/// A frame state the images are drawn for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ButtonState {
    Normal,
    Hover,
    Pressed,
}

impl ButtonState {
    pub const ALL: [ButtonState; 3] = [
        ButtonState::Normal,
        ButtonState::Hover,
        ButtonState::Pressed,
    ];
}

/// What an image set is drawn for: the theme, the scheme, the scale, and
/// whether the window is maximized or tiled. A change to any of them draws
/// new images; the cache keeps the old set for when it comes back.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FrameKey {
    pub gtk_theme: Option<String>,
    pub icon_theme: Option<String>,
    pub dark: bool,
    /// GTK draws at whole scales; a fractional scale is drawn at the next
    /// whole one up and scaled down when painted.
    pub scale: u32,
    pub maximized: bool,
    pub tiled: bool,
}

impl FrameKey {
    /// The other drawings a window with this key is likely to need soon: the
    /// same theme maximized, tiled, restored, and in the other appearance.
    /// They are drawn after the first, so maximizing or switching between
    /// light and dark never has to wait for GTK.
    pub fn siblings(&self) -> Vec<FrameKey> {
        let mut keys = Vec::new();
        for (maximized, tiled) in [(false, false), (true, false), (false, true)] {
            for dark in [self.dark, !self.dark] {
                let key = FrameKey {
                    dark,
                    maximized,
                    tiled,
                    ..self.clone()
                };
                if key != *self && !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }
}

/// What a window's frame state is: whether it draws its own frame, which
/// sides touch something, and whether it fills the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    pub client_decorated: bool,
    pub tiled: Tiled,
    /// Maximized, fullscreen, or tiled on every side: no frame at all.
    pub maximized: bool,
    pub active: bool,
}

impl WindowState {
    /// `tiling` is `None` for a window the compositor decorates.
    pub fn new(tiling: Option<Tiled>, maximized: bool, fullscreen: bool, active: bool) -> Self {
        match tiling {
            None => WindowState {
                client_decorated: false,
                tiled: Tiled::default(),
                maximized: maximized || fullscreen,
                active,
            },
            Some(tiled) => WindowState {
                client_decorated: true,
                tiled,
                maximized: maximized || fullscreen || tiled.all(),
                active,
            },
        }
    }

    /// The same window restored: what its inset is measured from.
    pub fn restored(self) -> Self {
        WindowState {
            tiled: Tiled::default(),
            maximized: false,
            ..self
        }
    }

    /// The key its frame is drawn for. `dark` is the window's own
    /// appearance, which follows the desktop's `color-scheme` when the person
    /// chose "system".
    pub fn key(self, settings: &DesktopSettings, dark: bool, scale: f32) -> FrameKey {
        FrameKey {
            gtk_theme: settings.gtk_theme.clone(),
            icon_theme: settings.icon_theme.clone(),
            dark,
            scale: render_scale(scale),
            maximized: self.maximized,
            tiled: self.tiled.any() && !self.maximized,
        }
    }
}

/// How many texels a stretched strip image has along its length; it is
/// sampled only between the middle two (see [`strip_image_span`]).
pub const STRIP_TEXELS: u32 = 4;

/// Where to place a strip image so that `length` pixels starting at `start`
/// sample only its middle texel: returns the image's start and length along
/// the stretch.
pub fn strip_image_span(start: f32, length: f32) -> (f32, f32) {
    let texels = STRIP_TEXELS as f32;
    (start - (texels / 2.0 - 0.5) * length, texels * length)
}

/// The whole scale GTK draws at for a window `scale`.
pub fn render_scale(scale: f32) -> u32 {
    if scale.is_finite() && scale > 0.0 {
        (scale.ceil() as u32).clamp(1, 4)
    } else {
        1
    }
}

/// The look used when GTK cannot be loaded: Adwaita's, as libadwaita 1.5.0
/// draws a window (its `base.css`, `defaults-light.css`, and
/// `defaults-dark.css`, read from the library on 2026-11-14): 12px window
/// corners, a 47px header bar with a 1px shade under it, title buttons whose
/// round 24px image takes 10% of the text colour, 15% under the pointer and
/// 30% pressed, and a header whose contents fade to half while the window is
/// in the background. Never the old Windows look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallbackLook {
    pub corner_radius: f32,
    pub header_height: f32,
    pub header: Color,
    pub header_backdrop: Color,
    pub header_shade: Color,
    pub text: Color,
    /// `window.csd`'s `0 0 0 1px` outline shadow.
    pub border: Color,
    /// The button's box (24px wide plus 5px padding each side).
    pub button_size: f32,
    /// The round image inside it: the button's 24px content box.
    pub button_image: f32,
    pub button_spacing: f32,
    /// The image fill at rest, under the pointer, and pressed, as a fraction
    /// of the text colour.
    pub button_fill: [f32; 3],
    /// How much of the header's contents shows while the window is not
    /// focused.
    pub backdrop_opacity: f32,
    /// The room kept for the shadow past the window on each side: as much as
    /// GTK's drawing may ever need ([`MAX_SHADOW`]), so its arrival does not
    /// change the window's inset.
    pub shadow: Insets,
}

const ADWAITA_SHADOW: Insets = Insets {
    top: MAX_SHADOW,
    right: MAX_SHADOW,
    bottom: MAX_SHADOW,
    left: MAX_SHADOW,
};

/// Adwaita in light.
pub const ADWAITA_LIGHT: FallbackLook = FallbackLook {
    corner_radius: 12.0,
    header_height: 47.0,
    header: Color::rgb(0xffffff),
    header_backdrop: Color::rgb(0xfafafa),
    header_shade: Color::rgb(0x000000).alpha(0.12),
    text: Color::rgb(0x000000).alpha(0.8),
    border: Color::rgb(0x000000).alpha(0.05),
    button_size: 34.0,
    button_image: 24.0,
    button_spacing: 3.0,
    button_fill: [0.1, 0.15, 0.3],
    backdrop_opacity: 0.5,
    shadow: ADWAITA_SHADOW,
};

/// Adwaita in dark.
pub const ADWAITA_DARK: FallbackLook = FallbackLook {
    corner_radius: 12.0,
    header_height: 47.0,
    header: Color::rgb(0x303030),
    header_backdrop: Color::rgb(0x242424),
    header_shade: Color::rgb(0x000000).alpha(0.36),
    text: Color::rgb(0xffffff),
    border: Color::rgb(0x000000).alpha(0.05),
    button_size: 34.0,
    button_image: 24.0,
    button_spacing: 3.0,
    button_fill: [0.1, 0.15, 0.3],
    backdrop_opacity: 0.5,
    shadow: ADWAITA_SHADOW,
};

pub fn fallback_look(dark: bool) -> &'static FallbackLook {
    if dark { &ADWAITA_DARK } else { &ADWAITA_LIGHT }
}

/// Why the frame is not drawn by GTK.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FallbackReason {
    /// `libgtk-4.so.1` could not be opened or lacks a symbol the frame uses.
    NoGtk(String),
    /// GTK could not connect to the display.
    GtkInitFailed,
    /// Set by `BETTER_OS_FRAME=adwaita`, for comparing the two.
    Requested,
    /// Not a Linux build.
    Unsupported,
    /// The thread that draws with GTK could not start or stopped.
    RendererStopped,
}

/// The environment variable that forces the built-in Adwaita drawing.
pub const FORCE_FALLBACK_VAR: &str = "BETTER_OS_FRAME";

/// Whether the environment asks for the built-in drawing.
pub fn fallback_requested(value: Option<&str>) -> bool {
    matches!(value, Some("adwaita") | Some("fallback"))
}
