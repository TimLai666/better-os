//! Reading the application out of a systemd application unit's name.
//!
//! A desktop puts each application it starts into a unit of its own, and the
//! name of that unit is the only record of which application it is. The
//! scheme is systemd's "Desktop Environments" document,
//! <https://systemd.io/DESKTOP_ENVIRONMENTS/>, section "XDG standardization
//! for applications" (read 2026-11-21):
//!
//! - `app[-<launcher>]-<ApplicationID>[@<RANDOM>].service`
//! - `app[-<launcher>]-<ApplicationID>-<RANDOM>.scope`
//!
//! `<RANDOM>` tells two launches apart; it may be left out of a service and is
//! mandatory for a scope, "as the format would be ambiguous otherwise". GNOME
//! starts scopes with the process id as `<RANDOM>`
//! (`app-gnome-org.gnome.Nautilus-4321.scope`); COSMIC starts templated
//! services (`app-<id>@<random>.service`).
//!
//! The `-` between the parts is a separator only: the application id reaches
//! the name through systemd's unit-name escaping, `systemd.unit(5)`, "String
//! Escaping for Inclusion in Unit Names" (read 2026-11-21), which keeps ASCII
//! alphanumerics, `:`, `_` and `.`, writes every other byte as `\xXX`, and
//! escapes a leading `.`. So `org.gnome.Text-Editor` is `org.gnome.Text\x2dEditor`
//! in the name, and the name is split on its raw dashes *before* it is
//! unescaped. A name whose raw dashes do not fit either form is not guessed at:
//! it yields nothing.
//!
//! Who escapes (read 2026-11-21): GNOME's `gnome-desktop`
//! (`libgnome-desktop/gnome-systemd.c`, `systemd_unit_name_escape`, since
//! `12367c1`, 2020-07-29, "Change naming scheme to conform to systemd
//! convention") writes `app-gnome-<id>-<pid>.scope` escaped, and Budgie
//! launches through the same library (`budgie-session`,
//! `gnome-session/gsm-autostart-app.c`, `gnome_start_systemd_scope`);
//! Flatpak escapes (`common/flatpak-run.c`, `app-flatpak-%s-%s.scope` of
//! `systemd_unit_name_escape`). Cinnamon does not: its copy of the function
//! (`linuxmint/cinnamon-desktop`, `libcinnamon-desktop/gnome-systemd.c`)
//! keeps `-` among the valid characters of `g_strcanon` and writes
//! `app-cinnamon-%s-%d.scope`, so a Cinnamon id with a dash reaches the name
//! with raw dashes. For a scope whose launcher is `cinnamon`, everything
//! between the launcher and the last part is the id. That cannot misread an
//! escaped name: an escaping launcher's id has no raw dash, and a three-part
//! Cinnamon name reads the same either way. Any other name with too many raw
//! dashes still yields nothing, because nothing says where its id ends.
//!
//! This is the one reading of the scheme; Better Monitor's grouping and Better
//! Awake's application trigger both call it.

/// The one launcher known to write an id's dashes unescaped.
const CINNAMON: &str = "cinnamon";

/// What an application unit's name says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationUnit {
    /// The launcher named between `app-` and the id (`gnome`, `flatpak`,
    /// `KDE`), unescaped. `None` when the name carries none.
    pub launcher: Option<String>,
    /// The application id, unescaped.
    pub application_id: String,
}

