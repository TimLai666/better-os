//! `org.freedesktop.FileManager1`: what "Show in folder" in a browser, an
//! editor or a download list calls. folio provides it (`src/filemanager1.cpp`);
//! so does Better Files, when no other file manager owns the name.
//!
//! `ShowFolders` opens each folder in a window; `ShowItems` and
//! `ShowItemProperties` open each item's folder with the item selected. Only
//! `file://` URIs are taken; anything else is ignored, never guessed at.

use std::sync::mpsc::Sender;

use files_core::LocalPath;

use crate::launch::StartTarget;

/// The bus name and object path the specification fixes.
pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

/// What a call asks for, one window per target.
pub fn targets(uris: &[String], select: bool) -> Vec<StartTarget> {
    uris.iter()
        .filter_map(|uri| local_path(uri))
        .filter_map(|path| {
            if select {
                let folder = LocalPath::new(path.parent()?.to_path_buf()).ok()?;
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some(StartTarget::File { folder, name })
            } else {
                LocalPath::new(path).ok().map(StartTarget::Folder)
            }
        })
        .collect()
}

/// A `file://` URI's path, `%XX` decoded. `None` for any other scheme, a
/// remote host, or a bad escape.
pub fn local_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = rest.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(out));
    // A trailing `..` or similar is resolved by the folder that is opened;
    // a path that is not absolute after decoding is refused.
    path.is_absolute().then_some(path)
}

struct FileManager1 {
    sender: Sender<StartTarget>,
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager1 {
    fn show_folders(&self, uris: Vec<String>, _startup_id: String) {
        for target in targets(&uris, false) {
            let _ = self.sender.send(target);
        }
    }

    fn show_items(&self, uris: Vec<String>, _startup_id: String) {
        for target in targets(&uris, true) {
            let _ = self.sender.send(target);
        }
    }

    fn show_item_properties(&self, uris: Vec<String>, _startup_id: String) {
        for target in targets(&uris, true) {
            let _ = self.sender.send(target);
        }
    }
}

/// Serves the interface on a connection from `builder` and then asks for the
/// name, without queueing behind another owner.
///
/// The interface goes in through the builder, so `build` returns only once
/// zbus's object server is reading calls; an object added to a built
/// connection starts that server lazily, and the "Show in folder" call that
/// arrives with the name could be dropped with no reply (ticket 107). The
/// connection is returned whether or not the name was granted, with the reply.
pub fn serve_on(
    builder: zbus::blocking::connection::Builder<'_>,
    sender: Sender<StartTarget>,
) -> zbus::Result<(
    zbus::blocking::Connection,
    zbus::Result<zbus::fdo::RequestNameReply>,
)> {
    let connection = builder
        .serve_at(OBJECT_PATH, FileManager1 { sender })?
        .build()?;
    let reply = connection
        .request_name_with_flags(BUS_NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into());
    Ok((connection, reply))
}

/// Serves the interface on the session bus from a thread of its own, sending
/// each request to `sender`. Does nothing (and says so on stderr) when there
/// is no session bus or another file manager owns the name: replacing it
/// would take "Show in folder" away from the one the person chose.
pub fn serve(sender: Sender<StartTarget>) {
    let _ = std::thread::Builder::new()
        .name("files-filemanager1".to_string())
        .spawn(move || {
            let built = zbus::blocking::connection::Builder::session()
                .and_then(|builder| serve_on(builder, sender));
            let connection = match built {
                Ok((connection, Ok(_))) => connection,
                Ok((_, Err(error))) => {
                    eprintln!("better-files: FileManager1 is someone else's: {error}");
                    return;
                }
                Err(error) => {
                    eprintln!("better-files: no session bus for FileManager1: {error}");
                    return;
                }
            };
            // The connection serves on its own executor; this thread only
            // keeps it alive for the life of the process.
            let _connection = connection;
            loop {
                std::thread::park();
            }
        });
}
