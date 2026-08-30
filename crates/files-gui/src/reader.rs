//! The directory reader the window hands to every pane.
//!
//! `files-platform` has one reader per source: a local directory, and the
//! freedesktop trash. A pane takes one `DirectoryReader`, so this is the
//! dispatcher that picks the right one for a typed location — and refuses the
//! locations this build cannot list with the same [`ListingError::NotListable`]
//! the model already knows how to draw, rather than with an empty folder that
//! looks like a folder with nothing in it.
//!
//! Nothing here reads a directory on the calling thread. Every branch either
//! spawns a thread or hands the request to a reader that does.

use std::thread;

use files_core::listing::{DirectoryReader, ListingRequest, ListingSink};
use files_core::{ListingError, Location, TrashLocation};
use files_platform::{LocalDirectoryReader, ReaderConfig, TrashDirectory, read_trash};

/// Reads whichever kind of location it is handed.
pub struct FilesReader {
    local: LocalDirectoryReader,
    /// The home trash, when the session has one. `None` means the Trash
    /// location lists as empty rather than failing, which is what a session
    /// with no `$XDG_DATA_HOME` and no `$HOME` honestly has.
    trash: Option<TrashDirectory>,
}

impl FilesReader {
    pub fn new(config: ReaderConfig, trash: Option<TrashDirectory>) -> Self {
        Self {
            local: LocalDirectoryReader::with_config(config),
            trash,
        }
    }

    /// The reader a running window uses: MIME detection from the session's
    /// shared MIME database, and the home trash.
    pub fn from_env() -> Self {
        Self::new(
            ReaderConfig::new().with_mime(files_platform::detector_from_env()),
            TrashDirectory::home_from_env(),
        )
    }

    pub fn trash(&self) -> Option<&TrashDirectory> {
        self.trash.as_ref()
    }
}

impl DirectoryReader for FilesReader {
    fn start(&self, request: ListingRequest, sink: ListingSink) {
        match &request.location {
            Location::Local(_) => self.local.start(request, sink),
            Location::Trash(TrashLocation::Root) => {
                let Some(trash) = self.trash.clone() else {
                    // No trash directory for this session: an empty listing,
                    // not a failure. "Nothing I can see" is the honest answer.
                    let _ = sink.finish();
                    return;
                };
                let _ = thread::Builder::new()
                    .name("files-trash-listing".to_string())
                    .spawn(move || {
                        let mut sink = sink;
                        if read_trash(&trash, &mut sink).is_ok() {
                            let _ = sink.finish();
                        }
                        // A cancelled read drops the sink, whose `Drop`
                        // reports the cancellation.
                    });
            }
            other => {
                let kind = other.kind();
                sink.fail(ListingError::NotListable(kind));
            }
        }
    }
}
