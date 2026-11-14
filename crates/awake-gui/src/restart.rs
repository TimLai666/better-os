//! What the Better Awake window carries over when an update restarts it
//! (ticket 105), with no GPUI in it.
//!
//! The view is the section. Everything the sections show is read from the
//! service again, which the same update restarted. A rule being edited is
//! typed input with no saved form, so a restart waits for it to be saved or
//! cancelled, and for any request to the service still in flight.

use better_ui::restart::Blocker;
use serde::{Deserialize, Serialize};

use crate::model::Section;

/// Everything the window puts back.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AwakeView {
    /// The section's place in the navigation.
    pub(crate) section: usize,
}

impl AwakeView {
    pub(crate) fn of(section: Section) -> AwakeView {
        AwakeView {
            section: Section::ALL.iter().position(|s| *s == section).unwrap_or(0),
        }
    }

    pub(crate) fn section(&self) -> Section {
        Section::ALL
            .get(self.section)
            .copied()
            .unwrap_or(Section::Status)
    }
}

pub(crate) fn restart_blocked(editing_rule: bool, talking_to_service: bool) -> Option<Blocker> {
    if editing_rule {
        Some(Blocker::Dialog)
    } else if talking_to_service {
        Some(Blocker::Task)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{AwakeView, Blocker, restart_blocked};
    use crate::model::Section;

    #[test]
    fn every_section_comes_back_as_itself() {
        for section in Section::ALL {
            let view = AwakeView::of(section);
            let read: AwakeView =
                serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
            assert_eq!(read.section(), section);
        }
        assert_eq!(AwakeView { section: 99 }.section(), Section::Status);
    }

    #[test]
    fn an_open_rule_editor_or_a_request_holds_the_restart() {
        assert_eq!(restart_blocked(false, false), None);
        assert_eq!(restart_blocked(true, false), Some(Blocker::Dialog));
        assert_eq!(restart_blocked(false, true), Some(Blocker::Task));
        assert_eq!(restart_blocked(true, true), Some(Blocker::Dialog));
    }
}
