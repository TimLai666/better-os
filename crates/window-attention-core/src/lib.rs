//! What happens when a window asks for attention, decided per application.
//!
//! GNOME Shell turns every attention request into a "“…” is ready" banner.
//! Better OS lets the person choose, for every application and for each one
//! separately, one of four outcomes (ticket 106). This crate is that choice and
//! the decision made from it, with no bus, no GPUI, and no shell in sight:
//!
//! - [`Choice`] is the closed set of four outcomes.
//! - [`Choices`] is what Better Manager writes: a default and an override per
//!   application.
//! - [`SeenApps`] is what the service writes: every application that has asked
//!   for attention, so the page can list it.
//! - [`decide`] turns one request the shell reported into the one action the
//!   shell is asked to perform.
//!
//! The GNOME Shell extension that reports the requests and performs the
//! actions holds none of this (ADR 0018). [`bus`] names the contract the two
//! halves speak.

pub mod bus;
pub mod store;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// What happens when an application's window asks for attention.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// GNOME's own behaviour: a banner, an entry in the notification list,
    /// and the taskbar's mark.
    BannerAndMark,
    /// The taskbar's mark only, with no banner and nothing in the
    /// notification list. The request stays on the window until the window
    /// is focused, and the application's taskbar icon is marked urgent through
    /// the Unity launcher API, which is what Zorin's taskbar (and Dash to
    /// Panel and Dash to Dock) draw a mark from; the taskbar clears it when
    /// the application is focused.
    #[default]
    MarkOnly,
    /// The request is ignored: no banner, and the mark is cleared at once.
    Nothing,
    /// The window comes forward, as if the person had clicked it.
    BringForward,
}

impl Choice {
    /// Every choice, in the order the page offers them.
    pub const ALL: [Choice; 4] = [
        Choice::BannerAndMark,
        Choice::MarkOnly,
        Choice::Nothing,
        Choice::BringForward,
    ];
}

/// The default every application follows until it has a choice of its own.
pub const DEFAULT_CHOICE: Choice = Choice::MarkOnly;

/// The longest application id kept. A desktop file id is a file name, and
/// Linux file names stop at 255 bytes.
pub const MAX_APP_ID_BYTES: usize = 255;

/// The longest application name kept, in characters. Longer is cut.
pub const MAX_APP_NAME_CHARS: usize = 200;

/// An application, named by its desktop file id as GNOME Shell's window
/// tracker reports it, for example `org.gnome.Nautilus.desktop`.
///
/// Only an id that names a desktop file is an application. The tracker gives a
/// window it cannot match a made-up id (`window:42`), which names nothing a
/// person could recognise or choose again, so such a window follows the
/// default and is never recorded.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AppId(String);

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AppIdError {
    #[error("an application id must name a .desktop file")]
    NotADesktopId,
    #[error("an application id is at most {MAX_APP_ID_BYTES} bytes")]
    TooLong,
}

