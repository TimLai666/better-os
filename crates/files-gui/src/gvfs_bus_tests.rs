//! Ticket 117's mounts against a fake GVfs mount tracker on a private bus:
//! the real [`GvfsMounter`] exports its mount operation and calls
//! `MountLocation`; the fake asks its questions back on that operation the
//! way `gvfsd-sftp` and `gvfsd-smb` do (signatures read off GVfs 1.54), and
//! answers with GIO's errors by their bus names. Nothing here reaches the
//! session bus, the person's GVfs or any server.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::gvfs::GvfsMounter;
use crate::network::{
    Answer, Ask, Asker, HostKeyQuestion, MountFailure, Mounter, Prompt, Remember,
};
use crate::secret::Secret;

/// The mounter's limit for a server that says nothing, in the tests about
/// that limit: short enough for a test, long enough that a loaded runner's
/// fake still asks inside it. Every other test keeps the window's own
/// limit ([`crate::network::MOUNT_TIMEOUT`]), so a slow runner is never
/// mistaken for a silent server.
const QUIET_LIMIT: Duration = Duration::from_secs(5);

const FINGERPRINT: &str = "SHA256:7mj0NZesAHJ2Jo7UyMBrm/zFylxFPMVWG+oDHpeMz1E";

/// A private bus with no service files.
struct PrivateBus {
    child: Child,
    address: String,
    directory: PathBuf,
}

impl PrivateBus {
    fn start() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "better-os-files-gvfs-bus-{}-{}",
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
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is needed: the CI image installs it (package dbus)");
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
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// GIO's errors, by the names GVfs sends them under.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.gtk.GDBus.UnmappedGError.Quark._g_2dio_2derror_2dquark")]
enum GioError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Code0(String),
    Code14(String),
    Code16(String),
    Code17(String),
    Code30(String),
}

#[zbus::proxy(interface = "org.gtk.vfs.MountOperation")]
trait Operation {
    fn ask_password(
        &self,
        message: &str,
        default_user: &str,
        default_domain: &str,
        flags: u32,
    ) -> zbus::Result<(bool, bool, String, String, String, bool, u32)>;
    fn ask_question(&self, message: &str, choices: &[&str]) -> zbus::Result<(bool, bool, u32)>;
    fn aborted(&self) -> zbus::Result<()>;
}

/// How the fake server behaves.
#[derive(Clone, Copy, Debug)]
enum Server {
    /// SFTP with a password ("secret"), asked up to three times.
    Password,
    /// SMB with a user, a domain and a password, or anonymous access.
    Share,
    /// SFTP that first asks whether to trust its host key.
    NewHostKey,
    /// SFTP whose host key changed: refused outright.
    ChangedHostKey,
    /// Never answers.
    Silent,
    /// Mounted already.
    Mounted,
    /// Mounts, but never says so (the reply is lost).
    MountsSilently,
    /// Says nothing past the limit, then still asks for a password.
    AsksLate,
    /// Asks for a password, then withdraws the question (`Aborted`).
    Withdraws,
    /// Another process asks the window for a password during the mount,
    /// started as [`Launch`] says.
    AsksFrom(Launch),
}

