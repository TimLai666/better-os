//! A local path as a `file://` URI.
//!
//! The thumbnail cache hashes this exact spelling, and the
//! `org.freedesktop.FileManager1` calls send it, so both use this one.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// `file://` and the path, with every byte outside the unreserved set and
/// `/` escaped, as the thumbnail standard's examples are.
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(byte) {
            uri.push(*byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::file_uri;
    use std::path::Path;

    #[test]
    fn reserved_bytes_and_non_ascii_names_are_escaped() {
        assert_eq!(
            file_uri(Path::new("/home/a/My Photo#1.png")),
            "file:///home/a/My%20Photo%231.png"
        );
        assert_eq!(
            file_uri(Path::new("/a/相片.jpg")),
            "file:///a/%E7%9B%B8%E7%89%87.jpg"
        );
    }
}
