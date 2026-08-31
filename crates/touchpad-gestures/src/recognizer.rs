//! Turning contact-point frames into gesture events.
//!
//! This is business logic, not compositor integration. The recognizer is handed
//! a stream of frames — each one a timestamp and a list of labelled contact
//! points in normalized pad coordinates — and it emits begin, progress,
//! threshold-crossed, complete, and cancel events. Where the frames come from
//! is somebody else's problem, and deliberately so: a replayed stream in a
//! test, a synthetic stream in Test gestures mode, and whatever backend ADR
//! 0012 eventually chooses all produce the same three facts, so all three drive
//! the same code.
//!
//! Four properties are load-bearing.
//!
//! **It holds no clock.** Every frame carries its own timestamp, so the
//! cooldown is replayable rather than timing-dependent. A cooldown tested with
//! a sleep is a flaky test.
//!
//! **Nothing is emitted until a gesture arms.** A hand resting on the pad, or a
//! few pixels of drift, produces no events at all. Without that rule a partial
//! gesture flaps a surface open and shut, which Issue #3 forbids.
//!
//! **The value at release decides.** A gesture that went past the activation
//! threshold and came back before the fingers lifted cancels; it does not
//! commit. This is the same rule `launcher-platform`'s recognizer applies, and
//! the two must agree because they are two halves of one interaction.
//!
//! **A contact count is never a weaker version of another one.** Changing the
//! number of fingers mid-gesture cancels rather than degrading into a different
//! gesture, and nothing is recognized again until every contact lifts.

use std::collections::BTreeMap;

use crate::definition::{Direction, GestureDefinition, GestureId, GestureShape};

/// What a contact is, as far as the frame source could tell.
///
/// Labelling is the source's job. A source that cannot tell a thumb from a
/// finger reports every contact as [`ContactRole::Finger`], and gestures that
/// require a thumb are then never recognized — which is the honest outcome, and
/// the reason `thumb_required` is a stored field rather than an inference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContactRole {
    #[default]
    Finger,
    Thumb,
}

/// One contact, in normalized pad coordinates: `0.0..=1.0` on each axis, with
/// `y` increasing downwards, so a device of any size produces the same numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactPoint {
    pub id: u32,
    pub role: ContactRole,
    pub x: f32,
    pub y: f32,
}

impl ContactPoint {
    pub fn finger(id: u32, x: f32, y: f32) -> Self {
        Self {
            id,
            role: ContactRole::Finger,
            x,
            y,
        }
    }

    pub fn thumb(id: u32, x: f32, y: f32) -> Self {
        Self {
            id,
            role: ContactRole::Thumb,
            x,
            y,
        }
    }
}

/// Everything touching the pad at one instant.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchFrame {
    /// Milliseconds on the stream's own monotonic scale.
    pub at_ms: u64,
    pub contacts: Vec<ContactPoint>,
}

impl TouchFrame {
    pub fn new(at_ms: u64, contacts: Vec<ContactPoint>) -> Self {
        Self { at_ms, contacts }
    }

    /// Every finger lifted. A stream that never ends with one of these has not
    /// finished, and the recognizer will not commit anything.
    pub fn lifted(at_ms: u64) -> Self {
        Self {
            at_ms,
            contacts: Vec::new(),
        }
    }

    fn has_thumb(&self) -> bool {
        self.contacts
            .iter()
            .any(|contact| contact.role == ContactRole::Thumb)
    }

    fn centroid(&self) -> (f32, f32) {
        let count = self.contacts.len() as f32;
        let sum = self.contacts.iter().fold((0.0, 0.0), |sum, contact| {
            (sum.0 + contact.x, sum.1 + contact.y)
        });
        (sum.0 / count, sum.1 / count)
    }

    fn radius(&self) -> f32 {
        let (x, y) = self.centroid();
        let count = self.contacts.len() as f32;
        self.contacts
            .iter()
            .map(|contact| ((contact.x - x).powi(2) + (contact.y - y).powi(2)).sqrt())
            .sum::<f32>()
            / count
    }
}

/// How much movement counts as a whole gesture.
///
/// These are ADR 0012's recorded starting values, in fractions of the pad. They
/// are not a tuned curve — Issue #3 explicitly defers the curve — and they live
/// in one struct so that tuning them later is one edit, one test, and no search
/// through the recognizer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecognizerScale {
    /// The travel a swipe needs to be complete, as a fraction of the pad.
    pub swipe_travel: f32,
    /// The change in mean radius a pinch or spread needs to be complete.
    pub pinch_travel: f32,
    /// How long a hold takes.
    pub hold_ms: u64,
    /// The longest a tap may last.
    pub tap_ms: u64,
    /// The furthest a tap or a hold may drift and still count.
    pub tap_travel: f32,
    /// The turn a rotate needs to be complete, in radians.
    pub rotate_turn: f32,
    /// How far a gesture must get before anything is emitted at all.
    pub arm_progress: f32,
}

impl Default for RecognizerScale {
    fn default() -> Self {
        Self {
            swipe_travel: 0.18,
            pinch_travel: 0.06,
            hold_ms: 500,
            tap_ms: 200,
            tap_travel: 0.02,
            rotate_turn: 0.5,
            arm_progress: 0.10,
        }
    }
}

/// What happened to a gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureEventKind {
    /// The gesture is under way. Emitted once, on the frame that arms it.
    Begin,
    Progress,
    /// The frame on which progress first reached the activation threshold.
    /// This is the superset `launcher-platform`'s four phases do not have; it
    /// maps onto `Update` there and exists here so a surface can commit to an
    /// animation at the moment the gesture becomes a decision.
    ThresholdCrossed,
    Complete,
    Cancel,
}

