//! Turning a process list into applications, with the reason attached.
//!
//! The rule this module exists to enforce is that a group is a claim, and a
//! claim needs evidence. Two processes are only put in the same application
//! when something the kernel or the desktop actually recorded says they belong
//! together: the same systemd user unit, the same Flatpak or Snap identity,
//! the same desktop application launch, or a direct parent-child link.
//!
//! Matching executable names are explicitly *not* such evidence. Two shells in
//! two different terminals, two `python3` processes belonging to two unrelated
//! tools, and two copies of a service started by different users are all
//! separate things, and merging them would make the CPU total of "python3"
//! meaningless. Executable identity survives here only as a way to *name* a
//! group that has exactly one process in it.
//!
//! ## Precedence is configurable, not decided
//!
//! Issue #16 lists an evidence order and ticket 23 records that the final
//! precedence and confidence thresholds still need an ADR. So the order lives
//! in [`GroupingPrecedence`] as data, with the issue's order as the default,
//! and every group records which rule produced it. Changing the order later is
//! a configuration change and a test, not a rewrite.
//!
//! ## Source
//!
//! cgroup path shapes are read from systemd's `systemd.special(7)` slice
//! layout and from `Desktop Entry` launch behaviour: an application launched
//! by a desktop shell lands in `app-<launcher>-<AppID>-<random>.scope` under
//! `app.slice`, Flatpak in `app-flatpak-<AppID>-<pid>.scope`, and Snap in
//! `snap.<snap>.<app>.<uuid>.scope`. Nothing here executes systemd or reads a
//! D-Bus interface; it interprets the `/proc/[pid]/cgroup` string ticket 22
//! already collects.

use crate::facts::ProcessFacts;
use std::collections::{BTreeMap, HashMap, HashSet};

/// How much a piece of evidence is worth trusting.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Confidence {
    /// The system itself records the relationship.
    High,
    /// The relationship is inferred from a structure the system maintains.
    Medium,
    /// Nothing groups this process with anything; it stands alone.
    Low,
}

/// The kinds of evidence, in the order Issue #16 lists them.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EvidenceKind {
    SystemdUnit,
    Flatpak,
    Snap,
    DesktopApplication,
    Ancestry,
    ExecutableIdentity,
}

impl EvidenceKind {
    pub fn key(self) -> &'static str {
        match self {
            EvidenceKind::SystemdUnit => "systemd-unit",
            EvidenceKind::Flatpak => "flatpak",
            EvidenceKind::Snap => "snap",
            EvidenceKind::DesktopApplication => "desktop-application",
            EvidenceKind::Ancestry => "ancestry",
            EvidenceKind::ExecutableIdentity => "executable-identity",
        }
    }
}

/// Why these processes are one application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GroupingEvidence {
    /// Same systemd unit, from the cgroup v2 path.
    SystemdUnit { unit: String, path: String },
    /// Same Flatpak application identity.
    Flatpak { app_id: String, unit: String },
    /// Same Snap.
    Snap {
        snap: String,
        app: Option<String>,
        unit: String,
    },
    /// Same desktop application id, from the scope a desktop shell created.
    DesktopApplication { app_id: String, unit: String },
    /// A direct descendant of a process that is already in this group, used
    /// only where no cgroup identity was readable.
    Ancestry { parent_pid: u32, root_pid: u32 },
    /// Nothing relates this process to any other. The executable name is used
    /// to label it, never to merge it.
    ExecutableIdentity { executable: String },
    /// The cgroup was unreadable and the process has no usable parent.
    Unattributed { detail: String },
}

