//! Finding the touchpads and reading what the hardware can actually do.
//!
//! Two kernel interfaces are used, and neither alone is enough:
//!
//! - `/proc/bus/input/devices` lists every input device with its identity and
//!   its capability bitmaps. That is where the touchpad is recognised and where
//!   the contact count comes from.
//! - `/sys/class/input/<handler>` says whether that device is still there and
//!   whether it has been inhibited. A device that vanished between the two
//!   reads is reported as disconnected rather than presented as usable.
//!
//! Identity never rests on the event node. `event5` is whatever the kernel
//! enumerated fifth this boot; a device that is unplugged and plugged back in
//! comes back somewhere else. The identity used here is the device's own
//! `Uniq`, and failing that the bus, vendor, product, version, and name — all
//! of which survive re-enumeration.

use std::fs;
use std::path::Path;

use touchpad_core::SettingId;

use crate::roots::Roots;

/// The kernel constants this file reads. Named rather than inlined, because a
/// bare `0x14d` in a bitmap test is unreviewable.
mod bits {
    /// `INPUT_PROP_POINTER`
    pub const PROP_POINTER: u32 = 0;
    /// `INPUT_PROP_BUTTONPAD` — a pad whose whole surface is the button.
    pub const PROP_BUTTONPAD: u32 = 2;
    /// `INPUT_PROP_SEMI_MT` — reports a bounding box, not real contacts.
    pub const PROP_SEMI_MT: u32 = 3;

    /// `EV_KEY`
    pub const EV_KEY: u32 = 1;
    /// `EV_ABS`
    pub const EV_ABS: u32 = 3;

    pub const BTN_LEFT: u32 = 0x110;
    pub const BTN_MIDDLE: u32 = 0x112;
    pub const BTN_TOOL_FINGER: u32 = 0x145;
    pub const BTN_TOUCH: u32 = 0x14a;
    pub const BTN_TOOL_DOUBLETAP: u32 = 0x14d;
    pub const BTN_TOOL_TRIPLETAP: u32 = 0x14e;
    pub const BTN_TOOL_QUADTAP: u32 = 0x14f;
    pub const BTN_TOOL_QUINTTAP: u32 = 0x148;

    pub const ABS_X: u32 = 0x00;
    pub const ABS_Y: u32 = 0x01;
    /// `ABS_MT_SLOT` — present only on a real multitouch device.
    pub const ABS_MT_SLOT: u32 = 0x2f;
}

/// A capability bitmap as `/proc/bus/input/devices` prints it: 64-bit words in
/// hexadecimal, most significant word first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Bitmap {
    words: Vec<u64>,
}

impl Bitmap {
    pub fn parse(text: &str) -> Self {
        let mut words: Vec<u64> = text
            .split_whitespace()
            .map(|word| u64::from_str_radix(word, 16).unwrap_or(0))
            .collect();
        // Printed high word first; indexing is easier low word first.
        words.reverse();
        Self { words }
    }

    pub fn has(&self, bit: u32) -> bool {
        let index = (bit / 64) as usize;
        self.words
            .get(index)
            .is_some_and(|word| word & (1u64 << (bit % 64)) != 0)
    }
}

/// What this particular pad can do, independent of any backend.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeviceCapabilities {
    pub pointer: bool,
    /// The whole surface is the button, so there are no separate hardware
    /// buttons to click.
    pub buttonpad: bool,
    /// Reports a bounding box rather than individual contacts.
    pub semi_multitouch: bool,
    pub multitouch: bool,
    /// The highest number of simultaneous contacts the pad reports.
    pub max_contacts: u8,
    pub physical_middle_button: bool,
}

impl DeviceCapabilities {
    fn read(properties: &Bitmap, keys: &Bitmap, absolutes: &Bitmap) -> Self {
        let max_contacts = if keys.has(bits::BTN_TOOL_QUINTTAP) {
            5
        } else if keys.has(bits::BTN_TOOL_QUADTAP) {
            4
        } else if keys.has(bits::BTN_TOOL_TRIPLETAP) {
            3
        } else if keys.has(bits::BTN_TOOL_DOUBLETAP) {
            2
        } else if keys.has(bits::BTN_TOOL_FINGER) {
            1
        } else {
            0
        };
        Self {
            pointer: properties.has(bits::PROP_POINTER),
            buttonpad: properties.has(bits::PROP_BUTTONPAD),
            semi_multitouch: properties.has(bits::PROP_SEMI_MT),
            multitouch: absolutes.has(bits::ABS_MT_SLOT),
            max_contacts,
            physical_middle_button: keys.has(bits::BTN_MIDDLE),
        }
    }

