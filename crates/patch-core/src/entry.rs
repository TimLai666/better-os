//! What an entry is: the [`Entry`] trait, what it reads ([`Read`]), what it
//! changes ([`Change`]), and the [`Host`] that does both.
//!
//! An entry never touches the machine itself. It names its reads, decides
//! from the answers, and returns the changes it wants as data; the host
//! answers and changes. So every decision is tested with a fake host, and
//! the only code that touches the machine is the host in `patch-platform`.
//!
//! An entry that needs root returns [`Change::Privileged`], naming one typed
//! daemon method ([`Privileged`]: reload the USB-C interface, rebind a dead
//! USB controller, change a USB-C recovery choice), which the host sends to
//! `manager-daemon` under its own polkit action, as ADR 0017 says for every
//! root write. The trait does not change: `plan` returns that change like
//! any other, and `plan_undo` returns its reverse.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::flatpak::{AppId, AppName, InstalledApp};
use crate::record::{AppliedRecord, ChangedElsewhere, OverrideKey, Recorded};

pub use hardware_core::usb_recovery::{UsbChoice, UsbFindings, UsbPresent, UsbRecoverySettings};

/// Every entry Better Patch knows, in the page's order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryId {
    /// Blank text from a damaged fontconfig cache.
    FontconfigCache,
    /// A half-size pointer over an Electron Flatpak on native Wayland.
    ElectronCursor,
    /// The USB-C interface (UCSI) stops answering, and a port stops
    /// charging, on a known ASUS model (ticket 101).
    UsbCInterface,
    /// The USB controller of one USB-C side dies on resume, and its hung
    /// hub work keeps every later sleep from working (ticket 101).
    UsbController,
}

/// What a finding is about: the whole machine, or one application.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subject {
    Machine,
    FlatpakApp(AppId),
}

/// Something an entry needs to know.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Read {
    /// A command that changes nothing ([`Command::changes_the_machine`] is
    /// false), and what it printed.
    Command(Command),
    /// `XDG_SESSION_TYPE` of the session the window runs in.
    SessionType,
    /// The scale of every logical monitor, as the compositor reports it.
    DisplayScales,
    /// Every installed Flatpak application, with its override files.
    FlatpakApps,
    /// The person's own override file for one application.
    UserOverride(AppId),
    /// The machine's model, USB-C ports, chargers, and xHCI controllers,
    /// and the USB-C recovery choices kept by the daemon.
    UsbMachine,
    /// This boot's kernel log, classified (never its text).
    UsbFindings,
}

/// What [`Read::UsbMachine`] answers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsbMachineView {
    pub present: UsbPresent,
    pub settings: UsbRecoverySettings,
}

/// Why something could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadError {
    /// The program is not installed.
    NotInstalled(&'static str),
    /// It ran too long and was stopped.
    TimedOut,
    /// Anything else, verbatim, for the error's details.
    Failed(String),
    /// This desktop has nothing to read it from.
    NotOnThisDesktop,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::NotInstalled(program) => write!(f, "{program} is not installed"),
            ReadError::TimedOut => f.write_str("it did not answer in time"),
            ReadError::Failed(reason) => f.write_str(reason),
            ReadError::NotOnThisDesktop => f.write_str("this desktop does not report it"),
        }
    }
}

/// The answer to one [`Read`].
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// What the command printed, when it ran and succeeded.
    Output(Result<String, ReadError>),
    /// `None` when the variable is not set.
    SessionType(Option<String>),
    Scales(Result<Vec<f64>, ReadError>),
    Apps(Result<Vec<InstalledApp>, ReadError>),
    /// A file's text, `None` when it is not there.
    File(Result<Option<String>, ReadError>),
    UsbMachine(Result<UsbMachineView, ReadError>),
    UsbFindings(Result<UsbFindings, ReadError>),
}

/// The answers a host gave, for an entry to decide from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answers(Vec<(Read, Answer)>);

impl Answers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, read: Read, answer: Answer) {
        self.0.retain(|(existing, _)| existing != &read);
        self.0.push((read, answer));
    }

    pub fn get(&self, read: &Read) -> Option<&Answer> {
        self.0
            .iter()
            .find(|(existing, _)| existing == read)
            .map(|(_, answer)| answer)
    }

    pub fn contains(&self, read: &Read) -> bool {
        self.get(read).is_some()
    }
}

