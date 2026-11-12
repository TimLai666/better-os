use git_core::remote::{clone_folder_name, clone_url};
use git_core::{Refusal, classify_refusal, parse_hosted, parse_remotes};

#[test]
fn reads_remote_v() {
    let remotes = parse_remotes(
        b"origin\thttps://github.com/o/r.git (fetch)\norigin\tgit@github.com:o/r.git (push)\nup\t/local/path (fetch)\n",
    );
    assert_eq!(remotes.len(), 2);
    assert_eq!(remotes[0].push_url, "git@github.com:o/r.git");
    assert_eq!(remotes[1].push_url, "/local/path");
}

#[test]
fn reads_hosted_urls() {
    for url in [
        "https://github.com/octo/hello.git",
        "https://user:token@github.com/octo/hello/",
        "git@github.com:octo/hello.git",
        "ssh://git@github.com:22/octo/hello",
        "git://github.com/octo/hello",
    ] {
        let repo = parse_hosted(url).unwrap_or_else(|| panic!("{url}"));
        assert_eq!(
            (repo.host.as_str(), repo.full_name().as_str()),
            ("github.com", "octo/hello")
        );
    }
    let enterprise = parse_hosted("https://ghe.example.com/team/app.git").unwrap();
    assert_eq!(enterprise.web_url(), "https://ghe.example.com/team/app");
    assert_eq!(parse_hosted("/srv/git/repo.git"), None);
    assert_eq!(parse_hosted("https://github.com/only"), None);
    assert_eq!(parse_hosted("https://github.com/a/b/c"), None);
}

#[test]
fn clone_inputs() {
    assert_eq!(
        clone_url("octo/hello").as_deref(),
        Some("https://github.com/octo/hello.git")
    );
    assert_eq!(
        clone_url(" https://x/y.git ").as_deref(),
        Some("https://x/y.git")
    );
    assert_eq!(clone_url("git@h:o/r.git").as_deref(), Some("git@h:o/r.git"));
    assert_eq!(clone_url("not a url"), None);
    assert_eq!(clone_url(""), None);
    assert_eq!(
        clone_folder_name("https://github.com/o/hello.git").as_deref(),
        Some("hello")
    );
    assert_eq!(clone_folder_name("git@h:o/r").as_deref(), Some("r"));
}

#[test]
fn classifies_refusals() {
    let cases = [
        (
            " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs",
            Refusal::NotFastForward,
        ),
        (
            " ! [rejected] main -> main (non-fast-forward)",
            Refusal::NotFastForward,
        ),
        (
            " ! [rejected] main -> main (stale info)",
            Refusal::StaleLease,
        ),
        (
            "fatal: Authentication failed for 'https://github.com/o/r.git/'",
            Refusal::Authentication,
        ),
        (
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            Refusal::Authentication,
        ),
        (
            "git@github.com: Permission denied (publickey).",
            Refusal::Authentication,
        ),
        (
            "remote: Permission to o/r.git denied to someone.\nfatal: unable to access",
            Refusal::PermissionDenied,
        ),
        (
            "remote: error: GH006: Protected branch update failed",
            Refusal::ProtectedBranch,
        ),
        (
            "remote: error: GH001: Large files detected.",
            Refusal::FileTooLarge,
        ),
        (
            "! [remote rejected] main -> main (pre-receive hook declined)",
            Refusal::HookDeclined,
        ),
        ("remote: Repository not found.", Refusal::RepositoryNotFound),
        (
            "fatal: 'nope' does not appear to be a git repository",
            Refusal::RepositoryNotFound,
        ),
        (
            "fatal: unable to access 'https://x/': Could not resolve host: x",
            Refusal::Network,
        ),
        (
            "fatal: The current branch topic has no upstream branch.",
            Refusal::NoUpstream,
        ),
        (
            "error: Your local changes to the following files would be overwritten by merge:",
            Refusal::LocalChangesWouldBeOverwritten,
        ),
        (
            "CONFLICT (content): Merge conflict in a.txt",
            Refusal::Conflicts,
        ),
        (
            "hint: You have divergent branches and need to specify how to reconcile them.",
            Refusal::Diverged,
        ),
        ("something else", Refusal::Other),
    ];
    for (message, expected) in cases {
        assert_eq!(classify_refusal(message), expected, "{message}");
    }
}
