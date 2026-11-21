//! The session menu (design/login-screen-spec.md sections 3.6 and 5.6).
//!
//! Every `.desktop` file in `/usr/share/wayland-sessions/` is a session,
//! parsed by the project's one desktop entry parser
//! (`app_catalog_core::entry`) as untrusted input: `Name` and `Name[zh_TW]`,
//! `DesktopNames`, `Exec`; `Hidden` and `NoDisplay` entries are left out.
//! Better OS is first, the rest by name.
//!
//! A GNOME session (its `Exec` runs `gnome-session`, as Zorin's two entries
//! do through `env`) can lock only through the GNOME unlock service (L2 (a)),
//! so it is listed only while that service answers, and the greeter checks
//! again right before starting it; [`SessionFamily::Gnome`] marks it for both
//! and for the refusal's copy.

use std::path::Path;

use app_catalog_core::error::MAX_ENTRY_BYTES;
use app_catalog_core::exec::{ArgumentPiece, ExecLine};
use app_catalog_core::{DesktopFile, EntryError, Locale, LocalizedText};
use better_core::desktop::{BETTER_OS_DESKTOP, BETTER_OS_SESSION_NAME};

use crate::copy::Language;

/// Where Wayland sessions are listed.
pub const WAYLAND_SESSIONS: &str = "/usr/share/wayland-sessions";

/// What kind of session an entry starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionFamily {
    /// The Better OS session (`better-os.desktop`, `DesktopNames=BetterOS`).
    BetterOs,
    /// A `gnome-session` session: offered only while the GNOME unlock
    /// service answers (L2 (a)).
    Gnome,
    Other,
}

/// One session the menu can offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    /// The file's name without `.desktop`: what AccountsService's `Session`
    /// and `start_session`'s environment carry.
    pub id: String,
    name: LocalizedText,
    pub desktop_names: Vec<String>,
    /// The argument vector greetd starts, from `Exec`.
    pub command: Vec<String>,
    pub family: SessionFamily,
}

/// Why an entry is not offered.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Entry(#[from] EntryError),
    #[error("the session's Exec takes a file or a URI, which a session cannot")]
    FieldCodeInExec,
}

impl Session {
    /// The name in the screen's language: `Name[zh_TW]` when the entry has
    /// one, else `Name`.
    pub fn name(&self, language: Language) -> &str {
        let locale = match language {
            Language::ZhTw => Locale::parse("zh_TW"),
            Language::EnUs => Locale::parse("en_US"),
        };
        self.name.resolve(locale.as_ref())
    }

    /// Whether it may be started only while the GNOME unlock service answers.
    pub fn needs_gnome_unlock(&self) -> bool {
        self.family == SessionFamily::Gnome
    }
}

fn is_gnome_session(command: &[String]) -> bool {
    command.iter().any(|argument| {
        Path::new(argument)
            .file_name()
            .is_some_and(|name| name == "gnome-session")
    })
}

/// Parses one session file. `Ok(None)` is an entry that is `Hidden` or
/// `NoDisplay`.
pub fn parse_session(id: &str, bytes: &[u8]) -> Result<Option<Session>, SessionError> {
    let file = DesktopFile::parse_bytes(bytes)?;
    let group = file.desktop_entry()?;
    if group.boolean("Hidden")?.unwrap_or(false) || group.boolean("NoDisplay")?.unwrap_or(false) {
        return Ok(None);
    }
    let name = group
        .localized("Name")
        .ok_or(EntryError::MissingField("Name"))?;
    let exec = group
        .value("Exec")
        .ok_or(EntryError::MissingField("Exec"))?;
    let line = ExecLine::parse(&exec)?;
    let mut command = Vec::new();
    for argument in line.arguments() {
        let mut text = String::new();
        for piece in argument {
            match piece {
                ArgumentPiece::Literal(literal) => text.push_str(literal),
                ArgumentPiece::Field(_) => return Err(SessionError::FieldCodeInExec),
            }
        }
        command.push(text);
    }
    let desktop_names = group.list("DesktopNames").unwrap_or_default();
    let family = if id == BETTER_OS_SESSION_NAME
        || desktop_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(BETTER_OS_DESKTOP))
    {
        SessionFamily::BetterOs
    } else if is_gnome_session(&command) {
        SessionFamily::Gnome
    } else {
        SessionFamily::Other
    };
    Ok(Some(Session {
        id: id.to_string(),
        name,
        desktop_names,
        command,
        family,
    }))
}

