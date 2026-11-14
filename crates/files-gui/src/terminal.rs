//! Open Terminal Here.
//!
//! folio starts `xdg-terminal-exec --dir=<folder>`, the freedesktop way to
//! start "the person's terminal". Better OS does the same first, then tries
//! the terminals a GNOME, KDE or tiling desktop usually has, each with its
//! own working-directory flag. The folder is always one argument of its own:
//! nothing is put through a shell.

use std::path::Path;

use app_catalog_core::Invocation;

/// Terminals, in the order they are tried, and how each is told where to
/// start.
const TERMINALS: &[(&str, Flag)] = &[
    ("xdg-terminal-exec", Flag::Joined("--dir=")),
    ("ptyxis", Flag::Joined("--working-directory=")),
    ("kgx", Flag::Joined("--working-directory=")),
    ("gnome-terminal", Flag::Joined("--working-directory=")),
    ("konsole", Flag::Separate("--workdir")),
    ("xfce4-terminal", Flag::Joined("--working-directory=")),
    ("tilix", Flag::Joined("--working-directory=")),
    ("foot", Flag::Joined("--working-directory=")),
    ("alacritty", Flag::Separate("--working-directory")),
    ("kitty", Flag::Separate("--directory")),
    ("wezterm", Flag::Subcommand("start", "--cwd")),
];

#[derive(Clone, Copy, Debug)]
enum Flag {
    /// `--flag=<folder>`.
    Joined(&'static str),
    /// `--flag <folder>`.
    Separate(&'static str),
    /// `<subcommand> --flag <folder>`.
    Subcommand(&'static str, &'static str),
}

/// What to start for a terminal in `folder`, given which programs are
/// installed. `None` when none is.
pub fn terminal_invocation(folder: &Path, installed: &dyn Fn(&str) -> bool) -> Option<Invocation> {
    let folder = folder.to_string_lossy().into_owned();
    TERMINALS
        .iter()
        .find(|(program, _)| installed(program))
        .map(|(program, flag)| Invocation {
            program: (*program).to_string(),
            arguments: match flag {
                Flag::Joined(prefix) => vec![format!("{prefix}{folder}")],
                Flag::Separate(name) => vec![(*name).to_string(), folder],
                Flag::Subcommand(sub, name) => {
                    vec![(*sub).to_string(), (*name).to_string(), folder]
                }
            },
        })
}

/// Whether `program` is an executable file on `$PATH`.
pub fn on_path(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            std::fs::metadata(dir.join(program))
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
    })
}
