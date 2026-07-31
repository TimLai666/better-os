use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use sysinfo::{Pid, System};

#[derive(Clone, Debug, Default)]
pub struct CpuDetails {
    pub model_name: Option<String>,
    pub architecture: String,
    pub logical_cpus: usize,
    pub physical_cpus: Option<usize>,
    pub sockets: Option<usize>,
    pub max_speed_mhz: Option<u64>,
    pub virtualization: Option<String>,
    pub temperature_c: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct ProcessExtra {
    pub user: Option<String>,
    pub uid: Option<u32>,
    pub command_line: String,
    pub executable: Option<String>,
    pub working_directory: Option<String>,
    pub cgroup: Option<String>,
    pub app_id: Option<String>,
    pub parent_pid: Option<u32>,
    pub threads: Option<u64>,
    pub file_descriptors: Option<usize>,
    pub swap_bytes: Option<u64>,
    pub total_cpu_time_ticks: Option<u64>,
    pub user_cpu_time_ticks: Option<u64>,
    pub system_cpu_time_ticks: Option<u64>,
    pub priority: Option<i64>,
    pub nice: Option<i64>,
    pub start_time_ticks: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct AppProcessSample {
    pub pid: Pid,
    pub name: String,
    pub cpu_usage: f32,
    pub memory: u64,
    pub swap: u64,
    pub read_speed: u64,
    pub read_total: u64,
    pub write_speed: u64,
    pub write_total: u64,
    pub app_id: Option<String>,
    pub cgroup: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AppGroup {
    pub id: String,
    pub display_name: String,
    pub process_count: usize,
    pub pids: Vec<Pid>,
    pub cpu_usage: f32,
    pub memory: u64,
    pub swap: u64,
    pub read_speed: u64,
    pub read_total: u64,
    pub write_speed: u64,
    pub write_total: u64,
    pub grouping_reason: String,
}

#[derive(Clone, Debug, Default)]
pub struct GpuDevice {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub driver: String,
    pub pci_slot: String,
    pub usage_percent: Option<f64>,
    pub encode_percent: Option<f64>,
    pub decode_percent: Option<f64>,
    pub memory_total: Option<u64>,
    pub memory_used: Option<u64>,
    pub temperature_c: Option<f64>,
    pub power_watts: Option<f64>,
    pub gpu_clock_mhz: Option<f64>,
    pub memory_clock_mhz: Option<f64>,
    pub max_power_watts: Option<f64>,
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct NpuDevice {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub driver: String,
    pub pci_slot: String,
    pub usage_percent: Option<f64>,
    pub memory_total: Option<u64>,
    pub memory_used: Option<u64>,
    pub temperature_c: Option<f64>,
    pub power_watts: Option<f64>,
    pub clock_mhz: Option<f64>,
    pub memory_clock_mhz: Option<f64>,
    pub max_power_watts: Option<f64>,
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BatteryDevice {
    pub id: String,
    pub name: String,
    pub charge_percent: Option<f64>,
    pub state: Option<String>,
    pub power_watts: Option<f64>,
    pub health_percent: Option<f64>,
    pub design_capacity_wh: Option<f64>,
    pub charge_cycles: Option<u64>,
    pub technology: Option<String>,
    pub manufacturer: Option<String>,
    pub model_name: Option<String>,
    pub device: String,
}

#[derive(Clone, Debug, Default)]
pub struct NetworkMetadata {
    pub interface: String,
    pub interface_type: String,
    pub manufacturer: Option<String>,
    pub driver: Option<String>,
    pub hardware_address: Option<String>,
    pub network_name: Option<String>,
    pub link: Option<String>,
    pub link_speed_mbps: Option<u64>,
    pub is_virtual: bool,
    pub state: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BlockMetadata {
    pub device: String,
    pub model: Option<String>,
    pub drive_type: String,
    pub writable: Option<bool>,
    pub removable: Option<bool>,
    pub link: Option<String>,
    pub is_virtual: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BlockCounters {
    pub read_sectors: u64,
    pub write_sectors: u64,
    pub read_ticks_ms: u64,
    pub write_ticks_ms: u64,
    pub io_ticks_ms: u64,
}

pub fn cpu_details(system: &System) -> CpuDetails {
    let cpuinfo = read_string("/proc/cpuinfo").unwrap_or_default();
    let mut model_name = None;
    let mut physical_ids = Vec::new();
    let mut socket_ids = Vec::new();
    let mut flags = String::new();

    for line in cpuinfo.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "model name" | "Hardware" if model_name.is_none() => {
                model_name = Some(value.to_string())
            }
            "core id" => physical_ids.push(value.to_string()),
            "physical id" => socket_ids.push(value.to_string()),
            "flags" | "Features" if flags.is_empty() => flags = value.to_string(),
            _ => {}
        }
    }

    physical_ids.sort();
    physical_ids.dedup();
    socket_ids.sort();
    socket_ids.dedup();

    let max_speed_mhz = read_number::<u64>("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .map(|khz| khz / 1_000)
        .or_else(|| system.cpus().iter().map(|cpu| cpu.frequency()).max());

    let virtualization = if flags.split_whitespace().any(|flag| flag == "vmx") {
        Some("Intel VT-x".to_string())
    } else if flags.split_whitespace().any(|flag| flag == "svm") {
        Some("AMD-V".to_string())
    } else {
        None
    };

    CpuDetails {
        model_name,
        architecture: std::env::consts::ARCH.to_string(),
        logical_cpus: system.cpus().len(),
        physical_cpus: System::physical_core_count()
            .or_else(|| (!physical_ids.is_empty()).then_some(physical_ids.len())),
        sockets: (!socket_ids.is_empty()).then_some(socket_ids.len()),
        max_speed_mhz,
        virtualization,
        temperature_c: highest_temperature_c(),
    }
}

pub fn process_extra(pid: Pid, users: &HashMap<u32, String>) -> ProcessExtra {
    let pid_u32 = pid.as_u32();
    let base = PathBuf::from("/proc").join(pid_u32.to_string());
    let status = read_string(base.join("status")).unwrap_or_default();
    let stat = read_string(base.join("stat")).unwrap_or_default();
    let command_line = fs::read(base.join("cmdline"))
        .ok()
        .map(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let cgroup =
        read_string(base.join("cgroup")).and_then(|content| parse_unified_cgroup(&content));
    let uid = status_value(&status, "Uid")
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u32>().ok());
    let threads = status_value(&status, "Threads").and_then(|value| value.parse::<u64>().ok());
    let swap_bytes = status_value(&status, "VmSwap").and_then(parse_kib_value);
    let file_descriptors = fs::read_dir(base.join("fd"))
        .ok()
        .map(|entries| entries.count());
    let executable = fs::read_link(base.join("exe"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    let working_directory = fs::read_link(base.join("cwd"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    let stat_fields = parse_proc_stat(&stat);
    let app_id = cgroup.as_deref().and_then(app_id_from_cgroup);

    ProcessExtra {
        user: uid.and_then(|id| users.get(&id).cloned()),
        uid,
        command_line,
        executable,
        working_directory,
        cgroup,
        app_id,
        parent_pid: stat_fields.as_ref().and_then(|fields| fields.parent_pid),
        threads,
        file_descriptors,
        swap_bytes,
        total_cpu_time_ticks: stat_fields
            .as_ref()
            .map(|fields| fields.user_ticks.saturating_add(fields.system_ticks)),
        user_cpu_time_ticks: stat_fields.as_ref().map(|fields| fields.user_ticks),
        system_cpu_time_ticks: stat_fields.as_ref().map(|fields| fields.system_ticks),
        priority: stat_fields.as_ref().and_then(|fields| fields.priority),
        nice: stat_fields.as_ref().and_then(|fields| fields.nice),
        start_time_ticks: stat_fields.as_ref().map(|fields| fields.start_time_ticks),
    }
}

pub fn users_by_id() -> HashMap<u32, String> {
    let mut users = HashMap::new();
    let Some(passwd) = read_string("/etc/passwd") else {
        return users;
    };
    for line in passwd.lines() {
        let mut fields = line.split(':');
        let Some(name) = fields.next() else {
            continue;
        };
        let _password = fields.next();
        let Some(uid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        users.insert(uid, name.to_string());
    }
    users
}

pub fn group_apps(samples: &[AppProcessSample]) -> Vec<AppGroup> {
    let mut groups: BTreeMap<String, AppGroup> = BTreeMap::new();

    for process in samples {
        let (id, display_name, grouping_reason) = if let Some(app_id) = &process.app_id {
            (
                app_id.clone(),
                prettify_app_id(app_id),
                "cgroup v2 / systemd application identity".to_string(),
            )
        } else if let Some(cgroup) = &process.cgroup {
            (
                format!("cgroup:{cgroup}"),
                prettify_app_id(cgroup.rsplit('/').next().unwrap_or(cgroup)),
                "cgroup membership".to_string(),
            )
        } else {
            (
                format!("process:{}", process.name.to_lowercase()),
                process.name.clone(),
                "executable-name fallback".to_string(),
            )
        };

        let group = groups.entry(id.clone()).or_insert_with(|| AppGroup {
            id,
            display_name,
            grouping_reason,
            ..AppGroup::default()
        });
        group.process_count += 1;
        group.pids.push(process.pid);
        group.cpu_usage += process.cpu_usage;
        group.memory = group.memory.saturating_add(process.memory);
        group.swap = group.swap.saturating_add(process.swap);
        group.read_speed = group.read_speed.saturating_add(process.read_speed);
        group.read_total = group.read_total.saturating_add(process.read_total);
        group.write_speed = group.write_speed.saturating_add(process.write_speed);
        group.write_total = group.write_total.saturating_add(process.write_total);
    }

    let mut result = groups.into_values().collect::<Vec<_>>();
    result.sort_by(|a, b| {
        b.cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.memory.cmp(&a.memory))
    });
    result
}

pub fn scan_gpus() -> Vec<GpuDevice> {
    let mut devices = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return devices;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device_path = entry.path().join("device");
        if !device_path.exists() {
            continue;
        }
        let canonical = fs::canonicalize(&device_path).unwrap_or_else(|_| device_path.clone());
        let pci_slot = canonical
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string());
        let vendor_id = read_hex_u32(device_path.join("vendor"));
        let device_id = read_hex_u32(device_path.join("device"));
        let manufacturer = vendor_name(vendor_id);
        let driver = fs::read_link(device_path.join("driver"))
            .ok()
            .and_then(|path| path.file_name().map(OsStr::to_owned))
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".to_string());
        let product_name = read_string(device_path.join("product_name"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| match device_id {
                Some(id) => format!("{manufacturer} GPU {id:04x}"),
                None => format!("{manufacturer} GPU"),
            });
        let usage_percent = read_number::<f64>(device_path.join("gpu_busy_percent"));
        let memory_total = read_number::<u64>(device_path.join("mem_info_vram_total"));
        let memory_used = read_number::<u64>(device_path.join("mem_info_vram_used"));
        let temperature_c = highest_hwmon_value(&device_path, "temp1_input", 1_000.0);
        let power_watts = highest_hwmon_value(&device_path, "power1_average", 1_000_000.0)
            .or_else(|| highest_hwmon_value(&device_path, "power1_input", 1_000_000.0));
        let gpu_clock_mhz = highest_hwmon_value(&device_path, "freq1_input", 1_000_000.0)
            .or_else(|| parse_pp_dpm_clock(device_path.join("pp_dpm_sclk")));
        let memory_clock_mhz = highest_hwmon_value(&device_path, "freq2_input", 1_000_000.0)
            .or_else(|| parse_pp_dpm_clock(device_path.join("pp_dpm_mclk")));
        let max_power_watts =
            read_number::<f64>(device_path.join("power_dpm_force_performance_level")).and(None);
        let link = pci_link(&device_path);

        devices.push(GpuDevice {
            id: name.to_string(),
            name: product_name,
            manufacturer,
            driver,
            pci_slot,
            usage_percent,
            encode_percent: read_number::<f64>(device_path.join("video_busy_percent")),
            decode_percent: None,
            memory_total,
            memory_used,
            temperature_c,
            power_watts,
            gpu_clock_mhz,
            memory_clock_mhz,
            max_power_watts,
            link,
        });
    }

    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices
}

pub fn scan_npus() -> Vec<NpuDevice> {
    let mut devices = Vec::new();
    let class_roots = ["/sys/class/accel", "/sys/class/misc"];

    for root in class_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            let lowercase = id.to_lowercase();
            if root.ends_with("misc")
                && !["npu", "ivpu", "xdna", "vpu"]
                    .iter()
                    .any(|needle| lowercase.contains(needle))
            {
                continue;
            }
            let device_path = entry.path().join("device");
            if !device_path.exists() {
                continue;
            }
            let canonical = fs::canonicalize(&device_path).unwrap_or_else(|_| device_path.clone());
            let pci_slot = canonical
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| id.clone());
            let vendor_id = read_hex_u32(device_path.join("vendor"));
            let manufacturer = vendor_name(vendor_id);
            let driver = fs::read_link(device_path.join("driver"))
                .ok()
                .and_then(|path| path.file_name().map(OsStr::to_owned))
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unknown".to_string());
            let device_id = read_hex_u32(device_path.join("device"));
            let name = device_id.map_or_else(
                || format!("{manufacturer} NPU"),
                |value| format!("{manufacturer} NPU {value:04x}"),
            );
            devices.push(NpuDevice {
                id,
                name,
                manufacturer,
                driver,
                pci_slot,
                usage_percent: read_number::<f64>(device_path.join("busy_percent"))
                    .or_else(|| read_number::<f64>(device_path.join("utilization"))),
                memory_total: read_number::<u64>(device_path.join("mem_info_total")),
                memory_used: read_number::<u64>(device_path.join("mem_info_used")),
                temperature_c: highest_hwmon_value(&device_path, "temp1_input", 1_000.0),
                power_watts: highest_hwmon_value(&device_path, "power1_average", 1_000_000.0),
                clock_mhz: highest_hwmon_value(&device_path, "freq1_input", 1_000_000.0),
                memory_clock_mhz: highest_hwmon_value(&device_path, "freq2_input", 1_000_000.0),
                max_power_watts: highest_hwmon_value(&device_path, "power1_cap_max", 1_000_000.0),
                link: pci_link(&device_path),
            });
        }
    }

    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices.dedup_by(|a, b| a.pci_slot == b.pci_slot && a.driver == b.driver);
    devices
}

pub fn scan_batteries() -> Vec<BatteryDevice> {
    let mut batteries = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
        return batteries;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if read_trimmed(path.join("type")).as_deref() != Some("Battery") {
            continue;
        }
        let device = entry.file_name().to_string_lossy().into_owned();
        let manufacturer = read_trimmed(path.join("manufacturer"));
        let model_name = read_trimmed(path.join("model_name"));
        let name = model_name
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| device.clone());
        let charge_percent = read_number::<f64>(path.join("capacity"));
        let energy_full = read_number::<f64>(path.join("energy_full"));
        let energy_design = read_number::<f64>(path.join("energy_full_design"));
        let charge_full = read_number::<f64>(path.join("charge_full"));
        let charge_design = read_number::<f64>(path.join("charge_full_design"));
        let health_percent = match (energy_full, energy_design, charge_full, charge_design) {
            (Some(full), Some(design), _, _) if design > 0.0 => Some(full / design * 100.0),
            (_, _, Some(full), Some(design)) if design > 0.0 => Some(full / design * 100.0),
            _ => None,
        };
        let design_capacity_wh = energy_design.map(|value| value / 1_000_000.0).or_else(|| {
            let voltage = read_number::<f64>(path.join("voltage_min_design"))?;
            let charge = charge_design?;
            Some(voltage * charge / 1_000_000_000_000.0)
        });
        let power_watts = read_number::<f64>(path.join("power_now"))
            .map(|value| value / 1_000_000.0)
            .or_else(|| {
                let current = read_number::<f64>(path.join("current_now"))?;
                let voltage = read_number::<f64>(path.join("voltage_now"))?;
                Some(current * voltage / 1_000_000_000_000.0)
            });

        batteries.push(BatteryDevice {
            id: device.clone(),
            name,
            charge_percent,
            state: read_trimmed(path.join("status")),
            power_watts,
            health_percent,
            design_capacity_wh,
            charge_cycles: read_number::<u64>(path.join("cycle_count")),
            technology: read_trimmed(path.join("technology")),
            manufacturer,
            model_name,
            device,
        });
    }

    batteries.sort_by(|a, b| a.id.cmp(&b.id));
    batteries
}

pub fn network_metadata(interface: &str) -> NetworkMetadata {
    let path = PathBuf::from("/sys/class/net").join(interface);
    let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let is_virtual = canonical.to_string_lossy().contains("/virtual/");
    let interface_type = if path.join("wireless").exists() {
        "Wi-Fi"
    } else if interface.starts_with("wl") {
        "Wi-Fi"
    } else if interface.starts_with("ww") {
        "Mobile broadband"
    } else if interface == "lo" {
        "Loopback"
    } else if interface.starts_with("br") || interface.starts_with("docker") {
        "Bridge"
    } else if interface.starts_with("tun")
        || interface.starts_with("tap")
        || interface.starts_with("wg")
    {
        "Tunnel"
    } else {
        "Ethernet"
    };
    let device_path = path.join("device");
    let vendor_id = read_hex_u32(device_path.join("vendor"));
    let driver = fs::read_link(device_path.join("driver"))
        .ok()
        .and_then(|path| path.file_name().map(OsStr::to_owned))
        .map(|value| value.to_string_lossy().into_owned());
    let speed = read_number::<u64>(path.join("speed"));

    NetworkMetadata {
        interface: interface.to_string(),
        interface_type: interface_type.to_string(),
        manufacturer: vendor_id.map(|_| vendor_name(vendor_id)),
        driver,
        hardware_address: read_trimmed(path.join("address")),
        network_name: None,
        link: read_trimmed(path.join("operstate")),
        link_speed_mbps: speed,
        is_virtual,
        state: read_trimmed(path.join("operstate")),
    }
}

pub fn block_metadata(device: &str) -> BlockMetadata {
    let device_name = block_parent_name(device);
    let path = PathBuf::from("/sys/class/block").join(&device_name);
    let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let is_virtual = canonical.to_string_lossy().contains("/virtual/");
    let rotational = read_number::<u64>(path.join("queue/rotational"));
    let drive_type = if is_virtual {
        "Virtual"
    } else if device_name.starts_with("nvme") {
        "NVMe"
    } else if rotational == Some(1) {
        "HDD"
    } else {
        "SSD"
    };
    let link = if device_name.starts_with("nvme") {
        Some("PCIe / NVMe".to_string())
    } else if canonical.to_string_lossy().contains("/usb") {
        Some("USB".to_string())
    } else if canonical.to_string_lossy().contains("/ata") {
        Some("SATA".to_string())
    } else {
        None
    };

    BlockMetadata {
        device: device_name,
        model: read_trimmed(path.join("device/model")),
        drive_type: drive_type.to_string(),
        writable: read_number::<u64>(path.join("ro")).map(|value| value == 0),
        removable: read_number::<u64>(path.join("removable")).map(|value| value != 0),
        link,
        is_virtual,
    }
}

pub fn block_counters() -> HashMap<String, BlockCounters> {
    let mut result = HashMap::new();
    let Some(content) = read_string("/proc/diskstats") else {
        return result;
    };
    for line in content.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 14 {
            continue;
        }
        let name = fields[2].to_string();
        let parse = |index: usize| {
            fields
                .get(index)
                .and_then(|value| value.parse::<u64>().ok())
        };
        let Some(read_sectors) = parse(5) else {
            continue;
        };
        let Some(read_ticks_ms) = parse(6) else {
            continue;
        };
        let Some(write_sectors) = parse(9) else {
            continue;
        };
        let Some(write_ticks_ms) = parse(10) else {
            continue;
        };
        let Some(io_ticks_ms) = parse(12) else {
            continue;
        };
        result.insert(
            name,
            BlockCounters {
                read_sectors,
                write_sectors,
                read_ticks_ms,
                write_ticks_ms,
                io_ticks_ms,
            },
        );
    }
    result
}

pub fn format_temperature(value_c: f64, unit: crate::settings::TemperatureUnit) -> String {
    match unit {
        crate::settings::TemperatureUnit::Celsius => format!("{value_c:.0} °C"),
        crate::settings::TemperatureUnit::Fahrenheit => {
            format!("{:.0} °F", value_c * 9.0 / 5.0 + 32.0)
        }
        crate::settings::TemperatureUnit::Kelvin => format!("{:.0} K", value_c + 273.15),
    }
}

pub fn format_bytes(bytes: u64, base: crate::settings::UnitBase) -> String {
    let unit = match base {
        crate::settings::UnitBase::Decimal => 1_000_u64,
        crate::settings::UnitBase::Binary => 1_024_u64,
    };
    let labels = match base {
        crate::settings::UnitBase::Decimal => ["B", "kB", "MB", "GB", "TB"],
        crate::settings::UnitBase::Binary => ["B", "KiB", "MiB", "GiB", "TiB"],
    };
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= unit as f64 && index < labels.len() - 1 {
        value /= unit as f64;
        index += 1;
    }
    if index == 0 {
        format!("{bytes} {}", labels[index])
    } else {
        format!("{value:.1} {}", labels[index])
    }
}

pub fn format_rate(bytes_per_second: u64, bits: bool, base: crate::settings::UnitBase) -> String {
    if bits {
        let bits_per_second = bytes_per_second.saturating_mul(8);
        let unit = match base {
            crate::settings::UnitBase::Decimal => 1_000_u64,
            crate::settings::UnitBase::Binary => 1_024_u64,
        };
        let labels = ["bit/s", "kbit/s", "Mbit/s", "Gbit/s", "Tbit/s"];
        let mut value = bits_per_second as f64;
        let mut index = 0;
        while value >= unit as f64 && index < labels.len() - 1 {
            value /= unit as f64;
            index += 1;
        }
        if index == 0 {
            format!("{bits_per_second} {}", labels[index])
        } else {
            format!("{value:.1} {}", labels[index])
        }
    } else {
        format!("{}/s", format_bytes(bytes_per_second, base))
    }
}

#[derive(Clone, Debug)]
struct ProcStatFields {
    parent_pid: Option<u32>,
    user_ticks: u64,
    system_ticks: u64,
    priority: Option<i64>,
    nice: Option<i64>,
    start_time_ticks: u64,
}

fn parse_proc_stat(content: &str) -> Option<ProcStatFields> {
    let close = content.rfind(')')?;
    let tail = content.get(close + 1..)?.trim();
    let fields = tail.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 20 {
        return None;
    }
    Some(ProcStatFields {
        parent_pid: fields.get(1).and_then(|value| value.parse::<u32>().ok()),
        user_ticks: fields.get(11)?.parse::<u64>().ok()?,
        system_ticks: fields.get(12)?.parse::<u64>().ok()?,
        priority: fields.get(15).and_then(|value| value.parse::<i64>().ok()),
        nice: fields.get(16).and_then(|value| value.parse::<i64>().ok()),
        start_time_ticks: fields.get(19)?.parse::<u64>().ok()?,
    })
}

fn parse_unified_cgroup(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields.next()?;
        let controllers = fields.next()?;
        let path = fields.next()?;
        if hierarchy == "0" && controllers.is_empty() {
            Some(path.to_string())
        } else {
            None
        }
    })
}

fn app_id_from_cgroup(cgroup: &str) -> Option<String> {
    for segment in cgroup.rsplit('/') {
        let candidate = segment
            .strip_suffix(".scope")
            .or_else(|| segment.strip_suffix(".service"))
            .unwrap_or(segment);
        if let Some(value) = candidate.strip_prefix("app-") {
            let value = value
                .split('@')
                .next()
                .unwrap_or(value)
                .trim_matches('-')
                .replace("\\x2d", "-");
            if !value.is_empty() {
                return Some(value);
            }
        }
        if let Some(value) = candidate.strip_prefix("flatpak-") {
            let value = value.split('-').next().unwrap_or(value);
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn prettify_app_id(value: &str) -> String {
    let raw = value
        .trim_end_matches(".scope")
        .trim_end_matches(".service")
        .replace("\\x2d", "-")
        .replace(['_', '-'], " ");
    let last = raw.rsplit('.').next().unwrap_or(&raw);
    let mut output = String::new();
    for (index, word) in last.split_whitespace().enumerate() {
        if index > 0 {
            output.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            output.extend(first.to_uppercase());
            output.extend(chars);
        }
    }
    if output.is_empty() {
        value.to_string()
    } else {
        output
    }
}

fn status_value<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate == key).then_some(value.trim())
    })
}

