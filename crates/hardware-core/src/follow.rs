//! "Switch power mode with the charger" (owner, 2026-11-13): the setting and
//! the one decision the session service makes on a plug event.

use crate::power::Profile;

/// The person's choice, stored in their own config directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChargerFollow {
    pub enabled: bool,
    /// The mode set when the charger is plugged in.
    pub plugged_in: Profile,
    /// The mode set when it is unplugged.
    pub on_battery: Profile,
}

impl Default for ChargerFollow {
    /// Off, and Performance and Power saver once it is turned on.
    fn default() -> Self {
        ChargerFollow {
            enabled: false,
            plugged_in: Profile::Performance,
            on_battery: Profile::PowerSaver,
        }
    }
}

/// The file name under `$XDG_CONFIG_HOME/better-os/`.
pub const FILE_NAME: &str = "power-follow.json";

/// What to set after UPower's `OnBattery` went from `before` to `now`, or
/// `None` to leave the mode alone.
///
/// Only a change of `OnBattery` acts, so a mode the person picked by hand
/// stays until the next plug event, and the service starting (when `before`
/// is not known yet) acts on nothing. A target the daemon does not offer is
/// not set.
pub fn on_power_change(
    settings: &ChargerFollow,
    before: Option<bool>,
    now: bool,
    offered: &[Profile],
) -> Option<Profile> {
    if !settings.enabled || before? == now {
        return None;
    }
    let target = if now {
        settings.on_battery
    } else {
        settings.plugged_in
    };
    offered.contains(&target).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Profile; 3] = Profile::ALL;

    #[test]
    fn off_by_default_with_performance_and_power_saver() {
        let d = ChargerFollow::default();
        assert!(!d.enabled);
        assert_eq!(d.plugged_in, Profile::Performance);
        assert_eq!(d.on_battery, Profile::PowerSaver);
        assert_eq!(on_power_change(&d, Some(false), true, &ALL), None);
    }

    #[test]
    fn plugging_in_and_unplugging_set_the_chosen_modes() {
        let on = ChargerFollow {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(
            on_power_change(&on, Some(true), false, &ALL),
            Some(Profile::Performance)
        );
        assert_eq!(
            on_power_change(&on, Some(false), true, &ALL),
            Some(Profile::PowerSaver)
        );
        let chosen = ChargerFollow {
            enabled: true,
            plugged_in: Profile::Balanced,
            on_battery: Profile::Balanced,
        };
        assert_eq!(
            on_power_change(&chosen, Some(false), true, &ALL),
            Some(Profile::Balanced)
        );
    }

    #[test]
    fn no_change_and_the_first_reading_do_nothing() {
        let on = ChargerFollow {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(on_power_change(&on, Some(true), true, &ALL), None);
        assert_eq!(on_power_change(&on, None, true, &ALL), None);
    }

    #[test]
    fn a_mode_the_daemon_does_not_offer_is_not_set() {
        let on = ChargerFollow {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(
            on_power_change(
                &on,
                Some(true),
                false,
                &[Profile::PowerSaver, Profile::Balanced]
            ),
            None
        );
    }

    #[test]
    fn the_stored_form_is_stable_and_fills_missing_fields() {
        let on = ChargerFollow {
            enabled: true,
            ..Default::default()
        };
        let text = serde_json::to_string(&on).unwrap();
        assert_eq!(
            text,
            r#"{"enabled":true,"plugged_in":"performance","on_battery":"power-saver"}"#
        );
        let partial: ChargerFollow = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(partial, on);
        assert!(serde_json::from_str::<ChargerFollow>(r#"{"enabled":1}"#).is_err());
    }
}
