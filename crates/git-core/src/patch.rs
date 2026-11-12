//! Patches made from part of a diff: commit some lines of a file and not
//! others, or discard some lines and keep the rest.
//!
//! The approach is GitHub Desktop's. The window shows the working tree
//! against HEAD, the person ticks lines, and at commit time the ticked lines
//! become a patch applied to the index with `git apply --cached`. Discarding
//! lines builds the opposite patch and applies it to the working tree in
//! reverse. The patch is made from git's own bytes, never from the lossy text
//! shown on screen, so a file that is not UTF-8 keeps its bytes.
//!
//! Lines are numbered the way [`crate::FileDiff`] numbers them: hunk index,
//! then line index within the hunk, with `\ No newline at end of file`
//! belonging to the line before it.

use crate::ParseError;

/// Which patch to build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchMode {
    /// Apply forwards to the index: unselected removals stay, unselected
    /// additions are left out.
    ToIndex,
    /// Apply in reverse (`git apply -R`) to the working tree: selected
    /// lines are undone, everything else stays as it is in the file.
    Discard,
}

#[derive(Clone, Debug)]
struct RawLine {
    /// The line with its leading ` `, `+`, or `-`, without the newline.
    bytes: Vec<u8>,
    /// git printed `\ No newline at end of file` after it.
    no_newline: bool,
}

#[derive(Clone, Debug)]
struct RawHunk {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    /// What follows the second `@@` on the header line.
    section: Vec<u8>,
    lines: Vec<RawLine>,
}

/// One file's diff as git printed it.
#[derive(Clone, Debug)]
pub struct RawDiff {
    header: Vec<Vec<u8>>,
    hunks: Vec<RawHunk>,
}

