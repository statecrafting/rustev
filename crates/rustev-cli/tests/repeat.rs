//! Spec 029 document commands with deterministic synthetic evidence.
mod common;
#[path = "../../rustev-eval/tests/selection_support/mod.rs"]
mod evidence;
use common::Scratch;
use evidence::Fixture;
use rustev_contract::Document;
use rustev_eval::metrics::TaskAdapter;
use serde_json::json;

fn adapter() -> rustev_surface::adapter::TaskAdapterDoc {
    rustev_surface::adapter::TaskAdapterDoc::parse(br#"{"schema":"rustev.task-adapter/1","name":"test","version":"1","labels":{"values":["yes"],"prefixes":[]},"rules":[{"action":"yes","label":"yes"}]}"#).unwrap()
}
fn fixture() -> Fixture {
    let mut f = Fixture::new();
    let rules = adapter().rules();
    for (e, c) in [(&mut f.b, &mut f.bc), (&mut f.c, &mut f.cc)] {
        e.config.adapter.rules = Some(rules.clone());
        Fixture::bind(e, c);
    }
    f
}
fn write_set(s: &Scratch, dir: &str, f: &Fixture, candidate: bool) {
    let (_, b, c) = f.reports();
    let report = if candidate { c } else { b };
    let (e, c) = if candidate {
        (&f.c, &f.cc)
    } else {
        (&f.b, &f.bc)
    };
    for (name, bytes) in [
        ("dataset.json", f.manifest.canonical().unwrap()),
        ("adapter.json", adapter().canonical().unwrap()),
        ("config.json", e.config.canonical().unwrap()),
        ("repeat-config.json", f.config.canonical().unwrap()),
        ("repeat.json", report.record_canonical().unwrap()),
        ("run-0.report.json", e.report.record_canonical().unwrap()),
        ("run-0.detail.json", e.detail.record_canonical().unwrap()),
        ("run-0.cases.json", c.record_canonical().unwrap()),
    ] {
        s.write(&format!("{dir}/{name}"), &bytes);
    }
}
fn setup(s: &Scratch) -> Fixture {
    let f = fixture();
    write_set(s, "base", &f, false);
    s.write("baseline.json", &f.reports().0.record_canonical().unwrap());
    f
}
fn golden(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("repeat-{name}"));
    if std::env::var_os("UPDATE_REPEAT_GOLDENS").is_some() {
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "{}", path.display());
    common::harvest::golden(&format!("repeat-{name}"), bytes);
}
#[test]
fn baseline_records_verified_documents_and_refuses_tampering() {
    let s = Scratch::new("baseline");
    let f = setup(&s);
    let args = [
        "baseline",
        "record",
        "--name",
        "base",
        "--eval",
        "base",
        "--now-ms",
        "0",
        "--out",
        "recorded.json",
    ];
    let r = s.rustev(&args);
    r.expect(0, "recorded");
    golden("recorded.stdout", &r.stdout);
    assert_eq!(s.read("recorded.json"), s.read("baseline.json"));
    s.rustev(&args).expect(3, "io_error");
    std::fs::remove_file(s.path("recorded.json")).unwrap();
    let mut bad = f.clone();
    bad.bc.cases[0].measures.clear();
    s.write("base/run-0.cases.json", &bad.bc.record_canonical().unwrap());
    s.rustev(&args).expect(4, "invalid_input");
    assert!(!s.exists("recorded.json"));
    write_set(&s, "base", &f, false);
    let mut bad = args;
    bad[3] = "invalid name";
    s.rustev(&bad).expect(4, "invalid_input");
}
#[test]
fn reproducibility_exact_band_drift_and_unknown_exit_codes() {
    let s = Scratch::new("reproducibility");
    let mut f = setup(&s);
    let args = [
        "eval",
        "reproducibility",
        "--baseline",
        "baseline.json",
        "--baseline-eval",
        "base",
        "--run",
        "run",
    ];
    for (name, loss, code, status) in [
        ("reproducible", 0.5, 0, "reproducible"),
        ("drift", 0.625, 13, "not_reproducible"),
    ] {
        f.loss(false, loss, loss);
        write_set(&s, "run", &f, false);
        let r = s.rustev(&args);
        r.expect(code, status);
        golden(&format!("{name}.stdout"), &r.stdout);
    }
    f.loss(false, 0.5, 0.5);
    f.config.bootstrap.seed = "30".into();
    write_set(&s, "run", &f, false);
    let r = s.rustev(&args);
    r.expect(12, "unknown");
    golden("unknown.stdout", &r.stdout);
    // An inclusive edge passes; a larger drift fails, with the baseline verified.
    f = fixture();
    f.config.noise.floor = evidence::dec("0.125");
    write_set(&s, "base", &f, false);
    s.write("baseline.json", &f.reports().0.record_canonical().unwrap());
    f.loss(false, 0.625, 0.625);
    write_set(&s, "run", &f, false);
    s.rustev(&args).expect(0, "reproducible");
    f.loss(
        false,
        f64::from_bits(0.625f64.to_bits() + 1),
        f64::from_bits(0.625f64.to_bits() + 1),
    );
    write_set(&s, "run", &f, false);
    s.rustev(&args).expect(13, "not_reproducible");
}
#[test]
fn diff_documents_and_markdown_have_stable_bytes_and_unknowns() {
    let s = Scratch::new("diff");
    let mut f = setup(&s);
    f.loss(false, 0.75, 0.25);
    write_set(&s, "run", &f, false);
    let args = [
        "eval",
        "diff",
        "--baseline",
        "baseline.json",
        "--baseline-eval",
        "base",
        "--run",
        "run",
        "--out",
        "diff",
    ];
    // Preserve the required output directory in the harvested inputs.
    s.write("diff/.keep", b"");
    let r = s.rustev(&args);
    r.expect(0, "diffed");
    golden("diff.stdout", &r.stdout);
    for file in ["diff.json", "diff.md"] {
        golden(file, &s.read(&format!("diff/{file}")));
    }
    s.rustev(&args).expect(3, "io_error");
    std::fs::remove_file(s.path("diff/diff.json")).unwrap();
    s.rustev(&args).expect(3, "io_error");
    assert!(!s.exists("diff/diff.json"));
    std::fs::remove_file(s.path("diff/diff.md")).unwrap();
    f.config.bootstrap.seed = "30".into();
    write_set(&s, "run", &f, false);
    s.rustev(&args).expect(12, "unknown");
    golden("diff-unknown.md", &s.read("diff/diff.md"));
}
#[test]
fn selection_all_verdicts_and_mixed_unknown_are_golden() {
    let s = Scratch::new("selection");
    let mut f = setup(&s);
    for (name, loss, source, code, status) in [
        (
            "qualifies",
            0.5,
            rustev_eval::repeat::Source::Replay,
            0,
            "selected",
        ),
        (
            "reject",
            0.75,
            rustev_eval::repeat::Source::Replay,
            11,
            "none_qualified",
        ),
        (
            "unknown",
            0.5,
            rustev_eval::repeat::Source::Captured,
            12,
            "unknown",
        ),
    ] {
        f.loss(true, loss, loss);
        f.source = source;
        write_set(&s, "base", &f, false);
        s.write("baseline.json", &f.reports().0.record_canonical().unwrap());
        write_set(&s, "candidates/one", &f, true);
        s.write("gate.json", &evidence::gate().canonical().unwrap());
        let out = format!("{name}.json");
        let r = s.rustev(&[
            "gate",
            "select",
            "--selection-gate",
            "gate.json",
            "--baseline",
            "baseline.json",
            "--baseline-eval",
            "base",
            "--candidates",
            "candidates",
            "--out",
            &out,
        ]);
        r.expect(code, status);
        golden(&format!("selection-{name}.stdout"), &r.stdout);
        golden(&format!("selection-{name}.json"), &s.read(&out));
    }
    f.source = rustev_eval::repeat::Source::Replay;
    write_set(&s, "base", &f, false);
    s.write("baseline.json", &f.reports().0.record_canonical().unwrap());
    write_set(&s, "candidates/one", &f, true);
    f.config.bootstrap.seed = "30".into();
    write_set(&s, "candidates/two", &f, true);
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
        "candidates",
        "--out",
        "mixed.json",
    ])
    .expect(12, "unknown");
    let mut gate = serde_json::to_value(evidence::gate()).unwrap();
    gate["metrics"] = json!({});
    s.write("gate.json", gate.to_string().as_bytes());
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
        "candidates",
        "--out",
        "invalid.json",
    ])
    .expect(4, "invalid_input");
    assert!(!s.exists("invalid.json"));
}