fn parse_kib_value(value: &str) -> Option<u64> {
    value
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
        .map(|kib| kib.saturating_mul(1_024))
}

fn highest_temperature_c() -> Option<f64> {
    let mut temperatures = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let value = read_number::<f64>(entry.path().join("temp"));
            if let Some(value) = value {
                let celsius = if value > 1_000.0 {
                    value / 1_000.0
                } else {
                    value
                };
                if (-50.0..=200.0).contains(&celsius) {
                    temperatures.push(celsius);
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            for index in 1..=16 {
                if let Some(value) =
                    read_number::<f64>(entry.path().join(format!("temp{index}_input")))
                {
                    let celsius = value / 1_000.0;
                    if (-50.0..=200.0).contains(&celsius) {
                        temperatures.push(celsius);
                    }
                }
            }
        }
    }
    temperatures.into_iter().reduce(f64::max)
}

fn highest_hwmon_value(device_path: &Path, file_name: &str, divisor: f64) -> Option<f64> {
    let hwmon = device_path.join("hwmon");
    let entries = fs::read_dir(hwmon).ok()?;
    entries
        .flatten()
        .filter_map(|entry| read_number::<f64>(entry.path().join(file_name)))
        .map(|value| value / divisor)
        .reduce(f64::max)
}

fn parse_pp_dpm_clock(path: PathBuf) -> Option<f64> {
    let content = read_string(path)?;
    content.lines().find_map(|line| {
        if !line.contains('*') {
            return None;
        }
        line.split_whitespace().find_map(|word| {
            word.trim_end_matches("Mhz")
                .trim_end_matches("MHz")
                .parse::<f64>()
                .ok()
        })
    })
}

