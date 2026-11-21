//! Better Screenshot on the session bus (the spec's "One process, its names,
//! and its life"):
//!
//! - `org.betteros.Screenshot` with `org.betteros.Screenshot1` at
//!   `/org/betteros/Screenshot`: a later start hands its request here, and a
//!   top bar can read and stop the recording;
//! - `org.freedesktop.impl.portal.desktop.betteros.Screenshot` with
//!   `org.freedesktop.impl.portal.Screenshot` at
//!   `/org/freedesktop/portal/desktop`, and a
//!   `org.freedesktop.impl.portal.Request` at each call's handle;
//! - a StatusNotifierItem while a recording runs;
//! - and, as a client, `org.freedesktop.Notifications`, the file manager's
//!   `org.freedesktop.FileManager1`, and the OpenURI portal.
//!
//! Every method only sends a [`Message`] to the window's thread and, for a
//! portal call, waits for its answer. zbus stays on its async-io executor
//! here (AGENTS.md): nothing in a handler needs tokio.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use screenshot_core::portal::{Outcome, ScreenshotOptions};
use screenshot_core::session::Request;
use screenshot_core::words::Notice;
use zbus::blocking::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

pub const BUS_NAME: &str = "org.betteros.Screenshot";
pub const OBJECT_PATH: &str = "/org/betteros/Screenshot";
pub const INTERFACE: &str = "org.betteros.Screenshot1";
pub const PORTAL_BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.betteros.Screenshot";
pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
pub const ITEM_PATH: &str = "/StatusNotifierItem";

/// What the bus asks the window's thread to do.
pub enum Message {
    Request(Request),
    StopRecording,
    PortalScreenshot {
        handle: String,
        options: ScreenshotOptions,
        reply: smol::channel::Sender<Outcome>,
    },
    PortalPickColor {
        handle: String,
        reply: smol::channel::Sender<Outcome>,
    },
    PortalClose {
        handle: String,
    },
    /// A notification's button or body was pressed.
    NotificationAction {
        key: String,
        path: PathBuf,
    },
}

/// What the bus reads back: the recording's state.
#[derive(Debug, Default)]
pub struct Status {
    pub recording: AtomicBool,
    pub seconds: AtomicU32,
    /// Notifications still shown, whose buttons need this process.
    pub notifications: AtomicUsize,
}

struct Control {
    tx: smol::channel::Sender<Message>,
    status: Arc<Status>,
}

