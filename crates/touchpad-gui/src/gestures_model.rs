//! Everything the Gestures screen decides, with no GPUI in it.
//!
//! The same split the rest of Better Touchpad follows: which rows exist, what
//! each one says, whether the preset can be applied, what a test run saw, and
//! whether an edit is legal are all decided here and asserted without a window.
//!
//! Two rules are worth reading the code for.
//!
//! **The screen cannot apply a plan the user has not confirmed.** It holds a
//! preview and a confirmation flag, and the only way past them is
//! `PresetPlan::approve`, which returns a type whose fields are private. There
//! is no second path.
//!
//! **Test mode performs no system action unless live testing is switched on,
//! and it is off by default.** The default adapter is the recording one, which
//! changes nothing whatever it is asked; live testing additionally hands each
//! recognized event to the adapter. Both halves are asserted.

use std::collections::{BTreeMap, BTreeSet};

use better_actions::{ActionSupport, DesktopAction, Key, KeyboardShortcut, Modifier};
use touchpad_gestures::{
    AdapterFailures, ApplyReport, BindingOutcome, ChangeKind, ConflictResolution, ContactCount,
    Cooldown, Direction, GestureConfig, GestureDefinition, GestureError, GestureEvent, GestureId,
    GestureProfiles, GestureShape, GestureStore, KnownShortcuts, PlanError, PresetId, PresetPlan,
    Recognizer, RecognizerScale, RestorePlan, RunState, ShortcutCheck, SuppressionEvent,
    SuppressionState, Threshold, VerificationRecord, mac_style, plan::bind_all, synthetic,
};
use touchpad_session::SessionAdapter;

use crate::i18n::Copy;

/// The compact direction diagram a row draws.
///
/// It is a description, not a picture: how many contacts, whether one of them
/// is the thumb, and which way they go. The renderer turns that into dots and
/// an arrow, and a test can assert the diagram without a display server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GestureGlyph {
    pub dots: u8,
    pub thumb: bool,
    pub arrow: Arrow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arrow {
    Up,
    Down,
    Left,
    Right,
    /// Contacts drawing together.
    In,
    /// Contacts spreading apart.
    Out,
    Turn,
    /// A hold or a tap: nothing moves.
    Still,
}

impl GestureGlyph {
    pub fn of(gesture: &GestureDefinition) -> Self {
        let arrow = match (gesture.shape, gesture.direction) {
            (GestureShape::Pinch, _) => Arrow::In,
            (GestureShape::Spread, _) => Arrow::Out,
            (GestureShape::Rotate, _) => Arrow::Turn,
            (GestureShape::Hold | GestureShape::Tap, _) => Arrow::Still,
            (GestureShape::Swipe, Some(Direction::Up)) => Arrow::Up,
            (GestureShape::Swipe, Some(Direction::Down)) => Arrow::Down,
            (GestureShape::Swipe, Some(Direction::Left)) => Arrow::Left,
            (GestureShape::Swipe, _) => Arrow::Right,
        };
        Self {
            dots: gesture.contacts.get(),
            thumb: gesture.thumb_required,
            arrow,
        }
    }
}

/// One row of the gesture list, fully decided.
#[derive(Clone, Debug, PartialEq)]
pub struct GestureRow {
    pub id: GestureId,
    pub label: String,
    pub glyph: GestureGlyph,
    pub contacts: u8,
    pub shape_label: &'static str,
    pub direction_label: Option<&'static str>,
    pub action_label: String,
    pub enabled: bool,
    /// `Some` only when detection has actually found a collision. A gesture
    /// nobody has checked shows nothing rather than a reassuring tick.
    pub conflict: Option<String>,
    pub supported: bool,
    pub support_detail: Option<String>,
    pub verification: &'static str,
    pub can_animate: bool,
}

/// Whether the shipped preset is what is configured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresetStatus {
    NotApplied,
    Applied,
    /// The preset was applied and then edited. Saying "applied" here would be
    /// a small lie that makes the next preview confusing.
    Differs,
}

/// The preset card at the top of the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetCard {
    pub status: PresetStatus,
    pub status_label: &'static str,
    /// Every change the plan would make, one line each. Issue #3 requires the
    /// preview to list all of them, so this is the whole list and never a
    /// summary of it.
    pub changes: Vec<String>,
    pub conflicts: Vec<ConflictRow>,
    pub unsupported: Vec<String>,
    pub confirmed: bool,
    /// Whether pressing Apply would do anything. False while a conflict has no
    /// decision or the confirmation has not been given.
    pub can_apply: bool,
    pub blocked_reason: Option<&'static str>,
}

/// One conflict, and the decision it is waiting for.
#[derive(Clone, Debug, PartialEq)]
pub struct ConflictRow {
    pub gesture: GestureId,
    pub gesture_label: String,
    pub built_in: String,
    pub resolution: Option<ConflictResolution>,
}

/// One line of the Test gestures panel.
#[derive(Clone, Debug, PartialEq)]
pub struct TestLine {
    pub gesture: String,
    pub kind: &'static str,
    pub progress: f32,
}

