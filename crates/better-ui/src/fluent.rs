//! The WinUI 3 (Windows 11 Fluent) control kit every Better OS window is
//! built from.
//!
//! This file holds the tokens: the two color palettes, the type ramp, the 4px
//! spacing grid, the two corner radii, the standard sizes, and the pieces every
//! control shares — the visual states, the keyboard focus ring, and the key
//! names a control reacts to. Each control lives in its own module below and
//! reads only these tokens, so a window that uses the kit cannot drift from
//! `DESIGN.md`.
//!
//! The color values are WinUI's own theme resources, read from
//! `microsoft/microsoft-ui-xaml` at commit
//! `e38939f2680f0da968d7536a0b7863617ab5793e`:
//! `controls/dev/CommonStyles/Common_themeresources_any.xaml` for the fills,
//! strokes, text, and status colors, and
//! `controls/dev/Materials/Acrylic/AcrylicBrush_themeresources.xaml` for the
//! flyout fill. WinUI takes the accent shades from the operating system
//! (`SystemAccentColorDark1` and friends); the values here are Windows 11's
//! default blue, which the source does not contain. Change a value in
//! `DESIGN.md` and here together.
//!
//! Every control has the WinUI states — rest, pointer over, pressed, focused,
//! disabled — and accepts [`ShownState`] so the gallery can draw any one of
//! them without a pointer over it.

pub mod auto_suggest;
pub mod badge;
pub mod breadcrumb;
pub mod button;
pub mod chart;
pub mod chrome;
pub mod command_bar;
pub mod dialog;
pub mod hyperlink;
pub mod info_bar;
pub mod list;
pub mod menu;
pub mod navigation;
pub mod progress;
pub mod selection;
pub mod settings_card;
pub mod tabs;
pub mod text_box;
pub mod theme;
pub mod toggle;

#[cfg(test)]
mod tests;

pub use theme::{apply_to_component_theme, set_appearance};

