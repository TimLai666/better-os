//! The keyboard colour card's words (ticket 136, ADR 0034), without GPUI.
//! `hardware_core::lighting` decides what the card shows; this decides how
//! it reads, in both languages.

use hardware_core::lamparray::{Description, LampArrayKind};
use hardware_core::lighting::{
    Access, Effect, Interface, LightingChoice, LightingDevice, LightingView, LiveLighting, Speed,
};

use crate::i18n::Locale;

pub(crate) struct Copy {
    pub(crate) header: &'static str,
    /// The search entry's description.
    pub(crate) search: &'static str,
    /// A backlight and no colour control found: only what Better OS knows.
    pub(crate) no_colour_interface: &'static str,
    /// The keyboard itself reports no colour channel.
    pub(crate) one_colour: &'static str,
    /// The built-in lighting device says it is not a keyboard. `{kind}`.
    pub(crate) not_a_keyboard: &'static str,
    /// The kind when the device reports none it names.
    pub(crate) kind_unnamed: &'static str,
    pub(crate) pending: &'static str,
    pub(crate) no_service: &'static str,
    pub(crate) outdated: &'static str,
    pub(crate) not_allowed: &'static str,
    pub(crate) cancelled: &'static str,
    pub(crate) absent: &'static str,
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
    /// On `kbd_rgb_mode`, what "the keyboard's own lighting" does.
    pub(crate) firmware_next_start_note: &'static str,
    /// Chosen on `kbd_rgb_mode`, until the PC starts again.
    pub(crate) firmware_next_start: &'static str,
    pub(crate) not_set_yet: &'static str,
    pub(crate) at_startup: &'static str,
    /// `{n}`.
    pub(crate) zones: &'static str,
    pub(crate) one_zone: &'static str,
    /// The details row: what the keyboard is.
    pub(crate) device: &'static str,
    /// `{id}`, `{zones}`, `{kind}`, `{red}`, `{green}`, `{blue}`,
    /// `{intensity}`.
    pub(crate) device_lamp_array: &'static str,
    pub(crate) device_asus_wmi: &'static str,
    /// The swatches' names, in `SWATCHES` order.
    pub(crate) swatches: [&'static str; 12],
    /// LampArrayKind names, `Undefined` to `Other` in the usage tables'
    /// order.
    pub(crate) kinds: [&'static str; 12],
}

pub(crate) static EN: Copy = Copy {
    header: "Keyboard colour",
    search: "The colour and effect of the keyboard backlight.",
    no_colour_interface: "Better OS found no way to set this keyboard's colour. As far as this PC reports, it has one colour.",
    one_colour: "This keyboard has one colour.",
    not_a_keyboard: "The built-in lighting device here says it is a {kind}, not a keyboard, so Better OS doesn't set it as the keyboard's colour.",
    kind_unnamed: "device of a kind Better OS doesn't know",
    pending: "Reading the keyboard…",
    no_service: "Better Manager's system service isn't answering, so the colour can't be changed.",
    outdated: "Better Manager's system service is older than this window. Update it to set the keyboard's colour.",
    not_allowed: "This account isn't allowed to change the keyboard colour.",
    cancelled: "Nothing was read: authentication wasn't completed.",
    absent: "The keyboard's lighting device wasn't found when Better OS looked for it.",
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
    firmware_next_start_note: "The keyboard's own lighting stops Better OS setting a colour; the keyboard's own setting comes back the next time the PC starts.",
    firmware_next_start: "The keyboard's own lighting, from the next start",
    not_set_yet: "Not set by Better OS yet. The keyboard can't say what it shows now.",
    at_startup: "Better OS sets it again each time the PC starts.",
    zones: "{n} zones",
    one_zone: "1 zone",
    device: "Device",
    device_lamp_array: "Lighting device {id} · {zones} · {kind} · colour levels {red}/{green}/{blue}, brightness levels {intensity}",
    device_asus_wmi: "ASUS keyboard lighting (asus-wmi)",
    swatches: [
        "White", "Red", "Orange", "Amber", "Lime", "Green", "Teal", "Sky blue", "Blue", "Purple",
        "Magenta", "Pink",
    ],
    kinds: [
        "device of undefined kind",
        "keyboard",
        "mouse",
        "game controller",
        "peripheral",
        "scene light",
        "notification light",
        "chassis light",
        "wearable",
        "furniture light",
        "art light",
        "device of a kind Better OS doesn't know",
    ],
};

pub(crate) static ZH_TW: Copy = Copy {
    header: "鍵盤顏色",
    search: "鍵盤背光的顏色和效果。",
    no_colour_interface: "Better OS 找不到設定這個鍵盤顏色的方法。就這台電腦回報的資訊來看，它只有一種顏色。",
    one_colour: "這個鍵盤只有一種顏色。",
    not_a_keyboard: "這台電腦內建的燈光裝置回報自己是「{kind}」，不是鍵盤，所以 Better OS 不會把它當成鍵盤顏色來設定。",
    kind_unnamed: "Better OS 不認得的裝置類型",
    pending: "正在讀取鍵盤…",
    no_service: "Better Manager 的系統服務沒有回應，所以無法變更顏色。",
    outdated: "Better Manager 的系統服務比這個視窗舊，請先更新才能設定鍵盤顏色。",
    not_allowed: "這個帳號不能變更鍵盤顏色。",
    cancelled: "沒有讀取：驗證沒有完成。",
    absent: "Better OS 尋找鍵盤的燈光裝置時沒有找到。",
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
    firmware_next_start_note: "選「鍵盤內建燈光」後，Better OS 就不再設定顏色；鍵盤自己的設定會在下次開機時回來。",
    firmware_next_start: "鍵盤內建燈光，下次開機起生效",
    not_set_yet: "Better OS 還沒設定過。鍵盤無法回報目前的燈光。",
    at_startup: "每次開機時，Better OS 會重新套用。",
    zones: "{n} 個區域",
    one_zone: "1 個區域",
    device: "裝置",
    device_lamp_array: "燈光裝置 {id} · {zones} · {kind} · 色階 {red}/{green}/{blue}，亮度 {intensity} 段",
    device_asus_wmi: "ASUS 鍵盤燈光（asus-wmi）",
    swatches: [
        "白色",
        "紅色",
        "橘色",
        "琥珀色",
        "萊姆綠",
        "綠色",
        "藍綠色",
        "天藍色",
        "藍色",
        "紫色",
        "洋紅色",
        "粉紅色",
    ],
    kinds: [
        "未定義類型的裝置",
        "鍵盤",
        "滑鼠",
        "遊戲控制器",
        "周邊裝置",
        "情境燈",
        "通知燈",
        "機殼燈",
        "穿戴裝置",
        "家具燈",
        "裝飾燈",
        "Better OS 不認得的裝置類型",
    ],
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

pub(crate) fn kind_label(kind: Option<LampArrayKind>, c: &Copy) -> &'static str {
    let Some(kind) = kind else {
        return c.kind_unnamed;
    };
    let index = match kind {
        LampArrayKind::Undefined => 0,
        LampArrayKind::Keyboard => 1,
        LampArrayKind::Mouse => 2,
        LampArrayKind::GameController => 3,
        LampArrayKind::Peripheral => 4,
        LampArrayKind::Scene => 5,
        LampArrayKind::Notification => 6,
        LampArrayKind::Chassis => 7,
        LampArrayKind::Wearable => 8,
        LampArrayKind::Furniture => 9,
        LampArrayKind::Art => 10,
        LampArrayKind::Other => 11,
    };
    c.kinds[index]
}

/// The swatch's name, which its tooltip shows.
pub(crate) fn swatch_name(index: usize, c: &Copy) -> &'static str {
    c.swatches.get(index).copied().unwrap_or("")
}

/// The card's description line for a view that is not live.
pub(crate) fn state_line(view: &LightingView, c: &Copy) -> Option<String> {
    Some(match view {
        LightingView::NoColourInterface => c.no_colour_interface.to_string(),
        LightingView::OneColour => c.one_colour.to_string(),
        LightingView::NotAKeyboard(kind) => {
            c.not_a_keyboard.replace("{kind}", kind_label(*kind, c))
        }
        LightingView::Pending => c.pending.to_string(),
        LightingView::NotProgrammable => c.not_programmable.to_string(),
        LightingView::Unavailable(access) => match access {
            Access::NoService => c.no_service,
            Access::Outdated => c.outdated,
            Access::NotAllowed => c.not_allowed,
            Access::Cancelled => c.cancelled,
            Access::Absent => c.absent,
            Access::Failed(_) => c.failed,
        }
        .to_string(),
        LightingView::Live(_) => return None,
    })
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
    if choice.effect == Effect::Firmware && live.firmware_at_next_start {
        return c.firmware_next_start.to_string();
    }
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
pub(crate) fn note(live: &LiveLighting, c: &Copy) -> String {
    match live.interface {
        Interface::LampArray => c.lamp_array_note.to_string(),
        Interface::AsusWmi => format!("{} {}", c.firmware_effects_note, c.firmware_next_start_note),
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

/// The details row: what the keyboard said about itself, so the owner's
/// check needs no root command (ticket 136).
pub(crate) fn device_line(
    device: &LightingDevice,
    description: Option<&Description>,
    c: &Copy,
) -> String {
    match (device.interface, description) {
        (Interface::LampArray, Some(d)) => c
            .device_lamp_array
            .replace("{id}", &device.label())
            .replace("{zones}", &zones(d.lamps, c))
            .replace("{kind}", kind_label(d.kind, c))
            .replace("{red}", &d.levels.red.to_string())
            .replace("{green}", &d.levels.green.to_string())
            .replace("{blue}", &d.levels.blue.to_string())
            .replace("{intensity}", &d.levels.intensity.to_string()),
        (Interface::LampArray, None) => device.label(),
        (Interface::AsusWmi, _) => c.device_asus_wmi.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hardware_core::lamparray::Levels;
    use hardware_core::lighting::{Rgb, SWATCHES, effects};

    fn live(interface: Interface) -> LiveLighting {
        LiveLighting {
            interface,
            effects: effects(interface),
            choice: None,
            intensity: interface == Interface::LampArray,
            lamps: None,
            firmware_at_next_start: false,
        }
    }

    #[test]
    fn every_state_has_its_words_in_both_languages() {
        for c in [&EN, &ZH_TW] {
            for view in [
                LightingView::NoColourInterface,
                LightingView::OneColour,
                LightingView::NotAKeyboard(Some(LampArrayKind::Chassis)),
                LightingView::NotAKeyboard(None),
                LightingView::Pending,
                LightingView::NotProgrammable,
                LightingView::Unavailable(Access::NoService),
                LightingView::Unavailable(Access::Outdated),
                LightingView::Unavailable(Access::NotAllowed),
                LightingView::Unavailable(Access::Cancelled),
                LightingView::Unavailable(Access::Absent),
                LightingView::Unavailable(Access::Failed("EIO".into())),
            ] {
                let line = state_line(&view, c).unwrap();
                assert!(!line.is_empty() && !line.contains('{'), "{view:?}: {line}");
            }
            for effect in Effect::ALL {
                assert!(!effect_label(effect, c).is_empty());
            }
            for speed in Speed::ALL {
                assert!(!speed_label(speed, c).is_empty());
            }
            for index in 0..SWATCHES.len() {
                assert!(!swatch_name(index, c).is_empty(), "swatch {index}");
            }
        }
        assert_eq!(EN.one_colour, "This keyboard has one colour.");
        assert_eq!(ZH_TW.one_colour, "這個鍵盤只有一種顏色。");
    }

    /// The review's finding 4: "one colour" only on the keyboard's own
    /// word; what was found otherwise is said as found.
    #[test]
    fn one_colour_is_said_only_when_the_keyboard_says_so() {
        let found = state_line(
            &LightingView::NotAKeyboard(Some(LampArrayKind::Chassis)),
            &EN,
        )
        .unwrap();
        assert!(found.contains("chassis light"), "{found}");
        assert!(!found.contains("one colour"));
        let zh = state_line(
            &LightingView::NotAKeyboard(Some(LampArrayKind::Peripheral)),
            &ZH_TW,
        )
        .unwrap();
        assert!(zh.contains("周邊裝置"), "{zh}");
        let none = state_line(&LightingView::NoColourInterface, &EN).unwrap();
        assert!(none.starts_with("Better OS found no way"), "{none}");
        assert_ne!(
            state_line(&LightingView::Unavailable(Access::Absent), &EN).unwrap(),
            EN.one_colour
        );
    }

    #[test]
    fn an_older_service_says_it_needs_updating() {
        let line = state_line(&LightingView::Unavailable(Access::Outdated), &EN).unwrap();
        assert!(line.contains("Update it"), "{line}");
        assert_ne!(line, EN.no_service);
    }

    #[test]
    fn the_lamp_array_note_says_why_there_is_no_effect() {
        let lamp = live(Interface::LampArray);
        assert!(note(&lamp, &EN).contains("Breathing and colour cycle aren't offered"));
        assert!(note(&lamp, &EN).contains("doesn't fake them"));
        assert!(note(&lamp, &ZH_TW).contains("沒有呼吸和色彩循環"));
        let asus = note(&live(Interface::AsusWmi), &EN);
        assert!(asus.contains("runs the effect itself"));
        assert!(asus.contains("next time the PC starts"), "{asus}");
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
        // The keyboard's own lighting on kbd_rgb_mode says when it comes.
        let mut next = live(Interface::AsusWmi);
        next.firmware_at_next_start = true;
        assert_eq!(
            live_description(
                &next,
                Some(&LightingChoice::starting(Effect::Firmware)),
                &EN
            ),
            "The keyboard's own lighting, from the next start"
        );
        assert_eq!(zones(1, &EN), "1 zone");
        assert_eq!(zones(24, &ZH_TW), "24 個區域");
    }

    #[test]
    fn the_details_row_says_what_the_keyboard_reported() {
        let device = LightingDevice {
            interface: Interface::LampArray,
            bus: Some(0x18),
            vendor: Some(0x0B05),
            product: Some(0x5570),
        };
        let description = Description {
            lamps: 1,
            kind: Some(LampArrayKind::Keyboard),
            min_update_interval_us: None,
            levels: Levels {
                red: 255,
                green: 255,
                blue: 255,
                intensity: 1,
            },
            programmable: true,
        };
        assert_eq!(
            device_line(&device, Some(&description), &EN),
            "Lighting device 0018:0B05:5570 · 1 zone · keyboard · colour levels 255/255/255, brightness levels 1"
        );
        assert!(device_line(&device, Some(&description), &ZH_TW).contains("鍵盤"));
        assert_eq!(
            device_line(&LightingDevice::ASUS_WMI, None, &EN),
            EN.device_asus_wmi
        );
    }
}
