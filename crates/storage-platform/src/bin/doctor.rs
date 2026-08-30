//! `better-storage-doctor` — what this machine exposes to the storage model.
//!
//! Run it by hand:
//!
//! ```text
//! cargo run -p storage-platform --bin better-storage-doctor
//! cargo run -p storage-platform --bin better-storage-doctor -- --flush
//! ```
//!
//! It needs no privileges and changes nothing, unless `--flush` is passed, in
//! which case it issues one filesystem-scoped flush per mounted external
//! volume so the flush path can be confirmed on real hardware.

use storage_platform::{LinuxFlush, Roots, UDisks2, probe};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let flush = std::env::args().any(|argument| argument == "--flush");

    let udisks = match UDisks2::connect().await {
        Ok(udisks) => udisks,
        Err(error) => {
            eprintln!("could not reach UDisks2: {error}");
            eprintln!(
                "without it this host has no external-device inventory, and the storage service reports every device as unknown rather than guessing."
            );
            return std::process::ExitCode::FAILURE;
        }
    };

    match probe(&udisks, &LinuxFlush, Roots::system(), flush).await {
        Ok(report) => {
            print!("{report}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("probe failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
