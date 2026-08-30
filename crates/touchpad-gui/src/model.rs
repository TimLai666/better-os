//! Everything the screens decide, with no GPUI anywhere in it.
//!
//! This is the same split `monitor-views` and `manager-gui`'s Defaults screens
//! use: a view decision is not a rendering decision. Which controls exist, what
//! each one says, whether it is available, what applying it did, and what a
//! restore would put back are all decided here and asserted without a window.
//!
//! The model never talks to the desktop itself. It is handed a
//! [`TouchpadBackend`] for the moments it needs one — reading, applying,
//! restoring — and those are the only three moments. There is no command, no
//! key name, and no dconf path in this file.

use touchpad_core::{
    Backup, Capabilities, HealthFacts, HealthReport, HealthState, Reading, RestoreScope,
    RestoreStep, RunReport, RunState, Section, SettingId, SettingState, SettingValue, Support,
    TouchpadConfig, TouchpadState, TouchpadStore, ValueError, ValueKind,
};
use touchpad_platform::{
    BackendStatus, DeviceInventory, DeviceState, Session, TouchpadBackend, TouchpadDevice,
};

use crate::i18n::{Copy, Locale, copy};

/// Which screen is showing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Page {
    Overview,
    Pointer,
    Scrolling,
    Clicking,
    Gestures,
    Devices,
    Diagnostics,
}

impl Page {
    pub const ALL: [Self; 7] = [
        Self::Overview,
        Self::Pointer,
        Self::Scrolling,
        Self::Clicking,
        Self::Gestures,
        Self::Devices,
        Self::Diagnostics,
    ];

    pub fn section(self) -> Option<Section> {
        match self {
            Self::Pointer => Some(Section::Pointer),
            Self::Scrolling => Some(Section::Scrolling),
            Self::Clicking => Some(Section::Clicking),
            _ => None,
        }
    }

    /// The screen a `--page` argument names. Anything else is the overview,
    /// because an unrecognized screen name is not worth refusing to start over.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "overview" => Self::Overview,
            "pointer" => Self::Pointer,
            "scrolling" => Self::Scrolling,
            "clicking" => Self::Clicking,
            "gestures" => Self::Gestures,
            "devices" => Self::Devices,
            "diagnostics" => Self::Diagnostics,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Pointer => "pointer",
            Self::Scrolling => "scrolling",
            Self::Clicking => "clicking",
            Self::Gestures => "gestures",
            Self::Devices => "devices",
            Self::Diagnostics => "diagnostics",
        }
    }

    pub fn label(self, c: &'static Copy) -> &'static str {
        match self {
            Self::Overview => c.nav_overview,
            Self::Pointer => c.nav_pointer,
            Self::Scrolling => c.nav_scrolling,
            Self::Clicking => c.nav_clicking,
            Self::Gestures => c.nav_gestures,
            Self::Devices => c.nav_devices,
            Self::Diagnostics => c.nav_diagnostics,
        }
    }
}

/// What kind of control a row draws as. The GUI switches on this rather than on
/// the setting, so a new setting of an existing shape needs no new rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Slider { min: f64, max: f64, step: f64 },
    Switch,
    Choice,
}

/// One control, fully decided.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingRow {
    pub setting: SettingId,
    pub label: &'static str,
    pub control: Control,
    /// `false` renders the explanation instead of the control. There is no
    /// third state, and an unavailable row is never an inert switch.
    pub available: bool,
    pub unavailable_detail: Option<String>,
    pub requested: SettingValue,
    pub requested_label: String,
    pub effective_label: String,
    pub previous_label: Option<String>,
    pub pending: bool,
    pub drifted: bool,
    pub needs_sign_out: bool,
    /// The result of the last run, if this row was in it.
    pub result: Option<String>,
    pub result_state: Option<RunState>,
}

