//! `better-storage-bench` — the real-device benchmark, one command per device.
//!
//! ```text
//! cargo run --release -p storage-platform --bin better-storage-bench -- \
//!     --mount /media/$USER/STICK --medium usb-flash \
//!     --out engineering/performance/storage-runs.jsonl
//! ```
//!
//! It needs no privilege. It writes about 1.1 GB inside a fresh hidden
//! directory on the named mount, removes it, and appends one JSON line per
//! workload to `--out` (format: `storage_platform::bench::RunRecord`), printing
//! a Markdown row for each. It refuses `/` and anything that is not itself a
//! mount point. It never mounts, unmounts, or opens a block device.
//!
//! `--workload NAME` runs one workload; `--repeat N` repeats each (default 3).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use storage_platform::Roots;
use storage_platform::bench::{self, Medium, RunContext, Shape, Workload};

const USAGE: &str = "usage: better-storage-bench --mount PATH --medium usb-flash|external-ssd|external-hdd --out FILE [--workload large-sequential|many-small|metadata-tree] [--repeat N]";

fn main() -> ExitCode {
    let mut mount = None;
    let mut medium = None;
    let mut out = None;
    let mut workloads = Workload::ALL.to_vec();
    let mut repeat = 3_u32;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let value = arguments.next();
        match (argument.as_str(), value) {
            ("--mount", Some(value)) => mount = Some(PathBuf::from(value)),
            ("--medium", Some(value)) => medium = Medium::parse(&value),
            ("--out", Some(value)) => out = Some(PathBuf::from(value)),
            ("--workload", Some(value)) => match Workload::parse(&value) {
                Some(workload) => workloads = vec![workload],
                None => return usage(),
            },
            ("--repeat", Some(value)) => match value.parse() {
                Ok(count) if count > 0 => repeat = count,
                _ => return usage(),
            },
            _ => return usage(),
        }
    }
    let (Some(mount), Some(medium), Some(out)) = (mount, medium, out) else {
        return usage();
    };

    let mountinfo = match std::fs::read_to_string("/proc/self/mountinfo") {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read /proc/self/mountinfo: {error}");
            return ExitCode::FAILURE;
        }
    };
    let facts = match bench::check_target(&mount, &mountinfo) {
        Ok(facts) => facts,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let rotational = rotational(&facts.source);
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let context = RunContext {
        mount: &facts,
        medium,
        rotational,
        kernel: &kernel,
        roots: Roots::system(),
    };

    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&out)
    {
        Ok(file) => file,
        Err(error) => {
            eprintln!("cannot open {}: {error}", out.display());
            return ExitCode::FAILURE;
        }
    };
    println!(
        "{} on {} ({}), kernel {kernel}",
        facts.source,
        facts.mount_point.display(),
        facts.filesystem
    );
    println!(
        "| workload | medium | fs | kernel | copy ms | flush ms | ready ms | app MiB/s | durable MiB/s | CPU ms | writer scan |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for workload in workloads {
        for _ in 0..repeat {
            match bench::run(&context, workload, Shape::RECORDED) {
                Ok(record) => {
                    println!("{}", bench::markdown_row(&record));
                    let line = serde_json::to_string(&record).expect("a record serializes");
                    if let Err(error) = writeln!(file, "{line}") {
                        eprintln!("cannot append to {}: {error}", out.display());
                        return ExitCode::FAILURE;
                    }
                }
                Err(error) => {
                    eprintln!("{} failed: {error}", workload.name());
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    ExitCode::SUCCESS
}

/// `/sys/block/<disk>/queue/rotational` for a source such as `/dev/sdb1`.
/// `None` when the source is not a plain disk partition this can map.
fn rotational(source: &str) -> Option<bool> {
    let name = source.strip_prefix("/dev/")?;
    let disk = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let disk = if disk.len() == name.len() { name } else { disk };
    let text = std::fs::read_to_string(format!("/sys/block/{disk}/queue/rotational")).ok()?;
    Some(text.trim() == "1")
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
