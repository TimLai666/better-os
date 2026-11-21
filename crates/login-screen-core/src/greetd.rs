//! greetd's IPC messages (`greetd-ipc(7)`, the same at 0.9.0 and `master`;
//! design/login-screen-spec.md section 5.2).
//!
//! Each message is a 32-bit length in native byte order followed by that many
//! bytes of UTF-8 JSON. The one request that carries a secret,
//! `post_auth_message_response`, is written straight into a [`GreetdMessage`]
//! (locked, self-wiping, never formatted) and never passes through a `String`
//! or `serde_json`; the requests without a secret, and the responses, use
//! `serde_json`.
//!
//! Nothing here opens the socket: the greeter (ticket 264) writes the bytes and
//! reads the replies.

use serde::Deserialize;

use crate::secret::{SECRET_CAPACITY, Secret, SecretBytes, SecretError};

/// The longest reply the greeter reads. greetd's replies are a type, an enum
/// and one PAM message; anything longer is refused before it is read.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// The environment `start_session` passes for `better-os-remember-login`
/// (spec section 3.6).
pub const SESSION_ENV: &str = "BETTER_OS_LOGIN_SESSION";
pub const LANGUAGE_ENV: &str = "BETTER_OS_LOGIN_LANGUAGE";

const RESPONSE_PREFIX: &[u8] = br#"{"type":"post_auth_message_response","response":""#;
const RESPONSE_SUFFIX: &[u8] = br#""}"#;
const EMPTY_RESPONSE: &[u8] = br#"{"type":"post_auth_message_response","response":null}"#;
const LENGTH_BYTES: usize = 4;

/// Room for the length, the JSON around the answer, and an answer whose every
/// byte needed the longest escape (`\u00XX`, six bytes).
pub const MESSAGE_CAPACITY: usize =
    LENGTH_BYTES + RESPONSE_PREFIX.len() + RESPONSE_SUFFIX.len() + 6 * SECRET_CAPACITY;

/// A framed `post_auth_message_response`, ready to write to greetd's socket.
/// Like [`Secret`] it is on locked pages, wipes itself when dropped, and
/// implements no formatting or copying trait.
pub struct GreetdMessage {
    bytes: SecretBytes<MESSAGE_CAPACITY>,
}

impl GreetdMessage {
    /// The whole frame: length, then JSON.
    pub fn frame_bytes(&self) -> &[u8] {
        self.bytes.as_bytes()
    }

    /// Wipes it now, right after it was written.
    pub fn wipe(&mut self) {
        self.bytes.wipe();
    }
}

fn push_escaped(out: &mut SecretBytes<MESSAGE_CAPACITY>, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in text.as_bytes() {
        let result = match byte {
            b'"' => out.push(b"\\\""),
            b'\\' => out.push(b"\\\\"),
            0x00..=0x1f => out.push(&[
                b'\\',
                b'u',
                b'0',
                b'0',
                HEX[usize::from(byte >> 4)],
                HEX[usize::from(byte & 0x0f)],
            ]),
            // UTF-8 above ASCII and printable ASCII are valid JSON as they are.
            _ => out.push(&[byte]),
        };
        result.expect("MESSAGE_CAPACITY holds the longest escaped answer");
    }
}

/// Builds `post_auth_message_response` for one prompt. `None` answers an
/// `info` or `error` message, which take no response.
pub fn post_auth_message_response(answer: Option<&Secret>) -> Result<GreetdMessage, SecretError> {
    let mut bytes = SecretBytes::<MESSAGE_CAPACITY>::try_new()?;
    let fits = "MESSAGE_CAPACITY holds the longest escaped answer";
    bytes.push(&[0; LENGTH_BYTES]).expect(fits);
    match answer {
        Some(answer) => {
            bytes.push(RESPONSE_PREFIX).expect(fits);
            push_escaped(&mut bytes, answer.expose_str());
            bytes.push(RESPONSE_SUFFIX).expect(fits);
        }
        None => bytes.push(EMPTY_RESPONSE).expect(fits),
    }
    let payload = u32::try_from(bytes.len() - LENGTH_BYTES).expect("the frame is small");
    bytes.overwrite(0, &payload.to_ne_bytes());
    Ok(GreetdMessage { bytes })
}

