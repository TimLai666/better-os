//! The periodic audit: what the machine *is*.
//!
//! This is the second layer of the specification's layered observation. The
//! continuous layer samples what the machine is doing every second; this one
//! runs every few minutes and asks a different question, because a kernel
//! upgrade, a disk that disappeared, or a Better OS component that changed
//! version explains slowdowns that no amount of CPU sampling ever will.
//!
//! Everything here is read through [`Roots`], the same seam the collectors
//! use, so an audit can be run against a captured `/proc` and `/sys` tree in a
//! test rather than only against the machine the test happens to run on.
//!
//! Every entry is classified where it is collected. That classification is
//! what the exporter's redaction reads; deciding it here, next to the read
//! that produced the value, is the only place the truth is actually known.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use monitor_collectors_linux::Roots;
use monitor_core::{MetricDescriptor, SupportState};
use monitor_store::{Inventory, InventoryEntry};

/// Where the session's own facts come from. Separated from [`Roots`] so a test
/// can supply them instead of inheriting the developer's environment.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionFacts {
    pub user: Option<String>,
    pub home: Option<String>,
    pub desktop: Option<String>,
    pub session_type: Option<String>,
    pub display_server: Option<String>,
}

impl SessionFacts {
    /// What the running session actually says about itself.
    pub fn from_environment() -> Self {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let session_type = read("XDG_SESSION_TYPE");
        Self {
            user: read("USER").or_else(|| read("LOGNAME")),
            home: read("HOME"),
            desktop: read("XDG_CURRENT_DESKTOP"),
            display_server: match session_type.as_deref() {
                Some("wayland") => Some("wayland".to_string()),
                Some("x11") => Some("x11".to_string()),
                // Neither variable set is a real answer on a headless
                // service, and it is not the same answer as "x11".
                _ => read("WAYLAND_DISPLAY")
                    .map(|_| "wayland".to_string())
                    .or_else(|| read("DISPLAY").map(|_| "x11".to_string())),
            },
            session_type,
        }
    }
}

/// Where a Better OS component version can be read from, if anywhere.
///
/// The manager's own state file is the source of truth for what Better OS
/// installed. Reading it is optional: a machine with no Better Manager state
/// yet is a normal machine, not a broken one, and the audit says so by leaving
/// the keys out rather than by inventing zeroes.
#[derive(Clone, Debug)]
pub struct ComponentVersions {
    path: Option<PathBuf>,
}

impl Default for ComponentVersions {
    fn default() -> Self {
        Self::from_manager_store()
    }
}

impl ComponentVersions {
    pub fn from_manager_store() -> Self {
        Self {
            path: Some(
                manager_store::JsonStore::from_default_path()
                    .path()
                    .to_path_buf(),
            ),
        }
    }

    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
        }
    }

    /// No manager state to read. Used by tests and by an audit that has been
    /// told not to look.
    pub fn none() -> Self {
        Self { path: None }
    }

    fn read(&self) -> BTreeMap<String, String> {
        let Some(path) = &self.path else {
            return BTreeMap::new();
        };
        let store = manager_store::JsonStore::at_path(path);
        // A missing, unreadable, or newer-schema state file is not an error
        // here. It means the audit cannot report component versions, which is
        // exactly what leaving the keys out says.
        let Ok(outcome) = manager_store::StateStore::load(&store) else {
            return BTreeMap::new();
        };
        outcome
            .state
            .components
            .iter()
            .filter_map(|(id, record)| {
                record
                    .installed_version
                    .as_ref()
                    .map(|version| (id.to_string(), version.clone()))
            })
            .collect()
    }
}

/// What an audit is allowed to look at.
#[derive(Clone, Debug)]
pub struct AuditSources {
    pub roots: Roots,
    pub session: SessionFacts,
    pub components: ComponentVersions,
}

impl AuditSources {
    pub fn system() -> Self {
        Self {
            roots: Roots::system(),
            session: SessionFacts::from_environment(),
            components: ComponentVersions::default(),
        }
    }
}

