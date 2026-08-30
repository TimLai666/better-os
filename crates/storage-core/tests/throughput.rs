//! Measured cost of the decision path itself.
//!
//! Numbers are printed so `cargo test -p storage-core --test throughput --
//! --nocapture` reproduces what the ticket records. The assertions are loose
//! bounds a much slower machine still passes: they exist to catch a state
//! machine that starts costing milliseconds per event, not to pin a figure to
//! this hardware.
//!
//! What this does not measure is real device throughput. Copy time, flush time,
//! and time-to-ready across exFAT, NTFS, and ext4 on flash, SSD, and spinning
//! external disks need the hardware; that work is recorded as a follow-up in
//! `docs/tickets/31-direct-removal-storage.md` rather than faked here.

use std::time::{Duration, Instant};
use storage_core::machine::ObservedSignals;
use storage_core::{
    DeviceEvent, DeviceHandle, DeviceIdentity, DeviceMachine, DeviceRegistry, DeviceStateKind,
    EvidencePolicy, FlushScope, FlushVerification, IdentityEvidence, OpenWriters, PendingWriteback,
    PreferenceSet, RemovalPolicy, ScanCoverage, SignalStatus, Timestamp, Transport, WritebackScope,
};

/// One synthetic device-lifetime: mount, a copy, its writeback draining, a
/// flush, an idle observation, and an unplug.
const EVENTS_PER_CYCLE: u64 = 6;
const CYCLES: u64 = 20_000;

/// A single event costing more than this on any machine means the state machine
/// has grown real work inside it.
const PER_EVENT_BUDGET: Duration = Duration::from_micros(50);

fn identity(index: u64) -> DeviceIdentity {
    DeviceIdentity::from_evidence(IdentityEvidence {
        filesystem_uuid: Some(format!("A1B2-{index:04X}")),
        drive_serial: Some(format!("SN-{index:08}")),
        transport: Transport::Usb,
        device_path: format!("/dev/sd{}1", (b'b' + (index % 20) as u8) as char),
        ..IdentityEvidence::default()
    })
}

fn cycle(at: u64) -> [DeviceEvent; EVENTS_PER_CYCLE as usize] {
    let observed = |bytes: u64, at: u64| {
        DeviceEvent::SignalsObserved(ObservedSignals {
            at: Timestamp::from_millis(at),
            mounted: true,
            writeback: SignalStatus::Observed(PendingWriteback {
                bytes,
                scope: WritebackScope::Device,
            }),
            open_writers: SignalStatus::Observed(OpenWriters {
                writers: Vec::new(),
                coverage: ScanCoverage::Complete,
            }),
        })
    };
    [
        DeviceEvent::Mounted {
            mount_point: "/run/media/user/DATA".to_string(),
        },
        DeviceEvent::OperationStarted {
            operation: "copy-1".to_string(),
        },
        observed(1 << 20, at + 1),
        DeviceEvent::OperationCompleted {
            operation: "copy-1".to_string(),
            flush: storage_core::FlushOutcome::Completed(FlushVerification {
                scope: FlushScope::Filesystem,
                completed_at: Timestamp::from_millis(at + 2),
            }),
        },
        observed(0, at + 3),
        DeviceEvent::Disconnected,
    ]
}

#[test]
fn the_state_machine_sustains_a_synthetic_event_stream_cheaply() {
    let mut ready_seen = 0_u64;
    let mut events = 0_u64;

    let started = Instant::now();
    for index in 0..CYCLES {
        let at = index * 10;
        let mut machine = DeviceMachine::connect(
            identity(index),
            RemovalPolicy::DirectRemoval,
            Timestamp::from_millis(at),
        );
        for (step, event) in cycle(at).into_iter().enumerate() {
            let transition = machine.apply(event, Timestamp::from_millis(at + step as u64));
            events += 1;
            if transition.state.kind() == DeviceStateKind::ReadyToUnplug {
                ready_seen += 1;
            }
        }
    }
    let elapsed = started.elapsed();
    let per_event = elapsed / events as u32;

    println!("cycles:       {CYCLES}");
    println!("events:       {events}");
    println!("total wall:   {elapsed:?}");
    println!("per event:    {per_event:?}");
    println!(
        "throughput:   {:.0} events/s",
        events as f64 / elapsed.as_secs_f64()
    );
    println!("ready states: {ready_seen}");

    assert_eq!(events, CYCLES * EVENTS_PER_CYCLE);
    // Every cycle reaches Ready exactly once, at the idle observation after the
    // flush. A regression that stopped producing it would still be fast.
    assert_eq!(ready_seen, CYCLES);
    assert!(
        per_event < PER_EVENT_BUDGET,
        "an event cost {per_event:?}, over the {PER_EVENT_BUDGET:?} budget"
    );
}

#[test]
fn event_to_state_update_latency_stays_in_the_microsecond_range() {
    let preferences = PreferenceSet::new();
    let mut registry = DeviceRegistry::new(EvidencePolicy::default());
    let handles: Vec<DeviceHandle> = (0..32)
        .map(|index| {
            let handle = DeviceHandle::new(format!("/dev/block/{index}"));
            registry.connect(
                handle.clone(),
                identity(index),
                &preferences,
                Timestamp::START,
            );
            handle
        })
        .collect();

    let mut samples: Vec<Duration> = Vec::with_capacity(handles.len() * 4 * 250);
    for round in 0..250_u64 {
        for handle in &handles {
            for (step, event) in cycle(round * 10).into_iter().take(4).enumerate() {
                let at = Timestamp::from_millis(round * 10 + step as u64);
                let started = Instant::now();
                let transition = registry.apply(handle, event, at);
                samples.push(started.elapsed());
                assert!(transition.is_some());
            }
        }
    }

    samples.sort();
    let p50 = samples[samples.len() / 2];
    let p99 = samples[samples.len() * 99 / 100];
    let worst = *samples.last().expect("samples");

    println!("samples: {}", samples.len());
    println!("p50:     {p50:?}");
    println!("p99:     {p99:?}");
    println!("worst:   {worst:?}");

    assert!(
        p99 < PER_EVENT_BUDGET,
        "p99 event-to-state-update latency was {p99:?}"
    );
}

#[test]
fn an_idle_registry_does_no_work_between_events() {
    // The service has no timer of its own: with no events delivered, no state
    // changes and nothing is recomputed. This is the bounded-idle-overhead
    // claim, asserted at the level where it is decided.
    let preferences = PreferenceSet::new();
    let mut registry = DeviceRegistry::new(EvidencePolicy::default());
    let handle = DeviceHandle::new("idle");
    registry.connect(handle.clone(), identity(1), &preferences, Timestamp::START);
    let before = registry.get(&handle).unwrap().state().clone();

    std::thread::sleep(Duration::from_millis(20));

    let after = registry.get(&handle).unwrap().state().clone();
    assert_eq!(before, after);
}
