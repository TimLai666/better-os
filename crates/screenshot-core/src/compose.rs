//! Pictures in memory, and making one picture from the frozen outputs (spec
//! O2, S2, C10, C14, R1).
//!
//! A [`Frame`] is what the compositor copied: rows of 4-byte pixels in
//! `wl_shm`'s `argb8888` or `xrgb8888` order, which in memory on a little
//! endian machine is blue, green, red, alpha. GPUI's images take exactly that
//! order, and PNG takes red, green, blue, alpha, so [`Frame::to_rgba`] swaps.
//!
//! An area is in logical pixels, and the picture is made at the highest scale
//! of the outputs it covers, as GNOME's stage picture is: every output's part
//! is scaled to that scale (nearest pixel), and where no output is the
//! picture is transparent.

use crate::geometry::Rect;

/// Whether the fourth byte of each pixel is meaningful.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alpha {
    /// `argb8888`: the alpha byte is the pixel's.
    Straight,
    /// `xrgb8888`: the fourth byte is undefined and the pixel is opaque.
    Ignored,
}

/// A copied picture, blue-green-red-alpha in memory.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Bytes from one row to the next.
    pub stride: u32,
    pub alpha: Alpha,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .field("alpha", &self.alpha)
            .finish_non_exhaustive()
    }
}

impl Frame {
    /// A frame of one colour, given as blue, green, red, alpha.
    pub fn filled(width: u32, height: u32, bgra: [u8; 4]) -> Frame {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            data.extend_from_slice(&bgra);
        }
        Frame {
            width,
            height,
            stride: width * 4,
            alpha: Alpha::Straight,
            data,
        }
    }

    /// The pixel at a place, blue-green-red-alpha, alpha forced opaque for
    /// [`Alpha::Ignored`].
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = (y * self.stride + x * 4) as usize;
        let mut pixel = [
            self.data[at],
            self.data[at + 1],
            self.data[at + 2],
            self.data[at + 3],
        ];
        if self.alpha == Alpha::Ignored {
            pixel[3] = 255;
        }
        pixel
    }

    /// The same pixels, packed tightly with opaque alpha where the format
    /// said so: what GPUI's images take.
    pub fn to_bgra(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height {
            let row = (y * self.stride) as usize;
            let row = &self.data[row..row + (self.width * 4) as usize];
            match self.alpha {
                Alpha::Straight => out.extend_from_slice(row),
                Alpha::Ignored => {
                    for pixel in row.as_chunks::<4>().0 {
                        out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                    }
                }
            }
        }
        out
    }

    /// Red-green-blue-alpha, packed: what a PNG holds.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = self.to_bgra();
        for pixel in out.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        out
    }

    /// A part of the frame, in its own pixels, clipped to it.
    pub fn crop(&self, rect: Rect) -> Option<Frame> {
        let bounds = Rect::new(0, 0, self.width as i32, self.height as i32);
        let part = bounds.intersection(&rect)?;
        let mut data = Vec::with_capacity((part.width * part.height * 4) as usize);
        for y in part.y..part.bottom() {
            let start = (y as u32 * self.stride + part.x as u32 * 4) as usize;
            data.extend_from_slice(&self.data[start..start + (part.width * 4) as usize]);
        }
        Some(Frame {
            width: part.width as u32,
            height: part.height as u32,
            stride: part.width as u32 * 4,
            alpha: self.alpha,
            data,
        })
    }
}

/// How a copied buffer is turned relative to the screen
/// (`ext_image_copy_capture_frame_v1.transform`, `wl_output.transform`'s
/// values: rotations counter-clockwise, the flipped ones mirrored around the
/// vertical axis first).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Transform {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Transform {
    /// From the protocol's number; an unknown one is taken as normal.
    pub fn from_protocol(value: u32) -> Transform {
        match value {
            1 => Transform::Rotate90,
            2 => Transform::Rotate180,
            3 => Transform::Rotate270,
            4 => Transform::Flipped,
            5 => Transform::Flipped90,
            6 => Transform::Flipped180,
            7 => Transform::Flipped270,
            _ => Transform::Normal,
        }
    }

    fn swaps_sides(self) -> bool {
        matches!(
            self,
            Transform::Rotate90
                | Transform::Rotate270
                | Transform::Flipped90
                | Transform::Flipped270
        )
    }
}