    /// The settings this hardware cannot do, whatever the backend says.
    ///
    /// A backend intersects this with its own key table, so a control is only
    /// shown when both the pad and the backend can carry it. Everything else is
    /// unavailable with the reason attached.
    pub fn limits(&self) -> Vec<(SettingId, &'static str, String)> {
        let mut limits = Vec::new();
        if self.max_contacts < 2 {
            limits.push((
                SettingId::TwoFingerScrolling,
                "touchpad.pad_reports_one_contact",
                format!(
                    "this pad reports at most {} contact(s), so two-finger scrolling has nothing to detect",
                    self.max_contacts
                ),
            ));
        }
        if self.physical_middle_button {
            limits.push((
                SettingId::MiddleClickEmulation,
                "touchpad.pad_has_a_middle_button",
                "this pad has a real middle button, so there is nothing to emulate".to_string(),
            ));
        }
        if !self.buttonpad {
            limits.push((
                SettingId::ClickMethod,
                "touchpad.pad_has_separate_buttons",
                "this pad has separate hardware buttons, so there is only one click method"
                    .to_string(),
            ));
        }
        limits
    }
}

/// Whether the device found in `/proc` is still present in `/sys`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceState {
    Connected,
    /// Listed by the kernel's input core but with no `/sys` node, which is what
    /// a device being removed mid-read looks like.
    Disconnected,
    /// Present but inhibited, so it produces no events at all.
    Inhibited,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TouchpadDevice {
    /// Stable across re-plugging and re-enumeration. Never an event node.
    pub identity: String,
    pub name: String,
    pub bus: String,
    pub vendor: String,
    pub product: String,
    pub version: String,
    pub physical_path: String,
    pub unique_id: String,
    /// The event node this boot. Diagnostics only; nothing keys off it.
    pub event_handler: Option<String>,
    pub state: DeviceState,
    pub capabilities: DeviceCapabilities,
}

impl TouchpadDevice {
    pub fn is_usable(&self) -> bool {
        self.state == DeviceState::Connected
    }

