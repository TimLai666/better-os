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
//! And ticket 131 added the fourth: **never suppress while not working.**
//! GNOME's own swipe gestures are taken away only for a gesture that really
//! collides with one of them under the current model, that is switched on,
//! that verified against the adapter, and only once a gesture has actually
//! arrived from the adapter, which is the evidence that the event path works.
//! A pipeline that has seen nothing takes nothing. When the adapter leaves the
//! bus or the service stops, GNOME has its gestures back: the extension
//! restores them itself the moment the service's bus name loses its owner,
//! and [`serve`] restores them on every way out it controls.
//!
//! ## Waiting for the adapter
//!
//! A login starts this service a moment before GNOME Shell enables its
//! extensions, so the adapter is very often not on the bus yet. [`serve`]
//! waits for it, starts when it appears, and when it goes away (a shell
//! restart, the extension turned off) gives GNOME its gestures back and waits
//! again. It ends only when it is asked to or when the session bus goes. With
//! nothing configured, in safe mode, or after the integration turned itself
//! off, it stays resident and idle, owning its bus name and watching the
//! configuration, so a change made in a window takes effect at once and
//! turning gestures back on is one action.
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
    AdapterFailures, CompositorGesture, CompositorGestureKind, CompositorPhase, EventRecognizer,
    GNOME_46_GESTURES, GestureConfig, GestureEvent, GestureEventKind, GestureProfiles,
    GestureStore, RunState, SuppressionEvent, SuppressionState, VerificationRecord,
    detect_conflicts, plan::bind_all,
};
use touchpad_session::{
    GnomeShellAdapter, InputDeviceNode, InvocationOutcome, ServiceBus, SessionAdapter,
    SessionEvent, ShellEvents, ShellGestureEvent, SuppressionOutcome,
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

impl SysfsDeviceIdentities {
    fn read(&self, handler: &str, name: &str) -> Option<String> {
        std::fs::read_to_string(
            self.sys
                .join("class/input")
                .join(handler)
                .join("device")
                .join(name),
        )
        .ok()
        .map(|text| text.trim().to_string())
    }

    /// The bus, vendor, product, version, and name: what a scroll
    /// pass-through clone copies from its touchpad.
    fn ids(&self, handler: &str) -> Option<[String; 5]> {
        Some([
            self.read(handler, "id/bustype")?,
            self.read(handler, "id/vendor")?,
            self.read(handler, "id/product")?,
            self.read(handler, "id/version")?,
            self.read(handler, "name")?,
        ])
    }

    fn is_clone(&self, handler: &str) -> bool {
        self.read(handler, "phys")
            .is_some_and(|phys| phys.starts_with(touchpad_scroll::caps::PHYS_PREFIX))
    }

    fn own_identity(&self, handler: &str) -> Option<String> {
        let [bus, vendor, product, version, name] = self.ids(handler)?;
        // A device with no `uniq` attribute has no unique id, which is what
        // an empty one means too.
        let unique_id = self.read(handler, "uniq").unwrap_or_default();
        Some(touchpad_core::device_identity(
            &unique_id, &bus, &vendor, &product, &version, &name,
        ))
    }
}

impl DeviceIdentities for SysfsDeviceIdentities {
    /// While the scroll pass-through holds a touchpad (ADR 0030), its gestures
    /// come from the clone's node, and the clone carries no `uniq`. So a
    /// clone's node is resolved to the touchpad it stands in for: the one
    /// present, non-clone device with the same bus, vendor, product, version,
    /// and name. Two such devices with different identities cannot be told
    /// apart, and the gesture goes to the global profile rather than to a guess.
    fn identity(&self, node: InputDeviceNode) -> Option<String> {
        let handler = node.handler();
        if !self.is_clone(&handler) {
            return self.own_identity(&handler);
        }
        let ids = self.ids(&handler)?;
        let mut found: Option<String> = None;
        for entry in std::fs::read_dir(self.sys.join("class/input"))
            .ok()?
            .flatten()
        {
            let Ok(other) = entry.file_name().into_string() else {
                continue;
            };
            if !other.starts_with("event") || other == handler || self.is_clone(&other) {
                continue;
            }
            if self.ids(&other).as_ref() != Some(&ids) {
                continue;
            }
            let identity = self.own_identity(&other)?;
            match &found {
                Some(previous) if *previous != identity => return None,
                _ => found = Some(identity),
            }
        }
        found
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
    /// Whether a gesture has arrived from the adapter this pipeline performs
    /// through: the evidence that the event path works, without which GNOME's
    /// own gestures are never taken (ticket 131).
    events_seen: bool,
    /// Whether the "GNOME keeps its gestures until one arrives" line was
    /// written, so it is written once.
    told_waiting_for_evidence: bool,
    /// Journal lines not yet handed out, in order.
    journal: Vec<String>,
    /// The document as this pipeline last read or wrote it: what its own
    /// changes are measured against when it writes (ticket 131).
    base: GestureProfiles,
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
    /// The recognizers are built from the gestures that are enabled now; a
    /// configuration written later reaches them through
    /// [`Self::replace_profiles`], which [`serve`] calls whenever the file
    /// changes (ticket 131).
    ///
    /// The adapter's own report of whether GNOME's gestures are off is
    /// adopted, so a suppression an older service left behind is lifted by the
    /// first transition that wants none.
    pub fn with_profiles(
        mut profiles: GestureProfiles,
        store: GestureStore,
        adapter: Box<dyn SessionAdapter>,
        identities: Box<dyn DeviceIdentities>,
    ) -> Self {
        let base = profiles.clone();
        profiles.annotate_conflicts();
        let mut suppression = SuppressionState::new();
        suppression.adopt_reported(adapter.reported_built_in_suppression());
        let mut pipeline = Self {
            profiles,
            store,
            adapter,
            identities,
            recognizers: BTreeMap::new(),
            live: None,
            suppression,
            failures: AdapterFailures::default(),
            performed: Vec::new(),
            problem: None,
            events_seen: false,
            told_waiting_for_evidence: false,
            journal: Vec::new(),
            base,
        };
        if pipeline.suppression.is_suppressed() {
            pipeline.note(
                "the adapter reports GNOME's gestures off from before this service started"
                    .to_string(),
            );
        }
        pipeline.build_recognizers();
        pipeline
    }

    /// Takes a configuration read again from the file (ticket 131).
    ///
    /// Returns `false`, and changes nothing, when it is the configuration this
    /// pipeline already holds, which is what its own writes read back as.
    /// Otherwise the gesture under way ends, the recognizers are rebuilt,
    /// everything is verified again, and GNOME's gestures are taken or given
    /// back for what now collides.
    pub fn replace_profiles(&mut self, mut profiles: GestureProfiles) -> bool {
        let read = profiles.clone();
        profiles.annotate_conflicts();
        if profiles == self.profiles {
            self.base = read;
            return false;
        }
        self.base = read;
        self.profiles = profiles;
        self.live = None;
        // Turning gestures back on after the automatic disable starts the
        // count again rather than one failure away from the next disable.
        self.failures = AdapterFailures::default();
        self.problem = None;
        self.build_recognizers();
        self.note("the gesture configuration changed; reading it again".to_string());
        self.verify();
        self.suppress_built_in_gestures();
        true
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
            let (mut verified, report) = bind_all(&profile, self.adapter.as_mut());
            // What collides is worked out again against the model as it
            // stands, by the one function the window uses too, so a file
            // annotated by a build with a wrong model (all of 0.2.16's, which
            // said GNOME took four fingers) is corrected (ticket 131).
            touchpad_gestures::conflict::annotate_against_gnome_46(&mut verified);
            if let Some(slot) = self.profile_mut(&key) {
                *slot = verified;
            }
            worst = worst.max(report.state());
        }
        self.save();
        self.note(format!("gestures verified: {worst:?}"));
        worst
    }

    /// The gestures that take one of GNOME's own away, by id: switched on in a
    /// profile that is on, colliding with GNOME 46's trackers as the model now
    /// stands, and verified to perform through the adapter.
    ///
    /// Taking a gesture is the person's decision and is recorded in the stored
    /// configuration: a colliding gesture they kept switched on is one they
    /// chose over GNOME's (resolving it the other way switches ours off). The
    /// collision itself is not read from the file, because a file written by a
    /// build with a wrong model would take GNOME's gestures for nothing; and a
    /// gesture that cannot be performed takes nothing, because suppressing
    /// GNOME's for it would leave the person with neither.
    pub fn colliding_gestures(&self) -> Vec<String> {
        let mut colliding = Vec::new();
        for (_, profile) in self.each_profile() {
            if !profile.enabled {
                continue;
            }
            for conflict in detect_conflicts(profile, GNOME_46_GESTURES) {
                let Some(gesture) = profile.get(&conflict.gesture) else {
                    continue;
                };
                let verified = matches!(
                    gesture.last_verification,
                    VerificationRecord::Verified { .. }
                );
                let id = gesture.id.to_string();
                if gesture.enabled && verified && !colliding.contains(&id) {
                    colliding.push(id);
                }
            }
        }
        colliding
    }

    /// Whether a gesture has arrived from the adapter yet.
    pub fn has_seen_events(&self) -> bool {
        self.events_seen
    }

    /// Takes GNOME's own gestures away when, and only when, the pipeline has
    /// shown it works and a gesture really needs them (ticket 131); gives them
    /// back otherwise.
    ///
    /// GNOME 46 has one set of swipe trackers for every pad, so a gesture any
    /// pad's profile takes from the desktop is taken on every pad.
    pub fn suppress_built_in_gestures(&mut self) -> SuppressionOutcome {
        let colliding = self.colliding_gestures();
        if !colliding.is_empty() && !self.events_seen && !self.told_waiting_for_evidence {
            self.told_waiting_for_evidence = true;
            self.note(format!(
                "GNOME keeps its own gestures until a gesture arrives from the adapter; \
                 then {} take them",
                colliding.join(", ")
            ));
        }
        let wanted = !colliding.is_empty() && self.events_seen;
        let before = self.suppression.is_suppressed();
        let outcome = self.suppression.transition(
            SuppressionEvent::PlanApplied { wanted },
            self.adapter.as_mut(),
        );
        let after = self.suppression.is_suppressed();
        match &outcome {
            SuppressionOutcome::Suppressed if !before => self.note(format!(
                "built-in gestures suppressed: gestures arrive from the adapter, and {} \
                 collide with GNOME's three-finger swipes",
                colliding.join(", ")
            )),
            SuppressionOutcome::Restored if before && !after => self.note(
                "built-in gestures restored: no gesture that works collides with GNOME's"
                    .to_string(),
            ),
            SuppressionOutcome::Failed { reason, detail } => self.note(format!(
                "built-in gestures could not be {}: {reason}: {detail}",
                if wanted { "suppressed" } else { "restored" }
            )),
            SuppressionOutcome::Unsupported { reason, .. } if wanted => {
                self.note(format!("built-in gestures cannot be suppressed: {reason}"))
            }
            _ => {}
        }
        outcome
    }

    /// Gives the desktop its own gestures back.
    pub fn restore_built_in_gestures(&mut self, event: SuppressionEvent) -> SuppressionOutcome {
        let before = self.suppression.is_suppressed();
        let outcome = self.suppression.transition(event, self.adapter.as_mut());
        match &outcome {
            SuppressionOutcome::Restored if before => self.note(format!(
                "built-in gestures restored ({}): GNOME has its own gestures back",
                event.key()
            )),
            SuppressionOutcome::Failed { reason, detail } => self.note(format!(
                "built-in gestures could not be restored ({}): {reason}: {detail}",
                event.key()
            )),
            _ => {}
        }
        outcome
    }

    /// The journal lines written since the last call, in order.
    pub fn take_journal(&mut self) -> Vec<String> {
        std::mem::take(&mut self.journal)
    }

    fn note(&mut self, line: String) {
        self.journal.push(line);
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
        if !self.events_seen {
            // The event path works: the extension saw a gesture and it reached
            // this process. That is what taking GNOME's gestures waits for.
            self.events_seen = true;
            self.note("the first gesture arrived from the adapter".to_string());
            self.suppress_built_in_gestures();
        }
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
        let ended = match compositor.phase {
            CompositorPhase::Begin => Some("began"),
            CompositorPhase::End => Some("ended"),
            CompositorPhase::Cancel => Some("was cancelled"),
            CompositorPhase::Update => None,
        };
        if let (Some(what), Some(key)) = (ended, profile.as_ref()) {
            self.note(format!(
                "gesture {what}: {} with {} fingers on {}, {}",
                match compositor.kind {
                    CompositorGestureKind::Swipe => "swipe",
                    CompositorGestureKind::Pinch => "pinch",
                },
                compositor.fingers,
                device.map_or_else(|| "an unknown device".to_string(), InputDeviceNode::wire),
                profile_words(key),
            ));
        }
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
            self.note(format!(
                "performed {} for {} ({}): {}",
                action.key(),
                event.gesture,
                profile_words(profile),
                outcome_words(&outcome),
            ));
            if self.failures.record(state) {
                self.auto_disable();
            }
        } else if let InvocationOutcome::Failed { reason, detail } = &outcome {
            self.note(format!(
                "{} for {} failed during the gesture: {reason}: {detail}",
                action.key(),
                event.gesture
            ));
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
        // Written by the same merge the window uses, so only the switch moves
        // on the file as it is now: whatever a window saved since this
        // service read it stays (ticket 131).
        self.save();
        // The profiles only say gestures are off. The marker says why, so
        // Better Manager's Home can tell the person after the fact.
        if let Err(error) = self.store.record_turned_off(self.failures.consecutive()) {
            self.problem = Some(error.to_string());
        }
        self.note(format!(
            "gestures turned off: {} gestures in a row failed through the adapter",
            self.failures.consecutive()
        ));
        self.restore_built_in_gestures(SuppressionEvent::Disabled);
    }

    /// Writes this pipeline's own changes onto the stored document as it is
    /// now (ticket 131).
    ///
    /// What changed between the document as this pipeline read it and its own
    /// copy (verification results, conflict annotations, the switch the
    /// automatic disable turns off) is merged onto what the file holds now,
    /// under the lock every writer of the file takes, by the one merge the
    /// windows use too ([`GestureStore::save_changes`]). A window that
    /// saved since this service read the file is never overwritten, and a
    /// file that will not parse is never written over.
    fn save(&mut self) {
        let ours = self.profiles.clone();
        match self.store.save_changes(&self.base, &ours) {
            Ok(_) => self.base = ours,
            Err(error) => self.problem = Some(error.to_string()),
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

/// A profile as a journal line names it.
fn profile_words(profile: &ProfileKey) -> String {
    match profile {
        None => "the global profile".to_string(),
        Some(identity) => format!("the profile for {identity}"),
    }
}

/// An invocation's outcome as a journal line names it.
fn outcome_words(outcome: &InvocationOutcome) -> String {
    match outcome {
        InvocationOutcome::Invoked => "done".to_string(),
        InvocationOutcome::Ignored { reason } => format!("ignored ({reason})"),
        InvocationOutcome::Unsupported { reason, .. } => format!("unsupported ({reason})"),
        InvocationOutcome::Failed { reason, detail } => format!("failed ({reason}: {detail})"),
    }
}

/// What the service waits on: the adapter's comings and goings, its
/// gestures, and a stop. [`ServiceBus`] is the session-bus one; a test drives
/// [`serve`] with a scripted one.
pub trait ServiceSession {
    /// Whether the adapter extension is on the bus now.
    fn adapter_present(&mut self) -> bool;
    /// The adapter's route, once it is there.
    fn connect_adapter(&mut self) -> Result<Box<dyn SessionAdapter>, String>;
    /// Blocks until something happens.
    fn next_event(&mut self) -> SessionEvent;
}

impl ServiceSession for ServiceBus {
    fn adapter_present(&mut self) -> bool {
        ServiceBus::adapter_present(self)
    }

    fn connect_adapter(&mut self) -> Result<Box<dyn SessionAdapter>, String> {
        GnomeShellAdapter::connect(Box::new(self.shell()))
            .map(|adapter| Box::new(adapter) as Box<dyn SessionAdapter>)
            .map_err(|error| error.to_string())
    }

    fn next_event(&mut self) -> SessionEvent {
        ServiceBus::next_event(self)
    }
}

/// Why [`serve`] returned, and whether GNOME has its own gestures.
///
/// `restored` is true when nothing of GNOME's is held any more: given back, or
/// never taken. The extension also gives them back by itself once the
/// service's bus name has no owner, so a `false` here is a restore that will
/// still happen, said in the journal.
///
/// There is no "finished on purpose": the service stays resident with nothing
/// configured, in safe mode, and after the integration turned itself off,
/// watching the configuration, so turning gestures back on is one action
/// (ticket 131).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceExit {
    /// SIGTERM or SIGINT.
    Stopped { restored: bool },
    /// The session bus is gone.
    Closed { restored: bool },
    /// The adapter appeared and did not answer.
    Failed(String),
}

/// What the stored configuration says, read again on every change.
#[derive(Clone, Debug, PartialEq)]
pub enum Configuration {
    Profiles(GestureProfiles),
    /// Safe mode is on: nothing is recognized and GNOME keeps its gestures.
    SafeMode,
    /// The file will not parse. Nothing is recognized: starting from
    /// defaults would recognize gestures the person never configured, and the
    /// file is left exactly as it is for the window to explain.
    Unreadable(String),
}

impl Configuration {
    /// The profiles the pipeline runs on: the stored ones, or none at all.
    fn effective(&self) -> GestureProfiles {
        match self {
            Self::Profiles(profiles) => profiles.clone(),
            Self::SafeMode | Self::Unreadable(_) => {
                let mut nothing = GestureProfiles::default();
                nothing.global.enabled = false;
                nothing
            }
        }
    }

    /// One journal line for the state this configuration puts the service in.
    pub fn words(&self) -> String {
        match self {
            Self::SafeMode => "safe mode is on: nothing is recognized and GNOME keeps its own \
                               gestures; waiting for it to be turned off"
                .to_string(),
            Self::Unreadable(error) => format!(
                "the gesture configuration could not be read ({error}); nothing is \
                 recognized until it can be"
            ),
            Self::Profiles(profiles) => {
                let listening = std::iter::once(&profiles.global)
                    .chain(profiles.devices.values())
                    .any(|profile| !profile.active().is_empty());
                if listening {
                    "gestures are configured".to_string()
                } else {
                    "no gesture is switched on; waiting for one to be configured".to_string()
                }
            }
        }
    }
}

/// Runs the service until it is asked to stop (ticket 131).
///
/// It waits for the adapter rather than exiting without it, and builds a
/// pipeline with `pipeline_for` each time the adapter appears, dropping it
/// when the adapter goes. `configuration` is read at the start and again on
/// every [`SessionEvent::ConfigurationChanged`], and a running pipeline takes
/// the new profiles at once: gestures turned off stop being recognized, new
/// ones are recognized, and GNOME's gestures are taken or given back for what
/// now collides. Every journal line goes to `log`.
pub fn serve(
    session: &mut dyn ServiceSession,
    configuration: &mut dyn FnMut() -> Configuration,
    pipeline_for: &mut dyn FnMut(Box<dyn SessionAdapter>, GestureProfiles) -> GesturePipeline,
    log: &mut dyn FnMut(String),
) -> ServiceExit {
    let mut current = configuration();
    log(current.words());
    let mut pipeline: Option<GesturePipeline> = None;
    let mut waiting_since = None;

    if session.adapter_present() {
        match attach(session, &current, pipeline_for, log, None) {
            Ok(started) => pipeline = Some(started),
            Err(error) => return ServiceExit::Failed(error),
        }
    } else {
        log(format!(
            "waiting for the GNOME Shell adapter extension ({}) to appear on the session bus",
            touchpad_session::gnome::BUS_NAME
        ));
        waiting_since = Some(std::time::Instant::now());
    }

    loop {
        match session.next_event() {
            SessionEvent::AdapterAppeared => {
                if pipeline.take().is_some() {
                    log("the adapter was replaced; starting again with the new one".to_string());
                }
                match attach(session, &current, pipeline_for, log, waiting_since.take()) {
                    Ok(started) => pipeline = Some(started),
                    Err(error) => return ServiceExit::Failed(error),
                }
            }
            SessionEvent::AdapterVanished => {
                // The extension restores GNOME's trackers itself when it is
                // disabled, and a shell that went away took them with it, so
                // there is nobody to call. What this process held goes with
                // the pipeline.
                if pipeline.take().is_some() {
                    log(
                        "the GNOME Shell adapter left the session bus; GNOME keeps its own \
                         gestures, and no gesture is recognized until it is back"
                            .to_string(),
                    );
                }
                if waiting_since.is_none() {
                    log("waiting for the GNOME Shell adapter extension to come back".to_string());
                    waiting_since = Some(std::time::Instant::now());
                }
            }
            SessionEvent::ConfigurationChanged => {
                let read = configuration();
                if read.words() != current.words() {
                    log(read.words());
                }
                current = read;
                if let Some(running) = pipeline.as_mut() {
                    running.replace_profiles(current.effective());
                    for line in running.take_journal() {
                        log(line);
                    }
                }
            }
            SessionEvent::Gesture(event) => {
                let Some(running) = pipeline.as_mut() else {
                    continue;
                };
                let was_enabled = running.is_enabled();
                running.observe(&event);
                for line in running.take_journal() {
                    log(line);
                }
                if was_enabled && !running.is_enabled() {
                    if let Some(problem) = running.problem() {
                        log(problem.to_string());
                    }
                    log(
                        "waiting for gestures to be turned back on; the service stays running"
                            .to_string(),
                    );
                }
            }
            SessionEvent::Stop => {
                return ServiceExit::Stopped {
                    restored: give_back(pipeline.as_mut(), log),
                };
            }
            SessionEvent::Closed => {
                return ServiceExit::Closed {
                    restored: give_back(pipeline.as_mut(), log),
                };
            }
        }
    }
}

/// Starts a pipeline on an adapter that just appeared.
fn attach(
    session: &mut dyn ServiceSession,
    current: &Configuration,
    pipeline_for: &mut dyn FnMut(Box<dyn SessionAdapter>, GestureProfiles) -> GesturePipeline,
    log: &mut dyn FnMut(String),
    waiting_since: Option<std::time::Instant>,
) -> Result<GesturePipeline, String> {
    match waiting_since {
        Some(since) => log(format!(
            "the GNOME Shell adapter appeared after {:.1} s of waiting",
            since.elapsed().as_secs_f32()
        )),
        None => log("the GNOME Shell adapter is on the session bus".to_string()),
    }
    let adapter = session
        .connect_adapter()
        .map_err(|error| format!("the GNOME Shell adapter did not answer: {error}"))?;
    let mut pipeline = pipeline_for(adapter, current.effective());
    pipeline.verify();
    // Whatever an older service left behind is settled here too: the
    // pipeline adopted the adapter's report, so wanting nothing restores.
    pipeline.suppress_built_in_gestures();
    for line in pipeline.take_journal() {
        log(line);
    }
    Ok(pipeline)
}

/// Gives GNOME its gestures back on the way out, and says whether they are.
fn give_back(pipeline: Option<&mut GesturePipeline>, log: &mut dyn FnMut(String)) -> bool {
    let Some(pipeline) = pipeline else {
        return true;
    };
    let outcome = pipeline.restore_built_in_gestures(SuppressionEvent::Restored);
    for line in pipeline.take_journal() {
        log(line);
    }
    matches!(outcome, SuppressionOutcome::Restored)
}

/// Reads what the stored configuration says now.
pub fn read_configuration(settings: &TouchpadStore, gestures: &GestureStore) -> Configuration {
    if settings.safe_mode_enabled() {
        return Configuration::SafeMode;
    }
    match gestures.load_profiles() {
        Ok(profiles) => Configuration::Profiles(profiles),
        Err(error) => Configuration::Unreadable(error.to_string()),
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
