//! Drives the daemon's D-Bus surface over a private session bus.
//!
//! Nothing here needs privileges: the authorizer, APT, the host, and the health
//! probe are all fakes. What is being tested is the bus surface itself —
//! authorization refusal, idempotency, staging over a file descriptor, and the
//! shape of what comes back.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use manager_daemon::apt::{DebFields, FakeAptDriver};
use manager_daemon::authorize::{
    APPLY_ACTION, Authorizer, FakeActionAuthorizer, FakeAuthorizer, REPAIR_ACTION,
};
use manager_daemon::executor::Executor;
use manager_daemon::health::FakeHealthProbe;
use manager_daemon::host::FixedHostProbe;
use manager_daemon::service::{ManagerService, OBJECT_PATH};
use manager_daemon::store::{ArtifactStore, Journal};
use manager_ipc::{
    DpkgPackageStatus, InterruptedPackage, OutcomeStatus, PROTOCOL_VERSION, PackageSystemState,
    RepairOutcome, RepairStatus, TransactionOutcome, WireAction, WireArtifact, WirePlan, WireStep,
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
            companions: Vec::new(),
        }],
    }
}

fn monitor_apt() -> FakeAptDriver {
    FakeAptDriver::new().with_deb(
        MONITOR_DEB,
        DebFields {
            package: "better-monitor".to_string(),
            version: "0.1.0".to_string(),
            architecture: "amd64".to_string(),
        },
    )
}

/// Publishes the service on a private bus and returns a connection to it.
async fn serve(
    bus: &PrivateBus,
    fixture: &Fixture,
    authorized: bool,
) -> zbus::Result<zbus::Connection> {
    serve_with(
        bus,
        fixture,
        FakeAuthorizer(authorized),
        Arc::new(monitor_apt()),
    )
    .await
}

