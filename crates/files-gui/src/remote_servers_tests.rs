//! Ticket 310's check against real servers: an OpenSSH `sshd` and a Samba
//! `smbd`, each run unprivileged by the test on a loopback port with a
//! generated host key and fixture accounts, reached through the installed
//! GVfs (`gvfsd`, its SFTP and SMB backends, and `gvfsd-fuse`) on a private
//! bus of the test's own, with its own `HOME` and runtime folder, no keyring
//! and no SSH agent. Nothing here reaches the session bus, the person's GVfs,
//! their `~/.ssh` or any server but the test's own.
//!
//! A server is "stopped" by ending its processes and "started again" by
//! running it again on the same port, which is what GVfs sees when a
//! server restarts or the network goes away and comes back.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use files_core::{EntryId, Location};
use files_operations::{EngineConfig, JobEngine};
use files_platform::{MountTable, ReaderConfig, UserDirectories};

use crate::bookmarks::BookmarkStore;
use crate::network::{Answer, Prompt, Remember};
use crate::prefs::{FilesPreferences, PreferenceStore};
use crate::reader::FilesReader;
use crate::remote::Reason;
use crate::session::{FilesSession, LinkState, Notice, SessionSetup};

const SMB_PASSWORD: &str = "fixture-pass";

/// The fixture SFTP server's name in the sandbox's `ssh_config` only.
const SSH_ALIAS: &str = "fixture-nas";

fn program(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("{} is not installed", path.display()))
    }
}

fn on_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("{name} is not on PATH"))
}

fn user_name() -> String {
    // SAFETY: getpwuid returns this process's own entry or null.
    let name = unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr((*entry).pw_name)
    };
    name.to_string_lossy().into_owned()
}

/// A free loopback port.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for_port(port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Every process descended from `root`, deepest last, read from `/proc`.
fn descendants(root: u32) -> Vec<u32> {
    let mut parents: Vec<(u32, u32)> = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            // `pid (comm) state ppid …`: the name may hold spaces.
            let Some(after) = stat.rsplit_once(')').map(|(_, after)| after) else {
                continue;
            };
            if let Some(ppid) = after.split_whitespace().nth(1).and_then(|p| p.parse().ok()) {
                parents.push((pid, ppid));
            }
        }
    }
    let mut found = vec![root];
    let mut index = 0;
    while index < found.len() {
        let parent = found[index];
        found.extend(
            parents
                .iter()
                .filter(|(_, ppid)| *ppid == parent)
                .map(|(pid, _)| *pid),
        );
        index += 1;
    }
    found.remove(0);
    found
}

fn end(pids: &[u32], signal: i32) {
    for pid in pids {
        // SAFETY: a plain kill of a process this test started.
        unsafe {
            libc::kill(*pid as i32, signal);
        }
    }
}

/// A process the test started, ended with everything it started.
struct Started(Option<Child>);

