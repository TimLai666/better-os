//! What Keep awake reads from Better Awake's service and the one kind of
//! change it asks for, over the session bus (`org.betteros.Awake1`). Plain
//! blocking calls, made off the UI thread; what the answers mean is decided
//! in `decide.rs` and `model.rs`.
//!
//! The page never holds an inhibitor and never runs a command: every change
//! is one `awake-ipc` request, and what the page shows next is what the
//! service reports afterwards.

use std::sync::mpsc;

use awake_ipc::{
    AwakeRequest, AwakeResponse, HistoryDocument, MAX_HISTORY_PAGE, QuickSessionsDocument,
    RequestBody, ResponseBody, RuleTestDocument, RulesDocument, StatusDocument,
};
use zbus::blocking::Connection;

use super::decide::ReadError;

pub(crate) const BUS_NAME: &str = "org.betteros.Awake1";
pub(crate) const OBJECT_PATH: &str = "/org/betteros/Awake1";
pub(crate) const INTERFACE: &str = "org.betteros.Awake1";

/// Everything the page shows, read at once.
#[derive(Clone, Debug)]
pub(crate) struct Read {
    /// The service's `ProtocolVersion`, read fresh each time: a service an
    /// update restarted says a new number without telling anyone.
    pub(crate) version: Option<u32>,
    pub(crate) status: Result<StatusDocument, ReadError>,
    pub(crate) rules: Option<RulesDocument>,
    pub(crate) history: Option<HistoryDocument>,
    pub(crate) offer: Option<QuickSessionsDocument>,
}

/// Where the service is: its bus name on a connection.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) connection: Connection,
    pub(crate) name: String,
}

impl Target {
    /// The session bus and the service's own name.
    pub(crate) fn session() -> Result<Self, String> {
        Connection::session()
            .map(|connection| Self {
                connection,
                name: BUS_NAME.to_string(),
            })
            .map_err(|error| error.to_string())
    }
}

/// A bus error that means nobody is there to answer.
fn not_on_bus(error: &zbus::Error) -> bool {
    let text = error.to_string();
    [
        "ServiceUnknown",
        "NameHasNoOwner",
        "Spawn.",
        "NoReply",
        "Timeout",
        "TimedOut",
    ]
    .iter()
    .any(|name| text.contains(name))
}

fn classify(error: zbus::Error) -> ReadError {
    if not_on_bus(&error) {
        ReadError::NotOnBus(error.to_string())
    } else {
        ReadError::Failed(error.to_string())
    }
}

/// Reads the version property, uncached.
pub(crate) fn protocol_version(target: &Target) -> Result<u32, ReadError> {
    let reply = target
        .connection
        .call_method(
            Some(target.name.as_str()),
            OBJECT_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(INTERFACE, "ProtocolVersion"),
        )
        .map_err(classify)?;
    let value: zbus::zvariant::OwnedValue = reply
        .body()
        .deserialize()
        .map_err(|error| ReadError::Failed(error.to_string()))?;
    u32::try_from(&value).map_err(|error| ReadError::Failed(error.to_string()))
}

/// Sends one request and reads its reply. The page speaks only the current
/// protocol: an older service is "too old" and offered an update instead
/// (ADR 0017), so nothing here falls back.
pub(crate) fn exchange(target: &Target, body: RequestBody) -> Result<ResponseBody, ReadError> {
    exchange_in(target, body, awake_ipc::PROTOCOL_VERSION)
}

/// [`exchange`] in an older version's shape. Used only to read an older
/// service's status, so the page can say which version answered and offer
/// the update.
fn exchange_in(
    target: &Target,
    body: RequestBody,
    version: u32,
) -> Result<ResponseBody, ReadError> {
    let request = AwakeRequest::new(body)
        .to_json_for(version)
        .map_err(|error| ReadError::Failed(error.to_string()))?;
    let reply = target
        .connection
        .call_method(
            Some(target.name.as_str()),
            OBJECT_PATH,
            Some(INTERFACE),
            "Request",
            &(request.as_str(),),
        )
        .map_err(classify)?;
    let document: String = reply
        .body()
        .deserialize()
        .map_err(|error| ReadError::Failed(error.to_string()))?;
    AwakeResponse::from_json_any(&document)
        .map(|response| response.body)
        .map_err(|error| ReadError::Failed(error.to_string()))
}

