//! The graphics collector and the per-process client scan, against sysfs and
//! proc trees recorded on the owner's ASUS Vivobook S 14 (amdgpu at
//! 0000:63:00.0, gnome-shell holding a render node), and against an Intel
//! card whose busy share can only come from its clients. Nothing here reads
//! the machine the test runs on.

use monitor_collectors_linux::{GpuCollector, ProcessUsageCollector, Roots};
use monitor_core::{CollectorReport, EntityKind, MetricId, Observation, Timestamp};
use std::os::unix::fs::symlink;
use std::path::Path;

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("{content}\n")).unwrap();
}

fn link(root: &Path, path: &str, target: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let target = if target.starts_with('/') {
        target.to_string()
    } else {
        root.join(target).display().to_string()
    };
    symlink(target, path).unwrap();
}

/// One graphics card laid out as sysfs lays it out: the class entry, the
/// device it links to, the driver link, and its hwmon chip.
fn card(root: &Path, card: &str, address: &str, driver: &str, files: &[(&str, &str)]) {
    let device = format!("devices/pci0000:00/{address}");
    std::fs::create_dir_all(root.join(format!("sys/class/drm/{card}"))).unwrap();
    link(
        root,
        &format!("sys/class/drm/{card}/device"),
        &format!("sys/{device}"),
    );
    std::fs::create_dir_all(root.join(format!("sys/bus/pci/drivers/{driver}"))).unwrap();
    link(
        root,
        &format!("sys/{device}/driver"),
        &format!("sys/bus/pci/drivers/{driver}"),
    );
    write(
        root,
        &format!("sys/{device}/uevent"),
        &format!("DRIVER={driver}\nPCI_SLOT_NAME={address}"),
    );
    for (path, content) in files {
        write(root, &format!("sys/{device}/{path}"), content);
    }
    // A connector is not a card.
    std::fs::create_dir_all(root.join(format!("sys/class/drm/{card}-eDP-1"))).unwrap();
}

/// Recorded from `/sys/class/drm/card1/device` on the owner's machine.
const AMDGPU: &[(&str, &str)] = &[
    ("gpu_busy_percent", "6"),
    ("mem_info_vram_used", "493719552"),
    ("mem_info_vram_total", "536870912"),
    ("mem_info_gtt_used", "2589380608"),
    ("mem_info_gtt_total", "16354619392"),
    ("vendor", "0x1002"),
    ("pp_dpm_sclk", "0: 600Mhz \n1: 1421Mhz *\n2: 2900Mhz "),
    ("pp_dpm_mclk", "0: 400Mhz \n1: 800Mhz \n2: 937Mhz *"),
    ("hwmon/hwmon10/name", "amdgpu"),
    ("hwmon/hwmon10/freq1_input", "1220000000"),
    ("hwmon/hwmon10/freq1_label", "sclk"),
    ("hwmon/hwmon10/power1_average", "27407000"),
    ("hwmon/hwmon10/power1_input", "27407000"),
    ("hwmon/hwmon10/temp1_input", "63000"),
    ("hwmon/hwmon10/temp1_label", "edge"),
];

fn fdinfo(driver: &str, pdev: &str, client: u32, engine: &str, ns: u64, vram_kib: u64) -> String {
    format!(
        "pos:\t0\nflags:\t02104002\ndrm-driver:\t{driver}\ndrm-client-id:\t{client}\n\
drm-pdev:\t{pdev}\ndrm-resident-vram:\t{vram_kib} KiB\ndrm-resident-gtt:\t1328048 KiB\n\
drm-engine-{engine}:\t{ns} ns"
    )
}

fn at(seconds: u64) -> Timestamp {
    Timestamp {
        unix_ms: 1_700_000_000_000 + seconds * 1000,
        monotonic_ns: seconds * 1_000_000_000,
    }
}

fn gpu(report: &CollectorReport, key: &str, metric: &str) -> Option<Observation> {
    report
        .entities
        .iter()
        .find(|e| e.id.kind == EntityKind::Gpu && e.id.key == key)?
        .metrics
        .get(&MetricId::new(metric).unwrap())
        .cloned()
}

#[test]
fn an_amdgpu_card_reports_busy_memory_clock_temperature_and_power() {
    let root = tempfile::tempdir().unwrap();
    card(root.path(), "card1", "0000:63:00.0", "amdgpu", AMDGPU);
    let roots = Roots::at(root.path());
    let report = GpuCollector::new(roots.clone()).sample_with(&roots, at(1), None);
    let key = "0000:63:00.0";
    assert_eq!(report.entities.len(), 1, "connectors are not cards");
    let float = |metric| gpu(&report, key, metric).and_then(|o| o.as_f64());
    assert_eq!(float("gpu.utilization"), Some(0.06));
    assert_eq!(float("gpu.memory.dedicated.used"), Some(493_719_552.0));
    assert_eq!(float("gpu.memory.dedicated.total"), Some(536_870_912.0));
    assert_eq!(float("gpu.memory.shared.used"), Some(2_589_380_608.0));
    assert_eq!(float("gpu.memory.shared.total"), Some(16_354_619_392.0));
    assert_eq!(float("gpu.temperature"), Some(63.0));
    assert_eq!(float("gpu.frequency"), Some(1.22e9));
    assert_eq!(float("gpu.memory.frequency"), Some(937e6));
    assert_eq!(float("gpu.power"), Some(27.407));
    assert_eq!(
        gpu(&report, key, "gpu.vendor"),
        Some(Observation::text("AMD"))
    );
    assert_eq!(
        gpu(&report, key, "gpu.driver"),
        Some(Observation::text("amdgpu"))
    );
    assert_eq!(
        gpu(&report, key, "gpu.utilization.source"),
        Some(Observation::text("driver"))
    );
}

