//! Each entry against a fixture machine in a scratch directory: detection
//! present and absent, the fix, its check, and the undo, through the same
//! file reading and writing the person's machine gets.
//!
//! Nothing here reads the environment or starts a process: the host's paths
//! are all under a fresh temporary directory, and its commands come from the
//! fixture's files, so no test can reach the person's font cache or Flatpak
//! overrides.

use std::path::Path;

use patch_core::engine;
use patch_core::entries::{electron_cursor::ElectronCursor, font_cache::FontCache};
use patch_core::flatpak::AppId;
use patch_core::{CheckOutcome, EntryState, Records, Subject};
use patch_platform::fixture::{self, SEEDED_APP, Seed};
use patch_platform::{LinuxHost, Paths};

fn scratch() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    // Every path the host will touch is under the scratch root.
    let paths = Paths::under(root.path());
    for path in [&paths.user_flatpak, &paths.system_flatpak, &paths.records] {
        assert!(path.starts_with(root.path()));
    }
    root
}

fn machine(seed: Seed) -> (tempfile::TempDir, LinuxHost) {
    let root = scratch();
    fixture::seed(root.path(), seed).unwrap();
    let host = fixture::host(root.path());
    (root, host)
}

fn app() -> AppId {
    AppId::parse(SEEDED_APP).unwrap()
}

fn override_text(root: &Path) -> Option<String> {
    std::fs::read_to_string(Paths::under(root).user_override(&app())).ok()
}

#[test]
fn font_cache_detection_present_and_absent() {
    let (_root, mut host) = machine(Seed::BothFaults);
    let report = engine::check(&FontCache, &mut host, &Records::default(), &[]);
    assert_eq!(report.overall(), EntryState::Found);
    assert_eq!(report.needing_attention(), 1);

    let (_root, mut host) = machine(Seed::Healthy);
    let report = engine::check(&FontCache, &mut host, &Records::default(), &[]);
    assert_eq!(report.overall(), EntryState::NotPresent);
    assert!(report.instances.is_empty());
}

#[test]
fn font_cache_fix_and_check_and_no_undo() {
    let (root, mut host) = machine(Seed::BothFaults);
    let report = engine::check(&FontCache, &mut host, &Records::default(), &[]);
    let finding = report.instances[0].finding.clone().unwrap();
    let outcome = engine::fix(&FontCache, &finding, &mut host, 10).unwrap();
    assert_eq!(outcome.check, CheckOutcome::Passed);
    // The record is not kept: there is nothing it could put back.
    let mut records = Records::default();
    records.keep(&FontCache, outcome.record.clone());
    assert!(records.applied.is_empty());
    let report = engine::check(&FontCache, &mut host, &records, &[]);
    assert_eq!(report.overall(), EntryState::NotPresent);
    let undone = engine::undo(&FontCache, &outcome.record, &mut host, false).unwrap();
    assert_eq!(undone.remaining, None);
    // The fixture's cache stayed rebuilt.
    let fonts = std::fs::read_to_string(root.path().join("fonts.tsv")).unwrap();
    assert!(fonts.contains("NotoSansCJK-Bold.ttc"));
}

#[test]
fn font_cache_without_fontconfig_cannot_check() {
    let (root, mut host) = machine(Seed::BothFaults);
    std::fs::remove_file(root.path().join("fonts.tsv")).unwrap();
    let report = engine::check(&FontCache, &mut host, &Records::default(), &[]);
    assert!(matches!(report.overall(), EntryState::CannotCheck(_)));
}

#[test]
fn electron_cursor_detection_present_and_absent() {
    let (_root, mut host) = machine(Seed::BothFaults);
    let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
    assert_eq!(report.overall(), EntryState::Found);
    assert_eq!(report.instances[0].subject, Subject::FlatpakApp(app()));

    let (_root, mut host) = machine(Seed::Healthy);
    let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
    assert_eq!(report.overall(), EntryState::NotPresent);

    // At scale 1 the same app is fine.
    let (root, mut host) = machine(Seed::BothFaults);
    std::fs::write(root.path().join("scale"), "1\n").unwrap();
    let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
    assert_eq!(report.overall(), EntryState::NotPresent);

    // On X11 too.
    std::fs::write(root.path().join("scale"), "2\n").unwrap();
    std::fs::write(root.path().join("session"), "x11\n").unwrap();
    let mut host = fixture::host(root.path());
    let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
    assert_eq!(report.overall(), EntryState::NotPresent);
}

