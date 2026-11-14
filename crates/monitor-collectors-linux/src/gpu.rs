//! Graphics cards: how busy, how much memory, how hot, how fast, how much
//! power.
//!
//! Each `/sys/class/drm/cardN` is one card. What it publishes depends on the
//! driver:
//!
//! - **amdgpu** has `gpu_busy_percent`, `mem_info_vram_used` and `_total`,
//!   `mem_info_gtt_used` and `_total`, the current clock levels in
//!   `pp_dpm_sclk` and `pp_dpm_mclk` (the active line is starred), and an
//!   hwmon chip under the device with `temp1_input` (`edge`), `power1_input`
//!   or `power1_average`, and `freq1_input` (`sclk`).
//! - **i915** and **xe** (Intel) publish no busy file. Busy time comes from the
//!   DRM client counters every open file carries, summed by
//!   [`crate::netproc`]; the clock is `gt_cur_freq_mhz` (i915) or
//!   `tile0/gt0/freq0/cur_freq` (xe); power is the hwmon `energy1_input`
//!   counter divided by the interval.
//! - **nvidia** publishes nothing useful in sysfs. When the machine has the
//!   driver's management library (`libnvidia-ml.so.1`) it is loaded and asked;
//!   without it the card is named and everything else is said to be not
//!   reported.
//!
//! Busy time summed from clients only counts the clients this user can read,
//! which on a desktop is every graphics client but the display server's when
//! that runs as another user; the reading says where it came from.

use crate::catalog::{collector_id, gauge, identity, metric_id, sys_source, utilization};
use crate::fsread::{ReadError, list_dir, read_attribute};
use crate::netproc::{DeviceClass, UsageRound};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const GPU_COLLECTOR: &str = "linux.gpu";

/// A vendor's name from its PCI vendor id.
pub fn vendor_name(id: &str) -> Option<&'static str> {
    match id.trim().to_ascii_lowercase().as_str() {
        "0x1002" | "0x1022" => Some("AMD"),
        "0x8086" => Some("Intel"),
        "0x10de" => Some("NVIDIA"),
        "0x1af4" => Some("Virtio"),
        "0x15ad" => Some("VMware"),
        "0x1414" => Some("Microsoft"),
        _ => None,
    }
}

/// The starred line of an amdgpu `pp_dpm_*` table, in hertz:
/// `1: 786Mhz *` is 786 MHz.
pub fn starred_clock(table: &str) -> Option<f64> {
    table
        .lines()
        .find(|line| line.trim_end().ends_with('*'))
        .and_then(|line| {
            let value = line.split_once(':')?.1.trim().trim_end_matches('*').trim();
            let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
            let number: f64 = digits.parse().ok()?;
            let unit = value[digits.len()..].trim().to_ascii_lowercase();
            Some(match unit.as_str() {
                "ghz" => number * 1e9,
                "khz" => number * 1e3,
                "hz" => number,
                _ => number * 1e6,
            })
        })
}

/// A PCI address in the short form sysfs uses (`0000:63:00.0`), from the
/// longer one NVML reports (`00000000:63:00.0`).
pub fn short_pci_address(address: &str) -> String {
    let lower = address.trim().to_ascii_lowercase();
    match lower.split_once(':') {
        Some((domain, rest)) if domain.len() > 4 => {
            format!("{}:{rest}", &domain[domain.len() - 4..])
        }
        _ => lower,
    }
}

/// One card's driver-independent identity.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub card: String,
    pub dir: PathBuf,
    pub device: PathBuf,
    pub driver: Option<String>,
    pub address: String,
    pub vendor: Option<String>,
}

/// Every `cardN` under `class/drm`, connectors left out.
pub fn list_cards(roots: &Roots) -> Result<Vec<Card>, ReadError> {
    let mut cards = Vec::new();
    for dir in list_dir(&roots.sys("class/drm"))? {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let is_card = name
            .strip_prefix("card")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
        if !is_card {
            continue;
        }
        let device = dir.join("device");
        let driver = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|target| target.file_name()?.to_str().map(str::to_string))
            .or_else(|| uevent(&device, "DRIVER"));
        let address = uevent(&device, "PCI_SLOT_NAME")
            .or_else(|| {
                std::fs::read_link(&device)
                    .ok()
                    .and_then(|target| target.file_name()?.to_str().map(str::to_string))
            })
            .unwrap_or_else(|| name.clone());
        let vendor = read_attribute(&device.join("vendor")).ok();
        cards.push(Card {
            card: name,
            dir,
            device,
            driver,
            address,
            vendor,
        });
    }
    Ok(cards)
}

