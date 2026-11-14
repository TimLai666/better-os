//! Shared presentation for every Better OS window: the WinUI 3 control kit in
//! [`fluent`], the window chrome and options, the command-line contract, and
//! the older status, badge, and application primitives.

pub mod command_line;
pub mod fluent;
pub mod restart;
pub mod system_frame;

use better_core::ComponentManifest;
use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, StyledImage,
    Window, WindowDecorations, WindowOptions, div,
};
use gpui_component::*;

/// The window chrome every first-party Better OS window draws for itself.
///
/// GNOME's Mutter offers no server-side decorations to an `xdg-toplevel`
/// client, so a window has no frame, no close, minimize or maximize button and
/// cannot be dragged or resized unless the application draws them. Since
/// ticket 108 the kit draws them the way the desktop does: GTK renders the
/// frame, shadow, header, and window buttons from the current theme
/// ([`crate::system_frame`]), [`root`] puts the frame around a window's view,
/// and [`fluent::chrome::TitleBar`] is the title bar.
pub mod window_chrome {
    use super::*;

    pub use crate::fluent::window_frame::{follow_desktop_accent, root, watch_desktop};

    /// The titlebar for a window whose header is its glyph and title: the
    /// kit's [`fluent::chrome::TitleBar`], with the desktop's buttons.
    ///
    /// `icon` is an element rather than a name so each application passes the
    /// glyph it already uses, and the bar never grows a table of application
    /// identities of its own. The title takes the header's own text colour, so
    /// `_foreground` is no longer used; it stays so callers need not change.
    pub fn title_bar(
        icon: impl IntoElement,
        title: impl Into<SharedString>,
        _foreground: Hsla,
    ) -> fluent::chrome::TitleBar {
        fluent::chrome::TitleBar::new(title).icon(icon)
    }

    /// The window options every decorated first-party window opens with.
    ///
    /// Two of these matter on Wayland and neither was set before. `app_id` is
    /// how the compositor matches a window to its desktop entry — without it
    /// the shell cannot find the application's icon or name no matter what the
    /// package installs. `WindowDecorations::Client` states the intent the
    /// drawn titlebar depends on rather than leaving it to a negotiation the
    /// compositor was always going to answer the same way.
    ///
    /// `app_id` must be the desktop entry's file name without its `.desktop`
    /// suffix, or the match silently fails.
    pub fn window_options(app_id: impl Into<String>) -> WindowOptions {
        // Start reading the theme now, so GTK has drawn the frame by the
        // window's first frame.
        crate::system_frame::service::FrameService::warm_up();
        WindowOptions {
            titlebar: Some(gpui_component::TitleBar::title_bar_options()),
            window_decorations: Some(WindowDecorations::Client),
            app_id: Some(app_id.into()),
            ..Default::default()
        }
    }

    /// The window options for a transient overlay that draws no titlebar.
    ///
    /// Better Launcher is the one first-party window with no title bar, and it
    /// is deliberate: it is a near-fullscreen overlay a person summons, uses
    /// once and dismisses with Escape or by launching something. A titlebar
    /// would give it a second way to close, a drag region it should not have,
    /// and a maximize button for a window that is already the size it wants.
    /// It still needs `app_id`, because the shell matches a window to its
    /// desktop entry — and therefore to its icon and name — by that string
    /// whether or not the window is decorated.
    pub fn overlay_window_options(app_id: impl Into<String>) -> WindowOptions {
        WindowOptions {
            window_decorations: Some(WindowDecorations::Client),
            app_id: Some(app_id.into()),
            ..Default::default()
        }
    }

    /// Whether the window is currently drawing its own decorations.
    ///
    /// A window that somehow did get server-side decorations would otherwise
    /// show two titlebars.
    pub fn is_client_decorated(window: &Window, _: &App) -> bool {
        matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusCard {
    pub title: String,
    pub value: String,
    pub detail: String,
}

pub fn component_card(manifest: &ComponentManifest) -> StatusCard {
    StatusCard {
        title: manifest.display_name.clone(),
        value: manifest.version.to_string(),
        detail: format!("{} component", manifest.component_type_label()),
    }
}

/// Shared status card primitive used by manager and monitor shells.
pub fn status_card(
    title: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> impl IntoElement {
    div().v_flex().child(title.into()).child(value.into())
}

/// Shared root heading primitive used by both first-party desktop applications.
pub fn page_heading(title: impl Into<SharedString>) -> impl IntoElement {
    div().child(title.into())
}

/// Theme-aware surface primitive shared by first-party desktop applications.
pub fn surface(
    child: impl IntoElement,
    border: Hsla,
    background: Hsla,
    radius: Pixels,
) -> impl IntoElement {
    div()
        .min_w_0()
        .p_4()
        .rounded(radius)
        .border_1()
        .border_color(border)
        .bg(background)
        .child(child)
}

/// The affordance convention every first-party Better OS window follows.
///
/// **Anything that looks clickable must be clickable, and anything that is not
/// clickable must not look like a button.** Stated as two closed cases:
///
/// - **Action.** Something a person can do. It is a `Button`, a `Checkbox`, a
///   `Switch`, a sidebar menu item, or a `div` that carries both a click
///   handler and a hover state. It may use the theme's action colors
///   (`primary`, `danger`) as a fill, and it always has a handler. A control
///   that is momentarily unavailable is a *disabled* action, not a label — it
///   must still name something the person could otherwise do.
/// - **Status.** Something a person can only read. It is [`StatusPill`], a
///   [`badge`], or plain text. It has **no** hover state, **no** cursor
///   change, **no** click handler, and **no** saturated fill in an action
///   color. It is tinted and outlined instead, so a status can be colour-coded
///   without borrowing the shape and weight of a button.
///
/// The failure mode this rule exists to prevent is real and was reported from
/// a running desktop: a filled red status chip sits next to a filled red
/// `danger` button, both are pill-shaped, both change under the pointer, and
/// only one of them does anything.
///
/// `gpui_component::Tag` cannot satisfy the Status case. Its filled variants
/// paint `cx.theme().danger` / `primary` / `success` — the exact tokens
/// `gpui_component::button::Button` paints for the same variants — and its
/// render adds `.hover(|this| this.opacity(0.9))` unconditionally, after the
/// caller's own style refinement, so a caller cannot switch it off. Use
/// [`StatusPill`] for a status indicator in a first-party window.
///
/// [`Affordance`] makes the distinction a type rather than a convention a
/// reviewer has to remember: a view model that classifies its own indicators
/// can be asserted against in a test with no window open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Affordance {
    /// The person can do this. It has a handler.
    Action,
    /// The person can only read this. It has no handler and no button chrome.
    Status,
}

/// What a status indicator means, independent of any theme.
///
/// A tone is not a color. The window maps it to one through
/// [`StatusPill::render`] so a primitive never carries a palette of its own,
/// and so a tone can be asserted in a test that never opens a window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusTone {
    /// Nothing notable. The default reading.
    Neutral,
    /// In progress, or worth knowing without being good or bad.
    Info,
    /// Working as intended.
    Success,
    /// Needs attention but still functioning.
    Warning,
    /// Broken, refused, or unavailable.
    Danger,
}

/// One read-only status indicator: what it says, and how it reads.
///
/// This is a view model, not an element. It carries no colors and no GPUI
/// state, which is the point — a screen's status indicators can be enumerated
/// and asserted in a unit test, and [`Self::AFFORDANCE`] states in the type
/// system that none of them is ever an action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusPill {
    pub label: SharedString,
    pub tone: StatusTone,
}