impl GestureEventKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Progress => "progress",
            Self::ThresholdCrossed => "threshold-crossed",
            Self::Complete => "complete",
            Self::Cancel => "cancel",
        }
    }

    /// The launcher's vocabulary. `ThresholdCrossed` is an update there,
    /// because a threshold is a policy decision and the launcher makes its own.
    pub fn phase(self) -> touchpad_session::GesturePhase {
        match self {
            Self::Begin => touchpad_session::GesturePhase::Begin,
            Self::Progress | Self::ThresholdCrossed => touchpad_session::GesturePhase::Update,
            Self::Complete => touchpad_session::GesturePhase::End,
            Self::Cancel => touchpad_session::GesturePhase::Cancel,
        }
    }
}

/// One thing the recognizer decided.
#[derive(Clone, Debug, PartialEq)]
pub struct GestureEvent {
    pub gesture: GestureId,
    pub kind: GestureEventKind,
    pub progress: f32,
    pub at_ms: u64,
}

impl GestureEvent {
    /// The progress report an adapter is invoked with.
    pub fn progress_report(&self) -> touchpad_session::GestureProgress {
        touchpad_session::GestureProgress::new(self.kind.phase(), self.progress)
    }
}

/// What the frame stream looked like, rather than what the fingers did.
///
/// Issue #3 asks for dropped or reordered gesture frames to be counted, and a
/// count only means something if somebody keeps it. A backend that stalls or
/// delivers out of order is a real failure mode — it is what a busy compositor
/// looks like — and this is how it shows up on the Diagnostics screen instead
/// of as a gesture that mysteriously did not fire.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameHealth {
    pub seen: u64,
    /// Frames whose timestamp went backwards. Counted and dropped.
    pub reordered: u64,
    /// An estimate of how many frames never arrived, from the gaps between the
    /// ones that did.
    pub dropped: u64,
    last_at_ms: Option<u64>,
}

impl FrameHealth {
    /// The interval a frame is expected at: about 60 Hz, which is what a
    /// touchpad reports and what a compositor forwards. A gap of two intervals
    /// counts as one dropped frame.
    pub const GAP_MS: u64 = 32;

    pub fn is_clean(&self) -> bool {
        self.reordered == 0 && self.dropped == 0
    }
}

/// Where a gesture started.
#[derive(Clone, Debug)]
struct Origin {
    at_ms: u64,
    centroid: (f32, f32),
    radius: f32,
    points: Vec<(u32, f32, f32)>,
    contacts: usize,
}

impl Origin {
    fn of(frame: &TouchFrame) -> Self {
        Self {
            at_ms: frame.at_ms,
            centroid: frame.centroid(),
            radius: frame.radius(),
            points: frame
                .contacts
                .iter()
                .map(|contact| (contact.id, contact.x, contact.y))
                .collect(),
            contacts: frame.contacts.len(),
        }
    }
}

/// The gesture currently under way.
#[derive(Clone, Debug)]
struct Active {
    index: usize,
    crossed: bool,
    peak: f32,
    last: f32,
}

/// Recognizes the configured gestures over a frame stream.
pub struct Recognizer {
    definitions: Vec<GestureDefinition>,
    scale: RecognizerScale,
    origin: Option<Origin>,
    candidates: Vec<usize>,
    active: Option<Active>,
    /// Set when a touch can no longer produce anything, and cleared only when
    /// every contact lifts. This is what stops a cancelled gesture from
    /// re-arming under the same fingers.
    suppressed: bool,
    cooldown_until: BTreeMap<GestureId, u64>,
    health: FrameHealth,
}

impl Recognizer {
    pub fn new(definitions: Vec<GestureDefinition>) -> Self {
        Self::with_scale(definitions, RecognizerScale::default())
    }

    pub fn with_scale(definitions: Vec<GestureDefinition>, scale: RecognizerScale) -> Self {
        Self {
            definitions,
            scale,
            origin: None,
            candidates: Vec::new(),
            active: None,
            suppressed: false,
            cooldown_until: BTreeMap::new(),
            health: FrameHealth::default(),
        }
    }

    pub fn definitions(&self) -> &[GestureDefinition] {
        &self.definitions
    }

    pub fn scale(&self) -> RecognizerScale {
        self.scale
    }

    /// Whether this gesture would be ignored right now because it just fired.
    pub fn in_cooldown(&self, id: &GestureId, at_ms: u64) -> bool {
        self.cooldown_until
            .get(id)
            .is_some_and(|until| at_ms < *until)
    }

    /// Runs a whole stream. The shape every replay test and Test gestures mode
    /// uses.
    pub fn replay(&mut self, frames: &[TouchFrame]) -> Vec<GestureEvent> {
        frames
            .iter()
            .flat_map(|frame| self.observe(frame))
            .collect()
    }

    /// What the frame stream itself has looked like.
    pub fn health(&self) -> FrameHealth {
        self.health
    }

