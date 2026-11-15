//! When the touchpad is held, and the guarantee that it is let go.
//!
//! The pass-through holds the touchpad (an exclusive `EVIOCGRAB`) only while a
//! factor other than one is set. At neutral it holds nothing, creates no clone,
//! and reads nothing. Taking and releasing the hold both wait for the pad to be
//! untouched, so the desktop never sees a finger appear mid-gesture or one that
//! never lifts.
//!
//! The hold is released on every way out of the process that runs code:
//! [`Passthrough`] releases in `Drop`, which runs on return, on `?`, and while
//! a panic unwinds. The ways out that run no code (`SIGKILL`, an abort, the
//! watchdog) are covered by the kernel itself: a grab belongs to the open file
//! and a uinput device to its file descriptor, and the kernel ends both when
//! the process's descriptors are closed.

use std::io;
use std::time::Duration;

use crate::caps::DeviceDescriptor;
use crate::event::InputEvent;
use crate::factors::Factors;
use crate::transform::Scaler;

/// The touchpad.
pub trait Source {
    fn descriptor(&self) -> &DeviceDescriptor;
    /// Take or release the exclusive grab.
    fn set_grab(&mut self, grab: bool) -> io::Result<()>;
    /// Wait up to `timeout` for events and append what is there. An empty
    /// read is a timeout.
    fn read(&mut self, timeout: Duration, out: &mut Vec<InputEvent>) -> io::Result<()>;
    /// Throw away whatever is queued for this reader.
    fn drain(&mut self) -> io::Result<()>;
    /// Whether the kernel says something is touching or pressing the pad now.
    fn touching(&mut self) -> io::Result<bool>;
    /// The kernel's current multitouch slot.
    fn current_slot(&mut self) -> io::Result<i32>;
}

/// The clone. Dropping it destroys it.
pub trait Sink {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()>;
}

/// Makes a clone of a device.
pub trait SinkFactory {
    type Sink: Sink;
    fn create(&mut self, device: &DeviceDescriptor) -> io::Result<Self::Sink>;
}

/// What the pass-through is doing, for the log and for tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Neutral factors: nothing held.
    Idle,
    /// A factor is set and the pad is being touched; the hold is taken when
    /// it is not.
    WaitingToHold,
    /// Holding the pad and forwarding through the clone.
    Holding,
    /// Holding, after lost events, discarding until the pad is untouched.
    Resyncing,
}

/// What forwarding cost in this process: from the moment a read returned to
/// the moment the last frame it held was written to the clone.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cost {
    pub batches: u64,
    pub total: Duration,
    pub worst: Duration,
}

impl Cost {
    pub fn mean(&self) -> Duration {
        if self.batches == 0 {
            Duration::ZERO
        } else {
            self.total / self.batches as u32
        }
    }
}

pub struct Passthrough<S: Source, F: SinkFactory> {
    cost: Cost,
    source: S,
    factory: F,
    sink: Option<F::Sink>,
    scaler: Scaler,
    resyncing: bool,
    pending: Vec<InputEvent>,
    read: Vec<InputEvent>,
    out: Vec<InputEvent>,
}

/// How long the loop waits before looking again when there is nothing to do.
pub const IDLE_POLL: Duration = Duration::from_millis(250);

impl<S: Source, F: SinkFactory> Passthrough<S, F> {
    pub fn new(source: S, factory: F) -> Self {
        let scaler = Scaler::new(source.descriptor());
        Self {
            cost: Cost::default(),
            source,
            factory,
            sink: None,
            scaler,
            resyncing: false,
            pending: Vec::new(),
            read: Vec::new(),
            out: Vec::new(),
        }
    }

