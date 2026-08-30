//! Rejection reasons for untrusted desktop entries.
//!
//! Every variant renders as a stable machine key so a presentation layer can
//! key off the reason without matching on English prose, the same contract
//! `manager-ipc` errors follow.

use thiserror::Error;

/// The largest desktop entry the parser will look at. A `.desktop` file is a
/// small metadata document; anything past this is either corrupt or hostile,
/// and reading it whole would let an attacker choose the catalog's memory use.
pub const MAX_ENTRY_BYTES: usize = 64 * 1024;

/// The longest single value the parser accepts. Names, comments, and list
/// values all share this bound so one oversized field cannot make a record
/// unrenderable.
pub const MAX_VALUE_CHARS: usize = 4096;

/// The most desktop actions one entry may declare. Actions become menu items;
/// an unbounded list is a denial-of-service surface for every consumer.
pub const MAX_ACTIONS: usize = 32;

/// The longest desktop file ID accepted. Filesystem names can be longer than
/// anything a real entry uses.
pub const MAX_DESKTOP_ID_CHARS: usize = 255;

/// Why one desktop entry was rejected. A rejected entry never becomes a record.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum EntryError {
    #[error("catalog.error.invalid_desktop_id:{0}")]
    InvalidDesktopId(String),
    #[error("catalog.error.entry_too_large:{0}")]
    EntryTooLarge(usize),
    #[error("catalog.error.invalid_encoding")]
    InvalidEncoding,
    /// The file exists but could not be read. Discovery records this instead
    /// of silently dropping an entry a user can see on disk.
    #[error("catalog.error.unreadable:{0}")]
    Unreadable(String),
    #[error("catalog.error.content_before_group:{0}")]
    ContentBeforeGroup(usize),
    #[error("catalog.error.invalid_group_header:{0}")]
    InvalidGroupHeader(usize),
    #[error("catalog.error.duplicate_group:{0}")]
    DuplicateGroup(String),
    #[error("catalog.error.invalid_line:{0}")]
    InvalidLine(usize),
    #[error("catalog.error.invalid_key:{0}")]
    InvalidKey(usize),
    #[error("catalog.error.duplicate_key:{group}:{key}")]
    DuplicateKey { group: String, key: String },
    #[error("catalog.error.value_too_long:{0}")]
    ValueTooLong(&'static str),
    #[error("catalog.error.missing_desktop_entry_group")]
    MissingDesktopEntryGroup,
    #[error("catalog.error.missing_field:{0}")]
    MissingField(&'static str),
    #[error("catalog.error.unsupported_type:{0}")]
    UnsupportedType(String),
    #[error("catalog.error.invalid_boolean:{0}")]
    InvalidBoolean(&'static str),
    #[error("catalog.error.control_character:{0}")]
    ControlCharacter(&'static str),
    #[error("catalog.error.too_many_actions:{0}")]
    TooManyActions(usize),
    #[error("catalog.error.invalid_action_id:{0}")]
    InvalidActionId(String),
    #[error("catalog.error.missing_action_group:{0}")]
    MissingActionGroup(String),
    #[error("catalog.error.duplicate_action_id:{0}")]
    DuplicateActionId(String),
    #[error("catalog.error.exec_empty")]
    ExecEmpty,
    #[error("catalog.error.exec_unterminated_quote")]
    ExecUnterminatedQuote,
    #[error("catalog.error.exec_trailing_escape")]
    ExecTrailingEscape,
    #[error("catalog.error.exec_unknown_field_code:{0}")]
    ExecUnknownFieldCode(char),
    #[error("catalog.error.exec_multiple_target_field_codes")]
    ExecMultipleTargetFieldCodes,
    #[error("catalog.error.exec_field_code_placement:{0}")]
    ExecFieldCodePlacement(char),
}

/// Why a launch request could not be turned into an argument vector.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum LaunchError {
    #[error("catalog.launch.error.targets_not_supported:{0}")]
    TargetsNotSupported(usize),
    #[error("catalog.launch.error.relative_path")]
    RelativePath,
    #[error("catalog.launch.error.non_utf8_path")]
    NonUtf8Path,
    #[error("catalog.launch.error.embedded_nul")]
    EmbeddedNul,
    #[error("catalog.launch.error.target_too_long:{0}")]
    TargetTooLong(usize),
    #[error("catalog.launch.error.invalid_uri")]
    InvalidUri,
    #[error("catalog.launch.error.uri_not_a_local_file")]
    UriNotALocalFile,
    #[error("catalog.launch.error.unknown_action:{0}")]
    UnknownAction(String),
    #[error("catalog.launch.error.no_launch_definition")]
    NoLaunchDefinition,
}
