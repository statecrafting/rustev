//! Runtime cases expressed as data (spec 027, 3.12.4): a scripted backend
//! script per attempt (outcome, charge, remote end, cancel answer), a
//! manual clock driven by declared steps, and the requests to decide. The
//! Rust runner interprets a case with Rust seams; a binding's runner
//! interprets the same case with its host's seams, and both must produce
//! the same run-record bytes. Under the manual clock every timing is fixed,
//! so the bytes are compared without normalization.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::output::RawOutput;
use rustev_contract::plan::Plan;
use rustev_contract::run::{Charge, CostBound, CostModel, RunRecord};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::Compiled;
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CancelAck, CancelSignal,
    DecisionBackend, EvidenceSink, RemoteEnd,
};
use rustev_runtime::{
    Completion, DecideError, DecisionRequest, ManualClock, Rejection, Runtime, RuntimeConfig,
    SinkPolicy,
};
use serde::Deserialize;
use serde_json::{Value as Json, json};
use tokio::sync::oneshot;

/// A runtime case's decision, as data.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCase {
    /// Input names of the compiled plan, the backend descriptors it was
    /// compiled against and its calibrations.
    pub plan: String,
    pub descriptors: Vec<String>,
    #[serde(default)]
    pub calibrations: Vec<String>,
    pub config: Config,
    pub backends: Vec<BackendScript>,
    pub requests: Vec<Request>,
    pub drive: Vec<Step>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub max_in_flight: usize,
    pub max_queued: usize,
    pub max_parallel_requests: usize,
    pub sink_timeout_ms: u64,
}

/// One scripted backend.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendScript {
    /// The input name of its descriptor.
    pub descriptor: String,
    pub max_concurrency: usize,
    pub cost_model: CostModel,
    pub cost_bound: CostBound,
    pub remote: Remote,
    pub on_cancel: OnCancel,
    /// Per task, the answer of each attempt in order (the last repeats).
    pub answers: BTreeMap<String, Vec<Answer>>,
    /// Per task, the answer a released gate gives.
    #[serde(default)]
    pub on_release: BTreeMap<String, Answer>,
}

/// What an attempt does.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Answer {
    Output {
        output: RawOutput,
        charge: Charge,
    },
    Fail {
        class: Class,
        detail: String,
        charge: Charge,
    },
    /// Wait for a release step or the attempt's cancellation signal.
    Gate,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Transient,
    Overloaded,
    Permanent,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Remote {
    Finished,
    PossiblyContinuing,
}

/// What a gated attempt answers when its cancellation signal is raised.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OnCancel {
    Stop { charge: Charge },
    Unconfirmed,
    Ignore,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub decision_id: String,
    /// The input name of its snapshot.
    pub snapshot: String,
    pub evaluation_time_ms: i64,
    #[serde(default)]
    /// Tightens the plan's deadline when set; absent, the plan's own
    /// `deadline_ms` applies (`runtime.deadline` relies on the support
    /// plan's 2000 ms).
    pub deadline_ms: Option<u64>,
}

/// One step of the driver, in order; each waits until its condition holds.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Start the next request.
    Start,
    /// Until backend `backend` holds `count` attempts at a gate.
    WaitGated {
        backend: usize,
        count: usize,
    },
    /// Until `count` decisions wait for admission.
    WaitQueued {
        count: usize,
    },
    /// Until something sleeps on the clock.
    WaitSleeper,
    AdvanceMs {
        ms: u64,
    },
    /// Release every gated attempt of `backend` with its task's
    /// `on_release` answer.
    ReleaseAll {
        backend: usize,
    },
    /// Raise request `request`'s cancellation signal.
    Cancel {
        request: usize,
    },
}

/// Map a document by input name; the runner supplies the bytes.
pub type Inputs = BTreeMap<String, Vec<u8>>;

fn doc<T: Document>(inputs: &Inputs, name: &str) -> Result<T, String> {
    let b = inputs
        .get(name)
        .ok_or_else(|| format!("no input {name:?}"))?;
    T::parse(b).map_err(|e| format!("{name}: {e}"))
}

