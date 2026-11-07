//! Spec 061: host repeat sets through the CLI, over the synthetic host
//! fixture shared with the eval tests. Goldens live under
//! `tests/golden/host/`.
mod common;
#[path = "../../rustev-eval/tests/host_support/mod.rs"]
mod host;

use common::Scratch;
use host::{HostRunDocs, RunSpec, config, repeat_config, set_docs, three_runs};
use rustev_contract::Document;
use rustev_eval::host_repeat::{HostRepeatReport, HostSetInputs, reassemble};
use rustev_eval::host_run::{HostCases, HostRun, HostRunInputs};
use rustev_eval::scores::{ScoreViews, Scores};

fn golden(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/host")
        .join(name);
    if std::env::var_os("UPDATE_HOST_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "{}", path.display());
}

fn golden_bytes(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/host")
            .join(name),
    )
    .unwrap()
}

/// Write one host run directory: `host-run.json`, `views.json` and one
/// file per scores document under `scores/`.
fn write_run(s: &Scratch, dir: &str, d: &HostRunDocs) {
    s.write(
        &format!("{dir}/host-run.json"),
        &d.record.record_canonical().unwrap(),
    );
    s.write(
        &format!("{dir}/views.json"),
        &d.views.record_canonical().unwrap(),
    );
    for doc in &d.scores {
        s.write(
            &format!("{dir}/scores/{}.json", doc.scorer.id),
            &doc.record_canonical().unwrap(),
        );
    }
}

/// The configurations and each run under `<prefix>run-<r>`.
fn write_inputs(s: &Scratch, prefix: &str, docs: &[HostRunDocs]) -> Vec<String> {
    s.write("repeat-config.json", &repeat_config().canonical().unwrap());
    s.write("config.json", &config().canonical().unwrap());
    docs.iter()
        .enumerate()
        .map(|(r, d)| {
            let dir = format!("{prefix}run-{r}");
            write_run(s, &dir, d);
            dir
        })
        .collect()
}

fn import_args<'a>(runs: &'a [String], out: &'a str) -> Vec<&'a str> {
    let mut args = vec![
        "eval",
        "repeat",
        "import",
        "--repeat-config",
        "repeat-config.json",
        "--config",
        "config.json",
    ];
    for r in runs {
        args.extend(["--run", r.as_str()]);
    }
    args.extend(["--out", out]);
    args
}

