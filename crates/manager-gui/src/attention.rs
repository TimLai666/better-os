//! What needs the person, gathered from every page for Home (spec section 3,
//! "Attention cards"), with no GPUI dependency.
//!
//! Each page module turns the window's [`Snapshot`] into its own
//! [`AttentionItem`]s, so Home never repeats a page's rules. Home puts them in
//! one order: most severe first, then the order of the spec's table. Only a
//! warning or an error replaces the "Everything's fine" card; an
//! informational item ("this PC is being kept awake") is listed after the
//! warnings and leaves that card in place.
//!
//! The Apps items are written here, from today's `manager-core` state,
//! because they exist now; ticket 85 owns the Apps page and may move them
//! into its module without changing what they say.

use better_core::ComponentId;
use manager_core::ComponentStatus;

use crate::i18n::{Locale, shell};
use crate::model::Page;
use crate::navigation::Route;

/// How much an item needs the person. Ordered from least to most severe.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) enum Severity {
    /// Worth knowing, nothing to fix.
    Info,
    /// Something to look at.
    Warning,
    /// Something is broken until it is fixed.
    Error,
}

/// What an attention card's one button does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AttentionAction {
    /// Go where the fix is.
    Open(Page),
    /// Refresh the component catalog where it stands.
    RefreshCatalog,
}

/// One attention card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttentionItem {
    /// The page whose rule produced it.
    pub(crate) source: Route,
    pub(crate) severity: Severity,
    /// Its row in the spec's table, which orders items of equal severity.
    pub(crate) row: u16,
    pub(crate) header: String,
    pub(crate) description: String,
    pub(crate) action_label: &'static str,
    pub(crate) action: AttentionAction,
}

/// The rows of the spec's attention table, in its order, so that items of
/// equal severity from different pages keep the table's order. The rows
/// later page tickets produce are numbered here already, and a page uses its
/// own constants; they are unused until that page exists.
#[allow(dead_code)]
pub(crate) mod row {
    pub(crate) const APPS_PACKAGES_INTERRUPTED: u16 = 1;
    pub(crate) const APPS_COMPONENT_FAILED: u16 = 2;
    pub(crate) const APPS_COMPONENT_DRIFT: u16 = 3;
    pub(crate) const APPS_UPDATES: u16 = 4;
    pub(crate) const APPS_CATALOG: u16 = 5;
    pub(crate) const KEEP_AWAKE_SERVICE_STOPPED: u16 = 6;
    pub(crate) const KEEP_AWAKE_SESSION_ACTIVE: u16 = 7;
    pub(crate) const KEEP_AWAKE_CANNOT_STOP_SLEEP: u16 = 8;
    pub(crate) const TOUCHPAD_SAFE_MODE: u16 = 9;
    pub(crate) const TOUCHPAD_GESTURES_OFF: u16 = 10;
    pub(crate) const DEFAULT_APPS_CHANGED: u16 = 11;
    pub(crate) const FIXES_FOUND: u16 = 12;
    pub(crate) const SYSTEM_BATTERY_WORN: u16 = 13;
}

/// What one component looks like to Home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentFacts {
    pub(crate) id: ComponentId,
    /// The name in the current language.
    pub(crate) name: String,
    pub(crate) status: ComponentStatus,
    /// The record and the machine disagree.
    pub(crate) drifted: bool,
}

/// Everything the pages' attention rules read, taken from the window once
/// per frame. Page tickets add the facts their rules need.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) locale: Locale,
    pub(crate) components: Vec<ComponentFacts>,
    /// dpkg left an install unfinished, as the privileged service last said.
    pub(crate) packages_interrupted: bool,
    /// The catalog's "may be outdated" sentence, when it may be.
    pub(crate) catalog_warning: Option<&'static str>,
}

impl Snapshot {
    /// A snapshot with nothing in it, for a page that has nothing to say.
    #[cfg(test)]
    pub(crate) fn empty(locale: Locale) -> Self {
        Self {
            locale,
            components: Vec::new(),
            packages_interrupted: false,
            catalog_warning: None,
        }
    }

    /// How many components have an update waiting.
    pub(crate) fn update_count(&self) -> usize {
        self.components
            .iter()
            .filter(|component| component.status == ComponentStatus::UpdateAvailable)
            .count()
    }
}

