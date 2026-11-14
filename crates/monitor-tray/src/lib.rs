//! Better Monitor in the taskbar: a StatusNotifierItem whose icon is drawn
//! from the live readings, a right-click menu that lists them, and a click
//! that opens the compact popup.
//!
//! The item is a client of `better-monitor-service` and nothing else. It asks
//! the service for a summary once a second (the busiest processes are left
//! out, because the item names none), redraws the icon only when the drawing
//! changed, and keeps no history of its own. Asking for the service starts it
//! through D-Bus activation when it is not running; when it cannot be reached
//! the item stays, says so in its menu, and asks again.
//!
//! The StatusNotifierItem and dbusmenu interfaces are written directly on
//! zbus, the same way Better Awake's tray does (see its `sni` module for why
//! not `ksni`), and registration is verified against the watcher's own list,
//! never assumed.

pub mod icon;
pub mod menu;
pub mod popup;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Type, Value};

#[cfg(test)]
use crate::icon::IconStyle;
use crate::icon::Pixmap;
use crate::menu::{
    IconChoice, ItemKind, Locale, Menu, MenuAction, MenuItem, ServiceState, TrayReadings,
};
use crate::popup::PopupLauncher;

pub const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
pub const ITEM_PATH: &str = "/StatusNotifierItem";
pub const MENU_PATH: &str = "/MenuBar";

/// Everything the item shows, behind one lock.
pub struct TrayState {
    pub locale: Locale,
    pub readings: Option<TrayReadings>,
    pub choice: IconChoice,
    pub choice_path: Option<PathBuf>,
    pub icon: Vec<Pixmap>,
    /// The menu as the panel was last told about it.
    pub menu: Menu,
    /// A menu with a different layout, held back while the panel has a
    /// submenu open (see [`TrayState::redraw`]).
    pub pending: Option<Menu>,
    /// The submenus the panel says are open.
    pub submenus: OpenSubmenus,
    pub revision: u32,
    pub launcher: PopupLauncher,
    pub service: ServiceState,
    /// The tooltip as last announced, so a change can be announced on its own.
    pub tooltip: (String, String),
}

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

/// What changed when new readings arrived, so only that is announced.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Changes {
    pub icon: bool,
    /// Anything in the menu the panel shows changed.
    pub menu: bool,
    pub tooltip: bool,
    /// Items were added, removed or moved: `LayoutUpdated`.
    pub layout: bool,
    /// Items the panel already has, each with the names of the properties
    /// that changed: `ItemsPropertiesUpdated`.
    pub properties: Vec<(i32, Vec<String>)>,
}

impl TrayState {
    pub fn new(locale: Locale, choice: IconChoice, choice_path: Option<PathBuf>) -> Self {
        let mut state = Self {
            locale,
            readings: None,
            choice,
            choice_path,
            icon: Vec::new(),
            menu: Menu::default(),
            pending: None,
            submenus: OpenSubmenus::default(),
            revision: 1,
            launcher: PopupLauncher::new(),
            service: ServiceState::Answering,
            tooltip: (String::new(), String::new()),
        };
        state.redraw();
        state
    }

    /// Redraw from the current readings and choice. The icon and the menu
    /// are replaced only when they differ, which is what keeps an idle
    /// machine from repainting the panel every second.
    ///
    /// A new reading keeps every item where it was, so it is announced as a
    /// change of properties. The GNOME AppIndicator extension rebuilds each
    /// row it knows on `LayoutUpdated`, which folds an open submenu up under
    /// the pointer; so a change of layout (a reading that appears or goes
    /// away) waits while the panel has a submenu open. Until then the panel
    /// keeps its rows and they show the new values, and the layout follows
    /// at the first redraw after the submenu closes.
    pub fn redraw(&mut self) -> Changes {
        self.redraw_at(Instant::now())
    }

    /// [`Self::redraw`] at a given moment, which decides whether a submenu
    /// still counts as open (see [`OpenSubmenus::busy`]).
    pub fn redraw_at(&mut self, now: Instant) -> Changes {
        let empty = TrayReadings::default();
        let readings = self.readings.as_ref().unwrap_or(&empty);
        let icon = icon::draw(&self.choice.shown, self.choice.style, readings);
        let menu = menu::build(
            self.readings.as_ref(),
            &self.choice,
            self.locale,
            self.service,
        );
        let tooltip = menu::tooltip(self.readings.as_ref(), self.locale, self.service);
        let mut changes = self.install(menu, now);
        changes.icon = icon != self.icon;
        changes.tooltip = tooltip != self.tooltip;
        if changes.tooltip {
            self.tooltip = tooltip;
        }
        if changes.icon {
            self.icon = icon;
        }
        changes
    }

