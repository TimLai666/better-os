//! The one picture Better Monitor shows in a panel (owner, 2026-11-22,
//! ticket 295): the power mode's mark, a gap, and the readings, side by
//! side, as one macOS menu bar item is. GPUI-free and bus-free, so the
//! taskbar item and the Better OS session's own top bar (tickets 156 and
//! 157) compose the same picture.
//!
//! The picture is an SVG document, because a panel draws an SVG at the
//! display's own resolution: GNOME Shell rasterises it at the stage's scale,
//! so it is as sharp at 200 % as at 100 %, where a PNG would be scaled up.
//! The mark is the desktop's own power mode icon (`power-profile-*-symbolic`),
//! found in the current icon theme by the icon theme rules and nested in the
//! document as it is, recoloured to the panel's foreground as a symbolic
//! icon is. The readings are their canvas's pixels as squares, edges kept
//! crisp, scaled to the picture's height; the mark adds width beside them
//! and never takes any.

use std::path::{Path, PathBuf};

use hardware_core::power::Profile;

/// A picture as pixels: width, height, and straight (not premultiplied)
/// RGBA8, row by row. The readings' canvas arrives as one, and the tests
/// draw the SVG into one to compare.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn empty(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        }
    }

    /// From one StatusNotifierItem pixmap: ARGB32 in network byte order.
    pub fn from_argb(width: i32, height: i32, argb: &[u8]) -> Self {
        let mut rgba = Vec::with_capacity(argb.len());
        for pixel in argb.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[pixel[1], pixel[2], pixel[3], pixel[0]]);
        }
        Self {
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            rgba,
        }
    }

    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    /// The pixels of a rectangle, for comparing one part of two pictures.
    pub fn region(&self, x: u32, y: u32, width: u32, height: u32) -> Vec<[u8; 4]> {
        let mut out = Vec::new();
        for row in y..y + height {
            for column in x..x + width {
                out.push(self.at(column, row));
            }
        }
        out
    }
}

/// The gap between the mark and the readings, in logical pixels.
pub const GAP: u32 = 4;

/// The picture's height in logical pixels: the size GNOME Shell draws a
/// panel icon at (`PANEL_ICON_SIZE`). A wide picture is drawn at its own
/// size, which the SVG states in logical pixels.
pub const HEIGHT: u32 = 16;

/// The panel's foreground for a symbolic icon: light on a dark panel, dark
/// on a light one, as GNOME's Adwaita draws symbolic icons in each.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Panel {
    Dark,
    Light,
}

impl Panel {
    /// From the desktop's colour scheme (`org.freedesktop.appearance`
    /// `color-scheme`: 0 no preference, 1 dark, 2 light). With no
    /// preference GNOME Shell's own panel is dark.
    pub fn from_color_scheme(scheme: Option<u32>) -> Self {
        match scheme {
            Some(2) => Panel::Light,
            _ => Panel::Dark,
        }
    }

    pub fn ink(self) -> [u8; 3] {
        match self {
            Panel::Dark => [0xff, 0xff, 0xff],
            Panel::Light => [0x2e, 0x34, 0x36],
        }
    }
}

/// The icon theme's name for a mode, as GNOME's own power mode menu uses.
pub fn icon_name(mode: Profile) -> &'static str {
    match mode {
        Profile::PowerSaver => "power-profile-power-saver-symbolic",
        Profile::Balanced => "power-profile-balanced-symbolic",
        Profile::Performance => "power-profile-performance-symbolic",
    }
}

/// Where icon themes live: `$XDG_DATA_HOME/icons`, then each of
/// `$XDG_DATA_DIRS`' `icons` (freedesktop Icon Theme Specification,
/// "Directory Layout"). `~/.icons` comes first when there is a home.
pub fn theme_dirs(home: Option<&Path>, data_home: Option<&Path>, data_dirs: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".icons"));
    }
    match data_home {
        Some(data_home) => dirs.push(data_home.join("icons")),
        None => {
            if let Some(home) = home {
                dirs.push(home.join(".local/share/icons"));
            }
        }
    }
    let data_dirs = if data_dirs.trim().is_empty() {
        "/usr/local/share:/usr/share"
    } else {
        data_dirs
    };
    for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push(Path::new(dir).join("icons"));
    }
    dirs
}

