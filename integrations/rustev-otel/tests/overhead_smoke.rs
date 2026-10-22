//! Spec 028, 3.14.3: a smoke run of the overhead benchmark with 100
//! decisions, so it cannot rot; every configuration built in reports zero
//! loss at this load. SYNTHETIC only.

#[path = "../examples/overhead.rs"]
mod overhead;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_benchmark_runs_and_reports_zero_loss() {
    let report = overhead::run(overhead::Load {
        decisions: 100,
        warmup: 10,
        concurrency: 4,
    })
    .await;
    let configs = report["configs"].as_array().unwrap();
    let names: Vec<&str> = configs
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        names.starts_with(&["a_memory", "b_otel_noop", "c_jsonl"]),
        "{names:?}"
    );
    if cfg!(feature = "otlp-grpc") {
        assert!(names.contains(&"d_otlp_grpc"));
    }
    if cfg!(feature = "otlp-http") {
        assert!(names.contains(&"e_otlp_http"));
    }
    for c in configs {
        assert!(overhead::lossless(c), "{c}");
        assert!(c["decision_ms"]["p50"].as_f64().unwrap() >= 0.0);
    }
    // JSONL and OTLP configurations exported bytes.
    for c in configs.iter().skip(2) {
        assert!(
            c["exported_bytes_per_decision"].as_f64().unwrap() > 0.0,
            "{c}"
        );
    }
}
