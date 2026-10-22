//! Spec 028, acceptance and I-1: a property test over seeded, bounded,
//! arbitrary run records, exchange records and valid policies. Unique
//! canaries are planted in every value slot that can carry caller, provider
//! or run-time text or an identifier; each decision is exported through
//! spans, events, logs, metrics and JSONL to in-memory exporters; no canary
//! of a field whose effective action is `drop` appears in any exported
//! byte, and no raw canary of a `digest` field either. Every default-deny
//! field is absent under the default policy (the default policy is one of
//! the generated ones). SYNTHETIC only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use rustev_contract::definition::ProvenanceClass;
use rustev_contract::evidence::InputStatus;
use rustev_contract::execution::FailureClass;
use rustev_contract::judgment::{OutValue, Outcome, Unresolved};
use rustev_contract::redaction::{
    Action, FieldClass, FieldOverride, RedactionPolicy, lineage_class,
};
use rustev_contract::remote::ExchangeRecord;
use rustev_contract::run::{AttemptEnd, CoreEvidence, RunRecord, Transition};
use rustev_contract::trace_fields::{self, ClassRule};
use rustev_core::seams::{BoxFuture, EvidenceSink};
use rustev_otel::jsonl::encode;
use rustev_otel::{
    DecisionTelemetry, ExportError, LogRecord, OtelSink, SeededIds, SinkConfig, TelemetryExporter,
};

/// SplitMix64: a small, seeded generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
}

/// A planted value: the catalogue field and class it was exported under.
struct Planted {
    canary: String,
    field: String,
    class: FieldClass,
}

struct Plant<'r> {
    rng: &'r mut Rng,
    planted: Vec<Planted>,
}

impl Plant<'_> {
    /// A unique canary: `zq` and fourteen letters from `g` to `z`, which no
    /// digest, hex id or number can contain.
    fn canary(&mut self, field: &str, class: FieldClass) -> String {
        let s: String = std::iter::once("zq".to_string())
            .chain((0..14).map(|_| ((b'g' + self.rng.below(20) as u8) as char).to_string()))
            .collect();
        self.planted.push(Planted {
            canary: s.clone(),
            field: field.to_string(),
            class,
        });
        s
    }
}

fn action(rng: &mut Rng, deny: bool) -> Action {
    match rng.below(if deny { 2 } else { 4 }) {
        0 => Action::Drop,
        1 => Action::Digest,
        2 => Action::Allow,
        _ => Action::Truncate {
            bytes: 1 + rng.below(40) as u32,
        },
    }
}

/// A valid policy: the default one, or random class actions (deny classes
/// only `drop` or `digest`) and random per-field overrides.
fn policy(rng: &mut Rng, i: usize) -> RedactionPolicy {
    let mut p = RedactionPolicy::default();
    if i.is_multiple_of(4) {
        return p;
    }
    for c in FieldClass::ALL {
        if rng.chance(1, 2) {
            p.classes.insert(c, action(rng, c.is_deny()));
        }
    }
    let fields: Vec<&str> = trace_fields::CATALOGUE
        .iter()
        .filter(|e| !e.template && e.rule != ClassRule::Fixed)
        .map(|e| e.name)
        .collect();
    let n = rng.below(12);
    for _ in 0..n {
        let f = fields[rng.below(fields.len() as u64) as usize];
        if p.fields.iter().any(|o| o.field == f) {
            continue;
        }
        p.fields.push(FieldOverride {
            field: f.to_string(),
            action: action(rng, false),
        });
    }
    p.validate().unwrap();
    p
}

fn lineage_inputs(r: &RunRecord) -> Vec<ProvenanceClass> {
    let CoreEvidence::Recorded(ev) = &r.core else {
        return vec![];
    };
    ev.judgment
        .lineage
        .inputs
        .iter()
        .map(|f| {
            ev.inputs
                .iter()
                .find(|i| &i.field == f)
                .and_then(|i| match &i.status {
                    InputStatus::Resolved { provenance, .. } => Some(*provenance),
                    InputStatus::Unresolved(_) => None,
                })
                .unwrap_or(ProvenanceClass::AttributedClaim)
        })
        .collect()
}