/// One theme's `index.theme`: what it inherits and its directories, each
/// with its nominal size and type.
struct ThemeIndex {
    inherits: Vec<String>,
    directories: Vec<(String, u32, String)>,
}

fn read_index(dirs: &[PathBuf], theme: &str) -> Option<ThemeIndex> {
    let text = dirs
        .iter()
        .find_map(|dir| std::fs::read_to_string(dir.join(theme).join("index.theme")).ok())?;
    let mut section = String::new();
    let mut inherits = Vec::new();
    let mut listed = Vec::new();
    let mut details: std::collections::HashMap<String, (u32, String)> = Default::default();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if section == "Icon Theme" {
            match key {
                "Inherits" => inherits = value.split(',').map(|s| s.trim().to_string()).collect(),
                "Directories" | "ScaledDirectories" => listed.extend(
                    value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                ),
                _ => {}
            }
        } else {
            let entry = details
                .entry(section.clone())
                .or_insert((0, "Threshold".into()));
            match key {
                "Size" => entry.0 = value.parse().unwrap_or(0),
                "Type" => entry.1 = value.to_string(),
                _ => {}
            }
        }
    }
    let directories = listed
        .into_iter()
        .map(|dir| {
            let (size, kind) = details
                .get(&dir)
                .cloned()
                .unwrap_or((0, "Threshold".into()));
            (dir, size, kind)
        })
        .collect();
    Some(ThemeIndex {
        inherits,
        directories,
    })
}

/// The SVG file for `name` in `theme` or a theme it inherits, then
/// `hicolor`, as the Icon Theme Specification looks icons up. Only SVG
/// files: the mark is drawn at whatever size the picture needs. Among a
/// theme's directories a scalable one wins, then the nearest in size.
pub fn find_icon(dirs: &[PathBuf], theme: &str, name: &str, size: u32) -> Option<PathBuf> {
    let mut queue = vec![theme.to_string()];
    let mut seen = Vec::new();
    while let Some(current) = queue.first().cloned() {
        queue.remove(0);
        if seen.contains(&current) {
            continue;
        }
        seen.push(current.clone());
        if let Some(index) = read_index(dirs, &current) {
            let mut best: Option<(u32, PathBuf)> = None;
            for (directory, nominal, kind) in &index.directories {
                for base in dirs {
                    let file = base
                        .join(&current)
                        .join(directory)
                        .join(format!("{name}.svg"));
                    if !file.is_file() {
                        continue;
                    }
                    // A scalable file draws at any size, so it wins over a
                    // fixed one even of the same nominal size.
                    let distance = if kind == "Scalable" {
                        0
                    } else {
                        nominal.abs_diff(size) + 1
                    };
                    if best.as_ref().is_none_or(|(d, _)| distance < *d) {
                        best = Some((distance, file));
                    }
                }
            }
            if let Some((_, file)) = best {
                return Some(file);
            }
            queue.extend(index.inherits);
        }
        if queue.is_empty() && !seen.iter().any(|t| t == "hicolor") {
            queue.push("hicolor".into());
        }
    }
    None
}

/// The mark: the icon's SVG document nested as an element `size` logical
/// pixels square at `x`, its own content untouched, with its colours
/// replaced by `ink` and its opacity kept, as a symbolic icon is recoloured.
/// `None` when the file is not an SVG that can be drawn.
pub fn mark(svg: &[u8], x: u32, size: u32, ink: [u8; 3]) -> Option<String> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let text = std::str::from_utf8(svg).ok()?;
    let open = text.find("<svg")?;
    let close = open + text[open..].find('>')?;
    let tag = &text[open..close];
    let end = text.rfind("</svg>")?;
    if end <= close {
        return None;
    }
    let inner = &text[close + 1..end];
    let view_box = attribute(tag, "viewBox").unwrap_or_else(|| {
        let size = tree.size();
        format!("0 0 {} {}", size.width(), size.height())
    });
    let [r, g, b] = ink.map(|c| f64::from(c) / 255.0);
    Some(format!(
        r#"<filter id="ink" x="0" y="0" width="1" height="1" color-interpolation-filters="sRGB"><feColorMatrix type="matrix" values="0 0 0 0 {r:.4} 0 0 0 0 {g:.4} 0 0 0 0 {b:.4} 0 0 0 1 0"/></filter><g filter="url(#ink)"><svg x="{x}" y="0" width="{size}" height="{size}" viewBox="{view_box}">{inner}</svg></g>"#
    ))
}

