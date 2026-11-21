//! The session's dconf profile, and the value of `DCONF_PROFILE` that means
//! "dconf's default".
//!
//! dconf picks a profile in this order (`engine/dconf-engine-profile.c`,
//! `dconf_engine_profile_open`): a mandatory profile for the uid, then
//! `DCONF_PROFILE` (a name looked up in `/etc/dconf/profile/` and then
//! `dconf/profile/` under each `XDG_DATA_DIRS` entry, or an absolute path),
//! then `$XDG_RUNTIME_DIR/dconf.profile`, then the profile named `user` by
//! the same lookup, and with none of these the single line `user-db:user`.
//! A named profile that is missing gives the null configuration, which
//! writes nothing, so `DCONF_PROFILE=user` is not the same as unset on a
//! machine with no `user` profile file (Ubuntu 24.04), and an empty value
//! names a profile file that is a directory.
//!
//! The session's own profile is `user-db:betteros` over what the machine's
//! `user` profile says. When that is only `user-db:user` (dconf's default,
//! and what Ubuntu 24.04 and Zorin OS 18.1 ship: ticket 203's notes) the
//! shipped `/usr/share/dconf/profile/betteros` is exactly that. When an
//! administrator's `user` profile adds system databases (defaults and
//! locks), the launcher writes a profile with those lines under the
//! session's runtime directory and points `DCONF_PROFILE` at it, so they
//! apply in the session as they do in GNOME.

use std::path::{Path, PathBuf};

/// The shipped session profile's name.
pub const SESSION_PROFILE: &str = crate::environment::DCONF_PROFILE;

/// A shipped profile that is dconf's default fallback, `user-db:user`.
pub const DEFAULT_USER_PROFILE: &str = "/usr/share/dconf/profile/betteros-default";

/// The file name of a profile the launcher writes for the session.
pub const GENERATED_PROFILE: &str = "dconf-profile";

/// Where dconf looks for profiles and runtime files, so tests can point at
/// a scratch tree.
#[derive(Clone, Debug)]
pub struct Probe {
    /// `/etc` on a machine.
    pub sysconf: PathBuf,
    /// The `XDG_DATA_DIRS` entries, in order.
    pub data_dirs: Vec<PathBuf>,
    /// `XDG_RUNTIME_DIR`.
    pub runtime_dir: Option<PathBuf>,
}

impl Probe {
    /// The machine's directories, with `data_dirs` from an `XDG_DATA_DIRS`
    /// value (the specification's default when it is empty).
    pub fn system(data_dirs: Option<&str>, runtime_dir: Option<&str>) -> Probe {
        let data_dirs = data_dirs
            .filter(|value| !value.is_empty())
            .unwrap_or(crate::environment::DEFAULT_DATA_DIRS);
        Probe {
            sysconf: PathBuf::from("/etc"),
            data_dirs: data_dirs
                .split(':')
                .filter(|entry| !entry.is_empty())
                .map(PathBuf::from)
                .collect(),
            runtime_dir: runtime_dir.map(PathBuf::from),
        }
    }

    /// The first `dconf/profile/<name>` by dconf's lookup.
    pub fn named(&self, name: &str) -> Option<PathBuf> {
        std::iter::once(&self.sysconf)
            .chain(self.data_dirs.iter())
            .map(|dir| dir.join("dconf/profile").join(name))
            .find(|path| path.exists())
    }
}

/// The value that makes a process with `DCONF_PROFILE` set behave as one
/// without it: the runtime profile, else the machine's `user` profile, each
/// by its absolute path, else [`DEFAULT_USER_PROFILE`].
pub fn default_profile(probe: &Probe) -> String {
    if let Some(runtime) = probe
        .runtime_dir
        .as_ref()
        .map(|dir| dir.join("dconf.profile"))
        && runtime.exists()
    {
        return runtime.display().to_string();
    }
    match probe.named("user") {
        Some(path) => path.display().to_string(),
        None => DEFAULT_USER_PROFILE.to_string(),
    }
}

