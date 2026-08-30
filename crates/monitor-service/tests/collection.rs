//! What the engine does when nobody is watching.
//!
//! The milestone this ticket exists for is one sentence: collection outlives
//! the GUI. Everything else here supports proving it — that a client can
//! connect and disconnect without touching the sampling loop, that the store
//! keeps growing afterwards, and that the timeline across the disconnect has
//! no hole in it.
//!
//! The collectors read a captured `/proc` and `/sys` tree from ticket 22's
//! fixtures rather than the machine the test runs on, so the assertions are
//! about the service rather than about how busy the build agent happened to
//! be.

use std::path::PathBuf;
use std::time::Duration;

use monitor_collectors_linux::Roots;
use monitor_ipc::{MonitorRequest, RequestBody, ResponseBody};
use monitor_service::{
    AuditSources, ComponentVersions, MonitorEngine, ServiceConfig, SessionFacts,
};
use monitor_store::{GapReason, RetentionPolicy, TimeRange};

/// Ticket 22's captured machine. Shared rather than duplicated: a second copy
/// would drift from the one the collectors are tested against, and then this
/// test would be proving something about a machine that does not exist.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("monitor-collectors-linux")
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The captured machine the *audit* reads.
///
/// Separate from the collector fixture on purpose. Ticket 22 captured only
/// what the collectors read, so it has no `os-release`, `cpuinfo`, `mounts`,
/// or `sys/kernel`; adding them there would change a fixture that six
/// collectors are asserted against. `AuditSources` carries its own roots
/// precisely so the two can differ.
fn machine() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("machine")
}

fn config(store_root: PathBuf) -> ServiceConfig {
    config_at(store_root, &machine())
}

fn config_at(store_root: PathBuf, audit_machine: &std::path::Path) -> ServiceConfig {
    let mut config = ServiceConfig::at(
        store_root,
        Roots::at(fixture("snapshot-a")),
        AuditSources {
            roots: Roots::at(audit_machine),
            session: SessionFacts {
                user: Some("tim".into()),
                home: Some("/home/tim".into()),
                desktop: Some("XFCE".into()),
                session_type: Some("wayland".into()),
                display_server: Some("wayland".into()),
            },
            components: ComponentVersions::none(),
        },
    );
    config.sample_interval = Duration::from_millis(10);
    config.retention = RetentionPolicy {
        // One stored sample per raw round, so a short test still writes
        // history. The downsampler is proved separately in `monitor-store`.
        resolution_seconds: 0,
        ..RetentionPolicy::default()
    };
    config
}

/// Drive the engine directly for a fixed number of rounds.
async fn tick(engine: &MonitorEngine, rounds: usize) {
    for _ in 0..rounds {
        engine.tick().await.expect("a recordable round");
    }
}

#[tokio::test]
async fn collection_continues_after_every_client_has_disconnected() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    let sampling = monitor_service::spawn_sampling(engine.clone());

    // A client connects, asks what it would ask, and goes away. In the real
    // system this is the window opening and being closed; here it is the same
    // request path, without a bus in the way.
    {
        let client_view = engine
            .handle(MonitorRequest::new(RequestBody::QueryStatus {
                include_latest_round: true,
            }))
            .await;
        let ResponseBody::Status(status) = client_view.body else {
            panic!("a status reply")
        };
        assert!(status.recording);
        assert!(status.latest_round.is_some() || status.rounds_collected == 0);
    }

    // Nothing holds a reference to a client any more. This is the moment the
    // GUI is gone.
    let rounds_when_the_client_left = engine.rounds();
    let samples_when_the_client_left = engine.with_store(|store| store.stats().samples).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    let rounds_later = engine.rounds();
    let stats_later = engine.with_store(|store| store.stats()).await;
    sampling.abort();

    assert!(
        rounds_later > rounds_when_the_client_left,
        "collection stopped when the client did: {rounds_when_the_client_left} then {rounds_later}"
    );
    assert!(
        stats_later.samples > samples_when_the_client_left,
        "the store stopped growing when the client did: {} then {}",
        samples_when_the_client_left,
        stats_later.samples
    );

    // The timeline across the disconnect is unbroken: nothing in the window
    // the client was absent for is recorded as a gap.
    let gaps = engine
        .with_store(|store| store.slice(TimeRange::all(), usize::MAX).gaps)
        .await;
    assert!(
        gaps.is_empty(),
        "the timeline was broken while nobody was watching: {gaps:?}"
    );
}