    /// Puts a freshly built menu in front of the panel, or holds its layout
    /// back while a submenu is open, and says what the panel must be told.
    fn install(&mut self, built: Menu, now: Instant) -> Changes {
        if !self.submenus.busy(now) || built.skeleton() == self.menu.skeleton() {
            self.pending = None;
            return self.replace(built);
        }
        let showing = self.menu.showing(&built);
        self.pending = Some(built);
        self.replace(showing)
    }

    /// Replaces the menu the panel is shown.
    fn replace(&mut self, menu: Menu) -> Changes {
        if menu == self.menu {
            return Changes::default();
        }
        let layout = menu.skeleton() != self.menu.skeleton();
        let properties = changed_properties(&self.menu, &menu);
        self.menu = menu;
        if layout {
            self.revision = self.revision.wrapping_add(1);
        }
        Changes {
            menu: true,
            layout,
            properties,
            ..Changes::default()
        }
    }

    /// The panel opened (`opened`) or closed a menu: id 0 is the whole
    /// menu, any other id a submenu; `host` and `order` are the caller's bus
    /// name and the call's serial (see [`OpenSubmenus`]). The whole menu
    /// opening or closing means no submenu is open, so a held layout is put
    /// in place at once. A submenu closing releases nothing at once: the
    /// panel closes one submenu just before opening the next, so a held
    /// layout waits for the first redraw after [`SUBMENU_SETTLE`].
    pub fn menu_event(
        &mut self,
        host: &str,
        id: i32,
        opened: bool,
        order: u64,
        now: Instant,
    ) -> Changes {
        self.submenus.event(host, id, opened, order, now);
        if id == 0 {
            return self.release(now);
        }
        Changes::default()
    }

    /// A new panel took the watcher's name: nothing of ours is open in it.
    pub fn host_changed(&mut self, now: Instant) -> Changes {
        self.submenus.reset();
        self.release(now)
    }

    /// Puts a held layout in place when no submenu counts as open.
    fn release(&mut self, now: Instant) -> Changes {
        match self.pending.take() {
            Some(built) if !self.submenus.busy(now) => self.replace(built),
            held => {
                self.pending = held;
                Changes::default()
            }
        }
    }

    pub fn adopt(&mut self, readings: Option<TrayReadings>) -> Changes {
        self.readings = readings;
        self.redraw()
    }

    /// Adopt what the service said, or why it said nothing. Returns the
    /// changes and whether the service's state is new, so a caller logs a
    /// failure once rather than every few seconds.
    pub fn adopt_from(&mut self, answer: Result<TrayReadings, ServiceState>) -> (Changes, bool) {
        let (readings, service) = match answer {
            Ok(readings) => (Some(readings), ServiceState::Answering),
            Err(state) => (None, state),
        };
        let changed = service != self.service;
        self.service = service;
        (self.adopt(readings), changed)
    }

    /// Apply a choice from the menu and remember it.
    pub fn choose(&mut self, action: MenuAction) -> Changes {
        match action {
            MenuAction::Toggle(reading) => self.choice.toggle(reading),
            MenuAction::Style(style) => self.choice.style = style,
            _ => return Changes::default(),
        }
        if let Some(path) = &self.choice_path
            && let Err(error) = self.choice.save(path)
        {
            eprintln!("better-monitor-tray: the icon choice could not be saved: {error}");
        }
        self.redraw()
    }
}

/// The items both menus have whose properties differ, each with the names
/// of the properties that changed. Only those names are sent: the GNOME
/// AppIndicator extension rebuilds a row when it is sent `type` or
/// `children-display`, even unchanged, and a rebuilt submenu row is a
/// closed one.
fn changed_properties(old: &Menu, new: &Menu) -> Vec<(i32, Vec<String>)> {
    let mut changed = Vec::new();
    for id in new.skeleton().into_iter().map(|(id, _)| id) {
        let (Some(before), Some(after)) = (old.find(id), new.find(id)) else {
            continue;
        };
        let before = properties(before, &[]);
        let mut names: Vec<String> = properties(after, &[])
            .into_iter()
            .filter(|(name, value)| before.get(name) != Some(value))
            .map(|(name, _)| name)
            .collect();
        if !names.is_empty() {
            names.sort();
            changed.push((id, names));
        }
    }
    changed
}

