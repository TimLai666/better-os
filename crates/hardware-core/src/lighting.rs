//! Keyboard colour and effects (ticket 136, ADR 0034): the choices, the
//! closed set each interface accepts, and what the System page's card shows.
//!
//! Two interfaces:
//! - a HID LampArray ([`crate::lamparray`]), which takes one colour and
//!   intensity for its lamps or gives the lighting back to its firmware. It
//!   has no effects, so none is offered: Better OS never animates a colour
//!   by rewriting it on a timer.
//! - asus-wmi's `kbd_rgb_mode` on ASUS TUF and ROG laptops, where the
//!   firmware runs the effect itself. The kernel's ABI document does not list
//!   the modes; [`asus_wmi`] says where the four offered here come from.

use serde::{Deserialize, Serialize};

use crate::lamparray;

/// A colour, eight bits per channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);

    pub const fn new(red: u8, green: u8, blue: u8) -> Rgb {
        Rgb { red, green, blue }
    }

    /// `#RRGGBB`.
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }

    /// The colour as one number, `0xRRGGBB`, as GPUI's `rgb()` takes it.
    pub fn packed(self) -> u32 {
        (u32::from(self.red) << 16) | (u32::from(self.green) << 8) | u32::from(self.blue)
    }

    /// From hue (degrees) and saturation (0 to 1) at full value: the colour
    /// picker's two sliders. Brightness is the intensity's, not the colour's.
    pub fn from_hue_saturation(hue: f64, saturation: f64) -> Rgb {
        let hue = if hue.is_finite() {
            hue.rem_euclid(360.0)
        } else {
            0.0
        };
        let saturation = if saturation.is_finite() {
            saturation.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let chroma = saturation;
        let sector = hue / 60.0;
        let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match sector as u32 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let m = 1.0 - chroma;
        let channel = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb::new(channel(r), channel(g), channel(b))
    }

    /// Hue (degrees) and saturation (0 to 1), ignoring value.
    pub fn hue_saturation(self) -> (f64, f64) {
        let r = f64::from(self.red) / 255.0;
        let g = f64::from(self.green) / 255.0;
        let b = f64::from(self.blue) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let saturation = if max == 0.0 { 0.0 } else { delta / max };
        let hue = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        (hue, saturation)
    }
}

/// The swatches the colour picker offers, in its order.
pub const SWATCHES: [Rgb; 12] = [
    Rgb::new(255, 255, 255),
    Rgb::new(255, 0, 0),
    Rgb::new(255, 96, 0),
    Rgb::new(255, 200, 0),
    Rgb::new(128, 255, 0),
    Rgb::new(0, 255, 64),
    Rgb::new(0, 255, 200),
    Rgb::new(0, 160, 255),
    Rgb::new(0, 64, 255),
    Rgb::new(128, 0, 255),
    Rgb::new(255, 0, 200),
    Rgb::new(255, 64, 96),
];

/// What the keyboard shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// The keyboard's own default: on a LampArray, autonomous mode on.
    Firmware,
    Static,
    Breathing,
    Cycle,
    Strobe,
}

impl Effect {
    pub const ALL: [Effect; 5] = [
        Effect::Firmware,
        Effect::Static,
        Effect::Breathing,
        Effect::Cycle,
        Effect::Strobe,
    ];

    /// The argument `SetKeyboardLighting` takes.
    pub fn wire(self) -> &'static str {
        match self {
            Effect::Firmware => "firmware",
            Effect::Static => "static",
            Effect::Breathing => "breathing",
            Effect::Cycle => "cycle",
            Effect::Strobe => "strobe",
        }
    }

    /// Exactly one of the five names; nothing else.
    pub fn from_wire(name: &str) -> Option<Effect> {
        Effect::ALL.into_iter().find(|e| e.wire() == name)
    }

    /// Whether the colour is part of it. Colour cycle runs through every
    /// colour, and the firmware's default is its own.
    pub fn uses_colour(self) -> bool {
        matches!(self, Effect::Static | Effect::Breathing | Effect::Strobe)
    }

    /// Whether it has a speed.
    pub fn uses_speed(self) -> bool {
        matches!(self, Effect::Breathing | Effect::Cycle)
    }
}

