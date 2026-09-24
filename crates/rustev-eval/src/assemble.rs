//! Assembling a replay bundle from what the host retained (spec 004, 5.5).
//!
//! Assembly takes the caller-retained plan, its descriptors and calibrations,
//! the snapshot, the delivered run record and the runtime's capture,
//! validates every binding between them, and applies the host's explicit
//! retention choice. Content defaults to digest-only and metadata to seven
//! days; keeping bytes, embedded or external, is an explicit choice, and no
//! item outlives the bundle. A capture is not a promise that a bundle fits:
//! the escaped size is checked and refused with a typed error.

use std::fmt;

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::canonical::tagged_digest;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::ids::ContentDigest;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::plan::Plan;
use rustev_contract::replay::{
    BundleError, Capture, CaptureStatus, CapturedValue, ItemLocation, MAX_BUNDLE_ITEMS,
    MAX_RESOLVED_BYTES, ReplayBundle, SupplyEntry,
};
use rustev_contract::retention::{
    DEFAULT_METADATA_LIFETIME_MS, MAX_BUNDLE_LIFETIME_MS, Retention, RetentionItem,
};
use rustev_contract::run::{CoreEvidence, RunRecord, Termination};
use rustev_contract::scope::Scope;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::{DurationMs, Timestamp};
use rustev_contract::{Document, Identified, schema};
use rustev_core::{Compiled, LoadError};

/// How the host keeps document bytes.
pub enum Content<'a> {
    /// Only digests: nothing is replayable. The default.
    DigestOnly,
    /// Embedded in the bundle.
    Embedded,
    /// Stored by the host, which returns an opaque reference for the bytes.
    External(&'a dyn Fn(ItemLocation, &[u8]) -> String),
}

/// The host's explicit retention choice for one bundle.
pub struct RetentionChoice<'a> {
    /// From creation; every item expires with the bundle.
    pub lifetime_ms: u64,
    pub content: Content<'a>,
    /// A cap tighter than the hard 30 days.
    pub host_cap_ms: Option<u64>,
}

impl Default for RetentionChoice<'_> {
    fn default() -> Self {
        RetentionChoice {
            lifetime_ms: DEFAULT_METADATA_LIFETIME_MS,
            content: Content::DigestOnly,
            host_cap_ms: None,
        }
    }
}

/// What the host retained for one decision.
pub struct AssemblyInput<'a> {
    /// `None` when capture was off: the bundle states `disabled`.
    pub capture: Option<&'a Capture>,
    /// The host's isolation scope; a capture's must be the same.
    pub scope: &'a Scope,
    pub plan: &'a Plan,
    pub descriptors: &'a [BackendDescriptor],
    pub calibrations: &'a [CalibrationArtifact],
    pub snapshot: &'a Snapshot,
    pub run: &'a RunRecord,
    /// The domain time the decision was evaluated at.
    pub evaluation_time: Timestamp,
    /// Host wall-clock creation time.
    pub created_at_ms: Timestamp,
}

/// Why no bundle was assembled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    /// The lifetime is zero, beyond the cap, or past the last timestamp.
    Lifetime {
        lifetime_ms: u64,
    },
    /// Capture, run record, plan and snapshot do not describe one decision.
    Binding {
        what: &'static str,
    },
    /// The plan does not load with the given dependencies.
    Plan(String),
    /// A document has no record canonical form.
    NotCanonical {
        location: ItemLocation,
    },
    /// The escaped bundle or its embedded bytes exceed the replay bounds.
    TooLarge {
        bytes: usize,
        limit: usize,
    },
    Bundle(BundleError),
}

impl fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AssemblyError {}

fn bytes_of<T: Document + PartialEq>(
    doc: &T,
    location: ItemLocation,
) -> Result<Vec<u8>, AssemblyError> {
    doc.record_canonical()
        .map_err(|_| AssemblyError::NotCanonical { location })
}