/// The value of one attribute in an element's opening tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}="))? + name.len() + 2;
    let quote = tag[at..].chars().next()?;
    let rest = &tag[at + 1..];
    Some(rest[..rest.find(quote)?].to_string())
}

/// The readings' canvas as squares, one per drawn pixel, at `x`, scaled so
/// the canvas is `HEIGHT` tall. `crispEdges` keeps every square's edges on
/// the display's pixels at any scale.
fn readings_svg(readings: &Image, x: u32) -> String {
    let scale = f64::from(HEIGHT) / f64::from(readings.height.max(1));
    let mut out = format!(
        r#"<g shape-rendering="crispEdges" transform="translate({x} 0) scale({scale:.6})">"#
    );
    for row in 0..readings.height {
        let mut column = 0;
        while column < readings.width {
            let pixel = readings.at(column, row);
            if pixel[3] == 0 {
                column += 1;
                continue;
            }
            // A run of the same colour is one rectangle.
            let mut run = 1;
            while column + run < readings.width && readings.at(column + run, row) == pixel {
                run += 1;
            }
            out.push_str(&format!(
                r##"<rect x="{column}" y="{row}" width="{run}" height="1" fill="#{:02x}{:02x}{:02x}" fill-opacity="{:.4}"/>"##,
                pixel[0],
                pixel[1],
                pixel[2],
                f64::from(pixel[3]) / 255.0
            ));
            column += run;
        }
    }
    out.push_str("</g>");
    out
}

/// The readings' width in the picture, in logical pixels.
pub fn readings_width(readings: &Image) -> u32 {
    (readings.width * HEIGHT).div_ceil(readings.height.max(1))
}

/// The one picture as an SVG document: the mark ([`mark`], drawn at x 0),
/// [`GAP`], then the readings; `mark_width` is 0 without a mark.
pub fn compose(readings: &Image, mark: Option<&str>) -> String {
    let mark_width = if mark.is_some() { HEIGHT + GAP } else { 0 };
    let width = mark_width + readings_width(readings);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{HEIGHT}" viewBox="0 0 {width} {HEIGHT}">{}{}</svg>"#,
        mark.unwrap_or(""),
        readings_svg(readings, mark_width)
    )
}

