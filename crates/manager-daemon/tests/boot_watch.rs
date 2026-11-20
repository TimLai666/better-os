//! `org.betteros.BootWatch1` over a private bus, against a temporary records
//! directory and the fake authorizers (ticket 140). Nothing here touches the
//! machine.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use manager_daemon::authorize::{
    Authorizer, CHARGE_LIMIT_ACTION, FakeActionAuthorizer, FakeAuthorizer,
    FakeDismissingAuthorizer, LOGIN_SCREEN_WATCH_ACTION,
};
use manager_daemon::boot_watch::BootWatchService;
use manager_daemon::service::{CallGate, OBJECT_PATH};
use manager_ipc::boot_watch::{Records, Setting};

const NAME: &str = "org.betteros.Manager1BootWatchTest";

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
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

macro_rules! bus_or_skip {
    () => {
        test_prerequisites::require!(
            "dbus-daemon",
            PrivateBus::start().ok_or("a private dbus-daemon did not start")
        )
    };
}

async fn serve<A: Authorizer + 'static>(
    bus: &PrivateBus,
    records: &Records,
    authorizer: A,
    name: &str,
) -> zbus::Connection {
    let address: zbus::Address = bus.address.parse().unwrap();
    let service = BootWatchService::new(authorizer, records.clone(), Arc::new(CallGate::default()));
    zbus::connection::Builder::address(address)
        .unwrap()
        .serve_at(OBJECT_PATH, service)
        .unwrap()
        .name(name.to_string())
        .unwrap()
        .build()
        .await
        .unwrap()
}

async fn set_enabled(bus: &PrivateBus, name: &str, enabled: bool) -> Result<bool, String> {
    let address: zbus::Address = bus.address.parse().unwrap();
    let client = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    match client
        .call_method(
            Some(name),
            OBJECT_PATH,
            Some("org.betteros.BootWatch1"),
            "SetEnabled",
            &(enabled,),
        )
        .await
    {
        Ok(reply) => Ok(reply.body().deserialize::<bool>().unwrap()),
        Err(zbus::Error::MethodError(name, message, _)) => {
            Err(format!("{} {}", name.as_str(), message.unwrap_or_default()))
        }
        Err(other) => Err(other.to_string()),
    }
}

#[tokio::test]
async fn an_administrator_turns_the_watch_off_and_on_and_the_file_says_so() {
    let bus = bus_or_skip!();
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().join("boot-watch"));
    let _server = serve(
        &bus,
        &records,
        FakeActionAuthorizer(LOGIN_SCREEN_WATCH_ACTION),
        NAME,
    )
    .await;
    assert_eq!(records.setting(), Setting::OnByDefault);
    assert_eq!(set_enabled(&bus, NAME, false).await, Ok(false));
    assert_eq!(records.setting(), Setting::Off);
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(records.settings_path())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);
    assert_eq!(set_enabled(&bus, NAME, true).await, Ok(true));
    assert_eq!(records.setting(), Setting::On);
}

#[tokio::test]
async fn a_refused_or_dismissed_prompt_or_another_action_writes_nothing() {
    let bus = bus_or_skip!();
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().join("boot-watch"));
    {
        let _server = serve(
            &bus,
            &records,
            FakeAuthorizer(false),
            "org.betteros.Manager1BootWatchRefused",
        )
        .await;
        let error = set_enabled(&bus, "org.betteros.Manager1BootWatchRefused", false)
            .await
            .unwrap_err();
        assert!(error.contains("daemon.error.unauthorized"), "{error}");
    }
    {
        let _server = serve(
            &bus,
            &records,
            FakeDismissingAuthorizer,
            "org.betteros.Manager1BootWatchDismissed",
        )
        .await;
        let error = set_enabled(&bus, "org.betteros.Manager1BootWatchDismissed", false)
            .await
            .unwrap_err();
        assert!(error.contains("daemon.error.auth_cancelled"), "{error}");
    }
    {
        // Allowed to set the charge limit is not allowed to turn this off.
        let _server = serve(
            &bus,
            &records,
            FakeActionAuthorizer(CHARGE_LIMIT_ACTION),
            "org.betteros.Manager1BootWatchOther",
        )
        .await;
        let error = set_enabled(&bus, "org.betteros.Manager1BootWatchOther", false)
            .await
            .unwrap_err();
        assert!(error.contains("daemon.error.unauthorized"), "{error}");
    }
    assert_eq!(records.setting(), Setting::OnByDefault);
    assert!(!records.settings_path().exists());
}
