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
//!
//! ## Which pad
//!
//! Gesture profiles are per device, and since ADR 0015 each event names the
//! device node that produced it. When a gesture begins, the node is turned into
//! the stable identity a profile is filed under, through [`DeviceIdentities`],
//! and the gesture is recognized against that device's own profile — or the
//! global one, for a device that is unknown or has no profile of its own.
//!
//! A gesture belongs to the device that began it. An event from any other
//! device while it is under way cancels it before that event is read, so frames
//! from two pads are never measured as one gesture.

use std::collections::BTreeMap;
use std::path::PathBuf;

use touchpad_core::TouchpadStore;
use touchpad_gestures::{
    AdapterFailures, CompositorGesture, CompositorPhase, EventRecognizer, GestureConfig,
    GestureEvent, GestureEventKind, GestureProfiles, GestureStore, RunState, SuppressionEvent,
    SuppressionState, plan::bind_all,
};
use touchpad_session::{
    InputDeviceNode, InvocationOutcome, SessionAdapter, ShellEvents, ShellGestureEvent,
    SuppressionOutcome,
};

/// Turns the device node a gesture arrived with into the stable identity a
/// gesture profile is filed under.
///
/// A node is whatever the kernel enumerated this boot, so it is asked about
/// when a gesture begins and the answer is not kept.
pub trait DeviceIdentities {
    /// The identity of the device behind this node right now, or `None` when
    /// there is no device there to read.
    fn identity(&self, node: InputDeviceNode) -> Option<String>;
}

/// Knows no device. Every gesture is recognized against the global profile.
pub struct NoDeviceIdentities;

impl DeviceIdentities for NoDeviceIdentities {
    fn identity(&self, _node: InputDeviceNode) -> Option<String> {
        None
    }
}

/// Reads a device's identity from `/sys/class/input/event<N>/device`.
///
/// Those are the same `uniq`, bus, vendor, product, version, and name
/// `touchpad-platform` reads from `/proc/bus/input/devices`, and both build the
/// identity with [`touchpad_core::device_identity`], so a profile the window
/// filed under a pad is the profile that pad's gestures find here.
pub struct SysfsDeviceIdentities {
    sys: PathBuf,
}

impl SysfsDeviceIdentities {
    pub fn system() -> Self {
        Self::at("/sys")
    }

    /// A different root, so a test never reads the machine it runs on.
    pub fn at(sys: impl Into<PathBuf>) -> Self {
        Self { sys: sys.into() }
    }
}

impl DeviceIdentities for SysfsDeviceIdentities {
    fn identity(&self, node: InputDeviceNode) -> Option<String> {
        let device = self
            .sys
            .join("class/input")
            .join(node.handler())
            .join("device");
        let read = |name: &str| {
            std::fs::read_to_string(device.join(name))
                .ok()
                .map(|text| text.trim().to_string())
        };
        // A device with no `uniq` attribute has no unique id, which is what
        // an empty one means too.
        let unique_id = read("uniq").unwrap_or_default();
        Some(touchpad_core::device_identity(
            &unique_id,
            &read("id/bustype")?,
            &read("id/vendor")?,
            &read("id/product")?,
            &read("id/version")?,
            &read("name")?,
        ))
    }
}

/// Which profile a gesture is recognized against: a device's own, by identity,
/// or `None` for the global one.
type ProfileKey = Option<String>;

/// The gesture under way, and the device that began it.
struct LiveGesture {
    device: Option<InputDeviceNode>,
    profile: ProfileKey,
}

/// One thing the pipeline did, in the order it did it. This is what the tests
/// assert against and what `--once` prints.
#[derive(Clone, Debug, PartialEq)]
pub struct Performed {
    /// The device profile the gesture was recognized against, by identity, or
    /// `None` for the global profile.
    pub profile: Option<String>,
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
    profiles: GestureProfiles,
    store: GestureStore,
    adapter: Box<dyn SessionAdapter>,
    identities: Box<dyn DeviceIdentities>,
    /// One recognizer per profile that is switched on, so each profile keeps
    /// its own cooldown. A profile that is off has none, and a pad on it
    /// recognizes nothing rather than borrowing the global profile.
    recognizers: BTreeMap<ProfileKey, EventRecognizer>,
    live: Option<LiveGesture>,
    suppression: SuppressionState,
    failures: AdapterFailures,
    performed: Vec<Performed>,
    problem: Option<String>,
}

