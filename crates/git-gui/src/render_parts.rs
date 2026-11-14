//! Parts of the window drawn outside `render.rs`: a dialog field's reason,
//! the identity step, the toolbar's shortcut buttons, the notice toast, and the
//! repository list's worktree rows. As in `render.rs`, nothing here
//! decides anything; the decisions are in `dialog_check`, `identity`,
//! `shortcuts`, `worktree_links`, and `better_ui::fluent::toast`.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, Div, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{Icon, IconName};

use better_ui::fluent::button::{Button, ButtonKind};
use better_ui::fluent::info_bar::Severity;
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
        // It gives way before Settings does: never wider than what is left.
        let mut row = div()
            .flex()
            .flex_shrink(1.0)
            .min_w_0()
            .overflow_hidden()
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

    /// The linked worktrees under one repository in the repository list.
    pub(crate) fn worktree_rows(
        &self,
        repository: &std::path::Path,
        first_index: usize,
        p: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let c = self.copy();
        let current = self.session.repo.as_ref().map(|r| r.root.as_path());
        crate::worktree_links::linked_rows(self.session.worktrees_for(repository), current)
            .into_iter()
            .enumerate()
            .map(|(offset, w)| {
                let path = w.path.clone();
                let branch = w
                    .branch
                    .clone()
                    .unwrap_or_else(|| c.worktree_detached().to_string());
                crate::render::row(
                    ("repo-worktree", first_index + offset),
                    w.current,
                    px(48.0),
                    p,
                )
                .pl(px(36.0))
                .child(
                    Icon::empty()
                        .path(crate::app::BRANCH_ICON)
                        .size(px(14.0))
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
                        .child(label(branch, BODY, p.text_primary).truncate())
                        .child(
                            label(
                                format!(
                                    "{} · {}",
                                    c.worktree_of(),
                                    crate::changes::truncate_leading(
                                        &w.path.display().to_string(),
                                        40
                                    )
                                ),
                                CAPTION,
                                p.text_secondary,
                            )
                            .truncate(),
                        ),
                )
                .when(!w.missing, |row| {
                    row.on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            if event.is_keyboard() {
                                return;
                            }
                            this.flyout = None;
                            this.session.open(path.clone());
                            this.drive(window, cx);
                        }),
                    )
                })
                .into_any_element()
            })
            .collect()
    }
}

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
