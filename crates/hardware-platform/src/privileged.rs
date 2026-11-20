//! The root writes, through `manager-daemon` (ADR 0017, spec 11.2): the
//! charge limit, the fan mode, and the keyboard's colour (ADR 0034).

use hardware_core::fan::FanMode;
use hardware_core::lighting::{Access, LightingChoice, LightingReply};
use hardware_core::write::keys;
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

/// What a refusal from the daemon means for the keyboard colour card.
pub fn lighting_access(error: &zbus::Error) -> Access {
    match error {
        zbus::Error::MethodError(name, message, _) => {
            let message = message.as_deref().unwrap_or("");
            let key = message.split(':').next().unwrap_or("").trim();
            match key {
                keys::AUTH_CANCELLED => Access::Cancelled,
                keys::UNAUTHORIZED => Access::NotAllowed,
                keys::HARDWARE_ABSENT => Access::Absent,
                _ if ["ServiceUnknown", "NameHasNoOwner"]
                    .iter()
                    .any(|n| name.as_str().ends_with(n)) =>
                {
                    Access::NoService
                }
                // A daemon that answers without these methods is older than
                // the window: it needs updating, it is not missing.
                _ if ["UnknownMethod", "UnknownInterface", "UnknownObject"]
                    .iter()
                    .any(|n| name.as_str().ends_with(n)) =>
                {
                    Access::Outdated
                }
                _ => Access::Failed(format!("{}: {message}", name.as_str())),
            }
        }
        // No system bus, or the call never reached a daemon.
        _ => Access::NoService,
    }
}

fn reply(message: zbus::Message) -> Result<LightingReply, Access> {
    let text: String = message
        .body()
        .deserialize()
        .map_err(|e| Access::Failed(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| Access::Failed(format!("{e}: {text}")))
}

/// Asks the daemon what the keyboard's colour interface is, what its lamps
/// can show, and what Better OS set. The daemon reads a LampArray through
/// its helper unit, so this can take a moment the first time.
pub fn keyboard_lighting(system: &Connection) -> Result<LightingReply, Access> {
    let message = system
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "KeyboardLighting",
            &(),
        )
        .map_err(|e| lighting_access(&e))?;
    reply(message)
}

/// Sets the keyboard's colour and effect, and returns what the daemon then
/// describes.
pub fn set_keyboard_lighting(
    system: &Connection,
    choice: &LightingChoice,
) -> (WriteOutcome, Option<LightingReply>) {
    let c = choice.colour;
    let call = system.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "SetKeyboardLighting",
        &(
            choice.effect.wire(),
            c.red,
            c.green,
            c.blue,
            choice.intensity,
            choice.speed.wire(),
        ),
    );
    match call {
        Ok(message) => match reply(message) {
            Ok(reply) => (WriteOutcome::Done, Some(reply)),
            Err(Access::Failed(details)) => (WriteOutcome::Failed { details }, None),
            Err(other) => (
                WriteOutcome::Failed {
                    details: format!("{other:?}"),
                },
                None,
            ),
        },
        Err(zbus::Error::MethodError(name, message, _)) => (
            classify_error(name.as_str(), message.as_deref().unwrap_or("")),
            None,
        ),
        Err(error) => (
            WriteOutcome::Failed {
                details: error.to_string(),
            },
            None,
        ),
    }
}
