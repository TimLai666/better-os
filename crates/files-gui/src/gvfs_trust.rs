//! Which programs on the session bus are GVfs (ticket 117).
//!
//! The session bus lets any program of this person's claim any name it
//! likes, `org.gtk.vfs.Daemon` included (before GVfs is started, or after
//! killing it), and call any object another connection exports. So
//! Better Files decides who GVfs is by the processes behind the
//! connections, not by bus names:
//!
//! - **The daemon** is the owner of `org.gtk.vfs.Daemon` whose program
//!   (`/proc/<pid>/exe`) is the `gvfsd` that GVfs's own D-Bus service file
//!   names (on this machine `/usr/libexec/gvfsd`, read from
//!   `/usr/share/dbus-1/services/org.gtk.vfs.Daemon.service`), and, when
//!   the session runs under `systemd --user`, which runs in the unit that
//!   file names (`gvfs-daemon.service`). Mounts go to that owner's unique
//!   name, never to the well-known one, so a name taken over after the
//!   check does not receive them.
//! - **A backend** may ask a mount's questions when it is a direct child of
//!   that daemon and its program is the backend GVfs's mount file
//!   (`/usr/share/gvfs/mounts/*.mount`) names for the mount's type: `sftp`
//!   is `gvfsd-sftp`, `smb-share` is `gvfsd-smb`, `smb-server`
//!   `gvfsd-smb-browse`, and so on. The verified daemon itself may also
//!   ask (it is trusted with the whole mount already). Anything else,
//!   `ssh`'s `ProxyCommand` or askpass children and other backends among
//!   them, is refused.
//!
//! What this does **not** guarantee: a program running as this person can
//! still start the real `gvfsd` itself under its own control (with
//! `LD_PRELOAD`, or by tracing it) and pass every check, and it can move its
//! own processes between the cgroups of its `systemd --user` (that subtree
//! is delegated to the person), so the cgroup check only keeps out a
//! careless impostor. The checks stop a different program from posing as
//! GVfs by name; they are not a boundary against code already running as
//! the same person.
//!
//! Processes are pinned with a pidfd: the bus's own (`ProcessFD` in
//! `GetConnectionCredentials`, from `SO_PEERPIDFD`) when it gives one,
//! otherwise one opened on the bus's `ProcessID` and confirmed by asking the
//! bus for that connection's PID again. `/proc` is read by number, then the
//! pidfd is checked to still be running, so a PID reused in between is
//! noticed rather than trusted. This machine's session bus (dbus-daemon
//! 1.14.10) gives no `ProcessFD`, so the PID path is the one used here;
//! buses that implement `ProcessFD` get the stronger one without a change.

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

/// Where an installed GVfs's files are looked for: the system's data
/// folders only, never the person's own (`~/.local/share`), which any
/// program of theirs can write.
const SYSTEM_DATA_DIRS: &[&str] = &["/usr/share", "/usr/local/share"];

/// What GVfs is on this computer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trust {
    /// The `gvfsd` program, resolved.
    pub daemon: PathBuf,
    /// The `systemd --user` unit `gvfsd` must run in, when there is one.
    pub unit: Option<String>,
    /// Each mount type's backend programs, resolved.
    pub backends: HashMap<String, Vec<PathBuf>>,
}

impl Trust {
    /// The GVfs installed on this computer, or why there is none to trust.
    pub fn system() -> Result<Self, String> {
        let mut trust = SYSTEM_DATA_DIRS
            .iter()
            .find_map(|data| Self::installed_in(Path::new(data)))
            .ok_or_else(|| {
                "GVfs's service file (org.gtk.vfs.Daemon.service) is not installed".to_string()
            })?;
        // The unit is only there when the session's programs run under
        // `systemd --user`; elsewhere the bus starts `gvfsd` itself.
        if !own_cgroup().is_some_and(|path| path.contains("/user@")) {
            trust.unit = None;
        }
        Ok(trust)
    }

