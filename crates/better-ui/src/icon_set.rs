//! Better OS's own icon set (ticket 147, DESIGN.md's Icons section): every
//! object and action picture by name, drawn once here and asked for by name
//! from every window.
//!
//! The window's `AssetSource` serves every file with [`load`] before falling
//! back to the kit's assets:
//!
//! ```ignore
//! if let Some(svg) = better_ui::icon_set::load(path) {
//!     return Ok(Some(Cow::Borrowed(svg)));
//! }
//! ```
//!
//! An object icon is full colour, so it cannot go through the toolkit's
//! `Icon`, which paints an SVG as a mask in the text colour; [`picture`]
//! draws it with `img`. An action icon is the opposite: its files are masks
//! and the window gives the colour, so [`action`] draws each of its two
//! layers with `svg`, tinted for where the icon sits.

mod catalog;
#[cfg(test)]
mod tests;

pub use catalog::{ASSET_PREFIX, INTRINSIC, Kind, LARGE_INTRINSIC, Picture, load};

use gpui::{
    AnyElement, Div, Img, IntoElement, ObjectFit, ParentElement, Pixels, Rgba, SharedString,
    Styled, StyledImage, div, img, prelude::FluentBuilder, px, svg,
};
use gpui_component::{Icon, IconName};

use crate::fluent::Palette;

/// How opaque a disabled icon is: the same as disabled text.
pub const DISABLED_OPACITY: f32 = 0.36;

/// What a kit control draws as its icon. Every control that shows an icon
/// takes `impl Into<Glyph>`, so a window hands it a toolkit symbol or a
/// picture from this set the same way.
#[derive(Clone)]
pub enum Glyph {
    /// A toolkit line icon in the control's text colour: a plain symbol
    /// (arrow, chevron, close, check) or a status mark.
    Symbol(IconName),
    /// An SVG the window serves itself, drawn like a symbol.
    Path(SharedString),
    /// A toolkit icon already built (sized or coloured by the caller),
    /// boxed because a built `Icon` is large.
    Icon(Box<Icon>),
    /// An object or action picture from this set.
    Picture(Picture),
}

impl From<IconName> for Glyph {
    fn from(name: IconName) -> Self {
        Glyph::Symbol(name)
    }
}

impl From<Icon> for Glyph {
    fn from(icon: Icon) -> Self {
        Glyph::Icon(Box::new(icon))
    }
}

impl From<Picture> for Glyph {
    fn from(picture: Picture) -> Self {
        Glyph::Picture(picture)
    }
}

impl Glyph {
    /// The picture, when this is one.
    pub fn picture(&self) -> Option<Picture> {
        match self {
            Glyph::Picture(picture) => Some(*picture),
            _ => None,
        }
    }

    /// The size a control's icon slot should be for this glyph: an action
    /// alone is 18px (DESIGN.md, Icons); everything else keeps the
    /// control's own `standard` size.
    pub fn slot(&self, standard: Pixels, placement: Placement) -> Pixels {
        match self {
            Glyph::Picture(picture)
                if picture.kind() == Kind::Action && placement == Placement::Alone =>
            {
                px(Placement::Alone.size())
            }
            _ => standard,
        }
    }

    /// The glyph drawn `size` square, the slot it is given. A symbol or a
    /// window's SVG is in the inherited text colour, as the kit always drew
    /// it; a picture is drawn at `size` too, an action in the colours
    /// `placement` gives it. `disabled` dims a picture (a symbol takes the
    /// control's disabled text colour already). A control that places an
    /// action alone asks [`Glyph::slot`] for its size first.
    pub fn element(
        &self,
        size: Pixels,
        placement: Placement,
        palette: &Palette,
        disabled: bool,
    ) -> AnyElement {
        match self {
            Glyph::Symbol(name) => Icon::new(name.clone()).size(size).into_any_element(),
            Glyph::Path(path) => Icon::empty()
                .path(path.clone())
                .size(size)
                .into_any_element(),
            Glyph::Icon(icon) => (**icon).clone().size(size).into_any_element(),
            Glyph::Picture(picture) => match picture.kind() {
                Kind::Object if disabled => {
                    picture_disabled(*picture, f32::from(size)).into_any_element()
                }
                Kind::Object => self::picture(*picture, f32::from(size)).into_any_element(),
                Kind::Action => {
                    action_sized(*picture, placement, palette, disabled, f32::from(size))
                        .into_any_element()
                }
            },
        }
    }
}

