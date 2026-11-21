//! The person's logind sessions, asked over D-Bus (`org.freedesktop.login1`
//! on the system bus), for the session guard. Read off the window's
//! interface thread with the rest of the session facts.
//!
//! Only stable interface is used: `Manager.GetUser`, `Manager.GetSessionByPID`,
//! `User.Sessions`, `User.Display`, and each session's `Id`, `Leader`, `Type`
//! and `Class`. A call that fails makes the whole answer unknown (`None`),
//! never "no sessions": the guard then falls back to names.

use monitor_core::session::LoginSession;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

const SERVICE: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const USER: &str = "org.freedesktop.login1.User";
const SESSION: &str = "org.freedesktop.login1.Session";

/// The person's sessions from the system bus, `None` when logind could not
/// be asked.
pub(crate) fn read_sessions(uid: u32, own_pid: u32) -> Option<Vec<LoginSession>> {
    let connection = Connection::system().ok()?;
    read_sessions_on(&connection, uid, own_pid)
}

/// The same, on any connection (a test serves a fake logind on a private
/// one).
pub(crate) fn read_sessions_on(
    connection: &Connection,
    uid: u32,
    own_pid: u32,
) -> Option<Vec<LoginSession>> {
    let manager = Proxy::new(connection, SERVICE, MANAGER_PATH, MANAGER).ok()?;
    let user_path: OwnedObjectPath = manager.call("GetUser", &(uid,)).ok()?;
    let user = Proxy::new(connection, SERVICE, user_path, USER).ok()?;
    let listed: Vec<(String, OwnedObjectPath)> = user.get_property("Sessions").ok()?;
    let (display_id, _): (String, OwnedObjectPath) = user.get_property("Display").ok()?;
    // A window started by the user manager is in no session: that is an
    // answer ("none"), and the user's display session stands in for it.
    let own_path: Option<OwnedObjectPath> = manager.call("GetSessionByPID", &(own_pid,)).ok();
    let mut sessions = Vec::new();
    for (_, path) in listed {
        let session = Proxy::new(connection, SERVICE, path.clone(), SESSION).ok()?;
        let id: String = session.get_property("Id").ok()?;
        let leader: u32 = session.get_property("Leader").ok()?;
        let kind: String = session.get_property("Type").ok()?;
        let class: String = session.get_property("Class").ok()?;
        let own = match &own_path {
            Some(own) => *own == path,
            None => !display_id.is_empty() && id == display_id,
        };
        sessions.push(LoginSession {
            id,
            leader: (leader != 0).then_some(leader),
            graphical: is_graphical(&kind, &class),
            own,
        });
    }
    Some(sessions)
}