/// The sources of a profile file, comments and blank lines left out.
pub fn sources(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// What the session's profile is, given the machine's `user` profile text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionProfile {
    /// The shipped `betteros` profile is exactly right.
    Shipped,
    /// A profile the launcher writes: this text.
    Generated(String),
}

pub fn session_profile(user_profile: Option<&str>) -> SessionProfile {
    let system = user_profile.map(sources).unwrap_or_default();
    if system.is_empty() || system == ["user-db:user"] {
        return SessionProfile::Shipped;
    }
    let mut text = String::from(
        "# Written by the Better OS session launcher: the session's own database\n\
         # over this machine's user profile, so its defaults and locks apply here.\n\
         user-db:betteros\n",
    );
    for source in system {
        if source != "user-db:betteros" {
            text.push_str(&source);
            text.push('\n');
        }
    }
    SessionProfile::Generated(text)
}

/// Whether `value` of `DCONF_PROFILE` is one the session sets.
pub fn is_session_profile(value: &str) -> bool {
    value == SESSION_PROFILE
        || Path::new(value).ends_with(Path::new("better-os/session").join(GENERATED_PROFILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(root: &Path) -> Probe {
        Probe {
            sysconf: root.join("etc"),
            data_dirs: vec![root.join("usr/local/share"), root.join("usr/share")],
            runtime_dir: Some(root.join("run")),
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn with_no_user_profile_the_default_is_the_shipped_fallback() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(default_profile(&probe(root.path())), DEFAULT_USER_PROFILE);
    }

    #[test]
    fn the_machines_user_profile_and_the_runtime_profile_win_in_dconfs_order() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("usr/share/dconf/profile/user"),
            "user-db:user\n",
        );
        assert_eq!(
            default_profile(&probe(root.path())),
            root.path()
                .join("usr/share/dconf/profile/user")
                .display()
                .to_string()
        );
        write(
            &root.path().join("etc/dconf/profile/user"),
            "user-db:user\nsystem-db:local\n",
        );
        assert_eq!(
            default_profile(&probe(root.path())),
            root.path()
                .join("etc/dconf/profile/user")
                .display()
                .to_string()
        );
        write(&root.path().join("run/dconf.profile"), "user-db:user\n");
        assert_eq!(
            default_profile(&probe(root.path())),
            root.path().join("run/dconf.profile").display().to_string()
        );
    }

    #[test]
    fn a_plain_user_profile_keeps_the_shipped_session_profile() {
        assert_eq!(session_profile(None), SessionProfile::Shipped);
        assert_eq!(
            session_profile(Some("# comment\nuser-db:user\n\n")),
            SessionProfile::Shipped
        );
    }

    #[test]
    fn an_administrators_system_databases_reach_the_session() {
        let SessionProfile::Generated(text) = session_profile(Some(
            "user-db:user\nsystem-db:local # site\nsystem-db:site\n",
        )) else {
            panic!("a profile with system databases is generated");
        };
        assert_eq!(
            sources(&text),
            vec![
                "user-db:betteros",
                "user-db:user",
                "system-db:local",
                "system-db:site"
            ]
        );
    }

    #[test]
    fn the_shipped_profile_files_say_what_this_module_says() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/session");
        let session = std::fs::read_to_string(repository.join("betteros.dconf-profile")).unwrap();
        assert_eq!(sources(&session), vec!["user-db:betteros", "user-db:user"]);
        let default =
            std::fs::read_to_string(repository.join("betteros-default.dconf-profile")).unwrap();
        assert_eq!(sources(&default), vec!["user-db:user"]);
    }

    #[test]
    fn the_session_profile_values_are_recognized() {
        assert!(is_session_profile("betteros"));
        assert!(is_session_profile(
            "/run/user/1000/better-os/session/dconf-profile"
        ));
        assert!(!is_session_profile("user"));
        assert!(!is_session_profile("/etc/dconf/profile/user"));
    }
}