    /// Applies one frame.
    ///
    /// A frame that arrives out of order is counted and dropped rather than
    /// processed: applying it would move the gesture backwards and could
    /// cancel a gesture the user is still making. A gap longer than
    /// [`FrameHealth::GAP_MS`] is counted as dropped frames, because a
    /// recognizer that silently interpolates over a stall is one that reports
    /// a gesture the hand did not make.
    pub fn observe(&mut self, frame: &TouchFrame) -> Vec<GestureEvent> {
        self.health.seen += 1;
        if let Some(previous) = self.health.last_at_ms {
            if frame.at_ms < previous {
                self.health.reordered += 1;
                return Vec::new();
            }
            let gap = frame.at_ms - previous;
            if gap > FrameHealth::GAP_MS {
                self.health.dropped += gap / FrameHealth::GAP_MS;
            }
        }
        self.health.last_at_ms = Some(frame.at_ms);

        if frame.contacts.is_empty() {
            return self.release(frame.at_ms);
        }
        if self.suppressed {
            return Vec::new();
        }
        match &self.origin {
            None => {
                self.begin_touch(frame);
                Vec::new()
            }
            Some(origin) if origin.contacts != frame.contacts.len() => {
                // A different number of fingers is a different gesture, never a
                // weaker version of this one.
                let events = self.cancel_active(frame.at_ms);
                self.suppressed = true;
                self.origin = None;
                self.candidates.clear();
                events
            }
            Some(_) => self.advance(frame),
        }
    }

    fn begin_touch(&mut self, frame: &TouchFrame) {
        let origin = Origin::of(frame);
        let count = frame.contacts.len() as u8;
        let has_thumb = frame.has_thumb();

        let matching: Vec<usize> = self
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, gesture)| {
                gesture.enabled
                    && gesture.contacts.get() == count
                    && !self.in_cooldown(&gesture.id, frame.at_ms)
            })
            .map(|(index, _)| index)
            .collect();

        // A labelled thumb decides which family of same-count gestures is even
        // considered. Thumb-and-three and four-fingers are both four contacts,
        // and they must not be able to trigger each other.
        let with_thumb: Vec<usize> = matching
            .iter()
            .copied()
            .filter(|index| self.definitions[*index].thumb_required)
            .collect();
        let without_thumb: Vec<usize> = matching
            .iter()
            .copied()
            .filter(|index| !self.definitions[*index].thumb_required)
            .collect();

        self.candidates = if has_thumb && !with_thumb.is_empty() {
            with_thumb
        } else {
            without_thumb
        };
        self.suppressed = self.candidates.is_empty();
        self.origin = Some(origin);
    }

    fn advance(&mut self, frame: &TouchFrame) -> Vec<GestureEvent> {
        let origin = self.origin.clone().expect("advance runs with an origin");
        let mut events = Vec::new();

        let Some(active) = self.active.clone() else {
            // Nothing has armed yet. The first candidate to get far enough
            // wins, in definition order, so the outcome does not depend on
            // floating-point ties.
            for index in self.candidates.clone() {
                let progress = self.progress_of(index, &origin, frame);
                if progress >= self.scale.arm_progress {
                    let gesture = &self.definitions[index];
                    let crossed = progress >= gesture.activation_threshold.get();
                    events.push(GestureEvent {
                        gesture: gesture.id.clone(),
                        kind: GestureEventKind::Begin,
                        progress,
                        at_ms: frame.at_ms,
                    });
                    if crossed {
                        events.push(GestureEvent {
                            gesture: gesture.id.clone(),
                            kind: GestureEventKind::ThresholdCrossed,
                            progress,
                            at_ms: frame.at_ms,
                        });
                    }
                    self.active = Some(Active {
                        index,
                        crossed,
                        peak: progress,
                        last: progress,
                    });
                    break;
                }
            }
            return events;
        };

        let progress = self.progress_of(active.index, &origin, frame);
        let gesture = &self.definitions[active.index];
        let activation = gesture.activation_threshold.get();
        let cancellation = gesture.cancellation_threshold.get();
        let id = gesture.id.clone();

        // Reversing counts only once the gesture had actually got somewhere;
        // otherwise every gesture would cancel on the frame after it armed.
        if active.peak > cancellation && progress <= cancellation {
            self.active = None;
            self.suppressed = true;
            events.push(GestureEvent {
                gesture: id,
                kind: GestureEventKind::Cancel,
                progress,
                at_ms: frame.at_ms,
            });
            return events;
        }

        events.push(GestureEvent {
            gesture: id.clone(),
            kind: GestureEventKind::Progress,
            progress,
            at_ms: frame.at_ms,
        });
        let crossed = active.crossed || progress >= activation;
        if crossed && !active.crossed {
            events.push(GestureEvent {
                gesture: id,
                kind: GestureEventKind::ThresholdCrossed,
                progress,
                at_ms: frame.at_ms,
            });
        }
        self.active = Some(Active {
            index: active.index,
            crossed,
            peak: active.peak.max(progress),
            last: progress,
        });
        events
    }

    /// Every contact lifted.
    fn release(&mut self, at_ms: u64) -> Vec<GestureEvent> {
        let mut events = Vec::new();
        match self.active.take() {
            Some(active) => {
                let gesture = &self.definitions[active.index];
                let id = gesture.id.clone();
                let activation = gesture.activation_threshold.get();
                let cooldown = gesture.cooldown.as_millis();
                // The value at release decides. A gesture that went far and
                // came back does not commit.
                if active.last >= activation {
                    self.cooldown_until.insert(id.clone(), at_ms + cooldown);
                    events.push(GestureEvent {
                        gesture: id,
                        kind: GestureEventKind::Complete,
                        progress: active.last,
                        at_ms,
                    });
                } else {
                    events.push(GestureEvent {
                        gesture: id,
                        kind: GestureEventKind::Cancel,
                        progress: active.last,
                        at_ms,
                    });
                }
            }
            None => {
                if !self.suppressed {
                    events.extend(self.tap_on_release(at_ms));
                }
            }
        }
        self.origin = None;
        self.candidates.clear();
        self.suppressed = false;
        events
    }

    /// A tap has no progress to watch, so it is decided entirely at release:
    /// short enough, and it never travelled.
    fn tap_on_release(&mut self, at_ms: u64) -> Vec<GestureEvent> {
        let Some(origin) = self.origin.clone() else {
            return Vec::new();
        };
        for index in self.candidates.clone() {
            let gesture = &self.definitions[index];
            if gesture.shape != GestureShape::Tap {
                continue;
            }
            if at_ms.saturating_sub(origin.at_ms) > self.scale.tap_ms {
                continue;
            }
            let id = gesture.id.clone();
            let cooldown = gesture.cooldown.as_millis();
            self.cooldown_until.insert(id.clone(), at_ms + cooldown);
            // Begin and Complete arrive together: a tap is one instant, and a
            // consumer that only ever saw `Complete` would have no begin to
            // pair it with.
            return vec![
                GestureEvent {
                    gesture: id.clone(),
                    kind: GestureEventKind::Begin,
                    progress: 1.0,
                    at_ms,
                },
                GestureEvent {
                    gesture: id,
                    kind: GestureEventKind::Complete,
                    progress: 1.0,
                    at_ms,
                },
            ];
        }
        Vec::new()
    }

    /// Cancels whatever is under way because the source said the gesture was
    /// cancelled.
    ///
    /// This is not a second cancellation rule. The threshold rules stay exactly
    /// where they are; this is the case where the *source* is authoritative — a
    /// compositor that reports a cancelled gesture has already decided the
    /// fingers did not do it — and there is no frame that could express it.
    /// Nothing is recognized again until the next gesture begins, which is the
    /// same suppression a mid-gesture contact-count change produces.
    pub fn cancel(&mut self, at_ms: u64) -> Vec<GestureEvent> {
        let events = self.cancel_active(at_ms);
        self.origin = None;
        self.candidates.clear();
        self.suppressed = false;
        events
    }

    fn cancel_active(&mut self, at_ms: u64) -> Vec<GestureEvent> {
        match self.active.take() {
            Some(active) => vec![GestureEvent {
                gesture: self.definitions[active.index].id.clone(),
                kind: GestureEventKind::Cancel,
                progress: active.last,
                at_ms,
            }],
            None => Vec::new(),
        }
    }

    fn progress_of(&self, index: usize, origin: &Origin, frame: &TouchFrame) -> f32 {
        let gesture = &self.definitions[index];
        let scale = self.scale;
        let value = match gesture.shape {
            GestureShape::Swipe => {
                let (x, y) = frame.centroid();
                let dx = x - origin.centroid.0;
                let dy = y - origin.centroid.1;
                let travelled = match gesture.direction {
                    Some(Direction::Up) => -dy,
                    Some(Direction::Down) => dy,
                    Some(Direction::Left) => -dx,
                    Some(Direction::Right) => dx,
                    // A swipe without a direction cannot exist: the definition
                    // is refused at construction.
                    _ => 0.0,
                };
                travelled / scale.swipe_travel
            }
            GestureShape::Pinch => (origin.radius - frame.radius()) / scale.pinch_travel,
            GestureShape::Spread => (frame.radius() - origin.radius) / scale.pinch_travel,
            GestureShape::Hold => {
                if drift(origin, frame) > scale.tap_travel {
                    0.0
                } else {
                    frame.at_ms.saturating_sub(origin.at_ms) as f32 / scale.hold_ms.max(1) as f32
                }
            }
            // Decided at release, never while touching.
            GestureShape::Tap => 0.0,
            GestureShape::Rotate => {
                let turned = rotation(origin, frame);
                let signed = match gesture.direction {
                    Some(Direction::CounterClockwise) => -turned,
                    _ => turned,
                };
                signed / scale.rotate_turn
            }
        };
        value.clamp(0.0, 1.0)
    }
}

