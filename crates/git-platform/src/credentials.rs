//! Handing a GitHub token to one git command.
//!
//! The token is written to a file only this user can read, in a folder made
//! for the one command (under `$XDG_RUNTIME_DIR`, which is memory and is
//! the user's alone, or the temporary folder), and git is told to use a
//! credential helper that prints that file for the token's host. Every
//! other helper for that host is switched off for the command, so the
//! token is the one git sends. The command line and the environment the
//! programs git starts inherit hold only the file's path; the folder is
//! removed when the command ends.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::Credentials;

/// The token's file for one command. Dropping it removes the file.
pub struct Helper {
    host: String,
    dir: PathBuf,
    file: PathBuf,
}

impl Helper {
    pub fn new(credentials: &Credentials) -> io::Result<Helper> {
        let host = credentials.host.as_str();
        if host.is_empty()
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
        {
            return Err(io::Error::other("the GitHub host name is not valid"));
        }
        if credentials.token.contains(['\n', '\0']) {
            return Err(io::Error::other("the token is not valid"));
        }
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("better-git-{}", random_hex()?));
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let file = dir.join("credential");
        let written = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .and_then(|mut f| {
                write!(
                    f,
                    "username=x-access-token\npassword={}\n",
                    credentials.token
                )
            });
        let helper = Helper {
            host: host.to_string(),
            dir,
            file,
        };
        written?;
        Ok(helper)
    }

    /// `-c` arguments: clear the host's helpers, then add the one that
    /// prints the file when git asks for (`get`) a credential.
    pub fn config_args(&self) -> Vec<String> {
        let key = format!("credential.https://{}.helper", self.host);
        vec![
            "-c".to_string(),
            format!("{key}="),
            "-c".to_string(),
            format!(
                "{key}=!f() {{ test \"$1\" = get && cat {}; }}; f",
                shell_quote(&self.file)
            ),
        ]
    }

    /// Where the token is, for tests.
    pub fn path(&self) -> &Path {
        &self.file
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file);
        let _ = fs::remove_dir(&self.dir);
    }
}

fn random_hex() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// A path in single quotes for `sh`, with any `'` closed and reopened.
pub(crate) fn shell_quote(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let text = String::from_utf8_lossy(path.as_os_str().as_bytes()).replace('\'', r"'\''");
    format!("'{text}'")
}
