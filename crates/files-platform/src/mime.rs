//! Type detection for listed entries.
//!
//! Detection sits behind a trait so the listing does not depend on where the
//! answer comes from. The real implementation is the shared MIME graph from
//! `app-chooser-core`, which already reads the freedesktop MIME database's
//! globs and subclass relations — there is no second glob table here, for the
//! same reason there is no second desktop-entry parser.
//!
//! The fallback exists because the shared database is not guaranteed to be
//! installed, and a list that shows no types at all on such a host is worse
//! than one that recognizes the common extensions. It is deliberately small
//! and says so, rather than growing into a private MIME database.

use std::sync::Arc;

use app_catalog_core::MimeType;
use app_chooser_core::MimeGraph;

/// Answers "what type is this entry".
///
/// Name-based only. Content sniffing would mean opening every file in a
/// directory, which is exactly the synchronous work Issue #6's performance
/// rules forbid during a listing.
pub trait MimeDetector: Send + Sync {
    fn detect(&self, file_name: &str) -> Option<MimeType>;
}

/// Detection through the shared freedesktop MIME database.
#[derive(Debug)]
pub struct SharedMimeDetector {
    graph: MimeGraph,
}

impl SharedMimeDetector {
    /// Reads the database from the session's XDG data directories.
    pub fn from_env() -> Self {
        Self {
            graph: MimeGraph::from_env(),
        }
    }

    pub fn with_graph(graph: MimeGraph) -> Self {
        Self { graph }
    }

    /// Whether the database actually had anything in it. A host with no
    /// `shared-mime-info` installed answers false, which is the signal to fall
    /// back rather than to report every file as untyped.
    pub fn is_populated(&self) -> bool {
        self.graph.guess_from_file_name("a.txt").is_some()
    }

    pub fn graph(&self) -> &MimeGraph {
        &self.graph
    }
}

impl MimeDetector for SharedMimeDetector {
    fn detect(&self, file_name: &str) -> Option<MimeType> {
        self.graph.guess_from_file_name(file_name)
    }
}

/// The extensions this build recognizes without the shared database.
///
/// Deliberately short. It covers what a file list is most often looking at and
/// nothing else; anything absent is reported as unknown rather than guessed.
const FALLBACK_GLOBS: &[(&str, &str)] = &[
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("json", "application/json"),
    ("xml", "application/xml"),
    ("csv", "text/csv"),
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("mp3", "audio/mpeg"),
    ("flac", "audio/flac"),
    ("ogg", "audio/ogg"),
    ("wav", "audio/x-wav"),
    ("mp4", "video/mp4"),
    ("mkv", "video/x-matroska"),
    ("webm", "video/webm"),
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("xz", "application/x-xz"),
    ("zst", "application/zstd"),
    ("tar", "application/x-tar"),
    ("deb", "application/vnd.debian.binary-package"),
    ("rs", "text/rust"),
    ("sh", "application/x-shellscript"),
    ("desktop", "application/x-desktop"),
];

/// Extension-only detection, used when the shared database is unavailable.
#[derive(Clone, Debug, Default)]
pub struct GlobMimeDetector;

impl MimeDetector for GlobMimeDetector {
    fn detect(&self, file_name: &str) -> Option<MimeType> {
        // A leading dot is not an extension: `.bashrc` has none.
        let extension = match file_name.rfind('.') {
            Some(0) | None => return None,
            Some(index) => file_name[index + 1..].to_ascii_lowercase(),
        };
        FALLBACK_GLOBS
            .iter()
            .find(|(suffix, _)| *suffix == extension)
            .and_then(|(_, mime)| MimeType::parse(mime))
    }
}

/// Picks the shared database when it is present and the small table when it is
/// not, so a caller does not have to decide.
pub fn detector_from_env() -> Arc<dyn MimeDetector> {
    let shared = SharedMimeDetector::from_env();
    if shared.is_populated() {
        Arc::new(shared)
    } else {
        Arc::new(GlobMimeDetector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_recognizes_common_extensions_case_insensitively() {
        let detector = GlobMimeDetector;
        assert_eq!(
            detector
                .detect("photo.JPG")
                .map(|mime| mime.as_str().to_string()),
            Some("image/jpeg".to_string())
        );
        assert_eq!(
            detector
                .detect("notes.txt")
                .map(|mime| mime.as_str().to_string()),
            Some("text/plain".to_string())
        );
    }

    #[test]
    fn an_unknown_extension_is_unknown_rather_than_guessed() {
        let detector = GlobMimeDetector;
        assert_eq!(detector.detect("firmware.q7z"), None);
        assert_eq!(detector.detect("Makefile"), None);
    }

    #[test]
    fn a_dotfile_has_no_extension_to_detect() {
        assert_eq!(GlobMimeDetector.detect(".bashrc"), None);
    }

    #[test]
    fn a_compound_extension_uses_the_last_component() {
        assert_eq!(
            GlobMimeDetector
                .detect("archive.tar.gz")
                .map(|mime| mime.as_str().to_string()),
            Some("application/gzip".to_string())
        );
    }

    #[test]
    fn the_shared_graph_is_used_when_the_database_has_the_type() {
        // Built from an explicit directory rather than the host's, so the test
        // does not depend on what is installed.
        let root = tempfile::tempdir().unwrap();
        let mime_dir = root.path().join("mime");
        std::fs::create_dir_all(&mime_dir).unwrap();
        std::fs::write(
            mime_dir.join("globs2"),
            "50:text/x-better-os-fixture:*.betteros\n",
        )
        .unwrap();
        let detector = SharedMimeDetector::with_graph(MimeGraph::from_data_dirs([root.path()]));
        assert_eq!(
            detector
                .detect("sample.betteros")
                .map(|mime| mime.as_str().to_string()),
            Some("text/x-better-os-fixture".to_string())
        );
    }
}