    pub fn mode(&self, factors: Factors) -> Mode {
        match (&self.sink, self.resyncing, factors.is_neutral()) {
            (Some(_), true, _) => Mode::Resyncing,
            (Some(_), false, _) => Mode::Holding,
            (None, _, true) => Mode::Idle,
            (None, _, false) => Mode::WaitingToHold,
        }
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    /// One turn of the loop. Returns within about `timeout`.
    pub fn step(&mut self, factors: Factors, timeout: Duration) -> io::Result<()> {
        let holding = self.sink.is_some();
        if !holding {
            // Asked every turn, neutral or not: it is also how an idle
            // instance learns its touchpad was unplugged (ENODEV), so it exits
            // instead of holding a dead node a re-plugged pad may be renumbered
            // onto.
            let touching = self.source.touching()?;
            if factors.is_neutral() || touching {
                std::thread::sleep(timeout.min(IDLE_POLL));
                return Ok(());
            }
            return self.hold();
        }
        if factors.is_neutral() && !self.scaler.touching() && self.pending.is_empty() {
            return self.release();
        }
        self.read.clear();
        self.source.read(timeout, &mut self.read)?;
        let events = std::mem::take(&mut self.read);
        let started = std::time::Instant::now();
        for event in &events {
            self.pending.push(*event);
            if event.is_dropped() {
                // The kernel lost events for this reader. Everything up to the
                // next report is incomplete, and so is the contact state: lift
                // everything on the clone and forward nothing until the pad is
                // untouched again.
                self.pending.clear();
                self.resyncing = true;
                self.out.clear();
                self.scaler.lift_all(&mut self.out);
                self.emit()?;
                continue;
            }
            if event.is_report() {
                let frame = std::mem::take(&mut self.pending);
                if self.resyncing {
                    if !self.source.touching()? {
                        self.resyncing = false;
                        self.scaler = Scaler::new(self.source.descriptor());
                        self.out.clear();
                        let slot = self.source.current_slot()?;
                        self.scaler.start(slot, &mut self.out);
                        self.emit()?;
                    }
                    continue;
                }
                self.out.clear();
                self.scaler.frame(&frame, factors, &mut self.out);
                self.emit()?;
            }
        }
        if !events.is_empty() {
            let spent = started.elapsed();
            self.cost.batches += 1;
            self.cost.total += spent;
            self.cost.worst = self.cost.worst.max(spent);
        }
        self.read = events;
        Ok(())
    }

    /// The forwarding cost since the last call, which resets it.
    pub fn take_cost(&mut self) -> Cost {
        std::mem::take(&mut self.cost)
    }

    fn emit(&mut self) -> io::Result<()> {
        if let Some(sink) = self.sink.as_mut()
            && !self.out.is_empty()
        {
            sink.emit(&self.out)?;
        }
        Ok(())
    }

    /// Create the clone, then take the grab. The order means there is never a
    /// moment where the pad is held and nothing receives it.
    fn hold(&mut self) -> io::Result<()> {
        let mut sink = self.factory.create(self.source.descriptor())?;
        self.source.drain()?;
        self.source.set_grab(true)?;
        self.scaler = Scaler::new(self.source.descriptor());
        self.pending.clear();
        self.resyncing = false;
        self.out.clear();
        let slot = self.source.current_slot()?;
        self.scaler.start(slot, &mut self.out);
        if !self.out.is_empty()
            && let Err(error) = sink.emit(&self.out)
        {
            let _ = self.source.set_grab(false);
            return Err(error);
        }
        self.sink = Some(sink);
        Ok(())
    }

    /// Release the grab, then destroy the clone: the pad goes back to libinput
    /// before its stand-in disappears. Anything the clone still has down is
    /// lifted first.
    pub fn release(&mut self) -> io::Result<()> {
        let Some(mut sink) = self.sink.take() else {
            return Ok(());
        };
        if self.scaler.touching() {
            self.out.clear();
            self.scaler.lift_all(&mut self.out);
            let _ = sink.emit(&self.out);
        }
        self.pending.clear();
        self.resyncing = false;
        let result = self.source.set_grab(false);
        drop(sink);
        result
    }
}

impl<S: Source, F: SinkFactory> Drop for Passthrough<S, F> {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

/// A device and a clone that record what happens to them, for tests and the
/// benchmark. Nothing here touches the kernel.
pub mod fake {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// What the fake devices saw, shared with the test that owns them.
    #[derive(Debug, Default)]
    pub struct Journal {
        pub grabbed: bool,
        pub grabs: usize,
        pub releases: usize,
        pub clones_alive: usize,
        pub clones_created: usize,
        pub emitted: Vec<InputEvent>,
    }

    pub type SharedJournal = Arc<Mutex<Journal>>;

    pub struct FakeSource {
        pub device: DeviceDescriptor,
        pub journal: SharedJournal,
        /// Each read hands out the next batch.
        pub batches: VecDeque<Vec<InputEvent>>,
        pub touching: bool,
        pub slot: i32,
        pub drained: usize,
    }

    impl FakeSource {
        pub fn new(device: DeviceDescriptor, journal: SharedJournal) -> Self {
            Self {
                device,
                journal,
                batches: VecDeque::new(),
                touching: false,
                slot: 0,
                drained: 0,
            }
        }
    }

