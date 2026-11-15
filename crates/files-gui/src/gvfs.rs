//! Mounting a server through GVfs's own bus interface (ticket 117), as
//! GIO does for Nautilus: `org.gtk.vfs.MountTracker.MountLocation` on the
//! session bus, with a mount operation of Better Files' own exported for
//! GVfs to ask its questions on.
//!
//! The signatures were read off the installed GVfs (1.54): the tracker by
//! introspecting `gvfsd` on a private bus, and `org.gtk.vfs.MountOperation`
//! (a client-side interface GVfs only calls) from the method signatures
//! compiled into `libgvfscommon.so`, then checked by mounting a local
//! `sshd` through a private `gvfsd`:
//!
//! - `MountLocation((aya{sv}) mount_spec, (so) mount_source)`: the spec's
//!   values are NUL-terminated byte strings; the source is this connection's
//!   unique name and the operation's object path. It returns when the
//!   mount is done or failed.
//! - `AskPassword(s message, s default_user, s default_domain, u flags)
//!   -> (b handled, b aborted, s password, s username, s domain,
//!   b anonymous, u password_save)`; asked again after a wrong password.
//! - `AskQuestion(s message, as choices) -> (b handled, b aborted, u choice)`
//!   (an SFTP host key it does not know); `ShowProcesses` likewise with the
//!   processes in the way; `ShowUnmountProgress` and `Aborted` tell.
//! - `LookupMount((aya{sv})) -> (sossssssbay(aya{sv})ay)`: the tenth field is
//!   the mount's folder under GVfs's FUSE mount, the last its default
//!   folder inside it.
//!
//! Errors come back as GIO's (`…UnmappedGError.Quark._g_2dio_2derror_2dquark.
//! Code<n>`): 30 the person cancelled, 17 already mounted, 14 refused, 24
//! timed out, 28 host not found, 37–39 not reachable or refused, 15 not
//! supported; a changed SSH host key is code 0 with GVfs's "Host key
//! verification failed".

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::network::{
    Answer, Asker, MOUNT_TIMEOUT, MountError, MountFailure, Prompt, SignInFields, percent_decode,
};

pub const DAEMON: &str = "org.gtk.vfs.Daemon";
pub const TRACKER_PATH: &str = "/org/gtk/vfs/mounttracker";

/// How long the tracker's call may take in all: GIO's own limit, 30
/// minutes, for a person who takes their time at the sign-in.
const CALL_LIMIT: Duration = Duration::from_secs(30 * 60);

/// A mount spec as GVfs takes it: the prefix, and its keys.
type SpecWire = (Vec<u8>, HashMap<String, OwnedValue>);

/// `LookupMount`'s answer.
type MountInfo = (
    String,
    OwnedObjectPath,
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    Vec<u8>,
    SpecWire,
    Vec<u8>,
);