/// What the Overview screen shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    pub device: String,
    pub device_state: Option<DeviceState>,
    pub session: String,
    pub backend: String,
    pub backend_reachable: bool,
    pub health: HealthState,
    pub health_lines: Vec<String>,
    pub pointer_summary: String,
    pub scroll_summary: String,
    pub pending_count: usize,
    pub awaiting_sign_out: Vec<SettingId>,
    pub unavailable_count: usize,
}

/// One line of the restore review: what was captured, and what putting it back
/// would do.
#[derive(Clone, Debug, PartialEq)]
pub struct RestoreRow {
    pub setting: SettingId,
    pub label: &'static str,
    pub captured_label: String,
    pub actionable: bool,
    pub detail: Option<String>,
}

/// Where the pointer is inside the test surface, as a fraction of it.
///
/// The fraction is what makes the surface testable: it is the same number at
/// every window size and every scaling factor, so the assertion is about the
/// mapping rather than about pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerTrace {
    pub x: f32,
    pub y: f32,
    pub inside: bool,
}

impl PointerTrace {
    pub fn idle() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            inside: false,
        }
    }

    /// Maps a position within a surface onto `0.0..=1.0` on each axis.
    ///
    /// A position outside the surface is reported as outside and clamped, so
    /// the drawn marker never leaves the box it belongs to.
    pub fn at(x: f32, y: f32, width: f32, height: f32) -> Self {
        if width <= 0.0 || height <= 0.0 {
            return Self::idle();
        }
        let fraction_x = x / width;
        let fraction_y = y / height;
        Self {
            x: fraction_x.clamp(0.0, 1.0),
            y: fraction_y.clamp(0.0, 1.0),
            inside: (0.0..=1.0).contains(&fraction_x) && (0.0..=1.0).contains(&fraction_y),
        }
    }
}

/// What the last apply or restore was.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunKind {
    Apply,
    Restore,
}

pub struct TouchpadModel {
    state: TouchpadState,
    session: Session,
    backend_status: BackendStatus,
    backend_name: String,
    inventory: DeviceInventory,
    selected: Option<TouchpadDevice>,
    locale: Locale,
    last_run: Option<(RunKind, RunReport)>,
    configuration_problem: Option<String>,
    safe_mode: bool,
}

impl TouchpadModel {
    pub fn new(
        config: TouchpadConfig,
        capabilities: Capabilities,
        session: Session,
        backend_status: BackendStatus,
        backend_name: impl Into<String>,
        inventory: DeviceInventory,
        locale: Locale,
    ) -> Self {
        let selected = inventory
            .select(match &config.selected_device {
                touchpad_core::DeviceSelection::Auto => None,
                touchpad_core::DeviceSelection::Device { identity } => Some(identity.as_str()),
            })
            .cloned();
        Self {
            state: TouchpadState::new(config, capabilities),
            session,
            backend_status,
            backend_name: backend_name.into(),
            inventory,
            selected,
            locale,
            last_run: None,
            configuration_problem: None,
            safe_mode: false,
        }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    pub fn copy(&self) -> &'static Copy {
        copy(self.locale)
    }

    pub fn state(&self) -> &TouchpadState {
        &self.state
    }

    pub fn config(&self) -> &TouchpadConfig {
        self.state.config()
    }

    pub fn devices(&self) -> &[TouchpadDevice] {
        &self.inventory.devices
    }

    pub fn selected_device(&self) -> Option<&TouchpadDevice> {
        self.selected.as_ref()
    }

