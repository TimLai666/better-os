//! Giving back what a component took over in the person's session, right
//! before its package goes or while it is turned off, and taking it again
//! when it is turned on (AGENTS.md: nothing a component set outlives it, and
//! a component that is off stays off). Better Manager runs a package's
//! `/usr/lib/better-os/before-removal/<package>` and
//! `/usr/lib/better-os/on-disable/<package>` as the person, the second with
//! `--undo` when the component is turned on again. Better Files' is
//! `better-files-before-removal` (ticket 129, ADR 0041).
//!
//! Putting back is the Restore the Default apps page runs, for one
//! component: a setting someone changed since is left alone, a setting that
//! was never changed has no record and is not touched. A file Better OS
//! wrote and has no record of is taken away only when it is byte for byte
//! Better OS's. What was put back is remembered, so `--undo` (a removal the
//! daemon refused, or the component turned on again) applies exactly that
//! again.

use std::path::{Path, PathBuf};

use better_core::defaults::IntegrationId;
use better_core::{ComponentCatalog, ComponentManifest};
use defaults_platform::AdapterSet;
use defaults_store::SnapshotStore;

use crate::engine::DefaultsEngine;
use crate::plan::{Confirmations, DefaultsOutcome, EntryOutcome, Selection};
use crate::status::{ComponentReadiness, SystemContext};

/// Better Files' own manifest, as the release that built this program
/// shipped it.
pub const BETTER_FILES_MANIFEST: &str =
    include_str!("../../../components/manifests/better-files.yaml");

/// What a put-back came to, as the program's exit status says it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PutBack {
    /// Exit 0: put back, or nothing to put back.
    Done,
    /// Exit 3: something could not be put back.
    NotRestored(String),
    /// Exit 4: no session to put it back in; Better Manager finishes it at
    /// its next start in the person's session.
    Deferred,
}

/// Which half a session-less run left for Better Manager's next start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pending {
    /// Give the defaults back: the component was removed or turned off.
    PutBack,
    /// Take them again: the component was turned on.
    TakeAgain,
}

/// Where the put-back remembers what it gave back, for `--undo`, and where a
/// put-back with no session leaves its work for Better Manager.
#[derive(Clone, Debug)]
pub struct RemovalState {
    directory: PathBuf,
    component: String,
}

impl RemovalState {
    /// `$XDG_STATE_HOME/better-os/defaults-put-back` (the person's own),
    /// for `component`.
    pub fn for_user(component: &str) -> Self {
        Self::at(Self::user_directory(), component)
    }

    pub fn at(directory: impl Into<PathBuf>, component: &str) -> Self {
        Self {
            directory: directory.into(),
            component: component.to_string(),
        }
    }

    fn given_back(&self) -> PathBuf {
        self.directory
            .join(format!("{}.given-back", self.component))
    }

    /// The marker a session-less run leaves.
    fn pending_path(&self) -> PathBuf {
        self.directory.join(format!("{}.pending", self.component))
    }

    /// What a session-less run left to finish, if anything.
    pub fn pending(&self) -> Option<Pending> {
        match std::fs::read_to_string(self.pending_path()).ok()?.trim() {
            "take-again" => Some(Pending::TakeAgain),
            // An empty marker is the first release's put-back.
            _ => Some(Pending::PutBack),
        }
    }

    /// The components this person has a marker or a memory for.
    pub fn components_in(directory: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut found: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".pending")
                    .or_else(|| name.strip_suffix(".given-back"))
                    .map(str::to_string)
            })
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// `$XDG_STATE_HOME/better-os/defaults-put-back`.
    pub fn user_directory() -> PathBuf {
        let state = match std::env::var("XDG_STATE_HOME") {
            Ok(value) if Path::new(&value).is_absolute() => PathBuf::from(value),
            _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"),
        };
        state.join("better-os/defaults-put-back")
    }

    fn remember(&self, ids: &[IntegrationId]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let text: String = ids.iter().map(|id| format!("{id}\n")).collect();
        std::fs::write(self.given_back(), text)
    }

    fn remembered(&self) -> Vec<String> {
        std::fs::read_to_string(self.given_back())
            .map(|text| text.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn mark_pending(&self, kind: Pending) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let word = match kind {
            Pending::PutBack => "put-back\n",
            Pending::TakeAgain => "take-again\n",
        };
        std::fs::write(self.pending_path(), word)
    }

    pub fn is_pending(&self) -> bool {
        self.pending().is_some()
    }

    fn clear_pending(&self) {
        let _ = std::fs::remove_file(self.pending_path());
    }
}

