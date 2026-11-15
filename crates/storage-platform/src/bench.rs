//! The real-device benchmark: what a copy to one external volume costs, and
//! how long it takes until the readiness rule's own signals could say "Ready
//! to unplug".
//!
//! Driven by `better-storage-bench` (see `src/bin/bench.rs`) against a mount
//! the person names. It writes only inside a fresh directory it creates on
//! that mount and removes it afterwards; it never mounts, unmounts, formats,
//! or opens a block device. The method and the numbers live in
//! `engineering/performance/storage-performance.md`.
//!
//! Every measurement here is application-visible wall time on one machine. A
//! record is only a number for the device, filesystem, and kernel it names.

use crate::flush::LinuxFlush;
use crate::openuse::ProcOpenUse;
use crate::roots::Roots;
use crate::traits::{FlushBackend, FlushReport, OpenUseInspector};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use storage_core::{ScanCoverage, SignalStatus};

/// The three workloads ticket 31 names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Workload {
    /// One large file written sequentially.
    LargeSequential,
    /// Many small files in one directory.
    ManySmall,
    /// A deep tree of directories with a tiny file in each.
    MetadataTree,
}

impl Workload {
    pub const ALL: [Workload; 3] = [
        Workload::LargeSequential,
        Workload::ManySmall,
        Workload::MetadataTree,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Workload::LargeSequential => "large-sequential",
            Workload::ManySmall => "many-small",
            Workload::MetadataTree => "metadata-tree",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|workload| workload.name() == value)
    }
}

/// How big each workload is. The defaults are the recorded method; a smaller
/// shape exists for tests and for a quick look at a slow stick.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub large_file_bytes: u64,
    pub small_files: u32,
    pub small_file_bytes: u64,
    pub tree_depth: u32,
    pub tree_fanout: u32,
    pub tree_file_bytes: u64,
}

impl Shape {
    /// 1 GiB; 2,000 × 16 KiB; depth 4 × fan-out 6 (1,554 directories) with a
    /// 1 KiB file in each.
    pub const RECORDED: Shape = Shape {
        large_file_bytes: 1 << 30,
        small_files: 2_000,
        small_file_bytes: 16 << 10,
        tree_depth: 4,
        tree_fanout: 6,
        tree_file_bytes: 1 << 10,
    };

    pub const TINY: Shape = Shape {
        large_file_bytes: 256 << 10,
        small_files: 20,
        small_file_bytes: 1 << 10,
        tree_depth: 2,
        tree_fanout: 3,
        tree_file_bytes: 64,
    };

    /// Files and bytes a workload writes, so a record can be checked against
    /// what landed on disk.
    pub fn expected(self, workload: Workload) -> (u64, u64) {
        match workload {
            Workload::LargeSequential => (1, self.large_file_bytes),
            Workload::ManySmall => (
                u64::from(self.small_files),
                u64::from(self.small_files) * self.small_file_bytes,
            ),
            Workload::MetadataTree => {
                let directories: u64 = (1..=self.tree_depth)
                    .map(|level| u64::from(self.tree_fanout).pow(level))
                    .sum();
                (directories, directories * self.tree_file_bytes)
            }
        }
    }
}

/// What the person says about the device. The harness cannot tell a flash
/// drive from an SSD in a USB enclosure, so it records what it was told next
/// to what the kernel reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Medium {
    UsbFlash,
    ExternalSsd,
    ExternalHdd,
}

impl Medium {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "usb-flash" => Some(Medium::UsbFlash),
            "external-ssd" => Some(Medium::ExternalSsd),
            "external-hdd" => Some(Medium::ExternalHdd),
            _ => None,
        }
    }
}

/// The mount a run targets, as `/proc/self/mountinfo` describes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MountFacts {
    pub mount_point: PathBuf,
    pub filesystem: String,
    pub source: String,
}

