//! Freedesktop desktop-entry and icon-theme lookup for the Apps table.
//!
//! This module only reads metadata and image files. `Exec=` is parsed for an
//! executable basename so fallback process groups can be matched, but it is
//! never executed or passed to a shell.

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_ICON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_WALK_DEPTH: usize = 5;

#[derive(Clone, Debug, Default)]
struct DesktopEntry {
    icon: Option<String>,
    executable: Option<String>,
    startup_wm_class: Option<String>,
}

#[derive(Default)]
struct DesktopCatalog {
    loaded_at: Option<Instant>,
    by_id: HashMap<String, DesktopEntry>,
    by_executable: HashMap<String, DesktopEntry>,
    by_wm_class: HashMap<String, DesktopEntry>,
    icon_index: HashMap<String, PathBuf>,
    resolved: HashMap<String, Option<PathBuf>>,
}

static CATALOG: OnceLock<Mutex<DesktopCatalog>> = OnceLock::new();

pub fn app_icon_path(identity: &str) -> Option<PathBuf> {
    let cache = CATALOG.get_or_init(|| Mutex::new(DesktopCatalog::default()));
    let mut catalog = cache.lock().ok()?;
    if catalog
        .loaded_at
        .is_none_or(|loaded| loaded.elapsed() >= CACHE_TTL)
    {
        *catalog = load_catalog();
    }

    let cache_key = identity.to_ascii_lowercase();
    if let Some(path) = catalog.resolved.get(&cache_key) {
        return path.clone();
    }

    let entry = identity_keys(identity).into_iter().find_map(|key| {
        catalog
            .by_id
            .get(&key)
            .or_else(|| catalog.by_executable.get(&key))
            .or_else(|| catalog.by_wm_class.get(&key))
            .cloned()
    });
    let path = entry
        .and_then(|entry| entry.icon)
        .and_then(|icon| resolve_icon(&icon, &catalog.icon_index));
    catalog.resolved.insert(cache_key, path.clone());
    path
}

fn load_catalog() -> DesktopCatalog {
    let mut catalog = DesktopCatalog {
        loaded_at: Some(Instant::now()),
        ..DesktopCatalog::default()
    };

    for directory in application_directories() {
        collect_desktop_entries(&directory, 0, &mut catalog);
    }
    catalog.icon_index = build_icon_index();
    catalog
}

fn collect_desktop_entries(directory: &Path, depth: usize, catalog: &mut DesktopCatalog) {
    if depth > 2 {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_entries(&path, depth + 1, catalog);
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
            continue;
        }
        let Some(document) = fs::read_to_string(&path).ok() else {
            continue;
        };
        let Some(parsed) = parse_desktop_entry(&document) else {
            continue;
        };
        let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let id = normalize_key(id);
        catalog.by_id.entry(id).or_insert_with(|| parsed.clone());
        if let Some(executable) = &parsed.executable {
            catalog
                .by_executable
                .entry(normalize_key(executable))
                .or_insert_with(|| parsed.clone());
        }
        if let Some(wm_class) = &parsed.startup_wm_class {
            catalog
                .by_wm_class
                .entry(normalize_key(wm_class))
                .or_insert(parsed);
        }
    }
}

fn parse_desktop_entry(document: &str) -> Option<DesktopEntry> {
    let mut in_desktop_entry = false;
    let mut application = true;
    let mut hidden = false;
    let mut icon = None;
    let mut executable = None;
    let mut startup_wm_class = None;

    for raw_line in document.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Type" => application = value.eq_ignore_ascii_case("Application"),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "Icon" if !value.is_empty() => icon = Some(value.to_string()),
            "Exec" if !value.is_empty() => executable = executable_basename(value),
            "StartupWMClass" if !value.is_empty() => startup_wm_class = Some(value.to_string()),
            _ => {}
        }
    }

    (application && !hidden && icon.is_some()).then_some(DesktopEntry {
        icon,
        executable,
        startup_wm_class,
    })
}

fn executable_basename(command: &str) -> Option<String> {
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in command.trim().chars() {
        if escaped {
            token.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(expected) = quote {
            if character == expected {
                quote = None;
            } else {
                token.push(character);
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character.is_whitespace() {
            break;
        } else {
            token.push(character);
        }
    }
    if token.is_empty() || token.starts_with('%') {
        return None;
    }
    Path::new(&token)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
}

fn identity_keys(identity: &str) -> Vec<String> {
    let raw = identity
        .strip_prefix("process:")
        .or_else(|| identity.strip_prefix("cgroup:"))
        .unwrap_or(identity)
        .rsplit('/')
        .next()
        .unwrap_or(identity)
        .trim_end_matches(".scope")
        .trim_end_matches(".service")
        .trim_end_matches(".desktop")
        .replace("\\x2d", "-");

    let mut keys = vec![normalize_key(&raw)];
    if let Some(value) = raw.strip_prefix("app-") {
        keys.push(normalize_key(value.split('@').next().unwrap_or(value)));
    }
    if let Some(value) = raw.strip_prefix("flatpak-") {
        keys.push(normalize_key(value.split('-').next().unwrap_or(value)));
    }
    keys.sort();
    keys.dedup();
    keys
}

fn normalize_key(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(".desktop")
        .to_ascii_lowercase()
}

fn application_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let data_home = env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    if let Some(data_home) = data_home {
        directories.push(data_home.join("applications"));
    }

    let data_dirs = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    directories.extend(
        data_dirs
            .split(':')
            .filter(|value| !value.is_empty())
            .map(|value| PathBuf::from(value).join("applications")),
    );
    deduplicate_paths(directories)
}

fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join(".icons"));
    }
    let data_home = env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    if let Some(data_home) = data_home {
        roots.push(data_home.join("icons"));
    }
    let data_dirs = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for directory in data_dirs.split(':').filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(directory).join("icons"));
        roots.push(PathBuf::from(directory).join("pixmaps"));
    }
    deduplicate_paths(roots)
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn build_icon_index() -> HashMap<String, PathBuf> {
    let mut candidates: HashMap<String, (i32, PathBuf)> = HashMap::new();
    for (root_index, root) in icon_roots().into_iter().enumerate() {
        collect_icons(&root, 0, 1_000 - root_index as i32 * 10, &mut candidates);
    }
    candidates
        .into_iter()
        .map(|(key, (_, path))| (key, path))
        .collect()
}