impl Frame {
    /// The picture as the screen shows it: the buffer with its transform
    /// applied. Seen on `cosmic-comp`'s nested output, which is
    /// `Flipped180` and copies upside down until this turns it.
    pub fn upright(&self, transform: Transform) -> Frame {
        if transform == Transform::Normal {
            return self.clone();
        }
        let (w, h) = (self.width, self.height);
        let (width, height) = if transform.swaps_sides() {
            (h, w)
        } else {
            (w, h)
        };
        let mut data = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let (ox, oy) = match transform {
                    Transform::Normal => (x, y),
                    Transform::Rotate90 => (w - 1 - y, x),
                    Transform::Rotate180 => (w - 1 - x, h - 1 - y),
                    Transform::Rotate270 => (y, h - 1 - x),
                    Transform::Flipped => (w - 1 - x, y),
                    Transform::Flipped90 => (y, x),
                    Transform::Flipped180 => (x, h - 1 - y),
                    Transform::Flipped270 => (w - 1 - y, h - 1 - x),
                };
                let from = (oy * self.stride + ox * 4) as usize;
                let to = ((y * width + x) * 4) as usize;
                data[to..to + 4].copy_from_slice(&self.data[from..from + 4]);
            }
        }
        Frame {
            width,
            height,
            stride: width * 4,
            alpha: self.alpha,
            data,
        }
    }
}

/// One output's frozen picture and where it is.
#[derive(Clone, Copy, Debug)]
pub struct Placed<'a> {
    /// The output in logical pixels.
    pub logical: Rect,
    pub frame: &'a Frame,
}

impl Placed<'_> {
    /// Device pixels per logical pixel, from the picture's own size: 1.5 for
    /// a 1920-wide picture of a 1280-wide output.
    pub fn scale(&self) -> f64 {
        if self.logical.width <= 0 {
            1.0
        } else {
            f64::from(self.frame.width) / f64::from(self.logical.width)
        }
    }
}

