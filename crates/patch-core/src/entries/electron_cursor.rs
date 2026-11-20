//! Entry 2: a half-size pointer over an Electron app.
//!
//! Seen on the owner's machine on 2026-11-11 (ticket 76): over an Electron
//! 32 window on a display scaled to 200%, the pointer was half size. On
//! native Wayland that Electron does not scale its pointer, and
//! `XCURSOR_SIZE` changes nothing. Running the app through XWayland fixed it
//! with the picture still sharp. For a Flatpak that is a per-user override:
//! `--nosocket=fallback-x11 --socket=x11` and
//! `ELECTRON_OZONE_PLATFORM_HINT=x11`. `--socket=x11` alone leaves `DISPLAY`
//! empty while `fallback-x11` is granted.
//!
//! Limits, which the page states: it was verified on a Flatpak only, so an
//! app installed any other way is offered nothing; only apps built on
//! Flathub's Electron base application are recognised; an app runs natively
//! on Wayland here only when it is granted the Wayland socket and its
//! Electron hint is `auto` or `wayland`, the configuration observed; and the
//! override should be undone once the app ships an Electron that scales its
//! pointer, which nothing here can tell.

use crate::command::Command;
use crate::entry::{
    Answer, Answers, CannotCheck, Change, ChangeLine, CheckOutcome, Description, Detection, Entry,
    EntryId, Finding, FindingDetail, Limit, Plan, PlanError, Read, ReadError, Subject, UndoKind,
    UndoPlan,
};
use crate::flatpak::{AppId, Effective};
use crate::keyfile::{self, KeyFile};
use crate::record::{AppliedRecord, ChangedElsewhere, KeyChange, KeyValue, OverrideKey, Recorded};

pub struct ElectronCursor;

/// Electron's platform hint.
pub const HINT: &str = "ELECTRON_OZONE_PLATFORM_HINT";

/// A scale is above 1 when it is at least this much above it; Mutter
/// reports fractional scales such as 1.25 exactly enough for this.
const SCALE_EPSILON: f64 = 0.001;

const CONTEXT: &str = "Context";
const ENVIRONMENT: &str = "Environment";
const SOCKETS: &str = "sockets";

/// The keys the fix sets, and what it sets them to.
pub fn fix_keys() -> Vec<(OverrideKey, KeyValue)> {
    vec![
        (OverrideKey::Socket("x11".into()), KeyValue::Granted),
        (OverrideKey::Socket("fallback-x11".into()), KeyValue::Denied),
        (
            OverrideKey::Environment(HINT.into()),
            KeyValue::Value("x11".into()),
        ),
    ]
}

/// Whether an app with these sockets and this environment runs on Wayland
/// itself rather than through XWayland.
pub fn runs_natively_on_wayland(effective: &Effective) -> bool {
    effective.grants("wayland")
        && effective
            .environment
            .get(HINT)
            .is_some_and(|hint| matches!(hint.trim(), "auto" | "wayland"))
}

fn group_of(key: &OverrideKey) -> &'static str {
    match key {
        OverrideKey::Socket(_) => CONTEXT,
        OverrideKey::Environment(_) => ENVIRONMENT,
    }
}

fn socket_name(token: &str) -> &str {
    token.strip_prefix('!').unwrap_or(token)
}

/// What a key holds in an override file.
pub fn current(file: &KeyFile, key: &OverrideKey) -> KeyValue {
    match key {
        OverrideKey::Socket(name) => file
            .get(CONTEXT, SOCKETS)
            .map(keyfile::list)
            .unwrap_or_default()
            .iter()
            .rev()
            .find(|token| socket_name(token) == name)
            .map_or(KeyValue::Absent, |token| {
                if token.starts_with('!') {
                    KeyValue::Denied
                } else {
                    KeyValue::Granted
                }
            }),
        OverrideKey::Environment(name) => file
            .get(ENVIRONMENT, name)
            .map_or(KeyValue::Absent, |value| KeyValue::Value(value.to_string())),
    }
}

