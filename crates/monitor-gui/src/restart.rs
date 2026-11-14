//! What Better Monitor carries over when an update restarts it (ticket 105),
//! with no GPUI in it.
//!
//! The view is the page, the resource the Performance page shows, the
//! selection and the expanded applications on the Processes page, the filter
//! being typed, every table's sort, and the pane and settings expanders. The
//! numbers themselves are not carried: the service keeps collecting while the
//! window restarts, and the new window reads them from it.
//!
//! A restart waits while a question is open over the page, such as "End this
//! process?", because the answer has not been given yet.

use monitor_views::{ProcessColumn, SortDirection};
use serde::{Deserialize, Serialize};

use crate::app::{Page, Resource};
use crate::tables::{AppColumn, AppSelection};

/// The Performance page's resource, as the hand-off spells it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub(crate) enum ResourceKey {
    #[default]
    Cpu,
    Memory,
    Disk(String),
    Adapter(String),
    Gpu(String),
    Npu(String),
    Temperatures,
    Battery,
}

impl ResourceKey {
    pub(crate) fn of(resource: &Resource) -> ResourceKey {
        match resource {
            Resource::Cpu => ResourceKey::Cpu,
            Resource::Memory => ResourceKey::Memory,
            Resource::Disk(name) => ResourceKey::Disk(name.clone()),
            Resource::Adapter(name) => ResourceKey::Adapter(name.clone()),
            Resource::Gpu(address) => ResourceKey::Gpu(address.clone()),
            Resource::Npu(address) => ResourceKey::Npu(address.clone()),
            Resource::Temperatures => ResourceKey::Temperatures,
            Resource::Battery => ResourceKey::Battery,
        }
    }

    pub(crate) fn resource(&self) -> Resource {
        match self {
            ResourceKey::Cpu => Resource::Cpu,
            ResourceKey::Memory => Resource::Memory,
            ResourceKey::Disk(name) => Resource::Disk(name.clone()),
            ResourceKey::Adapter(name) => Resource::Adapter(name.clone()),
            ResourceKey::Gpu(address) => Resource::Gpu(address.clone()),
            ResourceKey::Npu(address) => Resource::Npu(address.clone()),
            ResourceKey::Temperatures => Resource::Temperatures,
            ResourceKey::Battery => Resource::Battery,
        }
    }
}

/// A selected row on the Processes page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SelectionKey {
    Group(String),
    Process(u32),
}

/// Everything the window puts back. Columns and pages are carried by their
/// place in the lists the window draws them from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct MonitorView {
    pub(crate) page: usize,
    pub(crate) resource: ResourceKey,
    pub(crate) per_core: bool,
    pub(crate) paused: bool,
    pub(crate) compact_choice: Option<bool>,
    pub(crate) filter: String,
    pub(crate) app_sort: (usize, bool),
    pub(crate) app_selection: Option<SelectionKey>,
    pub(crate) expanded: Vec<String>,
    pub(crate) process_sort: (usize, bool),
    pub(crate) selected_pid: Option<u32>,
    pub(crate) startup_sort: Option<(usize, bool)>,
    pub(crate) services_sort: Option<(usize, bool)>,
    pub(crate) history_seconds: Option<u64>,
    pub(crate) grouping_open: bool,
    pub(crate) sources_open: bool,
}

impl Default for MonitorView {
    fn default() -> Self {
        MonitorView {
            page: 0,
            resource: ResourceKey::Cpu,
            per_core: false,
            paused: false,
            compact_choice: None,
            filter: String::new(),
            app_sort: (column_index(AppColumn::Cpu), false),
            app_selection: None,
            expanded: Vec::new(),
            process_sort: (process_column_index(ProcessColumn::CpuUtilization), false),
            selected_pid: None,
            startup_sort: None,
            services_sort: None,
            history_seconds: None,
            grouping_open: false,
            sources_open: false,
        }
    }
}

pub(crate) fn page_of(index: usize) -> Page {
    Page::ALL.get(index).copied().unwrap_or(Page::Overview)
}

pub(crate) fn column_index(column: AppColumn) -> usize {
    AppColumn::ALL
        .iter()
        .position(|c| *c == column)
        .unwrap_or(0)
}

pub(crate) fn column_of(index: usize) -> AppColumn {
    AppColumn::ALL.get(index).copied().unwrap_or(AppColumn::Cpu)
}

pub(crate) fn process_column_index(column: ProcessColumn) -> usize {
    ProcessColumn::ALL
        .iter()
        .position(|c| *c == column)
        .unwrap_or(0)
}

pub(crate) fn process_column_of(index: usize) -> ProcessColumn {
    ProcessColumn::ALL
        .get(index)
        .copied()
        .unwrap_or(ProcessColumn::CpuUtilization)
}

pub(crate) fn ascending(direction: SortDirection) -> bool {
    direction == SortDirection::Ascending
}

pub(crate) fn direction(ascending: bool) -> SortDirection {
    if ascending {
        SortDirection::Ascending
    } else {
        SortDirection::Descending
    }
}

impl SelectionKey {
    pub(crate) fn of(selection: &AppSelection) -> SelectionKey {
        match selection {
            AppSelection::Group(key) => SelectionKey::Group(key.clone()),
            AppSelection::Process(pid) => SelectionKey::Process(*pid),
        }
    }

    pub(crate) fn selection(&self) -> AppSelection {
        match self {
            SelectionKey::Group(key) => AppSelection::Group(key.clone()),
            SelectionKey::Process(pid) => AppSelection::Process(*pid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MonitorView, ResourceKey, SelectionKey, column_index, column_of, page_of,
        process_column_index, process_column_of,
    };
    use crate::app::{Page, Resource};
    use crate::tables::{AppColumn, AppSelection};
    use monitor_views::ProcessColumn;

    #[test]
    fn every_page_column_and_resource_comes_back_as_itself() {
        for page in Page::ALL {
            assert_eq!(page_of(page.index()), page);
        }
        assert_eq!(page_of(999), Page::Overview, "a page this build lacks");
        for column in AppColumn::ALL {
            assert_eq!(column_of(column_index(column)), column);
        }
        for column in ProcessColumn::ALL {
            assert_eq!(process_column_of(process_column_index(column)), column);
        }
        for resource in [
            Resource::Cpu,
            Resource::Memory,
            Resource::Disk("nvme0n1".into()),
            Resource::Adapter("wlan0".into()),
            Resource::Gpu("0000:01:00.0".into()),
            Resource::Npu("0000:00:0b.0".into()),
            Resource::Temperatures,
            Resource::Battery,
        ] {
            let text = serde_json::to_string(&ResourceKey::of(&resource)).unwrap();
            let read: ResourceKey = serde_json::from_str(&text).unwrap();
            assert_eq!(read.resource(), resource);
        }
        for selection in [
            AppSelection::Group("firefox".into()),
            AppSelection::Process(42),
        ] {
            assert_eq!(SelectionKey::of(&selection).selection(), selection);
        }
    }

    #[test]
    fn a_view_reads_through_the_handoff_encoding() {
        let view = MonitorView {
            page: Page::Processes.index(),
            filter: "fire".into(),
            expanded: vec!["firefox".into()],
            app_selection: Some(SelectionKey::Process(42)),
            startup_sort: Some((2, true)),
            ..MonitorView::default()
        };
        let read: MonitorView =
            serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
        assert_eq!(read, view);
        let older: MonitorView = serde_json::from_str(r#"{"page":1}"#).unwrap();
        assert_eq!(page_of(older.page), Page::Processes);
        assert_eq!(older.resource, ResourceKey::Cpu);
    }
}
