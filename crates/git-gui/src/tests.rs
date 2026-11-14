//! The window's rules without a display: the GPUI-free modules, and the
//! session's GitHub side against a local fake server (never GitHub).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use better_ui::fluent::toggle::CheckState;
use git_core::{
    BranchHead, ChangeKind, Commit, DiffLine, DiffLineKind, FileChange, FileDiff, Hunk, Refusal,
    RepositoryStatus,
};
use git_platform::{Git, GitError};
use github_api::{Host, Token};

use crate::changes::{Badge, Include, Inclusion, badge_of, hunk_lines, rows, truncate_leading};
use crate::commit::{Blocked, CommitForm, parse_co_authors, summary_too_long};
use crate::dialogs::{Dialog, Invalid, validate};
use crate::github::{Account, GhCall, SignIn, SignInProblem};
use crate::i18n::{EN_US, ZH_TW};
use crate::keys::{Action, SHORTCUTS, action_for};
use crate::remote::{NextStep, SyncAction, next_step, sync_action};
use crate::session::{Files, GitSession, KeyringChange, Notice, Problem};

fn change(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileChange {
    FileChange {
        path: PathBuf::from(path),
        original_path: None,
        staged,
        unstaged,
        untracked: false,
        conflicted: false,
        conflict: None,
    }
}

fn status(changes: Vec<FileChange>) -> RepositoryStatus {
    RepositoryStatus {
        head: BranchHead::Branch("main".into()),
        unborn: false,
        head_oid: None,
        upstream: None,
        ahead_behind: None,
        changes,
        stash_count: 0,
    }
}

fn diff() -> FileDiff {
    let line = |kind, text: &str, old: Option<u32>, new: Option<u32>| DiffLine {
        kind,
        text: text.into(),
        old_line: old,
        new_line: new,
        no_newline_at_end: false,
    };
    FileDiff::Text {
        hunks: vec![
            Hunk {
                header: "@@ -1,2 +1,2 @@".into(),
                lines: vec![
                    line(DiffLineKind::Context, "a", Some(1), Some(1)),
                    line(DiffLineKind::Removed, "b", Some(2), None),
                    line(DiffLineKind::Added, "B", None, Some(2)),
                ],
            },
            Hunk {
                header: "@@ -9 +9 @@".into(),
                lines: vec![
                    line(DiffLineKind::Removed, "x", Some(9), None),
                    line(DiffLineKind::Added, "X", None, Some(9)),
                ],
            },
        ],
    }
}

// --- changes -----------------------------------------------------------------

#[test]
fn badges_follow_head_to_working_tree() {
    let m = Some(ChangeKind::Modified);
    let a = Some(ChangeKind::Added);
    let d = Some(ChangeKind::Deleted);
    assert_eq!(badge_of(&change("x", m, None)), Badge::Modified);
    assert_eq!(badge_of(&change("x", a, m)), Badge::Added);
    assert_eq!(badge_of(&change("x", a, d)), Badge::Deleted);
    assert_eq!(badge_of(&change("x", None, d)), Badge::Deleted);
    assert_eq!(
        badge_of(&change("x", Some(ChangeKind::Renamed), None)),
        Badge::Renamed
    );
    let mut untracked = change("x", None, None);
    untracked.untracked = true;
    assert_eq!(badge_of(&untracked), Badge::Added);
    let mut conflicted = change("x", None, None);
    conflicted.conflicted = true;
    assert_eq!(badge_of(&conflicted), Badge::Conflicted);
}

#[test]
fn inclusion_defaults_to_all_and_survives_refresh() {
    let s = status(vec![
        change("a", Some(ChangeKind::Modified), None),
        change("b", None, Some(ChangeKind::Modified)),
    ]);
    let mut inclusion = Inclusion::default();
    inclusion.sync(&s);
    assert_eq!(inclusion.check(Path::new("a")), CheckState::Checked);
    inclusion.toggle_file(Path::new("a"));
    inclusion.sync(&s);
    assert_eq!(
        inclusion.check(Path::new("a")),
        CheckState::Unchecked,
        "kept"
    );
    let shown = rows(&s, &inclusion, Some(Path::new("b")), "");
    assert_eq!(Inclusion::summary(&shown), CheckState::Indeterminate);
    assert!(shown[1].selected && !shown[0].selected);
    inclusion.toggle_all(&shown);
    assert_eq!(inclusion.check(Path::new("a")), CheckState::Checked);
    let shown = rows(&s, &inclusion, None, "");
    inclusion.toggle_all(&shown);
    assert!(!inclusion.any_included());
    assert_eq!(
        rows(&s, &inclusion, None, "B").len(),
        1,
        "the filter ignores case"
    );
    inclusion.sync(&status(vec![change("c", None, Some(ChangeKind::Modified))]));
    assert_eq!(
        inclusion.check(Path::new("a")),
        CheckState::Unchecked,
        "gone"
    );
    assert_eq!(
        inclusion.check(Path::new("c")),
        CheckState::Checked,
        "new is in"
    );
}

#[test]
fn lines_make_a_file_indeterminate_and_build_a_selection() {
    let s = status(vec![change("f", None, Some(ChangeKind::Modified))]);
    let mut inclusion = Inclusion::default();
    inclusion.sync(&s);
    let d = diff();
    assert_eq!(hunk_lines(&d, 1), vec![(1, 0), (1, 1)]);
    inclusion.set_lines(Path::new("f"), &d, &hunk_lines(&d, 1), false);
    assert_eq!(inclusion.check(Path::new("f")), CheckState::Indeterminate);
    let FileDiff::Text { hunks } = &d else {
        unreachable!()
    };
    assert!(inclusion.line_included(Path::new("f"), &hunks[0].lines[1]));
    assert!(!inclusion.line_included(Path::new("f"), &hunks[1].lines[0]));
    let selection = inclusion.selections(&s);
    let lines = selection[0].lines.as_ref().unwrap();
    assert_eq!(lines.fingerprint, git_core::change_fingerprint(&d));
    assert_eq!(lines.selected, vec![(None, Some(2)), (Some(2), None)]);
    inclusion.set_lines(Path::new("f"), &d, &[(0, 0)], true);
    assert_eq!(
        inclusion.check(Path::new("f")),
        CheckState::Indeterminate,
        "context lines are not choices"
    );
    inclusion.set_lines(Path::new("f"), &d, &hunk_lines(&d, 1), true);
    assert_eq!(inclusion.get(Path::new("f")), Include::All);
    inclusion.set_lines(Path::new("f"), &d, &[(0, 1), (0, 2), (1, 0), (1, 1)], false);
    assert_eq!(inclusion.get(Path::new("f")), Include::Nothing);
    // The same changes shown with the whole file as context keep the
    // choice; a diff whose changes differ makes it the whole file again.
    inclusion.set_lines(Path::new("f"), &d, &[(0, 1)], true);
    assert_eq!(inclusion.check(Path::new("f")), CheckState::Indeterminate);
    let FileDiff::Text { hunks } = d.clone() else {
        unreachable!()
    };
    let mut whole = hunks.clone();
    let tail = whole.pop().unwrap();
    whole[0].header = "@@ -1,9 +1,9 @@".into();
    whole[0].lines.extend(tail.lines);
    inclusion.reconcile(Path::new("f"), &FileDiff::Text { hunks: whole });
    assert_eq!(inclusion.check(Path::new("f")), CheckState::Indeterminate);
    let mut edited = hunks.clone();
    edited[1].lines[1].text = "Y".into();
    inclusion.reconcile(Path::new("f"), &FileDiff::Text { hunks: edited });
    assert_eq!(inclusion.get(Path::new("f")), Include::All);
}

#[test]
fn folder_captions_cut_at_the_front() {
    assert_eq!(truncate_leading("short", 10), "short");
    assert_eq!(truncate_leading("a/very/long/folder", 8), "…/folder");
}

// --- commit form --------------------------------------------------------------

#[test]
fn commit_is_offered_only_when_it_can_work() {
    let mut form = CommitForm::default();
    assert_eq!(form.blocked(true, false, false), Some(Blocked::NoSummary));
    form.summary = "x".into();
    assert_eq!(
        form.blocked(false, false, false),
        Some(Blocked::NothingIncluded)
    );
    assert_eq!(form.blocked(true, true, false), Some(Blocked::Busy));
    assert_eq!(
        form.blocked(true, false, true),
        Some(Blocked::OperationInProgress)
    );
    assert_eq!(form.blocked(true, false, false), None);
    form.allow_empty = true;
    assert_eq!(form.blocked(false, false, false), None);
    form.show_co_authors = true;
    form.co_authors = "Ada <ada@example.invalid>\nnot a person".into();
    assert_eq!(form.blocked(true, false, false), Some(Blocked::BadCoAuthor));
    assert_eq!(parse_co_authors(&form.co_authors), Err(2));
    form.co_authors = "Ada <ada@example.invalid>\n\n".into();
    assert_eq!(form.options().co_authors.len(), 1);
    form.push_after = true;
    form.clear();
    assert!(form.summary.is_empty() && form.push_after && !form.allow_empty);
    assert!(summary_too_long(&"x".repeat(51)) && !summary_too_long(&"中".repeat(50)));
}

// --- history -----------------------------------------------------------------

fn commit(sha: &str, parent: &str, summary: &str) -> Commit {
    Commit {
        sha: sha.into(),
        parents: if parent.is_empty() {
            vec![]
        } else {
            vec![parent.into()]
        },
        author_name: "a".into(),
        author_email: "a@x".into(),
        author_time: 0,
        refs: vec![],
        summary: summary.into(),
        body: String::new(),
    }
}

#[test]
fn reorder_and_squash_todo_lists() {
    use git_platform::{HistoryEdit, TodoAction::*};
    let commits = vec![
        commit("c", "b", "C"),
        commit("b", "a", "B"),
        commit("a", "", "A"),
    ];
    let Some(HistoryEdit::Rewrite { base, todo, .. }) = crate::history::reorder(&commits, 0, 1)
    else {
        panic!()
    };
    assert_eq!(base.as_deref(), Some("a"));
    assert_eq!(todo, vec![(Pick, "b".into()), (Pick, "c".into())]);
    let Some(HistoryEdit::Rewrite { base, todo, .. }) = crate::history::reorder(&commits, 2, 0)
    else {
        panic!()
    };
    assert_eq!(base, None, "the root moves: rewrite from --root");
    assert_eq!(
        todo,
        vec![(Pick, "b".into()), (Pick, "c".into()), (Pick, "a".into())]
    );
    let Some(HistoryEdit::Rewrite { todo, message, .. }) =
        crate::history::squash(&commits, 0, 2, "AC")
    else {
        panic!()
    };
    assert_eq!(
        todo,
        vec![(Pick, "a".into()), (Squash, "c".into()), (Pick, "b".into())]
    );
    assert_eq!(message.as_deref(), Some("AC"));
    assert!(crate::history::reorder(&commits, 1, 1).is_none());
    assert!(crate::history::can_rewrite(&commits, 2, None));
    assert!(
        !crate::history::can_rewrite(&commits, 1, Some(1)),
        "pushed commits stay"
    );
    let mut merge = commits.clone();
    merge[0].parents.push("z".into());
    assert!(!crate::history::can_rewrite(&merge, 0, None));
    assert_eq!(crate::history::age(100, 40), (1, "m"));
    assert_eq!(
        crate::history::squash_message(&commits[2], &commits[0]),
        "A\n\nC"
    );
}

// --- branches and remotes ------------------------------------------------------

#[test]
fn branch_rows_group_default_recent_and_remote_only() {
    let parse = |text: &str| git_core::parse_branches(text.as_bytes()).unwrap();
    let branches = parse(
        "refs/heads/main\x1fa\x1f\x1f\x1f*\x1f3\x1fm\n\
refs/heads/zeta\x1fb\x1f\x1f\x1f \x1f2\x1fz\n\
refs/heads/alpha\x1fc\x1f\x1f\x1f \x1f1\x1fa\n\
refs/remotes/origin/main\x1fa\x1f\x1f\x1f \x1f3\x1fm\n\
refs/remotes/origin/pr-7\x1fd\x1f\x1f\x1f \x1f3\x1fp\n",
    );
    let rows = crate::branches::rows(&branches, Some("main"), &["zeta".into()], "");
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["main", "zeta", "alpha", "origin/pr-7"]);
    assert!(rows[3].remote_only);
    assert_eq!(crate::branches::rows(&branches, None, &[], "PR").len(), 1);
    assert_eq!(
        crate::branches::remember(&["a".into(), "b".into()], "b"),
        vec!["b", "a"]
    );
}

