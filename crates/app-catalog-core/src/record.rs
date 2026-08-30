//! The application identity model and the normalization that produces it.
//!
//! A record is what every consumer of the catalog sees. It is built from an
//! untrusted desktop entry and never carries a value the entry did not
//! actually declare: an unresolvable executable is reported as unresolved
//! rather than guessed, and a sandboxed application reports that no single
//! canonical executable exists at all.

use std::path::{Path, PathBuf};

use crate::entry::{DesktopFile, Group, Locale, LocalizedList, LocalizedText};
use crate::error::{EntryError, LaunchError, MAX_ACTIONS, MAX_DESKTOP_ID_CHARS};
use crate::exec::{ExecLine, ExpansionContext, Invocation, LaunchTarget, TargetAcceptance};

/// The canonical identity of an installed application: its desktop file ID,
/// including the `.desktop` suffix, with directory separators folded to `-` as
/// the specification requires. This, not a path, is what an association or a
/// selection refers to.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DesktopId(String);

impl DesktopId {
    pub fn new(value: impl Into<String>) -> Result<Self, EntryError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.chars().count() <= MAX_DESKTOP_ID_CHARS
            && value.ends_with(".desktop")
            && value.len() > ".desktop".len()
            && !value.starts_with('.')
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '+')
            });
        if !valid {
            return Err(EntryError::InvalidDesktopId(value));
        }
        Ok(Self(value))
    }

    /// Builds the ID for an entry found at `relative` inside an application
    /// directory, folding subdirectories into `-` per the specification.
    pub fn from_relative_path(relative: &Path) -> Result<Self, EntryError> {
        let mut parts = Vec::new();
        for component in relative.components() {
            match component {
                std::path::Component::Normal(part) => {
                    let part = part.to_str().ok_or_else(|| {
                        EntryError::InvalidDesktopId(relative.display().to_string())
                    })?;
                    parts.push(part);
                }
                _ => return Err(EntryError::InvalidDesktopId(relative.display().to_string())),
            }
        }
        if parts.is_empty() {
            return Err(EntryError::InvalidDesktopId(relative.display().to_string()));
        }
        Self::new(parts.join("-"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DesktopId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Whether an entry came from the user's own data directory or from a
/// system-wide one. Consumers that want to distinguish "installed for me" from
/// "installed for everyone" read this instead of comparing path prefixes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryScope {
    User,
    System,
}

/// How the application is packaged, as far as the entry itself reveals. This
/// is evidence for the executable status, not a guess about the runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    /// A normal entry that names a program directly.
    Native,
    Flatpak,
    Snap,
    /// An AppImage that has registered itself with the desktop registry.
    AppImage,
    /// The entry launches through a wrapper such as `sh -c` or `env`.
    Wrapper,
}

/// Where a record came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntrySource {
    pub kind: SourceKind,
    pub scope: EntryScope,
    pub path: PathBuf,
}

/// Why an entry has no single canonical executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoCanonicalExecutable {
    Flatpak,
    Snap,
    AppImage,
    Wrapper,
    DBusActivated,
}

/// The result of trying to name one executable for an application. Issue #4's
/// rule is encoded here: a selection is never silently converted into an
/// unreliable path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableStatus {
    /// A single executable was found on this host at this path.
    Resolved(PathBuf),
    /// The entry names one program, but it was not found.
    Unresolved { program: String },
    /// The entry has no single canonical executable and never will.
    NotApplicable { reason: NoCanonicalExecutable },
}

impl ExecutableStatus {
    /// The path, when one genuinely exists. Every other case is `None` rather
    /// than a fabricated path.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Resolved(path) => Some(path.as_path()),
            _ => None,
        }
    }
}

/// Looks a program name up on the host. Kept as a trait so normalization is
/// testable without the host's `PATH`.
pub trait ExecutableProbe {
    fn resolve(&self, program: &str) -> Option<PathBuf>;
}

/// Resolves nothing. Used where only the entry's own content matters.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProbe;

impl ExecutableProbe for NoProbe {
    fn resolve(&self, _program: &str) -> Option<PathBuf> {
        None
    }
}

/// An icon as the entry declares it. A value containing a separator is a file
/// path; anything else is a name to look up in the icon theme. Consumers must
/// not treat one as the other.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IconReference {
    Name(String),
    Path(PathBuf),
}

