//! A window restarting itself into the program an update installed, and
//! coming back where it was (ticket 105).
//!
//! Every Better OS window mounts one [`UpdateRestart`]. It watches the
//! window's own program file through `better_core::self_update`, and when an
//! update has replaced it and dpkg has configured the package:
//!
//! - if the window is in the middle of something a restart would lose — a file
//!   operation, a Git command, a package transaction, a dialog or changes with
//!   no saved form — it says "An update is ready. Better X will restart when
//!   …" and waits until that ends ([`Blocker`] says which);
//! - otherwise it asks the window for its view state ([`Restartable`]), writes
//!   it with the window's size into a private hand-off, and starts the new
//!   program in this process's place. The new window reads the hand-off with
//!   [`take_handoff`], opens at the same size, puts its view back, and shows
//!   "Updated to {version}" for a few seconds.
//!
//! Every decision is in `better_core::self_update`; this module connects it to
//! a GPUI window and shows the notices as toasts on the window's one
//! [`ToastHost`], which the window can use for its own toasts too.

use std::time::Instant;

use better_core::self_update::{
    HandoffDir, PROGRESS_POLL, ProgramFile, ProgramWatch, RestartDecision, UpdateState,
    UpdateTracker, WindowMode, WindowPlacement, decide, own_arguments, restart_in_place,
};
use gpui::{
    AnyElement, App, Bounds, Context, Entity, Subscription, Task, Window, WindowBounds, point, px,
    size,
};
use serde::Serialize;

use crate::fluent::info_bar::Severity;
use crate::fluent::toast::{ToastHost, ToastId, ToastSpec, ToastWords, new_host, toast_layer};
use crate::fluent::words::Locale;

/// What the program this one replaced handed over. Re-exported so a window
/// needs no other crate to read it.
pub use better_core::self_update::Handoff;

/// Why a window cannot restart yet. The notice says what ends the wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blocker {
    /// Work is running: a file operation, a Git command, a transaction.
    Task,
    /// A dialog is open whose input has no saved form.
    Dialog,
    /// Changes on the page that are neither saved nor discarded.
    UnsavedChanges,
    /// Other windows of the same program are open in this process; a restart
    /// would close them without their view. The last one left restarts.
    OtherWindows,
}

/// A window that can restart into a new program.
pub trait Restartable: Sized + 'static {
    /// What the window puts back after a restart. It is read by the next
    /// version of the program, so each field should default when missing.
    type State: Serialize;

    fn update_restart(&mut self) -> &mut UpdateRestart;

    /// What the window is in the middle of that a restart would lose, if
    /// anything.
    fn restart_blocked(&self, cx: &App) -> Option<Blocker>;

    /// The view as it is now.
    fn restart_state(&self, window: &Window, cx: &App) -> Self::State;
}

/// The hand-off the program this one replaced left for it, if any. Read it
/// once, before the window opens, so the window can open at its old size.
pub fn take_handoff(app_id: &str) -> Option<Handoff> {
    HandoffDir::from_environment()?.take_own(app_id)
}

/// The bounds a window opens with after a restart, or `None` to let the window
/// choose as it always does.
pub fn window_bounds(handoff: Option<&Handoff>) -> Option<WindowBounds> {
    let placement = handoff?.window?;
    if !(placement.width.is_finite() && placement.height.is_finite())
        || placement.width < 1.0
        || placement.height < 1.0
    {
        return None;
    }
    let bounds = Bounds {
        origin: point(px(placement.x), px(placement.y)),
        size: size(px(placement.width), px(placement.height)),
    };
    Some(match placement.mode {
        WindowMode::Windowed => WindowBounds::Windowed(bounds),
        WindowMode::Maximized => WindowBounds::Maximized(bounds),
        WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
    })
}

/// How a window is placed now, in the hand-off's terms.
pub fn placement(bounds: WindowBounds) -> WindowPlacement {
    let (mode, bounds) = match bounds {
        WindowBounds::Windowed(bounds) => (WindowMode::Windowed, bounds),
        WindowBounds::Maximized(bounds) => (WindowMode::Maximized, bounds),
        WindowBounds::Fullscreen(bounds) => (WindowMode::Fullscreen, bounds),
    };
    WindowPlacement {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        mode,
    }
}