/// What a test run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestRun {
    pub lines: Vec<TestLine>,
    /// How many actions were actually performed. Zero unless live testing is
    /// on, which is the property the screen promises.
    pub performed: usize,
}

/// Which part of the fixed key table a picker is showing.
///
/// The table has seventy-three keys, and seventy-three buttons is not a picker.
/// Splitting it is a presentation decision, so it is made here and asserted
/// without a window — including that the parts cover the table exactly once,
/// which is what stops a key becoming unreachable when the table grows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyGroup {
    Letters,
    Digits,
    Function,
    Navigation,
    Editing,
    Punctuation,
}

impl KeyGroup {
    pub const ALL: [Self; 6] = [
        Self::Letters,
        Self::Digits,
        Self::Function,
        Self::Navigation,
        Self::Editing,
        Self::Punctuation,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Letters => "letters",
            Self::Digits => "digits",
            Self::Function => "function",
            Self::Navigation => "navigation",
            Self::Editing => "editing",
            Self::Punctuation => "punctuation",
        }
    }

    /// The group a key belongs to. Every key belongs to exactly one.
    pub fn of(key: Key) -> Self {
        let name = key.name();
        let single = name.chars().count() == 1;
        if single
            && name
                .chars()
                .all(|character| character.is_ascii_alphabetic())
        {
            Self::Letters
        } else if single && name.chars().all(|character| character.is_ascii_digit()) {
            Self::Digits
        } else if name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit()) {
            Self::Function
        } else if matches!(
            name,
            "Left" | "Right" | "Up" | "Down" | "Home" | "End" | "Page_Up" | "Page_Down" | "Tab"
        ) {
            Self::Navigation
        } else if matches!(
            name,
            "Return" | "Escape" | "BackSpace" | "Delete" | "Insert" | "space"
        ) {
            Self::Editing
        } else {
            Self::Punctuation
        }
    }

    pub fn keys(self) -> Vec<Key> {
        Key::all().filter(|key| Self::of(*key) == self).collect()
    }
}

/// The keys chosen for a custom shortcut, while they are being chosen.
///
/// This is a modifier set and one key from the fixed table, and it is the only
/// shape the editor can hold — there is no text field anywhere in the path, so
/// nothing the user types can reach an action. An empty modifier set is a state
/// the draft can be in and a shortcut cannot, which is why building one returns
/// a result rather than a value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutDraft {
    pub modifiers: BTreeSet<Modifier>,
    pub key: Key,
}

impl ShortcutDraft {
    pub fn of(shortcut: &KeyboardShortcut) -> Self {
        Self {
            modifiers: shortcut.modifiers().collect(),
            key: shortcut.key(),
        }
    }

    /// The draft a gesture with no shortcut yet starts from.
    pub fn placeholder() -> Self {
        Self::of(&DesktopAction::placeholder_shortcut())
    }

    pub fn toggle(&mut self, modifier: Modifier) {
        if !self.modifiers.remove(&modifier) {
            self.modifiers.insert(modifier);
        }
    }

    pub fn holds(&self, modifier: Modifier) -> bool {
        self.modifiers.contains(&modifier)
    }

    pub fn group(&self) -> KeyGroup {
        KeyGroup::of(self.key)
    }

    pub fn build(&self) -> Result<KeyboardShortcut, GestureError> {
        KeyboardShortcut::new(self.modifiers.iter().copied(), self.key)
            .map_err(|error| GestureError::ShortcutNotUsable(error.to_string()))
    }

    /// The spelling to show, or the reason there is nothing to show yet.
    pub fn spelling(&self) -> Result<String, GestureError> {
        self.build().map(|shortcut| shortcut.to_gnome())
    }
}

/// The edit view for one gesture.
#[derive(Clone, Debug, PartialEq)]
pub struct GestureEditor {
    pub id: GestureId,
    pub shape: GestureShape,
    pub contacts: u8,
    pub thumb_required: bool,
    pub direction: Option<Direction>,
    pub action: DesktopAction,
    /// The keys a custom shortcut would use. Kept whatever the chosen action
    /// is, so switching away from the shortcut and back does not lose them.
    pub shortcut: ShortcutDraft,
    /// Which part of the key table the picker is showing.
    pub key_group: KeyGroup,
    pub activation: f32,
    pub cancellation: f32,
    pub cooldown_ms: u64,
    pub enabled: bool,
    pub error: Option<String>,
}

impl GestureEditor {
    pub fn of(gesture: &GestureDefinition) -> Self {
        let shortcut = match &gesture.action {
            DesktopAction::KeyboardShortcut { shortcut } => ShortcutDraft::of(shortcut),
            _ => ShortcutDraft::placeholder(),
        };
        Self {
            id: gesture.id.clone(),
            shape: gesture.shape,
            contacts: gesture.contacts.get(),
            thumb_required: gesture.thumb_required,
            direction: gesture.direction,
            action: gesture.action.clone(),
            key_group: shortcut.group(),
            shortcut,
            activation: gesture.activation_threshold.get(),
            cancellation: gesture.cancellation_threshold.get(),
            cooldown_ms: gesture.cooldown.as_millis(),
            enabled: gesture.enabled,
            error: None,
        }
    }

