//! The Better Awake tray client.
//!
//! It connects to the service, publishes a StatusNotifierItem, and verifies
//! that the watcher really registered it. If it did not, that is said plainly
//! and the process exits: an invisible tray that reports success is the failure
//! mode Issue #13 names explicitly.

use std::sync::Arc;

use awake_tray::client::ServiceClient;
use awake_tray::controller::TrayController;
use awake_tray::dbusmenu::DbusMenu;
use awake_tray::item::StatusNotifierItem;
use awake_tray::labels::Locale;
use awake_tray::notify::{DesktopNotifier, handle_event};
use awake_tray::sni::{ITEM_PATH, MENU_PATH, TrayAvailability, register_and_verify};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = zbus::Connection::session().await?;

    let client = ServiceClient::with_connection(connection.clone()).await?;
    let status = match client.status().await {
        Ok(status) => status,
        Err(error) => {
            eprintln!("better-awake-tray: the Better Awake service is not answering: {error}");
            return Err(error.into());
        }
    };

    let locale = Locale::from_environment();
    let controller = Arc::new(TrayController::new(client, locale, status));

    connection
        .object_server()
        .at(ITEM_PATH, StatusNotifierItem::new(controller.clone()))
        .await?;
    connection
        .object_server()
        .at(MENU_PATH, DbusMenu::new(controller.clone()))
        .await?;
    controller.attach(connection.clone()).await;

    let unique_name = connection
        .unique_name()
        .map(|name| name.to_string())
        .unwrap_or_default();
    match register_and_verify(&connection, &unique_name).await {
        TrayAvailability::Registered => {}
        other => {
            eprintln!(
                "better-awake-tray: the tray icon is not showing ({}); open Better Awake from the \
                 applications menu instead",
                other.as_key()
            );
            if let Some(remedy) = other.remedy_key() {
                eprintln!("better-awake-tray: {remedy}");
            }
            return Ok(());
        }
    }

    // A missing notification service costs the low-battery notification and
    // nothing else, so it is said and the tray starts anyway.
    let notifier = match DesktopNotifier::connect(&connection, locale).await {
        Ok(notifier) => Some(notifier),
        Err(error) => {
            eprintln!("better-awake-tray: desktop notifications are unavailable: {error}");
            None
        }
    };

    // Follow the service rather than polling it, so an idle tray costs nothing:
    // no timer, no busy loop, and no countdown recomputed when nobody is
    // looking at the menu.
    let updates = controller.clone();
    let watcher = tokio::spawn(async move {
        use zbus::export::futures_core::Stream;

        let Ok(client) = ServiceClient::connect().await else {
            return;
        };
        let Ok(stream) = client.status_updates().await else {
            return;
        };
        let mut stream = std::pin::pin!(stream);
        while let Some(signal) =
            std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
        {
            let Ok(args) = signal.args() else { continue };
            if let Some(status) = handle_event(args.event_json(), notifier.as_ref()).await {
                updates.set_status(status).await;
            }
        }
    });

    controller.quit_requested().await;
    watcher.abort();
    Ok(())
}
