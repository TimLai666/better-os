//! Starting an application from its registered desktop definition.
//!
//! Nothing here ever builds a command string. A plan arrives as argument
//! vectors or as a D-Bus activation, and the spawner hands the vector to
//! `execvp` through `std::process::Command`. A terminal application is wrapped
//! by prepending the terminal emulator's own argument vector, not by pasting
//! the command into `sh -c`.

use std::process::{Command, Stdio};
use std::sync::Mutex;

use app_catalog_core::{
    ApplicationRecord, DBusActivation, Invocation, LaunchPlan, LaunchTarget, Locale,
};

use crate::PlatformError;

/// The terminal emulator used for `Terminal=true` entries. The separator
/// arguments are what the emulator wants before the command it should run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalCommand {
    pub program: String,
    pub separator: Vec<String>,
}

impl Default for TerminalCommand {
    fn default() -> Self {
        Self {
            program: "x-terminal-emulator".to_string(),
            separator: vec!["-e".to_string()],
        }
    }
}

impl TerminalCommand {
    /// Wraps an invocation so it runs inside the terminal, as one argument
    /// vector.
    pub fn wrap(&self, invocation: &Invocation) -> Invocation {
        let mut arguments = self.separator.clone();
        arguments.push(invocation.program.clone());
        arguments.extend(invocation.arguments.iter().cloned());
        Invocation {
            program: self.program.clone(),
            arguments,
        }
    }
}

/// Starts one process. A trait so a launch can be asserted without starting
/// anything.
pub trait ProcessSpawner {
    fn spawn(&self, invocation: &Invocation) -> Result<(), PlatformError>;
}

/// Starts processes for real, detached from this process's standard streams.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSpawner;

impl ProcessSpawner for SystemSpawner {
    fn spawn(&self, invocation: &Invocation) -> Result<(), PlatformError> {
        Command::new(&invocation.program)
            .args(&invocation.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| PlatformError::SpawnFailed {
                program: invocation.program.clone(),
                reason: error.kind().to_string(),
            })
    }
}

/// Records what it was asked to start instead of starting it. This is the
/// launch smoke seam: a test reads back the exact argument vector and can
/// prove no shell interpretation happened.
#[derive(Debug, Default)]
pub struct RecordingSpawner {
    calls: Mutex<Vec<Invocation>>,
}

impl RecordingSpawner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calls(&self) -> Vec<Invocation> {
        self.calls.lock().expect("spawner lock").clone()
    }
}

impl ProcessSpawner for RecordingSpawner {
    fn spawn(&self, invocation: &Invocation) -> Result<(), PlatformError> {
        self.calls
            .lock()
            .expect("spawner lock")
            .push(invocation.clone());
        Ok(())
    }
}

/// Activates a `DBusActivatable` application over the session bus.
pub trait DesktopActivator {
    fn activate(&self, activation: &DBusActivation) -> Result<(), PlatformError>;
}

/// Records activations instead of performing them.
#[derive(Debug, Default)]
pub struct RecordingActivator {
    calls: Mutex<Vec<DBusActivation>>,
}

impl RecordingActivator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calls(&self) -> Vec<DBusActivation> {
        self.calls.lock().expect("activator lock").clone()
    }
}

impl DesktopActivator for RecordingActivator {
    fn activate(&self, activation: &DBusActivation) -> Result<(), PlatformError> {
        self.calls
            .lock()
            .expect("activator lock")
            .push(activation.clone());
        Ok(())
    }
}

/// What a launch actually did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchOutcome {
    /// `processes` processes were started. An entry declaring `%f` handed
    /// three files starts three.
    Started {
        processes: usize,
    },
    Activated,
    /// D-Bus activation was requested but unavailable, so the entry's own
    /// `Exec` line was used instead. The specification requires that line to
    /// exist for exactly this case.
    ActivationFellBackToProcess {
        processes: usize,
    },
}

/// Launches applications through their desktop definitions.
pub struct Launcher<'a> {
    spawner: &'a dyn ProcessSpawner,
    activator: Option<&'a dyn DesktopActivator>,
    terminal: TerminalCommand,
}

impl<'a> Launcher<'a> {
    pub fn new(spawner: &'a dyn ProcessSpawner) -> Self {
        Self {
            spawner,
            activator: None,
            terminal: TerminalCommand::default(),
        }
    }

    pub fn with_activator(mut self, activator: &'a dyn DesktopActivator) -> Self {
        self.activator = Some(activator);
        self
    }

    pub fn with_terminal(mut self, terminal: TerminalCommand) -> Self {
        self.terminal = terminal;
        self
    }

