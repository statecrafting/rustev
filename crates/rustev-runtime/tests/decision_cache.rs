//! Spec 044, 3.5 and 3.6: the decision-result cache's memory tier and its
//! refresh-failure policies, one test per section 5 row it owns. SYNTHETIC fixtures only; the clock is manual
//! and every served judgment is compared with independent execution of the
//! same request on a fresh runtime (3.6.3 item 3).

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use rustev_contract::Document;
use rustev_contract::canonical::{record_canonical_bytes, tagged_digest};
use rustev_contract::definition::{
    Definition, Freshness, InputDecl, ProvenanceClass, StepBody, TypeDecl,
};
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::ids::ContentDigest;
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::optimization::{
    DecisionCacheOutcome, DecisionCachePolicy, DecisionCacheRecord, DecisionCacheTier, ExpiryBasis,
    OptimizationAdmission, OptimizationMechanism, OptimizationOutcome, OptimizationPolicy,
    RefreshFailurePolicy, SubstitutedInput,
};
use rustev_contract::run::{Charge, RequestResult, RunRecord};
use rustev_contract::schema;
use rustev_contract::scope::{Handle as Dimension, Scope};
use rustev_contract::snapshot::Entry;
use rustev_core::builder::e;
use rustev_core::ops::{OpArgs, TimestampDiff};
use rustev_core::seams::{AdapterFailure, BoxFuture, CancelSignal, EvidenceSink};
use rustev_runtime::{
    CaptureConfig, Completion, Decided, DecisionRequest, MAX_CAPTURE_BYTES, ManualClock,
    PreparedPlan, Runtime,
};

const MINUTE: i64 = 60_000;

/// A sink that accepts `rustev.run/3` and checks every record against its
/// version as a reader would.
struct Sink;

impl EvidenceSink for Sink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        let parsed = RunRecord::parse(&record.canonical().unwrap()).map(|r| r == *record);
        Box::pin(async move {
            assert_eq!(parsed, Ok(true), "the run record reads back as itself");
            Ok("receipt".to_string())
        })
    }

    fn accepts(&self, s: &str) -> bool {
        s == schema::RUN || s == schema::RUN_V3
    }
}

/// Support routing as a version 2 definition, whose `frustration` step also
/// projects a value that moves with evaluation time: the minutes since the
/// first failed payment.
fn definition() -> Definition {
    let mut d = support_routing_builder(&topic_calibration("1.5"))
        .exact(
            "minutes_since_first_failure",
            OpArgs::TimestampDiff(TimestampDiff {
                later: e::r("now"),
                earlier: e::r("step:first_failure"),
                unit_ms: MINUTE as u64,
            }),
        )
        .build()
        .unwrap();
    d.schema = schema::DEFINITION_V2.into();
    // A map input is a version 2 construct (spec 032), as in the core's
    // own decision cache tests.
    d.inputs.push(InputDecl {
        name: "ticket.tags".into(),
        ty: TypeDecl::Map {
            key_max_bytes: 16,
            value: Box::new(TypeDecl::Bool),
            max_entries: 4,
        },
        provenance: vec![ProvenanceClass::UserSupplied],
        freshness: Freshness::MaxAgeMs(60_000),
        evidence: None,
        on_disagreement: None,
    });
    let step = d.steps.iter_mut().find(|s| s.id == "frustration").unwrap();
    let StepBody::Semantic(s) = &mut step.body else {
        panic!("frustration is semantic");
    };
    s.project.push("step:minutes_since_first_failure".into());
    d
}

fn policy(
    volatile: &[&str],
    dimensions: &[&str],
    admission: OptimizationAdmission,
) -> ExecutionPolicy {
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
    let mut p = execution(CostPolicy::Unlimited, vec![]);
    p.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: None,
        shared_calls: None,
        persistent_cache: None,
        decision_cache: Some(DecisionCachePolicy {
            tiers: vec![DecisionCacheTier::Memory],
            max_entry_bytes: 65_536,
            max_entries: Some(8),
            ttl_ms: 60_000,
            volatile: names(volatile),
            key_dimensions: names(dimensions),
            on_refresh_failure: RefreshFailurePolicy::Unresolved,
        }),
    });
    p
}

/// No volatile source and no key dimension.
fn plain() -> ExecutionPolicy {
    policy(&[], &[], OptimizationAdmission::Refuse)
}

/// The payment events as the volatile source: every exact step over them
/// and `frustration` form the refresh closure.
fn volatile_payments() -> ExecutionPolicy {
    policy(&["payments.events"], &[], OptimizationAdmission::Refuse)
}

