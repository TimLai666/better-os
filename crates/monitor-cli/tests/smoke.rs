//! All five subcommands, end to end, against a disposable store.
//!
//! Everything runs `--offline`, so the developer's own service is never
//! consulted and the test never depends on one being installed. That also
//! exercises the branch that matters most for honesty: what the CLI says when
//! nothing is recording.

use std::path::Path;

use clap::Parser;
use monitor_cli::{Cli, run};

async fn cli(store: &Path, arguments: &[&str]) -> String {
    let mut all = vec![
        "better-monitor",
        "--offline",
        "--store",
        store.to_str().unwrap(),
    ];
    all.extend_from_slice(arguments);
    let parsed = Cli::try_parse_from(all).expect("a parseable command line");
    run(&parsed).await.expect("a successful subcommand")
}

async fn cli_error(store: &Path, arguments: &[&str]) -> String {
    let mut all = vec![
        "better-monitor",
        "--offline",
        "--store",
        store.to_str().unwrap(),
    ];
    all.extend_from_slice(arguments);
    let parsed = Cli::try_parse_from(all).expect("a parseable command line");
    run(&parsed).await.expect_err("a refusal").to_string()
}

#[tokio::test]
async fn inspect_on_an_empty_store_says_nothing_is_recording() {
    let store = tempfile::tempdir().unwrap();
    let output = cli(store.path(), &["inspect"]).await;
    assert!(output.contains("the service is not running"));
    assert!(output.contains("stored samples   0"));
    // An empty store is not a machine that was idle. It is a machine nobody
    // watched, and the output must not read as the first.
    assert!(!output.contains("recording        yes"));
}

#[tokio::test]
async fn record_writes_history_and_the_other_subcommands_read_it() {
    let store = tempfile::tempdir().unwrap();

    // A one-second recording against the real machine. Short on purpose: this
    // proves the path, not the collectors, which have their own fixtures.
    let recorded = cli(
        store.path(),
        &["record", "--seconds", "1", "--resolution", "0"],
    )
    .await;
    assert!(recorded.contains("recorded"));
    assert!(recorded.contains("command lines    not collected"));
    assert!(store.path().join(monitor_store::HISTORY_FILE_NAME).exists());

    let inspected = cli(store.path(), &["inspect", "--last", "600"]).await;
    assert!(
        !inspected.contains("stored samples   0"),
        "record wrote nothing that inspect could see:\n{inspected}"
    );

    let doctored = cli(store.path(), &["doctor"]).await;
    assert!(doctored.contains("service          not consulted"));
    assert!(doctored.contains("schema version"));
    assert!(doctored.contains("What needs attention"));
}

#[tokio::test]
async fn mark_records_an_incident_and_says_where_it_came_from() {
    let store = tempfile::tempdir().unwrap();
    cli(
        store.path(),
        &["record", "--seconds", "1", "--resolution", "0"],
    )
    .await;

    let marked = cli(
        store.path(),
        &[
            "mark",
            "--note",
            "the system was just slow",
            "--before",
            "60",
            "--after",
            "30",
        ],
    )
    .await;
    assert!(marked.contains("incident         1"));
    assert!(marked.contains("note             the system was just slow"));
    assert!(marked.contains("window           60 s before, 30 s after"));
    assert!(marked.contains("the service is not running"));

    let inspected = cli(store.path(), &["inspect"]).await;
    assert!(inspected.contains("incidents        1"));
}

#[tokio::test]
async fn export_previews_before_it_writes_and_writes_a_whole_package() {
    let store = tempfile::tempdir().unwrap();
    cli(
        store.path(),
        &["record", "--seconds", "1", "--resolution", "0"],
    )
    .await;
    cli(store.path(), &["mark", "--note", "slow"]).await;

    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join("package");

    let previewed = cli(
        store.path(),
        &["export", "--to", destination.to_str().unwrap(), "--preview"],
    )
    .await;
    assert!(previewed.contains("nothing was written"));
    assert!(!destination.exists());

    let written = cli(
        store.path(),
        &["export", "--to", destination.to_str().unwrap()],
    )
    .await;
    assert!(written.contains("Nothing was uploaded"));
    assert!(written.contains("redaction-report.json"));
    for name in [
        "manifest.json",
        "inventory.json",
        "samples.jsonl",
        "incidents.json",
        "coverage.json",
        "collector-health.json",
        "redaction-report.json",
        "README.txt",
    ] {
        assert!(destination.join(name).exists(), "missing {name}");
    }
    assert!(
        destination
            .join("schema")
            .join("sample.schema.json")
            .exists()
    );

    // Exporting into the same directory twice would merge two packages into
    // one and produce a manifest that lies about its own contents.
    let refused = cli_error(
        store.path(),
        &["export", "--to", destination.to_str().unwrap()],
    )
    .await;
    assert!(refused.contains("destination_exists"));
}

#[tokio::test]
async fn json_output_is_machine_readable_for_every_reading_subcommand() {
    let store = tempfile::tempdir().unwrap();
    cli(
        store.path(),
        &["record", "--seconds", "1", "--resolution", "0"],
    )
    .await;

    for arguments in [vec!["inspect"], vec!["doctor"]] {
        let mut all = vec![
            "better-monitor",
            "--offline",
            "--json",
            "--store",
            store.path().to_str().unwrap(),
        ];
        all.extend_from_slice(&arguments);
        let parsed = Cli::try_parse_from(all).unwrap();
        let output = run(&parsed).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap_or_else(|error| {
            panic!("{arguments:?} did not produce JSON: {error}\n{output}")
        });
        assert!(parsed.is_object());
    }
}

#[tokio::test]
async fn the_store_the_commands_share_is_the_one_that_was_named() {
    let store = tempfile::tempdir().unwrap();
    cli(
        store.path(),
        &["record", "--seconds", "1", "--resolution", "0"],
    )
    .await;
    for name in [
        monitor_store::HISTORY_FILE_NAME,
        monitor_store::INCIDENTS_FILE_NAME,
        monitor_store::INVENTORY_FILE_NAME,
    ] {
        assert!(
            store.path().join(name).exists(),
            "{name} was not created in the store that was named"
        );
    }
}
