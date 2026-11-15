//! Touchpad (spec section 5, ticket 88): all of today's Better Touchpad as one
//! page of the control center.
//!
//! The page is a renderer. Every decision it draws comes from
//! `touchpad-views`, which has no GPUI dependency and carries the tests the
//! older window had: which controls exist and whether each is available,
//! staging, the apply and its read-back, the capture taken before the first
//! change, per-section restore, and the whole gesture screen. What this page
//! adds is decided in [`view`] (its sub-pages, states, the "This touchpad"
//! card, the Apply bar, search, and Home), and the file chooser in
//! [`portal`].
//!
//! Nothing here builds a backend command. Every change goes through
//! `touchpad-platform`'s typed backend and `touchpad-gestures`' store; a test
//! asserts it over this directory's source as the older window's did.

mod copy;
pub(crate) mod portal;
mod render;
mod render_gestures;
#[cfg(test)]
mod tests;
pub(crate) mod view;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{AppContext as _, Bounds, Context, Entity, Pixels, Task, WeakEntity, Window};
use gpui_component::input::InputState;
use serde::{Deserialize, Serialize};
use touchpad_core::{
    RestoreScope, RunState, ScrollFactor, Section, Sensitivity, SettingId, SettingValue,
    TouchpadStore,
};
use touchpad_gestures::{ConflictResolution, GestureId, GestureStore, RunState as GestureRunState};
use touchpad_platform::{Roots, TouchpadBackend};
use touchpad_views::gestures_model::GestureScreen;
use touchpad_views::{PointerTrace, Startup, StartupOptions, TouchpadModel};

pub(crate) use view::{SubPage, TouchpadFacts};

use crate::app::ManagerApp;
use crate::i18n::Locale;
use crate::navigation::Route;
use crate::pages::{PageModule, PageStatus};

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::Touchpad,
    name: "touchpad",
    status: PageStatus::Built,
    title: |s| s.touchpad,
    search_entries: view::search_entries,
    attention: view::attention,
    badge: |_| None,
};

/// What a restart after an update carries over for this page (ticket 105):
/// the sub-page, the device whose gesture profile is open, and the two paths.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TouchpadView {
    pub(crate) sub_page: String,
    pub(crate) gesture_device: Option<String>,
    pub(crate) export_path: Option<String>,
    pub(crate) import_path: Option<String>,
}

/// Input the page holds that a restart would lose. The rule is the older
/// window's (`touchpad-gui::restart`): running work, an open editor, or
/// changes not yet applied or cancelled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct InFlight {
    pub(crate) applying: bool,
    pub(crate) unapplied_settings: bool,
    pub(crate) preview: bool,
    pub(crate) import: bool,
    pub(crate) editor: bool,
    pub(crate) live_test: bool,
    pub(crate) choosing_file: bool,
}

impl InFlight {
    pub(crate) fn blocker(self) -> Option<better_ui::restart::Blocker> {
        use better_ui::restart::Blocker;
        if self.applying || self.live_test {
            Some(Blocker::Task)
        } else if self.editor || self.choosing_file {
            Some(Blocker::Dialog)
        } else if self.unapplied_settings || self.preview || self.import {
            Some(Blocker::UnsavedChanges)
        } else {
            None
        }
    }
}

/// How the page reaches the machine. A real window reads the kernel's
/// devices and GNOME's settings; a demo window, and the screenshots, use the
/// in-memory backend so no dconf key is written (ticket 88's acceptance).
#[derive(Clone, Debug)]
pub(crate) struct Environment {
    pub(crate) roots: Roots,
    pub(crate) store_directory: PathBuf,
    pub(crate) in_memory: bool,
}

impl Environment {
    pub(crate) fn for_window(demo: bool) -> Self {
        if demo {
            // A demo writes nothing outside its own scratch directory, which
            // is what a demo promises.
            return Self {
                roots: Roots::system(),
                store_directory: std::env::temp_dir().join(format!(
                    "better-manager-demo-touchpad-{}",
                    std::process::id()
                )),
                in_memory: true,
            };
        }
        Self {
            roots: Roots::system(),
            store_directory: TouchpadStore::user_directory(),
            in_memory: false,
        }
    }
}

/// What Home and the page share: the facts Home's items are made from.
pub(crate) type SharedFacts = Rc<RefCell<TouchpadFacts>>;

/// What the export or import file chooser is doing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum FileNote {
    #[default]
    None,
    Exported(PathBuf),
    PortalMissing,
    NotAFile,
}

