//! What the supervisor reads from the machine, each from a path a test can
//! point at a fixture. Everything here is someone else's output and is
//! read as untrusted: bounded sizes, regular files only where a file is
//! expected, and only the fields the rules use.

use std::io::Read as _;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use crate::rules::{Card, Drm, Greetd, HeartbeatFile, LogindSession, Process, Word};

/// Where each thing is on a running system.
pub mod paths {
    pub const DRM: &str = "/sys/class/drm";
    pub const SESSIONS: &str = "/run/systemd/sessions";
    /// The greeter's unit has no `Slice=`, so systemd puts it here.
    pub const CGROUPS: &str = "/sys/fs/cgroup";
    pub const GREETER_CGROUP: &str = "/sys/fs/cgroup/system.slice/better-os-greeter.service";
    pub const PROC: &str = "/proc";
    pub const HEARTBEAT: &str = "/run/better-os-greeter/alive";
    pub const PASSWD: &str = "/etc/passwd";
    pub const STATE: &str = "/run/better-os/login-path.json";
    pub const ISSUE: &str = "/run/better-os/login-fallback.issue";
    pub const FALLBACK_ACTIVE: &str = "/run/better-os/login-fallback.active";
    pub const HELD: &str = "/run/better-os/login-held";
    pub const GREETER_MODE: &str = "/run/better-os/greeter-mode";
}

/// The greeter's account.
pub const GREETER_ACCOUNT: &str = "_better-greeter";

/// The DRM drivers that are the firmware's framebuffer, not a GPU's own
/// driver (spec section 5.10, stage 1).
pub const FIRMWARE_DRIVERS: [&str; 8] = [
    "simple-framebuffer",
    "simpledrm",
    "efi-framebuffer",
    "efidrm",
    "vesa-framebuffer",
    "vesadrm",
    "of-display",
    "ofdrm",
];

/// Reads at most `limit` bytes of a regular file, opened without following
/// a link and without blocking on a FIFO. `None` for anything else.
pub fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

/// sysfs files report a size of 4096 whatever they hold, and are read the
/// same way: bounded.
fn sysfs_text(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(4096)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn is_card(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `/sys/class/drm`: each card, whether its driver is the firmware's, and
/// whether a connector says a screen is there. `None` when the directory
/// cannot be read (no DRM at all).
pub fn drm(dir: &Path) -> Option<Drm> {
    let mut entries: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    entries.sort();
    entries.truncate(256);
    let cards = entries
        .iter()
        .filter(|name| is_card(name))
        .map(|name| {
            let driver = std::fs::read_link(dir.join(name).join("device/driver"))
                .ok()
                .and_then(|target| {
                    target
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(str::to_string)
                });
            let firmware = driver
                .as_deref()
                .is_none_or(|driver| FIRMWARE_DRIVERS.contains(&driver));
            let prefix = format!("{name}-");
            let screen = entries.iter().any(|connector| {
                connector
                    .strip_prefix(&prefix)
                    .is_some_and(|kind| !kind.starts_with("Writeback"))
                    && sysfs_text(&dir.join(connector).join("status"))
                        .is_some_and(|status| status == "connected" || status == "unknown")
            });
            Card {
                name: name.clone(),
                firmware,
                screen,
            }
        })
        .collect();
    Some(Drm { cards, entries })
}

/// logind's session files (`/run/systemd/sessions/<id>`): its own record
/// of each session, read without asking it. Sessions that are closing are
/// kept and marked (their scope may still hold a greeter's processes); the
/// rules never count one as a person's session. logind makes the directory with its first session, so none
/// is no session; `None` when it is there and cannot be read.
pub fn sessions(dir: &Path) -> Option<Vec<LogindSession>> {
    let mut sessions = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(sessions),
        Err(_) => return None,
    };
    for entry in entries.flatten().take(1024) {
        let Ok(id) = entry.file_name().into_string() else {
            continue;
        };
        // `<id>.ref` and the like are FIFOs and references, not records.
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        let Some(bytes) = read_bounded(&entry.path(), 64 * 1024) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let field = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or("")
                .to_string()
        };
        let Ok(uid) = field("UID").parse::<u32>() else {
            continue;
        };
        sessions.push(LogindSession {
            id,
            uid,
            class: field("CLASS"),
            service: field("SERVICE"),
            seat: field("SEAT"),
            kind: field("TYPE"),
            scope: field("SCOPE"),
            closing: field("STATE") == "closing",
        });
    }
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    Some(sessions)
}