/// Why an entry cannot tell whether its fault is here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CannotCheck {
    /// A program the check needs is not installed.
    ProgramMissing(&'static str),
    /// fontconfig did not answer a query.
    FontQueryFailed(String),
    /// The session type is not set, so Wayland cannot be told from X11.
    SessionTypeUnknown,
    /// The compositor did not say how the display is scaled.
    DisplayScaleUnreadable(String),
    /// The installed Flatpak applications could not be read.
    FlatpakAppsUnreadable(String),
    /// A file the check needs could not be read.
    FileUnreadable(String),
    /// The desktop has no way to report what the check needs (the display
    /// scale outside GNOME, for now).
    NotOnThisDesktop,
    /// The host gave no answer to a read the entry asked for; a fault in
    /// the host, never a guess.
    NotAnswered,
    /// This boot's kernel log could not be read: by the person's account
    /// (not in `adm` or `systemd-journal`), nor through the daemon.
    KernelLogUnreadable(String),
}

/// What an entry found on this machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Detection {
    /// It looked. An empty list means the fault is not here.
    Checked(Vec<Finding>),
    CannotCheck(CannotCheck),
    /// The fault is known only on other machines (a model list), so on
    /// this one there is nothing to look for and nothing to offer.
    DoesNotApply,
}

/// One instance of a fault.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub subject: Subject,
    pub detail: FindingDetail,
}

/// What the page needs to say about a finding.
#[derive(Clone, Debug, PartialEq)]
pub enum FindingDetail {
    /// Every query was answered with this file and no style.
    FontCache { file: String },
    /// An Electron app on native Wayland at a scale above 1.
    ElectronCursor {
        name: AppName,
        /// Every logical monitor's scale.
        scales: Vec<f64>,
        /// The app's Electron, from the `version` file Electron ships beside
        /// its binary, when there is one.
        electron: Option<String>,
        /// Whether the verified fix is offered for these scales.
        offer: Offer,
    },
    /// The USB-C interface timed out since the last reset (ticket 101).
    UsbCInterface {
        /// `GET_CONNECTOR_STATUS` and other timeouts this boot.
        timeouts: u32,
        /// `failed to re-enable notifications` after a resume.
        after_sleep: u32,
    },
    /// A USB controller died on resume and is still dead (ticket 101).
    UsbController {
        /// Its PCI address, as the kernel names it.
        address: String,
        /// Sleeps this boot that failed because its hub work would not
        /// freeze, and how many of the last ones in a row.
        sleep_failures: u32,
        streak: u32,
        /// The hub work is stuck: only a restart brings it back.
        restart_needed: bool,
    },
}

/// Whether a finding's fix is offered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Offer {
    /// The fix was verified for this case.
    Fix,
    /// The fault is there, and the fix is not offered, for this reason.
    NotOffered(NotOffered),
}

/// Why a fix is not offered for a fault that is there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotOffered {
    /// A monitor is scaled by a fraction (125%, 150%): through XWayland
    /// GNOME 46 draws the app at 1x and stretches it, so the whole window
    /// turns blurry. Ticket 76 verified the fix at 200% only.
    FractionalScale,
    /// Monitors are scaled differently (200% and 100%): XWayland draws the
    /// app at one scale, so it is blurry or the wrong size on one of them.
    /// Not verified.
    MixedScales,
    /// The app's Electron version could not be found and nothing in its
    /// settings says it asks for Wayland: whether it shows the fault cannot
    /// be told (Electron 38 and later choose Wayland on their own, older
    /// ones only when asked).
    ElectronVersionUnknown,
}

/// Whether a fix can be taken back, and whether it needs to be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UndoKind {
    /// Undo puts back exactly what was there before.
    Reversible,
    /// Nothing the person had is lost, so there is nothing to put back.
    NotNeeded,
}

