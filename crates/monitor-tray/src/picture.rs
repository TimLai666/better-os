//! The one picture Better Monitor shows in a panel (owner, 2026-11-22,
//! ticket 295): the power mode's mark, a gap, and the readings, side by
//! side, as one macOS menu bar item is. GPUI-free and bus-free, so the
//! taskbar item and the Better OS session's own top bar (tickets 156 and
//! 157) compose the same picture.
//!
//! The picture is an SVG document, because a panel draws an SVG at the
//! display's own resolution: GNOME Shell rasterises it at the stage's scale,
//! so it is as sharp at 200 % as at 100 %, where a PNG would be scaled up.
//!
//! - The mark is the desktop's own power mode icon
//!   (`power-profile-*-symbolic`), found by the icon theme lookup every
//!   Better OS program shares (`better_core::icon_theme`). It is parsed and
//!   written out again by `usvg`, so what goes into the picture is
//!   self-contained: no styles that could reach the readings, no entities,
//!   no namespaces it does not declare. Its colours are then replaced by the
//!   panel's ink, as a symbolic icon's are.
//! - The readings are drawn natively on the picture's 16-pixel grid
//!   (`icon::draw_on_grid`), one square per cell, so no scaling drops a row.
//!   The mark adds width beside them and never takes any.

use hardware_core::power::Profile;

use crate::icon::Pixmap;

/// The gap between the mark and the readings, in logical pixels.
pub const GAP: u32 = 4;

/// The picture's height in logical pixels: the size GNOME Shell draws a
/// panel icon at (`PANEL_ICON_SIZE`), and the readings' grid
/// (`icon::GRID`). A wide picture is drawn at its own size, which the SVG
/// states in logical pixels.
pub const HEIGHT: u32 = crate::icon::GRID as u32;

/// The icon theme's name for a mode, as GNOME's own power mode menu uses.
pub fn icon_name(mode: Profile) -> &'static str {
    match mode {
        Profile::PowerSaver => "power-profile-power-saver-symbolic",
        Profile::Balanced => "power-profile-balanced-symbolic",
        Profile::Performance => "power-profile-performance-symbolic",
    }
}

/// How the panel's symbolic icons are drawn: light on a dark panel, dark on
/// a light one, and when which cannot be told, light with a dark edge, the
/// readings' own rule, which reads on both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Panel {
    Dark,
    Light,
    Unknown,
}

/// What the desktop says that decides the panel's colour, each `None` when
/// it could not be read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PanelHints {
    /// The GNOME Shell theme the User Themes extension applies
    /// (`org.gnome.shell.extensions.user-theme` `name`): Zorin's panel.
    pub shell_theme: Option<String>,
    /// The GTK theme (`org.gnome.desktop.interface` `gtk-theme`).
    pub gtk_theme: Option<String>,
    /// `org.freedesktop.appearance` `color-scheme`: 0 none, 1 dark, 2 light.
    pub color_scheme: Option<u32>,
}

/// A theme name's light or dark half, by the `-Light`/`-Dark` naming every
/// Zorin theme and Adwaita's `Adwaita-dark` follow.
fn by_name(theme: &str) -> Option<Panel> {
    let lower = theme.to_ascii_lowercase();
    if lower.ends_with("-dark") {
        Some(Panel::Dark)
    } else if lower.ends_with("-light") {
        Some(Panel::Light)
    } else {
        None
    }
}

impl Panel {
    /// The shell theme in force decides; then the GTK theme's name, which
    /// Zorin keeps in step with its shell theme; then an explicit colour
    /// scheme. Anything else is not known: Zorin's light desktop says no
    /// colour scheme at all, and GNOME 46's own panel ignores it.
    pub fn from_hints(hints: &PanelHints) -> Self {
        if let Some(panel) = hints.shell_theme.as_deref().and_then(by_name) {
            return panel;
        }
        if let Some(panel) = hints.gtk_theme.as_deref().and_then(by_name) {
            return panel;
        }
        match hints.color_scheme {
            Some(1) => Panel::Dark,
            Some(2) => Panel::Light,
            _ => Panel::Unknown,
        }
    }

