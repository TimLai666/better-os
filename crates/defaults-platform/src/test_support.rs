//! A real `dconf-service` on a private bus, for the tests of every crate that
//! writes dconf (Better Defaults and Better Touchpad). Built only with the
//! `test-support` feature, which no shipped binary enables.
//!
//! `dconf-service` is started on a `dbus-daemon` the test owns, with every
//! XDG directory pointed into a temporary directory, and the bus is
//! configured with no service directories so nothing on it can be activated
//! with the developer's environment. Nothing here can reach the person's own
//! settings.
//!
//! When either program is missing, [`PrivateDconf::start_or_skip`] says so
//! and the test does nothing, except under CI, where a missing program fails
//! the test: CI installs both, and a test that quietly did nothing there
//! would be a check nobody runs. The rule is `test_prerequisites`' (ticket
//! 177).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::dconf_profile::ProfileEnvironment;
use crate::dconf_writer::{DconfWriter, USER_WRITER_PATH};
use crate::gvariant::{ChangeValue, Changeset};
use crate::gvdb::GvdbDatabase;

/// A `dbus-daemon` and a `dconf-service` that belong to one test alone.
pub struct PrivateDconf {
    bus: Child,
    service: Child,
    /// The private bus's address.
    pub address: String,
    /// GNOME's database in the scratch configuration directory.
    pub database: PathBuf,
    directory: tempfile::TempDir,
}

impl PrivateDconf {
    /// Starts both, or says why it could not.
    pub fn start() -> Result<Self, String> {
        let service_binary = ["/usr/libexec/dconf-service", "/usr/lib/dconf/dconf-service"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists())
            .ok_or("dconf-service is not installed")?;
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let root = directory.path().to_path_buf();
        for name in ["config", "runtime", "home", "cache"] {
            std::fs::create_dir_all(root.join(name)).map_err(|error| error.to_string())?;
        }
        // No <servicedir>, so nothing on this bus can be activated — in
        // particular not a dconf-service carrying the developer's environment.
        let config = root.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:tmpdir={}</listen>\
                 <auth>EXTERNAL</auth><policy context=\"default\">\
                 <allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/>\
                 <allow own=\"*\"/></policy></busconfig>\n",
                root.display()
            ),
        )
        .map_err(|error| error.to_string())?;
        let path = std::env::var("PATH").unwrap_or_default();
        let isolated = |command: &mut Command| {
            command
                .env_clear()
                .env("PATH", &path)
                .env("HOME", root.join("home"))
                .env("XDG_CONFIG_HOME", root.join("config"))
                .env("XDG_RUNTIME_DIR", root.join("runtime"))
                .env("XDG_CACHE_HOME", root.join("cache"));
        };

        let mut bus_command = Command::new("dbus-daemon");
        bus_command
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        isolated(&mut bus_command);
        let mut bus = bus_command
            .spawn()
            .map_err(|error| format!("dbus-daemon could not start: {error}"))?;
        let mut address = String::new();
        let read = bus
            .stdout
            .take()
            .ok_or("dbus-daemon gave no output")
            .and_then(|stdout| {
                BufReader::new(stdout)
                    .read_line(&mut address)
                    .map_err(|_| "dbus-daemon's address could not be read")
            });
        let address = address.trim().to_string();
        if read.is_err() || address.is_empty() {
            let _ = bus.kill();
            let _ = bus.wait();
            return Err("dbus-daemon printed no address".to_string());
        }

        let mut service_command = Command::new(service_binary);
        service_command.stdout(Stdio::null()).stderr(Stdio::null());
        isolated(&mut service_command);
        service_command.env("DBUS_SESSION_BUS_ADDRESS", &address);
        let service = match service_command.spawn() {
            Ok(service) => service,
            Err(error) => {
                let _ = bus.kill();
                let _ = bus.wait();
                return Err(format!("dconf-service could not start: {error}"));
            }
        };
        let dconf = Self {
            bus,
            service,
            address,
            database: root.join("config/dconf/user"),
            directory,
        };

        // The service owns its name a moment after it starts. Nothing can
        // activate it on this bus, so until then a call simply fails.
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if dconf.writer().probe().is_ok() {
                return Ok(dconf);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("dconf-service never answered on the private bus".to_string())
    }

    /// [`Self::start`], or `None` with the reason on standard error. Under
    /// CI a missing service fails the test instead: the rule is
    /// `test_prerequisites`', which every skipping test shares.
    #[track_caller]
    pub fn start_or_skip() -> Option<Self> {
        test_prerequisites::available("dbus-daemon and dconf-service", Self::start())
    }

    /// The scratch directory every XDG variable of the service points into.
    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    /// A writer to GNOME's database.
    pub fn writer(&self) -> DconfWriter {
        self.writer_at(USER_WRITER_PATH)
    }

    /// A writer to the database at `object_path`.
    pub fn writer_at(&self, object_path: &str) -> DconfWriter {
        DconfWriter::connect_to(&self.address, object_path).expect("the private bus is listening")
    }

    /// The profile environment of a session over this service's directories.
    pub fn environment(&self, dconf_profile: Option<&str>, desktop: &str) -> ProfileEnvironment {
        let root = self.root();
        ProfileEnvironment {
            dconf_profile: dconf_profile.map(str::to_string),
            desktop: better_core::CurrentDesktop::parse(desktop),
            mandatory_dir: root.join("run/dconf/user"),
            uid: 1000,
            runtime_dir: root.join("runtime"),
            sysconf_dir: root.join("etc"),
            data_dirs: vec![root.join("share")],
            config_home: root.join("config"),
            config_home_on_nfs: false,
            dconf_uses_safe_writer_on_nfs: None,
        }
    }

    /// `$XDG_CONFIG_HOME` and `$XDG_RUNTIME_DIR` of the service.
    pub fn dirs(&self) -> (PathBuf, PathBuf) {
        (self.root().join("config"), self.root().join("runtime"))
    }

    /// The Better OS session's profile, as its package installs it.
    pub fn install_session_profile(&self) {
        let profile = self.root().join("share/dconf/profile/betteros");
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(profile, "user-db:betteros\nuser-db:user\n").unwrap();
    }

    /// GNOME's own database, given a value the way GNOME Settings would.
    pub fn seed_gnome(&self, key: &str, value: ChangeValue) {
        let mut change = Changeset::new("/").unwrap();
        change.set_path(key, value).unwrap();
        self.writer().change(&change).unwrap();
    }
}

/// A database file as the service wrote it.
pub fn read_database(path: &Path) -> GvdbDatabase {
    GvdbDatabase::parse(&std::fs::read(path).unwrap()).unwrap()
}

impl Drop for PrivateDconf {
    fn drop(&mut self) {
        let _ = self.service.kill();
        let _ = self.service.wait();
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}
