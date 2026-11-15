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

/// How much of a carried offset is kept each frame once two-finger mode ends:
/// a 1000-unit offset is gone in five frames (about 40 ms at a pad's rate).
const CONVERGE: f64 = 0.25;

/// What one axis of one contact does this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    /// Two-finger scrolling on this axis: movement is multiplied.
    Scale(f64),
    /// Two fingers down but not scrolling on this axis (a pinch or a
    /// rotation): movement passes as it is, and the offset is kept.
    Keep,
    /// Not two-finger mode: the offset shrinks back to nothing.
    Converge,
}

/// One axis of one contact: the raw position, and the offset scaling added.
#[derive(Clone, Copy, Debug, Default)]
struct Track {
    raw: Option<i32>,
    offset: f64,
}

impl Track {
    fn apply(&mut self, raw: i32, motion: Motion, range: (i32, i32)) -> i32 {
        match (self.raw, motion) {
            (None, _) => self.offset = 0.0,
            (Some(last), Motion::Scale(factor)) => {
                self.offset += f64::from(raw - last) * (factor - 1.0);
            }
            (Some(_), Motion::Keep) => {}
            (Some(_), Motion::Converge) => self.converge(),
        }
        self.raw = Some(raw);
        self.position(range)
    }

    fn converge(&mut self) {
        self.offset *= CONVERGE;
        if self.offset.abs() < 0.5 {
            self.offset = 0.0;
        }
    }

    /// The position the clone shows, kept on the pad: an offset that would
    /// leave it is cut back, so it never builds up past the edge.
    fn position(&mut self, (minimum, maximum): (i32, i32)) -> i32 {
        let Some(raw) = self.raw else { return 0 };
        let shown = (f64::from(raw) + self.offset).clamp(f64::from(minimum), f64::from(maximum));
        self.offset = shown - f64::from(raw);
        shown.round() as i32
    }

