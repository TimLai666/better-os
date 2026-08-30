//! Default-integration declarations, and the typed values they are about.
//!
//! A component manifest may declare which system integrations it wants to own:
//! the default file manager, a global shortcut, an autostart entry. Better
//! Manager never infers these from a component's name, so everything an adapter
//! needs to read, change, verify, and restore a setting is declared here.
//!
//! Two rules run through this module:
//!
//! - A manifest is untrusted input. Every declaration is validated before any
//!   planning happens, in the same style as the rest of the manifest: a closed
//!   enum where the set of acceptable words is known, an explicit rejection
//!   where it is not.
//! - A value is typed, never a shell string. [`DefaultsValue`] is what Better
//!   OS wants a setting to say and [`ObservedValue`] is what the system
//!   actually said, including the cases where the system said nothing useful.
//!   Neither can carry a command.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

use crate::manifest::ManifestError;

/// The longest integration id a declaration may use.
pub const MAX_INTEGRATION_ID_LENGTH: usize = 64;
/// The longest target key an adapter is asked to address, which is generous for
/// a MIME type or a dconf path and short enough to reject a pasted document.
pub const MAX_TARGET_KEY_LENGTH: usize = 255;

/// A stable identifier for one integration inside one component. It is the key
/// a snapshot entry, a plan entry, and a status are all filed under, so it has
/// the same conservative character set as a component id.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntegrationId(String);

impl IntegrationId {
    pub fn new(value: impl Into<String>) -> Result<Self, ManifestError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_INTEGRATION_ID_LENGTH
            || !value.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
            || value.starts_with('-')
            || value.ends_with('-')
        {
            return Err(ManifestError::InvalidIntegrationId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IntegrationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The integration kinds the architecture can represent. The set is closed so a
/// manifest cannot name a kind no adapter has ever heard of, and so a kind
/// without a production adapter is a known gap rather than an unknown word.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrationKind {
    /// Default application for a declared capability, addressed by desktop
    /// entry.
    ApplicationHandler,
    /// A group of MIME types or URI schemes handled together.
    MimeUriHandlerGroup,
    /// A desktop launcher or overview entry point.
    DesktopLauncherEntry,
    /// A global keyboard shortcut.
    GlobalShortcut,
    /// The selected input method.
    InputMethod,
    /// Autostart or session activation.
    Autostart,
    /// User service activation.
    UserService,
    /// A file-manager or system-tool entry point.
    ToolEntryPoint,
    /// A desktop setting specific to the component.
    ComponentDesktopSetting,
}

impl IntegrationKind {
    /// Every kind, so a caller building an adapter set can prove it covered all
    /// of them instead of listing them from memory.
    pub const ALL: [Self; 9] = [
        Self::ApplicationHandler,
        Self::MimeUriHandlerGroup,
        Self::DesktopLauncherEntry,
        Self::GlobalShortcut,
        Self::InputMethod,
        Self::Autostart,
        Self::UserService,
        Self::ToolEntryPoint,
        Self::ComponentDesktopSetting,
    ];
}

/// Whether one integration can have more than one owner at a time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrationExclusivity {
    /// Exactly one component can own it. A second claimant is a conflict.
    Exclusive,
    /// Several components may hold it at once.
    Shared,
}

/// The typed adapter a declaration asks for. The set is closed for the same
/// reason the icon set is: an untrusted manifest must not be able to name an
/// arbitrary executable, path, or command as the thing that changes a setting.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterId {
    /// Reads and writes the user's `mimeapps.list` default application.
    XdgDefaultApp,
    /// Reads the effective XDG default without being able to change it.
    XdgEffectiveDefault,
    /// Reads and verifies a GNOME keybinding.
    GnomeKeybinding,
    /// Reads and verifies a GNOME desktop setting.
    GnomeDesktopSetting,
    /// A desktop launcher or overview entry point.
    DesktopLauncherEntry,
    /// The selected input method.
    InputMethod,
    /// Autostart or session activation.
    SessionAutostart,
    /// User service activation.
    UserService,
    /// A file-manager or system-tool entry point.
    ToolEntryPoint,
}