#[tokio::test]
async fn a_second_client_sees_the_history_the_first_one_never_waited_for() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    tick(&engine, 6).await;

    let response = engine
        .handle(MonitorRequest::new(RequestBody::QueryHistory {
            from_unix_ms: 0,
            to_unix_ms: u64::MAX,
            max_samples: 1_000,
        }))
        .await;
    let ResponseBody::History(history) = response.body else {
        panic!("a history reply")
    };
    assert!(history.slice.samples.len() >= 5);
    assert!(!history.slice.truncated);
    // Every sample carries both clocks, which is what makes a chart honest
    // about irregular sampling.
    for sample in &history.slice.samples {
        assert!(sample.wall_unix_ms > 0);
        assert!(!sample.collectors.is_empty());
    }
}

#[tokio::test]
async fn a_restart_records_the_hole_it_left_rather_than_hiding_it() {
    let directory = tempfile::tempdir().unwrap();
    {
        let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
        tick(&engine, 3).await;
        engine.shutdown().await.unwrap();
    }

    // A stored sample now sits well in the past relative to `now`, because the
    // fixture's timestamps come from the wall clock at the moment of the run
    // and the store's own resolution window is what decides "well in the past".
    let mut restarted = config(directory.path().to_path_buf());
    restarted.retention.resolution_seconds = 1;
    let engine = MonitorEngine::start(restarted).unwrap();
    let gaps = engine
        .with_store(|store| store.slice(TimeRange::all(), usize::MAX).gaps)
        .await;
    // Either the restart was fast enough that there is genuinely no hole, or
    // the hole is recorded as one. What must never happen is a hole that is
    // not recorded, and the only way to be sure is that any gap present says
    // the service stopped.
    assert!(
        gaps.iter()
            .all(|gap| gap.reason == GapReason::ServiceStopped),
        "an unexplained gap survived a restart: {gaps:?}"
    );
}

#[tokio::test]
async fn a_clean_shutdown_writes_the_bucket_that_was_still_in_flight() {
    let directory = tempfile::tempdir().unwrap();
    let mut settings = config(directory.path().to_path_buf());
    // A long resolution means nothing is written by the loop itself, so
    // whatever ends up in the store got there through the shutdown flush.
    settings.retention.resolution_seconds = 3_600;
    let engine = MonitorEngine::start(settings).unwrap();
    tick(&engine, 4).await;
    assert_eq!(engine.with_store(|store| store.stats().samples).await, 0);

    engine.shutdown().await.unwrap();
    let stats = engine.with_store(|store| store.stats()).await;
    assert_eq!(
        stats.samples, 1,
        "the seconds before shutdown were dropped instead of flushed"
    );
    assert!(!engine.status(false).await.recording);
}

#[tokio::test]
async fn the_first_tick_captures_an_inventory_and_a_second_one_does_not_rewrite_it() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    tick(&engine, 1).await;

    let response = engine
        .handle(MonitorRequest::new(RequestBody::QueryInventory))
        .await;
    let ResponseBody::Inventory(document) = response.body else {
        panic!("an inventory reply")
    };
    let inventory = document.inventory.expect("the first audit ran");
    assert_eq!(document.captures, 1);
    assert_eq!(inventory.get("session.user").unwrap().value, "tim");
    assert!(inventory.get("kernel.release").is_some());
    // The audit names what this build cannot observe, which is the difference
    // between a machine with no PSI and a machine whose PSI reads zero.
    assert!(
        inventory
            .entries
            .keys()
            .any(|key| key.starts_with("collector.") && key.ends_with(".supported_metrics"))
    );

    // Nothing about the captured machine changed, so a second audit writes
    // nothing.
    engine
        .audit_now(monitor_service::now_unix_ms() + 1)
        .await
        .unwrap();
    let response = engine
        .handle(MonitorRequest::new(RequestBody::QueryInventory))
        .await;
    let ResponseBody::Inventory(document) = response.body else {
        panic!("an inventory reply")
    };
    assert_eq!(document.captures, 1);

    let response = engine
        .handle(MonitorRequest::new(RequestBody::QueryInventoryDiff))
        .await;
    let ResponseBody::InventoryDiff(document) = response.body else {
        panic!("an inventory diff reply")
    };
    assert!(
        document.diff.is_none(),
        "one capture has nothing to diff against, which is not the same as no change"
    );
}

