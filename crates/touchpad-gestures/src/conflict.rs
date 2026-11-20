//! What GNOME already does with these fingers.
//!
//! Issue #3 forbids replacing a desktop gesture silently, so a conflict has to
//! be a fact the preview screen can show rather than something discovered after
//! the fact by a user whose overview stopped opening. What is here is a
//! **static model** of GNOME 46's own touchpad gestures, and it is a model
//! rather than a probe for one honest reason: GNOME's swipe trackers are
//! compiled into the shell, not exposed as settings, so there is nothing to
//! read. Its accuracy is therefore a claim this repository makes, dated to the
//! release it names, and ADR 0012 records what would have to be re-checked when
//! the supported GNOME changes.
//!
//! A conflict is never resolved implicitly. [`Conflict`] carries no decision,
//! and `plan::PresetPlan` refuses to be approved while any conflict has no
//! [`ConflictResolution`] attached to it.

use serde::{Deserialize, Serialize};

use crate::config::GestureConfig;
use crate::definition::{ConflictState, Direction, GestureDefinition, GestureId, GestureShape};

/// One gesture the desktop already claims.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltInGesture {
    /// A stable machine key. Presentation layers word it.
    pub id: &'static str,
    pub shape: GestureShape,
    /// The contact counts the desktop accepts for this gesture.
    pub contacts: &'static [u8],
    /// The directions it responds to. Empty means direction does not matter.
    pub directions: &'static [Direction],
    /// What the desktop does with it, in machine-key form.
    pub does: &'static str,
}

impl BuiltInGesture {
    fn claims(&self, gesture: &GestureDefinition) -> bool {
        if self.shape != gesture.shape {
            return false;
        }
        if !self.contacts.contains(&gesture.contacts.get()) {
            return false;
        }
        match gesture.direction {
            None => self.directions.is_empty(),
            Some(direction) => self.directions.contains(&direction),
        }
    }
}

/// GNOME 46's shipped touchpad gestures.
///
/// Both of the shell's swipe trackers take three contacts and only three:
/// `js/ui/swipeTracker.js` in GNOME Shell 46.0 has `const
/// GESTURE_FINGER_COUNT = 3;` and its touchpad gesture returns early for any
/// other count (read from the shell's own gresource, 2026-11-21, ticket 131).
/// A four-finger swipe therefore reaches no GNOME gesture at all, and the
/// Mac-style preset, which swipes with four, collides with nothing. An earlier
/// model said "three and four" and made the service take GNOME's three-finger
/// gestures away for a preset that never used three fingers.
pub const GNOME_46_GESTURES: &[BuiltInGesture] = &[
    BuiltInGesture {
        id: "gnome.overview.swipe",
        shape: GestureShape::Swipe,
        contacts: &[3],
        directions: &[Direction::Up, Direction::Down],
        does: "gnome.overview_and_application_grid",
    },
    BuiltInGesture {
        id: "gnome.workspace.switch",
        shape: GestureShape::Swipe,
        contacts: &[3],
        directions: &[Direction::Left, Direction::Right],
        does: "gnome.switch_workspace",
    },
];

/// How a conflict is to be settled. Every option is an explicit choice; there
/// is no default, because a default is exactly the silent replacement Issue #3
/// forbids.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictResolution {
    /// Better Touchpad takes the gesture and the built-in one is turned off.
    ///
    /// No adapter in this build can turn a GNOME gesture off, so a plan
    /// carrying this resolution reports the built-in half as unsupported
    /// rather than claiming it happened.
    DisableBuiltIn,
    /// The desktop keeps it, and the Better Touchpad gesture is disabled
    /// instead. This is the choice that changes nothing about GNOME.
    KeepBuiltIn,
    /// Keep both by moving the Better Touchpad gesture somewhere the desktop
    /// does not look.
    RemapOurs { contacts: u8, direction: Direction },
}

impl ConflictResolution {
    pub fn key(self) -> &'static str {
        match self {
            Self::DisableBuiltIn => "disable-built-in",
            Self::KeepBuiltIn => "keep-built-in",
            Self::RemapOurs { .. } => "remap-ours",
        }
    }
}

/// One collision between a configured gesture and a built-in one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Conflict {
    pub gesture: GestureId,
    pub built_in: &'static str,
    pub built_in_does: &'static str,
    /// A suggestion, and only a suggestion. Nothing acts on it until the user
    /// chooses a resolution.
    pub suggested: ConflictResolution,
}

impl Conflict {
    pub fn state(&self) -> ConflictState {
        ConflictState::Conflicts {
            with: self.built_in.to_string(),
            detail: self.built_in_does.to_string(),
        }
    }
}

/// Every collision between a configuration and the desktop's own gestures.
///
/// Disabled gestures are checked too. A gesture that is off cannot collide
/// today, but the preview is about what applying the configuration *would* do,
/// and a row the user is about to enable should already say what it will hit.
pub fn detect(config: &GestureConfig, built_ins: &[BuiltInGesture]) -> Vec<Conflict> {
    config
        .gestures
        .iter()
        .filter_map(|gesture| {
            let built_in = built_ins.iter().find(|built_in| built_in.claims(gesture))?;
            Some(Conflict {
                gesture: gesture.id.clone(),
                built_in: built_in.id,
                built_in_does: built_in.does,
                // Keeping the desktop's own behaviour is the suggestion,
                // because it is the only one of the three that is certain to
                // leave the machine working.
                suggested: ConflictResolution::KeepBuiltIn,
            })
        })
        .collect()
}

