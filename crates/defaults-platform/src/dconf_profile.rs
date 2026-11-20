//! Which dconf database the running session reads, decided in one place.
//!
//! GSettings does not read one fixed file. dconf reads a *profile*, a list of
//! databases, and writes to the first of them. GNOME's default profile is
//! `user-db:user`, so GNOME's settings are `~/.config/dconf/user`, written
//! through `/ca/desrt/dconf/Writer/user`. A COSMIC session's `start-cosmic`
//! sets `DCONF_PROFILE` to the profile file's absolute path,
//! `/usr/share/dconf/profile/cosmic` (`user-db:cosmic` then `user-db:user`):
//! the script says `DCONF_PROFILE=cosmic` and `just install` rewrites it to
//! the installed path (`pop-os/cosmic-session` `Justfile`, lines 5 and 39,
//! read 2026-11-21). The Better OS session sets `DCONF_PROFILE=betteros` to
//! the same effect (ADR 0038, `engineering/specs/better-os-session.md`). A
//! key written to GNOME's database from inside such a session is hidden by
//! any value the session's own database holds, and reading GNOME's database
//! reports a value the session is not showing. Every Better OS reader and
//! writer of dconf — the defaults engine's GNOME adapters and Better
//! Touchpad's backend — asks [`session_database`] instead of naming a file.
//!
//! ## dconf's rules, as followed here
//!
//! From dconf's own documentation, `dconf(7)`, section PROFILES (dconf 0.40,
//! the version Ubuntu 24.04 ships, read 2026-11-21), and its source,
//! `engine/dconf-engine-profile.c`, `dconf-engine-source.c`,
//! `dconf-engine-source-user.c`, and `dconf-engine-source-service.c`
//! (GNOME/dconf tag `0.40.0` and `main`, read 2026-11-21):
//!
//! 1. A *mandatory* profile at `/run/dconf/user/<uid>` wins over everything,
//!    `DCONF_PROFILE` included. Failing to open it is silent.
//! 2. Otherwise `DCONF_PROFILE` names the profile. An absolute path is opened
//!    as it is; a name is looked up as `/etc/dconf/profile/<name>` and then as
//!    `<dir>/dconf/profile/<name>` for each `XDG_DATA_DIRS` entry, stopping at
//!    the first file that opens or the first error other than "not found". A
//!    named profile that cannot be opened gives the *null configuration*: no
//!    database at all, nothing readable and nothing writable.
//! 3. With `DCONF_PROFILE` unset: the runtime profile
//!    `$XDG_RUNTIME_DIR/dconf/profile` (silent on failure), then the profile
//!    named `user` by the same lookup, then the built-in `user-db:user`. A
//!    `user` profile that exists and cannot be opened is warned about ("Unable
//!    to open %s") and the built-in profile is used.
//! 4. A profile line has leading whitespace skipped, everything from `#` on
//!    dropped, and trailing whitespace trimmed; an empty line is skipped. A
//!    line is `user-db:`, `service-db:`, `system-db:`, or `file-db:` followed
//!    by a non-empty name; dconf warns about any other line and skips it.
//! 5. "The first line indicates the database used to write changes" and
//!    should be a `user-db` or `service-db`. A `user-db:<name>` is the file
//!    `$XDG_CONFIG_HOME/dconf/<name>`, written through the object
//!    `/ca/desrt/dconf/Writer/<name>` of `ca.desrt.dconf`. A
//!    `service-db:<name>` is the file `$XDG_RUNTIME_DIR/dconf-service/<name>`,
//!    written through `/ca/desrt/dconf/<name>`, where the first element of
//!    the name picks the service's writer (`keyfile`, `safe`).
//! 6. NFS. dconf 51.0 and later (GNOME/dconf `0d7ead6`, "engine: Use safe
//!    writer for NFS user databases", 2026-07-16, first in tag `51.0`)
//!    replace a first `user-db:<name>` with `service-db:safe/<name>` when the
//!    user configuration directory is on NFS. The service's safe writer
//!    (`service/dconf-safe-writer.c`) keeps `$XDG_CONFIG_HOME/dconf/<name>`
//!    as the lasting copy, under a lock file beside it, and gives readers the
//!    runtime copy. dconf 0.40 (Ubuntu 24.04) and 0.49 have no such rule: on
//!    NFS they read and write `user-db:<name>` as anywhere else, which is what
//!    Better Manager 0.2.16 did. So on NFS the rule follows the installed
//!    `libdconf1` (read with `dpkg-query`); under 51.0 or later 0.2.16's
//!    native write still reached the lasting file and the safe writer picked
//!    it up from its file monitor, without the lock, and this module writes
//!    through the safe writer dconf itself uses instead.
//!
//! ## Where this stops short, and says so
//!
//! - A profile whose first line is a `system-db` or a `file-db` is read-only
//!   and is reported, never written around.
//! - A named profile that exists and cannot be read is reported (dconf runs
//!   such a session with the null configuration). An unreadable `user`
//!   profile is what dconf works around with `user-db:user` (rule 3); this
//!   module does the same and keeps a note ([`SessionDatabase::notes`]).
//! - The later `user-db` lines are the databases beneath the session's own,
//!   in order, for a reader that needs the value the session shows when its
//!   own database holds nothing. `system-db` lines and their locks are not
//!   read, as they were not before this module existed.
//! - A process whose environment says COSMIC or Better OS and has no
//!   `DCONF_PROFILE` was not started with the session's environment:
//!   `start-cosmic` has exported it since `87a0644` (2024-12-04) and imported
//!   it into the user manager since `6df4561` (2025-02-27), every COSMIC
//!   Epoch release, Pop!_OS `noble`'s `cosmic-session 1.11.0` included
//!   (ADR 0038), and the Better OS session sets it. Its own profile would be
//!   GNOME's, so it is reported rather than used.