/// The shared state and what the item does with a click or a menu event.
pub struct Tray {
    pub state: Mutex<TrayState>,
    pub quit: tokio::sync::Notify,
    connection: Mutex<Option<zbus::Connection>>,
}

impl Tray {
    pub fn new(state: TrayState) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
            quit: tokio::sync::Notify::new(),
            connection: Mutex::new(None),
        })
    }

    pub async fn attach(&self, connection: zbus::Connection) {
        *self.connection.lock().await = Some(connection);
    }

    /// Announce what changed to the panel.
    pub async fn announce(&self, changes: Changes) {
        let Some(connection) = self.connection.lock().await.clone() else {
            return;
        };
        if (changes.icon || changes.tooltip)
            && let Ok(emitter) = SignalEmitter::new(&connection, ITEM_PATH)
        {
            if changes.icon {
                let _ = StatusNotifierItem::new_icon(&emitter).await;
            }
            if changes.tooltip {
                let _ = StatusNotifierItem::new_tool_tip(&emitter).await;
            }
        }
        if (changes.layout || !changes.properties.is_empty())
            && let Ok(emitter) = SignalEmitter::new(&connection, MENU_PATH)
        {
            // Read under the lock together, so the values sent are the ones
            // a `GetLayout` at this revision would return.
            let (revision, updated) = {
                let state = self.state.lock().await;
                let updated: Vec<(i32, HashMap<String, OwnedValue>)> = changes
                    .properties
                    .iter()
                    .filter_map(|(id, names)| {
                        state
                            .menu
                            .find(*id)
                            .map(|item| (*id, properties(item, names)))
                    })
                    .filter(|(_, values)| !values.is_empty())
                    .collect();
                (state.revision, updated)
            };
            if !updated.is_empty() {
                let _ = DbusMenu::items_properties_updated(&emitter, updated, Vec::new()).await;
            }
            if changes.layout {
                let _ = DbusMenu::layout_updated(&emitter, revision, 0).await;
            }
        }
    }

    /// The panel opened (`opened`) or closed a menu; see
    /// [`TrayState::menu_event`]. Returns whether a held layout went out,
    /// which `AboutToShow` answers with.
    pub async fn menu_event(&self, id: i32, event: &str, host: &str, order: u64) -> bool {
        let opened = match event {
            "opened" => true,
            "closed" => false,
            _ => return false,
        };
        let changes = self
            .state
            .lock()
            .await
            .menu_event(host, id, opened, order, Instant::now());
        let layout = changes.layout;
        self.announce(changes).await;
        layout
    }

    /// A panel started or restarted; see [`TrayState::host_changed`].
    pub async fn host_changed(&self) {
        let changes = self.state.lock().await.host_changed(Instant::now());
        self.announce(changes).await;
    }

    pub async fn activate_menu(&self, id: i32) {
        let action = {
            let state = self.state.lock().await;
            state.menu.find(id).and_then(|item| item.action)
        };
        let Some(action) = action else { return };
        match action {
            MenuAction::OpenWindow => {
                if let Err(error) = self.state.lock().await.launcher.open_window() {
                    eprintln!("better-monitor-tray: the window could not be opened: {error}");
                }
            }
            MenuAction::OpenPopup => self.toggle_popup(0, 0).await,
            MenuAction::Quit => self.quit.notify_one(),
            choice => {
                let changes = self.state.lock().await.choose(choice);
                self.announce(changes).await;
            }
        }
    }

    pub async fn toggle_popup(&self, x: i32, y: i32) {
        if let Err(error) = self.state.lock().await.launcher.toggle(x, y) {
            eprintln!("better-monitor-tray: the popup could not be opened: {error}");
        }
    }
}

