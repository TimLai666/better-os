//! What is actually known about a device, and what that is enough to claim.
//!
//! The rule this module exists to enforce: "Ready to unplug" is a positive
//! claim and needs positive evidence. A signal the kernel does not expose, a
//! signal this process may not read, and a signal that says "nothing pending"
//! are three different answers, and only the third one supports the claim.
//!
//! [`ReadinessProof`] has no public constructor and no `Deserialize`. The only
//! way to obtain one is [`ReadinessProof::from_evidence`], and the only way to
//! build a [`ReadyToUnplug`](crate::state::ReadyToUnplug) state is to hand one
//! over. That is the type-system half of the invariant; the tests are the other
//! half.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;

/// Which signal a status refers to, so a refusal can name what was missing.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// Whether a filesystem-scoped flush completed.
    Flush,
    /// Dirty and writeback bytes still owed to the device.
    PendingWriteback,
    /// Processes holding files on the mount open for writing.
    OpenWriters,
}

impl Signal {
    pub fn as_str(self) -> &'static str {
        match self {
            Signal::Flush => "flush",
            Signal::PendingWriteback => "pending_writeback",
            Signal::OpenWriters => "open_writers",
        }
    }
}

/// One platform signal's answer.
///
/// The three ways of not knowing are kept apart for the same reason Better
/// Monitor keeps five observation states apart: a UI that renders "the kernel
/// does not expose this" the same as "nothing is pending" is lying.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalStatus<T> {
    Observed(T),
    /// This kernel, filesystem, or transport has no such interface. Retrying
    /// will not help.
    Unsupported {
        detail: String,
    },
    /// The interface exists but produced nothing usable this time.
    Unavailable {
        detail: String,
    },
    /// The interface exists and would answer for a caller with more privilege.
    /// Recorded separately because it is the one case a privileged helper could
    /// fix, and because it must never read as "nothing pending".
    PermissionDenied {
        detail: String,
    },
}

impl<T> SignalStatus<T> {
    pub fn observed(&self) -> Option<&T> {
        match self {
            SignalStatus::Observed(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_observed(&self) -> bool {
        matches!(self, SignalStatus::Observed(_))
    }
}

/// How far a flush reached.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlushScope {
    /// `syncfs` on the mount. Narrow by construction: it touches one filesystem
    /// and never the whole machine.
    Filesystem,
    /// A device cache flush on top of the filesystem flush, where the platform
    /// exposes one.
    Device,
}

/// A flush that the platform reported as completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FlushVerification {
    pub scope: FlushScope,
    pub completed_at: Timestamp,
}

/// What a flush request did. A failure is a first-class result, not an absence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlushOutcome {
    Completed(FlushVerification),
    Failed {
        detail: String,
    },
    /// The filesystem or transport exposes no flush this code can verify.
    Unsupported {
        detail: String,
    },
}

/// Whether a writeback figure describes this device or the whole machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritebackScope {
    /// Per-backing-device accounting. Authoritative for this device.
    Device,
    /// The machine-wide `Dirty` and `Writeback` totals. They cannot prove a
    /// specific device is clean, so this model only ever uses them as
    /// corroboration and never as the reason for a readiness claim.
    SystemWide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingWriteback {
    pub bytes: u64,
    pub scope: WritebackScope,
}

/// One process holding a file open for writing.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct WriterIdentity {
    pub pid: i32,
    /// The process name where it could be read. A blocker that cannot be named
    /// is still a blocker, which is why this is optional rather than a
    /// placeholder string.
    pub name: Option<String>,
}

