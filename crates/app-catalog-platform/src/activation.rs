//! D-Bus activation for `DBusActivatable` entries.
//!
//! The bus starts the application from its own service file, so nothing here
//! needs an executable path. Behind a feature flag so a consumer that only
//! reads the catalog does not link a D-Bus stack, the same way
//! `manager-platform` gates its privileged client.

use std::collections::HashMap;

use app_catalog_core::{DBusActivation, DBusMethod};
use zbus::blocking::Connection;
use zbus::zvariant::Value;

use crate::PlatformError;
use crate::launch::DesktopActivator;

/// The interface every activatable application implements.
const APPLICATION_INTERFACE: &str = "org.freedesktop.Application";

/// Activates applications over the session bus.
pub struct SessionBusActivator {
    connection: Connection,
}

impl SessionBusActivator {
    pub fn connect() -> Result<Self, PlatformError> {
        let connection = Connection::session()
            .map_err(|error| PlatformError::ActivationFailed(error.to_string()))?;
        Ok(Self { connection })
    }

    pub fn with_connection(connection: Connection) -> Self {
        Self { connection }
    }
}

impl DesktopActivator for SessionBusActivator {
    fn activate(&self, activation: &DBusActivation) -> Result<(), PlatformError> {
        let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        let proxy = zbus::blocking::Proxy::new(
            &self.connection,
            activation.service.as_str(),
            activation.object_path.as_str(),
            APPLICATION_INTERFACE,
        )
        .map_err(|error| PlatformError::ActivationFailed(error.to_string()))?;
        let result = match &activation.method {
            DBusMethod::Activate => proxy.call::<_, _, ()>("Activate", &(platform_data,)),
            DBusMethod::Open => {
                let uris: Vec<&str> = activation.uris.iter().map(String::as_str).collect();
                proxy.call::<_, _, ()>("Open", &(uris, platform_data))
            }
            DBusMethod::ActivateAction(action) => {
                let parameters: Vec<Value<'_>> = Vec::new();
                proxy.call::<_, _, ()>(
                    "ActivateAction",
                    &(action.as_str(), parameters, platform_data),
                )
            }
        };
        result.map_err(|error| PlatformError::ActivationFailed(error.to_string()))
    }
}
