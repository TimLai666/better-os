//! Which desktop this session is, decided in one place.
//!
//! `XDG_CURRENT_DESKTOP` is a colon-separated list by the XDG specification,
//! and a distribution puts its own name first: Zorin's session says
//! `zorin:GNOME`, Ubuntu's `ubuntu:GNOME`, Pop's `pop:GNOME`. A session is
//! GNOME when *any* entry is `GNOME`, never only when the first one is
//! (ticket 134: reading the first entry made every Better Files default
//! integration "not supported on this system" on Zorin).
//!
//! `DESKTOP_SESSION` names the session file the display manager started, not a
//! desktop, so it is read only when `XDG_CURRENT_DESKTOP` says nothing at all.
//!
//! Every Better OS reading of the session's desktop goes through
//! [`CurrentDesktop`]: the default-integration declarations, the touchpad and
//! launcher GNOME checks, and the About line. Matching a desktop entry's
//! `OnlyShowIn`/`NotShowIn` is a different question with its own rule (the
//! desktop-entry specification compares against `XDG_CURRENT_DESKTOP` alone),
//! and stays with the application catalog.

/// The identity a session with no readable desktop is recorded under.
pub const UNKNOWN_DESKTOP: &str = "unknown";

/// The desktop names a session declares, in the order it declared them and
/// spelled as it spelled them. Comparisons ignore ASCII case.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CurrentDesktop {
    names: Vec<String>,
}

impl CurrentDesktop {
    /// Reads this process's own environment.
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the two variables through `lookup`, so a caller with a fixed
    /// environment (or a test) never touches the process's own.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        Self::from_values(
            lookup("XDG_CURRENT_DESKTOP").as_deref(),
            lookup("DESKTOP_SESSION").as_deref(),
        )
    }

    /// `XDG_CURRENT_DESKTOP` first; `DESKTOP_SESSION` only when the list is
    /// absent or holds nothing but separators and blanks.
    pub fn from_values(current_desktop: Option<&str>, desktop_session: Option<&str>) -> Self {
        let listed = Self::parse(current_desktop.unwrap_or_default());
        if listed.is_known() {
            return listed;
        }
        // Some display managers have put the session file's path here rather
        // than its name; the name is the last component either way.
        let session = desktop_session
            .unwrap_or_default()
            .trim()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".desktop")
            .trim();
        Self {
            names: (!session.is_empty())
                .then(|| session.to_string())
                .into_iter()
                .collect(),
        }
    }

    /// Parses the colon-separated `XDG_CURRENT_DESKTOP` form. Blank entries
    /// are dropped; nothing else is changed.
    pub fn parse(list: &str) -> Self {
        Self {
            names: list
                .split(':')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    /// The identity [`identity`](Self::identity) wrote, read back.
    pub fn from_identity(identity: &str) -> Self {
        if identity.trim().eq_ignore_ascii_case(UNKNOWN_DESKTOP) {
            return Self::default();
        }
        Self::parse(identity)
    }

    /// The names, in the session's order and spelling.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Whether the session named any desktop at all.
    pub fn is_known(&self) -> bool {
        !self.names.is_empty()
    }

    /// Whether `name` is one of the session's desktops, ignoring ASCII case.
    pub fn includes(&self, name: &str) -> bool {
        let name = name.trim();
        !name.is_empty() && self.names.iter().any(|own| own.eq_ignore_ascii_case(name))
    }

    /// Whether any declared session is one of this session's desktops. This
    /// is the rule a default-integration declaration's `sessions` list is
    /// checked with.
    pub fn matches_any<S: AsRef<str>>(&self, declared: &[S]) -> bool {
        declared.iter().any(|name| self.includes(name.as_ref()))
    }

    /// Whether this is a GNOME session. Every GNOME flavour lists `GNOME`
    /// itself (`GNOME-Classic:GNOME`, `GNOME-Flashback:GNOME`, `zorin:GNOME`).
    pub fn is_gnome(&self) -> bool {
        self.includes("gnome")
    }

    /// A stable lowercase form for a record (`zorin:gnome`), or
    /// [`UNKNOWN_DESKTOP`] when the session named nothing.
    pub fn identity(&self) -> String {
        if self.names.is_empty() {
            return UNKNOWN_DESKTOP.to_string();
        }
        self.names
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(":")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(current: Option<&str>, session: Option<&str>) -> CurrentDesktop {
        CurrentDesktop::from_values(current, session)
    }

    #[test]
    fn zorin_lists_its_own_name_first_and_is_still_gnome() {
        let desktop = from(Some("zorin:GNOME"), Some("zorin"));
        assert!(desktop.is_gnome());
        assert!(desktop.matches_any(&["gnome"]));
        assert!(desktop.matches_any(&["Zorin"]));
        assert_eq!(desktop.names(), ["zorin", "GNOME"]);
        assert_eq!(desktop.identity(), "zorin:gnome");
    }

    #[test]
    fn ubuntu_lists_its_own_name_first_and_is_still_gnome() {
        let desktop = from(Some("ubuntu:GNOME"), Some("ubuntu"));
        assert!(desktop.is_gnome());
        assert!(desktop.matches_any(&["GNOME"]));
    }

    #[test]
    fn plain_gnome_is_gnome() {
        let desktop = from(Some("GNOME"), None);
        assert!(desktop.is_gnome());
        assert_eq!(desktop.identity(), "gnome");
    }

    #[test]
    fn an_empty_session_matches_nothing_and_says_it_is_unknown() {
        for current in [None, Some(""), Some("  "), Some("::")] {
            let desktop = from(current, None);
            assert!(!desktop.is_known());
            assert!(!desktop.is_gnome());
            assert!(!desktop.matches_any(&["gnome", "unknown", ""]));
            assert_eq!(desktop.identity(), UNKNOWN_DESKTOP);
        }
    }

    #[test]
    fn kde_is_not_gnome() {
        let desktop = from(Some("KDE"), Some("plasma"));
        assert!(!desktop.is_gnome());
        assert!(!desktop.matches_any(&["gnome"]));
        assert!(desktop.matches_any(&["kde"]));
        // The fallback is not consulted when the list said something.
        assert!(!desktop.includes("plasma"));
    }

    #[test]
    fn desktop_session_is_only_a_fallback() {
        assert!(from(None, Some("gnome")).is_gnome());
        assert!(from(Some(""), Some("GNOME")).is_gnome());
        assert!(from(None, Some("/usr/share/xsessions/gnome.desktop")).is_gnome());
        assert!(!from(None, Some("ubuntu")).is_gnome());
        assert!(!from(None, Some("  ")).is_known());
    }

    #[test]
    fn blank_entries_and_spaces_are_ignored() {
        let desktop = from(Some(" zorin : : GNOME "), None);
        assert_eq!(desktop.names(), ["zorin", "GNOME"]);
        assert!(!desktop.includes(""));
    }

    #[test]
    fn a_recorded_identity_reads_back_as_the_same_session() {
        for current in ["zorin:GNOME", "GNOME", "KDE", ""] {
            let desktop = from(Some(current), None);
            let read = CurrentDesktop::from_identity(&desktop.identity());
            assert_eq!(read.identity(), desktop.identity());
            assert_eq!(read.is_gnome(), desktop.is_gnome());
        }
        assert!(!CurrentDesktop::from_identity("unknown").is_known());
    }
}
