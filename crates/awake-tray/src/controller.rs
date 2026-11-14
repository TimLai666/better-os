//! What the tray does when someone clicks something.
//!
//! The controller holds the last status the service reported, the tray-local
//! quick options, and the menu built from them. It owns no session state of its
//! own: everything it knows came from the service, and everything it changes it
//! changes by asking the service.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use awake_ipc::{AwakeRequest, RequestBody, StatusDocument, WireEnd, WireIndicator, WireSession};
use tokio::sync::{Mutex, Notify};
use zbus::zvariant::OwnedValue;

use crate::client::{ClientError, ServiceClient, menu_request, start_request};
use crate::dbusmenu::all_properties;
use crate::labels::{Labels, Locale, LocaleLabels};
use crate::localtime::UtcOffset;
use crate::menu::{Menu, MenuAction, MenuItem, OverrideConfirmation, QuickOptions, build};
use crate::notify::{DesktopNotifier, claim_notifications, handle_event};
use crate::sni::{ITEM_PATH, MENU_PATH, item_status};

/// How many replaced layouts are still answered. A click the host sends
/// with an id from the layout it was showing a moment ago, while the new one
/// was on its way, still does what the person saw.
const RETIRED_LAYOUTS: usize = 3;

/// What changed when the menu was rebuilt, which decides what the host is told.
#[derive(Debug, PartialEq)]
enum MenuUpdate {
    Unchanged,
    /// Same items, different wording or state: `ItemsPropertiesUpdated`.
    Properties(Vec<(i32, HashMap<String, OwnedValue>)>),
    /// Different items: `LayoutUpdated`, under ids the host has never seen.
    Layout(u32),
}

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
    /// The menu as `build` returns it, numbered from 1. Compared against the
    /// next build to tell a wording change from a layout change.
    built: Menu,
    /// The menu the host is shown: `built`, numbered from `first_id`.
    menu: Menu,
    first_id: i32,
    /// The first id the next new layout gets. Ids only ever move forward, so
    /// no host can mistake a new item for an old one it cached.
    next_id: i32,
    /// The layouts replaced most recently, newest first.
    retired: VecDeque<Menu>,
    revision: u32,
    offset: UtcOffset,
    /// The tray-local half of the two-step override.
    override_confirmation: OverrideConfirmation,
}

impl State {
    /// Puts a freshly built menu in place and says what the host must be told.
    fn install(&mut self, built: Menu) -> MenuUpdate {
        if built == self.built {
            return MenuUpdate::Unchanged;
        }
        if built.shape() == self.built.shape() {
            let menu = built.clone().renumbered(self.first_id);
            let mut changed = Vec::new();
            for (old, new) in self.menu.items.iter().zip(&menu.items) {
                changed_properties(old, new, &mut changed);
            }
            self.built = built;
            self.menu = menu;
            return MenuUpdate::Properties(changed);
        }

        let count = i32::try_from(built.len()).unwrap_or(i32::MAX / 2);
        if self.next_id > i32::MAX - count - 1 {
            // Two billion ids in, start again from the bottom. The layouts that
            // used the low ids are long gone from any host.
            self.next_id = 1;
        }
        let menu = built.clone().renumbered(self.next_id);
        self.first_id = self.next_id;
        self.next_id += count;
        let replaced = std::mem::replace(&mut self.menu, menu);
        self.retired.push_front(replaced);
        self.retired.truncate(RETIRED_LAYOUTS);
        self.built = built;
        self.revision = self.revision.wrapping_add(1).max(1);
        MenuUpdate::Layout(self.revision)
    }

    /// The action bound to an id in the layout shown now, or failing that in
    /// one shown just before it.
    fn action(&self, id: i32) -> Option<MenuAction> {
        if let Some(item) = self.menu.find(id) {
            return item.action;
        }
        self.retired
            .iter()
            .find_map(|menu| menu.find(id))
            .and_then(|item| item.action)
    }
}

