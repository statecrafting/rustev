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
                .is_some_and(|m| m.starts_with("crates/rustev-runtime/tests/")
                    || m.starts_with("crates/rustev-eval/tests/"))
        );
        ran += 1;
    }
    assert!(ran >= 6, "{ran} runtime cases");
}

/// Only the new capture expectations are blessed by this test.
#[test]
fn capture_cases() {
    use rustev_contract::Identified;
    use rustev_contract::descriptor::{OutputKind, TextStructure};
    let mut fx = fixtures();
    let mut definition = support_routing();
    definition.schema = "rustev.definition/2".into();
    definition.inputs.clear();
    definition.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let template: rustev_contract::generation::PromptTemplate = serde_json::from_value(json!({"schema":"rustev.prompt-template/1","id":"item-text","version":"1.0.0","instructions":"Describe the item.","params":[]})).unwrap();
    let mut step: Json = serde_json::from_str(include_str!(
        "../../../crates/rustev-contract/tests/fixtures/generate-step.json"
    ))
    .unwrap();
    let body = &mut step["body"]["generate"];
    body["for_each"] = json!("none");
    body["template"]["params"] = json!({});
    body["template"]["digest"] = json!(template.id().unwrap());
    body["projection"] = json!([]);
    body["carry"] = json!([]);
    body["backstop"]["fields"] =
        json!({"headline":{"text":"An option"},"why":{"text":"A useful choice."}});
    definition.steps = vec![serde_json::from_value(step).unwrap()];
    let mut backend = linear_head();
    backend.operations = vec![support(
        rustev_contract::definition::Operation::Generate,
        OutputKind::TextRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 4096,
            languages: vec!["en-GB".into()],
            streaming: true,
        },
        32,
    )];
    let compiled = rustev_core::compile_with_templates(
        &definition,
        &[backend.clone()],
        &[],
        &[],
        None,
        &[template],
    )
    .unwrap();
    fx.insert(
        "capture-generation.plan.json".into(),
        line(compiled.canonical()),
    );
    fx.insert(
        "capture-generation.descriptor.json".into(),
        line(backend.canonical().unwrap()),
    );
    fx.insert(
        "capture-generation.snapshot.json".into(),
        line(snapshot(vec![]).canonical().unwrap()),
    );
    // Existing fixtures are read and checked, never rewritten by capture blessing.
    for (name, bytes) in &fx {
        if name.starts_with("capture-") {
            // Beside, not in, fixtures/runtime: rustev-contract's
            // ranking_wire test counts that directory's descriptors.
            keep(&format!("fixtures/{}", fixture(name)), bytes);
        } else {
            assert_eq!(
                std::fs::read(root().join(DIR).join("fixtures/runtime").join(name)).unwrap(),
                *bytes
            );
        }
    }
    for generation in [false, true] {
        let id = if generation {
            "runtime.capture-generation"
        } else {
            "runtime.capture-replay"
        };
        let mut block = json!({
            "plan": "support.plan.json", "descriptors": DESCRIPTORS,
            "calibrations": ["topic.calibration.json"], "config": config(2, 0, 1),
            "backends": [head(every(|t| json!([output(t, 1)])), json!({}), json!("ignore"), 1)],
            "requests": [request("embedded", None), request("external", None)],
            "drive": ["start", "start"],
            "capture": {"scope": {"mode":"principal","tenant":"tenant","context_revision":"revision","principal_scope":"scope"},
                "max_bytes":16777216, "created_at_ms":NOW,
                "retain":{"embedded":"embedded","external":"external"}}
        });
        if generation {
            block["plan"] = json!("capture-generation.plan.json");
            block["descriptors"] = json!(["capture-generation.descriptor.json"]);
            block["calibrations"] = json!([]);
            block["backends"][0]["descriptor"] = json!("capture-generation.descriptor.json");
            block["backends"][0]["answers"] = json!({"*": [{"output":{"output":{"text_record":{
                "headline":{"text":"Hello synthetic reader"},"why":{"text":"A useful choice"},"tags":{"list":["quiet"]}}},"charge":{"observed":{"units":1}}}}]});
            for r in block["requests"].as_array_mut().unwrap() {
                r["snapshot"] = json!("capture-generation.snapshot.json");
            }
        }
        let case: RuntimeCase = serde_json::from_value(block.clone()).unwrap();
        let mut names = vec![case.plan.clone()];
        names.extend(case.descriptors.clone());
        names.extend(case.calibrations.clone());
        names.extend(case.requests.iter().map(|r| r.snapshot.clone()));
        names.sort();
        names.dedup();
        let inputs = names.iter().map(|n| (n.clone(), fx[n].clone())).collect();
        let out = runtime::run(&case, &inputs).unwrap();
        let mut outputs = vec![];
        for (file, bytes) in out {
            if generation && file.ends_with("record.json") {
                let r: Json = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    r["core"]["recorded"]["generation"][0]["counts"]["generated"],
                    3
                );
                assert_eq!(r["requests"][0]["attempts"][0]["end"], "output");
            }
            if file.ends_with("bundle.json") {
                let b: Json = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(b["capture"], "complete");
                assert_eq!(
                    b["supplies"].as_array().unwrap().len(),
                    if generation { 1 } else { 3 }
                );
            }
            if file.ends_with("replay.json") {
                assert_eq!(
                    serde_json::from_slice::<Json>(&bytes).unwrap()["status"],
                    "reproduced"
                );
            }
            let rel = format!("expected/runtime/{id}/{file}");
            keep(&rel, &bytes);
            outputs.push(json!({"file":file,"golden":format!("{DIR}/{rel}"),"sha256":sha256_hex(&bytes),"normalize":[]}));
        }
        let c = json!({"id":id,"command":"decide","surfaces":["rust-api","native"],
            "mirrors":if generation {"crates/rustev-eval/tests/generation_replay.rs"} else {"crates/rustev-eval/tests/acquisition_replay.rs::five_acquisitions_two_paged_reproduce_without_source_calls"},
            "inputs":names.iter().map(|n| json!({"name":n,"path":format!("{DIR}/fixtures/{}", fixture(n)),"sha256":sha256_hex(&fx[n])})).collect::<Vec<_>>(),
            "runtime":block,"outputs":outputs});
        keep(
            &format!("cases/runtime/{id}.json"),
            &line(serde_json::to_vec_pretty(&c).unwrap()),
        );
    }
}

