//! The gesture adapter boundary.
//!
//! This is a seam, not a feature. Issue #2 wants a five-finger pinch to open
//! the launcher and defers the mechanism to an ADR, so what exists here is the
//! shape of the conversation between whatever produces gesture samples and the
//! overlay that reacts to them:
//!
//! - an adapter produces [`GestureSample`]s and describes itself
//!   ([`AdapterDescription`]);
//! - a [`GestureRecognizer`] applies the threshold and cooldown policy and
//!   produces [`GestureOutcome`]s the overlay can act on;
//! - the only adapter in this build is [`MockGestureAdapter`].
//!
//! Two rules make this boundary worth having.
//!
//! **The recognizer holds no clock.** Every decision takes the current
//! [`Instant`] as an argument, so the cooldown is replayable rather than
//! timing-dependent. A cooldown tested with a sleep is a flaky test.
//!
//! **No policy crosses into the adapter.** An adapter reports fingers,
//! direction, and progress. It does not decide that the launcher should open.
//! That keeps every candidate integration in ADR 0008 — a GNOME Shell adapter,
//! compositor integration, a libinput service, a portal — replaceable without
//! moving launcher behavior into a shell extension, which Issue #2 forbids.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::activation::OverlayVisibility;

/// Where a gesture is in its life.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GesturePhase {
    Begin,
    Update,
    /// The fingers left the touchpad. Whether that commits depends on how far
    /// the gesture got, which is the recognizer's decision and not the
    /// adapter's.
    End,
    /// The producer abandoned the gesture — a finger count changed, the
    /// compositor took it, the session lost focus.
    Cancel,
}

/// Which way the fingers moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureDirection {
    /// Five fingers drawing together. Issue #2's opening gesture.
    Inward,
    /// Five fingers spreading apart. Closes an open overlay where the adapter
    /// can report it.
    Outward,
}

/// One observation from an adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureSample {
    pub phase: GesturePhase,
    pub direction: GestureDirection,
    /// How far through the gesture is, clamped to `0.0..=1.0`. An adapter with
    /// no continuous progress reports `0.0` until it reports `1.0`, which is
    /// why the recognizer never requires intermediate values.
    pub progress: f32,
    /// How many fingers the adapter saw. Carried rather than assumed, because
    /// the finger count is one of the things a different integration path gets
    /// differently.
    pub fingers: u8,
}

impl GestureSample {
    pub fn new(
        phase: GesturePhase,
        direction: GestureDirection,
        progress: f32,
        fingers: u8,
    ) -> Self {
        Self {
            phase,
            direction,
            progress: progress.clamp(0.0, 1.0),
            fingers,
        }
    }
}

/// The tunable half of the policy: how far is far enough, and how soon is too
/// soon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureThresholds {
    /// The progress an inward gesture must reach to open the overlay.
    pub open_at: f32,
    /// The progress an outward gesture must reach to close it.
    pub close_at: f32,
    /// How long after a commit further gestures are ignored. Issue #2 requires
    /// that accidental partial gestures cannot flap the overlay open and shut.
    pub cooldown: Duration,
    /// How many fingers the gesture requires. Samples with any other count are
    /// ignored rather than treated as a weaker version of the gesture.
    pub fingers: u8,
}

impl Default for GestureThresholds {
    fn default() -> Self {
        Self {
            // Deliberately past halfway: a gesture that commits at the halfway
            // point is one a hesitating hand triggers. The exact numbers are
            // configuration, and the animation mapping that would make them
            // feel right is a deferred decision.
            open_at: 0.6,
            close_at: 0.6,
            cooldown: Duration::from_millis(350),
            fingers: 5,
        }
    }
}

/// What the overlay should do about a sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GestureOutcome {
    /// Nothing to do: wrong finger count, wrong direction for the current
    /// state, or inside the cooldown.
    Ignored,
    /// The gesture is under way and this is how far along it is. A surface
    /// that animates follows this; one that does not can ignore it.
    Progress(f32),
    Open,
    Close,
    /// The gesture was abandoned or released short of the threshold. The
    /// overlay returns to whatever it was doing before.
    Cancelled,
}