/// Restores every default `manifest` declares that Better Manager changed.
pub fn restore_component(
    manifest: ComponentManifest,
    system: SystemContext,
    adapters: &mut AdapterSet,
    store: &SnapshotStore,
) -> Result<DefaultsOutcome, String> {
    let component = manifest.id.clone();
    let catalog = ComponentCatalog::from_manifests(vec![manifest]).map_err(|e| e.to_string())?;
    let engine = DefaultsEngine::new(&catalog, system)
        // Still installed: its package is removed only after this returns.
        .with_readiness(component.clone(), ComponentReadiness::ready());
    let history = store.history().map_err(|e| e.to_string())?;
    let plan = engine.plan_restore(
        &Selection::one(component),
        adapters,
        &history,
        &Confirmations::none(),
    );
    engine
        .execute(&plan, adapters, store)
        .map_err(|e| e.to_string())
}

/// Applies again the defaults of `manifest` whose ids are in `only`.
fn apply_again(
    manifest: ComponentManifest,
    system: SystemContext,
    adapters: &mut AdapterSet,
    store: &SnapshotStore,
    only: &[String],
) -> Result<DefaultsOutcome, String> {
    let component = manifest.id.clone();
    let catalog = ComponentCatalog::from_manifests(vec![manifest]).map_err(|e| e.to_string())?;
    let engine = DefaultsEngine::new(&catalog, system)
        .with_readiness(component.clone(), ComponentReadiness::ready());
    let history = store.history().map_err(|e| e.to_string())?;
    let mut plan = engine.plan_apply(
        &Selection::one(component),
        adapters,
        &history,
        &Confirmations::none(),
    );
    plan.entries
        .retain(|entry| only.iter().any(|id| id == entry.integration.as_str()));
    engine
        .execute(&plan, adapters, store)
        .map_err(|e| e.to_string())
}

/// What the session the program runs in is: `None` when there is none to
/// put anything back in (no desktop named, or no session bus).
pub fn session_context(distribution: String, desktop: String, bus: bool) -> Option<SystemContext> {
    (bus && !desktop.trim().is_empty()).then(|| SystemContext::new(distribution, desktop))
}

/// Gives back what `manifest`'s component took over: the engine's Restore,
/// then any activation file Better OS wrote and kept no record of, and
/// remembers what was given back for [`take_again`]. With no session, leaves
/// a pending marker and says [`PutBack::Deferred`].
pub fn put_back(
    manifest: ComponentManifest,
    session: Option<SystemContext>,
    adapters: &mut AdapterSet,
    store: &SnapshotStore,
    state: &RemovalState,
    leftover: impl FnOnce() -> Vec<String>,
) -> PutBack {
    let Some(system) = session else {
        return match state.mark_pending(Pending::PutBack) {
            Ok(()) => PutBack::Deferred,
            Err(error) => {
                PutBack::NotRestored(format!("the pending put-back was not kept: {error}"))
            }
        };
    };
    let outcome = match restore_component(manifest, system, adapters, store) {
        Ok(outcome) => outcome,
        Err(error) => return PutBack::NotRestored(error),
    };
    let given_back: Vec<IntegrationId> = outcome
        .results
        .iter()
        .filter(|result| matches!(result.outcome, EntryOutcome::Restored { .. }))
        .map(|result| result.integration.clone())
        .collect();
    let _ = state.remember(&given_back);
    let _ = leftover();
    state.clear_pending();
    if outcome.has_failures() {
        PutBack::NotRestored("a default could not be put back".to_string())
    } else {
        PutBack::Done
    }
}

/// This person's session as the programs and Better Manager meet it.
fn production() -> (
    Option<SystemContext>,
    Result<crate::adapters::AdapterSession, String>,
) {
    let distribution = better_core::host::read_os_release()
        .map(|content| better_core::host::resolve_distribution_id(&content))
        .unwrap_or_default();
    let bus = defaults_platform::bus_activation::ZbusSession::session().reachable();
    let session = session_context(distribution, crate::adapters::desktop_session(), bus);
    let adapters = crate::adapters::AdapterSession::open(&crate::adapters::AdapterMode::Production)
        .map_err(|error| error.to_string());
    (session, adapters)
}

/// Better Files' activation files that Better OS wrote and has no record
/// of, removed when they are byte for byte its own.
fn leftover_files() -> Vec<String> {
    use defaults_platform::bus_activation::{
        ZbusSession, production_activation, remove_own_files, user_services_dir,
    };
    remove_own_files(
        &user_services_dir(),
        "better-files",
        production_activation,
        &ZbusSession::session(),
    )
}

