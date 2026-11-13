//! What the large preview window (Space, its own process) shows for a file,
//! decided without a window: folio's `PreviewWindow.qml` and the owner's
//! list (`plans/folio.md:81,97-100`).
//!
//! - Images (PNG, JPEG, GIF with its animation, BMP, WebP, SVG through the
//!   window's image loader).
//! - Markdown, rendered.
//! - Code and other text, highlighted by a small lexical highlighter here
//!   (keywords, strings, comments, numbers) for the common languages. The
//!   workspace has no tree-sitter grammars, and adding forty of them for a
//!   preview is not worth the build.
//! - PDF pages through poppler's `pdftoppm`, and search through `pdftotext`,
//!   when they are installed.
//! - Office documents converted to PDF by LibreOffice
//!   (`soffice --headless --convert-to pdf`) when it is installed.
//! - Archives (zip, tar, tar.gz, tar.zst): the list of what is inside, read
//!   without extracting anything.
//! - Audio and video: the workspace has no decoders, so the window says so
//!   and shows the poster frame the thumbnail cache has, if any.
//! - Folders: what is in them.
//!
//! Every program is started with its arguments as a list (never a shell)
//! and stopped after a timeout.

use std::fs;
use std::io::{BufReader, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What kind of view a file gets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewKind {
    Image,
    Markdown,
    /// Source code, with the language its highlighting follows.
    Code(Language),
    Text,
    Pdf,
    /// Word processor, spreadsheet or slides: shown as LibreOffice's PDF.
    Office,
    Archive(ArchiveKind),
    Audio,
    Video,
    Folder,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveKind {
    Zip,
    Tar,
    TarGz,
    TarZst,
}

/// What a file is shown as, from its type (when known) and its name.
pub fn view_kind(path: &Path, mime: Option<&str>) -> ViewKind {
    if path.is_dir() {
        return ViewKind::Folder;
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let extension = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return ViewKind::Archive(ArchiveKind::TarGz);
    }
    if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        return ViewKind::Archive(ArchiveKind::TarZst);
    }
    match extension {
        "zip" | "jar" | "epub" => return ViewKind::Archive(ArchiveKind::Zip),
        "tar" => return ViewKind::Archive(ArchiveKind::Tar),
        "md" | "markdown" => return ViewKind::Markdown,
        "pdf" => return ViewKind::Pdf,
        "doc" | "docx" | "odt" | "rtf" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" | "odp" => {
            return ViewKind::Office;
        }
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" => return ViewKind::Image,
        _ => {}
    }
    if let Some(language) = Language::for_extension(extension, &name) {
        return ViewKind::Code(language);
    }
    match mime.unwrap_or("") {
        mime if mime.starts_with("image/") => ViewKind::Image,
        mime if mime.starts_with("audio/") => ViewKind::Audio,
        mime if mime.starts_with("video/") => ViewKind::Video,
        "application/pdf" => ViewKind::Pdf,
        "text/markdown" => ViewKind::Markdown,
        mime if mime.starts_with("text/") => ViewKind::Text,
        _ => match extension {
            "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" => ViewKind::Audio,
            "mp4" | "mkv" | "webm" | "mov" | "avi" => ViewKind::Video,
            "txt" | "log" | "csv" | "ini" | "conf" => ViewKind::Text,
            _ => ViewKind::Other,
        },
    }
}

// --- Highlighting ---------------------------------------------------------

/// The languages the highlighter knows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    Rust,
    C,
    Cpp,
    Go,
    JavaScript,
    TypeScript,
    Python,
    Shell,
    Json,
    Toml,
    Yaml,
    Java,
    Kotlin,
    Css,
    Html,
    Sql,
    Lua,
    Ruby,
}

