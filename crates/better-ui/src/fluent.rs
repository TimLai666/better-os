//! The WinUI (Windows 11 Fluent) design tokens from `DESIGN.md`, for every
//! window that is moved to that look.
//!
//! Only tokens live here: the two color palettes, the type ramp, the 4px
//! spacing grid, the two corner radii, and the standard control and row sizes.
//! Nothing here knows about any one application. A window reads the palette
//! for the appearance gpui-component is currently in with [`palette`], and
//! [`apply_to_component_theme`] points gpui-component's own controls (text
//! fields, buttons, scrollbars) at the same colors so they do not stay in the
//! library's default look beside drawn surfaces.
//!
//! The color values are WinUI's default theme resources as recalled when
//! `DESIGN.md` was written; that file says they still have to be checked
//! against the WinUI source before a release ships them. Change them there
//! and here together.

use gpui::{App, FontWeight, Pixels, Rgba, Styled, px};
use gpui_component::{ActiveTheme, Theme};

/// One appearance's colors. The roles are the rows of `DESIGN.md`'s color
/// table, plus the subtle fill WinUI uses for hover and selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// The lowest layer: behind everything else.
    pub window_background: Rgba,
    /// Cards, panes, and the rest state of standard controls.
    pub card: Rgba,
    /// What floats above the page: dialogs, flyouts, menus.
    pub dialog: Rgba,
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    /// The one primary action, the selection indicator, and focus. Never
    /// decoration.
    pub accent: Rgba,
    pub text_on_accent: Rgba,
    /// The 1px edge of a control, a card, or a pane.
    pub stroke: Rgba,
    /// Hover and selection fill on a row or a command bar button.
    pub subtle_fill: Rgba,
    pub critical: Rgba,
    pub success: Rgba,
    pub caution: Rgba,
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

/// A color at an opacity between 0 and 1, for the stroke and status tints.
pub const fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: alpha,
    }
}

const BLACK: Rgba = hex(0x000000);
const WHITE: Rgba = hex(0xffffff);

pub const LIGHT: Palette = Palette {
    window_background: hex(0xf3f3f3),
    card: hex(0xfbfbfb),
    dialog: hex(0xffffff),
    text_primary: hex(0x1a1a1a),
    text_secondary: hex(0x5f5f5f),
    accent: hex(0x005fb8),
    text_on_accent: hex(0xffffff),
    stroke: with_alpha(BLACK, 0.06),
    // WinUI's SubtleFillColorSecondary, as recalled.
    subtle_fill: with_alpha(BLACK, 0.04),
    critical: hex(0xc42b1c),
    success: hex(0x0f7b0f),
    caution: hex(0x9d5d00),
};

pub const DARK: Palette = Palette {
    window_background: hex(0x202020),
    card: hex(0x2b2b2b),
    dialog: hex(0x2c2c2c),
    text_primary: hex(0xffffff),
    text_secondary: hex(0xc5c5c5),
    accent: hex(0x60cdff),
    text_on_accent: hex(0x000000),
    stroke: with_alpha(WHITE, 0.08),
    subtle_fill: with_alpha(WHITE, 0.06),
    critical: hex(0xff99a4),
    success: hex(0x6ccb5f),
    caution: hex(0xfce100),
};

/// The palette for one appearance.
pub const fn palette_for(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// The palette for the appearance gpui-component is in right now.
pub fn palette(cx: &App) -> &'static Palette {
    palette_for(cx.theme().is_dark())
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

    /// Everything that sits in the page.
    pub const SM: Pixels = px(4.0);
    /// What floats above it.
    pub const MD: Pixels = px(8.0);
}

/// Standard sizes from `DESIGN.md`'s layout section.
pub mod size {
    use super::{Pixels, px};

    /// A button, a single-line field.
    pub const CONTROL_HEIGHT: Pixels = px(32.0);
    /// A list row with one line.
    pub const ROW_HEIGHT: Pixels = px(36.0);
    /// A list row with a second line of caption text.
    pub const TWO_LINE_ROW_HEIGHT: Pixels = px(48.0);
    /// A navigation pane holding a list a person works in.
    pub const LIST_PANE_WIDTH: Pixels = px(320.0);
    /// A navigation pane holding only icons.
    pub const ICON_PANE_WIDTH: Pixels = px(48.0);
    /// The accent bar at a selected row's leading edge.
    pub const SELECTION_BAR_WIDTH: Pixels = px(3.0);
    pub const SELECTION_BAR_HEIGHT: Pixels = px(16.0);
}