/// Run one audit.
///
/// Nothing here fails. A file that cannot be read produces no key, because an
/// inventory that reported `unknown` for everything it could not open would be
/// mostly noise, and the diff between two audits is what actually matters.
pub fn collect(
    sources: &AuditSources,
    capabilities: &[(String, Vec<MetricDescriptor>, Vec<SupportState>)],
    now_unix_ms: u64,
) -> Inventory {
    let mut inventory = Inventory::new(now_unix_ms);
    let roots = &sources.roots;

    collect_os(&mut inventory, roots);
    collect_kernel(&mut inventory, roots);
    collect_session(&mut inventory, &sources.session);
    collect_cpu(&mut inventory, roots);
    collect_memory(&mut inventory, roots);
    collect_graphics(&mut inventory, roots);
    collect_storage(&mut inventory, roots);
    collect_mounts(&mut inventory, roots);
    collect_network(&mut inventory, roots);

    for (id, version) in sources.components.read() {
        inventory.insert(
            format!("betteros.component.{id}.version"),
            InventoryEntry::public(version),
        );
    }

    collect_capabilities(&mut inventory, capabilities);
    inventory
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// `/etc`, derived from the passwd path so a snapshot root redirects it too.
fn etc_dir(roots: &Roots) -> PathBuf {
    roots
        .passwd_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/etc"))
}

fn collect_os(inventory: &mut Inventory, roots: &Roots) {
    let Some(text) = read_trimmed(&etc_dir(roots).join("os-release")) else {
        return;
    };
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        if !value.is_empty() {
            fields.insert(key.trim().to_string(), value);
        }
    }
    for (field, key) in [
        ("NAME", "os.name"),
        ("VERSION", "os.version"),
        ("ID", "os.id"),
        ("VERSION_ID", "os.version_id"),
        ("VARIANT", "os.variant"),
    ] {
        if let Some(value) = fields.get(field) {
            inventory.insert(key, InventoryEntry::public(value.clone()));
        }
    }
}

fn collect_kernel(inventory: &mut Inventory, roots: &Roots) {
    if let Some(release) = read_trimmed(&roots.proc("sys/kernel/osrelease")) {
        inventory.insert("kernel.release", InventoryEntry::public(release));
    }
    if let Some(version) = read_trimmed(&roots.proc("sys/kernel/version")) {
        inventory.insert("kernel.version", InventoryEntry::public(version));
    }
    // The hostname is often the owner's name or their employer's asset tag.
    if let Some(hostname) = read_trimmed(&roots.proc("sys/kernel/hostname")) {
        inventory.insert("host.name", InventoryEntry::personal(hostname));
    }
}

fn collect_session(inventory: &mut Inventory, session: &SessionFacts) {
    if let Some(user) = &session.user {
        inventory.insert("session.user", InventoryEntry::personal(user.clone()));
    }
    if let Some(home) = &session.home {
        inventory.insert("session.home", InventoryEntry::personal(home.clone()));
    }
    if let Some(desktop) = &session.desktop {
        inventory.insert("session.desktop", InventoryEntry::public(desktop.clone()));
    }
    if let Some(kind) = &session.session_type {
        inventory.insert("session.type", InventoryEntry::public(kind.clone()));
    }
    if let Some(server) = &session.display_server {
        inventory.insert(
            "session.display_protocol",
            InventoryEntry::public(server.clone()),
        );
    }
}

fn collect_cpu(inventory: &mut Inventory, roots: &Roots) {
    let Some(text) = read_trimmed(&roots.proc("cpuinfo")) else {
        return;
    };
    let mut logical = 0u32;
    let mut model: Option<String> = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "processor" {
            logical += 1;
        } else if key == "model name" && model.is_none() {
            model = Some(value.to_string());
        }
    }
    if let Some(model) = model {
        inventory.insert("cpu.model", InventoryEntry::public(model));
    }
    if logical > 0 {
        inventory.insert(
            "cpu.logical_count",
            InventoryEntry::public(logical.to_string()),
        );
    }
}

