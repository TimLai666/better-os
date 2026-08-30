//! Plain text and source code.
//!
//! Three things this provider will not do.
//!
//! It will not read a whole file. `max_text_bytes` is applied to the read
//! itself, so a 4 GB log costs one bounded read and reports that it was
//! truncated.
//!
//! It will not guess an encoding from statistics. A byte-order mark is
//! evidence, and UTF-8 that decodes is evidence. Everything else that is not
//! binary is shown as Latin-1, which is the one single-byte mapping that never
//! fails and never invents a character — and it is labelled, so the pane says
//! which reading it is showing rather than presenting a guess as the file.
//!
//! It will not show binary as text. A NUL byte in the first chunk is the
//! freedesktop-era rule and it is still the right one: a file with a NUL in its
//! first few kilobytes is not something a text pane should render.

use std::fs::File;
use std::io::Read;

use crate::{
    CancelToken, DegradeReason, Preview, PreviewError, PreviewProvider, PreviewRequest,
    TextPreview, extension_of, top_level_is,
};

/// How the bytes were read as characters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEncoding {
    /// Valid UTF-8, with or without a byte-order mark.
    Utf8,
    Utf16Le,
    Utf16Be,
    /// The fallback: every byte is one code point. Never fails, so it is only
    /// reached once UTF-8 has failed.
    Latin1,
}

impl TextEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf16Le => "UTF-16LE",
            TextEncoding::Utf16Be => "UTF-16BE",
            TextEncoding::Latin1 => "ISO-8859-1",
        }
    }
}

/// File extensions that are text even when the shared MIME database calls them
/// something else — `application/json`, `application/x-shellscript`, and the
/// long tail of source types that are not under `text/`.
const TEXT_EXTENSIONS: &[&str] = &[
    "c", "cc", "cfg", "conf", "cpp", "cs", "css", "csv", "d", "desktop", "diff", "env", "go", "h",
    "hpp", "hs", "htm", "html", "ini", "java", "js", "json", "jsx", "kt", "less", "lock", "log",
    "lua", "m", "md", "mk", "ml", "patch", "php", "pl", "py", "r", "rb", "rs", "rst", "scss",
    "service", "sh", "sql", "svg", "swift", "tex", "toml", "ts", "tsv", "tsx", "txt", "vim", "xml",
    "yaml", "yml", "zsh",
];

/// MIME types outside `text/` that are still text.
const TEXT_MIME_TYPES: &[&str] = &[
    "application/json",
    "application/javascript",
    "application/x-shellscript",
    "application/xml",
    "application/x-desktop",
    "application/toml",
    "application/x-yaml",
    "application/yaml",
];

pub struct TextProvider;

impl PreviewProvider for TextProvider {
    fn id(&self) -> &'static str {
        "text"
    }

    fn handles(&self, request: &PreviewRequest) -> bool {
        if request.is_directory {
            return false;
        }
        if top_level_is(request.mime.as_ref(), "text") {
            return true;
        }
        if TEXT_MIME_TYPES.contains(&request.mime_str()) {
            return true;
        }
        // No type was resolved: fall back to the extension. A file with no
        // extension and no type is not claimed, so it degrades rather than
        // being read as text on a hunch.
        request.mime.is_none() && TEXT_EXTENSIONS.contains(&extension_of(&request.path).as_str())
    }

    fn generate(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> Result<Preview, PreviewError> {
        cancel.check()?;
        let limit = request.limits.max_text_bytes;
        let mut file = File::open(&request.path).map_err(|error| {
            PreviewError::Degraded(DegradeReason::Unreadable(error.to_string()))
        })?;
        let source_bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);

        // One bounded read of limit + 1 bytes. The extra byte is how truncation
        // is detected without a second syscall and without trusting the size
        // the metadata reported, which can be stale for a growing file.
        let mut buffer = Vec::with_capacity(limit.min(64 * 1024) + 1);
        let read = (&mut file)
            .take(limit as u64 + 1)
            .read_to_end(&mut buffer)
            .map_err(|error| {
                PreviewError::Degraded(DegradeReason::Unreadable(error.to_string()))
            })?;
        cancel.check()?;
        let truncated = read > limit;
        if truncated {
            buffer.truncate(limit);
        }

        if is_binary(&buffer) {
            return Err(PreviewError::Degraded(DegradeReason::Binary));
        }

        let (text, encoding) = decode(&buffer);
        cancel.check()?;
        let lines = text.lines().count();
        Ok(Preview::Text(TextPreview {
            text,
            encoding,
            truncated,
            lines,
            source_bytes,
        }))
    }
}

/// Whether these bytes are binary.
///
/// A NUL byte decides it. UTF-16 is excluded first, because half of a UTF-16
/// file is NUL bytes and calling it binary would be wrong.
pub fn is_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if utf16_bom(bytes).is_some() {
        return false;
    }
    bytes.contains(&0)
}

fn utf16_bom(bytes: &[u8]) -> Option<TextEncoding> {
    match bytes {
        [0xFF, 0xFE, ..] => Some(TextEncoding::Utf16Le),
        [0xFE, 0xFF, ..] => Some(TextEncoding::Utf16Be),
        _ => None,
    }
}

/// Turns bytes into a string, and says how it read them.
pub fn decode(bytes: &[u8]) -> (String, TextEncoding) {
    if let Some(encoding) = utf16_bom(bytes) {
        return (decode_utf16(&bytes[2..], encoding), encoding);
    }
    // A UTF-8 byte-order mark is stripped rather than shown as a character.
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(body) {
        Ok(text) => (text.to_string(), TextEncoding::Utf8),
        Err(error) if error.error_len().is_none() && error.valid_up_to() > 0 => {
            // The tail is an incomplete multi-byte sequence, which is what a
            // bounded read of a UTF-8 file produces. Cutting it off is right;
            // declaring the file Latin-1 because of it would not be.
            let valid = error.valid_up_to();
            let text = std::str::from_utf8(&body[..valid])
                .expect("valid_up_to bounds a valid prefix")
                .to_string();
            (text, TextEncoding::Utf8)
        }
        Err(_) => (
            body.iter().map(|&byte| byte as char).collect(),
            TextEncoding::Latin1,
        ),
    }
}

fn decode_utf16(bytes: &[u8], encoding: TextEncoding) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| match encoding {
            TextEncoding::Utf16Be => u16::from_be_bytes([pair[0], pair[1]]),
            _ => u16::from_le_bytes([pair[0], pair[1]]),
        })
        .collect();
    String::from_utf16_lossy(&units)
}