#[test]
fn import_writes_the_host_set_and_prints_host_asserted_figures() {
    let s = Scratch::new("host-import");
    let c = config();
    let docs = set_docs(&c, &three_runs());
    let runs = write_inputs(&s, "in/", &docs);
    std::fs::create_dir(s.path("out")).unwrap();
    let before: Vec<Vec<u8>> = runs
        .iter()
        .map(|r| s.read(&format!("{r}/host-run.json")))
        .collect();
    let args = import_args(&runs, "out");
    let r = s.rustev(&args);
    let out = r.expect(0, "imported");
    golden("import.stdout", &r.stdout);
    golden("import.repeat.json", &s.read("out/repeat.json"));
    golden("import.run-1.cases.json", &s.read("out/run-1.cases.json"));
    assert_eq!(out["n"], 3);
    assert_eq!(out["assertion"], "host-asserted");
    for cohort in out["cohorts"].as_array().unwrap() {
        for m in cohort["metrics"].as_array().unwrap() {
            assert_eq!(m["assertion"], "host-asserted");
        }
    }
    // Byte copies of the inputs, and the inputs untouched.
    for (i, run) in runs.iter().enumerate() {
        assert_eq!(s.read(&format!("out/run-{i}.host-run.json")), before[i]);
        assert_eq!(s.read(&format!("{run}/host-run.json")), before[i]);
        assert_eq!(
            s.read(&format!("out/run-{i}.views.json")),
            s.read(&format!("{run}/views.json"))
        );
        for id in ["length", "tone", "judge"] {
            assert_eq!(
                s.read(&format!("out/run-{i}.scores-{id}.json")),
                s.read(&format!("{run}/scores/{id}.json"))
            );
        }
    }
    assert_eq!(
        s.read("out/repeat-config.json"),
        s.read("repeat-config.json")
    );
    assert_eq!(s.read("out/config.json"), s.read("config.json"));
    // The output holds every input re-assembly needs (3.3.4).
    let report = HostRepeatReport::parse(&s.read("out/repeat.json")).unwrap();
    assert_eq!(
        out["repeat_digest"],
        report.record_digest().unwrap().as_str()
    );
    let mut records = vec![];
    let mut cases = vec![];
    for i in 0..3 {
        let f = |k: &str| s.read(&format!("out/run-{i}.{k}.json"));
        records.push((
            HostRun::parse(&f("host-run")).unwrap(),
            ScoreViews::parse(&f("views")).unwrap(),
            ["length", "tone", "judge"]
                .map(|id| Scores::parse(&f(&format!("scores-{id}"))).unwrap())
                .to_vec(),
        ));
        cases.push(HostCases::parse(&f("cases")).unwrap());
    }
    let inputs: Vec<HostRunInputs<'_>> = records
        .iter()
        .map(|(record, views, scores)| HostRunInputs {
            record,
            views: Some(views),
            scores,
        })
        .collect();
    let rc = repeat_config();
    reassemble(
        &HostSetInputs {
            config: &rc,
            evaluator: &c,
            runs: &inputs,
        },
        &report,
        &cases,
    )
    .unwrap();
    // No clobber.
    s.rustev(&args).expect(3, "io_error");
}

/// Import into a fresh `out`, expecting a refusal with nothing written.
fn refuse(s: &Scratch, runs: &[String], needle: &str) {
    let _ = std::fs::remove_dir_all(s.path("out"));
    std::fs::create_dir(s.path("out")).unwrap();
    let r = s.rustev(&import_args(runs, "out"));
    let out = r.expect(4, "invalid_input");
    assert!(out.to_string().contains(needle), "{needle}: {out}");
    assert_eq!(std::fs::read_dir(s.path("out")).unwrap().count(), 0);
}

#[test]
fn import_refusals_write_nothing() {
    let s = Scratch::new("host-import-refusals");
    let c = config();
    let docs = set_docs(&c, &three_runs());
    let runs = write_inputs(&s, "", &docs);
    // A record whose views name no supplied file.
    std::fs::remove_file(s.path("run-1/views.json")).unwrap();
    refuse(&s, &runs, "host-binding");
    write_run(&s, "run-1", &docs[1]);
    // A scores file the record does not name.
    s.write(
        "run-0/scores/length.json",
        &docs[1].scores[0].record_canonical().unwrap(),
    );
    refuse(&s, &runs, "host-binding");
    write_run(&s, "run-0", &docs[0]);
    // Two runs with different flow versions.
    let mut spec = RunSpec::new(2);
    spec.flow_version = "v2".into();
    write_run(&s, "run-2", &HostRunDocs::new(&c, &spec));
    refuse(&s, &runs, "mixed-runs");
    // A repeated run handle.
    let mut spec = RunSpec::new(2);
    spec.run = "run-0".into();
    write_run(&s, "run-2", &HostRunDocs::new(&c, &spec));
    refuse(&s, &runs, "duplicate-run");
    write_run(&s, "run-2", &docs[2]);
    // A repeat configuration with `top_k`.
    let mut v = host::repeat_config_json();
    v["top_k"] = serde_json::json!({"param": "ranking", "k": 3});
    s.write("repeat-config.json", &serde_json::to_vec(&v).unwrap());
    refuse(&s, &runs, "top-k-not-observed");
}