/// Serves the item and its menu on a new connection, then attaches the tray.
///
/// Both interfaces are added through the connection builder, so `build` waits
/// for the object server task to subscribe before this returns. The caller
/// requests the well-known name afterwards, which means the panel's first call
/// after the name appears is dispatched rather than dropped.
pub async fn serve(
    builder: zbus::connection::Builder<'_>,
    tray: Arc<Tray>,
) -> zbus::Result<zbus::Connection> {
    let connection = builder
        .serve_at(ITEM_PATH, StatusNotifierItem::new(tray.clone()))?
        .serve_at(MENU_PATH, DbusMenu::new(tray.clone()))?
        .build()
        .await?;
    tray.attach(connection.clone()).await;
    Ok(connection)
}

/// `org.kde.StatusNotifierItem`.
pub struct StatusNotifierItem {
    tray: Arc<Tray>,
}

impl StatusNotifierItem {
    pub fn new(tray: Arc<Tray>) -> Self {
        Self { tray }
    }
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    /// A left click opens or closes the popup at the pointer.
    async fn activate(&self, x: i32, y: i32) {
        self.tray.toggle_popup(x, y).await;
    }

    /// A middle click opens the full window.
    async fn secondary_activate(&self, _x: i32, _y: i32) {
        if let Err(error) = self.tray.state.lock().await.launcher.open_window() {
            eprintln!("better-monitor-tray: the window could not be opened: {error}");
        }
    }

    async fn context_menu(&self, _x: i32, _y: i32) {}

    async fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(property)]
    async fn category(&self) -> &str {
        "Hardware"
    }

    #[zbus(property)]
    async fn id(&self) -> &str {
        "better-monitor"
    }

    #[zbus(property)]
    async fn title(&self) -> String {
        self.tray.state.lock().await.locale.app_name().to_string()
    }

    #[zbus(property)]
    async fn status(&self) -> &str {
        "Active"
    }

    /// Empty, so the panel draws the pixmap.
    #[zbus(property)]
    async fn icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    async fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        self.tray.state.lock().await.icon.clone()
    }

    #[zbus(property)]
    async fn tool_tip(&self) -> (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
        let state = self.tray.state.lock().await;
        let (title, body) = state.tooltip.clone();
        (String::new(), Vec::new(), title, body)
    }

    /// False: a left click is the popup, and the menu is the right click.
    #[zbus(property)]
    async fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn menu(&self) -> OwnedObjectPath {
        ObjectPath::try_from(MENU_PATH)
            .expect("the menu path is a constant and is valid")
            .into()
    }

    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// `(ia{sv}av)`: an id, its properties, and its children as variants.
#[derive(Debug, serde::Serialize, Type)]
pub struct LayoutNode(
    pub i32,
    pub HashMap<String, OwnedValue>,
    pub Vec<OwnedValue>,
);

/// The properties one item exposes, filtered to what the host asked for.
pub fn properties(item: &MenuItem, wanted: &[String]) -> HashMap<String, OwnedValue> {
    let mut properties: Vec<(&str, Value<'static>)> = Vec::new();
    match &item.kind {
        ItemKind::Separator => properties.push(("type", Value::from("separator"))),
        ItemKind::Checkmark { checked } | ItemKind::Radio { checked } => {
            properties.push(("label", Value::from(item.label.clone())));
            let toggle = if matches!(item.kind, ItemKind::Radio { .. }) {
                "radio"
            } else {
                "checkmark"
            };
            properties.push(("toggle-type", Value::from(toggle)));
            properties.push(("toggle-state", Value::from(i32::from(*checked))));
        }
        ItemKind::Submenu => {
            properties.push(("label", Value::from(item.label.clone())));
            properties.push(("children-display", Value::from("submenu")));
        }
        ItemKind::Standard | ItemKind::Info => {
            properties.push(("label", Value::from(item.label.clone())));
        }
    }
    properties.push(("enabled", Value::from(item.enabled)));
    properties.push(("visible", Value::from(true)));
    properties
        .into_iter()
        .filter(|(name, _)| wanted.is_empty() || wanted.iter().any(|asked| asked == name))
        .filter_map(|(name, value)| {
            OwnedValue::try_from(value)
                .ok()
                .map(|value| (name.to_string(), value))
        })
        .collect()
}

fn node(item: &MenuItem, depth: i32, wanted: &[String]) -> Option<Value<'static>> {
    let children: Vec<OwnedValue> = if depth == 0 {
        Vec::new()
    } else {
        item.children
            .iter()
            .filter_map(|child| node(child, depth - 1, wanted))
            .filter_map(|child| OwnedValue::try_from(child).ok())
            .collect()
    };
    zbus::zvariant::StructureBuilder::new()
        .add_field(item.id)
        .add_field(properties(item, wanted))
        .add_field(children)
        .build()
        .ok()
        .map(Value::from)
}