const HAND_MADE: &str = "\
# My own settings, kept by hand.
[Context]
filesystems=~/Projects;

[Environment]
GTK_THEME=Adwaita:dark
";

#[test]
fn electron_cursor_fix_check_and_undo_leave_the_person_s_own_lines() {
    let (root, mut host) = machine(Seed::BothFaults);
    let path = Paths::under(root.path()).user_override(&app());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, HAND_MADE).unwrap();

    let store = fixture::store(root.path());
    let report = engine::check(&ElectronCursor, &mut host, &store.load().unwrap(), &[]);
    let finding = report.instances[0].finding.clone().unwrap();
    let outcome = engine::fix(&ElectronCursor, &finding, &mut host, 20).unwrap();
    assert_eq!(outcome.check, CheckOutcome::Passed);
    assert_eq!(
        override_text(root.path()).as_deref(),
        Some(
            "\
# My own settings, kept by hand.
[Context]
filesystems=~/Projects;
sockets=x11;!fallback-x11;!wayland;

[Environment]
GTK_THEME=Adwaita:dark
ELECTRON_OZONE_PLATFORM_HINT=x11
XDG_SESSION_TYPE=x11
"
        )
    );
    let mut records = store.load().unwrap();
    records.keep(&ElectronCursor, outcome.record.clone());
    store.save(&records).unwrap();

    // A later check, from the kept record: fixed and checked.
    let records = store.load().unwrap();
    let report = engine::check(&ElectronCursor, &mut host, &records, &[]);
    assert_eq!(report.overall(), EntryState::FixedAndChecked);

    // Undo, from the kept record, takes out exactly the fix's keys.
    let record = records
        .find(
            patch_core::EntryId::ElectronCursor,
            &Subject::FlatpakApp(app()),
        )
        .unwrap()
        .clone();
    let undone = engine::undo(&ElectronCursor, &record, &mut host, false).unwrap();
    assert_eq!(undone.remaining, None);
    assert_eq!(override_text(root.path()).as_deref(), Some(HAND_MADE));
    let mut records = records;
    records.after_undo(record.entry, &record.subject, undone.remaining);
    store.save(&records).unwrap();
    let report = engine::check(
        &ElectronCursor,
        &mut host,
        &store.load().unwrap(),
        &[Subject::FlatpakApp(app())],
    );
    assert_eq!(report.instances[0].state, EntryState::Undone);
    // And the fault is back for the next check.
    let report = engine::check(&ElectronCursor, &mut host, &store.load().unwrap(), &[]);
    assert_eq!(report.overall(), EntryState::Found);
}

#[test]
fn electron_cursor_undo_of_a_file_it_made_removes_it() {
    let (root, mut host) = machine(Seed::BothFaults);
    let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
    let finding = report.instances[0].finding.clone().unwrap();
    let outcome = engine::fix(&ElectronCursor, &finding, &mut host, 1).unwrap();
    assert!(override_text(root.path()).is_some());
    engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
    assert_eq!(override_text(root.path()), None);
}

#[test]
fn electron_cursor_a_key_changed_after_the_fix_waits_for_confirmation() {
    let (root, mut host) = machine(Seed::BothFaults);
    let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
    let finding = report.instances[0].finding.clone().unwrap();
    let outcome = engine::fix(&ElectronCursor, &finding, &mut host, 1).unwrap();
    let path = Paths::under(root.path()).user_override(&app());
    // The person put the app back on Wayland by hand: the socket, the
    // session type, and (for this Electron 32) the hint.
    let changed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("!wayland;", "wayland;")
        .replace("XDG_SESSION_TYPE=x11", "XDG_SESSION_TYPE=wayland")
        .replace("HINT=x11", "HINT=auto");
    std::fs::write(&path, &changed).unwrap();

    let mut records = Records::default();
    records.keep(&ElectronCursor, outcome.record.clone());
    let report = engine::check(&ElectronCursor, &mut host, &records, &[]);
    // The changed hint brings the fault back, and the page says why.
    assert_eq!(report.overall(), EntryState::AppliedButStillPresent);
    assert_eq!(report.instances[0].changed_elsewhere.len(), 3);

    let undone = engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
    assert_eq!(
        override_text(root.path()).as_deref(),
        Some(
            "[Context]\nsockets=wayland;\n\n[Environment]\nELECTRON_OZONE_PLATFORM_HINT=auto\nXDG_SESSION_TYPE=wayland\n"
        )
    );
    let mut left = outcome.record.clone();
    left.change = undone.remaining.expect("the changed key stays recorded");
    engine::undo(&ElectronCursor, &left, &mut host, true).unwrap();
    assert_eq!(override_text(root.path()), None);
}

