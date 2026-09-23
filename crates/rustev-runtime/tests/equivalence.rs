//! Spec 003, 3.8: for identical validated step values the runtime's judgment
//! and core evidence equal direct core evaluation's; both reference plans run
//! through the runtime on scripted outputs. SYNTHETIC fixtures only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::judgment::{OutValue, Outcome};
use rustev_contract::output::RawOutput;
use rustev_contract::run::{Charge, CoreEvidence, RequestResult};
use rustev_core::Compiled;
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_runtime::{Completion, DecisionRequest};

/// A small deterministic generator (xorshift64*); no dependency, no seed
/// from the environment.
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn logit(&mut self) -> f64 {
        (self.below(2_001) as f64 - 1_000.0) / 100.0
    }
}

fn options(task: &str) -> &'static [&'static str] {
    match task {
        "support.topic" => &TOPICS,
        "support.frustration" => &["calm", "frustrated", "very_angry"],
        "support.deadline" => &["false", "true"],
        _ => unreachable!(),
    }
}

/// A generated answer: usually valid logits, sometimes a failure or an
/// invalid output.
fn generated(g: &mut Gen, task: &str) -> Answer {
    let opts = options(task);
    match g.below(10) {
        0 => Answer::Fail(
            AdapterFailure::Transient,
            "gen".into(),
            Charge::Observed { units: 1 },
        ),
        1 => Answer::Output(logits(&[(opts[0], 1.0)]), Charge::Observed { units: 1 }),
        _ => {
            let v: Vec<(&str, f64)> = opts.iter().map(|o| (*o, g.logit())).collect();
            Answer::Output(logits(&v), Charge::Observed { units: 1 })
        }
    }
}

