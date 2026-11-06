//! By hand on a host with the pinned artifacts (3.19.3); never declared
//! verification. Its output is evidence for a feasibility or qualification
//! record. Run with
//!
//! ```text
//! RUSTEV_EMBED_CHECKPOINT=/abs/all-MiniLM-L6-v2@1110a243 \
//! RUSTEV_EMBED_RUNTIME=/abs/libonnxruntime.1.31.0.dylib \
//! cargo test -p rustev-backend-embed-linear --test bundle --release \
//!   runtime::hand -- --ignored --nocapture
//! ```
//!
//! `RUSTEV_EMBED_CHECKPOINT` holds the checkpoint revision's
//! `onnx/model.onnx` and `tokenizer.json`; `RUSTEV_EMBED_RUNTIME` is the
//! platform's ONNX Runtime 1.31.0 library. The test assembles an `onnx`
//! bundle from them with one generated head (the encoder is real, the head
//! is not), then checks: the stage 2 reference token ids of every fixture
//! text reproduced by the pinned tokenizer (3.8.3); the runtime codes with a
//! real library (3.6.1, 3.6.2); the graph codes; every inferred fixture's
//! pooled embedding against the reference within the 3.3 bounds and bitwise
//! repeatable in process; termination of a run whose signal is raised
//! (3.12.2); and that the backend still refuses the platform (3.6.3).
use super::*;
use rustev_backend_embed_linear::EmbedSetupError;
use rustev_backend_embed_linear::onnx::OnnxEncoder;
use rustev_backend_embed_linear::request::{RequestCode, RequestPreparer};
use rustev_core::seams::CancelSignal;
use std::time::{Duration, Instant};

const HIDDEN_REAL: usize = 384;
const GRAPH_SHA256: &str = "6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452";

fn qualification(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("qualification")
        .join(name)
}

/// An `onnx` bundle over the real graph and tokenizer, in a temporary
/// directory, with `library` accepted for this platform.
fn real_bundle(checkpoint: &Path, platform: Platform, library: &[u8]) -> Bundle {
    let mut b = new_bundle();
    let graph = fs::read(checkpoint.join("onnx/model.onnx")).unwrap();
    assert_eq!(sha256_hex(&graph), GRAPH_SHA256, "the pinned graph");
    let tokenizer = fs::read(checkpoint.join("tokenizer.json")).unwrap();
    fs::create_dir_all(b.dir.join("onnx")).unwrap();
    fs::write(b.dir.join("onnx/model.onnx"), &graph).unwrap();
    fs::write(b.dir.join("tokenizer.json"), &tokenizer).unwrap();
    let weights = head_weights(7, 2, HIDDEN_REAL);
    fs::write(b.dir.join("heads/route.bin"), &weights).unwrap();
    let record = fs::read_to_string(qualification("feasibility.sha256")).unwrap();
    let mut g = file_entry("onnx/model.onnx", &graph);
    g["source"] = json!("onnx/model.onnx");
    let mut t = file_entry("tokenizer.json", &tokenizer);
    t["implementation"] = json!("tokenizers@1.0.0-rc.3");
    t["max_tokens"] = json!(256);
    t["truncation"] = json!("refuse");
    let mut h = head(
        "classify",
        "route",
        &["a", "b"],
        file_entry("heads/route.bin", &weights),
    );
    h["text"] = json!({"fields": [{"label": "", "ref": "input:text"}], "separator": ""});
    h["interpretation"] =
        json!("Generated linear scores over a real encoder; not fitted, not probabilities.");
    h["provenance"]["synthetic"] = json!(false);
    let platform = serde_json::to_value(platform).unwrap();
    b.manifest = json!({
        "schema": "rustev.embed-linear/1",
        "backend_id": "embed.minilm-hand",
        "checkpoint": {"repository": "sentence-transformers/all-MiniLM-L6-v2",
                       "revision": "1110a243fdf4706b3f48f1d95db1a4f5529b4d41",
                       "files": [{"name": "onnx/model.onnx", "sha256": GRAPH_SHA256}],
                       "license": "apache-2.0", "base_model": "nreimers/MiniLM-L6-H384-uncased"},
        "encoder": {"kind": "onnx", "graph": g,
                    "inputs": ["input_ids", "attention_mask", "token_type_ids"],
                    "output": "last_hidden_state", "hidden_size": HIDDEN_REAL, "precision": "f32"},
        "tokenizer": t,
        "pooling": {"mode": "mean_masked", "normalize": "l2"},
        "runtime": {"onnxruntime": "1.31.0", "ort": ORT_VERSION, "execution_provider": "cpu",
                    "graph_optimization": "all", "intra_op_threads": 1, "inter_op_threads": 1,
                    "libraries": [{"platform": platform, "sha256": sha256_hex(library)}]},
        "heads": [h],
        "limits": {"max_projection_bytes": 65536, "max_text_bytes": 4096},
        "cost": {"units_per_call": 1, "units_per_token": 2},
        "tolerance": {"abs_logit": "0.00001", "rel_logit": "0.0001",
                      "decision_margin": "0.001", "feasibility_record": record.trim()}
    });
    b.write();
    b
}

