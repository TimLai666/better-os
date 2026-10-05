//! The desktop notification the tray raises when low battery ends a session.
//!
//! The service decides the stop and says so on the bus; the tray is the
//! process in the person's desktop session, so it is the one that tells them.
//! It speaks `org.freedesktop.Notifications` directly on the connection it
//! already holds rather than through a notification crate, because the whole
//! surface it needs is one method.
//!
//! A notification is a courtesy, not a control. When there is no notification
//! service, or it refuses or never answers, the tray writes that to stderr and
//! carries on following the service: the session has already ended and been
//! recorded in History either way.
//!
//! Exactly one process raises it. A tray that will notify owns
//! [`TRAY_BUS_NAME`] on the session bus, and the service raises the
//! notification itself only when nobody owns that name.

use std::collections::HashMap;
use std::time::Duration;

use awake_ipc::StatusDocument;
use awake_ipc::notification::{
    DEFAULT_EXPIRY, LOW_BATTERY_ICON, TRAY_BUS_NAME, low_battery_notification,
};
use thiserror::Error;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::Value;

use crate::client::{LowBatteryStop, ServiceEvent, service_event};
use crate::labels::Locale;

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

/// How long the tray waits for the notification service before giving up.
/// The call is made from the loop that follows the service, so a notification
/// service that never answers must not be able to stop the menu updating.
pub const NOTIFY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum NotifyError {
    /// No notification service, or it refused the call.
    #[error("awake.tray.notify.error.call:{0}")]
    Call(String),
    #[error("awake.tray.notify.error.timed_out")]
    TimedOut,
}

pub struct DesktopNotifier {
    proxy: NotificationsProxy<'static>,
    locale: Locale,
    timeout: Duration,
}

impl DesktopNotifier {
    /// A notifier for the desktop's own notification service on `connection`.
    pub async fn connect(
        connection: &zbus::Connection,
        locale: Locale,
    ) -> Result<Self, NotifyError> {
        Self::build(NotificationsProxy::builder(connection), locale).await
    }

    /// A notifier for a notification service published under another name.
    /// Used by the private-bus tests, so no test can reach a real one.
    pub async fn with_destination(
        connection: &zbus::Connection,
        destination: String,
        locale: Locale,
    ) -> Result<Self, NotifyError> {
        let builder = NotificationsProxy::builder(connection)
            .destination(destination)
            .map_err(|error| NotifyError::Call(error.to_string()))?;
        Self::build(builder, locale).await
    }

    async fn build(
        builder: zbus::proxy::Builder<'static, NotificationsProxy<'static>>,
        locale: Locale,
    ) -> Result<Self, NotifyError> {
        // The interface has no properties worth caching, and not caching means
        // building the proxy sends nothing to a service that may not exist.
        let proxy = builder
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|error| NotifyError::Call(error.to_string()))?;
        Ok(Self {
            proxy,
            locale,
            timeout: NOTIFY_TIMEOUT,
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Raises the notification for one low-battery stop. Returns the id the
    /// notification service gave it.
    pub async fn low_battery_stop(&self, stop: &LowBatteryStop) -> Result<u32, NotifyError> {
        let notification =
            low_battery_notification(self.locale, stop.threshold_percent, stop.percent);
        let hints = HashMap::new();
        let call = self.proxy.notify(
            notification.application_name,
            0,
            LOW_BATTERY_ICON,
            &notification.summary,
            &notification.body,
            &[],
            &hints,
            DEFAULT_EXPIRY,
        );
        match tokio::time::timeout(self.timeout, call).await {
            Ok(Ok(id)) => Ok(id),
            Ok(Err(error)) => Err(NotifyError::Call(error.to_string())),
            Err(_) => Err(NotifyError::TimedOut),
        }
    }
}

/// Takes over the low-battery notification from the service, by owning
/// [`TRAY_BUS_NAME`] on `connection`. Returns the notifier this tray will
/// notify with, or `None` when it will not notify.
///
/// Call it only once the tray is receiving the service's events on
/// `connection`: from the moment the name is owned the service stops notifying,
/// so a tray that owned it before it could hear a stop would let one pass with
/// no notification at all. The name goes when the connection does, so a tray
/// that quits or crashes hands the notification back without saying so.
///
/// A tray with no notifier does not take the name, and neither does a second
/// tray while a first one holds it; two trays notifying would be two
/// notifications for one stop. The name is asked for without replacing or
/// queueing, so the first tray keeps it until it goes.
pub async fn claim_notifications(
    connection: &zbus::Connection,
    notifier: Option<DesktopNotifier>,
) -> Option<DesktopNotifier> {
    let notifier = notifier?;
    match connection
        .request_name_with_flags(TRAY_BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .await
    {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Some(notifier),
        Ok(reply) => {
            eprintln!(
                "better-awake-tray: another tray already raises the low-battery notification \
                 ({reply:?}), so this one will not"
            );
            None
        }
        Err(error) => {
            eprintln!(
                "better-awake-tray: another tray already raises the low-battery notification, \
                 or the name could not be taken ({error}), so this one will not"
            );
            None
        }
    }
}

/// Handles one event the service pushed.
///
/// Hands back the status when the event carries one, for the menu to redraw
/// from, and raises the notification when it is a low-battery stop. Nothing
/// that goes wrong here is returned as an error: the caller is the loop that
/// keeps the menu following the service, and one bad event must not end it.
pub async fn handle_event(
    document: &str,
    notifier: Option<&DesktopNotifier>,
) -> Option<StatusDocument> {
    match service_event(document) {
        Ok(ServiceEvent::Status(status)) => Some(*status),
        Ok(ServiceEvent::LowBatteryStop(stop)) => {
            match notifier {
                Some(notifier) => {
                    if let Err(error) = notifier.low_battery_stop(&stop).await {
                        eprintln!(
                            "better-awake-tray: session {} ended because the battery fell below \
                             {}%, and the notification could not be shown: {error}",
                            stop.session_id, stop.threshold_percent
                        );
                    }
                }
                None => eprintln!(
                    "better-awake-tray: session {} ended because the battery fell below {}%, \
                     and there is no notification service to show it",
                    stop.session_id, stop.threshold_percent
                ),
            }
            None
        }
        Ok(ServiceEvent::Other) => None,
        Err(error) => {
            eprintln!("better-awake-tray: ignoring an event the service sent: {error}");
            None
        }
    }
}
