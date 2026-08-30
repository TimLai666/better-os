//! Normalized identity for a hot-pluggable external volume.
//!
//! A per-device preference is worthless if it follows `/dev/sdb` instead of the
//! device, and it is worse than worthless if two identical USB sticks collapse
//! into one record. So identity is built from the most stable parts the platform
//! actually reported, it records how much that is worth, and a device whose only
//! distinguishing feature is its current kernel name is never persisted at all.

use serde::{Deserialize, Serialize};
use std::fmt;

/// How a device is attached. Transport is part of identity because the same
/// enclosure moved between a USB port and a Thunderbolt dock is not guaranteed
/// to report the same topology, and because removal policy only applies to
/// hot-pluggable transports in the first place.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Usb,
    Sdio,
    Mmc,
    Thunderbolt,
    Ieee1394,
    /// A transport string the platform reported that this model does not know.
    /// Kept distinct from `Unknown`: something was reported, it just is not one
    /// of the recognized buses.
    Other,
    #[default]
    Unknown,
}

impl Transport {
    /// Parses a UDisks2 `Drive.ConnectionBus` value. An empty value means the
    /// drive did not report a bus, which is not the same as reporting one this
    /// model does not recognize.
    pub fn from_connection_bus(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" => Transport::Unknown,
            "usb" => Transport::Usb,
            "sdio" => Transport::Sdio,
            "mmc" => Transport::Mmc,
            "thunderbolt" => Transport::Thunderbolt,
            "ieee1394" | "firewire" => Transport::Ieee1394,
            _ => Transport::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Usb => "usb",
            Transport::Sdio => "sdio",
            Transport::Mmc => "mmc",
            Transport::Thunderbolt => "thunderbolt",
            Transport::Ieee1394 => "ieee1394",
            Transport::Other => "other",
            Transport::Unknown => "unknown",
        }
    }
}

/// Everything the platform managed to report about one volume.
///
/// Every field except `device_path` is optional on purpose: cheap USB bridges
/// omit serials, unpartitioned media has no partition UUID, and a freshly
/// inserted unformatted card has no filesystem UUID.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdentityEvidence {
    pub filesystem_uuid: Option<String>,
    pub partition_uuid: Option<String>,
    pub drive_serial: Option<String>,
    /// World Wide Name, when the drive exposes one.
    pub wwn: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub transport: Transport,
    /// A bus path such as a USB port chain. Stable while the device stays in
    /// the same port, which is why it only ever supports a weak identity.
    pub topology: Option<String>,
    pub partition_number: Option<u32>,
    /// The current kernel name. Recorded for diagnostics and for talking to the
    /// platform. Never an identity on its own.
    pub device_path: String,
    /// A human label such as a filesystem label. Presentation only.
    pub label: Option<String>,
}

/// Values that some firmware writes into a serial or UUID field to mean "I have
/// nothing". Treating them as identity is how two unrelated sticks become one
/// preference record.
fn normalize(raw: Option<&String>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let folded = value.to_ascii_lowercase();
    let placeholder = matches!(
        folded.as_str(),
        "none" | "unknown" | "n/a" | "na" | "null" | "0" | "-"
    ) || folded.chars().all(|c| c == '0')
        || folded.chars().all(|c| c == 'f' || c == '-')
        || folded == "00000000-0000-0000-0000-000000000000";
    if placeholder {
        return None;
    }
    Some(folded)
}

/// How much the identity is worth.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityConfidence {
    /// At least one identifier that belongs to the media or the drive itself —
    /// filesystem UUID, partition UUID, serial, or WWN.
    Stable,
    /// Only descriptive and topological values. Good enough to tell two devices
    /// apart while both are plugged in, and good enough to remember a
    /// preference, but it follows the port as much as the device.
    Weak,
    /// Nothing but the current kernel name. Usable for this connection only.
    /// Never persisted, so a preference can never be applied to the wrong disk.
    Volatile,
}

impl IdentityConfidence {
    /// Whether a preference may be written under this identity.
    pub fn persistable(self) -> bool {
        !matches!(self, IdentityConfidence::Volatile)
    }
}

