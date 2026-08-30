//! Typed launch plans.
//!
//! A plan is the whole contract between the catalog and whatever actually
//! starts a process. It is either a list of argument vectors or a D-Bus
//! activation call. There is deliberately no variant carrying a command
//! string, so no consumer can hand one to a shell.

use crate::entry::Locale;
use crate::error::LaunchError;
use crate::exec::{Invocation, LaunchTarget};
use crate::record::ApplicationRecord;

/// The method to call on `org.freedesktop.Application`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DBusMethod {
    Activate,
    Open,
    ActivateAction(String),
}

/// A D-Bus activation, addressed by the well-known name the specification
/// derives from the desktop ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DBusActivation {
    pub service: String,
    pub object_path: String,
    pub method: DBusMethod,
    /// URIs to open. Empty for `Activate` and `ActivateAction`.
    pub uris: Vec<String>,
}

/// How to start an application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchPlan {
    /// One process per invocation. `terminal` says the entry declared
    /// `Terminal=true` and must be run inside a terminal emulator.
    Process {
        invocations: Vec<Invocation>,
        terminal: bool,
    },
    Activation(DBusActivation),
}

impl ApplicationRecord {
    /// The plan for launching this record. A `DBusActivatable` entry produces
    /// an activation; everything else produces argument vectors.
    pub fn launch_plan(
        &self,
        action_id: Option<&str>,
        targets: &[LaunchTarget],
        locale: Option<&Locale>,
    ) -> Result<LaunchPlan, LaunchError> {
        if self.capabilities.dbus_activatable {
            if let Some(service) = &self.dbus_service {
                if let Some(id) = action_id {
                    if self.action(id).is_none() {
                        return Err(LaunchError::UnknownAction(id.to_string()));
                    }
                }
                let uris: Vec<String> = targets.iter().map(LaunchTarget::to_uri).collect();
                let (method, uris) = match (action_id, uris.is_empty()) {
                    (Some(id), _) => (DBusMethod::ActivateAction(id.to_string()), Vec::new()),
                    (None, true) => (DBusMethod::Activate, Vec::new()),
                    (None, false) => (DBusMethod::Open, uris),
                };
                return Ok(LaunchPlan::Activation(DBusActivation {
                    service: service.clone(),
                    object_path: object_path_for(service),
                    method,
                    uris,
                }));
            }
        }
        Ok(LaunchPlan::Process {
            invocations: self.build_invocations(action_id, targets, locale)?,
            terminal: self.capabilities.terminal,
        })
    }

    /// The plan to fall back to when D-Bus activation is unavailable. The
    /// specification requires a `DBusActivatable` entry to keep a working
    /// `Exec` line precisely so this fallback exists.
    pub fn process_fallback(
        &self,
        action_id: Option<&str>,
        targets: &[LaunchTarget],
        locale: Option<&Locale>,
    ) -> Result<LaunchPlan, LaunchError> {
        Ok(LaunchPlan::Process {
            invocations: self.build_invocations(action_id, targets, locale)?,
            terminal: self.capabilities.terminal,
        })
    }
}

/// The object path the specification derives from a well-known name.
fn object_path_for(service: &str) -> String {
    let mut path = String::with_capacity(service.len() + 1);
    path.push('/');
    for character in service.chars() {
        match character {
            '.' => path.push('/'),
            '-' => path.push('_'),
            other => path.push(other),
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::DesktopFile;
    use crate::record::{DesktopId, EntryScope, NoProbe};
    use std::path::PathBuf;

    fn record(id: &str, body: &str) -> ApplicationRecord {
        let file = DesktopFile::parse(body).unwrap();
        ApplicationRecord::from_desktop_file(
            DesktopId::new(id).unwrap(),
            PathBuf::from(format!("/usr/share/applications/{id}")),
            EntryScope::System,
            &file,
            &NoProbe,
        )
        .unwrap()
    }

    #[test]
    fn a_plain_entry_plans_argument_vectors() {
        let record = record(
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %F\n",
        );
        let targets = vec![LaunchTarget::path("/tmp/a.txt").unwrap()];
        let plan = record.launch_plan(None, &targets, None).unwrap();
        match plan {
            LaunchPlan::Process {
                invocations,
                terminal,
            } => {
                assert!(!terminal);
                assert_eq!(invocations[0].program, "editor");
                assert_eq!(invocations[0].arguments, vec!["/tmp/a.txt"]);
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn a_terminal_entry_marks_its_plan() {
        let record = record(
            "htop.desktop",
            "[Desktop Entry]\nType=Application\nName=Htop\nExec=htop\nTerminal=true\n",
        );
        assert!(matches!(
            record.launch_plan(None, &[], None).unwrap(),
            LaunchPlan::Process { terminal: true, .. }
        ));
    }

    #[test]
    fn a_dbus_activatable_entry_plans_an_activation() {
        let record = record(
            "org.gnome.Nautilus.desktop",
            "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus %U\nDBusActivatable=true\n",
        );
        let plan = record.launch_plan(None, &[], None).unwrap();
        let LaunchPlan::Activation(activation) = plan else {
            panic!("expected an activation");
        };
        assert_eq!(activation.service, "org.gnome.Nautilus");
        assert_eq!(activation.object_path, "/org/gnome/Nautilus");
        assert_eq!(activation.method, DBusMethod::Activate);
        assert!(activation.uris.is_empty());
    }

    #[test]
    fn activation_with_targets_uses_open_and_sends_uris() {
        let record = record(
            "org.gnome.TextEditor.desktop",
            "[Desktop Entry]\nType=Application\nName=Text\nExec=editor %U\nDBusActivatable=true\n",
        );
        let targets = vec![LaunchTarget::path("/tmp/a b.txt").unwrap()];
        let LaunchPlan::Activation(activation) = record.launch_plan(None, &targets, None).unwrap()
        else {
            panic!("expected an activation");
        };
        assert_eq!(activation.method, DBusMethod::Open);
        assert_eq!(activation.uris, vec!["file:///tmp/a%20b.txt"]);
    }

    #[test]
    fn activation_of_an_action_uses_activate_action() {
        let record = record(
            "org.gnome.Nautilus.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Files\n\
             Exec=nautilus %U\n\
             DBusActivatable=true\n\
             Actions=new-window;\n\
             \n\
             [Desktop Action new-window]\n\
             Name=New Window\n\
             Exec=nautilus --new-window\n",
        );
        let LaunchPlan::Activation(activation) =
            record.launch_plan(Some("new-window"), &[], None).unwrap()
        else {
            panic!("expected an activation");
        };
        assert_eq!(
            activation.method,
            DBusMethod::ActivateAction("new-window".to_string())
        );
        assert_eq!(
            record.launch_plan(Some("nope"), &[], None).unwrap_err(),
            LaunchError::UnknownAction("nope".to_string())
        );
    }

    #[test]
    fn the_process_fallback_uses_the_exec_line() {
        let record = record(
            "org.gnome.Nautilus.desktop",
            "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus %U\nDBusActivatable=true\n",
        );
        let LaunchPlan::Process { invocations, .. } =
            record.process_fallback(None, &[], None).unwrap()
        else {
            panic!("expected a process plan");
        };
        assert_eq!(invocations[0].program, "nautilus");
    }

    #[test]
    fn object_paths_replace_dots_and_dashes() {
        assert_eq!(object_path_for("org.x-y.App"), "/org/x_y/App");
    }
}
