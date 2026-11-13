//! LocalSend's protocol, against a stand-in receiver on this machine that
//! serves HTTPS with a self-signed certificate, as LocalSend apps do. The
//! certificate and its key in `fixtures/` exist only for these tests.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::sync::{Arc, Mutex};

use files_gui::localsend::{
    ANSWER_LIMIT, KnownPeers, Peer, SendError, certificate_fingerprint, own_info,
    parse_announcement, prepare_body, read_answer, send,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

const CERT: &[u8] = include_bytes!("fixtures/localsend-cert.pem");
const KEY: &[u8] = include_bytes!("fixtures/localsend-key.pem");

/// The stand-in's certificate fingerprint, as it would announce it.
fn stand_in_fingerprint() -> String {
    certificate_fingerprint(CertificateDer::from_pem_slice(CERT).unwrap().as_ref())
}

fn server_config() -> Arc<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from_pem_slice(CERT).unwrap()],
                PrivateKeyDer::from_pem_slice(KEY).unwrap(),
            )
            .unwrap(),
    )
}

type Tls = rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>;

/// Accepts one connection and completes the TLS handshake, or `None` when
/// the client gave up on it.
fn accept(listener: &TcpListener, config: &Arc<rustls::ServerConfig>) -> Option<Tls> {
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut tls = rustls::StreamOwned::new(
        rustls::ServerConnection::new(config.clone()).unwrap(),
        socket,
    );
    while tls.conn.is_handshaking() {
        if tls.conn.complete_io(&mut tls.sock).is_err() {
            return None;
        }
    }
    Some(tls)
}

fn peer(port: u16, fingerprint: &str) -> Peer {
    Peer {
        alias: "Stand-in".into(),
        address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        protocol: "https".into(),
        fingerprint: fingerprint.into(),
    }
}

#[test]
fn an_announcement_is_read_and_our_own_is_ignored() {
    let json = br#"{"alias":"Phone","version":"2.1","deviceModel":"Pixel","deviceType":"mobile","fingerprint":"abc","port":53317,"protocol":"https","download":false,"announce":false}"#;
    let from = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 7));
    let peer = parse_announcement(json, from, "mine").unwrap();
    assert_eq!(peer.alias, "Phone");
    // A device that offers only plain HTTP is not offered.
    let plain = br#"{"alias":"Old","fingerprint":"zz","port":53317,"protocol":"http"}"#;
    assert_eq!(parse_announcement(plain, from, "mine"), None);
    assert_eq!(peer.address, from);
    assert_eq!(peer.port, 53317);
    assert_eq!(
        parse_announcement(json, from, "abc"),
        None,
        "our own announcement"
    );
    assert_eq!(parse_announcement(b"not json", from, "mine"), None);
}

fn device(alias: &str, fingerprint: &str, last: u8) -> Peer {
    Peer {
        alias: alias.into(),
        address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, last)),
        port: 53317,
        protocol: "https".into(),
        fingerprint: fingerprint.into(),
    }
}

#[test]
fn a_device_joins_the_list_the_moment_it_answers_and_leaves_when_gone() {
    // The window redraws an open menu when the generation moves, so each
    // change has to move it and nothing else may.
    let known = KnownPeers::default();
    let start = known.generation();
    let phone = device("Phone", "p", 7);
    known.found(&phone);
    assert_eq!(known.peers(), vec![phone.clone()]);
    let after_phone = known.generation();
    assert_ne!(after_phone, start, "a new device is a change");

    known.found(&phone);
    assert_eq!(
        known.generation(),
        after_phone,
        "the same answer twice is not"
    );

    let moved = device("Phone", "p", 9);
    known.found(&moved);
    assert_eq!(known.peers(), vec![moved.clone()], "one entry, new address");
    assert_ne!(known.generation(), after_phone);

    let laptop = device("Laptop", "l", 8);
    known.found(&laptop);
    let before_end = known.generation();
    known.finished(vec![moved.clone(), laptop.clone()]);
    assert_eq!(
        known.generation(),
        before_end,
        "the search ended as it went"
    );

    known.finished(vec![laptop.clone()]);
    assert_eq!(
        known.peers(),
        vec![laptop],
        "the phone did not answer this time"
    );
    assert_ne!(known.generation(), before_end);
}

#[test]
fn the_prepare_body_names_each_file_and_its_size() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("photo.jpg");
    std::fs::write(&file, b"12345").unwrap();
    let body = prepare_body(own_info("Me", "f", false), &[file]).unwrap();
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["files"]["0"]["fileName"], "photo.jpg");
    assert_eq!(value["files"]["0"]["size"], 5);
    assert_eq!(value["info"]["alias"], "Me");
}