fn pids(text: &str) -> impl Iterator<Item = u32> + '_ {
    text.split_whitespace().filter_map(|p| p.parse().ok())
}

/// A process's real uid, from `/proc/<pid>/status`.
pub fn uid_of_process(proc: &Path, pid: u32) -> Option<u32> {
    uid_of(proc, pid)
}

fn uid_of(proc: &Path, pid: u32) -> Option<u32> {
    let bytes = read_bounded(&proc.join(pid.to_string()).join("status"), 64 * 1024)?;
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
}

/// A process's children, from every thread's `children` file.
fn children_of(proc: &Path, pid: u32) -> Vec<u32> {
    let Ok(tasks) = std::fs::read_dir(proc.join(pid.to_string()).join("task")) else {
        return Vec::new();
    };
    let mut children = Vec::new();
    for task in tasks.flatten().take(1024) {
        if let Some(text) = sysfs_text(&task.path().join("children")) {
            children.extend(pids(&text));
        }
    }
    children
}

/// greetd's tree: the processes of the unit's cgroup and their
/// descendants, each with its uid. A unit with no cgroup has no process
/// (it is stopped); a cgroup that is there and cannot be read is unknown.
pub fn greetd(cgroup: &Path, proc: &Path) -> Option<Greetd> {
    let text = match std::fs::read_to_string(cgroup.join("cgroup.procs")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some(Greetd::default());
        }
        Err(_) => return None,
    };
    let mut processes: Vec<Process> = Vec::new();
    let mut queue: Vec<(u32, bool)> = pids(&text).map(|pid| (pid, true)).collect();
    while let Some((pid, in_cgroup)) = queue.pop() {
        if processes.len() >= 4096 || processes.iter().any(|p| p.pid == pid) {
            continue;
        }
        // A process that left between the listing and the read is gone.
        let Some(uid) = uid_of(proc, pid) else {
            continue;
        };
        processes.push(Process {
            pid,
            uid,
            in_cgroup,
        });
        queue.extend(
            children_of(proc, pid)
                .into_iter()
                .map(|child| (child, false)),
        );
    }
    processes.sort_by_key(|p| p.pid);
    Some(Greetd { processes })
}

/// The cgroup a process is in, from `/proc/<pid>/cgroup` (the unified
/// hierarchy's `0::` line).
pub fn cgroup_of(proc: &Path, pid: u32) -> Option<String> {
    let bytes = read_bounded(&proc.join(pid.to_string()).join("cgroup"), 64 * 1024)?;
    String::from_utf8_lossy(&bytes)
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(str::to_string)
}

/// The processes of a session scope's cgroup under `cgroup_root`, each with
/// its uid. Only a path of a session scope's shape is read; a scope that is
/// gone has no process; one that is there and cannot be read is unknown.
pub fn scope_processes(cgroup_root: &Path, path: &str, proc: &Path) -> Option<Vec<Process>> {
    if !crate::rules::is_session_scope(path) {
        return None;
    }
    let file = cgroup_root
        .join(path.trim_start_matches('/'))
        .join("cgroup.procs");
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return None,
    };
    let mut processes = Vec::new();
    for pid in pids(&text).take(4096) {
        // A process that left between the listing and the read is gone.
        if let Some(uid) = uid_of(proc, pid) {
            processes.push(Process {
                pid,
                uid,
                in_cgroup: false,
            });
        }
    }
    Some(processes)
}

/// Every process with a `/dev/dri/card*` open, with its uid, `plymouthd`
/// left out (its own path quits or kills it). Only the links' targets are
/// compared; `/proc/<pid>/comm` and `/proc/<pid>/status` are read for the
/// name and the uid.
pub fn drm_holders(proc: &Path) -> Option<Vec<Process>> {
    let mut holders = Vec::new();
    for entry in std::fs::read_dir(proc).ok()?.flatten().take(65_536) {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let comm = read_bounded(&entry.path().join("comm"), 64)
            .map(|c| String::from_utf8_lossy(&c).trim().to_string());
        if comm.as_deref() == Some("plymouthd") {
            continue;
        }
        let fds = match std::fs::read_dir(entry.path().join("fd")) {
            Ok(fds) => fds,
            // Not knowing what a process holds is not knowing the card is
            // free.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                holders.push(Process {
                    pid,
                    uid: uid_of(proc, pid).unwrap_or(0),
                    in_cgroup: false,
                });
                continue;
            }
            // Gone, or a kernel thread.
            Err(_) => continue,
        };
        let holds = fds.flatten().take(65_536).any(|fd| {
            std::fs::read_link(fd.path()).is_ok_and(|target| {
                target
                    .to_str()
                    .and_then(|t| t.strip_prefix("/dev/dri/card"))
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
        });
        if holds && let Some(uid) = uid_of(proc, pid) {
            holders.push(Process {
                pid,
                uid,
                in_cgroup: false,
            });
        }
    }
    holders.sort_by_key(|p| p.pid);
    Some(holders)
}