/// One `KEY=value` line of a device's `uevent`.
pub fn uevent(device: &Path, key: &str) -> Option<String> {
    let text = read_attribute(&device.join("uevent")).ok()?;
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name == key).then(|| value.trim().to_string())
    })
}

/// The hwmon chip directory under a device, if it has one.
fn hwmon_dir(device: &Path) -> Option<PathBuf> {
    list_dir(&device.join("hwmon")).ok()?.into_iter().next()
}

fn number(path: &Path) -> Option<f64> {
    read_attribute(path).ok()?.parse::<f64>().ok()
}

/// The temperature an hwmon chip calls `edge`, or its first.
fn hwmon_temperature(hwmon: &Path) -> Option<f64> {
    let mut first = None;
    for index in 1..=8 {
        let Some(raw) = number(&hwmon.join(format!("temp{index}_input"))) else {
            continue;
        };
        let celsius = raw / 1000.0;
        let label = read_attribute(&hwmon.join(format!("temp{index}_label"))).unwrap_or_default();
        if label.eq_ignore_ascii_case("edge") {
            return Some(celsius);
        }
        first.get_or_insert(celsius);
    }
    first
}

/// Whether runtime power management has powered the device down.
pub fn is_suspended(device: &Path) -> bool {
    read_attribute(&device.join("power/runtime_status")).is_ok_and(|status| status == "suspended")
}

/// The addresses of the NVIDIA cards NVML may be asked about: the ones that
/// are awake. NVML wakes every card it opens, so a card runtime power
/// management has powered down is never named to it.
pub fn nvml_targets(cards: &[Card]) -> Vec<String> {
    cards
        .iter()
        .filter(|c| c.driver.as_deref() == Some("nvidia") && !is_suspended(&c.device))
        .map(|c| short_pci_address(&c.address))
        .collect()
}

/// What NVIDIA's library said about one card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NvmlReading {
    pub address: String,
    pub name: Option<String>,
    pub utilization: Option<f64>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub temperature: Option<f64>,
    pub clock_hz: Option<f64>,
    pub power_watts: Option<f64>,
}

/// The graphics collector.
pub struct GpuCollector {
    roots: Roots,
    /// Intel energy counters: card address -> (microjoules, when).
    energy: HashMap<String, (f64, Timestamp)>,
}