/// How much of the process table the scan could see.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanCoverage {
    /// Every process was inspected.
    Complete,
    /// Some processes could not be inspected, almost always because they belong
    /// to another user. The count is kept so the gap is visible instead of
    /// implied.
    Partial { unreadable_processes: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenWriters {
    pub writers: Vec<WriterIdentity>,
    pub coverage: ScanCoverage,
}

/// File operations this system started and has not yet seen flushed. Unlike the
/// platform signals, this one is never unknown: the service owns it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackedOperations(BTreeSet<String>);

impl TrackedOperations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, operation: impl Into<String>) {
        self.0.insert(operation.into());
    }

    pub fn complete(&mut self, operation: &str) -> bool {
        self.0.remove(operation)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

/// Everything known about one device at one moment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SafetyEvidence {
    pub observed_at: Timestamp,
    /// Whether the volume is mounted. An unmounted volume has its own readiness
    /// path: there is no filesystem left to owe the device anything.
    pub mounted: bool,
    pub flush: SignalStatus<FlushVerification>,
    pub writeback: SignalStatus<PendingWriteback>,
    pub open_writers: SignalStatus<OpenWriters>,
    pub tracked_operations: TrackedOperations,
}

impl SafetyEvidence {
    /// Evidence with every platform signal unavailable, which is what a device
    /// looks like before anything has been observed about it.
    pub fn unobserved(observed_at: Timestamp, mounted: bool) -> Self {
        let detail = "not observed yet".to_string();
        Self {
            observed_at,
            mounted,
            flush: SignalStatus::Unavailable {
                detail: detail.clone(),
            },
            writeback: SignalStatus::Unavailable {
                detail: detail.clone(),
            },
            open_writers: SignalStatus::Unavailable { detail },
            tracked_operations: TrackedOperations::new(),
        }
    }
}

/// The thresholds that turn evidence into a claim.
///
/// Issue #5 defers the exact readiness algorithm to an ADR, so the values are
/// here as data rather than buried in the comparison, and every one of them is
/// documented in `docs/storage-safety-signals.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidencePolicy {
    /// How old a verified flush may be and still support a claim. Observation
    /// is event-driven, so a fresh write invalidates a proof immediately; this
    /// bound exists for the case where events were missed rather than absent.
    pub max_proof_age: Duration,
    /// Whether a writer scan that could not see every process still supports a
    /// claim. Default `false`, because on a single-user desktop the processes
    /// that can write to a user-mounted volume are the ones the scan can see,
    /// and refusing on every unreadable root process would make the state
    /// permanently unknown. This is the value most likely to change once the
    /// ADR has measurements.
    pub require_complete_writer_scan: bool,
}

impl Default for EvidencePolicy {
    fn default() -> Self {
        Self {
            max_proof_age: Duration::from_secs(300),
            require_complete_writer_scan: false,
        }
    }
}

/// Why a readiness claim was refused.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessRefusal {
    /// This system's own file operations are still in flight.
    OperationsInFlight {
        operations: Vec<String>,
    },
    /// The device is still owed bytes.
    WritebackPending {
        bytes: u64,
        scope: WritebackScope,
    },
    WritersOpen {
        writers: Vec<WriterIdentity>,
    },
    WriterScanIncomplete {
        unreadable_processes: u32,
    },
    /// No flush has been verified since the last write.
    FlushNotVerified,
    /// The last verified flush is older than the policy allows.
    EvidenceStale {
        age: Duration,
    },
    SignalUnsupported {
        signal: Signal,
        detail: String,
    },
    SignalUnavailable {
        signal: Signal,
        detail: String,
    },
    SignalPermissionDenied {
        signal: Signal,
        detail: String,
    },
}

/// Proof that, at a moment, nothing known was pending.
///
/// Constructed only by [`ReadinessProof::from_evidence`]. It deliberately does
/// not implement `Deserialize`: a proof that could be parsed from a document
/// would let any client hand the service a green light it never earned.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReadinessProof {
    proven_at: Timestamp,
    mounted: bool,
    /// `None` only on the unmounted path, where there is no filesystem to flush.
    flush: Option<FlushVerification>,
    writer_scan: Option<ScanCoverage>,
    writeback: Option<PendingWriteback>,
}

