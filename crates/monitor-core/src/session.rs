//! What the desktop session stands on, so no End task, End process tree, or
//! Force stop from Better Monitor ever ends it (owner, 2026-11-22: none of
//! the three is offered on these processes).
//!
//! Most processes are fine to end, by asking (`SIGTERM`) or at once
//! (`SIGKILL`), including every ordinary user service, a terminal and its
//! shell, and a Flatpak application. A few are not, because a session ends
//! with them whichever signal it is: the compositor, the session's message
//! bus, the user's service manager, the session manager, what logind records
//! as a graphical session's leader, the X server when the compositor treats
//! it as mandatory, and Better Monitor itself with what it runs under. A
//! second graphical session of the same person counts too: ending its
//! compositor closes every app there.
//!
//! Which process plays each part is decided from what the system records
//! about it first: who holds the listening sockets of the session bus and of
//! each Wayland and X display, the cgroup systemd put a process in, and
//! logind's sessions (read over D-Bus by the window). A name is used only
//! where nothing else says it ([`NAMED`], every entry with its reason), or
//! where the fact that would have decided could not be read, for the whole
//! session or for that one process: an unknown never makes a process that
//! matches part of the rule safe to end.
//!
//! This module only decides. `monitor-actions-linux` reads the facts.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The part a process plays in the session.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    /// Better Monitor's own window, or a process it runs under.
    Monitor,
    /// The Better Monitor service the window reads from.
    MonitorService,
    /// The user's service manager, `systemd --user`.
    UserManager,
    /// The session's message bus.
    MessageBus,
    /// A Wayland compositor of one of the person's graphical sessions.
    Compositor,
    /// An X server a compositor ends with.
    XServer,
    /// What logind records as a graphical session's leader, or what that
    /// leader started (GDM's `gdm-wayland-session`, a greeter's session
    /// command).
    SessionLauncher,
    /// The session manager.
    SessionManager,
}

/// What is read about one process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMember {
    pub pid: u32,
    /// The kernel's command name (`comm`), at most 15 bytes.
    pub name: String,
    pub parent: Option<u32>,
    /// The process's cgroup path (the `0::` line of `/proc/<pid>/cgroup`),
    /// `None` when it could not be read.
    pub cgroup: Option<String>,
}

/// One of the person's logind sessions, as `org.freedesktop.login1` says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginSession {
    pub id: String,
    /// `Session.Leader`; `None` when logind reports none (0).
    pub leader: Option<u32>,
    /// `Type` is `wayland`, `x11` or `mir` and `Class` is `user`.
    pub graphical: bool,
    /// The session Better Monitor runs in (`GetSessionByPID`), or the
    /// user's `Display` session when the window runs outside any.
    pub own: bool,
}

/// What the session's own records say, at one moment. Every `Option` is
/// `None` when the record could not be read, which is not the same as "no
/// such process"; [`SessionFacts::fds_read`] says for which processes a
/// holder list is a fact.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionFacts {
    /// The user the window runs as.
    pub uid: u32,
    /// The processes above Better Monitor.
    pub monitor_ancestors: Vec<u32>,
    /// The holders of the session bus's listening socket (the window's
    /// `DBUS_SESSION_BUS_ADDRESS`, and `$XDG_RUNTIME_DIR/bus`, the one user
    /// bus every session of the person shares).
    pub bus_holders: Option<Vec<u32>>,
    /// The holders of the window's own Wayland display socket.
    pub compositor_holders: Option<Vec<u32>>,
    /// The holders of the window's own X display sockets; empty when it has
    /// no X display.
    pub x_display_holders: Option<Vec<u32>>,
    /// The holders of every other listening `$XDG_RUNTIME_DIR/wayland-*`
    /// socket: a compositor of another of the person's sessions when it runs
    /// in a graphical session ([`in_graphical_session`]), a nested or
    /// sandboxed one otherwise.
    pub other_compositor_holders: Option<Vec<u32>>,
    /// The same for every other listening `/tmp/.X11-unix/X*` socket.
    pub other_x_holders: Option<Vec<u32>>,
    /// Compositors known to start Xwayland on demand and carry on without it
    /// ([`keeps_xwayland_on_demand`]).
    pub on_demand_compositors: Vec<u32>,
    /// The processes whose descriptors were read: a holder list is a fact
    /// only for these.
    pub fds_read: BTreeSet<u32>,
    /// What was read about each process scanned.
    pub members: BTreeMap<u32, SessionMember>,
    /// The person's logind sessions; `None` when logind could not be asked.
    pub sessions: Option<Vec<LoginSession>>,
}

