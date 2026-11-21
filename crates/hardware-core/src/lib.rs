//! What a machine has and what each System page card shows (ticket 86,
//! `design/control-center-spec.md` sections 4 and 11).
//!
//! No GPUI, no I/O, and no words: `hardware-platform` reads the machine into
//! the facts here, and the window turns the decisions here into its own copy.
//! The one exception is [`charge_state`], the battery's state with its icon
//! and words, which every battery icon in the suite shares (ticket 170).
//! Every rule a card follows (when it is drawn, when it is disabled, which
//! choices it offers, which reading is shown) lives in this crate so the
//! window, Home, the daemon, and the session service all ask the same place.

pub mod battery;
pub mod charge;
pub mod charge_state;
pub mod display;
pub mod fan;
pub mod follow;
pub mod lamparray;
pub mod lighting;
pub mod page;
pub mod power;
pub mod sensors;
pub mod usb_recovery;
pub mod write;

/// The variable a debug build of a window reads a fixture sysfs tree from,
/// laid out as `<root>/sys`, for screenshots in a sandbox
/// (`hardware_platform::sysfs::SysRoot::for_window`, and Better Monitor's
/// in-window collector). A release build never reads it.
pub const DEBUG_SYS_ROOT_VARIABLE: &str = "BETTER_OS_DEBUG_SYS_ROOT";

/// What a machine says about one control.
///
/// `Absent` means the hardware is not there, and the card is not drawn.
/// `Unreadable` means it is there and its state could not be read; the card
/// is drawn disabled and the reason goes into the error details. A value that
/// is missing is never turned into a plausible default.
#[derive(Clone, Debug, PartialEq)]
pub enum Presence<T> {
    Absent,
    Unreadable(String),
    Present(T),
}

impl<T> Presence<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Presence::Absent)
    }

    pub fn value(&self) -> Option<&T> {
        match self {
            Presence::Present(value) => Some(value),
            _ => None,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Presence<U> {
        match self {
            Presence::Absent => Presence::Absent,
            Presence::Unreadable(reason) => Presence::Unreadable(reason),
            Presence::Present(value) => Presence::Present(f(value)),
        }
    }
}

/// How a card is drawn, decided from a [`Presence`].
#[derive(Clone, Debug, PartialEq)]
pub enum Card<T> {
    /// Not drawn at all.
    Hidden,
    /// Drawn disabled, with "This PC has this, but its state can't be read
    /// right now." and these details.
    Disabled {
        details: String,
    },
    Live(T),
}

impl<T> Card<T> {
    pub fn is_drawn(&self) -> bool {
        !matches!(self, Card::Hidden)
    }
}

impl<T> From<Presence<T>> for Card<T> {
    fn from(presence: Presence<T>) -> Self {
        match presence {
            Presence::Absent => Card::Hidden,
            Presence::Unreadable(details) => Card::Disabled { details },
            Presence::Present(value) => Card::Live(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_hides_unreadable_disables_and_present_is_live() {
        assert_eq!(Card::from(Presence::<u8>::Absent), Card::Hidden);
        assert_eq!(
            Card::from(Presence::<u8>::Unreadable("EIO".into())),
            Card::Disabled {
                details: "EIO".into()
            }
        );
        assert_eq!(Card::from(Presence::Present(3u8)), Card::Live(3));
        assert!(!Card::<u8>::Hidden.is_drawn());
        assert!(
            Card::<u8>::Disabled {
                details: String::new()
            }
            .is_drawn()
        );
    }
}