#[test]
fn import_is_its_own_command_path() {
    let s = Scratch::new("host-import-usage");
    // `eval repeat import` never parses as `eval`.
    let r = s.rustev(&["eval", "repeat", "import", "--dataset", "d.json"]);
    assert_eq!(r.code, 2);
    assert!(
        r.stderr
            .contains("eval repeat import takes no flag --dataset"),
        "{}",
        r.stderr
    );
    let mut args = vec![
        "eval",
        "repeat",
        "import",
        "--repeat-config",
        "a",
        "--config",
        "b",
        "--out",
        "o",
    ];
    for _ in 0..65 {
        args.extend(["--run", "r"]);
    }
    let r = s.rustev(&args);
    assert_eq!(r.code, 2, "{}", r.stderr);
}

/// Import `specs` into `out` through the CLI, returning `out`.
fn imported(s: &Scratch, out: &str, specs: &[RunSpec]) -> String {
    let docs = set_docs(&config(), specs);
    let runs = write_inputs(s, &format!("in-{}/", out.replace('/', "-")), &docs);
    std::fs::create_dir_all(s.path(out)).unwrap();
    s.rustev(&import_args(&runs, out)).expect(0, "imported");
    out.to_string()
}

fn base_specs() -> Vec<RunSpec> {
    (0..3).map(RunSpec::new).collect()
}

fn candidate(edit: impl Fn(&mut RunSpec)) -> Vec<RunSpec> {
    (0..3)
        .map(|r| {
            let mut s = RunSpec::new(r);
            s.flow_version = "v2".into();
            edit(&mut s);
            s
        })
        .collect()
}

/// The consumer's gate of 3.7.6.
fn write_gate(s: &Scratch) {
    let gate = serde_json::json!({
        "schema": "rustev.selection-gate/1", "gate": "host-flow", "per": "variant",
        "metrics": {
            "rubric/brevity": "higher_is_better",
            "host/latency_ms/p0.5": "lower_is_better",
            "host/latency_ms/p0.95": "lower_is_better",
            "host/cost_per_1000": "lower_is_better"
        },
        "min_comparable_coverage": "0.8", "min_labeled_coverage": "0.4",
        "require": [
            {"all_slices": {"metric": "rubric/brevity", "not_worse_than": "baseline", "within": "noise"}}
        ],
        "and_any": [
            {"metric": "host/latency_ms/p0.5", "better_than": "baseline"},
            {"metric": "host/latency_ms/p0.95", "better_than": "baseline"},
            {"metric": "host/cost_per_1000", "better_than": "baseline"}
        ],
        "rank_by": ["host/cost_per_1000"]
    });
    s.write("gate.json", &serde_json::to_vec(&gate).unwrap());
}

fn record_baseline(s: &Scratch) {
    imported(s, "base", &base_specs());
    let r = s.rustev(&[
        "baseline",
        "record",
        "--name",
        "base",
        "--eval",
        "base",
        "--now-ms",
        "0",
        "--out",
        "baseline.json",
    ]);
    r.expect(0, "recorded");
    golden("recorded.stdout", &r.stdout);
    golden("baseline.json", &s.read("baseline.json"));
}

#[test]
fn a_host_baseline_and_its_reproducibility() {
    let s = Scratch::new("host-baseline");
    record_baseline(&s);
    let check = |run: &str| {
        s.rustev(&[
            "eval",
            "reproducibility",
            "--baseline",
            "baseline.json",
            "--baseline-eval",
            "base",
            "--run",
            run,
        ])
    };
    let r = check("base");
    r.expect(0, "reproducible");
    golden("reproducible.stdout", &r.stdout);
    let drift: Vec<RunSpec> = base_specs()
        .into_iter()
        .map(|s| RunSpec {
            score_shift: s.score_shift + 0.2,
            ..s
        })
        .collect();
    imported(&s, "drift", &drift);
    let r = check("drift");
    r.expect(13, "not_reproducible");
    golden("not-reproducible.stdout", &r.stdout);
    imported(&s, "v2", &candidate(|_| {}));
    check("v2").expect(12, "unknown");
    // A tampered baseline does not bind its import output.
    let mut b: serde_json::Value = serde_json::from_slice(&s.read("baseline.json")).unwrap();
    b["cohorts"][0]["metrics"][0]["pooled"] = serde_json::json!({"value": 0.25});
    s.write("tampered.json", &serde_json::to_vec(&b).unwrap());
    s.rustev(&[
        "eval",
        "reproducibility",
        "--baseline",
        "tampered.json",
        "--baseline-eval",
        "base",
        "--run",
        "base",
    ])
    .expect(4, "invalid_input");
}

