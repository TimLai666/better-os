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
//! `engineering/tickets/done/38-gnome-gesture-adapter.md` records that honestly rather than
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

/// A file with its comment lines removed.
///
/// Comments are stripped the same way the equivalent guards in `better-actions`
/// and `touchpad-session` do it: the header of `extension.js` names every
/// forbidden thing on purpose, in order to forbid it.
fn code(name: &str) -> String {
    read(name)
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !(trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*'))
        })
        .collect::<Vec<_>>()
        .join("\n")
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

/// The two signals carry the same six values in the same order, which is what
/// lets one deserialization read both. The sixth is the device node ADR 0015
/// allows, and nothing else was added beside it.
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
        assert_eq!(types, vec!["u", "u", "d", "d", "t", "s"], "{signal}");
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
    let extension = code("extension.js");
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

/// The bound ADR 0015 added, asserted rather than trusted.
///
/// The extension may report one fact about a device: the device node of the
/// event's source device, copied as it is. It may read no other fact about the
/// device, transform or hash nothing, and keep nothing between events.
#[test]
fn the_extension_reports_the_device_node_and_nothing_else_about_the_device() {
    let extension = code("extension.js");
    for accessor in ["get_source_device()", "get_device_node()"] {
        assert!(
            extension.contains(accessor),
            "the extension does not read {accessor}, so it cannot name the device ADR 0015 allows"
        );
    }
    for forbidden in [
        // Every other fact a Clutter input device exposes.
        "get_device_name",
        "get_vendor_id",
        "get_product_id",
        "get_device_type",
        "get_device_mode",
        "get_seat",
        "get_device()",
        "device_name",
        "vendor_id",
        "product_id",
        // Computing something from the node instead of reporting it.
        "compute_checksum",
        "Checksum",
        "hash",
        "split(",
        "replace(",
        "slice(",
        "substring",
        "parseInt",
        // Keeping it.
        "new Map",
        "new Set",
        "WeakMap",
    ] {
        assert!(
            !extension.contains(forbidden),
            "the extension names {forbidden}, which is outside the bounds ADR 0015 set"
        );
    }

    // And it keeps no state beyond what it kept before the device was named:
    // the bus export, the bus name, the stage handler, and the suppression
    // record. A device cache would have to be a new field.
    let mut fields: Vec<&str> = extension
        .match_indices("this.")
        .map(|(index, _)| {
            let rest = &extension[index + "this.".len()..];
            let end = rest
                .find(|character: char| !(character.is_alphanumeric() || character == '_'))
                .unwrap_or(rest.len());
            &rest[..end]
        })
        .filter(|name| {
            // A field is something assigned; a method call is not one.
            let assigned = format!("this.{name} =");
            extension.contains(&assigned)
        })
        .collect();
    fields.sort_unstable();
    fields.dedup();
    assert_eq!(
        fields,
        vec![
            "_capturedId",
            "_counterpart",
            "_exported",
            "_nameId",
            "_suppressed",
            "_watchId"
        ]
    );
}

/// The types an XML signal declares, in order, as one GVariant tuple string.
fn declared_signature(xml: &str, signal: &str) -> String {
    let start = xml
        .find(&format!("<signal name=\"{signal}\""))
        .unwrap_or_else(|| panic!("{signal} is not declared"));
    let end = xml[start..].find("</signal>").expect("a closed signal") + start;
    let body = &xml[start..end];
    let types: String = body
        .match_indices("type=\"")
        .map(|(index, _)| {
            let rest = &body[index + 6..];
            &rest[..rest.find('"').expect("a closing quote")]
        })
        .collect();
    format!("({types})")
}

/// Every signal the extension emits carries exactly the signature the XML
/// declares for it.
///
/// GJS builds the variant from the type string written beside the emit and
/// never checks it against the interface: `PinchGesture` went out as
/// `(uuddt)` with six values, GJS dropped the device node, and the Rust side
/// quietly read every pinch as coming from an unknown device (ticket 131).
#[test]
fn every_emitted_signal_has_the_signature_the_interface_declares() {
    let xml = read("org.betteros.TouchpadAdapter1.xml");
    let extension = code("extension.js");
    let mut emitted = Vec::new();
    for (index, _) in extension.match_indices("emit_signal('") {
        let rest = &extension[index + "emit_signal('".len()..];
        let name = &rest[..rest.find('\'').expect("a closing quote")];
        let variant = rest
            .find("new GLib.Variant('")
            .unwrap_or_else(|| panic!("{name} is emitted without a variant"));
        let rest = &rest[variant + "new GLib.Variant('".len()..];
        let signature = &rest[..rest.find('\'').expect("a closing quote")];
        assert_eq!(
            signature,
            declared_signature(&xml, name),
            "{name} is emitted with a signature the interface does not declare"
        );
        // And with as many values as the signature has types.
        let values_start = rest.find('[').expect("a value list");
        let values_end = rest.find("]))").expect("the end of the value list");
        let values = rest[values_start + 1..values_end]
            .split(',')
            .filter(|value| !value.trim().is_empty())
            .count();
        assert_eq!(
            values,
            signature.len() - 2,
            "{name} carries {values} values"
        );
        emitted.push(name.to_string());
    }
    emitted.sort();
    let mut expected: Vec<String> = SIGNALS.iter().map(|name| name.to_string()).collect();
    expected.sort();
    assert_eq!(emitted, expected, "every declared signal is emitted once");
}