use gpui::{
    App, Div, FontWeight, InteractiveElement, Keystroke, ParentElement, Pixels, Rgba, Stateful,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_component::ActiveTheme;

/// One appearance's colors, named after the WinUI theme resource each one
/// is. Translucent values are kept translucent, as WinUI draws them: a control
/// fill over a card reads differently from the same fill over the window, and
/// that difference is part of the look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// `SolidBackgroundFillColorBase`: behind everything else.
    pub window_background: Rgba,
    /// `CardBackgroundFillColorDefault`: cards, settings cards, panes.
    pub card: Rgba,
    /// `CardBackgroundFillColorSecondary`: an expanded settings card's body.
    pub card_secondary: Rgba,
    /// `LayerFillColorDefault`: the content area beside a navigation pane.
    pub layer: Rgba,
    /// A content dialog's body: `LayerFillColorAlt` over the window
    /// background, composited, because the dialog sits over a smoke layer and
    /// not over the page.
    pub dialog: Rgba,
    /// `SolidBackgroundFillColorTertiary`: the selected tab, joined to its
    /// content.
    pub tab_selected: Rgba,
    /// `LayerOnMicaBaseAltFillColorSecondary`: a tab under the pointer.
    pub tab_hover: Rgba,
    /// `LayerOnMicaBaseAltFillColorDefault`: a tab pressed.
    pub tab_pressed: Rgba,
    /// `AcrylicInAppFillColorDefault`'s fallback color: flyouts, menus, combo
    /// box drop-downs, tooltips. A Wayland client gets no acrylic.
    pub flyout: Rgba,
    /// `TextFillColorPrimary`.
    pub text_primary: Rgba,
    /// `TextFillColorSecondary`.
    pub text_secondary: Rgba,
    /// `TextFillColorTertiary`: pressed placeholder and chevron text.
    pub text_tertiary: Rgba,
    /// `TextFillColorDisabled`.
    pub text_disabled: Rgba,
    /// `TextFillColorInverse`: the glyph inside an info bar's status icon.
    pub text_inverse: Rgba,
    /// `AccentFillColorDefault`. The one primary action, the selection
    /// indicator, and focus; never decoration.
    pub accent: Rgba,
    /// `AccentFillColorSecondary`: accent under the pointer.
    pub accent_secondary: Rgba,
    /// `AccentFillColorTertiary`: accent pressed.
    pub accent_tertiary: Rgba,
    /// `AccentFillColorDisabled`.
    pub accent_disabled: Rgba,
    /// `AccentTextFillColorPrimary`: a hyperlink, an accent-colored word.
    pub accent_text: Rgba,
    /// `TextOnAccentFillColorPrimary`.
    pub text_on_accent: Rgba,
    /// `TextOnAccentFillColorSecondary`: text on a pressed accent fill.
    pub text_on_accent_secondary: Rgba,
    /// `TextOnAccentFillColorDisabled`.
    pub text_on_accent_disabled: Rgba,
    /// `ControlFillColorDefault`: a standard control at rest.
    pub control_fill: Rgba,
    /// `ControlFillColorSecondary`: under the pointer.
    pub control_fill_secondary: Rgba,
    /// `ControlFillColorTertiary`: pressed.
    pub control_fill_tertiary: Rgba,
    /// `ControlFillColorDisabled`.
    pub control_fill_disabled: Rgba,
    /// `ControlFillColorInputActive`: a focused text field.
    pub control_fill_input_active: Rgba,
    /// `ControlStrongFillColorDefault`: a slider's track, an unchecked knob.
    pub control_strong_fill: Rgba,
    /// `ControlStrongFillColorDisabled`.
    pub control_strong_fill_disabled: Rgba,
    /// `ControlSolidFillColorDefault`: the ring around a slider's thumb.
    pub control_solid_fill: Rgba,
    /// `ControlAltFillColorSecondary`: an unchecked check box or radio.
    pub control_alt_fill: Rgba,
    /// `ControlAltFillColorTertiary`: the same under the pointer.
    pub control_alt_fill_tertiary: Rgba,
    /// `ControlAltFillColorQuarternary`: the same pressed.
    pub control_alt_fill_quarternary: Rgba,
    /// `SubtleFillColorSecondary`: hover and selection on a row, a menu
    /// item, a command bar button.
    pub subtle_fill: Rgba,
    /// `SubtleFillColorTertiary`: the same pressed.
    pub subtle_fill_tertiary: Rgba,
    /// `ControlStrokeColorDefault`: the 1px edge of a control.
    pub stroke: Rgba,
    /// `ControlStrokeColorSecondary`: the darker edge of a control's
    /// elevation border (the bottom in light, the top in dark).
    pub stroke_secondary: Rgba,
    /// `ControlStrokeColorOnAccentDefault`: an accent control's edge.
    pub stroke_on_accent: Rgba,
    /// `ControlStrokeColorOnAccentSecondary`: an accent control's darker edge.
    pub stroke_on_accent_secondary: Rgba,
    /// `ControlStrongStrokeColorDefault`: a check box's or radio's outline, a
    /// text field's bottom edge.
    pub strong_stroke: Rgba,
    /// `ControlStrongStrokeColorDisabled`.
    pub strong_stroke_disabled: Rgba,
    /// `CardStrokeColorDefault`: the edge of a card or pane.
    pub card_stroke: Rgba,
    /// `DividerStrokeColorDefault`: separators in menus and lists.
    pub divider: Rgba,
    /// `SurfaceStrokeColorFlyout`: the edge of a flyout or menu.
    pub flyout_stroke: Rgba,
    /// `SurfaceStrokeColorDefault`: the edge of a content dialog.
    pub surface_stroke: Rgba,
    /// `FocusStrokeColorOuter`: the 2px outer keyboard focus ring.
    pub focus_outer: Rgba,
    /// `FocusStrokeColorInner`: the 1px inner keyboard focus ring.
    pub focus_inner: Rgba,
    /// `SmokeFillColorDefault`: behind a content dialog.
    pub smoke: Rgba,
    /// `SystemFillColorCritical`.
    pub critical: Rgba,
    /// `SystemFillColorSuccess`.
    pub success: Rgba,
    /// `SystemFillColorCaution`.
    pub caution: Rgba,
    /// `SystemFillColorCriticalBackground`: an error info bar.
    pub critical_background: Rgba,
    /// `SystemFillColorSuccessBackground`: a success info bar.
    pub success_background: Rgba,
    /// `SystemFillColorCautionBackground`: a warning info bar.
    pub caution_background: Rgba,
    /// `SystemFillColorAttentionBackground`: an informational info bar.
    pub attention_background: Rgba,
}

/// `0xRRGGBB` as an opaque color.
pub const fn hex(rgb: u32) -> Rgba {
    Rgba {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// `0xAARRGGBB`, the order XAML writes a translucent color in.
pub const fn argb(value: u32) -> Rgba {
    Rgba {
        r: ((value >> 16) & 0xff) as f32 / 255.0,
        g: ((value >> 8) & 0xff) as f32 / 255.0,
        b: (value & 0xff) as f32 / 255.0,
        a: ((value >> 24) & 0xff) as f32 / 255.0,
    }
}

/// A color at an opacity between 0 and 1, for the stroke and status tints.
pub const fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: alpha,
    }
}

