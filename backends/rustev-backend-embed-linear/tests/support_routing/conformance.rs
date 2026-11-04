//! Spec 011, 3.17.5: the shared conformance suite's local-embed cases, run
//! through the Rust API. The suite's runner (`rustev_conformance::local_embed`)
//! takes the opened backend as a seam, so the suite never depends on this
//! crate; this test opens the committed SYNTHETIC support-routing bundle and
//! hands it over. The native binding built with `local-embed` must produce
//! the same bytes (`run.mjs native --local-embed`). Mechanics only.
//!
//! `RUSTEV_BLESS=1` rewrites the suite's local-embed fixtures, case files
//! and expected outputs (then `RUSTEV_BLESS=1 cargo test -p
//! rustev-conformance --test manifest` aggregates the manifest); without
//! it, each must equal what is committed.
use super::*;

use rustev_conformance::local_embed::{self, LocalEmbedCase};
use rustev_conformance::{compare, contained, manifest, pinned, sha256_hex};

const SUITE: &str = "tools/rustev-conformance";
const BUNDLE: &str = "backends/rustev-backend-embed-linear/tests/fixtures/support-routing";
const ID: &str = "local-embed.support-routing";
/// One package case per topic family, and one whose message is blank.
const CASES: [&str; 4] = ["sr-c01", "sr-f01", "sr-m01", "sr-t01"];
const BLANK: &str = "SYNTHETIC.blank-message";

fn repo() -> PathBuf {
    rustev_conformance::root().canonicalize().unwrap()
}

fn bless() -> bool {
    std::env::var_os("RUSTEV_BLESS").is_some()
}

/// Write `bytes` at `rel` under the suite when blessing; otherwise require
/// that they are what is committed there.
fn keep(rel: &str, bytes: &[u8]) {
    let p = repo().join(SUITE).join(rel);
    if bless() {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        return;
    }
    let got = fs::read(&p).unwrap_or_else(|e| panic!("{rel}: {e}; bless it"));
    assert!(got == bytes, "{rel} is stale; run with RUSTEV_BLESS=1");
}

fn line(mut b: Vec<u8>) -> Vec<u8> {
    b.push(b'\n');
    b
}

/// Opened as every surface opens it: the bundle at its repository path, by
/// its expected artifact, with the case's workers and queue.
fn open(case: &LocalEmbedCase) -> Arc<dyn DecisionBackend> {
    Arc::new(
        EmbedLinearBackend::open(EmbedConfig {
            bundle_dir: repo().join(contained(&case.bundle).unwrap()),
            runtime_path: None,
            expected_artifact: ArtifactId::parse(&case.expected_artifact).unwrap(),
            workers: case.workers,
            max_queued: case.max_queued,
            embedding_cache: None,
        })
        .unwrap(),
    )
}

