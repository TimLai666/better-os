//! A key's value as the session's dconf sees it through every database its
//! profile lists, system databases and their locks included.
//!
//! [`crate::dconf_profile`] decides which profile is in force and which
//! database the session writes; [`crate::dconf`] and [`crate::gsettings`]
//! read the session's own database and the `user-db`s beneath it. This
//! module adds what an administrator's settings need: a `system-db` (and a
//! `file-db`) is opened too, and a key one of them locks hides everything
//! above it, so a reader sees what GSettings would show and whether the
//! person could change it. Better Screenshot asks it for
//! `org.gnome.desktop.lockdown disable-save-to-disk` (ticket 253).
//!
//! ## dconf's rule, as followed here
//!
//! `dconf_engine_read` (`engine/dconf-engine.c`, GNOME/dconf tag `0.40.0`,
//! lines 673–747, read 2026-11-22):
//!
//! 1. The lock level is the *last* source (highest index) whose `.locks`
//!    table holds the key; locks in source 0 are ignored.
//! 2. With no lock and a writable first source, the first source is read
//!    (a `user-db` or `service-db`; queued writes this process has not
//!    sent yet do not exist for a reader in another process).
//! 3. Then every source from the lock level on (from 1 when nothing is
//!    locked and source 0 was read; from 0 when source 0 is not writable)
//!    until one holds the key.
//!
//! A database that does not exist or cannot be read is a source with no
//! values, as dconf treats it (`dconf-engine-source.c`,
//! `dconf_engine_source_refresh`), and the reading says so in its notes.
//! The files are, for a `user-db:<name>`, `$XDG_CONFIG_HOME/dconf/<name>`
//! (the safe writer's runtime copy on NFS, as [`crate::dconf_profile`]
//! decides for the first line); for a `service-db:<name>`,
//! `$XDG_RUNTIME_DIR/dconf-service/<name>`; for a `system-db:<name>`,
//! `SYSCONFDIR/dconf/db/<name>` (`engine/dconf-engine-source-system.c` line
//! 31); for a `file-db:<path>`, the path.

use std::path::{Path, PathBuf};

use crate::dconf_profile::{
    DconfProfile, ProfileEnvironment, ProfileError, ProfileSource, SessionDatabase,
    resolve_profile_as_dconf,
};
use crate::gvdb::{GVariantValue, GvdbDatabase};

/// One database of the profile, opened.
#[derive(Clone, Debug)]
pub struct StackSource {
    pub source: ProfileSource,
    pub path: PathBuf,
    /// `None` when the file is missing or could not be read: no values.
    pub database: Option<GvdbDatabase>,
}

/// Every database a profile lists, in its order, opened.
#[derive(Clone, Debug)]
pub struct DatabaseStack {
    pub sources: Vec<StackSource>,
    /// What could not be opened, and why.
    pub notes: Vec<String>,
}

/// A key as the stack shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyReading {
    /// The value, or `None` when no database holds the key and the schema's
    /// default applies.
    pub value: Option<GVariantValue>,
    /// A database below the first locks the key: the person cannot change
    /// it, and their own value is ignored.
    pub locked: bool,
    /// The database the value came from.
    pub from: Option<ProfileSource>,
}

/// The directories a profile's databases are found under.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackDirs {
    pub config_home: PathBuf,
    pub runtime_dir: PathBuf,
    pub sysconf_dir: PathBuf,
}

impl StackDirs {
    pub fn of(environment: &ProfileEnvironment) -> StackDirs {
        StackDirs {
            config_home: environment.config_home.clone(),
            runtime_dir: environment.runtime_dir.clone(),
            sysconf_dir: environment.sysconf_dir.clone(),
        }
    }
}

