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
//!
//! The battery is drawn as the picture the battery rule chose
//! (`hardware_core::charge_state::battery_look`, ticket 170), in pixels: a
//! battery filled to its charge, with a bolt, a plug, a caution mark or a
//! question mark over it (ticket 295). Nothing here reads the kernel's
//! status; a picture the rule did not choose is never drawn.

use hardware_core::charge_state::{BatteryIcon, Language, battery_look};

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
/// A battery that runs low, or is plugged in and not charging: the caution
/// colour of the battery pictures in `better-ui`'s icon set.
const AMBER: [u8; 4] = [255, 0xf0, 0xa5, 0x1a];

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

/// What is drawn over a battery to say what it is doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mark {
    Bolt,
    Plug,
    /// The plug of a full battery in a row of numbers, where no fill is
    /// drawn: green, so a battery the kernel calls full below its limit never
    /// reads as one held there.
    PlugFull,
    Caution,
    Question,
}

impl Mark {
    /// A 5×7 drawing, one row per byte, the five low bits left to right.
    fn rows(self) -> [u8; 7] {
        match self {
            Mark::Bolt => [
                0b00110, 0b01100, 0b11110, 0b01111, 0b00110, 0b01100, 0b01000,
            ],
            Mark::Plug | Mark::PlugFull => [
                0b01010, 0b01010, 0b11111, 0b11111, 0b01110, 0b00100, 0b00100,
            ],
            Mark::Caution => [
                0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
            ],
            Mark::Question => [
                0b01110, 0b10001, 0b00001, 0b00110, 0b00100, 0b00000, 0b00100,
            ],
        }
    }

    fn color(self) -> [u8; 4] {
        match self {
            Mark::Caution => AMBER,
            Mark::PlugFull => hue(Reading::Battery),
            Mark::Bolt | Mark::Plug | Mark::Question => INK,
        }
    }
}

const MARK_WIDTH: usize = 5;
const MARK_HEIGHT: usize = 7;

/// How the battery's picture is drawn in pixels: how full, in what colour,
/// and with which mark over it. One arm per picture the rule names
/// (`BatteryIcon`), so the taskbar draws what every battery icon draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatteryDrawing {
    pub picture: BatteryIcon,
    /// `0.0..=1.0`.
    pub fill: f64,
    pub color: [u8; 4],
    pub mark: Option<Mark>,
}

/// The battery's drawing for these readings, or `None` when the machine has
/// no battery. The picture is the battery rule's (ticket 170); with no state
/// to ask it about, the battery is the rule's empty one.
pub fn battery_drawing(readings: &TrayReadings) -> Option<BatteryDrawing> {
    let charge = readings.battery?.clamp(0.0, 1.0);
    let picture = readings
        .battery_state
        .as_ref()
        .map(|state| battery_look(state, Language::English).icon)
        .unwrap_or(BatteryIcon::Unknown);
    let (fill, color, mark) = match picture {
        BatteryIcon::Low => (charge, AMBER, None),
        BatteryIcon::Medium | BatteryIcon::Full => (charge, hue(Reading::Battery), None),
        BatteryIcon::Charging => (charge, hue(Reading::Battery), Some(Mark::Bolt)),
        BatteryIcon::Plugged => (charge, hue(Reading::Battery), Some(Mark::Plug)),
        BatteryIcon::FullPlugged => (1.0, hue(Reading::Battery), Some(Mark::Plug)),
        // "A caution mark where the charge would be": a sliver of amber.
        BatteryIcon::PluggedCaution => (0.12, AMBER, Some(Mark::Caution)),
        BatteryIcon::Unknown => (0.0, INK, Some(Mark::Question)),
    };
    Some(BatteryDrawing {
        picture,
        fill,
        color,
        mark,
    })
}

/// A battery slot this wide carries its mark at double size with a dark edge
/// all round inside the frame, which is what
/// keeps the mark readable once a panel draws the icon at 16 pixels: the
/// battery and one other reading, or the battery alone.
pub const BATTERY_SLOT_LARGE: usize = MARK_WIDTH * 2 + 4;
/// The narrowest battery a mark fits on: a frame and five pixels inside.
pub const BATTERY_SLOT_MARK: usize = MARK_WIDTH + 2;