/// What an adapter is, for diagnostics and for the capability report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterDescription {
    pub name: String,
    /// Whether this adapter reports intermediate progress. A path that only
    /// says "it happened" cannot drive a continuous animation, which is one of
    /// the trade-offs ADR 0008 compares.
    pub continuous_progress: bool,
}

/// A source of gesture samples.
///
/// Deliberately narrow: poll for samples, and say what you are. An adapter has
/// no way to open the launcher, run a command, or read the application
/// library through this trait.
pub trait GestureAdapter {
    fn describe(&self) -> AdapterDescription;
    /// The next pending sample, or `None`. Must not block.
    fn next_sample(&self) -> Option<GestureSample>;
}

/// The only adapter in this build: one that replays samples handed to it.
#[derive(Debug, Default)]
pub struct MockGestureAdapter {
    queue: Mutex<VecDeque<GestureSample>>,
    continuous_progress: bool,
}

impl MockGestureAdapter {
    pub fn new(samples: impl IntoIterator<Item = GestureSample>) -> Self {
        Self {
            queue: Mutex::new(samples.into_iter().collect()),
            continuous_progress: true,
        }
    }

    /// A mock standing in for an integration path that reports only the
    /// completed gesture.
    pub fn without_progress(samples: impl IntoIterator<Item = GestureSample>) -> Self {
        Self {
            queue: Mutex::new(samples.into_iter().collect()),
            continuous_progress: false,
        }
    }

    pub fn push(&self, sample: GestureSample) {
        self.queue.lock().expect("queue lock").push_back(sample);
    }
}

impl GestureAdapter for MockGestureAdapter {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription {
            name: "mock".to_string(),
            continuous_progress: self.continuous_progress,
        }
    }

    fn next_sample(&self) -> Option<GestureSample> {
        self.queue.lock().expect("queue lock").pop_front()
    }
}

/// Turns samples into overlay outcomes under a threshold and cooldown policy.
#[derive(Clone, Debug)]
pub struct GestureRecognizer {
    thresholds: GestureThresholds,
    /// The furthest an in-flight gesture has travelled, so releasing after
    /// reversing cancels rather than commits.
    peak: Option<f32>,
    last_commit: Option<Instant>,
}

impl GestureRecognizer {
    pub fn new(thresholds: GestureThresholds) -> Self {
        Self {
            thresholds,
            peak: None,
            last_commit: None,
        }
    }

    pub fn thresholds(&self) -> &GestureThresholds {
        &self.thresholds
    }

    /// Whether a commit right now would be swallowed by the cooldown.
    pub fn in_cooldown(&self, now: Instant) -> bool {
        self.last_commit
            .is_some_and(|last| now.duration_since(last) < self.thresholds.cooldown)
    }

