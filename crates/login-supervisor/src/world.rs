//! The loop around the rules: look, decide, act, remember. Everything the
//! supervisor touches is behind [`World`], so the same loop runs against the
//! machine (`system::SystemWorld`) and against a fake with a fake clock.

use crate::rules::{self, Action, Loaded, Machine, Micros, Phase, Seen, State};

/// The process edges.
pub trait World {
    /// `CLOCK_MONOTONIC`.
    fn now(&mut self) -> Micros;
    /// One look at everything the rules read.
    fn observe(&mut self) -> Seen;
    /// Carries out one action. An error is logged; only the ones the rules
    /// act on change what happens next.
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
    /// Starts from what the state file held: a new boot, a restart in the
    /// middle of an appearance, or a file that could not be read. Says
    /// `READY=1` once the state is decided, so systemd's watchdog covers
    /// everything after.
    pub fn start(
        world: &mut dyn World,
        loaded: Loaded,
        boot_id: Option<String>,
        first_start: Option<Micros>,
    ) -> Supervisor {
        let now = world.now();
        let (machine, actions) = rules::start(loaded, boot_id, first_start, now);
        let mut supervisor = Supervisor {
            machine,
            saved: None,
            status: None,
        };
        let seen = Seen::default();
        supervisor.perform(world, actions, &seen);
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
        let seen = world.observe();
        let now = world.now();
        let actions = self.machine.step(now, &seen);
        self.perform(world, actions, &seen);
        self.save(world);
        let status = status_line(&self.machine.state);
        if self.status.as_deref() != Some(status.as_str()) {
            world.notify(&format!("STATUS={status}"));
            self.status = Some(status);
        }
    }

    fn perform(&mut self, world: &mut dyn World, actions: Vec<Action>, seen: &Seen) {
        let mut queue = actions;
        let mut index = 0;
        while index < queue.len() {
            let action = queue[index].clone();
            index += 1;
            if let Action::Journal(line) = &action {
                world.log(line);
                continue;
            }
            if let Err(error) = world.perform(&action) {
                world.log(&format!("{action:?} failed: {error}"));
                let now = world.now();
                if let Some(replacement) = self.machine.failed(now, seen, &action) {
                    queue = replacement;
                    index = 0;
                }
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
        Phase::NoScreen { .. } => "the login screen is waiting for a screen".to_string(),
        Phase::Session { .. } => "someone is signed in".to_string(),
        Phase::FallingBack { .. } => "starting the text login".to_string(),
        Phase::FellBack { tty, .. } => format!("the text login runs on {}", tty.name()),
        Phase::Unreadable => "the state file could not be read".to_string(),
    }
}