pub(crate) struct TouchpadPage {
    pub(crate) host: WeakEntity<ManagerApp>,
    pub(crate) locale: Locale,
    pub(crate) model: TouchpadModel,
    pub(crate) backend: Box<dyn TouchpadBackend>,
    pub(crate) store: TouchpadStore,
    pub(crate) gestures: GestureScreen,
    pub(crate) gesture_store: GestureStore,
    pub(crate) sub_page: SubPage,
    pub(crate) pointer: PointerTrace,
    /// The pointer test surface's bounds, refreshed every frame by a canvas
    /// element so a pointer position can be turned into a fraction of it.
    pub(crate) surface: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) busy: bool,
    pub(crate) facts: SharedFacts,
    /// The two path fields. They are shown only when the file chooser portal
    /// did not answer; a path is a filename and never reaches an action, and
    /// what comes out of an imported file is validated before it is shown.
    pub(crate) export_path: Entity<InputState>,
    pub(crate) import_path: Entity<InputState>,
    /// `false` once the portal failed to answer: the text fields are the way
    /// in from then on.
    pub(crate) portal: bool,
    pub(crate) file_note: FileNote,
    pub(crate) file_task: Option<Task<()>>,
    pub(crate) this_expanded: bool,
    pub(crate) device_details: Option<usize>,
}

impl TouchpadPage {
    pub(crate) fn new(
        host: WeakEntity<ManagerApp>,
        locale: Locale,
        environment: &Environment,
        facts: SharedFacts,
        restored: Option<&TouchpadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Startup {
            model,
            backend,
            store,
            mut gestures,
            gesture_store,
            ..
        } = Startup::run(StartupOptions {
            roots: environment.roots.clone(),
            store_directory: environment.store_directory.clone(),
            locale: view::views_locale(locale),
            offline: false,
            in_memory: environment.in_memory,
            page: touchpad_views::Page::Pointer,
        });
        let default_path = gesture_store
            .settings()
            .directory()
            .join("gestures-export.json")
            .display()
            .to_string();
        let sub_page = restored
            .map(|view| SubPage::from_key(&view.sub_page))
            .unwrap_or_default();
        if let Some(view) = restored {
            gestures.select_device(view.gesture_device.clone());
        }
        let export_value = restored
            .and_then(|view| view.export_path.clone())
            .unwrap_or_else(|| default_path.clone());
        let import_value = restored
            .and_then(|view| view.import_path.clone())
            .unwrap_or(default_path);
        let export_path = cx.new(|cx| InputState::new(window, cx).default_value(export_value));
        let import_path = cx.new(|cx| InputState::new(window, cx).default_value(import_value));
        let page = Self {
            host,
            locale,
            model,
            backend,
            store,
            gestures,
            gesture_store,
            sub_page,
            pointer: PointerTrace::idle(),
            surface: Rc::new(Cell::new(Bounds::default())),
            busy: false,
            facts,
            export_path,
            import_path,
            portal: true,
            file_note: FileNote::None,
            file_task: None,
            this_expanded: false,
            device_details: None,
        };
        page.share_facts();
        page
    }

    /// Keeps Home's facts in step with what the page now knows.
    fn share_facts(&self) {
        let mut facts = self.facts.borrow_mut();
        facts.safe_mode = self.model.safe_mode();
        facts.gestures_turned_off = self.gesture_store.turned_off_by_itself();
    }

