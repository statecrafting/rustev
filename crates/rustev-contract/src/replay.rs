//! Replay bundles, `rustev.replay/1` (spec 004, 3.2), and the in-memory
//! capture a runtime hands back (spec 004, 5.3).
//!
//! A bundle describes one admitted decision: its scope, lifetimes, the
//! retained plan with every descriptor and calibration needed to recompile
//! it, the snapshot, the delivered run record, the capture status, and one
//! entry per value supplied to the core, in actual supply order. It is
//! evidence of what the host retained, never an attestation of a producer
//! or an authorization token.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{PlanId, RequestId, SnapshotId};
use crate::limits::REPLAY_V1;
use crate::output::BackendOutputDoc;
use crate::reason::RuntimeReasonDoc;
use crate::retention::{MAX_BUNDLE_LIFETIME_MS, RetentionItem};
use crate::schema;
use crate::scope::Scope;
use crate::time::{DurationMs, Timestamp};

/// The most descriptors, calibrations or supplies one bundle carries.
pub const MAX_BUNDLE_ITEMS: usize = 4096;
/// The most resolved bytes of one case, external items included.
pub const MAX_RESOLVED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayBundle {
    pub schema: String,
    pub decision_id: String,
    pub scope: Scope,
    /// Host wall-clock times; copying a bundle resets neither.
    pub created_at_ms: Timestamp,
    pub expires_at_ms: Timestamp,
    /// The domain time the decision was evaluated at.
    pub evaluation_time_ms: Timestamp,
    pub plan_id: PlanId,
    pub plan: RetentionItem,
    /// Every descriptor the plan's bindings name.
    pub descriptors: Vec<RetentionItem>,
    /// Every calibration artifact the plan binds.
    pub calibrations: Vec<RetentionItem>,
    pub snapshot_id: SnapshotId,
    pub snapshot: RetentionItem,
    /// The delivered `rustev.run/1` record; the expected judgment is in it.
    pub run: RetentionItem,
    pub capture: CaptureStatus,
    /// In actual supply order.
    pub supplies: Vec<SupplyEntry>,
}

crate::document::document!(ReplayBundle, schema::REPLAY, REPLAY_V1);

/// Whether every supplied value was captured. Only `complete` reproduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Complete,
    Disabled,
    /// The byte or request cap was hit; captured payloads were discarded.
    LimitExceeded,
    Cancelled,
    Abandoned,
}

/// One value successfully supplied to the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyEntry {
    pub step: String,
    pub instance: Vec<String>,
    /// The identity of the request for the producing target (spec 004, 3.2);
    /// the document is recomputed from the retained plan and snapshot.
    pub request: RequestId,
    /// Actual `supply` order, contiguous from 0.
    pub sequence: u64,
    /// The attempt that produced the value (spec 004, 3.2); none when
    /// nothing was dispatched or a retry or fallback followed the last one.
    pub attempt_id: Option<String>,
    /// The target the value came from (0 is the primary, then declared
    /// fallbacks): the producing target of an output; for a failure, the `to`
    /// of the last fallback transition, else 0.
    pub target: u32,
    /// A `rustev.backend-output/1` or `rustev.runtime-reason/1` document.
    pub value: RetentionItem,
}

/// Where in a bundle a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemLocation {
    Bundle,
    Plan,
    Descriptor(usize),
    Calibration(usize),
    Snapshot,
    Run,
    Supply(usize),
}

impl fmt::Display for ItemLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemLocation::Bundle => f.write_str("bundle"),
            ItemLocation::Plan => f.write_str("plan"),
            ItemLocation::Descriptor(i) => write!(f, "descriptors[{i}]"),
            ItemLocation::Calibration(i) => write!(f, "calibrations[{i}]"),
            ItemLocation::Snapshot => f.write_str("snapshot"),
            ItemLocation::Run => f.write_str("run"),
            ItemLocation::Supply(i) => write!(f, "supplies[{i}]"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifetimeProblem {
    /// An expiry at or before creation.
    NotAfterCreation,
    /// A lifetime beyond the hard or host cap.
    ExceedsCap { cap_ms: u64 },
    /// An item that outlives its bundle.
    OutlivesBundle,
}

/// Why a bundle is structurally invalid, before anything is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    Schema(String),
    /// A decision id is 1 to 256 bytes.
    DecisionId,
    Lifetime {
        location: ItemLocation,
        problem: LifetimeProblem,
    },
    TooMany {
        what: &'static str,
        count: usize,
    },
    /// Supply sequence numbers are not contiguous from 0 in entry order.
    Sequence {
        index: usize,
    },
    /// Two entries share a step and instance.
    DuplicateSupply {
        index: usize,
    },
    /// An item declares a document schema its role does not allow.
    Role {
        location: ItemLocation,
    },
}

impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BundleError::Schema(s) => write!(f, "bundle schema {s:?}"),
            BundleError::DecisionId => f.write_str("a decision id is 1 to 256 bytes"),
            BundleError::Lifetime { location, problem } => {
                write!(f, "lifetime of {location}: {problem:?}")
            }
            BundleError::TooMany { what, count } => {
                write!(f, "{count} {what}, more than {MAX_BUNDLE_ITEMS}")
            }
            BundleError::Sequence { index } => {
                write!(f, "supplies[{index}] breaks the contiguous supply sequence")
            }
            BundleError::DuplicateSupply { index } => {
                write!(f, "supplies[{index}] repeats a step and instance")
            }
            BundleError::Role { location } => {
                write!(f, "{location} declares a schema its role does not allow")
            }
        }
    }
}

