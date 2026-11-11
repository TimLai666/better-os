//! The GPUI window: the session, the two text fields, and the background
//! runner.
//!
//! Everything this type decides it decides by calling
//! [`crate::session::GitSession`]. What is left here needs a window: the
//! text fields, the folder picker, and the task that runs the session's
//! requests on a background thread and hands their answers back.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{Theme, ThemeMode};

use crate::i18n::{Locale, copy};
use crate::session::GitSession;
use crate::{Appearance, StartOptions};

/// The smallest window the layout is drawn for (`design/compiled-spec.md`).
pub(crate) const MIN_WINDOW_WIDTH: f32 = 960.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 600.0;

/// The branch glyph for the command bar. gpui-component's icon set has none,
/// so the window carries its own: two stems and a fork, drawn for this
/// window in the same 24px, 2px-stroke grid as the library's icons.
pub(crate) const BRANCH_ICON: &str = "better-git/branch.svg";
const BRANCH_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10"/><path d="M18 9c0 5-12 3-12 8"/></svg>"#;

/// gpui-component's icons, plus the branch glyph.
struct GitAssets;

impl AssetSource for GitAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == BRANCH_ICON {
            return Ok(Some(Cow::Borrowed(BRANCH_SVG.as_bytes())));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

pub struct GitApp {
    pub(crate) session: GitSession,
    pub(crate) locale: Locale,
    pub(crate) summary_input: Entity<InputState>,
    pub(crate) description_input: Entity<InputState>,
    pub(crate) diff_scroll: UniformListScrollHandle,
    pub(crate) focus_handle: FocusHandle,
    /// The session's form generation the text fields last matched.
    form_generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl GitApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>, options: StartOptions) -> Self {
        let git: Arc<dyn git_platform::Git> = Arc::new(git_platform::SystemGit);
        let mut session = GitSession::new(git, crate::recent_file());
        if let Some(path) = options.path {
            session.open(absolute(path));
        }

        let c = copy(options.locale);
        let summary_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(c.summary_placeholder));
        let description_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(3)
                .placeholder(c.description_placeholder)
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &summary_input,
                window,
                |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.session.summary = state.read(cx).value().to_string();
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.commit(window, cx),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &description_input,
                window,
                |this, state, event: &InputEvent, _window, cx| {
                    if let InputEvent::Change = event {
                        this.session.description = state.read(cx).value().to_string();
                        cx.notify();
                    }
                },
            ),
        ];

        // `gpui_component::init` installs its light theme, so the appearance
        // is applied once the window exists. Better OS opens dark.
        let mode = match options.appearance {
            Appearance::Dark => ThemeMode::Dark,
            Appearance::Light => ThemeMode::Light,
            Appearance::System => match window.appearance() {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
                WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
            },
        };
        Theme::change(mode, Some(window), cx);
        better_ui::fluent::apply_to_component_theme(Theme::global_mut(cx));

        Self {
            session,
            locale: options.locale,
            summary_input,
            description_input,
            diff_scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            form_generation: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Runs the session's next request on a background thread, if one is
    /// waiting and none is running, and comes back for the one after it.
    pub(crate) fn drive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.session.next_request() else {
            return;
        };
        let git = self.session.git();
        let work = cx.background_spawn(async move { request.run(git.as_ref()) });
        cx.spawn_in(window, async move |this, cx| {
            let outcome = work.await;
            // A failed update means the window is gone; nothing is left to
            // tell.
            this.update_in(cx, |app, window, cx| {
                app.session.apply(outcome);
                app.sync_form(window, cx);
                app.drive(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Empties the text fields after the session emptied the form.
    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let generation = self.session.form_generation();
        if generation == self.form_generation {
            return;
        }
        self.form_generation = generation;
        for input in [self.summary_input.clone(), self.description_input.clone()] {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
    }

    pub(crate) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.session.open(path);
        self.drive(window, cx);
    }

    /// Shows the platform's folder picker and opens what was chosen.
    pub(crate) fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = copy(self.locale);
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(c.open_title.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let chosen = answer.await;
            this.update_in(cx, |app, window, cx| {
                match chosen {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            app.open_path(path, window, cx);
                        }
                    }
                    // Cancelled.
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => app.session.picker_failed(error.to_string()),
                    Err(error) => app.session.picker_failed(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn select(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.session.select(&path);
        self.drive(window, cx);
    }

    pub(crate) fn toggle_stage(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.toggle_stage(&path);
        self.drive(window, cx);
    }

    pub(crate) fn toggle_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.toggle_all();
        self.drive(window, cx);
    }

    pub(crate) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.commit();
        self.drive(window, cx);
    }

    pub(crate) fn close_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.close_repository();
        self.sync_form(window, cx);
        cx.notify();
    }

    pub(crate) fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        self.session.dismiss_error();
        cx.notify();
    }
}

/// A relative path from the command line, made absolute against the working
/// directory so the recent list never stores one.
fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(&path))
        .unwrap_or(path)
}

impl Focusable for GitApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Opens the window.
pub fn run(options: StartOptions) {
    let app = gpui_platform::application().with_assets(GitAssets);

    app.run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1_200.0), px(760.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options("io.betteros.Git")
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| GitApp::new(window, cx, options));
                view.update(cx, |app, cx| app.drive(window, cx));
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            })
            .expect("failed to open the Better Git window");
        })
        .detach();
    });
}