    pub fn backend_status(&self) -> &BackendStatus {
        &self.backend_status
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    pub fn set_safe_mode(&mut self, on: bool) {
        self.safe_mode = on;
        self.state.set_safe_mode(on);
    }

    pub fn set_configuration_problem(&mut self, problem: Option<String>) {
        self.configuration_problem = problem;
    }

    pub fn adopt_backup(&mut self, backup: Backup) {
        self.state.adopt_backup(backup);
    }

    pub fn has_pending(&self) -> bool {
        self.state.has_pending()
    }

    pub fn discard(&mut self) {
        self.state.discard_pending();
    }

    pub fn stage(&mut self, setting: SettingId, value: SettingValue) -> Result<(), ValueError> {
        self.state.stage(setting, value)
    }

    pub fn stage_linked_axes(&mut self, linked: bool) {
        self.state.stage_linked_axes(linked);
    }

    /// Reads every effective value again.
    pub fn refresh(&mut self, backend: &dyn TouchpadBackend) {
        self.state.set_capabilities(backend.capabilities().clone());
        self.state.adopt_readings(backend.read_all());
    }

    /// Applies what is staged: capture first, write, verify, then record.
    ///
    /// The capture happens before the write and is never replaced, which is the
    /// property that makes restore mean something.
    pub fn apply(
        &mut self,
        backend: &mut dyn TouchpadBackend,
        store: Option<&TouchpadStore>,
        now: u64,
    ) -> RunState {
        let plan = self.state.apply_plan();
        if !plan.is_empty() {
            self.state.capture_before(&plan, backend.name(), now);
            if let (Some(store), Some(backup)) = (store, self.state.backup()) {
                // A capture that could not be written is worth knowing about
                // before anything changes, but it does not stop the change:
                // the in-memory capture is still what restore uses this run.
                if let Err(error) = store.extend_capture(backup) {
                    self.configuration_problem = Some(error.to_string());
                }
            }
        }
        let report = backend.apply(&plan);
        self.state.record(&report);
        if let Some(store) = store {
            if let Err(error) = store.save_config(self.state.config()) {
                self.configuration_problem = Some(error.to_string());
            }
        }
        let outcome = report.state();
        self.last_run = Some((RunKind::Apply, report));
        outcome
    }

    /// Puts the captured state back, for one section or all of it.
    pub fn restore(
        &mut self,
        backend: &mut dyn TouchpadBackend,
        scope: RestoreScope,
        store: Option<&TouchpadStore>,
    ) -> Option<RunState> {
        let plan = self.state.restore_plan(scope)?;
        let report = backend.restore(&plan);
        self.state.record_restore(&report);
        if let Some(store) = store {
            if let Err(error) = store.save_config(self.state.config()) {
                self.configuration_problem = Some(error.to_string());
            }
        }
        let outcome = report.state();
        self.last_run = Some((RunKind::Restore, report));
        Some(outcome)
    }

    pub fn last_run(&self) -> Option<(RunKind, &RunReport)> {
        self.last_run.as_ref().map(|(kind, report)| (*kind, report))
    }

    /// The one-line result banner, or nothing when nothing has run.
    pub fn result_banner(&self) -> Option<(RunState, &'static str)> {
        let c = self.copy();
        let (kind, report) = self.last_run.as_ref()?;
        let state = report.state();
        let text = match (kind, state) {
            (_, RunState::NothingToDo) => c.result_nothing,
            (RunKind::Restore, RunState::Applied) => c.restored,
            (_, RunState::Applied) => c.result_applied,
            (_, RunState::AwaitingSignOut) => c.result_awaiting_sign_out,
            (_, RunState::PartiallySupported) => c.result_partial,
            (_, RunState::Failed) => c.result_failed,
        };
        Some((state, text))
    }

    pub fn rows(&self, section: Section) -> Vec<SettingRow> {
        self.state
            .section_states(section)
            .into_iter()
            .map(|state| self.row(state))
            .collect()
    }

    pub fn all_rows(&self) -> Vec<SettingRow> {
        self.state
            .states()
            .into_iter()
            .map(|state| self.row(state))
            .collect()
    }

    fn row(&self, state: SettingState) -> SettingRow {
        let c = self.copy();
        let (available, unavailable_detail, needs_sign_out) = match &state.support {
            Support::Full { effect } => (
                true,
                None,
                *effect == touchpad_core::SessionEffect::SignOutRequired,
            ),
            Support::Unavailable { detail, .. } => (false, Some(detail.clone()), false),
        };
        let requested = state.requested();
        let result = self.last_run.as_ref().and_then(|(_, report)| {
            report
                .outcome(state.setting)
                .map(|outcome| describe_outcome(outcome, c))
        });
        let result_state = result
            .as_ref()
            .and(self.last_run.as_ref().map(|(_, report)| report.state()));

        SettingRow {
            setting: state.setting,
            label: label_for(state.setting, c),
            control: control_for(state.setting),
            available,
            unavailable_detail,
            requested,
            requested_label: describe_value(requested, c),
            effective_label: describe_reading(&state.effective, c),
            previous_label: state
                .previous
                .as_ref()
                .map(|reading| describe_reading(reading, c)),
            pending: state.is_pending(),
            drifted: state.drifted(),
            needs_sign_out,
            result,
            result_state,
        }
    }

    pub fn overview(&self) -> Overview {
        let c = self.copy();
        let health = self.health();
        Overview {
            device: match &self.selected {
                Some(device) => device.describe(),
                None => c.no_devices.to_string(),
            },
            device_state: self.selected.as_ref().map(|device| device.state),
            session: self.session.describe(),
            backend: self.backend_name.clone(),
            backend_reachable: self.backend_status.reachable,
            health: health.state(),
            health_lines: health
                .checks
                .iter()
                .map(|check| format!("{}: {}", check.id, check.detail))
                .collect(),
            pointer_summary: describe_reading(
                &self.state.state(SettingId::PointerSensitivity).effective,
                c,
            ),
            scroll_summary: describe_reading(
                &self.state.state(SettingId::VerticalScrollFactor).effective,
                c,
            ),
            pending_count: self
                .state
                .states()
                .iter()
                .filter(|state| state.is_pending())
                .count(),
            awaiting_sign_out: self
                .state
                .states()
                .iter()
                .filter(|state| {
                    state.support.effect() == Some(touchpad_core::SessionEffect::SignOutRequired)
                })
                .map(|state| state.setting)
                .collect(),
            unavailable_count: self.state.capabilities().unavailable().len(),
        }
    }

    pub fn health(&self) -> HealthReport {
        let capabilities = self.state.capabilities().clone();
        HealthReport::evaluate(&HealthFacts {
            configuration_readable: self.configuration_problem.is_none(),
            configuration_detail: self
                .configuration_problem
                .clone()
                .unwrap_or_else(|| "the configuration was read".to_string()),
            backend_name: &self.backend_name,
            backend_reachable: self.backend_status.reachable,
            backend_detail: self.backend_status.detail.clone(),
            devices_found: self.inventory.devices.len(),
            selected_device: self
                .selected
                .as_ref()
                .map(|device| device.identity.as_str()),
            capabilities: &capabilities,
            capture_present: self.state.backup().is_some(),
            safe_mode: self.safe_mode,
            integration_enabled: self.state.config().enabled,
        })
    }

    /// What a restore of this scope would put back, shown before it runs.
    pub fn restore_rows(&self, scope: RestoreScope) -> Vec<RestoreRow> {
        let c = self.copy();
        let Some(plan) = self.state.restore_plan(scope) else {
            return Vec::new();
        };
        plan.steps
            .iter()
            .map(|step| {
                let setting = step.setting();
                let captured = self
                    .state
                    .backup()
                    .and_then(|backup| backup.reading(setting))
                    .cloned()
                    .unwrap_or_else(|| Reading::unknown("touchpad.not_captured"));
                RestoreRow {
                    setting,
                    label: label_for(setting, c),
                    captured_label: describe_reading(&captured, c),
                    actionable: step.is_actionable(),
                    detail: match step {
                        RestoreStep::Impossible { detail, .. } => Some(detail.clone()),
                        _ => None,
                    },
                }
            })
            .collect()
    }
}

pub fn control_for(setting: SettingId) -> Control {
    match setting.kind() {
        ValueKind::Sensitivity => Control::Slider {
            min: touchpad_core::Sensitivity::MIN,
            max: touchpad_core::Sensitivity::MAX,
            step: 0.05,
        },
        ValueKind::Factor => Control::Slider {
            min: touchpad_core::ScrollFactor::MIN,
            max: touchpad_core::ScrollFactor::MAX,
            step: 0.1,
        },
        ValueKind::Toggle => Control::Switch,
        ValueKind::Acceleration | ValueKind::Click => Control::Choice,
    }
}

pub fn label_for(setting: SettingId, c: &'static Copy) -> &'static str {
    match setting {
        SettingId::PointerSensitivity => c.pointer_sensitivity,
        SettingId::AccelerationProfile => c.acceleration_profile,
        SettingId::DisableWhileTyping => c.disable_while_typing,
        SettingId::VerticalScrollFactor => c.vertical_axis,
        SettingId::HorizontalScrollFactor => c.horizontal_axis,
        SettingId::NaturalScrolling => c.natural_scrolling,
        SettingId::TwoFingerScrolling => c.two_finger_scrolling,
        SettingId::SmoothScrolling => c.smooth_scrolling,
        SettingId::TapToClick => c.tap_to_click,
        SettingId::TapAndDrag => c.tap_and_drag,
        SettingId::DragLock => c.drag_lock,
        SettingId::ClickMethod => c.click_method,
        SettingId::MiddleClickEmulation => c.middle_click_emulation,
    }
}

pub fn describe_value(value: SettingValue, c: &'static Copy) -> String {
    match value {
        // A percentage rather than a bare fraction: the scale is a Better OS
        // one, so a raw 0.55 would look like a backend number the user could
        // look up somewhere, and it is not.
        SettingValue::Sensitivity { value } => format!("{:.0}%", value.get() * 100.0),
        SettingValue::Factor { value } => format!("{:.2}×", value.get()),
        SettingValue::Toggle { value } => if value { "on" } else { "off" }.to_string(),
        SettingValue::Acceleration { value } => match value {
            touchpad_core::AccelerationProfile::Default => c.profile_default.to_string(),
            touchpad_core::AccelerationProfile::Adaptive => c.profile_adaptive.to_string(),
            touchpad_core::AccelerationProfile::Flat => c.profile_flat.to_string(),
        },
        SettingValue::Click { value } => match value {
            touchpad_core::ClickMethod::Default => c.method_default.to_string(),
            touchpad_core::ClickMethod::Areas => c.method_areas.to_string(),
            touchpad_core::ClickMethod::Fingers => c.method_fingers.to_string(),
            touchpad_core::ClickMethod::None => c.method_none.to_string(),
        },
    }
}

pub fn describe_reading(reading: &Reading, c: &'static Copy) -> String {
    match reading {
        Reading::Value { value } => describe_value(*value, c),
        Reading::SessionDefault { .. } => c.value_session_default.to_string(),
        Reading::Unsupported { .. } => c.unavailable.to_string(),
        Reading::PermissionDenied { .. } => c.value_permission_denied.to_string(),
        Reading::Unknown { reason } if reason == "touchpad.not_read_yet" => {
            c.value_not_read.to_string()
        }
        Reading::Unknown { .. } => c.value_unknown.to_string(),
    }
}

fn describe_outcome(outcome: &touchpad_core::StepOutcome, c: &'static Copy) -> String {
    match outcome {
        touchpad_core::StepOutcome::Applied { .. } => c.result_applied.to_string(),
        touchpad_core::StepOutcome::AwaitingSignOut { .. } => {
            c.result_awaiting_sign_out.to_string()
        }
        touchpad_core::StepOutcome::PartiallySupported { effective, .. } => {
            format!("{} {}", c.result_partial, describe_reading(effective, c))
        }
        touchpad_core::StepOutcome::Failed { detail, .. } => {
            format!("{} {detail}", c.result_failed)
        }
        touchpad_core::StepOutcome::Unsupported { detail, .. } => {
            format!("{} {detail}", c.unavailable)
        }
    }
}