/// What the window says about an update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    /// An update is installed and the window is waiting for this to end.
    Waiting { app: &'static str, blocker: Blocker },
    /// The window restarted into this version.
    Updated { version: String },
    /// The new program could not be started; the old one keeps running.
    ReopenToUse,
}

impl Notice {
    /// The notice's severity and words.
    pub fn words(&self, locale: Locale) -> (Severity, ToastWords) {
        let zh = locale == Locale::TraditionalChinese;
        match self {
            Notice::Waiting { app, blocker } => {
                let message = match (blocker, zh) {
                    (Blocker::Dialog, false) => {
                        format!("{app} will restart when you close this dialog.")
                    }
                    (Blocker::Dialog, true) => format!("關閉這個對話方塊後，{app} 就會重新開啟。"),
                    (Blocker::Task, false) => {
                        format!("{app} will restart when this task finishes.")
                    }
                    (Blocker::Task, true) => format!("這項工作完成後，{app} 就會重新開啟。"),
                    (Blocker::UnsavedChanges, false) => {
                        format!("{app} will restart when you save or discard your changes.")
                    }
                    (Blocker::UnsavedChanges, true) => {
                        format!("儲存或捨棄變更後，{app} 就會重新開啟。")
                    }
                    (Blocker::OtherWindows, false) => {
                        format!("{app} will restart when you close its other windows.")
                    }
                    (Blocker::OtherWindows, true) => {
                        format!("關閉 {app} 的其他視窗後，它就會重新開啟。")
                    }
                };
                (
                    Severity::Informational,
                    ToastWords {
                        title: if zh {
                            "有可用的更新"
                        } else {
                            "An update is ready"
                        }
                        .to_string(),
                        message: Some(message),
                    },
                )
            }
            Notice::Updated { version } => (
                Severity::Success,
                ToastWords {
                    title: if zh {
                        format!("已更新到 {version}")
                    } else {
                        format!("Updated to {version}")
                    },
                    message: None,
                },
            ),
            Notice::ReopenToUse => (
                Severity::Warning,
                ToastWords {
                    title: if zh {
                        "已安裝新版本"
                    } else {
                        "A new version is installed"
                    }
                    .to_string(),
                    message: Some(
                        if zh {
                            "關閉這個視窗再重新開啟，就會使用新版本。"
                        } else {
                            "Close this window and open it again to use it."
                        }
                        .to_string(),
                    ),
                },
            ),
        }
    }

    /// The toast for this notice: "Updated to" goes away on its own; the other
    /// two stay while they are true, and can be closed.
    pub fn toast(&self) -> ToastSpec {
        let (severity, _) = self.words(Locale::English);
        let notice = self.clone();
        let words = move |locale| notice.words(locale).1;
        match self {
            Notice::Updated { .. } => ToastSpec::timed(severity, words),
            _ => ToastSpec::lasting(severity, words),
        }
    }
}

/// The notice for the window that a restart just opened.
pub fn notice_after_restart(handoff: Option<&Handoff>, own_version: &str) -> Option<Notice> {
    let handoff = handoff?;
    let version = handoff
        .updated_to
        .clone()
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| own_version.to_string());
    Some(Notice::Updated { version })
}

/// What the waiting notice should do, given what the window is blocked by now
/// and what the notice shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitingChange {
    Keep,
    /// Take the notice down (the wait is over).
    Remove,
    /// Show it, or show it again for a different reason.
    Show(Blocker),
}

/// The waiting notice's rule. A person who closed it is not shown it again
/// for the same wait; a new reason is a new notice.
pub fn waiting_change(
    blocked: Option<Blocker>,
    shown: Option<Blocker>,
    closed_for: Option<Blocker>,
) -> WaitingChange {
    match (blocked, shown) {
        (None, Some(_)) => WaitingChange::Remove,
        (None, None) => WaitingChange::Keep,
        (Some(now), Some(was)) if now == was => WaitingChange::Keep,
        (Some(now), _) if closed_for == Some(now) => WaitingChange::Keep,
        (Some(now), _) => WaitingChange::Show(now),
    }
}