struct Cached {
    rt: Runtime,
    plan: PreparedPlan,
    backend: Arc<Scripted>,
}

fn cached_with(policy: &ExecutionPolicy, backend: Arc<Scripted>) -> Cached {
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Sink),
        config(1, 0, 4),
    )
    .backend(Arc::new(Handle(backend.clone())), 4)
    .build()
    .unwrap();
    let compiled =
        rustev_core::compile_with(&definition(), &descriptors_with_fallbacks(), &cal(), policy)
            .unwrap();
    assert_eq!(compiled.plan.schema, schema::PLAN_V3);
    let plan = rt.prepare(compiled).unwrap();
    Cached { rt, plan, backend }
}

fn cached(policy: &ExecutionPolicy) -> Cached {
    cached_with(policy, answering(linear_head()))
}

async fn decide(c: &Cached, req: DecisionRequest) -> Decided {
    c.rt.decide(&c.plan, req, &CancelSignal::new())
        .await
        .unwrap()
}

async fn decide_keyed(c: &Cached, req: DecisionRequest, dims: &[(&str, &str)]) -> Decided {
    let dims = dims
        .iter()
        .map(|(k, v)| (k.to_string(), Dimension::new(*v).unwrap()))
        .collect::<BTreeMap<_, _>>();
    c.rt.decide_keyed(&c.plan, req, &dims, &CancelSignal::new())
        .await
        .unwrap()
}

/// The same request on a fresh runtime with an empty cache.
async fn independent(policy: &ExecutionPolicy, req: DecisionRequest) -> Decided {
    decide(&cached(policy), req).await
}

/// The judgment and core evidence equal independent execution (spec 017,
/// I-1, and spec 044, 3.6.3 item 3).
#[track_caller]
fn same_judgment(served: &Decided, alone: &Decided) {
    assert!(matches!(served.completion, Completion::Judged(_)));
    assert_eq!(served.completion, alone.completion);
    assert_eq!(served.record.core, alone.record.core);
}

fn at(id: &str, evaluation_ms: i64) -> DecisionRequest {
    let mut r = request(id);
    r.evaluation_time = ts(evaluation_ms);
    r
}

fn dc(d: &Decided) -> &DecisionCacheRecord {
    d.record
        .decision_cache
        .as_ref()
        .expect("a decision_cache member")
}

fn index(d: &Decided, step: &str) -> u32 {
    d.record
        .requests
        .iter()
        .position(|r| r.step == step)
        .unwrap() as u32
}

fn indices(d: &Decided, steps: &[&str]) -> Vec<u32> {
    let mut v: Vec<u32> = steps.iter().map(|s| index(d, s)).collect();
    v.sort_unstable();
    v
}

/// Spec 044, 3.6.3 and 3.6.6: a repeated decision with no volatile source is
/// served from the entry with nothing dispatched, its judgment recomputed by
/// the core and equal to independent execution; every reused request names
/// its source attempt and has no new charge.
#[tokio::test(flavor = "current_thread")]
async fn a_repeated_decision_is_served_and_recomputed() {
    let c = cached(&plain());
    let first = decide(&c, request("d1")).await;
    assert_eq!(dc(&first).outcome, DecisionCacheOutcome::Miss);
    assert_eq!(dc(&first).entry_id, None);
    assert!(first.served_from_cache.is_none());
    assert_eq!(c.backend.dispatched().len(), 3);

    let second = decide(&c, request("d2")).await;
    assert_eq!(c.backend.dispatched().len(), 3, "nothing dispatched again");
    let m = dc(&second);
    assert_eq!(m.outcome, DecisionCacheOutcome::Hit);
    assert_eq!(m.reused, [0, 1, 2]);
    assert!(m.refreshed.is_empty() && m.mismatched.is_empty());
    assert_eq!(m.age_ms, Some(0));
    let source = first.record.record_digest().unwrap();
    assert_eq!(m.source_run_record.as_ref(), Some(&source));
    let served = second.served_from_cache.as_ref().unwrap();
    assert_eq!(Some(&served.entry_id), m.entry_id.as_ref());
    assert_eq!(served.source_run_record, source);
    for (r, earlier) in second.record.requests.iter().zip(&first.record.requests) {
        assert!(r.attempts.is_empty());
        let o = r.optimization.as_ref().unwrap();
        assert_eq!(
            (o.mechanism, o.outcome),
            (
                OptimizationMechanism::DecisionCache,
                OptimizationOutcome::Hit
            )
        );
        assert_eq!(o.entry_id, m.entry_id);
        let from = r.supplied_from().unwrap();
        assert_eq!(
            from.reused_from,
            Some(earlier.attempts[0].attempt_id.as_str())
        );
    }
    assert_eq!(second.record.cost.observed, 0);
    same_judgment(&second, &independent(&plain(), request("d2")).await);
}

