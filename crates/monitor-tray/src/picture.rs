//! The one picture Better Monitor shows in a panel (owner, 2026-11-22,
//! ticket 295): the power mode's mark, a gap, and the readings, side by
//! side, as one macOS menu bar item is. GPUI-free and bus-free, so the
//! taskbar item and the Better OS session's own top bar (tickets 156 and
//! 157) compose the same picture.
//!
//! The mark is the desktop's own power mode icon
//! (`power-profile-*-symbolic`), looked up in the current icon theme by the
//! icon theme rules and drawn with `resvg`, the SVG renderer GPUI already
//! brings. It is tinted by the readings' rule: light ink with a dark edge,
//! so it reads on a light panel and a dark one. A theme without the icon
//! gets no mark; nothing is drawn in its place.
//!
//! The readings are drawn on their own canvas exactly as before and only
//! scaled to the picture's height; the mark adds width beside them and never
//! takes any from them.

use std::path::{Path, PathBuf};

use hardware_core::power::Profile;

/// A picture: width, height, and premultiplied-free RGBA8 pixels, row by
/// row.
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

    fn put(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].copy_from_slice(&pixel);
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

    /// The picture as a PNG file's bytes.
    pub fn png(&self) -> Option<Vec<u8>> {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(self.width, self.height)?;
        for (pixel, rgba) in pixmap
            .pixels_mut()
            .iter_mut()
            .zip(self.rgba.as_chunks::<4>().0)
        {
            *pixel = resvg::tiny_skia::ColorU8::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3])
                .premultiply();
        }
        pixmap.encode_png().ok()
    }
}

/// The readings' light ink and dark edge (`icon::INK`, `icon::OUTLINE`), as
/// RGBA.
const INK: [u8; 3] = [0xe8, 0xe8, 0xe8];
const EDGE: [u8; 4] = [0x1c, 0x1c, 0x1c, 200];

/// How opaque a pixel of the icon must be to count as its shape.
const STRONG: u8 = 96;

/// The gap between the mark and the readings, in pixels of the picture.
pub const GAP: u32 = 4;

/// The picture's height: the size GNOME Shell draws a panel icon at
/// (`PANEL_ICON_SIZE`, which the AppIndicator extension uses unless its own
/// icon size is set). A wide picture is drawn at its file's own size, so
/// this is its size in the panel.
pub const HEIGHT: u32 = 16;

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

/// The mark: the SVG drawn square at `size` pixels, then given the
/// readings' ink and a dark edge by its own shape, so a symbolic icon's
/// colour never decides how it reads on the panel. `None` when the file is
/// not an SVG resvg can draw.
pub fn mark(svg: &[u8], size: u32) -> Option<Image> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    // A pixel of margin on each side for the edge.
    let inner = size.checked_sub(2)?.max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(inner, inner)?;
    let tree_size = tree.size();
    let scale = (inner as f32 / tree_size.width()).min(inner as f32 / tree_size.height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut alpha = vec![0u8; (size * size) as usize];
    for y in 0..inner {
        for x in 0..inner {
            let pixel = pixmap.pixel(x, y)?;
            alpha[((y + 1) * size + x + 1) as usize] = pixel.alpha();
        }
    }
    let mut image = Image::empty(size, size);
    for y in 0..size {
        for x in 0..size {
            // Solid ink where the shape is; around it, the dark edge, which
            // takes the place of the shape's faint antialiased rim; a faint
            // pixel nowhere near the shape stays faint ink.
            let a = alpha[(y * size + x) as usize];
            if a >= STRONG {
                image.put(x, y, [INK[0], INK[1], INK[2], a]);
                continue;
            }
            let near = (-1i32..=1).any(|dy| {
                (-1i32..=1).any(|dx| {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    nx >= 0
                        && ny >= 0
                        && (nx as u32) < size
                        && (ny as u32) < size
                        && alpha[(ny as u32 * size + nx as u32) as usize] >= STRONG
                })
            });
            if near {
                image.put(x, y, EDGE);
            } else if a > 0 {
                image.put(x, y, [INK[0], INK[1], INK[2], a]);
            }
        }
    }
    Some(image)
}