use std::fmt;
use std::path::{Path, PathBuf};

use better_core::{CurrentDesktop, DconfDatabase};

/// Where a mandatory profile lives, per user id.
pub const MANDATORY_PROFILE_DIR: &str = "/run/dconf/user";
/// The system configuration directory dconf was built with on Ubuntu.
pub const SYSCONF_DIR: &str = "/etc";
/// The bus name of the dconf service.
pub const WRITER_SERVICE: &str = "ca.desrt.dconf";
/// The prefix of every user database's writer object.
pub const WRITER_PATH_PREFIX: &str = "/ca/desrt/dconf/Writer/";
/// The prefix of every service database's writer object.
pub const SERVICE_PATH_PREFIX: &str = "/ca/desrt/dconf/";
/// The service writers a `service-db` name may start with
/// (`service/dconf-service.c`, `dconf_service_dbus_register`).
const SERVICE_WRITERS: [&str; 2] = ["keyfile", "safe"];
/// The first dconf release whose engine sends a `user-db` on NFS through the
/// safe writer.
const NFS_SAFE_WRITER_SINCE: u32 = 51;
/// `NFS_SUPER_MAGIC` from `linux/magic.h`, the `f_type` both `nfs` and `nfs4`
/// mounts report.
const NFS_SUPER_MAGIC: i64 = 0x6969;

/// What dconf's profile lookup depends on. [`Self::from_env`] reads this
/// process's environment; a test builds one over scratch directories.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileEnvironment {
    /// `DCONF_PROFILE`, when it is set.
    pub dconf_profile: Option<String>,
    /// The session's desktop, from [`CurrentDesktop`].
    pub desktop: CurrentDesktop,
    /// The directory holding mandatory profiles, one file per user id.
    pub mandatory_dir: PathBuf,
    pub uid: u32,
    /// `$XDG_RUNTIME_DIR`, as GLib resolves it.
    pub runtime_dir: PathBuf,
    /// dconf's `SYSCONFDIR`.
    pub sysconf_dir: PathBuf,
    /// `XDG_DATA_DIRS`, as GLib resolves it.
    pub data_dirs: Vec<PathBuf>,
    /// `$XDG_CONFIG_HOME`, where every `user-db` file is.
    pub config_home: PathBuf,
    /// Whether `config_home` is on NFS.
    pub config_home_on_nfs: bool,
    /// Whether the installed dconf sends a `user-db` on NFS through the safe
    /// writer (rule 6); `None` when it could not be told. Read only when
    /// `config_home_on_nfs`.
    pub dconf_uses_safe_writer_on_nfs: Option<bool>,
}

impl ProfileEnvironment {
    /// This process's own environment and the system's paths.
    pub fn from_env() -> Self {
        // SAFETY: getuid has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        let mut environment = Self::from_lookup(|name| std::env::var(name).ok(), uid);
        environment.config_home_on_nfs = is_on_nfs(&environment.config_home);
        if environment.config_home_on_nfs {
            environment.dconf_uses_safe_writer_on_nfs =
                installed_dconf_major().map(|major| major >= NFS_SAFE_WRITER_SINCE);
        }
        environment
    }

    /// The environment read through `lookup`, with the system's own fixed
    /// paths and `config_home_on_nfs` false. A test replaces the paths.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>, uid: u32) -> Self {
        let set = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());
        let home = PathBuf::from(lookup("HOME").unwrap_or_default());
        let config_home = set("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        // `g_get_user_runtime_dir` falls back to the cache directory.
        let runtime_dir = set("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                set("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".cache"))
            });
        let data_dirs = set("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share/:/usr/share/".to_string())
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .collect();
        Self {
            dconf_profile: lookup("DCONF_PROFILE"),
            desktop: CurrentDesktop::from_lookup(&lookup),
            mandatory_dir: PathBuf::from(MANDATORY_PROFILE_DIR),
            uid,
            runtime_dir,
            sysconf_dir: PathBuf::from(SYSCONF_DIR),
            data_dirs,
            config_home,
            config_home_on_nfs: false,
            dconf_uses_safe_writer_on_nfs: None,
        }
    }

    /// How a first `user-db` is written here: `Some(true)` through the safe
    /// writer, `Some(false)` as itself, `None` when that cannot be told.
    fn nfs_rule(&self) -> Option<bool> {
        if self.config_home_on_nfs {
            self.dconf_uses_safe_writer_on_nfs
        } else {
            Some(false)
        }
    }
}

/// The major version of the installed `libdconf1`, from dpkg (a read-only
/// query). `None` when dpkg does not answer.
fn installed_dconf_major() -> Option<u32> {
    let output = std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Version}", "libdconf1"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    dconf_major(&String::from_utf8_lossy(&output.stdout))
}

/// `51.0-1` → 51, `0.40.0-4ubuntu0.1` → 0, `1:52.1-2` → 52.
fn dconf_major(version: &str) -> Option<u32> {
    let version = version.trim();
    let version = version.split_once(':').map_or(version, |(_, rest)| rest);
    version
        .split(['.', '-', '~', '+'])
        .next()
        .filter(|major| !major.is_empty())?
        .parse()
        .ok()
}

fn is_on_nfs(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut buffer = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `path` is a valid NUL-terminated string and `buffer` is large
    // enough for one `statfs`; it is read only when the call succeeded.
    let status = unsafe { libc::statfs(path.as_ptr(), buffer.as_mut_ptr()) };
    if status != 0 {
        // dconf's own check reads the type through GIO, which answers nothing
        // for a path it cannot stat, and then treats the directory as local.
        return false;
    }
    // SAFETY: statfs succeeded and filled the buffer.
    let info = unsafe { buffer.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    let kind = info.f_type as i64;
    kind == NFS_SUPER_MAGIC
}

/// One line of a profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileSource {
    User(String),
    Service(String),
    System(String),
    File(String),
}

