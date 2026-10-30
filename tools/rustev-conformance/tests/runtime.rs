//! Spec 027, 3.12.4: runtime cases as data. Each case mirrors a test under
//! `crates/rustev-runtime/tests` (admission, budget, retry, deadline and
//! cancellation), scripts its backend per attempt and drives a manual
//! clock; the Rust runner interprets it with Rust seams and must produce the
//! committed run-record bytes, which a binding's runner must produce too.
//! The fixtures come from spec 002's shared builders. `RUSTEV_BLESS=1`
//! rewrites the fixtures, the case files and the expected outputs; without
//! it, each must equal what is committed. The expected records are checked
//! here against what the mirrored test asserts, so a blessed record is
//! never merely whatever the runner printed.

// Spec 002's shared builders, included as source the way the CLI's tests
// include them (`crates/rustev-cli/tests/common/mod.rs`): no crate edge,
// and their dependencies are this crate's dev-dependencies.
#[path = "../../../crates/rustev-core/tests/common/mod.rs"]
mod builders;

use std::collections::BTreeMap;

use builders::*;
use rustev_conformance::runtime::{self, RuntimeCase};
use rustev_conformance::{compare, manifest, pinned, root, sha256_hex};
use rustev_contract::Document;
use rustev_contract::execution::{CostPolicy, Delay, FailureClass as F};
use rustev_contract::run::RunRecord;
use serde_json::{Value as Json, json};

const DIR: &str = "tools/rustev-conformance";

fn bless() -> bool {
    std::env::var_os("RUSTEV_BLESS").is_some()
}

/// Write `bytes` at `rel` under the suite when blessing; otherwise require
/// that they are what is committed there.
fn keep(rel: &str, bytes: &[u8]) {
    let p = root().join(DIR).join(rel);
    if bless() {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        return;
    }
    let got = std::fs::read(&p).unwrap_or_else(|e| panic!("{rel}: {e}; bless it"));
    assert!(got == bytes, "{rel} is stale; run with RUSTEV_BLESS=1");
}

fn line(mut b: Vec<u8>) -> Vec<u8> {
    b.push(b'\n');
    b
}

/// The fixtures by input name.
fn fixtures() -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    let ds = descriptors_with_fallbacks();
    let cal = vec![topic_calibration("1.5")];
    let plan = |p: Option<&rustev_contract::execution::ExecutionPolicy>| {
        let c = match p {
            None => rustev_core::compile(&support_routing(), &ds, &cal).unwrap(),
            Some(p) => rustev_core::compile_with(&support_routing(), &ds, &cal, p).unwrap(),
        };
        line(c.canonical())
    };
    f.insert("support.plan.json".into(), plan(None));
    let retry = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient],
            Delay::Exponential {
                initial_ms: 10,
                max_ms: 40,
            },
            &[],
            &[],
        )],
    );
    f.insert("retry.plan.json".into(), plan(Some(&retry)));
    let budget = execution(
        CostPolicy::Hard { max_units: 10 },
        vec![step_exec(
            "topic",
            3,
            &[F::Transient],
            Delay::None,
            &[],
            &[],
        )],
    );
    f.insert("budget.plan.json".into(), plan(Some(&budget)));
    for d in &ds {
        f.insert(
            format!("{}.descriptor.json", d.backend_id),
            line(d.canonical().unwrap()),
        );
    }
    f.insert(
        "topic.calibration.json".into(),
        line(cal[0].canonical().unwrap()),
    );
    f.insert(
        "support.snapshot.json".into(),
        line(support_snapshot("pro", &[2, 5], 0).canonical().unwrap()),
    );
    f
}

fn output(task: &str, units: u64) -> Json {
    json!({ "output": { "output": support_output_json(task), "charge": { "observed": { "units": units } } } })
}

/// A valid answer per support-routing task, as the runtime tests give it.
fn support_output_json(task: &str) -> Json {
    match task {
        "support.topic" => {
            json!({ "logits": { "billing": 4.0, "integration_defect": 0.0, "account_access": 0.0, "other": 0.0 } })
        }
        "support.frustration" => {
            json!({ "logits": { "calm": 3.0, "frustrated": 0.0, "very_angry": 0.0 } })
        }
        "support.deadline" => json!({ "logits": { "false": 3.0, "true": 0.0 } }),
        _ => unreachable!(),
    }
}

const TASKS: [&str; 3] = ["support.topic", "support.frustration", "support.deadline"];

