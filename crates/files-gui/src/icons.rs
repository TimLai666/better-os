//! The few glyphs the kit's icon set does not have, drawn in the same 24px,
//! 2px-stroke grid as the rest (Lucide's shapes, ISC), and served beside
//! the kit's own icons: a trash can, and one icon for each of the person's
//! own folders so a compact sidebar tells them apart.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub const TRASH: &str = "better-files/trash.svg";
pub const DOCUMENTS: &str = "better-files/documents.svg";
pub const DOWNLOADS: &str = "better-files/downloads.svg";
pub const PICTURES: &str = "better-files/pictures.svg";
pub const MUSIC: &str = "better-files/music.svg";
pub const VIDEOS: &str = "better-files/videos.svg";
pub const DESKTOP: &str = "better-files/desktop.svg";

const OPEN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#;

fn svg(path: &str) -> Option<String> {
    let body = match path {
        TRASH => {
            r#"<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M10 11v6"/><path d="M14 11v6"/>"#
        }
        DOCUMENTS => {
            r#"<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M16 13H8"/><path d="M16 17H8"/><path d="M10 9H8"/>"#
        }
        DOWNLOADS => {
            r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="M7 10l5 5 5-5"/><path d="M12 15V3"/>"#
        }
        PICTURES => {
            r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="M21 15l-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#
        }
        MUSIC => {
            r#"<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>"#
        }
        VIDEOS => {
            r#"<path d="M16 13l5.2 3.5a.5.5 0 0 0 .8-.4V7.9a.5.5 0 0 0-.8-.4L16 11"/><rect x="2" y="6" width="14" height="12" rx="2"/>"#
        }
        DESKTOP => {
            r#"<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>"#
        }
        _ => return None,
    };
    Some(format!("{OPEN}{body}</svg>"))
}

/// The kit's icons and the window's own.
pub struct FilesAssets;

impl AssetSource for FilesAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(svg) = svg(path) {
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

/// The window's own glyph for a sidebar place (by its row key,
/// `place-<user-dirs key>`), when it has one.
pub fn place_icon(location: &files_core::Location, key: &str) -> Option<&'static str> {
    use files_platform::UserDirectory as D;
    if matches!(location, files_core::Location::Trash(_)) {
        return Some(TRASH);
    }
    [
        (D::Documents, DOCUMENTS),
        (D::Downloads, DOWNLOADS),
        (D::Pictures, PICTURES),
        (D::Music, MUSIC),
        (D::Videos, VIDEOS),
        (D::Desktop, DESKTOP),
    ]
    .into_iter()
    .find(|(directory, _)| key.strip_prefix("place-") == Some(directory.key()))
    .map(|(_, icon)| icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_served_and_is_an_svg() {
        for path in [
            TRASH, DOCUMENTS, DOWNLOADS, PICTURES, MUSIC, VIDEOS, DESKTOP,
        ] {
            let bytes = FilesAssets.load(path).unwrap().expect(path);
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(
                text.starts_with("<svg") && text.ends_with("</svg>"),
                "{path}"
            );
        }
        // The kit's own still come through.
        assert!(FilesAssets.load("icons/folder.svg").unwrap().is_some());
    }
}