/// The greeter's heartbeat: a regular file of at most 64 bytes, its
/// identity changing with every rewrite (a rename gives a new inode; a
/// write in place a new modification time).
pub fn heartbeat(path: &Path) -> Option<HeartbeatFile> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(65).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 64 {
        return None;
    }
    let word = match String::from_utf8_lossy(&bytes).trim() {
        "starting" => Word::Starting,
        "ready" => Word::Ready,
        _ => Word::Other,
    };
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        meta.dev(),
        meta.ino(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.size(),
        &bytes,
    )
        .hash(&mut hasher);
    Some(HeartbeatFile {
        identity: hasher.finish(),
        word,
    })
}

/// Whether a unit's cgroup holds nothing but a root `agetty`: nobody has
/// begun to sign in on that text login (`login` and the person's shell
/// would be there). `None` when it cannot be read; an empty cgroup is not
/// "alone" either.
pub fn only_a_getty(cgroup: &Path, proc: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(cgroup.join("cgroup.procs")).ok()?;
    let mut any = false;
    for pid in pids(&text).take(4096) {
        let comm = read_bounded(&proc.join(pid.to_string()).join("comm"), 64)
            .map(|c| String::from_utf8_lossy(&c).trim().to_string());
        let Some(uid) = uid_of(proc, pid) else {
            // Gone between the listing and the read.
            continue;
        };
        if uid != 0 || comm.as_deref() != Some("agetty") {
            return Some(false);
        }
        any = true;
    }
    Some(any)
}

