//! Spec 028, 3.6 to 3.9.4 and the acceptance's sink, trace-context and
//! sampling items: acknowledgement modes under every spec 003 sink policy,
//! `TeeSink`, conservation with succeeding, failing, hanging and panicking
//! exporters, the context table, the join buffer, sampling, metrics and
//! logs, and the equivalence of 3.8.6. SYNTHETIC only.

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::run::{decide, runtime};
use common::*;
use rustev_contract::Document;
use rustev_contract::execution::FailureClass;
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::redaction::{FieldOverride, RedactionPolicy};
use rustev_contract::run::{CoreEvidence, RunRecord, Termination, Transition};
use rustev_core::seams::{BoxFuture, EvidenceSink};
use rustev_otel::metrics::{MetricsSnapshot, name};
use rustev_otel::sink::{loss, ratio_sampled};
use rustev_otel::{
    AckMode, AlwaysSampleOn, BeginError, ConfigError, DecisionTelemetry, ExportError, OtelSink,
    ParentSampling, RootSampling, SeededIds, SinkClock, SinkConfig, TeeSink, TelemetryExporter,
};
use rustev_runtime::{DecideError, Delivery, SinkPolicy};

#[derive(Clone, Copy, PartialEq)]
enum Behavior {
    Succeed,
    Fail,
    Hang,
    Panic,
}

struct TestExporter {
    behavior: Mutex<Behavior>,
    trees: Mutex<Vec<DecisionTelemetry>>,
    metrics: Mutex<Vec<MetricsSnapshot>>,
    gate: tokio::sync::Semaphore,
    gated: bool,
    entered: AtomicU64,
}

impl TestExporter {
    fn new(b: Behavior) -> Arc<Self> {
        Arc::new(TestExporter {
            behavior: Mutex::new(b),
            trees: Mutex::new(vec![]),
            metrics: Mutex::new(vec![]),
            gate: tokio::sync::Semaphore::new(0),
            gated: false,
            entered: AtomicU64::new(0),
        })
    }

    fn gated() -> Arc<Self> {
        Arc::new(TestExporter {
            behavior: Mutex::new(Behavior::Succeed),
            trees: Mutex::new(vec![]),
            metrics: Mutex::new(vec![]),
            gate: tokio::sync::Semaphore::new(0),
            gated: true,
            entered: AtomicU64::new(0),
        })
    }

    fn set(&self, b: Behavior) {
        *self.behavior.lock().unwrap() = b;
    }

    fn trees(&self) -> Vec<DecisionTelemetry> {
        self.trees.lock().unwrap().clone()
    }
}

impl TelemetryExporter for TestExporter {
    fn export<'a>(
        &'a self,
        batch: &'a [DecisionTelemetry],
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        Box::pin(async move {
            if self.gated {
                self.entered.fetch_add(1, Ordering::SeqCst);
                self.gate.acquire().await.unwrap().forget();
            }
            let b = *self.behavior.lock().unwrap();
            match b {
                Behavior::Succeed => {
                    self.trees.lock().unwrap().extend(batch.iter().cloned());
                    Ok(())
                }
                Behavior::Fail => Err(ExportError::Failed("refused".into())),
                Behavior::Hang => std::future::pending().await,
                Behavior::Panic => panic!("exporter panicked on purpose"),
            }
        })
    }

    fn export_metrics<'a>(
        &'a self,
        s: &'a MetricsSnapshot,
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        Box::pin(async move {
            match *self.behavior.lock().unwrap() {
                Behavior::Succeed => {
                    self.metrics.lock().unwrap().push(s.clone());
                    Ok(())
                }
                _ => Err(ExportError::Failed("refused".into())),
            }
        })
    }

    fn export_logs<'a>(
        &'a self,
        _logs: &'a [rustev_otel::LogRecord],
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        Box::pin(async move {
            match *self.behavior.lock().unwrap() {
                Behavior::Succeed => Ok(()),
                _ => Err(ExportError::Failed("refused".into())),
            }
        })
    }

    fn takes_logs(&self) -> bool {
        true
    }

    fn takes_metrics(&self) -> bool {
        true
    }
}

/// A manual clock for the tables' time to live.
#[derive(Default)]
struct Clock {
    ms: AtomicU64,
}

