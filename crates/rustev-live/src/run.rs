//! Dataset-scale execution over resolved inputs (spec 043).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::future::{Future, poll_fn};
use std::path::PathBuf;
use std::pin::Pin;
use std::task::Poll;

use rustev_contract::ids::{ContentDigest, DatasetId, SnapshotId};
use rustev_contract::live::AuthorizationOutcome;
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, Identified, Timestamp};
use rustev_core::Compiled;
use rustev_core::seams::CancelSignal;
use rustev_runtime::{
    Clock, Completion, LiveAuthorizer, LiveRefusal, LiveRequest, LiveResult, LiveSpend, Runtime,
};
use serde::{Deserialize, Serialize};

use crate::JournalGate;
use crate::documents::*;
use crate::journal::{RunSpend, SpendJournal, SpendSummary};
use crate::probe::{self, ProbeInputs, ProbeReport, ProbeSnapshot, ProbeStatus};

pub use crate::reports::ReportInputs;

pub struct RunSpec {
    pub reports: Option<ReportInputs>,
    pub config: LiveConfig,
    pub mode: Mode,
    pub run_id: String,
    pub dataset_id: DatasetId,
    pub split: String,
    pub candidates: BTreeMap<String, Compiled>,
    pub cases: Vec<LiveCase>,
    pub probe_snapshot: ProbeSnapshot,
    pub probe_time: Timestamp,
    pub principal_handle: Vec<u8>,
    pub subject: Handle,
    pub scope: Scope,
    pub input_digests: BTreeMap<String, ContentDigest>,
}

pub struct LiveCase {
    pub case_id: String,
    pub input: CaseInput,
}

/// Dataset identity and split come from `RunSpec`; bundles retain their source link.
pub enum CaseOrigin {
    DatasetCase,
    ReplayBundle {
        bundle_digest: ContentDigest,
        decision_id: String,
    },
}

