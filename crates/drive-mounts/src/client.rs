//! The windows' side: asking `manager-daemon` to change a drive's choice, and
//! asking systemd why a drive that is on did not mount. Both run on the
//! caller's thread; a window calls them off its UI thread.

use zbus::blocking::Connection;

pub const BUS_NAME: &str = "org.betteros.Manager1";
pub const OBJECT_PATH: &str = "/org/betteros/Manager1";
pub const INTERFACE: &str = "org.betteros.Drives1";

/// The daemon's error keys this module tells apart.
pub mod keys {
    pub const AUTH_CANCELLED: &str = "daemon.error.auth_cancelled";
    pub const UNAUTHORIZED: &str = "daemon.error.unauthorized";
    pub const DRIVE_ABSENT: &str = "daemon.error.drive_absent";
    pub const DRIVE_REFUSED: &str = "daemon.error.drive_refused";
    pub const DRIVE_BUSY: &str = "daemon.error.drive_busy";
    pub const MOUNT_TABLE_MALFORMED: &str = "daemon.error.mount_table_malformed";
    pub const MOUNT_FAILED: &str = "daemon.error.mount_failed";
    pub const MOUNT_TABLE_BUSY: &str = "daemon.error.mount_table_busy";
}

/// What a change came to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SetOutcome {
    /// Changed. For an internal drive turned on, where it is mounted (empty
    /// when the daemon left a mount someone else made in place).
    Done {
        mount_point: String,
    },
    /// The person dismissed the password prompt.
    Cancelled,
    NotAllowed,
    /// The daemon found no drive with that UUID.
    Absent,
    /// The daemon refused this drive: `system_drive`, `system_table`,
    /// `not_a_filesystem`.
    Refused(String),
    /// It is in use and was not unmounted; the entry stays.
    Busy(String),
    /// `/etc/fstab` or the rules file is malformed; nothing was written.
    Malformed(String),
    /// Another change held the mount table for longer than the daemon waits;
    /// nothing was written, and Retry asks again.
    TableBusy,
    /// The entry was written and the mount failed; Retry asks again.
    MountFailed(String),
    Failed(String),
}

pub fn classify_error(name: &str, message: &str) -> SetOutcome {
    let (key, detail) = message.split_once(':').unwrap_or((message, ""));
    let detail = detail.to_string();
    match key.trim() {
        keys::AUTH_CANCELLED => SetOutcome::Cancelled,
        keys::UNAUTHORIZED => SetOutcome::NotAllowed,
        keys::DRIVE_ABSENT => SetOutcome::Absent,
        keys::DRIVE_REFUSED => SetOutcome::Refused(detail),
        keys::DRIVE_BUSY => SetOutcome::Busy(detail),
        keys::MOUNT_TABLE_MALFORMED => SetOutcome::Malformed(detail),
        keys::MOUNT_FAILED => SetOutcome::MountFailed(detail),
        keys::MOUNT_TABLE_BUSY => SetOutcome::TableBusy,
        _ => SetOutcome::Failed(format!("{name}: {message}")),
    }
}

/// Turns a drive's choice on or off. polkit may ask for an administrator's
/// password, so this can take as long as the person takes.
pub fn set_mount_automatically(system: &Connection, uuid: &str, on: bool) -> SetOutcome {
    let reply = system.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "SetMountAutomatically",
        &(uuid, on),
    );
    match reply {
        Ok(message) => match message.body().deserialize::<String>() {
            Ok(mount_point) => SetOutcome::Done { mount_point },
            Err(error) => SetOutcome::Failed(error.to_string()),
        },
        Err(zbus::Error::MethodError(name, message, _)) => {
            classify_error(name.as_str(), message.as_deref().unwrap_or(""))
        }
        Err(error) => SetOutcome::Failed(error.to_string()),
    }
}

/// Why an internal drive that is on is not mounted, when systemd says: the
/// `Result` of its failed mount unit (`exit-code`, `timeout`, …). `None`
/// when the unit is not failed or systemd does not know it.
pub fn mount_failure(system: &Connection, uuid: &str) -> Option<String> {
    if !crate::ids::valid_uuid(uuid) {
        return None;
    }
    let unit = crate::ids::mount_unit(&crate::ids::mount_point(uuid));
    let path: zbus::zvariant::OwnedObjectPath = system
        .call_method(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            Some("org.freedesktop.systemd1.Manager"),
            "GetUnit",
            &(unit.as_str(),),
        )
        .ok()?
        .body()
        .deserialize()
        .ok()?;
    let property = |interface: &str, name: &str| -> Option<String> {
        let value: zbus::zvariant::OwnedValue = system
            .call_method(
                Some("org.freedesktop.systemd1"),
                path.as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(interface, name),
            )
            .ok()?
            .body()
            .deserialize()
            .ok()?;
        String::try_from(value).ok()
    };
    let state = property("org.freedesktop.systemd1.Unit", "ActiveState")?;
    if state != "failed" {
        return None;
    }
    Some(
        property("org.freedesktop.systemd1.Mount", "Result")
            .unwrap_or_else(|| "failed".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_daemon_key_is_its_outcome() {
        let denied = "org.freedesktop.DBus.Error.AccessDenied";
        let failed = "org.freedesktop.DBus.Error.Failed";
        assert_eq!(
            classify_error(denied, keys::AUTH_CANCELLED),
            SetOutcome::Cancelled
        );
        assert_eq!(
            classify_error(denied, keys::UNAUTHORIZED),
            SetOutcome::NotAllowed
        );
        assert_eq!(
            classify_error(failed, keys::DRIVE_ABSENT),
            SetOutcome::Absent
        );
        assert_eq!(
            classify_error(failed, "daemon.error.drive_refused:system_table"),
            SetOutcome::Refused("system_table".into())
        );
        assert_eq!(
            classify_error(failed, "daemon.error.drive_busy:target is busy"),
            SetOutcome::Busy("target is busy".into())
        );
        assert_eq!(
            classify_error(failed, "daemon.error.mount_table_malformed:line 3"),
            SetOutcome::Malformed("line 3".into())
        );
        assert_eq!(
            classify_error(failed, "daemon.error.mount_failed:wrong fs type"),
            SetOutcome::MountFailed("wrong fs type".into())
        );
        assert!(matches!(
            classify_error("org.freedesktop.DBus.Error.ServiceUnknown", "x"),
            SetOutcome::Failed(_)
        ));
    }
}
