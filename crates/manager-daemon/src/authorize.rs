//! Deciding whether a caller may change this machine.
//!
//! The daemon does not implement its own policy. It asks polkit, which is what
//! already decides this question for every other privileged desktop operation
//! on Zorin OS and Ubuntu, and which an administrator can inspect and override.

use std::collections::HashMap;

use zbus::message::Header;
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

use crate::DaemonError;

/// The action that gates staging, applying, and cancelling a transaction.
pub const APPLY_ACTION: &str = "org.betteros.manager.apply-transaction";

/// The action that gates `dpkg --configure -a`. It is separate from
/// [`APPLY_ACTION`] because a repair acts on every unfinished package on the
/// machine, not only on `better-*` ones; see ADR 0014.
pub const REPAIR_ACTION: &str = "org.betteros.manager.repair-package-system";

/// The action that gates `org.betteros.Hardware1.SetChargeLimit` (ADR 0017,
/// ticket 86).
pub const CHARGE_LIMIT_ACTION: &str = "org.betteros.manager.set-charge-limit";

/// The action that gates `org.betteros.Hardware1.SetFanMode`.
pub const FAN_MODE_ACTION: &str = "org.betteros.manager.set-fan-mode";

/// The action that gates `org.betteros.Hardware1.KeyboardLighting` and
/// `SetKeyboardLighting` (ADR 0034, ticket 136). The active local session
/// is allowed without a prompt, as UPower allows its keyboard brightness.
pub const KEYBOARD_LIGHTING_ACTION: &str = "org.betteros.manager.keyboard-lighting";

/// The action that gates `org.betteros.Drives1.SetMountAutomatically`
/// (ADR 0032, ticket 127).
pub const DRIVE_MOUNTING_ACTION: &str = "org.betteros.manager.drive-mounting";

/// The action that gates `org.betteros.BootWatch1.SetEnabled` (ADR 0033,
/// ticket 140).
pub const LOGIN_SCREEN_WATCH_ACTION: &str = "org.betteros.manager.login-screen-watch";

/// What polkit answered, with a cancelled prompt told apart from a refusal
/// so the window can say "authentication wasn't completed" rather than "not
/// allowed".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authorization {
    Granted,
    Denied,
    /// The person dismissed the authentication dialog.
    Dismissed,
}

pub trait Authorizer: Send + Sync {
    /// Whether this caller may perform `action`. An error means the question
    /// could not be answered, which is treated as a refusal by callers.
    fn check(
        &self,
        header: &Header<'_>,
        action: &'static str,
    ) -> impl std::future::Future<Output = Result<bool, DaemonError>> + Send;

    /// [`Authorizer::check`] with a dismissed prompt told apart. An
    /// authorizer that cannot tell says only granted or denied.
    fn decide(
        &self,
        header: &Header<'_>,
        action: &'static str,
    ) -> impl std::future::Future<Output = Result<Authorization, DaemonError>> + Send {
        let answer = self.check(header, action);
        async move {
            Ok(if answer.await? {
                Authorization::Granted
            } else {
                Authorization::Denied
            })
        }
    }
}

/// Asks the real polkit authority.
pub struct PolkitAuthorizer {
    connection: zbus::Connection,
}

impl PolkitAuthorizer {
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

impl Authorizer for PolkitAuthorizer {
    async fn check(&self, header: &Header<'_>, action: &'static str) -> Result<bool, DaemonError> {
        let authority = AuthorityProxy::new(&self.connection)
            .await
            .map_err(|error| DaemonError::Protocol(error.to_string()))?;

        // Identify the caller by its bus name and let polkitd resolve the
        // process itself. Passing a pid we looked up would race process exit
        // and reuse.
        let subject = Subject::new_for_message_header(header)
            .map_err(|error| DaemonError::Protocol(error.to_string()))?;

        let result = authority
            .check_authorization(
                &subject,
                action,
                &HashMap::new(),
                // The caller is a desktop application acting for a person, so
                // polkit is allowed to put up its authentication dialog.
                CheckAuthorizationFlags::AllowUserInteraction.into(),
                "",
            )
            .await
            .map_err(|error| DaemonError::Protocol(error.to_string()))?;

        Ok(result.is_authorized)
    }