#[test]
fn the_sync_button_and_the_next_step() {
    let mut s = status(vec![]);
    assert_eq!(sync_action(&s, false, false), SyncAction::PublishRepository);
    assert_eq!(sync_action(&s, true, false), SyncAction::PublishBranch);
    assert_eq!(sync_action(&s, true, true), SyncAction::None);
    s.upstream = Some("origin/main".into());
    s.ahead_behind = Some((2, 0));
    assert_eq!(sync_action(&s, true, false), SyncAction::Push { ahead: 2 });
    s.ahead_behind = Some((2, 3));
    assert_eq!(
        sync_action(&s, true, false),
        SyncAction::Pull {
            behind: 3,
            ahead: 2
        }
    );
    s.ahead_behind = Some((0, 0));
    assert_eq!(sync_action(&s, true, false), SyncAction::Fetch);
    assert_eq!(
        next_step(Refusal::NotFastForward, true, false, false),
        NextStep::Pull
    );
    assert_eq!(
        next_step(Refusal::Authentication, true, false, true),
        NextStep::SignIn
    );
    assert_eq!(
        next_step(Refusal::PermissionDenied, true, true, true),
        NextStep::Fork
    );
    assert_eq!(
        next_step(Refusal::LocalChangesWouldBeOverwritten, false, true, true),
        NextStep::StashAndRetry
    );
    assert_eq!(
        next_step(Refusal::Network, true, true, true),
        NextStep::None
    );
    for refusal in [
        Refusal::NotFastForward,
        Refusal::StaleLease,
        Refusal::ProtectedBranch,
        Refusal::Other,
    ] {
        assert!(
            !EN_US.refusal(refusal, true).is_empty() && !ZH_TW.refusal(refusal, true).is_empty()
        );
    }
}

