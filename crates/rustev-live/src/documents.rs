//! Canonical live-run inputs and durable checkpoints (spec 043, increment 2).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rustev_contract::definition::Operation;
use rustev_contract::ids::{ArtifactId, ContentDigest, DatasetId, PlanId, SnapshotId};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::{Document, Identified, Timestamp};
use serde::{Deserialize, Serialize};

pub use rustev_contract::ids::LiveConfigId;

/// A CLI-compatible refusal. Details contain no credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveError {
    pub status: &'static str,
    pub subject: &'static str,
    pub detail: String,
}

impl LiveError {
    pub(crate) fn invalid(subject: &'static str, detail: impl fmt::Display) -> Self {
        Self {
            status: "invalid_input",
            subject,
            detail: detail.to_string(),
        }
    }

    fn config(detail: impl fmt::Display) -> Self {
        Self::invalid("live-config", detail)
    }

    pub(crate) fn io(detail: impl fmt::Display) -> Self {
        Self {
            status: "io_error",
            subject: "checkpoint",
            detail: detail.to_string(),
        }
    }
}

impl fmt::Display for LiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.status, self.subject, self.detail)
    }
}
impl std::error::Error for LiveError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Cli,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Budget {
    Testing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EvaluationTime {
    Source,
    Fixed { ms: Timestamp },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostUnit {
    NanoUsd,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionMode {
    Live,
    Captured,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingIdentity {
    pub backend_id: String,
    pub artifact: ArtifactId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub identity: BindingIdentity,
    pub cost_unit: CostUnit,
    pub max_in_flight: u64,
    pub min_interval_ms: u64,
    pub case_deadline_ms: u64,
    pub rate_limit_pause_max_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeProvenance {
    Synthetic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    #[serde(default = "default_probe_calls")]
    pub calls: u64,
    pub interval_ms: u64,
    pub snapshot: String,
    pub digest: SnapshotId,
    pub provenance: ProbeProvenance,
}
fn default_probe_calls() -> u64 {
    3
}

/// Dispatch rules are evidence of the CLI host's choice, never permission
/// for a downstream action (spec 043, 3.12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationRule {
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub operations: Vec<Operation>,
    pub max_exposure_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveConfig {
    pub schema: String,
    pub budget: Budget,
    pub run_cap_units: u64,
    pub repeats: u64,
    pub evaluation_time: EvaluationTime,
    pub bindings: Vec<Binding>,
    pub acquisitions: BTreeMap<String, AcquisitionMode>,
    pub probe: Probe,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_authorize"
    )]
    pub authorize: Option<Vec<AuthorizationRule>>,
    #[serde(default)]
    pub compare_with_source: bool,
}

// A present null is not absence: the CLI requires a rule list and the
// library requires the member to be absent (spec 043, 3.2.2).
fn present_authorize<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Vec<AuthorizationRule>>, D::Error> {
    Vec::deserialize(d).map(Some)
}

macro_rules! live_document {
    ($ty:ty, $schema:literal) => {
        impl Document for $ty {
            const SCHEMA: &'static str = $schema;
            const LIMITS: rustev_contract::bounded::ParseLimits = REPLAY_V1;
            fn schema(&self) -> &str {
                &self.schema
            }
        }
    };
}
live_document!(LiveConfig, "rustev.live-config/1");
impl Identified for LiveConfig {
    type Id = LiveConfigId;
    fn wrap(digest: String) -> Self::Id {
        LiveConfigId::parse(&digest).expect("canonical digest is a validated identity")
    }
}

/// Candidate names and the union of primary and fallback bindings reachable
/// from each compiled plan. Resolving plans is the caller's responsibility.
pub type Candidates = BTreeMap<String, Vec<BindingIdentity>>;

impl LiveConfig {
    /// Bounded parsing with the live input refusal vocabulary.
    /// A probe whose provenance is absent or not `synthetic` is refused
    /// `probe-snapshot` (spec 043, 3.7.2); every other defect `live-config`.
    pub fn parse(bytes: &[u8]) -> Result<Self, LiveError> {
        <Self as Document>::parse(bytes).map_err(|e| {
            if probe_provenance_undeclared(bytes) {
                LiveError::invalid("probe-snapshot", "probe.provenance must be synthetic")
            } else {
                LiveError::config(e)
            }
        })
    }

    pub fn check(&self, mode: Mode, candidates: &Candidates) -> Result<(), LiveError> {
        if self.schema != Self::SCHEMA {
            return Err(LiveError::config("wrong schema"));
        }
        if (mode == Mode::Cli) != self.authorize.is_some() {
            return Err(LiveError::config("authorize is required only in CLI mode"));
        }
        if self.run_cap_units == 0
            || !(1..=64).contains(&self.repeats)
            || !(1..=5).contains(&self.probe.calls)
            || self.probe.interval_ms > 60_000
        {
            return Err(LiveError::config("run or probe value out of range"));
        }
        let mut names = BTreeSet::new();
        for name in candidates.keys() {
            component(name, 32, true)?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(LiveError::config(
                    "candidate names collide ignoring ASCII case",
                ));
            }
        }
        let reached: BTreeSet<_> = candidates.values().flatten().collect();
        let mut configured = BTreeSet::new();
        for binding in &self.bindings {
            if !configured.insert(&binding.identity) {
                return Err(LiveError::config("duplicate binding"));
            }
            if !(1..=64).contains(&binding.max_in_flight)
                || binding.min_interval_ms > 600_000
                || !(1..=3_600_000).contains(&binding.case_deadline_ms)
                || binding.rate_limit_pause_max_ms > 600_000
            {
                return Err(LiveError::config("binding value out of range"));
            }
        }
        if configured != reached {
            return Err(LiveError::config(
                "configured bindings differ from reachable bindings",
            ));
        }
        Ok(())
    }
}

// Consulted only for a refused document within the parse bound, the same
// bound `LiveConfig::parse` applies (`<LiveConfig as Document>::LIMITS`).
fn probe_provenance_undeclared(bytes: &[u8]) -> bool {
    bytes.len() <= <LiveConfig as Document>::LIMITS.max_bytes
        && serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|v| {
                let probe = v.get("probe")?.as_object()?;
                Some(probe.get("provenance").and_then(|p| p.as_str()) != Some("synthetic"))
            })
            .unwrap_or(false)
}

// Spec 006, 3.9.1. The eval candidate checker is crate-private and the
// CLI/surface layer is above live, so neither validator is reachable here.
fn component(value: &str, max: usize, starts_alphanumeric: bool) -> Result<(), LiveError> {
    if value.is_empty()
        || value.len() > max
        || (starts_alphanumeric && !value.as_bytes()[0].is_ascii_alphanumeric())
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(LiveError::config("invalid decision id component"));
    }
    Ok(())
}

