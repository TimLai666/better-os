//! The supervisor's process edges on a running system. Thin on purpose:
//! every decision is in `rules`; this only reads, runs fixed commands with
//! time limits, and writes its own files.

use std::os::fd::AsRawFd as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use manager_ipc::boot_watch::{
    Event, LOGIN_SUPERVISOR_UNIT, RECORD_VERSION, Records, prune, write_event,
};

use crate::read::{self, paths};
use crate::rules::{
    Action, GreeterMode, Job, Logind, Micros, RecordDraft, SECOND, Seen, State, Tty, limits,
};
use crate::world::World;

pub const SYSTEMCTL: &str = "/usr/bin/systemctl";
pub const BUSCTL: &str = "/usr/bin/busctl";
pub const PLYMOUTH: &str = "/usr/bin/plymouth";

/// `CLOCK_MONOTONIC` in microseconds.
pub fn monotonic() -> Micros {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable timespec.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    (now.tv_sec.max(0) as u64) * SECOND + (now.tv_nsec.clamp(0, 999_999_999) as u64) / 1_000
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A fixed program with fixed arguments, an empty environment, no input,
/// and a limit after which it is killed.
fn command(program: &str, arguments: &[&str]) -> Command {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env_clear()
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null());
    command
}

/// Runs to completion within `limit`, or is killed. Standard output on
/// success.
pub fn run_bounded(program: &str, arguments: &[&str], limit: Duration) -> Result<String, String> {
    let mut child = command(program, arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not return within {limit:?}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(format!("{program}: {error}")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// `busctl`'s answer to seat0's `CanGraphical`: `b true`.
pub fn parse_can_graphical(output: &str) -> Option<bool> {
    match output.trim() {
        "b true" => Some(true),
        "b false" => Some(false),
        _ => None,
    }
}

/// The logind probe: one `busctl` call at a time, polled, never waited on.
struct Probe {
    running: Option<(Child, Micros)>,
    /// When the last probe ended, and what it said (`None`: no answer).
    last: Option<(Micros, Option<bool>)>,
}

impl Probe {
    fn poll(&mut self, now: Micros) {
        if let Some((child, started)) = &mut self.running {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut output = String::new();
                    if let Some(stdout) = child.stdout.as_mut() {
                        use std::io::Read as _;
                        let _ = stdout.take(4096).read_to_string(&mut output);
                    }
                    let answer = status
                        .success()
                        .then(|| parse_can_graphical(&output))
                        .flatten();
                    self.last = Some((now, answer));
                    self.running = None;
                }
                Ok(None) if now.saturating_sub(*started) > limits::CALL + SECOND => {
                    let _ = child.kill();
                    let _ = child.wait();
                    self.last = Some((now, None));
                    self.running = None;
                }
                Ok(None) => {}
                Err(_) => {
                    self.last = Some((now, None));
                    self.running = None;
                }
            }
        }
        let due = self
            .last
            .is_none_or(|(at, _)| now.saturating_sub(at) >= SECOND);
        if self.running.is_none() && due {
            let timeout = format!("--timeout={}", limits::CALL / SECOND);
            self.running = command(
                BUSCTL,
                &[
                    &timeout,
                    "get-property",
                    "org.freedesktop.login1",
                    "/org/freedesktop/login1/seat/seat0",
                    "org.freedesktop.login1.Seat",
                    "CanGraphical",
                ],
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()
            .map(|child| (child, now));
            if self.running.is_none() {
                self.last = Some((now, None));
            }
        }
    }

    /// What logind last said, while it is recent: an answer older than the
    /// call's limit and a second is no answer.
    fn answer(&self, now: Micros) -> Option<bool> {
        let (at, answer) = self.last?;
        (now.saturating_sub(at) <= limits::CALL + SECOND)
            .then_some(answer)
            .flatten()
    }
}

enum PlymouthChild {
    None,
    Running(Child),
    Done,
}

/// The machine.
pub struct SystemWorld {
    probe: Probe,
    plymouth: PlymouthChild,
    notify: Option<std::os::unix::net::SocketAddr>,
    records: Records,
    boot_id: Option<String>,
}

impl SystemWorld {
    pub fn new() -> SystemWorld {
        SystemWorld {
            probe: Probe {
                running: None,
                last: None,
            },
            plymouth: PlymouthChild::None,
            notify: notify_address(),
            records: Records::system(),
            boot_id: manager_ipc::boot_watch::current_boot_id(),
        }
    }

    pub fn boot_id(&self) -> Option<String> {
        self.boot_id.clone()
    }

    /// When this unit began to start this boot: systemd's
    /// `InactiveExitTimestampMonotonic` for it.
    pub fn first_start(&self) -> Option<Micros> {
        run_bounded(
            SYSTEMCTL,
            &[
                "show",
                "--property=InactiveExitTimestampMonotonic",
                "--value",
                LOGIN_SUPERVISOR_UNIT,
            ],
            Duration::from_micros(limits::CALL),
        )
        .ok()?
        .trim()
        .parse()
        .ok()
    }

    fn plymouth_state(&mut self) -> Option<Job> {
        if let PlymouthChild::Running(child) = &mut self.plymouth
            && !matches!(child.try_wait(), Ok(None))
        {
            self.plymouth = PlymouthChild::Done;
        }
        match self.plymouth {
            PlymouthChild::None => None,
            PlymouthChild::Running(_) => Some(Job::Running),
            PlymouthChild::Done => Some(Job::Finished),
        }
    }

    fn record(&self, draft: &RecordDraft) -> Result<(), String> {
        let boot = self
            .boot_id
            .clone()
            .ok_or_else(|| "no boot id to name the record by".to_string())?;
        let at = unix_seconds();
        let event = Event {
            version: RECORD_VERSION,
            boot_id: boot.clone(),
            detected_at_unix_seconds: at,
            display_manager: LOGIN_SUPERVISOR_UNIT.to_string(),
            waited_seconds: draft.waited_seconds,
            evidence: draft.evidence.clone(),
            restart: draft.restart.clone(),
        };
        write_event(&self.records, &self.records.event_path(at, &boot), &event)
            .map_err(|e| e.to_string())?;
        prune(&self.records, &boot);
        Ok(())
    }
}

impl Default for SystemWorld {
    fn default() -> Self {
        Self::new()
    }
}

fn notify_address() -> Option<std::os::unix::net::SocketAddr> {
    let raw = std::env::var_os("NOTIFY_SOCKET")?;
    let raw = raw.to_str()?;
    if let Some(name) = raw.strip_prefix('@') {
        use std::os::linux::net::SocketAddrExt as _;
        std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).ok()
    } else {
        std::os::unix::net::SocketAddr::from_pathname(raw).ok()
    }
}

/// Every process whose name is `plymouthd`.
fn plymouthd_pids(proc: &Path) -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir(proc) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let comm = read::read_bounded(&entry.path().join("comm"), 64)?;
            (String::from_utf8_lossy(&comm).trim() == "plymouthd").then_some(pid)
        })
        .collect()
}

