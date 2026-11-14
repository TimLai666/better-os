//! Per-process TCP traffic, from a recorded sock_diag reply and a recorded
//! `/proc/<pid>/fd` layout. The reply is built byte for byte the way the
//! kernel lays it out (`nlmsghdr`, `inet_diag_msg`, an `INET_DIAG_INFO`
//! attribute carrying `tcp_info`), so the test drives the same parser the
//! running service uses without opening a netlink socket.

use monitor_collectors_linux::{ProcessUsageCollector, Roots};
use std::os::unix::fs::symlink;
use std::path::Path;

fn message(inode: u32, sent: u64, received: u64) -> Vec<u8> {
    let mut info = vec![0u8; 232];
    info[120..128].copy_from_slice(&sent.to_ne_bytes());
    info[128..136].copy_from_slice(&received.to_ne_bytes());
    let mut body = vec![0u8; 72];
    body[0] = 2; // AF_INET
    body[68..72].copy_from_slice(&inode.to_ne_bytes());
    body.extend_from_slice(&((4 + info.len()) as u16).to_ne_bytes());
    body.extend_from_slice(&2u16.to_ne_bytes()); // INET_DIAG_INFO
    body.extend_from_slice(&info);
    let mut out = Vec::new();
    out.extend_from_slice(&((16 + body.len()) as u32).to_ne_bytes());
    out.extend_from_slice(&20u16.to_ne_bytes()); // SOCK_DIAG_BY_FAMILY
    out.extend_from_slice(&[0u8; 10]);
    out.extend_from_slice(&body);
    out
}

fn dump(root: &Path, sockets: &[(u32, u64, u64)]) {
    let mut bytes = Vec::new();
    for (inode, sent, received) in sockets {
        bytes.extend(message(*inode, *sent, *received));
    }
    std::fs::create_dir_all(root.join("netlink")).unwrap();
    std::fs::write(root.join("netlink/sock_diag_tcp"), bytes).unwrap();
}

fn at(seconds: u64) -> monitor_core::Timestamp {
    monitor_core::Timestamp {
        unix_ms: 1_700_000_000_000 + seconds * 1000,
        monotonic_ns: seconds * 1_000_000_000,
    }
}

#[test]
fn each_process_is_given_the_traffic_of_its_own_sockets() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    for (pid, fd, inode) in [(10u32, 3u32, 917u32), (10, 4, 918), (20, 5, 919)] {
        std::fs::create_dir_all(r.join(format!("proc/{pid}/fd"))).unwrap();
        symlink(
            format!("socket:[{inode}]"),
            r.join(format!("proc/{pid}/fd/{fd}")),
        )
        .unwrap();
    }
    std::fs::create_dir_all(r.join("proc/30/fd")).unwrap();
    // A socket nobody here owns (another user's) is attributed to no one.
    dump(
        r,
        &[(917, 1_000, 5_000), (918, 0, 0), (919, 10, 10), (999, 7, 7)],
    );
    let roots = Roots::at(r);
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[10, 20, 30]);
    assert_eq!(
        usage.latest().processes[&10].receive_rate,
        None,
        "one dump is no rate"
    );
    dump(
        r,
        &[
            (917, 3_000, 9_000),
            (918, 500, 1_000),
            (919, 10, 10),
            (999, 70, 70),
        ],
    );
    // The socket table is dumped every three seconds; a round in between
    // keeps the last rates.
    usage.sample_for(&roots, at(2), &[10, 20, 30]);
    assert_eq!(usage.latest().processes[&10].receive_rate, None);
    usage.sample_for(&roots, at(4), &[10, 20, 30]);
    let round = usage.latest();
    assert_eq!(round.processes[&10].send_rate, Some(2_500.0 / 3.0));
    assert_eq!(round.processes[&10].receive_rate, Some(5_000.0 / 3.0));
    assert_eq!(round.processes[&20].send_rate, Some(0.0));
    assert_eq!(round.processes[&30].receive_rate, Some(0.0));
}

