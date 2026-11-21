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

#[cfg(test)]
mod tests {
    use super::{Outcome, Response, color_of, may_start};

    #[test]
    fn outcomes_become_the_portals_numbers() {
        assert_eq!(Outcome::Picture("file:///x.png".into()).response().code(), 0);
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