impl SessionFacts {
    /// Every process the facts name as holding something: when one of them
    /// is gone, the facts are read again.
    pub fn holders(&self) -> BTreeSet<u32> {
        [
            &self.bus_holders,
            &self.compositor_holders,
            &self.x_display_holders,
            &self.other_compositor_holders,
            &self.other_x_holders,
        ]
        .into_iter()
        .flatten()
        .flatten()
        .copied()
        .collect()
    }
}

/// A process recognised by its name, with why a name is used.
#[derive(Clone, Copy, Debug)]
pub struct NamedProcess {
    pub name: &'static str,
    pub role: SessionRole,
    /// `true`: the name decides on its own, because nothing the system
    /// records says it. `false`: the name decides only when the fact that
    /// would have decided could not be read.
    pub by_name_alone: bool,
    pub reason: &'static str,
}

/// Every name the guard knows, and why. Nothing else in Better Monitor
/// names a session process.
pub const NAMED: &[NamedProcess] = &[
    NamedProcess {
        name: "gnome-session-binary",
        role: SessionRole::SessionManager,
        by_name_alone: true,
        reason: "GNOME's session manager: GNOME ends the session when it goes, and no socket or logind record names it",
    },
    NamedProcess {
        name: "cosmic-session",
        role: SessionRole::SessionManager,
        by_name_alone: true,
        reason: "COSMIC's session manager: cosmic-comp and the panel are its children and end with it, and no record names it",
    },
    NamedProcess {
        name: "systemd",
        role: SessionRole::UserManager,
        by_name_alone: false,
        reason: "the user's service manager, when its cgroup (user@UID.service/init.scope) cannot be read",
    },
    NamedProcess {
        name: "better-monitor-service",
        role: SessionRole::MonitorService,
        by_name_alone: false,
        reason: "Better Monitor's service, when its cgroup (better-monitor.service) cannot be read",
    },
    NamedProcess {
        name: "dbus-daemon",
        role: SessionRole::MessageBus,
        by_name_alone: false,
        reason: "a message bus, when whether it holds the session bus's socket cannot be read",
    },
    NamedProcess {
        name: "dbus-broker",
        role: SessionRole::MessageBus,
        by_name_alone: false,
        reason: "a message bus, when whether it holds the session bus's socket cannot be read",
    },
    NamedProcess {
        name: "dbus-broker-launch",
        role: SessionRole::MessageBus,
        by_name_alone: false,
        reason: "dbus-broker's launcher holds the bus socket, when that cannot be read",
    },
    NamedProcess {
        name: "gnome-shell",
        role: SessionRole::Compositor,
        by_name_alone: false,
        reason: "GNOME's compositor, when whether it holds a Wayland display socket cannot be read; also the one compositor whose Xwayland policy is known (mutter: on demand inside a systemd user unit)",
    },
    NamedProcess {
        name: "cosmic-comp",
        role: SessionRole::Compositor,
        by_name_alone: false,
        reason: "COSMIC's compositor, when whether it holds a Wayland display socket cannot be read",
    },
    NamedProcess {
        name: "Xwayland",
        role: SessionRole::XServer,
        by_name_alone: false,
        reason: "an X server, when whether it holds an X display socket cannot be read",
    },
    NamedProcess {
        name: "gdm-wayland-session",
        role: SessionRole::SessionLauncher,
        by_name_alone: false,
        reason: "what GDM starts a Wayland session with, when logind cannot be asked",
    },
    NamedProcess {
        name: "gdm-x-session",
        role: SessionRole::SessionLauncher,
        by_name_alone: false,
        reason: "what GDM starts an X session with, when logind cannot be asked",
    },
];

