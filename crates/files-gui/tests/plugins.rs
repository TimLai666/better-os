//! Plugins as folio's docs/plugins.md defines them: checked, run without a
//! shell, timed out, their JSON read.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use files_gui::plugins::{
    Action, ActionResult, Open, RunError, Skipped, Text, action_shows, applies, discover, load,
    valid_id,
};

fn plugin_dir(root: &Path, id: &str, manifest: &str) -> PathBuf {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("manifest.json"), manifest).unwrap();
    fs::set_permissions(dir.join("manifest.json"), fs::Permissions::from_mode(0o644)).unwrap();
    dir
}

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    root
}

#[test]
fn ids_are_lowercase_letters_digits_and_dashes() {
    assert!(valid_id("tailscale"));
    assert!(valid_id("my-nas2"));
    assert!(!valid_id("-x"));
    assert!(!valid_id("My"));
    assert!(!valid_id("a.b"));
}

#[test]
fn a_folio_manifest_loads_and_its_poll_is_read() {
    let root = root();
    let dir = plugin_dir(
        root.path(),
        "servers",
        r#"{"folio":1,"poll":{"command":["./servers"],"every":60},"section":{"title":"Servers"},"actions":[{"id":"up","label":"Upload To","menu":"share","to":"items","files":true,"command":["./upload"]}],"future":true}"#,
    );
    script(
        &dir,
        "servers",
        r#"echo '{"items":[{"id":"nas","name":"nas","subtitle":["%1 shares","3"],"open":{"mount":"sftp://nas/"}}],"places":[{"path":"~/Dropbox","status":"syncing","tone":"warning"}]}'"#,
    );
    let plugin = load(&dir, "en", &|_| true).unwrap();
    assert_eq!(plugin.manifest.section.as_ref().unwrap().title, "Servers");
    assert_eq!(plugin.poll_every(), Duration::from_secs(60));
    let poll = plugin.poll("en").unwrap();
    assert_eq!(poll.items[0].name, "nas");
    assert_eq!(
        poll.items[0].open,
        Some(Open::Mount {
            mount: "sftp://nas/".into()
        })
    );
    assert_eq!(
        poll.items[0]
            .subtitle
            .as_ref()
            .unwrap()
            .render(&Default::default()),
        "3 shares"
    );
    assert_eq!(poll.places[0].tone.as_deref(), Some("warning"));
}

#[test]
fn a_plugin_anyone_else_can_write_to_is_skipped() {
    let root = root();
    let dir = plugin_dir(root.path(), "open", r#"{"folio":1}"#);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(
        load(&dir, "en", &|_| true),
        Err(Skipped::NotTrusted(_))
    ));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(dir.join("manifest.json"), fs::Permissions::from_mode(0o666)).unwrap();
    assert!(matches!(
        load(&dir, "en", &|_| true),
        Err(Skipped::NotTrusted(_))
    ));
}