/// Reads one HTTP request: its first line and its body (by length or
/// chunks).
fn read_request(stream: &mut Tls) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let mut length = 0usize;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
        if line == "\r\n" {
            break;
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).unwrap();
            let size = usize::from_str_radix(size.trim(), 16).unwrap();
            let mut chunk = vec![0u8; size + 2];
            reader.read_exact(&mut chunk).unwrap();
            if size == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..size]);
        }
    } else {
        body.resize(length, 0);
        reader.read_exact(&mut body).unwrap();
    }
    (first, body)
}

fn answer(stream: &mut Tls, status: &str, body: &str) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

/// One request the stand-in received: its first line and its body.
type Request = (String, Vec<u8>);

#[test]
fn files_are_offered_then_uploaded_with_the_session_and_token() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = server_config();
    let seen: Arc<Mutex<Vec<Request>>> = Arc::default();
    let record = seen.clone();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut stream = accept(&listener, &config).expect("a handshake");
            let (first, body) = read_request(&mut stream);
            if first.contains("prepare-upload") {
                answer(
                    &mut stream,
                    "200 OK",
                    r#"{"sessionId":"s1","files":{"0":"t0"}}"#,
                );
            } else {
                answer(&mut stream, "200 OK", "");
            }
            stream.conn.send_close_notify();
            let _ = stream.flush();
            record.lock().unwrap().push((first, body));
        }
    });
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("notes.txt");
    std::fs::write(&file, b"hello over the LAN").unwrap();
    send(
        &peer(port, &stand_in_fingerprint()),
        own_info("Me", "f", false),
        &[file],
    )
    .unwrap();
    server.join().unwrap();
    let seen = seen.lock().unwrap();
    assert!(
        seen[0]
            .0
            .starts_with("POST /api/localsend/v2/prepare-upload")
    );
    assert!(seen[1].0.contains("/api/localsend/v2/upload?"));
    assert!(
        seen[1].0.contains("sessionId=s1")
            && seen[1].0.contains("token=t0")
            && seen[1].0.contains("fileId=0")
    );
    assert_eq!(seen[1].1, b"hello over the LAN");
}

#[test]
fn a_receiver_that_declines_is_reported_as_declined() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = server_config();
    let server = std::thread::spawn(move || {
        let mut stream = accept(&listener, &config).expect("a handshake");
        let _ = read_request(&mut stream);
        answer(&mut stream, "403 Forbidden", "");
        stream.conn.send_close_notify();
        let _ = stream.flush();
    });
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(
        send(
            &peer(port, &stand_in_fingerprint()),
            own_info("Me", "f", false),
            &[file]
        ),
        Err(SendError::Declined)
    );
    server.join().unwrap();
}

/// A machine answering with a certificate other than the one announced
/// gets nothing: the connection ends in the handshake.
#[test]
fn a_certificate_other_than_the_announced_one_is_refused_before_anything_is_sent() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = server_config();
    let server = std::thread::spawn(move || accept(&listener, &config).is_some());
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("secret.txt");
    std::fs::write(&file, b"not for them").unwrap();
    let announced = "0".repeat(64);
    let result = send(&peer(port, &announced), own_info("Me", "f", false), &[file]);
    match result {
        Err(SendError::Failed(reason)) => {
            assert!(
                reason.contains("localsend.certificate_mismatch"),
                "{reason}"
            )
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!server.join().unwrap(), "the handshake never completed");
}

#[test]
fn a_plain_http_peer_is_refused() {
    let mut plain = peer(1, &stand_in_fingerprint());
    plain.protocol = "http".into();
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(
        send(&plain, own_info("Me", "f", false), &[file]),
        Err(SendError::Failed("localsend.insecure_peer".into()))
    );
}

#[test]
fn an_answer_is_read_up_to_its_limit_and_no_further() {
    let small = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec();
    assert_eq!(
        read_answer(&mut small.as_slice()),
        Ok((200, b"{}".to_vec()))
    );
    let chunked =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n".to_vec();
    assert_eq!(
        read_answer(&mut chunked.as_slice()),
        Ok((200, b"{}".to_vec()))
    );
    let declared = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
        ANSWER_LIMIT + 1
    );
    assert!(read_answer(&mut declared.as_bytes()).is_err());
    let mut endless = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
    endless.extend(std::iter::repeat_n(b'x', ANSWER_LIMIT + 10));
    assert!(read_answer(&mut endless.as_slice()).is_err());
}