impl Started {
    fn stop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let below = descendants(child.id());
            end(&below, libc::SIGKILL);
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Stops it without ending it: what a server that silently stops
    /// answering looks like from here.
    fn freeze(&self) -> Vec<u32> {
        let Some(child) = &self.0 else {
            return Vec::new();
        };
        let mut all = descendants(child.id());
        all.push(child.id());
        end(&all, libc::SIGSTOP);
        all
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The test's own computer: a scratch tree with a home, a runtime folder,
/// the servers' files and configurations, and a private bus whose only
/// service is GVfs.
struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    runtime: PathBuf,
    gvfs: PathBuf,
    srv: PathBuf,
    share: PathBuf,
    bus_address: String,
    bus: Started,
    sshd: Started,
    sshd_port: u16,
    password_sshd: Started,
    password_port: u16,
    smbd: Started,
    smbd_port: u16,
    user: String,
}

impl Sandbox {
    fn start() -> Result<Sandbox, String> {
        for needed in [
            "/usr/sbin/sshd",
            "/usr/sbin/smbd",
            "/usr/libexec/gvfsd",
            "/usr/libexec/gvfsd-fuse",
            "/usr/libexec/gvfsd-sftp",
            "/usr/libexec/gvfsd-smb",
            "/usr/bin/ssh",
        ] {
            program(needed)?;
        }
        for needed in ["dbus-daemon", "ssh-keygen", "pdbedit", "fusermount3"] {
            on_path(needed)?;
        }
        if !Path::new("/dev/fuse").exists() {
            return Err("/dev/fuse is not there".into());
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        // Short: a socket's path (the bus, ssh's control socket under the
        // runtime folder) has to fit in 108 bytes.
        let root = PathBuf::from(format!(
            "/tmp/bf310-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        let runtime = root.join("run");
        let srv = root.join("srv");
        let share = root.join("share");
        for dir in [
            &home.join("Documents"),
            &home.join(".config/gtk-3.0"),
            &runtime,
            &srv.join("projects/2026"),
            &share.join("reports"),
            &root.join("bin"),
            &root.join("ssh"),
            &root.join("bus/services"),
            &root.join("smb/private"),
        ] {
            fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        }
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        fs::write(srv.join("projects/plan.txt"), "plan").unwrap();
        fs::write(srv.join("projects/2026/notes.txt"), "notes").unwrap();
        fs::write(share.join("reports/q3.txt"), "q3").unwrap();
        let user = user_name();

        // The keys, generated: the servers' own, and the client's.
        for key in ["host", "client"] {
            let status = Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(root.join("ssh").join(key))
                .status()
                .map_err(|error| error.to_string())?;
            if !status.success() {
                return Err("ssh-keygen failed".into());
            }
        }
        fs::copy(
            root.join("ssh/client.pub"),
            root.join("ssh/authorized_keys"),
        )
        .map_err(|error| error.to_string())?;
        // The client's configuration, read only through the fixture ssh.
        fs::write(
            root.join("ssh/config"),
            format!(
                "Host *\n  UserKnownHostsFile {0}/ssh/known_hosts\n  GlobalKnownHostsFile /dev/null\n  \
                 IdentityFile {0}/ssh/client\n  IdentitiesOnly yes\n  StrictHostKeyChecking accept-new\n  \
                 ConnectTimeout 5\n  NumberOfPasswordPrompts 2\n",
                root.display()
            ),
        )
        .unwrap();
        // The fixture ssh, reached through a link, never written here.
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/remote/ssh");
        std::os::unix::fs::symlink(&fixture, root.join("bin/ssh"))
            .map_err(|error| error.to_string())?;

        let mut sandbox = Sandbox {
            bus_address: format!("unix:path={}/bus/socket", root.display()),
            root,
            home,
            runtime: runtime.clone(),
            gvfs: runtime.join("gvfs"),
            srv,
            share,
            bus: Started(None),
            sshd: Started(None),
            sshd_port: free_port(),
            password_sshd: Started(None),
            password_port: free_port(),
            smbd: Started(None),
            smbd_port: free_port(),
            user,
        };
        // A server known only by an alias in the sandbox's own ssh
        // configuration (ticket 310's second review): its name resolves
        // nowhere but in ssh.
        fs::OpenOptions::new()
            .append(true)
            .open(sandbox.root.join("ssh/config"))
            .and_then(|mut config| {
                write!(
                    config,
                    "Host {SSH_ALIAS}\n  HostName 127.0.0.1\n  Port {}\n",
                    sandbox.sshd_port
                )
            })
            .map_err(|error| error.to_string())?;
        sandbox.write_sshd("sshd", sandbox.sshd_port, false);
        sandbox.write_sshd("sshd-password", sandbox.password_port, true);
        sandbox.write_smb()?;
        sandbox.start_sshd()?;
        sandbox.start_password_sshd()?;
        sandbox.start_smbd()?;
        sandbox.start_bus()?;
        Ok(sandbox)
    }

    fn write_sshd(&self, name: &str, port: u16, password_only: bool) {
        let root = &self.root;
        let auth = if password_only {
            "PubkeyAuthentication no\nPasswordAuthentication yes\n"
        } else {
            "PubkeyAuthentication yes\nPasswordAuthentication no\n"
        };
        fs::write(
            root.join(format!("ssh/{name}_config")),
            format!(
                "ListenAddress 127.0.0.1\nListenAddress ::1\nPort {port}\nHostKey {0}/ssh/host\nPidFile none\n\
                 AuthorizedKeysFile {0}/ssh/authorized_keys\nUsePAM no\nStrictModes no\n\
                 KbdInteractiveAuthentication no\n{auth}Subsystem sftp internal-sftp\n",
                root.display()
            ),
        )
        .unwrap();
    }

    fn start_sshd_named(&self, name: &str, port: u16) -> Result<Started, String> {
        let child = Command::new("/usr/sbin/sshd")
            .args(["-D", "-e", "-f"])
            .arg(self.root.join(format!("ssh/{name}_config")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let started = Started(Some(child));
        if !wait_for_port(port) {
            return Err(format!("{name} did not listen on {port}"));
        }
        Ok(started)
    }

    fn start_sshd(&mut self) -> Result<(), String> {
        self.sshd = self.start_sshd_named("sshd", self.sshd_port)?;
        Ok(())
    }

    fn start_password_sshd(&mut self) -> Result<(), String> {
        self.password_sshd = self.start_sshd_named("sshd-password", self.password_port)?;
        Ok(())
    }

    fn write_smb(&self) -> Result<(), String> {
        let smb = self.root.join("smb");
        fs::write(
            smb.join("smb.conf"),
            format!(
                "[global]\n  workgroup = WORKGROUP\n  server role = standalone server\n  \
                 interfaces = 127.0.0.1\n  bind interfaces only = yes\n  smb ports = {port}\n  \
                 disable netbios = yes\n  private dir = {s}/private\n  state directory = {s}/state\n  \
                 cache directory = {s}/cache\n  lock directory = {s}/lock\n  pid directory = {s}/pid\n  \
                 ncalrpc dir = {s}/ncalrpc\n  log file = {s}/log.%m\n  \
                 passdb backend = tdbsam:{s}/private/passdb.tdb\n  map to guest = never\n  \
                 server min protocol = SMB2\n  load printers = no\n  printing = bsd\n  \
                 printcap name = /dev/null\n  disable spoolss = yes\n\
                 [data]\n  path = {share}\n  read only = no\n  valid users = {user}\n",
                port = self.smbd_port,
                s = smb.display(),
                share = self.share.display(),
                user = self.user,
            ),
        )
        .map_err(|error| error.to_string())?;
        for dir in ["state", "cache", "lock", "pid", "ncalrpc"] {
            fs::create_dir_all(smb.join(dir)).map_err(|error| error.to_string())?;
        }
        let mut pdbedit = Command::new("pdbedit")
            .arg("-s")
            .arg(smb.join("smb.conf"))
            .args(["-a", "-u", &self.user, "-t"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        if let Some(mut input) = pdbedit.stdin.take() {
            let _ = writeln!(input, "{SMB_PASSWORD}\n{SMB_PASSWORD}");
        }
        let status = pdbedit.wait().map_err(|error| error.to_string())?;
        if !status.success() {
            return Err("pdbedit could not add the fixture account".into());
        }
        Ok(())
    }

    fn start_smbd(&mut self) -> Result<(), String> {
        let child = Command::new("/usr/sbin/smbd")
            .args(["-F", "--debug-stdout", "-d", "0", "-s"])
            .arg(self.root.join("smb/smb.conf"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        self.smbd = Started(Some(child));
        if !wait_for_port(self.smbd_port) {
            return Err("smbd did not listen (it may need root here)".into());
        }
        Ok(())
    }

    fn start_bus(&mut self) -> Result<(), String> {
        let bus = self.root.join("bus");
        fs::write(
            bus.join("services/org.gtk.vfs.Daemon.service"),
            "[D-BUS Service]\nName=org.gtk.vfs.Daemon\nExec=/usr/libexec/gvfsd\n",
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            bus.join("bus.conf"),
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><keep_umask/>\
                 <listen>unix:path={0}/socket</listen><auth>EXTERNAL</auth>\
                 <servicedir>{0}/services</servicedir>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
                bus.display()
            ),
        )
        .map_err(|error| error.to_string())?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", bus.join("bus.conf").display()))
            .args(["--nofork", "--print-address"])
            // What GVfs and the ssh it runs see: the sandbox's, nothing of
            // the person's (no agent, no keyring, no real home).
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus_address)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .env("BETTER_FILES_TEST_SSH_CONFIG", self.root.join("ssh/config"))
            .env("LANG", "C.UTF-8")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut address = String::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = BufReader::new(stdout).read_line(&mut address);
        }
        self.bus = Started(Some(child));
        if address.trim().is_empty() {
            return Err("the private bus did not start".into());
        }
        Ok(())
    }

    /// A session of Better Files on this computer: GVfs on the private bus,
    /// the real mounter and mount list, the sandbox's GVfs folder.
    fn session(&self) -> FilesSession {
        files_platform::WatchHub::isolate_this_thread();
        let engine = Arc::new(JobEngine::new(EngineConfig {
            store: None,
            displaced_trash: Some(self.home.join(".trash-for-tests")),
            ..EngineConfig::default()
        }));
        let reader = Arc::new(
            FilesReader::new(
                ReaderConfig::new(),
                None,
                crate::apps::CatalogHandle::empty(Default::default()),
            )
            .with_gvfs_root(Some(self.gvfs.clone())),
        );
        let mut session = FilesSession::new(SessionSetup {
            start: Location::local(self.home.join("Documents")).unwrap(),
            preferences: FilesPreferences::default(),
            preference_store: PreferenceStore::at_path(self.home.join("prefs.json")),
            view_store: crate::settings_store::ViewStateStore::at_path(
                self.home.join("views.json"),
            ),
            bookmark_store: BookmarkStore::at_path(self.home.join(".config/gtk-3.0/bookmarks")),
            directories: UserDirectories::from_values(Some(&self.home), None),
            mounts: MountTable::new(Vec::new()),
            reader: reader.clone(),
            engine,
            catalog: reader.catalog().clone(),
            defaults: Box::new(crate::openwith::SessionDefaults::fixed([])),
            spawner: Box::new(app_catalog_platform::RecordingSpawner::new()),
            link: Box::new(crate::devices::NoDeviceLink),
            preview: crate::preview::PreviewPanel::default(),
        });
        // The installed gvfsd and its installed backends are checked as in
        // the window; only the systemd unit is not, since the private bus
        // starts this gvfsd itself rather than `systemd --user`.
        let trust = crate::gvfs_trust::Trust::system().map(|mut trust| {
            trust.unit = None;
            trust
        });
        let mounter = crate::gvfs::GvfsMounter::at_trusting(&self.bus_address, trust)
            .expect("GVfs on the private bus");
        session.set_remote(
            Some(self.gvfs.clone()),
            Arc::new(crate::remote::GvfsTracker::at(
                &self.bus_address,
                Some(self.gvfs.clone()),
            )),
            Arc::new(mounter),
        );
        session.set_network_up(Arc::new(|| true));
        session
    }

    fn sftp(&self, path: &Path) -> String {
        format!("sftp://127.0.0.1:{}{}", self.sshd_port, path.display())
    }

    fn smb(&self, inside: &str) -> String {
        format!("smb://127.0.0.1:{}/data{inside}", self.smbd_port)
    }

    /// The folder GVfs shows `path` under when the server is opened by its
    /// `ssh_config` alias.
    fn alias_folder(&self, path: &Path) -> PathBuf {
        self.gvfs
            .join(format!("sftp:host={SSH_ALIAS}"))
            .join(path.strip_prefix("/").unwrap())
    }

    /// The folder GVfs shows `path` on the SFTP server under.
    fn sftp_folder(&self, path: &Path) -> PathBuf {
        self.gvfs
            .join(format!("sftp:host=127.0.0.1,port={}", self.sshd_port))
            .join(path.strip_prefix("/").unwrap())
    }

    fn smb_folder(&self, inside: &str) -> PathBuf {
        self.gvfs
            .join(format!(
                "smb-share:port={},server=127.0.0.1,share=data",
                self.smbd_port
            ))
            .join(inside.trim_start_matches('/'))
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // The servers, then every process on the private bus (found by the
        // bus address in its environment, so a GVfs backend that outlived
        // the bus is ended too), then GVfs's FUSE folder.
        self.sshd.stop();
        self.password_sshd.stop();
        self.smbd.stop();
        let on_bus = processes_on(&self.bus_address);
        end(&on_bus, libc::SIGCONT);
        end(&on_bus, libc::SIGTERM);
        std::thread::sleep(Duration::from_millis(300));
        end(&processes_on(&self.bus_address), libc::SIGKILL);
        self.bus.stop();
        let _ = Command::new("fusermount3")
            .args(["-u", "-z"])
            .arg(&self.gvfs)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// This user's processes whose environment names `bus`.
fn processes_on(bus: &str) -> Vec<u32> {
    let needle = format!("DBUS_SESSION_BUS_ADDRESS={bus}");
    let starter = format!("DBUS_STARTER_ADDRESS={bus}");
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if pid == std::process::id() {
                continue;
            }
            let Ok(environ) = fs::read(entry.path().join("environ")) else {
                continue;
            };
            if environ
                .split(|byte| *byte == 0)
                .any(|variable| variable == needle.as_bytes() || variable == starter.as_bytes())
            {
                found.push(pid);
            }
        }
    }
    found
}

fn pump_until(
    session: &mut FilesSession,
    limit: Duration,
    done: impl Fn(&FilesSession) -> bool,
) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        session.pump();
        if done(session) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn shows(session: &FilesSession, folder: &Path) -> bool {
    session.location() == &Location::local(folder).unwrap()
        && session.link().is_none()
        && !session.is_listing()
        && matches!(
            session.pane().model().status(),
            files_core::ListingStatus::Complete
        )
}

/// Where the tab is, for a failure's message: its wait, its location, its
/// listing, the notice, and GVfs's folders.
fn where_(session: &FilesSession) -> String {
    format!(
        "{} || wait {:?}, at {:?}, listing {:?} (readings started {}, listing {}), \
         threads {}, notice {:?}, mounts {:?}",
        session.describe_remote_for_tests(),
        state(session),
        session.location(),
        session.pane().model().status(),
        session.pane().readings(),
        session.pane().is_listing(),
        threads(),
        session.notice,
        session
            .remote
            .gvfs_root
            .as_ref()
            .and_then(|root| fs::read_dir(root).ok())
            .map(|entries| entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>())
    )
}

/// Waits for `folder` to show after its server was started again. The
/// events it waits on are the product's: the folder listed, or its read
/// given up on past the listing deadline (the tab connecting again, which
/// is logged with where everything is, since that is a read that hung on
/// this machine), after which the product lists it again by itself.
fn opens_after_remount(session: &mut FilesSession, folder: &Path) -> bool {
    let wait_for_read = crate::remote::LISTING_DEADLINE + crate::remote::RETRY_EVERY;
    let shown_or_given_up = pump_until(session, OPEN, |s| {
        shows(s, folder)
            || matches!(
                s.pane().model().status(),
                files_core::ListingStatus::Failed(files_core::ListingError::NotAnswering { .. })
            )
    });
    if shows(session, folder) {
        return true;
    }
    if shown_or_given_up {
        eprintln!(
            "ticket-310: a read of the remounted folder was given up on: {}",
            where_(session)
        );
    }
    // The product's own promise: a read given up on is tried again by
    // itself, every retry interval, once the mount answers.
    pump_until(session, OPEN + wait_for_read, |s| shows(s, folder))
}

/// This process's threads that are not running, with the kernel function
/// each waits in (`/proc/self/task/*/wchan`): a listing stuck in FUSE shows
/// as a reader thread in `request_wait_answer` or the like.
fn threads() -> String {
    let mut out = Vec::new();
    for task in fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
    {
        let read = |name: &str| {
            fs::read_to_string(task.path().join(name))
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let name = read("comm");
        let wchan = read("wchan");
        if name.starts_with("files-") || wchan.contains("fuse") || wchan.contains("request") {
            out.push(format!("{name}:{wchan}"));
        }
    }
    out.join(" ")
}

fn names(session: &FilesSession) -> Vec<String> {
    let mut names: Vec<String> = session
        .pane()
        .model()
        .iter_visible()
        .map(|entry| entry.name.clone())
        .collect();
    names.sort();
    names
}

fn state(session: &FilesSession) -> Option<LinkState> {
    session.link().map(|link| link.state.clone())
}

const OPEN: Duration = Duration::from_secs(40);

/// One real-server sandbox at a time: two GVfs, two FUSE mounts and four
/// servers at once slow each other past the probes' own deadline.
static ONE_SANDBOX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn real_sftp_and_smb_servers_connect_again_when_their_folder_is_opened() {
    let _one = ONE_SANDBOX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut sandbox = test_prerequisites::require!(
        "sshd, smbd and pdbedit, GVfs with its SFTP, SMB and FUSE parts, dbus-daemon, \
         ssh-keygen, fusermount3 and /dev/fuse (packages openssh-server, samba, \
         gvfs-backends, gvfs-fuse, dbus)",
        Sandbox::start()
    );
    let mut session = sandbox.session();
    let projects = sandbox.srv.join("projects");

    // --- SFTP: pinned, the server stopped and started, the favourite opens.
    session.mount_server(sandbox.sftp(&projects));
    assert!(
        pump_until(&mut session, OPEN, |s| shows(
            s,
            &sandbox.sftp_folder(&projects)
        )),
        "the first connection: {:?} at {:?}",
        state(&session),
        session.location()
    );
    assert_eq!(names(&session), ["2026", "plan.txt"]);
    session.pin_current();
    let favourite = session.bookmarks.get(0).unwrap().location().clone();
    assert_eq!(favourite.to_uri(), sandbox.sftp(&projects));
    session.navigate_to(Location::local(sandbox.home.join("Documents")).unwrap());

    sandbox.sshd.stop();
    assert!(
        pump_until(&mut session, OPEN, |s| s.connected(&favourite)
            == Some(false)),
        "GVfs lets the mount go when its server stops"
    );
    // Server away: the page says why.
    session.navigate_to(favourite.clone());
    assert!(
        pump_until(&mut session, OPEN, |s| matches!(
            state(s),
            Some(LinkState::Failed {
                reason: Reason::Refused | Reason::NoAnswer,
                ..
            })
        )),
        "a server that is away: {:?}",
        where_(&session)
    );
    eprintln!("ticket-310: server away -> {:?}", state(&session));
    session.navigate_to(Location::local(sandbox.home.join("Documents")).unwrap());
    sandbox.start_sshd().unwrap();
    // The favourite, with nothing else done.
    session.navigate_to(favourite.clone());
    assert!(
        opens_after_remount(&mut session, &sandbox.sftp_folder(&projects)),
        "the favourite opens after the server is back: {:?}",
        where_(&session)
    );

    // --- A connection that drops while the folder is open comes back to the
    // same folder and selection.
    session.select_by_name("plan.txt");
    session.pump();
    // What this machine's network reads as, before and after the drop: a
    // restarted server comes back by itself unless a change is seen.
    eprintln!(
        "ticket-310: network before the drop: {:?}",
        crate::remote::read_network()
    );
    sandbox.sshd.stop();
    assert!(
        pump_until(&mut session, OPEN, |s| matches!(
            state(s),
            Some(LinkState::Connecting { again: true })
        )),
        "it says it is connecting again: {:?}",
        where_(&session)
    );
    sandbox.start_sshd().unwrap();
    assert!(
        opens_after_remount(&mut session, &sandbox.sftp_folder(&projects)),
        "it came back by itself: {:?} at {:?}, {:?}; network now {:?}",
        state(&session),
        session.location(),
        session.notice,
        crate::remote::read_network()
    );
    eprintln!(
        "ticket-310: network after it came back: {:?}",
        crate::remote::read_network()
    );
    assert_eq!(
        session.pane().model().selection().cursor(),
        Some(&EntryId::Name("plan.txt".into()))
    );

    // --- A server known only by its `ssh_config` alias (ticket 310's second
    // review): a look at the host from here cannot resolve it, so that look
    // never decides. Opened, stopped (the page keeps GVfs's own failure,
    // never "no computer called"), dropped while open, and back by itself.
    let by_alias = format!("sftp://{SSH_ALIAS}{}", projects.display());
    session.mount_server(by_alias.clone());
    assert!(
        pump_until(&mut session, OPEN, |s| shows(
            s,
            &sandbox.alias_folder(&projects)
        )),
        "the alias opens: {:?} at {:?}",
        state(&session),
        session.location()
    );
    sandbox.sshd.stop();
    assert!(
        pump_until(&mut session, OPEN, |s| matches!(
            state(s),
            Some(LinkState::Connecting { again: true })
        )),
        "the alias dropped: {:?}",
        where_(&session)
    );
    // Long enough for GVfs itself to be asked while the server is away.
    let away = Instant::now() + Duration::from_secs(8);
    while Instant::now() < away {
        session.pump();
        assert!(
            !matches!(
                state(&session),
                Some(LinkState::Failed {
                    reason: Reason::NameNotFound,
                    ..
                })
            ),
            "an alias is never \"no computer called\": {:?}",
            state(&session)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    eprintln!(
        "ticket-310: alias dropped, server away -> {:?}",
        where_(&session)
    );
    sandbox.start_sshd().unwrap();
    assert!(
        pump_until(&mut session, OPEN + Duration::from_secs(35), |s| shows(
            s,
            &sandbox.alias_folder(&projects)
        )),
        "the alias came back by itself: {:?}",
        where_(&session)
    );
    // Opened while its server is away: GVfs's own reason.
    session.navigate_to(Location::local(sandbox.home.join("Documents")).unwrap());
    sandbox.sshd.stop();
    assert!(pump_until(&mut session, OPEN, |s| s
        .connected(&Location::parse_uri(&by_alias))
        == Some(false)));
    session.mount_server(by_alias.clone());
    assert!(
        pump_until(&mut session, OPEN, |s| matches!(
            state(s),
            Some(LinkState::Failed { .. })
        )),
        "the alias away: {:?}",
        where_(&session)
    );
    eprintln!(
        "ticket-310: alias opened, server away -> {:?}",
        where_(&session)
    );
    assert!(
        matches!(
            state(&session),
            Some(LinkState::Failed {
                reason: Reason::Refused | Reason::NoAnswer,
                ..
            })
        ),
        "{:?}",
        where_(&session)
    );
    sandbox.start_sshd().unwrap();

    // --- A folder deleted on a server that connects: the nearest above.
    let deep = projects.join("2026");
    session.mount_server(sandbox.sftp(&deep));
    assert!(pump_until(&mut session, OPEN, |s| shows(
        s,
        &sandbox.sftp_folder(&deep)
    )));
    session.go_back();
    fs::remove_file(deep.join("notes.txt")).unwrap();
    fs::remove_dir(&deep).unwrap();
    session.go_forward();
    assert!(
        pump_until(&mut session, OPEN, |s| shows(
            s,
            &sandbox.sftp_folder(&projects)
        )),
        "the nearest folder that is there: {:?} at {:?}",
        state(&session),
        session.location()
    );
    assert!(matches!(
        &session.notice,
        Some(Notice::RemoteFolderGone { folder, server }) if folder == "2026" && server == "127.0.0.1"
    ));

    // --- A password the server does not take.
    session.mount_server(format!("sftp://127.0.0.1:{}/", sandbox.password_port));
    let mut asked = 0;
    let refused = pump_until(&mut session, OPEN, |s| {
        matches!(
            state(s),
            Some(LinkState::Failed {
                reason: Reason::SignInRefused,
                ..
            })
        ) || s.server_prompt.is_some()
    });
    assert!(refused, "a sign-in or its refusal: {:?}", state(&session));
    while session.server_prompt.is_some() {
        asked += 1;
        if let Some(Prompt::SignIn { again, .. }) =
            session.server_prompt.as_ref().map(|ask| &ask.prompt)
        {
            assert_eq!(*again, asked > 1, "the same form, saying it did not work");
        }
        session.answer_server(Answer::SignIn {
            user: sandbox.user.clone(),
            domain: String::new(),
            password: crate::secret::Secret::new("not-the-password"),
            anonymous: false,
            remember: Remember::Never,
        });
        pump_until(&mut session, OPEN, |s| {
            s.server_prompt.is_some() || matches!(state(s), Some(LinkState::Failed { .. }))
        });
        assert!(asked < 10, "asked without end");
    }
    assert!(
        matches!(
            state(&session),
            Some(LinkState::Failed {
                reason: Reason::SignInRefused,
                ..
            })
        ),
        "wrong password: {:?}",
        where_(&session)
    );
    eprintln!(
        "ticket-310: wrong SFTP password asked {asked} times -> {:?}",
        where_(&session)
    );

    // --- SMB: sign in (a wrong password first), pin, the server stopped and
    // started, the favourite opens, and a drop while open comes back.
    let reports = sandbox.smb("/reports");
    session.mount_server(reports.clone());
    let mut wrong_once = false;
    let opened = pump_until(&mut session, OPEN, |s| {
        s.server_prompt.is_some() || shows(s, &sandbox.smb_folder("/reports"))
    });
    assert!(opened, "SMB asks or opens: {:?}", state(&session));
    while let Some(Prompt::SignIn { again, .. }) =
        session.server_prompt.as_ref().map(|ask| ask.prompt.clone())
    {
        if wrong_once {
            assert!(again, "the same form, saying the password did not work");
        }
        let password = if wrong_once { SMB_PASSWORD } else { "wrong" };
        wrong_once = true;
        session.answer_server(Answer::SignIn {
            user: sandbox.user.clone(),
            domain: "WORKGROUP".into(),
            password: crate::secret::Secret::new(password),
            anonymous: false,
            remember: Remember::Never,
        });
        pump_until(&mut session, OPEN, |s| {
            s.server_prompt.is_some() || shows(s, &sandbox.smb_folder("/reports"))
        });
    }
    assert!(
        shows(&session, &sandbox.smb_folder("/reports")),
        "SMB opens: {:?} at {:?}",
        state(&session),
        session.location()
    );
    assert_eq!(names(&session), ["q3.txt"]);
    session.pin_current();
    assert!(
        session
            .bookmarks
            .bookmarks()
            .iter()
            .any(|bookmark| bookmark.location().to_uri() == reports),
        "pinned by its address"
    );
    let smb_favourite = Location::parse_uri(&reports);
    session.navigate_to(Location::local(sandbox.home.join("Documents")).unwrap());
    sandbox.smbd.stop();
    std::thread::sleep(Duration::from_secs(1));
    sandbox.start_smbd().unwrap();
    session.navigate_to(smb_favourite);
    assert!(
        pump_until(&mut session, OPEN, |s| shows(
            s,
            &sandbox.smb_folder("/reports")
        )),
        "the SMB favourite opens after its server is back: {:?}",
        where_(&session)
    );
    session.select_by_name("q3.txt");
    session.pump();
    sandbox.smbd.stop();
    session.reload();
    assert!(
        pump_until(&mut session, OPEN, |s| matches!(
            state(s),
            Some(LinkState::Connecting { again: true })
        )),
        "SMB away while open: {:?}",
        where_(&session)
    );
    sandbox.start_smbd().unwrap();
    assert!(
        pump_until(&mut session, OPEN + crate::remote::RETRY_EVERY, |s| shows(
            s,
            &sandbox.smb_folder("/reports")
        )),
        "SMB came back by itself: {:?}",
        where_(&session)
    );
    assert_eq!(
        session.pane().model().selection().cursor(),
        Some(&EntryId::Name("q3.txt".into()))
    );

    // --- The same share written another way is the same mount (ticket
    // 310's second review): gvfsd folds an SMB share to lower case and
    // writes an IPv6 host without brackets; neither makes a second mount.
    let mounts_now = |prefix: &str| {
        fs::read_dir(&sandbox.gvfs)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
                    .count()
            })
            .unwrap_or(0)
    };
    let shares = mounts_now("smb-share:");
    session.mount_server(format!(
        "smb://127.0.0.1:{}/DATA/reports",
        sandbox.smbd_port
    ));
    assert!(
        pump_until(&mut session, OPEN, |s| shows(
            s,
            &sandbox.smb_folder("/reports")
        )),
        "the share in capitals is the mounted one: {:?} at {:?}",
        state(&session),
        session.location()
    );
    assert_eq!(mounts_now("smb-share:"), shares, "no second SMB mount");
    let six = format!("sftp://[::1]:{}{}", sandbox.sshd_port, projects.display());
    session.mount_server(six.clone());
    let six_folder = sandbox
        .gvfs
        .join(format!("sftp:host=%3A%3A1,port={}", sandbox.sshd_port))
        .join(projects.strip_prefix("/").unwrap());
    if pump_until(&mut session, OPEN, |s| shows(s, &six_folder)) {
        let sftp_mounts = mounts_now("sftp:");
        session.navigate_to(Location::local(sandbox.home.join("Documents")).unwrap());
        session.mount_server(six);
        assert!(pump_until(&mut session, OPEN, |s| shows(s, &six_folder)));
        assert_eq!(mounts_now("sftp:"), sftp_mounts, "no second IPv6 mount");
        eprintln!("ticket-310: [::1] mounted once as {}", six_folder.display());
    } else {
        // A runner with no IPv6 loopback: said, not hidden.
        eprintln!(
            "ticket-310: [::1] not reachable here: {:?}",
            state(&session)
        );
        assert!(
            std::net::TcpStream::connect(("::1", sandbox.sshd_port)).is_err(),
            "[::1] answers but did not open: {:?}",
            state(&session)
        );
    }

    // --- What gvfsd-smb says for a name that does not resolve, a port
    // nothing listens on, and a share the server does not have (ticket 310's
    // reviews): GVfs's own failure, read as it says it. For a name it says
    // only "Invalid argument", so the page says it could not connect rather
    // than guess "no computer called" from a look that cannot see NetBIOS
    // names; "the share is gone" only comes from a server that answered.
    let closed = free_port();
    for (address, expected) in [
        (
            "smb://no-such-host.invalid/data".to_string(),
            Reason::Mount(crate::network::MountFailure::Failed),
        ),
        (format!("smb://127.0.0.1:{closed}/data"), Reason::Refused),
        (
            format!("smb://127.0.0.1:{}/nosuchshare", sandbox.smbd_port),
            Reason::ShareGone,
        ),
    ] {
        session.mount_server(address.clone());
        let user = sandbox.user.clone();
        let failed = |s: &FilesSession| matches!(state(s), Some(LinkState::Failed { .. }));
        let deadline = Instant::now() + OPEN;
        while !failed(&session) && Instant::now() < deadline {
            session.pump();
            if session.server_prompt.is_some() {
                session.answer_server(Answer::SignIn {
                    user: user.clone(),
                    domain: "WORKGROUP".into(),
                    password: crate::secret::Secret::new(SMB_PASSWORD),
                    anonymous: false,
                    remember: Remember::Never,
                });
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        eprintln!("ticket-310: {address} -> {:?}", state(&session));
        assert!(
            matches!(state(&session), Some(LinkState::Failed { reason, .. }) if reason == expected),
            "{address}: {:?}",
            state(&session)
        );
    }
}

/// A server that silently stops answering (its processes stopped, holding
/// the connection open): the window's work stays quick, the tab says it is
/// connecting again, and nothing waits for the dead mount.
#[test]
fn a_server_that_stops_answering_never_holds_the_window_up() {
    let _one = ONE_SANDBOX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let sandbox = test_prerequisites::require!(
        "sshd, GVfs with its SFTP and FUSE parts, dbus-daemon, ssh-keygen, smbd, pdbedit, \
         fusermount3 and /dev/fuse (packages openssh-server, samba, gvfs-backends, \
         gvfs-fuse, dbus)",
        Sandbox::start()
    );
    let mut session = sandbox.session();
    let projects = sandbox.srv.join("projects");
    session.mount_server(sandbox.sftp(&projects));
    assert!(pump_until(&mut session, OPEN, |s| shows(
        s,
        &sandbox.sftp_folder(&projects)
    )));
    session.pin_current();
    let frozen = sandbox.sshd.freeze();
    assert!(!frozen.is_empty());
    session.reload();
    // Every frame's work, measured, while the server does not answer: the
    // pump, the sidebar's rows, and a click on the favourite.
    let favourite = session.bookmarks.get(0).unwrap().location().clone();
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    let mut clicked = false;
    while started.elapsed() < Duration::from_secs(15) {
        let frame = Instant::now();
        session.pump();
        let rows = crate::sidebar::build_rows(
            &crate::sidebar::SidebarInputs {
                directories: &session.directories,
                mounts: &session.mounts,
                bookmarks: &session.bookmarks,
                states: &crate::sidebar::NoDeviceStates,
                probe: &session.probes,
                remote: &session,
            },
            &crate::i18n::EN_US,
        );
        assert!(!rows.is_empty());
        if !clicked && started.elapsed() > Duration::from_secs(5) {
            session.navigate_to(Location::local(sandbox.home.join("Documents")).unwrap());
            session.navigate_to(favourite.clone());
            clicked = true;
        }
        slowest = slowest.max(frame.elapsed());
        std::thread::sleep(Duration::from_millis(16));
    }
    eprintln!(
        "ticket-310: slowest frame of work with a server that does not answer: {slowest:?}; \
         the tab shows {:?}",
        where_(&session)
    );
    assert!(
        slowest < Duration::from_millis(100),
        "a frame waited {slowest:?} on a server that does not answer"
    );
    assert!(
        matches!(
            state(&session),
            Some(LinkState::Connecting { .. } | LinkState::Failed { .. })
        ),
        "{:?}",
        where_(&session)
    );
    end(&frozen, libc::SIGCONT);
}
