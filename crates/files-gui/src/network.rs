//! Servers typed in the path bar: `sftp://host/`, `smb://host/share`,
//! `ftp://`, `dav://`, `davs://`, `afp://`, `nfs://`. folio opens them through
//! GVfs (`src/PathBar.qml`, `src/MountPrompt.qml`); so does Better Files:
//! `gio mount <address>` mounts it, and the folder GVfs shows it under
//! (`/run/user/<uid>/gvfs/…`, from `gio info`) is listed like any other.
//!
//! `gio` is always started with its arguments as a list, never through a
//! shell, and the address is one argument. A server that wants a name and
//! password gets them on `gio mount`'s standard input, which is then
//! closed, so a prompt can never wait forever.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The schemes GVfs mounts that a person types.
pub const SCHEMES: [&str; 7] = ["sftp", "smb", "ftp", "dav", "davs", "afp", "nfs"];

/// How long a mount may take before it is given up on.
pub const MOUNT_TIMEOUT: Duration = Duration::from_secs(30);

/// The address, trimmed, when it is a server address this opens.
pub fn server_address(text: &str) -> Option<String> {
    let text = text.trim();
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    (SCHEMES.contains(&scheme.as_str()) && !rest.is_empty() && !rest.starts_with('/'))
        .then(|| format!("{scheme}://{rest}"))
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

/// A name and password for a server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credentials {
    pub user: String,
    pub password: String,
}

/// Why a server did not open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MountError {
    /// It asked for a name and password, or refused the ones given.
    NeedsPassword,
    /// `gio` is not installed.
    NoGio,
    /// It took longer than [`MOUNT_TIMEOUT`].
    TimedOut,
    /// Anything else, in `gio`'s own words.
    Failed(String),
}

/// Mounts a server and says where it is. A trait so the session can be
/// tested without a server.
pub trait Mounter: Send + Sync {
    fn mount(
        &self,
        address: &str,
        credentials: Option<&Credentials>,
    ) -> Result<PathBuf, MountError>;
}

/// The real one: `gio mount`, then `gio info`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GioMounter;

/// What `gio mount` printed that means "a password is wanted".
pub fn asks_for_password(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("password") || lower.contains("permission denied") || lower.contains("login")
}

impl Mounter for GioMounter {
    fn mount(
        &self,
        address: &str,
        credentials: Option<&Credentials>,
    ) -> Result<PathBuf, MountError> {
        let mut child = Command::new("gio")
            .args(["mount", address])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => MountError::NoGio,
                _ => MountError::Failed(error.to_string()),
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            // The answers to gio's prompts (user, domain for SMB, password),
            // or nothing; closing stdin ends any prompt left.
            if let Some(credentials) = credentials {
                let _ = writeln!(stdin, "{}", credentials.user);
                if address.starts_with("smb://") {
                    let _ = writeln!(stdin);
                }
                let _ = writeln!(stdin, "{}", credentials.password);
            }
        }
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() > MOUNT_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MountError::TimedOut);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(error) => return Err(MountError::Failed(error.to_string())),
            }
        };
        let output = child
            .wait_with_output()
            .map_err(|error| MountError::Failed(error.to_string()))?;
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        // "Already mounted" is a success: the folder is there.
        if !status.success() && !said.to_lowercase().contains("already mounted") {
            return Err(if asks_for_password(&said) {
                MountError::NeedsPassword
            } else {
                MountError::Failed(said.trim().to_string())
            });
        }
        let info = Command::new("gio")
            .args(["info", address])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| MountError::Failed(error.to_string()))?;
        local_path_from_info(&String::from_utf8_lossy(&info.stdout))
            .ok_or_else(|| MountError::Failed("no local path".to_string()))
    }
}
