//! `better-monitor`.
//!
//! The binary is deliberately thin: parse, run, print, and choose an exit
//! code. Everything a subcommand actually does lives in the library, where it
//! can be tested without a process.

use clap::Parser;
use monitor_cli::{Cli, run};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli).await {
        Ok(output) => {
            print!("{output}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("better-monitor: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