impl AdapterId {
    /// Whether this adapter is even allowed to be named as an apply adapter. A
    /// read-only adapter named as the thing that applies a change is a manifest
    /// bug, not a runtime surprise.
    pub fn can_apply(self) -> bool {
        !matches!(self, Self::XdgEffectiveDefault)
    }

    /// The kinds this adapter is allowed to serve.
    pub fn serves(self, kind: IntegrationKind) -> bool {
        match self {
            Self::XdgDefaultApp | Self::XdgEffectiveDefault => matches!(
                kind,
                IntegrationKind::ApplicationHandler | IntegrationKind::MimeUriHandlerGroup
            ),
            Self::GnomeKeybinding => kind == IntegrationKind::GlobalShortcut,
            Self::GnomeDesktopSetting => kind == IntegrationKind::ComponentDesktopSetting,
            Self::DesktopLauncherEntry => kind == IntegrationKind::DesktopLauncherEntry,
            Self::InputMethod => kind == IntegrationKind::InputMethod,
            Self::SessionAutostart => kind == IntegrationKind::Autostart,
            Self::UserService => kind == IntegrationKind::UserService,
            Self::ToolEntryPoint => kind == IntegrationKind::ToolEntryPoint,
        }
    }
}

/// What restoring this integration means.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestorePolicy {
    /// Put back the exact value captured before the first change.
    CapturedValue,
    /// Leave whatever is there. Better OS added something additive that does
    /// not displace a previous owner.
    LeaveInPlace,
    /// Restoring cannot be automated and must be described to the user.
    ManualOnly,
}

/// The privilege a change needs. Ordinary desktop defaults are user scope; an
/// integration that declares administrator scope has no executor in this
/// implementation and is reported rather than attempted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequiredPrivilege {
    User,
    Administrator,
}

/// When a change becomes effective.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionEffect {
    Immediate,
    SignOut,
    Restart,
}

/// What has to be true of the component before its integration can be claimed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HealthPrerequisite {
    Installed,
    Enabled,
    Healthy,
}

/// A typed setting value. There is deliberately no variant that can carry a
/// command line: a manifest declares what a setting should say, never what to
/// run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DefaultsValue {
    /// A desktop entry id such as `io.betteros.Files.desktop`.
    DesktopEntry(String),
    Text(String),
    TextList(Vec<String>),
    Boolean(bool),
}

impl DefaultsValue {
    fn is_well_formed(&self) -> bool {
        fn clean(value: &str) -> bool {
            !value.trim().is_empty()
                && value.len() <= MAX_TARGET_KEY_LENGTH
                && !value.chars().any(char::is_control)
        }
        match self {
            Self::DesktopEntry(value) => clean(value) && value.ends_with(".desktop"),
            Self::Text(value) => clean(value),
            Self::TextList(values) => !values.is_empty() && values.iter().all(|v| clean(v)),
            Self::Boolean(_) => true,
        }
    }

    fn matches_kind(&self, kind: IntegrationKind) -> bool {
        match kind {
            IntegrationKind::ApplicationHandler
            | IntegrationKind::MimeUriHandlerGroup
            | IntegrationKind::DesktopLauncherEntry
            | IntegrationKind::InputMethod
            | IntegrationKind::ToolEntryPoint => matches!(self, Self::DesktopEntry(_)),
            IntegrationKind::GlobalShortcut => matches!(self, Self::TextList(_)),
            IntegrationKind::Autostart | IntegrationKind::UserService => {
                matches!(self, Self::Boolean(_))
            }
            // A component's own desktop setting can legitimately be any of the
            // typed shapes, so this is the one kind with no narrower rule.
            IntegrationKind::ComponentDesktopSetting => true,
        }
    }
}

/// What an adapter saw when it read the system. The states are kept apart on
/// purpose: a setting that is unset, a setting an adapter is not allowed to
/// read, and a setting whose effective value cannot be determined are three
/// different facts, and only one of them can be safely overwritten.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ObservedValue {
    /// The system holds this value.
    Set { value: DefaultsValue },
    /// The setting exists and holds nothing. Restoring to this means clearing
    /// it, not writing an empty string.
    Unset,
    /// The effective value cannot be determined safely. `reason` is a stable
    /// machine key; presentation layers own the wording.
    Unknown { reason: String },
    /// No adapter on this system can address this integration at all.
    Unsupported { reason: String },
    /// The adapter was refused access.
    PermissionDenied { reason: String },
}