/// The readings' square canvas at `height` pixels, each pixel the average
/// of the canvas pixels it covers (alpha-weighted), as a panel scales it.
pub fn scaled(readings: &Image, height: u32) -> Image {
    if readings.height == height {
        return readings.clone();
    }
    let width = (readings.width * height).div_ceil(readings.height.max(1));
    let mut out = Image::empty(width, height);
    let (sx, sy) = (
        readings.width as f64 / width as f64,
        readings.height as f64 / height as f64,
    );
    for y in 0..height {
        for x in 0..width {
            let (x0, x1) = (x as f64 * sx, (x + 1) as f64 * sx);
            let (y0, y1) = (y as f64 * sy, (y + 1) as f64 * sy);
            let mut sum = [0f64; 4];
            let mut area = 0f64;
            let mut cy = y0.floor() as u32;
            while (cy as f64) < y1 && cy < readings.height {
                let hy = (y1.min(cy as f64 + 1.0) - y0.max(cy as f64)).max(0.0);
                let mut cx = x0.floor() as u32;
                while (cx as f64) < x1 && cx < readings.width {
                    let wx = (x1.min(cx as f64 + 1.0) - x0.max(cx as f64)).max(0.0);
                    let weight = wx * hy;
                    let p = readings.at(cx, cy);
                    let a = p[3] as f64 * weight;
                    sum[0] += p[0] as f64 * a;
                    sum[1] += p[1] as f64 * a;
                    sum[2] += p[2] as f64 * a;
                    sum[3] += a;
                    area += weight;
                    cx += 1;
                }
                cy += 1;
            }
            if sum[3] > 0.0 {
                out.put(
                    x,
                    y,
                    [
                        (sum[0] / sum[3]).round() as u8,
                        (sum[1] / sum[3]).round() as u8,
                        (sum[2] / sum[3]).round() as u8,
                        (sum[3] / area.max(f64::EPSILON)).round().min(255.0) as u8,
                    ],
                );
            }
        }
    }
    out
}

/// The one picture: the mark, [`GAP`], then the readings; or the readings
/// alone, as narrow as before, when there is no mark.
pub fn compose(readings: &Image, mark: Option<&Image>) -> Image {
    let Some(mark) = mark else {
        return readings.clone();
    };
    let height = readings.height.max(mark.height);
    let width = mark.width + GAP + readings.width;
    let mut out = Image::empty(width, height);
    let mut paste = |image: &Image, left: u32| {
        let top = (height - image.height) / 2;
        for y in 0..image.height {
            for x in 0..image.width {
                out.put(left + x, top + y, image.at(x, y));
            }
        }
    };
    paste(mark, 0);
    paste(readings, mark.width + GAP);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="6" fill="#2e3436"/></svg>"##;

    fn readings() -> Image {
        let mut image = Image::empty(22, 22);
        for y in 4..18 {
            image.put(3, y, [0x4c, 0xb4, 0xf5, 255]);
        }
        image
    }

    #[test]
    fn the_readings_are_the_same_pixels_with_and_without_the_mark() {
        let readings = scaled(&readings(), 16);
        let mark = mark(CIRCLE.as_bytes(), 16).unwrap();
        let alone = compose(&readings, None);
        let together = compose(&readings, Some(&mark));
        assert_eq!(
            alone, readings,
            "no mark, the picture is as narrow as before"
        );
        assert_eq!(together.width, 16 + GAP + readings.width);
        assert_eq!(together.height, 16);
        assert_eq!(
            together.region(16 + GAP, 0, readings.width, 16),
            alone.region(0, 0, readings.width, 16)
        );
        // The gap is empty.
        assert!(together.region(16, 0, GAP, 16).iter().all(|p| p[3] == 0));
    }

    #[test]
    fn the_mark_takes_the_readings_ink_and_edge_whatever_the_svgs_colour() {
        let mark = mark(CIRCLE.as_bytes(), 16).unwrap();
        let centre = mark.at(8, 8);
        assert_eq!(&centre[..3], &INK);
        assert_eq!(centre[3], 255);
        assert_eq!(mark.at(0, 8), [0, 0, 0, 0], "the margin is empty");
        // The circle's rim has the dark edge just outside it.
        assert!((0..16).any(|x| mark.at(x, 8) == EDGE));
        assert!(mark.png().is_some());
        assert_eq!(super::mark(b"not an svg", 16), None);
    }

    #[test]
    fn scaling_keeps_a_square_square_and_a_full_pixel_full() {
        let mut full = Image::empty(22, 22);
        for y in 0..22 {
            for x in 0..22 {
                full.put(x, y, [10, 20, 30, 255]);
            }
        }
        let small = scaled(&full, 16);
        assert_eq!((small.width, small.height), (16, 16));
        assert!(small.rgba.chunks(4).all(|p| p == [10, 20, 30, 255]));
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