/// An object icon drawn `size` pixels square. Row icons use 16 or 20.
pub fn picture(picture: Picture, size: f32) -> Img {
    debug_assert_eq!(
        picture.kind(),
        Kind::Object,
        "{} is an action",
        picture.name()
    );
    img(picture.path_for(size))
        .size(px(size))
        .flex_none()
        .object_fit(ObjectFit::Contain)
}

/// [`picture`] for a control that is disabled: drawn at
/// [`DISABLED_OPACITY`].
pub fn picture_disabled(picture: Picture, size: f32) -> Img {
    self::picture(picture, size).opacity(DISABLED_OPACITY)
}

/// Where an action icon sits, which decides its colour and size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Placement {
    /// Alone, as in a toolbar button: the accent, 18px.
    Alone,
    /// Beside a text label, as in a menu row or a labelled button: the
    /// secondary text colour, 16px, so the words lead.
    Labelled,
    /// On an accent-filled button, where the accent would vanish: the text
    /// on accent colour and a shade of it toward the accent, 16px.
    OnAccent,
}

impl Placement {
    /// Where a control's icon sits: on an accent fill (an accent button, a
    /// checked toggle) it turns over; otherwise it is alone without a label
    /// and beside the label with one.
    pub const fn for_control(accent_fill: bool, icon_only: bool) -> Placement {
        if accent_fill {
            Placement::OnAccent
        } else if icon_only {
            Placement::Alone
        } else {
            Placement::Labelled
        }
    }

    /// The size an action icon is drawn at here.
    pub const fn size(self) -> f32 {
        match self {
            Placement::Alone => 18.0,
            Placement::Labelled | Placement::OnAccent => 16.0,
        }
    }
}

/// The two colours an action icon is drawn in: its body, and its minor
/// layer's lighter shade. A destructive action is the critical colour in
/// either placement. On a disabled accent button the icon takes the
/// button's own disabled text colour, as its label does.
pub fn action_colours(
    picture: Picture,
    placement: Placement,
    palette: &Palette,
    disabled: bool,
) -> (Rgba, Rgba) {
    match placement {
        Placement::OnAccent if disabled => (
            palette.text_on_accent_disabled,
            crate::fluent::mix(
                palette.text_on_accent_disabled,
                palette.window_background,
                0.45,
            ),
        ),
        Placement::OnAccent => (
            palette.text_on_accent,
            crate::fluent::mix(palette.text_on_accent, palette.accent, 0.45),
        ),
        _ if picture.destructive() => (palette.critical, palette.icon_critical_2),
        Placement::Alone => (palette.accent, palette.icon_accent_2),
        Placement::Labelled => (palette.icon_quiet, palette.icon_quiet_2),
    }
}

/// An action icon at its placement's size.
pub fn action(picture: Picture, placement: Placement, palette: &Palette, disabled: bool) -> Div {
    action_sized(picture, placement, palette, disabled, placement.size())
}

