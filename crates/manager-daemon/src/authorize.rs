//! Deciding whether a caller may change this machine.
//!
//! The daemon does not implement its own policy. It asks polkit, which is what
//! already decides this question for every other privileged desktop operation
//! on Zorin OS and Ubuntu, and which an administrator can inspect and override.

use std::collections::HashMap;

use zbus::message::Header;
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

use crate::DaemonError;

/// The one action that gates every mutating method.
pub const APPLY_ACTION: &str = "org.betteros.manager.apply-transaction";

pub trait Authorizer: Send + Sync {
    /// Whether this caller may apply a transaction. An error means the question
    /// could not be answered, which is treated as a refusal by callers.
    fn check(
        &self,
        header: &Header<'_>,
    ) -> impl std::future::Future<Output = Result<bool, DaemonError>> + Send;
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
    async fn check(&self, header: &Header<'_>) -> Result<bool, DaemonError> {
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
                APPLY_ACTION,
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
}

/// A fixed answer, for tests. Nothing in the shipped binary constructs one.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeAuthorizer(pub bool);

#[cfg(any(test, feature = "test-support"))]
impl Authorizer for FakeAuthorizer {
    async fn check(&self, _header: &Header<'_>) -> Result<bool, DaemonError> {
        Ok(self.0)
    }
}
