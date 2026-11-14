//! The Better OS GNOME Shell adapter extension, checked from the Rust side.
//!
//! The extension is JavaScript, which the compiler cannot check, and it runs
//! only inside GNOME Shell, which this repository never installs into the
//! developer's session. These tests are what stands in for that: the interface
//! file and `window_attention_core::bus` name the same members, and the bound
//! ADR 0018 sets on the file is asserted rather than trusted. The extension's
//! behaviour against a live shell is recorded in ticket 106, from a headless
//! GNOME Shell 46 in a throwaway session.

use window_attention_core::bus::{
    ADAPTER_BUS_NAME, ADAPTER_INTERFACE, ADAPTER_INTERFACE_FILE, ADAPTER_METHODS,
    ADAPTER_OBJECT_PATH, ADAPTER_SIGNALS, EXTENSION_UUID, PROTOCOL_VERSION, SERVICE_BUS_NAME,
    WINDOW_ATTENTION_ENDED_SIGNATURE, WINDOW_ATTENTION_SIGNATURE,
};

fn adapter_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapters/gnome-shell-adapter")
        .canonicalize()
        .expect("the adapter directory ships in this repository")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(adapter_dir().join(name))
        .unwrap_or_else(|error| panic!("{name} could not be read: {error}"))
}

/// JavaScript with every comment removed: `/* … */` blocks wherever they are
/// and `//` to the end of a line outside a string. The header names every
/// forbidden thing on purpose, in order to forbid it.
fn strip_comments(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => {
                quote = Some(c);
                out.push(c);
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for inner in chars.by_ref() {
                    if previous == '*' && inner == '/' {
                        break;
                    }
                    if inner == '\n' {
                        out.push('\n');
                    }
                    previous = inner;
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn code(name: &str) -> String {
    strip_comments(&read(name))
}

fn declared(xml: &str, tag: &str) -> Vec<String> {
    let opening = format!("<{tag} name=\"");
    let mut names: Vec<String> = xml
        .match_indices(&opening)
        .map(|(index, _)| {
            let rest = &xml[index + opening.len()..];
            rest[..rest.find('"').expect("a closing quote")].to_string()
        })
        .collect();
    names.sort();
    names
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|name| name.to_string()).collect();
    names.sort();
    names
}

/// The body of one method of the extension's class.
fn method_body(extension: &str, name: &str) -> String {
    let start = extension
        .find(&format!("    {name}("))
        .unwrap_or_else(|| panic!("{name} is not defined"));
    let rest = &extension[start..];
    let end = rest.find("\n    }\n").expect("a closed method") + "\n    }".len();
    rest[..end].to_string()
}

#[test]
fn the_interface_file_declares_exactly_the_members_the_service_uses() {
    let xml = read(ADAPTER_INTERFACE_FILE);
    assert_eq!(declared(&xml, "interface"), vec![ADAPTER_INTERFACE]);
    assert_eq!(declared(&xml, "method"), sorted(&ADAPTER_METHODS));
    assert_eq!(declared(&xml, "signal"), sorted(&ADAPTER_SIGNALS));

    let start = xml.find("<signal name=\"WindowAttention\"").unwrap();
    let body = &xml[start..start + xml[start..].find("</signal>").unwrap()];
    let types: String = body
        .match_indices("type=\"")
        .map(|(index, _)| &body[index + 6..index + 7])
        .collect();
    assert_eq!(types, WINDOW_ATTENTION_SIGNATURE);
}

/// No method takes free text: every argument going into the shell is the
/// shell's own window id or a boolean.
#[test]
fn no_method_takes_text() {
    let xml = read(ADAPTER_INTERFACE_FILE);
    for (index, _) in xml.match_indices("direction=\"in\"") {
        let line_start = xml[..index].rfind('<').unwrap();
        let line = &xml[line_start..index];
        assert!(
            line.contains("type=\"t\"") || line.contains("type=\"b\""),
            "an input argument that is not a window id or a boolean: {line}"
        );
    }
}

#[test]
fn the_extension_uses_the_contract_the_service_uses() {
    let extension = code("extension.js");
    for name in [
        ADAPTER_BUS_NAME,
        ADAPTER_OBJECT_PATH,
        ADAPTER_INTERFACE_FILE,
        SERVICE_BUS_NAME,
    ] {
        assert!(
            extension.contains(name),
            "extension.js does not name {name}"
        );
    }
    assert!(extension.contains(&format!("const PROTOCOL_VERSION = {PROTOCOL_VERSION};")));
    assert!(extension.contains(&format!("'({WINDOW_ATTENTION_SIGNATURE})'")));
    assert!(extension.contains(&format!("'({WINDOW_ATTENTION_ENDED_SIGNATURE})'")));
    // The two display signals, and GNOME's handler as GNOME connects it.
    for signal in ["'window-demands-attention'", "'window-marked-urgent'"] {
        assert_eq!(extension.matches(signal).count(), 2, "{signal}");
    }
    assert!(extension.contains("handler._onWindowDemandsAttention.bind(handler)"));
}

/// What the module may declare at its top level: imports, one constant per
/// line holding a string or a number, and the class.
fn allowed_top_level(line: &str) -> bool {
    let constant = line
        .strip_prefix("const ")
        .and_then(|rest| rest.split_once(" = "))
        .is_some_and(|(name, value)| {
            name.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && value.ends_with(';')
                && {
                    let value = value.trim_end_matches(';');
                    (value.starts_with('\'')
                        && value.ends_with('\'')
                        && !value.contains(['[', '{']))
                        || value.parse::<u32>().is_ok()
                }
        });
    constant
        || (line.starts_with("import ") && line.ends_with("';") && line.contains(" from '"))
        || line == "export default class ShellAdapterExtension extends Extension {"
        || line == "}"
}

/// The methods the class may have.
const METHODS: [&str; 19] = [
    "enable",
    "disable",
    "_interfaceXml",
    "_acceptIfPackaged",
    "_tellCounterpart",
    "_fromCounterpart",
    "SuppressBuiltInAttentionAsync",
    "_suppressBuiltInAttention",
    "_restoreBuiltInAttention",
    "_reportAttention",
    "_reportIfWithdrawn",
    "_reportEnded",
    "_windowById",
    "ShowAttentionBannerAsync",
    "ActivateWindowAsync",
    "ClearAttentionAsync",
    "Capabilities",
    // Not methods: the two lines a method head pattern would also match.
    "if",
    "for",
];

/// The fields the class may assign: the export, the two bus handles, the
/// counterpart's unique name, and whether the takeover is on.
const FIELDS: [&str; 5] = [
    "_exported",
    "_nameId",
    "_watchId",
    "_counterpart",
    "_suppressed",
];

/// The local names a method body may declare.
const LOCALS: [&str; 9] = [
    "xml",
    "file",
    "contents",
    "handler",
    "app",
    "window",
    "actor",
    "credentials",
    "pid",
];

/// Every condition the code may test, exactly. A new branch is a new
/// decision, and has to be added here on purpose.
const CONDITIONS: [&str; 16] = [
    "if (GLib.file_read_link(`/proc/${pid}/exe`) === COUNTERPART_EXECUTABLE)",
    "if (this._counterpart === null)",
    "if (this._watchId) {",
    "if (this._nameId) {",
    "if (this._exported) {",
    "if (this._counterpart !== null && invocation.get_sender() === this._counterpart)",
    "if (!this._fromCounterpart(invocation))",
    "if (!suppress)",
    "else if (!this._suppressBuiltInAttention())",
    "if (this._suppressed)",
    "if (typeof handler?._onWindowDemandsAttention !== 'function')",
    "if (!this._suppressed)",
    "if (!window)",
    "if (!window.demands_attention && !window.urgent)",
    "if (actor.meta_window.get_id() === id)",
    "if (window)",
];

/// The lines outside a condition that may compare: two capability facts.
const COMPARISONS: [&str; 2] = [
    "counterpart_present: this._counterpart !== null,",
    "typeof Main.windowAttentionHandler?._onWindowDemandsAttention === 'function',",
];

/// The objects the code may construct.
const CONSTRUCTED: [&str; 3] = [
    "new TextDecoder()",
    "new GLib.Variant(",
    "new GLib.VariantType(",
];

/// Everything in `source` that is outside ADR 0018's bound, as an allow-list:
/// what is not named here is refused, whatever it is called.
fn violations(source: &str) -> Vec<String> {
    let code = strip_comments(source);
    let mut found = Vec::new();
    let mut in_class = false;
    for raw in code.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let top_level = !raw.starts_with(' ');
        if top_level {
            if !allowed_top_level(line) {
                found.push(format!("top-level declaration not allowed: {line}"));
            }
            if line.starts_with("export default class") {
                in_class = true;
            }
            continue;
        }
        if !in_class {
            // An import split over lines.
            if !(line.ends_with(',') || line.starts_with('}') || line.ends_with("';")) {
                found.push(format!("statement outside the class: {line}"));
            }
            continue;
        }
        // A method head, four spaces in.
        if raw.starts_with("    ") && !raw.starts_with("     ") && line.ends_with(") {") {
            let name = line.split('(').next().unwrap_or_default();
            if !METHODS.contains(&name) {
                found.push(format!("method not allowed: {name}"));
            }
        }
        for keyword in ["let ", "var ", "static ", "get ", "set "] {
            if line.starts_with(keyword) {
                found.push(format!("declaration not allowed: {line}"));
            }
        }
        for keyword in ["const ", "for (const "] {
            if let Some(rest) = line.strip_prefix(keyword) {
                let end = [" = ", " of "]
                    .iter()
                    .filter_map(|separator| rest.find(separator))
                    .min()
                    .unwrap_or(rest.len());
                let names = &rest[..end];
                for local in names
                    .split(['[', ']', ',', ' '])
                    .filter(|part| !part.is_empty())
                {
                    if !LOCALS.contains(&local) {
                        found.push(format!("local not allowed: {local}"));
                    }
                }
            }
        }
        for (index, _) in line.match_indices("this.") {
            let rest = &line[index + "this.".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let assigned = rest[name.len()..].trim_start().starts_with("= ");
            if assigned && !FIELDS.contains(&name.as_str()) {
                found.push(format!("field not allowed: this.{name}"));
            }
        }
        if line.starts_with("if (") || line.starts_with("else if (") || line.contains(" if (") {
            if !CONDITIONS.contains(&line) {
                found.push(format!("condition not allowed: {line}"));
            }
        } else if [
            "===", "!==", " == ", " != ", " < ", " > ", " <= ", " >= ", "switch", "?:", " ? ",
        ]
        .iter()
        .any(|operator| line.contains(operator))
            && !COMPARISONS.contains(&line)
        {
            found.push(format!("comparison not allowed: {line}"));
        }
        for (index, _) in line.match_indices("new ") {
            let rest = &line[index..];
            if !CONSTRUCTED.iter().any(|allowed| rest.starts_with(allowed)) {
                found.push(format!("construction not allowed: {rest}"));
            }
        }
        // A collection literal that is not a call's argument list or a
        // destructured parameter is somewhere to keep state.
        if line.contains("= [")
            || line.contains("= {")
            || line.contains(": [") && !line.starts_with("relays:")
        {
            found.push(format!("collection literal not allowed: {line}"));
        }
        for forbidden in [
            "spawn",
            "Subprocess",
            "Settings",
            "set_contents",
            "replace_contents",
            "localStorage",
            "setTimeout",
            "timeout_add",
            "JSON.parse",
            "eval(",
        ] {
            if line.contains(forbidden) {
                found.push(format!("{forbidden} is outside the bound: {line}"));
            }
        }
    }
    found
}

/// ADR 0018's bound, as an allow-list against the shipped file.
#[test]
fn the_extension_stores_nothing_runs_nothing_and_decides_nothing() {
    assert_eq!(violations(&read("extension.js")), Vec::<String>::new());
}

/// The allow-list is what refuses a decision, not the name it is given: the
/// review's variant that quiets a list of applications, and the variants that
/// remember what asked, all fail.
#[test]
fn a_variant_that_holds_a_list_or_remembers_what_asked_is_refused() {
    let original = read("extension.js");
    let report = "        const app = Shell.WindowTracker.get_default().get_window_app(window);\n";
    assert!(original.contains(report));
    let variants = [
        original.replace(
            "const PROTOCOL_VERSION = 1;",
            "const PROTOCOL_VERSION = 1;\nconst quietApps = ['com.anthropic.Claude.desktop'];",
        ),
        original.replace(
            report,
            &format!("{report}        if (quietApps.includes(app.get_id()))\n            return;\n"),
        ),
        original.replace(
            "const PROTOCOL_VERSION = 1;",
            "const PROTOCOL_VERSION = 1;\nlet asked = 0;",
        ),
        original.replace(
            report,
            &format!("{report}        this._asked = (this._asked ?? 0) + 1;\n"),
        ),
        original.replace(
            report,
            &format!("{report}        const seen = app.get_id();\n"),
        ),
        original.replace(
            "        this._suppressed = false;\n\n",
            "        this._suppressed = false;\n        this._counterpart = new Map();\n\n",
        ),
        original.replace(
            report,
            &format!("{report}        const quiet = app.get_id() === 'com.anthropic.Claude.desktop';\n"),
        ),
        original.replace(
            "    _reportIfWithdrawn(window) {",
            "    _remember(window) {\n        return window;\n    }\n\n    _reportIfWithdrawn(window) {",
        ),
    ];
    for (index, variant) in variants.iter().enumerate() {
        assert_ne!(
            variant, &original,
            "variant {index} did not change the file"
        );
        assert!(
            !violations(variant).is_empty(),
            "variant {index} passed the bound:\n{variant}"
        );
    }
}

/// Every window action and the takeover answer only the counterpart's current
/// owner, and the owner is the unique name the watcher reported.
#[test]
fn every_method_answers_only_the_counterpart() {
    let extension = code("extension.js");
    for method in [
        "SuppressBuiltInAttentionAsync",
        "ShowAttentionBannerAsync",
        "ActivateWindowAsync",
        "ClearAttentionAsync",
    ] {
        let body = method_body(&extension, method);
        assert!(
            body.contains("if (!this._fromCounterpart(invocation))\n            return;"),
            "{method}:\n{body}"
        );
    }
    let check = method_body(&extension, "_fromCounterpart");
    assert!(check.contains("invocation.get_sender() === this._counterpart"));
    assert!(check.contains("return_dbus_error(ACCESS_DENIED"));
    // The name's owner becomes the counterpart only through the check that
    // its process runs the packaged service.
    let enable = method_body(&extension, "enable");
    assert!(enable.contains(
        "(connection, _name, owner) => {\n                this._counterpart = null;\n                this._acceptIfPackaged(connection, owner);"
    ));
    let accept = method_body(&extension, "_acceptIfPackaged");
    assert!(accept.contains("'GetConnectionCredentials'"));
    assert!(accept.contains("credentials.ProcessID"));
    assert!(accept.contains(
        "if (GLib.file_read_link(`/proc/${pid}/exe`) === COUNTERPART_EXECUTABLE)\n                        this._counterpart = owner;"
    ));
    assert!(
        extension
            .contains("const COUNTERPART_EXECUTABLE = '/usr/libexec/better-window-attention';")
    );
    assert_eq!(extension.matches("this._counterpart = owner").count(), 1);
}

/// What the relay reports goes to the counterpart alone, never to the whole
/// session bus.
#[test]
fn every_signal_is_sent_to_the_counterpart_alone() {
    let extension = code("extension.js");
    assert!(!extension.contains("this._exported.emit_signal"));
    assert!(!extension.contains("this._exported?.emit_signal"));
    assert_eq!(extension.matches("emit_signal(").count(), 1);
    let tell = method_body(&extension, "_tellCounterpart");
    assert!(
        tell.contains("if (this._counterpart === null)\n            return;"),
        "{tell}"
    );
    assert!(tell.contains("Gio.DBus.session.emit_signal(\n            this._counterpart,"));
    for signal in ["'WindowAttention'", "'WindowAttentionEnded'"] {
        assert!(
            extension.contains(&format!("this._tellCounterpart({signal},")),
            "{signal}"
        );
    }
}

/// GNOME's handler comes back on every path out: the counterpart leaving and
/// the extension being disabled.
#[test]
fn gnome_s_handler_comes_back_when_the_service_goes_and_on_disable() {
    let extension = code("extension.js");
    let disable = method_body(&extension, "disable");
    assert!(
        disable.contains("this._restoreBuiltInAttention()"),
        "{disable}"
    );
    let enable = method_body(&extension, "enable");
    let vanished = enable
        .find("this._counterpart = null;")
        .expect("the vanished callback forgets the owner");
    assert!(
        enable[vanished..].contains("this._restoreBuiltInAttention()"),
        "{enable}"
    );
    // And the end-of-request handlers it put on windows go with it.
    let restore = method_body(&extension, "_restoreBuiltInAttention");
    assert!(restore.contains("actor.meta_window.disconnectObject(this);"));
}

/// Reading the interface file at enable is the one file access.
#[test]
fn the_one_file_read_is_the_interface_beside_it() {
    let extension = code("extension.js");
    assert_eq!(extension.matches("load_contents(").count(), 1);
    // And the one link read: where the counterpart's process runs from.
    assert_eq!(extension.matches("file_read_link(").count(), 1);
    assert_eq!(extension.matches("get_child(").count(), 1);
    assert!(extension.contains("this._interfaceXml(ATTENTION_INTERFACE_FILE)"));
}

#[test]
fn the_metadata_names_the_uuid_and_the_one_checked_shell() {
    let metadata = read("metadata.json");
    assert!(metadata.contains(&format!("\"uuid\": \"{EXTENSION_UUID}\"")));
    assert!(
        metadata.contains("\"shell-version\": [\"46\"]"),
        "{metadata}"
    );
}

/// Parses the JavaScript with the interpreter GNOME Shell uses. Where `gjs` is
/// not installed the test says so and passes.
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
            eprintln!("skipping: gjs is not installed, so extension.js was not parsed");
            return;
        }
    };
    let reported = String::from_utf8_lossy(&output.stderr);
    assert!(
        !reported.contains("SyntaxError") && !reported.contains("Failed to parse module"),
        "gjs could not parse extension.js:\n{reported}"
    );
    assert!(
        reported.is_empty() || reported.contains("resource:///org/gnome/shell"),
        "gjs reported something other than the expected missing shell import:\n{reported}"
    );
}