fn frame(json: &serde_json::Value) -> Vec<u8> {
    let payload = serde_json::to_vec(json).expect("a JSON value serializes");
    let length = u32::try_from(payload.len()).expect("a request is small");
    let mut framed = Vec::with_capacity(LENGTH_BYTES + payload.len());
    framed.extend_from_slice(&length.to_ne_bytes());
    framed.extend_from_slice(&payload);
    framed
}

/// `create_session` for an account name.
pub fn create_session(username: &str) -> Vec<u8> {
    frame(&serde_json::json!({ "type": "create_session", "username": username }))
}

/// `start_session` with the session's command and the environment that
/// carries the choice into the new session.
pub fn start_session(command: &[String], env: &[String]) -> Vec<u8> {
    frame(&serde_json::json!({ "type": "start_session", "cmd": command, "env": env }))
}

/// The `start_session` environment of spec section 3.6: the chosen session's
/// id and the language, for `better-os-remember-login` to write as the person.
pub fn session_environment(session_id: &str, language: Option<&str>) -> Vec<String> {
    let mut env = vec![format!("{SESSION_ENV}={session_id}")];
    if let Some(language) = language.filter(|language| !language.is_empty()) {
        env.push(format!("{LANGUAGE_ENV}={language}"));
    }
    env
}

/// `cancel_session`.
pub fn cancel_session() -> Vec<u8> {
    frame(&serde_json::json!({ "type": "cancel_session" }))
}

/// The two kinds of error greetd reports.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// PAM refused (a wrong password, an expired or locked account).
    AuthError,
    /// Anything else: a request out of order, a session that would not start.
    Error,
}

/// What PAM asks or tells, as greetd relays it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessageKind {
    /// A question whose answer is shown as it is typed (a user name).
    Visible,
    /// A question whose answer is hidden (a password).
    Secret,
    /// Information; answered with an empty response.
    Info,
    /// An error PAM shows; answered with an empty response.
    Error,
}

/// One reply from greetd.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error {
        error_type: ErrorKind,
        description: String,
    },
    AuthMessage {
        auth_message_type: AuthMessageKind,
        auth_message: String,
    },
}

/// A reply that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ResponseError {
    #[error("greetd's reply of {0} bytes is longer than {MAX_RESPONSE_BYTES}")]
    TooLong(usize),
    #[error("greetd's reply is not a message the greeter knows: {0}")]
    Malformed(#[from] serde_json::Error),
}

/// The payload length a reply's 4-byte header announces, refused when it is
/// longer than the greeter reads.
pub fn response_length(header: [u8; LENGTH_BYTES]) -> Result<usize, ResponseError> {
    let length = u32::from_ne_bytes(header) as usize;
    if length > MAX_RESPONSE_BYTES {
        return Err(ResponseError::TooLong(length));
    }
    Ok(length)
}

