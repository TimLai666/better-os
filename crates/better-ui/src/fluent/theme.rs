//! Pointing gpui-component at the Fluent tokens.
//!
//! A window built from this kit still uses a few gpui-component pieces under
//! the drawn surface — the text editing inside a text box, scrollbars, the
//! root that moves focus with Tab — and anything a window has not moved over
//! yet. [`set_appearance`] is the one call that switches the appearance and
//! repaints all of them in the Fluent palette, radii, and type size.

use gpui::{App, Hsla, Window, px};
use gpui_component::{Theme, ThemeMode, ThemeTokens};

use super::{BODY, Palette, palette_for, radius, with_alpha};

/// Switches gpui-component to light or dark and applies the Fluent tokens
/// over it. Call this instead of `Theme::change`, which reloads the
/// library's own colors.
pub fn set_appearance(dark: bool, window: Option<&mut Window>, cx: &mut App) {
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, None, cx);
    let theme = Theme::global_mut(cx);
    apply_to_component_theme(theme);
    // Body is the base size of a window built from the kit. Set here and not
    // in `apply_to_component_theme`, so a window that calls only that (Better
    // Git) keeps the root size its layout was drawn against.
    theme.font_size = px(BODY.size);
    if let Some(window) = window {
        window.refresh();
    }
}

fn h(color: gpui::Rgba) -> Hsla {
    color.into()
}

/// Points gpui-component's own controls at the Fluent palette for the theme's
/// current appearance and sets its radii to WinUI's 4px and 8px.
///
/// Call it after every `Theme::change`: that call reloads the library's own
/// colors and would otherwise put them back. [`set_appearance`] does both.
pub fn apply_to_component_theme(theme: &mut Theme) {
    let p: &Palette = palette_for(theme.is_dark());
    theme.radius = radius::SM;
    theme.radius_lg = radius::MD;
    theme.shadow = false;

    let colors = &mut theme.colors;
    colors.background = h(p.window_background);
    colors.foreground = h(p.text_primary);
    colors.muted = h(p.control_fill);
    colors.muted_foreground = h(p.text_secondary);
    colors.border = h(p.stroke);
    colors.input = h(p.stroke);
    colors.ring = h(p.accent);
    colors.caret = h(p.text_primary);
    colors.selection = h(with_alpha(p.accent, 0.35));
    colors.accent = h(p.subtle_fill);
    colors.accent_foreground = h(p.text_primary);
    colors.primary = h(p.accent);
    colors.primary_hover = h(p.accent_secondary);
    colors.primary_active = h(p.accent_tertiary);
    colors.primary_foreground = h(p.text_on_accent);
    colors.secondary = h(p.control_fill);
    colors.secondary_hover = h(p.control_fill_secondary);
    colors.secondary_active = h(p.control_fill_tertiary);
    colors.secondary_foreground = h(p.text_primary);
    colors.button = h(p.control_fill);
    colors.button_hover = h(p.control_fill_secondary);
    colors.button_active = h(p.control_fill_tertiary);
    colors.button_foreground = h(p.text_primary);
    colors.button_primary = h(p.accent);
    colors.button_primary_hover = h(p.accent_secondary);
    colors.button_primary_active = h(p.accent_tertiary);
    colors.button_primary_foreground = h(p.text_on_accent);
    colors.button_secondary = h(p.control_fill);
    colors.button_secondary_hover = h(p.control_fill_secondary);
    colors.button_secondary_active = h(p.control_fill_tertiary);
    colors.button_secondary_foreground = h(p.text_primary);
    colors.popover = h(p.flyout);
    colors.popover_foreground = h(p.text_primary);
    colors.title_bar = h(p.window_background);
    colors.title_bar_border = h(with_alpha(p.stroke, 0.0));
    colors.danger = h(p.critical);
    colors.success = h(p.success);
    colors.warning = h(p.caution);
    colors.info = h(p.accent);
    colors.link = h(p.accent_text);
    colors.list = h(with_alpha(p.card, 0.0));
    colors.list_hover = h(p.subtle_fill);
    colors.list_active = h(p.subtle_fill);
    colors.list_active_border = h(with_alpha(p.accent, 0.0));
    colors.table = h(with_alpha(p.card, 0.0));
    colors.table_hover = h(p.subtle_fill);
    colors.table_active = h(p.subtle_fill);
    colors.table_head = h(with_alpha(p.card, 0.0));
    colors.table_head_foreground = h(p.text_secondary);
    colors.table_row_border = h(p.divider);
    colors.switch = h(p.control_strong_fill);
    colors.switch_thumb = h(p.text_on_accent);
    colors.slider_bar = h(p.accent);
    colors.slider_thumb = h(p.accent);
    colors.progress_bar = h(p.accent);
    colors.scrollbar = h(with_alpha(p.card, 0.0));
    colors.scrollbar_thumb = h(p.control_strong_fill);
    colors.scrollbar_thumb_hover = h(p.text_secondary);
    colors.tab_bar = h(p.window_background);
    colors.tab = h(with_alpha(p.card, 0.0));
    colors.tab_active = h(p.dialog);
    colors.tab_foreground = h(p.text_secondary);
    colors.tab_active_foreground = h(p.text_primary);
    colors.sidebar = h(p.window_background);
    colors.sidebar_foreground = h(p.text_primary);
    colors.sidebar_border = h(p.card_stroke);
    colors.sidebar_accent = h(p.subtle_fill);
    colors.sidebar_accent_foreground = h(p.text_primary);
    colors.sidebar_primary = h(p.accent);
    colors.sidebar_primary_foreground = h(p.text_on_accent);
    colors.overlay = h(p.smoke);
    colors.window_border = h(p.surface_stroke);
    // Components that read the tokens rather than the colors (the title bar,
    // the buttons) follow the same values.
    theme.tokens = ThemeTokens::from(&theme.colors);
}