impl ObservedValue {
    pub fn value(&self) -> Option<&DefaultsValue> {
        match self {
            Self::Set { value } => Some(value),
            _ => None,
        }
    }

    /// Whether this observation is definite enough to compare against a desired
    /// or captured value. An indefinite reading is never treated as agreement.
    pub fn is_determinate(&self) -> bool {
        matches!(self, Self::Set { .. } | Self::Unset)
    }
}

/// The setting a declaration is about, and what Better OS wants it to say.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IntegrationTarget {
    /// The value Better OS wants: the owner it wants registered, or the setting
    /// value it wants stored.
    pub desired: DefaultsValue,
    /// The exact settings the adapter reads and writes — MIME types for an XDG
    /// handler group, a dconf path for a keybinding. Never a command.
    pub keys: Vec<String>,
}

/// One declared default integration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DefaultIntegration {
    pub id: IntegrationId,
    pub kind: IntegrationKind,
    pub exclusivity: IntegrationExclusivity,
    pub target: IntegrationTarget,
    /// Distribution ids this integration applies to.
    pub platforms: Vec<String>,
    /// Desktop sessions this integration applies to.
    pub sessions: Vec<String>,
    pub apply_adapter: AdapterId,
    pub verify_adapter: AdapterId,
    pub restore_policy: RestorePolicy,
    pub privileges: RequiredPrivilege,
    pub session_effect: SessionEffect,
    #[serde(default)]
    pub health_prerequisites: Vec<HealthPrerequisite>,
}

impl DefaultIntegration {
    /// Whether this declaration applies to the running system.
    pub fn applies_to(&self, distribution: &str, session: &str) -> bool {
        self.platforms
            .iter()
            .any(|platform| platform.eq_ignore_ascii_case(distribution))
            && self
                .sessions
                .iter()
                .any(|declared| declared.eq_ignore_ascii_case(session))
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        IntegrationId::new(self.id.0.clone())?;
        let integration = self.id.to_string();

        if self.platforms.is_empty() {
            return Err(ManifestError::MissingIntegrationField("platforms"));
        }
        if self.sessions.is_empty() {
            return Err(ManifestError::MissingIntegrationField("sessions"));
        }
        if self.target.keys.is_empty() {
            return Err(ManifestError::MissingIntegrationField("target.keys"));
        }
        for word in self.platforms.iter().chain(&self.sessions) {
            if word.trim().is_empty() || word.chars().any(char::is_control) {
                return Err(ManifestError::MissingIntegrationField("platforms"));
            }
        }
        let mut seen = HashSet::new();
        for key in &self.target.keys {
            if key.trim().is_empty()
                || key.len() > MAX_TARGET_KEY_LENGTH
                || key.chars().any(char::is_control)
                || !seen.insert(key)
            {
                return Err(ManifestError::InvalidIntegrationTargetKey {
                    integration,
                    key: key.clone(),
                });
            }
        }
        if !self.target.desired.is_well_formed() || !self.target.desired.matches_kind(self.kind) {
            return Err(ManifestError::IntegrationValueMismatch {
                integration,
                kind: self.kind,
            });
        }
        if !self.apply_adapter.can_apply() {
            return Err(ManifestError::ReadOnlyApplyAdapter {
                integration,
                adapter: self.apply_adapter,
            });
        }
        if !self.apply_adapter.serves(self.kind) {
            return Err(ManifestError::IntegrationAdapterMismatch {
                integration,
                adapter: self.apply_adapter,
                kind: self.kind,
            });
        }
        if !self.verify_adapter.serves(self.kind) {
            return Err(ManifestError::IntegrationAdapterMismatch {
                integration,
                adapter: self.verify_adapter,
                kind: self.kind,
            });
        }
        Ok(())
    }
}

/// Validates every declaration in one manifest, including that no integration
/// id is declared twice.
pub(crate) fn validate_declarations(
    integrations: &[DefaultIntegration],
) -> Result<(), ManifestError> {
    let mut seen = HashSet::new();
    for integration in integrations {
        integration.validate()?;
        if !seen.insert(integration.id.clone()) {
            return Err(ManifestError::DuplicateIntegration(integration.id.clone()));
        }
    }
    Ok(())
}
