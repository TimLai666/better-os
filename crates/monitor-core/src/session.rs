//! What the desktop session stands on, so no End task, End process tree, or
//! Force stop from Better Monitor ever ends it (owner, 2026-11-22: none of
//! the three is offered on these processes).
//!
//! Most processes are fine to end, by asking (`SIGTERM`) or at once
//! (`SIGKILL`), including every ordinary user service, a terminal and its
//! shell, and a Flatpak application. A few are not, because the session
//! ends with them whichever signal it is: the compositor, the session's
//! message bus, the user's service manager, the session manager, what logind
//! records as the session's leader, the X server when the compositor treats
//! it as mandatory, and Better Monitor itself with what it runs under.
//!
//! Which process plays each part is decided from what the system records
//! about it first: who holds the session bus's and the Wayland display's
//! listening sockets, the cgroup systemd put a process in, and logind's
//! record of the display session. A name is used only where nothing else
//! says it ([`NAMED`], every entry with its reason), or where the fact that
//! would have decided could not be read: an unknown never makes a process
//! that matches part of the rule safe to kill.
//!
//! This module only decides. `monitor-actions-linux` reads the facts.

use serde::{Deserialize, Serialize};

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
    /// The Wayland compositor: the desktop itself.
    Compositor,
    /// The X server, when the compositor ends with it.
    XServer,
    /// What logind records as the display session's leader, or what that
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

/// What the session's own records say. Every `Option` is `None` when the
/// record could not be read, which is not the same as "no such process".
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionFacts {
    /// The user the window runs as.
    pub uid: u32,
    /// Better Monitor's own process and every process above it.
    pub monitor_ancestors: Vec<u32>,
    /// The processes holding the session bus's listening socket.
    pub bus_holders: Option<Vec<u32>>,
    /// The processes holding the Wayland display's listening socket.
    pub compositor_holders: Option<Vec<u32>>,
    /// The processes holding the X display's listening socket; empty when
    /// the session has no X display.
    pub x_display_holders: Option<Vec<u32>>,
    /// The compositor is known to start Xwayland on demand and to survive
    /// it ending ([`xwayland_on_demand`]). `false` when that is not known.
    pub xwayland_on_demand: bool,
    /// logind's leader of the display session: `Some(None)` when the user has
    /// no display session.
    pub session_leader: Option<Option<u32>>,
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
        reason: "a message bus, when the holders of the session bus's socket cannot be read",
    },
    NamedProcess {
        name: "dbus-broker",
        role: SessionRole::MessageBus,
        by_name_alone: false,
        reason: "a message bus, when the holders of the session bus's socket cannot be read",
    },
    NamedProcess {
        name: "dbus-broker-launch",
        role: SessionRole::MessageBus,
        by_name_alone: false,
        reason: "dbus-broker's launcher holds the bus socket, when its holders cannot be read",
    },
    NamedProcess {
        name: "gnome-shell",
        role: SessionRole::Compositor,
        by_name_alone: false,
        reason: "GNOME's compositor, when the holder of the Wayland display socket cannot be read; also the one compositor whose Xwayland policy is known (mutter: on demand inside a systemd user unit)",
    },
    NamedProcess {
        name: "cosmic-comp",
        role: SessionRole::Compositor,
        by_name_alone: false,
        reason: "COSMIC's compositor, when the holder of the Wayland display socket cannot be read",
    },
    NamedProcess {
        name: "Xwayland",
        role: SessionRole::XServer,
        by_name_alone: false,
        reason: "the X server, when the holders of the X display socket cannot be read",
    },
    NamedProcess {
        name: "gdm-wayland-session",
        role: SessionRole::SessionLauncher,
        by_name_alone: false,
        reason: "what GDM starts a Wayland session with, when logind's record of the session cannot be read",
    },
    NamedProcess {
        name: "gdm-x-session",
        role: SessionRole::SessionLauncher,
        by_name_alone: false,
        reason: "what GDM starts an X session with, when logind's record of the session cannot be read",
    },
];

/// The unit Better Monitor's service runs as (`packaging/monitor/`).
pub const MONITOR_SERVICE_UNIT: &str = "better-monitor.service";

