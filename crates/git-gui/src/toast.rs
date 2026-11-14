//! The toast: a short-lived note for something the window does not show by
//! itself (a pull request or issue created, with its link; published;
//! forked; a stash waiting; checks re-run; signed in). It hides itself
//! about six seconds after it appears, never while the pointer is on it,
//! and the countdown starts again when the pointer leaves. A problem is not
//! a toast: its bar stays until it is closed.

use std::sync::OnceLock;
use std::time::Instant;

use crate::session::Notice;

/// How long a toast stays, in milliseconds.
pub const SHOWN_FOR_MS: u64 = 6_000;

/// Milliseconds since the window started, for the toast's clock.
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// When the toast on screen appeared, and whether the pointer is on it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToastClock {
    showing: Option<Notice>,
    since: u64,
    hovered: bool,
}

impl ToastClock {
    /// Notes the notice the session holds now; a new one starts its clock.
    pub fn observe(&mut self, notice: Option<&Notice>, now: u64) {
        if self.showing.as_ref() != notice {
            self.showing = notice.cloned();
            self.since = now;
            self.hovered = false;
        }
    }

    /// The pointer entered or left the toast. Leaving starts the countdown
    /// again, so a toast someone was reading does not vanish at once.
    pub fn hovered(&mut self, hovered: bool, now: u64) {
        if self.hovered && !hovered {
            self.since = now;
        }
        self.hovered = hovered;
    }

    /// Whether the toast has been up long enough to hide.
    pub fn expired(&self, now: u64) -> bool {
        self.showing.is_some() && !self.hovered && now.saturating_sub(self.since) >= SHOWN_FOR_MS
    }
}

/// The page a toast's button opens, when it has one.
pub fn link(notice: &Notice) -> Option<String> {
    use crate::notifications::Notice as Pull;
    match notice {
        Notice::PullCreated { url, .. }
        | Notice::IssueCreated { url, .. }
        | Notice::Published { url } => Some(url.clone()),
        Notice::Pull(Pull::ChecksFailed { url, .. } | Pull::Reviewed { url, .. }) => {
            Some(url.clone())
        }
        _ => None,
    }
}