/// An effect's speed, as `kbd_rgb_mode` numbers it (0, 1, 2).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Speed {
    Slow,
    #[default]
    Normal,
    Fast,
}

impl Speed {
    pub const ALL: [Speed; 3] = [Speed::Slow, Speed::Normal, Speed::Fast];

    pub fn wire(self) -> u8 {
        match self {
            Speed::Slow => 0,
            Speed::Normal => 1,
            Speed::Fast => 2,
        }
    }

    pub fn from_wire(value: u8) -> Option<Speed> {
        Speed::ALL.into_iter().find(|s| s.wire() == value)
    }
}

/// The person's choice, as the daemon keeps it in
/// `/var/lib/better-os/keyboard-lighting.json` and writes it back at boot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingChoice {
    pub effect: Effect,
    pub colour: Rgb,
    /// 0 to 100; a LampArray's intensity channel. `kbd_rgb_mode` has none
    /// (its brightness is the keyboard backlight's level).
    pub intensity: u8,
    pub speed: Speed,
}

impl LightingChoice {
    /// What a colour first chosen starts from.
    pub fn starting(effect: Effect) -> LightingChoice {
        LightingChoice {
            effect,
            colour: Rgb::WHITE,
            intensity: 100,
            speed: Speed::Normal,
        }
    }
}

/// The two interfaces.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Interface {
    LampArray,
    AsusWmi,
}

/// The effects each interface is offered, in the card's order: only what
/// the hardware does itself.
pub fn effects(interface: Interface) -> &'static [Effect] {
    match interface {
        Interface::LampArray => &[Effect::Static, Effect::Firmware],
        Interface::AsusWmi => &[
            Effect::Static,
            Effect::Breathing,
            Effect::Cycle,
            Effect::Strobe,
        ],
    }
}

/// Whether the daemon accepts `choice` for `interface`.
pub fn accepts(interface: Interface, choice: &LightingChoice) -> bool {
    effects(interface).contains(&choice.effect) && choice.intensity <= 100
}

/// asus-wmi's `kbd_rgb_mode` (`/sys/class/leds/asus::kbd_backlight/`),
/// write-only, `cmd mode red green blue speed`
/// (`drivers/platform/x86/asus-wmi.c`, `kbd_rgb_mode_store`).
///
/// - `cmd` 0 sets the mode for now (firmware command 0xB3); 1 would save
///   it in the firmware (0xB4). Better OS writes 0 only and sets it again
///   at boot, so removing Better OS leaves the firmware's own saved setting,
///   which comes back at the next start.
/// - The kernel's ABI document (`sysfs-platform-asus-wmi`) does not list
///   the modes. The driver forces 9 and anything from 12 up to 10, "the
///   known usable modes across all TUF/ROG"; OpenRGB's ASUS TUF controller
///   names 0 static, 1 breathing, 2 colour cycle, and 0x0A strobing
///   (`AsusTUFLaptopController_Windows.h`), and those four are all that is
///   offered.
/// - `speed` 0, 1, 2 are slow, normal, fast (0xE1, 0xEB, 0xF5).
pub mod asus_wmi {
    use super::{Effect, LightingChoice, Speed};

    /// The attribute's file name.
    pub const ATTRIBUTE: &str = "kbd_rgb_mode";

    pub fn mode_number(effect: Effect) -> Option<u8> {
        match effect {
            Effect::Static => Some(0),
            Effect::Breathing => Some(1),
            Effect::Cycle => Some(2),
            Effect::Strobe => Some(10),
            Effect::Firmware => None,
        }
    }

    /// What is written to `kbd_rgb_mode` for a choice, or `None` for one
    /// this interface does not take.
    pub fn value(choice: &LightingChoice) -> Option<String> {
        let mode = mode_number(choice.effect)?;
        let c = choice.colour;
        Some(format!(
            "0 {mode} {} {} {} {}",
            c.red,
            c.green,
            c.blue,
            choice.speed.wire()
        ))
    }