impl RawDiff {
    /// Reads git's unified diff of exactly one file.
    pub fn parse(output: &[u8]) -> Result<RawDiff, ParseError> {
        let mut header = Vec::new();
        let mut hunks: Vec<RawHunk> = Vec::new();
        let mut lines: Vec<&[u8]> = output.split(|&b| b == b'\n').collect();
        if lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        for line in lines {
            if line.starts_with(b"@@") {
                let (old_start, old_count, new_start, new_count, section) = hunk_header(line)?;
                hunks.push(RawHunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    section,
                    lines: Vec::new(),
                });
            } else if let Some(hunk) = hunks.last_mut() {
                if line.starts_with(b"\\") {
                    if let Some(last) = hunk.lines.last_mut() {
                        last.no_newline = true;
                    }
                } else if line.starts_with(b"diff --git") {
                    return Err(ParseError::Malformed(
                        "a partial patch is made from one file's diff".to_string(),
                    ));
                } else {
                    hunk.lines.push(RawLine {
                        bytes: line.to_vec(),
                        no_newline: false,
                    });
                }
            } else {
                header.push(line.to_vec());
            }
        }
        Ok(RawDiff { header, hunks })
    }

    /// How many hunks, and how many lines in each, to check a selection made
    /// on screen against the diff read again before applying it.
    pub fn shape(&self) -> Vec<usize> {
        self.hunks.iter().map(|h| h.lines.len()).collect()
    }

    /// The patch for the selected lines, or `None` when no selected line is a
    /// change.
    ///
    /// Expected results:
    /// - `ToIndex`: selected `+` and `-` lines stay changes; an unselected
    ///   `-` becomes context and an unselected `+` is left out.
    /// - `Discard`: selected changes stay changes; an unselected `+` becomes
    ///   context and an unselected `-` is left out. The result is applied with
    ///   `git apply -R`.
    /// - Hunks left with no change are dropped; the counts and the start of
    ///   the side that moves are recomputed.
    /// - A new file's `new file mode` header is kept for `ToIndex` (the index
    ///   gets the file with only the selected lines). A deletion that is only
    ///   partly selected becomes a modification. A new file only partly
    ///   discarded becomes a modification of the file on disk.
    pub fn partial(
        &self,
        selected: impl Fn(usize, usize) -> bool,
        mode: PatchMode,
    ) -> Option<Vec<u8>> {
        let mut body: Vec<u8> = Vec::new();
        let mut delta: i64 = 0;
        let mut every_change_selected = true;
        let mut any_change = false;
        for (h, hunk) in self.hunks.iter().enumerate() {
            let mut out: Vec<&RawLine> = Vec::new();
            let mut converted: Vec<(usize, u8)> = Vec::new();
            let (mut old_count, mut new_count) = (0u32, 0u32);
            let mut changes = 0;
            for (l, line) in hunk.lines.iter().enumerate() {
                let marker = line.bytes.first().copied().unwrap_or(b' ');
                let is_selected = selected(h, l);
                if matches!(marker, b'+' | b'-') && !is_selected {
                    every_change_selected = false;
                }
                let emitted = match (marker, is_selected, mode) {
                    (b'+', true, _) | (b'-', true, _) => Some(marker),
                    (b'-', false, PatchMode::ToIndex) => Some(b' '),
                    (b'+', false, PatchMode::Discard) => Some(b' '),
                    (b'+', false, _) | (b'-', false, _) => None,
                    _ => Some(b' '),
                };
                let Some(emitted) = emitted else { continue };
                match emitted {
                    b'+' => {
                        new_count += 1;
                        changes += 1;
                    }
                    b'-' => {
                        old_count += 1;
                        changes += 1;
                    }
                    _ => {
                        old_count += 1;
                        new_count += 1;
                    }
                }
                if emitted != marker {
                    converted.push((out.len(), emitted));
                }
                out.push(line);
            }
            if changes == 0 {
                continue;
            }
            any_change = true;
            let (old_start, new_start) = match mode {
                PatchMode::ToIndex => {
                    let old_anchor = anchor(hunk.old_start, hunk.old_count);
                    let new_anchor = (i64::from(old_anchor) + delta).max(0) as u32;
                    (
                        start_from(old_anchor, old_count),
                        start_from(new_anchor, new_count),
                    )
                }
                PatchMode::Discard => {
                    let new_anchor = anchor(hunk.new_start, hunk.new_count);
                    let old_anchor = (i64::from(new_anchor) - delta).max(0) as u32;
                    (
                        start_from(old_anchor, old_count),
                        start_from(new_anchor, new_count),
                    )
                }
            };
            delta += i64::from(new_count) - i64::from(old_count);
            body.extend_from_slice(
                format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@").as_bytes(),
            );
            body.extend_from_slice(&hunk.section);
            body.push(b'\n');
            for (index, line) in out.iter().enumerate() {
                match converted.iter().find(|(i, _)| *i == index) {
                    Some((_, marker)) => {
                        body.push(*marker);
                        body.extend_from_slice(line.bytes.get(1..).unwrap_or_default());
                    }
                    None => body.extend_from_slice(&line.bytes),
                }
                body.push(b'\n');
                if line.no_newline {
                    body.extend_from_slice(b"\\ No newline at end of file\n");
                }
            }
        }
        if !any_change {
            return None;
        }
        let mut patch = Vec::new();
        for line in &self.header {
            let rewritten = self.header_line(line, every_change_selected, mode);
            if let Some(line) = rewritten {
                patch.extend_from_slice(&line);
                patch.push(b'\n');
            }
        }
        patch.extend_from_slice(&body);
        Some(patch)
    }

    fn header_line(&self, line: &[u8], whole: bool, mode: PatchMode) -> Option<Vec<u8>> {
        if whole {
            return Some(line.to_vec());
        }
        let new_path = self
            .header
            .iter()
            .find_map(|l| l.strip_prefix(b"+++ "))
            .filter(|p| *p != b"/dev/null");
        let old_path = self
            .header
            .iter()
            .find_map(|l| l.strip_prefix(b"--- "))
            .filter(|p| *p != b"/dev/null");
        if line.starts_with(b"deleted file mode") {
            return None;
        }
        if line == b"+++ /dev/null" {
            // A deletion only partly taken: the file stays, with fewer lines.
            let old = old_path?;
            let mut out = b"+++ b/".to_vec();
            out.extend_from_slice(old.strip_prefix(b"a/").unwrap_or(old));
            return Some(out);
        }
        if mode == PatchMode::Discard {
            if line.starts_with(b"new file mode") {
                return None;
            }
            if line == b"--- /dev/null" {
                let new = new_path?;
                let mut out = b"--- a/".to_vec();
                out.extend_from_slice(new.strip_prefix(b"b/").unwrap_or(new));
                return Some(out);
            }
        }
        Some(line.to_vec())
    }
}

/// The line a hunk comes after. git writes a side's start as that line
/// when the side is empty, and as the line after it otherwise.
fn anchor(start: u32, count: u32) -> u32 {
    if count == 0 {
        start
    } else {
        start.saturating_sub(1)
    }
}

fn start_from(anchor: u32, count: u32) -> u32 {
    if count == 0 { anchor } else { anchor + 1 }
}

type HunkHeader = (u32, u32, u32, u32, Vec<u8>);

fn hunk_header(line: &[u8]) -> Result<HunkHeader, ParseError> {
    let bad = || ParseError::Malformed("hunk header is not `@@ -a,b +c,d @@`".to_string());
    let rest = line.strip_prefix(b"@@ -").ok_or_else(bad)?;
    let end = rest.windows(3).position(|w| w == b" @@").ok_or_else(bad)?;
    let ranges = std::str::from_utf8(&rest[..end]).map_err(|_| bad())?;
    let section = rest[end + 3..].to_vec();
    let (old, new) = ranges.split_once(" +").ok_or_else(bad)?;
    let range = |text: &str| -> Result<(u32, u32), ParseError> {
        match text.split_once(',') {
            Some((start, count)) => Ok((
                start.parse().map_err(|_| bad())?,
                count.parse().map_err(|_| bad())?,
            )),
            None => Ok((text.parse().map_err(|_| bad())?, 1)),
        }
    };
    let (old_start, old_count) = range(old)?;
    let (new_start, new_count) = range(new)?;
    Ok((old_start, old_count, new_start, new_count, section))
}