    impl Source for FakeSource {
        fn descriptor(&self) -> &DeviceDescriptor {
            &self.device
        }

        fn set_grab(&mut self, grab: bool) -> io::Result<()> {
            let mut journal = self.journal.lock().unwrap();
            if grab {
                if journal.grabbed {
                    return Err(io::Error::from_raw_os_error(libc::EBUSY));
                }
                journal.grabs += 1;
            } else if journal.grabbed {
                journal.releases += 1;
            }
            journal.grabbed = grab;
            Ok(())
        }

        fn read(&mut self, _timeout: Duration, out: &mut Vec<InputEvent>) -> io::Result<()> {
            if let Some(batch) = self.batches.pop_front() {
                out.extend(batch);
            }
            Ok(())
        }

        fn drain(&mut self) -> io::Result<()> {
            self.drained += 1;
            Ok(())
        }

        fn touching(&mut self) -> io::Result<bool> {
            Ok(self.touching)
        }

        fn current_slot(&mut self) -> io::Result<i32> {
            Ok(self.slot)
        }
    }

    pub struct FakeSink {
        journal: SharedJournal,
    }

    impl Sink for FakeSink {
        fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
            self.journal
                .lock()
                .unwrap()
                .emitted
                .extend_from_slice(events);
            Ok(())
        }
    }

    impl Drop for FakeSink {
        fn drop(&mut self) {
            self.journal.lock().unwrap().clones_alive -= 1;
        }
    }

    pub struct FakeFactory {
        pub journal: SharedJournal,
    }

    impl SinkFactory for FakeFactory {
        type Sink = FakeSink;

