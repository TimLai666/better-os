//! A Better Awake service for screenshots in a sandbox: the real engine and
//! D-Bus interface on the session bus it is started on, with a fake
//! inhibitor, so nothing here reaches logind, and every file and every
//! `/proc` and `/sys` reading under one directory.
//!
//! ```text
//! cp -r crates/hardware-platform/tests/fixtures/held /tmp/awake-held
//! dbus-run-session -- cargo run -p awake-service --features test-support \
//!     --example awake_standin -- /tmp/awake-held
//! ```
//!
//! Point it only at a private session bus (`dbus-run-session`): on the
//! person's own bus it would take Better Awake's name.

use std::sync::Arc;

use awake_service::backend::FakeInhibitorBackend;
use awake_service::{AwakeDbusService, AwakeEngine, BUS_NAME, SystemClock};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("usage: awake_standin <directory>")?;
    let engine = Arc::new(
        AwakeEngine::start_in(
            FakeInhibitorBackend::logind_shaped(),
            &directory,
            Arc::new(SystemClock),
        )
        .await,
    );
    let _connection = awake_service::service::serve(
        zbus::connection::Builder::session()?,
        BUS_NAME,
        AwakeDbusService::new(engine),
    )
    .await?;
    eprintln!(
        "awake_standin: serving {BUS_NAME} from {}",
        directory.display()
    );
    std::future::pending::<()>().await;
    Ok(())
}