fn projection(text: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"operation": "classify", "task": "route",
        "question": "Which route?", "options": ["a", "b"], "values": {"input:text": text}}))
    .unwrap()
}

fn err(r: Result<impl Sized, EmbedSetupError>) -> SetupCode {
    r.err().expect("refused").code
}

#[test]
#[ignore = "by hand: needs RUSTEV_EMBED_CHECKPOINT and RUSTEV_EMBED_RUNTIME (3.19.3)"]
fn hand_real_encoder() {
    let checkpoint = PathBuf::from(std::env::var("RUSTEV_EMBED_CHECKPOINT").unwrap());
    let library_path = PathBuf::from(std::env::var("RUSTEV_EMBED_RUNTIME").unwrap());
    let platform = platform_key().expect("a platform key");
    let library = fs::read(&library_path).unwrap();
    let b = real_bundle(&checkpoint, platform, &library);
    let bundle = b.verify().unwrap();
    let manifest = bundle.manifest.clone();

    // 3.8.3: the stage 2 reference token ids, through the backend's own
    // preparation (one field, no label, so the text is the fixture's).
    let reference: Value =
        serde_json::from_slice(&fs::read(qualification("reference.json")).unwrap()).unwrap();
    let preparer = RequestPreparer::new(bundle.clone()).unwrap();
    let mut inferred = Vec::new();
    for f in reference["fixtures"].as_array().unwrap() {
        let text = f["text"].as_str().unwrap();
        let expected: Vec<u32> = serde_json::from_value(f["token_ids"].clone()).unwrap();
        match preparer.prepare(&projection(text)) {
            Ok(r) => {
                assert_eq!(r.ids, expected, "{text:?}");
                inferred.push((r, f["reference_embedding"].clone()));
            }
            Err(e) if text.trim().is_empty() => assert_eq!(e.code, RequestCode::TextEmpty),
            Err(e) => {
                assert_eq!(e.code, RequestCode::TooManyTokens, "{text:?}");
                assert_eq!(expected.len(), 257);
            }
        }
    }
    assert_eq!(inferred.len(), 19, "the record's 19 inferred fixtures");

    // 3.6.1: a manifest naming another 1.x version refuses before loading.
    let mut older = manifest.clone();
    older.runtime.onnxruntime = "1.30.0".into();
    assert_eq!(
        err(runtime::load(Some(&library_path), &older, platform)),
        SetupCode::RuntimeVersionMismatch
    );
    let start = Instant::now();
    let environment = runtime::load(Some(&library_path), &manifest, platform).unwrap();
    // 3.6.2: an equal digest reuses the runtime; another digest conflicts;
    // the reused runtime still answers for its version.
    runtime::load(Some(&library_path), &manifest, platform).unwrap();
    assert_eq!(
        err(runtime::load(Some(&library_path), &older, platform)),
        SetupCode::RuntimeVersionMismatch
    );
    let mut altered = library.clone();
    altered.push(0);
    let other = b.dir.join("other-runtime");
    fs::write(&other, &altered).unwrap();
    let mut conflicting = manifest.clone();
    conflicting.runtime.libraries[0].sha256 = sha256_hex(&altered);
    assert_eq!(
        err(runtime::load(Some(&other), &conflicting, platform)),
        SetupCode::RuntimeConflict
    );

    // 3.7.2: graph codes.
    assert_eq!(
        err(OnnxEncoder::new(&environment, b"not a graph", &manifest)),
        SetupCode::GraphRejected
    );
    for edit in [
        |m: &mut EmbedManifest| m.encoder.output = "pooler_output".into(),
        |m: &mut EmbedManifest| m.encoder.inputs.truncate(2),
        |m: &mut EmbedManifest| m.encoder.hidden_size = 383,
    ] {
        let mut m = manifest.clone();
        edit(&mut m);
        assert_eq!(
            err(OnnxEncoder::new(&environment, &bundle.graph, &m)),
            SetupCode::GraphSignatureMismatch
        );
    }
    let encoder = OnnxEncoder::new(&environment, &bundle.graph, &manifest).unwrap();
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0;

    // 3.9.1, 3.9.2 and 3.3: the pooled embedding against the reference.
    let never = CancelSignal::new();
    let mut max_abs = 0.0_f64;
    for (request, expected) in &inferred {
        let vectors = encoder
            .encode(&request.ids, &request.attention_mask, &never)
            .unwrap();
        let again = encoder
            .encode(&request.ids, &request.attention_mask, &never)
            .unwrap();
        assert!(
            vectors
                .iter()
                .zip(&again)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "bitwise repeatable in process"
        );
        let pooled = pool(
            &vectors,
            &request.attention_mask,
            HIDDEN_REAL,
            Normalize::L2,
        )
        .unwrap();
        let expected = expected.as_array().unwrap();
        assert_eq!(expected.len(), HIDDEN_REAL);
        for (&a, b) in pooled.iter().zip(expected) {
            let (a, b) = (f64::from(a), b.as_f64().unwrap());
            let d = (a - b).abs();
            max_abs = max_abs.max(d);
            assert!(d <= 0.00001 || d <= 0.0001 * a.abs().max(b.abs()), "{d}");
        }
        bundle.heads[0].logits(&pooled).unwrap();
    }

    // 3.12.2: a raised signal stops a run before it starts, and a run in
    // progress is terminated through its run options.
    let long = &inferred.iter().max_by_key(|(r, _)| r.ids.len()).unwrap().0;
    let raised = CancelSignal::new();
    raised.raise();
    assert!(
        encoder
            .encode(&long.ids, &long.attention_mask, &raised)
            .is_err()
    );
    let max_ids: Vec<u32> = (0..256)
        .map(|i| {
            if i == 0 {
                101
            } else if i == 255 {
                102
            } else {
                7592
            }
        })
        .collect();
    let signal = CancelSignal::new();
    let raiser = signal.clone();
    let timer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2));
        raiser.raise();
    });
    let started = Instant::now();
    let terminated = encoder.encode(&max_ids, &vec![1; 256], &signal);
    let terminated_ms = started.elapsed().as_secs_f64() * 1000.0;
    timer.join().unwrap();
    assert!(terminated.is_err(), "the run was not terminated");
    let started = Instant::now();
    encoder.encode(&max_ids, &vec![1; 256], &never).unwrap();
    let full_ms = started.elapsed().as_secs_f64() * 1000.0;

    // 3.6.3: the backend refuses the platform until the record qualifies it.
    let e = rustev_backend_embed_linear::EmbedLinearBackend::open(
        rustev_backend_embed_linear::EmbedConfig {
            bundle_dir: b.dir.clone(),
            runtime_path: Some(library_path.clone()),
            expected_artifact: b.artifact(),
            workers: 1,
            max_queued: 0,
            embedding_cache: None,
        },
    )
    .err()
    .unwrap();
    assert_eq!(e.code, SetupCode::UnqualifiedPlatform);
    eprintln!(
        "{}",
        json!({"platform": platform, "runtime_sha256": sha256_hex(&library),
               "fixtures_inferred": inferred.len(), "max_abs_difference": max_abs.to_string(),
               "runtime_and_session_ms": cold_ms.to_string(),
               "terminated_run_ms": terminated_ms.to_string(),
               "full_256_token_run_ms": full_ms.to_string()})
    );
}