fn head(answers: Json, on_release: Json, on_cancel: Json, bound: u64) -> Json {
    json!({
        "descriptor": "synthetic-linear-head.descriptor.json",
        "max_concurrency": 8,
        "cost_model": "bounded",
        "cost_bound": { "bounded": { "max_units": bound } },
        "remote": "finished",
        "on_cancel": on_cancel,
        "answers": answers,
        "on_release": on_release,
    })
}

fn every(f: impl Fn(&str) -> Json) -> Json {
    Json::Object(TASKS.iter().map(|t| (t.to_string(), f(t))).collect())
}

fn request(id: &str, deadline: Option<u64>) -> Json {
    let mut r = json!({ "decision_id": id, "snapshot": "support.snapshot.json",
                        "evaluation_time_ms": NOW });
    if let Some(d) = deadline {
        r["deadline_ms"] = d.into();
    }
    r
}

fn config(in_flight: usize, queued: usize, parallel: usize) -> Json {
    json!({ "max_in_flight": in_flight, "max_queued": queued,
            "max_parallel_requests": parallel, "sink_timeout_ms": 1000 })
}

const DESCRIPTORS: [&str; 4] = [
    "synthetic-linear-head.descriptor.json",
    "synthetic-label-only.descriptor.json",
    "synthetic-linear-head-replica.descriptor.json",
    "synthetic-other-head.descriptor.json",
];

/// The cases: `(id, mirrored test, runtime block)`.
fn cases() -> Vec<(&'static str, &'static str, Json)> {
    let gate = every(|_| json!(["gate"]));
    let release = every(|t| output(t, 1));
    let base = |plan: &str, cfg: Json, backends: Json, requests: Json, drive: Json| {
        json!({ "plan": plan, "descriptors": DESCRIPTORS,
                "calibrations": ["topic.calibration.json"], "config": cfg,
                "backends": backends, "requests": requests, "drive": drive })
    };
    vec![
        (
            "runtime.admission",
            "crates/rustev-runtime/tests/admission.rs::a_full_queue_rejects_at_once_and_queued_work_runs_later",
            base(
                "support.plan.json",
                config(1, 1, 3),
                json!([head(gate.clone(), release.clone(), json!("ignore"), 1)]),
                json!([
                    request("d1", None),
                    request("d2", None),
                    request("d3", None)
                ]),
                json!([
                    "start", { "wait_gated": { "backend": 0, "count": 3 } },
                    "start", { "wait_queued": { "count": 1 } },
                    "start",
                    { "release_all": { "backend": 0 } },
                    { "wait_gated": { "backend": 0, "count": 3 } },
                    { "release_all": { "backend": 0 } }
                ]),
            ),
        ),
        (
            "runtime.budget",
            "crates/rustev-runtime/tests/budget.rs::retries_draw_from_the_same_budget",
            base(
                "budget.plan.json",
                config(1, 0, 1),
                json!([head(
                    {
                        let mut a = every(|t| json!([output(t, 4)]));
                        a["support.topic"] = json!([{ "fail": { "class": "transient", "detail": "flaky", "charge": { "observed": { "units": 4 } } } }]);
                        a
                    },
                    json!({}),
                    json!("ignore"),
                    4
                )]),
                json!([request("d1", None)]),
                json!(["start"]),
            ),
        ),
        (
            "runtime.retry",
            "crates/rustev-runtime/tests/retry.rs::transient_failures_retry_with_exponential_delays",
            base(
                "retry.plan.json",
                config(1, 0, 3),
                json!([head(
                    {
                        let mut a = every(|t| json!([output(t, 1)]));
                        let blip = json!({ "fail": { "class": "transient", "detail": "blip", "charge": { "observed": { "units": 1 } } } });
                        a["support.topic"] = json!([blip, blip, output("support.topic", 1)]);
                        a
                    },
                    json!({}),
                    json!("ignore"),
                    1
                )]),
                json!([request("d1", None)]),
                json!([
                    "start",
                    // Each retry delay's timer, once it is pending.
                    { "advance_timer": { "delay_ms": 10 } },
                    { "advance_timer": { "delay_ms": 20 } }
                ]),
            ),
        ),
        (
            "runtime.deadline",
            "crates/rustev-runtime/tests/time.rs::an_attempt_in_flight_at_the_deadline_is_abandoned_honestly",
            base(
                "support.plan.json",
                config(1, 0, 3),
                json!([head(gate.clone(), json!({}), json!("ignore"), 1)]),
                json!([request("d1", None)]),
                json!([
                    // The deadline's timer is waited for before the clock
                    // moves, whenever the runtime arms it.
                    "start", { "wait_gated": { "backend": 0, "count": 3 } },
                    "wait_sleeper", { "advance_ms": { "ms": 2000 } }
                ]),
            ),
        ),
        (
            "runtime.cancel",
            "crates/rustev-runtime/tests/cancel.rs::no_answer_means_local_waiting_stopped_and_nothing_more",
            base(
                "support.plan.json",
                config(1, 0, 3),
                json!([head(gate.clone(), json!({}), json!("ignore"), 5)]),
                json!([request("d1", None)]),
                json!([
                    "start", { "wait_gated": { "backend": 0, "count": 3 } },
                    { "cancel": { "request": 0 } }
                ]),
            ),
        ),
        (
            "runtime.cancel-stop",
            "crates/rustev-runtime/tests/cancel.rs::a_confirmed_stop_is_recorded_as_stopped_with_its_charge",
            base(
                "support.plan.json",
                config(1, 0, 3),
                json!([head(
                    gate,
                    json!({}),
                    json!({ "stop": { "charge": { "observed": { "units": 2 } } } }),
                    5
                )]),
                json!([request("d1", None)]),
                json!([
                    "start", { "wait_gated": { "backend": 0, "count": 3 } },
                    { "cancel": { "request": 0 } }
                ]),
            ),
        ),
    ]
}

