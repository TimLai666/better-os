//! What removing Better Manager takes back (ticket 90).
//!
//! A fix Better Manager applied is something Better OS added to the
//! machine, so it does not outlive the package (AGENTS.md). The package's
//! before-removal program (`/usr/lib/better-os/before-removal/better-manager`)
//! runs this as each person, from the package's prerm, before dpkg removes
//! the files:
//!
//! - every recorded fix with an undo is undone through the same path the
//!   page's Undo takes, and the same rule: a key the person changed after the
//!   fix is left as they made it, and named;
//! - a fix whose undo failed (its override file became a link, changed
//!   while it was being undone, cannot be written) stays recorded, and its
//!   file is named, so nothing is left changed with no record of it;
//! - the rest of the record is deleted;
//! - a fix with no undo (the rebuilt font cache) keeps no record and is left,
//!   since it restored the machine rather than adding to it.
//!
//! Before anything is changed, the record and each override file it names go
//! aside in [`SET_ASIDE_FILE`] at the default state location, stamped with
//! the dpkg run the prerm made up. `--undo` (postinst abort-remove) acts
//! only on the set-aside file of that same run and deletes any other. The
//! package's postrm deletes the file once the removal has happened. A
//! set-aside file Better Manager finds at its next start belongs to a removal
//! that did not finish: [`interrupted_removal`] reads it so the Fixes page
//! can say what was undone, and deletes it.

use std::path::{Path, PathBuf};

use patch_core::engine;
use patch_core::entry::{self, Answer, Change, Host, Read};
use patch_core::flatpak::AppId;
use patch_core::record::{AppliedRecord, KeyValue, OverrideKey, Recorded};
use patch_core::{Records, UndoKind};
use serde::{Deserialize, Serialize};

use crate::paths::{Paths, default_state};
use crate::store::RecordStore;

/// What a removal set aside, at the default state location.
pub const SET_ASIDE_FILE: &str = "fixes-removal-undo.json";
/// What an interrupted write of it can leave.
pub const SET_ASIDE_TEMPORARY: &str = "fixes-removal-undo.json.tmp";
/// The variable the maintainer scripts pass the dpkg run's stamp in.
pub const RUN_VARIABLE: &str = "BETTER_OS_REMOVAL_RUN";

/// Where [`SET_ASIDE_FILE`] is, for one home.
pub fn set_aside_path(home: &Path) -> PathBuf {
    default_state(home).join(SET_ASIDE_FILE)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SetAside {
    /// The dpkg run that wrote it.
    #[serde(default)]
    run: Option<String>,
    /// Where the record was, and its text (`None`: there was none).
    records_path: PathBuf,
    records: Option<String>,
    /// Each override file a fix wrote, as it was before this removal.
    overrides: Vec<(PathBuf, AppId, Option<String>)>,
}

/// What one run did; the program turns it into its exit status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Removal {
    /// No fix with an undo was recorded.
    NothingApplied,
    /// Every recorded fix was undone and the record deleted.
    Undone,
    /// Not everything: each line says what was left and why. Keys the person
    /// changed after a fix stay as they made them (their record is deleted);
    /// a fix whose undo failed stays, recorded, naming its file.
    Partly(Vec<String>),
    /// Nothing was changed, for this reason.
    Failed(String),
}

