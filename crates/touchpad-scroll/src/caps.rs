//! Which device the pass-through may hold: the one rule.
//!
//! A process that can read a touchpad's node could as easily read a keyboard's,
//! so the decision of *which* node is made from what the device says it can
//! report, never from its name, and it refuses anything that can report a key.
//! The same function judges a device described by sysfs (before anything is
//! opened, from the maintainer script) and by the evdev ioctls (after the
//! service opened the node it was started for); see [`crate::sysfs`] and
//! [`crate::linux`].

use std::collections::BTreeMap;
use std::fmt;

use crate::codes::*;

/// A capability bitmap, as the kernel reports one.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Bits {
    words: Vec<u64>,
}

impl Bits {
    pub fn new() -> Self {
        Self::default()
    }

    /// From little-endian bytes, the layout `EVIOCGBIT` fills.
    pub fn from_le_bytes(bytes: &[u8]) -> Self {
        let mut bits = Self::new();
        for (index, byte) in bytes.iter().enumerate() {
            for bit in 0..8 {
                if byte & (1 << bit) != 0 {
                    bits.set((index * 8 + bit) as u16);
                }
            }
        }
        bits
    }

    pub fn set(&mut self, code: u16) {
        let word = usize::from(code) / 64;
        if self.words.len() <= word {
            self.words.resize(word + 1, 0);
        }
        self.words[word] |= 1 << (code % 64);
    }

    pub fn has(&self, code: u16) -> bool {
        self.words
            .get(usize::from(code) / 64)
            .is_some_and(|word| word & (1 << (code % 64)) != 0)
    }

    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.words.iter().enumerate().flat_map(|(index, word)| {
            (0..64u16)
                .filter(move |bit| word & (1 << bit) != 0)
                .map(move |bit| index as u16 * 64 + bit)
        })
    }

    pub fn with(codes: &[u16]) -> Self {
        let mut bits = Self::new();
        for code in codes {
            bits.set(*code);
        }
        bits
    }
}

/// `struct input_absinfo`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AbsInfo {
    pub value: i32,
    pub minimum: i32,
    pub maximum: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

/// `struct input_id`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputId {
    pub bustype: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

/// Everything the pass-through knows about a device: what `classify` judges
/// and what the uinput clone is built from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeviceDescriptor {
    pub name: String,
    pub phys: String,
    pub id: InputId,
    pub properties: Bits,
    pub events: Bits,
    pub keys: Bits,
    pub relative: Bits,
    pub absolute: Bits,
    pub misc: Bits,
    pub abs_info: BTreeMap<u16, AbsInfo>,
}

/// The `phys` every clone carries, and the prefix the pass-through refuses so
/// it never grabs its own output. The suffix is the integration the udev rule
/// gives the clone.
pub const PHYS_PREFIX: &str = "better-touchpad-scroll/";

impl DeviceDescriptor {
    /// Whether the original sits inside the machine. libinput pairs a touchpad
    /// with the keyboard for disable-while-typing only when both are internal,
    /// so the clone has to say the same thing the original did.
    pub fn integration(&self) -> &'static str {
        match self.id.bustype {
            BUS_USB | BUS_BLUETOOTH => "external",
            _ => "internal",
        }
    }

    pub fn clone_phys(&self) -> String {
        format!("{PHYS_PREFIX}{}", self.integration())
    }

    pub fn is_multitouch(&self) -> bool {
        self.absolute.has(ABS_MT_POSITION_X) && self.absolute.has(ABS_MT_POSITION_Y)
    }

    pub fn has_hi_res_wheel(&self) -> bool {
        self.relative.has(REL_WHEEL_HI_RES)
    }

    pub fn has_hi_res_hwheel(&self) -> bool {
        self.relative.has(REL_HWHEEL_HI_RES)
    }
}