/// Section 5: a volatile input changes between two identical decisions. The
/// entry is served, the closure is dispatched again, and the judgment
/// reflects the new value as independent execution does.
#[tokio::test(flavor = "current_thread")]
async fn a_changed_volatile_input_refreshes_its_closure() {
    let c = cached(&volatile_payments());
    let first = decide(&c, request("d1")).await;
    assert_eq!(dc(&first).outcome, DecisionCacheOutcome::Miss);
    let mut changed = request("d2");
    changed.snapshot = support_snapshot("pro", &[2], 0);
    let second = decide(&c, changed.clone()).await;
    let m = dc(&second);
    assert_eq!(m.outcome, DecisionCacheOutcome::Refreshed);
    assert_eq!(m.refreshed, [index(&second, "frustration")]);
    assert_eq!(m.reused, indices(&second, &["topic", "explicit_deadline"]));
    assert!(m.mismatched.is_empty() && m.on_refresh_failure.is_none());
    let dispatched = c.backend.dispatched();
    assert_eq!(dispatched.len(), 4);
    assert_eq!(task_of(&dispatched[3]), "support.frustration");
    // The refreshed request ran through the normal path, with its own
    // attempt and charge.
    let refreshed = &second.record.requests[index(&second, "frustration") as usize];
    assert_eq!(refreshed.attempts.len(), 1);
    assert!(
        refreshed
            .optimization
            .as_ref()
            .is_none_or(|o| o.mechanism != OptimizationMechanism::DecisionCache)
    );
    assert_ne!(second.completion, first.completion);
    same_judgment(&second, &independent(&volatile_payments(), changed).await);
}

/// Section 5: a stored request key differs from the new one outside the
/// closure. That request alone is dispatched, the others are supplied from
/// the entry, and the served decision is not admitted.
#[tokio::test(flavor = "current_thread")]
async fn a_moved_request_key_outside_the_closure_is_dispatched_alone() {
    let c = cached(&plain());
    let first = decide(&c, at("d1", NOW)).await;
    let later = at("d2", NOW + MINUTE);
    let second = decide(&c, later.clone()).await;
    let m = dc(&second);
    assert_eq!(m.outcome, DecisionCacheOutcome::Refreshed);
    assert_eq!(m.mismatched, [index(&second, "frustration")]);
    assert!(m.refreshed.is_empty());
    assert_eq!(m.reused, indices(&second, &["topic", "explicit_deadline"]));
    assert!(
        m.diagnostics
            .iter()
            .any(|d| d.starts_with("closure_mismatch: step \"frustration\""))
    );
    assert_eq!(m.age_ms, Some(MINUTE as u64));
    assert_eq!(c.backend.dispatched().len(), 4);
    same_judgment(&second, &independent(&plain(), later).await);
    // The entry it used stays as it is: a third decision is served from the
    // first decision's entry again.
    let third = decide(&c, at("d3", NOW + MINUTE)).await;
    assert_eq!(dc(&third).entry_id, m.entry_id);
    assert_eq!(
        dc(&third).source_run_record,
        Some(first.record.record_digest().unwrap())
    );
    assert_eq!(dc(&third).mismatched, m.mismatched);
}

