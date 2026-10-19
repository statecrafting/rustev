//! A plan 3 at the runtime (spec 032, 3.11): preparation needs a sink that
//! accepts `rustev.run/3`, and capture refuses it before admission.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use rustev_contract::definition::Definition;
use rustev_contract::run::RunRecord;
use rustev_contract::schema;
use rustev_contract::scope::{Handle, Scope};
use rustev_core::seams::{BoxFuture, CancelSignal, EvidenceSink};
use rustev_runtime::{CaptureConfig, CaptureConfigError, ManualClock, PrepareError, Runtime};
use serde_json::json;

/// A sink that counts deliveries and accepts `rustev.run/3` when told to.
struct Sink {
    v3: bool,
    delivered: AtomicUsize,
}

impl EvidenceSink for Sink {
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async {
            self.delivered.fetch_add(1, Ordering::SeqCst);
            Ok("receipt".to_string())
        })
    }

    fn accepts(&self, schema: &str) -> bool {
        schema == schema::RUN || (self.v3 && schema == schema::RUN_V3)
    }
}

/// The smallest definition of each version: one record input, whose field
/// is nullable in version 2.
fn plan(v2: bool) -> rustev_core::Compiled {
    let text = json!({"text": {"max_bytes": 8}});
    let field = if v2 {
        json!({"nullable": {"type": text}})
    } else {
        text
    };
    let d = json!({
        "schema": if v2 { "rustev.definition/2" } else { "rustev.definition/1" },
        "name": "p3", "version": "0.1.0", "package": "p",
        "inputs": [{"name": "x", "type": {"record": {"fields": [{"name": "a", "type": field}]}},
            "provenance": ["user-supplied"], "freshness": "not_required"}],
        "steps": [], "policy": {"on_unresolved": [], "rules": [
            {"id": "r", "when": "always", "then": {"escalate": {"reason": "x"}}}], "adjustments": []},
        "limits": {"max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 0,
            "distribution_tolerance": "0.000001", "deadline_ms": 0}
    });
    let def: Definition = serde_json::from_value(d).unwrap();
    let c = rustev_core::compile(&def, &[], &[]).unwrap();
    let expected = if v2 { schema::PLAN_V3 } else { schema::PLAN };
    assert_eq!(c.plan.schema, expected);
    c
}

fn plan_3() -> rustev_core::Compiled {
    plan(true)
}

fn h(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

fn runtime(sink: Arc<Sink>) -> Runtime {
    Runtime::builder(Arc::new(ManualClock::new()), sink, config(1, 0, 1))
        .build()
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn a_plan_3_needs_a_sink_that_accepts_run_3() {
    let v1 = Arc::new(Sink {
        v3: false,
        delivered: AtomicUsize::new(0),
    });
    assert!(matches!(
        runtime(v1.clone()).prepare(plan_3()),
        Err(PrepareError::SinkVersionUnsupported { schema }) if schema == schema::RUN_V3
    ));
    // A plan 2 prepares on the same sink.
    assert!(runtime(v1).prepare(plan(false)).is_ok());
    let v3 = Arc::new(Sink {
        v3: true,
        delivered: AtomicUsize::new(0),
    });
    assert!(runtime(v3).prepare(plan_3()).is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn capture_refuses_a_plan_3_before_admission() {
    let sink = Arc::new(Sink {
        v3: true,
        delivered: AtomicUsize::new(0),
    });
    let rt = runtime(sink.clone());
    let plan = rt.prepare(plan_3()).unwrap();
    let config = CaptureConfig {
        scope: Scope::principal(h("tenant-a"), h("rev-1"), h("scope-p")),
        max_bytes: 1_024,
    };
    let r = rt
        .decide_with_capture(&plan, request("d1"), &CancelSignal::default(), &config)
        .await;
    assert!(matches!(
        r,
        Err(CaptureConfigError::CaptureUnsupported { plan_schema }) if plan_schema == schema::PLAN_V3
    ));
    assert_eq!(sink.delivered.load(Ordering::SeqCst), 0, "no run record");
}

#[tokio::test(flavor = "current_thread")]
async fn a_snapshot_2_for_a_plan_2_is_rejected_before_admission() {
    let sink = Arc::new(Sink {
        v3: false,
        delivered: AtomicUsize::new(0),
    });
    let rt = runtime(sink.clone());
    let plan = rt.prepare(plan(false)).unwrap();
    let mut req = request("d1");
    req.snapshot = <rustev_contract::snapshot::Snapshot as rustev_contract::Document>::parse(
        json!({"schema": schema::SNAPSHOT_V2, "entries": []})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let r = rt.decide(&plan, req, &CancelSignal::default()).await;
    assert!(
        matches!(
            r,
            Err(rustev_runtime::DecideError::Rejected(
                rustev_runtime::Rejection::InvalidRequest(ref d)
            )) if d.contains("snapshot_version")
        ),
        "{:?}",
        r.err()
    );
    assert_eq!(sink.delivered.load(Ordering::SeqCst), 0, "no run record");
}