/// Windows 11's default blue accent, as the operating system hands it to
/// WinUI. `SystemAccentColorDark1` is the light theme's accent fill,
/// `SystemAccentColorLight2` the dark theme's.
pub mod accent {
    use super::{Rgba, hex};
    pub const DARK_1: Rgba = hex(0x005fb8);
    pub const DARK_2: Rgba = hex(0x003e92);
    pub const LIGHT_2: Rgba = hex(0x60cdff);
    pub const LIGHT_3: Rgba = hex(0x99ebff);
}

pub const LIGHT: Palette = Palette {
    window_background: hex(0xf3f3f3),
    card: argb(0xb3ffffff),
    card_secondary: argb(0x80f6f6f6),
    layer: argb(0x80ffffff),
    dialog: hex(0xffffff),
    tab_selected: hex(0xf9f9f9),
    tab_hover: argb(0x0a000000),
    tab_pressed: argb(0xb3ffffff),
    flyout: hex(0xf9f9f9),
    text_primary: argb(0xe4000000),
    text_secondary: argb(0x9e000000),
    text_tertiary: argb(0x72000000),
    text_disabled: argb(0x5c000000),
    text_inverse: hex(0xffffff),
    accent: accent::DARK_1,
    accent_secondary: with_alpha(accent::DARK_1, 0.9),
    accent_tertiary: with_alpha(accent::DARK_1, 0.8),
    accent_disabled: argb(0x37000000),
    accent_text: accent::DARK_2,
    text_on_accent: hex(0xffffff),
    text_on_accent_secondary: argb(0xb3ffffff),
    text_on_accent_disabled: hex(0xffffff),
    control_fill: argb(0xb3ffffff),
    control_fill_secondary: argb(0x80f9f9f9),
    control_fill_tertiary: argb(0x4df9f9f9),
    control_fill_disabled: argb(0x4df9f9f9),
    control_fill_input_active: hex(0xffffff),
    control_strong_fill: argb(0x72000000),
    control_strong_fill_disabled: argb(0x51000000),
    control_solid_fill: hex(0xffffff),
    control_alt_fill: argb(0x06000000),
    control_alt_fill_tertiary: argb(0x0f000000),
    control_alt_fill_quarternary: argb(0x18000000),
    subtle_fill: argb(0x09000000),
    subtle_fill_tertiary: argb(0x06000000),
    stroke: argb(0x0f000000),
    stroke_secondary: argb(0x29000000),
    stroke_on_accent: argb(0x14ffffff),
    stroke_on_accent_secondary: argb(0x66000000),
    strong_stroke: argb(0x72000000),
    strong_stroke_disabled: argb(0x37000000),
    card_stroke: argb(0x0f000000),
    divider: argb(0x0f000000),
    flyout_stroke: argb(0x0f000000),
    surface_stroke: argb(0x66757575),
    focus_outer: argb(0xe4000000),
    focus_inner: argb(0xb3ffffff),
    smoke: argb(0x4d000000),
    critical: hex(0xc42b1c),
    success: hex(0x0f7b0f),
    caution: hex(0x9d5d00),
    critical_background: hex(0xfde7e9),
    success_background: hex(0xdff6dd),
    caution_background: hex(0xfff4ce),
    attention_background: argb(0x80f6f6f6),
};

