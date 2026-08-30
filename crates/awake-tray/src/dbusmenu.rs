//! `com.canonical.dbusmenu`, served at `/MenuBar`.
//!
//! This is the wire form of the menu model, and nothing more. Every decision
//! about what the menu says lives in `menu.rs`, which is why the layouts can be
//! asserted without a bus.

use std::collections::HashMap;
use std::sync::Arc;

use zbus::interface;
use zbus::zvariant::{OwnedValue, Type, Value};

use crate::controller::TrayController;
use crate::menu::{ItemKind, Menu, MenuItem};

/// `(ia{sv}av)`: an id, its properties, and its children as variants. The
/// recursion is expressed through the variant array, so one type describes a
/// menu of any depth.
#[derive(Debug, serde::Serialize, Type)]
pub struct LayoutNode(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

pub struct DbusMenu {
    controller: Arc<TrayController>,
}

impl DbusMenu {
    pub fn new(controller: Arc<TrayController>) -> Self {
        Self { controller }
    }
}

/// The properties one item exposes, filtered to what the host asked for.
fn properties(item: &MenuItem, wanted: &[String]) -> HashMap<String, OwnedValue> {
    let mut properties: Vec<(&str, Value<'static>)> = Vec::new();

    match &item.kind {
        ItemKind::Separator => properties.push(("type", Value::from("separator"))),
        ItemKind::Checkmark { checked } => {
            properties.push(("label", Value::from(item.label.clone())));
            properties.push(("toggle-type", Value::from("checkmark")));
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

/// One item and, unless the depth ran out, its children.
fn node(item: &MenuItem, depth: i32, wanted: &[String]) -> Option<Value<'static>> {
    let children = if depth == 0 {
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

/// The whole menu under the dbusmenu root, which is always id 0.
pub fn layout(menu: &Menu, depth: i32, wanted: &[String]) -> LayoutNode {
    let children = if depth == 0 {
        Vec::new()
    } else {
        menu.items
            .iter()
            .filter_map(|item| node(item, depth - 1, wanted))
            .filter_map(|item| OwnedValue::try_from(item).ok())
            .collect()
    };
    let mut root = HashMap::new();
    if let Ok(value) = OwnedValue::try_from(Value::from("submenu")) {
        root.insert("children-display".to_string(), value);
    }
    LayoutNode(0, root, children)
}

/// The subtree under one item, so a host may ask for a submenu alone.
fn subtree(menu: &Menu, parent_id: i32, depth: i32, wanted: &[String]) -> LayoutNode {
    if parent_id == 0 {
        return layout(menu, depth, wanted);
    }
    let Some(item) = menu.find(parent_id) else {
        return LayoutNode(parent_id, HashMap::new(), Vec::new());
    };
    let children = if depth == 0 {
        Vec::new()
    } else {
        item.children
            .iter()
            .filter_map(|child| node(child, depth - 1, wanted))
            .filter_map(|child| OwnedValue::try_from(child).ok())
            .collect()
    };
    LayoutNode(parent_id, properties(item, wanted), children)
}

#[interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    /// `recursion_depth` of -1 means the whole tree, which is what every panel
    /// asks for in practice.
    async fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> (u32, LayoutNode) {
        let menu = self.controller.menu().await;
        let revision = self.controller.revision().await;
        let depth = if recursion_depth < 0 {
            i32::MAX
        } else {
            recursion_depth
        };
        (revision, subtree(&menu, parent_id, depth, &property_names))
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let menu = self.controller.menu().await;
        ids.into_iter()
            .filter_map(|id| {
                menu.find(id)
                    .map(|item| (id, properties(item, &property_names)))
            })
            .collect()
    }

    async fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let menu = self.controller.menu().await;
        menu.find(id)
            .and_then(|item| properties(item, std::slice::from_ref(&name)).remove(&name))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no property {name} on {id}")))
    }

    /// The panel telling us something happened to an item.
    ///
    /// Only `clicked` does anything. Hover and open events are reported by some
    /// hosts and must not start a session.
    async fn event(
        &self,
        id: i32,
        event_id: String,
        _data: Value<'_>,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        if event_id == "clicked" {
            self.controller.activate(id).await;
        }
        Ok(())
    }

    /// Asked before the menu is shown. The answer is whether the layout changed
    /// as a result; refreshing from the service is how the countdown in an open
    /// menu stays right.
    async fn about_to_show(&self, _id: i32) -> bool {
        let before = self.controller.revision().await;
        let _ = self.controller.refresh().await;
        self.controller.revision().await != before
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
        emitter: &zbus::object_server::SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn items_properties_updated(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        updated: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::Locale;
    use crate::localtime::UtcOffset;
    use crate::menu::QuickOptions;
    use awake_core::{BackendCapabilities, SessionPolicy};
    use awake_ipc::{StatusDocument, WireBackend, WireIndicator};

    fn menu() -> Menu {
        let status = StatusDocument {
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
            now_unix_seconds: 1_700_000_000,
        };
        crate::menu::build(
            &status,
            QuickOptions::default(),
            Locale::EnUs,
            UtcOffset::UTC,
        )
    }

    #[test]
    fn the_layout_type_carries_the_signature_every_panel_expects() {
        assert_eq!(LayoutNode::SIGNATURE.to_string(), "(ia{sv}av)");
    }

    #[test]
    fn the_root_is_id_zero_and_every_top_level_item_hangs_off_it() {
        let menu = menu();
        let layout = layout(&menu, i32::MAX, &[]);
        assert_eq!(layout.0, 0);
        assert_eq!(layout.2.len(), menu.items.len());
    }

    #[test]
    fn a_depth_of_one_returns_the_top_level_without_submenu_children() {
        let menu = menu();
        let shallow = layout(&menu, 1, &[]);
        assert_eq!(shallow.2.len(), menu.items.len());
        // The children of "Start a session" are not included at this depth,
        // which is what a host asking for one level means.
        let deep = layout(&menu, i32::MAX, &[]);
        assert_ne!(format!("{:?}", shallow.2), format!("{:?}", deep.2));
    }

    #[test]
    fn a_separator_is_typed_rather_than_given_an_empty_label() {
        let menu = menu();
        let separator = menu
            .items
            .iter()
            .find(|item| item.kind == ItemKind::Separator)
            .unwrap();
        let properties = properties(separator, &[]);
        assert!(properties.contains_key("type"));
        assert!(!properties.contains_key("label"));
    }

    #[test]
    fn a_quick_option_is_a_checkmark_carrying_its_state() {
        let menu = menu();
        let toggle = menu
            .find_by_label("Allow display to turn off")
            .expect("the quick option must be in the menu");
        let properties = properties(toggle, &[]);
        assert_eq!(
            properties["toggle-type"].downcast_ref::<&str>().unwrap(),
            "checkmark"
        );
        assert_eq!(properties["toggle-state"].downcast_ref::<i32>().unwrap(), 1);
    }

    #[test]
    fn asking_for_one_property_returns_only_that_property() {
        let menu = menu();
        let item = menu.find_by_label("Quit Better Awake").unwrap();
        let properties = properties(item, &["label".to_string()]);
        assert_eq!(properties.len(), 1);
        assert!(properties.contains_key("label"));
    }

    #[test]
    fn a_submenu_declares_itself_so_the_panel_draws_an_arrow() {
        let menu = menu();
        let item = menu.find_by_label("Start a session").unwrap();
        let properties = properties(item, &[]);
        assert_eq!(
            properties["children-display"]
                .downcast_ref::<&str>()
                .unwrap(),
            "submenu"
        );
    }

    #[test]
    fn an_unknown_parent_yields_an_empty_subtree_rather_than_the_whole_menu() {
        let menu = menu();
        let subtree = subtree(&menu, 9_999, i32::MAX, &[]);
        assert_eq!(subtree.0, 9_999);
        assert!(subtree.2.is_empty());
    }

    #[test]
    fn a_submenu_can_be_asked_for_on_its_own() {
        let menu = menu();
        let start = menu.find_by_label("Start a session").unwrap();
        let subtree = subtree(&menu, start.id, i32::MAX, &[]);
        assert_eq!(subtree.0, start.id);
        assert_eq!(subtree.2.len(), start.children.len());
    }
}
