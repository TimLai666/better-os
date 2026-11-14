//! Tailnet machines, Taildrop and Dropbox, read from their tools' output.

use std::path::PathBuf;

use files_gui::share::{
    Machine, parse_filestatus, parse_sharelink, parse_status, sftp_address, taildrop_command,
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
