//! `/usr/libexec/better-os/session/start`: the process edges of the Better OS
//! session. Every rule is in the library (`session_launcher`); this file
//! starts processes, waits for them, talks to the bus, and passes events to
//! one loop.

use std::collections::{BTreeMap, HashMap};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use better_core::self_update::{
    PROGRESS_POLL, ProgramFile, ProgramWatch, RestartDecision, UpdateState, UpdateTracker, decide,
};
use session_launcher::callers::{self, COSMIC_EXIT_CALLERS, LOGOUT_CALLERS};
use session_launcher::children::{self, AfterExit, ChildSpec, Ending, Restarts};
use session_launcher::compositor;
use session_launcher::dconf;
use session_launcher::environment::{self, Import};
use session_launcher::lifecycle::{self, UserSession};
use session_launcher::process;
use session_launcher::record;
use session_launcher::resume::{self, Resume};
use zbus::fdo;
use zbus::message::Header;
use zbus::names::BusName;
use zbus::zvariant::OwnedObjectPath;

/// Everything the loop waits for.
enum Event {
    Signal(i32),
    Ready(Result<BTreeMap<String, String>, String>),
    /// A process the launcher started ended; `code` is its exit status, or
    /// 128 plus the signal that ended it.
    Exited {
        pid: i32,
        code: i32,
    },
    Logout(String),
    StartChild(&'static str),
    /// The program file moved; `true` when it is the poll timer's look.
    Update(bool),
}

fn log(message: impl std::fmt::Display) {
    eprintln!("better-os-session: {message}");
}

fn main() -> ExitCode {
    if let Some(argument) = std::env::args().nth(1) {
        return match argument.as_str() {
            "--version" => {
                println!("better-os-session {}", env!("CARGO_PKG_VERSION"));
                ExitCode::SUCCESS
            }
            "--help" => {
                println!(
                    "{}: the Better OS session, started by the display manager from \
                     /usr/share/wayland-sessions/better-os.desktop. It takes no arguments; \
                     --guard, --restore and --leftovers are its units' own modes.",
                    session_launcher::INSTALLED_PATH
                );
                ExitCode::SUCCESS
            }
            "--guard" => guard_mode(),
            "--restore" => restore_mode(),
            "--leftovers" => leftovers_mode(),
            other => {
                eprintln!(
                    "better-os-session takes no arguments (got {other}); the display manager \
                     starts it from /usr/share/wayland-sessions/better-os.desktop"
                );
                ExitCode::from(2)
            }
        };
    }
    // The state an earlier program of this session handed over, taken out of
    // the environment before anything can inherit it.
    let handed = std::env::var(resume::VARIABLE).ok();
    let mut inherited: BTreeMap<String, String> = std::env::vars()
        .filter(|(name, _)| name != resume::VARIABLE)
        .collect();
    // The stop signals go to one thread of their own; they are blocked here,
    // before any other thread exists, so every thread inherits the mask.
    let (sender, events) = mpsc::channel();
    if let Err(error) = take_signals(sender.clone()) {
        log(format!("the stop signals could not be handled: {error}"));
        return ExitCode::FAILURE;
    }
    let session = match handed.as_deref().map(Resume::decode) {
        Some(Some(state)) => {
            log("running the updated launcher in the same session");
            Session::adopt(state, inherited, sender.clone())
        }
        Some(None) => {
            log("the hand-over from the previous launcher is unreadable; ending the session");
            return ExitCode::FAILURE;
        }
        None => {
            let launch = environment::launch_environment(|name| inherited.get(name).cloned());
            for warning in &launch.warnings {
                log(warning);
            }
            let before_session = inherited.clone();
            for name in &launch.remove {
                inherited.remove(name);
            }
            inherited.extend(launch.set);
            if let Some(profile) = generated_dconf_profile(&before_session) {
                inherited.insert("DCONF_PROFILE".into(), profile);
            }
            match Session::start(inherited, before_session, sender.clone(), &events) {
                Ok(session) => session,
                Err(code) => return code,
            }
        }
    };
    session.run(sender, events)
}

fn take_signals(sender: Sender<Event>) -> std::io::Result<()> {
    // SAFETY: the set is initialized by sigemptyset before use, and the mask
    // is changed before any other thread is started.
    let set = unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, signal);
        }
        if libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        set
    };
    std::thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            loop {
                let mut received = 0;
                // SAFETY: `set` is a valid, initialized signal set.
                if unsafe { libc::sigwait(&set, &mut received) } == 0
                    && sender.send(Event::Signal(received)).is_err()
                {
                    return;
                }
            }
        })?;
    Ok(())
}

