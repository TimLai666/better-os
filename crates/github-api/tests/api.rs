//! The client against a local fake server on 127.0.0.1 that answers
//! recorded GitHub responses. Nothing here reaches GitHub.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use github_api::{ApiError, CLIENT_ID, CheckState, Client, Host, Poll, Token};

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<String>,
    body: String,
}

type Routes = Vec<(&'static str, &'static str, u16, String)>;

/// Serves the routes (method, path prefix, status, body) until dropped;
/// each request is recorded.
fn server(routes: Routes) -> (Host, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
            let mut headers = Vec::new();
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end().to_string();
                if header.is_empty() {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                headers.push(header);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let (status, answer) = routes
                .iter()
                .find(|(m, p, _, _)| *m == method && path.starts_with(p))
                .map(|(_, _, s, b)| (*s, b.clone()))
                .unwrap_or((404, r#"{"message":"Not Found"}"#.to_string()));
            log.lock().unwrap().push(Seen {
                method,
                path,
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
            });
            // An answer "Header: v\x01body" sends extra headers.
            let (extra, answer) = match answer.split_once('\x01') {
                Some((extra, body)) => (format!("{extra}\r\n"), body.to_string()),
                None => (String::new(), answer),
            };
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (Host::at("fake.test", &base), seen)
}

fn header<'a>(seen: &'a Seen, name: &str) -> Option<&'a str> {
    seen.headers.iter().find_map(|h| {
        let (k, v) = h.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

#[test]
fn device_flow_polls_until_a_token_with_refresh_arrives() {
    let (host, seen) = server(vec![
        ("POST", "/login/device/code", 200, r#"{"device_code":"dc","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#.into()),
        ("POST", "/login/oauth/access_token", 200, r#"{"access_token":"ghu_x","token_type":"bearer","refresh_token":"ghr_y","expires_in":28800,"refresh_token_expires_in":15897600}"#.into()),
    ]);
    let client = Client::new(host, None);
    let code = client.device_code().unwrap();
    assert_eq!(code.user_code, "ABCD-1234");
    let Poll::Token(token) = client.poll_token(&code.device_code, 1000).unwrap() else {
        panic!()
    };
    assert_eq!(token.access_token, "ghu_x");
    assert_eq!(token.refresh_token.as_deref(), Some("ghr_y"));
    assert_eq!(token.expires_at, Some(1000 + 28800));
    let seen = seen.lock().unwrap();
    assert!(seen[0].body.contains(&format!("client_id={CLIENT_ID}")));
    assert!(seen[0].body.contains("scope=repo+read%3Aorg"));
    assert!(!seen[0].body.contains("secret"), "no client secret is sent");
    assert!(
        seen[1]
            .body
            .contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
    );
}

#[test]
fn poll_answers_pending_slow_down_expired_and_denied() {
    for (body, expected) in [
        (r#"{"error":"authorization_pending"}"#, Poll::Pending),
        (r#"{"error":"slow_down","interval":10}"#, Poll::SlowDown(10)),
        (r#"{"error":"expired_token"}"#, Poll::Expired),
        (r#"{"error":"access_denied"}"#, Poll::Denied),
        (r#"{"error":"device_flow_disabled"}"#, Poll::Disabled),
    ] {
        let (host, _) = server(vec![(
            "POST",
            "/login/oauth/access_token",
            200,
            body.into(),
        )]);
        assert_eq!(
            Client::new(host, None).poll_token("dc", 0).unwrap(),
            expected
        );
    }
}

#[test]
fn refresh_trades_the_refresh_token_without_a_secret() {
    let (host, seen) = server(vec![(
        "POST",
        "/login/oauth/access_token",
        200,
        r#"{"access_token":"ghu_new","refresh_token":"ghr_new","expires_in":100}"#.into(),
    )]);
    let token = Client::new(host.clone(), None)
        .refresh_token("ghr_old", 50)
        .unwrap();
    assert_eq!(token.access_token, "ghu_new");
    assert_eq!(token.expires_at, Some(150));
    let body = seen.lock().unwrap()[0].body.clone();
    assert!(body.contains("grant_type=refresh_token") && body.contains("refresh_token=ghr_old"));
    let (bad, _) = server(vec![(
        "POST",
        "/login/oauth/access_token",
        200,
        r#"{"error":"bad_refresh_token"}"#.into(),
    )]);
    assert_eq!(
        Client::new(bad, None).refresh_token("x", 0).unwrap_err(),
        ApiError::Unauthorized
    );
}

#[test]
fn token_secret_round_trip_and_expiry_rules() {
    let token = Token {
        access_token: "a".into(),
        refresh_token: Some("r".into()),
        expires_at: Some(1000),
        refresh_expires_at: Some(5000),
    };
    assert_eq!(Token::from_secret(&token.to_secret()), Some(token.clone()));
    assert!(!format!("{token:?}").contains("\"a\""));
    assert!(!token.needs_refresh(600));
    assert!(token.needs_refresh(701));
    assert!(!token.refresh_expired(4999));
    assert!(token.refresh_expired(5000));
    let bare = Token::from_secret("gho_plain").unwrap();
    assert!(bare.refresh_token.is_none() && !bare.needs_refresh(i64::MAX));
    assert!(Token::from_secret("").is_none());
}

#[test]
fn api_calls_send_the_token_and_read_answers() {
    let (host, seen) = server(vec![
        ("GET", "/user/repos", 200, r#"[{"name":"hello","full_name":"octo/hello","owner":{"login":"octo"},"private":true,"clone_url":"https://github.com/octo/hello.git","html_url":"https://github.com/octo/hello","permissions":{"push":false}}]"#.into()),
        ("GET", "/user", 200, r#"{"login":"octo","name":"Octo Cat"}"#.into()),
        ("POST", "/user/repos", 201, r#"{"name":"new","full_name":"octo/new","owner":{"login":"octo"},"clone_url":"https://github.com/octo/new.git","html_url":"https://github.com/octo/new"}"#.into()),
        ("POST", "/repos/up/proj/forks", 202, r#"{"name":"proj","full_name":"octo/proj","owner":{"login":"octo"},"fork":true,"clone_url":"https://github.com/octo/proj.git","html_url":"https://github.com/octo/proj"}"#.into()),
        ("GET", "/repos/octo/hello/pulls/7/reviews", 200, r#"[{"id":1,"user":{"login":"rev"},"state":"CHANGES_REQUESTED"}]"#.into()),
        ("GET", "/repos/octo/hello/pulls", 200, r#"[{"number":7,"title":"Add things","html_url":"https://github.com/octo/hello/pull/7","user":{"login":"octo"},"head":{"ref":"topic","sha":"abc"},"base":{"ref":"main","sha":"def"},"draft":false}]"#.into()),
        ("POST", "/repos/octo/hello/pulls", 201, r#"{"number":8,"title":"T","html_url":"u","user":{"login":"octo"},"head":{"ref":"x","sha":"1"},"base":{"ref":"main","sha":"2"}}"#.into()),
        ("POST", "/repos/octo/hello/issues", 201, r#"{"number":9,"html_url":"https://github.com/octo/hello/issues/9"}"#.into()),
    ]);
    let client = Client::new(host, Some("tok".into()));
    assert_eq!(client.user().unwrap().login, "octo");
    let repos = client.repositories().unwrap();
    assert_eq!(repos[0].full_name, "octo/hello");
    assert!(!repos[0].can_push());
    assert_eq!(
        client
            .create_repository("new", "d", true, None)
            .unwrap()
            .full_name,
        "octo/new"
    );
    assert!(client.fork("up", "proj").unwrap().fork);
    let pulls = client.pull_requests("octo", "hello").unwrap();
    assert_eq!((pulls[0].number, pulls[0].head.name.as_str()), (7, "topic"));
    assert_eq!(
        client
            .create_pull_request(
                "octo",
                "hello",
                &github_api::NewPull {
                    title: "T".into(),
                    body: "B".into(),
                    head: "x".into(),
                    base: "main".into(),
                    draft: false
                }
            )
            .unwrap()
            .number,
        8
    );
    assert_eq!(
        client
            .create_issue("octo", "hello", "Bug", "Body")
            .unwrap()
            .number,
        9
    );
    assert_eq!(
        client.reviews("octo", "hello", 7).unwrap()[0].state,
        "CHANGES_REQUESTED"
    );
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter()
            .all(|s| header(s, "authorization") == Some("Bearer tok"))
    );
    let create = seen
        .iter()
        .find(|s| s.method == "POST" && s.path == "/user/repos")
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&create.body).unwrap();
    assert_eq!(body["private"], true);
}

#[test]
fn checks_combine_and_rerun_the_failed_ones() {
    let (host, seen) = server(vec![
        ("GET", "/repos/o/r/commits/main/check-runs", 200, r#"{"check_runs":[
            {"id":1,"name":"build","status":"completed","conclusion":"failure","details_url":"https://github.com/o/r/actions/runs/42/job/1","check_suite":{"id":5}},
            {"id":2,"name":"test","status":"completed","conclusion":"failure","details_url":"https://github.com/o/r/actions/runs/42/job/2","check_suite":{"id":5}},
            {"id":3,"name":"other app","status":"completed","conclusion":"timed_out","details_url":"https://ci.example/x","check_suite":{"id":9}},
            {"id":4,"name":"lint","status":"in_progress"}]}"#.into()),
        ("GET", "/repos/o/r/commits/main/status", 200, r#"{"statuses":[{"id":1,"context":"legacy","state":"success"}]}"#.into()),
        ("POST", "/repos/o/r/actions/runs/42/rerun-failed-jobs", 201, "".into()),
        ("POST", "/repos/o/r/check-suites/9/rerequest", 201, "".into()),
    ]);
    let client = Client::new(host, Some("t".into()));
    let checks = client.checks("o", "r", "main").unwrap();
    assert_eq!(checks.overall(), Some(CheckState::Failure));
    assert_eq!(checks.failed().len(), 3);
    assert_eq!(client.rerun_failed("o", "r", &checks).unwrap(), 2);
    let posts: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.method == "POST")
        .map(|s| s.path.clone())
        .collect();
    assert_eq!(
        posts,
        [
            "/repos/o/r/actions/runs/42/rerun-failed-jobs",
            "/repos/o/r/check-suites/9/rerequest"
        ]
    );
}

#[test]
fn errors_are_typed() {
    let (host, _) = server(vec![
        (
            "GET",
            "/user",
            401,
            r#"{"message":"Bad credentials"}"#.into(),
        ),
        (
            "GET",
            "/repos/o/r",
            403,
            r#"{"message":"Resource not accessible"}"#.into(),
        ),
    ]);
    let client = Client::new(host, Some("t".into()));
    assert_eq!(client.user().unwrap_err(), ApiError::Unauthorized);
    assert_eq!(
        client.repository("o", "r").unwrap_err(),
        ApiError::Status {
            status: 403,
            message: "Resource not accessible".into()
        }
    );
    let nowhere = Client::new(Host::at("x", "http://127.0.0.1:9"), None);
    assert!(matches!(nowhere.user(), Err(ApiError::Network(_))));
}

#[test]
fn enterprise_hosts() {
    let host = Host::enterprise("https://ghe.example.com/").unwrap();
    assert_eq!(host.api, "https://ghe.example.com/api/v3");
    assert_eq!(host.name, "ghe.example.com");
    assert_eq!(Host::enterprise("github.com").unwrap(), Host::github());
    assert!(Host::enterprise("http://insecure.example").is_none());
    assert!(Host::enterprise("not a host").is_none());
    assert!(Host::enterprise("").is_none());
}

// --- independent review fixes -------------------------------------------

#[test]
fn lists_are_read_page_by_page() {
    let page = |from: usize, n: usize| {
        let items: Vec<String> = (from..from + n)
            .map(|i| format!(r#"{{"login":"org{i}"}}"#))
            .collect();
        format!("[{}]", items.join(","))
    };
    let runs = |n: usize| {
        let items: Vec<String> = (0..n)
            .map(|i| format!(r#"{{"id":{i},"name":"c{i}","status":"completed","conclusion":"success","html_url":"","details_url":""}}"#))
            .collect();
        format!(
            r#"{{"total_count":150,"check_runs":[{}]}}"#,
            items.join(",")
        )
    };
    let (host, seen) = server(vec![
        ("GET", "/user/orgs?per_page=100&page=1", 200, page(0, 100)),
        ("GET", "/user/orgs?per_page=100&page=2", 200, page(100, 3)),
        (
            "GET",
            "/repos/o/r/commits/main/check-runs?per_page=100&page=1",
            200,
            runs(100),
        ),
        (
            "GET",
            "/repos/o/r/commits/main/check-runs?per_page=100&page=2",
            200,
            runs(50),
        ),
        (
            "GET",
            "/repos/o/r/commits/main/status?per_page=100&page=1",
            200,
            r#"{"statuses":[]}"#.into(),
        ),
    ]);
    let client = Client::new(host, Some("t".into()));
    let orgs = client.organizations().unwrap();
    assert_eq!(orgs.len(), 103);
    let checks = client.checks("o", "r", "main").unwrap();
    assert_eq!(checks.runs.len(), 150);
    assert_eq!(
        seen.lock()
            .unwrap()
            .iter()
            .filter(|s| s.path.starts_with("/user/orgs"))
            .count(),
        2
    );
}

#[test]
fn a_rate_limit_is_its_own_error_with_when_to_retry() {
    let (host, _) = server(vec![
        ("GET", "/user/orgs", 403, "x-ratelimit-remaining: 0\r\nretry-after: 120\x01{\"message\":\"API rate limit exceeded\"}".into()),
        ("GET", "/user", 429, "retry-after: 30\x01{}".into()),
        ("GET", "/repos/o/r", 403, r#"{"message":"Resource not accessible by integration"}"#.into()),
    ]);
    let client = Client::new(host, Some("t".into()));
    assert_eq!(
        client.organizations().unwrap_err(),
        ApiError::RateLimited {
            retry_after: Some(120)
        }
    );
    assert_eq!(
        client.user().unwrap_err(),
        ApiError::RateLimited {
            retry_after: Some(30)
        }
    );
    assert!(matches!(
        client.repository("o", "r").unwrap_err(),
        ApiError::Status { status: 403, .. }
    ));
}

#[test]
fn an_enterprise_host_signs_in_with_its_own_client_id() {
    let (host, seen) = server(vec![(
        "POST",
        "/login/device/code",
        200,
        r#"{"device_code":"dc","user_code":"U","verification_uri":"https://x/login/device","expires_in":900,"interval":5}"#.into(),
    )]);
    let host = host.with_client_id("Iv1.enterprise");
    Client::new(host.clone(), None).device_code().unwrap();
    assert!(
        seen.lock().unwrap()[0]
            .body
            .contains("client_id=Iv1.enterprise")
    );
    assert_eq!(host.clone().with_client_id("  ").client_id(), CLIENT_ID);
}