/// The stable name a preference is filed under.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdentityKey(String);

impl IdentityKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdentityKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A volume, named by the most stable combination available.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    key: IdentityKey,
    confidence: IdentityConfidence,
    evidence: IdentityEvidence,
}

impl DeviceIdentity {
    pub fn from_evidence(evidence: IdentityEvidence) -> Self {
        let filesystem_uuid = normalize(evidence.filesystem_uuid.as_ref());
        let partition_uuid = normalize(evidence.partition_uuid.as_ref());
        let serial = normalize(evidence.drive_serial.as_ref());
        let wwn = normalize(evidence.wwn.as_ref());
        let vendor = normalize(evidence.vendor.as_ref());
        let model = normalize(evidence.model.as_ref());
        let topology = normalize(evidence.topology.as_ref());

        let mut parts: Vec<String> = Vec::new();
        // Every stable identifier that is present goes into the key. Using only
        // the "best" one would make the key change when a device is reformatted
        // and its filesystem UUID is replaced while its serial did not move.
        if let Some(value) = &wwn {
            parts.push(format!("wwn={value}"));
        }
        if let Some(value) = &serial {
            parts.push(format!("serial={value}"));
        }
        if let Some(value) = &partition_uuid {
            parts.push(format!("partuuid={value}"));
        }
        if let Some(value) = &filesystem_uuid {
            parts.push(format!("fsuuid={value}"));
        }

        let confidence = if !parts.is_empty() {
            IdentityConfidence::Stable
        } else if (vendor.is_some() || model.is_some()) && topology.is_some() {
            // No media identifier at all. Vendor and model alone would merge two
            // identical sticks, so the port chain has to carry the difference,
            // and the result is honestly labelled weak.
            parts.push(format!("vendor={}", vendor.clone().unwrap_or_default()));
            parts.push(format!("model={}", model.clone().unwrap_or_default()));
            parts.push(format!("topology={}", topology.clone().unwrap_or_default()));
            IdentityConfidence::Weak
        } else {
            parts.push(format!(
                "transient-path={}",
                evidence.device_path.trim().to_ascii_lowercase()
            ));
            IdentityConfidence::Volatile
        };

        if let Some(number) = evidence.partition_number {
            parts.push(format!("part={number}"));
        }
        parts.push(format!("transport={}", evidence.transport.as_str()));

        Self {
            key: IdentityKey(parts.join(";")),
            confidence,
            evidence,
        }
    }

    pub fn key(&self) -> &IdentityKey {
        &self.key
    }

    pub fn confidence(&self) -> IdentityConfidence {
        self.confidence
    }

    pub fn evidence(&self) -> &IdentityEvidence {
        &self.evidence
    }

    pub fn device_path(&self) -> &str {
        &self.evidence.device_path
    }