impl GpuCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            energy: HashMap::new(),
        }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let source = || sys_source("class/drm/cardN/device");
        vec![
            identity("gpu.card", source(), "the kernel's card name"),
            identity(
                "gpu.driver",
                source(),
                "the kernel driver bound to the card",
            ),
            identity("gpu.vendor", source(), "the vendor, from the PCI vendor id"),
            identity(
                "gpu.name",
                source(),
                "the product name where the driver gives one",
            ),
            identity(
                "gpu.utilization.source",
                source(),
                "driver, clients, nvml, or asleep: where the busy share was read",
            ),
            identity(
                "gpu.runtime_status",
                source(),
                "runtime power management: active, or suspended while powered down",
            ),
            utilization(
                "gpu.utilization",
                source(),
                "gpu_busy_percent, summed client engine time, or NVML",
            ),
            gauge(
                "gpu.memory.dedicated.used",
                Unit::Bytes,
                source(),
                "mem_info_vram_used",
            ),
            gauge(
                "gpu.memory.dedicated.total",
                Unit::Bytes,
                source(),
                "mem_info_vram_total",
            ),
            gauge(
                "gpu.memory.shared.used",
                Unit::Bytes,
                source(),
                "mem_info_gtt_used",
            ),
            gauge(
                "gpu.memory.shared.total",
                Unit::Bytes,
                source(),
                "mem_info_gtt_total",
            ),
            gauge(
                "gpu.temperature",
                Unit::DegreesCelsius,
                source(),
                "the card's hwmon edge temperature",
            ),
            gauge(
                "gpu.frequency",
                Unit::Hertz,
                source(),
                "the graphics clock now",
            ),
            gauge(
                "gpu.memory.frequency",
                Unit::Hertz,
                source(),
                "the memory clock now",
            ),
            gauge(
                "gpu.power",
                Unit::Watts,
                source(),
                "the card's hwmon power, or its energy counter over the interval",
            ),
            gauge(
                "gpu.clients",
                Unit::Count,
                source(),
                "open graphics clients this user can see",
            ),
        ]
    }

    pub fn sample_with(
        &mut self,
        roots: &Roots,
        at: Timestamp,
        usage: Option<&UsageRound>,
    ) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(GPU_COLLECTOR), at);
        let cards = match list_cards(roots) {
            Ok(cards) => cards,
            Err(ReadError::Missing { path }) => {
                report.health = CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                    path: path.display().to_string(),
                });
                return report;
            }
            Err(other) => {
                report.health = CollectorHealth::Failed {
                    detail: other.path().display().to_string(),
                };
                return report;
            }
        };
        // NVML wakes every card it opens, so it is asked only about the
        // NVIDIA cards that are awake, each by its own address; a
        // powered-down one is reported asleep and left asleep.
        let targets = nvml_targets(&cards);
        let nvml = if roots.loads_nvml() && !targets.is_empty() {
            nvml::read(&targets)
        } else {
            Vec::new()
        };
        for card in cards {
            let metrics = self.read_card(&card, at, usage, &nvml);
            report.entities.push(Entity::new(
                EntityId::new(EntityKind::Gpu, card.address.clone()),
                metrics,
            ));
        }
        report
    }

    fn read_card(
        &mut self,
        card: &Card,
        at: Timestamp,
        usage: Option<&UsageRound>,
        nvml: &[NvmlReading],
    ) -> MetricSet {
        let mut m = MetricSet::new();
        let device = &card.device;
        let driver = card.driver.clone().unwrap_or_default();
        m.insert(metric_id("gpu.card"), Observation::text(card.card.clone()));
        if !driver.is_empty() {
            m.insert(metric_id("gpu.driver"), Observation::text(driver.clone()));
        }
        if let Some(vendor) = card.vendor.as_deref() {
            m.insert(
                metric_id("gpu.vendor"),
                Observation::text(vendor_name(vendor).unwrap_or(vendor)),
            );
        }
        if let Ok(name) = read_attribute(&device.join("product_name"))
            && !name.trim().is_empty()
        {
            m.insert(metric_id("gpu.name"), Observation::text(name.trim()));
        }
        let clients = usage.and_then(|u| u.devices.get(&card.address));
        if let Some(clients) = clients.filter(|c| c.class == Some(DeviceClass::Graphics)) {
            m.insert(
                metric_id("gpu.clients"),
                Observation::unsigned(clients.clients as u64),
            );
        }
        let not_reported = |detail: &str| {
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: detail.to_string(),
            })
        };
        let hwmon = hwmon_dir(device);

        // A discrete card the kernel has powered down. Reading its busy,
        // memory, clock, or sensor files would wake it, costing more battery
        // than the reading is worth, so it is reported asleep and idle.
        let runtime = read_attribute(&device.join("power/runtime_status")).ok();
        if let Some(runtime) = &runtime {
            m.insert(
                metric_id("gpu.runtime_status"),
                Observation::text(runtime.clone()),
            );
        }
        if runtime.as_deref() == Some("suspended") {
            m.insert(
                metric_id("gpu.utilization.source"),
                Observation::text("asleep"),
            );
            m.insert(metric_id("gpu.utilization"), Observation::float(0.0));
            return m;
        }

        if driver == "nvidia" {
            let short = short_pci_address(&card.address);
            match nvml.iter().find(|r| short_pci_address(&r.address) == short) {
                Some(reading) => {
                    if let Some(name) = &reading.name {
                        m.insert(metric_id("gpu.name"), Observation::text(name.clone()));
                    }
                    m.insert(
                        metric_id("gpu.utilization.source"),
                        Observation::text("nvml"),
                    );
                    let float = |value: Option<f64>| {
                        value.map(Observation::float).unwrap_or_else(|| {
                            not_reported("NVIDIA's library does not report it for this card")
                        })
                    };
                    m.insert(metric_id("gpu.utilization"), float(reading.utilization));
                    m.insert(
                        metric_id("gpu.memory.dedicated.used"),
                        float(reading.memory_used.map(|v| v as f64)),
                    );
                    m.insert(
                        metric_id("gpu.memory.dedicated.total"),
                        float(reading.memory_total.map(|v| v as f64)),
                    );
                    m.insert(metric_id("gpu.temperature"), float(reading.temperature));
                    m.insert(metric_id("gpu.frequency"), float(reading.clock_hz));
                    m.insert(metric_id("gpu.power"), float(reading.power_watts));
                }
                None => {
                    let detail = if self.roots.loads_nvml() {
                        "NVIDIA's management library (libnvidia-ml.so.1) is not installed"
                    } else {
                        "NVIDIA cards are read through NVIDIA's library, which a snapshot does not load"
                    };
                    m.insert(metric_id("gpu.utilization"), not_reported(detail));
                }
            }
            return m;
        }

        // Busy share.
        match number(&device.join("gpu_busy_percent")) {
            Some(percent) => {
                m.insert(
                    metric_id("gpu.utilization.source"),
                    Observation::text("driver"),
                );
                m.insert(
                    metric_id("gpu.utilization"),
                    Observation::float((percent / 100.0).clamp(0.0, 1.0)),
                );
            }
            None => match (usage, clients.and_then(|c| c.busy)) {
                (_, Some(busy)) => {
                    m.insert(
                        metric_id("gpu.utilization.source"),
                        Observation::text("clients"),
                    );
                    m.insert(metric_id("gpu.utilization"), Observation::float(busy));
                }
                (Some(round), None) if round.has_interval => {
                    // No client has the card open, so nothing made it busy;
                    // but only a driver that publishes client engine time
                    // can be read that way.
                    if clients.is_none_or(|c| c.clients == 0)
                        && crate::netproc::reports_engine_time(&driver)
                    {
                        m.insert(
                            metric_id("gpu.utilization.source"),
                            Observation::text("clients"),
                        );
                        m.insert(metric_id("gpu.utilization"), Observation::float(0.0));
                    } else {
                        m.insert(
                            metric_id("gpu.utilization"),
                            not_reported("the driver reports no busy time"),
                        );
                    }
                }
                _ => {
                    m.insert(
                        metric_id("gpu.utilization"),
                        Observation::Unknown(UnknownReason::NotYetSampled),
                    );
                }
            },
        }

        // Memory.
        for (id, file) in [
            ("gpu.memory.dedicated.used", "mem_info_vram_used"),
            ("gpu.memory.dedicated.total", "mem_info_vram_total"),
            ("gpu.memory.shared.used", "mem_info_gtt_used"),
            ("gpu.memory.shared.total", "mem_info_gtt_total"),
        ] {
            if let Some(bytes) = number(&device.join(file)) {
                m.insert(metric_id(id), Observation::unsigned(bytes as u64));
            }
        }

        // Temperature.
        if let Some(celsius) = hwmon.as_deref().and_then(hwmon_temperature) {
            m.insert(metric_id("gpu.temperature"), Observation::float(celsius));
        }

        // Clock.
        let clock = hwmon
            .as_deref()
            .and_then(|h| number(&h.join("freq1_input")))
            .or_else(|| {
                read_attribute(&device.join("pp_dpm_sclk"))
                    .ok()
                    .and_then(|t| starred_clock(&t))
            })
            .or_else(|| number(&card.dir.join("gt_cur_freq_mhz")).map(|mhz| mhz * 1e6))
            .or_else(|| number(&device.join("tile0/gt0/freq0/cur_freq")).map(|mhz| mhz * 1e6));
        if let Some(hz) = clock {
            m.insert(metric_id("gpu.frequency"), Observation::float(hz));
        }
        if let Some(hz) = read_attribute(&device.join("pp_dpm_mclk"))
            .ok()
            .and_then(|t| starred_clock(&t))
        {
            m.insert(metric_id("gpu.memory.frequency"), Observation::float(hz));
        }

        // Power.
        let instant = hwmon.as_deref().and_then(|h| {
            number(&h.join("power1_input")).or_else(|| number(&h.join("power1_average")))
        });
        match instant {
            Some(microwatts) => {
                m.insert(metric_id("gpu.power"), Observation::float(microwatts / 1e6));
            }
            None => {
                if let Some(microjoules) = hwmon
                    .as_deref()
                    .and_then(|h| number(&h.join("energy1_input")))
                {
                    let previous = self.energy.insert(card.address.clone(), (microjoules, at));
                    m.insert(
                        metric_id("gpu.power"),
                        match previous.and_then(|(before, then)| {
                            let seconds = Timestamp::interval_seconds(then, at)?;
                            (microjoules >= before).then(|| (microjoules - before) / 1e6 / seconds)
                        }) {
                            Some(watts) => Observation::float(watts),
                            None => Observation::Unknown(UnknownReason::NotYetSampled),
                        },
                    );
                }
            }
        }
        m
    }
}

