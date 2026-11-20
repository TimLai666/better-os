//! What the Fixes page shows, decided from `patch-core`'s reports with no
//! GPUI dependency: which group each fault is in, its words, its state, the
//! exact change, and which of Fix and Undo it offers.

use std::collections::BTreeMap;

use patch_core::engine::Report;
use patch_core::entry::{self, ChangeLine, Limit};
use patch_core::flatpak::{AppId, AppName};
use patch_core::record::{ChangedElsewhere, KeyValue, OverrideKey};
use patch_core::{CannotCheck, CheckOutcome, Entry, EntryId, EntryState, Subject, UndoKind};

use super::copy::copy;
use crate::i18n::Locale;

/// One fault on one subject, which Fix and Undo act on.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct RowKey {
    pub(crate) entry: EntryId,
    pub(crate) subject: Subject,
}

/// How a line is coloured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tone {
    Neutral,
    Success,
    Warning,
}

/// What the last Fix or Undo on a row did, until the next check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RowResult {
    Fixed(CheckOutcome),
    Undone { skipped: usize },
}

/// A button on a row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RowAction {
    Fix,
    /// Fix offered again because the problem is still there and the fix
    /// has no undo.
    FixAgain,
    /// Undo the keys that still hold what the fix wrote.
    Undo,
    /// Undo everything, the keys another program changed included.
    UndoAll,
}

/// One fault's expander.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FaultRow {
    pub(crate) key: RowKey,
    pub(crate) header: String,
    pub(crate) cause: &'static str,
    /// The state line, with its tone.
    pub(crate) state: (String, Tone),
    /// The exact change, one line each.
    pub(crate) change: Vec<String>,
    /// What the undo does, or that none is needed.
    pub(crate) undo_note: &'static str,
    /// When the change takes effect, when that is not right away.
    pub(crate) takes_effect: Option<String>,
    pub(crate) limits: Vec<&'static str>,
    /// Each key another program changed after the fix, as a line.
    pub(crate) changed_elsewhere: Vec<String>,
    /// The first button is the accent one.
    pub(crate) actions: Vec<RowAction>,
}

/// One entry on "All known problems".
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KnownRow {
    pub(crate) entry: EntryId,
    pub(crate) header: &'static str,
    pub(crate) cause: &'static str,
    pub(crate) state: (String, Tone),
    pub(crate) limits: Vec<&'static str>,
}

/// The page's three groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PageRows {
    pub(crate) found: Vec<FaultRow>,
    pub(crate) applied: Vec<FaultRow>,
    pub(crate) all: Vec<KnownRow>,
}

/// An app's name in a language, or its ID when its desktop entry names it
/// in neither: the ID is a fact about the app, not a stand-in for one.
pub(crate) fn app_name(locale: Locale, name: &AppName, id: &AppId) -> String {
    let named = match locale.resolved() {
        Locale::ZhTw => name.zh_tw.clone().or_else(|| name.default.clone()),
        Locale::EnUs | Locale::System => name.default.clone(),
    };
    named.unwrap_or_else(|| id.to_string())
}

fn header_of(locale: Locale, entry: EntryId, app: Option<&str>) -> String {
    let c = copy(locale);
    match entry {
        EntryId::FontconfigCache => c.font_header.to_string(),
        EntryId::ElectronCursor => match app {
            Some(app) => c.electron_header.replace("{app}", app),
            None => c.electron_header_any.to_string(),
        },
    }
}

fn cause_of(locale: Locale, entry: EntryId) -> &'static str {
    let c = copy(locale);
    match entry {
        EntryId::FontconfigCache => c.font_cause,
        EntryId::ElectronCursor => c.electron_cause,
    }
}

fn limit_text(locale: Locale, limit: Limit) -> &'static str {
    let c = copy(locale);
    match limit {
        Limit::FlatpakOnly => c.limit_flatpak_only,
        Limit::ElectronBaseAppOnly => c.limit_base_app,
        Limit::UntilTheAppScalesItsPointer => c.limit_until_scaled,
    }
}

