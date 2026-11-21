//! Plain symbols every Better OS window may need that gpui-component's icon
//! set does not have, drawn in the same 24px, 2px-stroke grid as the kit's
//! Lucide icons (Lucide's own shapes, ISC). Pictures, Refresh among them,
//! live in [`crate::icon_set`].
//!
//! A window that draws one serves them with [`load`] before falling back to
//! the kit's assets, and draws one with `Icon::empty().path(WRAP)`.

/// Wrap lines: a line that runs on and turns back under itself, as Lucide's
/// `text-wrap` (ticket 123, Better Git's diff).
pub const WRAP: &str = "better-ui/wrap.svg";

const WRAP_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m16 16-3 3 3 3"/><path d="M3 12h14.5a1 1 0 0 1 0 7H13"/><path d="M3 19h6"/><path d="M3 5h18"/></svg>"#;

/// The SVG for one of these glyphs' paths, or `None` for any other path.
pub fn load(path: &str) -> Option<&'static [u8]> {
    match path {
        WRAP => Some(WRAP_SVG.as_bytes()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_an_svg_whose_tag_comes_first() {
        let svg = std::str::from_utf8(load(WRAP).unwrap()).unwrap();
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!(load("icons/redo.svg").is_none());
        // Refresh is a picture now, served by the icon set.
        assert!(load("better-ui/refresh.svg").is_none());
    }
}
