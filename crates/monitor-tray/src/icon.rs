//! The taskbar icon, drawn from the readings.
//!
//! A StatusNotifierItem may carry its icon as pixels (`IconPixmap`, an array
//! of `(width, height, ARGB32 bytes in network order)`), and that is what lets
//! the icon show live numbers the way Stats does in the macOS menu bar: each
//! reading the person picked is a small bar, or a short label over its value.
//!
//! The font is a 3×5 bitmap of only the characters the icon needs, so the
//! icon looks the same everywhere and needs no font on the machine. Two sizes
//! are offered, 22 and 44 pixels tall; the panel picks the one that fits.

use crate::menu::{Reading, TrayReadings};

/// What the icon shows for each reading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IconStyle {
    /// A small vertical bar, filled to the reading's share.
    Bars,
    /// A short label over the value.
    Numbers,
}

/// One pixmap: width, height, and ARGB32 pixels in network byte order.
pub type Pixmap = (i32, i32, Vec<u8>);

const HEIGHT: usize = 22;

/// The foreground: light, because GNOME's top bar and most panels are dark.
const INK: [u8; 4] = [255, 0xe8, 0xe8, 0xe8];
const FRAME: [u8; 4] = [170, 0xe8, 0xe8, 0xe8];
/// A dark edge drawn around everything, so light ink still reads on a light
/// panel and the edge disappears into a dark one.
const OUTLINE: [u8; 4] = [200, 0x1c, 0x1c, 0x1c];

fn hue(reading: Reading) -> [u8; 4] {
    match reading {
        Reading::Cpu => [255, 0x4c, 0xb4, 0xf5],
        Reading::Memory => [255, 0xc2, 0x7c, 0xe6],
        Reading::Gpu => [255, 0x4f, 0xcf, 0xd6],
        Reading::Disk => [255, 0x7f, 0xcf, 0x4a],
        Reading::Network => [255, 0xf0, 0xa3, 0x5a],
        Reading::Battery => [255, 0x7f, 0xcf, 0x4a],
        Reading::Temperature => [255, 0xf2, 0x78, 0x6b],
    }
}

