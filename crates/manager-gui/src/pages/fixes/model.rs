//! What the Fixes page shows, decided from `patch-core`'s reports with no
//! GPUI dependency: which group each fault is in, its words, its state, the
//! exact change, and which of Fix and Undo it offers.

use std::collections::BTreeMap;

use hardware_core::usb_recovery::{
    ChargerShown, RecoveryReply, Restored, UsbChoice, UsbRecoverySettings,
};
use patch_core::engine::Report;
use patch_core::entry::{self, ChangeLine, Limit};
use patch_core::flatpak::{AppId, AppName};
use patch_core::record::{ChangedElsewhere, KeyValue, OverrideKey};
use patch_core::{
    CannotCheck, CheckOutcome, Entry, EntryId, EntryState, Privileged, Subject, UndoKind,
};

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
    Undone {
        skipped: usize,
    },
    /// What Reset USB-C restored.
    Reset(RecoveryReply),
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
    /// Reset the USB-C interface, or restart the dead controller, now.
    ResetUsbC,
    /// Choose whether a sleep that keeps failing with the lid closed turns
    /// the PC off (`true`) or only notifies (`false`).
    PowerOff(bool),
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
    /// Facts about what was found (the app's Electron version).
    pub(crate) facts: Vec<String>,
    pub(crate) limits: Vec<&'static str>,
    /// Each key another program changed after the fix, as a line.
    pub(crate) changed_elsewhere: Vec<String>,
    /// The first button is the accent one.
    pub(crate) actions: Vec<RowAction>,
    /// Under "Problems found": drawn with the warning sign whatever its
    /// state line says.
    pub(crate) found: bool,
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
        EntryId::UsbCInterface => c.usb_header.to_string(),
        EntryId::UsbController => c.controller_header.to_string(),
    }
}

fn known_header(locale: Locale, entry: EntryId) -> &'static str {
    let c = copy(locale);
    match entry {
        EntryId::FontconfigCache => c.font_header,
        EntryId::ElectronCursor => c.electron_header_any,
        EntryId::UsbCInterface => c.usb_header,
        EntryId::UsbController => c.controller_header,
    }
}

fn cause_of(locale: Locale, entry: EntryId) -> &'static str {
    let c = copy(locale);
    match entry {
        EntryId::FontconfigCache => c.font_cause,
        EntryId::ElectronCursor => c.electron_cause,
        EntryId::UsbCInterface => c.usb_cause,
        EntryId::UsbController => c.controller_cause,
    }
}

fn limit_text(locale: Locale, limit: Limit) -> &'static str {
    let c = copy(locale);
    match limit {
        Limit::FlatpakOnly => c.limit_flatpak_only,
        Limit::ElectronBaseAppOnly => c.limit_base_app,
        Limit::UntilTheAppScalesItsPointer => c.limit_until_scaled,
        Limit::WholeNumberScaleOnly => c.limit_whole_scale,
        Limit::ScreenSharing => c.limit_screen_sharing,
        Limit::KnownModelsOnly => c.limit_known_models,
        Limit::ResetsNotRepairs => c.limit_resets,
        Limit::RestartWhenStuck => c.limit_restart_when_stuck,
        Limit::NotMeasured => c.limit_not_measured,
    }
}

/// Why a found fault's fix is not offered, when it is not.
pub(crate) fn not_offered(
    instance: &patch_core::Instance,
) -> Option<patch_core::entry::NotOffered> {
    match &instance.finding {
        Some(patch_core::Finding {
            detail:
                patch_core::entry::FindingDetail::ElectronCursor {
                    offer: patch_core::entry::Offer::NotOffered(why),
                    ..
                },
            ..
        }) => Some(*why),
        _ => None,
    }
}

fn not_offered_text(locale: Locale, why: patch_core::entry::NotOffered) -> &'static str {
    let c = copy(locale);
    match why {
        patch_core::entry::NotOffered::FractionalScale => c.not_offered_fractional,
        patch_core::entry::NotOffered::MixedScales => c.not_offered_mixed,
        patch_core::entry::NotOffered::ElectronVersionUnknown => c.not_offered_unknown,
    }
}

/// The app's Electron, as its own version file says, when known.
fn electron_of(instance: &patch_core::Instance) -> Option<&str> {
    match &instance.finding {
        Some(patch_core::Finding {
            detail: patch_core::entry::FindingDetail::ElectronCursor { electron, .. },
            ..
        }) => electron.as_deref(),
        _ => None,
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
        CannotCheck::NotOnThisDesktop => c.reason_not_on_desktop.to_string(),
        CannotCheck::KernelLogUnreadable(_) => c.reason_kernel_log.to_string(),
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
        EntryState::DoesNotApply => (c.state_does_not_apply.to_string(), Tone::Neutral),
    }
}

