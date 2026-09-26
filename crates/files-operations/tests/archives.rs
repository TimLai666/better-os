//! Archive and extract, run as real jobs against real temporary directories.
//!
//! Ticket 56's acceptance list, in order: the four formats both ways, the new
//! folder named after the archive, every escaping entry refused by name, the
//! size and entry-count limits, and both operations behaving like a copy under
//! pause, cancel, and a job store. The hostile archives are assembled here,
//! byte by byte where a well-behaved writer would refuse to produce them.

mod support;

use std::ffi::OsStr;
use std::fs;
use std::io::{Cursor, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use files_operations::policy::{ExtractLimits, MAX_EXTRACTED_BYTES, MAX_EXTRACTED_ENTRIES};
use files_operations::{
    ArchiveFormat, ArchiveLimit, ConflictKind, ConflictPolicy, CopyPolicy, EngineConfig, JobEngine,
    JobSpec, JobState, JobStore, Operation, OperationError, OperationKind, Resolution,
};

use support::{engine, entries, local, run, run_ok, wait_for, write_pattern};

const BIG: usize = 8 * 1024 * 1024;

fn slow_policy() -> CopyPolicy {
    CopyPolicy {
        chunk_bytes: 4096,
        ..CopyPolicy::default()
    }
}

/// A small tree with every kind of entry an archive has to carry.
fn sample_tree(root: &Path, with_non_utf8: bool) -> PathBuf {
    let tree = root.join("photos");
    fs::create_dir_all(tree.join("2024/summer")).unwrap();
    fs::create_dir(tree.join("empty")).unwrap();
    fs::write(tree.join("2024/summer/beach.jpg"), b"jpeg bytes").unwrap();
    write_pattern(&tree.join("2024/raw.bin"), 300_000);
    fs::write(tree.join("empty.txt"), b"").unwrap();
    let script = tree.join("run.sh");
    fs::write(&script, b"#!/bin/sh\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("2024/summer/beach.jpg", tree.join("latest")).unwrap();
    if with_non_utf8 {
        fs::write(tree.join(OsStr::from_bytes(b"caf\xe9.txt")), b"latin-1").unwrap();
    }
    tree
}

fn assert_same_tree(original: &Path, extracted: &Path, with_non_utf8: bool) {
    assert_eq!(
        fs::read(extracted.join("2024/summer/beach.jpg")).unwrap(),
        b"jpeg bytes"
    );
    assert_eq!(
        fs::read(extracted.join("2024/raw.bin")).unwrap(),
        fs::read(original.join("2024/raw.bin")).unwrap()
    );
    assert_eq!(fs::read(extracted.join("empty.txt")).unwrap(), b"");
    assert!(extracted.join("empty").is_dir());
    let mode = fs::metadata(extracted.join("run.sh")).unwrap().mode();
    assert_eq!(mode & 0o777, 0o755, "the executable bit survived");
    assert_eq!(
        fs::read_link(extracted.join("latest")).unwrap(),
        PathBuf::from("2024/summer/beach.jpg"),
        "a symbolic link comes back as a link"
    );
    if with_non_utf8 {
        assert_eq!(
            fs::read(extracted.join(OsStr::from_bytes(b"caf\xe9.txt"))).unwrap(),
            b"latin-1"
        );
    }
}

fn archive_spec(sources: &[&Path], destination: &Path, format: ArchiveFormat) -> JobSpec {
    JobSpec::new(Operation::Archive {
        sources: sources.iter().map(local).collect(),
        destination: local(destination),
        format,
    })
}

fn extract_spec(archive: &Path, destination: &Path) -> JobSpec {
    JobSpec::new(Operation::Extract {
        archives: vec![local(archive)],
        destination: local(destination),
    })
}

/// Everything in a directory, recursively, so a leftover temporary anywhere is
/// visible.
fn everything_under(path: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).unwrap().flatten() {
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                stack.push(path.clone());
            }
            found.push(path);
        }
    }
    found.sort();
    found
}

// --- The four formats -----------------------------------------------------