/// How far the centroid moved from where it started.
fn drift(origin: &Origin, frame: &TouchFrame) -> f32 {
    let (x, y) = frame.centroid();
    ((x - origin.centroid.0).powi(2) + (y - origin.centroid.1).powi(2)).sqrt()
}

/// The mean turn of the contacts about their own centre, in radians, positive
/// clockwise in screen coordinates.
fn rotation(origin: &Origin, frame: &TouchFrame) -> f32 {
    let (cx, cy) = frame.centroid();
    let mut total = 0.0;
    let mut counted = 0.0;
    for contact in &frame.contacts {
        let Some((_, ox, oy)) = origin
            .points
            .iter()
            .find(|(id, _, _)| *id == contact.id)
            .copied()
        else {
            continue;
        };
        let before = (oy - origin.centroid.1).atan2(ox - origin.centroid.0);
        let now = (contact.y - cy).atan2(contact.x - cx);
        let mut delta = now - before;
        while delta > std::f32::consts::PI {
            delta -= 2.0 * std::f32::consts::PI;
        }
        while delta < -std::f32::consts::PI {
            delta += 2.0 * std::f32::consts::PI;
        }
        total += delta;
        counted += 1.0;
    }
    if counted == 0.0 { 0.0 } else { total / counted }
}

/// Synthetic frame streams.
///
/// These are how Test gestures mode shows a recognition without touching the
/// hardware, and how every replay test states what the fingers did. They are
/// shipped code rather than test helpers because the screen runs them.
pub mod synthetic {
    use super::*;

    /// The frames for one gesture, carried `completion` of the way to a whole
    /// gesture and then lifted.
    ///
    /// `completion` is in units of the recognizer's own scale, so `1.0` is a
    /// complete gesture and `0.4` is one abandoned well short of any sensible
    /// activation threshold.
    pub fn perform(
        gesture: &GestureDefinition,
        completion: f32,
        scale: RecognizerScale,
    ) -> Vec<TouchFrame> {
        travel(gesture, 0.0, completion, scale, 8, 16)
    }

