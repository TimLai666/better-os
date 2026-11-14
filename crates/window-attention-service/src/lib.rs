//! `better-window-attention`: decides, per application, what a window asking
//! for attention does (ticket 106, ADR 0018).
//!
//! The Better OS GNOME Shell adapter extension reports each request and does
//! what it is told; it holds no choice and makes no decision. This service is
//! the other half. It owns `org.betteros.WindowAttention1`, which is what
//! tells the extension a decider is present, asks the extension to take
//! GNOME's own handler off the two display signals, and then answers every
//! request it reports with one of four shell actions, decided by
//! `window_attention_core::decide` from the choices Better Manager wrote.
//!
//! When this process ends for any reason, its bus name goes with it and the
//! extension puts GNOME's handler back by itself, so a crash, a stop, and an
//! uninstall all leave GNOME's stock banner in place.
//!
//! [`Relay`] is the decision loop with the bus taken out, so the whole of it
//! is tested against a fake shell and a temporary store; [`bus`] is the
//! transport and holds no rule of its own.

pub mod bus;

use std::collections::BTreeMap;

use window_attention_core::store::Store;
use window_attention_core::{
    AppId, AttentionRequest, Choice, Choices, RequestKind, SeenApps, ShellAction, decide,
};

/// The four things the extension can be asked to do, and the switch.
pub trait Shell {
    fn suppress_built_in(&mut self, suppress: bool) -> Result<(), String>;
    /// Marks the application's taskbar icon urgent (the Unity launcher API).
    fn mark_taskbar(&mut self, app: &AppId) -> Result<(), String>;
    /// Takes that mark back.
    fn unmark_taskbar(&mut self, app: &AppId) -> Result<(), String>;
    fn show_banner(&mut self, window: u64) -> Result<(), String>;
    fn activate(&mut self, window: u64) -> Result<(), String>;
    fn clear(&mut self, window: u64) -> Result<(), String>;
}

/// What happened to one request, for the journal and for tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Handled {
    pub action: ShellAction,
    /// The action could not be performed; the request stays on the window.
    pub failure: Option<String>,
    /// The choices file could not be read, so the default was used.
    pub choices_problem: Option<String>,
    /// The application was new or renamed and was written to the seen list.
    pub recorded: bool,
    /// The seen list could not be written.
    pub record_problem: Option<String>,
}

/// The decision loop.
pub struct Relay {
    store: Store,
    clock: fn() -> u64,
    /// The windows whose application's taskbar icon this service marked, so
    /// the mark is taken back when the window is focused or closed, its
    /// request is withdrawn, or the choice stops asking for it.
    marks: BTreeMap<u64, AppId>,
}

fn now_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

impl Relay {
    pub fn new(store: Store) -> Self {
        Self::with_clock(store, now_unix_seconds)
    }

    pub fn with_clock(store: Store, clock: fn() -> u64) -> Self {
        Self {
            store,
            clock,
            marks: BTreeMap::new(),
        }
    }

    /// The windows whose taskbar mark this service is holding.
    pub fn marked(&self) -> &BTreeMap<u64, AppId> {
        &self.marks
    }

    /// Forgets one window's mark, and takes the application's mark back when
    /// no other window of it is still marked. Returns the failure, if any.
    fn release(&mut self, shell: &mut dyn Shell, window: u64) -> Option<String> {
        let app = self.marks.remove(&window)?;
        if self.marks.values().any(|other| *other == app) {
            return None;
        }
        shell.unmark_taskbar(&app).err()
    }

    /// A reported window stopped asking: focused, closed, or withdrawn.
    pub fn ended(&mut self, shell: &mut dyn Shell, window: u64) -> Option<String> {
        self.release(shell, window)
    }