impl StatusPill {
    /// A status pill is never an action. This constant is what a test asserts
    /// against so the claim cannot quietly stop being true.
    pub const AFFORDANCE: Affordance = Affordance::Status;

    pub fn new(label: impl Into<SharedString>, tone: StatusTone) -> Self {
        Self {
            label: label.into(),
            tone,
        }
    }

    /// Draw the pill with colors the calling window derived from its theme.
    ///
    /// This is [`badge`], and deliberately so: one non-interactive chip
    /// primitive, so a status indicator and an application badge cannot drift
    /// into looking like two different kinds of thing.
    pub fn render(&self, style: BadgeStyle) -> impl IntoElement {
        badge(self.label.clone(), style)
    }
}

/// How a badge reads. The colors come from the calling shell's theme so a
/// primitive never carries a palette of its own.
///
/// A `BadgeStyle` for a status indicator must not use an action fill. Tint the
/// background and let the border and the text carry the tone — see
/// [`Affordance`].
#[derive(Clone, Copy, Debug)]
pub struct BadgeStyle {
    pub foreground: Hsla,
    pub background: Hsla,
    pub border: Hsla,
}

/// A small labelled chip. Both the application source badge and the MIME
/// compatibility badge the app chooser shows are this primitive with different
/// styles, so they cannot drift apart visually.
///
/// **Non-interactive by construction.** It registers no hover state, no cursor
/// change and no click handler, and it must stay that way: it is the Status
/// half of [`Affordance`], and every status indicator in every first-party
/// window is drawn by it.
pub fn badge(label: impl Into<SharedString>, style: BadgeStyle) -> impl IntoElement {
    div()
        .px_2()
        .py_0p5()
        .rounded_full()
        .border_1()
        .border_color(style.border)
        .bg(style.background)
        .text_xs()
        .text_color(style.foreground)
        .child(label.into())
}

/// What stands for an application in a tile or a row: the icon its
/// desktop entry names, resolved to a file, or a letter when there is no
/// icon or the file cannot be drawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppIcon {
    /// An icon file (PNG or SVG), with the letter to draw if it fails to load.
    File {
        path: std::path::PathBuf,
        fallback: SharedString,
    },
    Letter(SharedString),
}

impl AppIcon {
    /// The icon for a desktop entry's `Icon=` value: an absolute path is used
    /// as it is; a name is looked up in the icon theme directories; the
    /// application name's first letter stands in when neither finds a file.
    pub fn for_entry(icon: Option<&str>, name: &str, search: &icons::IconSearch) -> AppIcon {
        let fallback = icons::initial(name);
        match icon.and_then(|icon| search.resolve(icon)) {
            Some(path) => AppIcon::File { path, fallback },
            None => AppIcon::Letter(fallback),
        }
    }

    fn render(&self, size: Pixels, style: &TileStyle) -> AnyElement {
        self.element(
            size,
            style.glyph_foreground,
            style.glyph_background,
            style.radius,
        )
    }

    /// The icon at `size`: the file, or the letter on a rounded square in
    /// the given colors when there is no file or it cannot be drawn.
    pub fn element(
        &self,
        size: Pixels,
        glyph_foreground: Hsla,
        glyph_background: Hsla,
        radius: Pixels,
    ) -> AnyElement {
        let letter = move |glyph: SharedString| {
            div()
                .size(size)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(radius)
                .bg(glyph_background)
                .text_color(glyph_foreground)
                .text_size(size * 0.5)
                .child(glyph)
                .into_any_element()
        };
        match self {
            AppIcon::Letter(glyph) => letter(glyph.clone()),
            AppIcon::File { path, fallback } => {
                let fallback = fallback.clone();
                gpui::img(path.clone())
                    .size(size)
                    .flex_none()
                    .object_fit(gpui::ObjectFit::Contain)
                    .with_fallback(move || letter(fallback.clone()))
                    .into_any_element()
            }
        }
    }

