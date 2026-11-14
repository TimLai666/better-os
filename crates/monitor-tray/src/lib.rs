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
    pub menu: Menu,
    pub revision: u32,
    pub launcher: PopupLauncher,
    pub service: ServiceState,
    /// The tooltip as last announced, so a change can be announced on its own.
    pub tooltip: (String, String),
}

/// What changed when new readings arrived, so only that is announced.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Changes {
    pub icon: bool,
    pub menu: bool,
    pub tooltip: bool,
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
    pub fn redraw(&mut self) -> Changes {
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
        let changes = Changes {
            icon: icon != self.icon,
            menu: menu != self.menu,
            tooltip: tooltip != self.tooltip,
        };
        if changes.tooltip {
            self.tooltip = tooltip;
        }
        if changes.icon {
            self.icon = icon;
        }
        if changes.menu {
            self.menu = menu;
            self.revision = self.revision.wrapping_add(1);
        }
        changes
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
        if changes.menu
            && let Ok(emitter) = SignalEmitter::new(&connection, MENU_PATH)
        {
            let revision = self.state.lock().await.revision;
            let _ = DbusMenu::layout_updated(&emitter, revision, 0).await;
        }
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

    /// Only `clicked` does anything; hover and open events change nothing.
    async fn event(
        &self,
        id: i32,
        event_id: String,
        _data: Value<'_>,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        if event_id == "clicked" {
            self.tray.activate_menu(id).await;
        }
        Ok(())
    }

    async fn about_to_show(&self, _id: i32) -> bool {
        false
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