impl GroupingEvidence {
    pub fn kind(&self) -> Option<EvidenceKind> {
        match self {
            GroupingEvidence::SystemdUnit { .. } => Some(EvidenceKind::SystemdUnit),
            GroupingEvidence::Flatpak { .. } => Some(EvidenceKind::Flatpak),
            GroupingEvidence::Snap { .. } => Some(EvidenceKind::Snap),
            GroupingEvidence::DesktopApplication { .. } => Some(EvidenceKind::DesktopApplication),
            GroupingEvidence::Ancestry { .. } => Some(EvidenceKind::Ancestry),
            GroupingEvidence::ExecutableIdentity { .. } => Some(EvidenceKind::ExecutableIdentity),
            GroupingEvidence::Unattributed { .. } => None,
        }
    }

    pub fn confidence(&self) -> Confidence {
        match self {
            GroupingEvidence::Flatpak { .. }
            | GroupingEvidence::Snap { .. }
            | GroupingEvidence::SystemdUnit { .. }
            | GroupingEvidence::DesktopApplication { .. } => Confidence::High,
            GroupingEvidence::Ancestry { .. } => Confidence::Medium,
            GroupingEvidence::ExecutableIdentity { .. } | GroupingEvidence::Unattributed { .. } => {
                Confidence::Low
            }
        }
    }
}

/// Which evidence kinds are used, and in what order.
///
/// Removing a kind is how a deployment says "do not trust this signal here",
/// and the order decides which of two applicable kinds names the group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupingPrecedence(Vec<EvidenceKind>);

impl GroupingPrecedence {
    pub fn new(order: Vec<EvidenceKind>) -> Self {
        Self(order)
    }

    pub fn order(&self) -> &[EvidenceKind] {
        &self.0
    }

    pub fn allows(&self, kind: EvidenceKind) -> bool {
        self.0.contains(&kind)
    }

    fn rank(&self, kind: EvidenceKind) -> Option<usize> {
        self.0.iter().position(|candidate| *candidate == kind)
    }

    /// The better of two applicable kinds, by this precedence.
    fn preferred(&self, left: EvidenceKind, right: EvidenceKind) -> EvidenceKind {
        match (self.rank(left), self.rank(right)) {
            (Some(l), Some(r)) if l <= r => left,
            (Some(_), Some(_)) => right,
            (Some(_), None) => left,
            (None, Some(_)) => right,
            (None, None) => left,
        }
    }
}

impl Default for GroupingPrecedence {
    /// The order Issue #16 states, pending the ADR that ticket 23 defers.
    fn default() -> Self {
        Self(vec![
            EvidenceKind::SystemdUnit,
            EvidenceKind::Flatpak,
            EvidenceKind::Snap,
            EvidenceKind::DesktopApplication,
            EvidenceKind::Ancestry,
            EvidenceKind::ExecutableIdentity,
        ])
    }
}

/// Whether a group is something the user launched or something the system
/// runs on its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppKind {
    UserApplication,
    BackgroundService,
}

/// One process's place in a group, and why it is there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberAttribution {
    pub pid: u32,
    pub evidence: GroupingEvidence,
}

/// One application, or one background service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppGroup {
    /// Stable across rounds for the same application, so a selected row does
    /// not jump when the table refreshes.
    pub key: String,
    pub display_name: String,
    pub kind: AppKind,
    /// The evidence that defines the group, which is the strongest evidence
    /// any member carries.
    pub evidence: GroupingEvidence,
    pub members: Vec<MemberAttribution>,
}

impl AppGroup {
    pub fn confidence(&self) -> Confidence {
        self.evidence.confidence()
    }

    pub fn pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.members.iter().map(|member| member.pid)
    }

    pub fn contains(&self, pid: u32) -> bool {
        self.members.iter().any(|member| member.pid == pid)
    }
}

/// Every group one round produced.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Grouping {
    pub applications: Vec<AppGroup>,
    pub services: Vec<AppGroup>,
}

impl Grouping {
    pub fn all(&self) -> impl Iterator<Item = &AppGroup> {
        self.applications.iter().chain(self.services.iter())
    }

    pub fn group_of(&self, pid: u32) -> Option<&AppGroup> {
        self.all().find(|group| group.contains(pid))
    }

