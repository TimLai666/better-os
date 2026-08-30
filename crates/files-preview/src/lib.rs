//! Better Files preview: the interface, and the two implementations that ship.
//!
//! Issue #6 states five requirements for preview work, and each one is held by
//! a specific piece of this crate rather than by care at the call site.
//!
//! **Off the render thread.** Nothing here touches GPUI, and [`PreviewService`]
//! runs every generation on its own worker thread. The window asks for a
//! preview and keeps drawing; it collects the answer on a later frame.
//!
//! **Cancellable.** Every generation is handed a [`CancelToken`] and checks it
//! between bounded units of work — a chunk of a file, a directory entry, the
//! step before a decode. Asking for a second preview cancels the first, which
//! is what arrow-keying down a folder does sixty times a second.
//!
//! **Size- and resource-limited.** [`PreviewLimits`] is applied before a file is
//! opened, not after it is read. An oversized file never reaches a parser.
//!
//! **Untrusted parsers are a boundary.** A preview reads a file that arrived
//! from somewhere else, and it hands it to a decoder. Three things enforce the
//! boundary: the byte limit above, the decoder's own pixel and allocation
//! limits ([`image::Limits`]), and a [`std::panic::catch_unwind`] around every
//! provider call so a panicking parser degrades this one preview instead of
//! taking the file manager with it. This is a boundary, not a sandbox — see
//! `docs/files-preview-policy.md` for what a real one would need.
//!
//! **Degrades to metadata.** Every refusal produces a [`Preview::Metadata`]
//! carrying the reason. There is no path that returns nothing and no path that
//! silently shows an empty pane.

pub mod cancel;
pub mod folder;
pub mod image_provider;
pub mod service;
pub mod text;

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

use app_catalog_core::MimeType;
use thiserror::Error;

pub use cancel::{CancelToken, Cancelled};
pub use folder::FolderProvider;
pub use image_provider::ImageProvider;
pub use service::{PreviewOutcome, PreviewService, RequestId};
pub use text::{TextEncoding, TextProvider};

/// Everything a preview is allowed to spend.
///
/// The defaults are chosen for a preview pane, not for a viewer: a preview that
/// takes a second is worse than no preview, so the ceilings are low enough that
/// the slow case stays interactive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewLimits {
    /// Files larger than this are never opened by a parser.
    pub max_source_bytes: u64,
    /// The largest image a decoder is allowed to allocate, in pixels.
    pub max_image_pixels: u64,
    /// The decoder's total allocation ceiling, in bytes.
    pub max_decode_bytes: u64,
    /// How much of a text file is read. The rest is reported as truncated.
    pub max_text_bytes: usize,
    /// How many directory entries a folder summary counts before it stops and
    /// says the total is a floor rather than a count.
    pub max_folder_entries: usize,
    /// The longest edge of the produced thumbnail.
    pub thumbnail_edge: u32,
}

impl Default for PreviewLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 64 * 1024 * 1024,
            max_image_pixels: 64_000_000,
            max_decode_bytes: 256 * 1024 * 1024,
            max_text_bytes: 128 * 1024,
            max_folder_entries: 20_000,
            thumbnail_edge: 512,
        }
    }
}

/// What is being previewed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewRequest {
    pub path: PathBuf,
    /// The MIME type the listing already resolved, when it resolved one.
    /// `None` means "not asked yet"; a provider may sniff, but never re-reads
    /// the shared MIME database, which is `files-platform`'s job.
    pub mime: Option<MimeType>,
    /// True when the target is a directory. The caller knows this from the
    /// entry it already has, so no provider needs a `stat` to find out.
    pub is_directory: bool,
    pub limits: PreviewLimits,
}