fn collect_memory(inventory: &mut Inventory, roots: &Roots) {
    let Some(text) = read_trimmed(&roots.proc("meminfo")) else {
        return;
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != "MemTotal" {
            continue;
        }
        // `/proc/meminfo` counts kibibytes. The conversion happens once, here,
        // the same rule the collectors follow.
        if let Some(kibibytes) = value
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok())
        {
            inventory.insert(
                "memory.total_bytes",
                InventoryEntry::public((kibibytes * 1024).to_string()),
            );
        }
        return;
    }
}

/// GPU identity, only where it is cheap.
///
/// The PCI vendor and device ids under `/sys/class/drm` are two small file
/// reads and name the adapter unambiguously. Anything more — engine
/// utilization, memory, power — needs a driver adapter that this ticket does
/// not build, and inventing a partial answer here would make the GPU page look
/// supported when it is not.
fn collect_graphics(inventory: &mut Inventory, roots: &Roots) {
    let Ok(entries) = std::fs::read_dir(roots.sys("class/drm")) else {
        return;
    };
    let mut cards: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        // `card0` yes, `card0-DP-1` no: the connectors are not adapters.
        .filter(|name| name.starts_with("card") && !name.contains('-'))
        .collect();
    cards.sort();
    for card in cards {
        let device = roots.sys(&format!("class/drm/{card}/device"));
        let vendor = read_trimmed(&device.join("vendor"));
        let identifier = read_trimmed(&device.join("device"));
        if let (Some(vendor), Some(identifier)) = (vendor, identifier) {
            inventory.insert(
                format!("graphics.{card}.pci_id"),
                InventoryEntry::public(format!("{vendor}:{identifier}")),
            );
        }
        if let Some(driver) = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
        {
            inventory.insert(
                format!("graphics.{card}.driver"),
                InventoryEntry::public(driver),
            );
        }
    }
}

fn collect_storage(inventory: &mut Inventory, roots: &Roots) {
    let Ok(entries) = std::fs::read_dir(roots.sys("block")) else {
        return;
    };
    let mut devices: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with("loop") && !name.starts_with("ram"))
        .collect();
    devices.sort();
    for device in devices {
        let base = roots.sys(&format!("block/{device}"));
        if let Some(sectors) = read_trimmed(&base.join("size")).and_then(|s| s.parse::<u64>().ok())
        {
            // `size` is in 512-byte sectors regardless of the physical sector
            // size, which is the one thing about this file everyone gets wrong.
            inventory.insert(
                format!("storage.{device}.size_bytes"),
                InventoryEntry::public((sectors * 512).to_string()),
            );
        }
        if let Some(rotational) = read_trimmed(&base.join("queue/rotational")) {
            inventory.insert(
                format!("storage.{device}.rotational"),
                InventoryEntry::public(rotational),
            );
        }
        if let Some(model) = read_trimmed(&base.join("device/model")) {
            inventory.insert(
                format!("storage.{device}.model"),
                InventoryEntry::public(model),
            );
        }
        // A serial names one physical unit and is exactly the kind of thing an
        // export must not carry.
        if let Some(serial) = read_trimmed(&base.join("device/serial")) {
            inventory.insert(
                format!("storage.{device}.serial"),
                InventoryEntry::identifier(serial),
            );
        }
    }
}

fn collect_mounts(inventory: &mut Inventory, roots: &Roots) {
    let Some(text) = read_trimmed(&roots.proc("mounts")) else {
        return;
    };
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(source), Some(target), Some(kind)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        // Only mounts backed by a real block device. The dozens of kernel
        // pseudo-filesystems would drown the diff without ever changing.
        if !source.starts_with("/dev/") {
            continue;
        }
        // A mount point can be under the user's home, so the classification
        // has to follow the value rather than the key.
        inventory.insert(
            format!("filesystem{}.type", target.replace('/', ".")),
            InventoryEntry::public(kind.to_string()),
        );
        inventory.insert(
            format!("filesystem{}.source", target.replace('/', ".")),
            InventoryEntry::public(source.to_string()),
        );
    }
}

