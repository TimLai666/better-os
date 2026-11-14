//! The Better Awake tray client.
//!
//! It connects to the service, publishes a StatusNotifierItem, and verifies
//! that the watcher really registered it. When it did not, that is said
//! plainly and never reported as a showing icon; the tray then waits for a
//! watcher to appear and registers with it, so a panel that starts after the
//! tray, or restarts, still gets the icon.

use std::sync::Arc;

use awake_tray::client::{CALL_TIMEOUT, FIRST_CALL_TIMEOUT, ServiceClient};
use awake_tray::controller::TrayController;
use awake_tray::labels::Locale;
use awake_tray::notify::DesktopNotifier;
use awake_tray::sni::{TrayAvailability, keep_registered, register_and_verify};

fn report(availability: TrayAvailability) {
    match availability {
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
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The first call may be what D-Bus-activates the service at login, which
    // can take longer than an ordinary answer, so it alone is given
    // FIRST_CALL_TIMEOUT, on a connection of its own.
    let first = zbus::connection::Builder::session()?
        .method_timeout(FIRST_CALL_TIMEOUT)
        .build()
        .await?;
    let first_status = ServiceClient::with_connection(first).await?.status().await;
    let status = match first_status {
        Ok(status) => status,
        Err(error) => {
            // An error exit, so the user unit starts the tray again.
            eprintln!("better-awake-tray: the Better Awake service is not answering: {error}");
            return Err(error.into());
        }
    };

    // Every call the tray makes after that gives up after CALL_TIMEOUT, so a
    // lost reply can neither hang a menu click nor the loop that follows the
    // service. This connection talks to the service, follows its signals, and
    // owns the low-battery notification name.
    let service_connection = zbus::connection::Builder::session()?
        .method_timeout(CALL_TIMEOUT)
        .build()
        .await?;
    let client = ServiceClient::with_connection(service_connection.clone()).await?;

    let locale = Locale::from_environment();
    let controller = Arc::new(TrayController::new(client, locale, status));

    // The item and the menu are served from the builder; `serve` says why.
    let connection = awake_tray::serve(
        zbus::connection::Builder::session()?.method_timeout(CALL_TIMEOUT),
        controller.clone(),
    )
    .await?;

    // A missing notification service costs the notifications and nothing
    // else, so it is said and the tray starts anyway. One notifier says a
    // refused menu action; the other is the low-battery one, which only the
    // tray that claims it raises.
    match DesktopNotifier::connect(&connection, locale).await {
        Ok(notifier) => controller.attach_notifier(notifier).await,
        Err(error) => {
            eprintln!("better-awake-tray: desktop notifications are unavailable: {error}")
        }
    }
    let low_battery = DesktopNotifier::connect(&connection, locale).await.ok();

    let unique_name = connection
        .unique_name()
        .map(|name| name.to_string())
        .unwrap_or_default();
    // A watcher that appears is a panel that started or restarted: it has
    // no submenu of ours open, so a layout held for one goes out.
    let host_controller = controller.clone();
    let registration = tokio::spawn(keep_registered(
        connection.clone(),
        unique_name.clone(),
        move |availability| {
            report(availability);
            let controller = host_controller.clone();
            tokio::spawn(async move { controller.host_changed().await });
        },
    ));
    report(register_and_verify(&connection, &unique_name).await);

    // Follow the service rather than polling it, so an idle tray costs nothing:
    // no timer, no busy loop, and no countdown recomputed when nobody is
    // looking at the menu.
    let follower = tokio::spawn(
        controller
            .clone()
            .follow_service(service_connection, low_battery),
    );

    tokio::select! {
        () = controller.quit_requested() => {}
        result = follower => {
            // The service stream only ends when the connection does. An error
            // exit lets the user unit start a tray on a working connection.
            registration.abort();
            return match result {
                Ok(Ok(())) => Err("the session bus connection ended".into()),
                Ok(Err(error)) => Err(error.into()),
                Err(error) => Err(error.into()),
            };
        }
    }
    registration.abort();
    Ok(())
}
