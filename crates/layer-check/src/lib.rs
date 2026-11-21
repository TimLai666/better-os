//! The GPUI-free half of ticket 200's check: the check program's command
//! line, the probe's key table, and the pixel work behind its screenshots.
//! The programs are `src/main.rs` (`better-layer-check`, GPUI) and
//! `src/bin/probe.rs` (`better-wl-probe`, plain Wayland). Neither is
//! packaged; `packaging/tests/cosmic-nested.sh` builds and runs them.

use better_ui::layer::{Edges, ExclusiveZone, Extent, Keyboard, Layer, Margin, SurfaceSpec};

/// What `better-layer-check` was asked to show.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckArgs {
    pub spec: SurfaceSpec,
    /// One surface per output rather than one.
    pub every_output: bool,
    /// Only this many logical pixels from the left take input.
    pub input_left: Option<f32>,
    /// After this many seconds, change the exclusive zone (and the height,
    /// for a strip) to this.
    pub zone_after: Option<(f32, f32)>,
    /// With `zone_after`, leave the size alone: only the zone changes, in
    /// place.
    pub zone_only: bool,
    /// After this many seconds, move the surface to these anchors.
    pub anchor_after: Option<(f32, Edges)>,
    /// A click opens a popup parented to the surface.
    pub popup: bool,
    /// An ordinary window instead of a layer surface, for what the surfaces
    /// do to windows.
    pub toplevel: bool,
    pub maximized: bool,
    pub label: String,
    pub color: u32,
}

/// The usage text.
pub const HELP: &str = "\
Usage: better-layer-check [OPTIONS]