    /// A gesture carried out to `peak` and then brought back to `end` before the
    /// fingers lift. This is the reversal Issue #3 requires to cancel.
    pub fn perform_and_reverse(
        gesture: &GestureDefinition,
        peak: f32,
        end: f32,
        scale: RecognizerScale,
    ) -> Vec<TouchFrame> {
        let mut frames = travel(gesture, 0.0, peak, scale, 8, 16);
        let last = frames.last().map(|frame| frame.at_ms).unwrap_or_default();
        let mut back = travel(gesture, peak, end, scale, 8, 16);
        back.remove(0);
        for frame in &mut back {
            frame.at_ms += last;
        }
        frames.extend(back);
        frames
    }

    /// The frames of a gesture moving from `from` to `to` completion, with no
    /// lift at the end.
    pub fn travel(
        gesture: &GestureDefinition,
        from: f32,
        to: f32,
        scale: RecognizerScale,
        steps: usize,
        step_ms: u64,
    ) -> Vec<TouchFrame> {
        let steps = steps.max(1);
        (0..=steps)
            .map(|step| {
                let fraction = from + (to - from) * (step as f32 / steps as f32);
                TouchFrame::new(step as u64 * step_ms, place(gesture, fraction, scale))
            })
            .collect()
    }

    /// A whole gesture, lifted at the end.
    pub fn complete(gesture: &GestureDefinition, scale: RecognizerScale) -> Vec<TouchFrame> {
        lift(perform(gesture, 1.0, scale))
    }

    /// Appends the frame where every contact leaves the pad.
    pub fn lift(mut frames: Vec<TouchFrame>) -> Vec<TouchFrame> {
        let at_ms = frames.last().map(|frame| frame.at_ms + 16).unwrap_or(0);
        frames.push(TouchFrame::lifted(at_ms));
        frames
    }

    /// Shifts a stream so it starts `at_ms` later. Used to replay the same
    /// gesture twice and watch the cooldown swallow the second one.
    pub fn after(frames: Vec<TouchFrame>, at_ms: u64) -> Vec<TouchFrame> {
        frames
            .into_iter()
            .map(|frame| TouchFrame::new(frame.at_ms + at_ms, frame.contacts))
            .collect()
    }

    /// The contacts of `gesture` at `fraction` of the way through it.
    fn place(
        gesture: &GestureDefinition,
        fraction: f32,
        scale: RecognizerScale,
    ) -> Vec<ContactPoint> {
        let count = gesture.contacts.get() as usize;
        let base = starting_points(count, gesture.thumb_required);
        // Pinches, spreads, and rotations are constructed about the contacts'
        // own centre, which is what the recognizer measures them against. About
        // any other point the produced travel would not be the travel asked
        // for.
        let centre = centre_of(&base);
        match gesture.shape {
            GestureShape::Swipe => {
                let distance = fraction * scale.swipe_travel;
                let (dx, dy) = match gesture.direction {
                    Some(Direction::Up) => (0.0, -distance),
                    Some(Direction::Down) => (0.0, distance),
                    Some(Direction::Left) => (-distance, 0.0),
                    _ => (distance, 0.0),
                };
                base.into_iter()
                    .map(|(id, role, x, y)| ContactPoint {
                        id,
                        role,
                        x: x + dx,
                        y: y + dy,
                    })
                    .collect()
            }
            GestureShape::Pinch | GestureShape::Spread => {
                let start_radius = mean_radius(&base, centre);
                let wanted = if gesture.shape == GestureShape::Pinch {
                    start_radius - fraction * scale.pinch_travel
                } else {
                    start_radius + fraction * scale.pinch_travel
                };
                let factor = (wanted / start_radius).max(0.0);
                base.into_iter()
                    .map(|(id, role, x, y)| ContactPoint {
                        id,
                        role,
                        x: centre.0 + (x - centre.0) * factor,
                        y: centre.1 + (y - centre.1) * factor,
                    })
                    .collect()
            }
            GestureShape::Rotate => {
                let angle = fraction
                    * scale.rotate_turn
                    * if gesture.direction == Some(Direction::CounterClockwise) {
                        -1.0
                    } else {
                        1.0
                    };
                let (sin, cos) = angle.sin_cos();
                base.into_iter()
                    .map(|(id, role, x, y)| {
                        let (dx, dy) = (x - centre.0, y - centre.1);
                        ContactPoint {
                            id,
                            role,
                            x: centre.0 + dx * cos - dy * sin,
                            y: centre.1 + dx * sin + dy * cos,
                        }
                    })
                    .collect()
            }
            // Neither moves. A hold is time, and a tap is time plus a lift.
            GestureShape::Hold | GestureShape::Tap => base
                .into_iter()
                .map(|(id, role, x, y)| ContactPoint { id, role, x, y })
                .collect(),
        }
    }

    /// The contacts a gesture of this shape starts from, before it has moved.
    ///
    /// A frame source that reports a whole gesture rather than contact points —
    /// a compositor — has no contacts to give, and this is the arrangement it
    /// borrows. It is shipped rather than private for that reason:
    /// [`crate::ingest`] places its synthetic contacts here and then moves them
    /// by what the compositor said, so an event stream and a replayed stream
    /// are measured against the same starting shape.
    pub fn base_contacts(count: usize, thumb: bool) -> Vec<ContactPoint> {
        starting_points(count, thumb)
            .into_iter()
            .map(|(id, role, x, y)| ContactPoint { id, role, x, y })
            .collect()
    }

