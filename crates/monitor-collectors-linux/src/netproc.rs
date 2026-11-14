//! What each process is doing with the network, the graphics card, and the
//! NPU.
//!
//! None of these has a per-process counter file. They are found through the
//! process's open files:
//!
//! - A socket is `socket:[inode]` under `/proc/<pid>/fd`. The kernel's
//!   `NETLINK_SOCK_DIAG` dump lists every TCP socket with its inode and its
//!   `tcp_info`, whose `tcpi_bytes_acked` and `tcpi_bytes_received` count what
//!   the socket has sent and received. Matching inodes to processes gives each
//!   process's TCP traffic without root, the way `ss -tp` does. UDP keeps no
//!   byte counter, so QUIC and other UDP traffic cannot be attributed and the
//!   collector says so rather than estimating it.
//! - A graphics or NPU client is an open `/dev/dri/*` or `/dev/accel/*` file.
//!   Its `/proc/<pid>/fdinfo/<fd>` carries the DRM client keys of the kernel's
//!   `Documentation/gpu/drm-usage-stats.rst`: busy time per engine
//!   (`drm-engine-<name>`, nanoseconds, or `drm-cycles-<name>` over
//!   `drm-total-cycles-<name>`), and memory per region (`drm-resident-<region>`).
//!
//! Reading every descriptor link costs a system call per descriptor, thousands
//! on a desktop, so the links are re-read every [`FD_SCAN_INTERVAL`] and the
//! counters behind them every round. A socket opened between scans is counted
//! from its first byte once the next scan finds its owner, so traffic is late,
//! never lost; only a socket that opened and closed between two scans goes
//! unseen.
//!
//! Only the caller's own processes have readable descriptors. Everyone else's
//! readings are permission denied, never zero.

use crate::catalog::{collector_id, gauge, metric_id, proc_source, rate, utilization};
use crate::fsread::{list_dir, read_text};
use crate::roots::{Roots, SocketSource};
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, MetricDescriptor, MetricSet,
    Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

pub const PROCESS_USAGE_COLLECTOR: &str = "linux.process.usage";

/// How often descriptor links are re-read.
pub const FD_SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// How often the TCP socket table is dumped. The kernel walks its whole
/// connection hash table for a dump, several milliseconds however few
/// sockets are open, so per-process traffic is a rate over three seconds
/// rather than one; between dumps the last rates stand.
pub const NETWORK_INTERVAL: Duration = Duration::from_secs(3);

/// Which device an open file is.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DeviceClass {
    /// `/dev/dri/*`: a graphics card.
    Graphics,
    /// `/dev/accel/*`: a compute accelerator such as an NPU.
    Accelerator,
}

/// Drivers known to publish DRM client engine time in fdinfo. Only for these
/// does "no program has it open" mean "idle"; for any other driver it means
/// nothing is known.
pub const ENGINE_TIME_DRIVERS: [&str; 5] = ["amdgpu", "i915", "xe", "amdxdna", "intel_vpu"];

/// Whether a driver publishes per-client engine time.
pub fn reports_engine_time(driver: &str) -> bool {
    ENGINE_TIME_DRIVERS.contains(&driver)
}

/// Whether some graphics card's driver publishes per-client engine time, so
/// a program's share of it can be read. A card that only reports its own busy
/// share (through `gpu_busy_percent` or NVIDIA's library) says how busy the
/// card is, not which program made it busy, so on a machine with only such
/// cards a program's graphics use is not reported rather than read as 0%.
pub fn has_engine_time_card(roots: &Roots) -> bool {
    crate::gpu::list_cards(roots).is_ok_and(|cards| {
        cards
            .iter()
            .any(|card| card.driver.as_deref().is_some_and(reports_engine_time))
    })
}

/// The same for accelerators under `class/accel`.
pub fn has_engine_time_accelerator(roots: &Roots) -> bool {
    list_dir(&roots.sys("class/accel")).is_ok_and(|devices| {
        devices.iter().any(|dir| {
            let device = dir.join("device");
            std::fs::read_link(device.join("driver"))
                .ok()
                .and_then(|target| target.file_name()?.to_str().map(str::to_string))
                .or_else(|| crate::gpu::uevent(&device, "DRIVER"))
                .is_some_and(|driver| reports_engine_time(&driver))
        })
    })
}

/// What one descriptor link points at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Link {
    Socket(u64),
    Device(DeviceClass),
    Other,
}

/// Classify a descriptor link target.
pub fn classify_link(target: &str) -> Link {
    if let Some(inode) = target
        .strip_prefix("socket:[")
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|inode| inode.parse().ok())
    {
        return Link::Socket(inode);
    }
    if target.starts_with("/dev/dri/") {
        return Link::Device(DeviceClass::Graphics);
    }
    if target.starts_with("/dev/accel/") {
        return Link::Device(DeviceClass::Accelerator);
    }
    Link::Other
}

/// One DRM client as its fdinfo describes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrmClient {
    pub driver: String,
    /// The PCI address of the device (`drm-pdev`).
    pub pdev: Option<String>,
    pub client_id: Option<u64>,
    /// Busy nanoseconds per engine.
    pub engine_ns: BTreeMap<String, u64>,
    /// Busy cycles and the cycles that elapsed, per engine, for drivers that
    /// count cycles (Intel `xe`).
    pub engine_cycles: BTreeMap<String, (u64, u64)>,
    /// How many of an engine a device has (`drm-engine-capacity-<name>`).
    pub capacity: BTreeMap<String, u64>,
    /// Resident bytes per memory region.
    pub memory: BTreeMap<String, u64>,
}