#[test]
fn a_socket_opened_after_the_first_dump_counts_from_its_first_byte() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    std::fs::create_dir_all(r.join("proc/10/fd")).unwrap();
    dump(r, &[]);
    let roots = Roots::at(r);
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[10]);
    symlink("socket:[500]", r.join("proc/10/fd/7")).unwrap();
    dump(r, &[(500, 4_000, 8_000)]);
    // Ten seconds later, past the descriptor rescan interval.
    usage.sample_for(&roots, at(11), &[10]);
    let process = &usage.latest().processes[&10];
    assert_eq!(process.send_rate, Some(400.0));
    assert_eq!(process.receive_rate, Some(800.0));
}

#[test]
fn an_unreadable_descriptor_table_is_not_traffic_of_zero() {
    let root = tempfile::tempdir().unwrap();
    // Process 1 exists but its descriptors cannot be listed.
    std::fs::create_dir_all(root.path().join("proc/1")).unwrap();
    dump(root.path(), &[]);
    let roots = Roots::at(root.path());
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[1]);
    usage.sample_for(&roots, at(2), &[1]);
    let process = &usage.latest().processes[&1];
    assert!(!process.readable);
    assert_eq!(process.receive_rate, None);
}

#[test]
fn a_snapshot_without_a_dump_says_the_interface_is_missing_rather_than_silence() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("proc/10/fd")).unwrap();
    let roots = Roots::at(root.path());
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[10]);
    assert!(usage.latest().network_unavailable.is_some());
    assert!(usage.latest().network_absent);
    assert_eq!(usage.latest().processes[&10].receive_rate, None);
}

#[test]
fn an_unreadable_dump_degrades_the_collector() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("proc/10/fd")).unwrap();
    // A directory where the dump should be: present, but not readable as one.
    std::fs::create_dir_all(root.path().join("netlink/sock_diag_tcp")).unwrap();
    let roots = Roots::at(root.path());
    let mut usage = ProcessUsageCollector::new(roots.clone());
    let report = usage.sample_for(&roots, at(1), &[10]);
    assert!(!usage.latest().network_absent);
    assert!(matches!(
        report.health,
        monitor_core::CollectorHealth::Degraded { .. }
    ));
}

fn process_metric(
    usage: &ProcessUsageCollector,
    pid: u32,
    metric: &str,
) -> Option<monitor_core::Observation> {
    use monitor_core::{CollectorId, CollectorReport, Entity, EntityId, EntityKind, MetricSet};
    let mut report = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at(9));
    report.entities.push(Entity::new(
        EntityId::new(EntityKind::Process, pid.to_string()),
        MetricSet::new(),
    ));
    usage.enrich(&mut report);
    report.entities[0]
        .metrics
        .get(&monitor_core::MetricId::new(metric).unwrap())
        .cloned()
}

#[test]
fn without_a_graphics_card_a_process_has_no_graphics_reading_rather_than_zero() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("proc/10/fd")).unwrap();
    dump(root.path(), &[]);
    let roots = Roots::at(root.path());
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[10]);
    usage.sample_for(&roots, at(2), &[10]);
    for metric in ["process.gpu.utilization", "process.npu.utilization"] {
        assert!(
            matches!(
                process_metric(&usage, 10, metric),
                Some(monitor_core::Observation::Unsupported(_))
            ),
            "{metric}"
        );
    }
}

#[test]
fn a_process_that_exited_before_its_descriptors_were_read_is_gone_not_denied() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("proc")).unwrap();
    dump(root.path(), &[]);
    let roots = Roots::at(root.path());
    let mut usage = ProcessUsageCollector::new(roots.clone());
    // Process 44 is in the process table but has no /proc entry any more.
    usage.sample_for(&roots, at(1), &[44]);
    assert_eq!(
        process_metric(&usage, 44, "process.network.receive.rate"),
        Some(monitor_core::Observation::Unknown(
            monitor_core::UnknownReason::EntityDisappeared
        ))
    );
}