struct Scripted {
    descriptor: BackendDescriptor,
    script: BackendScript,
    gates: Mutex<BTreeMap<String, (String, oneshot::Sender<Answer>)>>,
}

impl Scripted {
    fn answer(&self, task: &str, n: usize) -> Answer {
        let list = self
            .script
            .answers
            .get(task)
            .or_else(|| self.script.answers.get("*"));
        match list {
            // Attempts count from 1; a 0 reads as the first.
            Some(l) if !l.is_empty() => l[n.clamp(1, l.len()) - 1].clone(),
            _ => Answer::Fail {
                class: Class::Permanent,
                detail: format!("no scripted answer for {task}"),
                charge: Charge::Observed { units: 0 },
            },
        }
    }

    /// The attempts actually released: an answer counts only when a
    /// waiting attempt receives it. A cancelled attempt has already left
    /// the gates (whatever its backend does next), so releasing after a
    /// cancellation finds nothing, and a script that expects otherwise is
    /// told so.
    fn release_all(&self) -> usize {
        let gates = std::mem::take(&mut *self.gates.lock().unwrap());
        let mut n = 0;
        for (_, (task, tx)) in gates {
            let a = self
                .script
                .on_release
                .get(&task)
                .cloned()
                // A release answer that gates again would wait for a
                // release that never comes: it is a script error, failed
                // like a missing answer.
                .filter(|a| !matches!(a, Answer::Gate))
                .unwrap_or(Answer::Fail {
                    class: Class::Permanent,
                    detail: format!("no release answer (or a gate) for {task}"),
                    charge: Charge::Observed { units: 0 },
                });
            if tx.send(a).is_ok() {
                n += 1;
            }
        }
        n
    }

    fn remote(&self) -> RemoteEnd {
        match self.script.remote {
            Remote::Finished => RemoteEnd::Finished,
            Remote::PossiblyContinuing => RemoteEnd::PossiblyContinuing,
        }
    }
}

struct Handle(Arc<Scripted>);

impl DecisionBackend for Handle {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.0.descriptor
    }

    fn cost_model(&self) -> CostModel {
        self.0.script.cost_model
    }

    fn cost_bound(&self, _projection: &[u8]) -> CostBound {
        self.0.script.cost_bound
    }

    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        let s = self.0.clone();
        let id = call.attempt_id.to_string();
        // The attempt number within its request ends the attempt id.
        let n = id
            .rsplit('/')
            .next()
            .and_then(|x| x.parse().ok())
            .unwrap_or(1usize);
        // Every projection a plan sends names its task; one that does not is
        // a broken fixture, stopped here by name rather than surfacing as an
        // unexplained `permanent` failure.
        let task = serde_json::from_slice::<Json>(call.projection)
            .ok()
            .and_then(|p| p["task"].as_str().map(str::to_string))
            .unwrap_or_else(|| {
                panic!(
                    "attempt {id}: the projection names no task: {}",
                    String::from_utf8_lossy(call.projection)
                )
            });
        let first = s.answer(&task, n);
        let signal = call.cancel.clone();
        Box::pin(async move {
            let answer = match first {
                Answer::Gate => {
                    let (tx, mut rx) = oneshot::channel();
                    s.gates.lock().unwrap().insert(id.clone(), (task, tx));
                    let released = std::future::poll_fn(|cx| {
                        if let Poll::Ready(r) = Pin::new(&mut rx).poll(cx) {
                            return Poll::Ready(r.ok());
                        }
                        if signal.poll_raised(cx).is_ready() {
                            return Poll::Ready(None);
                        }
                        Poll::Pending
                    })
                    .await;
                    match released {
                        Some(a) => a,
                        None => {
                            s.gates.lock().unwrap().remove(&id);
                            return match s.script.on_cancel {
                                OnCancel::Stop { charge } => AttemptReport {
                                    result: Err((AdapterFailure::Cancelled, "stopped".into())),
                                    charge,
                                    cancel: CancelAck::Stopped,
                                    remote: s.remote(),
                                },
                                OnCancel::Unconfirmed => AttemptReport {
                                    result: Err((
                                        AdapterFailure::Cancelled,
                                        "stopped waiting".into(),
                                    )),
                                    charge: Charge::Unknown,
                                    cancel: CancelAck::Unconfirmed,
                                    remote: s.remote(),
                                },
                                // A backend that ignores cancellation never
                                // answers: the runtime polls a cancelled
                                // attempt once (spec 003, 3.7.3), records
                                // `not_observed` and moves on, so the
                                // decision ends without this future.
                                OnCancel::Ignore => std::future::pending().await,
                            };
                        }
                    }
                }
                a => a,
            };
            let (result, charge) = match answer {
                Answer::Output { output, charge } => (Ok(output), charge),
                Answer::Fail {
                    class,
                    detail,
                    charge,
                } => (
                    Err((
                        match class {
                            Class::Transient => AdapterFailure::Transient,
                            Class::Overloaded => AdapterFailure::Overloaded,
                            Class::Permanent => AdapterFailure::Permanent,
                        },
                        detail,
                    )),
                    charge,
                ),
                // Never sent: a released gate's answer is filtered above.
                Answer::Gate => unreachable!("a release answer never gates"),
            };
            // An answer that arrives with cancellation already raised still
            // stands, and the backend did not act on the cancellation.
            let cancel = if signal.is_raised() {
                CancelAck::Unconfirmed
            } else {
                CancelAck::NotRequested
            };
            AttemptReport {
                result,
                charge,
                cancel,
                remote: s.remote(),
            }
        })
    }
}

