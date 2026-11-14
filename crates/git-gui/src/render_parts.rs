//! Parts of the window drawn outside `render.rs`: a dialog field's reason,
//! the identity step, the toolbar's shortcut buttons, the notice toast, and the
//! toolbar's worktree dropdown. As in `render.rs`, nothing here
//! decides anything; the decisions are in `dialog_check`, `identity`,
//! `shortcuts`, `worktree_links`, and `better_ui::fluent::toast`.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, Div, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::input;
use gpui_component::{Icon, IconName};

use better_ui::fluent::button::{Button, ButtonKind, DropDownButton};
use better_ui::fluent::dialog::Flyout;
use better_ui::fluent::info_bar::Severity;
use better_ui::fluent::menu::{MenuEntry, MenuItem};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::toast::{ToastSpec, ToastWords};
use better_ui::fluent::{BODY, BODY_STRONG, CAPTION, CODE, Palette, label, radius, spacing};

use crate::app::GitApp;
use crate::dialog_check::{self, Blocked};
use crate::i18n::{Copy, fill};
use crate::shortcuts::{self, Shortcut};

impl GitApp {
    /// Why the open dialog's primary button is off, if it is. The answer is
    /// kept until the dialog or its text changes, so the disk is not read on
    /// every frame.
    pub(crate) fn dialog_blocked(&self, cx: &App) -> Option<Blocked> {
        let dialog = self.session.dialog.as_ref()?;
        let texts = self.dialog_texts(cx);
        if let Some(last) = self.dialog_check.borrow().as_ref()
            && last.dialog == *dialog
            && last.texts == texts
        {
            return last.blocked;
        }
        let (branches, tags) = self
            .session
            .repo
            .as_ref()
            .map(|r| (r.info.branches.as_slice(), r.info.tags.as_slice()))
            .unwrap_or_default();
        let blocked = dialog_check::check(dialog, &texts, branches, tags, &dialog_check::folder);
        *self.dialog_check.borrow_mut() = Some(DialogCheck {
            dialog: dialog.clone(),
            texts,
            blocked,
        });
        blocked
    }

    /// The reason to draw under each of the three dialog fields. "This
    /// can't be empty" waits until the person has typed in that field.
    pub(crate) fn field_errors(&self, blocked: Option<Blocked>) -> [Option<&'static str>; 3] {
        let mut errors = [None; 3];
        if let Some(Blocked {
            reason,
            field: Some(field),
        }) = blocked
            && field < errors.len()
            && (reason != crate::dialogs::Invalid::Empty || self.dialog_touched[field])
        {
            errors[field] = Some(self.copy().invalid_message(reason));
        }
        errors
    }

    /// The editor's name, found again only when the setting changes.
    pub(crate) fn editor_display_name(&self) -> String {
        let setting = &self.session.settings.editor;
        if let Some((for_setting, name)) = self.editor_name.borrow().as_ref()
            && for_setting == setting
        {
            return name
                .clone()
                .unwrap_or_else(|| self.copy().editor_fallback_name().to_string());
        }
        let path = std::env::var_os("PATH");
        let name = shortcuts::editor_name(setting, |program| {
            crate::settings::on_path(program, path.as_deref())
        });
        *self.editor_name.borrow_mut() = Some((setting.clone(), name.clone()));
        name.unwrap_or_else(|| self.copy().editor_fallback_name().to_string())
    }

    /// Each shortcut button shown, with its label.
    pub(crate) fn shortcut_labels(&self) -> Vec<(Shortcut, String)> {
        let c = self.copy();
        let hosted = self
            .session
            .repo
            .as_ref()
            .is_some_and(|r| r.hosted.is_some());
        let editor = self.editor_display_name();
        shortcuts::shown(hosted)
            .into_iter()
            .map(|shortcut| {
                let name = match shortcut {
                    Shortcut::Editor => fill(c.open_in_named_editor(), &[("editor", &editor)]),
                    Shortcut::Files => c.show_in_better_files().to_string(),
                    Shortcut::Terminal => c.open_in_terminal_short().to_string(),
                    Shortcut::GitHub => c.view_on_github_short().to_string(),
                };
                (shortcut, name)
            })
            .collect()
    }

