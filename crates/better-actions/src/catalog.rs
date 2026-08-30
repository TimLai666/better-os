//! The closed catalog of desktop actions.
//!
//! Every component that asks the desktop to do something — a gesture, a
//! shortcut, a menu item — names one of these. The enum is closed on purpose:
//! there is no `Command`, no `Exec`, and no free-text variant, so no
//! configuration file, no D-Bus message, and no GUI field can express "run
//! this". Issue #3 puts arbitrary shell execution out of scope; making it
//! unrepresentable is stronger than checking for it.
//!
//! An action being in the catalog is not a claim that any desktop can perform
//! it. That is what [`crate::support`] is for: an adapter reports, per action,
//! whether it can do it and whether it can follow a gesture's progress while
//! doing it.

use serde::{Deserialize, Serialize};

use crate::key::{Key, KeyboardShortcut, Modifier};

/// Everything Better OS can ask the desktop to do.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum DesktopAction {
    /// Open the Better Launcher overlay.
    LauncherOpen,
    /// Close it again.
    LauncherClose,
    /// Show the desktop by minimising everything.
    ShowDesktop,
    /// The workspace overview.
    ShowOverview,
    /// The windows belonging to the focused application.
    CurrentApplicationWindows,
    NextWorkspace,
    PreviousWorkspace,
    /// Move to the next application. The exact mapping is one of Issue #3's
    /// deferred decisions; the catalog only has to be able to say it.
    NextApplication,
    PreviousApplication,
    /// Application-level navigation, for the two-finger gestures. These reach
    /// the focused application rather than the shell, which is why an adapter
    /// is expected to report most of them unsupported.
    ApplicationBack,
    ApplicationForward,
    ApplicationZoom,
    ApplicationRotate,
    MediaPlayPause,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    /// A shortcut the user chose, built only from a validated modifier set and
    /// a key from a fixed table.
    KeyboardShortcut {
        shortcut: KeyboardShortcut,
    },
    /// Bound to nothing. A gesture set to this is inert rather than absent, so
    /// the row stays on screen and can be given an action again.
    Disabled,
}

