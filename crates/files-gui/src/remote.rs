//! Folders on servers, and the one rule that decides whether a location
//! needs its connection (ticket 310), with no GPUI in it.
//!
//! A server folder is reached through GVfs (ticket 117): the server is
//! mounted, and its files are shown under GVfs's own folder,
//! `$XDG_RUNTIME_DIR/gvfs/<mount>/…`, where `<mount>` is the mount's spec
//! written out (`sftp:host=nas,port=2222,user=ann`). That name is enough to
//! connect again, so a folder Better Files once showed there can always be
//! turned back into an address, whether its mount is still there or not.
//!
//! - [`Remote`] is a server folder by its address: the mount spec GVfs takes
//!   and the folder inside it. It reads an address, a GVfs folder's name, and
//!   writes the address back.
//! - [`need`] is the rule. Every listing goes through it (the reader applies
//!   it), and so does every other place that asks whether a location is
//!   there: the sidebar, a paste, the restart hand-off.
//! - [`MountWatch`] is what GVfs's mount tracker lists, read on a worker
//!   thread with a deadline and kept; nothing on the window's thread waits
//!   on the bus or on a mount.
//! - [`probe`] looks at a remote folder on a thread of its own with a
//!   deadline, once per mount at a time: a mount whose last probe has not
//!   come back is known not to answer.
//! - [`Attempts`] are the connections being made: one per mount, joined by
//!   every tab and click that asks for the same one, each on its own thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use files_core::{LocalPath, Location, NetworkLocation, NetworkScheme};

use crate::network::{Ask, Asker, MountError, MountFailure, Mounter};

/// How long the mount list may take to come back from GVfs's tracker.
pub const MOUNT_LIST_DEADLINE: Duration = Duration::from_secs(2);

/// How long a remote folder may take to answer a `stat` before it is
/// treated as not answering.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(3);

/// How old the mount list may be before it is read again.
pub const MOUNT_LIST_AGE: Duration = Duration::from_secs(3);

/// How long a dropped connection waits between tries.
pub const RETRY_EVERY: Duration = Duration::from_secs(5);

/// GVfs's own folder: `$XDG_RUNTIME_DIR/gvfs`.
pub fn gvfs_root_from_env() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|runtime| runtime.is_absolute())
        .map(|runtime| runtime.join("gvfs"))
}

/// A folder on a server: the mount GVfs makes for it, and where in that
/// mount the folder is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Remote {
    /// The mount spec's keys, `type` among them, sorted by key, without
    /// `prefix`.
    keys: Vec<(String, String)>,
    /// The folder inside the mount, from its root, starting with `/`. `None`
    /// is the mount's default folder (an SFTP address with no path opens
    /// the remote home, as it does in GNOME Files).
    path: Option<String>,
}

impl Remote {
    /// From an address (`sftp://ann@nas:2222/srv`, `smb://nas/photos/2024`).
    pub fn from_address(address: &str) -> Option<Self> {
        let address = crate::network::server_address(address)?;
        let spec = crate::gvfs::mount_spec(&address).ok()?;
        let mut keys: Vec<(String, String)> = spec
            .keys
            .into_iter()
            .filter(|(key, _)| key != "prefix")
            .collect();
        keys.sort();
        // A WebDAV address's whole path is the folder; GVfs mounts it at a
        // prefix it chooses, so the folder is kept as the address had it.
        //
        // An address with no folder (`sftp://nas`) is the server's default
        // folder; one that names the root (`sftp://nas/`) is the root, and
        // stays the root through every form it is written in (ticket 310).
        // GVfs's spec has no folder for either; the address's own last `/`
        // tells them apart.
        let path = spec
            .path
            .or_else(|| address.ends_with('/').then(|| "/".to_string()));
        Some(Self { keys, path })
    }

    /// From a typed location: a server address.
    pub fn from_location(location: &NetworkLocation) -> Option<Self> {
        Self::from_address(&Location::Network(location.clone()).to_uri())
    }