impl GesturePipeline {
    /// Builds the pipeline around one configuration that every pad follows.
    pub fn new(
        config: GestureConfig,
        store: GestureStore,
        adapter: Box<dyn SessionAdapter>,
    ) -> Self {
        Self::with_profiles(
            GestureProfiles::global_only(config),
            store,
            adapter,
            Box::new(NoDeviceIdentities),
        )
    }

    /// Builds the pipeline around every stored profile.
    ///
    /// The recognizers are built from the gestures that are enabled *now*. A
    /// configuration change reaches them by restarting the service, which is
    /// what the window's Apply already asks a user to accept, and is why there
    /// is no file watch here inventing a third way for the two to disagree.
    pub fn with_profiles(
        profiles: GestureProfiles,
        store: GestureStore,
        adapter: Box<dyn SessionAdapter>,
        identities: Box<dyn DeviceIdentities>,
    ) -> Self {
        let mut pipeline = Self {
            profiles,
            store,
            adapter,
            identities,
            recognizers: BTreeMap::new(),
            live: None,
            suppression: SuppressionState::new(),
            failures: AdapterFailures::default(),
            performed: Vec::new(),
            problem: None,
        };
        pipeline.build_recognizers();
        pipeline
    }

    fn build_recognizers(&mut self) {
        self.recognizers = self
            .each_profile()
            .filter(|(_, profile)| profile.enabled)
            .map(|(key, profile)| (key, EventRecognizer::new(profile.active())))
            .collect();
    }

    /// The global profile and every device profile, with the key each is
    /// recognized under.
    fn each_profile(&self) -> impl Iterator<Item = (ProfileKey, &GestureConfig)> {
        std::iter::once((None, &self.profiles.global)).chain(
            self.profiles
                .devices
                .iter()
                .map(|(identity, profile)| (Some(identity.clone()), profile)),
        )
    }

    fn profile_mut(&mut self, key: &ProfileKey) -> Option<&mut GestureConfig> {
        match key {
            None => Some(&mut self.profiles.global),
            Some(identity) => self.profiles.devices.get_mut(identity),
        }
    }

    pub fn profiles(&self) -> &GestureProfiles {
        &self.profiles
    }

