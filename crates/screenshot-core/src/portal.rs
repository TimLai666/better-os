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

/// The portal request in flight and who the interface answers for: every
/// way an interface or a request ends goes through here, so a pending request
/// is answered exactly once and the slot is never left taken (a taken slot
/// refuses every later call and keeps the process from leaving).
///
/// The interface owes an answer while it is opening, open, or fading out,
/// until its picture is handed to be stored ([`Requests::handed_over`]):
/// from then on the store answers, and a close during the fade out no longer
/// says "cancelled" for a picture the person took.
#[derive(Debug)]
pub struct Requests<T> {
    slot: PortalSlot<T>,
    interface: Option<Origin>,
}

impl<T> Default for Requests<T> {
    fn default() -> Self {
        Requests {
            slot: PortalSlot::default(),
            interface: None,
        }
    }
}

/// An answer to send: the reply and what it says.
pub type Answer<T> = (T, Outcome);

impl<T> Requests<T> {
    /// A new portal call. Refused while another is pending or the screen is
    /// busy, and answered "cancelled" while the session is locked (O9).
    pub fn begin(
        &mut self,
        handle: String,
        kind: RequestKind,
        reply: T,
        busy: bool,
        locked: bool,
    ) -> Result<Origin, Answer<T>> {
        if locked {
            return Err((reply, Outcome::Cancelled));
        }
        self.slot.begin(handle.clone(), kind, reply, busy)?;
        Ok(Origin::Portal(handle))
    }

    pub fn is_pending(&self) -> bool {
        self.slot.is_pending()
    }

    pub fn wants(&self, origin: &Origin) -> bool {
        self.slot.wants(origin)
    }

    /// An interface starts opening for `origin`.
    pub fn interface_opening(&mut self, origin: Origin) {
        self.interface = Some(origin);
    }

    /// Who the interface answers for now, if anyone.
    pub fn interface_owner(&self) -> Option<&Origin> {
        self.interface.as_ref()
    }

    /// Whether the interface `origin` asked for may still be shown once its
    /// screen copy is back: it was not ended meanwhile, its request is still
    /// pending, and the session is not locked. When it may not, the caller
    /// ends it with [`Requests::interface_ended`].
    pub fn may_show(&self, origin: &Origin, locked: bool) -> bool {
        !locked && self.interface.as_ref() == Some(origin) && self.slot.wants(origin)
    }

    /// The picture is handed to be stored: the store answers for it from
    /// now on. Returns whose picture it is.
    pub fn handed_over(&mut self) -> Origin {
        self.interface.take().unwrap_or(Origin::Person)
    }

    /// The interface ended without a picture handed over (Escape, close,
    /// Close, a lock, an output change, a failed open, a fade's end): its
    /// request, if still pending, hears "cancelled".
    pub fn interface_ended(&mut self) -> Option<Answer<T>> {
        let origin = self.interface.take()?;
        self.answer(&origin, Outcome::Cancelled)
    }

    /// Answers `origin`'s request with `outcome`, if it is still pending.
    pub fn answer(&mut self, origin: &Origin, outcome: Outcome) -> Option<Answer<T>> {
        self.slot.take_for(origin).map(|reply| (reply, outcome))
    }

    /// `Request.Close` or the caller leaving: "cancelled", once.
    pub fn close(&mut self, handle: &str) -> Option<Answer<T>> {
        self.slot
            .close(handle)
            .map(|reply| (reply, Outcome::Cancelled))
    }
}

#[cfg(test)]
mod request_tests {
    use super::{Answer, Origin, Outcome, RequestKind, Requests};

    /// What a run sent: each answer with its request.
    struct Run {
        requests: Requests<&'static str>,
        sent: Vec<(&'static str, Outcome)>,
    }

    impl Run {
        fn new() -> Run {
            Run {
                requests: Requests::default(),
                sent: Vec::new(),
            }
        }

        fn send(&mut self, answer: Option<Answer<&'static str>>) {
            if let Some(answer) = answer {
                self.sent.push(answer);
            }
        }

        /// A portal interface call, its interface opening.
        fn open(&mut self, kind: RequestKind) -> Origin {
            let origin = self
                .requests
                .begin("/r/a".into(), kind, "a", false, false)
                .unwrap();
            self.requests.interface_opening(origin.clone());
            origin
        }

        fn finished(self, want: Outcome) {
            assert!(!self.requests.is_pending(), "the slot is still taken");
            assert_eq!(self.requests.interface_owner(), None);
            assert_eq!(self.sent, vec![("a", want)], "exactly one answer");
        }
    }

    fn picture() -> Outcome {
        Outcome::Picture("file:///p.png".into())
    }