pub enum CaseInput {
    Available {
        source: CaseOrigin,
        snapshot_bytes: Vec<u8>,
        snapshot_id: SnapshotId,
        source_time: Option<Timestamp>,
        scope: Option<Scope>,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub out: PathBuf,
    pub resume: bool,
    pub rerun_interrupted: bool,
    pub probe_only: bool,
    pub run_deadline_ms: Option<u64>,
}

pub struct PreparedRun {
    spec: RunSpec,
    opts: RunOptions,
    snapshots: BTreeMap<String, Snapshot>,
    checkpoint: LiveCheckpoint,
    completed: BTreeMap<usize, Result<rustev_eval::report::Observation, LiveError>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    EvaluatedLive,
    Probed,
    ProbeFailed,
    SpendCapped,
    Interrupted,
}

/// Exit numbers are provisional under spec 043, 3.14; the status names never change.
pub fn exit_code(status: RunStatus) -> u8 {
    match status {
        RunStatus::EvaluatedLive | RunStatus::Probed => 0,
        RunStatus::SpendCapped => 14,
        RunStatus::ProbeFailed => 15,
        RunStatus::Interrupted => 16,
    }
}

#[derive(Debug, Serialize)]
pub struct RunOutcome {
    pub live_run: LiveRun,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub repeat_reports: BTreeMap<String, rustev_eval::repeat::RepeatReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<rustev_eval::comparison::Comparison>,
    pub status: RunStatus,
    pub probe: ProbeReport,
    /// Base decision id and last checkpoint state, in manifest order.
    /// Interrupted items have the reporting reason `live-interrupted`.
    pub items: Vec<(String, ItemState)>,
    pub run_spend: RunSpend,
    pub journal_before: SpendSummary,
    pub journal_after: SpendSummary,
}

/// Validate all resolved inputs before the caller opens a journal.
pub fn prepare(spec: RunSpec, opts: RunOptions) -> Result<PreparedRun, LiveError> {
    if let Some(reports) = &spec.reports {
        reports.check(&spec)?;
    }
    let reached: Candidates = spec
        .candidates
        .iter()
        .map(|(n, c)| (n.clone(), probe::reached(c).into_iter().collect()))
        .collect();
    spec.config.check(spec.mode, &reached)?;
    let mut items = Vec::new();
    for repeat in 1..=spec.config.repeats {
        for candidate in spec.candidates.keys() {
            for case in &spec.cases {
                decision_id(&spec.run_id, candidate, repeat, &case.case_id, None)?;
                items.push(CheckpointItem {
                    candidate: candidate.clone(),
                    repeat,
                    case_id: case.case_id.clone(),
                    states: vec![ItemState::Pending],
                });
            }
        }
    }
    let mut snapshots = BTreeMap::new();
    let mut names = BTreeSet::new();
    for case in &spec.cases {
        if !names.insert(&case.case_id) {
            return Err(LiveError::invalid("live-config", "duplicate case id"));
        }
        if let CaseInput::Available {
            snapshot_bytes,
            snapshot_id,
            source_time,
            ..
        } = &case.input
        {
            let snapshot = Snapshot::parse(snapshot_bytes)
                .map_err(|e| LiveError::invalid("identity-mismatch", e))?;
            if snapshot
                .id()
                .map_err(|e| LiveError::invalid("identity-mismatch", e))?
                != *snapshot_id
            {
                return Err(LiveError::invalid(
                    "identity-mismatch",
                    "case snapshot digest differs",
                ));
            }
            if spec.config.evaluation_time == EvaluationTime::Source && source_time.is_none() {
                return Err(LiveError::invalid(
                    "evaluation-time-unavailable",
                    &case.case_id,
                ));
            }
            snapshots.insert(case.case_id.clone(), snapshot);
        }
    }
    if spec.probe_snapshot.id() != &spec.config.probe.digest {
        return Err(LiveError::invalid(
            "probe-snapshot",
            "the probe snapshot is not the one probe.digest names",
        ));
    }
    if spec.cases.iter().any(|c| {
        matches!(&c.input, CaseInput::Available { snapshot_id, .. } if snapshot_id == spec.probe_snapshot.id())
    }) {
        return Err(LiveError::invalid(
            "probe-snapshot",
            "the probe snapshot is a case snapshot",
        ));
    }
    let bindings = CheckpointBindings {
        run_id: spec.run_id.clone(),
        live_config_id: spec.config.id().map_err(LiveError::io)?,
        dataset_id: spec.dataset_id.clone(),
        split: spec.split.clone(),
        candidates: spec
            .candidates
            .iter()
            .map(|(name, c)| CandidateBinding {
                name: name.clone(),
                plan_id: c.id.clone(),
                bindings: reached[name].clone(),
            })
            .collect(),
        journal_header_digest: SpendJournal::testing_header_digest(),
        input_digests: spec.input_digests.clone(),
    };
    let mut checkpoint = LiveCheckpoint {
        schema: LiveCheckpoint::SCHEMA.into(),
        bindings,
        items,
    };
    checkpoint.check()?;
    let path = opts.out.join("checkpoint.json");
    if opts.resume {
        if !path.try_exists().map_err(LiveError::io)? {
            return Err(LiveError {
                status: "io_error",
                subject: "checkpoint",
                detail: "no checkpoint exists; nothing was sent because the journal is opened only after the checkpoint exists; a fresh run without resume is safe after removing --out or choosing a new --out".into(),
            });
        }
        let saved = LiveCheckpoint::read(&path, &checkpoint.bindings)?;
        if saved.items.len() != checkpoint.items.len()
            || saved.items.iter().zip(&checkpoint.items).any(|(a, b)| {
                (&a.candidate, a.repeat, &a.case_id) != (&b.candidate, b.repeat, &b.case_id)
            })
        {
            return Err(LiveError::invalid(
                "checkpoint-mismatch",
                "work items differ",
            ));
        }
        checkpoint = saved;
        // Staged reports are rebuilt from the checkpoint and retained case files.
        clean_temps(&opts.out)?;
        let cases = opts.out.join("cases");
        if cases.try_exists().map_err(LiveError::io)? {
            clean_temps(&cases)?;
        }
        let probes = opts.out.join("probe");
        if probes.try_exists().map_err(LiveError::io)? {
            clean_temps(&probes)?;
        }
        let staging = opts.out.join(".repeat-staging");
        if staging.try_exists().map_err(LiveError::io)? {
            std::fs::remove_dir_all(staging).map_err(LiveError::io)?;
        }
    } else {
        std::fs::create_dir(&opts.out).map_err(|e| LiveError {
            status: "io_error",
            subject: "out",
            detail: e.to_string(),
        })?;
        std::fs::create_dir(opts.out.join("cases")).map_err(LiveError::io)?;
        write_atomic(
            &opts.out.join("live-config.json"),
            &spec.config.canonical().map_err(LiveError::io)?,
        )?;
        checkpoint.write_atomic(&path)?;
    }
    Ok(PreparedRun {
        spec,
        opts,
        snapshots,
        checkpoint,
        completed: BTreeMap::new(),
    })
}

/// The first writer wins: the deadline is the cause only when it raises a
/// signal nobody had raised yet.
fn raise_for_deadline(signal: &CancelSignal, deadline_hit: &Cell<bool>) {
    if !signal.is_raised() {
        deadline_hit.set(true);
        signal.raise();
    }
}

// Only the crate's atomic-write leftovers are disposable. Retained outputs
// and unrelated files are never used as staging or removed by recovery.
fn clean_temps(dir: &std::path::Path) -> Result<(), LiveError> {
    for entry in std::fs::read_dir(dir).map_err(LiveError::io)? {
        let entry = entry.map_err(LiveError::io)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".rustev-checkpoint-") && name.ends_with(".tmp") {
            std::fs::remove_file(entry.path()).map_err(LiveError::io)?;
        }
    }
    Ok(())
}