/// `better-files-before-removal` and its `--undo`, in this person's session.
pub fn better_files_program(undo: bool) -> PutBack {
    let manifest = match ComponentManifest::parse_yaml(BETTER_FILES_MANIFEST) {
        Ok(manifest) => manifest,
        Err(error) => return PutBack::NotRestored(error.to_string()),
    };
    let (session, adapters) = production();
    let mut adapters = match adapters {
        Ok(adapters) => adapters,
        Err(error) => return PutBack::NotRestored(error),
    };
    let store = SnapshotStore::from_default_path();
    let state = RemovalState::for_user("better-files");
    if undo {
        take_again(manifest, session, adapters.adapters_mut(), &store, &state)
    } else {
        put_back(
            manifest,
            session,
            adapters.adapters_mut(),
            &store,
            &state,
            leftover_files,
        )
    }
}

/// Where a component stands in Better Manager's record, for its start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Standing {
    pub installed: bool,
    pub enabled: bool,
}

/// What Better Manager's start does for one component's defaults (ticket
/// 129), going by the record rather than only by whether it is installed:
/// - not installed, or installed and turned off: a pending put-back is
///   finished, and, when it is not installed, Better OS's own activation
///   files it left are taken away (`leftover`);
/// - installed and on: a pending take-back is finished, and a pending
///   put-back that the component being turned on again overtook is dropped.
///
/// Returns whether anything was done.
#[allow(clippy::too_many_arguments)]
pub fn finish_one(
    manifest: ComponentManifest,
    standing: Standing,
    session: Option<SystemContext>,
    adapters: &mut AdapterSet,
    store: &SnapshotStore,
    state: &RemovalState,
    leftover: impl FnOnce() -> Vec<String>,
) -> bool {
    let on = standing.installed && standing.enabled;
    let mut done = false;
    if !standing.installed {
        done |= !leftover().is_empty();
    }
    match (state.pending(), on) {
        (Some(Pending::PutBack), false) => {
            done |= put_back(manifest, session, adapters, store, state, Vec::new) == PutBack::Done;
        }
        (Some(Pending::TakeAgain), true) => {
            done |= take_again(manifest, session, adapters, store, state) == PutBack::Done;
        }
        // What was pending was overtaken: on again before a put-back ran,
        // or off again before a take-back did.
        (Some(_), _) => {
            state.clear_pending();
            done = true;
        }
        (None, _) => {}
    }
    done
}

/// Better Manager's start in the person's session, for every component
/// whose manifest it has and that has a pending marker or a memory here, and
/// for every component whose own activation files outlived it (`apt remove`
/// never runs a before-removal program). `standing` reads the record.
/// Returns whether anything was done.
pub fn finish_in_session(
    manifests: Vec<ComponentManifest>,
    standing: impl Fn(&str) -> Option<Standing>,
) -> bool {
    use defaults_platform::bus_activation::{
        ZbusSession, production_activation, remove_own_files, user_services_dir,
    };
    let marked = RemovalState::components_in(&RemovalState::user_directory());
    let (session, adapters) = production();
    let mut adapters = match adapters {
        Ok(adapters) => adapters,
        Err(_) => return false,
    };
    let store = SnapshotStore::from_default_path();
    let mut done = false;
    for manifest in manifests {
        let id = manifest.id.as_str().to_string();
        let Some(standing) = standing(&id) else {
            continue;
        };
        let state = RemovalState::for_user(&id);
        if !marked.contains(&id) && standing.installed {
            continue;
        }
        let leftover = || {
            remove_own_files(
                &user_services_dir(),
                &id,
                production_activation,
                &ZbusSession::session(),
            )
        };
        done |= finish_one(
            manifest,
            standing,
            session.clone(),
            adapters.adapters_mut(),
            &store,
            &state,
            leftover,
        );
    }
    done
}

/// Takes again what the last [`put_back`] gave back: the removal did not
/// happen, or the component was turned on again.
pub fn take_again(
    manifest: ComponentManifest,
    session: Option<SystemContext>,
    adapters: &mut AdapterSet,
    store: &SnapshotStore,
    state: &RemovalState,
) -> PutBack {
    let only = state.remembered();
    if only.is_empty() {
        state.clear_pending();
        return PutBack::Done;
    }
    // With no session the memory stays, and Better Manager's next start in
    // the person's session takes it again.
    let Some(system) = session else {
        return match state.mark_pending(Pending::TakeAgain) {
            Ok(()) => PutBack::Deferred,
            Err(error) => {
                PutBack::NotRestored(format!("the pending take-back was not kept: {error}"))
            }
        };
    };
    match apply_again(manifest, system, adapters, store, &only) {
        Ok(outcome) if !outcome.has_failures() => {
            // Forgotten only once it is applied again.
            let _ = std::fs::remove_file(state.given_back());
            state.clear_pending();
            PutBack::Done
        }
        Ok(_) => PutBack::NotRestored("a default could not be applied again".to_string()),
        Err(error) => PutBack::NotRestored(error),
    }
}