/// A MIME type validated to `type/subtype` shape. The catalog does not own a
/// MIME database and does not pretend to.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MimeType(String);

impl MimeType {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let (media, subtype) = value.split_once('/')?;
        let valid = |part: &str| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '+' | '.' | '_' | '*')
                })
        };
        if valid(media) && valid(subtype) {
            Some(Self(value.to_ascii_lowercase()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A secondary launch the entry declares, such as "New Window".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopAction {
    pub id: String,
    pub name: LocalizedText,
    pub icon: Option<IconReference>,
    pub exec: Option<ExecLine>,
}

/// What the entry says about when it should be shown.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VisibilityRules {
    /// `Hidden=true` means the entry is deleted as far as the user is
    /// concerned. It removes the application, it does not merely hide it.
    pub hidden: bool,
    /// `NoDisplay=true` means the application is real but is not a menu item.
    pub no_display: bool,
    pub only_show_in: Vec<String>,
    pub not_show_in: Vec<String>,
    pub try_exec: Option<String>,
    /// Whether `TryExec` was found on this host. `None` when no `TryExec` was
    /// declared.
    pub try_exec_resolved: Option<bool>,
}

/// Why an application is not shown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExclusionReason {
    Hidden,
    NoDisplay,
    /// `OnlyShowIn` names desktops, none of which is the current one.
    NotInOnlyShowIn,
    /// `NotShowIn` names the current desktop.
    ListedInNotShowIn,
    /// `TryExec` names a program that is not installed.
    TryExecMissing,
}

/// The outcome of applying the visibility rules in one environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Visible,
    Excluded(ExclusionReason),
}

impl Visibility {
    pub fn is_visible(self) -> bool {
        matches!(self, Self::Visible)
    }
}

/// The desktop environments a visibility query runs against, taken from
/// `XDG_CURRENT_DESKTOP`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DesktopEnvironments(Vec<String>);

impl DesktopEnvironments {
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(
            names
                .into_iter()
                .map(|name| name.into().trim().to_ascii_uppercase())
                .filter(|name| !name.is_empty())
                .collect(),
        )
    }

    /// Parses the colon-separated `XDG_CURRENT_DESKTOP` form.
    pub fn parse(value: &str) -> Self {
        Self::new(value.split(':'))
    }

    fn contains(&self, name: &str) -> bool {
        let name = name.trim().to_ascii_uppercase();
        self.0.contains(&name)
    }

    pub fn names(&self) -> &[String] {
        &self.0
    }
}

/// What an application can do, as declared rather than as assumed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CapabilityFlags {
    /// `Terminal=true`. Carried so a consumer can run the application in a
    /// terminal instead of quietly dropping it on the floor.
    pub terminal: bool,
    pub dbus_activatable: bool,
    pub startup_notify: bool,
    pub accepts_files: bool,
    pub accepts_uris: bool,
    pub accepts_multiple_targets: bool,
    pub has_actions: bool,
}

/// A part of an entry that was dropped without rejecting the whole entry.
/// Surfacing these keeps a diagnostic view honest about what was ignored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryWarning {
    DroppedMimeType(String),
    DroppedIconPath(String),
    DroppedActionExec { action: String, error: EntryError },
}

/// One installed application, normalized and validated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationRecord {
    pub desktop_id: DesktopId,
    pub name: LocalizedText,
    pub generic_name: Option<LocalizedText>,
    pub comment: Option<LocalizedText>,
    pub icon: Option<IconReference>,
    pub categories: Vec<String>,
    pub keywords: LocalizedList,
    pub mime_types: Vec<MimeType>,
    pub source: EntrySource,
    pub exec: Option<ExecLine>,
    pub dbus_service: Option<String>,
    pub actions: Vec<DesktopAction>,
    pub visibility: VisibilityRules,
    pub executable: ExecutableStatus,
    pub capabilities: CapabilityFlags,
    pub warnings: Vec<EntryWarning>,
}