/// The program's own name, so a person can look it up.
fn program_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// Why a check could not run, in the person's words.
pub(crate) fn reason(locale: Locale, reason: &CannotCheck) -> String {
    let c = copy(locale);
    match reason {
        CannotCheck::ProgramMissing(program) => c
            .reason_program_missing
            .replace("{program}", program_name(program)),
        CannotCheck::FontQueryFailed(_) => c.reason_font_query.to_string(),
        CannotCheck::SessionTypeUnknown => c.reason_session_unknown.to_string(),
        CannotCheck::DisplayScaleUnreadable(_) => c.reason_scale.to_string(),
        CannotCheck::FlatpakAppsUnreadable(_) => c.reason_apps.to_string(),
        CannotCheck::FileUnreadable(_) => c.reason_file.to_string(),
        CannotCheck::NotAnswered => c.reason_not_answered.to_string(),
    }
}

/// A state's line and tone.
pub(crate) fn state_line(locale: Locale, state: &EntryState) -> (String, Tone) {
    let c = copy(locale);
    match state {
        EntryState::NotPresent => (c.state_not_present.to_string(), Tone::Neutral),
        EntryState::Found => (c.state_found.to_string(), Tone::Warning),
        EntryState::FixedAndChecked => (c.state_fixed.to_string(), Tone::Success),
        EntryState::AppliedButStillPresent => (c.state_still_present.to_string(), Tone::Warning),
        EntryState::Undone => (c.state_undone.to_string(), Tone::Neutral),
        EntryState::CannotCheck(why) => (
            c.state_cannot_check
                .replace("{reason}", &reason(locale, why)),
            Tone::Warning,
        ),
    }
}

/// What a Fix or an Undo just did, which a row says until the next check.
fn result_line(locale: Locale, result: &RowResult) -> (String, Tone) {
    let c = copy(locale);
    match result {
        RowResult::Fixed(CheckOutcome::Passed) => (c.state_fixed.to_string(), Tone::Success),
        RowResult::Fixed(CheckOutcome::StillPresent) => {
            (c.state_still_present.to_string(), Tone::Warning)
        }
        RowResult::Fixed(CheckOutcome::CannotCheck(why)) => (
            c.check_could_not_run
                .replace("{reason}", &reason(locale, why)),
            Tone::Warning,
        ),
        RowResult::Undone { skipped: 0 } => (c.state_undone.to_string(), Tone::Neutral),
        RowResult::Undone { skipped: 1 } => (c.undone_with_skipped_one.to_string(), Tone::Warning),
        RowResult::Undone { skipped } => (
            c.undone_with_skipped.replace("{n}", &skipped.to_string()),
            Tone::Warning,
        ),
    }
}

/// A key written as Flatpak's own command line would set it.
pub(crate) fn key_flag(key: &OverrideKey, value: &KeyValue) -> String {
    match (key, value) {
        (OverrideKey::Socket(name), KeyValue::Granted) => format!("--socket={name}"),
        (OverrideKey::Socket(name), KeyValue::Denied) => format!("--nosocket={name}"),
        (OverrideKey::Environment(name), KeyValue::Value(text)) => format!("--env={name}={text}"),
        (OverrideKey::Socket(name), _) | (OverrideKey::Environment(name), _) => name.clone(),
    }
}

/// The lines of "the exact change it makes". `override_dir` is the
/// person's override folder as the page shows it (`~/.local/...`).
pub(crate) fn change_lines(
    locale: Locale,
    change: &[ChangeLine],
    override_dir: &str,
) -> Vec<String> {
    let c = copy(locale);
    change
        .iter()
        .map(|line| match line {
            ChangeLine::Runs(command) => {
                let mut words = vec![program_name(command.program()).to_string()];
                words.extend(command.args());
                c.font_change.replace("{command}", &words.join(" "))
            }
            ChangeLine::OverrideKeys { app, keys } => {
                let flags: Vec<String> = keys.iter().map(|(k, v)| key_flag(k, v)).collect();
                c.electron_change
                    .replace("{path}", &format!("{override_dir}/{app}"))
                    .replace("{keys}", &flags.join(" "))
            }
        })
        .collect()
}