    /// The ink, and the edge drawn around it when the panel is not known.
    fn ink(self) -> ([u8; 3], Option<[u8; 3]>) {
        match self {
            Panel::Dark => ([0xff, 0xff, 0xff], None),
            Panel::Light => ([0x2e, 0x34, 0x36], None),
            Panel::Unknown => ([0xe8, 0xe8, 0xe8], Some([0x1c, 0x1c, 0x1c])),
        }
    }
}

/// The mark: the icon's SVG made self-contained by `usvg` and nested `size`
/// logical pixels square at the left, its colours replaced by the panel's
/// ink and its opacity kept. `None` when the file is not an SVG that can be
/// drawn.
pub fn mark(svg: &[u8], size: u32, panel: Panel) -> Option<String> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let (width, height) = (tree.size().width(), tree.size().height());
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let written = tree.to_string(&usvg::WriteOptions {
        id_prefix: Some("mark-".into()),
        ..usvg::WriteOptions::default()
    });
    let start = written.find("<svg")?;
    let element = &written[start..];
    let scale = f64::from(size) / f64::from(width.max(height));
    let ([r, g, b], edge) = panel.ink();
    let channel = |c: u8| f64::from(c) / 255.0;
    let colour = format!(
        "0 0 0 0 {:.4} 0 0 0 0 {:.4} 0 0 0 0 {:.4} 0 0 0 1 0",
        channel(r),
        channel(g),
        channel(b)
    );
    let filter = match edge {
        None => format!(
            r#"<filter id="ink" x="0" y="0" width="1" height="1" color-interpolation-filters="sRGB"><feColorMatrix type="matrix" values="{colour}"/></filter>"#
        ),
        Some(edge) => format!(
            r##"<filter id="ink" x="-0.2" y="-0.2" width="1.4" height="1.4" color-interpolation-filters="sRGB"><feMorphology in="SourceAlpha" operator="dilate" radius="0.75" result="wide"/><feFlood flood-color="#{:02x}{:02x}{:02x}" flood-opacity="0.8"/><feComposite in2="wide" operator="in" result="edge"/><feColorMatrix in="SourceGraphic" type="matrix" values="{colour}" result="fill"/><feMerge><feMergeNode in="edge"/><feMergeNode in="fill"/></feMerge></filter>"##,
            edge[0], edge[1], edge[2]
        ),
    };
    Some(format!(
        r#"<defs>{filter}</defs><g filter="url(#ink)"><g transform="scale({scale:.6})">{element}</g></g>"#
    ))
}

/// The readings' grid as squares, one per drawn cell (a run of one colour
/// in a row is one rectangle), at `x`. `crispEdges` keeps every edge on the
/// display's pixels; at 200 % each cell is exactly two by two.
fn readings_svg(readings: &Pixmap, x: u32) -> String {
    let (width, height, argb) = readings;
    let (width, height) = ((*width).max(0) as usize, (*height).max(0) as usize);
    let at = |column: usize, row: usize| {
        let i = (row * width + column) * 4;
        [argb[i], argb[i + 1], argb[i + 2], argb[i + 3]]
    };
    let mut out = format!(r#"<g shape-rendering="crispEdges" transform="translate({x} 0)">"#);
    for row in 0..height {
        let mut column = 0;
        while column < width {
            let pixel = at(column, row);
            if pixel[0] == 0 {
                column += 1;
                continue;
            }
            let mut run = 1;
            while column + run < width && at(column + run, row) == pixel {
                run += 1;
            }
            out.push_str(&format!(
                r##"<rect x="{column}" y="{row}" width="{run}" height="1" fill="#{:02x}{:02x}{:02x}" fill-opacity="{:.4}"/>"##,
                pixel[1],
                pixel[2],
                pixel[3],
                f64::from(pixel[0]) / 255.0
            ));
            column += run;
        }
    }
    out.push_str("</g>");
    out
}

/// The one picture as an SVG document: the mark ([`mark`]) at the left,
/// [`GAP`], then the readings drawn on the grid; without a mark, the
/// readings alone.
pub fn compose(readings: &Pixmap, mark: Option<&str>) -> String {
    let mark_width = if mark.is_some() { HEIGHT + GAP } else { 0 };
    let width = mark_width + readings.0.max(0) as u32;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{HEIGHT}" viewBox="0 0 {width} {HEIGHT}">{}{}</svg>"#,
        mark.unwrap_or(""),
        readings_svg(readings, mark_width)
    )
}

