//! The resident gesture pipeline: compositor events in, typed actions out.
//!
//! ## Why this is its own service
//!
//! Better Touchpad's window is where gestures are configured, and a person
//! closes a settings window. A gesture that only worked while that window was
//! open would be a gesture nobody could rely on, so the thing that listens for
//! compositor events is a separate, tiny binary — `better-touchpad-gestured` —
//! the same split `better-awake-service` and `better-storage-service` already
//! use. It has no window, links no toolkit, and holds no privilege.
//!
//! It is its own crate rather than a binary inside `touchpad-session` for a
//! plainer reason: `touchpad-gestures` already depends on `touchpad-session`,
//! and the pipeline needs both, so it has to sit above them.
//!
//! ## What it does, and what it deliberately does not
//!
//! It loads the gesture configuration, binds and verifies every gesture,
//! recognizes what the compositor reports, and invokes the typed action. It
//! makes no gesture decision of its own: the thresholds, the cancellation rule,
//! and the cooldown are `touchpad-gestures`'s, and the actions are
//! `better-actions`'.
//!
//! Three safety rules from Issue #3 and ticket 30 are wired here rather than
//! described:
//!
//! - **Safe mode wins.** With the marker present the pipeline gives the desktop
//!   its own gestures back and recognizes nothing.
//! - **A repeatedly failing adapter turns itself off.** Three failed
//!   invocations in a row disable the integration, write that to the
//!   configuration, and restore the desktop's gestures — the same rule and the
//!   same number the window uses, because both now share
//!   [`AdapterFailures`](touchpad_gestures::AdapterFailures).
//! - **Every way out restores.** Disabling, safe mode, and shutting down all go
//!   through the suppression state machine, so the desktop cannot be left
//!   without its own gestures by a Better OS process that stopped.

use touchpad_core::TouchpadStore;
use touchpad_gestures::{
    AdapterFailures, CompositorGesture, EventRecognizer, GestureConfig, GestureEvent,
    GestureEventKind, GestureStore, RunState, SuppressionEvent, SuppressionState, plan::bind_all,
};
use touchpad_session::{
    InvocationOutcome, SessionAdapter, ShellEvents, ShellGestureEvent, SuppressionOutcome,
};

/// One thing the pipeline did, in the order it did it. This is what the tests
/// assert against and what `--once` prints.
#[derive(Clone, Debug, PartialEq)]
pub struct Performed {
    pub gesture: String,
    pub action: &'static str,
    pub kind: GestureEventKind,
    pub progress: f32,
    pub outcome: InvocationOutcome,
}

/// Why the pipeline stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    /// The event stream ended: the extension went away, or the session did.
    StreamEnded,
    /// The adapter failed often enough to disable the integration.
    AutoDisabled,
    /// Safe mode was on before anything was recognized.
    SafeMode,
}

pub struct GesturePipeline {
    config: GestureConfig,
    store: GestureStore,
    adapter: Box<dyn SessionAdapter>,
    recognizer: EventRecognizer,
    suppression: SuppressionState,
    failures: AdapterFailures,
    performed: Vec<Performed>,
    problem: Option<String>,
}

impl GesturePipeline {
    /// Builds the pipeline around a configuration and an adapter.
    ///
    /// The recognizer is built from the gestures that are enabled *now*. A
    /// configuration change reaches it by restarting the service, which is what
    /// the window's Apply already asks a user to accept, and is why there is no
    /// file watch here inventing a third way for the two to disagree.
    pub fn new(
        config: GestureConfig,
        store: GestureStore,
        adapter: Box<dyn SessionAdapter>,
    ) -> Self {
        Self {
            recognizer: EventRecognizer::new(config.active()),
            config,
            store,
            adapter,
            suppression: SuppressionState::new(),
            failures: AdapterFailures::default(),
            performed: Vec::new(),
            problem: None,
        }
    }

    pub fn config(&self) -> &GestureConfig {
        &self.config
    }