/// A graphical session of a person: logind's `Type` is a display protocol
/// and its `Class` is `user` (a greeter's session is `greeter`).
pub(crate) fn is_graphical(kind: &str, class: &str) -> bool {
    matches!(kind, "wayland" | "x11" | "mir") && class == "user"
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::interface;
    use zbus::zvariant::ObjectPath;

    struct FakeManager;

    #[interface(name = "org.freedesktop.login1.Manager")]
    impl FakeManager {
        #[zbus(name = "GetUser")]
        fn get_user(&self, uid: u32) -> zbus::fdo::Result<OwnedObjectPath> {
            assert_eq!(uid, 1000);
            Ok(ObjectPath::try_from("/org/freedesktop/login1/user/_1000")
                .unwrap()
                .into())
        }

        #[zbus(name = "GetSessionByPID")]
        fn get_session_by_pid(&self, pid: u32) -> zbus::fdo::Result<OwnedObjectPath> {
            if pid == 4242 {
                Ok(ObjectPath::try_from("/org/freedesktop/login1/session/_33")
                    .unwrap()
                    .into())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "PID does not belong to any known session".into(),
                ))
            }
        }
    }

    struct FakeUser;

    #[interface(name = "org.freedesktop.login1.User")]
    impl FakeUser {
        #[zbus(property, name = "Sessions")]
        fn sessions(&self) -> Vec<(String, OwnedObjectPath)> {
            ["2", "3", "c1"]
                .iter()
                .map(|id| {
                    let path = format!("/org/freedesktop/login1/session/_3{id}");
                    (id.to_string(), ObjectPath::try_from(path).unwrap().into())
                })
                .collect()
        }

        #[zbus(property, name = "Display")]
        fn display(&self) -> (String, OwnedObjectPath) {
            (
                "2".into(),
                ObjectPath::try_from("/org/freedesktop/login1/session/_32")
                    .unwrap()
                    .into(),
            )
        }
    }

    struct FakeSession {
        id: &'static str,
        leader: u32,
        kind: &'static str,
        class: &'static str,
    }

    #[interface(name = "org.freedesktop.login1.Session")]
    impl FakeSession {
        #[zbus(property, name = "Id")]
        fn id(&self) -> String {
            self.id.into()
        }
        #[zbus(property, name = "Leader")]
        fn leader(&self) -> u32 {
            self.leader
        }
        #[zbus(property, name = "Type")]
        fn kind(&self) -> String {
            self.kind.into()
        }
        #[zbus(property, name = "Class")]
        fn class(&self) -> String {
            self.class.into()
        }
    }

    /// A private bus: no service directory, so nothing is ever activated
    /// and the system's logind is never reached.
    struct PrivateBus {
        child: std::process::Child,
        address: String,
        directory: std::path::PathBuf,
    }

    impl PrivateBus {
        fn start() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let directory = std::env::temp_dir().join(format!(
                "bm-logind-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&directory);
            std::fs::create_dir_all(&directory).unwrap();
            let config = directory.join("bus.conf");
            std::fs::write(
                &config,
                format!(
                    "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                     \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                     <busconfig><type>session</type><listen>unix:dir={}</listen>\
                     <auth>EXTERNAL</auth>\
                     <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                     <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
                    directory.display()
                ),
            )
            .unwrap();
            let mut child = std::process::Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("dbus-daemon is needed: the CI image has it (package dbus)");
            use std::io::{BufRead, BufReader};
            let mut address = String::new();
            if let Some(stdout) = child.stdout.take() {
                let _ = BufReader::new(stdout).read_line(&mut address);
            }
            Self {
                child,
                address: address.trim().to_string(),
                directory,
            }
        }

        fn connect(&self) -> Connection {
            zbus::blocking::connection::Builder::address(self.address.as_str())
                .unwrap()
                .build()
                .unwrap()
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    /// A logind on a private bus: GNOME (session 2, the display), COSMIC
    /// (session 3) and a terminal login (c1).
    fn fake_logind(bus: &PrivateBus) -> Connection {
        zbus::blocking::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(MANAGER_PATH, FakeManager)
            .unwrap()
            .serve_at("/org/freedesktop/login1/user/_1000", FakeUser)
            .unwrap()
            .serve_at(
                "/org/freedesktop/login1/session/_32",
                FakeSession {
                    id: "2",
                    leader: 2321,
                    kind: "wayland",
                    class: "user",
                },
            )
            .unwrap()
            .serve_at(
                "/org/freedesktop/login1/session/_33",
                FakeSession {
                    id: "3",
                    leader: 3300,
                    kind: "wayland",
                    class: "user",
                },
            )
            .unwrap()
            .serve_at(
                "/org/freedesktop/login1/session/_3c1",
                FakeSession {
                    id: "c1",
                    leader: 900,
                    kind: "tty",
                    class: "user",
                },
            )
            .unwrap()
            .build()
            .unwrap()
    }

    #[test]
    fn the_persons_sessions_are_read_from_logind_with_the_windows_own() {
        let bus = PrivateBus::start();
        let _logind = fake_logind(&bus);
        let connection = bus.connect();
        // The window runs in COSMIC's session.
        let sessions = read_sessions_on(&connection, 1000, 4242).unwrap();
        assert_eq!(
            sessions,
            vec![
                LoginSession {
                    id: "2".into(),
                    leader: Some(2321),
                    graphical: true,
                    own: false
                },
                LoginSession {
                    id: "3".into(),
                    leader: Some(3300),
                    graphical: true,
                    own: true
                },
                LoginSession {
                    id: "c1".into(),
                    leader: Some(900),
                    graphical: false,
                    own: false
                },
            ]
        );
        // A window in no session (started by the user manager): the
        // display session stands in for its own.
        let sessions = read_sessions_on(&connection, 1000, 1).unwrap();
        let own: Vec<&str> = sessions
            .iter()
            .filter(|s| s.own)
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(own, ["2"]);
    }

    #[test]
    fn a_logind_that_does_not_answer_is_unknown_not_no_sessions() {
        let bus = PrivateBus::start();
        assert_eq!(read_sessions_on(&bus.connect(), 1000, 1), None);
    }

    #[test]
    fn only_a_persons_display_sessions_are_graphical() {
        assert!(is_graphical("wayland", "user"));
        assert!(is_graphical("x11", "user"));
        assert!(!is_graphical("wayland", "greeter"));
        assert!(!is_graphical("tty", "user"));
        assert!(!is_graphical("unspecified", "user"));
    }
}