/// Assemble and check a bundle.
pub fn assemble(
    input: &AssemblyInput<'_>,
    choice: &RetentionChoice<'_>,
) -> Result<ReplayBundle, AssemblyError> {
    let cap = choice
        .host_cap_ms
        .map_or(MAX_BUNDLE_LIFETIME_MS, |c| c.min(MAX_BUNDLE_LIFETIME_MS));
    let lifetime = choice.lifetime_ms;
    if lifetime == 0 || lifetime > cap {
        return Err(AssemblyError::Lifetime {
            lifetime_ms: lifetime,
        });
    }
    let expires = input
        .created_at_ms
        .checked_add(DurationMs(lifetime))
        .map_err(|_| AssemblyError::Lifetime {
            lifetime_ms: lifetime,
        })?;
    let binding = |what| Err(AssemblyError::Binding { what });
    let run = input.run;
    let plan_id = input.plan.id().map_err(|_| AssemblyError::NotCanonical {
        location: ItemLocation::Plan,
    })?;
    let snapshot_id = input
        .snapshot
        .id()
        .map_err(|_| AssemblyError::NotCanonical {
            location: ItemLocation::Snapshot,
        })?;
    if run.plan_id != plan_id {
        return binding("run record plan id");
    }
    if let CoreEvidence::Recorded(e) = &run.core {
        if e.plan_id != plan_id || e.judgment.plan_id != plan_id {
            return binding("evidence plan id");
        }
        if e.snapshot_id != snapshot_id {
            return binding("snapshot id");
        }
        if e.evaluation_time_ms != input.evaluation_time {
            return binding("evaluation time");
        }
        if e.decision_id != run.decision_id {
            return binding("evidence decision id");
        }
    }
    if let Some(c) = input.capture {
        if c.decision_id != run.decision_id {
            return binding("capture decision id");
        }
        if &c.scope != input.scope {
            return binding("capture scope");
        }
        let max = input
            .plan
            .definition
            .limits
            .max_semantic_requests
            .min(MAX_BUNDLE_ITEMS as u64);
        if c.supplies.len() as u64 > max {
            return binding("capture entries beyond the plan's request bound");
        }
        if c.status == CaptureStatus::Complete && run.termination != Termination::Judged {
            return binding("a complete capture of a run without a judgment");
        }
    }
    Compiled::load_checked(input.plan, input.descriptors, input.calibrations)
        .map_err(|e: LoadError| AssemblyError::Plan(e.to_string()))?;

    // Every byte replay would resolve, embedded or external, counts.
    let mut resolved = 0usize;
    let mut not_utf8 = None;
    let mut item = |location: ItemLocation,
                    document_schema: &str,
                    bytes: Vec<u8>,
                    identified: bool|
     -> RetentionItem {
        let digest = ContentDigest::parse(&tagged_digest(document_schema, &bytes))
            .expect("a tagged digest is an identity");
        let retention = match &choice.content {
            Content::DigestOnly => Retention::DigestOnly {
                digest: digest.clone(),
                expires_at_ms: expires,
            },
            Content::Embedded => {
                resolved += bytes.len();
                Retention::Retained {
                    bytes: String::from_utf8(bytes).unwrap_or_else(|_| {
                        not_utf8 = Some(location);
                        String::new()
                    }),
                    digest: digest.clone(),
                    expires_at_ms: expires,
                }
            }
            Content::External(store) => {
                resolved += bytes.len();
                Retention::External {
                    reference: store(location, &bytes),
                    digest: digest.clone(),
                    expires_at_ms: expires,
                }
            }
        };
        RetentionItem {
            document_schema: document_schema.into(),
            identity: identified.then_some(digest),
            retention,
        }
    };

    let plan = item(
        ItemLocation::Plan,
        schema::PLAN,
        bytes_of(input.plan, ItemLocation::Plan)?,
        true,
    );
    let mut descriptors = vec![];
    for (i, d) in input.descriptors.iter().enumerate() {
        let at = ItemLocation::Descriptor(i);
        descriptors.push(item(at, schema::BACKEND, bytes_of(d, at)?, true));
    }
    let mut calibrations = vec![];
    for (i, c) in input.calibrations.iter().enumerate() {
        let at = ItemLocation::Calibration(i);
        calibrations.push(item(at, schema::CALIBRATION, bytes_of(c, at)?, true));
    }
    let snapshot = item(
        ItemLocation::Snapshot,
        schema::SNAPSHOT,
        bytes_of(input.snapshot, ItemLocation::Snapshot)?,
        true,
    );
    let run_item = item(
        ItemLocation::Run,
        schema::RUN,
        bytes_of(run, ItemLocation::Run)?,
        false,
    );
    let (capture, mut supplies) = match input.capture {
        None => (CaptureStatus::Disabled, vec![]),
        Some(c) => (c.status, vec![]),
    };
    for (i, s) in input
        .capture
        .map(|c| c.supplies.as_slice())
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let at = ItemLocation::Supply(i);
        let value = match &s.value {
            CapturedValue::Output(o) => item(at, schema::BACKEND_OUTPUT, bytes_of(o, at)?, false),
            CapturedValue::Reason(r) => item(at, schema::RUNTIME_REASON, bytes_of(r, at)?, false),
        };
        supplies.push(SupplyEntry {
            step: s.step.clone(),
            instance: s.instance.clone(),
            request: s.request.clone(),
            sequence: s.sequence,
            attempt_id: s.attempt_id.clone(),
            target: s.target,
            value,
        });
    }
    if let Some(location) = not_utf8 {
        return Err(AssemblyError::NotCanonical { location });
    }
    if resolved > MAX_RESOLVED_BYTES {
        return Err(AssemblyError::TooLarge {
            bytes: resolved,
            limit: MAX_RESOLVED_BYTES,
        });
    }
    let scope = input.scope.clone();
    let bundle = ReplayBundle {
        schema: schema::REPLAY.into(),
        decision_id: run.decision_id.clone(),
        scope,
        created_at_ms: input.created_at_ms,
        expires_at_ms: expires,
        evaluation_time_ms: input.evaluation_time,
        plan_id,
        plan,
        descriptors,
        calibrations,
        snapshot_id,
        snapshot,
        run: run_item,
        capture,
        supplies,
    };
    let size = bundle
        .record_canonical()
        .map_err(|_| AssemblyError::NotCanonical {
            location: ItemLocation::Bundle,
        })?
        .len();
    if size > REPLAY_V1.max_bytes {
        return Err(AssemblyError::TooLarge {
            bytes: size,
            limit: REPLAY_V1.max_bytes,
        });
    }
    bundle
        .check(choice.host_cap_ms)
        .map_err(AssemblyError::Bundle)?;
    Ok(bundle)
}