    /// From a GVfs folder's name (`sftp:host=nas,port=2222`) and the path
    /// below it.
    pub fn from_mount_name(name: &str, below: &Path) -> Option<Self> {
        let (kind, rest) = name.split_once(':')?;
        let kind = crate::network::percent_decode(kind);
        let mut keys = vec![("type".to_string(), kind.clone())];
        let mut prefix = String::new();
        for pair in rest.split(',').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=')?;
            let key = crate::network::percent_decode(key);
            let value = crate::network::percent_decode(value);
            if key == "prefix" {
                prefix = value;
            } else if key != "type" {
                keys.push((key, value));
            }
        }
        keys.sort();
        let remote = Self { keys, path: None };
        // Only the kinds a person types are servers this connects to.
        remote.scheme()?;
        remote.get(remote.host_key())?;
        let below = below.to_string_lossy();
        let below = below.trim_matches('/');
        let mut path = prefix.trim_end_matches('/').to_string();
        if !below.is_empty() {
            path.push('/');
            path.push_str(below);
        }
        if path.is_empty() {
            path.push('/');
        }
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        Some(Self {
            path: Some(path),
            ..remote
        })
    }

    /// The server folder a folder under GVfs's folder is, when it is one:
    /// `gvfs_root/<mount name>/<path>`.
    pub fn from_gvfs_path(path: &Path, gvfs_root: &Path) -> Option<Self> {
        let (name, below) = split_gvfs_path(path, gvfs_root)?;
        Self::from_mount_name(&name, &below)
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value.as_str())
    }

    fn kind(&self) -> &str {
        self.get("type").unwrap_or_default()
    }

    fn host_key(&self) -> &'static str {
        match self.kind() {
            "smb-share" | "smb-server" => "server",
            _ => "host",
        }
    }

    /// The scheme the address is written with.
    pub fn scheme(&self) -> Option<NetworkScheme> {
        Some(match self.kind() {
            "sftp" => NetworkScheme::Sftp,
            "ftp" => NetworkScheme::Ftp,
            "nfs" => NetworkScheme::Nfs,
            "smb-share" | "smb-server" => NetworkScheme::Smb,
            "dav" if self.get("ssl") == Some("true") => NetworkScheme::Davs,
            "dav" => NetworkScheme::Dav,
            "afp-volume" | "afp-server" => NetworkScheme::Afp,
            _ => return None,
        })
    }

    /// The server's name, as the pages and the sidebar say it.
    pub fn server(&self) -> String {
        self.get(self.host_key()).unwrap_or_default().to_string()
    }

    /// The SMB share or AFP volume, when the mount is one.
    pub fn share(&self) -> Option<&str> {
        self.get("share").or_else(|| self.get("volume"))
    }

    /// The folder inside the mount (`None`: its default folder).
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// What tells one mount from another: its whole identity
    /// ([`Self::identity`]), so two folders on one server share one
    /// connection, and two users, two ports or two shares on it do not.
    pub fn mount_key(&self) -> String {
        self.identity()
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The mount's identity: every key, the host in lower case, and a port
    /// that is the scheme's own default written as no port (GVfs leaves it
    /// out of the spec). A missing user is "no user given", never "any user"
    /// (ticket 310's review).
    fn identity(&self) -> Vec<(String, String)> {
        let default = self.default_port();
        let host_key = self.host_key();
        let mut identity: Vec<(String, String)> = self
            .keys
            .iter()
            .filter(|(key, value)| !(key == "port" && Some(value.as_str()) == default))
            .map(|(key, value)| {
                let value = if key == host_key {
                    value.to_ascii_lowercase()
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .collect();
        identity.sort();
        identity
    }

    /// The port a scheme uses when none is given.
    fn default_port(&self) -> Option<&'static str> {
        Some(match self.kind() {
            "sftp" => "22",
            "ftp" => "21",
            "smb-share" | "smb-server" => "445",
            "dav" if self.get("ssl") == Some("true") => "443",
            "dav" => "80",
            "afp-volume" | "afp-server" => "548",
            "nfs" => "2049",
            _ => return None,
        })
    }

    /// The port to reach the server on: the address's, or the scheme's.
    pub fn port(&self) -> Option<u16> {
        self.get("port")
            .or(self.default_port())
            .and_then(|port| port.parse().ok())
    }

    /// Whether a mount GVfs lists is this one's: the same identity, both
    /// ways. A mount on the same host with another port, another user,
    /// another share or another domain is another mount.
    pub fn is_mounted_as(&self, mounted: &Remote) -> bool {
        self.identity() == mounted.identity()
    }

    /// The same mount, at another folder in it.
    pub fn at(&self, path: Option<String>) -> Self {
        Self {
            keys: self.keys.clone(),
            path,
        }
    }

    /// The address of the mount itself, with no folder: what GVfs is asked
    /// to mount.
    pub fn mount_address(&self) -> String {
        self.at(None).address()
    }

    /// The address, written the way GIO reads it back.
    pub fn address(&self) -> String {
        let scheme = self.scheme().map(NetworkScheme::as_str).unwrap_or("sftp");
        let mut authority = String::new();
        if let Some(user) = self.get("user") {
            // GIO's `DOMAIN;user@`: a domain goes with a user only.
            if let Some(domain) = self.get("domain") {
                authority.push_str(&encode(domain, USERINFO));
                authority.push(';');
            }
            authority.push_str(&encode(user, USERINFO));
            authority.push('@');
        }
        let host = self.server();
        if host.contains(':') {
            authority.push('[');
            authority.push_str(&host);
            authority.push(']');
        } else {
            authority.push_str(&encode(&host, HOST));
        }
        if let Some(port) = self.get("port") {
            authority.push(':');
            authority.push_str(port);
        }
        let mut path = String::new();
        if let Some(share) = self.share() {
            path.push('/');
            path.push_str(&encode(share, SEGMENT));
        }
        let mut inside_any = false;
        if let Some(inside) = &self.path {
            for segment in inside.split('/').filter(|segment| !segment.is_empty()) {
                path.push('/');
                path.push_str(&encode(segment, SEGMENT));
                inside_any = true;
            }
        }
        // The root, named, ends with its `/`; no folder at all (the
        // server's default one) has none, so the two never read back as
        // each other.
        if self.path.is_some() && !inside_any {
            path.push('/');
        }
        format!("{scheme}://{authority}{path}")
    }

    /// The address as a location.
    pub fn location(&self) -> Location {
        Location::parse_uri(&self.address())
    }

    /// The folder this is under a mount GVfs shows at `mount_folder`; its
    /// default folder is `default`.
    pub fn local_in(&self, mount_folder: &Path, default: &Path) -> Option<LocalPath> {
        match &self.path {
            None => LocalPath::new(default.to_path_buf()).ok(),
            Some(inside) => {
                let inside = inside.trim_start_matches('/');
                let mut folder = mount_folder.to_path_buf();
                if !inside.is_empty() {
                    folder.push(inside);
                }
                LocalPath::new(folder).ok()
            }
        }
    }

    /// The folder's own name, for "{folder} isn't on {server} any more".
    pub fn folder_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(|path| path.rsplit('/').find(|segment| !segment.is_empty()))
            .map(str::to_string)
            .or_else(|| self.share().map(str::to_string))
            .unwrap_or_else(|| self.server())
    }
}

/// Characters an address keeps as they are, beside letters and digits.
const USERINFO: &[u8] = b"-._~!$&'()*+=";
const HOST: &[u8] = b"-._~";
const SEGMENT: &[u8] = b"-._~!$&'()*+,;=:@";

fn encode(text: &str, keep: &[u8]) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || keep.contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A path under GVfs's folder, split into the mount's name and what is
/// below it.
pub fn split_gvfs_path(path: &Path, gvfs_root: &Path) -> Option<(String, PathBuf)> {
    let rest = path.strip_prefix(gvfs_root).ok()?;
    let mut components = rest.components();
    let name = components
        .next()?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    Some((name, components.as_path().to_path_buf()))
}

/// One mount GVfs's tracker lists: its spec and where it is shown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownMount {
    pub remote: Remote,
    /// The folder GVfs shows it under (`gvfs_root/<name>`); `None` when its
    /// FUSE part is not running.
    pub folder: Option<PathBuf>,
}

/// What the tracker listed, and when.
#[derive(Clone, Debug, Default)]
pub struct Mounts {
    /// `None` until it has been read once (or when GVfs is not there).
    pub known: Option<Vec<KnownMount>>,
    pub read_at: Option<Instant>,
}

impl Mounts {
    /// A list a test or the tracker gives.
    pub fn listed(known: Vec<KnownMount>) -> Self {
        Self {
            known: Some(known),
            read_at: Some(Instant::now()),
        }
    }

    /// The mount a server folder is on, when GVfs lists it.
    pub fn find(&self, remote: &Remote) -> Option<&KnownMount> {
        self.known
            .as_ref()?
            .iter()
            .find(|known| remote.is_mounted_as(&known.remote))
    }

    /// Whether the mount shown at `folder` is listed: `None` when the list
    /// has not been read.
    pub fn has_folder(&self, folder: &Path) -> Option<bool> {
        let known = self.known.as_ref()?;
        Some(
            known
                .iter()
                .any(|mount| mount.folder.as_deref() == Some(folder)),
        )
    }
}

/// Where the mount list comes from. A trait so a test lists what it likes.
pub trait MountsSource: Send + Sync {
    /// The mounts, or `None` when GVfs cannot be asked. Called on a worker
    /// thread; it may take long, and the caller does not wait past
    /// [`MOUNT_LIST_DEADLINE`].
    fn list(&self) -> Option<Vec<KnownMount>>;
}

/// No GVfs: nothing is listed.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMounts;

impl MountsSource for NoMounts {
    fn list(&self) -> Option<Vec<KnownMount>> {
        None
    }
}

/// A fixed list, changed by the test that holds it.
#[derive(Clone, Default)]
pub struct FixedMounts(pub Arc<Mutex<Option<Vec<KnownMount>>>>);

impl FixedMounts {
    pub fn set(&self, known: Option<Vec<KnownMount>>) {
        *self.0.lock().expect("mounts") = known;
    }
}

impl MountsSource for FixedMounts {
    fn list(&self) -> Option<Vec<KnownMount>> {
        self.0.lock().expect("mounts").clone()
    }
}

/// The mount list, read again when it is older than [`MOUNT_LIST_AGE`], on
/// a worker thread, and kept. Reading it never waits.
pub struct MountWatch {
    source: Arc<dyn MountsSource>,
    current: Arc<Mutex<Mounts>>,
    reading: Arc<std::sync::atomic::AtomicBool>,
    /// Counts the readings that came back, so a caller can tell a new one.
    generation: Arc<std::sync::atomic::AtomicU64>,
}

