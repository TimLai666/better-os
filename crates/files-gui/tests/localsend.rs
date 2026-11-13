//! LocalSend's protocol, against a stand-in receiver on this machine (plain
//! HTTP, which the protocol allows; real receivers use HTTPS).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::sync::{Arc, Mutex};

use files_gui::localsend::{
    KnownPeers, Peer, SendError, own_info, parse_announcement, prepare_body, send,
};

#[test]
fn an_announcement_is_read_and_our_own_is_ignored() {
    let json = br#"{"alias":"Phone","version":"2.1","deviceModel":"Pixel","deviceType":"mobile","fingerprint":"abc","port":53317,"protocol":"https","download":false,"announce":false}"#;
    let from = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 7));
    let peer = parse_announcement(json, from, "mine").unwrap();
    assert_eq!(peer.alias, "Phone");
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
fn read_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
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

fn answer(stream: &mut std::net::TcpStream, status: &str, body: &str) {
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
    let seen: Arc<Mutex<Vec<Request>>> = Arc::default();
    let record = seen.clone();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
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
            record.lock().unwrap().push((first, body));
        }
    });
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("notes.txt");
    std::fs::write(&file, b"hello over the LAN").unwrap();
    let peer = Peer {
        alias: "Stand-in".into(),
        address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        protocol: "http".into(),
        fingerprint: "x".into(),
    };
    send(&peer, own_info("Me", "f", false), &[file]).unwrap();
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
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        answer(&mut stream, "403 Forbidden", "");
    });
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    let peer = Peer {
        alias: "No".into(),
        address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        protocol: "http".into(),
        fingerprint: "x".into(),
    };
    assert_eq!(
        send(&peer, own_info("Me", "f", false), &[file]),
        Err(SendError::Declined)
    );
    server.join().unwrap();
}