fn rerun(item: &CheckpointItem) -> Option<u64> {
    item.states
        .iter()
        .filter_map(|s| match s {
            ItemState::Rerun { k } => Some(*k),
            _ => None,
        })
        .max()
}
fn current_id(run: &str, item: &CheckpointItem) -> String {
    decision_id(
        run,
        &item.candidate,
        item.repeat,
        &item.case_id,
        rerun(item),
    )
    .expect("validated checkpoint item")
}
fn pending(item: &CheckpointItem) -> bool {
    matches!(
        item.states.last(),
        Some(ItemState::Pending | ItemState::Rerun { .. })
    )
}

type Work<'a> = Pin<Box<dyn Future<Output = (usize, Result<LiveResult, LiveRefusal>)> + 'a>>;

impl PreparedRun {
    fn save(&self) -> Result<(), LiveError> {
        self.checkpoint
            .write_atomic(self.opts.out.join("checkpoint.json"))
    }

    /// Each admitted item holds every binding reached by its plan until it
    /// settles. This conservatively bounds concurrent items per binding;
    /// scheduling of attempts within an item remains the runtime's job.
    pub async fn execute(
        mut self,
        runtime: &Runtime,
        journal: SpendJournal,
        authorizer: &dyn LiveAuthorizer,
        clock: &dyn Clock,
        cancel: &CancelSignal,
    ) -> Result<RunOutcome, LiveError> {
        let deadline = self
            .opts
            .run_deadline_ms
            .map(|d| clock.now().saturating_add(d));
        let signal = cancel.child();
        let gate = JournalGate::new(
            journal.clone(),
            &self.spec.run_id,
            self.spec.config.run_cap_units,
        )
        .map_err(|e| LiveError::invalid("journal-budget", e.code))?;
        let before = journal.summary();
        for item in &mut self.checkpoint.items {
            if pending(item) {
                let ids = journal.reservations_for(&current_id(&self.spec.run_id, item));
                if !ids.is_empty() {
                    item.states
                        .push(ItemState::Interrupted { attempt_ids: ids });
                }
            }
            if matches!(item.states.last(), Some(ItemState::Interrupted { .. }))
                && self.opts.rerun_interrupted
            {
                let k = rerun(item).unwrap_or(0) + 1;
                item.states.push(if k > 99 {
                    ItemState::Refused {
                        code: "rerun-limit".into(),
                    }
                } else {
                    ItemState::Rerun { k }
                });
            }
        }
        self.save()?;
        let inputs = ProbeInputs {
            runtime,
            config: &self.spec.config,
            candidates: &self.spec.candidates,
            snapshot: &self.spec.probe_snapshot,
            evaluation_time: self.spec.probe_time,
            principal_handle: self.spec.principal_handle.clone(),
            subject: self.spec.subject.clone(),
            scope: self.spec.scope.clone(),
            clock,
        };
        // Set only when the deadline raises the signal first; a caller's
        // earlier cancel keeps its own cause even if the deadline then passes.
        let deadline_hit = Cell::new(false);
        let report = {
            let probe_dir = self.opts.out.join("probe");
            std::fs::create_dir_all(&probe_dir).map_err(LiveError::io)?;
            let mut probing = Box::pin(probe::probe_recorded(
                inputs,
                authorizer,
                &gate,
                &signal,
                |index, call, candidate, result| {
                    let doc = LiveCaseDocument::from_result(
                        &self.spec.config,
                        CaseRun {
                            run_id: self.spec.run_id.clone(),
                            live_config_id: self.checkpoint.bindings.live_config_id.clone(),
                            candidate: candidate.into(),
                            repeat: None,
                            rerun: None,
                        },
                        CaseSource::None,
                        &journal,
                        result,
                    )?;
                    doc.write_result(&probe_dir.join(format!("{index}.{call}")), result)
                        .map(Some)
                },
            ));
            let mut timer = deadline.map(|d| clock.sleep_until(d));
            poll_fn(|cx| {
                // A completed timer is dropped, never polled again.
                if timer
                    .as_mut()
                    .is_some_and(|t| t.as_mut().poll(cx).is_ready())
                {
                    timer = None;
                    raise_for_deadline(&signal, &deadline_hit);
                }
                probing.as_mut().poll(cx)
            })
            .await?
        };
        let mut status = match report.status {
            ProbeStatus::Probed if self.opts.probe_only => RunStatus::Probed,
            ProbeStatus::Probed => RunStatus::EvaluatedLive,
            ProbeStatus::ProbeFailed => RunStatus::ProbeFailed,
            ProbeStatus::SpendCapped => RunStatus::SpendCapped,
            ProbeStatus::Interrupted => RunStatus::Interrupted,
        };
        if signal.is_raised() && status != RunStatus::SpendCapped {
            status = RunStatus::Interrupted;
        }
        let mut spend_refusal = report.spend_refusal.clone();
        if status == RunStatus::EvaluatedLive {
            (status, spend_refusal) = self
                .dispatch(
                    runtime,
                    authorizer,
                    clock,
                    &signal,
                    (deadline, &deadline_hit),
                    &gate,
                )
                .await?;
        }
        let stop_reason = match status {
            RunStatus::EvaluatedLive | RunStatus::Probed => None,
            RunStatus::SpendCapped => spend_refusal,
            RunStatus::ProbeFailed => report
                .bindings
                .iter()
                .find_map(|b| b.failure.as_ref().map(|f| f.code.clone())),
            RunStatus::Interrupted => Some(
                if deadline_hit.get() {
                    "run-deadline"
                } else {
                    "cancelled"
                }
                .into(),
            ),
        };
        let mut counts = BTreeMap::from([
            ("pending".into(), 0),
            ("completed".into(), 0),
            ("unavailable".into(), 0),
            ("refused".into(), 0),
            ("interrupted".into(), 0),
        ]);
        let mut cases = Vec::new();
        for item in &self.checkpoint.items {
            let name = match item.states.last().expect("nonempty history") {
                ItemState::Pending | ItemState::Rerun { .. } => "pending",
                ItemState::Completed {
                    live_document_digest,
                    run_record_digest,
                } => {
                    let case_document_digest = LiveCaseDocument::read(
                        &self
                            .opts
                            .out
                            .join("cases")
                            .join(format!("{}.case.json", case_name(item))),
                    )?
                    .record_digest()
                    .map_err(LiveError::io)?;
                    cases.push(LiveRunCase {
                        case_document_digest,
                        candidate: item.candidate.clone(),
                        repeat: item.repeat,
                        case_id: item.case_id.clone(),
                        rerun: rerun(item),
                        live_document_digest: live_document_digest.clone(),
                        run_record_digest: run_record_digest.clone(),
                    });
                    "completed"
                }
                ItemState::Unavailable { .. } => "unavailable",
                ItemState::Refused { .. } => "refused",
                ItemState::Interrupted { .. } => "interrupted",
            };
            *counts.get_mut(name).expect("known state") += 1;
        }
        let reports = if status == RunStatus::EvaluatedLive {
            self.spec
                .reports
                .as_ref()
                .map(|inputs| {
                    crate::reports::build(
                        inputs,
                        &self.spec,
                        &self.opts,
                        &self.checkpoint,
                        &self.completed,
                    )
                })
                .transpose()?
        } else {
            None
        };
        let live_run = LiveRun {
            declared_no_spend: self
                .spec
                .config
                .bindings
                .iter()
                .filter(|b| b.cost_unit == CostUnit::None)
                .map(|b| b.identity.clone())
                .collect(),
            schema: LiveRun::SCHEMA.into(),
            identities: self.checkpoint.bindings.clone(),
            status,
            stop_reason,
            probe: report.clone(),
            counts,
            run_spend: journal.run_split(&self.spec.run_id),
            journal_before: before.clone(),
            journal_after: journal.summary(),
            cases,
            repeat_reports: reports
                .as_ref()
                .map(|r| r.digests())
                .transpose()?
                .unwrap_or_default(),
        };
        live_run.check()?;
        let live_run_bytes = live_run.record_canonical().map_err(LiveError::io)?;
        if let Some(reports) = &reports {
            reports.publish(&self.opts.out)?;
        }
        let (repeat_reports, comparison) = reports
            .map(|r| (r.reports, Some(r.comparison)))
            .unwrap_or_default();
        let outcome = RunOutcome {
            repeat_reports,
            comparison,
            run_spend: live_run.run_spend.clone(),
            journal_after: live_run.journal_after.clone(),
            live_run,
            status,
            probe: report,
            items: self
                .checkpoint
                .items
                .iter()
                .map(|i| {
                    (
                        decision_id(&self.spec.run_id, &i.candidate, i.repeat, &i.case_id, None)
                            .expect("validated id"),
                        i.states.last().expect("nonempty history").clone(),
                    )
                })
                .collect(),
            journal_before: before,
        };
        write_atomic(&self.opts.out.join("live-run.json"), &live_run_bytes)?;
        Ok(outcome)
    }

