use super::*;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use rustev_backend_embed_linear::{EmbedConfig, EmbedLinearBackend};
use rustev_contract::output::RawOutput;
use rustev_contract::run::{Charge, CostModel};
use rustev_contract::time::DurationMs;
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CallContext, CancelAck, CancelSignal,
    DecisionBackend, RemoteEnd,
};

fn config(b: &Bundle, workers: usize, max_queued: usize) -> EmbedConfig {
    EmbedConfig {
        bundle_dir: b.dir.clone(),
        expected_artifact: b.artifact(),
        runtime_path: None,
        workers,
        max_queued,
        embedding_cache: None,
    }
}
fn projection() -> Vec<u8> {
    serde_json::to_vec(
        &json!({"operation":"classify", "task":"route", "question":"Which route?",
        "options":["billing","technical","other"],
        "values":{"input:subject":"hello", "bind:ticket":{"body":"world"}}}),
    )
    .unwrap()
}
fn context() -> CallContext {
    CallContext {
        remaining_ms: DurationMs(1000),
        trace_id: "fixture".into(),
        principal_handle: vec![],
    }
}
fn attempt<'a>(
    backend: &'a EmbedLinearBackend,
    p: &'a [u8],
    cx: &'a CallContext,
    cancel: &CancelSignal,
) -> BoxFuture<'a, AttemptReport> {
    backend.infer(AttemptCall {
        projection: p,
        attempt_id: "fixture",
        cancel: cancel.clone(),
        cx,
    })
}

#[derive(Default)]
struct Notice(Mutex<u64>, Condvar);
impl Wake for Notice {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        *self.0.lock().unwrap() += 1;
        self.1.notify_all();
    }
}
impl Notice {
    fn count(&self) -> u64 {
        *self.0.lock().unwrap()
    }
    fn after(&self, before: u64) {
        let (n, timeout) = self
            .1
            .wait_timeout_while(self.0.lock().unwrap(), Duration::from_secs(5), |n| {
                *n <= before
            })
            .unwrap();
        assert!(
            *n > before && !timeout.timed_out(),
            "worker did not wake its waiter"
        );
    }
}
fn poll(f: &mut BoxFuture<'_, AttemptReport>, notice: &Arc<Notice>) -> Poll<AttemptReport> {
    f.as_mut()
        .poll(&mut Context::from_waker(&Waker::from(notice.clone())))
}
fn finish(f: &mut BoxFuture<'_, AttemptReport>, notice: &Arc<Notice>) -> AttemptReport {
    loop {
        let before = notice.count();
        if let Poll::Ready(r) = poll(f, notice) {
            return r;
        }
        notice.after(before);
    }
}
fn ready(f: &mut BoxFuture<'_, AttemptReport>, notice: &Arc<Notice>) -> AttemptReport {
    match poll(f, notice) {
        Poll::Ready(r) => r,
        Poll::Pending => panic!("expected immediate report"),
    }
}
fn assert_charge(r: &AttemptReport, units: u64) {
    assert_eq!(r.charge, Charge::Observed { units });
}
fn assert_failure(r: &AttemptReport, class: AdapterFailure, code: &str, units: u64) {
    let (actual, detail) = r.result.as_ref().unwrap_err();
    assert_eq!(*actual, class);
    assert!(detail.starts_with(&format!("embed:{code}:")), "{detail}");
    assert!(detail.len() <= 256);
    assert_charge(r, units);
}