#[test]
fn electron_cursor_refuses_to_write_when_the_file_moved_under_it() {
    let (root, mut host) = machine(Seed::BothFaults);
    let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
    let finding = report.instances[0].finding.clone().unwrap();
    let outcome = engine::fix(&ElectronCursor, &finding, &mut host, 1).unwrap();
    // An undo planned from one reading cannot overwrite a newer one.
    let path = Paths::under(root.path()).user_override(&app());
    let plan_text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{plan_text}# a later line\n")).unwrap();
    let result = patch_core::Host::change(
        &mut host,
        &patch_core::Change::WriteUserOverride {
            app: app(),
            expected: Some(plan_text),
            contents: None,
        },
    );
    assert_eq!(result, Err(patch_core::ChangeError::ChangedSinceRead));
    assert!(
        override_text(root.path())
            .unwrap()
            .ends_with("# a later line\n")
    );
    drop(outcome);
}

// --- Entries 3 and 4: the USB-C faults (ticket 101) ------------------------

mod usb {
    use super::*;
    use patch_core::entries::usb_c::{UsbCInterface, UsbController};
    use patch_core::entry::FindingDetail;
    use patch_core::{Answer, Change, Entry, Host, Privileged, Read};
    use patch_platform::fixture::UsbSeed;

    fn usb_machine(seed: UsbSeed) -> (tempfile::TempDir, LinuxHost) {
        let root = scratch();
        fixture::seed(root.path(), Seed::Healthy).unwrap();
        fixture::seed_usb(root.path(), seed).unwrap();
        let host = fixture::host(root.path());
        (root, host)
    }

    fn settings(root: &Path) -> Option<String> {
        std::fs::read_to_string(
            root.join(fixture::MACHINE)
                .join(patch_platform::usb::SETTINGS),
        )
        .ok()
    }

    #[test]
    fn on_another_laptop_neither_entry_applies() {
        let (_root, mut host) = usb_machine(UsbSeed::NotAffected);
        for entry in [&UsbCInterface as &dyn Entry, &UsbController] {
            let report = engine::check(entry, &mut host, &Records::default(), &[]);
            assert_eq!(report.overall(), EntryState::DoesNotApply);
            assert!(report.instances.is_empty());
        }
    }

    #[test]
    fn a_healthy_affected_machine_shows_neither_fault() {
        let (_root, mut host) = usb_machine(UsbSeed::Healthy);
        for entry in [&UsbCInterface as &dyn Entry, &UsbController] {
            let report = engine::check(entry, &mut host, &Records::default(), &[]);
            assert_eq!(report.overall(), EntryState::NotPresent);
        }
    }

    #[test]
    fn the_interface_is_found_fixed_checked_and_undone() {
        let (root, mut host) = usb_machine(UsbSeed::Failing);
        let report = engine::check(&UsbCInterface, &mut host, &Records::default(), &[]);
        assert_eq!(report.overall(), EntryState::Found);
        let finding = report.instances[0].finding.clone().unwrap();
        assert_eq!(
            finding.detail,
            FindingDetail::UsbCInterface {
                timeouts: 1,
                after_sleep: 1
            }
        );
        let outcome = engine::fix(&UsbCInterface, &finding, &mut host, 5).unwrap();
        assert_eq!(outcome.check, CheckOutcome::Passed);
        assert!(
            settings(root.path())
                .unwrap()
                .contains("\"ucsi_automatic\": true")
        );

        // Kept by the machine: a fresh check with an empty personal record
        // sees it fixed.
        let report = engine::check(&UsbCInterface, &mut host, &Records::default(), &[]);
        assert_eq!(report.overall(), EntryState::FixedAndChecked);
        let record = report.instances[0].record.clone().unwrap();
        engine::undo(&UsbCInterface, &record, &mut host, false).unwrap();
        assert!(
            !settings(root.path())
                .unwrap()
                .contains("\"ucsi_automatic\": true")
        );
        let report = engine::check(&UsbCInterface, &mut host, &Records::default(), &[]);
        assert_eq!(report.overall(), EntryState::NotPresent);
    }