impl MountWatch {
    pub fn new(source: Arc<dyn MountsSource>) -> Self {
        Self {
            source,
            current: Arc::new(Mutex::new(Mounts::default())),
            reading: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// The list as last read.
    pub fn current(&self) -> Mounts {
        self.current.lock().expect("mounts").clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Starts a reading when the list is older than `age` and none is
    /// running. Returns at once.
    pub fn refresh_if_older(&self, age: Duration) {
        let stale = self
            .current
            .lock()
            .expect("mounts")
            .read_at
            .is_none_or(|at| at.elapsed() >= age);
        if stale {
            self.refresh();
        }
    }

    /// Starts a reading now unless one is running. Returns at once.
    pub fn refresh(&self) {
        use std::sync::atomic::Ordering;
        if self.reading.swap(true, Ordering::AcqRel) {
            return;
        }
        let source = Arc::clone(&self.source);
        let current = Arc::clone(&self.current);
        let reading = Arc::clone(&self.reading);
        let generation = Arc::clone(&self.generation);
        // The tracker is asked on a thread of its own: a GVfs that does not
        // answer leaves that thread waiting, never the caller, and the list
        // keeps what it last knew. While it waits no second reading starts,
        // so a GVfs that never answers costs one thread, not one a reading.
        let spawned = std::thread::Builder::new()
            .name("files-mount-list".to_string())
            .spawn(move || {
                let known = source.list();
                let mut mounts = current.lock().expect("mounts");
                mounts.known = known;
                mounts.read_at = Some(Instant::now());
                drop(mounts);
                generation.fetch_add(1, Ordering::AcqRel);
                reading.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.reading.store(false, Ordering::Release);
        }
    }

    /// Waits up to `deadline` for a reading newer than `age`, starting one.
    /// Only for a worker thread (the reader's listing thread) and tests.
    pub fn fresh(&self, age: Duration, deadline: Duration) -> Mounts {
        let started = Instant::now();
        loop {
            let mounts = self.current();
            if mounts.read_at.is_some_and(|at| at.elapsed() < age) {
                return mounts;
            }
            self.refresh_if_older(age);
            if started.elapsed() >= deadline {
                return mounts;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// What a location needs before it can be listed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Need {
    /// Nothing: list it.
    Nothing,
    /// A server address whose mount is there: list this folder instead.
    At(LocalPath),
    /// Its connection is not there: connect to this.
    Connect(Remote),
    /// It is under GVfs's folder with a mount name this does not understand:
    /// its address has to be asked for.
    Address(PathBuf),
}

/// **The rule** (ticket 310): whether `location` needs its connection and
/// whether it is there, from the mount list alone. It never touches the
/// location. `gvfs_root` is GVfs's folder (`None`: GVfs folders are not
/// known here).
///
/// A folder under GVfs's folder whose mount the list does not have needs
/// its connection; when the list has not been read yet, nothing is decided
/// here and the reader's probe ([`probe`]) tells.
pub fn need(location: &Location, gvfs_root: Option<&Path>, mounts: &Mounts) -> Need {
    match location {
        Location::Network(network) => {
            let Some(remote) = Remote::from_location(network) else {
                return Need::Nothing;
            };
            match mounts.find(&remote) {
                Some(KnownMount {
                    folder: Some(folder),
                    ..
                }) if remote.path().is_some() => remote
                    .local_in(folder, folder)
                    .map_or(Need::Connect(remote.clone()), Need::At),
                _ => Need::Connect(remote),
            }
        }
        Location::Local(path) => {
            let Some(root) = gvfs_root else {
                return Need::Nothing;
            };
            let Some((name, _)) = split_gvfs_path(path.as_path(), root) else {
                return Need::Nothing;
            };
            // A device's mount (a phone, a camera) is not a server.
            if !server_shaped(&name) {
                return Need::Nothing;
            }
            match mounts.has_folder(&root.join(&name)) {
                Some(false) => match Remote::from_gvfs_path(path.as_path(), root) {
                    Some(remote) => Need::Connect(remote),
                    None => Need::Address(path.as_path().to_path_buf()),
                },
                _ => Need::Nothing,
            }
        }
        _ => Need::Nothing,
    }
}

/// Whether `location` is on a server: an address, or a folder under GVfs's
/// folder whose mount is a server's ([`server_shaped`]). A phone, a camera
/// or an online account there is not, and keeps the availability any folder
/// has (ticket 310's review).
pub fn is_remote(location: &Location, gvfs_root: Option<&Path>) -> bool {
    match location {
        Location::Network(_) => true,
        Location::Local(path) => gvfs_root
            .and_then(|root| split_gvfs_path(path.as_path(), root))
            .is_some_and(|(name, _)| server_shaped(&name)),
        _ => false,
    }
}

/// GVfs's backends that are servers a person reaches by an address.
const SERVER_KINDS: &[&str] = &[
    "sftp",
    "ftp",
    "smb-share",
    "smb-server",
    "dav",
    "dav+sd",
    "afp-volume",
    "afp-server",
    "nfs",
];

/// Whether a GVfs mount's name (`sftp:host=nas`) is a server's: `mtp:`,
/// `gphoto2:`, `afc:`, `google-drive:` and any kind not known here are not.
pub fn server_shaped(mount_name: &str) -> bool {
    mount_name.split_once(':').is_some_and(|(kind, _)| {
        SERVER_KINDS.contains(&crate::network::percent_decode(kind).as_str())
    })
}

/// The server folder a location is, when it is one.
pub fn remote_of(location: &Location, gvfs_root: Option<&Path>) -> Option<Remote> {
    match location {
        Location::Network(network) => Remote::from_location(network),
        Location::Local(path) => Remote::from_gvfs_path(path.as_path(), gvfs_root?),
        _ => None,
    }
}

/// What a pinned or remembered location is stored as: a folder under GVfs's
/// folder whose mount name is understood becomes its address; anything else
/// stays as it is.
pub fn stored_form(location: &Location, gvfs_root: Option<&Path>) -> Location {
    match location {
        Location::Local(path) => gvfs_root
            .and_then(|root| Remote::from_gvfs_path(path.as_path(), root))
            .map(|remote| remote.location())
            .unwrap_or_else(|| location.clone()),
        other => other.clone(),
    }
}

/// What a probe of a remote folder found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Probed {
    /// A folder: list it.
    Folder,
    /// Something that is not a folder (a file): its listing says so.
    NotFolder,
    /// The mount is there and the folder is not.
    Missing,
    /// The mount is not there.
    NotMounted,
    /// No answer in time, or an answer that means the server is away (an
    /// I/O error, a closed connection).
    NotAnswering,
}

/// Probes in flight, by mount folder, across listings: a mount whose probe
/// has been out past its deadline is not probed again and does not answer.
#[derive(Clone, Default)]
pub struct Probing(Arc<Mutex<std::collections::HashMap<PathBuf, (u64, Instant)>>>);

/// Tells one probe from another, so a probe that comes back late takes out
/// only its own entry.
static PROBES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Probing {
    pub fn is_probing(&self, mount_folder: &Path) -> bool {
        self.0.lock().expect("probes").contains_key(mount_folder)
    }

    /// The mount was made again (ticket 310's review): a probe still stuck on
    /// the dead one no longer speaks for it.
    pub fn forget(&self, mount_folder: &Path) {
        self.0.lock().expect("probes").remove(mount_folder);
    }

    /// A probe that never comes back, for tests.
    #[cfg(test)]
    pub(crate) fn hold(&self, mount_folder: &Path) {
        self.0.lock().expect("probes").insert(
            mount_folder.to_path_buf(),
            (u64::MAX, Instant::now() - Duration::from_secs(3600)),
        );
    }

    #[cfg(test)]
    pub(crate) fn release(&self, mount_folder: &Path) {
        self.forget(mount_folder);
    }
}

/// Looks at `folder`, which is under the mount shown at `mount_folder`, on a
/// thread of its own, and waits at most `deadline` for it. Called on a
/// worker thread, never the window's.
pub fn probe(folder: &Path, mount_folder: &Path, deadline: Duration, probing: &Probing) -> Probed {
    let token = PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut out = probing.0.lock().expect("probes");
        // Another probe of this mount out for longer than the deadline is
        // stuck: the mount does not answer, and no second thread waits on it.
        // One still within its time is just running beside this one.
        if out
            .get(mount_folder)
            .is_some_and(|(_, started)| started.elapsed() >= deadline)
        {
            return Probed::NotAnswering;
        }
        out.insert(mount_folder.to_path_buf(), (token, Instant::now()));
    }
    let (sender, receiver) = channel();
    let (folder, mount) = (folder.to_path_buf(), mount_folder.to_path_buf());
    let set = Arc::clone(&probing.0);
    let spawned = std::thread::Builder::new()
        .name("files-remote-probe".to_string())
        .spawn(move || {
            let found = look(&folder, &mount);
            let mut out = set.lock().expect("probes");
            if out.get(&mount).map(|(known, _)| *known) == Some(token) {
                out.remove(&mount);
            }
            drop(out);
            let _ = sender.send(found);
        });
    if spawned.is_err() {
        probing.0.lock().expect("probes").remove(mount_folder);
        return Probed::NotAnswering;
    }
    receiver
        .recv_timeout(deadline)
        .unwrap_or(Probed::NotAnswering)
}

fn look(folder: &Path, mount: &Path) -> Probed {
    match std::fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => Probed::Folder,
        Ok(_) => Probed::NotFolder,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::metadata(mount) {
                Ok(_) => Probed::Missing,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Probed::NotMounted,
                Err(_) => Probed::NotAnswering,
            }
        }
        // Permission is the folder's own answer: its listing says so.
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Probed::Folder,
        Err(_) => Probed::NotAnswering,
    }
}

/// What a finished connection gives: the folder GVfs shows the mount under,
/// and its default folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mounted {
    pub root: PathBuf,
    pub folder: PathBuf,
}

/// GVfs's own messages for a failure it sends without a code of its own
/// (read from `gvfsd-sftp` 1.54), and what each means. GVfs sends them in
/// the desktop's language, so each is also looked up in its catalog.
const GVFS_WORDS: &[(&str, Reason)] = &[
    ("Connection refused by server", Reason::Refused),
    ("Connection refused", Reason::Refused),
    // When ssh gave up before the server said anything (seen against a
    // stopped sshd in ticket 310's sandbox).
    ("Connection failed", Reason::NoAnswer),
    (
        "The connection is closed (the underlying SSH process exited)",
        Reason::NoAnswer,
    ),
    ("No route to host", Reason::NoAnswer),
    ("Hostname not known", Reason::NameNotFound),
    ("Permission denied", Reason::SignInRefused),
];

/// What ssh, libsmbclient and the kernel say, in the C locale they write it.
const SYSTEM_WORDS: &[(&str, Reason)] = &[
    ("connection refused", Reason::Refused),
    ("timed out", Reason::NoAnswer),
    ("no route", Reason::NoAnswer),
    ("unreachable", Reason::NoAnswer),
    ("connection closed", Reason::NoAnswer),
    ("connection reset", Reason::NoAnswer),
    ("not known", Reason::NameNotFound),
    ("could not resolve", Reason::NameNotFound),
    ("authentication", Reason::SignInRefused),
    ("logon failure", Reason::SignInRefused),
    ("no such", Reason::ShareGone),
    ("not found", Reason::ShareGone),
];

/// Why a connection did not come about, as the page says it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    /// No answer: timed out, unreachable, or a connection that stopped
    /// answering.
    NoAnswer,
    /// Something answered and refused the connection.
    Refused,
    /// The name did not resolve.
    NameNotFound,
    /// The sign-in was refused.
    SignInRefused,
    /// The share or the folder is not there any more on a server that
    /// answered.
    ShareGone,
    /// This computer has no network.
    NoNetwork,
    /// Ticket 117's own reasons, with their own sentences.
    Mount(MountFailure),
}

impl Reason {
    /// The reason for a failed mount. `network_up` says whether this
    /// computer has a route anywhere; a name that did not resolve or a
    /// server that did not answer on a computer with no network is the
    /// network's fault. A server on this computer is never.
    pub fn of(error: &MountError, server: &str, network_up: bool) -> Self {
        Self::of_in(error, server, network_up, &|message| {
            crate::gvfs::gvfs_translation(message)
        })
    }

