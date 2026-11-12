//! The parts of GitHub's REST API and OAuth device flow Better Git uses.
//!
//! Every call is a blocking request made from Better Git's background
//! thread, never the thread that paints. The base URLs come from [`Host`],
//! so GitHub.com, an Enterprise server, and the tests' local fake server
//! are the same code. Nothing here stores a token; the caller hands one in
//! and keeps it in the desktop keyring.

pub mod checks;
pub mod issues;
pub mod oauth;
pub mod pulls;
pub mod repos;

use std::io::Read as _;
use std::time::Duration;

use serde::de::DeserializeOwned;
use thiserror::Error;

pub use checks::{CheckRun, CheckState, CombinedChecks};
pub use issues::Issue;
pub use oauth::{DeviceCode, Poll, Token};
pub use pulls::{NewPull, PullRequest, Review};
pub use repos::{Organization, RepoInfo, User};

/// The OAuth App's client ID. The owner registered it; it is a public
/// identifier (device flow needs no secret, and there is none). A build can
/// name another with `BETTER_GIT_GITHUB_CLIENT_ID`.
pub const CLIENT_ID: &str = match option_env!("BETTER_GIT_GITHUB_CLIENT_ID") {
    Some(id) => id,
    None => "Ov23lihee8DuKVJr7S8i",
};

/// What Better Git asks the account for: its repositories, organizations,
/// and workflows (so checks can be re-run).
pub const SCOPES: &str = "repo read:org workflow notifications";

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: u64 = 16 * 1024 * 1024;

/// Where a GitHub lives: its web address (sign-in) and its API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Host {
    /// `github.com`, or the Enterprise host name; the keyring and git's
    /// credentials are keyed by it.
    pub name: String,
    pub web: String,
    pub api: String,
    /// The OAuth App this host's sign-in uses. An Enterprise server has its
    /// own apps, registered by its administrator; `None` is [`CLIENT_ID`],
    /// which is registered on GitHub.com.
    pub client_id: Option<String>,
}

impl Host {
    pub fn github() -> Host {
        Host {
            name: "github.com".into(),
            web: "https://github.com".into(),
            api: "https://api.github.com".into(),
            client_id: None,
        }
    }

    /// The same host signing in with another OAuth App; an empty ID keeps
    /// the default.
    pub fn with_client_id(mut self, id: &str) -> Host {
        let id = id.trim();
        self.client_id = (!id.is_empty()).then(|| id.to_string());
        self
    }

    /// The OAuth App's client ID sign-in and token refresh send.
    pub fn client_id(&self) -> &str {
        self.client_id.as_deref().unwrap_or(CLIENT_ID)
    }

    /// An Enterprise server from what the person typed: a host name or an
    /// `https://` URL. Anything else is `None`.
    pub fn enterprise(typed: &str) -> Option<Host> {
        let typed = typed.trim().trim_end_matches('/');
        let name = typed.strip_prefix("https://").unwrap_or(typed);
        if name.is_empty()
            || name.contains('/')
            || name.contains(char::is_whitespace)
            || typed.starts_with("http://")
            || !name.contains('.')
        {
            return None;
        }
        if name.eq_ignore_ascii_case("github.com") {
            return Some(Host::github());
        }
        Some(Host {
            name: name.to_ascii_lowercase(),
            web: format!("https://{name}"),
            api: format!("https://{name}/api/v3"),
            client_id: None,
        })
    }

    /// A host served at one base URL for both, for tests' local server.
    pub fn at(name: &str, base: &str) -> Host {
        Host {
            name: name.into(),
            web: base.trim_end_matches('/').into(),
            api: base.trim_end_matches('/').into(),
            client_id: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ApiError {
    /// The server could not be reached. The text is the client's.
    #[error("github.network:{0}")]
    Network(String),
    /// 401: the token is missing, expired, or revoked.
    #[error("github.unauthorized")]
    Unauthorized,
    /// Any other refusal, with GitHub's own `message`.
    #[error("github.status:{status}:{message}")]
    Status { status: u16, message: String },
    /// The answer was not the JSON expected.
    #[error("github.unreadable:{0}")]
    Unreadable(String),
    /// GitHub's rate limit (403 or 429 with no requests left, or a
    /// `Retry-After`). `retry_after` is in seconds, when GitHub said.
    #[error("github.rate_limited")]
    RateLimited { retry_after: Option<u64> },
}

/// How many pages of a list are read at most (100 items each).
pub(crate) const MAX_PAGES: usize = 10;

/// A client for one host, with or without a token.
#[derive(Clone)]
pub struct Client {
    pub host: Host,
    token: Option<String>,
    agent: ureq::Agent,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("host", &self.host).finish()
    }
}

impl Client {
    pub fn new(host: Host, token: Option<String>) -> Client {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .user_agent("Better-Git")
            .build()
            .into();
        Client { host, token, agent }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.host.api, path)
    }

    fn finish<T: DeserializeOwned>(
        response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<T, ApiError> {
        let response = response.map_err(|e| ApiError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .map(str::to_string)
        };
        let retry_after = header("retry-after").and_then(|v| v.parse::<u64>().ok());
        let remaining = header("x-ratelimit-remaining");
        let reset = header("x-ratelimit-reset").and_then(|v| v.parse::<i64>().ok());
        if matches!(status, 403 | 429)
            && (retry_after.is_some() || remaining.as_deref() == Some("0"))
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let retry_after =
                retry_after.or_else(|| reset.map(|reset| (reset - now).max(0) as u64));
            return Err(ApiError::RateLimited { retry_after });
        }
        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_BODY)
            .read_to_end(&mut body)
            .map_err(|e| ApiError::Network(e.to_string()))?;
        if status == 401 {
            return Err(ApiError::Unauthorized);
        }
        if !(200..300).contains(&status) {
            let message = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .and_then(|m| m.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());
            return Err(ApiError::Status { status, message });
        }
        if body.is_empty() {
            body = b"null".to_vec();
        }
        serde_json::from_slice(&body).map_err(|e| ApiError::Unreadable(e.to_string()))
    }

    pub(crate) fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let mut request = self
            .agent
            .get(self.api_url(path))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        Self::finish(request.call())
    }

    /// Every page of a list (`path` already has its query, with
    /// `per_page=100`), up to [`MAX_PAGES`]; `items` takes a page's answer
    /// apart.
    pub(crate) fn get_pages<P: DeserializeOwned, T>(
        &self,
        path: &str,
        items: impl Fn(P) -> Vec<T>,
    ) -> Result<Vec<T>, ApiError> {
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let batch = items(self.get(&format!("{path}&page={page}"))?);
            let done = batch.len() < 100;
            all.extend(batch);
            if done {
                break;
            }
        }
        Ok(all)
    }

    pub(crate) fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, ApiError> {
        let mut request = self
            .agent
            .post(self.api_url(path))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Content-Type", "application/json");
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        Self::finish(request.send(body.to_string()))
    }

    /// A form POST to the web host, as the OAuth endpoints want.
    pub(crate) fn post_form<T: DeserializeOwned>(
        &self,
        path: &str,
        fields: &[(&str, &str)],
    ) -> Result<T, ApiError> {
        let body: Vec<String> = fields
            .iter()
            .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
            .collect();
        let request = self
            .agent
            .post(format!("{}{}", self.host.web, path))
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded");
        Self::finish(request.send(body.join("&")))
    }
}

/// `application/x-www-form-urlencoded` for one value.
pub fn form_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// One path segment, percent-encoded for a URL.
pub(crate) fn segment(value: &str) -> String {
    form_encode(value).replace('+', "%20")
}
