//! What the Fixes page keeps, and the checks, fixes, and undos it runs off
//! the UI thread through `patch-core`'s engine and `patch-platform`'s host.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{AppContext as _, Context, Task};
use patch_core::engine::{self, FixError, Report};
use patch_core::entry::{self, PlanError};
use patch_core::{ChangeError, Entry, Records, Subject};
use patch_platform::store::StoreError;
use patch_platform::{LinuxHost, RecordStore, fixture};

use super::model::{FixesFacts, RowKey, RowResult};
use crate::app::ManagerApp;

/// Which machine the page checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HostChoice {
    /// The person's own.
    User,
    /// A fixture machine under a directory: the demo window's, a sandbox's,
    /// and a test build's.
    Fixture(PathBuf),
    /// A demo window with no private place for its fixture machine; why.
    DemoUnavailable(String),
}

/// Where a test build's page looks: nowhere that exists, so no test reaches
/// the person's font cache or Flatpak overrides.
const TEST_ROOT: &str = "/nonexistent/better-os-test-root";

/// The demo window's fixture machine, under the session's private runtime
/// directory and nowhere else (`patch_platform::fixture::private_demo_root`),
/// seeded once with both faults so a demo shows the whole flow and never
/// changes this machine.
fn demo_root() -> Result<PathBuf, String> {
    static ROOT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        fixture::private_demo_root(runtime.as_deref())
    })
    .clone()
}

/// The machine this window's page checks. A debug build takes
/// `BETTER_OS_DEBUG_FIXES_ROOT`, a fixture machine a sandbox screenshot
/// points it at; a release build never reads it.
pub(crate) fn host_choice(demo: bool) -> HostChoice {
    if cfg!(test) {
        return HostChoice::Fixture(PathBuf::from(TEST_ROOT));
    }
    if cfg!(debug_assertions)
        && let Some(root) = std::env::var_os("BETTER_OS_DEBUG_FIXES_ROOT")
    {
        return HostChoice::Fixture(PathBuf::from(root));
    }
    if demo {
        return match demo_root() {
            Ok(root) => HostChoice::Fixture(root),
            Err(why) => HostChoice::DemoUnavailable(why),
        };
    }
    HostChoice::User
}

fn open(choice: &HostChoice) -> Option<(LinuxHost, RecordStore)> {
    match choice {
        HostChoice::User => {
            let host = LinuxHost::for_user()?;
            let store = RecordStore::at(host.paths().records.clone());
            Some((host, store))
        }
        HostChoice::Fixture(root) => Some((fixture::host(root), fixture::store(root))),
        HostChoice::DemoUnavailable(_) => None,
    }
}

/// The person's override folder as the page prints it, with `~` for home.
fn override_dir(choice: &HostChoice) -> String {
    let directory = match choice {
        HostChoice::User => patch_platform::Paths::for_user(),
        HostChoice::Fixture(root) => Some(patch_platform::Paths::under(root)),
        HostChoice::DemoUnavailable(_) => None,
    }
    .map(|paths| paths.user_flatpak.join("overrides"));
    let Some(directory) = directory else {
        return "~/.local/share/flatpak/overrides".to_string();
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home
        .as_ref()
        .and_then(|home| directory.strip_prefix(home).ok())
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => directory.display().to_string(),
    }
}

/// Leaves, at the default state location, where the record and the
/// overrides are when they are somewhere else, so removing Better Manager
/// finds them (`patch_platform::paths::keep_location`).
fn keep_location(choice: &HostChoice, host: &LinuxHost) -> Result<(), String> {
    match (choice, home()) {
        (HostChoice::User, Some(home)) => patch_platform::paths::keep_location(&home, host.paths()),
        _ => Ok(()),
    }
}

/// What is running.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Work {
    Fixing(RowKey),
    Undoing(RowKey),
}

/// A failure the page shows in an error bar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Failure {
    pub(crate) summary: &'static str,
    pub(crate) details: String,
}

/// What one check read.
struct Checked {
    reports: Vec<Report>,
    records: Result<Records, StoreError>,
    /// A removal of Better Manager that did not finish, and how many fixes
    /// it undid (`patch_platform::removal::interrupted_removal`).
    interrupted: Option<usize>,
}

