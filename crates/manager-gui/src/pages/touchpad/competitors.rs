//! Other gesture tools that would compete with Better Touchpad for the same
//! fingers (ticket 88, found on the owner's machine): the X11 Gestures GNOME
//! Shell extension, Touchégg, and fusuma.
//!
//! Detection is unprivileged and read-only: the shell's own extension list
//! over the session bus, the process table under `/proc` (only names and
//! command lines), and the XDG autostart directories. Turning one off is a
//! change to the person's own session and nothing else:
//!
//! - the extension is disabled through GNOME Shell's `DisableExtension`, the
//!   same route the Window attention page uses to turn its own extension on;
//! - Touchégg's and fusuma's autostart entries are hidden for this account
//!   with a `Hidden=true` copy in `~/.config/autostart` (the XDG rule for
//!   "this user does not want this entry"), marked as Better OS's so turning
//!   it back on removes exactly that file and never a file the person wrote.
//!
//! Nothing is killed: a running Touchégg client or fusuma stops at the next
//! sign-in, and the page says so. Touchégg's system daemon is a system unit,
//! which a window never touches; without its client it delivers nothing.

use std::path::{Path, PathBuf};

/// The X11 Gestures extension's UUID.
pub(crate) const X11_GESTURES_UUID: &str = "x11gestures@joseexposito.github.io";

/// What marks an autostart override as written by Better OS.
pub(crate) const OVERRIDE_MARK: &str = "X-BetterOS-HiddenBy=better-touchpad";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Tool {
    X11Gestures,
    Touchegg,
    Fusuma,
}

impl Tool {
    pub(crate) const ALL: [Self; 3] = [Self::X11Gestures, Self::Touchegg, Self::Fusuma];

    /// The product's own name, which is not translated.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::X11Gestures => "X11 Gestures",
            Self::Touchegg => "Touchégg",
            Self::Fusuma => "fusuma",
        }
    }

    /// The autostart entry's file name, for the tools that start that way.
    pub(crate) fn autostart_file(self) -> Option<&'static str> {
        match self {
            Self::X11Gestures => None,
            Self::Touchegg => Some("touchegg.desktop"),
            Self::Fusuma => Some("fusuma.desktop"),
        }
    }

    /// Whether a process is this tool: its name, or for fusuma (a Ruby
    /// program) its command line.
    fn matches_process(self, comm: &str, cmdline: &str) -> bool {
        match self {
            Self::X11Gestures => false,
            // `touchegg` is the client; `touchegg --daemon` is the system
            // daemon, which runs as root and does nothing without a client.
            Self::Touchegg => comm == "touchegg" && !cmdline.contains("--daemon"),
            Self::Fusuma => {
                comm == "fusuma"
                    || cmdline
                        .split('\0')
                        .any(|argument| argument.rsplit('/').next() == Some("fusuma"))
            }
        }
    }
}

/// One tool that was found, and how it can be turned off.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Found {
    pub(crate) tool: Tool,
    /// A process of it runs for this account now.
    pub(crate) running: bool,
    /// The autostart entry that starts it, when there is one and it is not
    /// already hidden for this account.
    pub(crate) autostart: Option<PathBuf>,
    /// For the extension: it is enabled.
    pub(crate) extension_enabled: bool,
    /// Better OS already hid its autostart entry; turning it on again
    /// removes that override.
    pub(crate) hidden_by_us: bool,
}

impl Found {
    /// Whether it competes right now or will at the next sign-in.
    pub(crate) fn competes(&self) -> bool {
        self.extension_enabled || (!self.hidden_by_us && (self.running || self.autostart.is_some()))
    }
}

/// Where detection looks. Tests point it at a scratch tree.
#[derive(Clone, Debug)]
pub(crate) struct Roots {
    pub(crate) proc: PathBuf,
    /// This account's autostart directory (`$XDG_CONFIG_HOME/autostart`).
    pub(crate) user_autostart: PathBuf,
    /// The system's (`$XDG_CONFIG_DIRS`/autostart, `/etc/xdg/autostart`).
    pub(crate) system_autostart: Vec<PathBuf>,
    /// This account's uid: only its own processes count.
    pub(crate) uid: u32,
}

