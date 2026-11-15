//! What a change through the daemon came back with, in the states the page
//! draws (spec section 2).

/// The error keys `org.betteros.Hardware1` answers with. The daemon owns the
/// keys, the window owns the words.
pub mod keys {
    /// The person cancelled the polkit prompt or did not complete it.
    pub const AUTH_CANCELLED: &str = "daemon.error.auth_cancelled";
    /// polkit says this account may not do this.
    pub const UNAUTHORIZED: &str = "daemon.error.unauthorized";
    /// The argument is outside the closed set; nothing was touched.
    pub const INVALID_ARGUMENT: &str = "daemon.error.invalid_argument";
    /// The daemon found no such hardware.
    pub const HARDWARE_ABSENT: &str = "daemon.error.hardware_absent";
    /// The write or the read back failed; the message carries the errno.
    pub const WRITE_FAILED: &str = "daemon.error.write_failed";
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WriteOutcome {
    Done,
    /// Written, and it reads back as this instead.
    ReadBack(String),
    /// "Nothing changed: authentication wasn't completed."
    Cancelled,
    /// "This account isn't allowed to change this setting."
    NotAllowed,
    /// "That didn't change." with the technical chain as details.
    Failed {
        details: String,
    },
}

/// Classifies a D-Bus error from the daemon by the key at the start of its
/// message (`daemon.error.write_failed:EROFS ...`).
pub fn classify_error(error_name: &str, message: &str) -> WriteOutcome {
    let key = message.split(':').next().unwrap_or("").trim();
    match key {
        keys::AUTH_CANCELLED => WriteOutcome::Cancelled,
        keys::UNAUTHORIZED => WriteOutcome::NotAllowed,
        _ => WriteOutcome::Failed {
            details: format!("{error_name}: {message}"),
        },
    }
}

/// A reply that names the value the daemon read back.
pub fn classify_reply(asked: &str, read_back: &str) -> WriteOutcome {
    if asked == read_back {
        WriteOutcome::Done
    } else {
        WriteOutcome::ReadBack(read_back.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_daemon_key_becomes_its_state() {
        assert_eq!(
            classify_error(
                "org.freedesktop.DBus.Error.AccessDenied",
                keys::AUTH_CANCELLED
            ),
            WriteOutcome::Cancelled
        );
        assert_eq!(
            classify_error(
                "org.freedesktop.DBus.Error.AccessDenied",
                keys::UNAUTHORIZED
            ),
            WriteOutcome::NotAllowed
        );
        let failed = classify_error(
            "org.freedesktop.DBus.Error.Failed",
            "daemon.error.write_failed:Read-only file system (os error 30)",
        );
        assert!(
            matches!(failed, WriteOutcome::Failed { details } if details.contains("os error 30"))
        );
        assert!(matches!(
            classify_error("org.freedesktop.DBus.Error.ServiceUnknown", "no daemon"),
            WriteOutcome::Failed { .. }
        ));
    }

    #[test]
    fn a_different_read_back_is_its_own_state() {
        assert_eq!(classify_reply("80", "80"), WriteOutcome::Done);
        assert_eq!(
            classify_reply("80", "100"),
            WriteOutcome::ReadBack("100".into())
        );
    }
}