// --- dialogs, keys, strings ------------------------------------------------------

#[test]
fn dialogs_validate_their_fields() {
    let branches =
        git_core::parse_branches(b"refs/heads/main\x1fa\x1f\x1f\x1f*\x1f1\x1fm\n").unwrap();
    let new_branch = Dialog::NewBranch {
        start: None,
        start_label: None,
    };
    let texts = |a: &str| vec![a.to_string(), String::new(), String::new()];
    assert_eq!(
        validate(&new_branch, &texts(""), &branches, &[]),
        Some(Invalid::Empty)
    );
    assert_eq!(
        validate(&new_branch, &texts("main"), &branches, &[]),
        Some(Invalid::BranchExists)
    );
    assert_eq!(
        validate(&new_branch, &texts("new work"), &branches, &[]),
        None
    );
    assert_eq!(
        validate(&Dialog::AddLocal, &texts("relative"), &[], &[]),
        Some(Invalid::NotAbsolute)
    );
    assert_eq!(validate(&Dialog::AddLocal, &texts("/abs"), &[], &[]), None);
    let squash = Dialog::Squash { from: 0, to: 1 };
    assert_eq!(
        validate(&squash, &texts("ignored"), &[], &[]),
        Some(Invalid::Empty),
        "the message is the third field"
    );
    assert_eq!(
        validate(
            &Dialog::SignIn { enterprise: true },
            &texts("not a host"),
            &[],
            &[]
        ),
        Some(Invalid::Empty)
    );
    assert_eq!(crate::dialogs::field_count(&squash), 3);
}

#[test]
fn every_shortcut_is_unique_and_found() {
    for (index, (action, keys, _)) in SHORTCUTS.iter().enumerate() {
        assert!(
            SHORTCUTS
                .iter()
                .skip(index + 1)
                .all(|(a, k, _)| a != action && k != keys),
            "{keys}"
        );
    }
    assert_eq!(action_for("p", true, true, false), Some(Action::Pull));
    assert_eq!(action_for("P", true, false, false), Some(Action::Push));
    assert_eq!(
        action_for("backspace", true, true, false),
        Some(Action::DiscardAll)
    );
    assert_eq!(
        action_for("w", true, false, true),
        Some(Action::WorktreeList)
    );
    assert_eq!(action_for("p", false, false, false), None);
    assert_eq!(crate::keys::label(Action::Settings), "Ctrl+,");
}

#[test]
fn every_string_exists_in_both_languages() {
    let (en, zh) = (EN_US.all(), ZH_TW.all());
    assert_eq!(en.len(), zh.len());
    for (e, z) in en.iter().zip(&zh) {
        assert!(!e.is_empty() && !z.is_empty(), "{e} / {z}");
    }
    assert_eq!(EN_US.commit_label(Some("main")), "Commit to main");
    assert_eq!(ZH_TW.commit_label(Some("main")), "提交到 main");
    let (title, message, details) = ZH_TW.problem(&Problem::Git {
        kind: crate::session::OpKind::Push,
        error: GitError::Failed {
            exit_code: Some(1),
            message: " ! [rejected] main -> main (fetch first)".into(),
        },
    });
    assert_eq!(title, "推送失敗");
    assert!(message.contains("先拉取"));
    assert!(details.contains("rejected"));
    assert_eq!(EN_US.ago(30), "just now");
    assert_eq!(EN_US.ago(180), "3 minutes ago");
}

#[test]
fn repository_rows_search_and_missing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file = tmp.path().join("repos.json");
    let mut list = crate::repositories::RepositoryList::load(Some(file.clone()));
    for path in ["/work/beta", "/work/alpha", "/gone/old"] {
        list.add(Path::new(path)).unwrap();
    }
    list.set_alias(Path::new("/work/beta"), "Best").unwrap();
    let rows = list.rows(
        "",
        Some(Path::new("/work/alpha")),
        &[PathBuf::from("/gone/old")],
        |p| p.starts_with("/work"),
    );
    assert_eq!(
        rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["old", "alpha", "Best"]
    );
    assert!(rows[0].missing && rows[0].recent && rows[1].current);
    assert_eq!(list.rows("best", None, &[], |_| true).len(), 1);
    list.relocate(Path::new("/gone/old"), Path::new("/work/old"))
        .unwrap();
    let reread = crate::repositories::RepositoryList::load(Some(file));
    assert!(
        reread
            .entries
            .iter()
            .any(|e| e.path == Path::new("/work/old"))
    );
    assert_eq!(reread.alias(Path::new("/work/beta")), Some("Best"));
}

#[test]
fn launches_use_argument_vectors() {
    let settings = crate::settings::Settings::default();
    let editor =
        crate::settings::editor_launch(&settings, Path::new("/r/f.txt"), |p| p == "zed").unwrap();
    assert_eq!(
        (editor.program.as_str(), editor.path_arg.as_deref()),
        ("zed", Some(Path::new("/r/f.txt")))
    );
    let custom = crate::settings::Settings {
        editor: "code --wait".into(),
        terminal: "konsole".into(),
        ..Default::default()
    };
    let editor = crate::settings::editor_launch(&custom, Path::new("/r"), |_| false).unwrap();
    assert_eq!(editor.args, vec!["--wait"]);
    let terminal = crate::settings::terminal_launch(&custom, Path::new("/r"), |_| false).unwrap();
    assert_eq!(terminal.args, vec!["--workdir", "/r"]);
    let gnome =
        crate::settings::terminal_launch(&settings, Path::new("/r"), |p| p == "gnome-terminal")
            .unwrap();
    assert_eq!(gnome.args, vec!["--working-directory=/r"]);
    assert!(crate::settings::terminal_launch(&settings, Path::new("/r"), |_| false).is_none());
    assert_eq!(
        crate::settings::files_launch(Path::new("/r"), |p| p == "better-files").program,
        "better-files"
    );
    assert_eq!(
        crate::settings::files_launch(Path::new("/r"), |_| false).program,
        "xdg-open"
    );
}