/// Reads the status, the rules, the history, and the offer. Asking for the
/// status first is what starts the service through D-Bus activation when it
/// is installed and not running.
pub(crate) fn read(target: &Target) -> Read {
    let status = match exchange(target, RequestBody::QueryStatus) {
        Ok(ResponseBody::Status(status)) => Ok(*status),
        Ok(ResponseBody::Rejected { error_key }) => Err(ReadError::Failed(error_key)),
        Ok(other) => Err(ReadError::Failed(format!("unexpected reply: {other:?}"))),
        Err(error) => Err(error),
    };
    let version = protocol_version(target).ok();
    // An older service refuses the current version; its status is read in
    // its own, so the page shows "too old" rather than "not running".
    let status = match (status, version) {
        (Err(ReadError::Failed(_)), Some(older))
            if older < awake_ipc::PROTOCOL_VERSION
                && awake_ipc::ACCEPTED_PROTOCOL_VERSIONS.contains(&older) =>
        {
            match exchange_in(target, RequestBody::QueryStatus, older) {
                Ok(ResponseBody::Status(status)) => Ok(*status),
                Ok(other) => Err(ReadError::Failed(format!("unexpected reply: {other:?}"))),
                Err(error) => Err(error),
            }
        }
        (status, _) => status,
    };
    if status.is_err() || version.is_some_and(|v| v < awake_ipc::PROTOCOL_VERSION) {
        return Read {
            version,
            status,
            rules: None,
            history: None,
            offer: None,
        };
    }
    let rules = match exchange(target, RequestBody::QueryRules) {
        Ok(ResponseBody::Rules(rules)) => Some(*rules),
        _ => None,
    };
    let history = match exchange(
        target,
        RequestBody::QueryHistory {
            limit: MAX_HISTORY_PAGE,
        },
    ) {
        Ok(ResponseBody::History(history)) => Some(*history),
        _ => None,
    };
    let offer = match exchange(target, RequestBody::QueryQuickSessions) {
        Ok(ResponseBody::QuickSessions(offer)) => Some(*offer),
        _ => None,
    };
    Read {
        version,
        status,
        rules,
        history,
        offer,
    }
}

/// What a change came back with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Done,
    /// A stable key from the service.
    Refused(String),
    Failed(String),
}

pub(crate) fn perform(target: &Target, body: RequestBody) -> Outcome {
    match exchange(target, body) {
        Ok(ResponseBody::Rejected { error_key }) => Outcome::Refused(error_key),
        Ok(_) => Outcome::Done,
        Err(error) => Outcome::Failed(error.details().to_string()),
    }
}

pub(crate) fn test_rule(target: &Target, rule_id: u64) -> Result<RuleTestDocument, Outcome> {
    match exchange(target, RequestBody::TestRule { rule_id }) {
        Ok(ResponseBody::RuleTest(test)) => Ok(*test),
        Ok(ResponseBody::Rejected { error_key }) => Err(Outcome::Refused(error_key)),
        Ok(other) => Err(Outcome::Failed(format!("unexpected reply: {other:?}"))),
        Err(error) => Err(Outcome::Failed(error.details().to_string())),
    }
}

/// Follows the service: every event it pushes, every new offer, and every
/// time its name gets or loses an owner calls `changed`, so the page reads
/// again instead of polling. Runs until the connection closes or `changed`
/// says nobody is listening any more.
pub(crate) fn follow(target: Target, mut changed: impl FnMut() -> bool) {
    let rules = [
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(target.name.as_str())
            .and_then(|rule| rule.interface(INTERFACE))
            .map(|rule| rule.build()),
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|rule| rule.interface("org.freedesktop.DBus"))
            .and_then(|rule| rule.member("NameOwnerChanged"))
            .and_then(|rule| rule.add_arg(target.name.as_str()))
            .map(|rule| rule.build()),
    ];
    let mut streams = Vec::new();
    for rule in rules.into_iter().flatten() {
        if let Ok(iterator) =
            zbus::blocking::MessageIterator::for_match_rule(rule, &target.connection, Some(16))
        {
            streams.push(iterator);
        }
    }
    let (merged, events) = mpsc::channel::<()>();
    for stream in streams {
        let merged = merged.clone();
        std::thread::spawn(move || {
            for message in stream {
                if message.is_err() || merged.send(()).is_err() {
                    return;
                }
            }
        });
    }
    drop(merged);
    while events.recv().is_ok() {
        if !changed() {
            return;
        }
    }
}