impl Language {
    pub fn for_extension(extension: &str, name: &str) -> Option<Language> {
        Some(match extension {
            "rs" => Language::Rust,
            "c" | "h" => Language::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hh" => Language::Cpp,
            "go" => Language::Go,
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            "py" | "pyi" => Language::Python,
            "sh" | "bash" | "zsh" => Language::Shell,
            "json" => Language::Json,
            "toml" => Language::Toml,
            "yaml" | "yml" => Language::Yaml,
            "java" => Language::Java,
            "kt" | "kts" => Language::Kotlin,
            "css" | "scss" => Language::Css,
            "html" | "htm" | "xml" | "svg" => Language::Html,
            "sql" => Language::Sql,
            "lua" => Language::Lua,
            "rb" => Language::Ruby,
            _ => match name {
                "makefile" | "dockerfile" | ".bashrc" | ".profile" => Language::Shell,
                _ => return None,
            },
        })
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Language::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            Language::C | Language::Cpp => &[
                "auto",
                "break",
                "case",
                "char",
                "class",
                "const",
                "continue",
                "default",
                "do",
                "double",
                "else",
                "enum",
                "extern",
                "float",
                "for",
                "if",
                "include",
                "inline",
                "int",
                "long",
                "namespace",
                "nullptr",
                "private",
                "public",
                "return",
                "short",
                "sizeof",
                "static",
                "struct",
                "switch",
                "template",
                "typedef",
                "union",
                "unsigned",
                "void",
                "while",
                "define",
            ],
            Language::Go => &[
                "break",
                "case",
                "chan",
                "const",
                "continue",
                "default",
                "defer",
                "else",
                "fallthrough",
                "for",
                "func",
                "go",
                "if",
                "import",
                "interface",
                "map",
                "package",
                "range",
                "return",
                "select",
                "struct",
                "switch",
                "type",
                "var",
                "nil",
                "true",
                "false",
            ],
            Language::JavaScript | Language::TypeScript => &[
                "async",
                "await",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "default",
                "delete",
                "do",
                "else",
                "export",
                "extends",
                "false",
                "finally",
                "for",
                "function",
                "if",
                "import",
                "in",
                "instanceof",
                "interface",
                "let",
                "new",
                "null",
                "return",
                "switch",
                "this",
                "throw",
                "true",
                "try",
                "type",
                "typeof",
                "undefined",
                "var",
                "void",
                "while",
                "yield",
                "from",
            ],
            Language::Python => &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "None", "not", "or", "pass", "raise", "return",
                "True", "try", "while", "with", "yield",
            ],
            Language::Shell => &[
                "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case",
                "esac", "function", "return", "local", "export", "set", "unset", "echo",
            ],
            Language::Json => &["true", "false", "null"],
            Language::Toml | Language::Yaml => &["true", "false", "null", "yes", "no"],
            Language::Java | Language::Kotlin => &[
                "abstract",
                "boolean",
                "break",
                "case",
                "catch",
                "class",
                "else",
                "extends",
                "final",
                "for",
                "fun",
                "if",
                "implements",
                "import",
                "int",
                "interface",
                "new",
                "null",
                "object",
                "override",
                "package",
                "private",
                "protected",
                "public",
                "return",
                "static",
                "this",
                "throw",
                "try",
                "val",
                "var",
                "void",
                "when",
                "while",
                "true",
                "false",
            ],
            Language::Css => &["important", "media", "import"],
            Language::Html => &[],
            Language::Sql => &[
                "select", "from", "where", "insert", "into", "values", "update", "set", "delete",
                "create", "table", "join", "left", "right", "on", "and", "or", "not", "null",
                "order", "by", "group", "limit", "as", "SELECT", "FROM", "WHERE", "INSERT", "INTO",
                "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "JOIN", "ON", "AND", "OR",
                "NOT", "NULL", "ORDER", "BY", "GROUP", "LIMIT", "AS",
            ],
            Language::Lua => &[
                "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if",
                "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until",
                "while",
            ],
            Language::Ruby => &[
                "begin", "class", "def", "do", "else", "elsif", "end", "ensure", "false", "if",
                "module", "nil", "require", "rescue", "return", "self", "true", "unless", "until",
                "when", "while", "yield",
            ],
        }
    }

    fn line_comment(self) -> Option<&'static str> {
        match self {
            Language::Python
            | Language::Shell
            | Language::Toml
            | Language::Yaml
            | Language::Ruby => Some("#"),
            Language::Sql | Language::Lua => Some("--"),
            Language::Json | Language::Html | Language::Css => None,
            _ => Some("//"),
        }
    }

    fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self {
            Language::Html => Some(("<!--", "-->")),
            Language::Python
            | Language::Shell
            | Language::Toml
            | Language::Yaml
            | Language::Ruby
            | Language::Json
            | Language::Lua
            | Language::Sql => None,
            _ => Some(("/*", "*/")),
        }
    }
}

/// What a highlighted stretch of text is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Token {
    Keyword,
    String,
    Comment,
    Number,
}