Opens a GPUI window as a zwlr_layer_shell_v1 surface and prints what happens
to it (configure sizes, scale, focus, clicks, keys) one line per event.

  --layer background|bottom|top|overlay   (top)
  --anchor EDGES        top,bottom,left,right in any combination, none, all
                        (top,left,right)
  --width N|fill        logical pixels, or fill (the protocol's 0)
  --height N|fill
  --zone none|ignore|N  exclusive zone: none (0), ignore (-1), or N pixels
  --edge EDGE           exclusive edge, for a corner
  --margin T,R,B,L
  --keyboard none|on-demand|exclusive     (none)
  --namespace NAME      (better-layer-check)
  --every-output        one surface per output, following outputs
  --input-left N        only the leftmost N pixels take input
  --zone-after S:N      after S seconds set the zone (and a strip's height) to N
  --zone-only           with --zone-after, change only the zone, not the size
  --anchor-after S:EDGES  after S seconds move to EDGES (a new surface)
  --popup               a click opens a popup parented to the surface
  --toplevel            an ordinary window instead, printing its size
  --maximized           with --toplevel, ask to be maximized
  --label TEXT          drawn on the surface
  --color RRGGBB        its fill (2f6fdf)";

/// Parses `better-layer-check`'s arguments (without the program name).
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<CheckArgs, String> {
    let mut spec = SurfaceSpec::strip("better-layer-check", Edges::TOP, 32.0);
    let mut width = None;
    let mut height = None;
    let mut zone = None;
    let mut parsed = CheckArgs {
        spec: spec.clone(),
        every_output: false,
        input_left: None,
        zone_after: None,
        zone_only: false,
        anchor_after: None,
        popup: false,
        toplevel: false,
        maximized: false,
        label: String::new(),
        color: 0x2f6fdf,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} needs a value; see --help"))
        };
        match arg.as_str() {
            "--layer" => spec.layer = parse_layer(&value("--layer")?)?,
            "--anchor" => spec.anchor = parse_edges(&value("--anchor")?)?,
            "--width" => width = Some(parse_extent(&value("--width")?)?),
            "--height" => height = Some(parse_extent(&value("--height")?)?),
            "--zone" => zone = Some(parse_zone(&value("--zone")?)?),
            "--edge" => spec.exclusive_edge = Some(parse_edges(&value("--edge")?)?),
            "--margin" => spec.margin = parse_margin(&value("--margin")?)?,
            "--keyboard" => spec.keyboard = parse_keyboard(&value("--keyboard")?)?,
            "--namespace" => spec.namespace = value("--namespace")?,
            "--every-output" => parsed.every_output = true,
            "--input-left" => parsed.input_left = Some(parse_number(&value("--input-left")?)?),
            "--zone-after" => {
                let text = value("--zone-after")?;
                let (seconds, pixels) = text
                    .split_once(':')
                    .ok_or_else(|| format!("--zone-after takes S:N, not {text}"))?;
                parsed.zone_after = Some((parse_number(seconds)?, parse_number(pixels)?));
            }
            "--anchor-after" => {
                let text = value("--anchor-after")?;
                let (seconds, edges) = text
                    .split_once(':')
                    .ok_or_else(|| format!("--anchor-after takes S:EDGES, not {text}"))?;
                parsed.anchor_after = Some((parse_number(seconds)?, parse_edges(edges)?));
            }
            "--zone-only" => parsed.zone_only = true,
            "--popup" => parsed.popup = true,
            "--toplevel" => parsed.toplevel = true,
            "--maximized" => parsed.maximized = true,
            "--label" => parsed.label = value("--label")?,
            "--color" => {
                let text = value("--color")?;
                parsed.color = u32::from_str_radix(text.trim_start_matches('#'), 16)
                    .ok()
                    .filter(|color| *color <= 0xff_ffff)
                    .ok_or_else(|| format!("--color takes RRGGBB, not {text}"))?;
            }
            other => return Err(format!("unknown argument {other}; see --help")),
        }
    }
    // A size not given follows the anchor: fill along an axis anchored on
    // both sides, the strip's 32 pixels across, 200 by 120 otherwise.
    spec.width = width.unwrap_or(if spec.anchor.spans_width() {
        Extent::Fill
    } else {
        Extent::Logical(if spec.anchor.spans_height() {
            32.0
        } else {
            200.0
        })
    });
    spec.height = height.unwrap_or(if spec.anchor.spans_height() {
        Extent::Fill
    } else {
        Extent::Logical(if spec.anchor.spans_width() {
            32.0
        } else {
            120.0
        })
    });
    // Only a strip (an edge plus both sides along it) reserves space unless
    // asked to.
    spec.exclusive_zone = zone.unwrap_or(match (spec.reserving_edge(), spec.width, spec.height) {
        (Some(edge), _, Extent::Logical(pixels))
            if (edge == Edges::TOP || edge == Edges::BOTTOM) && spec.anchor.spans_width() =>
        {
            ExclusiveZone::Reserve(pixels)
        }
        (Some(edge), Extent::Logical(pixels), _)
            if (edge == Edges::LEFT || edge == Edges::RIGHT) && spec.anchor.spans_height() =>
        {
            ExclusiveZone::Reserve(pixels)
        }
        _ => ExclusiveZone::None,
    });
    if parsed.label.is_empty() {
        parsed.label = format!(
            "{} {} {}",
            layer_name(spec.layer),
            edges_name(spec.anchor),
            keyboard_name(spec.keyboard)
        );
    }
    if !parsed.toplevel {
        spec.validate().map_err(|error| format!("{error:?}"))?;
    }
    parsed.spec = spec;
    Ok(parsed)
}

fn parse_number(text: &str) -> Result<f32, String> {
    text.trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| format!("{text} is not a number of pixels or seconds"))
}

/// `top,left`, `none`, `all`.
pub fn parse_edges(text: &str) -> Result<Edges, String> {
    let mut edges = Edges::NONE;
    for part in text
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        edges = edges
            | match part {
                "top" => Edges::TOP,
                "bottom" => Edges::BOTTOM,
                "left" => Edges::LEFT,
                "right" => Edges::RIGHT,
                "all" => Edges::ALL,
                "none" => Edges::NONE,
                other => return Err(format!("{other} is not an edge")),
            };
    }
    Ok(edges)
}

/// The edges' names, `top,left`, or `none`.
pub fn edges_name(edges: Edges) -> String {
    let names: Vec<&str> = [
        (Edges::TOP, "top"),
        (Edges::BOTTOM, "bottom"),
        (Edges::LEFT, "left"),
        (Edges::RIGHT, "right"),
    ]
    .into_iter()
    .filter(|(edge, _)| edges.contains(*edge))
    .map(|(_, name)| name)
    .collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(",")
    }
}

