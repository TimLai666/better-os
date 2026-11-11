//! The `defaults` subcommands.
//!
//! These are the command-line half of Better Defaults. They build the same
//! [`DefaultsEngine`] the GUI will, review the same plan, and print exactly what
//! each entry did — including the entries that did nothing and why.

use std::error::Error;
use std::path::PathBuf;

use better_core::{ComponentCatalog, ComponentId, ComponentManifest, ObservedValue};
use clap::{Args, Subcommand};
use defaults_core::{
    AdapterMode, AdapterSession, AggregateState, ComponentReadiness, Confirmations, DefaultsEngine,
    DefaultsOutcome, DefaultsPlan, DefaultsReport, EntryOutcome, IntegrationState, PlanAction,
    PlanKind, PlanWarning, RestoreChoice, Selection, SkipReason, SystemContext,
};
use defaults_platform::{ApplicationChoice, DesktopId, MimeType};
use defaults_store::SnapshotStore;
use manager_core::{ExecutionMode, HealthState, ManagerState};

#[derive(Debug, Subcommand)]
pub enum DefaultsCommand {
    /// Show what each component's declared integrations currently say.
    Inspect,
    /// Show what applying Better OS defaults would change, without changing it.
    Plan {
        /// Plan putting back the captured previous values instead.
        #[arg(long)]
        restore: bool,
    },
    /// Apply Better OS defaults for the selected components.
    Apply,
    /// Read every integration again and record what was seen.
    Verify,
    /// Put back the values captured before Better OS changed them.
    Restore,
}

#[derive(Debug, Args)]
pub struct DefaultsOptions {
    /// Where defaults snapshots are kept.
    #[arg(long)]
    pub snapshot_dir: Option<PathBuf>,
    /// Read these manifests instead of the built-in catalog. Repeatable.
    #[arg(long = "manifest")]
    pub manifests: Vec<PathBuf>,
    /// Where mock execution keeps its simulated desktop between runs.
    #[arg(long)]
    pub mock_desktop: Option<PathBuf>,
    /// Limit the operation to these components. Repeatable.
    #[arg(long = "component")]
    pub components: Vec<String>,
    /// Overwrite an entry that changed outside Better Manager, as
    /// `component:integration`. Repeatable, and required per entry.
    #[arg(long = "confirm-external")]
    pub confirm_external: Vec<String>,
    /// What to restore instead of an application that is no longer installed.
    ///
    /// `component:integration=DESKTOP_ID` restores that installed application,
    /// and `component:integration=none` leaves the type without a default. For
    /// one type of a group restored type by type, name the type:
    /// `component:integration:TYPE=...`, for example
    /// `better-files:image-viewer:image/png=org.gnome.Loupe.desktop`.
    /// Repeatable, once per place.
    ///
    /// Only `plan --restore` and `restore` take it. It is refused when it is
    /// not in that form, when the type is not a MIME type or the target is not
    /// a desktop ID or `none`, when the same place is given twice, and, after
    /// planning, when it matches no application the plan reports as no longer
    /// installed or names one that is not installed or does not open the type.
    /// A refused restore changes nothing.
    #[arg(
        long = "restore-to",
        value_name = "COMPONENT:INTEGRATION[:TYPE]=DESKTOP_ID|none"
    )]
    pub restore_to: Vec<String>,
}