impl SinkClock for Clock {
    fn now_unix_nanos(&self) -> u64 {
        ANCHOR_NANOS + self.ms.load(Ordering::SeqCst) * 1_000_000
    }

    fn monotonic_ms(&self) -> u64 {
        self.ms.load(Ordering::SeqCst)
    }
}

fn config() -> SinkConfig {
    SinkConfig {
        key: Some(test_key()),
        export_timeout_ms: 200,
        ..SinkConfig::default()
    }
}

fn sink(c: SinkConfig, ex: Arc<TestExporter>) -> OtelSink {
    OtelSink::with_parts(
        c,
        ex,
        Arc::new(Clock::default()),
        Box::new(SeededIds::new(b"t")),
    )
    .unwrap()
}

fn sink_with_clock(c: SinkConfig, ex: Arc<TestExporter>, clock: Arc<Clock>) -> OtelSink {
    OtelSink::with_parts(c, ex, clock, Box::new(SeededIds::new(b"t"))).unwrap()
}

/// Wait until the gated exporter holds `n` exports.
async fn entered(ex: &TestExporter, n: u64) {
    for _ in 0..1000 {
        if ex.entered.load(Ordering::SeqCst) >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("the exporter was never entered");
}

async fn settle(s: &OtelSink) {
    for _ in 0..1000 {
        let st = s.stats();
        if st.spans.in_flight == 0 && st.logs.in_flight == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("telemetry still in flight: {:?}", s.stats());
}

fn conserved(s: &OtelSink) {
    let st = s.stats();
    assert!(st.spans.conserved(), "{st:?}");
    assert!(st.logs.conserved(), "{st:?}");
    assert!(st.metric_exports.conserved(), "{st:?}");
}

fn digest(r: &RunRecord) -> String {
    r.record_digest().unwrap().to_string()
}

fn unresolved(mut r: RunRecord) -> RunRecord {
    if let CoreEvidence::Recorded(e) = &mut r.core {
        e.judgment.outcome = Outcome::Unresolved(Unresolved::missing(["ticket.message".into()]));
    }
    r
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_mode_receipts_name_the_record_digest_and_export() {
    let ex = TestExporter::new(Behavior::Succeed);
    let s = sink(config(), ex.clone());
    let rec = record("support");
    let receipt = s.deliver(&rec).await.unwrap();
    assert_eq!(receipt, format!("otel:accepted:{}", digest(&rec)));
    settle(&s).await;
    let st = s.stats();
    assert_eq!(st.spans.accepted, 12);
    assert_eq!(st.spans.exported, 12);
    assert_eq!(st.plan_unregistered, 1);
    conserved(&s);
    assert_eq!(ex.trees().len(), 1);
    assert!(ex.trees()[0].sampled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exported_mode_resolves_after_export_and_errors_on_failure() {
    let ex = TestExporter::new(Behavior::Succeed);
    let c = SinkConfig {
        ack: AckMode::Exported,
        ..config()
    };
    let s = sink(c, ex.clone());
    let rec = record("lodging");
    assert_eq!(
        s.deliver(&rec).await.unwrap(),
        format!("otel:exported:{}", digest(&rec))
    );
    assert_eq!(ex.trees().len(), 1);
    ex.set(Behavior::Fail);
    let e = s.deliver(&rec).await.unwrap_err();
    assert!(e.contains(loss::EXPORT_FAILED), "{e}");
    let st = s.stats();
    assert_eq!(st.spans.lost[loss::EXPORT_FAILED], 27);
    assert_eq!(st.decisions_lost[loss::EXPORT_FAILED], 1);
    conserved(&s);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_hanging_and_panicking_exporters_are_counted_and_conserved() {
    for (b, reason) in [
        (Behavior::Fail, loss::EXPORT_FAILED),
        (Behavior::Hang, loss::EXPORT_TIMEOUT),
        (Behavior::Panic, loss::EXPORTER_PANICKED),
    ] {
        let ex = TestExporter::new(b);
        let s = sink(config(), ex.clone());
        let rec = record("support");
        for _ in 0..3 {
            s.deliver(&rec).await.unwrap();
        }
        settle(&s).await;
        let st = s.stats();
        assert_eq!(st.spans.accepted, 36);
        assert_eq!(st.spans.exported, 0);
        assert_eq!(st.spans.lost.get(reason), Some(&36), "{reason}: {st:?}");
        assert_eq!(st.spans.lost.len(), 1);
        conserved(&s);
        // The worker survives: a succeeding export afterwards is counted.
        ex.set(Behavior::Succeed);
        s.deliver(&rec).await.unwrap();
        settle(&s).await;
        assert_eq!(s.stats().spans.exported, 12, "{reason}");
        conserved(&s);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_queue_refuses_under_accepted_and_counts_queue_full() {
    let ex = TestExporter::gated();
    let c = SinkConfig {
        max_queued_spans: 30,
        export_timeout_ms: 10_000,
        ..config()
    };
    let s = sink(c, ex.clone());
    let rec = record("support");
    // The first tree goes to the (gated) exporter; two more fill 24 of 30.
    s.deliver(&rec).await.unwrap();
    entered(&ex, 1).await;
    s.deliver(&rec).await.unwrap();
    s.deliver(&rec).await.unwrap();
    let e = s.deliver(&rec).await.unwrap_err();
    assert!(e.contains(loss::QUEUE_FULL), "{e}");
    let st = s.stats();
    assert_eq!(st.spans.lost[loss::QUEUE_FULL], 12);
    assert_eq!(st.spans.in_flight, 36);
    conserved(&s);
    ex.gate.add_permits(10);
    settle(&s).await;
    assert_eq!(s.stats().spans.exported, 36);
    conserved(&s);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_counts_what_it_could_not_flush() {
    let ex = TestExporter::gated();
    let c = SinkConfig {
        export_timeout_ms: 10_000,
        ..config()
    };
    let s = sink(c, ex.clone());
    let rec = record("support");
    for _ in 0..3 {
        s.deliver(&rec).await.unwrap();
    }
    entered(&ex, 1).await;
    s.shutdown(50).await;
    let st = s.stats();
    // One tree is in the exporter; two were still queued.
    assert_eq!(st.spans.lost[loss::SHUTDOWN_UNFLUSHED], 24);
    ex.gate.add_permits(10);
    settle(&s).await;
    assert_eq!(s.stats().spans.exported, 12);
    assert!(s.deliver(&rec).await.is_err());
    conserved(&s);
}

/// Spec 003's sink policies apply unchanged (3.8.3): both modes under
/// fail_decision, backpressure and drop_counted, through the runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_modes_under_every_runtime_sink_policy() {
    for ack in [AckMode::Accepted, AckMode::Exported] {
        for policy in [
            SinkPolicy::FailDecision { timeout_ms: 2_000 },
            SinkPolicy::Backpressure {
                max_pending: 4,
                enqueue_timeout_ms: 1_000,
                delivery_timeout_ms: 2_000,
            },
            SinkPolicy::DropCounted {
                max_pending: 4,
                delivery_timeout_ms: 2_000,
            },
        ] {
            let ex = TestExporter::new(Behavior::Succeed);
            let s = Arc::new(sink(SinkConfig { ack, ..config() }, ex.clone()));
            s.register_plan(&plan("support"));
            let (rt, prepared) = runtime("support", s.clone(), policy);
            let d = decide(&rt, &prepared, "support", "rt-1").await.unwrap();
            let receipt = match d.delivery {
                Delivery::Acknowledged { receipt } => receipt,
                Delivery::Queued(t) => t.wait().await.unwrap(),
                Delivery::Dropped { .. } => panic!("dropped with room"),
            };
            let mode = match ack {
                AckMode::Accepted => "accepted",
                AckMode::Exported => "exported",
            };
            assert_eq!(receipt, format!("otel:{mode}:{}", digest(&d.record)));
            settle(&s).await;
            assert_eq!(s.stats().plan_unregistered, 0);
            assert_eq!(ex.trees().len(), 1);
            conserved(&s);

            // A collector outage: under fail_decision in exported mode the
            // decision is evidence_not_delivered, the host's explicit choice.
            ex.set(Behavior::Fail);
            let r = decide(&rt, &prepared, "support", "rt-2").await;
            match (&policy, ack) {
                (SinkPolicy::FailDecision { .. }, AckMode::Exported) => {
                    assert!(matches!(r, Err(DecideError::EvidenceNotDelivered { .. })))
                }
                (SinkPolicy::FailDecision { .. }, AckMode::Accepted) => assert!(r.is_ok()),
                _ => {
                    let d = r.unwrap();
                    if let Delivery::Queued(t) = d.delivery {
                        assert_eq!(t.wait().await.is_ok(), ack == AckMode::Accepted);
                    }
                }
            }
            settle(&s).await;
            assert_eq!(s.stats().spans.lost[loss::EXPORT_FAILED], 12);
            conserved(&s);
        }
    }
}

/// A file sink as spec 006's: the record canonical bytes, receipt
/// `file:<digest>`.
struct FileSink {
    bytes: Mutex<Vec<Vec<u8>>>,
}

impl EvidenceSink for FileSink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        let r = record
            .record_canonical()
            .map_err(|e| e.to_string())
            .map(|b| self.bytes.lock().unwrap().push(b))
            .and_then(|()| {
                record
                    .record_digest()
                    .map(|d| format!("file:{d}"))
                    .map_err(|e| e.to_string())
            });
        Box::pin(async move { r })
    }
}

fn file_sink() -> Arc<FileSink> {
    Arc::new(FileSink {
        bytes: Mutex::new(vec![]),
    })
}

/// 3.8.6: telemetry attached versus not, identical run record and judgment
/// bytes for both reference decisions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attaching_the_sink_changes_no_record_or_judgment_byte() {
    for task in ["support", "lodging"] {
        let alone = file_sink();
        let (rt, prepared) = runtime(
            task,
            alone.clone(),
            SinkPolicy::FailDecision { timeout_ms: 2_000 },
        );
        let a = decide(&rt, &prepared, task, "eq-1").await.unwrap();

        let teed = file_sink();
        let ex = TestExporter::new(Behavior::Succeed);
        let otel = Arc::new(sink(config(), ex.clone()));
        otel.register_plan(&plan(task));
        let tee = Arc::new(TeeSink {
            primary: teed.clone(),
            telemetry: otel.clone(),
            require_telemetry: false,
            telemetry_timeout_ms: 1_000,
        });
        let (rt, prepared) = runtime(task, tee, SinkPolicy::FailDecision { timeout_ms: 2_000 });
        let b = decide(&rt, &prepared, task, "eq-1").await.unwrap();
        settle(&otel).await;
        assert_eq!(ex.trees().len(), 1, "{task}");

        let norm = |r: &RunRecord| {
            let mut r = r.clone();
            r.timing.queued_ms = 0;
            r.timing.elapsed_ms = 0;
            for q in &mut r.requests {
                for a in &mut q.attempts {
                    a.dispatched_ms = 0;
                    a.ended_ms = 0;
                }
            }
            r.record_canonical().unwrap()
        };
        assert_eq!(norm(&a.record), norm(&b.record), "{task}");
        let (rustev_runtime::Completion::Judged(ja), rustev_runtime::Completion::Judged(jb)) =
            (&a.completion, &b.completion)
        else {
            panic!("judged")
        };
        assert_eq!(
            ja.record_canonical().unwrap(),
            jb.record_canonical().unwrap(),
            "{task}"
        );
        // The bytes each file sink received are the delivered records.
        assert_eq!(
            alone.bytes.lock().unwrap()[0],
            a.record.record_canonical().unwrap()
        );
        assert_eq!(
            teed.bytes.lock().unwrap()[0],
            b.record.record_canonical().unwrap()
        );
        assert!(
            matches!(&b.delivery, Delivery::Acknowledged { receipt } if receipt.starts_with("file:"))
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tee_keeps_the_primary_receipt_and_counts_or_fails_telemetry() {
    let rec = record("support");
    for require in [false, true] {
        let primary = file_sink();
        let ex = TestExporter::new(Behavior::Fail);
        let otel = Arc::new(sink(
            SinkConfig {
                ack: AckMode::Exported,
                ..config()
            },
            ex,
        ));
        let tee = TeeSink {
            primary: primary.clone(),
            telemetry: otel.clone(),
            require_telemetry: require,
            telemetry_timeout_ms: 1_000,
        };
        let r = tee.deliver(&rec).await;
        if require {
            assert!(r.unwrap_err().contains(loss::EXPORT_FAILED));
        } else {
            assert_eq!(r.unwrap(), format!("file:{}", digest(&rec)));
        }
        settle(&otel).await;
        let st = otel.stats();
        // Counted once, under the reason that lost it.
        assert_eq!(st.decisions_lost.get(loss::EXPORT_FAILED), Some(&1));
        assert_eq!(st.decisions_lost.get(loss::TEE_FAILED), None);
        conserved(&otel);
    }
    // A projection failure is counted as `projection_unknown`, so the tee
    // does not count it again.
    let primary = file_sink();
    let otel = Arc::new(sink(config(), TestExporter::new(Behavior::Succeed)));
    let tee = TeeSink {
        primary,
        telemetry: otel.clone(),
        require_telemetry: false,
        telemetry_timeout_ms: 1_000,
    };
    let mut bad = record("support");
    bad.schema = "rustev.run/3".into();
    assert!(tee.deliver(&bad).await.is_ok());
    assert_eq!(
        otel.stats().decisions_lost.get(loss::PROJECTION_UNKNOWN),
        Some(&1)
    );
    assert_eq!(otel.stats().decisions_lost.get(loss::TEE_FAILED), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trace_context_parenting_expiry_abandon_and_a_full_table() {
    let clock = Arc::new(Clock::default());
    let ex = TestExporter::new(Behavior::Succeed);
    let c = SinkConfig {
        max_pending_contexts: 2,
        context_ttl_ms: 1_000,
        ..config()
    };
    let s = sink_with_clock(c, ex.clone(), clock.clone());
    let parent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
    let mut rec = record("support");
    rec.decision_id = "d-valid".into();
    let begun = s.begin("d-valid", Some(parent), Some("vendor=x")).unwrap();
    assert_eq!(begun.trace_id, "0af7651916cd43dd8448eb211c80319c");
    s.deliver(&rec).await.unwrap();
    settle(&s).await;
    let t = &ex.trees()[0];
    let root = &t.lines[0];
    assert_eq!(root.trace_id, begun.trace_id);
    assert_eq!(root.span_id, begun.root_span_id);
    assert_eq!(root.parent_span_id.as_deref(), Some("b7ad6b7169203331"));
    assert_eq!(t.tracestate.as_deref(), Some("vendor=x"));

    // Malformed: a new trace, counted, the decision unaffected.
    let b = s
        .begin("d-bad", Some("00-zz-01"), Some("vendor=y"))
        .unwrap();
    assert_ne!(b.trace_id, begun.trace_id);
    assert_eq!(s.stats().invalid_traceparent, 1);
    // Full table (d-bad and d-other), then abandon frees a slot.
    s.begin("d-other", None, None).unwrap();
    assert_eq!(s.begin("d-third", None, None), Err(BeginError::TableFull));
    assert_eq!(s.stats().table_full, 1);
    assert!(s.abandon("d-other"));
    assert!(!s.abandon("d-other"));
    s.begin("d-third", None, None).unwrap();
    // Expiry by the monotonic clock.
    clock.ms.store(1_000, Ordering::SeqCst);
    let mut rec = record("support");
    rec.decision_id = "d-bad".into();
    s.deliver(&rec).await.unwrap();
    settle(&s).await;
    assert_eq!(s.stats().context_expired, 2);
    let t = &ex.trees()[1];
    assert_ne!(t.lines[0].trace_id, b.trace_id);
    assert!(t.tracestate.is_none());
    assert!(t.lines[0].parent_span_id.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exchange_records_join_once_and_late_ones_are_unjoined() {
    let ex = TestExporter::new(Behavior::Succeed);
    let c = SinkConfig {
        bindings: remote_bindings(),
        join_buffer: 2,
        ..config()
    };
    let s = sink(c, ex.clone());
    s.register_plan(&plan("support"));
    let xs: Vec<_> = exchanges().into_values().collect();
    let mut seen = BTreeSet::new();
    let distinct: Vec<_> = xs
        .into_iter()
        .filter(|x| seen.insert(x.members[0].attempt_id.clone()))
        .collect();
    s.record_exchange(&distinct[0]);
    s.record_exchange(&distinct[1]);
    // The buffer holds two records; a third is refused.
    s.record_exchange(&distinct[2]);
    assert_eq!(s.stats().exchange_unjoined, 1);
    s.deliver(&record("support")).await.unwrap();
    settle(&s).await;
    let t = &ex.trees()[0];
    let joined = t
        .lines
        .iter()
        .filter(|l| l.attributes.contains_key("rustev.served.disclosure"))
        .filter(|l| l.attributes.contains_key("rustev.exchange.status"))
        .count();
    assert_eq!(joined, 2);
    // After its decision was exported, a record is unjoined at once.
    s.record_exchange(&distinct[2]);
    assert_eq!(s.stats().exchange_unjoined, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sampling_modes_and_triggers_with_metrics_for_sampled_out_decisions() {
    let rec = record("support");
    let run = |c: SinkConfig, rec: RunRecord, parent: Option<&'static str>| async move {
        let ex = TestExporter::new(Behavior::Succeed);
        let s = sink(c, ex.clone());
        if let Some(p) = parent {
            s.begin(&rec.decision_id, Some(p), None).unwrap();
        }
        let receipt = s.deliver(&rec).await.unwrap();
        settle(&s).await;
        (receipt, s.metrics(), s.stats(), ex.trees().len())
    };
    let never = |on: &[AlwaysSampleOn]| SinkConfig {
        sampling: rustev_otel::Sampling {
            parent: ParentSampling::Honor,
            root: RootSampling::Never,
            always_sample_on: on.iter().copied().collect(),
        },
        ..config()
    };
    // Root never: sampled out, acknowledged, metrics still recorded.
    let (receipt, m, st, n) = run(never(&[]), rec.clone(), None).await;
    assert_eq!(receipt, format!("otel:sampled_out:{}", digest(&rec)));
    assert_eq!((st.sampled_out, n), (1, 0));
    assert_eq!(m.counter_total(name::DECISIONS), 1);
    assert_eq!(m.counter_total(name::SAMPLED_OUT), 1);
    // Root always, the default.
    let (_, _, st, n) = run(config(), rec.clone(), None).await;
    assert_eq!((st.sampled_out, n), (0, 1));
    // Parent honored: an unsampled parent decides; ignored, the root rule.
    let unsampled = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-00";
    let (_, _, st, _) = run(config(), rec.clone(), Some(unsampled)).await;
    assert_eq!(st.sampled_out, 1);
    let mut ignore = config();
    ignore.sampling.parent = ParentSampling::Ignore;
    let (_, _, st, _) = run(ignore, rec.clone(), Some(unsampled)).await;
    assert_eq!(st.sampled_out, 0);
    let sampled = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
    let (_, _, st, _) = run(never(&[]), rec.clone(), Some(sampled)).await;
    assert_eq!(st.sampled_out, 0);
    // Ratio: consistent with the trace id.
    let mut ratio = config();
    ratio.sampling.root = RootSampling::Ratio {
        per_million: 500_000,
    };
    let low = "00-0af7651916cd43dd0000000000000001-b7ad6b7169203331-00";
    let high = "00-0af7651916cd43ddffffffffffffffff-b7ad6b7169203331-00";
    ratio.sampling.parent = ParentSampling::Ignore;
    let (_, _, st, _) = run(ratio.clone(), rec.clone(), Some(low)).await;
    assert_eq!(st.sampled_out, 0);
    let (_, _, st, _) = run(ratio, rec.clone(), Some(high)).await;
    assert_eq!(st.sampled_out, 1);
    assert!(ratio_sampled(&[0; 16], 1));
    assert!(!ratio_sampled(&[0xff; 16], 999_999));
    assert!(!ratio_sampled(&[0; 16], 0));

    // Each always_sample_on trigger overrides a negative decision.
    let mut escalate = rec.clone();
    let mut cancelled = rec.clone();
    let mut fallback = rec.clone();
    let mut step_unresolved = rec.clone();
    if let CoreEvidence::Recorded(e) = &mut escalate.core {
        e.judgment.outcome = Outcome::Escalate {
            reason: "billing_review".into(),
        };
    }
    cancelled.termination = Termination::Cancelled;
    cancelled.core = CoreEvidence::NotProduced;
    fallback.requests[0].transitions.push(Transition::Fallback {
        after: 1,
        class: FailureClass::Overloaded,
        to: 1,
    });
    if let CoreEvidence::Recorded(e) = &mut step_unresolved.core {
        e.steps[4].status =
            rustev_contract::evidence::StepStatus::Unresolved(Unresolved::BackendUnavailable {
                detail: "x".into(),
            });
    }
    for (trigger, r) in [
        (AlwaysSampleOn::Unresolved, unresolved(rec.clone())),
        (AlwaysSampleOn::Unresolved, step_unresolved),
        (AlwaysSampleOn::Refusal, escalate),
        (AlwaysSampleOn::Fallback, fallback),
        (AlwaysSampleOn::Cancelled, cancelled),
    ] {
        let (_, _, st, n) = run(never(&[trigger]), r.clone(), None).await;
        assert_eq!((st.sampled_out, n), (0, 1), "{trigger:?}");
        // And another trigger does not.
        let other = if trigger == AlwaysSampleOn::Cancelled {
            AlwaysSampleOn::Refusal
        } else {
            AlwaysSampleOn::Cancelled
        };
        let (_, _, st, _) = run(never(&[other]), r, None).await;
        assert_eq!(st.sampled_out, 1, "{trigger:?} vs {other:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metrics_carry_only_allowed_enum_and_plan_metadata_dimensions() {
    let ex = TestExporter::new(Behavior::Succeed);
    let mut c = config();
    c.bindings = remote_bindings();
    let s = sink(c, ex.clone());
    s.register_plan(&plan("support"));
    for x in exchanges().values() {
        s.record_exchange(x);
    }
    s.deliver(&unresolved(record("support"))).await.unwrap();
    settle(&s).await;
    let m = s.metrics();
    let plan_id = record("support").plan_id.to_string();
    assert_eq!(
        m.counter(
            name::DECISIONS,
            &[
                ("rustev.outcome", "unresolved"),
                ("rustev.plan.id", &plan_id),
                ("rustev.termination", "judged"),
            ]
        ),
        1
    );
    assert_eq!(
        m.counter(
            name::UNRESOLVED,
            &[
                ("rustev.outcome.unresolved_reason", "missing_evidence"),
                ("rustev.plan.id", &plan_id),
                ("scope", "outcome"),
            ]
        ),
        1
    );
    assert!(m.instruments.contains_key(name::ATTEMPT_DURATION));
    assert!(m.instruments.contains_key(name::TOKEN_USAGE));
    assert!(m.instruments.contains_key(name::OPERATION_DURATION));
    // No id, digest or text is ever a dimension.
    let text = format!("{m:?}");
    assert!(!text.contains("hmac-sha256") && !text.contains("support-1"));

    // A policy that drops the plan id drops the dimension.
    let mut p = RedactionPolicy::default();
    p.fields.push(FieldOverride {
        field: "rustev.plan.id".into(),
        action: rustev_contract::redaction::Action::Digest,
    });
    let s = sink(
        SinkConfig {
            policy: p,
            ..config()
        },
        TestExporter::new(Behavior::Succeed),
    );
    s.deliver(&record("support")).await.unwrap();
    let m = s.metrics();
    assert_eq!(
        m.counter(
            name::DECISIONS,
            &[
                ("rustev.outcome", "propose"),
                ("rustev.termination", "judged")
            ]
        ),
        1
    );

    // Runtime gauges, read through the host's callback.
    s.observe_runtime(|| rustev_runtime::Gauges {
        in_flight: 0,
        queued: 0,
        rejected: 7,
        abandoned: 0,
        backend_in_flight: Default::default(),
        sink: rustev_runtime::SinkStats {
            acknowledged: 3,
            failed: 1,
            timed_out: 0,
            dropped: 2,
            queued: 0,
            in_delivery: 0,
        },
    });
    let m = s.metrics();
    assert_eq!(m.counter_total(name::ADMISSION_REJECTED), 7);
    assert_eq!(m.counter(name::EVIDENCE_SINK, &[("result", "dropped")]), 2);

    // Metric exports are conserved like spans.
    let ex = TestExporter::new(Behavior::Succeed);
    let s = sink(config(), ex.clone());
    s.flush_metrics().await.unwrap();
    ex.set(Behavior::Fail);
    assert!(s.flush_metrics().await.is_err());
    let st = s.stats();
    assert_eq!(
        (st.metric_exports.accepted, st.metric_exports.exported),
        (2, 1)
    );
    conserved(&s);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logs_are_off_by_default_and_fixed_templates_when_on() {
    let ex = TestExporter::new(Behavior::Succeed);
    let s = sink(config(), ex.clone());
    s.deliver(&unresolved(record("support"))).await.unwrap();
    settle(&s).await;
    assert!(ex.trees()[0].logs.is_empty());

    let ex = TestExporter::new(Behavior::Succeed);
    let s = sink(
        SinkConfig {
            logs: true,
            ..config()
        },
        ex.clone(),
    );
    let mut rec = unresolved(record("support"));
    rec.requests[0].attempts[0].end = rustev_contract::run::AttemptEnd::Failed {
        class: FailureClass::Transient,
        detail: "CANARY-detail".into(),
    };
    s.deliver(&rec).await.unwrap();
    settle(&s).await;
    let logs = &ex.trees()[0].logs;
    let bodies: Vec<&str> = logs.iter().map(|l| l.body.as_str()).collect();
    assert_eq!(
        bodies,
        vec![
            "decision unresolved: missing_evidence",
            "attempt failed: transient"
        ]
    );
    assert!(logs.iter().all(|l| l.severity_text == "WARN"));
    assert_eq!(logs[0].span_id, ex.trees()[0].lines[0].span_id);
    assert!(!format!("{logs:?}").contains("CANARY"));
    let st = s.stats();
    assert_eq!((st.logs.accepted, st.logs.exported), (2, 2));
    conserved(&s);

    // A propose decision logs nothing; a sampled-out one is not logged.
    let ex = TestExporter::new(Behavior::Succeed);
    let s = sink(
        SinkConfig {
            logs: true,
            ..config()
        },
        ex.clone(),
    );
    s.deliver(&record("support")).await.unwrap();
    settle(&s).await;
    assert!(ex.trees()[0].logs.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn construction_refuses_bad_configurations() {
    let ex = || TestExporter::new(Behavior::Succeed);
    let refuse = |c: SinkConfig| {
        OtelSink::with_parts(
            c,
            ex(),
            Arc::new(Clock::default()),
            Box::new(SeededIds::new(b"x")),
        )
        .unwrap_err()
    };
    for (c, setting) in [
        (
            SinkConfig {
                max_queued_spans: 0,
                ..config()
            },
            "max_queued_spans",
        ),
        (
            SinkConfig {
                batch_size: 0,
                ..config()
            },
            "batch_size",
        ),
        (
            SinkConfig {
                export_timeout_ms: 0,
                ..config()
            },
            "export_timeout_ms",
        ),
        (
            SinkConfig {
                value_bound: 4097,
                ..config()
            },
            "value_bound",
        ),
        (
            SinkConfig {
                max_pending_contexts: 0,
                ..config()
            },
            "max_pending_contexts",
        ),
        (
            SinkConfig {
                context_ttl_ms: 0,
                ..config()
            },
            "context_ttl_ms",
        ),
        (
            SinkConfig {
                join_buffer: 0,
                ..config()
            },
            "join_buffer",
        ),
    ] {
        assert!(
            matches!(refuse(c), ConfigError::OutOfRange { setting: s, .. } if s == setting),
            "{setting}"
        );
    }
    let mut c = config();
    c.sampling.root = RootSampling::Ratio {
        per_million: 1_000_001,
    };
    assert!(matches!(refuse(c), ConfigError::OutOfRange { .. }));
    assert_eq!(
        refuse(SinkConfig {
            schema: 2,
            ..config()
        }),
        ConfigError::UnknownSchema { schema: 2 }
    );
    // The default policy digests ids: no key, no sink.
    assert!(matches!(
        refuse(SinkConfig {
            key: None,
            ..config()
        }),
        ConfigError::Redaction(_)
    ));
    // The JSONL exporter takes no logs.
    let dir = std::env::temp_dir().join(format!("rustev-otel-sinkcfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let jsonl = Arc::new(rustev_otel::JsonlExporter::open(&dir.join("t.jsonl"), 1 << 20).unwrap());
    assert_eq!(
        OtelSink::new(
            SinkConfig {
                logs: true,
                ..config()
            },
            jsonl.clone()
        )
        .unwrap_err(),
        ConfigError::LogsUnsupported
    );
    // The JSONL exporter as a sink's exporter writes the decision's lines.
    let s = OtelSink::new(config(), jsonl).unwrap();
    s.deliver(&record("support")).await.unwrap();
    settle(&s).await;
    let text = std::fs::read_to_string(dir.join("t.jsonl")).unwrap();
    assert_eq!(text.lines().count(), 12);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn construction_outside_a_runtime_is_refused() {
    assert_eq!(
        OtelSink::new(config(), TestExporter::new(Behavior::Succeed)).unwrap_err(),
        ConfigError::NoRuntime
    );
}