        fn create(&mut self, _device: &DeviceDescriptor) -> io::Result<FakeSink> {
            let mut journal = self.journal.lock().unwrap();
            journal.clones_alive += 1;
            journal.clones_created += 1;
            Ok(FakeSink {
                journal: self.journal.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use crate::caps::fixtures::multitouch_touchpad;
    use crate::codes::*;
    use std::sync::{Arc, Mutex};

    const TICK: Duration = Duration::from_millis(1);

    fn setup() -> (Passthrough<FakeSource, FakeFactory>, SharedJournal) {
        let journal = SharedJournal::default();
        let source = FakeSource::new(multitouch_touchpad(), journal.clone());
        let factory = FakeFactory {
            journal: journal.clone(),
        };
        (Passthrough::new(source, factory), journal)
    }

    fn faster() -> Factors {
        Factors::new(2.0, 1.0).unwrap()
    }

    fn touch_down() -> Vec<InputEvent> {
        vec![
            InputEvent::new(EV_ABS, ABS_MT_TRACKING_ID, 1),
            InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, 500),
            InputEvent::new(EV_KEY, BTN_TOUCH, 1),
            InputEvent::new(EV_KEY, BTN_TOOL_FINGER, 1),
            InputEvent::syn(),
        ]
    }

    #[test]
    fn neutral_factors_hold_nothing() {
        let (mut passthrough, journal) = setup();
        for _ in 0..3 {
            passthrough.step(Factors::NEUTRAL, TICK).unwrap();
        }
        let journal = journal.lock().unwrap();
        assert_eq!(journal.grabs, 0);
        assert_eq!(journal.clones_created, 0);
        assert_eq!(passthrough.mode(Factors::NEUTRAL), Mode::Idle);
    }

    #[test]
    fn a_factor_takes_the_hold_only_while_the_pad_is_untouched() {
        let (mut passthrough, journal) = setup();
        passthrough.source_mut().touching = true;
        passthrough.step(faster(), TICK).unwrap();
        assert_eq!(journal.lock().unwrap().grabs, 0);
        assert_eq!(passthrough.mode(faster()), Mode::WaitingToHold);

        passthrough.source_mut().touching = false;
        passthrough.step(faster(), TICK).unwrap();
        let journal = journal.lock().unwrap();
        assert!(journal.grabbed);
        assert_eq!(journal.clones_alive, 1);
        assert_eq!(passthrough.source().drained, 1);
        assert_eq!(passthrough.mode(faster()), Mode::Holding);
    }

    #[test]
    fn returning_to_neutral_releases_after_the_fingers_lift() {
        let (mut passthrough, journal) = setup();
        passthrough.step(faster(), TICK).unwrap();
        passthrough.source_mut().batches.push_back(touch_down());
        passthrough.step(faster(), TICK).unwrap();
        // A finger is down on the clone: neutral waits.
        passthrough.step(Factors::NEUTRAL, TICK).unwrap();
        assert!(journal.lock().unwrap().grabbed);
        passthrough.source_mut().batches.push_back(vec![
            InputEvent::new(EV_ABS, ABS_MT_TRACKING_ID, -1),
            InputEvent::new(EV_KEY, BTN_TOUCH, 0),
            InputEvent::new(EV_KEY, BTN_TOOL_FINGER, 0),
            InputEvent::syn(),
        ]);
        passthrough.step(Factors::NEUTRAL, TICK).unwrap();
        passthrough.step(Factors::NEUTRAL, TICK).unwrap();
        let journal = journal.lock().unwrap();
        assert!(!journal.grabbed);
        assert_eq!(journal.releases, 1);
        assert_eq!(journal.clones_alive, 0);
    }

    #[test]
    fn dropping_the_pass_through_releases_the_grab_and_destroys_the_clone() {
        let (mut passthrough, journal) = setup();
        passthrough.step(faster(), TICK).unwrap();
        passthrough.source_mut().batches.push_back(touch_down());
        passthrough.step(faster(), TICK).unwrap();
        drop(passthrough);
        let journal = journal.lock().unwrap();
        assert!(!journal.grabbed);
        assert_eq!(journal.clones_alive, 0);
        // The finger that was down was lifted on the clone first.
        let tail: Vec<_> = journal
            .emitted
            .iter()
            .rev()
            .take(6)
            .rev()
            .copied()
            .collect();
        assert!(tail.contains(&InputEvent::new(EV_ABS, ABS_MT_TRACKING_ID, -1)));
        assert!(tail.contains(&InputEvent::new(EV_KEY, BTN_TOUCH, 0)));
    }

    #[test]
    fn a_panic_while_holding_releases_the_grab() {
        let journal = SharedJournal::default();
        let inner = journal.clone();
        let result = std::panic::catch_unwind(move || {
            let source = FakeSource::new(multitouch_touchpad(), inner.clone());
            let mut passthrough = Passthrough::new(source, FakeFactory { journal: inner });
            passthrough.step(faster(), TICK).unwrap();
            assert!(passthrough.mode(faster()) == Mode::Holding);
            panic!("a bug in the loop");
        });
        assert!(result.is_err());
        let journal = journal.lock().unwrap();
        assert_eq!(journal.grabs, 1);
        assert!(!journal.grabbed, "the panic left the touchpad grabbed");
        assert_eq!(journal.clones_alive, 0);
    }

    #[test]
    fn lost_events_lift_everything_and_wait_for_an_untouched_pad() {
        let (mut passthrough, journal) = setup();
        passthrough.step(faster(), TICK).unwrap();
        passthrough.source_mut().batches.push_back(touch_down());
        passthrough.step(faster(), TICK).unwrap();
        passthrough.source_mut().touching = true;
        passthrough.source_mut().batches.push_back(vec![
            InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, 510),
            InputEvent::new(EV_SYN, SYN_DROPPED, 0),
            InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, 900),
            InputEvent::syn(),
        ]);
        passthrough.step(faster(), TICK).unwrap();
        assert_eq!(passthrough.mode(faster()), Mode::Resyncing);
        let emitted_after_drop = {
            let journal = journal.lock().unwrap();
            assert!(
                journal
                    .emitted
                    .contains(&InputEvent::new(EV_ABS, ABS_MT_TRACKING_ID, -1))
            );
            journal.emitted.len()
        };
        // Still touching: nothing is forwarded.
        passthrough.source_mut().batches.push_back(vec![
            InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, 950),
            InputEvent::syn(),
        ]);
        passthrough.step(faster(), TICK).unwrap();
        assert_eq!(journal.lock().unwrap().emitted.len(), emitted_after_drop);
        // Lifted: forwarding resumes.
        passthrough.source_mut().touching = false;
        passthrough.source_mut().batches.push_back(vec![
            InputEvent::new(EV_KEY, BTN_TOUCH, 0),
            InputEvent::syn(),
        ]);
        passthrough.step(faster(), TICK).unwrap();
        assert_eq!(passthrough.mode(faster()), Mode::Holding);
    }

    #[test]
    fn the_grab_is_exclusive() {
        let journal = Arc::new(Mutex::new(Journal::default()));
        let mut source = FakeSource::new(multitouch_touchpad(), journal);
        source.set_grab(true).unwrap();
        assert!(source.set_grab(true).is_err());
    }
}
