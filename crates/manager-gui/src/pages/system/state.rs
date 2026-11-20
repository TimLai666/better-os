//! What the System page keeps between interactions, and the reads and
//! writes it makes. Every read and write runs off the UI thread; what the
//! answers mean is decided in `hardware-core` and `decide`.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{AppContext as _, Context, Task};
use hardware_core::display::{KeyboardLevels, Throttle};
use hardware_core::fan::FanMode;
use hardware_core::follow::ChargerFollow;
use hardware_core::page::MachineFacts;
use hardware_core::power::Profile;
use hardware_core::write::WriteOutcome;
use hardware_platform::bus::{self, Endpoint};
use hardware_platform::sysfs::{self, SysRoot};
use hardware_platform::{follow_store, privileged};
use zbus::blocking::Connection;

use super::decide::{AboutFacts, desktop_line};
use crate::app::ManagerApp;
use crate::model::Page;

/// How often temperatures and fans are read while the page is shown.
pub(crate) const SENSOR_INTERVAL: Duration = Duration::from_secs(2);

/// Which group a state bar sits above.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Group {
    Power,
    Display,
    Cooling,
}

/// A write that is waiting for its answer (and maybe for polkit).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Writing {
    PowerMode,
    ChargeLimit,
    FanMode,
    Keyboard,
    Follow,
}

#[derive(Default)]
pub(crate) struct SystemState {
    /// `None` until the first read lands.
    pub(crate) facts: Option<MachineFacts>,
    pub(crate) about: AboutFacts,
    pub(crate) follow: Option<Result<ChargerFollow, String>>,
    /// The last write's outcome that needs a bar, and over which group.
    pub(crate) bar: Option<(Group, WriteOutcome)>,
    pub(crate) writing: Option<Writing>,
    pub(crate) reading_since: Option<Instant>,
    pub(crate) battery_open: bool,
    pub(crate) sensors_open: bool,
    pub(crate) about_open: bool,
    pub(crate) follow_open: bool,
    /// The keyboard slider's own level while a write is on its way.
    pub(crate) keyboard_drag: Option<u32>,
    /// A keyboard level asked for while another was being written.
    keyboard_pending: Option<(KeyboardLevels, u32)>,
    /// The about row whose value was just copied.
    pub(crate) copied: Option<usize>,
    /// The slider's own value while a drag is being written.
    pub(crate) brightness_drag: Option<u8>,
    pub(crate) throttle: Throttle,
    pub(crate) following: bool,
    /// The Drives section (ticket 127).
    pub(crate) drives: super::drives::DrivesState,
    again: bool,
    task: Option<Task<()>>,
    sensor_task: Option<Task<()>>,
    brightness_task: Option<Task<()>>,
    write_task: Option<Task<()>>,
}

impl SystemState {
    pub(crate) fn shows_ring(&self) -> bool {
        self.facts.is_none()
            && self.reading_since.is_some_and(|since| {
                since.elapsed()
                    >= Duration::from_millis(crate::pages::states::LOADING_RING_AFTER_MS)
            })
    }

    /// Battery health for Home, when the battery could be read.
    pub(crate) fn battery_health(&self) -> Option<u8> {
        self.facts.as_ref()?.battery.value()?.health_percent()
    }

    /// A password prompt or a write is open: an update restart waits for it.
    pub(crate) fn blocks_restart(&self) -> bool {
        self.writing.is_some() || self.drives.blocks_restart()
    }
}

/// Where the page reads sysfs. A test build reads nothing of the machine's.
/// A debug build takes `BETTER_OS_DEBUG_SYS_ROOT`, so a sandbox can show the
/// page for a fixture machine; a release build always reads `/`.
pub(crate) fn root() -> SysRoot {
    if cfg!(test) {
        return SysRoot::at("/nonexistent/better-os-test-root");
    }
    if cfg!(debug_assertions)
        && let Some(root) = std::env::var_os("BETTER_OS_DEBUG_SYS_ROOT")
    {
        return SysRoot::at(root);
    }
    SysRoot::system()
}

/// The system and session buses, opened once. A test build opens neither, so
/// no test reaches the machine's own services.
fn buses() -> (Option<Connection>, Option<Connection>) {
    static BUSES: OnceLock<(Option<Connection>, Option<Connection>)> = OnceLock::new();
    BUSES
        .get_or_init(|| {
            if cfg!(test) {
                (None, None)
            } else {
                (Connection::system().ok(), Connection::session().ok())
            }
        })
        .clone()
}

fn follow_path() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        None
    } else {
        follow_store::path_for_user()
    }
}