/// The document drawn at `scale` device pixels per logical pixel, as a
/// panel draws it.
pub fn render(svg: &str, scale: f32) -> Option<Image> {
    let tree =
        resvg::usvg::Tree::from_data(svg.as_bytes(), &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let (width, height) = (
        (size.width() * scale).round() as u32,
        (size.height() * scale).round() as u32,
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut image = Image::empty(width, height);
    for (i, pixel) in pixmap.pixels().iter().enumerate() {
        let c = pixel.demultiply();
        image.rgba[i * 4..i * 4 + 4].copy_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CIRCLE: &str = r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="16px" height="16px" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" fill="#2e3436"/></svg>"##;

    fn readings() -> Image {
        let mut image = Image::empty(22, 22);
        for y in 4..18 {
            for x in 3..10 {
                let i = ((y * 22 + x) * 4) as usize;
                image.rgba[i..i + 4].copy_from_slice(&[0x4c, 0xb4, 0xf5, 255]);
            }
        }
        image
    }

    #[test]
    fn the_readings_are_the_same_pixels_with_and_without_the_mark_at_each_scale() {
        let readings = readings();
        let width = readings_width(&readings);
        assert_eq!(width, 16);
        let mark = mark(CIRCLE.as_bytes(), 0, HEIGHT, Panel::Dark.ink()).unwrap();
        for scale in [1.0f32, 1.25, 1.5, 2.0] {
            let alone = render(&compose(&readings, None), scale).unwrap();
            let together = render(&compose(&readings, Some(&mark)), scale).unwrap();
            let offset = ((HEIGHT + GAP) as f32 * scale).round() as u32;
            assert_eq!(together.width, alone.width + offset, "{scale}");
            assert_eq!(
                together.region(offset, 0, alone.width, alone.height),
                alone.region(0, 0, alone.width, alone.height),
                "{scale}"
            );
        }
    }

    #[test]
    fn the_readings_edges_stay_on_the_displays_pixels() {
        // A bar's edge is never a half-covered pixel, at 100 % or 200 %.
        for scale in [1.0f32, 2.0] {
            let image = render(&compose(&readings(), None), scale).unwrap();
            assert!(
                image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[3] == 0 || p[3] == 255),
                "{scale}"
            );
        }
    }

    #[test]
    fn the_mark_is_the_icon_in_the_panels_ink() {
        for (panel, ink) in [
            (Panel::Dark, [0xff, 0xff, 0xff]),
            (Panel::Light, [0x2e, 0x34, 0x36]),
        ] {
            let mark = mark(CIRCLE.as_bytes(), 0, HEIGHT, panel.ink()).unwrap();
            let image = render(&compose(&Image::empty(22, 22), Some(&mark)), 2.0).unwrap();
            let centre = image.at(16, 16);
            // The filter's arithmetic may round one step off.
            for (got, want) in centre[..3].iter().zip(ink) {
                assert!(got.abs_diff(want) <= 1, "{panel:?}: {centre:?}");
            }
            assert_eq!(centre[3], 255);
            assert_eq!(image.at(1, 16)[3], 0, "outside the circle is clear");
        }
        assert_eq!(mark(b"not an svg", 0, HEIGHT, [0; 3]), None);
        assert_eq!(Panel::from_color_scheme(Some(2)), Panel::Light);
        assert_eq!(Panel::from_color_scheme(Some(1)), Panel::Dark);
        assert_eq!(Panel::from_color_scheme(None), Panel::Dark);
    }

    #[test]
    fn the_picture_is_wide_enough_for_a_panel_to_draw_it_at_its_own_size() {
        let mark = mark(CIRCLE.as_bytes(), 0, HEIGHT, Panel::Dark.ink()).unwrap();
        let svg = compose(&readings(), Some(&mark));
        let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &Default::default()).unwrap();
        // GNOME's AppIndicator extension draws a file at its own size when
        // it is at least half again as wide as tall.
        assert!(tree.size().width() >= tree.size().height() * 1.5);
        assert_eq!(tree.size().height(), HEIGHT as f32);
    }

    fn theme(root: &Path, name: &str, index: &str, files: &[&str]) {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("index.theme"), index).unwrap();
        for file in files {
            let path = root.join(name).join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, CIRCLE).unwrap();
        }
    }

    #[test]
    fn the_icon_is_found_by_the_icon_theme_rules() {
        let dir = tempfile::tempdir().unwrap();
        let icons = dir.path().join("icons");
        theme(
            &icons,
            "Parent",
            "[Icon Theme]\nDirectories=16x16/status,symbolic/status\n\n[16x16/status]\nSize=16\nType=Fixed\n\n[symbolic/status]\nSize=16\nType=Scalable\n",
            &[
                "16x16/status/power-profile-balanced-symbolic.svg",
                "symbolic/status/power-profile-balanced-symbolic.svg",
            ],
        );
        theme(
            &icons,
            "Child",
            "[Icon Theme]\nInherits=Parent\nDirectories=scalable/status\n\n[scalable/status]\nSize=16\nType=Scalable\n",
            &["scalable/status/power-profile-performance-symbolic.svg"],
        );
        theme(&icons, "hicolor", "[Icon Theme]\nDirectories=\n", &[]);
        let dirs = vec![icons.clone()];
        assert_eq!(
            find_icon(&dirs, "Child", "power-profile-performance-symbolic", 16),
            Some(icons.join("Child/scalable/status/power-profile-performance-symbolic.svg"))
        );
        // Inherited, and the scalable directory wins.
        assert_eq!(
            find_icon(&dirs, "Child", "power-profile-balanced-symbolic", 16),
            Some(icons.join("Parent/symbolic/status/power-profile-balanced-symbolic.svg"))
        );
        // No theme has it: no mark, never a made-up one.
        assert_eq!(
            find_icon(&dirs, "Child", "power-profile-power-saver-symbolic", 16),
            None
        );
        assert_eq!(
            find_icon(&dirs, "Missing", "power-profile-balanced-symbolic", 16),
            None
        );
    }

    #[test]
    fn theme_directories_follow_the_specification() {
        let dirs = theme_dirs(
            Some(Path::new("/home/a")),
            Some(Path::new("/home/a/.local/share")),
            "/usr/local/share:/usr/share",
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/home/a/.icons"),
                PathBuf::from("/home/a/.local/share/icons"),
                PathBuf::from("/usr/local/share/icons"),
                PathBuf::from("/usr/share/icons"),
            ]
        );
    }
}