/// An action icon at a size of its own: its layers stacked and tinted, the
/// whole drawn at [`DISABLED_OPACITY`] when the control is disabled, except
/// on an accent fill, where the disabled colours are already the button's.
pub fn action_sized(
    picture: Picture,
    placement: Placement,
    palette: &Palette,
    disabled: bool,
    size: f32,
) -> Div {
    debug_assert_eq!(
        picture.kind(),
        Kind::Action,
        "{} is an object",
        picture.name()
    );
    let (body, minor) = action_colours(picture, placement, palette, disabled);
    let dimmed = disabled && placement != Placement::OnAccent;
    let layer = |path: &'static str, colour: Rgba| {
        svg()
            .path(path)
            .absolute()
            .top_0()
            .left_0()
            .size(px(size))
            .text_color(colour)
    };
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .children(picture.second_layer().map(|(path, _)| layer(path, minor)))
        .child(layer(picture.path(), body))
        .when(dimmed, |icon| icon.opacity(DISABLED_OPACITY))
}

#[cfg(test)]
mod renderer_tests {
    use std::sync::Arc;

    use gpui::SvgRenderer;

    use super::{Kind, Picture, Placement, action_colours};
    use crate::fluent::{DARK, LIGHT, second_layer, with_accent};

    /// Which of the 48×48 pixels GPUI's renderer covers for one file.
    fn covered(renderer: &SvgRenderer, name: &str, svg: &str) -> Vec<bool> {
        let image = renderer
            .render_single_frame(svg.as_bytes(), 1.0)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (48, 48), "{name}");
        image
            .as_bytes(0)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3] > 128)
            .collect()
    }

    /// GPUI's own SVG renderer (usvg and resvg) reads every file and draws
    /// something: no file it would refuse, and none it would draw empty. The
    /// whole icon (an action's two layers together) covers enough of the
    /// square to be a picture, not a mark.
    #[test]
    fn gpui_draws_every_picture() {
        let renderer = SvgRenderer::new(Arc::new(()));
        let mut thin = Vec::new();
        for &picture in Picture::ALL {
            let mut drawn = covered(&renderer, picture.name(), picture.svg());
            assert!(drawn.contains(&true), "{} draws nothing", picture.name());
            if let Some((path, svg)) = picture.second_layer() {
                let second = covered(&renderer, path, svg);
                assert!(second.contains(&true), "{path} draws nothing");
                for (pixel, other) in drawn.iter_mut().zip(second) {
                    *pixel |= other;
                }
            }
            let count = drawn.iter().filter(|p| **p).count();
            // An object covers a sixth of the square; an action an eighth,
            // because the owner's approved Refresh is two arcs and covers 382
            // of 2304 pixels.
            let floor = match picture.kind() {
                Kind::Object => 48 * 48 / 6,
                Kind::Action => 48 * 48 / 8,
            };
            if count <= floor {
                thin.push((picture.name(), count));
            }
        }
        assert!(thin.is_empty(), "too little drawn: {thin:?}");
    }

    /// The colours follow the placement, a destructive action is critical
    /// everywhere, and a desktop accent brings its own lighter shade.
    #[test]
    fn an_action_takes_its_colours_from_where_it_sits() {
        assert_eq!(
            action_colours(Picture::Copy, Placement::Alone, &LIGHT, false),
            (LIGHT.accent, LIGHT.icon_accent_2)
        );
        assert_eq!(
            action_colours(Picture::Copy, Placement::Labelled, &DARK, false),
            (DARK.icon_quiet, DARK.icon_quiet_2)
        );
        for placement in [Placement::Alone, Placement::Labelled] {
            assert_eq!(
                action_colours(Picture::Delete, placement, &LIGHT, false),
                (LIGHT.critical, LIGHT.icon_critical_2)
            );
        }
        // On an accent fill the icon turns over, destructive or not, so it
        // never sits accent on accent.
        for picture in [Picture::Copy, Picture::Delete] {
            let (body, minor) = action_colours(picture, Placement::OnAccent, &LIGHT, false);
            assert_eq!(body, LIGHT.text_on_accent);
            assert_ne!(minor, LIGHT.accent);
            assert_ne!(minor, body);
        }
        // Disabled on an accent fill, the icon is the button's disabled
        // text colour, as its label is, not the live colour dimmed.
        let (body, minor) = action_colours(Picture::Copy, Placement::OnAccent, &LIGHT, true);
        assert_eq!(body, LIGHT.text_on_accent_disabled);
        assert_ne!(minor, body);
        assert_eq!(Placement::Alone.size(), 18.0);
        assert_eq!(Placement::Labelled.size(), 16.0);
        let purple = crate::fluent::hex(0x9141ac);
        let accented = with_accent(&DARK, purple, crate::fluent::hex(0xffffff));
        assert_eq!(accented.icon_accent_2, second_layer(purple, true));
        assert_ne!(accented.icon_accent_2, DARK.icon_accent_2);
    }

    /// A checked toggle and an accent button are accent-filled, so their
    /// icon turns over; otherwise an icon alone and one beside a label differ.
    #[test]
    fn a_control_places_its_icon_by_its_fill_and_its_label() {
        assert_eq!(Placement::for_control(true, true), Placement::OnAccent);
        assert_eq!(Placement::for_control(true, false), Placement::OnAccent);
        assert_eq!(Placement::for_control(false, true), Placement::Alone);
        assert_eq!(Placement::for_control(false, false), Placement::Labelled);
    }

    /// An action alone gets the 18px slot; every other glyph keeps the
    /// control's own size, and an action is drawn at whatever slot it gets.
    #[test]
    fn an_action_alone_gets_an_eighteen_pixel_slot() {
        use super::Glyph;
        use gpui::px;
        let copy = Glyph::from(Picture::Copy);
        assert_eq!(copy.slot(px(16.0), Placement::Alone), px(18.0));
        assert_eq!(copy.slot(px(16.0), Placement::Labelled), px(16.0));
        assert_eq!(copy.slot(px(48.0), Placement::OnAccent), px(48.0));
        let folder = Glyph::from(Picture::Folder);
        assert_eq!(folder.slot(px(16.0), Placement::Alone), px(16.0));
    }

    /// GPUI rasterises an `img` SVG at twice the size its file declares, so
    /// every size a window draws an object at must come from a file that
    /// declares at least that size: then the raster covers it on a 2x
    /// display. Checked through GPUI's own renderer for every size up to
    /// the largest the kit and the windows use (64px, Better Launcher's
    /// preview).
    #[test]
    fn an_object_is_rasterised_at_least_twice_the_size_it_is_drawn_at() {
        let renderer = SvgRenderer::new(Arc::new(()));
        for size in [12.0, 16.0, 20.0, 24.0, 32.0, 40.0, 48.0, 64.0_f32] {
            for picture in [Picture::Folder, Picture::Computer, Picture::Disc] {
                let path = picture.path_for(size);
                let bytes = super::load(path).expect(path);
                let image = renderer.render_single_frame(bytes, 1.0).unwrap();
                let width = image.size(0).width.0 as f32;
                assert!(
                    width >= 2.0 * size,
                    "{} at {size}px rasterises to {width}px from {path}",
                    picture.name()
                );
            }
        }
        assert_eq!(super::LARGE_INTRINSIC, 64.0);
    }

    /// The lighter shade is solid and sits between the body and the window
    /// background, nearer the background in light and nearer the body in
    /// dark.
    #[test]
    fn the_second_layer_is_a_solid_shade_toward_the_window() {
        for (palette, body, shade) in [
            (&LIGHT, LIGHT.accent, LIGHT.icon_accent_2),
            (&DARK, DARK.accent, DARK.icon_accent_2),
            (&LIGHT, LIGHT.icon_quiet, LIGHT.icon_quiet_2),
            (&DARK, DARK.critical, DARK.icon_critical_2),
        ] {
            assert_eq!(shade.a, 1.0);
            let window = palette.window_background;
            for (b, s, w) in [
                (body.r, shade.r, window.r),
                (body.g, shade.g, window.g),
                (body.b, shade.b, window.b),
            ] {
                assert!(s >= b.min(w) - 1e-6 && s <= b.max(w) + 1e-6);
            }
        }
    }
}
