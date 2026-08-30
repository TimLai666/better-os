//! Images, decoded inside a budget.
//!
//! An image decoder is the most exposed parser a file manager runs: it is fed
//! whatever was on the USB stick, and a malformed header is the classic way to
//! ask a program to allocate four gigabytes. Three bounds are applied here, and
//! the order matters.
//!
//! 1. The engine has already refused anything over `max_source_bytes`, so the
//!    decoder never sees a file bigger than the policy allows.
//! 2. [`image::Limits`] is handed to the reader **before** `decode`, so the
//!    dimensions in the header are checked against `max_image_pixels` and the
//!    allocation against `max_decode_bytes` by the decoder itself. This is a
//!    refusal, not a crash.
//! 3. Dimensions are read separately first, so an oversized image is refused
//!    from its header without a single pixel being allocated.
//!
//! The format is decided by content, not by extension. `with_guessed_format`
//! reads the magic bytes; a `.png` that is really a JPEG previews correctly and
//! a `.png` that is really nothing at all is refused rather than misread.

use std::fs::File;
use std::io::BufReader;

use image::imageops::FilterType;
use image::{ImageFormat, ImageReader, Limits};

use crate::{
    CancelToken, DegradeReason, ImagePreview, Preview, PreviewError, PreviewProvider,
    PreviewRequest, extension_of, top_level_is,
};

/// Extensions claimed when no MIME type was resolved. Deliberately only the
/// formats this build can actually decode: claiming a format and then failing
/// is worse than not claiming it.
const IMAGE_EXTENSIONS: &[&str] = &["bmp", "gif", "jpeg", "jpg", "png"];

pub struct ImageProvider;

impl PreviewProvider for ImageProvider {
    fn id(&self) -> &'static str {
        "image"
    }

    fn handles(&self, request: &PreviewRequest) -> bool {
        if request.is_directory {
            return false;
        }
        top_level_is(request.mime.as_ref(), "image")
            || (request.mime.is_none()
                && IMAGE_EXTENSIONS.contains(&extension_of(&request.path).as_str()))
    }

    fn generate(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> Result<Preview, PreviewError> {
        cancel.check()?;
        let limits = &request.limits;

        let file = File::open(&request.path).map_err(|error| {
            PreviewError::Degraded(DegradeReason::Unreadable(error.to_string()))
        })?;
        let reader = ImageReader::new(BufReader::new(file))
            .with_guessed_format()
            .map_err(|error| {
                PreviewError::Degraded(DegradeReason::Unreadable(error.to_string()))
            })?;

        let Some(format) = reader.format() else {
            // No magic bytes matched. This is not an image, whatever the type
            // or the extension said, and saying so is more useful than a
            // decoder error.
            return Err(PreviewError::Degraded(DegradeReason::DecodeFailed(
                "the file has no recognized image header".to_string(),
            )));
        };
        cancel.check()?;

        // The header check. Dimensions consume the reader, so it is rebuilt
        // afterwards — one extra open, in exchange for never allocating for an
        // image whose own header says it is too big.
        let (source_width, source_height) = reader.into_dimensions().map_err(|error| {
            PreviewError::Degraded(decode_failure(&error, limits.max_image_pixels))
        })?;
        let pixels = u64::from(source_width) * u64::from(source_height);
        if pixels > limits.max_image_pixels {
            return Err(PreviewError::Degraded(DegradeReason::TooLarge {
                limit: limits.max_image_pixels,
                actual: pixels,
            }));
        }
        cancel.check()?;

        let file = File::open(&request.path).map_err(|error| {
            PreviewError::Degraded(DegradeReason::Unreadable(error.to_string()))
        })?;
        let mut reader = ImageReader::with_format(BufReader::new(file), format);
        let mut decoder_limits = Limits::default();
        decoder_limits.max_image_width =
            Some(limits.max_image_pixels.min(u64::from(u32::MAX)) as u32);
        decoder_limits.max_image_height =
            Some(limits.max_image_pixels.min(u64::from(u32::MAX)) as u32);
        decoder_limits.max_alloc = Some(limits.max_decode_bytes);
        reader.limits(decoder_limits);

        let decoded = reader.decode().map_err(|error| {
            PreviewError::Degraded(decode_failure(&error, limits.max_decode_bytes))
        })?;
        cancel.check()?;

        // Downscale on the worker thread rather than uploading a 24-megapixel
        // texture and letting the GPU sort it out.
        let edge = limits.thumbnail_edge.max(1);
        let thumbnail = if source_width > edge || source_height > edge {
            decoded.resize(edge, edge, FilterType::Triangle)
        } else {
            decoded
        };
        cancel.check()?;

        let rgba = thumbnail.to_rgba8();
        Ok(Preview::Image(ImagePreview {
            width: rgba.width(),
            height: rgba.height(),
            source_width,
            source_height,
            format: format_name(format),
            rgba: rgba.into_raw(),
        }))
    }
}

/// Turns a decoder error into a degrade reason, keeping the limit refusal
/// distinct from a genuinely broken file.
///
/// The decoder does not report how much it wanted, so the refusal carries the
/// limit that stopped it and repeats it as the observed value rather than
/// inventing a number. `actual` is a floor here, and the only case where it is.
fn decode_failure(error: &image::ImageError, limit: u64) -> DegradeReason {
    match error {
        image::ImageError::Limits(_) => DegradeReason::TooLarge {
            limit,
            actual: limit,
        },
        other => DegradeReason::DecodeFailed(other.to_string()),
    }
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::Bmp => "BMP",
        _ => "image",
    }
}
