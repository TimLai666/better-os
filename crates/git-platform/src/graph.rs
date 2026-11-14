//! The branch graph's commits, read from one `git log --topo-order` that
//! stays open while the person scrolls.
//!
//! A history of 100,000 commits is too long to read before the first row is
//! drawn, and `--skip` would make git walk every skipped commit again for
//! each page. So one `git log -z` runs for the life of the graph and is read
//! a page at a time: what git prints beyond the page waits in the pipe, and
//! git waits on the pipe, until the next page is asked for. Dropping the
//! source stops git.

use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout};
use std::thread::JoinHandle;

use git_core::log::LOG_FORMAT;
use git_core::{Commit, RecordReader};

use crate::{GitError, Run, has_commit, io_failed, unreadable};

/// Whose history the graph shows.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum GraphScope {
    /// Every local branch, and remote branches and tags when they are
    /// shown.
    #[default]
    All,
    /// Local branches only.
    Local,
    /// The branch HEAD is on (or the detached HEAD) only.
    Current,
}

impl GraphScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Local => "local",
            Self::Current => "current",
        }
    }

    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "local" => Some(Self::Local),
            "current" => Some(Self::Current),
            _ => None,
        }
    }
}

/// What the graph is asked to show.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphQuery {
    pub scope: GraphScope,
    /// Remote-tracking branches: walked under `All`, labelled under every
    /// scope.
    pub remotes: bool,
    /// Tags: walked under `All`, labelled under every scope.
    pub tags: bool,
}

impl Default for GraphQuery {
    fn default() -> Self {
        Self {
            scope: GraphScope::All,
            remotes: true,
            tags: true,
        }
    }
}

impl GraphQuery {
    /// The revisions `git log` starts from.
    ///
    /// Expected results:
    /// - `All` is `--branches`, then `--remotes` and `--tags` when shown, then
    ///   `HEAD`; the stash and notes are never walked (`--all` would show
    ///   the stash's internal commits).
    /// - `Local` is `--branches` and `HEAD`; `Current` is `HEAD`.
    /// - `HEAD` is left out when it has no commit yet, so an unborn branch
    ///   beside others still shows them.
    pub fn revisions(&self, head_born: bool) -> Vec<&'static str> {
        let mut revisions = Vec::new();
        match self.scope {
            GraphScope::All => {
                revisions.push("--branches");
                if self.remotes {
                    revisions.push("--remotes");
                }
                if self.tags {
                    revisions.push("--tags");
                }
            }
            GraphScope::Local => revisions.push("--branches"),
            GraphScope::Current => {}
        }
        if head_born {
            revisions.push("HEAD");
        }
        revisions
    }
}

/// Commits for the graph, newest first in topological order, a page at a
/// time. A source is used from one thread at a time and may be moved
/// between them.
pub trait GraphSource: Send {
    /// Up to `max` more commits; fewer only at the end of the history.
    /// After the end it returns no commits.
    fn next_page(&mut self, max: usize) -> Result<Vec<Commit>, GitError>;
    /// Whether every commit has been returned.
    fn is_done(&self) -> bool;
}

/// A source over commits already in memory, for a fake `Git` and for the
/// end of a repository with no commit.
#[derive(Clone, Debug, Default)]
pub struct MemorySource {
    commits: VecDeque<Commit>,
}

impl MemorySource {
    pub fn new(commits: Vec<Commit>) -> Self {
        Self {
            commits: commits.into(),
        }
    }
}

impl GraphSource for MemorySource {
    fn next_page(&mut self, max: usize) -> Result<Vec<Commit>, GitError> {
        let n = max.min(self.commits.len());
        Ok(self.commits.drain(..n).collect())
    }

    fn is_done(&self) -> bool {
        self.commits.is_empty()
    }
}

