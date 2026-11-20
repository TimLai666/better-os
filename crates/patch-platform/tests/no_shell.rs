//! Nothing in `patch-core` or `patch-platform` spawns a shell or takes a
//! command from data (ticket 90, ADR 0017), asserted over the crates'
//! source as `touchpad-gui` asserts its own.
//!
//! - `patch-core` does no I/O at all: no process, no file, no socket.
//! - In `patch-platform`, `src/command.rs` is the only file that names
//!   `std::process`, and it starts exactly one thing: the program and the
//!   arguments a `patch_core::Command` names, whose programs are absolute
//!   paths from a closed list (`patch_core::command::PROGRAMS`).
//! - No file names a shell or a shell's `-c`.

use std::path::{Path, PathBuf};

fn sources(crate_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut pending = vec![crate_dir.join("src")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                // Comments may name what the code must not do; that is how
                // the rule is explained.
                let code = std::fs::read_to_string(&path)
                    .unwrap()
                    .lines()
                    .filter(|line| {
                        let line = line.trim_start();
                        !line.starts_with("//")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                found.push((path, code));
            }
        }
    }
    assert!(!found.is_empty(), "no source under {}", crate_dir.display());
    found
}

fn patch_core_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../patch-core")
}

fn patch_platform_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const SHELLS: [&str; 9] = [
    "\"sh\"",
    "/bin/sh",
    "/bin/bash",
    "\"bash\"",
    "\"-c\"",
    "\"zsh\"",
    "\"dash\"",
    "/usr/bin/env",
    "execvp",
];

#[test]
fn no_file_names_a_shell() {
    for crate_dir in [patch_core_dir(), patch_platform_dir()] {
        for (path, code) in sources(&crate_dir) {
            for shell in SHELLS {
                assert!(!code.contains(shell), "{} names {shell}", path.display());
            }
        }
    }
}

#[test]
fn patch_core_does_no_io() {
    for (path, code) in sources(&patch_core_dir()) {
        for io in [
            "std::process",
            "std::fs",
            "std::net",
            "std::env",
            "File::open",
            "File::create",
            "OpenOptions",
            "Command::new",
            "libc::",
        ] {
            assert!(!code.contains(io), "{} names {io}", path.display());
        }
    }
}

#[test]
fn only_the_command_module_starts_a_process_and_only_what_a_command_names() {
    for (path, code) in sources(&patch_platform_dir()) {
        let is_command_module = path.ends_with("src/command.rs");
        for token in ["std::process::Command", "process::Command", "Command::new"] {
            if is_command_module {
                continue;
            }
            assert!(
                !code.contains(token),
                "{} names {token}; only src/command.rs may start a process",
                path.display()
            );
        }
        for token in ["libc::exec", "nix::unistd", "posix_spawn", "fork("] {
            assert!(!code.contains(token), "{} names {token}", path.display());
        }
        if is_command_module {
            assert_eq!(
                code.matches("Command::new(").count(),
                1,
                "src/command.rs starts one kind of process"
            );
            assert!(code.contains("std::process::Command::new(program)"));
            assert!(code.contains(".args(command.args())"));
            assert!(code.contains("let program: &'static str = command.program();"));
            // No other argument is added.
            assert_eq!(code.matches(".arg(").count(), 0);
            assert_eq!(code.matches(".args(command.args())").count(), 1);
            let rest = code.replacen(".args(command.args())", "", 1);
            assert!(
                !rest.contains(".args("),
                "src/command.rs adds arguments of its own"
            );
            // Nothing is read from the environment to choose what runs.
            assert!(!code.contains("env::var"));
        }
    }
}

#[test]
fn every_program_a_command_names_is_an_absolute_path_on_the_closed_list() {
    for program in patch_core::command::PROGRAMS {
        assert!(program.starts_with("/usr/bin/"), "{program}");
        let name = program.rsplit('/').next().unwrap();
        assert!(
            ["fc-match", "fc-cache", "flatpak"].contains(&name),
            "{program}"
        );
    }
}
