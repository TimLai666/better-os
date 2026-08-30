//! Which session this is.
//!
//! The answer comes from the environment the process was started in, because
//! that is where the session type actually is. There is no `loginctl` call and
//! no display connection: an environment read cannot fail slowly, cannot hang,
//! and cannot be the reason a control centre takes a second to open.
//!
//! Every variable is read through a lookup function, so the tests drive real
//! combinations rather than the developer's own session.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionKind {
    Wayland,
    X11,
    /// Neither variable said anything usable. Reported rather than guessed,
    /// because a wrong guess here changes what the whole application claims it
    /// can do.
    Unknown,
}

impl SessionKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub kind: SessionKind,
    /// The desktop names `XDG_CURRENT_DESKTOP` lists, lowercased.
    pub desktops: Vec<String>,
    /// Whether a session bus address was in the environment at all. Without
    /// one, the GNOME backend cannot write.
    pub has_session_bus: bool,
}

impl Session {
    /// Reads the session out of this process's own environment.
    pub fn detect() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let non_empty = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());

        let declared = non_empty("XDG_SESSION_TYPE").unwrap_or_default();
        let kind = match declared.to_ascii_lowercase().as_str() {
            "wayland" => SessionKind::Wayland,
            "x11" => SessionKind::X11,
            // The declared type is the first answer, but it is missing under
            // some session managers. A Wayland display socket is stronger
            // evidence than a DISPLAY variable, because XWayland sets both.
            _ if non_empty("WAYLAND_DISPLAY").is_some() => SessionKind::Wayland,
            _ if non_empty("DISPLAY").is_some() => SessionKind::X11,
            _ => SessionKind::Unknown,
        };

        let desktops = non_empty("XDG_CURRENT_DESKTOP")
            .map(|value| {
                value
                    .split(':')
                    .map(|name| name.trim().to_ascii_lowercase())
                    .filter(|name| !name.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            kind,
            desktops,
            has_session_bus: non_empty("DBUS_SESSION_BUS_ADDRESS").is_some(),
        }
    }

    /// Whether this is a session the GNOME backend applies to. Zorin's own
    /// session lists `zorin:GNOME`, so a plain equality check would miss it.
    pub fn is_gnome(&self) -> bool {
        self.desktops
            .iter()
            .any(|desktop| desktop == "gnome" || desktop.starts_with("gnome-"))
    }

    /// A one-line description for the Overview and Diagnostics screens.
    pub fn describe(&self) -> String {
        let desktop = if self.desktops.is_empty() {
            "unknown desktop".to_string()
        } else {
            self.desktops.join(", ")
        };
        format!("{} / {desktop}", self.kind.key())
    }
}

/// A fixed environment, for tests and for a caller that has one to hand.
pub fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect();
    move |name: &str| map.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_wayland_session_is_taken_at_its_word() {
        let session = Session::from_lookup(lookup_from(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("XDG_CURRENT_DESKTOP", "zorin:GNOME"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
        ]));
        assert_eq!(session.kind, SessionKind::Wayland);
        assert!(session.is_gnome());
        assert!(session.has_session_bus);
        assert_eq!(session.describe(), "wayland / zorin, gnome");
    }

    #[test]
    fn a_declared_x11_session_is_x11_even_with_a_wayland_socket_around() {
        let session = Session::from_lookup(lookup_from(&[
            ("XDG_SESSION_TYPE", "x11"),
            ("WAYLAND_DISPLAY", "wayland-0"),
        ]));
        assert_eq!(session.kind, SessionKind::X11);
    }

    #[test]
    fn an_undeclared_session_with_a_wayland_socket_is_wayland_not_x11() {
        // XWayland sets DISPLAY too, so the display variable alone would call
        // a Wayland session X11.
        let session = Session::from_lookup(lookup_from(&[
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0"),
        ]));
        assert_eq!(session.kind, SessionKind::Wayland);
    }

    #[test]
    fn an_undeclared_session_with_only_a_display_is_x11() {
        let session = Session::from_lookup(lookup_from(&[("DISPLAY", ":0")]));
        assert_eq!(session.kind, SessionKind::X11);
    }

    #[test]
    fn an_empty_variable_counts_as_absent_rather_than_as_an_answer() {
        let session = Session::from_lookup(lookup_from(&[
            ("XDG_SESSION_TYPE", "  "),
            ("WAYLAND_DISPLAY", ""),
            ("DISPLAY", ""),
            ("XDG_CURRENT_DESKTOP", ""),
        ]));
        assert_eq!(session.kind, SessionKind::Unknown);
        assert!(session.desktops.is_empty());
        assert!(!session.is_gnome());
        assert_eq!(session.describe(), "unknown / unknown desktop");
    }

    #[test]
    fn a_desktop_that_is_not_gnome_is_not_treated_as_one() {
        let session = Session::from_lookup(lookup_from(&[("XDG_CURRENT_DESKTOP", "KDE")]));
        assert!(!session.is_gnome());
    }

    #[test]
    fn a_gnome_flavour_still_counts_as_gnome() {
        let session = Session::from_lookup(lookup_from(&[(
            "XDG_CURRENT_DESKTOP",
            "GNOME-Classic:GNOME",
        )]));
        assert!(session.is_gnome());
    }
}
