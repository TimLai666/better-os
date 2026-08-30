//! What the tray does when someone clicks something.
//!
//! The controller holds the last status the service reported, the tray-local
//! quick options, and the menu built from them. It owns no session state of its
//! own: everything it knows came from the service, and everything it changes it
//! changes by asking the service.

use std::sync::Arc;
use std::time::Instant;

use awake_ipc::{AwakeRequest, RequestBody, StatusDocument, WireEnd, WireIndicator};
use tokio::sync::{Mutex, Notify};

use crate::client::{ClientError, ServiceClient, menu_request, start_request};
use crate::labels::Locale;
use crate::localtime::UtcOffset;
use crate::menu::{Menu, MenuAction, OverrideConfirmation, QuickOptions, build};
use crate::sni::{ITEM_PATH, MENU_PATH, icon_name, item_status};

/// The binary the tray opens for anything that needs a window: the time picker,
/// the Change dialog, and the first-time security confirmation.
///
/// This is a direct execution of a known first-party binary with a fixed
/// argument list. It is not a shell, and no part of it comes from a rule, a
/// menu label, or any other input.
pub const APPLICATION_BINARY: &str = "awake-gui";

/// What an activation turned into. Returned so a caller — including a test —
/// can see the outcome without watching the bus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Activation {
    /// The service accepted it and the status was updated.
    Applied,
    /// A local toggle that only shapes the next session.
    OptionToggled,
    /// The override was armed. Nothing was asked of the service; the menu now
    /// shows the confirming label, and a second activation of *that* is what
    /// switches every rule off.
    OverrideArmed,
    /// The action needs the full window, which was asked to open.
    OpenedApplication,
    Quitting,
    /// Nothing is bound to that id, which is what an info line or a separator
    /// being clicked means.
    Ignored,
    Failed(ClientError),
}

struct State {
    status: StatusDocument,
    options: QuickOptions,
    menu: Menu,
    revision: u32,
    offset: UtcOffset,
    /// The tray-local half of the two-step override.
    override_confirmation: OverrideConfirmation,
}

pub struct TrayController {
    client: ServiceClient,
    locale: Locale,
    state: Mutex<State>,
    connection: Mutex<Option<zbus::Connection>>,
    quit: Notify,
    /// The tray's own monotonic origin. The override arming is measured against
    /// this rather than against the service's wall clock, which can step.
    started: Instant,
}

impl TrayController {
    pub fn new(client: ServiceClient, locale: Locale, status: StatusDocument) -> Self {
        let options = QuickOptions::default();
        let offset = UtcOffset::for_system(status.now_unix_seconds);
        let menu = build(&status, options, locale, offset, false);
        Self {
            client,
            locale,
            state: Mutex::new(State {
                status,
                options,
                menu,
                revision: 1,
                offset,
                override_confirmation: OverrideConfirmation::default(),
            }),
            connection: Mutex::new(None),
            quit: Notify::new(),
            started: Instant::now(),
        }
    }