    /// Better Manager saved the choices: every mark the new choices no longer
    /// ask for is taken back. Returns the failures.
    pub fn choices_changed(&mut self, shell: &mut dyn Shell) -> Vec<String> {
        let Ok(choices) = self.store.load_choices() else {
            return Vec::new();
        };
        let stale: Vec<u64> = self
            .marks
            .iter()
            .filter(|(_, app)| choices.for_app(Some(app)) != Choice::MarkOnly)
            .map(|(window, _)| *window)
            .collect();
        stale
            .into_iter()
            .filter_map(|window| self.release(shell, window))
            .collect()
    }

    /// The extension went away: nothing more will be reported about the
    /// windows, so every mark is taken back now.
    pub fn adapter_gone(&mut self, shell: &mut dyn Shell) -> Vec<String> {
        let windows: Vec<u64> = self.marks.keys().copied().collect();
        windows
            .into_iter()
            .filter_map(|window| self.release(shell, window))
            .collect()
    }

    /// The extension appeared, or came back after a shell restart: take
    /// GNOME's own handler off so this service answers instead.
    pub fn adapter_appeared(&self, shell: &mut dyn Shell) -> Result<(), String> {
        shell.suppress_built_in(true)
    }

    /// Answers one request.
    ///
    /// The choices are read for every request rather than cached, so a choice
    /// made in Better Manager applies to the very next request with nothing to
    /// reload. A request is rare, and the file is a few hundred bytes.
    pub fn handle(&mut self, shell: &mut dyn Shell, request: &AttentionRequest) -> Handled {
        let (choices, choices_problem) = match self.store.load_choices() {
            Ok(choices) => (choices, None),
            // An unreadable file is reported and the default used: the
            // person still gets the quiet behaviour they installed this for,
            // and Better Manager shows the same error on the page.
            Err(error) => (Choices::default(), Some(error.to_string())),
        };
        let decision = decide(&choices, request);
        // A window asking again under a choice that no longer marks it gives
        // its earlier mark back first.
        let previous = if matches!(decision.action, ShellAction::MarkTaskbar(_)) {
            None
        } else {
            self.release(shell, request.window)
        };
        let failure = match &decision.action {
            ShellAction::Leave => None,
            ShellAction::MarkTaskbar(app) => shell.mark_taskbar(app).err(),
            ShellAction::ShowBanner(window) => shell.show_banner(*window).err(),
            ShellAction::Activate(window) => shell.activate(*window).err(),
            ShellAction::Clear(window) => shell.clear(*window).err(),
        };
        if failure.is_none()
            && let ShellAction::MarkTaskbar(app) = &decision.action
        {
            self.marks.insert(request.window, app.clone());
        }
        let mut handled = Handled {
            action: decision.action,
            failure: failure.or(previous),
            choices_problem,
            recorded: false,
            record_problem: None,
        };
        if let Some((app, name)) = decision.record {
            match self.store.load_seen() {
                Ok(mut seen) => {
                    if seen.record(&app, &name, (self.clock)()) {
                        match self.store.save_seen(&seen) {
                            Ok(()) => handled.recorded = true,
                            Err(error) => handled.record_problem = Some(error.to_string()),
                        }
                    }
                }
                // A seen list this version cannot read is left as it is
                // rather than replaced by one with a single entry.
                Err(error) => handled.record_problem = Some(error.to_string()),
            }
        }
        handled
    }

    /// The seen list, for a status answer.
    pub fn seen(&self) -> SeenApps {
        self.store.load_seen().unwrap_or_default()
    }
}

/// Reads the `WindowAttention` signal's body. Anything that is not exactly the
/// six values in order, with a kind the contract names, is not a request.
pub fn parse_request(body: (u64, String, String, u32, bool, bool)) -> Option<AttentionRequest> {
    let (window, app_id, app_name, kind, focused, skip_taskbar) = body;
    Some(AttentionRequest {
        window,
        app_id,
        app_name,
        kind: RequestKind::from_wire(kind)?,
        focused,
        skip_taskbar,
    })
}

#[cfg(test)]
mod tests;
