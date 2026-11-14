//! No error key reaches the window as a key: every one the storage service
//! and the job engine report reads as a sentence, in both languages.

use std::path::PathBuf;

use files_gui::errors::explain;
use files_gui::i18n::{EN_US, ZH_TW};
use files_gui::session::Notice;
use files_operations::OperationError;

/// Every key the window can be handed.
fn keys() -> Vec<String> {
    let path = PathBuf::from("/media/stick/a.iso");
    let mut keys: Vec<String> = [
        "storage.erase.busy",
        "storage.erase.not_confirmed",
        "storage.image.unmount_failed:/dev/sdc2:unmount was refused for /dev/sdc2: target is busy",
        "storage.image.not_a_drive",
        "storage.image.not_a_file",
        "storage.format.label:the label is longer than the 11 bytes the filesystem stores",
        "storage.client.error.rejected:storage.erase.busy",
        "storage.client.error.service_unavailable:no reply",
        "storage.client.error.transport:broken pipe",
        "storage.client.error.protocol:bad document",
        "storage.client.error.protocol_version:3:2",
        "files.prefs.error.unwritable:permission denied",
        "files.plugin.no_view:tailnet",
        // The disk service's own sentences, as the client relays them.
        "storage.client.error.rejected:unmount was refused for /org/freedesktop/UDisks2/block_devices/sdc1: GDBus.Error:org.freedesktop.UDisks2.Error.DeviceBusy: Error unmounting /dev/sdc1: target is busy",
        "storage.client.error.rejected:mount was refused for /org/freedesktop/UDisks2/block_devices/sdc1: GDBus.Error:org.freedesktop.UDisks2.Error.NotAuthorizedDismissed: Not authorized to perform operation",
        "storage.client.error.rejected:format was refused for /org/freedesktop/UDisks2/block_devices/sda2: it belongs to the running system",
        "storage.client.error.rejected:write image was refused for /org/freedesktop/UDisks2/block_devices/sdc1: it is a partition, not a whole drive",
        "storage.client.error.rejected:write image was refused for /org/freedesktop/UDisks2/block_devices/sdc: the image is larger than the drive",
        "storage.client.error.rejected:write image was refused for /org/freedesktop/UDisks2/block_devices/sdc: GDBus.Error:org.freedesktop.UDisks2.Error.Cancelled: Operation was cancelled",
        "storage.client.error.rejected:filesystem access is not supported for /org/freedesktop/UDisks2/block_devices/dm_2d0: no filesystem",
        "storage.client.error.rejected:no device is known at /org/freedesktop/UDisks2/block_devices/sdc",
        "storage.client.error.rejected:the storage service could not reach org.freedesktop.UDisks2: timed out",
    ]
    .iter()
    .map(|key| key.to_string())
    .collect();
    let errors = vec![
        OperationError::NoSpace { path: path.clone() },
        OperationError::QuotaExceeded { path: path.clone() },
        OperationError::PermissionDenied { path: path.clone() },
        OperationError::ReadOnly { path: path.clone() },
        OperationError::NotFound { path: path.clone() },
        OperationError::AlreadyExists { path: path.clone() },
        OperationError::NotADirectory { path: path.clone() },
        OperationError::IsADirectory { path: path.clone() },
        OperationError::DestinationInsideSource { path: path.clone() },
        OperationError::ReplacesSource { path: path.clone() },
        OperationError::SymlinkLoop { path: path.clone() },
        OperationError::NameTooLong { path: path.clone() },
        OperationError::InvalidName { name: path.clone() },
        OperationError::DeviceLost { path: path.clone() },
        OperationError::CrossDevice { path: path.clone() },
        OperationError::ExternallyModified { path: path.clone() },
        OperationError::ConfirmationRequired,
        OperationError::Cancelled { path: path.clone() },
        OperationError::Interrupted,
        OperationError::ConflictUnresolved { path: path.clone() },
        OperationError::TrashUnavailable {
            reason: "no home".into(),
        },
        OperationError::VerificationFailed {
            path: path.clone(),
            reason: "checksum".into(),
        },
        OperationError::ArchiveEntryRefused {
            path: path.clone(),
            entry: PathBuf::from("../escape"),
            reason: "outside".into(),
        },
        OperationError::ArchiveUnreadable {
            path: path.clone(),
            reason: "damaged".into(),
        },
        OperationError::Io {
            path: path.clone(),
            reason: "other".into(),
            errno: Some(5),
        },
    ];
    keys.extend(errors.iter().map(|error| error.to_string()));
    keys
}

#[test]
fn every_key_reads_as_a_sentence_in_both_languages() {
    for key in keys() {
        for c in [&EN_US, &ZH_TW] {
            let said = explain(&key, c);
            assert_ne!(said, key, "{key} has no sentence");
            for raw in [
                "storage.",
                "files.operation.",
                "files.prefs.",
                "files.plugin.",
            ] {
                assert!(!said.contains(raw), "{key} shows a key: {said}");
            }
            let message = Notice::Key(key.clone()).message(c);
            assert!(
                !message.contains("storage.") && !message.contains("files."),
                "{message}"
            );
        }
    }
    // The device and the path are kept where the sentence names them.
    assert!(explain("storage.image.unmount_failed:/dev/sdc2:busy", &EN_US).contains("/dev/sdc2"));
    assert!(
        explain("files.operation.error.no_space:/media/stick", &ZH_TW).contains("/media/stick")
    );
    // The disk service's sentences are translated, its device paths shown as
    // the device nodes people know, and UDisks' own reasons put plainly.
    let busy = "storage.client.error.rejected:unmount was refused for /org/freedesktop/UDisks2/block_devices/sdc1: GDBus.Error:org.freedesktop.UDisks2.Error.DeviceBusy: target is busy";
    assert_eq!(
        explain(busy, &ZH_TW),
        "無法卸載 /dev/sdc1：有程式正在使用它。請關閉使用它的程式後再試一次。"
    );
    assert_eq!(
        explain(busy, &EN_US),
        "Couldn't unmount /dev/sdc1: something is using it. Close what is using it and try again."
    );
    assert!(
        explain(
            "filesystem access is not supported for /org/freedesktop/UDisks2/block_devices/dm_2d0: no filesystem",
            &EN_US
        )
        .contains("/dev/dm-0")
    );
    // Text that is no key is left as it is.
    assert_eq!(explain("Sent to Phone.", &EN_US), "Sent to Phone.");
}