impl ApplicationRecord {
    /// Normalizes one desktop entry into a record, rejecting anything the
    /// specification does not allow an application entry to be.
    pub fn from_desktop_file(
        desktop_id: DesktopId,
        source_path: PathBuf,
        scope: EntryScope,
        file: &DesktopFile,
        probe: &dyn ExecutableProbe,
    ) -> Result<Self, EntryError> {
        let group = file.desktop_entry()?;
        let entry_type = group
            .value("Type")
            .ok_or(EntryError::MissingField("Type"))?;
        if entry_type != "Application" {
            return Err(EntryError::UnsupportedType(entry_type));
        }
        let name = group
            .localized("Name")
            .filter(|name| !name.default_value().trim().is_empty())
            .ok_or(EntryError::MissingField("Name"))?;

        let dbus_activatable = group.boolean("DBusActivatable")?.unwrap_or(false);
        let exec = match group.value("Exec") {
            Some(value) => Some(ExecLine::parse(&value)?),
            None => None,
        };
        if exec.is_none() && !dbus_activatable {
            return Err(EntryError::MissingField("Exec"));
        }

        let mut warnings = Vec::new();
        let icon = parse_icon(group, &mut warnings);
        let categories = group.list("Categories").unwrap_or_default();
        let keywords = group.localized_list("Keywords").unwrap_or_default();
        let mut mime_types = Vec::new();
        for raw in group.list("MimeType").unwrap_or_default() {
            match MimeType::parse(&raw) {
                Some(mime) => mime_types.push(mime),
                None => warnings.push(EntryWarning::DroppedMimeType(raw)),
            }
        }
        mime_types.sort();
        mime_types.dedup();

        let try_exec = group.value("TryExec");
        let try_exec_resolved = try_exec
            .as_ref()
            .map(|program| resolve_program(program, probe).is_some());
        let visibility = VisibilityRules {
            hidden: group.boolean("Hidden")?.unwrap_or(false),
            no_display: group.boolean("NoDisplay")?.unwrap_or(false),
            only_show_in: group.list("OnlyShowIn").unwrap_or_default(),
            not_show_in: group.list("NotShowIn").unwrap_or_default(),
            try_exec,
            try_exec_resolved,
        };

        let actions = parse_actions(file, group, &mut warnings)?;
        let kind = detect_source_kind(group, exec.as_ref());
        let acceptance = exec
            .as_ref()
            .map(ExecLine::acceptance)
            .unwrap_or(TargetAcceptance::None);
        let capabilities = CapabilityFlags {
            terminal: group.boolean("Terminal")?.unwrap_or(false),
            dbus_activatable,
            startup_notify: group.boolean("StartupNotify")?.unwrap_or(false),
            accepts_files: acceptance.accepts_files(),
            // A D-Bus activated entry with no `Exec` is opened through the
            // `org.freedesktop.Application.Open` method, which takes URIs and
            // takes any number of them.
            accepts_uris: acceptance.accepts_uris() || (dbus_activatable && exec.is_none()),
            accepts_multiple_targets: acceptance.accepts_multiple()
                || (dbus_activatable && exec.is_none()),
            has_actions: !actions.is_empty(),
        };
        let dbus_service =
            dbus_activatable.then(|| desktop_id.as_str().trim_end_matches(".desktop").to_string());
        let executable = resolve_executable(
            kind,
            dbus_activatable,
            exec.as_ref(),
            visibility.try_exec.as_deref(),
            probe,
        );

        Ok(Self {
            desktop_id,
            name,
            generic_name: group.localized("GenericName"),
            comment: group.localized("Comment"),
            icon,
            categories,
            keywords,
            mime_types,
            source: EntrySource {
                kind,
                scope,
                path: source_path,
            },
            exec,
            dbus_service,
            actions,
            visibility,
            executable,
            capabilities,
            warnings,
        })
    }

    /// Applies every visibility rule in one environment. The order matters:
    /// `Hidden` outranks everything because it means the entry was deleted.
    pub fn visibility_in(&self, environments: &DesktopEnvironments) -> Visibility {
        let rules = &self.visibility;
        if rules.hidden {
            return Visibility::Excluded(ExclusionReason::Hidden);
        }
        if rules.try_exec_resolved == Some(false) {
            return Visibility::Excluded(ExclusionReason::TryExecMissing);
        }
        if !rules.only_show_in.is_empty()
            && !rules
                .only_show_in
                .iter()
                .any(|name| environments.contains(name))
        {
            return Visibility::Excluded(ExclusionReason::NotInOnlyShowIn);
        }
        if rules
            .not_show_in
            .iter()
            .any(|name| environments.contains(name))
        {
            return Visibility::Excluded(ExclusionReason::ListedInNotShowIn);
        }
        if rules.no_display {
            return Visibility::Excluded(ExclusionReason::NoDisplay);
        }
        Visibility::Visible
    }