fn pci_link(device_path: &Path) -> Option<String> {
    let speed = read_trimmed(device_path.join("current_link_speed"));
    let width = read_trimmed(device_path.join("current_link_width"));
    match (speed, width) {
        (Some(speed), Some(width)) => Some(format!("PCIe {speed} ×{width}")),
        (Some(speed), None) => Some(format!("PCIe {speed}")),
        (None, Some(width)) => Some(format!("PCIe ×{width}")),
        (None, None) => None,
    }
}

fn vendor_name(vendor: Option<u32>) -> String {
    match vendor {
        Some(0x1002) | Some(0x1022) => "AMD".to_string(),
        Some(0x8086) => "Intel".to_string(),
        Some(0x10de) => "NVIDIA".to_string(),
        Some(0x1a03) => "ASPEED".to_string(),
        Some(value) => format!("PCI vendor {value:04x}"),
        None => "Unknown".to_string(),
    }
}

fn block_parent_name(device: &str) -> String {
    let value = device.trim_start_matches("/dev/");
    if value.starts_with("nvme") || value.starts_with("mmcblk") {
        value
            .rfind('p')
            .and_then(|index| value.get(index + 1..).map(|suffix| (index, suffix)))
            .filter(|(_, suffix)| suffix.chars().all(|character| character.is_ascii_digit()))
            .map_or_else(
                || value.to_string(),
                |(index, _)| value[..index].to_string(),
            )
    } else {
        value
            .trim_end_matches(|character: char| character.is_ascii_digit())
            .to_string()
    }
}

fn read_hex_u32(path: impl AsRef<Path>) -> Option<u32> {
    let value = read_trimmed(path)?;
    u32::from_str_radix(value.trim_start_matches("0x"), 16).ok()
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    read_string(path).map(|value| value.trim().to_string())
}

fn read_string(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn read_number<T>(path: impl AsRef<Path>) -> Option<T>
where
    T: std::str::FromStr,
{
    read_trimmed(path)?.parse::<T>().ok()
}