impl Roots {
    pub(crate) fn for_session() -> Self {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_default();
        let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_string());
        Self {
            proc: PathBuf::from("/proc"),
            user_autostart: config.join("autostart"),
            system_autostart: dirs
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(|dir| PathBuf::from(dir).join("autostart"))
                .collect(),
            // This process's own uid, read from its own /proc entry: no
            // unsafe call needed for it.
            // A /proc that cannot be read matches no process.
            uid: std::fs::metadata("/proc/self")
                .map(|meta| std::os::unix::fs::MetadataExt::uid(&meta))
                .unwrap_or(u32::MAX),
        }
    }
}

/// The processes of `uid`, as (name, NUL-separated command line).
fn processes(proc: &Path, uid: u32) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(proc) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .filter_map(|entry| {
            let path = entry.path();
            let status = std::fs::read_to_string(path.join("status")).ok()?;
            let owner = status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|uids| uids.split_whitespace().next())
                .and_then(|uid| uid.parse::<u32>().ok())?;
            if owner != uid {
                return None;
            }
            let comm = std::fs::read_to_string(path.join("comm")).ok()?;
            let cmdline = std::fs::read_to_string(path.join("cmdline")).unwrap_or_default();
            Some((comm.trim().to_string(), cmdline))
        })
        .collect()
}

/// Whether an autostart file asks not to be started (`Hidden=true`).
fn hidden(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim().eq_ignore_ascii_case("hidden=true"))
}

/// What the tools look like on this machine. `extensions` is the shell's
/// answer for the X11 Gestures extension: `Some(enabled)` when it is
/// installed, `None` when it is not (or there is no shell to ask).
pub(crate) fn detect(roots: &Roots, x11_gestures: Option<bool>) -> Vec<Found> {
    let processes = processes(&roots.proc, roots.uid);
    let mut found = Vec::new();
    for tool in Tool::ALL {
        if tool == Tool::X11Gestures {
            if let Some(enabled) = x11_gestures {
                found.push(Found {
                    tool,
                    running: false,
                    autostart: None,
                    extension_enabled: enabled,
                    hidden_by_us: false,
                });
            }
            continue;
        }
        let running = processes
            .iter()
            .any(|(comm, cmdline)| tool.matches_process(comm, cmdline));
        let file = tool.autostart_file().unwrap_or_default();
        let user = roots.user_autostart.join(file);
        let user_text = std::fs::read_to_string(&user).ok();
        let hidden_by_us = user_text
            .as_deref()
            .is_some_and(|text| text.contains(OVERRIDE_MARK));
        // The user's own file decides when there is one (it shadows the
        // system's); a hidden one starts nothing.
        let autostart = match &user_text {
            Some(text) if hidden(text) => None,
            Some(_) => Some(user.clone()),
            None => roots
                .system_autostart
                .iter()
                .map(|dir| dir.join(file))
                .find(|path| std::fs::read_to_string(path).is_ok_and(|text| !hidden(&text))),
        };
        if running || autostart.is_some() || hidden_by_us {
            found.push(Found {
                tool,
                running,
                autostart,
                extension_enabled: false,
                hidden_by_us,
            });
        }
    }
    found
}

/// Why an autostart entry could not be hidden or shown again.
#[derive(Debug, thiserror::Error)]
pub(crate) enum OverrideError {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    /// The person's own file is there; Better OS does not overwrite it.
    #[error("{0} is the person's own file and is left alone")]
    NotOurs(PathBuf),
}

/// Hides `tool`'s autostart entry for this account: a `Hidden=true` file of
/// the same name in the user's autostart directory, marked as Better OS's.
/// When the person already has their own file there, it gains `Hidden=true`
/// only if it is ours; theirs is refused rather than rewritten.
pub(crate) fn hide_autostart(roots: &Roots, tool: Tool) -> Result<PathBuf, OverrideError> {
    let file = tool.autostart_file().unwrap_or("unknown.desktop");
    let path = roots.user_autostart.join(file);
    if let Ok(text) = std::fs::read_to_string(&path)
        && !text.contains(OVERRIDE_MARK)
    {
        return Err(OverrideError::NotOurs(path));
    }
    std::fs::create_dir_all(&roots.user_autostart)
        .map_err(|error| OverrideError::Io(roots.user_autostart.clone(), error))?;
    let body = format!(
        "[Desktop Entry]\nType=Application\nName={}\nHidden=true\n{OVERRIDE_MARK}\n",
        tool.name()
    );
    let temporary = roots
        .user_autostart
        .join(format!("{file}{TEMPORARY_SUFFIX}"));
    std::fs::write(&temporary, body)
        .map_err(|error| OverrideError::Io(temporary.clone(), error))?;
    std::fs::rename(&temporary, &path).map_err(|error| OverrideError::Io(path.clone(), error))?;
    Ok(path)
}

