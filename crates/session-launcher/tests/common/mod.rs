//! Reading the repository's packaging files, for the tests that hold the
//! session's package data to its rules.

// Each test file uses a part of this module.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The shortcut configuration's directory under the session's data.
pub const SESSION_SHORTCUTS: &str = "cosmic/com.system76.CosmicSettings.Shortcuts/v1";

pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(relative: &str) -> String {
    let path = repository().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// `Key=value` lines of a unit file, in order, with comments left out.
pub fn unit_lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('['))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

pub fn values(lines: &[(String, String)], key: &str) -> Vec<String> {
    lines
        .iter()
        .filter(|(name, _)| name == key)
        .flat_map(|(_, value)| {
            value
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The section names a unit file has.
pub fn sections(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('[') && line.ends_with(']'))
        .map(|line| line[1..line.len() - 1].to_string())
        .collect()
}

/// The quoted strings between `start` and `end` in build-deb.sh.
pub fn build_deb_block(start: &str, end: &str) -> String {
    let script = read("packaging/build-deb.sh");
    let from = script
        .find(start)
        .unwrap_or_else(|| panic!("build-deb.sh has no {start}"));
    let to = from
        + script[from..]
            .find(end)
            .unwrap_or_else(|| panic!("build-deb.sh has no {end}"));
    script[from..to].to_string()
}

/// The session package's `Depends` entries, as build-deb.sh lists them.
pub fn session_depends() -> Vec<String> {
    let block = build_deb_block(
        "if [[ \"$RELEASE_TARGET\" != ubuntu-22.04 ]]; then",
        "make_package better-os-session",
    );
    let depends = &block[block
        .find("PACKAGE_DEPENDS=(")
        .expect("the session package has Depends")..];
    let depends = &depends[..depends.find("\n    )").expect("the Depends list ends")];
    depends
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// Each package's programs under `/usr/bin`, as build-deb.sh's
/// `PACKAGE_BINARIES` lists them before its `make_package` call.
pub fn programs_by_package() -> Vec<(String, String)> {
    let script = read("packaging/build-deb.sh");
    let mut found = Vec::new();
    let mut rest = script.as_str();
    while let Some(start) = rest.find("PACKAGE_BINARIES=(") {
        let block = &rest[start..];
        // The list ends at a ')' that closes it: at the end of its first line
        // (a one-line list) or alone on a line; a ')' in a comment does not.
        let first_line = block.lines().next().unwrap();
        let end = if first_line.trim_end().ends_with(')') {
            first_line.trim_end().len() - 1
        } else {
            block
                .find("\n    )")
                .or_else(|| block.find("\n)"))
                .expect("the list ends")
                + 1
        };
        let list = &block[..end];
        let after = &block[end..];
        let package = after
            .find("make_package ")
            .map(|at| {
                after[at + "make_package ".len()..]
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_string()
            })
            .expect("every binary list belongs to a package");
        for quoted in list.split('"').skip(1).step_by(2) {
            if let Some((_, installed)) = quoted.split_once(':')
                && let Some(program) = installed.strip_prefix("usr/bin/")
            {
                found.push((package.clone(), program.to_string()));
            }
        }
        rest = &block[end..];
    }
    found
}

/// Every `binary:installed/path` a package of build-deb.sh installs.
pub fn installed_binaries() -> Vec<String> {
    read("packaging/build-deb.sh")
        .split('"')
        .filter_map(|quoted| quoted.split_once(':'))
        .filter(|(source, installed)| {
            !source.contains('/')
                && (installed.starts_with("usr/bin/") || installed.starts_with("usr/libexec/"))
        })
        .map(|(_, installed)| format!("/{installed}"))
        .collect()
}

/// Every `source:installed` pair build-deb.sh installs as data.
pub fn installed_data() -> Vec<(String, String)> {
    read("packaging/build-deb.sh")
        .replace("$SESSION_SHORTCUTS", SESSION_SHORTCUTS)
        .split('"')
        .filter_map(|quoted| quoted.split_once(':'))
        .filter(|(source, installed)| {
            (source.starts_with("packaging/") || source.starts_with("adapters/"))
                && installed.starts_with("usr/")
        })
        .map(|(source, installed)| (source.to_string(), installed.to_string()))
        .collect()
}
