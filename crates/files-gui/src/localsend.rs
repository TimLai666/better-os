//! LocalSend: sending files to a LocalSend app on the same network, as
//! folio's LocalSend plugin does, over LocalSend's own protocol (v2):
//!
//! - **Discovery.** A UDP announcement to `224.0.0.167:53317`; LocalSend
//!   apps answer with their own announcement on the same group.
//! - **Sending.** `POST /api/localsend/v2/prepare-upload` with the files'
//!   names and sizes; the receiver accepts (a session and one token per
//!   file) or refuses; then one `POST /api/localsend/v2/upload` per file.
//!
//! LocalSend apps serve HTTPS with a self-signed certificate and announce
//! its fingerprint, so the certificate is not checked against a CA here
//! (there is none to check against); the receiver still has to accept the
//! transfer on its own screen. Discovery runs only when a Share menu is
//! opened, never in the background.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const MULTICAST: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
pub const PORT: u16 = 53317;
pub const VERSION: &str = "2.1";

/// What a LocalSend device says about itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub alias: String,
    #[serde(default = "version")]
    pub version: String,
    #[serde(default)]
    pub device_model: Option<String>,
    #[serde(default)]
    pub device_type: Option<String>,
    pub fingerprint: String,
    #[serde(default = "port")]
    pub port: u16,
    #[serde(default = "protocol")]
    pub protocol: String,
    #[serde(default)]
    pub download: bool,
    #[serde(default)]
    pub announce: Option<bool>,
}

fn version() -> String {
    VERSION.to_string()
}
fn port() -> u16 {
    PORT
}
fn protocol() -> String {
    "https".to_string()
}

/// A device found on the network.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Peer {
    pub alias: String,
    pub address: IpAddr,
    pub port: u16,
    pub protocol: String,
    pub fingerprint: String,
}

impl Peer {
    fn base(&self) -> String {
        let scheme = if self.protocol == "http" {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{}:{}", self.address, self.port)
    }
}

/// This program, as LocalSend devices see it. It only sends, so it serves
/// nothing (`port` is LocalSend's, and `download` is false).
pub fn own_info(alias: &str, fingerprint: &str, announce: bool) -> Info {
    Info {
        alias: alias.to_string(),
        version: VERSION.to_string(),
        device_model: Some("Better Files".to_string()),
        device_type: Some("desktop".to_string()),
        fingerprint: fingerprint.to_string(),
        port: PORT,
        protocol: "https".to_string(),
        download: false,
        announce: Some(announce),
    }
}

/// A device's announcement, unless it is our own.
pub fn parse_announcement(bytes: &[u8], from: IpAddr, own_fingerprint: &str) -> Option<Peer> {
    let info: Info = serde_json::from_slice(bytes).ok()?;
    (info.fingerprint != own_fingerprint && !info.alias.is_empty()).then_some(Peer {
        alias: info.alias,
        address: from,
        port: info.port,
        protocol: info.protocol,
        fingerprint: info.fingerprint,
    })
}

/// Announces on the multicast group and collects who answers for `wait`.
/// The socket is shared with a LocalSend app on this machine (address
/// reuse), so both keep working.
/// Announces this device and collects the answers for `wait`. `found` is
/// told about each device the moment it answers, so a menu that is already
/// open can show it then rather than when the wait is over.
pub fn discover(
    alias: &str,
    fingerprint: &str,
    wait: Duration,
    mut found: impl FnMut(&Peer),
) -> Vec<Peer> {
    let Ok(socket) = multicast_socket() else {
        return Vec::new();
    };
    let announcement = serde_json::to_vec(&own_info(alias, fingerprint, true)).unwrap_or_default();
    let _ = socket.send_to(&announcement, SocketAddr::from((MULTICAST, PORT)));
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    let started = Instant::now();
    let mut peers: Vec<Peer> = Vec::new();
    let mut buffer = [0u8; 8192];
    while started.elapsed() < wait {
        if let Ok((length, from)) = socket.recv_from(&mut buffer)
            && let Some(peer) = parse_announcement(&buffer[..length], from.ip(), fingerprint)
            && !peers
                .iter()
                .any(|known| known.fingerprint == peer.fingerprint)
        {
            found(&peer);
            peers.push(peer);
        }
    }
    peers
}

/// The devices the window knows about, shared with the thread that looks
/// for them. `generation` changes whenever the list does, so the window
/// redraws an open menu exactly then.
#[derive(Default)]
pub struct KnownPeers {
    peers: std::sync::Mutex<Vec<Peer>>,
    generation: std::sync::atomic::AtomicU64,
}

impl KnownPeers {
    pub fn peers(&self) -> Vec<Peer> {
        self.peers
            .lock()
            .map(|peers| peers.clone())
            .unwrap_or_default()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    /// A device answered: added (or its address brought up to date).
    pub fn found(&self, peer: &Peer) {
        let Ok(mut peers) = self.peers.lock() else {
            return;
        };
        match peers
            .iter_mut()
            .find(|known| known.fingerprint == peer.fingerprint)
        {
            Some(known) if known == peer => return,
            Some(known) => *known = peer.clone(),
            None => peers.push(peer.clone()),
        }
        self.bump();
    }

    /// A search ended: the devices that answered it are the list, so one
    /// that has gone away leaves it.
    pub fn finished(&self, answered: Vec<Peer>) {
        let Ok(mut peers) = self.peers.lock() else {
            return;
        };
        if *peers != answered {
            *peers = answered;
            self.bump();
        }
    }

    fn bump(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
}

fn multicast_socket() -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, PORT)).into())?;
    socket.join_multicast_v4(&MULTICAST, &Ipv4Addr::UNSPECIFIED)?;
    Ok(socket.into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDto {
    id: String,
    file_name: String,
    size: u64,
    file_type: String,
}

#[derive(Serialize)]
struct PrepareUpload {
    info: Info,
    files: std::collections::BTreeMap<String, FileDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareAnswer {
    session_id: String,
    files: std::collections::BTreeMap<String, String>,
}

/// The `prepare-upload` body for these files, numbered `0`, `1`, ….
pub fn prepare_body(own: Info, files: &[PathBuf]) -> std::io::Result<String> {
    let mut entries = std::collections::BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        let size = std::fs::metadata(file)?.len();
        let id = index.to_string();
        entries.insert(
            id.clone(),
            FileDto {
                id,
                file_name: file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                size,
                file_type: "application/octet-stream".to_string(),
            },
        );
    }
    serde_json::to_string(&PrepareUpload {
        info: own,
        files: entries,
    })
    .map_err(std::io::Error::other)
}

/// Why a send did not go.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SendError {
    /// The receiver said no on its screen.
    Declined,
    /// Anything else, in the transport's words.
    Failed(String),
}

