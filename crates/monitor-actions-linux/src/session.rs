//! Reading what the desktop session stands on, for the session guard.
//!
//! Everything here reads files the user may read: `/proc/net/unix` once for
//! every listening socket the guard cares about, each of the user's own
//! processes' descriptors once (one pass for all sockets), and
//! `/proc/<pid>/stat` and `cgroup` for the processes scanned. logind is not
//! read here: this crate links nothing that speaks D-Bus, so the window asks
//! `org.freedesktop.login1` itself and hands the sessions in.
//!
//! A read is not cheap (about 20 ms over 600 processes and 4,400
//! descriptors on the owner's laptop), so the window does it off its
//! interface thread and keeps the result in its model; the controller's
//! `perform` does it again, fresh, right before it signals.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use monitor_core::session::{LoginSession, SessionFacts, SessionMember};

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

    /// The session bus's sockets: the window's address (a path, or `@name`
    /// for an abstract one) and the user bus in the runtime directory that
    /// every session of the person shares.
    pub fn bus_sockets(&self) -> Vec<String> {
        let mut sockets = Vec::new();
        if let Some(address) = &self.bus_address
            && let Some(socket) = address.split(';').find_map(|entry| {
                let rest = entry.strip_prefix("unix:")?;
                rest.split(',')
                    .find_map(|pair| match pair.split_once('=')? {
                        ("path", path) => Some(path.to_string()),
                        ("abstract", name) => Some(format!("@{name}")),
                        _ => None,
                    })
            })
        {
            sockets.push(socket);
        }
        if let Some(dir) = &self.runtime_dir {
            let user_bus = dir.join("bus").to_string_lossy().into_owned();
            if !sockets.contains(&user_bus) {
                sockets.push(user_bus);
            }
        }
        sockets
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

/// One listening socket from `/proc/net/unix`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listening {
    pub inode: u64,
    pub path: String,
}

/// Every listening socket with a path or an abstract name, from the text of
/// `/proc/net/unix`.
pub fn listening_sockets(net_unix: &str) -> Vec<Listening> {
    /// `__SO_ACCEPTCON`: the socket is listening.
    const ACCEPTING: u32 = 0x0001_0000;
    net_unix
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let flags = u32::from_str_radix(fields.get(3)?, 16).ok()?;
            let inode = fields.get(6)?.parse().ok()?;
            let path = fields.get(7)?.to_string();
            (flags & ACCEPTING != 0).then_some(Listening { inode, path })
        })
        .collect()
}

/// What the guard wants to know about, for one read.
#[derive(Clone, Debug, Default)]
pub struct ScanRequest {
    /// The user's own processes to scan, from the window's snapshot; `None`
    /// scans every process of the user `/proc` lists (what `perform` does).
    pub pids: Option<Vec<u32>>,
    /// The person's logind sessions as the window last read them.
    pub sessions: Option<Vec<LoginSession>>,
}

/// Something that reads [`SessionFacts`]. The window's interface thread
/// never calls one: a test gives the render path a reader that panics.
pub trait SessionReader: Send + Sync + std::fmt::Debug {
    fn read(&self, request: &ScanRequest) -> SessionFacts;
}

/// The reader against a `/proc` (the real one, or a test's scratch tree).
#[derive(Clone, Debug)]
pub struct ProcReader {
    pub proc_root: PathBuf,
    pub env: SessionEnv,
    pub uid: u32,
    pub own_pid: u32,
}

/// What one process's descriptors were found to hold.
#[derive(Default)]
struct Held {
    bus: BTreeSet<u32>,
    compositor: BTreeSet<u32>,
    x: BTreeSet<u32>,
    other_compositor: BTreeSet<u32>,
    other_x: BTreeSet<u32>,
}

#[derive(Clone, Copy)]
enum Kind {
    Bus,
    Compositor,
    X,
    OtherCompositor,
    OtherX,
}

