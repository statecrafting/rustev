//! The overhead benchmark (spec 028, 3.14): both reference plans with the
//! deterministic rules backend, at concurrency 4 under `backpressure`,
//! after a warm-up, with (a) an in-memory acknowledging sink, (b) the
//! OpenTelemetry sink with a no-op exporter, (c) the JSONL exporter to a
//! temporary file, and, when built with their features, (d) OTLP/gRPC and
//! (e) OTLP/HTTP to a loopback fake collector. One JSON document on
//! stdout. No pass threshold: measured, not asserted (principle XI).
//!
//! `cargo run -p rustev-otel --release --all-features --example overhead
//! [-- --decisions N --warmup N]` (defaults 10 000 and 1 000).
#![allow(dead_code)]

#[path = "../tests/collector/mod.rs"]
mod collector;
#[path = "../tests/common/mod.rs"]
mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rustev_contract::run::RunRecord;
use rustev_core::seams::{BoxFuture, EvidenceSink};
use rustev_otel::{
    DecisionTelemetry, ExportError, JsonlExporter, OtelSink, SinkConfig, TelemetryExporter,
};
use rustev_runtime::{Delivery, SinkPolicy};
use serde_json::{Value, json};

/// The load of one run.
#[derive(Debug, Clone, Copy)]
pub struct Load {
    pub decisions: usize,
    pub warmup: usize,
    pub concurrency: usize,
}

/// (a): acknowledges every record in memory.
struct Memory;

impl EvidenceSink for Memory {
    fn deliver<'a>(&'a self, _r: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("memory".to_string()) })
    }
}

/// (b): exports nothing, successfully.
struct Noop;

impl TelemetryExporter for Noop {
    fn export<'a>(&'a self, _b: &'a [DecisionTelemetry]) -> BoxFuture<'a, Result<(), ExportError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Times each `deliver`.
struct Timed {
    inner: Arc<dyn EvidenceSink>,
    nanos: Mutex<Vec<u64>>,
    on: std::sync::atomic::AtomicBool,
}

impl EvidenceSink for Timed {
    fn deliver<'a>(&'a self, r: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let t = Instant::now();
            let out = self.inner.deliver(r).await;
            if self.on.load(Ordering::SeqCst) {
                self.nanos
                    .lock()
                    .unwrap()
                    .push(t.elapsed().as_nanos() as u64);
            }
            out
        })
    }
}

/// `ps` fields of this process: CPU time in milliseconds, resident KiB.
fn ps(field: &str) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args([
            "-o",
            &format!("{field}="),
            "-p",
            &std::process::id().to_string(),
        ])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn cpu_ms() -> Option<f64> {
    // `[[dd-]hh:]mm:ss[.ff]`
    let t = ps("time")?;
    let (days, rest) = match t.split_once('-') {
        Some((d, r)) => (d.parse::<f64>().ok()?, r.to_string()),
        None => (0.0, t),
    };
    let mut secs = 0.0;
    for part in rest.split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some((days * 86_400.0 + secs) * 1000.0)
}

fn rss_kib() -> Option<u64> {
    ps("rss")?.parse().ok()
}

fn percentile(sorted: &[u64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i] as f64 / 1e6
}

fn command(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn otel_config() -> SinkConfig {
    SinkConfig {
        key: Some(common::test_key()),
        ..SinkConfig::default()
    }
}

/// One configuration's sink, the OpenTelemetry sink inside it if any, and
/// how many bytes it has exported so far.
struct Setup {
    name: &'static str,
    sink: Arc<dyn EvidenceSink>,
    otel: Option<Arc<OtelSink>>,
    bytes: Box<dyn Fn() -> u64 + Send + Sync>,
}

async fn setups(dir: &std::path::Path) -> Vec<Setup> {
    let mut out = vec![Setup {
        name: "a_memory",
        sink: Arc::new(Memory),
        otel: None,
        bytes: Box::new(|| 0),
    }];
    let noop = Arc::new(OtelSink::new(otel_config(), Arc::new(Noop)).unwrap());
    out.push(Setup {
        name: "b_otel_noop",
        sink: noop.clone(),
        otel: Some(noop),
        bytes: Box::new(|| 0),
    });
    let path = dir.join("overhead.trace.jsonl");
    let jsonl = Arc::new(
        OtelSink::new(
            otel_config(),
            Arc::new(JsonlExporter::open(&path, u64::MAX).unwrap()),
        )
        .unwrap(),
    );
    out.push(Setup {
        name: "c_jsonl",
        sink: jsonl.clone(),
        otel: Some(jsonl),
        bytes: Box::new(move || std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)),
    });
    #[cfg(feature = "otlp-grpc")]
    {
        let (addr, seen) = collector::grpc_with(true).await;
        out.push(otlp_setup(
            "d_otlp_grpc",
            addr,
            seen,
            rustev_otel::otlp::Protocol::Grpc,
        ));
    }
    #[cfg(feature = "otlp-http")]
    {
        let (addr, seen) = collector::http_with(collector::Answer::Ok, true).await;
        out.push(otlp_setup(
            "e_otlp_http",
            addr,
            seen,
            rustev_otel::otlp::Protocol::HttpProtobuf,
        ));
    }
    out
}

