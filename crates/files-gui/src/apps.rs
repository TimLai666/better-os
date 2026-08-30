//! The Applications location: what it shows, how it refreshes, and what
//! opening a row does.
//!
//! Issue #4 forbids implementing this as a directory of any kind, and
//! `files-core` already holds that by construction — an application row is an
//! `EntryBody::Application` with no path accessor at all. What was missing was
//! the other half: something to put a real catalog behind it, keep that catalog
//! current, and turn a launch intent into a running application.
//!
//! Three rules this module exists to keep.
//!
//! **One catalog.** [`CatalogHandle`] holds a snapshot produced by
//! `app-catalog-platform`, the same crate Better Launcher and Better App
//! Chooser read. Nothing here parses a desktop file.
//!
//! **Launching is never a shell string.** [`launch`] hands the record to
//! `app_catalog_platform::Launcher`, which builds an argument vector from the
//! registered definition and honours D-Bus activation. The spawner is the
//! platform crate's own `ProcessSpawner` trait, so a test that proves a launch
//! proves the production path with only the `fork` replaced.
//!
//! **No `.desktop` file is ever shown as the application.**
//! [`ApplicationDetails`] carries the source path because Issue #4 asks for
//! source metadata to be *revealed for diagnostics*, and it is a labelled
//! diagnostic field rather than the row's name, its path, or anything a click
//! acts on.

use std::sync::{Arc, RwLock};

use app_catalog_core::{
    ApplicationRecord, Catalog, DesktopId, EntryScope, EntryWarning, ExecutableStatus,
    LaunchTarget, Locale, NoCanonicalExecutable, SourceKind, Visibility,
};
use app_catalog_platform::{
    HostProbe, LaunchOutcome, Launcher, ProcessSpawner, SessionEnvironment, load_catalog,
};
use files_core::{ApplicationView, Entry, LocalPath};

/// A catalog snapshot the whole window shares.
///
/// The `RwLock` is read on every Applications listing and written only when the
/// watcher says the desktop entries changed, which is the access pattern it is
/// for. The inner `Arc` means a listing takes a snapshot and releases the lock
/// immediately, so a reload cannot block a directory that is already streaming.
#[derive(Clone)]
pub struct CatalogHandle {
    catalog: Arc<RwLock<Arc<Catalog>>>,
    session: Arc<SessionEnvironment>,
    /// How many times the catalog has been replaced. A view compares this to
    /// what it last drew and reloads if it moved, which is cheaper than
    /// comparing two catalogs.
    generation: Arc<std::sync::atomic::AtomicU64>,
}

