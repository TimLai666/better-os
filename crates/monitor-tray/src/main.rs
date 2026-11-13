//! `better-monitor-tray`: Better Monitor in the taskbar.
//!
//! It owns `org.betteros.MonitorTray1` so the window can tell it is running,
//! publishes the item, verifies the watcher listed it, and then asks the
//! service for a summary once a second. A watcher that is missing is said
//! plainly and the process exits, as Better Awake's tray does: an invisible
//! item that reports success is the failure mode to avoid.

use std::time::Duration;

use monitor_service::MonitorClient;
use monitor_tray::menu::{IconChoice, Locale, ServiceState, TrayReadings};
use monitor_tray::{
    DbusMenu, ITEM_PATH, MENU_PATH, StatusNotifierItem, Tray, TrayAvailability, TrayState,
    language_path, register_and_verify,
};

/// How often the readings are asked for. The icon is redrawn at most this
/// often, and only when the drawing changed.
const POLL: Duration = Duration::from_secs(1);

/// How often, and how many times, the item's bus name is asked for again
/// while an older copy lets go of it.
const NAME_RETRY: Duration = Duration::from_millis(500);
const NAME_ATTEMPTS: u32 = 10;

/// How long to wait before asking again after the service did not answer.
const RETRY: Duration = Duration::from_secs(5);

/// Say what happened to a registration attempt. Without a watcher the item
/// keeps waiting for one, so that is a note rather than an error.
fn report(availability: TrayAvailability) {
    match availability {
        TrayAvailability::Registered => {
            eprintln!("better-monitor-tray: the taskbar item is showing")
        }
        TrayAvailability::NoWatcher => eprintln!(
            "better-monitor-tray: no taskbar to show in yet; waiting for one (on GNOME this \
             needs the AppIndicator extension; Better Monitor's window works without it)"
        ),
        other => eprintln!("better-monitor-tray: the taskbar did not take the item ({other:?})"),
    }
}

fn print_help() {
    println!(
        "better-monitor-tray — Better Monitor 的工作列圖示 / Better Monitor in the taskbar\n\n\
         用法 / Usage:\n  better-monitor-tray            啟動 / start\n  \
         better-monitor-tray --version  顯示版本 / print the version"
    );
}

#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(argument) = std::env::args().nth(1) {
        match argument.as_str() {
            "--version" | "-V" => {
                println!("better-monitor-tray {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => {
                eprintln!("better-monitor-tray: unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }

    let connection = zbus::Connection::session().await?;
    let stored = language_path().and_then(|path| std::fs::read_to_string(path).ok());
    let lang = std::env::var("LANGUAGE")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("LC_ALL").ok().filter(|v| !v.is_empty()))
        .or_else(|| std::env::var("LANG").ok());
    let locale = Locale::resolve(stored.as_deref(), lang.as_deref());
    let choice_path = IconChoice::default_path();
    let choice = choice_path
        .as_deref()
        .map(IconChoice::load)
        .unwrap_or_default();
    let tray = Tray::new(TrayState::new(locale, choice, choice_path));

    connection
        .object_server()
        .at(ITEM_PATH, StatusNotifierItem::new(tray.clone()))
        .await?;
    connection
        .object_server()
        .at(MENU_PATH, DbusMenu::new(tray.clone()))
        .await?;
    tray.attach(connection.clone()).await;

    // One item per session. A copy that is still exiting (the one the unit's
    // ExecStartPre just ended, or one being replaced by an update) may hold the
    // name for a moment, so it is asked for again for a few seconds; a name
    // that stays taken means another item is really running, and this one
    // fails so systemd can say so.
    let mut attempts = 0;
    loop {
        match connection
            .request_name_with_flags(
                monitor_ipc::TRAY_BUS_NAME,
                zbus::fdo::RequestNameFlags::DoNotQueue.into(),
            )
            .await
        {
            Ok(zbus::fdo::RequestNameReply::PrimaryOwner)
            | Ok(zbus::fdo::RequestNameReply::AlreadyOwner) => break,
            _ if attempts < NAME_ATTEMPTS => {
                attempts += 1;
                tokio::time::sleep(NAME_RETRY).await;
            }
            _ => {
                eprintln!(
                    "better-monitor-tray: another taskbar item still holds {} after {} seconds",
                    monitor_ipc::TRAY_BUS_NAME,
                    (NAME_RETRY * NAME_ATTEMPTS).as_secs()
                );
                std::process::exit(1);
            }
        }
    }

    let unique = connection
        .unique_name()
        .map(|name| name.to_string())
        .unwrap_or_default();
    // The watcher belongs to the panel. At login the panel may come up after
    // this item, and GNOME Shell or swaybar restarting brings a new watcher
    // that has never heard of it. So the item never exits for want of a
    // watcher: it watches for one and registers each time one appears. The
    // watch is set up before the first attempt, so a watcher that appears in
    // between is not missed.
    let rewatch = {
        let watch =
            monitor_tray::keep_registered(connection.clone(), unique.clone(), report).await?;
        tokio::spawn(watch)
    };
    report(register_and_verify(&connection, &unique).await);

    let poller = tray.clone();
    let polling = tokio::spawn(async move {
        let mut client: Option<MonitorClient> = None;
        loop {
            let mut refused: Option<(ServiceState, String)> = None;
            if client.is_none() {
                client = MonitorClient::with_connection(connection.clone())
                    .await
                    .ok();
                // A service of another protocol version would fail to parse:
                // it is an update waiting for a restart, and is said so.
                if let Some(c) = &client
                    && let Ok(version) = c.protocol_version().await
                    && version != monitor_ipc::PROTOCOL_VERSION
                {
                    client = None;
                    refused = Some((
                        ServiceState::Outdated,
                        format!(
                            "the service speaks protocol {version}, this item {}",
                            monitor_ipc::PROTOCOL_VERSION
                        ),
                    ));
                }
            }
            let answer = match (&client, refused) {
                (_, Some(refused)) => Err(refused),
                // No processes: the item names none.
                (Some(c), None) => match c.summary(0).await {
                    Ok(status) => status
                        .latest_round
                        .as_deref()
                        .map(TrayReadings::from_round)
                        .ok_or((ServiceState::Unavailable, "no round yet".to_string())),
                    Err(error) => {
                        client = None;
                        Err((ServiceState::Unavailable, error.to_string()))
                    }
                },
                (None, None) => Err((
                    ServiceState::Unavailable,
                    "no connection to the service".to_string(),
                )),
            };
            let answered = answer.is_ok();
            let detail = answer.as_ref().err().map(|(_, detail)| detail.clone());
            let (changes, new_state) = poller
                .state
                .lock()
                .await
                .adopt_from(answer.map_err(|(state, _)| state));
            // Said once when it starts failing, and once when it recovers.
            if new_state {
                match detail {
                    Some(detail) => {
                        eprintln!("better-monitor-tray: the service is not answering: {detail}")
                    }
                    None => eprintln!("better-monitor-tray: the service is answering again"),
                }
            }
            poller.announce(changes).await;
            tokio::time::sleep(if answered { POLL } else { RETRY }).await;
        }
    });

    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tray.quit.notified() => {}
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
    polling.abort();
    rewatch.abort();
    Ok(())
}
