//! The probe of every binding before any dataset case (spec 043, 3.7).
//!
//! Static checks first, with no dispatch: each reached binding's first
//! candidate plan prepares against the runtime, the registered descriptor
//! names exactly the binding's backend and artifact, and no binding's cost
//! model is `unknown`. Then each binding receives `probe.calls` sequential
//! live decisions of that plan on the probe snapshot, through the host's
//! authorizer and the journal gate, like any live attempt.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::Arc;

use rustev_contract::ids::{ArtifactId, SnapshotId};
use rustev_contract::live::AuthorizationOutcome;
use rustev_contract::plan::{PlanExecution, PlanStepDetail};
use rustev_contract::remote::Disclosure;
use rustev_contract::run::{AttemptEnd, CostModel, RunRecord};
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified};
use rustev_core::Compiled;
use rustev_core::seams::CancelSignal;
use rustev_runtime::{
    Clock, LiveAuthorizer, LiveRefusal, LiveRequest, LiveResult, LiveSpend, PreparedPlan, Runtime,
    SpendBinding, SpendUnit,
};
use serde::Serialize;

use crate::JournalGate;
use crate::documents::{BindingIdentity, CostUnit, LiveConfig, LiveError, Probe, ProbeProvenance};

/// The probe snapshot, read and identified anew (spec 043, 3.7.2). Loading
/// it opens no journal, so every refusal here precedes any spend.
#[derive(Debug, Clone)]
pub struct ProbeSnapshot {
    snapshot: Snapshot,
    id: SnapshotId,
    provenance: ProbeProvenance,
}

impl ProbeSnapshot {
    /// Read `probe.snapshot` within the snapshot parse bound.
    pub fn load(probe: &Probe, case_snapshots: &[SnapshotId]) -> Result<Self, LiveError> {
        let mut bytes = Vec::new();
        std::fs::File::open(&probe.snapshot)
            .and_then(|f| {
                f.take(Snapshot::LIMITS.max_bytes as u64 + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|e| LiveError {
                status: "io_error",
                subject: "probe-snapshot",
                detail: e.to_string(),
            })?;
        Self::from_bytes(probe, &bytes, case_snapshots)
    }

    /// Refused `probe-snapshot` when the recomputed digest is not the
    /// declared one or equals any case snapshot's.
    pub fn from_bytes(
        probe: &Probe,
        bytes: &[u8],
        case_snapshots: &[SnapshotId],
    ) -> Result<Self, LiveError> {
        let refused = |detail: &str| LiveError::invalid("probe-snapshot", detail);
        let snapshot = Snapshot::parse(bytes).map_err(|e| refused(&e.to_string()))?;
        let id = snapshot.id().map_err(|e| refused(&e.to_string()))?;
        if id != probe.digest {
            return Err(refused("the recomputed digest differs from probe.digest"));
        }
        if case_snapshots.contains(&id) {
            return Err(refused("the probe snapshot is a case snapshot"));
        }
        Ok(Self {
            snapshot,
            id,
            provenance: probe.provenance,
        })
    }

    pub fn id(&self) -> &SnapshotId {
        &self.id
    }
}

/// Everything one probe needs besides the authorizer, gate and signal.
pub struct ProbeInputs<'a> {
    pub runtime: &'a Runtime,
    /// The checked configuration (`LiveConfig::check`).
    pub config: &'a LiveConfig,
    /// Candidate plans by name; a binding is probed with the first plan, in
    /// name order, that reaches it as a primary or fallback target.
    pub candidates: &'a BTreeMap<String, Compiled>,
    pub snapshot: &'a ProbeSnapshot,
    /// The run's fixed evaluation time, or the host's current time.
    pub evaluation_time: Timestamp,
    pub principal_handle: Vec<u8>,
    pub subject: Handle,
    pub scope: Scope,
    /// The runtime's clock; calls to one binding are spaced on it.
    pub clock: &'a dyn Clock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    Probed,
    ProbeFailed,
    /// A reservation was refused; no later attempt of the run was sent.
    SpendCapped,
    /// The caller's signal was raised; no later call was made.
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityVerdict {
    Matched,
    Unverified,
    Mismatched,
}

/// The first failing call (1-based), or `None` for a static check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeFailure {
    pub call: Option<u64>,
    pub code: String,
}

/// One probe decision, whose purpose is `probe` (3.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeCall {
    pub decision_id: String,
    /// Attempts to the probed binding and their served identities, as the
    /// backend reported them (`None` is `unknown`).
    pub attempts: u64,
    pub served: Vec<Option<String>>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BindingProbe {
    pub identity: BindingIdentity,
    pub candidate: String,
    pub calls: Vec<ProbeCall>,
    pub reachable: bool,
    /// `None` until a call returned output to this binding.
    pub served_identity: Option<IdentityVerdict>,
    pub failure: Option<ProbeFailure>,
}

/// The probe section of `rustev.live-run/1` (3.11.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeReport {
    pub snapshot_digest: SnapshotId,
    pub provenance: ProbeProvenance,
    pub status: ProbeStatus,
    pub bindings: Vec<BindingProbe>,
    pub spend_refusal: Option<String>,
}

