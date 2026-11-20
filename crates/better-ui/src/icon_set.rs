//! Better OS's own icon set (ticket 147, DESIGN.md's Icons section): every
//! object and action picture by name, drawn once here and asked for by name
//! from every window.
//!
//! A picture is full colour, so it cannot go through the toolkit's `Icon`,
//! which paints an SVG as a mask in the text colour. It is drawn with
//! `img(path)` instead, and the window's `AssetSource` serves the path with
//! [`load`] before falling back to the kit's assets:
//!
//! ```ignore
//! if let Some(svg) = better_ui::icon_set::load(path) {
//!     return Ok(Some(Cow::Borrowed(svg)));
//! }
//! ```
//!
//! and a view draws one with [`picture`] (or [`picture_disabled`]).

mod catalog;
#[cfg(test)]
mod tests;

pub use catalog::{ASSET_PREFIX, Kind, Picture, load};

use gpui::{Img, ObjectFit, Styled, StyledImage, img, px};

/// How opaque a disabled picture is: the same as disabled text.
pub const DISABLED_OPACITY: f32 = 0.36;

/// A picture drawn `size` pixels square. Row icons use 16 or 20.
pub fn picture(picture: Picture, size: f32) -> Img {
    img(picture.path())
        .size(px(size))
        .flex_none()
        .object_fit(ObjectFit::Contain)
}

/// [`picture`] for a control that is disabled: drawn at
/// [`DISABLED_OPACITY`].
pub fn picture_disabled(picture: Picture, size: f32) -> Img {
    self::picture(picture, size).opacity(DISABLED_OPACITY)
}

#[cfg(test)]
mod renderer_tests {
    use std::sync::Arc;

    use gpui::SvgRenderer;

    use super::Picture;

    /// GPUI's own SVG renderer (usvg and resvg) reads every picture and
    /// draws something: no file it would refuse, and none it would draw
    /// empty.
    #[test]
    fn gpui_draws_every_picture() {
        let renderer = SvgRenderer::new(Arc::new(()));
        let mut thin = Vec::new();
        for &picture in Picture::ALL {
            let image = renderer
                .render_single_frame(picture.svg().as_bytes(), 1.0)
                .unwrap_or_else(|e| panic!("{}: {e}", picture.name()));
            let size = image.size(0);
            assert_eq!((size.width.0, size.height.0), (48, 48), "{}", picture.name());
            let pixels = image.as_bytes(0).unwrap();
            let opaque = pixels.chunks_exact(4).filter(|p| p[3] > 128).count();
            // At least a sixth of the square is drawn: a picture, not a mark.
            if opaque <= 48 * 48 / 6 {
                thin.push((picture.name(), opaque));
            }
        }
        assert!(thin.is_empty(), "too little drawn: {thin:?}");
    }
}