#[derive(Clone, Copy)]
enum End {
    Good,
    Fail,
    Nan,
    Panic,
}
#[derive(Default)]
struct GateState {
    started: usize,
    permits: VecDeque<End>,
    ids: Vec<Vec<u32>>,
    signals: Vec<CancelSignal>,
}
#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}
impl Gate {
    fn run(&self, ids: &[u32], signal: &CancelSignal) -> Result<Vec<f32>, ()> {
        let end = {
            let mut state = self.state.lock().unwrap();
            state.started += 1;
            state.ids.push(ids.to_vec());
            state.signals.push(signal.clone());
            self.changed.notify_all();
            while state.permits.is_empty() {
                state = self.changed.wait(state).unwrap();
            }
            state.permits.pop_front().unwrap()
        };
        match end {
            End::Good => Ok(SyntheticEncoder::new(HIDDEN).unwrap().encode(ids)),
            End::Fail => Err(()),
            End::Nan => Ok(vec![f32::NAN; ids.len() * HIDDEN]),
            End::Panic => panic!("synthetic run failure"),
        }
    }
    fn started(&self, n: usize) {
        let (state, timeout) = self
            .changed
            .wait_timeout_while(self.state.lock().unwrap(), Duration::from_secs(5), |s| {
                s.started < n
            })
            .unwrap();
        assert!(
            state.started >= n && !timeout.timed_out(),
            "worker did not start"
        );
    }
    fn release(&self, end: End) {
        self.state.lock().unwrap().permits.push_back(end);
        self.changed.notify_all();
    }
}
fn blocked(b: &Bundle, workers: usize, queue: usize) -> (EmbedLinearBackend, Arc<Gate>) {
    let gate = Arc::new(Gate::default());
    let runner = gate.clone();
    (
        EmbedLinearBackend::open_with_synthetic_runner(config(b, workers, queue), move |ids, c| {
            runner.run(ids, c)
        })
        .unwrap(),
        gate,
    )
}

#[test]
fn synthetic_setup_without_runtime_and_configuration_limits() {
    let b = new_bundle();
    for (workers, queue) in [(0, 0), (17, 0), (1, 1025)] {
        assert_eq!(
            EmbedLinearBackend::open(config(&b, workers, queue))
                .err()
                .unwrap()
                .code,
            SetupCode::ManifestInvalid
        );
    }
    for (workers, queue) in [(1, 0), (16, 1024)] {
        let mut c = config(&b, workers, queue);
        c.runtime_path = Some(PathBuf::from("unused/relative/runtime"));
        let backend = EmbedLinearBackend::open(c).unwrap();
        assert_eq!(
            backend.descriptor(),
            &derive_descriptor(&b.verify().unwrap().manifest, b.artifact())
        );
        assert_eq!(backend.cost_model(), CostModel::Bounded);
        assert_eq!(
            backend.cost_bound(b"irrelevant"),
            CostBound::Bounded { max_units: 513 }
        );
    }
    let mut c = config(&b, 1, 0);
    c.expected_artifact = ArtifactId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
    assert_eq!(
        EmbedLinearBackend::open(c).err().unwrap().code,
        SetupCode::ArtifactMismatch
    );
}

#[test]
fn output_is_head_arithmetic_and_charge_uses_token_count() {
    let b = new_bundle();
    let backend = EmbedLinearBackend::open(config(&b, 1, 0)).unwrap();
    let (p, cx, signal, notice) = (
        projection(),
        context(),
        CancelSignal::new(),
        Arc::new(Notice::default()),
    );
    let r = finish(&mut attempt(&backend, &p, &cx, &signal), &notice);
    let bundle = b.verify().unwrap();
    let vectors = SyntheticEncoder::new(HIDDEN)
        .unwrap()
        .encode(&[1, 5, 3, 4, 2]);
    let embedding = pool(&vectors, &[1; 5], HIDDEN, Normalize::L2).unwrap();
    let logits = bundle.heads[0].logits(&embedding).unwrap();
    let expected = RawOutput::Logits(
        bundle.manifest.heads[0]
            .options
            .iter()
            .cloned()
            .zip(logits)
            .collect(),
    );
    assert_eq!(r.result, Ok(expected));
    assert_charge(&r, 11);
    assert_eq!(
        (r.cancel, r.remote),
        (CancelAck::NotRequested, RemoteEnd::Finished)
    );
    assert!(
        !signal.is_raised(),
        "private cleanup must not cancel the caller"
    );
    assert_eq!(signal.registered(), 0);
}