/// Section 5: a declared key dimension is missing. The decision is neither
/// served nor admitted, and runs or is refused under `admission`; supplied
/// dimensions separate entries, and an undeclared one is ineligible.
#[tokio::test(flavor = "current_thread")]
async fn a_missing_key_dimension_bypasses_or_refuses() {
    let dims = policy(
        &[],
        &["catalogue_revision"],
        OptimizationAdmission::ExecuteIndependent,
    );
    let c = cached(&dims);
    for id in ["d1", "d2"] {
        let d = decide(&c, request(id)).await;
        let m = dc(&d);
        assert_eq!(m.outcome, DecisionCacheOutcome::Bypassed);
        assert_eq!(
            m.diagnostics,
            ["key_dimension_missing: \"catalogue_revision\""]
        );
        assert!(d.served_from_cache.is_none());
    }
    assert_eq!(
        c.backend.dispatched().len(),
        6,
        "never served, never admitted"
    );
    let rev = |r| [("catalogue_revision", r)];
    let miss = decide_keyed(&c, request("d3"), &rev("rev-1")).await;
    assert_eq!(dc(&miss).outcome, DecisionCacheOutcome::Miss);
    let hit = decide_keyed(&c, request("d4"), &rev("rev-1")).await;
    assert_eq!(dc(&hit).outcome, DecisionCacheOutcome::Hit);
    let other = decide_keyed(&c, request("d5"), &rev("rev-2")).await;
    assert_eq!(dc(&other).outcome, DecisionCacheOutcome::Miss);
    let extra = decide_keyed(
        &c,
        request("d6"),
        &[("catalogue_revision", "rev-1"), ("region", "eu")],
    )
    .await;
    assert_eq!(dc(&extra).outcome, DecisionCacheOutcome::Bypassed);
    assert!(dc(&extra).diagnostics[0].starts_with("key_ineligible: dimension \"region\""));

    // Under `admission: refuse` nothing is dispatched; every request is
    // refused and the core still judges.
    let refusing = cached(&policy(
        &[],
        &["catalogue_revision"],
        OptimizationAdmission::Refuse,
    ));
    let d = decide(&refusing, request("d1")).await;
    assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Refused);
    assert!(refusing.backend.dispatched().is_empty());
    assert!(matches!(d.completion, Completion::Judged(_)));
    for r in &d.record.requests {
        assert!(matches!(r.result, RequestResult::Failed(_)));
        let o = r.optimization.as_ref().unwrap();
        assert_eq!(
            (o.mechanism, o.outcome),
            (
                OptimizationMechanism::DecisionCache,
                OptimizationOutcome::Refused
            )
        );
    }
}

/// Section 5: two tenants send identical decisions; no hit across them.
#[tokio::test(flavor = "current_thread")]
async fn two_tenants_never_share_an_entry() {
    let c = cached(&plain());
    decide(&c, request("d1")).await;
    assert_eq!(
        dc(&decide(&c, request("d2")).await).outcome,
        DecisionCacheOutcome::Hit
    );
    let mut other = request("d3");
    other.principal_handle = b"tenant-b".to_vec();
    let d = decide(&c, other).await;
    assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Miss);
    assert_eq!(c.backend.dispatched().len(), 6);
}

/// Spec 044, decision 6 and 3.6.3: a decision entry expires by evaluation
/// time; an entry created after the request's evaluation time is a miss and
/// is kept; invalidation removes it.
#[tokio::test(flavor = "current_thread")]
async fn entries_expire_by_evaluation_time_and_invalidate() {
    let c = cached(&volatile_payments());
    decide(&c, at("d1", NOW)).await;
    let edge = decide(&c, at("d2", NOW + 60_000)).await;
    assert_eq!(dc(&edge).outcome, DecisionCacheOutcome::Refreshed);
    assert_eq!(dc(&edge).age_ms, Some(60_000));
    let expired = decide(&c, at("d3", NOW + 60_001)).await;
    assert_eq!(dc(&expired).outcome, DecisionCacheOutcome::Miss);
    assert_eq!(dc(&expired).diagnostics, ["entry_expired"]);
    // d3 ran as a miss and wrote a new entry, created at its evaluation
    // time.
    let served = decide(&c, at("d4", NOW + 60_001)).await;
    assert_eq!(dc(&served).outcome, DecisionCacheOutcome::Refreshed);
    assert_eq!(dc(&served).age_ms, Some(0));
    let early = decide(&c, at("d5", NOW + 60_000)).await;
    assert_eq!(dc(&early).outcome, DecisionCacheOutcome::Miss);
    assert_eq!(dc(&early).diagnostics, ["entry_from_future"]);
    let result = c.rt.invalidate_plan(&c.plan).unwrap();
    assert!(result.removed_entries >= 1);
    let after = decide(&c, at("d6", NOW + 60_000)).await;
    assert_eq!(dc(&after).outcome, DecisionCacheOutcome::Miss);
    assert_eq!(dc(&after).diagnostics, ["decision cache miss"]);
}

/// Section 5: invalidation advances the generation while a decision runs;
/// its result is not stored.
#[tokio::test(flavor = "current_thread")]
async fn an_invalidation_during_the_decision_keeps_it_out() {
    let c = Arc::new(cached_with(&plain(), gate_all(linear_head())));
    let running = {
        let c = c.clone();
        tokio::spawn(async move { decide(&c, request("d1")).await })
    };
    until(|| c.backend.gated().len() == 3).await;
    c.rt.invalidate_plan(&c.plan).unwrap();
    release_all(&c.backend);
    until(|| running.is_finished()).await;
    let first = running.await.unwrap();
    assert_eq!(dc(&first).outcome, DecisionCacheOutcome::Miss);
    let second = {
        let c = c.clone();
        tokio::spawn(async move { decide(&c, request("d2")).await })
    };
    until(|| c.backend.gated().len() == 3).await;
    release_all(&c.backend);
    until(|| second.is_finished()).await;
    let second = second.await.unwrap();
    assert_eq!(dc(&second).outcome, DecisionCacheOutcome::Miss);
}

