use better_core::{ComponentCatalog, ComponentManifest};
use better_ui::{page_heading, status_card};
use gpui::*;
use gpui_component::{
    Root, StyledExt,
    button::{Button, ButtonVariants},
};
use manager_core::{InMemoryBackend, InstallationState, Manager};

struct ManagerWindow {
    manager: Manager<InMemoryBackend>,
}

impl Render for ManagerWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let component_count = self.manager.manifests().count().to_string();
        let installed_count = self
            .manager
            .manifests()
            .filter(|manifest| {
                matches!(
                    self.manager.status(&manifest.id),
                    Ok(InstallationState::Installed { .. })
                )
            })
            .count()
            .to_string();
        let available_count = self
            .manager
            .manifests()
            .filter(|manifest| {
                matches!(
                    self.manager.status(&manifest.id),
                    Ok(InstallationState::Available)
                )
            })
            .count()
            .to_string();
        let manager = self.manager.clone();
        div()
            .v_flex()
            .gap_2()
            .size_full()
            .p_4()
            .child(page_heading("Better Manager"))
            .child(status_card("Catalog components", component_count))
            .child(status_card("Installed", installed_count))
            .child(status_card("Available", available_count))
            .child(
                Button::new("update-all")
                    .primary()
                    .label("Update All")
                    .on_click(move |_, _, _| match manager.plan_all() {
                        Ok(plan) => println!("Update All planned {} step(s)", plan.steps.len()),
                        Err(error) => eprintln!("Update All planning failed: {error}"),
                    }),
            )
    }
}

fn demo_manager() -> Manager<InMemoryBackend> {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-files-example.yaml"),
    ]
    .into_iter()
    .map(|input| ComponentManifest::parse_yaml(input).expect("example manifest must be valid"))
    .collect::<Vec<_>>();
    Manager::new(
        ComponentCatalog::from_manifests(manifests).expect("example catalog must be valid"),
        InMemoryBackend::default().with_installed(
            better_core::ComponentId::new("better-monitor").expect("example id must be valid"),
            "0.1.0",
        ),
    )
}

fn main() {
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| ManagerWindow {
                    manager: demo_manager(),
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open Better Manager window");
        })
        .detach();
    });
}