impl fmt::Display for ProfileSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(name) => write!(formatter, "user-db:{name}"),
            Self::Service(name) => write!(formatter, "service-db:{name}"),
            Self::System(name) => write!(formatter, "system-db:{name}"),
            Self::File(name) => write!(formatter, "file-db:{name}"),
        }
    }
}

/// Where the profile in force came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileOrigin {
    /// A mandatory profile at `/run/dconf/user/<uid>`.
    Mandatory(PathBuf),
    /// The profile `DCONF_PROFILE` named, found at this path.
    Named { name: String, path: PathBuf },
    /// `$XDG_RUNTIME_DIR/dconf/profile`.
    Runtime(PathBuf),
    /// The profile named `user`, found at this path.
    User(PathBuf),
    /// No profile file: dconf's built-in `user-db:user`.
    BuiltIn,
    /// Not a profile: the database a defaults record names.
    Recorded,
}

/// A profile as dconf reads it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DconfProfile {
    pub origin: ProfileOrigin,
    pub sources: Vec<ProfileSource>,
    /// Lines dconf warns about and skips, kept so a status can name them.
    pub ignored: Vec<String>,
    /// Problems dconf warns about and works around, in the words of
    /// [`SessionDatabase::notes`].
    pub notes: Vec<String>,
}

/// Why the session's database could not be decided. Each has a stable
/// machine key, [`Self::reason`], with a sentence for the person in
/// [`sentence`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileError {
    /// `DCONF_PROFILE` names a profile no lookup path holds. dconf runs such
    /// a session with the null configuration.
    NamedProfileMissing {
        name: String,
        searched: Vec<PathBuf>,
    },
    /// The profile `DCONF_PROFILE` names exists and could not be read.
    Unreadable { path: PathBuf, detail: String },
    /// The profile's first line is read-only (`system-db`, `file-db`), or the
    /// profile lists nothing.
    NotWritable { first: Option<String> },
    /// A database name that has no writer object: a `user-db` name that is
    /// not one D-Bus object path element, or a `service-db` name that names
    /// no service writer.
    InvalidDatabaseName { name: String },
    /// The environment says COSMIC or Better OS and has no `DCONF_PROFILE`,
    /// which both sessions set: this process does not have the session's
    /// environment.
    NotTheSessionEnvironment { desktop: String },
    /// The home directory is on NFS and the installed dconf version, which
    /// decides how a database there is written (rule 6), could not be read.
    DconfVersionUnknown,
    /// A defaults record names a database in a form no dconf profile gives.
    RecordedDatabaseInvalid { name: String, writer_path: String },
}

impl ProfileError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::NamedProfileMissing { .. } => "dconf.profile_missing",
            Self::Unreadable { .. } => "dconf.profile_unreadable",
            Self::NotWritable { .. } => "dconf.profile_not_writable",
            Self::InvalidDatabaseName { .. } => "dconf.profile_invalid_database_name",
            Self::NotTheSessionEnvironment { .. } => "dconf.session_profile_not_in_environment",
            Self::DconfVersionUnknown => "dconf.profile_nfs_dconf_version_unknown",
            Self::RecordedDatabaseInvalid { .. } => "dconf.recorded_database_invalid",
        }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NamedProfileMissing { name, searched } => write!(
                formatter,
                "the dconf profile {name:?} that DCONF_PROFILE names is not at {}",
                searched
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Unreadable { path, detail } => write!(
                formatter,
                "the dconf profile {} could not be read: {detail}",
                path.display()
            ),
            Self::NotWritable { first: Some(first) } => write!(
                formatter,
                "the dconf profile's first database is {first}, which is read-only"
            ),
            Self::NotWritable { first: None } => {
                write!(formatter, "the dconf profile lists no database")
            }
            Self::InvalidDatabaseName { name } => {
                write!(
                    formatter,
                    "the dconf database {name:?} has no writer object"
                )
            }
            Self::NotTheSessionEnvironment { desktop } => write!(
                formatter,
                "this process runs in a {desktop} session without the session's DCONF_PROFILE"
            ),
            Self::DconfVersionUnknown => write!(
                formatter,
                "the home directory is on NFS and dpkg did not say which dconf is installed"
            ),
            Self::RecordedDatabaseInvalid { name, writer_path } => write!(
                formatter,
                "the record names the dconf database {name:?} at {writer_path:?}, which no profile gives"
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

/// The machine key of a reason string: everything before the first `:`,
/// which the adapters use to append a detail.
fn reason_key(reason: &str) -> &str {
    reason.split_once(':').map_or(reason, |(key, _)| key)
}

/// What a dconf refusal or note means to the person, and what they can do,
/// in English and Traditional Chinese (`(en, zh_TW)`). Every key this module
/// and the dconf adapters report has one; `reason` may carry a `:detail`.
pub fn sentence(reason: &str) -> Option<(&'static str, &'static str)> {
    Some(match reason_key(reason) {
        "dconf.profile_missing" => (
            "This session asks for a settings profile that isn't installed, so its settings can't be read or changed. Sign out and back in; if it stays, reinstall the session you sign in to.",
            "這個工作階段指定的設定檔沒有安裝，所以讀不到也改不了它的設定。請登出再登入；如果還是這樣，請重新安裝你登入的工作階段。",
        ),
        "dconf.profile_unreadable" => (
            "This session's settings profile can't be read, so Better OS doesn't know where the session keeps its settings. The details name the file; check that you can read it.",
            "讀不到這個工作階段的設定檔，Better OS 不知道它把設定存在哪裡。詳細資訊裡有檔案位置，請確認你有讀取權限。",
        ),
        "dconf.profile_not_writable" => (
            "This session's settings are locked by the administrator, so Better OS can't change them.",
            "這個工作階段的設定被管理員鎖定，Better OS 無法變更。",
        ),
        "dconf.profile_invalid_database_name" => (
            "This session keeps its settings somewhere Better OS can't write. Nothing was changed.",
            "這個工作階段把設定存在 Better OS 無法寫入的地方，所以沒有做任何變更。",
        ),
        "dconf.session_profile_not_in_environment" => (
            "Better OS was started without this session's settings, so a change would land where the session doesn't look. Open it again from the session's own app menu, or sign out and back in, and try again.",
            "Better OS 啟動時沒有拿到這個工作階段的設定位置，現在變更會改到工作階段看不到的地方。請從這個工作階段的應用程式選單重新開啟，或登出再登入後再試一次。",
        ),
        "dconf.profile_nfs_dconf_version_unknown" => (
            "Your home folder is on a network drive and the installed settings service couldn't be identified, so Better OS doesn't know how this session saves settings. Nothing was changed.",
            "你的家目錄在網路磁碟上，又認不出安裝的設定服務版本，Better OS 不知道這個工作階段怎麼儲存設定，所以沒有做任何變更。",
        ),
        "dconf.recorded_database_invalid" => (
            "The record of where this setting was changed is damaged, so it can't be put back automatically.",
            "記錄這項設定改在哪裡的資料已損壞，無法自動還原。",
        ),
        "dconf.database_beneath_unreadable" => (
            "One of the settings databases this session reads from can't be read, so this value is unknown.",
            "這個工作階段讀取的其中一個設定資料庫讀不到，所以不知道這個值。",
        ),
        "dconf.user_profile_unreadable" => (
            "The system's settings profile can't be read, so the desktop is using its built-in one, as Better OS does.",
            "系統的設定檔讀不到，桌面改用內建的設定檔，Better OS 也一樣。",
        ),
        _ => return None,
    })
}

/// [`sentence`] in the language `LANG` names, for a program with no window
/// (the before-removal step), by the rule every Better OS window uses: a
/// `zh_TW`, `zh-TW`, or `Hant` language is Traditional Chinese.
pub fn sentence_for_environment(reason: &str) -> Option<&'static str> {
    let language = std::env::var("LANG")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let zh = language.contains("zh_tw") || language.contains("zh-tw") || language.contains("hant");
    sentence(reason).map(|(en, zh_tw)| if zh { zh_tw } else { en })
}

/// The database the session reads first and dconf writes to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionDatabase {
    /// The name in its profile line: `user`, `betteros`, `safe/user`.
    pub name: String,
    /// The file read: `$XDG_CONFIG_HOME/dconf/<name>` for a `user-db`,
    /// `$XDG_RUNTIME_DIR/dconf-service/<name>` for a `service-db`.
    pub path: PathBuf,
    /// `/ca/desrt/dconf/Writer/<name>` or `/ca/desrt/dconf/<name>`.
    pub writer_path: String,
    /// The later `user-db` files, in profile order: what the session shows
    /// for a key its own database does not hold, before any schema default.
    pub beneath: Vec<PathBuf>,
    pub origin: ProfileOrigin,
    /// Problems dconf warns about and works around, as reason keys with a
    /// detail (`dconf.user_profile_unreadable:<path>: <error>`).
    pub notes: Vec<String>,
}

