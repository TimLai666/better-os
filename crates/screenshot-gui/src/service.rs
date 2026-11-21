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
use smol::stream::StreamExt;
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
    /// logind asked the session to lock (`Lock`): the interface closes.
    SessionLocking,
    /// logind's `LockedHint`, when it changes.
    LockedHint(bool),
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

/// The bus name of the portal frontend, the one caller the backend serves.
pub const PORTAL_FRONTEND: &str = "org.freedesktop.portal.Desktop";

/// Whether `sender` is the portal frontend. A Flatpak's bus proxy never lets
/// an application reach a backend's name; an unsandboxed process could, and
/// gains nothing it did not have (`cosmic-comp` offers its capture protocols
/// to every unsandboxed client, `src/state.rs` 691–698), but the backend
/// answers only the frontend, whose permission store and Access dialog are
/// the portal's checks.
async fn from_frontend(connection: &zbus::Connection, sender: Option<&str>) -> bool {
    let Some(sender) = sender else { return false };
    let Ok(proxy) = zbus::fdo::DBusProxy::new(connection).await else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from(PORTAL_FRONTEND) else {
        return false;
    };
    proxy
        .get_name_owner(name)
        .await
        .is_ok_and(|owner| owner.as_str() == sender)
}

/// A watch on `sender` leaving the bus, subscribed before the call is handed
/// on so a caller that leaves at once is not missed.
async fn watch_leaving(
    connection: &zbus::Connection,
    sender: &str,
) -> Option<zbus::fdo::NameOwnerChangedStream> {
    let proxy = zbus::fdo::DBusProxy::new(connection).await.ok()?;
    let changes = proxy
        .receive_name_owner_changed_with_args(&[(0, sender)])
        .await
        .ok()?;
    // Gone before the watch began.
    let name = zbus::names::BusName::try_from(sender.to_string()).ok()?;
    if !proxy.name_has_owner(name).await.unwrap_or(false) {
        return None;
    }
    Some(changes)
}

/// Waits until the watched caller has left; without a watch, it has.
async fn caller_left(changes: Option<zbus::fdo::NameOwnerChangedStream>) {
    let Some(mut changes) = changes else { return };
    while let Some(change) = changes.next().await {
        if let Ok(args) = change.args()
            && args.new_owner().is_none()
        {
            return;
        }
    }
}

impl Portal {
    /// One call: the handle's `Request` object, the message to the window's
    /// thread, and its answer, or "cancelled" when the frontend leaves the bus
    /// first (GNOME's `_onNameVanished`, 2477).
    async fn serve(
        &self,
        connection: &zbus::Connection,
        server: &zbus::ObjectServer,
        sender: Option<String>,
        handle: OwnedObjectPath,
        message: impl FnOnce(String, smol::channel::Sender<Outcome>) -> Message,
    ) -> (u32, HashMap<String, OwnedValue>) {
        if !from_frontend(connection, sender.as_deref()).await {
            eprintln!("better-screenshot: a portal call not from the portal frontend was refused");
            let refused = Outcome::Refused("not the portal frontend".into());
            return (refused.response().code(), HashMap::new());
        }
        let key = handle.as_str().to_string();
        let leaving = watch_leaving(connection, sender.as_deref().unwrap_or_default()).await;
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
        let outcome = match self.tx.send(message(key.clone(), reply)).await {
            Ok(()) => {
                let answered = async {
                    answer
                        .recv()
                        .await
                        .unwrap_or_else(|_| Outcome::Refused("the window is gone".into()))
                };
                let left = async {
                    caller_left(leaving).await;
                    let _ = self.tx.send(Message::PortalClose { handle: key }).await;
                    Outcome::Cancelled
                };
                smol::future::or(answered, left).await
            }
            Err(_) => Outcome::Refused("the window is gone".into()),
        };
        let _ = server.remove::<PortalRequest, _>(handle.as_ref()).await;
        if let Outcome::Refused(reason) = &outcome {
            eprintln!("better-screenshot: portal request refused: {reason}");
        }
        (outcome.response().code(), results(&outcome))
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
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let sender = header.sender().map(|sender| sender.to_string());
        let options = read_options(&options);
        self.serve(
            connection,
            connection.object_server(),
            sender,
            handle,
            |handle, reply| Message::PortalScreenshot {
                handle,
                options,
                reply,
            },
        )
        .await
    }