pub const DARK: Palette = Palette {
    window_background: hex(0x202020),
    card: argb(0x0dffffff),
    card_secondary: argb(0x08ffffff),
    layer: argb(0x4c3a3a3a),
    dialog: hex(0x2b2b2b),
    tab_selected: hex(0x282828),
    tab_hover: argb(0x0fffffff),
    tab_pressed: argb(0x733a3a3a),
    flyout: hex(0x2c2c2c),
    text_primary: hex(0xffffff),
    text_secondary: argb(0xc5ffffff),
    text_tertiary: argb(0x87ffffff),
    text_disabled: argb(0x5dffffff),
    text_inverse: argb(0xe4000000),
    accent: accent::LIGHT_2,
    accent_secondary: with_alpha(accent::LIGHT_2, 0.9),
    accent_tertiary: with_alpha(accent::LIGHT_2, 0.8),
    accent_disabled: argb(0x28ffffff),
    accent_text: accent::LIGHT_3,
    text_on_accent: hex(0x000000),
    text_on_accent_secondary: argb(0x80000000),
    text_on_accent_disabled: argb(0x87ffffff),
    control_fill: argb(0x0fffffff),
    control_fill_secondary: argb(0x15ffffff),
    control_fill_tertiary: argb(0x08ffffff),
    control_fill_disabled: argb(0x0bffffff),
    control_fill_input_active: argb(0xb31e1e1e),
    control_strong_fill: argb(0x8bffffff),
    control_strong_fill_disabled: argb(0x3fffffff),
    control_solid_fill: hex(0x454545),
    control_alt_fill: argb(0x19000000),
    control_alt_fill_tertiary: argb(0x0bffffff),
    control_alt_fill_quarternary: argb(0x12ffffff),
    subtle_fill: argb(0x0fffffff),
    subtle_fill_tertiary: argb(0x0affffff),
    stroke: argb(0x12ffffff),
    stroke_secondary: argb(0x18ffffff),
    stroke_on_accent: argb(0x14ffffff),
    stroke_on_accent_secondary: argb(0x23000000),
    strong_stroke: argb(0x8bffffff),
    strong_stroke_disabled: argb(0x28ffffff),
    card_stroke: argb(0x19000000),
    divider: argb(0x15ffffff),
    flyout_stroke: argb(0x33000000),
    surface_stroke: argb(0x66757575),
    focus_outer: hex(0xffffff),
    focus_inner: argb(0xb3000000),
    smoke: argb(0x4d000000),
    critical: hex(0xff99a4),
    success: hex(0x6ccb5f),
    caution: hex(0xfce100),
    critical_background: hex(0x442726),
    success_background: hex(0x393d1b),
    caution_background: hex(0x433519),
    attention_background: argb(0x08ffffff),
};

/// The palette for one appearance.
pub const fn palette_for(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// The palette for the appearance gpui-component is in right now.
pub fn palette(cx: &App) -> &'static Palette {
    palette_for(cx.theme().is_dark())
}

impl Palette {
    /// Whether this is the dark palette. The elevation border puts its darker
    /// edge at the bottom in light and at the top in dark.
    pub fn is_dark(&self) -> bool {
        self.window_background == DARK.window_background
    }
}

/// One step of the WinUI type ramp: size over line height, and weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    pub size: f32,
    pub line_height: f32,
    pub weight: FontWeight,
}

/// Metadata, hints, timestamps.
pub const CAPTION: TypeStyle = TypeStyle {
    size: 12.0,
    line_height: 16.0,
    weight: FontWeight::NORMAL,
};
/// Everything a person reads or clicks.
pub const BODY: TypeStyle = TypeStyle {
    size: 14.0,
    line_height: 20.0,
    weight: FontWeight::NORMAL,
};
/// A row's name, a field's label, a selected item.
pub const BODY_STRONG: TypeStyle = TypeStyle {
    size: 14.0,
    line_height: 20.0,
    weight: FontWeight::SEMIBOLD,
};
/// A section or pane title.
pub const SUBTITLE: TypeStyle = TypeStyle {
    size: 20.0,
    line_height: 28.0,
    weight: FontWeight::SEMIBOLD,
};
/// The page title, once per page.
pub const TITLE: TypeStyle = TypeStyle {
    size: 28.0,
    line_height: 36.0,
    weight: FontWeight::SEMIBOLD,
};
/// Code, diffs, paths, and hashes, in the platform monospace font.
pub const CODE: TypeStyle = TypeStyle {
    size: 13.0,
    line_height: 20.0,
    weight: FontWeight::NORMAL,
};

/// Applies one step of the type ramp to an element. The font family is left
/// to the element: monospace text sets it alongside [`CODE`].
pub fn text<E: Styled>(element: E, style: TypeStyle) -> E {
    element
        .text_size(px(style.size))
        .line_height(px(style.line_height))
        .font_weight(style.weight)
}

/// A block of text in one style and color: the shorthand every control uses.
pub fn label(content: impl Into<gpui::SharedString>, style: TypeStyle, color: Rgba) -> Div {
    text(div(), style).text_color(color).child(content.into())
}

/// The 4px spacing grid.
pub mod spacing {
    use super::{Pixels, px};

    /// Between an icon and its label.
    pub const XS: Pixels = px(4.0);
    /// Between related controls.
    pub const SM: Pixels = px(8.0);
    /// Inside a control row.
    pub const MD: Pixels = px(12.0);
    /// A card's padding, and between cards.
    pub const LG: Pixels = px(16.0);
    /// A page's margin.
    pub const XL: Pixels = px(24.0);
}

/// The two corner radii.
pub mod radius {
    use super::{Pixels, px};

    /// Everything that sits in the page (`ControlCornerRadius`).
    pub const SM: Pixels = px(4.0);
    /// What floats above it (`OverlayCornerRadius`).
    pub const MD: Pixels = px(8.0);
}

