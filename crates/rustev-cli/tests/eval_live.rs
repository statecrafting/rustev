//! SYNTHETIC fixtures only: live rules execution without network access.
mod common;
use common::Scratch;
use rustev_cli::{Context, Exit, execute};
use rustev_contract::{Document, Identified};
use rustev_live::journal::{BudgetKind, SpendJournal, SystemClock};
use serde_json::{Value, json};
use std::sync::Arc;

struct Fixture {
    s: Scratch,
    args: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        let s = Scratch::new("eval-live");
        let (definition, calibration) = common::support_definition();
        s.write("definition.json", &definition.canonical().unwrap());
        s.write(
            "candidates/a/rules/support.json",
            &common::rules_fixture("support-routing.rules.json"),
        );
        s.write(
            "candidates/a/calibrations/topic.json",
            &calibration.canonical().unwrap(),
        );
        let path = |name: &str| s.path(name).to_str().unwrap().to_owned();
        let compiled = execute(
            &[
                "plan".into(),
                "compile".into(),
                "--definition".into(),
                path("definition.json"),
                "--rules".into(),
                path("candidates/a/rules/support.json"),
                "--calibration".into(),
                path("candidates/a/calibrations/topic.json"),
                "--out".into(),
                path("candidates/a/plan.json"),
            ],
            &Context::default(),
        );
        assert_eq!(compiled.code, 0, "{compiled:?}");
        let snapshot = common::support_snapshot_default();
        let probe = common::support_snapshot("basic", &[1], 0);
        for case in ["one", "two"] {
            s.write(
                &format!("snapshots/{case}.json"),
                &snapshot.canonical().unwrap(),
            );
        }
        s.write("probe.json", &probe.canonical().unwrap());
        let backend = common::support_rules();
        let identity =
            json!({"backend_id":"synthetic-rules-support", "artifact":backend.artifact()});
        let config = json!({"schema":"rustev.live-config/1", "budget":"testing", "run_cap_units":1,
            "repeats":1, "evaluation_time":{"fixed":{"ms":common::NOW}},
            "bindings":[{"identity":identity,"cost_unit":"none","max_in_flight":1,"min_interval_ms":0,
                "case_deadline_ms":10000,"rate_limit_pause_max_ms":0}], "acquisitions":{},
            "probe":{"calls":1,"interval_ms":0,"snapshot":path("probe.json"),"digest":probe.id().unwrap(),"provenance":"synthetic"},
            "authorize":[{"backend_id":"synthetic-rules-support","artifact":backend.artifact(),
                "operations":["classify","proposition","rubric","rank"],"max_exposure_units":100000}]});
        s.write("live.json", &serde_json::to_vec(&config).unwrap());
        let dataset = json!({"schema":"rustev.dataset/1","name":"SYNTHETIC live","provenance":{"synthetic":{"generator":"SYNTHETIC tests"}},
            "adapter":{"name":"test","version":"1"}, "cases":[
                {"id":"one","source":"one","snapshot":snapshot.id().unwrap(),"split":"training","label":"missing","subgroups":[]},
                {"id":"two","source":"two","snapshot":snapshot.id().unwrap(),"split":"training","label":"missing","subgroups":[]}]});
        s.write("dataset.json", &serde_json::to_vec(&dataset).unwrap());
        s.write("adapter.json", br#"{"schema":"rustev.task-adapter/1","name":"test","version":"1","labels":{"values":["yes"],"prefixes":[]},"rules":[{"action":"route_ticket","label":"yes"}]}"#);
        s.write("config.json", br#"{"schema":"rustev.evaluator-config/1","adapter":{"name":"test","version":"1"},"label_interpretation":"SYNTHETIC","agreement":{"params":[]},"probability_step":null,"reliability_bins":["0","1"],"subgroups":[],"latency_quantiles":[],"cost_units_comparable":false,"metrics":[],"gates":[],"temperature_grid":[]}"#);
        s.write("repeat.json", br#"{"schema":"rustev.repeat-config/1","bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.95","resamples":1000,"seed":"1"},"noise":{"rule":"interval_width","floor":"0","overrides":{}},"slices":[],"stability_metrics":["outcome_agreement"],"top_k":null}"#);
        let mut args: Vec<String> = [
            "eval",
            "live",
            "--live",
            "--run-id",
            "synthetic",
            "--split",
            "training",
            "--now-ms",
            "1790164800000",
            "--scope",
            "tenant-only",
            "--tenant",
            "synthetic",
            "--context-revision",
            "1",
        ]
        .map(String::from)
        .to_vec();
        for (flag, file) in [
            ("live-config", "live.json"),
            ("journal", "journal.jsonl"),
            ("dataset", "dataset.json"),
            ("config", "config.json"),
            ("adapter", "adapter.json"),
            ("repeat-config", "repeat.json"),
            ("candidates", "candidates"),
            ("snapshots", "snapshots"),
            ("out", "out"),
        ] {
            args.extend([format!("--{flag}"), path(file)]);
        }
        Self { s, args }
    }
    fn run(&self) -> Exit {
        execute(&self.args, &Context::default())
    }
    fn json(&self, file: &str) -> Value {
        serde_json::from_slice(&self.s.read(file)).unwrap()
    }
    fn change(&self, file: &str, edit: impl FnOnce(&mut Value)) {
        let mut doc = self.json(file);
        edit(&mut doc);
        self.s.write(file, &serde_json::to_vec(&doc).unwrap());
    }
    fn pending(&self) {
        let summary = self.json("out/live-run.json");
        assert_eq!(summary["counts"]["pending"], 2);
        assert_eq!(summary["counts"]["completed"], 0);
        assert!(summary["cases"].as_array().unwrap().is_empty());
        for item in self.json("out/checkpoint.json")["items"]
            .as_array()
            .unwrap()
        {
            assert_eq!(item["states"], json!(["pending"]));
        }
        let journal = String::from_utf8(self.s.read("journal.jsonl")).unwrap();
        assert!(!journal.contains("live/synthetic/a/"), "{journal}");
    }
}
fn expect(result: &Exit, code: u8, text: &str) {
    assert_eq!(result.code, code, "{result:?}");
    assert!(
        String::from_utf8_lossy(&result.stdout).contains(text),
        "{result:?}"
    );
}
#[test]
fn usage_precedes_reads_and_output_creation() {
    let f = Fixture::new();
    for flag in [
        "live",
        "live-config",
        "journal",
        "out",
        "run-id",
        "dataset",
        "split",
        "config",
        "adapter",
        "candidates",
        "repeat-config",
    ] {
        let mut args = f.args.clone();
        let i = args.iter().position(|s| s == &format!("--{flag}")).unwrap();
        args.remove(i);
        if flag != "live" {
            args.remove(i);
        }
        assert_eq!(execute(&args, &Context::default()).code, 2, "{flag}");
    }
    for flags in [
        vec!["--repeat", "2"],
        vec!["--captures", "missing"],
        vec!["--bundles", "missing"],
    ] {
        let mut args = f.args.clone();
        args.extend(flags.into_iter().map(String::from));
        assert_eq!(execute(&args, &Context::default()).code, 2);
    }
    assert!(!f.s.exists("journal.jsonl"));
    assert!(!f.s.exists("out"));
}
#[test]
fn probe_only_has_no_dataset_dispatches() {
    let mut f = Fixture::new();
    f.args.push("--probe-only".into());
    expect(&f.run(), 0, "probed synthetic run_spend=0");
    assert!(f.json("out/live-run.json")["probe"].is_object());
    f.pending();
}
#[test]
fn two_cases_publish_records_and_one_summary_line_with_zero_spend() {
    let f = Fixture::new();
    let result = f.run();
    expect(&result, 0, "evaluated_live synthetic");
    // Rules cost units measure work, not money: cost_unit none spends zero even with a cap of 1.
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "evaluated_live synthetic run_spend=0 headroom=5000000000 completed=2 unavailable=0 refused=0 interrupted=0 pending=0\n"
    );
    assert!(f.s.exists("out/checkpoint.json"));
    let entries: Vec<_> = std::fs::read_dir(f.s.path("out/cases"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries.iter().filter(|n| n.ends_with(".live.json")).count(),
        2
    );
    assert_eq!(
        entries.iter().filter(|n| n.ends_with(".run.json")).count(),
        2
    );
    assert_eq!(f.json("out/live-run.json")["counts"]["completed"], 2);
}
#[test]
fn empty_authorization_fails_probe_and_leaves_every_item_pending() {
    let f = Fixture::new();
    f.change("live.json", |c| c["authorize"] = json!([]));
    expect(&f.run(), 15, "probe_failed synthetic");
    f.pending();
}
#[test]
fn locked_journal_is_io_error_before_dispatch() {
    let f = Fixture::new();
    let _journal = SpendJournal::open(
        f.s.path("journal.jsonl"),
        BudgetKind::Testing,
        Arc::new(SystemClock),
    )
    .unwrap();
    let before = f.s.read("journal.jsonl");
    expect(&f.run(), 3, "journal-locked");
    assert_eq!(before, f.s.read("journal.jsonl"));
    assert!(!f.s.exists("out/live-run.json"));
}
#[test]
fn real_declaration_without_admission_is_rejected_before_journal() {
    let f = Fixture::new();
    // The content remains SYNTHETIC; only the provenance declaration exercises admission.
    f.change("dataset.json", |d| d["provenance"] = json!({"real":{"source":"SYNTHETIC","license":"test","labeling":"test","limitations":"synthetic only"}}));
    expect(&f.run(), 4, "data-not-admitted");
    assert!(!f.s.exists("journal.jsonl"));
    assert!(!f.s.exists("out"));
}
#[test]
fn missing_snapshot_is_unavailable_but_identity_mismatch_is_pre_journal() {
    let f = Fixture::new();
    std::fs::remove_file(f.s.path("snapshots/one.json")).unwrap();
    expect(&f.run(), 0, "completed=1 unavailable=1");
    let f = Fixture::new();
    f.s.write("snapshots/one.json", &f.s.read("probe.json"));
    expect(&f.run(), 4, "identity-mismatch");
    assert!(!f.s.exists("journal.jsonl"));
}

impl Fixture {
    fn bundles(&mut self) {
        let snapshot = common::support_snapshot_default();
        let plan = self.json("candidates/a/plan.json");
        let digest = snapshot.record_digest().unwrap();
        let expires = common::NOW + 100_000;
        let retained = |schema: &str| {
            json!({"document_schema":schema,"identity":null,
            "retention":{"external":{"reference":"never-read","digest":digest,"expires_at_ms":expires}}})
        };
        let bundle = json!({"schema":"rustev.replay/1","decision_id":"SYNTHETIC source",
            "scope":{"mode":"tenant_only","tenant":"synthetic","context_revision":"1"},
            "created_at_ms":common::NOW,"expires_at_ms":expires,"evaluation_time_ms":common::NOW,
            "plan_id":rustev_contract::plan::Plan::parse(&serde_json::to_vec(&plan).unwrap()).unwrap().id().unwrap(),
            "plan":retained(rustev_contract::schema::PLAN),"descriptors":[],"calibrations":[],
            "snapshot_id":snapshot.id().unwrap(),"snapshot":{"document_schema":"rustev.snapshot/1","identity":snapshot.id().unwrap(),
                "retention":{"retained":{"bytes":String::from_utf8(snapshot.record_canonical().unwrap()).unwrap(),"digest":digest,"expires_at_ms":expires}}},
            "run":retained("rustev.run/1"),"capture":"disabled","supplies":[]});
        rustev_contract::replay::ReplayBundle::parse(&serde_json::to_vec(&bundle).unwrap())
            .unwrap()
            .check(None)
            .unwrap();
        for id in ["one", "two"] {
            self.s.write(
                &format!("bundles/{id}.json"),
                &serde_json::to_vec(&bundle).unwrap(),
            );
        }
        let i = self.args.iter().position(|s| s == "--snapshots").unwrap();
        self.args[i] = "--bundles".into();
        self.args[i + 1] = self.s.path("bundles").to_str().unwrap().into();
        self.change("live.json", |c| c["evaluation_time"] = json!("source"));
    }
}
#[test]
fn bundles_use_retained_snapshot_and_time_without_restoring_source_execution() {
    let mut f = Fixture::new();
    f.bundles();
    f.change("bundles/two.json", |b| {
        b["schema"] = json!("rustev.replay/2");
        b["plan"]["document_schema"] = json!(rustev_contract::schema::PLAN_V3);
        b["run"]["document_schema"] = json!(rustev_contract::schema::RUN_V3);
    });
    expect(&f.run(), 0, "completed=2 unavailable=0");
}
#[test]
fn expired_corrupt_and_inaccessible_bundles_are_unavailable() {
    let mut f = Fixture::new();
    f.bundles();
    let i = f.args.iter().position(|s| s == "--now-ms").unwrap();
    f.args[i + 1] = (common::NOW + 100_000).to_string();
    f.s.write("bundles/two.json", b"invalid JSON");
    expect(&f.run(), 0, "completed=0 unavailable=2");
    let checkpoint = f.json("out/checkpoint.json");
    assert_eq!(
        checkpoint["items"][0]["states"][1]["unavailable"]["reason"],
        "expired"
    );
    assert_eq!(
        checkpoint["items"][1]["states"][1]["unavailable"]["reason"],
        "bundle-corrupt"
    );
    let mut f = Fixture::new();
    f.bundles();
    std::fs::remove_file(f.s.path("bundles/one.json")).unwrap();
    std::fs::create_dir(f.s.path("bundles/one.json")).unwrap();
    expect(&f.run(), 0, "completed=1 unavailable=1");
}
#[test]
fn admission_and_candidate_bytes_are_bound_in_checkpoint() {
    let mut f = Fixture::new();
    f.s.write(
        "admission.json",
        br#"{"declaration":"SYNTHETIC admission fixture"}"#,
    );
    f.args.extend([
        "--admission".into(),
        f.s.path("admission.json").to_str().unwrap().into(),
    ]);
    expect(&f.run(), 0, "evaluated_live");
    let checkpoint = f.json("out/checkpoint.json");
    let digests = checkpoint["bindings"]["input_digests"].as_object().unwrap();
    for path in [
        "dataset.json",
        "config.json",
        "adapter.json",
        "repeat.json",
        "admission.json",
        "candidates/a/plan.json",
        "candidates/a/rules/support.json",
        "candidates/a/calibrations/topic.json",
    ] {
        assert!(
            digests.contains_key(f.s.path(path).to_str().unwrap()),
            "{path}"
        );
    }
}
#[test]
fn first_matching_authorization_rule_decides_and_unknown_backends_refuse_early() {
    let f = Fixture::new();
    f.change("live.json", |c| {
        let allow = c["authorize"][0].clone();
        c["authorize"][0]["operations"] = json!([]);
        c["authorize"].as_array_mut().unwrap().push(allow);
    });
    expect(&f.run(), 15, "probe_failed");
    f.pending();
    for unit in ["none", "nano_usd"] {
        let f = Fixture::new();
        f.change("live.json", |c| {
            c["bindings"][0]["identity"]["backend_id"] = json!("not-linked");
            c["bindings"][0]["cost_unit"] = json!(unit);
        });
        expect(
            &f.run(),
            4,
            if unit == "none" {
                "spend-unit"
            } else {
                "live-config"
            },
        );
        assert!(!f.s.exists("journal.jsonl"));
    }
}
#[test]
fn source_time_requires_bundles_and_resume_binds_input_bytes() {
    let f = Fixture::new();
    f.change("live.json", |c| c["evaluation_time"] = json!("source"));
    expect(&f.run(), 4, "evaluation-time-unavailable");
    assert!(!f.s.exists("journal.jsonl"));
    let mut f = Fixture::new();
    expect(&f.run(), 0, "evaluated_live");
    f.args.push("--resume".into());
    expect(&f.run(), 0, "completed=2");
    f.change("config.json", |c| {
        c["label_interpretation"] = json!("SYNTHETIC changed")
    });
    expect(&f.run(), 4, "checkpoint-mismatch");
}

#[test]
fn journal_budget_mismatch_and_cancellation_have_distinct_statuses() {
    let f = Fixture::new();
    drop(
        SpendJournal::open(
            f.s.path("journal.jsonl"),
            BudgetKind::Production,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let before = f.s.read("journal.jsonl");
    expect(&f.run(), 4, "journal-budget");
    assert_eq!(before, f.s.read("journal.jsonl"));
    let f = Fixture::new();
    let context = Context::default();
    context.cancel.raise();
    expect(&execute(&f.args, &context), 16, "interrupted synthetic");
    f.pending();
}
#[test]
fn bad_live_config_and_probe_are_refused_before_journal() {
    for edit in [0, 1, 2] {
        let f = Fixture::new();
        match edit {
            0 => f.change("live.json", |c| {
                c.as_object_mut().unwrap().remove("authorize");
            }),
            1 => f.change("live.json", |c| {
                c["bindings"][0]["max_in_flight"] = json!(0)
            }),
            _ => {
                f.s.write("probe.json", &f.s.read("snapshots/one.json"));
            }
        }
        expect(
            &f.run(),
            4,
            if edit == 2 {
                "probe-snapshot"
            } else {
                "live-config"
            },
        );
        assert!(!f.s.exists("journal.jsonl"));
        assert!(!f.s.exists("out"));
    }
}