impl DrmClient {
    /// Bytes in device-local memory (VRAM, Intel `local`).
    pub fn dedicated_bytes(&self) -> u64 {
        self.memory
            .iter()
            .filter(|(region, _)| region.contains("vram") || region.starts_with("local"))
            .map(|(_, bytes)| *bytes)
            .sum()
    }

    /// Bytes in memory shared with the processor (GTT, system).
    pub fn shared_bytes(&self) -> u64 {
        self.memory
            .iter()
            .filter(|(region, _)| !(region.contains("vram") || region.starts_with("local")))
            .map(|(_, bytes)| *bytes)
            .sum()
    }
}

fn parse_quantity(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number: u64 = parts.next()?.parse().ok()?;
    let scale = match parts.next() {
        None | Some("ns") => 1,
        Some("KiB") => 1024,
        Some("MiB") => 1024 * 1024,
        Some("GiB") => 1024 * 1024 * 1024,
        Some(_) => return None,
    };
    Some(number * scale)
}

/// Parse one `/proc/<pid>/fdinfo/<fd>`. `None` when it is not a DRM client.
pub fn parse_drm_fdinfo(text: &str) -> Option<DrmClient> {
    let mut client = DrmClient::default();
    let mut resident: BTreeMap<String, u64> = BTreeMap::new();
    let mut memory: BTreeMap<String, u64> = BTreeMap::new();
    let mut cycles: BTreeMap<String, u64> = BTreeMap::new();
    let mut total_cycles: BTreeMap<String, u64> = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "drm-driver" {
            client.driver = value.to_string();
        } else if key == "drm-pdev" {
            client.pdev = Some(value.to_string());
        } else if key == "drm-client-id" {
            client.client_id = value.parse().ok();
        } else if let Some(engine) = key.strip_prefix("drm-engine-capacity-") {
            if let Ok(capacity) = value.parse() {
                client.capacity.insert(engine.to_string(), capacity);
            }
        } else if let Some(engine) = key.strip_prefix("drm-engine-") {
            if let Some(ns) = parse_quantity(value) {
                client.engine_ns.insert(engine.to_string(), ns);
            }
        } else if let Some(engine) = key.strip_prefix("drm-total-cycles-") {
            if let Ok(count) = value.parse() {
                total_cycles.insert(engine.to_string(), count);
            }
        } else if let Some(engine) = key.strip_prefix("drm-cycles-") {
            if let Ok(count) = value.parse() {
                cycles.insert(engine.to_string(), count);
            }
        } else if let Some(region) = key.strip_prefix("drm-resident-") {
            if let Some(bytes) = parse_quantity(value) {
                resident.insert(region.to_string(), bytes);
            }
        } else if let Some(region) = key.strip_prefix("drm-memory-")
            && let Some(bytes) = parse_quantity(value)
        {
            memory.insert(region.to_string(), bytes);
        }
    }
    if client.driver.is_empty() {
        return None;
    }
    for (engine, busy) in cycles {
        if let Some(total) = total_cycles.get(&engine) {
            client.engine_cycles.insert(engine, (busy, *total));
        }
    }
    // `drm-resident-*` is the current form; `drm-memory-*` is the older name
    // for the same figure and only fills a region the newer key left out.
    client.memory = resident;
    for (region, bytes) in memory {
        client.memory.entry(region).or_insert(bytes);
    }
    Some(client)
}

/// One TCP socket's counters, from a sock_diag dump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketCounters {
    pub inode: u64,
    pub sent: u64,
    pub received: u64,
}

const NLMSG_HEADER: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const INET_DIAG_MSG_LEN: usize = 72;
const INET_DIAG_INODE_OFFSET: usize = 68;
const INET_DIAG_INFO: u16 = 2;
const TCP_INFO_BYTES_ACKED: usize = 120;
const TCP_INFO_BYTES_RECEIVED: usize = 128;

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_ne_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

/// Parse the netlink messages of a TCP sock_diag dump.
///
/// A socket without an inode (in `TIME_WAIT`, owned by nobody) or without a
/// `tcp_info` long enough to carry the byte counters (a kernel before 4.2) is
/// skipped. A message that runs past the buffer ends the parse.
pub fn parse_sock_diag(bytes: &[u8]) -> Vec<SocketCounters> {
    let mut sockets = Vec::new();
    let mut offset = 0usize;
    while offset + NLMSG_HEADER <= bytes.len() {
        let Some(length) = u32_at(bytes, offset).map(|l| l as usize) else {
            break;
        };
        let Some(kind) = u16_at(bytes, offset + 4) else {
            break;
        };
        if length < NLMSG_HEADER || offset + length > bytes.len() {
            break;
        }
        let message = &bytes[offset + NLMSG_HEADER..offset + length];
        offset += align4(length);
        if kind == NLMSG_DONE || kind == NLMSG_ERROR {
            continue;
        }
        if kind != SOCK_DIAG_BY_FAMILY || message.len() < INET_DIAG_MSG_LEN {
            continue;
        }
        let Some(inode) = u32_at(message, INET_DIAG_INODE_OFFSET) else {
            continue;
        };
        if inode == 0 {
            continue;
        }
        let mut attribute = INET_DIAG_MSG_LEN;
        while attribute + 4 <= message.len() {
            let (Some(rta_len), Some(rta_type)) =
                (u16_at(message, attribute), u16_at(message, attribute + 2))
            else {
                break;
            };
            let rta_len = rta_len as usize;
            if rta_len < 4 || attribute + rta_len > message.len() {
                break;
            }
            if rta_type == INET_DIAG_INFO {
                let info = &message[attribute + 4..attribute + rta_len];
                if let (Some(sent), Some(received)) = (
                    u64_at(info, TCP_INFO_BYTES_ACKED),
                    u64_at(info, TCP_INFO_BYTES_RECEIVED),
                ) {
                    sockets.push(SocketCounters {
                        inode: inode as u64,
                        sent,
                        received,
                    });
                }
            }
            attribute += align4(rta_len);
        }
    }
    sockets
}

