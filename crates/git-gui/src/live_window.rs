//! The window's half of [`crate::live`]: setting the watch up off the
//! painting thread whenever the open repository changes, and a short timer
//! that turns what the watch saw into a status refresh.

use std::time::Duration;

use gpui::{AppContext as _, Context, Window};

use crate::app::GitApp;
use crate::live::{self, NotWatched};
use crate::toast::now_ms;

/// How often the window looks at what the watch saw.
const TICK: Duration = Duration::from_millis(100);

impl GitApp {
    /// Starts the timer that reads the watch, unless it runs already. It
    /// stops by itself once no repository is open.
    fn start_watch_loop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.live.ticking, true) {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let go_on = this
                    .update_in(cx, |app, window, cx| {
                        if app.live.root.is_none() {
                            app.live.ticking = false;
                            return false;
                        }
                        app.live_tick(window, cx);
                        true
                    })
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        })
        .detach();
    }

    fn live_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (refresh, restart) = self.live.tick(now_ms());
        if restart && let Some(root) = self.live.root.clone() {
            self.watch(root, window, cx);
        }
        if refresh {
            self.session.refresh();
            self.drive(window, cx);
        }
    }

    /// Watches the repository the session has open, and stops watching one
    /// it closed.
    pub(crate) fn follow_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.session.repo.as_ref().map(|r| r.root.clone());
        if open == self.live.root {
            return;
        }
        self.live.watching = None;
        self.live.polling = false;
        self.live.debounce = live::Debounce::default();
        self.live.root = open.clone();
        if let Some(root) = open {
            self.watch(root, window, cx);
            self.start_watch_loop(window, cx);
        }
    }

    fn watch(&mut self, root: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.live.starting {
            self.live.restart = true;
            return;
        }
        // The old watch goes first, so its folders' watches are given back
        // before the new one asks for them.
        self.live.watching = None;
        self.live.starting = true;
        let git = self.session.git();
        let for_root = root.clone();
        let work = cx.background_spawn(async move { live::start(git, &for_root) });
        cx.spawn_in(window, async move |this, cx| {
            let started = work.await;
            this.update_in(cx, |app, window, cx| {
                app.live.starting = false;
                // The window moved on to another repository meanwhile.
                if app.live.root.as_ref() != Some(&root) {
                    if let Some(now_open) = app.live.root.clone() {
                        app.watch(now_open, window, cx);
                    }
                    return;
                }
                match started {
                    Ok(watching) => {
                        app.live.watching = Some(watching);
                        app.live.polling = false;
                    }
                    Err(NotWatched::Watch(_)) | Err(NotWatched::Git(_)) => {
                        app.live.watching = None;
                        app.live.polling = true;
                        app.live.last_poll = now_ms();
                        if !std::mem::replace(&mut app.live.told, true) {
                            app.session.notice = Some(crate::session::Notice::WatchFallback);
                            cx.notify();
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }
}