/// The revisions `query` names, resolved to object ids in git's order
/// (`git rev-parse`). An annotated tag resolves to the tag object, which
/// `git log` peels to its commit. No revisions (an unborn current branch)
/// is no tips.
pub fn tips(repository: &Path, query: &GraphQuery) -> Result<Vec<String>, GitError> {
    let born = has_commit(repository)?;
    let revisions = query.revisions(born);
    if revisions.is_empty() {
        return Ok(Vec::new());
    }
    let out = Run::new(repository, ["rev-parse"])
        .args(revisions)
        .output()?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// How much is read from git's pipe at once.
const CHUNK: usize = 64 * 1024;

/// A running `git log -z --topo-order --decorate=full` in
/// `git_core::log::LOG_FORMAT`, read as it prints.
pub struct LogStream {
    child: Child,
    stdout: ChildStdout,
    stderr: Option<JoinHandle<Vec<u8>>>,
    reader: RecordReader,
    ready: VecDeque<Commit>,
    done: bool,
    /// git has been waited for; nothing is left to stop.
    reaped: bool,
}

impl LogStream {
    /// Starts git for `query` in `repository`, leaving out the first `skip`
    /// commits.
    pub fn start(
        repository: &Path,
        query: &GraphQuery,
        tips: &[String],
        skip: usize,
    ) -> Result<Box<dyn GraphSource>, GitError> {
        let run = Run::new(
            repository,
            ["log", "-z", "--topo-order", "--decorate=full", LOG_FORMAT],
        )
        .arg(format!("--skip={skip}"));
        let run = if tips.is_empty() {
            let born = has_commit(repository)?;
            if !born && query.scope == GraphScope::Current {
                return Ok(Box::new(MemorySource::default()));
            }
            run.args(query.revisions(born))
        } else {
            for tip in tips {
                if tip.is_empty() || !tip.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(crate::refused("a graph tip is not a commit id"));
                }
            }
            // Through stdin, one per line: tens of thousands of tags as
            // arguments would pass the system's argument length limit.
            let mut list = tips.join("\n");
            list.push('\n');
            run.arg("--stdin").stdin(list.into_bytes())
        };
        let mut child = run.arg("--").spawn_reading()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GitError::Io("git's output could not be read".to_string()))?;
        // Read beside stdout so a git that prints a lot of warnings never
        // blocks on a full stderr pipe.
        let stderr = child.stderr.take().map(|mut pipe: ChildStderr| {
            std::thread::spawn(move || {
                let mut out = Vec::new();
                let _ = pipe.read_to_end(&mut out);
                out
            })
        });
        Ok(Box::new(LogStream {
            child,
            stdout,
            stderr,
            reader: RecordReader::new(),
            ready: VecDeque::new(),
            done: false,
            reaped: false,
        }))
    }

    /// Reading stopped early (unreadable output, a failed read): git may be
    /// blocked on a full pipe, so it is ended here rather than waited for.
    fn abandon(&mut self) {
        self.done = true;
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }

    fn finish(&mut self) -> Result<(), GitError> {
        self.done = true;
        // git closed its output: it has exited or is about to.
        let status = self.child.wait().map_err(io_failed);
        self.reaped = true;
        let status = status?;
        if let Some(last) = self.reader.finish().map_err(unreadable)? {
            self.ready.push_back(last);
        }
        let stderr = self
            .stderr
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        if !status.success() {
            return Err(crate::failed(&crate::Captured {
                status,
                stdout: Vec::new(),
                stderr,
            }));
        }
        Ok(())
    }
}

impl GraphSource for LogStream {
    fn next_page(&mut self, max: usize) -> Result<Vec<Commit>, GitError> {
        let mut chunk = vec![0u8; CHUNK];
        while self.ready.len() < max && !self.done {
            let n = match self.stdout.read(&mut chunk) {
                Ok(n) => n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.abandon();
                    return Err(io_failed(error));
                }
            };
            if n == 0 {
                self.finish()?;
                break;
            }
            match self.reader.feed(&chunk[..n]) {
                Ok(commits) => self.ready.extend(commits),
                Err(error) => {
                    self.abandon();
                    return Err(unreadable(error));
                }
            }
        }
        let n = max.min(self.ready.len());
        Ok(self.ready.drain(..n).collect())
    }

    fn is_done(&self) -> bool {
        self.done && self.ready.is_empty()
    }
}

impl Drop for LogStream {
    /// Stops git when the graph is closed or read again before the end.
    /// git is killed unless it is known to have exited: one blocked on a
    /// full pipe nobody reads would never exit, and waiting for it would
    /// hang whichever thread drops the stream.
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