/// Ask the running kernel for every TCP socket's counters, IPv4 then IPv6.
fn dump_tcp_sockets() -> Result<Vec<u8>, String> {
    let mut all = Vec::new();
    for family in [libc::AF_INET as u8, libc::AF_INET6 as u8] {
        all.extend(dump_family(family)?);
    }
    Ok(all)
}

fn dump_family(family: u8) -> Result<Vec<u8>, String> {
    // SAFETY: plain socket(2); the descriptor is closed on every path below.
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_SOCK_DIAG,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let result = (|| {
        // nlmsghdr (16) + inet_diag_req_v2 (56).
        let mut request = [0u8; 72];
        request[0..4].copy_from_slice(&72u32.to_ne_bytes());
        request[4..6].copy_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
        let flags = (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16;
        request[6..8].copy_from_slice(&flags.to_ne_bytes());
        request[8..12].copy_from_slice(&1u32.to_ne_bytes());
        request[16] = family;
        request[17] = libc::IPPROTO_TCP as u8;
        // Ask for tcp_info: bit INET_DIAG_INFO - 1.
        request[18] = 1 << (INET_DIAG_INFO - 1);
        request[20..24].copy_from_slice(&u32::MAX.to_ne_bytes());
        // SAFETY: a zeroed sockaddr_nl addressed to the kernel (pid 0).
        let mut address: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        address.nl_family = libc::AF_NETLINK as u16;
        // SAFETY: the buffer and address outlive the call and their lengths
        // are passed exactly.
        let sent = unsafe {
            libc::sendto(
                fd,
                request.as_ptr().cast(),
                request.len(),
                0,
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as u32,
            )
        };
        if sent < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut collected = Vec::new();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            // SAFETY: recv into a buffer of the length given.
            let received = unsafe { libc::recv(fd, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
            if received < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let chunk = &buffer[..received as usize];
            collected.extend_from_slice(chunk);
            if received == 0 || chunk_is_done(chunk) {
                break;
            }
        }
        Ok(collected)
    })();
    // SAFETY: closing the descriptor this function opened.
    unsafe {
        libc::close(fd);
    }
    result
}

fn chunk_is_done(chunk: &[u8]) -> bool {
    let mut offset = 0usize;
    while offset + NLMSG_HEADER <= chunk.len() {
        let (Some(length), Some(kind)) = (u32_at(chunk, offset), u16_at(chunk, offset + 4)) else {
            return true;
        };
        if kind == NLMSG_DONE || kind == NLMSG_ERROR {
            return true;
        }
        if length < NLMSG_HEADER as u32 {
            return true;
        }
        offset += align4(length as usize);
    }
    false
}

/// What one round found a process using.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessUsage {
    /// Whether the descriptors could be read at all.
    pub readable: bool,
    /// Whether the process was gone by the time its descriptors were read.
    pub gone: bool,
    /// Bytes per second, TCP only. `None` before two dumps.
    pub receive_rate: Option<f64>,
    pub send_rate: Option<f64>,
    /// Per graphics device, by PCI address: busy share of the busiest engine,
    /// dedicated bytes, shared bytes.
    pub gpu: BTreeMap<String, DeviceUse>,
    /// Per accelerator, by PCI address.
    pub npu: BTreeMap<String, DeviceUse>,
}

/// A process's use of one device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceUse {
    pub driver: String,
    /// `0.0..=1.0`, the busiest engine. `None` before two rounds, or when the
    /// driver reports no engine time.
    pub busy: Option<f64>,
    pub dedicated_bytes: u64,
    pub shared_bytes: u64,
}

/// A whole device's use, summed over every client this caller can see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceTotal {
    pub class: Option<DeviceClass>,
    pub driver: String,
    pub busy: Option<f64>,
    pub clients: usize,
}

/// One round of per-process usage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageRound {
    pub processes: HashMap<u32, ProcessUsage>,
    /// Per device PCI address.
    pub devices: BTreeMap<String, DeviceTotal>,
    /// Why per-process network traffic is missing this round, when it is.
    pub network_unavailable: Option<String>,
    /// Whether it is missing because there is no socket dump at all (a
    /// snapshot recorded without one), rather than because the dump failed.
    pub network_absent: bool,
    /// Whether two rounds have been seen, so a rate is meaningful.
    pub has_interval: bool,
    /// Whether the machine has a graphics card (`class/drm/cardN`) and an
    /// accelerator (`class/accel`) at all. Without one, a process's use of
    /// it is not a measured zero.
    pub has_graphics: bool,
    pub has_accelerator: bool,
}

