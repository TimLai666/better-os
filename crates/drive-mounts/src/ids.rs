//! The one value that crosses the privileged boundary, a filesystem UUID, and
//! the names derived from it (ADR 0032).

use std::path::PathBuf;

/// Where Better OS mounts the internal drives it was asked to.
pub const MOUNT_BASE: &str = storage_platform::system_use::MOUNT_BASE;

/// A filesystem UUID as `blkid` and UDisks2 report it: hex digits and dashes
/// (an ext4 `8a1c…-…`, a FAT `ABCD-1234`, an NTFS `01D9…`, an ISO 9660
/// `2024-04-23-10-58-15-00`), 1 to 64 characters, not starting with a dash.
/// Anything else is refused before it is used for a path, a line or a rule.
pub fn valid_uuid(uuid: &str) -> bool {
    !uuid.is_empty()
        && uuid.len() <= 64
        && !uuid.starts_with('-')
        && uuid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// `/mnt/better-os/<uuid>`. Only for a UUID [`valid_uuid`] accepts.
pub fn mount_point(uuid: &str) -> PathBuf {
    PathBuf::from(MOUNT_BASE).join(uuid)
}

/// The mount unit systemd's fstab generator makes for `path`, escaped as
/// `systemd-escape --path --suffix=mount` does.
pub fn mount_unit(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    let trimmed = text.trim_matches('/');
    if trimmed.is_empty() {
        return "-.mount".to_string();
    }
    let mut out = String::new();
    for (index, byte) in trimmed.bytes().enumerate() {
        match byte {
            b'/' => out.push('-'),
            b'.' if index == 0 => out.push_str("\\x2e"),
            b if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b':' => {
                out.push(b as char)
            }
            b => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out.push_str(".mount");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hex_and_dashes_cross() {
        for good in [
            "8a1c2f3e-1111-4222-8333-944455556666",
            "ABCD-1234",
            "01D9F00DBEEF1234",
            "2024-04-23-10-58-15-00",
        ] {
            assert!(valid_uuid(good), "{good}");
        }
        for bad in [
            "",
            "-ABCD",
            "../etc",
            "ABCD 1234",
            "ABCD\n1234",
            "abcd/1234",
            "UUID=ABCD",
            "g000",
            &"a".repeat(65),
        ] {
            assert!(!valid_uuid(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_unit_name_is_systemd_escape_s() {
        assert_eq!(
            mount_unit(&mount_point("ABCD-1234")),
            "mnt-better\\x2dos-ABCD\\x2d1234.mount"
        );
        assert_eq!(mount_unit(std::path::Path::new("/")), "-.mount");
        assert_eq!(
            mount_unit(std::path::Path::new("/media/a b")),
            "media-a\\x20b.mount"
        );
    }
}