/// Why a device is not held. Every variant is a reason to leave the device to
/// libinput exactly as it is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The device is a pass-through clone, ours.
    OwnClone,
    /// It can report a key, so it could be a keyboard or carry one.
    ReportsKey(u16),
    /// It reports an event type no touchpad needs: LEDs, autorepeat, switches,
    /// sound, force feedback.
    EventType(u16),
    /// It is a touchscreen or a tablet, not a touchpad.
    NotATouchpad(&'static str),
    /// A position axis has no range to clamp to.
    MissingAxisRange(u16),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnClone => write!(f, "the device is a Better Touchpad scroll clone"),
            Self::ReportsKey(code) => write!(
                f,
                "the device can report key code {code:#x}, so it is refused as a possible keyboard"
            ),
            Self::EventType(kind) => {
                write!(
                    f,
                    "the device reports event type {kind:#x}, which no touchpad needs"
                )
            }
            Self::NotATouchpad(why) => write!(f, "the device is not a touchpad: {why}"),
            Self::MissingAxisRange(code) => {
                write!(f, "the device reports axis {code:#x} without a range")
            }
        }
    }
}

/// Event types a touchpad may report. Anything else refuses the device.
const ALLOWED_EVENT_TYPES: [u16; 5] = [EV_SYN, EV_KEY, EV_REL, EV_ABS, EV_MSC];

/// Whether `device` is a touchpad the pass-through may hold.
///
/// The keyboard rule comes first and is absolute: a single key bit outside the
/// button block (`BTN_MISC..KEY_OK`) refuses the device, whatever else it is.
/// A touchpad reports buttons and finger tools, all of which are in that
/// block; a keyboard, a combined keyboard-and-touchpad receiver, a remote, or a
/// joystick does not fit in it.
pub fn classify(device: &DeviceDescriptor) -> Result<(), Refusal> {
    classify_capabilities(device)?;
    for axis in [ABS_X, ABS_Y, ABS_MT_POSITION_X, ABS_MT_POSITION_Y] {
        if device.absolute.has(axis) {
            match device.abs_info.get(&axis) {
                Some(info) if info.maximum > info.minimum => {}
                _ => return Err(Refusal::MissingAxisRange(axis)),
            }
        }
    }
    Ok(())
}

/// Everything [`classify`] checks except the axis ranges, which only the
/// opened device can report. This is what a sysfs description is judged by,
/// before any node is opened; the service judges the opened node again with
/// [`classify`].
pub fn classify_capabilities(device: &DeviceDescriptor) -> Result<(), Refusal> {
    if device.phys.starts_with(PHYS_PREFIX) {
        return Err(Refusal::OwnClone);
    }
    if let Some(code) = device
        .keys
        .iter()
        .find(|code| !(BTN_MISC..KEY_OK).contains(code))
    {
        return Err(Refusal::ReportsKey(code));
    }
    if let Some(kind) = device
        .events
        .iter()
        .find(|kind| !ALLOWED_EVENT_TYPES.contains(kind))
    {
        return Err(Refusal::EventType(kind));
    }
    if !device.events.has(EV_ABS) || !device.events.has(EV_KEY) {
        return Err(Refusal::NotATouchpad(
            "it reports no absolute position or no buttons",
        ));
    }
    if device.properties.has(INPUT_PROP_DIRECT) {
        return Err(Refusal::NotATouchpad(
            "it is a direct-input device (a touchscreen)",
        ));
    }
    if !device.keys.has(BTN_TOOL_FINGER) {
        return Err(Refusal::NotATouchpad("it reports no finger tool"));
    }
    if device.keys.has(BTN_TOOL_PEN) || device.keys.has(BTN_STYLUS) {
        return Err(Refusal::NotATouchpad("it reports a pen (a tablet)"));
    }
    if !device.absolute.has(ABS_X) || !device.absolute.has(ABS_Y) {
        return Err(Refusal::NotATouchpad("it reports no x and y position"));
    }
    Ok(())
}

/// What the uinput clone of `device` declares: the device's own capabilities,
/// judged again, under the clone's `phys`. A clone is only ever made from a
/// device [`classify`] accepts, so it can report no key outside the button
/// block, and the kernel drops any event whose code a uinput device did not
/// declare.
pub fn clone_descriptor(device: &DeviceDescriptor) -> Result<DeviceDescriptor, Refusal> {
    classify(device)?;
    let mut clone = device.clone();
    clone.phys = device.clone_phys();
    Ok(clone)
}

/// Test and benchmark descriptors. Public so the integration tests and the
/// benchmark build the same devices the unit tests do.
pub mod fixtures {
    use super::*;