#[derive(Clone, Debug, Default)]
struct OpenFiles {
    sockets: Vec<u64>,
    devices: Vec<(u32, DeviceClass)>,
}

type ClientKey = (String, String, u64);

#[derive(Clone, Debug)]
struct ClientCounters {
    engine_ns: BTreeMap<String, u64>,
    engine_cycles: BTreeMap<String, (u64, u64)>,
}

/// The per-process usage scanner.
pub struct ProcessUsageCollector {
    roots: Roots,
    files: HashMap<u32, Option<OpenFiles>>,
    last_scan: Option<Timestamp>,
    /// Bytes already counted per socket inode: (sent, received).
    attributed: HashMap<u64, (u64, u64)>,
    first_dump_done: bool,
    /// When the socket table was last dumped, and the rates it gave.
    last_dump: Option<Timestamp>,
    last_rates: HashMap<u32, (f64, f64)>,
    last_matched: u64,
    clients: HashMap<ClientKey, ClientCounters>,
    previous_at: Option<Timestamp>,
    latest: UsageRound,
}

impl ProcessUsageCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            files: HashMap::new(),
            last_scan: None,
            attributed: HashMap::new(),
            first_dump_done: false,
            last_dump: None,
            last_rates: HashMap::new(),
            last_matched: 0,
            clients: HashMap::new(),
            previous_at: None,
            latest: UsageRound::default(),
        }
    }

    /// What the last round found.
    pub fn latest(&self) -> &UsageRound {
        &self.latest
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        vec![
            gauge(
                "process.usage.tcp_sockets",
                Unit::Count,
                proc_source("[pid]/fd"),
                "TCP sockets matched to a process this round",
            ),
            gauge(
                "process.usage.device_clients",
                Unit::Count,
                proc_source("[pid]/fdinfo"),
                "open graphics and accelerator clients this round",
            ),
            rate(
                "process.network.receive.rate",
                Unit::BytesPerSecond,
                proc_source("net sock_diag tcp_info bytes_received per socket inode"),
                "TCP bytes received per second by the process's sockets",
            ),
            rate(
                "process.network.send.rate",
                Unit::BytesPerSecond,
                proc_source("net sock_diag tcp_info bytes_acked per socket inode"),
                "TCP bytes sent and acknowledged per second",
            ),
            utilization(
                "process.gpu.utilization",
                proc_source("[pid]/fdinfo drm-engine-*"),
                "busiest graphics engine's busy share this process caused",
            ),
            gauge(
                "process.gpu.memory.dedicated",
                Unit::Bytes,
                proc_source("[pid]/fdinfo drm-resident-vram"),
                "graphics memory on the card the process holds",
            ),
            gauge(
                "process.gpu.memory.shared",
                Unit::Bytes,
                proc_source("[pid]/fdinfo drm-resident-gtt"),
                "system memory the process shares with the graphics card",
            ),
            utilization(
                "process.npu.utilization",
                proc_source("[pid]/fdinfo of /dev/accel"),
                "NPU busy share this process caused, where the driver reports it",
            ),
            gauge(
                "process.npu.memory",
                Unit::Bytes,
                proc_source("[pid]/fdinfo of /dev/accel"),
                "memory the process holds on the NPU",
            ),
        ]
    }

    fn rescan_files(&mut self, roots: &Roots, pids: &[u32]) {
        let mut files = HashMap::new();
        for pid in pids {
            let fd_dir = roots.proc(&format!("{pid}/fd"));
            let entries = match list_dir(&fd_dir) {
                Ok(entries) => entries,
                // The process exited between being listed and being read:
                // there is nothing to say about it, and it is not a refusal.
                Err(crate::fsread::ReadError::Missing { .. }) => continue,
                Err(_) => {
                    files.insert(*pid, None);
                    continue;
                }
            };
            let mut open = OpenFiles::default();
            for entry in entries {
                let Ok(target) = std::fs::read_link(&entry) else {
                    continue;
                };
                let target = target.to_string_lossy();
                match classify_link(&target) {
                    Link::Socket(inode) => open.sockets.push(inode),
                    Link::Device(class) => {
                        if let Some(fd) = entry
                            .file_name()
                            .and_then(|n| n.to_str())
                            .and_then(|n| n.parse().ok())
                        {
                            open.devices.push((fd, class));
                        }
                    }
                    Link::Other => {}
                }
            }
            files.insert(*pid, Some(open));
        }
        self.files = files;
    }

    /// The sockets, or why there are none: `(detail, absent)`, where absent
    /// means a snapshot that simply has no recorded dump.
    fn read_sockets(roots: &Roots) -> Result<Vec<SocketCounters>, (String, bool)> {
        match roots.sockets() {
            SocketSource::Netlink => dump_tcp_sockets()
                .map(|bytes| parse_sock_diag(&bytes))
                .map_err(|detail| (detail, false)),
            SocketSource::Recorded(path) => std::fs::read(path)
                .map(|bytes| parse_sock_diag(&bytes))
                .map_err(|error| {
                    (
                        format!("{}: {error}", path.display()),
                        error.kind() == std::io::ErrorKind::NotFound,
                    )
                }),
        }
    }

    /// Scan the given processes. `pids` is the process collector's list for
    /// this round, so both agree on who exists.
    pub fn sample_for(&mut self, roots: &Roots, at: Timestamp, pids: &[u32]) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(PROCESS_USAGE_COLLECTOR), at);
        if !roots.proc_dir().is_dir() {
            report.health = CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                path: roots.proc_dir().display().to_string(),
            });
            self.latest = UsageRound::default();
            return report;
        }
        let due = match self.last_scan {
            None => true,
            Some(last) => Timestamp::interval_seconds(last, at)
                .is_none_or(|s| s >= FD_SCAN_INTERVAL.as_secs_f64()),
        };
        if due {
            self.rescan_files(roots, pids);
            self.last_scan = Some(at);
        } else {
            // A process that started since the last scan is read now, so it
            // is never shown as unreadable merely for being new.
            let fresh: Vec<u32> = pids
                .iter()
                .copied()
                .filter(|pid| !self.files.contains_key(pid))
                .collect();
            if !fresh.is_empty() {
                let kept = std::mem::take(&mut self.files);
                self.rescan_files(roots, &fresh);
                self.files.extend(kept);
            }
        }
        let seconds = self
            .previous_at
            .and_then(|previous| Timestamp::interval_seconds(previous, at))
            .filter(|s| *s > 0.0);
        let mut round = UsageRound {
            has_interval: seconds.is_some(),
            has_graphics: has_engine_time_card(roots),
            has_accelerator: has_engine_time_accelerator(roots),
            ..UsageRound::default()
        };
        let alive: HashSet<u32> = pids.iter().copied().collect();

        // Network.
        let mut owner: HashMap<u64, u32> = HashMap::new();
        for (pid, files) in &self.files {
            if !alive.contains(pid) {
                continue;
            }
            if let Some(files) = files {
                for inode in &files.sockets {
                    owner.entry(*inode).or_insert(*pid);
                }
            }
        }
        let dump_due = self.last_dump.is_none_or(|last| {
            Timestamp::interval_seconds(last, at)
                .is_none_or(|s| s >= NETWORK_INTERVAL.as_secs_f64())
        });
        if dump_due {
            let dump_seconds = self
                .last_dump
                .and_then(|last| Timestamp::interval_seconds(last, at))
                .filter(|s| *s > 0.0);
            let mut traffic: HashMap<u32, (u64, u64)> = HashMap::new();
            let mut matched = 0u64;
            match Self::read_sockets(roots) {
                Ok(sockets) => {
                    let mut still_open = HashMap::new();
                    for socket in sockets {
                        // Sockets present at the very first dump started
                        // before anyone was watching; only what they move from
                        // now on is this process's traffic. A socket first
                        // seen later was opened since, so every byte it
                        // carries is new.
                        let baseline = self.attributed.get(&socket.inode).copied().unwrap_or(
                            if self.first_dump_done {
                                (0, 0)
                            } else {
                                (socket.sent, socket.received)
                            },
                        );
                        let mut counted = baseline;
                        if let Some(pid) = owner.get(&socket.inode) {
                            let entry = traffic.entry(*pid).or_default();
                            entry.0 += socket.sent.saturating_sub(baseline.0);
                            entry.1 += socket.received.saturating_sub(baseline.1);
                            counted = (socket.sent, socket.received);
                            matched += 1;
                        }
                        still_open.insert(socket.inode, counted);
                    }
                    let had_previous = self.first_dump_done;
                    self.attributed = still_open;
                    self.first_dump_done = true;
                    self.last_rates.clear();
                    if let (true, Some(seconds)) = (had_previous, dump_seconds) {
                        for pid in pids {
                            let (sent, received) = traffic.get(pid).copied().unwrap_or((0, 0));
                            self.last_rates
                                .insert(*pid, (received as f64 / seconds, sent as f64 / seconds));
                        }
                    }
                    self.last_matched = matched;
                    self.last_dump = Some(at);
                }
                Err((detail, absent)) => {
                    round.network_unavailable = Some(detail);
                    round.network_absent = absent;
                    self.last_rates.clear();
                    self.last_dump = None;
                }
            }
        }
        let matched = self.last_matched;

        // Devices.
        let mut client_rounds: Vec<(u32, DeviceClass, ClientKey, DrmClient)> = Vec::new();
        let mut seen_clients: HashSet<ClientKey> = HashSet::new();
        let mut pids_sorted: Vec<u32> = self.files.keys().copied().collect();
        pids_sorted.sort_unstable();
        for pid in pids_sorted {
            if !alive.contains(&pid) {
                continue;
            }
            let Some(Some(files)) = self.files.get(&pid) else {
                continue;
            };
            for (fd, class) in &files.devices {
                let Ok(text) = read_text(&roots.proc(&format!("{pid}/fdinfo/{fd}"))) else {
                    continue;
                };
                let Some(client) = parse_drm_fdinfo(&text) else {
                    continue;
                };
                let key: ClientKey = (
                    client.driver.clone(),
                    client.pdev.clone().unwrap_or_default(),
                    client
                        .client_id
                        .unwrap_or(((pid as u64) << 20) | *fd as u64),
                );
                // A client shared by a parent and its child, or opened twice
                // through dup, is counted once, for the first process seen.
                if seen_clients.insert(key.clone()) {
                    client_rounds.push((pid, *class, key, client));
                }
            }
        }

        let mut next_clients = HashMap::new();
        // Per device: engine -> summed busy share; per process and device the same.
        let mut device_engines: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        let mut process_engines: HashMap<(u32, String), BTreeMap<String, f64>> = HashMap::new();
        for (pid, class, key, client) in &client_rounds {
            let pdev = client.pdev.clone().unwrap_or_else(|| client.driver.clone());
            let earlier = self.clients.get(key);
            let mut busy: BTreeMap<String, f64> = BTreeMap::new();
            if let (Some(earlier), Some(seconds)) = (earlier, seconds) {
                for (engine, ns) in &client.engine_ns {
                    if let Some(before) = earlier.engine_ns.get(engine) {
                        let capacity = client.capacity.get(engine).copied().unwrap_or(1).max(1);
                        let share =
                            ns.saturating_sub(*before) as f64 / (seconds * 1e9) / capacity as f64;
                        busy.insert(engine.clone(), share);
                    }
                }
                for (engine, (cycles, total)) in &client.engine_cycles {
                    if let Some((before, before_total)) = earlier.engine_cycles.get(engine) {
                        let elapsed = total.saturating_sub(*before_total);
                        if elapsed > 0 {
                            let capacity = client.capacity.get(engine).copied().unwrap_or(1).max(1);
                            busy.insert(
                                engine.clone(),
                                cycles.saturating_sub(*before) as f64
                                    / elapsed as f64
                                    / capacity as f64,
                            );
                        }
                    }
                }
            }
            next_clients.insert(
                key.clone(),
                ClientCounters {
                    engine_ns: client.engine_ns.clone(),
                    engine_cycles: client.engine_cycles.clone(),
                },
            );
            let reports_engines = !client.engine_ns.is_empty() || !client.engine_cycles.is_empty();
            let device = round.devices.entry(pdev.clone()).or_default();
            device.class = Some(*class);
            device.driver = client.driver.clone();
            device.clients += 1;
            let summed = device_engines.entry(pdev.clone()).or_default();
            for (engine, share) in &busy {
                *summed.entry(engine.clone()).or_default() += share;
            }
            let process_summed = process_engines.entry((*pid, pdev.clone())).or_default();
            for (engine, share) in &busy {
                *process_summed.entry(engine.clone()).or_default() += share;
            }
            let usage = round.processes.entry(*pid).or_default();
            let slot = match class {
                DeviceClass::Graphics => usage.gpu.entry(pdev.clone()).or_default(),
                DeviceClass::Accelerator => usage.npu.entry(pdev.clone()).or_default(),
            };
            slot.driver = client.driver.clone();
            slot.dedicated_bytes += client.dedicated_bytes();
            slot.shared_bytes += client.shared_bytes();
            if reports_engines && earlier.is_some() && seconds.is_some() {
                slot.busy = Some(0.0);
            }
        }
        for ((pid, pdev), engines) in process_engines {
            let peak = engines.values().copied().fold(0.0_f64, f64::max).min(1.0);
            if let Some(usage) = round.processes.get_mut(&pid) {
                for slot in [usage.gpu.get_mut(&pdev), usage.npu.get_mut(&pdev)]
                    .into_iter()
                    .flatten()
                {
                    if slot.busy.is_some() {
                        slot.busy = Some(peak);
                    }
                }
            }
        }
        for (pdev, engines) in device_engines {
            if let Some(device) = round.devices.get_mut(&pdev)
                && seconds.is_some()
                && !engines.is_empty()
            {
                device.busy = Some(engines.values().copied().fold(0.0_f64, f64::max).min(1.0));
            }
        }
        self.clients = next_clients;

        for pid in pids {
            let readable = matches!(self.files.get(pid), Some(Some(_)));
            let gone = !self.files.contains_key(pid);
            let usage = round.processes.entry(*pid).or_default();
            usage.readable = readable;
            usage.gone = gone;
            if readable
                && round.network_unavailable.is_none()
                && let Some((receive, send)) = self.last_rates.get(pid)
            {
                usage.receive_rate = Some(*receive);
                usage.send_rate = Some(*send);
            } else if readable && round.network_unavailable.is_none() && !self.last_rates.is_empty()
            {
                // A process that started since the last dump has moved
                // nothing anyone has counted yet.
                usage.receive_rate = Some(0.0);
                usage.send_rate = Some(0.0);
            }
        }
        // Forget processes that are gone.
        self.files.retain(|pid, _| alive.contains(pid));

        report.metrics.insert(
            metric_id("process.usage.tcp_sockets"),
            Observation::unsigned(matched),
        );
        report.metrics.insert(
            metric_id("process.usage.device_clients"),
            Observation::unsigned(client_rounds.len() as u64),
        );
        if let Some(detail) = round
            .network_unavailable
            .as_ref()
            .filter(|_| !round.network_absent)
        {
            report.health = CollectorHealth::Degraded {
                detail: format!("per-process network traffic is unavailable: {detail}"),
            };
        }
        self.previous_at = Some(at);
        self.latest = round;
        report
    }

    /// Write this round's per-process readings into the process collector's
    /// entities.
    pub fn enrich(&self, processes: &mut CollectorReport) {
        for entity in &mut processes.entities {
            let Ok(pid) = entity.id.key.parse::<u32>() else {
                continue;
            };
            let usage = self.latest.processes.get(&pid);
            write_usage(&mut entity.metrics, usage, &self.latest, pid, &self.roots);
        }
    }
}

