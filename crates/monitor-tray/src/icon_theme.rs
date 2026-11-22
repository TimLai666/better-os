//! The power mode's mark from the desktop's icon theme, and what decides the
//! panel's colour, followed as they change.
//!
//! The icon is looked up by the icon theme rule every Better OS program
//! shares (`better_core::icon_theme`), in the `Status` context. What the
//! desktop says is read from its settings portal
//! (`org.freedesktop.portal.Settings`): `org.gnome.desktop.interface`
//! `icon-theme` and `gtk-theme`, and `org.freedesktop.appearance`
//! `color-scheme`; its `SettingChanged` signal is followed, so a theme
//! picked in Settings redraws the mark at once. The shell theme
//! (`org.gnome.shell.extensions.user-theme` `name`), which the portal does
//! not offer, is read with `gsettings` each time one of those changes:
//! Zorin's appearance tool changes it together with the GTK theme. Read
//! only, never written.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use better_core::icon_theme::{FixedTheme, IconSearch, MAX_ICON_BYTES, acceptable_icon_file};
use hardware_core::power::Profile;

use crate::Tray;
use crate::picture::{self, Panel, PanelHints};

/// Where marks come from, the panel they are drawn for, and the icons
/// already read.
pub struct Marks {
    data_dirs: Vec<PathBuf>,
    theme: Option<String>,
    search: IconSearch,
    hints: PanelHints,
    read: HashMap<Profile, Option<Vec<u8>>>,
}

impl Default for Marks {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Marks {
    /// Marks looked up under these data directories (each holding `icons`).
    pub fn new(data_dirs: Vec<PathBuf>) -> Self {
        let search = IconSearch::with_theme_from(data_dirs.clone(), &[&FixedTheme(None)]);
        Self {
            data_dirs,
            theme: None,
            search,
            hints: PanelHints::default(),
            read: HashMap::new(),
        }
    }

    /// The data directories from this process's environment.
    pub fn from_environment() -> Self {
        Self::new(better_core::icon_theme::data_dirs_from_environment())
    }

    pub fn theme(&self) -> Option<&str> {
        self.theme.as_deref()
    }

    /// A new theme forgets every icon read from the old one. The name is the
    /// shared rule's to check: one that is a path is no theme.
    pub fn set_theme(&mut self, theme: Option<String>) {
        let theme = theme.filter(|t| !t.trim().is_empty());
        if theme != self.theme {
            self.search =
                IconSearch::with_theme_from(self.data_dirs.clone(), &[&FixedTheme(theme.clone())]);
            self.theme = theme;
            self.read.clear();
        }
    }

    pub fn set_hints(&mut self, hints: PanelHints) {
        self.hints = hints;
    }

    pub fn hints(&self) -> &PanelHints {
        &self.hints
    }

    /// The panel the mark is drawn for.
    pub fn panel(&self) -> Panel {
        Panel::from_hints(&self.hints)
    }