fn collect_icons(
    directory: &Path,
    depth: usize,
    root_priority: i32,
    candidates: &mut HashMap<String, (i32, PathBuf)>,
) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if depth >= 2 && should_skip_icon_context(name) {
                continue;
            }
            collect_icons(&path, depth + 1, root_priority, candidates);
            continue;
        }
        if !is_supported_icon(&path) {
            continue;
        }
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() == 0 || metadata.len() > MAX_ICON_BYTES {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let key = normalize_key(stem);
        let score = root_priority + icon_score(&path);
        let replace = candidates
            .get(&key)
            .is_none_or(|(existing_score, _)| score > *existing_score);
        if replace {
            candidates.insert(key, (score, path));
        }
    }
}

fn should_skip_icon_context(name: &str) -> bool {
    matches!(
        name,
        "actions"
            | "animations"
            | "categories"
            | "emblems"
            | "emotes"
            | "mimetypes"
            | "places"
            | "status"
    )
}

fn is_supported_icon(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("svg" | "png" | "xpm" | "ico" | "webp")
    )
}

fn icon_score(path: &Path) -> i32 {
    let value = path.to_string_lossy().to_ascii_lowercase();
    let mut score = 0;
    if value.contains("/apps/") {
        score += 100;
    }
    if value.contains("/scalable/") {
        score += 60;
    }
    for (marker, points) in [
        ("/512x512/", 55),
        ("/256x256/", 50),
        ("/128x128/", 45),
        ("/64x64/", 40),
        ("/48x48/", 35),
        ("/32x32/", 25),
        ("/24x24/", 15),
        ("/16x16/", 5),
    ] {
        if value.contains(marker) {
            score += points;
            break;
        }
    }
    if value.contains("symbolic") {
        score -= 20;
    }
    score
        + match path.extension().and_then(|value| value.to_str()) {
            Some("svg") => 5,
            Some("png") => 4,
            Some("webp") => 3,
            Some("ico") => 2,
            Some("xpm") => 1,
            _ => 0,
        }
}

fn resolve_icon(icon: &str, index: &HashMap<String, PathBuf>) -> Option<PathBuf> {
    let direct = PathBuf::from(icon);
    if direct.is_absolute() && is_safe_direct_icon(&direct) {
        return fs::canonicalize(direct).ok();
    }
    let key = normalize_key(
        Path::new(icon)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(icon),
    );
    index.get(&key).cloned()
}

fn is_safe_direct_icon(path: &Path) -> bool {
    if !is_supported_icon(path) {
        return false;
    }
    fs::metadata(path)
        .map(|metadata| {
            metadata.is_file() && metadata.len() > 0 && metadata.len() <= MAX_ICON_BYTES
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entries_are_metadata_only() {
        let entry = parse_desktop_entry(
            "[Desktop Entry]\nType=Application\nIcon=org.example.App\nExec=\"/opt/Example App/bin/example\" %U\nStartupWMClass=ExampleApp\n",
        )
        .unwrap();
        assert_eq!(entry.icon.as_deref(), Some("org.example.App"));
        assert_eq!(entry.executable.as_deref(), Some("example"));
        assert_eq!(entry.startup_wm_class.as_deref(), Some("ExampleApp"));
        assert!(parse_desktop_entry("[Desktop Entry]\nHidden=true\nIcon=hidden\n").is_none());
    }

    #[test]
    fn cgroup_and_process_identities_produce_lookup_keys() {
        assert!(identity_keys("org.gnome.Nautilus").contains(&"org.gnome.nautilus".to_string()));
        assert!(identity_keys("process:firefox").contains(&"firefox".to_string()));
        assert!(
            identity_keys("cgroup:/app-org.example.App@123.scope")
                .contains(&"org.example.app".to_string())
        );
    }

    #[test]
    fn app_icons_outscore_symbolic_and_tiny_candidates() {
        assert!(
            icon_score(Path::new("/icons/hicolor/scalable/apps/example.svg"))
                > icon_score(Path::new("/icons/hicolor/16x16/apps/example.png"))
        );
        assert!(
            icon_score(Path::new("/icons/hicolor/64x64/apps/example.png"))
                > icon_score(Path::new(
                    "/icons/hicolor/symbolic/apps/example-symbolic.svg"
                ))
        );
    }
}