/// A limit an entry states about itself, which the page shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Limit {
    /// Verified on a Flatpak only; an app installed another way gets nothing.
    FlatpakOnly,
    /// The fix should be undone once the app ships an Electron that scales
    /// its pointer on Wayland, which nothing here can tell.
    UntilTheAppScalesItsPointer,
    /// Recognised only on Flathub's Electron base application or by the
    /// version file Electron ships.
    ElectronBaseAppOnly,
    /// Offered only where every monitor has the same whole-number scale
    /// above 1, the case ticket 76 verified.
    WholeNumberScaleOnly,
    /// Through XWayland, the app's screen sharing may show only X11
    /// windows: Chromium picks its PipeWire capturer only in a Wayland
    /// session.
    ScreenSharing,
    /// Offered only on the models it was seen on (ASUS Vivobook S 14
    /// M5406WA and S 15 M5506WA).
    KnownModelsOnly,
    /// It resets the USB-C interface after each sleep; the firmware fault
    /// itself stays until ASUS or the kernel fixes it.
    ResetsNotRepairs,
    /// When the hub work is stuck, only a restart fixes it.
    RestartWhenStuck,
    /// Not yet measured on the owner's machine before and after.
    NotMeasured,
}

/// One line of "the exact change it makes".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChangeLine {
    /// Runs a program; the page prints it with its arguments.
    Runs(Command),
    /// Sets these keys in the person's override file for an app.
    OverrideKeys {
        app: AppId,
        keys: Vec<(OverrideKey, crate::record::KeyValue)>,
    },
    /// Asks `manager-daemon` for one typed action.
    Privileged(Privileged),
}

/// One typed `manager-daemon` method (`org.betteros.UsbRecovery1`), each
/// behind its own polkit action. None takes a path, a device, or text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Privileged {
    /// `ReloadUcsi`: unload and load `ucsi_acpi`.
    ReloadUcsi,
    /// `RebindController`: unbind and bind the controller the daemon finds
    /// dead.
    RebindController,
    /// `SetChoice`: turn one USB-C recovery choice on or off.
    SetChoice { choice: UsbChoice, on: bool },
}

/// What a finding means and what its fix does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Description {
    pub change: Vec<ChangeLine>,
    pub undo: UndoKind,
    pub limits: Vec<Limit>,
}

/// A change to the machine, as data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    /// Runs a command that changes the machine.
    Run(Command),
    /// Replaces the person's override file for an app, only while it still
    /// holds `expected` (`None`: there is no file). `contents` `None`
    /// removes the file.
    WriteUserOverride {
        app: AppId,
        expected: Option<String>,
        contents: Option<String>,
    },
    /// Asks the daemon for one typed action.
    Privileged(Privileged),
}

/// Why a change was not made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChangeError {
    /// The file changed after it was read; nothing was written.
    ChangedSinceRead,
    /// The program is not installed.
    NotInstalled(&'static str),
    TimedOut,
    /// The host refuses this change (a read-only command, a link where a
    /// file should be).
    Refused(String),
    /// Anything else, verbatim.
    Failed(String),
}

impl fmt::Display for ChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeError::ChangedSinceRead => {
                f.write_str("the file changed while it was being fixed")
            }
            ChangeError::NotInstalled(program) => write!(f, "{program} is not installed"),
            ChangeError::TimedOut => f.write_str("it did not finish in time"),
            ChangeError::Refused(reason) | ChangeError::Failed(reason) => f.write_str(reason),
        }
    }
}

/// What reads and changes the machine for the entries.
pub trait Host {
    fn read(&mut self, read: &Read) -> Answer;
    fn change(&mut self, change: &Change) -> Result<(), ChangeError>;
}

/// Why an entry could not plan a fix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanError {
    /// What the plan needed to read could not be read.
    CannotRead(CannotCheck),
    /// Everything the fix would set is already set.
    NothingToChange,
    /// The fault is there and its fix is not offered here.
    NotOffered(NotOffered),
    /// The finding is not one this entry makes.
    NotThisEntry,
}

/// The changes a fix makes and the record of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub record: Recorded,
}

/// The changes an undo makes, and what it leaves.
#[derive(Clone, Debug, PartialEq)]
pub struct UndoPlan {
    pub changes: Vec<Change>,
    /// What stays recorded: the keys another program changed and the person
    /// did not confirm. `None` when the undo takes back everything.
    pub remaining: Option<Recorded>,
    /// The keys left alone because they changed after the fix.
    pub skipped: Vec<ChangedElsewhere>,
}

/// How a check after a fix came out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckOutcome {
    /// The fault is gone.
    Passed,
    /// The change is in place and the fault is still there.
    StillPresent,
    CannotCheck(CannotCheck),
}

/// A known fault: how to find it, describe it, fix it, check the fix, and
/// undo it. Every method is a decision over answers; none reads or writes.
pub trait Entry: Send + Sync {
    fn id(&self) -> EntryId;