/// A capture fixture's path under `fixtures/`.
fn fixture(name: &str) -> String {
    if name.starts_with("capture-") {
        format!("capture/{name}")
    } else {
        format!("runtime/{name}")
    }
}

// ---- spec 063 3.13.2: acquisition source cases --------------------------

/// The SYNTHETIC source of `crates/rustev-runtime/tests/acquisition.rs`,
/// named `name` with its item field `field`.
fn acquire_source(name: &str, field: &str) -> rustev_contract::source::SourceDescriptor {
    serde_json::from_value(
        json!({"schema":"rustev.source/1","name":name,"version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges",
        "request":[{"name":"q","type":{"text":{"max_bytes":512}},"required":true},
                   {"name":"optional","type":"integer","required":false}],"max_request_bytes":2048,
        "item":[{"name":field,"type":"integer","provenance":"system-of-record",
        "max_grade":"cached","reliability":"reliable","freshness":"not_required"}],
        "max_page_items":4,"max_page_bytes":1024,"paging":"offset",
        "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]}),
    )
    .unwrap()
}

/// The mirrored test's definition: one `acquire` step `lookup` with a 100
/// ms deadline and a 0.2 window share.
fn acquire_definition(
    max_attempts: u32,
    retry_on: &[&str],
) -> rustev_contract::definition::Definition {
    let mut d = support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.limits.deadline_ms = 100;
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]})).unwrap();
    d.steps = vec![serde_json::from_value(json!({"id":"lookup","body":{"acquire":{
        "source":"catalog","operation":"search","request":{"q":{"lit":{"text":"tea"}}},"when":"always",
        "limits":{"max_pages":2,"max_items":5,"max_attempts":max_attempts,"retry_on":retry_on,"delay":"none","deadline_share":"0.2","as_of_skew_ms":100},
        "stop":"none","on_failure":"unresolved"}}})).unwrap()];
    d.limits.acquisition = Some(
        serde_json::from_value(json!({"max_calls":8,"max_items":20,"max_bytes":16384})).unwrap(),
    );
    d
}