/// One window's update watch, hand-off, and notices.
pub struct UpdateRestart {
    app_id: &'static str,
    app_name: &'static str,
    tracker: Option<UpdateTracker>,
    host: Entity<ToastHost>,
    waiting: Option<(ToastId, Blocker)>,
    closed_for: Option<Blocker>,
    failed: bool,
    _observe: Option<Subscription>,
    _watch: Option<ProgramWatch>,
    _task: Option<Task<()>>,
}

impl UpdateRestart {
    /// Starts watching the window's program. `app_name` is the window's
    /// product name as notices print it ("Better Git"), `package` the dpkg
    /// package it ships in, `version` the program's own version, used when
    /// dpkg names none, and `restored` the hand-off [`take_handoff`] returned.
    pub fn new<V: Restartable>(
        app_id: &'static str,
        app_name: &'static str,
        package: &'static str,
        version: &'static str,
        restored: Option<&Handoff>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> UpdateRestart {
        let (host, observe) = new_host(cx);
        if let Some(notice) = notice_after_restart(restored, version) {
            host.update(cx, |host, cx| host.push(notice.toast(), cx));
        }

        let tracker = match ProgramFile::current() {
            Ok(program) => Some(UpdateTracker::new(program, Some(package))),
            Err(error) => {
                eprintln!("{app_id}: cannot watch for updates: {error}");
                None
            }
        };
        let (wake, wakes) = smol::channel::bounded::<()>(1);
        let watch = tracker.as_ref().and_then(|tracker| {
            let wake = wake.clone();
            ProgramWatch::start(tracker.program().path(), move || {
                let _ = wake.try_send(());
            })
            .map_err(|error| eprintln!("{app_id}: cannot watch for updates: {error}"))
            .ok()
        });
        let task = watch.as_ref().map(|_| {
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    if wakes.recv().await.is_err() {
                        return;
                    }
                    loop {
                        match this.update_in(cx, |view, window, cx| step(view, window, cx)) {
                            Ok(true) => cx.background_executor().timer(PROGRESS_POLL).await,
                            Ok(false) => break,
                            Err(_) => return,
                        }
                    }
                }
            })
        });
        drop(wake);

        UpdateRestart {
            app_id,
            app_name,
            tracker,
            host,
            waiting: None,
            closed_for: None,
            failed: false,
            _observe: Some(observe),
            _watch: watch,
            _task: task,
        }
    }

    /// One that watches nothing, for a window built by a test or a benchmark,
    /// which must never restart the process it runs in. It still has a toast
    /// host, so the window's own toasts work.
    pub fn inactive<V: 'static>(app_id: &'static str, cx: &mut Context<V>) -> UpdateRestart {
        let (host, observe) = new_host(cx);
        UpdateRestart {
            app_id,
            app_name: app_id,
            tracker: None,
            host,
            waiting: None,
            closed_for: None,
            failed: false,
            _observe: Some(observe),
            _watch: None,
            _task: None,
        }
    }

    /// The window's toast host, for the window's own toasts.
    pub fn toast_host(&self) -> &Entity<ToastHost> {
        &self.host
    }

    /// The window's toasts, over everything else. Add it as the last child of
    /// the window's root element.
    pub fn toast_layer(&self, locale: Locale, cx: &App) -> Option<AnyElement> {
        toast_layer(&self.host, locale, cx)
    }

    fn sync_waiting(&mut self, blocked: Option<Blocker>, cx: &mut App) {
        // A notice no longer in the host was closed by the person.
        if let Some((id, blocker)) = self.waiting
            && !self.host.read(cx).contains(id)
        {
            self.waiting = None;
            self.closed_for = Some(blocker);
        }
        if blocked.is_none() {
            self.closed_for = None;
        }
        match waiting_change(blocked, self.waiting.map(|w| w.1), self.closed_for) {
            WaitingChange::Keep => {}
            WaitingChange::Remove => {
                if let Some((id, _)) = self.waiting.take() {
                    self.host.update(cx, |host, cx| host.dismiss(id, cx));
                }
            }
            WaitingChange::Show(blocker) => {
                if let Some((id, _)) = self.waiting.take() {
                    self.host.update(cx, |host, cx| host.dismiss(id, cx));
                }
                self.closed_for = None;
                let notice = Notice::Waiting {
                    app: self.app_name,
                    blocker,
                };
                let id = self
                    .host
                    .update(cx, |host, cx| host.push(notice.toast(), cx));
                self.waiting = Some((id, blocker));
            }
        }
    }
}