/// Each shown reading's width in the bars, in order. The battery takes room
/// for a double-size mark while every other bar keeps four pixels, then for
/// a mark while every other bar keeps two, then what is left.
pub fn slot_widths(shown: &[Reading], room: usize, gap: usize) -> Vec<usize> {
    let count = shown.len().max(1);
    let free = room - gap * (count - 1);
    let even = (free / count).max(2);
    let Some(battery_at) = shown.iter().position(|r| *r == Reading::Battery) else {
        return vec![even; count];
    };
    let others = count - 1;
    let battery = if free >= BATTERY_SLOT_LARGE + 4 * others {
        BATTERY_SLOT_LARGE
    } else if free >= BATTERY_SLOT_MARK + 2 * others {
        BATTERY_SLOT_MARK
    } else {
        free.saturating_sub(2 * others).max(2)
    };
    // With the battery alone there is no other bar.
    let other = (free - battery)
        .checked_div(others)
        .map_or(0, |width| width.max(2));
    (0..count)
        .map(|index| if index == battery_at { battery } else { other })
        .collect()
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

    /// A mark at `scale` (1 or 2) with its top-left at `(x, y)`, with a dark
    /// edge on every pixel around it that is not the frame, so it reads over
    /// the fill and on a light panel alike.
    fn mark(&mut self, x: usize, y: usize, mark: Mark, scale: usize) {
        let rows = mark.rows();
        let (width, height) = (MARK_WIDTH * scale, MARK_HEIGHT * scale);
        let on = |column: i32, row: i32| {
            (0..width as i32).contains(&column)
                && (0..height as i32).contains(&row)
                && rows[row as usize / scale] & (0b10000 >> (column as usize / scale)) != 0
        };
        for row in -1..=height as i32 {
            for column in -1..=width as i32 {
                if on(column, row) {
                    continue;
                }
                let near = (-1..=1).any(|dy| (-1..=1).any(|dx| on(column + dx, row + dy)));
                let (px, py) = (x as i32 + column, y as i32 + row);
                if !near || px < 0 || py < 0 {
                    continue;
                }
                let (px, py) = (px as usize, py as usize);
                if px < self.width && py < HEIGHT && self.pixels[py * self.width + px] != FRAME {
                    self.set(px, py, OUTLINE);
                }
            }
        }
        for row in 0..height {
            for column in 0..width {
                if on(column as i32, row as i32) {
                    self.set(x + column, y + row, mark.color());
                }
            }
        }
    }

    /// The battery standing in its bar slot: a cap on top, a body filled to
    /// the charge, and its mark over the body when the slot is wide enough.
    fn battery_bar(
        &mut self,
        x: usize,
        width: usize,
        top: usize,
        bottom: usize,
        d: BatteryDrawing,
    ) {
        let cap = width.saturating_sub(4).max(1);
        self.rect(x + (width - cap) / 2, top, cap, 1, FRAME);
        let body = top + 1;
        self.rect(x, body, width, 1, FRAME);
        self.rect(x, bottom - 1, width, 1, FRAME);
        self.rect(x, body, 1, bottom - body, FRAME);
        self.rect(x + width - 1, body, 1, bottom - body, FRAME);
        let inside = bottom - body - 2;
        let mut fill = ((d.fill * inside as f64).round() as usize).min(inside);
        if d.fill > 0.0 {
            fill = fill.max(1);
        }
        self.rect(
            x + 1,
            bottom - 1 - fill,
            width.saturating_sub(2),
            fill,
            d.color,
        );
        if let Some(mark) = d.mark
            && width >= BATTERY_SLOT_MARK
        {
            let scale = if width >= BATTERY_SLOT_LARGE { 2 } else { 1 };
            let mx = x + 1 + (width - 2 - MARK_WIDTH * scale) / 2;
            let my = body + 1 + (inside - MARK_HEIGHT * scale) / 2;
            self.mark(mx, my, mark, scale);
        }
    }

    /// A 5×7 battery for a row of numbers, filled to the level.
    fn small_battery(&mut self, x: usize, y: usize, d: BatteryDrawing) {
        self.rect(x + 1, y, 3, 1, FRAME);
        self.rect(x, y + 1, 5, 1, FRAME);
        self.rect(x, y + 6, 5, 1, FRAME);
        self.rect(x, y + 2, 1, 4, FRAME);
        self.rect(x + 4, y + 2, 1, 4, FRAME);
        let mut rows = ((d.fill * 4.0).round() as usize).min(4);
        if d.fill > 0.0 {
            rows = rows.max(1);
        }
        self.rect(x + 1, y + 6 - rows, 3, rows, d.color);
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
            let widths = slot_widths(&shown, room, gap);
            let used = widths.iter().sum::<usize>() + gap * (count - 1);
            let mut x = 1 + (room - used.min(room)) / 2;
            let (top, bottom) = (1, HEIGHT - 1);
            for (index, bar) in widths.into_iter().enumerate() {
                let reading = shown.get(index).copied();
                if reading == Some(Reading::Battery)
                    && let Some(drawing) = battery_drawing(readings)
                {
                    canvas.battery_bar(x, bar, top, bottom, drawing);
                    x += bar + gap;
                    continue;
                }
                canvas.rect(x, top, bar, 1, FRAME);
                canvas.rect(x, bottom - 1, bar, 1, FRAME);
                canvas.rect(x, top, 1, bottom - top, FRAME);
                canvas.rect(x + bar - 1, top, 1, bottom - top, FRAME);
                if let Some(reading) = reading {
                    let inside = bottom - top - 2;
                    let fill = bar_fraction(reading, readings)
                        .map(|f| ((f * inside as f64).round() as usize).min(inside))
                        .unwrap_or(0);
                    canvas.rect(
                        x + 1,
                        bottom - 1 - fill,
                        bar.saturating_sub(2),
                        fill,
                        hue(reading),
                    );
                }
                x += bar + gap;
            }
        }
        IconStyle::Numbers => {
            for (row, reading) in shown.iter().take(MAX_NUMBER_ROWS).enumerate() {
                let value = compact_value(*reading, readings);
                let row_top = 1 + row * 11;
                if *reading == Reading::Battery
                    && let Some(drawing) = battery_drawing(readings)
                {
                    // The battery's picture, then its charge: in the large
                    // digits when both fit, in the small ones when not.
                    let characters = value.chars().count().max(1);
                    let scale = if MARK_WIDTH + 1 + (characters * 4 - 1) * 2 <= HEIGHT {
                        2
                    } else {
                        1
                    };
                    let width = MARK_WIDTH + 1 + (characters * 4 - 1) * scale;
                    let x = HEIGHT.saturating_sub(width) / 2;
                    match drawing.mark {
                        // No fill is drawn here, so a full battery's plug
                        // is green and a held one's is not.
                        Some(Mark::Plug) if drawing.picture == BatteryIcon::FullPlugged => {
                            canvas.mark(x, row_top + 1, Mark::PlugFull, 1)
                        }
                        Some(mark) => canvas.mark(x, row_top + 1, mark, 1),
                        None => canvas.small_battery(x, row_top + 1, drawing),
                    }
                    let text_top = if scale == 2 { row_top } else { row_top + 2 };
                    canvas.text(x + MARK_WIDTH + 1, text_top, &value, scale, drawing.color);
                    continue;
                }
                let width = (value.chars().count() * 4).saturating_sub(1) * 2;
                let x = HEIGHT.saturating_sub(width) / 2;
                canvas.text(x, row_top, &value, 2, hue(*reading));
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

    use hardware_core::battery::BatteryStatus;
    use hardware_core::charge_state::{ChargeLimit, ChargeReadings};

    /// The battery at `percent` in one of ticket 170's states.
    fn battery(state: &str, percent: u8) -> TrayReadings {
        let limit = |end| ChargeLimit::Percent { end, start: None };
        let charge = match state {
            "on battery" => ChargeReadings::single(
                Some(false),
                Some(BatteryStatus::Discharging),
                Some(percent),
                ChargeLimit::NotSupported,
            ),
            "charging" => ChargeReadings::single(
                Some(true),
                Some(BatteryStatus::Charging),
                Some(percent),
                limit(80),
            ),
            "holding" => ChargeReadings::single(
                Some(true),
                Some(BatteryStatus::NotCharging),
                Some(percent),
                limit(percent),
            ),
            "full" => ChargeReadings::single(
                Some(true),
                Some(BatteryStatus::Full),
                Some(percent),
                ChargeLimit::NotSupported,
            ),
            "not charging" => ChargeReadings::single(
                Some(true),
                Some(BatteryStatus::NotCharging),
                Some(percent),
                limit(100),
            ),
            "draining" => ChargeReadings::single(
                Some(true),
                Some(BatteryStatus::Discharging),
                Some(percent),
                limit(100),
            ),
            "unknown" => ChargeReadings::single(None, None, Some(percent), limit(80)),
            other => panic!("{other}"),
        };
        TrayReadings {
            cpu: Some(0.25),
            battery: Some(f64::from(percent) / 100.0),
            battery_state: Some(charge),
            ..TrayReadings::default()
        }
    }

    const STATES: [&str; 6] = [
        "on battery",
        "charging",
        "holding",
        "full",
        "not charging",
        "unknown",
    ];

    #[test]
    fn each_battery_state_draws_the_rules_picture() {
        let picture = |state, percent| battery_drawing(&battery(state, percent)).unwrap();
        assert_eq!(picture("on battery", 95).picture, BatteryIcon::Full);
        assert_eq!(picture("on battery", 60).picture, BatteryIcon::Medium);
        assert_eq!(picture("on battery", 20).picture, BatteryIcon::Low);
        assert_eq!(picture("on battery", 20).color, AMBER);
        assert_eq!(picture("on battery", 60).mark, None);
        assert_eq!(picture("charging", 60).mark, Some(Mark::Bolt));
        assert_eq!(picture("holding", 80).picture, BatteryIcon::Plugged);
        assert_eq!(picture("holding", 80).mark, Some(Mark::Plug));
        assert_eq!(picture("full", 100).picture, BatteryIcon::FullPlugged);
        assert_eq!(picture("full", 100).fill, 1.0);
        assert_eq!(picture("full", 100).mark, Some(Mark::Plug));
        assert_eq!(picture("not charging", 60).mark, Some(Mark::Caution));
        assert_eq!(picture("unknown", 60).picture, BatteryIcon::Unknown);
        assert_eq!(picture("unknown", 60).mark, Some(Mark::Question));
        // The two caution states share their picture, as every battery icon
        // draws them.
        assert_eq!(picture("draining", 60), picture("not charging", 60));
        // No battery, no drawing.
        assert_eq!(battery_drawing(&readings()), None);
    }

    #[test]
    fn no_two_battery_states_look_the_same_in_the_taskbar() {
        for style in [IconStyle::Bars, IconStyle::Numbers] {
            for shown in [
                vec![Reading::Battery],
                vec![Reading::Cpu, Reading::Battery],
                vec![Reading::Battery, Reading::Cpu],
            ] {
                let icons: Vec<Vec<Pixmap>> = STATES
                    .iter()
                    .map(|state| draw(&shown, style, &battery(state, 80)))
                    .collect();
                for a in 0..STATES.len() {
                    for b in a + 1..STATES.len() {
                        assert_ne!(
                            icons[a], icons[b],
                            "{style:?} {shown:?}: {} and {} look the same",
                            STATES[a], STATES[b]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_held_battery_never_looks_like_one_running_the_machine() {
        // The defect ticket 170 named, now in the panel too.
        for style in [IconStyle::Bars, IconStyle::Numbers] {
            let shown = [Reading::Cpu, Reading::Battery];
            assert_ne!(
                draw(&shown, style, &battery("holding", 80)),
                draw(&shown, style, &battery("on battery", 80)),
            );
        }
    }

    #[test]
    fn the_battery_keeps_room_for_its_mark_up_to_five_readings() {
        for count in 1..=7usize {
            let mut shown: Vec<Reading> = Reading::ALL
                .iter()
                .copied()
                .filter(|r| *r != Reading::Battery)
                .take(count - 1)
                .collect();
            shown.push(Reading::Battery);
            shown.sort();
            let gap = if count <= 4 { 2 } else { 1 };
            let widths = slot_widths(&shown, HEIGHT - 2, gap);
            let used = widths.iter().sum::<usize>() + gap * (count - 1);
            assert!(used <= HEIGHT - 2, "{count}: {widths:?}");
            assert!(widths.iter().all(|w| *w >= 2), "{count}: {widths:?}");
            let battery = widths[shown.iter().position(|r| *r == Reading::Battery).unwrap()];
            assert!(battery <= BATTERY_SLOT_LARGE);
            if count <= 2 {
                assert_eq!(battery, BATTERY_SLOT_LARGE, "{count}: {widths:?}");
            }
            if count <= 5 {
                assert!(battery >= BATTERY_SLOT_MARK, "{count}: {widths:?}");
            }
        }
    }

    #[test]
    fn a_mark_is_drawn_over_the_battery_with_a_dark_edge() {
        let icons = draw(
            &[Reading::Battery],
            IconStyle::Bars,
            &battery("charging", 50),
        );
        let pixels = &icons[0].2;
        let ink = pixels.chunks(4).filter(|p| *p == INK).count();
        // At double size: four pixels for each of the drawing's.
        assert_eq!(
            ink,
            4 * Mark::Bolt
                .rows()
                .iter()
                .map(|r| r.count_ones() as usize)
                .sum::<usize>()
        );
        let at = |x: usize, y: usize| &pixels[(y * 22 + x) * 4..(y * 22 + x) * 4 + 4];
        for y in 0..22 {
            for x in 0..22 {
                if at(x, y) == INK {
                    for (dx, dy) in [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)] {
                        let n = at((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                        assert!(n == INK || n == OUTLINE, "bare mark edge at {x},{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_three_digit_charge_still_has_its_mark_in_numbers() {
        let icons = draw(
            &[Reading::Battery],
            IconStyle::Numbers,
            &battery("full", 100),
        );
        let pixels = &icons[0].2;
        let green = pixels
            .chunks(4)
            .filter(|p| *p == hue(Reading::Battery))
            .count();
        let plug: usize = Mark::Plug
            .rows()
            .iter()
            .map(|r| r.count_ones() as usize)
            .sum();
        // The plug, whole, and the three figures beside it.
        assert!(green > plug, "{green}");
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
