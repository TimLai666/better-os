//! Drives the daemon's D-Bus surface over a private session bus.
//!
//! Nothing here needs privileges: the authorizer, APT, the host, and the health
//! probe are all fakes. What is being tested is the bus surface itself —
//! authorization refusal, idempotency, staging over a file descriptor, and the
//! shape of what comes back.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use manager_daemon::apt::{DebFields, FakeAptDriver};
use manager_daemon::authorize::FakeAuthorizer;
use manager_daemon::executor::Executor;
use manager_daemon::health::FakeHealthProbe;
use manager_daemon::host::FixedHostProbe;
use manager_daemon::service::{ManagerService, OBJECT_PATH};
use manager_daemon::store::{ArtifactStore, Journal};
use manager_ipc::{
    OutcomeStatus, PROTOCOL_VERSION, TransactionOutcome, WireAction, WireArtifact, WirePlan,
    WireStep,
};
use sha2::{Digest, Sha256};

const MONITOR_DEB: &str = "better-monitor_0.1.0_ubuntu-24.04_amd64.deb";
const TRANSACTION: &str = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";

/// A private session bus, so the test never touches the developer's own bus.
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
        let mut reader = BufReader::new(stdout);
        let mut address = String::new();
        reader.read_line(&mut address).ok()?;
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

struct Fixture {
    root: std::path::PathBuf,
    artifacts: Arc<ArtifactStore>,
    journal: Arc<Journal>,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("better-os-dbus-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self {
            artifacts: Arc::new(ArtifactStore::new(root.join("archives"))),
            journal: Arc::new(Journal::new(root.join("state"))),
            root,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn digest(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn plan(sha256: String) -> WirePlan {
    WirePlan {
        protocol_version: PROTOCOL_VERSION,
        transaction_id: TRANSACTION.to_string(),
        target_release: "24.04".to_string(),
        target_architecture: "amd64".to_string(),
        steps: vec![WireStep {
            component: "better-monitor".to_string(),
            action: WireAction::Install,
            before_version: None,
            after_version: Some("0.1.0".to_string()),
            artifact: Some(WireArtifact {
                filename: MONITOR_DEB.to_string(),
                sha256,
                size_bytes: 64,
            }),
        }],
    }
}

/// Publishes the service on a private bus and returns a connection to it.
async fn serve(
    bus: &PrivateBus,
    fixture: &Fixture,
    authorized: bool,
) -> zbus::Result<zbus::Connection> {
    let apt = FakeAptDriver::new().with_deb(
        MONITOR_DEB,
        DebFields {
            package: "better-monitor".to_string(),
            version: "0.1.0".to_string(),
            architecture: "amd64".to_string(),
        },
    );
    let executor = Arc::new(Executor {
        apt: Arc::new(apt),
        host: Arc::new(FixedHostProbe::ubuntu_2404()),
        health: Arc::new(FakeHealthProbe(vec![std::path::PathBuf::from(
            "/usr/bin/better-monitor",
        )])),
        artifacts: fixture.artifacts.clone(),
        journal: fixture.journal.clone(),
    });

    let address: zbus::Address = bus.address.parse()?;
    let connection = zbus::connection::Builder::address(address.clone())?
        .name("org.betteros.Manager1Test")?
        .serve_at(
            OBJECT_PATH,
            ManagerService::new(
                FakeAuthorizer(authorized),
                executor,
                fixture.artifacts.clone(),
                fixture.journal.clone(),
            ),
        )?
        .build()
        .await?;
    Ok(connection)
}

async fn client(bus: &PrivateBus) -> zbus::Result<zbus::Connection> {
    let address: zbus::Address = bus.address.parse()?;
    zbus::connection::Builder::address(address)?.build().await
}

async fn call<'a, B, R>(connection: &zbus::Connection, method: &str, body: &B) -> zbus::Result<R>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
{
    let reply = connection
        .call_method(
            Some("org.betteros.Manager1Test"),
            OBJECT_PATH,
            Some("org.betteros.Manager1"),
            method,
            body,
        )
        .await?;
    reply.body().deserialize()
}

/// Skips rather than failing when the environment has no session bus binary.
macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None => {
                eprintln!("skipping: dbus-daemon is not available in this environment");
                return;
            }
        }
    };
}

#[tokio::test]
async fn an_unauthorized_caller_cannot_apply_a_transaction() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("unauthorized");
    let _service = serve(&bus, &fixture, false).await.unwrap();
    let client = client(&bus).await.unwrap();

    let document = plan("a".repeat(64)).to_json().unwrap();
    let error = call::<_, String>(&client, "ApplyTransaction", &(document,))
        .await
        .expect_err("an unauthorized caller must be refused");

    assert!(
        error.to_string().contains("daemon.error.unauthorized"),
        "unexpected error: {error}"
    );
    // Nothing was recorded, because nothing was attempted.
    assert!(fixture.journal.read(TRANSACTION).unwrap().is_none());
}