#[test]
fn preparation_failures_and_pre_dispatch_cancellation_cost_zero() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 1, 0);
    let (p, mut cx, signal, notice) = (
        projection(),
        context(),
        CancelSignal::new(),
        Arc::new(Notice::default()),
    );
    let r = ready(&mut attempt(&backend, b"{}", &cx, &signal), &notice);
    assert_failure(&r, AdapterFailure::Permanent, "task_not_found", 0);
    cx.remaining_ms = DurationMs(0);
    let r = ready(&mut attempt(&backend, &p, &cx, &signal), &notice);
    assert_failure(&r, AdapterFailure::Cancelled, "cancelled", 0);
    assert_eq!(r.cancel, CancelAck::Stopped);
    cx.remaining_ms = DurationMs(1000);
    signal.raise();
    let r = ready(&mut attempt(&backend, &p, &cx, &signal), &notice);
    assert_failure(&r, AdapterFailure::Cancelled, "cancelled", 0);
    assert_eq!(
        (r.cancel, r.remote),
        (CancelAck::Stopped, RemoteEnd::Finished)
    );
    assert_eq!(gate.state.lock().unwrap().started, 0);
}

#[test]
fn worker_failures_and_nonfinite_charge_after_handoff_and_slot_is_reusable() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 1, 0);
    let (p, cx, notice) = (projection(), context(), Arc::new(Notice::default()));
    for (i, end, code) in [
        (1, End::Fail, "runtime_error"),
        (2, End::Nan, "non_finite"),
        (3, End::Panic, "runtime_error"),
        (4, End::Good, ""),
    ] {
        let signal = CancelSignal::new();
        let mut f = attempt(&backend, &p, &cx, &signal);
        assert!(poll(&mut f, &notice).is_pending());
        gate.started(i);
        let overflow = ready(
            &mut attempt(&backend, &p, &cx, &CancelSignal::new()),
            &notice,
        );
        assert_failure(&overflow, AdapterFailure::Overloaded, "queue_full", 0);
        gate.release(end);
        let r = finish(&mut f, &notice);
        if code.is_empty() {
            assert!(r.result.is_ok());
            assert_charge(&r, 11);
        } else {
            assert_failure(&r, AdapterFailure::Permanent, code, 11);
        }
        assert_eq!(r.cancel, CancelAck::NotRequested);
    }
}

#[test]
fn queued_cancellation_removes_work_without_charge_and_queue_is_bounded() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 1, 1);
    let (p, cx, notice) = (projection(), context(), Arc::new(Notice::default()));
    let (a, q, overflow) = (
        CancelSignal::new(),
        CancelSignal::new(),
        CancelSignal::new(),
    );
    let mut first = attempt(&backend, &p, &cx, &a);
    assert!(poll(&mut first, &notice).is_pending());
    gate.started(1);
    let mut queued = attempt(&backend, &p, &cx, &q);
    assert!(poll(&mut queued, &notice).is_pending());
    let r = ready(&mut attempt(&backend, &p, &cx, &overflow), &notice);
    assert_failure(&r, AdapterFailure::Overloaded, "queue_full", 0);
    q.raise();
    let r = ready(&mut queued, &notice);
    assert_failure(&r, AdapterFailure::Cancelled, "cancelled", 0);
    assert_eq!(
        (r.cancel, r.remote),
        (CancelAck::Stopped, RemoteEnd::Finished)
    );
    let mut replacement = attempt(&backend, &p, &cx, &overflow);
    assert!(poll(&mut replacement, &notice).is_pending());
    gate.release(End::Good);
    assert!(finish(&mut first, &notice).result.is_ok());
    gate.started(2);
    gate.release(End::Good);
    assert!(finish(&mut replacement, &notice).result.is_ok());
    assert_eq!(gate.state.lock().unwrap().started, 2);
}

