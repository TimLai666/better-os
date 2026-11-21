//! The freedesktop icon theme lookup every Better OS program uses (Icon
//! Theme Specification): the desktop's theme, what it inherits, Adwaita and
//! `hicolor`, with the guards a theme anyone can install in
//! `~/.local/share/icons` needs. GPUI-free, so a window (`better_ui::icons`
//! re-exports it) and a process with no window (Better Monitor's taskbar
//! item, ticket 295) share one rule.

use std::path::{Path, PathBuf};

/// The sizes looked in when a theme has no `index.theme`, largest useful
/// first. Scalable comes first because it draws sharp at any size.
pub const SIZES: [&str; 8] = [
    "scalable", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32", "24x24",
];

/// The fixed size looked for first: a launcher draws icons at 24 to 64
/// pixels.
const PREFERRED_SIZE: u32 = 48;

/// The theme searched after the desktop's own chain and before
/// `hicolor`: GNOME's, which most application icons a GNOME desktop
/// shows fall back to.
pub const FALLBACK_THEME: &str = "Adwaita";

/// Where the name of the desktop's icon theme comes from.
pub trait IconThemeSource {
    fn icon_theme(&self) -> Option<String>;
}

/// GNOME's own setting, `org.gnome.desktop.interface icon-theme`, which
/// is what GNOME and Zorin set and what GTK applications follow there.
/// Read through `gsettings`, the GLib tool that reads it from whichever
/// backend the session uses; read only, never written.
pub struct GnomeInterfaceSetting;

impl IconThemeSource for GnomeInterfaceSetting {
    fn icon_theme(&self) -> Option<String> {
        let output = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_gsettings_string(&String::from_utf8_lossy(&output.stdout))
    }
}

/// `gtk-icon-theme-name` from GTK's `settings.ini`, for a desktop that is
/// not GNOME.
pub struct GtkSettingsIni {
    pub config: Option<PathBuf>,
}

impl IconThemeSource for GtkSettingsIni {
    fn icon_theme(&self) -> Option<String> {
        gtk_icon_theme(self.config.as_deref()?)
    }
}

/// A theme name fixed in advance, for tests.
pub struct FixedTheme(pub Option<String>);

impl IconThemeSource for FixedTheme {
    fn icon_theme(&self) -> Option<String> {
        self.0.clone()
    }
}

/// What `gsettings get` prints for a string: `'ZorinBlue-Light'`.
pub fn parse_gsettings_string(text: &str) -> Option<String> {
    let text = text.trim();
    let inner = text
        .strip_prefix('\'')
        .and_then(|t| t.strip_suffix('\''))
        .or_else(|| text.strip_prefix('"').and_then(|t| t.strip_suffix('"')))
        .unwrap_or(text);
    (!inner.is_empty() && valid_name(inner)).then(|| inner.to_string())
}

/// How many themes an inheritance chain may hold. A theme that inherits
/// from itself, or a long chain, stops here rather than looping.
const MAX_THEMES: usize = 8;

/// Where to look.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IconSearch {
    /// `$XDG_DATA_HOME` and every `$XDG_DATA_DIRS` entry, in order.
    pub data_dirs: Vec<PathBuf>,
    /// Theme names to try before `hicolor`, such as the desktop's own and
    /// the themes it inherits from, in order.
    pub themes: Vec<String>,
    /// Each theme's application directories, read once from its
    /// `index.theme`.
    theme_dirs: Vec<(String, Vec<String>)>,
    /// Each theme's other directories with their context, scalable first,
    /// read at the same time.
    context_dirs: Vec<(String, Vec<(String, String)>)>,
}

impl IconSearch {
    /// A search over these data directories and themes, with every
    /// theme's `index.theme` read now rather than once per icon.
    pub fn new(data_dirs: Vec<PathBuf>, themes: Vec<String>) -> IconSearch {
        let mut search = IconSearch {
            data_dirs,
            themes,
            theme_dirs: Vec::new(),
            context_dirs: Vec::new(),
        };
        let names: Vec<String> = search
            .themes
            .iter()
            .cloned()
            .chain(std::iter::once("hicolor".to_string()))
            .collect();
        for name in names {
            if let Some(theme) = search.read_theme(&name) {
                search.theme_dirs.push((name.clone(), theme.app_dirs));
                search.context_dirs.push((name, theme.context_dirs));
            }
        }
        search
    }