/// The unit Better Monitor's service runs as (`packaging/monitor/`).
pub const MONITOR_SERVICE_UNIT: &str = "better-monitor.service";

/// Whether a kernel command name is `name`. The kernel keeps the first 15
/// bytes of a program's name, so `gnome-session-binary` reads as
/// `gnome-session-b`.
pub fn comm_is(comm: &str, name: &str) -> bool {
    const COMM_LENGTH: usize = 15;
    // A full name (a target's own name, before the snapshot covers it)
    // matches itself.
    if comm == name || name.len() <= COMM_LENGTH {
        comm == name
    } else {
        comm.len() == COMM_LENGTH && name.as_bytes().starts_with(comm.as_bytes())
    }
}

fn named(member: &SessionMember, role: SessionRole, by_name_alone: bool) -> bool {
    NAMED.iter().any(|entry| {
        entry.role == role
            && entry.by_name_alone == by_name_alone
            && comm_is(&member.name, entry.name)
    })
}

fn user_manager_cgroup(uid: u32) -> String {
    format!("/user@{uid}.service/")
}

/// Whether a cgroup path is inside a unit of the user's own service manager
/// (what `sd_pid_get_user_unit` succeeds on).
fn in_user_unit(cgroup: &str, uid: u32) -> bool {
    let Some((_, inside)) = cgroup.split_once(&user_manager_cgroup(uid)) else {
        return false;
    };
    let leaf = inside.rsplit('/').next().unwrap_or_default();
    leaf != "init.scope" && (leaf.ends_with(".service") || leaf.ends_with(".scope"))
}

/// Whether a process runs as part of one of the person's graphical sessions:
/// in the user manager's `session.slice` (systemd's slice for units the
/// graphical session needs, where GNOME runs gnome-shell), or in the scope
/// logind made for a graphical session. An unreadable cgroup counts, so an
/// unknown never makes a compositor safe; a nested or sandboxed compositor
/// in `app.slice` does not.
pub fn in_graphical_session(member: &SessionMember, facts: &SessionFacts) -> bool {
    let Some(cgroup) = member.cgroup.as_deref() else {
        return true;
    };
    if cgroup.contains(&format!("{}session.slice/", user_manager_cgroup(facts.uid))) {
        return true;
    }
    match &facts.sessions {
        Some(sessions) => sessions.iter().filter(|s| s.graphical).any(|session| {
            cgroup
                .rsplit('/')
                .next()
                .is_some_and(|leaf| leaf == format!("session-{}.scope", session.id))
        }),
        // Without logind, any session scope may be a graphical one.
        None => cgroup
            .rsplit('/')
            .next()
            .is_some_and(|leaf| leaf.starts_with("session-") && leaf.ends_with(".scope")),
    }
}

/// Whether a compositor is known to start Xwayland only on demand and to
/// carry on when it ends.
///
/// Read from mutter 46.2 (the GNOME 46 of Ubuntu 24.04 and Zorin 18):
/// `meta_context_main_get_x11_display_policy` in
/// `src/core/meta-context-main.c` returns `ON_DEMAND` for a Wayland session
/// when `sd_pid_get_user_unit` succeeds (gnome-shell runs as
/// `org.gnome.Shell@wayland.service` there) and `MANDATORY` otherwise; and
/// `xserver_died` in `src/wayland/meta-xwayland.c` calls `meta_exit` when
/// Xwayland ends under `MANDATORY`, and shuts the X11 display down and sets
/// its sockets up again under `ON_DEMAND`. No other compositor's policy is
/// known here, so every other one keeps its X server protected.
pub fn keeps_xwayland_on_demand(compositor: &SessionMember, uid: u32) -> bool {
    comm_is(&compositor.name, "gnome-shell")
        && compositor
            .cgroup
            .as_deref()
            .is_some_and(|cgroup| in_user_unit(cgroup, uid))
}