impl ProcReader {
    /// Read `stat` and `cgroup` of one process.
    pub fn member(&self, pid: u32) -> Option<SessionMember> {
        let text =
            std::fs::read_to_string(self.proc_root.join(pid.to_string()).join("stat")).ok()?;
        let (name, parent, _, _) = crate::parse_stat(&text)?;
        Some(SessionMember {
            pid,
            name,
            parent,
            cgroup: cgroup_of(&self.proc_root, pid),
        })
    }

    /// The processes above this one.
    pub fn ancestors(&self) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut at = self.own_pid;
        while let Some(parent) = self.member(at).and_then(|member| member.parent) {
            if parent == 0 || chain.contains(&parent) || chain.len() > 4096 {
                break;
            }
            chain.push(parent);
            at = parent;
        }
        chain
    }

    /// The user's processes `/proc` lists now.
    fn own_processes(&self) -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir(&self.proc_root) else {
            return Vec::new();
        };
        entries
            .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| crate::real_uid(&self.proc_root, *pid) == Some(self.uid))
            .collect()
    }

    /// Which socket each wanted inode is, by the environment's sockets.
    fn wanted(&self, listening: &[Listening]) -> BTreeMap<u64, Kind> {
        let bus = self.env.bus_sockets();
        let own_wayland = self.env.wayland_socket();
        let own_x = self.env.x_sockets().unwrap_or_default();
        let runtime = self
            .env
            .runtime_dir
            .as_ref()
            .map(|dir| dir.to_string_lossy().into_owned());
        let mut wanted = BTreeMap::new();
        for socket in listening {
            let path = socket.path.as_str();
            let kind = if bus.iter().any(|b| b == path) {
                Kind::Bus
            } else if own_wayland.as_deref() == Some(path) {
                Kind::Compositor
            } else if own_x.iter().any(|x| x == path) {
                Kind::X
            } else if runtime.as_deref().is_some_and(|dir| {
                path.strip_prefix(dir)
                    .and_then(|rest| rest.strip_prefix("/wayland-"))
                    .is_some_and(|name| !name.contains('/'))
            }) {
                Kind::OtherCompositor
            } else if path
                .trim_start_matches('@')
                .strip_prefix("/tmp/.X11-unix/X")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            {
                Kind::OtherX
            } else {
                continue;
            };
            wanted.insert(socket.inode, kind);
        }
        wanted
    }
}

