//! The commit form: summary, description, co-authors, amend, and the
//! options GitHub Desktop offers, and when Commit is offered.

use git_platform::{CommitMessage, CommitOptions};

/// What the person typed and chose.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitForm {
    pub summary: String,
    pub description: String,
    /// One co-author per line: `Name <email>`.
    pub co_authors: String,
    pub show_co_authors: bool,
    /// Replace the last commit.
    pub amend: bool,
    pub no_verify: bool,
    pub allow_empty: bool,
    pub push_after: bool,
    /// Bumped whenever the session changes the text itself (clear after a
    /// commit, the last message put back after undo or amend), so the window
    /// copies it into its text fields.
    pub generation: u64,
}

/// Why Commit is not offered, or `None` when it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blocked {
    NoSummary,
    NothingIncluded,
    Busy,
    /// A merge, rebase, cherry-pick, or revert is in progress; its own
    /// Continue commits.
    OperationInProgress,
    /// A co-author line is not `Name <email>`.
    BadCoAuthor,
}

/// Reads the co-author lines. Blank lines are skipped; a line that is not
/// `Name <email>` is `Err` with its number (from 1).
pub fn parse_co_authors(text: &str) -> Result<Vec<(String, String)>, usize> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed = git_core::log::parse_co_author(&format!("Co-authored-by: {}", line.trim()));
        match parsed {
            Some((name, email)) if email.contains('@') => out.push((name, email)),
            _ => return Err(index + 1),
        }
    }
    Ok(out)
}

impl CommitForm {
    /// Whether Commit is offered.
    pub fn blocked(&self, included: bool, busy: bool, operation: bool) -> Option<Blocked> {
        if operation {
            Some(Blocked::OperationInProgress)
        } else if busy {
            Some(Blocked::Busy)
        } else if self.summary.trim().is_empty() {
            Some(Blocked::NoSummary)
        } else if !included && !self.amend && !self.allow_empty {
            Some(Blocked::NothingIncluded)
        } else if self.show_co_authors && parse_co_authors(&self.co_authors).is_err() {
            Some(Blocked::BadCoAuthor)
        } else {
            None
        }
    }

    pub fn message(&self) -> CommitMessage {
        CommitMessage {
            summary: self.summary.trim().to_string(),
            description: self.description.trim().to_string(),
        }
    }

    pub fn options(&self) -> CommitOptions {
        CommitOptions {
            amend: self.amend,
            no_verify: self.no_verify,
            allow_empty: self.allow_empty,
            co_authors: if self.show_co_authors {
                parse_co_authors(&self.co_authors).unwrap_or_default()
            } else {
                Vec::new()
            },
        }
    }

    /// Empties the form after a commit; the toggles that are about one
    /// commit go off, "push after commit" stays.
    pub fn clear(&mut self) {
        let push_after = self.push_after;
        let generation = self.generation + 1;
        *self = CommitForm {
            push_after,
            generation,
            ..CommitForm::default()
        };
    }

    /// Puts a commit's message back in the form (undo, or amend), with its
    /// co-author trailers in the co-author field.
    pub fn restore(&mut self, summary: &str, body: &str) {
        let mut description = Vec::new();
        let mut co_authors = Vec::new();
        for line in body.lines() {
            match git_core::log::parse_co_author(line) {
                Some((name, email)) => co_authors.push(format!("{name} <{email}>")),
                None => description.push(line),
            }
        }
        self.summary = summary.to_string();
        self.description = description.join("\n").trim().to_string();
        self.show_co_authors = !co_authors.is_empty();
        self.co_authors = co_authors.join("\n");
        self.generation += 1;
    }
}

/// GitHub Desktop's summary hint: past 50 characters, a summary reads
/// better shortened. The count is characters, not bytes.
pub const SUMMARY_HINT: usize = 50;

pub fn summary_too_long(summary: &str) -> bool {
    summary.trim().chars().count() > SUMMARY_HINT
}
