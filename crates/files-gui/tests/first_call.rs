//! Better Files answers the "Show in folder" call that D-Bus-activated it.
//!
//! zbus starts a connection's object server lazily, in a task that subscribes
//! to method calls some time after it is spawned. A name requested before that
//! subscription exists lets the call the bus was holding for the name reach a
//! stream nobody is reading, and that call is never answered (ticket 107).
//!
//! So the real path, [`files_gui::filemanager1::serve_on`] as `serve` calls
//! it, is started [`STARTS`] times on a private, activating `dbus-daemon`,
//! each time behind a `ShowFolders` the bus is holding for
//! `org.freedesktop.FileManager1`, and every one must be answered and reach
//! the window's channel.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use files_core::LocalPath;
use files_gui::filemanager1::{BUS_NAME, OBJECT_PATH, serve_on};
use files_gui::launch::StartTarget;

/// How many times the name is taken.
const STARTS: usize = 200;

/// How long a call may go unanswered once the name is owned; long enough that
/// only a call nobody will ever answer runs into it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// The caller's own limit, longer than any start plus [`ANSWER_TIMEOUT`].
const CALLER_TIMEOUT: Duration = Duration::from_secs(600);

/// A private bus that D-Bus-activates `org.freedesktop.FileManager1`.
///
/// The service file's `Exec` only records that the bus asked for the name and
/// idles for a second. The bus holds a call to the name until somebody owns
/// it and delivers it the moment the name is taken. The bus, its
/// configuration and the service file are all this test's own.
struct ActivatingBus {
    child: Child,
    address: String,
    directory: PathBuf,
}

impl ActivatingBus {
    fn start() -> Option<Self> {
        let directory = std::env::temp_dir().join(format!(
            "better-os-first-call-files-gui-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let services = directory.join("services");
        std::fs::create_dir_all(&services).ok()?;
        let asked = directory.join("asked");
        std::fs::write(
            services.join(format!("{BUS_NAME}.service")),
            format!(
                "[D-BUS Service]\nName={BUS_NAME}\nExec=/bin/sh -c \"touch {}; exec sleep 1\"\n",
                asked.display()
            ),
        )
        .ok()?;
        let config = directory.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:dir={}</listen>\
                 <auth>EXTERNAL</auth><servicedir>{}</servicedir>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
                directory.display(),
                services.display()
            ),
        )
        .ok()?;

        let child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            let _ = std::fs::remove_dir_all(&directory);
            return None;
        };
        use std::io::{BufRead, BufReader};
        let mut address = String::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = BufReader::new(stdout).read_line(&mut address);
        }
        let bus = Self {
            child,
            address: address.trim().to_string(),
            directory,
        };
        (!bus.address.is_empty()).then_some(bus)
    }

    fn builder(&self) -> zbus::blocking::connection::Builder<'static> {
        zbus::blocking::connection::Builder::address(self.address.as_str())
            .expect("a builder for the bus")
    }

    fn asked(&self) -> PathBuf {
        self.directory.join("asked")
    }

    /// Waits until the bus has started activating the name, so a call to it
    /// is being held for whoever takes it.
    fn activating(&self) -> bool {
        for _ in 0..5_000 {
            if self.asked().exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}

impl Drop for ActivatingBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn every_start_answers_the_call_that_activated_it() {
    let Some(bus) = ActivatingBus::start() else {
        eprintln!("skipping: dbus-daemon is not available in this environment");
        return;
    };
    let caller = bus
        .builder()
        .method_timeout(CALLER_TIMEOUT)
        .build()
        .expect("a client connection");
    let dbus = zbus::blocking::fdo::DBusProxy::new(&caller).unwrap();
    let folder = bus.directory.join("services");
    let uri = format!("file://{}", folder.display());
    let expected = StartTarget::Folder(LocalPath::new(folder.clone()).unwrap());

    let mut unanswered = Vec::new();
    for start in 0..STARTS {
        let _ = std::fs::remove_file(bus.asked());
        let (answer, answers) = std::sync::mpsc::channel();
        let call = {
            let caller = caller.clone();
            let uri = uri.clone();
            std::thread::spawn(move || {
                let reply = caller.call_method(
                    Some(BUS_NAME),
                    OBJECT_PATH,
                    Some("org.freedesktop.FileManager1"),
                    "ShowFolders",
                    &(vec![uri], ""),
                );
                let _ = answer.send(reply.map(|_| ()).map_err(|error| error.to_string()));
            })
        };
        assert!(bus.activating(), "start {start}: the bus activated");

        let (sender, requests) = std::sync::mpsc::channel();
        let (connection, reply) = serve_on(bus.builder(), sender).expect("served on the bus");
        assert!(
            reply.is_ok(),
            "start {start}: the name was granted: {reply:?}"
        );

        match answers.recv_timeout(ANSWER_TIMEOUT) {
            Ok(answer) => {
                // The caller has sent its answer, so its thread is ending.
                let _ = call.join();
                match answer {
                    Ok(()) => assert_eq!(
                        requests.try_recv().ok(),
                        Some(expected.clone()),
                        "start {start}: the answered call reached the window"
                    ),
                    Err(error) => unanswered.push(format!("start {start}: {error}")),
                }
            }
            // A caller that ended without sending panicked: pass its message on.
            Err(_) if call.is_finished() => match call.join() {
                Err(panic) => std::panic::resume_unwind(panic),
                Ok(()) => unreachable!("the caller always sends before it ends"),
            },
            // A call still waiting is left to the bus; nothing more comes of it.
            Err(_) => unanswered.push(format!(
                "start {start}: no answer within {ANSWER_TIMEOUT:?} of the name being taken"
            )),
        }

        // Give the name up before the next start asks the bus for it again.
        let _ = connection.release_name(BUS_NAME);
        drop(connection);
        let name = zbus::names::BusName::try_from(BUS_NAME).unwrap();
        while dbus.name_has_owner(name.clone()).unwrap() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    assert!(
        unanswered.is_empty(),
        "{} of {STARTS} starts left the call that activated them unanswered: {unanswered:#?}",
        unanswered.len()
    );
    eprintln!("{STARTS} of {STARTS} starts answered the call that activated them");
}