#[test]
fn a_process_holding_a_render_node_is_given_its_engine_time_and_memory() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    card(r, "card1", "0000:63:00.0", "amdgpu", AMDGPU);
    write(r, "proc/2438/stat", "2438 (gnome-shell) S 1 0 0 0");
    link(r, "proc/2438/fd/12", "/dev/dri/renderD128");
    link(r, "proc/2438/fd/13", "/dev/dri/renderD128");
    link(r, "proc/2438/fd/3", "/home/someone/file");
    let info = |ns| fdinfo("amdgpu", "0000:63:00.0", 14, "gfx", ns, 195_608);
    write(r, "proc/2438/fdinfo/12", &info(365_000_000_000));
    // The same client through a second descriptor is counted once.
    write(r, "proc/2438/fdinfo/13", &info(365_000_000_000));
    let roots = Roots::at(r);
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[2438]);
    write(r, "proc/2438/fdinfo/12", &info(365_250_000_000));
    write(r, "proc/2438/fdinfo/13", &info(365_250_000_000));
    usage.sample_for(&roots, at(2), &[2438]);
    let round = usage.latest();
    let process = &round.processes[&2438];
    let device = &process.gpu["0000:63:00.0"];
    assert_eq!(device.busy, Some(0.25), "0.25 s of gfx time in 1 s");
    assert_eq!(device.dedicated_bytes, 195_608 * 1024);
    assert_eq!(device.shared_bytes, 1_328_048 * 1024);
    assert_eq!(round.devices["0000:63:00.0"].clients, 1);

    let report = GpuCollector::new(roots.clone()).sample_with(&roots, at(2), Some(round));
    assert_eq!(
        gpu(&report, "0000:63:00.0", "gpu.clients"),
        Some(Observation::unsigned(1))
    );
}

#[test]
fn an_intel_card_is_as_busy_as_its_clients_made_it() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    card(
        r,
        "card0",
        "0000:00:02.0",
        "i915",
        &[
            ("vendor", "0x8086"),
            ("hwmon/hwmon3/name", "i915"),
            ("hwmon/hwmon3/energy1_input", "1000000"),
        ],
    );
    write(r, "sys/class/drm/card0/gt_cur_freq_mhz", "1100");
    for pid in [100u32, 200] {
        link(r, &format!("proc/{pid}/fd/5"), "/dev/dri/renderD128");
    }
    let info = |client, ns| fdinfo("i915", "0000:00:02.0", client, "render", ns, 0);
    write(r, "proc/100/fdinfo/5", &info(1, 0));
    write(r, "proc/200/fdinfo/5", &info(2, 0));
    let roots = Roots::at(r);
    let mut usage = ProcessUsageCollector::new(roots.clone());
    let mut collector = GpuCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[100, 200]);
    collector.sample_with(&roots, at(1), Some(usage.latest()));
    write(r, "proc/100/fdinfo/5", &info(1, 300_000_000));
    write(r, "proc/200/fdinfo/5", &info(2, 200_000_000));
    write(
        r,
        "sys/devices/pci0000:00/0000:00:02.0/hwmon/hwmon3/energy1_input",
        "6000000",
    );
    usage.sample_for(&roots, at(2), &[100, 200]);
    let report = collector.sample_with(&roots, at(2), Some(usage.latest()));
    let float = |metric| gpu(&report, "0000:00:02.0", metric).and_then(|o| o.as_f64());
    assert!((float("gpu.utilization").unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(
        gpu(&report, "0000:00:02.0", "gpu.utilization.source"),
        Some(Observation::text("clients"))
    );
    assert_eq!(float("gpu.frequency"), Some(1.1e9));
    // 5 J over 1 s.
    assert_eq!(float("gpu.power"), Some(5.0));
    assert_eq!(
        gpu(&report, "0000:00:02.0", "gpu.vendor"),
        Some(Observation::text("Intel"))
    );
}

#[test]
fn an_nvidia_card_in_a_snapshot_is_named_and_says_why_it_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    card(
        root.path(),
        "card0",
        "0000:01:00.0",
        "nvidia",
        &[("vendor", "0x10de")],
    );
    let roots = Roots::at(root.path());
    let report = GpuCollector::new(roots.clone()).sample_with(&roots, at(1), None);
    assert_eq!(
        gpu(&report, "0000:01:00.0", "gpu.vendor"),
        Some(Observation::text("NVIDIA"))
    );
    assert!(matches!(
        gpu(&report, "0000:01:00.0", "gpu.utilization"),
        Some(Observation::Unsupported(_))
    ));
}

