//! What the tray does when someone clicks something.
//!
//! The controller holds the last status the service reported, the tray-local
//! quick options, and the menu built from them. It owns no session state of its
//! own: everything it knows came from the service, and everything it changes it
//! changes by asking the service.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// How long a held layout waits after the host's last open submenu closed.
/// GNOME closes one submenu just before it opens the next, and a layout sent
/// in between would rebuild the next one closed.
pub const SUBMENU_SETTLE: Duration = Duration::from_millis(300);

/// How long a submenu counts as open after the host last said it opened. A
/// row the host destroyed never says it closed, and a host may never send
/// the whole menu's events; past this a held layout goes out anyway.
pub const SUBMENU_LIMIT: Duration = Duration::from_secs(300);

/// Which submenus the host has open, from its `opened` and `closed` events.
///
/// Each event carries an order: the D-Bus serial of the call, which only
/// grows on the host's connection. Calls are handled on separate tasks, so
/// an `opened` can be handled after the `closed` that followed it; the
/// order is what puts them back in sequence. An event about an id older
/// than the last one seen for it, or older than the last event of the whole
/// menu (id 0), is stale and ignored. Serials of two connections cannot be
/// compared, so an event from another connection than the last one is a new
/// host, which starts with nothing open.
#[derive(Debug, Default)]
pub struct OpenSubmenus {
    /// The connection the events come from.
    host: String,
    /// Open submenu ids, each with the order and time of its `opened`.
    open: HashMap<i32, (u64, Instant)>,
    /// The order of the last event seen for each id since the whole menu's.
    last: HashMap<i32, u64>,
    /// The order of the last event of the whole menu.
    root: u64,
    /// Until when a submenu that just closed still counts as open.
    settle_until: Option<Instant>,
}

impl OpenSubmenus {
    /// Records one event from `host`; `opened` false is `closed`.
    pub fn event(&mut self, host: &str, id: i32, opened: bool, order: u64, now: Instant) {
        if host != self.host {
            self.reset();
            self.host = host.to_string();
        }
        if id == 0 {
            if order < self.root {
                return;
            }
            // Nothing inside a menu that is opening or closing is open.
            self.root = order;
            self.open.retain(|_, (seen, _)| *seen > order);
            self.last.retain(|_, seen| *seen > order);
            self.settle_until = None;
            return;
        }
        if order <= self.root || self.last.get(&id).is_some_and(|seen| *seen >= order) {
            return;
        }
        self.open
            .retain(|_, (_, opened)| now.saturating_duration_since(*opened) < SUBMENU_LIMIT);
        self.last.insert(id, order);
        if opened {
            self.open.insert(id, (order, now));
        } else if self.open.remove(&id).is_some() && self.open.is_empty() {
            self.settle_until = Some(now + SUBMENU_SETTLE);
        }
    }

    /// Whether a new layout must wait: a submenu is open and was opened less
    /// than [`SUBMENU_LIMIT`] ago, or one closed less than
    /// [`SUBMENU_SETTLE`] ago.
    pub fn busy(&self, now: Instant) -> bool {
        self.settle_until.is_some_and(|until| now < until)
            || self
                .open
                .values()
                .any(|(_, opened)| now.saturating_duration_since(*opened) < SUBMENU_LIMIT)
    }

    /// A new host (the panel restarted): it has nothing open, and the order
    /// of its calls starts again.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// What changed when the menu was rebuilt, which decides what the host is told.
#[derive(Debug, PartialEq)]
enum MenuUpdate {
    Unchanged,
    /// Same items, different wording or state: `ItemsPropertiesUpdated`,
    /// carrying only the properties that changed.
    Properties(Vec<(i32, HashMap<String, OwnedValue>)>),
    /// Different items: `LayoutUpdated`, under ids the host has never seen.
    Layout(u32),
    /// Different items while the host has a submenu open: held until it
    /// closes, because the AppIndicator extension rebuilds every row it
    /// knows on `LayoutUpdated`, and a rebuilt submenu row is a closed one.
    /// What can be said meanwhile without a new layout is carried, as for
    /// `Properties`: new wording on rows that still mean the same thing, and
    /// rows whose action no longer applies greyed out.
    Deferred(Vec<(i32, HashMap<String, OwnedValue>)>),
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
    /// A rebuilt menu with a different layout, held while a submenu is open.
    deferred: Option<Menu>,
    /// The submenus the host says are open.
    submenus: OpenSubmenus,
    revision: u32,
    offset: UtcOffset,
    /// The tray-local half of the two-step override.
    override_confirmation: OverrideConfirmation,
}

impl State {
    /// Puts a freshly built menu in place and says what the host must be told.
    fn install(&mut self, built: Menu) -> MenuUpdate {
        self.install_at(built, Instant::now())
    }