/// The appearance a person chose, stored as `system`, `light`, or `dark`
/// (ADR 0019). Better OS follows the desktop: a missing or unreadable choice
/// is [`Appearance::System`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum Appearance {
    /// Follow the desktop, and keep following it while the window is open.
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    /// In the order a picker lists them: the default first.
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    /// The stored word.
    pub fn as_str(self) -> &'static str {
        match self {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
            Appearance::System => "system",
        }
    }

    /// Reads the stored word, ignoring case and surrounding space. Anything
    /// else is `None`, which a caller treats as the default.
    pub fn parse(value: &str) -> Option<Appearance> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Appearance::Dark),
            "light" => Some(Appearance::Light),
            "system" => Some(Appearance::System),
            _ => None,
        }
    }

    /// Whether to draw dark, given what the desktop is in right now.
    pub fn is_dark(self, desktop: gpui::WindowAppearance) -> bool {
        match self {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => matches!(
                desktop,
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
            ),
        }
    }

    /// The choice stored at `path`, or [`Appearance::System`] when there is
    /// none or it cannot be read. The file is written only when a person
    /// picks, so a stored word is always a choice they made.
    pub fn load(path: &std::path::Path) -> Appearance {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Appearance::parse(&text))
            .unwrap_or_default()
    }

    /// Stores the choice at `path`, creating its directory, through a
    /// temporary file renamed into place so a crash never leaves half a word.
    pub fn save(self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, format!("{}\n", self.as_str()))?;
        std::fs::rename(&temporary, path)
    }

    /// Where a window stores its choice when it has no settings file of its
    /// own: `$XDG_CONFIG_HOME/better-os/<app>/appearance`, or
    /// `~/.config/...` when `XDG_CONFIG_HOME` is unset or relative.
    pub fn default_path(app: &str) -> Option<std::path::PathBuf> {
        config_home(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        )
        .map(|base| base.join("better-os").join(app).join("appearance"))
    }

    /// Applies the choice to the window, now.
    pub fn apply(self, window: &mut Window, cx: &mut App) {
        let dark = self.is_dark(window.appearance());
        set_appearance(dark, Some(window), cx);
    }

    /// Makes this the process's appearance, paints this window in the Fluent
    /// palette, and keeps the window following it. See [`Appearance::follow`].
    pub fn apply_and_follow(self, window: &mut Window, cx: &mut App) {
        self.follow(window, cx, |dark, window, cx| {
            set_appearance(dark, Some(window), cx)
        })
    }

    /// Makes this the appearance of every window in the process, calls
    /// `paint` with whether to draw dark now, and keeps this window following
    /// the process's appearance: while it is [`Appearance::System`], `paint`
    /// runs again every time the desktop switches. `paint` is for a window
    /// that draws in something other than the Fluent palette.
    ///
    /// The choice is one per process because the theme it paints is one per
    /// process (gpui-component's `Theme` is a global): two windows that each
    /// kept their own would fight, the one still following the desktop
    /// repainting the other after the person picked dark in it (ticket 110).
    /// So the choice lives in a GPUI global, read with
    /// [`Appearance::current`], and every window's subscription repaints the
    /// process's choice rather than its own.
    ///
    /// Following is not optional for `System`. When a window opens, GPUI on
    /// Linux has not heard from the desktop yet: it asks the XDG desktop
    /// portal in the background and reports light until the answer arrives,
    /// so a window that reads the desktop only once opens light on a dark
    /// desktop (ticket 110). The answer arrives as an appearance change. The
    /// subscription that sees it is kept in the same global, one per window,
    /// so no window can lose it by dropping a return value.
    pub fn follow(self, window: &mut Window, cx: &mut App, paint: fn(bool, &mut Window, &mut App)) {
        let id = window.window_handle().window_id();
        let open: Vec<gpui::WindowId> = cx
            .windows()
            .iter()
            .map(|handle| handle.window_id())
            .collect();
        let subscription = window
            .observe_window_appearance(move |window, cx| repaint_for_desktop(paint, window, cx));
        let state = cx.default_global::<ProcessAppearance>();
        state.choice = self;
        // A closed window's subscription would never fire again; dropping it
        // keeps the list from growing with every window ever opened.
        state.windows.retain(|(window, _)| open.contains(window));
        state.windows.retain(|(window, _)| *window != id);
        state.windows.push((id, subscription));
        paint(self.is_dark(window.appearance()), window, cx);
        // The theme is shared, so every other window shows the new choice too.
        cx.refresh_windows();
    }

    /// The appearance every window in this process shows: the last one set
    /// through [`Appearance::follow`], or the default before any was.
    pub fn current(cx: &App) -> Appearance {
        cx.try_global::<ProcessAppearance>()
            .map(|state| state.choice)
            .unwrap_or_default()
    }

    /// Whether `window` is kept following the process's appearance. A window
    /// that applied its choice without [`Appearance::follow`] would not be.
    pub fn is_following(window: &Window, cx: &App) -> bool {
        let id = window.window_handle().window_id();
        cx.try_global::<ProcessAppearance>()
            .is_some_and(|state| state.windows.iter().any(|(window, _)| *window == id))
    }

    /// Whether this choice keeps changing with the desktop.
    pub fn follows_desktop(self) -> bool {
        self == Appearance::System
    }
}

/// The process's one appearance choice and every window's subscription to
/// the desktop's appearance (see [`Appearance::follow`]).
#[derive(Default)]
struct ProcessAppearance {
    choice: Appearance,
    windows: Vec<(gpui::WindowId, gpui::Subscription)>,
}

impl gpui::Global for ProcessAppearance {}

/// What a window's subscription does when the desktop switches: repaint the
/// process's choice, which only changes anything while that choice follows
/// the desktop. A choice made in another window of the process wins over
/// whatever this window was opened with.
pub(crate) fn repaint_for_desktop(
    paint: fn(bool, &mut Window, &mut App),
    window: &mut Window,
    cx: &mut App,
) {
    let choice = Appearance::current(cx);
    if choice.follows_desktop() {
        paint(choice.is_dark(window.appearance()), window, cx);
    }
}

/// The XDG base directory rule: an absolute `XDG_CONFIG_HOME`, else
/// `$HOME/.config`.
pub fn config_home(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    xdg_config_home
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(std::path::PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".config"))
        })
}