#[test]
fn a_missing_program_or_another_version_turns_the_plugin_off() {
    let root = root();
    let dir = plugin_dir(
        root.path(),
        "needs",
        r#"{"folio":1,"requires":["no-such-tool"]}"#,
    );
    assert_eq!(
        load(&dir, "en", &|program| program != "no-such-tool").err(),
        Some(Skipped::Missing("no-such-tool".into()))
    );
    let later = plugin_dir(root.path(), "later", r#"{"folio":2}"#);
    assert_eq!(
        load(&later, "en", &|_| true).err(),
        Some(Skipped::WrongVersion)
    );
    let broken = plugin_dir(root.path(), "broken", "not json");
    assert!(matches!(
        load(&broken, "en", &|_| true),
        Err(Skipped::BadManifest(_))
    ));
}

#[test]
fn the_persons_plugin_replaces_a_packaged_one_with_the_same_id() {
    let mine = root();
    let packaged = root();
    plugin_dir(
        mine.path(),
        "x",
        r#"{"folio":1,"section":{"title":"Mine"}}"#,
    );
    plugin_dir(
        packaged.path(),
        "x",
        r#"{"folio":1,"section":{"title":"Packaged"}}"#,
    );
    plugin_dir(packaged.path(), "y", r#"{"folio":1}"#);
    let (plugins, _) = discover(
        &[mine.path().to_path_buf(), packaged.path().to_path_buf()],
        "en",
        &|_| true,
    );
    assert_eq!(plugins.len(), 2);
    assert_eq!(plugins[0].manifest.section.as_ref().unwrap().title, "Mine");
}

#[test]
fn commands_get_their_arguments_whole_and_a_hung_poll_is_killed() {
    let root = root();
    let dir = plugin_dir(
        root.path(),
        "slow",
        r#"{"folio":1,"poll":{"command":["./poll"]}}"#,
    );
    script(&dir, "poll", "sleep 30");
    script(&dir, "echo-args", r#"printf '%s\n' "$@""#);
    let plugin = load(&dir, "en", &|_| true).unwrap();
    let started = Instant::now();
    assert_eq!(plugin.poll("en"), None, "a poll that hangs counts as empty");
    assert!(started.elapsed() < Duration::from_secs(15));
    let output = plugin
        .run(
            &["./echo-args".into()],
            &["a b".into(), "$(rm -rf ~)".into()],
            &[],
            "en",
            Some(Duration::from_secs(5)),
        )
        .unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "a b\n$(rm -rf ~)\n");
}

#[test]
fn a_failing_action_reports_its_last_error_line() {
    let root = root();
    let dir = plugin_dir(root.path(), "fails", r#"{"folio":1}"#);
    script(
        &dir,
        "send",
        "echo first >&2; echo 'no such peer' >&2; exit 3",
    );
    let plugin = load(&dir, "en", &|_| true).unwrap();
    assert_eq!(
        plugin.run(&["./send".into()], &[], &[], "en", None),
        Err(RunError::Failed {
            code: Some(3),
            last_error: "no such peer".into()
        })
    );
}

#[test]
fn results_and_templates_read_as_folio_writes_them() {
    let result: ActionResult =
        serde_json::from_str(r#"{"error":["Couldn't send to %1: %2","studio","no such peer"]}"#)
            .unwrap();
    assert_eq!(
        result.error.unwrap().render(&Default::default()),
        "Couldn't send to studio: no such peer"
    );
    let mut catalog = std::collections::BTreeMap::new();
    catalog.insert("Syncing".to_string(), "同步中".to_string());
    assert_eq!(Text::Plain("Syncing".into()).render(&catalog), "同步中");
}

#[test]
fn actions_show_only_where_they_apply() {
    let action: Action = serde_json::from_str(
        r#"{"id":"link","label":"Dropbox Link","menu":"share","command":["./link"],"under":["~/Dropbox/"],"single":true}"#,
    )
    .unwrap();
    let home = Path::new("/home/a");
    let inside = vec![(
        PathBuf::from("/home/a/Dropbox/x.pdf"),
        Some("application/pdf".into()),
        false,
    )];
    let outside = vec![(
        PathBuf::from("/home/a/x.pdf"),
        Some("application/pdf".into()),
        false,
    )];
    assert!(action_shows(&action, None, &inside, 0, Some(home)));
    assert!(!action_shows(&action, None, &outside, 0, Some(home)));
    let two = vec![inside[0].clone(), inside[0].clone()];
    assert!(!action_shows(&action, None, &two, 0, Some(home)), "single");
    assert!(!action_shows(
        &action,
        Some(&["other".to_string()]),
        &inside,
        0,
        Some(home)
    ));
    assert!(applies(
        &[],
        &["image/*".into()],
        &[(PathBuf::from("/a.png"), Some("image/png".into()), false)],
        None
    ));
    assert!(applies(
        &[],
        &["text/plain".into()],
        &[(PathBuf::from("/a.rs"), Some("text/rust".into()), false)],
        None
    ));
    assert!(!applies(
        &[],
        &["image/*".into()],
        &[(PathBuf::from("/d"), None, true)],
        None
    ));
}

#[test]
fn the_host_runs_actions_off_the_window_and_hands_back_their_results() {
    use files_gui::plugin_host::{Outcome, PluginHost};
    let root = root();
    let dir = plugin_dir(
        root.path(),
        "linker",
        r#"{"folio":1,"details":{"command":["./status"]}}"#,
    );
    script(
        &dir,
        "link",
        r#"printf '{"copy":"https://example.invalid/%s"}' "$(basename "$1")""#,
    );
    script(&dir, "status", r#"echo '[["Sync","up to date"]]'"#);
    let plugin = load(&dir, "en", &|_| true).unwrap();
    let host = PluginHost::with(vec![plugin.clone()], "en");
    host.run(
        plugin,
        vec!["./link".into()],
        vec![PathBuf::from("/x/report.pdf")],
        None,
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let outcomes = loop {
        let outcomes = host.take_outcomes();
        if !outcomes.is_empty() {
            break outcomes;
        }
        assert!(Instant::now() < deadline, "the action never finished");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        outcomes,
        [Outcome::Copy("https://example.invalid/report.pdf".into())]
    );

    let file = root.path().join("a.txt");
    fs::write(&file, b"x").unwrap();
    assert!(
        host.details_for(&file, Some("text/plain")).is_empty(),
        "asked, not waited for"
    );
    let rows = loop {
        let rows = host.details_for(&file, Some("text/plain"));
        if !rows.is_empty() {
            break rows;
        }
        assert!(Instant::now() < deadline, "the details never came");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(rows, [("Sync".to_string(), "up to date".to_string())]);
}