impl ReadinessProof {
    /// Decides whether the evidence supports "no known pending writes".
    ///
    /// Note what this does not claim. It says nothing about a device-internal
    /// volatile cache, about a USB bridge that lies about flush completion, or
    /// about physical persistence. It says that every signal this host could
    /// read reported nothing outstanding, and it records which signals those
    /// were.
    pub fn from_evidence(
        evidence: &SafetyEvidence,
        policy: &EvidencePolicy,
    ) -> Result<Self, ReadinessRefusal> {
        if !evidence.tracked_operations.is_empty() {
            return Err(ReadinessRefusal::OperationsInFlight {
                operations: evidence
                    .tracked_operations
                    .iter()
                    .map(str::to_string)
                    .collect(),
            });
        }

        // An unmounted volume owes the device nothing through a filesystem this
        // host controls, so it does not need a flush to be provably idle. It
        // still needs the tracked-operation check above.
        if !evidence.mounted {
            return Ok(Self {
                proven_at: evidence.observed_at,
                mounted: false,
                flush: None,
                writer_scan: None,
                writeback: None,
            });
        }

        let writeback = match &evidence.writeback {
            SignalStatus::Observed(pending) => {
                if pending.bytes > 0 {
                    match pending.scope {
                        // Device-scoped bytes are this device's bytes.
                        WritebackScope::Device => {
                            return Err(ReadinessRefusal::WritebackPending {
                                bytes: pending.bytes,
                                scope: pending.scope,
                            });
                        }
                        // A machine-wide total is almost never zero on a running
                        // desktop and says nothing about this device. Recording
                        // it and moving on is the honest reading; treating it as
                        // a blocker would make readiness unreachable for a
                        // reason unrelated to the device.
                        WritebackScope::SystemWide => {}
                    }
                }
                Some(*pending)
            }
            // Writeback accounting is the one signal a successful filesystem
            // flush already covers, so its absence is recorded rather than
            // fatal. The flush below is what carries the claim.
            SignalStatus::Unsupported { .. }
            | SignalStatus::Unavailable { .. }
            | SignalStatus::PermissionDenied { .. } => None,
        };

        let writer_scan = match &evidence.open_writers {
            SignalStatus::Observed(open) => {
                if !open.writers.is_empty() {
                    return Err(ReadinessRefusal::WritersOpen {
                        writers: open.writers.clone(),
                    });
                }
                if let ScanCoverage::Partial {
                    unreadable_processes,
                } = open.coverage
                    && policy.require_complete_writer_scan
                {
                    return Err(ReadinessRefusal::WriterScanIncomplete {
                        unreadable_processes,
                    });
                }
                Some(open.coverage.clone())
            }
            SignalStatus::Unsupported { detail } => {
                return Err(ReadinessRefusal::SignalUnsupported {
                    signal: Signal::OpenWriters,
                    detail: detail.clone(),
                });
            }
            SignalStatus::Unavailable { detail } => {
                return Err(ReadinessRefusal::SignalUnavailable {
                    signal: Signal::OpenWriters,
                    detail: detail.clone(),
                });
            }
            SignalStatus::PermissionDenied { detail } => {
                return Err(ReadinessRefusal::SignalPermissionDenied {
                    signal: Signal::OpenWriters,
                    detail: detail.clone(),
                });
            }
        };

        let flush = match &evidence.flush {
            SignalStatus::Observed(verification) => *verification,
            SignalStatus::Unsupported { detail } => {
                return Err(ReadinessRefusal::SignalUnsupported {
                    signal: Signal::Flush,
                    detail: detail.clone(),
                });
            }
            SignalStatus::Unavailable { .. } => return Err(ReadinessRefusal::FlushNotVerified),
            SignalStatus::PermissionDenied { detail } => {
                return Err(ReadinessRefusal::SignalPermissionDenied {
                    signal: Signal::Flush,
                    detail: detail.clone(),
                });
            }
        };

        let age = evidence.observed_at.duration_since(flush.completed_at);
        if age > policy.max_proof_age {
            return Err(ReadinessRefusal::EvidenceStale { age });
        }

        Ok(Self {
            proven_at: evidence.observed_at,
            mounted: true,
            flush: Some(flush),
            writer_scan,
            writeback,
        })
    }