#[test]
fn every_format_archives_a_tree_and_extracts_it_back_into_a_folder_named_after_it() {
    for format in ArchiveFormat::ALL {
        let root = tempfile::tempdir().unwrap();
        // Zip names are UTF-8; a name that is not is refused, tested below.
        let with_non_utf8 = format != ArchiveFormat::Zip;
        let tree = sample_tree(root.path(), with_non_utf8);
        let archive = root.path().join(format!("photos.{}", format.extension()));
        let engine = engine();

        let made = run_ok(&engine, archive_spec(&[&tree], &archive, format));
        assert_eq!(made.kind, OperationKind::Archive);
        assert!(archive.is_file(), "{format:?} produced its archive");
        assert!(made.progress.bytes_total >= 300_000);
        assert_eq!(made.progress.bytes_done, made.progress.bytes_total);

        let out = root.path().join("out");
        fs::create_dir(&out).unwrap();
        let unpacked = run_ok(&engine, extract_spec(&archive, &out));
        assert_eq!(unpacked.kind, OperationKind::Extract);
        assert_eq!(
            unpacked.progress.bytes_total,
            fs::metadata(&archive).unwrap().len()
        );
        // The archive's own name minus its extension, and inside it the tree
        // exactly as it was archived: one top-level `photos`.
        let folder = out.join("photos");
        assert_eq!(entries(&out), vec![folder.clone()], "{format:?}");
        assert_same_tree(&tree, &folder.join("photos"), with_non_utf8);
    }
}

#[test]
fn a_tar_keeps_modification_times_and_a_zip_keeps_them_to_two_seconds() {
    for format in ArchiveFormat::ALL {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("dated.txt");
        fs::write(&file, b"old").unwrap();
        // 2021-03-04 05:06:08 UTC, an even second so the zip's two-second
        // resolution can hold it.
        let stamp = std::time::UNIX_EPOCH + Duration::from_secs(1_614_834_368);
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(stamp)
            .unwrap();
        let archive = root.path().join(format!("dated.{}", format.extension()));
        let engine = engine();
        run_ok(&engine, archive_spec(&[&file], &archive, format));
        let out = root.path().join("out");
        fs::create_dir(&out).unwrap();
        run_ok(&engine, extract_spec(&archive, &out));
        let modified = fs::metadata(out.join("dated/dated.txt"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(modified, stamp, "{format:?}");
    }
}

#[test]
fn several_sources_go_into_one_archive_side_by_side() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.txt");
    let b = root.path().join("b");
    fs::write(&a, b"alpha").unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(b.join("c.txt"), b"gamma").unwrap();
    let archive = root.path().join("both.tar.gz");
    let engine = engine();
    run_ok(
        &engine,
        archive_spec(&[&a, &b], &archive, ArchiveFormat::TarGz),
    );

    let out = root.path().join("out");
    fs::create_dir(&out).unwrap();
    run_ok(&engine, extract_spec(&archive, &out));
    assert_eq!(fs::read(out.join("both/a.txt")).unwrap(), b"alpha");
    assert_eq!(fs::read(out.join("both/b/c.txt")).unwrap(), b"gamma");
}

#[test]
fn a_zip_refuses_a_name_that_is_not_utf8_and_says_which() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join(OsStr::from_bytes(b"caf\xe9.txt")), b"x").unwrap();
    let archive = root.path().join("tree.zip");
    let snapshot = run(
        &engine(),
        archive_spec(&[&tree], &archive, ArchiveFormat::Zip),
    );
    assert_eq!(snapshot.state, JobState::Failed);
    match &snapshot.failures[0].1 {
        OperationError::ArchiveEntryRefused { entry, reason, .. } => {
            assert_eq!(entry.as_os_str().as_bytes(), b"tree/caf\xe9.txt");
            assert_eq!(reason, "files.archive.entry.name_not_utf8");
        }
        other => panic!("expected a refused entry, got {other:?}"),
    }
    assert_eq!(entries(root.path()), vec![tree], "no archive, no temporary");
}