#[test]
fn clone_plans_and_names() {
    let plan = crate::clone::plan("octo/hello", Path::new("/projects"), |_| false).unwrap();
    assert_eq!(
        plan,
        (
            "https://github.com/octo/hello.git".into(),
            PathBuf::from("/projects/hello")
        )
    );
    assert_eq!(
        crate::clone::plan("", Path::new("/p"), |_| false),
        Err(crate::clone::CloneProblem::NoUrl)
    );
    assert_eq!(
        crate::clone::plan("o/r", Path::new("rel"), |_| false),
        Err(crate::clone::CloneProblem::NoFolder)
    );
    assert_eq!(
        crate::clone::plan("o/r", Path::new("/p"), |_| true),
        Err(crate::clone::CloneProblem::FolderInUse)
    );
    assert_eq!(
        crate::publish::sanitize_repository_name(" My Project! "),
        "My-Project"
    );
    assert_eq!(
        crate::publish::fill_license("(c) [year] [fullname]", 2026, "Ada"),
        "(c) 2026 Ada"
    );
    assert_eq!(crate::publish::current_year(1_780_000_000), 2026);
}

#[test]
fn trust_lists_the_programs_a_repository_names() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git/hooks")).unwrap();
    std::fs::write(
        tmp.path().join(".git/config"),
        "[filter \"x\"]\n\tclean = run-me\n[core]\n\tfsmonitor = watch\n\tbare = false\n\tsshCommand = ssh-wrapper\n[gpg]\n\tprogram = fake-gpg\n[diff]\n\texternal = differ\n[credential]\n\thelper = leak\n[merge \"m\"]\n\tdriver = merger\n[include]\n\tpath = /elsewhere\n[includeIf \"gitdir:/x/\"]\n\tpath = /more\n[user]\n\tname = Ada\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join(".git/hooks/pre-commit"), "").unwrap();
    std::fs::write(tmp.path().join(".git/hooks/pre-push.sample"), "").unwrap();
    assert_eq!(
        crate::trust::configured_programs(tmp.path()),
        vec![
            "clean = run-me",
            "fsmonitor = watch",
            "sshCommand = ssh-wrapper",
            "program = fake-gpg",
            "external = differ",
            "helper = leak",
            "driver = merger",
            "path = /elsewhere",
            "path = /more",
            "hook: pre-commit",
        ]
    );
    // A worktree's `.git` file points at its git directory, whose
    // `commondir` holds the configuration.
    let worktree = tmp.path().join("wt");
    std::fs::create_dir_all(tmp.path().join(".git/worktrees/wt")).unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!(
            "gitdir: {}\n",
            tmp.path().join(".git/worktrees/wt").display()
        ),
    )
    .unwrap();
    std::fs::write(tmp.path().join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
    assert!(crate::trust::configured_programs(&worktree).contains(&"clean = run-me".to_string()));
}

// --- notifications --------------------------------------------------------------

#[test]
fn notifications_fire_once_and_not_for_old_news() {
    use crate::github::MyPull;
    use github_api::{CheckRun, CombinedChecks, PullRequest, Review};
    let failed = |sha: &str| MyPull {
        pull: PullRequest {
            number: 7,
            title: "T".into(),
            html_url: "u".into(),
            head: github_api::pulls::PullRef {
                name: "b".into(),
                sha: sha.into(),
                repo: None,
            },
            ..Default::default()
        },
        checks: CombinedChecks {
            runs: vec![CheckRun {
                id: 1,
                name: "ci".into(),
                status: "completed".into(),
                conclusion: Some("failure".into()),
                html_url: None,
                details_url: None,
                check_suite: None,
            }],
            statuses: vec![],
        },
        reviews: vec![],
    };
    let mut seen = crate::notifications::Seen::default();
    assert!(
        seen.update(&[failed("a")]).is_empty(),
        "the first poll only primes"
    );
    assert!(seen.update(&[failed("a")]).is_empty());
    assert_eq!(
        seen.update(&[failed("b")]).len(),
        1,
        "a new head that fails"
    );
    let mut reviewed = failed("b");
    reviewed.reviews.push(Review {
        id: 9,
        user: github_api::repos::Owner {
            login: "rev".into(),
        },
        state: "APPROVED".into(),
        html_url: String::new(),
    });
    let notices = seen.update(&[reviewed.clone()]);
    assert!(
        matches!(&notices[..], [crate::notifications::Notice::Reviewed { reviewer, .. }] if reviewer == "rev")
    );
    assert!(seen.update(&[reviewed]).is_empty());
}

// --- the GitHub side of the session, against a local fake server ------------------

/// Serves (method, path prefix, status, body) routes on 127.0.0.1.
fn server(
    routes: Vec<(&'static str, &'static str, u16, String)>,
) -> (Host, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (method, path) = (
                parts.next().unwrap_or("").to_string(),
                parts.next().unwrap_or("").to_string(),
            );
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            log.lock().unwrap().push(format!("{method} {path}"));
            let (status, answer) = routes
                .iter()
                .find(|(m, p, _, _)| *m == method && path.starts_with(p))
                .map(|(_, _, s, b)| (*s, b.clone()))
                .unwrap_or((404, r#"{"message":"Not Found"}"#.into()));
            let _ = stream.write_all(
                format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len()).as_bytes(),
            );
        }
    });
    (Host::at("github.com", &base), seen)
}

/// A `Git` that knows one repository with an `origin` on github.com.
struct HostedGit;

impl Git for HostedGit {
    fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError> {
        Ok(path.to_path_buf())
    }
    fn status(&self, _: &Path) -> Result<RepositoryStatus, GitError> {
        Ok(status(vec![]))
    }
    fn diff(&self, _: &Path, _: &Path, _: git_platform::DiffSide) -> Result<FileDiff, GitError> {
        Ok(FileDiff::Text { hunks: vec![] })
    }
    fn stage(&self, _: &Path, _: &[PathBuf]) -> Result<(), GitError> {
        Ok(())
    }
    fn unstage(&self, _: &Path, _: &[PathBuf]) -> Result<(), GitError> {
        Ok(())
    }
    fn commit(&self, _: &Path, _: &git_platform::CommitMessage) -> Result<(), GitError> {
        Ok(())
    }
    fn info(&self, _: &Path) -> Result<git_platform::RepositoryInfo, GitError> {
        Ok(git_platform::RepositoryInfo {
            remotes: vec![git_core::Remote {
                name: "origin".into(),
                fetch_url: "https://github.com/octo/hello.git".into(),
                push_url: "https://github.com/octo/hello.git".into(),
            }],
            ..Default::default()
        })
    }
}

fn hosted_session(host: &Host) -> GitSession {
    let mut session = GitSession::new(Arc::new(HostedGit), Files::default());
    session.trust_and_open(PathBuf::from("/work/hello"));
    session.run_git_jobs();
    session.github.host = Some(host.clone());
    session
}

