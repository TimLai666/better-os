//! Servers typed in the path bar: `sftp://host/`, `smb://host/share`,
//! `ftp://`, `dav://`, `davs://`, `afp://`, `nfs://`, mounted through GVfs
//! and listed through the folder GVfs shows them under
//! (`/run/user/<uid>/gvfs/…`) like any other folder.
//!
//! **How (ticket 117).** [`crate::gvfs::GvfsMounter`] asks GVfs's own mount
//! tracker on the session bus to mount the address, with a mount operation
//! of Better Files' own: every question GVfs asks (a name and password, a
//! host key it does not know) arrives as a [`Prompt`], is shown in the
//! window, and the person's [`Answer`] goes back the same way. Nothing is
//! scripted from a terminal's prompts.
//!
//! Only when GVfs is not on the bus does [`GioMounter`] run `gio mount`,
//! in the C locale so its prompts can be recognised, its arguments as a
//! list and never through a shell. Its output is never shown as the
//! message: a failure is a [`MountError`] with a plain sentence, and what
//! `gio` said is only the detail under it.
//!
//! A password is never logged and never put in a process's arguments: it
//! goes to GVfs over the bus (or to `gio` on its standard input), and only
//! GVfs and the keyring keep it, when the person chose to remember it.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The schemes GVfs mounts that a person types.
pub const SCHEMES: [&str; 7] = ["sftp", "smb", "ftp", "dav", "davs", "afp", "nfs"];

/// How long a mount may go without news (no question asked, no answer
/// given) before it is given up on. Time the person spends answering does
/// not count.
pub const MOUNT_TIMEOUT: Duration = Duration::from_secs(30);

/// The address, trimmed, when it is a server address this opens.
pub fn server_address(text: &str) -> Option<String> {
    let text = text.trim();
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    (SCHEMES.contains(&scheme.as_str()) && !rest.is_empty() && !rest.starts_with('/'))
        .then(|| format!("{scheme}://{rest}"))
}

/// The server's name in an address, for the sign-in page's title
/// (`sftp://ann@build.local:2222/srv` is `build.local`).
pub fn server_name(address: &str) -> String {
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or(bracketed),
        None => host.split(':').next().unwrap_or(host),
    };
    percent_decode(host)
}

/// `%20` and the like in an address, decoded; anything malformed is kept.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = text.get(index + 1..index + 3)
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The local folder in `gio info` output (`local path: /run/user/…`).
pub fn local_path_from_info(output: &str) -> Option<PathBuf> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("local path:")
            .map(|path| PathBuf::from(path.trim()))
            .filter(|path| path.is_absolute())
    })
}

/// What `gio mount` printed, in the C locale, that means "a password is
/// wanted".
pub fn asks_for_password(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("password") || lower.contains("permission denied") || lower.contains("login")
}

/// Which fields a sign-in asks for (GIO's `GAskPasswordFlags`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SignInFields {
    pub password: bool,
    pub user: bool,
    pub domain: bool,
    /// The password can be remembered (for this session, or in the
    /// keyring).
    pub remember: bool,
    /// The server can be used without signing in.
    pub anonymous: bool,
}

impl SignInFields {
    /// From GIO's `GAskPasswordFlags` bits.
    pub fn from_bits(bits: u32) -> Self {
        Self {
            password: bits & 1 != 0,
            user: bits & 2 != 0,
            domain: bits & 4 != 0,
            remember: bits & 8 != 0,
            anonymous: bits & 16 != 0,
        }
    }
}

/// How long a password is kept (GIO's `GPasswordSave`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Remember {
    /// Forgotten as soon as the server is open.
    #[default]
    Never,
    /// Until the person logs out.
    Session,
    /// In the keyring.
    Forever,
}

impl Remember {
    pub const ALL: [Remember; 3] = [Remember::Never, Remember::Session, Remember::Forever];

    pub fn as_bits(self) -> u32 {
        match self {
            Remember::Never => 0,
            Remember::Session => 1,
            Remember::Forever => 2,
        }
    }
}

/// A question GVfs asks while it mounts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Prompt {
    /// A name, a password, a domain: whichever `fields` says.
    SignIn {
        /// GVfs's own words (in the desktop's language), shown small under
        /// the form.
        message: String,
        user: String,
        domain: String,
        fields: SignInFields,
        /// The last answer was refused: this is the same question again.
        again: bool,
    },
    /// A question with choices, such as whether to trust a host key it
    /// has not seen before.
    Question {
        message: String,
        choices: Vec<String>,
    },
}

/// The person's answer to a [`Prompt`]. Not `Clone`: the password in it
/// overwrites itself when the answer is dropped.
#[derive(Debug)]
pub enum Answer {
    SignIn {
        user: String,
        domain: String,
        password: crate::secret::Secret,
        anonymous: bool,
        remember: Remember,
    },
    /// The index of the choice taken.
    Choice(u32),
    /// The person gave up: the mount ends without an error shown.
    Cancel,
}

