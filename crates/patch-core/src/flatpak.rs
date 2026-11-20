//! What an installed Flatpak application is, as its metadata and the
//! override files on top of it say, read the way Flatpak reads them.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::keyfile::{self, KeyFile};

/// A Flatpak application ID that Flatpak itself would accept.
///
/// It comes from directory names on disk, so it is data: it is checked
/// before it names a file or becomes an argument. Three or more dot-separated
/// parts of letters, digits, and `_`, none starting with a digit, `-` only in
/// the last part, at most 255 bytes. That also means it can never start with
/// `-` and be read as an option, or hold `/` or `..` and leave a directory.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AppId(String);

/// Why a string is not an application ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidAppId(pub String);

impl fmt::Display for InvalidAppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a Flatpak application ID", self.0)
    }
}

impl std::error::Error for InvalidAppId {}

impl AppId {
    pub fn parse(id: &str) -> Result<Self, InvalidAppId> {
        let invalid = || InvalidAppId(id.to_string());
        if id.is_empty() || id.len() > 255 {
            return Err(invalid());
        }
        let parts: Vec<&str> = id.split('.').collect();
        if parts.len() < 3 {
            return Err(invalid());
        }
        let last = parts.len() - 1;
        for (index, part) in parts.iter().enumerate() {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return Err(invalid());
            };
            if first.is_ascii_digit() || first == '-' {
                return Err(invalid());
            }
            let allowed =
                |c: char| c.is_ascii_alphanumeric() || c == '_' || (c == '-' && index == last);
            if !part.chars().all(allowed) {
                return Err(invalid());
            }
        }
        Ok(Self(id.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for AppId {
    type Error = InvalidAppId;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<AppId> for String {
    fn from(id: AppId) -> Self {
        id.0
    }
}

/// Which installation an application is in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Installation {
    /// The person's own, under `$XDG_DATA_HOME/flatpak`.
    User,
    /// The machine's, under `/var/lib/flatpak`.
    System,
}

/// The override files that apply to one application, lowest precedence
/// first, as Flatpak layers them. `None` is a file that is not there.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OverrideLayers {
    pub system_global: Option<String>,
    pub system_app: Option<String>,
    pub user_global: Option<String>,
    pub user_app: Option<String>,
}

impl OverrideLayers {
    fn in_order(&self) -> [&Option<String>; 4] {
        [
            &self.system_global,
            &self.system_app,
            &self.user_global,
            &self.user_app,
        ]
    }
}

/// One installed application, as read from disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledApp {
    pub id: AppId,
    pub installation: Installation,
    /// Its `metadata` file.
    pub metadata: String,
    /// Its exported desktop entry, `<id>.desktop`, when it has one.
    pub desktop_entry: Option<String>,
    pub overrides: OverrideLayers,
}

/// The name a desktop entry gives, in the two languages the window speaks.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppName {
    pub default: Option<String>,
    pub zh_tw: Option<String>,
}

/// The Electron base application Flathub's Electron apps are built on.
pub const ELECTRON_BASE_APP: &str = "org.electronjs.Electron2.BaseApp";

impl InstalledApp {
    /// Whether it is built on Flathub's Electron base application, as its
    /// metadata's `[Application] base` says. An app that bundles Electron
    /// another way is not recognised.
    pub fn is_electron(&self) -> bool {
        KeyFile::parse(&self.metadata)
            .get("Application", "base")
            .is_some_and(|base| base.split('/').any(|part| part == ELECTRON_BASE_APP))
    }

    /// The name from its desktop entry.
    pub fn name(&self) -> AppName {
        let Some(entry) = &self.desktop_entry else {
            return AppName::default();
        };
        let file = KeyFile::parse(entry);
        let read = |key: &str| {
            file.get("Desktop Entry", key)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        };
        AppName {
            default: read("Name"),
            zh_tw: read("Name[zh_TW]"),
        }
    }

    /// What the application runs with: its metadata with every override
    /// layered on top.
    pub fn effective(&self) -> Effective {
        let mut effective = Effective::default();
        effective.layer(&self.metadata);
        for layer in self.overrides.in_order().into_iter().flatten() {
            effective.layer(layer);
        }
        effective
    }
}

/// The sockets and environment an application runs with.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Effective {
    /// Each socket named anywhere, and whether it ends up granted.
    pub sockets: BTreeMap<String, bool>,
    pub environment: BTreeMap<String, String>,
}

impl Effective {
    fn layer(&mut self, text: &str) {
        let file = KeyFile::parse(text);
        if let Some(sockets) = file.get("Context", "sockets") {
            for token in keyfile::list(sockets) {
                match token.strip_prefix('!') {
                    Some(name) => self.sockets.insert(name.to_string(), false),
                    None => self.sockets.insert(token, true),
                };
            }
        }
        for key in file.keys("Environment") {
            if let Some(value) = file.get("Environment", key) {
                self.environment.insert(key.to_string(), value.to_string());
            }
        }
        if let Some(unset) = file.get("Context", "unset-environment") {
            for name in keyfile::list(unset) {
                self.environment.remove(&name);
            }
        }
    }

