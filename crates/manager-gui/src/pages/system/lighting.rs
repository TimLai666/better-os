//! The keyboard colour card's words (ticket 136, ADR 0034), without GPUI.
//! `hardware_core::lighting` decides what the card shows; this decides how
//! it reads, in both languages.

use hardware_core::lighting::{
    Access, Effect, Interface, LightingChoice, LightingView, LiveLighting, Speed,
};

use crate::i18n::Locale;

pub(crate) struct Copy {
    pub(crate) header: &'static str,
    /// The search entry's description.
    pub(crate) search: &'static str,
    pub(crate) one_colour: &'static str,
    pub(crate) pending: &'static str,
    pub(crate) no_service: &'static str,
    pub(crate) not_allowed: &'static str,
    pub(crate) cancelled: &'static str,
    pub(crate) failed: &'static str,
    pub(crate) not_programmable: &'static str,
    /// The header of the row that says which effects are offered and why.
    pub(crate) about_effects: &'static str,
    /// The effect list's placeholder while nothing is selected.
    pub(crate) choose_effect: &'static str,
    pub(crate) firmware: &'static str,
    pub(crate) steady: &'static str,
    pub(crate) breathing: &'static str,
    pub(crate) cycle: &'static str,
    pub(crate) strobe: &'static str,
    pub(crate) colour: &'static str,
    pub(crate) hue: &'static str,
    pub(crate) saturation: &'static str,
    pub(crate) intensity: &'static str,
    pub(crate) intensity_description: &'static str,
    pub(crate) speed: &'static str,
    pub(crate) slow: &'static str,
    pub(crate) normal: &'static str,
    pub(crate) fast: &'static str,
    /// Why a LampArray offers no effect.
    pub(crate) lamp_array_note: &'static str,
    pub(crate) firmware_effects_note: &'static str,
    pub(crate) not_set_yet: &'static str,
    pub(crate) at_startup: &'static str,
    /// `{n}`.
    pub(crate) zones: &'static str,
    pub(crate) one_zone: &'static str,
}

pub(crate) static EN: Copy = Copy {
    header: "Keyboard colour",
    search: "The colour and effect of the keyboard backlight.",
    one_colour: "This keyboard has one colour.",
    pending: "Reading the keyboard…",
    no_service: "Better Manager's system service isn't answering, so the colour can't be changed.",
    not_allowed: "This account isn't allowed to change the keyboard colour.",
    cancelled: "Nothing was read: authentication wasn't completed.",
    failed: "The keyboard's colour couldn't be read.",
    not_programmable: "This keyboard's lights don't accept a colour from programs.",
    about_effects: "About the effects",
    choose_effect: "Choose an effect",
    firmware: "The keyboard's own lighting",
    steady: "Steady colour",
    breathing: "Breathing",
    cycle: "Colour cycle",
    strobe: "Strobe",
    colour: "Colour",
    hue: "Hue",
    saturation: "Saturation",
    intensity: "Colour brightness",
    intensity_description: "Set apart from the backlight level above.",
    speed: "Speed",
    slow: "Slow",
    normal: "Normal",
    fast: "Fast",
    lamp_array_note: "This keyboard takes one steady colour. Breathing and colour cycle aren't offered: the keyboard can't run them itself, and Better OS doesn't fake them by changing the colour many times a second.",
    firmware_effects_note: "The keyboard runs the effect itself.",
    not_set_yet: "Not set by Better OS yet. The keyboard can't say what it shows now.",
    at_startup: "Better OS sets it again each time the PC starts.",
    zones: "{n} zones",
    one_zone: "1 zone",
};

pub(crate) static ZH_TW: Copy = Copy {
    header: "鍵盤顏色",
    search: "鍵盤背光的顏色和效果。",
    one_colour: "這個鍵盤只有一種顏色。",
    pending: "正在讀取鍵盤…",
    no_service: "Better Manager 的系統服務沒有回應，所以無法變更顏色。",
    not_allowed: "這個帳號不能變更鍵盤顏色。",
    cancelled: "沒有讀取：驗證沒有完成。",
    failed: "讀不到鍵盤的顏色。",
    not_programmable: "這個鍵盤的燈不接受程式設定顏色。",
    about_effects: "關於效果",
    choose_effect: "選擇效果",
    firmware: "鍵盤內建燈光",
    steady: "固定顏色",
    breathing: "呼吸",
    cycle: "色彩循環",
    strobe: "閃爍",
    colour: "顏色",
    hue: "色相",
    saturation: "飽和度",
    intensity: "顏色亮度",
    intensity_description: "和上方的背光段數分開調整。",
    speed: "速度",
    slow: "慢",
    normal: "中",
    fast: "快",
    lamp_array_note: "這個鍵盤只能設定一種固定顏色。這裡沒有呼吸和色彩循環：鍵盤本身做不到，Better OS 也不會用每秒改好幾次顏色的方式假裝。",
    firmware_effects_note: "效果由鍵盤自己執行。",
    not_set_yet: "Better OS 還沒設定過。鍵盤無法回報目前的燈光。",
    at_startup: "每次開機時，Better OS 會重新套用。",
    zones: "{n} 個區域",
    one_zone: "1 個區域",
};

pub(crate) fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        Locale::EnUs | Locale::System => &EN,
    }
}

pub(crate) fn effect_label(effect: Effect, c: &Copy) -> &'static str {
    match effect {
        Effect::Firmware => c.firmware,
        Effect::Static => c.steady,
        Effect::Breathing => c.breathing,
        Effect::Cycle => c.cycle,
        Effect::Strobe => c.strobe,
    }
}

