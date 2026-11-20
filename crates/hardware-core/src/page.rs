//! The whole System page: which groups and cards are drawn.

use crate::battery::BatteryFacts;
use crate::charge::{ChargeLimitView, charge_limit_view};
use crate::charge_state::ChargeReadings;
use crate::display::KeyboardLevels;
use crate::fan::FanState;
use crate::lighting::{self, LightingHint};
use crate::power::{PowerModeView, PowerProfilesFacts, power_mode_view};
use crate::sensors::{SensorInput, SensorRow, rows};
use crate::{Card, Presence};

/// Everything the page reads, from sysfs and the buses.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineFacts {
    pub power_mode: Presence<PowerProfilesFacts>,
    pub charge_limit: Presence<u8>,
    pub battery: Presence<BatteryFacts>,
    pub brightness: Presence<u8>,
    pub keyboard: Presence<KeyboardLevels>,
    /// The keyboard's colour interface, or its one colour (ticket 136).
    pub lighting: Presence<LightingHint>,
    pub fan: Presence<FanState>,
    pub sensors: Vec<SensorInput>,
}

impl MachineFacts {
    /// A machine with nothing Better OS can change.
    pub fn nothing() -> MachineFacts {
        MachineFacts {
            power_mode: Presence::Absent,
            charge_limit: Presence::Absent,
            battery: Presence::Absent,
            brightness: Presence::Absent,
            keyboard: Presence::Absent,
            lighting: Presence::Absent,
            fan: Presence::Absent,
            sensors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerGroup {
    pub mode: Card<PowerModeView>,
    /// The charger switch sits under Power mode and is drawn only while
    /// power-profiles-daemon answers, since that is what it drives.
    pub follow_charger: bool,
    pub charge_limit: Card<ChargeLimitView>,
    pub battery: Card<BatteryFacts>,
    /// What the battery is doing (`charge_state`), drawn on the charge limit
    /// card and the battery card. `None` when the battery's facts could not
    /// be read.
    pub charge: Option<ChargeReadings>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayGroup {
    pub brightness: Card<u8>,
    pub keyboard: Card<KeyboardLevels>,
    /// The keyboard colour card; what it shows also needs the daemon's
    /// answer (`lighting::view`).
    pub lighting: Card<LightingHint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoolingGroup {
    pub fan: Card<FanState>,
    /// Empty when the machine reports no temperature or fan.
    pub sensors: Vec<SensorRow>,
}

/// The page. A group with no card drawn is `None`, and its header is not
/// drawn either. About this PC is always drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemView {
    pub power: Option<PowerGroup>,
    pub display: Option<DisplayGroup>,
    pub cooling: Option<CoolingGroup>,
}

impl SystemView {
    /// "This PC has no hardware settings Better OS can change."
    pub fn is_empty(&self) -> bool {
        self.power.is_none() && self.display.is_none() && self.cooling.is_none()
    }
}

pub fn system_view(facts: &MachineFacts) -> SystemView {
    let mode: Card<PowerModeView> = facts.power_mode.clone().map(|f| power_mode_view(&f)).into();
    let follow_charger = matches!(mode, Card::Live(_));
    let charge_limit: Card<ChargeLimitView> =
        facts.charge_limit.clone().map(charge_limit_view).into();
    let battery: Card<BatteryFacts> = facts.battery.clone().into();
    let charge = facts
        .battery
        .value()
        .map(|battery| battery.charge_readings(&facts.charge_limit));
    let power =
        (mode.is_drawn() || charge_limit.is_drawn() || battery.is_drawn()).then_some(PowerGroup {
            mode,
            follow_charger,
            charge_limit,
            battery,
            charge,
        });

    let brightness: Card<u8> = facts.brightness.clone().into();
    let keyboard: Card<KeyboardLevels> = facts.keyboard.clone().into();
    let lighting: Card<LightingHint> = match &facts.lighting {
        Presence::Present(hint) if !lighting::drawn(hint) => Card::Hidden,
        other => other.clone().into(),
    };
    let display = (brightness.is_drawn() || keyboard.is_drawn() || lighting.is_drawn()).then_some(
        DisplayGroup {
            brightness,
            keyboard,
            lighting,
        },
    );

    let fan: Card<FanState> = facts.fan.clone().into();
    let sensors = rows(&facts.sensors);
    let cooling = (fan.is_drawn() || !sensors.is_empty()).then_some(CoolingGroup { fan, sensors });

    SystemView {
        power,
        display,
        cooling,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battery::BatteryStatus;
    use crate::fan::FanMode;
    use crate::sensors::SensorKind;

    fn owner() -> MachineFacts {
        MachineFacts {
            power_mode: Presence::Present(PowerProfilesFacts {
                active: "balanced".into(),
                profiles: vec![
                    "power-saver".into(),
                    "balanced".into(),
                    "performance".into(),
                ],
                ..Default::default()
            }),
            charge_limit: Presence::Present(100),
            battery: Presence::Present(BatteryFacts {
                capacity: Some(87),
                status: BatteryStatus::Charging,
                full: Some(90.0),
                full_design: Some(100.0),
                cycles: Some(0),
                model: None,
                charger_online: Some(true),
                charge_start: None,
            }),
            brightness: Presence::Present(60),
            keyboard: Presence::Present(KeyboardLevels::from_gnome(33, 4).unwrap()),
            lighting: Presence::Present(LightingHint {
                interface: Some(crate::lighting::HintInterface::LampArray {
                    node: "hidraw1".into(),
                }),
                backlight: true,
            }),
            fan: Presence::Present(FanState::Mode(FanMode::Auto)),
            sensors: vec![SensorInput {
                chip: "k10temp".into(),
                label: Some("Tctl".into()),
                kind: SensorKind::Temperature,
                value: Some(50.0),
            }],
        }
    }

    #[test]
    fn the_owners_machine_draws_every_card() {
        let view = system_view(&owner());
        let power = view.power.unwrap();
        assert!(matches!(power.mode, Card::Live(_)));
        assert!(power.follow_charger);
        assert!(matches!(power.charge_limit, Card::Live(_)));
        assert!(matches!(power.battery, Card::Live(_)));
        let display = view.display.unwrap();
        assert!(matches!(display.brightness, Card::Live(60)));
        assert!(matches!(display.keyboard, Card::Live(_)));
        assert!(matches!(display.lighting, Card::Live(_)));
        let cooling = view.cooling.unwrap();
        assert!(matches!(cooling.fan, Card::Live(_)));
        assert_eq!(cooling.sensors.len(), 1);
    }

    #[test]
    fn a_machine_with_nothing_is_empty() {
        let view = system_view(&MachineFacts::nothing());
        assert!(view.is_empty());
    }

    /// One test per control: absent hides it (and its group when it was the
    /// only card), unreadable draws it disabled.
    #[test]
    fn each_control_is_hidden_when_absent_and_disabled_when_unreadable() {
        type Set = fn(&mut MachineFacts, bool);
        let cases: [(&str, Set); 7] = [
            ("power mode", |f, gone| {
                f.power_mode = if gone {
                    Presence::Absent
                } else {
                    Presence::Unreadable("x".into())
                }
            }),
            ("charge limit", |f, gone| {
                f.charge_limit = if gone {
                    Presence::Absent
                } else {
                    Presence::Unreadable("x".into())
                }
            }),
            ("battery", |f, gone| {
                f.battery = if gone {
                    Presence::Absent
                } else {
                    Presence::Unreadable("x".into())
                }
            }),
            ("brightness", |f, gone| {
                f.brightness = if gone {
                    Presence::Absent
                } else {
                    Presence::Unreadable("x".into())
                }
            }),
            ("keyboard", |f, gone| {
                f.keyboard = if gone {
                    Presence::Absent
                } else {
                    Presence::Unreadable("x".into())
                }
            }),
            ("lighting", |f, gone| {
                f.lighting = if gone {
                    Presence::Absent
                } else {
                    Presence::Unreadable("x".into())
                }
            }),
            ("fan mode", |f, gone| {
                f.fan = if gone {
                    Presence::Absent
                } else {
                    Presence::Unreadable("x".into())
                }
            }),
        ];
        for (name, set) in cases {
            for gone in [true, false] {
                let mut only = MachineFacts::nothing();
                set(&mut only, gone);
                let view = system_view(&only);
                let card_drawn = |v: &SystemView| -> Option<bool> {
                    let disabled = |c: bool| Some(c);
                    match name {
                        "power mode" => v
                            .power
                            .as_ref()
                            .map(|g| matches!(g.mode, Card::Disabled { .. })),
                        "charge limit" => v
                            .power
                            .as_ref()
                            .map(|g| matches!(g.charge_limit, Card::Disabled { .. })),
                        "battery" => v
                            .power
                            .as_ref()
                            .map(|g| matches!(g.battery, Card::Disabled { .. })),
                        "brightness" => v
                            .display
                            .as_ref()
                            .map(|g| matches!(g.brightness, Card::Disabled { .. })),
                        "keyboard" => v
                            .display
                            .as_ref()
                            .map(|g| matches!(g.keyboard, Card::Disabled { .. })),
                        "lighting" => v
                            .display
                            .as_ref()
                            .map(|g| matches!(g.lighting, Card::Disabled { .. })),
                        "fan mode" => v
                            .cooling
                            .as_ref()
                            .map(|g| matches!(g.fan, Card::Disabled { .. })),
                        _ => disabled(false),
                    }
                };
                if gone {
                    assert!(view.is_empty(), "{name} absent leaves the page empty");
                } else {
                    assert_eq!(
                        card_drawn(&view),
                        Some(true),
                        "{name} unreadable is drawn disabled"
                    );
                }
            }
        }
    }

    #[test]
    fn a_keyboard_with_neither_a_backlight_nor_a_colour_interface_draws_no_colour_card() {
        let mut f = MachineFacts::nothing();
        f.lighting = Presence::Present(LightingHint {
            interface: None,
            backlight: false,
        });
        assert!(system_view(&f).is_empty());
        f.lighting = Presence::Present(LightingHint {
            interface: None,
            backlight: true,
        });
        assert!(matches!(
            system_view(&f).display.unwrap().lighting,
            Card::Live(_)
        ));
    }

    #[test]
    fn the_charger_switch_needs_a_readable_power_mode() {
        let mut f = owner();
        f.power_mode = Presence::Unreadable("timeout".into());
        assert!(!system_view(&f).power.unwrap().follow_charger);
    }

    #[test]
    fn the_power_group_carries_the_battery_state_with_the_limit() {
        use crate::charge_state::{ChargeLimit, ChargeState, charge_state};
        let mut f = owner();
        f.charge_limit = Presence::Present(80);
        if let Presence::Present(battery) = &mut f.battery {
            battery.capacity = Some(80);
            battery.status = BatteryStatus::NotCharging;
        }
        let charge = system_view(&f).power.unwrap().charge.unwrap();
        assert_eq!(
            charge.limit,
            ChargeLimit::Percent {
                end: 80,
                start: None
            }
        );
        assert_eq!(
            charge_state(&charge),
            ChargeState::HoldingAtLimit { limit: 80 }
        );
        f.battery = Presence::Unreadable("EIO".into());
        assert_eq!(system_view(&f).power.unwrap().charge, None);
    }
}