    fn axis(minimum: i32, maximum: i32, resolution: i32) -> AbsInfo {
        AbsInfo {
            minimum,
            maximum,
            resolution,
            ..AbsInfo::default()
        }
    }

    /// A clickpad as an I2C HID multitouch touchpad reports itself.
    pub fn multitouch_touchpad() -> DeviceDescriptor {
        let mut abs_info = BTreeMap::new();
        abs_info.insert(ABS_X, axis(0, 3200, 31));
        abs_info.insert(ABS_Y, axis(0, 2000, 31));
        abs_info.insert(ABS_MT_SLOT, axis(0, 4, 0));
        abs_info.insert(ABS_MT_POSITION_X, axis(0, 3200, 31));
        abs_info.insert(ABS_MT_POSITION_Y, axis(0, 2000, 31));
        abs_info.insert(ABS_MT_TRACKING_ID, axis(0, 65535, 0));
        DeviceDescriptor {
            name: "SYNA2BA6:00 06CB:CEB0 Touchpad".to_string(),
            phys: "i2c-SYNA2BA6:00".to_string(),
            id: InputId {
                bustype: 0x18,
                vendor: 0x06cb,
                product: 0xceb0,
                version: 0x100,
            },
            properties: Bits::with(&[INPUT_PROP_POINTER, INPUT_PROP_BUTTONPAD]),
            events: Bits::with(&[EV_SYN, EV_KEY, EV_ABS, EV_MSC]),
            keys: Bits::with(&[
                BTN_LEFT,
                BTN_TOOL_FINGER,
                BTN_TOOL_QUINTTAP,
                BTN_TOUCH,
                BTN_TOOL_DOUBLETAP,
                BTN_TOOL_TRIPLETAP,
                BTN_TOOL_QUADTAP,
            ]),
            relative: Bits::new(),
            absolute: Bits::with(&[
                ABS_X,
                ABS_Y,
                ABS_MT_SLOT,
                ABS_MT_POSITION_X,
                ABS_MT_POSITION_Y,
                ABS_MT_TRACKING_ID,
            ]),
            misc: Bits::with(&[5]),
            abs_info,
        }
    }

    /// An older single-touch pad: `ABS_X`/`ABS_Y` and finger-count tools only.
    pub fn single_touch_touchpad() -> DeviceDescriptor {
        let mut device = multitouch_touchpad();
        device.name = "SynPS/2 Synaptics TouchPad".to_string();
        device.phys = "isa0060/serio1/input0".to_string();
        device.id.bustype = 0x11;
        for code in [
            ABS_MT_SLOT,
            ABS_MT_POSITION_X,
            ABS_MT_POSITION_Y,
            ABS_MT_TRACKING_ID,
        ] {
            device.abs_info.remove(&code);
        }
        device.absolute = Bits::with(&[ABS_X, ABS_Y]);
        device
    }

    /// A touchpad that also reports a high-resolution wheel, as a few
    /// touchpad-and-trackpoint combinations and some USB pads do.
    pub fn touchpad_with_wheel() -> DeviceDescriptor {
        let mut device = multitouch_touchpad();
        device.events.set(EV_REL);
        device.relative = Bits::with(&[REL_WHEEL, REL_HWHEEL, REL_WHEEL_HI_RES, REL_HWHEEL_HI_RES]);
        device
    }

