//! The production adapter set, pointed at a temporary `XDG_CONFIG_HOME`.
//!
//! The other engine tests hand the XDG adapter a file path directly. This one
//! goes through `AdapterSession::open(&AdapterMode::Production)`, which finds
//! `mimeapps.list` the way a real run does, so what it proves is the wiring a
//! user gets. The environment is set on a child copy of this test binary rather
//! than on this process, so no other test ever sees it and nothing here can
//! reach the real home directory.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use better_core::defaults::ObservedValue;
use common::*;
use defaults_core::{
    AdapterMode, AdapterSession, ComponentReadiness, Confirmations, DefaultsEngine, EntryOutcome,
    PlanAction, Selection, SystemContext,
};
use defaults_store::SnapshotStore;

const CHILD: &str = "DEFAULTS_CORE_PRODUCTION_XDG_CHILD";
const FILES: &str = "io.betteros.Files.desktop";
const ORIGINAL: &str = "# the user's own file\n\
    [Default Applications]\n\
    image/png=org.gnome.eog.desktop\n";

struct Home {
    directory: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("config")).unwrap();
        std::fs::write(directory.path().join("config/mimeapps.list"), ORIGINAL).unwrap();
        Self { directory }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    /// Runs `test` again in a child whose whole XDG world is this directory.
    fn run_child(&self, test: &str) {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env_clear()
            .env(CHILD, self.directory.path())
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("data"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "the child ran no test"
        );
    }
}

/// The child's side: the temporary root it was given, after checking that the
/// environment really points there.
fn child_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os(CHILD)?);
    for key in ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME"] {
        let value = PathBuf::from(std::env::var_os(key).expect("set by the parent"));
        assert!(
            value.starts_with(&root),
            "{key} must be inside the temporary root"
        );
    }
    Some(root)
}

fn engine(catalog: &better_core::ComponentCatalog) -> DefaultsEngine<'_> {
    DefaultsEngine::new(catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready())
}

fn run(
    catalog: &better_core::ComponentCatalog,
    session: &mut AdapterSession,
    store: &SnapshotStore,
    restore: bool,
) -> defaults_core::DefaultsOutcome {
    let history = store.history().unwrap();
    let plan = if restore {
        engine(catalog).plan_restore(
            &Selection::All,
            session.adapters(),
            &history,
            &Confirmations::none(),
        )
    } else {
        engine(catalog).plan_apply(
            &Selection::All,
            session.adapters(),
            &history,
            &Confirmations::none(),
        )
    };
    if restore {
        assert_eq!(
            plan.entries[0].action,
            PlanAction::Restore {
                to: ObservedValue::Unset
            }
        );
    }
    engine(catalog)
        .execute(&plan, session.adapters_mut(), store)
        .unwrap()
}

fn apply_then_restore_a_type_that_had_no_owner(root: &Path) {
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let store = SnapshotStore::at_path(root.join("snapshots"));
    let mut session = AdapterSession::open(&AdapterMode::Production).unwrap();
    let mimeapps = root.join("config/mimeapps.list");

    let applied = run(&catalog, &mut session, &store, false);
    assert!(applied.results[0].outcome.is_success(), "{applied:?}");
    assert!(
        std::fs::read_to_string(&mimeapps)
            .unwrap()
            .contains("inode/directory=io.betteros.Files.desktop")
    );

    let restored = run(&catalog, &mut session, &store, true);
    assert_eq!(
        restored.results[0].outcome,
        EntryOutcome::Restored {
            value: ObservedValue::Unset
        }
    );
}

#[test]
fn restoring_a_type_that_had_no_owner_clears_it_in_the_users_own_file() {
    if let Some(root) = child_root() {
        apply_then_restore_a_type_that_had_no_owner(&root);
        return;
    }
    let home = Home::new();
    home.run_child("restoring_a_type_that_had_no_owner_clears_it_in_the_users_own_file");

    assert_eq!(
        std::fs::read_to_string(home.path("config/mimeapps.list")).unwrap(),
        ORIGINAL,
        "every byte of the user's file is back"
    );
    // Both changes left a rollback record in the temporary data directory,
    // written before the file was touched.
    let records = std::fs::read_dir(home.path("data/better-os/app-chooser/rollback"))
        .unwrap()
        .count();
    assert_eq!(records, 2);
}