/// The subtree under one item; id 0 is the root.
pub fn layout(menu: &Menu, parent_id: i32, depth: i32, wanted: &[String]) -> LayoutNode {
    let (items, properties_of_parent) = if parent_id == 0 {
        let mut root = HashMap::new();
        if let Ok(value) = OwnedValue::try_from(Value::from("submenu")) {
            root.insert("children-display".to_string(), value);
        }
        (menu.items.as_slice(), root)
    } else {
        match menu.find(parent_id) {
            Some(item) => (item.children.as_slice(), properties(item, wanted)),
            None => return LayoutNode(parent_id, HashMap::new(), Vec::new()),
        }
    };
    let children = if depth == 0 {
        Vec::new()
    } else {
        items
            .iter()
            .filter_map(|item| node(item, depth - 1, wanted))
            .filter_map(|item| OwnedValue::try_from(item).ok())
            .collect()
    };
    LayoutNode(parent_id, properties_of_parent, children)
}

/// Who called and where the call stands among that caller's calls: its bus
/// name and its D-Bus serial, which only grows on one connection. See
/// [`OpenSubmenus`].
fn order_of(header: &zbus::message::Header<'_>) -> (String, u64) {
    (
        header
            .sender()
            .map(|sender| sender.to_string())
            .unwrap_or_default(),
        u64::from(header.primary().serial_num().get()),
    )
}

/// `com.canonical.dbusmenu`.
pub struct DbusMenu {
    tray: Arc<Tray>,
}

impl DbusMenu {
    pub fn new(tray: Arc<Tray>) -> Self {
        Self { tray }
    }
}

#[interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    async fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> (u32, LayoutNode) {
        let state = self.tray.state.lock().await;
        let depth = if recursion_depth < 0 {
            i32::MAX
        } else {
            recursion_depth
        };
        (
            state.revision,
            layout(&state.menu, parent_id, depth, &property_names),
        )
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let state = self.tray.state.lock().await;
        ids.into_iter()
            .filter_map(|id| {
                state
                    .menu
                    .find(id)
                    .map(|item| (id, properties(item, &property_names)))
            })
            .collect()
    }

    async fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let state = self.tray.state.lock().await;
        state
            .menu
            .find(id)
            .and_then(|item| properties(item, std::slice::from_ref(&name)).remove(&name))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no property {name} on {id}")))
    }

    /// `clicked` runs the item. `opened` and `closed` say which submenus
    /// the panel has open, which decides when a new layout may be sent.
    async fn event(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        id: i32,
        event_id: String,
        _data: Value<'_>,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        match event_id.as_str() {
            "clicked" => self.tray.activate_menu(id).await,
            "opened" | "closed" => {
                let (host, order) = order_of(&header);
                self.tray.menu_event(id, &event_id, &host, order).await;
            }
            _ => {}
        }
        Ok(())
    }

    /// The whole menu about to open puts a held layout in place, and the
    /// answer says whether the panel must read it again. A submenu asking
    /// changes nothing.
    async fn about_to_show(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        id: i32,
    ) -> bool {
        if id != 0 {
            return false;
        }
        let (host, order) = order_of(&header);
        self.tray.menu_event(0, "opened", &host, order).await
    }

    #[zbus(property)]
    async fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    async fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    async fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn items_properties_updated(
        emitter: &SignalEmitter<'_>,
        updated: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
pub trait StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;
}

/// What is actually true about the icon right now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrayAvailability {
    /// The watcher lists the item and a host draws it.
    Registered,
    /// No `org.kde.StatusNotifierWatcher`: on stock GNOME, the AppIndicator
    /// extension is missing.
    NoWatcher,
    NoHost,
    RegistrationFailed(String),
    NotListed,
}