    /// The process's data directories, as the XDG base directory rule
    /// defines them, and the icon theme the desktop's GTK settings name,
    /// followed through what it inherits.
    pub fn from_environment() -> IconSearch {
        let config = config_home(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        );
        let gtk = GtkSettingsIni { config };
        IconSearch::with_theme_from(
            data_dirs_from_environment(),
            &[&GnomeInterfaceSetting, &gtk],
        )
    }

    /// A search over these data directories whose theme is the first
    /// one any of `sources` names, followed through what it inherits,
    /// then Adwaita (the theme GNOME's own icons fall back to) before
    /// `hicolor`.
    pub fn with_theme_from(
        data_dirs: Vec<PathBuf>,
        sources: &[&dyn IconThemeSource],
    ) -> IconSearch {
        let base = IconSearch::new(data_dirs.clone(), Vec::new());
        let mut themes = sources
            .iter()
            .find_map(|source| source.icon_theme())
            .map(|name| base.theme_chain(&name))
            .unwrap_or_default();
        if !themes.iter().any(|theme| theme == FALLBACK_THEME) {
            themes.push(FALLBACK_THEME.to_string());
        }
        IconSearch::new(data_dirs, themes)
    }

    /// A theme and everything it inherits, breadth first, without
    /// `hicolor` (which every search ends with) and without repeats.
    pub fn theme_chain(&self, name: &str) -> Vec<String> {
        let mut chain: Vec<String> = Vec::new();
        let mut queue = std::collections::VecDeque::from([name.to_string()]);
        while let Some(theme) = queue.pop_front() {
            if chain.len() >= MAX_THEMES {
                break;
            }
            if theme == "hicolor" || theme.is_empty() || chain.contains(&theme) {
                continue;
            }
            if !valid_name(&theme) {
                continue;
            }
            let inherits = self
                .read_theme(&theme)
                .map(|index| index.inherits)
                .unwrap_or_default();
            chain.push(theme);
            queue.extend(inherits);
        }
        chain
    }

    /// A theme's `index.theme`, from the first data directory that has
    /// one.
    fn read_theme(&self, name: &str) -> Option<ThemeIndex> {
        if !valid_name(name) {
            return None;
        }
        self.data_dirs.iter().find_map(|dir| {
            std::fs::read_to_string(dir.join("icons").join(name).join("index.theme"))
                .ok()
                .map(|text| ThemeIndex::parse(&text))
        })
    }

    /// The application directories searched in one theme.
    fn dirs_for(&self, theme: &str) -> Vec<String> {
        self.theme_dirs
            .iter()
            .find(|(name, _)| name == theme)
            .map(|(_, dirs)| dirs.clone())
            .unwrap_or_else(|| SIZES.iter().map(|size| format!("{size}/apps")).collect())
    }

    /// Every path tried for `name`, in order. Separate from
    /// [`IconSearch::resolve`] so the order can be tested.
    pub fn candidates(&self, name: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let themes: Vec<&str> = self
            .themes
            .iter()
            .map(String::as_str)
            .chain(std::iter::once("hicolor"))
            .collect();
        for theme in &themes {
            let dirs = self.dirs_for(theme);
            for dir in &self.data_dirs {
                for sub in &dirs {
                    for extension in ["svg", "png"] {
                        if sub.starts_with("scalable") && extension == "png" {
                            continue;
                        }
                        paths.push(
                            dir.join("icons")
                                .join(theme)
                                .join(sub)
                                .join(format!("{name}.{extension}")),
                        );
                    }
                }
            }
        }
        for dir in &self.data_dirs {
            for extension in ["svg", "png"] {
                paths.push(dir.join("pixmaps").join(format!("{name}.{extension}")));
            }
        }
        paths
    }

