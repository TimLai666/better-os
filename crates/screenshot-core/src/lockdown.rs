//! What "saving to disk is disabled" does (spec S6, A5): GNOME 46's
//! `org.gnome.desktop.lockdown disable-save-to-disk`, read from its source.
//!
//! - A picture from the interface, Shift+Print, or Alt+Print still goes to
//!   the clipboard and still raises the notification; it is not saved, not
//!   added to the recently used files, and the notification has no "Show in
//!   Files" and opens nothing (`_storeScreenshot`, `js/ui/screenshot.js`
//!   2267–2272 and 2343).
//! - The interface therefore has no file to hand the portal's interactive
//!   call, which hears that nothing was taken (`InteractiveScreenshotAsync`
//!   2681–2690: `screenshot-taken` is emitted only with a file, 1951).
//! - The non-interactive calls that save are refused before anything is
//!   copied (`_createScreenshot` 2445–2457, `needsDisk`), so no flash;
//!   `PickColor` saves nothing and is allowed (2746).
//! - A recording is refused at its start (`ScreencastAsync`,
//!   `screencastService.js` 626–631), which the interface reports as
//!   "Screencast failed to start" (`_screencastFailed` 2058–2063).

/// What a request may do while saving to disk is disabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Allowed {
    /// Go ahead as usual.
    Everything,
    /// Copy to the clipboard and notify, but save nothing.
    ClipboardOnly,
    /// Refuse before copying anything.
    Refused,
}

/// What can be asked of Better Screenshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Asked {
    /// A picture from the interface, Shift+Print, or Alt+Print.
    Picture,
    /// The portal's non-interactive `Screenshot`.
    PortalInstantPicture,
    /// The portal's `PickColor`.
    PickColor,
    /// A recording.
    Recording,
}

pub fn allowed(asked: Asked, saving_disabled: bool) -> Allowed {
    if !saving_disabled {
        return Allowed::Everything;
    }
    match asked {
        Asked::Picture => Allowed::ClipboardOnly,
        Asked::PickColor => Allowed::Everything,
        Asked::PortalInstantPicture | Asked::Recording => Allowed::Refused,
    }
}

/// The dconf key.
pub const KEY: &str = "/org/gnome/desktop/lockdown/disable-save-to-disk";

#[cfg(test)]
mod tests {
    use super::{Allowed, Asked, KEY, allowed};

    #[test]
    fn nothing_changes_while_saving_is_allowed() {
        for asked in [
            Asked::Picture,
            Asked::PortalInstantPicture,
            Asked::PickColor,
            Asked::Recording,
        ] {
            assert_eq!(allowed(asked, false), Allowed::Everything);
        }
    }

    #[test]
    fn locked_down_pictures_are_copied_only_and_saving_requests_are_refused() {
        assert_eq!(allowed(Asked::Picture, true), Allowed::ClipboardOnly);
        assert_eq!(allowed(Asked::PortalInstantPicture, true), Allowed::Refused);
        assert_eq!(allowed(Asked::Recording, true), Allowed::Refused);
        assert_eq!(allowed(Asked::PickColor, true), Allowed::Everything);
        assert_eq!(KEY, "/org/gnome/desktop/lockdown/disable-save-to-disk");
    }
}
