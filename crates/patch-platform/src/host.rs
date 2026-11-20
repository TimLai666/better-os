//! The host the entries run against on a real machine, or a fixture one.

use patch_core::{Answer, Change, ChangeError, Host, Read, ReadError};

use crate::command::{CommandRunner, RunError, SystemCommands};
use crate::display::{DisplayScales, MutterDisplay};
use crate::flatpak;
use crate::paths::Paths;

/// Answers reads and makes changes through its seams.
pub struct LinuxHost {
    paths: Paths,
    commands: Box<dyn CommandRunner>,
    display: Box<dyn DisplayScales>,
    session_type: Option<String>,
}

impl LinuxHost {
    pub fn new(
        paths: Paths,
        commands: Box<dyn CommandRunner>,
        display: Box<dyn DisplayScales>,
        session_type: Option<String>,
    ) -> Self {
        Self {
            paths,
            commands,
            display,
            session_type,
        }
    }

    /// The person's own machine: their files, the real programs, Mutter,
    /// and this session's type. `None` without a home directory.
    pub fn for_user() -> Option<Self> {
        Some(Self::new(
            Paths::for_user()?,
            Box::new(SystemCommands),
            Box::new(MutterDisplay),
            std::env::var("XDG_SESSION_TYPE")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        ))
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }
}

fn read_error(error: RunError) -> ReadError {
    match error {
        RunError::NotInstalled(program) => ReadError::NotInstalled(program),
        RunError::TimedOut => ReadError::TimedOut,
        RunError::Failed(reason) => ReadError::Failed(reason),
    }
}

impl Host for LinuxHost {
    fn read(&mut self, read: &Read) -> Answer {
        match read {
            Read::Command(command) => {
                if command.changes_the_machine() {
                    return Answer::Output(Err(ReadError::Failed(format!(
                        "{} changes the machine and is not a read",
                        command.program()
                    ))));
                }
                Answer::Output(match self.commands.run(command) {
                    Ok(output) if output.success => Ok(output.stdout),
                    Ok(output) => Err(ReadError::Failed(output.failure(command.program()))),
                    Err(error) => Err(read_error(error)),
                })
            }
            Read::SessionType => Answer::SessionType(self.session_type.clone()),
            Read::DisplayScales => Answer::Scales(self.display.scales()),
            Read::FlatpakApps => Answer::Apps(flatpak::installed_apps(&self.paths)),
            Read::UserOverride(app) => Answer::File(flatpak::read_user_override(&self.paths, app)),
        }
    }

    fn change(&mut self, change: &Change) -> Result<(), ChangeError> {
        match change {
            Change::Run(command) => {
                if !command.changes_the_machine() {
                    return Err(ChangeError::Refused(format!(
                        "{} is a read, not a change",
                        command.program()
                    )));
                }
                match self.commands.run(command) {
                    Ok(output) if output.success => Ok(()),
                    Ok(output) => Err(ChangeError::Failed(output.failure(command.program()))),
                    Err(RunError::NotInstalled(program)) => Err(ChangeError::NotInstalled(program)),
                    Err(RunError::TimedOut) => Err(ChangeError::TimedOut),
                    Err(RunError::Failed(reason)) => Err(ChangeError::Failed(reason)),
                }
            }
            Change::WriteUserOverride {
                app,
                expected,
                contents,
            } => flatpak::write_user_override(
                &self.paths,
                app,
                expected.as_deref(),
                contents.as_deref(),
            ),
        }
    }
}