    /// Launches a record, optionally through one of its declared actions.
    pub fn launch(
        &self,
        record: &ApplicationRecord,
        action_id: Option<&str>,
        targets: &[LaunchTarget],
        locale: Option<&Locale>,
    ) -> Result<LaunchOutcome, PlatformError> {
        let plan = record.launch_plan(action_id, targets, locale)?;
        match plan {
            LaunchPlan::Activation(activation) => match self.activator {
                Some(activator) => {
                    activator.activate(&activation)?;
                    Ok(LaunchOutcome::Activated)
                }
                None => {
                    let LaunchPlan::Process {
                        invocations,
                        terminal,
                    } = record.process_fallback(action_id, targets, locale)?
                    else {
                        unreachable!("process_fallback always plans a process");
                    };
                    let processes = self.run(&invocations, terminal)?;
                    Ok(LaunchOutcome::ActivationFellBackToProcess { processes })
                }
            },
            LaunchPlan::Process {
                invocations,
                terminal,
            } => Ok(LaunchOutcome::Started {
                processes: self.run(&invocations, terminal)?,
            }),
        }
    }

    fn run(&self, invocations: &[Invocation], terminal: bool) -> Result<usize, PlatformError> {
        for invocation in invocations {
            if terminal {
                self.spawner.spawn(&self.terminal.wrap(invocation))?;
            } else {
                self.spawner.spawn(invocation)?;
            }
        }
        Ok(invocations.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_catalog_core::{DBusMethod, DesktopFile, DesktopId, EntryScope, NoProbe};
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
    fn a_launch_passes_an_argument_vector_not_a_command_string() {
        let spawner = RecordingSpawner::new();
        let launcher = Launcher::new(&spawner);
        let record = record(
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor --wait %F\n",
        );
        let targets = vec![
            LaunchTarget::path("/tmp/a b.txt").unwrap(),
            LaunchTarget::path("/tmp/$(whoami); rm -rf x.txt").unwrap(),
        ];
        let outcome = launcher.launch(&record, None, &targets, None).unwrap();
        assert_eq!(outcome, LaunchOutcome::Started { processes: 1 });
        let calls = spawner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "editor");
        assert_eq!(
            calls[0].arguments,
            vec!["--wait", "/tmp/a b.txt", "/tmp/$(whoami); rm -rf x.txt"]
        );
    }

    #[test]
    fn a_single_file_entry_starts_one_process_per_file() {
        let spawner = RecordingSpawner::new();
        let launcher = Launcher::new(&spawner);
        let record = record(
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %f\n",
        );
        let targets = vec![
            LaunchTarget::path("/tmp/one.txt").unwrap(),
            LaunchTarget::path("/tmp/two.txt").unwrap(),
        ];
        assert_eq!(
            launcher.launch(&record, None, &targets, None).unwrap(),
            LaunchOutcome::Started { processes: 2 }
        );
        assert_eq!(spawner.calls().len(), 2);
    }

    #[test]
    fn a_terminal_entry_is_wrapped_in_the_terminal_argument_vector() {
        let spawner = RecordingSpawner::new();
        let launcher = Launcher::new(&spawner).with_terminal(TerminalCommand {
            program: "kgx".to_string(),
            separator: vec!["--".to_string()],
        });
        let record = record(
            "htop.desktop",
            "[Desktop Entry]\nType=Application\nName=Htop\nExec=htop -d 5\nTerminal=true\n",
        );
        launcher.launch(&record, None, &[], None).unwrap();
        let calls = spawner.calls();
        assert_eq!(calls[0].program, "kgx");
        assert_eq!(calls[0].arguments, vec!["--", "htop", "-d", "5"]);
    }

    #[test]
    fn a_dbus_activatable_entry_activates_when_an_activator_exists() {
        let spawner = RecordingSpawner::new();
        let activator = RecordingActivator::new();
        let launcher = Launcher::new(&spawner).with_activator(&activator);
        let record = record(
            "org.gnome.Nautilus.desktop",
            "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus %U\nDBusActivatable=true\n",
        );
        let targets = vec![LaunchTarget::path("/tmp/dir").unwrap()];
        assert_eq!(
            launcher.launch(&record, None, &targets, None).unwrap(),
            LaunchOutcome::Activated
        );
        assert!(spawner.calls().is_empty());
        let calls = activator.calls();
        assert_eq!(calls[0].service, "org.gnome.Nautilus");
        assert_eq!(calls[0].method, DBusMethod::Open);
        assert_eq!(calls[0].uris, vec!["file:///tmp/dir"]);
    }

    #[test]
    fn without_an_activator_the_exec_line_is_used_instead() {
        let spawner = RecordingSpawner::new();
        let launcher = Launcher::new(&spawner);
        let record = record(
            "org.gnome.Nautilus.desktop",
            "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus %U\nDBusActivatable=true\n",
        );
        assert_eq!(
            launcher.launch(&record, None, &[], None).unwrap(),
            LaunchOutcome::ActivationFellBackToProcess { processes: 1 }
        );
        assert_eq!(spawner.calls()[0].program, "nautilus");
    }

    #[test]
    fn an_action_is_launched_by_id() {
        let spawner = RecordingSpawner::new();
        let launcher = Launcher::new(&spawner);
        let record = record(
            "browser.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Browser\n\
             Exec=browser %U\n\
             Actions=Private;\n\
             \n\
             [Desktop Action Private]\n\
             Name=Private Window\n\
             Exec=browser --private-window\n",
        );
        launcher
            .launch(&record, Some("Private"), &[], None)
            .unwrap();
        assert_eq!(spawner.calls()[0].arguments, vec!["--private-window"]);
    }
}