#[test]
fn a_machine_without_drm_shows_no_card() {
    let root = tempfile::tempdir().unwrap();
    let roots = Roots::at(root.path());
    let report = GpuCollector::new(roots.clone()).sample_with(&roots, at(1), None);
    assert!(report.entities.is_empty());
}

#[test]
fn a_powered_down_card_is_asleep_and_its_busy_file_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    // `gpu_busy_percent` is left unreadable-looking on purpose: a value of
    // 99 would show if it were read.
    let mut files = AMDGPU.to_vec();
    files.push(("power/runtime_status", "suspended"));
    files.retain(|(path, _)| *path != "gpu_busy_percent");
    files.push(("gpu_busy_percent", "99"));
    card(root.path(), "card1", "0000:03:00.0", "amdgpu", &files);
    let roots = Roots::at(root.path());
    let report = GpuCollector::new(roots.clone()).sample_with(&roots, at(1), None);
    let key = "0000:03:00.0";
    assert_eq!(
        gpu(&report, key, "gpu.utilization"),
        Some(Observation::float(0.0))
    );
    assert_eq!(
        gpu(&report, key, "gpu.utilization.source"),
        Some(Observation::text("asleep"))
    );
    assert_eq!(
        gpu(&report, key, "gpu.temperature"),
        None,
        "the sensors were not read"
    );
    assert_eq!(gpu(&report, key, "gpu.memory.dedicated.used"), None);
}

#[test]
fn a_card_whose_driver_publishes_no_engine_time_is_not_called_idle() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    card(
        r,
        "card0",
        "0000:00:02.0",
        "nouveau",
        &[("vendor", "0x10de")],
    );
    std::fs::create_dir_all(r.join("proc")).unwrap();
    let roots = Roots::at(r);
    let mut usage = ProcessUsageCollector::new(roots.clone());
    let mut collector = GpuCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[]);
    collector.sample_with(&roots, at(1), Some(usage.latest()));
    usage.sample_for(&roots, at(2), &[]);
    let report = collector.sample_with(&roots, at(2), Some(usage.latest()));
    assert!(matches!(
        gpu(&report, "0000:00:02.0", "gpu.utilization"),
        Some(Observation::Unsupported(_))
    ));
}

#[test]
fn a_powered_down_nvidia_card_is_asleep_without_asking_its_library() {
    let root = tempfile::tempdir().unwrap();
    card(
        root.path(),
        "card0",
        "0000:01:00.0",
        "nvidia",
        &[("vendor", "0x10de"), ("power/runtime_status", "suspended")],
    );
    let roots = Roots::at(root.path());
    let report = GpuCollector::new(roots.clone()).sample_with(&roots, at(1), None);
    assert_eq!(
        gpu(&report, "0000:01:00.0", "gpu.utilization.source"),
        Some(Observation::text("asleep"))
    );
    assert_eq!(
        gpu(&report, "0000:01:00.0", "gpu.utilization"),
        Some(Observation::float(0.0))
    );
}

#[test]
fn only_the_awake_nvidia_cards_are_named_to_its_library() {
    let root = tempfile::tempdir().unwrap();
    card(
        root.path(),
        "card0",
        "0000:01:00.0",
        "nvidia",
        &[("vendor", "0x10de"), ("power/runtime_status", "suspended")],
    );
    card(
        root.path(),
        "card1",
        "0000:02:00.0",
        "nvidia",
        &[("vendor", "0x10de"), ("power/runtime_status", "active")],
    );
    card(
        root.path(),
        "card2",
        "0000:63:00.0",
        "amdgpu",
        &[("vendor", "0x1002")],
    );
    let cards = monitor_collectors_linux::gpu::list_cards(&Roots::at(root.path())).unwrap();
    assert_eq!(cards.len(), 3);
    // The sleeping card is never opened, so asking cannot wake it.
    assert_eq!(
        monitor_collectors_linux::gpu::nvml_targets(&cards),
        vec!["0000:02:00.0".to_string()]
    );
}

#[test]
fn a_program_on_a_card_that_publishes_no_client_time_is_not_reported() {
    use monitor_core::{CollectorId, CollectorReport, Entity, EntityId, MetricSet};
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    card(
        r,
        "card0",
        "0000:01:00.0",
        "nvidia",
        &[("vendor", "0x10de")],
    );
    std::fs::create_dir_all(r.join("proc/10/fd")).unwrap();
    let roots = Roots::at(r);
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[10]);
    usage.sample_for(&roots, at(2), &[10]);
    let mut table = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at(2));
    table.entities.push(Entity::new(
        EntityId::new(EntityKind::Process, "10"),
        MetricSet::new(),
    ));
    usage.enrich(&mut table);
    assert!(matches!(
        table.entities[0]
            .metrics
            .get(&MetricId::new("process.gpu.utilization").unwrap()),
        Some(Observation::Unsupported(_))
    ));
}