#[test]
fn an_archive_that_already_exists_is_a_conflict_like_any_other_destination() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("notes.txt");
    fs::write(&file, b"new").unwrap();
    let archive = root.path().join("notes.zip");
    fs::write(&archive, b"somebody else's").unwrap();

    let engine = engine();
    let skipped = run(
        &engine,
        archive_spec(&[&file], &archive, ArchiveFormat::Zip).with_conflicts(
            ConflictPolicy::always(ConflictKind::Exists, Resolution::Skip),
        ),
    );
    assert_eq!(skipped.progress.items_skipped, 1);
    assert_eq!(fs::read(&archive).unwrap(), b"somebody else's");

    run_ok(
        &engine,
        archive_spec(&[&file], &archive, ArchiveFormat::Zip).with_conflicts(
            ConflictPolicy::always(ConflictKind::Exists, Resolution::Rename),
        ),
    );
    assert!(root.path().join("notes (copy).zip").is_file());
    assert_eq!(fs::read(&archive).unwrap(), b"somebody else's");
}

// --- The new folder --------------------------------------------------------

#[test]
fn each_extension_names_the_folder_without_itself() {
    let cases = [
        ("report.zip", "report"),
        ("report.tar", "report"),
        ("report.tar.gz", "report"),
        ("report.TGZ", "report"),
        ("report.tar.zst", "report"),
        ("report.tzst", "report"),
        ("v1.2.tar.gz", "v1.2"),
    ];
    for (name, folder) in cases {
        assert_eq!(
            ArchiveFormat::folder_name_for(OsStr::new(name)),
            OsStr::new(folder),
            "{name}"
        );
        assert!(ArchiveFormat::from_file_name(OsStr::new(name)).is_some());
    }
    assert!(ArchiveFormat::from_file_name(OsStr::new("report.txt")).is_none());
    assert!(ArchiveFormat::from_file_name(OsStr::new("report.gz")).is_none());
}

#[test]
fn an_existing_folder_of_that_name_is_a_conflict_answered_by_the_policy() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("notes.txt");
    fs::write(&file, b"n").unwrap();
    let archive = root.path().join("notes.tar");
    let engine = engine();
    run_ok(
        &engine,
        archive_spec(&[&file], &archive, ArchiveFormat::Tar),
    );
    let taken = root.path().join("notes");
    fs::create_dir(&taken).unwrap();
    fs::write(taken.join("keep.txt"), b"mine").unwrap();

    let skipped = run(
        &engine,
        extract_spec(&archive, root.path()).with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Skip,
        )),
    );
    assert_eq!(skipped.progress.items_skipped, 1);
    assert_eq!(entries(&taken), vec![taken.join("keep.txt")]);

    run_ok(
        &engine,
        extract_spec(&archive, root.path()).with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Rename,
        )),
    );
    assert_eq!(
        fs::read(root.path().join("notes (copy)/notes.txt")).unwrap(),
        b"n"
    );
    assert_eq!(entries(&taken), vec![taken.join("keep.txt")], "untouched");
}

#[test]
fn a_file_that_is_not_an_archive_is_refused_at_submission() {
    let root = tempfile::tempdir().unwrap();
    let text = root.path().join("notes.txt");
    fs::write(&text, b"n").unwrap();
    let refused = engine()
        .submit(extract_spec(&text, root.path()))
        .unwrap_err();
    assert_eq!(refused.key(), "files.operation.error.archive_unreadable");
}

#[test]
fn a_damaged_archive_fails_with_a_named_error_and_leaves_nothing() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("broken.tar.gz");
    fs::write(&archive, b"\x1f\x8b\x08\x00 this is not a gzip stream").unwrap();
    let snapshot = run(&engine(), extract_spec(&archive, root.path()));
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failures[0].1.key(),
        "files.operation.error.archive_unreadable"
    );
    assert_eq!(entries(root.path()), vec![archive]);
}

// --- Hostile entries -------------------------------------------------------

