//! The host the entries run against on a real machine, or a fixture one.

use patch_core::{Answer, Change, ChangeError, Host, Read, ReadError};

use crate::command::{CommandRunner, RunError, SystemCommands};
use crate::display::DisplayScales;
use crate::flatpak;
use crate::paths::Paths;
use crate::usb::{NoUsbDaemon, SystemUsbDaemon, UsbDaemon};

/// Answers reads and makes changes through its seams.
pub struct LinuxHost {
    paths: Paths,
    commands: Box<dyn CommandRunner>,
    display: Box<dyn DisplayScales>,
    session_type: Option<String>,
    /// Where the machine's `/sys`, `/proc`, `/etc`, and `/var` are for the
    /// USB-C entries, `None` until [`LinuxHost::with_usb`] says.
    usb_root: Option<std::path::PathBuf>,
    usb_daemon: Box<dyn UsbDaemon>,
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
            usb_root: None,
            usb_daemon: Box::new(NoUsbDaemon),
        }
    }

    /// The machine the USB-C entries read (`/` on a real one) and the
    /// daemon they ask. A host built without it answers both reads with
    /// "not on this desktop" and refuses every privileged change, so no
    /// test reaches the person's machine or the system bus by default.
    pub fn with_usb(
        mut self,
        root: impl Into<std::path::PathBuf>,
        daemon: Box<dyn UsbDaemon>,
    ) -> Self {
        self.usb_root = Some(root.into());
        self.usb_daemon = daemon;
        self
    }

    /// One typed daemon action, with what a recovery restored (the page's
    /// Reset USB-C, which reports it).
    pub fn usb_act(
        &mut self,
        action: patch_core::Privileged,
    ) -> Result<Option<hardware_core::usb_recovery::RecoveryReply>, ChangeError> {
        self.usb_daemon.act(action)
    }

    /// The daemon's USB-C choices, as its file says (none until one is
    /// made, or when the host has no USB-C machine).
    pub fn usb_settings(&self) -> hardware_core::usb_recovery::UsbRecoverySettings {
        self.usb_root
            .as_ref()
            .map(|root| crate::usb::machine(root).settings)
            .unwrap_or_default()
    }

    fn usb_findings(&self) -> Result<hardware_core::usb_recovery::UsbFindings, ReadError> {
        let Some(root) = &self.usb_root else {
            return Err(ReadError::NotOnThisDesktop);
        };
        if !crate::usb::journal_readable(root) {
            return self.usb_daemon.findings();
        }
        let mut last = None;
        for grep in [true, false] {
            let command = patch_core::Command::KernelUsbLog { grep };
            match self.commands.run(&command) {
                Ok(output) if output.success => {
                    return Ok(hardware_core::usb_recovery::classify(&output.stdout));
                }
                Ok(output) => last = Some(ReadError::Failed(output.failure(command.program()))),
                Err(error) => {
                    last = Some(read_error(error));
                    break;
                }
            }
        }
        // The person's read failed: the daemon may still answer.
        self.usb_daemon
            .findings()
            .map_err(|daemon| last.unwrap_or(daemon))
    }

    /// The person's own machine: their files, the real programs, Mutter,
    /// and this session's type. `None` without a home directory.
    pub fn for_user() -> Option<Self> {
        Some(
            Self::new(
                Paths::for_user()?,
                Box::new(SystemCommands),
                crate::display::for_desktop(&better_core::CurrentDesktop::from_env()),
                std::env::var("XDG_SESSION_TYPE")
                    .ok()
                    .filter(|value| !value.trim().is_empty()),
            )
            .with_usb("/", Box::new(SystemUsbDaemon)),
        )
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
            Read::UsbMachine => Answer::UsbMachine(match &self.usb_root {
                Some(root) => Ok(crate::usb::machine(root)),
                None => Err(ReadError::NotOnThisDesktop),
            }),
            Read::UsbFindings => Answer::UsbFindings(self.usb_findings()),
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
            Change::Privileged(action) => self.usb_daemon.act(*action).map(|_| ()),
        }
    }
}
