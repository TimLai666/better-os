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
        "files.prefs.error.unreadable:expected value at line 1",
        "files.bookmarks.error.unwritable:permission denied",
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
    // A plugin command that did not answer (ticket 172: these reached the
    // window as their debug shape).
    use files_gui::plugins::RunError;
    keys.extend(
        [
            RunError::NotTrusted(PathBuf::from("/home/a/.local/share/folio/plugins/x/run")),
            RunError::NotFound("tailscale".into()),
            RunError::NotFound(String::new()),
            RunError::Failed {
                code: Some(1),
                last_error: "no network".into(),
            },
            RunError::Failed {
                code: Some(1),
                last_error: String::new(),
            },
            RunError::TimedOut,
            RunError::TooLarge,
        ]
        .iter()
        .map(RunError::key),
    );
    keys
}

#[test]
fn a_key_with_no_sentence_says_so_and_names_it() {
    assert_eq!(
        explain("files.something.new:detail", &EN_US),
        "Unknown reason (files.something.new:detail)"
    );
    assert_eq!(
        explain("storage.something_new", &ZH_TW),
        "原因不明（storage.something_new）"
    );
    // Text that is not a key is still left as it is.
    assert_eq!(explain("Sent to Phone.", &ZH_TW), "Sent to Phone.");
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

/// Names of enum values, not messages: the window labels each from its
/// typed value (`i18n`'s job-state, operation, conflict, confidence, format,
/// scope, view and scale labels), and the string is only what a job log or a
/// preferences file stores.
const LABEL_FAMILIES: &[&str] = &[
    "files.job.state.",
    "files.operation.kind.",
    "files.conflict.kind.",
    "files.job.confidence.",
    "files.archive.format.",
    "files.search.scope.",
    "files.view.",
    "files.scale.",
];

/// The production lines of a source file, before its `#[cfg(test)] mod`.
fn production(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]"
            && lines
                .get(index + 1)
                .is_some_and(|next| next.trim_start().starts_with("mod "))
        {
            break;
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn keys_in(directory: &std::path::Path, into: &mut std::collections::BTreeSet<String>) {
    for entry in std::fs::read_dir(directory).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            keys_in(&path, into);
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // The table itself names every key; it is the answer, not the
        // question.
        if path.extension().is_none_or(|e| e != "rs")
            || stem == "tests"
            || stem.ends_with("_tests")
            || path.ends_with("files-gui/src/errors.rs")
        {
            continue;
        }
        let text = production(&std::fs::read_to_string(&path).unwrap());
        for prefix in ["\"files.", "\"storage."] {
            for (at, _) in text.match_indices(prefix) {
                let rest = &text[at + 1..];
                let end = rest
                    .find(|c: char| {
                        !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
                    })
                    .unwrap_or(rest.len());
                let key = rest[..end].trim_end_matches('.');
                if key.matches('.').count() >= 2 {
                    into.insert(key.to_string());
                }
            }
        }
    }
}

/// Every message key the crates below the window and the window itself can
/// emit (ticket 172).
fn emitted() -> std::collections::BTreeSet<String> {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut keys = std::collections::BTreeSet::new();
    for member in [
        "files-core",
        "files-platform",
        "files-operations",
        "files-preview",
        "files-search",
        "storage-core",
        "storage-service",
        "files-gui",
    ] {
        keys_in(&crates.join(member).join("src"), &mut keys);
    }
    keys.retain(|key| !LABEL_FAMILIES.iter().any(|family| key.starts_with(family)));
    keys
}

/// Whether text shows a key (`files.trash.error.io`), as opposed to a
/// sentence that ends on the word ("a list of files.").
fn shows_key(text: &str) -> bool {
    ["files.", "storage."].iter().any(|family| {
        text.match_indices(family)
            .any(|(at, _)| text[at + family.len()..].starts_with(|c: char| c.is_ascii_lowercase()))
    })
}

#[test]
fn every_message_key_the_files_and_storage_crates_emit_has_a_sentence_in_both_languages() {
    let keys = emitted();
    for required in [
        "files.trash.error.no_home",
        "files.archive.entry.parent_traversal",
        "files.listing.error.permission_denied",
        "storage.unknown.signal_unsupported",
        "files.preview.degraded.too_large",
    ] {
        assert!(keys.contains(required), "the scan missed {required}");
    }
    for key in &keys {
        for c in [&EN_US, &ZH_TW] {
            for text in [key.clone(), format!("{key}:/media/stick/a.iso")] {
                let said = explain(&text, c);
                assert!(
                    !said.contains("Unknown reason") && !said.contains("原因不明"),
                    "{text} has no sentence: {said}"
                );
                assert!(!shows_key(&said), "{text} shows a key: {said}");
                assert!(
                    !said.contains("()") && !said.contains("（）"),
                    "{text}: {said}"
                );
            }
        }
    }
    // The reverse: a sentence nothing emits is a key that went away.
    for (key, _, _) in files_gui::errors::SENTENCES {
        if !key.contains(':') {
            assert!(
                keys.contains(*key),
                "{key} has a sentence but nothing emits it"
            );
        }
    }
}

#[test]
fn a_key_inside_a_sentences_detail_is_said_as_its_own_sentence() {
    let error = OperationError::TrashUnavailable {
        reason: "files.trash.error.no_home".into(),
    };
    assert_eq!(
        explain(&error.to_string(), &EN_US),
        format!(
            "{} There is no home folder to keep the trash in.",
            EN_US.errors.trash_unavailable.replace(" ({detail})", "")
        )
    );
    let refused = OperationError::ArchiveEntryRefused {
        path: PathBuf::from("/home/a/evil.tar"),
        entry: PathBuf::from("../etc/passwd"),
        reason: "files.archive.entry.parent_traversal".into(),
    };
    let said = explain(&refused.to_string(), &ZH_TW);
    assert!(said.contains("/home/a/evil.tar"), "{said}");
    assert!(
        said.ends_with("這個項目的名稱用「..」跳出了資料夾。"),
        "{said}"
    );
    assert!(!said.contains("files."), "{said}");
    let limit = OperationError::ArchiveLimitExceeded {
        path: PathBuf::from("/home/a/big.zip"),
        limit: files_operations::ArchiveLimit::Entries,
        maximum: 100_000,
    };
    assert_eq!(
        explain(&limit.to_string(), &EN_US),
        "The archive holds more than 100000 items, which is more than Better Files unpacks (/home/a/big.zip)."
    );
}
