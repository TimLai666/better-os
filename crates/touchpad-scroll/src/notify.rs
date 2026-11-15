//! `sd_notify` without libsystemd: readiness and the watchdog heartbeat.
//!
//! The heartbeat is what turns a hang into a restart. The unit sets
//! `WatchdogSec=`, the event loop sends `WATCHDOG=1` from the same thread that
//! forwards events, and a loop that stops turning stops the heartbeat, so
//! systemd kills the process; the kernel then ends the grab and the clone with
//! its file descriptors.

use std::os::unix::net::UnixDatagram;

/// Send one notification. Without `NOTIFY_SOCKET` (not started by systemd) it
/// does nothing.
pub fn notify(message: &str) -> std::io::Result<()> {
    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else {
        return Ok(());
    };
    let socket = UnixDatagram::unbound()?;
    let bytes = path.as_encoded_bytes();
    if let Some(name) = bytes.strip_prefix(b"@") {
        use std::os::linux::net::SocketAddrExt;
        let address = std::os::unix::net::SocketAddr::from_abstract_name(name)?;
        socket.send_to_addr(message.as_bytes(), &address)?;
    } else {
        socket.send_to(message.as_bytes(), &path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_notification_reaches_the_socket_systemd_names() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notify");
        let receiver = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
        // SAFETY: this test is the only one that touches NOTIFY_SOCKET.
        unsafe { std::env::set_var("NOTIFY_SOCKET", &path) };
        super::notify("WATCHDOG=1").unwrap();
        unsafe { std::env::remove_var("NOTIFY_SOCKET") };
        let mut buffer = [0u8; 32];
        let length = receiver.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"WATCHDOG=1");
    }
}
