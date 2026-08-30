//! The first-party **Mac-style gestures** preset.
//!
//! This is Issue #3's table, row for row, and the test below asserts it against
//! the table rather than against whatever the code happens to say. Two details
//! in it are decisions rather than transcription:
//!
//! - **The launcher and Show Desktop gestures use four contact points**, a
//!   thumb plus three fingers, not five fingers. Five-finger mappings remain
//!   available as custom gestures. `thumb_required` is what separates
//!   "thumb and three" from "four fingers", which matters because the preset
//!   contains both.
//! - **Four fingers left switches to the workspace on the right.** The content
//!   follows the fingers, which is what the natural-scrolling direction means
//!   applied to workspaces. Direction is configurable, and it is stored rather
//!   than compiled in, because that preference genuinely differs.
//!
//! The two-finger rows map to application-level actions where the application
//! and the backend support them. No adapter in this build supports any of them,
//! so they arrive in the plan preview as unsupported rather than as bindings
//! that quietly do nothing. How deep that support goes is one of Issue #3's
//! explicitly deferred decisions.

use better_actions::DesktopAction;

use crate::config::{GestureConfig, PresetId};
use crate::definition::{Direction, GestureDefinition, GestureError, GestureShape};

/// Issue #3's Mac-style mapping.
pub fn mac_style() -> GestureConfig {
    GestureConfig::with_gestures(
        mac_style_gestures().expect("the shipped preset is valid"),
        PresetId::MacStyle,
    )
}