/// Parse one unit name (the last segment of a cgroup path, still escaped as
/// systemd wrote it).
///
/// Returns `None` for a unit that is not an application unit by the scheme
/// above: a slice, a unit not starting with `app-`, a scope without
/// `<RANDOM>`, a template with an empty instance, or a name whose dashes are
/// ambiguous.
pub fn parse_application_unit(unit: &str) -> Option<ApplicationUnit> {
    let (launcher, id) = if let Some(stem) = unit.strip_suffix(".service") {
        // `@<RANDOM>` is the instance; a template unit (`app-x@.service`) is
        // not something a process runs in.
        let body = match stem.split_once('@') {
            Some((body, instance)) if !instance.is_empty() && !instance.contains('@') => body,
            Some(_) => return None,
            None => stem,
        };
        let parts: Vec<&str> = body.strip_prefix("app-")?.split('-').collect();
        match parts.as_slice() {
            [id] => (None, *id),
            [launcher, id] => (Some(*launcher), *id),
            _ => return None,
        }
    } else {
        let stem = unit.strip_suffix(".scope")?;
        let body = stem.strip_prefix("app-")?;
        let parts: Vec<&str> = body.split('-').collect();
        match parts.as_slice() {
            [id, random] if !random.is_empty() => (None, *id),
            [launcher, id, random] if !random.is_empty() => (Some(*launcher), *id),
            // Cinnamon writes its ids' dashes raw; see the module notes.
            [CINNAMON, .., random] if parts.len() > 3 && !random.is_empty() => {
                let id = &body[CINNAMON.len() + 1..body.len() - random.len() - 1];
                (Some(CINNAMON), id)
            }
            _ => return None,
        }
    };
    if id.is_empty() || launcher.is_some_and(str::is_empty) {
        return None;
    }
    Some(ApplicationUnit {
        launcher: launcher.map(unescape_unit_name),
        application_id: unescape_unit_name(id),
    })
}

