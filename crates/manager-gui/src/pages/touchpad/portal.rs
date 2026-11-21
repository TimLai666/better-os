//! Picking the file a gesture profile is exported to or imported from,
//! through the desktop's file chooser portal
//! (`org.freedesktop.portal.FileChooser`, version 1 and later).
//!
//! The call is the portal's documented request pattern: subscribe to the
//! `Response` signal on the request object the handle token names, then call
//! `SaveFile` or `OpenFile`, then wait for the person to choose or cancel.
//! It blocks, so the page runs it off the UI thread. When the portal does not
//! answer at all (no `xdg-desktop-portal`, no session bus) the page keeps the
//! path text field it had before, which is the fallback the ticket asks for.
//!
//! What comes back is a `file://` URI. Anything else (a portal that hands
//! back a `document` URI from a sandbox, or no URI) is refused rather than
//! guessed into a path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const PORTAL_NAME: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const FILE_CHOOSER: &str = "org.freedesktop.portal.FileChooser";
const REQUEST: &str = "org.freedesktop.portal.Request";

/// Which dialog to ask for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Pick {
    /// Where to write a new file: the suggested name and folder.
    Save { name: String, folder: PathBuf },
    /// An existing file to read.
    Open { folder: PathBuf },
}

/// What the person did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Picked {
    Chosen(PathBuf),
    /// They closed the dialog. Nothing happens.
    Cancelled,
    /// The portal answered with something that is not a local file.
    NotAFile(String),
    /// There is no portal to ask; the text field is the way in.
    Unavailable(String),
}

/// The portal's response code (`org.freedesktop.portal.Request::Response`):
/// 0 is success, 1 is cancelled by the person, 2 is any other end.
pub(crate) fn outcome(response: u32, uris: &[String]) -> Picked {
    match response {
        0 => match uris.first() {
            Some(uri) => match path_of(uri) {
                Some(path) => Picked::Chosen(path),
                None => Picked::NotAFile(uri.clone()),
            },
            None => Picked::NotAFile(String::new()),
        },
        1 => Picked::Cancelled,
        other => Picked::Unavailable(format!("the file chooser ended with response {other}")),
    }
}

/// A local path from a `file://` URI, percent-decoded. A URI with a host
/// other than `localhost`, or any other scheme, is not a local file.
pub(crate) fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    use std::os::unix::ffi::OsStringExt as _;
    let path = PathBuf::from(std::ffi::OsString::from_vec(decoded));
    // A NUL inside a path cannot name a file.
    (!path.as_os_str().as_encoded_bytes().contains(&0)).then_some(path)
}

/// The request object the portal will answer on, for this connection and
/// token: `/org/freedesktop/portal/desktop/request/SENDER/TOKEN`, where
/// SENDER is the unique name without its colon and with dots as
/// underscores.
pub(crate) fn request_path(unique_name: &str, token: &str) -> String {
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    format!("{PORTAL_PATH}/request/{sender}/{token}")
}