/// One raw tar entry. The name and the link target are written into the header
/// as bytes, because `tar::Builder` refuses to write `..` itself.
struct Raw<'a> {
    name: &'a [u8],
    kind: tar::EntryType,
    link: &'a [u8],
    data: &'a [u8],
}

fn file(name: &[u8]) -> Raw<'_> {
    Raw {
        name,
        kind: tar::EntryType::Regular,
        link: b"",
        data: b"payload",
    }
}

fn symlink<'a>(name: &'a [u8], target: &'a [u8]) -> Raw<'a> {
    Raw {
        name,
        kind: tar::EntryType::Symlink,
        link: target,
        data: b"",
    }
}

fn hard_link<'a>(name: &'a [u8], target: &'a [u8]) -> Raw<'a> {
    Raw {
        name,
        kind: tar::EntryType::Link,
        link: target,
        data: b"",
    }
}

fn raw_tar(path: &Path, entries: &[Raw<'_>]) {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        {
            let old = header.as_old_mut();
            old.name[..entry.name.len()].copy_from_slice(entry.name);
            old.linkname[..entry.link.len()].copy_from_slice(entry.link);
        }
        header.set_entry_type(entry.kind);
        header.set_size(entry.data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(1_600_000_000);
        header.set_cksum();
        builder.append(&header, entry.data).unwrap();
    }
    fs::write(path, builder.into_inner().unwrap()).unwrap();
}

/// Extracts `archive` into `root/out`, asserts it was refused naming `entry`
/// with `reason`, and asserts nothing was left anywhere under `root` except
/// the archive and the empty `out`.
fn assert_refused(root: &Path, archive: &Path, entry: &[u8], reason: &str) {
    let out = root.join("out");
    fs::create_dir_all(&out).unwrap();
    let before = everything_under(root);
    let snapshot = run(&engine(), extract_spec(archive, &out));
    assert_eq!(snapshot.state, JobState::Failed, "{:?}", snapshot.failures);
    match &snapshot.failures[0].1 {
        OperationError::ArchiveEntryRefused {
            path,
            entry: named,
            reason: why,
        } => {
            assert_eq!(path, archive);
            assert_eq!(named.as_os_str().as_bytes(), entry);
            assert_eq!(why, reason);
        }
        other => panic!("expected {reason} for {entry:?}, got {other:?}"),
    }
    // The message the operation centre shows carries the entry's name.
    let shown = snapshot.failures[0].1.to_string();
    assert!(
        shown.contains(&*String::from_utf8_lossy(entry)),
        "{shown} names the entry"
    );
    assert_eq!(everything_under(root), before, "nothing was left behind");
}

#[test]
fn an_entry_with_parent_components_is_refused_by_name() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.tar");
    raw_tar(&archive, &[file(b"fine.txt"), file(b"sub/../../evil.txt")]);
    assert_refused(
        root.path(),
        &archive,
        b"sub/../../evil.txt",
        "files.archive.entry.parent_traversal",
    );
    assert!(!root.path().join("evil.txt").exists());
}

#[test]
fn an_absolute_entry_is_refused_by_name() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.tar");
    let target = root.path().join("absolute.txt");
    raw_tar(&archive, &[file(target.as_os_str().as_bytes())]);
    assert_refused(
        root.path(),
        &archive,
        target.as_os_str().as_bytes(),
        "files.archive.entry.absolute_path",
    );
    assert!(!target.exists());
}

#[test]
fn a_symlink_pointing_outside_is_refused_whether_absolute_or_relative() {
    for target in [&b"/etc/passwd"[..], b"../../outside", b"a/../../outside"] {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("evil.tar");
        raw_tar(&archive, &[symlink(b"link", target)]);
        assert_refused(
            root.path(),
            &archive,
            b"link",
            "files.archive.entry.link_outside",
        );
    }
}