/// Register the item and read the watcher's own list back.
pub async fn register_and_verify(connection: &zbus::Connection, name: &str) -> TrayAvailability {
    let dbus = match zbus::fdo::DBusProxy::new(connection).await {
        Ok(dbus) => dbus,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    };
    let watcher_name = match zbus::names::BusName::try_from(WATCHER_SERVICE) {
        Ok(name) => name,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    };
    match dbus.name_has_owner(watcher_name).await {
        Ok(true) => {}
        Ok(false) => return TrayAvailability::NoWatcher,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    }
    let watcher = match StatusNotifierWatcherProxy::new(connection).await {
        Ok(watcher) => watcher,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    };
    if let Err(error) = watcher.register_status_notifier_item(name).await {
        return TrayAvailability::RegistrationFailed(error.to_string());
    }
    match watcher.registered_status_notifier_items().await {
        Ok(items) if items.iter().any(|item| item.starts_with(name)) => {}
        Ok(_) => return TrayAvailability::NotListed,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    }
    match watcher.is_status_notifier_host_registered().await {
        Ok(true) => TrayAvailability::Registered,
        Ok(false) => TrayAvailability::NoHost,
        Err(error) => TrayAvailability::RegistrationFailed(error.to_string()),
    }
}

/// Keep the item registered with whichever watcher the panel runs.
///
/// Subscribes to `NameOwnerChanged` for `org.kde.StatusNotifierWatcher` now,
/// and returns the future that registers the item each time the name gets an
/// owner: a watcher that appears after the item started (a panel that starts
/// later at login) or a new one after the panel restarted. Subscribing before
/// returning is what lets the caller try a first registration without a
/// watcher that appears in between being missed. The future ends only when
/// the bus does.
pub async fn keep_registered(
    connection: zbus::Connection,
    unique_name: String,
    mut report: impl FnMut(TrayAvailability) + Send + 'static,
) -> zbus::Result<impl std::future::Future<Output = ()> + Send + 'static> {
    use zbus::export::futures_core::Stream;
    let dbus = zbus::fdo::DBusProxy::new(&connection).await?;
    let changes = dbus
        .receive_name_owner_changed_with_args(&[(0, WATCHER_SERVICE)])
        .await?;
    Ok(async move {
        let mut changes = std::pin::pin!(changes);
        while let Some(change) =
            std::future::poll_fn(|context| changes.as_mut().poll_next(context)).await
        {
            let Ok(args) = change.args() else { continue };
            if args.new_owner().is_some() {
                report(register_and_verify(&connection, &unique_name).await);
            }
        }
    })
}

/// Where the window keeps its language choice, so the item speaks the same.
pub fn language_path() -> Option<PathBuf> {
    IconChoice::default_path().map(|path| path.with_file_name("language"))
}