/// The file a profile line names, by dconf's rules above.
pub fn source_path(source: &ProfileSource, dirs: &StackDirs) -> PathBuf {
    match source {
        ProfileSource::User(name) => dirs.config_home.join("dconf").join(name),
        ProfileSource::Service(name) => dirs.runtime_dir.join("dconf-service").join(name),
        ProfileSource::System(name) => dirs.sysconf_dir.join("dconf/db").join(name),
        ProfileSource::File(path) => PathBuf::from(path),
    }
}

fn writable(source: &ProfileSource) -> bool {
    matches!(source, ProfileSource::User(_) | ProfileSource::Service(_))
}

impl DatabaseStack {
    /// Opens every database `profile` lists. `first` replaces the first
    /// line's file when the session's own database lives elsewhere (the
    /// safe writer's copy on NFS).
    pub fn open(profile: &DconfProfile, dirs: &StackDirs, first: Option<&Path>) -> DatabaseStack {
        let mut notes = Vec::new();
        let sources = profile
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let path = match (index, first) {
                    (0, Some(first)) => first.to_path_buf(),
                    _ => source_path(source, dirs),
                };
                let database = match std::fs::read(&path) {
                    Ok(bytes) => match GvdbDatabase::parse(&bytes) {
                        Ok(database) => {
                            if let Some(problem) = database.lock_problem() {
                                notes.push(format!(
                                    "{source} ({}): its lock table is not readable: {problem}",
                                    path.display()
                                ));
                            }
                            Some(database)
                        }
                        Err(error) => {
                            notes.push(format!(
                                "{source} ({}) is not readable: {error}",
                                path.display()
                            ));
                            None
                        }
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        notes.push(format!(
                            "{source} ({}) is not readable: {error}",
                            path.display()
                        ));
                        None
                    }
                };
                StackSource {
                    source: source.clone(),
                    path,
                    database,
                }
            })
            .collect();
        DatabaseStack { sources, notes }
    }

    /// The key as dconf shows it (`dconf_engine_read`, above).
    pub fn read(&self, key: &str) -> KeyReading {
        let lock_level = (1..self.sources.len())
            .rev()
            .find(|index| {
                self.sources[*index]
                    .database
                    .as_ref()
                    .is_some_and(|db| db.is_locked(key))
            })
            .unwrap_or(0);
        let mut start = lock_level;
        if lock_level == 0
            && let Some(first) = self.sources.first()
            && writable(&first.source)
        {
            if let Some(value) = first.database.as_ref().and_then(|db| db.get(key)) {
                return KeyReading {
                    value: Some(value.clone()),
                    locked: false,
                    from: Some(first.source.clone()),
                };
            }
            start = 1;
        }
        for source in self.sources.iter().skip(start) {
            if let Some(value) = source.database.as_ref().and_then(|db| db.get(key)) {
                return KeyReading {
                    value: Some(value.clone()),
                    locked: lock_level > 0,
                    from: Some(source.source.clone()),
                };
            }
        }
        KeyReading {
            value: None,
            locked: lock_level > 0,
            from: None,
        }
    }

    /// A boolean key: its value, or `default` when no database holds it or
    /// it holds a value of another type (GSettings then uses the schema's
    /// default).
    pub fn boolean(&self, key: &str, default: bool) -> bool {
        match self.read(key).value {
            Some(GVariantValue::Boolean(value)) => value,
            _ => default,
        }
    }
}

/// The stack of the profile dconf puts in force for this process.
pub fn session_stack() -> Result<DatabaseStack, ProfileError> {
    stack_in(&ProfileEnvironment::from_env())
}