#[test]
fn a_symlink_that_escapes_only_through_another_link_is_refused() {
    // `s` points at the root of the extraction, which is inside. `e` points at
    // `s/..`, which reads as the root too, but the kernel resolves `s` first
    // and lands one level above the extraction.
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.tar");
    raw_tar(&archive, &[symlink(b"s", b"."), symlink(b"e", b"s/..")]);
    assert_refused(
        root.path(),
        &archive,
        b"e",
        "files.archive.entry.link_outside",
    );
}

#[test]
fn a_symlink_through_a_name_a_later_entry_could_turn_into_a_link_is_refused() {
    // `e` -> `p/..` reads as the root while `p` does not exist. A later `p` ->
    // `.` would make it the parent of the root, so `..` after a name that is
    // not yet a real directory is not accepted.
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.tar");
    raw_tar(&archive, &[symlink(b"e", b"p/.."), symlink(b"p", b".")]);
    assert_refused(
        root.path(),
        &archive,
        b"e",
        "files.archive.entry.link_outside",
    );
}

#[test]
fn an_entry_written_through_a_symlink_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.tar");
    raw_tar(
        &archive,
        &[
            Raw {
                name: b"real",
                kind: tar::EntryType::Directory,
                link: b"",
                data: b"",
            },
            symlink(b"d", b"real"),
            file(b"d/file.txt"),
        ],
    );
    assert_refused(
        root.path(),
        &archive,
        b"d/file.txt",
        "files.archive.entry.through_link",
    );
}

#[test]
fn a_symlink_cannot_replace_an_entry_already_extracted() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.tar");
    raw_tar(&archive, &[file(b"x"), symlink(b"x", b"y")]);
    assert_refused(
        root.path(),
        &archive,
        b"x",
        "files.archive.entry.replaces_existing",
    );
}

#[test]
fn a_hard_link_to_anything_outside_is_refused() {
    for target in [&b"/etc/passwd"[..], b"../outside.txt"] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("outside.txt"), b"secret").unwrap();
        let archive = root.path().join("evil.tar");
        raw_tar(&archive, &[hard_link(b"h", target)]);
        assert_refused(
            root.path(),
            &archive,
            b"h",
            "files.archive.entry.hard_link_outside",
        );
    }
}

#[test]
fn a_hard_link_inside_the_archive_is_extracted_as_a_link() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("linked.tar");
    raw_tar(&archive, &[file(b"a.txt"), hard_link(b"b.txt", b"a.txt")]);
    run_ok(&engine(), extract_spec(&archive, root.path()));
    let a = fs::metadata(root.path().join("linked/a.txt")).unwrap();
    let b = fs::metadata(root.path().join("linked/b.txt")).unwrap();
    assert_eq!(a.ino(), b.ino());
    assert_eq!(
        fs::read(root.path().join("linked/b.txt")).unwrap(),
        b"payload"
    );
}

fn raw_zip(path: &Path, build: impl FnOnce(&mut zip::ZipWriter<Cursor<Vec<u8>>>)) {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    build(&mut writer);
    fs::write(path, writer.finish().unwrap().into_inner()).unwrap();
}

#[test]
fn a_zip_entry_that_escapes_is_refused_by_name() {
    let options = zip::write::SimpleFileOptions::default();
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.zip");
    raw_zip(&archive, |writer| {
        writer.start_file("../evil.txt", options).unwrap();
        writer.write_all(b"x").unwrap();
    });
    assert_refused(
        root.path(),
        &archive,
        b"../evil.txt",
        "files.archive.entry.parent_traversal",
    );

    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("evil.zip");
    raw_zip(&archive, |writer| {
        writer.add_symlink("link", "/etc/passwd", options).unwrap();
    });
    assert_refused(
        root.path(),
        &archive,
        b"link",
        "files.archive.entry.link_outside",
    );
}

// --- Limits ----------------------------------------------------------------

fn limited(max_bytes: u64, max_entries: u64) -> CopyPolicy {
    CopyPolicy {
        extract_limits: ExtractLimits {
            max_bytes,
            max_entries,
        },
        ..CopyPolicy::default()
    }
}