    /// [`Self::of`], with GVfs's words looked up by `translated` (GVfs sends
    /// a message with no code of its own in the desktop's language).
    pub fn of_in(
        error: &MountError,
        server: &str,
        network_up: bool,
        translated: &dyn Fn(&str) -> Option<String>,
    ) -> Self {
        let local = matches!(server, "localhost" | "127.0.0.1" | "::1");
        let reason = match error.failure {
            MountFailure::TimedOut => Reason::NoAnswer,
            MountFailure::HostNotFound => Reason::NameNotFound,
            MountFailure::Refused => Reason::Refused,
            MountFailure::PermissionDenied => Reason::SignInRefused,
            MountFailure::ShareGone => Reason::ShareGone,
            MountFailure::NetworkDown => Reason::NoNetwork,
            MountFailure::Failed => {
                let detail = error.detail.to_lowercase();
                // GVfs's own messages first, in English or as its catalog
                // has them for this session; then what ssh and the kernel say.
                GVFS_WORDS
                    .iter()
                    .find(|(message, _)| {
                        detail.contains(&message.to_lowercase())
                            || translated(message).is_some_and(|words| {
                                !words.is_empty() && error.detail.contains(&words)
                            })
                    })
                    .map(|(_, reason)| *reason)
                    .or_else(|| {
                        SYSTEM_WORDS
                            .iter()
                            .find(|(words, _)| detail.contains(words))
                            .map(|(_, reason)| *reason)
                    })
                    .unwrap_or(Reason::Mount(MountFailure::Failed))
            }
            other => Reason::Mount(other),
        };
        match reason {
            Reason::NoAnswer | Reason::NameNotFound | Reason::Refused if !network_up && !local => {
                Reason::NoNetwork
            }
            reason => reason,
        }
    }

    /// Whether Network settings is offered beside Try again.
    pub fn offers_network_settings(self) -> bool {
        matches!(
            self,
            Reason::NoAnswer | Reason::NameNotFound | Reason::NoNetwork
        )
    }
}

/// Set in a debug build's sandbox to say this computer has no network.
pub const NO_NETWORK_VARIABLE: &str = "BETTER_FILES_TEST_NO_NETWORK";

/// Whether this computer has a route to anywhere but itself: a default
/// route in the kernel's IPv4 or IPv6 table. A file read, never a network
/// request.
pub fn network_up() -> bool {
    // Debug builds only: a sandbox shows the No-network page without taking
    // the computer's network down (ticket 310's screenshots). A release
    // build never reads it.
    if cfg!(debug_assertions) && std::env::var_os(NO_NETWORK_VARIABLE).is_some() {
        return false;
    }
    let ipv4 = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let ipv4 = ipv4.lines().skip(1).any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        fields.len() > 2 && fields[0] != "lo" && fields[1] == "00000000"
    });
    let ipv6 = std::fs::read_to_string("/proc/net/ipv6_route").unwrap_or_default();
    let ipv6 = ipv6.lines().any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        fields.len() > 9
            && fields[0] == "00000000000000000000000000000000"
            && fields[1] == "00"
            && fields[9] != "lo"
    });
    ipv4 || ipv6
}