/// Makes a key hold a value, touching nothing else: a socket's token is
/// replaced where it stands, so putting back the previous value puts it
/// back in its place.
pub fn put(file: &mut KeyFile, key: &OverrideKey, value: &KeyValue) {
    match key {
        OverrideKey::Socket(name) => {
            let mut tokens = file
                .get(CONTEXT, SOCKETS)
                .map(keyfile::list)
                .unwrap_or_default();
            let token = match value {
                KeyValue::Granted => Some(name.clone()),
                KeyValue::Denied => Some(format!("!{name}")),
                KeyValue::Absent | KeyValue::Value(_) => None,
            };
            let position = tokens.iter().rposition(|t| socket_name(t) == name);
            tokens.retain(|t| socket_name(t) != name);
            if let Some(token) = token {
                // `retain` removed the token and its repeats; put the new one
                // where the last of them stood.
                let at = position
                    .map(|p| p.min(tokens.len()))
                    .unwrap_or(tokens.len());
                tokens.insert(at, token);
            }
            if tokens.is_empty() {
                file.remove(CONTEXT, SOCKETS);
            } else {
                file.set(CONTEXT, SOCKETS, &keyfile::join_list(&tokens));
            }
        }
        OverrideKey::Environment(name) => match value {
            KeyValue::Value(text) => file.set(ENVIRONMENT, name, text),
            _ => file.remove(ENVIRONMENT, name),
        },
    }
}

fn app_of(finding: &Finding) -> Result<&AppId, PlanError> {
    match (&finding.subject, &finding.detail) {
        (Subject::FlatpakApp(app), FindingDetail::ElectronCursor { .. }) => Ok(app),
        _ => Err(PlanError::NotThisEntry),
    }
}

fn override_text(answers: &Answers, app: &AppId) -> Result<Option<String>, PlanError> {
    match answers.get(&Read::UserOverride(app.clone())) {
        Some(Answer::File(Ok(text))) => Ok(text.clone()),
        Some(Answer::File(Err(error))) => Err(PlanError::CannotRead(CannotCheck::FileUnreadable(
            error.to_string(),
        ))),
        _ => Err(PlanError::CannotRead(crate::engine::not_answered())),
    }
}

impl Entry for ElectronCursor {
    fn id(&self) -> EntryId {
        EntryId::ElectronCursor
    }

    fn undo_kind(&self) -> UndoKind {
        UndoKind::Reversible
    }

    fn detection_reads(&self) -> Vec<Read> {
        vec![Read::SessionType, Read::FlatpakApps, Read::DisplayScales]
    }

    fn detect(&self, answers: &Answers) -> Detection {
        match answers.get(&Read::SessionType) {
            Some(Answer::SessionType(Some(session))) if session.trim() == "wayland" => {}
            Some(Answer::SessionType(Some(_))) => return Detection::Checked(Vec::new()),
            Some(Answer::SessionType(None)) => {
                return Detection::CannotCheck(CannotCheck::SessionTypeUnknown);
            }
            _ => return Detection::CannotCheck(crate::engine::not_answered()),
        }
        let apps = match answers.get(&Read::FlatpakApps) {
            Some(Answer::Apps(Ok(apps))) => apps,
            Some(Answer::Apps(Err(error))) => {
                return Detection::CannotCheck(CannotCheck::FlatpakAppsUnreadable(
                    error.to_string(),
                ));
            }
            _ => return Detection::CannotCheck(crate::engine::not_answered()),
        };
        let mut candidates = Vec::new();
        for app in apps {
            if app.is_electron()
                && runs_natively_on_wayland(&app.effective())
                && !candidates
                    .iter()
                    .any(|seen: &&crate::flatpak::InstalledApp| seen.id == app.id)
            {
                candidates.push(app);
            }
        }
        if candidates.is_empty() {
            return Detection::Checked(Vec::new());
        }
        let scale = match answers.get(&Read::DisplayScales) {
            Some(Answer::Scales(Ok(scales))) => scales.iter().copied().fold(0.0_f64, f64::max),
            Some(Answer::Scales(Err(error))) => {
                return Detection::CannotCheck(CannotCheck::DisplayScaleUnreadable(
                    error.to_string(),
                ));
            }
            _ => return Detection::CannotCheck(crate::engine::not_answered()),
        };
        if scale <= 1.0 + SCALE_EPSILON {
            return Detection::Checked(Vec::new());
        }
        Detection::Checked(
            candidates
                .into_iter()
                .map(|app| Finding {
                    subject: Subject::FlatpakApp(app.id.clone()),
                    detail: FindingDetail::ElectronCursor {
                        name: app.name(),
                        scale,
                    },
                })
                .collect(),
        )
    }