#[zbus::proxy(
    interface = "org.gtk.vfs.MountTracker",
    default_service = "org.gtk.vfs.Daemon",
    default_path = "/org/gtk/vfs/mounttracker"
)]
trait MountTracker {
    fn mount_location(
        &self,
        mount_spec: &(Vec<u8>, HashMap<String, Value<'_>>),
        mount_source: &(&str, ObjectPath<'_>),
    ) -> zbus::Result<()>;

    fn lookup_mount(
        &self,
        mount_spec: &(Vec<u8>, HashMap<String, Value<'_>>),
    ) -> zbus::Result<MountInfo>;
}

/// What GVfs is asked to mount: the spec, and the folder inside the mount
/// the address names (`None`: GVfs's default folder for it).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountSpec {
    pub keys: Vec<(String, String)>,
    pub path: Option<String>,
}

impl MountSpec {
    fn wire(&self) -> (Vec<u8>, HashMap<String, Value<'static>>) {
        let bytes = |text: &str| {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            bytes
        };
        (
            bytes("/"),
            self.keys
                .iter()
                .map(|(key, value)| (key.clone(), Value::from(bytes(value))))
                .collect(),
        )
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value.as_str())
    }
}

/// The GVfs mount spec for a server address, as GIO's URI mappers build it:
/// `sftp`, `ftp` and `nfs` by host, port and user; `smb` by server and share
/// (`domain;user@`), or the server alone; `dav`/`davs` with `ssl`; `afp`
/// by server and volume.
pub fn mount_spec(address: &str) -> Result<MountSpec, MountError> {
    let unknown = || MountError::new(MountFailure::NotSupported, address.to_string());
    let (scheme, rest) = address.split_once("://").ok_or_else(unknown)?;
    let scheme = scheme.to_ascii_lowercase();
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };
    let (host, port) = match hostport.strip_prefix('[') {
        Some(bracketed) => {
            let (host, after) = bracketed.split_once(']').ok_or_else(unknown)?;
            (host, after.strip_prefix(':'))
        }
        None => match hostport.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (hostport, None),
        },
    };
    if host.is_empty() {
        return Err(unknown());
    }
    let host = percent_decode(host);
    let port = port.filter(|port| !port.is_empty()).map(str::to_string);
    let (mut user, mut domain) = (userinfo.map(percent_decode), None);
    if scheme == "smb"
        && let Some(both) = &user
        && let Some((given_domain, given_user)) = both.split_once(';')
    {
        domain = Some(given_domain.to_string());
        user = Some(given_user.to_string());
    }
    let segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(percent_decode)
        .collect();
    let whole =
        |segments: &[String]| (!segments.is_empty()).then(|| format!("/{}", segments.join("/")));
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut put = |key: &str, value: Option<String>| {
        if let Some(value) = value {
            keys.push((key.to_string(), value));
        }
    };
    let inside = match scheme.as_str() {
        "sftp" | "ftp" | "nfs" => {
            put("type", Some(scheme.clone()));
            put("host", Some(host));
            put("port", port);
            put("user", user);
            whole(&segments)
        }
        "smb" => match segments.split_first() {
            Some((share, rest)) => {
                put("type", Some("smb-share".into()));
                put("server", Some(host));
                put("share", Some(share.clone()));
                put("port", port);
                put("user", user);
                put("domain", domain);
                whole(rest)
            }
            None => {
                put("type", Some("smb-server".into()));
                put("server", Some(host));
                None
            }
        },
        "dav" | "davs" => {
            put("type", Some("dav".into()));
            put("host", Some(host));
            put("ssl", Some((scheme == "davs").to_string()));
            put("port", port);
            put("user", user);
            whole(&segments)
        }
        "afp" => match segments.split_first() {
            Some((volume, rest)) => {
                put("type", Some("afp-volume".into()));
                put("host", Some(host));
                put("volume", Some(volume.clone()));
                put("user", user);
                whole(rest)
            }
            None => {
                put("type", Some("afp-server".into()));
                put("host", Some(host));
                None
            }
        },
        _ => return Err(unknown()),
    };
    Ok(MountSpec { keys, path: inside })
}

/// What a GIO error from GVfs means.
pub fn failure_of(error: &zbus::Error) -> (MountFailure, String) {
    let zbus::Error::MethodError(name, detail, _) = error else {
        return (MountFailure::Failed, error.to_string());
    };
    let detail = detail.clone().unwrap_or_default();
    let name = name.as_str();
    if name.ends_with(".ServiceUnknown") || name.ends_with(".NameHasNoOwner") {
        return (MountFailure::NoGvfs, detail);
    }
    if name.ends_with(".NoReply") || name.ends_with(".Timeout") {
        return (MountFailure::TimedOut, detail);
    }
    let code = name
        .rsplit_once(".Code")
        .filter(|_| name.contains("_g_2dio_2derror_2dquark"))
        .and_then(|(_, code)| code.parse::<u32>().ok());
    let failure = match code {
        Some(30 | 19) => MountFailure::Cancelled,
        Some(14) => MountFailure::PermissionDenied,
        Some(24) => MountFailure::TimedOut,
        Some(28) => MountFailure::HostNotFound,
        Some(37..=39) => MountFailure::Refused,
        Some(15) => MountFailure::NotSupported,
        _ if is_host_key_failure(&detail) => MountFailure::HostKeyChanged,
        _ => MountFailure::Failed,
    };
    (failure, detail)
}

