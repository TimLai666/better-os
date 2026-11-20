//! What removing Better Manager takes back (ticket 90).
//!
//! A fix Better Manager applied is something Better OS added to the
//! machine, so it does not outlive the package (AGENTS.md). The package's
//! before-removal program (`/usr/lib/better-os/before-removal/better-manager`)
//! runs this as each person, before dpkg removes the files:
//!
//! - every recorded fix with an undo is undone through the same path the
//!   page's Undo takes, and the same rule: a key the person changed after the
//!   fix is left as they made it, and named;
//! - the record is then deleted;
//! - a fix with no undo (the rebuilt font cache) keeps no record and is left,
//!   since it restored the machine rather than adding to it.
//!
//! Before anything is changed, the record and each override file it names go
//! aside in [`UNDO_FILE`], so a removal that then does not happen can have
//! them back exactly (`--undo`). The package's postrm deletes that file once
//! the removal has happened.

use std::path::{Path, PathBuf};

#[cfg(test)]
use patch_core::Records;
use patch_core::UndoKind;
use patch_core::engine;
use patch_core::entry::{self, Answer, Change, Host, Read};
use patch_core::flatpak::AppId;
use patch_core::record::{KeyValue, OverrideKey, Recorded};
use serde::{Deserialize, Serialize};

use crate::paths::Paths;
use crate::store::RecordStore;

/// What a removal set aside, beside the record.
pub const UNDO_FILE: &str = "fixes-removal-undo.json";

/// Where [`UNDO_FILE`] is, for one person.
pub fn undo_path(paths: &Paths) -> PathBuf {
    paths
        .records
        .parent()
        .map_or_else(|| PathBuf::from(UNDO_FILE), |dir| dir.join(UNDO_FILE))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SetAside {
    /// The record file's text, `None` when there was none.
    records: Option<String>,
    /// Each override file a fix wrote, as it was before this removal.
    overrides: Vec<(AppId, Option<String>)>,
}

/// What one run did; the program turns it into its exit status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Removal {
    /// No fix with an undo was recorded.
    NothingApplied,
    /// Every recorded fix was undone and the record deleted.
    Undone,
    /// Undone, except these keys, which the person changed after the fix and
    /// which stay as they made them; the record is deleted all the same.
    LeftAsChanged(Vec<String>),
    /// Something could not be read or written; the reason. What could be
    /// undone was.
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

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text).map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(failed)
}

fn override_now(host: &mut dyn Host, app: &AppId) -> Result<Option<String>, String> {
    match host.read(&Read::UserOverride(app.clone())) {
        Answer::File(Ok(text)) => Ok(text),
        Answer::File(Err(error)) => Err(error.to_string()),
        _ => Err("the override file was not read".to_string()),
    }
}