impl DesktopAction {
    /// A stable machine key. Reports, stored configuration, and logs carry
    /// this; the wording belongs to whatever is drawing it.
    pub fn key(&self) -> &'static str {
        match self {
            Self::LauncherOpen => "better-launcher.open",
            Self::LauncherClose => "better-launcher.close",
            Self::ShowDesktop => "desktop.show",
            Self::ShowOverview => "overview.show",
            Self::CurrentApplicationWindows => "current-app.windows",
            Self::NextWorkspace => "workspace.next",
            Self::PreviousWorkspace => "workspace.previous",
            Self::NextApplication => "app.next",
            Self::PreviousApplication => "app.previous",
            Self::ApplicationBack => "app.back",
            Self::ApplicationForward => "app.forward",
            Self::ApplicationZoom => "app.zoom",
            Self::ApplicationRotate => "app.rotate",
            Self::MediaPlayPause => "media.play-pause",
            Self::VolumeUp => "volume.up",
            Self::VolumeDown => "volume.down",
            Self::VolumeMute => "volume.mute",
            Self::KeyboardShortcut { .. } => "shortcut.custom",
            Self::Disabled => "none",
        }
    }

    /// Every action, each one represented once. The custom shortcut appears
    /// with a placeholder binding, because a picker has to offer the row before
    /// the user has chosen the keys.
    pub fn catalog() -> Vec<Self> {
        let mut actions = vec![
            Self::LauncherOpen,
            Self::LauncherClose,
            Self::ShowDesktop,
            Self::ShowOverview,
            Self::CurrentApplicationWindows,
            Self::NextWorkspace,
            Self::PreviousWorkspace,
            Self::NextApplication,
            Self::PreviousApplication,
            Self::ApplicationBack,
            Self::ApplicationForward,
            Self::ApplicationZoom,
            Self::ApplicationRotate,
            Self::MediaPlayPause,
            Self::VolumeUp,
            Self::VolumeDown,
            Self::VolumeMute,
        ];
        actions.push(Self::KeyboardShortcut {
            shortcut: Self::placeholder_shortcut(),
        });
        actions.push(Self::Disabled);
        actions
    }

    /// The binding a freshly chosen custom shortcut starts from.
    pub fn placeholder_shortcut() -> KeyboardShortcut {
        KeyboardShortcut::new(
            [Modifier::Super, Modifier::Alt],
            Key::parse("g").expect("g is in the key table"),
        )
        .expect("two modifiers is at least one")
    }

    /// Whether this action can meaningfully follow a gesture's progress rather
    /// than only firing once.
    ///
    /// This is the action's own half of the animation question. Whether the
    /// adapter can deliver progress is the adapter's half, and both have to be
    /// true before a gesture animates.
    pub fn follows_progress(&self) -> bool {
        matches!(
            self,
            Self::LauncherOpen
                | Self::LauncherClose
                | Self::ShowDesktop
                | Self::ShowOverview
                | Self::CurrentApplicationWindows
                | Self::NextWorkspace
                | Self::PreviousWorkspace
                | Self::ApplicationZoom
                | Self::ApplicationRotate
        )
    }

    /// Whether this action changes anything at all.
    pub fn changes_something(&self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_catalog_holds_every_action_issue_three_requires() {
        let keys: BTreeSet<&'static str> = DesktopAction::catalog()
            .iter()
            .map(DesktopAction::key)
            .collect();
        for required in [
            "better-launcher.open",
            "better-launcher.close",
            "desktop.show",
            "overview.show",
            "current-app.windows",
            "workspace.next",
            "workspace.previous",
            "app.next",
            "app.previous",
            "media.play-pause",
            "volume.up",
            "volume.down",
            "volume.mute",
            "shortcut.custom",
            "none",
        ] {
            assert!(keys.contains(required), "the catalog is missing {required}");
        }
    }

    #[test]
    fn every_action_has_its_own_key_and_the_catalog_lists_each_one_once() {
        let catalog = DesktopAction::catalog();
        let keys: BTreeSet<&'static str> = catalog.iter().map(DesktopAction::key).collect();
        assert_eq!(keys.len(), catalog.len());
    }

    #[test]
    fn no_stored_action_can_carry_a_shell_command() {
        // Every shape a configuration file could try. The tag is closed, so an
        // unknown one is refused; the only payload any variant carries is a
        // shortcut, and that is validated on the way in.
        for attempt in [
            r#"{"action":"shell","command":"rm -rf ~"}"#,
            r#"{"action":"exec","command":"sh -c id"}"#,
            r#"{"action":"keyboard-shortcut","shortcut":"pkill -9 gnome-shell"}"#,
            r#"{"action":"keyboard-shortcut","shortcut":"<Super>;id"}"#,
        ] {
            assert!(
                serde_json::from_str::<DesktopAction>(attempt).is_err(),
                "{attempt} was accepted"
            );
        }

        // An extra field is the one case serde does not refuse — an internally
        // tagged enum ignores what it does not know. That is harmless here for
        // the reason the whole design rests on: the variant it produces has
        // nowhere to put the smuggled text, so it is dropped rather than
        // carried.
        let parsed: DesktopAction =
            serde_json::from_str(r#"{"action":"launcher-open","command":"id"}"#).unwrap();
        assert_eq!(parsed, DesktopAction::LauncherOpen);
        assert!(!serde_json::to_string(&parsed).unwrap().contains("id"));
    }

    #[test]
    fn a_valid_action_round_trips_through_json() {
        for action in DesktopAction::catalog() {
            let text = serde_json::to_string(&action).unwrap();
            assert_eq!(
                serde_json::from_str::<DesktopAction>(&text).unwrap(),
                action,
                "{text} did not round trip"
            );
        }
    }

    #[test]
    fn only_the_actions_a_progress_stream_could_drive_claim_to_follow_it() {
        assert!(DesktopAction::LauncherOpen.follows_progress());
        assert!(DesktopAction::ShowOverview.follows_progress());
        assert!(!DesktopAction::VolumeUp.follows_progress());
        assert!(!DesktopAction::MediaPlayPause.follows_progress());
        assert!(!DesktopAction::Disabled.follows_progress());
        assert!(!DesktopAction::Disabled.changes_something());
    }

    #[test]
    fn the_shipped_source_never_names_a_way_to_run_a_program() {
        // The guarantee is structural — the enum has no command variant — but
        // asserting it over the source as well catches the change that would
        // add one back in a helper rather than in the enum.
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(&source).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text: String = std::fs::read_to_string(&path)
                .unwrap()
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                // The test module names these deliberately.
                .take_while(|line| !line.contains("mod tests"))
                .collect::<Vec<_>>()
                .join("\n");
            for forbidden in ["Command::new", "process::Command", "sh -c", "exec("] {
                assert!(
                    !text.contains(forbidden),
                    "{} names {forbidden}",
                    path.display()
                );
            }
        }
    }
}
