//! The person's global git configuration, and making a new repository.

use std::fs;
use std::path::Path;

use crate::{GitError, NewRepository, Run, io_error, refused};

/// The keys the settings page may read and write.
pub const EDITABLE_KEYS: [&str; 3] = ["user.name", "user.email", "init.defaultBranch"];

fn editable(key: &str) -> Result<(), GitError> {
    if EDITABLE_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(refused("that setting is not one Better Git changes"))
    }
}

/// `git config --global --get <key>`; unset is `None`.
pub fn get_global(key: &str) -> Result<Option<String>, GitError> {
    editable(key)?;
    let captured = Run::new(Path::new("/"), ["config", "--global", "--get", key]).capture()?;
    match captured.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&captured.stdout).trim().to_string(),
        )),
        Some(1) => Ok(None),
        _ => Err(crate::failed(&captured)),
    }
}

/// `git config --global <key> <value>`, or `--unset` for an empty value.
pub fn set_global(key: &str, value: &str) -> Result<(), GitError> {
    editable(key)?;
    let value = value.trim();
    if value.is_empty() {
        let captured =
            Run::new(Path::new("/"), ["config", "--global", "--unset", key]).capture()?;
        // 5 is "the key was not set", which is what was wanted.
        return match captured.code() {
            Some(0) | Some(5) => Ok(()),
            _ => Err(crate::failed(&captured)),
        };
    }
    Run::new(Path::new("/"), ["config", "--global", key, value]).run()
}

/// Makes a repository: the folder, `git init -b <branch>`, the chosen
/// README, `.gitignore`, and LICENSE, and a first commit of them when there
/// are any. A folder that already holds a repository is refused.
pub fn init(new: &NewRepository) -> Result<(), GitError> {
    if !new.path.is_absolute() {
        return Err(refused("the folder must be an absolute path"));
    }
    if new.path.join(".git").exists() {
        return Err(refused("that folder is already a repository"));
    }
    let existed = new.path.exists();
    fs::create_dir_all(&new.path).map_err(io_error)?;
    let branch = if new.default_branch.trim().is_empty() {
        "main"
    } else {
        new.default_branch.trim()
    };
    Run::new(&new.path, ["init", "-q", "-b", branch]).run()?;
    // The first commit needs a name and an email. Git is asked inside the
    // new repository, where its own and `includeIf` settings apply; when it
    // has none, what was just made is taken away again and the caller asks
    // the person (nothing is committed, so nothing was needed otherwise).
    let commits = new.readme.is_some() || new.gitignore.is_some() || new.license.is_some();
    if commits && !crate::watching::identity_known(&new.path).unwrap_or(true) {
        let _ = fs::remove_dir_all(new.path.join(".git"));
        if !existed {
            let _ = fs::remove_dir(&new.path);
        }
        return Err(GitError::Failed {
            exit_code: None,
            message: "Author identity unknown: no name or email for the first commit".into(),
        });
    }
    let mut written = Vec::new();
    for (name, text) in [
        ("README.md", &new.readme),
        (".gitignore", &new.gitignore),
        ("LICENSE", &new.license),
    ] {
        if let Some(text) = text {
            let path = new.path.join(name);
            if !path.exists() {
                fs::write(&path, text).map_err(io_error)?;
                written.push(Path::new(name));
            }
        }
    }
    if !written.is_empty() {
        Run::new(&new.path, ["add"]).paths(written).run()?;
        Run::new(&new.path, ["commit", "-q", "-m", "Initial commit"]).run()?;
    }
    Ok(())
}