    fn describe(&self, subject: &Subject) -> Description {
        let change = match subject {
            Subject::FlatpakApp(app) => vec![ChangeLine::OverrideKeys {
                app: app.clone(),
                keys: fix_keys(),
            }],
            Subject::Machine => Vec::new(),
        };
        Description {
            change,
            undo: UndoKind::Reversible,
            limits: vec![
                Limit::FlatpakOnly,
                Limit::ElectronBaseAppOnly,
                Limit::UntilTheAppScalesItsPointer,
            ],
        }
    }

    fn plan_reads(&self, finding: &Finding) -> Vec<Read> {
        match app_of(finding) {
            Ok(app) => vec![Read::UserOverride(app.clone())],
            Err(_) => Vec::new(),
        }
    }

    fn plan(&self, finding: &Finding, answers: &Answers) -> Result<Plan, PlanError> {
        let app = app_of(finding)?;
        let before = override_text(answers, app)?;
        let mut file = KeyFile::parse(before.as_deref().unwrap_or(""));
        let mut created_groups: Vec<String> = Vec::new();
        let mut keys = Vec::new();
        for (key, want) in fix_keys() {
            let previous = current(&file, &key);
            if previous == want {
                continue;
            }
            let group = group_of(&key);
            if !file.has_group(group) && !created_groups.iter().any(|g| g == group) {
                created_groups.push(group.to_string());
            }
            put(&mut file, &key, &want);
            keys.push(KeyChange {
                key,
                previous,
                written: want,
            });
        }
        if keys.is_empty() {
            return Err(PlanError::NothingToChange);
        }
        Ok(Plan {
            changes: vec![Change::WriteUserOverride {
                app: app.clone(),
                expected: before.clone(),
                contents: Some(file.to_text()),
            }],
            record: Recorded::OverrideKeys {
                app: app.clone(),
                name: match &finding.detail {
                    FindingDetail::ElectronCursor { name, .. } => name.clone(),
                    FindingDetail::FontCache { .. } => Default::default(),
                },
                created_file: before.is_none(),
                created_groups,
                keys,
            },
        })
    }

    fn check_reads(&self, record: &AppliedRecord) -> Vec<Read> {
        match &record.change {
            Recorded::OverrideKeys { app, .. } => {
                vec![Read::Command(Command::ShowFlatpakOverride(app.clone()))]
            }
            Recorded::RebuiltFontCache => Vec::new(),
        }
    }

    /// The override as Flatpak itself reads it holds every key the fix
    /// wrote.
    fn check(&self, record: &AppliedRecord, answers: &Answers) -> CheckOutcome {
        let Recorded::OverrideKeys { app, keys, .. } = &record.change else {
            return CheckOutcome::CannotCheck(crate::engine::not_answered());
        };
        let shown = match answers.get(&Read::Command(Command::ShowFlatpakOverride(app.clone()))) {
            Some(Answer::Output(Ok(text))) => KeyFile::parse(text),
            Some(Answer::Output(Err(ReadError::NotInstalled(program)))) => {
                return CheckOutcome::CannotCheck(CannotCheck::ProgramMissing(program));
            }
            Some(Answer::Output(Err(error))) => {
                return CheckOutcome::CannotCheck(CannotCheck::FileUnreadable(error.to_string()));
            }
            _ => return CheckOutcome::CannotCheck(crate::engine::not_answered()),
        };
        if keys
            .iter()
            .all(|change| current(&shown, &change.key) == change.written)
        {
            CheckOutcome::Passed
        } else {
            CheckOutcome::StillPresent
        }
    }

    fn undo_reads(&self, record: &AppliedRecord) -> Vec<Read> {
        match &record.change {
            Recorded::OverrideKeys { app, .. } => vec![Read::UserOverride(app.clone())],
            Recorded::RebuiltFontCache => Vec::new(),
        }
    }