#[test]
fn sign_in_polls_stores_the_token_and_reads_the_repository() {
    let (host, seen) = server(vec![
        ("POST", "/login/device/code", 200, r#"{"device_code":"dc","user_code":"WXYZ-1234","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#.into()),
        ("POST", "/login/oauth/access_token", 200, r#"{"access_token":"ghu_1","refresh_token":"ghr_1","expires_in":28800,"refresh_token_expires_in":15897600}"#.into()),
        ("GET", "/user", 200, r#"{"login":"octo"}"#.into()),
        ("GET", "/repos/octo/hello/pulls", 200, r#"[{"number":3,"title":"Mine","html_url":"https://x/3","user":{"login":"octo"},"head":{"ref":"main","sha":"s"},"base":{"ref":"dev","sha":"t"}}]"#.into()),
        ("GET", "/repos/octo/hello/commits", 200, r#"{"check_runs":[],"statuses":[]}"#.into()),
        ("GET", "/repos/octo/hello/branches/main", 200, r#"{"protected":true}"#.into()),
        ("GET", "/repos/octo/hello", 200, r#"{"name":"hello","full_name":"octo/hello","owner":{"login":"octo"},"clone_url":"c","html_url":"h","permissions":{"push":true}}"#.into()),
    ]);
    let mut session = hosted_session(&host);
    session.start_sign_in(host.clone());
    session.run_pending();
    let SignIn::Waiting { code, interval } = &session.github.sign_in else {
        panic!("{:?}", session.github.sign_in)
    };
    assert_eq!((code.user_code.as_str(), *interval), ("WXYZ-1234", 5));
    session.poll_sign_in();
    session.run_pending();
    let account = session.github.account.clone().expect("signed in");
    assert_eq!(account.login, "octo");
    assert_eq!(session.github.sign_in, SignIn::Idle);
    assert!(matches!(session.notice, Some(Notice::SignedIn(_))) || session.notice.is_none());
    assert!(
        matches!(session.keyring_changes.last(), Some(KeyringChange::Store(a)) if a.login == "octo" && a.token.refresh_token.as_deref() == Some("ghr_1"))
    );
    let repo = session.repo.as_ref().unwrap();
    assert!(repo.github.loaded && repo.github.protected && repo.github.can_push);
    assert_eq!(session.current_pull().map(|p| p.number), Some(3));
    let network = session.network_options();
    assert_eq!(
        network.credentials.unwrap().token,
        "ghu_1",
        "git gets the token for github.com"
    );
    session.sign_out();
    assert!(
        matches!(session.keyring_changes.last(), Some(KeyringChange::Delete(h)) if h == "github.com")
    );
    assert!(seen.lock().unwrap().iter().all(|r| !r.contains("secret")));
}

#[test]
fn sign_in_problems_and_a_build_without_a_client_id() {
    for (body, problem) in [
        (r#"{"error":"expired_token"}"#, SignInProblem::Expired),
        (r#"{"error":"access_denied"}"#, SignInProblem::Denied),
    ] {
        let (host, _) = server(vec![
            (
                "POST",
                "/login/device/code",
                200,
                r#"{"device_code":"d","user_code":"u","verification_uri":"v","expires_in":1}"#
                    .into(),
            ),
            ("POST", "/login/oauth/access_token", 200, body.into()),
        ]);
        let mut session = hosted_session(&host);
        session.start_sign_in(host);
        session.run_pending();
        session.poll_sign_in();
        session.run_pending();
        assert_eq!(session.github.sign_in, SignIn::Failed(problem));
    }
    let mut session = GitSession::new(Arc::new(HostedGit), Files::default());
    session.set_client_id("");
    session.start_sign_in(Host::github());
    assert_eq!(session.problem, Some(Problem::NoClientId));
    assert!(session.queued_gh().is_empty(), "nothing is sent");
}

#[test]
fn an_expired_token_is_refreshed_before_use_and_stored() {
    let (host, seen) = server(vec![
        (
            "POST",
            "/login/oauth/access_token",
            200,
            r#"{"access_token":"ghu_new","refresh_token":"ghr_new","expires_in":28800}"#.into(),
        ),
        (
            "POST",
            "/repos/octo/hello/issues",
            201,
            r#"{"number":5,"html_url":"https://x/5"}"#.into(),
        ),
    ]);
    let mut session = hosted_session(&host);
    session.github.account = Some(Account {
        host: host.clone(),
        login: "octo".into(),
        token: Token {
            access_token: "old".into(),
            refresh_token: Some("ghr_old".into()),
            expires_at: Some(0),
            refresh_expires_at: None,
        },
    });
    session.create_issue("Bug", "body");
    session.run_pending();
    assert!(matches!(
        session.notice,
        Some(Notice::IssueCreated { number: 5, .. })
    ));
    assert_eq!(
        session.github.account.as_ref().unwrap().token.access_token,
        "ghu_new"
    );
    assert!(
        matches!(session.keyring_changes.first(), Some(KeyringChange::Store(a)) if a.token.access_token == "ghu_new")
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0], "POST /login/oauth/access_token", "refresh first");
}

#[test]
fn forking_points_origin_at_the_fork_and_keeps_upstream() {
    let (host, _) = server(vec![(
        "POST",
        "/repos/octo/hello/forks",
        202,
        r#"{"name":"hello","full_name":"me/hello","owner":{"login":"me"},"fork":true,"clone_url":"https://github.com/me/hello.git","html_url":"h"}"#.into(),
    )]);
    let mut session = hosted_session(&host);
    session.github.account = Some(Account {
        host: host.clone(),
        login: "me".into(),
        token: Token {
            access_token: "t".into(),
            refresh_token: None,
            expires_at: None,
            refresh_expires_at: None,
        },
    });
    session.fork();
    while let Some(request) = session.next_gh() {
        let answer = request.run(crate::github::now());
        session.apply_gh(answer);
    }
    let ops = session.queued();
    assert!(ops.iter().any(|op| matches!(op, crate::session::Op::AddRemote { name, url } if name == "upstream" && url.contains("octo/hello"))));
    assert!(ops.iter().any(|op| matches!(op, crate::session::Op::SetRemoteUrl { name, url } if name == "origin" && url.contains("me/hello"))));
    assert!(matches!(session.notice, Some(Notice::Forked { .. })));
    let _ = GhCall::DeviceCode;
}

#[test]
fn notification_text_is_never_read_as_markup() {
    assert_eq!(
        crate::notifications::plain_text("<a href=\"x\">Fix</a> & <img src=y>\u{7}"),
        "&lt;a href=\"x\"&gt;Fix&lt;/a&gt; &amp; &lt;img src=y&gt;"
    );
}

// --- ticket 103: dialogs say why, identity, toasts, shortcuts, worktrees,
// and live changes ---------------------------------------------------------

use crate::dialog_check::{Blocked as DialogBlocked, Folder, check};

fn new_repository() -> Dialog {
    Dialog::NewRepository {
        gitignore: None,
        license: None,
        readme: true,
    }
}

fn three(a: &str, b: &str) -> Vec<String> {
    vec![a.to_string(), b.to_string(), String::new()]
}

/// "Create" with an empty name, a name that is already a repository, and a
/// folder that already has files: each has its reason, under the field it
/// is about, and the button is off.
#[test]
fn a_new_repository_says_why_it_cannot_be_created() {
    let free = |_: &Path| Folder::Free;
    let dialog = new_repository();
    assert_eq!(
        check(&dialog, &three("", "/home/ada/Projects"), &[], &[], &free),
        Some(DialogBlocked {
            reason: Invalid::Empty,
            field: Some(0)
        })
    );
    assert_eq!(
        check(&dialog, &three("site", "Projects"), &[], &[], &free),
        Some(DialogBlocked {
            reason: Invalid::NotAbsolute,
            field: Some(1)
        })
    );
    let looked = Mutex::new(Vec::new());
    let repository = |path: &Path| {
        looked.lock().unwrap().push(path.to_path_buf());
        Folder::Repository
    };
    assert_eq!(
        check(
            &dialog,
            &three("my site", "/home/ada/Projects"),
            &[],
            &[],
            &repository
        ),
        Some(DialogBlocked {
            reason: Invalid::AlreadyRepository,
            field: Some(0)
        })
    );
    assert_eq!(
        looked.lock().unwrap().as_slice(),
        [PathBuf::from("/home/ada/Projects/my-site")],
        "the folder the repository would be made in, by its cleaned name"
    );
    let has_files = |_: &Path| Folder::HasFiles;
    assert_eq!(
        check(
            &dialog,
            &three("site", "/home/ada/Projects"),
            &[],
            &[],
            &has_files
        )
        .map(|b| b.reason),
        Some(Invalid::NotEmpty)
    );
    let file = |_: &Path| Folder::NotAFolder;
    assert_eq!(
        check(
            &dialog,
            &three("site", "/home/ada/Projects"),
            &[],
            &[],
            &file
        )
        .map(|b| b.reason),
        Some(Invalid::NotAFolder)
    );
    assert_eq!(
        check(
            &dialog,
            &three("site", "/home/ada/Projects"),
            &[],
            &[],
            &free
        ),
        None
    );
}

#[test]
fn the_disk_tells_a_free_folder_from_a_repository_or_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path();
    assert_eq!(
        crate::dialog_check::folder(&path.join("missing")),
        Folder::Free
    );
    std::fs::create_dir(path.join("empty")).unwrap();
    assert_eq!(
        crate::dialog_check::folder(&path.join("empty")),
        Folder::Free
    );
    std::fs::create_dir_all(path.join("full")).unwrap();
    std::fs::write(path.join("full/notes.txt"), "x").unwrap();
    assert_eq!(
        crate::dialog_check::folder(&path.join("full")),
        Folder::HasFiles
    );
    std::fs::create_dir_all(path.join("repo/.git")).unwrap();
    assert_eq!(
        crate::dialog_check::folder(&path.join("repo")),
        Folder::Repository
    );
    assert_eq!(
        crate::dialog_check::folder(&path.join("full/notes.txt")),
        Folder::NotAFolder
    );
}

/// Every reason is under the field it is about, so the person sees it
/// where they are typing.
#[test]
fn each_dialog_reason_belongs_to_its_field() {
    let free = |_: &Path| Folder::Free;
    let worktree = Dialog::AddWorktree { create: true };
    assert_eq!(
        check(&worktree, &three("relative", "x"), &[], &[], &free).and_then(|b| b.field),
        Some(0)
    );
    assert_eq!(
        check(&worktree, &three("/abs/wt", ""), &[], &[], &free).and_then(|b| b.field),
        Some(1)
    );
    let clone = Dialog::Clone {
        tab: crate::dialogs::CloneTab::GitHub,
        selected: None,
    };
    assert_eq!(
        check(&clone, &three("", "/abs"), &[], &[], &free),
        Some(DialogBlocked {
            reason: Invalid::NoSelection,
            field: None
        }),
        "nothing chosen in the list is about the dialog, not a field"
    );
    assert_eq!(
        check(
            &Dialog::Squash { from: 0, to: 1 },
            &three("", ""),
            &[],
            &[],
            &free
        )
        .and_then(|b| b.field),
        Some(2)
    );
    assert_eq!(
        check(&Dialog::Identity, &three("", "a@b.c"), &[], &[], &free),
        Some(DialogBlocked {
            reason: Invalid::Empty,
            field: Some(0)
        })
    );
    assert_eq!(
        check(&Dialog::Identity, &three("Ada", "ada"), &[], &[], &free),
        Some(DialogBlocked {
            reason: Invalid::NotAnEmail,
            field: Some(1)
        })
    );
    assert_eq!(
        check(
            &Dialog::Identity,
            &three("Ada", "ada@example.com"),
            &[],
            &[],
            &free
        ),
        None
    );
}

#[test]
fn every_dialog_reason_has_words_in_both_languages() {
    for reason in [
        Invalid::Empty,
        Invalid::BranchExists,
        Invalid::BranchReserved,
        Invalid::TagExists,
        Invalid::TagTooLong,
        Invalid::NotAbsolute,
        Invalid::NoSelection,
        Invalid::AlreadyRepository,
        Invalid::NotEmpty,
        Invalid::NotAFolder,
        Invalid::NotAnEmail,
    ] {
        let en = EN_US.invalid_message(reason);
        let zh = ZH_TW.invalid_message(reason);
        assert!(!en.is_empty() && !zh.is_empty() && en != zh, "{reason:?}");
    }
}

#[test]
fn the_identity_step_never_tells_the_person_to_type_a_command() {
    for copy in [&EN_US, &ZH_TW] {
        for text in [
            copy.identity_title(),
            copy.identity_body(),
            copy.identity_writes(),
            copy.identity_email_hint(),
        ] {
            assert!(!text.contains("git config"), "{text}");
            assert!(!text.is_empty());
        }
    }
}

#[test]
fn git_refusing_for_want_of_a_name_is_an_identity_error() {
    let refusal = |message: &str| GitError::Failed {
        exit_code: Some(128),
        message: message.to_string(),
    };
    assert!(crate::identity::is_identity_error(&refusal(
        "Author identity unknown\n\n*** Please tell me who you are.\n\nRun\n\n  git config --global user.email \"you@example.com\"\n\nfatal: unable to auto-detect email address (got 'tim@host.(none)')"
    )));
    assert!(crate::identity::is_identity_error(&refusal(
        "fatal: no email was given and auto-detection is disabled"
    )));
    assert!(crate::identity::is_identity_error(&refusal(
        "fatal: empty ident name (for <a@b>) not allowed"
    )));
    assert!(!crate::identity::is_identity_error(&refusal(
        "error: pathspec 'x' did not match any file(s) known to git"
    )));
    assert!(!crate::identity::is_identity_error(&GitError::NotInstalled));
    assert!(crate::identity::known(Ok(true)));
    assert!(!crate::identity::known(Ok(false)));
    assert!(
        crate::identity::known(Err(GitError::Unsupported("identity_known"))),
        "a check that cannot run leaves the answer to Git"
    );
}

#[test]
fn the_identity_step_is_prefilled_from_git_then_the_github_account() {
    use crate::identity::{Draft, prefill};
    let user = github_api::User {
        login: "octo".into(),
        name: Some("Octo Cat".into()),
        email: Some("octo@example.com".into()),
    };
    let github = Host::github();
    assert_eq!(
        prefill("", "", Some((&github, &user))),
        Draft {
            name: "Octo Cat".into(),
            email: "octo@example.com".into()
        }
    );
    let private = github_api::User {
        login: "octo".into(),
        ..Default::default()
    };
    assert_eq!(
        prefill("", "", Some((&github, &private))),
        Draft {
            name: "octo".into(),
            email: "octo@users.noreply.github.com".into()
        },
        "a hidden email becomes the private no-reply address"
    );
    let enterprise = Host::enterprise("git.example.com").unwrap();
    assert_eq!(
        prefill("", "", Some((&enterprise, &private))).email,
        "octo@users.noreply.git.example.com"
    );
    assert_eq!(
        prefill("Ada", "", Some((&github, &user))),
        Draft {
            name: "Ada".into(),
            email: "octo@example.com".into()
        },
        "what Git already has wins"
    );
    assert_eq!(prefill("", "", None), Draft::default());
}

#[test]
fn the_identity_step_checks_and_writes_two_settings() {
    use crate::identity::{Problem as IdentityProblem, check as identity_check, writes};
    assert_eq!(identity_check(" ", "a@b.c"), Some(IdentityProblem::NoName));
    assert_eq!(identity_check("Ada", ""), Some(IdentityProblem::NoEmail));
    for bad in [
        "ada",
        "@example.com",
        "ada@",
        "ada @example.com",
        "a@b@c",
        "<a@b>",
    ] {
        assert_eq!(
            identity_check("Ada", bad),
            Some(IdentityProblem::NotAnEmail),
            "{bad}"
        );
    }
    assert_eq!(identity_check("Ada", "ada@example.com"), None);
    assert_eq!(IdentityProblem::NoName.field(), 0);
    assert_eq!(IdentityProblem::NotAnEmail.field(), 1);
    assert_eq!(
        writes(" Ada ", " ada@example.com "),
        vec![
            ("user.name".to_string(), "Ada".to_string()),
            ("user.email".to_string(), "ada@example.com".to_string())
        ]
    );
}

/// A notice is shown as the kit's toast (`better_ui::fluent::toast`), whose
/// one timer rule is the window's: about six seconds, held while the pointer
/// is on it, and resumed with what was left — never less than a second.
#[test]
fn a_notice_toast_follows_the_kits_timer() {
    use better_ui::fluent::toast::{TOAST_DURATION, TOAST_MIN_REMAINING, ToastQueue};
    use std::time::{Duration, Instant};
    let start = Instant::now();
    let mut queue = ToastQueue::default();
    let forked = queue.push(
        Notice::Forked {
            name: "ada/site".into(),
        },
        Some(TOAST_DURATION),
        start,
    );
    assert!(!queue.expire(start + TOAST_DURATION - Duration::from_millis(1)));
    queue.hover(forked, true, start + Duration::from_secs(2));
    assert!(
        !queue.expire(start + Duration::from_secs(60)),
        "it stays while the pointer is on it"
    );
    queue.hover(forked, false, start + Duration::from_secs(60));
    assert!(!queue.expire(start + Duration::from_secs(63)));
    assert!(
        queue.expire(start + Duration::from_secs(64)),
        "4 s were left"
    );
    assert!(TOAST_MIN_REMAINING >= Duration::from_secs(1));
}

#[test]
fn a_toast_keeps_the_link_of_what_it_announces() {
    assert_eq!(
        Notice::PullCreated {
            number: 7,
            url: "https://github.com/a/b/pull/7".into()
        }
        .link()
        .as_deref(),
        Some("https://github.com/a/b/pull/7")
    );
    assert_eq!(Notice::ChecksRerun(1).link(), None);
    assert_eq!(Notice::WatchFallback.link(), None);
}

/// No operation the window already shows has a "done" notice: the variant
/// is gone, and every notice left carries something the window does not
/// show by itself.
#[test]
fn every_notice_left_has_words_and_none_says_done() {
    for copy in [&EN_US, &ZH_TW] {
        for notice in [
            Notice::PullCreated {
                number: 1,
                url: "u".into(),
            },
            Notice::IssueCreated {
                number: 1,
                url: "u".into(),
            },
            Notice::Published { url: "u".into() },
            Notice::Forked { name: "n".into() },
            Notice::ChecksRerun(1),
            Notice::SignedIn("octo".into()),
            Notice::StashWaiting("stash@{0}".into()),
            Notice::WatchFallback,
        ] {
            let text = copy.notice(&notice);
            assert!(!text.is_empty());
            assert!(!text.contains("done") && !text.contains("完成"), "{text}");
        }
    }
}

#[test]
fn the_toolbar_shortcuts_are_always_there_and_github_only_when_hosted() {
    use crate::shortcuts::{Shortcut, compact, editor_name, shown};
    assert_eq!(
        shown(false),
        vec![Shortcut::Editor, Shortcut::Files, Shortcut::Terminal]
    );
    assert_eq!(shown(true).last(), Some(&Shortcut::GitHub));
    // Each is the menu's action, so its shortcut is the menu's.
    assert_eq!(
        crate::keys::label(Shortcut::Editor.action()),
        "Ctrl+Shift+A"
    );
    assert_eq!(crate::keys::label(Shortcut::Files.action()), "Ctrl+Shift+F");
    assert_eq!(crate::keys::label(Shortcut::Terminal.action()), "Ctrl+`");
    assert_eq!(
        crate::keys::label(Shortcut::GitHub.action()),
        "Ctrl+Shift+G"
    );
    // Short names fit with labels in a wide window; the review's case, a
    // long editor and long repository and branch names at 1210px, does not.
    let short = vec![
        "Open in Zed".to_string(),
        "Show in Better Files".to_string(),
        "Open in Terminal".to_string(),
    ];
    assert!(!compact(
        1600.0,
        [("Current repository", "site"), ("Current branch", "main")],
        &short,
        false
    ));
    let long = vec![
        "Open in Visual Studio Code".to_string(),
        "Show in Better Files".to_string(),
        "Open in Terminal".to_string(),
        "View on GitHub".to_string(),
    ];
    assert!(compact(
        1210.0,
        [
            ("Current repository", "a-repository-with-a-long-name"),
            ("Current branch", "feature/a-branch-with-a-long-name")
        ],
        &long,
        false
    ));
    assert!(compact(
        1270.0,
        [("Current repository", "site"), ("Current branch", "main")],
        &long,
        true
    ));
    // Chinese labels are wider per character.
    assert!(
        crate::shortcuts::text_width("在終端機開啟") > crate::shortcuts::text_width("Terminal")
    );
    assert_eq!(crate::shortcuts::item_width("", ""), 200.0);
    assert_eq!(crate::shortcuts::item_width("", &"x".repeat(80)), 300.0);
    let none = |_: &str| false;
    assert_eq!(
        editor_name("gnome-text-editor", none).as_deref(),
        Some("GNOME Text Editor")
    );
    assert_eq!(
        editor_name("code --wait", none).as_deref(),
        Some("Visual Studio Code")
    );
    assert_eq!(
        editor_name("/opt/bin/helix", none).as_deref(),
        Some("helix")
    );
    assert_eq!(
        editor_name("", |p| p == "zed").as_deref(),
        Some("Zed"),
        "the first editor found when none is set"
    );
    assert_eq!(editor_name("", |p| p == "xdg-open"), None);
    assert_eq!(editor_name("", none), None);
}

fn worktree(path: &str, branch: Option<&str>, main: bool) -> git_core::Worktree {
    git_core::Worktree {
        path: PathBuf::from(path),
        head: Some("abc".into()),
        branch: branch.map(str::to_string),
        bare: false,
        detached: branch.is_none(),
        locked: false,
        prunable: false,
        main,
    }
}

#[test]
fn linked_worktrees_are_listed_under_their_repository_and_mark_their_branches() {
    use crate::worktree_links::{checked_out_elsewhere, linked_rows};
    let list = vec![
        worktree("/r/main", Some("main"), true),
        worktree("/r/one", Some("one"), false),
        worktree("/r/loose", None, false),
    ];
    let rows = linked_rows(&list, Some(Path::new("/r/one")));
    assert_eq!(rows.len(), 2, "the main worktree is the repository itself");
    assert!(rows[0].current && rows[0].branch.as_deref() == Some("one"));
    assert_eq!(rows[1].branch, None);
    assert_eq!(
        checked_out_elsewhere("one", &list, Path::new("/r/main")),
        Some(PathBuf::from("/r/one"))
    );
    assert_eq!(
        checked_out_elsewhere("one", &list, Path::new("/r/one")),
        None
    );
    assert_eq!(
        checked_out_elsewhere("other", &list, Path::new("/r/main")),
        None
    );
}

#[test]
fn only_changes_the_window_shows_ask_for_a_refresh() {
    use crate::live::WatchPlan;
    let plan = WatchPlan::new(
        Path::new("/r"),
        PathBuf::from("/r/.git"),
        PathBuf::from("/r/.git"),
        &[PathBuf::from("node_modules"), PathBuf::from("build/out")],
    );
    for path in [
        "/r/src/main.rs",
        "/r/README.md",
        "/r/.git/HEAD",
        "/r/.git/index",
        "/r/.git/MERGE_HEAD",
        "/r/.git/refs/heads/feature/x",
        "/r/.git/packed-refs",
        "/r/.git/rebase-merge/done",
    ] {
        assert!(plan.matters(Path::new(path)), "{path}");
    }
    for path in [
        "/r/node_modules/pkg/index.js",
        "/r/build/out/a.o",
        "/r/.git/index.lock",
        "/r/.git/objects/ab/cdef",
        "/r/.git/logs/HEAD",
        "/r/.git/FETCH_HEAD",
        "/r/vendor/lib/.git/HEAD",
        "/elsewhere/file",
    ] {
        assert!(!plan.matters(Path::new(path)), "{path}");
    }
    // A linked worktree: its own git directory, and the shared refs.
    let linked = WatchPlan::new(
        Path::new("/w"),
        PathBuf::from("/r/.git/worktrees/w"),
        PathBuf::from("/r/.git"),
        &[],
    );
    assert!(linked.matters(Path::new("/r/.git/worktrees/w/HEAD")));
    assert!(linked.matters(Path::new("/r/.git/refs/heads/main")));
    assert!(!linked.matters(Path::new("/w/.git")), "the pointer file");
}

#[test]
fn a_refresh_waits_for_quiet_but_not_forever() {
    use crate::live::{Debounce, MAX_WAIT_MS, QUIET_MS};
    let mut debounce = Debounce::default();
    assert!(!debounce.due(10_000), "nothing changed");
    debounce.changed(0);
    assert!(!debounce.due(QUIET_MS - 1));
    assert!(debounce.due(QUIET_MS));
    assert!(!debounce.due(QUIET_MS + 1), "once");
    // Changes every 100 ms: the refresh still comes after a second.
    let mut now = 0;
    while now < MAX_WAIT_MS {
        debounce.changed(now);
        assert!(!debounce.due(now));
        now += 100;
    }
    debounce.changed(now);
    assert!(debounce.due(now));
}

#[test]
fn a_window_that_cannot_watch_refreshes_every_three_seconds() {
    use crate::live::{Live, POLL_MS};
    let mut live = Live {
        polling: true,
        last_poll: 0,
        ..Default::default()
    };
    assert_eq!(live.tick(POLL_MS - 1), (false, false));
    assert_eq!(live.tick(POLL_MS), (true, false));
    assert_eq!(live.tick(POLL_MS + 10), (false, false));
}

/// Ticket 116: the toolbar holds three drop-down items. At the minimum
/// window width the shortcut buttons are icons, the captions show whole
/// when they fit and are hidden otherwise, and the narrowest toolbar fits.
#[test]
fn the_toolbar_fits_the_minimum_width_with_three_items() {
    use crate::i18n::{EN_US, ZH_TW};
    use crate::shortcuts::{captions_fit, compact, item_min_width, narrowest};
    let width = crate::app::MIN_WINDOW_WIDTH;
    let labels = vec![
        "Open in Visual Studio Code".to_string(),
        "Show in Better Files".to_string(),
        "Open in Terminal".to_string(),
        "View on GitHub".to_string(),
    ];
    for c in [&EN_US, &ZH_TW] {
        let captions = [c.current_repository, c.current_worktree(), c.current_branch];
        assert!(compact(
            width,
            [
                (captions[0], "better-os"),
                (captions[1], "ticket-116-git-toolbar-names-the-repository"),
                (captions[2], "feature/git-toolbar-names-the-repository"),
            ],
            &labels,
            false
        ));
        // Without the checks item the captions fit whole.
        assert!(captions_fit(width, captions, 4, false), "{}", captions[0]);
        // With it they are hidden, and still nothing overflows.
        assert!(!captions_fit(width, captions, 4, true));
        for caption in captions {
            assert!(item_min_width(caption) >= crate::shortcuts::caption_width(caption) + 84.0);
        }
    }
    assert!(narrowest(3, 4, true) <= width, "{}", narrowest(3, 4, true));
}

#[test]
fn the_arrow_keys_walk_a_dropdown_and_stop_at_its_ends() {
    use crate::keys::step;
    assert_eq!(step(None, 0, true), None);
    assert_eq!(step(None, 3, true), Some(0));
    assert_eq!(step(None, 3, false), Some(2));
    assert_eq!(step(Some(0), 3, true), Some(1));
    assert_eq!(step(Some(2), 3, true), Some(2));
    assert_eq!(step(Some(0), 3, false), Some(0));
    // A search that left fewer rows: from the last one.
    assert_eq!(step(Some(7), 3, false), Some(1));
}