    async fn decide(
        &self,
        header: &Header<'_>,
        action: &'static str,
    ) -> Result<Authorization, DaemonError> {
        let authority = AuthorityProxy::new(&self.connection)
            .await
            .map_err(|error| DaemonError::Protocol(error.to_string()))?;
        let subject = Subject::new_for_message_header(header)
            .map_err(|error| DaemonError::Protocol(error.to_string()))?;
        let result = authority
            .check_authorization(
                &subject,
                action,
                &HashMap::new(),
                CheckAuthorizationFlags::AllowUserInteraction.into(),
                "",
            )
            .await
            .map_err(|error| DaemonError::Protocol(error.to_string()))?;
        Ok(if result.is_authorized {
            Authorization::Granted
        } else if result
            .details
            .get("polkit.dismissed")
            .is_some_and(|value| value == "true")
        {
            // polkitd sets this when the agent's dialog was cancelled.
            Authorization::Dismissed
        } else {
            Authorization::Denied
        })
    }
}

/// A fixed answer for every action, for tests. Nothing in the shipped binary
/// constructs one.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeAuthorizer(pub bool);

#[cfg(any(test, feature = "test-support"))]
impl Authorizer for FakeAuthorizer {
    async fn check(
        &self,
        _header: &Header<'_>,
        _action: &'static str,
    ) -> Result<bool, DaemonError> {
        Ok(self.0)
    }
}

/// Grants exactly one action and refuses every other, for tests that prove a
/// method asks for its own action rather than another one.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeActionAuthorizer(pub &'static str);

#[cfg(any(test, feature = "test-support"))]
impl Authorizer for FakeActionAuthorizer {
    async fn check(&self, _header: &Header<'_>, action: &'static str) -> Result<bool, DaemonError> {
        Ok(action == self.0)
    }
}

/// Answers every action as a cancelled prompt, for tests of the
/// "authentication wasn't completed" path.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeDismissingAuthorizer;

#[cfg(any(test, feature = "test-support"))]
impl Authorizer for FakeDismissingAuthorizer {
    async fn check(
        &self,
        _header: &Header<'_>,
        _action: &'static str,
    ) -> Result<bool, DaemonError> {
        Ok(false)
    }

    async fn decide(
        &self,
        _header: &Header<'_>,
        _action: &'static str,
    ) -> Result<Authorization, DaemonError> {
        Ok(Authorization::Dismissed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = include_str!("../../../packaging/daemon/org.betteros.manager.policy");

    /// The action block for one id, from its opening tag to its closing one.
    fn action_block(id: &str) -> &'static str {
        let start = POLICY
            .find(&format!("<action id=\"{id}\">"))
            .unwrap_or_else(|| panic!("the shipped policy does not declare {id}"));
        let end = POLICY[start..]
            .find("</action>")
            .expect("an action block is closed");
        &POLICY[start..start + end]
    }

    /// The daemon asks polkit by these ids, and the policy the daemon package
    /// ships is what makes polkit know them. A constant renamed on one side
    /// only would make every request fail as unauthorized.
    #[test]
    fn every_action_the_daemon_asks_for_is_declared_in_the_shipped_policy() {
        for id in [
            APPLY_ACTION,
            REPAIR_ACTION,
            CHARGE_LIMIT_ACTION,
            FAN_MODE_ACTION,
            DRIVE_MOUNTING_ACTION,
            LOGIN_SCREEN_WATCH_ACTION,
        ] {
            action_block(id);
        }
        assert_ne!(APPLY_ACTION, REPAIR_ACTION);
    }

    /// ADR 0014: a repair runs maintainer scripts of packages Better OS did not
    /// install, so every session authenticates every time and polkit keeps no
    /// authorization between repairs.
    #[test]
    fn a_repair_asks_for_an_administrator_every_time() {
        let block = action_block(REPAIR_ACTION);
        for default in ["allow_any", "allow_inactive", "allow_active"] {
            assert!(
                block.contains(&format!("<{default}>auth_admin</{default}>")),
                "{default} must be auth_admin in {block}"
            );
        }
        assert!(!block.contains("auth_admin_keep"), "{block}");
    }

    /// ADR 0017: each hardware control has its own action, an administrator
    /// authenticates, polkit keeps the answer for a while in an active
    /// session, and both languages describe it.
    #[test]
    fn each_hardware_action_asks_an_administrator_and_says_so_in_both_languages() {
        for id in [
            CHARGE_LIMIT_ACTION,
            FAN_MODE_ACTION,
            LOGIN_SCREEN_WATCH_ACTION,
        ] {
            let block = action_block(id);
            assert!(
                block.contains("<allow_active>auth_admin_keep</allow_active>"),
                "{block}"
            );
            assert!(
                block.contains("<allow_any>auth_admin</allow_any>"),
                "{block}"
            );
            assert!(
                block.contains("<allow_inactive>auth_admin</allow_inactive>"),
                "{block}"
            );
            assert_eq!(block.matches("<description").count(), 2, "{block}");
            assert_eq!(block.matches("<message").count(), 2, "{block}");
            assert!(
                block.contains("<description xml:lang=\"zh_TW\">"),
                "{block}"
            );
            assert!(block.contains("<message xml:lang=\"zh_TW\">"), "{block}");
        }
    }
}