/// An arbitrary decision built from a reference record, with canaries in
/// every slot, and exchange records for some of its attempts.
fn arbitrary(rng: &mut Rng, i: usize) -> (RunRecord, Vec<ExchangeRecord>, Vec<Planted>) {
    let task = if i.is_multiple_of(2) {
        "support"
    } else {
        "lodging"
    };
    let mut r = record(task);
    let mut p = Plant {
        rng,
        planted: vec![],
    };
    r.decision_id = p.canary("rustev.decision.id", FieldClass::Id);
    if let CoreEvidence::Recorded(ev) = &mut r.core {
        for input in &mut ev.inputs {
            if let InputStatus::Resolved { source, .. } = &mut input.status {
                *source = p.canary("rustev.input.source", FieldClass::Id);
            }
        }
        for n in &mut ev.judgment.notices {
            n.reason = p.canary("rustev.notice.reason", FieldClass::PlanMetadata);
        }
        match p.rng.below(3) {
            0 => {
                let f = p.canary("rustev.outcome.unresolved_fields", FieldClass::PlanMetadata);
                let d = p.canary(
                    "rustev.outcome.unresolved_detail",
                    FieldClass::FreeTextDetail,
                );
                ev.judgment.outcome = Outcome::Unresolved(Unresolved::invalid([f], d));
            }
            1 => {
                let d = p.canary(
                    "rustev.outcome.unresolved_detail",
                    FieldClass::FreeTextDetail,
                );
                ev.judgment.outcome =
                    Outcome::Unresolved(Unresolved::BackendUnavailable { detail: d });
            }
            _ => {}
        }
    }
    let inputs = lineage_inputs(&r);
    if let CoreEvidence::Recorded(ev) = &mut r.core
        && let Outcome::Propose { params, .. } = &mut ev.judgment.outcome
    {
        let field = "rustev.outcome.param.note";
        let class = lineage_class(false, inputs.iter().copied());
        params.insert("note".into(), OutValue::Text(p.canary(field, class)));
    }
    let mut xs = vec![];
    let n_requests = r.requests.len();
    for (ri, req) in r.requests.iter_mut().enumerate() {
        for a in &mut req.attempts {
            a.attempt_id = p.canary("rustev.attempt.id", FieldClass::Id);
            if p.rng.chance(1, 3) {
                a.end = AttemptEnd::Failed {
                    class: FailureClass::Permanent,
                    detail: p.canary("rustev.attempt.failure_detail", FieldClass::FreeTextDetail),
                };
            }
            if p.rng.chance(1, 2) {
                let mut x: ExchangeRecord = exchanges()
                    .into_values()
                    .next()
                    .map(|x| (*x).clone())
                    .unwrap();
                x.members[0].attempt_id = a.attempt_id.clone();
                x.members[0].outcome = rustev_contract::remote::MemberOutcome::Failure {
                    code: rustev_contract::remote::RemoteCode::RemoteError,
                    detail: p.canary("rustev.exchange.member_detail", FieldClass::FreeTextDetail),
                };
                x.generation_id = Some(p.canary("gen_ai.response.id", FieldClass::Id));
                x.retry_after =
                    Some(p.canary("rustev.exchange.retry_after", FieldClass::ThirdParty));
                x.provider_extras = BTreeMap::from([(
                    "note".to_string(),
                    p.canary("rustev.exchange.extra.note", FieldClass::ThirdParty),
                )]);
                x.requested_model =
                    Some(p.canary("gen_ai.request.model", FieldClass::ModelIdentity));
                xs.push(x);
            }
        }
        if ri == n_requests - 1 {
            req.transitions.push(Transition::Stop {
                after: req.attempts.len() as u32,
                reason: p.canary("rustev.transition.reason", FieldClass::FreeTextDetail),
            });
        }
    }
    (r, xs, p.planted)
}

#[derive(Default)]
struct Capture {
    trees: Mutex<Vec<DecisionTelemetry>>,
    logs: Mutex<Vec<LogRecord>>,
}

impl TelemetryExporter for Capture {
    fn export<'a>(
        &'a self,
        batch: &'a [DecisionTelemetry],
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        self.trees.lock().unwrap().extend(batch.iter().cloned());
        Box::pin(async { Ok(()) })
    }

    fn export_logs<'a>(&'a self, logs: &'a [LogRecord]) -> BoxFuture<'a, Result<(), ExportError>> {
        self.logs.lock().unwrap().extend(logs.iter().cloned());
        Box::pin(async { Ok(()) })
    }

    fn takes_logs(&self) -> bool {
        true
    }
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_dropped_or_raw_digested_canary_reaches_any_export() {
    let mut rng = Rng(0x2028_1016);
    let mut checked = [0usize; 2];
    // Allowed canaries that were found: the bytes searched hold values.
    let mut shown = 0usize;
    for i in 0..200 {
        let p = policy(&mut rng, i);
        let (rec, xs, planted) = arbitrary(&mut rng, i);
        let ex = Arc::new(Capture::default());
        let s = OtelSink::with_parts(
            SinkConfig {
                policy: p.clone(),
                key: Some(test_key()),
                logs: true,
                bindings: remote_bindings(),
                ..SinkConfig::default()
            },
            ex.clone(),
            Arc::new(rustev_otel::SystemClock::default()),
            Box::new(SeededIds::new(&(i as u64).to_be_bytes())),
        )
        .unwrap();
        s.register_plan(&plan(if i.is_multiple_of(2) {
            "support"
        } else {
            "lodging"
        }));
        for x in &xs {
            s.record_exchange(x);
        }
        s.deliver(&rec).await.unwrap();
        for _ in 0..1000 {
            if s.stats().spans.in_flight == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let trees = ex.trees.lock().unwrap().clone();
        assert_eq!(trees.len(), 1, "iteration {i}");
        let mut bytes = encode(&trees[0].lines).unwrap();
        bytes.extend(format!("{:?}", ex.logs.lock().unwrap()).into_bytes());
        bytes.extend(format!("{:?}", trees[0].logs).into_bytes());
        bytes.extend(format!("{:?}", s.metrics()).into_bytes());
        for c in &planted {
            let a = p.action(&c.field, c.class);
            let head = &c.canary[..8];
            match a {
                Action::Drop => {
                    checked[0] += 1;
                    assert!(
                        !contains(&bytes, head),
                        "iteration {i}: dropped {} ({}) leaked",
                        c.field,
                        c.class
                    );
                }
                Action::Digest => {
                    checked[1] += 1;
                    assert!(
                        !contains(&bytes, head),
                        "iteration {i}: digested {} ({}) appears raw",
                        c.field,
                        c.class
                    );
                }
                Action::Allow => shown += usize::from(contains(&bytes, &c.canary)),
                Action::Truncate { .. } => {}
            }
        }
    }
    // The run exercised both properties many times.
    assert!(checked[0] > 500 && checked[1] > 200, "{checked:?}");
    assert!(shown > 100, "{shown}");
}
