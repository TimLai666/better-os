//! A commit's context menu, one menu for History and the Graph tab.

use gpui::{Context, SharedString, Window};

use better_ui::fluent::menu::{MenuEntry, MenuItem};
use git_core::Commit;

use crate::app::GitApp;
use crate::dialogs::Dialog;
use crate::i18n::{Copy, fill};

/// What the menu needs to know beyond the commit.
pub(crate) struct CommitMenu {
    /// It is HEAD's commit on a branch with commits: undo and amend are
    /// offered.
    pub head: bool,
    /// It is HEAD or one of its ancestors: reset to and revert are offered,
    /// because both act on the current branch's own history.
    pub ancestor: bool,
    /// Its tags, by name.
    pub tags: Vec<String>,
}

impl GitApp {
    fn menu_entry(
        text: impl Into<SharedString>,
        cx: &mut Context<Self>,
        f: impl Fn(&mut GitApp, &mut Window, &mut Context<GitApp>) + 'static,
    ) -> MenuEntry {
        MenuItem::new(text).on_select(Self::act(cx, f)).entry()
    }

    /// The entries for one commit: undo and amend on HEAD's commit; reset
    /// to, check out, and revert; cherry-pick onto another local branch;
    /// create a branch or a tag here; delete or copy each tag; copy the
    /// hash; and view it on GitHub when the repository is hosted there.
    pub(crate) fn commit_entries(
        &self,
        commit: &Commit,
        menu: CommitMenu,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<MenuEntry> {
        let Some(repo) = self.session.repo.as_ref() else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        if menu.head {
            entries.push(Self::menu_entry(c.undo_commit, cx, |this, _, _| {
                this.session.ask_undo_commit()
            }));
            entries.push(Self::menu_entry(c.amend_commit, cx, |this, _, _| {
                this.session.amend_last()
            }));
        }
        if menu.ancestor {
            let (sha, summary) = (commit.sha.clone(), commit.summary.clone());
            entries.push(Self::menu_entry(c.reset_to, cx, move |this, _, _| {
                this.session.dialog = Some(Dialog::ResetTo {
                    sha: sha.clone(),
                    summary: summary.clone(),
                })
            }));
        }
        let sha = commit.sha.clone();
        entries.push(Self::menu_entry(
            c.checkout_commit,
            cx,
            move |this, _, _| this.session.checkout_commit(sha.clone()),
        ));
        if menu.ancestor {
            let reverted = commit.clone();
            entries.push(Self::menu_entry(c.revert, cx, move |this, _, _| {
                this.session.revert_commit(&reverted)
            }));
        }
        let branches: Vec<MenuEntry> = repo
            .info
            .branches
            .iter()
            .filter(|b| !b.remote && !b.current)
            .map(|b| {
                let (sha, name) = (commit.sha.clone(), b.name.clone());
                Self::menu_entry(b.name.clone(), cx, move |this, _, _| {
                    this.session
                        .cherry_pick_sha(sha.clone(), Some(name.clone()))
                })
            })
            .collect();
        if !branches.is_empty() {
            entries.push(MenuEntry::submenu(c.cherry_pick_to, branches));
        }
        entries.push(MenuEntry::Separator);
        let (sha, short) = (commit.sha.clone(), commit.short_sha().to_string());
        entries.push(Self::menu_entry(
            c.create_branch_here,
            cx,
            move |this, _, _| {
                this.session.dialog = Some(Dialog::NewBranch {
                    start: Some(sha.clone()),
                    start_label: Some(short.clone()),
                })
            },
        ));
        let sha = commit.sha.clone();
        entries.push(Self::menu_entry(c.create_tag, cx, move |this, _, _| {
            this.session.dialog = Some(Dialog::CreateTag { sha: sha.clone() })
        }));
        for tag in menu.tags {
            let (delete, copy) = (tag.clone(), tag.clone());
            entries.push(Self::menu_entry(
                fill(c.delete_tag, &[("tag", &tag)]),
                cx,
                move |this, _, _| {
                    this.session.dialog = Some(Dialog::DeleteTag {
                        name: delete.clone(),
                    })
                },
            ));
            entries.push(Self::menu_entry(
                fill(c.copy_tag, &[("tag", &tag)]),
                cx,
                move |this, _, cx| this.copy_text(copy.clone(), cx),
            ));
        }
        let sha = commit.sha.clone();
        entries.push(Self::menu_entry(c.copy_sha, cx, move |this, _, cx| {
            this.copy_text(sha.clone(), cx)
        }));
        if let Some(hosted) = &repo.hosted {
            let url = format!("{}/commit/{}", hosted.web_url(), commit.sha);
            entries.push(Self::menu_entry(
                c.view_on_github_commit,
                cx,
                move |this, _, _| this.open_url(&url),
            ));
        }
        entries
    }
}