/// Why nothing can be checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Unavailable {
    /// No home directory.
    NoHome,
    /// A demo window with no private runtime directory; why.
    Demo(String),
}

/// The person's home, for the files a removal reads at fixed places.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
}

#[derive(Default)]
pub(crate) struct FixesState {
    /// `None` until the first check lands.
    pub(crate) reports: Option<Vec<Report>>,
    pub(crate) checked_at: Option<u64>,
    /// Set while the record of applied fixes cannot be read; nothing is
    /// fixed or undone then, so the record is never overwritten.
    pub(crate) record_error: Option<String>,
    /// Why nothing can be checked, when nothing can.
    pub(crate) unavailable: Option<Unavailable>,
    /// A removal of Better Manager that did not finish undid this many
    /// fixes; said once on the page.
    pub(crate) interrupted: Option<usize>,
    pub(crate) reading_since: Option<Instant>,
    pub(crate) work: Option<Work>,
    pub(crate) results: BTreeMap<RowKey, RowResult>,
    pub(crate) failure: Option<Failure>,
    /// The expanders that are open.
    pub(crate) open: BTreeSet<RowKey>,
    /// The "All known problems" expanders that are open.
    pub(crate) known_open: BTreeSet<patch_core::EntryId>,
    /// Subjects undone in this window, which read "Undone" until the next
    /// Check again.
    just_undone: Vec<RowKey>,
    again: bool,
    task: Option<Task<()>>,
    work_task: Option<Task<()>>,
}

impl FixesState {
    /// A check has run long enough for a ring.
    pub(crate) fn shows_ring(&self) -> bool {
        self.task.is_some()
            && self.reading_since.is_some_and(|since| {
                since.elapsed()
                    >= Duration::from_millis(crate::pages::states::LOADING_RING_AFTER_MS)
            })
    }

    pub(crate) fn checking(&self) -> bool {
        self.task.is_some()
    }

    /// A fix or an undo is changing the machine: an update restart waits.
    pub(crate) fn blocks_restart(&self) -> bool {
        self.work.is_some()
    }

    /// Fix and Undo are offered only with a readable record and nothing
    /// running.
    pub(crate) fn can_change(&self) -> bool {
        self.work.is_none() && self.task.is_none() && self.record_error.is_none()
    }

    /// What Home and the pane know.
    pub(crate) fn facts(&self) -> FixesFacts {
        self.reports
            .as_deref()
            .map(FixesFacts::of)
            .unwrap_or_default()
    }
}

fn now_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn check_all(host: &mut LinuxHost, records: &Records, undone: &[RowKey]) -> Vec<Report> {
    entry::all()
        .into_iter()
        .map(|entry| {
            let subjects: Vec<Subject> = undone
                .iter()
                .filter(|key| key.entry == entry.id())
                .map(|key| key.subject.clone())
                .collect();
            engine::check(entry, host, records, &subjects)
        })
        .collect()
}

fn read(choice: &HostChoice, undone: &[RowKey]) -> Result<Checked, Unavailable> {
    if let HostChoice::DemoUnavailable(why) = choice {
        return Err(Unavailable::Demo(why.clone()));
    }
    let (mut host, store) = open(choice).ok_or(Unavailable::NoHome)?;
    // Only the person's own home can hold what a removal set aside.
    let interrupted = match choice {
        HostChoice::User => {
            home().and_then(|home| patch_platform::removal::interrupted_removal(&home))
        }
        _ => None,
    };
    let records = store.load();
    let reports = check_all(
        &mut host,
        records.as_ref().unwrap_or(&Records::default()),
        undone,
    );
    Ok(Checked {
        reports,
        records,
        interrupted,
    })
}

/// Whether dpkg is removing Better Manager right now: its prerm may already
/// have undone the fixes, so nothing new may be written.
fn being_removed(choice: &HostChoice) -> bool {
    use better_core::self_update::PackageSource as _;
    *choice == HostChoice::User
        && better_core::self_update::DpkgSource::system()
            .record(manager_core::SELF_COMPONENT_ID)
            .is_some_and(|record| record.removing)
}

/// How a fix or an undo ended.
enum Done {
    Fixed {
        key: RowKey,
        outcome: patch_core::CheckOutcome,
        saved: Result<(), StoreError>,
    },
    Undone {
        key: RowKey,
        skipped: usize,
        saved: Result<(), StoreError>,
    },
    Failed(Failure),
}

