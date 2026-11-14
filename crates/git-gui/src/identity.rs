//! The name and email every commit carries, asked for the first time Better
//! Git would commit without one (GitHub Desktop's "Configure Git" step).
//!
//! Git refuses a commit when it cannot tell who is making it, and says so in
//! text that ends by telling the person to type `git config --global`. Better
//! Git asks `git var` before it commits or creates a repository, so the
//! refusal never happens; when Git has no identity the window asks for a
//! name and an email, prefilled from the signed-in GitHub account, shows the
//! two settings it is about to write, writes them itself, and then does what
//! it was about to do.

use git_platform::GitError;
use github_api::{Host, User};

/// Whether this error is Git refusing because it has no name or email.
/// Git is run with `LC_ALL=C`, so these are its English messages
/// (`ident.c`: "Author identity unknown", "empty ident name", "unable to
/// auto-detect email address", "no email was given and auto-detection is
/// disabled", "no name was given and auto-detection is disabled").
pub fn is_identity_error(error: &GitError) -> bool {
    let GitError::Failed { message, .. } = error else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    [
        "identity unknown",
        "empty ident name",
        "unable to auto-detect email address",
        "no email was given",
        "no name was given",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// Whether a check of Git's identity lets the commit go ahead. A check that
/// could not run does, so Git's own answer to the commit is the last word.
pub fn known(check: Result<bool, GitError>) -> bool {
    check.unwrap_or(true)
}

/// The two fields of the identity step.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Draft {
    pub name: String,
    pub email: String,
}

/// The step's fields before the person types: what Git already has, then
/// the signed-in account's name and public email, then the account's
/// private no-reply address, as GitHub Desktop fills them.
pub fn prefill(
    configured_name: &str,
    configured_email: &str,
    account: Option<(&Host, &User)>,
) -> Draft {
    let name = non_empty(configured_name)
        .or_else(|| account.and_then(|(_, user)| user.name.as_deref().and_then(non_empty)))
        .or_else(|| account.map(|(_, user)| user.login.clone()))
        .unwrap_or_default();
    let email = non_empty(configured_email)
        .or_else(|| account.and_then(|(_, user)| user.email.as_deref().and_then(non_empty)))
        .or_else(|| account.map(|(host, user)| no_reply(host, &user.login)))
        .unwrap_or_default();
    Draft { name, email }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The address GitHub keeps for a person who hides theirs:
/// `login@users.noreply.github.com`, or the Enterprise server's own.
pub fn no_reply(host: &Host, login: &str) -> String {
    if host.name == "github.com" {
        format!("{login}@users.noreply.github.com")
    } else {
        format!("{login}@users.noreply.{}", host.name)
    }
}

/// Why the step's button is off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Problem {
    /// Field 0.
    NoName,
    /// Field 1.
    NoEmail,
    /// Field 1: no `@` with something on each side.
    NotAnEmail,
}

impl Problem {
    /// The field the problem is about: 0 the name, 1 the email.
    pub fn field(self) -> usize {
        match self {
            Problem::NoName => 0,
            Problem::NoEmail | Problem::NotAnEmail => 1,
        }
    }
}

/// Checks the two fields, the name first.
pub fn check(name: &str, email: &str) -> Option<Problem> {
    let email = email.trim();
    if name.trim().is_empty() {
        Some(Problem::NoName)
    } else if email.is_empty() {
        Some(Problem::NoEmail)
    } else if !looks_like_email(email) {
        Some(Problem::NotAnEmail)
    } else {
        None
    }
}

fn looks_like_email(email: &str) -> bool {
    match email.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.contains('@')
                && !email
                    .chars()
                    .any(|c| c.is_whitespace() || c == '<' || c == '>')
        }
        None => false,
    }
}

/// The two settings, in the order the step shows them.
pub fn writes(name: &str, email: &str) -> Vec<(String, String)> {
    vec![
        ("user.name".to_string(), name.trim().to_string()),
        ("user.email".to_string(), email.trim().to_string()),
    ]
}

/// What the step writes: only the settings Git does not have yet. A value
/// the person already set is kept, whatever the field says.
pub fn writes_missing(
    name: &str,
    email: &str,
    configured_name: &str,
    configured_email: &str,
) -> Vec<(String, String)> {
    writes(name, email)
        .into_iter()
        .zip([configured_name, configured_email])
        .filter(|(_, configured)| configured.trim().is_empty())
        .map(|(write, _)| write)
        .collect()
}

/// Whether the step shows a setting as already set (kept, not editable) or
/// as new.
pub fn kept(configured: &str) -> bool {
    !configured.trim().is_empty()
}