/// Wait for a future on this thread, parking between polls.
fn block_on<T>(mut f: rustev_core::seams::BoxFuture<'_, T>) -> T {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

/// Peak resident set of this process from `/proc/self/status` (`VmHWM`, in
/// bytes), where the kernel reports it.
fn vm_hwm_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// Stage 4 of the complete backend (3.2.2, 3.3, 3.13) by hand, on a host
/// with the pinned artifacts: run alone, in a fresh process, with the
/// platform in the qualified table. Every number goes through
/// `EmbedLinearBackend` and `DecisionBackend::infer`: cold setup is `open`
/// to ready with the runtime library already on disk; the 19 inferred
/// fixtures' logits, each twice, against the head applied to the stage 2
/// reference embedding within the 3.3 bounds; then one worker, the
/// 256-token fixture, 50 warm-ups and 1,000 timed requests one at a time,
/// every logit bitwise equal to the first. `RUSTEV_EMBED_STAGE4_OUT` names
/// the JSON written. Peak RSS is the process's, collected by its parent.
#[test]
#[ignore = "by hand: needs RUSTEV_EMBED_CHECKPOINT, RUSTEV_EMBED_RUNTIME, RUSTEV_EMBED_STAGE4_OUT and a qualified platform"]
fn hand_complete_backend() {
    use rustev_backend_embed_linear::{EmbedConfig, EmbedLinearBackend};
    use rustev_contract::output::RawOutput;
    use rustev_contract::time::DurationMs;
    use rustev_core::seams::{AttemptCall, CallContext, CancelSignal, DecisionBackend};

    let checkpoint = PathBuf::from(std::env::var("RUSTEV_EMBED_CHECKPOINT").unwrap());
    let library_path = PathBuf::from(std::env::var("RUSTEV_EMBED_RUNTIME").unwrap());
    let out = PathBuf::from(std::env::var("RUSTEV_EMBED_STAGE4_OUT").unwrap());
    let platform = platform_key().expect("a platform key");
    let library = fs::read(&library_path).unwrap();
    let b = real_bundle(&checkpoint, platform, &library);
    drop(library);
    let bundle = b.verify().unwrap();
    let mut bundle_files = vec![("manifest.json".to_string(), 0_u64)];
    bundle_files.extend(
        bundle
            .manifest
            .files()
            .into_iter()
            .map(|f| (f.file.clone(), f.bytes)),
    );
    bundle_files[0].1 = fs::metadata(b.dir.join("manifest.json")).unwrap().len();
    let bundle_bytes: u64 = bundle_files.iter().map(|(_, n)| n).sum();
    let head = &bundle.heads[0];
    let options = bundle.manifest.heads[0].options.clone();

    let reference: Value =
        serde_json::from_slice(&fs::read(qualification("reference.json")).unwrap()).unwrap();
    let fixtures: Vec<(String, Vec<f32>, usize)> = reference["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["reference_embedding"].is_array())
        .map(|f| {
            let e = f["reference_embedding"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            let n = f["token_ids"].as_array().unwrap().len();
            (f["text"].as_str().unwrap().to_string(), e, n)
        })
        .collect();
    assert_eq!(fixtures.len(), 19, "the record's 19 inferred fixtures");
    drop(bundle);

    let start = Instant::now();
    let backend = EmbedLinearBackend::open(EmbedConfig {
        bundle_dir: b.dir.clone(),
        runtime_path: Some(library_path.clone()),
        expected_artifact: b.artifact(),
        workers: 1,
        max_queued: 0,
        embedding_cache: None,
    })
    .expect("the platform must be in the measurement build's table");
    let cold_setup_ms = start.elapsed().as_secs_f64() * 1000.0;
    let rss_after_setup = vm_hwm_bytes();

    let cx = CallContext {
        remaining_ms: DurationMs(60_000),
        trace_id: "stage4".into(),
        principal_handle: vec![],
    };
    let infer = |p: &[u8]| -> Vec<f64> {
        let report = block_on(backend.infer(AttemptCall {
            projection: p,
            attempt_id: "stage4",
            cancel: CancelSignal::new(),
            cx: &cx,
        }));
        match report.result {
            Ok(RawOutput::Logits(map)) => options.iter().map(|o| map[o]).collect(),
            other => panic!("not logits: {other:?}"),
        }
    };

    // 3.13.2: logits against the head over the reference embedding, and run
    // to run in this process.
    let mut max_abs = 0.0_f64;
    let mut agree = true;
    let mut rows = Vec::new();
    for (text, embedding, tokens) in &fixtures {
        let p = projection(text);
        let first = infer(&p);
        let second = infer(&p);
        let bitwise = first
            .iter()
            .zip(&second)
            .all(|(a, b)| a.to_bits() == b.to_bits());
        let expected = head.logits(embedding).unwrap();
        let within = first.iter().zip(&expected).all(|(a, b)| {
            let d = (a - b).abs();
            max_abs = max_abs.max(d);
            a.is_finite() && (d <= 0.00001 || d <= 0.0001 * a.abs().max(b.abs()))
        });
        agree &= within && bitwise;
        rows.push(json!({
            "tokens": tokens,
            "text_sha256": sha256_hex(text.as_bytes()),
            "logits": first.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
            "reference_head_logits": expected.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
            "within_bounds": within,
            "bitwise_repeatable": bitwise,
        }));
    }

    // 3.3 warm latency: the 256-token fixture, one request at a time.
    let (long, _, n) = fixtures.iter().max_by_key(|(_, _, n)| *n).unwrap();
    assert_eq!(*n, 256, "a max_tokens input");
    let p = projection(long);
    let baseline = infer(&p);
    for _ in 0..50 {
        infer(&p);
    }
    let mut durations = Vec::with_capacity(1000);
    let mut warm_bitwise = true;
    for _ in 0..1000 {
        let t = Instant::now();
        let logits = infer(&p);
        durations.push(t.elapsed().as_secs_f64() * 1000.0);
        warm_bitwise &= logits
            .iter()
            .zip(&baseline)
            .all(|(a, b)| a.to_bits() == b.to_bits());
    }
    let mut sorted = durations.clone();
    sorted.sort_by(f64::total_cmp);
    // Nearest rank, as the encoder probe computes it.
    let rank = |q: f64| sorted[((q * 1000.0_f64).ceil() as usize).max(1) - 1];
    let result = json!({
        "schema": "rustev.embed-stage4-hand/1",
        "platform": platform,
        "runtime_sha256": sha256_hex(&fs::read(&library_path).unwrap()),
        "artifact": b.artifact().to_string(),
        "bundle_files": bundle_files.iter().map(|(n, s)| json!({"name": n, "bytes": s})).collect::<Vec<_>>(),
        "bundle_bytes": bundle_bytes,
        "workers": 1, "max_queued": 0, "embedding_cache": "off",
        "intra_op_threads": 1, "inter_op_threads": 1, "graph_optimization": "all",
        "cold_setup_ms": format!("{cold_setup_ms}"),
        "fixtures": rows,
        "fixtures_agree_and_repeatable": agree,
        "max_abs_logit_difference_from_reference_head": format!("{max_abs:?}"),
        "warm_tokens": n, "warm_ups": 50, "timed_requests": 1000,
        "warm_bitwise_equal_to_first": warm_bitwise,
        "p50_ms": format!("{}", rank(0.50)),
        "p95_ms": format!("{}", rank(0.95)),
        "p99_ms": format!("{}", rank(0.99)),
        "max_ms": format!("{}", sorted[999]),
        "warm_request_ms": durations.iter().map(|d| format!("{d}")).collect::<Vec<_>>(),
        "vm_hwm_after_setup_bytes": rss_after_setup,
        "vm_hwm_end_bytes": vm_hwm_bytes(),
    });
    fs::write(&out, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    assert!(agree && warm_bitwise, "numerics or repeatability failed");
}