    pub fn performed(&self) -> &[Performed] {
        &self.performed
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Whether any profile is switched on.
    pub fn is_enabled(&self) -> bool {
        self.each_profile().any(|(_, profile)| profile.enabled)
    }

    pub fn suppression(&self) -> &SuppressionState {
        &self.suppression
    }

    /// Binds and verifies every configured gesture, in every profile, and
    /// writes the results back.
    ///
    /// The window reads the same file, so a row's verification result is what
    /// the process that actually performs the gesture found, rather than what a
    /// window with a different adapter guessed. The state returned is the
    /// worst any profile reached.
    pub fn verify(&mut self) -> RunState {
        let keys: Vec<ProfileKey> = self.each_profile().map(|(key, _)| key).collect();
        let mut worst = RunState::NothingToDo;
        for key in keys {
            let Some(profile) = self.profile_mut(&key).cloned() else {
                continue;
            };
            let (verified, report) = bind_all(&profile, self.adapter.as_mut());
            if let Some(slot) = self.profile_mut(&key) {
                *slot = verified;
            }
            worst = worst.max(report.state());
        }
        self.save();
        worst
    }

    /// Asks the desktop to give up the gestures the confirmed configuration
    /// took from it.
    ///
    /// The decision was made and confirmed in the window, and it is recorded in
    /// the stored configuration: a gesture that is enabled and conflicts with a
    /// built-in one is one the user chose to keep. Re-deciding it here would be
    /// a second opinion about something the user already answered.
    ///
    /// GNOME 46 has one set of swipe trackers for every pad, so a gesture any
    /// pad's profile takes from the desktop is taken on every pad.
    pub fn suppress_built_in_gestures(&mut self) -> SuppressionOutcome {
        let wanted = self.each_profile().any(|(_, profile)| {
            profile.enabled
                && profile
                    .gestures
                    .iter()
                    .any(|gesture| gesture.enabled && gesture.conflict.conflicts())
        });
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
        if !self.is_enabled() {
            return Vec::new();
        }
        let Some(compositor) = CompositorGesture::from_shell(event) else {
            return Vec::new();
        };
        let device = event.device();
        // The profile is chosen when a gesture begins, and only then.
        let beginning =
            (compositor.phase == CompositorPhase::Begin).then(|| self.profile_for(device));

        let mut recognized = Vec::new();
        if let Some(live) = self.live.take() {
            let continues = live.device == device
                && beginning
                    .as_ref()
                    .is_none_or(|profile| *profile == live.profile);
            if continues {
                self.live = Some(live);
            } else {
                // Another pad, or the same node now naming another device: the
                // gesture under way ends here, before this event is read.
                recognized.extend(self.cancel(live.profile, compositor.at_ms));
            }
        }

        let profile = match beginning {
            Some(profile) => {
                self.live = Some(LiveGesture {
                    device,
                    profile: profile.clone(),
                });
                Some(profile)
            }
            None => match compositor.phase {
                CompositorPhase::End | CompositorPhase::Cancel => {
                    self.live.take().map(|live| live.profile)
                }
                // An update with no gesture under way belongs to nothing: the
                // stream started mid-gesture, or its gesture was cancelled.
                _ => self.live.as_ref().map(|live| live.profile.clone()),
            },
        };
        if let Some(profile) = profile
            && let Some(recognizer) = self.recognizers.get_mut(&profile)
        {
            recognized.extend(
                recognizer
                    .observe(&compositor)
                    .into_iter()
                    .map(|event| (profile.clone(), event)),
            );
        }

        let mut performed = Vec::new();
        for (profile, event) in &recognized {
            if let Some(one) = self.perform(profile, event) {
                performed.push(one);
            }
        }
        self.performed.extend(performed.iter().cloned());
        performed
    }

    /// The profile a gesture from this device is recognized against: the
    /// device's own if it has one, the global one otherwise.
    fn profile_for(&self, device: Option<InputDeviceNode>) -> ProfileKey {
        let identity = device.and_then(|node| self.identities.identity(node))?;
        self.profiles
            .devices
            .contains_key(&identity)
            .then_some(identity)
    }

    /// Ends the gesture under way on this profile's recognizer.
    fn cancel(&mut self, profile: ProfileKey, at_ms: u64) -> Vec<(ProfileKey, GestureEvent)> {
        let Some(recognizer) = self.recognizers.get_mut(&profile) else {
            return Vec::new();
        };
        // A cancel carries nothing but its time, whichever stream it is on.
        let cancel = CompositorGesture::swipe(CompositorPhase::Cancel, 0, 0.0, 0.0, at_ms);
        recognizer
            .observe(&cancel)
            .into_iter()
            .map(|event| (profile.clone(), event))
            .collect()
    }

    /// Invokes the action a recognized event belongs to.
    ///
    /// Every phase is forwarded, not only the last one. An adapter that follows
    /// a gesture animates from these; one that does not reports the update as
    /// ignored and acts on the end. That is the whole of "continuous progress
    /// where the adapter supports it", and it is one code path rather than two.
    fn perform(&mut self, profile: &ProfileKey, event: &GestureEvent) -> Option<Performed> {
        let gesture = self
            .profiles
            .resolve(profile.as_deref())
            .get(&event.gesture)?;
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
            profile: profile.clone(),
            gesture: event.gesture.to_string(),
            action: action.key(),
            kind: event.kind,
            progress: event.progress,
            outcome,
        })
    }

    /// Turns the integration off after a repeatedly failing adapter.
    ///
    /// Every profile goes off, because every pad shares the adapter that
    /// failed. It writes the configuration, because the window has to open on
    /// the same state the service is in, and it restores the desktop's
    /// gestures, because an integration that is off must not still be holding
    /// them.
    fn auto_disable(&mut self) {
        self.profiles.global.enabled = false;
        for profile in self.profiles.devices.values_mut() {
            profile.enabled = false;
        }
        self.recognizers.clear();
        self.live = None;
        self.problem = Some(format!(
            "gestures.adapter_disabled_after_failures:{}",
            self.failures.consecutive()
        ));
        self.save();
        self.suppression
            .transition(SuppressionEvent::Disabled, self.adapter.as_mut());
    }

    /// Writes the profiles this pipeline holds back into the stored document.
    ///
    /// Only what this pipeline owns is replaced: the global profile and the
    /// device profiles the document still has. A device profile the window
    /// dropped since this service started stays dropped, and the window's
    /// selected pad is left alone.
    fn save(&mut self) {
        let mut stored = self.store.load_profiles().unwrap_or_default();
        stored.global = self.profiles.global.clone();
        for (identity, profile) in &self.profiles.devices {
            if let Some(slot) = stored.devices.get_mut(identity) {
                *slot = profile.clone();
            }
        }
        if let Err(error) = self.store.save_profiles(&stored) {
            self.problem = Some(error.to_string());
        }
    }

    /// Reads events until the stream ends or the integration turns itself off.
    pub fn run(&mut self, events: &mut dyn ShellEvents) -> StopReason {
        while let Some(event) = events.recv() {
            self.observe(&event);
            if !self.is_enabled() {
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