/// Whether `member` holds a socket the facts list, by the facts when its
/// descriptors were read, and by its name when they were not.
fn holds(
    member: &SessionMember,
    holders: &Option<Vec<u32>>,
    facts: &SessionFacts,
    role: SessionRole,
) -> bool {
    match holders {
        Some(holders) if holders.contains(&member.pid) => true,
        Some(_) if facts.fds_read.contains(&member.pid) => false,
        _ => named(member, role, false),
    }
}

/// Whether an X server's compositor is known to carry on without it.
fn x_server_on_demand(member: &SessionMember, facts: &SessionFacts) -> bool {
    member
        .parent
        .is_some_and(|parent| facts.on_demand_compositors.contains(&parent))
}

/// The part `member` plays in the session, or `None` when no session stands
/// on it and it may be ended.
pub fn session_role(member: &SessionMember, facts: &SessionFacts) -> Option<SessionRole> {
    if facts.monitor_ancestors.contains(&member.pid) {
        return Some(SessionRole::Monitor);
    }
    match member.cgroup.as_deref() {
        Some(cgroup) => {
            let manager = user_manager_cgroup(facts.uid);
            if cgroup.ends_with(&format!("{manager}init.scope")) {
                return Some(SessionRole::UserManager);
            }
            if cgroup.contains(&manager) && cgroup.rsplit('/').next() == Some(MONITOR_SERVICE_UNIT)
            {
                return Some(SessionRole::MonitorService);
            }
        }
        None => {
            for role in [SessionRole::UserManager, SessionRole::MonitorService] {
                if named(member, role, false) {
                    return Some(role);
                }
            }
        }
    }
    if holds(member, &facts.bus_holders, facts, SessionRole::MessageBus) {
        return Some(SessionRole::MessageBus);
    }
    if holds(
        member,
        &facts.compositor_holders,
        facts,
        SessionRole::Compositor,
    ) {
        return Some(SessionRole::Compositor);
    }
    if holds(
        member,
        &facts.other_compositor_holders,
        facts,
        SessionRole::Compositor,
    ) && in_graphical_session(member, facts)
    {
        return Some(SessionRole::Compositor);
    }
    if !x_server_on_demand(member, facts) {
        if holds(
            member,
            &facts.x_display_holders,
            facts,
            SessionRole::XServer,
        ) {
            return Some(SessionRole::XServer);
        }
        if holds(member, &facts.other_x_holders, facts, SessionRole::XServer)
            && in_graphical_session(member, facts)
        {
            return Some(SessionRole::XServer);
        }
    }
    let launcher = match &facts.sessions {
        Some(sessions) => sessions
            .iter()
            .filter(|session| session.graphical || session.own)
            .filter_map(|session| session.leader)
            .any(|leader| member.pid == leader || member.parent == Some(leader)),
        None => named(member, SessionRole::SessionLauncher, false),
    };
    if launcher {
        return Some(SessionRole::SessionLauncher);
    }
    named(member, SessionRole::SessionManager, true).then_some(SessionRole::SessionManager)
}

/// Whether an action is one this guard governs: every action that ends a
/// process. Pausing and lowering priority end nothing and keep their own
/// rules.
pub fn guards(action: crate::ProcessAction) -> bool {
    action.is_destructive()
}

/// The refusal for one process the session stands on, `None` when it may
/// be ended.
pub fn refusal(member: &SessionMember, facts: &SessionFacts) -> Option<crate::ActionRefusal> {
    session_role(member, facts).map(|role| crate::ActionRefusal::SessionProcess {
        pid: member.pid,
        name: member.name.clone(),
        role,
    })
}

