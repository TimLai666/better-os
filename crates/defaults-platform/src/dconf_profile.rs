//! Which dconf database the running session reads, decided in one place.
//!
//! GSettings does not read one fixed file. dconf reads a *profile*, a list of
//! databases, and writes to the first of them. GNOME's default profile is
//! `user-db:user`, so GNOME's settings are `~/.config/dconf/user`, written
//! through `/ca/desrt/dconf/Writer/user`. A COSMIC session sets
//! `DCONF_PROFILE=cosmic` (`user-db:cosmic` then `user-db:user`), and the
//! Better OS session sets `DCONF_PROFILE=betteros` the same way (ADR 0038,
//! `engineering/specs/better-os-session.md`). A key written to GNOME's database
//! from inside such a session is hidden by any value the session's own
//! database holds, and reading GNOME's database reports a value the session is
//! not showing. Every Better OS reader and writer of dconf — the defaults
//! engine's GNOME adapters and Better Touchpad's backend — asks
//! [`session_database`] instead of naming a file.
//!
//! ## dconf's rules, as followed here
//!
//! From dconf's own documentation, `dconf(7)`, section PROFILES (dconf 0.40,
//! the version Ubuntu 24.04 ships, read 2026-11-21), and its source,
//! `engine/dconf-engine-profile.c`, `dconf-engine-source.c`, and
//! `dconf-engine-source-user.c` (GNOME/dconf `main`, read 2026-11-21):
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
//!    named `user` by the same lookup, then the built-in `user-db:user`.
//! 4. A profile line has leading whitespace skipped, everything from `#` on
//!    dropped, and trailing whitespace trimmed; an empty line is skipped. A
//!    line is `user-db:`, `service-db:`, `system-db:`, or `file-db:` followed
//!    by a non-empty name; dconf warns about any other line and skips it.
//! 5. "The first line indicates the database used to write changes" and
//!    should be a `user-db` or `service-db`. A `user-db:<name>` is the file
//!    `$XDG_CONFIG_HOME/dconf/<name>`, written through the object
//!    `/ca/desrt/dconf/Writer/<name>` of `ca.desrt.dconf`.
//! 6. When the user configuration directory is on NFS, a first `user-db:<name>`
//!    is replaced by `service-db:safe/<name>`.
//!
//! ## Where this stops short, and says so
//!
//! - Only a first `user-db` is a database this module can write: a profile
//!   whose first line is a `service-db` (or becomes one on NFS), a
//!   `system-db`, or a `file-db` is reported, never written around.
//! - A profile that exists and cannot be read is reported. dconf itself
//!   would warn and use the null configuration for a named profile, and the
//!   built-in `user-db:user` for the `user` profile; this module does not
//!   claim either.
//! - The later `user-db` lines are the databases beneath the session's own,
//!   in order, for a reader that needs the value the session shows when its
//!   own database holds nothing. `system-db` lines and their locks are not
//!   read, as they were not before this module existed.
//! - A process whose environment says COSMIC or Better OS and has no
//!   `DCONF_PROFILE` was not started with the session's environment, because
//!   both sessions set it. Its own profile would be GNOME's, so it is
//!   reported rather than used.

use std::fmt;
use std::path::{Path, PathBuf};

use better_core::CurrentDesktop;

/// Where a mandatory profile lives, per user id.
pub const MANDATORY_PROFILE_DIR: &str = "/run/dconf/user";
/// The system configuration directory dconf was built with on Ubuntu.
pub const SYSCONF_DIR: &str = "/etc";
/// The bus name of the dconf service.
pub const WRITER_SERVICE: &str = "ca.desrt.dconf";
/// The prefix of every user database's writer object.
pub const WRITER_PATH_PREFIX: &str = "/ca/desrt/dconf/Writer/";
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
}

impl ProfileEnvironment {
    /// This process's own environment and the system's paths.
    pub fn from_env() -> Self {
        // SAFETY: getuid has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        let mut environment = Self::from_lookup(|name| std::env::var(name).ok(), uid);
        environment.config_home_on_nfs = is_on_nfs(&environment.config_home);
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
        }
    }
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
}

/// A profile as dconf reads it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DconfProfile {
    pub origin: ProfileOrigin,
    pub sources: Vec<ProfileSource>,
    /// Lines dconf warns about and skips, kept so a status can name them.
    pub ignored: Vec<String>,
}

/// Why the session's database could not be decided. Each has a stable
/// machine key, [`Self::reason`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileError {
    /// `DCONF_PROFILE` names a profile no lookup path holds. dconf runs such
    /// a session with the null configuration.
    NamedProfileMissing {
        name: String,
        searched: Vec<PathBuf>,
    },
    /// A profile file exists and could not be read.
    Unreadable { path: PathBuf, detail: String },
    /// The profile's first line is not a database this module can write, or
    /// the profile lists nothing.
    NotWritable { first: Option<String> },
    /// The first database would be kept by the dconf service in the runtime
    /// directory (`service-db`, or a `user-db` on NFS).
    ServiceDatabase { source: String },
    /// A `user-db` name that cannot be the last element of a D-Bus object
    /// path, so no writer object exists for it.
    InvalidDatabaseName { name: String },
    /// The environment says COSMIC or Better OS and has no `DCONF_PROFILE`,
    /// which both sessions set: this process does not have the session's
    /// environment.
    NotTheSessionEnvironment { desktop: String },
}

