use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use manager_daemon::authorize::FakeAuthorizer;
use manager_daemon::dmi::FixedMemoryInventory;
use manager_daemon::monitor_service::{MonitorService, OBJECT_PATH};
use monitor_ipc::{MemoryDevice, MemoryReport, PROTOCOL_VERSION};

struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        use std::io::{BufRead, BufReader};
        let mut address = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        let address = address.trim().to_string();
        (!address.is_empty()).then_some(Self { child, address })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn report() -> MemoryReport {
    MemoryReport {
        protocol_version: PROTOCOL_VERSION,
        smbios_major: 3,
        smbios_minor: 6,
        devices: vec![MemoryDevice {
            locator: "DIMM_A0".to_string(),
            bank: Some("BANK 0".to_string()),
            installed: true,
            size_bytes: Some(16 * 1024 * 1024 * 1024),
            speed_mt_s: Some(5600),
            configured_speed_mt_s: Some(5200),
            form_factor: "SO-DIMM".to_string(),
            memory_type: "DDR5".to_string(),
            type_detail: vec!["Synchronous".to_string()],
            manufacturer: Some("Example".to_string()),
            part_number: Some("ABC-123".to_string()),
            configured_voltage_mv: Some(1100),
        }],
    }
}

async fn serve(bus: &PrivateBus, authorized: bool) -> zbus::Result<zbus::Connection> {
    let address: zbus::Address = bus.address.parse()?;
    zbus::connection::Builder::address(address)?
        .name("org.betteros.Monitor1Test")?
        .serve_at(
            OBJECT_PATH,
            MonitorService::new(
                FakeAuthorizer(authorized),
                Arc::new(FixedMemoryInventory::new(report())),
            ),
        )?
        .build()
        .await
}

async fn client(bus: &PrivateBus) -> zbus::Result<zbus::Connection> {
    let address: zbus::Address = bus.address.parse()?;
    zbus::connection::Builder::address(address)?.build().await
}

macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None => {
                eprintln!("skipping: dbus-daemon is unavailable");
                return;
            }
        }
    };
}

#[tokio::test]
async fn authorized_call_returns_bounded_structured_report() {
    let bus = bus_or_skip!();
    let _service = serve(&bus, true).await.unwrap();
    let client = client(&bus).await.unwrap();
    let reply = client
        .call_method(
            Some("org.betteros.Monitor1Test"),
            OBJECT_PATH,
            Some("org.betteros.Monitor1"),
            "ReadMemoryDevices",
            &(),
        )
        .await
        .unwrap();
    let document: String = reply.body().deserialize().unwrap();
    assert_eq!(MemoryReport::from_json(&document).unwrap(), report());
    assert!(!document.to_ascii_lowercase().contains("serial"));
    assert!(!document.to_ascii_lowercase().contains("asset_tag"));
}

#[tokio::test]
async fn unauthorized_call_is_refused_before_inventory_is_returned() {
    let bus = bus_or_skip!();
    let _service = serve(&bus, false).await.unwrap();
    let client = client(&bus).await.unwrap();
    let error = client
        .call_method(
            Some("org.betteros.Monitor1Test"),
            OBJECT_PATH,
            Some("org.betteros.Monitor1"),
            "ReadMemoryDevices",
            &(),
        )
        .await
        .expect_err("unauthorized callers must be refused");
    assert!(error.to_string().contains("daemon.error.unauthorized"));
}