/// Standard sizes from `DESIGN.md`'s layout section and WinUI's resources.
pub mod size {
    use super::{Pixels, px};

    /// A button, a single-line field.
    pub const CONTROL_HEIGHT: Pixels = px(32.0);
    /// A list row with one line.
    pub const ROW_HEIGHT: Pixels = px(36.0);
    /// A list row with a second line of caption text.
    pub const TWO_LINE_ROW_HEIGHT: Pixels = px(48.0);
    /// A navigation pane holding a list a person works in
    /// (`OpenPaneLength`'s default).
    pub const LIST_PANE_WIDTH: Pixels = px(320.0);
    /// A navigation pane holding only icons (`NavigationViewCompactPaneLength`).
    pub const ICON_PANE_WIDTH: Pixels = px(48.0);
    /// The accent bar at a selected row's leading edge
    /// (`NavigationViewSelectionIndicatorWidth`/`Height`).
    pub const SELECTION_BAR_WIDTH: Pixels = px(3.0);
    pub const SELECTION_BAR_HEIGHT: Pixels = px(16.0);
    /// A small glyph inside a control.
    pub const ICON: Pixels = px(16.0);
}

/// Which of WinUI's visual states a control is drawn in. Focus is separate,
/// because a control can be focused and under the pointer at once.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Interaction {
    Rest,
    PointerOver,
    Pressed,
    Disabled,
}

/// A state the gallery asks a control to draw itself in, whatever the
/// pointer and the keyboard are doing. `Live` is the default and means "draw
/// what is really happening".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ShownState {
    #[default]
    Live,
    Rest,
    PointerOver,
    Pressed,
    Focused,
    Disabled,
}

impl ShownState {
    /// The five states every control is shown in, in WinUI's order.
    pub const ALL: [ShownState; 5] = [
        ShownState::Rest,
        ShownState::PointerOver,
        ShownState::Pressed,
        ShownState::Focused,
        ShownState::Disabled,
    ];

    /// The visual state to draw, or `None` to follow the pointer.
    pub fn forced(self) -> Option<Interaction> {
        match self {
            ShownState::Live => None,
            ShownState::Rest | ShownState::Focused => Some(Interaction::Rest),
            ShownState::PointerOver => Some(Interaction::PointerOver),
            ShownState::Pressed => Some(Interaction::Pressed),
            ShownState::Disabled => Some(Interaction::Disabled),
        }
    }

    /// Whether to draw the focus ring regardless of the real focus.
    pub fn shows_focus(self) -> bool {
        self == ShownState::Focused
    }

    pub fn is_live(self) -> bool {
        self == ShownState::Live
    }

    pub fn is_disabled(self) -> bool {
        self == ShownState::Disabled
    }

    /// The English name the gallery prints over a column.
    pub fn name(self) -> &'static str {
        match self {
            ShownState::Live => "Live",
            ShownState::Rest => "Rest",
            ShownState::PointerOver => "Pointer over",
            ShownState::Pressed => "Pressed",
            ShownState::Focused => "Focused",
            ShownState::Disabled => "Disabled",
        }
    }
}

/// The colors one control is drawn with in one state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlColors {
    pub background: Rgba,
    pub foreground: Rgba,
    pub border: Rgba,
    /// The darker edge of WinUI's elevation border, when the state has one.
    pub elevation: Option<Rgba>,
}

/// A control's colors for every visual state, worked out once so the live
/// hover and press refinements and a forced gallery state agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateColors {
    pub rest: ControlColors,
    pub pointer_over: ControlColors,
    pub pressed: ControlColors,
    pub disabled: ControlColors,
}

impl StateColors {
    pub fn from_fn(f: impl Fn(Interaction) -> ControlColors) -> Self {
        Self {
            rest: f(Interaction::Rest),
            pointer_over: f(Interaction::PointerOver),
            pressed: f(Interaction::Pressed),
            disabled: f(Interaction::Disabled),
        }
    }

    pub fn get(&self, state: Interaction) -> ControlColors {
        match state {
            Interaction::Rest => self.rest,
            Interaction::PointerOver => self.pointer_over,
            Interaction::Pressed => self.pressed,
            Interaction::Disabled => self.disabled,
        }
    }
}

/// The group name the elevation edge listens to for its pressed state.
const CONTROL_GROUP: &str = "fluent-control";

/// The state to draw: `Disabled` wins, then the gallery's forced state, and
/// `None` means follow the pointer.
pub fn resolve(disabled: bool, shown: ShownState) -> Option<Interaction> {
    if disabled || shown.is_disabled() {
        Some(Interaction::Disabled)
    } else {
        shown.forced()
    }
}

