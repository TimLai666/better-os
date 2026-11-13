//! Opening the popup and the window.
//!
//! Both are the window's own program: `better-monitor --popup --at X,Y` is the
//! compact popup, `better-monitor` the full window. A click while the popup
//! is open closes it, so the icon toggles it the way a flyout behaves. The
//! popup also closes itself when it loses focus; a child that has exited is
//! simply forgotten.
//!
//! Clicking the icon while the popup is open takes the focus away from the
//! popup first, so the popup has already closed itself by the time the click
//! reaches the item. Without care that click would open it again at once. A
//! click that arrives within [`REOPEN_DEBOUNCE`] of the popup closing is
//! therefore the click that closed it, and opens nothing.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How soon after the popup closed a click still counts as the one that
/// closed it.
pub const REOPEN_DEBOUNCE: Duration = Duration::from_millis(500);

/// The window's program.
pub const WINDOW_PROGRAM: &str = "better-monitor";

/// The arguments that open the popup at a point.
pub fn popup_arguments(x: i32, y: i32) -> Vec<String> {
    vec!["--popup".into(), "--at".into(), format!("{x},{y}")]
}

/// Whether a systemd user manager runs this session, by its private socket.
/// Without one (a session not run by systemd) the window is started directly.
pub fn has_user_manager(runtime_dir: Option<&std::path::Path>) -> bool {
    runtime_dir.is_some_and(|dir| dir.join("systemd").join("private").exists())
}

/// `systemd-run` arguments that run the window in a transient scope of its
/// own and return its exit status. `systemd-run --scope` replaces itself with
/// the program, so the process the item holds is the window itself.
pub fn scope_arguments() -> [&'static str; 5] {
    ["--user", "--scope", "--quiet", "--collect", "--"]
}

/// What a click does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Click {
    Open,
    Close,
    /// The popup closed itself because this very click took its focus.
    AlreadyClosed,
}

/// What a click does, given whether a popup is open and how long ago one
/// closed itself.
pub fn on_click(popup_open: bool, closed_ago: Option<Duration>) -> Click {
    if popup_open {
        Click::Close
    } else if closed_ago.is_some_and(|ago| ago < REOPEN_DEBOUNCE) {
        Click::AlreadyClosed
    } else {
        Click::Open
    }
}

/// How long after starting a failure still counts as the way of starting
/// failing, rather than the window ending: `systemd-run` that finds no user
/// manager, or is refused by it, exits unsuccessfully within this.
pub const FAILED_START: Duration = Duration::from_secs(3);

/// How often a started window is looked at to see whether it has ended.
const WATCH_INTERVAL: Duration = Duration::from_millis(50);

/// A window started the first way that worked, with the way left to try if
/// that one fails at once. Nothing here waits: starting returns as soon as a
/// process runs, and [`Started::poll`] only looks.
pub struct Started {
    child: Child,
    started: Instant,
    failed_start: Duration,
    fallbacks: Vec<Command>,
    arguments: Vec<String>,
}

/// What [`Started::poll`] found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Running,
    /// The first way failed at once and the window was started the next way.
    Restarted,
    Exited,
}

fn spawn_one(command: &mut Command, arguments: &[String]) -> std::io::Result<Child> {
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
}