/// The mirrored widening definition, with the second tier on `other`.
fn widen_definition() -> rustev_contract::definition::Definition {
    let mut d = acquire_definition(1, &[]);
    d.steps = vec![serde_json::from_value(json!({"id":"pool","body":{"widen":{
        "item":{"fields":[{"name":"id","type":"integer"}]},
        "sources":[{"source":"catalog","operation":"search","map":{"id":"id"}},
                   {"source":"other","operation":"search","map":{"id":"other_id"}}],
        "base_request":{"q":{"lit":{"text":"protected"}},"optional":{"lit":{"integer":7}}},
        "groups":{"scope":["q"],"preferences":["optional"]},"protected":["scope"],
        "tiers":[{"name":"specific","source_index":0,"request_patch":{},"when":"always","relax":[]},
                 {"name":"broad","source_index":1,"request_patch":{},"when":"always","relax":["preferences"]}],
        "max_tiers":2,"dedupe_key":"id","order":"tier_then_source_rank",
        "stop":{"unique_at_least":{"lit":{"integer":3}}},"on_tier_failure":"continue",
        "limits":{"max_pages":2,"max_items":5,"max_attempts":1,"retry_on":[],"delay":"none","deadline_share":"0.2","as_of_skew_ms":100}
    }}})).unwrap()];
    d
}

fn acquire_fixtures() -> BTreeMap<String, Vec<u8>> {
    let catalog = acquire_source("catalog", "id");
    let other = acquire_source("other", "other_id");
    let plan = |d: &rustev_contract::definition::Definition,
                s: &[rustev_contract::source::SourceDescriptor]| {
        line(
            rustev_core::compile_with_sources(d, &[], &[], s, None)
                .unwrap()
                .canonical(),
        )
    };
    let one = std::slice::from_ref(&catalog);
    let mut window = acquire_definition(1, &[]);
    let mut second = window.steps[0].clone();
    second.id = "other".into();
    window.steps.push(second);
    BTreeMap::from([
        (
            "catalog.source.json".into(),
            line(catalog.canonical().unwrap()),
        ),
        ("other.source.json".into(), line(other.canonical().unwrap())),
        (
            "acquire-retry.plan.json".into(),
            plan(&acquire_definition(2, &["transient", "overloaded"]), one),
        ),
        (
            "acquire-denied.plan.json".into(),
            plan(&acquire_definition(2, &["transient"]), one),
        ),
        ("acquire-window.plan.json".into(), plan(&window, one)),
        (
            "acquire-widen.plan.json".into(),
            plan(&widen_definition(), &[catalog.clone(), other.clone()]),
        ),
        (
            "empty.snapshot.json".into(),
            line(snapshot(vec![]).canonical().unwrap()),
        ),
    ])
}

fn page(ids: &[i64], field: &str, next: Json) -> Json {
    json!({"page":{"items":ids.iter().map(|i| json!({field: i})).collect::<Vec<_>>(),
        "next":next,"as_of_ms":900,"charge":{"observed":{"units":1}}}})
}

fn fail(class: &str) -> Json {
    json!({"fail":{"class":class,"detail":"scripted","charge":{"observed":{"units":1}}}})
}

fn source_script(descriptor: &str, answers: Json) -> Json {
    json!({"descriptor": descriptor, "max_concurrency": 1, "answers": answers})
}