    /// Applies one sample.
    ///
    /// The current [`OverlayVisibility`] is an argument rather than state,
    /// because the overlay owns whether it is on screen and a recognizer that
    /// kept its own copy would eventually disagree with it.
    pub fn observe(
        &mut self,
        sample: GestureSample,
        visibility: OverlayVisibility,
        now: Instant,
    ) -> GestureOutcome {
        if sample.fingers != self.thresholds.fingers {
            self.peak = None;
            return GestureOutcome::Ignored;
        }
        // Inward opens, outward closes. A gesture in the direction that would
        // not change anything is ignored before any threshold is considered.
        let would_change = matches!(
            (sample.direction, visibility),
            (GestureDirection::Inward, OverlayVisibility::Hidden)
                | (GestureDirection::Outward, OverlayVisibility::Visible)
        );
        if !would_change {
            self.peak = None;
            return GestureOutcome::Ignored;
        }

        match sample.phase {
            GesturePhase::Cancel => {
                let was_active = self.peak.take().is_some();
                if was_active {
                    GestureOutcome::Cancelled
                } else {
                    GestureOutcome::Ignored
                }
            }
            GesturePhase::Begin | GesturePhase::Update => {
                if self.in_cooldown(now) {
                    return GestureOutcome::Ignored;
                }
                let peak = self.peak.get_or_insert(0.0);
                *peak = peak.max(sample.progress);
                GestureOutcome::Progress(sample.progress)
            }
            GesturePhase::End => {
                if self.in_cooldown(now) {
                    self.peak = None;
                    return GestureOutcome::Ignored;
                }
                // The release value decides, not the peak. Reversing the
                // gesture before letting go is how Issue #2 says an opening is
                // cancelled, so a gesture that went far and came back must not
                // commit.
                let reached = sample.progress;
                self.peak = None;
                let threshold = match sample.direction {
                    GestureDirection::Inward => self.thresholds.open_at,
                    GestureDirection::Outward => self.thresholds.close_at,
                };
                if reached >= threshold {
                    self.last_commit = Some(now);
                    match sample.direction {
                        GestureDirection::Inward => GestureOutcome::Open,
                        GestureDirection::Outward => GestureOutcome::Close,
                    }
                } else {
                    GestureOutcome::Cancelled
                }
            }
        }
    }
}

