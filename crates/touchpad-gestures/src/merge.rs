//! Writing one's own changes onto the gesture file as it is now (ticket 131).
//!
//! Two processes write `gestures.json`: a window, when the person changes
//! something, and the gesture service, when it verifies, corrects a conflict
//! annotation, or turns gestures off after a failing adapter. Each holds a copy
//! it read earlier. Writing that whole copy back loses whatever the other wrote
//! in between: a window saving an edit after the service's automatic disable
//! turned gestures back on while the "turned off" marker still said off.
//!
//! So neither writes its copy. Each keeps the document as it last read it (the
//! *base*), and writes only what differs between that and its own copy (the
//! *ours*), onto the file as it is now (the *theirs*), with [`merge`]. The
//! read, the merge, and the write happen under one lock both writers take
//! ([`crate::GestureStore::update_profiles`]), so nothing lands in between.
//!
//! The merge is field by field. A profile's switch and preset, each gesture
//! as a definition (what the person chose), and each gesture's findings (its
//! verification result and conflict annotation, which the service and a
//! window's own verification write) are each taken from ours when ours changed
//! them and from theirs otherwise. A gesture or a device profile that ours
//! added or removed is added or removed; one theirs added or removed and ours
//! did not touch stays as theirs has it.

use crate::config::GestureConfig;
use crate::definition::{ConflictState, GestureDefinition, VerificationRecord};
use crate::profiles::GestureProfiles;

/// `theirs`, with every change `ours` made to `base` applied.
pub fn merge(
    base: &GestureProfiles,
    ours: &GestureProfiles,
    theirs: &GestureProfiles,
) -> GestureProfiles {
    let mut merged = theirs.clone();
    if ours.active_device != base.active_device {
        merged.active_device = ours.active_device.clone();
    }
    merged.global = merge_config(&base.global, &ours.global, &theirs.global);

    let identities: std::collections::BTreeSet<&String> =
        base.devices.keys().chain(ours.devices.keys()).collect();
    for identity in identities {
        match (
            base.devices.get(identity),
            ours.devices.get(identity),
            theirs.devices.get(identity),
        ) {
            // Ours added it.
            (None, Some(added), _) => {
                merged.devices.insert(identity.clone(), added.clone());
            }
            // Ours removed it.
            (Some(_), None, _) => {
                merged.devices.remove(identity);
            }
            (Some(before), Some(after), Some(now)) => {
                merged
                    .devices
                    .insert(identity.clone(), merge_config(before, after, now));
            }
            // Theirs removed it: it stays removed unless ours changed it.
            (Some(before), Some(after), None) => {
                if after != before {
                    merged.devices.insert(identity.clone(), after.clone());
                }
            }
            (None, None, _) => {}
        }
    }
    merged
}

fn merge_config(
    base: &GestureConfig,
    ours: &GestureConfig,
    theirs: &GestureConfig,
) -> GestureConfig {
    let mut merged = theirs.clone();
    if ours.enabled != base.enabled {
        merged.enabled = ours.enabled;
    }
    if ours.preset != base.preset {
        merged.preset = ours.preset;
    }
    if ours.schema_version != base.schema_version {
        merged.schema_version = ours.schema_version;
    }

    let mut ids: Vec<&crate::definition::GestureId> = Vec::new();
    for gesture in base.gestures.iter().chain(ours.gestures.iter()) {
        if !ids.contains(&&gesture.id) {
            ids.push(&gesture.id);
        }
    }
    for id in ids {
        match (base.get(id), ours.get(id), theirs.get(id)) {
            (None, Some(added), _) => replace_or_push(&mut merged, added.clone()),
            (Some(_), None, _) => merged.gestures.retain(|gesture| &gesture.id != id),
            (Some(before), Some(after), Some(now)) => {
                replace_or_push(&mut merged, merge_gesture(before, after, now));
            }
            (Some(before), Some(after), None) => {
                if definition(after) != definition(before) {
                    merged.gestures.push(after.clone());
                }
            }
            (None, None, _) => {}
        }
    }
    merged
}

fn merge_gesture(
    base: &GestureDefinition,
    ours: &GestureDefinition,
    theirs: &GestureDefinition,
) -> GestureDefinition {
    let mut merged = if definition(ours) != definition(base) {
        ours.clone()
    } else {
        theirs.clone()
    };
    let (conflict, verification) = if findings(ours) != findings(base) {
        (ours.conflict.clone(), ours.last_verification.clone())
    } else {
        (theirs.conflict.clone(), theirs.last_verification.clone())
    };
    merged.conflict = conflict;
    merged.last_verification = verification;
    merged
}