fn is_path_element(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_service_name(name: &str) -> bool {
    let mut elements = name.split('/');
    elements
        .next()
        .is_some_and(|writer| SERVICE_WRITERS.contains(&writer))
        && {
            let rest: Vec<&str> = elements.collect();
            !rest.is_empty() && rest.iter().all(|element| is_path_element(element))
        }
}

impl SessionDatabase {
    /// The database as a defaults record keeps it.
    pub fn id(&self) -> DconfDatabase {
        DconfDatabase {
            name: self.name.clone(),
            writer_path: self.writer_path.clone(),
        }
    }

    fn user_db(name: &str, config_home: &Path) -> Result<(PathBuf, String), ProfileError> {
        // A D-Bus object path element is `[A-Za-z0-9_]+`. dconf itself does
        // not check, and the call would fail at the bus.
        if !is_path_element(name) {
            return Err(ProfileError::InvalidDatabaseName {
                name: name.to_string(),
            });
        }
        Ok((
            config_home.join("dconf").join(name),
            format!("{WRITER_PATH_PREFIX}{name}"),
        ))
    }

    fn service_db(name: &str, runtime_dir: &Path) -> Result<(PathBuf, String), ProfileError> {
        if !is_service_name(name) {
            return Err(ProfileError::InvalidDatabaseName {
                name: name.to_string(),
            });
        }
        Ok((
            runtime_dir.join("dconf-service").join(name),
            format!("{SERVICE_PATH_PREFIX}{name}"),
        ))
    }

    /// The database a defaults record names, opened by the same rules: a
    /// `Writer/<name>` object is the `user-db` file, any other is the
    /// `service-db` file, which needs `runtime_dir`. Nothing is beneath it.
    pub fn recorded(
        recorded: &DconfDatabase,
        config_home: &Path,
        runtime_dir: Option<&Path>,
    ) -> Result<Self, ProfileError> {
        let invalid = || ProfileError::RecordedDatabaseInvalid {
            name: recorded.name.clone(),
            writer_path: recorded.writer_path.clone(),
        };
        let opened = if recorded.writer_path == format!("{WRITER_PATH_PREFIX}{}", recorded.name) {
            Self::user_db(&recorded.name, config_home)
        } else if let Some(runtime_dir) = runtime_dir
            && recorded.writer_path == format!("{SERVICE_PATH_PREFIX}{}", recorded.name)
        {
            Self::service_db(&recorded.name, runtime_dir)
        } else {
            Err(invalid())
        };
        let (path, writer_path) = opened.map_err(|_| invalid())?;
        Ok(Self {
            name: recorded.name.clone(),
            path,
            writer_path,
            beneath: Vec::new(),
            origin: ProfileOrigin::Recorded,
            notes: Vec::new(),
        })
    }

    /// The database a profile writes to, by rules 5 and 6 above.
    /// `nfs_rule` is `Some(true)` when a first `user-db` goes through the
    /// safe writer, `Some(false)` when it is written as itself, and `None`
    /// when that could not be told.
    pub fn from_profile(
        profile: &DconfProfile,
        config_home: &Path,
        runtime_dir: &Path,
        nfs_rule: Option<bool>,
    ) -> Result<Self, ProfileError> {
        let (name, (path, writer_path)) = match profile.sources.first() {
            Some(ProfileSource::User(name)) => match nfs_rule {
                Some(false) => (name.clone(), Self::user_db(name, config_home)?),
                Some(true) => {
                    let safe = format!("safe/{name}");
                    let opened = Self::service_db(&safe, runtime_dir)?;
                    (safe, opened)
                }
                None => return Err(ProfileError::DconfVersionUnknown),
            },
            Some(ProfileSource::Service(name)) => {
                (name.clone(), Self::service_db(name, runtime_dir)?)
            }
            Some(other) => {
                return Err(ProfileError::NotWritable {
                    first: Some(other.to_string()),
                });
            }
            None => return Err(ProfileError::NotWritable { first: None }),
        };
        let database_dir = config_home.join("dconf");
        let beneath = profile
            .sources
            .iter()
            .skip(1)
            .filter_map(|source| match source {
                // Opened the way dconf opens it: the name as written, under
                // the dconf directory.
                ProfileSource::User(lower) => Some(database_dir.join(lower)),
                _ => None,
            })
            .collect();
        Ok(Self {
            name,
            path,
            writer_path,
            beneath,
            origin: profile.origin.clone(),
            notes: profile.notes.clone(),
        })
    }
}

/// Parse a profile's text by rule 4 above.
pub fn parse_profile(text: &str, origin: ProfileOrigin) -> DconfProfile {
    let mut sources = Vec::new();
    let mut ignored = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let line = line.split('#').next().unwrap_or_default().trim_end();
        if line.is_empty() {
            continue;
        }
        let parsed = line
            .split_once(':')
            .filter(|(_, name)| !name.is_empty())
            .and_then(|(kind, name)| {
                let name = name.to_string();
                match kind {
                    "user-db" => Some(ProfileSource::User(name)),
                    "service-db" => Some(ProfileSource::Service(name)),
                    "system-db" => Some(ProfileSource::System(name)),
                    "file-db" => Some(ProfileSource::File(name)),
                    _ => None,
                }
            });
        match parsed {
            Some(source) => sources.push(source),
            None => ignored.push(line.to_string()),
        }
    }
    DconfProfile {
        origin,
        sources,
        ignored,
        notes: Vec::new(),
    }
}