#[test]
fn the_default_limits_are_the_documented_constants() {
    let limits = CopyPolicy::default().extract_limits;
    assert_eq!(limits.max_bytes, MAX_EXTRACTED_BYTES);
    assert_eq!(limits.max_entries, MAX_EXTRACTED_ENTRIES);
}

#[test]
fn an_archive_over_the_entry_limit_stops_and_leaves_nothing() {
    for format in ArchiveFormat::ALL {
        let root = tempfile::tempdir().unwrap();
        let tree = root.path().join("many");
        fs::create_dir(&tree).unwrap();
        for index in 0..5 {
            fs::write(tree.join(format!("{index}.txt")), b"x").unwrap();
        }
        let archive = root.path().join(format!("many.{}", format.extension()));
        let engine = engine();
        run_ok(&engine, archive_spec(&[&tree], &archive, format));
        let out = root.path().join("out");
        fs::create_dir(&out).unwrap();

        let snapshot = run(
            &engine,
            extract_spec(&archive, &out).with_policy(limited(1 << 30, 3)),
        );
        assert_eq!(snapshot.state, JobState::Failed, "{format:?}");
        assert_eq!(
            snapshot.failures[0].1,
            OperationError::ArchiveLimitExceeded {
                path: archive.clone(),
                limit: ArchiveLimit::Entries,
                maximum: 3,
            },
            "{format:?}"
        );
        assert!(entries(&out).is_empty(), "{format:?}: {:?}", entries(&out));
    }
}

#[test]
fn an_archive_over_the_size_limit_stops_and_leaves_nothing() {
    for format in ArchiveFormat::ALL {
        let root = tempfile::tempdir().unwrap();
        let big = root.path().join("big.bin");
        // Zeroes compress to almost nothing, which is what a bomb looks like.
        fs::write(&big, vec![0u8; 2 * 1024 * 1024]).unwrap();
        let archive = root.path().join(format!("big.{}", format.extension()));
        let engine = engine();
        run_ok(&engine, archive_spec(&[&big], &archive, format));
        let out = root.path().join("out");
        fs::create_dir(&out).unwrap();

        let snapshot = run(
            &engine,
            extract_spec(&archive, &out).with_policy(limited(1024 * 1024, 100)),
        );
        assert_eq!(snapshot.state, JobState::Failed, "{format:?}");
        assert_eq!(
            snapshot.failures[0].1,
            OperationError::ArchiveLimitExceeded {
                path: archive.clone(),
                limit: ArchiveLimit::UnpackedBytes,
                maximum: 1024 * 1024,
            },
            "{format:?}"
        );
        assert!(entries(&out).is_empty(), "{format:?}: {:?}", entries(&out));
    }
}

#[test]
fn many_small_entries_that_add_up_past_the_size_limit_are_stopped_too() {
    // A tar entry's size is the header's, so the running count is what stops
    // a stream of many small entries that add up past the limit.
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("sum.tar");
    let names: Vec<Vec<u8>> = (0..4).map(|n| format!("{n}.txt").into_bytes()).collect();
    let raws: Vec<Raw<'_>> = names.iter().map(|name| file(name)).collect();
    raw_tar(&archive, &raws);
    let out = root.path().join("out");
    fs::create_dir(&out).unwrap();
    // Four entries of seven bytes: 28 bytes against a limit of 20.
    let snapshot = run(
        &engine(),
        extract_spec(&archive, &out).with_policy(limited(20, 100)),
    );
    assert_eq!(
        snapshot.failures[0].1.key(),
        "files.operation.error.archive_limit_exceeded"
    );
    assert!(entries(&out).is_empty());
}

// --- Jobs -------------------------------------------------------------------