fn write_usage(
    metrics: &mut MetricSet,
    usage: Option<&ProcessUsage>,
    round: &UsageRound,
    pid: u32,
    roots: &Roots,
) {
    let gone = usage.is_some_and(|u| u.gone);
    let denied = || {
        if gone {
            Observation::Unknown(UnknownReason::EntityDisappeared)
        } else {
            Observation::PermissionDenied {
                path: roots.proc(&format!("{pid}/fd")).display().to_string(),
            }
        }
    };
    let readable = usage.is_some_and(|u| u.readable);
    let network = |value: Option<f64>| -> Observation {
        if !readable {
            return denied();
        }
        if let Some(detail) = &round.network_unavailable {
            return if round.network_absent {
                Observation::Unsupported(UnsupportedReason::InterfaceMissing {
                    path: detail.clone(),
                })
            } else {
                Observation::Unknown(UnknownReason::ReadFailed {
                    detail: detail.clone(),
                })
            };
        }
        match value {
            Some(rate) => Observation::float(rate),
            None => Observation::Unknown(UnknownReason::NotYetSampled),
        }
    };
    metrics.insert(
        metric_id("process.network.receive.rate"),
        network(usage.and_then(|u| u.receive_rate)),
    );
    metrics.insert(
        metric_id("process.network.send.rate"),
        network(usage.and_then(|u| u.send_rate)),
    );
    let absent = |what: &str| {
        Observation::Unsupported(UnsupportedReason::NotReported {
            detail: format!("no {what} on this machine reports per-program use"),
        })
    };
    let gpu_ids = [
        "process.gpu.utilization",
        "process.gpu.memory.dedicated",
        "process.gpu.memory.shared",
    ];
    let npu_ids = ["process.npu.utilization", "process.npu.memory"];
    if !readable {
        for id in gpu_ids {
            metrics.insert(
                metric_id(id),
                if round.has_graphics {
                    denied()
                } else {
                    absent("graphics card")
                },
            );
        }
        for id in npu_ids {
            metrics.insert(
                metric_id(id),
                if round.has_accelerator {
                    denied()
                } else {
                    absent("NPU")
                },
            );
        }
        return;
    }
    let usage = usage.cloned().unwrap_or_default();
    let busy = |devices: &BTreeMap<String, DeviceUse>| -> Observation {
        if devices.is_empty() {
            return if round.has_interval {
                Observation::float(0.0)
            } else {
                Observation::Unknown(UnknownReason::NotYetSampled)
            };
        }
        let known: Vec<f64> = devices.values().filter_map(|d| d.busy).collect();
        if known.is_empty() {
            if round.has_interval {
                Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "the driver reports no busy time for this client".into(),
                })
            } else {
                Observation::Unknown(UnknownReason::NotYetSampled)
            }
        } else {
            Observation::float(known.iter().copied().fold(0.0_f64, f64::max))
        }
    };
    if round.has_graphics {
        metrics.insert(metric_id("process.gpu.utilization"), busy(&usage.gpu));
        metrics.insert(
            metric_id("process.gpu.memory.dedicated"),
            Observation::unsigned(usage.gpu.values().map(|d| d.dedicated_bytes).sum()),
        );
        metrics.insert(
            metric_id("process.gpu.memory.shared"),
            Observation::unsigned(usage.gpu.values().map(|d| d.shared_bytes).sum()),
        );
    } else {
        for id in gpu_ids {
            metrics.insert(metric_id(id), absent("graphics card"));
        }
    }
    if round.has_accelerator {
        metrics.insert(metric_id("process.npu.utilization"), busy(&usage.npu));
        metrics.insert(
            metric_id("process.npu.memory"),
            Observation::unsigned(
                usage
                    .npu
                    .values()
                    .map(|d| d.dedicated_bytes + d.shared_bytes)
                    .sum(),
            ),
        );
    } else {
        for id in npu_ids {
            metrics.insert(metric_id(id), absent("NPU"));
        }
    }
}

