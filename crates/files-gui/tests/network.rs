//! Server addresses in the path bar, through GVfs.

use std::path::PathBuf;

use files_gui::network::{asks_for_password, local_path_from_info, server_address};

#[test]
fn only_the_schemes_gvfs_mounts_are_server_addresses() {
    assert_eq!(
        server_address(" sftp://host/home "),
        Some("sftp://host/home".into())
    );
    assert_eq!(
        server_address("SMB://nas/share"),
        Some("smb://nas/share".into())
    );
    assert_eq!(
        server_address("davs://cloud.example/remote.php/dav"),
        Some("davs://cloud.example/remote.php/dav".into())
    );
    assert_eq!(server_address("/home/a"), None);
    assert_eq!(server_address("http://example.com"), None);
    assert_eq!(server_address("sftp://"), None);
    assert_eq!(server_address("sftp:///etc"), None, "a server needs a host");
}

#[test]
fn the_folder_gvfs_shows_a_server_under_is_read_from_gio_info() {
    let info = "display name: / on host\nlocal path: /run/user/1000/gvfs/sftp:host=host\nuri: sftp://host/\n";
    assert_eq!(
        local_path_from_info(info),
        Some(PathBuf::from("/run/user/1000/gvfs/sftp:host=host"))
    );
    assert_eq!(local_path_from_info("uri: sftp://host/\n"), None);
}

#[test]
fn a_prompt_for_a_password_is_recognized() {
    assert!(asks_for_password(
        "Password: \ngio: sftp://host/: Permission denied"
    ));
    assert!(!asks_for_password(
        "gio: sftp://nohost/: Could not resolve hostname"
    ));
}
