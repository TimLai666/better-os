//! The answers `org.freedesktop.impl.portal.Screenshot` gives (spec A1–A5).
//!
//! xdg-desktop-portal 1.18 passes the backend's response number through to
//! the application (`src/screenshot.c` `send_response_in_thread_func`): 0 is
//! success with the results, 1 the person cancelled, 2 anything else; and
//! for `Screenshot` it reads exactly one result, `uri`, and for `PickColor`
//! `color` as `(ddd)`.

/// The response number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Response {
    Success,
    Cancelled,
    Other,
}

impl Response {
    pub fn code(self) -> u32 {
        match self {
            Response::Success => 0,
            Response::Cancelled => 1,
            Response::Other => 2,
        }
    }
}

/// How a portal request ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// A picture was saved here (a `file://` URI).
    Picture(String),
    /// A colour was picked, each channel in 0–1.
    Color(f64, f64, f64),
    /// The person left the interface, or the portal closed the request.
    Cancelled,
    /// The request could not be served: no picture, saving locked down, a
    /// request already pending.
    Refused(String),
}

impl Outcome {
    pub fn response(&self) -> Response {
        match self {
            Outcome::Picture(_) | Outcome::Color(..) => Response::Success,
            Outcome::Cancelled => Response::Cancelled,
            Outcome::Refused(_) => Response::Other,
        }
    }
}

/// What a `Screenshot` call asks for, from its options.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScreenshotOptions {
    /// Show the interface (A1); otherwise take every output at once (A2).
    pub interactive: bool,
    /// The frontend asked the person already (version 2's
    /// `permission_store_checked`).
    pub permission_store_checked: bool,
}

/// A pixel's colour as the portal answers it: 0–255 to 0–1, alpha dropped
/// (GNOME's `PickColorAsync` 2752–2759).
pub fn color_of(bgra: [u8; 4]) -> (f64, f64, f64) {
    (
        f64::from(bgra[2]) / 255.0,
        f64::from(bgra[1]) / 255.0,
        f64::from(bgra[0]) / 255.0,
    )
}

/// The rule for a new request while another is pending (A4): one at a time,
/// as GNOME refuses a second operation "for this sender" (2449); with one
/// interface on the screen there can be only one, whoever asks.
pub fn may_start(pending: bool) -> Result<(), Outcome> {
    if pending {
        Err(Outcome::Refused(
            "a screenshot request is already showing".into(),
        ))
    } else {
        Ok(())
    }
}

/// Who a capture is for. Only a capture the portal request started may
/// answer it: a Shift+Print while the portal's interface is open is the
/// person's own picture, not the application's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    /// Print Screen, its keys, or `org.betteros.Screenshot1`.
    Person,
    /// The portal call with this request handle.
    Portal(String),
}

/// What a portal request asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestKind {
    /// `Screenshot` with `interactive`: the interface.
    Interface,
    /// `Screenshot` without it: every output at once, a file only.
    Instant,
    PickColor,
}

/// The one portal request in flight, with the way to answer it.
#[derive(Debug)]
pub struct Pending<T> {
    pub handle: String,
    pub kind: RequestKind,
    pub reply: T,
}

/// The portal request in flight, at most one (A4).
#[derive(Debug)]
pub struct PortalSlot<T> {
    pending: Option<Pending<T>>,
}

impl<T> Default for PortalSlot<T> {
    fn default() -> Self {
        PortalSlot { pending: None }
    }
}