/// What Reset USB-C restored, in words.
pub(crate) fn reset_line(locale: Locale, reply: &RecoveryReply) -> (String, Tone) {
    let c = copy(locale);
    let (text, tone) = match &reply.restored {
        Restored::Ucsi {
            ports,
            expected_ports,
            ..
        } if ports < expected_ports => (
            c.reset_ports_missing
                .replace("{n}", &ports.to_string())
                .replace("{m}", &expected_ports.to_string()),
            Tone::Warning,
        ),
        Restored::Ucsi { charger, .. } => match charger {
            ChargerShown::Online => (c.reset_ports_back.to_string(), Tone::Success),
            ChargerShown::NoneConnected => {
                (c.reset_ports_back_no_charger.to_string(), Tone::Success)
            }
            ChargerShown::Unknown => (
                c.reset_ports_back_charger_unknown.to_string(),
                Tone::Success,
            ),
            ChargerShown::NotShown => (c.reset_charger_missing.to_string(), Tone::Warning),
        },
        Restored::Controller { devices, .. } if reply.restored.complete() => (
            c.reset_controller_back.replace("{n}", &devices.to_string()),
            Tone::Success,
        ),
        Restored::Controller { .. } => (c.reset_controller_missing.to_string(), Tone::Warning),
    };
    if reply.recent {
        (format!("{text} {}", c.reset_recent), tone)
    } else {
        (text, tone)
    }
}

/// A daemon refusal or failure from the USB-C actions, in words.
pub(crate) fn usb_refusal(locale: Locale, error: &patch_core::ChangeError) -> &'static str {
    let c = copy(locale);
    let text = error.to_string();
    if let patch_core::ChangeError::NotInstalled(_) = error {
        return c.daemon_missing;
    }
    match hardware_core::usb_recovery::Refusal::from_key(&text) {
        Some(hardware_core::usb_recovery::Refusal::NothingToReset) => c.refused_nothing,
        Some(hardware_core::usb_recovery::Refusal::RestartNeeded) => c.refused_restart,
        Some(hardware_core::usb_recovery::Refusal::NotAffected) => c.refused_not_affected,
        Some(hardware_core::usb_recovery::Refusal::ControllerNotRecognised) => {
            c.refused_not_recognised
        }
        None if text.contains("daemon.error.auth_cancelled") => c.refused_cancelled,
        None if text.contains("daemon.error.unauthorized") => c.refused_not_allowed,
        None if text.contains("daemon.error.busy")
            || text.contains("daemon.error.shutting_down") =>
        {
            c.refused_busy
        }
        None => c.reset_failed,
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
        RowResult::Reset(reply) => reset_line(locale, reply),
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
            ChangeLine::Privileged(action) => match action {
                Privileged::ReloadUcsi => c.change_ucsi_now.to_string(),
                Privileged::RebindController => c.change_controller_now.to_string(),
                Privileged::SetChoice { choice, .. } => match choice {
                    UsbChoice::UcsiAutomatic => c.change_ucsi_auto.to_string(),
                    UsbChoice::ControllerAutomatic => c.change_controller_auto.to_string(),
                    UsbChoice::PowerOffWhenSleepBlocked => c.change_power_off.to_string(),
                },
            },
        })
        .collect()
}

/// What detection found about a USB-C fault, as lines.
fn usb_facts(locale: Locale, instance: &patch_core::Instance) -> Vec<String> {
    let c = copy(locale);
    match &instance.finding {
        Some(patch_core::Finding {
            detail:
                patch_core::entry::FindingDetail::UsbCInterface {
                    timeouts,
                    after_sleep,
                },
            ..
        }) => vec![
            c.fact_ucsi_seen
                .replace("{n}", &(timeouts + after_sleep).to_string())
                .replace("{m}", &after_sleep.to_string()),
        ],
        Some(patch_core::Finding {
            detail:
                patch_core::entry::FindingDetail::UsbController {
                    address,
                    sleep_failures,
                    streak,
                    restart_needed,
                },
            ..
        }) => {
            let mut lines = vec![c.fact_controller_seen.replace("{address}", address)];
            if *sleep_failures > 0 {
                lines.push(
                    c.fact_sleep_blocked
                        .replace("{n}", &sleep_failures.to_string())
                        .replace("{m}", &streak.to_string()),
                );
            }
            if *restart_needed {
                lines.push(c.fact_restart_needed.to_string());
            }
            lines
        }
        _ => Vec::new(),
    }
}

/// Whether a USB-C fault row's Reset USB-C can do anything now: the fault
/// is there and, for the controller, its hub work is not stuck.
fn resettable(instance: &patch_core::Instance) -> bool {
    match &instance.finding {
        Some(patch_core::Finding {
            detail: patch_core::entry::FindingDetail::UsbCInterface { .. },
            ..
        }) => true,
        Some(patch_core::Finding {
            detail: patch_core::entry::FindingDetail::UsbController { restart_needed, .. },
            ..
        }) => !restart_needed,
        _ => false,
    }
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
        undo_note: match key.entry {
            EntryId::UsbCInterface | EntryId::UsbController => c.usb_undo,
            _ if reversible => c.electron_undo,
            _ => c.no_undo_needed,
        },
        takes_effect: app.map(|app| c.electron_restart.replace("{app}", app)),
        facts: Vec::new(),
        limits: description
            .limits
            .iter()
            .map(|limit| limit_text(locale, *limit))
            .collect(),
        changed_elsewhere: changed_lines(locale, changed),
        actions,
        key,
        found: false,
    }
}

