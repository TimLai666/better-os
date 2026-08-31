//! The GNOME Shell adapter extension, checked from the Rust side.
//!
//! The extension is the one piece of Better OS that is not Rust, which makes it
//! the one piece the compiler cannot check. These tests are what stands in for
//! that: the interface both halves speak is a file, and this asserts that the
//! Rust client and that file name the same methods and the same signals, that
//! the packaged metadata is the metadata the packaging installs, and that the
//! JavaScript at least parses.
//!
//! What they cannot do is run it. A GNOME Shell extension only runs inside
//! GNOME Shell, and this repository does not install one into the developer's
//! session — `AGENTS.md` forbids touching host state. So the behaviour of the
//! extension against a live shell is untested here, and
//! `docs/tickets/38-gnome-gesture-adapter.md` records that honestly rather than
//! this file implying otherwise.

use touchpad_session::gnome::{INTERFACE_NAME, METHODS, SIGNALS};

fn adapter_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapters/gnome-shell-touchpad")
        .canonicalize()
        .expect("the adapter directory ships in this repository")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(adapter_dir().join(name))
        .unwrap_or_else(|error| panic!("{name} could not be read: {error}"))
}

/// Every `name="…"` that follows one of these tags. Enough of an XML reader for
/// a file this shape, and it needs no dependency to be one.
fn declared(xml: &str, tag: &str) -> Vec<String> {
    let mut names = Vec::new();
    let opening = format!("<{tag} name=\"");
    for (index, _) in xml.match_indices(&opening) {
        let rest = &xml[index + opening.len()..];
        let end = rest.find('"').expect("a closing quote");
        names.push(rest[..end].to_string());
    }
    names
}

#[test]
fn the_interface_file_declares_exactly_the_members_the_client_calls() {
    let xml = read("org.betteros.TouchpadAdapter1.xml");
    assert_eq!(
        declared(&xml, "interface"),
        vec![INTERFACE_NAME.to_string()]
    );

    let mut methods = declared(&xml, "method");
    methods.sort();
    let mut expected: Vec<String> = METHODS.iter().map(|name| name.to_string()).collect();
    expected.sort();
    assert_eq!(methods, expected);

    let mut signals = declared(&xml, "signal");
    signals.sort();
    let mut expected: Vec<String> = SIGNALS.iter().map(|name| name.to_string()).collect();
    expected.sort();
    assert_eq!(signals, expected);
}

/// The two signals carry the same five values in the same order, which is what
/// lets one deserialization read both.
#[test]
fn both_gesture_signals_have_the_shape_the_client_deserializes() {
    let xml = read("org.betteros.TouchpadAdapter1.xml");
    for signal in SIGNALS {
        let start = xml
            .find(&format!("<signal name=\"{signal}\""))
            .unwrap_or_else(|| panic!("{signal} is not declared"));
        let end = xml[start..].find("</signal>").expect("a closed signal") + start;
        let body = &xml[start..end];
        let types: Vec<&str> = body
            .match_indices("type=\"")
            .map(|(index, _)| {
                let rest = &body[index + 6..];
                &rest[..rest.find('"').expect("a closing quote")]
            })
            .collect();
        assert_eq!(types, vec!["u", "u", "d", "d", "t"], "{signal}");
    }
}

#[test]
fn the_extension_loads_the_interface_file_that_ships_beside_it() {
    let extension = read("extension.js");
    assert!(
        extension.contains("org.betteros.TouchpadAdapter1.xml"),
        "the extension does not read the interface file, so the two can drift"
    );
    // And it takes the bus name and the object path from the same contract the
    // Rust client uses.
    assert!(extension.contains(touchpad_session::gnome::BUS_NAME));
    assert!(extension.contains(touchpad_session::gnome::OBJECT_PATH));
}

/// The bound of the language exception, asserted rather than trusted.
///
/// ADR 0012 allows GJS for bridging typed events and actions and for nothing
/// else. A threshold, a spawn, or a settings read appearing in this file is the
/// exception being widened, and it should fail a test rather than pass a
/// review.
#[test]
fn the_extension_runs_nothing_and_decides_nothing() {
    // Comments are stripped first, the same way the equivalent guards in
    // `better-actions` and `touchpad-session` do it: the header of this file
    // names every forbidden thing on purpose, in order to forbid it.
    let extension: String = read("extension.js")
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !(trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*'))
        })
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "spawn",
        "GLib.spawn_command_line",
        "Gio.Subprocess",
        "Gio.Settings",
        "threshold",
        "cooldown",
        "setTimeout",
    ] {
        assert!(
            !extension.contains(forbidden),
            "the extension names {forbidden}, which is outside the bounds ADR 0012 set"
        );
    }
}

#[test]
fn the_packaged_metadata_names_the_uuid_the_packaging_installs_under() {
    let metadata = read("metadata.json");
    assert!(metadata.contains("\"touchpad-adapter@betteros.org\""));
    // GNOME 46 is the release the conflict model and the shell facilities in
    // `gnome.rs` were written against. A shell-version list that quietly grew
    // would be a claim about a release nothing here has been checked on.
    assert!(
        metadata.contains("\"shell-version\": [\"46\"]"),
        "{metadata}"
    );
}

/// Validates the JavaScript with the interpreter GNOME Shell itself uses.
///
/// `gjs -m` parses the module before it resolves any import, so a syntax error
/// is reported as a parse failure while the missing shell resources are
/// reported as an import failure. That distinction is what makes this a usable
/// check outside a shell.
///
/// Where `gjs` is not installed the test says so and passes: it is a check that
/// can be run rather than a dependency this workspace takes.
#[test]
fn the_extension_is_valid_javascript() {
    let path = adapter_dir().join("extension.js");
    let output = match std::process::Command::new("gjs")
        .arg("-m")
        .arg(&path)
        .output()
    {
        Ok(output) => output,
        Err(_) => {
            eprintln!(
                "skipping: gjs is not installed, so extension.js was not parsed. \
                 The interface, metadata, and bounds checks above still ran."
            );
            return;
        }
    };
    let reported = String::from_utf8_lossy(&output.stderr);
    assert!(
        !reported.contains("SyntaxError") && !reported.contains("Failed to parse module"),
        "gjs could not parse extension.js:\n{reported}"
    );
    // The only failure a parse outside GNOME Shell should produce is the
    // missing shell resource, which is proof the file was parsed and then tried
    // to import.
    assert!(
        reported.is_empty() || reported.contains("resource:///org/gnome/shell"),
        "gjs reported something other than the expected missing shell import:\n{reported}"
    );
}