    /// Every path tried for `name` in one icon theme context other than
    /// applications, such as `mimetypes` for a file type or `places` for
    /// a folder: each theme of the search, then `hicolor`, at the usual
    /// sizes, scalable first.
    pub fn candidates_in(&self, context: &str, name: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let themes: Vec<&str> = self
            .themes
            .iter()
            .map(String::as_str)
            .chain(std::iter::once("hicolor"))
            .collect();
        for theme in &themes {
            // The theme's own directories for that context first, as its
            // `index.theme` lists them (Adwaita keeps symbolic icons in
            // `symbolic/status`), then the usual sizes.
            let listed: Vec<String> = self
                .context_dirs
                .iter()
                .find(|(name, _)| name == theme)
                .map(|(_, dirs)| {
                    dirs.iter()
                        .filter(|(_, c)| c.eq_ignore_ascii_case(context))
                        .map(|(dir, _)| dir.clone())
                        .collect()
                })
                .unwrap_or_default();
            let usual = SIZES.iter().map(|size| format!("{size}/{context}"));
            let mut subs: Vec<String> = Vec::new();
            for sub in listed.into_iter().chain(usual) {
                if !subs.contains(&sub) {
                    subs.push(sub);
                }
            }
            for dir in &self.data_dirs {
                for sub in &subs {
                    for extension in ["svg", "png"] {
                        if sub.starts_with("scalable") && extension == "png" {
                            continue;
                        }
                        paths.push(
                            dir.join("icons")
                                .join(theme)
                                .join(sub)
                                .join(format!("{name}.{extension}")),
                        );
                    }
                }
            }
        }
        paths
    }

    /// The file for a themed icon name in one context, or `None`. Neither
    /// the name nor the context may be a path.
    pub fn resolve_in(&self, context: &str, name: &str) -> Option<PathBuf> {
        let name = name.trim();
        if name.is_empty() || !valid_name(name) || !valid_name(context) || context.is_empty() {
            return None;
        }
        self.candidates_in(context, name)
            .into_iter()
            .find(|path| path.is_file())
    }

    /// The file for an `Icon=` value, or `None`. An absolute path must
    /// exist; a name must be a plain file name, never a path, so a
    /// desktop entry cannot point the lookup outside the icon
    /// directories.
    pub fn resolve(&self, icon: &str) -> Option<PathBuf> {
        let icon = icon.trim();
        if icon.is_empty() {
            return None;
        }
        let as_path = Path::new(icon);
        if as_path.is_absolute() {
            return acceptable_icon_file(as_path).then(|| as_path.to_path_buf());
        }
        if !valid_name(icon) {
            return None;
        }
        // `Icon=firefox.png` is tolerated by most desktops: drop a known
        // extension and look the name up.
        let name = icon
            .strip_suffix(".png")
            .or_else(|| icon.strip_suffix(".svg"))
            .unwrap_or(icon);
        self.candidates(name)
            .into_iter()
            .find(|path| path.is_file())
    }
}

/// The largest icon file an `Icon=` path may name. A desktop entry is a
/// file anyone can install; an icon is never this big.
pub const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;

/// The largest PNG an `Icon=` path may name, across and down; decoded
/// at full size, anything larger costs memory for nothing.
pub const MAX_ICON_PIXELS: u32 = 1024;

/// Whether an absolute `Icon=` path from a desktop entry, which anyone
/// can install, is drawn: a regular PNG, SVG or XPM file (by extension
/// and, for a PNG, by its header), at most [`MAX_ICON_BYTES`], a PNG at
/// most [`MAX_ICON_PIXELS`] each way, and never under `/proc`, `/sys` or
/// `/dev`. Reads the file's metadata and first bytes.
pub fn acceptable_icon_file(path: &Path) -> bool {
    if ["/proc", "/sys", "/dev"]
        .iter()
        .any(|kernel| path.starts_with(kernel))
    {
        return false;
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let Some(extension) =
        extension.filter(|extension| matches!(extension.as_str(), "png" | "svg" | "xpm"))
    else {
        return false;
    };
    // `metadata` follows a link, and the file it lands on is what is
    // judged: a regular file, not a device or a fifo.
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() > MAX_ICON_BYTES {
        return false;
    }
    if extension == "png" {
        return png_fits(path);
    }
    true
}

/// Whether a PNG's header says it is at most [`MAX_ICON_PIXELS`] square.
fn png_fits(path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 24];
    let read = std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut header));
    if read.is_err() || &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return false;
    }
    let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]);
    let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]);
    width > 0 && height > 0 && width <= MAX_ICON_PIXELS && height <= MAX_ICON_PIXELS
}

/// A theme directory entry that stays inside the theme: relative, with
/// no `..` and no empty or `.` part. An `index.theme` is a file anyone
/// can install in `~/.local/share/icons`; a `Directories=/` or `../..`
/// entry must not send the lookup elsewhere.
pub fn safe_subdirectory(entry: &str) -> bool {
    !entry.starts_with('/')
        && !entry.contains('\\')
        && entry
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// A plain file name: no separator, and not `.` or `..`.
fn valid_name(name: &str) -> bool {
    !(name.contains('/') || name.contains('\\') || name == "." || name == "..")
}

/// What Better OS reads from a theme's `index.theme`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThemeIndex {
    /// `Inherits=`, in order.
    pub inherits: Vec<String>,
    /// The `Directories=` (and `ScaledDirectories=`) entries whose
    /// `Context` is `Applications`, scalable first, then the largest
    /// fixed size down, scaled (`@2x`) ones after their plain size.
    pub app_dirs: Vec<String>,
    /// Every other listed directory with its `Context`, scalable first.
    pub context_dirs: Vec<(String, String)>,
}