/// Everything the page draws, with no USB-C choice made (the tests' view).
#[cfg(test)]
pub(crate) fn page_rows(
    locale: Locale,
    reports: &[Report],
    results: &BTreeMap<RowKey, RowResult>,
    override_dir: &str,
) -> PageRows {
    page_rows_with(
        locale,
        reports,
        results,
        override_dir,
        &UsbRecoverySettings::default(),
    )
}

/// Everything the page draws; `usb` is the daemon's USB-C choices, for the
/// controller card's power-off choice.
pub(crate) fn page_rows_with(
    locale: Locale,
    reports: &[Report],
    results: &BTreeMap<RowKey, RowResult>,
    override_dir: &str,
    usb: &UsbRecoverySettings,
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
            let refused = not_offered(instance);
            let state = match (results.get(&key), refused) {
                (Some(result), _) => result_line(locale, result),
                (None, Some(patch_core::entry::NotOffered::ElectronVersionUnknown))
                    if instance.record.is_none() =>
                {
                    (c.cannot_tell.to_string(), Tone::Warning)
                }
                (None, Some(_)) if instance.record.is_none() => {
                    (c.not_offered.to_string(), Tone::Warning)
                }
                (None, _) => state_line(locale, &instance.state),
            };
            let (group_found, actions) = match &instance.state {
                // Found, and its fix is not offered here: it is listed with
                // the reason, and nothing to press.
                EntryState::Found | EntryState::Undone
                    if refused.is_some() && instance.record.is_none() =>
                {
                    (true, Vec::new())
                }
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
                EntryState::CannotCheck(_)
                | EntryState::NotPresent
                | EntryState::Undone
                | EntryState::DoesNotApply => {
                    continue;
                }
            };
            let mut actions = actions;
            if actions.contains(&RowAction::Undo) && !instance.changed_elsewhere.is_empty() {
                actions.push(RowAction::UndoAll);
            }
            let usb_entry = matches!(
                report.entry,
                EntryId::UsbCInterface | EntryId::UsbController
            );
            if usb_entry && resettable(instance) {
                actions.push(RowAction::ResetUsbC);
            }
            // The power-off choice belongs to entry 4's fix, once it is on.
            let power_off = (report.entry == EntryId::UsbController && instance.record.is_some())
                .then_some(usb.power_off_when_sleep_blocked);
            if let Some(on) = power_off {
                actions.push(RowAction::PowerOff(!on));
            }
            let mut row = fault_row(
                locale,
                entry,
                key,
                app.as_deref(),
                state,
                actions,
                &instance.changed_elsewhere,
                override_dir,
            );
            // A fix not offered says why where the cause would be, so the
            // reason is read without opening the card; the cause moves in.
            if let (Some(why), None) = (refused, &instance.record) {
                row.facts.push(row.cause.to_string());
                row.cause = not_offered_text(locale, why);
            }
            if let Some(version) = electron_of(instance) {
                row.facts
                    .push(c.electron_version.replace("{version}", version));
            }
            if usb_entry {
                row.facts.extend(usb_facts(locale, instance));
                row.takes_effect = None;
            }
            if let Some(on) = power_off {
                row.facts.push(
                    if on {
                        c.power_off_now_off
                    } else {
                        c.power_off_now_notify
                    }
                    .to_string(),
                );
                row.facts.push(c.power_off_safety.to_string());
            }
            if group_found {
                row.found = true;
                rows.found.push(row);
            } else {
                rows.applied.push(row);
            }
        }
        // A fix that keeps no record (its fault is simply gone), and a
        // Reset USB-C that cleared its fault, still say how it went until the
        // next Check again.
        for (key, result) in results.iter().filter(|(key, _)| key.entry == report.entry) {
            let shown = report.instances.iter().any(|i| i.subject == key.subject);
            if shown || !matches!(result, RowResult::Fixed(_) | RowResult::Reset(_)) {
                continue;
            }
            let app = match &key.subject {
                Subject::FlatpakApp(id) => Some(id.to_string()),
                Subject::Machine => None,
            };
            let mut row = fault_row(
                locale,
                entry,
                key.clone(),
                app.as_deref(),
                result_line(locale, result),
                Vec::new(),
                &[],
                override_dir,
            );
            // A reset changed nothing the fix would: its card says only what
            // came back.
            if matches!(result, RowResult::Reset(_)) {
                row.change.clear();
                row.undo_note = "";
            }
            rows.applied.push(row);
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
                limit_text(locale, Limit::WholeNumberScaleOnly),
                limit_text(locale, Limit::ScreenSharing),
            ],
            EntryId::UsbCInterface | EntryId::UsbController => entry
                .describe(&Subject::Machine)
                .limits
                .iter()
                .map(|limit| limit_text(locale, *limit))
                .collect(),
        };
        rows.all.push(KnownRow {
            entry: report.entry,
            header: known_header(locale, report.entry),
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
                // A fault whose fix is not offered here is listed on the page
                // with its reason; Home and the pane count only what the
                // person can act on.
                if instance.record.is_none() && not_offered(instance).is_some() {
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
