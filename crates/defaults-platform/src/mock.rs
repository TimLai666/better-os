//! An adapter for every declared kind that mutates nothing outside itself.
//!
//! ADR 0005 settled how a mock is allowed to behave: a backend that returned a
//! fake success would claim the host changed. This adapter never touches the
//! host at all — it keeps its own map of values and is honest that this is what
//! it is doing. That is enough to prove aggregate status, partial failure, and
//! external-change detection before a production adapter exists for a kind, and
//! it is what `--execution mock` runs against.
//!
//! It can also be told to refuse or to fail, so the two outcomes a real adapter
//! can produce and a happy path cannot are both testable.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::rc::Rc;

use better_core::defaults::{AdapterId, ObservedValue};

use crate::{AdapterRequest, AdapterSet, DefaultsAdapter, WriteOutcome, WriteValue};

/// The values every in-memory adapter in one set shares, keyed by adapter,
/// component, and integration.
type Desktop = Rc<RefCell<BTreeMap<String, ObservedValue>>>;

/// Every adapter id, so a set built from this list cannot silently miss one.
pub const ALL_ADAPTER_IDS: [AdapterId; 9] = [
    AdapterId::XdgDefaultApp,
    AdapterId::XdgEffectiveDefault,
    AdapterId::GnomeKeybinding,
    AdapterId::GnomeDesktopSetting,
    AdapterId::DesktopLauncherEntry,
    AdapterId::InputMethod,
    AdapterId::SessionAutostart,
    AdapterId::UserService,
    AdapterId::ToolEntryPoint,
];

/// What the adapter does when asked to write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MockBehavior {
    /// Record the value in memory.
    Accept,
    /// Refuse, the way an adapter with no way to change this setting refuses.
    Refuse {
        reason: String,
        detail: Option<String>,
    },
    /// Report a failure, the way an adapter that tried and could not reports.
    Fail {
        reason: String,
        detail: Option<String>,
    },
    /// Record the value but report the setting still reads as it did. This is
    /// the "the write went through and did not take" case that only a verifying
    /// read can catch.
    AcceptWithoutEffect,
}

/// A whole simulated desktop: the values every in-memory adapter reads and
/// writes, and the file `--execution mock` keeps them in between runs.
///
/// This is what makes the simulation coherent rather than amnesiac. It is still
/// only a simulation, and it is stored somewhere the caller names, never at a
/// path a real desktop reads.
#[derive(Clone, Debug, Default)]
pub struct MockDesktop {
    values: Desktop,
}

impl MockDesktop {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads a simulated desktop back. A file that is not there is an empty
    /// desktop; a file that will not parse is an error rather than a silent
    /// reset, because the caller is about to plan against it.
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let values: BTreeMap<String, ObservedValue> = serde_json::from_slice(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                Ok(Self {
                    values: Rc::new(RefCell::new(values)),
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_vec_pretty(&*self.values.borrow())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        std::fs::write(path, body)
    }

    /// An adapter for every declared id, all reading and writing this desktop.
    pub fn adapter_set(&self) -> AdapterSet {
        let mut set = AdapterSet::new();
        for id in ALL_ADAPTER_IDS {
            set.insert(Box::new(InMemoryAdapter::sharing(id, self.values.clone())));
        }
        set
    }
}

#[derive(Debug)]
pub struct InMemoryAdapter {
    id: AdapterId,
    values: Desktop,
    behavior: BTreeMap<String, MockBehavior>,
    default_behavior: MockBehavior,
    writes: Vec<String>,
}

impl InMemoryAdapter {
    pub fn new(id: AdapterId) -> Self {
        Self::sharing(id, Desktop::default())
    }

    /// An adapter reading and writing a desktop shared with other adapters.
    pub fn sharing(id: AdapterId, values: Desktop) -> Self {
        Self {
            id,
            values,
            behavior: BTreeMap::new(),
            default_behavior: MockBehavior::Accept,
            writes: Vec::new(),
        }
    }

    fn slot(request: &AdapterRequest<'_>) -> String {
        format!("{}/{}", request.component, request.integration.id)
    }

    /// The key one adapter's slot has in the shared desktop. Two adapters
    /// addressing the same integration are addressing different settings.
    fn key(&self, slot: &str) -> String {
        format!("{:?}/{slot}", self.id)
    }

    /// Seeds what the system says before anything is applied.
    pub fn preset(&mut self, slot: impl Into<String>, value: ObservedValue) -> &mut Self {
        let key = self.key(&slot.into());
        self.values.borrow_mut().insert(key, value);
        self
    }

    /// Changes a value the way something outside Better Manager would.
    pub fn change_externally(&mut self, slot: impl Into<String>, value: ObservedValue) {
        self.preset(slot, value);
    }

    pub fn set_behavior(&mut self, slot: impl Into<String>, behavior: MockBehavior) -> &mut Self {
        self.behavior.insert(slot.into(), behavior);
        self
    }

    pub fn set_default_behavior(&mut self, behavior: MockBehavior) -> &mut Self {
        self.default_behavior = behavior;
        self
    }

    /// Every slot this adapter was asked to write, in order. A test that must
    /// prove nothing was changed asserts this is empty.
    pub fn writes(&self) -> &[String] {
        &self.writes
    }
}

impl DefaultsAdapter for InMemoryAdapter {
    fn id(&self) -> AdapterId {
        self.id
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        self.values
            .borrow()
            .get(&self.key(&Self::slot(request)))
            .cloned()
            .unwrap_or(ObservedValue::Unset)
    }

    fn write(&mut self, request: &AdapterRequest<'_>, value: &WriteValue) -> WriteOutcome {
        let slot = Self::slot(request);
        let key = self.key(&slot);
        let behavior = self
            .behavior
            .get(&slot)
            .unwrap_or(&self.default_behavior)
            .clone();
        match behavior {
            MockBehavior::Refuse { reason, detail } => {
                WriteOutcome::ManualActionRequired { reason, detail }
            }
            MockBehavior::Fail { reason, detail } => WriteOutcome::Failed { reason, detail },
            MockBehavior::AcceptWithoutEffect => {
                self.writes.push(slot);
                WriteOutcome::Written
            }
            MockBehavior::Accept => {
                let next = match value {
                    WriteValue::Set { value } => ObservedValue::Set {
                        value: value.clone(),
                    },
                    WriteValue::Clear => ObservedValue::Unset,
                };
                if self.values.borrow().get(&key) == Some(&next) {
                    return WriteOutcome::AlreadyCorrect;
                }
                self.writes.push(slot);
                self.values.borrow_mut().insert(key, next);
                WriteOutcome::Written
            }
        }
    }
}
