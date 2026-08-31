//! Reading a compositor's gesture events with the recognizer that reads
//! frames.
//!
//! A touchpad reports contact points. A compositor does not: by the time GNOME
//! Shell has an event, the individual fingers are gone and what is left is "a
//! four-finger swipe moved this far" or "a pinch is now at this scale". That is
//! a different vocabulary, and the tempting answer — a second recognizer that
//! understands it — is the wrong one. Two recognizers means two activation
//! thresholds, two cancellation rules, and two cooldowns, and the day they
//! disagree is the day a gesture behaves differently depending on which backend
//! is installed.
//!
//! So there is one recognizer, and this module gives it frames. Each compositor
//! event is turned into the contact frame it would have been: the contacts a
//! gesture of that many fingers starts from, moved by exactly what the
//! compositor said had happened. Every threshold, the arming rule, the
//! reversal rule, the value-at-release rule, the cooldown, and the frame health
//! counters are therefore the ones in [`crate::recognizer`], unchanged and
//! untouched.
//!
//! Two things this path cannot do, stated rather than hidden.
//!
//! **It cannot see a thumb.** Clutter's touchpad gesture events carry a contact
//! count and nothing else about the contacts, so thumb-and-three and four
//! fingers are the same four contacts here. The rule this module applies is in
//! [`assumes_thumb`], and it is deliberately narrow: a thumb is assumed only
//! where assuming it cannot shadow another gesture.
//!
//! **It cannot report holds or taps.** Only swipe and pinch cross this bridge,
//! so a hold or a tap configured against a compositor backend is never
//! recognized. It is not silently turned into something else.

use crate::definition::{GestureDefinition, GestureShape};
use crate::recognizer::{
    ContactPoint, GestureEvent, Recognizer, RecognizerScale, TouchFrame, synthetic,
};

/// Which of the compositor's two gesture streams an event came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositorGestureKind {
    Swipe,
    /// Pinch, spread, and rotate all arrive on this one. Which of the three it
    /// turns out to be is the recognizer's decision, from the movement.
    Pinch,
}

impl CompositorGestureKind {
    /// The shapes this stream can produce. A swipe event can never become a
    /// pinch, and this is where that is stated once.
    pub fn shapes(self) -> &'static [GestureShape] {
        match self {
            Self::Swipe => &[GestureShape::Swipe],
            Self::Pinch => &[
                GestureShape::Pinch,
                GestureShape::Spread,
                GestureShape::Rotate,
            ],
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Swipe => "swipe",
            Self::Pinch => "pinch",
        }
    }
}

/// Where a compositor gesture is in its life. The same four the adapter
/// interface carries on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositorPhase {
    Begin,
    Update,
    End,
    Cancel,
}

impl CompositorPhase {
    /// The wire values `org.betteros.TouchpadAdapter1` uses, which are the
    /// values the extension writes. An unknown number is refused rather than
    /// guessed at, because guessing a phase invents a gesture.
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Begin),
            1 => Some(Self::Update),
            2 => Some(Self::End),
            3 => Some(Self::Cancel),
            _ => None,
        }
    }

    pub fn wire(self) -> u32 {
        match self {
            Self::Begin => 0,
            Self::Update => 1,
            Self::End => 2,
            Self::Cancel => 3,
        }
    }
}

/// One event as the compositor reported it.
///
/// The two relative figures are relative to the previous event and the one
/// absolute figure is measured from the beginning of the gesture, which is what
/// Clutter delivers. Accumulating them is this module's job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompositorGesture {
    pub kind: CompositorGestureKind,
    pub phase: CompositorPhase,
    pub fingers: u8,
    /// Motion since the previous event, in pixels. Swipe only.
    pub dx: f32,
    pub dy: f32,
    /// The pinch scale measured from the start of the gesture; `1.0` at the
    /// beginning. Pinch only.
    pub scale: f32,
    /// The turn since the previous event, in radians. Pinch only.
    pub angle_delta: f32,
    pub at_ms: u64,
}