    /// Whether the chosen action is the custom shortcut, which is the one that
    /// needs the key picker on screen.
    pub fn action_is_shortcut(&self) -> bool {
        matches!(self.action, DesktopAction::KeyboardShortcut { .. })
    }

    /// Chooses an action. Picking the custom shortcut adopts the draft rather
    /// than the catalog row's placeholder binding, so the keys already chosen
    /// survive a trip through the action list.
    pub fn set_action(&mut self, action: DesktopAction) {
        self.action = match action {
            DesktopAction::KeyboardShortcut { .. } => DesktopAction::KeyboardShortcut {
                shortcut: self
                    .shortcut
                    .build()
                    .ok()
                    .unwrap_or_else(DesktopAction::placeholder_shortcut),
            },
            other => other,
        };
    }

    pub fn set_key(&mut self, key: Key) {
        self.shortcut.key = key;
        self.key_group = KeyGroup::of(key);
    }

    /// Changing the shape can invalidate the direction, so the editor keeps
    /// them consistent rather than letting the user reach a state the
    /// definition would refuse.
    pub fn set_shape(&mut self, shape: GestureShape) {
        self.shape = shape;
        self.direction = match (shape.needs_direction(), self.direction) {
            (false, _) => None,
            (true, Some(direction)) if shape.allowed_directions().contains(&direction) => {
                Some(direction)
            }
            (true, _) => shape.allowed_directions().first().copied(),
        };
    }

    /// The definition this editor describes, or why it is not one.
    pub fn build(&self, original: &GestureDefinition) -> Result<GestureDefinition, GestureError> {
        let mut gesture = original.clone();
        gesture.shape = self.shape;
        gesture.contacts = ContactCount::new(self.contacts)?;
        gesture.thumb_required = self.thumb_required;
        gesture.direction = self.direction;
        // The shortcut the gesture gets is always rebuilt from the draft, so
        // the keys on screen and the keys stored cannot disagree, and a draft
        // that is not a shortcut yet is refused here rather than saved.
        gesture.action = if self.action_is_shortcut() {
            DesktopAction::KeyboardShortcut {
                shortcut: self.shortcut.build()?,
            }
        } else {
            self.action.clone()
        };
        gesture.activation_threshold = Threshold::new(self.activation)?;
        gesture.cancellation_threshold = Threshold::new(self.cancellation)?;
        gesture.cooldown = Cooldown::from_millis(self.cooldown_ms)?;
        gesture.enabled = self.enabled;
        // An edit invalidates whatever the last run said about this gesture.
        gesture.last_verification = VerificationRecord::NotRun;
        gesture.validate()?;
        Ok(gesture)
    }
}

/// What an import would bring in, worked out before anything is applied.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportSummary {
    /// Where it came from, as the user named it.
    pub source: String,
    /// The device identities the file carries a profile for.
    pub device_profiles: Vec<String>,
    /// Whether the file has a profile for the device selected here. When it
    /// does not, the profile being previewed is the file's global one.
    pub matches_selected_device: bool,
}

/// The Gestures screen's whole state.
pub struct GestureScreen {
    profiles: GestureProfiles,
    /// The device whose profile is being edited, or `None` for the global one.
    device: Option<String>,
    /// The keyboard shortcuts that could be read from the session, for the
    /// collision note the shortcut picker shows.
    known_shortcuts: KnownShortcuts,
    /// A validated imported document waiting for the plan gate. Nothing here
    /// is stored or bound until an approved plan says so.
    pending_import: Option<GestureProfiles>,
    import: Option<ImportSummary>,
    captured: Option<GestureConfig>,
    adapter: Box<dyn SessionAdapter>,
    plan: Option<PresetPlan>,
    resolutions: BTreeMap<GestureId, ConflictResolution>,
    confirmed: bool,
    report: Option<ApplyReport>,
    editor: Option<GestureEditor>,
    live_testing: bool,
    last_test: TestRun,
    problem: Option<String>,
    /// Consecutive runs the adapter failed. The rule and the number live in
    /// `touchpad-gestures` because the resident gesture service applies the
    /// same one, and a rule with two copies is a rule they can disagree about.
    failures: AdapterFailures,
    /// Whether GNOME has been asked to give up the gestures this configuration
    /// took from it, and whether it did.
    suppression: SuppressionState,
}

impl GestureScreen {
    /// A screen with one global profile and no device selected.
    pub fn new(
        config: GestureConfig,
        captured: Option<GestureConfig>,
        adapter: Box<dyn SessionAdapter>,
    ) -> Self {
        Self::with_profiles(
            GestureProfiles::global_only(config),
            None,
            captured,
            adapter,
        )
    }

    pub fn with_profiles(
        profiles: GestureProfiles,
        device: Option<String>,
        captured: Option<GestureConfig>,
        adapter: Box<dyn SessionAdapter>,
    ) -> Self {
        Self {
            profiles,
            device,
            known_shortcuts: KnownShortcuts::default(),
            pending_import: None,
            import: None,
            captured,
            adapter,
            plan: None,
            resolutions: BTreeMap::new(),
            confirmed: false,
            report: None,
            editor: None,
            // Off by default, and the only thing that turns it on is the user.
            live_testing: false,
            last_test: TestRun::default(),
            problem: None,
            failures: AdapterFailures::default(),
            suppression: SuppressionState::new(),
        }
    }

