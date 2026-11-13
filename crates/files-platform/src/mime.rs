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

/// The name a person reads for a file type, and the icon names to draw it
/// with, both from the freedesktop shared MIME database.
///
/// The name is the type's `<comment>` in `mime/<type>.xml`, in the person's
/// language when the database has it (`Plain text document`, `純文字文件`),
/// so the Type column never shows `text/plain`. The icon names follow the
/// shared-mime-info rule: the type with `/` replaced by `-`, then the
/// type's generic icon from `mime/generic-icons`, then `<media>-x-generic`.
///
/// Each answer is read once per type and kept, so a folder of ten thousand
/// photos reads one small file, not ten thousand.
#[derive(Debug)]
pub struct MimeDescriptions {
    data_dirs: Vec<std::path::PathBuf>,
    language: String,
    names: std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    generic: std::sync::OnceLock<std::collections::HashMap<String, String>>,
}

impl MimeDescriptions {
    /// `language` is a POSIX tag such as `zh_TW` or `en`; an empty one reads
    /// the untranslated comment.
    pub fn new(data_dirs: Vec<std::path::PathBuf>, language: impl Into<String>) -> Self {
        Self {
            data_dirs,
            language: language.into(),
            names: std::sync::Mutex::new(std::collections::HashMap::new()),
            generic: std::sync::OnceLock::new(),
        }
    }

    /// The session's data directories (`$XDG_DATA_HOME`, then
    /// `$XDG_DATA_DIRS`) and the given language.
    pub fn from_env(language: impl Into<String>) -> Self {
        Self::new(crate::xdg::data_dirs_from_env(), language)
    }

    /// The readable name of `mime`, or `None` when no database names it.
    pub fn describe(&self, mime: &str) -> Option<String> {
        if !is_plain_type(mime) {
            return None;
        }
        if let Ok(names) = self.names.lock()
            && let Some(known) = names.get(mime)
        {
            return known.clone();
        }
        let found = self.data_dirs.iter().find_map(|dir| {
            let text =
                std::fs::read_to_string(dir.join("mime").join(format!("{mime}.xml"))).ok()?;
            comment_for(&text, &self.language)
        });
        if let Ok(mut names) = self.names.lock() {
            names.insert(mime.to_string(), found.clone());
        }
        found
    }

    /// The themed icon names for `mime`, most specific first.
    pub fn icon_names(&self, mime: &str) -> Vec<String> {
        let mut names = Vec::new();
        if !is_plain_type(mime) {
            return names;
        }
        names.push(mime.replace('/', "-"));
        let generic = self.generic.get_or_init(|| {
            let mut map = std::collections::HashMap::new();
            // The first directory that defines a type wins, as with globs.
            for dir in self.data_dirs.iter().rev() {
                if let Ok(text) = std::fs::read_to_string(dir.join("mime/generic-icons")) {
                    map.extend(parse_generic_icons(&text));
                }
            }
            map
        });
        if let Some(icon) = generic.get(mime) {
            names.push(icon.clone());
        }
        if let Some((media, _)) = mime.split_once('/') {
            let fallback = format!("{media}-x-generic");
            if !names.contains(&fallback) {
                names.push(fallback);
            }
        }
        names
    }
}

/// `media/subtype` with no path characters, so a type name can never point a
/// lookup outside the `mime` directory.
fn is_plain_type(mime: &str) -> bool {
    let Some((media, subtype)) = mime.split_once('/') else {
        return false;
    };
    let ok = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-._".contains(c))
    };
    ok(media) && ok(subtype)
}

