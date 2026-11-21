//! What `cosmic-comp` tells the program that started it.
//!
//! A process that starts `cosmic-comp` with `COSMIC_SESSION_SOCK` naming one
//! end of a socket pair gets one message on its end once the compositor is up
//! (`cosmic-comp` `src/session.rs`, `run_socket`; the framing is
//! `cosmic-comp-ipc` `write_msg_blocking`): a native-endian `u16` length and
//! that many bytes of JSON, `{"message":"set_env","variables":{...}}` with
//! `WAYLAND_DISPLAY` and, when XWayland runs, `DISPLAY`. This is how
//! `cosmic-session` starts it. The launcher takes those two variables and
//! nothing else from it.

use std::collections::BTreeMap;
use std::io::{self, Read};

use serde::Deserialize;

/// The variable naming the compositor's end of the socket pair.
pub const SOCKET_VARIABLE: &str = "COSMIC_SESSION_SOCK";

/// The only variables taken from the compositor.
pub const REPORTED: [&str; 2] = ["WAYLAND_DISPLAY", "DISPLAY"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "message")]
enum Message {
    SetEnv { variables: BTreeMap<String, String> },
}

/// Why the compositor's message could not be used.
#[derive(Debug)]
pub enum ReadError {
    /// The socket closed or failed before a whole message came.
    Io(io::Error),
    /// The bytes were not a message the launcher knows.
    Malformed(String),
    /// The message named no Wayland display.
    NoDisplay,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Io(error) => write!(formatter, "the compositor's socket: {error}"),
            ReadError::Malformed(why) => {
                write!(
                    formatter,
                    "the compositor sent a message the launcher does not know: {why}"
                )
            }
            ReadError::NoDisplay => write!(formatter, "the compositor named no Wayland display"),
        }
    }
}

/// Reads one message and returns the display variables in it.
pub fn read_ready<R: Read>(reader: &mut R) -> Result<BTreeMap<String, String>, ReadError> {
    let mut length = [0u8; 2];
    reader.read_exact(&mut length).map_err(ReadError::Io)?;
    let mut body = vec![0u8; u16::from_ne_bytes(length) as usize];
    reader.read_exact(&mut body).map_err(ReadError::Io)?;
    decode(&body)
}

/// The display variables of one message body.
pub fn decode(body: &[u8]) -> Result<BTreeMap<String, String>, ReadError> {
    let Message::SetEnv { variables } =
        serde_json::from_slice(body).map_err(|error| ReadError::Malformed(error.to_string()))?;
    let mut reported = BTreeMap::new();
    for name in REPORTED {
        if let Some(value) = variables.get(name).filter(|value| acceptable(name, value)) {
            reported.insert(name.to_string(), value.clone());
        }
    }
    if !reported.contains_key("WAYLAND_DISPLAY") {
        return Err(ReadError::NoDisplay);
    }
    Ok(reported)
}

/// A socket name in the runtime directory (`wayland-1`) or an X display
/// (`:0`); anything else is not a display the session can hand on.
fn acceptable(name: &str, value: &str) -> bool {
    if value.is_empty() || value.len() > 108 || value.chars().any(char::is_control) {
        return false;
    }
    match name {
        "WAYLAND_DISPLAY" => !value.contains('/'),
        "DISPLAY" => {
            value.starts_with(':')
                && value[1..]
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(json: &str) -> Vec<u8> {
        let mut bytes = (json.len() as u16).to_ne_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        bytes
    }

    #[test]
    fn the_compositors_message_gives_both_displays() {
        let bytes = frame(
            r#"{"message":"set_env","variables":{"WAYLAND_DISPLAY":"wayland-1","DISPLAY":":0"}}"#,
        );
        let reported = read_ready(&mut bytes.as_slice()).unwrap();
        assert_eq!(
            reported.get("WAYLAND_DISPLAY").map(String::as_str),
            Some("wayland-1")
        );
        assert_eq!(reported.get("DISPLAY").map(String::as_str), Some(":0"));
    }

    #[test]
    fn without_xwayland_there_is_no_display_and_that_is_fine() {
        let reported =
            decode(br#"{"message":"set_env","variables":{"WAYLAND_DISPLAY":"wayland-1"}}"#)
                .unwrap();
        assert_eq!(reported.len(), 1);
    }

    #[test]
    fn nothing_but_the_two_displays_is_taken() {
        let reported = decode(
            br#"{"message":"set_env","variables":{"WAYLAND_DISPLAY":"wayland-1","LD_PRELOAD":"/tmp/x.so"}}"#,
        )
        .unwrap();
        assert!(!reported.contains_key("LD_PRELOAD"));
    }

    #[test]
    fn a_display_that_is_not_one_is_refused() {
        assert!(matches!(
            decode(br#"{"message":"set_env","variables":{"WAYLAND_DISPLAY":"../x"}}"#),
            Err(ReadError::NoDisplay)
        ));
        let reported = decode(
            br#"{"message":"set_env","variables":{"WAYLAND_DISPLAY":"wayland-0","DISPLAY":"host:0"}}"#,
        )
        .unwrap();
        assert!(!reported.contains_key("DISPLAY"));
        let reported = decode(
            br#"{"message":"set_env","variables":{"WAYLAND_DISPLAY":"wayland-0","DISPLAY":":1.0"}}"#,
        )
        .unwrap();
        assert_eq!(reported.get("DISPLAY").map(String::as_str), Some(":1.0"));
    }

    #[test]
    fn a_short_or_unknown_message_is_an_error() {
        assert!(matches!(
            read_ready(&mut [5u8].as_slice()),
            Err(ReadError::Io(_))
        ));
        assert!(matches!(
            read_ready(&mut frame(r#"{"message":"set_env""#).as_slice()),
            Err(ReadError::Malformed(_))
        ));
        assert!(matches!(
            decode(br#"{"message":"quit"}"#),
            Err(ReadError::Malformed(_))
        ));
    }
}
