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
    /// The tray's tokio runtime. zbus runs these handlers on its own executor,
    /// where tokio's timers do not exist, so anything that talks to the
    /// service (and so may time out) is run here and awaited.
    runtime: tokio::runtime::Handle,
}

impl DbusMenu {
    /// Must be called from inside the tray's tokio runtime.
    pub fn new(controller: Arc<TrayController>) -> Self {
        Self {
            controller,
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

/// The properties one item exposes, filtered to what the host asked for.
fn properties(item: &MenuItem, wanted: &[String]) -> HashMap<String, OwnedValue> {
    let mut properties: Vec<(&str, Value<'static>)> = Vec::new();

    match &item.kind {
        ItemKind::Separator => properties.push(("type", Value::from("separator"))),
        ItemKind::Checkmark { checked } => {
            properties.push(("type", Value::from("standard")));
            properties.push(("label", Value::from(item.label.clone())));
            properties.push(("toggle-type", Value::from("checkmark")));
            properties.push(("toggle-state", Value::from(i32::from(*checked))));
        }
        ItemKind::Submenu => {
            properties.push(("type", Value::from("standard")));
            properties.push(("label", Value::from(item.label.clone())));
            properties.push(("children-display", Value::from("submenu")));
        }
        ItemKind::Standard | ItemKind::Info => {
            // Said outright rather than left to the default, so a host that
            // somehow kept an older item under this id is corrected.
            properties.push(("type", Value::from("standard")));
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

/// Every property of one item, as `GetGroupProperties` with no names returns
/// them. The controller compares these to tell a host which items changed.
pub fn all_properties(item: &MenuItem) -> HashMap<String, OwnedValue> {
    properties(item, &[])
}

/// Whether the host is shown an item at all.
///
/// A disabled submenu is not: GNOME Shell draws it as a greyed row with an
/// arrow that opens nothing, which reads as a broken menu rather than as an
/// unavailable action. The row that explains why (no keep-awake support, an
/// indefinite session with no end to push out) is already in the menu.
fn is_drawn(item: &MenuItem) -> bool {
    !(item.kind == ItemKind::Submenu && (!item.enabled || item.children.is_empty()))
}

/// The items of one level a host is shown, in order: no hidden submenu, and
/// no separator at either end or next to another one, which is what hiding an
/// item between two separators would otherwise leave behind.
pub fn drawn(items: &[MenuItem]) -> Vec<&MenuItem> {
    let mut shown: Vec<&MenuItem> = Vec::new();
    for item in items.iter().filter(|item| is_drawn(item)) {
        let separator = item.kind == ItemKind::Separator;
        let after_separator = shown
            .last()
            .is_none_or(|last| last.kind == ItemKind::Separator);
        if separator && after_separator {
            continue;
        }
        shown.push(item);
    }
    while shown
        .last()
        .is_some_and(|last| last.kind == ItemKind::Separator)
    {
        shown.pop();
    }
    shown
}

/// One item and, unless the depth ran out, its children.
fn node(item: &MenuItem, depth: i32, wanted: &[String]) -> Option<Value<'static>> {
    zbus::zvariant::StructureBuilder::new()
        .add_field(item.id)
        .add_field(properties(item, wanted))
        .add_field(children(&item.children, depth, wanted))
        .build()
        .ok()
        .map(Value::from)
}

fn children(items: &[MenuItem], depth: i32, wanted: &[String]) -> Vec<OwnedValue> {
    if depth == 0 {
        return Vec::new();
    }
    drawn(items)
        .into_iter()
        .filter_map(|child| node(child, depth - 1, wanted))
        .filter_map(|child| OwnedValue::try_from(child).ok())
        .collect()
}

/// The whole menu under the dbusmenu root, which is always id 0.
pub fn layout(menu: &Menu, depth: i32, wanted: &[String]) -> LayoutNode {
    let mut root = HashMap::new();
    if (wanted.is_empty() || wanted.iter().any(|name| name == "children-display"))
        && let Ok(value) = OwnedValue::try_from(Value::from("submenu"))
    {
        root.insert("children-display".to_string(), value);
    }
    LayoutNode(0, root, children(&menu.items, depth, wanted))
}

/// The subtree under one item, so a host may ask for a submenu alone.
fn subtree(menu: &Menu, parent_id: i32, depth: i32, wanted: &[String]) -> LayoutNode {
    if parent_id == 0 {
        return layout(menu, depth, wanted);
    }
    let Some(item) = menu.find(parent_id) else {
        return LayoutNode(parent_id, HashMap::new(), Vec::new());
    };
    LayoutNode(
        parent_id,
        properties(item, wanted),
        children(&item.children, depth, wanted),
    )
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
        let (revision, menu) = self.controller.layout().await;
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
            let controller = self.controller.clone();
            self.runtime
                .spawn(async move { controller.activate(id).await })
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        }
        Ok(())
    }

    /// Asked before a menu is shown. The answer is whether the layout changed
    /// as a result, which makes the host read it again.
    ///
    /// Only the whole menu opening refreshes from the service; that is how
    /// the countdown in it stays right. A submenu opening does not: GNOME
    /// asks for each one as it unfolds, and a layout that changed under an
    /// unfolding submenu would rebuild it closed under the pointer.
    async fn about_to_show(&self, id: i32) -> bool {
        if id != 0 {
            return false;
        }
        let controller = self.controller.clone();
        self.runtime
            .spawn(async move {
                let before = controller.revision().await;
                let _ = controller.refresh().await;
                controller.revision().await != before
            })
            .await
            .unwrap_or(false)
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
            active_rules: Vec::new(),
            rule_summary: awake_ipc::WireRuleSummary::default(),
            rules_suppression: None,
            conflicts: Vec::new(),
            providers: Vec::new(),
            battery_protection: awake_ipc::WireBatteryProtection::default(),
            now_unix_seconds: 1_700_000_000,
        };
        crate::menu::build(
            &status,
            QuickOptions::default(),
            Locale::EnUs,
            UtcOffset::UTC,
            false,
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

    fn drawn_labels(menu: &Menu) -> Vec<String> {
        fn walk(items: &[MenuItem], out: &mut Vec<String>) {
            for item in drawn(items) {
                out.push(match item.kind {
                    ItemKind::Separator => "---".to_string(),
                    _ => item.label.clone(),
                });
                walk(&item.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&menu.items, &mut out);
        out
    }

    fn awake(end: awake_ipc::WireEnd, remaining: awake_ipc::WireRemaining) -> Menu {
        let mut status = StatusDocument {
            indicator: WireIndicator::ActiveManual,
            ..status_document()
        };
        let session = awake_ipc::WireSession {
            session_id: 1,
            reason: "Build".to_string(),
            origin: awake_core::SessionOrigin::Manual,
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: None,
            end,
            started_at_unix_seconds: 1_700_000_000,
            remaining,
        };
        status.reasons = vec![awake_ipc::WireReason {
            session_id: 1,
            origin: awake_core::SessionOrigin::Manual,
            reason: session.reason.clone(),
        }];
        status.sessions = vec![session];
        crate::menu::build(
            &status,
            QuickOptions::default(),
            Locale::EnUs,
            UtcOffset::UTC,
            false,
        )
    }

    fn status_document() -> StatusDocument {
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
            now_unix_seconds: 1_700_000_000,
        }
    }

    #[test]
    fn an_indefinite_session_draws_no_extend_arrow_that_opens_nothing() {
        let menu = awake(
            awake_ipc::WireEnd::Indefinite,
            awake_ipc::WireRemaining::UntilEnded,
        );
        let labels = drawn_labels(&menu);
        assert!(
            !labels.contains(&"Extend session".to_string()),
            "{labels:?}"
        );
        assert!(labels.contains(&"End session".to_string()));
        // The hidden children are not drawn either.
        assert!(!labels.iter().any(|label| label == "15 min"), "{labels:?}");
    }

    #[test]
    fn a_timed_session_draws_an_enabled_extend_submenu() {
        let menu = awake(
            awake_ipc::WireEnd::Duration { seconds: 3_600 },
            awake_ipc::WireRemaining::Seconds { seconds: 1_800 },
        );
        let extend = menu.find_by_label("Extend session").unwrap();
        assert!(is_drawn(extend));
        assert_eq!(drawn(&extend.children).len(), 3);
    }

    #[test]
    fn hiding_an_item_leaves_no_doubled_or_dangling_separator() {
        let mut status = status_document();
        status.indicator = WireIndicator::Unavailable;
        status.backend.available = false;
        let menu = crate::menu::build(
            &status,
            QuickOptions::default(),
            Locale::EnUs,
            UtcOffset::UTC,
            false,
        );
        let labels = drawn_labels(&menu);
        assert!(!labels.contains(&"Start a session".to_string()));
        assert_ne!(labels.first().map(String::as_str), Some("---"));
        assert_ne!(labels.last().map(String::as_str), Some("---"));
        assert!(
            !labels
                .windows(2)
                .any(|pair| pair[0] == "---" && pair[1] == "---"),
            "{labels:?}"
        );
    }

    #[test]
    fn every_item_states_its_type_so_a_cached_id_cannot_keep_an_old_one() {
        let menu = menu();
        for item in &menu.items {
            item.walk(&mut |candidate| {
                assert!(
                    all_properties(candidate).contains_key("type"),
                    "{candidate:?}"
                );
            });
        }
    }

    #[test]
    fn a_host_asking_only_for_type_and_children_display_gets_only_those() {
        let menu = menu();
        let wanted = ["type".to_string(), "children-display".to_string()];
        let layout = layout(&menu, i32::MAX, &wanted);
        for name in layout.1.keys() {
            assert!(wanted.contains(name));
        }
        let start = menu.find_by_label("Start a session").unwrap();
        let properties = properties(start, &wanted);
        assert_eq!(properties.len(), 2);
    }
}