impl Started {
    /// Start the first of these commands that is installed. The rest are
    /// kept to fall back on if it exits unsuccessfully within `failed_start`.
    pub fn start(
        commands: Vec<Command>,
        arguments: &[String],
        failed_start: Duration,
    ) -> std::io::Result<Self> {
        let mut commands = std::collections::VecDeque::from(commands);
        let mut last_error = std::io::Error::from(std::io::ErrorKind::NotFound);
        while let Some(mut command) = commands.pop_front() {
            match spawn_one(&mut command, arguments) {
                Ok(child) => {
                    return Ok(Self {
                        child,
                        started: Instant::now(),
                        failed_start,
                        fallbacks: commands.into(),
                        arguments: arguments.to_vec(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    eprintln!(
                        "better-monitor-tray: {} is not installed; trying the next way to start the window",
                        command.get_program().to_string_lossy()
                    );
                    last_error = error;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error)
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Look, without waiting, whether the window still runs. A first way
    /// that failed soon enough gives way to the next, once.
    pub fn poll(&mut self) -> Step {
        let status = match self.child.try_wait() {
            Ok(None) => return Step::Running,
            Ok(Some(status)) => status,
            Err(_) => return Step::Exited,
        };
        if status.success() || self.started.elapsed() >= self.failed_start {
            return Step::Exited;
        }
        let failed = self.child.id();
        while !self.fallbacks.is_empty() {
            let mut command = self.fallbacks.remove(0);
            match spawn_one(&mut command, &self.arguments) {
                Ok(child) => {
                    eprintln!(
                        "better-monitor-tray: the window's first start (process {failed}) failed ({status}); started it as {}",
                        command.get_program().to_string_lossy()
                    );
                    self.child = child;
                    self.started = Instant::now();
                    return Step::Restarted;
                }
                Err(error) => eprintln!(
                    "better-monitor-tray: {} could not be started: {error}",
                    command.get_program().to_string_lossy()
                ),
            }
        }
        Step::Exited
    }

    /// End the window, with no fallback after it.
    pub fn kill(&mut self) {
        self.fallbacks.clear();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The popup process, if one is open.
#[derive(Default)]
pub struct PopupLauncher {
    /// Programs to run in place of the window, each with leading arguments,
    /// tried in order like the scope and the direct start; for tests.
    programs: Vec<(String, Vec<String>)>,
    shared: Arc<Mutex<Shared>>,
}

/// The open popup and when the last one closed itself, shared with the
/// thread that watches it.
#[derive(Default)]
struct Shared {
    popup: Option<Started>,
    /// Counts popups opened, so a watcher of an older one stops.
    generation: u64,
    closed_at: Option<Instant>,
}

impl Shared {
    /// Whether the popup is still open; one that closed itself is reaped
    /// and its closing time recorded, once.
    fn poll(&mut self) -> bool {
        match self.popup.as_mut().map(Started::poll) {
            Some(Step::Running | Step::Restarted) => true,
            Some(Step::Exited) => {
                self.popup = None;
                self.closed_at = Some(Instant::now());
                false
            }
            None => false,
        }
    }
}

impl PopupLauncher {
    pub fn new() -> Self {
        Self::default()
    }

    /// A launcher that runs another program with leading arguments, for
    /// tests.
    pub fn with_program(program: impl Into<String>, leading: &[&str]) -> Self {
        Self::with_programs(&[(&program.into(), leading)])
    }

    /// A launcher that tries several programs in order, the way the window
    /// is tried in a scope first and then directly; for tests.
    pub fn with_programs(programs: &[(&str, &[&str])]) -> Self {
        Self {
            programs: programs
                .iter()
                .map(|(program, leading)| {
                    (
                        program.to_string(),
                        leading.iter().map(|a| a.to_string()).collect(),
                    )
                })
                .collect(),
            shared: Arc::default(),
        }
    }

    /// The ways to start the window, in the order they are tried.
    fn commands(&self) -> Vec<Command> {
        if !self.programs.is_empty() {
            return self
                .programs
                .iter()
                .map(|(program, leading)| {
                    let mut command = Command::new(program);
                    command.args(leading);
                    command
                })
                .collect();
        }
        let mut commands = Vec::new();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
        if has_user_manager(runtime.as_deref()) {
            // A scope of its own under the user manager: the window is not
            // part of the taskbar item's unit, so stopping or restarting the
            // item never touches it.
            let mut command = Command::new("systemd-run");
            command.args(scope_arguments());
            command.arg(WINDOW_PROGRAM);
            commands.push(command);
        }
        // Without systemd-run, or when it fails, the window still opens:
        // started directly, it only shares the item's unit.
        commands.push(Command::new(WINDOW_PROGRAM));
        commands
    }

    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether the popup is still open; a popup that closed itself is reaped.
    pub fn is_open(&mut self) -> bool {
        self.shared().poll()
    }

    /// Open the popup at the point, or close it if it is open. Returns as
    /// soon as the popup's process runs; nothing here waits for it.
    pub fn toggle(&mut self, x: i32, y: i32) -> std::io::Result<Click> {
        let mut shared = self.shared();
        let open = shared.poll();
        let closed_ago = shared.closed_at.map(|at| at.elapsed());
        let click = on_click(open, closed_ago);
        match click {
            Click::AlreadyClosed => {}
            Click::Close => {
                // Closed by the item, not by itself: no debounce follows.
                if let Some(mut popup) = shared.popup.take() {
                    popup.kill();
                }
            }
            Click::Open => {
                let popup = Started::start(self.commands(), &popup_arguments(x, y), FAILED_START)?;
                shared.generation += 1;
                shared.closed_at = None;
                shared.popup = Some(popup);
                let generation = shared.generation;
                drop(shared);
                self.watch(generation);
            }
        }
        Ok(click)
    }

    /// Watch the popup of this generation in the background, so the time
    /// it closed itself is recorded when it closes, not at the next click,
    /// and a first start that fails is followed by the next way.
    fn watch(&self, generation: u64) {
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(WATCH_INTERVAL);
                let mut shared = shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if shared.generation != generation || !shared.poll() {
                    return;
                }
            }
        });
    }

    /// Open the full window, and close the popup if it is open. Returns as
    /// soon as the window's process runs.
    pub fn open_window(&mut self) -> std::io::Result<()> {
        {
            let mut shared = self.shared();
            if let Some(mut popup) = shared.popup.take() {
                popup.kill();
            }
        }
        let mut window = Started::start(self.commands(), &[], FAILED_START)?;
        // The window runs on its own; a thread falls back if its first start
        // fails and reaps it so no zombie is left.
        std::thread::spawn(move || {
            while window.poll() != Step::Exited {
                std::thread::sleep(WATCH_INTERVAL);
            }
        });
        Ok(())
    }
}

impl Drop for PopupLauncher {
    fn drop(&mut self) {
        if let Some(mut popup) = self.shared().popup.take() {
            popup.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popup_is_the_window_program_with_the_click_point() {
        assert_eq!(
            popup_arguments(1500, 12),
            vec!["--popup", "--at", "1500,12"]
        );
        assert_eq!(on_click(false, None), Click::Open);
        assert_eq!(on_click(true, None), Click::Close);
        // The click that took the popup's focus closed it; it opens nothing.
        assert_eq!(
            on_click(false, Some(Duration::from_millis(80))),
            Click::AlreadyClosed
        );
        assert_eq!(on_click(false, Some(Duration::from_secs(3))), Click::Open);
    }

    #[test]
    fn the_window_gets_a_scope_of_its_own_only_where_a_user_manager_runs() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!has_user_manager(Some(directory.path())));
        assert!(!has_user_manager(None));
        std::fs::create_dir_all(directory.path().join("systemd")).unwrap();
        std::fs::write(directory.path().join("systemd/private"), b"").unwrap();
        assert!(has_user_manager(Some(directory.path())));
        assert_eq!(
            scope_arguments(),
            ["--user", "--scope", "--quiet", "--collect", "--"]
        );
    }

    #[test]
    fn a_watcher_of_an_older_popup_leaves_the_new_one_alone() {
        let mut launcher = PopupLauncher::with_program("sh", &["-c", "sleep 30", "popup"]);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Close);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
        // The first popup's watcher wakes, sees a later generation, and
        // stops; the second popup stays open and nothing is recorded.
        std::thread::sleep(WATCH_INTERVAL * 3);
        assert!(launcher.is_open());
        assert!(launcher.shared().closed_at.is_none());
    }

    #[test]
    fn a_popup_seen_exited_by_a_click_before_its_watcher_still_debounces() {
        let mut launcher = PopupLauncher::with_program("sh", &["-c", "exit 0", "popup"]);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
        // Whichever notices first, the click or the watcher, the closing is
        // recorded and the next click is the one that closed it.
        let deadline = Instant::now() + Duration::from_secs(2);
        while launcher.is_open() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::AlreadyClosed);
    }

    #[test]
    fn a_second_click_closes_the_popup_the_first_opened() {
        // A shell that sleeps stands in for the popup: open until killed.
        let mut launcher = PopupLauncher::with_program("sh", &["-c", "sleep 30", "popup"]);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
        assert!(launcher.is_open());
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Close);
        assert!(!launcher.is_open());
    }

    #[test]
    fn a_popup_that_closed_itself_is_opened_again_by_the_next_click() {
        let mut launcher = PopupLauncher::with_program("sh", &["-c", "exit 0", "popup"]);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!launcher.is_open());
        // Straight after it closed, a click is the one that closed it.
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::AlreadyClosed);
        std::thread::sleep(REOPEN_DEBOUNCE);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
    }

