//! Keeps the frame images and the desktop settings current for every window
//! in the process.
//!
//! One thread owns GTK (see [`super::gtk`]) and draws what a window asks for;
//! it sleeps in GLib's own poll, so it wakes only for GTK's display
//! connection, a settings change GTK watches, or a request. Another thread
//! reads the desktop's settings once, and a third follows `gsettings monitor`
//! on the two schemas the frame reads, re-reading them when one of its keys
//! changes. Each change, and each finished drawing, bumps
//! [`FrameService::generation`] and wakes every [`FrameService::subscribe`]r.
//! Nothing here writes a setting.
//!
//! The settings reader and the renderer are passed in
//! ([`FrameService::start`]), so the service is tested with fixed settings
//! and a fake renderer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::BufRead as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::{DesktopSettings, FallbackReason, FrameKey, GSettingsListing};

#[cfg(target_os = "linux")]
pub use super::gtk::Rendered;

/// What GTK drew, or nothing on a platform without it.
#[cfg(not(target_os = "linux"))]
#[derive(Clone, Debug)]
pub struct Rendered;

/// Something that draws the frame: GTK, or a fake in tests. It lives on the
/// render thread.
pub trait FrameRenderer {
    fn render(&mut self, key: &FrameKey) -> Rendered;
    /// Sleeps until there is work: `wake` readable, or the renderer's own
    /// event sources.
    fn wait(&mut self, wake: i32);
    /// The desktop theme changed; let the renderer catch up before the next
    /// drawing.
    fn theme_changed(&mut self) {}
}

/// Opens the renderer on the render thread.
pub type RendererFactory =
    Box<dyn FnOnce() -> Result<Box<dyn FrameRenderer>, FallbackReason> + Send>;

/// Reads the desktop's settings; called once, on its own thread.
pub type SettingsReader = Box<dyn FnOnce() -> DesktopSettings + Send>;

enum Request {
    /// A drawing, for the theme generation it was asked under.
    Render(FrameKey, u64),
    ThemeChanged,
}

#[derive(Default)]
struct State {
    settings: DesktopSettings,
    /// Whether `settings` has been read from the desktop yet.
    settings_ready: bool,
    renders: HashMap<FrameKey, Arc<Rendered>>,
    pending: HashSet<FrameKey>,
    /// Bumped by every theme change; a drawing asked for under an older one
    /// is thrown away when it arrives.
    theme_generation: u64,
    /// Theme, icon theme, and scale groups whose siblings were queued.
    prequeued: HashSet<(Option<String>, Option<String>, u32)>,
    fallback: Option<FallbackReason>,
}

/// Wakes the render thread: a pipe it polls next to GTK's own descriptors.
struct Wake {
    read: i32,
    write: i32,
}

impl Wake {
    fn new() -> Option<Wake> {
        let mut fds = [0; 2];
        let made = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) };
        (made == 0).then_some(Wake {
            read: fds[0],
            write: fds[1],
        })
    }

    fn ring(&self) {
        let byte = 1u8;
        unsafe { libc::write(self.write, (&byte as *const u8).cast(), 1) };
    }

    fn drain(&self) {
        let mut buffer = [0u8; 64];
        while unsafe { libc::read(self.read, buffer.as_mut_ptr().cast(), buffer.len()) } > 0 {}
    }
}

impl Drop for Wake {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read);
            libc::close(self.write);
        }
    }
}

/// The process-wide frame state.
pub struct FrameService {
    state: Mutex<State>,
    changed: Condvar,
    generation: AtomicU64,
    requests: Mutex<VecDeque<Request>>,
    wake: Option<Wake>,
    subscribers: Mutex<Vec<smol::channel::Sender<()>>>,
}

static SERVICE: OnceLock<Arc<FrameService>> = OnceLock::new();

