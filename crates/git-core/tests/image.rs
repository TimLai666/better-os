use std::path::Path;

use git_core::{ImageKind, image_kind, sniff_image};

#[test]
fn image_kinds() {
    assert_eq!(image_kind(Path::new("a/B.PNG")), Some(ImageKind::Png));
    assert_eq!(image_kind(Path::new("x.jpeg")), Some(ImageKind::Jpeg));
    assert_eq!(image_kind(Path::new("x.txt")), None);
    assert_eq!(image_kind(Path::new("noext")), None);
    assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some(ImageKind::Png));
    assert_eq!(
        sniff_image(&[0xff, 0xd8, 0xff, 0xe0]),
        Some(ImageKind::Jpeg)
    );
    assert_eq!(sniff_image(b"GIF89a.."), Some(ImageKind::Gif));
    assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageKind::Webp));
    assert_eq!(sniff_image(b"  <svg xmlns=\"\"/>"), Some(ImageKind::Svg));
    assert_eq!(sniff_image(b"hello"), None);
}