impl ThemeIndex {
    pub fn parse(text: &str) -> ThemeIndex {
        let mut inherits = Vec::new();
        let mut listed: Vec<String> = Vec::new();
        let mut section = String::new();
        // (directory, context, size, scalable)
        let mut details: Vec<(String, String, u32, bool, u32)> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.to_string();
                if section != "Icon Theme" {
                    details.push((section.clone(), String::new(), 0, false, 1));
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if section == "Icon Theme" {
                match key {
                    "Inherits" => inherits.extend(
                        value
                            .split(',')
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .map(String::from),
                    ),
                    "Directories" | "ScaledDirectories" => listed.extend(
                        value
                            .split(',')
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .filter(|v| safe_subdirectory(v))
                            .map(String::from),
                    ),
                    _ => {}
                }
            } else if let Some(entry) = details.last_mut() {
                match key {
                    "Context" => entry.1 = value.to_string(),
                    "Size" => entry.2 = value.parse().unwrap_or(0),
                    "Type" => entry.3 = value.eq_ignore_ascii_case("Scalable"),
                    "Scale" => entry.4 = value.parse().unwrap_or(1),
                    _ => {}
                }
            }
        }
        let mut others: Vec<(String, String, bool)> = details
            .iter()
            .filter(|(dir, context, ..)| {
                listed.contains(dir) && !context.eq_ignore_ascii_case("Applications")
            })
            .map(|(dir, context, _, scalable, _)| (dir.clone(), context.clone(), *scalable))
            .collect();
        others.sort_by(|a, b| b.2.cmp(&a.2));
        let mut apps: Vec<(String, u32, bool, u32)> = details
            .into_iter()
            .filter(|(dir, context, ..)| {
                listed.contains(dir) && context.eq_ignore_ascii_case("Applications")
            })
            .map(|(dir, _, size, scalable, scale)| (dir, size, scalable, scale))
            .collect();
        // Scalable first; then the smallest size that is still at
        // least 48px, upward, because a 512px bitmap or drawing costs
        // far more to load than a launcher row needs; then the sizes
        // below 48, largest first.
        let rank = |size: u32| {
            if size >= PREFERRED_SIZE {
                (0, size)
            } else {
                (1, u32::MAX - size)
            }
        };
        apps.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then(rank(a.1).cmp(&rank(b.1)))
                .then(a.3.cmp(&b.3))
                .then(a.0.cmp(&b.0))
        });
        ThemeIndex {
            inherits,
            app_dirs: apps.into_iter().map(|(dir, ..)| dir).collect(),
            context_dirs: others
                .into_iter()
                .map(|(dir, context, _)| (dir, context))
                .collect(),
        }
    }
}

/// The icon theme GTK is set to, from `gtk-4.0/settings.ini` or
/// `gtk-3.0/settings.ini` under the configuration directory.
pub fn gtk_icon_theme(config: &Path) -> Option<String> {
    ["gtk-4.0", "gtk-3.0"].iter().find_map(|version| {
        let text = std::fs::read_to_string(config.join(version).join("settings.ini")).ok()?;
        text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "gtk-icon-theme-name")
                .then(|| value.trim().trim_matches('"').to_string())
                .filter(|value| !value.is_empty())
        })
    })
}

/// The data directories, as the XDG base directory rule defines them:
/// `$XDG_DATA_HOME` (or `~/.local/share`), then each of `$XDG_DATA_DIRS`
/// (or `/usr/local/share` and `/usr/share`), absolute ones only.
pub fn data_dirs_from_environment() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    let mut dirs: Vec<PathBuf> = data_home.into_iter().collect();
    dirs.extend(
        data_dirs
            .split(':')
            .map(PathBuf::from)
            .filter(|path| path.is_absolute()),
    );
    dirs
}

/// `$XDG_CONFIG_HOME`, or `~/.config` when it is unset or not absolute.
pub fn config_home(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".config"))
        })
}
