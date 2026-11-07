//! Synthetic live cases, generated from Rust and re-blessed automatically.
#[path = "../../../crates/rustev-core/tests/common/mod.rs"]
mod builders;
use builders::*;
use rustev_conformance::runtime::{Inputs, Outputs};
use rustev_conformance::{live, manifest, pinned, root, sha256_hex};
use rustev_contract::{Document, Identified};
use rustev_eval::metrics::TaskAdapter;
use rustev_live::journal::{BudgetKind, SetClock, SpendJournal, TESTING_CAP_UNITS};
use serde_json::{Value, json};
use std::sync::Arc;

const DIR: &str = "tools/rustev-conformance";
fn bless() -> bool {
    std::env::var_os("RUSTEV_BLESS").is_some()
}
fn bytes(v: &Value) -> Vec<u8> {
    let mut b = serde_json::to_vec_pretty(v).unwrap();
    b.push(b'\n');
    b
}
fn keep(name: &str, bytes: &[u8]) {
    let path = root().join(DIR).join(name);
    if bless() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    } else {
        assert_eq!(
            std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            bytes,
            "{name} is stale"
        );
    }
}
fn fixtures() -> Inputs {
    let mut f = Inputs::new();
    let ds = vec![linear_head()];
    let cal = topic_calibration("1.5");
    let plan = rustev_core::compile(&support_routing(), &ds, std::slice::from_ref(&cal)).unwrap();
    f.insert("candidates/a/plan.json".into(), plan.canonical());
    for d in ds {
        f.insert(
            format!("candidates/a/descriptors/{}.json", d.backend_id),
            d.canonical().unwrap(),
        );
    }
    f.insert(
        "candidates/a/calibrations/topic.json".into(),
        cal.canonical().unwrap(),
    );
    let snapshot = support_snapshot("free", &[1], 0);
    let probe = support_snapshot("pro", &[2, 5], 0);
    f.insert("snapshots/x.json".into(), snapshot.canonical().unwrap());
    f.insert("probe.json".into(), probe.canonical().unwrap());
    let d = linear_head();
    f.insert("live.json".into(), bytes(&json!({
        "schema":"rustev.live-config/1", "budget":"testing", "run_cap_units":1000,
        "repeats":1,"evaluation_time":{"fixed":{"ms":NOW}},
        "bindings":[{"identity":{"backend_id":d.backend_id,"artifact":d.artifact},"cost_unit":"nano_usd",
            "max_in_flight":1,"min_interval_ms":0,"case_deadline_ms":2000,"rate_limit_pause_max_ms":0}],
        "acquisitions":{},"probe":{"calls":1,"interval_ms":0,"snapshot":"probe.json","digest":probe.id().unwrap(),"provenance":"synthetic"},
        "authorize":[{"backend_id":d.backend_id,"artifact":d.artifact,"operations":["classify","proposition","rubric","rank"],"max_exposure_units":1}]
    })));
    f.insert("dataset.json".into(), bytes(&json!({
        "schema":"rustev.dataset/1","name":"SYNTHETIC live conformance", "provenance":{"synthetic":{"generator":"conformance live fixtures"}},
        "adapter":{"name":"test","version":"1"},
        "cases": (["x"].map(|id| json!({"id":id,"source":format!("synthetic:{id}"),"snapshot":snapshot.id().unwrap(),"split":"training","label":"missing","subgroups":[]})))
    })));
    let mut capped: Value = serde_json::from_slice(&f["live.json"]).unwrap();
    capped["run_cap_units"] = json!(3);
    f.insert("live-run-cap.json".into(), bytes(&capped));
    let mut resume: Value = serde_json::from_slice(&f["dataset.json"]).unwrap();
    let mut second = resume["cases"][0].clone();
    second["id"] = json!("y");
    second["source"] = json!("synthetic:y");
    resume["cases"].as_array_mut().unwrap().push(second);
    f.insert("dataset-resume.json".into(), bytes(&resume));
    let adapter = json!({"schema":"rustev.task-adapter/1","name":"test","version":"1","labels":{"values":["yes"],"prefixes":[]},"rules":[{"action":"route_ticket","label":"yes"}]});
    f.insert("adapter.json".into(), bytes(&adapter));
    let adapter = rustev_surface::adapter::TaskAdapterDoc::parse(&bytes(&adapter)).unwrap();
    f.insert("config.json".into(), bytes(&json!({
        "schema":"rustev.evaluator-config/2","adapter":rustev_eval::config::ConfigAdapter::v2(adapter.adapter(), adapter.rules()),
        "label_interpretation":"SYNTHETIC","agreement":{"params":[]},"probability_step":null,
        "reliability_bins":["0","1"],"subgroups":[],"latency_quantiles":[],"cost_units_comparable":true,
        "metrics": formulas(),"gates":[],"temperature_grid":[]
    })));
    f.insert("repeat.json".into(), bytes(&json!({"schema":"rustev.repeat-config/1",
        "bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.95","resamples":1000,"seed":"1"},
        "noise":{"rule":"interval_width","floor":"0","overrides":{}},"slices":[],"stability_metrics":["coverage"],"top_k":null})));
    {
        // The testing cap is fixed. Prior durable liability leaves exactly
        // enough headroom for the probe, then refuses the first dataset send.
        let dir = live::Directory::new().unwrap();
        let j = SpendJournal::open(
            dir.0.join("journal"),
            BudgetKind::Testing,
            Arc::new(SetClock::new(NOW)),
        )
        .unwrap();
        j.reserve("prior/attempt", TESTING_CAP_UNITS - 3).unwrap();
        f.insert("journal-cap.jsonl".into(), std::fs::read(j.path()).unwrap());
    }
    f
}
// Disk fixtures are shared; only the selected variant is staged under each
// option's input label, so unrelated variants do not enter the input digests.
fn inputs_for(id: &str, shared: &Inputs) -> Vec<(String, String)> {
    let mut inputs: Vec<_> = shared
        .keys()
        .filter_map(|path| {
            let name = match path.as_str() {
                "live.json" if id == "live.run-cap" => return None,
                "live-run-cap.json" if id == "live.run-cap" => "live.json",
                "dataset.json" if id == "live.resume" => return None,
                "dataset-resume.json" if id == "live.resume" => "dataset.json",
                "journal-cap.jsonl" if id == "live.journal-cap" => "journal.jsonl",
                "live-run-cap.json" | "dataset-resume.json" | "journal-cap.jsonl" => return None,
                name => name,
            };
            Some((name.into(), path.clone()))
        })
        .collect();
    if id == "live.resume" {
        inputs.push(("snapshots/y.json".into(), "snapshots/x.json".into()));
    }
    inputs
}
fn formulas() -> Value {
    json!([
        {"name":"coverage","formula":"comparable cases / all cases"},
        {"name":"acceptance_coverage","formula":"proposals / comparable cases"},
        {"name":"labeled_proposal_coverage","formula":"labeled proposals / proposals"},
        {"name":"error_among_accepted","formula":"wrong labeled proposals / labeled proposals"},
        {"name":"candidate_outcome_change","formula":"changed outcomes / comparable cases"},
        {"name":"mean_log_loss","formula":"mean negative log probability of label"},
        {"name":"brier","formula":"mean sum of squared probability errors"},
        {"name":"reliability","formula":"per bin mean confidence and accuracy"},
        {"name":"latency_ms","formula":"quantile of elapsed minus queued"},
        {"name":"queue_ms","formula":"quantile of queued"},
        {"name":"cost_observed_units","formula":"sum of observed units"},
        {"name":"cost_estimated_units","formula":"sum of estimated units"},
        {"name":"cost_unknown_attempts","formula":"attempts with unknown charge"},
        {"name":"cost_liability_units","formula":"units kept as liability"}
    ])
}
fn answer(task: &str) -> Value {
    let output = match task {
        "support.topic" => {
            json!({"logits":{"billing":4.0,"integration_defect":0.0,"account_access":0.0,"other":0.0}})
        }
        "support.frustration" => json!({"logits":{"calm":3.0,"frustrated":0.0,"very_angry":0.0}}),
        _ => json!({"logits":{"false":3.0,"true":0.0}}),
    };
    json!({"output":{"output":output,"charge":{"observed":{"units":1}}}})
}
fn case(id: &str) -> Value {
    let mut answers = json!({});
    let mut release = json!({});
    for task in ["support.topic", "support.frustration", "support.deadline"] {
        answers[task] = json!([match id {
            "live.resume" => json!("gate"),
            "live.probe-failure" =>
                json!({"fail":{"class":"permanent","detail":"SYNTHETIC probe failure","charge":{"observed":{"units":0}}}}),
            _ => answer(task),
        }]);
        release[task] = answer(task);
    }
    let options = json!({"live":true,"liveConfig":"live.json","dataset":"dataset.json","config":"config.json",
        "adapter":"adapter.json","repeatConfig":"repeat.json","candidates":"candidates","snapshots":"snapshots",
        "journal":"journal.jsonl","out":"out","runId":"conformance","nowMs":NOW,"split":"training",
        "scope":{"mode":"tenant_only","tenant":"synthetic","context_revision":"1"},"probeOnly":id == "live.probe-only","resume":false,"rerunInterrupted":false});
    let release_steps = || {
        vec![
            json!({"wait_gated":{"backend":0,"count":1}}),
            json!({"release_all":{"backend":0}}),
        ]
    };
    let mut runs = vec![json!({"options":options,"drive":[]})];
    if id == "live.resume" {
        let mut drive = vec![];
        // Three probe attempts, then all three attempts of x. The next
        // gate is y, after x has been checkpointed as completed.
        for _ in 0..6 {
            drive.extend(release_steps());
        }
        drive.push(json!({"wait_gated":{"backend":0,"count":1}}));
        drive.push(json!("abort"));
        runs[0]["drive"] = json!(drive);
        let mut resumed = options;
        resumed["resume"] = json!(true);
        let drive: Vec<_> = (0..3).flat_map(|_| release_steps()).collect();
        runs.push(json!({"options":resumed,"drive":drive}));
    }
    json!({"backends":[{"descriptor":"candidates/a/descriptors/synthetic-linear-head.json",
        "max_concurrency":1,"cost_model":"bounded","cost_bound":{"bounded":{"max_units":1}},
        "remote":"finished","on_cancel":"ignore","answers":answers,"on_release":release}],"runs":runs})
}
const CASES: [(&str, &str); 6] = [
    (
        "live.full",
        "live_run_records_digests_counts_spend_and_deterministic_bytes",
    ),
    (
        "live.probe-only",
        "authorization_refusal_continues_and_probe_statuses_send_no_cases",
    ),
    (
        "live.probe-failure",
        "authorization_refusal_continues_and_probe_statuses_send_no_cases",
    ),
    ("live.run-cap", "live_run_spend_cap_and_exit_codes"),
    (
        "live.journal-cap",
        "spend_refusal_stops_admission_and_keeps_unsent_items_pending",
    ),
    (
        "live.resume",
        "run_deadline_cancels_in_flight_and_leaves_a_resumable_checkpoint",
    ),
];
fn mirrors(id: &str, out: &Outputs) {
    let doc: Value = serde_json::from_slice(&out["out/live-run.json"]).unwrap();
    let statuses: Value = serde_json::from_slice(&out["status.json"]).unwrap();
    let (status, code) = match id {
        "live.probe-only" => ("probed", 0),
        "live.probe-failure" => ("probe_failed", 15),
        "live.run-cap" | "live.journal-cap" => ("spend_capped", 14),
        _ => ("evaluated_live", 0),
    };
    assert_eq!(
        statuses.as_array().unwrap().last().unwrap(),
        &json!({"status":status,"code":code})
    );
    assert_eq!(doc["status"], status);
    assert!(out.contains_key("out/checkpoint.json"));
    let journal = String::from_utf8_lossy(&out["journal.jsonl"]);
    let reservations: Vec<Value> = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["op"] == "reserve")
        .collect();
    let sends = |item: &str| {
        reservations
            .iter()
            .filter(|r| {
                r["attempt"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("live/conformance/a/1/{item}/"))
            })
            .count()
    };
    match id {
        "live.full" => {
            assert_eq!(doc["counts"]["completed"], 1);
            assert_eq!((sends("x"), sends("y")), (3, 0));
            assert!(out.contains_key("out/a/repeat.json"));
        }
        "live.probe-only" | "live.probe-failure" => {
            assert_eq!(doc["counts"]["pending"], 1);
            assert_eq!((sends("x"), sends("y")), (0, 0));
            assert!(!out.keys().any(|p| p.starts_with("out/cases/")));
            assert!(!out.contains_key("out/a/repeat.json"));
        }
        "live.run-cap" | "live.journal-cap" => {
            assert_eq!(doc["counts"]["completed"], 0);
            assert_eq!((sends("x"), sends("y")), (0, 0));
            if id == "live.journal-cap" {
                assert_eq!(doc["journal_before"]["headroom"], 3);
                assert_eq!(doc["journal_after"]["headroom"], 0);
            }
            assert_eq!(doc["counts"]["pending"], 0);
            assert!(!out.contains_key("out/a/repeat.json"));
            assert_eq!(
                doc["stop_reason"],
                if id == "live.run-cap" {
                    "run-cap-exceeded"
                } else {
                    "spend-cap-exceeded"
                }
            );
        }
        "live.resume" => {
            assert_eq!(statuses[0], json!({"status":"interrupted","code":16}));
            assert_eq!(statuses.as_array().unwrap().len(), 2);
            // Completed x is not resent; y's sole unsettled reservation stays liability.
            assert_eq!((sends("x"), sends("y")), (3, 1));
            assert_eq!(doc["journal_before"]["liability"], 1);
            assert_eq!(doc["journal_after"]["liability"], 1);
            assert_eq!(doc["counts"]["completed"], 1);
            assert_eq!(doc["counts"]["interrupted"], 1);
            assert!(out.contains_key("out/cases/a.1.x.run.json"));
            assert!(
                String::from_utf8_lossy(&out["out/a/run-0.detail.json"])
                    .contains("live-interrupted")
            );
        }
        _ => unreachable!(),
    }
}
#[test]
fn live_cases() {
    let shared = fixtures();
    for (path, bytes) in &shared {
        keep(&format!("fixtures/live/{path}"), bytes);
    }
    for (id, mirrored) in CASES {
        let paths = inputs_for(id, &shared);
        let inputs = paths
            .iter()
            .map(|(name, path)| (name.clone(), shared[path].clone()))
            .collect();
        let block = case(id);
        let parsed: live::LiveCase = serde_json::from_value(block.clone()).unwrap();
        let out = live::run(&parsed, &inputs).unwrap_or_else(|e| panic!("{id}: {e}"));
        mirrors(id, &out);
        let outputs: Vec<_> = out.iter().map(|(file,b)| {
            let golden = format!("expected/live/{id}/{file}");
            keep(&golden,b);
            json!({"file":file,"golden":format!("{DIR}/{golden}"),"sha256":sha256_hex(b),"normalize":[]})
        }).collect();
        // `native` joins with the binding runner over `engine.liveRun` (part 2).
        let case = json!({"id":id,"command":"eval live","surfaces":["rust-api"],
            "mirrors":format!("crates/rustev-live/tests/run.rs::{mirrored}"),"live":block,
            "inputs":paths.iter().map(|(name,path)| json!({"path":format!("{DIR}/fixtures/live/{path}"),"name":name,"sha256":sha256_hex(&shared[path])})).collect::<Vec<_>>(),"outputs":outputs});
        keep(&format!("cases/live/{id}.json"), &bytes(&case));
    }
}
#[test]
fn the_manifest_live_cases_pass_through_the_runner() {
    if bless() {
        return;
    }
    let mut ran = 0;
    for case in manifest()
        .unwrap()
        .cases
        .into_iter()
        .filter(|c| c.live.is_some())
    {
        let parsed = serde_json::from_value(case.live.clone().unwrap()).unwrap();
        let inputs = case
            .inputs
            .iter()
            .map(|i| (i.name.clone(), pinned(&i.path, &i.sha256).unwrap()))
            .collect();
        let out = live::run(&parsed, &inputs).unwrap();
        mirrors(&case.id, &out);
        let expected: Outputs = case
            .outputs
            .iter()
            .map(|o| {
                assert!(o.normalize.is_empty());
                (
                    o.file.clone().unwrap(),
                    pinned(&o.golden, &o.sha256).unwrap(),
                )
            })
            .collect();
        assert_eq!(out, expected, "{}: exact file set and bytes", case.id);
        let (path, test) = case.mirrors.as_deref().unwrap().split_once("::").unwrap();
        assert!(
            std::fs::read_to_string(root().join(path))
                .unwrap()
                .contains(&format!("fn {test}("))
        );
        assert_eq!(case.surfaces, ["rust-api"]);
        ran += 1;
    }
    assert_eq!(ran, 6);
}