/// Supply the core directly with exactly what the run record says was
/// supplied: the scripted output for an output, the recorded reason for a
/// failure.
fn direct(
    c: &Compiled,
    req: &DecisionRequest,
    outputs: &BTreeMap<String, RawOutput>,
    record: &rustev_contract::run::RunRecord,
) -> (
    rustev_contract::judgment::Judgment,
    rustev_contract::evidence::EvidenceRecord,
) {
    let mut ev = Evaluation::start(c, &req.snapshot, req.evaluation_time).unwrap();
    loop {
        let pending = ev.pending();
        if pending.is_empty() {
            break;
        }
        for p in pending {
            let r = record
                .requests
                .iter()
                .find(|r| r.step == p.step && r.instance == p.instance)
                .unwrap();
            let s = match &r.result {
                RequestResult::Output { .. } => Supplied::Output(
                    outputs[&format!("{}|{}", p.step, p.instance.join(","))].clone(),
                ),
                RequestResult::Failed(u) => Supplied::Failed(u.clone()),
                RequestResult::NotSupplied => unreachable!(),
            };
            ev.supply(&p.step, &p.instance, s).unwrap();
        }
    }
    ev.finish(&record.decision_id).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn generated_step_values_give_the_core_judgment() {
    let mut g = Gen(0x5eed_0003);
    let c = support_compiled(None);
    for case in 0..200 {
        let answers: Arc<Mutex<BTreeMap<String, Answer>>> = Default::default();
        for task in ["support.topic", "support.frustration", "support.deadline"] {
            answers
                .lock()
                .unwrap()
                .insert(task.into(), generated(&mut g, task));
        }
        let a = answers.clone();
        let head = Scripted::new(linear_head(), move |call| {
            a.lock().unwrap()[call.task()].clone()
        });
        let rig = rig(config(1, 0, 3), &[(head, 4)]);
        let plan = Arc::new(rig.rt.prepare(c.clone()).unwrap());
        let mut req = request(&format!("case-{case}"));
        req.snapshot = support_snapshot(
            ["free", "pro", "enterprise"][g.below(3) as usize],
            &[g.below(40) as i64, g.below(40) as i64],
            (g.below(2) * 400_000) as i64,
        );
        let d = decided(
            start_req(&rig, &plan, req.clone(), &CancelSignal::new())
                .await
                .unwrap(),
        );
        let Completion::Judged(j) = &d.completion else {
            panic!("case {case}")
        };
        let outputs: BTreeMap<String, RawOutput> = d
            .record
            .requests
            .iter()
            .filter_map(|r| {
                let task = task_of(&format!("x/{}/", r.step));
                match &answers.lock().unwrap()[task] {
                    Answer::Output(o, _) => {
                        Some((format!("{}|{}", r.step, r.instance.join(",")), o.clone()))
                    }
                    _ => None,
                }
            })
            .collect();
        let (core_j, core_e) = direct(&c, &req, &outputs, &d.record);
        assert_eq!(**j, core_j, "case {case}");
        let CoreEvidence::Recorded(e) = &d.record.core else {
            panic!()
        };
        assert_eq!(**e, core_e, "case {case}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn support_routing_runs_through_the_runtime_to_the_expected_judgment() {
    let rig = rig(config(1, 0, 3), &[(answering(linear_head()), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    // Billing topic with two recent failures: billing priority (spec 002's
    // reference expectation).
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let Completion::Judged(j) = &d.completion else {
        panic!()
    };
    let Outcome::Propose { action, params } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    assert_eq!(action, "route_ticket");
    assert_eq!(params["queue"], OutValue::Enum("billing-priority".into()));
}

fn lodging_output(call: &Call) -> Answer {
    // `<decision>/<step>/<instance joined by ','>/<n>` (spec 003, 3.9.2).
    let inst: Vec<&str> = call
        .attempt_id
        .split('/')
        .nth(2)
        .map(|i| i.split(',').collect())
        .unwrap_or_default();
    let out = match call.task() {
        "lodging.intent" => logits(&[
            ("business", 2.0),
            ("leisure", 0.0),
            ("family", 0.0),
            ("other", 0.0),
        ]),
        "lodging.supports_preference" => {
            let p: f64 = if inst == ["h-mid", "c-quiet"] {
                0.9
            } else {
                0.4
            };
            logits(&[("false", 0.0), ("true", (p / (1.0 - p)).ln())])
        }
        "lodging.suitability" => logits(&[
            ("poor", -30.0),
            ("fair", 0.0),
            ("good", 0.0),
            ("excellent", -30.0),
        ]),
        other => panic!("{other}"),
    };
    Answer::Output(out, Charge::Observed { units: 1 })
}

#[tokio::test(flavor = "current_thread")]
async fn lodging_runs_through_the_runtime_and_equals_direct_evaluation() {
    let c = rustev_core::compile(&lodging(), &descriptors(), &[]).unwrap();
    let head = Scripted::new(linear_head(), lodging_output);
    let rig = rig(config(1, 0, 4), &[(head.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(c.clone()).unwrap());
    let candidates = vec![
        candidate("h-cheap", "100", "USD", 2, false),
        candidate("h-euro", "120", "EUR", 4, true),
        candidate("h-mid", "150", "USD", 2, true),
        candidate("h-small", "90", "USD", 1, true),
        candidate("h-dear", "900", "USD", 4, true),
        candidate("h-yen", "30000", "JPY", 2, true),
    ];
    let claims = vec![
        claim("c-quiet", "verified", "preference", NOW + 30 * DAY),
        claim("c-pool", "inferred", "preference", NOW + 30 * DAY),
        claim("c-old", "stated", "preference", NOW - DAY),
        claim("c-access", "stated", "constraint", NOW + 30 * DAY),
    ];
    let req = DecisionRequest {
        decision_id: "lodging-1".into(),
        snapshot: lodging_snapshot(true, candidates, claims),
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: vec![],
    };
    let d = decided(
        start_req(&rig, &plan, req.clone(), &CancelSignal::new())
            .await
            .unwrap(),
    );
    let Completion::Judged(j) = &d.completion else {
        panic!()
    };
    // Fan-out requests appear only after their inputs resolve; the runtime
    // drives the core's stages until nothing is pending.
    assert!(d.record.requests.len() > 8, "{}", d.record.requests.len());
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    let OutValue::Ranking(r) = &params["ranking"] else {
        panic!()
    };
    assert_eq!(r.entries[0].candidate, "h-mid");
    // Equal to direct core evaluation over the same values.
    let mut outputs = BTreeMap::new();
    for r in &d.record.requests {
        let call = Call {
            attempt_id: format!("x/{}/{}/1", r.step, r.instance.join(",")),
            n: 1,
            projection: serde_json::json!({
                "task": match r.step.as_str() {
                    "trip_intent" => "lodging.intent",
                    "supports_preference" => "lodging.supports_preference",
                    _ => "lodging.suitability",
                },
            }),
        };
        let Answer::Output(o, _) = lodging_output(&call) else {
            unreachable!()
        };
        outputs.insert(format!("{}|{}", r.step, r.instance.join(",")), o);
    }
    let (core_j, _) = direct(&c, &req, &outputs, &d.record);
    assert_eq!(**j, core_j);
}