    #[test]
    fn a_stuck_hub_is_a_restart_and_the_guard_is_turned_on() {
        let (root, mut host) = usb_machine(UsbSeed::Failing);
        let report = engine::check(&UsbController, &mut host, &Records::default(), &[]);
        let finding = report.instances[0].finding.clone().unwrap();
        assert!(matches!(
            finding.detail,
            FindingDetail::UsbController {
                restart_needed: true,
                sleep_failures: 2,
                streak: 2,
                ..
            }
        ));
        let outcome = engine::fix(&UsbController, &finding, &mut host, 5).unwrap();
        assert_eq!(outcome.check, CheckOutcome::StillPresent);
        assert!(
            settings(root.path())
                .unwrap()
                .contains("\"controller_automatic\": true")
        );
        // Reset USB-C on that machine is refused as a restart.
        assert_eq!(
            host.usb_act(Privileged::RebindController),
            Err(patch_core::ChangeError::Refused(
                "daemon.error.usb_restart_needed".into()
            ))
        );
    }

    #[test]
    fn a_dead_controller_with_free_hub_work_is_rebound_and_checked() {
        let (_root, mut host) = usb_machine(UsbSeed::ControllerDeadOnly);
        let report = engine::check(&UsbController, &mut host, &Records::default(), &[]);
        let finding = report.instances[0].finding.clone().unwrap();
        let outcome = engine::fix(&UsbController, &finding, &mut host, 5).unwrap();
        assert_eq!(outcome.check, CheckOutcome::Passed);
        let report = engine::check(&UsbController, &mut host, &Records::default(), &[]);
        assert_eq!(report.overall(), EntryState::FixedAndChecked);
    }

    #[test]
    fn reset_usb_c_reports_what_came_back() {
        let (_root, mut host) = usb_machine(UsbSeed::Failing);
        let reply = host.usb_act(Privileged::ReloadUcsi).unwrap().unwrap();
        assert!(reply.restored.complete(), "{reply:?}");
        // Nothing failing any more: a second reset is refused, not run.
        assert_eq!(
            host.usb_act(Privileged::ReloadUcsi),
            Err(patch_core::ChangeError::Refused(
                "daemon.error.usb_nothing_to_reset".into()
            ))
        );
    }

    /// The person who can read the system journal reads it themselves;
    /// nobody else's read reaches a log line, only the daemon's findings.
    #[test]
    fn the_kernel_log_is_read_as_the_person_when_they_may() {
        let (root, mut host) = usb_machine(UsbSeed::Failing);
        let machine = root.path().join(fixture::MACHINE);
        // The daemon's answer (from the fixture daemon) when they may not.
        let from_daemon = host.read(&Read::UsbFindings);
        // Readable: the same classification through journalctl's output.
        std::fs::create_dir_all(machine.join("etc")).unwrap();
        std::fs::write(machine.join("etc/machine-id"), "0123abcd\n").unwrap();
        let journal = machine.join("var/log/journal/0123abcd");
        std::fs::create_dir_all(&journal).unwrap();
        std::fs::write(journal.join("system.journal"), b"").unwrap();
        let as_person = host.read(&Read::UsbFindings);
        assert_eq!(from_daemon, as_person);
        assert!(matches!(as_person, Answer::UsbFindings(Ok(ref f)) if f.ucsi.active));
    }

    #[test]
    fn a_host_without_usb_set_up_reaches_nothing() {
        let root = scratch();
        let mut host = LinuxHost::new(
            Paths::under(root.path()),
            Box::new(patch_platform::fixture::FixtureCommands::new(root.path())),
            Box::new(NoScales),
            None,
        );
        assert!(matches!(
            host.read(&Read::UsbMachine),
            Answer::UsbMachine(Err(_))
        ));
        assert!(
            host.change(&Change::Privileged(Privileged::ReloadUcsi))
                .is_err()
        );
    }

    struct NoScales;
    impl patch_platform::display::DisplayScales for NoScales {
        fn scales(&self) -> Result<Vec<f64>, patch_core::ReadError> {
            Err(patch_core::ReadError::NotOnThisDesktop)
        }
    }
}
