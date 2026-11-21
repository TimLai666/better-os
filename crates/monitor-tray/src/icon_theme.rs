//! The power mode's mark from the desktop's icon theme, and the theme and
//! the colour scheme followed as they change.
//!
//! Both are read from the desktop's settings portal
//! (`org.freedesktop.portal.Settings`: `org.gnome.desktop.interface`
//! `icon-theme`, and `org.freedesktop.appearance` `color-scheme`, which says
//! whether the panel's symbolic icons are light or dark), and its
//! `SettingChanged` signal is followed, so a theme or scheme picked in
//! Settings redraws the mark at once. Without the portal the theme is not
//! known; the lookup then has only `hicolor`, as the Icon Theme
//! Specification falls back to, and a mark it does not have is not drawn.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hardware_core::power::Profile;

use crate::Tray;
use crate::picture::{self, Panel};

/// Where marks come from, the panel they are drawn for, and the icons
/// already read.
pub struct Marks {
    dirs: Vec<PathBuf>,
    theme: Option<String>,
    panel: Panel,
    read: HashMap<Profile, Option<Vec<u8>>>,
}

impl Default for Marks {
    fn default() -> Self {
        Self {
            dirs: Vec::new(),
            theme: None,
            panel: Panel::from_color_scheme(None),
            read: HashMap::new(),
        }
    }
}

impl Marks {
    /// Marks looked up under these icon directories
    /// ([`picture::theme_dirs`]).
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            ..Self::default()
        }
    }

    /// The directories from this process's environment.
    pub fn from_environment() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        Self::new(picture::theme_dirs(
            home.as_deref(),
            data_home.as_deref(),
            &data_dirs,
        ))
    }

    pub fn theme(&self) -> Option<&str> {
        self.theme.as_deref()
    }

    /// A new theme forgets every icon read from the old one.
    pub fn set_theme(&mut self, theme: Option<String>) {
        let theme = theme.filter(|t| !t.trim().is_empty());
        if theme != self.theme {
            self.theme = theme;
            self.read.clear();
        }
    }

    pub fn set_panel(&mut self, panel: Panel) {
        self.panel = panel;
    }

    pub fn panel(&self) -> Panel {
        self.panel
    }

    /// The mark for `mode` as an element of the picture, at the left in the
    /// panel's ink, or `None` when the theme has no such icon or it cannot
    /// be drawn. A theme's file is read once.
    pub fn mark(&mut self, mode: Profile) -> Option<String> {
        let (dirs, theme) = (&self.dirs, self.theme.as_deref().unwrap_or("hicolor"));
        let svg = self
            .read
            .entry(mode)
            .or_insert_with(|| {
                let file =
                    picture::find_icon(dirs, theme, picture::icon_name(mode), picture::HEIGHT)?;
                std::fs::read(file).ok()
            })
            .as_deref()?;
        picture::mark(svg, 0, picture::HEIGHT, self.panel.ink())
    }
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.gnome.desktop.interface";
const KEY: &str = "icon-theme";
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

/// The icon theme's name now, from the settings portal.
pub async fn read(connection: &zbus::Connection) -> Option<String> {
    unwrap_string(&*read_setting(connection, NAMESPACE, KEY).await?)
}

/// Whether the panel is light or dark now, from the settings portal.
pub async fn read_panel(connection: &zbus::Connection) -> Panel {
    let scheme = read_setting(connection, APPEARANCE, SCHEME).await;
    Panel::from_color_scheme(scheme.as_deref().and_then(unwrap_u32))
}

/// Follows the icon theme for `tray`: reads it now and again each time the
/// portal says it changed, and redraws the picture.
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
    let adopt = |theme: Option<String>| {
        let tray = tray.clone();
        async move {
            let changes = tray.state.lock().await.adopt_icon_theme(theme);
            tray.announce(changes).await;
        }
    };
    let adopt_panel = |panel: Panel| {
        let tray = tray.clone();
        async move {
            let changes = tray.state.lock().await.adopt_panel(panel);
            tray.announce(changes).await;
        }
    };
    adopt_panel(read_panel(&connection).await).await;
    adopt(read(&connection).await).await;
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
        if namespace == NAMESPACE && key == KEY {
            adopt(unwrap_string(&value)).await;
        } else if namespace == APPEARANCE && key == SCHEME {
            adopt_panel(Panel::from_color_scheme(unwrap_u32(&value))).await;
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
            let base = dir.path().join(name);
            std::fs::create_dir_all(base.join("status")).unwrap();
            std::fs::write(
                base.join("index.theme"),
                "[Icon Theme]\nDirectories=status\n\n[status]\nSize=16\nType=Scalable\n",
            )
            .unwrap();
            std::fs::write(
                base.join("status/power-profile-balanced-symbolic.svg"),
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
        assert!(one.contains(r#"x="2""#), "{one}");
        assert_eq!(marks.mark(Profile::Performance), None);
        marks.set_theme(Some("Two".into()));
        let two = marks.mark(Profile::Balanced).unwrap();
        assert!(two.contains(r#"x="10""#), "{two}");
        // A light panel draws the same icon in dark ink.
        marks.set_panel(Panel::Light);
        assert_ne!(marks.mark(Profile::Balanced).unwrap(), two);
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
    }
}