/// The Apps rows of the attention table.
pub(crate) fn apps_attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    let s = shell(snapshot.locale);
    let item =
        |severity, row, header: String, description: String, action_label, action| AttentionItem {
            source: Route::Apps,
            severity,
            row,
            header,
            description,
            action_label,
            action,
        };
    let mut items = Vec::new();
    if snapshot.packages_interrupted {
        items.push(item(
            Severity::Error,
            row::APPS_PACKAGES_INTERRUPTED,
            s.attention_dpkg_header.to_string(),
            s.attention_dpkg_description.to_string(),
            s.attention_dpkg_action,
            AttentionAction::Open(Page::Health),
        ));
    }
    for component in &snapshot.components {
        let severity = match component.status {
            ComponentStatus::Failed => Some(Severity::Error),
            ComponentStatus::Degraded => Some(Severity::Warning),
            _ => None,
        };
        if let Some(severity) = severity {
            items.push(item(
                severity,
                row::APPS_COMPONENT_FAILED,
                s.attention_failed_header
                    .replace("{component}", &component.name),
                s.attention_failed_description.to_string(),
                s.attention_view,
                AttentionAction::Open(Page::ComponentDetail(component.id.clone())),
            ));
        }
    }
    for component in snapshot.components.iter().filter(|c| c.drifted) {
        items.push(item(
            Severity::Warning,
            row::APPS_COMPONENT_DRIFT,
            s.attention_drift_header
                .replace("{component}", &component.name),
            s.attention_drift_description.to_string(),
            s.attention_view,
            AttentionAction::Open(Page::ComponentDetail(component.id.clone())),
        ));
    }
    let updates = snapshot.update_count();
    if updates > 0 {
        let header = if updates == 1 {
            s.attention_update_header_one.to_string()
        } else {
            s.attention_updates_header
                .replace("{n}", &updates.to_string())
        };
        items.push(item(
            Severity::Warning,
            row::APPS_UPDATES,
            header,
            s.attention_updates_description.to_string(),
            s.attention_updates_action,
            AttentionAction::Open(Page::Updates),
        ));
    }
    if let Some(warning) = snapshot.catalog_warning {
        items.push(item(
            Severity::Warning,
            row::APPS_CATALOG,
            s.attention_catalog_header.to_string(),
            warning.to_string(),
            s.attention_catalog_action,
            AttentionAction::RefreshCatalog,
        ));
    }
    items
}

/// Puts items in Home's order: most severe first, then by the table's row,
/// keeping each page's own order within a row.
pub(crate) fn order(mut items: Vec<AttentionItem>) -> Vec<AttentionItem> {
    items.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.row.cmp(&b.row)));
    items
}