    pub fn performed(&self) -> &[Performed] {
        &self.performed
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn suppression(&self) -> &SuppressionState {
        &self.suppression
    }

    /// Binds and verifies every configured gesture and writes the results back.
    ///
    /// The window reads the same file, so a row's verification result is what
    /// the process that actually performs the gesture found, rather than what a
    /// window with a different adapter guessed.
    pub fn verify(&mut self) -> RunState {
        let (config, report) = bind_all(&self.config, self.adapter.as_mut());
        self.config = config;
        self.save();
        report.state()
    }

    /// Asks the desktop to give up the gestures the confirmed configuration
    /// took from it.
    ///
    /// The decision was made and confirmed in the window, and it is recorded in
    /// the stored configuration: a gesture that is enabled and conflicts with a
    /// built-in one is one the user chose to keep. Re-deciding it here would be
    /// a second opinion about something the user already answered.
    pub fn suppress_built_in_gestures(&mut self) -> SuppressionOutcome {
        let wanted = self.config.enabled
            && self
                .config
                .gestures
                .iter()
                .any(|gesture| gesture.enabled && gesture.conflict.conflicts());
        self.suppression.transition(
            SuppressionEvent::PlanApplied { wanted },
            self.adapter.as_mut(),
        )
    }

    /// Gives the desktop its own gestures back.
    pub fn restore_built_in_gestures(&mut self, event: SuppressionEvent) -> SuppressionOutcome {
        self.suppression.transition(event, self.adapter.as_mut())
    }

    /// Applies one event from the extension.
    ///
    /// A wire event this build cannot read is counted as nothing at all rather
    /// than turned into a gesture, which is [`CompositorGesture::from_shell`]'s
    /// rule and not this one's.
    pub fn observe(&mut self, event: &ShellGestureEvent) -> Vec<Performed> {
        if !self.config.enabled {
            return Vec::new();
        }
        let Some(compositor) = CompositorGesture::from_shell(event) else {
            return Vec::new();
        };
        let recognized = self.recognizer.observe(&compositor);
        let mut performed = Vec::new();
        for event in &recognized {
            if let Some(one) = self.perform(event) {
                performed.push(one);
            }
        }
        self.performed.extend(performed.iter().cloned());
        performed
    }

    /// Invokes the action a recognized event belongs to.
    ///
    /// Every phase is forwarded, not only the last one. An adapter that follows
    /// a gesture animates from these; one that does not reports the update as
    /// ignored and acts on the end. That is the whole of "continuous progress
    /// where the adapter supports it", and it is one code path rather than two.
    fn perform(&mut self, event: &GestureEvent) -> Option<Performed> {
        let gesture = self.config.get(&event.gesture)?;
        let action = gesture.action.clone();
        let outcome = self.adapter.invoke(&action, event.progress_report());

        // Only a completed gesture counts towards the automatic disable. A
        // progress update that an adapter ignored is not a failure, and
        // counting it would turn every discrete adapter into a broken one.
        if event.kind == GestureEventKind::Complete {
            let state = match &outcome {
                InvocationOutcome::Invoked => RunState::Applied,
                InvocationOutcome::Ignored { .. } => RunState::NothingToDo,
                InvocationOutcome::Unsupported { .. } => RunState::PartiallySupported,
                InvocationOutcome::Failed { .. } => RunState::Failed,
            };
            if self.failures.record(state) {
                self.auto_disable();
            }
        }

        Some(Performed {
            gesture: event.gesture.to_string(),
            action: action.key(),
            kind: event.kind,
            progress: event.progress,
            outcome,
        })
    }

    /// Turns the integration off after a repeatedly failing adapter.
    ///
    /// It writes the configuration, because the window has to open on the same
    /// state the service is in, and it restores the desktop's gestures, because
    /// an integration that is off must not still be holding them.
    fn auto_disable(&mut self) {
        self.config.enabled = false;
        self.problem = Some(format!(
            "gestures.adapter_disabled_after_failures:{}",
            self.failures.consecutive()
        ));
        self.save();
        self.suppression
            .transition(SuppressionEvent::Disabled, self.adapter.as_mut());
    }

    fn save(&mut self) {
        if let Err(error) = self.store.save_config(&self.config) {
            self.problem = Some(error.to_string());
        }
    }

    /// Reads events until the stream ends or the integration turns itself off.
    pub fn run(&mut self, events: &mut dyn ShellEvents) -> StopReason {
        while let Some(event) = events.recv() {
            self.observe(&event);
            if !self.config.enabled {
                return StopReason::AutoDisabled;
            }
        }
        StopReason::StreamEnded
    }
}

/// Whether safe mode is on for this store directory.
///
/// The marker is `touchpad-core`'s, and it is read rather than mirrored so that
/// `better-touchpad --safe-mode` from a text console turns the gestures off
/// too, without the two halves having to agree about a second file.
pub fn safe_mode_enabled(store: &TouchpadStore) -> bool {
    store.safe_mode_enabled()
}
