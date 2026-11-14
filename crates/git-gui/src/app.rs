//! The GPUI window: the session, the text fields, the background runners,
//! the timers, the keyring, and the programs Better Git starts.
//!
//! Everything this type decides it decides by calling
//! [`crate::session::GitSession`]. What is left here needs a window: text
//! fields, the folder picker, the clipboard, the keyring (GPUI's
//! credentials API, which is the Secret Service on Linux), timers, and the
//! tasks that run git jobs and GitHub requests off the painting thread.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::*;
use gpui_component::input::{InputEvent, InputState};

use better_ui::fluent::menu::{MenuOpen, new_state};
use git_platform::keyring::credential_url;
use github_api::{Host, Token};

use crate::dialogs::Dialog;
use crate::github::Account;
use crate::i18n::{Copy, Locale, copy};
use crate::keys::{self, Action};
use crate::session::{Files, GitSession, KeyringChange, Tab};
use crate::settings::{self, Launch};
use crate::{Appearance, StartOptions};

/// The smallest window the layout is drawn for.
pub(crate) const MIN_WINDOW_WIDTH: f32 = 960.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 600.0;

/// The branch glyph. gpui-component's icon set has none, so the window
/// carries its own, drawn in the same 24px, 2px-stroke grid as Lucide.
pub(crate) const BRANCH_ICON: &str = "better-git/branch.svg";
const BRANCH_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10"/><path d="M18 9c0 5-12 3-12 8"/></svg>"#;

struct GitAssets;

impl AssetSource for GitAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == BRANCH_ICON {
            return Ok(Some(Cow::Borrowed(BRANCH_SVG.as_bytes())));
        }
        if path == crate::graph_pane::TAG_ICON {
            return Ok(Some(Cow::Borrowed(crate::graph_pane::TAG_SVG.as_bytes())));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

/// Which flyout under the toolbar is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Flyout {
    Repositories,
    Branches,
}

/// What a context menu is about.
#[derive(Clone, Debug)]
pub(crate) enum ContextTarget {
    Change(PathBuf),
    Commit(usize),
    /// A row of the Graph tab.
    GraphCommit(usize),
    /// The graph labels a row had no room for ("+N").
    GraphLabels(Vec<git_core::RefLabel>),
    Branch(String),
    Repository(PathBuf),
}

/// The text fields a page or dialog uses.
pub(crate) struct Inputs {
    pub summary: Entity<InputState>,
    pub description: Entity<InputState>,
    pub co_authors: Entity<InputState>,
    pub filter: Entity<InputState>,
    pub repo_search: Entity<InputState>,
    pub branch_search: Entity<InputState>,
    pub dialog: [Entity<InputState>; 3],
    pub editor: Entity<InputState>,
    pub terminal: Entity<InputState>,
    pub git_name: Entity<InputState>,
    pub git_email: Entity<InputState>,
    pub git_branch: Entity<InputState>,
}

pub struct GitApp {
    pub(crate) session: GitSession,
    pub(crate) locale: Locale,
    pub(crate) inputs: Inputs,
    pub(crate) diff_scroll: UniformListScrollHandle,
    pub(crate) graph: crate::graph_pane::GraphUi,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) flyout: Option<Flyout>,
    /// 0 branches, 1 pull requests.
    pub(crate) branch_tab: usize,
    /// File, View, Repository, Branch.
    pub(crate) app_menu: [Entity<MenuOpen>; 4],
    pub(crate) context_menu: Entity<MenuOpen>,
    pub(crate) context: Option<(ContextTarget, Point<Pixels>)>,
    pub(crate) show_filter: bool,
    pub(crate) show_commit_options: bool,
    /// The settings page's section.
    pub(crate) settings_page: usize,
    /// The session notice shown as a toast, and that toast.
    pub(crate) notice_toast: Option<(crate::session::Notice, better_ui::fluent::toast::ToastId)>,
    /// Which dialog fields the person has typed in since the dialog opened.
    pub(crate) dialog_touched: [bool; 3],
    /// The last dialog check, so the disk is looked at when the dialog or
    /// its text changes and not on every frame.
    pub(crate) dialog_check: std::cell::RefCell<Option<crate::render_parts::DialogCheck>>,
    /// The editor's name for "Open in {editor}", with the setting it was
    /// found for, so `PATH` is searched when the setting changes only.
    pub(crate) editor_name: std::cell::RefCell<Option<(String, Option<String>)>>,
    /// The open repository's folders being watched for changes.
    pub(crate) live: crate::live::Live,
    /// The dialog the dialog fields were last filled for.
    dialog_seen: Option<Dialog>,
    form_generation: u64,
    config_loaded: bool,
    last_poll: i64,
    /// Watches for an update and restarts into it (ticket 105).
    pub(crate) update_restart: better_ui::restart::UpdateRestart,
    /// The diff's scroll position a restart handed over, until a diff is
    /// on screen to scroll.
    restored_diff_scroll: Option<f32>,
    _subscriptions: Vec<Subscription>,
}