    #[test]
    fn a_missing_systemd_run_falls_back_to_starting_the_window_directly() {
        let mut launcher = PopupLauncher::with_programs(&[
            ("/nonexistent/systemd-run", &[]),
            ("sh", &["-c", "sleep 30", "popup"]),
        ]);
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
        assert!(launcher.is_open());
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Close);
    }

    #[test]
    fn a_scope_that_fails_falls_back_once_without_holding_up_the_click() {
        let directory = tempfile::tempdir().unwrap();
        let ran = directory.path().join("ran");
        let record = format!("echo direct >> {}; sleep 30", ran.display());
        // The first way fails after 300 ms, later than a click could wait.
        let mut launcher = PopupLauncher::with_programs(&[
            ("sh", &["-c", "sleep 0.3; exit 1", "systemd-run"]),
            ("sh", &["-c", &record, "popup"]),
        ]);
        let clicked = Instant::now();
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Open);
        assert!(
            clicked.elapsed() < Duration::from_millis(100),
            "the click returned at once"
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !ran.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(&ran).unwrap(), "direct\n");
        // The fallback is the popup now: open, and a click closes it.
        assert!(launcher.is_open());
        assert!(
            launcher.shared().closed_at.is_none(),
            "no debounce from the failed start"
        );
        assert_eq!(launcher.toggle(0, 0).unwrap(), Click::Close);
    }

    #[test]
    fn a_window_that_ends_unsuccessfully_later_is_not_started_again() {
        let mut started = Started::start(
            vec![
                {
                    let mut c = Command::new("sh");
                    c.args(["-c", "sleep 0.2; exit 1", "scope"]);
                    c
                },
                Command::new("/nonexistent/direct"),
            ],
            &[],
            Duration::from_millis(50),
        )
        .unwrap();
        assert_eq!(started.poll(), Step::Running);
        std::thread::sleep(Duration::from_millis(400));
        // It ran past the grace period, so it was the window, not a failed
        // start, and nothing else is tried.
        assert_eq!(started.poll(), Step::Exited);
    }
}