fn failure(locale: crate::i18n::Locale, fixing: bool, error: &FixError) -> Failure {
    let c = super::copy::copy(locale);
    let summary = match error {
        FixError::Change {
            error: ChangeError::ChangedSinceRead,
            ..
        } => c.changed_while_fixing,
        FixError::Plan(PlanError::NothingToChange) => c.nothing_to_change,
        _ if fixing => c.fix_failed,
        _ => c.undo_failed,
    };
    Failure {
        summary,
        details: error.details(),
    }
}

impl ManagerApp {
    /// Checks every entry off the UI thread. A check asked for while one
    /// runs runs once that one lands.
    pub(crate) fn refresh_fixes(&mut self, cx: &mut Context<Self>) {
        if self.fixes.task.is_some() {
            self.fixes.again = true;
            return;
        }
        self.fixes.reading_since = Some(Instant::now());
        let choice = host_choice(self.is_demo());
        let undone = self.fixes.just_undone.clone();
        self.fixes.task = Some(cx.spawn(async move |this, cx| {
            let ring = cx.background_executor().timer(Duration::from_millis(
                crate::pages::states::LOADING_RING_AFTER_MS,
            ));
            let work = cx.background_spawn(async move { read(&choice, &undone) });
            // Redraw once the ring is due, so a slow check shows it.
            let ring_task = cx.spawn({
                let this = this.clone();
                async move |cx| {
                    ring.await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                }
            });
            let checked = work.await;
            drop(ring_task);
            let _ = this.update(cx, |app, cx| {
                app.fixes.task = None;
                app.fixes.reading_since = None;
                match checked {
                    Err(why) => app.fixes.unavailable = Some(why),
                    Ok(checked) => {
                        app.fixes.unavailable = None;
                        if checked.interrupted.is_some() {
                            app.fixes.interrupted = checked.interrupted;
                        }
                        app.fixes.record_error = checked.records.err().map(|e| e.to_string());
                        app.fixes.reports = Some(checked.reports);
                        app.fixes.checked_at = Some(now_unix_seconds());
                    }
                }
                if std::mem::take(&mut app.fixes.again) {
                    app.refresh_fixes(cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Check again: forgets the last results and the undone marks, then
    /// checks.
    pub(crate) fn check_fixes_again(&mut self, cx: &mut Context<Self>) {
        self.fixes.results.clear();
        self.fixes.just_undone.clear();
        self.fixes.failure = None;
        self.refresh_fixes(cx);
    }

    /// Fixes one fault: plans from a fresh read, changes, records, and
    /// checks, off the UI thread; then checks the page again.
    pub(crate) fn fix_fault(&mut self, key: RowKey, cx: &mut Context<Self>) {
        if !self.fixes.can_change() || self.update_restart.program_removed() {
            return;
        }
        let finding = self
            .fixes
            .reports
            .as_ref()
            .and_then(|reports| reports.iter().find(|r| r.entry == key.entry))
            .and_then(|report| report.instances.iter().find(|i| i.subject == key.subject))
            .and_then(|instance| instance.finding.clone());
        let Some(finding) = finding else {
            return;
        };
        let choice = host_choice(self.is_demo());
        let locale = self.locale;
        self.fixes.work = Some(Work::Fixing(key.clone()));
        self.fixes.failure = None;
        self.fixes.results.remove(&key);
        self.fixes.just_undone.retain(|undone| undone != &key);
        let task_key = key.clone();
        self.fixes.work_task = Some(cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move {
                    let key = task_key;
                    if being_removed(&choice) {
                        return Done::Failed(Failure {
                            summary: super::copy::copy(locale).program_removed,
                            details: String::new(),
                        });
                    }
                    let Some((mut host, store)) = open(&choice) else {
                        return Done::Failed(Failure {
                            summary: super::copy::copy(locale).host_unavailable,
                            details: String::new(),
                        });
                    };
                    let mut records = match store.load() {
                        Ok(records) => records,
                        Err(error) => {
                            return Done::Failed(Failure {
                                summary: super::copy::copy(locale).record_unreadable,
                                details: error.to_string(),
                            });
                        }
                    };
                    let entry: &dyn Entry = entry::by_id(key.entry);
                    match engine::fix(entry, &finding, &mut host, now_unix_seconds()) {
                        Ok(outcome) => {
                            records.keep(entry, outcome.record);
                            let saved = if entry.undo_kind() == patch_core::UndoKind::Reversible {
                                store.save(&records).and_then(|()| {
                                    keep_location(&choice, &host).map_err(StoreError::Unwritable)
                                })
                            } else {
                                Ok(())
                            };
                            Done::Fixed {
                                key,
                                outcome: outcome.check,
                                saved,
                            }
                        }
                        Err(error) => {
                            // A change that failed part way still leaves a
                            // record, so what it did can be undone.
                            if let FixError::Change {
                                applied: Some(record),
                                ..
                            } = &error
                            {
                                records.keep(entry, (**record).clone());
                                let _ = store.save(&records);
                            }
                            Done::Failed(failure(locale, true, &error))
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| app.fixes_done(done, cx));
        }));
        cx.notify();
    }

    /// Undoes one fix from its kept record. `all` also replaces the keys
    /// another program changed after the fix, which the person confirmed by
    /// choosing Undo all.
    pub(crate) fn undo_fix(&mut self, key: RowKey, all: bool, cx: &mut Context<Self>) {
        if !self.fixes.can_change() || self.update_restart.program_removed() {
            return;
        }
        let choice = host_choice(self.is_demo());
        let locale = self.locale;
        self.fixes.work = Some(Work::Undoing(key.clone()));
        self.fixes.failure = None;
        self.fixes.results.remove(&key);
        let task_key = key.clone();
        self.fixes.work_task = Some(cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move {
                    let key = task_key;
                    if being_removed(&choice) {
                        return Done::Failed(Failure {
                            summary: super::copy::copy(locale).program_removed,
                            details: String::new(),
                        });
                    }
                    let Some((mut host, store)) = open(&choice) else {
                        return Done::Failed(Failure {
                            summary: super::copy::copy(locale).host_unavailable,
                            details: String::new(),
                        });
                    };
                    let mut records = match store.load() {
                        Ok(records) => records,
                        Err(error) => {
                            return Done::Failed(Failure {
                                summary: super::copy::copy(locale).record_unreadable,
                                details: error.to_string(),
                            });
                        }
                    };
                    let Some(record) = records.find(key.entry, &key.subject).cloned() else {
                        // Nothing recorded: nothing this page may take back.
                        return Done::Undone {
                            key,
                            skipped: 0,
                            saved: Ok(()),
                        };
                    };
                    let entry: &dyn Entry = entry::by_id(key.entry);
                    match engine::undo(entry, &record, &mut host, all) {
                        Ok(outcome) => {
                            records.after_undo(key.entry, &key.subject, outcome.remaining);
                            Done::Undone {
                                key,
                                skipped: outcome.skipped.len(),
                                saved: store.save(&records),
                            }
                        }
                        Err(error) => Done::Failed(failure(locale, false, &error)),
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| app.fixes_done(done, cx));
        }));
        cx.notify();
    }

    fn fixes_done(&mut self, done: Done, cx: &mut Context<Self>) {
        let c = super::copy::copy(self.locale);
        self.fixes.work = None;
        self.fixes.work_task = None;
        let unsaved = |error: StoreError| Failure {
            summary: c.record_unsaved,
            details: error.to_string(),
        };
        match done {
            Done::Fixed {
                key,
                outcome,
                saved,
            } => {
                self.fixes.results.insert(key, RowResult::Fixed(outcome));
                self.fixes.failure = saved.err().map(unsaved);
            }
            Done::Undone {
                key,
                skipped,
                saved,
            } => {
                self.fixes
                    .results
                    .insert(key.clone(), RowResult::Undone { skipped });
                if skipped == 0 {
                    self.fixes.just_undone.push(key);
                }
                self.fixes.failure = saved.err().map(unsaved);
            }
            Done::Failed(failure) => self.fixes.failure = Some(failure),
        }
        // What the page shows next comes from a fresh check, not from what
        // the fix believed it did; the result line stays until Check again.
        self.refresh_fixes(cx);
        cx.notify();
    }

    /// The override folder the page names.
    pub(crate) fn fixes_override_dir(&self) -> String {
        override_dir(&host_choice(self.is_demo()))
    }
}
