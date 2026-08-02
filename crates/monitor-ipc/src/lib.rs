//! Bounded wire types for Better Monitor's privileged read-only helpers.
//!
//! The GUI and the privileged daemon share only this serialized contract. The
//! daemon never returns raw SMBIOS bytes, serial numbers, asset tags, or file
//! paths. Every string and collection is bounded before it crosses D-Bus.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REPORT_BYTES: usize = 1024 * 1024;
pub const MAX_MEMORY_DEVICES: usize = 256;
pub const MAX_TEXT_BYTES: usize = 256;
pub const MAX_TYPE_DETAILS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDevice {
    pub locator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank: Option<String>,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_mt_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_speed_mt_s: Option<u32>,
    pub form_factor: String,
    pub memory_type: String,
    pub type_detail: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_voltage_mv: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReport {
    pub protocol_version: u32,
    pub smbios_major: u8,
    pub smbios_minor: u8,
    pub devices: Vec<MemoryDevice>,
}

impl MemoryReport {
    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_REPORT_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_REPORT_BYTES,
            });
        }
        let report: Self = serde_json::from_str(document)
            .map_err(|error| IpcError::Malformed(error.to_string()))?;
        report.validate()?;
        Ok(report)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        self.validate()?;
        let document =
            serde_json::to_string(self).map_err(|error| IpcError::Malformed(error.to_string()))?;
        if document.len() > MAX_REPORT_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_REPORT_BYTES,
            });
        }
        Ok(document)
    }

    pub fn validate(&self) -> Result<(), IpcError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: self.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        if self.devices.len() > MAX_MEMORY_DEVICES {
            return Err(IpcError::TooManyDevices {
                found: self.devices.len(),
                limit: MAX_MEMORY_DEVICES,
            });
        }
        for device in &self.devices {
            device.validate()?;
        }
        Ok(())
    }

    pub fn installed_devices(&self) -> impl Iterator<Item = &MemoryDevice> {
        self.devices.iter().filter(|device| device.installed)
    }
}

impl MemoryDevice {
    fn validate(&self) -> Result<(), IpcError> {
        validate_text("locator", &self.locator)?;
        validate_text("form_factor", &self.form_factor)?;
        validate_text("memory_type", &self.memory_type)?;
        for (field, value) in [
            ("bank", self.bank.as_deref()),
            ("manufacturer", self.manufacturer.as_deref()),
            ("part_number", self.part_number.as_deref()),
        ] {
            if let Some(value) = value {
                validate_text(field, value)?;
            }
        }
        if self.type_detail.len() > MAX_TYPE_DETAILS {
            return Err(IpcError::TooManyTypeDetails {
                found: self.type_detail.len(),
                limit: MAX_TYPE_DETAILS,
            });
        }
        for detail in &self.type_detail {
            validate_text("type_detail", detail)?;
        }
        if !self.installed && self.size_bytes.is_some() {
            return Err(IpcError::EmptySlotHasSize);
        }
        Ok(())
    }
}

fn validate_text(field: &'static str, value: &str) -> Result<(), IpcError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.contains('\0') {
        return Err(IpcError::InvalidText {
            field,
            bytes: value.len(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IpcError {
    #[error("monitor.ipc.payload_too_large:{bytes}:{limit}")]
    PayloadTooLarge { bytes: usize, limit: usize },
    #[error("monitor.ipc.malformed:{0}")]
    Malformed(String),
    #[error("monitor.ipc.protocol:{found}:{expected}")]
    ProtocolVersion { found: u32, expected: u32 },
    #[error("monitor.ipc.too_many_devices:{found}:{limit}")]
    TooManyDevices { found: usize, limit: usize },
    #[error("monitor.ipc.too_many_type_details:{found}:{limit}")]
    TooManyTypeDetails { found: usize, limit: usize },
    #[error("monitor.ipc.invalid_text:{field}:{bytes}")]
    InvalidText { field: &'static str, bytes: usize },
    #[error("monitor.ipc.empty_slot_has_size")]
    EmptySlotHasSize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> MemoryReport {
        MemoryReport {
            protocol_version: PROTOCOL_VERSION,
            smbios_major: 3,
            smbios_minor: 6,
            devices: vec![MemoryDevice {
                locator: "DIMM_A0".to_string(),
                bank: Some("BANK 0".to_string()),
                installed: true,
                size_bytes: Some(16 * 1024 * 1024 * 1024),
                speed_mt_s: Some(5600),
                configured_speed_mt_s: Some(5200),
                form_factor: "SO-DIMM".to_string(),
                memory_type: "DDR5".to_string(),
                type_detail: vec!["Synchronous".to_string()],
                manufacturer: Some("Example".to_string()),
                part_number: Some("ABC-123".to_string()),
                configured_voltage_mv: Some(1100),
            }],
        }
    }

    #[test]
    fn report_round_trips() {
        let report = sample_report();
        assert_eq!(
            MemoryReport::from_json(&report.to_json().unwrap()).unwrap(),
            report
        );
    }

    #[test]
    fn oversized_and_sensitive_strings_are_refused() {
        let mut report = sample_report();
        report.devices[0].part_number = Some("x".repeat(MAX_TEXT_BYTES + 1));
        assert!(matches!(
            report.validate(),
            Err(IpcError::InvalidText {
                field: "part_number",
                ..
            })
        ));

        let mut report = sample_report();
        report.devices[0].locator = "DIMM\0A0".to_string();
        assert!(matches!(
            report.validate(),
            Err(IpcError::InvalidText {
                field: "locator",
                ..
            })
        ));
    }

    #[test]
    fn empty_slots_cannot_claim_capacity() {
        let mut report = sample_report();
        report.devices[0].installed = false;
        assert_eq!(report.validate(), Err(IpcError::EmptySlotHasSize));
    }
}
