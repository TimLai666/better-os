//! The one Better Screenshot process: what each start asks for, what it does
//! given what is already running, and when the process may end (the spec's
//! "One process, its names, and its life").

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::interface::Kept;

/// What a start of `better-screenshot` asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// Print: the interface.
    Interface,
    /// Ctrl+Shift+Alt+R: the interface in recording mode.
    Recording,
    /// Shift+Print: every output at once.
    Screen,
    /// Alt+Print: the focused window at once.
    Window,
    /// D-Bus activation for the portal: nothing but serving it.
    Portal,
}

impl Request {
    /// The flag that asks for it, as the key bindings run it.
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Request::Interface => None,
            Request::Recording => Some("--record"),
            Request::Screen => Some("--screen"),
            Request::Window => Some("--window"),
            Request::Portal => Some("--portal"),
        }
    }

    /// Reads the flags the command line accepted (the first one wins).
    pub fn from_arguments(arguments: &[String]) -> Request {
        for argument in arguments {
            match argument.as_str() {
                "--record" => return Request::Recording,
                "--screen" => return Request::Screen,
                "--window" => return Request::Window,
                "--portal" => return Request::Portal,
                _ => {}
            }
        }
        Request::Interface
    }

    /// The name the D-Bus method carries.
    pub fn name(self) -> &'static str {
        match self {
            Request::Interface => "interface",
            Request::Recording => "recording",
            Request::Screen => "screen",
            Request::Window => "window",
            Request::Portal => "portal",
        }
    }

    pub fn from_name(name: &str) -> Option<Request> {
        [
            Request::Interface,
            Request::Recording,
            Request::Screen,
            Request::Window,
            Request::Portal,
        ]
        .into_iter()
        .find(|request| request.name() == name)
    }
}

/// What the process does with a request, given what runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    OpenInterface,
    OpenRecordingInterface,
    TakeScreens,
    TakeFocusedWindow,
    StopRecording,
    /// Bring a fading interface back, in picture mode.
    Reopen,
    /// Bring a fading interface back, in recording mode.
    ReopenRecording,
    /// Already busy with the interface (or a portal call), refused while
    /// locked, or nothing to do.
    Nothing,
}

/// The interface's state, as a request sees it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Shown {
    #[default]
    Closed,
    /// Open, or being opened.
    Open,
    /// Fading out (O5): a new Print brings it back, as GNOME's `open` does
    /// "from in process of closing" (1579).
    Closing,
}

/// The session's lock, as logind reports it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lock {
    /// logind's `LockedHint`: a lock screen is up.
    pub locked: bool,
}

/// The decision:
///
/// - Print and the recording key stop a running recording (K3, R6).
/// - While the session is locked, the recording interface and the window
///   picture are refused, as GNOME binds them in `restrictedModes`
///   (1400–1404, 1414–1427) and allows no recording on its lock screen
///   (`allowScreencast` 1469–1473). The interface is refused too, where
///   GNOME's Print (`uiModes`, 1406–1412) opens it over its own lock screen:
///   COSMIC's lock is an `ext-session-lock` surface that hides every layer
///   surface and takes all input, so an interface there could be neither
///   seen nor used. Shift+Print stays, as GNOME's `screenshot` binding is in
///   `uiModes`.
/// - Nothing opens over an interface already open (`open` 1566); one
///   fading out comes back.
/// - The instant pictures wait for nothing.
pub fn action_for(request: Request, recording: bool, shown: Shown, lock: Lock) -> Action {
    match request {
        Request::Interface | Request::Recording if recording => Action::StopRecording,
        Request::Interface | Request::Recording | Request::Window if lock.locked => Action::Nothing,
        Request::Interface | Request::Recording if shown == Shown::Open => Action::Nothing,
        Request::Interface if shown == Shown::Closing => Action::Reopen,
        Request::Recording if shown == Shown::Closing => Action::ReopenRecording,
        Request::Interface => Action::OpenInterface,
        Request::Recording => Action::OpenRecordingInterface,
        Request::Screen => Action::TakeScreens,
        Request::Window => Action::TakeFocusedWindow,
        Request::Portal => Action::Nothing,
    }
}

/// What keeps the process alive.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Holds {
    pub interface_open: bool,
    pub recording: bool,
    /// It serves the clipboard's picture.
    pub clipboard: bool,
    /// A notification it raised is still shown, so its buttons still work.
    pub notifications: bool,
    /// A portal call is waiting for an answer.
    pub portal_pending: bool,
    /// A picture or a recording is being written.
    pub saving: bool,
}

impl Holds {
    /// Whether the process may end now.
    pub fn may_exit(&self) -> bool {
        !(self.interface_open
            || self.recording
            || self.clipboard
            || self.notifications
            || self.portal_pending
            || self.saving)
    }
}

