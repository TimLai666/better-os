//! Better Touchpad in a session whose dconf profile puts its own database in
//! front of GNOME's (ticket 207, review finding 3), against a real
//! `dconf-service` on a private bus (`defaults_platform::test_support`):
//! never the person's own settings.

use defaults_platform::dconf_profile::session_database_in;
use defaults_platform::gvariant::ChangeValue;
use defaults_platform::test_support::{PrivateDconf, read_database};
use touchpad_core::{Backup, Reading, RestorePlan, RestoreScope, SettingId, SettingValue};
use touchpad_platform::keybindings::{self, KeybindingReading};
use touchpad_platform::{GnomeBackend, TouchpadBackend};

const TAP: &str = "/org/gnome/desktop/peripherals/touchpad/tap-to-click";
const CLOSE: &str = "/org/gnome/desktop/wm/keybindings/close";

fn tap(backend: &GnomeBackend) -> Reading {
    backend.read(&[SettingId::TapToClick]).remove(0).1
}

#[test]
fn in_the_session_gnome_s_value_is_shown_written_over_and_put_back_by_reset() {
    let Some(dconf) = PrivateDconf::start_or_skip() else {
        return;
    };
    dconf.seed_gnome(TAP, ChangeValue::Boolean(false));
    let gnome_before = std::fs::read(&dconf.database).unwrap();
    dconf.install_session_profile();
    let session =
        session_database_in(&dconf.environment(Some("betteros"), "BetterOS:COSMIC")).unwrap();
    assert_eq!(session.writer_path, "/ca/desrt/dconf/Writer/betteros");
    let mut backend =
        GnomeBackend::with_session(&session, dconf.writer_at(&session.writer_path), None);

    // What the session shows: GNOME's value, from beneath.
    assert_eq!(
        tap(&backend),
        Reading::Inherited {
            value: SettingValue::toggle(false),
            from: "user".to_string()
        }
    );
    // A capture keeps it as "the session's own database holds nothing".
    let backup = Backup::capture(
        backend.name(),
        None,
        backend.read(&[SettingId::TapToClick]),
        1,
    );
    assert!(matches!(
        backup.reading(SettingId::TapToClick),
        Some(Reading::SessionDefault { .. })
    ));

    let written = backend.write_all(&[(SettingId::TapToClick, Some(SettingValue::toggle(true)))]);
    assert!(
        matches!(written[0].1, touchpad_platform::WriteOutcome::Written),
        "{written:?}"
    );
    assert_eq!(tap(&backend), Reading::value(SettingValue::toggle(true)));
    assert!(read_database(&session.path).get(TAP).is_some());
    assert_eq!(std::fs::read(&dconf.database).unwrap(), gnome_before);

    let capabilities = backend.capabilities().clone();
    let plan = RestorePlan::from_backup(&backup, RestoreScope::All, |setting| {
        capabilities.support(setting)
    });
    let report = backend.restore(&plan);
    assert_eq!(
        report.state(),
        touchpad_core::RunState::Applied,
        "{report:?}"
    );
    assert!(
        read_database(&session.path).get(TAP).is_none(),
        "the restore copied a value into the session's database"
    );
    assert!(matches!(tap(&backend), Reading::Inherited { .. }));
    assert_eq!(std::fs::read(&dconf.database).unwrap(), gnome_before);
}

#[test]
fn the_shortcut_check_sees_bindings_held_beneath_the_session_s_database() {
    let Some(dconf) = PrivateDconf::start_or_skip() else {
        return;
    };
    dconf.seed_gnome(CLOSE, ChangeValue::TextList(vec!["<Super>q".to_string()]));
    dconf.install_session_profile();
    let session =
        session_database_in(&dconf.environment(Some("betteros"), "BetterOS:COSMIC")).unwrap();
    let stacked: Vec<_> = std::iter::once(session.path.clone())
        .chain(session.beneath.clone())
        .collect();
    let KeybindingReading::Recorded(bindings) = keybindings::read_stacked(&stacked) else {
        panic!("both databases are readable");
    };
    assert!(
        bindings
            .iter()
            .any(|binding| binding.key == CLOSE && binding.binding == "<Super>q"),
        "{bindings:?}"
    );
    // The session's own value hides GNOME's for the same key.
    let mut change = defaults_platform::gvariant::Changeset::new("/").unwrap();
    change
        .set_path(CLOSE, ChangeValue::TextList(vec!["<Super>w".to_string()]))
        .unwrap();
    dconf
        .writer_at(&session.writer_path)
        .change(&change)
        .unwrap();
    let KeybindingReading::Recorded(bindings) = keybindings::read_stacked(&stacked) else {
        panic!("both databases are readable");
    };
    let close: Vec<_> = bindings
        .iter()
        .filter(|binding| binding.key == CLOSE)
        .map(|binding| binding.binding.as_str())
        .collect();
    assert_eq!(close, vec!["<Super>w"]);
}
