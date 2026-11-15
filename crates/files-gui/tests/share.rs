//! Tailnet machines, Taildrop and Dropbox, read from their tools' output.

use std::path::PathBuf;

use files_gui::share::{
    Machine, parse_filestatus, parse_sharelink, parse_status, sftp_address, sidebar_machines,
    taildrop_command, tailnet_machines_from,
};

const STATUS: &str = r#"{
  "BackendState": "Running",
  "Self": {"HostName": "me", "DNSName": "me.tail0.ts.net.", "Online": true},
  "Peer": {
    "nodekey:1": {"HostName": "nas", "DNSName": "nas.tail0.ts.net.", "Online": false, "OS": "linux"},
    "nodekey:2": {"HostName": "Laptop", "DNSName": "laptop.tail0.ts.net.", "Online": true, "OS": "windows"},
    "nodekey:3": {"HostName": "", "DNSName": "phone.tail0.ts.net.", "Online": true, "OS": "android"}
  }
}"#;

#[test]
fn the_tailnet_lists_online_machines_first_and_never_itself() {
    let machines = parse_status(STATUS);
    let names: Vec<&str> = machines
        .iter()
        .map(|machine| machine.name.as_str())
        .collect();
    assert_eq!(names, ["Laptop", "phone", "nas"]);
    assert_eq!(machines[0].address, "laptop.tail0.ts.net");
    assert_eq!(sftp_address(&machines[0]), "sftp://laptop.tail0.ts.net/");
    assert!(parse_status(r#"{"BackendState":"Stopped","Peer":{}}"#).is_empty());
    assert!(parse_status("not json").is_empty());
}

#[test]
fn taildrop_sends_each_file_as_its_own_argument() {
    let machine = Machine {
        name: "nas".into(),
        address: "nas.tail0.ts.net".into(),
        online: true,
        os: "linux".into(),
    };
    assert_eq!(
        taildrop_command(
            &[PathBuf::from("/h/a b.txt"), PathBuf::from("/h/$(x)")],
            &machine
        ),
        ["file", "cp", "/h/a b.txt", "/h/$(x)", "nas.tail0.ts.net:"]
    );
}

#[test]
fn dropbox_state_and_links_are_read_from_its_output() {
    assert_eq!(
        parse_filestatus("/home/a/Dropbox: up to date\n").as_deref(),
        Some("up to date")
    );
    assert_eq!(parse_filestatus(""), None);
    assert_eq!(
        parse_sharelink("https://www.dropbox.com/s/abc/x.pdf?dl=0\n").as_deref(),
        Some("https://www.dropbox.com/s/abc/x.pdf?dl=0")
    );
    assert_eq!(parse_sharelink("Dropbox isn't running!\n"), None);
}

/// Two phones that both call themselves "localhost" are listed by their
/// MagicDNS names, so the sidebar never shows two rows with one name.
#[test]
fn machines_with_the_same_name_are_told_apart_by_their_dns_names() {
    let status = r#"{"BackendState":"Running","Peer":{
        "a":{"HostName":"localhost","DNSName":"pixel.tail1.ts.net.","Online":false,"OS":"android"},
        "b":{"HostName":"localhost","DNSName":"iphone.tail1.ts.net.","Online":false,"OS":"iOS"},
        "c":{"HostName":"studio","DNSName":"studio.tail1.ts.net.","Online":true,"OS":"linux"}}}"#;
    let names: Vec<String> = parse_status(status)
        .into_iter()
        .map(|machine| machine.name)
        .collect();
    assert_eq!(names, ["studio", "iphone", "pixel"]);
}

/// A fake `tailscale` that prints `output` and exits with `status` for
/// `status --json`, and fails for anything else.
fn fake_tailscale(folder: &std::path::Path, output: &str, status: i32) -> String {
    use std::os::unix::fs::PermissionsExt;
    let json = folder.join("status.json");
    std::fs::write(&json, output).unwrap();
    let program = folder.join("tailscale");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\n[ \"$1 $2\" = \"status --json\" ] || exit 2\ncat '{}'\nexit {status}\n",
            json.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program.display().to_string()
}

#[test]
fn the_sidebar_lists_the_online_machines_a_running_tailscale_reports() {
    let folder = tempfile::tempdir().unwrap();
    let program = fake_tailscale(folder.path(), STATUS, 0);
    let machines = tailnet_machines_from(&program);
    assert_eq!(machines.len(), 3);
    let shown: Vec<(String, String)> = sidebar_machines(machines)
        .into_iter()
        .map(|machine| (machine.name, sftp_address_of(&machine.address)))
        .collect();
    assert_eq!(
        shown,
        [
            (
                "Laptop".to_string(),
                "sftp://laptop.tail0.ts.net/".to_string()
            ),
            (
                "phone".to_string(),
                "sftp://phone.tail0.ts.net/".to_string()
            ),
        ],
        "the offline nas is left out; each opens over SFTP by its tailnet name"
    );
}

fn sftp_address_of(address: &str) -> String {
    sftp_address(&Machine {
        name: String::new(),
        address: address.to_string(),
        online: true,
        os: String::new(),
    })
}

#[test]
fn nothing_is_listed_when_tailscale_is_missing_or_not_running() {
    let folder = tempfile::tempdir().unwrap();
    // Not installed.
    assert!(
        tailnet_machines_from(&folder.path().join("tailscale").display().to_string()).is_empty()
    );
    // Installed, stopped: it says so and exits with an error.
    let stopped = fake_tailscale(
        folder.path(),
        r#"{"BackendState": "Stopped", "Peer": null}"#,
        1,
    );
    assert!(tailnet_machines_from(&stopped).is_empty());
    // Running but logged out: a state other than Running.
    let other = tempfile::tempdir().unwrap();
    let logged_out = fake_tailscale(other.path(), r#"{"BackendState": "NeedsLogin"}"#, 0);
    assert!(tailnet_machines_from(&logged_out).is_empty());
}