    fn carries_offset(&self) -> bool {
        self.raw.is_some() && self.offset != 0.0
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
        let (body, report) = match frame.split_last() {
            Some((last, body)) if last.is_report() => (body, Some(*last)),
            _ => (frame, None),
        };
        // The frame's finger count is the one after its key events.
        let two_fingers = {
            let mut doubletap = self.keys_down.contains(&BTN_TOOL_DOUBLETAP);
            for event in body {
                if event.kind == EV_KEY && event.code == BTN_TOOL_DOUBLETAP {
                    doubletap = event.value != 0;
                }
            }
            doubletap
        };
        let (horizontal, vertical) = if two_fingers {
            self.two_finger_motion(body, factors)
        } else {
            (Motion::Converge, Motion::Converge)
        };

        let mut moved = vec![(false, false); self.contacts.len()];
        let mut legacy_moved = (false, false);
        for event in body {
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
                            moved[slot].0 = true;
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
                            moved[slot].1 = true;
                            self.contacts[slot]
                                .y
                                .apply(event.value, vertical, self.mt_y_range)
                        }
                        None => event.value,
                    };
                    out.push(InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, value));
                }
                (EV_ABS, ABS_X) => {
                    legacy_moved.0 = true;
                    let value = self.legacy.x.apply(event.value, horizontal, self.x_range);
                    out.push(InputEvent::new(EV_ABS, ABS_X, value));
                }
                (EV_ABS, ABS_Y) => {
                    legacy_moved.1 = true;
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
        if !two_fingers {
            self.converge_resting(&moved, legacy_moved, out);
        }
        if let Some(report) = report {
            out.push(report);
        }
    }

    /// Once two-finger mode ends, a finger that rests (sends no position)
    /// still has to be brought back to where it really is: a click on a
    /// clickpad is placed in a software-button zone by the finger's position.
    /// Each frame moves every resting contact's shown position a step closer.
    fn converge_resting(
        &mut self,
        moved: &[(bool, bool)],
        legacy_moved: (bool, bool),
        out: &mut Vec<InputEvent>,
    ) {
        let multi = self.contacts.len() > 1;
        let current = self.slot;
        let mut switched = false;
        for (index, contact) in self.contacts.iter_mut().enumerate() {
            if !contact.active {
                continue;
            }
            let (moved_x, moved_y) = moved[index];
            let x = !moved_x && contact.x.carries_offset();
            let y = !moved_y && contact.y.carries_offset();
            if !x && !y {
                continue;
            }
            if multi && current != Some(index) {
                out.push(InputEvent::new(EV_ABS, ABS_MT_SLOT, index as i32));
                switched = true;
            }
            if x {
                contact.x.converge();
                let value = contact.x.position(self.mt_x_range);
                out.push(InputEvent::new(EV_ABS, ABS_MT_POSITION_X, value));
            }
            if y {
                contact.y.converge();
                let value = contact.y.position(self.mt_y_range);
                out.push(InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, value));
            }
        }
        if switched && let Some(slot) = current {
            out.push(InputEvent::new(EV_ABS, ABS_MT_SLOT, slot as i32));
        }
        if !legacy_moved.0 && self.legacy.x.carries_offset() {
            self.legacy.x.converge();
            let value = self.legacy.x.position(self.x_range);
            out.push(InputEvent::new(EV_ABS, ABS_X, value));
        }
        if !legacy_moved.1 && self.legacy.y.carries_offset() {
            self.legacy.y.converge();
            let value = self.legacy.y.position(self.y_range);
            out.push(InputEvent::new(EV_ABS, ABS_Y, value));
        }
    }

    /// Which axes of a two-finger frame are a scroll. An axis is scaled only
    /// when both contacts move along it in the same direction: fingers moving
    /// toward or away from each other are a pinch or a rotation, and scaling
    /// them, with two different factors at that, would change the pinch's
    /// scale and angle. A pad that reports no separate contacts (single
    /// touch) cannot tell, and is scaled.
    fn two_finger_motion(&self, body: &[InputEvent], factors: Factors) -> (Motion, Motion) {
        if self.contacts.len() <= 1 {
            return (
                Motion::Scale(factors.horizontal),
                Motion::Scale(factors.vertical),
            );
        }
        let mut slot = self.slot;
        let mut delta: Vec<(i32, i32)> = vec![(0, 0); self.contacts.len()];
        let mut fresh = vec![false; self.contacts.len()];
        for event in body {
            match (event.kind, event.code) {
                (EV_ABS, ABS_MT_SLOT) => {
                    slot = usize::try_from(event.value)
                        .ok()
                        .filter(|slot| *slot < self.contacts.len());
                }
                (EV_ABS, ABS_MT_TRACKING_ID) => {
                    if let Some(slot) = slot {
                        fresh[slot] = true;
                    }
                }
                (EV_ABS, ABS_MT_POSITION_X) => {
                    if let Some(slot) = slot
                        && let Some(last) = self.contacts[slot].x.raw
                        && !fresh[slot]
                    {
                        delta[slot].0 = event.value - last;
                    }
                }
                (EV_ABS, ABS_MT_POSITION_Y) => {
                    if let Some(slot) = slot
                        && let Some(last) = self.contacts[slot].y.raw
                        && !fresh[slot]
                    {
                        delta[slot].1 = event.value - last;
                    }
                }
                _ => {}
            }
        }
        let active: Vec<usize> = (0..self.contacts.len())
            .filter(|index| self.contacts[*index].active && !fresh[*index])
            .collect();
        let axis = |pick: fn(&(i32, i32)) -> i32, factor: f64| match active.as_slice() {
            [a, b] => {
                let (da, db) = (pick(&delta[*a]), pick(&delta[*b]));
                if da != 0 && db != 0 && da.signum() == db.signum() {
                    Motion::Scale(factor)
                } else {
                    Motion::Keep
                }
            }
            _ => Motion::Keep,
        };
        (
            axis(|d| d.0, factors.horizontal),
            axis(|d| d.1, factors.vertical),
        )
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
                    abs(ABS_MT_POSITION_X, 1030),
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
        assert_eq!(values(&frames[1], ABS_MT_POSITION_X), vec![1015, 1415]);
    }

    #[test]
    fn neutral_factors_change_nothing() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let frames = two_finger_scroll(&mut scaler, Factors::NEUTRAL);
        assert_eq!(values(&frames[1], ABS_MT_POSITION_Y), vec![1100, 1100]);
        assert_eq!(values(&frames[1], ABS_MT_POSITION_X), vec![1030, 1430]);
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

    /// Both fingers move to `y` in one frame.
    fn both_move(scaler: &mut Scaler, y: i32, factors: Factors) -> Vec<InputEvent> {
        run(
            scaler,
            vec![
                abs(ABS_MT_SLOT, 0),
                abs(ABS_MT_POSITION_Y, y),
                abs(ABS_MT_SLOT, 1),
                abs(ABS_MT_POSITION_Y, y),
            ],
            factors,
        )
    }

    #[test]
    fn a_click_after_a_scroll_lands_where_the_finger_really_is() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = Factors::new(5.0, 1.0).unwrap();
        two_finger_scroll(&mut scaler, factors);
        // Slot 0 is at raw 1100 and shown at 1500. The second finger lifts;
        // the first rests in the bottom area to click, sending no position.
        let lift = run(
            &mut scaler,
            vec![
                abs(ABS_MT_SLOT, 1),
                abs(ABS_MT_TRACKING_ID, -1),
                key(BTN_TOOL_DOUBLETAP, 0),
                key(BTN_TOOL_FINGER, 1),
            ],
            factors,
        );
        let mut shown = values(&lift, ABS_MT_POSITION_Y);
        // Frames with only a timestamp bring the resting finger back.
        for stamp in 0..6 {
            let out = run(
                &mut scaler,
                vec![InputEvent::new(EV_MSC, 5, stamp)],
                factors,
            );
            shown.extend(values(&out, ABS_MT_POSITION_Y));
        }
        assert!(shown.windows(2).all(|pair| pair[1] <= pair[0]), "{shown:?}");
        assert_eq!(*shown.last().unwrap(), 1100, "{shown:?}");
        // The click is reported at the real position: nothing more moves.
        let click = run(&mut scaler, vec![key(BTN_LEFT, 1)], factors);
        assert!(values(&click, ABS_MT_POSITION_Y).is_empty());
        // The converging events name slot 0 and then return to slot 1, the
        // slot the source is on.
        assert!(lift.contains(&abs(ABS_MT_SLOT, 0)));
        let slots: Vec<_> = lift.iter().filter(|e| e.code == ABS_MT_SLOT).collect();
        assert_eq!(slots.last(), Some(&&abs(ABS_MT_SLOT, 1)));
    }

    #[test]
    fn a_pinch_keeps_its_scale_and_angle_under_unequal_factors() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = Factors::new(3.0, 0.5).unwrap();
        two_finger_scroll(&mut scaler, Factors::NEUTRAL);
        // Slot 0 at (1030, 1100), slot 1 at (1430, 1100). They spread apart
        // and twist: x apart, y opposite.
        let out = run(
            &mut scaler,
            vec![
                abs(ABS_MT_SLOT, 0),
                abs(ABS_MT_POSITION_X, 980),
                abs(ABS_MT_POSITION_Y, 1140),
                abs(ABS_MT_SLOT, 1),
                abs(ABS_MT_POSITION_X, 1480),
                abs(ABS_MT_POSITION_Y, 1060),
            ],
            factors,
        );
        assert_eq!(values(&out, ABS_MT_POSITION_X), vec![980, 1480]);
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![1140, 1060]);
    }

    #[test]
    fn only_the_axis_both_fingers_share_is_scaled() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = Factors::new(2.0, 2.0).unwrap();
        two_finger_scroll(&mut scaler, Factors::NEUTRAL);
        // Both down 50 (a scroll), x apart (a spread): y scaled, x not.
        let out = run(
            &mut scaler,
            vec![
                abs(ABS_MT_SLOT, 0),
                abs(ABS_MT_POSITION_X, 1020),
                abs(ABS_MT_POSITION_Y, 1150),
                abs(ABS_MT_SLOT, 1),
                abs(ABS_MT_POSITION_X, 1440),
                abs(ABS_MT_POSITION_Y, 1150),
            ],
            factors,
        );
        assert_eq!(values(&out, ABS_MT_POSITION_X), vec![1020, 1440]);
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![1200, 1200]);
    }

    #[test]
    fn a_shown_position_stops_at_the_edge_of_the_pad() {
        let mut scaler = Scaler::new(&multitouch_touchpad());
        let factors = Factors::new(5.0, 1.0).unwrap();
        two_finger_scroll(&mut scaler, factors);
        // Shown at 1500. 200 more raw is 1000 scaled, past 2000.
        let out = both_move(&mut scaler, 1300, factors);
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![2000, 2000]);
        // And coming back moves from the edge, not from beyond it.
        let out = both_move(&mut scaler, 1290, factors);
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![1950, 1950]);
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
            let out = both_move(&mut scaler, 1100 + step, factors);
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
        // One contact alone is never a scroll; what matters is that the
        // position was tracked as slot 3's and passed through.
        let out = run(
            &mut scaler,
            vec![abs(ABS_MT_POSITION_Y, 110)],
            twice_vertical(),
        );
        assert_eq!(values(&out, ABS_MT_POSITION_Y), vec![110]);
        assert_eq!(scaler.contacts[3].y.raw, Some(110));
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