fn collect_network(inventory: &mut Inventory, roots: &Roots) {
    let Ok(entries) = std::fs::read_dir(roots.sys("class/net")) else {
        return;
    };
    let mut links: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    links.sort();
    for link in links {
        let base = roots.sys(&format!("class/net/{link}"));
        if let Some(address) = read_trimmed(&base.join("address")) {
            inventory.insert(
                format!("network.{link}.mac"),
                InventoryEntry::identifier(address),
            );
        }
        if let Some(state) = read_trimmed(&base.join("operstate")) {
            inventory.insert(
                format!("network.{link}.state"),
                InventoryEntry::public(state),
            );
        }
    }
}

/// What this build can and cannot observe.
///
/// The unavailable metrics are the point. A machine where PSI is missing and a
/// machine where PSI reads zero are different machines, and only the inventory
/// records which one this is.
fn collect_capabilities(
    inventory: &mut Inventory,
    capabilities: &[(String, Vec<MetricDescriptor>, Vec<SupportState>)],
) {
    for (collector, descriptors, support) in capabilities {
        let mut unavailable = Vec::new();
        let mut supported = 0u32;
        for (descriptor, state) in descriptors.iter().zip(support.iter()) {
            match state {
                SupportState::Supported => supported += 1,
                SupportState::Unsupported(_) => {
                    unavailable.push(format!("{}=unsupported", descriptor.id))
                }
                SupportState::PermissionDenied { .. } => {
                    unavailable.push(format!("{}=permission_denied", descriptor.id))
                }
                SupportState::Unknown => unavailable.push(format!("{}=unknown", descriptor.id)),
            }
        }
        inventory.insert(
            format!("collector.{collector}.supported_metrics"),
            InventoryEntry::public(supported.to_string()),
        );
        if !unavailable.is_empty() {
            inventory.insert(
                format!("collector.{collector}.unavailable_metrics"),
                InventoryEntry::public(unavailable.join(" ")),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_store::Sensitivity;

    /// A minimal captured machine, laid out the way `Roots::at` expects.
    fn snapshot() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let write = |relative: &str, contents: &str| {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        };
        write(
            "etc/os-release",
            "NAME=\"Zorin OS\"\nVERSION=\"18 (Core)\"\nID=zorin\nVERSION_ID=\"18\"\n",
        );
        write("etc/passwd", "tim:x:1000:1000::/home/tim:/bin/bash\n");
        write("proc/sys/kernel/osrelease", "6.11.0-19-generic\n");
        write("proc/sys/kernel/version", "#19-Ubuntu SMP\n");
        write("proc/sys/kernel/hostname", "workshop\n");
        write(
            "proc/cpuinfo",
            "processor\t: 0\nmodel name\t: AMD Ryzen 7 7840U\n\nprocessor\t: 1\nmodel name\t: AMD Ryzen 7 7840U\n",
        );
        write(
            "proc/meminfo",
            "MemTotal:       32768000 kB\nMemFree:  100 kB\n",
        );
        write(
            "proc/mounts",
            "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\nproc /proc proc rw 0 0\n/dev/nvme0n1p1 /boot/efi vfat rw 0 0\n",
        );
        write("sys/block/nvme0n1/size", "1000215216\n");
        write("sys/block/nvme0n1/queue/rotational", "0\n");
        write("sys/block/nvme0n1/device/model", "WD_BLACK SN770\n");
        write("sys/block/nvme0n1/device/serial", "23071X800123\n");
        write("sys/block/loop0/size", "8\n");
        write("sys/class/net/eth0/address", "aa:bb:cc:dd:ee:ff\n");
        write("sys/class/net/eth0/operstate", "up\n");
        write("sys/class/net/lo/address", "00:00:00:00:00:00\n");
        write("sys/class/drm/card0/device/vendor", "0x1002\n");
        write("sys/class/drm/card0/device/device", "0x15bf\n");
        write("sys/class/drm/card0-DP-1/device/vendor", "0x1002\n");
        directory
    }

    fn sources(root: &Path) -> AuditSources {
        AuditSources {
            roots: Roots::at(root),
            session: SessionFacts {
                user: Some("tim".into()),
                home: Some("/home/tim".into()),
                desktop: Some("XFCE".into()),
                session_type: Some("wayland".into()),
                display_server: Some("wayland".into()),
            },
            components: ComponentVersions::none(),
        }
    }

    fn audit(root: &Path) -> Inventory {
        collect(&sources(root), &[], 1_700_000_000_000)
    }

    #[test]
    fn the_audit_reads_the_operating_system_and_kernel() {
        let directory = snapshot();
        let inventory = audit(directory.path());
        assert_eq!(inventory.get("os.name").unwrap().value, "Zorin OS");
        assert_eq!(inventory.get("os.version").unwrap().value, "18 (Core)");
        assert_eq!(
            inventory.get("kernel.release").unwrap().value,
            "6.11.0-19-generic"
        );
    }

    #[test]
    fn the_hostname_and_the_session_user_are_classified_as_personal() {
        let directory = snapshot();
        let inventory = audit(directory.path());
        for key in ["host.name", "session.user", "session.home"] {
            assert_eq!(
                inventory.get(key).unwrap().sensitivity,
                Sensitivity::Personal,
                "{key} must be personal"
            );
        }
        assert_eq!(
            inventory.get("session.desktop").unwrap().sensitivity,
            Sensitivity::Public
        );
    }

    #[test]
    fn a_hardware_address_and_a_disk_serial_are_classified_as_identifiers() {
        let directory = snapshot();
        let inventory = audit(directory.path());
        assert_eq!(
            inventory.get("network.eth0.mac").unwrap().sensitivity,
            Sensitivity::Identifier
        );
        assert_eq!(
            inventory.get("storage.nvme0n1.serial").unwrap().sensitivity,
            Sensitivity::Identifier
        );
        // The model is not an identity and stays readable.
        assert_eq!(
            inventory.get("storage.nvme0n1.model").unwrap().sensitivity,
            Sensitivity::Public
        );
    }

    #[test]
    fn the_cpu_and_memory_totals_are_read_and_converted_once() {
        let directory = snapshot();
        let inventory = audit(directory.path());
        assert_eq!(
            inventory.get("cpu.model").unwrap().value,
            "AMD Ryzen 7 7840U"
        );
        assert_eq!(inventory.get("cpu.logical_count").unwrap().value, "2");
        assert_eq!(
            inventory.get("memory.total_bytes").unwrap().value,
            (32_768_000u64 * 1024).to_string()
        );
    }

    #[test]
    fn a_disk_size_is_reported_in_bytes_from_512_byte_sectors() {
        let directory = snapshot();
        let inventory = audit(directory.path());
        assert_eq!(
            inventory.get("storage.nvme0n1.size_bytes").unwrap().value,
            (1_000_215_216u64 * 512).to_string()
        );
        assert!(inventory.get("storage.loop0.size_bytes").is_none());
    }

    #[test]
    fn a_display_connector_is_not_reported_as_a_graphics_adapter() {
        let directory = snapshot();
        let inventory = audit(directory.path());
        assert_eq!(
            inventory.get("graphics.card0.pci_id").unwrap().value,
            "0x1002:0x15bf"
        );
        assert!(inventory.get("graphics.card0-DP-1.pci_id").is_none());
    }

    #[test]
    fn only_block_device_mounts_are_recorded() {
        let directory = snapshot();
        let inventory = audit(directory.path());
        assert_eq!(inventory.get("filesystem..type").unwrap().value, "ext4");
        assert_eq!(
            inventory.get("filesystem.boot.efi.type").unwrap().value,
            "vfat"
        );
        assert!(inventory.get("filesystem.proc.type").is_none());
    }

    #[test]
    fn what_cannot_be_observed_is_recorded_as_such() {
        use monitor_core::{MetricSource, SamplingBehavior, SemanticType, Unit, UnsupportedReason};
        use std::time::Duration;

        let descriptor = |raw: &str| {
            MetricDescriptor::new(
                monitor_core::MetricId::new(raw).unwrap(),
                Unit::Percent,
                SemanticType::Saturation,
                MetricSource::Proc("pressure/cpu".into()),
                SamplingBehavior::kernel_averaged(Duration::from_secs(10)),
                "test",
            )
        };
        let directory = snapshot();
        let inventory = collect(
            &sources(directory.path()),
            &[(
                "linux.pressure".to_string(),
                vec![
                    descriptor("pressure.some.avg10"),
                    descriptor("pressure.full.avg10"),
                ],
                vec![
                    SupportState::Supported,
                    SupportState::Unsupported(UnsupportedReason::InterfaceMissing {
                        path: "/proc/pressure/cpu".into(),
                    }),
                ],
            )],
            1,
        );
        assert_eq!(
            inventory
                .get("collector.linux.pressure.supported_metrics")
                .unwrap()
                .value,
            "1"
        );
        assert_eq!(
            inventory
                .get("collector.linux.pressure.unavailable_metrics")
                .unwrap()
                .value,
            "pressure.full.avg10=unsupported"
        );
    }

    #[test]
    fn a_collector_that_supports_everything_lists_nothing_as_unavailable() {
        let directory = snapshot();
        let inventory = collect(
            &sources(directory.path()),
            &[("linux.cpu".to_string(), Vec::new(), Vec::new())],
            1,
        );
        assert_eq!(
            inventory
                .get("collector.linux.cpu.supported_metrics")
                .unwrap()
                .value,
            "0"
        );
        assert!(
            inventory
                .get("collector.linux.cpu.unavailable_metrics")
                .is_none()
        );
    }

    #[test]
    fn a_machine_with_nothing_readable_produces_an_empty_audit_rather_than_an_error() {
        let empty = tempfile::tempdir().unwrap();
        let sources = AuditSources {
            roots: Roots::at(empty.path()),
            session: SessionFacts::default(),
            components: ComponentVersions::none(),
        };
        let inventory = collect(&sources, &[], 42);
        assert!(inventory.entries.is_empty());
        assert_eq!(inventory.captured_at_unix_ms, 42);
    }

    #[test]
    fn two_audits_of_an_unchanged_machine_are_equal_apart_from_their_timestamps() {
        let directory = snapshot();
        let first = audit(directory.path());
        let second = collect(&sources(directory.path()), &[], 9_999);
        assert!(!first.differs_from(&second));
        assert!(monitor_store::inventory_diff(&first, &second).is_empty());
    }

    #[test]
    fn a_kernel_upgrade_shows_up_in_the_next_audit() {
        let directory = snapshot();
        let before = audit(directory.path());
        std::fs::write(
            directory.path().join("proc/sys/kernel/osrelease"),
            "6.14.0-2-generic\n",
        )
        .unwrap();
        let after = audit(directory.path());
        let changes = monitor_store::inventory_diff(&before, &after);
        assert_eq!(changes.changed.len(), 1);
        assert_eq!(changes.changed[0].key, "kernel.release");
    }

    #[test]
    fn component_versions_come_from_the_manager_state_when_it_is_readable() {
        let directory = snapshot();
        let state = directory.path().join("manager-state.json");
        // The shape `manager-store` actually writes, copied rather than
        // guessed: a state file this audit could not parse would make the test
        // pass for the wrong reason.
        std::fs::write(
            &state,
            r#"{"schema_version":2,"revision":4,"components":{"better-monitor":{"installed_version":"0.1.0","enabled":true,"health":"healthy","restore_snapshot":null,"failure":null,"recovery":null}},"activity":[],"settings":{"release_channel":"stable","locale":"system","check_updates":true,"auto_download":false,"diagnostic_logs":false,"onboarding_complete":true,"component_filter":"all"},"active_operation":null}"#,
        )
        .unwrap();

        let mut sources = sources(directory.path());
        sources.components = ComponentVersions::at_path(&state);
        let inventory = collect(&sources, &[], 1);
        assert_eq!(
            inventory
                .get("betteros.component.better-monitor.version")
                .unwrap()
                .value,
            "0.1.0"
        );
    }

    #[test]
    fn an_unreadable_manager_state_leaves_the_component_keys_out_rather_than_guessing() {
        let directory = snapshot();
        let mut sources = sources(directory.path());
        sources.components = ComponentVersions::at_path(directory.path().join("absent.json"));
        let inventory = collect(&sources, &[], 1);
        assert!(
            !inventory
                .entries
                .keys()
                .any(|key| key.starts_with("betteros.component."))
        );
    }
}
