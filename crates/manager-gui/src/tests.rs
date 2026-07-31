use std::collections::HashSet;

use crate::{
    app::demo_manager,
    i18n::{Locale, copy},
    model::{COMPONENTS, component_by_id},
};
use manager_core::{DesiredOperation, InstallationState};

#[test]
fn component_ids_are_unique_and_resolvable() {
    let mut ids = HashSet::new();
    for component in COMPONENTS {
        assert!(
            ids.insert(component.id),
            "duplicate component id: {}",
            component.id
        );
        assert_eq!(component_by_id(component.id), Some(component));
    }
}

#[test]
fn required_navigation_copy_exists_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for value in [
            c.overview,
            c.components,
            c.updates,
            c.health,
            c.activity,
            c.settings,
            c.update_all,
            c.review_changes,
            c.install_updates,
            c.traditional_chinese,
        ] {
            assert!(!value.trim().is_empty());
        }
    }
}

#[test]
fn update_all_uses_the_demo_catalog_and_shared_manager_plan() {
    let manager = demo_manager();
    let plan = manager.plan_all().expect("demo catalog must be plannable");

    assert!(plan.dry_run);
    assert_eq!(plan.steps.len(), 3);
    assert!(
        plan.steps
            .iter()
            .all(|step| step.operation == DesiredOperation::Update)
    );
    assert!(manager.manifests().all(|manifest| matches!(
        manager.status(&manifest.id),
        Ok(InstallationState::Installed { .. })
    )));
}