fn changed_lines(locale: Locale, changed: &[ChangedElsewhere]) -> Vec<String> {
    let words = crate::i18n::copy(locale);
    changed
        .iter()
        .map(|change| {
            let now = match &change.now {
                KeyValue::Absent => String::new(),
                value => format!(" ({})", key_flag(&change.key, value)),
            };
            let name = match &change.key {
                OverrideKey::Socket(name) | OverrideKey::Environment(name) => name,
            };
            format!("{name}{now}: {}", words.skip_changed_externally)
        })
        .collect()
}

/// The app a fault is about, by the name detection found or, once the fault
/// is gone, the name its fix recorded.
fn instance_app(locale: Locale, instance: &patch_core::Instance) -> Option<String> {
    let Subject::FlatpakApp(id) = &instance.subject else {
        return None;
    };
    let found = match &instance.finding {
        Some(patch_core::Finding {
            detail: patch_core::entry::FindingDetail::ElectronCursor { name, .. },
            ..
        }) => Some(name),
        _ => None,
    };
    let recorded = match instance.record.as_ref().map(|record| &record.change) {
        Some(patch_core::record::Recorded::OverrideKeys { name, .. }) => Some(name),
        _ => None,
    };
    let empty = AppName::default();
    Some(app_name(locale, found.or(recorded).unwrap_or(&empty), id))
}

#[allow(clippy::too_many_arguments)]
fn fault_row(
    locale: Locale,
    entry: &dyn Entry,
    key: RowKey,
    app: Option<&str>,
    state: (String, Tone),
    actions: Vec<RowAction>,
    changed: &[ChangedElsewhere],
    override_dir: &str,
) -> FaultRow {
    let c = copy(locale);
    let description = entry.describe(&key.subject);
    let reversible = description.undo == UndoKind::Reversible;
    FaultRow {
        header: header_of(locale, key.entry, app),
        cause: cause_of(locale, key.entry),
        state,
        change: change_lines(locale, &description.change, override_dir),
        undo_note: if reversible {
            c.electron_undo
        } else {
            c.no_undo_needed
        },
        takes_effect: app.map(|app| c.electron_restart.replace("{app}", app)),
        limits: description
            .limits
            .iter()
            .map(|limit| limit_text(locale, *limit))
            .collect(),
        changed_elsewhere: changed_lines(locale, changed),
        actions,
        key,
    }
}