/// Paints a control's fill, text color, and border for its state, and wires
/// the hover and press refinements when it follows the pointer.
pub(crate) fn paint(
    element: Stateful<Div>,
    colors: StateColors,
    state: Option<Interaction>,
    radius: Pixels,
    dark: bool,
) -> Stateful<Div> {
    let base = colors.get(state.unwrap_or(Interaction::Rest));
    let element = element
        .relative()
        .group(CONTROL_GROUP)
        .bg(base.background)
        .text_color(base.foreground)
        .border_1()
        .border_color(base.border)
        .rounded(radius);
    let element = match state {
        Some(_) => element,
        None => {
            let hover = colors.pointer_over;
            let pressed = colors.pressed;
            element
                .hover(move |style| {
                    style
                        .bg(hover.background)
                        .text_color(hover.foreground)
                        .border_color(hover.border)
                })
                .active(move |style| {
                    style
                        .bg(pressed.background)
                        .text_color(pressed.foreground)
                        .border_color(pressed.border)
                })
        }
    };
    match base.elevation {
        Some(edge) => {
            let pressed_edge = colors.pressed.elevation;
            element.child(
                elevation_edge(edge, radius, dark).when(state.is_none(), |edge_el| {
                    edge_el.group_active(CONTROL_GROUP, move |style| match pressed_edge {
                        Some(color) => style.bg(color),
                        None => style.opacity(0.0),
                    })
                }),
            )
        }
        None => element,
    }
}

/// The 1px darker edge of WinUI's `ControlElevationBorderBrush`: at the bottom
/// in light, at the top in dark, inset by the corner radius so it meets the
/// rounded border rather than squaring it.
pub(crate) fn elevation_edge(color: Rgba, radius: Pixels, dark: bool) -> Stateful<Div> {
    div()
        .id("elevation-edge")
        .absolute()
        .left(radius)
        .right(radius)
        .h(px(1.0))
        .when(dark, |edge| edge.top(px(-1.0)))
        .when(!dark, |edge| edge.bottom(px(-1.0)))
        .bg(color)
}

/// WinUI's keyboard focus visual: a 2px outer ring and a 1px inner ring,
/// drawn 3px outside the control (`FocusVisualMargin="-3"`). The element it is
/// added to must be `relative()`.
pub fn focus_ring(radius: Pixels, p: &Palette) -> Div {
    div()
        .absolute()
        .top(px(-3.0))
        .left(px(-3.0))
        .right(px(-3.0))
        .bottom(px(-3.0))
        .rounded(radius + px(3.0))
        .border_2()
        .border_color(p.focus_outer)
        .child(
            div()
                .size_full()
                .rounded(radius + px(1.0))
                .border_1()
                .border_color(p.focus_inner),
        )
}

/// Adds the focus ring to a control when it should show one.
pub fn with_focus_ring<E: ParentElement>(element: E, show: bool, radius: Pixels, p: &Palette) -> E {
    if show {
        let mut element = element;
        element.extend([gpui::IntoElement::into_any_element(focus_ring(radius, p))]);
        element
    } else {
        element
    }
}

/// Whether a control with this focus handle shows its focus ring: when it
/// holds focus and the last input was the keyboard, as WinUI's
/// `FocusState::Keyboard`, or when the gallery forces it.
pub fn shows_focus(handle: &gpui::FocusHandle, shown: ShownState, window: &gpui::Window) -> bool {
    shown.shows_focus() || (handle.is_focused(window) && window.last_input_was_keyboard())
}

/// A focus handle that lives as long as the element with this id keeps
/// being drawn, so a builder-style control can be a tab stop.
pub fn keyed_focus(
    id: impl Into<gpui::ElementId>,
    window: &mut gpui::Window,
    cx: &mut App,
) -> gpui::FocusHandle {
    window
        .use_keyed_state(id, cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone()
}

/// An id for a part of a control, derived from the control's own id so two
/// controls' parts never share state.
pub fn child_id(id: &gpui::ElementId, part: &'static str) -> gpui::ElementId {
    gpui::ElementId::NamedChild(std::sync::Arc::new(id.clone()), part.into())
}

/// A handler a key press can call as well as a pointer.
pub type Handler = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut App)>;

/// A handler that is told the value a control changed to.
pub type ValueHandler<T> = std::rc::Rc<dyn Fn(T, &mut gpui::Window, &mut App)>;

/// The keys a control reacts to, named once so every control's keyboard
/// handling is a pure function of this enum and can be tested without a
/// window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Key {
    Enter,
    Space,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    /// Alt+Down and F4: open a drop-down.
    OpenDropDown,
    /// Ctrl+Tab.
    NextTab,
    /// Ctrl+Shift+Tab.
    PreviousTab,
    /// Ctrl+W and Ctrl+F4.
    CloseTab,
}

