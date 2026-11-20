//! What the Better Awake window carries over when an update restarts it
//! (ticket 105), with no GPUI in it.
//!
//! The view is the section and the navigation pane's open or closed choice.
//! Everything the sections show is read from the service again, which the
//! same update restarted. A rule being edited is typed input with no saved
//! form, so a restart waits for it to be saved or cancelled, and for any
//! request to the service still in flight.

use better_ui::fluent::navigation::PaneChoice;
use better_ui::restart::Blocker;
use serde::{Deserialize, Serialize};

use crate::model::Section;

/// Everything the window puts back.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AwakeView {
    /// The section's place in the navigation.
    pub(crate) section: usize,
    /// The pane closed with its button in a wide window.
    pub(crate) wide_collapsed: bool,
    /// The pane opened with its button in a narrow window.
    pub(crate) narrow_open: bool,
}

impl AwakeView {
    pub(crate) fn of(section: Section, pane: PaneChoice) -> AwakeView {
        AwakeView {
            section: Section::ALL.iter().position(|s| *s == section).unwrap_or(0),
            wide_collapsed: pane.wide_collapsed,
            narrow_open: pane.narrow_open,
        }
    }

    pub(crate) fn pane(&self) -> PaneChoice {
        PaneChoice {
            wide_collapsed: self.wide_collapsed,
            narrow_open: self.narrow_open,
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
    use super::{AwakeView, Blocker, PaneChoice, restart_blocked};
    use crate::model::Section;

    #[test]
    fn every_section_comes_back_as_itself() {
        for section in Section::ALL {
            let view = AwakeView::of(section, PaneChoice::default());
            let read: AwakeView =
                serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
            assert_eq!(read.section(), section);
        }
        let lost = AwakeView {
            section: 99,
            ..AwakeView::default()
        };
        assert_eq!(lost.section(), Section::Status);
    }

    /// The pane's open or closed choice survives an update restart, for a
    /// wide and a narrow window alike, and a hand-off written by a version
    /// that had no such field still reads, with the pane as a new window
    /// draws it.
    #[test]
    fn the_panes_choice_comes_back_and_an_older_handoff_still_reads() {
        for (wide_collapsed, narrow_open) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let pane = PaneChoice {
                wide_collapsed,
                narrow_open,
            };
            let view = AwakeView::of(Section::Rules, pane);
            let read: AwakeView =
                serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
            assert_eq!(read.pane(), pane);
            assert_eq!(read.section(), Section::Rules);
        }
        let older: AwakeView = serde_json::from_str(r#"{"section":2}"#).unwrap();
        assert_eq!(older.section(), Section::Rules);
        assert_eq!(older.pane(), PaneChoice::default());
    }

    #[test]
    fn an_open_rule_editor_or_a_request_holds_the_restart() {
        assert_eq!(restart_blocked(false, false), None);
        assert_eq!(restart_blocked(true, false), Some(Blocker::Dialog));
        assert_eq!(restart_blocked(false, true), Some(Blocker::Task));
        assert_eq!(restart_blocked(true, true), Some(Blocker::Dialog));
    }
}