/// Probe decisions are `live/<run_id>/.probe/<binding>/<call>`: run spend
/// (3.5.3) counts them, and no candidate name starts with `.` (3.3.4).
const PURPOSE: &str = ".probe";
const MISMATCH: &str = "served-identity-mismatch";

/// The bindings a plan reaches: every primary and fallback target.
pub fn reached(compiled: &Compiled) -> BTreeSet<BindingIdentity> {
    let mut out: BTreeSet<_> = compiled
        .plan
        .steps
        .iter()
        .filter_map(|s| match &s.detail {
            PlanStepDetail::Semantic(b) => Some((&b.backend_id, &b.artifact)),
            PlanStepDetail::Generate(b) => Some((&b.backend_id, &b.artifact)),
            PlanStepDetail::Extract(b) => Some((&b.backend_id, &b.artifact)),
            _ => None,
        })
        .map(|(b, a)| identity(b, a))
        .collect();
    if let PlanExecution::Declared(d) = &compiled.plan.execution {
        out.extend(
            d.fallbacks
                .iter()
                .map(|f| identity(&f.backend_id, &f.artifact)),
        );
    }
    out
}

fn identity(backend_id: &str, artifact: &ArtifactId) -> BindingIdentity {
    BindingIdentity {
        backend_id: backend_id.to_string(),
        artifact: artifact.clone(),
    }
}