    async fn pick_color(
        &self,
        handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        _options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let sender = header.sender().map(|sender| sender.to_string());
        self.serve(
            connection,
            connection.object_server(),
            sender,
            handle,
            |handle, reply| Message::PortalPickColor { handle, reply },
        )
        .await
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
    /// Where the bus's messages for the window's thread go.
    pub fn sender(&self) -> smol::channel::Sender<Message> {
        self.tx.clone()
    }

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
    pub fn notify(&self, notice: &Notice, file: Option<&Path>, image: NoticeImage<'_>) -> bool {
        let mut actions: Vec<&str> = Vec::new();
        if file.is_some() {
            actions.extend(["default", ""]);
        }
        if let Some(label) = notice.show_in_files {
            actions.extend(["show-in-files", label]);
        }
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        // Not transient: the notification stays in the list with its "Show in
        // Files", as GNOME's does once its banner has gone.
        hints.insert("desktop-entry", Value::from("io.betteros.Screenshot"));
        let image_path = match image {
            NoticeImage::Path(path) => Some(path.to_string_lossy().into_owned()),
            _ => None,
        };
        if let Some(path) = &image_path {
            hints.insert("image-path", Value::from(path.as_str()));
        }
        if let NoticeImage::Pixels(frame) = image {
            hints.insert("image-data", image_data(frame));
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
                listen(&connection, id, &path, &tx, NOTIFICATION_PATIENCE);
                status.notifications.fetch_sub(1, Ordering::SeqCst);
            });
        true
    }
}

