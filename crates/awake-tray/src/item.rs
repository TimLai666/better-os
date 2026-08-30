//! `org.kde.StatusNotifierItem`, served at `/StatusNotifierItem`.
//!
//! The item itself is nearly stateless: it reports the icon, the status, and
//! the tooltip the controller derived from the service's status, and points the
//! panel at the menu object. Activation opens the same compact menu that a
//! right-click gives, because Issue #13 requires every essential action to be
//! reachable from one click on hosts that do not distinguish click types.

use std::sync::Arc;

use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use crate::controller::TrayController;
use crate::sni::{MENU_PATH, icon_name, item_status};

pub struct StatusNotifierItem {
    controller: Arc<TrayController>,
}

impl StatusNotifierItem {
    pub fn new(controller: Arc<TrayController>) -> Self {
        Self { controller }
    }
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    /// A left click. The panel shows the menu from the `Menu` property, so
    /// there is nothing to do here beyond not opening the full window.
    async fn activate(&self, _x: i32, _y: i32) {}

    /// A middle click. Issue #13 makes the middle-click toggle optional and
    /// configurable because hosts differ, and Phase 1 does not bind it: an
    /// action that silently starts or ends a session is worse than none.
    async fn secondary_activate(&self, _x: i32, _y: i32) {}

    async fn context_menu(&self, _x: i32, _y: i32) {}

    async fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(property)]
    async fn category(&self) -> &str {
        "SystemServices"
    }

    #[zbus(property)]
    async fn id(&self) -> &str {
        "better-awake"
    }

    #[zbus(property)]
    async fn title(&self) -> String {
        self.controller
            .locale()
            .labels()
            .application_name
            .to_string()
    }

    #[zbus(property)]
    async fn status(&self) -> String {
        item_status(self.controller.indicator().await).to_string()
    }

    #[zbus(property)]
    async fn icon_name(&self) -> String {
        icon_name(self.controller.indicator().await).to_string()
    }

    #[zbus(property)]
    async fn attention_icon_name(&self) -> String {
        icon_name(awake_ipc::WireIndicator::AttentionRequired).to_string()
    }

    /// `(s, s)` title and body, plus the empty icon name and pixmap array the
    /// interface's `(sa(iiay)ss)` signature requires.
    #[zbus(property)]
    async fn tool_tip(&self) -> (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
        let (title, body) = self.controller.tooltip().await;
        (String::new(), Vec::new(), title, body)
    }

    /// True so hosts that can render the menu directly do, rather than sending
    /// an Activate the tray would have to translate into a menu.
    #[zbus(property)]
    async fn item_is_menu(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn menu(&self) -> OwnedObjectPath {
        ObjectPath::try_from(MENU_PATH)
            .expect("the menu path is a constant and is valid")
            .into()
    }

    #[zbus(signal)]
    async fn new_icon(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_status(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        status: &str,
    ) -> zbus::Result<()>;
}