    /// The localized display name for a locale.
    pub fn display_name(&self, locale: Option<&Locale>) -> &str {
        self.name.resolve(locale)
    }

    pub fn supports_mime_type(&self, mime: &MimeType) -> bool {
        self.mime_types.contains(mime)
    }

    pub fn action(&self, id: &str) -> Option<&DesktopAction> {
        self.actions.iter().find(|action| action.id == id)
    }

    /// Builds the argument vectors for launching this record, optionally
    /// through one of its declared actions. This is the only supported way to
    /// turn a record plus targets into something executable.
    pub fn build_invocations(
        &self,
        action_id: Option<&str>,
        targets: &[LaunchTarget],
        locale: Option<&Locale>,
    ) -> Result<Vec<Invocation>, LaunchError> {
        let exec = match action_id {
            Some(id) => self
                .action(id)
                .ok_or_else(|| LaunchError::UnknownAction(id.to_string()))?
                .exec
                .as_ref(),
            None => self.exec.as_ref(),
        }
        .ok_or(LaunchError::NoLaunchDefinition)?;
        for target in targets {
            target.check(exec.acceptance())?;
        }
        let icon = match &self.icon {
            Some(IconReference::Name(name)) => Some(name.as_str()),
            Some(IconReference::Path(_)) | None => None,
        };
        exec.build(
            targets,
            &ExpansionContext {
                icon,
                display_name: self.display_name(locale),
                source_path: &self.source.path,
            },
        )
    }
}

fn parse_icon(group: &Group, warnings: &mut Vec<EntryWarning>) -> Option<IconReference> {
    let value = group.value("Icon")?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return None;
    }
    if value.contains('/') {
        let path = PathBuf::from(&value);
        if path.is_absolute() {
            return Some(IconReference::Path(path));
        }
        warnings.push(EntryWarning::DroppedIconPath(value));
        return None;
    }
    Some(IconReference::Name(value))
}

fn parse_actions(
    file: &DesktopFile,
    group: &Group,
    warnings: &mut Vec<EntryWarning>,
) -> Result<Vec<DesktopAction>, EntryError> {
    let ids = group.list("Actions").unwrap_or_default();
    if ids.len() > MAX_ACTIONS {
        return Err(EntryError::TooManyActions(ids.len()));
    }
    let mut actions: Vec<DesktopAction> = Vec::with_capacity(ids.len());
    for id in ids {
        let valid = !id.is_empty()
            && id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-');
        if !valid {
            return Err(EntryError::InvalidActionId(id));
        }
        if actions.iter().any(|action| action.id == id) {
            return Err(EntryError::DuplicateActionId(id));
        }
        let action_group = file
            .group(&format!("Desktop Action {id}"))
            .ok_or_else(|| EntryError::MissingActionGroup(id.clone()))?;
        let name = action_group
            .localized("Name")
            .filter(|name| !name.default_value().trim().is_empty())
            .ok_or(EntryError::MissingField("Action Name"))?;
        let exec = match action_group.value("Exec") {
            Some(value) => match ExecLine::parse(&value) {
                Ok(line) => Some(line),
                Err(error) => {
                    // A broken action must not delete a working application.
                    warnings.push(EntryWarning::DroppedActionExec {
                        action: id.clone(),
                        error,
                    });
                    None
                }
            },
            None => None,
        };
        actions.push(DesktopAction {
            id,
            name,
            icon: parse_icon(action_group, warnings),
            exec,
        });
    }
    Ok(actions)
}

/// Programs known to launch something else. An entry going through one of
/// these has no single canonical executable of its own.
const WRAPPER_PROGRAMS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "env",
    "gio",
    "xdg-open",
    "kioclient",
    "systemd-run",
    "dbus-run-session",
    "gtk-launch",
];