    /// The letter this icon falls back to.
    pub fn fallback(&self) -> &SharedString {
        match self {
            AppIcon::File { fallback, .. } | AppIcon::Letter(fallback) => fallback,
        }
    }
}

/// Finding the file an application's `Icon=` names, the way the freedesktop
/// icon theme specification does for the cases Better OS needs: the desktop's
/// own icon theme and every theme it inherits from, then `hicolor`, then
/// `pixmaps`. A theme's application directories come from its `index.theme`
/// (`Directories=` with `Context=Applications`); a theme without one is
/// searched at the usual sizes.
pub mod icons {
    use std::path::{Path, PathBuf};

    use gpui::SharedString;

    /// The sizes looked in when a theme has no `index.theme`, largest useful
    /// first. Scalable comes first because it draws sharp at any size.
    pub const SIZES: [&str; 8] = [
        "scalable", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32", "24x24",
    ];

    /// The fixed size looked for first: a launcher draws icons at 24 to 64
    /// pixels.
    const PREFERRED_SIZE: u32 = 48;

    /// The theme searched after the desktop's own chain and before
    /// `hicolor`: GNOME's, which most application icons a GNOME desktop
    /// shows fall back to.
    pub const FALLBACK_THEME: &str = "Adwaita";

    /// Where the name of the desktop's icon theme comes from.
    pub trait IconThemeSource {
        fn icon_theme(&self) -> Option<String>;
    }

    /// GNOME's own setting, `org.gnome.desktop.interface icon-theme`, which
    /// is what GNOME and Zorin set and what GTK applications follow there.
    /// Read through `gsettings`, the GLib tool that reads it from whichever
    /// backend the session uses; read only, never written.
    pub struct GnomeInterfaceSetting;