/// How the process that asks is started, relative to this test (which
/// plays `gvfsd`).
#[derive(Clone, Copy, Debug)]
enum Launch {
    /// In a session of its own: no child of the daemon at all.
    Detached,
    /// A direct child running the named backend program.
    Child(&'static str),
    /// The named backend program, under a shell that is the daemon's child.
    Grandchild(&'static str),
}

/// What the fake tracker heard: the spec's keys and every answer.
#[derive(Default)]
struct Heard {
    spec: HashMap<String, String>,
    answers: Vec<String>,
}

struct FakeTracker {
    server: Server,
    folder: PathBuf,
    heard: Arc<Mutex<Heard>>,
    /// Whether the location is mounted (`LookupMount` finds it).
    mounted: Arc<std::sync::atomic::AtomicBool>,
    bus: String,
    /// Where the fake backends' programs are.
    libexec: PathBuf,
}

fn text(value: &OwnedValue) -> String {
    let bytes: Vec<u8> = value
        .try_clone()
        .ok()
        .and_then(|value| value.try_into().ok())
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .to_string()
}

type Spec = (Vec<u8>, HashMap<String, OwnedValue>);

impl FakeTracker {
    /// What the fake GVfs does with one mount.
    async fn serve(
        &self,
        operation: &OperationProxy<'_>,
        source: &(String, OwnedObjectPath),
        note: &impl Fn(String),
    ) -> Result<(), GioError> {
        let _ = source;
        match self.server {
            Server::Password => {
                for _ in 0..3 {
                    let (_, aborted, password, user, _, _, save) = operation
                        .ask_password("Enter password for ann on nas", "ann", "", 1 | 2 | 8)
                        .await?;
                    if aborted {
                        return Err(GioError::Code30("Password dialog cancelled".into()));
                    }
                    note(format!("{user}/{password}/{save}"));
                    if password == "secret" {
                        return Ok(());
                    }
                }
                Err(GioError::Code14("Permission denied".into()))
            }
            Server::Share => {
                let (_, aborted, password, user, domain, anonymous, _) = operation
                    .ask_password("Enter user and password for share", "ann", "WORKGROUP", 31)
                    .await?;
                if aborted {
                    return Err(GioError::Code30("Password dialog cancelled".into()));
                }
                note(format!("{domain}\\{user}/{password}/anonymous={anonymous}"));
                Ok(())
            }
            Server::NewHostKey => {
                let message = format!(
                    "Identity Verification Failed\nVerifying the identity of “nas” failed, this happens when you log in to a computer the first time.\n\nThe identity sent by the remote computer is “{FINGERPRINT}”. If you want to be absolutely sure it is safe to continue, contact the system administrator."
                );
                let (_, aborted, choice) = operation
                    .ask_question(&message, &["Log In Anyway", "Cancel Login"])
                    .await?;
                note(format!("choice={choice}/aborted={aborted}"));
                if aborted || choice != 0 {
                    return Err(GioError::Code30("Login dialog cancelled".into()));
                }
                Ok(())
            }
            Server::ChangedHostKey => Err(GioError::Code0("Host key verification failed".into())),
            Server::Silent => {
                smol::Timer::after(QUIET_LIMIT * 10).await;
                Ok(())
            }
            Server::Mounted => Err(GioError::Code17("Location is already mounted".into())),
            Server::MountsSilently => {
                self.mounted
                    .store(true, std::sync::atomic::Ordering::Release);
                smol::Timer::after(QUIET_LIMIT * 10).await;
                Ok(())
            }
            Server::AsksLate => {
                smol::Timer::after(QUIET_LIMIT * 2).await;
                let answer = operation.ask_password("Enter password", "ann", "", 1).await;
                note(match answer {
                    Ok((_, aborted, ..)) => format!("late: aborted={aborted}"),
                    Err(error) => format!("late: error {error}"),
                });
                Err(GioError::Code30("Password dialog cancelled".into()))
            }
            Server::Withdraws => {
                let (asked, withdrawn) = smol::future::zip(
                    operation.ask_password("Enter password", "ann", "", 1),
                    async {
                        smol::Timer::after(Duration::from_millis(400)).await;
                        operation.aborted().await
                    },
                )
                .await;
                withdrawn?;
                let (_, aborted, ..) = asked?;
                note(format!("withdrawn: aborted={aborted}"));
                Err(GioError::Code30("Password dialog cancelled".into()))
            }
            Server::AsksFrom(launch) => {
                let reply = ask_from(
                    launch,
                    &self.libexec,
                    &self.bus,
                    &source.0,
                    source.1.as_str(),
                );
                let signed_in = reply.contains("string \"secret\"");
                note(reply);
                if signed_in {
                    Ok(())
                } else {
                    Err(GioError::Code30("Password dialog cancelled".into()))
                }
            }
        }
    }
}

#[zbus::interface(name = "org.gtk.vfs.MountTracker")]
impl FakeTracker {
    async fn mount_location(
        &self,
        spec: Spec,
        source: (String, OwnedObjectPath),
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<(), GioError> {
        self.heard.lock().unwrap().spec = spec
            .1
            .iter()
            .map(|(key, value)| (key.clone(), text(value)))
            .collect();
        let operation = OperationProxy::builder(connection)
            .destination(source.0.clone())?
            .path(source.1.clone())?
            .build()
            .await?;
        let note = |what: String| self.heard.lock().unwrap().answers.push(what);
        let outcome = self.serve(&operation, &source, &note).await;
        if outcome.is_ok() {
            self.mounted
                .store(true, std::sync::atomic::Ordering::Release);
        }
        outcome
    }

    #[allow(clippy::type_complexity)]
    fn lookup_mount(
        &self,
        spec: Spec,
    ) -> Result<
        (
            String,
            OwnedObjectPath,
            String,
            String,
            String,
            String,
            String,
            String,
            bool,
            Vec<u8>,
            Spec,
            Vec<u8>,
        ),
        GioError,
    > {
        if !self.mounted.load(std::sync::atomic::Ordering::Acquire) {
            return Err(GioError::Code16(
                "The specified location is not mounted".into(),
            ));
        }
        let bytes = |text: &str| {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            bytes
        };
        Ok((
            ":1.1".into(),
            OwnedObjectPath::try_from("/org/gtk/vfs/mount/1").unwrap(),
            "ann on nas".into(),
            "sftp:host=nas".into(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            true,
            bytes(&self.folder.to_string_lossy()),
            spec,
            bytes("/home/ann"),
        ))
    }
}

/// The `AskPassword` call `dbus-send` makes, after its own program name.
fn ask_password_arguments(bus: &str, client: &str, path: &str) -> Vec<String> {
    [
        format!("--bus={bus}"),
        "--print-reply".into(),
        format!("--dest={client}"),
        path.into(),
        "org.gtk.vfs.MountOperation.AskPassword".into(),
        "string:Enter your password".into(),
        "string:ann".into(),
        "string:".into(),
        "uint32:1".into(),
    ]
    .into()
}

/// Another process (`dbus-send`, or a copy of it under a backend's name)
/// asks the window's mount operation for a password. What it got back.
fn ask_from(
    launch: Launch,
    libexec: &std::path::Path,
    bus: &str,
    client: &str,
    path: &str,
) -> String {
    let arguments = ask_password_arguments(bus, client, path);
    let mut command = match launch {
        Launch::Detached => return ask_detached(bus, client, path),
        Launch::Child(program) => {
            let mut command = Command::new(libexec.join(program));
            command.args(&arguments);
            command
        }
        Launch::Grandchild(program) => {
            // `sh` runs the program and waits for it, so the shell, not
            // the program, is the daemon's child.
            let mut command = Command::new("sh");
            command
                .arg("-c")
                .arg(r#""$0" "$@"; status=$?; exit "$status""#)
                .arg(libexec.join(program))
                .args(&arguments);
            command
        }
    };
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    // A program copied a moment ago may still be open for writing in a
    // process another test is forking ("text file busy"): try again.
    let mut tries = 0;
    let child = loop {
        match command.spawn() {
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => break other.expect("dbus-send is needed (dbus)"),
        }
    };
    let output = child.wait_with_output().expect("the asker ends");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// [`Launch::Detached`]: `dbus-send` in a session of its own (`setsid
/// -f`), so it is no child of this test.
fn ask_detached(bus: &str, client: &str, path: &str) -> String {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("reply");
    let status = Command::new("setsid")
        .arg("-f")
        .arg("sh")
        .arg("-c")
        .arg(r#"out="$1"; shift; exec dbus-send "$@" > "$out" 2>&1"#)
        .arg("impersonator")
        .arg(out.display().to_string())
        .args(ask_password_arguments(bus, client, path))
        .status()
        .expect("setsid and dbus-send are needed (util-linux, dbus)");
    assert!(status.success());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let reply = std::fs::read_to_string(&out).unwrap_or_default();
        if reply.contains("boolean") || reply.contains("Error") {
            std::thread::sleep(Duration::from_millis(50));
            return std::fs::read_to_string(&out).unwrap_or_default();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    "no reply".into()
}

/// The fake backends' programs: copies of `dbus-send` under GVfs's
/// backend names, in `libexec`.
const FAKE_BACKENDS: [&str; 2] = ["gvfsd-sftp", "gvfsd-smb"];

fn fake_backends(libexec: &std::path::Path) {
    let dbus_send = ["/usr/bin/dbus-send", "/bin/dbus-send"]
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .expect("dbus-send is needed (dbus)");
    std::fs::create_dir_all(libexec).unwrap();
    for program in FAKE_BACKENDS {
        std::fs::copy(dbus_send, libexec.join(program)).unwrap();
    }
}

/// What the tests trust as GVfs: this test program as `gvfsd` (it plays
/// the daemon), and the fake backends for `sftp` and `smb-share`.
fn test_trust(libexec: &std::path::Path) -> crate::gvfs_trust::Trust {
    let resolved = |program: &str| std::fs::canonicalize(libexec.join(program)).unwrap();
    crate::gvfs_trust::Trust {
        daemon: std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap(),
        unit: None,
        backends: HashMap::from([
            ("sftp".to_string(), vec![resolved("gvfsd-sftp")]),
            ("smb-share".to_string(), vec![resolved("gvfsd-smb")]),
        ]),
    }
}

/// A fake GVfs on its own bus, and a mounter on that bus.
struct Fixture {
    _bus: PrivateBus,
    _tracker: zbus::Connection,
    mounter: GvfsMounter,
    folder: PathBuf,
    heard: Arc<Mutex<Heard>>,
    _root: tempfile::TempDir,
}

fn fixture(server: Server) -> Fixture {
    fixture_with(server, crate::network::MOUNT_TIMEOUT)
}

/// The fake GVfs, with the mounter giving up after `quiet` without news.
fn fixture_with(server: Server, quiet: Duration) -> Fixture {
    fixture_trusting(server, quiet, |libexec| Ok(test_trust(libexec)))
}

/// The fake GVfs, the mounter trusting what `trust` says of the fake
/// backends' folder.
fn fixture_trusting(
    server: Server,
    quiet: Duration,
    trust: impl FnOnce(&std::path::Path) -> Result<crate::gvfs_trust::Trust, String>,
) -> Fixture {
    let bus = PrivateBus::start();
    let root = tempfile::tempdir().unwrap();
    let libexec = root.path().join("libexec");
    fake_backends(&libexec);
    let folder = root.path().join("gvfs/sftp:host=nas");
    let heard: Arc<Mutex<Heard>> = Arc::default();
    let tracker = smol::block_on(async {
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .serve_at(
                crate::gvfs::TRACKER_PATH,
                FakeTracker {
                    server,
                    folder: folder.clone(),
                    heard: Arc::clone(&heard),
                    mounted: Arc::new(std::sync::atomic::AtomicBool::new(matches!(
                        server,
                        Server::Mounted
                    ))),
                    bus: bus.address.clone(),
                    libexec: libexec.clone(),
                },
            )
            .unwrap()
            .name(crate::gvfs::DAEMON)
            .unwrap()
            .build()
            .await
            .unwrap()
    });
    let mut mounter = GvfsMounter::at_trusting(&bus.address, trust(&libexec))
        .expect("the fake GVfs is on the bus");
    mounter.quiet_limit = quiet;
    Fixture {
        _bus: bus,
        _tracker: tracker,
        mounter,
        folder,
        heard,
        _root: root,
    }
}

/// Mounts `address` on a thread of its own, answering each question with
/// `answer`, and returns the questions asked and how it ended.
fn mount(
    fixture: &Fixture,
    address: &str,
    mut answer: impl FnMut(&Prompt) -> Answer,
) -> (Vec<Prompt>, Result<PathBuf, crate::network::MountError>) {
    let (asks, questions): (_, Receiver<Ask>) = channel();
    let asker = Asker::new(address, asks);
    let mut asked = Vec::new();
    std::thread::scope(|scope| {
        let mounting = scope.spawn(|| fixture.mounter.mount(address, &asker));
        loop {
            match questions.recv_timeout(Duration::from_millis(50)) {
                Ok(ask) => {
                    let reply = answer(&ask.prompt);
                    asked.push(ask.prompt.clone());
                    let _ = ask.reply.try_send(reply);
                }
                Err(_) if mounting.is_finished() => break,
                Err(_) => {}
            }
        }
        (asked, mounting.join().unwrap())
    })
}

fn sign_in(user: &str, domain: &str, password: &str, remember: Remember) -> Answer {
    Answer::SignIn {
        user: user.into(),
        domain: domain.into(),
        password: Secret::new(password),
        anonymous: false,
        remember,
    }
}

#[test]
fn a_password_is_asked_in_the_window_and_accepted() {
    let fixture = fixture(Server::Password);
    let (asked, result) = mount(&fixture, "sftp://ann@nas/", |_| {
        sign_in("ann", "", "secret", Remember::Session)
    });
    assert_eq!(result.unwrap(), fixture.folder.join("home/ann"));
    assert_eq!(asked.len(), 1);
    let Prompt::SignIn {
        user,
        fields,
        again,
        ..
    } = &asked[0]
    else {
        panic!("{asked:?}");
    };
    assert_eq!((user.as_str(), *again), ("ann", false));
    assert!(fields.password && fields.user && fields.remember && !fields.domain);
    let heard = fixture.heard.lock().unwrap();
    assert_eq!(
        heard.answers,
        ["ann/secret/1"],
        "remember for the session is 1"
    );
    assert_eq!(heard.spec.get("type").map(String::as_str), Some("sftp"));
    assert_eq!(heard.spec.get("host").map(String::as_str), Some("nas"));
    assert_eq!(heard.spec.get("user").map(String::as_str), Some("ann"));
}

#[test]
fn a_wrong_password_asks_again_and_the_right_one_opens() {
    let fixture = fixture(Server::Password);
    let mut tries = ["wrong", "secret"].into_iter();
    let (asked, result) = mount(&fixture, "sftp://nas/srv/www", |_| {
        sign_in("ann", "", tries.next().unwrap_or("secret"), Remember::Never)
    });
    // The address's own folder inside the mount.
    assert_eq!(result.unwrap(), fixture.folder.join("srv/www"));
    let agains: Vec<bool> = asked
        .iter()
        .map(|prompt| matches!(prompt, Prompt::SignIn { again: true, .. }))
        .collect();
    assert_eq!(agains, [false, true]);
}

#[test]
fn a_share_asks_for_a_domain_and_can_be_used_anonymously() {
    let fixture = fixture(Server::Share);
    let (asked, result) = mount(&fixture, "smb://nas/photos", |_| {
        sign_in("ann", "WORK", "secret", Remember::Never)
    });
    assert!(result.is_ok(), "{result:?}");
    let Prompt::SignIn { fields, domain, .. } = &asked[0] else {
        panic!("{asked:?}");
    };
    assert!(fields.domain && fields.anonymous);
    assert_eq!(domain, "WORKGROUP");
    assert_eq!(
        fixture.heard.lock().unwrap().answers,
        ["WORK\\ann/secret/anonymous=false"]
    );
    assert_eq!(
        fixture
            .heard
            .lock()
            .unwrap()
            .spec
            .get("share")
            .map(String::as_str),
        Some("photos")
    );

    let fixture = self::fixture(Server::Share);
    let (_, result) = mount(&fixture, "smb://nas/photos", |_| Answer::SignIn {
        user: String::new(),
        domain: String::new(),
        password: Secret::new(""),
        anonymous: true,
        remember: Remember::Never,
    });
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        fixture.heard.lock().unwrap().answers,
        ["\\//anonymous=true"]
    );
}

#[test]
fn a_new_host_key_is_shown_with_its_fingerprint_and_can_be_accepted_or_refused() {
    let fixture = fixture(Server::NewHostKey);
    // The window's Connect anyway, against the order GVfs's SFTP backend
    // gives its choices in (the fake proceeds on choice 0 only, as
    // `login_answer_yes_no` does).
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| {
        Answer::Choice(crate::network::CONNECT_ANYWAY)
    });
    assert!(result.is_ok(), "{result:?}");
    let Prompt::Question { message, choices } = &asked[0] else {
        panic!("{asked:?}");
    };
    assert_eq!(
        HostKeyQuestion::read("sftp://nas/", message, choices),
        Some(HostKeyQuestion::Unknown {
            fingerprint: FINGERPRINT.into()
        })
    );

    let fixture = self::fixture(Server::NewHostKey);
    let (_, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert_eq!(result.unwrap_err().failure, MountFailure::Cancelled);
    assert_eq!(
        fixture.heard.lock().unwrap().answers,
        ["choice=0/aborted=true"]
    );
}

#[test]
fn a_changed_host_key_is_its_own_failure() {
    let fixture = fixture(Server::ChangedHostKey);
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert!(asked.is_empty());
    let error = result.unwrap_err();
    assert_eq!(error.failure, MountFailure::HostKeyChanged);
    assert_eq!(error.detail, "Host key verification failed");
}

#[test]
fn cancelling_the_sign_in_ends_the_mount_as_cancelled() {
    let fixture = fixture(Server::Password);
    let (_, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert_eq!(result.unwrap_err().failure, MountFailure::Cancelled);
}

#[test]
fn a_server_that_never_answers_times_out_but_a_person_thinking_does_not() {
    let fixture = fixture_with(Server::Silent, QUIET_LIMIT);
    let started = std::time::Instant::now();
    let (_, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert_eq!(result.unwrap_err().failure, MountFailure::TimedOut);
    assert!(started.elapsed() < QUIET_LIMIT + Duration::from_secs(5));

    // Taking longer than the limit to type the password is not a timeout.
    let fixture = fixture_with(Server::Password, QUIET_LIMIT);
    let (_, result) = mount(&fixture, "sftp://nas/", |_| {
        std::thread::sleep(QUIET_LIMIT + Duration::from_millis(700));
        sign_in("ann", "", "secret", Remember::Never)
    });
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn an_address_already_mounted_opens_its_folder() {
    let fixture = fixture(Server::Mounted);
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert!(asked.is_empty());
    assert_eq!(result.unwrap(), fixture.folder.join("home/ann"));
}

#[test]
fn a_mount_that_finished_unannounced_opens_after_the_wait() {
    let fixture = fixture_with(Server::MountsSilently, QUIET_LIMIT);
    let (_, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert_eq!(result.unwrap(), fixture.folder.join("home/ann"));
}

#[test]
fn a_question_after_the_mount_was_given_up_on_is_answered_aborted() {
    let fixture = fixture_with(Server::AsksLate, QUIET_LIMIT);
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert_eq!(result.unwrap_err().failure, MountFailure::TimedOut);
    assert!(asked.is_empty(), "nothing late reaches the window");
    let deadline = std::time::Instant::now() + QUIET_LIMIT * 4;
    while fixture.heard.lock().unwrap().answers.is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fixture.heard.lock().unwrap().answers,
        ["late: aborted=true"]
    );
}

#[test]
fn a_question_gvfs_withdraws_leaves_the_window() {
    let fixture = fixture(Server::Withdraws);
    let (asks, questions): (_, Receiver<Ask>) = channel();
    let asker = Asker::new("sftp://nas/", asks);
    std::thread::scope(|scope| {
        let mounting = scope.spawn(|| fixture.mounter.mount("sftp://nas/", &asker));
        let ask = questions
            .recv_timeout(Duration::from_secs(10))
            .expect("the sign-in was asked");
        // The person has not answered; GVfs withdraws the question, and the
        // window's end of it closes (the session then takes it off screen).
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !ask.reply.is_closed() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            ask.reply.is_closed(),
            "the question stayed after GVfs withdrew it"
        );
        assert_eq!(
            mounting.join().unwrap().unwrap_err().failure,
            MountFailure::Cancelled
        );
    });
    assert_eq!(
        fixture.heard.lock().unwrap().answers,
        ["withdrawn: aborted=true"]
    );
}

#[test]
fn another_program_cannot_ask_for_the_password() {
    let fixture = fixture(Server::AsksFrom(Launch::Detached));
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| {
        sign_in("ann", "", "secret", Remember::Never)
    });
    assert!(
        asked.is_empty(),
        "the impersonator's question reached the window: {asked:?}"
    );
    assert_eq!(result.unwrap_err().failure, MountFailure::Cancelled);
    let reply = fixture.heard.lock().unwrap().answers.join("\n");
    let values: Vec<&str> = reply.lines().skip(1).map(str::trim).collect();
    assert_eq!(
        values.get(..3),
        Some(&["boolean true", "boolean true", "string \"\""][..]),
        "handled, aborted, and no password: {reply}"
    );
}

/// The window's answer to the asker, and what the asker got back.
fn asked_from(
    launch: Launch,
) -> (
    Vec<Prompt>,
    Result<PathBuf, crate::network::MountError>,
    String,
) {
    let fixture = fixture(Server::AsksFrom(launch));
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| {
        sign_in("ann", "", "secret", Remember::Never)
    });
    let reply = fixture.heard.lock().unwrap().answers.join("\n");
    (asked, result, reply)
}

fn assert_refused(launch: Launch, what: &str) {
    let (asked, result, reply) = asked_from(launch);
    assert!(
        asked.is_empty(),
        "{what}'s question reached the window: {asked:?}"
    );
    assert_eq!(result.unwrap_err().failure, MountFailure::Cancelled);
    let values: Vec<&str> = reply.lines().skip(1).map(str::trim).collect();
    assert_eq!(
        values.get(..3),
        Some(&["boolean true", "boolean true", "string \"\""][..]),
        "{what}: handled, aborted, and no password: {reply}"
    );
}

#[test]
fn the_mount_s_own_backend_asks_and_gets_the_password() {
    let (asked, result, reply) = asked_from(Launch::Child("gvfsd-sftp"));
    assert_eq!(asked.len(), 1, "{reply}");
    assert!(result.is_ok(), "{result:?}");
    let values: Vec<&str> = reply.lines().skip(1).map(str::trim).collect();
    assert_eq!(
        values.get(..4),
        Some(
            &[
                "boolean true",
                "boolean false",
                "string \"secret\"",
                "string \"ann\""
            ][..]
        ),
        "{reply}"
    );
}

#[test]
fn a_descendant_that_is_not_a_direct_backend_child_is_refused() {
    assert_refused(Launch::Grandchild("gvfsd-sftp"), "a backend's own child");
}

#[test]
fn a_backend_for_another_scheme_is_refused() {
    assert_refused(
        Launch::Child("gvfsd-smb"),
        "the SMB backend, in an SFTP mount",
    );
}

#[test]
fn a_fake_owner_of_the_gvfs_name_is_refused() {
    // The fake tracker owns org.gtk.vfs.Daemon, but GVfs here is said to be
    // another program: nothing is sent to it, and nobody is asked.
    let fixture = fixture_trusting(Server::Password, crate::network::MOUNT_TIMEOUT, |libexec| {
        let mut trust = test_trust(libexec);
        trust.daemon = std::fs::canonicalize("/bin/sh").unwrap();
        Ok(trust)
    });
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| {
        sign_in("ann", "", "secret", Remember::Never)
    });
    assert!(asked.is_empty(), "{asked:?}");
    let error = result.unwrap_err();
    assert_eq!(error.failure, MountFailure::UntrustedGvfs, "{error:?}");
    assert!(error.detail.contains("not by"), "{error:?}");
    let heard = fixture.heard.lock().unwrap();
    assert!(
        heard.spec.is_empty(),
        "MountLocation reached the fake owner"
    );
}

#[test]
fn without_an_installed_gvfs_to_compare_with_nothing_is_mounted() {
    let fixture = fixture_trusting(Server::Password, crate::network::MOUNT_TIMEOUT, |_| {
        Err("GVfs's service file (org.gtk.vfs.Daemon.service) is not installed".into())
    });
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| {
        sign_in("ann", "", "secret", Remember::Never)
    });
    assert!(asked.is_empty());
    assert_eq!(result.unwrap_err().failure, MountFailure::UntrustedGvfs);
    assert!(fixture.heard.lock().unwrap().spec.is_empty());
}