/// A sink that acknowledges every record with its decision id.
struct Ack;

impl EvidenceSink for Ack {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("ack:{}", record.decision_id)) })
    }

    fn accepts(&self, s: &str) -> bool {
        s == rustev_contract::schema::RUN || s == rustev_contract::schema::RUN_V3
    }
}

/// Each request's outputs: `<decision>.result.json` (its status, and a
/// rejection's kind) and, when a record was produced, `<decision>.record.json`
/// (the record canonical bytes).
pub type Outputs = BTreeMap<String, Vec<u8>>;

/// Run `case` with Rust seams on a manual clock.
pub fn run(case: &RuntimeCase, inputs: &Inputs) -> Result<Outputs, String> {
    let plan: Plan = doc(inputs, &case.plan)?;
    let descriptors: Vec<BackendDescriptor> = case
        .descriptors
        .iter()
        .map(|n| doc(inputs, n))
        .collect::<Result<_, _>>()?;
    let calibrations: Vec<CalibrationArtifact> = case
        .calibrations
        .iter()
        .map(|n| doc(inputs, n))
        .collect::<Result<_, _>>()?;
    let compiled = Compiled::load_checked(&plan, &descriptors, &calibrations)
        .map_err(|e| format!("load: {e}"))?;
    let clock = ManualClock::new();
    let mut builder = Runtime::builder(
        Arc::new(clock.clone()),
        Arc::new(Ack),
        RuntimeConfig {
            max_in_flight: case.config.max_in_flight,
            max_queued: case.config.max_queued,
            max_parallel_requests: case.config.max_parallel_requests,
            sink: SinkPolicy::FailDecision {
                timeout_ms: case.config.sink_timeout_ms,
            },
        },
    );
    let mut scripted = vec![];
    for b in &case.backends {
        let s = Arc::new(Scripted {
            descriptor: doc(inputs, &b.descriptor)?,
            script: b.clone(),
            gates: Mutex::new(BTreeMap::new()),
        });
        builder = builder.backend(Arc::new(Handle(s.clone())), b.max_concurrency);
        scripted.push(s);
    }
    let rt = Arc::new(builder.build().map_err(|e| format!("{e:?}"))?);
    let prepared = Arc::new(rt.prepare(compiled).map_err(|e| format!("{e:?}"))?);
    let mut requests = vec![];
    for r in &case.requests {
        requests.push(DecisionRequest {
            decision_id: r.decision_id.clone(),
            snapshot: doc::<Snapshot>(inputs, &r.snapshot)?,
            evaluation_time: Timestamp::from_ms(r.evaluation_time_ms)
                .map_err(|_| "evaluation time out of range".to_string())?,
            deadline_ms: r.deadline_ms,
            principal_handle: vec![],
        });
    }
    let signals: Vec<CancelSignal> = case.requests.iter().map(|_| CancelSignal::new()).collect();
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| e.to_string())?;
    tokio.block_on(async {
        let mut running = vec![];
        let mut next = requests.into_iter();
        // A hang is a failure, never a pass: every wait is bounded.
        async fn until(what: &str, mut cond: impl FnMut() -> bool) -> Result<(), String> {
            for _ in 0..100_000 {
                if cond() {
                    return Ok(());
                }
                tokio::task::yield_now().await;
            }
            Err(format!("the driver waited forever for {what}"))
        }
        for step in &case.drive {
            match step {
                Step::Start => {
                    let req = next.next().ok_or("no request left to start")?;
                    let i = running.len();
                    let (rt, plan, cancel) = (rt.clone(), prepared.clone(), signals[i].clone());
                    running.push(tokio::spawn(
                        async move { rt.decide(&plan, req, &cancel).await },
                    ));
                    tokio::task::yield_now().await;
                }
                Step::WaitGated { backend, count } => {
                    let s = scripted.get(*backend).ok_or("no such backend")?;
                    until("gated attempts", || s.gates.lock().unwrap().len() >= *count).await?;
                }
                Step::WaitQueued { count } => {
                    until("queued decisions", || rt.gauges().queued >= *count).await?;
                }
                Step::WaitSleeper => until("a sleeper", || clock.sleepers() > 0).await?,
                Step::AdvanceMs { ms } => clock.advance(*ms),
                Step::ReleaseAll { backend } => {
                    // A release that frees nothing is a script error, caught
                    // here rather than as a later mismatch.
                    let released = scripted
                        .get(*backend)
                        .ok_or("no such backend")?
                        .release_all();
                    if released == 0 {
                        return Err(format!("release_all on backend {backend} released nothing"));
                    }
                }
                Step::Cancel { request } => {
                    signals.get(*request).ok_or("no such request")?.raise();
                }
            }
        }
        let mut out = Outputs::new();
        for (i, h) in running.into_iter().enumerate() {
            let id = &case.requests[i].decision_id;
            let r = tokio::time::timeout(std::time::Duration::from_secs(30), h)
                .await
                .map_err(|_| format!("{id} never finished"))?
                .map_err(|e| format!("{id}: {e}"))?;
            let (status, record) = match r {
                Ok(d) => (
                    json!({ "decision_id": id, "status": match d.completion {
                        Completion::Judged(_) => "judged",
                        Completion::Cancelled => "cancelled",
                    }}),
                    Some(d.record),
                ),
                Err(DecideError::Rejected(r)) => (
                    json!({ "decision_id": id, "status": "rejected", "rejection": match r {
                        Rejection::Overloaded => "overloaded",
                        Rejection::QueueDeadline => "queue_deadline",
                        Rejection::Cancelled => "cancelled",
                        Rejection::InvalidRequest(_) => "invalid_request",
                    }}),
                    None,
                ),
                Err(DecideError::EvidenceNotDelivered { record, .. }) => (
                    json!({ "decision_id": id, "status": "evidence_not_delivered" }),
                    Some(record),
                ),
            };
            let mut b = serde_json::to_vec(&status).map_err(|e| e.to_string())?;
            b.push(b'\n');
            out.insert(format!("{id}.result.json"), b);
            if let Some(rec) = record {
                let mut b = rec.record_canonical().map_err(|e| e.to_string())?;
                b.push(b'\n');
                out.insert(format!("{id}.record.json"), b);
            }
        }
        Ok(out)
    })
}