/// A prompt on its way to the window, with where the answer goes.
#[derive(Debug)]
pub struct Ask {
    pub address: String,
    pub prompt: Prompt,
    pub reply: smol::channel::Sender<Answer>,
}

/// How a mount asks the window its questions. A question the window can no
/// longer answer (it closed) is answered with [`Answer::Cancel`].
#[derive(Clone, Debug)]
pub struct Asker {
    address: String,
    window: std::sync::mpsc::Sender<Ask>,
}

impl Asker {
    pub fn new(address: &str, window: std::sync::mpsc::Sender<Ask>) -> Self {
        Self {
            address: address.to_string(),
            window,
        }
    }

    /// Asks and waits for the answer.
    pub async fn ask(&self, prompt: Prompt) -> Answer {
        let (reply, answer) = smol::channel::bounded(1);
        let ask = Ask {
            address: self.address.clone(),
            prompt,
            reply,
        };
        if self.window.send(ask).is_err() {
            return Answer::Cancel;
        }
        answer.recv().await.unwrap_or(Answer::Cancel)
    }
}

/// What a host-key question means, read from GVfs's message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostKeyQuestion {
    /// A server this computer has not connected to before, and the key's
    /// fingerprint (`SHA256:…`).
    Unknown { fingerprint: String },
    /// The key GVfs was sent is not the one this computer knows for the
    /// server's address: possibly someone in between.
    Mismatch,
}

impl HostKeyQuestion {
    /// Reads GVfs's SFTP question. GVfs words it in the desktop's
    /// language, so it is recognised by being an SFTP question with two
    /// choices and by its fingerprint: a first-time question carries one,
    /// the address-mismatch warning does not.
    pub fn read(address: &str, message: &str, choices: &[String]) -> Option<Self> {
        if !address.starts_with("sftp://") || choices.len() != 2 {
            return None;
        }
        match fingerprint(message) {
            Some(fingerprint) => Some(Self::Unknown { fingerprint }),
            None => Some(Self::Mismatch),
        }
    }
}

/// The `SHA256:…` (or older `MD5:…`) fingerprint in a host-key message.
pub fn fingerprint(message: &str) -> Option<String> {
    ["SHA256:", "MD5:"].iter().find_map(|prefix| {
        let start = message.find(prefix)?;
        let token: String = message[start..]
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, ':' | '+' | '/' | '=')
            })
            .collect();
        (token.len() > prefix.len()).then_some(token)
    })
}

/// Why a server did not open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountFailure {
    /// The person cancelled; nothing is shown.
    Cancelled,
    /// Neither GVfs nor `gio` is there.
    NoGvfs,
    /// No news for [`MOUNT_TIMEOUT`].
    TimedOut,
    /// The name does not resolve.
    HostNotFound,
    /// Nothing listens there, or the network is not reachable.
    Refused,
    /// The server refused the name and password, or this person.
    PermissionDenied,
    /// The server's key is not the one this computer knows for it.
    HostKeyChanged,
    /// GVfs cannot open this kind of address.
    NotSupported,
    /// It mounted, but GVfs shows no folder for it (its FUSE part is not
    /// running).
    NoLocalFolder,
    /// Anything else.
    Failed,
}

/// A failure, with the server's or GVfs's own words for the detail line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountError {
    pub failure: MountFailure,
    pub detail: String,
}

impl MountError {
    pub fn new(failure: MountFailure, detail: impl Into<String>) -> Self {
        Self {
            failure,
            detail: detail.into(),
        }
    }
}

/// Mounts a server and says where its folder is, asking the window through
/// `asker` whatever the server wants to know. A trait so the session can be
/// tested without a server.
pub trait Mounter: Send + Sync {
    fn mount(&self, address: &str, asker: &Asker) -> Result<PathBuf, MountError>;
}

/// GVfs through its bus interface when it is there, `gio` otherwise.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemMounter;

impl Mounter for SystemMounter {
    fn mount(&self, address: &str, asker: &Asker) -> Result<PathBuf, MountError> {
        match crate::gvfs::GvfsMounter::session() {
            Some(gvfs) => gvfs.mount(address, asker),
            None => GioMounter.mount(address, asker),
        }
    }
}

/// The fallback: `gio mount` in the C locale, then `gio info`. Each
/// question it prints is asked through the window and `gio` is run again
/// with every answer so far on its standard input.
#[derive(Clone, Copy, Debug, Default)]
pub struct GioMounter;

/// How many times `gio mount` is run for one address, one per question.
const GIO_ATTEMPTS: usize = 4;