impl<T> PortalSlot<T> {
    /// Takes a new request, or gives its reply back with the refusal when one
    /// is pending or the interface is busy with someone else.
    pub fn begin(
        &mut self,
        handle: String,
        kind: RequestKind,
        reply: T,
        busy: bool,
    ) -> Result<(), (T, Outcome)> {
        if let Err(refusal) = may_start(self.pending.is_some() || busy) {
            return Err((reply, refusal));
        }
        self.pending = Some(Pending {
            handle,
            kind,
            reply,
        });
        Ok(())
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn kind(&self) -> Option<RequestKind> {
        self.pending.as_ref().map(|pending| pending.kind)
    }

    /// Whether a capture for `origin` is still wanted: the person's always,
    /// a portal request's only while that request is pending.
    pub fn wants(&self, origin: &Origin) -> bool {
        match origin {
            Origin::Person => true,
            Origin::Portal(handle) => self
                .pending
                .as_ref()
                .is_some_and(|pending| &pending.handle == handle),
        }
    }

    /// The reply of the request `origin` belongs to, taken; `None` for the
    /// person's own captures and for any other request.
    pub fn take_for(&mut self, origin: &Origin) -> Option<T> {
        match origin {
            Origin::Person => None,
            Origin::Portal(_) if self.wants(origin) => {
                self.pending.take().map(|pending| pending.reply)
            }
            Origin::Portal(_) => None,
        }
    }

    /// `Request.Close`, or the caller leaving the bus: the request ends, and
    /// any capture still running for it is dropped (`wants` turns false).
    pub fn close(&mut self, handle: &str) -> Option<T> {
        self.take_for(&Origin::Portal(handle.to_string()))
    }
}

#[cfg(test)]
mod slot_tests {
    use super::{Origin, Outcome, PortalSlot, RequestKind};

    #[test]
    fn a_persons_capture_never_answers_a_pending_request() {
        let mut slot = PortalSlot::default();
        slot.begin("/r/a".into(), RequestKind::PickColor, "reply-a", false)
            .unwrap();
        // Shift+Print finishes while PickColor waits.
        assert_eq!(slot.take_for(&Origin::Person), None);
        assert!(slot.is_pending());
        // Another request's capture does not answer it either.
        assert_eq!(slot.take_for(&Origin::Portal("/r/b".into())), None);
        assert!(slot.wants(&Origin::Person));
        assert!(!slot.wants(&Origin::Portal("/r/b".into())));
        // Its own does.
        assert_eq!(
            slot.take_for(&Origin::Portal("/r/a".into())),
            Some("reply-a")
        );
        assert!(!slot.is_pending());
    }

    #[test]
    fn one_request_at_a_time_and_none_over_a_busy_interface() {
        let mut slot = PortalSlot::default();
        slot.begin("/r/a".into(), RequestKind::Instant, 1, false)
            .unwrap();
        let (reply, refusal) = slot
            .begin("/r/b".into(), RequestKind::Interface, 2, false)
            .unwrap_err();
        assert_eq!(reply, 2);
        assert!(matches!(refusal, Outcome::Refused(_)));
        let mut idle = PortalSlot::default();
        assert!(
            idle.begin("/r/c".into(), RequestKind::Interface, 3, true)
                .is_err()
        );
    }

    #[test]
    fn closing_a_request_drops_the_capture_running_for_it() {
        let mut slot = PortalSlot::default();
        slot.begin("/r/a".into(), RequestKind::Instant, 1, false)
            .unwrap();
        assert_eq!(slot.close("/r/x"), None);
        assert_eq!(slot.close("/r/a"), Some(1));
        assert!(!slot.wants(&Origin::Portal("/r/a".into())));
    }
}

#[cfg(test)]
mod tests {
    use super::{Outcome, Response, color_of, may_start};

    #[test]
    fn outcomes_become_the_portals_numbers() {
        assert_eq!(
            Outcome::Picture("file:///x.png".into()).response().code(),
            0
        );
        assert_eq!(Outcome::Color(0.0, 0.0, 0.0).response(), Response::Success);
        assert_eq!(Outcome::Cancelled.response().code(), 1);
        assert_eq!(Outcome::Refused("locked down".into()).response().code(), 2);
    }

    #[test]
    fn a_pixel_becomes_a_colour() {
        assert_eq!(color_of([0, 0, 255, 255]), (1.0, 0.0, 0.0));
        assert_eq!(color_of([255, 51, 0, 10]), (0.0, 0.2, 1.0));
    }

    #[test]
    fn a_second_request_while_one_shows_is_refused() {
        assert!(may_start(false).is_ok());
        assert_eq!(may_start(true).unwrap_err().response(), Response::Other);
    }
}