fn built_in_profile() -> DconfProfile {
    DconfProfile {
        origin: ProfileOrigin::BuiltIn,
        sources: vec![ProfileSource::User("user".to_string())],
        ignored: Vec::new(),
        notes: Vec::new(),
    }
}

enum Opened {
    Text(PathBuf, String),
    NotFound,
}

fn read_profile(path: &Path) -> Result<Opened, ProfileError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Opened::Text(
            path.to_path_buf(),
            String::from_utf8_lossy(&bytes).into_owned(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Opened::NotFound),
        Err(error) => Err(ProfileError::Unreadable {
            path: path.to_path_buf(),
            detail: error.to_string(),
        }),
    }
}

/// dconf's `dconf_engine_open_profile_file`: an absolute path as it is, a
/// name through `SYSCONFDIR` and then `XDG_DATA_DIRS`.
fn open_profile_file(
    environment: &ProfileEnvironment,
    profile: &str,
) -> Result<(Opened, Vec<PathBuf>), ProfileError> {
    if profile.starts_with('/') {
        let path = PathBuf::from(profile);
        return Ok((read_profile(&path)?, vec![path]));
    }
    let mut searched = Vec::new();
    for prefix in std::iter::once(&environment.sysconf_dir).chain(&environment.data_dirs) {
        let path = prefix.join("dconf/profile").join(profile);
        searched.push(path.clone());
        if let Opened::Text(path, text) = read_profile(&path)? {
            return Ok((Opened::Text(path, text), searched));
        }
    }
    Ok((Opened::NotFound, searched))
}