    /// GVfs as installed under the data folder `data` (`/usr/share`): its
    /// D-Bus service file and its mount files.
    pub fn installed_in(data: &Path) -> Option<Self> {
        let services = std::fs::read_dir(data.join("dbus-1/services")).ok()?;
        let (exec, unit) = services.flatten().find_map(|entry| {
            let text = std::fs::read_to_string(entry.path()).ok()?;
            let fields = ini_fields(&text);
            (fields.get("Name").map(String::as_str) == Some(crate::gvfs::DAEMON)).then(|| {
                (
                    fields.get("Exec").cloned().unwrap_or_default(),
                    fields.get("SystemdService").cloned(),
                )
            })
        })?;
        let program = exec.split_whitespace().next()?;
        let daemon = std::fs::canonicalize(program).ok()?;
        let mut backends: HashMap<String, Vec<PathBuf>> = HashMap::new();
        if let Ok(mounts) = std::fs::read_dir(data.join("gvfs/mounts")) {
            for entry in mounts.flatten() {
                let Ok(text) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                let fields = ini_fields(&text);
                let (Some(types), Some(exec)) = (fields.get("Type"), fields.get("Exec")) else {
                    continue;
                };
                let Some(program) = exec
                    .split_whitespace()
                    .next()
                    .and_then(|program| std::fs::canonicalize(program).ok())
                else {
                    continue;
                };
                for kind in types.split(';').filter(|kind| !kind.is_empty()) {
                    let known = backends.entry(kind.to_string()).or_default();
                    if !known.contains(&program) {
                        known.push(program.clone());
                    }
                }
            }
        }
        Some(Self {
            daemon,
            unit,
            backends,
        })
    }

    /// Whether `daemon`, the owner of `org.gtk.vfs.Daemon`, is the
    /// installed `gvfsd`; the reason when it is not.
    pub fn verify_daemon(&self, daemon: &Peer) -> Result<(), String> {
        if daemon.uid != Some(own_uid()) {
            return Err(format!(
                "org.gtk.vfs.Daemon is owned by user {:?}, not this one",
                daemon.uid
            ));
        }
        let program = program_of(daemon.pid);
        if !program
            .as_deref()
            .is_some_and(|program| same_program(program, &self.daemon))
        {
            return Err(format!(
                "org.gtk.vfs.Daemon is owned by {}, not by {}",
                program.map_or_else(|| "an unknown program".into(), |p| p.display().to_string()),
                self.daemon.display()
            ));
        }
        if let Some(unit) = &self.unit
            && !in_unit(daemon.pid, unit)
        {
            return Err(format!(
                "{} owns org.gtk.vfs.Daemon outside {unit}",
                self.daemon.display()
            ));
        }
        if !daemon.alive() {
            return Err("the owner of org.gtk.vfs.Daemon has ended".into());
        }
        Ok(())
    }

    /// Whether `caller` may ask the questions of a mount of `kind` (the
    /// spec's `type`) going through the verified `daemon`.
    pub fn is_backend(&self, caller: &Peer, daemon: &Peer, kind: &str) -> bool {
        if caller.uid != Some(own_uid()) {
            return false;
        }
        if caller.pid == daemon.pid {
            // Two pidfds on one running PID are one process.
            return caller.alive() && daemon.alive();
        }
        let backend = program_of(caller.pid).is_some_and(|program| {
            self.backends
                .get(kind)
                .is_some_and(|known| known.iter().any(|exe| same_program(&program, exe)))
        });
        // Read while both are pinned and checked running after: the parent
        // and the program are this caller's and this daemon's.
        backend && parent_of(caller.pid) == Some(daemon.pid) && caller.alive() && daemon.alive()
    }
}

/// `Key=Value` lines (the first of each key), as D-Bus service files and
/// GVfs's mount files are written.
fn ini_fields(text: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            fields
                .entry(key.trim().to_string())
                .or_insert_with(|| value.trim().to_string());
        }
    }
    fields
}