/// Points gpui-component's own controls at the Fluent palette for the theme's
/// current appearance, and sets its radii to 4px.
///
/// Call it after every `Theme::change`: that call reloads the library's own
/// colors and would otherwise put them back.
pub fn apply_to_component_theme(theme: &mut Theme) {
    let p = palette_for(theme.is_dark());
    theme.radius = radius::SM;
    theme.radius_lg = radius::MD;
    theme.shadow = false;

    let colors = &mut theme.colors;
    colors.background = p.window_background.into();
    colors.foreground = p.text_primary.into();
    colors.muted = p.card.into();
    colors.muted_foreground = p.text_secondary.into();
    colors.border = p.stroke.into();
    colors.input = p.stroke.into();
    colors.ring = p.accent.into();
    colors.caret = p.text_primary.into();
    colors.selection = with_alpha(p.accent, 0.35).into();
    colors.accent = p.subtle_fill.into();
    colors.accent_foreground = p.text_primary.into();
    colors.primary = p.accent.into();
    colors.primary_hover = with_alpha(p.accent, 0.9).into();
    colors.primary_active = with_alpha(p.accent, 0.8).into();
    colors.primary_foreground = p.text_on_accent.into();
    colors.secondary = p.card.into();
    colors.secondary_hover = p.subtle_fill.into();
    colors.secondary_active = p.subtle_fill.into();
    colors.secondary_foreground = p.text_primary.into();
    colors.popover = p.dialog.into();
    colors.popover_foreground = p.text_primary.into();
    colors.title_bar = p.window_background.into();
    colors.title_bar_border = p.stroke.into();
    colors.danger = p.critical.into();
    colors.success = p.success.into();
    colors.warning = p.caution.into();
    colors.list = p.card.into();
    colors.list_hover = p.subtle_fill.into();
    colors.list_active = p.subtle_fill.into();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn byte(channel: f32) -> u32 {
        (channel * 255.0).round() as u32
    }

    fn rgb(color: Rgba) -> u32 {
        (byte(color.r) << 16) | (byte(color.g) << 8) | byte(color.b)
    }

    #[test]
    fn hex_reads_each_channel() {
        let color = hex(0x005fb8);
        assert_eq!(rgb(color), 0x005fb8);
        assert_eq!(color.a, 1.0);
    }

    /// The values are `DESIGN.md`'s table, row by row, so a change to one of
    /// them is a change to the design baseline and not a drive-by edit.
    #[test]
    fn the_palettes_are_the_design_table() {
        let rows = [
            (
                LIGHT.window_background,
                DARK.window_background,
                0xf3f3f3,
                0x202020,
            ),
            (LIGHT.card, DARK.card, 0xfbfbfb, 0x2b2b2b),
            (LIGHT.dialog, DARK.dialog, 0xffffff, 0x2c2c2c),
            (LIGHT.text_primary, DARK.text_primary, 0x1a1a1a, 0xffffff),
            (
                LIGHT.text_secondary,
                DARK.text_secondary,
                0x5f5f5f,
                0xc5c5c5,
            ),
            (LIGHT.accent, DARK.accent, 0x005fb8, 0x60cdff),
            (
                LIGHT.text_on_accent,
                DARK.text_on_accent,
                0xffffff,
                0x000000,
            ),
            (LIGHT.critical, DARK.critical, 0xc42b1c, 0xff99a4),
            (LIGHT.success, DARK.success, 0x0f7b0f, 0x6ccb5f),
            (LIGHT.caution, DARK.caution, 0x9d5d00, 0xfce100),
        ];
        for (light, dark, light_hex, dark_hex) in rows {
            assert_eq!(rgb(light), light_hex);
            assert_eq!(rgb(dark), dark_hex);
        }
        assert_eq!((rgb(LIGHT.stroke), LIGHT.stroke.a), (0x000000, 0.06));
        assert_eq!((rgb(DARK.stroke), DARK.stroke.a), (0xffffff, 0.08));
    }

    #[test]
    fn the_appearance_picks_its_palette() {
        assert_eq!(palette_for(true), &DARK);
        assert_eq!(palette_for(false), &LIGHT);
    }

    #[test]
    fn the_type_ramp_never_goes_below_body_except_for_captions() {
        for style in [BODY, BODY_STRONG, SUBTITLE, TITLE] {
            assert!(style.size >= 14.0);
            assert!(style.line_height > style.size);
        }
        assert_eq!((CAPTION.size, CAPTION.line_height), (12.0, 16.0));
        assert_eq!((CODE.size, CODE.line_height), (13.0, 20.0));
        const { assert!(TITLE.size >= SUBTITLE.size) };
    }

    #[test]
    fn spacing_is_on_the_four_pixel_grid() {
        for step in [
            spacing::XS,
            spacing::SM,
            spacing::MD,
            spacing::LG,
            spacing::XL,
        ] {
            assert_eq!(f32::from(step) % 4.0, 0.0);
        }
    }
}
