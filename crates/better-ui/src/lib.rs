//! Shared presentation for every Better OS window: the WinUI 3 control kit in
//! [`fluent`], the window chrome and options, the command-line contract, and
//! the older status, badge, and application primitives.

pub mod command_line;
pub mod fluent;

use better_core::ComponentManifest;
use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, StyledImage,
    Window, WindowDecorations, WindowOptions, div,
};
use gpui_component::*;

/// The window chrome every first-party Better OS window draws for itself.
///
/// GNOME's Mutter offers no server-side decorations to an `xdg-toplevel`
/// client, so on the Wayland session a person actually runs, a GPUI window has
/// no close, minimize or maximize button and cannot be dragged unless the
/// application draws them. Every first-party window therefore carries this bar.
///
/// The behaviour — dragging, double-click to maximize, and the three controls —
/// comes from `gpui_component::TitleBar`, which already talks to the
/// compositor through `start_window_move`, `zoom_window`, `minimize_window` and
/// `remove_window`. This wrapper exists so the icon, the title and the spacing
/// are the same in all seven windows rather than seven near-copies.
pub mod window_chrome {
    use super::*;

    /// The height of the shared titlebar, re-exported so a window that has to
    /// reason about its own content height does not guess.
    pub use gpui_component::TITLE_BAR_HEIGHT;

    /// The titlebar itself: the application's glyph, its localized window
    /// title, and the platform's window controls.
    ///
    /// `icon` is an element rather than a name so each application passes the
    /// glyph it already uses in its own header, and the bar never grows a
    /// table of application identities of its own.
    pub fn title_bar(
        icon: impl IntoElement,
        title: impl Into<SharedString>,
        foreground: Hsla,
    ) -> TitleBar {
        TitleBar::new().child(
            h_flex().min_w_0().items_center().gap_2().child(icon).child(
                div()
                    .min_w_0()
                    .text_sm()
                    .font_medium()
                    .text_color(foreground)
                    .child(title.into()),
            ),
        )
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
        WindowOptions {
            titlebar: Some(TitleBar::title_bar_options()),
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
        let letter = move |glyph: SharedString, style: TileStyle| {
            div()
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(style.radius)
                .bg(style.glyph_background)
                .text_color(style.glyph_foreground)
                .child(glyph)
                .into_any_element()
        };
        match self {
            AppIcon::Letter(glyph) => letter(glyph.clone(), *style),
            AppIcon::File { path, fallback } => {
                let fallback = fallback.clone();
                let style = *style;
                gpui::img(path.clone())
                    .size(size)
                    .object_fit(gpui::ObjectFit::Contain)
                    .with_fallback(move || letter(fallback.clone(), style))
                    .into_any_element()
            }
        }
    }
}

/// Finding the file an application's `Icon=` names, the way the freedesktop
/// icon theme specification does for the cases Better OS needs: the user's
/// and the system's `icons/<theme>` and `icons/hicolor` directories at the
/// usual sizes, then `pixmaps`.
pub mod icons {
    use std::path::{Path, PathBuf};

    use gpui::SharedString;

    /// The sizes looked in, largest useful first. Scalable comes first
    /// because it draws sharp at any size.
    pub const SIZES: [&str; 8] = [
        "scalable", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32", "24x24",
    ];

    /// Where to look.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct IconSearch {
        /// `$XDG_DATA_HOME` and every `$XDG_DATA_DIRS` entry, in order.
        pub data_dirs: Vec<PathBuf>,
        /// Theme names to try before `hicolor`, such as the desktop's own.
        pub themes: Vec<String>,
    }

    impl IconSearch {
        /// The process's data directories, as the XDG base directory rule
        /// defines them.
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
            IconSearch {
                data_dirs: dirs,
                themes: Vec::new(),
            }
        }

        /// Every path tried for `name`, in order. Separate from
        /// [`IconSearch::resolve`] so the order can be tested.
        pub fn candidates(&self, name: &str) -> Vec<PathBuf> {
            let mut paths = Vec::new();
            let themes = self
                .themes
                .iter()
                .map(String::as_str)
                .chain(std::iter::once("hicolor"));
            let themes: Vec<&str> = themes.collect();
            for dir in &self.data_dirs {
                for theme in &themes {
                    for size in SIZES {
                        for extension in ["svg", "png"] {
                            if size == "scalable" && extension == "png" {
                                continue;
                            }
                            paths.push(
                                dir.join("icons")
                                    .join(theme)
                                    .join(size)
                                    .join("apps")
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
                return as_path.is_file().then(|| as_path.to_path_buf());
            }
            if icon.contains('/') || icon.contains('\\') || icon == "." || icon == ".." {
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
    fn a_named_icon_is_found_in_the_theme_then_pixmaps() {
        let root = scratch("lookup");
        let hicolor = root.join("share/icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&hicolor).unwrap();
        std::fs::write(hicolor.join("editor.png"), b"png").unwrap();
        let pixmaps = root.join("share/pixmaps");
        std::fs::create_dir_all(&pixmaps).unwrap();
        std::fs::write(pixmaps.join("legacy.svg"), b"<svg/>").unwrap();
        std::fs::write(pixmaps.join("editor.svg"), b"<svg/>").unwrap();
        let search = IconSearch {
            data_dirs: vec![root.join("share")],
            themes: Vec::new(),
        };
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
    fn scalable_comes_before_fixed_sizes_and_themes_before_hicolor() {
        let search = IconSearch {
            data_dirs: vec!["/a".into()],
            themes: vec!["Yaru".into()],
        };
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