#[cfg(test)]
pub(crate) mod testing {
    //! Drawing the picture as a panel does, for the tests: with resvg, and
    //! with librsvg through GdkPixbuf, which is what GNOME Shell uses.

    /// A picture as straight RGBA8 pixels.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct Image {
        pub width: u32,
        pub height: u32,
        pub rgba: Vec<u8>,
    }

    impl Image {
        pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
            let i = ((y * self.width + x) * 4) as usize;
            [
                self.rgba[i],
                self.rgba[i + 1],
                self.rgba[i + 2],
                self.rgba[i + 3],
            ]
        }

        pub fn region(&self, x: u32, y: u32, width: u32, height: u32) -> Vec<[u8; 4]> {
            let mut out = Vec::new();
            for row in y..y + height {
                for column in x..x + width {
                    out.push(self.at(column, row));
                }
            }
            out
        }

        fn from_pixmap(pixmap: &resvg::tiny_skia::Pixmap) -> Image {
            let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
            for pixel in pixmap.pixels() {
                let c = pixel.demultiply();
                rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
            }
            Image {
                width: pixmap.width(),
                height: pixmap.height(),
                rgba,
            }
        }
    }

    /// The document drawn with resvg at `scale` device pixels per logical
    /// pixel.
    pub fn render(svg: &str, scale: f32) -> Option<Image> {
        let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).ok()?;
        let size = tree.size();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(
            (size.width() * scale).round() as u32,
            (size.height() * scale).round() as u32,
        )?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        Some(Image::from_pixmap(&pixmap))
    }

    /// The document drawn by librsvg through GdkPixbuf
    /// (`gdk-pixbuf-thumbnailer`), as GNOME Shell's texture cache loads it,
    /// `size` pixels on its longer side. `Err` with GdkPixbuf's words when it
    /// cannot be drawn; `None` when the tool is missing.
    pub fn render_with_librsvg(svg: &str, size: u32) -> Option<Result<Image, String>> {
        let dir = tempfile::tempdir().ok()?;
        let input = dir.path().join("picture.svg");
        let output = dir.path().join("picture.png");
        std::fs::write(&input, svg).ok()?;
        let run = std::process::Command::new("gdk-pixbuf-thumbnailer")
            .args(["-s", &size.to_string()])
            .arg(&input)
            .arg(&output)
            .output()
            .ok()?;
        if !run.status.success() {
            return Some(Err(String::from_utf8_lossy(&run.stderr).into_owned()));
        }
        let pixmap = resvg::tiny_skia::Pixmap::load_png(&output).ok()?;
        Some(Ok(Image::from_pixmap(&pixmap)))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{render, render_with_librsvg};
    use super::*;
    use crate::icon::{self, IconStyle};
    use crate::menu::{Reading, TrayReadings};

    const CIRCLE: &str = r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="16px" height="16px" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" fill="#2e3436"/></svg>"##;

    fn readings() -> TrayReadings {
        TrayReadings {
            cpu: Some(0.25),
            memory_used: Some(1.0),
            memory_total: Some(2.0),
            ..TrayReadings::default()
        }
    }

    fn grid(style: IconStyle) -> Pixmap {
        icon::draw_on_grid(
            &[Reading::Cpu, Reading::Memory],
            style,
            &readings(),
            icon::GRID,
        )
    }

    /// `gdk-pixbuf-thumbnailer` with GdkPixbuf's SVG loader, librsvg.
    macro_rules! librsvg_or_skip {
        ($svg:expr, $size:expr) => {
            test_prerequisites::require!(
                "gdk-pixbuf-thumbnailer and librsvg2-common",
                render_with_librsvg($svg, $size).ok_or("gdk-pixbuf-thumbnailer is missing")
            )
        };
    }

    #[test]
    fn the_readings_are_the_same_pixels_with_and_without_the_mark_at_each_scale() {
        for style in [IconStyle::Bars, IconStyle::Numbers] {
            let readings = grid(style);
            let mark = mark(CIRCLE.as_bytes(), HEIGHT, Panel::Dark).unwrap();
            for scale in [1.0f32, 1.25, 1.5, 2.0] {
                let alone = render(&compose(&readings, None), scale).unwrap();
                let together = render(&compose(&readings, Some(&mark)), scale).unwrap();
                let offset = ((HEIGHT + GAP) as f32 * scale).round() as u32;
                assert_eq!(together.width, alone.width + offset, "{scale}");
                assert_eq!(
                    together.region(offset, 0, alone.width, alone.height),
                    alone.region(0, 0, alone.width, alone.height),
                    "{style:?} {scale}"
                );
            }
        }
    }

    #[test]
    fn every_cell_of_the_readings_keeps_its_colour_at_each_scale() {
        // Drawn on the 16 grid, no cell is dropped at any scale: each one
        // still has a display pixel of its own colour.
        for style in [IconStyle::Bars, IconStyle::Numbers] {
            let readings = grid(style);
            let (w, h, argb) = &readings;
            let svg = compose(&readings, None);
            for scale in [1.0f32, 1.25, 1.5, 2.0] {
                let image = render(&svg, scale).unwrap();
                for row in 0..*h as usize {
                    for column in 0..*w as usize {
                        let i = (row * *w as usize + column) * 4;
                        if argb[i] != 255 {
                            continue;
                        }
                        let want = [argb[i + 1], argb[i + 2], argb[i + 3], 255];
                        let (x0, x1) = (
                            (column as f32 * scale).floor() as u32,
                            ((column + 1) as f32 * scale).ceil() as u32,
                        );
                        let (y0, y1) = (
                            (row as f32 * scale).floor() as u32,
                            ((row + 1) as f32 * scale).ceil() as u32,
                        );
                        let found = (y0..y1.min(image.height))
                            .any(|y| (x0..x1.min(image.width)).any(|x| image.at(x, y) == want));
                        assert!(found, "{style:?} at {scale}: cell {column},{row} vanished");
                    }
                }
            }
        }
    }

    #[test]
    fn the_mark_is_the_icon_in_the_panels_ink() {
        for (panel, ink) in [
            (Panel::Dark, [0xff, 0xff, 0xff]),
            (Panel::Light, [0x2e, 0x34, 0x36]),
            (Panel::Unknown, [0xe8, 0xe8, 0xe8]),
        ] {
            let mark = mark(CIRCLE.as_bytes(), HEIGHT, panel).unwrap();
            let image = render(&compose(&grid(IconStyle::Bars), Some(&mark)), 2.0).unwrap();
            let centre = image.at(16, 16);
            for (got, want) in centre[..3].iter().zip(ink) {
                assert!(got.abs_diff(want) <= 1, "{panel:?}: {centre:?}");
            }
            assert_eq!(centre[3], 255);
            // Only the unknown panel gets a dark edge around the shape.
            let edge = image.at(16, 3);
            assert_eq!(edge[3] > 0, panel == Panel::Unknown, "{panel:?}: {edge:?}");
        }
        assert_eq!(mark(b"not an svg", HEIGHT, Panel::Dark), None);
    }

    #[test]
    fn the_panel_follows_the_shell_theme_then_the_gtk_theme_then_the_scheme() {
        let hints = |shell: Option<&str>, gtk: Option<&str>, scheme: Option<u32>| PanelHints {
            shell_theme: shell.map(String::from),
            gtk_theme: gtk.map(String::from),
            color_scheme: scheme,
        };
        // Zorin's light desktop: no colour scheme, a light theme.
        assert_eq!(
            Panel::from_hints(&hints(None, Some("ZorinBlue-Light"), Some(0))),
            Panel::Light
        );
        assert_eq!(
            Panel::from_hints(&hints(
                Some("ZorinBlue-Dark"),
                Some("ZorinBlue-Light"),
                Some(2)
            )),
            Panel::Dark
        );
        assert_eq!(
            Panel::from_hints(&hints(None, Some("Adwaita"), Some(2))),
            Panel::Light
        );
        assert_eq!(
            Panel::from_hints(&hints(None, Some("Adwaita-dark"), None)),
            Panel::Dark
        );
        assert_eq!(
            Panel::from_hints(&hints(None, Some("Yaru"), Some(0))),
            Panel::Unknown
        );
        assert_eq!(Panel::from_hints(&PanelHints::default()), Panel::Unknown);
    }

    /// Icons as themes ship them, which pasting a file's text into the
    /// picture used to break: Inkscape's namespaces, an entity, a comment
    /// holding `<svg`, and a style that would have reached the readings.
    const AWKWARD: [&str; 4] = [
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" xmlns:xlink="http://www.w3.org/1999/xlink" width="16" height="16"><sodipodi:namedview inkscape:zoom="1"/><defs><circle id="c" cx="8" cy="8" r="6"/></defs><use xlink:href="#c" fill="#2e3436"/></svg>"##,
        r##"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY c "#2e3436">]><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="6" fill="&c;"/></svg>"##,
        r##"<!-- an icon: <svg> --><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="6"/></svg>"##,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><style>rect{fill:#ff0000 !important} circle{fill:#2e3436}</style><circle cx="8" cy="8" r="6"/></svg>"##,
    ];

    #[test]
    fn a_themes_icon_however_it_is_written_composes_a_picture_librsvg_draws() {
        let readings = grid(IconStyle::Bars);
        // At 200 %: the readings alone are 16 logical pixels, the picture 36.
        let alone = librsvg_or_skip!(&compose(&readings, None), 32).expect("the readings draw");
        for icon in AWKWARD {
            let mark = mark(icon.as_bytes(), HEIGHT, Panel::Dark).expect(icon);
            let svg = compose(&readings, Some(&mark));
            let drawn = librsvg_or_skip!(&svg, 72).unwrap_or_else(|error| panic!("{error}: {svg}"));
            assert_eq!((drawn.width, drawn.height), (72, 32));
            // The mark is drawn, white on a dark panel.
            assert_eq!(drawn.at(16, 16), [255, 255, 255, 255], "{icon}");
            // The readings are untouched by the icon's style.
            assert_eq!(
                drawn.region(40, 0, alone.width, alone.height),
                alone.region(0, 0, alone.width, alone.height),
                "{icon}"
            );
        }
    }

    #[test]
    fn the_picture_is_wide_enough_for_a_panel_to_draw_it_at_its_own_size() {
        let mark = mark(CIRCLE.as_bytes(), HEIGHT, Panel::Dark).unwrap();
        let svg = compose(&grid(IconStyle::Bars), Some(&mark));
        let image = render(&svg, 1.0).unwrap();
        // GNOME's AppIndicator extension draws a file at its own size when
        // it is at least half again as wide as tall.
        assert!(image.width * 2 >= image.height * 3);
        assert_eq!(image.height, HEIGHT);
    }
}
