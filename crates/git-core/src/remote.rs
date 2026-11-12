//! Remotes: `git remote -v`, which host a URL points at, and what a refused
//! fetch, pull, or push means.

/// One remote, with its fetch and push URLs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Remote {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

/// Reads `git remote -v`: `name<TAB>url (fetch|push)` lines. Lines that do
/// not have that shape are skipped; a remote seen only with `(fetch)` pushes
/// to the same URL.
pub fn parse_remotes(output: &[u8]) -> Vec<Remote> {
    let text = String::from_utf8_lossy(output);
    let mut remotes: Vec<Remote> = Vec::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let Some((url, kind)) = rest.rsplit_once(' ') else {
            continue;
        };
        let index = match remotes.iter().position(|r| r.name == name) {
            Some(index) => index,
            None => {
                remotes.push(Remote {
                    name: name.to_string(),
                    fetch_url: String::new(),
                    push_url: String::new(),
                });
                remotes.len() - 1
            }
        };
        match kind {
            "(fetch)" => remotes[index].fetch_url = url.to_string(),
            "(push)" => remotes[index].push_url = url.to_string(),
            _ => {}
        }
    }
    for remote in &mut remotes {
        if remote.push_url.is_empty() {
            remote.push_url = remote.fetch_url.clone();
        }
    }
    remotes
}

/// A repository on a GitHub host, read from a remote URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostedRepository {
    /// `github.com`, or an Enterprise host.
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl HostedRepository {
    /// `owner/name`.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// The repository's page.
    pub fn web_url(&self) -> String {
        format!("https://{}/{}/{}", self.host, self.owner, self.name)
    }
}

/// Reads the host, owner, and name from a remote URL.
///
/// Expected results:
/// - `https://host/owner/name(.git)(/)`, `http://...`, `git@host:owner/name(.git)`,
///   `ssh://git@host(:port)/owner/name(.git)`, and `git://host/owner/name`
///   are read; credentials before `@` in an https URL are dropped.
/// - Anything else (a local path, a URL with fewer than two path parts) is
///   `None`.
pub fn parse_hosted(url: &str) -> Option<HostedRepository> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("git://"))
        .or_else(|| url.strip_prefix("ssh://"))
    {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split_once(':').map_or(host, |(h, _)| h);
        (host, path)
    } else if let Some((user_host, path)) = url.split_once(':') {
        if user_host.contains('/') || !user_host.contains('@') {
            return None;
        }
        let host = user_host.rsplit_once('@').map_or(user_host, |(_, h)| h);
        (host, path)
    } else {
        return None;
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next().filter(|p| !p.is_empty())?;
    let name = parts.next().filter(|p| !p.is_empty())?;
    if parts.next().is_some() || host.is_empty() {
        return None;
    }
    Some(HostedRepository {
        host: host.to_ascii_lowercase(),
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// What a person typed in the clone dialog: a URL, or `owner/name` for
/// GitHub.com, turned into a URL git can clone.
pub fn clone_url(typed: &str) -> Option<String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    if typed.contains("://") || (typed.contains('@') && typed.contains(':')) {
        return Some(typed.to_string());
    }
    let parts: Vec<&str> = typed.split('/').collect();
    if parts.len() == 2 && parts.iter().all(|p| !p.is_empty() && !p.contains(' ')) {
        return Some(format!("https://github.com/{}/{}.git", parts[0], parts[1]));
    }
    None
}

/// The folder name a clone goes into: the repository's name from the URL.
pub fn clone_folder_name(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_string())
}

/// Why git refused a network operation, read from its message.
///
/// git has no machine-readable form for these, so this is the one place
/// Better Git reads text git wrote for people, to choose an explanation.
/// It runs with `LC_ALL=C`, so the text is git's English, and nothing but the
/// explanation depends on it: git's own message is always shown beside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The remote has commits the branch does not: pull first.
    NotFastForward,
    /// The lease of a force push no longer matches: fetch, look, retry.
    StaleLease,
    /// The credentials were refused or missing.
    Authentication,
    /// Signed in, but this account may not write here.
    PermissionDenied,
    /// The branch is protected on the host.
    ProtectedBranch,
    /// A server hook declined the push.
    HookDeclined,
    /// A file is larger than the host accepts.
    FileTooLarge,
    /// The repository is not there, or this account cannot see it.
    RepositoryNotFound,
    /// The host could not be reached.
    Network,
    /// The branch has no upstream yet.
    NoUpstream,
    /// Pulling would overwrite local changes.
    LocalChangesWouldBeOverwritten,
    /// The pull stopped with conflicts.
    Conflicts,
    /// The branches have diverged and git was not told how to reconcile.
    Diverged,
    /// Anything else; only git's message is shown.
    Other,
}

/// Picks the explanation for a refused fetch, pull, or push.
pub fn classify_refusal(message: &str) -> Refusal {
    let m = message.to_ascii_lowercase();
    let has = |needle: &str| m.contains(needle);
    if has("stale info") {
        Refusal::StaleLease
    } else if has("gh006") || has("protected branch") {
        Refusal::ProtectedBranch
    } else if has("gh001") || has("exceeds github's file size limit") || has("large files detected")
    {
        Refusal::FileTooLarge
    } else if has("pre-receive hook declined") || has("hook declined") {
        Refusal::HookDeclined
    } else if has("non-fast-forward") || has("fetch first") || has("[rejected]") {
        Refusal::NotFastForward
    } else if has("permission to") && has("denied")
        || has("the requested url returned error: 403")
        || has("write access to repository not granted")
    {
        Refusal::PermissionDenied
    } else if has("authentication failed")
        || has("could not read username")
        || has("permission denied (publickey")
        || has("invalid username or password")
        || has("terminal prompts disabled")
    {
        Refusal::Authentication
    } else if has("repository not found")
        || has("does not appear to be a git repository")
        || has("not found") && has("repository")
    {
        Refusal::RepositoryNotFound
    } else if has("could not resolve host")
        || has("unable to access")
        || has("connection timed out")
        || has("connection refused")
        || has("network is unreachable")
    {
        Refusal::Network
    } else if has("has no upstream branch") || has("no tracking information") {
        Refusal::NoUpstream
    } else if has("would be overwritten by merge") || has("would be overwritten by checkout") {
        Refusal::LocalChangesWouldBeOverwritten
    } else if has("conflict") {
        Refusal::Conflicts
    } else if has("divergent branches") || has("need to specify how to reconcile") {
        Refusal::Diverged
    } else {
        Refusal::Other
    }
}