/// One run, one workload: the line appended to the record file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub format: u32,
    pub workload: Workload,
    pub medium: Medium,
    pub mount: MountFacts,
    /// `/sys/block/<disk>/queue/rotational`, when the source names a disk.
    pub rotational: Option<bool>,
    pub kernel: String,
    pub files: u64,
    pub bytes: u64,
    /// Every write returned. What a file manager would call "done", and what
    /// default desktop behaviour stops at until Eject.
    pub copy_ms: f64,
    /// `syncfs(2)` on the mount, issued once after the copy as the service
    /// does after an operation completes.
    pub flush_ms: f64,
    /// The open-writer scan the readiness rule needs.
    pub writer_scan_ms: f64,
    /// Whether that scan saw every process. A partial scan is the normal
    /// unprivileged case (ADR 0026).
    pub writer_scan_complete: bool,
    /// Copy + flush + scan: the earliest the service could report ready.
    pub ready_ms: f64,
    pub app_visible_mib_per_s: f64,
    pub durable_mib_per_s: f64,
    /// User + system CPU this process spent across the run.
    pub cpu_ms: f64,
}

pub const RECORD_FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    #[error("{0}")]
    Refused(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    #[error("the flush did not complete: {0}")]
    Flush(String),
}

fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> BenchError {
    let context = context.into();
    move |source| BenchError::Io { context, source }
}