    pub fn proven_at(&self) -> Timestamp {
        self.proven_at
    }

    pub fn mounted(&self) -> bool {
        self.mounted
    }

    pub fn flush(&self) -> Option<FlushVerification> {
        self.flush
    }

    pub fn writer_scan(&self) -> Option<&ScanCoverage> {
        self.writer_scan.as_ref()
    }

    pub fn observed_writeback(&self) -> Option<PendingWriteback> {
        self.writeback
    }

    /// Whether every signal behind the claim was authoritative for this device.
    ///
    /// A proof built while the writer scan could not see the whole process
    /// table, or while only machine-wide writeback figures were available, is
    /// still a proof — but a surface that wants to say more than "no known
    /// pending writes" needs to know the difference.
    pub fn fully_corroborated(&self) -> bool {
        if !self.mounted {
            return true;
        }
        matches!(self.writer_scan, Some(ScanCoverage::Complete))
            && matches!(
                self.writeback,
                Some(PendingWriteback {
                    scope: WritebackScope::Device,
                    ..
                })
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(at: u64) -> SafetyEvidence {
        SafetyEvidence {
            observed_at: Timestamp::from_millis(at),
            mounted: true,
            flush: SignalStatus::Observed(FlushVerification {
                scope: FlushScope::Filesystem,
                completed_at: Timestamp::from_millis(at),
            }),
            writeback: SignalStatus::Observed(PendingWriteback {
                bytes: 0,
                scope: WritebackScope::Device,
            }),
            open_writers: SignalStatus::Observed(OpenWriters {
                writers: Vec::new(),
                coverage: ScanCoverage::Complete,
            }),
            tracked_operations: TrackedOperations::new(),
        }
    }

    #[test]
    fn a_clean_device_produces_a_proof_that_names_its_signals() {
        let proof = ReadinessProof::from_evidence(&clean(10), &EvidencePolicy::default()).unwrap();
        assert_eq!(proof.proven_at(), Timestamp::from_millis(10));
        assert_eq!(proof.flush().unwrap().scope, FlushScope::Filesystem);
        assert!(proof.fully_corroborated());
    }

    #[test]
    fn a_pending_write_of_this_device_refuses_the_claim() {
        let mut evidence = clean(10);
        evidence.writeback = SignalStatus::Observed(PendingWriteback {
            bytes: 4096,
            scope: WritebackScope::Device,
        });
        assert_eq!(
            ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()),
            Err(ReadinessRefusal::WritebackPending {
                bytes: 4096,
                scope: WritebackScope::Device
            })
        );
    }

    #[test]
    fn a_machine_wide_dirty_total_never_grants_and_never_blocks_a_claim() {
        let mut evidence = clean(10);
        evidence.writeback = SignalStatus::Observed(PendingWriteback {
            bytes: 128 * 1024 * 1024,
            scope: WritebackScope::SystemWide,
        });
        let proof = ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default())
            .expect("a machine-wide figure is not this device's pending write");
        // But it is not treated as if it proved anything either.
        assert!(!proof.fully_corroborated());
    }

    #[test]
    fn an_unverified_flush_is_not_the_same_answer_as_an_unsupported_one() {
        let mut unavailable = clean(10);
        unavailable.flush = SignalStatus::Unavailable {
            detail: "no flush requested since the last write".to_string(),
        };
        assert_eq!(
            ReadinessProof::from_evidence(&unavailable, &EvidencePolicy::default()),
            Err(ReadinessRefusal::FlushNotVerified)
        );

        let mut unsupported = clean(10);
        unsupported.flush = SignalStatus::Unsupported {
            detail: "syncfs is not available for this mount".to_string(),
        };
        assert!(matches!(
            ReadinessProof::from_evidence(&unsupported, &EvidencePolicy::default()),
            Err(ReadinessRefusal::SignalUnsupported {
                signal: Signal::Flush,
                ..
            })
        ));
    }