/// The `<comment>` for `language` in one `mime/<type>.xml` file: the exact
/// tag (`zh_TW`), then the language alone (`zh`), then the untranslated one.
pub fn comment_for(xml: &str, language: &str) -> Option<String> {
    let mut plain = None;
    let mut exact = None;
    let mut short = None;
    let base = language.split(['_', '.', '@']).next().unwrap_or("");
    let wanted = language.split(['.', '@']).next().unwrap_or("");
    let mut rest = xml;
    while let Some(start) = rest.find("<comment") {
        rest = &rest[start + "<comment".len()..];
        let Some(close) = rest.find('>') else { break };
        let attributes = &rest[..close];
        let body_start = close + 1;
        let Some(end) = rest[body_start..].find("</comment>") else {
            break;
        };
        let body = unescape(rest[body_start..body_start + end].trim());
        rest = &rest[body_start + end..];
        match attributes
            .split("xml:lang=\"")
            .nth(1)
            .and_then(|tail| tail.split('"').next())
        {
            None => {
                plain.get_or_insert(body);
            }
            Some(tag) if !wanted.is_empty() && tag == wanted => {
                exact.get_or_insert(body);
            }
            Some(tag) if !base.is_empty() && tag == base => {
                short.get_or_insert(body);
            }
            Some(_) => {}
        }
    }
    exact.or(short).or(plain).filter(|text| !text.is_empty())
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `mime/generic-icons`: one `type:icon` per line.
pub fn parse_generic_icons(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (mime, icon) = line.trim().split_once(':')?;
            (is_plain_type(mime) && !icon.is_empty() && !icon.contains('/'))
                .then(|| (mime.to_string(), icon.to_string()))
        })
        .collect()
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

    #[test]
    fn a_type_reads_as_its_comment_in_the_persons_language() {
        let xml = r#"<mime-type type="text/plain">
  <comment>Plain text document</comment>
  <comment xml:lang="zh_TW">純文字文件</comment>
  <comment xml:lang="zh_CN">纯文本文档</comment>
  <comment xml:lang="de">Einfaches Textdokument</comment>
</mime-type>"#;
        assert_eq!(comment_for(xml, "zh_TW").as_deref(), Some("純文字文件"));
        assert_eq!(
            comment_for(xml, "zh_TW.UTF-8").as_deref(),
            Some("純文字文件")
        );
        assert_eq!(
            comment_for(xml, "de_AT").as_deref(),
            Some("Einfaches Textdokument")
        );
        assert_eq!(
            comment_for(xml, "en").as_deref(),
            Some("Plain text document")
        );
        assert_eq!(comment_for(xml, "").as_deref(), Some("Plain text document"));
        assert_eq!(
            comment_for("<comment>A &amp; B</comment>", "en").as_deref(),
            Some("A & B")
        );
        assert_eq!(comment_for("<mime-type/>", "en"), None);
    }

    #[test]
    fn descriptions_and_icons_come_from_the_database_and_never_leave_it() {
        let root = tempfile::tempdir().unwrap();
        let mime = root.path().join("mime");
        std::fs::create_dir_all(mime.join("text")).unwrap();
        std::fs::write(
            mime.join("text/markdown.xml"),
            "<mime-type><comment>Markdown document</comment></mime-type>",
        )
        .unwrap();
        std::fs::write(mime.join("generic-icons"), "text/markdown:text-x-generic\n").unwrap();
        let names = MimeDescriptions::new(vec![root.path().to_path_buf()], "en");
        assert_eq!(
            names.describe("text/markdown").as_deref(),
            Some("Markdown document")
        );
        // Read once, then remembered: removing the file changes nothing.
        std::fs::remove_file(mime.join("text/markdown.xml")).unwrap();
        assert_eq!(
            names.describe("text/markdown").as_deref(),
            Some("Markdown document")
        );
        assert_eq!(names.describe("text/unknown"), None);
        assert_eq!(names.describe("../../etc/passwd"), None);
        assert_eq!(names.describe("text/../x"), None);
        assert_eq!(
            names.icon_names("text/markdown"),
            vec!["text-markdown".to_string(), "text-x-generic".to_string()]
        );
        assert_eq!(
            names.icon_names("image/png"),
            vec!["image-png".to_string(), "image-x-generic".to_string()]
        );
        assert!(names.icon_names("nonsense").is_empty());
    }
}