/// The acquisition cases: `(id, mirrored test, runtime block)`.
fn acquire_cases() -> Vec<(&'static str, &'static str, Json)> {
    const RETRY: &str = "crates/rustev-runtime/tests/acquisition.rs::retries_only_named_classes_denied_never_retried_and_panics_contained";
    let base = |plan: &str, parallel: usize, sources: Json, ids: &[&str], drive: Json| {
        let requests: Vec<Json> = ids
            .iter()
            .map(|id| {
                json!({"decision_id": id,
            "snapshot": "empty.snapshot.json", "evaluation_time_ms": 1000})
            })
            .collect();
        json!({"plan": plan, "descriptors": [], "calibrations": [],
               "config": config(1, ids.len(), parallel), "backends": [],
               "sources": sources, "requests": requests, "drive": drive})
    };
    let starts = |n: usize| json!(vec!["start"; n]);
    vec![
        (
            "runtime.acquire-page",
            RETRY,
            base(
                "acquire-retry.plan.json",
                2,
                json!([source_script(
                    "catalog.source.json",
                    json!({
                        // A named transient is retried; the page then pages on.
                        "transient": [fail("transient"), page(&[1], "id", json!({"offset":{"integer":1}})), page(&[2], "id", json!("none"))],
                        // A named overloaded is retried.
                        "overloaded": [fail("overloaded"), page(&[1], "id", json!("none"))],
                    })
                )]),
                &["transient", "overloaded"],
                starts(2),
            ),
        ),
        (
            "runtime.acquire-retry-denied",
            RETRY,
            base(
                "acquire-denied.plan.json",
                2,
                json!([source_script(
                    "catalog.source.json",
                    json!({
                        "denied": [fail("denied"), page(&[1], "id", json!("none"))],
                        "unnamed": [fail("overloaded"), page(&[1], "id", json!("none"))],
                        "invalid": [{"invalid":{"charge":{"observed":{"units":1}}}}, page(&[1], "id", json!("none"))],
                    })
                )]),
                &["denied", "unnamed", "invalid"],
                starts(3),
            ),
        ),
        (
            "runtime.acquire-window",
            "crates/rustev-runtime/tests/acquisition.rs::window_expiry_cancels_and_queued_pages_do_not_dispatch",
            base(
                "acquire-window.plan.json",
                1,
                json!([source_script(
                    "catalog.source.json",
                    json!({
                        "expiry": ["ignore"],
                    })
                )]),
                &["expiry"],
                // The window's timer (0.2 of 100 ms), armed at dispatch; the
                // decision's own deadline timer is 100 ms and never matches.
                json!(["start", {"advance_timer": {"delay_ms": 20}}]),
            ),
        ),
        (
            "runtime.acquire-widen",
            "crates/rustev-runtime/tests/acquisition.rs::widening_switches_sources_keeps_protected_fields_and_dedupes",
            base(
                "acquire-widen.plan.json",
                2,
                json!([
                    source_script(
                        "catalog.source.json",
                        json!({"widen": [page(&[1, 2], "id", json!("none"))]})
                    ),
                    source_script(
                        "other.source.json",
                        json!({"widen": [page(&[1, 3], "other_id", json!("none"))]})
                    ),
                ]),
                &["widen"],
                starts(1),
            ),
        ),
    ]
}

/// What each mirrored test asserts, on an acquisition case's outputs.
fn acquire_mirrors(id: &str, out: &BTreeMap<String, Vec<u8>>) {
    use rustev_contract::acquisition_record::{AcquisitionAttemptEnd as End, AcquisitionResult};
    use rustev_contract::judgment::Unresolved;
    use rustev_contract::run::CoreEvidence;
    let records = |d: &str| record(out, d).acquisitions.clone().unwrap();
    let ends = |d: &str| -> Vec<Vec<End>> {
        records(d)
            .iter()
            .map(|r| r.attempts.iter().map(|a| a.end.clone()).collect())
            .collect()
    };
    for f in out.keys().filter(|f| f.ends_with(".result.json")) {
        assert_eq!(
            status(out, f.trim_end_matches(".result.json"))["status"],
            "judged",
            "{id} {f}"
        );
    }
    match id {
        "runtime.acquire-page" => {
            assert_eq!(
                ends("transient"),
                [vec![End::Transient, End::Page], vec![End::Page]]
            );
            let r = records("transient");
            assert_eq!((r[1].tier, r[1].page), (0, 1));
            assert_eq!(r[1].attempts[0].attempt_id, "transient/lookup/0,1/1");
            assert!(r[0].attempts[1].attempt_id.ends_with("/lookup/0,0/2"));
            assert_eq!(ends("overloaded"), [vec![End::Overloaded, End::Page]]);
        }
        "runtime.acquire-retry-denied" => {
            assert_eq!(ends("denied"), [vec![End::Denied]]);
            assert_eq!(
                records("denied")[0].result,
                AcquisitionResult::Failed(Unresolved::missing(["lookup".into()]))
            );
            assert_eq!(ends("unnamed"), [vec![End::Overloaded]]);
            assert_eq!(ends("invalid"), [vec![End::InvalidPage]]);
        }
        "runtime.acquire-window" => {
            let r = records("expiry");
            for a in &r {
                assert_eq!(
                    a.result,
                    AcquisitionResult::Failed(Unresolved::DeadlineExceeded)
                );
            }
            assert_eq!(r[0].attempts.len(), 1);
            assert_eq!(r[0].attempts[0].end, End::Deadline);
            assert!(r[1].attempts.is_empty(), "a queued page never dispatches");
        }
        "runtime.acquire-widen" => {
            let rec = record(out, "widen");
            assert_eq!(
                rec.acquisitions
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|r| r.tier)
                    .collect::<Vec<_>>(),
                [0, 1]
            );
            let CoreEvidence::Recorded(e) = &rec.core else {
                panic!("no evidence")
            };
            let a = &e.acquisitions.as_ref().unwrap()[0];
            assert_eq!(a.items.len(), 3);
            assert_eq!(a.tiers[1].duplicates, 1);
            assert_eq!(a.items[2].origin.as_ref().unwrap().source, "other");
        }
        other => panic!("no mirrored assertions for {other}"),
    }
}