/// Whether a kernel command name is `name`. The kernel keeps the first 15
/// bytes of a program's name, so `gnome-session-binary` reads as
/// `gnome-session-b`.
pub fn comm_is(comm: &str, name: &str) -> bool {
    const COMM_LENGTH: usize = 15;
    if name.len() <= COMM_LENGTH {
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

/// Whether the compositor is known to start Xwayland only on demand and to
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
pub fn xwayland_on_demand(compositors: &[SessionMember], uid: u32) -> bool {
    !compositors.is_empty()
        && compositors.iter().all(|member| {
            comm_is(&member.name, "gnome-shell")
                && member
                    .cgroup
                    .as_deref()
                    .is_some_and(|cgroup| in_user_unit(cgroup, uid))
        })
}

/// The part `member` plays in the session, or `None` when the session does
/// not stand on it and it may be force stopped.
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
    let held = |holders: &Option<Vec<u32>>, role: SessionRole| match holders {
        Some(holders) => holders.contains(&member.pid),
        None => named(member, role, false),
    };
    if held(&facts.bus_holders, SessionRole::MessageBus) {
        return Some(SessionRole::MessageBus);
    }
    if held(&facts.compositor_holders, SessionRole::Compositor) {
        return Some(SessionRole::Compositor);
    }
    if !facts.xwayland_on_demand && held(&facts.x_display_holders, SessionRole::XServer) {
        return Some(SessionRole::XServer);
    }
    let launcher = match facts.session_leader {
        Some(Some(leader)) => member.pid == leader || member.parent == Some(leader),
        Some(None) => false,
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

    /// A GNOME 46 session as Ubuntu 24.04 and Zorin 18 run it, with the
    /// PIDs and cgroups read on the owner's desktop on 2026-11-22.
    fn gnome() -> (SessionFacts, Vec<SessionMember>) {
        let facts = SessionFacts {
            uid: UID,
            monitor_ancestors: vec![9000, 2338],
            bus_holders: Some(vec![2338, 2370]),
            compositor_holders: Some(vec![2545]),
            x_display_holders: Some(vec![2545, 2600]),
            xwayland_on_demand: true,
            session_leader: Some(Some(2321)),
        };
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
        // The user manager is recognised by its cgroup even when it is not
        // above the window.
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
        assert!(xwayland_on_demand(std::slice::from_ref(&shell), UID));
        // A gnome-shell outside a user unit runs X11 as mandatory.
        let mut bare = shell.clone();
        bare.cgroup = Some("/user.slice/user-1000.slice/session-2.scope".into());
        assert!(!xwayland_on_demand(std::slice::from_ref(&bare), UID));
        // An unreadable cgroup is not proof.
        bare.cgroup = None;
        assert!(!xwayland_on_demand(&[bare], UID));
        // Nor is another compositor, or none.
        let comp = member(70, "cosmic-comp", 60, &user("app.slice/cosmic.service"));
        assert!(!xwayland_on_demand(&[comp], UID));
        assert!(!xwayland_on_demand(&[], UID));
    }

    /// A COSMIC session from a greeter: the leader's command starts
    /// cosmic-session, which starts cosmic-comp, which starts Xwayland.
    #[test]
    fn a_cosmic_session_keeps_its_compositor_its_x_server_and_its_session() {
        let facts = SessionFacts {
            uid: UID,
            monitor_ancestors: vec![9000],
            bus_holders: Some(vec![20, 21]),
            compositor_holders: Some(vec![70]),
            x_display_holders: Some(vec![70, 71]),
            xwayland_on_demand: false,
            session_leader: Some(Some(50)),
        };
        let scope = "session-3.scope";
        let start = member(55, "start-cosmic", 50, scope);
        let session = member(60, "cosmic-session", 55, scope);
        let comp = member(70, "cosmic-comp", 60, scope);
        let x = member(71, "Xwayland", 70, scope);
        let panel = member(72, "cosmic-panel", 60, scope);
        let app = member(80, "cosmic-edit", 60, scope);
        assert_eq!(
            session_role(&start, &facts),
            Some(SessionRole::SessionLauncher)
        );
        assert_eq!(
            session_role(&session, &facts),
            Some(SessionRole::SessionManager)
        );
        assert_eq!(session_role(&comp, &facts), Some(SessionRole::Compositor));
        assert_eq!(session_role(&x, &facts), Some(SessionRole::XServer));
        // The panel and an application in the same scope are not the session.
        assert_eq!(session_role(&panel, &facts), None);
        assert_eq!(session_role(&app, &facts), None);
    }

    #[test]
    fn a_terminal_its_shell_a_user_service_and_a_flatpak_app_may_be_force_stopped() {
        let (facts, _) = gnome();
        // Every one of these leads its own session; that is not the rule.
        for one in [
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
        ] {
            assert_eq!(session_role(&one, &facts), None, "{one:?}");
        }
    }

    #[test]
    fn a_gnome_shell_that_is_not_this_sessions_compositor_may_be_stopped() {
        // A nested shell another program started, in its own scope.
        let (facts, _) = gnome();
        let nested = member(
            3100671,
            "gnome-shell",
            3100668,
            &user("app.slice/run-r1.scope"),
        );
        assert_eq!(session_role(&nested, &facts), None);
    }

    #[test]
    fn an_unknown_never_makes_a_matching_process_safe() {
        let unknown = SessionFacts {
            uid: UID,
            monitor_ancestors: vec![],
            bus_holders: None,
            compositor_holders: None,
            x_display_holders: None,
            xwayland_on_demand: false,
            session_leader: None,
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
        // Something no part of the rule names stays allowed.
        assert_eq!(session_role(&blind(19, "make"), &unknown), None);
        // When the facts were read, a name alone does not protect.
        let (facts, _) = gnome();
        assert_eq!(session_role(&blind(20, "dbus-daemon"), &facts), None);
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
        // Only the session managers are decided by name alone.
        let alone: Vec<&str> = NAMED
            .iter()
            .filter(|entry| entry.by_name_alone)
            .map(|entry| entry.name)
            .collect();
        assert_eq!(alone, ["gnome-session-binary", "cosmic-session"]);
    }

    #[test]
    fn a_tree_is_refused_by_its_root_or_by_a_protected_process_below_it() {
        let (facts, members) = gnome();
        // The user manager's tree: the root itself.
        assert!(matches!(
            tree_refusal(2338, &members, &facts),
            Some(crate::ActionRefusal::SessionProcess {
                pid: 2338,
                role: SessionRole::Monitor,
                ..
            })
        ));
        // GDM's launcher, whose tree holds the session manager.
        let launcher = [members[4].clone(), members[5].clone()];
        assert!(matches!(
            tree_refusal(2408, &launcher, &facts),
            Some(crate::ActionRefusal::SessionProcess {
                role: SessionRole::SessionLauncher,
                ..
            })
        ));
        // A plain process that started something the session stands on.
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
        assert_eq!(
            tree_refusal(5000, &below, &facts),
            Some(crate::ActionRefusal::ProtectedTree {
                pid: 5001,
                name: "gnome-session-b".into(),
                role: SessionRole::SessionManager,
            })
        );
        // A terminal that is not above the window, with its shell and a build.
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