impl CatalogHandle {
    /// An empty catalog with a given session. Used before the first load
    /// finishes and by tests.
    pub fn empty(session: SessionEnvironment) -> Self {
        Self {
            catalog: Arc::new(RwLock::new(Arc::new(Catalog::default()))),
            session: Arc::new(session),
            generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn with_catalog(session: SessionEnvironment, catalog: Catalog) -> Self {
        let handle = Self::empty(session);
        handle.replace(catalog);
        handle
    }

    /// Reads the host. Blocking I/O over every XDG application directory, so a
    /// caller runs it on a background thread; nothing here touches a window.
    pub fn load_from_env() -> Self {
        let session = SessionEnvironment::from_env();
        let probe = HostProbe::from_env();
        let catalog = load_catalog(&session, &probe);
        Self::with_catalog(session, catalog)
    }

    /// Re-reads the host into this handle. This is what a `CatalogWatcher`
    /// change triggers: the watcher never says *which* record changed, because
    /// precedence means one new file can reveal a different application, so the
    /// answer is always a whole reload.
    pub fn reload_from_env(&self) {
        let probe = HostProbe::from_env();
        self.replace(load_catalog(&self.session, &probe));
    }

    pub fn replace(&self, catalog: Catalog) {
        *self.catalog.write().expect("catalog lock") = Arc::new(catalog);
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn snapshot(&self) -> Arc<Catalog> {
        self.catalog.read().expect("catalog lock").clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn session(&self) -> &SessionEnvironment {
        &self.session
    }

    pub fn locale(&self) -> Option<&Locale> {
        self.session.locale.as_ref()
    }

    /// The view the listing uses: this session's desktop, this session's
    /// language, and the window's hidden-entry preference.
    pub fn view(&self, include_hidden: bool) -> ApplicationView {
        ApplicationView {
            environments: self.session.environments.clone(),
            locale: self.session.locale.clone(),
            include_hidden,
        }
    }

    pub fn len(&self) -> usize {
        self.snapshot().len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshot().is_empty()
    }
}

/// What "View Details" shows about one application.
///
/// Every field is something the catalog recorded. Nothing is derived from the
/// file name, and nothing is guessed for an entry that did not declare it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationDetails {
    pub desktop_id: String,
    pub name: String,
    pub comment: Option<String>,
    pub categories: Vec<String>,
    pub source_kind: SourceKind,
    pub scope: EntryScope,
    /// The `.desktop` file this record came from. A diagnostic, drawn under a
    /// heading that says so — never the row's name and never something a click
    /// opens.
    pub source_path: String,
    pub executable: ExecutableSummary,
    pub mime_types: Vec<String>,
    pub actions: Vec<(String, String)>,
    pub dbus_activatable: bool,
    pub visible: bool,
    /// Warnings the catalog recorded while normalizing this entry, as stable
    /// keys.
    pub warnings: Vec<String>,
}

/// What the catalog could say about the program behind the entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableSummary {
    Resolved(String),
    /// The `Exec` line named a program that is not on `PATH`.
    Unresolved(String),
    /// A Flatpak, Snap, AppImage, wrapper, or D-Bus-activated entry has no
    /// single executable, and the catalog says so rather than fabricating one.
    NotApplicable(NoCanonicalExecutable),
}

/// Builds the details panel from a record.
pub fn details_for(record: &ApplicationRecord, locale: Option<&Locale>) -> ApplicationDetails {
    ApplicationDetails {
        desktop_id: record.desktop_id.as_str().to_string(),
        name: record.display_name(locale).to_string(),
        comment: record
            .comment
            .as_ref()
            .map(|text| text.resolve(locale).to_string()),
        categories: record.categories.clone(),
        source_kind: record.source.kind,
        scope: record.source.scope,
        source_path: record.source.path.display().to_string(),
        executable: match &record.executable {
            ExecutableStatus::Resolved(path) => {
                ExecutableSummary::Resolved(path.display().to_string())
            }
            ExecutableStatus::Unresolved { program } => {
                ExecutableSummary::Unresolved(program.clone())
            }
            ExecutableStatus::NotApplicable { reason } => ExecutableSummary::NotApplicable(*reason),
        },
        mime_types: record
            .mime_types
            .iter()
            .map(|mime| mime.as_str().to_string())
            .collect(),
        actions: record
            .actions
            .iter()
            .map(|action| (action.id.clone(), action.name.resolve(locale).to_string()))
            .collect(),
        dbus_activatable: record.capabilities.dbus_activatable,
        visible: !record.visibility.hidden && !record.visibility.no_display,
        warnings: record.warnings.iter().map(warning_key).collect(),
    }
}

/// Looks up an application and builds its details.
pub fn details(catalog: &CatalogHandle, desktop_id: &DesktopId) -> Option<ApplicationDetails> {
    let snapshot = catalog.snapshot();
    let record = snapshot.get(desktop_id)?;
    Some(details_for(record, catalog.locale()))
}

/// Whether an application declares a way to open a second window.
///
/// Issue #4 puts "Open New Window, when a desktop action or supported
/// activation path exists" in scope, and the condition is the point: an entry
/// with neither gets no such action rather than a button that starts a second
/// copy of a single-instance application.
pub fn new_window_action(record: &ApplicationRecord) -> Option<String> {
    const CANDIDATES: [&str; 4] = ["new-window", "NewWindow", "new_window", "window"];
    record
        .actions
        .iter()
        .find(|action| {
            CANDIDATES
                .iter()
                .any(|candidate| action.id.eq_ignore_ascii_case(candidate))
        })
        .map(|action| action.id.clone())
}

/// What a launch attempt produced, as a value a view renders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchReport {
    Started {
        name: String,
        processes: usize,
    },
    Activated {
        name: String,
    },
    /// D-Bus activation was asked for and unavailable, so the entry's own
    /// `Exec` line ran. The specification requires that line to exist for
    /// exactly this case, and reporting it is how a diagnostic session can
    /// tell the two paths apart.
    FellBackToProcess {
        name: String,
        processes: usize,
    },
    /// The catalog does not have this application any more, which is what a
    /// stale row after an uninstall looks like.
    NoSuchApplication {
        desktop_id: String,
    },
    Failed {
        name: String,
        detail: String,
    },
}

impl LaunchReport {
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            LaunchReport::NoSuchApplication { .. } | LaunchReport::Failed { .. }
        )
    }
}