#[test]
fn acquire_cases_produce_their_expected_bytes() {
    let fx = acquire_fixtures();
    for (name, bytes) in &fx {
        keep(&format!("fixtures/acquire/{name}"), bytes);
    }
    for (id, mirrored, block) in acquire_cases() {
        let case: RuntimeCase = serde_json::from_value(block.clone()).unwrap();
        let mut names: Vec<&str> = vec![&case.plan];
        names.extend(case.sources.iter().map(|s| s.descriptor.as_str()));
        names.extend(case.requests.iter().map(|r| r.snapshot.as_str()));
        names.sort();
        names.dedup();
        let inputs: BTreeMap<String, Vec<u8>> = names
            .iter()
            .map(|n| (n.to_string(), fx[*n].clone()))
            .collect();
        let out = runtime::run(&case, &inputs).unwrap_or_else(|e| panic!("{id}: {e}"));
        acquire_mirrors(id, &out);
        let mut outputs = vec![];
        for (file, bytes) in &out {
            let rel = format!("expected/runtime/{id}/{file}");
            keep(&rel, bytes);
            outputs.push(json!({ "file": file, "golden": format!("{DIR}/{rel}"),
                                 "sha256": sha256_hex(bytes), "normalize": [] }));
        }
        let case_file = json!({
            "id": id, "command": "decide", "surfaces": ["rust-api", "native"],
            "mirrors": mirrored,
            "inputs": names.iter().map(|n| json!({
                "path": format!("{DIR}/fixtures/acquire/{n}"), "name": n,
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

// ---- spec 063 3.13.2: store cases ----------------------------------------

/// The SYNTHETIC contract of `crates/rustev-runtime/tests/persistent.rs`.
fn store_contract() -> rustev_contract::optimization::StoreContract {
    let term = |t: &str| format!("SYNTHETIC {t} term");
    rustev_contract::optimization::StoreContract {
        schema: term("schema"),
        namespace: term("namespace"),
        durability: term("durability"),
        encryption: term("encryption"),
        atomicity: term("atomicity"),
        conflict: "first writer wins".into(),
        expiry: term("expiry"),
        invalidation: term("invalidation"),
        erasure: term("erasure"),
    }
}

/// The mirrored test's `policy_with`: a persistent tier only.
fn store_policy(
    on_store_failure: rustev_contract::optimization::StoreFailurePolicy,
    admission: rustev_contract::optimization::OptimizationAdmission,
) -> rustev_contract::execution::ExecutionPolicy {
    use rustev_contract::optimization::*;
    let mut p = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    p.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: None,
        shared_calls: None,
        persistent_cache: Some(PersistentCachePolicy {
            store_contract: store_contract(),
            max_entry_bytes: 16_384,
            max_scope_bytes: 1_000_000,
            max_namespace_bytes: 10_000_000,
            ttl_ms: 60_000,
            on_store_failure,
        }),
        decision_cache: None,
    });
    p
}

fn store_fixtures() -> BTreeMap<String, Vec<u8>> {
    use rustev_contract::optimization::{OptimizationAdmission as A, StoreFailurePolicy as P};
    let ds = descriptors_with_fallbacks();
    let cal = vec![topic_calibration("1.5")];
    let plan = |p| {
        line(
            rustev_core::compile_with(&support_routing(), &ds, &cal, &p)
                .unwrap()
                .canonical(),
        )
    };
    BTreeMap::from([
        (
            "store.plan.json".into(),
            plan(store_policy(P::ContinueWithoutCache, A::Refuse)),
        ),
        (
            "store-closed.plan.json".into(),
            plan(store_policy(P::FailClosed, A::Refuse)),
        ),
        (
            "store-closed-independent.plan.json".into(),
            plan(store_policy(P::FailClosed, A::ExecuteIndependent)),
        ),
    ])
}

/// The store cases: `(id, mirrored test, runtime block)`.
fn store_cases() -> Vec<(&'static str, &'static str, Json)> {
    let answers = every(|t| json!([output(t, 1)]));
    let base = |requests: Vec<(&str, i64)>, store: Json| {
        let n = requests.len();
        let requests: Vec<Json> = requests
            .into_iter()
            .map(|(id, at)| json!({"decision_id": id, "snapshot": "support.snapshot.json", "evaluation_time_ms": at}))
            .collect();
        json!({ "plan": "store.plan.json", "descriptors": DESCRIPTORS,
                "calibrations": ["topic.calibration.json"], "config": config(1, 0, 3),
                "backends": [head(answers.clone(), json!({}), json!("ignore"), 1)],
                "store": store, "requests": requests, "drive": vec!["start"; n] })
    };
    let contract = serde_json::to_value(store_contract()).unwrap();
    const P: &str = "crates/rustev-runtime/tests/persistent.rs::";
    vec![
        (
            "runtime.store-restart-hit",
            "crates/rustev-runtime/tests/persistent.rs::a_restarted_runtime_hits_the_same_store",
            base(
                vec![("cold", NOW), ("warm", NOW + 1_000), ("after", NOW + 2_000)],
                // Another process's invalidation between two runtimes: the
                // generation advances and the entries stay (G2).
                json!({"contract": contract, "restart_before": ["warm", "after"],
                       "invalidate_before": ["after"], "remove_on_invalidate": false}),
            ),
        ),
        (
            "runtime.store-failure-policy",
            "crates/rustev-runtime/tests/persistent.rs::store_failures_follow_the_failure_policy",
            base(
                vec![
                    ("closed", NOW),
                    ("independent", NOW),
                    ("open", NOW),
                    ("write", NOW),
                ],
                json!({"contract": contract,
                       "plan_for": {"closed": "store-closed.plan.json",
                                    "independent": "store-closed-independent.plan.json",
                                    "write": "store-closed.plan.json"},
                       "failures": {"closed": {"fail_get": true}, "independent": {"fail_get": true},
                                    "open": {"fail_get": true}, "write": {"fail_put": true}}}),
            ),
        ),
        (
            "runtime.store-invalidation",
            "crates/rustev-runtime/tests/persistent.rs::invalidation_reaches_every_tier",
            base(
                vec![
                    ("cold", NOW),
                    ("unavailable", NOW),
                    ("after", NOW),
                    ("warm", NOW),
                ],
                json!({"contract": contract, "invalidate_before": ["unavailable", "after"],
                       "failures": {"unavailable": {"fail_invalidate": true}}}),
            ),
        ),
    ]
    .into_iter()
    .map(|(id, m, b)| {
        debug_assert!(m.starts_with(P));
        (id, m, b)
    })
    .collect()
}

/// What each mirrored test asserts, on a store case's outputs.
fn store_mirrors(id: &str, out: &BTreeMap<String, Vec<u8>>) {
    use rustev_contract::judgment::Unresolved;
    use rustev_contract::optimization::{OptimizationMechanism as M, OptimizationOutcome as O};
    use rustev_contract::run::RequestResult;
    let every_is = |d: &str, o: O| {
        let r = record(out, d);
        assert_eq!(r.requests.len(), 3, "{id} {d}");
        r.requests.iter().all(|q| {
            let x = q.optimization.as_ref().unwrap();
            x.mechanism == M::PersistentCache && x.outcome == o
        })
    };
    let diagnosed = |d: &str, needle: &str| {
        record(out, d).requests.iter().all(|q| {
            q.optimization
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .any(|x| x.contains(needle))
        })
    };
    let dispatched = |d: &str| {
        record(out, d)
            .requests
            .iter()
            .filter(|q| !q.attempts.is_empty())
            .count()
    };
    let invalidation = |d: &str| -> Json {
        serde_json::from_slice(&out[&format!("{d}.invalidation.json")]).unwrap()
    };
    match id {
        "runtime.store-restart-hit" => {
            assert!(every_is("cold", O::Miss));
            assert_eq!(dispatched("cold"), 3);
            assert!(every_is("warm", O::Hit));
            assert_eq!(dispatched("warm"), 0);
            let (cold, warm) = (record(out, "cold"), record(out, "warm"));
            for (hit, source) in warm.requests.iter().zip(&cold.requests) {
                let o = hit.optimization.as_ref().unwrap();
                assert_eq!(hit.result, source.result);
                assert_eq!(o.age_ms, 1_000);
                assert_eq!(
                    o.source_attempt_id.as_deref(),
                    Some(source.attempts[0].attempt_id.as_str())
                );
            }
            // The same proposal: only the decision and evaluation time differ.
            let judgment = |r: &RunRecord| match &r.core {
                rustev_contract::run::CoreEvidence::Recorded(e) => (
                    e.judgment.outcome.clone(),
                    e.judgment.derivation,
                    e.judgment.lineage.clone(),
                ),
                _ => panic!("no evidence"),
            };
            assert_eq!(judgment(&warm), judgment(&cold));
            assert!(every_is("after", O::Miss));
            assert!(diagnosed("after", "generation_advanced"));
            assert_eq!(dispatched("after"), 3);
            let inv = invalidation("after");
            assert_eq!(inv["persistent"], json!({"generation": 1, "removed": 0}));
            assert_eq!(inv["complete"], json!(true));
        }
        "runtime.store-failure-policy" => {
            assert!(every_is("closed", O::Refused));
            assert!(diagnosed("closed", "store_failed"));
            assert!(record(out, "closed").requests.iter().all(|q| matches!(&q.result,
                RequestResult::Failed(Unresolved::BackendUnavailable { detail }) if detail.starts_with("store_failed"))));
            assert_eq!(dispatched("closed"), 0);
            for d in ["independent", "open"] {
                assert!(every_is(d, O::Miss), "{d}");
                assert!(diagnosed(d, "store_failed"), "{d}");
            }
            assert_eq!(status(out, "write")["status"], "judged");
            assert!(every_is("write", O::Miss));
            assert!(diagnosed("write", "store_failed: SYNTHETIC put failure"));
        }
        "runtime.store-invalidation" => {
            assert!(every_is("cold", O::Miss));
            let failed = invalidation("unavailable");
            assert_eq!(
                (&failed["persistent"], &failed["complete"]),
                (&json!("pending"), &json!(false))
            );
            assert!(every_is("unavailable", O::Miss));
            assert!(diagnosed(
                "unavailable",
                "persistent tier unavailable until invalidation completes"
            ));
            let done = invalidation("after");
            assert_eq!(done["persistent"], json!({"generation": 1, "removed": 3}));
            assert_eq!(done["complete"], json!(true));
            assert!(every_is("after", O::Miss));
            assert_eq!(dispatched("after"), 3);
            assert!(every_is("warm", O::Hit));
        }
        other => panic!("no mirrored assertions for {other}"),
    }
}

#[test]
fn store_cases_produce_their_expected_bytes() {
    let mut fx = store_fixtures();
    for (name, bytes) in &fx {
        keep(&format!("fixtures/store/{name}"), bytes);
    }
    let shared = fixtures();
    for (id, mirrored, block) in store_cases() {
        let case: RuntimeCase = serde_json::from_value(block.clone()).unwrap();
        let mut names: Vec<&str> = vec![&case.plan];
        let store = case.store.as_ref().unwrap();
        names.extend(store.plan_for.values().map(String::as_str));
        names.extend(case.descriptors.iter().map(String::as_str));
        names.extend(case.calibrations.iter().map(String::as_str));
        names.extend(case.requests.iter().map(|r| r.snapshot.as_str()));
        names.sort();
        names.dedup();
        for n in &names {
            if !fx.contains_key(*n) {
                fx.insert(n.to_string(), shared[*n].clone());
            }
        }
        let inputs: BTreeMap<String, Vec<u8>> = names
            .iter()
            .map(|n| (n.to_string(), fx[*n].clone()))
            .collect();
        let out = runtime::run(&case, &inputs).unwrap_or_else(|e| panic!("{id}: {e}"));
        store_mirrors(id, &out);
        let mut outputs = vec![];
        for (file, bytes) in &out {
            let rel = format!("expected/runtime/{id}/{file}");
            keep(&rel, bytes);
            outputs.push(json!({ "file": file, "golden": format!("{DIR}/{rel}"),
                                 "sha256": sha256_hex(bytes), "normalize": [] }));
        }
        let dir = |n: &str| {
            if n.starts_with("store") {
                "store"
            } else {
                "runtime"
            }
        };
        let case_file = json!({
            "id": id, "command": "decide", "surfaces": ["rust-api", "native"],
            "mirrors": mirrored,
            "inputs": names.iter().map(|n| json!({
                "path": format!("{DIR}/fixtures/{}/{n}", dir(n)), "name": n,
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
