//! Read-only SMBIOS memory inventory.
//!
//! The daemon reads the root-protected sysfs tables and returns a bounded,
//! privacy-minimized report. Serial numbers and asset tags are deliberately
//! never copied into the wire contract.

use std::fs;

#[cfg(any(test, feature = "test-support"))]
use std::sync::Arc;

#[cfg(any(test, feature = "test-support"))]
use dmidecode::structures::memory_device::{
    Detail, FormFactor, MemoryDevice as RawMemoryDevice, Type,
};
use dmidecode::{EntryPoint, Structure};
use monitor_ipc::{MemoryDevice, MemoryReport, PROTOCOL_VERSION};

use crate::DaemonError;

const ENTRY_POINT_PATH: &str = "/sys/firmware/dmi/tables/smbios_entry_point";
const TABLE_PATH: &str = "/sys/firmware/dmi/tables/DMI";
const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

pub trait MemoryInventory: Send + Sync {
    fn read(&self) -> Result<MemoryReport, DaemonError>;
}

pub struct SystemMemoryInventory;

impl MemoryInventory for SystemMemoryInventory {
    fn read(&self) -> Result<MemoryReport, DaemonError> {
        let entry_bytes = fs::read(ENTRY_POINT_PATH)
            .map_err(|error| DaemonError::HostUnreadable(error.to_string()))?;
        let table_bytes =
            fs::read(TABLE_PATH).map_err(|error| DaemonError::HostUnreadable(error.to_string()))?;
        parse_memory_report(&entry_bytes, &table_bytes)
    }
}

pub fn parse_memory_report(
    entry_bytes: &[u8],
    table_bytes: &[u8],
) -> Result<MemoryReport, DaemonError> {
    let entry = EntryPoint::search(entry_bytes)
        .map_err(|error| DaemonError::HostUnreadable(error.to_string()))?;
    let mut devices = Vec::new();
    for structure in entry.structures(table_bytes) {
        let structure =
            structure.map_err(|error| DaemonError::HostUnreadable(error.to_string()))?;
        if let Structure::MemoryDevice(device) = structure {
            devices.push(convert_memory_device(&device));
        }
    }

    let report = MemoryReport {
        protocol_version: PROTOCOL_VERSION,
        smbios_major: entry.major(),
        smbios_minor: entry.minor(),
        devices,
    };
    report.validate().map_err(DaemonError::from)?;
    Ok(report)
}

fn convert_memory_device(device: &RawMemoryDevice<'_>) -> MemoryDevice {
    let size_bytes = memory_size_bytes(device);
    let installed = size_bytes.is_some();
    MemoryDevice {
        locator: normalize_required(device.device_locator, "Memory slot"),
        bank: normalize_optional(device.bank_locator),
        installed,
        size_bytes: installed.then_some(size_bytes).flatten(),
        speed_mt_s: device
            .extended_speed
            .filter(|speed| *speed > 0)
            .or_else(|| device.speed.map(u32::from)),
        configured_speed_mt_s: device
            .extended_configured_memory_speed
            .filter(|speed| *speed > 0)
            .or_else(|| device.configured_memory_speed.map(u32::from)),
        form_factor: form_factor_label(device.form_factor),
        memory_type: memory_type_label(device.memory_type),
        type_detail: detail_labels(device.type_detail.clone()),
        manufacturer: installed
            .then(|| normalize_optional(device.manufacturer))
            .flatten(),
        part_number: installed
            .then(|| normalize_optional(device.part_number))
            .flatten(),
        configured_voltage_mv: installed
            .then_some(device.configured_voltage.filter(|voltage| *voltage > 0))
            .flatten(),
    }
}

fn memory_size_bytes(device: &RawMemoryDevice<'_>) -> Option<u64> {
    if let Some(bytes) = device
        .volatile_size
        .filter(|value| *value > 0 && *value != u64::MAX)
    {
        return Some(bytes);
    }

    let size = device.size?;
    match size {
        0 => None,
        0x7fff => {
            let megabytes = u64::from(device.extended_size & 0x7fff_ffff);
            (megabytes > 0).then_some(megabytes.saturating_mul(MIB))
        }
        value if value & 0x8000 != 0 => Some(u64::from(value & 0x7fff).saturating_mul(KIB)),
        value => Some(u64::from(value).saturating_mul(MIB)),
    }
}

fn normalize_required(value: &str, fallback: &str) -> String {
    normalize_optional(value).unwrap_or_else(|| fallback.to_string())
}

fn normalize_optional(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('\0');
    let placeholder = value.is_empty()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "unknown"
                | "not specified"
                | "not provided"
                | "none"
                | "n/a"
                | "na"
                | "00000000"
                | "to be filled by o.e.m."
                | "default string"
        );
    (!placeholder).then(|| value.to_string())
}

fn form_factor_label(value: FormFactor) -> String {
    match value {
        FormFactor::Dimm => "DIMM".to_string(),
        FormFactor::SoDimm => "SO-DIMM".to_string(),
        FormFactor::FbDimm => "FB-DIMM".to_string(),
        FormFactor::Simm => "SIMM".to_string(),
        FormFactor::Rimm => "RIMM".to_string(),
        FormFactor::Srimm => "SRIMM".to_string(),
        FormFactor::Tsop => "TSOP".to_string(),
        FormFactor::Dip => "DIP".to_string(),
        FormFactor::Sip => "SIP".to_string(),
        FormFactor::Zip => "ZIP".to_string(),
        other => humanize_debug(other),
    }
}