/// A process on the bus, pinned by a pidfd.
#[derive(Debug)]
pub struct Peer {
    pub pid: u32,
    pub uid: Option<u32>,
    /// Whether the bus gave the pidfd (`ProcessFD`), rather than it being
    /// opened on the bus's `ProcessID`.
    pub from_bus: bool,
    pidfd: OwnedFd,
}

impl Peer {
    /// The process behind the connection `name`.
    pub async fn of(
        bus: &zbus::fdo::DBusProxy<'_>,
        name: &zbus::names::UniqueName<'_>,
    ) -> Option<Self> {
        let ask = || async {
            bus.get_connection_credentials(zbus::names::BusName::Unique(name.to_owned()))
                .await
                .ok()
        };
        let credentials = ask().await?;
        let uid = credentials.unix_user_id();
        if let Some(fd) = credentials.process_fd() {
            let pidfd = fd.as_fd().try_clone_to_owned().ok()?;
            let pid = pid_of_pidfd(&pidfd)?;
            return Some(Self {
                pid,
                uid,
                from_bus: true,
                pidfd,
            });
        }
        let pid = credentials.process_id()?;
        let pidfd = pidfd_open(pid)?;
        // Pinned now; the connection still being that PID's means the
        // pinned process is the one the bus saw (unless that process handed
        // its bus socket to another, which is its own doing).
        let again = ask().await?.process_id()?;
        let peer = Self {
            pid,
            uid,
            from_bus: false,
            pidfd,
        };
        (again == pid && peer.alive()).then_some(peer)
    }

    /// A process of this program's own, for tests.
    #[cfg(test)]
    pub fn pinned(pid: u32) -> Option<Self> {
        Some(Self {
            pid,
            uid: Some(own_uid()),
            from_bus: false,
            pidfd: pidfd_open(pid)?,
        })
    }

    /// Whether the pinned process is still running (a pidfd turns readable
    /// when it ends).
    pub fn alive(&self) -> bool {
        let mut poll = libc::pollfd {
            fd: self.pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid `pollfd` for the duration of the call; zero
        // timeout.
        let ready = unsafe { libc::poll(&mut poll, 1, 0) };
        ready == 0
    }
}

fn pidfd_open(pid: u32) -> Option<OwnedFd> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: `pidfd_open(pid, 0)` takes two integers and returns a new file
    // descriptor this function then owns, or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    let fd = i32::try_from(fd).ok().filter(|fd| *fd >= 0)?;
    // SAFETY: `fd` was just returned by the kernel and nothing else owns it.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// The PID a pidfd names in this namespace (`Pid:` in its fdinfo; `-1`
/// once the process has ended).
pub fn pid_of_pidfd(pidfd: &OwnedFd) -> Option<u32> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd())).ok()?;
    info.lines()
        .find_map(|line| line.strip_prefix("Pid:"))?
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|pid| *pid > 0)
        .and_then(|pid| u32::try_from(pid).ok())
}

fn own_uid() -> u32 {
    // SAFETY: `getuid` takes nothing and cannot fail.
    unsafe { libc::getuid() }
}

fn own_cgroup() -> Option<String> {
    cgroup_of("self")
}

/// The cgroup v2 path of process `pid` (`"self"` for this one).
fn cgroup_of(pid: &str) -> Option<String> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(str::to_string)
}

/// Whether process `pid` runs in the systemd unit `unit`.
fn in_unit(pid: u32, unit: &str) -> bool {
    cgroup_of(&pid.to_string())
        .is_some_and(|path| path.rsplit('/').next() == Some(unit) && path.contains("/user@"))
}