/// Spec 044, 3.6.4: a decision with a request outside the closure that has
/// no validated output of target 0 is not admitted.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_request_outside_the_closure_is_not_admitted() {
    let failing = Scripted::new(linear_head(), |c| {
        if c.task() == "support.deadline" {
            Answer::Fail(
                AdapterFailure::Transient,
                "down".into(),
                Charge::Observed { units: 0 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let c = cached_with(&plain(), failing);
    let first = decide(&c, request("d1")).await;
    assert_eq!(dc(&first).outcome, DecisionCacheOutcome::Miss);
    assert!(dc(&first).diagnostics.iter().any(|d| d
        == "decision cache not admitted: step \"explicit_deadline\" has no validated target 0 output"));
    let second = decide(&c, request("d2")).await;
    assert_eq!(dc(&second).outcome, DecisionCacheOutcome::Miss);
}

/// Spec 044, 3.6.5 under `unresolved`: a volatile input without an accepted
/// entry in the new snapshot is supplied as it is; the core yields what
/// independent execution yields, and the record names the failed refresh.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_refresh_is_supplied_as_it_is() {
    let c = cached(&volatile_payments());
    decide(&c, request("d1")).await;
    let mut stale = request("d2");
    // The payment events are past their 300 s freshness.
    stale.snapshot = support_snapshot("pro", &[2, 5], 600_000);
    let d = decide(&c, stale.clone()).await;
    let m = dc(&d);
    // `frustration` reads the stale events through the closure, so the core
    // never requests it: nothing is dispatched again.
    assert_eq!(m.outcome, DecisionCacheOutcome::Hit);
    assert!(d.record.requests.iter().all(|r| r.step != "frustration"));
    assert_eq!(m.on_refresh_failure, Some(RefreshFailurePolicy::Unresolved));
    assert!(
        m.diagnostics
            .iter()
            .any(|x| x.starts_with("refresh_failed: input \"payments.events\""))
    );
    same_judgment(&d, &independent(&volatile_payments(), stale).await);
}

fn outcome(c: &Completion) -> Option<&Outcome> {
    match c {
        Completion::Judged(j) => Some(&j.outcome),
        Completion::Cancelled => None,
    }
}

/// The payment events as the volatile source, under
/// `serve_cached_within_freshness` (spec 044, 3.6.5).
fn serving_stored() -> ExecutionPolicy {
    let mut p = volatile_payments();
    let o = p.optimization.as_mut().unwrap();
    o.decision_cache.as_mut().unwrap().on_refresh_failure =
        RefreshFailurePolicy::ServeCachedWithinFreshness;
    p
}

/// `req` with no payment events in its snapshot: the volatile input has no
/// accepted entry.
fn without_payments(mut req: DecisionRequest) -> DecisionRequest {
    req.snapshot
        .entries
        .retain(|e| e.field != "payments.events");
    req
}

fn payments(req: &DecisionRequest) -> Entry {
    req.snapshot
        .entries
        .iter()
        .find(|e| e.field == "payments.events")
        .unwrap()
        .clone()
}

/// `req` given the stored entry, as the substitution supplies it.
fn with_entry(mut req: DecisionRequest, entry: Entry) -> DecisionRequest {
    req.snapshot.entries.push(entry);
    req
}

/// The entry digest the run record names a substituted input by (3.6.6).
fn entry_digest(e: &Entry) -> ContentDigest {
    let bytes = record_canonical_bytes(e).unwrap();
    ContentDigest::parse(&tagged_digest("rustev.decision-input/1", &bytes)).unwrap()
}

/// Section 5: refresh fails under `serve_cached_within_freshness` with the
/// value inside its maximum age. The volatile input has no accepted entry in
/// the new snapshot, so the stored entry is supplied with its original as-of
/// time; it is fresh, so its closure is dispatched and the decision is
/// served with `refresh_failed`. The judgment equals independent execution
/// given the same supplied values, never the failure turned into a value.
#[tokio::test(flavor = "current_thread")]
async fn a_missing_volatile_input_is_served_from_the_entry_within_its_max_age() {
    let c = cached(&serving_stored());
    let source = request("d1");
    decide(&c, source.clone()).await;
    let later = without_payments(at("d2", NOW + MINUTE));
    let d = decide(&c, later.clone()).await;
    let m = dc(&d);
    assert_eq!(m.outcome, DecisionCacheOutcome::Refreshed);
    assert_eq!(m.refreshed, [index(&d, "frustration")]);
    assert_eq!(
        m.on_refresh_failure,
        Some(RefreshFailurePolicy::ServeCachedWithinFreshness)
    );
    let stored = payments(&source);
    assert_eq!(
        m.substituted,
        [SubstitutedInput {
            input: "payments.events".into(),
            entry_digest: entry_digest(&stored),
        }]
    );
    assert!(
        m.diagnostics
            .iter()
            .any(|x| x == "refresh_failed: input \"payments.events\"")
    );
    assert!(d.served_from_cache.is_some());
    same_judgment(
        &d,
        &independent(&serving_stored(), with_entry(later.clone(), stored)).await,
    );
    // Supplied as it is, the failure is `missing_evidence`.
    assert_eq!(
        outcome(&d.completion).map(|o| matches!(o, Outcome::Unresolved(_))),
        Some(false)
    );
    assert_eq!(
        outcome(&independent(&serving_stored(), later).await.completion),
        Some(&Outcome::Unresolved(Unresolved::MissingEvidence {
            fields: vec!["payments.events".into()]
        }))
    );
}

/// Section 5: past its maximum age the stored entry is `stale_evidence`, as
/// for any stale input: the core's freshness check decides, so the closure
/// is not requested and nothing older than the declared freshness is
/// supplied as a value.
#[tokio::test(flavor = "current_thread")]
async fn a_stored_entry_past_its_max_age_is_stale_evidence() {
    let c = cached(&serving_stored());
    // Fresh at the source decision: 250 s of a 300 s bound.
    let mut source = request("d1");
    source.snapshot = support_snapshot("pro", &[2, 5], 250_000);
    let first = decide(&c, source.clone()).await;
    assert!(
        first
            .record
            .requests
            .iter()
            .any(|r| r.step == "frustration")
    );
    // A minute later the stored entry is 310 s old.
    let later = without_payments(at("d2", NOW + MINUTE));
    let d = decide(&c, later.clone()).await;
    let m = dc(&d);
    let stored = payments(&source);
    assert_eq!(m.substituted.len(), 1);
    assert_eq!(m.substituted[0].entry_digest, entry_digest(&stored));
    assert_eq!(m.outcome, DecisionCacheOutcome::Hit);
    assert!(d.record.requests.iter().all(|r| r.step != "frustration"));
    let stale = independent(&serving_stored(), with_entry(later.clone(), stored)).await;
    same_judgment(&d, &stale);
    assert_eq!(
        outcome(&stale.completion),
        Some(&Outcome::Unresolved(Unresolved::StaleEvidence {
            fields: vec!["payments.events".into()]
        })),
        "stale_evidence, as for any stale input"
    );
}

/// Section 5: a refreshed request fails under
/// `serve_cached_within_freshness`. Nothing stored is supplied for it; the
/// failure is supplied as under `unresolved` and the core yields what
/// independent execution yields.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_refreshed_request_is_supplied_as_it_is() {
    // `frustration` fails for the second decision only.
    let failing = || {
        Scripted::new(linear_head(), |c| {
            if c.task() == "support.frustration" && c.attempt_id.starts_with("d2/") {
                Answer::Fail(
                    AdapterFailure::Transient,
                    "down".into(),
                    Charge::Observed { units: 0 },
                )
            } else {
                ok(c.task(), 1)
            }
        })
    };
    let c = cached_with(&serving_stored(), failing());
    decide(&c, request("d1")).await;
    let mut changed = request("d2");
    changed.snapshot = support_snapshot("pro", &[2], 0);
    let d = decide(&c, changed.clone()).await;
    let m = dc(&d);
    assert_eq!(m.outcome, DecisionCacheOutcome::Refreshed);
    assert!(m.substituted.is_empty());
    assert_eq!(
        m.on_refresh_failure,
        Some(RefreshFailurePolicy::ServeCachedWithinFreshness)
    );
    assert!(
        m.diagnostics
            .iter()
            .any(|x| x == "refresh_failed: step \"frustration\"")
    );
    let frustration = &d.record.requests[index(&d, "frustration") as usize];
    assert!(matches!(frustration.result, RequestResult::Failed(_)));
    let alone = decide(&cached_with(&serving_stored(), failing()), changed).await;
    same_judgment(&d, &alone);
}

/// Spec 044, 3.6.5: a decision under capture is not substituted, since its
/// capture could not carry the stored entries a replay needs; the failure
/// is supplied as it is.
#[tokio::test(flavor = "current_thread")]
async fn a_captured_decision_is_never_substituted() {
    let c = cached(&serving_stored());
    decide(&c, request("d1")).await;
    let later = without_payments(at("d2", NOW + MINUTE));
    let config = CaptureConfig {
        scope: Scope::tenant_only(
            Dimension::new("tenant-a").unwrap(),
            Dimension::new("rev-1").unwrap(),
        ),
        max_bytes: MAX_CAPTURE_BYTES,
    };
    let captured =
        c.rt.decide_with_capture(&c.plan, later.clone(), &CancelSignal::new(), &config)
            .await
            .unwrap();
    let d = captured.result.unwrap();
    let m = dc(&d);
    assert!(m.substituted.is_empty());
    assert!(
        m.diagnostics
            .iter()
            .any(|x| x == "stored volatile entries are not substituted under capture")
    );
    same_judgment(&d, &independent(&serving_stored(), later).await);
}

/// The payment events as the volatile source, under `recompute`.
fn recomputing() -> ExecutionPolicy {
    let mut p = volatile_payments();
    p.optimization
        .as_mut()
        .unwrap()
        .decision_cache
        .as_mut()
        .unwrap()
        .on_refresh_failure = RefreshFailurePolicy::Recompute;
    p
}

/// Every attempt in the record's requests ends in `r<n>`: the rerun's own
/// namespace (spec 044, 3.6.5 item 2).
#[track_caller]
fn all_rerun(d: &Decided) {
    for r in &d.record.requests {
        for a in &r.attempts {
            let n = a.attempt_id.rsplit('/').next().unwrap();
            assert!(
                n.strip_prefix('r')
                    .is_some_and(|n| n.parse::<u32>().is_ok()),
                "{}",
                a.attempt_id
            );
        }
    }
}

/// Spec 044, 3.6.5 under `recompute`, section 5's row "Refresh fails under
/// `recompute`": a refreshed request fails, so the served run is abandoned
/// once it has ended and the decision runs again. The rerun's attempt ids
/// are `r<n>`, its requests and core evidence are an independent run's, the
/// served run's request is in `abandoned` with its charge, and the cost
/// summary covers both runs.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_refreshed_request_is_recomputed() {
    // `frustration` fails in the second decision's served run only.
    let failing = || {
        Scripted::new(linear_head(), |c| {
            if c.task() == "support.frustration" && c.attempt_id == "d2/frustration//1" {
                return Answer::Fail(
                    AdapterFailure::Transient,
                    "down".into(),
                    Charge::Observed { units: 2 },
                );
            }
            ok(c.task(), 1)
        })
    };
    let c = cached_with(&recomputing(), failing());
    decide(&c, request("d1")).await;
    let mut changed = request("d2");
    changed.snapshot = support_snapshot("pro", &[2], 0);
    let before = c.backend.dispatched().len();
    let d = decide(&c, changed.clone()).await;
    let m = dc(&d);
    assert_eq!(m.outcome, DecisionCacheOutcome::Miss);
    assert_eq!(m.entry_id, None);
    assert!(m.reused.is_empty() && m.refreshed.is_empty());
    assert_eq!(m.on_refresh_failure, Some(RefreshFailurePolicy::Recompute));
    assert!(
        m.diagnostics
            .iter()
            .any(|x| x == "refresh_failed: step \"frustration\"")
    );
    // The adapter saw the served run's key and then the rerun's.
    let seen = &c.backend.dispatched()[before..];
    assert!(seen.iter().any(|a| a == "d2/frustration//1"), "{seen:?}");
    assert!(seen.iter().any(|a| a == "d2/frustration//r1"), "{seen:?}");
    all_rerun(&d);
    let abandoned = m.abandoned.as_ref().unwrap();
    assert!(abandoned.acquisitions.is_empty());
    let served: Vec<_> = abandoned.requests.iter().map(|r| r.step.as_str()).collect();
    assert!(served.contains(&"frustration"), "{served:?}");
    let frustration = abandoned
        .requests
        .iter()
        .find(|r| r.step == "frustration")
        .unwrap();
    assert!(matches!(frustration.result, RequestResult::Failed(_)));
    assert_eq!(frustration.attempts[0].attempt_id, "d2/frustration//1");
    // The cost summary covers both runs.
    let charged = |rs: &[rustev_contract::run::RequestRecord]| -> u64 {
        rs.iter()
            .flat_map(|r| &r.attempts)
            .map(|a| match a.cost.charge {
                Charge::Observed { units } => units,
                _ => 0,
            })
            .sum()
    };
    assert_eq!(
        d.record.cost.observed,
        charged(&d.record.requests) + charged(&abandoned.requests)
    );
    assert!(charged(&abandoned.requests) >= 2);
    // Requests and core evidence are an independent run's.
    let alone = decide(&cached(&recomputing()), changed).await;
    same_judgment(&d, &alone);
    assert_eq!(d.record.requests.len(), alone.record.requests.len());
    for (a, b) in d.record.requests.iter().zip(&alone.record.requests) {
        assert_eq!(
            (&a.step, &a.instance, &a.result),
            (&b.step, &b.instance, &b.result)
        );
    }
}

/// Spec 044, 3.6.5 under `recompute`: a volatile input with no accepted
/// entry in the new snapshot fails the refresh before anything is
/// dispatched, so the rerun abandons nothing and `abandoned` is omitted.
#[tokio::test(flavor = "current_thread")]
async fn a_missing_volatile_input_is_recomputed() {
    let c = cached(&recomputing());
    decide(&c, request("d1")).await;
    let mut stale = request("d2");
    stale.snapshot = support_snapshot("pro", &[2, 5], 600_000);
    let d = decide(&c, stale.clone()).await;
    let m = dc(&d);
    assert_eq!(m.outcome, DecisionCacheOutcome::Miss);
    assert_eq!(m.abandoned, None);
    assert_eq!(m.on_refresh_failure, Some(RefreshFailurePolicy::Recompute));
    assert!(
        m.diagnostics
            .iter()
            .any(|x| x.starts_with("refresh_failed: input \"payments.events\""))
    );
    assert!(!d.record.requests.is_empty());
    all_rerun(&d);
    same_judgment(&d, &independent(&recomputing(), stale).await);
}

/// Spec 044, 3.6.5 item 3: a captured decision under `recompute` captures
/// the rerun's supplies alone, each naming an `r<n>` attempt.
#[tokio::test(flavor = "current_thread")]
async fn a_recomputed_decision_captures_the_rerun_alone() {
    let failing = Scripted::new(linear_head(), |c| {
        if c.task() == "support.frustration" && c.attempt_id == "d2/frustration//1" {
            Answer::Fail(
                AdapterFailure::Transient,
                "down".into(),
                Charge::Observed { units: 0 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let c = cached_with(&recomputing(), failing);
    decide(&c, request("d1")).await;
    let mut changed = request("d2");
    changed.snapshot = support_snapshot("pro", &[2], 0);
    let captured =
        c.rt.decide_with_capture(
            &c.plan,
            changed,
            &CancelSignal::new(),
            &CaptureConfig {
                scope: Scope::tenant_only(
                    Dimension::new("tenant-a").unwrap(),
                    Dimension::new("rev-1").unwrap(),
                ),
                max_bytes: MAX_CAPTURE_BYTES,
            },
        )
        .await
        .unwrap();
    let d = captured.result.unwrap();
    assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Miss);
    assert!(dc(&d).abandoned.is_some());
    all_rerun(&d);
    let rustev_runtime::CaptureOutcome::Captured(capture) = captured.capture else {
        panic!("captured");
    };
    let text = format!("{capture:?}");
    assert!(text.contains("d2/frustration//r1"), "{text}");
    assert!(!text.contains("d2/frustration//1\""), "{text}");
}

/// Spec 044, 3.6.4: the rerun is a miss that ran normally, so it is
/// admitted, and the next identical decision is served from its entry.
#[tokio::test(flavor = "current_thread")]
async fn a_recomputed_decision_is_admitted() {
    let failing = Scripted::new(linear_head(), |c| {
        if c.task() == "support.frustration" && c.attempt_id == "d2/frustration//1" {
            Answer::Fail(
                AdapterFailure::Transient,
                "down".into(),
                Charge::Observed { units: 0 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let c = cached_with(&recomputing(), failing);
    decide(&c, request("d1")).await;
    let mut changed = request("d2");
    changed.snapshot = support_snapshot("pro", &[2], 0);
    let d = decide(&c, changed.clone()).await;
    assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Miss);
    let source = d.record.record_digest().unwrap();
    changed.decision_id = "d3".into();
    let next = decide(&c, changed).await;
    assert_eq!(dc(&next).outcome, DecisionCacheOutcome::Refreshed);
    assert_eq!(dc(&next).source_run_record.as_ref(), Some(&source));
}
