//! "Show in file manager": the person's file manager, asked over
//! `org.freedesktop.FileManager1` (review of ticket 179).
//!
//! This is what GNOME and the browsers call for "Show in folder", and what
//! ticket 129 makes Better Files answer when it is the desktop's default. So
//! Better Git starts no program by path or by name for it: the bus starts
//! whichever file manager the person chose, Better Files or Nautilus, the
//! way any other application asks. A folder is opened with `ShowFolders`,
//! a file is shown selected in its folder with `ShowItems`.
//!
//! Under the test guard nothing is asked: on the session bus the call would
//! start an installed file manager, which a test or a sandbox run must never
//! do (ticket 179). The window says why instead.

use std::path::Path;

use better_core::test_guard::TestGuard;

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";
pub const INTERFACE: &str = "org.freedesktop.FileManager1";

/// The method for a path: a folder is opened, anything else is shown
/// selected in its folder.
pub fn method_for(is_folder: bool) -> &'static str {
    if is_folder {
        "ShowFolders"
    } else {
        "ShowItems"
    }
}

/// Asks the file manager on `connection` to show `path`.
pub fn show(connection: &zbus::blocking::Connection, path: &Path) -> Result<(), String> {
    let uri = better_core::uri::file_uri(path);
    connection
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            method_for(path.is_dir()),
            // No startup notification id: none was handed to this window.
            &(vec![uri], ""),
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Asks the file manager on the session bus, or says why it was not asked.
pub fn show_in_session(path: &Path) -> Result<(), String> {
    show_unless_guarded(TestGuard::from_environment().as_ref(), path, || {
        zbus::blocking::Connection::session().map_err(|error| error.to_string())
    })
}

/// Asks the file manager on the bus `connect` reaches, unless `guard` is on;
/// then nothing is connected to or asked, and the words say why.
pub fn show_unless_guarded(
    guard: Option<&TestGuard>,
    path: &Path,
    connect: impl FnOnce() -> Result<zbus::blocking::Connection, String>,
) -> Result<(), String> {
    if let Some(guard) = guard {
        return Err(guard.reason("the file manager"));
    }
    show(&connect()?, path)
}

#[cfg(test)]
mod tests {
    use super::{BUS_NAME, method_for, show};
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, Mutex};

    struct Bus(Child);

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A private bus with no service directory at all, so nothing installed
    /// (Nautilus, Better Files) can be activated on it.
    fn private_bus(directory: &std::path::Path) -> Result<(Bus, String), String> {
        let config = directory.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\"
 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context=\"default\">
    <allow send_destination=\"*\" eavesdrop=\"true\"/>
    <allow eavesdrop=\"true\"/>
    <allow own=\"*\"/>
  </policy>
</busconfig>
",
                directory.display()
            ),
        )
        .map_err(|error| error.to_string())?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut line = String::new();
        BufReader::new(child.stdout.take().ok_or("no output")?)
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        Ok((Bus(child), line.trim().to_string()))
    }

    /// Each method the fake was asked, with the URIs.
    type Calls = Arc<Mutex<Vec<(String, Vec<String>)>>>;

    #[derive(Default)]
    struct FakeFileManager {
        calls: Calls,
    }

    #[zbus::interface(name = "org.freedesktop.FileManager1")]
    impl FakeFileManager {
        fn show_folders(&self, uris: Vec<String>, _startup_id: String) {
            self.calls
                .lock()
                .unwrap()
                .push(("ShowFolders".into(), uris));
        }

        fn show_items(&self, uris: Vec<String>, _startup_id: String) {
            self.calls.lock().unwrap().push(("ShowItems".into(), uris));
        }
    }

    /// Under the test guard the bus is never reached: on the session bus the
    /// call would start an installed file manager.
    #[test]
    fn under_the_guard_nothing_is_asked_and_the_words_say_why() {
        let guard = better_core::test_guard::TestGuard::silent();
        let refusal = super::show_unless_guarded(Some(&guard), std::path::Path::new("/"), || {
            panic!("the bus must not be reached under the guard")
        })
        .unwrap_err();
        assert!(
            refusal.contains(better_core::test_guard::GUARD_VARIABLE),
            "{refusal}"
        );
    }

    #[test]
    fn a_folder_is_opened_and_a_file_is_shown_in_its_folder() {
        assert_eq!(method_for(true), "ShowFolders");
        assert_eq!(method_for(false), "ShowItems");
    }

    #[test]
    fn the_file_manager_on_the_bus_is_asked_for_the_folder_and_the_file() {
        let scratch = tempfile::tempdir().unwrap();
        let (_bus, address) =
            test_prerequisites::require!("dbus-daemon", private_bus(scratch.path()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let _server = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .name(BUS_NAME)
            .unwrap()
            .serve_at(
                super::OBJECT_PATH,
                FakeFileManager {
                    calls: calls.clone(),
                },
            )
            .unwrap()
            .build()
            .unwrap();
        let client = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .unwrap();
        let folder = scratch.path().join("my repo");
        std::fs::create_dir(&folder).unwrap();
        let file = folder.join("read me.md");
        std::fs::write(&file, "").unwrap();

        show(&client, &folder).unwrap();
        show(&client, &file).unwrap();

        let uri = |path: &std::path::Path| better_core::uri::file_uri(path);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                ("ShowFolders".to_string(), vec![uri(&folder)]),
                ("ShowItems".to_string(), vec![uri(&file)]),
            ]
        );
    }

    /// With no file manager on the bus, and nothing the bus may start, the
    /// call fails and says so; nothing else is tried.
    #[test]
    fn with_no_file_manager_the_call_says_so() {
        let scratch = tempfile::tempdir().unwrap();
        let (_bus, address) =
            test_prerequisites::require!("dbus-daemon", private_bus(scratch.path()));
        let client = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .unwrap();
        let error = show(&client, scratch.path()).unwrap_err();
        assert!(error.contains("ServiceUnknown"), "{error}");
    }
}