/// Better OS first, then by the untranslated name, then by id.
pub fn order_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|left, right| {
        (left.family != SessionFamily::BetterOs)
            .cmp(&(right.family != SessionFamily::BetterOs))
            .then_with(|| {
                left.name
                    .default_value()
                    .to_lowercase()
                    .cmp(&right.name.default_value().to_lowercase())
            })
            .then_with(|| left.id.cmp(&right.id))
    });
}

/// What reading a sessions directory found.
#[derive(Debug, Default)]
pub struct SessionScan {
    /// In the menu's order.
    pub sessions: Vec<Session>,
    /// Files that are not offered, with the reason (for the journal).
    pub rejected: Vec<(String, SessionError)>,
}

/// Reads every `*.desktop` file in `directory`. A directory that cannot be
/// read gives an empty scan; the greeter then offers no session and says so.
pub fn read_sessions(directory: &Path) -> std::io::Result<SessionScan> {
    use std::io::Read;

    let mut scan = SessionScan::default();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(id) = file_name.strip_suffix(".desktop") else {
            continue;
        };
        if id.is_empty() || !entry.file_type()?.is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        let read = std::fs::File::open(entry.path()).and_then(|file| {
            file.take(MAX_ENTRY_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        });
        if let Err(error) = read {
            scan.rejected.push((
                file_name.to_string(),
                SessionError::Entry(EntryError::Unreadable(error.to_string())),
            ));
            continue;
        }
        match parse_session(id, &bytes) {
            Ok(Some(session)) => scan.sessions.push(session),
            Ok(None) => {}
            Err(error) => scan.rejected.push((file_name.to_string(), error)),
        }
    }
    order_sessions(&mut scan.sessions);
    Ok(scan)
}

/// The sessions the menu lists now: GNOME's only while the unlock service
/// answers.
pub fn menu_sessions(sessions: &[Session], gnome_unlock_answers: bool) -> Vec<&Session> {
    sessions
        .iter()
        .filter(|session| gnome_unlock_answers || !session.needs_gnome_unlock())
        .collect()
}