    pub(crate) fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        self.model.set_locale(view::views_locale(locale));
        cx.notify();
    }

    pub(crate) fn show(&mut self, sub_page: SubPage, cx: &mut Context<Self>) {
        self.sub_page = sub_page;
        cx.notify();
    }

    pub(crate) fn in_flight(&self) -> InFlight {
        InFlight {
            applying: self.busy,
            unapplied_settings: self.model.has_pending(),
            preview: self.gestures.plan().is_some(),
            import: self.gestures.import().is_some(),
            editor: self.gestures.editor().is_some(),
            live_test: self.gestures.live_testing(),
            choosing_file: self.file_task.is_some(),
        }
    }

    pub(crate) fn restart_view(&self, cx: &gpui::App) -> TouchpadView {
        TouchpadView {
            sub_page: self.sub_page.key().to_string(),
            gesture_device: self.gestures.active_device().map(str::to_string),
            export_path: Some(self.export_path.read(cx).value().to_string()),
            import_path: Some(self.import_path.read(cx).value().to_string()),
        }
    }

    // Settings: stage, apply, discard, restore, safe mode.

    /// Stages a number a slider produced. An out-of-range value cannot get
    /// here (the slider's bounds are the supported range); if one did, the
    /// core refuses it and nothing is staged, which a clamp would hide.
    pub(crate) fn stage_number(&mut self, setting: SettingId, value: f64, cx: &mut Context<Self>) {
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
        cx.notify();
    }

    pub(crate) fn stage_value(
        &mut self,
        setting: SettingId,
        value: SettingValue,
        cx: &mut Context<Self>,
    ) {
        let _ = self.model.stage(setting, value);
        cx.notify();
    }

    pub(crate) fn stage_linked_axes(&mut self, linked: bool, cx: &mut Context<Self>) {
        self.model.stage_linked_axes(linked);
        cx.notify();
    }

    /// Reads every effective value and the devices again.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.model.refresh(self.backend.as_ref());
        self.share_facts();
        cx.notify();
    }

    pub(crate) fn discard(&mut self, cx: &mut Context<Self>) {
        self.model.discard();
        cx.notify();
    }

    /// Applies what is staged, verifies it by reading back, and shows what
    /// actually happened. A measured decision, kept from the older window: an
    /// apply plus its read-back takes about 4 ms against the real dconf
    /// service, so it runs here rather than on a task.
    pub(crate) fn apply(&mut self, cx: &mut Context<Self>) -> RunState {
        self.busy = true;
        let state = self.model.apply(
            self.backend.as_mut(),
            Some(&self.store),
            touchpad_views::startup::now(),
        );
        // A partial or failed run can have moved settings the plan did not
        // name, so everything is read once more.
        self.model.refresh(self.backend.as_ref());
        self.busy = false;
        cx.notify();
        state
    }

    pub(crate) fn restore(
        &mut self,
        scope: RestoreScope,
        cx: &mut Context<Self>,
    ) -> Option<RunState> {
        self.busy = true;
        let state = self
            .model
            .restore(self.backend.as_mut(), scope, Some(&self.store));
        self.model.refresh(self.backend.as_ref());
        self.busy = false;
        cx.notify();
        state
    }

    pub(crate) fn restore_section(
        &mut self,
        section: Section,
        cx: &mut Context<Self>,
    ) -> Option<RunState> {
        self.restore(RestoreScope::Section { section }, cx)
    }

    pub(crate) fn set_safe_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        let written = if on {
            self.store.enable_safe_mode()
        } else {
            self.store.clear_safe_mode()
        };
        match written {
            Ok(()) => {
                self.model.set_safe_mode(on);
                // Safe mode gives the desktop its own gestures back at once,
                // as starting in it does.
                if on {
                    self.gestures.enter_safe_mode();
                }
            }
            Err(error) => self
                .model
                .set_configuration_problem(Some(error.to_string())),
        }
        self.share_facts();
        cx.notify();
    }

    /// Records where the pointer is inside the test surface.
    pub(crate) fn trace_pointer(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = self.surface.get();
        self.pointer = PointerTrace::at(
            f32::from(position.x - bounds.origin.x),
            f32::from(position.y - bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        );
        cx.notify();
    }

    // Gestures.

    pub(crate) fn preview_preset(&mut self, cx: &mut Context<Self>) {
        self.gestures.preview_preset();
        cx.notify();
    }

    pub(crate) fn cancel_preview(&mut self, cx: &mut Context<Self>) {
        self.gestures.cancel_preview();
        cx.notify();
    }

    pub(crate) fn confirm_preset(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        self.gestures.confirm(confirmed);
        cx.notify();
    }

    pub(crate) fn resolve_conflict(
        &mut self,
        gesture: GestureId,
        resolution: ConflictResolution,
        cx: &mut Context<Self>,
    ) {
        self.gestures.resolve(gesture, resolution);
        cx.notify();
    }

    pub(crate) fn apply_plan(&mut self, cx: &mut Context<Self>) -> Option<GestureRunState> {
        let outcome = self.gestures.apply_plan(Some(&self.gesture_store));
        if let Err(error) = &outcome {
            self.gestures.set_problem(Some(error.to_string()));
        }
        self.share_facts();
        cx.notify();
        outcome.ok()
    }

    pub(crate) fn restore_gestures(&mut self, cx: &mut Context<Self>) -> Option<GestureRunState> {
        let outcome = self.gestures.restore(Some(&self.gesture_store));
        self.share_facts();
        cx.notify();
        outcome
    }

    pub(crate) fn disable_gestures(&mut self, cx: &mut Context<Self>) -> GestureRunState {
        let outcome = self.gestures.disable(Some(&self.gesture_store));
        self.share_facts();
        cx.notify();
        outcome
    }

    pub(crate) fn toggle_gesture(
        &mut self,
        gesture: &GestureId,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.gestures
            .set_enabled(gesture, enabled, Some(&self.gesture_store));
        cx.notify();
    }

    pub(crate) fn select_gesture_device(&mut self, device: Option<String>, cx: &mut Context<Self>) {
        self.gestures.select_device(device);
        cx.notify();
    }

    pub(crate) fn detach_gesture_profile(&mut self, cx: &mut Context<Self>) {
        self.gestures
            .detach_device_profile(Some(&self.gesture_store));
        cx.notify();
    }

    pub(crate) fn forget_gesture_profile(&mut self, cx: &mut Context<Self>) {
        self.gestures
            .forget_device_profile(Some(&self.gesture_store));
        cx.notify();
    }

    pub(crate) fn edit_gesture(&mut self, gesture: &GestureId, cx: &mut Context<Self>) {
        self.gestures.edit(gesture);
        cx.notify();
    }

    pub(crate) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.gestures.cancel_edit();
        cx.notify();
    }

    pub(crate) fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let _ = self.gestures.commit_edit(Some(&self.gesture_store));
        cx.notify();
    }

    /// Replays a gesture and shows what the recognizer saw. Nothing reaches
    /// the desktop unless live testing is on.
    pub(crate) fn test_gesture(&mut self, gesture: &GestureId, cx: &mut Context<Self>) {
        let c = touchpad_views::i18n::copy(view::views_locale(self.locale));
        self.gestures.test_gesture(gesture, c);
        cx.notify();
    }

    pub(crate) fn set_live_testing(&mut self, live: bool, cx: &mut Context<Self>) {
        self.gestures.set_live_testing(live);
        cx.notify();
    }

    // Export and import.

    /// Writes every gesture profile to `path`.
    pub(crate) fn export_to(&mut self, path: &Path, cx: &mut Context<Self>) {
        let problem = if path.as_os_str().is_empty() {
            Some("gestures.export.no_path".to_string())
        } else {
            self.gesture_store
                .export_to(path, &self.gestures.document())
                .err()
                .map(|error| error.to_string())
        };
        self.file_note = match problem {
            None => FileNote::Exported(path.to_path_buf()),
            Some(_) => FileNote::None,
        };
        self.gestures.set_problem(problem);
        cx.notify();
    }

    /// Reads a profile document and previews it. Nothing is applied here: the
    /// preview goes through the same confirmation the preset does.
    pub(crate) fn import_from(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.file_note = FileNote::None;
        match self.gesture_store.import_from(path) {
            Ok(document) => {
                self.gestures.set_problem(None);
                self.gestures
                    .preview_import(path.display().to_string(), document);
            }
            Err(error) => self.gestures.set_problem(Some(error.to_string())),
        }
        cx.notify();
    }

    /// Asks the desktop's file chooser where to export, or what to import.
    /// When the portal does not answer, the page shows the path fields
    /// instead and says why.
    pub(crate) fn choose_file(&mut self, export: bool, cx: &mut Context<Self>) {
        if self.file_task.is_some() {
            return;
        }
        let c = copy::page_copy(self.locale);
        let tc = touchpad_views::i18n::copy(view::views_locale(self.locale));
        let folder = self.gesture_store.settings().directory().to_path_buf();
        let folder = if folder.is_dir() {
            folder
        } else {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        };
        let (pick, title, accept) = if export {
            (
                portal::Pick::Save {
                    name: "gestures-export.json".to_string(),
                    folder,
                },
                c.export_title,
                tc.export_profiles,
            )
        } else {
            (
                portal::Pick::Open { folder },
                c.import_title,
                tc.import_profiles,
            )
        };
        self.file_task = Some(cx.spawn(async move |this, cx| {
            let picked = cx
                .background_spawn(async move { portal::pick(&pick, title, accept) })
                .await;
            let _ = this.update(cx, |page, cx| {
                page.file_task = None;
                page.picked(export, picked, cx);
            });
        }));
        cx.notify();
    }

    fn picked(&mut self, export: bool, picked: portal::Picked, cx: &mut Context<Self>) {
        match picked {
            portal::Picked::Chosen(path) if export => self.export_to(&path, cx),
            portal::Picked::Chosen(path) => self.import_from(&path, cx),
            portal::Picked::Cancelled => {}
            portal::Picked::NotAFile(_) => self.file_note = FileNote::NotAFile,
            portal::Picked::Unavailable(_) => {
                self.portal = false;
                self.file_note = FileNote::PortalMissing;
            }
        }
        cx.notify();
    }

    /// The Install or Update button of the "not installed" and "too old"
    /// states: the component's page in Apps, where installing is reviewed.
    pub(crate) fn open_component(&self, cx: &mut Context<Self>) {
        let _ = self.host.update(cx, |app, cx| {
            app.navigate(
                crate::model::Page::ComponentDetail(view::component_id()),
                cx,
            )
        });
    }
}