/// Waits for a child of this process from a thread of its own. It works for
/// a child an earlier program of this process started too.
fn watch_exit(pid: i32, sender: Sender<Event>) {
    let _ = std::thread::Builder::new()
        .name(format!("wait-{pid}"))
        .spawn(move || {
            let mut status = 0;
            loop {
                // SAFETY: waitpid writes only to `status`.
                let result = unsafe { libc::waitpid(pid, &mut status, 0) };
                if result == pid {
                    break;
                }
                if result == -1
                    && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR)
                {
                    status = 0;
                    break;
                }
            }
            let code = if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else if libc::WIFSIGNALED(status) {
                128 + libc::WTERMSIG(status)
            } else {
                0
            };
            let _ = sender.send(Event::Exited { pid, code });
        });
}

/// Starts a child. Only the main thread calls this: the parent-death signal
/// follows the thread that forks ([`process::with_parent_death_signal`]).
fn spawn(spec: &ChildSpec, environment: &BTreeMap<String, String>) -> std::io::Result<i32> {
    let mut command = Command::new(spec.program);
    process::with_parent_death_signal(&mut command, libc::SIGTERM);
    let child = process::with_signals_unblocked(&mut command)
        .args(spec.arguments)
        .env_clear()
        .envs(environment)
        .spawn()?;
    Ok(child.id() as i32)
}

fn signal(pid: i32, signal: i32) {
    // SAFETY: kill has no memory preconditions; the pid is a child of ours.
    unsafe {
        libc::kill(pid, signal);
    }
}

/// `XDG_RUNTIME_DIR` of a unit's mode, which the user manager sets.
fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
}

/// The session's dconf profile when the machine's `user` profile adds system
/// databases ([`dconf::session_profile`]): written under the runtime
/// directory, and its path is what `DCONF_PROFILE` names. `None` keeps the
/// shipped `betteros` profile.
fn generated_dconf_profile(before: &BTreeMap<String, String>) -> Option<String> {
    let probe = dconf::Probe::system(
        before.get("XDG_DATA_DIRS").map(String::as_str),
        before.get("XDG_RUNTIME_DIR").map(String::as_str),
    );
    let user = probe
        .named("user")
        .and_then(|path| std::fs::read_to_string(path).ok());
    let dconf::SessionProfile::Generated(text) = dconf::session_profile(user.as_deref()) else {
        return None;
    };
    let directory = probe.runtime_dir?.join(record::DIRECTORY);
    let path = directory.join(dconf::GENERATED_PROFILE);
    let written = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)
        .and_then(|()| std::fs::write(&path, text));
    match written {
        Ok(()) => Some(path.display().to_string()),
        Err(error) => {
            log(format!(
                "the session's dconf profile with this machine's system databases could not be \
                 written ({error}); the shipped one is used"
            ));
            None
        }
    }
}

/// Puts back what a session that ended without its launcher left: its
/// target, if it still runs, and its environment ([`record`]).
fn put_back_if_orphaned(bus: &Bus, runtime: &std::path::Path) {
    let Some(found) = record::read(runtime) else {
        return;
    };
    if record::leader_alive(&found, std::path::Path::new("/proc")) {
        return;
    }
    log(format!(
        "the session of launcher {} ended without it; putting back what it left",
        found.leader
    ));
    if bus.is_running(session_launcher::TARGET).unwrap_or(false)
        && let Err(error) = bus.stop_unit(session_launcher::TARGET)
    {
        log(format!(
            "stopping the left {}: {error}",
            session_launcher::TARGET
        ));
    }
    if let Some(taken) = record::take(runtime) {
        for problem in lifecycle::restore(bus, &taken.import, &taken.activation) {
            log(problem);
        }
    }
}