/// Sends files to a peer: asks, then uploads each file the peer accepted.
pub fn send(peer: &Peer, own: Info, files: &[PathBuf]) -> Result<(), SendError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                // Self-signed by design; see the module notes.
                .disable_verification(true)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(3_600)))
        .http_status_as_error(false)
        .build()
        .into();
    let body = prepare_body(own, files).map_err(|error| SendError::Failed(error.to_string()))?;
    let mut answer = agent
        .post(format!("{}/api/localsend/v2/prepare-upload", peer.base()))
        .content_type("application/json")
        .send(body)
        .map_err(|error| SendError::Failed(error.to_string()))?;
    match answer.status().as_u16() {
        200 => {}
        204 => return Ok(()),
        403 => return Err(SendError::Declined),
        code => return Err(SendError::Failed(format!("HTTP {code}"))),
    }
    let mut text = String::new();
    answer
        .body_mut()
        .as_reader()
        .read_to_string(&mut text)
        .map_err(|error| SendError::Failed(error.to_string()))?;
    let accepted: PrepareAnswer =
        serde_json::from_str(&text).map_err(|error| SendError::Failed(error.to_string()))?;
    for (index, file) in files.iter().enumerate() {
        let id = index.to_string();
        let Some(token) = accepted.files.get(&id) else {
            continue;
        };
        let reader =
            std::fs::File::open(file).map_err(|error| SendError::Failed(error.to_string()))?;
        let response = agent
            .post(format!("{}/api/localsend/v2/upload", peer.base()))
            .query("sessionId", &accepted.session_id)
            .query("fileId", &id)
            .query("token", token)
            .content_type("application/octet-stream")
            .send(ureq::SendBody::from_reader(&mut std::io::BufReader::new(
                reader,
            )))
            .map_err(|error| SendError::Failed(error.to_string()))?;
        if response.status().as_u16() != 200 {
            return Err(SendError::Failed(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }
    }
    Ok(())
}

/// A fingerprint for this program's announcements: random per process,
/// since it serves no certificate whose hash it could be.
pub fn fingerprint() -> String {
    let mut bytes = [0u8; 16];
    if let Ok(mut random) = std::fs::File::open("/dev/urandom") {
        let _ = random.read_exact(&mut bytes);
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The name this machine shows other LocalSend devices.
pub fn alias() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .map(|name| format!("Better Files on {name}"))
        .unwrap_or_else(|| "Better Files".to_string())
}

/// Whether a path is something LocalSend can send: a regular file.
pub fn sendable(path: &Path) -> bool {
    path.is_file()
}