#[test]
fn baseline_binding_is_checked_before_diff_or_reproducibility() {
    let s = Scratch::new("baseline-binding");
    let f = setup(&s);
    write_set(&s, "run", &f, false);
    let mut baseline = f.reports().0;
    baseline.recorded_at_ms = 1;
    // A copied metric may not be edited independently of its repeat set.
    baseline.cohorts[0].metrics[0].pooled = rustev_eval::repeat::FigureDoc::Value(0.25);
    s.write("baseline.json", &baseline.record_canonical().unwrap());
    s.rustev(&[
        "eval",
        "reproducibility",
        "--baseline",
        "baseline.json",
        "--baseline-eval",
        "base",
        "--run",
        "run",
    ])
    .expect(4, "invalid_input");
}

#[test]
fn repeat_document_refusals_and_live_source_acceptance() {
    let s = Scratch::new("repeat-document-refusals");
    let f = setup(&s);
    let args = [
        "baseline",
        "record",
        "--name",
        "base",
        "--eval",
        "base",
        "--now-ms",
        "0",
        "--out",
        "recorded.json",
    ];
    for name in [
        "dataset.json",
        "config.json",
        "adapter.json",
        "repeat-config.json",
        "repeat.json",
        "run-0.report.json",
        "run-0.detail.json",
        "run-0.cases.json",
    ] {
        let path = format!("base/{name}");
        let bytes = s.read(&path);
        s.write(&path, b"{}");
        s.rustev(&args).expect(4, "invalid_input");
        assert!(!s.exists("recorded.json"));
        s.write(&path, &bytes);
    }
    let report = rustev_eval::repeat::repeat(&rustev_eval::repeat::RepeatInputs {
        source: rustev_eval::repeat::Source::Live,
        config: &f.config,
        manifest: &f.manifest,
        runs: &[rustev_eval::repeat::Run {
            evaluated: &f.b,
            cases: &f.bc,
        }],
    })
    .unwrap();
    s.write("base/repeat.json", &report.record_canonical().unwrap());
    let result = s.rustev(&args);
    result.expect(0, "recorded");
    let recorded = rustev_eval::baseline::Baseline::parse(&s.read("recorded.json")).unwrap();
    assert_eq!(recorded.source, rustev_eval::repeat::Source::Live);
    std::fs::remove_file(s.path("recorded.json")).unwrap();
    std::fs::remove_file(s.path("base/repeat.json")).unwrap();
    s.rustev(&args).expect(3, "io_error");
}

#[test]
fn selection_refuses_inconsistent_candidate_binding() {
    let s = Scratch::new("selection-binding");
    let f = setup(&s);
    write_set(&s, "candidates/one", &f, true);
    s.write("gate.json", &evidence::gate().canonical().unwrap());
    // The detail remains bound to the original report, not this edited report.
    let mut report = f.c.report.clone();
    report.plan = f.b.report.plan.clone();
    s.write(
        "candidates/one/run-0.report.json",
        &report.record_canonical().unwrap(),
    );
    let result = s.rustev(&[
        "gate",
        "select",
        "--selection-gate",
        "gate.json",
        "--baseline",
        "baseline.json",
        "--baseline-eval",
        "base",
        "--candidates",
        "candidates",
        "--out",
        "selection.json",
    ]);
    result.expect(4, "invalid_input");
    assert!(
        result
            .stdout_str()
            .contains("run configuration or detail binding mismatch")
    );
    assert!(!s.exists("selection.json"));
}