impl CompositorGesture {
    pub fn swipe(phase: CompositorPhase, fingers: u8, dx: f32, dy: f32, at_ms: u64) -> Self {
        Self {
            kind: CompositorGestureKind::Swipe,
            phase,
            fingers,
            dx,
            dy,
            scale: 1.0,
            angle_delta: 0.0,
            at_ms,
        }
    }

    /// The same event as it arrived from the GNOME Shell adapter.
    ///
    /// `None` for a phase this build does not know and for a contact count that
    /// will not fit a touchpad. Both are refusals rather than guesses: an event
    /// that cannot be understood must not become a gesture that was not made.
    pub fn from_shell(event: &touchpad_session::ShellGestureEvent) -> Option<Self> {
        use touchpad_session::ShellGestureEvent;

        let phase = CompositorPhase::from_wire(event.phase())?;
        let fingers = u8::try_from(event.fingers()).ok()?;
        Some(match *event {
            ShellGestureEvent::Swipe { dx, dy, at_ms, .. } => {
                Self::swipe(phase, fingers, dx as f32, dy as f32, at_ms)
            }
            ShellGestureEvent::Pinch {
                scale,
                angle_delta,
                at_ms,
                ..
            } => Self::pinch(phase, fingers, scale as f32, angle_delta as f32, at_ms),
        })
    }

    pub fn pinch(phase: CompositorPhase, fingers: u8, scale: f32, angle: f32, at_ms: u64) -> Self {
        Self {
            kind: CompositorGestureKind::Pinch,
            phase,
            fingers,
            dx: 0.0,
            dy: 0.0,
            scale,
            angle_delta: angle,
            at_ms,
        }
    }
}

/// How a compositor's pixels become a fraction of the pad.
///
/// One number, for the same reason [`RecognizerScale`] is one struct: it is a
/// recorded starting value and not a measurement. A compositor reports swipe
/// motion in pixels and never says how big the pad is, so something has to
/// decide how many pixels a whole pad is worth. Nothing has been measured
/// against a hand, and changing this is one edit and one test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventScale {
    /// Pixels of compositor swipe motion that count as the full width of the
    /// pad. With the shipped [`RecognizerScale::swipe_travel`] of 0.18, a whole
    /// swipe is a fifth of this.
    pub swipe_pad_pixels: f32,
}

impl Default for EventScale {
    fn default() -> Self {
        Self {
            swipe_pad_pixels: 1000.0,
        }
    }
}

/// Whether a gesture of this kind and contact count should be treated as
/// including a thumb, on a stream that cannot see one.
///
/// The rule is: assume a thumb only when every gesture of that kind and count
/// wants one. If the configuration has both a thumb-and-three pinch and a
/// four-finger pinch, they are indistinguishable here, and the honest answer is
/// to leave the thumb unlabelled — which recognizes the four-finger gesture and
/// never the thumb one, rather than silently picking the wrong one of the two.
///
/// With the shipped Mac-style preset this labels the four-contact pinch and
/// spread — the launcher and Show Desktop — and labels nothing else, because
/// the four-finger swipes want no thumb.
pub fn assumes_thumb(
    definitions: &[GestureDefinition],
    kind: CompositorGestureKind,
    fingers: u8,
) -> bool {
    let shapes = kind.shapes();
    let matching = definitions
        .iter()
        .filter(|gesture| gesture.enabled && gesture.contacts.get() == fingers)
        .filter(|gesture| shapes.contains(&gesture.shape));
    let mut wants_thumb = false;
    let mut wants_none = false;
    for gesture in matching {
        if gesture.thumb_required {
            wants_thumb = true;
        } else {
            wants_none = true;
        }
    }
    wants_thumb && !wants_none
}

/// The gesture currently being reported by the compositor.
#[derive(Clone, Debug)]
struct Live {
    kind: CompositorGestureKind,
    fingers: u8,
    base: Vec<ContactPoint>,
    centre: (f32, f32),
    dx: f32,
    dy: f32,
    scale: f32,
    angle: f32,
}

