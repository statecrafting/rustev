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
