use crate::app::ComponentTranslation;
use crate::i18n::{Locale, copy};
use better_core::{ComponentIcon, ComponentId, ComponentManifest, ComponentType};
use manager_core::catalog::{CatalogDegradation, CatalogSource, CatalogStatus};
use manager_core::{
    ComponentRecord, ComponentStatus, FailureRecord, HealthState, InstallProvenance,
    RestartRequirement, SystemProfile,
};

/// The one status line the Components screen shows about the catalog itself.
///
/// It is built here, with no GPUI in sight, because whether a catalog is
/// presented as current or as possibly outdated is a decision and not a
/// rendering detail. The screen draws what this says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CatalogLine {
    /// Where the catalog came from, in the user's words.
    pub(crate) source: &'static str,
    /// How old it is, or that it has never been fetched.
    pub(crate) age: String,
    /// What is wrong with it, if anything. `None` means it was fetched in full
    /// during this session.
    pub(crate) warning: Option<&'static str>,
    /// How many manifests the last refresh refused.
    pub(crate) rejected: usize,
}

impl CatalogLine {
    pub(crate) fn present(locale: Locale, status: &CatalogStatus, now_unix_seconds: u64) -> Self {
        let c = copy(locale);
        Self {
            source: match status.source {
                CatalogSource::BuiltIn => c.catalog_source_built_in,
                CatalogSource::Cache => c.catalog_source_cache,
                CatalogSource::Remote => c.catalog_source_remote,
            },
            age: match status.fetched_at_unix_seconds {
                None => c.catalog_never_updated.to_string(),
                Some(fetched_at) => age_phrase(locale, now_unix_seconds.saturating_sub(fetched_at)),
            },
            warning: status.degraded.map(|degradation| match degradation {
                CatalogDegradation::NeverRefreshed => c.catalog_degraded_never,
                CatalogDegradation::RefreshFailedUsingCache => c.catalog_degraded_failed_cache,
                CatalogDegradation::RefreshFailedUsingBuiltIn => c.catalog_degraded_failed_built_in,
                CatalogDegradation::PartiallyRefreshed => c.catalog_degraded_partial,
            }),
            rejected: status.rejections.len(),
        }
    }

    pub(crate) fn is_degraded(&self) -> bool {
        self.warning.is_some()
    }

    /// The count of refused manifests, as a line, when there were any.
    pub(crate) fn rejected_line(&self, locale: Locale) -> Option<String> {
        (self.rejected > 0).then(|| {
            copy(locale)
                .catalog_rejected
                .replace("{n}", &self.rejected.to_string())
        })
    }
}