/// Finds the mountinfo line whose mount point is exactly `mount_point`.
/// Mountinfo escapes spaces and the like as octal; this undoes `\040` and
/// friends so a label with a space still matches.
pub fn find_mount(mountinfo: &str, mount_point: &Path) -> Option<MountFacts> {
    let unescape = |field: &str| {
        let bytes = field.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'\\'
                && index + 3 < bytes.len()
                && bytes[index + 1..index + 4].iter().all(u8::is_ascii_digit)
            {
                let octal = std::str::from_utf8(&bytes[index + 1..index + 4]).ok();
                if let Some(value) = octal.and_then(|o| u8::from_str_radix(o, 8).ok()) {
                    out.push(value);
                    index += 4;
                    continue;
                }
            }
            out.push(bytes[index]);
            index += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    mountinfo.lines().rev().find_map(|line| {
        let (left, right) = line.split_once(" - ")?;
        let point = unescape(left.split(' ').nth(4)?);
        if Path::new(&point) != mount_point {
            return None;
        }
        let mut right = right.split(' ');
        let filesystem = right.next()?.to_string();
        let source = unescape(right.next()?);
        Some(MountFacts {
            mount_point: mount_point.to_path_buf(),
            filesystem,
            source,
        })
    })
}

/// Refuses anything that is not a separate mount, and the root filesystem.
/// The harness writes a gigabyte; it must never be pointed at the system disk
/// by a slip of the path.
pub fn check_target(mount_point: &Path, mountinfo: &str) -> Result<MountFacts, BenchError> {
    let canonical = mount_point
        .canonicalize()
        .map_err(io(format!("{} cannot be resolved", mount_point.display())))?;
    if canonical == Path::new("/") {
        return Err(BenchError::Refused(
            "refusing to benchmark the root filesystem".to_string(),
        ));
    }
    let here = fs::metadata(&canonical).map_err(io("stat of the target"))?;
    let root = fs::metadata("/").map_err(io("stat of /"))?;
    if here.dev() == root.dev() {
        return Err(BenchError::Refused(format!(
            "{} is on the same filesystem as /; point at the external volume's mount",
            canonical.display()
        )));
    }
    find_mount(mountinfo, &canonical).ok_or_else(|| {
        BenchError::Refused(format!(
            "{} is not itself a mount point; pass the volume's mount, such as /media/$USER/LABEL",
            canonical.display()
        ))
    })
}

/// Writes one workload into `directory`, returning files and bytes written.
fn write_workload(
    directory: &Path,
    workload: Workload,
    shape: Shape,
) -> Result<(u64, u64), BenchError> {
    // Not all zeros, so a compressing filesystem or bridge cannot shortcut it.
    let pattern: Vec<u8> = (0..(1 << 20))
        .map(|i: u32| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let write_file = |path: &Path, mut remaining: u64| -> Result<u64, BenchError> {
        let mut file = fs::File::create(path).map_err(io(format!("create {}", path.display())))?;
        let total = remaining;
        while remaining > 0 {
            let chunk = remaining.min(pattern.len() as u64) as usize;
            file.write_all(&pattern[..chunk])
                .map_err(io(format!("write {}", path.display())))?;
            remaining -= chunk as u64;
        }
        Ok(total)
    };
    let mut files = 0;
    let mut bytes = 0;
    match workload {
        Workload::LargeSequential => {
            bytes += write_file(&directory.join("large.bin"), shape.large_file_bytes)?;
            files += 1;
        }
        Workload::ManySmall => {
            for index in 0..shape.small_files {
                bytes += write_file(
                    &directory.join(format!("small-{index:05}.bin")),
                    shape.small_file_bytes,
                )?;
                files += 1;
            }
        }
        Workload::MetadataTree => {
            let mut level = vec![directory.to_path_buf()];
            for _ in 0..shape.tree_depth {
                let mut next = Vec::new();
                for parent in &level {
                    for branch in 0..shape.tree_fanout {
                        let child = parent.join(format!("d{branch}"));
                        fs::create_dir(&child).map_err(io(format!("mkdir {}", child.display())))?;
                        bytes += write_file(&child.join("f.txt"), shape.tree_file_bytes)?;
                        files += 1;
                        next.push(child);
                    }
                }
                level = next;
            }
        }
    }
    Ok((files, bytes))
}

fn cpu_time() -> Duration {
    // SAFETY: getrusage writes into the zeroed struct it is handed.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let tv = |t: libc::timeval| Duration::new(t.tv_sec as u64, (t.tv_usec as u32) * 1000);
    tv(usage.ru_utime) + tv(usage.ru_stime)
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn mib_per_s(bytes: u64, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds == 0.0 {
        0.0
    } else {
        bytes as f64 / (1 << 20) as f64 / seconds
    }
}

/// The context a run records alongside its timings.
pub struct RunContext<'a> {
    pub mount: &'a MountFacts,
    pub medium: Medium,
    pub rotational: Option<bool>,
    pub kernel: &'a str,
    pub roots: Roots,
}

/// Runs one workload inside a fresh directory under the mount, then removes
/// it and flushes the removal so the next run starts from a clean device.
pub fn run(
    context: &RunContext<'_>,
    workload: Workload,
    shape: Shape,
) -> Result<RunRecord, BenchError> {
    let mount_point = &context.mount.mount_point;
    let scratch = mount_point.join(format!(
        ".better-storage-bench-{}-{}",
        std::process::id(),
        workload.name()
    ));
    if scratch.exists() {
        return Err(BenchError::Refused(format!(
            "{} already exists",
            scratch.display()
        )));
    }
    fs::create_dir(&scratch).map_err(io(format!("mkdir {}", scratch.display())))?;
    let flush = LinuxFlush;
    // The directory itself is a write; start from a flushed device.
    let flushed = |report: FlushReport| match report {
        FlushReport::Completed { .. } => Ok(()),
        FlushReport::Unsupported { detail } | FlushReport::Failed { detail } => {
            Err(BenchError::Flush(detail))
        }
    };
    let outcome = (|| {
        flushed(flush.flush_filesystem(mount_point))?;
        let cpu_start = cpu_time();
        let start = Instant::now();
        let (files, bytes) = write_workload(&scratch, workload, shape)?;
        let copy = start.elapsed();
        let flush_start = Instant::now();
        flushed(flush.flush_filesystem(mount_point))?;
        let flush_time = flush_start.elapsed();
        let scan_start = Instant::now();
        let scan = ProcOpenUse::new(context.roots.clone()).open_writers(mount_point);
        let scan_time = scan_start.elapsed();
        let cpu = cpu_time().saturating_sub(cpu_start);
        let writer_scan_complete = match &scan {
            SignalStatus::Observed(writers) => matches!(writers.coverage, ScanCoverage::Complete),
            _ => false,
        };
        Ok(RunRecord {
            format: RECORD_FORMAT,
            workload,
            medium: context.medium,
            mount: context.mount.clone(),
            rotational: context.rotational,
            kernel: context.kernel.to_string(),
            files,
            bytes,
            copy_ms: millis(copy),
            flush_ms: millis(flush_time),
            writer_scan_ms: millis(scan_time),
            writer_scan_complete,
            ready_ms: millis(copy + flush_time + scan_time),
            app_visible_mib_per_s: mib_per_s(bytes, copy),
            durable_mib_per_s: mib_per_s(bytes, copy + flush_time),
            cpu_ms: millis(cpu),
        })
    })();
    let cleanup = fs::remove_dir_all(&scratch).map_err(io(format!("remove {}", scratch.display())));
    let _ = flush.flush_filesystem(mount_point);
    let record = outcome?;
    cleanup?;
    Ok(record)
}

/// One Markdown table row, in the column order of the performance record.
pub fn markdown_row(record: &RunRecord) -> String {
    format!(
        "| {} | {:?} | {} | {} | {:.0} | {:.0} | {:.0} | {:.1} | {:.1} | {:.0} | {} |",
        record.workload.name(),
        record.medium,
        record.mount.filesystem,
        record.kernel,
        record.copy_ms,
        record.flush_ms,
        record.ready_ms,
        record.app_visible_mib_per_s,
        record.durable_mib_per_s,
        record.cpu_ms,
        if record.writer_scan_complete {
            "complete"
        } else {
            "partial"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
98 22 8:17 / /media/tim/MY\\040STICK rw,nosuid,nodev,relatime shared:50 - exfat /dev/sdb1 rw,uid=1000
";

    #[test]
    fn a_mount_with_a_space_in_its_label_is_found_and_described() {
        let facts = find_mount(MOUNTINFO, Path::new("/media/tim/MY STICK")).unwrap();
        assert_eq!(facts.filesystem, "exfat");
        assert_eq!(facts.source, "/dev/sdb1");
        assert_eq!(find_mount(MOUNTINFO, Path::new("/media/tim")), None);
    }

    #[test]
    fn the_root_filesystem_and_a_plain_directory_are_refused() {
        assert!(matches!(
            check_target(Path::new("/"), MOUNTINFO),
            Err(BenchError::Refused(_))
        ));
        let directory = tempfile::tempdir().unwrap();
        // A temporary directory is either on / or on a tmpfs that is not
        // itself the directory's mount; both must be refused.
        assert!(matches!(
            check_target(directory.path(), MOUNTINFO),
            Err(BenchError::Refused(_))
        ));
    }

    #[test]
    fn the_recorded_shape_writes_what_the_method_says() {
        assert_eq!(
            Shape::RECORDED.expected(Workload::LargeSequential),
            (1, 1 << 30)
        );
        assert_eq!(
            Shape::RECORDED.expected(Workload::ManySmall),
            (2_000, 2_000 * (16 << 10))
        );
        assert_eq!(
            Shape::RECORDED.expected(Workload::MetadataTree).0,
            6 + 36 + 216 + 1296
        );
    }

    #[test]
    fn every_workload_runs_writes_its_shape_and_leaves_nothing_behind() {
        let volume = tempfile::tempdir().unwrap();
        let mount = MountFacts {
            mount_point: volume.path().to_path_buf(),
            filesystem: "test".to_string(),
            source: "none".to_string(),
        };
        let context = RunContext {
            mount: &mount,
            medium: Medium::UsbFlash,
            rotational: None,
            kernel: "test",
            roots: Roots::system(),
        };
        for workload in Workload::ALL {
            let record = run(&context, workload, Shape::TINY).unwrap();
            assert_eq!((record.files, record.bytes), Shape::TINY.expected(workload));
            assert!(record.ready_ms >= record.copy_ms + record.flush_ms);
            let line = serde_json::to_string(&record).unwrap();
            // Read back by the format it was written in; floats are compared
            // through their text because serde_json does not round-trip
            // every last bit by default.
            let parsed: RunRecord = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed.workload, record.workload);
            assert_eq!((parsed.files, parsed.bytes), (record.files, record.bytes));
            assert!((parsed.ready_ms - record.ready_ms).abs() < 1e-6);
            assert!(markdown_row(&record).starts_with(&format!("| {} |", workload.name())));
        }
        assert_eq!(
            fs::read_dir(volume.path()).unwrap().count(),
            0,
            "scratch left behind"
        );
    }

    #[test]
    fn workload_and_medium_names_round_trip() {
        for workload in Workload::ALL {
            assert_eq!(Workload::parse(workload.name()), Some(workload));
        }
        assert_eq!(Medium::parse("external-hdd"), Some(Medium::ExternalHdd));
        assert_eq!(Medium::parse("floppy"), None);
    }
}