fn replace_or_push(config: &mut GestureConfig, gesture: GestureDefinition) {
    match config
        .gestures
        .iter_mut()
        .find(|slot| slot.id == gesture.id)
    {
        Some(slot) => *slot = gesture,
        None => config.gestures.push(gesture),
    }
}

/// What the person chose: the gesture with its findings left out.
fn definition(gesture: &GestureDefinition) -> GestureDefinition {
    let mut chosen = gesture.clone();
    chosen.conflict = ConflictState::Unknown;
    chosen.last_verification = VerificationRecord::NotRun;
    chosen
}

fn findings(gesture: &GestureDefinition) -> (&ConflictState, &VerificationRecord) {
    (&gesture.conflict, &gesture.last_verification)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{Direction, GestureShape};
    use crate::preset::mac_style;
    use better_actions::DesktopAction;

    fn document(config: GestureConfig) -> GestureProfiles {
        GestureProfiles::global_only(config)
    }

    fn mine() -> GestureDefinition {
        GestureDefinition::new(
            "mine",
            GestureShape::Swipe,
            5,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )
        .unwrap()
    }

    /// The defect the review found: the service turned gestures off, and a
    /// window that read the file before that saved an edit. The edit lands and
    /// the switch stays off.
    #[test]
    fn an_edit_from_a_stale_copy_keeps_the_switch_another_writer_turned_off() {
        let base = document(mac_style());
        let mut ours = base.clone();
        ours.global.gestures.push(mine());
        let mut theirs = base.clone();
        theirs.global.enabled = false;

        let merged = merge(&base, &ours, &theirs);
        assert!(!merged.global.enabled);
        assert!(merged.global.get(&mine().id).is_some());
    }

    /// The service writes findings and nothing else; a gesture the window
    /// added meanwhile survives, and the findings land on the gestures that
    /// exist.
    #[test]
    fn findings_from_one_writer_and_a_new_gesture_from_the_other_both_land() {
        let base = document(mac_style());
        let mut ours = base.clone();
        for gesture in &mut ours.global.gestures {
            gesture.last_verification = VerificationRecord::Verified {
                continuous_progress: false,
            };
        }
        let mut theirs = base.clone();
        theirs.global.gestures.push(mine());

        let merged = merge(&base, &ours, &theirs);
        assert!(merged.global.get(&mine().id).is_some());
        let overview = merged
            .global
            .get(&crate::definition::GestureId::new("overview").unwrap())
            .unwrap();
        assert!(matches!(
            overview.last_verification,
            VerificationRecord::Verified { .. }
        ));
    }

    #[test]
    fn a_definition_ours_changed_wins_and_one_it_did_not_change_follows_theirs() {
        let base = document(mac_style());
        let overview = crate::definition::GestureId::new("overview").unwrap();
        let launcher = crate::definition::GestureId::new("launcher").unwrap();
        let mut ours = base.clone();
        ours.global.get_mut(&overview).unwrap().enabled = false;
        let mut theirs = base.clone();
        theirs.global.get_mut(&launcher).unwrap().enabled = false;

        let merged = merge(&base, &ours, &theirs);
        assert!(!merged.global.get(&overview).unwrap().enabled);
        assert!(!merged.global.get(&launcher).unwrap().enabled);
    }

    #[test]
    fn a_gesture_ours_removed_goes_and_one_theirs_removed_stays_gone() {
        let base = document(mac_style());
        let overview = crate::definition::GestureId::new("overview").unwrap();
        let launcher = crate::definition::GestureId::new("launcher").unwrap();
        let mut ours = base.clone();
        ours.global
            .gestures
            .retain(|gesture| gesture.id != overview);
        let mut theirs = base.clone();
        theirs
            .global
            .gestures
            .retain(|gesture| gesture.id != launcher);

        let merged = merge(&base, &ours, &theirs);
        assert!(merged.global.get(&overview).is_none());
        assert!(merged.global.get(&launcher).is_none());
    }

    #[test]
    fn a_device_profile_ours_added_lands_and_one_theirs_added_stays() {
        let base = document(mac_style());
        let mut ours = base.clone();
        ours.devices.insert("pad-a".to_string(), mac_style());
        let mut theirs = base.clone();
        theirs.devices.insert("pad-b".to_string(), mac_style());

        let merged = merge(&base, &ours, &theirs);
        assert!(merged.devices.contains_key("pad-a"));
        assert!(merged.devices.contains_key("pad-b"));
    }

    #[test]
    fn with_no_change_of_ours_the_result_is_theirs() {
        let base = document(mac_style());
        let mut theirs = base.clone();
        theirs.global.enabled = false;
        theirs.global.gestures.push(mine());
        assert_eq!(merge(&base, &base, &theirs), theirs);
    }
}