/// One look at the program file. Returns whether to look again soon.
fn step<V: Restartable>(view: &mut V, window: &mut Window, cx: &mut Context<V>) -> bool {
    let blocked = view.restart_blocked(cx);
    let restart = view.update_restart();
    if restart.failed {
        return false;
    }
    let Some(tracker) = restart.tracker.as_mut() else {
        return false;
    };
    let state = tracker.poll(Instant::now());
    match decide(&state, blocked.is_some()) {
        RestartDecision::Stay => {
            restart.sync_waiting(None, cx);
            state != UpdateState::Current
        }
        RestartDecision::Wait => {
            restart.sync_waiting(blocked, cx);
            true
        }
        RestartDecision::Restart => {
            restart.sync_waiting(None, cx);
            let version = match state {
                UpdateState::Ready { version } => version,
                _ => None,
            };
            restart_now(view, version, window, cx);
            false
        }
    }
}

/// Writes the hand-off and starts the new program. Returns only when that
/// failed, and then says so instead.
fn restart_now<V: Restartable>(
    view: &mut V,
    version: Option<String>,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let state = view.restart_state(window, cx);
    let restart = view.update_restart();
    let Some(program) = restart
        .tracker
        .as_ref()
        .map(|t| t.program().path().to_path_buf())
    else {
        return;
    };
    let handoff = Handoff::new(
        restart.app_id,
        version,
        Some(placement(window.window_bounds())),
        &state,
    );
    let written = HandoffDir::from_environment().and_then(|dir| match dir.write(&handoff) {
        Ok(path) => Some(path),
        Err(error) => {
            eprintln!(
                "{}: cannot save the view for the restart: {error}",
                restart.app_id
            );
            None
        }
    });
    let error = restart_in_place(&program, &own_arguments());
    eprintln!(
        "{}: cannot start the updated program {}: {error}",
        restart.app_id,
        program.display()
    );
    if let Some(path) = written {
        let _ = std::fs::remove_file(path);
    }
    restart.failed = true;
    let toast = Notice::ReopenToUse.toast();
    restart.host.update(cx, |host, cx| {
        host.push(toast, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::{
        Blocker, Notice, WaitingChange, notice_after_restart, placement, waiting_change,
        window_bounds,
    };
    use crate::fluent::info_bar::Severity;
    use crate::fluent::words::Locale;
    use better_core::self_update::{Handoff, WindowMode, WindowPlacement};
    use gpui::{Bounds, WindowBounds, point, px, size};

    fn handoff(updated_to: Option<&str>, window: Option<WindowPlacement>) -> Handoff {
        Handoff::new(
            "io.betteros.Test",
            updated_to.map(str::to_string),
            window,
            &(),
        )
    }

    #[test]
    fn a_window_opens_at_the_size_and_state_it_had() {
        let bounds = Bounds {
            origin: point(px(40.0), px(30.0)),
            size: size(px(1100.0), px(700.0)),
        };
        for kept in [
            WindowBounds::Windowed(bounds),
            WindowBounds::Maximized(bounds),
            WindowBounds::Fullscreen(bounds),
        ] {
            let saved = handoff(None, Some(placement(kept)));
            assert_eq!(window_bounds(Some(&saved)), Some(kept));
        }
        assert_eq!(window_bounds(None), None);
        assert_eq!(window_bounds(Some(&handoff(None, None))), None);
    }

    #[test]
    fn a_nonsense_size_is_not_used() {
        for (width, height) in [(0.0, 700.0), (f32::NAN, 700.0), (1000.0, f32::INFINITY)] {
            let saved = handoff(
                None,
                Some(WindowPlacement {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                    mode: WindowMode::Windowed,
                }),
            );
            assert_eq!(window_bounds(Some(&saved)), None);
        }
    }

    #[test]
    fn the_notice_names_the_version_dpkg_installed() {
        assert_eq!(notice_after_restart(None, "0.2.12"), None);
        assert_eq!(
            notice_after_restart(Some(&handoff(Some("0.2.13"), None)), "0.2.12"),
            Some(Notice::Updated {
                version: "0.2.13".into()
            })
        );
        // A program no package owns names its own version.
        assert_eq!(
            notice_after_restart(Some(&handoff(None, None)), "0.2.12"),
            Some(Notice::Updated {
                version: "0.2.12".into()
            })
        );
    }

    #[test]
    fn the_notices_read_in_both_languages_and_say_what_ends_the_wait() {
        let updated = Notice::Updated {
            version: "0.2.13".into(),
        };
        let (severity, words) = updated.words(Locale::English);
        assert_eq!(severity, Severity::Success);
        assert_eq!(words.title, "Updated to 0.2.13");
        assert_eq!(
            updated.words(Locale::TraditionalChinese).1.title,
            "已更新到 0.2.13"
        );
        let waiting = |blocker| Notice::Waiting {
            app: "Better Git",
            blocker,
        };
        for (blocker, en, zh) in [
            (
                Blocker::Dialog,
                "Better Git will restart when you close this dialog.",
                "關閉這個對話方塊後，Better Git 就會重新開啟。",
            ),
            (
                Blocker::Task,
                "Better Git will restart when this task finishes.",
                "這項工作完成後，Better Git 就會重新開啟。",
            ),
            (
                Blocker::UnsavedChanges,
                "Better Git will restart when you save or discard your changes.",
                "儲存或捨棄變更後，Better Git 就會重新開啟。",
            ),
            (
                Blocker::OtherWindows,
                "Better Git will restart when you close its other windows.",
                "關閉 Better Git 的其他視窗後，它就會重新開啟。",
            ),
        ] {
            let english = waiting(blocker).words(Locale::English).1;
            assert_eq!(english.title, "An update is ready");
            assert_eq!(english.message.as_deref(), Some(en));
            let chinese = waiting(blocker).words(Locale::TraditionalChinese).1;
            assert_eq!(chinese.title, "有可用的更新");
            assert_eq!(chinese.message.as_deref(), Some(zh));
        }
        assert!(
            Notice::ReopenToUse
                .words(Locale::English)
                .1
                .message
                .is_some()
        );
    }

    #[test]
    fn only_the_after_restart_notice_goes_away_on_its_own_and_the_others_can_be_closed() {
        let updated = Notice::Updated {
            version: "1".into(),
        }
        .toast();
        assert!(updated.duration.is_some());
        assert!(!updated.closable);
        for notice in [
            Notice::Waiting {
                app: "Better Git",
                blocker: Blocker::Task,
            },
            Notice::ReopenToUse,
        ] {
            let toast = notice.toast();
            assert!(toast.duration.is_none(), "{notice:?}");
            assert!(toast.closable, "{notice:?}");
        }
    }

    #[test]
    fn the_waiting_notice_follows_the_wait_and_stays_closed_once_closed() {
        use WaitingChange::*;
        assert_eq!(waiting_change(None, None, None), Keep);
        assert_eq!(
            waiting_change(Some(Blocker::Task), None, None),
            Show(Blocker::Task)
        );
        assert_eq!(
            waiting_change(Some(Blocker::Task), Some(Blocker::Task), None),
            Keep
        );
        assert_eq!(
            waiting_change(Some(Blocker::Dialog), Some(Blocker::Task), None),
            Show(Blocker::Dialog),
            "a new reason replaces the old notice"
        );
        assert_eq!(waiting_change(None, Some(Blocker::Task), None), Remove);
        assert_eq!(
            waiting_change(Some(Blocker::Task), None, Some(Blocker::Task)),
            Keep,
            "closed by the person, not shown again for the same wait"
        );
        assert_eq!(
            waiting_change(Some(Blocker::Dialog), None, Some(Blocker::Task)),
            Show(Blocker::Dialog)
        );
    }
}