/// The window's application id, which the desktop entry is named after.
pub(crate) const APP_ID: &str = "io.betteros.Git";

fn field(window: &mut Window, cx: &mut Context<GitApp>, placeholder: &str) -> Entity<InputState> {
    let placeholder = placeholder.to_string();
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

fn multi(
    window: &mut Window,
    cx: &mut Context<GitApp>,
    rows: usize,
    placeholder: &str,
) -> Entity<InputState> {
    let placeholder = placeholder.to_string();
    cx.new(|cx| {
        InputState::new(window, cx)
            .multi_line(true)
            .rows(rows)
            .placeholder(placeholder)
    })
}

impl GitApp {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        options: StartOptions,
        handoff: Option<better_ui::restart::Handoff>,
    ) -> Self {
        let git: Arc<dyn git_platform::Git> = Arc::new(git_platform::SystemGit);
        let mut session = GitSession::new(git, Files::defaults());

        // The command line wins over the stored choice.
        let locale = match options.locale {
            Locale::System => Locale::from_argument(&session.settings.language)
                .unwrap_or(Locale::System)
                .resolved(),
            other => other.resolved(),
        };
        let appearance = options.appearance.unwrap_or_else(|| {
            Appearance::from_argument(&session.settings.appearance).unwrap_or_default()
        });
        crate::theme::apply(appearance, window, cx);
        // The settings page shows what the window is in, including a choice
        // made on the command line (which is not saved).
        session.settings.appearance = appearance.as_str().to_string();
        if options.locale != Locale::System {
            session.settings.language = match locale {
                Locale::ZhTw => "zh-TW",
                _ => "en-US",
            }
            .to_string();
        }

        let c = copy(locale);
        let inputs = Inputs {
            summary: field(window, cx, c.summary_placeholder),
            description: multi(window, cx, 3, c.description_placeholder),
            co_authors: multi(window, cx, 2, c.co_authors_placeholder),
            filter: field(window, cx, c.filter_changes),
            repo_search: field(window, cx, c.filter_repositories),
            branch_search: field(window, cx, c.filter_branches),
            dialog: [
                field(window, cx, ""),
                field(window, cx, ""),
                multi(window, cx, 4, ""),
            ],
            editor: field(window, cx, "code"),
            terminal: field(window, cx, "gnome-terminal"),
            git_name: field(window, cx, ""),
            git_email: field(window, cx, ""),
            git_branch: field(window, cx, "main"),
        };
        let editor = session.settings.editor.clone();
        let terminal = session.settings.terminal.clone();
        inputs
            .editor
            .update(cx, |s, cx| s.set_value(editor, window, cx));
        inputs
            .terminal
            .update(cx, |s, cx| s.set_value(terminal, window, cx));

        let mut subscriptions = vec![
            cx.subscribe_in(
                &inputs.summary,
                window,
                |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        if let Some(repo) = this.session.repo.as_mut() {
                            repo.form.summary = state.read(cx).value().to_string();
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.commit(window, cx),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &inputs.description,
                window,
                |this, state, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event
                        && let Some(repo) = this.session.repo.as_mut()
                    {
                        repo.form.description = state.read(cx).value().to_string();
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &inputs.co_authors,
                window,
                |this, state, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event
                        && let Some(repo) = this.session.repo.as_mut()
                    {
                        repo.form.co_authors = state.read(cx).value().to_string();
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &inputs.filter,
                window,
                |this, state, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        let text = state.read(cx).value().to_string();
                        this.session.set_filter(&text);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &inputs.editor,
                window,
                |this, state, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        this.session.settings.editor = state.read(cx).value().to_string();
                        this.session.save_settings();
                    }
                },
            ),
            cx.subscribe_in(
                &inputs.terminal,
                window,
                |this, state, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        this.session.settings.terminal = state.read(cx).value().to_string();
                        this.session.save_settings();
                    }
                },
            ),
        ];
        // A dialog field that has been typed in may say it is empty; one
        // the person has not reached yet does not.
        for (index, input) in inputs.dialog.clone().into_iter().enumerate() {
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, _, event: &InputEvent, _, _| {
                    if let InputEvent::Change = event {
                        this.dialog_touched[index] = true;
                    }
                },
            ));
        }
        for input in [
            inputs.repo_search.clone(),
            inputs.branch_search.clone(),
            inputs.dialog[0].clone(),
            inputs.dialog[1].clone(),
        ] {
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } if this.session.dialog.is_some() => {
                        this.confirm_dialog(window, cx)
                    }
                    _ => {}
                },
            ));
        }
        subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.session.refresh();
                this.drive(window, cx);
            }
        }));

        let (graph, graph_subscription) =
            crate::graph_pane::GraphUi::new(window, cx, crate::graph::graph_copy(locale));
        subscriptions.push(graph_subscription);

        // A restart after an update puts the view back; it wins over the
        // path the program was first started with, which is the same one or
        // one the person has since moved away from.
        let restored: Option<crate::restart::GitView> = handoff.as_ref().and_then(|h| h.state());
        match &restored {
            Some(view) if view.session.repository.is_some() || view.session.settings_page => {
                session.restore_view(view.session.clone());
            }
            _ => {
                if let Some(path) = options.path {
                    session.open(absolute(path));
                }
            }
        }
        if let Some(view) = &restored {
            let filter = view.session.filter.clone();
            inputs
                .filter
                .update(cx, |s, cx| s.set_value(filter, window, cx));
        }
        let update_restart = better_ui::restart::UpdateRestart::new(
            APP_ID,
            "Better Git",
            "better-git",
            env!("CARGO_PKG_VERSION"),
            handoff.as_ref(),
            window,
            cx,
        );

        let mut app = Self {
            session,
            locale,
            inputs,
            diff_scroll: UniformListScrollHandle::new(),
            graph,
            focus_handle: cx.focus_handle(),
            flyout: None,
            branch_tab: 0,
            app_menu: [new_state(cx), new_state(cx), new_state(cx), new_state(cx)],
            context_menu: new_state(cx),
            context: None,
            show_filter: false,
            show_commit_options: false,
            settings_page: 0,
            notice_toast: None,
            dialog_touched: [false; 3],
            dialog_check: std::cell::RefCell::new(None),
            editor_name: std::cell::RefCell::new(None),
            live: crate::live::Live::default(),
            dialog_seen: None,
            form_generation: 0,
            config_loaded: false,
            last_poll: 0,
            update_restart,
            restored_diff_scroll: restored.as_ref().map(|view| view.diff_scroll),
            _subscriptions: subscriptions,
        };
        if let Some(view) = restored {
            app.show_filter = view.show_filter;
            app.show_commit_options = view.show_commit_options;
            app.settings_page = view.settings_section;
            app.branch_tab = view.branch_tab;
        }
        app.restore_account(window, cx);
        app.start_timer(window, cx);
        app
    }

    pub(crate) fn copy(&self) -> &'static Copy {
        copy(self.locale)
    }

    pub(crate) fn words(&self) -> better_ui::fluent::words::Locale {
        match self.locale {
            Locale::ZhTw => better_ui::fluent::words::Locale::TraditionalChinese,
            _ => better_ui::fluent::words::Locale::English,
        }
    }

    /// A handler for a fluent control that runs `f` on the app and then
    /// drives the session.
    pub(crate) fn act(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |window, cx| {
            let _ = weak.update(cx, |this, cx| {
                f(this, window, cx);
                this.drive(window, cx);
                cx.notify();
            });
        }
    }

    pub(crate) fn act_with<T: 'static>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(T, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |value, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                f(this, value, window, cx);
                this.drive(window, cx);
                cx.notify();
            });
        }
    }

    /// Runs the next git job and the next GitHub request on background
    /// threads, when one is waiting and none of its kind is running, and
    /// comes back for the one after.
    pub(crate) fn drive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(job) = self.session.next_job() {
            let git = self.session.git();
            let work = cx.background_spawn(async move { job.run(git.as_ref()) });
            cx.spawn_in(window, async move |this, cx| {
                let outcome = work.await;
                this.update_in(cx, |app, window, cx| {
                    app.session.apply(outcome);
                    app.after_change(window, cx);
                    app.drive(window, cx);
                })
                .ok();
            })
            .detach();
        }
        if let Some(job) = self.session.next_background() {
            let git = self.session.git();
            let work = cx.background_spawn(async move { job.run(git.as_ref()) });
            cx.spawn_in(window, async move |this, cx| {
                let outcome = work.await;
                this.update_in(cx, |app, window, cx| {
                    app.session.apply_background(outcome);
                    app.drive(window, cx);
                })
                .ok();
            })
            .detach();
        }
        if let Some(job) = self.session.next_graph() {
            let git = self.session.git();
            let work = cx.background_spawn(async move { job.run(git.as_ref()) });
            cx.spawn_in(window, async move |this, cx| {
                let outcome = work.await;
                this.update_in(cx, |app, window, cx| {
                    app.session.apply_graph(outcome);
                    app.drive(window, cx);
                })
                .ok();
            })
            .detach();
        }
        if let Some(request) = self.session.next_gh() {
            let work = cx.background_spawn(async move { request.run(crate::github::now()) });
            cx.spawn_in(window, async move |this, cx| {
                let answer = work.await;
                this.update_in(cx, |app, window, cx| {
                    app.session.apply_gh(answer);
                    app.after_change(window, cx);
                    app.drive(window, cx);
                })
                .ok();
            })
            .detach();
        }
        // A click or key that changed the session reaches the text fields in
        // the same frame, not only a finished job (amend fills the form at
        // once).
        self.after_change(window, cx);
        self.sync_dialog(window, cx);
        self.follow_repository(window, cx);
        cx.notify();
    }

    /// What follows any answer: the keyring, desktop notifications, and the
    /// text fields the session changed.
    pub(crate) fn after_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for change in std::mem::take(&mut self.session.keyring_changes) {
            self.write_keyring(change, cx);
        }
        for notice in std::mem::take(&mut self.session.desktop_notices) {
            let (summary, body) = self.copy().pull_notice(&notice);
            crate::notifications::send_desktop(&summary, &body);
        }
        self.sync_form(window, cx);
        if let Some(offset) = self.restored_diff_scroll
            && self
                .session
                .repo
                .as_ref()
                .is_some_and(|repo| repo.diff.is_some())
        {
            self.restored_diff_scroll = None;
            let handle = self.diff_scroll.0.borrow().base_handle.clone();
            handle.set_offset(point(px(0.0), px(-offset.max(0.0))));
        }
        if self.session.git_config.loaded && !self.config_loaded {
            self.config_loaded = true;
            let config = self.session.git_config.clone();
            for (input, key) in [
                (self.inputs.git_name.clone(), "user.name"),
                (self.inputs.git_email.clone(), "user.email"),
                (self.inputs.git_branch.clone(), "init.defaultBranch"),
            ] {
                Self::set_input(&input, config.get(key), window, cx);
            }
        }
    }

    /// Puts the identity fields back to what git's configuration says.
    pub(crate) fn revert_git_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.session.git_config.clone();
        for (input, key) in [
            (self.inputs.git_name.clone(), "user.name"),
            (self.inputs.git_email.clone(), "user.email"),
            (self.inputs.git_branch.clone(), "init.defaultBranch"),
        ] {
            Self::set_input(&input, config.get(key), window, cx);
        }
    }

    pub(crate) fn set_input(
        input: &Entity<InputState>,
        value: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        let value = value.to_string();
        input.update(cx, |state, cx| state.set_value(value, window, cx));
    }

    /// Copies the commit form into the text fields after the session
    /// changed it (cleared after a commit, a message put back).
    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.session.repo.as_ref().map(|r| r.form.clone()) else {
            return;
        };
        if form.generation == self.form_generation {
            return;
        }
        self.form_generation = form.generation;
        Self::set_input(&self.inputs.summary, &form.summary, window, cx);
        Self::set_input(&self.inputs.description, &form.description, window, cx);
        Self::set_input(&self.inputs.co_authors, &form.co_authors, window, cx);
    }

    /// Fills a newly opened dialog's fields.
    fn sync_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.dialog == self.dialog_seen {
            return;
        }
        let opened_new = match (&self.session.dialog, &self.dialog_seen) {
            (Some(a), Some(b)) => std::mem::discriminant(a) != std::mem::discriminant(b),
            (Some(_), None) => true,
            _ => false,
        };
        let closed = self.session.dialog.is_none();
        self.dialog_seen = self.session.dialog.clone();
        if closed {
            // The dialog's focus went with it; shortcuts need it back.
            self.focus_handle.focus(window, cx);
            return;
        }
        if !opened_new {
            return;
        }
        let values = self.dialog_defaults();
        for (input, value) in self.inputs.dialog.clone().iter().zip(values) {
            Self::set_input(input, &value, window, cx);
        }
        // Filling the fields is not the person typing in them.
        self.dialog_touched = [false; 3];
        // After the dialog has drawn once: the dialog takes focus for its
        // own trap on its first frame, and the first field takes it next.
        if crate::dialogs::field_count(self.session.dialog.as_ref().expect("opened")) > 0 {
            let first = self.inputs.dialog[0].clone();
            cx.on_next_frame(window, move |_, window, cx| {
                first.update(cx, |state, cx| state.focus(window, cx));
            });
        }
    }

    fn dialog_defaults(&self) -> [String; 3] {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let clone_folder = self
            .session
            .settings
            .clone_folder(home.as_deref())
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let repo = self.session.repo.as_ref();
        let one = |s: String| [s, String::new(), String::new()];
        match &self.session.dialog {
            Some(Dialog::Clone { .. }) | Some(Dialog::NewRepository { .. }) => {
                [String::new(), clone_folder, String::new()]
            }
            Some(Dialog::Publish { .. }) => one(repo.map(|r| r.name()).unwrap_or_default()),
            Some(Dialog::Alias { path }) => one(self
                .session
                .repositories
                .alias(path)
                .unwrap_or("")
                .to_string()),
            Some(Dialog::RenameBranch { from }) => one(from.clone()),
            Some(Dialog::Squash { from, to }) => [
                String::new(),
                String::new(),
                self.session.squash_message(*from, *to),
            ],
            Some(Dialog::AddWorktree { .. }) => one(repo
                .and_then(|r| {
                    r.root
                        .parent()
                        .map(|p| p.join(format!("{}-worktree", r.name())))
                })
                .map(|p| p.display().to_string())
                .unwrap_or_default()),
            Some(Dialog::RenameWorktree { path }) => one(path.display().to_string()),
            Some(Dialog::Identity) => {
                // The account's profile when it has been read, its login
                // otherwise.
                let github = &self.session.github;
                let user = github.account.as_ref().map(|account| {
                    github
                        .profile
                        .clone()
                        .filter(|user| user.login == account.login)
                        .unwrap_or_else(|| github_api::User {
                            login: account.login.clone(),
                            ..Default::default()
                        })
                });
                let account = github.account.as_ref().map(|a| &a.host).zip(user.as_ref());
                let draft = crate::identity::prefill(
                    self.session.git_config.get("user.name"),
                    self.session.git_config.get("user.email"),
                    account,
                );
                [draft.name, draft.email, String::new()]
            }
            Some(Dialog::CreatePull { .. }) => one(repo
                .and_then(|r| r.history.commits.first().map(|c| c.summary.clone()))
                .or_else(|| repo.and_then(|r| r.branch().map(str::to_string)))
                .unwrap_or_default()),
            _ => Default::default(),
        }
    }

    pub(crate) fn dialog_texts(&self, cx: &App) -> Vec<String> {
        self.inputs
            .dialog
            .iter()
            .map(|i| i.read(cx).value().to_string())
            .collect()
    }

    // --- timers -------------------------------------------------------------

    /// Once a second: poll a sign-in that is waiting, and start the
    /// background fetch when it is due.
    fn start_timer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this
                    .update_in(cx, |app, window, cx| app.tick(window, cx))
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = crate::github::now();
        let mut changed = false;
        if let crate::github::SignIn::Waiting { interval, .. } = &self.session.github.sign_in
            && now - self.last_poll >= *interval as i64
        {
            self.last_poll = now;
            self.session.poll_sign_in();
            changed = true;
        }
        if self.session.fetch_due(now) {
            self.session.fetch(true, now);
            changed = true;
        }
        if changed {
            self.drive(window, cx);
        }
    }

    // --- keyring ------------------------------------------------------------

    fn host_from_settings(&self) -> Host {
        let name = self.session.settings.account_host.trim();
        let host = if name.is_empty() {
            Host::github()
        } else {
            Host::enterprise(name).unwrap_or_else(Host::github)
        };
        host.with_client_id(&self.session.settings.account_client_id)
    }

    /// Reads the token stored for the account's host, if any.
    /// Nothing is read until the person has signed in once, so a desktop
    /// without a keyring is never asked.
    fn restore_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.settings.account_host.trim().is_empty() {
            return;
        }
        let host = self.host_from_settings();
        let read = cx.read_credentials(&credential_url(&host.name));
        cx.spawn_in(window, async move |this, cx| {
            let found = read.await;
            this.update_in(cx, |app, window, cx| {
                match found {
                    Ok(Some((login, secret))) => {
                        let secret = String::from_utf8_lossy(&secret).into_owned();
                        match Token::from_secret(&secret) {
                            Some(token) if !token.refresh_expired(crate::github::now()) => {
                                app.session.restore_account(Account { host, login, token });
                            }
                            // Expired for good: forget it.
                            Some(_) => app
                                .session
                                .keyring_changes
                                .push(KeyringChange::Delete(host.name.clone())),
                            None => {}
                        }
                    }
                    Ok(None) => {}
                    Err(error) => app.session.keyring_failed(error.to_string()),
                }
                app.after_change(window, cx);
                app.drive(window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn write_keyring(&mut self, change: KeyringChange, cx: &mut Context<Self>) {
        let task = match &change {
            KeyringChange::Store(account) => {
                self.session.settings.account_host = account.host.name.clone();
                self.session.settings.account_client_id = account
                    .host
                    .client_id
                    .clone()
                    .filter(|id| id != github_api::CLIENT_ID)
                    .unwrap_or_default();
                self.session.save_settings();
                cx.write_credentials(
                    &credential_url(&account.host.name),
                    &account.login,
                    crate::github::secret(account).as_bytes(),
                )
            }
            KeyringChange::Delete(host) => {
                self.session.settings.account_host.clear();
                self.session.settings.account_client_id.clear();
                self.session.save_settings();
                cx.delete_credentials(&credential_url(host))
            }
        };
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                this.update(cx, |app, cx| {
                    app.session.keyring_failed(error.to_string());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // --- actions ------------------------------------------------------------

    pub(crate) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.commit();
        self.drive(window, cx);
    }

    pub(crate) fn launch(&mut self, launch: Option<Launch>) {
        let Some(launch) = launch else {
            self.session
                .launch_failed(self.copy().open_in_editor.to_string());
            return;
        };
        if let Err(error) = launch.spawn() {
            self.session
                .launch_failed(format!("{}: {error}", launch.program));
        }
    }

    fn exists(program: &str) -> bool {
        settings::on_path(program, std::env::var_os("PATH").as_deref())
    }

    pub(crate) fn open_in_editor(&mut self, path: Option<PathBuf>) {
        let target = path.or_else(|| self.session.repo.as_ref().map(|r| r.root.clone()));
        if let Some(target) = target {
            let launch = settings::editor_launch(&self.session.settings, &target, Self::exists);
            self.launch(launch);
        }
    }

    pub(crate) fn open_terminal(&mut self) {
        if let Some(root) = self.session.repo.as_ref().map(|r| r.root.clone()) {
            let launch = settings::terminal_launch(&self.session.settings, &root, Self::exists);
            self.launch(launch);
        }
    }

    pub(crate) fn show_in_files(&mut self, path: Option<PathBuf>) {
        let folder = path.or_else(|| self.session.repo.as_ref().map(|r| r.root.clone()));
        if let Some(folder) = folder {
            self.launch(Some(settings::files_launch(&folder, Self::exists)));
        }
    }

    pub(crate) fn open_url(&mut self, url: &str) {
        self.launch(Some(settings::browser_launch(url)));
    }

    pub(crate) fn copy_text(&self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Shows the folder picker and hands the chosen folder to `then`.
    pub(crate) fn choose_folder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.copy().choose.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let chosen = answer.await;
            this.update_in(cx, |app, window, cx| {
                match chosen {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            then(app, path, window, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => app.session.platform_failed(error.to_string()),
                    Err(error) => app.session.platform_failed(error.to_string()),
                }
                app.drive(window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// The dialog's primary button, or Enter in its field.
    pub(crate) fn confirm_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.session.dialog.clone() else {
            return;
        };
        // The button is off and the field says why (every field, now that
        // the person tried); Enter does nothing more.
        if self.dialog_blocked(cx).is_some() {
            self.dialog_touched = [true; 3];
            cx.notify();
            return;
        }
        let texts = self.dialog_texts(cx);
        crate::render::confirm(self, dialog, texts, window, cx);
        self.drive(window, cx);
    }

    pub(crate) fn run_action(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_repo = self.session.repo.is_some();
        let branch = self
            .session
            .repo
            .as_ref()
            .and_then(|r| r.branch().map(str::to_string));
        match action {
            Action::Settings => self.session.open_settings(),
            Action::NewRepository => {
                self.session.dialog = Some(Dialog::NewRepository {
                    gitignore: None,
                    license: None,
                    readme: true,
                });
                self.session.load_templates();
            }
            Action::AddLocalRepository => self.session.dialog = Some(Dialog::AddLocal),
            Action::CloneRepository => self.session.open_clone_dialog(),
            Action::ShowChanges => self.session.set_tab(Tab::Changes),
            Action::ShowHistory => self.session.set_tab(Tab::History),
            Action::ShowGraph => self.session.set_tab(Tab::Graph),
            Action::FindInGraph if self.graph_find_allowed(window, cx) => {
                self.open_graph_find(window, cx);
            }
            Action::RepositoryList => self.flyout = Some(Flyout::Repositories),
            Action::BranchList if has_repo => {
                self.flyout = Some(Flyout::Branches);
                self.branch_tab = 0;
            }
            Action::PullRequest if has_repo => {
                if let Some(pull) = self.session.current_pull().cloned() {
                    self.open_url(&pull.html_url);
                } else {
                    self.session.dialog = Some(Dialog::CreatePull {
                        base: None,
                        draft: false,
                    });
                }
            }
            Action::WorktreeList if has_repo => self.session.dialog = Some(Dialog::Worktrees),
            Action::GoToSummary => {
                self.inputs
                    .summary
                    .update(cx, |state, cx| state.focus(window, cx));
            }
            Action::ToggleStash => {
                let stash = self.session.branch_stash().map(|x| x.name.clone());
                let shown = self
                    .session
                    .repo
                    .as_ref()
                    .is_some_and(|r| r.stash.is_some());
                match (shown, stash) {
                    (true, _) => self.session.close_stash(),
                    (false, Some(name)) => self.session.view_stash(name),
                    _ => {}
                }
            }
            Action::ToggleFilter => self.show_filter = !self.show_filter,
            Action::Refresh => self.session.refresh(),
            Action::Push => self.sync_button(false),
            Action::Pull => self.session.pull(),
            Action::Fetch => self.session.fetch(false, crate::github::now()),
            Action::RemoveRepository => {
                if let Some(root) = self.session.repo.as_ref().map(|r| r.root.clone()) {
                    self.session.ask_remove_repository(root);
                }
            }
            Action::ViewOnGitHub => {
                if let Some(url) = self
                    .session
                    .repo
                    .as_ref()
                    .and_then(|r| r.hosted.as_ref())
                    .map(|h| h.web_url())
                {
                    self.open_url(&url);
                }
            }
            Action::OpenInTerminal => self.open_terminal(),
            Action::ShowInFiles => self.show_in_files(None),
            Action::OpenInEditor => self.open_in_editor(None),
            Action::CreateIssue if has_repo => self.session.dialog = Some(Dialog::CreateIssue),
            Action::NewWorktree if has_repo => {
                self.session.dialog = Some(Dialog::AddWorktree { create: true })
            }
            Action::NewBranch if has_repo => {
                self.session.dialog = Some(Dialog::NewBranch {
                    start: None,
                    start_label: None,
                })
            }
            Action::RenameBranch => {
                if let Some(branch) = branch {
                    self.session.dialog = Some(Dialog::RenameBranch { from: branch });
                }
            }
            Action::DeleteBranch => {
                if let Some(branch) = branch {
                    self.session.ask_delete_branch(branch);
                }
            }
            Action::DiscardAll => self.session.ask_discard_all(),
            Action::StashAll => self.session.stash_all(),
            Action::UpdateFromDefault => self.session.update_from_default(),
            Action::CompareToBranch => self
                .session
                .pick_branch(crate::dialogs::BranchPurpose::Compare),
            Action::MergeIntoCurrent => self
                .session
                .pick_branch(crate::dialogs::BranchPurpose::Merge),
            Action::SquashAndMerge => self
                .session
                .pick_branch(crate::dialogs::BranchPurpose::SquashMerge),
            Action::RebaseCurrent => self
                .session
                .pick_branch(crate::dialogs::BranchPurpose::Rebase),
            Action::CompareOnGitHub | Action::ViewBranchOnGitHub => {
                if let Some(repo) = self.session.repo.as_ref()
                    && let (Some(hosted), Some(branch)) = (&repo.hosted, repo.branch())
                {
                    let url = if action == Action::CompareOnGitHub {
                        crate::github::compare_url(
                            hosted,
                            &repo.default_branch().unwrap_or_default(),
                            branch,
                        )
                    } else {
                        crate::github::branch_url(hosted, branch)
                    };
                    self.open_url(&url);
                }
            }
            Action::Commit => self.session.commit(),
            _ => {}
        }
        self.drive(window, cx);
    }

    /// The toolbar's sync button: publish, pull, push, or fetch.
    pub(crate) fn sync_button(&mut self, fetch_when_even: bool) {
        use crate::remote::SyncAction;
        let Some(action) = self.session.repo.as_ref().map(|r| {
            crate::remote::sync_action(&r.status, !r.info.remotes.is_empty(), r.branch().is_none())
        }) else {
            return;
        };
        match action {
            SyncAction::PublishRepository => {
                if self.session.github.account.is_none() {
                    self.session.start_sign_in(Host::github());
                } else {
                    self.session.dialog = Some(Dialog::Publish {
                        private: true,
                        organization: None,
                    });
                    self.session.load_organizations();
                }
            }
            SyncAction::PublishBranch | SyncAction::Push { .. } => self.session.push(false),
            SyncAction::Pull { .. } => self.session.pull(),
            SyncAction::Fetch if fetch_when_even => self.session.fetch(false, crate::github::now()),
            SyncAction::Fetch => self.session.push(false),
            SyncAction::None => {}
        }
    }

    pub(crate) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &event.keystroke;
        let Some(action) = keys::action_for(
            &k.key,
            k.modifiers.control,
            k.modifiers.shift,
            k.modifiers.alt,
        ) else {
            return;
        };
        // Ctrl+F belongs to the Graph tab's find, and only while the tab is
        // shown and no other text field has the keyboard.
        if action == Action::FindInGraph && !self.graph_find_allowed(window, cx) {
            return;
        }
        cx.stop_propagation();
        self.run_action(action, window, cx);
    }

    pub(crate) fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.settings.appearance = appearance.as_str().to_string();
        self.session.save_settings();
        crate::theme::apply(appearance, window, cx);
    }

    pub(crate) fn set_locale(&mut self, locale: Locale) {
        self.locale = locale.resolved();
        self.session.settings.language = match locale {
            Locale::ZhTw => "zh-TW",
            Locale::EnUs => "en-US",
            Locale::System => "system",
        }
        .to_string();
        self.session.save_settings();
    }
}

/// A relative path from the command line, made absolute against the working
/// directory so the lists never store one.
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

        let handoff = better_ui::restart::take_handoff(APP_ID);
        let window_options = WindowOptions {
            window_bounds: better_ui::restart::window_bounds(handoff.as_ref())
                .or_else(|| Some(WindowBounds::centered(size(px(1_280.0), px(800.0)), cx))),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options(APP_ID)
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| GitApp::new(window, cx, options, handoff));
                view.update(cx, |app, cx| app.drive(window, cx));
                cx.new(|cx| better_ui::window_chrome::root(view, window, cx))
            })
            .expect("failed to open the Better Git window");
        })
        .detach();
    });
}

impl better_ui::restart::Restartable for GitApp {
    type State = crate::restart::GitView;

    fn update_restart(&mut self) -> &mut better_ui::restart::UpdateRestart {
        &mut self.update_restart
    }

    fn restart_blocked(&self, cx: &App) -> Option<better_ui::restart::Blocker> {
        if let Some(blocker) = self.session.restart_blocked() {
            return Some(blocker);
        }
        let config = &self.session.git_config;
        let typed = [
            self.inputs.git_name.read(cx).value().to_string(),
            self.inputs.git_email.read(cx).value().to_string(),
            self.inputs.git_branch.read(cx).value().to_string(),
        ];
        crate::restart::settings_unsaved(
            self.config_loaded,
            [&typed[0], &typed[1], &typed[2]],
            [
                config.get("user.name"),
                config.get("user.email"),
                config.get("init.defaultBranch"),
            ],
        )
        .then_some(better_ui::restart::Blocker::UnsavedChanges)
    }

    fn restart_state(&self, _window: &Window, _cx: &App) -> crate::restart::GitView {
        let offset = self.diff_scroll.0.borrow().base_handle.offset();
        crate::restart::GitView {
            session: self.session.view(),
            show_filter: self.show_filter,
            show_commit_options: self.show_commit_options,
            settings_section: self.settings_page,
            branch_tab: self.branch_tab,
            diff_scroll: -f32::from(offset.y),
        }
    }
}