/// The session preselected for an account: its last one when the menu lists
/// it, else Better OS, else the first listed.
pub fn preselected_session<'a>(
    listed: &[&'a Session],
    last_used: Option<&str>,
) -> Option<&'a Session> {
    last_used
        .filter(|id| !id.is_empty())
        .and_then(|id| listed.iter().find(|session| session.id == id))
        .or_else(|| {
            listed
                .iter()
                .find(|session| session.family == SessionFamily::BetterOs)
        })
        .or_else(|| listed.first())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BETTER_OS: &str = "[Desktop Entry]\nName=Better OS\nComment=Better OS on COSMIC\nExec=/usr/bin/better-os-session\nType=Application\nDesktopNames=BetterOS;COSMIC;\n";
    const ZORIN: &str = "[Desktop Entry]\nName=Zorin OS\nName[zh_TW]=Zorin OS 桌面\nExec=env GNOME_SHELL_SESSION_MODE=zorin /usr/bin/gnome-session --session=zorin\nType=Application\nDesktopNames=zorin:GNOME\n";
    const SWAY: &str = "[Desktop Entry]\nName=Sway\nExec=sway\nType=Application\n";

    fn session(id: &str, text: &str) -> Session {
        parse_session(id, text.as_bytes()).unwrap().unwrap()
    }

    #[test]
    fn a_zorin_entry_is_a_gnome_session_with_its_translation() {
        let zorin = session("zorin", ZORIN);
        assert_eq!(zorin.family, SessionFamily::Gnome);
        assert!(zorin.needs_gnome_unlock());
        assert_eq!(zorin.name(Language::ZhTw), "Zorin OS 桌面");
        assert_eq!(zorin.name(Language::EnUs), "Zorin OS");
        assert_eq!(
            zorin.command,
            [
                "env",
                "GNOME_SHELL_SESSION_MODE=zorin",
                "/usr/bin/gnome-session",
                "--session=zorin"
            ]
        );
        assert_eq!(zorin.desktop_names, ["zorin:GNOME"]);
    }

    #[test]
    fn better_os_is_recognised_and_first() {
        let mut sessions = vec![
            session("sway", SWAY),
            session("zorin", ZORIN),
            session("better-os", BETTER_OS),
        ];
        order_sessions(&mut sessions);
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["better-os", "sway", "zorin"]);
        assert_eq!(sessions[0].family, SessionFamily::BetterOs);
        assert_eq!(sessions[1].family, SessionFamily::Other);
        // A session with no translation shows its Name in both languages.
        assert_eq!(sessions[1].name(Language::ZhTw), "Sway");
    }

    #[test]
    fn hidden_and_no_display_entries_are_left_out() {
        let hidden = "[Desktop Entry]\nName=Old\nExec=old\nHidden=true\n";
        let no_display = "[Desktop Entry]\nName=Old\nExec=old\nNoDisplay=true\n";
        assert_eq!(parse_session("old", hidden.as_bytes()).unwrap(), None);
        assert_eq!(parse_session("old", no_display.as_bytes()).unwrap(), None);
    }

    #[test]
    fn a_broken_entry_is_rejected_not_guessed() {
        assert!(parse_session("x", b"[Desktop Entry]\nExec=x\n").is_err());
        assert!(parse_session("x", b"[Desktop Entry]\nName=X\n").is_err());
        assert_eq!(
            parse_session("x", b"[Desktop Entry]\nName=X\nExec=x %f\n"),
            Err(SessionError::FieldCodeInExec)
        );
        assert!(parse_session("x", b"[Desktop Entry]\nName=X\nExec=x\nHidden=yes\n").is_err());
        assert!(parse_session("x", &[0xff, 0xfe]).is_err());
    }

    #[test]
    fn gnome_is_listed_only_while_the_unlock_service_answers() {
        let mut sessions = vec![
            session("better-os", BETTER_OS),
            session("zorin", ZORIN),
            session("sway", SWAY),
        ];
        order_sessions(&mut sessions);
        let with: Vec<&str> = menu_sessions(&sessions, true)
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(with, ["better-os", "sway", "zorin"]);
        let without: Vec<&str> = menu_sessions(&sessions, false)
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(without, ["better-os", "sway"]);
    }

    #[test]
    fn the_preselected_session() {
        let sessions = vec![
            session("better-os", BETTER_OS),
            session("zorin", ZORIN),
            session("sway", SWAY),
        ];
        let listed = menu_sessions(&sessions, true);
        assert_eq!(
            preselected_session(&listed, Some("sway")).unwrap().id,
            "sway"
        );
        assert_eq!(
            preselected_session(&listed, Some("gone")).unwrap().id,
            "better-os"
        );
        assert_eq!(preselected_session(&listed, None).unwrap().id, "better-os");
        // A last-used GNOME session the menu cannot list now is not chosen.
        let without_gnome = menu_sessions(&sessions, false);
        assert_eq!(
            preselected_session(&without_gnome, Some("zorin"))
                .unwrap()
                .id,
            "better-os"
        );
        let only_sway = vec![session("sway", SWAY)];
        assert_eq!(
            preselected_session(&menu_sessions(&only_sway, true), None)
                .unwrap()
                .id,
            "sway"
        );
        assert!(preselected_session(&[], Some("sway")).is_none());
    }

    #[test]
    fn a_directory_is_read_in_the_menus_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zorin.desktop"), ZORIN).unwrap();
        std::fs::write(dir.path().join("better-os.desktop"), BETTER_OS).unwrap();
        std::fs::write(dir.path().join("broken.desktop"), "not an entry").unwrap();
        std::fs::write(dir.path().join("README"), "ignored").unwrap();
        std::fs::create_dir(dir.path().join("dir.desktop")).unwrap();
        let scan = read_sessions(dir.path()).unwrap();
        let ids: Vec<&str> = scan.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["better-os", "zorin"]);
        assert_eq!(scan.rejected.len(), 1);
        assert_eq!(scan.rejected[0].0, "broken.desktop");
        assert!(read_sessions(&dir.path().join("missing")).is_err());
    }
}