fn mac_style_gestures() -> Result<Vec<GestureDefinition>, GestureError> {
    Ok(vec![
        // Thumb + three fingers pinch inward → open Better Launcher.
        GestureDefinition::new(
            "launcher",
            GestureShape::Pinch,
            4,
            true,
            None,
            DesktopAction::LauncherOpen,
        )?,
        // Thumb + three fingers spread outward → Show Desktop.
        GestureDefinition::new(
            "show-desktop",
            GestureShape::Spread,
            4,
            true,
            None,
            DesktopAction::ShowDesktop,
        )?,
        // Four fingers up → Overview.
        GestureDefinition::new(
            "overview",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )?,
        // Four fingers down → the current application's windows.
        GestureDefinition::new(
            "current-app-windows",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Down),
            DesktopAction::CurrentApplicationWindows,
        )?,
        // Four fingers left → the workspace on the right.
        GestureDefinition::new(
            "workspace-next",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Left),
            DesktopAction::NextWorkspace,
        )?,
        // Four fingers right → the workspace on the left.
        GestureDefinition::new(
            "workspace-previous",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Right),
            DesktopAction::PreviousWorkspace,
        )?,
        // Two fingers horizontally → application back and forward. One row of
        // the issue's table needs two gestures, because a gesture carries one
        // direction and one action.
        GestureDefinition::new(
            "app-back",
            GestureShape::Swipe,
            2,
            false,
            Some(Direction::Right),
            DesktopAction::ApplicationBack,
        )?,
        GestureDefinition::new(
            "app-forward",
            GestureShape::Swipe,
            2,
            false,
            Some(Direction::Left),
            DesktopAction::ApplicationForward,
        )?,
        // Two fingers pinch → application zoom.
        GestureDefinition::new(
            "app-zoom",
            GestureShape::Pinch,
            2,
            false,
            None,
            DesktopAction::ApplicationZoom,
        )?,
        // Two fingers rotate → application rotate.
        GestureDefinition::new(
            "app-rotate",
            GestureShape::Rotate,
            2,
            false,
            Some(Direction::Clockwise),
            DesktopAction::ApplicationRotate,
        )?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{AnimationProgress, Threshold};

    /// Issue #3's table, written out again so the assertion is against the
    /// issue rather than against the code that implements it.
    struct Row {
        id: &'static str,
        shape: GestureShape,
        contacts: u8,
        thumb: bool,
        direction: Option<Direction>,
        action: &'static str,
    }

    const fn row(
        id: &'static str,
        shape: GestureShape,
        contacts: u8,
        thumb: bool,
        direction: Option<Direction>,
        action: &'static str,
    ) -> Row {
        Row {
            id,
            shape,
            contacts,
            thumb,
            direction,
            action,
        }
    }

    const TABLE: &[Row] = &[
        row(
            "launcher",
            GestureShape::Pinch,
            4,
            true,
            None,
            "better-launcher.open",
        ),
        row(
            "show-desktop",
            GestureShape::Spread,
            4,
            true,
            None,
            "desktop.show",
        ),
        row(
            "overview",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Up),
            "overview.show",
        ),
        row(
            "current-app-windows",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Down),
            "current-app.windows",
        ),
        row(
            "workspace-next",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Left),
            "workspace.next",
        ),
        row(
            "workspace-previous",
            GestureShape::Swipe,
            4,
            false,
            Some(Direction::Right),
            "workspace.previous",
        ),
        row(
            "app-back",
            GestureShape::Swipe,
            2,
            false,
            Some(Direction::Right),
            "app.back",
        ),
        row(
            "app-forward",
            GestureShape::Swipe,
            2,
            false,
            Some(Direction::Left),
            "app.forward",
        ),
        row("app-zoom", GestureShape::Pinch, 2, false, None, "app.zoom"),
        row(
            "app-rotate",
            GestureShape::Rotate,
            2,
            false,
            Some(Direction::Clockwise),
            "app.rotate",
        ),
    ];

    #[test]
    fn the_preset_is_the_table_issue_three_specifies() {
        let preset = mac_style();
        assert_eq!(preset.gestures.len(), TABLE.len());
        for (index, expected) in TABLE.iter().enumerate() {
            let gesture = &preset.gestures[index];
            let id = expected.id;
            assert_eq!(gesture.id.as_str(), id);
            assert_eq!(gesture.shape, expected.shape, "{id}");
            assert_eq!(gesture.contacts.get(), expected.contacts, "{id}");
            assert_eq!(gesture.thumb_required, expected.thumb, "{id}");
            assert_eq!(gesture.direction, expected.direction, "{id}");
            assert_eq!(gesture.action.key(), expected.action, "{id}");
            assert!(gesture.enabled, "{id}");
        }
    }

    #[test]
    fn the_launcher_and_show_desktop_gestures_use_a_thumb_and_three_fingers() {
        let preset = mac_style();
        for (id, shape) in [
            ("launcher", GestureShape::Pinch),
            ("show-desktop", GestureShape::Spread),
        ] {
            let gesture = preset
                .gestures
                .iter()
                .find(|gesture| gesture.id.as_str() == id)
                .unwrap();
            assert_eq!(gesture.contacts.get(), 4, "{id} must be four contacts");
            assert!(gesture.thumb_required, "{id} must require the thumb");
            assert_eq!(gesture.shape, shape);
        }
    }

    #[test]
    fn no_five_finger_gesture_is_the_default_for_the_launcher_or_show_desktop() {
        // Five-finger mappings stay available as custom gestures; the preset
        // simply does not use one, which is the decision Issue #3 records.
        assert!(
            mac_style()
                .gestures
                .iter()
                .all(|gesture| gesture.contacts.get() != 5)
        );
    }

    #[test]
    fn the_preset_validates_and_every_identity_is_distinct() {
        let preset = mac_style();
        preset.validate().unwrap();
        assert_eq!(preset.preset, PresetId::MacStyle);
        assert_eq!(GestureConfig::from_json(&preset.to_json()).unwrap(), preset);
    }

    #[test]
    fn every_preset_gesture_ships_the_recorded_starting_thresholds() {
        for gesture in mac_style().gestures {
            assert_eq!(
                gesture.activation_threshold,
                Threshold::new(GestureDefinition::DEFAULT_ACTIVATION).unwrap(),
                "{}",
                gesture.id
            );
            assert_eq!(
                gesture.cancellation_threshold,
                Threshold::new(GestureDefinition::DEFAULT_CANCELLATION).unwrap(),
                "{}",
                gesture.id
            );
            assert_eq!(
                gesture.cooldown.as_millis(),
                GestureDefinition::DEFAULT_COOLDOWN_MS,
                "{}",
                gesture.id
            );
            assert_eq!(gesture.animation_progress, AnimationProgress::WhenAvailable);
        }
    }

    #[test]
    fn the_four_gestures_that_should_animate_can() {
        let preset = mac_style();
        for id in [
            "launcher",
            "show-desktop",
            "overview",
            "current-app-windows",
        ] {
            assert!(
                preset
                    .gestures
                    .iter()
                    .find(|gesture| gesture.id.as_str() == id)
                    .unwrap()
                    .can_animate(),
                "{id} cannot follow the fingers"
            );
        }
    }
}