/// The program that shows the network settings, from the ones installed:
/// GNOME's, then COSMIC's (the Better OS session).
pub fn network_settings(installed: &dyn Fn(&str) -> bool) -> Option<(String, Vec<String>)> {
    [
        ("gnome-control-center", "network"),
        ("cosmic-settings", "network"),
    ]
    .into_iter()
    .find(|(program, _)| installed(program))
    .map(|(program, page)| (program.to_string(), vec![page.to_string()]))
}

/// What a look at a server's host found, after a failure or before a retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostState {
    /// Its name does not resolve.
    Unknown,
    /// It resolves and nothing accepted a connection on its port in time.
    Unreachable,
    /// Something accepted a connection on its port.
    Reachable,
}

/// A look at a host. Called on a worker thread, never the window's.
pub type HostCheck = dyn Fn(&Remote) -> HostState + Send + Sync;

/// How long a look at a host may wait for its port to accept.
pub const HOST_CHECK_DEADLINE: Duration = Duration::from_secs(3);

/// The real look: the name resolved, then one TCP connection to the
/// server's port, closed at once. Nothing is sent over it, so no
/// credential goes anywhere.
pub fn host_state(remote: &Remote) -> HostState {
    use std::net::ToSocketAddrs;
    let Some(port) = remote.port() else {
        return HostState::Reachable;
    };
    let Ok(addresses) = (remote.server().as_str(), port).to_socket_addrs() else {
        return HostState::Unknown;
    };
    let addresses: Vec<_> = addresses.collect();
    if addresses.is_empty() {
        return HostState::Unknown;
    }
    let reached = addresses
        .iter()
        .any(|address| std::net::TcpStream::connect_timeout(address, HOST_CHECK_DEADLINE).is_ok());
    if reached {
        HostState::Reachable
    } else {
        HostState::Unreachable
    }
}

/// A failure GVfs reported, made exact by a look at the host (ticket 310's
/// review): `gvfsd-smb` says "Invalid argument" for a name that does not
/// resolve, and a share that is "not there" on a server nothing answers for
/// was never reached. A refused sign-in, a changed key and a cancel are
/// never second-guessed.
fn refine(error: MountError, remote: &Remote, check: &HostCheck) -> MountError {
    if matches!(
        error.failure,
        MountFailure::Cancelled
            | MountFailure::PermissionDenied
            | MountFailure::HostKeyChanged
            | MountFailure::UntrustedGvfs
            | MountFailure::NoGvfs
            | MountFailure::HostNotFound
    ) {
        return error;
    }
    match check(remote) {
        HostState::Unknown => MountError::new(MountFailure::HostNotFound, error.detail),
        HostState::Unreachable if error.failure == MountFailure::ShareGone => {
            MountError::new(MountFailure::TimedOut, error.detail)
        }
        _ => error,
    }
}

/// Which network this computer is on, as the kernel's routing tables say:
/// each default route's interface and gateway. A dropped connection is
/// tried again by itself only while this stays the same (ticket 310's
/// review); after a change, only a click connects.
pub fn network_id() -> String {
    let mut parts: Vec<String> = Vec::new();
    let ipv4 = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    for line in ipv4.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() > 2 && fields[1] == "00000000" && fields[0] != "lo" {
            parts.push(format!("{}>{}", fields[0], fields[2]));
        }
    }
    let ipv6 = std::fs::read_to_string("/proc/net/ipv6_route").unwrap_or_default();
    for line in ipv6.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() > 9
            && fields[0] == "00000000000000000000000000000000"
            && fields[1] == "00"
            && fields[9] != "lo"
        {
            parts.push(format!("{}>{}", fields[9], fields[4]));
        }
    }
    parts.sort();
    parts.join(",")
}

/// One connection being made.
struct Attempt {
    key: String,
    /// The address GVfs was asked to mount.
    address: String,
    server: String,
    result: Receiver<Result<Mounted, MountError>>,
}

/// The connections being made, one per mount.
pub struct Attempts {
    mounter: Arc<dyn Mounter>,
    live: Vec<Attempt>,
    asks: (Sender<Ask>, Receiver<Ask>),
    /// What a failed attempt asks of the host to say why it failed.
    host_check: Arc<HostCheck>,
}

impl Attempts {
    pub fn new(mounter: Arc<dyn Mounter>) -> Self {
        Self {
            mounter,
            live: Vec::new(),
            asks: channel(),
            host_check: Arc::new(host_state),
        }
    }

    /// Uses another way of looking at a host after a failure. For tests.
    pub fn set_host_check(&mut self, check: Arc<HostCheck>) {
        self.host_check = check;
    }

    pub fn set_mounter(&mut self, mounter: Arc<dyn Mounter>) {
        self.mounter = mounter;
    }

    /// Connects to `remote`'s mount, or joins the attempt for it already in
    /// flight. Returns whether a new one was started.
    pub fn connect(&mut self, remote: &Remote) -> bool {
        let key = remote.mount_key();
        if self.is_connecting(&key) {
            return false;
        }
        let address = remote.mount_address();
        let (sender, result) = channel();
        let mounter = Arc::clone(&self.mounter);
        let asker = Asker::new(&address, self.asks.0.clone());
        let asked = address.clone();
        let check = Arc::clone(&self.host_check);
        let server = remote.clone();
        let spawned = std::thread::Builder::new()
            .name("files-server-mount".to_string())
            .spawn(move || {
                let result = mounter
                    .connect(&asked, &asker)
                    .map_err(|error| refine(error, &server, check.as_ref()));
                let _ = sender.send(result);
            });
        if spawned.is_err() {
            return false;
        }
        self.live.push(Attempt {
            key,
            address,
            server: remote.server(),
            result,
        });
        true
    }

    pub fn is_connecting(&self, key: &str) -> bool {
        self.live.iter().any(|attempt| attempt.key == key)
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The server an attempt is for, by its mount key.
    pub fn server(&self, key: &str) -> Option<&str> {
        self.live
            .iter()
            .find(|attempt| attempt.key == key)
            .map(|attempt| attempt.server.as_str())
    }

    /// Stops waiting for the attempt: its thread is left to end (GVfs gives
    /// up after its own limit), and what it brings is not used. Returns the
    /// address it was for.
    pub fn cancel(&mut self, key: &str) -> Option<String> {
        let at = self.live.iter().position(|attempt| attempt.key == key)?;
        Some(self.live.remove(at).address)
    }

    /// The questions asked since the last call.
    pub fn questions(&mut self) -> Vec<Ask> {
        self.asks.1.try_iter().collect()
    }

    /// The attempts that ended, by mount key.
    pub fn finished(&mut self) -> Vec<(String, Result<Mounted, MountError>)> {
        let mut done = Vec::new();
        self.live.retain(|attempt| match attempt.result.try_recv() {
            Ok(result) => {
                done.push((attempt.key.clone(), result));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                done.push((
                    attempt.key.clone(),
                    Err(MountError::new(MountFailure::Failed, "")),
                ));
                false
            }
        });
        done
    }
}

/// The mount key of the address a question is about.
pub fn key_of_address(address: &str) -> Option<String> {
    Remote::from_address(address).map(|remote| remote.mount_key())
}

/// GVfs's tracker on the session bus, asked for its mounts with
/// `ListMounts`.
pub struct GvfsTracker {
    connection: Mutex<Option<zbus::blocking::Connection>>,
    gvfs_root: Option<PathBuf>,
    /// The bus to ask on: `None` is the session's.
    bus: Option<String>,
}

impl GvfsTracker {
    pub fn session(gvfs_root: Option<PathBuf>) -> Self {
        Self {
            connection: Mutex::new(None),
            gvfs_root,
            bus: None,
        }
    }

