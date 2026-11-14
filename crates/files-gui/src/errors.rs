//! What the storage service's, the disk service's and the job engine's
//! errors mean, in words a person can act on, in the window's language.
//!
//! The layers below report stable keys (`storage.erase.busy`,
//! `files.operation.error.no_space:/media/stick/a.iso`), or, for the disk
//! service, an English sentence of a fixed shape
//! (`unmount was refused for /org/freedesktop/UDisks2/block_devices/sdc1:
//! target is busy`), not translated text. Everything the window shows one in
//! (a notice, a failed job's line) passes it through [`explain`] first; the
//! sentences themselves are [`crate::i18n::ErrorCopy`], one table per
//! language. Text that is neither is shown as it is.

use crate::i18n::{Copy, ErrorCopy};

/// The sentence for `text`, with what follows its key (a path, a device, a
/// detail) where the sentence has a place for it. Text that is no key or
/// disk service sentence this knows comes back unchanged.
pub fn explain(text: &str, c: &'static Copy) -> String {
    let e = &c.errors;
    // A refusal relayed by the storage client wraps the service's own key
    // or the disk service's sentence.
    let text = text
        .strip_prefix("storage.client.error.rejected:")
        .unwrap_or(text);
    // A key's detail may quote a disk service sentence; only text that is
    // not a key is one.
    if !text.starts_with("storage.")
        && !text.starts_with("files.")
        && let Some(sentence) = platform(text, e)
    {
        return sentence;
    }
    let (key, detail) = match text.split_once(':') {
        Some((key, detail)) if key.contains('.') && !key.contains(' ') => (key, detail),
        _ => (text, ""),
    };
    let detail = detail.trim();
    let sentence = match key {
        // --- The storage service -------------------------------------
        "storage.erase.busy" => e.erase_busy,
        "storage.erase.not_confirmed" => e.erase_not_confirmed,
        "storage.image.unmount_failed" => {
            let device = detail.split(':').next().unwrap_or(detail);
            return e.image_unmount_failed.replace("{device}", device);
        }
        "storage.image.not_a_drive" => e.image_not_a_drive,
        "storage.image.not_a_file" => e.image_not_a_file,
        "storage.format.label" => e.format_label,
        "storage.client.error.service_unavailable"
        | "storage.client.error.transport"
        | "storage.client.error.protocol"
        | "storage.client.error.protocol_version" => e.service_unavailable,
        // --- The job engine ------------------------------------------
        "files.operation.error.no_space" => e.no_space,
        "files.operation.error.quota_exceeded" => e.quota_exceeded,
        "files.operation.error.permission_denied" => e.permission_denied,
        "files.operation.error.read_only" => e.read_only,
        "files.operation.error.not_found" => e.not_found,
        "files.operation.error.already_exists" => e.already_exists,
        "files.operation.error.not_a_directory" => e.not_a_directory,
        "files.operation.error.is_a_directory" => e.is_a_directory,
        "files.operation.error.destination_inside_source" => e.destination_inside_source,
        "files.operation.error.replaces_source" => e.replaces_source,
        "files.operation.error.symlink_loop" => e.symlink_loop,
        "files.operation.error.name_too_long" => e.name_too_long,
        "files.operation.error.invalid_name" => e.invalid_name,
        "files.operation.error.device_lost" => e.device_lost,
        "files.operation.error.cross_device" => e.cross_device,
        "files.operation.error.externally_modified" => e.externally_modified,
        "files.operation.error.verification_failed" => e.verification_failed,
        "files.operation.error.confirmation_required" => e.confirmation_required,
        "files.operation.error.cancelled" => e.cancelled,
        "files.operation.error.interrupted" => e.interrupted,
        "files.operation.error.conflict_unresolved" => e.conflict_unresolved,
        "files.operation.error.trash_unavailable" => e.trash_unavailable,
        "files.operation.error.archive_entry_refused" => e.archive_entry_refused,
        "files.operation.error.archive_limit_exceeded" => e.archive_limit_exceeded,
        "files.operation.error.archive_unreadable" => e.archive_unreadable,
        "files.operation.error.io" => e.io,
        // --- The window ----------------------------------------------
        "files.prefs.error.unwritable" => e.prefs_unwritable,
        "files.plugin.no_view" => e.plugin_no_view,
        _ => return text.to_string(),
    };
    sentence.replace("{detail}", detail)
}

/// The disk service's errors (`storage_platform::PlatformError`), which
/// reach the window as their English sentences.
fn platform(text: &str, e: &ErrorCopy) -> Option<String> {
    if text.starts_with("the storage service could not reach ") {
        return Some(e.platform_unreachable.to_string());
    }
    if let Some(device) = text.strip_prefix("no device is known at ") {
        return Some(
            e.platform_unknown_device
                .replace("{device}", &device_name(device)),
        );
    }
    let (operation, rest) = text
        .split_once(" was refused for ")
        .or_else(|| text.split_once(" is not supported for "))?;
    let (device, detail) = rest.split_once(": ").unwrap_or((rest, ""));
    let operation = match operation {
        "mount" => e.op_mount,
        "unmount" => e.op_unmount,
        "format" => e.op_format,
        "write image" => e.op_write_image,
        "filesystem access" => e.op_filesystem_access,
        _ => e.op_other,
    };
    Some(
        e.platform_refused
            .replace("{operation}", operation)
            .replace("{device}", &device_name(device))
            .replace("{detail}", &reason(detail, e)),
    )
}

/// Why the disk service refused, in the window's language where the reason
/// is one it gives in its own words or one UDisks names; anything else as
/// UDisks said it.
fn reason(detail: &str, e: &ErrorCopy) -> String {
    let lower = detail.to_ascii_lowercase();
    let known = match detail {
        "it belongs to the running system" => Some(e.why_system),
        "it is a partition, not a whole drive" => Some(e.why_partition),
        "the image is larger than the drive" => Some(e.why_image_too_large),
        _ if lower.contains("notauthorized") || lower.contains("not authorized") => {
            Some(e.why_not_authorized)
        }
        _ if lower.contains("error.cancelled") || lower.contains("dismissed") => {
            Some(e.why_cancelled)
        }
        _ if lower.contains("devicebusy") || lower.contains("target is busy") => Some(e.why_busy),
        _ => None,
    };
    known.map_or_else(|| detail.to_string(), str::to_string)
}

/// `/dev/sdc1` for UDisks' `/org/freedesktop/UDisks2/block_devices/sdc1`
/// (whose `_2d` is an escaped `-`); anything else as it is.
fn device_name(device: &str) -> String {
    let Some(name) = device.strip_prefix("/org/freedesktop/UDisks2/block_devices/") else {
        return device.to_string();
    };
    let mut out = String::from("/dev/");
    let mut rest = name;
    while let Some(at) = rest.find('_') {
        out.push_str(&rest[..at]);
        let escaped = rest.get(at + 1..at + 3);
        match escaped.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
            Some(byte) if byte.is_ascii() => {
                out.push(byte as char);
                rest = &rest[at + 3..];
            }
            _ => {
                out.push('_');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}