    #[test]
    fn every_way_an_interface_ends_answers_once_and_frees_the_slot() {
        // Done: the picture is handed over, the fade ends, the store answers.
        let mut run = Run::new();
        let origin = run.open(RequestKind::Interface);
        assert!(run.requests.may_show(&origin, false));
        let theirs = run.requests.handed_over();
        let ended = run.requests.interface_ended();
        run.send(ended);
        let stored = run.requests.answer(&theirs, picture());
        run.send(stored);
        run.finished(picture());

        // Done, then anything that closes the interface during the fade out
        // (Print, an output change, a lock): still the picture.
        let mut run = Run::new();
        run.open(RequestKind::Interface);
        let theirs = run.requests.handed_over();
        let ended = run.requests.interface_ended();
        run.send(ended);
        let stored = run.requests.answer(&theirs, picture());
        run.send(stored);
        run.finished(picture());

        // PickColor done: the colour, then the interface ends.
        let mut run = Run::new();
        let origin = run.open(RequestKind::PickColor);
        let colour = run.requests.answer(&origin, Outcome::Color(1.0, 0.0, 0.0));
        run.send(colour);
        let ended = run.requests.interface_ended();
        run.send(ended);
        run.finished(Outcome::Color(1.0, 0.0, 0.0));

        // Failed: the screen copy failed.
        let mut run = Run::new();
        let origin = run.open(RequestKind::Interface);
        let refused = run.requests.answer(&origin, Outcome::Refused("no".into()));
        run.send(refused);
        let ended = run.requests.interface_ended();
        run.send(ended);
        run.finished(Outcome::Refused("no".into()));

        // Cancelled (Escape or the close button), an output change, a lock
        // while open: the interface ends.
        let mut run = Run::new();
        run.open(RequestKind::Interface);
        let ended = run.requests.interface_ended();
        run.send(ended);
        let again = run.requests.interface_ended();
        run.send(again);
        run.finished(Outcome::Cancelled);

        // Close, or the caller leaving, then the interface ends.
        let mut run = Run::new();
        run.open(RequestKind::Interface);
        let closed = run.requests.close("/r/a");
        run.send(closed);
        let ended = run.requests.interface_ended();
        run.send(ended);
        run.finished(Outcome::Cancelled);

        // Close while the screen copy is still being made: it is not shown.
        let mut run = Run::new();
        let origin = run.open(RequestKind::Interface);
        let closed = run.requests.close("/r/a");
        run.send(closed);
        assert!(!run.requests.may_show(&origin, false));
        let ended = run.requests.interface_ended();
        run.send(ended);
        run.finished(Outcome::Cancelled);

        // A lock while the interface is opening: answered at the lock, and
        // the copy that comes back is not shown.
        let mut run = Run::new();
        let origin = run.open(RequestKind::PickColor);
        let ended = run.requests.interface_ended();
        run.send(ended);
        assert!(!run.requests.may_show(&origin, true));
        assert!(!run.requests.may_show(&origin, false));
        run.finished(Outcome::Cancelled);

        // A lock that the copy's return sees first (`LockedHint` arrived
        // between): not shown, and ended there.
        let mut run = Run::new();
        let origin = run.open(RequestKind::Interface);
        assert!(!run.requests.may_show(&origin, true));
        let ended = run.requests.interface_ended();
        run.send(ended);
        run.finished(Outcome::Cancelled);

        // Print during the fade out of a portal's interface: it finishes
        // closing (cancelled, nothing was taken) and the person's opens.
        let mut run = Run::new();
        run.open(RequestKind::Interface);
        let ended = run.requests.interface_ended();
        run.send(ended);
        run.requests.interface_opening(Origin::Person);
        let persons = run.requests.interface_ended();
        assert!(persons.is_none());
        run.finished(Outcome::Cancelled);
    }

    #[test]
    fn a_call_while_locked_is_cancelled_and_takes_no_slot() {
        let mut requests: Requests<&str> = Requests::default();
        for kind in [
            RequestKind::Interface,
            RequestKind::Instant,
            RequestKind::PickColor,
        ] {
            let (reply, outcome) = requests
                .begin("/r/a".into(), kind, "a", false, true)
                .unwrap_err();
            assert_eq!((reply, outcome), ("a", Outcome::Cancelled));
            assert!(!requests.is_pending());
        }
        // An instant picture closed while it is taken writes nothing and is
        // answered once.
        let origin = requests
            .begin("/r/b".into(), RequestKind::Instant, "b", false, false)
            .unwrap();
        assert_eq!(requests.close("/r/b"), Some(("b", Outcome::Cancelled)));
        assert!(!requests.wants(&origin));
        assert_eq!(requests.answer(&origin, Outcome::Picture("x".into())), None);
        assert!(!requests.is_pending());
    }

    #[test]
    fn the_persons_interface_answers_nothing() {
        let mut requests: Requests<&str> = Requests::default();
        let portal = requests
            .begin("/r/a".into(), RequestKind::Instant, "a", false, false)
            .unwrap();
        requests.interface_opening(Origin::Person);
        assert!(requests.may_show(&Origin::Person, false));
        assert_eq!(requests.interface_ended(), None);
        assert_eq!(requests.answer(&Origin::Person, Outcome::Cancelled), None);
        assert!(requests.wants(&portal));
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