    /// The identity step's body: what it is for, the two fields, and the
    /// two settings it will write, read from the fields as they are typed.
    pub(crate) fn identity_content(
        &self,
        texts: &[String],
        name: TextBox,
        email: TextBox,
        p: &'static Palette,
        c: &'static Copy,
    ) -> Vec<AnyElement> {
        let text = |i: usize| texts.get(i).map(|t| t.trim()).unwrap_or("").to_string();
        // A value Git already has is kept: shown, not editable, not written.
        let configured = [
            self.session.git_config.get("user.name"),
            self.session.git_config.get("user.email"),
        ];
        let kept = configured.map(crate::identity::kept);
        let mut lines = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(p.layer)
            .border_1()
            .border_color(p.card_stroke);
        for (index, (key, value)) in crate::identity::writes(&text(0), &text(1))
            .into_iter()
            .enumerate()
        {
            let (word, color) = if kept[index] {
                (c.identity_kept(), p.text_secondary)
            } else {
                (c.identity_new(), p.accent_text)
            };
            lines = lines.child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .child(
                        label(format!("{key} = {value}"), CODE, p.text_primary).whitespace_normal(),
                    )
                    .child(label(word, CAPTION, color)),
            );
        }
        vec![
            label(c.identity_body(), BODY, p.text_secondary)
                .whitespace_normal()
                .into_any_element(),
            name.disabled(kept[0]).into_any_element(),
            email
                .disabled(kept[1])
                .description(c.identity_email_hint())
                .into_any_element(),
            label(c.identity_writes(), BODY_STRONG, p.text_primary).into_any_element(),
            lines.into_any_element(),
        ]
    }

    /// The four buttons at the toolbar's right end: Open in the editor,
    /// Show in Better Files, Open in Terminal, and View on GitHub for a
    /// repository hosted there. Narrow windows get icons with tooltips.
    pub(crate) fn shortcut_buttons(
        &self,
        compact: bool,
        p: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        if self.session.repo.is_none() {
            return div();
        }
        // With labels it gives way before Settings does: never wider than
        // what is left. As icons it keeps its width, and the drop-down
        // items shrink instead, so every button stays on screen.
        let mut row = div()
            .flex()
            .when(compact, |d| d.flex_none())
            .when(!compact, |d| d.flex_shrink(1.0).min_w_0().overflow_hidden())
            .items_center()
            .gap(px(4.0))
            .px(spacing::SM)
            .h_full()
            .border_l_1()
            .border_color(p.divider);
        for (shortcut, name) in self.shortcut_labels() {
            let keys = crate::keys::label(shortcut.action());
            let tooltip = format!("{name} ({keys})");
            let icon = match shortcut {
                Shortcut::Editor => IconName::File,
                Shortcut::Files => IconName::FolderOpen,
                Shortcut::Terminal => IconName::SquareTerminal,
                Shortcut::GitHub => IconName::Github,
            };
            let mut button =
                Button::new(("toolbar-shortcut", shortcut as usize), ButtonKind::Subtle)
                    .icon(icon)
                    .tooltip(tooltip);
            if !compact {
                button = button.label(name);
            }
            let action = shortcut.action();
            row = row.child(button.on_click(Self::act(cx, move |this, window, cx| {
                this.run_action(action, window, cx)
            })));
        }
        row
    }

    /// Shows the session's notice as a toast on the window's one toast host
    /// (`better_ui::fluent::toast`): a short-lived note at the bottom right
    /// for something the window does not otherwise show, with its link or its
    /// "restore" as the toast's action. A new notice replaces the one shown;
    /// a notice cleared takes its toast away; a toast that went away by itself
    /// clears the notice.
    pub(crate) fn sync_notice_toast(&mut self, cx: &mut Context<Self>) {
        let host = self.update_restart.toast_host().clone();
        if let Some((_, id)) = &self.notice_toast
            && !host.read(cx).contains(*id)
        {
            let (shown, _) = self.notice_toast.take().expect("checked above");
            if self.session.notice.as_ref() == Some(&shown) {
                self.session.dismiss_notice();
            }
        }
        if self.notice_toast.as_ref().map(|(notice, _)| notice) == self.session.notice.as_ref() {
            return;
        }
        if let Some((_, id)) = self.notice_toast.take() {
            host.update(cx, |host, cx| host.dismiss(id, cx));
        }
        let Some(notice) = self.session.notice.clone() else {
            return;
        };
        let words_of = notice.clone();
        let mut spec = ToastSpec::timed(Severity::Success, move |locale| ToastWords {
            title: crate::i18n::copy(git_locale(locale)).notice(&words_of),
            message: None,
        })
        .closable(true);
        if let Some(url) = notice.link() {
            spec = spec.action(
                |locale| crate::i18n::copy(git_locale(locale)).open_link.to_string(),
                Self::act(cx, move |this, _, _| this.open_url(&url)),
            );
        }
        if let crate::session::Notice::StashWaiting(name) = &notice {
            let name = name.clone();
            spec = spec.action(
                |locale| crate::i18n::copy(git_locale(locale)).restore.to_string(),
                Self::act(cx, move |this, _, _| this.session.view_stash(name.clone())),
            );
        }
        let id = host.update(cx, |host, cx| host.push(spec, cx));
        self.notice_toast = Some((notice, id));
    }

    /// The toolbar's worktree dropdown (ticket 116): the open repository's
    /// worktrees, each named by its folder with its branch (or the commit
    /// it is detached at) beneath, the main one, locked ones, and the open
    /// one marked, a filter, and the worktree actions: add from the header;
    /// open, show in Better Files, copy the path, move, and remove from each
    /// row's More button or its context menu. The whole row opens it; a
    /// missing one opens nothing. The filter has focus; Up and Down walk
    /// the rows, Enter opens one, Tab reaches the More buttons and Add.
    pub(crate) fn worktree_flyout(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.inputs.worktree_search.read(cx).value().to_string();
        let rows = self.session.worktree_rows(&query);
        let only_main = self.names.as_ref().is_some_and(|names| names.only_main);
        let mut list = div()
            .flex()
            .flex_col()
            .max_h(px(420.0))
            .id("worktree-list")
            .overflow_y_scroll();
        if rows.is_empty() {
            list = list.child(div().px(spacing::MD).py(spacing::SM).child(label(
                c.no_matching_worktrees(),
                CAPTION,
                p.text_secondary,
            )));
        }
        for (index, w) in rows.iter().enumerate() {
            list = list.child(self.worktree_row(index, w, p, c, cx));
        }
        let content = div()
            .flex()
            .flex_col()
            .w(px(400.0))
            .gap(spacing::SM)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .child(div().flex_1().child(TextBox::search(
                        "worktree-search",
                        &self.inputs.worktree_search,
                    )))
                    .child(
                        Button::standard("worktree-add", c.add_worktree)
                            .icon(IconName::Plus)
                            .on_click(Self::act(cx, |this, _, _| {
                                this.flyout = None;
                                this.session.dialog =
                                    Some(crate::dialogs::Dialog::AddWorktree { create: true });
                            })),
                    ),
            )
            .child(label(c.worktrees_title, CAPTION, p.text_secondary))
            .child(list)
            .when(only_main && query.trim().is_empty(), |d| {
                d.child(div().px(spacing::MD).child(label(
                    c.only_main_worktree(),
                    CAPTION,
                    p.text_secondary,
                )))
            });
        Flyout::new("worktree-flyout")
            .initial_focus(self.inputs.worktree_search.focus_handle(cx))
            .on_dismiss(Self::act(cx, |this, _, _| this.flyout = None))
            .child(self.flyout_keys(content, cx))
            .into_any_element()
    }

    /// The arrow keys and Enter of a dropdown's search box walk its rows
    /// instead of its text (they are the box's own cursor actions, taken on
    /// their way down, as Better Launcher takes them).
    pub(crate) fn flyout_keys(&self, content: Div, cx: &mut Context<Self>) -> Div {
        content
            .capture_action(cx.listener(|this, _: &input::MoveDown, _, cx| {
                if this.flyout_step(true, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::MoveUp, _, cx| {
                if this.flyout_step(false, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Enter, window, cx| {
                if this.flyout_enter(window, cx) {
                    cx.stop_propagation();
                }
            }))
    }

    /// The ring around the row the arrow keys are on: WinUI's focus visual,
    /// 2px in primary text color, inset in the row.
    pub(crate) fn highlight_ring(&self, index: usize, p: &Palette) -> Option<Div> {
        (self.highlight == Some(index)).then(|| {
            div()
                .absolute()
                .top(px(1.0))
                .bottom(px(1.0))
                .left(px(4.0))
                .right(px(4.0))
                .rounded(radius::SM)
                .border_2()
                .border_color(p.text_primary)
        })
    }

    /// One row of the worktree dropdown.
    fn worktree_row(
        &self,
        index: usize,
        w: &crate::repositories::WorktreeRow,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut detail = match (&w.branch, &w.short_sha) {
            (Some(branch), _) => branch.clone(),
            (None, Some(sha)) => fill(c.worktree_detached_at(), &[("sha", sha)]),
            (None, None) => c.worktree_detached().to_string(),
        };
        if w.main {
            detail = format!("{detail} · {}", c.worktree_main);
        }
        if w.locked {
            detail = format!("{detail} · {}", c.worktree_locked());
        }
        if w.missing {
            detail = format!("{detail} · {}", c.missing);
        }
        let (open, menu) = (w.path.clone(), w.clone());
        let current = w.current;
        let entries = self.worktree_entries(w, c, cx);
        // No path tooltip over a menu opened from the row.
        let menu_open = self.context_menu.read(cx).is_open();
        crate::render::row(("worktree", index), w.current, px(48.0), p)
            .when(!menu_open, |row| {
                row.tooltip(better_ui::fluent::dialog::tooltip_builder(
                    w.path.display().to_string().into(),
                ))
            })
            .children(self.highlight_ring(index, p))
            .child(
                if w.missing {
                    Icon::new(IconName::TriangleAlert)
                } else {
                    Icon::empty().path(crate::app::WORKTREE_ICON)
                }
                .size(px(16.0))
                .text_color(if w.missing {
                    p.caution
                } else {
                    p.text_secondary
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        label(
                            w.name.clone(),
                            BODY_STRONG,
                            if w.missing {
                                p.text_disabled
                            } else {
                                p.text_primary
                            },
                        )
                        .truncate(),
                    )
                    .child(label(detail, CAPTION, p.text_secondary).truncate()),
            )
            .child(
                // Its own press, so the row under it does not open. Its menu
                // hangs from the button, whether a click or a key opened it.
                div()
                    .flex_none()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        DropDownButton::more(("worktree-more", index), c.worktree_actions())
                            .entries(entries)
                            .align_right(),
                    ),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    if this.worktree_entries(&menu, c, cx).is_empty() {
                        return;
                    }
                    this.context = Some((
                        crate::app::ContextTarget::Worktree(menu.path.clone()),
                        event.position,
                    ));
                    better_ui::fluent::menu::open_menu(&this.context_menu, None, window, cx);
                    cx.stop_propagation();
                }),
            )
            .when(!w.missing, |row| {
                row.on_click(
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        if event.is_keyboard() {
                            return;
                        }
                        this.flyout = None;
                        if !current {
                            this.session.open(open.clone());
                        }
                        this.drive(window, cx);
                    }),
                )
            })
            .into_any_element()
    }

    /// The context menu for a worktree the dropdown shows, found again by
    /// its folder; none for one that is no longer there.
    pub(crate) fn worktree_context(
        &self,
        path: &std::path::Path,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<MenuEntry> {
        match self
            .session
            .worktree_rows("")
            .iter()
            .find(|w| w.path == path)
        {
            Some(row) => self.worktree_entries(row, c, cx),
            None => Vec::new(),
        }
    }

    /// The menu entries for one worktree: [`WorktreeRow::actions`], each
    /// worded and wired. Move and Remove of a locked worktree are shown off,
    /// saying it must be unlocked first.
    ///
    /// [`WorktreeRow::actions`]: crate::repositories::WorktreeRow::actions
    pub(crate) fn worktree_entries(
        &self,
        row: &crate::repositories::WorktreeRow,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<MenuEntry> {
        use crate::repositories::WorktreeAction;
        let item = |text: &'static str, enabled: bool, cx: &mut Context<Self>, f: MenuAction| {
            MenuItem::new(text)
                .disabled(!enabled)
                .on_select(Self::act(cx, move |this, window, cx| f(this, window, cx)))
                .entry()
        };
        let mut entries = Vec::new();
        for (action, enabled) in row.actions() {
            let path = row.path.clone();
            let entry = match action {
                WorktreeAction::Open => item(
                    c.worktree_open,
                    enabled,
                    cx,
                    Box::new(move |this, _, _| {
                        this.flyout = None;
                        this.session.open(path.clone())
                    }),
                ),
                WorktreeAction::ShowInFiles => item(
                    c.show_in_files,
                    enabled,
                    cx,
                    Box::new(move |this, _, _| this.show_in_files(Some(path.clone()))),
                ),
                WorktreeAction::CopyPath => {
                    let text = path.display().to_string();
                    item(
                        c.copy_folder_path(),
                        enabled,
                        cx,
                        Box::new(move |this, _, cx| this.copy_text(text.clone(), cx)),
                    )
                }
                WorktreeAction::Move => {
                    entries.push(MenuEntry::Separator);
                    item(
                        if enabled {
                            c.rename_worktree_title
                        } else {
                            c.move_locked_worktree()
                        },
                        enabled,
                        cx,
                        Box::new(move |this, _, _| {
                            this.flyout = None;
                            this.session.dialog =
                                Some(crate::dialogs::Dialog::RenameWorktree { path: path.clone() })
                        }),
                    )
                }
                WorktreeAction::Remove => item(
                    if row.locked {
                        c.remove_locked_worktree()
                    } else {
                        c.remove_worktree_title
                    },
                    enabled,
                    cx,
                    Box::new(move |this, _, _| {
                        this.flyout = None;
                        this.session.ask_remove_worktree(path.clone())
                    }),
                ),
            };
            entries.push(entry);
        }
        entries
    }
}

/// What a worktree menu entry does.
type MenuAction = Box<dyn Fn(&mut GitApp, &mut gpui::Window, &mut Context<GitApp>)>;

/// The last dialog check: the dialog and texts it was for, and its answer.
pub(crate) struct DialogCheck {
    dialog: crate::dialogs::Dialog,
    texts: Vec<String>,
    blocked: Option<Blocked>,
}

/// The window's language for the kit's.
fn git_locale(locale: better_ui::fluent::words::Locale) -> crate::i18n::Locale {
    match locale {
        better_ui::fluent::words::Locale::TraditionalChinese => crate::i18n::Locale::ZhTw,
        better_ui::fluent::words::Locale::English => crate::i18n::Locale::EnUs,
    }
}