impl FrameService {
    /// The service, started on first use with the desktop's settings and GTK,
    /// and following settings changes.
    pub fn global() -> &'static Arc<FrameService> {
        SERVICE.get_or_init(|| {
            let forced =
                super::fallback_requested(std::env::var(super::FORCE_FALLBACK_VAR).ok().as_deref());
            let renderer = (!forced).then(gtk_factory);
            let service = FrameService::start(
                Box::new(|| DesktopSettings::read(&GSettingsListing::read())),
                renderer,
            );
            if forced {
                service.give_up(FallbackReason::Requested);
            }
            let watcher = service.clone();
            let _ = std::thread::Builder::new()
                .name("better-ui-desktop-settings".into())
                .spawn(move || watch_settings(watcher));
            service
        })
    }

    /// A service reading settings with `read` (once, on its own thread) and
    /// drawing with `renderer`, or always falling back without one. It does
    /// not follow settings changes; [`FrameService::global`] adds that.
    pub fn start(read: SettingsReader, renderer: Option<RendererFactory>) -> Arc<FrameService> {
        let service = Arc::new(FrameService {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            generation: AtomicU64::new(0),
            requests: Mutex::new(VecDeque::new()),
            wake: Wake::new(),
            subscribers: Mutex::new(Vec::new()),
        });
        let reader = service.clone();
        if std::thread::Builder::new()
            .name("better-ui-desktop-read".into())
            .spawn(move || reader.settings_read(read()))
            .is_err()
        {
            service.settings_read(DesktopSettings::default());
        }
        match renderer {
            Some(factory) if service.wake.is_some() => {
                let runner = service.clone();
                if std::thread::Builder::new()
                    .name("better-ui-gtk-frame".into())
                    .spawn(move || render_thread(runner, factory))
                    .is_err()
                {
                    service.give_up(FallbackReason::RendererStopped);
                }
            }
            Some(_) => service.give_up(FallbackReason::RendererStopped),
            None => {}
        }
        service
    }

    /// Starts the service early, so GTK is ready by the first frame.
    pub fn warm_up() {
        let _ = Self::global();
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Bumped on every settings change and every finished drawing.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// A channel that receives a message after every change. A window holds
    /// one and repaints when it fires; dropping it unsubscribes.
    pub fn subscribe(&self) -> smol::channel::Receiver<()> {
        let (send, receive) = smol::channel::bounded(1);
        self.subscribers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(send);
        receive
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_all();
        let mut subscribers = self.subscribers.lock().unwrap_or_else(|p| p.into_inner());
        subscribers.retain(|s| !s.is_closed());
        for subscriber in subscribers.iter() {
            let _ = subscriber.try_send(());
        }
    }

    pub fn settings(&self) -> DesktopSettings {
        self.lock().settings.clone()
    }

    /// The settings, waiting until `deadline` for the first read; GNOME's
    /// defaults if it has not finished by then.
    pub fn settings_by(&self, deadline: Instant) -> DesktopSettings {
        let mut state = self.lock();
        while !state.settings_ready {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        state.settings.clone()
    }

    /// Why GTK is not drawing the frame, once that is known.
    pub fn fallback(&self) -> Option<FallbackReason> {
        self.lock().fallback.clone()
    }

    /// Whether GTK is known to be unavailable, so the fallback is final.
    pub fn gave_up(&self) -> bool {
        self.lock().fallback.is_some()
    }

    fn send(&self, request: Request) {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(request);
        if let Some(wake) = &self.wake {
            wake.ring();
        }
    }

    fn request(&self, state: &mut State, key: &FrameKey) {
        if state.fallback.is_none() && state.pending.insert(key.clone()) {
            self.send(Request::Render(key.clone(), state.theme_generation));
        }
    }

    /// The images for `key`, asking for them when they are not drawn yet.
    pub fn get(&self, key: &FrameKey) -> Option<Arc<Rendered>> {
        let mut state = self.lock();
        if let Some(found) = state.renders.get(key) {
            return Some(found.clone());
        }
        self.request(&mut state, key);
        None
    }

    /// Like [`get`](Self::get), but waits until `deadline` for the drawing,
    /// so a window's first frame is the system's frame and not the fallback.
    /// Returns at once when GTK has given up.
    pub fn get_by(&self, key: &FrameKey, deadline: Instant) -> Option<Arc<Rendered>> {
        let mut state = self.lock();
        loop {
            if let Some(found) = state.renders.get(key) {
                return Some(found.clone());
            }
            self.request(&mut state, key);
            if state.fallback.is_some() {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    fn store(&self, key: FrameKey, theme_generation: u64, rendered: Rendered) {
        let mut state = self.lock();
        state.pending.remove(&key);
        if theme_generation != state.theme_generation {
            // Drawn with the theme that was just replaced.
            return;
        }
        let group = (key.gtk_theme.clone(), key.icon_theme.clone(), key.scale);
        if state.prequeued.insert(group) {
            for sibling in key.siblings() {
                if !state.renders.contains_key(&sibling) {
                    self.request(&mut state, &sibling);
                }
            }
        }
        state.renders.insert(key, Arc::new(rendered));
        drop(state);
        self.bump();
    }

    fn give_up(&self, reason: FallbackReason) {
        let mut state = self.lock();
        if state.fallback.is_none() {
            eprintln!("better-ui: drawing the built-in Adwaita frame: {reason:?}");
            state.fallback = Some(reason);
        }
        state.pending.clear();
        drop(state);
        self.bump();
    }

    fn settings_read(&self, settings: DesktopSettings) {
        let mut state = self.lock();
        if state.settings_ready {
            drop(state);
            self.settings_changed(settings);
            return;
        }
        state.settings_ready = true;
        state.settings = settings;
        drop(state);
        self.bump();
    }

    /// Takes newly read settings. A new theme or icon theme throws every
    /// drawing away (the theme's files may draw differently under the same
    /// name, and libadwaita follows the desktop on its own); a new colour
    /// scheme does not, because each drawing is already for light or dark.
    pub fn settings_changed(&self, settings: DesktopSettings) {
        let mut state = self.lock();
        state.settings_ready = true;
        if state.settings == settings {
            return;
        }
        let theme_changed = state.settings.gtk_theme != settings.gtk_theme
            || state.settings.icon_theme != settings.icon_theme;
        state.settings = settings;
        if theme_changed {
            state.renders.clear();
            state.pending.clear();
            state.prequeued.clear();
            state.theme_generation += 1;
            if state.fallback.is_none() {
                self.send(Request::ThemeChanged);
            }
        }
        drop(state);
        self.bump();
    }

    fn take_requests(&self) -> Vec<Request> {
        if let Some(wake) = &self.wake {
            wake.drain();
        }
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain(..)
            .collect()
    }

    fn has_requests(&self) -> bool {
        !self
            .requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
    }
}

#[cfg(target_os = "linux")]
fn gtk_factory() -> RendererFactory {
    struct Gtk(super::gtk::Renderer);
    impl FrameRenderer for Gtk {
        fn render(&mut self, key: &FrameKey) -> Rendered {
            self.0.render(key)
        }
        fn wait(&mut self, wake: i32) {
            self.0.wait(wake);
        }
        fn theme_changed(&mut self) {
            // libadwaita watches the same settings in this thread's main
            // context; give it a moment to load the new theme.
            let until = Instant::now() + Duration::from_millis(250);
            while Instant::now() < until {
                self.0.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    Box::new(|| super::gtk::Renderer::open().map(|r| Box::new(Gtk(r)) as Box<dyn FrameRenderer>))
}

#[cfg(not(target_os = "linux"))]
fn gtk_factory() -> RendererFactory {
    Box::new(|| Err(FallbackReason::Unsupported))
}

/// Gives up when the render thread ends for any reason, a panic included,
/// so no window waits for a drawing that will never come.
struct GiveUpOnExit(Arc<FrameService>);

impl Drop for GiveUpOnExit {
    fn drop(&mut self) {
        self.0.give_up(FallbackReason::RendererStopped);
    }
}

fn render_thread(service: Arc<FrameService>, factory: RendererFactory) {
    let _guard = GiveUpOnExit(service.clone());
    let mut renderer = match factory() {
        Ok(renderer) => renderer,
        Err(reason) => {
            service.give_up(reason);
            return;
        }
    };
    let Some(wake) = service.wake.as_ref().map(|w| w.read) else {
        return;
    };
    loop {
        for request in service.take_requests() {
            match request {
                Request::Render(key, generation) => {
                    let current = service.lock().theme_generation;
                    if generation != current {
                        service.lock().pending.remove(&key);
                        continue;
                    }
                    let rendered = renderer.render(&key);
                    service.store(key, generation, rendered);
                }
                Request::ThemeChanged => renderer.theme_changed(),
            }
        }
        if !service.has_requests() {
            renderer.wait(wake);
        }
    }
}

/// How often a `gsettings monitor` that exits is started again, and how much
/// longer to wait before each attempt.
const MONITOR_RESTARTS: u32 = 5;
const MONITOR_RESTART_DELAY: Duration = Duration::from_secs(2);

/// Follows both schemas with `gsettings monitor` and re-reads the settings
/// when a key the frame uses changes. A monitor that exits is started again,
/// a few times, after a growing pause. Without `gsettings` the settings stay
/// as first read.
fn watch_settings(service: Arc<FrameService>) {
    let (send, receive) = std::sync::mpsc::channel::<()>();
    for schema in [super::INTERFACE_SCHEMA, super::WM_SCHEMA] {
        let send = send.clone();
        let _ = std::thread::Builder::new()
            .name("better-ui-gsettings-monitor".into())
            .spawn(move || {
                for attempt in 0..=MONITOR_RESTARTS {
                    if attempt > 0 {
                        std::thread::sleep(MONITOR_RESTART_DELAY * attempt);
                        // Anything may have changed while nobody watched.
                        if send.send(()).is_err() {
                            return;
                        }
                    }
                    if !monitor(schema, &send) {
                        return;
                    }
                }
            });
    }
    drop(send);
    while receive.recv().is_ok() {
        // A theme switch changes several keys at once; read them together.
        std::thread::sleep(Duration::from_millis(100));
        while receive.try_recv().is_ok() {}
        service.settings_changed(DesktopSettings::read(&GSettingsListing::read()));
    }
}

/// Runs one `gsettings monitor` until it exits. Returns whether to start it
/// again (false once nobody listens).
fn monitor(schema: &str, send: &std::sync::mpsc::Sender<()>) -> bool {
    let mut command = std::process::Command::new("gsettings");
    command
        .args(["monitor", schema])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt as _;
        // The monitor ends with this process rather than outliving it.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    let Ok(mut child) = command.spawn() else {
        return true;
    };
    if let Some(stdout) = child.stdout.take() {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if super::monitored_key(&line).is_some() && send.send(()).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
    let _ = child.wait();
    true
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::{FrameRenderer, FrameService, Rendered};
    use crate::system_frame::{
        ColorScheme, DesktopSettings, FallbackReason, FrameKey, Tiled, WindowState,
    };

    /// Draws placeholders, counting them; with `hold` it waits for a signal
    /// before each drawing, like a slow GTK.
    struct Fake {
        drawn: Arc<AtomicUsize>,
        hold: Option<std::sync::mpsc::Receiver<()>>,
    }

    impl FrameRenderer for Fake {
        fn render(&mut self, key: &FrameKey) -> Rendered {
            if let Some(hold) = &self.hold {
                let _ = hold.recv_timeout(Duration::from_secs(5));
            }
            self.drawn.fetch_add(1, Ordering::SeqCst);
            Rendered::placeholder(key.clone())
        }
        fn wait(&mut self, wake: i32) {
            let mut fd = libc::pollfd {
                fd: wake,
                events: libc::POLLIN,
                revents: 0,
            };
            unsafe { libc::poll(&mut fd, 1, 1000) };
        }
    }

    fn settings(theme: &str) -> DesktopSettings {
        DesktopSettings {
            gtk_theme: Some(theme.to_string()),
            ..DesktopSettings::default()
        }
    }

    fn key(settings: &DesktopSettings, dark: bool) -> FrameKey {
        WindowState::new(Some(Tiled::default()), false, false, true).key(settings, dark, 1.0)
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    fn fake(hold: Option<std::sync::mpsc::Receiver<()>>) -> (Arc<FrameService>, Arc<AtomicUsize>) {
        let drawn = Arc::new(AtomicUsize::new(0));
        let counter = drawn.clone();
        let service = FrameService::start(
            Box::new(|| settings("Adwaita")),
            Some(Box::new(move || {
                Ok(Box::new(Fake {
                    drawn: counter,
                    hold,
                }) as Box<dyn FrameRenderer>)
            })),
        );
        (service, drawn)
    }

    #[test]
    fn a_window_gets_the_drawing_and_its_siblings_are_drawn_next() {
        let (service, drawn) = fake(None);
        let s = service.settings_by(soon());
        assert_eq!(s.gtk_theme.as_deref(), Some("Adwaita"));
        let before = service.generation();
        let key = key(&s, true);
        let rendered = service.get_by(&key, soon()).expect("drawn");
        assert_eq!(rendered.key, key);
        assert!(service.generation() > before);
        // Maximized, tiled, and the other appearance follow without asking.
        for sibling in key.siblings() {
            assert!(service.get_by(&sibling, soon()).is_some(), "{sibling:?}");
        }
        assert_eq!(drawn.load(Ordering::SeqCst), 1 + key.siblings().len());
    }

    #[test]
    fn waiting_ends_at_the_deadline_while_gtk_is_slow() {
        let (release, hold) = std::sync::mpsc::channel();
        let (service, _) = fake(Some(hold));
        let s = service.settings_by(soon());
        let started = Instant::now();
        let missing = service.get_by(&key(&s, false), Instant::now() + Duration::from_millis(150));
        assert!(missing.is_none());
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert!(started.elapsed() < Duration::from_secs(2));
        release.send(()).unwrap();
    }

    #[test]
    fn when_gtk_cannot_open_windows_do_not_wait() {
        let failing = FrameService::start(
            Box::new(DesktopSettings::default),
            Some(Box::new(|| Err(FallbackReason::GtkInitFailed))),
        );
        let deadline = soon();
        while !failing.gave_up() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        assert!(
            failing
                .get_by(&key(&DesktopSettings::default(), false), deadline)
                .is_none()
        );
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(failing.fallback(), Some(FallbackReason::GtkInitFailed));
    }

    #[test]
    fn a_renderer_that_panics_gives_up_instead_of_leaving_windows_waiting() {
        struct Panics;
        impl FrameRenderer for Panics {
            fn render(&mut self, _: &FrameKey) -> Rendered {
                panic!("GTK went away")
            }
            fn wait(&mut self, _: i32) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let service = FrameService::start(
            Box::new(DesktopSettings::default),
            Some(Box::new(|| Ok(Box::new(Panics) as Box<dyn FrameRenderer>))),
        );
        let deadline = soon();
        assert!(
            service
                .get_by(&key(&DesktopSettings::default(), false), deadline)
                .is_none()
        );
        assert!(Instant::now() < deadline);
        assert_eq!(service.fallback(), Some(FallbackReason::RendererStopped));
    }

    #[test]
    fn without_a_renderer_nothing_is_drawn() {
        let service = FrameService::start(Box::new(DesktopSettings::default), None);
        assert!(
            service
                .get(&key(&DesktopSettings::default(), false))
                .is_none()
        );
        assert!(!service.gave_up(), "only GTK failing counts as giving up");
    }

    #[test]
    fn a_new_theme_drops_the_old_drawings_but_a_new_scheme_does_not() {
        let (service, _) = fake(None);
        let s = service.settings_by(soon());
        let old = key(&s, false);
        assert!(service.get_by(&old, soon()).is_some());
        let before = service.generation();
        let mut dark = s.clone();
        dark.color_scheme = ColorScheme::PreferDark;
        service.settings_changed(dark.clone());
        assert!(service.generation() > before, "a scheme change repaints");
        assert!(service.get(&old).is_some(), "the drawing for light is kept");
        let mut zorin = dark;
        zorin.gtk_theme = Some("ZorinBlue-Dark".into());
        let before = service.generation();
        service.settings_changed(zorin.clone());
        assert!(service.generation() > before);
        assert!(
            service.get(&old).is_none(),
            "the old theme's drawing is gone"
        );
        let new = key(&zorin, false);
        let drawn = service.get_by(&new, soon()).expect("drawn");
        assert_eq!(drawn.key.gtk_theme.as_deref(), Some("ZorinBlue-Dark"));
    }

    #[test]
    fn a_drawing_for_the_theme_just_replaced_is_thrown_away() {
        let (release, hold) = std::sync::mpsc::channel();
        let (service, drawn) = fake(Some(hold));
        let s = service.settings_by(soon());
        let old = key(&s, false);
        assert!(service.get(&old).is_none());
        // Let the thread start drawing, then change the theme under it.
        let deadline = soon();
        while service.has_requests() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut zorin = s.clone();
        zorin.gtk_theme = Some("ZorinBlue-Light".into());
        service.settings_changed(zorin);
        release.send(()).unwrap();
        while drawn.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(50));
        let state = service.lock();
        assert!(state.renders.is_empty(), "nothing of the old theme is kept");
    }

    #[test]
    fn subscribers_hear_every_change() {
        let (service, _) = fake(None);
        let changes = service.subscribe();
        let s = service.settings_by(soon());
        while changes.try_recv().is_ok() {}
        assert!(service.get_by(&key(&s, false), soon()).is_some());
        assert!(changes.try_recv().is_ok());
    }
}