impl Collector for GpuCollector {
    fn id(&self) -> CollectorId {
        collector_id(GPU_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        GpuCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        self.sample_with(&roots, at, None)
    }
}

/// NVIDIA's management library, loaded at run time when it is installed.
///
/// Linking it would make every Better Monitor fail to start on a machine
/// without NVIDIA's driver, so it is opened with `dlopen` and only the handful
/// of read-only queries below are looked up. Nothing is ever set.
mod nvml {
    use super::NvmlReading;
    use std::ffi::{CStr, c_char, c_uint, c_void};
    use std::sync::OnceLock;

    type Device = *mut c_void;
    type Status = i32;

    #[repr(C)]
    struct Utilization {
        gpu: c_uint,
        memory: c_uint,
    }

    #[repr(C)]
    struct Memory {
        total: u64,
        free: u64,
        used: u64,
    }

    #[repr(C)]
    struct PciInfo {
        bus_id_legacy: [c_char; 16],
        domain: c_uint,
        bus: c_uint,
        device: c_uint,
        pci_device_id: c_uint,
        pci_subsystem_id: c_uint,
        bus_id: [c_char; 32],
    }

    struct Library {
        handle: unsafe extern "C" fn(*const c_char, *mut Device) -> Status,
        name: unsafe extern "C" fn(Device, *mut c_char, c_uint) -> Status,
        utilization: unsafe extern "C" fn(Device, *mut Utilization) -> Status,
        memory: unsafe extern "C" fn(Device, *mut Memory) -> Status,
        temperature: unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> Status,
        clock: unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> Status,
        power: unsafe extern "C" fn(Device, *mut c_uint) -> Status,
        pci: unsafe extern "C" fn(Device, *mut PciInfo) -> Status,
    }

