//! The two root writes, through `manager-daemon` (ADR 0017, spec 11.2).

use hardware_core::fan::FanMode;
use hardware_core::write::{WriteOutcome, classify_error, classify_reply};
use zbus::blocking::Connection;

pub const BUS_NAME: &str = "org.betteros.Manager1";
pub const OBJECT_PATH: &str = "/org/betteros/Manager1";
pub const INTERFACE: &str = "org.betteros.Hardware1";

fn outcome<T>(reply: zbus::Result<zbus::Message>, asked: &str) -> WriteOutcome
where
    T: for<'de> serde::Deserialize<'de> + zbus::zvariant::Type + ToString,
{
    match reply {
        Ok(message) => match message.body().deserialize::<T>() {
            Ok(value) => classify_reply(asked, &value.to_string()),
            Err(error) => WriteOutcome::Failed {
                details: error.to_string(),
            },
        },
        Err(zbus::Error::MethodError(name, message, _)) => {
            classify_error(name.as_str(), message.as_deref().unwrap_or(""))
        }
        Err(error) => WriteOutcome::Failed {
            details: error.to_string(),
        },
    }
}

/// Asks for a charge limit of 60, 80, or 100. polkit may put up its prompt,
/// so this can take as long as the person takes.
pub fn set_charge_limit(system: &Connection, percent: u8) -> WriteOutcome {
    let reply = system.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "SetChargeLimit",
        &(percent,),
    );
    outcome::<u8>(reply, &percent.to_string())
}

pub fn set_fan_mode(system: &Connection, mode: FanMode) -> WriteOutcome {
    let reply = system.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "SetFanMode",
        &(mode.wire(),),
    );
    outcome::<String>(reply, mode.wire())
}