impl Key {
    /// Reads a key from GPUI's key name and modifiers.
    pub fn parse(name: &str, control: bool, alt: bool, shift: bool) -> Option<Key> {
        Some(match (name, control, alt, shift) {
            ("down", false, true, _) | ("f4", false, false, false) => Key::OpenDropDown,
            ("tab", true, false, false) => Key::NextTab,
            ("tab", true, false, true) => Key::PreviousTab,
            ("w", true, false, false) | ("f4", true, false, false) => Key::CloseTab,
            (_, true, _, _) | (_, _, true, _) => return None,
            ("enter", ..) => Key::Enter,
            ("space", ..) => Key::Space,
            ("escape", ..) => Key::Escape,
            ("up", ..) => Key::Up,
            ("down", ..) => Key::Down,
            ("left", ..) => Key::Left,
            ("right", ..) => Key::Right,
            ("home", ..) => Key::Home,
            ("end", ..) => Key::End,
            ("pageup", ..) => Key::PageUp,
            ("pagedown", ..) => Key::PageDown,
            _ => return None,
        })
    }

    pub fn from_keystroke(keystroke: &Keystroke) -> Option<Key> {
        let m = &keystroke.modifiers;
        Key::parse(&keystroke.key, m.control, m.alt, m.shift)
    }

    /// Whether the key invokes a button: Space, or Enter
    /// (`ButtonBaseKeyProcess.h`'s `IsPress`).
    pub fn invokes(self) -> bool {
        matches!(self, Key::Enter | Key::Space)
    }
}

/// Moves a highlight through `len` items with the arrow, Home, and End keys,
/// skipping the indices `skip` says cannot take it (headers, separators,
/// disabled items). `wrap` makes the ends meet, as a menu does; a list stops
/// at its ends. Returns `None` when the key does not move anything.
pub fn step_index(
    current: Option<usize>,
    len: usize,
    key: Key,
    wrap: bool,
    skip: impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let first = (0..len).find(|&i| !skip(i))?;
    let last = (0..len).rev().find(|&i| !skip(i))?;
    let walk = |from: usize, forward: bool| -> Option<usize> {
        let mut i = from;
        for _ in 0..len {
            i = match (forward, i + 1 >= len, i == 0) {
                (true, true, _) if wrap => 0,
                (true, true, _) => return None,
                (true, false, _) => i + 1,
                (false, _, true) if wrap => len - 1,
                (false, _, true) => return None,
                (false, _, false) => i - 1,
            };
            if !skip(i) {
                return Some(i);
            }
        }
        None
    };
    let next = match (key, current) {
        (Key::Home, _) => Some(first),
        (Key::End, _) => Some(last),
        (Key::Down | Key::Right, None) => Some(first),
        (Key::Up | Key::Left, None) => Some(last),
        (Key::Down | Key::Right, Some(at)) => walk(at.min(len - 1), true),
        (Key::Up | Key::Left, Some(at)) => walk(at.min(len - 1), false),
        _ => None,
    };
    next.filter(|&n| Some(n) != current)
}

/// The words every window says the same way, in English and in Taiwan
/// Mandarin, so on/off, common states, and units read the same in all eight
/// windows. A window's own copy stays in its own `i18n` module; these are the
/// shared ones.
pub mod words {
    /// The two languages Better OS ships. `system` in a window's settings
    /// resolves to one of these through [`Locale::from_tag`].
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
    pub enum Locale {
        #[default]
        English,
        /// zh-TW.
        TraditionalChinese,
    }

    impl Locale {
        /// Reads a locale tag such as `zh_TW.UTF-8`, `zh-Hant`, or `en_US`.
        /// Any Chinese tag that is not Simplified reads as zh-TW; anything
        /// else is English.
        pub fn from_tag(tag: &str) -> Locale {
            let tag = tag.to_ascii_lowercase().replace('_', "-");
            let simplified = ["zh-cn", "zh-sg", "zh-hans"]
                .iter()
                .any(|prefix| tag.starts_with(prefix));
            if tag.starts_with("zh") && !simplified {
                Locale::TraditionalChinese
            } else {
                Locale::English
            }
        }

        /// The process's locale, read the way glibc does: `LC_ALL`, then
        /// `LC_MESSAGES`, then `LANG`.
        pub fn from_environment() -> Locale {
            ["LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .filter_map(|name| std::env::var(name).ok())
                .find(|value| !value.is_empty())
                .map(|value| Locale::from_tag(&value))
                .unwrap_or_default()
        }

        fn pick(self, en: &'static str, zh: &'static str) -> &'static str {
            match self {
                Locale::English => en,
                Locale::TraditionalChinese => zh,
            }
        }
    }