fn select(s: &Scratch, candidates: &str, out: &str) -> common::Ran {
    s.rustev(&[
        "gate",
        "select",
        "--selection-gate",
        "gate.json",
        "--baseline",
        "baseline.json",
        "--baseline-eval",
        "base",
        "--candidates",
        candidates,
        "--out",
        out,
    ])
}

#[test]
fn host_selection_exit_codes() {
    let s = Scratch::new("host-select");
    record_baseline(&s);
    write_gate(&s);
    imported(&s, "good/faster", &candidate(|c| c.latency_base = 500));
    imported(
        &s,
        "good/cheaper",
        &candidate(|c| c.cost_shift -= 2_400_000),
    );
    let r = select(&s, "good", "selected.json");
    let out = r.expect(0, "selected");
    golden("selected.stdout", &r.stdout);
    golden("selection.json", &s.read("selected.json"));
    assert_eq!(out["ranked"], serde_json::json!(["cheaper", "faster"]));
    assert_eq!(out["assertion"], "host-asserted");
    select(&s, "good", "selected.json").expect(3, "io_error");
    // A tampered baseline is refused before any selection, nothing written.
    let mut b: serde_json::Value = serde_json::from_slice(&s.read("baseline.json")).unwrap();
    b["cohorts"][0]["metrics"][0]["pooled"] = serde_json::json!({"value": 0.25});
    s.write("baseline.json", &serde_json::to_vec(&b).unwrap());
    select(&s, "good", "tampered.json").expect(4, "invalid_input");
    assert!(!s.exists("tampered.json"));
    s.write("baseline.json", &golden_bytes("baseline.json"));

    imported(
        &s,
        "bad/worse",
        &candidate(|c| {
            c.latency_base = 500;
            c.score_shift -= 0.15;
        }),
    );
    imported(
        &s,
        "bad/failing",
        &candidate(|c| {
            c.latency_base = 500;
            c.extra_failed = vec!["c4"];
        }),
    );
    let r = select(&s, "bad", "rejected.json");
    r.expect(11, "none_qualified");
    golden("none-qualified.stdout", &r.stdout);

    imported(&s, "open/single", &candidate(|c| c.latency_base = 500)[..1]);
    imported(&s, "open/faster", &candidate(|c| c.latency_base = 500));
    let r = select(&s, "open", "unknown.json");
    r.expect(12, "unknown");
    golden("unknown.stdout", &r.stdout);

    // A candidate changed after import is unknown, not refused.
    imported(&s, "changed/faster", &candidate(|c| c.latency_base = 500));
    let mut rec: serde_json::Value =
        serde_json::from_slice(&s.read("changed/faster/run-0.host-run.json")).unwrap();
    rec["recorded_at_ms"] = serde_json::json!(5);
    s.write(
        "changed/faster/run-0.host-run.json",
        &serde_json::to_vec(&rec).unwrap(),
    );
    let out = select(&s, "changed", "changed.json").expect(12, "unknown");
    assert!(out.to_string().contains("reconcile"), "{out}");
}