/// The scale a picture of `area` is made at: the highest of the outputs it
/// covers, 1 when it covers none.
pub fn scale_for(area: Rect, outputs: &[Placed<'_>]) -> f64 {
    outputs
        .iter()
        .filter(|output| output.logical.intersection(&area).is_some())
        .map(Placed::scale)
        .fold(None, |best: Option<f64>, scale| {
            Some(best.map_or(scale, |best| best.max(scale)))
        })
        .unwrap_or(1.0)
}

/// The picture of `area` (logical pixels) from the outputs' frozen pictures,
/// at [`scale_for`]'s scale. `None` for an empty area.
pub fn compose(area: Rect, outputs: &[Placed<'_>]) -> Option<Frame> {
    if area.is_empty() {
        return None;
    }
    let scale = scale_for(area, outputs);
    let width = (f64::from(area.width) * scale).round().max(1.0) as u32;
    let height = (f64::from(area.height) * scale).round().max(1.0) as u32;
    let mut out = Frame {
        width,
        height,
        stride: width * 4,
        alpha: Alpha::Straight,
        data: vec![0; (width * height * 4) as usize],
    };

    for output in outputs {
        let Some(part) = output.logical.intersection(&area) else {
            continue;
        };
        let source_scale = output.scale();
        // The part in the picture's pixels.
        let x0 = (f64::from(part.x - area.x) * scale).round() as u32;
        let y0 = (f64::from(part.y - area.y) * scale).round() as u32;
        let x1 = ((f64::from(part.right() - area.x) * scale).round() as u32).min(width);
        let y1 = ((f64::from(part.bottom() - area.y) * scale).round() as u32).min(height);
        for y in y0..y1 {
            // The logical place of this row's centre, then the source pixel.
            let logical_y = f64::from(area.y) + (f64::from(y) + 0.5) / scale;
            let sy = ((logical_y - f64::from(output.logical.y)) * source_scale).floor();
            let sy = (sy.max(0.0) as u32).min(output.frame.height.saturating_sub(1));
            let row_out = (y * out.stride) as usize;
            if (source_scale - scale).abs() < f64::EPSILON {
                // Same scale: one copy per row.
                let sx = ((f64::from(x0) / scale + f64::from(area.x - output.logical.x))
                    * source_scale)
                    .round()
                    .max(0.0) as u32;
                let count = (x1 - x0).min(output.frame.width.saturating_sub(sx));
                let from = (sy * output.frame.stride + sx * 4) as usize;
                let to = row_out + (x0 * 4) as usize;
                out.data[to..to + (count * 4) as usize]
                    .copy_from_slice(&output.frame.data[from..from + (count * 4) as usize]);
                if output.frame.alpha == Alpha::Ignored {
                    for pixel in out.data[to..to + (count * 4) as usize]
                        .as_chunks_mut::<4>()
                        .0
                    {
                        pixel[3] = 255;
                    }
                }
                continue;
            }
            for x in x0..x1 {
                let logical_x = f64::from(area.x) + (f64::from(x) + 0.5) / scale;
                let sx = ((logical_x - f64::from(output.logical.x)) * source_scale).floor();
                let sx = (sx.max(0.0) as u32).min(output.frame.width.saturating_sub(1));
                let pixel = output.frame.pixel(sx, sy);
                let at = row_out + (x * 4) as usize;
                out.data[at..at + 4].copy_from_slice(&pixel);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{Alpha, Frame, Placed, compose, scale_for};
    use crate::geometry::Rect;

    const RED: [u8; 4] = [0, 0, 255, 255];
    const BLUE: [u8; 4] = [255, 0, 0, 255];

    #[test]
    fn png_order_swaps_red_and_blue_and_xrgb_is_opaque() {
        let mut frame = Frame::filled(1, 1, [1, 2, 3, 0]);
        assert_eq!(frame.to_rgba(), vec![3, 2, 1, 0]);
        frame.alpha = Alpha::Ignored;
        assert_eq!(frame.to_rgba(), vec![3, 2, 1, 255]);
        assert_eq!(frame.to_bgra(), vec![1, 2, 3, 255]);
    }

    #[test]
    fn a_stride_wider_than_the_row_is_skipped() {
        let frame = Frame {
            width: 1,
            height: 2,
            stride: 8,
            alpha: Alpha::Straight,
            data: vec![1, 1, 1, 1, 9, 9, 9, 9, 2, 2, 2, 2, 9, 9, 9, 9],
        };
        assert_eq!(frame.to_bgra(), vec![1, 1, 1, 1, 2, 2, 2, 2]);
    }

    /// A 3 × 2 frame whose pixels are numbered 0..6 in their first byte.
    fn numbered() -> Frame {
        let mut data = Vec::new();
        for n in 0..6u8 {
            data.extend_from_slice(&[n, 0, 0, 255]);
        }
        Frame {
            width: 3,
            height: 2,
            stride: 12,
            alpha: Alpha::Straight,
            data,
        }
    }

    fn numbers(frame: &Frame) -> (u32, u32, Vec<u8>) {
        (
            frame.width,
            frame.height,
            frame.data.as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
        )
    }

    #[test]
    fn every_transform_turns_the_buffer_upright() {
        use super::Transform;
        // 0 1 2
        // 3 4 5
        let frame = numbered();
        let cases = [
            (Transform::Normal, (3, 2, vec![0, 1, 2, 3, 4, 5])),
            (Transform::Rotate90, (2, 3, vec![2, 5, 1, 4, 0, 3])),
            (Transform::Rotate180, (3, 2, vec![5, 4, 3, 2, 1, 0])),
            (Transform::Rotate270, (2, 3, vec![3, 0, 4, 1, 5, 2])),
            (Transform::Flipped, (3, 2, vec![2, 1, 0, 5, 4, 3])),
            (Transform::Flipped90, (2, 3, vec![0, 3, 1, 4, 2, 5])),
            (Transform::Flipped180, (3, 2, vec![3, 4, 5, 0, 1, 2])),
            (Transform::Flipped270, (2, 3, vec![5, 2, 4, 1, 3, 0])),
        ];
        for (transform, want) in cases {
            assert_eq!(numbers(&frame.upright(transform)), want, "{transform:?}");
        }
        assert_eq!(Transform::from_protocol(6), Transform::Flipped180);
        assert_eq!(Transform::from_protocol(99), Transform::Normal);
    }

    #[test]
    fn crop_takes_a_part_and_clips_it() {
        let mut frame = Frame::filled(4, 4, RED);
        frame.data[(4 + 2) * 4..(4 + 2) * 4 + 4].copy_from_slice(&BLUE);
        let part = frame.crop(Rect::new(2, 1, 10, 10)).unwrap();
        assert_eq!((part.width, part.height), (2, 3));
        assert_eq!(part.pixel(0, 0), BLUE);
        assert_eq!(frame.crop(Rect::new(10, 10, 2, 2)), None);
    }

    #[test]
    fn an_area_on_one_output_is_its_pixels() {
        let red = Frame::filled(1280, 800, RED);
        let outputs = [Placed {
            logical: Rect::new(0, 0, 1280, 800),
            frame: &red,
        }];
        let picture = compose(Rect::new(10, 10, 100, 50), &outputs).unwrap();
        assert_eq!((picture.width, picture.height), (100, 50));
        assert_eq!(picture.pixel(0, 0), RED);
        assert_eq!(picture.pixel(99, 49), RED);
    }

    #[test]
    fn an_area_at_150_percent_has_the_outputs_own_pixels() {
        let red = Frame::filled(1920, 1200, RED);
        let outputs = [Placed {
            logical: Rect::new(0, 0, 1280, 800),
            frame: &red,
        }];
        assert_eq!(scale_for(Rect::new(0, 0, 10, 10), &outputs), 1.5);
        let picture = compose(Rect::new(0, 0, 1280, 800), &outputs).unwrap();
        assert_eq!((picture.width, picture.height), (1920, 1200));
    }

    #[test]
    fn an_area_across_two_outputs_takes_each_part_at_the_higher_scale() {
        let red = Frame::filled(100, 100, RED); // 100 × 100 at 1
        let blue = Frame::filled(200, 200, BLUE); // 100 × 100 at 2
        let outputs = [
            Placed {
                logical: Rect::new(0, 0, 100, 100),
                frame: &red,
            },
            Placed {
                logical: Rect::new(100, 0, 100, 100),
                frame: &blue,
            },
        ];
        let picture = compose(Rect::new(50, 0, 100, 50), &outputs).unwrap();
        assert_eq!((picture.width, picture.height), (200, 100));
        assert_eq!(picture.pixel(0, 0), RED);
        assert_eq!(picture.pixel(99, 99), RED);
        assert_eq!(picture.pixel(100, 0), BLUE);
        assert_eq!(picture.pixel(199, 99), BLUE);
    }

    #[test]
    fn where_no_output_is_the_picture_is_transparent() {
        let red = Frame::filled(100, 100, RED);
        let outputs = [Placed {
            logical: Rect::new(0, 0, 100, 100),
            frame: &red,
        }];
        let picture = compose(Rect::new(50, 50, 100, 100), &outputs).unwrap();
        assert_eq!(picture.pixel(0, 0), RED);
        assert_eq!(picture.pixel(99, 99), [0, 0, 0, 0]);
        assert_eq!(compose(Rect::new(0, 0, 0, 5), &outputs), None);
    }
}