/// A file dconf opens silently: any failure means "not there".
fn open_silently(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// The profile in force, by rules 1 to 3 above.
pub fn resolve_profile(environment: &ProfileEnvironment) -> Result<DconfProfile, ProfileError> {
    let mandatory = environment.mandatory_dir.join(environment.uid.to_string());
    if let Some(text) = open_silently(&mandatory) {
        return Ok(parse_profile(&text, ProfileOrigin::Mandatory(mandatory)));
    }
    if let Some(name) = &environment.dconf_profile {
        if name.is_empty() {
            return Err(ProfileError::NamedProfileMissing {
                name: name.clone(),
                searched: Vec::new(),
            });
        }
        return match open_profile_file(environment, name)? {
            (Opened::Text(path, text), _) => Ok(parse_profile(
                &text,
                ProfileOrigin::Named {
                    name: name.clone(),
                    path,
                },
            )),
            (Opened::NotFound, searched) => Err(ProfileError::NamedProfileMissing {
                name: name.clone(),
                searched,
            }),
        };
    }
    for desktop in ["COSMIC", "BetterOS"] {
        if environment.desktop.includes(desktop) {
            return Err(ProfileError::NotTheSessionEnvironment {
                desktop: environment.desktop.identity(),
            });
        }
    }
    let runtime = environment.runtime_dir.join("dconf/profile");
    if let Some(text) = open_silently(&runtime) {
        return Ok(parse_profile(&text, ProfileOrigin::Runtime(runtime)));
    }
    match open_profile_file(environment, "user") {
        Ok((Opened::Text(path, text), _)) => Ok(parse_profile(&text, ProfileOrigin::User(path))),
        Ok((Opened::NotFound, _)) => Ok(built_in_profile()),
        // dconf warns and uses its built-in profile (rule 3), so a GNOME
        // session with an unreadable `/etc/dconf/profile/user` reads and
        // writes `user-db:user`; so does Better OS, and it keeps the note.
        Err(ProfileError::Unreadable { path, detail }) => {
            let mut profile = built_in_profile();
            profile.notes.push(format!(
                "dconf.user_profile_unreadable:{}: {detail}",
                path.display()
            ));
            Ok(profile)
        }
        Err(other) => Err(other),
    }
}

/// The database the session in `environment` reads first and writes to.
pub fn session_database_in(
    environment: &ProfileEnvironment,
) -> Result<SessionDatabase, ProfileError> {
    let profile = resolve_profile(environment)?;
    SessionDatabase::from_profile(
        &profile,
        &environment.config_home,
        &environment.runtime_dir,
        environment.nfs_rule(),
    )
}

/// The database this process's session reads first and writes to. A note
/// dconf would warn about goes to standard error, as dconf's own warning does.
pub fn session_database() -> Result<SessionDatabase, ProfileError> {
    let database = session_database_in(&ProfileEnvironment::from_env());
    if let Ok(database) = &database {
        for note in &database.notes {
            eprintln!("better-os: {note}");
        }
    }
    database
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch {
        _directory: tempfile::TempDir,
        root: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().to_path_buf();
            Self {
                _directory: directory,
                root,
            }
        }

        fn environment(&self, dconf_profile: Option<&str>, desktop: &str) -> ProfileEnvironment {
            ProfileEnvironment {
                dconf_profile: dconf_profile.map(str::to_string),
                desktop: CurrentDesktop::parse(desktop),
                mandatory_dir: self.root.join("run/dconf/user"),
                uid: 1000,
                runtime_dir: self.root.join("runtime"),
                sysconf_dir: self.root.join("etc"),
                data_dirs: vec![self.root.join("local/share"), self.root.join("share")],
                config_home: self.root.join("config"),
                config_home_on_nfs: false,
                dconf_uses_safe_writer_on_nfs: None,
            }
        }

        fn write(&self, relative: &str, text: &str) -> PathBuf {
            let path = self.root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        }
    }

    #[test]
    fn with_no_profile_anywhere_it_is_gnome_s_user_database() {
        let scratch = Scratch::new();
        let database = session_database_in(&scratch.environment(None, "zorin:GNOME")).unwrap();
        assert_eq!(database.name, "user");
        assert_eq!(database.path, scratch.root.join("config/dconf/user"));
        assert_eq!(database.writer_path, "/ca/desrt/dconf/Writer/user");
        assert!(database.beneath.is_empty());
        assert_eq!(database.origin, ProfileOrigin::BuiltIn);
    }

    #[test]
    fn a_user_profile_of_user_db_user_is_the_same_database() {
        let scratch = Scratch::new();
        let path = scratch.write("etc/dconf/profile/user", "user-db:user\n");
        let database = session_database_in(&scratch.environment(None, "GNOME")).unwrap();
        assert_eq!(database.path, scratch.root.join("config/dconf/user"));
        assert_eq!(database.writer_path, "/ca/desrt/dconf/Writer/user");
        assert_eq!(database.origin, ProfileOrigin::User(path));
    }

    #[test]
    fn the_session_profile_writes_its_own_database_with_gnome_s_beneath() {
        let scratch = Scratch::new();
        // Where the Better OS session's package puts it: a data directory.
        let path = scratch.write(
            "share/dconf/profile/betteros",
            "user-db:betteros\nuser-db:user\n",
        );
        let database =
            session_database_in(&scratch.environment(Some("betteros"), "BetterOS:COSMIC")).unwrap();
        assert_eq!(database.name, "betteros");
        assert_eq!(database.path, scratch.root.join("config/dconf/betteros"));
        assert_eq!(database.writer_path, "/ca/desrt/dconf/Writer/betteros");
        assert_eq!(
            database.beneath,
            vec![scratch.root.join("config/dconf/user")]
        );
        assert_eq!(
            database.origin,
            ProfileOrigin::Named {
                name: "betteros".to_string(),
                path
            }
        );
    }

    #[test]
    fn sysconfdir_is_searched_before_the_data_directories() {
        let scratch = Scratch::new();
        scratch.write(
            "share/dconf/profile/cosmic",
            "user-db:cosmic\nuser-db:user\n",
        );
        let etc = scratch.write("etc/dconf/profile/cosmic", "user-db:local\n");
        let database = session_database_in(&scratch.environment(Some("cosmic"), "COSMIC")).unwrap();
        assert_eq!(database.name, "local");
        assert!(matches!(database.origin, ProfileOrigin::Named { ref path, .. } if *path == etc));
    }

    #[test]
    fn an_absolute_dconf_profile_is_opened_as_it_is() {
        let scratch = Scratch::new();
        let path = scratch.write("elsewhere/mine", "user-db:mine\n");
        let database =
            session_database_in(&scratch.environment(Some(path.to_str().unwrap()), "GNOME"))
                .unwrap();
        assert_eq!(database.name, "mine");
    }

    #[test]
    fn a_system_db_line_is_read_and_not_written() {
        let profile = parse_profile(
            "user-db:user\nsystem-db:local\nsystem-db:site\n",
            ProfileOrigin::BuiltIn,
        );
        assert_eq!(
            profile.sources,
            vec![
                ProfileSource::User("user".to_string()),
                ProfileSource::System("local".to_string()),
                ProfileSource::System("site".to_string()),
            ]
        );
        let database =
            SessionDatabase::from_profile(&profile, Path::new("/c"), Path::new("/r"), Some(false))
                .unwrap();
        assert_eq!(database.name, "user");
        assert!(database.beneath.is_empty(), "system databases are not read");

        let first = parse_profile("system-db:local\nuser-db:user\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&first, Path::new("/c"), Path::new("/r"), Some(false)),
            Err(ProfileError::NotWritable {
                first: Some("system-db:local".to_string())
            })
        );
    }

    #[test]
    fn lines_are_trimmed_comments_dropped_and_unknown_lines_skipped() {
        let profile = parse_profile(
            "# a comment\n\n   user-db:betteros   # the session's own\n\tuser-db:user\nnonsense\nuser-db:\n",
            ProfileOrigin::BuiltIn,
        );
        assert_eq!(
            profile.sources,
            vec![
                ProfileSource::User("betteros".to_string()),
                ProfileSource::User("user".to_string()),
            ]
        );
        assert_eq!(profile.ignored, vec!["nonsense", "user-db:"]);
    }

    #[test]
    fn a_missing_named_profile_is_reported_with_every_path_searched() {
        let scratch = Scratch::new();
        let error = session_database_in(&scratch.environment(Some("betteros"), "BetterOS:COSMIC"))
            .unwrap_err();
        assert_eq!(error.reason(), "dconf.profile_missing");
        let ProfileError::NamedProfileMissing { searched, .. } = error else {
            panic!("expected a missing profile");
        };
        assert_eq!(
            searched,
            vec![
                scratch.root.join("etc/dconf/profile/betteros"),
                scratch.root.join("local/share/dconf/profile/betteros"),
                scratch.root.join("share/dconf/profile/betteros"),
            ]
        );
    }

    #[test]
    fn an_unreadable_profile_is_reported_not_guessed() {
        let scratch = Scratch::new();
        // A directory where the profile file should be: it exists and cannot
        // be read as a file, whatever user the test runs as.
        std::fs::create_dir_all(scratch.root.join("etc/dconf/profile/betteros")).unwrap();
        let error =
            session_database_in(&scratch.environment(Some("betteros"), "BetterOS")).unwrap_err();
        assert_eq!(error.reason(), "dconf.profile_unreadable");
    }

    // Review finding 5: dconf warns about an unreadable `user` profile and
    // uses `user-db:user`, so GNOME keeps working; so does this, with a note.
    #[test]
    fn an_unreadable_user_profile_falls_back_to_gnome_s_database_with_a_note() {
        let scratch = Scratch::new();
        std::fs::create_dir_all(scratch.root.join("etc/dconf/profile/user")).unwrap();
        let database = session_database_in(&scratch.environment(None, "GNOME")).unwrap();
        assert_eq!(database.path, scratch.root.join("config/dconf/user"));
        assert_eq!(database.writer_path, "/ca/desrt/dconf/Writer/user");
        assert_eq!(database.origin, ProfileOrigin::BuiltIn);
        assert_eq!(database.notes.len(), 1);
        assert!(database.notes[0].starts_with("dconf.user_profile_unreadable:"));
        assert!(sentence(&database.notes[0]).is_some());
    }

    #[test]
    fn a_mandatory_profile_wins_over_dconf_profile() {
        let scratch = Scratch::new();
        scratch.write("share/dconf/profile/betteros", "user-db:betteros\n");
        scratch.write("run/dconf/user/1000", "user-db:enforced\nsystem-db:site\n");
        let database =
            session_database_in(&scratch.environment(Some("betteros"), "BetterOS")).unwrap();
        assert_eq!(database.name, "enforced");
    }

    #[test]
    fn the_runtime_profile_comes_before_the_user_profile_only_without_dconf_profile() {
        let scratch = Scratch::new();
        scratch.write("runtime/dconf/profile", "user-db:runtime\n");
        scratch.write("etc/dconf/profile/user", "user-db:user\n");
        scratch.write("etc/dconf/profile/named", "user-db:named\n");
        assert_eq!(
            session_database_in(&scratch.environment(None, "GNOME"))
                .unwrap()
                .name,
            "runtime"
        );
        assert_eq!(
            session_database_in(&scratch.environment(Some("named"), "GNOME"))
                .unwrap()
                .name,
            "named"
        );
    }

    #[test]
    fn a_cosmic_or_better_os_process_without_dconf_profile_is_reported() {
        let scratch = Scratch::new();
        for desktop in ["COSMIC", "BetterOS:COSMIC"] {
            let error = session_database_in(&scratch.environment(None, desktop)).unwrap_err();
            assert_eq!(error.reason(), "dconf.session_profile_not_in_environment");
        }
    }

    #[test]
    fn a_service_db_is_the_service_s_runtime_file_and_writer() {
        let profile = parse_profile("service-db:keyfile/user\n", ProfileOrigin::BuiltIn);
        let database =
            SessionDatabase::from_profile(&profile, Path::new("/c"), Path::new("/r"), Some(false))
                .unwrap();
        assert_eq!(database.name, "keyfile/user");
        assert_eq!(
            database.path,
            PathBuf::from("/r/dconf-service/keyfile/user")
        );
        assert_eq!(database.writer_path, "/ca/desrt/dconf/keyfile/user");
        let unknown = parse_profile("service-db:nothing/user\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&unknown, Path::new("/c"), Path::new("/r"), Some(false))
                .unwrap_err()
                .reason(),
            "dconf.profile_invalid_database_name"
        );
    }

    // Review finding 10: dconf 51.0 sends a `user-db` on NFS through the safe
    // writer; dconf 0.40 writes it as anywhere else, as 0.2.16 did.
    #[test]
    fn on_nfs_the_installed_dconf_decides_the_writer() {
        let profile = parse_profile("user-db:user\n", ProfileOrigin::BuiltIn);
        let old =
            SessionDatabase::from_profile(&profile, Path::new("/c"), Path::new("/r"), Some(false))
                .unwrap();
        assert_eq!(old.writer_path, "/ca/desrt/dconf/Writer/user");
        assert_eq!(old.path, PathBuf::from("/c/dconf/user"));
        let new =
            SessionDatabase::from_profile(&profile, Path::new("/c"), Path::new("/r"), Some(true))
                .unwrap();
        assert_eq!(new.name, "safe/user");
        assert_eq!(new.writer_path, "/ca/desrt/dconf/safe/user");
        assert_eq!(new.path, PathBuf::from("/r/dconf-service/safe/user"));
        assert_eq!(
            SessionDatabase::from_profile(&profile, Path::new("/c"), Path::new("/r"), None),
            Err(ProfileError::DconfVersionUnknown)
        );

        let scratch = Scratch::new();
        let mut environment = scratch.environment(None, "GNOME");
        environment.config_home_on_nfs = true;
        environment.dconf_uses_safe_writer_on_nfs = Some(false);
        assert_eq!(
            session_database_in(&environment).unwrap().writer_path,
            "/ca/desrt/dconf/Writer/user"
        );
        environment.dconf_uses_safe_writer_on_nfs = Some(true);
        assert_eq!(
            session_database_in(&environment).unwrap().writer_path,
            "/ca/desrt/dconf/safe/user"
        );
    }

    #[test]
    fn the_dconf_major_version_is_read_from_dpkg_s_version_string() {
        assert_eq!(dconf_major("0.40.0-4ubuntu0.1"), Some(0));
        assert_eq!(dconf_major("51.0-1"), Some(51));
        assert_eq!(dconf_major("1:52.1-2"), Some(52));
        assert_eq!(dconf_major(""), None);
        assert_eq!(dconf_major("x"), None);
    }

    #[test]
    fn a_recorded_database_is_opened_by_the_same_rules_and_a_forged_one_refused() {
        let user = SessionDatabase::recorded(
            &DconfDatabase::gnome_user(),
            Path::new("/c"),
            Some(Path::new("/r")),
        )
        .unwrap();
        assert_eq!(user.path, PathBuf::from("/c/dconf/user"));
        assert!(user.beneath.is_empty());
        let safe = DconfDatabase {
            name: "safe/user".to_string(),
            writer_path: "/ca/desrt/dconf/safe/user".to_string(),
        };
        assert_eq!(
            SessionDatabase::recorded(&safe, Path::new("/c"), Some(Path::new("/r")))
                .unwrap()
                .path,
            PathBuf::from("/r/dconf-service/safe/user")
        );
        for (name, writer_path) in [
            ("user", "/ca/desrt/dconf/Writer/other"),
            ("../user", "/ca/desrt/dconf/Writer/../user"),
            ("Writer/user", "/ca/desrt/dconf/Writer/user"),
            ("user", "/org/example/user"),
        ] {
            let forged = DconfDatabase {
                name: name.to_string(),
                writer_path: writer_path.to_string(),
            };
            assert_eq!(
                SessionDatabase::recorded(&forged, Path::new("/c"), Some(Path::new("/r")))
                    .unwrap_err()
                    .reason(),
                "dconf.recorded_database_invalid",
                "{name} {writer_path}"
            );
        }
    }

    #[test]
    fn every_reason_has_a_sentence_in_both_languages() {
        let errors = [
            ProfileError::NamedProfileMissing {
                name: String::new(),
                searched: Vec::new(),
            },
            ProfileError::Unreadable {
                path: PathBuf::new(),
                detail: String::new(),
            },
            ProfileError::NotWritable { first: None },
            ProfileError::InvalidDatabaseName {
                name: String::new(),
            },
            ProfileError::NotTheSessionEnvironment {
                desktop: String::new(),
            },
            ProfileError::DconfVersionUnknown,
            ProfileError::RecordedDatabaseInvalid {
                name: String::new(),
                writer_path: String::new(),
            },
        ];
        for error in errors {
            let reason = format!("{}:{error}", error.reason());
            let (en, zh) = sentence(&reason).unwrap_or_else(|| panic!("{reason}"));
            assert!(!en.is_empty() && !zh.is_empty());
        }
        assert!(sentence("dconf.database_beneath_unreadable:/x").is_some());
        assert!(sentence("gnome.something_else").is_none());
    }

    #[test]
    fn an_empty_profile_or_a_name_with_no_writer_is_reported() {
        let empty = parse_profile("# nothing\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&empty, Path::new("/c"), Path::new("/r"), Some(false)),
            Err(ProfileError::NotWritable { first: None })
        );
        let dashed = parse_profile("user-db:my-db\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&dashed, Path::new("/c"), Path::new("/r"), Some(false))
                .unwrap_err()
                .reason(),
            "dconf.profile_invalid_database_name"
        );
    }

    #[test]
    fn the_environment_is_read_as_glib_reads_it() {
        let values = |name: &str| match name {
            "HOME" => Some("/home/p".to_string()),
            "XDG_CONFIG_HOME" => Some(" ".to_string()),
            "DCONF_PROFILE" => Some("betteros".to_string()),
            "XDG_CURRENT_DESKTOP" => Some("BetterOS:COSMIC".to_string()),
            _ => None,
        };
        let environment = ProfileEnvironment::from_lookup(values, 1000);
        assert_eq!(environment.config_home, PathBuf::from("/home/p/.config"));
        assert_eq!(environment.runtime_dir, PathBuf::from("/home/p/.cache"));
        assert_eq!(
            environment.data_dirs,
            vec![
                PathBuf::from("/usr/local/share/"),
                PathBuf::from("/usr/share/")
            ]
        );
        assert_eq!(environment.dconf_profile.as_deref(), Some("betteros"));
        assert!(environment.desktop.includes("BetterOS"));
        assert_eq!(environment.sysconf_dir, PathBuf::from("/etc"));
    }
}