/// Follows the session's lock through logind on the system bus: `Lock`
/// (the request to lock, which `loginctl lock-session` and the idle daemon
/// send and the locker answers) and `LockedHint`. The session is found as
/// logind's `auto` session (the caller's, or the user's graphical one), so
/// a process started by D-Bus activation outside the session's scope still
/// finds it. Without a system bus or logind nothing is followed.
pub fn watch_session_lock(tx: smol::channel::Sender<Message>) {
    let _ = std::thread::Builder::new()
        .name("session-lock".into())
        .spawn(move || {
            let Ok(system) = Connection::system() else {
                return;
            };
            let session: Result<OwnedObjectPath, _> = system
                .call_method(
                    Some("org.freedesktop.login1"),
                    "/org/freedesktop/login1",
                    Some("org.freedesktop.login1.Manager"),
                    "GetSession",
                    &("auto",),
                )
                .and_then(|reply| reply.body().deserialize());
            let Ok(session) = session else { return };
            let hint: Result<OwnedValue, _> = system
                .call_method(
                    Some("org.freedesktop.login1"),
                    session.as_str(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &("org.freedesktop.login1.Session", "LockedHint"),
                )
                .and_then(|reply| reply.body().deserialize());
            if let Ok(hint) = hint
                && let Ok(locked) = bool::try_from(&hint)
            {
                let _ = tx.send_blocking(Message::LockedHint(locked));
            }
            let Ok(rule) = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.login1")
                .and_then(|rule| rule.path(session.as_str()))
                .map(|rule| rule.build())
            else {
                return;
            };
            let Ok(messages) = zbus::blocking::MessageIterator::for_match_rule(rule, &system, None)
            else {
                return;
            };
            for message in messages {
                let Ok(message) = message else { return };
                let header = message.header();
                let member = header.member().map(|member| member.as_str().to_string());
                let sent = match member.as_deref() {
                    Some("Lock") => tx.send_blocking(Message::SessionLocking),
                    Some("PropertiesChanged") => {
                        let changed: Result<(String, HashMap<String, OwnedValue>, Vec<String>), _> =
                            message.body().deserialize();
                        match changed
                            .ok()
                            .and_then(|(_, values, _)| values.get("LockedHint").cloned())
                            .and_then(|value| bool::try_from(&value).ok())
                        {
                            Some(locked) => tx.send_blocking(Message::LockedHint(locked)),
                            None => Ok(()),
                        }
                    }
                    _ => Ok(()),
                };
                if sent.is_err() {
                    return;
                }
            }
        });
}

/// How long a notification's buttons keep the process: a notification
/// service that never says the notification closed does not keep it forever.
pub const NOTIFICATION_PATIENCE: Duration = Duration::from_secs(15 * 60);

/// Waits for this notification's presses until it closes or `patience`
/// has passed, whether or not any signal arrives meanwhile.
pub fn listen(
    connection: &Connection,
    id: u32,
    path: &Path,
    tx: &smol::channel::Sender<Message>,
    patience: Duration,
) {
    let deadline = Instant::now() + patience;
    let rule = match zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.Notifications")
        .and_then(|rule| rule.path("/org/freedesktop/Notifications"))
    {
        Ok(rule) => rule.build(),
        Err(_) => return,
    };
    let connection = connection.inner().clone();
    smol::block_on(async {
        let Ok(mut messages) = zbus::MessageStream::for_match_rule(rule, &connection, None).await
        else {
            return;
        };
        loop {
            let next = async { messages.next().await };
            let timeout = async {
                smol::Timer::at(deadline).await;
                None
            };
            let Some(Ok(message)) = smol::future::or(next, timeout).await else {
                return;
            };
            let header = message.header();
            match header.member().map(|member| member.as_str()) {
                Some("ActionInvoked") => {
                    if let Ok((which, key)) = message.body().deserialize::<(u32, String)>()
                        && which == id
                    {
                        let _ = tx
                            .send(Message::NotificationAction {
                                key,
                                path: path.to_path_buf(),
                            })
                            .await;
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
    });
}

/// What a notification shows as its image.
pub enum NoticeImage<'a> {
    None,
    /// The saved file (`image-path`).
    Path(&'a Path),
    /// Pixels, when nothing was saved (`image-data`), as GNOME's
    /// notification shows the picture either way (2315–2340).
    Pixels(&'a screenshot_core::compose::Frame),
}

/// The notification specification's `image-data`: `(iiibiiay)` of width,
/// height, row stride, has alpha, bits per sample, channels, and RGBA bytes,
/// at most 256 pixels on the longer side.
pub fn image_data(frame: &screenshot_core::compose::Frame) -> Value<'static> {
    let small = frame.thumbnail(256);
    Value::from((
        small.width as i32,
        small.height as i32,
        (small.width * 4) as i32,
        true,
        8i32,
        4i32,
        small.to_rgba(),
    ))
}

/// "Show in Files": the file selected in its folder, by the person's file
/// manager over `org.freedesktop.FileManager1` (what Better Git does,
/// `crates/git-gui/src/file_manager.rs`; ticket 301 folds the two); without
/// one, its folder through the portal's `OpenDirectory` with the file's
/// descriptor (xdg-desktop-portal 1.18 refuses a `file:` URI to `OpenURI`,
/// `src/open-uri.c` 441–444).
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
        Err(_) => open_through_portal(connection, path, "OpenDirectory"),
    }
}

/// The portal's `OpenFile` (the file with its default application) or
/// `OpenDirectory` (its folder), given the file's descriptor.
pub fn open_through_portal(
    connection: &Connection,
    path: &Path,
    method: &str,
) -> Result<(), String> {
    use std::os::fd::AsFd;
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    connection
        .call_method(
            Some(PORTAL_FRONTEND),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.OpenURI"),
            method,
            &("", zbus::zvariant::Fd::from(file.as_fd()), options),
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

    struct Bus(std::process::Child);

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A private bus with no service directory, so nothing installed can be
    /// activated on it.
    fn private_bus(directory: &std::path::Path) -> Result<(Bus, String), String> {
        use std::io::{BufRead, BufReader};
        let config = directory.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\"
 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context=\"default\">
    <allow send_destination=\"*\" eavesdrop=\"true\"/>
    <allow eavesdrop=\"true\"/>
    <allow own=\"*\"/>
  </policy>
</busconfig>
",
                directory.display()
            ),
        )
        .map_err(|error| error.to_string())?;
        let mut child = std::process::Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut line = String::new();
        BufReader::new(child.stdout.take().ok_or("no output")?)
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        Ok((Bus(child), line.trim().to_string()))
    }

    fn connect(address: &str) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(address)
            .unwrap()
            .build()
            .unwrap()
    }

    /// The listener for a notification nobody closes, on a bus where no
    /// signal ever comes, still ends at its deadline.
    #[test]
    fn a_notification_listener_ends_at_its_deadline_without_any_signal() {
        let scratch = tempfile::tempdir().unwrap();
        let (_bus, address) =
            test_prerequisites::require!("dbus-daemon", private_bus(scratch.path()));
        let connection = connect(&address);
        let (tx, _rx) = smol::channel::unbounded();
        let started = std::time::Instant::now();
        super::listen(
            &connection,
            7,
            std::path::Path::new("/nowhere.png"),
            &tx,
            std::time::Duration::from_millis(300),
        );
        let took = started.elapsed();
        assert!(took >= std::time::Duration::from_millis(300), "{took:?}");
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    }

    fn portal_server(
        address: &str,
    ) -> (
        zbus::blocking::Connection,
        smol::channel::Receiver<super::Message>,
    ) {
        let (tx, rx) = smol::channel::unbounded();
        let server = zbus::blocking::connection::Builder::address(address)
            .unwrap()
            .serve_at(super::PORTAL_PATH, super::Portal { tx })
            .unwrap()
            .name(super::PORTAL_BUS_NAME)
            .unwrap()
            .build()
            .unwrap();
        (server, rx)
    }

    fn screenshot_call(handle: &str) -> zbus::Message {
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        zbus::Message::method_call(super::PORTAL_PATH, "Screenshot")
            .unwrap()
            .destination(super::PORTAL_BUS_NAME)
            .unwrap()
            .interface("org.freedesktop.impl.portal.Screenshot")
            .unwrap()
            .build(&(
                zbus::zvariant::ObjectPath::try_from(handle).unwrap(),
                "app",
                "",
                options,
            ))
            .unwrap()
    }

    /// Only the portal frontend's name owner is served: anyone else calling
    /// the backend's name is answered "other" (2) and nothing is captured.
    #[test]
    fn a_caller_that_is_not_the_portal_frontend_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let (_bus, address) =
            test_prerequisites::require!("dbus-daemon", private_bus(scratch.path()));
        let (_server, rx) = portal_server(&address);
        let stranger = connect(&address);
        let reply = stranger.call_method(
            Some(super::PORTAL_BUS_NAME),
            super::PORTAL_PATH,
            Some("org.freedesktop.impl.portal.Screenshot"),
            "Screenshot",
            &(
                zbus::zvariant::ObjectPath::try_from("/r/stranger").unwrap(),
                "app",
                "",
                HashMap::<&str, Value<'_>>::new(),
            ),
        );
        let (code, results): (u32, HashMap<String, OwnedValue>) =
            reply.unwrap().body().deserialize().unwrap();
        assert_eq!(code, 2);
        assert!(results.is_empty());
        assert!(
            rx.try_recv().is_err(),
            "a stranger's call reached the window"
        );
    }

    /// The frontend's call reaches the window; when the frontend leaves the
    /// bus before the answer, the request is closed (GNOME's
    /// `_onNameVanished`).
    #[test]
    fn a_frontend_that_leaves_the_bus_closes_its_request() {
        let scratch = tempfile::tempdir().unwrap();
        let (_bus, address) =
            test_prerequisites::require!("dbus-daemon", private_bus(scratch.path()));
        let (_server, rx) = portal_server(&address);
        let frontend = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .name(super::PORTAL_FRONTEND)
            .unwrap()
            .build()
            .unwrap();
        zbus::block_on(frontend.inner().send(&screenshot_call("/r/one"))).unwrap();
        let first = smol::block_on(async {
            smol::future::or(async { rx.recv().await.ok() }, async {
                smol::Timer::after(std::time::Duration::from_secs(5)).await;
                None
            })
            .await
        });
        let reply = match first {
            Some(super::Message::PortalScreenshot { handle, reply, .. }) => {
                assert_eq!(handle, "/r/one");
                reply
            }
            _ => panic!("the frontend's call did not reach the window"),
        };
        // The frontend's process ends: its connection closes.
        zbus::block_on(frontend.into_inner().close()).unwrap();
        let closed = smol::block_on(async {
            smol::future::or(async { rx.recv().await.ok() }, async {
                smol::Timer::after(std::time::Duration::from_secs(5)).await;
                None
            })
            .await
        });
        match closed {
            Some(super::Message::PortalClose { handle }) => assert_eq!(handle, "/r/one"),
            _ => panic!("the request was not closed when the frontend left"),
        }
        drop(reply);
    }
}