    impl IconThemeSource for GnomeInterfaceSetting {
        fn icon_theme(&self) -> Option<String> {
            let output = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "icon-theme"])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            parse_gsettings_string(&String::from_utf8_lossy(&output.stdout))
        }
    }

    /// `gtk-icon-theme-name` from GTK's `settings.ini`, for a desktop that is
    /// not GNOME.
    pub struct GtkSettingsIni {
        pub config: Option<PathBuf>,
    }

    impl IconThemeSource for GtkSettingsIni {
        fn icon_theme(&self) -> Option<String> {
            gtk_icon_theme(self.config.as_deref()?)
        }
    }

    /// A theme name fixed in advance, for tests.
    pub struct FixedTheme(pub Option<String>);

    impl IconThemeSource for FixedTheme {
        fn icon_theme(&self) -> Option<String> {
            self.0.clone()
        }
    }

    /// What `gsettings get` prints for a string: `'ZorinBlue-Light'`.
    pub fn parse_gsettings_string(text: &str) -> Option<String> {
        let text = text.trim();
        let inner = text
            .strip_prefix('\'')
            .and_then(|t| t.strip_suffix('\''))
            .or_else(|| text.strip_prefix('"').and_then(|t| t.strip_suffix('"')))
            .unwrap_or(text);
        (!inner.is_empty() && valid_name(inner)).then(|| inner.to_string())
    }

    /// How many themes an inheritance chain may hold. A theme that inherits
    /// from itself, or a long chain, stops here rather than looping.
    const MAX_THEMES: usize = 8;

    /// Where to look.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct IconSearch {
        /// `$XDG_DATA_HOME` and every `$XDG_DATA_DIRS` entry, in order.
        pub data_dirs: Vec<PathBuf>,
        /// Theme names to try before `hicolor`, such as the desktop's own and
        /// the themes it inherits from, in order.
        pub themes: Vec<String>,
        /// Each theme's application directories, read once from its
        /// `index.theme`.
        theme_dirs: Vec<(String, Vec<String>)>,
    }

    impl IconSearch {
        /// A search over these data directories and themes, with every
        /// theme's `index.theme` read now rather than once per icon.
        pub fn new(data_dirs: Vec<PathBuf>, themes: Vec<String>) -> IconSearch {
            let mut search = IconSearch {
                data_dirs,
                themes,
                theme_dirs: Vec::new(),
            };
            let names: Vec<String> = search
                .themes
                .iter()
                .cloned()
                .chain(std::iter::once("hicolor".to_string()))
                .collect();
            for name in names {
                if let Some(dirs) = search.read_theme(&name).map(|theme| theme.app_dirs) {
                    search.theme_dirs.push((name, dirs));
                }
            }
            search
        }

        /// The process's data directories, as the XDG base directory rule
        /// defines them, and the icon theme the desktop's GTK settings name,
        /// followed through what it inherits.
        pub fn from_environment() -> IconSearch {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let data_home = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
            let data_dirs = std::env::var("XDG_DATA_DIRS")
                .ok()
                .filter(|dirs| !dirs.is_empty())
                .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
            let mut dirs: Vec<PathBuf> = data_home.into_iter().collect();
            dirs.extend(
                data_dirs
                    .split(':')
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute()),
            );
            let config = crate::fluent::theme::config_home(
                std::env::var_os("XDG_CONFIG_HOME"),
                std::env::var_os("HOME"),
            );
            let gtk = GtkSettingsIni { config };
            IconSearch::with_theme_from(dirs, &[&GnomeInterfaceSetting, &gtk])
        }

        /// A search over these data directories whose theme is the first
        /// one any of `sources` names, followed through what it inherits,
        /// then Adwaita (the theme GNOME's own icons fall back to) before
        /// `hicolor`.
        pub fn with_theme_from(
            data_dirs: Vec<PathBuf>,
            sources: &[&dyn IconThemeSource],
        ) -> IconSearch {
            let base = IconSearch::new(data_dirs.clone(), Vec::new());
            let mut themes = sources
                .iter()
                .find_map(|source| source.icon_theme())
                .map(|name| base.theme_chain(&name))
                .unwrap_or_default();
            if !themes.iter().any(|theme| theme == FALLBACK_THEME) {
                themes.push(FALLBACK_THEME.to_string());
            }
            IconSearch::new(data_dirs, themes)
        }

        /// A theme and everything it inherits, breadth first, without
        /// `hicolor` (which every search ends with) and without repeats.
        pub fn theme_chain(&self, name: &str) -> Vec<String> {
            let mut chain: Vec<String> = Vec::new();
            let mut queue = std::collections::VecDeque::from([name.to_string()]);
            while let Some(theme) = queue.pop_front() {
                if chain.len() >= MAX_THEMES {
                    break;
                }
                if theme == "hicolor" || theme.is_empty() || chain.contains(&theme) {
                    continue;
                }
                if !valid_name(&theme) {
                    continue;
                }
                let inherits = self
                    .read_theme(&theme)
                    .map(|index| index.inherits)
                    .unwrap_or_default();
                chain.push(theme);
                queue.extend(inherits);
            }
            chain
        }

        /// A theme's `index.theme`, from the first data directory that has
        /// one.
        fn read_theme(&self, name: &str) -> Option<ThemeIndex> {
            if !valid_name(name) {
                return None;
            }
            self.data_dirs.iter().find_map(|dir| {
                std::fs::read_to_string(dir.join("icons").join(name).join("index.theme"))
                    .ok()
                    .map(|text| ThemeIndex::parse(&text))
            })
        }

        /// The application directories searched in one theme.
        fn dirs_for(&self, theme: &str) -> Vec<String> {
            self.theme_dirs
                .iter()
                .find(|(name, _)| name == theme)
                .map(|(_, dirs)| dirs.clone())
                .unwrap_or_else(|| SIZES.iter().map(|size| format!("{size}/apps")).collect())
        }

        /// Every path tried for `name`, in order. Separate from
        /// [`IconSearch::resolve`] so the order can be tested.
        pub fn candidates(&self, name: &str) -> Vec<PathBuf> {
            let mut paths = Vec::new();
            let themes: Vec<&str> = self
                .themes
                .iter()
                .map(String::as_str)
                .chain(std::iter::once("hicolor"))
                .collect();
            for theme in &themes {
                let dirs = self.dirs_for(theme);
                for dir in &self.data_dirs {
                    for sub in &dirs {
                        for extension in ["svg", "png"] {
                            if sub.starts_with("scalable") && extension == "png" {
                                continue;
                            }
                            paths.push(
                                dir.join("icons")
                                    .join(theme)
                                    .join(sub)
                                    .join(format!("{name}.{extension}")),
                            );
                        }
                    }
                }
            }
            for dir in &self.data_dirs {
                for extension in ["svg", "png"] {
                    paths.push(dir.join("pixmaps").join(format!("{name}.{extension}")));
                }
            }
            paths
        }

        /// Every path tried for `name` in one icon theme context other than
        /// applications, such as `mimetypes` for a file type or `places` for
        /// a folder: each theme of the search, then `hicolor`, at the usual
        /// sizes, scalable first.
        pub fn candidates_in(&self, context: &str, name: &str) -> Vec<PathBuf> {
            let mut paths = Vec::new();
            let themes: Vec<&str> = self
                .themes
                .iter()
                .map(String::as_str)
                .chain(std::iter::once("hicolor"))
                .collect();
            for theme in &themes {
                for dir in &self.data_dirs {
                    for size in SIZES {
                        for extension in ["svg", "png"] {
                            if size == "scalable" && extension == "png" {
                                continue;
                            }
                            paths.push(
                                dir.join("icons")
                                    .join(theme)
                                    .join(size)
                                    .join(context)
                                    .join(format!("{name}.{extension}")),
                            );
                        }
                    }
                }
            }
            paths
        }

        /// The file for a themed icon name in one context, or `None`. Neither
        /// the name nor the context may be a path.
        pub fn resolve_in(&self, context: &str, name: &str) -> Option<PathBuf> {
            let name = name.trim();
            if name.is_empty() || !valid_name(name) || !valid_name(context) || context.is_empty() {
                return None;
            }
            self.candidates_in(context, name)
                .into_iter()
                .find(|path| path.is_file())
        }

        /// The file for an `Icon=` value, or `None`. An absolute path must
        /// exist; a name must be a plain file name, never a path, so a
        /// desktop entry cannot point the lookup outside the icon
        /// directories.
        pub fn resolve(&self, icon: &str) -> Option<PathBuf> {
            let icon = icon.trim();
            if icon.is_empty() {
                return None;
            }
            let as_path = Path::new(icon);
            if as_path.is_absolute() {
                return acceptable_icon_file(as_path).then(|| as_path.to_path_buf());
            }
            if !valid_name(icon) {
                return None;
            }
            // `Icon=firefox.png` is tolerated by most desktops: drop a known
            // extension and look the name up.
            let name = icon
                .strip_suffix(".png")
                .or_else(|| icon.strip_suffix(".svg"))
                .unwrap_or(icon);
            self.candidates(name)
                .into_iter()
                .find(|path| path.is_file())
        }
    }

    /// The largest icon file an `Icon=` path may name. A desktop entry is a
    /// file anyone can install; an icon is never this big.
    pub const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;

    /// The largest PNG an `Icon=` path may name, across and down; decoded
    /// at full size, anything larger costs memory for nothing.
    pub const MAX_ICON_PIXELS: u32 = 1024;

    /// Whether an absolute `Icon=` path from a desktop entry, which anyone
    /// can install, is drawn: a regular PNG, SVG or XPM file (by extension
    /// and, for a PNG, by its header), at most [`MAX_ICON_BYTES`], a PNG at
    /// most [`MAX_ICON_PIXELS`] each way, and never under `/proc`, `/sys` or
    /// `/dev`. Reads the file's metadata and first bytes.
    pub fn acceptable_icon_file(path: &Path) -> bool {
        if ["/proc", "/sys", "/dev"]
            .iter()
            .any(|kernel| path.starts_with(kernel))
        {
            return false;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        let Some(extension) =
            extension.filter(|extension| matches!(extension.as_str(), "png" | "svg" | "xpm"))
        else {
            return false;
        };
        // `metadata` follows a link, and the file it lands on is what is
        // judged: a regular file, not a device or a fifo.
        let Ok(metadata) = std::fs::metadata(path) else {
            return false;
        };
        if !metadata.is_file() || metadata.len() > MAX_ICON_BYTES {
            return false;
        }
        if extension == "png" {
            return png_fits(path);
        }
        true
    }

    /// Whether a PNG's header says it is at most [`MAX_ICON_PIXELS`] square.
    fn png_fits(path: &Path) -> bool {
        use std::io::Read;
        let mut header = [0u8; 24];
        let read = std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut header));
        if read.is_err() || &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
            return false;
        }
        let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]);
        let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]);
        width > 0 && height > 0 && width <= MAX_ICON_PIXELS && height <= MAX_ICON_PIXELS
    }

    /// A theme directory entry that stays inside the theme: relative, with
    /// no `..` and no empty or `.` part. An `index.theme` is a file anyone
    /// can install in `~/.local/share/icons`; a `Directories=/` or `../..`
    /// entry must not send the lookup elsewhere.
    pub fn safe_subdirectory(entry: &str) -> bool {
        !entry.starts_with('/')
            && !entry.contains('\\')
            && entry
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
    }

    /// A plain file name: no separator, and not `.` or `..`.
    fn valid_name(name: &str) -> bool {
        !(name.contains('/') || name.contains('\\') || name == "." || name == "..")
    }

    /// What Better OS reads from a theme's `index.theme`.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct ThemeIndex {
        /// `Inherits=`, in order.
        pub inherits: Vec<String>,
        /// The `Directories=` (and `ScaledDirectories=`) entries whose
        /// `Context` is `Applications`, scalable first, then the largest
        /// fixed size down, scaled (`@2x`) ones after their plain size.
        pub app_dirs: Vec<String>,
    }

    impl ThemeIndex {
        pub fn parse(text: &str) -> ThemeIndex {
            let mut inherits = Vec::new();
            let mut listed: Vec<String> = Vec::new();
            let mut section = String::new();
            // (directory, context, size, scalable)
            let mut details: Vec<(String, String, u32, bool, u32)> = Vec::new();
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    section = name.to_string();
                    if section != "Icon Theme" {
                        details.push((section.clone(), String::new(), 0, false, 1));
                    }
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let (key, value) = (key.trim(), value.trim());
                if section == "Icon Theme" {
                    match key {
                        "Inherits" => inherits.extend(
                            value
                                .split(',')
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(String::from),
                        ),
                        "Directories" | "ScaledDirectories" => listed.extend(
                            value
                                .split(',')
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .filter(|v| safe_subdirectory(v))
                                .map(String::from),
                        ),
                        _ => {}
                    }
                } else if let Some(entry) = details.last_mut() {
                    match key {
                        "Context" => entry.1 = value.to_string(),
                        "Size" => entry.2 = value.parse().unwrap_or(0),
                        "Type" => entry.3 = value.eq_ignore_ascii_case("Scalable"),
                        "Scale" => entry.4 = value.parse().unwrap_or(1),
                        _ => {}
                    }
                }
            }
            let mut apps: Vec<(String, u32, bool, u32)> = details
                .into_iter()
                .filter(|(dir, context, ..)| {
                    listed.contains(dir) && context.eq_ignore_ascii_case("Applications")
                })
                .map(|(dir, _, size, scalable, scale)| (dir, size, scalable, scale))
                .collect();
            // Scalable first; then the smallest size that is still at
            // least 48px, upward, because a 512px bitmap or drawing costs
            // far more to load than a launcher row needs; then the sizes
            // below 48, largest first.
            let rank = |size: u32| {
                if size >= PREFERRED_SIZE {
                    (0, size)
                } else {
                    (1, u32::MAX - size)
                }
            };
            apps.sort_by(|a, b| {
                b.2.cmp(&a.2)
                    .then(rank(a.1).cmp(&rank(b.1)))
                    .then(a.3.cmp(&b.3))
                    .then(a.0.cmp(&b.0))
            });
            ThemeIndex {
                inherits,
                app_dirs: apps.into_iter().map(|(dir, ..)| dir).collect(),
            }
        }
    }

    /// The icon theme GTK is set to, from `gtk-4.0/settings.ini` or
    /// `gtk-3.0/settings.ini` under the configuration directory.
    pub fn gtk_icon_theme(config: &Path) -> Option<String> {
        ["gtk-4.0", "gtk-3.0"].iter().find_map(|version| {
            let text = std::fs::read_to_string(config.join(version).join("settings.ini")).ok()?;
            text.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "gtk-icon-theme-name")
                    .then(|| value.trim().trim_matches('"').to_string())
                    .filter(|value| !value.is_empty())
            })
        })
    }

    /// The letter that stands for an application: the first alphanumeric
    /// character of its name, upper-cased, or `?` for a name with none.
    pub fn initial(name: &str) -> SharedString {
        name.chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_else(|| "?".to_string())
            .into()
    }
}

