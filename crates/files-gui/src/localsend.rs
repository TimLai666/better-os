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
//! its fingerprint (the certificate's SHA-256). There is no CA to check it
//! against, so it is pinned instead: a connection whose certificate is not
//! the announced one is refused before anything is sent, and a device that
//! offers only plain HTTP is not offered at all. The receiver still has to
//! accept the transfer on its own screen. Discovery runs only when a Share
//! menu is opened, never in the background.

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
    // Plain HTTP would send the files to whoever answers: not offered.
    (info.fingerprint != own_fingerprint && !info.alias.is_empty() && info.protocol == "https")
        .then_some(Peer {
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

/// The most of a receiver's answer that is read: a `prepare-upload` answer
/// is a session and one token per file, a few kilobytes at most.
pub const ANSWER_LIMIT: usize = 1024 * 1024;

/// Sends files to a peer: asks, then uploads each file the peer accepted.
///
/// Only over HTTPS, and only to the certificate the peer announced: the
/// connection is refused unless the certificate's SHA-256 is the
/// fingerprint from its announcement, so a machine answering in its place
/// cannot take the files.
pub fn send(peer: &Peer, own: Info, files: &[PathBuf]) -> Result<(), SendError> {
    if peer.protocol != "https" {
        return Err(SendError::Failed("localsend.insecure_peer".to_string()));
    }
    let body = prepare_body(own, files).map_err(|error| SendError::Failed(error.to_string()))?;
    let (status, answer) = exchange(
        peer,
        "/api/localsend/v2/prepare-upload",
        "application/json",
        body.len() as u64,
        &mut body.as_bytes(),
    )?;
    match status {
        200 => {}
        204 => return Ok(()),
        403 => return Err(SendError::Declined),
        code => return Err(SendError::Failed(format!("HTTP {code}"))),
    }
    let accepted: PrepareAnswer =
        serde_json::from_slice(&answer).map_err(|error| SendError::Failed(error.to_string()))?;
    for (index, file) in files.iter().enumerate() {
        let id = index.to_string();
        let Some(token) = accepted.files.get(&id) else {
            continue;
        };
        let mut reader =
            std::fs::File::open(file).map_err(|error| SendError::Failed(error.to_string()))?;
        let length = reader
            .metadata()
            .map_err(|error| SendError::Failed(error.to_string()))?
            .len();
        let path = format!(
            "/api/localsend/v2/upload?sessionId={}&fileId={}&token={}",
            query_value(&accepted.session_id),
            query_value(&id),
            query_value(token)
        );
        let (status, _) = exchange(peer, &path, "application/octet-stream", length, &mut reader)?;
        if status != 200 {
            return Err(SendError::Failed(format!("HTTP {status}")));
        }
    }
    Ok(())
}

/// Percent-encodes a query value: everything but the unreserved characters.
fn query_value(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Accepts exactly the certificate whose SHA-256 is the announced
/// fingerprint; the signatures in the handshake are checked as usual.
#[derive(Debug)]
struct PinnedCertificate {
    fingerprint: String,
    provider: std::sync::Arc<rustls::crypto::CryptoProvider>,
}

/// A certificate's fingerprint as LocalSend announces it: the SHA-256 of
/// its DER encoding, in hex.
pub fn certificate_fingerprint(der: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(der)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl rustls::client::danger::ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if certificate_fingerprint(end_entity.as_ref()).eq_ignore_ascii_case(&self.fingerprint) {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "localsend.certificate_mismatch".to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// One request on a fresh connection to the peer: `POST path` with
/// `length` bytes from `body`. Returns the status and at most
/// [`ANSWER_LIMIT`] bytes of the answer.
fn exchange(
    peer: &Peer,
    path: &str,
    content_type: &str,
    length: u64,
    body: &mut dyn Read,
) -> Result<(u16, Vec<u8>), SendError> {
    use std::io::Write;
    let failed = |error: &dyn std::fmt::Display| SendError::Failed(error.to_string());
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|error| failed(&error))?
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(PinnedCertificate {
            fingerprint: peer.fingerprint.clone(),
            provider,
        }))
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::IpAddress(peer.address.into());
    let connection = rustls::ClientConnection::new(std::sync::Arc::new(config), name)
        .map_err(|error| failed(&error))?;
    let address = SocketAddr::new(peer.address, peer.port);
    let socket = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(10))
        .map_err(|error| failed(&error))?;
    // A receiver that stops answering does not hold the send forever.
    let _ = socket.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = socket.set_write_timeout(Some(Duration::from_secs(120)));
    let mut stream = rustls::StreamOwned::new(connection, socket);

    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| failed(&error))?;
    let sent =
        std::io::copy(&mut body.take(length), &mut stream).map_err(|error| failed(&error))?;
    if sent != length {
        return Err(SendError::Failed("localsend.file_changed".to_string()));
    }
    stream.flush().map_err(|error| failed(&error))?;
    read_answer(&mut stream)
}

/// Reads an HTTP/1.1 answer: the status, then a body by length, by chunks,
/// or to the end, never more than [`ANSWER_LIMIT`].
pub fn read_answer(stream: &mut dyn Read) -> Result<(u16, Vec<u8>), SendError> {
    use std::io::BufRead;
    let too_long = || SendError::Failed("localsend.answer_too_long".to_string());
    let failed = |error: std::io::Error| SendError::Failed(error.to_string());
    let mut reader = std::io::BufReader::new(stream.take(ANSWER_LIMIT as u64 * 2));
    let mut line = String::new();
    reader.read_line(&mut line).map_err(failed)?;
    let status: u16 = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| SendError::Failed("localsend.bad_answer".to_string()))?;
    let mut length: Option<usize> = None;
    let mut chunked = false;
    let mut header_bytes = 0usize;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(failed)?;
        header_bytes += read;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if header_bytes > 64 * 1024 {
            return Err(too_long());
        }
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().ok();
        } else if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            line.clear();
            reader.read_line(&mut line).map_err(failed)?;
            let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| SendError::Failed("localsend.bad_answer".to_string()))?;
            if size == 0 {
                break;
            }
            if body.len() + size > ANSWER_LIMIT {
                return Err(too_long());
            }
            let start = body.len();
            body.resize(start + size, 0);
            reader.read_exact(&mut body[start..]).map_err(failed)?;
            line.clear();
            reader.read_line(&mut line).map_err(failed)?;
        }
    } else if let Some(length) = length {
        if length > ANSWER_LIMIT {
            return Err(too_long());
        }
        body.resize(length, 0);
        reader.read_exact(&mut body).map_err(failed)?;
    } else {
        reader
            .take(ANSWER_LIMIT as u64 + 1)
            .read_to_end(&mut body)
            .map_err(failed)?;
        if body.len() > ANSWER_LIMIT {
            return Err(too_long());
        }
    }
    Ok((status, body))
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