#[test]
fn cancellation_after_handoff_stopped_only_when_completion_is_observed() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 1, 0);
    let (p, cx, notice) = (projection(), context(), Arc::new(Notice::default()));
    for (n, end) in [(1, End::Good), (2, End::Fail)] {
        let signal = CancelSignal::new();
        let mut f = attempt(&backend, &p, &cx, &signal);
        assert!(poll(&mut f, &notice).is_pending());
        gate.started(n);
        signal.raise();
        let before = notice.count();
        gate.release(end);
        notice.after(before); // Completion is published before this wake.
        let r = ready(&mut f, &notice);
        if n == 1 {
            assert!(r.result.is_ok());
        } else {
            assert_failure(&r, AdapterFailure::Cancelled, "cancelled", 11);
        }
        assert_charge(&r, 11);
        assert_eq!(
            (r.cancel, r.remote),
            (CancelAck::Stopped, RemoteEnd::Finished)
        );
    }
}

#[test]
fn unconfirmed_cancellation_holds_slot_discards_late_result_and_preserves_fifo() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 1, 2);
    let (p, cx, signal, notice) = (
        projection(),
        context(),
        CancelSignal::new(),
        Arc::new(Notice::default()),
    );
    let mut f = attempt(&backend, &p, &cx, &signal);
    assert!(poll(&mut f, &notice).is_pending());
    gate.started(1);
    signal.raise();
    let r = ready(&mut f, &notice);
    assert_failure(&r, AdapterFailure::Cancelled, "cancelled", 11);
    assert_eq!(
        (r.cancel, r.remote),
        (CancelAck::Unconfirmed, RemoteEnd::PossiblyContinuing)
    );
    let p2 = String::from_utf8(p.clone())
        .unwrap()
        .replace("hello", "world")
        .into_bytes();
    let p3 = String::from_utf8(p.clone())
        .unwrap()
        .replace("world", "hello")
        .into_bytes();
    let mut second = attempt(&backend, &p2, &cx, &CancelSignal::new());
    let mut third = attempt(&backend, &p3, &cx, &CancelSignal::new());
    assert!(poll(&mut second, &notice).is_pending());
    assert!(poll(&mut third, &notice).is_pending());
    let r = ready(
        &mut attempt(&backend, &p, &cx, &CancelSignal::new()),
        &notice,
    );
    assert_failure(&r, AdapterFailure::Overloaded, "queue_full", 0);
    let before = notice.count();
    gate.release(End::Good); // Late output belongs to the abandoned first attempt.
    gate.started(2);
    assert_eq!(
        notice.count(),
        before,
        "late output must not wake its former waiter"
    );
    assert!(poll(&mut second, &notice).is_pending());
    gate.release(End::Fail);
    assert_failure(
        &finish(&mut second, &notice),
        AdapterFailure::Permanent,
        "runtime_error",
        11,
    );
    gate.started(3);
    gate.release(End::Good);
    assert!(finish(&mut third, &notice).result.is_ok());
    let state = gate.state.lock().unwrap();
    assert_eq!(
        state.ids,
        [
            vec![1, 5, 3, 4, 2],
            vec![1, 5, 4, 4, 2],
            vec![1, 5, 3, 3, 2]
        ]
    );
}

#[test]
fn dropping_queued_or_running_future_keeps_running_work_bounded() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 2, 1);
    let (p, cx, notice) = (projection(), context(), Arc::new(Notice::default()));
    let mut a = attempt(&backend, &p, &cx, &CancelSignal::new());
    let mut b = attempt(&backend, &p, &cx, &CancelSignal::new());
    assert!(poll(&mut a, &notice).is_pending());
    assert!(poll(&mut b, &notice).is_pending());
    gate.started(2);
    let mut q = attempt(&backend, &p, &cx, &CancelSignal::new());
    assert!(poll(&mut q, &notice).is_pending());
    drop(q);
    drop(a);
    drop(b);
    assert!(
        gate.state
            .lock()
            .unwrap()
            .signals
            .iter()
            .all(CancelSignal::is_raised)
    );
    let mut q = attempt(&backend, &p, &cx, &CancelSignal::new());
    assert!(poll(&mut q, &notice).is_pending());
    assert_failure(
        &ready(
            &mut attempt(&backend, &p, &cx, &CancelSignal::new()),
            &notice,
        ),
        AdapterFailure::Overloaded,
        "queue_full",
        0,
    );
    gate.release(End::Good);
    gate.started(3);
    gate.release(End::Good);
    gate.release(End::Good);
    assert!(finish(&mut q, &notice).result.is_ok());
}