#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
fn otlp_setup(
    name: &'static str,
    addr: std::net::SocketAddr,
    seen: Arc<collector::Seen>,
    protocol: rustev_otel::otlp::Protocol,
) -> Setup {
    let origin = format!("http://{addr}");
    let ex = rustev_otel::otlp::OtlpExporter::new(&rustev_otel::otlp::OtlpConfig {
        protocol,
        endpoint: origin.clone(),
        allowed_endpoints: vec![origin],
        headers: vec![],
        ca_pem: None,
        timeout_ms: 10_000,
    })
    .unwrap();
    let s = Arc::new(OtelSink::new(otel_config(), Arc::new(ex)).unwrap());
    Setup {
        name,
        sink: s.clone(),
        otel: Some(s),
        bytes: Box::new(move || seen.total_bytes() as u64),
    }
}

async fn measure(setup: &Setup, load: Load) -> Value {
    let timed = Arc::new(Timed {
        inner: setup.sink.clone(),
        nanos: Mutex::new(vec![]),
        on: std::sync::atomic::AtomicBool::new(false),
    });
    let policy = SinkPolicy::Backpressure {
        max_pending: 64,
        enqueue_timeout_ms: 30_000,
        delivery_timeout_ms: 30_000,
    };
    if let Some(o) = &setup.otel {
        o.register_plan(&common::plan("support"));
        o.register_plan(&common::plan("lodging"));
    }
    let rts = Arc::new([
        common::run::runtime("support", timed.clone(), policy),
        common::run::runtime("lodging", timed.clone(), policy),
    ]);
    let next = Arc::new(AtomicU64::new(0));
    let decision_ns = Arc::new(Mutex::new(Vec::<u64>::new()));
    let failures = Arc::new(AtomicU64::new(0));
    let peak = Arc::new(AtomicU64::new(rss_kib().unwrap_or(0)));
    let total = (load.warmup + load.decisions) as u64;
    let mut cpu_start = None;
    let bytes_start = (setup.bytes)();
    let mut handles = vec![];
    for worker in 0..load.concurrency {
        let (rts, next, decision_ns, failures, peak, timed) = (
            rts.clone(),
            next.clone(),
            decision_ns.clone(),
            failures.clone(),
            peak.clone(),
            timed.clone(),
        );
        let warmup = load.warmup as u64;
        handles.push(tokio::spawn(async move {
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= total {
                    return;
                }
                if i == warmup {
                    timed.on.store(true, Ordering::SeqCst);
                }
                let (task, idx) = if i.is_multiple_of(2) {
                    ("support", 0)
                } else {
                    ("lodging", 1)
                };
                let (rt, prepared) = &rts[idx];
                let t = Instant::now();
                let r = common::run::decide(rt, prepared, task, &format!("b-{i}")).await;
                let ns = t.elapsed().as_nanos() as u64;
                match r {
                    Ok(d) => {
                        if let Delivery::Queued(ticket) = d.delivery
                            && ticket.wait().await.is_err()
                        {
                            failures.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Err(_) => {
                        failures.fetch_add(1, Ordering::SeqCst);
                    }
                }
                if i >= warmup {
                    decision_ns.lock().unwrap().push(ns);
                }
                if worker == 0 && i.is_multiple_of(250) {
                    peak.fetch_max(rss_kib().unwrap_or(0), Ordering::SeqCst);
                }
            }
        }));
        if worker == 0 {
            cpu_start = cpu_ms();
        }
    }
    for h in handles {
        h.await.unwrap();
    }
    let cpu_end = cpu_ms();
    // Let the exporters drain before reading their counters.
    if let Some(o) = &setup.otel {
        for _ in 0..10_000 {
            if o.stats().spans.in_flight == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }
    peak.fetch_max(rss_kib().unwrap_or(0), Ordering::SeqCst);
    let mut d = decision_ns.lock().unwrap().clone();
    d.sort_unstable();
    let mut v = timed.nanos.lock().unwrap().clone();
    v.sort_unstable();
    let gauges: Vec<Value> = rts
        .iter()
        .map(|(rt, _)| {
            let g = rt.gauges().sink;
            json!({"acknowledged": g.acknowledged, "failed": g.failed, "timed_out": g.timed_out, "dropped": g.dropped})
        })
        .collect();
    let otel = setup.otel.as_ref().map(|o| {
        let s = o.stats();
        json!({
            "decisions_lost": s.decisions_lost.values().sum::<u64>(),
            "spans_lost": s.spans.lost_total(),
            "spans_exported": s.spans.exported,
            "spans_in_flight": s.spans.in_flight,
            "sampled_out": s.sampled_out,
        })
    });
    let measured = load.decisions as f64;
    json!({
        "name": setup.name,
        "decision_ms": {"p50": percentile(&d, 0.5), "p95": percentile(&d, 0.95), "p99": percentile(&d, 0.99)},
        "deliver_ms": {"p50": percentile(&v, 0.5), "p99": percentile(&v, 0.99)},
        "cpu_ms_per_decision": match (cpu_start, cpu_end) {
            (Some(a), Some(b)) => json!((b - a) / measured),
            _ => json!("unavailable"),
        },
        "exported_bytes_per_decision": ((setup.bytes)() - bytes_start) as f64 / total as f64,
        "peak_rss_kib": peak.load(Ordering::SeqCst),
        "loss": {
            "decision_failures": failures.load(Ordering::SeqCst),
            "runtime_sink": gauges,
            "otel": otel,
        },
    })
}

/// True when a configuration's result shows no loss at all.
pub fn lossless(c: &Value) -> bool {
    let l = &c["loss"];
    l["decision_failures"] == 0
        && l["runtime_sink"].as_array().is_some_and(|a| {
            a.iter()
                .all(|g| g["failed"] == 0 && g["timed_out"] == 0 && g["dropped"] == 0)
        })
        && (l["otel"].is_null()
            || (l["otel"]["decisions_lost"] == 0
                && l["otel"]["spans_lost"] == 0
                && l["otel"]["spans_in_flight"] == 0))
}

/// Run every configuration built into this binary.
pub async fn run(load: Load) -> Value {
    let dir = std::env::temp_dir().join(format!("rustev-otel-overhead-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut configs = vec![];
    for s in setups(&dir).await {
        configs.push(measure(&s, load).await);
    }
    let _ = std::fs::remove_dir_all(&dir);
    json!({
        "schema": "rustev-otel.overhead/1",
        "load": {
            "decisions": load.decisions,
            "warmup": load.warmup,
            "concurrency": load.concurrency,
            "sink_policy": "backpressure{max_pending:64}",
            "plans": ["support-routing", "lodging"],
            "backend": "rustev-backend-rules",
        },
        "machine": {
            "arch": std::env::consts::ARCH,
            "os": std::env::consts::OS,
            "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            "os_release": command("uname", &["-sr"]),
        },
        "toolchain": command("rustc", &["--version"]),
        "commit": command("git", &["rev-parse", "HEAD"]),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "configs": configs,
    })
}

fn main() {
    let mut load = Load {
        decisions: 10_000,
        warmup: 1_000,
        concurrency: 4,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().and_then(|v| v.parse().ok());
        match (a.as_str(), v) {
            ("--decisions", Some(n)) => load.decisions = n,
            ("--warmup", Some(n)) => load.warmup = n,
            _ => {
                eprintln!("usage: overhead [--decisions N] [--warmup N]");
                std::process::exit(2);
            }
        }
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let report = rt.block_on(run(load));
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