/// The highlighted stretches of `text`, as byte ranges in order.
pub fn highlight(language: Language, text: &str) -> Vec<(Range<usize>, Token)> {
    let bytes = text.as_bytes();
    let keywords = language.keywords();
    let line = language.line_comment();
    let block = language.block_comment();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let rest = &text[at..];
        if let Some(marker) = line
            && rest.starts_with(marker)
        {
            let end = rest.find('\n').map_or(bytes.len(), |n| at + n);
            out.push((at..end, Token::Comment));
            at = end;
            continue;
        }
        if let Some((open, close)) = block
            && rest.starts_with(open)
        {
            let end = rest[open.len()..]
                .find(close)
                .map_or(bytes.len(), |n| at + open.len() + n + close.len());
            out.push((at..end, Token::Comment));
            at = end;
            continue;
        }
        let byte = bytes[at];
        if byte == b'"' || byte == b'\'' || (byte == b'`' && language == Language::JavaScript) {
            // A Rust lifetime is not a string.
            if byte == b'\'' && language == Language::Rust {
                let close = rest[1..].find('\'').map(|n| n + 1);
                if close.is_none_or(|n| n > 3 && !rest[1..n].starts_with('\\')) {
                    at += 1;
                    continue;
                }
            }
            let mut end = at + 1;
            while end < bytes.len() && bytes[end] != byte && bytes[end] != b'\n' {
                if bytes[end] == b'\\' {
                    end += 1;
                }
                end += 1;
            }
            let end = (end + 1).min(bytes.len());
            out.push((at..end, Token::String));
            at = end;
            continue;
        }
        if byte.is_ascii_digit() && (at == 0 || !is_word(bytes[at - 1])) {
            let mut end = at;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'.' || bytes[end] == b'_')
            {
                end += 1;
            }
            out.push((at..end, Token::Number));
            at = end;
            continue;
        }
        if is_word(byte) {
            let mut end = at;
            while end < bytes.len() && is_word(bytes[end]) {
                end += 1;
            }
            if keywords.contains(&&text[at..end]) {
                out.push((at..end, Token::Keyword));
            }
            at = end;
            continue;
        }
        // Step over one whole character.
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

// --- Archives ------------------------------------------------------------

/// One entry of an archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    pub is_folder: bool,
}

/// At most this many entries are listed.
pub const ARCHIVE_LIMIT: usize = 5_000;

/// What is inside an archive, read without extracting anything. Returns the
/// entries (at most [`ARCHIVE_LIMIT`]) and whether there were more.
pub fn archive_entries(
    path: &Path,
    kind: ArchiveKind,
) -> std::io::Result<(Vec<ArchiveEntry>, bool)> {
    let file = fs::File::open(path)?;
    let mut entries = Vec::new();
    let mut more = false;
    match kind {
        ArchiveKind::Zip => {
            let mut archive =
                zip::ZipArchive::new(BufReader::new(file)).map_err(std::io::Error::other)?;
            for index in 0..archive.len() {
                if entries.len() >= ARCHIVE_LIMIT {
                    more = true;
                    break;
                }
                let item = archive.by_index_raw(index).map_err(std::io::Error::other)?;
                entries.push(ArchiveEntry {
                    name: item.name().to_string(),
                    size: item.size(),
                    is_folder: item.is_dir(),
                });
            }
        }
        ArchiveKind::Tar => more = read_tar(BufReader::new(file), &mut entries)?,
        ArchiveKind::TarGz => {
            more = read_tar(
                flate2::read::GzDecoder::new(BufReader::new(file)),
                &mut entries,
            )?
        }
        ArchiveKind::TarZst => {
            let decoder = ruzstd::decoding::StreamingDecoder::new(BufReader::new(file))
                .map_err(std::io::Error::other)?;
            more = read_tar(decoder, &mut entries)?;
        }
    }
    Ok((entries, more))
}

fn read_tar(reader: impl Read, entries: &mut Vec<ArchiveEntry>) -> std::io::Result<bool> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        if entries.len() >= ARCHIVE_LIMIT {
            return Ok(true);
        }
        let entry = entry?;
        let header = entry.header();
        entries.push(ArchiveEntry {
            name: entry.path()?.to_string_lossy().into_owned(),
            size: header.size().unwrap_or(0),
            is_folder: header.entry_type().is_dir(),
        });
    }
    Ok(false)
}

// --- Programs: poppler and LibreOffice ------------------------------------