    /// How many failed runs in a row disable the integration.
    pub const FAILURES_BEFORE_DISABLE: u32 = AdapterFailures::BEFORE_DISABLE;

    pub fn consecutive_failures(&self) -> u32 {
        self.failures.consecutive()
    }

    /// Whether GNOME's own gestures are currently suppressed.
    pub fn suppression(&self) -> &SuppressionState {
        &self.suppression
    }

    /// Gives the desktop its own gestures back and leaves them that way.
    ///
    /// Safe mode is the path a user reaches when the machine has become hard to
    /// use, so it undoes rather than asks.
    pub fn enter_safe_mode(&mut self) {
        self.suppression
            .transition(SuppressionEvent::SafeMode, self.adapter.as_mut());
    }

    /// Counts a run and turns the integration off once the adapter has failed
    /// [`Self::FAILURES_BEFORE_DISABLE`] times in a row.
    ///
    /// Disabling stops the recognizer and the bindings. It touches nothing in
    /// `touchpad-core`, so pointer movement and two-finger scrolling are
    /// unaffected by a gesture adapter giving up — which is the whole point of
    /// the two halves keeping separate state.
    fn record_run(&mut self, state: RunState, store: Option<&GestureStore>) {
        if !self.failures.record(state) {
            return;
        }
        if self.config().enabled {
            self.active_mut().enabled = false;
            self.problem = Some(format!(
                "gestures.adapter_disabled_after_failures:{}",
                self.failures.consecutive()
            ));
            self.save(store);
            // An integration that is off must not still be holding GNOME's
            // gestures.
            self.suppression
                .transition(SuppressionEvent::Disabled, self.adapter.as_mut());
        }
    }

    /// The profile in force: this device's own, or the global one.
    pub fn config(&self) -> &GestureConfig {
        self.profiles.resolve(self.device.as_deref())
    }

    /// The profile in force, to be changed. A device that is following the
    /// shared profile is edited *through* it: opening the window, verifying a
    /// binding, or changing a gesture on such a pad must not silently give it a
    /// profile of its own.
    fn active_mut(&mut self) -> &mut GestureConfig {
        let device = self.device.clone();
        self.profiles.resolve_mut(device.as_deref())
    }

    /// Gives the selected device a gesture profile of its own, copied from the
    /// shared one. Nothing else diverges a device.
    pub fn detach_device_profile(&mut self, store: Option<&GestureStore>) {
        let Some(identity) = self.device.clone() else {
            return;
        };
        if self.profiles.has_profile(&identity) {
            return;
        }
        self.profiles.detach(&identity);
        self.cancel_preview();
        self.editor = None;
        self.save(store);
    }

    pub fn profiles(&self) -> &GestureProfiles {
        &self.profiles
    }

    pub fn active_device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// Whether the selected device has diverged from the global profile.
    pub fn device_has_own_profile(&self) -> bool {
        self.device
            .as_deref()
            .is_some_and(|identity| self.profiles.has_profile(identity))
    }

    /// Switches which profile is being edited.
    ///
    /// Any preview, import, or open edit belongs to the profile it was built
    /// against, so switching drops all three rather than letting a plan made
    /// for one device be applied to another.
    pub fn select_device(&mut self, device: Option<String>) {
        if self.device == device {
            return;
        }
        self.device = device;
        self.cancel_preview();
        self.editor = None;
        self.report = None;
    }

    /// Puts the selected device back on the global profile.
    pub fn forget_device_profile(&mut self, store: Option<&GestureStore>) {
        let Some(identity) = self.device.clone() else {
            return;
        };
        if self.profiles.forget(&identity) {
            self.cancel_preview();
            self.editor = None;
            self.save(store);
        }
    }

    /// The document export writes and import replaces.
    pub fn document(&self) -> GestureProfiles {
        let mut document = self.profiles.clone();
        document.active_device = self
            .device
            .clone()
            .filter(|identity| document.has_profile(identity));
        document
    }

    pub fn set_known_shortcuts(&mut self, known: KnownShortcuts) {
        self.known_shortcuts = known;
    }

    pub fn known_shortcuts(&self) -> &KnownShortcuts {
        &self.known_shortcuts
    }

    /// What the recorded keybindings say about the shortcut being edited.
    pub fn shortcut_check(&self) -> Option<ShortcutCheck> {
        let editor = self.editor.as_ref()?;
        if !editor.action_is_shortcut() {
            return None;
        }
        let shortcut = editor.shortcut.build().ok()?;
        Some(self.known_shortcuts.check(&shortcut))
    }

    pub fn import(&self) -> Option<&ImportSummary> {
        self.import.as_ref()
    }

    pub fn captured(&self) -> Option<&GestureConfig> {
        self.captured.as_ref()
    }