impl AppId {
    pub fn new(value: impl Into<String>) -> Result<Self, AppIdError> {
        let value = value.into();
        if value.len() > MAX_APP_ID_BYTES {
            return Err(AppIdError::TooLong);
        }
        let stem = value
            .strip_suffix(".desktop")
            .ok_or(AppIdError::NotADesktopId)?;
        let allowed = |character: char| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+')
        };
        if stem.is_empty() || !stem.chars().all(allowed) {
            return Err(AppIdError::NotADesktopId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AppId {
    type Error = AppIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<AppId> for String {
    fn from(value: AppId) -> Self {
        value.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// An application name as it may be shown and stored: control characters
/// dropped, surrounding space trimmed, and cut at [`MAX_APP_NAME_CHARS`].
pub fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_APP_NAME_CHARS)
        .collect()
}

/// What Better Manager writes: the default, and each application's own choice.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Choices {
    pub version: u32,
    #[serde(default)]
    pub default: Choice,
    /// Applications with a choice of their own.
    #[serde(default)]
    pub apps: BTreeMap<AppId, AppChoice>,
}

/// One application's own choice, with the name it had when it was chosen so
/// the page can show it after the application is gone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppChoice {
    pub choice: Choice,
    #[serde(default)]
    pub name: String,
}

/// The schema version [`Choices`] is written at.
pub const CHOICES_VERSION: u32 = 1;

impl Default for Choices {
    fn default() -> Self {
        Self {
            version: CHOICES_VERSION,
            default: DEFAULT_CHOICE,
            apps: BTreeMap::new(),
        }
    }
}

impl Choices {
    /// No application with a choice of its own, and this default.
    pub fn with_default(default: Choice) -> Self {
        Self {
            default,
            ..Self::default()
        }
    }

    /// The choice that applies to an application: its own when it has one,
    /// the default otherwise.
    pub fn for_app(&self, app: Option<&AppId>) -> Choice {
        app.and_then(|app| self.apps.get(app))
            .map(|own| own.choice)
            .unwrap_or(self.default)
    }

    /// Gives an application a choice of its own.
    pub fn set(&mut self, app: AppId, name: &str, choice: Choice) {
        self.apps.insert(
            app,
            AppChoice {
                choice,
                name: clean_name(name),
            },
        );
    }

    /// Takes an application's own choice away, so it follows the default
    /// again. Returns whether it had one.
    pub fn remove(&mut self, app: &AppId) -> bool {
        self.apps.remove(app).is_some()
    }
}

/// What the service writes: every application that has asked for attention.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeenApps {
    pub version: u32,
    #[serde(default)]
    pub apps: BTreeMap<AppId, SeenApp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeenApp {
    pub name: String,
    /// When it first asked, in seconds since the epoch.
    pub first_seen: u64,
}

/// The schema version [`SeenApps`] is written at.
pub const SEEN_VERSION: u32 = 1;

/// How many applications the seen list keeps. Past this the oldest is
/// forgotten, so a stream of made-up ids cannot grow the file without end.
pub const MAX_SEEN_APPS: usize = 512;

impl SeenApps {
    /// Records that an application asked. Returns whether anything changed,
    /// which is when the file needs writing: a new application, or a known one
    /// whose name changed.
    pub fn record(&mut self, app: &AppId, name: &str, now: u64) -> bool {
        let name = clean_name(name);
        if let Some(seen) = self.apps.get_mut(app) {
            if seen.name == name || name.is_empty() {
                return false;
            }
            seen.name = name;
            return true;
        }
        if self.apps.len() >= MAX_SEEN_APPS {
            let oldest = self
                .apps
                .iter()
                .min_by_key(|(_, seen)| seen.first_seen)
                .map(|(id, _)| id.clone());
            if let Some(oldest) = oldest {
                self.apps.remove(&oldest);
            }
        }
        self.apps.insert(
            app.clone(),
            SeenApp {
                name,
                first_seen: now,
            },
        );
        self.version = SEEN_VERSION;
        true
    }
}

/// Which of the shell's two signals a request came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestKind {
    /// `window-demands-attention`: a Wayland activation request, or
    /// `_NET_WM_STATE_DEMANDS_ATTENTION` on X11.
    DemandsAttention,
    /// `window-marked-urgent`: the X11 urgency hint.
    MarkedUrgent,
}

impl RequestKind {
    /// The value on the wire ([`bus::WINDOW_ATTENTION_SIGNAL`]).
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::DemandsAttention),
            1 => Some(Self::MarkedUrgent),
            _ => None,
        }
    }
}

/// One attention request, exactly as the extension reported it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionRequest {
    /// The shell's own id for the window (`Meta.Window.get_id()`).
    pub window: u64,
    /// The window tracker's application id, which may be a made-up one.
    pub app_id: String,
    pub app_name: String,
    pub kind: RequestKind,
    /// The window already has the keyboard.
    pub focused: bool,
    /// The window is not in the taskbar (a toolbar, a splash screen).
    pub skip_taskbar: bool,
}

/// The one thing the shell is asked to do about a request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellAction {
    /// Nothing at all: the request stays on the window as it is.
    Leave,
    /// The request stays on the window and the application's taskbar icon is
    /// marked urgent.
    MarkTaskbar(AppId),
    /// GNOME's own banner, made by GNOME's own handler.
    ShowBanner(u64),
    /// Clear the request, so nothing marks the window.
    Clear(u64),
    /// Bring the window forward.
    Activate(u64),
}

/// What to do about one request, and the application to record, if any.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    pub action: ShellAction,
    pub record: Option<(AppId, String)>,
}

/// Decides what one request does.
///
/// A focused window and a window that is not in the taskbar are left as they
/// are and nothing is recorded, which is what GNOME's own handler does with
/// them: a focused window has nothing to ask for, and some applications mark
/// their toolbars urgent while they themselves have focus.
pub fn decide(choices: &Choices, request: &AttentionRequest) -> Decision {
    if request.focused || request.skip_taskbar {
        return Decision {
            action: ShellAction::Leave,
            record: None,
        };
    }
    let app = AppId::new(request.app_id.clone()).ok();
    let action = match choices.for_app(app.as_ref()) {
        Choice::BannerAndMark => ShellAction::ShowBanner(request.window),
        // A window the tracker could not match has no taskbar icon to mark;
        // the request on the window is all there is.
        Choice::MarkOnly => match &app {
            Some(app) => ShellAction::MarkTaskbar(app.clone()),
            None => ShellAction::Leave,
        },
        Choice::Nothing => ShellAction::Clear(request.window),
        Choice::BringForward => ShellAction::Activate(request.window),
    };
    Decision {
        action,
        record: app.map(|app| (app, clean_name(&request.app_name))),
    }
}

#[cfg(test)]
mod tests;