/// How long ago a fetch happened, in the coarsest unit that still says
/// something. A clock that ran backwards produces "just now" rather than a
/// negative age.
fn age_phrase(locale: Locale, seconds: u64) -> String {
    let c = copy(locale);
    let (template, value) = match seconds {
        0..=59 => return c.catalog_updated_just_now.to_string(),
        60..=3599 => (c.catalog_updated_minutes, seconds / 60),
        3600..=86_399 => (c.catalog_updated_hours, seconds / 3600),
        _ => (c.catalog_updated_days, seconds / 86_400),
    };
    template.replace("{n}", &value.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    FirstRun,
    Overview,
    Components,
    ComponentDetail(ComponentId),
    Defaults,
    DefaultsComponent(ComponentId),
    DefaultsReview,
    DefaultsResults,
    Updates,
    ReviewChanges,
    Installing,
    Finished,
    Restore,
    Restored,
    Health,
    DoctorResults,
    Activity,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Overview,
    Versions,
    Permissions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivityFilter {
    All,
    Success,
    Warning,
    Failure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentKind {
    Replacement,
    Enhancement,
    Diagnostic,
}

impl From<ComponentType> for ComponentKind {
    fn from(value: ComponentType) -> Self {
        match value {
            ComponentType::Replacement => Self::Replacement,
            ComponentType::Enhancement => Self::Enhancement,
            ComponentType::Diagnostic => Self::Diagnostic,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentInfo {
    pub(crate) core_id: ComponentId,
    /// Shipped translation when this build carries one, otherwise the name the
    /// manifest declares.
    pub(crate) name: String,
    /// One line of purpose. Empty when neither a translation nor the manifest
    /// declares one, which the presentation layer shows as undeclared rather
    /// than guessing from the component ID.
    pub(crate) summary: String,
    pub(crate) detail: String,
    pub(crate) icon: ComponentIcon,
    /// What is actually installed on this machine, as far as the manager knows.
    /// `None` means nothing is — never "the version the catalog offers".
    pub(crate) installed_version: Option<String>,
    /// Who installed it. A component apt put there carries no artifact and no
    /// restore snapshot, and the screen says so instead of implying otherwise.
    pub(crate) provenance: InstallProvenance,
    /// The attempt that failed, when one did. Kept whole so the component page
    /// can show the stage, the evidence, and the service's own words rather
    /// than a bare tag.
    pub(crate) failure: Option<FailureRecord>,
    pub(crate) enabled: bool,
    pub(crate) available_version: String,
    pub(crate) state: ComponentStatus,
    pub(crate) health: HealthState,
    pub(crate) restart_requirement: RestartRequirement,
    pub(crate) restore_available: bool,
    pub(crate) kind: ComponentKind,
    pub(crate) replaces: Vec<String>,
    pub(crate) enhances: Vec<String>,
    pub(crate) paths: Vec<String>,
    pub(crate) release_notes: Vec<String>,
}

impl ComponentInfo {
    /// Builds the row a catalog component is presented as. Name and purpose
    /// come from the shipped translation when one exists and from the manifest
    /// otherwise, so a component this build has never heard of still renders
    /// with its own declared identity.
    pub(crate) fn present(
        manifest: &ComponentManifest,
        record: Option<&ComponentRecord>,
        state: ComponentStatus,
        translation: Option<ComponentTranslation>,
    ) -> Self {
        let summary = match (translation, manifest.summary.as_deref()) {
            (Some(translation), _) => translation.summary.to_string(),
            (None, Some(summary)) => summary.to_string(),
            (None, None) => String::new(),
        };
        Self {
            core_id: manifest.id.clone(),
            name: translation
                .map(|translation| translation.name.to_string())
                .unwrap_or_else(|| manifest.display_name.clone()),
            detail: translation
                .map(|translation| translation.detail.to_string())
                .unwrap_or_else(|| summary.clone()),
            summary,
            icon: manifest.icon,
            installed_version: record.and_then(|record| record.installed_version.clone()),
            provenance: record.map(|record| record.provenance).unwrap_or_default(),
            failure: record.and_then(|record| record.failure.clone()),
            enabled: record.is_some_and(|record| record.enabled),
            available_version: manifest.version.to_string(),
            state,
            health: record.map(|record| record.health).unwrap_or_default(),
            restart_requirement: RestartRequirement::from(manifest.restart),
            restore_available: record
                .and_then(|record| record.restore_snapshot.as_ref())
                .is_some(),
            kind: manifest.component_type.clone().into(),
            replaces: manifest.replaces.clone(),
            enhances: manifest.enhances.clone(),
            paths: manifest.paths.clone(),
            release_notes: manifest.release_notes.clone(),
        }
    }

    /// What is installed, in the user's words.
    ///
    /// The component page used to render [`Self::version_label`] under an
    /// "Installed version" heading, which meant a component that was never
    /// installed reported the catalog's version as its own. `not_installed` is
    /// the only honest answer there.
    pub(crate) fn installed_label(&self, not_installed: &'static str) -> String {
        self.installed_version
            .clone()
            .unwrap_or_else(|| not_installed.to_string())
    }

    /// Whether something outside Better Manager put this component here.
    pub(crate) fn installed_outside_manager(&self) -> bool {
        self.installed_version.is_some() && self.provenance == InstallProvenance::Dpkg
    }

    /// The version line on a component card.
    ///
    /// A bare catalog version beside a red tag reads as "this installed version
    /// is broken", so a component with nothing installed says so first and
    /// offers the catalog version second.
    pub(crate) fn version_label(&self, not_installed: &'static str) -> String {
        match self.installed_version.as_deref() {
            Some(current) if current != self.available_version => {
                format!("{current} → {}", self.available_version)
            }
            Some(current) => current.to_string(),
            None => format!("{not_installed} · {}", self.available_version),
        }
    }

    /// A stable element-ID fragment. Component IDs are already restricted to
    /// lowercase ASCII, digits, and dashes by the manifest parser.
    pub(crate) fn element_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.core_id)
    }
}

/// The release and distribution a profile carries when the host could not be
/// identified at all. `SystemProfile::unidentified` writes it.
const UNKNOWN: &str = "unknown";

/// The one line the window footer shows about the machine.
///
/// A Zorin host is two facts, and the footer used to show them as one: it
/// printed the distribution ID beside the resolved Ubuntu release, so a Zorin
/// OS 18 machine read as "zorin 24.04" — a Zorin version that does not exist.
/// This keeps them apart. The host's own name and badge come first, the Ubuntu
/// base it is built on second, and a plain Ubuntu machine has only one fact to
/// show and shows it alone.
pub(crate) fn host_line(profile: &SystemProfile, base_word: &str) -> String {
    let base_is_known = profile.release != UNKNOWN;
    match (&profile.distribution_label, base_is_known) {
        // A derivative: its own identity, then the base its packages come from.
        (Some(label), true) if profile.distribution != "ubuntu" => {
            format!("{label} · Ubuntu {} {base_word}", profile.release)
        }
        // Plain Ubuntu, or a derivative whose base was never resolved.
        (Some(label), _) => label.clone(),
        // The host named itself nothing. Whatever was resolved is all there is.
        (None, true) => format!("Ubuntu {}", profile.release),
        (None, false) => profile.distribution.clone(),
    }
}