    #[test]
    fn an_unreadable_writer_signal_refuses_rather_than_reading_as_idle() {
        for (status, expected) in [
            (
                SignalStatus::PermissionDenied {
                    detail: "/proc/931/fd".to_string(),
                },
                "permission",
            ),
            (
                SignalStatus::Unavailable {
                    detail: "scan failed".to_string(),
                },
                "unavailable",
            ),
            (
                SignalStatus::Unsupported {
                    detail: "no procfs".to_string(),
                },
                "unsupported",
            ),
        ] {
            let mut evidence = clean(10);
            evidence.open_writers = status;
            let refusal = ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default())
                .expect_err("an unreadable writer signal must not prove idleness");
            let matched = matches!(
                (&refusal, expected),
                (
                    ReadinessRefusal::SignalPermissionDenied { .. },
                    "permission"
                ) | (ReadinessRefusal::SignalUnavailable { .. }, "unavailable")
                    | (ReadinessRefusal::SignalUnsupported { .. }, "unsupported")
            );
            assert!(matched, "{refusal:?} did not match {expected}");
        }
    }

    #[test]
    fn an_open_writer_refuses_the_claim_and_names_the_process() {
        let mut evidence = clean(10);
        evidence.open_writers = SignalStatus::Observed(OpenWriters {
            writers: vec![WriterIdentity {
                pid: 4242,
                name: Some("gimp".to_string()),
            }],
            coverage: ScanCoverage::Complete,
        });
        assert_eq!(
            ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()),
            Err(ReadinessRefusal::WritersOpen {
                writers: vec![WriterIdentity {
                    pid: 4242,
                    name: Some("gimp".to_string())
                }]
            })
        );
    }

    #[test]
    fn a_partial_writer_scan_is_recorded_and_can_be_made_fatal_by_policy() {
        let mut evidence = clean(10);
        evidence.open_writers = SignalStatus::Observed(OpenWriters {
            writers: Vec::new(),
            coverage: ScanCoverage::Partial {
                unreadable_processes: 7,
            },
        });
        let proof = ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()).unwrap();
        assert!(!proof.fully_corroborated());

        let strict = EvidencePolicy {
            require_complete_writer_scan: true,
            ..EvidencePolicy::default()
        };
        assert_eq!(
            ReadinessProof::from_evidence(&evidence, &strict),
            Err(ReadinessRefusal::WriterScanIncomplete {
                unreadable_processes: 7
            })
        );
    }

    #[test]
    fn this_systems_own_unfinished_copy_blocks_the_claim_before_any_signal_is_read() {
        let mut evidence = clean(10);
        evidence.tracked_operations.start("copy-1");
        assert_eq!(
            ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()),
            Err(ReadinessRefusal::OperationsInFlight {
                operations: vec!["copy-1".to_string()]
            })
        );
    }

    #[test]
    fn a_flush_older_than_the_policy_allows_stops_proving_anything() {
        let mut evidence = clean(10);
        evidence.observed_at = Timestamp::from_millis(400_000);
        assert!(matches!(
            ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()),
            Err(ReadinessRefusal::EvidenceStale { .. })
        ));
    }

    #[test]
    fn an_unmounted_volume_is_provably_idle_without_a_flush() {
        let mut evidence = SafetyEvidence::unobserved(Timestamp::from_millis(5), false);
        let proof = ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default())
            .expect("an unmounted volume owes this host nothing");
        assert!(!proof.mounted());
        assert!(proof.flush().is_none());

        evidence.tracked_operations.start("copy-1");
        assert!(ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()).is_err());
    }

    #[test]
    fn unobserved_evidence_proves_nothing_about_a_mounted_volume() {
        let evidence = SafetyEvidence::unobserved(Timestamp::from_millis(5), true);
        assert!(ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()).is_err());
    }
}