/// `better-os-session-guard.service`: lives as long as the launcher that
/// wrote the record, so the target, bound to it, stops when the launcher
/// ends however it ends.
fn guard_mode() -> ExitCode {
    let Some(runtime) = runtime_dir() else {
        log("the guard has no XDG_RUNTIME_DIR");
        return ExitCode::FAILURE;
    };
    let Some(found) = record::read(&runtime) else {
        log("the guard found no session record; the session's units stop");
        return ExitCode::FAILURE;
    };
    while record::leader_alive(&found, std::path::Path::new("/proc")) {
        std::thread::sleep(Duration::from_secs(1));
    }
    log(format!(
        "launcher {} has ended; the session's units stop",
        found.leader
    ));
    ExitCode::SUCCESS
}

/// The guard's `ExecStopPost=`: puts the environment back when the target
/// stops, however it stops.
fn restore_mode() -> ExitCode {
    let Some(runtime) = runtime_dir() else {
        return ExitCode::FAILURE;
    };
    let Some(taken) = record::take(&runtime) else {
        return ExitCode::SUCCESS;
    };
    let bus = match Bus::connect() {
        Ok(bus) => bus,
        Err(error) => {
            log(format!(
                "the session bus is not reachable to put the environment back: {error}"
            ));
            return ExitCode::FAILURE;
        }
    };
    for problem in lifecycle::restore(&bus, &taken.import, &taken.activation) {
        log(problem);
    }
    ExitCode::SUCCESS
}

/// `better-os-session-leftovers.service`, in every session of the person:
/// a record whose launcher is gone is put back.
fn leftovers_mode() -> ExitCode {
    let Some(runtime) = runtime_dir() else {
        return ExitCode::SUCCESS;
    };
    match Bus::connect() {
        Ok(bus) => {
            put_back_if_orphaned(&bus, &runtime);
            ExitCode::SUCCESS
        }
        Err(error) => {
            log(format!("the session bus is not reachable: {error}"));
            ExitCode::FAILURE
        }
    }
}

/// Why the session ends.
enum End {
    Logout,
    Signal,
    Compositor(i32),
}

struct Session {
    environment: BTreeMap<String, String>,
    compositor: i32,
    socket: Option<OwnedFd>,
    running: BTreeMap<&'static str, i32>,
    restarts: BTreeMap<&'static str, Restarts>,
    import: Import,
    bus: Option<Bus>,
    runtime_dir: Option<PathBuf>,
    /// Whether the record was written, so the units can put the
    /// environment back without this process ([`record`]).
    recorded: bool,
}

