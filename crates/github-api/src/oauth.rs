//! The OAuth device flow: the person enters a code on GitHub's page while
//! Better Git polls for the token. Tokens that expire are refreshed with
//! the refresh token, no client secret involved.

use serde::Deserialize;

use crate::{ApiError, Client, SCOPES};

/// What `POST /login/device/code` answers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds until the code stops working.
    pub expires_in: u64,
    /// Seconds to wait between polls.
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// A token, with when it and its refresh token expire (seconds since the
/// epoch) when GitHub said they do.
#[derive(Clone, Eq, PartialEq)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub refresh_expires_at: Option<i64>,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("expires_at", &self.expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish_non_exhaustive()
    }
}

impl Token {
    /// Whether the access token should be refreshed before use: it expires
    /// within five minutes and there is a refresh token.
    pub fn needs_refresh(&self, now: i64) -> bool {
        self.refresh_token.is_some() && self.expires_at.is_some_and(|at| at - 300 <= now)
    }

    /// Whether signing in again is the only way: the refresh token itself
    /// has expired.
    pub fn refresh_expired(&self, now: i64) -> bool {
        self.refresh_expires_at.is_some_and(|at| at <= now)
    }

    /// The keyring secret: the fields as one JSON object.
    pub fn to_secret(&self) -> String {
        serde_json::json!({
            "access_token": self.access_token,
            "refresh_token": self.refresh_token,
            "expires_at": self.expires_at,
            "refresh_expires_at": self.refresh_expires_at,
        })
        .to_string()
    }

    /// Reads a keyring secret: the JSON object, or a bare token stored by
    /// an older build.
    pub fn from_secret(secret: &str) -> Option<Token> {
        let value: serde_json::Value = match serde_json::from_str(secret) {
            Ok(value) => value,
            Err(_) if !secret.trim().is_empty() && !secret.contains(char::is_whitespace) => {
                return Some(Token {
                    access_token: secret.trim().to_string(),
                    refresh_token: None,
                    expires_at: None,
                    refresh_expires_at: None,
                });
            }
            Err(_) => return None,
        };
        Some(Token {
            access_token: value.get("access_token")?.as_str()?.to_string(),
            refresh_token: value
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            expires_at: value.get("expires_at").and_then(|v| v.as_i64()),
            refresh_expires_at: value.get("refresh_expires_at").and_then(|v| v.as_i64()),
        })
    }
}

/// One answer while polling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Poll {
    Token(Token),
    /// Not entered yet; poll again after the interval.
    Pending,
    /// Polling too fast; wait this many seconds from now on.
    SlowDown(u64),
    /// The code expired; start again.
    Expired,
    /// The person declined.
    Denied,
    /// Device flow is off for this app.
    Disabled,
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    refresh_token_expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
    interval: Option<u64>,
}

impl TokenAnswer {
    fn token(self, now: i64) -> Option<Token> {
        Some(Token {
            access_token: self.access_token.filter(|t| !t.is_empty())?,
            refresh_token: self.refresh_token.filter(|t| !t.is_empty()),
            expires_at: self.expires_in.map(|s| now + s),
            refresh_expires_at: self.refresh_token_expires_in.map(|s| now + s),
        })
    }
}

impl Client {
    /// Starts sign-in.
    pub fn device_code(&self) -> Result<DeviceCode, ApiError> {
        self.post_form(
            "/login/device/code",
            &[("client_id", self.host.client_id()), ("scope", SCOPES)],
        )
    }

    /// Asks once whether the person entered the code. `now` is seconds
    /// since the epoch, for the expiry times.
    pub fn poll_token(&self, device_code: &str, now: i64) -> Result<Poll, ApiError> {
        let answer: TokenAnswer = self.post_form(
            "/login/oauth/access_token",
            &[
                ("client_id", self.host.client_id()),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        )?;
        Ok(match answer.error.as_deref() {
            None => match answer.token(now) {
                Some(token) => Poll::Token(token),
                None => return Err(ApiError::Unreadable("no access_token".into())),
            },
            Some("authorization_pending") => Poll::Pending,
            Some("slow_down") => Poll::SlowDown(answer.interval.unwrap_or(10)),
            Some("expired_token") => Poll::Expired,
            Some("access_denied") => Poll::Denied,
            Some("device_flow_disabled") => Poll::Disabled,
            Some(other) => {
                return Err(ApiError::Status {
                    status: 400,
                    message: answer
                        .error_description
                        .unwrap_or_else(|| other.to_string()),
                });
            }
        })
    }

    /// Trades a refresh token for a new token.
    pub fn refresh_token(&self, refresh_token: &str, now: i64) -> Result<Token, ApiError> {
        let answer: TokenAnswer = self.post_form(
            "/login/oauth/access_token",
            &[
                ("client_id", self.host.client_id()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ],
        )?;
        if let Some(error) = answer.error.clone() {
            return Err(if error == "bad_refresh_token" {
                ApiError::Unauthorized
            } else {
                ApiError::Status {
                    status: 400,
                    message: answer.error_description.unwrap_or(error),
                }
            });
        }
        answer
            .token(now)
            .ok_or_else(|| ApiError::Unreadable("no access_token".into()))
    }
}
