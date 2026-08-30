//! Measured cost of the service loop against a synthetic event stream.
//!
//! What is measured is the part that is this component's own: the time from a
//! platform event arriving to the matching typed update being published, and
//! the cost of an idle session. The platform work behind it — a D-Bus round
//! trip, a `syncfs` on a real filesystem — is faked here on purpose, because
//! including it would measure the kernel and the bus rather than this code.
//!
//! Real device throughput (exFAT, NTFS, ext4 across USB flash, external SSD,
//! and external HDD) needs hardware and is recorded as a follow-up in
//! `docs/tickets/31-direct-removal-storage.md`.
//!
//! Run with `cargo test -p storage-service --test latency -- --nocapture` to
//! see the numbers.

use std::sync::Arc;
use std::time::{Duration, Instant};
use storage_core::DeviceHandle;
use storage_platform::fake::{FakeDeviceControl, FakeFlush, FakeOpenUse, FakeWriteback, usb_stick};
use storage_platform::{DeviceAddress, PlatformEvent};
use storage_service::coordinator::Clock;
use storage_service::{PreferenceStore, StorageCoordinator};

const DEVICES: usize = 8;
const ROUNDS: usize = 200;

/// One event that produces an update must not cost more than this on any
/// machine. It is a loose bound against a regression, not a hardware figure.
const EVENT_BUDGET: Duration = Duration::from_millis(5);

fn object_path(index: usize) -> String {
    format!(
        "/org/freedesktop/UDisks2/block_devices/sd{}1",
        (b'b' + index as u8) as char
    )
}

fn temporary_root(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "better-os-storage-latency-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

async fn coordinator(label: &str) -> (StorageCoordinator<FakeDeviceControl>, Clock, FakeWriteback) {
    let devices: Vec<_> = (0..DEVICES)
        .map(|index| {
            usb_stick(
                &object_path(index),
                &format!("/dev/sd{}1", (b'b' + index as u8) as char),
                &format!("A1B2-{index:04}"),
            )
        })
        .collect();
    let writeback = FakeWriteback::idle();
    let clock = Clock::manual();
    let mut coordinator = StorageCoordinator::new(
        FakeDeviceControl::new(devices),
        Arc::new(FakeFlush::default()),
        Arc::new(writeback.clone()),
        Arc::new(FakeOpenUse::idle()),
        PreferenceStore::at_path(temporary_root(label).join("storage-preferences.json")),
        clock.clone(),
    )
    .unwrap();
    coordinator.refresh_inventory().await.unwrap();
    (coordinator, clock, writeback)
}

#[tokio::test]
async fn event_to_state_update_latency_stays_well_under_a_frame() {
    let (mut coordinator, clock, writeback) = coordinator("latency").await;
    let mut updates = coordinator.subscribe();
    let mut samples: Vec<Duration> = Vec::with_capacity(DEVICES * ROUNDS);
    let mut published = 0_u64;

    for round in 0..ROUNDS {
        // Alternate between a device that is being written to and one that has
        // gone quiet, so every round changes state and produces an update.
        let pending = if round % 2 == 0 { 4 * 1024 * 1024 } else { 0 };
        writeback.pending_bytes(pending);
        for index in 0..DEVICES {
            clock.advance(10);
            let handle = DeviceHandle::new(object_path(index));
            let started = Instant::now();
            coordinator.observe(&handle).await;
            samples.push(started.elapsed());
            while updates.try_recv().is_ok() {
                published += 1;
            }
        }
    }

    samples.sort();
    let p50 = samples[samples.len() / 2];
    let p99 = samples[samples.len() * 99 / 100];
    let worst = *samples.last().expect("samples");

    println!("devices:   {DEVICES}");
    println!("events:    {}", samples.len());
    println!("updates:   {published}");
    println!("p50:       {p50:?}");
    println!("p99:       {p99:?}");
    println!("worst:     {worst:?}");

    assert!(published > 0, "no update was ever published");
    assert!(
        p99 < EVENT_BUDGET,
        "p99 event-to-update latency was {p99:?}, over the {EVENT_BUDGET:?} budget"
    );
}

#[tokio::test]
async fn a_burst_of_connect_and_disconnect_events_is_handled_at_a_steady_cost() {
    let (mut coordinator, clock, _writeback) = coordinator("burst").await;
    let control = FakeDeviceControl::new([]);
    let _ = control;

    let started = Instant::now();
    let mut events = 0_u64;
    for round in 0..ROUNDS {
        for index in 0..DEVICES {
            clock.advance(1);
            coordinator
                .handle_event(PlatformEvent::Changed {
                    address: DeviceAddress {
                        object_path: object_path(index),
                        device_path: format!("/dev/sd{}1", (b'b' + index as u8) as char),
                    },
                })
                .await;
            events += 1;
        }
        if round == 0 {
            // The first round includes admission work; the rest is steady state.
            assert_eq!(coordinator.reports().len(), DEVICES);
        }
    }
    let elapsed = started.elapsed();
    let per_event = elapsed / u32::try_from(events).expect("event count fits");

    println!("events:    {events}");
    println!("total:     {elapsed:?}");
    println!("per event: {per_event:?}");
    assert!(per_event < EVENT_BUDGET, "an event averaged {per_event:?}");
}

#[tokio::test]
async fn an_idle_service_publishes_nothing_and_runs_no_timer() {
    let (coordinator, _clock, _writeback) = coordinator("idle").await;
    let mut updates = coordinator.subscribe();
    let before = coordinator.reports();

    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        updates.try_recv().is_err(),
        "an idle service published an update nobody asked for"
    );
    assert_eq!(coordinator.reports(), before);
}