pub(crate) fn speed_label(speed: Speed, c: &Copy) -> &'static str {
    match speed {
        Speed::Slow => c.slow,
        Speed::Normal => c.normal,
        Speed::Fast => c.fast,
    }
}

/// The card's description line for a view that is not live, and whether it
/// is drawn as a warning.
pub(crate) fn state_line(view: &LightingView, c: &Copy) -> Option<&'static str> {
    match view {
        LightingView::OneColour => Some(c.one_colour),
        LightingView::Pending => Some(c.pending),
        LightingView::NotProgrammable => Some(c.not_programmable),
        LightingView::Unavailable(access) => Some(match access {
            Access::NoService => c.no_service,
            Access::NotAllowed => c.not_allowed,
            Access::Cancelled => c.cancelled,
            Access::Absent => c.one_colour,
            Access::Failed(_) => c.failed,
        }),
        LightingView::Live(_) => None,
    }
}

/// `Steady colour · #FF8000`, `Breathing · #00A0FF · Slow`, `Colour cycle ·
/// Fast`, or what nothing set means.
pub(crate) fn live_description(
    live: &LiveLighting,
    choice: Option<&LightingChoice>,
    c: &Copy,
) -> String {
    let Some(choice) = choice else {
        return c.not_set_yet.to_string();
    };
    let mut parts = vec![effect_label(choice.effect, c).to_string()];
    if choice.effect.uses_colour() {
        parts.push(choice.colour.hex());
    }
    if live.intensity && choice.effect.uses_colour() {
        parts.push(format!("{}%", choice.intensity));
    }
    if live.interface == Interface::AsusWmi && choice.effect.uses_speed() {
        parts.push(speed_label(choice.speed, c).to_string());
    }
    parts.join(" · ")
}

/// The note under the effect list.
pub(crate) fn note(live: &LiveLighting, c: &Copy) -> &'static str {
    match live.interface {
        Interface::LampArray => c.lamp_array_note,
        Interface::AsusWmi => c.firmware_effects_note,
    }
}

/// `1 zone`, `24 zones`.
pub(crate) fn zones(lamps: u16, c: &Copy) -> String {
    if lamps == 1 {
        c.one_zone.to_string()
    } else {
        c.zones.replace("{n}", &lamps.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hardware_core::lighting::{Rgb, effects};

    fn live(interface: Interface) -> LiveLighting {
        LiveLighting {
            interface,
            effects: effects(interface),
            choice: None,
            intensity: interface == Interface::LampArray,
            lamps: None,
        }
    }

    #[test]
    fn every_state_has_its_words_in_both_languages() {
        for c in [&EN, &ZH_TW] {
            for view in [
                LightingView::OneColour,
                LightingView::Pending,
                LightingView::NotProgrammable,
                LightingView::Unavailable(Access::NoService),
                LightingView::Unavailable(Access::NotAllowed),
                LightingView::Unavailable(Access::Cancelled),
                LightingView::Unavailable(Access::Failed("EIO".into())),
            ] {
                assert!(!state_line(&view, c).unwrap().is_empty(), "{view:?}");
            }
            for effect in Effect::ALL {
                assert!(!effect_label(effect, c).is_empty());
            }
            for speed in Speed::ALL {
                assert!(!speed_label(speed, c).is_empty());
            }
        }
        assert_eq!(EN.one_colour, "This keyboard has one colour.");
        assert_eq!(ZH_TW.one_colour, "這個鍵盤只有一種顏色。");
    }

    #[test]
    fn the_lamp_array_note_says_why_there_is_no_effect() {
        let lamp = live(Interface::LampArray);
        assert!(note(&lamp, &EN).contains("Breathing and colour cycle aren't offered"));
        assert!(note(&lamp, &EN).contains("doesn't fake them"));
        assert!(note(&lamp, &ZH_TW).contains("沒有呼吸和色彩循環"));
        assert_eq!(
            note(&live(Interface::AsusWmi), &EN),
            EN.firmware_effects_note
        );
    }

    #[test]
    fn the_description_names_the_effect_colour_brightness_and_speed() {
        let lamp = live(Interface::LampArray);
        let steady = LightingChoice {
            effect: Effect::Static,
            colour: Rgb::new(255, 128, 0),
            intensity: 80,
            speed: Speed::Normal,
        };
        assert_eq!(
            live_description(&lamp, Some(&steady), &EN),
            "Steady colour · #FF8000 · 80%"
        );
        assert_eq!(
            live_description(
                &lamp,
                Some(&LightingChoice::starting(Effect::Firmware)),
                &ZH_TW
            ),
            "鍵盤內建燈光"
        );
        let asus = live(Interface::AsusWmi);
        let cycle = LightingChoice {
            effect: Effect::Cycle,
            speed: Speed::Fast,
            ..steady
        };
        assert_eq!(
            live_description(&asus, Some(&cycle), &EN),
            "Colour cycle · Fast"
        );
        let breathing = LightingChoice {
            effect: Effect::Breathing,
            speed: Speed::Slow,
            ..steady
        };
        assert_eq!(
            live_description(&asus, Some(&breathing), &ZH_TW),
            "呼吸 · #FF8000 · 慢"
        );
        assert_eq!(live_description(&asus, None, &EN), EN.not_set_yet);
        assert_eq!(zones(1, &EN), "1 zone");
        assert_eq!(zones(24, &ZH_TW), "24 個區域");
    }
}