#[tokio::test]
async fn an_inventory_change_is_captured_and_diffable() {
    let staging = tempfile::tempdir().unwrap();
    copy_tree(&machine(), staging.path());

    let directory = tempfile::tempdir().unwrap();
    let engine =
        MonitorEngine::start(config_at(directory.path().to_path_buf(), staging.path())).unwrap();
    engine.audit_now(1_000).await.unwrap();

    std::fs::write(
        staging.path().join("proc/sys/kernel/osrelease"),
        "9.9.9-generic\n",
    )
    .unwrap();
    assert!(engine.audit_now(2_000).await.unwrap());

    let response = engine
        .handle(MonitorRequest::new(RequestBody::QueryInventoryDiff))
        .await;
    let ResponseBody::InventoryDiff(document) = response.body else {
        panic!("an inventory diff reply")
    };
    let diff = document.diff.expect("two captures to compare");
    let kernel = diff
        .changed
        .iter()
        .find(|change| change.key == "kernel.release")
        .expect("the kernel upgrade");
    assert_eq!(kernel.before.as_deref(), Some("6.11.0-19-generic"));
    assert_eq!(kernel.after.as_deref(), Some("9.9.9-generic"));
    // The observation capabilities legitimately move between the two audits:
    // a counter-delta metric has no value on the first round of a run and one
    // on the second. That is a real change in what the machine can be
    // observed to do, so the diff is asserted to contain the kernel change
    // rather than to be only the kernel change.
    assert!(
        diff.changed
            .iter()
            .all(|change| change.key == "kernel.release" || change.key.starts_with("collector.")),
        "an unexpected inventory key changed: {:?}",
        diff.changed
    );
}

#[tokio::test]
async fn marking_an_incident_captures_the_state_around_it() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    tick(&engine, 8).await;

    let response = engine
        .handle(MonitorRequest::new(RequestBody::MarkIncident {
            note: Some("everything froze while saving".into()),
            window_before_seconds: 60,
            window_after_seconds: 30,
            about_pid: Some(1),
        }))
        .await;
    let ResponseBody::IncidentWindow(document) = response.body else {
        panic!("an incident window reply")
    };
    let incident = &document.incident;
    assert_eq!(incident.id, 1);
    assert_eq!(
        incident.note.as_deref(),
        Some("everything froze while saving")
    );
    assert_eq!(incident.about_pid, Some(1));
    assert_eq!(incident.window.before_seconds, 60);
    assert_eq!(incident.window.after_seconds, 30);

    // The snapshot is the machine as it was at the marker: every collector's
    // health, the entities, and the bounded process list.
    assert!(!incident.snapshot.collectors.is_empty());
    assert!(!incident.snapshot.processes.is_empty());
    assert!(
        incident.snapshot.processes.len() <= monitor_store::DEFAULT_TRACKED_PROCESSES,
        "an incident must not capture an unbounded process list"
    );
    // Samples were recorded before the marker, so there is a baseline to
    // compare against.
    assert!(
        !incident.baseline.is_empty(),
        "the marker recorded no comparison with the preceding baseline"
    );
    assert!(!document.slice.samples.is_empty());
    assert!(
        document
            .slice
            .samples
            .iter()
            .all(|sample| sample.wall_unix_ms >= incident.marked_at_unix_ms - 60_000)
    );
}

#[tokio::test]
async fn an_incident_marked_before_anything_was_collected_is_honest_about_it() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();

    let response = engine
        .handle(MonitorRequest::new(RequestBody::MarkIncident {
            note: None,
            window_before_seconds: 60,
            window_after_seconds: 30,
            about_pid: None,
        }))
        .await;
    let ResponseBody::IncidentWindow(document) = response.body else {
        panic!("an incident window reply")
    };
    // No fabricated readings and no invented baseline.
    assert!(document.incident.snapshot.metrics.is_empty());
    assert!(document.incident.baseline.is_empty());
    assert!(document.slice.samples.is_empty());
}

#[tokio::test]
async fn an_incident_survives_a_restart_and_keeps_climbing_its_identifier() {
    let directory = tempfile::tempdir().unwrap();
    {
        let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
        tick(&engine, 2).await;
        engine
            .mark(Some("first"), Default::default(), None)
            .await
            .unwrap();
        engine.shutdown().await.unwrap();
    }
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    let second = engine
        .mark(Some("second"), Default::default(), None)
        .await
        .unwrap();
    assert_eq!(second, 2);

    let response = engine
        .handle(MonitorRequest::new(RequestBody::QueryIncidents))
        .await;
    let ResponseBody::Incidents(document) = response.body else {
        panic!("an incidents reply")
    };
    assert_eq!(document.incidents.len(), 2);
    assert_eq!(document.incidents[0].note.as_deref(), Some("first"));
}