    pub fn adapter(&self) -> &dyn SessionAdapter {
        self.adapter.as_ref()
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn set_problem(&mut self, problem: Option<String>) {
        self.problem = problem;
    }

    pub fn report(&self) -> Option<&ApplyReport> {
        self.report.as_ref()
    }

    pub fn plan(&self) -> Option<&PresetPlan> {
        self.plan.as_ref()
    }

    pub fn editor(&self) -> Option<&GestureEditor> {
        self.editor.as_ref()
    }

    pub fn editor_mut(&mut self) -> Option<&mut GestureEditor> {
        self.editor.as_mut()
    }

    pub fn live_testing(&self) -> bool {
        self.live_testing
    }

    pub fn set_live_testing(&mut self, live: bool) {
        self.live_testing = live;
    }

    pub fn last_test(&self) -> &TestRun {
        &self.last_test
    }

    pub fn rows(&self, c: &'static Copy) -> Vec<GestureRow> {
        self.config()
            .gestures
            .iter()
            .map(|gesture| self.row(gesture, c))
            .collect()
    }

    fn row(&self, gesture: &GestureDefinition, c: &'static Copy) -> GestureRow {
        let support = self.adapter.support(&gesture.action);
        let (supported, support_detail) = match &support {
            ActionSupport::Supported { .. } => (true, None),
            ActionSupport::Unsupported { detail, .. } => (false, Some(detail.clone())),
        };
        GestureRow {
            id: gesture.id.clone(),
            label: gesture_label(&gesture.id, c),
            glyph: GestureGlyph::of(gesture),
            contacts: gesture.contacts.get(),
            shape_label: shape_label(gesture.shape, c),
            direction_label: gesture
                .direction
                .map(|direction| direction_label(direction, c)),
            action_label: action_label(&gesture.action, c),
            enabled: gesture.enabled,
            conflict: match &gesture.conflict {
                touchpad_gestures::ConflictState::Conflicts { detail, .. } => {
                    Some(format!("{} · {detail}", c.conflict_badge))
                }
                _ => None,
            },
            supported,
            support_detail,
            verification: match gesture.last_verification {
                VerificationRecord::NotRun => c.verification_not_run,
                VerificationRecord::Verified { .. } => c.verification_verified,
                VerificationRecord::Failed { .. } => c.verification_failed,
                VerificationRecord::Unsupported { .. } => c.verification_unsupported,
            },
            can_animate: gesture.can_animate()
                && support.follows_progress()
                && self.adapter.describe().continuous_progress,
        }
    }

    /// Builds the preview. Nothing is applied by doing this, and a fresh
    /// preview clears the previous confirmation, so confirming one plan can
    /// never apply a different one.
    pub fn preview_preset(&mut self) {
        let plan = PresetPlan::build(
            self.config(),
            &mac_style(),
            touchpad_gestures::GNOME_46_GESTURES,
            self.adapter.as_ref(),
        );
        self.resolutions.clear();
        self.confirmed = false;
        // A preset preview replaces an import preview entirely. Leaving the
        // pending document behind would let confirming the preset install a
        // file the preview never mentioned.
        self.pending_import = None;
        self.import = None;
        self.plan = Some(plan);
    }

    pub fn cancel_preview(&mut self) {
        self.plan = None;
        self.resolutions.clear();
        self.confirmed = false;
        self.pending_import = None;
        self.import = None;
    }

    /// Previews an imported document. Nothing is stored or bound by doing this.
    ///
    /// What is previewed is the profile the document holds for the device
    /// selected here, falling back to the document's global profile — the same
    /// fallback rule a local profile follows, so an import from a machine with
    /// different hardware brings that machine's global profile rather than
    /// nothing.
    pub fn preview_import(&mut self, source: impl Into<String>, document: GestureProfiles) {
        let incoming = document.resolve(self.device.as_deref()).clone();
        let plan = PresetPlan::build(
            self.config(),
            &incoming,
            touchpad_gestures::GNOME_46_GESTURES,
            self.adapter.as_ref(),
        );
        self.resolutions.clear();
        self.confirmed = false;
        self.import = Some(ImportSummary {
            source: source.into(),
            device_profiles: document.identities().map(str::to_string).collect(),
            matches_selected_device: self
                .device
                .as_deref()
                .is_some_and(|identity| document.has_profile(identity)),
        });
        self.pending_import = Some(document);
        self.plan = Some(plan);
    }

    pub fn confirm(&mut self, confirmed: bool) {
        self.confirmed = confirmed;
    }

    pub fn resolve(&mut self, gesture: GestureId, resolution: ConflictResolution) {
        self.resolutions.insert(gesture, resolution);
    }

    pub fn preset_status(&self) -> PresetStatus {
        if self.config().preset != PresetId::MacStyle {
            return PresetStatus::NotApplied;
        }
        let preset = mac_style();
        if touchpad_gestures::plan::differences(self.config(), &preset).is_empty() {
            PresetStatus::Applied
        } else {
            PresetStatus::Differs
        }
    }

    pub fn preset_card(&self, c: &'static Copy) -> PresetCard {
        let status = self.preset_status();
        let status_label = match status {
            PresetStatus::NotApplied => c.preset_not_applied,
            PresetStatus::Applied => c.preset_applied,
            PresetStatus::Differs => c.preset_differs,
        };
        let Some(plan) = &self.plan else {
            return PresetCard {
                status,
                status_label,
                changes: Vec::new(),
                conflicts: Vec::new(),
                unsupported: Vec::new(),
                confirmed: false,
                can_apply: false,
                blocked_reason: None,
            };
        };

        let conflicts: Vec<ConflictRow> = plan
            .conflicts
            .iter()
            .map(|conflict| ConflictRow {
                gesture: conflict.gesture.clone(),
                gesture_label: gesture_label(&conflict.gesture, c),
                built_in: conflict.built_in_does.to_string(),
                resolution: self.resolutions.get(&conflict.gesture).copied(),
            })
            .collect();
        let unresolved = conflicts
            .iter()
            .filter(|conflict| conflict.resolution.is_none())
            .count();
        let blocked_reason = if unresolved > 0 {
            Some(c.unresolved_note)
        } else if !self.confirmed {
            Some(c.confirm_changes)
        } else {
            None
        };

        PresetCard {
            status,
            status_label,
            changes: plan
                .changes
                .iter()
                .map(|change| {
                    format!(
                        "{} · {} · {}",
                        gesture_label(&change.gesture, c),
                        change_label(change.kind, c),
                        change
                            .after
                            .clone()
                            .or_else(|| change.before.clone())
                            .unwrap_or_default()
                    )
                })
                .collect(),
            conflicts,
            unsupported: plan
                .unsupported
                .iter()
                .map(|(gesture, support)| {
                    format!(
                        "{} · {}",
                        gesture_label(gesture, c),
                        support.detail().unwrap_or(c.unsupported_badge)
                    )
                })
                .collect(),
            confirmed: self.confirmed,
            can_apply: unresolved == 0 && self.confirmed && !plan.is_empty(),
            blocked_reason,
        }
    }

    /// Applies whatever was previewed — the shipped preset or an imported
    /// document — through the one gate: capture first, bind, verify, then store.
    ///
    /// There is deliberately no second apply path for an import. An imported
    /// document reaches a binding the same way the preset does, so "nothing is
    /// applied without preview and confirmation" holds for a file from another
    /// machine exactly as it holds for the preset.
    ///
    /// The capture is written before the first binding and is never replaced,
    /// which is what makes restore mean something here for the same reason it
    /// does for pointer and scrolling settings.
    pub fn apply_plan(&mut self, store: Option<&GestureStore>) -> Result<RunState, PlanError> {
        let plan = self.plan.take().ok_or(PlanError::NothingToDo)?;
        let approved = match plan.approve(&self.resolutions, self.confirmed) {
            Ok(approved) => approved,
            Err(error) => {
                // Put the preview back: a refused approval must not lose the
                // preview the user is looking at.
                self.plan = Some(plan);
                return Err(error);
            }
        };
        if self.captured.is_none() {
            self.captured = Some(approved.previous().clone());
        }
        if let Some(store) = store {
            if let Err(error) = store.capture_once(approved.previous()) {
                self.problem = Some(error.to_string());
            }
        }

        let (config, report) = approved.apply_with(self.adapter.as_mut(), &mut self.suppression);
        // An approved import brings the whole document with it: the profile
        // that was previewed is the one just bound, and the rest of the file's
        // profiles land beside it. The file's own device selection is not
        // adopted — it names a machine that is not this one.
        if let Some(document) = self.pending_import.take() {
            self.profiles = document;
        }
        *self.active_mut() = config;
        self.save(store);
        let state = report.state();
        self.report = Some(report);
        self.confirmed = false;
        self.resolutions.clear();
        self.import = None;
        self.record_run(state, store);
        Ok(state)
    }

    /// Puts the captured gesture configuration back.
    pub fn restore(&mut self, store: Option<&GestureStore>) -> Option<RunState> {
        let captured = self.captured.clone()?;
        let plan = RestorePlan::from_capture(self.config(), &captured);
        let (config, report) = plan.apply_with(self.adapter.as_mut(), &mut self.suppression);
        *self.active_mut() = config;
        self.save(store);
        let state = report.state();
        self.report = Some(report);
        self.record_run(state, store);
        Some(state)
    }

    /// Turns gestures off, restoring the captured configuration on the way.
    ///
    /// With no capture there is nothing to go back to, so the current
    /// configuration is kept and only the master switch moves — which is still
    /// the honest thing: nothing was ever changed, so nothing is put back.
    pub fn disable(&mut self, store: Option<&GestureStore>) -> RunState {
        let captured = self
            .captured
            .clone()
            .unwrap_or_else(|| self.config().clone());
        let plan = RestorePlan::disable(self.config(), &captured);
        let (config, report) = plan.apply_with(self.adapter.as_mut(), &mut self.suppression);
        *self.active_mut() = config;
        self.save(store);
        let state = report.state();
        self.report = Some(report);
        state
    }

    /// Binds and verifies what is configured now, without changing it. This is
    /// what the screen does on the way in, so a row's verification result is
    /// this session's rather than the last one's.
    pub fn verify_all(&mut self) -> RunState {
        let (config, report) = bind_all(&self.config().clone(), self.adapter.as_mut());
        *self.active_mut() = config;
        let state = report.state();
        self.report = Some(report);
        self.record_run(state, None);
        state
    }

    fn save(&mut self, store: Option<&GestureStore>) {
        if let Some(store) = store {
            if let Err(error) = store.save_profiles(&self.document()) {
                self.problem = Some(error.to_string());
            }
        }
    }

    pub fn edit(&mut self, id: &GestureId) {
        self.editor = self.config().get(id).map(GestureEditor::of);
    }

    pub fn cancel_edit(&mut self) {
        self.editor = None;
    }

    /// Saves the edit, or leaves the editor open carrying the reason it was
    /// refused. Nothing partial is ever written.
    pub fn commit_edit(&mut self, store: Option<&GestureStore>) -> Result<(), GestureError> {
        let Some(editor) = self.editor.clone() else {
            return Ok(());
        };
        let original = self
            .config()
            .get(&editor.id)
            .cloned()
            .ok_or_else(|| GestureError::UnknownId(editor.id.to_string()))?;
        let result = editor
            .build(&original)
            .and_then(|gesture| self.active_mut().replace(gesture));
        match result {
            Ok(()) => {
                // An edited configuration is no longer the shipped preset, and
                // saying otherwise would make the preset card claim something
                // false.
                if self.config().preset == PresetId::MacStyle
                    && !touchpad_gestures::plan::differences(self.config(), &mac_style()).is_empty()
                {
                    // The status is derived, not stored, so nothing to do here
                    // beyond saving; `preset_status` will report `Differs`.
                }
                self.editor = None;
                self.save(store);
                Ok(())
            }
            Err(error) => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.error = Some(error.to_string());
                }
                Err(error)
            }
        }
    }

    pub fn set_enabled(&mut self, id: &GestureId, enabled: bool, store: Option<&GestureStore>) {
        if let Some(gesture) = self.active_mut().get_mut(id) {
            gesture.enabled = enabled;
        }
        self.save(store);
    }

    /// Replays one gesture and shows what the recognizer saw.
    ///
    /// With live testing off — the default — the events are shown and no action
    /// is performed. With it on, each event is handed to the session adapter,
    /// which in this build is still the recording one.
    pub fn test_gesture(&mut self, id: &GestureId, c: &'static Copy) -> TestRun {
        self.test_gesture_with(id, 1.0, c)
    }

    /// The same, carried only `completion` of the way, so a cancelled gesture
    /// can be shown as well as a completed one.
    pub fn test_gesture_with(
        &mut self,
        id: &GestureId,
        completion: f32,
        c: &'static Copy,
    ) -> TestRun {
        let Some(gesture) = self.config().get(id).cloned() else {
            self.last_test = TestRun::default();
            return self.last_test.clone();
        };
        let scale = RecognizerScale::default();
        let frames = synthetic::lift(synthetic::perform(&gesture, completion, scale));
        // A recognizer built for this run only: testing one gesture must not
        // leave a cooldown behind that changes the next test.
        let mut recognizer = Recognizer::with_scale(self.config().active(), scale);
        let events = recognizer.replay(&frames);

        let mut performed = 0;
        if self.live_testing {
            for event in &events {
                let action = self
                    .config()
                    .get(&event.gesture)
                    .map(|gesture| gesture.action.clone())
                    .unwrap_or(DesktopAction::Disabled);
                if self
                    .adapter
                    .invoke(&action, event.progress_report())
                    .is_success()
                {
                    performed += 1;
                }
            }
        }

        self.last_test = TestRun {
            lines: events
                .iter()
                .map(|event| test_line(event, c, self))
                .collect(),
            performed,
        };
        self.last_test.clone()
    }

    /// The recognized-event log for the Diagnostics screen.
    pub fn diagnostics_lines(&self, c: &'static Copy) -> Vec<String> {
        let mut lines: Vec<String> = self
            .last_test
            .lines
            .iter()
            .map(|line| {
                format!(
                    "{} · {} · {:.0}%",
                    line.gesture,
                    line.kind,
                    line.progress * 100.0
                )
            })
            .collect();
        if let Some(report) = &self.report {
            for (gesture, outcome) in &report.results {
                lines.push(format!(
                    "{} · {}",
                    gesture_label(gesture, c),
                    outcome_label(outcome, c)
                ));
            }
            for (built_in, _) in &report.built_ins {
                lines.push(format!("{built_in} · {}", c.unsupported_badge));
            }
        }
        lines
    }
}

