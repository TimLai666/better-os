//! Read-only D-Bus surface for Better Monitor's privileged hardware data.

use std::sync::Arc;

use monitor_ipc::PROTOCOL_VERSION;
use zbus::message::Header;
use zbus::{fdo, interface};

use crate::DaemonError;
use crate::authorize::{Authorizer, READ_DMI_ACTION};
use crate::dmi::MemoryInventory;
use crate::service::refuse;

pub const BUS_NAME: &str = "org.betteros.Monitor1";
pub const OBJECT_PATH: &str = "/org/betteros/Monitor1";

pub struct MonitorService<A: Authorizer + 'static, I: MemoryInventory + 'static> {
    authorizer: A,
    inventory: Arc<I>,
}

impl<A: Authorizer + 'static, I: MemoryInventory + 'static> MonitorService<A, I> {
    pub fn new(authorizer: A, inventory: Arc<I>) -> Self {
        Self {
            authorizer,
            inventory,
        }
    }

    async fn authorize(&self, header: &Header<'_>) -> Result<(), fdo::Error> {
        match self.authorizer.check(header, READ_DMI_ACTION).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(refuse(DaemonError::Unauthorized)),
            Err(error) => Err(refuse(error)),
        }
    }
}

#[interface(name = "org.betteros.Monitor1")]
impl<A: Authorizer + 'static, I: MemoryInventory + 'static> MonitorService<A, I> {
    async fn read_memory_devices(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, fdo::Error> {
        self.authorize(&header).await?;
        let inventory = self.inventory.clone();
        let report = tokio::task::spawn_blocking(move || inventory.read())
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?
            .map_err(refuse)?;
        report.to_json().map_err(|error| refuse(error.into()))
    }

    #[zbus(property)]
    fn protocol_version(&self) -> u32 {
        PROTOCOL_VERSION
    }
}