/// Probe every configured binding (spec 043, 3.7). `Err` is a refusal
/// before any dispatch: a reached binding with no entry (`live-config`) or
/// a binding whose cost model is `unknown` (`spend-unit`).
pub async fn probe(
    inputs: ProbeInputs<'_>,
    authorizer: &dyn LiveAuthorizer,
    gate: &JournalGate,
    cancel: &CancelSignal,
) -> Result<ProbeReport, LiveError> {
    let ProbeInputs {
        runtime: rt,
        config,
        candidates,
        snapshot,
        ..
    } = inputs;
    let reach: BTreeMap<&String, BTreeSet<BindingIdentity>> =
        candidates.iter().map(|(n, c)| (n, reached(c))).collect();
    let configured: BTreeSet<_> = config.bindings.iter().map(|b| &b.identity).collect();
    if reach.values().flatten().collect::<BTreeSet<_>>() != configured {
        return Err(LiveError::invalid(
            "live-config",
            "configured bindings differ from reachable bindings",
        ));
    }
    for b in &config.bindings {
        if rt
            .backend_terms(&b.identity.backend_id)
            .is_some_and(|(_, model)| model == CostModel::Unknown)
        {
            return Err(LiveError::invalid(
                "spend-unit",
                format!("{} discloses an unknown cost model", b.identity.backend_id),
            ));
        }
    }

    // Static checks: no dispatch, and any failure ends the probe before a call.
    let mut prepared: BTreeMap<&String, Result<Arc<PreparedPlan>, ()>> = BTreeMap::new();
    let mut probes = Vec::new();
    for b in &config.bindings {
        let name = reach
            .iter()
            .find(|(_, set)| set.contains(&b.identity))
            .map(|(n, _)| *n)
            .expect("every configured binding is reached");
        let plan = prepared
            .entry(name)
            .or_insert_with(|| {
                rt.prepare(candidates[name].clone())
                    .map(Arc::new)
                    .map_err(drop)
            })
            .clone();
        let terms = rt.backend_terms(&b.identity.backend_id);
        let failure = if terms.is_some_and(|(d, _)| {
            d.backend_id != b.identity.backend_id || d.artifact != b.identity.artifact
        }) {
            Some("descriptor-identity")
        } else if plan.is_err() {
            Some("check-plan")
        } else {
            None
        };
        probes.push(BindingProbe {
            identity: b.identity.clone(),
            candidate: name.clone(),
            calls: vec![],
            reachable: false,
            served_identity: None,
            failure: failure.map(|code| ProbeFailure {
                call: None,
                code: code.into(),
            }),
        });
    }
    let mut report = ProbeReport {
        snapshot_digest: snapshot.id.clone(),
        provenance: snapshot.provenance,
        status: ProbeStatus::ProbeFailed,
        bindings: vec![],
        spend_refusal: None,
    };
    if probes.iter().any(|p| p.failure.is_some()) {
        report.bindings = probes;
        return Ok(report);
    }

    let spend_bindings = spend_bindings(config);
    let mut stopped = None;
    'bindings: for (index, (p, b)) in probes.iter_mut().zip(&config.bindings).enumerate() {
        let Ok(plan) = prepared[&p.candidate].clone() else {
            unreachable!("static checks passed")
        };
        let mut last_end = None;
        for call in 1..=config.probe.calls {
            if let Some(end) = last_end {
                let mut sleep = inputs
                    .clock
                    .sleep_until(u64::saturating_add(end, config.probe.interval_ms));
                std::future::poll_fn(|cx| {
                    if cancel.poll_raised(cx).is_ready() {
                        return std::task::Poll::Ready(());
                    }
                    sleep.as_mut().poll(cx)
                })
                .await;
            }
            if cancel.is_raised() {
                stopped = Some(ProbeStatus::Interrupted);
                break 'bindings;
            }
            let decision_id = format!("live/{}/{PURPOSE}/{}/{call}", gate.run_id(), index + 1);
            let result = rt
                .reexecute_live_with_gate(
                    &plan,
                    LiveRequest {
                        decision_id: decision_id.clone(),
                        snapshot: snapshot.snapshot.clone(),
                        expected_snapshot_id: Some(snapshot.id.clone()),
                        evaluation_time: inputs.evaluation_time,
                        deadline_ms: Some(b.case_deadline_ms),
                        principal_handle: inputs.principal_handle.clone(),
                        subject: inputs.subject.clone(),
                        scope: inputs.scope.clone(),
                        capture: None,
                        historical: None,
                    },
                    authorizer,
                    cancel,
                    LiveSpend {
                        gate,
                        bindings: &spend_bindings,
                    },
                )
                .await;
            last_end = Some(inputs.clock.now());
            let observed = observe(&p.identity, &decision_id, result);
            if let Some(code) = &observed.spend_refusal {
                report.spend_refusal = Some(code.clone());
                stopped = Some(ProbeStatus::SpendCapped);
            }
            let failed = observed.call.code.clone();
            p.calls.push(observed.call);
            if let Some(code) = failed {
                p.failure = Some(ProbeFailure {
                    call: Some(call),
                    code,
                });
            }
            if stopped.is_none() && cancel.is_raised() {
                stopped = Some(ProbeStatus::Interrupted);
            }
            if stopped.is_some() {
                break 'bindings;
            }
            if p.failure.is_some() {
                break;
            }
        }
    }
    for p in &mut probes {
        // A mismatched identity is still a mapped output: reachable, failed.
        let mapped = |c: &ProbeCall| c.code.as_deref().is_none_or(|c| c == MISMATCH);
        p.reachable = p.calls.iter().all(mapped)
            && (p.calls.len() as u64 == config.probe.calls
                || p.failure.as_ref().is_some_and(|f| f.code == MISMATCH));
        p.served_identity = verdict(p);
    }
    report.status = match stopped {
        Some(s) => s,
        None if probes.iter().all(|p| p.failure.is_none()) => ProbeStatus::Probed,
        None => ProbeStatus::ProbeFailed,
    };
    report.bindings = probes;
    Ok(report)
}

struct Observed {
    call: ProbeCall,
    spend_refusal: Option<String>,
}