/// Publishes the service with a chosen authorizer and APT, so a test can grant
/// one polkit action and not another, and can inspect what APT was asked.
async fn serve_with<A: Authorizer + 'static>(
    bus: &PrivateBus,
    fixture: &Fixture,
    authorizer: A,
    apt: Arc<FakeAptDriver>,
) -> zbus::Result<zbus::Connection> {
    let executor = Arc::new(Executor {
        apt,
        host: Arc::new(FixedHostProbe::ubuntu_2404()),
        health: Arc::new(FakeHealthProbe::with_files(vec![std::path::PathBuf::from(
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
                authorizer,
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

fn interrupted() -> PackageSystemState {
    PackageSystemState {
        protocol_version: PROTOCOL_VERSION,
        journal_pending: true,
        packages: vec![InterruptedPackage {
            package: "better-monitor".to_string(),
            status: DpkgPackageStatus::HalfConfigured,
        }],
        omitted: 0,
    }
}

/// Reading dpkg's state changes nothing, so it needs no authorization, the
/// same as `GetStatus`. A caller polkit would refuse still gets the answer.
#[tokio::test]
async fn the_package_system_state_is_readable_without_authorization() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("dpkg-state");
    let apt = Arc::new(monitor_apt().with_package_system(interrupted()));
    let _service = serve_with(&bus, &fixture, FakeAuthorizer(false), apt.clone())
        .await
        .unwrap();
    let client = client(&bus).await.unwrap();

    let reply: String = call(&client, "GetPackageSystemState", &()).await.unwrap();
    let state = PackageSystemState::from_json(&reply).unwrap();

    assert_eq!(state, interrupted());
    assert!(apt.calls().is_empty(), "reading the state ran nothing");
}

/// The repair asks polkit for its own action. A caller allowed to apply
/// transactions is not thereby allowed to run `dpkg --configure -a`.
#[tokio::test]
async fn a_caller_authorized_only_to_apply_transactions_cannot_repair() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("repair-wrong-action");
    let apt = Arc::new(monitor_apt().with_package_system(interrupted()));
    let _service = serve_with(
        &bus,
        &fixture,
        FakeActionAuthorizer(APPLY_ACTION),
        apt.clone(),
    )
    .await
    .unwrap();
    let client = client(&bus).await.unwrap();

    let error = call::<_, String>(&client, "RepairPackageSystem", &())
        .await
        .expect_err("the apply action must not authorize a repair");

    assert!(
        error.to_string().contains("daemon.error.unauthorized"),
        "unexpected error: {error}"
    );
    assert!(apt.calls().is_empty(), "dpkg must not have run");
}

#[tokio::test]
async fn an_authorized_repair_runs_dpkg_and_reports_its_outcome_and_log() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("repair-ok");
    let apt = Arc::new(monitor_apt().with_package_system(interrupted()));
    let _service = serve_with(
        &bus,
        &fixture,
        FakeActionAuthorizer(REPAIR_ACTION),
        apt.clone(),
    )
    .await
    .unwrap();
    let client = client(&bus).await.unwrap();

    let reply: String = call(&client, "RepairPackageSystem", &()).await.unwrap();
    let outcome = RepairOutcome::from_json(&reply).unwrap();

    assert_eq!(outcome.status, RepairStatus::Repaired);
    assert_eq!(
        outcome.log[0].argv,
        vec![
            "dpkg",
            "--force-confdef",
            "--force-confold",
            "--configure",
            "-a"
        ]
    );
    assert_eq!(apt.calls(), vec!["configure-pending".to_string()]);

    // The state the daemon now reports agrees with the outcome.
    let reply: String = call(&client, "GetPackageSystemState", &()).await.unwrap();
    assert!(
        !PackageSystemState::from_json(&reply)
            .unwrap()
            .needs_repair()
    );
}

#[tokio::test]
async fn a_repair_with_nothing_to_repair_is_refused_and_runs_nothing() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("repair-clean");
    let apt = Arc::new(monitor_apt());
    let _service = serve_with(&bus, &fixture, FakeAuthorizer(true), apt.clone())
        .await
        .unwrap();
    let client = client(&bus).await.unwrap();

    let error = call::<_, String>(&client, "RepairPackageSystem", &())
        .await
        .expect_err("a clean package system has nothing to repair");

    assert!(
        error.to_string().contains("daemon.error.nothing_to_repair"),
        "unexpected error: {error}"
    );
    assert!(apt.calls().is_empty(), "dpkg must not have run");
}

/// The refusal is reported as an outcome under the client's transaction id,
/// like every other refusal before a change, so the client records why.
#[tokio::test]
async fn a_transaction_sent_while_dpkg_is_interrupted_is_refused_before_any_change() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("apply-interrupted");
    let apt = Arc::new(monitor_apt().with_package_system(interrupted()));
    let _service = serve_with(&bus, &fixture, FakeAuthorizer(true), apt.clone())
        .await
        .unwrap();
    let client = client(&bus).await.unwrap();

    let reply: String = call(
        &client,
        "ApplyTransaction",
        &(plan("a".repeat(64)).to_json().unwrap(),),
    )
    .await
    .unwrap();
    let outcome = TransactionOutcome::from_json(&reply).unwrap();

    let OutcomeStatus::Failed {
        error_key,
        step_index,
        recovery,
    } = &outcome.status
    else {
        panic!("expected a refusal, got {:?}", outcome.status);
    };
    assert_eq!(error_key, "daemon.error.dpkg_interrupted");
    assert_eq!(*step_index, None);
    assert_eq!(*recovery, None, "nothing changed, so nothing recovered");
    assert_eq!(outcome.transaction_id, TRANSACTION);
    assert!(apt.calls().is_empty(), "APT must not have run");
}

/// Ticket 105: a client that goes away in the middle of `ApplyTransaction`
/// must not make the daemon think it is idle while dpkg still runs — or it
/// could leave, and a package postinst could restart it, half way through.
/// The running slot, the call's place inside the gate, and the busy marker the
/// daemon package's postinst reads all stay until the install has finished.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_disconnects_mid_transaction_leaves_the_daemon_busy_until_dpkg_ends() {
    use std::time::Duration;
    let bus = bus_or_skip!();
    let fixture = Fixture::new("disconnect");
    let marker = fixture.root.join("run").join("daemon-busy");
    let apt = Arc::new(monitor_apt().with_install_duration(Duration::from_millis(1500)));
    let executor = Arc::new(Executor {
        apt: apt.clone(),
        host: Arc::new(FixedHostProbe::ubuntu_2404()),
        health: Arc::new(FakeHealthProbe::with_files(vec![std::path::PathBuf::from(
            "/usr/bin/better-monitor",
        )])),
        artifacts: fixture.artifacts.clone(),
        journal: fixture.journal.clone(),
    });
    let service = ManagerService::new(
        FakeAuthorizer(true),
        executor,
        fixture.artifacts.clone(),
        fixture.journal.clone(),
    )
    .with_busy_marker(&marker);
    let running = service.activity();
    let gate = service.gate();
    let address: zbus::Address = bus.address.parse().unwrap();
    let _server = zbus::connection::Builder::address(address)
        .unwrap()
        .name("org.betteros.Manager1Test")
        .unwrap()
        .serve_at(OBJECT_PATH, service)
        .unwrap()
        .build()
        .await
        .unwrap();

    let content = b"a package would be here";
    let checksum = digest(content);
    let source = fixture.root.join("source.deb");
    std::fs::write(&source, content).unwrap();
    let stager = client(&bus).await.unwrap();
    let fd = zbus::zvariant::OwnedFd::from(std::os::fd::OwnedFd::from(
        std::fs::File::open(&source).unwrap(),
    ));
    let _: String = call(
        &stager,
        "StageArtifact",
        &(TRANSACTION, MONITOR_DEB, checksum.as_str(), fd),
    )
    .await
    .unwrap();

    // A client that asks and then goes away.
    let leaver = client(&bus).await.unwrap();
    let document = plan(checksum).to_json().unwrap();
    let asking = {
        let leaver = leaver.clone();
        tokio::spawn(async move {
            let _ = call::<_, String>(&leaver, "ApplyTransaction", &(document,)).await;
        })
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !apt.calls().iter().any(|c| c.starts_with("install:")) {
        assert!(
            std::time::Instant::now() < deadline,
            "the install never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    asking.abort();
    drop(leaver);
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(
        running.load(std::sync::atomic::Ordering::SeqCst),
        "still running"
    );
    assert!(gate.inside() > 0, "the call is still inside the gate");
    assert!(marker.exists(), "the postinst can see a transaction runs");

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while running.load(std::sync::atomic::Ordering::SeqCst) || gate.inside() > 0 {
        assert!(std::time::Instant::now() < deadline, "never finished");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!marker.exists(), "the marker goes with the transaction");
    let entry = fixture.journal.read(TRANSACTION).unwrap().unwrap();
    assert!(entry.outcome.is_some(), "the transaction ran to its end");
}

#[tokio::test]
async fn a_busy_marker_left_by_a_daemon_that_died_is_cleared_at_start() {
    let fixture = Fixture::new("stale-marker");
    let marker = fixture.root.join("daemon-busy");
    std::fs::write(&marker, "1234\n").unwrap();
    let executor = Arc::new(Executor {
        apt: Arc::new(monitor_apt()),
        host: Arc::new(FixedHostProbe::ubuntu_2404()),
        health: Arc::new(FakeHealthProbe::empty()),
        artifacts: fixture.artifacts.clone(),
        journal: fixture.journal.clone(),
    });
    let _service = ManagerService::new(
        FakeAuthorizer(true),
        executor,
        fixture.artifacts.clone(),
        fixture.journal.clone(),
    )
    .with_busy_marker(&marker);
    assert!(!marker.exists());
}