/// What Better Screenshot keeps between runs within one login (spec C9,
/// P1): the area, the type, and the pointer switch, as GNOME keeps them for
/// as long as its shell runs. They live in the login's private runtime
/// directory (`$XDG_RUNTIME_DIR/better-os/screenshot/state.json`), which
/// logind removes at logout, so the next login starts as GNOME's would.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remembered {
    pub area: Option<Rect>,
    pub kept: Kept,
}

impl Remembered {
    /// Reads the file's text; anything unreadable is the defaults, because a
    /// lost area costs nothing but drawing it again.
    pub fn parse(text: &str) -> Remembered {
        serde_json::from_str(text).unwrap_or_default()
    }

    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Holds, Lock, Remembered, Request, Shown, action_for};
    use crate::geometry::Rect;
    use crate::interface::{Kept, Kind};

    #[test]
    fn flags_become_requests_and_back() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(Request::from_arguments(&args(&[])), Request::Interface);
        assert_eq!(
            Request::from_arguments(&args(&["--screen"])),
            Request::Screen
        );
        assert_eq!(
            Request::from_arguments(&args(&["--window"])),
            Request::Window
        );
        assert_eq!(
            Request::from_arguments(&args(&["--record"])),
            Request::Recording
        );
        assert_eq!(
            Request::from_arguments(&args(&["--portal"])),
            Request::Portal
        );
        for request in [
            Request::Interface,
            Request::Recording,
            Request::Screen,
            Request::Window,
            Request::Portal,
        ] {
            assert_eq!(Request::from_name(request.name()), Some(request));
            if let Some(flag) = request.flag() {
                assert_eq!(Request::from_arguments(&args(&[flag])), request);
            }
        }
        assert_eq!(Request::from_name("nonsense"), None);
    }

    #[test]
    fn the_keys_stop_a_recording_and_nothing_opens_twice() {
        let open =
            |request, recording, shown| action_for(request, recording, shown, Lock::default());
        assert_eq!(
            open(Request::Interface, false, Shown::Closed),
            Action::OpenInterface
        );
        assert_eq!(
            open(Request::Recording, false, Shown::Closed),
            Action::OpenRecordingInterface
        );
        assert_eq!(
            open(Request::Recording, true, Shown::Closed),
            Action::StopRecording
        );
        assert_eq!(
            open(Request::Interface, true, Shown::Closed),
            Action::StopRecording
        );
        assert_eq!(
            open(Request::Interface, false, Shown::Open),
            Action::Nothing
        );
        assert_eq!(
            open(Request::Screen, true, Shown::Closed),
            Action::TakeScreens
        );
        assert_eq!(
            open(Request::Window, false, Shown::Open),
            Action::TakeFocusedWindow
        );
        assert_eq!(open(Request::Portal, false, Shown::Closed), Action::Nothing);
    }

    #[test]
    fn print_during_the_fade_out_brings_the_interface_back() {
        let lock = Lock::default();
        assert_eq!(
            action_for(Request::Interface, false, Shown::Closing, lock),
            Action::Reopen
        );
        assert_eq!(
            action_for(Request::Recording, false, Shown::Closing, lock),
            Action::ReopenRecording
        );
    }

    #[test]
    fn a_locked_session_refuses_the_interface_the_recording_and_the_window() {
        let locked = Lock { locked: true };
        for request in [Request::Interface, Request::Recording, Request::Window] {
            assert_eq!(
                action_for(request, false, Shown::Closed, locked),
                Action::Nothing
            );
        }
        assert_eq!(
            action_for(Request::Screen, false, Shown::Closed, locked),
            Action::TakeScreens
        );
        // A running recording can still be stopped.
        assert_eq!(
            action_for(Request::Recording, true, Shown::Closed, locked),
            Action::StopRecording
        );
    }

    #[test]
    fn the_process_stays_while_anything_holds_it() {
        assert!(Holds::default().may_exit());
        for holds in [
            Holds {
                interface_open: true,
                ..Holds::default()
            },
            Holds {
                recording: true,
                ..Holds::default()
            },
            Holds {
                clipboard: true,
                ..Holds::default()
            },
            Holds {
                notifications: true,
                ..Holds::default()
            },
            Holds {
                portal_pending: true,
                ..Holds::default()
            },
            Holds {
                saving: true,
                ..Holds::default()
            },
        ] {
            assert!(!holds.may_exit(), "{holds:?}");
        }
    }

    #[test]
    fn what_is_remembered_survives_a_round_trip_and_junk_is_the_default() {
        let remembered = Remembered {
            area: Some(Rect::new(1, 2, 3, 4)),
            kept: Kept {
                kind: Kind::Screen,
                show_pointer: true,
            },
        };
        assert_eq!(Remembered::parse(&remembered.to_text()), remembered);
        assert_eq!(Remembered::parse("not json"), Remembered::default());
        assert_eq!(Remembered::parse("{\"area\":null}"), Remembered::default());
    }
}