/// The grid presentation of one application: icon glyph, name, and the badges
/// the caller decided to show.
pub fn application_tile(
    glyph: impl Into<SharedString>,
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    badges: Vec<AnyElement>,
    style: TileStyle,
) -> impl IntoElement {
    application_tile_with_icon(AppIcon::Letter(glyph.into()), name, detail, badges, style)
}

/// [`application_tile`] with the application's real icon, falling back to
/// its letter.
pub fn application_tile_with_icon(
    icon: AppIcon,
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    badges: Vec<AnyElement>,
    style: TileStyle,
) -> impl IntoElement {
    div()
        .v_flex()
        .min_w_0()
        .gap_2()
        .p_3()
        .rounded(style.radius)
        .border_1()
        .border_color(style.border)
        .bg(style.background)
        .child(icon.render(gpui::px(40.0), &style))
        .child(
            div()
                .min_w_0()
                .text_sm()
                .font_semibold()
                .text_color(style.foreground)
                .child(name.into()),
        )
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(style.muted_foreground)
                .child(detail.into()),
        )
        .child(div().flex().flex_wrap().gap_1().children(badges))
}

/// The list presentation of one application. Same information as the tile, laid
/// out for scanning a long list rather than for a grid.
pub fn application_list_row(
    glyph: impl Into<SharedString>,
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    badges: Vec<AnyElement>,
    style: TileStyle,
) -> impl IntoElement {
    application_list_row_with_icon(AppIcon::Letter(glyph.into()), name, detail, badges, style)
}