fn parse_layer(text: &str) -> Result<Layer, String> {
    match text {
        "background" => Ok(Layer::Background),
        "bottom" => Ok(Layer::Bottom),
        "top" => Ok(Layer::Top),
        "overlay" => Ok(Layer::Overlay),
        other => Err(format!("{other} is not a layer")),
    }
}

/// The layer's name as the command line spells it.
pub fn layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::Background => "background",
        Layer::Bottom => "bottom",
        Layer::Top => "top",
        Layer::Overlay => "overlay",
    }
}

fn parse_keyboard(text: &str) -> Result<Keyboard, String> {
    match text {
        "none" => Ok(Keyboard::None),
        "on-demand" => Ok(Keyboard::OnDemand),
        "exclusive" => Ok(Keyboard::Exclusive),
        other => Err(format!("{other} is not a keyboard mode")),
    }
}

/// The keyboard mode's name as the command line spells it.
pub fn keyboard_name(keyboard: Keyboard) -> &'static str {
    match keyboard {
        Keyboard::None => "none",
        Keyboard::OnDemand => "on-demand",
        Keyboard::Exclusive => "exclusive",
    }
}

fn parse_extent(text: &str) -> Result<Extent, String> {
    if text == "fill" {
        Ok(Extent::Fill)
    } else {
        parse_number(text).map(Extent::Logical)
    }
}

fn parse_zone(text: &str) -> Result<ExclusiveZone, String> {
    match text {
        "none" => Ok(ExclusiveZone::None),
        "ignore" => Ok(ExclusiveZone::IgnoreOthers),
        other => parse_number(other).map(ExclusiveZone::Reserve),
    }
}