/// The stack of the profile dconf puts in force in `environment`
/// ([`resolve_profile_as_dconf`]: a COSMIC or Better OS process without
/// `DCONF_PROFILE` reads the `user` profile, as dconf would).
pub fn stack_in(environment: &ProfileEnvironment) -> Result<DatabaseStack, ProfileError> {
    let profile = resolve_profile_as_dconf(environment)?;
    let dirs = StackDirs::of(environment);
    let nfs_rule = if environment.config_home_on_nfs {
        environment.dconf_uses_safe_writer_on_nfs
    } else {
        Some(false)
    };
    // The session's own database where dconf reads it; a profile with no
    // writable first line has none, and every line is read as written.
    let first = SessionDatabase::from_profile(
        &profile,
        &environment.config_home,
        &environment.runtime_dir,
        nfs_rule,
    )
    .ok()
    .map(|database| database.path);
    Ok(DatabaseStack::open(&profile, &dirs, first.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::{DatabaseStack, KeyReading, StackDirs, source_path, stack_in};
    use crate::dconf_profile::{ProfileEnvironment, ProfileOrigin, ProfileSource, parse_profile};
    use crate::gvdb::GVariantValue;
    use std::path::Path;

    // Compiled by dconf 0.40's own `dconf compile` from keyfiles:
    // `system-local-locked` sets disable-save-to-disk true and locks it, and
    // /org/example/plain/shown 'from local'; `system-site` sets the same key
    // false, unlocked, shown 'from site', below 'only in site';
    // `user-lockdown-off` sets the key false and shown 'from the user'.
    const LOCAL: &[u8] = include_bytes!("../tests/fixtures/dconf/system-local-locked");
    const SITE: &[u8] = include_bytes!("../tests/fixtures/dconf/system-site");
    const USER: &[u8] = include_bytes!("../tests/fixtures/dconf/user-lockdown-off");
    const LOCKDOWN: &str = "/org/gnome/desktop/lockdown/disable-save-to-disk";

    struct Machine {
        _dir: tempfile::TempDir,
        dirs: StackDirs,
    }

    fn machine() -> Machine {
        let dir = tempfile::tempdir().unwrap();
        let dirs = StackDirs {
            config_home: dir.path().join("config"),
            runtime_dir: dir.path().join("run"),
            sysconf_dir: dir.path().join("etc"),
        };
        std::fs::create_dir_all(dirs.config_home.join("dconf")).unwrap();
        std::fs::create_dir_all(dirs.sysconf_dir.join("dconf/db")).unwrap();
        Machine { _dir: dir, dirs }
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn stack(machine: &Machine, profile: &str) -> DatabaseStack {
        DatabaseStack::open(
            &parse_profile(profile, ProfileOrigin::BuiltIn),
            &machine.dirs,
            None,
        )
    }

    #[test]
    fn the_gvdb_reader_sees_a_system_databases_locks() {
        let local = crate::gvdb::GvdbDatabase::parse(LOCAL).unwrap();
        assert!(local.is_locked(LOCKDOWN));
        assert!(!local.is_locked("/org/example/plain/shown"));
        assert_eq!(local.get(LOCKDOWN), Some(&GVariantValue::Boolean(true)));
        let site = crate::gvdb::GvdbDatabase::parse(SITE).unwrap();
        assert!(!site.is_locked(LOCKDOWN));
    }

    #[test]
    fn each_line_names_dconfs_file() {
        let machine = machine();
        let dirs = &machine.dirs;
        assert_eq!(
            source_path(&ProfileSource::User("user".into()), dirs),
            dirs.config_home.join("dconf/user")
        );
        assert_eq!(
            source_path(&ProfileSource::Service("safe/user".into()), dirs),
            dirs.runtime_dir.join("dconf-service/safe/user")
        );
        assert_eq!(
            source_path(&ProfileSource::System("local".into()), dirs),
            dirs.sysconf_dir.join("dconf/db/local")
        );
        assert_eq!(
            source_path(&ProfileSource::File("/x/db".into()), dirs),
            Path::new("/x/db")
        );
    }

    #[test]
    fn the_users_value_wins_over_an_unlocked_system_value() {
        let machine = machine();
        write(&machine.dirs.config_home.join("dconf/user"), USER);
        write(&machine.dirs.sysconf_dir.join("dconf/db/site"), SITE);
        let stack = stack(&machine, "user-db:user\nsystem-db:site\n");
        let reading = stack.read("/org/example/plain/shown");
        assert_eq!(
            reading.value,
            Some(GVariantValue::Text("from the user".into()))
        );
        assert!(!reading.locked);
        assert_eq!(reading.from, Some(ProfileSource::User("user".into())));
        // A key only the system holds comes from it.
        assert_eq!(
            stack.read("/org/example/plain/below").value,
            Some(GVariantValue::Text("only in site".into()))
        );
    }

    #[test]
    fn a_lock_hides_the_users_value_and_everything_above_it() {
        let machine = machine();
        write(&machine.dirs.config_home.join("dconf/user"), USER);
        write(&machine.dirs.sysconf_dir.join("dconf/db/local"), LOCAL);
        write(&machine.dirs.sysconf_dir.join("dconf/db/site"), SITE);
        let stack = stack(&machine, "user-db:user\nsystem-db:local\nsystem-db:site\n");
        assert_eq!(
            stack.read(LOCKDOWN),
            KeyReading {
                value: Some(GVariantValue::Boolean(true)),
                locked: true,
                from: Some(ProfileSource::System("local".into())),
            }
        );
        assert!(stack.boolean(LOCKDOWN, false));
        // An unlocked key in the same database still follows the user.
        assert_eq!(
            stack.read("/org/example/plain/shown").value,
            Some(GVariantValue::Text("from the user".into()))
        );
    }

    #[test]
    fn the_last_lock_decides_and_reading_continues_below_it() {
        let machine = machine();
        write(&machine.dirs.sysconf_dir.join("dconf/db/local"), LOCAL);
        write(&machine.dirs.sysconf_dir.join("dconf/db/site"), SITE);
        // `local` locks the key but is listed last: the read starts there
        // and `site`'s unlocked false above it is ignored.
        let stack = stack(&machine, "user-db:user\nsystem-db:site\nsystem-db:local\n");
        let reading = stack.read(LOCKDOWN);
        assert_eq!(reading.value, Some(GVariantValue::Boolean(true)));
        assert!(reading.locked);
    }

    #[test]
    fn a_lock_in_the_first_source_is_ignored() {
        let machine = machine();
        write(&machine.dirs.config_home.join("dconf/user"), LOCAL);
        let stack = stack(&machine, "user-db:user\n");
        let reading = stack.read(LOCKDOWN);
        assert_eq!(reading.value, Some(GVariantValue::Boolean(true)));
        assert!(!reading.locked);
    }

    #[test]
    fn a_profile_without_its_system_database_never_sees_the_lock() {
        // The Better OS session's profile today lists only user databases
        // (better-os-session.md, the environment table): an administrator's
        // `local` database is not read there.
        let machine = machine();
        write(&machine.dirs.config_home.join("dconf/user"), USER);
        write(&machine.dirs.sysconf_dir.join("dconf/db/local"), LOCAL);
        let stack = stack(&machine, "user-db:betteros\nuser-db:user\n");
        assert!(!stack.boolean(LOCKDOWN, false));
        assert!(!stack.read(LOCKDOWN).locked);
    }

    #[test]
    fn missing_and_damaged_databases_hold_nothing_and_damage_is_noted() {
        let machine = machine();
        write(
            &machine.dirs.sysconf_dir.join("dconf/db/local"),
            b"not a database",
        );
        let stack = stack(
            &machine,
            "user-db:user\nsystem-db:local\nsystem-db:absent\n",
        );
        assert_eq!(stack.read(LOCKDOWN).value, None);
        assert!(!stack.boolean(LOCKDOWN, false));
        assert_eq!(stack.notes.len(), 1, "{:?}", stack.notes);
        assert!(stack.notes[0].contains("system-db:local"));
    }

    #[test]
    fn a_read_only_first_line_is_read_from_its_start() {
        let machine = machine();
        write(&machine.dirs.sysconf_dir.join("dconf/db/site"), SITE);
        let stack = stack(&machine, "system-db:site\n");
        assert_eq!(
            stack.read("/org/example/plain/shown").value,
            Some(GVariantValue::Text("from site".into()))
        );
    }

    #[test]
    fn a_session_environment_without_dconf_profile_reads_the_user_profile_as_dconf_does() {
        let machine = machine();
        let root = machine.dirs.config_home.parent().unwrap().to_path_buf();
        write(
            &root.join("etc/dconf/profile/user"),
            b"user-db:user\nsystem-db:local\n",
        );
        write(&machine.dirs.sysconf_dir.join("dconf/db/local"), LOCAL);
        let mut environment = ProfileEnvironment::from_lookup(
            |name| match name {
                "XDG_CURRENT_DESKTOP" => Some("BetterOS:COSMIC".into()),
                "HOME" => Some(root.join("home").to_string_lossy().into_owned()),
                "XDG_CONFIG_HOME" => Some(machine.dirs.config_home.to_string_lossy().into_owned()),
                "XDG_RUNTIME_DIR" => Some(machine.dirs.runtime_dir.to_string_lossy().into_owned()),
                "XDG_DATA_DIRS" => Some(root.join("share").to_string_lossy().into_owned()),
                _ => None,
            },
            1000,
        );
        environment.sysconf_dir = machine.dirs.sysconf_dir.clone();
        environment.mandatory_dir = root.join("run/dconf/user");
        // The writer's rule refuses this environment; the reader follows dconf.
        assert!(crate::dconf_profile::resolve_profile(&environment).is_err());
        let stack = stack_in(&environment).unwrap();
        assert!(stack.boolean(LOCKDOWN, false));
        assert!(stack.read(LOCKDOWN).locked);
    }

    #[test]
    fn a_damaged_lock_table_is_noted_in_the_stack() {
        let machine = machine();
        let mut bytes = LOCAL.to_vec();
        crate::gvdb::damage_lock_table(&mut bytes);
        write(&machine.dirs.sysconf_dir.join("dconf/db/local"), &bytes);
        let stack = stack(&machine, "user-db:user\nsystem-db:local\n");
        assert_eq!(stack.notes.len(), 1, "{:?}", stack.notes);
        assert!(stack.notes[0].contains("lock table"));
        // The value still reads; the lock it held cannot be seen.
        assert_eq!(
            stack.read(LOCKDOWN).value,
            Some(GVariantValue::Boolean(true))
        );
        assert!(!stack.read(LOCKDOWN).locked);
    }

    #[test]
    fn the_session_profile_is_resolved_and_opened() {
        let machine = machine();
        let root = machine.dirs.config_home.parent().unwrap().to_path_buf();
        write(
            &root.join("etc/dconf/profile/betteros"),
            b"user-db:betteros\nuser-db:user\nsystem-db:local\n",
        );
        write(&machine.dirs.sysconf_dir.join("dconf/db/local"), LOCAL);
        let mut environment = ProfileEnvironment::from_lookup(
            |name| match name {
                "DCONF_PROFILE" => Some("betteros".into()),
                "HOME" => Some(root.join("home").to_string_lossy().into_owned()),
                "XDG_CONFIG_HOME" => Some(machine.dirs.config_home.to_string_lossy().into_owned()),
                "XDG_RUNTIME_DIR" => Some(machine.dirs.runtime_dir.to_string_lossy().into_owned()),
                "XDG_DATA_DIRS" => Some(root.join("share").to_string_lossy().into_owned()),
                _ => None,
            },
            1000,
        );
        environment.sysconf_dir = machine.dirs.sysconf_dir.clone();
        environment.mandatory_dir = root.join("run/dconf/user");
        let stack = stack_in(&environment).unwrap();
        assert_eq!(stack.sources.len(), 3);
        assert_eq!(
            stack.sources[0].path,
            machine.dirs.config_home.join("dconf/betteros")
        );
        assert!(stack.boolean(LOCKDOWN, false));
    }
}