/// Whether Home shows "Everything's fine": no warning and no error. An
/// informational item never takes that card away on its own.
pub(crate) fn everything_fine(items: &[AttentionItem]) -> bool {
    items.iter().all(|item| item.severity == Severity::Info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(id: &str, name: &str, status: ComponentStatus, drifted: bool) -> ComponentFacts {
        ComponentFacts {
            id: ComponentId::new(id).expect("a valid id"),
            name: name.to_string(),
            status,
            drifted,
        }
    }

    /// Every kind of Apps fault at once: an interrupted dpkg, a failed and a
    /// degraded component, drift, two updates, and an outdated catalog.
    fn faulty(locale: Locale) -> Snapshot {
        Snapshot {
            locale,
            components: vec![
                facts(
                    "better-files",
                    "Better Files",
                    ComponentStatus::UpdateAvailable,
                    false,
                ),
                facts(
                    "better-awake",
                    "Better Awake",
                    ComponentStatus::Failed,
                    false,
                ),
                facts(
                    "better-monitor",
                    "Better Monitor",
                    ComponentStatus::Degraded,
                    false,
                ),
                facts("better-git", "Better Git", ComponentStatus::Healthy, true),
                facts(
                    "better-touchpad",
                    "Better Touchpad",
                    ComponentStatus::UpdateAvailable,
                    false,
                ),
            ],
            packages_interrupted: true,
            catalog_warning: Some("The published catalog could not be reached."),
        }
    }

    #[test]
    fn nothing_wrong_is_everything_fine() {
        let snapshot = Snapshot {
            components: vec![facts(
                "better-files",
                "Better Files",
                ComponentStatus::Healthy,
                false,
            )],
            ..Snapshot::empty(Locale::EnUs)
        };
        let items = order(apps_attention(&snapshot));
        assert!(items.is_empty());
        assert!(everything_fine(&items));
    }

    #[test]
    fn each_kind_of_apps_fault_has_its_card_in_english() {
        let items = order(apps_attention(&faulty(Locale::EnUs)));
        let headers: Vec<&str> = items.iter().map(|item| item.header.as_str()).collect();
        assert_eq!(
            headers,
            [
                "A package installation was interrupted",
                "Better Awake needs attention",
                "Better Monitor needs attention",
                "Better Git doesn't match its record",
                "2 components have new versions",
                "The version list may be out of date",
            ]
        );
        assert!(!everything_fine(&items));
        assert_eq!(items[0].action_label, "Repair");
        assert_eq!(items[0].action, AttentionAction::Open(Page::Health));
        assert_eq!(items[1].description, "It didn't pass its check.");
        assert_eq!(
            items[1].action,
            AttentionAction::Open(Page::ComponentDetail(
                ComponentId::new("better-awake").unwrap()
            ))
        );
        assert_eq!(
            items[3].description,
            "It can't be installed, updated, or restored until they match."
        );
        assert_eq!(items[4].action_label, "See updates");
        assert_eq!(items[4].action, AttentionAction::Open(Page::Updates));
        assert_eq!(
            items[5].description,
            "The published catalog could not be reached."
        );
        assert_eq!(items[5].action, AttentionAction::RefreshCatalog);
        assert_eq!(items[5].action_label, "Check for updates");
        assert!(items.iter().all(|item| item.source == Route::Apps));
    }

    #[test]
    fn each_kind_of_apps_fault_has_its_card_in_chinese() {
        let items = order(apps_attention(&faulty(Locale::ZhTw)));
        let headers: Vec<&str> = items.iter().map(|item| item.header.as_str()).collect();
        assert_eq!(
            headers,
            [
                "套件安裝中途被中斷",
                "Better Awake 需要處理",
                "Better Monitor 需要處理",
                "Better Git 和紀錄對不上",
                "2 個元件有新版本",
                "版本清單可能不是最新的",
            ]
        );
        assert_eq!(
            items[0].description,
            "完成之前，任何元件都無法安裝、更新或移除。"
        );
        assert_eq!(items[0].action_label, "修復");
        assert_eq!(items[4].description, "確認後才會安裝。");
        assert_eq!(items[4].action_label, "查看更新");
        assert_eq!(items[5].action_label, "檢查更新");
    }

    #[test]
    fn one_update_is_said_in_the_singular() {
        let snapshot = Snapshot {
            components: vec![facts(
                "better-files",
                "Better Files",
                ComponentStatus::UpdateAvailable,
                false,
            )],
            ..Snapshot::empty(Locale::EnUs)
        };
        assert_eq!(
            apps_attention(&snapshot)[0].header,
            "1 component has a new version"
        );
    }

    #[test]
    fn severity_comes_first_then_the_table_row() {
        let make = |severity, row| AttentionItem {
            source: Route::Apps,
            severity,
            row,
            header: format!("{severity:?} {row}"),
            description: String::new(),
            action_label: "",
            action: AttentionAction::RefreshCatalog,
        };
        let ordered = order(vec![
            make(Severity::Info, 1),
            make(Severity::Warning, 9),
            make(Severity::Error, 7),
            make(Severity::Warning, 2),
            make(Severity::Error, 3),
        ]);
        let headers: Vec<&str> = ordered.iter().map(|item| item.header.as_str()).collect();
        assert_eq!(
            headers,
            ["Error 3", "Error 7", "Warning 2", "Warning 9", "Info 1"]
        );
    }

    /// "This PC is being kept awake" is listed, after the warnings, and does
    /// not on its own take "Everything's fine" away.
    #[test]
    fn an_informational_item_leaves_everything_fine_in_place() {
        let info = AttentionItem {
            source: Route::KeepAwake,
            severity: Severity::Info,
            row: 7,
            header: "This PC is being kept awake".to_string(),
            description: String::new(),
            action_label: "View",
            action: AttentionAction::Open(Page::KeepAwake),
        };
        assert!(everything_fine(std::slice::from_ref(&info)));
        let warning = AttentionItem {
            severity: Severity::Warning,
            ..info.clone()
        };
        assert!(!everything_fine(&[info, warning]));
    }
}