const KDSETMODE: libc::c_ulong = 0x4B3A;
const KD_TEXT: libc::c_ulong = 0;
const VT_SETMODE: libc::c_ulong = 0x5602;
const VT_ACTIVATE: libc::c_ulong = 0x5606;
const VT_AUTO: libc::c_char = 0;

/// `struct vt_mode` (linux/vt.h).
#[repr(C)]
struct VtMode {
    mode: libc::c_char,
    waitv: libc::c_char,
    relsig: libc::c_short,
    acqsig: libc::c_short,
    frsig: libc::c_short,
}

fn open_tty(tty: Tty) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(tty.device())
        .map_err(|e| format!("{}: {e}", tty.device()))
}

fn ioctl_result(result: libc::c_int, what: &str) -> Result<(), String> {
    if result < 0 {
        Err(format!("{what}: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

/// Gives the terminal back to text: a compositor or `plymouthd` killed
/// while holding it may have left it in graphics mode, or in `VT_PROCESS`
/// mode, where no switch away from it completes.
fn reset_tty(tty: Tty) -> Result<(), String> {
    let file = open_tty(tty)?;
    let fd = file.as_raw_fd();
    // SAFETY: a valid descriptor; KDSETMODE takes an integer argument.
    ioctl_result(
        unsafe { libc::ioctl(fd, KDSETMODE as _, KD_TEXT) },
        "KDSETMODE",
    )?;
    let mode = VtMode {
        mode: VT_AUTO,
        waitv: 0,
        relsig: 0,
        acqsig: 0,
        frsig: 0,
    };
    // SAFETY: a valid descriptor and a valid `struct vt_mode` that outlives
    // the call.
    ioctl_result(
        unsafe { libc::ioctl(fd, VT_SETMODE as _, &mode as *const VtMode) },
        "VT_SETMODE",
    )
}

/// What `chvt` does, on the terminal's own device.
fn activate(tty: Tty) -> Result<(), String> {
    let file = open_tty(tty)?;
    // SAFETY: a valid descriptor; VT_ACTIVATE takes the terminal's number.
    ioctl_result(
        unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                VT_ACTIVATE as _,
                libc::c_ulong::from(tty.number()),
            )
        },
        "VT_ACTIVATE",
    )
}

fn write_file(path: &str, text: &str) -> Result<(), String> {
    crate::state::write_atomic(Path::new(path), text.as_bytes()).map_err(|e| format!("{path}: {e}"))
}

impl World for SystemWorld {
    fn now(&mut self) -> Micros {
        monotonic()
    }

    fn observe(&mut self) -> Seen {
        let now = monotonic();
        self.probe.poll(now);
        let logind = self.probe.answer(now).and_then(|can_graphical| {
            Some(Logind {
                can_graphical,
                sessions: read::sessions(Path::new(paths::SESSIONS))?,
            })
        });
        Seen {
            drm: read::drm(Path::new(paths::DRM)),
            logind,
            greetd: read::greetd(Path::new(paths::GREETER_CGROUP), Path::new(paths::PROC)),
            heartbeat: read::heartbeat(Path::new(paths::HEARTBEAT)),
            plymouth: self.plymouth_state(),
            greeter_uid: read::greeter_uid(Path::new(paths::PASSWD)),
        }
    }

    fn perform(&mut self, action: &Action) -> Result<(), String> {
        let call = Duration::from_micros(limits::CALL);
        match action {
            Action::StartUnit(unit) => {
                run_bounded(SYSTEMCTL, &["--no-block", "start", unit.name()], call).map(drop)
            }
            Action::StopUnit(unit) => {
                run_bounded(SYSTEMCTL, &["--no-block", "stop", unit.name()], call).map(drop)
            }
            Action::Plymouth(plymouth) => {
                if let PlymouthChild::Running(child) = &mut self.plymouth {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                self.plymouth = match command(PLYMOUTH, plymouth.arguments())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => PlymouthChild::Running(child),
                    // No Plymouth installed: nothing to deactivate or quit.
                    Err(_) => PlymouthChild::Done,
                };
                Ok(())
            }
            Action::KillPlymouth => {
                if let PlymouthChild::Running(child) = &mut self.plymouth {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                self.plymouth = PlymouthChild::Done;
                for pid in plymouthd_pids(Path::new(paths::PROC)) {
                    // SAFETY: kill has no memory preconditions.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
                Ok(())
            }
            Action::SetGreeterMode(GreeterMode::Safe) => write_file(paths::GREETER_MODE, "safe\n"),
            Action::SetGreeterMode(GreeterMode::Plain) => {
                match std::fs::remove_file(paths::GREETER_MODE) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        Err(format!("{}: {error}", paths::GREETER_MODE))
                    }
                    _ => Ok(()),
                }
            }
            Action::ResetTty(tty) => reset_tty(*tty),
            Action::ActivateVt(tty) => activate(*tty),
            Action::WriteIssue(stage) => {
                let time = better_core::local_time::clock_time(
                    unix_seconds(),
                    better_core::local_time::TimeZone::System,
                );
                write_file(paths::ISSUE, &crate::text::issue(*stage, &time))
            }
            Action::MarkFallbackActive(tty) => {
                write_file(paths::FALLBACK_ACTIVE, &format!("{}\n", tty.name()))
            }
            Action::MarkHeld => {
                if Path::new(paths::HELD).exists() {
                    Ok(())
                } else {
                    write_file(paths::HELD, "")
                }
            }
            Action::Journal(line) => {
                eprintln!("{line}");
                Ok(())
            }
            Action::Record(draft) => self.record(draft),
        }
    }

    fn save(&mut self, state: &State) -> Result<(), String> {
        crate::state::save(Path::new(paths::STATE), state).map_err(|e| e.to_string())
    }

    fn notify(&mut self, message: &str) {
        let Some(address) = &self.notify else {
            return;
        };
        if let Ok(socket) = std::os::unix::net::UnixDatagram::unbound() {
            let _ = socket.send_to_addr(message.as_bytes(), address);
        }
    }

    fn log(&mut self, line: &str) {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busctls_answer_is_read_and_anything_else_is_no_answer() {
        assert_eq!(parse_can_graphical("b true\n"), Some(true));
        assert_eq!(parse_can_graphical("b false"), Some(false));
        assert_eq!(parse_can_graphical(""), None);
        assert_eq!(parse_can_graphical("s \"yes\""), None);
    }

    #[test]
    fn a_command_that_does_not_return_is_killed_at_its_limit() {
        let started = Instant::now();
        let result = run_bounded("/bin/sleep", &["10"], Duration::from_millis(200));
        assert!(result.is_err(), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            run_bounded("/bin/echo", &["b true"], Duration::from_secs(5)).as_deref(),
            Ok("b true\n")
        );
        assert!(run_bounded("/bin/false", &[], Duration::from_secs(5)).is_err());
    }

    #[test]
    fn plymouthd_is_found_by_its_name_only() {
        let dir = tempfile::tempdir().unwrap();
        for (pid, name) in [(10, "plymouthd"), (11, "plymouth"), (12, "plymouthd-x")] {
            std::fs::create_dir_all(dir.path().join(pid.to_string())).unwrap();
            std::fs::write(dir.path().join(format!("{pid}/comm")), format!("{name}\n")).unwrap();
        }
        assert_eq!(plymouthd_pids(dir.path()), [10]);
    }
}