#[tokio::test]
async fn an_artifact_is_staged_over_a_descriptor_and_then_installed() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("stage-apply");
    let _service = serve(&bus, &fixture, true).await.unwrap();
    let client = client(&bus).await.unwrap();

    let content = b"a package would be here";
    let checksum = digest(content);
    let source = fixture.root.join("source.deb");
    std::fs::write(&source, content).unwrap();
    let file = std::fs::File::open(&source).unwrap();
    let fd = zbus::zvariant::OwnedFd::from(std::os::fd::OwnedFd::from(file));

    let verified: String = call(
        &client,
        "StageArtifact",
        &(TRANSACTION, MONITOR_DEB, checksum.as_str(), fd),
    )
    .await
    .unwrap();
    assert_eq!(verified, checksum);

    let document = plan(checksum).to_json().unwrap();
    let reply: String = call(&client, "ApplyTransaction", &(document,))
        .await
        .unwrap();
    let outcome = TransactionOutcome::from_json(&reply).unwrap();

    assert_eq!(outcome.status, OutcomeStatus::Succeeded);
    assert_eq!(outcome.transaction_id, TRANSACTION);
    assert_eq!(outcome.reports.len(), 1);
}

#[tokio::test]
async fn an_artifact_whose_checksum_is_wrong_is_refused_and_never_lands() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("bad-checksum");
    let _service = serve(&bus, &fixture, true).await.unwrap();
    let client = client(&bus).await.unwrap();

    let source = fixture.root.join("source.deb");
    std::fs::write(&source, b"not what was promised").unwrap();
    let file = std::fs::File::open(&source).unwrap();
    let fd = zbus::zvariant::OwnedFd::from(std::os::fd::OwnedFd::from(file));

    let error = call::<_, String>(
        &client,
        "StageArtifact",
        &(TRANSACTION, MONITOR_DEB, "b".repeat(64).as_str(), fd),
    )
    .await
    .expect_err("a mismatched checksum must be refused");

    assert!(
        error.to_string().contains("checksum_mismatch"),
        "unexpected error: {error}"
    );
    assert!(!fixture.artifacts.contains(MONITOR_DEB));
}

#[tokio::test]
async fn re_sending_a_finished_transaction_reports_it_instead_of_running_it_again() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("idempotent");
    let _service = serve(&bus, &fixture, true).await.unwrap();
    let client = client(&bus).await.unwrap();

    let content = b"a package would be here";
    let checksum = digest(content);
    let source = fixture.root.join("source.deb");
    std::fs::write(&source, content).unwrap();
    let file = std::fs::File::open(&source).unwrap();
    let fd = zbus::zvariant::OwnedFd::from(std::os::fd::OwnedFd::from(file));
    let _: String = call(
        &client,
        "StageArtifact",
        &(TRANSACTION, MONITOR_DEB, checksum.as_str(), fd),
    )
    .await
    .unwrap();

    let document = plan(checksum).to_json().unwrap();
    let first: String = call(&client, "ApplyTransaction", &(document.clone(),))
        .await
        .unwrap();
    let second: String = call(&client, "ApplyTransaction", &(document,))
        .await
        .unwrap();

    assert_eq!(first, second, "a repeat must report, not re-run");

    // GetStatus needs no authorization and answers after the fact, which is
    // what a client that lost its connection relies on.
    let status: String = call(&client, "GetStatus", &(TRANSACTION,)).await.unwrap();
    assert!(status.contains("completed"));
}

#[tokio::test]
async fn a_plan_for_another_host_is_refused_without_touching_anything() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("wrong-host");
    let _service = serve(&bus, &fixture, true).await.unwrap();
    let client = client(&bus).await.unwrap();

    let mut plan = plan("a".repeat(64));
    plan.target_release = "22.04".to_string();
    plan.steps[0].artifact.as_mut().unwrap().filename =
        "better-monitor_0.1.0_ubuntu-22.04_amd64.deb".to_string();

    let reply: String = call(&client, "ApplyTransaction", &(plan.to_json().unwrap(),))
        .await
        .unwrap();
    let outcome = TransactionOutcome::from_json(&reply).unwrap();

    let OutcomeStatus::Failed {
        recovery,
        error_key,
        step_index,
    } = &outcome.status
    else {
        panic!("expected a refusal, got {:?}", outcome.status);
    };
    assert!(error_key.contains("plan_rejected"), "{error_key}");
    assert_eq!(*step_index, None);
    assert_eq!(*recovery, None, "nothing changed, so nothing recovered");
}

#[tokio::test]
async fn the_protocol_version_is_readable_before_anything_is_attempted() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("version");
    let _service = serve(&bus, &fixture, true).await.unwrap();
    let client = client(&bus).await.unwrap();

    let properties = zbus::fdo::PropertiesProxy::builder(&client)
        .destination("org.betteros.Manager1Test")
        .unwrap()
        .path(OBJECT_PATH)
        .unwrap()
        .build()
        .await
        .unwrap();
    let version = properties
        .get(
            "org.betteros.Manager1".try_into().unwrap(),
            "ProtocolVersion",
        )
        .await
        .unwrap();

    assert_eq!(u32::try_from(&version).unwrap(), PROTOCOL_VERSION);
}