    async fn dispatch(
        &mut self,
        runtime: &Runtime,
        authorizer: &dyn LiveAuthorizer,
        clock: &dyn Clock,
        signal: &CancelSignal,
        (deadline, deadline_hit): (Option<u64>, &Cell<bool>),
        gate: &JournalGate,
    ) -> Result<(RunStatus, Option<String>), LiveError> {
        let spend = probe::spend_bindings(&self.spec.config);
        let reach: BTreeMap<_, _> = self
            .spec
            .candidates
            .iter()
            .map(|(n, c)| (n.clone(), probe::reached(c)))
            .collect();
        let mut held: BTreeMap<BindingIdentity, u64> = BTreeMap::new();
        let mut last: BTreeMap<BindingIdentity, u64> = BTreeMap::new();
        let mut work: Vec<Work<'_>> = Vec::new();
        let mut next = 0;
        let mut status = RunStatus::EvaluatedLive;
        let mut error = None;
        let mut spend_refusal = None;
        loop {
            if deadline.is_some_and(|d| clock.now() >= d) {
                raise_for_deadline(signal, deadline_hit);
            }
            if signal.is_raised() && status != RunStatus::SpendCapped {
                status = RunStatus::Interrupted;
            }
            let mut wake_at = None;
            if status == RunStatus::EvaluatedLive && error.is_none() {
                while next < self.checkpoint.items.len() {
                    let item = &self.checkpoint.items[next];
                    if !pending(item) {
                        next += 1;
                        continue;
                    }
                    let case = self
                        .spec
                        .cases
                        .iter()
                        .find(|c| c.case_id == item.case_id)
                        .expect("validated case");
                    if let CaseInput::Unavailable { reason } = &case.input {
                        self.checkpoint.items[next]
                            .states
                            .push(ItemState::Unavailable {
                                reason: reason.clone(),
                            });
                        if let Err(e) = self.save() {
                            error = Some(e);
                            break;
                        }
                        next += 1;
                        continue;
                    }
                    let bindings = &reach[&item.candidate];
                    let rules: Vec<_> = self
                        .spec
                        .config
                        .bindings
                        .iter()
                        .filter(|b| bindings.contains(&b.identity))
                        .collect();
                    if rules
                        .iter()
                        .any(|b| held.get(&b.identity).copied().unwrap_or(0) >= b.max_in_flight)
                    {
                        break;
                    }
                    let start = rules
                        .iter()
                        .filter_map(|b| {
                            last.get(&b.identity)
                                .map(|t| t.saturating_add(b.min_interval_ms))
                        })
                        .max()
                        .unwrap_or(0);
                    if start > clock.now() {
                        wake_at = Some(start);
                        break;
                    }
                    let CaseInput::Available {
                        snapshot_id,
                        source_time,
                        scope,
                        ..
                    } = &case.input
                    else {
                        unreachable!()
                    };
                    let request = LiveRequest {
                        decision_id: current_id(&self.spec.run_id, item),
                        snapshot: self.snapshots[&case.case_id].clone(),
                        expected_snapshot_id: Some(snapshot_id.clone()),
                        evaluation_time: match self.spec.config.evaluation_time {
                            EvaluationTime::Fixed { ms } => ms,
                            EvaluationTime::Source => source_time.expect("checked source time"),
                        },
                        deadline_ms: rules.iter().map(|b| b.case_deadline_ms).min(),
                        principal_handle: self.spec.principal_handle.clone(),
                        subject: self.spec.subject.clone(),
                        scope: scope.clone().unwrap_or_else(|| self.spec.scope.clone()),
                        capture: None,
                        historical: None,
                    };
                    let plan = match runtime.prepare(self.spec.candidates[&item.candidate].clone())
                    {
                        Ok(plan) => plan,
                        Err(_) => {
                            self.checkpoint.items[next].states.push(ItemState::Refused {
                                code: "check-plan".into(),
                            });
                            if let Err(e) = self.save() {
                                error = Some(e);
                                break;
                            }
                            next += 1;
                            continue;
                        }
                    };
                    for b in bindings {
                        *held.entry(b.clone()).or_default() += 1;
                        last.insert(b.clone(), clock.now());
                    }
                    let index = next;
                    let spend = &spend;
                    work.push(Box::pin(async move {
                        let result = runtime
                            .reexecute_live_with_gate(
                                &plan,
                                request,
                                authorizer,
                                signal,
                                LiveSpend {
                                    gate,
                                    bindings: spend,
                                },
                            )
                            .await;
                        (index, result)
                    }));
                    next += 1;
                    // Poll admitted work before admitting another item, so an immediate
                    // spend refusal stops admission at its first observable result.
                    break;
                }
            }
            if work.is_empty()
                && (next == self.checkpoint.items.len()
                    || status != RunStatus::EvaluatedLive
                    || error.is_some())
            {
                break;
            }
            let more = status == RunStatus::EvaluatedLive
                && error.is_none()
                && next < self.checkpoint.items.len()
                && wake_at.is_none();
            let mut timer = wake_at
                .into_iter()
                .chain(deadline.filter(|_| !signal.is_raised()))
                .min()
                .map(|d| clock.sleep_until(d));
            let event = poll_fn(|cx| {
                if status == RunStatus::EvaluatedLive && signal.poll_raised(cx).is_ready() {
                    return Poll::Ready(None);
                }
                if deadline.is_some_and(|d| clock.now() >= d) && !signal.is_raised() {
                    raise_for_deadline(signal, deadline_hit);
                    return Poll::Ready(None);
                }
                for (slot, f) in work.iter_mut().enumerate() {
                    if let Poll::Ready(result) = f.as_mut().poll(cx) {
                        return Poll::Ready(Some((slot, result)));
                    }
                }
                if timer
                    .as_mut()
                    .is_some_and(|t| t.as_mut().poll(cx).is_ready())
                {
                    return Poll::Ready(None);
                }
                if more {
                    // Continue only if the next item can acquire all its bindings.
                    let item = &self.checkpoint.items[next];
                    if !pending(item)
                        || self
                            .spec
                            .config
                            .bindings
                            .iter()
                            .filter(|b| reach[&item.candidate].contains(&b.identity))
                            .all(|b| held.get(&b.identity).copied().unwrap_or(0) < b.max_in_flight)
                    {
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            })
            .await;
            let Some((slot, (index, result))) = event else {
                continue;
            };
            drop(work.remove(slot));
            let item = &mut self.checkpoint.items[index];
            for b in &reach[&item.candidate] {
                *held.get_mut(b).expect("held binding") -= 1;
            }
            let state = match result {
                Err(e) => ItemState::Refused {
                    code: probe::refusal_code(&e).into(),
                },
                Ok(r) => {
                    if let Some(s) = r.spend_refusal {
                        spend_refusal.get_or_insert_with(|| s.code.clone());
                        status = RunStatus::SpendCapped;
                        ItemState::Refused { code: s.code }
                    } else if signal.is_raised() && matches!(r.completion, Completion::Cancelled) {
                        // A judgment reached before the signal is kept.
                        ItemState::Interrupted {
                            attempt_ids: r
                                .record
                                .requests
                                .iter()
                                .flat_map(|q| &q.attempts)
                                .map(|a| a.attempt_id.clone())
                                .collect(),
                        }
                    } else if r.record.requests.iter().all(|q| q.attempts.is_empty())
                        && r.document.effects.iter().any(|e| {
                            matches!(e.authorization, AuthorizationOutcome::Refused { .. })
                        })
                    {
                        ItemState::Refused {
                            code: "authorization_refused".into(),
                        }
                    } else {
                        match save_case(&self.opts, &self.spec, gate.journal(), item, &r) {
                            Ok(s) => {
                                if self.spec.reports.is_some() {
                                    let observation = crate::reports::completed(
                                        &r.document,
                                        &r.record,
                                        &self.spec.candidates[&item.candidate].plan,
                                        match &r.completion {
                                            Completion::Judged(j) => Some(j.as_ref()),
                                            Completion::Cancelled => None,
                                        },
                                    );
                                    self.completed.insert(index, observation);
                                }
                                s
                            }
                            Err(e) => {
                                error.get_or_insert(e);
                                continue;
                            }
                        }
                    }
                }
            };
            item.states.push(state);
            if let Err(e) = self.save() {
                error.get_or_insert(e);
            }
        }
        if let Some(e) = error {
            return Err(e);
        }
        Ok((status, spend_refusal))
    }
}

fn case_name(item: &CheckpointItem) -> String {
    let suffix = rerun(item)
        .map(|k| format!(".rerun-{k}"))
        .unwrap_or_default();
    format!(
        "{}.{}.{}{suffix}",
        item.candidate, item.repeat, item.case_id
    )
}

fn save_case(
    opts: &RunOptions,
    spec: &RunSpec,
    journal: &SpendJournal,
    item: &CheckpointItem,
    r: &LiveResult,
) -> Result<ItemState, LiveError> {
    let case = spec
        .cases
        .iter()
        .find(|c| c.case_id == item.case_id)
        .expect("validated case");
    let CaseInput::Available { source, .. } = &case.input else {
        unreachable!()
    };
    let source = match source {
        CaseOrigin::DatasetCase => CaseSource::DatasetCase {
            dataset_id: spec.dataset_id.clone(),
            case_id: case.case_id.clone(),
            split: spec.split.clone(),
        },
        CaseOrigin::ReplayBundle {
            bundle_digest,
            decision_id,
        } => CaseSource::ReplayBundle {
            bundle_digest: bundle_digest.clone(),
            decision_id: decision_id.clone(),
        },
    };
    let doc = LiveCaseDocument::from_result(
        &spec.config,
        CaseRun {
            run_id: spec.run_id.clone(),
            live_config_id: spec.config.id().map_err(LiveError::io)?,
            candidate: item.candidate.clone(),
            repeat: Some(item.repeat),
            rerun: rerun(item),
        },
        source,
        journal,
        r,
    )?;
    doc.write_result(&opts.out.join("cases").join(case_name(item)), r)?;
    Ok(ItemState::Completed {
        live_document_digest: doc.live_document_digest,
        run_record_digest: doc.run_record_digest,
    })
}