/// The program process `pid` runs.
pub fn program_of(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// Whether the program `/proc` names is `expected`: also when it was
/// replaced on disk since it started (a GVfs update), which `/proc` marks
/// " (deleted)"; only whoever can write the system's folders can do that.
fn same_program(program: &Path, expected: &Path) -> bool {
    program == expected
        || program
            .as_os_str()
            .to_str()
            .is_some_and(|program| program.strip_suffix(" (deleted)") == expected.to_str())
}

/// The parent of process `pid`, from `/proc/<pid>/stat`.
pub fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (comm) state ppid …`; `comm` may hold spaces and parentheses.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gvfs_is_read_from_its_service_and_mount_files() {
        let data = tempfile::tempdir().unwrap();
        let libexec = data.path().join("libexec");
        std::fs::create_dir_all(&libexec).unwrap();
        for program in ["gvfsd", "gvfsd-sftp", "gvfsd-smb", "gvfsd-smb-browse"] {
            std::fs::write(libexec.join(program), "").unwrap();
        }
        let services = data.path().join("dbus-1/services");
        let mounts = data.path().join("gvfs/mounts");
        std::fs::create_dir_all(&services).unwrap();
        std::fs::create_dir_all(&mounts).unwrap();
        std::fs::write(
            services.join("org.gtk.vfs.Daemon.service"),
            format!(
                "[D-BUS Service]\nName=org.gtk.vfs.Daemon\nExec={}\nSystemdService=gvfs-daemon.service\n",
                libexec.join("gvfsd").display()
            ),
        )
        .unwrap();
        std::fs::write(
            services.join("other.service"),
            "Name=org.example\nExec=/x\n",
        )
        .unwrap();
        for (file, types, program) in [
            ("sftp.mount", "sftp", "gvfsd-sftp"),
            ("smb.mount", "smb-share", "gvfsd-smb"),
            (
                "smb-browse.mount",
                "smb-network;smb-server",
                "gvfsd-smb-browse",
            ),
        ] {
            std::fs::write(
                mounts.join(file),
                format!(
                    "[Mount]\nType={types}\nExec={}\nAutoMount=false\n",
                    libexec.join(program).display()
                ),
            )
            .unwrap();
        }
        let trust = Trust::installed_in(data.path()).expect("GVfs is there");
        let libexec = std::fs::canonicalize(libexec).unwrap();
        assert_eq!(trust.daemon, libexec.join("gvfsd"));
        assert_eq!(trust.unit.as_deref(), Some("gvfs-daemon.service"));
        assert_eq!(trust.backends["sftp"], [libexec.join("gvfsd-sftp")]);
        assert_eq!(trust.backends["smb-share"], [libexec.join("gvfsd-smb")]);
        assert_eq!(
            trust.backends["smb-server"],
            [libexec.join("gvfsd-smb-browse")]
        );
        assert_eq!(
            trust.backends["smb-network"],
            [libexec.join("gvfsd-smb-browse")]
        );
        assert!(Trust::installed_in(&data.path().join("nothing")).is_none());
    }

    #[test]
    fn the_installed_gvfs_is_found_when_there_is_one() {
        // CI installs `gvfs-backends` (ticket 177).
        let trust = test_prerequisites::require!("GVfs (gvfs-backends)", Trust::system());
        assert!(trust.daemon.is_absolute());
        assert!(trust.backends.contains_key("sftp"), "{trust:?}");
    }

    #[test]
    fn a_pidfd_names_its_process_until_it_ends() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("sleep");
        let peer = Peer::pinned(child.id()).expect("pidfd_open");
        assert_eq!(pid_of_pidfd(&peer.pidfd), Some(child.id()));
        assert!(peer.alive());
        assert_eq!(parent_of(child.id()), Some(std::process::id()));
        let _ = child.kill();
        let _ = child.wait();
        assert!(!peer.alive(), "an ended process is not alive");
        assert_eq!(pid_of_pidfd(&peer.pidfd), None);
    }

    #[test]
    fn a_program_replaced_on_disk_is_still_itself() {
        let expected = Path::new("/usr/libexec/gvfsd");
        assert!(same_program(Path::new("/usr/libexec/gvfsd"), expected));
        assert!(same_program(
            Path::new("/usr/libexec/gvfsd (deleted)"),
            expected
        ));
        assert!(!same_program(Path::new("/tmp/gvfsd"), expected));
        assert!(!same_program(
            Path::new("/usr/libexec/gvfsd-sftp"),
            expected
        ));
    }
}
