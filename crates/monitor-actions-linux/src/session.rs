//! Reading what the desktop session stands on, for the Force stop guard.
//!
//! Everything here reads files the user may read: `/proc/net/unix` and each
//! of the user's own processes' descriptors for who holds the session's
//! listening sockets, `/proc/<pid>/cgroup` for the unit systemd runs a
//! process in, and logind's record under `/run/systemd` for the display
//! session's leader. The decision is `monitor_core::session::session_role`.
//!
//! logind marks its files under `/run/systemd/users` and `/run/systemd/
//! sessions` as private ("Do not parse"). They are read here because this
//! crate links nothing that speaks D-Bus; a file that cannot be read, or
//! whose format changed, is an unknown, and an unknown never makes a
//! matching process safe to kill (`monitor_core::session::NAMED`).

use std::path::{Path, PathBuf};

use monitor_core::session::{SessionFacts, SessionMember};

/// Where the session's sockets are, as the window's environment says.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionEnv {
    pub runtime_dir: Option<PathBuf>,
    pub wayland_display: Option<String>,
    pub bus_address: Option<String>,
    pub x_display: Option<String>,
}

impl SessionEnv {
    /// The window's own environment.
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        Self {
            runtime_dir: var("XDG_RUNTIME_DIR").map(PathBuf::from),
            wayland_display: var("WAYLAND_DISPLAY"),
            bus_address: var("DBUS_SESSION_BUS_ADDRESS"),
            x_display: var("DISPLAY"),
        }
    }

    /// The session bus's socket: a path, or `@name` for an abstract one.
    pub fn bus_socket(&self) -> Option<String> {
        match &self.bus_address {
            Some(address) => address.split(';').find_map(|entry| {
                let rest = entry.strip_prefix("unix:")?;
                rest.split(',')
                    .find_map(|pair| match pair.split_once('=')? {
                        ("path", path) => Some(path.to_string()),
                        ("abstract", name) => Some(format!("@{name}")),
                        _ => None,
                    })
            }),
            None => self
                .runtime_dir
                .as_ref()
                .map(|dir| dir.join("bus").to_string_lossy().into_owned()),
        }
    }

    /// The Wayland display's socket, `None` when the session names none.
    pub fn wayland_socket(&self) -> Option<String> {
        let display = self.wayland_display.as_ref()?;
        if display.starts_with('/') {
            return Some(display.clone());
        }
        let dir = self.runtime_dir.as_ref()?;
        Some(dir.join(display).to_string_lossy().into_owned())
    }

    /// The X display's sockets (the file and its abstract twin): `Some`
    /// empty when the session has no X display, `None` for a display that
    /// is not a local one.
    pub fn x_sockets(&self) -> Option<Vec<String>> {
        let Some(display) = &self.x_display else {
            return Some(Vec::new());
        };
        let number = display.strip_prefix(':')?.split('.').next()?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let path = format!("/tmp/.X11-unix/X{number}");
        Some(vec![format!("@{path}"), path])
    }
}

/// The inodes of the listening sockets bound to `wanted`, from the text of
/// `/proc/net/unix`.
pub fn listening_inodes(net_unix: &str, wanted: &str) -> Vec<u64> {
    /// `__SO_ACCEPTCON`: the socket is listening.
    const ACCEPTING: u32 = 0x0001_0000;
    net_unix
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let flags = u32::from_str_radix(fields.get(3)?, 16).ok()?;
            let inode = fields.get(6)?.parse().ok()?;
            let path = fields.get(7)?;
            (flags & ACCEPTING != 0 && *path == wanted).then_some(inode)
        })
        .collect()
}

/// Every process under `proc_root` holding a descriptor to one of `inodes`.
/// Only descriptors the reader may read are seen, which is every one of
/// the user's own processes.
pub fn socket_holders(proc_root: &Path, inodes: &[u64]) -> Vec<u32> {
    let wanted: Vec<String> = inodes
        .iter()
        .map(|inode| format!("socket:[{inode}]"))
        .collect();
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    let mut holders: Vec<u32> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_dir(proc_root.join(pid.to_string()).join("fd"))
                .into_iter()
                .flatten()
                .filter_map(|fd| fd.ok())
                .filter_map(|fd| std::fs::read_link(fd.path()).ok())
                .any(|link| wanted.iter().any(|w| link.as_os_str() == w.as_str()))
        })
        .collect();
    holders.sort_unstable();
    holders
}

/// The holders of the listening socket(s) at `paths`, `None` when no such
/// listening socket is found, so nothing can be proven about who holds it.
fn holders_of(proc_root: &Path, net_unix: Option<&str>, paths: &[String]) -> Option<Vec<u32>> {
    let net_unix = net_unix?;
    let inodes: Vec<u64> = paths
        .iter()
        .flat_map(|path| listening_inodes(net_unix, path))
        .collect();
    if inodes.is_empty() {
        return None;
    }
    Some(socket_holders(proc_root, &inodes))
}

/// One `KEY=value` from a logind record.
fn record_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(str::to_string)
}

/// logind's leader of the user's display session: `Some(None)` when the
/// user has no display session, `None` when the record cannot be read.
pub fn display_session_leader(run_root: &Path, uid: u32) -> Option<Option<u32>> {
    let user =
        std::fs::read_to_string(run_root.join("systemd/users").join(uid.to_string())).ok()?;
    let Some(session) = record_value(&user, "DISPLAY").filter(|value| !value.is_empty()) else {
        return Some(None);
    };
    if session.contains('/') {
        return None;
    }
    let record = std::fs::read_to_string(run_root.join("systemd/sessions").join(&session)).ok()?;
    let leader = record_value(&record, "LEADER")?.parse().ok()?;
    Some(Some(leader))
}