impl PreviewRequest {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            mime: None,
            is_directory: false,
            limits: PreviewLimits::default(),
        }
    }

    pub fn directory(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            mime: None,
            is_directory: true,
            limits: PreviewLimits::default(),
        }
    }

    pub fn with_mime(mut self, mime: Option<MimeType>) -> Self {
        self.mime = mime;
        self
    }

    pub fn with_limits(mut self, limits: PreviewLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The MIME type as a string, or the empty string when none was resolved.
    pub fn mime_str(&self) -> &str {
        self.mime.as_ref().map_or("", MimeType::as_str)
    }

    /// The file's size, or `None` when it cannot be read.
    pub fn source_bytes(&self) -> Option<u64> {
        std::fs::metadata(&self.path).ok().map(|meta| meta.len())
    }
}

/// A decoded image, ready to upload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImagePreview {
    pub width: u32,
    pub height: u32,
    /// The dimensions of the file itself, which is what the details panel
    /// shows. The thumbnail above is usually smaller.
    pub source_width: u32,
    pub source_height: u32,
    pub format: &'static str,
    /// Row-major RGBA, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// A bounded read of a text file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextPreview {
    pub text: String,
    pub encoding: TextEncoding,
    /// True when the file is longer than the limit, so the pane can say so
    /// rather than implying the file ends where the preview does.
    pub truncated: bool,
    pub lines: usize,
    pub source_bytes: u64,
}

/// What a folder contains, counted rather than guessed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderSummary {
    pub files: usize,
    pub directories: usize,
    /// The size of the immediate children only. A recursive total needs a walk
    /// that is not bounded by anything a preview may spend.
    pub immediate_bytes: u64,
    /// True when the entry limit stopped the count, so the numbers are floors.
    pub truncated: bool,
}

/// Why a preview is metadata rather than content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DegradeReason {
    /// Nothing in this build can render the type.
    NoProvider,
    /// The file is over the size limit. The limit is carried so the pane can
    /// state it rather than saying "too large" and leaving the user guessing.
    TooLarge { limit: u64, actual: u64 },
    /// A text provider found bytes that are not text.
    Binary,
    /// The parser ran and failed. The string is the decoder's own message.
    DecodeFailed(String),
    /// The parser panicked and the boundary caught it.
    ParserFaulted,
    /// The file could not be read at all.
    Unreadable(String),
}

impl DegradeReason {
    /// A stable machine key. Presentation layers own the wording, the way
    /// `manager-core` errors do.
    pub fn key(&self) -> &'static str {
        match self {
            DegradeReason::NoProvider => "files.preview.degraded.no_provider",
            DegradeReason::TooLarge { .. } => "files.preview.degraded.too_large",
            DegradeReason::Binary => "files.preview.degraded.binary",
            DegradeReason::DecodeFailed(_) => "files.preview.degraded.decode_failed",
            DegradeReason::ParserFaulted => "files.preview.degraded.parser_faulted",
            DegradeReason::Unreadable(_) => "files.preview.degraded.unreadable",
        }
    }
}

/// The fallback every refusal produces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataPreview {
    pub size_bytes: Option<u64>,
    pub mime: Option<String>,
    pub reason: DegradeReason,
}

/// What a preview is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Preview {
    Image(ImagePreview),
    Text(TextPreview),
    Folder(FolderSummary),
    Metadata(MetadataPreview),
}

impl Preview {
    /// Whether this is the degraded form. Used by tests and by the pane, which
    /// draws a reason instead of content.
    pub fn is_metadata_only(&self) -> bool {
        matches!(self, Preview::Metadata(_))
    }

    pub fn degrade_reason(&self) -> Option<&DegradeReason> {
        match self {
            Preview::Metadata(meta) => Some(&meta.reason),
            _ => None,
        }
    }
}

/// The only two ways generation ends without a `Preview`.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum PreviewError {
    /// The token was set. Nothing partial is returned: a half-decoded image is
    /// not a preview of anything.
    #[error("files.preview.error.cancelled")]
    Cancelled,
    /// This provider does not handle the request; try the next one.
    #[error("files.preview.error.not_handled")]
    NotHandled,
    /// The provider ran and refused. It has already decided what the pane
    /// should say.
    #[error("files.preview.error.degraded:{}", .0.key())]
    Degraded(DegradeReason),
}

impl From<Cancelled> for PreviewError {
    fn from(_: Cancelled) -> Self {
        PreviewError::Cancelled
    }
}

