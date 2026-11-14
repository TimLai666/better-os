//! Open Terminal Here picks the installed terminal and passes the folder as
//! one argument, never through a shell.

use std::path::Path;

use files_gui::terminal::terminal_invocation;

#[test]
fn xdg_terminal_exec_is_preferred_as_in_folio() {
    let all = |_: &str| true;
    let invocation = terminal_invocation(Path::new("/home/a/My Files"), &all).unwrap();
    assert_eq!(invocation.program, "xdg-terminal-exec");
    assert_eq!(invocation.arguments, ["--dir=/home/a/My Files"]);
}

#[test]
fn each_terminal_gets_its_own_flag() {
    let only = |name: &'static str| move |program: &str| program == name;
    let folder = Path::new("/w");
    let konsole = terminal_invocation(folder, &only("konsole")).unwrap();
    assert_eq!(konsole.arguments, ["--workdir", "/w"]);
    let wezterm = terminal_invocation(folder, &only("wezterm")).unwrap();
    assert_eq!(wezterm.arguments, ["start", "--cwd", "/w"]);
    let ptyxis = terminal_invocation(folder, &only("ptyxis")).unwrap();
    assert_eq!(ptyxis.arguments, ["--working-directory=/w"]);
}

#[test]
fn with_no_terminal_installed_there_is_nothing_to_start() {
    assert!(terminal_invocation(Path::new("/w"), &|_: &str| false).is_none());
}

#[test]
fn a_folder_name_with_shell_characters_stays_one_argument() {
    let invocation =
        terminal_invocation(Path::new("/w/$(rm -rf ~); x"), &|p: &str| p == "foot").unwrap();
    assert_eq!(
        invocation.arguments,
        ["--working-directory=/w/$(rm -rf ~); x"]
    );
}