    fn plan_undo(
        &self,
        record: &AppliedRecord,
        answers: &Answers,
        confirm_changed: bool,
    ) -> Result<UndoPlan, PlanError> {
        let Recorded::OverrideKeys {
            app,
            name,
            created_file,
            created_groups,
            keys,
        } = &record.change
        else {
            return Err(PlanError::NotThisEntry);
        };
        let before = override_text(answers, app)?;
        let mut file = KeyFile::parse(before.as_deref().unwrap_or(""));
        let mut kept = Vec::new();
        let mut skipped = Vec::new();
        for change in keys {
            let now = current(&file, &change.key);
            if now == change.previous {
                // Already as it was before the fix.
                continue;
            }
            if now != change.written && !confirm_changed {
                skipped.push(ChangedElsewhere {
                    key: change.key.clone(),
                    now,
                });
                kept.push(change.clone());
                continue;
            }
            put(&mut file, &change.key, &change.previous);
        }
        for group in created_groups {
            file.remove_group_if_empty(group);
        }
        let contents = if *created_file && file.is_blank() {
            None
        } else {
            Some(file.to_text())
        };
        let unchanged = match (&before, &contents) {
            (None, None) => true,
            (Some(before), Some(after)) => before == after,
            _ => false,
        };
        let changes = if unchanged {
            Vec::new()
        } else {
            vec![Change::WriteUserOverride {
                app: app.clone(),
                expected: before,
                contents,
            }]
        };
        Ok(UndoPlan {
            changes,
            remaining: (!kept.is_empty()).then(|| Recorded::OverrideKeys {
                app: app.clone(),
                name: name.clone(),
                created_file: *created_file,
                created_groups: created_groups.clone(),
                keys: kept,
            }),
            skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{self, fake::FakeHost};
    use crate::flatpak::{Installation, InstalledApp, OverrideLayers};
    use crate::record::Records;
    use crate::state::EntryState;

    const ELECTRON: &str = "\
[Application]
name=io.github.shiftey.Desktop
base=app/org.electronjs.Electron2.BaseApp/x86_64/25.08

[Context]
sockets=x11;wayland;fallback-x11;ssh-auth;

[Environment]
ELECTRON_OZONE_PLATFORM_HINT=auto
";

    const GNOME_APP: &str = "\
[Application]
name=net.nokyan.Resources
runtime=org.gnome.Platform/x86_64/50

[Context]
sockets=x11;wayland;fallback-x11;
";

    fn id(name: &str) -> AppId {
        AppId::parse(name).unwrap()
    }

    fn installed(name: &str, metadata: &str, user_override: Option<&str>) -> InstalledApp {
        InstalledApp {
            id: id(name),
            installation: Installation::System,
            metadata: metadata.into(),
            desktop_entry: Some("[Desktop Entry]\nName=GitHub Desktop\n".into()),
            overrides: OverrideLayers {
                user_app: user_override.map(str::to_string),
                ..OverrideLayers::default()
            },
        }
    }

    fn machine(session: Option<&str>, scales: Vec<f64>, apps: Vec<InstalledApp>) -> Answers {
        let mut answers = Answers::new();
        answers.insert(
            Read::SessionType,
            Answer::SessionType(session.map(str::to_string)),
        );
        answers.insert(Read::DisplayScales, Answer::Scales(Ok(scales)));
        answers.insert(Read::FlatpakApps, Answer::Apps(Ok(apps)));
        answers
    }

    fn found(answers: &Answers) -> Vec<Subject> {
        match ElectronCursor.detect(answers) {
            Detection::Checked(findings) => findings.into_iter().map(|f| f.subject).collect(),
            Detection::CannotCheck(reason) => panic!("cannot check: {reason:?}"),
        }
    }

    const DESKTOP: &str = "io.github.shiftey.Desktop";

    #[test]
    fn an_electron_flatpak_on_wayland_at_scale_two_is_the_fault() {
        let answers = machine(
            Some("wayland"),
            vec![1.0, 2.0],
            vec![
                installed(DESKTOP, ELECTRON, None),
                installed("net.nokyan.Resources", GNOME_APP, None),
            ],
        );
        assert_eq!(found(&answers), [Subject::FlatpakApp(id(DESKTOP))]);
        let Detection::Checked(findings) = ElectronCursor.detect(&answers) else {
            unreachable!()
        };
        let FindingDetail::ElectronCursor { name, scale } = &findings[0].detail else {
            panic!("electron finding");
        };
        assert_eq!(name.default.as_deref(), Some("GitHub Desktop"));
        assert_eq!(*scale, 2.0);
    }

    #[test]
    fn a_fractional_scale_above_one_counts() {
        let answers = machine(
            Some("wayland"),
            vec![1.25],
            vec![installed(DESKTOP, ELECTRON, None)],
        );
        assert_eq!(found(&answers).len(), 1);
    }

    #[test]
    fn the_fault_is_absent_where_any_condition_fails() {
        let electron = || vec![installed(DESKTOP, ELECTRON, None)];
        // X11 session.
        assert!(found(&machine(Some("x11"), vec![2.0], electron())).is_empty());
        // Scale 1.
        assert!(found(&machine(Some("wayland"), vec![1.0], electron())).is_empty());
        // Not Electron.
        assert!(
            found(&machine(
                Some("wayland"),
                vec![2.0],
                vec![installed("net.nokyan.Resources", GNOME_APP, None)]
            ))
            .is_empty()
        );
        // No Wayland socket.
        let no_wayland = ELECTRON.replace("x11;wayland;fallback-x11;", "x11;");
        assert!(
            found(&machine(
                Some("wayland"),
                vec![2.0],
                vec![installed(DESKTOP, &no_wayland, None)]
            ))
            .is_empty()
        );
        // No hint: older Electron runs through XWayland on its own.
        let no_hint = ELECTRON.replace("ELECTRON_OZONE_PLATFORM_HINT=auto\n", "");
        assert!(
            found(&machine(
                Some("wayland"),
                vec![2.0],
                vec![installed(DESKTOP, &no_hint, None)]
            ))
            .is_empty()
        );
        // The fix's override is already there.
        let fixed = "[Context]\nsockets=x11;!fallback-x11;\n\n[Environment]\nELECTRON_OZONE_PLATFORM_HINT=x11\n";
        assert!(
            found(&machine(
                Some("wayland"),
                vec![2.0],
                vec![installed(DESKTOP, ELECTRON, Some(fixed))]
            ))
            .is_empty()
        );
        // No Flatpak apps at all.
        assert!(found(&machine(Some("wayland"), vec![2.0], Vec::new())).is_empty());
    }

    #[test]
    fn what_cannot_be_read_is_cannot_check() {
        let mut answers = machine(None, vec![2.0], vec![installed(DESKTOP, ELECTRON, None)]);
        assert_eq!(
            ElectronCursor.detect(&answers),
            Detection::CannotCheck(CannotCheck::SessionTypeUnknown)
        );
        answers.insert(
            Read::SessionType,
            Answer::SessionType(Some("wayland".into())),
        );
        answers.insert(
            Read::DisplayScales,
            Answer::Scales(Err(ReadError::Failed("no Mutter".into()))),
        );
        assert_eq!(
            ElectronCursor.detect(&answers),
            Detection::CannotCheck(CannotCheck::DisplayScaleUnreadable("no Mutter".into()))
        );
        answers.insert(
            Read::FlatpakApps,
            Answer::Apps(Err(ReadError::Failed("denied".into()))),
        );
        assert_eq!(
            ElectronCursor.detect(&answers),
            Detection::CannotCheck(CannotCheck::FlatpakAppsUnreadable("denied".into()))
        );
    }

    #[test]
    fn an_unreadable_scale_does_not_matter_without_a_candidate() {
        let mut answers = machine(Some("wayland"), vec![], Vec::new());
        answers.insert(
            Read::DisplayScales,
            Answer::Scales(Err(ReadError::Failed("no Mutter".into()))),
        );
        assert_eq!(
            ElectronCursor.detect(&answers),
            Detection::Checked(Vec::new())
        );
    }

    /// A host whose override file and Flatpak's reading of it follow each
    /// write, as a real machine's would.
    fn host_with(file: Option<&str>) -> FakeHost {
        let app = id(DESKTOP);
        let mut answers = machine(
            Some("wayland"),
            vec![2.0],
            vec![installed(DESKTOP, ELECTRON, file)],
        );
        answers.insert(
            Read::UserOverride(app.clone()),
            Answer::File(Ok(file.map(str::to_string))),
        );
        answers.insert(
            Read::Command(Command::ShowFlatpakOverride(app.clone())),
            Answer::Output(Ok(file.unwrap_or("").to_string())),
        );
        FakeHost {
            answers,
            after_change: Some(Box::new(move |change, answers| {
                let Change::WriteUserOverride { app, contents, .. } = change else {
                    panic!("only override writes");
                };
                answers.insert(
                    Read::UserOverride(app.clone()),
                    Answer::File(Ok(contents.clone())),
                );
                answers.insert(
                    Read::Command(Command::ShowFlatpakOverride(app.clone())),
                    Answer::Output(Ok(contents.clone().unwrap_or_default())),
                );
                answers.insert(
                    Read::FlatpakApps,
                    Answer::Apps(Ok(vec![installed(DESKTOP, ELECTRON, contents.as_deref())])),
                );
            })),
            ..FakeHost::default()
        }
    }

    fn written(host: &FakeHost) -> Option<String> {
        match host.answers.get(&Read::UserOverride(id(DESKTOP))) {
            Some(Answer::File(Ok(text))) => text.clone(),
            other => panic!("{other:?}"),
        }
    }

    fn fix_it(host: &mut FakeHost) -> engine::FixOutcome {
        let report = engine::check(&ElectronCursor, host, &Records::default(), &[]);
        let finding = report.instances[0].finding.clone().expect("found");
        engine::fix(&ElectronCursor, &finding, host, 7).unwrap()
    }

    const FIXED_FILE: &str = "[Context]\nsockets=x11;!fallback-x11;\n\n[Environment]\nELECTRON_OZONE_PLATFORM_HINT=x11\n";

    #[test]
    fn the_fix_writes_the_override_ticket_76_verified_and_checks_it() {
        let mut host = host_with(None);
        let outcome = fix_it(&mut host);
        assert_eq!(written(&host).as_deref(), Some(FIXED_FILE));
        assert_eq!(outcome.check, CheckOutcome::Passed);
        let Recorded::OverrideKeys {
            created_file,
            keys,
            created_groups,
            ..
        } = &outcome.record.change
        else {
            panic!("override record");
        };
        assert!(created_file);
        assert_eq!(created_groups, &["Context", "Environment"]);
        assert_eq!(keys.len(), 3);
        assert!(keys.iter().all(|k| k.previous == KeyValue::Absent));
        // And the fault is gone as detection sees it.
        let mut records = Records::default();
        records.keep(&ElectronCursor, outcome.record);
        let report = engine::check(&ElectronCursor, &mut host, &records, &[]);
        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].state, EntryState::FixedAndChecked);
        assert!(report.instances[0].changed_elsewhere.is_empty());
    }

    const HAND_MADE: &str = "\
# added by hand
[Context]
filesystems=~/Projects;
sockets=wayland;

[Environment]
MY_THEME=dark
";

    #[test]
    fn the_fix_keeps_what_the_person_wrote_and_the_undo_removes_exactly_its_keys() {
        let mut host = host_with(Some(HAND_MADE));
        let outcome = fix_it(&mut host);
        assert_eq!(
            written(&host).as_deref(),
            Some(
                "\
# added by hand
[Context]
filesystems=~/Projects;
sockets=wayland;x11;!fallback-x11;

[Environment]
MY_THEME=dark
ELECTRON_OZONE_PLATFORM_HINT=x11
"
            )
        );
        let undone = engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
        assert_eq!(undone.remaining, None);
        assert!(undone.skipped.is_empty());
        assert_eq!(written(&host).as_deref(), Some(HAND_MADE));
    }

    #[test]
    fn an_undo_of_a_file_the_fix_made_removes_the_file() {
        let mut host = host_with(None);
        let outcome = fix_it(&mut host);
        engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
        assert_eq!(written(&host), None);
        assert_eq!(
            host.changes.last(),
            Some(&Change::WriteUserOverride {
                app: id(DESKTOP),
                expected: Some(FIXED_FILE.into()),
                contents: None,
            })
        );
    }

    #[test]
    fn a_file_the_fix_made_keeps_a_key_added_later_by_hand() {
        let mut host = host_with(None);
        let outcome = fix_it(&mut host);
        let added = format!("{FIXED_FILE}MY_VAR=1\n");
        host.answers.insert(
            Read::UserOverride(id(DESKTOP)),
            Answer::File(Ok(Some(added))),
        );
        engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
        assert_eq!(written(&host).as_deref(), Some("[Environment]\nMY_VAR=1\n"));
    }

    #[test]
    fn a_value_the_person_changed_after_the_fix_is_left_until_they_confirm() {
        let mut host = host_with(None);
        let outcome = fix_it(&mut host);
        let changed = FIXED_FILE.replace("HINT=x11", "HINT=wayland");
        host.answers.insert(
            Read::UserOverride(id(DESKTOP)),
            Answer::File(Ok(Some(changed.clone()))),
        );

        // The page shows it as changed by another program.
        let mut records = Records::default();
        records.keep(&ElectronCursor, outcome.record.clone());
        let report = engine::check(&ElectronCursor, &mut host, &records, &[]);
        let hint = OverrideKey::Environment(HINT.into());
        assert_eq!(
            report.instances[0].changed_elsewhere,
            [ChangedElsewhere {
                key: hint.clone(),
                now: KeyValue::Value("wayland".into()),
            }]
        );

        // Undo takes back the sockets and leaves the hint, still recorded.
        let undone = engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
        assert_eq!(
            written(&host).as_deref(),
            Some("[Environment]\nELECTRON_OZONE_PLATFORM_HINT=wayland\n")
        );
        assert_eq!(undone.skipped.len(), 1);
        let Some(Recorded::OverrideKeys { keys, .. }) = &undone.remaining else {
            panic!("the hint stays recorded");
        };
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key, hint);

        // Confirmed, it goes too, and the file the fix made goes with it.
        let mut left = outcome.record.clone();
        left.change = undone.remaining.unwrap();
        let undone = engine::undo(&ElectronCursor, &left, &mut host, true).unwrap();
        assert_eq!(undone.remaining, None);
        assert_eq!(written(&host), None);
    }