fn parse_margin(text: &str) -> Result<Margin, String> {
    let values: Vec<f32> = text
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<f32>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("{part} is not a margin"))
        })
        .collect::<Result<_, _>>()?;
    match values.as_slice() {
        [top, right, bottom, left] => Ok(Margin {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => Err(format!("--margin takes T,R,B,L, not {text}")),
    }
}

/// The probe's typing: text to evdev key codes.
pub mod keys {
    /// One key press and release, with Shift held when `shift`.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Stroke {
        /// The evdev code (`KEY_A` is 30); the xkb keycode is 8 more.
        pub code: u32,
        pub shift: bool,
    }

    /// `KEY_LEFTSHIFT`.
    pub const LEFT_SHIFT: u32 = 42;

    const LETTERS: [u32; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    const DIGITS: [u32; 10] = [11, 2, 3, 4, 5, 6, 7, 8, 9, 10];

    fn named(name: &str) -> Option<u32> {
        Some(match name {
            "Return" | "Enter" => 28,
            "Escape" => 1,
            "Tab" => 15,
            "BackSpace" => 14,
            "space" => 57,
            "Up" => 103,
            "Down" => 108,
            "Left" => 105,
            "Right" => 106,
            "Super" => 125,
            _ => return None,
        })
    }

    /// The strokes that type `text` on a US layout. `{Name}` is a named key
    /// (`{Return}`, `{Escape}`, `{Tab}`, `{BackSpace}`, `{Up}`, `{Down}`,
    /// `{Left}`, `{Right}`, `{Super}`, `{space}`).
    pub fn plan(text: &str) -> Result<Vec<Stroke>, String> {
        let mut strokes = Vec::new();
        let mut chars = text.chars();
        while let Some(character) = chars.next() {
            let stroke = match character {
                '{' => {
                    let name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                    let code = named(&name).ok_or_else(|| format!("{{{name}}} is not a key"))?;
                    Stroke { code, shift: false }
                }
                'a'..='z' => Stroke {
                    code: LETTERS[(character as u8 - b'a') as usize],
                    shift: false,
                },
                'A'..='Z' => Stroke {
                    code: LETTERS[(character as u8 - b'A') as usize],
                    shift: true,
                },
                '0'..='9' => Stroke {
                    code: DIGITS[(character as u8 - b'0') as usize],
                    shift: false,
                },
                ' ' => Stroke {
                    code: 57,
                    shift: false,
                },
                '-' => Stroke {
                    code: 12,
                    shift: false,
                },
                '.' => Stroke {
                    code: 52,
                    shift: false,
                },
                other => return Err(format!("{other:?} has no key in the probe's table")),
            };
            strokes.push(stroke);
        }
        Ok(strokes)
    }
}

/// Pixels from a screen copy.
pub mod pixels {
    /// The `wl_shm` formats a compositor offers for a screen copy, as their
    /// protocol values.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Format {
        /// `argb8888` (0) and `xrgb8888` (1): B, G, R, A bytes in memory.
        Bgra,
        /// `abgr8888` and `xbgr8888`: R, G, B, A bytes in memory.
        Rgba,
    }

    impl Format {
        /// The layout of a `wl_shm` format code, when it is one of the four.
        pub fn from_shm(code: u32) -> Option<Format> {
            match code {
                0 | 1 => Some(Format::Bgra),
                0x3432_4241 | 0x3432_4258 => Some(Format::Rgba),
                _ => None,
            }
        }
    }

    /// Opaque RGBA rows from a copied buffer, flipped when the compositor
    /// said the copy is upside down.
    pub fn to_rgba(
        format: Format,
        data: &[u8],
        width: usize,
        height: usize,
        stride: usize,
        y_invert: bool,
    ) -> Option<Vec<u8>> {
        if stride < width * 4 || data.len() < stride * height {
            return None;
        }
        let mut out = Vec::with_capacity(width * height * 4);
        for row in 0..height {
            let source = if y_invert { height - 1 - row } else { row };
            let line = &data[source * stride..source * stride + width * 4];
            for pixel in line.as_chunks::<4>().0 {
                let (red, green, blue) = match format {
                    Format::Bgra => (pixel[2], pixel[1], pixel[0]),
                    Format::Rgba => (pixel[0], pixel[1], pixel[2]),
                };
                out.extend_from_slice(&[red, green, blue, 255]);
            }
        }
        Some(out)
    }

    /// The smallest rectangle holding every pixel within `tolerance` of
    /// `color` in each channel, as `(x, y, width, height)` in image pixels.
    pub fn bounding_box(
        rgba: &[u8],
        width: usize,
        color: [u8; 3],
        tolerance: u8,
    ) -> Option<(usize, usize, usize, usize)> {
        let close = |a: u8, b: u8| a.abs_diff(b) <= tolerance;
        let mut found: Option<(usize, usize, usize, usize)> = None;
        for (index, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
            if close(pixel[0], color[0]) && close(pixel[1], color[1]) && close(pixel[2], color[2]) {
                let (x, y) = (index % width, index / width);
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((left, top, right, bottom)) => {
                        (left.min(x), top.min(y), right.max(x), bottom.max(y))
                    }
                });
            }
        }
        found.map(|(left, top, right, bottom)| (left, top, right - left + 1, bottom - top + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<CheckArgs, String> {
        parse_args(list.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn the_default_is_a_top_strip_reserving_its_height() {
        let parsed = args(&[]).unwrap();
        assert_eq!(
            parsed.spec,
            SurfaceSpec::strip("better-layer-check", Edges::TOP, 32.0)
        );
        assert_eq!(parsed.label, "top top,left,right none");
    }

    #[test]
    fn a_corner_gets_a_fixed_size_and_reserves_nothing() {
        let parsed = args(&["--anchor", "bottom,right", "--layer", "overlay"]).unwrap();
        assert_eq!(parsed.spec.width, Extent::Logical(200.0));
        assert_eq!(parsed.spec.height, Extent::Logical(120.0));
        assert_eq!(parsed.spec.exclusive_zone, ExclusiveZone::None);
        assert_eq!(parsed.spec.layer, Layer::Overlay);
    }

    #[test]
    fn every_anchor_combination_parses_into_a_valid_spec() {
        for bits in 0..16u8 {
            let edges = Edges::from_bits(bits);
            let name = edges_name(edges);
            let parsed =
                args(&["--anchor", &name]).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(parsed.spec.anchor, edges, "{name}");
            assert_eq!(parse_edges(&name).unwrap(), edges);
        }
    }

    #[test]
    fn a_side_strip_reserves_its_width() {
        let parsed = args(&["--anchor", "left,top,bottom", "--width", "64"]).unwrap();
        assert_eq!(parsed.spec.height, Extent::Fill);
        assert_eq!(parsed.spec.exclusive_zone, ExclusiveZone::Reserve(64.0));
    }

    #[test]
    fn values_are_checked() {
        assert!(args(&["--layer", "middle"]).is_err());
        assert!(args(&["--anchor", "up"]).is_err());
        assert!(args(&["--width"]).is_err());
        assert!(args(&["--zone-after", "3"]).is_err());
        assert!(args(&["--margin", "1,2,3"]).is_err());
        assert!(args(&["--color", "red"]).is_err());
        assert!(args(&["--bogus"]).is_err());
        assert!(
            args(&["--anchor", "top", "--width", "fill"]).is_err(),
            "fill without both sides is refused before the compositor sees it"
        );
        let parsed = args(&[
            "--zone-after",
            "2.5:64",
            "--anchor-after",
            "3:bottom,left,right",
            "--zone",
            "ignore",
            "--keyboard",
            "exclusive",
            "--margin",
            "8,0,0,0",
        ])
        .unwrap();
        assert_eq!(parsed.zone_after, Some((2.5, 64.0)));
        assert_eq!(
            parsed.anchor_after,
            Some((3.0, Edges::BOTTOM | Edges::LEFT | Edges::RIGHT))
        );
        assert_eq!(parsed.spec.exclusive_zone, ExclusiveZone::IgnoreOthers);
        assert_eq!(parsed.spec.keyboard, Keyboard::Exclusive);
        assert_eq!(parsed.spec.margin.top, 8.0);
    }

    #[test]
    fn text_becomes_us_layout_strokes() {
        use keys::{Stroke, plan};
        assert_eq!(
            plan("aZ1 {Return}").unwrap(),
            vec![
                Stroke {
                    code: 30,
                    shift: false
                },
                Stroke {
                    code: 44,
                    shift: true
                },
                Stroke {
                    code: 2,
                    shift: false
                },
                Stroke {
                    code: 57,
                    shift: false
                },
                Stroke {
                    code: 28,
                    shift: false
                },
            ]
        );
        assert!(plan("{Nope}").is_err());
        assert!(plan("é").is_err());
    }

    #[test]
    fn a_copy_becomes_rgba_and_flips_when_inverted() {
        use pixels::{Format, to_rgba};
        // Two rows of one pixel each, stride 8 (padded): blue then red, BGRA.
        let data = [255, 0, 0, 0, 9, 9, 9, 9, 0, 0, 255, 0, 9, 9, 9, 9];
        assert_eq!(
            to_rgba(Format::Bgra, &data, 1, 2, 8, false).unwrap(),
            vec![0, 0, 255, 255, 255, 0, 0, 255]
        );
        assert_eq!(
            to_rgba(Format::Bgra, &data, 1, 2, 8, true).unwrap(),
            vec![255, 0, 0, 255, 0, 0, 255, 255]
        );
        assert_eq!(
            to_rgba(Format::Rgba, &data[..4], 1, 1, 4, false).unwrap(),
            vec![255, 0, 0, 255]
        );
        assert!(to_rgba(Format::Bgra, &data, 3, 2, 8, false).is_none());
        assert_eq!(Format::from_shm(1), Some(Format::Bgra));
        assert_eq!(Format::from_shm(0x3432_4258), Some(Format::Rgba));
        assert_eq!(Format::from_shm(7), None);
    }

    #[test]
    fn the_bounding_box_finds_a_colour() {
        use pixels::bounding_box;
        // 3 by 2, the target colour at (1,0) and (2,1).
        let mut rgba = vec![0u8; 3 * 2 * 4];
        for index in [1usize, 5] {
            rgba[index * 4..index * 4 + 4].copy_from_slice(&[47, 111, 223, 255]);
        }
        assert_eq!(
            bounding_box(&rgba, 3, [47, 111, 223], 2),
            Some((1, 0, 2, 2))
        );
        assert_eq!(bounding_box(&rgba, 3, [200, 0, 0], 2), None);
        // A trailing partial pixel is ignored, as chunks_exact did.
        rgba.extend_from_slice(&[47, 111, 223]);
        assert_eq!(
            bounding_box(&rgba, 3, [47, 111, 223], 2),
            Some((1, 0, 2, 2))
        );
    }
}