/// GVfs's words for a refused SSH host key, in English.
const HOST_KEY_FAILED: &str = "Host key verification failed";

/// Whether GVfs's message is its "host key verification failed", which it
/// sends in the desktop's language (code 0 says nothing more): the English
/// words, or GVfs's own translation of them for this session.
pub fn is_host_key_failure(detail: &str) -> bool {
    detail.contains(HOST_KEY_FAILED)
        || gvfs_translation(HOST_KEY_FAILED).is_some_and(|words| detail.contains(&words))
}

unsafe extern "C" {
    fn dgettext(domain: *const libc::c_char, message: *const libc::c_char) -> *const libc::c_char;
    fn bind_textdomain_codeset(
        domain: *const libc::c_char,
        codeset: *const libc::c_char,
    ) -> *const libc::c_char;
}

/// `message` in GVfs's own catalog for this session's language (the one
/// `gvfsd` runs in), when there is a translation.
pub fn gvfs_translation(message: &str) -> Option<String> {
    gvfs_translation_in(message, c"")
}

/// `message` in GVfs's catalog for `locale` (`""`: the environment's).
/// Only this thread's locale is switched for the lookup, then put back; the
/// catalog is read as UTF-8 whatever the locale's own character set, so
/// Chinese comes back as Chinese and not as question marks.
pub fn gvfs_translation_in(message: &str, locale: &std::ffi::CStr) -> Option<String> {
    static UTF8: std::sync::Once = std::sync::Once::new();
    let id = std::ffi::CString::new(message).ok()?;
    let domain = c"gvfs";
    // SAFETY: `bind_textdomain_codeset` gets two NUL-terminated strings
    // that live for the program (it copies them) and only affects the
    // "gvfs" domain, which nothing else in this process uses. `newlocale`
    // builds a new locale object (null on failure, checked); `uselocale`
    // switches only this thread and is switched back before the object is
    // freed; `dgettext` gets NUL-terminated strings that outlive the call
    // and returns a NUL-terminated string owned by the catalog (or `id`
    // itself), copied before anything else runs.
    unsafe {
        UTF8.call_once(|| {
            bind_textdomain_codeset(domain.as_ptr(), c"UTF-8".as_ptr());
        });
        let wanted = libc::newlocale(
            libc::LC_MESSAGES_MASK | libc::LC_CTYPE_MASK,
            locale.as_ptr(),
            std::ptr::null_mut(),
        );
        if wanted.is_null() {
            return None;
        }
        let previous = libc::uselocale(wanted);
        let found = dgettext(domain.as_ptr(), id.as_ptr());
        let words = (!found.is_null()).then(|| {
            std::ffi::CStr::from_ptr(found)
                .to_string_lossy()
                .into_owned()
        });
        libc::uselocale(previous);
        libc::freelocale(wanted);
        words.filter(|words| words != message && !words.is_empty())
    }
}

/// Whether GVfs said the location was mounted already (code 17).
fn already_mounted(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, ..)
        if name.as_str().ends_with("_g_2dio_2derror_2dquark.Code17"))
}

/// When the mount last had news, and whether it is waiting on the person
/// (which never counts against [`GvfsMounter::quiet_limit`]).
#[derive(Default)]
struct Activity {
    last: Mutex<Option<Instant>>,
    waiting: AtomicBool,
}

impl Activity {
    fn touch(&self) {
        if let Ok(mut last) = self.last.lock() {
            *last = Some(Instant::now());
        }
    }

    fn quiet_for(&self) -> Duration {
        self.last
            .lock()
            .ok()
            .and_then(|last| *last)
            .map_or(Duration::ZERO, |last| last.elapsed())
    }
}

