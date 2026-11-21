//! The status row's battery (design/login-screen-spec.md sections 3.1 and
//! 3.15).
//!
//! The battery's picture, its words and its accessible name are
//! `hardware_core::charge_state::battery_look`'s, as for every battery icon
//! in Better OS (`engineering/specs/battery-charge-state.md`): this module
//! only carries the screen's language over. The button draws the picture and
//! the charge figure; a screen reader hears `spoken`.

use hardware_core::charge_state::{self, BatteryLook, ChargeReadings};

use crate::copy::Language;

/// The language `charge_state` speaks for the screen's.
pub fn charge_language(language: Language) -> charge_state::Language {
    match language {
        Language::ZhTw => charge_state::Language::TraditionalChinese,
        Language::EnUs => charge_state::Language::English,
    }
}

/// The battery button: picture, words and name, from the one battery rule.
pub fn battery(readings: &ChargeReadings, language: Language) -> BatteryLook {
    charge_state::battery_look(readings, charge_language(language))
}

/// The figure the button draws beside the picture (Caption), `None` when no
/// charge was read: the picture then says the state alone.
pub fn battery_figure(readings: &ChargeReadings) -> Option<String> {
    readings.percent().map(|percent| format!("{percent}%"))
}

#[cfg(test)]
mod tests {
    use hardware_core::battery::BatteryStatus;
    use hardware_core::charge_state::{BatteryIcon, ChargeLimit, ChargeState};

    use super::*;

    fn readings(
        charger: Option<bool>,
        status: Option<BatteryStatus>,
        percent: Option<u8>,
    ) -> ChargeReadings {
        ChargeReadings::single(
            charger,
            status,
            percent,
            ChargeLimit::Percent {
                end: 80,
                start: None,
            },
        )
    }

    /// Every state `battery_look` returns reaches the screen in both
    /// languages with its own picture and words.
    #[test]
    fn every_state_comes_from_the_one_battery_rule() {
        let cases = [
            (
                readings(Some(true), Some(BatteryStatus::Charging), Some(60)),
                ChargeState::Charging,
            ),
            (
                readings(Some(true), Some(BatteryStatus::NotCharging), Some(80)),
                ChargeState::Holding {
                    limit: 80,
                    percent: Some(80),
                },
            ),
            (
                readings(Some(false), Some(BatteryStatus::Discharging), Some(87)),
                ChargeState::OnBattery,
            ),
            (
                readings(Some(false), Some(BatteryStatus::Discharging), Some(9)),
                ChargeState::OnBattery,
            ),
            (
                readings(Some(true), Some(BatteryStatus::NotCharging), Some(30)),
                ChargeState::PluggedInNotCharging,
            ),
            (readings(None, None, None), ChargeState::Unknown),
        ];
        for (reading, state) in cases {
            for language in [Language::ZhTw, Language::EnUs] {
                let look = battery(&reading, language);
                let shared = charge_state::battery_look(&reading, charge_language(language));
                assert_eq!(look, shared);
                assert_eq!(look.state, state);
            }
        }
        let low = battery(
            &readings(Some(false), Some(BatteryStatus::Discharging), Some(9)),
            Language::ZhTw,
        );
        assert_eq!(low.icon, BatteryIcon::Low);
        assert_eq!(low.spoken, "電池 9%，使用電池");
        let charging = battery(
            &readings(Some(true), Some(BatteryStatus::Charging), Some(60)),
            Language::EnUs,
        );
        assert_eq!(charging.spoken, "Battery 60%: Charging");
    }

    #[test]
    fn the_figure_is_shown_only_when_read() {
        assert_eq!(
            battery_figure(&readings(
                Some(false),
                Some(BatteryStatus::Discharging),
                Some(87)
            ))
            .as_deref(),
            Some("87%")
        );
        assert_eq!(battery_figure(&readings(None, None, None)), None);
    }
}