/// Runs `gio` with `arguments` in the C locale, writes `input` to it and
/// closes its standard input, and returns whether it succeeded and what it
/// printed.
fn run_gio(arguments: &[&str], input: &[String]) -> Result<(bool, String), MountError> {
    let mut child = Command::new("gio")
        .args(arguments)
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => MountError::new(MountFailure::NoGvfs, ""),
            _ => MountError::new(MountFailure::Failed, error.to_string()),
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        for line in input {
            let _ = writeln!(stdin, "{line}");
        }
    }
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > MOUNT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(MountError::new(MountFailure::TimedOut, ""));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(MountError::new(MountFailure::Failed, error.to_string())),
        }
    };
    let output = child
        .wait_with_output()
        .map_err(|error| MountError::new(MountFailure::Failed, error.to_string()))?;
    Ok((
        status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    ))
}

/// The last thing `gio` said, without its `gio: <address>: ` prefix: the
/// detail line under a failure.
pub fn gio_detail(output: &str) -> String {
    let line = output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let line = line.strip_prefix("gio: ").unwrap_or(line);
    match line.split_once(": ") {
        Some((head, rest)) if head.contains("://") => rest.to_string(),
        _ => line.to_string(),
    }
}

/// What a failed `gio mount` (C locale) means.
pub fn gio_failure(output: &str) -> MountFailure {
    let lower = output.to_lowercase();
    if lower.contains("cancel") {
        MountFailure::Cancelled
    } else if lower.contains("host key verification failed") {
        MountFailure::HostKeyChanged
    } else if lower.contains("not known") || lower.contains("could not resolve") {
        MountFailure::HostNotFound
    } else if lower.contains("connection refused") || lower.contains("unreachable") {
        MountFailure::Refused
    } else if asks_for_password(output) {
        MountFailure::PermissionDenied
    } else if lower.contains("not supported") {
        MountFailure::NotSupported
    } else if lower.contains("timed out") {
        MountFailure::TimedOut
    } else {
        MountFailure::Failed
    }
}

/// gio's answers for one run: the host-key choice, then the sign-in.
fn gio_input(trusted: bool, sign_in: &[String]) -> Vec<String> {
    let mut input = Vec::new();
    if trusted {
        input.push("1".to_string());
    }
    input.extend(sign_in.iter().cloned());
    input
}

impl Mounter for GioMounter {
    fn mount(&self, address: &str, asker: &Asker) -> Result<PathBuf, MountError> {
        let mut trusted = false;
        let mut sign_in: Vec<String> = Vec::new();
        let mut mounted = false;
        for _ in 0..GIO_ATTEMPTS {
            let (success, said) = run_gio(&["mount", address], &gio_input(trusted, &sign_in))?;
            if success || said.to_lowercase().contains("already mounted") {
                mounted = true;
                break;
            }
            // A question gio could not answer from what it was given is
            // asked through the window, and gio is run again with it.
            if said.contains("Choice:") && !trusted {
                let message = said
                    .lines()
                    .take_while(|line| !line.trim_start().starts_with('['))
                    .collect::<Vec<_>>()
                    .join("\n");
                let choices: Vec<String> = said
                    .lines()
                    .filter_map(|line| line.trim().strip_prefix('[')?.split_once("] "))
                    .map(|(_, label)| label.to_string())
                    .collect();
                match smol::block_on(asker.ask(Prompt::Question { message, choices })) {
                    Answer::Choice(0) => trusted = true,
                    _ => return Err(MountError::new(MountFailure::Cancelled, "")),
                }
                continue;
            }
            if asks_for_password(&said) {
                let answer = smol::block_on(asker.ask(Prompt::SignIn {
                    message: String::new(),
                    user: std::env::var("USER").unwrap_or_default(),
                    domain: String::new(),
                    fields: SignInFields {
                        password: true,
                        user: true,
                        domain: address.starts_with("smb://"),
                        remember: false,
                        anonymous: false,
                    },
                    again: !sign_in.is_empty(),
                }));
                let Answer::SignIn {
                    user,
                    domain,
                    password,
                    ..
                } = answer
                else {
                    return Err(MountError::new(MountFailure::Cancelled, ""));
                };
                sign_in = vec![user];
                if address.starts_with("smb://") {
                    sign_in.push(domain);
                }
                sign_in.push(password.expose().to_string());
                continue;
            }
            return Err(MountError::new(gio_failure(&said), gio_detail(&said)));
        }
        if !mounted {
            return Err(MountError::new(MountFailure::PermissionDenied, ""));
        }
        let (_, info) = run_gio(&["info", address], &[])?;
        local_path_from_info(&info)
            .ok_or_else(|| MountError::new(MountFailure::NoLocalFolder, gio_detail(&info)))
    }
}