    /// The mark for `mode` as an element of the picture, in the panel's ink,
    /// or `None` when the theme has no such icon as an SVG file within the
    /// shared size limit, or it cannot be drawn. A theme's file is read once.
    pub fn mark(&mut self, mode: Profile) -> Option<String> {
        let search = &self.search;
        let svg = self
            .read
            .entry(mode)
            .or_insert_with(|| {
                let file = search.resolve_in("status", picture::icon_name(mode))?;
                let is_svg = file.extension().is_some_and(|e| e == "svg");
                if !is_svg || !acceptable_icon_file(&file) {
                    return None;
                }
                let bytes = std::fs::read(file).ok()?;
                (bytes.len() as u64 <= MAX_ICON_BYTES).then_some(bytes)
            })
            .as_deref()?;
        picture::mark(svg, picture::HEIGHT, Panel::from_hints(&self.hints))
    }
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const INTERFACE: &str = "org.gnome.desktop.interface";
const ICON_THEME: &str = "icon-theme";
const GTK_THEME: &str = "gtk-theme";
const USER_THEME: &str = "org.gnome.shell.extensions.user-theme";
const USER_THEME_NAME: &str = "name";
const APPEARANCE: &str = "org.freedesktop.appearance";
const SCHEME: &str = "color-scheme";

/// A string inside however many variants hold it (`Read` wraps it twice).
fn unwrap_string(value: &zbus::zvariant::Value<'_>) -> Option<String> {
    match value {
        zbus::zvariant::Value::Str(text) => Some(text.to_string()),
        zbus::zvariant::Value::Value(inner) => unwrap_string(inner),
        _ => None,
    }
}

/// A number inside however many variants hold it.
fn unwrap_u32(value: &zbus::zvariant::Value<'_>) -> Option<u32> {
    match value {
        zbus::zvariant::Value::U32(n) => Some(*n),
        zbus::zvariant::Value::Value(inner) => unwrap_u32(inner),
        _ => None,
    }
}

/// One setting from the settings portal.
async fn read_setting(
    connection: &zbus::Connection,
    namespace: &str,
    key: &str,
) -> Option<zbus::zvariant::OwnedValue> {
    for method in ["ReadOne", "Read"] {
        let reply = connection
            .call_method(
                Some(PORTAL),
                PORTAL_PATH,
                Some(SETTINGS),
                method,
                &(namespace, key),
            )
            .await;
        if let Ok(reply) = reply
            && let Ok(value) = reply.body().deserialize::<zbus::zvariant::OwnedValue>()
        {
            return Some(value);
        }
    }
    None
}

async fn read_string(connection: &zbus::Connection, namespace: &str, key: &str) -> Option<String> {
    unwrap_string(&*read_setting(connection, namespace, key).await?).filter(|s| !s.is_empty())
}

/// The icon theme's name now, from the settings portal (or `gsettings`).
pub async fn read(connection: &zbus::Connection) -> Option<String> {
    read_either(connection, INTERFACE, ICON_THEME).await
}

/// One string setting read with `gsettings` (read only): the shell theme,
/// which the settings portal does not offer, and any setting while the
/// portal has not started yet. `None` without the schema or the tool.
fn gsettings_string(schema: &'static str, key: &'static str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    better_core::icon_theme::parse_gsettings_string(&String::from_utf8_lossy(&output.stdout))
}

/// A string setting from the settings portal, or from `gsettings` when the
/// portal does not answer for it.
async fn read_either(
    connection: &zbus::Connection,
    namespace: &'static str,
    key: &'static str,
) -> Option<String> {
    match read_string(connection, namespace, key).await {
        Some(value) => Some(value),
        None => tokio::task::spawn_blocking(move || gsettings_string(namespace, key))
            .await
            .ok()
            .flatten(),
    }
}

/// What decides the panel's colour now: the shell theme and the GTK theme
/// (from the portal, or `gsettings` where it has none), and the colour
/// scheme.
pub async fn read_hints(connection: &zbus::Connection) -> PanelHints {
    PanelHints {
        shell_theme: read_either(connection, USER_THEME, USER_THEME_NAME).await,
        gtk_theme: read_either(connection, INTERFACE, GTK_THEME).await,
        color_scheme: read_setting(connection, APPEARANCE, SCHEME)
            .await
            .as_deref()
            .and_then(unwrap_u32),
    }
}

/// Whether a changed setting is one the hints read.
fn changes_hints(namespace: &str, key: &str) -> bool {
    matches!(
        (namespace, key),
        (INTERFACE, GTK_THEME) | (USER_THEME, USER_THEME_NAME) | (APPEARANCE, SCHEME)
    )
}

/// Follows the icon theme and the panel's colour for `tray`: reads them now
/// and again each time the portal says one changed, and redraws the
/// picture.
pub async fn follow(connection: zbus::Connection, tray: Arc<Tray>) {
    use zbus::export::futures_core::Stream;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(SETTINGS)
        .and_then(|r| r.member("SettingChanged"))
        .and_then(|r| r.path(PORTAL_PATH))
        .map(|r| r.build());
    let stream = match rule {
        Ok(rule) => zbus::MessageStream::for_match_rule(rule, &connection, Some(8))
            .await
            .ok(),
        Err(_) => None,
    };
    let adopt_hints = |hints: PanelHints| {
        let tray = tray.clone();
        async move {
            let changes = tray.state.lock().await.adopt_panel_hints(hints);
            tray.announce(changes).await;
        }
    };
    let adopt_theme = |theme: Option<String>| {
        let tray = tray.clone();
        async move {
            let changes = tray.state.lock().await.adopt_icon_theme(theme);
            tray.announce(changes).await;
        }
    };
    adopt_hints(read_hints(&connection).await).await;
    adopt_theme(read(&connection).await).await;
    let Some(stream) = stream else {
        eprintln!("better-monitor-tray: icon theme changes cannot be followed");
        return;
    };
    let mut stream = std::pin::pin!(stream);
    while let Some(Ok(message)) =
        std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
    {
        let Ok((namespace, key, value)) =
            message
                .body()
                .deserialize::<(String, String, zbus::zvariant::OwnedValue)>()
        else {
            continue;
        };
        if namespace == INTERFACE && key == ICON_THEME {
            adopt_theme(unwrap_string(&value)).await;
        }
        if changes_hints(&namespace, &key) {
            adopt_hints(read_hints(&connection).await).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_theme_forgets_the_marks_of_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let theme = |name: &str, fill: &str| {
            let base = dir.path().join("icons").join(name);
            std::fs::create_dir_all(base.join("symbolic/status")).unwrap();
            std::fs::write(
                base.join("index.theme"),
                "[Icon Theme]\nDirectories=symbolic/status\n\n[symbolic/status]\nContext=Status\nSize=16\nType=Scalable\n",
            )
            .unwrap();
            std::fs::write(
                base.join("symbolic/status/power-profile-balanced-symbolic.svg"),
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect x="{fill}" y="2" width="4" height="12"/></svg>"#
                ),
            )
            .unwrap();
        };
        theme("One", "2");
        theme("Two", "10");
        let mut marks = Marks::new(vec![dir.path().to_path_buf()]);
        assert_eq!(marks.mark(Profile::Balanced), None, "no theme, no mark");
        marks.set_theme(Some("One".into()));
        let one = marks.mark(Profile::Balanced).unwrap();
        assert_eq!(marks.mark(Profile::Performance), None);
        marks.set_theme(Some("Two".into()));
        let two = marks.mark(Profile::Balanced).unwrap();
        assert_ne!(one, two);
        // A light panel draws the same icon in dark ink.
        marks.set_hints(PanelHints {
            gtk_theme: Some("ZorinBlue-Light".into()),
            ..PanelHints::default()
        });
        assert_eq!(marks.panel(), Panel::Light);
        assert_ne!(marks.mark(Profile::Balanced).unwrap(), two);
        // A theme name that is a path is no theme.
        marks.set_theme(Some("../One".into()));
        assert_eq!(marks.mark(Profile::Balanced), None);
    }

    #[test]
    fn an_icon_over_the_size_limit_is_no_mark() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("icons/Big/scalable/status");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            dir.path().join("icons/Big/index.theme"),
            "[Icon Theme]\nDirectories=scalable/status\n\n[scalable/status]\nContext=Status\nSize=16\nType=Scalable\n",
        )
        .unwrap();
        let mut big =
            String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">"#);
        while (big.len() as u64) <= MAX_ICON_BYTES {
            big.push_str("<!-- padding padding padding padding padding padding -->");
        }
        big.push_str("</svg>");
        std::fs::write(base.join("power-profile-balanced-symbolic.svg"), big).unwrap();
        let mut marks = Marks::new(vec![dir.path().to_path_buf()]);
        marks.set_theme(Some("Big".into()));
        assert_eq!(marks.mark(Profile::Balanced), None);
    }

    #[test]
    fn the_portal_wraps_the_name_in_variants() {
        let inner = zbus::zvariant::Value::from("Yaru");
        let twice = zbus::zvariant::Value::Value(Box::new(zbus::zvariant::Value::Value(Box::new(
            inner.try_clone().unwrap(),
        ))));
        assert_eq!(unwrap_string(&inner).as_deref(), Some("Yaru"));
        assert_eq!(unwrap_string(&twice).as_deref(), Some("Yaru"));
        assert_eq!(unwrap_string(&zbus::zvariant::Value::from(3u32)), None);
        let scheme = zbus::zvariant::Value::Value(Box::new(zbus::zvariant::Value::from(2u32)));
        assert_eq!(unwrap_u32(&scheme), Some(2));
        assert!(changes_hints(INTERFACE, GTK_THEME));
        assert!(changes_hints(USER_THEME, USER_THEME_NAME));
        assert!(!changes_hints(INTERFACE, ICON_THEME));
    }
}
