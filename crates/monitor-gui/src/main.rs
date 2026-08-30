use better_ui::{page_heading, status_card};
use gpui::*;
use gpui_component::{Root, StyledExt, button::Button};
use monitor_core::{
    CollectorId, CollectorReport, MetricId, MetricSet, MonitorStore, Observation, ObservationState,
    Timestamp, UnsupportedReason,
};

/// The shell still renders a fixed demonstration round rather than live
/// collectors. What changed is that it now speaks the real contract, so the
/// real views in the next ticket replace the source of the report without
/// touching the presentation. A metric with no value is rendered as the
/// reason it has none, never as a zero.
fn demonstration_report() -> CollectorReport {
    let mut report = CollectorReport::new(
        CollectorId::new("demo").expect("a valid collector id"),
        Timestamp::now(),
    );
    let mut metrics = MetricSet::new();
    metrics.insert(
        MetricId::new("cpu.utilization.busy").expect("a valid metric id"),
        Observation::float(0.12),
    );
    metrics.insert(
        MetricId::new("memory.utilization").expect("a valid metric id"),
        Observation::float(0.38),
    );
    metrics.insert(
        MetricId::new("pressure.some.avg10").expect("a valid metric id"),
        Observation::Unsupported(UnsupportedReason::InterfaceMissing {
            path: "/proc/pressure/cpu".into(),
        }),
    );
    report.metrics = metrics;
    report
}

/// A reading as one line of text, keeping the five observation states apart.
fn present(report: &CollectorReport, id: &str, percent: bool) -> String {
    let Ok(metric) = MetricId::new(id) else {
        return "Unavailable".to_string();
    };
    let Some(observation) = report.metrics.get(&metric) else {
        return "Not collected".to_string();
    };
    match observation.state() {
        ObservationState::Value | ObservationState::Stale => {
            let value = observation
                .as_f64()
                .or_else(|| match observation {
                    Observation::Stale { value, .. } => value.as_f64(),
                    _ => None,
                })
                .unwrap_or_default();
            let rendered = if percent {
                format!("{:.0}%", value * 100.0)
            } else {
                format!("{value:.2}")
            };
            if observation.state() == ObservationState::Stale {
                format!("{rendered} (stale)")
            } else {
                rendered
            }
        }
        ObservationState::Unknown => "Not measured yet".to_string(),
        ObservationState::Unsupported => "Not available on this system".to_string(),
        ObservationState::PermissionDenied => "Needs permission".to_string(),
    }
}

struct MonitorWindow {
    store: MonitorStore,
    latest: CollectorReport,
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
            .child(status_card(
                "CPU",
                present(&self.latest, "cpu.utilization.busy", true),
            ))
            .child(status_card(
                "Memory",
                present(&self.latest, "memory.utilization", true),
            ))
            .child(status_card(
                "CPU pressure",
                present(&self.latest, "pressure.some.avg10", false),
            ))
            .child(status_card(
                "Observation mode",
                "Continuous + event-triggered",
            ))
            .child(status_card(
                "Collector rounds",
                self.store.reports().len().to_string(),
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
                let latest = demonstration_report();
                store.record_report(latest.clone());
                let view = cx.new(|_| MonitorWindow { store, latest });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open Better Monitor window");
        })
        .detach();
    });
}