    /// Whether a value is exactly one [`value`] produces: the helper checks
    /// it again before it writes.
    pub fn accepts_value(text: &str) -> bool {
        let parts: Vec<&str> = text.split(' ').collect();
        let [cmd, mode, r, g, b, speed] = parts.as_slice() else {
            return false;
        };
        let number = |s: &str| -> Option<u8> {
            (!s.is_empty()
                && s.bytes().all(|c| c.is_ascii_digit())
                && (s == "0" || !s.starts_with('0')))
            .then(|| s.parse().ok())
            .flatten()
        };
        *cmd == "0"
            && number(mode).is_some_and(|m| matches!(m, 0 | 1 | 2 | 10))
            && number(r).is_some()
            && number(g).is_some()
            && number(b).is_some()
            && number(speed).and_then(Speed::from_wire).is_some()
    }
}

/// What the daemon answers to `KeyboardLighting`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LightingReply {
    pub interface: Interface,
    /// The LampArray's own description; `None` for `kbd_rgb_mode`.
    pub lamp_array: Option<lamparray::Description>,
    /// What Better OS last set and sets again at boot; `None` when it has
    /// set nothing (the firmware's own lighting).
    pub stored: Option<LightingChoice>,
}

/// Which colour interface sysfs shows, read without privilege.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HintInterface {
    /// A hidraw node whose report descriptor declares a LampArray.
    LampArray {
        node: String,
    },
    AsusWmi,
}

/// What sysfs says about the keyboard's lighting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LightingHint {
    pub interface: Option<HintInterface>,
    /// A `*::kbd_backlight` LED exists.
    pub backlight: bool,
}

/// Why the daemon could not answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Access {
    /// The daemon is not installed or not answering.
    NoService,
    NotAllowed,
    Cancelled,
    /// It answered that the machine has no such keyboard (a LampArray that
    /// is not a keyboard, for one).
    Absent,
    Failed(String),
}

/// The card, as the page draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum LightingView {
    /// "This keyboard has one colour."
    OneColour,
    /// The daemon has not answered yet.
    Pending,
    Unavailable(Access),
    /// The lamps say they cannot be programmed.
    NotProgrammable,
    Live(LiveLighting),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiveLighting {
    pub interface: Interface,
    pub effects: &'static [Effect],
    /// `None` on `kbd_rgb_mode` when Better OS has set nothing: the file is
    /// write-only, so what the keyboard shows cannot be read.
    pub choice: Option<LightingChoice>,
    /// The intensity slider is drawn.
    pub intensity: bool,
    pub lamps: Option<u16>,
}

impl LiveLighting {
    /// The effect selected in the list, if any.
    pub fn effect(&self) -> Option<Effect> {
        self.choice.map(|c| c.effect)
    }
}

/// The card for what sysfs showed and what the daemon answered (`None`
/// while it has not).
pub fn view(hint: &LightingHint, reply: Option<&Result<LightingReply, Access>>) -> LightingView {
    let one_colour_or_unavailable = |access: Access| {
        if access == Access::Absent {
            LightingView::OneColour
        } else {
            LightingView::Unavailable(access)
        }
    };
    if hint.interface.is_none() {
        return LightingView::OneColour;
    }
    let reply = match reply {
        None => return LightingView::Pending,
        Some(Err(access)) => return one_colour_or_unavailable(access.clone()),
        Some(Ok(reply)) => reply,
    };
    match reply.interface {
        Interface::LampArray => {
            let Some(description) = &reply.lamp_array else {
                return LightingView::Unavailable(Access::Failed(
                    "the daemon did not describe the LampArray".to_string(),
                ));
            };
            if !description.levels.has_colour() {
                return LightingView::OneColour;
            }
            if !description.programmable {
                return LightingView::NotProgrammable;
            }
            LightingView::Live(LiveLighting {
                interface: Interface::LampArray,
                effects: effects(Interface::LampArray),
                choice: Some(
                    reply
                        .stored
                        .unwrap_or(LightingChoice::starting(Effect::Firmware)),
                ),
                intensity: description.levels.has_intensity(),
                lamps: Some(description.lamps),
            })
        }
        Interface::AsusWmi => LightingView::Live(LiveLighting {
            interface: Interface::AsusWmi,
            effects: effects(Interface::AsusWmi),
            choice: reply.stored,
            intensity: false,
            lamps: None,
        }),
    }
}

