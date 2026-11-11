//! The low-battery notification, as the tray and the service both word it.
//!
//! Exactly one of them raises it for each stop. The tray does when it is
//! running, because it is the process in the person's desktop session; the
//! service does when no tray is there. Both need the same wording, the same
//! locale rule, and the bus name that tells the service a tray is there, so they
//! live here rather than in either process. Nothing here talks to a bus.

/// The name a running tray owns on the session bus, so the service can tell
/// whether a tray is there to raise the notification. The tray takes it only
/// once it is following the service's events.
pub const TRAY_BUS_NAME: &str = "org.betteros.AwakeTray1";

/// A standard icon name from the freedesktop Icon Naming Specification, so the
/// notification has an icon without Better Awake shipping artwork for it.
pub const LOW_BATTERY_ICON: &str = "battery-caution";

/// Lets the notification service apply its own default lifetime.
pub const DEFAULT_EXPIRY: i32 = -1;

/// The two locales Phase 1 ships.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    ZhTw,
    EnUs,
}

impl Locale {
    /// Reads the user's locale the way every other POSIX program does.
    /// Anything that is not Traditional Chinese falls back to `en-US`, which is
    /// a fallback, not a guess about the user.
    pub fn from_environment() -> Self {
        let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .unwrap_or_default();
        Self::from_tag(&value)
    }

    pub fn from_tag(tag: &str) -> Self {
        let tag = tag.replace('-', "_").to_ascii_lowercase();
        if tag.starts_with("zh_tw") || tag.starts_with("zh_hant") || tag.starts_with("zh_hk") {
            Locale::ZhTw
        } else {
            Locale::EnUs
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Locale::ZhTw => "zh-TW",
            Locale::EnUs => "en-US",
        }
    }

    pub fn notification_wording(self) -> &'static NotificationWording {
        match self {
            Locale::ZhTw => &ZH_TW,
            Locale::EnUs => &EN_US,
        }
    }
}

/// The strings a notification is built from.
#[derive(Clone, Copy, Debug)]
pub struct NotificationWording {
    /// The application name the notification is shown under. The tray's own
    /// name in its menu is this string too.
    pub application_name: &'static str,
    /// `{threshold}` is the stop threshold that was crossed.
    pub low_battery_stop_summary: &'static str,
    /// `{percent}` is the reading that crossed it.
    pub low_battery_stop_body: &'static str,
}

pub const ZH_TW: NotificationWording = NotificationWording {
    application_name: "保持清醒",
    low_battery_stop_summary: "電量低於 {threshold}%，已停止保持清醒",
    low_battery_stop_body: "工作階段在電量剩 {percent}% 時結束。",
};

pub const EN_US: NotificationWording = NotificationWording {
    application_name: "Better Awake",
    low_battery_stop_summary: "Stopped keeping awake: battery below {threshold}%",
    low_battery_stop_body: "The session ended at {percent}% battery.",
};

/// What one notification says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    pub application_name: &'static str,
    pub summary: String,
    pub body: String,
}

/// The wording for one low-battery stop, naming the threshold that was crossed
/// and the reading that crossed it.
pub fn low_battery_notification(
    locale: Locale,
    threshold_percent: u8,
    percent: u8,
) -> Notification {
    let wording = locale.notification_wording();
    Notification {
        application_name: wording.application_name,
        summary: wording
            .low_battery_stop_summary
            .replace("{threshold}", &threshold_percent.to_string()),
        body: wording
            .low_battery_stop_body
            .replace("{percent}", &percent.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traditional_chinese_locale_is_recognized_in_every_shape_it_arrives_in() {
        for tag in [
            "zh_TW.UTF-8",
            "zh-TW",
            "zh_Hant",
            "zh_HK.UTF-8",
            "ZH_tw.utf8",
        ] {
            assert_eq!(Locale::from_tag(tag), Locale::ZhTw, "{tag}");
        }
    }

    #[test]
    fn anything_else_falls_back_to_english() {
        for tag in ["en_US.UTF-8", "zh_CN.UTF-8", "", "C", "de_DE"] {
            assert_eq!(Locale::from_tag(tag), Locale::EnUs, "{tag}");
        }
    }

    #[test]
    fn the_notification_names_the_threshold_and_the_reading_in_both_locales() {
        for locale in [Locale::ZhTw, Locale::EnUs] {
            let notification = low_battery_notification(locale, 25, 24);
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
            low_battery_notification(Locale::ZhTw, 25, 24),
            low_battery_notification(Locale::EnUs, 25, 24)
        );
        assert_eq!(ZH_TW.application_name, "保持清醒");
    }
}