/// The temporary name `hide_autostart` writes through; the package's postrm
/// cleans the same name.
pub(crate) const TEMPORARY_SUFFIX: &str = ".tmp-better-os";

/// Removes a temporary copy a crash in the middle of `hide_autostart` left
/// behind: a regular file (never a link) carrying the marker. Run before the
/// page looks at the tools, so a leftover never outlives the next visit.
pub(crate) fn forget_stale_temporaries(roots: &Roots) {
    for file in Tool::ALL.iter().filter_map(|tool| tool.autostart_file()) {
        let path = roots
            .user_autostart
            .join(format!("{file}{TEMPORARY_SUFFIX}"));
        let regular = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_file());
        if regular && std::fs::read_to_string(&path).is_ok_and(|text| text.contains(OVERRIDE_MARK))
        {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Removes the override `hide_autostart` wrote, and only that.
pub(crate) fn show_autostart(roots: &Roots, tool: Tool) -> Result<(), OverrideError> {
    let file = tool.autostart_file().unwrap_or("unknown.desktop");
    let path = roots.user_autostart.join(file);
    match std::fs::read_to_string(&path) {
        Ok(text) if text.contains(OVERRIDE_MARK) => {
            std::fs::remove_file(&path).map_err(|error| OverrideError::Io(path, error))
        }
        Ok(_) => Err(OverrideError::NotOurs(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(OverrideError::Io(path, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crashed_write_s_temporary_copy_is_cleaned_and_nothing_else() {
        let tree = tree();
        let dir = &tree.roots.user_autostart;
        std::fs::create_dir_all(dir).unwrap();
        let ours = dir.join(format!("touchegg.desktop{TEMPORARY_SUFFIX}"));
        std::fs::write(
            &ours,
            format!("[Desktop Entry]\nHidden=true\n{OVERRIDE_MARK}\n"),
        )
        .unwrap();
        let theirs = dir.join(format!("fusuma.desktop{TEMPORARY_SUFFIX}"));
        std::fs::write(&theirs, "[Desktop Entry]\nExec=fusuma\n").unwrap();

        forget_stale_temporaries(&tree.roots);

        assert!(!ours.exists());
        assert!(theirs.exists(), "a file without the marker is not ours");
    }

    /// The postrm cleans the same temporary name the page writes through.
    #[test]
    fn the_removal_script_cleans_the_temporary_name_this_writes() {
        let script = include_str!("../../../../../packaging/maintainer/autostart-overrides.sh");
        assert!(script.contains(&format!("$file{TEMPORARY_SUFFIX}")));
    }

    struct Tree {
        _guard: tempfile::TempDir,
        roots: Roots,
    }

    fn tree() -> Tree {
        let guard = tempfile::tempdir().unwrap();
        let base = guard.path();
        let roots = Roots {
            proc: base.join("proc"),
            user_autostart: base.join("config/autostart"),
            system_autostart: vec![base.join("etc/xdg/autostart")],
            uid: 1000,
        };
        std::fs::create_dir_all(&roots.proc).unwrap();
        Tree {
            _guard: guard,
            roots,
        }
    }

    fn process(roots: &Roots, pid: u32, uid: u32, comm: &str, cmdline: &[&str]) {
        let dir = roots.proc.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("status"),
            format!("Name:\t{comm}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
        )
        .unwrap();
        std::fs::write(dir.join("comm"), format!("{comm}\n")).unwrap();
        std::fs::write(dir.join("cmdline"), cmdline.join("\0")).unwrap();
    }

    fn system_entry(roots: &Roots, file: &str) {
        let dir = &roots.system_autostart[0];
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(file),
            "[Desktop Entry]\nType=Application\nExec=x\n",
        )
        .unwrap();
    }

    /// The package's postrm deletes only files carrying this exact marker,
    /// so the window and the packaging must name the same line.
    #[test]
    fn the_removal_script_looks_for_the_marker_this_writes() {
        let build = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/build-deb.sh"
        ))
        .unwrap();
        assert!(build.contains(&format!("AUTOSTART_OVERRIDE_MARK='{OVERRIDE_MARK}'")));
        for tool in Tool::ALL {
            if let Some(file) = tool.autostart_file() {
                assert!(
                    build.contains("PACKAGE_AUTOSTART_OVERRIDES='touchegg.desktop fusuma.desktop'")
                        && build.contains(file),
                    "{file}"
                );
            }
        }
    }

    #[test]
    fn nothing_installed_is_nothing_found() {
        let t = tree();
        assert!(detect(&t.roots, None).is_empty());
    }

    #[test]
    fn the_extension_is_reported_as_the_shell_says() {
        let t = tree();
        let found = detect(&t.roots, Some(true));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tool, Tool::X11Gestures);
        assert!(found[0].competes());
        assert!(!detect(&t.roots, Some(false))[0].competes());
    }

    #[test]
    fn a_running_client_and_its_autostart_entry_are_found_and_the_daemon_is_not() {
        let t = tree();
        process(
            &t.roots,
            10,
            0,
            "touchegg",
            &["/usr/bin/touchegg", "--daemon"],
        );
        process(&t.roots, 11, 1000, "touchegg", &["touchegg", "--daemon"]);
        assert!(
            detect(&t.roots, None).is_empty(),
            "a daemon is not the client"
        );
        process(&t.roots, 12, 1000, "touchegg", &["touchegg"]);
        system_entry(&t.roots, "touchegg.desktop");
        let found = detect(&t.roots, None);
        assert_eq!(found.len(), 1);
        assert!(found[0].running);
        assert_eq!(
            found[0].autostart.as_deref(),
            Some(
                t.roots.system_autostart[0]
                    .join("touchegg.desktop")
                    .as_path()
            )
        );
    }

    #[test]
    fn fusuma_is_found_by_its_command_line() {
        let t = tree();
        process(
            &t.roots,
            20,
            1000,
            "ruby",
            &["ruby", "/home/a/.gem/bin/fusuma", "-d"],
        );
        // Someone else's fusuma is not this account's concern.
        process(&t.roots, 21, 1001, "fusuma", &["fusuma"]);
        let found = detect(&t.roots, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tool, Tool::Fusuma);
        assert!(found[0].running && found[0].autostart.is_none());
    }

    #[test]
    fn hiding_an_entry_writes_our_override_and_showing_removes_only_ours() {
        let t = tree();
        system_entry(&t.roots, "touchegg.desktop");
        let path = hide_autostart(&t.roots, Tool::Touchegg).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("Hidden=true") && text.contains(OVERRIDE_MARK));
        let found = detect(&t.roots, None);
        assert!(found[0].hidden_by_us && !found[0].competes());
        assert!(found[0].autostart.is_none());
        // Hiding twice is fine.
        hide_autostart(&t.roots, Tool::Touchegg).unwrap();
        show_autostart(&t.roots, Tool::Touchegg).unwrap();
        assert!(!path.exists());
        assert!(detect(&t.roots, None)[0].competes());
        // Showing what is not hidden is fine.
        show_autostart(&t.roots, Tool::Touchegg).unwrap();
    }

    #[test]
    fn the_person_s_own_autostart_file_is_never_rewritten_or_removed() {
        let t = tree();
        std::fs::create_dir_all(&t.roots.user_autostart).unwrap();
        let mine = t.roots.user_autostart.join("fusuma.desktop");
        std::fs::write(&mine, "[Desktop Entry]\nExec=fusuma -d\n").unwrap();
        assert!(matches!(
            hide_autostart(&t.roots, Tool::Fusuma),
            Err(OverrideError::NotOurs(_))
        ));
        assert!(matches!(
            show_autostart(&t.roots, Tool::Fusuma),
            Err(OverrideError::NotOurs(_))
        ));
        assert_eq!(
            std::fs::read_to_string(&mine).unwrap(),
            "[Desktop Entry]\nExec=fusuma -d\n"
        );
        // Their own Hidden=true is respected as "starts nothing".
        std::fs::write(&mine, "[Desktop Entry]\nHidden=true\n").unwrap();
        assert!(detect(&t.roots, None).is_empty());
    }
}