    // SAFETY: the function pointers are plain C entry points of a library
    // that stays loaded for the life of the process.
    unsafe impl Send for Library {}
    unsafe impl Sync for Library {}

    /// One entry point of the library as the function type it is declared as.
    ///
    /// # Safety
    /// `T` must be the `unsafe extern "C" fn` type the symbol is documented
    /// to have; every such type is the size of a pointer.
    unsafe fn symbol<T>(handle: *mut c_void, name: &CStr) -> Option<T> {
        debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
        // SAFETY: dlsym with a NUL-terminated name; null is checked below.
        let pointer = unsafe { libc::dlsym(handle, name.as_ptr()) };
        if pointer.is_null() {
            return None;
        }
        // SAFETY: the caller names the symbol's own function pointer type.
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&pointer) })
    }

    fn load() -> Option<Library> {
        // SAFETY: dlopen with a NUL-terminated name; each symbol is looked up
        // with its documented type and the library stays loaded for the life
        // of the process.
        unsafe {
            let handle = libc::dlopen(
                c"libnvidia-ml.so.1".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            );
            if handle.is_null() {
                return None;
            }
            let init: unsafe extern "C" fn() -> Status = symbol(handle, c"nvmlInit_v2")?;
            if init() != 0 {
                return None;
            }
            Some(Library {
                handle: symbol(handle, c"nvmlDeviceGetHandleByPciBusId_v2")?,
                name: symbol(handle, c"nvmlDeviceGetName")?,
                utilization: symbol(handle, c"nvmlDeviceGetUtilizationRates")?,
                memory: symbol(handle, c"nvmlDeviceGetMemoryInfo")?,
                temperature: symbol(handle, c"nvmlDeviceGetTemperature")?,
                clock: symbol(handle, c"nvmlDeviceGetClockInfo")?,
                power: symbol(handle, c"nvmlDeviceGetPowerUsage")?,
                pci: symbol(handle, c"nvmlDeviceGetPciInfo_v3")?,
            })
        }
    }

    fn library() -> Option<&'static Library> {
        static LIBRARY: OnceLock<Option<Library>> = OnceLock::new();
        LIBRARY.get_or_init(load).as_ref()
    }

    /// The cards at these PCI addresses, as NVML reports them. Only the
    /// cards named are opened, so a card left out is never woken. Empty when
    /// the library is not installed.
    pub(super) fn read(addresses: &[String]) -> Vec<NvmlReading> {
        let Some(lib) = library() else {
            return Vec::new();
        };
        let mut readings = Vec::new();
        // SAFETY: each call writes only into the local it is given; a non-zero
        // status leaves that reading out.
        unsafe {
            for address in addresses {
                let Ok(bus_id) = std::ffi::CString::new(address.as_str()) else {
                    continue;
                };
                let mut device: Device = std::ptr::null_mut();
                if (lib.handle)(bus_id.as_ptr(), &mut device) != 0 {
                    continue;
                }
                let mut pci: PciInfo = std::mem::zeroed();
                if (lib.pci)(device, &mut pci) != 0 {
                    continue;
                }
                let address = CStr::from_ptr(pci.bus_id.as_ptr())
                    .to_string_lossy()
                    .into_owned();
                let mut reading = NvmlReading {
                    address,
                    ..NvmlReading::default()
                };
                let mut name = [0 as c_char; 96];
                if (lib.name)(device, name.as_mut_ptr(), name.len() as c_uint) == 0 {
                    reading.name =
                        Some(CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned());
                }
                let mut utilization = Utilization { gpu: 0, memory: 0 };
                if (lib.utilization)(device, &mut utilization) == 0 {
                    reading.utilization = Some(utilization.gpu as f64 / 100.0);
                }
                let mut memory = Memory {
                    total: 0,
                    free: 0,
                    used: 0,
                };
                if (lib.memory)(device, &mut memory) == 0 {
                    reading.memory_used = Some(memory.used);
                    reading.memory_total = Some(memory.total);
                }
                let mut value: c_uint = 0;
                if (lib.temperature)(device, 0, &mut value) == 0 {
                    reading.temperature = Some(value as f64);
                }
                if (lib.clock)(device, 0, &mut value) == 0 {
                    reading.clock_hz = Some(value as f64 * 1e6);
                }
                if (lib.power)(device, &mut value) == 0 {
                    reading.power_watts = Some(value as f64 / 1000.0);
                }
                readings.push(reading);
            }
        }
        readings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starred_clock_level_is_the_current_one() {
        // Recorded from the owner's machine.
        assert_eq!(
            starred_clock("0: 600Mhz \n1: 786Mhz *\n2: 2900Mhz \n"),
            Some(786e6)
        );
        assert_eq!(starred_clock("0: 400Mhz \n1: 800Mhz \n"), None);
    }

    #[test]
    fn vendor_ids_are_named() {
        assert_eq!(vendor_name("0x1002"), Some("AMD"));
        assert_eq!(vendor_name("0x8086\n"), Some("Intel"));
        assert_eq!(vendor_name("0x10DE"), Some("NVIDIA"));
        assert_eq!(vendor_name("0x1234"), None);
    }

    #[test]
    fn an_nvml_bus_id_matches_the_sysfs_address() {
        assert_eq!(short_pci_address("00000000:01:00.0"), "0000:01:00.0");
        assert_eq!(short_pci_address("0000:63:00.0"), "0000:63:00.0");
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = GpuCollector::descriptors();
        let mut ids: Vec<_> = descriptors.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), descriptors.len());
    }
}