impl Session {
    fn start(
        mut environment: BTreeMap<String, String>,
        before_session: BTreeMap<String, String>,
        sender: Sender<Event>,
        events: &Receiver<Event>,
    ) -> Result<Session, ExitCode> {
        let (ours, theirs) = UnixStream::pair().map_err(|error| {
            log(format!("no socket pair for the compositor: {error}"));
            ExitCode::FAILURE
        })?;
        let theirs_fd: RawFd = theirs.as_raw_fd();
        let mut command = Command::new(session_launcher::COMPOSITOR);
        // The compositor ignores its session socket closing, so without this
        // it would outlive a launcher that died (ticket 203's review, P1).
        process::with_parent_death_signal(&mut command, libc::SIGTERM);
        process::with_signals_unblocked(&mut command)
            .env_clear()
            .envs(&environment)
            .env(compositor::SOCKET_VARIABLE, theirs_fd.to_string());
        // SAFETY: fcntl is async-signal-safe; the descriptor stays open in
        // the parent until spawn returns.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(theirs_fd, libc::F_GETFD);
                if flags == -1
                    || libc::fcntl(theirs_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let compositor_pid = match command.spawn() {
            Ok(child) => child.id() as i32,
            Err(error) => {
                log(format!(
                    "{} could not start: {error}",
                    session_launcher::COMPOSITOR
                ));
                return Err(ExitCode::FAILURE);
            }
        };
        drop(theirs);
        watch_exit(compositor_pid, sender.clone());
        let socket = OwnedFd::from(ours);
        let reader = socket.try_clone().map(UnixStream::from);
        let ready_sender = sender.clone();
        let _ = std::thread::Builder::new()
            .name("compositor".into())
            .spawn(move || {
                let result = match reader {
                    Ok(mut stream) => {
                        compositor::read_ready(&mut stream).map_err(|error| error.to_string())
                    }
                    Err(error) => Err(error.to_string()),
                };
                let _ = ready_sender.send(Event::Ready(result));
            });

        // Nothing else starts before the compositor is up.
        loop {
            match events.recv() {
                Ok(Event::Ready(Ok(displays))) => {
                    environment.extend(displays);
                    break;
                }
                Ok(Event::Ready(Err(error))) => {
                    log(format!("the compositor never said it was up: {error}"));
                    end_processes(&[compositor_pid], events);
                    return Err(ExitCode::FAILURE);
                }
                Ok(Event::Exited { pid, code }) if pid == compositor_pid => {
                    log(format!(
                        "the compositor ended before it was up (status {code})"
                    ));
                    return Err(ExitCode::from(code.clamp(1, 255) as u8));
                }
                Ok(Event::Signal(_)) => {
                    end_processes(&[compositor_pid], events);
                    return Err(ExitCode::SUCCESS);
                }
                Ok(_) => {}
                Err(_) => return Err(ExitCode::FAILURE),
            }
        }

        let bus = match zbus::blocking::Connection::session() {
            Ok(connection) => Some(Bus { connection }),
            Err(error) => {
                log(format!(
                    "the session bus is not reachable; no unit will start: {error}"
                ));
                None
            }
        };
        let runtime_dir = environment.get("XDG_RUNTIME_DIR").map(PathBuf::from);
        let mut recorded = false;
        let import = match &bus {
            Some(bus) => {
                // An earlier session that ended without its launcher is put
                // back before this one reads what is "before".
                if let Some(runtime) = &runtime_dir {
                    put_back_if_orphaned(bus, runtime);
                }
                let probe = dconf::Probe::system(
                    before_session.get("XDG_DATA_DIRS").map(String::as_str),
                    runtime_dir.as_deref().and_then(|dir| dir.to_str()),
                );
                let mut write_record = |import: &Import| -> Result<(), String> {
                    let runtime = runtime_dir
                        .as_ref()
                        .ok_or_else(|| "XDG_RUNTIME_DIR is not set".to_string())?;
                    let own = std::process::id() as i32;
                    let record = record::Record {
                        leader: own,
                        leader_start: record::start_time(std::path::Path::new("/proc"), own)
                            .ok_or_else(|| "this process's start time is unreadable".to_string())?,
                        import: import.clone(),
                        activation: environment::activation_restore(
                            import,
                            &dconf::default_profile(&probe),
                        ),
                    };
                    record::write(runtime, &record).map_err(|error| error.to_string())?;
                    recorded = true;
                    Ok(())
                };
                let (import, problems) = lifecycle::begin(bus, &environment, &mut write_record);
                for problem in problems {
                    log(problem);
                }
                import
            }
            None => Import::default(),
        };
        let mut session = Session {
            environment,
            compositor: compositor_pid,
            socket: Some(socket),
            running: BTreeMap::new(),
            restarts: BTreeMap::new(),
            import,
            bus,
            runtime_dir,
            recorded,
        };
        session.serve(&sender);
        for spec in children::CHILDREN {
            session.start_child(&spec, &sender);
        }
        Ok(session)
    }

    fn adopt(
        state: Resume,
        environment: BTreeMap<String, String>,
        sender: Sender<Event>,
    ) -> Session {
        watch_exit(state.compositor, sender.clone());
        let mut running = BTreeMap::new();
        for (name, pid) in &state.children {
            if let Some(spec) = children::by_name(name) {
                watch_exit(*pid, sender.clone());
                running.insert(spec.name, *pid);
            }
        }
        // SAFETY: the previous program kept this descriptor open across the
        // exec for this program, and nothing else owns it.
        let socket = state.socket.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) });
        // The previous program cleared FD_CLOEXEC for the exec; a locker
        // started from now on must not inherit the compositor's socket.
        if let Some(fd) = &socket
            && let Err(error) = process::set_close_on_exec(fd.as_raw_fd(), true)
        {
            log(format!(
                "the compositor's socket stays open to children: {error}"
            ));
        }
        let bus = zbus::blocking::Connection::session()
            .map(|connection| Bus { connection })
            .map_err(|error| log(format!("the session bus is not reachable: {error}")))
            .ok();
        let runtime_dir = environment.get("XDG_RUNTIME_DIR").map(PathBuf::from);
        let mut session = Session {
            environment,
            compositor: state.compositor,
            socket,
            running,
            restarts: BTreeMap::new(),
            import: state.import,
            bus,
            runtime_dir,
            recorded: state.recorded,
        };
        session.serve(&sender);
        for spec in children::CHILDREN {
            if !session.running.contains_key(spec.name) {
                session.start_child(&spec, &sender);
            }
        }
        session
    }

    fn serve(&self, sender: &Sender<Event>) {
        let Some(bus) = &self.bus else { return };
        // SAFETY: getuid cannot fail.
        let own_uid = unsafe { libc::getuid() };
        let server = bus.connection.object_server();
        let served = server
            .at(
                callers::SESSION_PATH,
                SessionInterface {
                    sender: sender.clone(),
                    own_uid,
                },
            )
            .and_then(|_| {
                server.at(
                    callers::COSMIC_SESSION_PATH,
                    CosmicSessionInterface {
                        sender: sender.clone(),
                        own_uid,
                    },
                )
            });
        if let Err(error) = served {
            log(format!(
                "the session's interfaces could not be served: {error}"
            ));
            return;
        }
        for name in [callers::SESSION_NAME, callers::COSMIC_SESSION_NAME] {
            if let Err(error) = bus
                .connection
                .request_name_with_flags(name, zbus::fdo::RequestNameFlags::DoNotQueue.into())
            {
                log(format!("{name} could not be owned: {error}"));
            }
        }
    }

    fn start_child(&mut self, spec: &ChildSpec, sender: &Sender<Event>) {
        match spawn(spec, &self.environment) {
            Ok(pid) => {
                watch_exit(pid, sender.clone());
                self.running.insert(spec.name, pid);
            }
            Err(error) => log(format!(
                "the {} ({}) could not start: {error}",
                spec.name, spec.program
            )),
        }
    }

    fn run(mut self, sender: Sender<Event>, events: Receiver<Event>) -> ExitCode {
        let mut update = UpdateWatch::start(&sender);
        // Events taken off the channel before an exec that did not happen.
        let mut backlog: std::collections::VecDeque<Event> = Default::default();
        let end = loop {
            let Some(event) = backlog.pop_front().or_else(|| events.recv().ok()) else {
                break End::Signal;
            };
            match event {
                Event::Signal(number) => {
                    log(format!("signal {number}: ending the session"));
                    break End::Signal;
                }
                Event::Logout(who) => {
                    log(format!("{who} asked to log out"));
                    break End::Logout;
                }
                Event::Exited { pid, code } if pid == self.compositor => {
                    log(format!("the compositor ended (status {code})"));
                    break End::Compositor(code);
                }
                Event::Exited { pid, code } => self.child_ended(pid, code, &sender),
                Event::StartChild(name) => {
                    if let Some(spec) = children::by_name(name)
                        && !self.running.contains_key(name)
                    {
                        self.start_child(&spec, &sender);
                    }
                }
                Event::Update(timer) => {
                    if let Some(watch) = update.as_mut()
                        && watch.ready(timer, &sender)
                    {
                        // Whatever is already queued is looked at first: an
                        // ending the exec would drop ends the session
                        // instead, and a child that ended is let go so the
                        // new program starts it again.
                        let waiting: Vec<Event> = events.try_iter().collect();
                        let pending: Vec<resume::Pending> =
                            waiting.iter().map(|event| self.pending(event)).collect();
                        if resume::may_restart(&pending) {
                            for event in waiting {
                                if let Event::Exited { pid, .. } = event
                                    && let Some((&name, _)) =
                                        self.running.iter().find(|(_, running)| **running == pid)
                                {
                                    self.running.remove(name);
                                }
                            }
                            self.restart_in_place(watch.program());
                        } else {
                            backlog.extend(waiting);
                        }
                    }
                }
                Event::Ready(_) => {}
            }
        };
        drop(update);
        self.end(end, &events)
    }

    fn pending(&self, event: &Event) -> resume::Pending {
        match event {
            Event::Signal(_) | Event::Logout(_) => resume::Pending::Ending,
            Event::Exited { pid, .. } if *pid == self.compositor => resume::Pending::Ending,
            Event::Exited { .. } => resume::Pending::ChildEnded,
            _ => resume::Pending::Other,
        }
    }

    fn child_ended(&mut self, pid: i32, code: i32, sender: &Sender<Event>) {
        let Some((&name, _)) = self.running.iter().find(|(_, running)| **running == pid) else {
            return;
        };
        self.running.remove(name);
        match self.restarts.entry(name).or_default().ended(Instant::now()) {
            AfterExit::Restart(delay) => {
                log(format!(
                    "the {name} ended (status {code}); starting it again"
                ));
                let sender = sender.clone();
                let _ = std::thread::Builder::new()
                    .name("restart".into())
                    .spawn(move || {
                        std::thread::sleep(delay);
                        let _ = sender.send(Event::StartChild(name));
                    });
            }
            AfterExit::GiveUp => log(format!(
                "the {name} ended {} times in {} s (status {code}); it stays ended",
                children::RESTART_LIMIT + 1,
                children::RESTART_WINDOW.as_secs()
            )),
        }
    }

    /// Starts the updated launcher in this process's place. It returns only
    /// when that failed, and the session then goes on with this program.
    fn restart_in_place(&self, program: PathBuf) {
        // The socket crosses the exec; it is the only descriptor that does.
        let socket = self
            .socket
            .as_ref()
            .map(|fd| fd.as_raw_fd())
            .filter(|fd| process::set_close_on_exec(*fd, false).is_ok());
        let state = Resume {
            compositor: self.compositor,
            socket,
            children: self
                .running
                .iter()
                .map(|(name, pid)| (name.to_string(), *pid))
                .collect(),
            import: self.import.clone(),
            recorded: self.recorded,
        };
        log("the launcher was updated; starting the new program in its place");
        // Called from the main thread, so the children's parent-death signal,
        // which follows this thread, carries over to the new program.
        let error = Command::new(&program)
            .env_clear()
            .envs(&self.environment)
            .env(resume::VARIABLE, state.encode())
            .exec();
        log(format!(
            "the updated launcher could not start ({error}); this one goes on"
        ));
        if let Some(fd) = socket {
            let _ = process::set_close_on_exec(fd, true);
        }
    }

    fn end(self, end: End, events: &Receiver<Event>) -> ExitCode {
        if let Some(bus) = &self.bus {
            for problem in lifecycle::stop_units(bus, &std::thread::sleep) {
                log(problem);
            }
        }
        // The units have stopped; the children and the compositor end together.
        let mut pids: Vec<i32> = children::CHILDREN
            .iter()
            .rev()
            .filter_map(|spec| self.running.get(spec.name).copied())
            .collect();
        if !matches!(end, End::Compositor(_)) {
            pids.push(self.compositor);
        }
        end_processes(&pids, events);
        if let Some(bus) = &self.bus {
            // The guard unit's ExecStopPost normally took the record and put
            // the environment back as the target stopped; whatever is left
            // is done here. Without a record (it could not be written) the
            // import this process holds is put back.
            let problems = match self.runtime_dir.as_deref().and_then(record::take) {
                Some(record) => lifecycle::restore(bus, &record.import, &record.activation),
                None if !self.recorded => {
                    let probe = dconf::Probe::system(
                        None,
                        self.runtime_dir.as_deref().and_then(|dir| dir.to_str()),
                    );
                    let activation = environment::activation_restore(
                        &self.import,
                        &dconf::default_profile(&probe),
                    );
                    lifecycle::restore(bus, &self.import, &activation)
                }
                None => Vec::new(),
            };
            for problem in problems {
                log(problem);
            }
        }
        match end {
            End::Logout | End::Signal => ExitCode::SUCCESS,
            End::Compositor(0) => ExitCode::SUCCESS,
            End::Compositor(code) => ExitCode::from(code.clamp(1, 255) as u8),
        }
    }
}

