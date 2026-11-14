//! The service answers the call that D-Bus-activated it.
//!
//! zbus starts a connection's object server lazily, in a task that subscribes
//! to method calls some time after it is spawned. A name requested before that
//! subscription exists lets the call the bus was holding for the name reach a
//! stream nobody is reading, and that call is never answered (ticket 107).
//!
//! So the real start-up path, [`awake_service::service::serve`] as `main`
//! calls it, is started [`STARTS`] times on a private, activating
//! `dbus-daemon`, each time behind a call the bus is holding for the name, and
//! every one of those calls must be answered. The inhibitor is a fake, so
//! nothing here touches logind.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use awake_service::backend::{FakeInhibitorBackend, FixedClock};
use awake_service::{AwakeDbusService, AwakeEngine, INTERFACE_NAME, OBJECT_PATH};

/// How many times the service is started.
const STARTS: usize = 200;

/// How long a call may go unanswered once the service owns its name. Counted
/// from then, not from when the call was sent, so a slow start under load is
/// not mistaken for a lost call; long enough that only a call nobody will
/// ever answer runs into it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// The caller's own limit, longer than any start plus [`ANSWER_TIMEOUT`]:
/// zbus would otherwise give up on its own 25 seconds after sending.
const CALLER_TIMEOUT: Duration = Duration::from_secs(600);

/// A private bus that D-Bus-activates the test's names.
///
/// Each name has a service file whose `Exec` only records that the bus asked
/// for it and then idles for a second. The bus holds a call to the name until
/// somebody owns it and delivers it the moment the name is taken, which is
/// exactly what happens to the call that activates a service at login. The
/// bus, its configuration, and the service files are all this test's own.
struct ActivatingBus {
    child: Child,
    address: String,
    directory: PathBuf,
}

impl ActivatingBus {
    fn start(label: &str, names: &[String]) -> Option<Self> {
        let directory = std::env::temp_dir().join(format!(
            "better-os-first-call-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let services = directory.join("services");
        std::fs::create_dir_all(&services).ok()?;
        for name in names {
            let asked = directory.join(format!("{name}.asked"));
            std::fs::write(
                services.join(format!("{name}.service")),
                format!(
                    "[D-BUS Service]\nName={name}\nExec=/bin/sh -c \"touch {}; exec sleep 1\"\n",
                    asked.display()
                ),
            )
            .ok()?;
        }
        let config = directory.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:dir={}</listen>\
                 <auth>EXTERNAL</auth><servicedir>{}</servicedir>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
                directory.display(),
                services.display()
            ),
        )
        .ok()?;

        let child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            let _ = std::fs::remove_dir_all(&directory);
            return None;
        };
        use std::io::{BufRead, BufReader};
        let mut address = String::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = BufReader::new(stdout).read_line(&mut address);
        }
        let bus = Self {
            child,
            address: address.trim().to_string(),
            directory,
        };
        (!bus.address.is_empty()).then_some(bus)
    }

    fn builder(&self) -> zbus::connection::Builder<'static> {
        let address: zbus::Address = self.address.parse().expect("the bus address");
        zbus::connection::Builder::address(address).expect("a builder for the bus")
    }

    /// The client connection: its calls wait as long as the test does.
    async fn caller(&self) -> zbus::Connection {
        self.builder()
            .method_timeout(CALLER_TIMEOUT)
            .build()
            .await
            .expect("a client connection")
    }

    /// Waits until the bus has started activating `name`, so a call to it is
    /// being held for whoever takes the name.
    async fn activating(&self, name: &str) -> bool {
        let asked = self.directory.join(format!("{name}.asked"));
        for _ in 0..5_000 {
            if asked.exists() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        false
    }
}

impl Drop for ActivatingBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Sends `call`, the one a client activating the service makes. The bus
/// holds it until the name has an owner; [`settle`] says whether it was then
/// answered.
fn answered<F>(call: F) -> tokio::task::JoinHandle<zbus::Result<zbus::Message>>
where
    F: std::future::Future<Output = zbus::Result<zbus::Message>> + Send + 'static,
{
    tokio::spawn(call)
}

/// Waits for the call [`answered`] sent, from the moment the service owns its
/// name, for at most [`ANSWER_TIMEOUT`]. A panic inside the call is passed on
/// with its own message.
async fn settle(
    mut call: tokio::task::JoinHandle<zbus::Result<zbus::Message>>,
) -> Result<(), String> {
    match tokio::time::timeout(ANSWER_TIMEOUT, &mut call).await {
        Ok(Ok(Ok(_))) => Ok(()),
        Ok(Ok(Err(error))) => Err(error.to_string()),
        Ok(Err(error)) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => {
            call.abort();
            Err(format!(
                "no answer within {ANSWER_TIMEOUT:?} of the name being taken"
            ))
        }
    }
}

/// The names the test activates, one per start, so one start never waits for
/// the last to let go.
fn names(prefix: &str) -> Vec<String> {
    (0..STARTS)
        .map(|start| format!("{prefix}.Start{start}"))
        .collect()
}

fn require_every_start_answered(unanswered: &[String]) {
    assert!(
        unanswered.is_empty(),
        "{} of {STARTS} starts left the call that activated them unanswered: {unanswered:#?}",
        unanswered.len()
    );
    eprintln!("{STARTS} of {STARTS} starts answered the call that activated them");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_start_answers_the_call_that_activated_it() {
    let names = names("org.betteros.Awake1Test");
    let Some(bus) = ActivatingBus::start("awake-service", &names) else {
        eprintln!("skipping: dbus-daemon is not available in this environment");
        return;
    };
    let caller = bus.caller().await;

    let mut unanswered = Vec::new();
    for (start, name) in names.iter().enumerate() {
        let call = answered({
            let caller = caller.clone();
            let name = name.clone();
            async move {
                caller
                    .call_method(
                        Some(name.as_str()),
                        OBJECT_PATH,
                        Some("org.freedesktop.DBus.Properties"),
                        "Get",
                        &(INTERFACE_NAME, "ProtocolVersion"),
                    )
                    .await
            }
        });
        assert!(
            bus.activating(name).await,
            "start {start}: the bus activated"
        );

        let directory = tempfile::tempdir().unwrap();
        let engine = Arc::new(
            AwakeEngine::start_in(
                FakeInhibitorBackend::logind_shaped(),
                directory.path(),
                Arc::new(FixedClock::at(1_700_000_000)),
            )
            .await,
        );
        let connection =
            awake_service::service::serve(bus.builder(), name, AwakeDbusService::new(engine))
                .await
                .expect("the service started on the bus");

        if let Err(error) = settle(call).await {
            unanswered.push(format!("start {start}: {error}"));
        }
        let _ = connection.release_name(name.as_str()).await;
    }
    require_every_start_answered(&unanswered);
}