/// Re-exported for the binary and the tests.
pub use crate::icon::IconStyle as Style;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Reading;

    fn state() -> TrayState {
        TrayState::new(Locale::English, IconChoice::default(), None)
    }

    #[test]
    fn the_layout_type_carries_the_signature_every_panel_expects() {
        assert_eq!(LayoutNode::SIGNATURE.to_string(), "(ia{sv}av)");
    }

    #[test]
    fn nothing_is_announced_when_nothing_changed() {
        let mut state = state();
        let readings = TrayReadings {
            cpu: Some(0.2),
            memory_used: Some(1.0),
            memory_total: Some(4.0),
            ..TrayReadings::default()
        };
        let first = state.adopt(Some(readings.clone()));
        assert!(first.icon && first.menu && first.tooltip);
        assert_eq!(state.adopt(Some(readings)), Changes::default());
    }

    #[test]
    fn a_tooltip_change_is_announced_without_redrawing_the_icon() {
        let mut state = state();
        let readings = TrayReadings {
            cpu: Some(0.2),
            cpu_hz: Some(2.0e9),
            ..TrayReadings::default()
        };
        state.adopt(Some(readings.clone()));
        // Only the clock moved: the bar is the same, the text is not.
        let changes = state.adopt(Some(TrayReadings {
            cpu_hz: Some(2.4e9),
            ..readings
        }));
        assert!(changes.tooltip);
        assert!(!changes.icon);
    }

    #[test]
    fn a_service_failure_is_new_once() {
        let mut state = state();
        let (_, first) = state.adopt_from(Err(ServiceState::Unavailable));
        let (_, again) = state.adopt_from(Err(ServiceState::Unavailable));
        assert!(first);
        assert!(!again);
        let (changes, outdated) = state.adopt_from(Err(ServiceState::Outdated));
        assert!(outdated && changes.menu && changes.tooltip);
    }

    #[test]
    fn a_style_choice_is_a_radio_and_redraws_the_icon() {
        let mut state = state();
        state.adopt(Some(TrayReadings {
            cpu: Some(0.2),
            ..TrayReadings::default()
        }));
        let numbers = state
            .menu
            .find_by_label("As numbers (the first two)")
            .unwrap();
        let props = properties(numbers, &[]);
        assert_eq!(
            props["toggle-type"].downcast_ref::<&str>().unwrap(),
            "radio"
        );
        let changes = state.choose(MenuAction::Style(IconStyle::Numbers));
        assert!(changes.icon);
        assert_eq!(state.choice.style, IconStyle::Numbers);
        let changes = state.choose(MenuAction::Toggle(Reading::Gpu));
        assert!(changes.menu);
    }

    fn cpu(fraction: f64) -> TrayReadings {
        TrayReadings {
            cpu: Some(fraction),
            memory_used: Some(1.0),
            memory_total: Some(4.0),
            ..TrayReadings::default()
        }
    }

    fn show_id(state: &TrayState) -> i32 {
        state.menu.find_by_label("Show in the taskbar").unwrap().id
    }

    #[test]
    fn a_new_reading_is_a_change_of_properties_and_not_of_layout() {
        let mut state = state();
        state.adopt(Some(cpu(0.2)));
        let revision = state.revision;
        let line = state.menu.find_by_label("Processor: 20%").unwrap().id;

        let changes = state.adopt(Some(cpu(0.6)));
        assert!(changes.menu);
        assert!(!changes.layout);
        assert_eq!(state.revision, revision);
        assert_eq!(changes.properties, vec![(line, vec!["label".to_string()])]);
        assert_eq!(state.menu.find(line).unwrap().label, "Processor: 60%");
    }

    #[test]
    fn a_reading_that_appears_is_a_change_of_layout() {
        let mut state = state();
        state.adopt(Some(cpu(0.2)));
        let revision = state.revision;
        let changes = state.adopt(Some(TrayReadings {
            gpu: Some(0.5),
            ..cpu(0.2)
        }));
        assert!(changes.layout);
        assert_eq!(state.revision, revision + 1);
        assert!(state.menu.find_by_label("Graphics card: 50%").is_some());
    }

    const HOST: &str = ":1.7";

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    fn adopt_at(state: &mut TrayState, readings: TrayReadings, now: Instant) -> Changes {
        state.readings = Some(readings);
        state.redraw_at(now)
    }

    #[test]
    fn a_layout_change_while_a_submenu_is_open_waits_until_it_closes() {
        let start = Instant::now();
        let mut state = state();
        adopt_at(&mut state, cpu(0.2), start);
        let revision = state.revision;
        let show = show_id(&state);
        assert_eq!(
            state.menu_event(HOST, show, true, 10, start),
            Changes::default()
        );

        // The graphics reading appears and the processor moves. The rows the
        // panel has show the new processor value; the new row waits.
        let gpu = TrayReadings {
            gpu: Some(0.5),
            ..cpu(0.6)
        };
        let changes = adopt_at(&mut state, gpu.clone(), at(start, 1_000));
        assert!(!changes.layout, "{changes:?}");
        assert_eq!(state.revision, revision);
        assert!(state.menu.find_by_label("Processor: 60%").is_some());
        assert!(state.menu.find_by_label("Graphics card: 50%").is_none());
        // The graphics check box under the open submenu can be ticked now.
        let graphics = state.menu.find_by_label("Graphics card").unwrap().id;
        assert!(
            changes
                .properties
                .contains(&(graphics, vec!["enabled".to_string()]))
        );
        assert!(state.menu.find(graphics).unwrap().enabled);
        assert!(state.pending.is_some());

        // Closing it releases nothing at once: the next submenu may be
        // opening. A redraw within the settle time keeps it held too.
        assert_eq!(
            state.menu_event(HOST, show, false, 11, at(start, 2_000)),
            Changes::default()
        );
        assert!(!adopt_at(&mut state, gpu.clone(), at(start, 2_100)).layout);
        let changes = adopt_at(&mut state, gpu, at(start, 2_300));
        assert!(changes.layout);
        assert_eq!(state.revision, revision + 1);
        assert!(state.menu.find_by_label("Graphics card: 50%").is_some());
        assert!(state.pending.is_none());
    }

    #[test]
    fn the_next_submenu_opening_within_the_settle_time_keeps_the_layout_held() {
        let start = Instant::now();
        let mut state = state();
        adopt_at(&mut state, cpu(0.2), start);
        let show = show_id(&state);
        state.menu_event(HOST, show, true, 10, start);
        let gpu = TrayReadings {
            gpu: Some(0.5),
            ..cpu(0.2)
        };
        adopt_at(&mut state, gpu.clone(), start);
        state.menu_event(HOST, show, false, 11, at(start, 1_000));
        // A host with a second submenu opens it right after.
        state.menu_event(HOST, 99, true, 12, at(start, 1_100));
        assert!(!adopt_at(&mut state, gpu, at(start, 1_400)).layout);
        assert!(state.pending.is_some());
    }

    #[test]
    fn the_whole_menu_opening_or_closing_releases_a_held_layout() {
        for opened in [true, false] {
            let start = Instant::now();
            let mut state = state();
            adopt_at(&mut state, cpu(0.2), start);
            let show = show_id(&state);
            state.menu_event(HOST, show, true, 10, start);
            adopt_at(
                &mut state,
                TrayReadings {
                    gpu: Some(0.5),
                    ..cpu(0.2)
                },
                start,
            );
            let changes = state.menu_event(HOST, 0, opened, 11, start);
            assert!(changes.layout);
            assert!(!state.submenus.busy(start));
            assert!(state.menu.find_by_label("Graphics card: 50%").is_some());
        }
    }

    #[test]
    fn a_new_panel_releases_a_held_layout() {
        let start = Instant::now();
        let mut state = state();
        adopt_at(&mut state, cpu(0.2), start);
        state.menu_event(HOST, show_id(&state), true, 10, start);
        adopt_at(
            &mut state,
            TrayReadings {
                gpu: Some(0.5),
                ..cpu(0.2)
            },
            start,
        );
        assert!(state.host_changed(start).layout);
    }

    #[test]
    fn out_of_order_and_forgotten_events_do_not_hold_a_layout_forever() {
        let start = Instant::now();
        let mut open = OpenSubmenus::default();
        open.event(HOST, 5, false, 21, start);
        open.event(HOST, 5, true, 20, start);
        assert!(!open.busy(start), "a stale opened must not count");

        let mut open = OpenSubmenus::default();
        open.event(HOST, 5, true, 31, start);
        open.event(HOST, 0, true, 30, start);
        assert!(
            open.busy(start),
            "a stale whole-menu event must not clear it"
        );
        // Never closed: it stops counting after the limit.
        assert!(open.busy(start + SUBMENU_LIMIT - Duration::from_millis(1)));
        assert!(!open.busy(start + SUBMENU_LIMIT));

        // Another connection is another panel, whose serials start again.
        let mut open = OpenSubmenus::default();
        open.event(HOST, 5, true, 40, start);
        open.event(":1.90", 6, true, 2, start);
        open.event(":1.90", 6, false, 3, start);
        assert!(!open.busy(start + SUBMENU_SETTLE));
    }

    #[test]
    fn an_unchanged_submenu_row_is_never_sent_its_type_again() {
        let mut state = state();
        state.adopt(Some(cpu(0.2)));
        let show = show_id(&state);
        let graphics = state.menu.find_by_label("Graphics card").unwrap().id;
        let changes = state.choose(MenuAction::Toggle(Reading::Gpu));
        assert!(!changes.layout);
        assert!(!changes.properties.is_empty());
        assert!(
            changes
                .properties
                .contains(&(graphics, vec!["toggle-state".to_string()])),
            "{changes:?}"
        );
        for (id, names) in &changes.properties {
            assert_ne!(*id, show);
            assert!(!names.is_empty(), "{changes:?}");
            assert!(
                !names
                    .iter()
                    .any(|name| name == "type" || name == "children-display")
            );
        }
    }

    #[test]
    fn a_submenu_can_be_asked_for_on_its_own() {
        let state = state();
        let show = state.menu.find_by_label("Show in the taskbar").unwrap();
        let subtree = layout(&state.menu, show.id, i32::MAX, &[]);
        assert_eq!(subtree.0, show.id);
        assert_eq!(subtree.2.len(), show.children.len());
        let missing = layout(&state.menu, 9_999, i32::MAX, &[]);
        assert!(missing.2.is_empty());
    }
}