fn key_name(key: &OverrideKey, now: &KeyValue) -> String {
    let name = match key {
        OverrideKey::Socket(name) => format!("socket {name}"),
        OverrideKey::Environment(name) => name.clone(),
    };
    let now = match now {
        KeyValue::Absent => "removed".to_string(),
        KeyValue::Granted => "granted".to_string(),
        KeyValue::Denied => "denied".to_string(),
        KeyValue::Value(value) => format!("set to {value}"),
    };
    format!("{name} ({now})")
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Writes beside `path` under `temporary` and renames it over, removing the
/// temporary file when anything fails.
fn write_text(path: &Path, temporary: &Path, text: &str) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let written = std::fs::write(temporary, text).and_then(|()| std::fs::rename(temporary, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(temporary);
        return Err(failed(error));
    }
    Ok(())
}

fn override_now(host: &mut dyn Host, app: &AppId) -> Result<Option<String>, String> {
    match host.read(&Read::UserOverride(app.clone())) {
        Answer::File(Ok(text)) => Ok(text),
        Answer::File(Err(error)) => Err(error.to_string()),
        _ => Err("the override file was not read".to_string()),
    }
}

fn app_of(record: &AppliedRecord) -> Option<&AppId> {
    match &record.change {
        Recorded::OverrideKeys { app, .. } => Some(app),
        Recorded::RebuiltFontCache | Recorded::UsbRecovery { .. } => None,
    }
}

/// The run before removal, for the person whose home is `home`, with their
/// files where `paths` says. `run` is the dpkg run's stamp.
pub fn before_removal(
    host: &mut dyn Host,
    paths: &Paths,
    home: &Path,
    run: Option<&str>,
) -> Removal {
    let aside_path = set_aside_path(home);
    let aside_temporary = default_state(home).join(SET_ASIDE_TEMPORARY);
    // A set-aside file is only ever about this run: one an earlier removal
    // left would make a refused removal's `--undo` write that older state.
    if let Err(error) = remove(&aside_path).and_then(|()| remove(&aside_temporary)) {
        return Removal::Failed(format!("an old undo file could not be removed: {error}"));
    }
    let store = RecordStore::at(&paths.records);
    let text = match read_text(&paths.records) {
        Ok(Some(text)) => text,
        Ok(None) => return Removal::NothingApplied,
        Err(error) => return Removal::Failed(error),
    };
    // A record this build cannot read is kept rather than deleted: it is the
    // only account of what was changed.
    let records = match store.load() {
        Ok(records) => records,
        Err(error) => return Removal::Failed(error.to_string()),
    };
    let reversible: Vec<AppliedRecord> = records
        .applied
        .iter()
        .filter(|record| entry::by_id(record.entry).undo_kind() == UndoKind::Reversible)
        .cloned()
        .collect();

    let mut aside = SetAside {
        run: run.map(str::to_string),
        records_path: paths.records.clone(),
        records: Some(text),
        overrides: Vec::new(),
    };
    for app in reversible.iter().filter_map(app_of) {
        match override_now(host, app) {
            Ok(now) => aside
                .overrides
                .push((paths.user_override(app), app.clone(), now)),
            Err(error) => return Removal::Failed(error),
        }
    }
    let aside_text = serde_json::to_string_pretty(&aside).expect("the set-aside state serializes");
    if let Err(error) = write_text(&aside_path, &aside_temporary, &aside_text) {
        return Removal::Failed(format!("what is there now could not be kept: {error}"));
    }

    let mut left = Vec::new();
    let mut still_applied = Records::default();
    for record in &reversible {
        let entry = entry::by_id(record.entry);
        let file = app_of(record).map(|app| paths.user_override(app));
        match engine::undo(entry, record, host, false) {
            Ok(outcome) => {
                let app = app_of(record).map(AppId::to_string).unwrap_or_default();
                left.extend(outcome.skipped.iter().map(|changed| {
                    format!(
                        "{app}: {} was changed after the fix and is left as it is",
                        key_name(&changed.key, &changed.now)
                    )
                }));
            }
            Err(error) => {
                left.push(format!(
                    "{}: not undone, still recorded ({})",
                    file.map_or_else(String::new, |file| file.display().to_string()),
                    error.details()
                ));
                still_applied.applied.push(record.clone());
            }
        }
    }
    let kept = if still_applied.applied.is_empty() {
        remove(&paths.records)
    } else {
        store
            .save(&still_applied)
            .map_err(|error| error.to_string())
    };
    if let Err(error) = kept {
        left.push(format!("the record could not be updated: {error}"));
    }
    if !left.is_empty() {
        Removal::Partly(left)
    } else if reversible.is_empty() {
        Removal::NothingApplied
    } else {
        Removal::Undone
    }
}

/// `--undo`: the removal did not happen. Only the set-aside file this same
/// dpkg run wrote is used: every override file and the record go back to
/// what they were before the run. A set-aside file from any other run (or
/// with no stamp) is stale and deleted unused. Returns why not, when
/// something could not be put back.
pub fn undo_before_removal(
    host: &mut dyn Host,
    home: &Path,
    run: Option<&str>,
) -> Result<(), String> {
    let path = set_aside_path(home);
    let Some(text) = read_text(&path)? else {
        return Ok(());
    };
    let aside: Option<SetAside> = serde_json::from_str(&text).ok();
    let Some(aside) = aside.filter(|aside| run.is_some() && aside.run.as_deref() == run) else {
        return remove(&path);
    };
    let mut failures = Vec::new();
    for (file, app, before) in &aside.overrides {
        let now = match override_now(host, app) {
            Ok(now) => now,
            Err(error) => {
                failures.push(error);
                continue;
            }
        };
        if &now == before {
            continue;
        }
        if let Err(error) = host.change(&Change::WriteUserOverride {
            app: app.clone(),
            expected: now,
            contents: before.clone(),
        }) {
            failures.push(format!("{}: {error}", file.display()));
        }
    }
    let temporary = aside
        .records_path
        .with_file_name(crate::store::RECORDS_TEMPORARY);
    let records = match &aside.records {
        Some(text) => write_text(&aside.records_path, &temporary, text),
        None => remove(&aside.records_path),
    };
    if let Err(error) = records {
        failures.push(error);
    }
    if !failures.is_empty() {
        return Err(failures.join("; "));
    }
    remove(&path)
}

/// A removal that did not finish, as Better Manager finds it at its next
/// start: a set-aside file is there while no removal is running. The caller
/// must have checked that first (no removal stamp in /run/better-os, and
/// dpkg reporting no removal stopped part way:
/// `better_core::self_update::removal` is `Removal::None`): while a removal
/// runs, the file is that removal's, and an abort-remove's `--undo` needs
/// it. Returns how many fixes that removal undid, and
/// deletes the file; the Fixes page says so, and the next check shows each
/// fault that came back as found again. Nothing is reapplied on the
/// strength of a file that might be older than this installation.
pub fn interrupted_removal(home: &Path) -> Option<usize> {
    let path = set_aside_path(home);
    let text = std::fs::read_to_string(&path).ok()?;
    let undone = serde_json::from_str::<SetAside>(&text)
        .ok()
        .and_then(|aside| aside.records)
        .and_then(|records| serde_json::from_str::<Records>(&records).ok())
        .map_or(0, |records| {
            records
                .applied
                .iter()
                .filter(|record| entry::by_id(record.entry).undo_kind() == UndoKind::Reversible)
                .count()
        });
    let _ = remove(&path);
    let _ = remove(&default_state(home).join(SET_ASIDE_TEMPORARY));
    Some(undone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{self, SEEDED_APP, Seed};
    use patch_core::entries::{electron_cursor::ElectronCursor, font_cache::FontCache};
    use patch_core::{Entry, Subject};

    const HAND_MADE: &str = "# mine\n[Context]\nfilesystems=~/Projects;\n";

    fn app() -> AppId {
        AppId::parse(SEEDED_APP).unwrap()
    }

    /// A fixture machine with both fixes applied and the Electron one
    /// recorded, the way the page leaves it. The fixture root is the home.
    fn fixed_machine(hand_made: bool) -> (tempfile::TempDir, Paths) {
        let root = tempfile::tempdir().unwrap();
        fixture::seed(root.path(), Seed::BothFaults).unwrap();
        let paths = Paths::under(root.path());
        if hand_made {
            let path = paths.user_override(&app());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, HAND_MADE).unwrap();
        }
        let mut host = fixture::host(root.path());
        let store = RecordStore::at(&paths.records);
        let mut records = Records::default();
        for entry in [&FontCache as &dyn Entry, &ElectronCursor] {
            let report = engine::check(entry, &mut host, &records, &[]);
            let finding = report.instances[0].finding.clone().unwrap();
            let outcome = engine::fix(entry, &finding, &mut host, 1).unwrap();
            records.keep(entry, outcome.record);
        }
        store.save(&records).unwrap();
        (root, paths)
    }

    fn override_text(paths: &Paths) -> Option<String> {
        std::fs::read_to_string(paths.user_override(&app())).ok()
    }

    #[test]
    fn removal_undoes_every_recorded_fix_and_deletes_the_record() {
        let (root, paths) = fixed_machine(true);
        let mut host = fixture::host(root.path());
        assert_eq!(
            before_removal(&mut host, &paths, root.path(), Some("run-1")),
            Removal::Undone
        );
        assert_eq!(override_text(&paths).as_deref(), Some(HAND_MADE));
        assert!(!paths.records.exists());
        // The font cache stays rebuilt: nothing of the person's to give back.
        let fonts = std::fs::read_to_string(root.path().join("fonts.tsv")).unwrap();
        assert!(fonts.contains("NotoSansCJK-Bold.ttc"));
        // The fault is back, as it was before Better Manager.
        let report = engine::check(&ElectronCursor, &mut host, &Records::default(), &[]);
        assert_eq!(report.instances[0].subject, Subject::FlatpakApp(app()));
    }

    #[test]
    fn a_file_the_fix_made_is_removed_with_it() {
        let (root, paths) = fixed_machine(false);
        let mut host = fixture::host(root.path());
        assert_eq!(
            before_removal(&mut host, &paths, root.path(), None),
            Removal::Undone
        );
        assert_eq!(override_text(&paths), None);
    }

    #[test]
    fn a_key_the_person_changed_is_left_and_named() {
        let (root, paths) = fixed_machine(false);
        let path = paths.user_override(&app());
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("HINT=x11", "HINT=wayland")).unwrap();
        let mut host = fixture::host(root.path());
        assert_eq!(
            before_removal(&mut host, &paths, root.path(), None),
            Removal::Partly(vec![format!(
                "{SEEDED_APP}: ELECTRON_OZONE_PLATFORM_HINT (set to wayland) was changed after \
                 the fix and is left as it is"
            )])
        );
        assert_eq!(
            override_text(&paths).as_deref(),
            Some("[Environment]\nELECTRON_OZONE_PLATFORM_HINT=wayland\n")
        );
        assert!(!paths.records.exists());
    }

    #[test]
    fn a_fix_whose_undo_fails_stays_recorded_and_named() {
        let (root, paths) = fixed_machine(false);
        // The override became a link: the write is refused.
        let path = paths.user_override(&app());
        let text = std::fs::read_to_string(&path).unwrap();
        let elsewhere = root.path().join("elsewhere");
        std::fs::write(&elsewhere, &text).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        let mut host = fixture::host(root.path());
        let Removal::Partly(left) = before_removal(&mut host, &paths, root.path(), None) else {
            panic!("a failed undo is not a clean removal");
        };
        assert_eq!(left.len(), 1);
        assert!(left[0].contains(&path.display().to_string()), "{left:?}");
        assert!(left[0].contains("still recorded"), "{left:?}");
        let records = RecordStore::at(&paths.records).load().unwrap();
        assert_eq!(records.applied.len(), 1);
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), text);
    }

    #[test]
    fn the_same_run_s_undo_puts_everything_back() {
        let (root, paths) = fixed_machine(true);
        let fixed = override_text(&paths);
        let records = std::fs::read_to_string(&paths.records).unwrap();
        let mut host = fixture::host(root.path());
        before_removal(&mut host, &paths, root.path(), Some("run-7"));
        assert!(set_aside_path(root.path()).exists());
        undo_before_removal(&mut host, root.path(), Some("run-7")).unwrap();
        assert_eq!(override_text(&paths), fixed);
        assert_eq!(std::fs::read_to_string(&paths.records).unwrap(), records);
        assert!(!set_aside_path(root.path()).exists());
        // And a second --undo has nothing to do.
        undo_before_removal(&mut host, root.path(), Some("run-7")).unwrap();
        assert_eq!(override_text(&paths), fixed);
    }

    #[test]
    fn another_run_s_set_aside_file_is_deleted_unused() {
        let (root, paths) = fixed_machine(true);
        let mut host = fixture::host(root.path());
        before_removal(&mut host, &paths, root.path(), Some("old-run"));
        let after_removal = override_text(&paths);
        for run in [Some("new-run"), None] {
            std::fs::copy(set_aside_path(root.path()), root.path().join("copy")).unwrap();
            undo_before_removal(&mut host, root.path(), run).unwrap();
            assert!(!set_aside_path(root.path()).exists());
            assert_eq!(override_text(&paths), after_removal);
            assert!(!paths.records.exists());
            std::fs::rename(root.path().join("copy"), set_aside_path(root.path())).unwrap();
        }
    }

    #[test]
    fn an_unfinished_removal_is_found_at_the_next_start_and_cleared() {
        let (root, paths) = fixed_machine(true);
        let mut host = fixture::host(root.path());
        before_removal(&mut host, &paths, root.path(), Some("run"));
        assert_eq!(interrupted_removal(root.path()), Some(1));
        assert!(!set_aside_path(root.path()).exists());
        assert_eq!(interrupted_removal(root.path()), None);
    }

    #[test]
    fn nothing_recorded_is_nothing_to_do() {
        let root = tempfile::tempdir().unwrap();
        fixture::seed(root.path(), Seed::BothFaults).unwrap();
        let paths = Paths::under(root.path());
        let mut host = fixture::host(root.path());
        assert_eq!(
            before_removal(&mut host, &paths, root.path(), None),
            Removal::NothingApplied
        );
        assert!(!set_aside_path(root.path()).exists());
        undo_before_removal(&mut host, root.path(), None).unwrap();
    }

    #[test]
    fn a_record_it_cannot_read_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        std::fs::create_dir_all(paths.records.parent().unwrap()).unwrap();
        std::fs::write(&paths.records, "{ not json").unwrap();
        let mut host = fixture::host(root.path());
        assert!(matches!(
            before_removal(&mut host, &paths, root.path(), None),
            Removal::Failed(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&paths.records).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn an_old_set_aside_file_is_never_used_and_its_temporary_goes() {
        let (root, paths) = fixed_machine(true);
        let state = default_state(root.path());
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(set_aside_path(root.path()), "{}").unwrap();
        std::fs::write(state.join(SET_ASIDE_TEMPORARY), "half").unwrap();
        let mut host = fixture::host(root.path());
        assert_eq!(
            before_removal(&mut host, &paths, root.path(), Some("r")),
            Removal::Undone
        );
        let aside = std::fs::read_to_string(set_aside_path(root.path())).unwrap();
        assert!(aside.contains("electron-cursor"));
        assert!(!state.join(SET_ASIDE_TEMPORARY).exists());
    }
}
