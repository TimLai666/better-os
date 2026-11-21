//! The power mode's mark from the desktop's icon theme, and the theme
//! followed as it changes.
//!
//! The theme's name is read from the desktop's settings portal
//! (`org.freedesktop.portal.Settings`, `org.gnome.desktop.interface`
//! `icon-theme`), and its `SettingChanged` signal is followed, so a theme
//! picked in Settings redraws the mark at once. Without the portal the
//! theme is not known; the lookup then has only `hicolor`, as the Icon
//! Theme Specification falls back to, and a mark it does not have is not
//! drawn.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hardware_core::power::Profile;

use crate::Tray;
use crate::picture::{self, Image};

/// Where marks come from, and the ones already drawn.
#[derive(Default)]
pub struct Marks {
    dirs: Vec<PathBuf>,
    theme: Option<String>,
    drawn: HashMap<(Profile, u32), Option<Image>>,
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

    /// A new theme forgets every mark drawn from the old one.
    pub fn set_theme(&mut self, theme: Option<String>) {
        let theme = theme.filter(|t| !t.trim().is_empty());
        if theme != self.theme {
            self.theme = theme;
            self.drawn.clear();
        }
    }

    /// The mark for `mode` at `size` pixels, or `None` when the theme has
    /// no such icon or it cannot be drawn.
    pub fn mark(&mut self, mode: Profile, size: u32) -> Option<Image> {
        let (dirs, theme) = (&self.dirs, self.theme.as_deref().unwrap_or("hicolor"));
        self.drawn
            .entry((mode, size))
            .or_insert_with(|| {
                let file = picture::find_icon(dirs, theme, picture::icon_name(mode), size)?;
                let svg = std::fs::read(file).ok()?;
                picture::mark(&svg, size)
            })
            .clone()
    }
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.gnome.desktop.interface";
const KEY: &str = "icon-theme";

/// A string inside however many variants hold it (`Read` wraps it twice).
fn unwrap_string(value: &zbus::zvariant::Value<'_>) -> Option<String> {
    match value {
        zbus::zvariant::Value::Str(text) => Some(text.to_string()),
        zbus::zvariant::Value::Value(inner) => unwrap_string(inner),
        _ => None,
    }
}

/// The icon theme's name now, from the settings portal.
pub async fn read(connection: &zbus::Connection) -> Option<String> {
    for method in ["ReadOne", "Read"] {
        let reply = connection
            .call_method(
                Some(PORTAL),
                PORTAL_PATH,
                Some(SETTINGS),
                method,
                &(NAMESPACE, KEY),
            )
            .await;
        if let Ok(reply) = reply
            && let Ok(value) = reply.body().deserialize::<zbus::zvariant::OwnedValue>()
        {
            return unwrap_string(&value);
        }
    }
    None
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
        .and_then(|r| r.add_arg(NAMESPACE))
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
    adopt(read(&connection).await).await;
    let Some(stream) = stream else {
        eprintln!("better-monitor-tray: icon theme changes cannot be followed");
        return;
    };
    let mut stream = std::pin::pin!(stream);
    while let Some(Ok(message)) =
        std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
    {
        let Ok((_, key, value)) = message
            .body()
            .deserialize::<(String, String, zbus::zvariant::OwnedValue)>()
        else {
            continue;
        };
        if key == KEY {
            adopt(unwrap_string(&value)).await;
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
        assert_eq!(marks.mark(Profile::Balanced, 16), None, "no theme, no mark");
        marks.set_theme(Some("One".into()));
        let one = marks.mark(Profile::Balanced, 16).unwrap();
        assert_eq!(marks.mark(Profile::Performance, 16), None);
        marks.set_theme(Some("Two".into()));
        let two = marks.mark(Profile::Balanced, 16).unwrap();
        assert_ne!(one, two);
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
    }
}