fn record(out: &BTreeMap<String, Vec<u8>>, id: &str) -> RunRecord {
    RunRecord::parse(out[&format!("{id}.record.json")].trim_ascii_end()).unwrap()
}

fn status(out: &BTreeMap<String, Vec<u8>>, id: &str) -> Json {
    serde_json::from_slice(&out[&format!("{id}.result.json")]).unwrap()
}

/// What each mirrored test asserts, on the case's outputs.
fn mirrors(id: &str, out: &BTreeMap<String, Vec<u8>>) {
    use rustev_contract::judgment::Unresolved;
    use rustev_contract::run::{AttemptEnd, Charge, FinalCost, RequestResult, Termination};
    match id {
        "runtime.admission" => {
            assert_eq!(status(out, "d3")["rejection"], "overloaded");
            assert_eq!(status(out, "d1")["status"], "judged");
            assert_eq!(status(out, "d2")["status"], "judged");
            assert!(
                !out.contains_key("d3.record.json"),
                "a rejection has no record"
            );
        }
        "runtime.budget" => {
            let r = record(out, "d1");
            let topic = r.requests.iter().find(|q| q.step == "topic").unwrap();
            assert_eq!(
                topic.attempts.len(),
                2,
                "the third attempt's 4 units do not fit"
            );
            assert!(matches!(
                &topic.result,
                RequestResult::Failed(Unresolved::BudgetExhausted { .. })
            ));
            assert_eq!(r.cost.observed, 8);
        }
        "runtime.retry" => {
            let r = record(out, "d1");
            let t = r.requests.iter().find(|q| q.step == "topic").unwrap();
            assert_eq!(t.result, RequestResult::Output { target: 0 });
            let at: Vec<u64> = t.attempts.iter().map(|a| a.dispatched_ms).collect();
            assert_eq!(at, vec![0, 10, 30]);
        }
        "runtime.deadline" => {
            let r = record(out, "d1");
            assert_eq!(r.termination, Termination::Judged);
            for q in &r.requests {
                assert_eq!(
                    q.result,
                    RequestResult::Failed(Unresolved::DeadlineExceeded)
                );
                assert_eq!(q.attempts[0].end, AttemptEnd::Deadline);
                assert_eq!(q.attempts[0].cost.charge, Charge::Unknown);
            }
            assert_eq!(r.cost.final_cost, FinalCost::Unknown);
            assert!(r.timing.deadline_expired);
        }
        "runtime.cancel" => {
            use rustev_contract::run::{CancelAnswer, Cancellation, RemoteState};
            let r = record(out, "d1");
            assert_eq!(r.termination, Termination::Cancelled);
            for q in &r.requests {
                assert_eq!(
                    q.attempts[0].cancellation,
                    Cancellation::Requested {
                        answer: CancelAnswer::NotObserved
                    }
                );
                assert_eq!(q.attempts[0].remote, RemoteState::PossiblyContinuing);
            }
            assert_eq!(r.cost.liability, 15);
        }
        "runtime.cancel-stop" => {
            let r = record(out, "d1");
            assert_eq!(r.termination, Termination::Cancelled);
            for q in &r.requests {
                assert_eq!(q.result, RequestResult::NotSupplied);
                assert_eq!(q.attempts[0].end, AttemptEnd::Cancelled);
                assert_eq!(q.attempts[0].cost.charge, Charge::Observed { units: 2 });
            }
            assert_eq!((r.cost.observed, r.cost.liability), (6, 0));
        }
        other => panic!("no mirrored assertions for {other}"),
    }
}

