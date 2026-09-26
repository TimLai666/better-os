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

use std::collections::HashMap;
use std::time::Duration;

use awake_ipc::StatusDocument;
use thiserror::Error;
use zbus::zvariant::Value;

use crate::client::{LowBatteryStop, ServiceEvent, service_event};
use crate::labels::{Labels, Locale};

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

/// A standard icon name from the freedesktop Icon Naming Specification, so
/// the notification has an icon without Better Awake shipping artwork for it.
const LOW_BATTERY_ICON: &str = "battery-caution";

/// Let the notification service apply its own default lifetime.
const DEFAULT_EXPIRY: i32 = -1;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum NotifyError {
    /// No notification service, or it refused the call.
    #[error("awake.tray.notify.error.call:{0}")]
    Call(String),
    #[error("awake.tray.notify.error.timed_out")]
    TimedOut,
}

/// What the notification says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    pub summary: String,
    pub body: String,
}

/// The wording for one low-battery stop, naming the threshold that was
/// crossed and the reading that crossed it.
pub fn low_battery_notification(labels: &Labels, stop: &LowBatteryStop) -> Notification {
    Notification {
        summary: labels
            .low_battery_stop_summary
            .replace("{threshold}", &stop.threshold_percent.to_string()),
        body: labels
            .low_battery_stop_body
            .replace("{percent}", &stop.percent.to_string()),
    }
}

pub struct DesktopNotifier {
    proxy: NotificationsProxy<'static>,
    labels: &'static Labels,
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
            labels: locale.labels(),
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
        let notification = low_battery_notification(self.labels, stop);
        let hints = HashMap::new();
        let call = self.proxy.notify(
            self.labels.application_name,
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

#[cfg(test)]
mod tests {
    use super::*;

    const STOP: LowBatteryStop = LowBatteryStop {
        session_id: 4,
        threshold_percent: 25,
        percent: 24,
    };

    #[test]
    fn the_notification_names_the_threshold_and_the_reading_in_both_locales() {
        for locale in [Locale::ZhTw, Locale::EnUs] {
            let notification = low_battery_notification(locale.labels(), &STOP);
            assert!(
                notification.summary.contains("25%"),
                "{}: {}",
                locale.tag(),
                notification.summary
            );
            assert!(
                notification.body.contains("24%"),
                "{}: {}",
                locale.tag(),
                notification.body
            );
            assert!(
                !notification.summary.contains('{') && !notification.body.contains('{'),
                "{}: a placeholder was left unfilled",
                locale.tag()
            );
        }
    }

    #[test]
    fn the_zh_tw_wording_is_translated_rather_than_copied() {
        assert_ne!(
            low_battery_notification(Locale::ZhTw.labels(), &STOP),
            low_battery_notification(Locale::EnUs.labels(), &STOP)
        );
    }
}