    /// A menu shaped like the one the host has is a change of properties and
    /// goes out at once, whatever is open. A different shape waits while the
    /// host has a submenu open (see [`OpenSubmenus::busy`]); until
    /// [`State::release_at`] the layout the host has stays served, so its
    /// ids stay true, showing what [`Menu::showing`] can carry over.
    fn install_at(&mut self, built: Menu, now: Instant) -> MenuUpdate {
        if built.shape() == self.built.shape() {
            self.deferred = None;
            let menu = built.clone().renumbered(self.first_id);
            let changed = changes_between(&self.menu, &menu);
            self.built = built;
            self.menu = menu;
            return if changed.is_empty() {
                MenuUpdate::Unchanged
            } else {
                MenuUpdate::Properties(changed)
            };
        }
        if self.submenus.busy(now) {
            if self.deferred.as_ref() == Some(&built) {
                return MenuUpdate::Unchanged;
            }
            let shown = self.menu.showing(&built);
            let changed = changes_between(&self.menu, &shown);
            self.menu = shown;
            self.deferred = Some(built);
            return MenuUpdate::Deferred(changed);
        }
        self.deferred = None;
        self.replace_layout(built)
    }

    /// The held layout, once no submenu counts as open.
    fn release_at(&mut self, now: Instant) -> MenuUpdate {
        if self.submenus.busy(now) {
            return MenuUpdate::Unchanged;
        }
        match self.deferred.take() {
            Some(built) => self.replace_layout(built),
            None => MenuUpdate::Unchanged,
        }
    }