    /// GVfs on the bus at `address` (a test's private bus).
    pub fn at(address: &str, gvfs_root: Option<PathBuf>) -> Self {
        Self {
            connection: Mutex::new(None),
            gvfs_root,
            bus: Some(address.to_string()),
        }
    }
}

/// `ListMounts`'s answer for one mount (the same shape as `LookupMount`).
type MountInfo = (
    String,
    zbus::zvariant::OwnedObjectPath,
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    Vec<u8>,
    (
        Vec<u8>,
        std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ),
    Vec<u8>,
);

fn wire_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

impl MountsSource for GvfsTracker {
    fn list(&self) -> Option<Vec<KnownMount>> {
        let mut held = self.connection.lock().ok()?;
        if held.is_none() {
            let builder = match &self.bus {
                Some(address) => zbus::blocking::connection::Builder::address(address.as_str()),
                None => zbus::blocking::connection::Builder::session(),
            };
            *held = builder
                .ok()?
                .method_timeout(MOUNT_LIST_DEADLINE)
                .build()
                .ok();
        }
        let connection = held.as_ref()?;
        // Only when GVfs is already there: listing must not start it.
        let bus = zbus::blocking::fdo::DBusProxy::new(connection).ok()?;
        let name = zbus::names::BusName::try_from(crate::gvfs::DAEMON).ok()?;
        if !bus.name_has_owner(name).unwrap_or(false) {
            return Some(Vec::new());
        }
        let reply = connection.call_method(
            Some(crate::gvfs::DAEMON),
            crate::gvfs::TRACKER_PATH,
            Some("org.gtk.vfs.MountTracker"),
            "ListMounts",
            &(),
        );
        let reply = match reply {
            Ok(reply) => reply,
            Err(_) => {
                // A connection that broke is made again next time.
                *held = None;
                return None;
            }
        };
        let (mounts,): (Vec<MountInfo>,) = reply.body().deserialize().ok()?;
        Some(
            mounts
                .into_iter()
                .filter_map(|info| {
                    let fuse = wire_text(&info.9);
                    let folder = (!fuse.is_empty()).then(|| PathBuf::from(&fuse));
                    let keys: Vec<(String, String)> = info
                        .10
                        .1
                        .iter()
                        .map(|(key, value)| (key.clone(), spec_text(value)))
                        .collect();
                    let remote = remote_from_keys(keys).or_else(|| {
                        let root = self.gvfs_root.as_deref()?;
                        Remote::from_gvfs_path(folder.as_deref()?, root)
                    })?;
                    Some(KnownMount { remote, folder })
                })
                .collect(),
        )
    }
}

/// A spec value as text: GVfs sends NUL-terminated byte strings.
fn spec_text(value: &zbus::zvariant::OwnedValue) -> String {
    match &**value {
        zbus::zvariant::Value::Array(array) => {
            let bytes: Vec<u8> = array
                .iter()
                .filter_map(|item| match item {
                    zbus::zvariant::Value::U8(byte) => Some(*byte),
                    _ => None,
                })
                .collect();
            wire_text(&bytes)
        }
        zbus::zvariant::Value::Str(text) => text.to_string(),
        _ => String::new(),
    }
}

/// A [`Remote`] from a spec's keys, when it is a server this connects to.
pub fn remote_from_keys(keys: Vec<(String, String)>) -> Option<Remote> {
    let mut keys: Vec<(String, String)> = keys
        .into_iter()
        .filter(|(key, _)| key != "prefix")
        .collect();
    keys.sort();
    let remote = Remote { keys, path: None };
    remote.scheme()?;
    remote.get(remote.host_key())?;
    Some(remote)
}

/// Rewrites, in place, every favourite stored as a folder under GVfs's
/// folder into its address (ticket 310). Returns whether any line changed.
pub fn migrate_bookmarks(file: &mut crate::bookmarks::BookmarkFile, gvfs_root: &Path) -> bool {
    let mut changed = false;
    for index in 0..file.len() {
        let Some(bookmark) = file.get(index) else {
            continue;
        };
        let Location::Local(path) = bookmark.location() else {
            continue;
        };
        if let Some(remote) = Remote::from_gvfs_path(path.as_path(), gvfs_root) {
            changed |= file.replace_location(index, &remote.location());
        }
    }
    changed
}

/// Rewrites every favourite stored as a folder under `mount_folder` (a
/// mount whose name was not understood, connected to by its typed address)
/// into the address of the same folder on `remote`'s mount.
pub fn migrate_under(
    file: &mut crate::bookmarks::BookmarkFile,
    mount_folder: &Path,
    remote: &Remote,
) -> bool {
    let mut changed = false;
    for index in 0..file.len() {
        let Some(bookmark) = file.get(index) else {
            continue;
        };
        let Location::Local(path) = bookmark.location() else {
            continue;
        };
        let Ok(below) = path.as_path().strip_prefix(mount_folder) else {
            continue;
        };
        let below = below.to_string_lossy();
        let inside = format!("/{}", below.trim_matches('/'));
        changed |= file.replace_location(index, &remote.at(Some(inside)).location());
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/run/user/1000/gvfs")
    }

    #[test]
    fn a_gvfs_folder_name_is_read_back_into_its_address() {
        let cases = [
            (
                "sftp:host=127.0.0.1,port=42210/tmp/srv/projects",
                "sftp://127.0.0.1:42210/tmp/srv/projects",
            ),
            (
                "sftp:host=build.local,user=ann/home/ann",
                "sftp://ann@build.local/home/ann",
            ),
            (
                "smb-share:port=42245,server=127.0.0.1,share=data/reports",
                "smb://127.0.0.1:42245/data/reports",
            ),
            (
                "smb-share:domain=WORK,server=nas,share=photos,user=ann/2024%20trip",
                "smb://WORK;ann@nas/photos/2024%2520trip",
            ),
            (
                "smb-share:server=nas,share=my%20files",
                "smb://nas/my%20files/",
            ),
            (
                "dav:host=cloud.example,prefix=%2Fremote.php%2Fdav,ssl=true/docs",
                "davs://cloud.example/remote.php/dav/docs",
            ),
            ("ftp:host=ftp.example/pub", "ftp://ftp.example/pub"),
            (
                "afp-volume:host=mac.local,user=ann,volume=Media",
                "afp://ann@mac.local/Media/",
            ),
            ("sftp:host=fe80%3A%3A1", "sftp://[fe80::1]/"),
        ];
        for (name, address) in cases {
            let path = root().join(name);
            let remote = Remote::from_gvfs_path(&path, &root())
                .unwrap_or_else(|| panic!("{name} is a server folder"));
            assert_eq!(remote.address(), address, "{name}");
        }
    }

    #[test]
    fn a_name_that_is_not_a_server_is_not_read_as_one() {
        for name in [
            "mtp:host=Phone",
            "google-drive:host=gmail.com,user=ann",
            "nonsense",
            "sftp:",
        ] {
            assert_eq!(Remote::from_gvfs_path(&root().join(name), &root()), None);
        }
        assert_eq!(
            Remote::from_gvfs_path(Path::new("/home/ann/sftp:host=x"), &root()),
            None
        );
    }

    #[test]
    fn an_address_and_its_gvfs_folder_are_the_same_mount() {
        let typed = Remote::from_address("sftp://127.0.0.1:42210/tmp/srv").unwrap();
        let shown = Remote::from_gvfs_path(&root().join("sftp:host=127.0.0.1,port=42210"), &root())
            .unwrap();
        assert!(typed.is_mounted_as(&shown));
        assert_eq!(typed.mount_key(), shown.mount_key());
        // Another port, or another user, is another mount.
        let other = Remote::from_address("sftp://127.0.0.1:42211/tmp").unwrap();
        assert!(!other.is_mounted_as(&shown));
        let ann = Remote::from_address("sftp://ann@127.0.0.1:42210/").unwrap();
        assert!(!ann.is_mounted_as(&shown));
        // GVfs leaves the user in when it was given, and a name typed in
        // capitals is the same host.
        let named =
            Remote::from_gvfs_path(&root().join("sftp:host=nas,user=ann"), &root()).unwrap();
        assert!(
            Remote::from_address("sftp://ann@NAS/")
                .unwrap()
                .is_mounted_as(&named)
        );
    }

    /// Ticket 310's review: a favourite never takes another server's or
    /// account's mount on the same host for its own.
    #[test]
    fn a_mount_on_the_same_host_with_another_port_user_or_share_is_another_mount() {
        let mount = |name: &str| Remote::from_gvfs_path(&root().join(name), &root()).unwrap();
        let address = |text: &str| Remote::from_address(text).unwrap();
        // A port the favourite names is that port only.
        assert!(!address("sftp://nas:2222/srv").is_mounted_as(&mount("sftp:host=nas")));
        // A favourite with no port is the scheme's own port, never "any".
        assert!(!address("sftp://nas/srv").is_mounted_as(&mount("sftp:host=nas,port=2222")));
        assert!(address("sftp://nas/srv").is_mounted_as(&mount("sftp:host=nas")));
        assert!(address("sftp://nas:22/srv").is_mounted_as(&mount("sftp:host=nas")));
        // A favourite with no user is "no user given", never "any user".
        assert!(!address("sftp://nas/srv").is_mounted_as(&mount("sftp:host=nas,user=root")));
        assert!(!address("sftp://ann@nas/srv").is_mounted_as(&mount("sftp:host=nas,user=root")));
        assert!(!address("sftp://ann@nas/srv").is_mounted_as(&mount("sftp:host=nas")));
        // Another share, another domain, another scheme.
        assert!(
            !address("smb://nas/photos").is_mounted_as(&mount("smb-share:server=nas,share=music"))
        );
        assert!(!address("smb://WORK;ann@nas/photos").is_mounted_as(&mount(
            "smb-share:domain=HOME,server=nas,share=photos,user=ann"
        )));
        assert!(!address("ftp://nas/pub").is_mounted_as(&mount("sftp:host=nas")));
        assert!(
            address("smb://nas/photos").is_mounted_as(&mount("smb-share:server=nas,share=photos"))
        );
        assert!(
            address("smb://nas:445/photos")
                .is_mounted_as(&mount("smb-share:server=nas,share=photos"))
        );
        // The rule never lists a folder on such a mount for the favourite.
        let mounts = Mounts::listed(vec![KnownMount {
            remote: mount("sftp:host=nas,port=2222").at(None),
            folder: Some(root().join("sftp:host=nas,port=2222")),
        }]);
        assert!(matches!(
            need(
                &Location::parse_uri("sftp://nas/srv"),
                Some(&root()),
                &mounts
            ),
            Need::Connect(_)
        ));
    }

    /// Ticket 310's review: a password typed in an address is never kept.
    #[test]
    fn a_password_in_an_address_never_reaches_the_location_or_the_spec() {
        let location = Location::parse_uri("sftp://ann:secret@nas/srv");
        assert_eq!(location.to_uri(), "sftp://ann@nas/srv");
        let remote = address_of("ftp://ann:secret@host/pub");
        assert!(!remote.address().contains("secret"));
        assert_eq!(remote.address(), "ftp://ann@host/pub");
    }

    fn address_of(text: &str) -> Remote {
        Remote::from_address(text).unwrap()
    }

    /// An explicit root stays the root through every form (ticket 310's
    /// review): a favourite of a mount's root never becomes its home.
    #[test]
    fn a_mount_root_favourite_stays_the_root() {
        let gvfs = root();
        let folder = Location::local(gvfs.join("sftp:host=nas")).unwrap();
        let stored = stored_form(&folder, Some(&gvfs));
        assert_eq!(stored.to_uri(), "sftp://nas/");
        let back = remote_of(&Location::parse_uri(&stored.to_uri()), None).unwrap();
        assert_eq!(back.path(), Some("/"));
        assert_eq!(
            back.local_in(
                &gvfs.join("sftp:host=nas"),
                &gvfs.join("sftp:host=nas/home/ann")
            ),
            LocalPath::new(gvfs.join("sftp:host=nas")).ok()
        );
        // With no folder at all, it is the server's own default one.
        let default = remote_of(&Location::parse_uri("sftp://nas"), None).unwrap();
        assert_eq!(default.path(), None);
    }

    #[test]
    fn an_address_round_trips_through_its_own_text() {
        for address in [
            "sftp://ann@build.local:2222/srv/www",
            "smb://WORK;ann@nas/photos/2024%20trip",
            "smb://nas/share/",
            "ftp://ftp.example/pub",
            "davs://cloud.example/remote.php/dav",
            "sftp://machine.tailnet.ts.net/",
        ] {
            let remote = Remote::from_address(address).unwrap();
            assert_eq!(Remote::from_address(&remote.address()).unwrap(), remote);
        }
        assert_eq!(
            Remote::from_address("sftp://machine.tailnet.ts.net")
                .unwrap()
                .path(),
            None,
            "an address with no folder opens the server's default folder"
        );
        assert_eq!(
            Remote::from_address("sftp://machine.tailnet.ts.net/")
                .unwrap()
                .path(),
            Some("/"),
            "one that names the root is the root"
        );
    }

    fn mounted(name: &str) -> KnownMount {
        KnownMount {
            remote: Remote::from_gvfs_path(&root().join(name), &root())
                .unwrap()
                .at(None),
            folder: Some(root().join(name)),
        }
    }

    #[test]
    fn the_rule_decides_from_the_mount_list_alone() {
        let name = "sftp:host=nas,user=ann";
        let folder = Location::local(root().join(name).join("srv/site")).unwrap();
        let address = Location::parse_uri("sftp://ann@nas/srv/site");
        let up = Mounts::listed(vec![mounted(name)]);
        let down = Mounts::listed(Vec::new());
        let unknown = Mounts::default();
        let gvfs = Some(root());
        let gvfs = gvfs.as_deref();

        assert_eq!(need(&folder, gvfs, &up), Need::Nothing);
        assert_eq!(
            need(&folder, gvfs, &down),
            Need::Connect(Remote::from_address("sftp://ann@nas/srv/site").unwrap())
        );
        // Not read yet: the reader's probe tells.
        assert_eq!(need(&folder, gvfs, &unknown), Need::Nothing);

        assert_eq!(
            need(&address, gvfs, &up),
            Need::At(LocalPath::new(root().join(name).join("srv/site")).unwrap())
        );
        assert!(matches!(need(&address, gvfs, &down), Need::Connect(_)));
        assert!(matches!(need(&address, gvfs, &unknown), Need::Connect(_)));
        // An address with no folder is its default one, which only a
        // connection names.
        assert!(matches!(
            need(&Location::parse_uri("sftp://ann@nas"), gvfs, &up),
            Need::Connect(_)
        ));

        // A server's mount whose name is not understood asks for its address.
        let odd = Location::local(root().join("dav+sd:host=Cloud._webdav._tcp.local/a")).unwrap();
        assert_eq!(
            need(&odd, gvfs, &down),
            Need::Address(root().join("dav+sd:host=Cloud._webdav._tcp.local/a"))
        );
        // A device's mount (a phone, a camera, an online account) is not a
        // server's, whatever its name: it keeps any folder's behaviour.
        for device in [
            "mtp:host=Phone/DCIM",
            "gphoto2:host=Camera",
            "afc:host=1234",
            "google-drive:host=gmail.com,user=ann",
            "mystery-backend:x=1/a",
        ] {
            let location = Location::local(root().join(device)).unwrap();
            assert_eq!(need(&location, gvfs, &down), Need::Nothing, "{device}");
            assert!(!is_remote(&location, gvfs), "{device}");
        }
        // Anything else needs nothing, and GVfs's folder itself is a folder.
        for location in [
            Location::local("/home/ann").unwrap(),
            Location::local(root()).unwrap(),
            Location::Recent,
        ] {
            assert_eq!(need(&location, gvfs, &down), Need::Nothing);
        }
        assert_eq!(need(&folder, None, &down), Need::Nothing);
    }

    #[test]
    fn a_pinned_gvfs_folder_is_stored_as_its_address() {
        let gvfs = root();
        let folder = Location::local(gvfs.join("sftp:host=nas,port=2222/srv")).unwrap();
        assert_eq!(
            stored_form(&folder, Some(&gvfs)).to_uri(),
            "sftp://nas:2222/srv"
        );
        let local = Location::local("/home/ann/Documents").unwrap();
        assert_eq!(stored_form(&local, Some(&gvfs)), local);
    }

    #[test]
    fn failures_become_reasons_a_person_can_act_on() {
        let error = |failure, detail: &str| MountError::new(failure, detail);
        let of = |failure, detail, up| Reason::of(&error(failure, detail), "nas", up);
        assert_eq!(of(MountFailure::TimedOut, "", true), Reason::NoAnswer);
        assert_eq!(
            of(MountFailure::HostNotFound, "", true),
            Reason::NameNotFound
        );
        assert_eq!(of(MountFailure::Refused, "", true), Reason::Refused);
        assert_eq!(
            of(MountFailure::PermissionDenied, "", true),
            Reason::SignInRefused
        );
        assert_eq!(of(MountFailure::ShareGone, "", true), Reason::ShareGone);
        assert_eq!(of(MountFailure::NetworkDown, "", true), Reason::NoNetwork);
        assert_eq!(
            of(
                MountFailure::Failed,
                "ssh: connect to host nas port 22: Connection refused",
                true
            ),
            Reason::Refused
        );
        assert_eq!(
            of(
                MountFailure::Failed,
                "The connection is closed (the underlying SSH process exited)",
                true
            ),
            Reason::NoAnswer
        );
        assert_eq!(
            of(MountFailure::Failed, "Connection failed", true),
            Reason::NoAnswer
        );
        // Without a network, the network is what to fix.
        assert_eq!(of(MountFailure::TimedOut, "", false), Reason::NoNetwork);
        assert_eq!(of(MountFailure::HostNotFound, "", false), Reason::NoNetwork);
        // A server on this computer needs no network.
        assert_eq!(
            Reason::of(&error(MountFailure::Refused, ""), "127.0.0.1", false),
            Reason::Refused
        );
        assert_eq!(
            of(MountFailure::HostKeyChanged, "", true),
            Reason::Mount(MountFailure::HostKeyChanged)
        );
        assert!(Reason::NoAnswer.offers_network_settings());
        assert!(!Reason::SignInRefused.offers_network_settings());
    }

    /// GVfs sends "Connection failed" in the desktop's language (seen in
    /// the sandbox in zh_TW): it is still a server that did not answer.
    #[test]
    fn gvfs_s_own_words_are_read_in_its_chinese_too() {
        let words = test_prerequisites::require!(
            "the zh_TW.UTF-8 locale and GVfs's zh_TW catalog",
            crate::gvfs::gvfs_translation_in("Connection failed", c"zh_TW.UTF-8")
                .ok_or("gettext found no zh_TW translation of GVfs's message")
        );
        assert_eq!(words, "連線失敗");
        let error = MountError::new(MountFailure::Failed, words.clone());
        let in_chinese = |message: &str| crate::gvfs::gvfs_translation_in(message, c"zh_TW.UTF-8");
        assert_eq!(
            Reason::of_in(&error, "nas", true, &in_chinese),
            Reason::NoAnswer
        );
        // Words GVfs does not have stay unknown, never guessed.
        let other = MountError::new(MountFailure::Failed, "隨便什麼");
        assert_eq!(
            Reason::of_in(&other, "nas", true, &in_chinese),
            Reason::Mount(MountFailure::Failed)
        );
    }

    #[test]
    fn network_settings_is_whichever_is_installed() {
        assert_eq!(
            network_settings(&|program| program == "cosmic-settings"),
            Some(("cosmic-settings".into(), vec!["network".into()]))
        );
        assert_eq!(network_settings(&|_| false), None);
    }

    #[test]
    fn a_folder_that_does_not_answer_is_given_up_on_and_not_probed_twice() {
        let scratch = tempfile::tempdir().unwrap();
        let probing = Probing::default();
        let mount = scratch.path().join("sftp:host=x");
        std::fs::create_dir_all(mount.join("in")).unwrap();
        assert_eq!(
            probe(&mount.join("in"), &mount, PROBE_DEADLINE, &probing),
            Probed::Folder
        );
        assert_eq!(
            probe(&mount.join("gone"), &mount, PROBE_DEADLINE, &probing),
            Probed::Missing
        );
        let absent = scratch.path().join("sftp:host=y");
        assert_eq!(
            probe(&absent.join("in"), &absent, PROBE_DEADLINE, &probing),
            Probed::NotMounted
        );
        // A probe still out on a mount answers for it.
        probing.hold(&mount);
        let started = Instant::now();
        assert_eq!(
            probe(&mount.join("in"), &mount, PROBE_DEADLINE, &probing),
            Probed::NotAnswering
        );
        assert!(started.elapsed() < Duration::from_millis(100));
        // A probe still within its time is only running beside this one: two
        // listings of one mount at once both get their answer (seen in the
        // sandbox: a refresh and a navigation side by side).
        probing.forget(&mount);
        probing
            .0
            .lock()
            .unwrap()
            .insert(mount.clone(), (7, Instant::now()));
        assert_eq!(
            probe(&mount.join("in"), &mount, PROBE_DEADLINE, &probing),
            Probed::Folder
        );
    }

    /// A tracker that never answers: the list stands as it was, and nobody
    /// waits past the deadline.
    struct Hanging;

    impl MountsSource for Hanging {
        fn list(&self) -> Option<Vec<KnownMount>> {
            std::thread::sleep(Duration::from_secs(60));
            None
        }
    }

    #[test]
    fn a_tracker_that_does_not_answer_holds_nobody_up() {
        let watch = MountWatch::new(Arc::new(Hanging));
        let started = Instant::now();
        watch.refresh();
        assert!(started.elapsed() < Duration::from_millis(50));
        let mounts = watch.fresh(Duration::from_secs(60), Duration::from_millis(300));
        assert!(mounts.known.is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn the_mount_list_is_read_off_the_calling_thread_and_kept() {
        let fixed = FixedMounts::default();
        fixed.set(Some(vec![mounted("sftp:host=nas")]));
        let watch = MountWatch::new(Arc::new(fixed.clone()));
        let mounts = watch.fresh(Duration::from_secs(60), Duration::from_secs(5));
        assert_eq!(mounts.known.as_ref().map(Vec::len), Some(1));
        assert_eq!(mounts.has_folder(&root().join("sftp:host=nas")), Some(true));
        assert_eq!(
            mounts.has_folder(&root().join("sftp:host=other")),
            Some(false)
        );
    }
}