impl Default for GestureRecognizer {
    fn default() -> Self {
        Self::new(GestureThresholds::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(phase: GesturePhase, progress: f32) -> GestureSample {
        GestureSample::new(phase, GestureDirection::Inward, progress, 5)
    }

    #[test]
    fn an_inward_gesture_past_the_threshold_opens_the_overlay() {
        let mut recognizer = GestureRecognizer::default();
        let now = Instant::now();
        assert_eq!(
            recognizer.observe(
                sample(GesturePhase::Begin, 0.1),
                OverlayVisibility::Hidden,
                now
            ),
            GestureOutcome::Progress(0.1)
        );
        assert_eq!(
            recognizer.observe(
                sample(GesturePhase::Update, 0.5),
                OverlayVisibility::Hidden,
                now
            ),
            GestureOutcome::Progress(0.5)
        );
        assert_eq!(
            recognizer.observe(
                sample(GesturePhase::End, 0.8),
                OverlayVisibility::Hidden,
                now
            ),
            GestureOutcome::Open
        );
    }

    #[test]
    fn reversing_before_the_threshold_cancels_instead_of_opening() {
        let mut recognizer = GestureRecognizer::default();
        let now = Instant::now();
        recognizer.observe(
            sample(GesturePhase::Begin, 0.2),
            OverlayVisibility::Hidden,
            now,
        );
        recognizer.observe(
            sample(GesturePhase::Update, 0.55),
            OverlayVisibility::Hidden,
            now,
        );
        assert_eq!(
            recognizer.observe(
                sample(GesturePhase::End, 0.05),
                OverlayVisibility::Hidden,
                now
            ),
            GestureOutcome::Cancelled,
            "a gesture that went far and came back must not commit"
        );
    }

    #[test]
    fn an_abandoned_gesture_reports_a_cancellation_once_and_not_again() {
        let mut recognizer = GestureRecognizer::default();
        let now = Instant::now();
        recognizer.observe(
            sample(GesturePhase::Begin, 0.3),
            OverlayVisibility::Hidden,
            now,
        );
        assert_eq!(
            recognizer.observe(
                sample(GesturePhase::Cancel, 0.3),
                OverlayVisibility::Hidden,
                now
            ),
            GestureOutcome::Cancelled
        );
        assert_eq!(
            recognizer.observe(
                sample(GesturePhase::Cancel, 0.0),
                OverlayVisibility::Hidden,
                now
            ),
            GestureOutcome::Ignored
        );
    }

    #[test]
    fn a_second_gesture_inside_the_cooldown_cannot_flap_the_overlay() {
        let thresholds = GestureThresholds::default();
        let mut recognizer = GestureRecognizer::new(thresholds);
        let opened_at = Instant::now();
        assert_eq!(
            recognizer.observe(
                sample(GesturePhase::End, 0.9),
                OverlayVisibility::Hidden,
                opened_at
            ),
            GestureOutcome::Open
        );

        let inside = opened_at + thresholds.cooldown / 2;
        assert!(recognizer.in_cooldown(inside));
        assert_eq!(
            recognizer.observe(
                GestureSample::new(GesturePhase::End, GestureDirection::Outward, 0.9, 5),
                OverlayVisibility::Visible,
                inside
            ),
            GestureOutcome::Ignored
        );

        let outside = opened_at + thresholds.cooldown + Duration::from_millis(1);
        assert!(!recognizer.in_cooldown(outside));
        assert_eq!(
            recognizer.observe(
                GestureSample::new(GesturePhase::End, GestureDirection::Outward, 0.9, 5),
                OverlayVisibility::Visible,
                outside
            ),
            GestureOutcome::Close
        );
    }

    #[test]
    fn the_wrong_finger_count_is_never_a_weaker_version_of_the_gesture() {
        let mut recognizer = GestureRecognizer::default();
        let now = Instant::now();
        for fingers in [1u8, 2, 3, 4, 6] {
            assert_eq!(
                recognizer.observe(
                    GestureSample::new(GesturePhase::End, GestureDirection::Inward, 1.0, fingers),
                    OverlayVisibility::Hidden,
                    now
                ),
                GestureOutcome::Ignored,
                "{fingers} fingers must not open the launcher"
            );
        }
    }

    #[test]
    fn a_gesture_in_the_direction_that_changes_nothing_is_ignored() {
        let mut recognizer = GestureRecognizer::default();
        let now = Instant::now();
        assert_eq!(
            recognizer.observe(
                GestureSample::new(GesturePhase::End, GestureDirection::Outward, 1.0, 5),
                OverlayVisibility::Hidden,
                now
            ),
            GestureOutcome::Ignored
        );
        assert_eq!(
            recognizer.observe(
                GestureSample::new(GesturePhase::End, GestureDirection::Inward, 1.0, 5),
                OverlayVisibility::Visible,
                now
            ),
            GestureOutcome::Ignored
        );
    }

    #[test]
    fn progress_is_clamped_so_an_adapter_cannot_report_past_the_end() {
        let sample = GestureSample::new(GesturePhase::Update, GestureDirection::Inward, 4.2, 5);
        assert_eq!(sample.progress, 1.0);
        let sample = GestureSample::new(GesturePhase::Update, GestureDirection::Inward, -1.0, 5);
        assert_eq!(sample.progress, 0.0);
    }

    #[test]
    fn the_mock_adapter_replays_samples_in_order_and_describes_itself() {
        let adapter = MockGestureAdapter::new([
            sample(GesturePhase::Begin, 0.0),
            sample(GesturePhase::End, 1.0),
        ]);
        assert_eq!(adapter.describe().name, "mock");
        assert!(adapter.describe().continuous_progress);
        assert_eq!(adapter.next_sample().unwrap().phase, GesturePhase::Begin);
        assert_eq!(adapter.next_sample().unwrap().phase, GesturePhase::End);
        assert!(adapter.next_sample().is_none());

        let stepwise = MockGestureAdapter::without_progress([sample(GesturePhase::End, 1.0)]);
        assert!(!stepwise.describe().continuous_progress);
    }

    #[test]
    fn an_adapter_with_no_intermediate_progress_still_opens_the_overlay() {
        let adapter = MockGestureAdapter::without_progress([
            sample(GesturePhase::Begin, 0.0),
            sample(GesturePhase::End, 1.0),
        ]);
        let mut recognizer = GestureRecognizer::default();
        let now = Instant::now();
        let mut outcomes = Vec::new();
        while let Some(sample) = adapter.next_sample() {
            outcomes.push(recognizer.observe(sample, OverlayVisibility::Hidden, now));
        }
        assert_eq!(
            outcomes,
            vec![GestureOutcome::Progress(0.0), GestureOutcome::Open]
        );
    }
}