/// Starts an application through its registered desktop definition.
///
/// `targets` is empty for "open the application" and holds the selected files
/// for "open these with it". Both go through the same platform path, because
/// Issue #4 requires exactly one place that turns an `Exec` line into a
/// process.
pub fn launch(
    catalog: &CatalogHandle,
    desktop_id: &DesktopId,
    action: Option<&str>,
    targets: &[LaunchTarget],
    spawner: &dyn ProcessSpawner,
) -> LaunchReport {
    let snapshot = catalog.snapshot();
    let Some(record) = snapshot.get(desktop_id) else {
        return LaunchReport::NoSuchApplication {
            desktop_id: desktop_id.as_str().to_string(),
        };
    };
    let name = record.display_name(catalog.locale()).to_string();
    match Launcher::new(spawner).launch(record, action, targets, catalog.locale()) {
        Ok(LaunchOutcome::Started { processes }) => LaunchReport::Started { name, processes },
        Ok(LaunchOutcome::Activated) => LaunchReport::Activated { name },
        Ok(LaunchOutcome::ActivationFellBackToProcess { processes }) => {
            LaunchReport::FellBackToProcess { name, processes }
        }
        Err(error) => LaunchReport::Failed {
            name,
            detail: error.to_string(),
        },
    }
}

/// Builds a launch target from a file the user selected.
///
/// Returns `None` for a path the launch layer refuses — a relative path, a
/// non-UTF-8 path, an embedded NUL, one that is too long. A refused target is
/// dropped rather than passed on, because a half-valid argument vector is worse
/// than a launch that did not happen.
pub fn target_for(path: &LocalPath) -> Option<LaunchTarget> {
    LaunchTarget::path(path.as_path().to_path_buf()).ok()
}

/// The launch targets for a selection, and how many were refused.
pub fn targets_for(entries: &[&Entry]) -> (Vec<LaunchTarget>, usize) {
    let mut targets = Vec::new();
    let mut refused = 0;
    for entry in entries {
        match entry.as_local_path().and_then(target_for) {
            Some(target) => targets.push(target),
            None => refused += 1,
        }
    }
    (targets, refused)
}

/// A stable key per warning, so the diagnostics list is presentable rather
/// than a `Debug` rendering of an enum.
fn warning_key(warning: &EntryWarning) -> String {
    match warning {
        EntryWarning::DroppedMimeType(value) => {
            format!("files.apps.warning.dropped_mime_type:{value}")
        }
        EntryWarning::DroppedIconPath(value) => {
            format!("files.apps.warning.dropped_icon_path:{value}")
        }
        EntryWarning::DroppedActionExec { action, error } => {
            format!("files.apps.warning.dropped_action_exec:{action}:{error}")
        }
    }
}

/// Whether a record is shown in this session's Applications view.
pub fn is_visible(record: &ApplicationRecord, catalog: &CatalogHandle) -> bool {
    record.visibility_in(&catalog.session().environments) == Visibility::Visible
}
