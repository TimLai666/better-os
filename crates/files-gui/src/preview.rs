//! The preview pane: when it asks, what it shows, and when it stops.
//!
//! `files-preview` owns the generation, the limits, and the worker thread. This
//! is the part that belongs to a window: which entry is being previewed, which
//! answer is still wanted, and what the pane draws while there is none.
//!
//! **Nothing is generated on the render thread.** [`PreviewPanel::request_for`]
//! posts and returns; [`PreviewPanel::pump`] takes whatever finished. The
//! service cancels the previous request as part of accepting the new one, so
//! holding Down through a folder of photographs decodes the one the user stops
//! on rather than all of them.
//!
//! **A late answer is discarded, not drawn.** Every outcome carries the id it
//! was asked under, and anything that is not the current id is dropped. Without
//! that, a slow decode of the previous selection would replace the preview of
//! the current one.
//!
//! **Closing the pane cancels.** Space with the pane open cancels whatever is
//! in flight rather than leaving a decode running for a pane nobody is looking
//! at.

use std::sync::Arc;

use files_core::{Entry, EntryKind, Location};
use files_preview::{Preview, PreviewLimits, PreviewRequest, PreviewService, RequestId};

use crate::i18n::Copy;

/// What the pane is showing right now.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PreviewSlot {
    /// The pane is open and nothing is selected.
    #[default]
    Nothing,
    /// A request is out and no answer has come back.
    Waiting,
    Ready(Box<Preview>),
    /// The selected entry cannot be previewed at all — an application row,
    /// which has no file behind it by construction.
    NotPreviewable,
}

/// The preview pane's state.
pub struct PreviewPanel {
    service: Arc<PreviewService>,
    /// Whether the pane is open. Space toggles it, and the preference is
    /// persisted with the other view settings.
    pub open: bool,
    pub limits: PreviewLimits,
    slot: PreviewSlot,
    /// The request whose answer is still wanted.
    pending: Option<RequestId>,
    /// What the last request was for, so an unchanged selection does not
    /// restart a decode every frame.
    last_target: Option<String>,
}

impl Default for PreviewPanel {
    fn default() -> Self {
        Self::new(Arc::new(PreviewService::default()))
    }
}

impl PreviewPanel {
    pub fn new(service: Arc<PreviewService>) -> Self {
        Self {
            service,
            open: false,
            limits: PreviewLimits::default(),
            slot: PreviewSlot::Nothing,
            pending: None,
            last_target: None,
        }
    }

    pub fn slot(&self) -> &PreviewSlot {
        &self.slot
    }

    pub fn is_waiting(&self) -> bool {
        matches!(self.slot, PreviewSlot::Waiting)
    }

    /// Space. Opening asks for the current selection; closing cancels.
    pub fn toggle(&mut self) -> bool {
        self.open = !self.open;
        if !self.open {
            self.stop();
        }
        self.open
    }

    /// Cancels whatever is in flight and forgets what was showing.
    pub fn stop(&mut self) {
        self.service.cancel_in_flight();
        self.pending = None;
        self.last_target = None;
        self.slot = PreviewSlot::Nothing;
    }

    /// Asks for a preview of whatever is focused.
    ///
    /// `None` clears the pane. An unchanged target is a no-op, which is what
    /// makes calling this every frame free.
    pub fn request_for(&mut self, entry: Option<&Entry>, location: &Location) {
        if !self.open {
            return;
        }
        let Some(entry) = entry else {
            if self.last_target.is_some() {
                self.stop();
            }
            return;
        };
        let key = format!("{:?}", entry.id());
        if self.last_target.as_deref() == Some(key.as_str()) {
            return;
        }
        self.last_target = Some(key);

        let Some(request) = request_for(entry, location, self.limits) else {
            // An application row has no file. Saying so beats an empty pane
            // and beats a metadata card about a path that does not exist.
            self.pending = None;
            self.slot = PreviewSlot::NotPreviewable;
            return;
        };
        self.pending = Some(self.service.request(request));
        self.slot = PreviewSlot::Waiting;
    }

    /// Takes whatever finished. Returns whether the pane changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        for outcome in self.service.poll() {
            if self.pending != Some(outcome.id) {
                // An answer to a question that was withdrawn.
                continue;
            }
            self.pending = None;
            match outcome.preview {
                Some(preview) => self.slot = PreviewSlot::Ready(Box::new(preview)),
                // Cancelled between the poll and the worker noticing. Leave the
                // pane waiting; the request that superseded it will answer.
                None => self.slot = PreviewSlot::Waiting,
            }
            changed = true;
        }
        changed
    }

    /// The message the pane draws when it has no content.
    pub fn placeholder(&self, c: &'static Copy) -> Option<&'static str> {
        match &self.slot {
            PreviewSlot::Nothing => Some(c.preview_nothing_selected),
            PreviewSlot::Waiting => Some(c.preview_loading),
            PreviewSlot::NotPreviewable => Some(c.preview_not_previewable),
            PreviewSlot::Ready(_) => None,
        }
    }
}

/// Builds the request for one entry, or `None` when there is no file to read.
///
/// `Entry::as_local_path` answers `None` for an application row and for a
/// trashed item, which is why both reach the pane as "nothing to preview"
/// rather than as a request for a path that is not the entry. Previewing a
/// trashed item needs its stored path, which `files-core` deliberately does not
/// expose as the entry's path; that is a follow-up, not an oversight here.
pub fn request_for(
    entry: &Entry,
    _location: &Location,
    limits: PreviewLimits,
) -> Option<PreviewRequest> {
    let path = entry.as_local_path()?;
    let request = if entry.kind == EntryKind::Directory {
        PreviewRequest::directory(path.as_path().to_path_buf())
    } else {
        PreviewRequest::file(path.as_path().to_path_buf())
    };
    Some(request.with_mime(entry.mime.clone()).with_limits(limits))
}

/// The reason line for a degraded preview, in the user's language.
pub fn degrade_message(reason: &files_preview::DegradeReason, c: &'static Copy) -> String {
    use files_preview::DegradeReason as R;
    match reason {
        R::NoProvider => c.preview_no_provider.to_string(),
        R::TooLarge { limit, .. } => c
            .preview_too_large
            .replace("{limit}", &crate::format::bytes(*limit)),
        R::Binary => c.preview_binary.to_string(),
        R::DecodeFailed(_) => c.preview_decode_failed.to_string(),
        R::ParserFaulted => c.preview_parser_faulted.to_string(),
        R::Unreadable(_) => c.preview_unreadable.to_string(),
    }
}