    /// A laptop keyboard.
    pub fn keyboard() -> DeviceDescriptor {
        let mut keys = Bits::new();
        for code in 1..=248 {
            keys.set(code);
        }
        DeviceDescriptor {
            name: "AT Translated Set 2 keyboard".to_string(),
            phys: "isa0060/serio0/input0".to_string(),
            id: InputId {
                bustype: 0x11,
                vendor: 1,
                product: 1,
                version: 0xab83,
            },
            events: Bits::with(&[EV_SYN, EV_KEY, EV_MSC, EV_LED, EV_REP]),
            keys,
            ..DeviceDescriptor::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn a_multitouch_and_a_single_touch_touchpad_are_held() {
        assert_eq!(classify(&multitouch_touchpad()), Ok(()));
        assert_eq!(classify(&single_touch_touchpad()), Ok(()));
        assert_eq!(classify(&touchpad_with_wheel()), Ok(()));
    }

    #[test]
    fn a_keyboard_is_refused_by_its_first_key() {
        assert_eq!(classify(&keyboard()), Err(Refusal::ReportsKey(1)));
    }

    #[test]
    fn a_touchpad_that_also_reports_one_key_is_refused() {
        // A combined receiver (keyboard and touchpad on one node) looks like a
        // touchpad in every other respect. One letter key is enough to refuse.
        for key in [
            30u16, /* KEY_A */
            0xff, KEY_OK, KEY_MAX, 0x2c0, /* BTN_TRIGGER_HAPPY */
        ] {
            let mut device = multitouch_touchpad();
            device.keys.set(key);
            assert_eq!(classify(&device), Err(Refusal::ReportsKey(key)), "{key:#x}");
        }
    }

    #[test]
    fn a_keyboard_dressed_as_a_touchpad_is_still_a_keyboard() {
        // Every touchpad property, plus the keyboard's keys: the key rule wins
        // before anything that would accept it is looked at.
        let mut device = multitouch_touchpad();
        for code in keyboard().keys.iter() {
            device.keys.set(code);
        }
        device.name = "Touchpad".to_string();
        assert!(matches!(classify(&device), Err(Refusal::ReportsKey(_))));
    }

    #[test]
    fn leds_autorepeat_switches_and_force_feedback_refuse_a_device() {
        for kind in [EV_LED, EV_REP, EV_SW, EV_SND, EV_FF] {
            let mut device = multitouch_touchpad();
            device.events.set(kind);
            assert_eq!(classify(&device), Err(Refusal::EventType(kind)));
        }
    }

    #[test]
    fn touchscreens_and_tablets_are_not_touchpads() {
        let mut screen = multitouch_touchpad();
        screen.properties.set(INPUT_PROP_DIRECT);
        assert!(matches!(classify(&screen), Err(Refusal::NotATouchpad(_))));

        let mut tablet = multitouch_touchpad();
        tablet.keys.set(BTN_TOOL_PEN);
        assert!(matches!(classify(&tablet), Err(Refusal::NotATouchpad(_))));

        let mut mouse = multitouch_touchpad();
        mouse.keys = Bits::with(&[BTN_LEFT, BTN_RIGHT, BTN_MIDDLE]);
        assert!(matches!(classify(&mouse), Err(Refusal::NotATouchpad(_))));
    }

    #[test]
    fn the_pass_through_never_holds_its_own_clone() {
        let mut clone = multitouch_touchpad();
        clone.phys = multitouch_touchpad().clone_phys();
        assert_eq!(classify(&clone), Err(Refusal::OwnClone));
    }

    #[test]
    fn an_axis_without_a_range_is_refused() {
        let mut device = multitouch_touchpad();
        device.abs_info.remove(&ABS_MT_POSITION_Y);
        assert_eq!(
            classify(&device),
            Err(Refusal::MissingAxisRange(ABS_MT_POSITION_Y))
        );
    }

    #[test]
    fn the_clone_says_where_the_original_was() {
        assert_eq!(
            multitouch_touchpad().clone_phys(),
            "better-touchpad-scroll/internal"
        );
        let mut usb = multitouch_touchpad();
        usb.id.bustype = BUS_USB;
        assert_eq!(usb.clone_phys(), "better-touchpad-scroll/external");
    }

    #[test]
    fn a_clone_declares_no_key_and_cannot_be_made_from_a_keyboard() {
        let clone = clone_descriptor(&multitouch_touchpad()).unwrap();
        assert!(
            clone
                .keys
                .iter()
                .all(|code| (BTN_MISC..KEY_OK).contains(&code))
        );
        assert!(clone.phys.starts_with(PHYS_PREFIX));
        assert_eq!(clone.abs_info, multitouch_touchpad().abs_info);
        assert!(clone_descriptor(&keyboard()).is_err());
        // And a clone of a clone is refused, so the service never stacks one.
        assert_eq!(clone_descriptor(&clone), Err(Refusal::OwnClone));
    }

    #[test]
    fn bits_round_trip_through_the_ioctl_byte_layout() {
        let bits = Bits::from_le_bytes(&[0b0000_0101, 0, 0x80]);
        assert_eq!(bits.iter().collect::<Vec<_>>(), vec![0, 2, 23]);
        assert!(bits.has(23) && !bits.has(24) && !bits.has(9999));
    }
}
