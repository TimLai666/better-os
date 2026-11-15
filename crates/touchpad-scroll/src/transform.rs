//! How one frame is scaled.
//!
//! libinput recognises two-finger scrolling from the fingers' motion, so a
//! scroll factor is applied to that motion: while exactly two fingers are on
//! the pad (`BTN_TOOL_DOUBLETAP`), every contact's movement is multiplied, the
//! vertical factor on the y axes and the horizontal factor on the x axes.
//! One-finger pointer motion and three- and four-finger gestures are forwarded
//! unscaled.
//!
//! Positions are absolute, so the scaled position is carried per contact: a
//! contact's virtual position starts where the finger is and moves by the
//! scaled delta. When the contact leaves two-finger mode the offset it gained
//! is kept, unscaled, until the finger lifts, so the pointer never jumps. A
//! virtual position stops at the axis's edge; motion past it is lost, which a
//! factor above one can make reachable halfway across the pad.
//!
//! Wheels (`REL_WHEEL`, `REL_HWHEEL`, and their high-resolution forms) are
//! scaled with the remainder carried, so a factor of 0.5 turns two notches into
//! one rather than none. On a device with a high-resolution wheel the
//! low-resolution notch is regenerated from the scaled high-resolution stream,
//! the way the kernel derives it, so the two never disagree.
//!
//! Frames are processed whole, at `SYN_REPORT`, because the finger count of a
//! frame is only known once its key events have been seen, and nothing reads
//! a frame before its report anyway: buffering to the report adds no latency.

use std::collections::BTreeSet;

use crate::caps::DeviceDescriptor;
use crate::codes::*;
use crate::event::InputEvent;
use crate::factors::Factors;

/// One axis of one contact.
#[derive(Clone, Copy, Debug, Default)]
struct Track {
    raw: Option<i32>,
    virtual_position: f64,
}