    pub fn len(&self) -> usize {
        self.applications.len() + self.services.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What a cgroup path said about one process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CgroupIdentity {
    /// The last path segment, the unit or scope name.
    pub unit: String,
    /// The whole path, kept so a group can show where it came from.
    pub path: String,
    pub flatpak_app_id: Option<String>,
    pub snap: Option<(String, Option<String>)>,
    pub desktop_app_id: Option<String>,
    /// Whether the unit sits under `app.slice`, which is where a desktop
    /// session puts things a person launched.
    pub in_app_slice: bool,
    /// Whether the unit sits under `system.slice`.
    pub in_system_slice: bool,
}

/// systemd escapes characters that a unit name cannot carry. `\x2d` for `-`
/// is the one that appears in practice, in application ids that contain a
/// hyphen.
fn unescape_unit(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() && bytes[index + 1] == b'x' {
            if let Ok(code) = u8::from_str_radix(&raw[index + 2..index + 4], 16) {
                out.push(code as char);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

/// Drop the trailing `-<digits>` a desktop launcher appends to make the scope
/// name unique. Without this every launch of one application would be a
/// different app id.
fn strip_launch_suffix(raw: &str) -> &str {
    match raw.rsplit_once('-') {
        Some((head, tail)) if !head.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => raw,
    }
}

/// Launchers systemd names between `app-` and the application id.
const LAUNCHERS: [&str; 6] = ["gnome", "kde", "xfce", "mate", "cinnamon", "budgie"];

/// Read the identity out of a `/proc/[pid]/cgroup` path.
///
/// Returns `None` for a path with no unit, such as the root cgroup a kernel
/// thread reports.
pub fn parse_cgroup_identity(path: &str) -> Option<CgroupIdentity> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return None;
    }
    let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    let unit = *segments.last()?;
    if !unit.ends_with(".scope") && !unit.ends_with(".service") && !unit.ends_with(".slice") {
        return None;
    }
    let unit = unescape_unit(unit);
    let mut identity = CgroupIdentity {
        unit: unit.clone(),
        path: trimmed.to_string(),
        flatpak_app_id: None,
        snap: None,
        desktop_app_id: None,
        in_app_slice: segments.contains(&"app.slice"),
        in_system_slice: segments.contains(&"system.slice"),
    };

    let stem = unit
        .strip_suffix(".scope")
        .or_else(|| unit.strip_suffix(".service"))
        .unwrap_or(&unit);

    if let Some(rest) = stem.strip_prefix("snap.") {
        // snap.<snap>.<app>.<uuid>
        let mut parts = rest.split('.');
        if let Some(snap) = parts.next().filter(|snap| !snap.is_empty()) {
            let app = parts
                .next()
                .map(str::to_string)
                .filter(|app| !app.is_empty());
            identity.snap = Some((snap.to_string(), app));
        }
    } else if let Some(rest) = stem.strip_prefix("app-flatpak-") {
        let app_id = strip_launch_suffix(rest);
        if !app_id.is_empty() {
            identity.flatpak_app_id = Some(app_id.to_string());
        }
    } else if let Some(rest) = stem.strip_prefix("app-") {
        let rest = LAUNCHERS
            .iter()
            .find_map(|launcher| rest.strip_prefix(&format!("{launcher}-")))
            .unwrap_or(rest);
        let app_id = strip_launch_suffix(rest);
        if !app_id.is_empty() {
            identity.desktop_app_id = Some(app_id.to_string());
        }
    }

    Some(identity)
}

/// The evidence a cgroup identity supports, chosen by the configured
/// precedence.
fn evidence_from_cgroup(
    identity: &CgroupIdentity,
    precedence: &GroupingPrecedence,
) -> Option<GroupingEvidence> {
    let mut best: Option<(EvidenceKind, GroupingEvidence)> = None;
    let mut consider = |kind: EvidenceKind, evidence: GroupingEvidence| {
        if !precedence.allows(kind) {
            return;
        }
        best = match best.take() {
            None => Some((kind, evidence)),
            Some((current_kind, current)) => {
                if precedence.preferred(current_kind, kind) == kind {
                    Some((kind, evidence))
                } else {
                    Some((current_kind, current))
                }
            }
        };
    };

    if let Some(app_id) = &identity.flatpak_app_id {
        consider(
            EvidenceKind::Flatpak,
            GroupingEvidence::Flatpak {
                app_id: app_id.clone(),
                unit: identity.unit.clone(),
            },
        );
    }
    if let Some((snap, app)) = &identity.snap {
        consider(
            EvidenceKind::Snap,
            GroupingEvidence::Snap {
                snap: snap.clone(),
                app: app.clone(),
                unit: identity.unit.clone(),
            },
        );
    }
    if let Some(app_id) = &identity.desktop_app_id {
        consider(
            EvidenceKind::DesktopApplication,
            GroupingEvidence::DesktopApplication {
                app_id: app_id.clone(),
                unit: identity.unit.clone(),
            },
        );
    }
    consider(
        EvidenceKind::SystemdUnit,
        GroupingEvidence::SystemdUnit {
            unit: identity.unit.clone(),
            path: identity.path.clone(),
        },
    );
    best.map(|(_, evidence)| evidence)
}

/// The key two processes must share to be one group.
fn group_key(evidence: &GroupingEvidence, pid: u32, owner_uid: Option<u64>) -> String {
    // The owning user is part of every key. Two users running the same unit
    // are two separate things, and one CPU total across both would be a lie
    // about who is using the machine.
    let user = owner_uid
        .map(|uid| uid.to_string())
        .unwrap_or_else(|| "unknown-user".to_string());
    match evidence {
        GroupingEvidence::Flatpak { app_id, .. } => format!("flatpak:{user}:{app_id}"),
        GroupingEvidence::Snap { snap, .. } => format!("snap:{user}:{snap}"),
        GroupingEvidence::DesktopApplication { app_id, .. } => format!("desktop:{user}:{app_id}"),
        GroupingEvidence::SystemdUnit { path, .. } => format!("unit:{user}:{path}"),
        // The two low-confidence cases never merge anything, so the PID is
        // part of the key by design.
        GroupingEvidence::ExecutableIdentity { executable } => {
            format!("process:{user}:{executable}:{pid}")
        }
        GroupingEvidence::Unattributed { .. } => format!("process:{user}:unattributed:{pid}"),
        GroupingEvidence::Ancestry { root_pid, .. } => format!("ancestry:{user}:{root_pid}"),
    }
}

/// The name to show for a group.
///
/// Which evidence *defines* the group is a precedence question; what to call
/// it is not. An application launched into a systemd scope is still GIMP, so
/// the most human-readable identity in the cgroup wins the label even when the
/// unit membership is what did the grouping. The evidence stays visible next
/// to the name, so the label never overstates what is known.
fn display_name_for(
    evidence: &GroupingEvidence,
    identity: Option<&CgroupIdentity>,
    fallback: &str,
) -> String {
    let last_segment = |app_id: &str| -> String {
        app_id
            .rsplit('.')
            .next()
            .filter(|last| !last.is_empty())
            .unwrap_or(app_id)
            .to_string()
    };
    if let Some(identity) = identity {
        if let Some(app_id) = identity
            .flatpak_app_id
            .as_ref()
            .or(identity.desktop_app_id.as_ref())
        {
            return last_segment(app_id);
        }
        if let Some((snap, _)) = &identity.snap {
            return snap.clone();
        }
    }
    match evidence {
        GroupingEvidence::Flatpak { app_id, .. }
        | GroupingEvidence::DesktopApplication { app_id, .. } => last_segment(app_id),
        GroupingEvidence::Snap { snap, .. } => snap.clone(),
        GroupingEvidence::SystemdUnit { unit, .. } => unit
            .strip_suffix(".service")
            .or_else(|| unit.strip_suffix(".scope"))
            .unwrap_or(unit)
            .to_string(),
        _ => fallback.to_string(),
    }
}

fn kind_for(evidence: &GroupingEvidence, identity: Option<&CgroupIdentity>) -> AppKind {
    match evidence {
        GroupingEvidence::Flatpak { .. }
        | GroupingEvidence::Snap { .. }
        | GroupingEvidence::DesktopApplication { .. } => AppKind::UserApplication,
        GroupingEvidence::SystemdUnit { .. } => match identity {
            Some(identity) if identity.in_app_slice => AppKind::UserApplication,
            _ => AppKind::BackgroundService,
        },
        // Nothing justifies calling these a user-facing application.
        _ => AppKind::BackgroundService,
    }
}

struct Attribution {
    key: String,
    evidence: GroupingEvidence,
    kind: AppKind,
    display_name: String,
}

/// Group a process list into applications and background services.
pub fn group_processes(processes: &[ProcessFacts], precedence: &GroupingPrecedence) -> Grouping {
    let identities: HashMap<u32, Option<CgroupIdentity>> = processes
        .iter()
        .map(|process| {
            let identity = process
                .cgroup
                .any_value()
                .and_then(|path| parse_cgroup_identity(path));
            (process.pid, identity)
        })
        .collect();

    // First pass: everything a cgroup can decide on its own.
    let mut attributions: HashMap<u32, Attribution> = HashMap::new();
    for process in processes {
        let identity = identities.get(&process.pid).and_then(Option::as_ref);
        let Some(evidence) =
            identity.and_then(|identity| evidence_from_cgroup(identity, precedence))
        else {
            continue;
        };
        attributions.insert(
            process.pid,
            Attribution {
                key: group_key(&evidence, process.pid, process.uid.copied()),
                kind: kind_for(&evidence, identity),
                display_name: display_name_for(&evidence, identity, &process.display_name()),
                evidence,
            },
        );
    }

    // Second pass: a process with no cgroup identity joins its nearest
    // ancestor that has one. The walk is bounded by the number of processes
    // and refuses to revisit a PID, so a cycle in a malformed parent chain
    // cannot hang the view.
    let by_pid: HashMap<u32, &ProcessFacts> = processes
        .iter()
        .map(|process| (process.pid, process))
        .collect();
    if precedence.allows(EvidenceKind::Ancestry) {
        let unresolved: Vec<u32> = processes
            .iter()
            .map(|process| process.pid)
            .filter(|pid| !attributions.contains_key(pid))
            .collect();
        for pid in unresolved {
            let mut seen = HashSet::new();
            let immediate_parent = by_pid.get(&pid).and_then(|process| process.parent());
            let mut cursor = immediate_parent;
            while let Some(ancestor_pid) = cursor {
                if ancestor_pid == 0 || !seen.insert(ancestor_pid) {
                    break;
                }
                if let Some(ancestor) = attributions.get(&ancestor_pid) {
                    // An ancestor that itself joined by ancestry already knows
                    // where the group actually starts, so the root travels
                    // down the chain instead of being re-derived at each step.
                    let root_pid = match &ancestor.evidence {
                        GroupingEvidence::Ancestry { root_pid, .. } => *root_pid,
                        _ => ancestor_pid,
                    };
                    let attribution = Attribution {
                        key: ancestor.key.clone(),
                        kind: ancestor.kind,
                        display_name: ancestor.display_name.clone(),
                        evidence: GroupingEvidence::Ancestry {
                            parent_pid: immediate_parent.unwrap_or(ancestor_pid),
                            root_pid,
                        },
                    };
                    attributions.insert(pid, attribution);
                    break;
                }
                cursor = by_pid
                    .get(&ancestor_pid)
                    .and_then(|process| process.parent());
            }
        }
    }

    // Third pass: whatever is left stands alone. This is the executable
    // identity fallback, and it is a labelling rule rather than a merge rule:
    // the key carries the PID, so two processes never join here just because
    // they run the same program.
    for process in processes {
        if attributions.contains_key(&process.pid) {
            continue;
        }
        let evidence = if precedence.allows(EvidenceKind::ExecutableIdentity) {
            GroupingEvidence::ExecutableIdentity {
                executable: process.display_name(),
            }
        } else {
            GroupingEvidence::Unattributed {
                detail: "no evidence source is enabled for this process".into(),
            }
        };
        attributions.insert(
            process.pid,
            Attribution {
                key: group_key(&evidence, process.pid, process.uid.copied()),
                kind: AppKind::BackgroundService,
                display_name: process.display_name(),
                evidence,
            },
        );
    }

    // Assemble. A BTreeMap keys the output deterministically, so two runs over
    // the same round produce the same table.
    let mut groups: BTreeMap<String, AppGroup> = BTreeMap::new();
    let mut order: Vec<u32> = processes.iter().map(|process| process.pid).collect();
    order.sort_unstable();
    for pid in order {
        let Some(attribution) = attributions.get(&pid) else {
            continue;
        };
        let group = groups
            .entry(attribution.key.clone())
            .or_insert_with(|| AppGroup {
                key: attribution.key.clone(),
                display_name: attribution.display_name.clone(),
                kind: attribution.kind,
                evidence: attribution.evidence.clone(),
                members: Vec::new(),
            });
        // The group is defined by its strongest evidence: a child that joined
        // by ancestry must not downgrade the unit that named the group.
        if attribution.evidence.confidence() < group.evidence.confidence() {
            group.evidence = attribution.evidence.clone();
            group.display_name = attribution.display_name.clone();
            group.kind = attribution.kind;
        }
        group.members.push(MemberAttribution {
            pid,
            evidence: attribution.evidence.clone(),
        });
    }

    let mut grouping = Grouping::default();
    for group in groups.into_values() {
        match group.kind {
            AppKind::UserApplication => grouping.applications.push(group),
            AppKind::BackgroundService => grouping.services.push(group),
        }
    }
    grouping
        .applications
        .sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.key.cmp(&b.key)));
    grouping
        .services
        .sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.key.cmp(&b.key)));
    grouping
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::Field;

    fn process(pid: u32, name: &str, parent: u32, cgroup: Option<&str>) -> ProcessFacts {
        let mut facts = ProcessFacts::synthetic(pid, name);
        facts.parent_pid = Field::Value(parent as u64);
        facts.cgroup = match cgroup {
            Some(path) => Field::Value(path.to_string()),
            None => Field::Unknown(monitor_core::UnknownReason::EntityDisappeared),
        };
        facts
    }

    const NAUTILUS: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Nautilus-4321.scope";
    const GIMP: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-flatpak-org.gimp.GIMP-9182.scope";
    const FIREFOX_SNAP: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/snap.firefox.firefox.aaaa-bbbb.scope";
    const NETWORK_MANAGER: &str = "/system.slice/NetworkManager.service";
    const TERMINAL_A: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Terminal-11.scope";
    const TERMINAL_B: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Terminal-22.scope";

    #[test]
    fn a_desktop_launch_scope_yields_the_application_id_without_the_launch_suffix() {
        let identity = parse_cgroup_identity(NAUTILUS).unwrap();
        assert_eq!(
            identity.desktop_app_id.as_deref(),
            Some("org.gnome.Nautilus")
        );
        assert!(identity.in_app_slice);
        assert!(!identity.in_system_slice);
    }

    #[test]
    fn flatpak_and_snap_identities_are_read_from_their_own_scope_shapes() {
        let flatpak = parse_cgroup_identity(GIMP).unwrap();
        assert_eq!(flatpak.flatpak_app_id.as_deref(), Some("org.gimp.GIMP"));
        assert_eq!(flatpak.desktop_app_id, None);

        let snap = parse_cgroup_identity(FIREFOX_SNAP).unwrap();
        assert_eq!(
            snap.snap,
            Some(("firefox".to_string(), Some("firefox".to_string())))
        );
    }

    #[test]
    fn a_systemd_escape_in_an_application_id_is_decoded() {
        let identity = parse_cgroup_identity(
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-com.example.my\\x2dapp-7.scope",
        )
        .unwrap();
        assert_eq!(
            identity.desktop_app_id.as_deref(),
            Some("com.example.my-app")
        );
    }

    #[test]
    fn a_root_cgroup_carries_no_identity() {
        assert!(parse_cgroup_identity("/").is_none());
        assert!(parse_cgroup_identity("").is_none());
        assert!(parse_cgroup_identity("/not-a-unit").is_none());
    }

    #[test]
    fn processes_in_one_launch_scope_become_one_application() {
        let processes = vec![
            process(100, "nautilus", 1, Some(NAUTILUS)),
            process(101, "nautilus", 100, Some(NAUTILUS)),
        ];
        let grouping = group_processes(&processes, &GroupingPrecedence::default());
        assert_eq!(grouping.applications.len(), 1);
        assert!(grouping.services.is_empty());
        let group = &grouping.applications[0];
        // The default precedence groups by unit membership, and the label
        // still comes from the desktop application id inside that unit.
        assert_eq!(group.display_name, "Nautilus");
        assert_eq!(group.members.len(), 2);
        assert_eq!(group.confidence(), Confidence::High);
        assert!(matches!(
            group.evidence,
            GroupingEvidence::SystemdUnit { .. }
        ));
    }

    #[test]
    fn unrelated_processes_with_the_same_executable_name_are_never_merged() {
        // Two shells with the same executable name, one in each of two
        // terminal windows, plus two with no cgroup at all and no shared
        // parent. Nothing here is one application.
        let processes = vec![
            process(200, "bash", 10, Some(TERMINAL_A)),
            process(201, "bash", 11, Some(TERMINAL_B)),
            process(202, "python3", 0, None),
            process(203, "python3", 0, None),
        ];
        let grouping = group_processes(&processes, &GroupingPrecedence::default());
        assert_eq!(grouping.len(), 4, "{grouping:#?}");
        for pid in [200, 201, 202, 203] {
            let group = grouping.group_of(pid).expect("every process is placed");
            assert_eq!(
                group.members.len(),
                1,
                "pid {pid} was merged with something unrelated"
            );
        }
        // And the fallback still records that the name was all it had.
        let lonely = grouping.group_of(202).unwrap();
        assert!(matches!(
            lonely.evidence,
            GroupingEvidence::ExecutableIdentity { .. }
        ));
        assert_eq!(lonely.confidence(), Confidence::Low);
    }

    #[test]
    fn the_same_unit_run_by_two_users_is_two_groups() {
        let mut mine = process(300, "pipewire", 1, Some(NETWORK_MANAGER));
        mine.uid = Field::Value(1000);
        let mut theirs = process(301, "pipewire", 1, Some(NETWORK_MANAGER));
        theirs.uid = Field::Value(1001);
        let grouping = group_processes(&[mine, theirs], &GroupingPrecedence::default());
        assert_eq!(grouping.services.len(), 2);
    }

    #[test]
    fn a_system_service_is_kept_out_of_the_applications_list() {
        let processes = vec![process(400, "NetworkManager", 1, Some(NETWORK_MANAGER))];
        let grouping = group_processes(&processes, &GroupingPrecedence::default());
        assert!(grouping.applications.is_empty());
        assert_eq!(grouping.services.len(), 1);
        assert_eq!(grouping.services[0].display_name, "NetworkManager");
        assert!(matches!(
            grouping.services[0].evidence,
            GroupingEvidence::SystemdUnit { .. }
        ));
    }

    #[test]
    fn a_child_with_no_cgroup_joins_its_nearest_attributed_ancestor() {
        let processes = vec![
            process(500, "gimp", 1, Some(GIMP)),
            process(501, "script-fu", 500, None),
            process(502, "helper", 501, None),
        ];
        let grouping = group_processes(&processes, &GroupingPrecedence::default());
        assert_eq!(grouping.applications.len(), 1);
        let group = &grouping.applications[0];
        assert_eq!(group.members.len(), 3);
        assert_eq!(group.display_name, "GIMP");
        // The group keeps the leader's high-confidence evidence; the members
        // that joined by ancestry say so individually.
        assert!(matches!(
            group.evidence,
            GroupingEvidence::SystemdUnit { .. }
        ));
        let grandchild = group
            .members
            .iter()
            .find(|member| member.pid == 502)
            .unwrap();
        assert_eq!(
            grandchild.evidence,
            GroupingEvidence::Ancestry {
                parent_pid: 501,
                root_pid: 500
            }
        );
    }

    #[test]
    fn a_cycle_in_the_parent_chain_does_not_hang_the_grouping() {
        let mut a = process(600, "a", 601, None);
        let b = process(601, "b", 600, None);
        a.parent_pid = Field::Value(601);
        let grouping = group_processes(&[a, b], &GroupingPrecedence::default());
        assert_eq!(grouping.len(), 2);
    }

    #[test]
    fn precedence_decides_which_identity_names_a_flatpak_scope() {
        let processes = vec![process(700, "gimp", 1, Some(GIMP))];

        let default = group_processes(&processes, &GroupingPrecedence::default());
        assert!(matches!(
            default.applications[0].evidence,
            GroupingEvidence::SystemdUnit { .. }
        ));

        // Move Flatpak identity above unit membership and the same process
        // groups under the application id instead.
        let flatpak_first = GroupingPrecedence::new(vec![
            EvidenceKind::Flatpak,
            EvidenceKind::SystemdUnit,
            EvidenceKind::Ancestry,
            EvidenceKind::ExecutableIdentity,
        ]);
        let reordered = group_processes(&processes, &flatpak_first);
        assert!(matches!(
            reordered.applications[0].evidence,
            GroupingEvidence::Flatpak { .. }
        ));
    }

    #[test]
    fn disabling_every_evidence_source_leaves_each_process_unattributed() {
        let processes = vec![
            process(800, "nautilus", 1, Some(NAUTILUS)),
            process(801, "nautilus", 800, Some(NAUTILUS)),
        ];
        let grouping = group_processes(&processes, &GroupingPrecedence::new(Vec::new()));
        assert_eq!(grouping.len(), 2);
        for group in grouping.all() {
            assert!(matches!(
                group.evidence,
                GroupingEvidence::Unattributed { .. }
            ));
            assert_eq!(group.kind, AppKind::BackgroundService);
        }
    }

    #[test]
    fn grouping_is_deterministic_across_input_order() {
        let forward = vec![
            process(900, "nautilus", 1, Some(NAUTILUS)),
            process(901, "gimp", 1, Some(GIMP)),
            process(902, "firefox", 1, Some(FIREFOX_SNAP)),
        ];
        let mut backward = forward.clone();
        backward.reverse();
        let a = group_processes(&forward, &GroupingPrecedence::default());
        let b = group_processes(&backward, &GroupingPrecedence::default());
        assert_eq!(a, b);
    }

    #[test]
    fn a_process_whose_cgroup_was_unreadable_still_appears_somewhere() {
        let mut denied = ProcessFacts::synthetic(1000, "unknown");
        denied.cgroup = Field::PermissionDenied {
            path: "/proc/1000/cgroup".into(),
        };
        let grouping = group_processes(&[denied], &GroupingPrecedence::default());
        assert_eq!(grouping.len(), 1);
        assert!(grouping.group_of(1000).is_some());
    }
}
