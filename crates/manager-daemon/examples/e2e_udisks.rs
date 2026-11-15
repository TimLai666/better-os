//! A stand-in UDisks2 on the system bus for the container end-to-end check
//! (ticket 127): the container has no block device the daemon may touch, so
//! this answers `ResolveDevice` for two drives that exist nowhere, an
//! internal ext4 partition and a removable exFAT stick. The daemon under
//! test is the real one; only UDisks2's answer is made up.
//!
//! Runs until killed. Built only with the `test-support` feature.

use std::collections::HashMap;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};

pub const INTERNAL: &str = "e2e00001-0000-4000-8000-000000000001";
pub const STICK: &str = "E2E0-0002";

struct Block {
    uuid: &'static str,
    filesystem: &'static str,
    device: &'static str,
    drive: &'static str,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl Block {
    #[zbus(property, name = "IdUUID")]
    fn id_uuid(&self) -> String {
        self.uuid.into()
    }
    #[zbus(property, name = "IdType")]
    fn id_type(&self) -> String {
        self.filesystem.into()
    }
    #[zbus(property, name = "IdUsage")]
    fn id_usage(&self) -> String {
        "filesystem".into()
    }
    #[zbus(property, name = "IdLabel")]
    fn id_label(&self) -> String {
        String::new()
    }
    #[zbus(property, name = "Device")]
    fn device(&self) -> Vec<u8> {
        format!("{}\0", self.device).into_bytes()
    }
    #[zbus(property, name = "Symlinks")]
    fn symlinks(&self) -> Vec<Vec<u8>> {
        Vec::new()
    }
    #[zbus(property, name = "Drive")]
    fn drive(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(self.drive).unwrap()
    }
}

struct Drive {
    removable: bool,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl Drive {
    #[zbus(property, name = "Removable")]
    fn removable(&self) -> bool {
        self.removable
    }
    #[zbus(property, name = "MediaRemovable")]
    fn media_removable(&self) -> bool {
        false
    }
}

struct Manager;

#[zbus::interface(name = "org.freedesktop.UDisks2.Manager")]
impl Manager {
    fn resolve_device(
        &self,
        devspec: HashMap<String, OwnedValue>,
        _options: HashMap<String, OwnedValue>,
    ) -> Vec<OwnedObjectPath> {
        let uuid = devspec
            .get("uuid")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let path = match uuid.as_str() {
            INTERNAL => "/org/freedesktop/UDisks2/block_devices/vdb1",
            STICK => "/org/freedesktop/UDisks2/block_devices/sdz1",
            _ => return Vec::new(),
        };
        vec![OwnedObjectPath::try_from(path).unwrap()]
    }
}

#[tokio::main]
async fn main() {
    let _connection = zbus::connection::Builder::system()
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2/Manager", Manager)
        .unwrap()
        .serve_at(
            "/org/freedesktop/UDisks2/block_devices/vdb1",
            Block {
                uuid: INTERNAL,
                filesystem: "ext4",
                device: "/dev/vdb1",
                drive: "/org/freedesktop/UDisks2/drives/vdb",
            },
        )
        .unwrap()
        .serve_at(
            "/org/freedesktop/UDisks2/block_devices/sdz1",
            Block {
                uuid: STICK,
                filesystem: "exfat",
                device: "/dev/sdz1",
                drive: "/org/freedesktop/UDisks2/drives/sdz",
            },
        )
        .unwrap()
        .serve_at(
            "/org/freedesktop/UDisks2/drives/vdb",
            Drive { removable: false },
        )
        .unwrap()
        .serve_at(
            "/org/freedesktop/UDisks2/drives/sdz",
            Drive { removable: true },
        )
        .unwrap()
        .name("org.freedesktop.UDisks2")
        .unwrap()
        .build()
        .await
        .expect("the stand-in UDisks2 owns its name");
    eprintln!("stand-in UDisks2 ready");
    std::future::pending::<()>().await;
}