/// A 3×5 glyph, one row per byte, the three low bits left to right.
fn glyph(character: char) -> [u8; 5] {
    match character {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        '°' => [0b010, 0b101, 0b010, 0b000, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        _ => [0; 5],
    }
}

/// A small canvas of ARGB pixels.
struct Canvas {
    width: usize,
    pixels: Vec<[u8; 4]>,
}

impl Canvas {
    fn new(width: usize) -> Self {
        Self {
            width,
            pixels: vec![[0; 4]; width * HEIGHT],
        }
    }

    fn set(&mut self, x: usize, y: usize, color: [u8; 4]) {
        if x < self.width && y < HEIGHT {
            self.pixels[y * self.width + x] = color;
        }
    }

    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: [u8; 4]) {
        for dy in 0..h {
            for dx in 0..w {
                self.set(x + dx, y + dy, color);
            }
        }
    }

    /// Text at `scale` (1 or 2), left-aligned at `x`, top at `y`.
    fn text(&mut self, x: usize, y: usize, text: &str, scale: usize, color: [u8; 4]) {
        let mut cursor = x;
        for character in text.chars() {
            let rows = glyph(character);
            for (row, bits) in rows.iter().enumerate() {
                for column in 0..3 {
                    if bits & (0b100 >> column) != 0 {
                        self.rect(
                            cursor + column * scale,
                            y + row * scale,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
            cursor += 4 * scale;
        }
    }

    /// Put a one-pixel dark edge on every empty pixel that touches a drawn
    /// one.
    fn outline(&mut self) {
        let drawn: Vec<bool> = self.pixels.iter().map(|p| p[0] != 0).collect();
        let height = self.pixels.len() / self.width;
        for y in 0..height {
            for x in 0..self.width {
                if drawn[y * self.width + x] {
                    continue;
                }
                let touches = (-1i32..=1).any(|dy| {
                    (-1i32..=1).any(|dx| {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        nx >= 0
                            && ny >= 0
                            && (nx as usize) < self.width
                            && (ny as usize) < height
                            && drawn[ny as usize * self.width + nx as usize]
                    })
                });
                if touches {
                    self.pixels[y * self.width + x] = OUTLINE;
                }
            }
        }
    }

    fn pixmap(&self, scale: usize) -> Pixmap {
        let width = self.width * scale;
        let height = HEIGHT * scale;
        let mut bytes = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                bytes.extend_from_slice(&self.pixels[(y / scale) * self.width + x / scale]);
            }
        }
        (width as i32, height as i32, bytes)
    }
}

/// A rate in four characters or fewer: `950`, `12K`, `1.2M`.
fn short_rate(bytes_per_second: f64) -> String {
    let units = [
        ("", 1.0),
        ("K", 1024.0),
        ("M", 1024.0 * 1024.0),
        ("G", 1024.0 * 1024.0 * 1024.0),
    ];
    let mut chosen = units[0];
    for unit in units {
        if bytes_per_second >= unit.1 {
            chosen = unit;
        }
    }
    let value = bytes_per_second / chosen.1;
    if value < 10.0 && !chosen.0.is_empty() {
        format!("{value:.1}{}", chosen.0)
    } else {
        format!("{:.0}{}", value.min(999.0), chosen.0)
    }
}

/// The value a reading shows in Numbers style.
pub fn short_value(reading: Reading, readings: &TrayReadings) -> String {
    let percent = |value: Option<f64>| match value {
        Some(fraction) => format!("{:.0}%", (fraction * 100.0).clamp(0.0, 100.0)),
        None => "-".to_string(),
    };
    match reading {
        Reading::Cpu => percent(readings.cpu),
        Reading::Memory => percent(readings.memory_fraction()),
        Reading::Gpu => percent(readings.gpu),
        Reading::Battery => percent(readings.battery),
        Reading::Disk => readings
            .disk_total()
            .map(short_rate)
            .unwrap_or_else(|| "-".into()),
        Reading::Network => readings
            .network_total()
            .map(short_rate)
            .unwrap_or_else(|| "-".into()),
        Reading::Temperature => readings
            .hottest
            .map(|t| format!("{t:.0}°"))
            .unwrap_or_else(|| "-".into()),
    }
}

/// A reading as a share of its bar: percentages as they are, rates on a
/// logarithmic scale up to 100 MB/s, temperatures out of 100 °C.
pub fn bar_fraction(reading: Reading, readings: &TrayReadings) -> Option<f64> {
    let log = |rate: f64| ((rate.max(1.0)).log10() / 8.0).clamp(0.0, 1.0);
    match reading {
        Reading::Cpu => readings.cpu,
        Reading::Memory => readings.memory_fraction(),
        Reading::Gpu => readings.gpu,
        Reading::Battery => readings.battery,
        Reading::Disk => readings.disk_total().map(log),
        Reading::Network => readings.network_total().map(log),
        Reading::Temperature => readings.hottest.map(|t| (t / 100.0).clamp(0.0, 1.0)),
    }
}

/// How many readings Numbers style fits: two rows of double-size digits in
/// a square icon, so they stay readable when the panel scales the icon down.
pub const MAX_NUMBER_ROWS: usize = 2;

/// A value in three characters at most, for a row of Numbers style: the
/// colour says which reading it is, so `%` and units are left out (`29`,
/// `100`, `14K`, `1M`, `73`).
pub fn compact_value(reading: Reading, readings: &TrayReadings) -> String {
    let full = short_value(reading, readings);
    let trimmed = full.trim_end_matches(['%', '°']).to_string();
    if trimmed.chars().count() <= 3 {
        return trimmed;
    }
    // `1.4M` becomes `1M`.
    match trimmed.split_once('.') {
        Some((whole, rest)) => {
            let unit: String = rest.chars().filter(|c| c.is_ascii_alphabetic()).collect();
            format!("{whole}{unit}")
        }
        None => trimmed.chars().take(3).collect(),
    }
}

/// Draw the icon for the chosen readings. Readings this machine does not
/// have are left out; with nothing left, an empty frame is drawn so the item
/// never disappears.
///
/// The icon is square. Hosts disagree about anything else: swaybar ignores a
/// pixmap that is not square, and GNOME's AppIndicator extension fits every
/// icon into a square slot. So bars share the width, and numbers are up to
/// two rows of values in each reading's colour.
pub fn draw(shown: &[Reading], style: IconStyle, readings: &TrayReadings) -> Vec<Pixmap> {
    let shown: Vec<Reading> = shown
        .iter()
        .copied()
        .filter(|reading| readings.has(*reading))
        .collect();
    let mut canvas = Canvas::new(HEIGHT);
    match style {
        IconStyle::Bars => {
            let count = shown.len().max(1);
            let gap = if count <= 4 { 2 } else { 1 };
            // A pixel is kept free on each side for the dark edge.
            let room = HEIGHT - 2;
            let bar = ((room - gap * (count - 1)) / count).max(2);
            let used = bar * count + gap * (count - 1);
            let left = 1 + (room - used.min(room)) / 2;
            let (top, bottom) = (1, HEIGHT - 1);
            for index in 0..count {
                let x = left + index * (bar + gap);
                canvas.rect(x, top, bar, 1, FRAME);
                canvas.rect(x, bottom - 1, bar, 1, FRAME);
                canvas.rect(x, top, 1, bottom - top, FRAME);
                canvas.rect(x + bar - 1, top, 1, bottom - top, FRAME);
                let Some(reading) = shown.get(index) else {
                    continue;
                };
                let inside = bottom - top - 2;
                let fill = bar_fraction(*reading, readings)
                    .map(|f| ((f * inside as f64).round() as usize).min(inside))
                    .unwrap_or(0);
                canvas.rect(
                    x + 1,
                    bottom - 1 - fill,
                    bar.saturating_sub(2),
                    fill,
                    hue(*reading),
                );
            }
        }
        IconStyle::Numbers => {
            for (row, reading) in shown.iter().take(MAX_NUMBER_ROWS).enumerate() {
                let value = compact_value(*reading, readings);
                let width = (value.chars().count() * 4).saturating_sub(1) * 2;
                let x = HEIGHT.saturating_sub(width) / 2;
                canvas.text(x, 1 + row * 11, &value, 2, hue(*reading));
            }
            if shown.is_empty() {
                canvas.text(8, 6, "-", 2, INK);
            }
        }
    }
    canvas.outline();
    vec![canvas.pixmap(1), canvas.pixmap(2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readings() -> TrayReadings {
        TrayReadings {
            cpu: Some(0.25),
            memory_used: Some(10.0),
            memory_total: Some(40.0),
            disk_read: Some(1_500_000.0),
            disk_write: Some(0.0),
            ..TrayReadings::default()
        }
    }

    #[test]
    fn the_icon_is_square_so_every_host_draws_it() {
        for style in [IconStyle::Bars, IconStyle::Numbers] {
            for shown in [&Reading::ALL[..1], &Reading::ALL[..2], &Reading::ALL[..]] {
                let icons = draw(shown, style, &readings());
                assert_eq!((icons[0].0, icons[0].1), (22, 22));
                assert_eq!((icons[1].0, icons[1].1), (44, 44));
                assert_eq!(icons[0].2.len(), 22 * 22 * 4);
            }
        }
    }

    #[test]
    fn bars_are_filled_to_the_reading() {
        let icons = draw(
            &[Reading::Cpu, Reading::Memory],
            IconStyle::Bars,
            &readings(),
        );
        let small = &icons[0];
        // Two 10-pixel bars with a 2-pixel gap; the processor bar's inside
        // is 18 pixels tall and a quarter of it (5 rounded) is blue.
        let blue = (0..22usize)
            .filter(|y| {
                let at = (y * 22 + 3) * 4;
                small.2[at..at + 4] == [255, 0x4c, 0xb4, 0xf5]
            })
            .count();
        assert_eq!(blue, 5);
        let purple = (0..22usize)
            .filter(|y| {
                let at = (y * 22 + 15) * 4;
                small.2[at..at + 4] == [255, 0xc2, 0x7c, 0xe6]
            })
            .count();
        assert_eq!(purple, 5);
    }

    #[test]
    fn every_drawn_shape_has_a_dark_edge_for_light_panels() {
        for style in [IconStyle::Bars, IconStyle::Numbers] {
            let icons = draw(&[Reading::Cpu, Reading::Memory], style, &readings());
            let pixels = &icons[0].2;
            assert!(
                pixels.chunks(4).any(|p| p == OUTLINE),
                "{style:?} has no outline"
            );
            // Light ink is never left touching a fully empty pixel.
            let at = |x: usize, y: usize| &pixels[(y * 22 + x) * 4..(y * 22 + x) * 4 + 4];
            for y in 1..21 {
                for x in 1..21 {
                    if at(x, y)[0] == 255 {
                        for (dx, dy) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
                            let n = at((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                            assert_ne!(n[0], 0, "{style:?} bare edge at {x},{y}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn compact_values_fit_three_characters() {
        let mut r = readings();
        assert_eq!(compact_value(Reading::Cpu, &r), "25");
        r.cpu = Some(1.0);
        assert_eq!(compact_value(Reading::Cpu, &r), "100");
        assert_eq!(compact_value(Reading::Disk, &r), "1M");
        r.hottest = Some(73.2);
        assert_eq!(compact_value(Reading::Temperature, &r), "73");
        assert_eq!(compact_value(Reading::Gpu, &r), "-");
    }

    #[test]
    fn numbers_show_a_tag_and_the_value_per_row() {
        assert_eq!(short_value(Reading::Cpu, &readings()), "25%");
        assert_eq!(short_value(Reading::Memory, &readings()), "25%");
        assert_eq!(short_value(Reading::Disk, &readings()), "1.4M");
        assert_eq!(short_value(Reading::Gpu, &readings()), "-");
        let icons = draw(&[Reading::Cpu], IconStyle::Numbers, &readings());
        // "25" is drawn in the processor's blue.
        assert!(icons[0].2.chunks(4).any(|p| p == [255, 0x4c, 0xb4, 0xf5]));
    }

    #[test]
    fn a_reading_the_machine_lacks_is_left_out_and_the_icon_never_vanishes() {
        let icons = draw(&[Reading::Battery], IconStyle::Bars, &readings());
        assert!(icons[0].2.chunks(4).any(|pixel| pixel[0] > 0));
    }

    #[test]
    fn rates_fit_in_four_characters() {
        assert_eq!(short_rate(950.0), "950");
        assert_eq!(short_rate(12_500.0), "12K");
        assert_eq!(short_rate(1_300_000.0), "1.2M");
        assert_eq!(short_rate(5e12), "999G");
    }

    #[test]
    fn every_character_the_icon_writes_has_a_glyph() {
        for text in ["0123456789%.°-KMG"] {
            for character in text.chars() {
                assert_ne!(glyph(character), [0; 5], "{character}");
            }
        }
    }
}
