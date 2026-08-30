//! What this session can actually offer the launcher.
//!
//! Issue #2 requires a machine with no five-finger touchpad to reach the
//! launcher with no error state. The way to keep that promise is to answer the
//! question once, here, and let every activation path be present or absent
//! rather than present and failing. Nothing in this module reads an input
//! device, opens a compositor connection, or asks for a privilege; it reads
//! the environment the session already published.

use app_catalog_core::DesktopEnvironments;

use crate::activation::ActivationPath;

/// The display protocol the session is running on.
///
/// This is not a preference. It decides which gesture integration paths ADR
/// 0008 even considers: an X11 session and a Wayland session do not have the
/// same options, and neither of them lets an unprivileged process grab the
/// touchpad.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SessionType {
    Wayland,
    X11,
    /// `XDG_SESSION_TYPE` said something else, or said nothing. Reported
    /// rather than guessed.
    #[default]
    Unknown,
}

/// The shell the session is running, as far as it is willing to say.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ShellKind {
    GnomeShell,
    /// A desktop that named itself but is not GNOME. The name is kept because
    /// a diagnostic that says "not GNOME" is less useful than one that says
    /// which desktop it actually found.
    Other(String),
    #[default]
    Unknown,
}

/// Whether a global keyboard shortcut can exist on this session, and by what
/// route.
///
/// No variant means "the launcher installs a shortcut for you". A normal
/// unprivileged application cannot register a system-wide shortcut on GNOME;
/// what it can do is name the settings that would carry one. Writing them is
/// Better Defaults' job, over its own reviewed boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShortcutAvailability {
    /// GNOME's custom-keybinding settings can carry the shortcut. See
    /// [`crate::shortcut`] for the exact keys.
    GnomeCustomKeybinding,
    /// This session publishes no route this build knows how to describe. The
    /// desktop entry and a second launch still work.
    #[default]
    ManualConfiguration,
}

/// Whether a gesture adapter is attached.
///
/// There is no production adapter in this build, so a real session always
/// reports [`GestureAvailability::NoAdapter`]. The variant carrying a name
/// exists so the overlay's degradation path is exercised from both sides
/// rather than being dead until ticket 30 lands.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum GestureAvailability {
    #[default]
    NoAdapter,
    Adapter(String),
}

impl GestureAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Adapter(_))
    }
}

/// Everything about the session that changes what the launcher can do.
#[derive(Clone, Debug, Default)]
pub struct SessionCapabilities {
    pub session_type: SessionType,
    pub desktops: DesktopEnvironments,
    pub shell: ShellKind,
    pub shortcut: ShortcutAvailability,
    pub gesture: GestureAvailability,
}

impl SessionCapabilities {
    /// Reads the session from the process environment.
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        )
    }

    /// The same detection over supplied values, so every branch is testable
    /// without touching the process environment.
    pub fn from_values(session_type: Option<&str>, current_desktop: Option<&str>) -> Self {
        let session_type = match session_type.unwrap_or_default().trim().to_ascii_lowercase() {
            value if value == "wayland" => SessionType::Wayland,
            value if value == "x11" => SessionType::X11,
            _ => SessionType::Unknown,
        };
        let raw_desktop = current_desktop.unwrap_or_default();
        let desktops = DesktopEnvironments::parse(raw_desktop);
        let shell = match desktops
            .names()
            .iter()
            .find(|name| name.as_str() == "GNOME")
        {
            Some(_) => ShellKind::GnomeShell,
            None => match desktops.names().first() {
                Some(name) => ShellKind::Other(name.clone()),
                None => ShellKind::Unknown,
            },
        };
        let shortcut = match shell {
            ShellKind::GnomeShell => ShortcutAvailability::GnomeCustomKeybinding,
            _ => ShortcutAvailability::ManualConfiguration,
        };
        Self {
            session_type,
            desktops,
            shell,
            shortcut,
            // No production adapter exists. This is the honest answer on every
            // machine this build runs on.
            gesture: GestureAvailability::NoAdapter,
        }
    }

    /// The activation paths that work on this session, most direct first.
    ///
    /// The desktop entry and a second launch are always here: neither depends
    /// on the shell, the display protocol, or the hardware. That is what makes
    /// an unsupported touchpad a shorter list rather than a failure.
    pub fn activation_paths(&self) -> Vec<ActivationPath> {
        let mut paths = Vec::new();
        if self.gesture.is_available() {
            paths.push(ActivationPath::Gesture);
        }
        if self.shortcut == ShortcutAvailability::GnomeCustomKeybinding {
            paths.push(ActivationPath::GlobalShortcut);
        }
        paths.push(ActivationPath::DesktopEntry);
        paths.push(ActivationPath::SecondLaunch);
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gnome_wayland_session_offers_a_shortcut_route_and_no_gesture() {
        let capabilities = SessionCapabilities::from_values(Some("wayland"), Some("ubuntu:GNOME"));
        assert_eq!(capabilities.session_type, SessionType::Wayland);
        assert_eq!(capabilities.shell, ShellKind::GnomeShell);
        assert_eq!(
            capabilities.shortcut,
            ShortcutAvailability::GnomeCustomKeybinding
        );
        assert!(!capabilities.gesture.is_available());
        assert_eq!(
            capabilities.activation_paths(),
            vec![
                ActivationPath::GlobalShortcut,
                ActivationPath::DesktopEntry,
                ActivationPath::SecondLaunch,
            ]
        );
    }

    #[test]
    fn an_unknown_desktop_still_has_two_ways_in_and_reports_no_error() {
        let capabilities = SessionCapabilities::from_values(None, None);
        assert_eq!(capabilities.session_type, SessionType::Unknown);
        assert_eq!(capabilities.shell, ShellKind::Unknown);
        assert_eq!(
            capabilities.shortcut,
            ShortcutAvailability::ManualConfiguration
        );
        let paths = capabilities.activation_paths();
        assert_eq!(
            paths,
            vec![ActivationPath::DesktopEntry, ActivationPath::SecondLaunch]
        );
        assert!(
            !paths.is_empty(),
            "there is always a way to open the launcher"
        );
    }

    #[test]
    fn a_non_gnome_desktop_is_named_rather_than_reported_as_unknown() {
        let capabilities = SessionCapabilities::from_values(Some("x11"), Some("KDE"));
        assert_eq!(capabilities.session_type, SessionType::X11);
        assert_eq!(capabilities.shell, ShellKind::Other("KDE".to_string()));
    }

    #[test]
    fn an_attached_adapter_adds_the_gesture_path_ahead_of_the_others() {
        let mut capabilities = SessionCapabilities::from_values(Some("wayland"), Some("GNOME"));
        capabilities.gesture = GestureAvailability::Adapter("mock".to_string());
        assert_eq!(
            capabilities.activation_paths().first(),
            Some(&ActivationPath::Gesture)
        );
    }
}