/// Whether a process tree may be ended, by the same rule over every member:
/// the root's own refusal when the root is protected, otherwise the first
/// protected process below it, which refuses the whole tree. Members that
/// are gone are simply absent from `members`.
pub fn tree_refusal(
    root: u32,
    members: &[SessionMember],
    facts: &SessionFacts,
) -> Option<crate::ActionRefusal> {
    if let Some(top) = members.iter().find(|member| member.pid == root)
        && let Some(refused) = refusal(top, facts)
    {
        return Some(refused);
    }
    members.iter().find_map(|member| {
        session_role(member, facts).map(|role| crate::ActionRefusal::ProtectedTree {
            pid: member.pid,
            name: member.name.clone(),
            role,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UID: u32 = 1000;

    fn member(pid: u32, name: &str, parent: u32, cgroup: &str) -> SessionMember {
        SessionMember {
            pid,
            name: name.into(),
            parent: Some(parent),
            cgroup: Some(format!("/user.slice/user-1000.slice/{cgroup}")),
        }
    }

    fn user(unit: &str) -> String {
        format!("user@1000.service/{unit}")
    }

    fn session(id: &str, leader: u32, graphical: bool, own: bool) -> LoginSession {
        LoginSession {
            id: id.into(),
            leader: Some(leader),
            graphical,
            own,
        }
    }

    fn read(facts: &mut SessionFacts, members: &[SessionMember]) {
        for member in members {
            facts.fds_read.insert(member.pid);
            facts.members.insert(member.pid, member.clone());
        }
    }

    /// A GNOME 46 session as Ubuntu 24.04 and Zorin 18 run it, with the
    /// PIDs and cgroups read on the owner's desktop on 2026-11-22.
    fn gnome() -> (SessionFacts, Vec<SessionMember>) {
        let members = vec![
            member(2338, "systemd", 1, &user("init.scope")),
            member(
                2370,
                "dbus-daemon",
                2338,
                &user("session.slice/dbus.service"),
            ),
            member(
                2545,
                "gnome-shell",
                2338,
                &user("session.slice/org.gnome.Shell@wayland.service"),
            ),
            member(
                2600,
                "Xwayland",
                2545,
                &user("session.slice/org.gnome.Shell@wayland.service"),
            ),
            member(2408, "gdm-wayland-ses", 2321, "session-2.scope"),
            member(2412, "gnome-session-b", 2408, "session-2.scope"),
            member(
                2480,
                "gnome-session-b",
                2338,
                &user("session.slice/gnome-session-manager@gnome.service"),
            ),
            member(
                3336,
                "better-monitor-",
                2338,
                &user("app.slice/better-monitor.service"),
            ),
        ];
        let mut facts = SessionFacts {
            uid: UID,
            monitor_ancestors: vec![2338],
            bus_holders: Some(vec![2338, 2370]),
            compositor_holders: Some(vec![2545]),
            x_display_holders: Some(vec![2545, 2600]),
            other_compositor_holders: Some(vec![]),
            other_x_holders: Some(vec![]),
            on_demand_compositors: vec![2545],
            sessions: Some(vec![session("2", 2321, true, true)]),
            ..SessionFacts::default()
        };
        read(&mut facts, &members);
        (facts, members)
    }

    #[test]
    fn a_gnome_session_keeps_what_it_stands_on() {
        let (facts, members) = gnome();
        let roles: Vec<(u32, Option<SessionRole>)> = members
            .iter()
            .map(|member| (member.pid, session_role(member, &facts)))
            .collect();
        assert_eq!(
            roles,
            vec![
                (2338, Some(SessionRole::Monitor)),
                (2370, Some(SessionRole::MessageBus)),
                (2545, Some(SessionRole::Compositor)),
                // Mutter starts Xwayland on demand inside a user unit and
                // carries on when it ends.
                (2600, None),
                (2408, Some(SessionRole::SessionLauncher)),
                (2412, Some(SessionRole::SessionManager)),
                (2480, Some(SessionRole::SessionManager)),
                (3336, Some(SessionRole::MonitorService)),
            ]
        );
        let mut away = facts.clone();
        away.monitor_ancestors = vec![9000];
        assert_eq!(
            session_role(&members[0], &away),
            Some(SessionRole::UserManager)
        );
    }

    #[test]
    fn what_mutter_does_with_xwayland_is_read_from_where_gnome_shell_runs() {
        let (_, members) = gnome();
        let shell = members[2].clone();
        assert!(keeps_xwayland_on_demand(&shell, UID));
        let mut bare = shell.clone();
        bare.cgroup = Some("/user.slice/user-1000.slice/session-2.scope".into());
        assert!(!keeps_xwayland_on_demand(&bare, UID));
        bare.cgroup = None;
        assert!(!keeps_xwayland_on_demand(&bare, UID));
        let comp = member(70, "cosmic-comp", 60, &user("app.slice/cosmic.service"));
        assert!(!keeps_xwayland_on_demand(&comp, UID));
    }

    /// COSMIC from a greeter: the leader starts `start-cosmic`, which starts
    /// cosmic-session, which starts cosmic-comp, which starts Xwayland.
    fn cosmic(own: bool) -> (SessionFacts, Vec<SessionMember>) {
        let scope = "session-3.scope";
        let members = vec![
            member(55, "start-cosmic", 50, scope),
            member(60, "cosmic-session", 55, scope),
            member(70, "cosmic-comp", 60, scope),
            member(71, "Xwayland", 70, scope),
            member(72, "cosmic-panel", 60, scope),
            member(80, "cosmic-edit", 60, scope),
        ];
        let mut facts = SessionFacts {
            uid: UID,
            monitor_ancestors: vec![9000],
            bus_holders: Some(vec![20, 21]),
            compositor_holders: Some(if own { vec![70] } else { vec![] }),
            x_display_holders: Some(if own { vec![70, 71] } else { vec![] }),
            other_compositor_holders: Some(if own { vec![] } else { vec![70] }),
            other_x_holders: Some(if own { vec![] } else { vec![70, 71] }),
            sessions: Some(vec![session("3", 50, true, own)]),
            ..SessionFacts::default()
        };
        read(&mut facts, &members);
        (facts, members)
    }

    #[test]
    fn a_cosmic_session_keeps_its_compositor_its_x_server_and_its_session() {
        let (facts, m) = cosmic(true);
        let roles: Vec<Option<SessionRole>> = m.iter().map(|x| session_role(x, &facts)).collect();
        assert_eq!(
            roles,
            vec![
                Some(SessionRole::SessionLauncher),
                Some(SessionRole::SessionManager),
                Some(SessionRole::Compositor),
                Some(SessionRole::XServer),
                None,
                None,
            ]
        );
    }

    /// The person signed in to GNOME and to COSMIC at once, with Better
    /// Monitor in GNOME: the COSMIC session's compositor, X server, leader
    /// and session manager are protected by the same facts, as is GNOME's.
    #[test]
    fn a_second_graphical_session_of_the_same_person_is_protected_too() {
        let (mut facts, mut members) = gnome();
        let (cosmic_facts, cosmic_members) = cosmic(false);
        facts.other_compositor_holders = cosmic_facts.other_compositor_holders.clone();
        facts.other_x_holders = cosmic_facts.other_x_holders.clone();
        facts
            .sessions
            .as_mut()
            .unwrap()
            .push(session("3", 50, true, false));
        read(&mut facts, &cosmic_members);
        members.extend(cosmic_members);
        let role = |pid: u32, facts: &SessionFacts| {
            let member = members.iter().find(|m| m.pid == pid).unwrap();
            session_role(member, facts)
        };
        // GNOME, as before.
        assert_eq!(role(2545, &facts), Some(SessionRole::Compositor));
        assert_eq!(role(2408, &facts), Some(SessionRole::SessionLauncher));
        // COSMIC's own parts.
        assert_eq!(role(55, &facts), Some(SessionRole::SessionLauncher));
        assert_eq!(role(60, &facts), Some(SessionRole::SessionManager));
        assert_eq!(role(70, &facts), Some(SessionRole::Compositor));
        assert_eq!(role(71, &facts), Some(SessionRole::XServer));
        assert_eq!(role(72, &facts), None);
        assert_eq!(role(80, &facts), None);
        // A nested compositor someone runs in an app scope is not a session.
        let nested = member(4400, "gnome-shell", 4390, &user("app.slice/run-r1.scope"));
        facts.other_compositor_holders.as_mut().unwrap().push(4400);
        facts.fds_read.insert(4400);
        assert_eq!(session_role(&nested, &facts), None);
        // Without logind, any session scope may be a graphical session's.
        facts.sessions = None;
        assert_eq!(role(70, &facts), Some(SessionRole::Compositor));
    }

    #[test]
    fn a_terminal_its_shell_a_user_service_and_a_flatpak_app_may_be_ended() {
        let (mut facts, _) = gnome();
        let ones = [
            member(
                4000,
                "ptyxis-agent",
                2338,
                &user("app.slice/app-ptyxis.scope"),
            ),
            member(4001, "bash", 4000, &user("app.slice/app-ptyxis.scope")),
            member(
                4100,
                "tracker-miner-f",
                2338,
                &user("session.slice/tracker-miner-fs-3.service"),
            ),
            member(4200, "gvfsd", 2338, &user("app.slice/gvfs-daemon.service")),
            member(
                4300,
                "bwrap",
                4290,
                &user("app.slice/app-flatpak-net.nokyan.Resources-1.scope"),
            ),
            member(
                4301,
                "resources",
                4300,
                &user("app.slice/app-flatpak-net.nokyan.Resources-1.scope"),
            ),
        ];
        read(&mut facts, &ones);
        for one in ones {
            assert_eq!(session_role(&one, &facts), None, "{one:?}");
        }
    }

    /// A process whose descriptors could not be read (a non-dumpable one
    /// such as gpg-agent or ssh-agent here) is not known not to hold a
    /// socket: a matching name protects it.
    #[test]
    fn a_process_whose_descriptors_cannot_be_read_is_not_known_to_hold_nothing() {
        let (mut facts, _) = gnome();
        let hidden_bus = member(2371, "dbus-daemon", 2338, &user("app.slice/x.service"));
        let hidden_shell = member(2546, "gnome-shell", 2338, &user("app.slice/y.service"));
        let agent = member(2372, "gpg-agent", 2338, &user("app.slice/gpg.service"));
        for one in [&hidden_bus, &hidden_shell, &agent] {
            facts.members.insert(one.pid, one.clone());
        }
        assert_eq!(
            session_role(&hidden_bus, &facts),
            Some(SessionRole::MessageBus)
        );
        assert_eq!(
            session_role(&hidden_shell, &facts),
            Some(SessionRole::Compositor)
        );
        assert_eq!(session_role(&agent, &facts), None);
        // Read and holding nothing, the same name is not protected.
        facts.fds_read.insert(2371);
        assert_eq!(session_role(&hidden_bus, &facts), None);
    }

    #[test]
    fn a_gnome_shell_that_is_not_a_sessions_compositor_may_be_stopped() {
        let (mut facts, _) = gnome();
        let nested = member(
            3100671,
            "gnome-shell",
            3100668,
            &user("app.slice/run-r1.scope"),
        );
        facts.fds_read.insert(nested.pid);
        assert_eq!(session_role(&nested, &facts), None);
    }

    #[test]
    fn an_unknown_never_makes_a_matching_process_safe() {
        let unknown = SessionFacts {
            uid: UID,
            ..SessionFacts::default()
        };
        let blind = |pid: u32, name: &str| SessionMember {
            pid,
            name: name.into(),
            parent: Some(1),
            cgroup: None,
        };
        for (one, role) in [
            (blind(10, "systemd"), SessionRole::UserManager),
            (blind(11, "better-monitor-"), SessionRole::MonitorService),
            (blind(12, "dbus-daemon"), SessionRole::MessageBus),
            (blind(13, "dbus-broker-lau"), SessionRole::MessageBus),
            (blind(14, "gnome-shell"), SessionRole::Compositor),
            (blind(15, "cosmic-comp"), SessionRole::Compositor),
            (blind(16, "Xwayland"), SessionRole::XServer),
            (blind(17, "gdm-wayland-ses"), SessionRole::SessionLauncher),
            (blind(18, "gnome-session-b"), SessionRole::SessionManager),
        ] {
            assert_eq!(session_role(&one, &unknown), Some(role), "{one:?}");
        }
        assert_eq!(session_role(&blind(19, "make"), &unknown), None);
        // When the facts were read for that process, a name alone does not
        // protect it.
        let (mut facts, _) = gnome();
        facts.fds_read.insert(20);
        let mut known = blind(20, "dbus-daemon");
        known.cgroup =
            Some("/user.slice/user-1000.slice/user@1000.service/app.slice/a.scope".into());
        assert_eq!(session_role(&known, &facts), None);
    }

    #[test]
    fn logind_that_cannot_be_asked_is_unknown_and_not_no_session() {
        let (mut facts, members) = gnome();
        facts.sessions = None;
        assert_eq!(
            session_role(&members[4], &facts),
            Some(SessionRole::SessionLauncher),
            "gdm-wayland-session by name"
        );
        facts.sessions = Some(vec![]);
        assert_eq!(session_role(&members[4], &facts), None, "logind says none");
    }

    #[test]
    fn the_facts_list_every_holder_once() {
        let (facts, _) = gnome();
        assert_eq!(
            facts.holders().into_iter().collect::<Vec<_>>(),
            vec![2338, 2370, 2545, 2600]
        );
    }

    #[test]
    fn a_truncated_command_name_matches_its_full_name_and_nothing_shorter() {
        assert!(comm_is("gnome-session-b", "gnome-session-binary"));
        assert!(!comm_is("gnome-session", "gnome-session-binary"));
        assert!(comm_is("systemd", "systemd"));
        assert!(!comm_is("systemd-oomd", "systemd"));
    }

    #[test]
    fn every_name_has_a_reason() {
        for entry in NAMED {
            assert!(!entry.reason.is_empty(), "{}", entry.name);
        }
        let alone: Vec<&str> = NAMED
            .iter()
            .filter(|entry| entry.by_name_alone)
            .map(|entry| entry.name)
            .collect();
        assert_eq!(alone, ["gnome-session-binary", "cosmic-session"]);
    }

    #[test]
    fn a_tree_is_refused_by_its_root_or_by_a_protected_process_below_it() {
        let (mut facts, members) = gnome();
        assert!(matches!(
            tree_refusal(2338, &members, &facts),
            Some(crate::ActionRefusal::SessionProcess {
                pid: 2338,
                role: SessionRole::Monitor,
                ..
            })
        ));
        let launcher = [members[4].clone(), members[5].clone()];
        assert!(matches!(
            tree_refusal(2408, &launcher, &facts),
            Some(crate::ActionRefusal::SessionProcess {
                role: SessionRole::SessionLauncher,
                ..
            })
        ));
        let wrapper = member(5000, "sh", 2338, &user("app.slice/run-r9.scope"));
        let below = [
            member(
                5001,
                "gnome-session-b",
                5000,
                &user("app.slice/run-r9.scope"),
            ),
            wrapper.clone(),
        ];
        read(&mut facts, &below);
        assert_eq!(
            tree_refusal(5000, &below, &facts),
            Some(crate::ActionRefusal::ProtectedTree {
                pid: 5001,
                name: "gnome-session-b".into(),
                role: SessionRole::SessionManager,
            })
        );
        let terminal = [
            member(6002, "make", 6001, &user("app.slice/app-ptyxis-6000.scope")),
            member(6001, "bash", 6000, &user("app.slice/app-ptyxis-6000.scope")),
            member(
                6000,
                "ptyxis",
                2338,
                &user("app.slice/app-ptyxis-6000.scope"),
            ),
        ];
        read(&mut facts, &terminal);
        assert_eq!(tree_refusal(6000, &terminal, &facts), None);
        assert_eq!(tree_refusal(6000, &[wrapper], &facts), None);
    }

    #[test]
    fn every_action_that_ends_a_process_is_guarded_and_nothing_else() {
        use crate::ProcessAction as A;
        assert!(guards(A::Terminate));
        assert!(guards(A::ForceStop));
        assert!(!guards(A::Pause));
        assert!(!guards(A::Resume));
        assert!(!guards(A::SetNice(10)));
    }
}
