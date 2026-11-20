//! The changes list's operations: diffs of the working tree against HEAD,
//! building the index from what the person ticked, and discarding changes
//! with the discarded content kept in the desktop's trash.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use files_platform::{Put, TrashError};
use git_core::{
    DiffLineKind, FileDiff, LineKey, PatchMode, RawDiff, change_fingerprint, line_key, parse_diff,
};

use crate::{
    DiffOptions, DiffSide, GitError, Run, empty_tree, failed, has_commit, io_error, is_tracked,
    refused,
};

/// Which changed lines of a file are ticked, with the fingerprint of the
/// diff they were ticked on (`git_core::change_fingerprint`), so a diff
/// that changed meanwhile in any way is refused rather than applied to the
/// wrong lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineSelection {
    pub fingerprint: u64,
    /// The ticked lines, by their numbers (`git_core::line_key`), which do
    /// not depend on how much context the diff showed.
    pub selected: Vec<LineKey>,
}

/// One file of a commit or a discard.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSelection {
    pub path: PathBuf,
    /// For a rename, where it came from; it is committed or discarded with
    /// the new path.
    pub original_path: Option<PathBuf>,
    /// Some lines only; `None` is the whole file.
    pub lines: Option<LineSelection>,
}

impl FileSelection {
    pub fn whole(path: impl Into<PathBuf>) -> Self {
        FileSelection {
            path: path.into(),
            original_path: None,
            lines: None,
        }
    }

    fn paths(&self) -> Vec<&Path> {
        let mut paths = vec![self.path.as_path()];
        if let Some(original) = &self.original_path {
            paths.push(original);
        }
        paths
    }
}