/// Everything the page draws.
pub(crate) fn page_rows(
    locale: Locale,
    reports: &[Report],
    results: &BTreeMap<RowKey, RowResult>,
    override_dir: &str,
) -> PageRows {
    let c = copy(locale);
    let mut rows = PageRows::default();
    for report in reports {
        let entry: &dyn Entry = entry::by_id(report.entry);
        for instance in &report.instances {
            let key = RowKey {
                entry: report.entry,
                subject: instance.subject.clone(),
            };
            let app = instance_app(locale, instance);
            let description = entry.describe(&instance.subject);
            let reversible = description.undo == UndoKind::Reversible;
            let state = match results.get(&key) {
                Some(result) => result_line(locale, result),
                None => state_line(locale, &instance.state),
            };
            let (group_found, actions) = match &instance.state {
                EntryState::Found => (true, vec![RowAction::Fix]),
                EntryState::Undone if instance.finding.is_some() => (true, vec![RowAction::Fix]),
                EntryState::AppliedButStillPresent if reversible => (true, vec![RowAction::Undo]),
                EntryState::AppliedButStillPresent => (true, vec![RowAction::FixAgain]),
                EntryState::FixedAndChecked if reversible => (false, vec![RowAction::Undo]),
                EntryState::FixedAndChecked => (false, Vec::new()),
                // A recorded fix whose fault cannot be checked can still be
                // undone: an undo reads only the file it wrote.
                EntryState::CannotCheck(_) if instance.record.is_some() && reversible => {
                    (false, vec![RowAction::Undo])
                }
                EntryState::CannotCheck(_) | EntryState::NotPresent | EntryState::Undone => {
                    continue;
                }
            };
            let mut actions = actions;
            if actions.contains(&RowAction::Undo) && !instance.changed_elsewhere.is_empty() {
                actions.push(RowAction::UndoAll);
            }
            let row = fault_row(
                locale,
                entry,
                key,
                app.as_deref(),
                state,
                actions,
                &instance.changed_elsewhere,
                override_dir,
            );
            if group_found {
                rows.found.push(row);
            } else {
                rows.applied.push(row);
            }
        }
        // A fix that keeps no record (its fault is simply gone) still says
        // how it went until the next Check again.
        for (key, result) in results.iter().filter(|(key, _)| key.entry == report.entry) {
            let shown = report.instances.iter().any(|i| i.subject == key.subject);
            if shown || !matches!(result, RowResult::Fixed(_)) {
                continue;
            }
            let app = match &key.subject {
                Subject::FlatpakApp(id) => Some(id.to_string()),
                Subject::Machine => None,
            };
            rows.applied.push(fault_row(
                locale,
                entry,
                key.clone(),
                app.as_deref(),
                result_line(locale, result),
                Vec::new(),
                &[],
                override_dir,
            ));
        }
        let state = match report.overall() {
            EntryState::Found if report.needing_attention() > 1 => (
                c.state_found_many
                    .replace("{n}", &report.needing_attention().to_string()),
                Tone::Warning,
            ),
            other => state_line(locale, &other),
        };
        let limits = match report.entry {
            EntryId::FontconfigCache => Vec::new(),
            EntryId::ElectronCursor => vec![
                limit_text(locale, Limit::FlatpakOnly),
                limit_text(locale, Limit::ElectronBaseAppOnly),
            ],
        };
        rows.all.push(KnownRow {
            entry: report.entry,
            header: match report.entry {
                EntryId::FontconfigCache => c.font_header,
                EntryId::ElectronCursor => c.electron_header_any,
            },
            cause: cause_of(locale, report.entry),
            state,
            limits,
        });
    }
    rows
}

/// One fault Home and the pane count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FoundFault {
    pub(crate) entry: EntryId,
    pub(crate) app: Option<(AppId, AppName)>,
}

/// What Home and the pane know about the page: the faults that need the
/// person, in the page's order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FixesFacts {
    pub(crate) found: Vec<FoundFault>,
}

impl FixesFacts {
    pub(crate) fn of(reports: &[Report]) -> Self {
        let mut found = Vec::new();
        for report in reports {
            for instance in &report.instances {
                // An undone fix whose fault is back is a fault found again.
                let undone_and_back =
                    instance.state == EntryState::Undone && instance.finding.is_some();
                if !instance.state.needs_attention() && !undone_and_back {
                    continue;
                }
                let app = match (&instance.subject, &instance.finding) {
                    (
                        Subject::FlatpakApp(id),
                        Some(patch_core::Finding {
                            detail: patch_core::entry::FindingDetail::ElectronCursor { name, .. },
                            ..
                        }),
                    ) => Some((id.clone(), name.clone())),
                    (Subject::FlatpakApp(id), _) => Some((id.clone(), AppName::default())),
                    (Subject::Machine, _) => None,
                };
                found.push(FoundFault {
                    entry: report.entry,
                    app,
                });
            }
        }
        Self { found }
    }

    /// The first fault's one-line title, as the page heads it.
    pub(crate) fn first_title(&self, locale: Locale) -> Option<String> {
        let first = self.found.first()?;
        let app = first
            .app
            .as_ref()
            .map(|(id, name)| app_name(locale, name, id));
        Some(header_of(locale, first.entry, app.as_deref()))
    }
}