/// A [`Recognizer`] driven by compositor events instead of contact frames.
///
/// It owns the recognizer rather than wrapping a borrowed one so that there is
/// exactly one place a compositor-backed gesture is recognized, and so the
/// frames it synthesizes cannot be interleaved with frames from somewhere else.
pub struct EventRecognizer {
    recognizer: Recognizer,
    definitions: Vec<GestureDefinition>,
    events: EventScale,
    live: Option<Live>,
}

impl EventRecognizer {
    pub fn new(definitions: Vec<GestureDefinition>) -> Self {
        Self::with_scales(
            definitions,
            RecognizerScale::default(),
            EventScale::default(),
        )
    }

    pub fn with_scales(
        definitions: Vec<GestureDefinition>,
        scale: RecognizerScale,
        events: EventScale,
    ) -> Self {
        Self {
            recognizer: Recognizer::with_scale(definitions.clone(), scale),
            definitions,
            events,
            live: None,
        }
    }

    pub fn recognizer(&self) -> &Recognizer {
        &self.recognizer
    }

    pub fn events_scale(&self) -> EventScale {
        self.events
    }

    /// Whether a gesture is under way right now.
    pub fn is_live(&self) -> bool {
        self.live.is_some()
    }

    /// Applies one compositor event and returns what the recognizer made of it.
    pub fn observe(&mut self, event: &CompositorGesture) -> Vec<GestureEvent> {
        match event.phase {
            CompositorPhase::Begin => {
                let thumb = assumes_thumb(&self.definitions, event.kind, event.fingers);
                let base = synthetic::base_contacts(event.fingers as usize, thumb);
                self.live = Some(Live {
                    kind: event.kind,
                    fingers: event.fingers,
                    centre: synthetic::centre(&base),
                    base,
                    dx: 0.0,
                    dy: 0.0,
                    scale: 1.0,
                    angle: 0.0,
                });
                let frame = self.frame(event.at_ms);
                self.recognizer.observe(&frame)
            }
            CompositorPhase::Update => {
                // An update with no begin behind it is a stream that started
                // mid-gesture — a client that connected late, or a compositor
                // that dropped the first event. Beginning here would measure
                // the gesture from the middle, so it is ignored until the next
                // one starts properly.
                if self.live.is_none() {
                    return Vec::new();
                }
                self.accumulate(event);
                let frame = self.frame(event.at_ms);
                self.recognizer.observe(&frame)
            }
            CompositorPhase::End => {
                if self.live.is_none() {
                    return Vec::new();
                }
                self.accumulate(event);
                let frame = self.frame(event.at_ms);
                let mut produced = self.recognizer.observe(&frame);
                // The fingers left the pad. Whether that commits is the
                // recognizer's value-at-release rule, not this module's.
                produced.extend(self.recognizer.observe(&TouchFrame::lifted(event.at_ms)));
                self.live = None;
                produced
            }
            CompositorPhase::Cancel => {
                self.live = None;
                self.recognizer.cancel(event.at_ms)
            }
        }
    }