/// `_better-greeter`'s uid in `/etc/passwd`, read directly: a name service
/// lookup could wait on a network directory.
pub fn greeter_uid(passwd: &Path) -> Option<u32> {
    let bytes = read_bounded(passwd, 16 * 1024 * 1024)?;
    String::from_utf8_lossy(&bytes).lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != GREETER_ACCOUNT {
            return None;
        }
        fields.nth(1)?.parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_real_card_with_a_connected_screen_is_told_from_the_firmware_framebuffer() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // card0: simpledrm, with a connector that says connected.
        std::fs::create_dir_all(root.join("card0/device")).unwrap();
        symlink(
            "../../../bus/platform/drivers/simple-framebuffer",
            root.join("card0/device/driver"),
        )
        .unwrap();
        write(&root.join("card0-Unknown-1/status"), "connected\n");
        // card1: amdgpu, its eDP connected and a writeback that says unknown.
        std::fs::create_dir_all(root.join("card1/device")).unwrap();
        symlink(
            "../../../../bus/pci/drivers/amdgpu",
            root.join("card1/device/driver"),
        )
        .unwrap();
        write(&root.join("card1-eDP-1/status"), "connected\n");
        write(&root.join("card1-Writeback-1/status"), "unknown\n");
        write(&root.join("version"), "drm 1.1.0 20060810\n");
        std::fs::create_dir_all(root.join("renderD128")).unwrap();
        let drm = drm(root).unwrap();
        assert_eq!(
            drm.cards,
            [
                Card {
                    name: "card0".into(),
                    firmware: true,
                    screen: true
                },
                Card {
                    name: "card1".into(),
                    firmware: false,
                    screen: true
                },
            ]
        );
        assert!(drm.entries.contains(&"renderD128".to_string()));
    }

    #[test]
    fn a_writeback_connector_is_never_a_screen_and_unknown_counts_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("card1/device")).unwrap();
        symlink("x/drivers/i915", root.join("card1/device/driver")).unwrap();
        write(&root.join("card1-DP-1/status"), "disconnected\n");
        write(&root.join("card1-Writeback-1/status"), "unknown\n");
        assert!(!drm(root).unwrap().cards[0].screen);
        write(&root.join("card1-HDMI-A-1/status"), "unknown\n");
        assert!(drm(root).unwrap().cards[0].screen);
        // A driver that cannot be read is not taken for a real one.
        std::fs::create_dir_all(root.join("card2")).unwrap();
        assert!(drm(root).unwrap().cards[1].firmware);
        assert!(drm(&root.join("absent")).is_none());
    }

    #[test]
    fn logind_session_files_give_class_service_seat_and_uid_and_mark_closing() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("2"),
            "UID=1000\nUSER=tim\nACTIVE=1\nSTATE=active\nTYPE=wayland\nCLASS=user\nSEAT=seat0\nSERVICE=greetd\n",
        );
        write(
            &dir.path().join("c1"),
            "UID=118\nSTATE=online\nTYPE=wayland\nCLASS=greeter\nSEAT=seat0\nSERVICE=greetd-greeter\nSCOPE=session-c1.scope\n",
        );
        write(
            &dir.path().join("5"),
            "UID=1001\nSTATE=closing\nCLASS=user\nSEAT=seat0\nSERVICE=greetd\n",
        );
        write(&dir.path().join("2.ref"), "");
        write(&dir.path().join("7"), "CLASS=user\n");
        let sessions = sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 3, "{sessions:?}");
        assert!(sessions[0].is_greetd_person());
        assert_eq!(sessions[0].uid, 1000);
        // Closing: kept for its scope, never a person's session.
        assert!(sessions[1].closing);
        assert!(!sessions[1].is_person());
        assert!(sessions[2].is_greeter());
        assert_eq!(sessions[2].uid, 118);
        assert_eq!(sessions[2].scope, "session-c1.scope");
        assert_eq!(
            super::sessions(&dir.path().join("absent")),
            Some(Vec::new())
        );
    }

    #[test]
    fn greetds_tree_follows_children_out_of_the_cgroup() {
        let dir = tempfile::tempdir().unwrap();
        let cgroup = dir.path().join("cgroup");
        let proc = dir.path().join("proc");
        write(&cgroup.join("cgroup.procs"), "100\n");
        let process = |pid: u32, uid: u32, children: &str| {
            write(
                &proc.join(format!("{pid}/status")),
                &format!("Name:\tx\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            );
            write(&proc.join(format!("{pid}/task/{pid}/children")), children);
        };
        // greetd (root) -> worker (root, moved to the session's scope) ->
        // the session (1000).
        process(100, 0, "101 ");
        process(101, 0, "102 ");
        process(102, 1000, "");
        let tree = greetd(&cgroup, &proc).unwrap();
        assert_eq!(
            tree.processes,
            [
                Process {
                    pid: 100,
                    uid: 0,
                    in_cgroup: true
                },
                Process {
                    pid: 101,
                    uid: 0,
                    in_cgroup: false
                },
                Process {
                    pid: 102,
                    uid: 1000,
                    in_cgroup: false
                },
            ]
        );
        assert!(!tree.unit_empty());
        // No cgroup at all: the unit has no process.
        assert_eq!(
            greetd(&dir.path().join("none"), &proc),
            Some(Greetd::default())
        );
    }

    #[test]
    fn a_session_scopes_processes_are_read_only_for_a_scope_shaped_path() {
        let dir = tempfile::tempdir().unwrap();
        let cgroups = dir.path().join("cgroup");
        let proc = dir.path().join("proc");
        let scope = "/user.slice/user-118.slice/session-c1.scope";
        write(
            &cgroups.join(&scope[1..]).join("cgroup.procs"),
            "200\n201\n",
        );
        write(&proc.join("200/status"), "Uid:\t118\t118\t118\t118\n");
        write(&proc.join("201/status"), "Uid:\t0\t0\t0\t0\n");
        write(&proc.join("200/cgroup"), &format!("0::{scope}\n"));
        let found = scope_processes(&cgroups, scope, &proc).unwrap();
        assert_eq!(
            found.iter().map(|p| (p.pid, p.uid)).collect::<Vec<_>>(),
            [(200, 118), (201, 0)]
        );
        assert_eq!(cgroup_of(&proc, 200).as_deref(), Some(scope));
        // Gone: no process. Another shape: never read.
        assert_eq!(
            scope_processes(
                &cgroups,
                "/user.slice/user-118.slice/session-c2.scope",
                &proc
            ),
            Some(Vec::new())
        );
        write(&cgroups.join("system.slice/x.service/cgroup.procs"), "1\n");
        assert_eq!(
            scope_processes(&cgroups, "/system.slice/x.service", &proc),
            None
        );
        assert_eq!(
            scope_processes(
                &cgroups,
                "/user.slice/user-118.slice/session-c1.scope/../../..",
                &proc
            ),
            None
        );
    }

    #[test]
    fn a_card_holder_is_found_by_its_open_descriptor_and_plymouthd_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let proc = dir.path();
        for (pid, name, target) in [
            (10, "cosmic-comp", "/dev/dri/card1"),
            (11, "plymouthd", "/dev/dri/card0"),
            (12, "app", "/dev/dri/renderD128"),
            (13, "bash", "/dev/pts/0"),
        ] {
            std::fs::create_dir_all(proc.join(format!("{pid}/fd"))).unwrap();
            std::fs::write(proc.join(format!("{pid}/comm")), format!("{name}\n")).unwrap();
            symlink(target, proc.join(format!("{pid}/fd/3"))).unwrap();
        }
        std::fs::write(proc.join("10/status"), "Uid:\t118\t118\t118\t118\n").unwrap();
        assert_eq!(
            drm_holders(proc),
            Some(vec![Process {
                pid: 10,
                uid: 118,
                in_cgroup: false
            }])
        );
    }

    #[test]
    fn a_text_login_is_alone_only_while_its_cgroup_holds_a_root_agetty_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let cgroup = dir.path().join("cgroup");
        let proc = dir.path().join("proc");
        let process = |pid: u32, uid: u32, comm: &str| {
            write(
                &proc.join(format!("{pid}/status")),
                &format!("Uid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            );
            write(&proc.join(format!("{pid}/comm")), &format!("{comm}\n"));
        };
        process(10, 0, "agetty");
        process(11, 0, "login");
        process(12, 1000, "bash");
        write(&cgroup.join("cgroup.procs"), "10\n");
        assert_eq!(only_a_getty(&cgroup, &proc), Some(true));
        write(&cgroup.join("cgroup.procs"), "11\n");
        assert_eq!(
            only_a_getty(&cgroup, &proc),
            Some(false),
            "a sign-in has begun"
        );
        write(&cgroup.join("cgroup.procs"), "10\n12\n");
        assert_eq!(
            only_a_getty(&cgroup, &proc),
            Some(false),
            "a person's shell"
        );
        write(&cgroup.join("cgroup.procs"), "");
        assert_eq!(only_a_getty(&cgroup, &proc), Some(false));
        assert_eq!(only_a_getty(&dir.path().join("none"), &proc), None);
    }

    #[test]
    fn the_heartbeat_changes_identity_with_a_rewrite_and_refuses_what_is_not_a_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alive");
        assert!(heartbeat(&path).is_none());
        std::fs::write(&path, "starting\n").unwrap();
        let first = heartbeat(&path).unwrap();
        assert_eq!(first.word, Word::Starting);
        let new = dir.path().join(".alive.new");
        std::fs::write(&new, "ready\n").unwrap();
        std::fs::rename(&new, &path).unwrap();
        let second = heartbeat(&path).unwrap();
        assert_eq!(second.word, Word::Ready);
        assert_ne!(first.identity, second.identity);
        assert_eq!(heartbeat(&path).unwrap().identity, second.identity);
        std::fs::write(&path, "x".repeat(65)).unwrap();
        assert!(heartbeat(&path).is_none());
        std::fs::remove_file(&path).unwrap();
        symlink(dir.path().join("elsewhere"), &path).unwrap();
        std::fs::write(dir.path().join("elsewhere"), "ready\n").unwrap();
        assert!(heartbeat(&path).is_none(), "a link is not followed");
    }

    #[test]
    fn the_greeters_uid_is_read_from_passwd_and_a_missing_account_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let passwd = dir.path().join("passwd");
        std::fs::write(
            &passwd,
            "root:x:0:0:root:/root:/bin/bash\n_better-greeter:x:118:124::/var/lib/better-os-greeter:/usr/sbin/nologin\n",
        )
        .unwrap();
        assert_eq!(greeter_uid(&passwd), Some(118));
        std::fs::write(&passwd, "root:x:0:0:root:/root:/bin/bash\n").unwrap();
        assert_eq!(greeter_uid(&passwd), None);
    }
}
