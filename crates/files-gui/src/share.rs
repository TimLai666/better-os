//! Sharing and the tailnet, each only when its tool is installed: folio's
//! Tailscale and Dropbox plugins (`plugins/tailscale`, `plugins/dropbox`).
//!
//! - **Tailnet machines** from `tailscale status --json`, listed in the
//!   sidebar and opened over SFTP through GVfs (`sftp://<machine>/`).
//! - **Taildrop**: `tailscale file cp <files…> <machine>:` sends files to a
//!   machine on the tailnet.
//! - **Dropbox**: the Dropbox folder in the sidebar with its sync state from
//!   `dropbox filestatus`, and "Copy Dropbox link" from `dropbox sharelink`.
//!
//! Every program is started with its arguments as a list, never through a
//! shell, and stopped after a timeout. Nothing here runs at startup unless
//! the program is on `$PATH`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// A machine on the tailnet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Machine {
    pub name: String,
    /// The MagicDNS name, without the trailing dot, which SFTP and Taildrop
    /// are addressed by.
    pub address: String,
    pub online: bool,
    pub os: String,
}

#[derive(Deserialize)]
struct Status {
    #[serde(rename = "BackendState", default)]
    backend_state: String,
    #[serde(rename = "Peer", default)]
    peer: Option<std::collections::BTreeMap<String, Peer>>,
}

#[derive(Deserialize)]
struct Peer {
    #[serde(rename = "HostName", default)]
    host_name: String,
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "Online", default)]
    online: bool,
    #[serde(rename = "OS", default)]
    os: String,
}

/// The machines in `tailscale status --json` output, online first, then by
/// name. Empty when Tailscale is not running.
pub fn parse_status(json: &str) -> Vec<Machine> {
    let Ok(status) = serde_json::from_str::<Status>(json) else {
        return Vec::new();
    };
    if status.backend_state != "Running" {
        return Vec::new();
    }
    let mut machines: Vec<Machine> = status
        .peer
        .unwrap_or_default()
        .into_values()
        .filter(|peer| !peer.dns_name.is_empty())
        .map(|peer| Machine {
            name: if peer.host_name.is_empty() {
                peer.dns_name
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            } else {
                peer.host_name
            },
            address: peer.dns_name.trim_end_matches('.').to_string(),
            online: peer.online,
            os: peer.os,
        })
        .collect();
    machines.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    machines
}

/// The tailnet's machines, when Tailscale is installed and running.
pub fn tailnet_machines() -> Vec<Machine> {
    if !crate::terminal::on_path("tailscale") {
        return Vec::new();
    }
    files_preview::viewer::run(
        "tailscale",
        &["status".into(), "--json".into()],
        Duration::from_secs(5),
    )
    .map(|output| parse_status(&String::from_utf8_lossy(&output)))
    .unwrap_or_default()
}

/// The address a tailnet machine's files open at.
pub fn sftp_address(machine: &Machine) -> String {
    format!("sftp://{}/", machine.address)
}

/// The words that send `files` to `machine` with Taildrop.
pub fn taildrop_command(files: &[PathBuf], machine: &Machine) -> Vec<String> {
    let mut words = vec!["file".to_string(), "cp".to_string()];
    words.extend(files.iter().map(|file| file.display().to_string()));
    words.push(format!("{}:", machine.address));
    words
}

/// Sends files with Taildrop. Runs to completion (up to ten minutes for a
/// large file); call it from a worker thread.
pub fn taildrop(files: &[PathBuf], machine: &Machine) -> bool {
    files_preview::viewer::run(
        "tailscale",
        &taildrop_command(files, machine),
        Duration::from_secs(600),
    )
    .is_some()
}

/// The Dropbox folder: `~/Dropbox`, when it is there and the Dropbox command
/// is installed.
pub fn dropbox_folder(home: &Path) -> Option<PathBuf> {
    let folder = home.join("Dropbox");
    (folder.is_dir() && crate::terminal::on_path("dropbox")).then_some(folder)
}

/// Dropbox's word for a path's sync state, from `dropbox filestatus`
/// (`/home/a/Dropbox: up to date`).
pub fn parse_filestatus(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .and_then(|line| line.rsplit_once(": "))
        .map(|(_, state)| state.trim().to_string())
        .filter(|state| !state.is_empty())
}

pub fn dropbox_status(folder: &Path) -> Option<String> {
    files_preview::viewer::run(
        "dropbox",
        &["filestatus".into(), folder.display().to_string()],
        Duration::from_secs(5),
    )
    .and_then(|output| parse_filestatus(&String::from_utf8_lossy(&output)))
}

/// The link `dropbox sharelink` prints, when it is one.
pub fn parse_sharelink(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("https://"))
        .map(str::to_string)
}

/// A share link for a file inside the Dropbox folder.
pub fn dropbox_link(path: &Path) -> Option<String> {
    files_preview::viewer::run(
        "dropbox",
        &["sharelink".into(), path.display().to_string()],
        Duration::from_secs(30),
    )
    .and_then(|output| parse_sharelink(&String::from_utf8_lossy(&output)))
}
