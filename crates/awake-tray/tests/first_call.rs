//! The taskbar item answers the first call made the instant it is published.
//!
//! zbus starts a connection's object server lazily, in a task that subscribes
//! to method calls some time after it is spawned. An item published that way
//! can be registered with the panel before the subscription exists, and the
//! panel's first call (its layout or its icon) is then never answered: the
//! hang ticket 102 traced and ticket 107 removes everywhere.
//!
//! The item is reached by its unique name, so there is no name to activate.
//! The real start-up path, [`awake_tray::serve`] as `main` calls it, is run
//! [`STARTS`] times on a private `dbus-daemon`, and the moment it returns,
//! which is when `main` registers with the panel, a client calls the item.
//! Every one of those calls must be answered.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use awake_service::backend::{FakeInhibitorBackend, FixedClock};
use awake_service::{AwakeDbusService, AwakeEngine};
use awake_tray::controller::TrayController;
use awake_tray::labels::Locale;
use awake_tray::{ITEM_PATH, MENU_PATH, ServiceClient};

/// How many times the item is published.
const STARTS: usize = 200;

/// How long a call may go unanswered, counted from when it is sent, which is
/// once the item is published; long enough that only a call nobody will ever
/// answer runs into it, even under load.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

const SERVICE_NAME: &str = "org.betteros.Awake1FirstCallTest";

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
        let stdout = child.stdout.take()?;
        let mut address = String::new();
        BufReader::new(stdout).read_line(&mut address).ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
    }

    fn builder(&self) -> zbus::connection::Builder<'static> {
        let address: zbus::Address = self.address.parse().expect("the bus address");
        zbus::connection::Builder::address(address).expect("a builder for the bus")
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Calls `path` on `destination` and says whether it was answered in time.
async fn answered(
    caller: &zbus::Connection,
    destination: &str,
    path: &str,
    interface: &str,
    property: &str,
) -> Result<(), String> {
    let body = (interface, property);
    let call = caller.call_method(
        Some(destination),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &body,
    );
    match tokio::time::timeout(ANSWER_TIMEOUT, call).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(format!("no answer within {ANSWER_TIMEOUT:?}")),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_published_item_answers_its_first_call() {
    let Some(bus) = PrivateBus::start() else {
        eprintln!("skipping: dbus-daemon is not available in this environment");
        return;
    };

    // The tray is a client of the service, so one is running on the bus.
    let directory = tempfile::tempdir().unwrap();
    let engine = Arc::new(
        AwakeEngine::start_in(
            FakeInhibitorBackend::logind_shaped(),
            directory.path(),
            Arc::new(FixedClock::at(1_700_000_000)),
        )
        .await,
    );
    let _service =
        awake_service::service::serve(bus.builder(), SERVICE_NAME, AwakeDbusService::new(engine))
            .await
            .expect("the service on the bus");
    // zbus's own 25-second limit must not decide before the test does.
    let caller = bus
        .builder()
        .method_timeout(ANSWER_TIMEOUT * 2)
        .build()
        .await
        .expect("a client connection");

    let mut unanswered = Vec::new();
    for start in 0..STARTS {
        let client = ServiceClient::with_destination(
            bus.builder().build().await.expect("a client connection"),
            SERVICE_NAME.to_string(),
        )
        .await
        .expect("a client of the service");
        let status = client.status().await.expect("the service's status");
        let controller = Arc::new(TrayController::new(client, Locale::EnUs, status));

        let connection = awake_tray::serve(bus.builder(), controller)
            .await
            .expect("the item served on the bus");
        let item = connection.unique_name().unwrap().to_string();
        // The panel's first two calls, made the moment `main` would register.
        let (icon, menu) = tokio::join!(
            answered(
                &caller,
                &item,
                ITEM_PATH,
                "org.kde.StatusNotifierItem",
                "Id"
            ),
            answered(
                &caller,
                &item,
                MENU_PATH,
                "com.canonical.dbusmenu",
                "Version"
            ),
        );
        for (part, answer) in [("item", icon), ("menu", menu)] {
            if let Err(error) = answer {
                unanswered.push(format!("start {start}, {part}: {error}"));
            }
        }
    }

    assert!(
        unanswered.is_empty(),
        "{} calls across {STARTS} starts were left unanswered: {unanswered:#?}",
        unanswered.len()
    );
    eprintln!("{STARTS} of {STARTS} published items answered their first calls");
}