/// Marks every gesture with what detection found, including the ones it
/// cleared. A gesture that was checked and found clear is a different state
/// from one that was never checked.
pub fn annotate(config: &mut GestureConfig, conflicts: &[Conflict]) {
    for gesture in &mut config.gestures {
        gesture.conflict = match conflicts
            .iter()
            .find(|conflict| conflict.gesture == gesture.id)
        {
            Some(conflict) => conflict.state(),
            None => ConflictState::Clear,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::mac_style;
    use better_actions::DesktopAction;

    fn conflicting_ids(conflicts: &[Conflict]) -> Vec<&str> {
        conflicts
            .iter()
            .map(|conflict| conflict.gesture.as_str())
            .collect()
    }

    /// The preset's real collisions on GNOME 46: none. Every swipe in it
    /// uses four fingers, and GNOME's trackers answer three alone.
    #[test]
    fn the_mac_style_preset_collides_with_nothing_gnome_46_does() {
        assert!(detect(&mac_style(), GNOME_46_GESTURES).is_empty());
    }

    #[test]
    fn gnome_46_answers_three_fingers_and_only_three() {
        for built_in in GNOME_46_GESTURES {
            assert_eq!(built_in.contacts, &[3], "{}", built_in.id);
        }
    }

    fn swipe_with(contacts: u8, direction: Direction) -> GestureConfig {
        let gesture = GestureDefinition::new(
            "swipe",
            GestureShape::Swipe,
            contacts,
            false,
            Some(direction),
            DesktopAction::ShowOverview,
        )
        .unwrap();
        GestureConfig::with_gestures(vec![gesture], crate::config::PresetId::Custom)
    }

    #[test]
    fn a_four_finger_swipe_in_any_direction_is_out_of_gnomes_reach() {
        for direction in [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ] {
            assert!(
                detect(&swipe_with(4, direction), GNOME_46_GESTURES).is_empty(),
                "{direction:?}"
            );
        }
    }

    #[test]
    fn a_three_finger_swipe_collides_with_the_tracker_for_its_direction() {
        for (direction, built_in) in [
            (Direction::Up, "gnome.overview.swipe"),
            (Direction::Down, "gnome.overview.swipe"),
            (Direction::Left, "gnome.workspace.switch"),
            (Direction::Right, "gnome.workspace.switch"),
        ] {
            let conflicts = detect(&swipe_with(3, direction), GNOME_46_GESTURES);
            assert_eq!(conflicts.len(), 1, "{direction:?}");
            assert_eq!(conflicts[0].built_in, built_in, "{direction:?}");
        }
    }

    #[test]
    fn the_pinch_and_spread_gestures_collide_with_nothing_gnome_does() {
        let conflicts = detect(&mac_style(), GNOME_46_GESTURES);
        for clear in ["launcher", "show-desktop", "app-zoom", "app-rotate"] {
            assert!(
                !conflicting_ids(&conflicts).contains(&clear),
                "{clear} was reported as a conflict"
            );
        }
    }

    #[test]
    fn a_two_finger_swipe_is_not_a_gnome_gesture() {
        let conflicts = detect(&mac_style(), GNOME_46_GESTURES);
        assert!(!conflicting_ids(&conflicts).contains(&"app-back"));
        assert!(!conflicting_ids(&conflicts).contains(&"app-forward"));
    }

    #[test]
    fn a_five_finger_swipe_is_out_of_gnomes_reach() {
        let five = GestureDefinition::new(
            "five-up",
            GestureShape::Swipe,
            5,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )
        .unwrap();
        let config = GestureConfig::with_gestures(vec![five], crate::config::PresetId::Custom);
        assert!(detect(&config, GNOME_46_GESTURES).is_empty());
    }

    #[test]
    fn a_disabled_gesture_is_still_reported_because_the_preview_is_about_what_would_happen() {
        let mut config = swipe_with(3, Direction::Up);
        for gesture in &mut config.gestures {
            gesture.enabled = false;
        }
        assert_eq!(detect(&config, GNOME_46_GESTURES).len(), 1);
    }

    #[test]
    fn annotation_tells_checked_and_clear_apart_from_never_checked() {
        let mut config = mac_style();
        let three = GestureDefinition::new(
            "three-up",
            GestureShape::Swipe,
            3,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )
        .unwrap();
        config.gestures.push(three);
        assert!(
            config
                .gestures
                .iter()
                .all(|gesture| gesture.conflict == ConflictState::Unknown)
        );
        let conflicts = detect(&config, GNOME_46_GESTURES);
        annotate(&mut config, &conflicts);

        let colliding = config.get(&conflicts[0].gesture).unwrap();
        assert!(colliding.conflict.conflicts());
        assert_eq!(
            config
                .gestures
                .iter()
                .find(|gesture| gesture.id.as_str() == "launcher")
                .unwrap()
                .conflict,
            ConflictState::Clear
        );
    }

    #[test]
    fn the_suggestion_is_the_one_that_leaves_the_desktop_alone() {
        let conflicts = detect(&swipe_with(3, Direction::Left), GNOME_46_GESTURES);
        assert!(!conflicts.is_empty());
        for conflict in conflicts {
            assert_eq!(conflict.suggested, ConflictResolution::KeepBuiltIn);
        }
    }
}
