//! The Better Awake Status window.
//!
//! Phase 1's window is deliberately small: it shows what the service is doing
//! and why, and it is the place a person can look when the tray icon is not
//! available. Quick Sessions, Automatic Rules, History, and Diagnostics are
//! ticket 26's, and none of them are stubbed out here — an empty section that
//! looks clickable is worse than one that is not there yet.
//!
//! The window holds no session state. Everything it shows came from one query
//! to the service, and closing it changes nothing.

use awake_ipc::{StatusDocument, WireIndicator, WireRemaining};
use awake_tray::client::ServiceClient;
use awake_tray::labels::{Labels, Locale};
use awake_tray::localtime::{UtcOffset, clock_time};
use awake_tray::menu::duration_label;
use better_ui::{page_heading, status_card};
use gpui::*;
use gpui_component::{Root, StyledExt};

/// What the window is showing, including the case where there is nothing to
/// show because the service could not be reached.
enum View {
    Connected(Box<StatusDocument>),
    Unreachable(String),
}

struct StatusWindow {
    view: View,
    locale: Locale,
}

impl StatusWindow {
    fn labels(&self) -> &'static Labels {
        self.locale.labels()
    }

    fn rows(&self) -> Vec<(String, String)> {
        let labels = self.labels();
        let status = match &self.view {
            View::Unreachable(detail) => {
                return vec![(
                    labels.backend_unavailable.to_string(),
                    // The reason is a stable key; showing it beats showing
                    // nothing when someone has to diagnose this.
                    detail.clone(),
                )];
            }
            View::Connected(status) => status,
        };

        let offset = UtcOffset::for_system(status.now_unix_seconds);
        let mut rows = vec![(
            labels.application_name.to_string(),
            match status.indicator {
                WireIndicator::Unavailable => labels.backend_unavailable.to_string(),
                WireIndicator::AttentionRequired => labels.attention.to_string(),
                _ if status.is_active() => labels.active_summary.to_string(),
                _ => labels.inactive_summary.to_string(),
            },
        )];

        if let Some(session) = status.manual_session().or_else(|| status.sessions.first()) {
            rows.push((labels.reason.to_string(), session.reason.clone()));
            rows.push((
                labels.remaining.to_string(),
                match session.remaining {
                    WireRemaining::UntilEnded => labels.until_ended.to_string(),
                    WireRemaining::Seconds { seconds } => duration_label(seconds, labels),
                    WireRemaining::Elapsed => duration_label(0, labels),
                },
            ));
            rows.push((
                labels.started.to_string(),
                clock_time(session.started_at_unix_seconds, offset),
            ));
        }

        rows.push((
            labels.system_sleep.to_string(),
            yes_no(status.effective_policy.prevent_system_suspend, labels),
        ));
        rows.push((
            labels.display_sleep.to_string(),
            yes_no(status.effective_policy.prevent_display_sleep, labels),
        ));
        rows.push((
            labels.automatic_lock.to_string(),
            yes_no(status.effective_policy.prevent_automatic_lock, labels),
        ));
        rows.push((
            labels.battery_protection.to_string(),
            match status.battery_stop_percent {
                Some(percent) => labels
                    .battery_stops_at
                    .replace("{percent}", &percent.to_string()),
                None => labels.battery_off.to_string(),
            },
        ));

        if let Some(interrupted) = &status.interrupted_previous_session {
            rows.push((
                labels.interrupted_previous_session.to_string(),
                interrupted.reason.clone(),
            ));
        }
        rows
    }
}

fn yes_no(prevented: bool, labels: &Labels) -> String {
    if prevented {
        labels.prevented.to_string()
    } else {
        labels.allowed.to_string()
    }
}

impl Render for StatusWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let heading = self.labels().application_name.to_string();
        let mut root = div().v_flex().gap_2().size_full().p_4();
        root = root.child(page_heading(heading));
        for (title, value) in self.rows() {
            root = root.child(status_card(title, value));
        }
        root
    }
}

fn main() {
    let locale = Locale::from_environment();

    // One query, on a runtime that ends before the window opens. The window is
    // a reader; it never holds a session or a connection open.
    let view = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map(|runtime| {
            runtime.block_on(async {
                match ServiceClient::connect().await {
                    Ok(client) => match client.status().await {
                        Ok(status) => View::Connected(Box::new(status)),
                        Err(error) => View::Unreachable(error.to_string()),
                    },
                    Err(error) => View::Unreachable(error.to_string()),
                }
            })
        })
        .unwrap_or_else(|error| View::Unreachable(error.to_string()));

    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        let mut view = Some(view);
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = view.take().expect("the window is opened once");
                let window_view = cx.new(|_| StatusWindow { view, locale });
                cx.new(|cx| Root::new(window_view, window, cx))
            })
            .expect("failed to open the Better Awake status window");
        })
        .detach();
    });
}
