//! "Do you trust this repository?" — asked the first time a path is
//! opened, before any git command reads it. A repository's own
//! configuration can make git run programs (a filter on every read, hooks,
//! a diff or merge driver, `gpg.program`, an `ssh` command, a credential
//! helper, a configuration file it includes from anywhere), so the question
//! says plainly that a trusted repository can run any program, and lists
//! the ones found as examples. The answer is remembered per path by
//! `git_platform::trust::TrustStore`.

use std::path::{Path, PathBuf};

use git_platform::trust::TrustStore;

/// Whether opening `path` must ask first.
pub fn must_ask(store: &TrustStore, path: &Path) -> bool {
    !store.is_trusted(path)
}

/// `(section, key)` pairs whose value is a program git runs, or a file
/// that can name more. `None` is any section; an empty key is every key of
/// the section.
const PROGRAM_KEYS: [(Option<&str>, &str); 20] = [
    (Some("filter"), "clean"),
    (Some("filter"), "smudge"),
    (Some("filter"), "process"),
    (Some("diff"), "textconv"),
    (Some("diff"), "command"),
    (Some("diff"), "external"),
    (Some("merge"), "driver"),
    (Some("core"), "fsmonitor"),
    (Some("core"), "hookspath"),
    (Some("core"), "sshcommand"),
    (Some("core"), "editor"),
    (Some("core"), "pager"),
    (Some("core"), "askpass"),
    (Some("sequence"), "editor"),
    (Some("gpg"), "program"),
    (Some("credential"), "helper"),
    (Some("include"), "path"),
    (Some("includeif"), "path"),
    (Some("alias"), ""),
    (None, "program"),
];

/// The git directory's configuration file, following a `.git` file
/// (`gitdir: ...`, a worktree or a submodule) and a worktree's
/// `commondir`, where its configuration really is.
fn config_file(repository: &Path) -> Option<PathBuf> {
    let dot_git = repository.join(".git");
    let mut git_dir = dot_git.clone();
    if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        git_dir = repository.join(target);
    }
    if let Ok(common) = std::fs::read_to_string(git_dir.join("commondir")) {
        git_dir = git_dir.join(common.trim());
    }
    Some(git_dir.join("config"))
}

/// What this repository's own configuration names that git would run, and
/// its hooks, for the question to list. Read as text without running git;
/// best effort, never complete (the question says so).
pub fn configured_programs(repository: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let config = config_file(repository)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let mut section = String::new();
    for line in config.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            let name = header
                .split(|c: char| c == ']' || c == '"' || c.is_whitespace())
                .next()
                .unwrap_or("");
            section = name.split('.').next().unwrap_or("").to_ascii_lowercase();
            continue;
        }
        let key = line
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if key.is_empty() || key.starts_with(['#', ';']) {
            continue;
        }
        let names_a_program = PROGRAM_KEYS
            .iter()
            .any(|(s, k)| s.is_none_or(|s| s == section) && (k.is_empty() || *k == key));
        if names_a_program {
            found.push(line.to_string());
        }
    }
    if let Some(hooks) = config_file(repository).and_then(|c| c.parent().map(|d| d.join("hooks")))
        && let Ok(entries) = std::fs::read_dir(hooks)
    {
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with(".sample"))
            .map(|n| format!("hook: {n}"))
            .collect();
        names.sort();
        found.extend(names);
    }
    found
}
