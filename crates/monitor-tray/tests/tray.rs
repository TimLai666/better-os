//! The taskbar item on a private session bus: a fake watcher, the item and
//! its menu published the way the binary publishes them, and a panel's view
//! of them read back over the bus. Nothing here touches the developer's own
//! session bus or desktop.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use monitor_tray::menu::{IconChoice, Locale, TrayReadings};
use monitor_tray::{
    DbusMenu, ITEM_PATH, MENU_PATH, StatusNotifierItem, Tray, TrayAvailability, TrayState,
    register_and_verify,
};
use zbus::interface;
use zbus::zvariant::OwnedValue;

struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        use std::io::{BufRead, BufReader};
        let stdout = child.stdout.take()?;
        let mut address = String::new();
        BufReader::new(stdout).read_line(&mut address).ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
    }

    async fn connect(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A watcher with a host, as swaybar or the GNOME AppIndicator extension
/// provides.
struct FakeWatcher {
    items: Arc<Mutex<Vec<String>>>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl FakeWatcher {
    async fn register_status_notifier_item(&self, service: &str) {
        self.items.lock().unwrap().push(service.to_string());
    }

    #[zbus(property)]
    async fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }

    #[zbus(property)]
    async fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn protocol_version(&self) -> i32 {
        0
    }
}

/// What a panel reads: pixmaps, a tooltip, and a menu node.
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;
type ToolTip = (String, Pixmaps, String, String);
type Node = (
    i32,
    std::collections::HashMap<String, OwnedValue>,
    Vec<OwnedValue>,
);

#[zbus::proxy(
    interface = "org.kde.StatusNotifierItem",
    default_path = "/StatusNotifierItem"
)]
trait Item {
    #[zbus(property)]
    fn icon_pixmap(&self) -> zbus::Result<Pixmaps>;
    #[zbus(property)]
    fn item_is_menu(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn tool_tip(&self) -> zbus::Result<ToolTip>;
}

#[zbus::proxy(interface = "com.canonical.dbusmenu", default_path = "/MenuBar")]
trait Menu {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: &[&str],
    ) -> zbus::Result<(u32, Node)>;
}

macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None => {
                eprintln!("skipped: dbus-daemon is not available");
                return;
            }
        }
    };
}

fn readings() -> TrayReadings {
    TrayReadings {
        cpu: Some(0.31),
        memory_used: Some(8.0 * 1024.0 * 1024.0 * 1024.0),
        memory_total: Some(16.0 * 1024.0 * 1024.0 * 1024.0),
        ..TrayReadings::default()
    }
}

#[tokio::test]
async fn the_item_registers_and_a_panel_reads_its_live_icon_and_menu() {
    let bus = bus_or_skip!();
    let watcher_connection = bus.connect().await;
    watcher_connection
        .object_server()
        .at(
            "/StatusNotifierWatcher",
            FakeWatcher {
                items: Arc::default(),
            },
        )
        .await
        .unwrap();
    watcher_connection
        .request_name("org.kde.StatusNotifierWatcher")
        .await
        .unwrap();

    let connection = bus.connect().await;
    let tray = Tray::new(TrayState::new(Locale::English, IconChoice::default(), None));
    connection
        .object_server()
        .at(ITEM_PATH, StatusNotifierItem::new(tray.clone()))
        .await
        .unwrap();
    connection
        .object_server()
        .at(MENU_PATH, DbusMenu::new(tray.clone()))
        .await
        .unwrap();
    tray.attach(connection.clone()).await;
    let unique = connection.unique_name().unwrap().to_string();
    assert_eq!(
        register_and_verify(&connection, &unique).await,
        TrayAvailability::Registered
    );

    let changes = tray.state.lock().await.adopt(Some(readings()));
    tray.announce(changes).await;

    let panel = bus.connect().await;
    let item = ItemProxy::builder(&panel)
        .destination(unique.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let pixmaps = item.icon_pixmap().await.unwrap();
    assert_eq!(pixmaps.len(), 2);
    let (width, height, pixels) = &pixmaps[0];
    assert_eq!(*height, 22);
    assert_eq!(pixels.len(), (*width * *height * 4) as usize);
    assert!(
        pixels.chunks(4).any(|pixel| pixel[0] == 255),
        "something is drawn"
    );
    assert!(
        !item.item_is_menu().await.unwrap(),
        "a left click opens the popup"
    );
    let (_, _, title, body) = item.tool_tip().await.unwrap();
    assert_eq!(title, "Better Monitor");
    assert!(body.contains("Processor: 31%"), "{body}");

    let menu = MenuProxy::builder(&panel)
        .destination(unique.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let (_, (root, _, children)) = menu.get_layout(0, -1, &[]).await.unwrap();
    assert_eq!(root, 0);
    let text = format!("{children:?}");
    assert!(text.contains("Processor: 31%"), "{text}");
    assert!(text.contains("Memory: 8.0 GB of 16.0 GB (50%)"), "{text}");
    assert!(text.contains("Show in the taskbar"), "{text}");
}

#[tokio::test]
async fn without_a_watcher_the_item_says_so_rather_than_claiming_to_show() {
    let bus = bus_or_skip!();
    let connection = bus.connect().await;
    let unique = connection.unique_name().unwrap().to_string();
    assert_eq!(
        register_and_verify(&connection, &unique).await,
        TrayAvailability::NoWatcher
    );
}

/// Publish a fake watcher on its own connection; dropping the connection is
/// the panel going away.
async fn watcher(bus: &PrivateBus) -> (zbus::Connection, Arc<Mutex<Vec<String>>>) {
    let items: Arc<Mutex<Vec<String>>> = Arc::default();
    let connection = bus.connect().await;
    connection
        .object_server()
        .at(
            "/StatusNotifierWatcher",
            FakeWatcher {
                items: items.clone(),
            },
        )
        .await
        .unwrap();
    connection
        .request_name("org.kde.StatusNotifierWatcher")
        .await
        .unwrap();
    (connection, items)
}

async fn until_registered(items: &Arc<Mutex<Vec<String>>>, name: &str) -> bool {
    for _ in 0..100 {
        if items.lock().unwrap().iter().any(|item| item == name) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn an_item_started_before_the_panel_registers_when_the_panel_appears_and_again_after_it_restarts()
 {
    let bus = bus_or_skip!();
    let connection = bus.connect().await;
    let unique = connection.unique_name().unwrap().to_string();
    let seen: Arc<Mutex<Vec<TrayAvailability>>> = Arc::default();
    let record = seen.clone();
    let watch = monitor_tray::keep_registered(connection.clone(), unique.clone(), move |a| {
        record.lock().unwrap().push(a)
    })
    .await
    .unwrap();
    let task = tokio::spawn(watch);

    // At login the panel is not there yet: the item says so and keeps going.
    assert_eq!(
        register_and_verify(&connection, &unique).await,
        TrayAvailability::NoWatcher
    );

    // The panel comes up.
    let (panel, items) = watcher(&bus).await;
    assert!(
        until_registered(&items, &unique).await,
        "the late panel never got the item"
    );

    // The panel restarts: a new watcher that has never heard of the item.
    drop(panel);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (_panel, items) = watcher(&bus).await;
    assert!(
        until_registered(&items, &unique).await,
        "the restarted panel never got the item"
    );

    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .filter(|a| **a == TrayAvailability::Registered)
            .count()
            >= 2
    );
    task.abort();
}
