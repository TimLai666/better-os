//! What the shared application catalog says is installed, as a restore needs
//! it.
//!
//! A restore puts back a captured desktop entry. When that application has been
//! uninstalled since, writing its ID would point the type at nothing, so the
//! engine asks here first. The answer comes from `app-catalog-core`'s one
//! catalog; nothing in this crate parses a `.desktop` file or walks an
//! application directory.

use app_catalog_core::{Catalog, DesktopEnvironments, DesktopId, Locale, MimeType};
use serde::{Deserialize, Serialize};

/// One installed application a person can choose instead of one that is gone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApplicationChoice {
    pub desktop_id: String,
    pub name: String,
}

/// The installed applications a restore checks against.
pub trait InstalledApplications {
    /// Whether the catalog holds an application with this desktop ID. An ID
    /// that is not a valid desktop ID is never installed.
    fn is_installed(&self, desktop_id: &str) -> bool;

    /// The applications that declare every one of `types`, in the order a
    /// person would look for them. A type that is not a MIME type has none.
    fn for_types(&self, types: &[String]) -> Vec<ApplicationChoice>;
}

/// The shared catalog, with the session details that decide which of its
/// records a person would be offered.
#[derive(Clone, Debug)]
pub struct ApplicationCatalog {
    catalog: Catalog,
    environments: DesktopEnvironments,
    locale: Option<Locale>,
}

impl ApplicationCatalog {
    pub fn new(
        catalog: Catalog,
        environments: DesktopEnvironments,
        locale: Option<Locale>,
    ) -> Self {
        Self {
            catalog,
            environments,
            locale,
        }
    }
}

impl InstalledApplications for ApplicationCatalog {
    fn is_installed(&self, desktop_id: &str) -> bool {
        DesktopId::new(desktop_id).is_ok_and(|id| self.catalog.get(&id).is_some())
    }

    fn for_types(&self, types: &[String]) -> Vec<ApplicationChoice> {
        let Some(types) = types
            .iter()
            .map(|value| MimeType::parse(value))
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        if types.is_empty() {
            return Vec::new();
        }
        self.catalog
            .sorted_by_name(self.locale.as_ref(), &self.environments)
            .into_iter()
            .filter(|record| types.iter().all(|mime| record.supports_mime_type(mime)))
            .map(|record| ApplicationChoice {
                desktop_id: record.desktop_id.as_str().to_string(),
                name: record.display_name(self.locale.as_ref()).to_string(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_catalog_core::{CatalogBuilder, DirectoryRank, EntryScope};
    use std::path::PathBuf;

    fn catalog(entries: &[(&str, &str)]) -> ApplicationCatalog {
        let mut builder = CatalogBuilder::default();
        let rank = DirectoryRank {
            rank: 0,
            scope: EntryScope::System,
        };
        for (id, body) in entries {
            builder.add_entry(
                DesktopId::new(*id).unwrap(),
                PathBuf::from(format!("/fixture/applications/{id}")),
                &rank,
                body.as_bytes(),
            );
        }
        ApplicationCatalog::new(builder.build(), DesktopEnvironments::parse("GNOME"), None)
    }

    fn entry(name: &str, types: &str, extra: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec=app %U\nMimeType={types}\n{extra}"
        )
    }

    #[test]
    fn an_application_the_catalog_holds_is_installed_whether_or_not_it_is_shown() {
        let loupe = entry("Image Viewer", "image/png;", "");
        let helper = entry("Helper", "image/png;", "NoDisplay=true\n");
        let catalog = catalog(&[
            ("org.gnome.Loupe.desktop", &loupe),
            ("helper.desktop", &helper),
        ]);
        assert!(catalog.is_installed("org.gnome.Loupe.desktop"));
        assert!(catalog.is_installed("helper.desktop"));
        assert!(!catalog.is_installed("org.gnome.eog.desktop"));
        assert!(!catalog.is_installed("not a desktop id"));
    }

    #[test]
    fn the_offer_is_what_declares_every_type_sorted_by_name_and_shown() {
        let both = entry("Zoom Viewer", "image/png;image/jpeg;", "");
        let png = entry("PNG Only", "image/png;", "");
        let first = entry("Another Viewer", "image/png;image/jpeg;", "");
        let hidden = entry("Background", "image/png;image/jpeg;", "NoDisplay=true\n");
        let catalog = catalog(&[
            ("zoom.desktop", &both),
            ("png.desktop", &png),
            ("another.desktop", &first),
            ("background.desktop", &hidden),
        ]);
        let ids = |types: &[&str]| -> Vec<String> {
            catalog
                .for_types(
                    &types
                        .iter()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>(),
                )
                .into_iter()
                .map(|choice| choice.desktop_id)
                .collect()
        };
        assert_eq!(
            ids(&["image/png", "image/jpeg"]),
            vec!["another.desktop", "zoom.desktop"]
        );
        assert_eq!(
            ids(&["image/png"]),
            vec!["another.desktop", "png.desktop", "zoom.desktop"]
        );
        assert!(ids(&["not a type"]).is_empty());
        assert!(ids(&[]).is_empty());
        assert_eq!(
            catalog.for_types(&["image/png".to_string()])[0].name,
            "Another Viewer"
        );
    }
}