fn read_about(profile_line: Option<String>, architecture: String) -> AboutFacts {
    let files = sysfs::about(&root());
    let (_, session) = buses();
    let shell = session.as_ref().and_then(bus::shell_major_version);
    AboutFacts {
        product: files
            .product_name
            .as_deref()
            .and_then(crate::home::product_name),
        system: profile_line,
        architecture: (!architecture.is_empty()).then_some(architecture),
        kernel: files.kernel,
        desktop: desktop_line(
            &better_core::CurrentDesktop::from_env(),
            shell.as_deref(),
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        ),
    }
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

impl ManagerApp {
    /// Opens the expander a search result or Home's item lands on.
    pub(crate) fn open_system_card(&mut self, card_id: &str) {
        use super::card;
        match card_id {
            card::BATTERY => self.system.battery_open = true,
            card::SENSORS => self.system.sensors_open = true,
            card::ABOUT => self.system.about_open = true,
            card::FOLLOW => self.system.follow_open = true,
            _ => {}
        }
    }

    /// Reads the whole page off the UI thread. A read asked for while one
    /// runs runs once that one lands.
    pub(crate) fn refresh_system(&mut self, cx: &mut Context<Self>) {
        if self.system.task.is_some() {
            self.system.again = true;
            return;
        }
        if self.system.facts.is_none() {
            self.system.reading_since = Some(Instant::now());
        }
        if !self.system.following {
            self.system.following = true;
            self.follow_system(cx);
        }
        self.refresh_drives(cx);
        let profile_line = Some(crate::model::host_line(
            self.manager.profile(),
            crate::i18n::copy(self.locale).ubuntu_base,
        ))
        .filter(|line| !line.is_empty());
        let architecture = self.manager.profile().architecture.clone();
        self.system.task = Some(cx.spawn(async move |this, cx| {
            let ring = cx.background_executor().timer(Duration::from_millis(
                crate::pages::states::LOADING_RING_AFTER_MS,
            ));
            let work = cx.background_spawn(async move {
                let (system, session) = buses();
                let facts =
                    hardware_platform::read_machine(&root(), system.as_ref(), session.as_ref());
                let follow = follow_path().map(|path| follow_store::load(&path));
                (facts, follow, read_about(profile_line, architecture))
            });
            ring.await;
            let _ = this.update(cx, |_, cx| cx.notify());
            let (facts, follow, about) = work.await;
            let _ = this.update(cx, |app, cx| {
                app.system.task = None;
                app.system.reading_since = None;
                // A drag being written keeps the slider where the hand is.
                if app.system.brightness_task.is_none() {
                    app.system.brightness_drag = None;
                }
                app.system.facts = Some(facts);
                app.system.follow = follow;
                app.system.about = about;
                if std::mem::take(&mut app.system.again) {
                    app.refresh_system(cx);
                }
                cx.notify();
            });
        }));
    }

    /// Follows power-profiles-daemon and gsd-power, so a brightness key, a
    /// mode changed in GNOME's own menu, or a daemon that starts moves the
    /// page as soon as it happens, without polling.
    fn follow_system(&mut self, cx: &mut Context<Self>) {
        let (system, session) = buses();
        let (sender, receiver) = smol::channel::unbounded::<()>();
        if let Some(system) = system {
            let sender = sender.clone();
            let endpoints: Vec<Endpoint> = bus::POWER_PROFILES.to_vec();
            std::thread::spawn(move || {
                bus::follow(&system, &endpoints, || sender.send_blocking(()).is_ok())
            });
        }
        if let Some(session) = session {
            std::thread::spawn(move || {
                bus::follow(&session, &[bus::GSD_SCREEN], || {
                    sender.send_blocking(()).is_ok()
                })
            });
        }
        cx.spawn(async move |this, cx| {
            while receiver.recv().await.is_ok() {
                while receiver.try_recv().is_ok() {}
                if this.update(cx, |app, cx| app.refresh_system(cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Reads temperatures and fans every two seconds while the page is
    /// shown, off the UI thread, and stops once another page is.
    pub(crate) fn tick_sensors(&mut self, cx: &mut Context<Self>) {
        if self.system.sensor_task.is_some() {
            return;
        }
        self.system.sensor_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SENSOR_INTERVAL).await;
                let visible = this
                    .update(cx, |app, _| app.page == Page::System)
                    .unwrap_or(false);
                if !visible {
                    let _ = this.update(cx, |app, _| app.system.sensor_task = None);
                    return;
                }
                let readings = cx
                    .background_spawn(async move { sysfs::sensors(&root()) })
                    .await;
                let _ = this.update(cx, |app, cx| {
                    if let Some(facts) = app.system.facts.as_mut() {
                        facts.sensors = readings;
                        cx.notify();
                    }
                });
            }
        }));
    }

    /// Runs one write off the UI thread, then shows its outcome and reads
    /// the page again.
    fn system_write(
        &mut self,
        what: Writing,
        group: Group,
        work: impl FnOnce() -> WriteOutcome + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.system.writing.is_some() {
            return;
        }
        self.system.writing = Some(what);
        self.system.bar = None;
        cx.notify();
        self.system.write_task = Some(cx.spawn(async move |this, cx| {
            let outcome = cx.background_spawn(async move { work() }).await;
            let _ = this.update(cx, |app, cx| {
                app.system.writing = None;
                app.system.write_task = None;
                if outcome != WriteOutcome::Done {
                    app.system.bar = Some((group, outcome));
                }
                if let Some((levels, level)) = app.system.keyboard_pending.take() {
                    app.set_keyboard(levels, level, cx);
                } else {
                    app.system.keyboard_drag = None;
                }
                app.refresh_system(cx);
                cx.notify();
            });
        }));
    }

    pub(crate) fn set_power_mode(&mut self, profile: Profile, cx: &mut Context<Self>) {
        self.system_write(
            Writing::PowerMode,
            Group::Power,
            move || match buses().0 {
                Some(system) => match bus::set_power_profile(&system, profile) {
                    Ok(()) => WriteOutcome::Done,
                    Err(error) => WriteOutcome::Failed {
                        details: error.details().to_string(),
                    },
                },
                None => WriteOutcome::Failed {
                    details: "the system bus is not reachable".to_string(),
                },
            },
            cx,
        );
    }

    /// Through `manager-daemon`; polkit asks for an administrator.
    pub(crate) fn set_charge_limit(&mut self, percent: u8, cx: &mut Context<Self>) {
        self.system_write(
            Writing::ChargeLimit,
            Group::Power,
            move || match buses().0 {
                Some(system) => privileged::set_charge_limit(&system, percent),
                None => WriteOutcome::Failed {
                    details: "the system bus is not reachable".to_string(),
                },
            },
            cx,
        );
    }

    pub(crate) fn set_fan_mode(&mut self, mode: FanMode, cx: &mut Context<Self>) {
        self.system_write(
            Writing::FanMode,
            Group::Cooling,
            move || match buses().0 {
                Some(system) => privileged::set_fan_mode(&system, mode),
                None => WriteOutcome::Failed {
                    details: "the system bus is not reachable".to_string(),
                },
            },
            cx,
        );
    }

    pub(crate) fn set_keyboard(
        &mut self,
        levels: KeyboardLevels,
        level: u32,
        cx: &mut Context<Self>,
    ) {
        // A drag moves faster than the bus answers: the last level asked for
        // is written once the one on its way lands.
        if self.system.writing.is_some() {
            self.system.keyboard_pending = Some((levels, level));
            return;
        }
        self.system_write(
            Writing::Keyboard,
            Group::Display,
            move || match buses() {
                (Some(system), Some(session)) => {
                    match bus::set_keyboard(&session, &system, &levels, level) {
                        Ok(()) => WriteOutcome::Done,
                        Err(error) => WriteOutcome::Failed {
                            details: error.details().to_string(),
                        },
                    }
                }
                _ => WriteOutcome::Failed {
                    details: "the buses are not reachable".to_string(),
                },
            },
            cx,
        );
    }

    /// Writes the charger switch's file; the session service reads it at the
    /// next plug event.
    pub(crate) fn set_follow(&mut self, follow: ChargerFollow, cx: &mut Context<Self>) {
        self.system.follow = Some(Ok(follow));
        self.system_write(
            Writing::Follow,
            Group::Power,
            move || match follow_path() {
                Some(path) => match follow_store::save(&path, &follow) {
                    Ok(()) => WriteOutcome::Done,
                    Err(details) => WriteOutcome::Failed { details },
                },
                None => WriteOutcome::Failed {
                    details: "neither XDG_CONFIG_HOME nor HOME is set".to_string(),
                },
            },
            cx,
        );
    }

    /// A slider value while dragging: written at once when 50 ms have passed
    /// since the last write, otherwise kept and written when they have, so
    /// the last position is always the one written.
    pub(crate) fn drag_brightness(&mut self, value: u8, cx: &mut Context<Self>) {
        self.system.brightness_drag = Some(value);
        let now = now_ms();
        if let Some(value) = self.system.throttle.push(now, value) {
            send_brightness(value);
        }
        if self.system.brightness_task.is_none()
            && let Some(wait) = self.system.throttle.wait_ms(now)
        {
            self.system.brightness_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(wait.max(1)))
                    .await;
                let _ = this.update(cx, |app, cx| {
                    app.system.brightness_task = None;
                    if let Some(value) = app.system.throttle.due(now_ms()) {
                        send_brightness(value);
                    }
                    if app.system.throttle.wait_ms(now_ms()).is_some() {
                        let last = app.system.brightness_drag.unwrap_or(value);
                        app.drag_brightness(last, cx);
                    }
                });
            }));
        }
        cx.notify();
    }
}

/// One brightness write on a thread of its own; gsd-power answers with
/// `PropertiesChanged`, which reads the page again.
fn send_brightness(value: u8) {
    std::thread::spawn(move || {
        if let (_, Some(session)) = buses() {
            let _ = bus::set_screen_brightness(&session, value);
        }
    });
}