#[test]
/// Refused `unqualified_platform` before any runtime is loaded, except on
/// the qualified `linux-x86_64-gnu`, where the absent runtime path is
/// refused next (3.6.1, 3.6.3).
fn onnx_is_refused_before_runtime_loading_on_every_unqualified_target() {
    let mut b = new_bundle();
    let v = &mut b.manifest;
    v["encoder"]["kind"] = json!("onnx");
    v["encoder"]["graph"]["source"] = json!("onnx/model.onnx");
    v["checkpoint"]["repository"] = json!("sentence-transformers/all-MiniLM-L6-v2");
    v["checkpoint"]["files"] =
        json!([{"name":"onnx/model.onnx", "sha256":v["encoder"]["graph"]["sha256"]}]);
    v["runtime"]["libraries"] = json!([{"platform":"macos-aarch64", "sha256":"a".repeat(64)}]);
    // The pinned versions, so a qualified target reaches the path check.
    v["runtime"]["ort"] = json!(rustev_backend_embed_linear::runtime::ORT_VERSION);
    v["runtime"]["onnxruntime"] = json!("1.31.0");
    v["tolerance"]["feasibility_record"] = json!(format!("sha256:{}", "b".repeat(64)));
    for h in v["heads"].as_array_mut().unwrap() {
        h["interpretation"] = json!("linear scores");
        h["provenance"]["synthetic"] = json!(false);
    }
    b.write();
    let e = EmbedLinearBackend::open(config(&b, 1, 0)).err().unwrap();
    if rustev_backend_embed_linear::runtime::qualified_platform().is_ok() {
        assert_eq!(
            (e.code, e.item.as_str()),
            (SetupCode::RuntimeMissing, "runtime_path")
        );
    } else {
        assert_eq!(
            (e.code, e.item.as_str()),
            (SetupCode::UnqualifiedPlatform, "platform")
        );
    }
}

#[test]
fn cancelled_queued_work_is_skipped_even_before_its_waiter_polls() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 1, 1);
    let (p, cx, notice) = (projection(), context(), Arc::new(Notice::default()));
    let mut first = attempt(&backend, &p, &cx, &CancelSignal::new());
    assert!(poll(&mut first, &notice).is_pending());
    gate.started(1);
    let signal = CancelSignal::new();
    let queued_notice = Arc::new(Notice::default());
    let mut queued = attempt(&backend, &p, &cx, &signal);
    assert!(poll(&mut queued, &queued_notice).is_pending());
    signal.raise();
    let before = queued_notice.count();
    gate.release(End::Good);
    queued_notice.after(before);
    let r = ready(&mut queued, &queued_notice);
    assert_failure(&r, AdapterFailure::Cancelled, "cancelled", 0);
    assert_eq!(r.cancel, CancelAck::Stopped);
    assert!(finish(&mut first, &notice).result.is_ok());
    assert_eq!(gate.state.lock().unwrap().started, 1);
}

#[test]
fn truncated_input_is_charged_only_for_tokens_handed_to_encoder() {
    let mut b = new_bundle();
    b.manifest["tokenizer"]["max_tokens"] = json!(3);
    b.manifest["tokenizer"]["truncation"] = json!("truncate_end");
    b.write();
    let backend = EmbedLinearBackend::open(config(&b, 1, 0)).unwrap();
    let (p, cx, notice) = (projection(), context(), Arc::new(Notice::default()));
    let r = finish(
        &mut attempt(&backend, &p, &cx, &CancelSignal::new()),
        &notice,
    );
    assert!(r.result.is_ok());
    assert_charge(&r, 7);
    assert_eq!(backend.cost_bound(&p), CostBound::Bounded { max_units: 7 });
}

mod cache;