/// The processes a round of the process collector enumerated.
pub fn pids_of(report: &CollectorReport) -> Vec<u32> {
    report
        .entities
        .iter()
        .filter(|entity| entity.id.kind == monitor_core::EntityKind::Process)
        .filter_map(|entity| entity.id.key.parse().ok())
        .collect()
}

/// Every process directory under the proc root, for a caller with no process
/// report to hand.
pub fn list_pids(roots: &Roots) -> Vec<u32> {
    list_dir(roots.proc_dir())
        .unwrap_or_default()
        .iter()
        .filter_map(|path| path.file_name()?.to_str()?.parse().ok())
        .collect()
}

impl Collector for ProcessUsageCollector {
    fn id(&self) -> CollectorId {
        collector_id(PROCESS_USAGE_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        ProcessUsageCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        let pids = list_pids(&roots);
        self.sample_for(&roots, at, &pids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_drivers_that_publish_engine_time_read_as_idle_without_clients() {
        for driver in ["amdgpu", "i915", "xe", "amdxdna", "intel_vpu"] {
            assert!(reports_engine_time(driver));
        }
        for driver in ["nouveau", "radeon", "virtio_gpu", "simpledrm"] {
            assert!(!reports_engine_time(driver));
        }
    }

    #[test]
    fn descriptor_links_are_told_apart() {
        assert_eq!(classify_link("socket:[917]"), Link::Socket(917));
        assert_eq!(
            classify_link("/dev/dri/renderD128"),
            Link::Device(DeviceClass::Graphics)
        );
        assert_eq!(
            classify_link("/dev/accel/accel0"),
            Link::Device(DeviceClass::Accelerator)
        );
        assert_eq!(classify_link("/home/tim/notes.txt"), Link::Other);
        assert_eq!(classify_link("socket:[abc]"), Link::Other);
        assert_eq!(classify_link("pipe:[12]"), Link::Other);
    }

    /// Recorded from the owner's machine (amdgpu, gnome-shell).
    const AMDGPU_FDINFO: &str = "pos:\t0\nflags:\t02104002\nmnt_id:\t497\nino:\t917\n\
drm-driver:\tamdgpu\ndrm-client-id:\t14\ndrm-pdev:\t0000:63:00.0\npasid:\t12\n\
drm-total-cpu:\t0\ndrm-shared-cpu:\t0\ndrm-resident-cpu:\t0\n\
drm-total-gtt:\t69284 KiB\ndrm-shared-gtt:\t0\ndrm-resident-gtt:\t1328048 KiB\n\
drm-total-vram:\t1454372 KiB\ndrm-shared-vram:\t491752 KiB\ndrm-resident-vram:\t195608 KiB\n\
drm-memory-vram:\t195608 KiB\ndrm-memory-gtt: \t1328048 KiB\ndrm-memory-cpu: \t0 KiB\n\
amd-evicted-vram:\t0 KiB\ndrm-engine-gfx:\t365777857782 ns\ndrm-engine-compute:\t2221615957 ns\n";

    #[test]
    fn an_amdgpu_client_yields_engines_and_memory() {
        let client = parse_drm_fdinfo(AMDGPU_FDINFO).expect("a DRM client");
        assert_eq!(client.driver, "amdgpu");
        assert_eq!(client.pdev.as_deref(), Some("0000:63:00.0"));
        assert_eq!(client.client_id, Some(14));
        assert_eq!(client.engine_ns["gfx"], 365_777_857_782);
        assert_eq!(client.engine_ns["compute"], 2_221_615_957);
        assert!(!client.engine_ns.contains_key("capacity-gfx"));
        assert_eq!(client.dedicated_bytes(), 195_608 * 1024);
        assert_eq!(client.shared_bytes(), 1_328_048 * 1024);
    }

    #[test]
    fn an_xe_client_counts_cycles_against_total_cycles() {
        let text = "drm-driver:\txe\ndrm-client-id:\t3\ndrm-cycles-rcs:\t500\n\
drm-total-cycles-rcs:\t1000\ndrm-engine-capacity-vcs:\t2\ndrm-resident-system0:\t4 KiB\n";
        let client = parse_drm_fdinfo(text).unwrap();
        assert_eq!(client.engine_cycles["rcs"], (500, 1000));
        assert_eq!(client.capacity["vcs"], 2);
        assert_eq!(client.shared_bytes(), 4096);
    }

    #[test]
    fn a_file_that_is_not_a_drm_client_is_not_one() {
        assert_eq!(parse_drm_fdinfo("pos:\t0\nflags:\t02\n"), None);
    }

    pub(crate) fn diag_message(inode: u32, sent: u64, received: u64) -> Vec<u8> {
        let mut info = vec![0u8; 232];
        info[TCP_INFO_BYTES_ACKED..TCP_INFO_BYTES_ACKED + 8].copy_from_slice(&sent.to_ne_bytes());
        info[TCP_INFO_BYTES_RECEIVED..TCP_INFO_BYTES_RECEIVED + 8]
            .copy_from_slice(&received.to_ne_bytes());
        let mut body = vec![0u8; INET_DIAG_MSG_LEN];
        body[0] = libc::AF_INET as u8;
        body[INET_DIAG_INODE_OFFSET..INET_DIAG_INODE_OFFSET + 4]
            .copy_from_slice(&inode.to_ne_bytes());
        let rta_len = (4 + info.len()) as u16;
        body.extend_from_slice(&rta_len.to_ne_bytes());
        body.extend_from_slice(&INET_DIAG_INFO.to_ne_bytes());
        body.extend_from_slice(&info);
        let length = (NLMSG_HEADER + body.len()) as u32;
        let mut message = Vec::new();
        message.extend_from_slice(&length.to_ne_bytes());
        message.extend_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
        message.extend_from_slice(&[0u8; 10]);
        message.extend_from_slice(&body);
        message
    }

    #[test]
    fn a_sock_diag_dump_yields_each_sockets_counters() {
        let mut bytes = diag_message(917, 20_510_274, 2_349_475);
        bytes.extend(diag_message(0, 5, 5));
        bytes.extend(diag_message(918, 1, 2));
        let mut done = vec![0u8; 20];
        done[0..4].copy_from_slice(&20u32.to_ne_bytes());
        done[4..6].copy_from_slice(&NLMSG_DONE.to_ne_bytes());
        bytes.extend(done);
        assert_eq!(
            parse_sock_diag(&bytes),
            vec![
                SocketCounters {
                    inode: 917,
                    sent: 20_510_274,
                    received: 2_349_475
                },
                SocketCounters {
                    inode: 918,
                    sent: 1,
                    received: 2
                },
            ]
        );
    }

    #[test]
    fn a_truncated_dump_ends_the_parse_instead_of_reading_past_it() {
        let mut bytes = diag_message(917, 1, 2);
        bytes.truncate(bytes.len() - 10);
        assert!(parse_sock_diag(&bytes).is_empty());
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = ProcessUsageCollector::descriptors();
        let mut ids: Vec<_> = descriptors.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), descriptors.len());
    }
}
