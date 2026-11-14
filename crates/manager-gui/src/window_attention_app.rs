//! The Window attention page's state and the work it runs off the UI thread.
//! Reads and the one shell change go through `window_attention_bus`; every
//! rule is in `window_attention_page`. Saving a choice writes one small file
//! and is done where it is asked for.

use gpui::{AppContext as _, Context, Task};
use window_attention_core::store::Store;
use window_attention_core::{Choice, Choices, SeenApps};

use crate::app::ManagerApp;
use crate::window_attention_bus::{self, Loaded};
use crate::window_attention_page::{
    self as page, AppRow, ExtensionState, InstalledApp, ServiceState,
};

/// Everything the page holds between interactions.
pub(crate) struct WindowAttentionState {
    pub(crate) store: Store,
    pub(crate) icons: std::collections::HashMap<window_attention_core::AppId, better_ui::AppIcon>,
    /// `None` until read; an error when the file is there and cannot be read,
    /// which stops the page from writing over it.
    pub(crate) choices: Option<Result<Choices, String>>,
    pub(crate) seen: SeenApps,
    pub(crate) installed: Vec<InstalledApp>,
    pub(crate) extension: ExtensionState,
    pub(crate) service: ServiceState,
    /// The last save or turn-on that failed, as (summary, details).
    pub(crate) failure: Option<(&'static str, String)>,
    pub(crate) turning_on: bool,
    pub(crate) task: Option<Task<()>>,
}

impl Default for WindowAttentionState {
    fn default() -> Self {
        Self {
            store: Store::for_user(),
            icons: Default::default(),
            choices: None,
            seen: SeenApps::default(),
            installed: Vec::new(),
            extension: ExtensionState::Checking,
            service: ServiceState::Checking,
            failure: None,
            turning_on: false,
            task: None,
        }
    }
}

impl WindowAttentionState {
    /// The choices, when they were read cleanly.
    pub(crate) fn choices(&self) -> Option<&Choices> {
        self.choices
            .as_ref()
            .and_then(|choices| choices.as_ref().ok())
    }

    pub(crate) fn rows(&self) -> Vec<AppRow> {
        self.choices()
            .map(|choices| page::rows(choices, &self.seen, &self.installed))
            .unwrap_or_default()
    }

    fn adopt(&mut self, loaded: Loaded) {
        self.icons = loaded.icons;
        self.choices = Some(loaded.choices);
        self.seen = loaded.seen;
        self.installed = loaded.installed;
        self.extension = loaded.extension;
        self.service = loaded.service;
    }
}

impl ManagerApp {
    /// Reads the two files, the menu, the extension, and the service. Asking
    /// the service for its status also starts it when it is installed and not
    /// running, which is how opening the page brings it up.
    pub(crate) fn refresh_window_attention(&mut self, cx: &mut Context<Self>) {
        if self.window_attention.task.is_some() {
            return;
        }
        let store = self.window_attention.store.clone();
        self.window_attention.task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move { window_attention_bus::load(&store) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.window_attention.task = None;
                app.window_attention.adopt(loaded);
                cx.notify();
            });
        }));
    }

    /// Turns the shell extension on, then reads everything again.
    pub(crate) fn turn_on_shell_adapter(&mut self, cx: &mut Context<Self>) {
        if self.window_attention.turning_on {
            return;
        }
        self.window_attention.turning_on = true;
        self.window_attention.failure = None;
        cx.notify();
        let store = self.window_attention.store.clone();
        cx.spawn(async move |this, cx| {
            let (outcome, loaded) = cx
                .background_spawn(async move {
                    let outcome = window_attention_bus::turn_on();
                    // The shell enables an extension asynchronously and the
                    // service takes over once it sees it; a moment later the
                    // page reads what actually happened.
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    (outcome, window_attention_bus::load(&store))
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.window_attention.turning_on = false;
                if let Err(error) = outcome {
                    app.window_attention.failure =
                        Some((page::copy(app.locale).enable_failed, error));
                }
                app.window_attention.adopt(loaded);
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes a change to the choices and keeps it only once it is on disk.
    fn save_window_attention(
        &mut self,
        change: impl FnOnce(&mut Choices) -> bool,
        cx: &mut Context<Self>,
    ) {
        let Some(mut choices) = self.window_attention.choices().cloned() else {
            return;
        };
        if !change(&mut choices) {
            return;
        }
        match self.window_attention.store.save_choices(&choices) {
            Ok(()) => {
                self.window_attention.choices = Some(Ok(choices));
                self.window_attention.failure = None;
                cx.background_spawn(async { window_attention_bus::choices_changed() })
                    .detach();
            }
            Err(error) => {
                self.window_attention.failure =
                    Some((page::copy(self.locale).save_failed, error.to_string()));
            }
        }
        cx.notify();
    }

    pub(crate) fn set_attention_default(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(choice) = Choice::ALL.get(index).copied() else {
            return;
        };
        self.save_window_attention(
            |choices| {
                let changed = choices.default != choice;
                choices.default = choice;
                changed
            },
            cx,
        );
    }

    pub(crate) fn set_attention_for_app(
        &mut self,
        row: AppRow,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.save_window_attention(|choices| page::pick(choices, &row, index), cx);
    }

    pub(crate) fn add_attention_app(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        self.save_window_attention(
            |choices| {
                if choices.apps.contains_key(&app.id) {
                    return false;
                }
                page::add(choices, &app);
                true
            },
            cx,
        );
    }
}
