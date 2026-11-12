//! Which changed files are images the diff pane can draw side by side.

use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Svg,
    Ico,
    Tiff,
}

/// The image kind a path's extension names, case-insensitively.
pub fn image_kind(path: &Path) -> Option<ImageKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => ImageKind::Png,
        "jpg" | "jpeg" => ImageKind::Jpeg,
        "gif" => ImageKind::Gif,
        "webp" => ImageKind::Webp,
        "bmp" => ImageKind::Bmp,
        "svg" => ImageKind::Svg,
        "ico" => ImageKind::Ico,
        "tif" | "tiff" => ImageKind::Tiff,
        _ => return None,
    })
}

/// The image kind the bytes start with. An extension can lie; the window
/// draws only what the bytes say they are.
pub fn sniff_image(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageKind::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageKind::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageKind::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageKind::Webp)
    } else if bytes.starts_with(b"BM") {
        Some(ImageKind::Bmp)
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some(ImageKind::Ico)
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some(ImageKind::Tiff)
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
        let head = head.trim_start();
        (head.starts_with("<svg") || head.starts_with("<?xml") && head.contains("<svg"))
            .then_some(ImageKind::Svg)
    }
}