/// Undo systemd's `\xXX` unit-name escaping.
///
/// The escapes are bytes, so a multi-byte character arrives as several of
/// them and is decoded as UTF-8. Text that does not decode to UTF-8 is
/// returned exactly as it was written rather than with replacement
/// characters, so what is shown is what systemd recorded.
pub fn unescape_unit_name(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1] == b'x'
            && let Some(code) = raw
                .get(index + 2..index + 4)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(code);
            index += 4;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ApplicationUnit, parse_application_unit, unescape_unit_name};

    fn unit(launcher: Option<&str>, id: &str) -> Option<ApplicationUnit> {
        Some(ApplicationUnit {
            launcher: launcher.map(str::to_string),
            application_id: id.to_string(),
        })
    }

    #[test]
    fn a_scope_with_a_launcher_and_a_process_id() {
        // The shape GNOME 46 produces on this machine.
        assert_eq!(
            parse_application_unit("app-gnome-org.example.Viewer-6030.scope"),
            unit(Some("gnome"), "org.example.Viewer")
        );
    }

    #[test]
    fn a_scope_without_a_launcher() {
        // Also seen on this machine, beside the launcher form.
        assert_eq!(
            parse_application_unit("app-com.example.Browser-117362.scope"),
            unit(None, "com.example.Browser")
        );
    }

    #[test]
    fn a_scope_whose_random_part_is_not_digits() {
        assert_eq!(
            parse_application_unit("app-KDE-org.example.reader-a1b2c3.scope"),
            unit(Some("KDE"), "org.example.reader")
        );
    }

    #[test]
    fn an_escaped_dash_in_a_scope_id_is_part_of_the_id() {
        // `app-gnome-<word>\x2d<word>-<pid>.scope` and three escapes in one id,
        // both live on this machine.
        assert_eq!(
            parse_application_unit("app-gnome-example\\x2dnotifier-6846.scope"),
            unit(Some("gnome"), "example-notifier")
        );
        assert_eq!(
            parse_application_unit("app-gnome-ex\\x2dam\\x2dple\\x2dbus-2547.scope"),
            unit(Some("gnome"), "ex-am-ple-bus")
        );
        assert_eq!(
            parse_application_unit("app-gnome-org.example.Mail\\x2dalarm\\x2dnotify-2901.scope"),
            unit(Some("gnome"), "org.example.Mail-alarm-notify")
        );
    }

    #[test]
    fn an_escaped_dash_followed_by_digits_is_not_taken_for_the_random_part() {
        assert_eq!(
            parse_application_unit("app-gnome-org.example.Tool\\x2d2-77.scope"),
            unit(Some("gnome"), "org.example.Tool-2")
        );
    }

    #[test]
    fn a_templated_service_drops_its_instance() {
        assert_eq!(
            parse_application_unit("app-org.example.Editor@9f8e7d6c.service"),
            unit(None, "org.example.Editor")
        );
        assert_eq!(
            parse_application_unit("app-gnome-org.example.Viewer@12345.service"),
            unit(Some("gnome"), "org.example.Viewer")
        );
        assert_eq!(
            parse_application_unit("app-flatpak-org.example.Chat@12345.service"),
            unit(Some("flatpak"), "org.example.Chat")
        );
    }

    #[test]
    fn a_templated_service_with_an_escaped_id() {
        assert_eq!(
            parse_application_unit("app-cosmic-com.example.my\\x2dterm@a1b2.service"),
            unit(Some("cosmic"), "com.example.my-term")
        );
    }

    #[test]
    fn a_service_without_an_instance() {
        assert_eq!(
            parse_application_unit("app-org.example.player.service"),
            unit(None, "org.example.player")
        );
        assert_eq!(
            parse_application_unit("app-gnome-org.example.player.service"),
            unit(Some("gnome"), "org.example.player")
        );
    }

    #[test]
    fn an_escaped_dash_followed_by_digits_in_a_service_is_kept() {
        assert_eq!(
            parse_application_unit("app-org.example.tool\\x2d123.service"),
            unit(None, "org.example.tool-123")
        );
    }

    #[test]
    fn multi_byte_escapes_decode_as_utf8() {
        assert_eq!(
            parse_application_unit("app-gnome-org.example.\\xe4\\xb8\\xad-5.scope"),
            unit(Some("gnome"), "org.example.中")
        );
        // Bytes that are not UTF-8 are shown as systemd wrote them.
        assert_eq!(unescape_unit_name("a\\xffb"), "a\\xffb");
    }

    // Review finding 11: Cinnamon keeps an id's dashes raw.
    #[test]
    fn a_cinnamon_scope_keeps_the_raw_dashes_of_its_id() {
        assert_eq!(
            parse_application_unit("app-cinnamon-my-code-1234.scope"),
            unit(Some("cinnamon"), "my-code")
        );
        assert_eq!(
            parse_application_unit("app-cinnamon-org.example.Mail-alarm-notify-77.scope"),
            unit(Some("cinnamon"), "org.example.Mail-alarm-notify")
        );
        assert_eq!(
            parse_application_unit("app-cinnamon-org.example.Viewer-5.scope"),
            unit(Some("cinnamon"), "org.example.Viewer")
        );
        // An escaping launcher's names read as before.
        assert_eq!(
            parse_application_unit("app-gnome-my\\x2dcode-1234.scope"),
            unit(Some("gnome"), "my-code")
        );
        // A launcher nobody showed writes raw dashes is not guessed at.
        assert_eq!(parse_application_unit("app-xfce-my-code-1234.scope"), None);
    }

    #[test]
    fn names_outside_the_scheme_yield_nothing() {
        for name in [
            // A scope has a mandatory random part.
            "app-org.example.Viewer.scope",
            // Raw dashes that fit neither form.
            "app-gnome-org-example-Viewer-12.scope",
            "app-gnome-org.example.Viewer-1-2.service",
            // A template, not a running instance.
            "app-org.example.Editor@.service",
            // Slices are not defined by the scheme.
            "app-gnome\\x2dsession\\x2dmanager.slice",
            "app-org.example.Terminal.slice",
            "app.slice",
            "session-2.scope",
            "NetworkManager.service",
            "app-.service",
            "app--x-1.scope",
            "",
        ] {
            assert_eq!(parse_application_unit(name), None, "{name}");
        }
    }
}
