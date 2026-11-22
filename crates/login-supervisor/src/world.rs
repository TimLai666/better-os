//! The loop around the rules: look, decide, act, remember. Everything the
//! supervisor touches is behind [`World`], so the same loop runs against the
//! machine (`system::SystemWorld`) and against a fake with a fake clock.

use crate::rules::{self, Action, Loaded, Machine, Micros, Needs, Phase, Seen, State, UnitStart};

/// The process edges.
pub trait World {
    /// `CLOCK_MONOTONIC`.
    fn now(&mut self) -> Micros;
    /// One look at everything the rules read, and at what they asked for.
    fn observe(&mut self, needs: &Needs) -> Seen;
    /// Carries out one action. An error is logged; the rules see its
    /// effect, or its absence, at the next look.
    fn perform(&mut self, action: &Action) -> Result<(), String>;
    /// Writes the state file, whole and atomically.
    fn save(&mut self, state: &State) -> Result<(), String>;
    /// `sd_notify`: `READY=1`, `WATCHDOG=1`, `STATUS=…`.
    fn notify(&mut self, message: &str);
    /// A line for the journal.
    fn log(&mut self, line: &str);
}

/// The supervisor: the machine, and what it last wrote.
pub struct Supervisor {
    machine: Machine,
    saved: Option<State>,
    status: Option<String>,
}

impl Supervisor {
    /// Starts from what the state file held: a new boot, the supervisor's
    /// own restart in the middle of an appearance, a display manager
    /// started again, or a file that could not be read. Says `READY=1`
    /// once the state is decided, so systemd's watchdog covers everything
    /// after.
    pub fn start(
        world: &mut dyn World,
        loaded: Loaded,
        boot_id: Option<String>,
        unit: UnitStart,
    ) -> Supervisor {
        let now = world.now();
        let (machine, actions) = rules::start(loaded, boot_id, unit, now);
        let mut supervisor = Supervisor {
            machine,
            saved: None,
            status: None,
        };
        supervisor.perform(world, actions);
        supervisor.save(world);
        world.notify("READY=1");
        supervisor
    }

    pub fn state(&self) -> &State {
        &self.machine.state
    }

    /// One look and what follows from it.
    pub fn tick(&mut self, world: &mut dyn World) {
        world.notify("WATCHDOG=1");
        let seen = world.observe(&self.machine.needs());
        let now = world.now();
        let actions = self.machine.step(now, &seen);
        self.perform(world, actions);
        self.save(world);
        let status = status_line(&self.machine.state);
        if self.status.as_deref() != Some(status.as_str()) {
            world.notify(&format!("STATUS={status}"));
            self.status = Some(status);
        }
    }

    fn perform(&mut self, world: &mut dyn World, actions: Vec<Action>) {
        for action in actions {
            if let Action::Journal(line) = &action {
                world.log(line);
                continue;
            }
            if let Err(error) = world.perform(&action) {
                world.log(&format!("{action:?} failed: {error}"));
            }
        }
    }

    fn save(&mut self, world: &mut dyn World) {
        if self.saved.as_ref() == Some(&self.machine.state) {
            return;
        }
        match world.save(&self.machine.state) {
            Ok(()) => self.saved = Some(self.machine.state.clone()),
            // /run full or not writable: the journal has it, and the next
            // look tries again.
            Err(error) => world.log(&format!("the state file was not written: {error}")),
        }
    }
}

/// What systemd shows as the unit's status line.
pub fn status_line(state: &State) -> String {
    match state.phase {
        Phase::Checking { .. } => "checking the graphics device, the seat and logind".to_string(),
        Phase::Attempt { number, .. } => format!("starting the login screen, attempt {number}"),
        Phase::Stopping { .. } => "stopping the login screen".to_string(),
        Phase::Ready { .. } => "the login screen is showing".to_string(),
        Phase::Session { .. } => "someone is signed in".to_string(),
        Phase::FallingBack { .. } => "starting the text login".to_string(),
        Phase::TextLogin {
            tty,
            confirmed: true,
            ..
        } => format!("the text login runs on {}", tty.name()),
        Phase::TextLogin { tty, .. } => format!("starting the text login on {}", tty.name()),
        Phase::Unreadable => "the state file could not be read".to_string(),
    }
}