/// One way of turning a file into a preview.
///
/// A trait rather than a match, because Issue #6's out-of-scope list — Markdown,
/// PDF, media, archives — is a list of providers that must be addable without
/// this crate's interface changing.
pub trait PreviewProvider: Send + Sync {
    /// A stable identifier, used in diagnostics and in tests that assert which
    /// provider answered.
    fn id(&self) -> &'static str;

    /// Whether this provider claims the request. Cheap: no I/O.
    fn handles(&self, request: &PreviewRequest) -> bool;

    /// Produces the preview, or says why it will not.
    fn generate(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> Result<Preview, PreviewError>;
}

/// The providers this build has, in priority order.
///
/// The engine is the security boundary: it applies the size limit before any
/// provider is called, it catches a panicking parser, and it turns every
/// refusal into a metadata preview so a caller has one shape to render.
pub struct PreviewEngine {
    providers: Vec<Box<dyn PreviewProvider>>,
}

impl Default for PreviewEngine {
    /// Image, then text, then folder. The shipped set.
    fn default() -> Self {
        Self::new(vec![
            Box::new(FolderProvider),
            Box::new(ImageProvider),
            Box::new(TextProvider),
        ])
    }
}

impl PreviewEngine {
    pub fn new(providers: Vec<Box<dyn PreviewProvider>>) -> Self {
        Self { providers }
    }

    /// An engine with no providers, so every request degrades. Used by tests
    /// that assert the fallback, and by a future safe-mode switch.
    pub fn metadata_only() -> Self {
        Self::new(Vec::new())
    }

    pub fn provider_ids(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    /// Produces a preview. This never returns a provider's panic, and outside
    /// of cancellation it never returns an error: a refusal is a metadata
    /// preview carrying its reason.
    pub fn preview(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> Result<Preview, Cancelled> {
        if cancel.is_cancelled() {
            return Err(Cancelled);
        }
        let size = request.source_bytes();

        // The size limit is applied here, before a provider sees the request,
        // so a new provider cannot forget it. Directories have no size and are
        // bounded by their own entry limit instead.
        if !request.is_directory
            && let Some(actual) = size
            && actual > request.limits.max_source_bytes
        {
            return Ok(self.degrade(
                request,
                size,
                DegradeReason::TooLarge {
                    limit: request.limits.max_source_bytes,
                    actual,
                },
            ));
        }

        for provider in &self.providers {
            if !provider.handles(request) {
                continue;
            }
            // The parser boundary. A decoder that panics on a malformed file
            // loses this preview and nothing else.
            let result =
                std::panic::catch_unwind(AssertUnwindSafe(|| provider.generate(request, cancel)));
            match result {
                Ok(Ok(preview)) => return Ok(preview),
                Ok(Err(PreviewError::Cancelled)) => return Err(Cancelled),
                Ok(Err(PreviewError::NotHandled)) => continue,
                Ok(Err(PreviewError::Degraded(reason))) => {
                    return Ok(self.degrade(request, size, reason));
                }
                Err(_) => {
                    return Ok(self.degrade(request, size, DegradeReason::ParserFaulted));
                }
            }
        }
        Ok(self.degrade(request, size, DegradeReason::NoProvider))
    }

    fn degrade(
        &self,
        request: &PreviewRequest,
        size: Option<u64>,
        reason: DegradeReason,
    ) -> Preview {
        Preview::Metadata(MetadataPreview {
            size_bytes: size,
            mime: request.mime.as_ref().map(|m| m.as_str().to_string()),
            reason,
        })
    }
}

/// Whether a MIME type or a file name looks like the given top-level type.
///
/// Shared by the providers so "is this an image" is answered the same way
/// everywhere, and so the answer is derived from the type the listing already
/// resolved rather than from the extension whenever a type exists.
pub(crate) fn top_level_is(mime: Option<&MimeType>, expected: &str) -> bool {
    mime.is_some_and(|mime| {
        mime.as_str()
            .split('/')
            .next()
            .is_some_and(|top| top.eq_ignore_ascii_case(expected))
    })
}

pub(crate) fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}