    /// A toggle switch's On and Off (`ToggleSwitch.OnContent`/`OffContent`).
    pub fn on_off(locale: Locale, on: bool) -> &'static str {
        if on {
            locale.pick("On", "開啟")
        } else {
            locale.pick("Off", "關閉")
        }
    }

    pub fn yes_no(locale: Locale, yes: bool) -> &'static str {
        if yes {
            locale.pick("Yes", "是")
        } else {
            locale.pick("No", "否")
        }
    }

    /// States a status line or a badge reports.
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
    pub enum State {
        Enabled,
        Disabled,
        Running,
        Stopped,
        Loading,
        Ready,
        Applying,
        Failed,
        Unavailable,
        Unknown,
    }

    pub fn state(locale: Locale, state: State) -> &'static str {
        match state {
            State::Enabled => locale.pick("Enabled", "已啟用"),
            State::Disabled => locale.pick("Disabled", "已停用"),
            State::Running => locale.pick("Running", "執行中"),
            State::Stopped => locale.pick("Stopped", "已停止"),
            State::Loading => locale.pick("Loading", "正在載入"),
            State::Ready => locale.pick("Ready", "就緒"),
            State::Applying => locale.pick("Applying settings", "正在套用設定"),
            State::Failed => locale.pick("Failed", "失敗"),
            State::Unavailable => locale.pick("Unavailable", "無法使用"),
            State::Unknown => locale.pick("Unknown", "未知"),
        }
    }

    /// The words of the buttons every dialog and bar uses.
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
    pub enum Action {
        Ok,
        Cancel,
        Close,
        Save,
        Apply,
        Retry,
        Dismiss,
        ShowDetails,
        HideDetails,
        CopyDetails,
        More,
    }

    pub fn action(locale: Locale, action: Action) -> &'static str {
        match action {
            Action::Ok => locale.pick("OK", "確定"),
            Action::Cancel => locale.pick("Cancel", "取消"),
            Action::Close => locale.pick("Close", "關閉"),
            Action::Save => locale.pick("Save", "儲存"),
            Action::Apply => locale.pick("Apply", "套用"),
            Action::Retry => locale.pick("Try again", "再試一次"),
            Action::Dismiss => locale.pick("Dismiss", "關閉通知"),
            Action::ShowDetails => locale.pick("Show details", "顯示詳細資料"),
            Action::HideDetails => locale.pick("Hide details", "隱藏詳細資料"),
            Action::CopyDetails => locale.pick("Copy details", "複製詳細資料"),
            Action::More => locale.pick("More options", "更多選項"),
        }
    }

    /// A byte count in binary units with one decimal, as Windows shows sizes:
    /// `512 B`, `1.5 KB`, `3.2 GB`. Windows writes KB for 1024 bytes and so
    /// does this; the units are the same words in both languages.
    pub fn bytes(value: u64) -> String {
        const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
        if value < 1024 {
            return format!("{value} B");
        }
        let mut amount = value as f64;
        let mut unit = 0;
        while amount >= 1024.0 && unit < UNITS.len() - 1 {
            amount /= 1024.0;
            unit += 1;
        }
        if amount >= 100.0 {
            format!("{amount:.0} {}", UNITS[unit])
        } else {
            format!("{amount:.1} {}", UNITS[unit])
        }
    }

    /// A percentage from a fraction between 0 and 1, clamped, no decimals.
    pub fn percent(fraction: f32) -> String {
        let value = if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0)
        };
        format!("{}%", (value * 100.0).round() as u32)
    }

    /// A duration a person reads, to the largest two units:
    /// `45 s`, `3 min 20 s`, `2 h 5 min` / `45 秒`, `3 分 20 秒`, `2 小時 5 分`.
    pub fn duration(locale: Locale, seconds: u64) -> String {
        let (h, m, s) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
        let (hu, mu, su, sep) = match locale {
            Locale::English => (" h", " min", " s", " "),
            Locale::TraditionalChinese => (" 小時", " 分", " 秒", " "),
        };
        match (h, m) {
            (0, 0) => format!("{s}{su}"),
            (0, _) if s == 0 => format!("{m}{mu}"),
            (0, _) => format!("{m}{mu}{sep}{s}{su}"),
            _ if m == 0 => format!("{h}{hu}"),
            _ => format!("{h}{hu}{sep}{m}{mu}"),
        }
    }
}