#[test]
fn archiving_pauses_between_chunks_resumes_and_cancels_leaving_nothing() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("big.bin");
    write_pattern(&source, BIG);
    let archive = root.path().join("big.tar.zst");
    let engine = engine();

    let handle = engine
        .submit(
            archive_spec(&[&source], &archive, ArchiveFormat::TarZst).with_policy(slow_policy()),
        )
        .unwrap();
    let id = handle.id();
    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    assert!(engine.pause(id));
    let paused = wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
    assert!(paused.progress.bytes_done < BIG as u64);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        engine.snapshot(id).unwrap().progress.bytes_done,
        paused.progress.bytes_done,
        "it stays where it stopped"
    );
    assert!(!archive.exists(), "the archive is still a temporary");
    assert!(engine.resume(id));
    let finished = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(finished.state, JobState::Completed);
    assert!(archive.is_file());

    // And a second one, cancelled halfway, leaves no archive and no temporary.
    let second = root.path().join("second.zip");
    let handle = engine
        .submit(archive_spec(&[&source], &second, ArchiveFormat::Zip).with_policy(slow_policy()))
        .unwrap();
    let id = handle.id();
    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    assert!(engine.cancel(id));
    let cancelled = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert_eq!(entries(root.path()), vec![source, archive]);
}

#[test]
fn extracting_pauses_resumes_and_cancels_leaving_no_folder() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("big.bin");
    write_pattern(&source, BIG);
    let archive = root.path().join("big.tar");
    let engine = engine();
    run_ok(
        &engine,
        archive_spec(&[&source], &archive, ArchiveFormat::Tar),
    );
    let out = root.path().join("out");
    fs::create_dir(&out).unwrap();

    let handle = engine
        .submit(extract_spec(&archive, &out).with_policy(slow_policy()))
        .unwrap();
    let id = handle.id();
    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    assert!(engine.pause(id));
    let paused = wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
    assert!(paused.progress.bytes_done < paused.progress.bytes_total);
    assert!(
        !out.join("big").exists(),
        "the folder appears only once it is complete"
    );
    assert!(engine.cancel(id));
    let cancelled = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(
        everything_under(&out).is_empty(),
        "{:?}",
        everything_under(&out)
    );

    let handle = engine
        .submit(extract_spec(&archive, &out).with_policy(slow_policy()))
        .unwrap();
    let id = handle.id();
    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    assert!(engine.pause(id));
    wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
    assert!(engine.resume(id));
    let finished = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(finished.state, JobState::Completed);
    assert_eq!(
        fs::read(out.join("big/big.bin")).unwrap(),
        fs::read(&source).unwrap()
    );
}

#[test]
fn a_failed_archive_can_be_retried_once_the_cause_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join(OsStr::from_bytes(b"caf\xe9.txt")), b"x").unwrap();
    let archive = root.path().join("tree.zip");
    let engine = engine();
    let handle = engine
        .submit(archive_spec(&[&tree], &archive, ArchiveFormat::Zip))
        .unwrap();
    let failed = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(failed.state, JobState::Failed);

    fs::rename(
        tree.join(OsStr::from_bytes(b"caf\xe9.txt")),
        tree.join("cafe.txt"),
    )
    .unwrap();
    assert!(engine.retry_failed(handle.id()));
    let retried = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(retried.state, JobState::Completed, "{:?}", retried.failures);
    assert!(archive.is_file());
}

#[test]
fn both_operations_leave_a_record_a_later_process_can_read() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let file = data.join("notes.txt");
    fs::write(&file, b"n").unwrap();
    let archive = data.join("notes.tar.gz");
    {
        let engine = JobEngine::new(EngineConfig {
            workers: 1,
            store: Some(store.clone()),
            conflicts: ConflictPolicy::new(),
        });
        run_ok(
            &engine,
            archive_spec(&[&file], &archive, ArchiveFormat::TarGz),
        );
        run_ok(&engine, extract_spec(&archive, &data));
    }
    let recovery = store.recover();
    assert!(recovery.interrupted.is_empty());
    let kinds: Vec<OperationKind> = recovery.settled.iter().map(|record| record.kind).collect();
    assert_eq!(kinds, vec![OperationKind::Archive, OperationKind::Extract]);
    for record in &recovery.settled {
        assert_eq!(record.state, JobState::Completed);
        assert_eq!(record.items.len(), 1);
    }
    assert_eq!(fs::read(data.join("notes/notes.txt")).unwrap(), b"n");
}