/// [`application_list_row`] with the application's real icon, falling back to
/// its letter.
pub fn application_list_row_with_icon(
    icon: AppIcon,
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    badges: Vec<AnyElement>,
    style: TileStyle,
) -> impl IntoElement {
    div()
        .flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(style.radius)
        .border_1()
        .border_color(style.border)
        .bg(style.background)
        .child(icon.render(gpui::px(32.0), &style))
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .font_semibold()
                        .text_color(style.foreground)
                        .child(name.into()),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(style.muted_foreground)
                        .child(detail.into()),
                ),
        )
        .child(div().flex().flex_wrap().gap_1().children(badges))
}

/// The colors an application tile or row uses, including its selected state.
/// A selected item differs by border and background rather than by a color the
/// primitive invented.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileStyle {
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub border: Hsla,
    pub background: Hsla,
    pub glyph_foreground: Hsla,
    pub glyph_background: Hsla,
    pub radius: Pixels,
}

/// A centered state message: nothing found, still loading, or an operation that
/// failed. One primitive so all three read the same way.
pub fn state_message(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    foreground: Hsla,
    muted_foreground: Hsla,
) -> impl IntoElement {
    div()
        .v_flex()
        .w_full()
        .items_center()
        .gap_2()
        .py_10()
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(foreground)
                .child(title.into()),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted_foreground)
                .child(detail.into()),
        )
}

/// A full-width notice bar. Used for a launch failure and for the explanation
/// shown when a chosen application does not declare the selected file type.
pub fn notice(
    message: impl Into<SharedString>,
    foreground: Hsla,
    background: Hsla,
    radius: Pixels,
) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .px_3()
        .py_2()
        .rounded(radius)
        .bg(background)
        .text_sm()
        .text_color(foreground)
        .child(message.into())
}

trait ComponentTypeLabel {
    fn component_type_label(&self) -> &'static str;
}

impl ComponentTypeLabel for ComponentManifest {
    fn component_type_label(&self) -> &'static str {
        match &self.component_type {
            better_core::ComponentType::Replacement => "replacement",
            better_core::ComponentType::Enhancement => "enhancement",
            better_core::ComponentType::Diagnostic => "diagnostic",
        }
    }
}

#[cfg(test)]
mod affordance_tests {
    use super::*;

    #[test]
    fn a_status_pill_is_never_an_action() {
        assert_eq!(StatusPill::AFFORDANCE, Affordance::Status);
        assert_ne!(StatusPill::AFFORDANCE, Affordance::Action);
    }

    #[test]
    fn every_tone_carries_its_label_unchanged() {
        for tone in [
            StatusTone::Neutral,
            StatusTone::Info,
            StatusTone::Success,
            StatusTone::Warning,
            StatusTone::Danger,
        ] {
            let pill = StatusPill::new("Healthy", tone);
            assert_eq!(pill.label.as_ref(), "Healthy");
            assert_eq!(pill.tone, tone);
        }
    }

    /// The Status half of the affordance rule is a property of the source
    /// text, not of a rendered frame: `badge` — which every status indicator
    /// is drawn by — must register no hover, no cursor and no click handler.
    /// A headless test cannot inspect a GPUI style refinement, so this asserts
    /// against the primitive's own definition instead of a screenshot.
    #[test]
    fn the_badge_primitive_registers_no_interactive_chrome() {
        let source = include_str!("lib.rs");
        let start = source
            .find("pub fn badge(")
            .expect("badge must still be defined here");
        let body = &source[start..];
        let end = body
            .find("\n}\n")
            .expect("badge must have a terminating brace");
        let body = &body[..end];
        for chrome in [".hover(", "cursor_pointer", "on_click", ".id("] {
            assert!(
                !body.contains(chrome),
                "badge must stay non-interactive, but its body uses {chrome}"
            );
        }
    }
}