/// Ends the processes together: SIGTERM to all of them, then SIGKILL to any
/// still there after the patience. Together and not one after another, so a
/// logout waits the patience once at most: in ticket 203's VM both the
/// locker and the compositor outlived SIGTERM, and one after the other they
/// held the logout for ten seconds.
fn end_processes(pids: &[i32], events: &Receiver<Event>) {
    let mut left: Vec<i32> = pids.to_vec();
    let mut terminated = None;
    while !left.is_empty() {
        match children::ending(terminated, Instant::now()) {
            Ending::Terminate => {
                for &pid in &left {
                    signal(pid, libc::SIGTERM);
                }
                terminated = Some(Instant::now());
            }
            Ending::Kill => {
                for &pid in &left {
                    log(format!("process {pid} outlived SIGTERM; killing it"));
                    signal(pid, libc::SIGKILL);
                }
                // A killed process is gone at once; its waiter says so.
                let deadline = Instant::now() + Duration::from_secs(2);
                while !left.is_empty() && Instant::now() < deadline {
                    match events.recv_timeout(Duration::from_millis(100)) {
                        Ok(Event::Exited { pid, .. }) => left.retain(|&running| running != pid),
                        Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                return;
            }
            Ending::Wait => match events.recv_timeout(Duration::from_millis(100)) {
                Ok(Event::Exited { pid, .. }) => left.retain(|&running| running != pid),
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            },
        }
    }
}

/// The launcher's own program file, watched for an update.
struct UpdateWatch {
    tracker: UpdateTracker,
    _watch: Option<ProgramWatch>,
    polling: bool,
}

impl UpdateWatch {
    fn start(sender: &Sender<Event>) -> Option<UpdateWatch> {
        let program = ProgramFile::current()
            .map_err(|error| {
                log(format!(
                    "the launcher's own program cannot be watched: {error}"
                ))
            })
            .ok()?;
        let wake = sender.clone();
        let watch = ProgramWatch::start(program.path(), move || {
            let _ = wake.send(Event::Update(false));
        })
        .map_err(|error| log(format!("no watch on the launcher's program: {error}")))
        .ok();
        Some(UpdateWatch {
            tracker: UpdateTracker::new(program, Some(session_launcher::PACKAGE)),
            _watch: watch,
            polling: false,
        })
    }

    fn program(&self) -> PathBuf {
        self.tracker.program().path().to_path_buf()
    }

    /// Whether the new program is ready to take over. While an update is
    /// under way it looks again every [`PROGRESS_POLL`]; `timer` says this
    /// look is that timer's.
    fn ready(&mut self, timer: bool, sender: &Sender<Event>) -> bool {
        if timer {
            self.polling = false;
        }
        match self.tracker.poll(Instant::now()) {
            UpdateState::InProgress => {
                if !self.polling {
                    self.polling = true;
                    let sender = sender.clone();
                    let _ = std::thread::Builder::new()
                        .name("update-poll".into())
                        .spawn(move || {
                            std::thread::sleep(PROGRESS_POLL);
                            let _ = sender.send(Event::Update(true));
                        });
                }
                false
            }
            // Nothing the launcher does cannot be carried over the exec.
            state => decide(&state, false) == RestartDecision::Restart,
        }
    }
}

/// The user manager and the session bus over zbus.
struct Bus {
    connection: zbus::blocking::Connection,
}

impl Bus {
    fn connect() -> zbus::Result<Bus> {
        zbus::blocking::Connection::session().map(|connection| Bus { connection })
    }

    fn manager(&self) -> Result<zbus::blocking::Proxy<'static>, String> {
        zbus::blocking::Proxy::new(
            &self.connection,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .map_err(|error| error.to_string())
    }
}

impl UserSession for Bus {
    fn environment(&self) -> Result<Vec<String>, String> {
        self.manager()?
            .get_property::<Vec<String>>("Environment")
            .map_err(|error| error.to_string())
    }

    fn set_environment(&self, assignments: &[String]) -> Result<(), String> {
        self.manager()?
            .call::<_, _, ()>("SetEnvironment", &(assignments,))
            .map_err(|error| error.to_string())
    }

    fn unset_environment(&self, names: &[String]) -> Result<(), String> {
        self.manager()?
            .call::<_, _, ()>("UnsetEnvironment", &(names,))
            .map_err(|error| error.to_string())
    }

    fn update_activation_environment(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let map: HashMap<&str, &str> = values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        zbus::blocking::fdo::DBusProxy::new(&self.connection)
            .and_then(|proxy| proxy.update_activation_environment(map).map_err(Into::into))
            .map_err(|error| error.to_string())
    }

    fn start_unit(&self, unit: &str) -> Result<(), String> {
        self.manager()?
            .call::<_, _, OwnedObjectPath>("StartUnit", &(unit, "replace"))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn stop_unit(&self, unit: &str) -> Result<(), String> {
        self.manager()?
            .call::<_, _, OwnedObjectPath>("StopUnit", &(unit, "replace"))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn is_running(&self, unit: &str) -> Result<bool, String> {
        let path = match self
            .manager()?
            .call::<_, _, OwnedObjectPath>("GetUnit", &(unit,))
        {
            Ok(path) => path,
            Err(zbus::Error::MethodError(name, _, _)) if name.as_str().ends_with("NoSuchUnit") => {
                return Ok(false);
            }
            Err(error) => return Err(error.to_string()),
        };
        let state: String = zbus::blocking::Proxy::new(
            &self.connection,
            "org.freedesktop.systemd1",
            path,
            "org.freedesktop.systemd1.Unit",
        )
        .and_then(|unit| unit.get_property("ActiveState"))
        .map_err(|error| error.to_string())?;
        Ok(matches!(
            state.as_str(),
            "active" | "activating" | "deactivating" | "reloading"
        ))
    }
}

/// The caller's program, checked against `allowed`.
async fn authorize(
    connection: &zbus::Connection,
    header: &Header<'_>,
    own_uid: u32,
    allowed: &[&str],
) -> fdo::Result<String> {
    let sender = header
        .sender()
        .ok_or_else(|| fdo::Error::AccessDenied("the call has no sender".into()))?;
    let dbus = fdo::DBusProxy::new(connection).await?;
    let name = BusName::Unique(sender.to_owned());
    let uid = dbus.get_connection_unix_user(name.clone()).await?;
    let pid = dbus.get_connection_unix_process_id(name).await?;
    let exe = PathBuf::from(format!("/proc/{pid}/exe"));
    let caller = std::fs::read_link(&exe)
        .ok()
        .zip(callers::file_id(&exe))
        .map(|(link, file)| callers::Caller { link, file });
    callers::check(uid, own_uid, caller.as_ref(), allowed, callers::file_id).map_err(
        |refusal| {
            log(format!("refused a logout: {refusal}"));
            fdo::Error::AccessDenied(refusal.to_string())
        },
    )?;
    Ok(caller
        .map(|caller| callers::running_program(&caller.link).display().to_string())
        .unwrap_or_default())
}

struct SessionInterface {
    sender: Sender<Event>,
    own_uid: u32,
}

#[zbus::interface(name = "org.betteros.Session1")]
impl SessionInterface {
    /// Ends the session: its units stop, the compositor ends, and the display
    /// manager shows the login screen.
    async fn logout(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<()> {
        let who = authorize(connection, &header, self.own_uid, LOGOUT_CALLERS).await?;
        self.sender
            .send(Event::Logout(who))
            .map_err(|_| fdo::Error::Failed("the session is already ending".into()))
    }
}

struct CosmicSessionInterface {
    sender: Sender<Event>,
    own_uid: u32,
}

#[zbus::interface(name = "com.system76.CosmicSession")]
impl CosmicSessionInterface {
    /// What `cosmic-osd log-out` calls after its confirmation.
    async fn exit(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<()> {
        let who = authorize(connection, &header, self.own_uid, COSMIC_EXIT_CALLERS).await?;
        self.sender
            .send(Event::Logout(who))
            .map_err(|_| fdo::Error::Failed("the session is already ending".into()))
    }
}