/// Build a decision id before opening a journal (spec 043, 3.3.4).
pub fn decision_id(
    run_id: &str,
    candidate: &str,
    repeat: u64,
    case_id: &str,
    rerun: Option<u64>,
) -> Result<String, LiveError> {
    component(run_id, 64, false)?;
    component(candidate, 32, true)?;
    component(case_id, 128, true)?;
    if !(1..=64).contains(&repeat) || rerun.is_some_and(|k| !(1..=99).contains(&k)) {
        return Err(LiveError::config("repeat or rerun out of range"));
    }
    let mut id = format!("live/{run_id}/{candidate}/{repeat}/{case_id}");
    if let Some(k) = rerun {
        id.push_str(&format!("/rerun-{k}"));
    }
    Ok(id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBinding {
    pub name: String,
    pub plan_id: PlanId,
    pub bindings: Vec<BindingIdentity>,
}

/// Every value whose equality is required before a checkpoint can resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointBindings {
    pub run_id: String,
    pub live_config_id: LiveConfigId,
    pub dataset_id: DatasetId,
    pub split: String,
    pub candidates: Vec<CandidateBinding>,
    pub journal_header_digest: ContentDigest,
    pub input_digests: BTreeMap<String, ContentDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemState {
    Pending,
    Completed {
        live_document_digest: ContentDigest,
        run_record_digest: ContentDigest,
    },
    Unavailable {
        reason: String,
    },
    Refused {
        code: String,
    },
    Interrupted {
        attempt_ids: Vec<String>,
    },
    /// Persist before the first reservation. Earlier reruns stay in history.
    Rerun {
        k: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointItem {
    pub candidate: String,
    pub repeat: u64,
    pub case_id: String,
    /// Ordered history preserves every rerun number across interruptions.
    pub states: Vec<ItemState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCheckpoint {
    pub schema: String,
    pub bindings: CheckpointBindings,
    pub items: Vec<CheckpointItem>,
}
live_document!(LiveCheckpoint, "rustev.live-checkpoint/1");

/// The companion evidence for one evaluation item or probe (spec 043, 3.9.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaseDocument {
    pub schema: String,
    pub mode: CaseMode,
    pub purpose: CasePurpose,
    pub source: CaseSource,
    pub run: CaseRun,
    pub evaluation_time: CaseEvaluationTime,
    pub acquisitions: BTreeMap<String, CaseAcquisition>,
    pub spend: Vec<CaseSpend>,
    pub declared_no_spend: Vec<BindingIdentity>,
    pub live_document_digest: ContentDigest,
    pub run_record_digest: ContentDigest,
}
live_document!(LiveCaseDocument, "rustev.live-case/1");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseMode {
    LiveReexecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CasePurpose {
    Evaluation,
    Probe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CaseSource {
    None,
    DatasetCase {
        dataset_id: DatasetId,
        case_id: String,
        split: String,
    },
    ReplayBundle {
        bundle_digest: ContentDigest,
        decision_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRun {
    pub run_id: String,
    pub live_config_id: LiveConfigId,
    pub candidate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerun: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseEvaluationTime {
    Source,
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CaseAcquisition {
    Live,
    Captured { capture_digests: Vec<ContentDigest> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CaseSettlement {
    Observed { units: u64 },
    Estimated { units: u64 },
    Unknown,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRefusal {
    pub code: String,
}

/// Untagged on the wire. Serde does not apply `deny_unknown_fields` to an
/// untagged enum, so parsing goes through [`CaseSpendWire`], whose variant
/// structs each refuse unknown fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, from = "CaseSpendWire")]
pub enum CaseSpend {
    Reserved {
        attempt_id: String,
        seq: u64,
        reserved_units: u64,
        settlement: CaseSettlement,
    },
    Refused {
        attempt_id: String,
        refused: CaseRefusal,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservedWire {
    attempt_id: String,
    seq: u64,
    reserved_units: u64,
    settlement: CaseSettlement,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefusedWire {
    attempt_id: String,
    refused: CaseRefusal,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CaseSpendWire {
    Reserved(ReservedWire),
    Refused(RefusedWire),
}

impl From<CaseSpendWire> for CaseSpend {
    fn from(wire: CaseSpendWire) -> Self {
        match wire {
            CaseSpendWire::Reserved(r) => CaseSpend::Reserved {
                attempt_id: r.attempt_id,
                seq: r.seq,
                reserved_units: r.reserved_units,
                settlement: r.settlement,
            },
            CaseSpendWire::Refused(r) => CaseSpend::Refused {
                attempt_id: r.attempt_id,
                refused: r.refused,
            },
        }
    }
}

impl LiveCaseDocument {
    pub fn check(&self) -> Result<(), LiveError> {
        use rustev_contract::live::{MAX_EFFECTS, MAX_TEXT_BYTES};
        let invalid = || LiveError::invalid("live-case", "invalid identity, purpose or bound");
        let text = |s: &str| !s.is_empty() && s.len() <= MAX_TEXT_BYTES;
        if self.schema != Self::SCHEMA
            || self.spend.len() > MAX_EFFECTS
            || self.acquisitions.len() > MAX_EFFECTS
            || self.declared_no_spend.len() > MAX_EFFECTS
            || self.run.repeat.is_some_and(|n| !(1..=64).contains(&n))
            || self.run.rerun.is_some_and(|n| !(1..=99).contains(&n))
        {
            return Err(invalid());
        }
        component(&self.run.run_id, 64, false).map_err(|_| invalid())?;
        component(&self.run.candidate, 32, true).map_err(|_| invalid())?;
        match (&self.purpose, &self.source) {
            (CasePurpose::Probe, CaseSource::None)
                if self.run.repeat.is_none() && self.run.rerun.is_none() => {}
            (CasePurpose::Evaluation, CaseSource::DatasetCase { case_id, split, .. })
                if self.run.repeat.is_some() && text(split) =>
            {
                component(case_id, 128, true).map_err(|_| invalid())?;
            }
            (CasePurpose::Evaluation, CaseSource::ReplayBundle { decision_id, .. })
                if self.run.repeat.is_some() && text(decision_id) => {}
            _ => return Err(invalid()),
        }
        let mut seqs = BTreeSet::new();
        let mut refusals = BTreeSet::new();
        for entry in &self.spend {
            let attempt = match entry {
                CaseSpend::Reserved {
                    attempt_id, seq, ..
                } => {
                    if !seqs.insert(seq) {
                        return Err(invalid());
                    }
                    attempt_id
                }
                CaseSpend::Refused {
                    attempt_id,
                    refused,
                } => {
                    if !text(&refused.code) {
                        return Err(invalid());
                    }
                    attempt_id
                }
            };
            if attempt.is_empty() || attempt.len() > 4 * MAX_TEXT_BYTES {
                return Err(invalid());
            }
        }
        // A refusal is listed once, and never for an attempt with a
        // reservation. Reservations are keyed by journal sequence: a probe
        // decision repeated on resume can reserve the same attempt id again.
        let reserved: BTreeSet<&str> = self
            .spend
            .iter()
            .filter_map(|s| match s {
                CaseSpend::Reserved { attempt_id, .. } => Some(attempt_id.as_str()),
                CaseSpend::Refused { .. } => None,
            })
            .collect();
        for entry in &self.spend {
            if let CaseSpend::Refused { attempt_id, .. } = entry
                && (reserved.contains(attempt_id.as_str()) || !refusals.insert(attempt_id))
            {
                return Err(invalid());
            }
        }
        for (source, acquisition) in &self.acquisitions {
            if !text(source)
                || matches!(acquisition, CaseAcquisition::Captured { capture_digests } if capture_digests.len() > MAX_EFFECTS)
            {
                return Err(invalid());
            }
        }
        let mut bindings = BTreeSet::new();
        for binding in &self.declared_no_spend {
            if !text(&binding.backend_id) || !bindings.insert(binding) {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub(crate) fn from_result(
        config: &LiveConfig,
        run: CaseRun,
        source: CaseSource,
        journal: &crate::journal::SpendJournal,
        result: &rustev_runtime::LiveResult,
    ) -> Result<Self, LiveError> {
        use rustev_contract::live::{AuthorizationOutcome, Dispatch};
        let mut spend = journal.reservations_detail(&result.document.decision_id);
        for effect in &result.document.effects {
            if spend.iter().any(|s| matches!(s, CaseSpend::Reserved { attempt_id, .. } if attempt_id == &effect.attempt_id)) {
                continue;
            }
            let code = match &effect.authorization {
                AuthorizationOutcome::Refused { .. } => Some("authorization_refused"),
                AuthorizationOutcome::Allowed { .. }
                    if effect.dispatch == Dispatch::NotDispatched =>
                {
                    result.spend_refusal.as_ref().map(|r| r.code.as_str())
                }
                _ => None,
            };
            if let Some(code) = code {
                spend.push(CaseSpend::Refused {
                    attempt_id: effect.attempt_id.clone(),
                    refused: CaseRefusal { code: code.into() },
                });
            }
        }
        let doc = Self {
            schema: Self::SCHEMA.into(),
            mode: CaseMode::LiveReexecution,
            purpose: if source == CaseSource::None {
                CasePurpose::Probe
            } else {
                CasePurpose::Evaluation
            },
            source,
            run,
            evaluation_time: match config.evaluation_time {
                EvaluationTime::Source => CaseEvaluationTime::Source,
                EvaluationTime::Fixed { .. } => CaseEvaluationTime::Fixed,
            },
            // The gated runtime refuses plans with acquisitions before returning a result.
            acquisitions: BTreeMap::new(),
            spend,
            declared_no_spend: config
                .bindings
                .iter()
                .filter(|b| b.cost_unit == CostUnit::None)
                .map(|b| b.identity.clone())
                .collect(),
            live_document_digest: result.document.record_digest().map_err(LiveError::io)?,
            run_record_digest: result.record.record_digest().map_err(LiveError::io)?,
        };
        doc.check()?;
        Ok(doc)
    }

    pub(crate) fn write_result(
        &self,
        stem: &Path,
        result: &rustev_runtime::LiveResult,
    ) -> Result<ContentDigest, LiveError> {
        self.check()?;
        let bytes = self.record_canonical().map_err(LiveError::io)?;
        Self::parse(&bytes).map_err(LiveError::io)?;
        for (suffix, bytes) in [
            (
                "live",
                result.document.record_canonical().map_err(LiveError::io)?,
            ),
            (
                "run",
                result.record.record_canonical().map_err(LiveError::io)?,
            ),
            ("case", bytes),
        ] {
            let name = format!(
                "{}.{suffix}.json",
                stem.file_name().expect("file stem").to_string_lossy()
            );
            write_atomic(&stem.with_file_name(name), &bytes)?;
        }
        self.record_digest().map_err(LiveError::io)
    }

    pub(crate) fn read(path: &Path) -> Result<Self, LiveError> {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(LiveError::io)?
            // One byte past the document's own parse bound, so an oversized
            // file is refused by `parse`, never truncated into a valid one.
            .take(<Self as Document>::LIMITS.max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(LiveError::io)?;
        let doc = Self::parse(&bytes).map_err(LiveError::io)?;
        doc.check()?;
        Ok(doc)
    }
}

/// A completed work item's record digests, in work order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRunCase {
    pub case_document_digest: ContentDigest,
    pub candidate: String,
    pub repeat: u64,
    pub case_id: String,
    pub rerun: Option<u64>,
    pub live_document_digest: ContentDigest,
    pub run_record_digest: ContentDigest,
}

/// Record-canonical summary of a live run (spec 043, 3.11.4).
// Record canonical bytes sort object keys, so declaration order never
// changes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRun {
    pub schema: String,
    pub declared_no_spend: Vec<BindingIdentity>,
    pub identities: CheckpointBindings,
    pub status: crate::run::RunStatus,
    pub stop_reason: Option<String>,
    pub probe: crate::probe::ProbeReport,
    pub counts: BTreeMap<String, u64>,
    pub run_spend: crate::journal::RunSpend,
    pub journal_before: crate::journal::SpendSummary,
    pub journal_after: crate::journal::SpendSummary,
    pub cases: Vec<LiveRunCase>,
    /// Empty until repeat outputs exist.
    pub repeat_reports: Vec<ContentDigest>,
}
live_document!(LiveRun, "rustev.live-run/1");

impl LiveRun {
    pub fn check(&self) -> Result<(), LiveError> {
        if self.schema != Self::SCHEMA {
            return Err(LiveError::invalid("live-run", "wrong schema"));
        }
        let mut cases = BTreeSet::new();
        for case in &self.cases {
            if !cases.insert((&case.candidate, case.repeat, &case.case_id, case.rerun)) {
                return Err(LiveError::invalid("live-run", "duplicate case"));
            }
        }
        Ok(())
    }
}

impl LiveCheckpoint {
    pub fn check(&self) -> Result<(), LiveError> {
        let invalid = |detail: &str| LiveError::invalid("checkpoint-mismatch", detail);
        if self.schema != Self::SCHEMA {
            return Err(invalid("wrong checkpoint schema"));
        }
        component(&self.bindings.run_id, 64, false).map_err(|e| invalid(&e.detail))?;
        let mut names = BTreeSet::new();
        for c in &self.bindings.candidates {
            component(&c.name, 32, true).map_err(|e| invalid(&e.detail))?;
            if !names.insert(&c.name) {
                return Err(invalid("duplicate candidate"));
            }
        }
        let mut items = BTreeSet::new();
        for item in &self.items {
            decision_id(
                &self.bindings.run_id,
                &item.candidate,
                item.repeat,
                &item.case_id,
                None,
            )
            .map_err(|e| invalid(&e.detail))?;
            if !names.contains(&item.candidate)
                || item.states.is_empty()
                || !items.insert((&item.candidate, item.repeat, &item.case_id))
            {
                return Err(invalid("invalid or duplicate work item"));
            }
            let mut last = 0;
            for state in &item.states {
                if let ItemState::Rerun { k } = state {
                    if *k != last + 1 || *k > 99 {
                        return Err(invalid("invalid rerun sequence"));
                    }
                    last = *k;
                }
            }
        }
        Ok(())
    }

    /// Compare all bindings before the caller opens any spend journal.
    pub fn read(path: impl AsRef<Path>, expected: &CheckpointBindings) -> Result<Self, LiveError> {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(LiveError::io)?
            // One byte past the document's own parse bound, so an oversized
            // file is refused by `parse`, never truncated into a valid one.
            .take(<Self as Document>::LIMITS.max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(LiveError::io)?;
        let doc = Self::parse(&bytes).map_err(|e| LiveError::invalid("checkpoint-mismatch", e))?;
        if &doc.bindings != expected {
            return Err(LiveError::invalid(
                "checkpoint-mismatch",
                "checkpoint bindings differ",
            ));
        }
        doc.check()?;
        Ok(doc)
    }

    /// Replace atomically on the same filesystem, then persist the directory
    /// entry. A failed write before rename preserves the previous checkpoint.
    pub fn write_atomic(&self, path: impl AsRef<Path>) -> Result<(), LiveError> {
        self.check()?;
        let bytes = self.canonical().map_err(LiveError::io)?;
        // Refuse an output our bounded reader could not read.
        Self::parse(&bytes).map_err(|e| LiveError::invalid("checkpoint-mismatch", e))?;
        write_atomic(path.as_ref(), &bytes)
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), LiveError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (temp, mut file) = loop {
        let temp = parent.join(format!(
            ".rustev-checkpoint-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(LiveError::io(e)),
        }
    };
    #[cfg(feature = "fault-injection")]
    let name = path.file_name().unwrap().to_string_lossy();
    let written = (|| {
        #[cfg(feature = "fault-injection")]
        crate::fault_injection::point("atomic", &name, "before-write")?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(feature = "fault-injection")]
        crate::fault_injection::point("atomic", &name, "before-rename")?;
        std::fs::rename(&temp, path)
    })();
    if let Err(e) = written {
        // Before the rename the previous checkpoint is untouched.
        let _ = std::fs::remove_file(&temp);
        return Err(LiveError::io(e));
    }
    // After the rename the new checkpoint is in place, but it is not
    // durable until its directory entry is: a failed directory sync is
    // still an error, since the caller may not rely on the checkpoint
    // surviving a crash. There is no temporary file left to remove.
    #[cfg(feature = "fault-injection")]
    crate::fault_injection::point("atomic", &name, "before-directory-sync")
        .map_err(LiveError::io)?;
    File::open(parent)
        .and_then(|d| d.sync_all())
        .map_err(LiveError::io)?;
    #[cfg(feature = "fault-injection")]
    crate::fault_injection::point("atomic", &name, "after-directory-sync")
        .map_err(LiveError::io)?;
    Ok(())
}