/// The answer to a sign-in GVfs did not get to ask: aborted.
type PasswordReply = (bool, bool, String, String, String, bool, u32);
const PASSWORD_ABORTED: fn() -> PasswordReply = || {
    (
        true,
        true,
        String::new(),
        String::new(),
        String::new(),
        false,
        0,
    )
};

/// Who may ask a mount's questions. GVfs's backends (`gvfsd-sftp`,
/// `gvfsd-smb`…) call the client's mount operation themselves, on their own
/// session-bus connection (`common/gmountsource.c`: a proxy made with
/// `gvfs_dbus_mount_operation_proxy_new_for_bus_sync (G_BUS_TYPE_SESSION,
/// …)` from the backend's process), and `gvfsd`, which owns
/// `org.gtk.vfs.Daemon`, spawns every backend. So a caller is GVfs when its
/// process is `gvfsd`'s or a descendant of it; anything else on the session
/// bus is refused without showing the person anything.
async fn from_gvfs(connection: &zbus::Connection, header: &zbus::message::Header<'_>) -> bool {
    let Some(sender) = header.sender() else {
        return false;
    };
    let Ok(bus) = zbus::fdo::DBusProxy::new(connection).await else {
        return false;
    };
    let Ok(daemon) = zbus::names::BusName::try_from(DAEMON) else {
        return false;
    };
    let Ok(owner) = bus.get_name_owner(daemon).await else {
        return false;
    };
    let pid_of = |name: zbus::names::BusName<'static>| {
        let bus = bus.clone();
        async move { bus.get_connection_unix_process_id(name).await.ok() }
    };
    let (Some(caller), Some(gvfsd)) = (
        pid_of(zbus::names::BusName::Unique(sender.to_owned())).await,
        pid_of(zbus::names::BusName::Unique(owner.into_inner())).await,
    ) else {
        return false;
    };
    descends_from(caller, gvfsd)
}

/// Whether process `pid` is `ancestor` or below it, by the parents
/// `/proc` names (a few generations at most).
pub fn descends_from(pid: u32, ancestor: u32) -> bool {
    let mut current = pid;
    for _ in 0..16 {
        if current == ancestor {
            return true;
        }
        let Some(parent) = parent_of(current) else {
            return false;
        };
        if parent <= 1 || parent == current {
            return false;
        }
        current = parent;
    }
    false
}

fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (comm) state ppid …`; `comm` may hold spaces and parentheses.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// The mount operation GVfs asks its questions on, at a path nobody can
/// guess, answering only GVfs ([`from_gvfs`]).
struct Operation {
    asker: Asker,
    passwords_asked: AtomicU32,
    activity: Arc<Activity>,
    /// Set by GVfs's `Aborted`: the question on screen goes away.
    abort: (smol::channel::Sender<()>, smol::channel::Receiver<()>),
    /// The mount is over (given up on): anything asked now is aborted.
    closed: Arc<AtomicBool>,
}

impl Operation {
    async fn ask(&self, prompt: Prompt) -> Answer {
        if self.closed.load(Ordering::Acquire) {
            return Answer::Cancel;
        }
        // A stale abort from an earlier question does not end this one.
        while self.abort.1.try_recv().is_ok() {}
        self.activity.waiting.store(true, Ordering::Release);
        let answer = smol::future::or(self.asker.ask(prompt), async {
            let _ = self.abort.1.recv().await;
            // The window's end of the question is dropped with this
            // future, and it takes the question off the screen.
            Answer::Cancel
        })
        .await;
        self.activity.waiting.store(false, Ordering::Release);
        self.activity.touch();
        answer
    }
}

#[zbus::interface(name = "org.gtk.vfs.MountOperation")]
impl Operation {
    async fn ask_password(
        &self,
        message: String,
        default_user: String,
        default_domain: String,
        flags: u32,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> PasswordReply {
        if !from_gvfs(connection, &header).await {
            return PASSWORD_ABORTED();
        }
        let again = self.passwords_asked.fetch_add(1, Ordering::AcqRel) > 0;
        let answer = self
            .ask(Prompt::SignIn {
                message,
                user: default_user,
                domain: default_domain,
                fields: SignInFields::from_bits(flags),
                again,
            })
            .await;
        match answer {
            Answer::SignIn {
                user,
                domain,
                password,
                anonymous,
                remember,
            } => (
                true,
                false,
                // The only copy outside the `Secret`: the bus reply.
                password.expose().to_string(),
                user,
                domain,
                anonymous,
                remember.as_bits(),
            ),
            Answer::Choice(_) | Answer::Cancel => PASSWORD_ABORTED(),
        }
    }

    async fn ask_question(
        &self,
        message: String,
        choices: Vec<String>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> (bool, bool, u32) {
        if !from_gvfs(connection, &header).await {
            return (true, true, 0);
        }
        self.choose(message, choices).await
    }

    async fn show_processes(
        &self,
        message: String,
        choices: Vec<String>,
        _processes: Vec<i32>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> (bool, bool, u32) {
        if !from_gvfs(connection, &header).await {
            return (true, true, 0);
        }
        self.choose(message, choices).await
    }

    async fn show_unmount_progress(
        &self,
        _message: String,
        _time_left: i64,
        _bytes_left: i64,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) {
        if from_gvfs(connection, &header).await {
            self.activity.touch();
        }
    }

    async fn aborted(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) {
        if from_gvfs(connection, &header).await {
            let _ = self.abort.0.try_send(());
        }
    }
}

impl Operation {
    async fn choose(&self, message: String, choices: Vec<String>) -> (bool, bool, u32) {
        match self.ask(Prompt::Question { message, choices }).await {
            Answer::Choice(choice) => (true, false, choice),
            Answer::SignIn { .. } | Answer::Cancel => (true, true, 0),
        }
    }
}

/// Counts operations, for the never-used fallback below.
static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);

/// A path for one mount's operation that another program on the bus cannot
/// guess: 128 random bits.
fn operation_path() -> String {
    use std::io::Read;
    let mut token = [0u8; 16];
    let random = std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut token))
        .is_ok();
    if !random {
        // Never reached on Linux; still unique, and the caller check stands.
        let fallback = NEXT_OPERATION.fetch_add(1, Ordering::Relaxed) as u128
            ^ (std::process::id() as u128) << 64
            ^ Instant::now().elapsed().as_nanos();
        token = fallback.to_le_bytes();
    }
    let hex: String = token.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("/org/betteros/Files/MountOperation/op_{hex}")
}

/// Keeps a finished mount's operation answering "aborted" for this long,
/// for a GVfs backend that is still asking after the mount was given up on.
const CLOSED_GRACE: Duration = Duration::from_secs(120);

/// Where [`Listening`] is served.
const LISTENING_PATH: &str = "/org/betteros/Files";

/// Served from the moment the connection is made, so zbus's object server
/// is already taking calls when GVfs first asks on a mount operation added
/// later: zbus starts a connection's object server lazily, and a call that
/// arrives before it listens is never answered (GVfs's question would hang
/// until the mount is given up on). It has nothing to offer.
struct Listening;

#[zbus::interface(name = "org.betteros.Files.Listening")]
impl Listening {}

/// Mounts through GVfs on the session bus (or a private bus, in tests).
pub struct GvfsMounter {
    connection: zbus::Connection,
    /// How long a mount may go without news before it is given up on.
    pub quiet_limit: Duration,
}

impl GvfsMounter {
    /// The session's GVfs, when it is on the bus or can be started there.
    pub fn session() -> Option<Self> {
        smol::block_on(async {
            let connection = zbus::connection::Builder::session()
                .ok()?
                .method_timeout(CALL_LIMIT)
                .serve_at(LISTENING_PATH, Listening)
                .ok()?
                .build()
                .await
                .ok()?;
            Self::reachable(connection).await
        })
    }

    /// GVfs on the bus at `address`.
    pub fn at(address: &str) -> Option<Self> {
        smol::block_on(async {
            let connection = zbus::connection::Builder::address(address)
                .ok()?
                .method_timeout(CALL_LIMIT)
                .serve_at(LISTENING_PATH, Listening)
                .ok()?
                .build()
                .await
                .ok()?;
            Self::reachable(connection).await
        })
    }

    async fn reachable(connection: zbus::Connection) -> Option<Self> {
        let bus = zbus::fdo::DBusProxy::new(&connection).await.ok()?;
        let name = zbus::names::BusName::try_from(DAEMON).ok()?;
        let owned = bus.name_has_owner(name.clone()).await.unwrap_or(false);
        let activatable = || async {
            bus.list_activatable_names()
                .await
                .unwrap_or_default()
                .iter()
                .any(|known| known.as_str() == DAEMON)
        };
        (owned || activatable().await).then_some(Self {
            connection,
            quiet_limit: MOUNT_TIMEOUT,
        })
    }

    /// The folder GVfs shows the mount under, with the address's own folder
    /// in it.
    async fn local_folder(
        &self,
        tracker: &MountTrackerProxy<'_>,
        spec: &MountSpec,
    ) -> Result<PathBuf, MountError> {
        let info = tracker.lookup_mount(&spec.wire()).await.map_err(|error| {
            let (failure, detail) = failure_of(&error);
            MountError::new(failure, detail)
        })?;
        let text = |bytes: &[u8]| {
            String::from_utf8_lossy(bytes)
                .trim_end_matches('\0')
                .to_string()
        };
        let fuse = text(&info.9);
        if fuse.is_empty() {
            return Err(MountError::new(MountFailure::NoLocalFolder, ""));
        }
        let inside = spec.path.clone().unwrap_or_else(|| text(&info.11));
        let inside = inside.trim_start_matches('/');
        let mut folder = PathBuf::from(fuse);
        if !inside.is_empty() {
            folder.push(inside);
        }
        Ok(folder)
    }

    async fn mount_async(&self, address: &str, asker: &Asker) -> Result<PathBuf, MountError> {
        let spec = mount_spec(address)?;
        let tracker = MountTrackerProxy::new(&self.connection)
            .await
            .map_err(|error| MountError::new(MountFailure::NoGvfs, error.to_string()))?;
        let activity = Arc::new(Activity::default());
        activity.touch();
        let closed = Arc::new(AtomicBool::new(false));
        let path = operation_path();
        let operation = Operation {
            asker: asker.clone(),
            passwords_asked: AtomicU32::new(0),
            activity: Arc::clone(&activity),
            abort: smol::channel::bounded(1),
            closed: Arc::clone(&closed),
        };
        let server = self.connection.object_server();
        server
            .at(path.as_str(), operation)
            .await
            .map_err(|error| MountError::new(MountFailure::Failed, error.to_string()))?;
        let unique = self
            .connection
            .unique_name()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let source_path = ObjectPath::try_from(path.as_str())
            .map_err(|error| MountError::new(MountFailure::Failed, error.to_string()))?;
        let wire = spec.wire();
        let call = async {
            Some(
                tracker
                    .mount_location(&wire, &(unique.as_str(), source_path.clone()))
                    .await,
            )
        };
        // Given up on after `quiet_limit` without news, never while the
        // person is answering.
        let watchdog = async {
            loop {
                smol::Timer::after(Duration::from_millis(200)).await;
                if !activity.waiting.load(Ordering::Acquire)
                    && activity.quiet_for() > self.quiet_limit
                {
                    return None;
                }
            }
        };
        let outcome = smol::future::or(call, watchdog).await;
        if outcome.is_none() {
            // Given up on: GVfs may still ask, and is told "aborted" rather
            // than that nobody is there, for a while.
            closed.store(true, Ordering::Release);
            let connection = self.connection.clone();
            let path = path.clone();
            let _ = std::thread::Builder::new()
                .name("files-mount-closed".to_string())
                .spawn(move || {
                    std::thread::sleep(CLOSED_GRACE);
                    smol::block_on(async {
                        let _ = connection
                            .object_server()
                            .remove::<Operation, _>(path.as_str())
                            .await;
                    });
                });
        } else {
            let _ = server.remove::<Operation, _>(path.as_str()).await;
        }
        match outcome {
            // It may have mounted after all, just slowly.
            None => self
                .local_folder(&tracker, &spec)
                .await
                .map_err(|_| MountError::new(MountFailure::TimedOut, "")),
            Some(Ok(())) => self.local_folder(&tracker, &spec).await,
            Some(Err(error)) if already_mounted(&error) => self.local_folder(&tracker, &spec).await,
            Some(Err(error)) => {
                let (failure, detail) = failure_of(&error);
                Err(MountError::new(failure, detail))
            }
        }
    }
}

impl crate::network::Mounter for GvfsMounter {
    fn mount(&self, address: &str, asker: &Asker) -> Result<PathBuf, MountError> {
        smol::block_on(self.mount_async(address, asker))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(address: &str) -> (Vec<(String, String)>, Option<String>) {
        let spec = mount_spec(address).unwrap();
        (spec.keys, spec.path)
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn an_address_becomes_the_spec_gio_would_send() {
        assert_eq!(
            keys("sftp://ann@build.local:2222/srv/www"),
            (
                pairs(&[
                    ("type", "sftp"),
                    ("host", "build.local"),
                    ("port", "2222"),
                    ("user", "ann")
                ]),
                Some("/srv/www".into())
            )
        );
        assert_eq!(
            keys("sftp://machine.tailnet.ts.net/"),
            (
                pairs(&[("type", "sftp"), ("host", "machine.tailnet.ts.net")]),
                None
            )
        );
        assert_eq!(
            keys("smb://WORK;ann@nas/photos/2024%20trip"),
            (
                pairs(&[
                    ("type", "smb-share"),
                    ("server", "nas"),
                    ("share", "photos"),
                    ("user", "ann"),
                    ("domain", "WORK")
                ]),
                Some("/2024 trip".into())
            )
        );
        assert_eq!(
            keys("smb://nas/"),
            (pairs(&[("type", "smb-server"), ("server", "nas")]), None)
        );
        assert_eq!(
            keys("davs://cloud.example/remote.php/dav").0,
            pairs(&[("type", "dav"), ("host", "cloud.example"), ("ssl", "true")])
        );
        assert_eq!(
            keys("sftp://[fe80::1]:22/").0,
            pairs(&[("type", "sftp"), ("host", "fe80::1"), ("port", "22")])
        );
        assert!(mount_spec("gopher://x/").is_err());
        assert!(mount_spec("sftp:///etc").is_err());
    }

    #[test]
    fn gio_errors_from_gvfs_mean_what_their_codes_say() {
        let error = |code: u32, detail: &str| {
            zbus::Error::MethodError(
                zbus::names::OwnedErrorName::try_from(format!(
                    "org.gtk.GDBus.UnmappedGError.Quark._g_2dio_2derror_2dquark.Code{code}"
                ))
                .unwrap(),
                Some(detail.to_string()),
                zbus::message::Message::method_call("/", "Ping")
                    .unwrap()
                    .build(&())
                    .unwrap(),
            )
        };
        let failure = |code, detail| failure_of(&error(code, detail)).0;
        assert_eq!(
            failure(30, "Password dialog cancelled"),
            MountFailure::Cancelled
        );
        assert_eq!(
            failure(39, "Connection refused by server"),
            MountFailure::Refused
        );
        assert_eq!(
            failure(28, "Hostname not known"),
            MountFailure::HostNotFound
        );
        assert_eq!(
            failure(14, "Permission denied"),
            MountFailure::PermissionDenied
        );
        assert_eq!(failure(24, "Timed out"), MountFailure::TimedOut);
        assert_eq!(
            failure(0, "Host key verification failed"),
            MountFailure::HostKeyChanged
        );
        assert_eq!(failure(0, "Something else"), MountFailure::Failed);
        assert!(already_mounted(&error(17, "Location is already mounted")));
        assert_eq!(
            failure_of(&error(39, "Connection refused by server")).1,
            "Connection refused by server"
        );
    }

    #[test]
    fn a_changed_host_key_is_recognised_in_gvfs_own_chinese() {
        // GVfs's zh_TW catalog, read as UTF-8 whatever the locale's own
        // character set. Needs the zh_TW.UTF-8 locale and GVfs's zh_TW
        // translation (Ubuntu's language pack); without them there is
        // nothing to compare against.
        let Some(words) = gvfs_translation_in(HOST_KEY_FAILED, c"zh_TW.UTF-8") else {
            eprintln!("skipping: no zh_TW.UTF-8 locale or no GVfs zh_TW catalog here");
            return;
        };
        assert_eq!(words, "驗證主機金鑰失敗");
        assert!(
            words.contains("主機金鑰"),
            "read as UTF-8, not mangled: {words:?}"
        );
    }

    #[test]
    fn a_host_key_question_in_another_language_is_still_one() {
        // GVfs's question in Chinese: found by its fingerprint and its two
        // choices, whatever the words.
        let message = "身分核對失敗\n核對「nas」的身分失敗，這通常發生在首次登入該電腦時。\n\n遠端電腦送來的身分是「SHA256:7mj0NZesAHJ2Jo7UyMBrm/zFylxFPMVWG+oDHpeMz1E」。若想要確保繼續作業安全無虞，請聯絡系統管理員。";
        let choices = vec!["仍要登入".to_string(), "取消登入".to_string()];
        assert_eq!(
            crate::network::HostKeyQuestion::read("sftp://nas/", message, &choices),
            Some(crate::network::HostKeyQuestion::Unknown {
                fingerprint: "SHA256:7mj0NZesAHJ2Jo7UyMBrm/zFylxFPMVWG+oDHpeMz1E".into()
            })
        );
        // The address-mismatch question carries no fingerprint: a warning.
        assert_eq!(
            crate::network::HostKeyQuestion::read(
                "sftp://nas/",
                "「nas」的主機金鑰和 IP 位址「192.0.2.1」的金鑰不同",
                &choices
            ),
            Some(crate::network::HostKeyQuestion::Mismatch)
        );
    }

    #[test]
    fn gvfs_s_backends_are_its_descendants_and_nothing_else_is() {
        let me = std::process::id();
        assert!(descends_from(me, me));
        let parent = parent_of(me).expect("a parent");
        assert!(descends_from(me, parent), "a child of the daemon");
        assert!(
            !descends_from(parent, me),
            "the daemon's parent is not its child"
        );
        assert!(!descends_from(1, me));
    }

    #[test]
    fn gio_s_question_is_read_by_its_own_numbers() {
        // `gio mount` against an SSH server it has not seen (C locale), as
        // printed by GVfs 1.54's gio.
        let said = "Identity Verification Failed\nVerifying the identity of \"nas\" failed, this happens when you log in to a computer the first time.\n\nThe identity sent by the remote computer is \"SHA256:i4D6NouSBoL4kCuY1t4/EldVQabVENDr2C/UBM1bO78\". If you want to be absolutely sure it is safe to continue, contact the system administrator.\n[1] Log In Anyway\n[2] Cancel Login\nChoice: \ngio: sftp://nas/: Login dialog cancelled\n";
        let question = crate::network::gio_question(said).expect("a question");
        assert_eq!(
            question.choices,
            [
                ("1".to_string(), "Log In Anyway".to_string()),
                ("2".to_string(), "Cancel Login".to_string())
            ]
        );
        // The window's Connect anyway is GVfs's choice 0, which gio numbers 1.
        assert_eq!(
            question.choices[crate::network::CONNECT_ANYWAY as usize].0,
            "1"
        );
        assert!(
            !question.text.contains("[1]"),
            "the choices are not the text"
        );
        assert_eq!(crate::network::gio_question("Password: \n"), None);
    }
}