impl SessionReader for ProcReader {
    fn read(&self, request: &ScanRequest) -> SessionFacts {
        let net_unix = std::fs::read_to_string(self.proc_root.join("net/unix")).ok();
        let listening = net_unix.as_deref().map(listening_sockets);
        let wanted = listening
            .as_deref()
            .map(|listening| self.wanted(listening))
            .unwrap_or_default();
        let pids = request.pids.clone().unwrap_or_else(|| self.own_processes());

        // One pass over the descriptors of every process scanned.
        let mut held = Held::default();
        let mut fds_read = BTreeSet::new();
        let mut members = BTreeMap::new();
        for pid in pids {
            if let Some(member) = self.member(pid) {
                members.insert(pid, member);
            }
            let Ok(fds) = std::fs::read_dir(self.proc_root.join(pid.to_string()).join("fd")) else {
                continue;
            };
            fds_read.insert(pid);
            for fd in fds.flatten() {
                let Ok(link) = std::fs::read_link(fd.path()) else {
                    continue;
                };
                let Some(inode) = link
                    .to_str()
                    .and_then(|l| l.strip_prefix("socket:["))
                    .and_then(|l| l.strip_suffix(']'))
                    .and_then(|l| l.parse::<u64>().ok())
                else {
                    continue;
                };
                let set = match wanted.get(&inode) {
                    Some(Kind::Bus) => &mut held.bus,
                    Some(Kind::Compositor) => &mut held.compositor,
                    Some(Kind::X) => &mut held.x,
                    Some(Kind::OtherCompositor) => &mut held.other_compositor,
                    Some(Kind::OtherX) => &mut held.other_x,
                    None => continue,
                };
                set.insert(pid);
            }
        }

        // A socket that was not found at all leaves nothing proven.
        let found = |paths: &[String]| {
            listening
                .as_ref()
                .is_some_and(|l| l.iter().any(|s| paths.contains(&s.path)))
        };
        let list = |set: BTreeSet<u32>| set.into_iter().collect::<Vec<_>>();
        let bus_holders = found(&self.env.bus_sockets()).then(|| list(held.bus));
        let compositor_holders = self
            .env
            .wayland_socket()
            .filter(|socket| found(std::slice::from_ref(socket)))
            .map(|_| list(held.compositor));
        let x_display_holders = match self.env.x_sockets() {
            Some(sockets) if sockets.is_empty() => Some(Vec::new()),
            Some(sockets) if found(&sockets) => Some(list(held.x)),
            _ => None,
        };
        let other = |set: BTreeSet<u32>| listening.as_ref().map(|_| list(set));
        let other_compositor_holders = other(held.other_compositor);
        let other_x_holders = other(held.other_x);

        let uid = self.uid;
        let on_demand_compositors = compositor_holders
            .iter()
            .chain(other_compositor_holders.iter())
            .flatten()
            .filter(|pid| {
                members
                    .get(pid)
                    .is_some_and(|m| monitor_core::session::keeps_xwayland_on_demand(m, uid))
            })
            .copied()
            .collect();

        SessionFacts {
            uid,
            monitor_ancestors: self.ancestors(),
            bus_holders,
            compositor_holders,
            x_display_holders,
            other_compositor_holders,
            other_x_holders,
            on_demand_compositors,
            fds_read,
            members,
            sessions: request.sessions.clone(),
        }
    }
}

/// The cgroup path of a process (`0::` line), `None` when unreadable.
pub fn cgroup_of(proc_root: &Path, pid: u32) -> Option<String> {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join("cgroup"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_bus_is_found_from_its_address_and_the_user_bus() {
        let mut env = SessionEnv {
            runtime_dir: Some("/run/user/1000".into()),
            bus_address: Some("unix:path=/run/user/1000/bus".into()),
            ..SessionEnv::default()
        };
        assert_eq!(env.bus_sockets(), vec!["/run/user/1000/bus".to_string()]);
        env.bus_address = Some("unix:abstract=/tmp/dbus-x,guid=1;tcp:host=a".into());
        assert_eq!(
            env.bus_sockets(),
            vec!["@/tmp/dbus-x".to_string(), "/run/user/1000/bus".to_string()]
        );
        env.bus_address = None;
        assert_eq!(env.bus_sockets(), vec!["/run/user/1000/bus".to_string()]);
        env.runtime_dir = None;
        assert!(env.bus_sockets().is_empty());
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
    fn only_listening_sockets_are_read() {
        // From this desktop's /proc/net/unix on 2026-11-22: a listening
        // socket (flags 00010000, state 01) and a connected one.
        let text = "Num       RefCount Protocol Flags    Type St Inode Path\n\
            0000000000000000: 00000002 00000000 00010000 0001 01   927 /run/user/1000/bus\n\
            0000000000000000: 00000003 00000000 00000000 0001 03 20805 /run/user/1000/bus\n\
            0000000000000000: 00000002 00000000 00010000 0001 01 31750 /run/user/1000/wayland-0\n\
            0000000000000000: 00000002 00000000 00010000 0001 01 40000\n";
        assert_eq!(
            listening_sockets(text),
            vec![
                Listening {
                    inode: 927,
                    path: "/run/user/1000/bus".into()
                },
                Listening {
                    inode: 31750,
                    path: "/run/user/1000/wayland-0".into()
                },
            ]
        );
    }
}