#[test]
fn every_runtime_case_produces_its_expected_bytes() {
    let fx = fixtures();
    for (name, bytes) in &fx {
        keep(&format!("fixtures/runtime/{name}"), bytes);
    }
    for (id, mirrored, block) in cases() {
        let case: RuntimeCase = serde_json::from_value(block.clone()).unwrap();
        let mut names: Vec<&str> = vec![&case.plan];
        names.extend(case.descriptors.iter().map(String::as_str));
        names.extend(case.calibrations.iter().map(String::as_str));
        names.extend(case.requests.iter().map(|r| r.snapshot.as_str()));
        names.sort();
        names.dedup();
        let inputs: BTreeMap<String, Vec<u8>> = names
            .iter()
            .map(|n| (n.to_string(), fx[*n].clone()))
            .collect();
        let out = runtime::run(&case, &inputs).unwrap_or_else(|e| panic!("{id}: {e}"));
        mirrors(id, &out);
        let mut outputs = vec![];
        for (file, bytes) in &out {
            let rel = format!("expected/runtime/{id}/{file}");
            keep(&rel, bytes);
            outputs.push(json!({ "file": file, "golden": format!("{DIR}/{rel}"),
                                 "sha256": sha256_hex(bytes), "normalize": [] }));
        }
        let case_file = json!({
            "id": id,
            "command": "decide",
            // The native runner interprets the same data with TypeScript
            // scripted seams (3.12.4); the decision service's runner binds
            // the Rust scripted seams as service backends (spec 042, 3.17.1).
            "surfaces": if id == "runtime.cancel-stop" {
                // The runtime polls a cancelled attempt once for its answer
                // (spec 003, 3.7.3); a host seam answers across threads and
                // can never be ready then, so a host backend's confirmed stop
                // is recorded honestly as not observed (`runtime.cancel`).
                // The service's backends are Rust seams, polled in process.
                json!(["rust-api", "serve-grpc", "serve-http"])
            } else {
                json!(["rust-api", "native", "serve-grpc", "serve-http"])
            },
            "mirrors": mirrored,
            "inputs": names.iter().map(|n| json!({
                "path": format!("{DIR}/fixtures/runtime/{n}"), "name": n,
                "sha256": sha256_hex(&fx[*n]) })).collect::<Vec<_>>(),
            "runtime": block,
            "outputs": outputs,
        });
        keep(
            &format!("cases/runtime/{id}.json"),
            &line(serde_json::to_vec_pretty(&case_file).unwrap()),
        );
    }
}

#[test]
fn the_manifest_runtime_cases_pass_through_the_runner() {
    if bless() {
        return;
    }
    // The committed cases, read back as any runner reads them.
    let m = manifest().unwrap();
    let mut ran = 0;
    for c in m.cases.iter().filter(|c| c.runtime.is_some()) {
        let case: RuntimeCase = serde_json::from_value(c.runtime.clone().unwrap()).unwrap();
        let inputs: BTreeMap<String, Vec<u8>> = c
            .inputs
            .iter()
            .map(|i| (i.name.clone(), pinned(&i.path, &i.sha256).unwrap()))
            .collect();
        let out = runtime::run(&case, &inputs).unwrap();
        // Unique per case and run; kept, with its path printed, when the
        // case diverges, as the CLI runner keeps its outputs.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rustev-conformance-rt-{}-{}-{nanos}",
            std::process::id(),
            c.id
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (f, b) in &out {
            std::fs::write(dir.join(rustev_conformance::contained(f).unwrap()), b).unwrap();
        }
        let v = compare(c, 0, b"", &dir);
        assert!(v.is_empty(), "{v:#?}\noutputs kept under {}", dir.display());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            c.mirrors
                .as_deref()
                .is_some_and(|m| m.starts_with("crates/rustev-runtime/tests/"))
        );
        ran += 1;
    }
    assert!(ran >= 6, "{ran} runtime cases");
}