/// Ticket 131: GNOME's swipe trackers stay off only while the gesture service
/// is on the bus, the rule ADR 0018 set for a relay and its counterpart.
#[test]
fn gnome_gets_its_gestures_back_when_the_gesture_service_leaves_the_bus() {
    let extension = code("extension.js");
    assert!(
        extension.contains(&format!(
            "const COUNTERPART = '{}';",
            touchpad_session::gnome::SERVICE_BUS_NAME
        )),
        "the extension watches a name the service does not own"
    );
    assert!(extension.contains(
        "Gio.bus_watch_name(\n            Gio.BusType.SESSION,\n            COUNTERPART,"
    ));
    // The vanished callback puts the trackers back, and nothing else.
    assert!(extension.contains(
        "() => {\n                this._counterpart = null;\n                this._restoreBuiltInGestures();\n            });"
    ));
    // The appeared callback forgets the previous owner and accepts the new
    // one only through the check that its process runs the packaged service
    // (ADR 0035, in ADR 0018's form).
    assert!(extension.contains(
        "(connection, _name, owner) => {\n                this._counterpart = null;\n                this._acceptIfPackaged(connection, owner);"
    ));
    assert_eq!(extension.matches("this._counterpart = owner").count(), 1);
    // And disable stops watching as well as restoring.
    assert!(extension.contains("Gio.bus_unwatch_name(this._watchId);"));
}

/// Only the gesture service's current owner can turn GNOME's gestures off;
/// anyone can put them back.
#[test]
fn only_the_gesture_service_can_take_gnomes_gestures() {
    let extension = code("extension.js");
    let start = extension
        .find("SuppressBuiltInGesturesAsync([suppress], invocation) {")
        .expect("the suppression method takes its invocation");
    let body = &extension[start..start + extension[start..].find("\n    }\n").unwrap()];
    let check = body
        .find("invocation.get_sender() !== this._counterpart")
        .expect("the caller is checked against the counterpart");
    let refusal = body.find("return_dbus_error(").expect("a refusal");
    let suppress = body
        .find("this._suppressBuiltInGestures();")
        .expect("a suppression");
    assert!(check < refusal && refusal < suppress, "{body}");
    assert!(body.contains("this._counterpart === null ||"), "{body}");
    // A plain `SuppressBuiltInGestures(suppress)` would take a request from
    // anyone on the bus.
    assert!(!extension.contains("SuppressBuiltInGestures(suppress) {"));
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
/// Where `gjs` is not installed the test says so and returns on a developer's
/// machine: it is a program the check runs, not a dependency this workspace
/// takes. Under CI, which installs it, a missing `gjs` fails the test
/// (`test_prerequisites`, ticket 177); the interface, metadata, and bounds
/// checks above run either way.
#[test]
fn the_extension_is_valid_javascript() {
    let path = adapter_dir().join("extension.js");
    let output = test_prerequisites::require!(
        "gjs",
        std::process::Command::new("gjs")
            .arg("-m")
            .arg(&path)
            .output()
            .map_err(|error| format!(
                "gjs could not be run, so extension.js was not parsed: {error}"
            ))
    );
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

/// The body of one method of the extension's class, up to its closing brace.
fn method_body(extension: &str, name: &str) -> String {
    let start = extension
        .find(&format!("\n    {name}("))
        .unwrap_or_else(|| panic!("{name} is not defined"));
    let end = extension[start..]
        .find("\n    }\n")
        .unwrap_or_else(|| panic!("{name} is not closed"))
        + start;
    extension[start..end].to_string()
}

/// ADR 0035: the name's owner becomes the counterpart only when the kernel
/// says its process runs the packaged service, the same check ADR 0018 and the
/// shared extension make. Without it any program of the person's could take
/// the name first and be obeyed.
#[test]
fn the_counterpart_is_accepted_only_when_it_runs_the_packaged_service() {
    let extension = code("extension.js");
    assert!(extension.contains(&format!(
        "const COUNTERPART_EXECUTABLE = '{}';",
        touchpad_session::gnome::SERVICE_EXECUTABLE
    )));
    let accept = method_body(&extension, "_acceptIfPackaged");
    assert!(accept.contains("'GetConnectionCredentials'"), "{accept}");
    assert!(accept.contains("credentials.ProcessID"), "{accept}");
    assert!(accept.contains(
        "if (GLib.file_read_link(`/proc/${pid}/exe`) === COUNTERPART_EXECUTABLE)\n                        this._counterpart = owner;"
    ));
    // The one assignment of an owner is inside that check.
    assert_eq!(extension.matches("this._counterpart = owner").count(), 1);
    // And `/proc` is read for that link alone.
    assert_eq!(extension.matches("/proc/").count(), 1);
}

/// Ticket 131: a restore writes what GNOME keeps when nothing is under way,
/// `true`, never a value captured at suppression time, which could be GNOME's
/// own `false` from the middle of a workspace animation.
#[test]
fn a_restore_writes_enabled_rather_than_a_captured_value() {
    let extension = code("extension.js");
    let restore = method_body(&extension, "_restoreBuiltInGestures");
    assert!(
        restore.contains(
            "for (const tracker of this._swipeTrackers())\n            tracker.enabled = true;"
        ),
        "{restore}"
    );
    assert!(!extension.contains("const was = tracker.enabled"));
    let suppress = method_body(&extension, "_suppressBuiltInGestures");
    assert!(suppress.contains("tracker.enabled = false;"), "{suppress}");
}