#[zbus::interface(name = "org.betteros.Screenshot1")]
impl Control {
    /// Does what `better-screenshot` with that request's flag does:
    /// `interface`, `recording`, `screen`, `window`.
    async fn request(&self, name: String) -> zbus::fdo::Result<()> {
        let request = Request::from_name(&name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no request {name:?}")))?;
        self.tx
            .send(Message::Request(request))
            .await
            .map_err(|_| zbus::fdo::Error::Failed("the window is gone".into()))
    }

    /// Stops the recording, if one runs.
    async fn stop_recording(&self) -> zbus::fdo::Result<()> {
        self.tx
            .send(Message::StopRecording)
            .await
            .map_err(|_| zbus::fdo::Error::Failed("the window is gone".into()))
    }

    #[zbus(property)]
    fn recording(&self) -> bool {
        self.status.recording.load(Ordering::SeqCst)
    }

    /// Whole seconds since the recording started.
    #[zbus(property)]
    fn recording_seconds(&self) -> u32 {
        self.status.seconds.load(Ordering::SeqCst)
    }
}

struct Portal {
    tx: smol::channel::Sender<Message>,
}

/// The handle object of one portal call: `Close` ends it as cancelled.
struct PortalRequest {
    tx: smol::channel::Sender<Message>,
    handle: String,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl PortalRequest {
    async fn close(&self) {
        let _ = self
            .tx
            .send(Message::PortalClose {
                handle: self.handle.clone(),
            })
            .await;
    }
}

fn results(outcome: &Outcome) -> HashMap<String, OwnedValue> {
    let mut results = HashMap::new();
    match outcome {
        Outcome::Picture(uri) => {
            if let Ok(value) = OwnedValue::try_from(Value::from(uri.as_str())) {
                results.insert("uri".to_string(), value);
            }
        }
        Outcome::Color(red, green, blue) => {
            if let Ok(value) = OwnedValue::try_from(Value::from((*red, *green, *blue))) {
                results.insert("color".to_string(), value);
            }
        }
        Outcome::Cancelled | Outcome::Refused(_) => {}
    }
    results
}

fn read_options(options: &HashMap<String, OwnedValue>) -> ScreenshotOptions {
    let flag = |name: &str| {
        options
            .get(name)
            .and_then(|value| bool::try_from(value).ok())
            .unwrap_or(false)
    };
    ScreenshotOptions {
        interactive: flag("interactive"),
        permission_store_checked: flag("permission_store_checked"),
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl Portal {
    async fn screenshot(
        &self,
        handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let key = handle.as_str().to_string();
        let _ = server
            .at(
                handle.as_ref(),
                PortalRequest {
                    tx: self.tx.clone(),
                    handle: key.clone(),
                },
            )
            .await;
        let (reply, answer) = smol::channel::bounded(1);
        let outcome = match self
            .tx
            .send(Message::PortalScreenshot {
                handle: key,
                options: read_options(&options),
                reply,
            })
            .await
        {
            Ok(()) => answer
                .recv()
                .await
                .unwrap_or_else(|_| Outcome::Refused("the window is gone".into())),
            Err(_) => Outcome::Refused("the window is gone".into()),
        };
        let _ = server.remove::<PortalRequest, _>(handle.as_ref()).await;
        if let Outcome::Refused(reason) = &outcome {
            eprintln!("better-screenshot: portal screenshot refused: {reason}");
        }
        (outcome.response().code(), results(&outcome))
    }

    async fn pick_color(
        &self,
        handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        _options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let key = handle.as_str().to_string();
        let _ = server
            .at(
                handle.as_ref(),
                PortalRequest {
                    tx: self.tx.clone(),
                    handle: key.clone(),
                },
            )
            .await;
        let (reply, answer) = smol::channel::bounded(1);
        let outcome = match self
            .tx
            .send(Message::PortalPickColor { handle: key, reply })
            .await
        {
            Ok(()) => answer
                .recv()
                .await
                .unwrap_or_else(|_| Outcome::Refused("the window is gone".into())),
            Err(_) => Outcome::Refused("the window is gone".into()),
        };
        let _ = server.remove::<PortalRequest, _>(handle.as_ref()).await;
        (outcome.response().code(), results(&outcome))
    }

    /// Version 2: the frontend says when it has asked the person already.
    #[zbus(property(emits_changed_signal = "const"), name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

/// A StatusNotifierItem tool tip: icon name, icon pixmaps, title, text.
type ToolTip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);

/// The recording's tray item (spec R4, R6): its title has the time, and
/// pressing it stops the recording.
struct Item {
    tx: smol::channel::Sender<Message>,
    title: Arc<Mutex<String>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    async fn activate(&self, _x: i32, _y: i32) {
        let _ = self.tx.send(Message::StopRecording).await;
    }

    async fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = self.tx.send(Message::StopRecording).await;
    }

    async fn context_menu(&self, _x: i32, _y: i32) {}

    async fn scroll(&self, _delta: i32, _orientation: String) {}

    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        "better-screenshot"
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.title
            .lock()
            .map(|title| title.clone())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        "media-record"
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        let title = self
            .title
            .lock()
            .map(|title| title.clone())
            .unwrap_or_default();
        ("media-record".into(), Vec::new(), title, String::new())
    }

    #[zbus(signal)]
    async fn new_title(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// The process's bus connection and what it serves.
pub struct Service {
    pub connection: Connection,
    pub status: Arc<Status>,
    tx: smol::channel::Sender<Message>,
    item_title: Arc<Mutex<String>>,
    item_shown: bool,
}

/// Why the process is not the service.
pub enum Claim {
    /// This process owns the names and serves them.
    Owned(Service, smol::channel::Receiver<Message>),
    /// Another process owns them and was handed the request.
    HandedOver,
}

/// Becomes the one Better Screenshot, or hands `request` to the one that
/// runs. The portal's name is taken alongside; when another program holds
/// it, the interface still works and only the portal is not served.
pub fn claim(request: Request) -> Result<Claim, String> {
    let (tx, rx) = smol::channel::unbounded();
    let status = Arc::new(Status::default());
    let connection = zbus::blocking::connection::Builder::session()
        .map_err(|error| error.to_string())?
        .serve_at(
            OBJECT_PATH,
            Control {
                tx: tx.clone(),
                status: status.clone(),
            },
        )
        .map_err(|error| error.to_string())?
        .serve_at(PORTAL_PATH, Portal { tx: tx.clone() })
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let flags = zbus::fdo::RequestNameFlags::DoNotQueue.into();
    // zbus reports a name another connection holds as `NameTaken`.
    let owned = match connection.request_name_with_flags(BUS_NAME, flags) {
        Ok(reply) => reply == zbus::fdo::RequestNameReply::PrimaryOwner,
        Err(zbus::Error::NameTaken) => false,
        Err(error) => return Err(error.to_string()),
    };
    if !owned {
        // Another Better Screenshot runs: hand the request over.
        if request != Request::Portal {
            connection
                .call_method(
                    Some(BUS_NAME),
                    OBJECT_PATH,
                    Some(INTERFACE),
                    "Request",
                    &(request.name(),),
                )
                .map_err(|error| error.to_string())?;
        }
        return Ok(Claim::HandedOver);
    }
    if let Err(error) = connection.request_name_with_flags(PORTAL_BUS_NAME, flags) {
        eprintln!("better-screenshot: the portal's name is not ours: {error}");
    }
    Ok(Claim::Owned(
        Service {
            connection,
            status,
            tx,
            item_title: Arc::new(Mutex::new(String::new())),
            item_shown: false,
        },
        rx,
    ))
}

impl Service {
    /// Sends the recording's state to whoever reads the properties, and
    /// shows, updates, or takes away the tray item.
    pub fn set_recording(&mut self, recording: bool, seconds: u32, title: &str) {
        let was = self.status.recording.swap(recording, Ordering::SeqCst);
        self.status.seconds.store(seconds, Ordering::SeqCst);
        if let Ok(mut current) = self.item_title.lock() {
            *current = title.to_string();
        }
        let server = self.connection.object_server();
        if let Ok(control) = server.interface::<_, Control>(OBJECT_PATH) {
            let emitter = control.signal_emitter().clone();
            let control = control.get();
            if was != recording {
                let _ = zbus::block_on(control.recording_changed(&emitter));
            }
            let _ = zbus::block_on(control.recording_seconds_changed(&emitter));
        }
        if recording && !self.item_shown {
            let item = Item {
                tx: self.tx.clone(),
                title: self.item_title.clone(),
            };
            if server.at(ITEM_PATH, item).unwrap_or(false) {
                self.item_shown = true;
                let name = self
                    .connection
                    .unique_name()
                    .map(|name| name.to_string())
                    .unwrap_or_default();
                // No watcher (no top bar yet): the recording bar is the
                // indicator, and the item waits for nobody.
                let _ = self.connection.call_method(
                    Some("org.kde.StatusNotifierWatcher"),
                    "/StatusNotifierWatcher",
                    Some("org.kde.StatusNotifierWatcher"),
                    "RegisterStatusNotifierItem",
                    &(name,),
                );
            }
        } else if recording && self.item_shown {
            if let Ok(item) = server.interface::<_, Item>(ITEM_PATH) {
                let emitter = item.signal_emitter().clone();
                let _ = zbus::block_on(Item::new_title(&emitter));
                let _ = zbus::block_on(Item::new_tool_tip(&emitter));
            }
        } else if !recording && self.item_shown {
            let _ = server.remove::<Item, _>(ITEM_PATH);
            self.item_shown = false;
        }
    }

    /// Raises a notification and, while it is shown, sends its presses back
    /// as [`Message::NotificationAction`]. Returns false when nobody shows
    /// notifications (the session has no notification service until
    /// tickets 215 and 216).
    pub fn notify(&self, notice: &Notice, file: Option<&Path>, image: Option<&Path>) -> bool {
        let mut actions: Vec<&str> = Vec::new();
        if file.is_some() {
            actions.extend(["default", ""]);
        }
        if let Some(label) = notice.show_in_files {
            actions.extend(["show-in-files", label]);
        }
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("desktop-entry", Value::from("io.betteros.Screenshot"));
        hints.insert("transient", Value::from(true));
        let image_path = image.map(|path| path.to_string_lossy().into_owned());
        if let Some(path) = &image_path {
            hints.insert("image-path", Value::from(path.as_str()));
        }
        let reply = self.connection.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                notice.app_name,
                0u32,
                notice.icon,
                notice.title.as_str(),
                notice.body.as_str(),
                actions,
                hints,
                -1i32,
            ),
        );
        let id: u32 = match reply.and_then(|message| message.body().deserialize::<u32>()) {
            Ok(id) => id,
            Err(error) => {
                eprintln!("better-screenshot: no notification shown: {error}");
                return false;
            }
        };
        let Some(path) = file.map(Path::to_path_buf) else {
            return true;
        };
        // Wait for this notification's presses while it is shown.
        let connection = self.connection.clone();
        let tx = self.tx.clone();
        let status = self.status.clone();
        status.notifications.fetch_add(1, Ordering::SeqCst);
        let _ = std::thread::Builder::new()
            .name("notification".into())
            .spawn(move || {
                listen(&connection, id, &path, &tx);
                status.notifications.fetch_sub(1, Ordering::SeqCst);
            });
        true
    }
}

/// How long a notification's buttons keep the process: a notification
/// service that never says the notification closed does not keep it forever.
pub const NOTIFICATION_PATIENCE: Duration = Duration::from_secs(15 * 60);

fn listen(connection: &Connection, id: u32, path: &Path, tx: &smol::channel::Sender<Message>) {
    let rule = match zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.Notifications")
        .and_then(|rule| rule.path("/org/freedesktop/Notifications"))
    {
        Ok(rule) => rule.build(),
        Err(_) => return,
    };
    let Ok(messages) = zbus::blocking::MessageIterator::for_match_rule(rule, connection, None)
    else {
        return;
    };
    let started = Instant::now();
    for message in messages {
        if started.elapsed() > NOTIFICATION_PATIENCE {
            return;
        }
        let Ok(message) = message else { return };
        let header = message.header();
        match header.member().map(|member| member.as_str()) {
            Some("ActionInvoked") => {
                if let Ok((which, key)) = message.body().deserialize::<(u32, String)>()
                    && which == id
                {
                    let _ = tx.send_blocking(Message::NotificationAction {
                        key,
                        path: path.to_path_buf(),
                    });
                }
            }
            Some("NotificationClosed") => {
                if let Ok((which, _reason)) = message.body().deserialize::<(u32, u32)>()
                    && which == id
                {
                    return;
                }
            }
            _ => {}
        }
    }
}

/// "Show in Files": the file selected in its folder, by the person's file
/// manager over `org.freedesktop.FileManager1` (what Better Git does,
/// `crates/git-gui/src/file_manager.rs`); without one, the folder through
/// the OpenURI portal.
pub fn show_in_files(connection: &Connection, path: &Path) -> Result<(), String> {
    let uri = better_core::uri::file_uri(path);
    let shown = connection.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowItems",
        &(vec![uri], ""),
    );
    match shown {
        Ok(_) => Ok(()),
        Err(error) => match path.parent() {
            Some(folder) => open_uri(connection, folder),
            None => Err(error.to_string()),
        },
    }
}

/// Opens a file or folder with the person's default application, through
/// the OpenURI portal, as the notification's body asks (GNOME's
/// `launch_default_for_uri`).
pub fn open_uri(connection: &Connection, path: &Path) -> Result<(), String> {
    let uri = better_core::uri::file_uri(path);
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    connection
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.OpenURI"),
            "OpenURI",
            &("", uri.as_str(), options),
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{read_options, results};
    use screenshot_core::portal::Outcome;
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedValue, Value};

    #[test]
    fn a_picture_answers_its_uri_and_a_colour_its_three_channels() {
        let picture = results(&Outcome::Picture("file:///a.png".into()));
        assert_eq!(
            String::try_from(picture["uri"].clone()).unwrap(),
            "file:///a.png"
        );
        let color = results(&Outcome::Color(1.0, 0.5, 0.0));
        let (r, g, b): (f64, f64, f64) = color["color"].clone().try_into().unwrap();
        assert_eq!((r, g, b), (1.0, 0.5, 0.0));
        assert!(results(&Outcome::Cancelled).is_empty());
        assert!(results(&Outcome::Refused("x".into())).is_empty());
    }

    #[test]
    fn options_are_read_by_name_and_missing_ones_are_false() {
        let mut options: HashMap<String, OwnedValue> = HashMap::new();
        assert!(!read_options(&options).interactive);
        options.insert(
            "interactive".into(),
            OwnedValue::try_from(Value::from(true)).unwrap(),
        );
        options.insert(
            "modal".into(),
            OwnedValue::try_from(Value::from(true)).unwrap(),
        );
        let read = read_options(&options);
        assert!(read.interactive);
        assert!(!read.permission_store_checked);
    }
}