fn memory_type_label(value: Type) -> String {
    match value {
        Type::Ddr => "DDR".to_string(),
        Type::Ddr2 => "DDR2".to_string(),
        Type::Ddr2FbDimm => "DDR2 FB-DIMM".to_string(),
        Type::Ddr3 => "DDR3".to_string(),
        Type::Ddr4 => "DDR4".to_string(),
        Type::Ddr5 => "DDR5".to_string(),
        Type::LpDdr => "LPDDR".to_string(),
        Type::LpDdr2 => "LPDDR2".to_string(),
        Type::LpDdr3 => "LPDDR3".to_string(),
        Type::LpDdr4 => "LPDDR4".to_string(),
        Type::LpDdr5 => "LPDDR5".to_string(),
        Type::Hbm => "HBM".to_string(),
        Type::Hbm2 => "HBM2".to_string(),
        Type::Sdram => "SDRAM".to_string(),
        Type::Sram => "SRAM".to_string(),
        Type::Dram => "DRAM".to_string(),
        Type::Edram => "EDRAM".to_string(),
        Type::Vram => "VRAM".to_string(),
        Type::Rdram => "RDRAM".to_string(),
        other => humanize_debug(other),
    }
}

fn detail_labels(detail: Detail) -> Vec<String> {
    let definitions = [
        (Detail::OTHER, "Other"),
        (Detail::UNKNOWN, "Unknown"),
        (Detail::FAST_PAGED, "Fast-paged"),
        (Detail::STATIC_COLUMN, "Static column"),
        (Detail::PSEUDO_STATIC, "Pseudo-static"),
        (Detail::RAMBUS, "Rambus"),
        (Detail::SYNCHRONOUS, "Synchronous"),
        (Detail::CMOS, "CMOS"),
        (Detail::EDO, "EDO"),
        (Detail::WINDOW_DRAM, "Window DRAM"),
        (Detail::CACHE_DRAM, "Cache DRAM"),
        (Detail::NON_VOLATILE, "Non-volatile"),
        (Detail::REGISTERED, "Registered"),
        (Detail::UNREGISTERED, "Unbuffered"),
        (Detail::LRDIMM, "LRDIMM"),
    ];
    definitions
        .into_iter()
        .filter_map(|(flag, label)| detail.contains(flag).then(|| label.to_string()))
        .collect()
}

fn humanize_debug(value: impl std::fmt::Debug) -> String {
    let raw = format!("{value:?}");
    if raw.starts_with("Undefined(") {
        return "Undefined".to_string();
    }
    let mut result = String::new();
    for (index, character) in raw.chars().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            result.push(' ');
        }
        result.push(character);
    }
    result
}

#[cfg(any(test, feature = "test-support"))]
pub struct FixedMemoryInventory {
    report: Arc<MemoryReport>,
}

#[cfg(any(test, feature = "test-support"))]
impl FixedMemoryInventory {
    pub fn new(report: MemoryReport) -> Self {
        Self {
            report: Arc::new(report),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl MemoryInventory for FixedMemoryInventory {
    fn read(&self) -> Result<MemoryReport, DaemonError> {
        Ok((*self.report).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dmidecode::structures::memory_device::{Detail, FormFactor, MemoryDevice, Type};

    #[test]
    fn size_encodings_are_converted_without_guessing() {
        let mut device = MemoryDevice {
            size: Some(8192),
            ..Default::default()
        };
        assert_eq!(memory_size_bytes(&device), Some(8192 * MIB));

        device.size = Some(0x8001);
        assert_eq!(memory_size_bytes(&device), Some(KIB));

        device.size = Some(0x7fff);
        device.extended_size = 65536;
        assert_eq!(memory_size_bytes(&device), Some(65536 * MIB));

        device.size = Some(0);
        assert_eq!(memory_size_bytes(&device), None);
    }

    #[test]
    fn newer_volatile_size_wins_and_sentinels_do_not() {
        let mut device = MemoryDevice {
            size: Some(1024),
            volatile_size: Some(12_345),
            ..Default::default()
        };
        assert_eq!(memory_size_bytes(&device), Some(12_345));
        device.volatile_size = Some(u64::MAX);
        assert_eq!(memory_size_bytes(&device), Some(1024 * MIB));
    }

    #[test]
    fn labels_are_stable_and_sensitive_placeholders_are_removed() {
        assert_eq!(form_factor_label(FormFactor::SoDimm), "SO-DIMM");
        assert_eq!(memory_type_label(Type::LpDdr5), "LPDDR5");
        assert_eq!(
            detail_labels(Detail::SYNCHRONOUS | Detail::UNREGISTERED),
            ["Synchronous", "Unbuffered"]
        );
        assert_eq!(normalize_optional("To Be Filled By O.E.M."), None);
        assert_eq!(normalize_optional("  Micron  "), Some("Micron".to_string()));
    }
}
