//! The StatusNotifierItem side of the tray.
//!
//! # Why this is hand-written rather than `ksni`
//!
//! Issue #13 asks for `ksni` to be *evaluated*, not adopted, and defers the
//! crate choice to an ADR. Phase 1 therefore implements the two interfaces
//! directly on the zbus connection the tray already needs for the service and
//! for `org.kde.StatusNotifierWatcher`. That keeps the deferred decision open,
//! adds no dependency, and leaves the menu model — the part with the product
//! logic in it — independent of whichever crate the ADR settles on. See the
//! ticket for the recorded evaluation.
//!
//! # Never claiming visibility
//!
//! Registration is a request, not a result. This module asks the watcher to
//! register the item and then reads back the watcher's own list of registered
//! items. Anything short of finding ourselves there is reported as a specific
//! unavailability, never as "the icon is showing".

use awake_ipc::WireIndicator;

pub const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
pub const ITEM_PATH: &str = "/StatusNotifierItem";
pub const MENU_PATH: &str = "/MenuBar";

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
pub trait StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;
}

/// What is actually true about the tray icon right now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrayAvailability {
    /// The watcher lists this item. Only this value means the icon exists.
    Registered,
    /// No `org.kde.StatusNotifierWatcher` on the session bus. On stock GNOME
    /// this is the missing AppIndicator extension.
    NoWatcher,
    /// A watcher is present but no host is displaying items, so registering
    /// would succeed and show nothing.
    NoHost,
    /// The watcher refused the registration.
    RegistrationFailed(String),
    /// The watcher accepted the call and then did not list us.
    NotListed,
}

impl TrayAvailability {
    pub fn is_visible(&self) -> bool {
        matches!(self, TrayAvailability::Registered)
    }

    /// A stable key the full window and the log both use.
    pub fn as_key(&self) -> &'static str {
        match self {
            TrayAvailability::Registered => "awake.tray.registered",
            TrayAvailability::NoWatcher => "awake.tray.no_watcher",
            TrayAvailability::NoHost => "awake.tray.no_host",
            TrayAvailability::RegistrationFailed(_) => "awake.tray.registration_failed",
            TrayAvailability::NotListed => "awake.tray.not_listed",
        }
    }

    /// What a user can do about it. The desktop-entry fallback is the answer
    /// when there is no tray at all: Better Awake still installs a launcher and
    /// the full window still works, so the component is usable without a panel
    /// icon.
    pub fn remedy_key(&self) -> Option<&'static str> {
        match self {
            TrayAvailability::Registered => None,
            TrayAvailability::NoWatcher | TrayAvailability::NoHost => {
                Some("awake.tray.remedy.install_appindicator_support")
            }
            TrayAvailability::RegistrationFailed(_) | TrayAvailability::NotListed => {
                Some("awake.tray.remedy.use_desktop_entry")
            }
        }
    }
}

/// Registers the item and verifies the result against the watcher's own list.
pub async fn register_and_verify(
    connection: &zbus::Connection,
    unique_name: &str,
) -> TrayAvailability {
    let dbus = match zbus::fdo::DBusProxy::new(connection).await {
        Ok(dbus) => dbus,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    };
    match dbus
        .name_has_owner(match WATCHER_SERVICE.try_into() {
            Ok(name) => name,
            Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
        })
        .await
    {
        Ok(true) => {}
        Ok(false) => return TrayAvailability::NoWatcher,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    }

    let watcher = match StatusNotifierWatcherProxy::new(connection).await {
        Ok(watcher) => watcher,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    };

    if let Err(error) = watcher.register_status_notifier_item(unique_name).await {
        return TrayAvailability::RegistrationFailed(error.to_string());
    }

    match watcher.registered_status_notifier_items().await {
        // The watcher records either the bus name alone or the bus name and
        // path, depending on the implementation, so a prefix match is what
        // "we are in the list" means.
        Ok(items)
            if items
                .iter()
                .any(|item| item == unique_name || item.starts_with(unique_name)) => {}
        Ok(_) => return TrayAvailability::NotListed,
        Err(error) => return TrayAvailability::RegistrationFailed(error.to_string()),
    }

    match watcher.is_status_notifier_host_registered().await {
        Ok(true) => TrayAvailability::Registered,
        // Registered, but nothing is drawing items. Saying "visible" here would
        // be the claim Issue #13 forbids.
        Ok(false) => TrayAvailability::NoHost,
        Err(error) => TrayAvailability::RegistrationFailed(error.to_string()),
    }
}

/// The icon name for one state. Distinct names, not a shared icon recolored:
/// Issue #13 forbids leaning on color alone, and an icon theme is free to make
/// these look however it likes as long as they differ.
pub fn icon_name(indicator: WireIndicator) -> &'static str {
    match indicator {
        WireIndicator::Inactive => "better-awake-inactive",
        WireIndicator::ActiveManual => "better-awake-active",
        WireIndicator::ActiveTrigger => "better-awake-active-trigger",
        WireIndicator::PausedRules => "better-awake-paused",
        WireIndicator::AttentionRequired => "better-awake-attention",
        WireIndicator::Unavailable => "better-awake-unavailable",
    }
}

/// The `Status` property a StatusNotifierItem exposes. `NeedsAttention` is
/// reserved for the states that really need a person.
pub fn item_status(indicator: WireIndicator) -> &'static str {
    match indicator {
        WireIndicator::AttentionRequired | WireIndicator::Unavailable => "NeedsAttention",
        _ => "Active",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_verified_registration_counts_as_visible() {
        assert!(TrayAvailability::Registered.is_visible());
        for other in [
            TrayAvailability::NoWatcher,
            TrayAvailability::NoHost,
            TrayAvailability::NotListed,
            TrayAvailability::RegistrationFailed("boom".to_string()),
        ] {
            assert!(!other.is_visible(), "{other:?} must not claim visibility");
            assert!(other.remedy_key().is_some(), "{other:?} needs a remedy");
        }
    }

    #[test]
    fn every_icon_state_has_its_own_name() {
        let names = [
            WireIndicator::Inactive,
            WireIndicator::ActiveManual,
            WireIndicator::ActiveTrigger,
            WireIndicator::PausedRules,
            WireIndicator::AttentionRequired,
            WireIndicator::Unavailable,
        ]
        .map(icon_name);
        let mut unique = names.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn only_the_states_that_need_a_person_ask_for_attention() {
        assert_eq!(item_status(WireIndicator::ActiveManual), "Active");
        assert_eq!(item_status(WireIndicator::Inactive), "Active");
        assert_eq!(
            item_status(WireIndicator::AttentionRequired),
            "NeedsAttention"
        );
        assert_eq!(item_status(WireIndicator::Unavailable), "NeedsAttention");
    }
}
