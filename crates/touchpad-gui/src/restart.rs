//! What Better Touchpad carries over when an update restarts it (ticket 105),
//! with no GPUI in it.
//!
//! The view is the page, the device whose gesture profile is being edited,
//! and the two file paths on the Gestures screen. A setting moved but not yet
//! applied, a preset preview, an import waiting for its plan, an open gesture
//! editor, and a live test are all input with no saved form, so a restart
//! waits for each of them to be applied, cancelled, or finished, as it does
//! while settings are being applied.

use better_ui::restart::Blocker;
use serde::{Deserialize, Serialize};

use crate::model::Page;

/// Everything the window puts back.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchpadView {
    /// The page's key, as `--page` spells it.
    pub page: String,
    pub gesture_device: Option<String>,
    pub export_path: Option<String>,
    pub import_path: Option<String>,
}

impl TouchpadView {
    /// The page to open, or the overview when this build has no such page.
    pub fn page(&self) -> Page {
        Page::parse(&self.page).unwrap_or(Page::Overview)
    }
}

/// What is going on in the window that a restart would lose.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InFlight {
    pub applying: bool,
    pub unapplied_settings: bool,
    pub preview: bool,
    pub import: bool,
    pub editor: bool,
    pub live_test: bool,
}

impl InFlight {
    /// What ends the wait: work that runs, the editor dialog, or changes to
    /// apply or discard.
    pub fn blocker(self) -> Option<Blocker> {
        if self.applying || self.live_test {
            Some(Blocker::Task)
        } else if self.editor {
            Some(Blocker::Dialog)
        } else if self.unapplied_settings || self.preview || self.import {
            Some(Blocker::UnsavedChanges)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InFlight, TouchpadView};
    use crate::model::Page;

    #[test]
    fn every_page_comes_back_as_itself() {
        for page in Page::ALL {
            let view = TouchpadView {
                page: page.key().to_string(),
                ..TouchpadView::default()
            };
            let read: TouchpadView =
                serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
            assert_eq!(read.page(), page);
        }
        assert_eq!(TouchpadView::default().page(), Page::Overview);
    }

    #[test]
    fn input_with_no_saved_form_holds_the_restart_and_says_why() {
        use better_ui::restart::Blocker;
        assert_eq!(InFlight::default().blocker(), None);
        let one = |set: fn(&mut InFlight)| {
            let mut flight = InFlight::default();
            set(&mut flight);
            flight.blocker()
        };
        assert_eq!(one(|f| f.applying = true), Some(Blocker::Task));
        assert_eq!(one(|f| f.live_test = true), Some(Blocker::Task));
        assert_eq!(one(|f| f.editor = true), Some(Blocker::Dialog));
        for set in [
            (|f: &mut InFlight| f.unapplied_settings = true) as fn(&mut InFlight),
            |f| f.preview = true,
            |f| f.import = true,
        ] {
            assert_eq!(one(set), Some(Blocker::UnsavedChanges));
        }
    }
}