    /// The centre of these contacts.
    pub fn centre(contacts: &[ContactPoint]) -> (f32, f32) {
        let points: Vec<(u32, ContactRole, f32, f32)> = contacts
            .iter()
            .map(|contact| (contact.id, contact.role, contact.x, contact.y))
            .collect();
        centre_of(&points)
    }

    /// Contacts spread evenly along the pad, with the thumb below the others
    /// where one is wanted. The exact arrangement does not matter: every
    /// measurement the recognizer makes is relative to where the contacts
    /// started.
    fn starting_points(count: usize, thumb: bool) -> Vec<(u32, ContactRole, f32, f32)> {
        (0..count)
            .map(|index| {
                let role = if thumb && index == 0 {
                    ContactRole::Thumb
                } else {
                    ContactRole::Finger
                };
                let spread = 0.16;
                let offset = index as f32 - (count as f32 - 1.0) / 2.0;
                let y = if role == ContactRole::Thumb {
                    0.62
                } else {
                    0.44
                };
                (index as u32, role, 0.5 + offset * spread, y)
            })
            .collect()
    }

    fn centre_of(points: &[(u32, ContactRole, f32, f32)]) -> (f32, f32) {
        let count = points.len() as f32;
        let sum = points
            .iter()
            .fold((0.0, 0.0), |sum, (_, _, x, y)| (sum.0 + x, sum.1 + y));
        (sum.0 / count, sum.1 / count)
    }

    fn mean_radius(points: &[(u32, ContactRole, f32, f32)], centre: (f32, f32)) -> f32 {
        let count = points.len() as f32;
        points
            .iter()
            .map(|(_, _, x, y)| ((x - centre.0).powi(2) + (y - centre.1).powi(2)).sqrt())
            .sum::<f32>()
            / count
    }
}

#[cfg(test)]
mod tests {
    use super::synthetic;
    use super::*;
    use crate::preset::mac_style;

