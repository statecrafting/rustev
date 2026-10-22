//! The held-out ledger, `rustev.holdout-ledger/1` (spec 029, 3.7.3 to
//! 3.7.5).
//!
//! A host-kept, hash-chained file: each entry binds one held-out repeat
//! set's lineage, plan, repeat configuration and run reports, and the
//! digest of the entry before it. Rustev reads a ledger it is given and
//! returns a new one with one entry appended; it keeps no store and never
//! modifies its input. The chain proves the ledger's internal consistency,
//! not its completeness: a host can supply an older or truncated ledger,
//! which the heads recorded in every repeat report let an auditor detect.

use std::fmt;

use rustev_contract::canonical::tagged_digest;
use rustev_contract::ids::{ContentDigest, DatasetId, PlanId};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::scope::Handle;
use serde::{Deserialize, Serialize};

pub const HOLDOUT_LEDGER: &str = "rustev.holdout-ledger/1";
/// The tag of an entry's chain digest.
pub const ENTRY_TAG: &str = "rustev.holdout-ledger/1#entry";
/// The most entries one ledger holds.
pub const MAX_ENTRIES: usize = 10_000;
/// The most run reports one entry binds (a repeat set's N).
pub const MAX_ENTRY_RUNS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldoutLedger {
    pub schema: String,
    pub dataset: DatasetId,
    pub entries: Vec<LedgerEntry>,
}

crate::doc::eval_document!(HoldoutLedger, HOLDOUT_LEDGER, REPLAY_V1, ContentDigest);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerEntry {
    /// The host's lineage handle for what is being evaluated.
    pub lineage: Handle,
    pub plan: PlanId,
    pub repeat_config: ContentDigest,
    /// The record digests of the repeat set's run reports.
    pub runs: Vec<ContentDigest>,
    /// Supplied by the host, never read from a clock.
    pub recorded_at_ms: i64,
    /// Null for the first entry; otherwise the chain digest of the entry
    /// before.
    pub prev: Option<ContentDigest>,
}

/// What one held-out repeat report records (3.7.4): the entries with this
/// lineage, with this plan and in total, each including the new one, and
/// the heads before and after (none for an empty ledger).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldOutRecord {
    pub lineage: Handle,
    pub count: u64,
    pub plan_count: u64,
    pub dataset_count: u64,
    pub ledger_head_before: Option<ContentDigest>,
    pub ledger_head_after: ContentDigest,
}

/// Why a ledger was refused (`invalid_input`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    Schema,
    OtherDataset,
    TooManyEntries(usize),
    /// An entry with no run, or more than a repeat set's 64.
    MalformedEntry(usize),
    /// Entry `i`'s `prev` is not the chain digest of entry `i - 1`.
    BrokenChain(usize),
    NotCanonical,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::Schema => write!(f, "not a rustev.holdout-ledger/1 document"),
            LedgerError::OtherDataset => write!(f, "the ledger is for another dataset"),
            LedgerError::TooManyEntries(n) => write!(f, "{n} entries exceed {MAX_ENTRIES}"),
            LedgerError::MalformedEntry(i) => write!(f, "entry {i} binds no run or more than 64"),
            LedgerError::BrokenChain(i) => write!(f, "entry {i} does not extend the chain"),
            LedgerError::NotCanonical => write!(f, "an entry has no canonical form"),
        }
    }
}

impl std::error::Error for LedgerError {}

/// `tagged_digest("rustev.holdout-ledger/1#entry", canonical bytes)`.
pub fn entry_digest(entry: &LedgerEntry) -> Result<ContentDigest, LedgerError> {
    let bytes = rustev_contract::canonical::canonical_bytes(entry)
        .map_err(|_| LedgerError::NotCanonical)?;
    ContentDigest::parse(&tagged_digest(ENTRY_TAG, &bytes)).map_err(|_| LedgerError::NotCanonical)
}

/// What one append binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Append {
    pub lineage: Handle,
    pub plan: PlanId,
    pub repeat_config: ContentDigest,
    pub runs: Vec<ContentDigest>,
    pub recorded_at_ms: i64,
}

impl HoldoutLedger {
    /// The explicit new ledger of 3.7.3.
    pub fn new(dataset: DatasetId) -> HoldoutLedger {
        HoldoutLedger {
            schema: HOLDOUT_LEDGER.into(),
            dataset,
            entries: vec![],
        }
    }

    /// The schema, the dataset, the bound, every entry's shape and the
    /// chain.
    pub fn verify(&self, dataset: &DatasetId) -> Result<(), LedgerError> {
        if self.schema != HOLDOUT_LEDGER {
            return Err(LedgerError::Schema);
        }
        if &self.dataset != dataset {
            return Err(LedgerError::OtherDataset);
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(LedgerError::TooManyEntries(self.entries.len()));
        }
        let mut prev = None;
        for (i, e) in self.entries.iter().enumerate() {
            if e.runs.is_empty() || e.runs.len() > MAX_ENTRY_RUNS {
                return Err(LedgerError::MalformedEntry(i));
            }
            if e.prev != prev {
                return Err(LedgerError::BrokenChain(i));
            }
            prev = Some(entry_digest(e)?);
        }
        Ok(())
    }

    /// The chain digest of the last entry; none for an empty ledger.
    pub fn head(&self) -> Result<Option<ContentDigest>, LedgerError> {
        self.entries.last().map(entry_digest).transpose()
    }

    /// Verify this ledger and return a new one with one entry appended,
    /// with the counts and heads a repeat report records. `self` is never
    /// modified.
    pub fn append(
        &self,
        dataset: &DatasetId,
        a: Append,
    ) -> Result<(HoldoutLedger, HeldOutRecord), LedgerError> {
        self.verify(dataset)?;
        if self.entries.len() >= MAX_ENTRIES {
            return Err(LedgerError::TooManyEntries(self.entries.len() + 1));
        }
        if a.runs.is_empty() || a.runs.len() > MAX_ENTRY_RUNS {
            return Err(LedgerError::MalformedEntry(self.entries.len()));
        }
        let before = self.head()?;
        let entry = LedgerEntry {
            lineage: a.lineage,
            plan: a.plan,
            repeat_config: a.repeat_config,
            runs: a.runs,
            recorded_at_ms: a.recorded_at_ms,
            prev: before.clone(),
        };
        let after = entry_digest(&entry)?;
        let mut next = self.clone();
        next.entries.push(entry);
        let new = next.entries.last().expect("just appended");
        let count =
            |f: &dyn Fn(&LedgerEntry) -> bool| next.entries.iter().filter(|e| f(e)).count() as u64;
        let record = HeldOutRecord {
            lineage: new.lineage.clone(),
            count: count(&|e| e.lineage == new.lineage),
            plan_count: count(&|e| e.plan == new.plan),
            dataset_count: next.entries.len() as u64,
            ledger_head_before: before,
            ledger_head_after: after,
        };
        Ok((next, record))
    }
}