/// The run before removal.
pub fn before_removal(host: &mut dyn Host, paths: &Paths) -> Removal {
    let undo = undo_path(paths);
    // An undo file is only ever about this run: one an earlier removal left
    // would make a refused removal's `--undo` write that older state back.
    if let Err(error) = remove(&undo) {
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
    let reversible: Vec<_> = records
        .applied
        .iter()
        .filter(|record| entry::by_id(record.entry).undo_kind() == UndoKind::Reversible)
        .cloned()
        .collect();

    let mut aside = SetAside {
        records: Some(text),
        overrides: Vec::new(),
    };
    for record in &reversible {
        if let Recorded::OverrideKeys { app, .. } = &record.change {
            match override_now(host, app) {
                Ok(now) => aside.overrides.push((app.clone(), now)),
                Err(error) => return Removal::Failed(error),
            }
        }
    }
    let aside_text = serde_json::to_string_pretty(&aside).expect("the set-aside state serializes");
    if let Err(error) = write_text(&undo, &aside_text) {
        return Removal::Failed(format!("what is there now could not be kept: {error}"));
    }

    let mut left = Vec::new();
    let mut failures = Vec::new();
    for record in &reversible {
        let entry = entry::by_id(record.entry);
        match engine::undo(entry, record, host, false) {
            Ok(outcome) => {
                let app = match &record.change {
                    Recorded::OverrideKeys { app, .. } => app.to_string(),
                    Recorded::RebuiltFontCache => String::new(),
                };
                left.extend(
                    outcome
                        .skipped
                        .iter()
                        .map(|changed| format!("{app}: {}", key_name(&changed.key, &changed.now))),
                );
            }
            Err(error) => failures.push(error.details()),
        }
    }
    if let Err(error) = remove(&paths.records) {
        failures.push(error);
    }
    if !failures.is_empty() {
        return Removal::Failed(failures.join("; "));
    }
    if left.is_empty() {
        if reversible.is_empty() {
            Removal::NothingApplied
        } else {
            Removal::Undone
        }
    } else {
        Removal::LeftAsChanged(left)
    }
}

/// `--undo`: the removal did not happen, so every override file and the
/// record go back to what they were before the run. Returns why not, when
/// something could not be put back.
pub fn undo_before_removal(host: &mut dyn Host, paths: &Paths) -> Result<(), String> {
    let undo = undo_path(paths);
    let Some(text) = read_text(&undo)? else {
        return Ok(());
    };
    let aside: SetAside =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", undo.display()))?;
    let mut failures = Vec::new();
    for (app, before) in &aside.overrides {
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
            failures.push(format!("{app}: {error}"));
        }
    }
    match &aside.records {
        Some(records) => {
            if let Err(error) = write_text(&paths.records, records) {
                failures.push(error);
            }
        }
        None => {
            if let Err(error) = remove(&paths.records) {
                failures.push(error);
            }
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("; "));
    }
    remove(&undo)
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
    /// recorded, the way the page leaves it.
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
        assert_eq!(before_removal(&mut host, &paths), Removal::Undone);
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
        assert_eq!(before_removal(&mut host, &paths), Removal::Undone);
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
            before_removal(&mut host, &paths),
            Removal::LeftAsChanged(vec![format!(
                "{SEEDED_APP}: ELECTRON_OZONE_PLATFORM_HINT (set to wayland)"
            )])
        );
        assert_eq!(
            override_text(&paths).as_deref(),
            Some("[Environment]\nELECTRON_OZONE_PLATFORM_HINT=wayland\n")
        );
        assert!(!paths.records.exists());
    }

    #[test]
    fn a_refused_removal_puts_everything_back() {
        let (root, paths) = fixed_machine(true);
        let fixed = override_text(&paths);
        let records = std::fs::read_to_string(&paths.records).unwrap();
        let mut host = fixture::host(root.path());
        before_removal(&mut host, &paths);
        assert!(undo_path(&paths).exists());
        undo_before_removal(&mut host, &paths).unwrap();
        assert_eq!(override_text(&paths), fixed);
        assert_eq!(std::fs::read_to_string(&paths.records).unwrap(), records);
        assert!(!undo_path(&paths).exists());
        // And a second --undo has nothing to do.
        undo_before_removal(&mut host, &paths).unwrap();
        assert_eq!(override_text(&paths), fixed);
    }

    #[test]
    fn nothing_recorded_is_nothing_to_do() {
        let root = tempfile::tempdir().unwrap();
        fixture::seed(root.path(), Seed::BothFaults).unwrap();
        let paths = Paths::under(root.path());
        let mut host = fixture::host(root.path());
        assert_eq!(before_removal(&mut host, &paths), Removal::NothingApplied);
        assert!(!undo_path(&paths).exists());
        undo_before_removal(&mut host, &paths).unwrap();
    }

    #[test]
    fn a_record_it_cannot_read_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        std::fs::create_dir_all(paths.records.parent().unwrap()).unwrap();
        std::fs::write(&paths.records, "{ not json").unwrap();
        let mut host = fixture::host(root.path());
        assert!(matches!(
            before_removal(&mut host, &paths),
            Removal::Failed(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&paths.records).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn an_old_undo_file_is_never_used() {
        let (root, paths) = fixed_machine(true);
        write_text(&undo_path(&paths), "{\"records\":null,\"overrides\":[]}").unwrap();
        let mut host = fixture::host(root.path());
        assert_eq!(before_removal(&mut host, &paths), Removal::Undone);
        let aside = std::fs::read_to_string(undo_path(&paths)).unwrap();
        assert!(aside.contains("electron-cursor"));
    }
}