/// Runs `program` with `arguments` (never through a shell) and returns its
/// standard output, or `None` when it is missing, fails, or outlives
/// `limit`.
///
/// The program runs in a process group of its own, and a timeout ends the
/// whole group: LibreOffice's `soffice` starts `soffice.bin` and leaves it
/// running, and killing only the first would leave the second converting
/// (and holding the pipe) after the window gave up on it.
pub fn run(program: &str, arguments: &[String], limit: Duration) -> Option<Vec<u8>> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        buffer
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = reader.join().ok()?;
                return status.success().then_some(output);
            }
            Ok(None) if started.elapsed() > limit => {
                end_group(&mut child);
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
}

/// Kills a child started with `process_group(0)` and everything it
/// started, then reaps it.
pub(crate) fn end_group(child: &mut std::process::Child) {
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: `kill` takes plain integers. The group is the child's own
        // (it was started as a group leader and has not been reaped), so no
        // unrelated process can be named by it.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The number of pages `pdfinfo` reports.
pub fn pdf_page_count(pdf: &Path) -> Option<u32> {
    let output = run(
        "pdfinfo",
        &[pdf.display().to_string()],
        Duration::from_secs(10),
    )?;
    parse_page_count(&String::from_utf8_lossy(&output))
}

pub fn parse_page_count(info: &str) -> Option<u32> {
    info.lines()
        .find_map(|line| line.strip_prefix("Pages:"))
        .and_then(|count| count.trim().parse().ok())
}

/// Renders one page (1-based) to a PNG `width` pixels wide in `folder`, and
/// returns its path.
pub fn render_pdf_page(pdf: &Path, page: u32, width: u32, folder: &Path) -> Option<PathBuf> {
    let stem = folder.join(format!("page-{page}"));
    run(
        "pdftoppm",
        &[
            "-png".into(),
            "-singlefile".into(),
            "-f".into(),
            page.to_string(),
            "-l".into(),
            page.to_string(),
            "-scale-to-x".into(),
            width.to_string(),
            "-scale-to-y".into(),
            "-1".into(),
            pdf.display().to_string(),
            stem.display().to_string(),
        ],
        Duration::from_secs(20),
    )?;
    let png = stem.with_extension("png");
    png.is_file().then_some(png)
}

/// The text of every page, from `pdftotext`, which separates pages with a
/// form feed.
pub fn pdf_text(pdf: &Path) -> Option<Vec<String>> {
    let output = run(
        "pdftotext",
        &["-layout".into(), pdf.display().to_string(), "-".into()],
        Duration::from_secs(30),
    )?;
    Some(split_pages(&String::from_utf8_lossy(&output)))
}

pub fn split_pages(text: &str) -> Vec<String> {
    let mut pages: Vec<String> = text.split('\u{c}').map(str::to_string).collect();
    if pages.last().is_some_and(|page| page.trim().is_empty()) {
        pages.pop();
    }
    pages
}

/// The pages (1-based) whose text contains `query`, ignoring case.
pub fn search_pages(pages: &[String], query: &str) -> Vec<u32> {
    let query = query.to_lowercase();
    if query.trim().is_empty() {
        return Vec::new();
    }
    pages
        .iter()
        .enumerate()
        .filter(|(_, text)| text.to_lowercase().contains(&query))
        .map(|(index, _)| index as u32 + 1)
        .collect()
}

/// Converts an office document to PDF in `folder` with LibreOffice, and
/// returns the PDF. `None` when LibreOffice is not installed or fails.
pub fn office_to_pdf(document: &Path, folder: &Path) -> Option<PathBuf> {
    // A profile of its own, so a running LibreOffice is not disturbed.
    let profile = folder.join("lo-profile");
    run(
        "soffice",
        &[
            format!("-env:UserInstallation=file://{}", profile.display()),
            "--headless".into(),
            "--convert-to".into(),
            "pdf".into(),
            "--outdir".into(),
            folder.display().to_string(),
            document.display().to_string(),
        ],
        Duration::from_secs(90),
    )?;
    let pdf = folder.join(document.file_stem()?).with_extension("pdf");
    pdf.is_file().then_some(pdf)
}

/// The files beside `file` the window steps through with the arrows: its
/// folder's entries that are not hidden, by name, and the file's position.
pub fn siblings(file: &Path) -> (Vec<PathBuf>, usize) {
    let Some(folder) = file.parent() else {
        return (vec![file.to_path_buf()], 0);
    };
    let mut all: Vec<PathBuf> = fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
                })
                .collect()
        })
        .unwrap_or_default();
    all.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    let at = all.iter().position(|path| path == file).unwrap_or_else(|| {
        all.insert(0, file.to_path_buf());
        0
    });
    (all, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_file_gets_its_view() {
        let p = Path::new;
        assert_eq!(view_kind(p("/x/a.md"), None), ViewKind::Markdown);
        assert_eq!(
            view_kind(p("/x/a.rs"), Some("text/rust")),
            ViewKind::Code(Language::Rust)
        );
        assert_eq!(view_kind(p("/x/a.PDF"), None), ViewKind::Pdf);
        assert_eq!(view_kind(p("/x/a.docx"), None), ViewKind::Office);
        assert_eq!(
            view_kind(p("/x/a.tar.gz"), None),
            ViewKind::Archive(ArchiveKind::TarGz)
        );
        assert_eq!(
            view_kind(p("/x/a.zip"), None),
            ViewKind::Archive(ArchiveKind::Zip)
        );
        assert_eq!(view_kind(p("/x/a.gif"), None), ViewKind::Image);
        assert_eq!(view_kind(p("/x/song"), Some("audio/flac")), ViewKind::Audio);
        assert_eq!(view_kind(p("/x/a.mkv"), None), ViewKind::Video);
        assert_eq!(
            view_kind(p("/x/notes.txt"), Some("text/plain")),
            ViewKind::Text
        );
        assert_eq!(view_kind(p("/x/blob"), None), ViewKind::Other);
        assert_eq!(
            view_kind(p("/x/Makefile"), None),
            ViewKind::Code(Language::Shell)
        );
    }

    fn tokens(language: Language, text: &str) -> Vec<(&str, Token)> {
        highlight(language, text)
            .into_iter()
            .map(|(range, token)| (&text[range], token))
            .collect()
    }

    #[test]
    fn rust_keywords_strings_comments_and_numbers() {
        let got = tokens(
            Language::Rust,
            "fn main() { let s = \"hi \\\" there\"; // done\n let n = 42; }",
        );
        assert_eq!(
            got,
            [
                ("fn", Token::Keyword),
                ("let", Token::Keyword),
                ("\"hi \\\" there\"", Token::String),
                ("// done", Token::Comment),
                ("let", Token::Keyword),
                ("42", Token::Number),
            ]
        );
        // A lifetime is not a string, and a word containing a keyword is not
        // a keyword.
        assert_eq!(
            tokens(Language::Rust, "fn f<'a>(letter: &'a str)"),
            [("fn", Token::Keyword)]
        );
    }

    #[test]
    fn python_and_shell_use_hash_comments_and_c_uses_blocks() {
        assert_eq!(
            tokens(Language::Python, "def f(): # note\n  return 'x'"),
            [
                ("def", Token::Keyword),
                ("# note", Token::Comment),
                ("return", Token::Keyword),
                ("'x'", Token::String),
            ]
        );
        assert_eq!(
            tokens(Language::C, "/* a\n b */ int x = 0x1F;"),
            [
                ("/* a\n b */", Token::Comment),
                ("int", Token::Keyword),
                ("0x1F", Token::Number)
            ]
        );
        // Unterminated things end at the text's end without panicking.
        assert_eq!(
            tokens(Language::C, "/* open"),
            [("/* open", Token::Comment)]
        );
        assert_eq!(
            tokens(Language::Rust, "\"open"),
            [("\"open", Token::String)]
        );
        assert!(!tokens(Language::Rust, "文字 fn").is_empty());
    }

    #[test]
    fn an_archive_lists_what_is_inside_without_extracting() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.zip");
        {
            let file = fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.add_directory("docs/", options).unwrap();
            zip.start_file("docs/readme.txt", options).unwrap();
            std::io::Write::write_all(&mut zip, b"hello").unwrap();
            zip.finish().unwrap();
        }
        let (entries, more) = archive_entries(&path, ArchiveKind::Zip).unwrap();
        assert!(!more);
        assert_eq!(
            entries,
            [
                ArchiveEntry {
                    name: "docs/".into(),
                    size: 0,
                    is_folder: true
                },
                ArchiveEntry {
                    name: "docs/readme.txt".into(),
                    size: 5,
                    is_folder: false
                },
            ]
        );
        // Nothing was written beside it.
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

        let tar_path = root.path().join("b.tar");
        {
            let mut builder = tar::Builder::new(fs::File::create(&tar_path).unwrap());
            let mut header = tar::Header::new_gnu();
            header.set_size(3);
            header.set_cksum();
            builder
                .append_data(&mut header, "x.txt", &b"abc"[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let (entries, _) = archive_entries(&tar_path, ArchiveKind::Tar).unwrap();
        assert_eq!(entries[0].name, "x.txt");
        assert_eq!(entries[0].size, 3);
        assert!(archive_entries(&path, ArchiveKind::TarGz).is_err());
    }

    #[test]
    fn pdf_pages_are_found_by_their_text() {
        let pages = split_pages("Alpha intro\u{c}Beta has the WORD\u{c}gamma word again\u{c}");
        assert_eq!(pages.len(), 3);
        assert_eq!(search_pages(&pages, "word"), [2, 3]);
        assert!(search_pages(&pages, "  ").is_empty());
        assert_eq!(parse_page_count("Title: x\nPages:          12\n"), Some(12));
        assert_eq!(parse_page_count("nothing"), None);
    }

    #[test]
    fn a_program_that_hangs_is_stopped_and_a_missing_one_is_none() {
        let started = Instant::now();
        assert_eq!(
            run("sleep", &["30".into()], Duration::from_millis(200)),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            run("no-such-program-better-os", &[], Duration::from_secs(1)),
            None
        );
        assert_eq!(
            run("echo", &["$(x)".into()], Duration::from_secs(5)),
            Some(b"$(x)\n".to_vec())
        );
    }

    /// A program that started one of its own and hangs: the timeout ends
    /// both, as `soffice` and its `soffice.bin` have to be.
    #[test]
    fn a_timeout_ends_what_the_program_started_too() {
        let folder = tempfile::tempdir().unwrap();
        let pid_file = folder.path().join("pid");
        let script = format!("sleep 60 & echo $! > '{}'; wait", pid_file.display());
        assert_eq!(
            run("sh", &["-c".into(), script], Duration::from_millis(300)),
            None
        );
        let pid: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // Gone, or a zombie its shell parent left for init to reap.
        let deadline = Instant::now() + Duration::from_secs(5);
        let alive = || {
            fs::read_to_string(format!("/proc/{pid}/stat"))
                .map(|stat| !stat.contains(") Z "))
                .unwrap_or(false)
        };
        while alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "the grandchild {pid} is still running");
    }

    #[test]
    fn the_arrows_step_through_the_folder_by_name_without_hidden_files() {
        let root = tempfile::tempdir().unwrap();
        for name in ["b.txt", "A.txt", ".hidden", "c.txt"] {
            fs::write(root.path().join(name), b"x").unwrap();
        }
        let (all, at) = siblings(&root.path().join("b.txt"));
        let names: Vec<String> = all
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["A.txt", "b.txt", "c.txt"]);
        assert_eq!(at, 1);
    }

    #[test]
    fn poppler_renders_and_searches_a_real_pdf_when_installed() {
        if run("pdftoppm", &["-v".into()], Duration::from_secs(5)).is_none()
            && Command::new("pdftoppm").arg("-v").output().is_err()
        {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let pdf = root.path().join("two.pdf");
        fs::write(&pdf, TWO_PAGE_PDF).unwrap();
        assert_eq!(pdf_page_count(&pdf), Some(2));
        let page = render_pdf_page(&pdf, 2, 300, root.path()).expect("a rendered page");
        assert_eq!(image::image_dimensions(&page).unwrap().0, 300);
        let pages = pdf_text(&pdf).unwrap();
        assert_eq!(search_pages(&pages, "second"), [2]);
    }

    /// A minimal two-page PDF with "First page" and "Second page".
    const TWO_PAGE_PDF: &[u8] = b"%PDF-1.4
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R 5 0 R]/Count 2>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Resources<</Font<</F1 7 0 R>>>>/Contents 4 0 R>>endobj
4 0 obj<</Length 41>>stream
BT /F1 18 Tf 20 40 Td (First page) Tj ET
endstream endobj
5 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Resources<</Font<</F1 7 0 R>>>>/Contents 6 0 R>>endobj
6 0 obj<</Length 42>>stream
BT /F1 18 Tf 20 40 Td (Second page) Tj ET
endstream endobj
7 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj
trailer<</Root 1 0 R>>
%%EOF
";
}
