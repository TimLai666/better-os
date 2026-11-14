//! Glyphs every Better OS window needs that gpui-component's icon set does
//! not have, drawn in the same 24px, 2px-stroke grid as the kit's Lucide
//! icons (Lucide's own shapes, ISC).
//!
//! Each window's `AssetSource` serves them with [`load`] before falling back
//! to the kit's assets, and draws one with `Icon::empty().path(REFRESH)` (or
//! a command's `icon_path`).

/// Refresh: two arrows round a circle, as Windows 11 File Explorer's Refresh.
/// The kit's nearest, `Redo`, is one curved arrow and reads as Redo.
pub const REFRESH: &str = "better-ui/refresh.svg";

const REFRESH_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/></svg>"#;

/// The SVG for one of these glyphs' paths, or `None` for any other path.
pub fn load(path: &str) -> Option<&'static [u8]> {
    match path {
        REFRESH => Some(REFRESH_SVG.as_bytes()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_an_svg_whose_tag_comes_first() {
        let svg = std::str::from_utf8(load(REFRESH).unwrap()).unwrap();
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!(load("icons/redo.svg").is_none());
    }
}