    #[test]
    fn a_previous_value_is_put_back_where_it_stood() {
        let before = "[Context]\nsockets=!x11;wayland;fallback-x11;\n\n[Environment]\nELECTRON_OZONE_PLATFORM_HINT=wayland\n";
        let mut host = host_with(Some(before));
        let outcome = fix_it(&mut host);
        assert_eq!(
            written(&host).as_deref(),
            Some(
                "[Context]\nsockets=x11;wayland;!fallback-x11;\n\n[Environment]\nELECTRON_OZONE_PLATFORM_HINT=x11\n"
            )
        );
        engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
        assert_eq!(written(&host).as_deref(), Some(before));
    }

    #[test]
    fn an_undo_after_the_person_removed_the_file_changes_nothing() {
        let mut host = host_with(None);
        let outcome = fix_it(&mut host);
        host.answers
            .insert(Read::UserOverride(id(DESKTOP)), Answer::File(Ok(None)));
        let writes = host.changes.len();
        let undone = engine::undo(&ElectronCursor, &outcome.record, &mut host, false).unwrap();
        assert_eq!(undone.remaining, None);
        assert_eq!(host.changes.len(), writes);
    }

    #[test]
    fn nothing_to_change_is_refused_and_a_check_that_differs_is_still_present() {
        let finding = Finding {
            subject: Subject::FlatpakApp(id(DESKTOP)),
            detail: FindingDetail::ElectronCursor {
                name: Default::default(),
                scale: 2.0,
            },
        };
        let mut answers = Answers::new();
        answers.insert(
            Read::UserOverride(id(DESKTOP)),
            Answer::File(Ok(Some(FIXED_FILE.into()))),
        );
        assert_eq!(
            ElectronCursor.plan(&finding, &answers),
            Err(PlanError::NothingToChange)
        );

        let mut host = host_with(None);
        let outcome = fix_it(&mut host);
        let mut answers = Answers::new();
        answers.insert(
            Read::Command(Command::ShowFlatpakOverride(id(DESKTOP))),
            Answer::Output(Ok("[Context]\nsockets=x11;\n".into())),
        );
        assert_eq!(
            ElectronCursor.check(&outcome.record, &answers),
            CheckOutcome::StillPresent
        );
        answers.insert(
            Read::Command(Command::ShowFlatpakOverride(id(DESKTOP))),
            Answer::Output(Err(ReadError::NotInstalled(crate::command::FLATPAK))),
        );
        assert_eq!(
            ElectronCursor.check(&outcome.record, &answers),
            CheckOutcome::CannotCheck(CannotCheck::ProgramMissing("/usr/bin/flatpak"))
        );
    }

    #[test]
    fn it_says_what_it_changes_and_its_limits() {
        let description = ElectronCursor.describe(&Subject::FlatpakApp(id(DESKTOP)));
        assert_eq!(description.undo, UndoKind::Reversible);
        assert!(description.limits.contains(&Limit::FlatpakOnly));
        assert_eq!(
            description.change,
            [ChangeLine::OverrideKeys {
                app: id(DESKTOP),
                keys: fix_keys()
            }]
        );
    }
}