fn test_line(event: &GestureEvent, c: &'static Copy, screen: &GestureScreen) -> TestLine {
    let _ = screen;
    TestLine {
        gesture: gesture_label(&event.gesture, c),
        kind: event.kind.key(),
        progress: event.progress,
    }
}

fn change_label(kind: ChangeKind, c: &'static Copy) -> &'static str {
    match kind {
        ChangeKind::Added => c.preset_not_applied,
        ChangeKind::Removed => c.disable_gestures,
        ChangeKind::Changed => c.edit_gesture,
    }
}

pub fn outcome_label(outcome: &BindingOutcome, c: &'static Copy) -> String {
    match outcome {
        BindingOutcome::Verified { .. } => c.verification_verified.to_string(),
        BindingOutcome::Unverified { detail, .. } | BindingOutcome::Failed { detail, .. } => {
            format!("{} · {detail}", c.verification_failed)
        }
        BindingOutcome::Unsupported { detail, .. } => {
            format!("{} · {detail}", c.verification_unsupported)
        }
        BindingOutcome::Skipped { reason } => format!("{} · {reason}", c.verification_not_run),
    }
}

/// The name of a shipped gesture. A gesture the user built carries its own
/// identity, which is the only honest name for it.
pub fn gesture_label(id: &GestureId, c: &'static Copy) -> String {
    match id.as_str() {
        "launcher" => c.gesture_launcher.to_string(),
        "show-desktop" => c.gesture_show_desktop.to_string(),
        "overview" => c.gesture_overview.to_string(),
        "current-app-windows" => c.gesture_current_app_windows.to_string(),
        "workspace-next" => c.gesture_workspace_next.to_string(),
        "workspace-previous" => c.gesture_workspace_previous.to_string(),
        "app-back" => c.gesture_app_back.to_string(),
        "app-forward" => c.gesture_app_forward.to_string(),
        "app-zoom" => c.gesture_app_zoom.to_string(),
        "app-rotate" => c.gesture_app_rotate.to_string(),
        other => other.to_string(),
    }
}

pub fn action_label(action: &DesktopAction, c: &'static Copy) -> String {
    match action {
        DesktopAction::LauncherOpen => c.action_launcher_open.to_string(),
        DesktopAction::LauncherClose => c.action_launcher_close.to_string(),
        DesktopAction::ShowDesktop => c.action_show_desktop.to_string(),
        DesktopAction::ShowOverview => c.action_overview.to_string(),
        DesktopAction::CurrentApplicationWindows => c.action_current_app_windows.to_string(),
        DesktopAction::NextWorkspace => c.action_workspace_next.to_string(),
        DesktopAction::PreviousWorkspace => c.action_workspace_previous.to_string(),
        DesktopAction::NextApplication => c.action_app_next.to_string(),
        DesktopAction::PreviousApplication => c.action_app_previous.to_string(),
        DesktopAction::ApplicationBack => c.action_app_back.to_string(),
        DesktopAction::ApplicationForward => c.action_app_forward.to_string(),
        DesktopAction::ApplicationZoom => c.action_app_zoom.to_string(),
        DesktopAction::ApplicationRotate => c.action_app_rotate.to_string(),
        DesktopAction::MediaPlayPause => c.action_media_play_pause.to_string(),
        DesktopAction::VolumeUp => c.action_volume_up.to_string(),
        DesktopAction::VolumeDown => c.action_volume_down.to_string(),
        DesktopAction::VolumeMute => c.action_volume_mute.to_string(),
        // The keys themselves are the user's own text, and showing them beside
        // the label is the only way the row says which shortcut it is.
        DesktopAction::KeyboardShortcut { shortcut } => {
            format!("{} · {}", c.action_shortcut, shortcut.to_gnome())
        }
        DesktopAction::Disabled => c.action_none.to_string(),
    }
}

pub fn shape_label(shape: GestureShape, c: &'static Copy) -> &'static str {
    match shape {
        GestureShape::Swipe => c.shape_swipe,
        GestureShape::Pinch => c.shape_pinch,
        GestureShape::Spread => c.shape_spread,
        GestureShape::Hold => c.shape_hold,
        GestureShape::Tap => c.shape_tap,
        GestureShape::Rotate => c.shape_rotate,
    }
}

pub fn direction_label(direction: Direction, c: &'static Copy) -> &'static str {
    match direction {
        Direction::Up => c.direction_up,
        Direction::Down => c.direction_down,
        Direction::Left => c.direction_left,
        Direction::Right => c.direction_right,
        Direction::Clockwise => c.direction_clockwise,
        Direction::CounterClockwise => c.direction_counter_clockwise,
    }
}

pub fn resolution_label(resolution: ConflictResolution, c: &'static Copy) -> &'static str {
    match resolution {
        ConflictResolution::KeepBuiltIn => c.resolution_keep_built_in,
        ConflictResolution::DisableBuiltIn => c.resolution_disable_built_in,
        ConflictResolution::RemapOurs { .. } => c.resolution_remap,
    }
}

/// The three choices a conflict row offers.
///
/// The remap option moves the gesture to five contacts, which is out of the
/// reach of every GNOME 46 swipe tracker, so it is the one choice that keeps
/// both gestures working.
pub fn resolution_choices(direction: Option<Direction>) -> Vec<ConflictResolution> {
    let mut choices = vec![
        ConflictResolution::KeepBuiltIn,
        ConflictResolution::DisableBuiltIn,
    ];
    if let Some(direction) = direction {
        choices.push(ConflictResolution::RemapOurs {
            contacts: 5,
            direction,
        });
    }
    choices
}