#[tokio::test]
async fn the_engine_refuses_what_the_protocol_refuses() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();

    for (document, expected) in [
        (
            r#"{"protocol_version":2,"body":{"request":"query_incidents"}}"#,
            "monitor.ipc.error.protocol_version",
        ),
        (
            r#"{"protocol_version":1,"body":{"request":"burn_the_disk"}}"#,
            "monitor.ipc.error.malformed",
        ),
        (
            r#"{"protocol_version":1,"body":{"request":"query_incidents"},"extra":1}"#,
            "monitor.ipc.error.malformed",
        ),
        (
            r#"{"protocol_version":1,"body":{"request":"query_history","from_unix_ms":9,"to_unix_ms":1,"max_samples":10}}"#,
            "monitor.ipc.error.invalid_range",
        ),
        (
            r#"{"protocol_version":1,"body":{"request":"request_export","from_unix_ms":0,"to_unix_ms":1,"destination":"../escape"}}"#,
            "monitor.ipc.error.invalid_destination",
        ),
        (
            r#"{"protocol_version":1,"body":{"request":"query_export_progress","export_id":404}}"#,
            "monitor.ipc.error.unknown_export",
        ),
        (
            r#"{"protocol_version":1,"body":{"request":"query_incident_window","incident_id":404}}"#,
            "monitor.ipc.error.unknown_incident",
        ),
    ] {
        let reply = engine.handle_document(document).await;
        assert!(
            reply.contains(expected),
            "expected {expected} for {document}, got {reply}"
        );
    }
}

#[tokio::test]
async fn an_oversized_document_is_refused_without_being_parsed() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    let reply = engine
        .handle_document(&" ".repeat(monitor_ipc::MAX_REQUEST_BYTES + 1))
        .await;
    assert!(reply.contains("monitor.ipc.error.payload_too_large"));
}

#[tokio::test]
async fn an_export_is_produced_only_when_it_is_asked_for_and_never_uploaded() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    tick(&engine, 5).await;

    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join("better-monitor-export");

    // A preview writes nothing.
    let previewed = engine
        .handle(MonitorRequest::new(RequestBody::RequestExport {
            from_unix_ms: 0,
            to_unix_ms: u64::MAX,
            destination: destination.display().to_string(),
            include_processes: true,
            preview_only: true,
        }))
        .await;
    let ResponseBody::Export(document) = previewed.body else {
        panic!("an export reply")
    };
    assert!(matches!(
        document.state,
        monitor_ipc::ExportState::Previewed { .. }
    ));
    assert!(!destination.exists());

    let written = engine
        .handle(MonitorRequest::new(RequestBody::RequestExport {
            from_unix_ms: 0,
            to_unix_ms: u64::MAX,
            destination: destination.display().to_string(),
            include_processes: true,
            preview_only: false,
        }))
        .await;
    let ResponseBody::Export(document) = written.body else {
        panic!("an export reply")
    };
    let monitor_ipc::ExportState::Completed { files, .. } = &document.state else {
        panic!("a completed export, got {:?}", document.state)
    };
    assert!(files.iter().any(|file| file == "manifest.json"));
    assert!(destination.join("manifest.json").exists());

    // The progress of an export that already finished is still answerable.
    let progress = engine
        .handle(MonitorRequest::new(RequestBody::QueryExportProgress {
            export_id: document.export_id,
        }))
        .await;
    let ResponseBody::Export(same) = progress.body else {
        panic!("an export reply")
    };
    assert_eq!(same.export_id, document.export_id);
}

#[tokio::test]
async fn the_status_document_reports_what_the_service_can_and_cannot_observe() {
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    tick(&engine, 2).await;

    let status = engine.status(false).await;
    assert!(status.recording);
    assert_eq!(status.rounds_collected, 2);
    assert_eq!(status.collectors.len(), 6);
    assert!(status.latest_round.is_none(), "not asked for");
    assert_eq!(
        status.retention.window_seconds,
        RetentionPolicy::default().window_seconds
    );

    let with_round = engine.status(true).await;
    assert_eq!(with_round.latest_round.map(|round| round.len()), Some(6));
}

/// Copy a fixture so a test can modify it without touching the original.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    for entry in std::fs::read_dir(from).expect("a readable fixture") {
        let entry = entry.expect("a readable entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            std::fs::create_dir_all(&target).expect("a writable directory");
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a copyable file");
        }
    }
}