/// Parses a reply's payload (the bytes after the header).
pub fn parse_response(payload: &[u8]) -> Result<Response, ResponseError> {
    if payload.len() > MAX_RESPONSE_BYTES {
        return Err(ResponseError::TooLong(payload.len()));
    }
    Ok(serde_json::from_slice(payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(frame: &[u8]) -> &[u8] {
        let length = u32::from_ne_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(length, frame.len() - 4);
        &frame[4..]
    }

    fn value(frame: &[u8]) -> serde_json::Value {
        serde_json::from_slice(payload(frame)).unwrap()
    }

    #[test]
    fn the_answer_is_framed_and_escaped_as_json() {
        let mut secret = Secret::new();
        secret.push_str("pa\"ss\\wo\u{1}rd 密碼").unwrap();
        let message = post_auth_message_response(Some(&secret)).unwrap();
        let parsed = value(message.frame_bytes());
        assert_eq!(parsed["type"], "post_auth_message_response");
        assert_eq!(parsed["response"], "pa\"ss\\wo\u{1}rd 密碼");
    }

    #[test]
    fn an_info_message_is_answered_with_null() {
        let message = post_auth_message_response(None).unwrap();
        let parsed = value(message.frame_bytes());
        assert_eq!(parsed["response"], serde_json::Value::Null);
    }

    #[test]
    fn the_longest_answer_fits() {
        let mut secret = Secret::new();
        for _ in 0..SECRET_CAPACITY {
            secret.push_char('\u{1f}').unwrap();
        }
        let message = post_auth_message_response(Some(&secret)).unwrap();
        assert_eq!(message.frame_bytes().len(), MESSAGE_CAPACITY);
        let parsed = value(message.frame_bytes());
        assert_eq!(parsed["response"].as_str().unwrap().len(), SECRET_CAPACITY);
    }

    #[test]
    fn wiping_the_message_zeroes_it() {
        let mut secret = Secret::new();
        secret.push_str("hunter2").unwrap();
        let mut message = post_auth_message_response(Some(&secret)).unwrap();
        message.wipe();
        assert!(message.frame_bytes().is_empty());
        assert!(message.bytes.storage().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn requests_without_a_secret() {
        assert_eq!(
            value(&create_session("tim")),
            serde_json::json!({"type": "create_session", "username": "tim"})
        );
        assert_eq!(
            value(&cancel_session()),
            serde_json::json!({"type": "cancel_session"})
        );
        let env = session_environment("better-os", Some("zh_TW.UTF-8"));
        assert_eq!(
            env,
            [
                "BETTER_OS_LOGIN_SESSION=better-os",
                "BETTER_OS_LOGIN_LANGUAGE=zh_TW.UTF-8"
            ]
        );
        assert_eq!(
            session_environment("sway", None),
            ["BETTER_OS_LOGIN_SESSION=sway"]
        );
        assert_eq!(
            value(&start_session(&["sway".to_string()], &env)),
            serde_json::json!({"type": "start_session", "cmd": ["sway"], "env": env})
        );
    }

    #[test]
    fn every_reply_greetd_documents_is_read() {
        assert_eq!(
            parse_response(br#"{"type":"success"}"#).unwrap(),
            Response::Success
        );
        assert_eq!(
            parse_response(
                br#"{"type":"error","error_type":"auth_error","description":"Authentication failure"}"#
            )
            .unwrap(),
            Response::Error {
                error_type: ErrorKind::AuthError,
                description: "Authentication failure".into()
            }
        );
        for (kind, expected) in [
            ("visible", AuthMessageKind::Visible),
            ("secret", AuthMessageKind::Secret),
            ("info", AuthMessageKind::Info),
            ("error", AuthMessageKind::Error),
        ] {
            let text = format!(
                r#"{{"type":"auth_message","auth_message_type":"{kind}","auth_message":"Password: "}}"#
            );
            assert_eq!(
                parse_response(text.as_bytes()).unwrap(),
                Response::AuthMessage {
                    auth_message_type: expected,
                    auth_message: "Password: ".into()
                }
            );
        }
        assert!(matches!(
            parse_response(br#"{"type":"surprise"}"#),
            Err(ResponseError::Malformed(_))
        ));
    }

    #[test]
    fn an_oversized_reply_is_refused_before_it_is_read() {
        let header = u32::try_from(MAX_RESPONSE_BYTES + 1).unwrap().to_ne_bytes();
        assert!(matches!(
            response_length(header),
            Err(ResponseError::TooLong(_))
        ));
        assert_eq!(response_length(12u32.to_ne_bytes()).unwrap(), 12);
    }
}
