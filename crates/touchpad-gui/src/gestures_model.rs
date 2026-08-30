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

use std::collections::BTreeMap;

use better_actions::{ActionSupport, DesktopAction};
use touchpad_gestures::{
    ApplyReport, BindingOutcome, ChangeKind, ConflictResolution, ContactCount, Cooldown, Direction,
    GestureConfig, GestureDefinition, GestureError, GestureEvent, GestureId, GestureShape,
    GestureStore, PlanError, PresetId, PresetPlan, Recognizer, RecognizerScale, RestorePlan,
    RunState, Threshold, VerificationRecord, mac_style, plan::bind_all, synthetic,
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

/// The edit view for one gesture.
#[derive(Clone, Debug, PartialEq)]
pub struct GestureEditor {
    pub id: GestureId,
    pub shape: GestureShape,
    pub contacts: u8,
    pub thumb_required: bool,
    pub direction: Option<Direction>,
    pub action: DesktopAction,
    pub activation: f32,
    pub cancellation: f32,
    pub cooldown_ms: u64,
    pub enabled: bool,
    pub error: Option<String>,
}

impl GestureEditor {
    pub fn of(gesture: &GestureDefinition) -> Self {
        Self {
            id: gesture.id.clone(),
            shape: gesture.shape,
            contacts: gesture.contacts.get(),
            thumb_required: gesture.thumb_required,
            direction: gesture.direction,
            action: gesture.action.clone(),
            activation: gesture.activation_threshold.get(),
            cancellation: gesture.cancellation_threshold.get(),
            cooldown_ms: gesture.cooldown.as_millis(),
            enabled: gesture.enabled,
            error: None,
        }
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
        gesture.action = self.action.clone();
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

/// The Gestures screen's whole state.
pub struct GestureScreen {
    config: GestureConfig,
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
    /// Consecutive runs the adapter failed. Issue #3 requires a repeatedly
    /// failing gesture adapter to be disabled automatically, and "repeatedly"
    /// has to be a number somebody chose rather than a feeling.
    consecutive_failures: u32,
}

impl GestureScreen {
    pub fn new(
        config: GestureConfig,
        captured: Option<GestureConfig>,
        adapter: Box<dyn SessionAdapter>,
    ) -> Self {
        Self {
            config,
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
            consecutive_failures: 0,
        }
    }

    /// How many failed runs in a row disable the integration.
    ///
    /// Three rather than one: a single failure can be a session that was still
    /// starting, and turning every gesture off for that would be worse than the
    /// failure. Three in a row is a broken adapter.
    pub const FAILURES_BEFORE_DISABLE: u32 = 3;

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Counts a run and turns the integration off once the adapter has failed
    /// [`Self::FAILURES_BEFORE_DISABLE`] times in a row.
    ///
    /// Disabling stops the recognizer and the bindings. It touches nothing in
    /// `touchpad-core`, so pointer movement and two-finger scrolling are
    /// unaffected by a gesture adapter giving up — which is the whole point of
    /// the two halves keeping separate state.
    fn record_run(&mut self, state: RunState, store: Option<&GestureStore>) {
        if state == RunState::Failed {
            self.consecutive_failures += 1;
        } else {
            self.consecutive_failures = 0;
            return;
        }
        if self.consecutive_failures >= Self::FAILURES_BEFORE_DISABLE && self.config.enabled {
            self.config.enabled = false;
            self.problem = Some(format!(
                "gestures.adapter_disabled_after_failures:{}",
                self.consecutive_failures
            ));
            self.save(store);
        }
    }

    pub fn config(&self) -> &GestureConfig {
        &self.config
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
        self.config
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
            &self.config,
            &mac_style(),
            touchpad_gestures::GNOME_46_GESTURES,
            self.adapter.as_ref(),
        );
        self.resolutions.clear();
        self.confirmed = false;
        self.plan = Some(plan);
    }

    pub fn cancel_preview(&mut self) {
        self.plan = None;
        self.resolutions.clear();
        self.confirmed = false;
    }

    pub fn confirm(&mut self, confirmed: bool) {
        self.confirmed = confirmed;
    }

    pub fn resolve(&mut self, gesture: GestureId, resolution: ConflictResolution) {
        self.resolutions.insert(gesture, resolution);
    }

    pub fn preset_status(&self) -> PresetStatus {
        if self.config.preset != PresetId::MacStyle {
            return PresetStatus::NotApplied;
        }
        let preset = mac_style();
        if touchpad_gestures::plan::differences(&self.config, &preset).is_empty() {
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

    /// Applies the previewed preset: capture first, bind, verify, then store.
    ///
    /// The capture is written before the first binding and is never replaced,
    /// which is what makes restore mean something here for the same reason it
    /// does for pointer and scrolling settings.
    pub fn apply_preset(&mut self, store: Option<&GestureStore>) -> Result<RunState, PlanError> {
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

        let (config, report) = approved.apply(self.adapter.as_mut());
        self.config = config;
        self.save(store);
        let state = report.state();
        self.report = Some(report);
        self.confirmed = false;
        self.resolutions.clear();
        self.record_run(state, store);
        Ok(state)
    }

    /// Puts the captured gesture configuration back.
    pub fn restore(&mut self, store: Option<&GestureStore>) -> Option<RunState> {
        let captured = self.captured.clone()?;
        let plan = RestorePlan::from_capture(&self.config, &captured);
        let (config, report) = plan.apply(self.adapter.as_mut());
        self.config = config;
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
        let captured = self.captured.clone().unwrap_or_else(|| self.config.clone());
        let plan = RestorePlan::disable(&self.config, &captured);
        let (config, report) = plan.apply(self.adapter.as_mut());
        self.config = config;
        self.save(store);
        let state = report.state();
        self.report = Some(report);
        state
    }

    /// Binds and verifies what is configured now, without changing it. This is
    /// what the screen does on the way in, so a row's verification result is
    /// this session's rather than the last one's.
    pub fn verify_all(&mut self) -> RunState {
        let (config, report) = bind_all(&self.config, self.adapter.as_mut());
        self.config = config;
        let state = report.state();
        self.report = Some(report);
        self.record_run(state, None);
        state
    }

    fn save(&mut self, store: Option<&GestureStore>) {
        if let Some(store) = store {
            if let Err(error) = store.save_config(&self.config) {
                self.problem = Some(error.to_string());
            }
        }
    }

    pub fn edit(&mut self, id: &GestureId) {
        self.editor = self.config.get(id).map(GestureEditor::of);
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
            .config
            .get(&editor.id)
            .cloned()
            .ok_or_else(|| GestureError::UnknownId(editor.id.to_string()))?;
        let result = editor
            .build(&original)
            .and_then(|gesture| self.config.replace(gesture));
        match result {
            Ok(()) => {
                // An edited configuration is no longer the shipped preset, and
                // saying otherwise would make the preset card claim something
                // false.
                if self.config.preset == PresetId::MacStyle
                    && !touchpad_gestures::plan::differences(&self.config, &mac_style()).is_empty()
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
        if let Some(gesture) = self.config.get_mut(id) {
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
        let Some(gesture) = self.config.get(id).cloned() else {
            self.last_test = TestRun::default();
            return self.last_test.clone();
        };
        let scale = RecognizerScale::default();
        let frames = synthetic::lift(synthetic::perform(&gesture, completion, scale));
        // A recognizer built for this run only: testing one gesture must not
        // leave a cooldown behind that changes the next test.
        let mut recognizer = Recognizer::with_scale(self.config.active(), scale);
        let events = recognizer.replay(&frames);

        let mut performed = 0;
        if self.live_testing {
            for event in &events {
                let action = self
                    .config
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