/// Whether the card is drawn at all: a backlight or a colour interface.
pub fn drawn(hint: &LightingHint) -> bool {
    hint.backlight || hint.interface.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lamparray::{Description, LampArrayKind, Levels};

    fn lamp_reply(levels: Levels, programmable: bool) -> LightingReply {
        LightingReply {
            interface: Interface::LampArray,
            lamp_array: Some(Description {
                lamps: 1,
                kind: Some(LampArrayKind::Keyboard),
                min_update_interval_us: None,
                levels,
                programmable,
            }),
            stored: None,
        }
    }

    const FULL: Levels = Levels {
        red: 255,
        green: 255,
        blue: 255,
        intensity: 255,
    };

    fn lamp_hint() -> LightingHint {
        LightingHint {
            interface: Some(HintInterface::LampArray {
                node: "hidraw1".into(),
            }),
            backlight: true,
        }
    }

    #[test]
    fn only_the_five_effect_names_and_three_speeds_cross_the_bus() {
        for effect in Effect::ALL {
            assert_eq!(Effect::from_wire(effect.wire()), Some(effect));
        }
        for refused in ["", "Static", "rainbow", "static ", "breathe", "0"] {
            assert_eq!(Effect::from_wire(refused), None, "{refused:?}");
        }
        assert_eq!(Speed::from_wire(3), None);
        assert_eq!(Speed::from_wire(2), Some(Speed::Fast));
    }

    #[test]
    fn a_lamp_array_is_offered_a_colour_and_its_own_default_and_no_effect() {
        assert_eq!(
            effects(Interface::LampArray),
            &[Effect::Static, Effect::Firmware]
        );
        let mut choice = LightingChoice::starting(Effect::Static);
        assert!(accepts(Interface::LampArray, &choice));
        for effect in [Effect::Breathing, Effect::Cycle, Effect::Strobe] {
            choice.effect = effect;
            assert!(!accepts(Interface::LampArray, &choice), "{effect:?}");
            assert!(accepts(Interface::AsusWmi, &choice), "{effect:?}");
        }
        choice.effect = Effect::Firmware;
        assert!(!accepts(Interface::AsusWmi, &choice));
        choice = LightingChoice::starting(Effect::Static);
        choice.intensity = 101;
        assert!(!accepts(Interface::LampArray, &choice));
    }

    #[test]
    fn kbd_rgb_mode_gets_the_drivers_six_numbers_and_nothing_else() {
        let choice = LightingChoice {
            effect: Effect::Strobe,
            colour: Rgb::new(255, 8, 0),
            intensity: 100,
            speed: Speed::Fast,
        };
        assert_eq!(asus_wmi::value(&choice).unwrap(), "0 10 255 8 0 2");
        for effect in [
            Effect::Static,
            Effect::Breathing,
            Effect::Cycle,
            Effect::Strobe,
        ] {
            let value = asus_wmi::value(&LightingChoice { effect, ..choice }).unwrap();
            assert!(asus_wmi::accepts_value(&value), "{value}");
        }
        assert_eq!(
            asus_wmi::value(&LightingChoice::starting(Effect::Firmware)),
            None
        );
        for refused in [
            "1 0 255 0 0 1", // saving to the firmware
            "0 3 255 0 0 1", // a mode not offered
            "0 9 255 0 0 1",
            "0 0 256 0 0 1", // out of range
            "0 0 255 0 0 3",
            "0 0 255 0 0",     // too few
            "0 0 255 0 0 1 1", // too many
            "0  0 255 0 0 1",
            "0 0 255 0 0 1\n",
            "0 0 0255 0 0 1",
            "0 0 -1 0 0 1",
            "0 0 +1 0 0 1",
            "",
        ] {
            assert!(!asus_wmi::accepts_value(refused), "{refused:?}");
        }
    }

    #[test]
    fn hue_and_saturation_round_trip_the_swatches() {
        assert_eq!(Rgb::from_hue_saturation(0.0, 1.0), Rgb::new(255, 0, 0));
        assert_eq!(Rgb::from_hue_saturation(120.0, 1.0), Rgb::new(0, 255, 0));
        assert_eq!(Rgb::from_hue_saturation(240.0, 1.0), Rgb::new(0, 0, 255));
        assert_eq!(Rgb::from_hue_saturation(77.0, 0.0), Rgb::WHITE);
        assert_eq!(Rgb::from_hue_saturation(360.0, 1.0), Rgb::new(255, 0, 0));
        assert_eq!(
            Rgb::from_hue_saturation(f64::NAN, f64::INFINITY),
            Rgb::WHITE
        );
        for swatch in SWATCHES {
            let (h, s) = swatch.hue_saturation();
            assert_eq!(Rgb::from_hue_saturation(h, s), swatch, "{}", swatch.hex());
        }
        assert_eq!(Rgb::new(255, 128, 0).hex(), "#FF8000");
        assert_eq!(Rgb::new(1, 2, 3).packed(), 0x010203);
    }

    #[test]
    fn a_stored_choice_round_trips_and_refuses_unknown_fields() {
        let choice = LightingChoice {
            effect: Effect::Static,
            colour: Rgb::new(10, 20, 30),
            intensity: 40,
            speed: Speed::Slow,
        };
        let text = serde_json::to_string(&choice).unwrap();
        assert_eq!(
            serde_json::from_str::<LightingChoice>(&text).unwrap(),
            choice
        );
        assert!(
            serde_json::from_str::<LightingChoice>(
                r#"{"effect":"static","colour":{"red":1,"green":2,"blue":3},"intensity":4,"speed":"slow","vendor":95}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<LightingChoice>(
                r#"{"effect":"rainbow","colour":{"red":1,"green":2,"blue":3},"intensity":4,"speed":"slow"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn the_card_says_one_colour_unavailable_pending_or_live() {
        let white = LightingHint {
            interface: None,
            backlight: true,
        };
        assert!(drawn(&white));
        assert_eq!(view(&white, None), LightingView::OneColour);
        assert!(!drawn(&LightingHint {
            interface: None,
            backlight: false
        }));

        let hint = lamp_hint();
        assert_eq!(view(&hint, None), LightingView::Pending);
        assert_eq!(
            view(&hint, Some(&Err(Access::NoService))),
            LightingView::Unavailable(Access::NoService)
        );
        assert_eq!(
            view(&hint, Some(&Err(Access::Absent))),
            LightingView::OneColour
        );
        // A LampArray with one level of colour is a one-colour keyboard.
        let white_lamp = Levels {
            red: 1,
            green: 1,
            blue: 1,
            intensity: 4,
        };
        assert_eq!(
            view(&hint, Some(&Ok(lamp_reply(white_lamp, true)))),
            LightingView::OneColour
        );
        assert_eq!(
            view(&hint, Some(&Ok(lamp_reply(FULL, false)))),
            LightingView::NotProgrammable
        );
        let LightingView::Live(live) = view(&hint, Some(&Ok(lamp_reply(FULL, true)))) else {
            panic!("live");
        };
        // Nothing stored: the keyboard's own lighting is what it shows.
        assert_eq!(live.effect(), Some(Effect::Firmware));
        assert!(live.intensity);
        assert_eq!(live.lamps, Some(1));

        let asus = LightingHint {
            interface: Some(HintInterface::AsusWmi),
            backlight: true,
        };
        let reply = LightingReply {
            interface: Interface::AsusWmi,
            lamp_array: None,
            stored: None,
        };
        let LightingView::Live(live) = view(&asus, Some(&Ok(reply))) else {
            panic!("live");
        };
        // Write-only: with nothing stored, nothing is selected.
        assert_eq!(live.effect(), None);
        assert!(!live.intensity);
        assert_eq!(live.effects.len(), 4);
    }
}