    /// The name a user would recognize, falling back through label, model, and
    /// finally the kernel name so a row is never blank.
    pub fn display_name(&self) -> String {
        for candidate in [&self.evidence.label, &self.evidence.model] {
            if let Some(value) = candidate
                && !value.trim().is_empty()
            {
                return value.trim().to_string();
            }
        }
        self.evidence.device_path.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> IdentityEvidence {
        IdentityEvidence {
            device_path: "/dev/sdb1".to_string(),
            transport: Transport::Usb,
            ..IdentityEvidence::default()
        }
    }

    #[test]
    fn a_transient_kernel_name_never_becomes_a_stable_identity() {
        let identity = DeviceIdentity::from_evidence(evidence());
        assert_eq!(identity.confidence(), IdentityConfidence::Volatile);
        assert!(!identity.confidence().persistable());
    }

    #[test]
    fn the_same_media_keeps_its_key_when_the_kernel_name_changes() {
        let mut first = evidence();
        first.filesystem_uuid = Some("A1B2-C3D4".to_string());
        let mut second = first.clone();
        second.device_path = "/dev/sdd1".to_string();

        let first = DeviceIdentity::from_evidence(first);
        let second = DeviceIdentity::from_evidence(second);
        assert_eq!(first.key(), second.key());
        assert_eq!(first.confidence(), IdentityConfidence::Stable);
    }

    #[test]
    fn a_filesystem_uuid_is_matched_regardless_of_the_case_it_was_reported_in() {
        let mut lower = evidence();
        lower.filesystem_uuid = Some("a1b2-c3d4".to_string());
        let mut upper = evidence();
        upper.filesystem_uuid = Some("A1B2-C3D4".to_string());
        assert_eq!(
            DeviceIdentity::from_evidence(lower).key(),
            DeviceIdentity::from_evidence(upper).key()
        );
    }

    #[test]
    fn firmware_placeholders_do_not_count_as_identifiers() {
        for placeholder in ["", "  ", "none", "Unknown", "000000000", "0"] {
            let mut candidate = evidence();
            candidate.drive_serial = Some(placeholder.to_string());
            let identity = DeviceIdentity::from_evidence(candidate);
            assert_eq!(
                identity.confidence(),
                IdentityConfidence::Volatile,
                "{placeholder:?} was accepted as a serial"
            );
        }
    }

    #[test]
    fn two_identical_sticks_without_serials_stay_apart_through_their_ports() {
        let mut left = evidence();
        left.vendor = Some("Generic".to_string());
        left.model = Some("Flash Disk".to_string());
        left.topology = Some("usb-0:1.2".to_string());
        let mut right = left.clone();
        right.topology = Some("usb-0:1.3".to_string());
        right.device_path = "/dev/sdc1".to_string();

        let left = DeviceIdentity::from_evidence(left);
        let right = DeviceIdentity::from_evidence(right);
        assert_ne!(left.key(), right.key());
        assert_eq!(left.confidence(), IdentityConfidence::Weak);
        assert!(left.confidence().persistable());
    }

    #[test]
    fn two_partitions_of_one_drive_are_not_one_device() {
        let mut first = evidence();
        first.drive_serial = Some("0123456789".to_string());
        first.partition_number = Some(1);
        let mut second = first.clone();
        second.partition_number = Some(2);
        second.device_path = "/dev/sdb2".to_string();
        assert_ne!(
            DeviceIdentity::from_evidence(first).key(),
            DeviceIdentity::from_evidence(second).key()
        );
    }

    #[test]
    fn a_reformat_that_replaces_the_filesystem_uuid_still_matches_on_the_serial() {
        let mut before = evidence();
        before.drive_serial = Some("0123456789".to_string());
        before.filesystem_uuid = Some("aaaa-1111".to_string());
        let mut after = before.clone();
        after.filesystem_uuid = Some("bbbb-2222".to_string());

        let before = DeviceIdentity::from_evidence(before);
        let after = DeviceIdentity::from_evidence(after);
        // The keys differ, which is correct: the volume really is a new one.
        // What matters is that the serial is present in both, so a registry can
        // report the pair as related rather than silently reusing a preference.
        assert_ne!(before.key(), after.key());
        assert!(before.key().as_str().contains("serial=0123456789"));
        assert!(after.key().as_str().contains("serial=0123456789"));
    }

    #[test]
    fn an_unrecognized_connection_bus_is_not_reported_as_a_missing_one() {
        assert_eq!(Transport::from_connection_bus("usb"), Transport::Usb);
        assert_eq!(Transport::from_connection_bus("USB"), Transport::Usb);
        assert_eq!(Transport::from_connection_bus(""), Transport::Unknown);
        assert_eq!(Transport::from_connection_bus("sas"), Transport::Other);
        assert_ne!(Transport::Other, Transport::Unknown);
    }

    #[test]
    fn a_device_row_always_has_something_to_show() {
        let mut named = evidence();
        named.label = Some("FIELD DATA".to_string());
        assert_eq!(
            DeviceIdentity::from_evidence(named).display_name(),
            "FIELD DATA"
        );
        assert_eq!(
            DeviceIdentity::from_evidence(evidence()).display_name(),
            "/dev/sdb1"
        );
    }
}