    /// A new layout under ids the host has never seen.
    fn replace_layout(&mut self, built: Menu) -> MenuUpdate {
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

    /// Whether the item under an id can be chosen. An item greyed out while
    /// a layout waits (its action no longer applies) cannot.
    fn enabled(&self, id: i32) -> bool {
        self.menu
            .find(id)
            .or_else(|| self.retired.iter().find_map(|menu| menu.find(id)))
            .is_none_or(|item| item.enabled)
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

/// For two menus with the same shape, every id whose properties differ.
fn changes_between(old: &Menu, new: &Menu) -> Vec<(i32, HashMap<String, OwnedValue>)> {
    let mut changed = Vec::new();
    for (old, new) in old.items.iter().zip(&new.items) {
        changed_properties(old, new, &mut changed);
    }
    changed
}

/// Collects, for two items with the same shape, every id whose properties
/// differ, with only the properties that changed.
///
/// Never the whole set: the AppIndicator extension rebuilds a row it is sent
/// `type` or `children-display` for, even unchanged, and a rebuilt submenu
/// row is a closed one.
fn changed_properties(
    old: &MenuItem,
    new: &MenuItem,
    changed: &mut Vec<(i32, HashMap<String, OwnedValue>)>,
) {
    let before = all_properties(old);
    let properties: HashMap<String, OwnedValue> = all_properties(new)
        .into_iter()
        .filter(|(name, value)| before.get(name) != Some(value))
        .collect();
    if !properties.is_empty() {
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
                deferred: None,
                submenus: OpenSubmenus::default(),
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

    /// The host opened (`opened`) or closed a menu; `host` is the caller's
    /// bus name and `order` the D-Bus serial of its call (see
    /// [`OpenSubmenus`]). The whole menu (id 0)
    /// opening or closing sends a held layout at once. For a submenu it
    /// returns when the caller should call [`Self::release_layout`]: after
    /// [`SUBMENU_SETTLE`] for a close, after [`SUBMENU_LIMIT`] for an open.
    pub async fn menu_event(
        &self,
        id: i32,
        event: &str,
        host: &str,
        order: u64,
    ) -> Option<Duration> {
        let opened = match event {
            "opened" => true,
            "closed" => false,
            _ => return None,
        };
        {
            self.state
                .lock()
                .await
                .submenus
                .event(host, id, opened, order, Instant::now());
        }
        if id == 0 {
            self.release_layout().await;
            return None;
        }
        Some(if opened {
            SUBMENU_LIMIT
        } else {
            SUBMENU_SETTLE
        })
    }

    /// Sends a layout held while a submenu was open, if none counts as open
    /// now.
    pub async fn release_layout(&self) {
        let _announcing = self.announcing.lock().await;
        let update = { self.state.lock().await.release_at(Instant::now()) };
        self.announce_menu(update).await;
    }

    /// A new panel took the watcher's name: it has no submenu open, and it
    /// reads the layout afresh, so a held one goes out.
    pub async fn host_changed(&self) {
        {
            self.state.lock().await.submenus.reset();
        }
        self.release_layout().await;
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
        let (action, enabled) = {
            let state = self.state.lock().await;
            (state.action(item_id), state.enabled(item_id))
        };
        let Some(action) = action else {
            return Activation::Ignored;
        };
        // Greyed out because it no longer applies (the session it would act
        // on ended while a submenu was open). Said rather than dropped: a
        // click that does nothing visible is the failure the owner reported.
        let activation = if enabled {
            self.perform(action).await
        } else {
            Activation::Failed(ClientError::Rejected(NO_LONGER_APPLIES.to_string()))
        };
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
                // The session the menu offered to extend has ended.
                let Some(session) = manual else {
                    return Activation::Failed(ClientError::Rejected(
                        "awake.error.unknown_session".to_string(),
                    ));
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
            MenuUpdate::Properties(changed) | MenuUpdate::Deferred(changed)
                if changed.is_empty() =>
            {
                return;
            }
            MenuUpdate::Properties(changed) | MenuUpdate::Deferred(changed) => {
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

/// The refusal an item greyed out while a layout waits is clicked with.
pub const NO_LONGER_APPLIES: &str = "awake.tray.error.no_longer_applies";

/// What a refused action is said as, in plain words.
pub fn failure_text(error: &ClientError, labels: &'static Labels) -> &'static str {
    match error {
        ClientError::ServiceUnavailable(_) | ClientError::Transport(_) => {
            labels.failed_service_unavailable
        }
        ClientError::Protocol(_) => labels.failed_other,
        ClientError::Rejected(key) => {
            if key == NO_LONGER_APPLIES {
                labels.failed_no_longer_applies
            } else if key.contains("backend_unavailable") {
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
            deferred: None,
            submenus: OpenSubmenus::default(),
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
    fn a_wording_change_sends_only_the_changed_property() {
        let mut state = state(awake(3_600));
        let MenuUpdate::Properties(changed) = state.install(built(&awake(3_000))) else {
            panic!("a new countdown is not a new layout");
        };
        assert!(!changed.is_empty());
        for (id, properties) in &changed {
            let names: Vec<&str> = properties.keys().map(String::as_str).collect();
            // Never `type` or `children-display`: the AppIndicator extension
            // rebuilds a row it is sent either of, and a rebuilt submenu row
            // is a closed one.
            assert_eq!(names, ["label"], "item {id}");
        }
    }

    const HOST: &str = ":1.7";

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    fn extend_id(state: &State) -> i32 {
        state.menu.find_by_label("Extend session").unwrap().id
    }

    /// Awake with a second reason: rows are added above the submenus.
    fn awake_twice(remaining: u64) -> StatusDocument {
        let mut status = awake(remaining);
        status.reasons.push(WireReason {
            session_id: 9,
            origin: SessionOrigin::Manual,
            reason: "Backup".to_string(),
        });
        status
    }

    #[test]
    fn a_layout_change_while_a_submenu_is_open_waits_until_it_closes() {
        let start = Instant::now();
        let mut state = state(awake(3_600));
        let ids_before = ids(&state.menu);
        let extend = extend_id(&state);
        state.submenus.event(HOST, extend, true, 10, start);

        // The session ended: a different layout. The host keeps its rows,
        // and the ones that would act on the ended session are greyed out.
        let MenuUpdate::Deferred(changed) = state.install_at(built(&inactive()), at(start, 1_000))
        else {
            panic!("a layout change was not held");
        };
        assert_eq!(ids(&state.menu), ids_before);
        assert_eq!(state.revision, 1);
        let fifteen = state.menu.find_by_label("15 min").unwrap();
        assert!(!fifteen.enabled);
        assert!(!state.enabled(fifteen.id));
        let greyed = changed
            .iter()
            .find(|(id, _)| *id == fifteen.id)
            .expect("the greyed row is announced");
        assert_eq!(greyed.1.keys().collect::<Vec<_>>(), ["enabled"]);
        for (_, properties) in &changed {
            assert!(!properties.is_empty());
            assert!(!properties.contains_key("type"));
            assert!(!properties.contains_key("children-display"));
        }
        // The submenu's own row is not greyed under the pointer.
        assert!(state.menu.find(extend).unwrap().enabled);

        assert_eq!(
            state.install_at(built(&inactive()), at(start, 1_500)),
            MenuUpdate::Unchanged
        );
        assert_eq!(state.release_at(at(start, 2_000)), MenuUpdate::Unchanged);

        state
            .submenus
            .event(HOST, extend, false, 11, at(start, 3_000));
        assert_eq!(
            state.release_at(at(start, 3_100)),
            MenuUpdate::Unchanged,
            "a submenu that just closed may be followed by the next one opening"
        );
        assert_eq!(state.release_at(at(start, 3_300)), MenuUpdate::Layout(2));
        assert!(state.menu.find_by_label("Start a session").is_some());
        assert_eq!(state.release_at(at(start, 3_400)), MenuUpdate::Unchanged);
    }

    #[test]
    fn a_held_layout_still_updates_the_rows_that_line_up() {
        let start = Instant::now();
        let mut state = state(awake(3_600));
        let remaining = state.menu.find_by_label("Remaining: 1 hr").unwrap().id;
        state
            .submenus
            .event(HOST, extend_id(&state), true, 10, start);

        let MenuUpdate::Deferred(changed) = state.install_at(built(&awake_twice(3_000)), start)
        else {
            panic!("a second reason is a new layout");
        };
        assert_eq!(
            state.menu.find(remaining).unwrap().label,
            "Remaining: 50 min"
        );
        assert!(changed.iter().any(|(id, properties)| *id == remaining
            && properties.keys().collect::<Vec<_>>() == ["label"]));
        // The rows below the first that does not line up keep what they said.
        assert!(state.menu.find_by_label("Current settings").is_some());
        // Every action is still offered, so nothing is greyed out.
        assert!(state.menu.find_by_label("15 min").unwrap().enabled);
    }

    #[test]
    fn a_wording_change_still_goes_out_while_a_submenu_is_open() {
        let start = Instant::now();
        let mut state = state(awake(3_600));
        let extend = extend_id(&state);
        state.submenus.event(HOST, extend, true, 10, start);
        assert!(matches!(
            state.install_at(built(&inactive()), start),
            MenuUpdate::Deferred(_)
        ));
        // Back to the shape the host has, with a new countdown: that is sent
        // now, greyed rows come back, and the held layout is no longer true.
        let MenuUpdate::Properties(changed) = state.install_at(built(&awake(3_000)), start) else {
            panic!("the host's own shape is a change of properties");
        };
        let fifteen = state.menu.find_by_label("15 min").unwrap().id;
        assert!(state.enabled(fifteen));
        assert!(changed.iter().any(|(id, _)| *id == fifteen));
        state.submenus.event(HOST, extend, false, 11, start);
        assert_eq!(state.release_at(at(start, 1_000)), MenuUpdate::Unchanged);
    }

    #[test]
    fn the_whole_menu_opening_or_closing_means_no_submenu_is_open() {
        for opened in [true, false] {
            let start = Instant::now();
            let mut state = state(awake(3_600));
            state
                .submenus
                .event(HOST, extend_id(&state), true, 10, start);
            assert!(matches!(
                state.install_at(built(&inactive()), start),
                MenuUpdate::Deferred(_)
            ));
            state.submenus.event(HOST, 0, opened, 11, start);
            assert_eq!(state.release_at(start), MenuUpdate::Layout(2));
        }
    }

    #[test]
    fn the_next_submenu_opening_within_the_settle_time_keeps_the_layout_held() {
        let start = Instant::now();
        let mut state = state(awake(3_600));
        let extend = extend_id(&state);
        let settings = state.menu.find_by_label("Current settings").unwrap().id;
        state.submenus.event(HOST, extend, true, 10, start);
        assert!(matches!(
            state.install_at(built(&inactive()), start),
            MenuUpdate::Deferred(_)
        ));
        state
            .submenus
            .event(HOST, extend, false, 11, at(start, 1_000));
        state
            .submenus
            .event(HOST, settings, true, 12, at(start, 1_100));
        // The timer the close started fires.
        assert_eq!(state.release_at(at(start, 1_300)), MenuUpdate::Unchanged);
        assert!(state.submenus.busy(at(start, 1_300)));
        // A rebuild meanwhile is held too.
        assert!(matches!(
            state.install_at(built(&awake_twice(600)), at(start, 1_400)),
            MenuUpdate::Deferred(_)
        ));
    }

    #[test]
    fn events_handled_out_of_order_are_put_back_in_order() {
        let start = Instant::now();
        let mut open = OpenSubmenus::default();
        // `closed` (serial 21) handled before the `opened` (20) it followed.
        open.event(HOST, 5, false, 21, start);
        open.event(HOST, 5, true, 20, start);
        assert!(!open.busy(start), "a stale opened must not count");

        // The whole menu's `opened` (30) handled after a submenu's (31).
        let mut open = OpenSubmenus::default();
        open.event(HOST, 5, true, 31, start);
        open.event(HOST, 0, true, 30, start);
        assert!(
            open.busy(start),
            "a stale whole-menu event must not clear it"
        );
        open.event(HOST, 0, false, 32, start);
        assert!(!open.busy(start));
    }

    #[test]
    fn a_submenu_that_never_says_it_closed_stops_counting_after_the_limit() {
        let start = Instant::now();
        let mut open = OpenSubmenus::default();
        open.event(HOST, 5, true, 1, start);
        assert!(open.busy(start + SUBMENU_LIMIT - Duration::from_millis(1)));
        assert!(!open.busy(start + SUBMENU_LIMIT));
        // Opening it again starts the limit again.
        open.event(HOST, 5, true, 2, start + SUBMENU_LIMIT);
        assert!(open.busy(start + SUBMENU_LIMIT + Duration::from_secs(1)));
    }

    #[test]
    fn a_new_host_starts_with_nothing_open() {
        let start = Instant::now();
        let mut open = OpenSubmenus::default();
        open.event(HOST, 5, true, 40, start);
        // The panel restarted: its serials start low again.
        open.event(":1.90", 6, true, 2, start);
        open.event(":1.90", 6, false, 3, start);
        assert!(!open.busy(start + SUBMENU_SETTLE));
        let mut open = OpenSubmenus::default();
        open.event(HOST, 5, true, 40, start);
        open.reset();
        assert!(!open.busy(start));
    }

    #[test]
    fn a_greyed_out_choice_is_refused_in_plain_words() {
        let labels = Locale::EnUs.labels();
        assert_eq!(
            failure_text(
                &ClientError::Rejected(NO_LONGER_APPLIES.to_string()),
                labels
            ),
            labels.failed_no_longer_applies
        );
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
