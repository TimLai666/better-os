//! A frame as a PNG file's bytes (spec S2).

use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use screenshot_core::compose::Frame;

/// Encodes a frame as an 8-bit RGBA PNG.
pub fn encode(frame: &Frame) -> Result<Vec<u8>, image::ImageError> {
    let mut bytes = Vec::new();
    // GNOME's `composite_to_stream` saves through GdkPixbuf at its default
    // compression; a fast filter keeps a 4K picture well under a second.
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::Adaptive)
        .write_image(
            &frame.to_rgba(),
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgba8,
        )?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::encode;
    use screenshot_core::compose::Frame;

    #[test]
    fn a_frame_round_trips_through_png_with_its_colours() {
        let frame = Frame::filled(3, 2, [10, 20, 30, 255]); // blue, green, red
        let bytes = encode(&frame).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (3, 2));
        assert_eq!(decoded.get_pixel(2, 1).0, [30, 20, 10, 255]);
    }
}