    fn accumulate(&mut self, event: &CompositorGesture) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        // A gesture whose contact count changed mid-flight is a different
        // gesture. Recording the new count is enough: the frame it produces
        // has a different number of contacts, and the recognizer cancels on
        // exactly that.
        live.fingers = event.fingers;
        match event.kind {
            CompositorGestureKind::Swipe => {
                live.dx += event.dx;
                live.dy += event.dy;
            }
            CompositorGestureKind::Pinch => {
                live.scale = event.scale;
                live.angle += event.angle_delta;
            }
        }
    }

    /// The contact frame this gesture would have produced.
    fn frame(&self, at_ms: u64) -> TouchFrame {
        let Some(live) = self.live.as_ref() else {
            return TouchFrame::lifted(at_ms);
        };
        if live.fingers as usize != live.base.len() {
            // The count changed. Any frame with the new count cancels the
            // gesture, and the contacts it holds are never measured against
            // anything, so the starting arrangement is the honest one to send.
            return TouchFrame::new(
                at_ms,
                synthetic::base_contacts(live.fingers as usize, false),
            );
        }
        let contacts = match live.kind {
            CompositorGestureKind::Swipe => {
                let dx = live.dx / self.events.swipe_pad_pixels;
                let dy = live.dy / self.events.swipe_pad_pixels;
                live.base
                    .iter()
                    .map(|contact| ContactPoint {
                        x: contact.x + dx,
                        y: contact.y + dy,
                        ..*contact
                    })
                    .collect()
            }
            CompositorGestureKind::Pinch => {
                let (cx, cy) = live.centre;
                let (sin, cos) = live.angle.sin_cos();
                let factor = live.scale.max(0.0);
                live.base
                    .iter()
                    .map(|contact| {
                        let (x, y) = ((contact.x - cx) * factor, (contact.y - cy) * factor);
                        ContactPoint {
                            x: cx + x * cos - y * sin,
                            y: cy + x * sin + y * cos,
                            ..*contact
                        }
                    })
                    .collect()
            }
        };
        TouchFrame::new(at_ms, contacts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::mac_style;
    use crate::recognizer::GestureEventKind;

    fn completed(events: &[GestureEvent]) -> Vec<&str> {
        events
            .iter()
            .filter(|event| event.kind == GestureEventKind::Complete)
            .map(|event| event.gesture.as_str())
            .collect()
    }

    fn kinds(events: &[GestureEvent]) -> Vec<GestureEventKind> {
        events.iter().map(|event| event.kind).collect()
    }

    fn preset() -> EventRecognizer {
        EventRecognizer::new(mac_style().gestures)
    }

    /// A four-finger swipe up, as GNOME Shell would report it: a begin, a few
    /// updates carrying pixels, and an end.
    fn swipe_up(from_ms: u64, total_pixels: f32, steps: u32) -> Vec<CompositorGesture> {
        let step = -total_pixels / steps as f32;
        let mut stream = vec![CompositorGesture::swipe(
            CompositorPhase::Begin,
            4,
            0.0,
            0.0,
            from_ms,
        )];
        for index in 1..=steps {
            let phase = if index == steps {
                CompositorPhase::End
            } else {
                CompositorPhase::Update
            };
            stream.push(CompositorGesture::swipe(
                phase,
                4,
                0.0,
                step,
                from_ms + index as u64 * 16,
            ));
        }
        stream
    }

    fn pinch_to(from_ms: u64, scale: f32, steps: u32) -> Vec<CompositorGesture> {
        let mut stream = vec![CompositorGesture::pinch(
            CompositorPhase::Begin,
            4,
            1.0,
            0.0,
            from_ms,
        )];
        for index in 1..=steps {
            let fraction = index as f32 / steps as f32;
            let phase = if index == steps {
                CompositorPhase::End
            } else {
                CompositorPhase::Update
            };
            stream.push(CompositorGesture::pinch(
                phase,
                4,
                1.0 + (scale - 1.0) * fraction,
                0.0,
                from_ms + index as u64 * 16,
            ));
        }
        stream
    }

    fn replay(recognizer: &mut EventRecognizer, stream: &[CompositorGesture]) -> Vec<GestureEvent> {
        stream
            .iter()
            .flat_map(|event| recognizer.observe(event))
            .collect()
    }

    /// The distance in pixels that is one whole swipe under the shipped
    /// scales: 0.18 of the pad, and 1000 pixels to the pad.
    const WHOLE_SWIPE_PIXELS: f32 = 180.0;

    #[test]
    fn a_four_finger_swipe_up_from_the_compositor_is_the_overview_gesture() {
        let mut recognizer = preset();
        let events = replay(&mut recognizer, &swipe_up(0, WHOLE_SWIPE_PIXELS, 8));
        assert_eq!(completed(&events), vec!["overview"]);
        assert!(kinds(&events).contains(&GestureEventKind::ThresholdCrossed));
        assert!(!recognizer.is_live());
    }

    #[test]
    fn the_thresholds_are_the_recognizers_own_and_a_short_swipe_does_not_commit() {
        let mut recognizer = preset();
        // Half a gesture: past the arming point, nowhere near the 0.6
        // activation threshold.
        let events = replay(&mut recognizer, &swipe_up(0, WHOLE_SWIPE_PIXELS * 0.4, 8));
        assert!(completed(&events).is_empty(), "{events:?}");
        assert_eq!(kinds(&events).last(), Some(&GestureEventKind::Cancel));
    }

    #[test]
    fn reversing_before_the_fingers_lift_cancels_the_gesture() {
        let mut recognizer = preset();
        let mut stream = swipe_up(0, WHOLE_SWIPE_PIXELS, 8);
        // Take the end off and push the gesture back down again.
        stream.pop();
        for index in 0..8 {
            stream.push(CompositorGesture::swipe(
                CompositorPhase::Update,
                4,
                0.0,
                WHOLE_SWIPE_PIXELS / 8.0,
                200 + index * 16,
            ));
        }
        stream.push(CompositorGesture::swipe(
            CompositorPhase::End,
            4,
            0.0,
            0.0,
            400,
        ));
        let events = replay(&mut recognizer, &stream);
        assert!(completed(&events).is_empty(), "{events:?}");
        assert!(kinds(&events).contains(&GestureEventKind::Cancel));
    }

    #[test]
    fn a_compositor_cancel_cancels_whatever_was_under_way() {
        let mut recognizer = preset();
        let mut stream = swipe_up(0, WHOLE_SWIPE_PIXELS, 8);
        stream.pop();
        stream.push(CompositorGesture::swipe(
            CompositorPhase::Cancel,
            4,
            0.0,
            0.0,
            200,
        ));
        let events = replay(&mut recognizer, &stream);
        assert_eq!(kinds(&events).last(), Some(&GestureEventKind::Cancel));
        assert!(completed(&events).is_empty());
        assert!(!recognizer.is_live());

        // And the next gesture is recognized normally rather than being
        // swallowed by the cancelled one.
        let events = replay(&mut recognizer, &swipe_up(1_000, WHOLE_SWIPE_PIXELS, 8));
        assert_eq!(completed(&events), vec!["overview"]);
    }

    #[test]
    fn a_four_contact_pinch_is_the_launcher_and_a_spread_is_show_desktop() {
        let mut recognizer = preset();
        let events = replay(&mut recognizer, &pinch_to(0, 0.5, 8));
        assert_eq!(completed(&events), vec!["launcher"]);

        let mut recognizer = preset();
        let events = replay(&mut recognizer, &pinch_to(0, 1.5, 8));
        assert_eq!(completed(&events), vec!["show-desktop"]);
    }

    /// The honesty rule, asserted: the compositor sees four contacts and no
    /// thumb, and the preset's thumb-and-three pinch is still recognized —
    /// because this module assumes the thumb where assuming it is unambiguous.
    #[test]
    fn a_thumb_is_assumed_only_where_it_cannot_shadow_another_gesture() {
        let preset = mac_style().gestures;
        assert!(assumes_thumb(&preset, CompositorGestureKind::Pinch, 4));
        assert!(!assumes_thumb(&preset, CompositorGestureKind::Swipe, 4));
        assert!(!assumes_thumb(&preset, CompositorGestureKind::Pinch, 2));

        // Add a four-finger pinch that wants no thumb, and the thumb can no
        // longer be assumed: the two are the same four contacts here.
        let mut ambiguous = preset;
        ambiguous.push(
            GestureDefinition::new(
                "four-finger-pinch",
                GestureShape::Pinch,
                4,
                false,
                None,
                better_actions::DesktopAction::VolumeMute,
            )
            .unwrap(),
        );
        assert!(!assumes_thumb(&ambiguous, CompositorGestureKind::Pinch, 4));
    }

    #[test]
    fn the_cooldown_swallows_a_repeat_the_same_way_a_replayed_stream_does() {
        let mut recognizer = preset();
        assert_eq!(
            completed(&replay(
                &mut recognizer,
                &swipe_up(0, WHOLE_SWIPE_PIXELS, 8)
            )),
            vec!["overview"]
        );
        let inside = replay(&mut recognizer, &swipe_up(200, WHOLE_SWIPE_PIXELS, 8));
        assert!(completed(&inside).is_empty(), "{inside:?}");
        let outside = replay(&mut recognizer, &swipe_up(1_000, WHOLE_SWIPE_PIXELS, 8));
        assert_eq!(completed(&outside), vec!["overview"]);
    }

    #[test]
    fn changing_the_finger_count_mid_gesture_cancels() {
        let mut recognizer = preset();
        let mut stream = swipe_up(0, WHOLE_SWIPE_PIXELS, 8);
        stream.pop();
        stream.push(CompositorGesture::swipe(
            CompositorPhase::Update,
            3,
            0.0,
            -10.0,
            200,
        ));
        let events = replay(&mut recognizer, &stream);
        assert!(completed(&events).is_empty());
        assert_eq!(kinds(&events).last(), Some(&GestureEventKind::Cancel));
    }

    #[test]
    fn an_update_with_no_begin_behind_it_is_ignored_rather_than_measured() {
        let mut recognizer = preset();
        let stray = CompositorGesture::swipe(CompositorPhase::Update, 4, 0.0, -200.0, 10);
        assert!(recognizer.observe(&stray).is_empty());
        assert!(!recognizer.is_live());
    }

    #[test]
    fn the_frame_health_counters_see_the_compositors_own_stalls() {
        let mut recognizer = preset();
        let mut stream = swipe_up(0, WHOLE_SWIPE_PIXELS, 8);
        // A quarter of a second with nothing delivered.
        for event in stream.iter_mut().skip(4) {
            event.at_ms += 250;
        }
        replay(&mut recognizer, &stream);
        assert!(
            recognizer.recognizer().health().dropped >= 7,
            "{:?}",
            recognizer.recognizer().health()
        );
    }

    #[test]
    fn a_wire_event_becomes_the_gesture_it_describes_and_nonsense_is_refused() {
        use touchpad_session::ShellGestureEvent;

        let swipe = ShellGestureEvent::Swipe {
            phase: 1,
            fingers: 4,
            dx: -3.5,
            dy: -12.0,
            at_ms: 42,
        };
        assert_eq!(
            CompositorGesture::from_shell(&swipe),
            Some(CompositorGesture::swipe(
                CompositorPhase::Update,
                4,
                -3.5,
                -12.0,
                42
            ))
        );

        let pinch = ShellGestureEvent::Pinch {
            phase: 0,
            fingers: 4,
            scale: 0.8,
            angle_delta: 0.1,
            at_ms: 7,
        };
        assert_eq!(
            CompositorGesture::from_shell(&pinch),
            Some(CompositorGesture::pinch(
                CompositorPhase::Begin,
                4,
                0.8,
                0.1,
                7
            ))
        );

        // A phase this build does not know, and a contact count no hand has.
        assert_eq!(
            CompositorGesture::from_shell(&ShellGestureEvent::Swipe {
                phase: 9,
                fingers: 4,
                dx: 0.0,
                dy: 0.0,
                at_ms: 0,
            }),
            None
        );
        assert_eq!(
            CompositorGesture::from_shell(&ShellGestureEvent::Swipe {
                phase: 1,
                fingers: 4_000,
                dx: 0.0,
                dy: 0.0,
                at_ms: 0,
            }),
            None
        );
    }

    #[test]
    fn an_unknown_phase_number_is_refused_rather_than_guessed_at() {
        for value in 0..4 {
            assert!(CompositorPhase::from_wire(value).is_some());
        }
        assert_eq!(CompositorPhase::from_wire(4), None);
        assert_eq!(CompositorPhase::from_wire(u32::MAX), None);
        for phase in [
            CompositorPhase::Begin,
            CompositorPhase::Update,
            CompositorPhase::End,
            CompositorPhase::Cancel,
        ] {
            assert_eq!(CompositorPhase::from_wire(phase.wire()), Some(phase));
        }
    }
}