#[test]
fn host_and_rustev_documents_are_never_mixed() {
    let s = Scratch::new("host-mixed");
    record_baseline(&s);
    write_gate(&s);
    // Rustev-evaluated documents, refused by schema before any parse.
    s.write("rustev-baseline.json", br#"{"schema":"rustev.baseline/1"}"#);
    s.write(
        "rustev/repeat.json",
        br#"{"schema":"rustev.repeat-report/1"}"#,
    );
    s.write(
        "rustev-cands/a/repeat.json",
        br#"{"schema":"rustev.repeat-report/1"}"#,
    );
    imported(&s, "host-cands/a", &candidate(|c| c.latency_base = 500));
    let mixed = |args: &[&str]| {
        let r = s.rustev(args);
        let out = r.expect(4, "invalid_input");
        assert!(out.to_string().contains("mixed-provenance"), "{out}");
    };
    // A host baseline with Rustev candidates, and the reverse.
    let sel = |b: &str, be: &str, c: &str| {
        mixed(&[
            "gate",
            "select",
            "--selection-gate",
            "gate.json",
            "--baseline",
            b,
            "--baseline-eval",
            be,
            "--candidates",
            c,
            "--out",
            "sel.json",
        ])
    };
    sel("baseline.json", "base", "rustev-cands");
    sel("rustev-baseline.json", "rustev", "host-cands");
    // A Rustev baseline where a host baseline is expected.
    sel("rustev-baseline.json", "base", "host-cands");
    assert!(!s.exists("sel.json"));
    mixed(&[
        "eval",
        "reproducibility",
        "--baseline",
        "baseline.json",
        "--baseline-eval",
        "base",
        "--run",
        "rustev",
    ]);
    mixed(&[
        "eval",
        "reproducibility",
        "--baseline",
        "rustev-baseline.json",
        "--baseline-eval",
        "base",
        "--run",
        "base",
    ]);
    s.write("halfway/repeat.json", &s.read("base/repeat.json"));
    s.write(
        "halfway/run-0.cases.json",
        br#"{"schema":"rustev.eval-cases/1"}"#,
    );
    mixed(&[
        "baseline", "record", "--name", "x", "--eval", "halfway", "--now-ms", "0", "--out",
        "x.json",
    ]);
    assert!(!s.exists("x.json"));
    // `eval diff` refuses a host document by schema.
    std::fs::create_dir(s.path("diff")).unwrap();
    s.rustev(&[
        "eval",
        "diff",
        "--baseline",
        "baseline.json",
        "--baseline-eval",
        "base",
        "--run",
        "base",
        "--out",
        "diff",
    ])
    .expect(4, "invalid_input");
}

#[test]
fn case_regressions_stdout_keeps_host_assertion_and_full_file_evidence() {
    let s = Scratch::new("host-case-regressions");
    record_baseline(&s);
    imported(&s, "candidates/worse", &candidate(|c| c.score_shift -= 0.1));
    let gate = serde_json::json!({
        "schema":"rustev.selection-gate/1","gate":"cases","per":"variant",
        "metrics":{"rubric/brevity":"higher_is_better"},"min_comparable_coverage":"0","min_labeled_coverage":"0",
        "require":[{"metric":"rubric/brevity","case_regressions":{"by":{"absolute":"0.05"},"at_most":0}}],"rank_by":[]
    });
    s.write("gate.json", &serde_json::to_vec(&gate).unwrap());
    let r = select(&s, "candidates", "selection.json");
    let out = r.expect(11, "none_qualified");
    assert_eq!(out["assertion"], "host-asserted");
    let summary = &out["candidates"][0]["clauses"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["cases"];
    assert_eq!(summary["regressions"], 4);
    assert_eq!(summary["at_most"], 0);
    assert_eq!(summary["unresolved"], 0);
    assert_eq!(summary["first"][0]["case"], "c1");
    let file: serde_json::Value = serde_json::from_slice(&s.read("selection.json")).unwrap();
    let cases = &file["candidates"][0]["clauses"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["cases"];
    assert_eq!(cases["regressions"]["total"], 4);
    assert_eq!(cases["unmeasured"], 2);
    assert!(cases["regressions"]["list"][0]["baseline"].is_number());
}