    /// Monotonic seconds since the tray started.
    fn monotonic_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Handed the connection once the objects are served, so property changes
    /// can be announced.
    pub async fn attach(&self, connection: zbus::Connection) {
        *self.connection.lock().await = Some(connection);
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub async fn menu(&self) -> Menu {
        self.state.lock().await.menu.clone()
    }

    pub async fn revision(&self) -> u32 {
        self.state.lock().await.revision
    }

    pub async fn status(&self) -> StatusDocument {
        self.state.lock().await.status.clone()
    }

    pub async fn indicator(&self) -> WireIndicator {
        self.state.lock().await.status.indicator
    }

    /// Waits until something asks the tray to quit.
    pub async fn quit_requested(&self) {
        self.quit.notified().await;
    }

    /// Asks the service what is true now.
    pub async fn refresh(&self) -> Result<(), ClientError> {
        let status = self.client.status().await?;
        self.set_status(status).await;
        Ok(())
    }

    /// Replaces the status and rebuilds everything that is drawn from it.
    pub async fn set_status(&self, status: StatusDocument) {
        let now = self.monotonic_seconds();
        let (revision, indicator) = {
            let mut state = self.state.lock().await;
            let armed = state.override_confirmation.is_armed(now);
            state.offset = UtcOffset::for_system(status.now_unix_seconds);
            state.menu = build(&status, state.options, self.locale, state.offset, armed);
            state.revision = state.revision.wrapping_add(1).max(1);
            state.status = status;
            (state.revision, state.status.indicator)
        };
        self.announce(revision, indicator).await;
    }

    /// The tooltip, which carries the same facts as the menu header so the icon
    /// is readable without opening anything.
    pub async fn tooltip(&self) -> (String, String) {
        let state = self.state.lock().await;
        let labels = self.locale.labels();
        let title = labels.application_name.to_string();
        let body = match state.status.manual_session() {
            Some(session) if state.status.backend.available => {
                format!("{} — {}", labels.active_summary, session.reason)
            }
            Some(_) => labels.backend_unavailable.to_string(),
            None if !state.status.backend.available => labels.backend_unavailable.to_string(),
            None => labels.inactive_summary.to_string(),
        };
        (title, body)
    }

    /// Runs the action bound to a menu item.
    pub async fn activate(&self, item_id: i32) -> Activation {
        let action = { self.state.lock().await.menu.action(item_id) };
        let Some(action) = action else {
            return Activation::Ignored;
        };
        self.perform(action).await
    }

    pub async fn perform(&self, action: MenuAction) -> Activation {
        let now = self.monotonic_seconds();
        let (options, session_id, armed) = {
            let state = self.state.lock().await;
            (
                state.options,
                state
                    .status
                    .manual_session()
                    .map(|session| session.session_id),
                state.override_confirmation.is_armed(now),
            )
        };
        let reason = self.locale.labels().tray_session_reason;

        // Choosing anything else drops an armed override, so a confirmation
        // someone walked away from cannot be completed by a later click on a
        // menu that has since been used for something else.
        if armed && !matches!(action, MenuAction::ConfirmOverrideAllRules) {
            self.rearm_override(None).await;
        }

        let request = match action {
            MenuAction::ArmOverrideAllRules => {
                self.rearm_override(Some(now)).await;
                return Activation::OverrideArmed;
            }
            MenuAction::ConfirmOverrideAllRules => {
                if !armed {
                    // A stale layout, a replayed event, or a host that kept the
                    // confirming id from a previous open. The arming is what
                    // authorizes this, and it is not live.
                    return Activation::Ignored;
                }
                self.rearm_override(None).await;
                match menu_request(action) {
                    Some(request) => request,
                    None => return Activation::Ignored,
                }
            }
            MenuAction::EndSession | MenuAction::PauseRules(_) | MenuAction::ResumeRules => {
                match menu_request(action) {
                    Some(request) => request,
                    None => return Activation::Ignored,
                }
            }
            MenuAction::StartIndefinite => {
                start_request(reason, WireEnd::Indefinite, options, false)
            }
            MenuAction::StartMinutes(minutes) => start_request(
                reason,
                WireEnd::Duration {
                    seconds: minutes * 60,
                },
                options,
                false,
            ),
            MenuAction::ExtendMinutes(minutes) => {
                let Some(session_id) = session_id else {
                    return Activation::Ignored;
                };
                AwakeRequest::new(RequestBody::ExtendSession {
                    session_id,
                    by_seconds: minutes * 60,
                })
            }
            MenuAction::ToggleAllowDisplayOff => {
                self.toggle(|options| options.allow_display_off = !options.allow_display_off)
                    .await;
                return Activation::OptionToggled;
            }
            MenuAction::ToggleStopBelowBattery => {
                self.toggle(|options| options.stop_below_battery = !options.stop_below_battery)
                    .await;
                return Activation::OptionToggled;
            }
            // A time picker, a change dialog, and the first-time security
            // warning all need more room than a panel menu has.
            MenuAction::StartUntilTime
            | MenuAction::ChangeSession
            | MenuAction::OpenApplication => {
                self.open_application();
                return Activation::OpenedApplication;
            }
            MenuAction::Quit => {
                self.quit.notify_waiters();
                return Activation::Quitting;
            }
        };

        match self.client.send(request).await {
            Ok(status) => {
                self.set_status(status).await;
                Activation::Applied
            }
            Err(ClientError::Rejected(error_key))
                if error_key == "awake.error.security_confirmation_required" =>
            {
                // The consequence has to be shown before it is accepted, and
                // the tray has nowhere to show it.
                self.open_application();
                Activation::OpenedApplication
            }
            Err(error) => {
                // The menu must not keep showing a state the service did not
                // agree to, so the truth is fetched again either way.
                let _ = self.refresh().await;
                Activation::Failed(error)
            }
        }
    }

    async fn toggle(&self, change: impl FnOnce(&mut QuickOptions)) {
        let now = self.monotonic_seconds();
        let (revision, indicator) = {
            let mut state = self.state.lock().await;
            change(&mut state.options);
            let armed = state.override_confirmation.is_armed(now);
            state.menu = build(
                &state.status,
                state.options,
                self.locale,
                state.offset,
                armed,
            );
            state.revision = state.revision.wrapping_add(1).max(1);
            (state.revision, state.status.indicator)
        };
        self.announce(revision, indicator).await;
    }

    /// Arms or drops the override, and redraws the menu so the label matches.
    /// `Some(now)` arms; `None` drops.
    async fn rearm_override(&self, armed_at: Option<u64>) {
        let (revision, indicator) = {
            let mut state = self.state.lock().await;
            match armed_at {
                Some(now) => state.override_confirmation.arm(now),
                None => state.override_confirmation.clear(),
            }
            let armed = armed_at.is_some();
            state.menu = build(
                &state.status,
                state.options,
                self.locale,
                state.offset,
                armed,
            );
            state.revision = state.revision.wrapping_add(1).max(1);
            (state.revision, state.status.indicator)
        };
        self.announce(revision, indicator).await;
    }

    pub async fn options(&self) -> QuickOptions {
        self.state.lock().await.options
    }

    /// Whether the override control is currently showing its confirming label.
    pub async fn override_armed(&self) -> bool {
        let now = self.monotonic_seconds();
        self.state.lock().await.override_confirmation.is_armed(now)
    }

    fn open_application(&self) {
        // Failure is not fatal to the tray: the session it is showing is
        // unaffected by whether a window opened.
        let _ = std::process::Command::new(APPLICATION_BINARY).spawn();
    }

    /// Tells the panel that the layout and the icon changed.
    async fn announce(&self, revision: u32, indicator: WireIndicator) {
        let connection = { self.connection.lock().await.clone() };
        let Some(connection) = connection else {
            return;
        };
        let _ = connection
            .emit_signal(
                None::<&str>,
                MENU_PATH,
                "com.canonical.dbusmenu",
                "LayoutUpdated",
                &(revision, 0i32),
            )
            .await;
        // NewIcon and NewToolTip carry nothing; NewStatus carries the new value.
        for signal in ["NewIcon", "NewToolTip"] {
            let _ = connection
                .emit_signal(
                    None::<&str>,
                    ITEM_PATH,
                    "org.kde.StatusNotifierItem",
                    signal,
                    &(),
                )
                .await;
        }
        let _ = connection
            .emit_signal(
                None::<&str>,
                ITEM_PATH,
                "org.kde.StatusNotifierItem",
                "NewStatus",
                &(item_status(indicator),),
            )
            .await;
        let _ = icon_name(indicator);
    }
}

/// Convenience for callers that hold the controller behind an `Arc`.
pub type SharedController = Arc<TrayController>;