    fn undo_kind(&self) -> UndoKind;

    /// What detection needs to read.
    fn detection_reads(&self) -> Vec<Read>;

    fn detect(&self, answers: &Answers) -> Detection;

    /// What fixing this subject changes, and its limits.
    fn describe(&self, subject: &Subject) -> Description;

    /// What planning needs to read, just before the change.
    fn plan_reads(&self, finding: &Finding) -> Vec<Read>;

    /// The exact change that fixes a finding, and its record.
    fn plan(&self, finding: &Finding, answers: &Answers) -> Result<Plan, PlanError>;

    /// What the check after a fix needs to read.
    fn check_reads(&self, record: &AppliedRecord) -> Vec<Read>;

    fn check(&self, record: &AppliedRecord, answers: &Answers) -> CheckOutcome;

    /// What planning an undo needs to read.
    fn undo_reads(&self, record: &AppliedRecord) -> Vec<Read>;

    /// The changes that take back what `record` says was changed. A key that
    /// changed after the fix is left alone, and listed, unless
    /// `confirm_changed` says the person confirmed replacing it.
    fn plan_undo(
        &self,
        record: &AppliedRecord,
        answers: &Answers,
        confirm_changed: bool,
    ) -> Result<UndoPlan, PlanError>;

    /// Whether the machine keeps this entry's fix (a choice the daemon
    /// holds for every account), rather than the person's record file.
    fn kept_by_machine(&self) -> bool {
        false
    }

    /// The fixes the machine itself says are on, from detection's answers,
    /// for an entry [`Entry::kept_by_machine`].
    fn machine_records(&self, _answers: &Answers) -> Vec<(Subject, Recorded)> {
        Vec::new()
    }
}

/// Every entry, in the page's order.
pub fn all() -> [&'static dyn Entry; 4] {
    [
        &crate::entries::font_cache::FontCache,
        &crate::entries::electron_cursor::ElectronCursor,
        &crate::entries::usb_c::UsbCInterface,
        &crate::entries::usb_c::UsbController,
    ]
}

/// The entry with an ID.
pub fn by_id(id: EntryId) -> &'static dyn Entry {
    all()
        .into_iter()
        .find(|entry| entry.id() == id)
        .expect("every entry ID has an entry")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_has_exactly_one_entry_in_order() {
        let ids: Vec<EntryId> = all().iter().map(|entry| entry.id()).collect();
        assert_eq!(
            ids,
            [
                EntryId::FontconfigCache,
                EntryId::ElectronCursor,
                EntryId::UsbCInterface,
                EntryId::UsbController
            ]
        );
        for id in ids {
            assert_eq!(by_id(id).id(), id);
        }
    }

    #[test]
    fn ids_and_subjects_have_stable_names_on_disk() {
        assert_eq!(
            serde_json::to_string(&EntryId::FontconfigCache).unwrap(),
            "\"fontconfig-cache\""
        );
        assert_eq!(
            serde_json::to_string(&EntryId::ElectronCursor).unwrap(),
            "\"electron-cursor\""
        );
        assert_eq!(
            serde_json::to_string(&EntryId::UsbCInterface).unwrap(),
            "\"usb-c-interface\""
        );
        let subject = Subject::FlatpakApp(AppId::parse("org.example.App").unwrap());
        assert_eq!(
            serde_json::to_string(&subject).unwrap(),
            "{\"flatpak-app\":\"org.example.App\"}"
        );
        assert_eq!(
            serde_json::to_string(&Subject::Machine).unwrap(),
            "\"machine\""
        );
    }

    #[test]
    fn answers_keep_one_answer_per_read() {
        let mut answers = Answers::new();
        answers.insert(Read::SessionType, Answer::SessionType(None));
        answers.insert(
            Read::SessionType,
            Answer::SessionType(Some("wayland".into())),
        );
        assert_eq!(
            answers.get(&Read::SessionType),
            Some(&Answer::SessionType(Some("wayland".into())))
        );
        assert!(!answers.contains(&Read::DisplayScales));
    }

    #[test]
    fn errors_read_as_sentences_for_the_details() {
        assert_eq!(
            ReadError::NotInstalled("/usr/bin/fc-match").to_string(),
            "/usr/bin/fc-match is not installed"
        );
        assert_eq!(
            ChangeError::ChangedSinceRead.to_string(),
            "the file changed while it was being fixed"
        );
    }
}