impl Track {
    fn apply(&mut self, raw: i32, factor: f64, (minimum, maximum): (i32, i32)) -> i32 {
        match self.raw {
            None => self.virtual_position = f64::from(raw),
            Some(last) => {
                self.virtual_position = (self.virtual_position + f64::from(raw - last) * factor)
                    .clamp(f64::from(minimum), f64::from(maximum));
            }
        }
        self.raw = Some(raw);
        self.virtual_position.round() as i32
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Contact {
    active: bool,
    x: Track,
    y: Track,
}

/// One wheel axis.
#[derive(Clone, Copy, Debug, Default)]
struct Wheel {
    hi_res: bool,
    remainder: f64,
    notch: i32,
}

impl Wheel {
    fn reset_on_reversal(&mut self, value: i32) {
        let previous = if self.remainder != 0.0 {
            self.remainder.signum()
        } else {
            f64::from(self.notch.signum())
        };
        if previous != 0.0 && previous != f64::from(value.signum()) {
            self.remainder = 0.0;
            self.notch = 0;
        }
    }

    fn scale(&mut self, value: i32, factor: f64) -> i32 {
        self.reset_on_reversal(value);
        self.remainder += f64::from(value) * factor;
        let whole = self.remainder.trunc();
        self.remainder -= whole;
        whole as i32
    }

    /// A high-resolution event: the scaled hi-res value, and the notches it
    /// completes.
    fn hi_res(&mut self, value: i32, factor: f64) -> (i32, i32) {
        let scaled = self.scale(value, factor);
        self.notch += scaled;
        let notches = self.notch / 120;
        self.notch -= notches * 120;
        (scaled, notches)
    }
}

pub struct Scaler {
    contacts: Vec<Contact>,
    slot: Option<usize>,
    legacy: Contact,
    keys_down: BTreeSet<u16>,
    x_range: (i32, i32),
    y_range: (i32, i32),
    mt_x_range: (i32, i32),
    mt_y_range: (i32, i32),
    wheel: Wheel,
    hwheel: Wheel,
}

fn range(device: &DeviceDescriptor, axis: u16) -> (i32, i32) {
    device
        .abs_info
        .get(&axis)
        .map_or((i32::MIN, i32::MAX), |info| (info.minimum, info.maximum))
}

impl Scaler {
    pub fn new(device: &DeviceDescriptor) -> Self {
        let slots = device
            .abs_info
            .get(&ABS_MT_SLOT)
            .map_or(1, |info| (info.maximum.clamp(0, 63) + 1) as usize);
        Self {
            contacts: vec![Contact::default(); slots],
            slot: Some(0),
            legacy: Contact::default(),
            keys_down: BTreeSet::new(),
            x_range: range(device, ABS_X),
            y_range: range(device, ABS_Y),
            mt_x_range: range(device, ABS_MT_POSITION_X),
            mt_y_range: range(device, ABS_MT_POSITION_Y),
            wheel: Wheel {
                hi_res: device.has_hi_res_wheel(),
                ..Wheel::default()
            },
            hwheel: Wheel {
                hi_res: device.has_hi_res_hwheel(),
                ..Wheel::default()
            },
        }
    }

    /// Start a clone that is about to receive the source's stream from its
    /// current slot onwards. A source that has been reporting on slot 3 sends
    /// later events for slot 3 without naming it again, so the clone is told.
    pub fn start(&mut self, current_slot: i32, out: &mut Vec<InputEvent>) {
        self.set_slot(current_slot);
        if self.contacts.len() > 1 {
            out.push(InputEvent::new(EV_ABS, ABS_MT_SLOT, current_slot));
        }
    }

    fn set_slot(&mut self, value: i32) {
        self.slot = usize::try_from(value)
            .ok()
            .filter(|slot| *slot < self.contacts.len());
    }

    /// Whether the clone has a contact or a button down, as far as what was
    /// forwarded says. The grab is only released when this is false, so the
    /// desktop never sees a finger that never lifts.
    pub fn touching(&self) -> bool {
        !self.keys_down.is_empty() || self.contacts.iter().any(|contact| contact.active)
    }

    /// Lift every contact and release every key the clone holds, ending with a
    /// report. Used when a frame was lost (`SYN_DROPPED`) and before the clone
    /// is destroyed with something still down.
    pub fn lift_all(&mut self, out: &mut Vec<InputEvent>) {
        let current = self.slot;
        let multi = self.contacts.len() > 1;
        let mut lifted = false;
        for (index, contact) in self.contacts.iter_mut().enumerate() {
            if contact.active {
                if multi {
                    out.push(InputEvent::new(EV_ABS, ABS_MT_SLOT, index as i32));
                }
                out.push(InputEvent::new(EV_ABS, ABS_MT_TRACKING_ID, -1));
                lifted = true;
            }
            *contact = Contact::default();
        }
        if lifted
            && multi
            && let Some(slot) = current
        {
            out.push(InputEvent::new(EV_ABS, ABS_MT_SLOT, slot as i32));
        }
        for key in std::mem::take(&mut self.keys_down) {
            out.push(InputEvent::new(EV_KEY, key, 0));
        }
        self.legacy = Contact::default();
        out.push(InputEvent::syn());
    }

    /// Scale one frame. `frame` is every event up to and including its
    /// `SYN_REPORT`.
    pub fn frame(&mut self, frame: &[InputEvent], factors: Factors, out: &mut Vec<InputEvent>) {
        // The frame's finger count is the one after its key events.
        let two_fingers = {
            let mut doubletap = self.keys_down.contains(&BTN_TOOL_DOUBLETAP);
            for event in frame {
                if event.kind == EV_KEY && event.code == BTN_TOOL_DOUBLETAP {
                    doubletap = event.value != 0;
                }
            }
            doubletap
        };
        let (horizontal, vertical) = if two_fingers {
            (factors.horizontal, factors.vertical)
        } else {
            (1.0, 1.0)
        };

        for event in frame {
            match (event.kind, event.code) {
                (EV_ABS, ABS_MT_SLOT) => {
                    self.set_slot(event.value);
                    out.push(*event);
                }
                (EV_ABS, ABS_MT_TRACKING_ID) => {
                    if let Some(slot) = self.slot {
                        self.contacts[slot] = Contact {
                            active: event.value >= 0,
                            ..Contact::default()
                        };
                    }
                    out.push(*event);
                }
                (EV_ABS, ABS_MT_POSITION_X) => {
                    let value = match self.slot {
                        Some(slot) => {
                            self.contacts[slot]
                                .x
                                .apply(event.value, horizontal, self.mt_x_range)
                        }
                        None => event.value,
                    };
                    out.push(InputEvent::new(EV_ABS, ABS_MT_POSITION_X, value));
                }
                (EV_ABS, ABS_MT_POSITION_Y) => {
                    let value = match self.slot {
                        Some(slot) => {
                            self.contacts[slot]
                                .y
                                .apply(event.value, vertical, self.mt_y_range)
                        }
                        None => event.value,
                    };
                    out.push(InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, value));
                }
                (EV_ABS, ABS_X) => {
                    let value = self.legacy.x.apply(event.value, horizontal, self.x_range);
                    out.push(InputEvent::new(EV_ABS, ABS_X, value));
                }
                (EV_ABS, ABS_Y) => {
                    let value = self.legacy.y.apply(event.value, vertical, self.y_range);
                    out.push(InputEvent::new(EV_ABS, ABS_Y, value));
                }
                (EV_KEY, code) => {
                    if event.value != 0 {
                        self.keys_down.insert(code);
                    } else {
                        self.keys_down.remove(&code);
                        if code == BTN_TOUCH {
                            self.legacy = Contact::default();
                        }
                    }
                    out.push(*event);
                }
                (EV_REL, REL_WHEEL_HI_RES) => self.hi_res(
                    event.value,
                    factors.vertical,
                    REL_WHEEL_HI_RES,
                    REL_WHEEL,
                    out,
                ),
                (EV_REL, REL_HWHEEL_HI_RES) => self.hi_res(
                    event.value,
                    factors.horizontal,
                    REL_HWHEEL_HI_RES,
                    REL_HWHEEL,
                    out,
                ),
                (EV_REL, REL_WHEEL) => {
                    if !self.wheel.hi_res {
                        let value = self.wheel.scale(event.value, factors.vertical);
                        if value != 0 {
                            out.push(InputEvent::new(EV_REL, REL_WHEEL, value));
                        }
                    }
                }
                (EV_REL, REL_HWHEEL) => {
                    if !self.hwheel.hi_res {
                        let value = self.hwheel.scale(event.value, factors.horizontal);
                        if value != 0 {
                            out.push(InputEvent::new(EV_REL, REL_HWHEEL, value));
                        }
                    }
                }
                _ => out.push(*event),
            }
        }
    }

    fn hi_res(
        &mut self,
        value: i32,
        factor: f64,
        hi_code: u16,
        notch_code: u16,
        out: &mut Vec<InputEvent>,
    ) {
        let wheel = if hi_code == REL_WHEEL_HI_RES {
            &mut self.wheel
        } else {
            &mut self.hwheel
        };
        let (scaled, notches) = wheel.hi_res(value, factor);
        if notches != 0 {
            out.push(InputEvent::new(EV_REL, notch_code, notches));
        }
        if scaled != 0 {
            out.push(InputEvent::new(EV_REL, hi_code, scaled));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::fixtures::*;

    fn abs(code: u16, value: i32) -> InputEvent {
        InputEvent::new(EV_ABS, code, value)
    }

    fn key(code: u16, value: i32) -> InputEvent {
        InputEvent::new(EV_KEY, code, value)
    }

    fn rel(code: u16, value: i32) -> InputEvent {
        InputEvent::new(EV_REL, code, value)
    }

    fn run(scaler: &mut Scaler, frame: Vec<InputEvent>, factors: Factors) -> Vec<InputEvent> {
        let mut frame = frame;
        frame.push(InputEvent::syn());
        let mut out = Vec::new();
        scaler.frame(&frame, factors, &mut out);
        out
    }

    fn values(events: &[InputEvent], code: u16) -> Vec<i32> {
        events
            .iter()
            .filter(|event| event.kind == EV_ABS && event.code == code)
            .map(|event| event.value)
            .collect()
    }

    fn twice_vertical() -> Factors {
        Factors::new(2.0, 1.0).unwrap()
    }

    /// Two fingers down at y = 1000 and 1000, then both moving down 100.
    fn two_finger_scroll(scaler: &mut Scaler, factors: Factors) -> Vec<Vec<InputEvent>> {
        vec![
            run(
                scaler,
                vec![
                    abs(ABS_MT_SLOT, 0),
                    abs(ABS_MT_TRACKING_ID, 10),
                    abs(ABS_MT_POSITION_X, 1000),
                    abs(ABS_MT_POSITION_Y, 1000),
                    abs(ABS_MT_SLOT, 1),
                    abs(ABS_MT_TRACKING_ID, 11),
                    abs(ABS_MT_POSITION_X, 1400),
                    abs(ABS_MT_POSITION_Y, 1000),
                    key(BTN_TOUCH, 1),
                    key(BTN_TOOL_DOUBLETAP, 1),
                ],
                factors,
            ),
            run(
                scaler,
                vec![
                    abs(ABS_MT_SLOT, 0),
                    abs(ABS_MT_POSITION_Y, 1100),
                    abs(ABS_MT_SLOT, 1),
                    abs(ABS_MT_POSITION_X, 1430),
                    abs(ABS_MT_POSITION_Y, 1100),
                ],
                factors,
            ),
        ]
    }

    #[test]
    fn two_finger_motion_is_scaled_on_each_axis_by_its_own_factor() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let frames = two_finger_scroll(&mut scaler, Factors::new(2.0, 0.5).unwrap());
        // The first frame places the fingers where they are.
        assert_eq!(values(&frames[0], ABS_MT_POSITION_Y), vec![1000, 1000]);
        // Then 100 down becomes 200, and 30 across becomes 15.
        assert_eq!(values(&frames[1], ABS_MT_POSITION_Y), vec![1200, 1200]);
        assert_eq!(values(&frames[1], ABS_MT_POSITION_X), vec![1415]);
    }

    #[test]
    fn neutral_factors_change_nothing() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let frames = two_finger_scroll(&mut scaler, Factors::NEUTRAL);
        assert_eq!(values(&frames[1], ABS_MT_POSITION_Y), vec![1100, 1100]);
        assert_eq!(values(&frames[1], ABS_MT_POSITION_X), vec![1430]);
    }

    #[test]
    fn one_finger_pointer_motion_is_never_scaled() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = Factors::new(4.0, 4.0).unwrap();
        run(
            &mut scaler,
            vec![
                abs(ABS_MT_TRACKING_ID, 1),
                abs(ABS_MT_POSITION_X, 500),
                abs(ABS_MT_POSITION_Y, 500),
                key(BTN_TOUCH, 1),
                key(BTN_TOOL_FINGER, 1),
            ],
            factors,
        );
        let out = run(
            &mut scaler,
            vec![abs(ABS_MT_POSITION_X, 600), abs(ABS_MT_POSITION_Y, 450)],
            factors,
        );
        assert_eq!(values(&out, ABS_MT_POSITION_X), vec![600]);
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![450]);
    }

    #[test]
    fn three_and_four_finger_gestures_are_not_scaled() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = twice_vertical();
        run(
            &mut scaler,
            vec![
                abs(ABS_MT_TRACKING_ID, 1),
                abs(ABS_MT_POSITION_Y, 500),
                key(BTN_TOUCH, 1),
                key(BTN_TOOL_TRIPLETAP, 1),
            ],
            factors,
        );
        let out = run(&mut scaler, vec![abs(ABS_MT_POSITION_Y, 700)], factors);
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![700]);
    }

    #[test]
    fn the_offset_a_scroll_gained_is_kept_until_the_finger_lifts() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = twice_vertical();
        two_finger_scroll(&mut scaler, factors);
        // The second finger lifts; the first is at raw 1100, virtual 1200.
        run(
            &mut scaler,
            vec![
                abs(ABS_MT_SLOT, 1),
                abs(ABS_MT_TRACKING_ID, -1),
                key(BTN_TOOL_DOUBLETAP, 0),
                key(BTN_TOOL_FINGER, 1),
            ],
            factors,
        );
        // One finger moving 10 moves the pointer 10, from where it was shown,
        // so the pointer does not jump back 100.
        let out = run(
            &mut scaler,
            vec![abs(ABS_MT_SLOT, 0), abs(ABS_MT_POSITION_Y, 1110)],
            factors,
        );
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![1210]);
        // A new contact in that slot starts where its finger is.
        run(&mut scaler, vec![abs(ABS_MT_TRACKING_ID, -1)], factors);
        let out = run(
            &mut scaler,
            vec![abs(ABS_MT_TRACKING_ID, 12), abs(ABS_MT_POSITION_Y, 300)],
            factors,
        );
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![300]);
    }

    #[test]
    fn a_virtual_position_stops_at_the_edge_of_the_pad() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = Factors::new(5.0, 1.0).unwrap();
        two_finger_scroll(&mut scaler, factors);
        // Virtual is 1500. 200 more raw is 1000 scaled, past 2000.
        let out = run(
            &mut scaler,
            vec![abs(ABS_MT_SLOT, 0), abs(ABS_MT_POSITION_Y, 1300)],
            factors,
        );
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![2000]);
        // And coming back moves from the edge, not from beyond it.
        let out = run(&mut scaler, vec![abs(ABS_MT_POSITION_Y, 1290)], factors);
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![1950]);
    }

    #[test]
    fn a_single_touch_pad_is_scaled_on_its_abs_x_and_abs_y() {
        let mut scaler = Scaler::new(&single_touch_touchpad());
        let factors = Factors::new(3.0, 2.0).unwrap();
        run(
            &mut scaler,
            vec![
                abs(ABS_X, 1000),
                abs(ABS_Y, 1000),
                key(BTN_TOUCH, 1),
                key(BTN_TOOL_DOUBLETAP, 1),
            ],
            factors,
        );
        let out = run(
            &mut scaler,
            vec![abs(ABS_X, 1010), abs(ABS_Y, 990)],
            factors,
        );
        assert_eq!(values(&out, ABS_X), vec![1020]);
        assert_eq!(values(&out, ABS_Y), vec![970]);
        // Lifting resets it.
        run(
            &mut scaler,
            vec![key(BTN_TOUCH, 0), key(BTN_TOOL_DOUBLETAP, 0)],
            factors,
        );
        let out = run(&mut scaler, vec![abs(ABS_X, 400)], factors);
        assert_eq!(values(&out, ABS_X), vec![400]);
    }

    #[test]
    fn fractional_motion_is_carried_rather_than_rounded_away() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = Factors::new(0.3, 1.0).unwrap();
        two_finger_scroll(&mut scaler, Factors::NEUTRAL);
        let mut last = 0;
        for step in 1..=10 {
            let out = run(
                &mut scaler,
                vec![abs(ABS_MT_SLOT, 0), abs(ABS_MT_POSITION_Y, 1100 + step)],
                factors,
            );
            last = values(&out, ABS_MT_POSITION_Y)[0];
        }
        // Ten single-unit moves at 0.3 are three units, not zero.
        assert_eq!(last, 1103);
    }

    #[test]
    fn the_hi_res_wheel_is_scaled_and_its_notches_follow_it() {
        let mut scaler = Scaler::new(&touchpad_with_wheel());
        let factors = Factors::new(0.5, 2.0).unwrap();
        // The device sends the notch and the hi-res value for each detent.
        let mut hi = Vec::new();
        let mut notches = Vec::new();
        for _ in 0..4 {
            let out = run(
                &mut scaler,
                vec![rel(REL_WHEEL, 1), rel(REL_WHEEL_HI_RES, 120)],
                factors,
            );
            for event in out {
                match event.code {
                    REL_WHEEL_HI_RES => hi.push(event.value),
                    REL_WHEEL if event.kind == EV_REL => notches.push(event.value),
                    _ => {}
                }
            }
        }
        // Four detents at half speed: 60 each, and one notch per 120.
        assert_eq!(hi, vec![60, 60, 60, 60]);
        assert_eq!(notches, vec![1, 1]);

        let out = run(
            &mut scaler,
            vec![rel(REL_HWHEEL, -1), rel(REL_HWHEEL_HI_RES, -120)],
            factors,
        );
        assert_eq!(
            out,
            vec![
                rel(REL_HWHEEL, -2),
                rel(REL_HWHEEL_HI_RES, -240),
                InputEvent::syn()
            ]
        );
    }

    #[test]
    fn a_fine_hi_res_stream_keeps_its_remainder() {
        let mut scaler = Scaler::new(&touchpad_with_wheel());
        let factors = Factors::new(0.25, 1.0).unwrap();
        let mut total = 0;
        for _ in 0..16 {
            let out = run(&mut scaler, vec![rel(REL_WHEEL_HI_RES, 15)], factors);
            total += out
                .iter()
                .filter(|event| event.code == REL_WHEEL_HI_RES && event.kind == EV_REL)
                .map(|event| event.value)
                .sum::<i32>();
        }
        assert_eq!(total, 60);
    }

    #[test]
    fn reversing_the_wheel_drops_the_old_remainder() {
        let mut scaler = Scaler::new(&touchpad_with_wheel());
        let factors = Factors::new(0.5, 1.0).unwrap();
        // 60 forward leaves 60 toward the next notch.
        run(&mut scaler, vec![rel(REL_WHEEL_HI_RES, 120)], factors);
        // Reversing must not need 180 back before the first notch back.
        let out = run(&mut scaler, vec![rel(REL_WHEEL_HI_RES, -240)], factors);
        assert!(out.contains(&rel(REL_WHEEL, -1)), "{out:?}");
    }

    #[test]
    fn a_low_res_wheel_carries_its_remainder() {
        let mut device = multitouch_touchpad();
        device.events.set(EV_REL);
        device.relative = crate::caps::Bits::with(&[REL_WHEEL]);
        let mut scaler = Scaler::new(&device);
        let factors = Factors::new(0.5, 1.0).unwrap();
        let first = run(&mut scaler, vec![rel(REL_WHEEL, 1)], factors);
        let second = run(&mut scaler, vec![rel(REL_WHEEL, 1)], factors);
        assert_eq!(first, vec![InputEvent::syn()]);
        assert_eq!(second, vec![rel(REL_WHEEL, 1), InputEvent::syn()]);
    }

    #[test]
    fn everything_else_passes_through_untouched() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let frame = vec![
            InputEvent::new(EV_MSC, 5, 123_456),
            abs(0x3a /* ABS_MT_PRESSURE */, 40),
            key(BTN_LEFT, 1),
        ];
        let out = run(&mut scaler, frame.clone(), twice_vertical());
        assert_eq!(&out[..3], &frame[..]);
    }

    #[test]
    fn lifting_everything_releases_every_contact_and_key() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        two_finger_scroll(&mut scaler, twice_vertical());
        assert!(scaler.touching());
        let mut out = Vec::new();
        scaler.lift_all(&mut out);
        assert!(!scaler.touching());
        assert_eq!(
            out,
            vec![
                abs(ABS_MT_SLOT, 0),
                abs(ABS_MT_TRACKING_ID, -1),
                abs(ABS_MT_SLOT, 1),
                abs(ABS_MT_TRACKING_ID, -1),
                abs(ABS_MT_SLOT, 1),
                key(BTN_TOUCH, 0),
                key(BTN_TOOL_DOUBLETAP, 0),
                InputEvent::syn(),
            ]
        );
    }

    #[test]
    fn the_clone_is_told_which_slot_the_source_is_on() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let mut out = Vec::new();
        scaler.start(3, &mut out);
        assert_eq!(out, vec![abs(ABS_MT_SLOT, 3)]);
        // A position with no slot event is slot 3's.
        run(
            &mut scaler,
            vec![
                abs(ABS_MT_TRACKING_ID, 5),
                abs(ABS_MT_POSITION_Y, 100),
                key(BTN_TOOL_DOUBLETAP, 1),
            ],
            twice_vertical(),
        );
        let out = run(
            &mut scaler,
            vec![abs(ABS_MT_POSITION_Y, 110)],
            twice_vertical(),
        );
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![120]);
    }

    #[test]
    fn a_slot_the_device_does_not_have_is_forwarded_unscaled() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let out = run(
            &mut scaler,
            vec![
                abs(ABS_MT_SLOT, 40),
                abs(ABS_MT_POSITION_Y, 77),
                key(BTN_TOOL_DOUBLETAP, 1),
            ],
            twice_vertical(),
        );
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![77]);
    }
}