    fn gesture(id: &str) -> GestureDefinition {
        mac_style()
            .gestures
            .into_iter()
            .find(|gesture| gesture.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is in the preset"))
    }

    fn preset_recognizer() -> Recognizer {
        Recognizer::new(mac_style().gestures)
    }

    fn kinds(events: &[GestureEvent]) -> Vec<GestureEventKind> {
        events.iter().map(|event| event.kind).collect()
    }

    fn completed(events: &[GestureEvent]) -> Vec<&str> {
        events
            .iter()
            .filter(|event| event.kind == GestureEventKind::Complete)
            .map(|event| event.gesture.as_str())
            .collect()
    }

    #[test]
    fn a_whole_four_finger_up_swipe_begins_crosses_and_completes() {
        let mut recognizer = preset_recognizer();
        let frames = synthetic::complete(&gesture("overview"), RecognizerScale::default());
        let events = recognizer.replay(&frames);

        assert!(
            events
                .iter()
                .all(|event| event.gesture.as_str() == "overview"),
            "{events:?}"
        );
        let kinds = kinds(&events);
        assert_eq!(kinds.first(), Some(&GestureEventKind::Begin));
        assert!(kinds.contains(&GestureEventKind::ThresholdCrossed));
        assert_eq!(kinds.last(), Some(&GestureEventKind::Complete));
        assert_eq!(events.last().unwrap().progress, 1.0);
    }

    #[test]
    fn every_preset_gesture_is_recognized_from_its_own_stream_and_only_it() {
        let scale = RecognizerScale::default();
        for definition in mac_style().gestures {
            let mut recognizer = preset_recognizer();
            let events = recognizer.replay(&synthetic::complete(&definition, scale));
            assert_eq!(
                completed(&events),
                vec![definition.id.as_str()],
                "{} produced {events:?}",
                definition.id
            );
        }
    }

    #[test]
    fn a_gesture_that_never_gets_going_produces_no_events_at_all() {
        let mut recognizer = preset_recognizer();
        // Five per cent of a swipe: a hand shifting its weight, not a gesture.
        let frames = synthetic::lift(synthetic::perform(
            &gesture("overview"),
            0.05,
            RecognizerScale::default(),
        ));
        assert!(recognizer.replay(&frames).is_empty());
    }

    #[test]
    fn a_gesture_released_short_of_the_threshold_cancels_rather_than_completing() {
        let mut recognizer = preset_recognizer();
        let frames = synthetic::lift(synthetic::perform(
            &gesture("overview"),
            0.4,
            RecognizerScale::default(),
        ));
        let events = recognizer.replay(&frames);
        assert_eq!(kinds(&events).last(), Some(&GestureEventKind::Cancel));
        assert!(completed(&events).is_empty());
    }

    #[test]
    fn a_gesture_that_went_far_and_came_back_does_not_commit() {
        let mut recognizer = preset_recognizer();
        // Past the activation threshold, then reversed to a third of the way,
        // which is still above the cancellation threshold, then released.
        let frames = synthetic::lift(synthetic::perform_and_reverse(
            &gesture("overview"),
            0.9,
            0.35,
            RecognizerScale::default(),
        ));
        let events = recognizer.replay(&frames);
        assert!(
            kinds(&events).contains(&GestureEventKind::ThresholdCrossed),
            "the gesture never got far enough to be worth cancelling"
        );
        assert_eq!(kinds(&events).last(), Some(&GestureEventKind::Cancel));
    }

    #[test]
    fn reversing_below_the_cancellation_threshold_cancels_before_the_fingers_lift() {
        let mut recognizer = preset_recognizer();
        let frames = synthetic::perform_and_reverse(
            &gesture("overview"),
            0.9,
            0.05,
            RecognizerScale::default(),
        );
        let events = recognizer.replay(&frames);
        assert_eq!(kinds(&events).last(), Some(&GestureEventKind::Cancel));
        // And nothing more happens under the same fingers, however far they go.
        let more = recognizer.replay(&synthetic::perform(
            &gesture("overview"),
            1.0,
            RecognizerScale::default(),
        ));
        assert!(more.is_empty(), "{more:?}");
    }

    #[test]
    fn a_second_identical_gesture_inside_the_cooldown_is_swallowed() {
        let scale = RecognizerScale::default();
        let overview = gesture("overview");
        let mut recognizer = preset_recognizer();
        let first = synthetic::complete(&overview, scale);
        let ended_at = first.last().unwrap().at_ms;
        assert_eq!(completed(&recognizer.replay(&first)), vec!["overview"]);

        let cooldown = overview.cooldown.as_millis();
        assert!(recognizer.in_cooldown(&overview.id, ended_at + cooldown - 1));
        let inside = synthetic::after(synthetic::complete(&overview, scale), ended_at + 10);
        assert!(
            recognizer.replay(&inside).is_empty(),
            "the cooldown let a repeat through"
        );

        let outside = synthetic::after(
            synthetic::complete(&overview, scale),
            ended_at + cooldown + 1,
        );
        assert_eq!(completed(&recognizer.replay(&outside)), vec!["overview"]);
    }

    #[test]
    fn the_cooldown_of_one_gesture_does_not_swallow_a_different_one() {
        let scale = RecognizerScale::default();
        let mut recognizer = preset_recognizer();
        let first = synthetic::complete(&gesture("overview"), scale);
        let ended_at = first.last().unwrap().at_ms;
        recognizer.replay(&first);

        let other = synthetic::after(
            synthetic::complete(&gesture("current-app-windows"), scale),
            ended_at + 10,
        );
        assert_eq!(
            completed(&recognizer.replay(&other)),
            vec!["current-app-windows"]
        );
    }

    #[test]
    fn a_thumb_and_three_fingers_pinch_is_the_launcher_and_four_fingers_is_not() {
        let scale = RecognizerScale::default();
        let mut recognizer = preset_recognizer();
        assert_eq!(
            completed(&recognizer.replay(&synthetic::complete(&gesture("launcher"), scale))),
            vec!["launcher"]
        );

        // The same pinch with no thumb labelled. There is no four-finger pinch
        // without a thumb in the preset, so nothing is recognized — rather than
        // the launcher opening because four contacts is four contacts.
        let mut without_thumb = gesture("launcher");
        without_thumb.thumb_required = false;
        let mut fresh = preset_recognizer();
        let events = fresh.replay(&synthetic::complete(&without_thumb, scale));
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_labelled_thumb_keeps_a_four_finger_swipe_from_being_recognized() {
        // Four contacts including a thumb belong to the thumb-and-three family.
        // A swipe made with them is not the four-finger swipe.
        let mut with_thumb = gesture("overview");
        with_thumb.thumb_required = true;
        let mut recognizer = preset_recognizer();
        let events = recognizer.replay(&synthetic::complete(
            &with_thumb,
            RecognizerScale::default(),
        ));
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn changing_the_finger_count_mid_gesture_cancels_and_recognizes_nothing_else() {
        let scale = RecognizerScale::default();
        let mut recognizer = preset_recognizer();
        let mut frames = synthetic::perform(&gesture("overview"), 0.5, scale);
        // One finger leaves.
        let mut fewer = frames.last().unwrap().clone();
        fewer.at_ms += 16;
        fewer.contacts.pop();
        frames.push(fewer);
        let events = recognizer.replay(&frames);
        assert_eq!(kinds(&events).last(), Some(&GestureEventKind::Cancel));

        // Three contacts now, and the preset has no three-contact gesture, so
        // nothing takes over.
        let mut more = frames.last().unwrap().clone();
        more.at_ms += 200;
        more.contacts[0].y -= 0.2;
        assert!(recognizer.observe(&more).is_empty());
    }

    #[test]
    fn a_disabled_gesture_is_never_recognized() {
        let mut config = mac_style();
        config.gestures[2].enabled = false;
        let mut recognizer = Recognizer::new(config.active());
        let events = recognizer.replay(&synthetic::complete(
            &gesture("overview"),
            RecognizerScale::default(),
        ));
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_recognizer_with_nothing_configured_recognizes_nothing() {
        let mut recognizer = Recognizer::new(Vec::new());
        assert!(
            recognizer
                .replay(&synthetic::complete(
                    &gesture("overview"),
                    RecognizerScale::default()
                ))
                .is_empty()
        );
    }

    #[test]
    fn progress_rises_monotonically_through_a_deliberate_gesture() {
        let mut recognizer = preset_recognizer();
        let events = recognizer.replay(&synthetic::complete(
            &gesture("overview"),
            RecognizerScale::default(),
        ));
        let progresses: Vec<f32> = events.iter().map(|event| event.progress).collect();
        assert!(
            progresses.windows(2).all(|pair| pair[1] >= pair[0] - 1e-6),
            "{progresses:?}"
        );
    }

    #[test]
    fn a_hold_completes_on_time_and_a_hold_that_wanders_does_not() {
        let hold = GestureDefinition::new(
            "hold",
            GestureShape::Hold,
            3,
            false,
            None,
            better_actions::DesktopAction::ShowDesktop,
        )
        .unwrap();
        let scale = RecognizerScale::default();
        let still: Vec<TouchFrame> = (0..=8)
            .map(|step| {
                TouchFrame::new(
                    step * 80,
                    synthetic::travel(&hold, 0.0, 0.0, scale, 1, 1)[0]
                        .contacts
                        .clone(),
                )
            })
            .collect();
        let mut recognizer = Recognizer::new(vec![hold.clone()]);
        let events = recognizer.replay(&synthetic::lift(still.clone()));
        assert_eq!(completed(&events), vec!["hold"]);

        // The same duration, but the hand slid across the pad.
        let mut wandering = still;
        for (step, frame) in wandering.iter_mut().enumerate() {
            for contact in &mut frame.contacts {
                contact.x += step as f32 * 0.03;
            }
        }
        let mut recognizer = Recognizer::new(vec![hold]);
        assert!(completed(&recognizer.replay(&synthetic::lift(wandering))).is_empty());
    }

    #[test]
    fn a_tap_completes_only_when_it_is_short_enough() {
        let tap = GestureDefinition::new(
            "tap",
            GestureShape::Tap,
            3,
            false,
            None,
            better_actions::DesktopAction::MediaPlayPause,
        )
        .unwrap();
        let scale = RecognizerScale::default();
        let contacts = synthetic::travel(&tap, 0.0, 0.0, scale, 1, 1)[0]
            .contacts
            .clone();

        let mut recognizer = Recognizer::new(vec![tap.clone()]);
        let quick = vec![
            TouchFrame::new(0, contacts.clone()),
            TouchFrame::new(40, contacts.clone()),
            TouchFrame::lifted(80),
        ];
        assert_eq!(completed(&recognizer.replay(&quick)), vec!["tap"]);

        let mut recognizer = Recognizer::new(vec![tap]);
        let slow = vec![
            TouchFrame::new(0, contacts.clone()),
            TouchFrame::new(400, contacts),
            TouchFrame::lifted(900),
        ];
        assert!(completed(&recognizer.replay(&slow)).is_empty());
    }

    #[test]
    fn every_event_maps_onto_the_launchers_four_phases() {
        use touchpad_session::GesturePhase;
        assert_eq!(GestureEventKind::Begin.phase(), GesturePhase::Begin);
        assert_eq!(GestureEventKind::Progress.phase(), GesturePhase::Update);
        assert_eq!(
            GestureEventKind::ThresholdCrossed.phase(),
            GesturePhase::Update
        );
        assert_eq!(GestureEventKind::Complete.phase(), GesturePhase::End);
        assert_eq!(GestureEventKind::Cancel.phase(), GesturePhase::Cancel);

        let mut recognizer = preset_recognizer();
        let events = recognizer.replay(&synthetic::complete(
            &gesture("launcher"),
            RecognizerScale::default(),
        ));
        let report = events.last().unwrap().progress_report();
        assert!(report.is_final());
        assert_eq!(report.fraction, 1.0);
    }

    #[test]
    fn the_two_finger_gestures_do_not_answer_to_four_contacts() {
        let scale = RecognizerScale::default();
        let mut wide = gesture("app-zoom");
        wide.contacts = crate::definition::ContactCount::new(4).unwrap();
        let mut recognizer = preset_recognizer();
        // Four contacts pinching with no thumb: the preset's four-contact pinch
        // requires one, and the two-contact one does not match the count.
        let events = recognizer.replay(&synthetic::complete(&wide, scale));
        assert!(events.is_empty(), "{events:?}");
    }
}

#[cfg(test)]
mod frame_health_tests {
    use super::synthetic;
    use super::*;
    use crate::preset::mac_style;

    fn overview() -> GestureDefinition {
        mac_style()
            .gestures
            .into_iter()
            .find(|gesture| gesture.id.as_str() == "overview")
            .unwrap()
    }

    #[test]
    fn a_clean_stream_reports_no_dropped_or_reordered_frames() {
        let mut recognizer = Recognizer::new(mac_style().gestures);
        let frames = synthetic::complete(&overview(), RecognizerScale::default());
        recognizer.replay(&frames);

        let health = recognizer.health();
        assert_eq!(health.seen, frames.len() as u64);
        assert!(health.is_clean(), "{health:?}");
    }

    #[test]
    fn a_frame_that_arrives_out_of_order_is_counted_and_dropped() {
        let mut recognizer = Recognizer::new(mac_style().gestures);
        let scale = RecognizerScale::default();
        let mut frames = synthetic::perform(&overview(), 1.0, scale);
        // The stream stutters: one frame from earlier in the gesture arrives
        // after a later one.
        let stale = frames[2].clone();
        let ended_at = frames.last().unwrap().at_ms;
        frames.push(stale);
        frames.push(TouchFrame::lifted(ended_at + 16));

        let events = recognizer.replay(&frames);
        assert_eq!(recognizer.health().reordered, 1);
        // And the gesture still completes: the stale frame did not drag the
        // progress backwards into a cancellation.
        assert_eq!(events.last().unwrap().kind, GestureEventKind::Complete);
    }

    #[test]
    fn a_stall_in_the_stream_is_counted_as_dropped_frames() {
        let mut recognizer = Recognizer::new(mac_style().gestures);
        let scale = RecognizerScale::default();
        let mut frames = synthetic::perform(&overview(), 1.0, scale);
        // A quarter of a second with nothing delivered.
        for frame in frames.iter_mut().skip(4) {
            frame.at_ms += 250;
        }
        recognizer.replay(&synthetic::lift(frames));

        let health = recognizer.health();
        assert!(health.dropped >= 7, "{health:?}");
        assert_eq!(health.reordered, 0);
    }
}