/// How every diff Better Git reads starts: no colour, no external diff or
/// textconv program, and the `a/` and `b/` prefixes `git apply -p1`
/// expects whatever `diff.noprefix` or `diff.srcPrefix` say.
pub(crate) const DIFF_BASE: [&str; 6] = [
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// Largest file read for an image diff.
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

/// git's diff of one path, as bytes.
///
/// - `WorkingTree`: `git diff -M HEAD -- [original] <path>` (against the
///   empty tree before the first commit); an untracked path is
///   `git diff --no-index -- /dev/null <path>`, whose exit code 1 means
///   "differences found".
/// - `Staged`: `git diff --cached -- <path>`. `Unstaged`: `git diff -- <path>`,
///   or the `--no-index` form for an untracked path.
pub fn raw_diff(
    repository: &Path,
    path: &Path,
    original: Option<&Path>,
    side: DiffSide,
    options: DiffOptions,
) -> Result<Vec<u8>, GitError> {
    let base = DIFF_BASE;
    let untracked = side != DiffSide::Staged && !is_tracked(repository, path)?;
    if untracked {
        let captured = Run::new(repository, base)
            .args(options.args())
            .args(["--no-index", "--", "/dev/null"])
            .arg(path)
            .capture()?;
        return match captured.code() {
            // Exit 1 with nothing printed is an error git reported on
            // stderr, as it does for an untracked folder that is a
            // repository of its own.
            Some(1) if captured.stdout.is_empty() && !captured.stderr.is_empty() => {
                Err(failed(&captured))
            }
            Some(0) | Some(1) => Ok(captured.stdout),
            _ => Err(failed(&captured)),
        };
    }
    let mut run = Run::new(repository, base).args(options.args());
    match side {
        DiffSide::Unstaged => {}
        DiffSide::Staged => run = run.arg("--cached"),
        DiffSide::WorkingTree => {
            let head = if has_commit(repository)? {
                "HEAD".to_string()
            } else {
                empty_tree(repository)?
            };
            run = run.arg("-M").arg(head);
        }
    }
    let mut paths: Vec<&Path> = Vec::new();
    if let Some(original) = original.filter(|_| side == DiffSide::WorkingTree) {
        paths.push(original);
    }
    paths.push(path);
    run.paths(paths).output()
}

/// The bytes of a file at HEAD and in the working tree. Either is `None`
/// when that side has no file; a file larger than 32 MiB is refused.
pub fn file_versions(
    repository: &Path,
    path: &Path,
    original: Option<&Path>,
) -> Result<crate::FileVersions, GitError> {
    let old_path = original.unwrap_or(path);
    let old = if has_commit(repository)? {
        let mut spec = b"HEAD:".to_vec();
        spec.extend_from_slice(old_path.as_os_str().as_bytes());
        let size = Run::new(repository, ["cat-file", "-s"])
            .arg(OsString::from_vec(spec.clone()))
            .capture()?;
        if size.status.success() {
            let bytes: u64 = String::from_utf8_lossy(&size.stdout)
                .trim()
                .parse()
                .unwrap_or(u64::MAX);
            if bytes > MAX_IMAGE_BYTES {
                return Err(refused("the file is too large to show"));
            }
            Some(
                Run::new(repository, ["cat-file", "blob"])
                    .arg(OsString::from_vec(spec))
                    .output()?,
            )
        } else {
            None
        }
    } else {
        None
    };
    let new = read_capped(&repository.join(path), MAX_IMAGE_BYTES)?;
    Ok((old, new))
}

/// A working-tree file's bytes, at most `limit` of them: `None` when there
/// is no file, and refused when it is not a plain file (a symbolic link to
/// `/dev/zero` would never end) or is larger than `limit`. The file is
/// opened without following a link, and read through a cap, so a file
/// swapped or grown meanwhile cannot make the read run on.
pub fn read_capped(full: &Path, limit: u64) -> Result<Option<Vec<u8>>, GitError> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;

    match full.symlink_metadata() {
        Ok(meta) if !meta.file_type().is_file() => {
            return Err(refused("the file is not a plain file"));
        }
        Ok(meta) if meta.len() > limit => return Err(refused("the file is too large to show")),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(full)
        .map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.file_type().is_file() {
        return Err(refused("the file is not a plain file"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > limit {
        return Err(refused("the file is too large to show"));
    }
    Ok(Some(bytes))
}

/// Makes the index hold exactly the selection on top of HEAD: the index is
/// read back from HEAD (or emptied before the first commit), whole files
/// are added with `git add -A`, and partly ticked files get a patch applied
/// with `git apply --cached`. `git read-tree` is used rather than `git
/// reset`, which would also end a merge in progress.
pub fn build_index(repository: &Path, files: &[FileSelection]) -> Result<(), GitError> {
    if has_commit(repository)? {
        Run::new(repository, ["read-tree", "HEAD"]).run()?;
    } else {
        Run::new(repository, ["read-tree", "--empty"]).run()?;
    }
    let whole: Vec<&Path> = files
        .iter()
        .filter(|file| file.lines.is_none())
        .flat_map(FileSelection::paths)
        .collect();
    if !whole.is_empty() {
        Run::new(repository, ["add", "-A"]).paths(whole).run()?;
    }
    for file in files {
        let Some(lines) = &file.lines else { continue };
        let patch = partial_patch(repository, file, lines, PatchMode::ToIndex)?;
        if let Some(patch) = patch {
            Run::new(
                repository,
                [
                    "apply",
                    "--cached",
                    "-p1",
                    "--whitespace=nowarn",
                    "--recount",
                    "-",
                ],
            )
            .stdin(patch)
            .run()?;
        }
    }
    Ok(())
}

/// The index as a tree, so what was staged but left out of a commit can be
/// put back after it ([`restore_index`]); `None` when the index cannot be
/// written as a tree (an unresolved conflict), and nothing is put back.
pub fn save_index(repository: &Path) -> Option<String> {
    let out = Run::new(repository, ["write-tree"]).output().ok()?;
    let tree = String::from_utf8(out).ok()?.trim().to_string();
    (!tree.is_empty()).then_some(tree)
}

/// After a commit of `files`: the index as it was before ([`save_index`]),
/// with the committed paths as the new HEAD has them. A staged change that
/// was not ticked (a `git mv` left for later) stays staged, so it stays one
/// rename row instead of a deletion and an untracked file (ticket 123).
pub fn restore_index(
    repository: &Path,
    tree: &str,
    files: &[FileSelection],
) -> Result<(), GitError> {
    Run::new(repository, ["read-tree"]).arg(tree).run()?;
    for path in files.iter().flat_map(FileSelection::paths) {
        // A path neither HEAD nor the index has is already as HEAD has it.
        let _ = Run::new(repository, ["reset", "-q", "HEAD", "--"])
            .paths([path])
            .run();
    }
    Ok(())
}

fn partial_patch(
    repository: &Path,
    file: &FileSelection,
    lines: &LineSelection,
    mode: PatchMode,
) -> Result<Option<Vec<u8>>, GitError> {
    let raw = raw_diff(
        repository,
        &file.path,
        file.original_path.as_deref(),
        DiffSide::WorkingTree,
        DiffOptions::default(),
    )?;
    let shown = parse_diff(&raw).map_err(crate::unreadable)?;
    if change_fingerprint(&shown) != lines.fingerprint {
        return Err(GitError::ChangedMeanwhile);
    }
    let FileDiff::Text { hunks } = shown else {
        return Err(GitError::ChangedMeanwhile);
    };
    let raw = RawDiff::parse(&raw).map_err(crate::unreadable)?;
    if raw.shape() != hunks.iter().map(|h| h.lines.len()).collect::<Vec<_>>() {
        return Err(GitError::ChangedMeanwhile);
    }
    Ok(raw.partial(
        |h, l| {
            hunks[h].lines.get(l).is_some_and(|line| {
                line.kind != DiffLineKind::Context && lines.selected.contains(&line_key(line))
            })
        },
        mode,
    ))
}

/// Discards each file's changes against HEAD. The working-tree file is
/// copied to the desktop's trash first, so what was discarded can be got
/// back from Better Files or any file manager's trash.
///
/// - A path HEAD has is restored with `git restore --source=HEAD --staged
///   --worktree`.
/// - A path HEAD does not have (untracked, or added) is taken out of the
///   index and moved to the trash.
pub fn discard_files(repository: &Path, files: &[FileSelection]) -> Result<(), GitError> {
    let committed = has_commit(repository)?;
    for file in files {
        for path in file.paths() {
            let full = repository.join(path);
            let in_head = committed && in_head(repository, path)?;
            if full.symlink_metadata().is_ok() {
                trash(&full, !in_head)?;
            }
            if in_head {
                Run::new(
                    repository,
                    ["restore", "--source=HEAD", "--staged", "--worktree"],
                )
                .paths([path])
                .run()?;
            } else if is_tracked(repository, path)? {
                Run::new(
                    repository,
                    ["rm", "--cached", "-q", "-f", "--ignore-unmatch"],
                )
                .paths([path])
                .run()?;
            }
        }
    }
    Ok(())
}

/// Discards the ticked lines of one file: a copy goes to the trash, then the
/// patch of those lines is applied to the file in reverse.
pub fn discard_lines(repository: &Path, file: &FileSelection) -> Result<(), GitError> {
    let Some(lines) = &file.lines else {
        return discard_files(repository, std::slice::from_ref(file));
    };
    let Some(patch) = partial_patch(repository, file, lines, PatchMode::Discard)? else {
        return Ok(());
    };
    let full = repository.join(&file.path);
    if full.exists() {
        trash(&full, false)?;
    }
    Run::new(
        repository,
        ["apply", "-R", "-p1", "--whitespace=nowarn", "-"],
    )
    .stdin(patch)
    .run()
}

fn in_head(repository: &Path, path: &Path) -> Result<bool, GitError> {
    let out = Run::new(repository, ["ls-tree", "--name-only", "HEAD"])
        .paths([path])
        .output()?;
    Ok(!out.is_empty())
}

/// Appends a pattern to the repository's `.gitignore`, on a line of its own.
/// A `.gitignore` that is a symbolic link is refused (the write would land
/// wherever it points), and the file is opened without following one, so a
/// link put there meanwhile is refused too.
pub fn ignore(repository: &Path, pattern: &str) -> Result<(), GitError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let pattern = pattern.trim_start();
    if pattern.trim().is_empty() || pattern.contains(['\n', '\r', '\0']) {
        return Err(refused("the pattern is empty"));
    }
    let path = repository.join(".gitignore");
    match path.symlink_metadata() {
        Ok(meta) if !meta.file_type().is_file() => {
            return Err(refused(".gitignore is not a plain file"));
        }
        _ => {}
    }
    let text = match fs::read(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(io_error(error)),
    };
    if String::from_utf8_lossy(&text)
        .lines()
        .any(|line| line.trim() == pattern)
    {
        return Ok(());
    }
    let mut line = Vec::new();
    if !text.is_empty() && !text.ends_with(b"\n") {
        line.push(b'\n');
    }
    line.extend_from_slice(pattern.as_bytes());
    line.push(b'\n');
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .and_then(|mut file| file.write_all(&line))
        .map_err(io_error)
}

/// The pattern that ignores every file with this one's extension, as
/// GitHub Desktop offers it: `*.log`.
pub fn extension_pattern(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?;
    Some(format!("*.{extension}"))
}

/// The pattern that ignores exactly this file: its path from the root, with
/// every character `.gitignore` treats specially escaped, trailing spaces
/// included (git drops them unless escaped).
pub fn path_pattern(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut out = String::from("/");
    let keep = text.trim_end_matches(' ').len();
    for (i, c) in text.char_indices() {
        if matches!(c, '*' | '?' | '[' | ']' | '!' | '#' | '\\') || (c == ' ' && i >= keep) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Puts the file in the trash the freedesktop.org trash specification gives
/// it, so Better Files or any file manager can put it back where it was: the
/// file itself when `take` is set, a copy otherwise. The writer is Better
/// Files' (`files_platform::trash`, ticket 173), so a discard names, escapes,
/// and places its item exactly as a deletion in Better Files does.
fn trash(path: &Path, take: bool) -> Result<(), GitError> {
    let put = if take { Put::Move } else { Put::Copy };
    files_platform::trash_path(path, put)
        .map(|_| ())
        .map_err(trash_failure)
}

/// A trash failure as the system's own words about the file, the way the
/// I/O error this replaced read: Better Git shows `GitError::Io`'s text as
/// it is, and a `files.trash.error.…` key must not reach its window
/// (ticket 172).
fn trash_failure(error: TrashError) -> GitError {
    GitError::Io(match error {
        TrashError::Io { path, reason } | TrashError::Unusable { path, reason } => {
            format!("{}: {reason}", path.display())
        }
        TrashError::NotFound { path } => format!("{}: not found", path.display()),
        TrashError::CrossDevice { path } => {
            format!("{}: the trash is on another device", path.display())
        }
        TrashError::DestinationOccupied { path } | TrashError::OriginalParentMissing { path } => {
            path.display().to_string()
        }
        TrashError::NoRecord { item } => item,
    })
}

#[cfg(test)]
mod tests {
    use super::{GitError, TrashError, trash_failure};
    use std::path::PathBuf;

    #[test]
    fn a_trash_failure_reaches_the_window_as_words_about_the_file_not_a_key() {
        let path = PathBuf::from("/work/repo/notes.txt");
        for error in [
            TrashError::Io {
                path: path.clone(),
                reason: "permission denied".into(),
            },
            TrashError::Unusable {
                path: path.clone(),
                reason: "symlink".into(),
            },
            TrashError::NotFound { path: path.clone() },
            TrashError::CrossDevice { path: path.clone() },
        ] {
            let GitError::Io(text) = trash_failure(error) else {
                panic!("a trash failure is an I/O failure");
            };
            assert!(text.starts_with("/work/repo/notes.txt"), "{text}");
            assert!(!text.contains("files.trash"), "{text}");
        }
    }
}