/// The cgroup path of a process (`0::` line), `None` when unreadable.
pub fn cgroup_of(proc_root: &Path, pid: u32) -> Option<String> {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join("cgroup"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(str::to_string)
}

/// Everything the guard reads about the session, at one moment.
pub fn read_facts(
    proc_root: &Path,
    run_root: &Path,
    env: &SessionEnv,
    uid: u32,
    monitor_ancestors: Vec<u32>,
    member: impl Fn(u32) -> Option<SessionMember>,
) -> SessionFacts {
    let net_unix = std::fs::read_to_string(proc_root.join("net/unix")).ok();
    let net_unix = net_unix.as_deref();
    let bus_holders = env
        .bus_socket()
        .and_then(|socket| holders_of(proc_root, net_unix, &[socket]));
    let compositor_holders = env
        .wayland_socket()
        .and_then(|socket| holders_of(proc_root, net_unix, &[socket]));
    let x_display_holders = match env.x_sockets() {
        Some(sockets) if sockets.is_empty() => Some(Vec::new()),
        Some(sockets) => holders_of(proc_root, net_unix, &sockets),
        None => None,
    };
    let xwayland_on_demand = compositor_holders.as_ref().is_some_and(|holders| {
        let members: Option<Vec<SessionMember>> = holders.iter().map(|pid| member(*pid)).collect();
        members.is_some_and(|members| monitor_core::session::xwayland_on_demand(&members, uid))
    });
    SessionFacts {
        uid,
        monitor_ancestors,
        bus_holders,
        compositor_holders,
        x_display_holders,
        xwayland_on_demand,
        session_leader: display_session_leader(run_root, uid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_bus_is_found_from_its_address_or_the_runtime_directory() {
        let mut env = SessionEnv {
            runtime_dir: Some("/run/user/1000".into()),
            bus_address: Some("unix:path=/run/user/1000/bus".into()),
            ..SessionEnv::default()
        };
        assert_eq!(env.bus_socket().as_deref(), Some("/run/user/1000/bus"));
        env.bus_address = Some("unix:abstract=/tmp/dbus-x,guid=1;tcp:host=a".into());
        assert_eq!(env.bus_socket().as_deref(), Some("@/tmp/dbus-x"));
        env.bus_address = None;
        assert_eq!(env.bus_socket().as_deref(), Some("/run/user/1000/bus"));
        env.runtime_dir = None;
        assert_eq!(env.bus_socket(), None);
    }

    #[test]
    fn the_display_sockets_follow_the_environment() {
        let env = SessionEnv {
            runtime_dir: Some("/run/user/1000".into()),
            wayland_display: Some("wayland-0".into()),
            x_display: Some(":0.0".into()),
            ..SessionEnv::default()
        };
        assert_eq!(
            env.wayland_socket().as_deref(),
            Some("/run/user/1000/wayland-0")
        );
        assert_eq!(
            env.x_sockets(),
            Some(vec![
                "@/tmp/.X11-unix/X0".into(),
                "/tmp/.X11-unix/X0".into()
            ])
        );
        let none = SessionEnv::default();
        assert_eq!(none.wayland_socket(), None, "no Wayland display: unknown");
        assert_eq!(none.x_sockets(), Some(vec![]), "no X display at all");
        let remote = SessionEnv {
            x_display: Some("host:10".into()),
            ..SessionEnv::default()
        };
        assert_eq!(remote.x_sockets(), None);
    }

    #[test]
    fn only_listening_sockets_at_the_path_count() {
        // Read from this desktop's /proc/net/unix on 2026-11-22: a listening
        // socket (flags 00010000, state 01) and a connected one.
        let text = "Num       RefCount Protocol Flags    Type St Inode Path\n\
            0000000000000000: 00000002 00000000 00010000 0001 01   927 /run/user/1000/bus\n\
            0000000000000000: 00000003 00000000 00000000 0001 03 20805 /run/user/1000/bus\n\
            0000000000000000: 00000002 00000000 00010000 0001 01 31750 /run/user/1000/wayland-0\n";
        assert_eq!(listening_inodes(text, "/run/user/1000/bus"), vec![927]);
        assert_eq!(
            listening_inodes(text, "/run/user/1000/wayland-0"),
            vec![31750]
        );
        assert!(listening_inodes(text, "/tmp/.X11-unix/X0").is_empty());
    }

    #[test]
    fn a_logind_record_names_the_display_sessions_leader() {
        let root = std::env::temp_dir().join(format!("monitor-logind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(display_session_leader(&root, 1000), None, "unreadable");
        std::fs::create_dir_all(root.join("systemd/users")).unwrap();
        std::fs::create_dir_all(root.join("systemd/sessions")).unwrap();
        std::fs::write(root.join("systemd/users/1000"), "NAME=tim\nSESSIONS=4\n").unwrap();
        assert_eq!(
            display_session_leader(&root, 1000),
            Some(None),
            "no display"
        );
        std::fs::write(root.join("systemd/users/1000"), "NAME=tim\nDISPLAY=2\n").unwrap();
        assert_eq!(display_session_leader(&root, 1000), None, "no session file");
        std::fs::write(
            root.join("systemd/sessions/2"),
            "# This is private data. Do not parse.\nUID=1000\nLEADER=2321\n",
        )
        .unwrap();
        assert_eq!(display_session_leader(&root, 1000), Some(Some(2321)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
