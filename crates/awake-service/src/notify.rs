//! The low-battery notification the service raises when no tray is running.
//!
//! The tray raises it when it is there, because it is the process in the
//! person's desktop session. Without one, a stop would only reach History and
//! stderr, so the service raises it itself. Which of the two does is decided by
//! the bus: a tray that will notify owns [`TRAY_BUS_NAME`], and the service
//! asks whether anyone does when it has a stop to report. Nothing is polled.
//!
//! A notification is a courtesy, not a control. When there is no notification
//! service, or it refuses or never answers, the failure is written to stderr and
//! nothing else changes: the session has already ended, been recorded in
//! History, and been announced on the bus.

use std::collections::HashMap;
use std::time::Duration;

use awake_ipc::notification::{
    DEFAULT_EXPIRY, LOW_BATTERY_ICON, Locale, TRAY_BUS_NAME, low_battery_notification,
};
use zbus::zvariant::Value;

use crate::engine::EndedSession;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    /// `Notify` as the Desktop Notifications Specification defines it,
    /// signature `susssasa{sv}i`.
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, &Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// The desktop's notification service.
const NOTIFICATIONS_SERVICE: &str = "org.freedesktop.Notifications";

/// How long a notification call may take before it is given up on.
pub const NOTIFY_TIMEOUT: Duration = Duration::from_secs(5);

/// One low-battery stop, as the notification words it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LowBatteryStop {
    pub session_id: u64,
    /// The threshold that was crossed, in percent.
    pub threshold_percent: u8,
    /// The reading that crossed it.
    pub percent: u8,
}

impl LowBatteryStop {
    /// The low-battery stops among `ended`. A stop always carries the threshold
    /// it crossed, so one without it is not a stop anything can word.
    pub fn among(ended: &[EndedSession]) -> Vec<Self> {
        ended
            .iter()
            .filter_map(|ended| {
                let stop = ended.battery_stop()?;
                Some(Self {
                    session_id: stop.session.0,
                    threshold_percent: ended.battery_stop_percent?,
                    percent: stop.percent,
                })
            })
            .collect()
    }
}

/// Raises the notification from the service.
///
/// The call is made on a task spawned through the tokio runtime captured here,
/// never awaited where the stop was found. A stop can be found inside a D-Bus
/// method handler, which zbus polls on its own executor where `tokio::spawn`
/// and tokio's timer are not available (see AGENTS.md), and a notification
/// service that never answers must not hold up the reply or the tick.
#[derive(Clone)]
pub struct LowBatteryNotifier {
    runtime: tokio::runtime::Handle,
    locale: Locale,
    destination: String,
    timeout: Duration,
}

impl LowBatteryNotifier {
    /// A notifier for the desktop's own notification service, worded in
    /// `locale`.
    pub fn new(runtime: tokio::runtime::Handle, locale: Locale) -> Self {
        Self {
            runtime,
            locale,
            destination: NOTIFICATIONS_SERVICE.to_string(),
            timeout: NOTIFY_TIMEOUT,
        }
    }

    /// Sends to a notification service published under another name. Used by
    /// the private-bus tests, so no test can reach a real one.
    pub fn with_destination(mut self, destination: String) -> Self {
        self.destination = destination;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Raises one notification for each stop, on its own task. The handle is
    /// returned for a caller that wants to wait; the service does not.
    pub fn raise(
        &self,
        connection: &zbus::Connection,
        stops: Vec<LowBatteryStop>,
    ) -> tokio::task::JoinHandle<()> {
        let notifier = self.clone();
        let connection = connection.clone();
        self.runtime.spawn(async move {
            for stop in stops {
                if let Err(error) = notifier.notify(&connection, &stop).await {
                    eprintln!(
                        "better-awake-service: session {} ended because the battery fell below \
                         {}%, and the notification could not be shown: {error}",
                        stop.session_id, stop.threshold_percent
                    );
                }
            }
        })
    }

    async fn notify(
        &self,
        connection: &zbus::Connection,
        stop: &LowBatteryStop,
    ) -> Result<u32, String> {
        let notification =
            low_battery_notification(self.locale, stop.threshold_percent, stop.percent);
        let call = async {
            // The interface has no properties worth caching, and not caching
            // means building the proxy sends nothing to a service that may not
            // exist.
            let proxy = NotificationsProxy::builder(connection)
                .destination(self.destination.as_str())?
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await?;
            let hints = HashMap::new();
            proxy
                .notify(
                    notification.application_name,
                    0,
                    LOW_BATTERY_ICON,
                    &notification.summary,
                    &notification.body,
                    &[],
                    &hints,
                    DEFAULT_EXPIRY,
                )
                .await
        };
        match tokio::time::timeout(self.timeout, call).await {
            Ok(Ok(id)) => Ok(id),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err("no answer in time".to_string()),
        }
    }
}

/// Whether a tray that will raise the notification is on the bus.
///
/// A bus that cannot answer counts as no tray, so the service notifies. If a
/// tray was there after all, that is a second notification for one stop, which
/// is the better way to be wrong than none.
pub async fn tray_is_running(connection: &zbus::Connection) -> bool {
    let answer: zbus::Result<bool> = async {
        let proxy = zbus::fdo::DBusProxy::builder(connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?;
        let name = zbus::names::BusName::try_from(TRAY_BUS_NAME)?;
        Ok(proxy.name_has_owner(name).await?)
    }
    .await;
    match answer {
        Ok(running) => running,
        Err(error) => {
            eprintln!(
                "better-awake-service: could not ask the bus whether a tray is running, so the \
                 service will raise the notification itself: {error}"
            );
            false
        }
    }
}
