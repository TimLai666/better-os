//! Folder summary: the fallback for the one thing a preview pane is always
//! asked about and no parser handles.
//!
//! Issue #6 lists folder summary among the preview targets. It is the cheapest
//! one and the only one whose cost is unbounded in the wrong direction: a
//! directory can hold a million entries, and a recursive size is a filesystem
//! walk. So this counts the immediate children only, stops at
//! `max_folder_entries`, and says when it stopped — a truncated summary is
//! reported as a floor, never rounded into a total.

use crate::{
    CancelToken, DegradeReason, FolderSummary, Preview, PreviewError, PreviewProvider,
    PreviewRequest,
};

/// How often the cancellation flag is read while counting. Reading it per entry
/// would be an atomic load per `readdir` result; once per chunk keeps a
/// cancelled scan of a huge directory bounded without that cost.
const CANCEL_CHECK_INTERVAL: usize = 256;

pub struct FolderProvider;

impl PreviewProvider for FolderProvider {
    fn id(&self) -> &'static str {
        "folder"
    }

    fn handles(&self, request: &PreviewRequest) -> bool {
        request.is_directory
    }

    fn generate(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> Result<Preview, PreviewError> {
        cancel.check()?;
        let entries = std::fs::read_dir(&request.path).map_err(|error| {
            PreviewError::Degraded(DegradeReason::Unreadable(error.to_string()))
        })?;

        let mut summary = FolderSummary {
            files: 0,
            directories: 0,
            immediate_bytes: 0,
            truncated: false,
        };
        let limit = request.limits.max_folder_entries;
        let mut seen = 0usize;

        for entry in entries {
            if seen >= limit {
                summary.truncated = true;
                break;
            }
            if seen.is_multiple_of(CANCEL_CHECK_INTERVAL) {
                cancel.check()?;
            }
            seen += 1;
            let Ok(entry) = entry else {
                // One unreadable entry does not invalidate the count of the
                // rest. It is skipped, and the totals stay honest about being
                // what could be read.
                continue;
            };
            // `DirEntry::metadata` does not follow a symlink, which is what
            // keeps a link loop from turning a summary into a hang.
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                summary.directories += 1;
            } else {
                summary.files += 1;
                summary.immediate_bytes = summary.immediate_bytes.saturating_add(metadata.len());
            }
        }

        Ok(Preview::Folder(summary))
    }
}
