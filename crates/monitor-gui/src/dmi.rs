use monitor_ipc::{MemoryReport, PROTOCOL_VERSION};
use zbus::blocking::{Connection, Proxy};

const BUS_NAME: &str = "org.betteros.Monitor1";
const OBJECT_PATH: &str = "/org/betteros/Monitor1";
const INTERFACE: &str = "org.betteros.Monitor1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DmiClientError {
    Denied,
    Unavailable(String),
    Protocol(String),
}

pub fn read_memory_report() -> Result<MemoryReport, DmiClientError> {
    let connection =
        Connection::system().map_err(|error| DmiClientError::Unavailable(error.to_string()))?;
    let proxy = Proxy::new(&connection, BUS_NAME, OBJECT_PATH, INTERFACE)
        .map_err(|error| DmiClientError::Unavailable(error.to_string()))?;
    let version = proxy
        .get_property::<u32>("ProtocolVersion")
        .map_err(classify)?;
    if version != PROTOCOL_VERSION {
        return Err(DmiClientError::Protocol(format!(
            "monitor.ipc.protocol:{version}:{PROTOCOL_VERSION}"
        )));
    }
    let document: String = proxy.call("ReadMemoryDevices", &()).map_err(classify)?;
    MemoryReport::from_json(&document).map_err(|error| DmiClientError::Protocol(error.to_string()))
}

fn classify(error: zbus::Error) -> DmiClientError {
    let text = error.to_string();
    if text.contains("daemon.error.unauthorized")
        || text.contains("org.freedesktop.DBus.Error.AccessDenied")
    {
        return DmiClientError::Denied;
    }
    if text.contains("ServiceUnknown")
        || text.contains("NameHasNoOwner")
        || text.contains("was not provided by any .service files")
    {
        return DmiClientError::Unavailable(text);
    }
    DmiClientError::Protocol(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_keep_denied_and_missing_service_distinct() {
        assert_eq!(
            classify(zbus::Error::Failure(
                "daemon.error.unauthorized".to_string()
            )),
            DmiClientError::Denied
        );
        assert!(matches!(
            classify(zbus::Error::Failure(
                "org.freedesktop.DBus.Error.ServiceUnknown".to_string()
            )),
            DmiClientError::Unavailable(_)
        ));
    }
}