/// One call's attempts to `binding`, with the first failing condition.
fn observe(
    binding: &BindingIdentity,
    decision_id: &str,
    result: Result<LiveResult, LiveRefusal>,
) -> Observed {
    let mut call = ProbeCall {
        decision_id: decision_id.to_string(),
        attempts: 0,
        served: vec![],
        code: None,
    };
    let r = match result {
        Ok(r) => r,
        Err(e) => {
            call.code = Some(refusal_code(&e).into());
            return Observed {
                call,
                spend_refusal: None,
            };
        }
    };
    let ours = |backend_id: &str, artifact: &ArtifactId| {
        backend_id == binding.backend_id && *artifact == binding.artifact
    };
    let refused_authorization = r.document.effects.iter().any(|e| {
        ours(&e.backend_id, &e.artifact)
            && matches!(e.authorization, AuthorizationOutcome::Refused { .. })
    });
    let mut ends = vec![];
    for a in r.record.requests.iter().flat_map(|q| &q.attempts) {
        if ours(&a.backend_id, &a.artifact) {
            ends.push(a.end.clone());
            call.served.push(served(&r.record, &a.attempt_id));
        }
    }
    call.attempts = ends.len() as u64;
    let spend_refusal = r.spend_refusal.map(|s| s.code);
    call.code = if let Some(code) = &spend_refusal {
        Some(code.clone())
    } else if refused_authorization {
        Some("authorization_refused".into())
    } else if let Some(end) = ends.iter().find(|e| **e != AttemptEnd::Output) {
        Some(match end {
            AttemptEnd::Failed { class, .. } => class.name().into(),
            AttemptEnd::Deadline => "deadline".into(),
            AttemptEnd::Cancelled => "cancelled".into(),
            AttemptEnd::Output => unreachable!(),
        })
    } else if ends.is_empty() {
        Some("probe-unreached".into())
    } else if call
        .served
        .iter()
        .flatten()
        .any(|s| s != binding.artifact.as_str())
    {
        Some(MISMATCH.into())
    } else {
        None
    };
    Observed {
        call,
        spend_refusal,
    }
}

/// The served identity in the record's remote evidence; absent is `unknown`.
fn served(record: &RunRecord, attempt_id: &str) -> Option<String> {
    let e = record
        .remote_evidence
        .iter()
        .flatten()
        .find(|e| e.attempt_id == attempt_id)?;
    match e.served.disclosure {
        Disclosure::Unknown => None,
        Disclosure::Pinned | Disclosure::Reported => e.served.identity.clone(),
    }
}

/// 3.7.3, in order: `mismatched`, then `unverified`, then `matched`.
fn verdict(p: &BindingProbe) -> Option<IdentityVerdict> {
    let served: Vec<_> = p.calls.iter().flat_map(|c| &c.served).collect();
    if served.is_empty() {
        None
    } else if served.iter().any(|s| {
        s.as_deref()
            .is_some_and(|s| s != p.identity.artifact.as_str())
    }) {
        Some(IdentityVerdict::Mismatched)
    } else if served.iter().any(|s| s.is_none()) {
        Some(IdentityVerdict::Unverified)
    } else {
        Some(IdentityVerdict::Matched)
    }
}

pub(crate) fn refusal_code(e: &LiveRefusal) -> &'static str {
    match e {
        LiveRefusal::SpendAcquisitionUnsupported => "acquisition-unsupported",
        LiveRefusal::Rejected(_) => "rejected",
        LiveRefusal::GenerationFailed(_) => "generation-failed",
        LiveRefusal::SnapshotIdentity { .. } | LiveRefusal::InvalidSnapshot(_) => {
            "identity-mismatch"
        }
        _ => "live-refused",
    }
}

pub(crate) fn spend_bindings(config: &LiveConfig) -> Vec<SpendBinding> {
    config
        .bindings
        .iter()
        .map(|b| SpendBinding {
            backend_id: b.identity.backend_id.clone(),
            artifact: b.identity.artifact.clone(),
            cost_unit: match b.cost_unit {
                CostUnit::NanoUsd => SpendUnit::NanoUsd,
                CostUnit::None => SpendUnit::None,
            },
        })
        .collect()
}