impl std::error::Error for BundleError {}

impl ReplayBundle {
    /// Every retention item with its location, in document order.
    pub fn items(&self) -> Vec<(ItemLocation, &RetentionItem)> {
        let mut v = vec![(ItemLocation::Plan, &self.plan)];
        v.extend(
            self.descriptors
                .iter()
                .enumerate()
                .map(|(i, d)| (ItemLocation::Descriptor(i), d)),
        );
        v.extend(
            self.calibrations
                .iter()
                .enumerate()
                .map(|(i, c)| (ItemLocation::Calibration(i), c)),
        );
        v.push((ItemLocation::Snapshot, &self.snapshot));
        v.push((ItemLocation::Run, &self.run));
        v.extend(
            self.supplies
                .iter()
                .enumerate()
                .map(|(i, s)| (ItemLocation::Supply(i), &s.value)),
        );
        v
    }

    /// Structural checks that need no resolved bytes: schema, decision id,
    /// lifetimes under the hard cap and an optional tighter host cap, item
    /// counts, item roles, and the supply sequence. Applies to a typed
    /// bundle as well as a parsed one.
    pub fn check(&self, host_cap_ms: Option<u64>) -> Result<(), BundleError> {
        if self.schema != schema::REPLAY {
            return Err(BundleError::Schema(self.schema.chars().take(80).collect()));
        }
        if self.decision_id.is_empty() || self.decision_id.len() > 256 {
            return Err(BundleError::DecisionId);
        }
        let cap_ms = host_cap_ms.map_or(MAX_BUNDLE_LIFETIME_MS, |c| c.min(MAX_BUNDLE_LIFETIME_MS));
        let life = |location, problem| Err(BundleError::Lifetime { location, problem });
        if self.expires_at_ms <= self.created_at_ms {
            return life(ItemLocation::Bundle, LifetimeProblem::NotAfterCreation);
        }
        // When creation plus the cap is past the last timestamp, every
        // representable expiry is within the cap.
        let beyond = match self.created_at_ms.checked_add(DurationMs(cap_ms)) {
            Ok(latest) => self.expires_at_ms > latest,
            Err(_) => false,
        };
        if beyond {
            return life(ItemLocation::Bundle, LifetimeProblem::ExceedsCap { cap_ms });
        }
        for (what, count) in [
            ("descriptors", self.descriptors.len()),
            ("calibrations", self.calibrations.len()),
            ("supplies", self.supplies.len()),
        ] {
            if count > MAX_BUNDLE_ITEMS {
                return Err(BundleError::TooMany { what, count });
            }
        }
        for (location, item) in self.items() {
            let exp = item.retention.expires_at();
            if exp <= self.created_at_ms {
                return life(location, LifetimeProblem::NotAfterCreation);
            }
            if exp > self.expires_at_ms {
                return life(location, LifetimeProblem::OutlivesBundle);
            }
            let allowed: &[&str] = match location {
                ItemLocation::Plan => &[schema::PLAN],
                ItemLocation::Descriptor(_) => &[schema::BACKEND],
                ItemLocation::Calibration(_) => &[schema::CALIBRATION],
                ItemLocation::Snapshot => &[schema::SNAPSHOT],
                ItemLocation::Run => &[schema::RUN],
                ItemLocation::Supply(_) => &[schema::BACKEND_OUTPUT, schema::RUNTIME_REASON],
                ItemLocation::Bundle => &[],
            };
            if !allowed.contains(&item.document_schema.as_str()) {
                return Err(BundleError::Role { location });
            }
        }
        let mut keys = BTreeSet::new();
        for (i, s) in self.supplies.iter().enumerate() {
            if s.sequence != i as u64 {
                return Err(BundleError::Sequence { index: i });
            }
            if !keys.insert((&s.step, &s.instance)) {
                return Err(BundleError::DuplicateSupply { index: i });
            }
        }
        Ok(())
    }
}

/// What a runtime captured for one admitted decision (spec 004, 5.3): a
/// bounded in-memory handoff, not persistent retention and not a callback.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub decision_id: String,
    pub scope: Scope,
    /// `complete`, `limit_exceeded` or `cancelled`. Entries are empty under
    /// `limit_exceeded`: a partial capture is never reported as complete.
    pub status: CaptureStatus,
    /// In actual supply order.
    pub supplies: Vec<CapturedSupply>,
    /// The record-canonical lengths of every captured value document plus
    /// each `RequestId` string (spec 004, 5.7), counted before any discard.
    pub bytes: u64,
}

/// One supplied value, as [`SupplyEntry`] describes it before retention.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedSupply {
    pub sequence: u64,
    pub step: String,
    pub instance: Vec<String>,
    pub request: RequestId,
    pub attempt_id: Option<String>,
    pub target: u32,
    pub value: CapturedValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapturedValue {
    /// Names the actual producing artifact, fallback included.
    Output(BackendOutputDoc),
    Reason(RuntimeReasonDoc),
}