#[cfg(test)]
mod icon_tests {
    use super::icons::{IconSearch, initial};
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("better-ui-icons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    #[test]
    fn a_letter_stands_in_for_any_name() {
        assert_eq!(initial("files").as_ref(), "F");
        assert_eq!(initial("  9 lives").as_ref(), "9");
        assert_eq!(initial("檔案").as_ref(), "檔");
        assert_eq!(initial("—").as_ref(), "?");
    }

    #[test]
    fn an_absolute_icon_path_must_be_a_small_image_outside_the_kernel_s_files() {
        use super::icons::{IconSearch, MAX_ICON_BYTES, acceptable_icon_file};
        let root = scratch("absolute-icons");
        let png = |width: u32, height: u32| {
            let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
            bytes.extend_from_slice(&width.to_be_bytes());
            bytes.extend_from_slice(&height.to_be_bytes());
            bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
            bytes
        };
        let write = |name: &str, bytes: &[u8]| {
            let path = root.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let search = IconSearch::new(vec![root.clone()], Vec::new());
        let small = write("app.png", &png(256, 256));
        assert_eq!(search.resolve(small.to_str().unwrap()), Some(small.clone()));
        let svg = write("app.svg", b"<svg/>");
        assert!(acceptable_icon_file(&svg));
        assert!(acceptable_icon_file(&write("app.xpm", b"/* XPM */")));
        // Too many pixels, not a PNG, the wrong kind of file, too big.
        assert!(!acceptable_icon_file(&write(
            "huge.png",
            &png(20_000, 20_000)
        )));
        assert!(!acceptable_icon_file(&write(
            "fake.png",
            b"not a png at all, honestly"
        )));
        assert!(!acceptable_icon_file(&write("notes.txt", b"text")));
        let big = root.join("big.svg");
        std::fs::write(&big, vec![b' '; MAX_ICON_BYTES as usize + 1]).unwrap();
        assert!(!acceptable_icon_file(&big));
        assert!(!acceptable_icon_file(std::path::Path::new(
            "/proc/self/environ"
        )));
        assert!(!acceptable_icon_file(std::path::Path::new("/dev/zero.png")));
        assert!(!acceptable_icon_file(&root));
        assert_eq!(search.resolve("/proc/self/root/etc/passwd"), None);
    }

    #[test]
    fn a_named_icon_is_found_in_the_theme_then_pixmaps() {
        let root = scratch("lookup");
        let hicolor = root.join("share/icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&hicolor).unwrap();
        std::fs::write(hicolor.join("editor.png"), b"png").unwrap();
        let pixmaps = root.join("share/pixmaps");
        std::fs::create_dir_all(&pixmaps).unwrap();
        std::fs::write(pixmaps.join("legacy.svg"), b"<svg/>").unwrap();
        std::fs::write(pixmaps.join("editor.svg"), b"<svg/>").unwrap();
        let search = IconSearch::new(vec![root.join("share")], Vec::new());
        // The theme wins over pixmaps.
        assert_eq!(search.resolve("editor"), Some(hicolor.join("editor.png")));
        assert_eq!(
            search.resolve("editor.png"),
            Some(hicolor.join("editor.png"))
        );
        assert_eq!(search.resolve("legacy"), Some(pixmaps.join("legacy.svg")));
        assert_eq!(search.resolve("missing"), None);
        // A name is never a path out of the icon directories.
        assert_eq!(search.resolve("../pixmaps/legacy"), None);
        assert_eq!(search.resolve(""), None);
        // An absolute path is used as it is, when it exists.
        let absolute = pixmaps.join("legacy.svg");
        assert_eq!(
            search.resolve(absolute.to_str().unwrap()),
            Some(absolute.clone())
        );
        assert_eq!(search.resolve("/nonexistent/icon.png"), None);

        let icon = AppIcon::for_entry(Some("editor"), "Text Editor", &search);
        assert_eq!(
            icon,
            AppIcon::File {
                path: hicolor.join("editor.png"),
                fallback: "T".into()
            }
        );
        assert_eq!(
            AppIcon::for_entry(Some("missing"), "Text Editor", &search),
            AppIcon::Letter("T".into())
        );
        assert_eq!(
            AppIcon::for_entry(None, "files", &search),
            AppIcon::Letter("F".into())
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_type_icon_is_found_in_its_own_context_only() {
        let root = scratch("context");
        let mimetypes = root.join("share/icons/Adwaita/scalable/mimetypes");
        std::fs::create_dir_all(&mimetypes).unwrap();
        std::fs::write(mimetypes.join("text-x-generic.svg"), b"<svg/>").unwrap();
        let search = IconSearch::new(vec![root.join("share")], vec!["Adwaita".into()]);
        assert_eq!(
            search.resolve_in("mimetypes", "text-x-generic"),
            Some(mimetypes.join("text-x-generic.svg"))
        );
        // An application lookup never lands on a file type icon.
        assert_eq!(search.resolve("text-x-generic"), None);
        assert_eq!(search.resolve_in("places", "text-x-generic"), None);
        // Neither the name nor the context may be a path.
        assert_eq!(search.resolve_in("mimetypes", "../x"), None);
        assert_eq!(search.resolve_in("../apps", "x"), None);
        assert_eq!(search.resolve_in("mimetypes", " "), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_theme_index_lists_its_application_directories_scalable_then_largest() {
        let index = icons::ThemeIndex::parse(
            "[Icon Theme]\nName=Test\nInherits=Parent, Adwaita,hicolor\n\
             Directories=16x16/apps,48x48/apps,48x48/places,scalable/apps,48x48@2x/apps,512x512/apps\n\
             [16x16/apps]\nContext=Applications\nSize=16\n\
             [48x48/apps]\nContext=Applications\nSize=48\n\
             [48x48/places]\nContext=Places\nSize=48\n\
             [48x48@2x/apps]\nContext=Applications\nSize=48\nScale=2\n\
             [scalable/apps]\nContext=Applications\nSize=128\nType=Scalable\n\
             [512x512/apps]\nContext=Applications\nSize=512\n\
             [unlisted/apps]\nContext=Applications\nSize=256\n",
        );
        assert_eq!(index.inherits, ["Parent", "Adwaita", "hicolor"]);
        assert_eq!(
            index.app_dirs,
            [
                "scalable/apps",
                "48x48/apps",
                "48x48@2x/apps",
                "512x512/apps",
                "16x16/apps"
            ]
        );
    }

    #[test]
    fn the_desktop_theme_and_what_it_inherits_are_searched_before_hicolor() {
        let root = scratch("chain");
        let share = root.join("share");
        let write = |theme: &str, index: &str| {
            let dir = share.join("icons").join(theme);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("index.theme"), index).unwrap();
        };
        write(
            "Child",
            "[Icon Theme]\nInherits=Parent,hicolor\nDirectories=32x32/apps\n[32x32/apps]\nContext=Applications\nSize=32\n",
        );
        write(
            "Parent",
            "[Icon Theme]\nInherits=Child\nDirectories=apps/64\n[apps/64]\nContext=Applications\nSize=64\n",
        );
        let parent_icon = share.join("icons/Parent/apps/64");
        std::fs::create_dir_all(&parent_icon).unwrap();
        std::fs::write(parent_icon.join("terminal.svg"), b"<svg/>").unwrap();
        let hicolor = share.join("icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&hicolor).unwrap();
        std::fs::write(hicolor.join("terminal.png"), b"png").unwrap();

        let base = IconSearch::new(vec![share.clone()], Vec::new());
        // The loop back to Child stops rather than repeating.
        assert_eq!(base.theme_chain("Child"), ["Child", "Parent"]);
        let search = IconSearch::new(vec![share.clone()], base.theme_chain("Child"));
        assert_eq!(
            search.resolve("terminal"),
            Some(parent_icon.join("terminal.svg"))
        );
        // A theme name is never a path.
        assert!(base.theme_chain("../escape").is_empty());

        let config = root.join("config");
        std::fs::create_dir_all(config.join("gtk-3.0")).unwrap();
        std::fs::write(
            config.join("gtk-3.0/settings.ini"),
            "[Settings]\ngtk-theme-name=Adwaita\ngtk-icon-theme-name = Child\n",
        )
        .unwrap();
        assert_eq!(icons::gtk_icon_theme(&config).as_deref(), Some("Child"));
        assert_eq!(icons::gtk_icon_theme(&root.join("nowhere")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_theme_directory_outside_the_theme_is_never_searched() {
        let index = icons::ThemeIndex::parse(
            "[Icon Theme]\nDirectories=/etc,../../secret,48x48/../../x,apps/48,./a\n\
             [/etc]\nContext=Applications\nSize=48\n\
             [../../secret]\nContext=Applications\nSize=48\n\
             [48x48/../../x]\nContext=Applications\nSize=48\n\
             [./a]\nContext=Applications\nSize=48\n\
             [apps/48]\nContext=Applications\nSize=48\n",
        );
        assert_eq!(index.app_dirs, ["apps/48"]);
        assert!(icons::safe_subdirectory("48x48@2x/apps"));
        assert!(!icons::safe_subdirectory("/usr/share"));
        assert!(!icons::safe_subdirectory(""));
    }

    #[test]
    fn the_gnome_setting_names_the_theme_and_the_chain_ends_with_adwaita() {
        use icons::{FixedTheme, GtkSettingsIni, IconThemeSource};
        let root = scratch("gnome");
        let share = root.join("share");
        for (theme, index) in [
            (
                "ZorinBlue-Light",
                "[Icon Theme]\nInherits=Zorin,hicolor\nDirectories=\n",
            ),
            ("Zorin", "[Icon Theme]\nInherits=hicolor\nDirectories=\n"),
        ] {
            let dir = share.join("icons").join(theme);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("index.theme"), index).unwrap();
        }
        let config = root.join("config");
        std::fs::create_dir_all(config.join("gtk-3.0")).unwrap();
        std::fs::write(
            config.join("gtk-3.0/settings.ini"),
            "[Settings]\ngtk-icon-theme-name=Yaru\n",
        )
        .unwrap();
        let gtk = GtkSettingsIni {
            config: Some(config),
        };
        let gnome = FixedTheme(Some("ZorinBlue-Light".into()));
        // GNOME's setting wins over settings.ini.
        let search = IconSearch::with_theme_from(vec![share.clone()], &[&gnome, &gtk]);
        assert_eq!(search.themes, ["ZorinBlue-Light", "Zorin", "Adwaita"]);
        // Without it, settings.ini; and Adwaita still comes before hicolor.
        let unset = FixedTheme(None);
        let search = IconSearch::with_theme_from(vec![share.clone()], &[&unset, &gtk]);
        assert_eq!(search.themes, ["Yaru", "Adwaita"]);
        let search = IconSearch::with_theme_from(vec![share], &[&unset]);
        assert_eq!(search.themes, ["Adwaita"]);
        assert_eq!(gtk.icon_theme().as_deref(), Some("Yaru"));
        assert_eq!(
            icons::parse_gsettings_string("'ZorinBlue-Light'\n").as_deref(),
            Some("ZorinBlue-Light")
        );
        assert_eq!(icons::parse_gsettings_string("''"), None);
        assert_eq!(icons::parse_gsettings_string("'../x'"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scalable_comes_before_fixed_sizes_and_themes_before_hicolor() {
        let search = IconSearch::new(vec!["/a".into()], vec!["Yaru".into()]);
        let candidates = search.candidates("x");
        assert_eq!(
            candidates[0],
            std::path::PathBuf::from("/a/icons/Yaru/scalable/apps/x.svg")
        );
        let hicolor = candidates
            .iter()
            .position(|p| p.starts_with("/a/icons/hicolor"))
            .unwrap();
        let yaru_last = candidates
            .iter()
            .rposition(|p| p.starts_with("/a/icons/Yaru"))
            .unwrap();
        assert!(yaru_last < hicolor);
        assert!(candidates.last().unwrap().starts_with("/a/pixmaps"));
    }
}