    /// A short line for the Devices screen.
    pub fn describe(&self) -> String {
        format!(
            "{} ({}:{} on {})",
            self.name, self.vendor, self.product, self.bus
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Record {
    bus: String,
    vendor: String,
    product: String,
    version: String,
    name: String,
    physical_path: String,
    unique_id: String,
    handlers: Vec<String>,
    properties: Bitmap,
    events: Bitmap,
    keys: Bitmap,
    absolutes: Bitmap,
}

impl Record {
    /// The kernel's own definition of a touchpad, as libinput applies it: an
    /// absolute pointing device that reports a finger. The name is not used,
    /// because a name is a marketing string and some pads do not carry the
    /// word at all.
    fn is_touchpad(&self) -> bool {
        self.events.has(bits::EV_ABS)
            && self.events.has(bits::EV_KEY)
            && self.absolutes.has(bits::ABS_X)
            && self.absolutes.has(bits::ABS_Y)
            && self.keys.has(bits::BTN_TOOL_FINGER)
            && (self.keys.has(bits::BTN_TOUCH) || self.keys.has(bits::BTN_LEFT))
    }

    /// The identity a per-device configuration is filed under.
    fn identity(&self) -> String {
        if !self.unique_id.is_empty() {
            return format!("uniq:{}", self.unique_id);
        }
        format!(
            "input:{}:{}:{}:{}:{}",
            self.bus, self.vendor, self.product, self.version, self.name
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeviceInventory {
    pub devices: Vec<TouchpadDevice>,
    /// Why nothing was found, when nothing was found.
    pub problem: Option<String>,
}

impl DeviceInventory {
    /// The device a configuration selects, or the automatic choice.
    ///
    /// Automatic means the first connected touchpad in kernel order, which is
    /// stable within a boot and is the same one GNOME itself would drive.
    pub fn select(&self, wanted: Option<&str>) -> Option<&TouchpadDevice> {
        match wanted {
            Some(identity) => self
                .devices
                .iter()
                .find(|device| device.identity == identity),
            None => self.devices.iter().find(|device| device.is_usable()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

/// Reads every touchpad the kernel knows about.
pub fn enumerate(roots: &Roots) -> DeviceInventory {
    let path = roots.proc("bus/input/devices");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            return DeviceInventory {
                devices: Vec::new(),
                problem: Some(format!("{}: {error}", path.display())),
            };
        }
    };

    let devices = parse_records(&text)
        .into_iter()
        .filter(Record::is_touchpad)
        .map(|record| {
            let event_handler = record
                .handlers
                .iter()
                .find(|handler| handler.starts_with("event"))
                .cloned();
            let state = device_state(roots, event_handler.as_deref());
            TouchpadDevice {
                identity: record.identity(),
                capabilities: DeviceCapabilities::read(
                    &record.properties,
                    &record.keys,
                    &record.absolutes,
                ),
                name: record.name.clone(),
                bus: record.bus.clone(),
                vendor: record.vendor.clone(),
                product: record.product.clone(),
                version: record.version.clone(),
                physical_path: record.physical_path.clone(),
                unique_id: record.unique_id.clone(),
                event_handler,
                state,
            }
        })
        .collect::<Vec<_>>();

    let problem = devices
        .is_empty()
        .then(|| "no device in /proc/bus/input/devices looks like a touchpad".to_string());
    DeviceInventory { devices, problem }
}

fn device_state(roots: &Roots, handler: Option<&str>) -> DeviceState {
    let Some(handler) = handler else {
        return DeviceState::Disconnected;
    };
    let node = roots.sys(&format!("class/input/{handler}"));
    if !node.exists() {
        return DeviceState::Disconnected;
    }
    if reads_one(&node.join("device/inhibited")) {
        return DeviceState::Inhibited;
    }
    DeviceState::Connected
}

fn reads_one(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|text| text.trim() == "1")
        .unwrap_or(false)
}

fn parse_records(text: &str) -> Vec<Record> {
    let mut records = Vec::new();
    let mut current = Record::default();
    let mut started = false;

    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if started {
                records.push(std::mem::take(&mut current));
                started = false;
            }
            continue;
        }
        started = true;
        let Some((tag, rest)) = line.split_once(": ") else {
            continue;
        };
        let rest = rest.trim();
        match tag {
            "I" => {
                for field in rest.split_whitespace() {
                    match field.split_once('=') {
                        Some(("Bus", value)) => current.bus = value.to_string(),
                        Some(("Vendor", value)) => current.vendor = value.to_string(),
                        Some(("Product", value)) => current.product = value.to_string(),
                        Some(("Version", value)) => current.version = value.to_string(),
                        _ => {}
                    }
                }
            }
            "N" => {
                current.name = rest
                    .strip_prefix("Name=")
                    .unwrap_or(rest)
                    .trim_matches('"')
                    .to_string();
            }
            "P" => current.physical_path = rest.strip_prefix("Phys=").unwrap_or("").to_string(),
            "U" => current.unique_id = rest.strip_prefix("Uniq=").unwrap_or("").to_string(),
            "H" => {
                current.handlers = rest
                    .strip_prefix("Handlers=")
                    .unwrap_or("")
                    .split_whitespace()
                    .map(str::to_string)
                    .collect();
            }
            "B" => match rest.split_once('=') {
                Some(("PROP", value)) => current.properties = Bitmap::parse(value),
                Some(("EV", value)) => current.events = Bitmap::parse(value),
                Some(("KEY", value)) => current.keys = Bitmap::parse(value),
                Some(("ABS", value)) => current.absolutes = Bitmap::parse(value),
                _ => {}
            },
            _ => {}
        }
    }
    if started {
        records.push(current);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Roots {
        Roots::at(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
    }

    #[test]
    fn a_bitmap_is_read_low_word_first_however_it_was_printed() {
        let keys = Bitmap::parse("e520 10000 0 0 0 0");
        assert!(keys.has(bits::BTN_TOOL_FINGER));
        assert!(keys.has(bits::BTN_TOOL_DOUBLETAP));
        assert!(keys.has(bits::BTN_TOOL_TRIPLETAP));
        assert!(keys.has(bits::BTN_TOOL_QUADTAP));
        assert!(keys.has(bits::BTN_TOUCH));
        assert!(keys.has(bits::BTN_LEFT));
        // This pad reports five contacts; the fifth is what makes the bit at
        // 0x148 set, and it sits below the four-finger bit rather than above.
        assert!(keys.has(bits::BTN_TOOL_QUINTTAP));
        assert!(!keys.has(bits::BTN_MIDDLE));
    }

    #[test]
    fn a_bit_past_the_end_of_a_bitmap_is_absent_rather_than_a_panic() {
        assert!(!Bitmap::parse("3").has(4096));
        assert!(!Bitmap::default().has(0));
        // A word that is not hexadecimal reads as zero rather than stopping
        // the whole enumeration.
        assert!(!Bitmap::parse("zzzz").has(0));
    }

    #[test]
    fn the_laptop_fixture_yields_one_touchpad_with_its_real_capabilities() {
        let inventory = enumerate(&fixture("one-touchpad"));
        assert_eq!(inventory.devices.len(), 1, "{inventory:?}");
        let device = &inventory.devices[0];

        assert_eq!(device.name, "ASCF1200:00 2808:0233 Touchpad");
        assert_eq!(device.event_handler.as_deref(), Some("event5"));
        assert_eq!(device.state, DeviceState::Connected);
        assert_eq!(
            device.capabilities,
            DeviceCapabilities {
                pointer: true,
                buttonpad: true,
                semi_multitouch: false,
                multitouch: true,
                max_contacts: 5,
                physical_middle_button: false,
            }
        );
    }

    #[test]
    fn a_keyboard_and_a_mouse_are_not_mistaken_for_touchpads() {
        let inventory = enumerate(&fixture("one-touchpad"));
        assert!(
            inventory
                .devices
                .iter()
                .all(|device| device.name.contains("Touchpad"))
        );
    }

    #[test]
    fn identity_comes_from_the_device_and_not_from_its_event_node() {
        let inventory = enumerate(&fixture("one-touchpad"));
        let identity = &inventory.devices[0].identity;
        assert_eq!(
            identity,
            "input:0018:2808:0233:0000:ASCF1200:00 2808:0233 Touchpad"
        );
        assert!(!identity.contains("event"));
    }

    #[test]
    fn a_device_that_reports_a_unique_id_is_filed_under_it() {
        let inventory = enumerate(&fixture("two-touchpads"));
        let usb = inventory
            .devices
            .iter()
            .find(|device| device.name.contains("Magic"))
            .expect("the fixture has a second pad");
        assert_eq!(usb.identity, "uniq:a0:b1:c2:d3:e4:f5");
    }

    #[test]
    fn a_device_with_no_sys_node_is_disconnected_rather_than_usable() {
        let inventory = enumerate(&fixture("two-touchpads"));
        let gone = inventory
            .devices
            .iter()
            .find(|device| device.name.contains("Magic"))
            .unwrap();
        assert_eq!(gone.state, DeviceState::Disconnected);
        assert!(!gone.is_usable());
    }

    #[test]
    fn an_inhibited_device_is_its_own_state_and_not_simply_connected() {
        let inventory = enumerate(&fixture("inhibited"));
        assert_eq!(inventory.devices[0].state, DeviceState::Inhibited);
        assert!(!inventory.devices[0].is_usable());
    }

    #[test]
    fn automatic_selection_skips_a_device_that_is_not_connected() {
        let inventory = enumerate(&fixture("two-touchpads"));
        assert_eq!(inventory.devices.len(), 2);
        let selected = inventory.select(None).expect("one pad is connected");
        assert!(selected.name.contains("ASCF1200"));
    }

    #[test]
    fn an_explicit_selection_finds_the_named_device_even_when_it_is_gone() {
        let inventory = enumerate(&fixture("two-touchpads"));
        let selected = inventory.select(Some("uniq:a0:b1:c2:d3:e4:f5")).unwrap();
        assert_eq!(selected.state, DeviceState::Disconnected);
        assert!(inventory.select(Some("uniq:nothing-like-this")).is_none());
    }

    #[test]
    fn a_semi_multitouch_pad_reports_the_controls_it_cannot_carry() {
        let inventory = enumerate(&fixture("semi-mt"));
        let device = &inventory.devices[0];
        assert!(device.capabilities.semi_multitouch);
        assert_eq!(device.capabilities.max_contacts, 1);
        assert!(!device.capabilities.multitouch);

        let limits = device.capabilities.limits();
        let two_finger = limits
            .iter()
            .find(|(setting, _, _)| *setting == SettingId::TwoFingerScrolling)
            .expect("a one-contact pad cannot two-finger scroll");
        assert_eq!(two_finger.1, "touchpad.pad_reports_one_contact");
        assert!(two_finger.2.contains("1 contact"));
    }

    #[test]
    fn a_pad_with_a_real_middle_button_is_not_offered_middle_click_emulation() {
        let inventory = enumerate(&fixture("semi-mt"));
        let limits = inventory.devices[0].capabilities.limits();
        assert!(limits.iter().any(|(setting, reason, _)| *setting
            == SettingId::MiddleClickEmulation
            && *reason == "touchpad.pad_has_a_middle_button"));
    }

    #[test]
    fn a_missing_proc_file_is_reported_rather_than_read_as_no_touchpad() {
        let inventory = enumerate(&Roots::at("/nonexistent/snapshot"));
        assert!(inventory.is_empty());
        assert!(
            inventory
                .problem
                .as_deref()
                .unwrap_or_default()
                .contains("bus/input/devices")
        );
    }

    #[test]
    fn a_truncated_record_is_skipped_instead_of_stopping_the_enumeration() {
        let inventory = enumerate(&fixture("truncated"));
        // The half-written first record has no capability lines, so it is not
        // a touchpad; the complete second one still comes back.
        assert_eq!(inventory.devices.len(), 1);
        assert!(inventory.devices[0].name.contains("ASCF1200"));
    }
}
