//! `org.freedesktop.FileManager1`: what "Show in folder" in a browser, an
//! editor or a download list calls. folio provides it (`src/filemanager1.cpp`);
//! so does Better Files, when no other file manager owns the name.
//!
//! `ShowFolders` opens each folder in a window; `ShowItems` and
//! `ShowItemProperties` open each item's folder with the item selected.
//! `file://` URIs and server addresses (`sftp://`, `smb://` and the other
//! schemes GVfs mounts, ticket 310, connected first) are taken; anything else
//! is ignored, never guessed at.

use std::sync::mpsc::Sender;

use files_core::LocalPath;

use crate::launch::StartTarget;

/// The bus name and object path the specification fixes.
pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

/// What a call asks for, one window per target.
pub fn targets(uris: &[String], select: bool) -> Vec<StartTarget> {
    uris.iter()
        .filter_map(|uri| {
            // A file or folder on a server (ticket 310): the window connects
            // first.
            if let Some(files_core::Location::Network(network)) =
                crate::launch::remote_argument(uri)
            {
                if !select {
                    return Some(StartTarget::Remote {
                        location: files_core::Location::Network(network),
                        select: None,
                    });
                }
                let trimmed = network.path.trim_end_matches('/');
                let (parent, name) = trimmed.rsplit_once('/')?;
                if name.is_empty() {
                    return None;
                }
                let folder = files_core::NetworkLocation::new(
                    network.scheme,
                    network.authority.clone(),
                    if parent.is_empty() { "/" } else { parent },
                )
                .ok()?;
                return Some(StartTarget::Remote {
                    location: files_core::Location::Network(folder),
                    select: Some(crate::network::percent_decode(name)),
                });
            }
            let path = local_path(uri)?;
            local_target(path, select)
        })
        .collect()
}

fn local_target(path: std::path::PathBuf, select: bool) -> Option<StartTarget> {
    if select {
        let folder = LocalPath::new(path.parent()?.to_path_buf()).ok()?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(StartTarget::File { folder, name })
    } else {
        LocalPath::new(path).ok().map(StartTarget::Folder)
    }
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

/// One window a call asks for. The call is answered once the window opened
/// ([`ShowRequest::opened`]), so a call that reaches a process on its way out
/// fails for the caller rather than being answered and lost (ticket 129).
#[derive(Debug)]
pub struct ShowRequest {
    pub target: StartTarget,
    opened: smol::channel::Sender<()>,
}

impl ShowRequest {
    /// Says the window is open; the call that asked for it is answered.
    pub fn opened(self) {
        let _ = self.opened.try_send(());
    }
}

/// How long a call waits for its window before it is answered with an
/// error.
pub const OPEN_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

struct FileManager1 {
    sender: Sender<ShowRequest>,
}

impl FileManager1 {
    async fn show(&self, targets: Vec<StartTarget>) -> zbus::fdo::Result<()> {
        let mut waiting = Vec::new();
        for target in targets {
            let (opened, answer) = smol::channel::bounded(1);
            self.sender
                .send(ShowRequest { target, opened })
                .map_err(|_| zbus::fdo::Error::Failed("Better Files is closing".into()))?;
            waiting.push(answer);
        }
        for answer in waiting {
            let opened = smol::future::or(async { answer.recv().await.is_ok() }, async {
                smol::Timer::after(OPEN_WAIT).await;
                false
            })
            .await;
            if !opened {
                return Err(zbus::fdo::Error::Failed(
                    "Better Files did not open the window".into(),
                ));
            }
        }
        Ok(())
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager1 {
    async fn show_folders(&self, uris: Vec<String>, _startup_id: String) -> zbus::fdo::Result<()> {
        self.show(targets(&uris, false)).await
    }

    async fn show_items(&self, uris: Vec<String>, _startup_id: String) -> zbus::fdo::Result<()> {
        self.show(targets(&uris, true)).await
    }

    async fn show_item_properties(
        &self,
        uris: Vec<String>,
        _startup_id: String,
    ) -> zbus::fdo::Result<()> {
        self.show(targets(&uris, true)).await
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
    sender: Sender<ShowRequest>,
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

/// This process's hold on the name, given up before it leaves.
#[derive(Clone, Default)]
pub struct NameHold(std::sync::Arc<std::sync::Mutex<Option<zbus::blocking::Connection>>>);

impl NameHold {
    /// A hold on a connection already serving the name.
    pub fn of(connection: zbus::blocking::Connection) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Some(connection))))
    }

    /// Gives the name back to the bus, so the next call starts a new Better
    /// Files rather than reaching this one as it leaves. The connection
    /// itself stays until the process ends: a call already received and
    /// waiting for its window still gets its answer on it.
    pub fn release(&self) {
        let connection = self.0.lock().ok().and_then(|held| held.clone());
        if let Some(connection) = connection {
            let _ = connection.release_name(BUS_NAME);
        }
    }
}

/// Serves the interface on the session bus from a thread of its own, sending
/// each request to `sender`. Does nothing (and says so on stderr) when there
/// is no session bus or another file manager owns the name: replacing it
/// would take "Show in folder" away from the one the person chose.
pub fn serve(sender: Sender<ShowRequest>) -> NameHold {
    let hold = NameHold::default();
    let kept = hold.clone();
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
            // The connection serves on its own executor; the hold keeps it
            // for the life of the process, or until it is released.
            if let Ok(mut held) = kept.0.lock() {
                *held = Some(connection);
            }
        });
    hold
}