/// The bundle's files the manifest names, and the manifest, by name.
fn bundle_files() -> BTreeMap<String, Vec<u8>> {
    let dir = fixtures::directory();
    let manifest = EmbedManifest::parse(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let mut out = BTreeMap::new();
    out.insert(
        "manifest.json".to_string(),
        fs::read(dir.join("manifest.json")).unwrap(),
    );
    for f in manifest.files() {
        out.insert(f.file.clone(), fs::read(dir.join(&f.file)).unwrap());
    }
    out
}

/// The fixtures by input name: the plan compiled against the bundle's
/// descriptor, the descriptor, the package's SYNTHETIC calibration rebound
/// to the bundle, and the snapshots.
fn fixtures_by_name(descriptor: &BackendDescriptor) -> BTreeMap<String, Vec<u8>> {
    let (compiled, cal) = plan(descriptor);
    let mut f = BTreeMap::new();
    f.insert("support.plan.json".into(), line(compiled.canonical()));
    f.insert(
        "support-routing.descriptor.json".into(),
        line(descriptor.canonical().unwrap()),
    );
    f.insert(
        "topic.calibration.json".into(),
        line(cal.canonical().unwrap()),
    );
    for case in CASES {
        f.insert(
            format!("{case}.snapshot.json"),
            line(pkg::snapshot(case).unwrap().to_vec()),
        );
    }
    // A blank message: the backend refuses it `text_empty` before any work,
    // supplied as `backend_unavailable` (3.7.3), and the steps are unresolved.
    let mut blank: Value = serde_json::from_slice(pkg::snapshot(CASES[0]).unwrap()).unwrap();
    let entry = blank["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["field"] == "ticket.message")
        .unwrap();
    entry["value"] = json!("   ");
    let blank = Snapshot::parse(&serde_json::to_vec(&blank).unwrap()).unwrap();
    f.insert(
        format!("{BLANK}.snapshot.json"),
        line(blank.canonical().unwrap()),
    );
    f
}

#[test]
fn every_local_embed_case_produces_its_expected_bytes() {
    let directory = embed(&fixtures::directory());
    let descriptor = directory.descriptor().clone();
    drop(directory);
    let fx = fixtures_by_name(&descriptor);
    for (name, bytes) in &fx {
        keep(&format!("fixtures/local-embed/{name}"), bytes);
    }
    let mut requests: Vec<Value> = CASES
        .iter()
        .map(|c| {
            json!({ "decision_id": format!("SYNTHETIC.{c}"),
                    "snapshot": format!("{c}.snapshot.json"),
                    "evaluation_time_ms": pkg::EVALUATION_TIME_MS,
                    "principal_handle": "SYNTHETIC-tenant/scope-p" })
        })
        .collect();
    requests.push(
        json!({ "decision_id": BLANK, "snapshot": format!("{BLANK}.snapshot.json"),
                          "evaluation_time_ms": pkg::EVALUATION_TIME_MS,
                          "principal_handle": "SYNTHETIC-tenant/scope-p" }),
    );
    let block = json!({
        "bundle": BUNDLE,
        "expected_artifact": descriptor.artifact.to_string(),
        "workers": 1,
        "max_queued": 2,
        "max_concurrency": 3,
        "plan": "support.plan.json",
        "descriptors": ["support-routing.descriptor.json"],
        "calibrations": ["topic.calibration.json"],
        "config": { "max_in_flight": 1, "max_queued": 0, "max_parallel_requests": 3,
                    "sink_timeout_ms": 1000 },
        "requests": requests,
    });
    let case: LocalEmbedCase = serde_json::from_value(block.clone()).unwrap();
    let out = local_embed::run(&case, &fx, open(&case)).unwrap();
    // Decided twice in fresh backends: the synthetic encoder is bitwise
    // repeatable (3.19.1), so the expected bytes are no accident of one run.
    assert_eq!(local_embed::run(&case, &fx, open(&case)).unwrap(), out);
    mirrors(&out);
    let mut outputs = vec![];
    for (file, bytes) in &out {
        let rel = format!("expected/local-embed/{ID}/{file}");
        keep(&rel, bytes);
        outputs.push(json!({ "file": file, "golden": format!("{SUITE}/{rel}"),
                             "sha256": sha256_hex(bytes), "normalize": [] }));
    }
    let mut inputs: Vec<Value> = bundle_files()
        .iter()
        .map(|(n, b)| {
            json!({ "path": format!("{BUNDLE}/{n}"), "name": format!("bundle/{n}"),
                    "sha256": sha256_hex(b) })
        })
        .collect();
    inputs.extend(fx.iter().map(|(n, b)| {
        json!({ "path": format!("{SUITE}/fixtures/local-embed/{n}"), "name": n,
                "sha256": sha256_hex(b) })
    }));
    let case_file = json!({
        "id": ID,
        "command": "decide",
        // The decision service's transports join with its own `local-embed`
        // feature (011 3.17.3).
        "surfaces": ["rust-api", "native"],
        "local_embed": block,
        "inputs": inputs,
        "outputs": outputs,
    });
    keep(
        &format!("cases/local-embed/{ID}.json"),
        &line(serde_json::to_vec_pretty(&case_file).unwrap()),
    );
}

/// What the expected bytes must show, so a blessed output is never merely
/// whatever the runner printed: every package case judged with its three
/// semantic steps `model-derived` and charged; the blank message refused
/// before any work and the decision still judged.
fn mirrors(out: &BTreeMap<String, Vec<u8>>) {
    use rustev_contract::judgment::Unresolved;
    use rustev_contract::run::{Charge, RequestResult};
    for c in CASES {
        let id = format!("SYNTHETIC.{c}");
        let status: Value = serde_json::from_slice(&out[&format!("{id}.result.json")]).unwrap();
        assert_eq!(status["status"], "judged", "{id}");
        let judgment =
            Judgment::parse(out[&format!("{id}.judgment.json")].trim_ascii_end()).unwrap();
        assert!(judgment.lineage.inputs.contains(&"ticket.message".into()));
        let record = RunRecord::parse(out[&format!("{id}.record.json")].trim_ascii_end()).unwrap();
        assert_eq!(record.requests.len(), 3, "{id}");
        for q in &record.requests {
            assert_eq!(q.result, RequestResult::Output { target: 0 }, "{id}");
            assert!(
                matches!(q.attempts[0].cost.charge, Charge::Observed { units } if units > 0),
                "{id}: {:?}",
                q.attempts[0].cost.charge
            );
        }
    }
    let status: Value = serde_json::from_slice(&out[&format!("{BLANK}.result.json")]).unwrap();
    assert_eq!(status["status"], "judged");
    let record = RunRecord::parse(out[&format!("{BLANK}.record.json")].trim_ascii_end()).unwrap();
    for q in &record.requests {
        assert!(
            matches!(
                &q.result,
                RequestResult::Failed(Unresolved::BackendUnavailable { .. })
            ),
            "{:?}",
            q.result
        );
        assert_eq!(q.attempts[0].cost.charge, Charge::Observed { units: 0 });
    }
}

#[test]
fn the_manifest_local_embed_cases_pass_through_the_rust_api() {
    if bless() {
        return;
    }
    let m = manifest().unwrap();
    let mut ran = 0;
    for c in m.cases.iter().filter(|c| c.local_embed.is_some()) {
        assert!(c.surfaces.iter().any(|s| s == "rust-api"), "{}", c.id);
        let case: LocalEmbedCase = serde_json::from_value(c.local_embed.clone().unwrap()).unwrap();
        let inputs: BTreeMap<String, Vec<u8>> = c
            .inputs
            .iter()
            .map(|i| (i.name.clone(), pinned(&i.path, &i.sha256).unwrap()))
            .collect();
        let out = local_embed::run(&case, &inputs, open(&case)).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "rustev-conformance-le-{}-{}",
            std::process::id(),
            c.id
        ));
        fs::create_dir_all(&dir).unwrap();
        for (f, b) in &out {
            fs::write(dir.join(contained(f).unwrap()), b).unwrap();
        }
        let v = compare(c, 0, b"", &dir);
        assert!(v.is_empty(), "{v:#?}\noutputs kept under {}", dir.display());
        assert_eq!(out.len(), c.outputs.len(), "{}", c.id);
        let _ = fs::remove_dir_all(&dir);
        ran += 1;
    }
    assert!(ran >= 1, "{ran} local-embed cases");
}