    pub fn grants(&self, socket: &str) -> bool {
        self.sockets.get(socket).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GitHub Desktop's metadata as Flathub ships it, read on the owner's
    /// machine on 2026-11-21 (read only).
    pub(crate) const ELECTRON_METADATA: &str = "\
[Application]
name=io.github.shiftey.Desktop
runtime=org.freedesktop.Platform/x86_64/25.08
sdk=org.freedesktop.Sdk/x86_64/25.08
base=app/org.electronjs.Electron2.BaseApp/x86_64/25.08
command=start-github-desktop

[Context]
shared=network;ipc;
sockets=x11;wayland;fallback-x11;ssh-auth;gpg-agent;

[Environment]
FLATPAK_HOST=1
ELECTRON_OZONE_PLATFORM_HINT=auto
";

    fn app(metadata: &str, overrides: OverrideLayers) -> InstalledApp {
        InstalledApp {
            id: AppId::parse("io.github.shiftey.Desktop").unwrap(),
            installation: Installation::System,
            metadata: metadata.to_string(),
            desktop_entry: Some(
                "[Desktop Entry]\nName=GitHub Desktop\nName[zh_TW]=GitHub 桌面版\n".to_string(),
            ),
            overrides,
        }
    }

    #[test]
    fn ids_flatpak_accepts_and_ones_it_refuses() {
        for good in [
            "io.github.shiftey.Desktop",
            "com.visualstudio.code",
            "org.example.My_App",
            "org.example.my-app",
        ] {
            assert!(AppId::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "two.parts",
            "-x.example.App",
            "org.1example.App",
            "org.ex-ample.App",
            "org..App",
            "org.example.App/../x",
            "org.example.App bad",
            "../../etc.passwd.x",
            &"a.b.".repeat(100),
        ] {
            assert!(AppId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_app_id_is_checked_when_it_is_read_back_from_a_record() {
        let parsed: Result<AppId, _> = serde_json::from_str("\"../evil\"");
        assert!(parsed.is_err());
        let id: AppId = serde_json::from_str("\"org.example.App\"").unwrap();
        assert_eq!(id.as_str(), "org.example.App");
    }

    #[test]
    fn the_electron_base_app_is_recognised_and_nothing_else_is() {
        assert!(app(ELECTRON_METADATA, OverrideLayers::default()).is_electron());
        let gnome =
            "[Application]\nname=net.nokyan.Resources\nruntime=org.gnome.Platform/x86_64/50\n";
        assert!(!app(gnome, OverrideLayers::default()).is_electron());
    }

    #[test]
    fn the_name_comes_from_the_desktop_entry_in_both_languages() {
        let name = app(ELECTRON_METADATA, OverrideLayers::default()).name();
        assert_eq!(name.default.as_deref(), Some("GitHub Desktop"));
        assert_eq!(name.zh_tw.as_deref(), Some("GitHub 桌面版"));
        let mut nameless = app(ELECTRON_METADATA, OverrideLayers::default());
        nameless.desktop_entry = None;
        assert_eq!(nameless.name(), AppName::default());
    }

    #[test]
    fn overrides_layer_in_flatpak_s_order() {
        let effective = app(ELECTRON_METADATA, OverrideLayers::default()).effective();
        assert!(effective.grants("wayland"));
        assert!(effective.grants("fallback-x11"));
        assert_eq!(
            effective
                .environment
                .get("ELECTRON_OZONE_PLATFORM_HINT")
                .map(String::as_str),
            Some("auto")
        );

        let layers = OverrideLayers {
            system_global: Some("[Environment]\nELECTRON_OZONE_PLATFORM_HINT=wayland\n".into()),
            system_app: Some("[Context]\nsockets=!x11;\n".into()),
            user_global: None,
            user_app: Some(
                "[Context]\nsockets=x11;!fallback-x11;\n\n[Environment]\nELECTRON_OZONE_PLATFORM_HINT=x11\n"
                    .into(),
            ),
        };
        let effective = app(ELECTRON_METADATA, layers).effective();
        assert!(effective.grants("x11"));
        assert!(!effective.grants("fallback-x11"));
        assert_eq!(effective.environment["ELECTRON_OZONE_PLATFORM_HINT"], "x11");
    }

    #[test]
    fn unset_environment_takes_a_variable_away() {
        let layers = OverrideLayers {
            user_app: Some("[Context]\nunset-environment=ELECTRON_OZONE_PLATFORM_HINT;\n".into()),
            ..OverrideLayers::default()
        };
        let effective = app(ELECTRON_METADATA, layers).effective();
        assert!(
            !effective
                .environment
                .contains_key("ELECTRON_OZONE_PLATFORM_HINT")
        );
    }
}
