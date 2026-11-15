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
                smol::Timer::after(Duration::from_secs(20)).await;
                Ok(())
            }
            Server::Mounted => Err(GioError::Code17("Location is already mounted".into())),
        }
    }

    #[allow(clippy::type_complexity)]
    fn lookup_mount(
        &self,
        spec: Spec,
    ) -> (
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
    ) {
        let bytes = |text: &str| {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            bytes
        };
        (
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
        )
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
    let bus = PrivateBus::start();
    let root = tempfile::tempdir().unwrap();
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
                },
            )
            .unwrap()
            .name(crate::gvfs::DAEMON)
            .unwrap()
            .build()
            .await
            .unwrap()
    });
    let mut mounter = GvfsMounter::at(&bus.address).expect("the fake GVfs is on the bus");
    mounter.quiet_limit = Duration::from_millis(800);
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
    assert!(result.is_ok());
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
    assert!(result.is_ok());
    assert_eq!(
        fixture.heard.lock().unwrap().answers,
        ["\\//anonymous=true"]
    );
}

#[test]
fn a_new_host_key_is_shown_with_its_fingerprint_and_can_be_accepted_or_refused() {
    let fixture = fixture(Server::NewHostKey);
    let (asked, result) = mount(&fixture, "sftp://nas/", |_| Answer::Choice(0));
    assert!(result.is_ok());
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
    let fixture = fixture(Server::Silent);
    let started = std::time::Instant::now();
    let (_, result) = mount(&fixture, "sftp://nas/", |_| Answer::Cancel);
    assert_eq!(result.unwrap_err().failure, MountFailure::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));

    // Taking longer than the limit to type the password is not a timeout.
    let fixture = self::fixture(Server::Password);
    let (_, result) = mount(&fixture, "sftp://nas/", |_| {
        std::thread::sleep(Duration::from_millis(1500));
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
