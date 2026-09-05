use crate::app::ComponentTranslation;
use crate::i18n::{Locale, copy};
use better_core::{ComponentIcon, ComponentId, ComponentManifest, ComponentType};
use manager_core::catalog::{CatalogDegradation, CatalogSource, CatalogStatus};
use manager_core::{ComponentRecord, ComponentStatus, HealthState, RestartRequirement};

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
    pub(crate) installed_version: Option<String>,
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

    pub(crate) fn version_label(&self) -> String {
        match self.installed_version.as_deref() {
            Some(current) if current != self.available_version => {
                format!("{current} → {}", self.available_version)
            }
            Some(current) => current.to_string(),
            None => self.available_version.clone(),
        }
    }

    /// A stable element-ID fragment. Component IDs are already restricted to
    /// lowercase ASCII, digits, and dashes by the manifest parser.
    pub(crate) fn element_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.core_id)
    }
}