/// Collects, for two items with the same shape, every id whose properties
/// differ.
fn changed_properties(
    old: &MenuItem,
    new: &MenuItem,
    changed: &mut Vec<(i32, HashMap<String, OwnedValue>)>,
) {
    let properties = all_properties(new);
    if all_properties(old) != properties {
        changed.push((new.id, properties));
    }
    for (old, new) in old.children.iter().zip(&new.children) {
        changed_properties(old, new, changed);
    }
}

pub struct TrayController {
    client: ServiceClient,
    locale: Locale,
    state: Mutex<State>,
    /// Held from installing a menu until the host has been told about it, so
    /// two rebuilds racing cannot send their signals in the other order: a
    /// host that read the older layout last would keep it.
    announcing: Mutex<()>,
    connection: Mutex<Option<zbus::Connection>>,
    /// Where a refused action is said. `None` until one is attached, and on a
    /// desktop with no notification service.
    notifier: Mutex<Option<DesktopNotifier>>,
    quit: Notify,
    /// The tray's own monotonic origin. The override arming is measured against
    /// this rather than against the service's wall clock, which can step.
    started: Instant,
}

impl TrayController {
    pub fn new(client: ServiceClient, locale: Locale, status: StatusDocument) -> Self {
        let options = QuickOptions::default();
        let offset = UtcOffset::for_system(status.now_unix_seconds);
        let built = build(&status, options, locale, offset, false);
        let next_id = 1 + i32::try_from(built.len()).unwrap_or(0);
        Self {
            client,
            locale,
            state: Mutex::new(State {
                status,
                options,
                menu: built.clone(),
                built,
                first_id: 1,
                next_id,
                retired: VecDeque::new(),
                revision: 1,
                offset,
                override_confirmation: OverrideConfirmation::default(),
            }),
            connection: Mutex::new(None),
            notifier: Mutex::new(None),
            announcing: Mutex::new(()),
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

    /// Hands the controller the notifier it says a refused action through.
    pub async fn attach_notifier(&self, notifier: DesktopNotifier) {
        *self.notifier.lock().await = Some(notifier);
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

    /// The menu and its revision, read together so a `GetLayout` reply never
    /// pairs one layout with another's revision.
    pub async fn layout(&self) -> (u32, Menu) {
        let state = self.state.lock().await;
        (state.revision, state.menu.clone())
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
        let _announcing = self.announcing.lock().await;
        let (update, indicator) = {
            let mut state = self.state.lock().await;
            let armed = state.override_confirmation.is_armed(now);
            state.offset = UtcOffset::for_system(status.now_unix_seconds);
            let built = build(&status, state.options, self.locale, state.offset, armed);
            state.status = status;
            (state.install(built), state.status.indicator)
        };
        self.announce_menu(update).await;
        self.announce_item(indicator).await;
    }

    /// Follows the service until its signal stream ends: every status it
    /// pushes redraws the menu, and when the service comes back on the bus
    /// after a restart the status is read again, because a new service says
    /// nothing until something changes.
    ///
    /// The events are subscribed to before the low-battery notification is
    /// claimed, so the service leaves that notification to this tray exactly
    /// while this tray can hear a stop; see [`claim_notifications`].
    pub async fn follow_service(
        self: Arc<Self>,
        connection: zbus::Connection,
        notifier: Option<DesktopNotifier>,
    ) -> Result<(), ClientError> {
        use zbus::export::futures_core::Stream;

        let statuses = self.client.status_updates().await?;
        let legacy_statuses = self.client.legacy_status_updates().await?;
        let owners = self.client.owner_changes().await?;
        let notifier = claim_notifications(&connection, notifier).await;
        let mut statuses = std::pin::pin!(statuses);
        let mut legacy_statuses = std::pin::pin!(legacy_statuses);
        let mut owners = std::pin::pin!(owners);
        loop {
            tokio::select! {
                signal = std::future::poll_fn(|context| statuses.as_mut().poll_next(context)) => {
                    let Some(signal) = signal else { return Ok(()) };
                    let Ok(args) = signal.args() else { continue };
                    if let Some(status) = handle_event(args.event_json(), notifier.as_ref()).await {
                        self.set_status(status).await;
                    }
                }
                // A current service sends every event on both signals, so the
                // legacy one is read only while the service is a legacy one,
                // which sends nothing else.
                signal = std::future::poll_fn(|context| legacy_statuses.as_mut().poll_next(context)) => {
                    let Some(signal) = signal else { return Ok(()) };
                    if !self.client.speaks_legacy() {
                        continue;
                    }
                    let Ok(args) = signal.args() else { continue };
                    if let Some(status) = handle_event(args.event_json(), notifier.as_ref()).await {
                        self.set_status(status).await;
                    }
                }
                owner = std::future::poll_fn(|context| owners.as_mut().poll_next(context)) => {
                    match owner {
                        None => return Ok(()),
                        Some(Some(_)) => {
                            // A service that came back may be a newer one: an
                            // update restarts it. Ask in the current version
                            // first again.
                            self.client.reset_protocol();
                            if let Err(error) = self.refresh().await {
                                eprintln!(
                                    "better-awake-tray: the Better Awake service came back and \
                                     could not be read: {error}"
                                );
                            }
                        }
                        // Gone. Nothing is redrawn: the next action either
                        // starts it again through D-Bus activation or is
                        // refused, and a refusal is said.
                        Some(None) => {
                            eprintln!("better-awake-tray: the Better Awake service left the bus");
                        }
                    }
                }
            }
        }
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
        let action = { self.state.lock().await.action(item_id) };
        let Some(action) = action else {
            return Activation::Ignored;
        };
        let activation = self.perform(action).await;
        if let Activation::Failed(error) = &activation {
            self.report_failure(error).await;
        }
        activation
    }

    pub async fn perform(&self, action: MenuAction) -> Activation {
        let now = self.monotonic_seconds();
        let (options, manual, armed) = {
            let state = self.state.lock().await;
            (
                state.options,
                state.status.manual_session().cloned(),
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
            MenuAction::StartIndefinite | MenuAction::StartMinutes(_) => {
                let end = match action {
                    MenuAction::StartMinutes(minutes) => WireEnd::Duration {
                        seconds: minutes * 60,
                    },
                    _ => WireEnd::Indefinite,
                };
                return self.start_or_change(reason, end, options, manual).await;
            }
            MenuAction::ExtendMinutes(minutes) => {
                let Some(session) = manual else {
                    return Activation::Ignored;
                };
                AwakeRequest::new(RequestBody::ExtendSession {
                    session_id: session.session_id,
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
            MenuAction::ToggleKeepAwakeLidClosed => {
                self.toggle(|options| {
                    options.keep_awake_lid_closed = !options.keep_awake_lid_closed
                })
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

        self.send(request).await
    }

    /// A length chosen from the menu. With no session of the person's own it
    /// starts one; with one running it changes that one's end, measured from
    /// now, keeping its reason and policy. Starting twice is never an error.
    ///
    /// The menu the click came from may be older than the service's state, so
    /// a start the service refuses because a session is already running is
    /// read again and sent as the change it should have been.
    async fn start_or_change(
        &self,
        reason: &str,
        end: WireEnd,
        options: QuickOptions,
        manual: Option<WireSession>,
    ) -> Activation {
        let request = match &manual {
            Some(session) => change_request(session, end),
            None => start_request(reason, end, options, false),
        };
        match self.client.send(request).await {
            // The menu was older than the service. Either a session started
            // that the menu did not know about, or the one it knew ended:
            // read what is true and send what the choice means now.
            Err(ClientError::Rejected(key))
                if (manual.is_none()
                    && key.starts_with("awake.error.manual_session_already_active"))
                    || (manual.is_some() && key.starts_with("awake.error.unknown_session")) =>
            {
                let status = match self.client.status().await {
                    Ok(status) => status,
                    Err(error) => return Activation::Failed(error),
                };
                let running = status.manual_session().cloned();
                self.set_status(status).await;
                match running {
                    Some(session) => self.send(change_request(&session, end)).await,
                    None => self.send(start_request(reason, end, options, false)).await,
                }
            }
            reply => self.settle(reply).await,
        }
    }

    async fn send(&self, request: AwakeRequest) -> Activation {
        let reply = self.client.send(request).await;
        self.settle(reply).await
    }

    /// Turns the service's answer into an activation, redrawing either way.
    async fn settle(&self, reply: Result<StatusDocument, ClientError>) -> Activation {
        match reply {
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

    /// Says a refused action out loud. A click that does nothing visible is
    /// the failure the owner reported, so the tray never stays silent.
    async fn report_failure(&self, error: &ClientError) {
        let body = failure_text(error, self.locale.labels());
        eprintln!("better-awake-tray: a menu action failed ({error}): {body}");
        let notifier = self.notifier.lock().await;
        if let Some(notifier) = notifier.as_ref()
            && let Err(notify_error) = notifier.action_failed(body).await
        {
            eprintln!("better-awake-tray: and the notification could not be shown: {notify_error}");
        }
    }

    async fn toggle(&self, change: impl FnOnce(&mut QuickOptions)) {
        let now = self.monotonic_seconds();
        let _announcing = self.announcing.lock().await;
        let update = {
            let mut state = self.state.lock().await;
            change(&mut state.options);
            let armed = state.override_confirmation.is_armed(now);
            let built = build(
                &state.status,
                state.options,
                self.locale,
                state.offset,
                armed,
            );
            state.install(built)
        };
        self.announce_menu(update).await;
    }

    /// Arms or drops the override, and redraws the menu so the label matches.
    /// `Some(now)` arms; `None` drops.
    async fn rearm_override(&self, armed_at: Option<u64>) {
        let _announcing = self.announcing.lock().await;
        let update = {
            let mut state = self.state.lock().await;
            match armed_at {
                Some(now) => state.override_confirmation.arm(now),
                None => state.override_confirmation.clear(),
            }
            let armed = armed_at.is_some();
            let built = build(
                &state.status,
                state.options,
                self.locale,
                state.offset,
                armed,
            );
            state.install(built)
        };
        self.announce_menu(update).await;
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
        match std::process::Command::new(APPLICATION_BINARY).spawn() {
            // Reaped when it closes, so no zombie stays behind the tray.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => {
                eprintln!(
                    "better-awake-tray: the Better Awake window could not be opened: {error}"
                );
            }
        }
    }

    /// Tells the host what changed in the menu, in the form the AppIndicator
    /// extension acts on: a new layout under new ids, or the changed
    /// properties of the ids it already has.
    async fn announce_menu(&self, update: MenuUpdate) {
        let connection = { self.connection.lock().await.clone() };
        let Some(connection) = connection else {
            return;
        };
        let _ = match update {
            MenuUpdate::Unchanged => return,
            MenuUpdate::Properties(changed) if changed.is_empty() => return,
            MenuUpdate::Properties(changed) => {
                connection
                    .emit_signal(
                        None::<&str>,
                        MENU_PATH,
                        "com.canonical.dbusmenu",
                        "ItemsPropertiesUpdated",
                        &(changed, Vec::<(i32, Vec<String>)>::new()),
                    )
                    .await
            }
            MenuUpdate::Layout(revision) => {
                connection
                    .emit_signal(
                        None::<&str>,
                        MENU_PATH,
                        "com.canonical.dbusmenu",
                        "LayoutUpdated",
                        &(revision, 0i32),
                    )
                    .await
            }
        };
    }

    /// Tells the panel that the icon, the tooltip, and the status may have
    /// changed. The host re-reads each property these name.
    async fn announce_item(&self, indicator: WireIndicator) {
        let connection = { self.connection.lock().await.clone() };
        let Some(connection) = connection else {
            return;
        };
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
    }
}

/// The request a length chosen while a session runs becomes: the same session
/// with a new end, so nothing is started beside it and nothing is refused.
pub fn change_request(session: &WireSession, end: WireEnd) -> AwakeRequest {
    AwakeRequest::new(RequestBody::ChangeSession {
        session_id: session.session_id,
        reason: session.reason.clone(),
        policy: session.policy,
        battery_stop_percent: session.battery_stop_percent,
        end,
        // The running session already carries its policy, so whatever
        // confirmation it needed was given when it started.
        security_confirmed: false,
    })
}

/// What a refused action is said as, in plain words.
pub fn failure_text(error: &ClientError, labels: &'static Labels) -> &'static str {
    match error {
        ClientError::ServiceUnavailable(_) | ClientError::Transport(_) => {
            labels.failed_service_unavailable
        }
        ClientError::Protocol(_) => labels.failed_other,
        ClientError::Rejected(key) => {
            if key.contains("backend_unavailable") {
                labels.failed_backend_unavailable
            } else if key.contains("no_manual_session") {
                labels.failed_no_manual_session
            } else if key.contains("unknown_session") {
                labels.failed_session_gone
            } else if key.contains("cannot_extend_indefinite") {
                labels.failed_cannot_extend
            } else if key.contains("end_time_in_the_past") {
                labels.failed_end_in_the_past
            } else if key.contains("too_many_sessions") {
                labels.failed_too_many_sessions
            } else {
                labels.failed_other
            }
        }
    }
}

/// Convenience for callers that hold the controller behind an `Arc`.
pub type SharedController = Arc<TrayController>;

#[cfg(test)]
mod tests {
    use super::*;
    use awake_core::{BackendCapabilities, SessionOrigin, SessionPolicy};
    use awake_ipc::{WireBackend, WireReason, WireRemaining};

    const NOW: u64 = 1_700_000_000;

    fn inactive() -> StatusDocument {
        StatusDocument {
            indicator: WireIndicator::Inactive,
            effective_policy: SessionPolicy::default(),
            unmet_policy: Vec::new(),
            battery_stop_percent: None,
            sessions: Vec::new(),
            reasons: Vec::new(),
            backend: WireBackend {
                name: "logind".to_string(),
                available: true,
                capabilities: BackendCapabilities::NONE,
                detail: None,
            },
            attention: None,
            interrupted_previous_session: None,
            reduced_security_confirmed: false,
            active_rules: Vec::new(),
            rule_summary: awake_ipc::WireRuleSummary::default(),
            rules_suppression: None,
            conflicts: Vec::new(),
            providers: Vec::new(),
            battery_protection: awake_ipc::WireBatteryProtection::default(),
            lid: awake_ipc::WireLidProtection::default(),
            now_unix_seconds: NOW,
        }
    }

    fn session(remaining: u64) -> WireSession {
        WireSession {
            session_id: 4,
            reason: "Rendering".to_string(),
            origin: SessionOrigin::Manual,
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(30),
            end: WireEnd::UntilUnixSeconds {
                unix_seconds: NOW + remaining,
            },
            started_at_unix_seconds: NOW,
            remaining: WireRemaining::Seconds { seconds: remaining },
        }
    }

    fn awake(remaining: u64) -> StatusDocument {
        let session = session(remaining);
        StatusDocument {
            indicator: WireIndicator::ActiveManual,
            reasons: vec![WireReason {
                session_id: session.session_id,
                origin: SessionOrigin::Manual,
                reason: session.reason.clone(),
            }],
            sessions: vec![session],
            ..inactive()
        }
    }

    fn built(status: &StatusDocument) -> Menu {
        build(
            status,
            QuickOptions::default(),
            Locale::EnUs,
            UtcOffset::UTC,
            false,
        )
    }

    fn state(status: StatusDocument) -> State {
        let built = built(&status);
        let next_id = 1 + built.len() as i32;
        State {
            status,
            options: QuickOptions::default(),
            menu: built.clone(),
            built,
            first_id: 1,
            next_id,
            retired: VecDeque::new(),
            revision: 1,
            offset: UtcOffset::UTC,
            override_confirmation: OverrideConfirmation::default(),
        }
    }

    fn ids(menu: &Menu) -> Vec<i32> {
        let mut ids = Vec::new();
        for item in &menu.items {
            item.walk(&mut |candidate| ids.push(candidate.id));
        }
        ids
    }

    #[test]
    fn a_new_layout_never_reuses_an_id_a_host_has_already_cached() {
        let mut state = state(inactive());
        let mut seen: Vec<i32> = ids(&state.menu);
        for status in [awake(3_600), inactive(), awake(600), inactive()] {
            let update = state.install(built(&status));
            assert!(matches!(update, MenuUpdate::Layout(_)), "{update:?}");
            for id in ids(&state.menu) {
                assert!(!seen.contains(&id), "id {id} was reused");
                seen.push(id);
            }
        }
        assert_eq!(state.revision, 5);
    }

    #[test]
    fn a_wording_change_keeps_the_ids_and_names_only_what_changed() {
        let mut state = state(awake(3_600));
        let before = ids(&state.menu);
        let revision = state.revision;

        let MenuUpdate::Properties(changed) = state.install(built(&awake(3_000))) else {
            panic!("a new countdown is not a new layout");
        };
        assert_eq!(ids(&state.menu), before);
        assert_eq!(state.revision, revision, "the layout itself did not change");
        assert_eq!(changed.len(), 1, "{changed:?}");
        let remaining = state.menu.find(changed[0].0).unwrap();
        assert_eq!(remaining.label, "Remaining: 50 min");

        assert_eq!(state.install(built(&awake(3_000))), MenuUpdate::Unchanged);
    }

    #[test]
    fn a_click_from_the_layout_just_replaced_still_does_what_it_showed() {
        let mut state = state(inactive());
        let old = state.menu.find_by_label("15 minutes").unwrap().id;
        state.install(built(&awake(600)));
        assert!(state.menu.find(old).is_none());
        assert_eq!(state.action(old), Some(MenuAction::StartMinutes(15)));

        for _ in 0..RETIRED_LAYOUTS {
            state.install(built(&inactive()));
            state.install(built(&awake(600)));
        }
        assert_eq!(state.action(old), None, "only the last few layouts count");
    }

    #[test]
    fn a_length_chosen_while_awake_changes_that_session_and_keeps_its_terms() {
        let session = session(600);
        let request = change_request(&session, WireEnd::Duration { seconds: 1_800 });
        assert_eq!(
            request.body,
            RequestBody::ChangeSession {
                session_id: 4,
                reason: "Rendering".to_string(),
                policy: SessionPolicy::quick_default(),
                battery_stop_percent: Some(30),
                end: WireEnd::Duration { seconds: 1_800 },
                security_confirmed: false,
            }
        );
        assert!(request.validate().is_ok());
    }

    #[test]
    fn every_refusal_is_said_in_plain_words() {
        let labels = Locale::EnUs.labels();
        for (error, expected) in [
            (
                ClientError::Transport("no reply".to_string()),
                labels.failed_service_unavailable,
            ),
            (
                ClientError::Rejected("awake.error.backend_unavailable".to_string()),
                labels.failed_backend_unavailable,
            ),
            (
                ClientError::Rejected("awake.error.no_manual_session".to_string()),
                labels.failed_no_manual_session,
            ),
            (
                ClientError::Rejected(
                    "awake.error.end_condition:awake.error.cannot_extend_indefinite".to_string(),
                ),
                labels.failed_cannot_extend,
            ),
            (
                ClientError::Rejected(
                    "awake.error.end_condition:awake.error.end_time_in_the_past".to_string(),
                ),
                labels.failed_end_in_the_past,
            ),
            (
                ClientError::Rejected("awake.error.unknown_session:4".to_string()),
                labels.failed_session_gone,
            ),
            (
                ClientError::Rejected("awake.error.too_many_sessions".to_string()),
                labels.failed_too_many_sessions,
            ),
            (
                ClientError::Rejected("awake.error.something_new".to_string()),
                labels.failed_other,
            ),
        ] {
            assert_eq!(failure_text(&error, labels), expected, "{error}");
        }
    }
}