pub fn run(
    command: DefaultsCommand,
    options: DefaultsOptions,
    mode: ExecutionMode,
    distribution: &str,
    state: &ManagerState,
    fallback_catalog: ComponentCatalog,
) -> Result<(), Box<dyn Error>> {
    let catalog = match options.manifests.is_empty() {
        true => fallback_catalog,
        false => {
            let manifests = options
                .manifests
                .iter()
                .map(|path| {
                    let text = std::fs::read_to_string(path)?;
                    Ok::<_, Box<dyn Error>>(ComponentManifest::parse_yaml(&text)?)
                })
                .collect::<Result<Vec<_>, _>>()?;
            ComponentCatalog::from_manifests(manifests)?
        }
    };

    let mut engine = DefaultsEngine::new(
        &catalog,
        SystemContext::new(distribution, defaults_core::adapters::desktop_session()),
    );
    for manifest in catalog.manifests() {
        engine = engine.with_readiness(manifest.id.clone(), readiness(state, &manifest.id));
    }

    let selection = if options.components.is_empty() {
        Selection::All
    } else {
        Selection::Components(
            options
                .components
                .iter()
                .map(|value| ComponentId::new(value.clone()))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let confirmations = parse_confirmations(&options.confirm_external, &options.restore_to)?;
    let restoring = matches!(
        command,
        DefaultsCommand::Restore | DefaultsCommand::Plan { restore: true }
    );
    if !restoring && !options.restore_to.is_empty() {
        return Err(Box::new(std::io::Error::other(
            "--restore-to is only for `plan --restore` and `restore`",
        )));
    }
    let store = SnapshotStore::at_path(
        options
            .snapshot_dir
            .clone()
            .unwrap_or_else(|| SnapshotStore::from_default_path().directory().to_path_buf()),
    );
    let mut session = AdapterSession::open(&adapter_mode(mode, options.mock_desktop.clone()))?;
    if session.is_ephemeral() {
        println!(
            "mock execution: the simulated desktop is not kept between runs; \
             pass --mock-desktop PATH to keep it"
        );
    }
    match command {
        DefaultsCommand::Inspect => {
            print_report(&engine.inspect(&selection, session.adapters(), &store.history()?));
        }
        DefaultsCommand::Plan { restore } => {
            let history = store.history()?;
            let plan = if restore {
                engine.plan_restore(&selection, session.adapters(), &history, &confirmations)
            } else {
                engine.plan_apply(&selection, session.adapters(), &history, &confirmations)
            };
            print_plan(&plan);
            refuse_unmatched_restore_choices(&plan, &confirmations)?;
        }
        DefaultsCommand::Apply | DefaultsCommand::Restore => {
            let history = store.history()?;
            let plan = match command {
                DefaultsCommand::Apply => {
                    engine.plan_apply(&selection, session.adapters(), &history, &confirmations)
                }
                _ => engine.plan_restore(&selection, session.adapters(), &history, &confirmations),
            };
            print_plan(&plan);
            refuse_unmatched_restore_choices(&plan, &confirmations)?;
            let outcome = engine.execute(&plan, session.adapters_mut(), &store)?;
            print_outcome(&outcome);
        }
        DefaultsCommand::Verify => {
            print_report(&engine.verify(&selection, session.adapters(), &store)?);
        }
    }

    session.persist()?;
    Ok(())
}

/// What this run works against. Mock execution simulates a desktop and says so;
/// real execution gets the production adapters ADR 0009 decided.
fn adapter_mode(mode: ExecutionMode, mock_desktop: Option<PathBuf>) -> AdapterMode {
    match mode {
        ExecutionMode::Mock => AdapterMode::Simulated {
            desktop_path: mock_desktop,
        },
        ExecutionMode::Real => AdapterMode::Production,
    }
}

fn readiness(state: &ManagerState, component: &ComponentId) -> ComponentReadiness {
    match state.component(component) {
        None => ComponentReadiness::default(),
        Some(record) => ComponentReadiness {
            installed: record.installed_version.is_some(),
            enabled: record.enabled,
            healthy: record.health == HealthState::Healthy,
        },
    }
}

fn parse_confirmations(
    values: &[String],
    restore_to: &[String],
) -> Result<Confirmations, Box<dyn Error>> {
    let mut confirmations = Confirmations::none();
    for value in values {
        let (component, integration) = value.split_once(':').ok_or_else(|| {
            std::io::Error::other(format!("expected component:integration, found {value}"))
        })?;
        confirmations = confirmations.with(
            ComponentId::new(component)?,
            better_core::IntegrationId::new(integration)?,
        );
    }
    let mut places = Vec::new();
    for value in restore_to {
        let refuse = |why: &str| -> Box<dyn Error> {
            Box::new(std::io::Error::other(format!(
                "--restore-to {value}: {why}; expected \
                 component:integration[:type]=DESKTOP_ID or ...=none"
            )))
        };
        let (place, target) = value.split_once('=').ok_or_else(|| refuse("no `=`"))?;
        let mut parts = place.splitn(3, ':');
        let (Some(component), Some(integration)) = (parts.next(), parts.next()) else {
            return Err(refuse("no `component:integration`"));
        };
        let key = match parts.next() {
            None => None,
            Some(key) => Some(
                MimeType::parse(key)
                    .filter(|mime| mime.as_str() == key)
                    .ok_or_else(|| refuse("the type is not a MIME type"))?
                    .as_str()
                    .to_string(),
            ),
        };
        let choice = match target {
            "none" => RestoreChoice::ClearDefault,
            desktop_id => RestoreChoice::Application(
                DesktopId::new(desktop_id)
                    .map_err(|_| refuse("not a desktop ID or `none`"))?
                    .as_str()
                    .to_string(),
            ),
        };
        let component = ComponentId::new(component).map_err(|error| refuse(&error.to_string()))?;
        let integration = better_core::IntegrationId::new(integration)
            .map_err(|error| refuse(&error.to_string()))?;
        let place = (component.clone(), integration.clone(), key.clone());
        if places.contains(&place) {
            return Err(refuse("this place is chosen more than once"));
        }
        places.push(place);
        confirmations = confirmations.with_restore_choice(component, integration, key, choice);
    }
    Ok(confirmations)
}

/// Every `--restore-to` the plan did not take, named as `component:integration`
/// or `component:integration:type`. A choice is taken when the plan carries it
/// for an application that is no longer installed; one for an entry that is
/// not missing anything, for a component or type the plan does not have, or
/// that names an application that does not open the type, is not.
fn unmatched_restore_choices(plan: &DefaultsPlan, confirmations: &Confirmations) -> Vec<String> {
    confirmations
        .restore_choices()
        .filter(|(component, integration, key, _)| {
            !plan.entries.iter().any(|entry| {
                &entry.component == *component
                    && &entry.integration == *integration
                    && entry.warnings.iter().any(|warning| {
                        matches!(
                            warning,
                            PlanWarning::PreviousApplicationMissing { key: taken, chosen: Some(_), .. }
                                if taken.as_deref() == *key
                        )
                    })
            })
        })
        .map(|(component, integration, key, _)| match key {
            Some(key) => format!("{component}:{integration}:{key}"),
            None => format!("{component}:{integration}"),
        })
        .collect()
}

/// Stops the command, before anything is written, when a `--restore-to` was
/// not taken, naming each one.
fn refuse_unmatched_restore_choices(
    plan: &DefaultsPlan,
    confirmations: &Confirmations,
) -> Result<(), Box<dyn Error>> {
    let unmatched = unmatched_restore_choices(plan, confirmations);
    if unmatched.is_empty() {
        return Ok(());
    }
    for place in &unmatched {
        eprintln!(
            "--restore-to {place} was not used: nothing there is an application that is no \
             longer installed, or the choice is not installed or does not open the type"
        );
    }
    Err(Box::new(std::io::Error::other(format!(
        "{} --restore-to choice(s) not used; nothing was changed",
        unmatched.len()
    ))))
}

/// What the CLI says when a restore puts the person's choice in place of an
/// application that is no longer installed.
fn chosen_instead_line(key: Option<&str>, desktop_id: &str, choice: &RestoreChoice) -> String {
    let label = key.map(|key| format!("{key}: ")).unwrap_or_default();
    let instead = match choice {
        RestoreChoice::Application(chosen) => format!("restoring {chosen} instead"),
        RestoreChoice::ClearDefault => "clearing the default instead".to_string(),
    };
    format!("  {label}the previous application is no longer installed: {desktop_id}; {instead}")
}

/// What the CLI says about an application a restore cannot put back because
/// it is no longer installed: which one, that nothing is written for it, and
/// the two ways forward. `key` names the type of a group restored type by
/// type.
fn missing_application_lines(
    component: &ComponentId,
    integration: &better_core::IntegrationId,
    key: Option<&str>,
    desktop_id: &str,
    alternatives: &[ApplicationChoice],
) -> Vec<String> {
    let (label, place) = match key {
        Some(key) => (
            format!("{key}: "),
            format!("{component}:{integration}:{key}"),
        ),
        None => (String::new(), format!("{component}:{integration}")),
    };
    let mut lines = vec![format!(
        "  {label}the previous application is no longer installed: {desktop_id}; \
         nothing is written for it"
    )];
    if alternatives.is_empty() {
        lines.push("    no installed application declares this type".to_string());
    } else {
        lines.push(format!(
            "    choose another with --restore-to {place}=<desktop id>, one of:"
        ));
        for choice in alternatives {
            lines.push(format!("      {} ({})", choice.desktop_id, choice.name));
        }
    }
    lines.push(format!(
        "    or clear the default with --restore-to {place}=none"
    ));
    lines
}

fn print_report(report: &DefaultsReport) {
    for path in &report.damaged_snapshots {
        println!("unreadable snapshot: {path}");
    }
    if report.components.is_empty() {
        println!("no component in this catalog declares a default integration");
    }
    for component in &report.components {
        println!(
            "{} {}",
            component.component,
            aggregate_label(&component.aggregate)
        );
        for status in &component.integrations {
            let current = match status.current.per_key() {
                Some(_) => "per-type".to_string(),
                None => format!("{:?}", status.current),
            };
            println!(
                "  {} {:?} {} current={current} desired={:?} session_effect={:?} restore_available={}",
                status.integration,
                status.kind,
                state_label(&status.state),
                status.desired,
                status.session_effect,
                status.restore_available
            );
            print_per_type("    ", "current", &status.current);
        }
    }
}

fn aggregate_label(aggregate: &AggregateState) -> String {
    match aggregate {
        AggregateState::Default => "default".to_string(),
        AggregateState::NotDefault => "not-default".to_string(),
        AggregateState::PartiallyDefault => "partially-default".to_string(),
        AggregateState::ChangedExternally => "changed-externally".to_string(),
        AggregateState::Unavailable { reason } => format!("unavailable ({reason})"),
        AggregateState::Conflict { claimant } => format!("conflict (claimed by {claimant})"),
        AggregateState::Unknown { reason } => format!("unknown ({reason})"),
        AggregateState::NeedsSignOut => "needs-sign-out".to_string(),
    }
}

fn state_label(state: &IntegrationState) -> String {
    match state {
        IntegrationState::Default => "default".to_string(),
        IntegrationState::NotDefault => "not-default".to_string(),
        IntegrationState::ChangedExternally { last_known } => {
            format!("changed-externally (last written {last_known:?})")
        }
        IntegrationState::Unavailable { reason } => format!("unavailable ({reason})"),
        IntegrationState::Conflict { claimant } => format!("conflict (claimed by {claimant})"),
        IntegrationState::Unknown { reason } => format!("unknown ({reason})"),
        IntegrationState::NeedsSignOut => "needs-sign-out".to_string(),
    }
}

fn print_plan(plan: &DefaultsPlan) {
    for path in &plan.damaged_snapshots {
        println!("unreadable snapshot: {path}");
    }
    println!(
        "{:?} plan: {} of {} entries would change",
        plan.kind,
        plan.changes().count(),
        plan.entries.len()
    );
    for entry in &plan.entries {
        let action = match &entry.action {
            PlanAction::Apply { to } => format!("apply {to:?}"),
            PlanAction::Restore { to } if to.per_key().is_some() => {
                "restore per type, each to its captured value".to_string()
            }
            PlanAction::Restore { to } => format!("restore {to:?}"),
            PlanAction::Skip {
                reason: SkipReason::PreviousApplicationMissing { desktop_id, .. },
            } => format!("skip: the previous application is no longer installed ({desktop_id})"),
            PlanAction::Skip { reason } => format!("skip {reason:?}"),
        };
        println!("{}:{} {action}", entry.component, entry.integration);
        if let PlanAction::Skip {
            reason:
                SkipReason::PreviousApplicationMissing {
                    desktop_id,
                    alternatives,
                },
        } = &entry.action
        {
            for line in missing_application_lines(
                &entry.component,
                &entry.integration,
                None,
                desktop_id,
                alternatives,
            ) {
                println!("{line}");
            }
        }
        if !print_per_type("  ", "current", &entry.current) {
            println!("  current: {:?}", entry.current);
        }
        match &entry.captured_previous {
            Some(value) if print_per_type("  ", "captured previous", value) => {}
            Some(value) => println!("  captured previous: {value:?}"),
            None => println!("  captured previous: nothing captured yet"),
        }
        println!("  session effect: {:?}", entry.session_effect);
        if entry.requires_confirmation {
            println!(
                "  changed outside Better Manager: confirmed={} (to overwrite it, run \
                 `defaults --confirm-external {}:{} apply`)",
                entry.confirmed, entry.component, entry.integration
            );
        }
        for warning in &entry.warnings {
            match warning {
                PlanWarning::PreviousApplicationMissing {
                    key,
                    desktop_id,
                    alternatives,
                    chosen,
                } => {
                    let lines = match chosen {
                        Some(choice) => {
                            vec![chosen_instead_line(key.as_deref(), desktop_id, choice)]
                        }
                        None => missing_application_lines(
                            &entry.component,
                            &entry.integration,
                            key.as_deref(),
                            desktop_id,
                            alternatives,
                        ),
                    };
                    for line in lines {
                        println!("{line}");
                    }
                }
                other => println!("  warning: {other:?}"),
            }
        }
    }
    if plan.kind == PlanKind::Apply && plan.is_empty() {
        println!("nothing to apply");
    }
}

fn print_outcome(outcome: &DefaultsOutcome) {
    println!(
        "{:?} finished: {} of {} entries succeeded",
        outcome.kind,
        outcome.succeeded(),
        outcome.results.len()
    );
    if let Some(id) = &outcome.baseline_snapshot {
        println!("captured previous values into snapshot {id}");
    }
    if let Some(id) = &outcome.recorded_snapshot {
        println!("recorded results into snapshot {id}");
    }
    for result in &outcome.results {
        println!(
            "{}:{} {}",
            result.component,
            result.integration,
            outcome_line(&result.outcome)
        );
        if let EntryOutcome::Skipped {
            reason:
                SkipReason::PreviousApplicationMissing {
                    desktop_id,
                    alternatives,
                },
        } = &result.outcome
        {
            for line in missing_application_lines(
                &result.component,
                &result.integration,
                None,
                desktop_id,
                alternatives,
            ) {
                println!("{line}");
            }
        }
        if let EntryOutcome::PerKey { keys } = &result.outcome {
            for key in keys {
                let line = match &key.outcome {
                    EntryOutcome::Restored { value } => {
                        format!("restored {}", observed_text(value))
                    }
                    other => outcome_line(other),
                };
                println!("    {}: {line}", key.key);
                if let EntryOutcome::Skipped {
                    reason:
                        SkipReason::PreviousApplicationMissing {
                            desktop_id,
                            alternatives,
                        },
                } = &key.outcome
                {
                    for line in missing_application_lines(
                        &result.component,
                        &result.integration,
                        Some(&key.key),
                        desktop_id,
                        alternatives,
                    ) {
                        println!("  {line}");
                    }
                }
            }
        }
    }
}

/// Each key of a reading that differs per key, one per line under `label`.
/// Returns whether it printed anything, so a caller prints a single value
/// otherwise.
fn print_per_type(indent: &str, label: &str, observed: &ObservedValue) -> bool {
    let Some(per_key) = observed.per_key() else {
        return false;
    };
    println!("{indent}{label}, per type:");
    for key in per_key {
        println!("{indent}  {}: {}", key.key, observed_text(&key.observed));
    }
    true
}

fn observed_text(observed: &ObservedValue) -> String {
    match observed {
        ObservedValue::Set { value } => format!("{value:?}"),
        other => format!("{other:?}"),
    }
}

fn outcome_line(outcome: &EntryOutcome) -> String {
    match outcome {
        EntryOutcome::Applied { value } => format!("applied {value:?}"),
        EntryOutcome::AppliedNeedsSignOut { value } => {
            format!("applied {value:?}, effective after sign-out")
        }
        EntryOutcome::Restored { value } => format!("restored {value:?}"),
        EntryOutcome::AlreadyCorrect => "already correct".to_string(),
        EntryOutcome::NotVerified { observed } => {
            format!("NOT VERIFIED: the setting now reads {observed:?}")
        }
        EntryOutcome::VerificationInconclusive { observed } => {
            format!("could not verify: the setting reads {observed:?}")
        }
        EntryOutcome::Skipped {
            reason: SkipReason::PreviousApplicationMissing { desktop_id, .. },
        } => {
            format!("not restored: the previous application is no longer installed ({desktop_id})")
        }
        EntryOutcome::Skipped {
            reason: SkipReason::ChosenApplicationMissing { desktop_id, .. },
        } => format!(
            "not restored: the application chosen instead is no longer installed ({desktop_id})"
        ),
        EntryOutcome::Skipped { reason } => format!("skipped {reason:?}"),
        EntryOutcome::ManualActionRequired { reason, detail } => format!(
            "manual action required: {reason}{}",
            detail
                .as_ref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default()
        ),
        EntryOutcome::Failed { reason, detail } => format!(
            "failed: {reason}{}",
            detail
                .as_ref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default()
        ),
        EntryOutcome::PerKey { keys } => format!(
            "restored per type ({} of {} types put back)",
            keys.iter().filter(|key| key.outcome.is_success()).count(),
            keys.len()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use defaults_core::{RestoreChoice, SkipReason};
    use defaults_platform::ApplicationChoice;

    fn alternatives() -> Vec<ApplicationChoice> {
        vec![
            ApplicationChoice {
                desktop_id: "org.gnome.Loupe.desktop".to_string(),
                name: "Image Viewer".to_string(),
            },
            ApplicationChoice {
                desktop_id: "org.gnome.gThumb.desktop".to_string(),
                name: "gThumb".to_string(),
            },
        ]
    }

    fn component() -> ComponentId {
        ComponentId::new("better-files").unwrap()
    }

    fn integration() -> better_core::IntegrationId {
        better_core::IntegrationId::new("image-viewer").unwrap()
    }

    #[test]
    fn a_missing_application_is_named_and_the_alternatives_are_offered() {
        let lines = missing_application_lines(
            &component(),
            &integration(),
            None,
            "org.gnome.eog.desktop",
            &alternatives(),
        );
        let text = lines.join("\n");
        assert!(
            text.contains("the previous application is no longer installed: org.gnome.eog.desktop"),
            "{text}"
        );
        assert!(text.contains("nothing is written for it"), "{text}");
        assert!(
            text.contains("--restore-to better-files:image-viewer=<desktop id>"),
            "{text}"
        );
        assert!(
            text.contains("org.gnome.Loupe.desktop (Image Viewer)"),
            "{text}"
        );
        assert!(text.contains("org.gnome.gThumb.desktop (gThumb)"), "{text}");
        assert!(
            text.contains("--restore-to better-files:image-viewer=none"),
            "{text}"
        );
    }

    #[test]
    fn a_missing_type_in_a_group_is_offered_by_its_own_name() {
        let text = missing_application_lines(
            &component(),
            &integration(),
            Some("image/png"),
            "org.gnome.eog.desktop",
            &[],
        )
        .join("\n");
        assert!(
            text.contains("image/png: the previous application"),
            "{text}"
        );
        assert!(
            text.contains("--restore-to better-files:image-viewer:image/png=none"),
            "{text}"
        );
        assert!(
            text.contains("no installed application declares this type"),
            "{text}"
        );
    }

    #[test]
    fn a_choice_made_instead_is_said_in_words() {
        assert_eq!(
            chosen_instead_line(
                Some("image/png"),
                "org.gnome.eog.desktop",
                &RestoreChoice::Application("org.gnome.Loupe.desktop".to_string()),
            ),
            "  image/png: the previous application is no longer installed: \
             org.gnome.eog.desktop; restoring org.gnome.Loupe.desktop instead"
        );
        assert!(
            chosen_instead_line(None, "a.desktop", &RestoreChoice::ClearDefault)
                .ends_with("a.desktop; clearing the default instead")
        );
    }

    #[test]
    fn the_skip_reason_is_named_rather_than_printed_as_debug() {
        let line = outcome_line(&EntryOutcome::Skipped {
            reason: SkipReason::PreviousApplicationMissing {
                desktop_id: "org.gnome.eog.desktop".to_string(),
                alternatives: alternatives(),
            },
        });
        assert_eq!(
            line,
            "not restored: the previous application is no longer installed \
             (org.gnome.eog.desktop)"
        );
    }

    #[test]
    fn restore_choices_parse_for_an_entry_and_for_one_type() {
        let confirmations = parse_confirmations(
            &[],
            &[
                "better-files:image-viewer=org.gnome.Loupe.desktop".to_string(),
                "better-files:image-viewer:image/png=none".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(
            confirmations.restore_choice(&component(), &integration(), None),
            Some(&RestoreChoice::Application(
                "org.gnome.Loupe.desktop".to_string()
            ))
        );
        assert_eq!(
            confirmations.restore_choice(&component(), &integration(), Some("image/png")),
            Some(&RestoreChoice::ClearDefault)
        );
        for bad in [
            "better-files=none",
            "better-files:image-viewer",
            "better-files:image-viewer=",
            "better-files:image-viewer:=none",
            "better-files:image-viewer:not-a-type=none",
            "better-files:image-viewer=c=d",
            "better-files:image-viewer=not a desktop id",
            "better-files:image-viewer=org.gnome.Loupe",
            "Not A Component:image-viewer=none",
        ] {
            assert!(
                parse_confirmations(&[], &[bad.to_string()]).is_err(),
                "{bad} must be refused"
            );
        }
    }

    #[test]
    fn the_same_place_chosen_twice_is_refused() {
        let error = parse_confirmations(
            &[],
            &[
                "better-files:image-viewer=org.gnome.Loupe.desktop".to_string(),
                "better-files:image-viewer=none".to_string(),
            ],
        )
        .expect_err("two choices for one place are refused");
        assert!(error.to_string().contains("more than once"), "{error}");
        // The same entry as a whole and one of its types are different places.
        assert!(
            parse_confirmations(
                &[],
                &[
                    "better-files:image-viewer=none".to_string(),
                    "better-files:image-viewer:image/png=none".to_string(),
                ],
            )
            .is_ok()
        );
    }

    fn restore_plan(entries: Vec<defaults_core::PlanEntry>) -> DefaultsPlan {
        DefaultsPlan::new(PlanKind::Restore, entries, Vec::new())
    }

    fn plan_entry(action: PlanAction, warnings: Vec<PlanWarning>) -> defaults_core::PlanEntry {
        defaults_core::PlanEntry {
            component: component(),
            integration: integration(),
            kind: better_core::IntegrationKind::ApplicationHandler,
            adapter: better_core::AdapterId::XdgDefaultApp,
            action,
            current: ObservedValue::Unset,
            desired: better_core::DefaultsValue::DesktopEntry("io.betteros.Files.desktop".into()),
            captured_previous: None,
            session_effect: better_core::SessionEffect::Immediate,
            requires_confirmation: false,
            confirmed: false,
            warnings,
        }
    }

    #[test]
    fn a_choice_the_plan_did_not_take_is_listed() {
        let confirmations = parse_confirmations(
            &[],
            &[
                "better-files:image-viewer=org.gnome.Loupe.desktop".to_string(),
                "better-files:image-viewer:image/png=none".to_string(),
                "better-monitor:nothing=none".to_string(),
            ],
        )
        .unwrap();
        // The entry as a whole took its choice; the type and the unknown
        // component matched nothing that is missing.
        let plan = restore_plan(vec![plan_entry(
            PlanAction::Restore {
                to: ObservedValue::Set {
                    value: better_core::DefaultsValue::DesktopEntry(
                        "org.gnome.Loupe.desktop".into(),
                    ),
                },
            },
            vec![PlanWarning::PreviousApplicationMissing {
                key: None,
                desktop_id: "org.gnome.eog.desktop".into(),
                alternatives: alternatives(),
                chosen: Some(RestoreChoice::Application("org.gnome.Loupe.desktop".into())),
            }],
        )]);
        assert_eq!(
            unmatched_restore_choices(&plan, &confirmations),
            vec![
                "better-files:image-viewer:image/png".to_string(),
                "better-monitor:nothing".to_string(),
            ]
        );

        // A rejected choice leaves the entry skipped, and it is listed too.
        let rejected = restore_plan(vec![plan_entry(
            PlanAction::Skip {
                reason: SkipReason::PreviousApplicationMissing {
                    desktop_id: "org.gnome.eog.desktop".into(),
                    alternatives: alternatives(),
                },
            },
            Vec::new(),
        )]);
        let only_entry = parse_confirmations(
            &[],
            &["better-files:image-viewer=org.gnome.Loupe.desktop".to_string()],
        )
        .unwrap();
        assert_eq!(
            unmatched_restore_choices(&rejected, &only_entry),
            vec!["better-files:image-viewer".to_string()]
        );
    }
}