impl ProfileError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::NamedProfileMissing { .. } => "dconf.profile_missing",
            Self::Unreadable { .. } => "dconf.profile_unreadable",
            Self::NotWritable { .. } => "dconf.profile_not_writable",
            Self::ServiceDatabase { .. } => "dconf.profile_service_db_unsupported",
            Self::InvalidDatabaseName { .. } => "dconf.profile_invalid_database_name",
            Self::NotTheSessionEnvironment { .. } => "dconf.session_profile_not_in_environment",
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
                "the dconf profile's first database is {first}, which is not a user database"
            ),
            Self::NotWritable { first: None } => {
                write!(formatter, "the dconf profile lists no database")
            }
            Self::ServiceDatabase { source } => write!(
                formatter,
                "the session's dconf database is {source}, kept by the dconf service, which Better OS does not write"
            ),
            Self::InvalidDatabaseName { name } => write!(
                formatter,
                "the dconf user database {name:?} has no writer object"
            ),
            Self::NotTheSessionEnvironment { desktop } => write!(
                formatter,
                "this process runs in a {desktop} session without the session's DCONF_PROFILE"
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

/// The database the session reads first and dconf writes to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionDatabase {
    /// The `user-db` name.
    pub name: String,
    /// `$XDG_CONFIG_HOME/dconf/<name>`.
    pub path: PathBuf,
    /// `/ca/desrt/dconf/Writer/<name>`.
    pub writer_path: String,
    /// The later `user-db` files, in profile order: what the session shows
    /// for a key its own database does not hold, before any schema default.
    pub beneath: Vec<PathBuf>,
    pub origin: ProfileOrigin,
}

impl SessionDatabase {
    /// A plain `user-db:<name>` profile over `config_home`, as when a caller
    /// names the database itself.
    pub fn user_db(config_home: &Path, name: &str) -> Result<Self, ProfileError> {
        Self::from_profile(
            &DconfProfile {
                origin: ProfileOrigin::BuiltIn,
                sources: vec![ProfileSource::User(name.to_string())],
                ignored: Vec::new(),
            },
            config_home,
            false,
        )
    }

    /// The database a profile writes to, by rules 5 and 6 above.
    pub fn from_profile(
        profile: &DconfProfile,
        config_home: &Path,
        config_home_on_nfs: bool,
    ) -> Result<Self, ProfileError> {
        let name = match profile.sources.first() {
            Some(ProfileSource::User(name)) if config_home_on_nfs => {
                return Err(ProfileError::ServiceDatabase {
                    source: format!("service-db:safe/{name}"),
                });
            }
            Some(ProfileSource::User(name)) => name,
            Some(ProfileSource::Service(name)) => {
                return Err(ProfileError::ServiceDatabase {
                    source: format!("service-db:{name}"),
                });
            }
            Some(other) => {
                return Err(ProfileError::NotWritable {
                    first: Some(other.to_string()),
                });
            }
            None => return Err(ProfileError::NotWritable { first: None }),
        };
        // A D-Bus object path element is `[A-Za-z0-9_]+`. dconf itself does
        // not check, and the call would fail at the bus.
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(ProfileError::InvalidDatabaseName { name: name.clone() });
        }
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
            name: name.clone(),
            path: database_dir.join(name),
            writer_path: format!("{WRITER_PATH_PREFIX}{name}"),
            beneath,
            origin: profile.origin.clone(),
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
    match open_profile_file(environment, "user")? {
        (Opened::Text(path, text), _) => Ok(parse_profile(&text, ProfileOrigin::User(path))),
        (Opened::NotFound, _) => Ok(DconfProfile {
            origin: ProfileOrigin::BuiltIn,
            sources: vec![ProfileSource::User("user".to_string())],
            ignored: Vec::new(),
        }),
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
        environment.config_home_on_nfs,
    )
}

/// The database this process's session reads first and writes to.
pub fn session_database() -> Result<SessionDatabase, ProfileError> {
    session_database_in(&ProfileEnvironment::from_env())
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
        let database = SessionDatabase::from_profile(&profile, Path::new("/c"), false).unwrap();
        assert_eq!(database.name, "user");
        assert!(database.beneath.is_empty(), "system databases are not read");

        let first = parse_profile("system-db:local\nuser-db:user\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&first, Path::new("/c"), false),
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

        std::fs::create_dir_all(scratch.root.join("etc/dconf/profile/user")).unwrap();
        let error = session_database_in(&scratch.environment(None, "GNOME")).unwrap_err();
        assert_eq!(error.reason(), "dconf.profile_unreadable");
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
    fn a_service_db_or_nfs_home_is_reported() {
        let profile = parse_profile("service-db:keyfile/user\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&profile, Path::new("/c"), false)
                .unwrap_err()
                .reason(),
            "dconf.profile_service_db_unsupported"
        );
        let profile = parse_profile("user-db:user\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&profile, Path::new("/c"), true),
            Err(ProfileError::ServiceDatabase {
                source: "service-db:safe/user".to_string()
            })
        );
    }

    #[test]
    fn an_empty_profile_or_a_name_with_no_writer_is_reported() {
        let empty = parse_profile("# nothing\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&empty, Path::new("/c"), false),
            Err(ProfileError::NotWritable { first: None })
        );
        let dashed = parse_profile("user-db:my-db\n", ProfileOrigin::BuiltIn);
        assert_eq!(
            SessionDatabase::from_profile(&dashed, Path::new("/c"), false)
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
