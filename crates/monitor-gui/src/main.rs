use better_ui::{page_heading, status_card};
use gpui::*;
use gpui_component::{Root, StyledExt, button::Button};
use monitor_core::{MonitorStore, Sample};

struct MonitorWindow {
    store: MonitorStore,
}

impl Render for MonitorWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .gap_2()
            .size_full()
            .p_4()
            .child(page_heading("Better Monitor"))
            .child(status_card(
                "Navigation",
                "Overview • Apps & Processes • Hardware • History • Incidents",
            ))
            .child(status_card("CPU", "12%"))
            .child(status_card("Memory", "38%"))
            .child(status_card(
                "Observation mode",
                "Continuous + event-triggered",
            ))
            .child(status_card(
                "Samples",
                self.store.samples().len().to_string(),
            ))
            .child(Button::new("record-incident").label("Record incident"))
    }
}

fn main() {
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let mut store = MonitorStore::default();
                store.record_sample(Sample {
                    timestamp_unix_ms: 1,
                    cpu_percent: 12.0,
                    memory_percent: 38.0,
                    psi_some_percent: None,
                });
                let view = cx.new(|_| MonitorWindow { store });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open Better Monitor window");
        })
        .detach();
    });
}