/// A folder as the portal takes it: a NUL-terminated byte array.
pub(crate) fn folder_bytes(folder: &Path) -> Vec<u8> {
    let mut bytes = folder.as_os_str().as_encoded_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Asks the session's portal, and waits for the answer. Blocking.
pub(crate) fn pick(pick: &Pick, title: &str, accept: &str) -> Picked {
    pick_on(Connection::session(), pick, title, accept)
}

/// The same, on a connection the caller made: the session bus, or a test's
/// private one.
pub(crate) fn pick_on(
    connection: zbus::Result<Connection>,
    pick: &Pick,
    title: &str,
    accept: &str,
) -> Picked {
    match connection.and_then(|connection| ask(&connection, pick, title, accept)) {
        Ok(picked) => picked,
        Err(error) => Picked::Unavailable(error.to_string()),
    }
}

fn ask(connection: &Connection, pick: &Pick, title: &str, accept: &str) -> zbus::Result<Picked> {
    let connection = connection.clone();
    let unique = connection
        .unique_name()
        .map(|name| name.to_string())
        .unwrap_or_default();
    let token = format!("better_manager_{}", std::process::id());
    let request = request_path(&unique, &token);
    // Subscribe first: a portal may answer before the call returns.
    let request_proxy = Proxy::new(&connection, PORTAL_NAME, request.as_str(), REQUEST)?;
    let mut responses = request_proxy.receive_signal("Response")?;

    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("modal", Value::from(true));
    options.insert("accept_label", Value::from(accept));
    let chooser = Proxy::new(&connection, PORTAL_NAME, PORTAL_PATH, FILE_CHOOSER)?;
    let method = match pick {
        Pick::Save { name, folder } => {
            options.insert("current_name", Value::from(name.as_str()));
            options.insert("current_folder", Value::from(folder_bytes(folder)));
            "SaveFile"
        }
        Pick::Open { folder } => {
            options.insert("current_folder", Value::from(folder_bytes(folder)));
            "OpenFile"
        }
    };
    let handle: OwnedObjectPath = chooser.call(method, &("", title, options))?;
    // A portal older than version 0.9 picks its own handle; listen there.
    if handle.as_str() != request {
        let proxy = Proxy::new(&connection, PORTAL_NAME, handle.as_str(), REQUEST)?;
        responses = proxy.receive_signal("Response")?;
    }
    let Some(message) = responses.next() else {
        return Ok(Picked::Unavailable(
            "the file chooser closed without answering".to_string(),
        ));
    };
    let (response, results): (u32, HashMap<String, OwnedValue>) = message.body().deserialize()?;
    let uris: Vec<String> = results
        .get("uris")
        .and_then(|value| Vec::<String>::try_from(value.clone()).ok())
        .unwrap_or_default();
    Ok(outcome(response, &uris))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_uri_becomes_its_path_and_anything_else_does_not() {
        assert_eq!(
            path_of("file:///home/a/gestures-export.json"),
            Some(PathBuf::from("/home/a/gestures-export.json"))
        );
        assert_eq!(
            path_of("file:///home/a/%E6%89%8B%E5%8B%A2%20copy.json"),
            Some(PathBuf::from("/home/a/手勢 copy.json"))
        );
        assert_eq!(
            path_of("file://localhost/tmp/x.json"),
            Some(PathBuf::from("/tmp/x.json"))
        );
        assert_eq!(path_of("file://server/share/x.json"), None);
        assert_eq!(path_of("https://example.com/x.json"), None);
        assert_eq!(path_of("file:///bad%2"), None);
        assert_eq!(path_of("file:///bad%zz"), None);
        assert_eq!(path_of("file:///nul%00here"), None);
    }

    #[test]
    fn the_response_code_decides_what_happens() {
        let uri = vec!["file:///tmp/a.json".to_string()];
        assert_eq!(
            outcome(0, &uri),
            Picked::Chosen(PathBuf::from("/tmp/a.json"))
        );
        assert_eq!(outcome(1, &uri), Picked::Cancelled);
        assert!(matches!(outcome(2, &uri), Picked::Unavailable(_)));
        assert_eq!(outcome(0, &[]), Picked::NotAFile(String::new()));
        assert_eq!(
            outcome(0, &["document:///x".to_string()]),
            Picked::NotAFile("document:///x".to_string())
        );
    }

    #[test]
    fn the_request_path_follows_the_portal_rule() {
        assert_eq!(
            request_path(":1.42", "better_manager_7"),
            "/org/freedesktop/portal/desktop/request/1_42/better_manager_7"
        );
        assert_eq!(folder_bytes(Path::new("/tmp")), b"/tmp\0".to_vec());
    }

    /// No portal to ask is not a crash: it is the text field.
    #[test]
    fn no_bus_means_the_text_field() {
        let connection = zbus::blocking::connection::Builder::address(
            "unix:path=/nonexistent/better-manager-test-bus",
        )
        .and_then(|builder| builder.build());
        let picked = pick_on(
            connection,
            &Pick::Open {
                folder: PathBuf::from("/tmp"),
            },
            "t",
            "a",
        );
        assert!(matches!(picked, Picked::Unavailable(_)), "{picked:?}");
    }

    struct FakePortal {
        /// What the fake answers: the response code and the URIs.
        response: u32,
        uris: Vec<String>,
        /// What it was asked, for the assertions.
        asked: std::sync::mpsc::Sender<(String, String, Vec<u8>)>,
    }

    #[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
    impl FakePortal {
        #[zbus(name = "SaveFile")]
        async fn save_file(
            &self,
            #[zbus(connection)] connection: &zbus::Connection,
            #[zbus(header)] header: zbus::message::Header<'_>,
            _parent: &str,
            title: &str,
            options: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            self.answer(connection, &header, "SaveFile", title, options)
                .await
        }

        #[zbus(name = "OpenFile")]
        async fn open_file(
            &self,
            #[zbus(connection)] connection: &zbus::Connection,
            #[zbus(header)] header: zbus::message::Header<'_>,
            _parent: &str,
            title: &str,
            options: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            self.answer(connection, &header, "OpenFile", title, options)
                .await
        }
    }

    impl FakePortal {
        async fn answer(
            &self,
            connection: &zbus::Connection,
            header: &zbus::message::Header<'_>,
            method: &str,
            title: &str,
            options: HashMap<String, OwnedValue>,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            let token = options
                .get("handle_token")
                .and_then(|value| String::try_from(value.clone()).ok())
                .unwrap_or_default();
            let folder = options
                .get("current_folder")
                .and_then(|value| Vec::<u8>::try_from(value.clone()).ok())
                .unwrap_or_default();
            let sender = header
                .sender()
                .map(|name| name.to_string())
                .unwrap_or_default();
            let path = request_path(&sender, &token);
            let _ = self
                .asked
                .send((method.to_string(), title.to_string(), folder));
            let mut results: HashMap<&str, Value<'_>> = HashMap::new();
            results.insert("uris", Value::from(self.uris.clone()));
            connection
                .emit_signal(
                    None::<()>,
                    path.as_str(),
                    REQUEST,
                    "Response",
                    &(self.response, results),
                )
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
            OwnedObjectPath::try_from(path)
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
        }
    }

    /// The real call, against a fake portal on a private bus: the dialog is
    /// asked for with the title, the folder, and a token, and the path the
    /// person chose comes back; a cancel comes back as a cancel.
    #[test]
    fn the_portal_is_asked_and_its_answer_is_the_path() {
        let private = test_prerequisites::require!(
            "dbus-daemon",
            crate::first_call_tests::PrivateBus::start()
                .ok_or("a private dbus-daemon did not start")
        );
        for (response, uris, expected) in [
            (
                0,
                vec!["file:///tmp/picked%20one.json".to_string()],
                Picked::Chosen(PathBuf::from("/tmp/picked one.json")),
            ),
            (1, Vec::new(), Picked::Cancelled),
        ] {
            let (asked, questions) = std::sync::mpsc::channel();
            let _portal = zbus::blocking::connection::Builder::address(private.address.as_str())
                .unwrap()
                .name(PORTAL_NAME)
                .unwrap()
                .serve_at(
                    PORTAL_PATH,
                    FakePortal {
                        response,
                        uris,
                        asked,
                    },
                )
                .unwrap()
                .build()
                .unwrap();
            let client = zbus::blocking::connection::Builder::address(private.address.as_str())
                .and_then(|builder| builder.build());
            let picked = pick_on(
                client,
                &Pick::Save {
                    name: "gestures-export.json".to_string(),
                    folder: PathBuf::from("/tmp"),
                },
                "Export gestures",
                "Export",
            );
            assert_eq!(picked, expected);
            let (method, title, folder) = questions.try_recv().unwrap();
            assert_eq!(method, "SaveFile");
            assert_eq!(title, "Export gestures");
            assert_eq!(folder, b"/tmp\0".to_vec());
        }
    }
}
