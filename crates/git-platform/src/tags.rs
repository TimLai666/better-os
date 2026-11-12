//! Tags: list, create, delete.

use std::path::Path;

use git_core::tag::TAG_FORMAT;
use git_core::{Tag, parse_tags};

use crate::{GitError, Run, refused, unreadable};

pub fn list(repository: &Path) -> Result<Vec<Tag>, GitError> {
    let out = Run::new(repository, ["for-each-ref", TAG_FORMAT, "refs/tags"]).output()?;
    parse_tags(&out).map_err(unreadable)
}

/// A lightweight tag, as GitHub Desktop makes them.
pub fn create(repository: &Path, name: &str, sha: &str) -> Result<(), GitError> {
    if name.starts_with('-') {
        return Err(refused("a tag name cannot start with -"));
    }
    Run::new(repository, ["tag", "--end-of-options", name, sha]).run()
}

pub fn delete(repository: &Path, name: &str) -> Result<(), GitError> {
    Run::new(repository, ["tag", "-d", "--end-of-options", name]).run()
}
