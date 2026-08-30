//! The window's state and the four things a user can ask it to do.
//!
//! Reading, applying, and restoring all run on the calling thread rather than
//! on a background task. That is a measured decision, not a shortcut: a full
//! read of every setting takes tens of microseconds against the recorded
//! database, and an apply plus its verifying read takes about 4 ms against the
//! real dconf service (`crates/touchpad-platform/tests/live_apply.rs`). A task
//! and a channel would add more machinery than the work they moved.
//!
//! No method here builds a command, a key name, or a path. Every change goes
//! through the typed backend.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{AppContext as _, Bounds, Context, Entity, Pixels, Window};
use gpui_component::slider::{SliderEvent, SliderState, SliderValue};
use touchpad_core::{
    RestoreScope, RunState, ScrollFactor, Section, Sensitivity, SettingId, SettingValue,
    TouchpadStore,
};
use touchpad_platform::TouchpadBackend;

use crate::i18n::Locale;
use crate::model::{Control, Page, PointerTrace, TouchpadModel};
use crate::startup::{Startup, now};

pub struct TouchpadApp {
    pub(crate) model: TouchpadModel,
    pub(crate) backend: Box<dyn TouchpadBackend>,
    pub(crate) store: TouchpadStore,
    pub(crate) page: Page,
    pub(crate) pointer: PointerTrace,
    /// The test surface's bounds, refreshed every frame by a canvas element so
    /// a pointer position can be turned into a fraction of the surface.
    pub(crate) surface: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) sliders: Vec<(SettingId, Entity<SliderState>)>,
    pub(crate) busy: bool,
}

impl TouchpadApp {
    pub fn new(startup: Startup, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let Startup {
            model,
            backend,
            store,
        } = startup;

        let mut sliders = Vec::new();
        for setting in SettingId::ALL {
            let Control::Slider { min, max, step } = crate::model::control_for(setting) else {
                continue;
            };
            let value = model
                .state()
                .state(setting)
                .requested()
                .as_number()
                .unwrap_or(0.0);
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(min as f32)
                    .max(max as f32)
                    .step(step as f32)
                    .default_value(value as f32)
            });
            cx.subscribe_in(
                &slider,
                window,
                move |this: &mut Self, _, event: &SliderEvent, _window, cx| {
                    let SliderEvent::Change(SliderValue::Single(value)) = event else {
                        return;
                    };
                    this.stage_number(setting, f64::from(*value));
                    cx.notify();
                },
            )
            .detach();
            sliders.push((setting, slider));
        }

        Self {
            model,
            backend,
            store,
            page: Page::Overview,
            pointer: PointerTrace::idle(),
            surface: Rc::new(Cell::new(Bounds::default())),
            sliders,
            busy: false,
        }
    }

    pub fn model(&self) -> &TouchpadModel {
        &self.model
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    pub fn slider(&self, setting: SettingId) -> Option<&Entity<SliderState>> {
        self.sliders
            .iter()
            .find(|(id, _)| *id == setting)
            .map(|(_, slider)| slider)
    }

    /// Stages a number a slider produced.
    ///
    /// An out-of-range value cannot get here — the slider's own bounds are the
    /// supported range — but if one did, the core refuses it and nothing is
    /// staged, which is the behavior a clamp would hide.
    pub fn stage_number(&mut self, setting: SettingId, value: f64) {
        let staged = match setting.kind() {
            touchpad_core::ValueKind::Sensitivity => {
                Sensitivity::new(value).map(SettingValue::sensitivity)
            }
            touchpad_core::ValueKind::Factor => ScrollFactor::new(value).map(SettingValue::factor),
            _ => return,
        };
        if let Ok(value) = staged {
            let _ = self.model.stage(setting, value);
        }
    }

    pub fn stage_toggle(&mut self, setting: SettingId, value: bool, cx: &mut Context<Self>) {
        let _ = self.model.stage(setting, SettingValue::toggle(value));
        cx.notify();
    }

    pub fn stage_value(&mut self, setting: SettingId, value: SettingValue, cx: &mut Context<Self>) {
        let _ = self.model.stage(setting, value);
        cx.notify();
    }

    pub fn stage_linked_axes(&mut self, linked: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.model.stage_linked_axes(linked);
        self.sync_sliders(window, cx);
        cx.notify();
    }

    /// Reads every effective value again.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.model.refresh(self.backend.as_ref());
        cx.notify();
    }

    pub fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.discard();
        self.sync_sliders(window, cx);
        cx.notify();
    }

    /// Applies what is staged and shows what actually happened.
    pub fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) -> RunState {
        self.busy = true;
        let state = self
            .model
            .apply(self.backend.as_mut(), Some(&self.store), now());
        // The read-back the apply already did is the new effective value, but
        // a partial or failed run can have moved settings the plan did not
        // name, so everything is read once more.
        self.model.refresh(self.backend.as_ref());
        self.sync_sliders(window, cx);
        self.busy = false;
        cx.notify();
        state
    }

    pub fn restore(
        &mut self,
        scope: RestoreScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<RunState> {
        self.busy = true;
        let state = self
            .model
            .restore(self.backend.as_mut(), scope, Some(&self.store));
        self.model.refresh(self.backend.as_ref());
        self.sync_sliders(window, cx);
        self.busy = false;
        cx.notify();
        state
    }

    pub fn restore_section(
        &mut self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<RunState> {
        self.restore(RestoreScope::Section { section }, window, cx)
    }

    pub fn toggle_safe_mode(&mut self, cx: &mut Context<Self>) {
        let wanted = !self.model.safe_mode();
        let written = if wanted {
            self.store.enable_safe_mode()
        } else {
            self.store.clear_safe_mode()
        };
        match written {
            Ok(()) => self.model.set_safe_mode(wanted),
            Err(error) => self
                .model
                .set_configuration_problem(Some(error.to_string())),
        }
        cx.notify();
    }

    pub fn toggle_locale(&mut self, cx: &mut Context<Self>) {
        let next = self.model.locale().toggled();
        self.model.set_locale(next);
        cx.notify();
    }

    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.model.set_locale(locale);
        cx.notify();
    }

    /// Records where the pointer is inside the test surface.
    pub fn trace_pointer(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = self.surface.get();
        self.pointer = PointerTrace::at(
            f32::from(position.x - bounds.origin.x),
            f32::from(position.y - bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        );
        cx.notify();
    }

    /// Puts every slider back on the value the model now holds.
    ///
    /// `set_value` does not emit a change event, which matters: a slider moved
    /// by the model rather than by the user must not stage the value that was
    /// just unstaged.
    fn sync_sliders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted: Vec<(Entity<SliderState>, f32)> = self
            .sliders
            .iter()
            .filter_map(|(setting, slider)| {
                let value = self.model.state().state(*setting).requested().as_number()?;
                Some((slider.clone(), value as f32))
            })
            .collect();
        for (slider, value) in wanted {
            slider.update(cx, |state, cx| {
                state.set_value(value, window, cx);
            });
        }
    }
}