fn detect_source_kind(group: &Group, exec: Option<&ExecLine>) -> SourceKind {
    if group.has_key_prefix("X-Flatpak") {
        return SourceKind::Flatpak;
    }
    if group.has_key_prefix("X-Snap") {
        return SourceKind::Snap;
    }
    if group.has_key_prefix("X-AppImage") {
        return SourceKind::AppImage;
    }
    let Some(exec) = exec else {
        return SourceKind::Native;
    };
    let program = exec.program();
    let base = program.rsplit('/').next().unwrap_or(program);
    if base == "flatpak" {
        return SourceKind::Flatpak;
    }
    if base == "snap" || program.starts_with("/snap/") {
        return SourceKind::Snap;
    }
    if program.ends_with(".AppImage") {
        return SourceKind::AppImage;
    }
    if WRAPPER_PROGRAMS.contains(&base) {
        return SourceKind::Wrapper;
    }
    SourceKind::Native
}

fn resolve_program(program: &str, probe: &dyn ExecutableProbe) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    probe.resolve(program)
}

fn resolve_executable(
    kind: SourceKind,
    dbus_activatable: bool,
    exec: Option<&ExecLine>,
    try_exec: Option<&str>,
    probe: &dyn ExecutableProbe,
) -> ExecutableStatus {
    let reason = match kind {
        SourceKind::Flatpak => Some(NoCanonicalExecutable::Flatpak),
        SourceKind::Snap => Some(NoCanonicalExecutable::Snap),
        SourceKind::AppImage => Some(NoCanonicalExecutable::AppImage),
        SourceKind::Wrapper => Some(NoCanonicalExecutable::Wrapper),
        SourceKind::Native => None,
    };
    if let Some(reason) = reason {
        return ExecutableStatus::NotApplicable { reason };
    }
    let Some(exec) = exec else {
        return ExecutableStatus::NotApplicable {
            reason: NoCanonicalExecutable::DBusActivated,
        };
    };
    if dbus_activatable {
        return ExecutableStatus::NotApplicable {
            reason: NoCanonicalExecutable::DBusActivated,
        };
    }
    let candidate = try_exec.unwrap_or_else(|| exec.program());
    match resolve_program(candidate, probe) {
        Some(path) => ExecutableStatus::Resolved(path),
        None => ExecutableStatus::Unresolved {
            program: candidate.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeProbe {
        known: HashMap<String, PathBuf>,
    }

    impl FakeProbe {
        fn with(programs: &[(&str, &str)]) -> Self {
            Self {
                known: programs
                    .iter()
                    .map(|(name, path)| ((*name).to_string(), PathBuf::from(path)))
                    .collect(),
            }
        }
    }

    impl ExecutableProbe for FakeProbe {
        fn resolve(&self, program: &str) -> Option<PathBuf> {
            self.known.get(program).cloned()
        }
    }

    fn record_with(
        text: &str,
        probe: &dyn ExecutableProbe,
    ) -> Result<ApplicationRecord, EntryError> {
        let file = DesktopFile::parse(text)?;
        ApplicationRecord::from_desktop_file(
            DesktopId::new("editor.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/editor.desktop"),
            EntryScope::System,
            &file,
            probe,
        )
    }

    fn record(text: &str) -> Result<ApplicationRecord, EntryError> {
        record_with(text, &NoProbe)
    }

    #[test]
    fn desktop_id_rules() {
        assert!(DesktopId::new("org.gnome.Nautilus.desktop").is_ok());
        assert!(DesktopId::new("editor").is_err());
        assert!(DesktopId::new(".desktop").is_err());
        assert!(DesktopId::new("a/b.desktop").is_err());
        assert_eq!(
            DesktopId::from_relative_path(Path::new("kde4/konsole.desktop"))
                .unwrap()
                .as_str(),
            "kde4-konsole.desktop"
        );
        assert!(DesktopId::from_relative_path(Path::new("../escape.desktop")).is_err());
    }

    #[test]
    fn carries_every_required_field() {
        let record = record_with(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Editor\n\
             Name[zh_TW]=編輯器\n\
             GenericName=Text Editor\n\
             Comment=Edit text\n\
             Icon=text-editor\n\
             Categories=Utility;TextEditor;\n\
             Keywords=text;write;\n\
             MimeType=text/plain;text/markdown;\n\
             Exec=editor %F\n\
             TryExec=editor\n\
             Terminal=false\n\
             StartupNotify=true\n\
             Actions=NewWindow;\n\
             \n\
             [Desktop Action NewWindow]\n\
             Name=New Window\n\
             Exec=editor --new-window\n",
            &FakeProbe::with(&[("editor", "/usr/bin/editor")]),
        )
        .unwrap();

        assert_eq!(record.desktop_id.as_str(), "editor.desktop");
        assert_eq!(record.display_name(None), "Editor");
        assert_eq!(
            record.display_name(Locale::parse("zh_TW").as_ref()),
            "編輯器"
        );
        assert_eq!(
            record.generic_name.as_ref().unwrap().default_value(),
            "Text Editor"
        );
        assert_eq!(
            record.comment.as_ref().unwrap().default_value(),
            "Edit text"
        );
        assert_eq!(record.icon, Some(IconReference::Name("text-editor".into())));
        assert_eq!(record.categories, vec!["Utility", "TextEditor"]);
        assert_eq!(record.keywords.default_value(), ["text", "write"]);
        assert_eq!(
            record
                .mime_types
                .iter()
                .map(MimeType::as_str)
                .collect::<Vec<_>>(),
            vec!["text/markdown", "text/plain"]
        );
        assert_eq!(record.exec.as_ref().unwrap().program(), "editor");
        assert_eq!(record.visibility.try_exec.as_deref(), Some("editor"));
        assert_eq!(record.visibility.try_exec_resolved, Some(true));
        assert_eq!(
            record.executable,
            ExecutableStatus::Resolved(PathBuf::from("/usr/bin/editor"))
        );
        assert_eq!(record.actions.len(), 1);
        assert_eq!(record.actions[0].id, "NewWindow");
        assert!(record.capabilities.has_actions);
        assert!(record.capabilities.startup_notify);
        assert!(!record.capabilities.terminal);
        assert_eq!(
            record.source,
            EntrySource {
                kind: SourceKind::Native,
                scope: EntryScope::System,
                path: PathBuf::from("/usr/share/applications/editor.desktop"),
            }
        );
    }

    #[test]
    fn a_terminal_entry_carries_the_flag_instead_of_being_dropped() {
        let record =
            record("[Desktop Entry]\nType=Application\nName=Htop\nExec=htop\nTerminal=true\n")
                .unwrap();
        assert!(record.capabilities.terminal);
        assert!(
            record
                .visibility_in(&DesktopEnvironments::default())
                .is_visible()
        );
    }

    #[test]
    fn rejects_a_non_application_type() {
        assert_eq!(
            record("[Desktop Entry]\nType=Link\nName=Site\nURL=https://example.com\n").unwrap_err(),
            EntryError::UnsupportedType("Link".to_string())
        );
        assert_eq!(
            record("[Desktop Entry]\nName=No Type\nExec=x\n").unwrap_err(),
            EntryError::MissingField("Type")
        );
    }

    #[test]
    fn rejects_a_missing_name_or_exec() {
        assert_eq!(
            record("[Desktop Entry]\nType=Application\nExec=editor\n").unwrap_err(),
            EntryError::MissingField("Name")
        );
        assert_eq!(
            record("[Desktop Entry]\nType=Application\nName=   \nExec=editor\n").unwrap_err(),
            EntryError::MissingField("Name")
        );
        assert_eq!(
            record("[Desktop Entry]\nType=Application\nName=Editor\n").unwrap_err(),
            EntryError::MissingField("Exec")
        );
    }

    #[test]
    fn a_dbus_activatable_entry_may_omit_exec_and_reports_no_executable() {
        let file = DesktopFile::parse(
            "[Desktop Entry]\nType=Application\nName=Files\nDBusActivatable=true\n",
        )
        .unwrap();
        let record = ApplicationRecord::from_desktop_file(
            DesktopId::new("org.gnome.Nautilus.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/org.gnome.Nautilus.desktop"),
            EntryScope::System,
            &file,
            &NoProbe,
        )
        .unwrap();
        assert!(record.capabilities.dbus_activatable);
        assert_eq!(record.dbus_service.as_deref(), Some("org.gnome.Nautilus"));
        assert_eq!(
            record.executable,
            ExecutableStatus::NotApplicable {
                reason: NoCanonicalExecutable::DBusActivated
            }
        );
        assert_eq!(record.executable.path(), None);
    }

    #[test]
    fn sandboxed_and_wrapped_entries_never_get_a_fabricated_path() {
        let probe = FakeProbe::with(&[
            ("flatpak", "/usr/bin/flatpak"),
            ("snap", "/usr/bin/snap"),
            ("sh", "/bin/sh"),
        ]);
        let cases = [
            (
                "Exec=/usr/bin/flatpak run --branch=stable org.gimp.GIMP",
                SourceKind::Flatpak,
                NoCanonicalExecutable::Flatpak,
            ),
            (
                "Exec=/snap/bin/chromium",
                SourceKind::Snap,
                NoCanonicalExecutable::Snap,
            ),
            (
                "Exec=/home/user/Apps/Tool-x86_64.AppImage",
                SourceKind::AppImage,
                NoCanonicalExecutable::AppImage,
            ),
            (
                "Exec=sh -c \"exec editor\"",
                SourceKind::Wrapper,
                NoCanonicalExecutable::Wrapper,
            ),
        ];
        for (exec, kind, reason) in cases {
            let record = record_with(
                &format!("[Desktop Entry]\nType=Application\nName=App\n{exec}\n"),
                &probe,
            )
            .unwrap();
            assert_eq!(record.source.kind, kind, "kind for {exec}");
            assert_eq!(
                record.executable,
                ExecutableStatus::NotApplicable { reason },
                "status for {exec}"
            );
            assert_eq!(record.executable.path(), None, "path for {exec}");
        }
    }

    #[test]
    fn an_unresolvable_program_is_reported_not_invented() {
        let record =
            record("[Desktop Entry]\nType=Application\nName=Ghost\nExec=ghost-editor %f\n")
                .unwrap();
        assert_eq!(
            record.executable,
            ExecutableStatus::Unresolved {
                program: "ghost-editor".to_string()
            }
        );
        assert_eq!(record.executable.path(), None);
    }

    #[test]
    fn hidden_removes_the_application_entirely() {
        let record =
            record("[Desktop Entry]\nType=Application\nName=Gone\nExec=gone\nHidden=true\n")
                .unwrap();
        assert_eq!(
            record.visibility_in(&DesktopEnvironments::parse("GNOME")),
            Visibility::Excluded(ExclusionReason::Hidden)
        );
    }

    #[test]
    fn no_display_excludes_only_from_display() {
        let record =
            record("[Desktop Entry]\nType=Application\nName=Helper\nExec=helper\nNoDisplay=true\n")
                .unwrap();
        assert_eq!(
            record.visibility_in(&DesktopEnvironments::parse("GNOME")),
            Visibility::Excluded(ExclusionReason::NoDisplay)
        );
        assert!(record.visibility.no_display);
        assert!(!record.visibility.hidden);
    }

    #[test]
    fn only_show_in_excludes_other_desktops() {
        let record = record(
            "[Desktop Entry]\nType=Application\nName=KDE Thing\nExec=kthing\nOnlyShowIn=KDE;\n",
        )
        .unwrap();
        assert_eq!(
            record.visibility_in(&DesktopEnvironments::parse("GNOME")),
            Visibility::Excluded(ExclusionReason::NotInOnlyShowIn)
        );
        assert!(
            record
                .visibility_in(&DesktopEnvironments::parse("KDE:X-Cinnamon"))
                .is_visible()
        );
    }

    #[test]
    fn not_show_in_excludes_the_current_desktop() {
        let record = record(
            "[Desktop Entry]\nType=Application\nName=Not GNOME\nExec=thing\nNotShowIn=GNOME;\n",
        )
        .unwrap();
        assert_eq!(
            record.visibility_in(&DesktopEnvironments::parse("ubuntu:GNOME")),
            Visibility::Excluded(ExclusionReason::ListedInNotShowIn)
        );
        assert!(
            record
                .visibility_in(&DesktopEnvironments::parse("KDE"))
                .is_visible()
        );
    }

    #[test]
    fn a_missing_try_exec_excludes_the_entry() {
        let record = record(
            "[Desktop Entry]\nType=Application\nName=Maybe\nExec=maybe\nTryExec=maybe-not\n",
        )
        .unwrap();
        assert_eq!(record.visibility.try_exec_resolved, Some(false));
        assert_eq!(
            record.visibility_in(&DesktopEnvironments::default()),
            Visibility::Excluded(ExclusionReason::TryExecMissing)
        );

        let record = record_with(
            "[Desktop Entry]\nType=Application\nName=Maybe\nExec=maybe\nTryExec=maybe\n",
            &FakeProbe::with(&[("maybe", "/usr/bin/maybe")]),
        )
        .unwrap();
        assert!(
            record
                .visibility_in(&DesktopEnvironments::default())
                .is_visible()
        );
    }

    #[test]
    fn try_exec_wins_over_exec_for_executable_resolution() {
        let record = record_with(
            "[Desktop Entry]\nType=Application\nName=App\nExec=app --flag\nTryExec=/opt/app/bin/app\n",
            &FakeProbe::with(&[("/opt/app/bin/app", "/opt/app/bin/app")]),
        )
        .unwrap();
        assert_eq!(
            record.executable,
            ExecutableStatus::Resolved(PathBuf::from("/opt/app/bin/app"))
        );
    }

    #[test]
    fn malformed_mime_types_are_dropped_not_fatal() {
        let record = record(
            "[Desktop Entry]\nType=Application\nName=App\nExec=app\nMimeType=text/plain;nonsense;text/;\n",
        )
        .unwrap();
        assert_eq!(
            record
                .mime_types
                .iter()
                .map(MimeType::as_str)
                .collect::<Vec<_>>(),
            vec!["text/plain"]
        );
        assert!(
            record
                .warnings
                .contains(&EntryWarning::DroppedMimeType("nonsense".into()))
        );
    }

    #[test]
    fn an_action_without_its_group_is_rejected() {
        assert_eq!(
            record("[Desktop Entry]\nType=Application\nName=App\nExec=app\nActions=Missing;\n")
                .unwrap_err(),
            EntryError::MissingActionGroup("Missing".to_string())
        );
    }

    #[test]
    fn an_action_id_is_validated() {
        assert_eq!(
            record("[Desktop Entry]\nType=Application\nName=App\nExec=app\nActions=../evil;\n")
                .unwrap_err(),
            EntryError::InvalidActionId("../evil".to_string())
        );
    }

    #[test]
    fn too_many_actions_are_rejected() {
        let ids: Vec<String> = (0..MAX_ACTIONS + 1)
            .map(|index| format!("a{index}"))
            .collect();
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=App\nExec=app\nActions={};\n",
            ids.join(";")
        );
        assert_eq!(
            record(&text).unwrap_err(),
            EntryError::TooManyActions(MAX_ACTIONS + 1)
        );
    }

    #[test]
    fn an_action_is_launchable_by_id() {
        let record = record(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Browser\n\
             Exec=browser %U\n\
             Actions=NewWindow;\n\
             \n\
             [Desktop Action NewWindow]\n\
             Name=New Window\n\
             Exec=browser --new-window\n",
        )
        .unwrap();
        let invocations = record
            .build_invocations(Some("NewWindow"), &[], None)
            .unwrap();
        assert_eq!(invocations[0].program, "browser");
        assert_eq!(invocations[0].arguments, vec!["--new-window"]);
        assert_eq!(
            record
                .build_invocations(Some("Nope"), &[], None)
                .unwrap_err(),
            LaunchError::UnknownAction("Nope".to_string())
        );
    }

    #[test]
    fn a_relative_icon_path_is_dropped_with_a_warning() {
        let record =
            record("[Desktop Entry]\nType=Application\nName=App\nExec=app\nIcon=./icons/app.png\n")
                .unwrap();
        assert_eq!(record.icon, None);
        assert!(
            record
                .warnings
                .contains(&EntryWarning::DroppedIconPath("./icons/app.png".into()))
        );
    }

    #[test]
    fn an_absolute_icon_path_stays_a_path() {
        let record = record(
            "[Desktop Entry]\nType=Application\nName=App\nExec=app\nIcon=/usr/share/pixmaps/app.png\n",
        )
        .unwrap();
        assert_eq!(
            record.icon,
            Some(IconReference::Path(PathBuf::from(
                "/usr/share/pixmaps/app.png"
            )))
        );
    }
}
