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
    /// Alone, as in a toolbar button: the palette's icon accent (the
    /// desktop's accent, or the nearest tone of it that reads on every
    /// surface), 18px whatever the control's own icon size.
    Alone,
    /// Beside a text label, as in a menu row or a labelled button: the
    /// quiet grey, so the words lead, at the size of the control's own icon
    /// slot (16px in a button or menu row, 20px in a settings card, 13px in
    /// a tab, 12px in a chip).
    Labelled,
    /// On an accent-filled button or checked toggle: the palette's icon on
    /// accent (the theme's text on accent, or the nearest tone that reads on
    /// that fill at rest, under the pointer, and pressed) and a shade of it
    /// toward the fill, at the control's own icon slot.
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

    /// The size an action icon is drawn at here when the caller has no slot
    /// of its own ([`action`]); a control passes its slot through
    /// [`Glyph::slot`], which keeps 18px only for [`Placement::Alone`].
    pub const fn size(self) -> f32 {
        match self {
            Placement::Alone => 18.0,
            Placement::Labelled | Placement::OnAccent => 16.0,
        }
    }
}

/// The two colours an action icon is drawn in: its body, and its minor
/// layer's lighter shade. A destructive action is the critical colour in
/// either placement. Alone and on an accent fill the icon takes the
/// palette's icon roles, which always read on what they are drawn on,
/// never the accent itself (DESIGN.md, Icons). On a disabled accent button
/// the icon takes the button's own disabled text colour, as its label does.
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
        Placement::OnAccent => (palette.icon_on_accent, palette.icon_on_accent_2),
        _ if picture.destructive() => (palette.critical, palette.icon_critical_2),
        Placement::Alone => (palette.icon_accent, palette.icon_accent_2),
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
    use gpui::Rgba;

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

    /// Which pixels of a 96px render GPUI's renderer covers for one file.
    fn covered_at_96(renderer: &SvgRenderer, name: &str, svg: &str) -> Vec<bool> {
        let image = renderer
            .render_single_frame(svg.as_bytes(), 2.0)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(image.size(0).width.0, 96, "{name}");
        image
            .as_bytes(0)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3] > 128)
            .collect()
    }

    /// DESIGN.md: an action's minor part is the lighter layer, never its
    /// body, and the two layers do not overlap. So in every two-layer action
    /// the second layer paints less than the first, and no pixel of a 96px
    /// render is painted by both.
    #[test]
    fn an_actions_second_layer_is_the_smaller_and_never_overlaps() {
        let renderer = SvgRenderer::new(Arc::new(()));
        let mut faults = Vec::new();
        for &picture in Picture::ALL {
            let Some((path, second)) = picture.second_layer() else {
                continue;
            };
            let main = covered_at_96(&renderer, picture.name(), picture.svg());
            let minor = covered_at_96(&renderer, path, second);
            let main_area = main.iter().filter(|p| **p).count();
            let minor_area = minor.iter().filter(|p| **p).count();
            let both = main.iter().zip(&minor).filter(|(a, b)| **a && **b).count();
            if minor_area >= main_area {
                faults.push(format!(
                    "{}: second layer {minor_area} >= main {main_area}",
                    picture.name()
                ));
            }
            if both > 0 {
                faults.push(format!("{}: {both} pixels in both layers", picture.name()));
            }
        }
        assert!(faults.is_empty(), "{faults:#?}");
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
            (LIGHT.icon_accent, LIGHT.icon_accent_2)
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
            assert_eq!(body, LIGHT.icon_on_accent);
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
        assert_eq!(
            accented.icon_accent_2,
            second_layer(accented.icon_accent, true)
        );
        assert_ne!(accented.icon_accent_2, DARK.icon_accent_2);
    }

    /// The accents a desktop can hand the windows, with the text colour its
    /// theme puts on each: Windows 11's own, Zorin's light and dark blues,
    /// GNOME's nine, the extremes, and pairings whose text on accent does
    /// not read on the fill, so the on-accent check is a check and not an
    /// assumption.
    const ACCENTS: [(&str, Option<(u32, u32)>); 20] = [
        ("Windows 11 default", None),
        ("Windows 11 blues swapped", Some((0x60cdff, 0x000000))),
        ("Zorin blue", Some((0x15a6f0, 0xffffff))),
        ("Zorin blue's dark variant", Some((0xbde6fb, 0x161c1f))),
        ("pale with white on it", Some((0xbde6fb, 0xffffff))),
        ("GNOME blue", Some((0x3584e4, 0xffffff))),
        ("GNOME teal", Some((0x2190a4, 0xffffff))),
        ("GNOME green", Some((0x3a944a, 0xffffff))),
        ("GNOME yellow", Some((0xc88800, 0xffffff))),
        ("GNOME orange", Some((0xed5b00, 0xffffff))),
        ("GNOME red", Some((0xe62d42, 0xffffff))),
        ("GNOME pink", Some((0xd56199, 0xffffff))),
        ("GNOME purple", Some((0x9141ac, 0xffffff))),
        ("GNOME slate", Some((0x6f8396, 0xffffff))),
        ("near-black", Some((0x0a0a0a, 0xffffff))),
        ("near-black with black on it", Some((0x0a0a0a, 0x000000))),
        ("near-white", Some((0xf5f5f5, 0x161c1f))),
        ("near-white with white on it", Some((0xf5f5f5, 0xffffff))),
        ("mid grey", Some((0x808080, 0xffffff))),
        ("saturated yellow", Some((0xffd400, 0xffffff))),
    ];

    /// Every palette [`ACCENTS`] gives, in both appearances, with its name.
    fn accented_palettes() -> Vec<(String, crate::fluent::Palette)> {
        let mut out = Vec::new();
        for (name, accent) in ACCENTS {
            for base in [&LIGHT, &DARK] {
                let palette = match accent {
                    None => *base,
                    Some((fill, on)) => {
                        with_accent(base, crate::fluent::hex(fill), crate::fluent::hex(on))
                    }
                };
                let appearance = if base.is_dark() { "dark" } else { "light" };
                out.push((format!("{name} in {appearance}"), palette));
            }
        }
        out
    }

    /// The fills an accent button or checked toggle really shows, from the
    /// palette's own roles: at rest, under the pointer, and pressed, over
    /// the window and over a card.
    fn accent_fills(palette: &crate::fluent::Palette) -> Vec<Rgba> {
        use crate::fluent::over;
        let window = palette.window_background;
        let card = over(palette.card, window);
        let mut fills = Vec::new();
        for base in [window, card] {
            for fill in [
                palette.accent,
                palette.accent_secondary,
                palette.accent_tertiary,
            ] {
                fills.push(over(fill, base));
            }
        }
        fills
    }

    /// Whatever accent the desktop gives, every placement's icon reads on
    /// every surface it is drawn on: its main layer at 3:1 and its second
    /// layer a tone of its own, 1.5:1 from the main layer and from the
    /// surface. An icon alone or beside a label is checked on every fill a
    /// window draws one on (`icon_surfaces`: buttons and rows at rest, under
    /// the pointer and pressed, cards, menus, dialogs, info bars); one on an
    /// accent fill on that fill at rest, under the pointer, and pressed.
    /// Disabled icons are exempt, as WCAG exempts them.
    #[test]
    fn every_icon_reads_on_what_it_is_drawn_on_whatever_the_accent() {
        use crate::fluent::{ICON_CONTRAST, LAYER_CONTRAST, contrast, icon_surfaces};
        let mut faults = Vec::new();
        for (name, palette) in accented_palettes() {
            // The rule's own list, and the fills a review measured an icon
            // on, built here so a narrower list in the rule cannot pass.
            let window = palette.window_background;
            let card = crate::fluent::over(palette.card, window);
            let mut surfaces = icon_surfaces(&palette);
            surfaces.extend([
                crate::fluent::over(palette.subtle_fill, window),
                crate::fluent::over(palette.subtle_fill_tertiary, card),
                crate::fluent::over(palette.control_fill, card),
                crate::fluent::over(palette.control_fill_secondary, window),
                crate::fluent::over(palette.critical_background, window),
                crate::fluent::over(palette.tab_hover, window),
            ]);
            let fills = accent_fills(&palette);
            for picture in [Picture::Copy, Picture::Delete] {
                for placement in [Placement::Alone, Placement::Labelled, Placement::OnAccent] {
                    let (main, second) = action_colours(picture, placement, &palette, false);
                    let under: &[Rgba] = match placement {
                        Placement::OnAccent => &fills,
                        _ => &surfaces,
                    };
                    let what = format!("{name}, {} {placement:?}", picture.name());
                    if contrast(main, second) < LAYER_CONTRAST {
                        faults.push(format!("{what}: layers {:.2}:1", contrast(main, second)));
                    }
                    for surface in under {
                        if contrast(main, *surface) < ICON_CONTRAST {
                            faults.push(format!(
                                "{what}: main layer {:.2}:1 on {surface:?}",
                                contrast(main, *surface)
                            ));
                        }
                        if contrast(second, *surface) < LAYER_CONTRAST {
                            faults.push(format!(
                                "{what}: second layer {:.2}:1 on {surface:?}",
                                contrast(second, *surface)
                            ));
                        }
                    }
                }
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    /// Accent text (a link, an accent word, a shortcut key) keeps 4.5:1 on
    /// every surface text sits on, whatever the accent.
    #[test]
    fn accent_text_reads_whatever_the_accent() {
        use crate::fluent::{TEXT_CONTRAST, contrast, icon_surfaces};
        let mut faults = Vec::new();
        for (name, palette) in accented_palettes() {
            for surface in icon_surfaces(&palette) {
                let ratio = contrast(palette.accent_text, surface);
                if ratio < TEXT_CONTRAST {
                    faults.push(format!("{name}: accent text {ratio:.2}:1 on {surface:?}"));
                }
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    /// The icon roles change only what icons are drawn with: the accent
    /// fill stays the desktop's own, and a pale accent's icon alone keeps
    /// its hue's direction, a darker blue rather than a grey or black.
    #[test]
    fn the_icon_roles_leave_the_accent_fill_alone() {
        use crate::fluent::hex;
        let pale = hex(0xbde6fb);
        let accented = with_accent(&LIGHT, pale, hex(0x161c1f));
        assert_eq!(accented.accent, pale);
        assert_ne!(accented.icon_accent, pale);
        assert!(accented.icon_accent.b > accented.icon_accent.r);
        // A pale fill keeps its theme's dark text for icons on it.
        assert_eq!(accented.icon_on_accent, hex(0x161c1f));
        // White on that fill does not read, so an icon on it goes dark.
        let white_on_pale = with_accent(&LIGHT, pale, hex(0xffffff));
        let luminance = crate::fluent::relative_luminance;
        assert!(luminance(white_on_pale.icon_on_accent) < luminance(pale));
        assert!(
            crate::fluent::contrast(white_on_pale.icon_on_accent, pale)
                >= crate::fluent::ICON_CONTRAST
        );
    }

    /// The default palettes' icon roles are what the rule gives Windows 11's
    /// own accents: written out because a `const` cannot compute contrast.
    #[test]
    fn the_default_icon_roles_are_what_the_rule_gives() {
        for base in [&LIGHT, &DARK] {
            let rebuilt = with_accent(base, base.accent, base.text_on_accent);
            assert_eq!(rebuilt.icon_accent, base.icon_accent);
            assert_eq!(rebuilt.icon_accent_2, base.icon_accent_2);
            assert_eq!(rebuilt.icon_on_accent, base.icon_on_accent);
            assert_eq!(rebuilt.icon_on_accent_2, base.icon_on_accent_2);
        }
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
        let objects: Vec<Picture> = Picture::ALL
            .iter()
            .copied()
            .filter(|p| p.kind() == Kind::Object)
            .collect();
        assert_eq!(objects.len(), 86);
        for size in [12.0, 16.0, 20.0, 24.0, 32.0, 40.0, 48.0, 64.0_f32] {
            for &picture in &objects {
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
            (&LIGHT, LIGHT.icon_accent, LIGHT.icon_accent_2),
            (&DARK, DARK.icon_accent, DARK.icon_accent_2),
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
