//! SYNTHETIC scripted backends and sinks (spec 003, 3.12; R-09). Test
//! fixtures with observable dispatch, completion, cancellation and cost; not
//! production backends, and no evidence of semantic quality (R-04).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::output::RawOutput;
use rustev_contract::run::{Charge, CostBound, CostModel, RunRecord};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BatchAttemptCall, BatchAttemptReport,
    BatchCapability, BatchMemberReport, BoxFuture, CancelAck, DecisionBackend, EvidenceSink,
    RemoteEnd,
};
use serde_json::Value as Json;
use tokio::sync::oneshot;

/// What a scripted attempt does.
#[derive(Debug, Clone)]
pub enum Answer {
    Output(RawOutput, Charge),
    Fail(AdapterFailure, String, Charge),
    /// Wait for [`Scripted::release`] or the attempt's cancellation signal.
    Gate,
    Panic,
}

/// What a waiting attempt does when its cancellation signal is raised.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnCancel {
    /// Acknowledge a confirmed stop with this charge.
    Stop(Charge),
    /// Answer without confirming a remote stop.
    Unconfirmed,
    /// Never answer.
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Dispatched(String),
    BatchDispatched(String, Vec<String>),
    Finished(String),
    CancelSeen(String),
    Dropped(String),
}

/// What the answer function sees.
pub struct Call {
    pub attempt_id: String,
    /// The attempt number within its request, from the attempt id.
    pub n: u32,
    pub projection: Json,
}

impl Call {
    pub fn task(&self) -> &str {
        self.projection["task"].as_str().unwrap_or_default()
    }
}

type AnswerFn = dyn Fn(&Call) -> Answer + Send + Sync;

pub struct Scripted {
    descriptor: BackendDescriptor,
    pub cost_model: Mutex<CostModel>,
    pub bound: Mutex<CostBound>,
    pub on_cancel: Mutex<OnCancel>,
    /// The remote end every report states (spec 013, 3.1).
    pub remote: Mutex<RemoteEnd>,
    /// Runs inside `cost_bound`, before it answers.
    pub before_bound: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    answer: Box<AnswerFn>,
    events: Mutex<Vec<Event>>,
    /// The principal handle each dispatched attempt was called with.
    principals: Mutex<Vec<Vec<u8>>>,
    gates: Mutex<BTreeMap<String, oneshot::Sender<Answer>>>,
    batch_enabled: Mutex<bool>,
    batch_omit_task: Mutex<Option<String>>,
}

impl Scripted {
    pub fn new(
        descriptor: BackendDescriptor,
        answer: impl Fn(&Call) -> Answer + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Scripted {
            descriptor,
            cost_model: Mutex::new(CostModel::Bounded),
            bound: Mutex::new(CostBound::Bounded { max_units: 1 }),
            on_cancel: Mutex::new(OnCancel::Ignore),
            remote: Mutex::new(RemoteEnd::Finished),
            before_bound: Mutex::new(None),
            answer: Box::new(answer),
            events: Mutex::new(vec![]),
            principals: Mutex::new(vec![]),
            gates: Mutex::new(BTreeMap::new()),
            batch_enabled: Mutex::new(false),
            batch_omit_task: Mutex::new(None),
        })
    }

    pub fn with_cost(self: Arc<Self>, model: CostModel, bound: CostBound) -> Arc<Self> {
        *self.cost_model.lock().unwrap() = model;
        *self.bound.lock().unwrap() = bound;
        self
    }

    pub fn with_remote(self: Arc<Self>, remote: RemoteEnd) -> Arc<Self> {
        *self.remote.lock().unwrap() = remote;
        self
    }

    pub fn with_on_cancel(self: Arc<Self>, on: OnCancel) -> Arc<Self> {
        *self.on_cancel.lock().unwrap() = on;
        self
    }

    pub fn with_batch(self: Arc<Self>) -> Arc<Self> {
        *self.batch_enabled.lock().unwrap() = true;
        self
    }

    pub fn with_batch_omitting(self: Arc<Self>, task: &str) -> Arc<Self> {
        *self.batch_enabled.lock().unwrap() = true;
        *self.batch_omit_task.lock().unwrap() = Some(task.into());
        self
    }

    pub fn principals(&self) -> Vec<Vec<u8>> {
        self.principals.lock().unwrap().clone()
    }

    pub fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }

    pub fn dispatched(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .flat_map(|e| match e {
                Event::Dispatched(a) => vec![a],
                Event::BatchDispatched(_, members) => members,
                _ => vec![],
            })
            .collect()
    }

    /// Attempt ids waiting at a gate.
    pub fn gated(&self) -> Vec<String> {
        self.gates.lock().unwrap().keys().cloned().collect()
    }

    /// Release a gated attempt with `answer`. False if it is not waiting.
    pub fn release(&self, attempt_id: &str, answer: Answer) -> bool {
        match self.gates.lock().unwrap().remove(attempt_id) {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }

    fn push(&self, e: Event) {
        self.events.lock().unwrap().push(e);
    }
}

fn report(
    result: Result<RawOutput, (AdapterFailure, String)>,
    charge: Charge,
    remote: RemoteEnd,
) -> AttemptReport {
    AttemptReport {
        result,
        charge,
        cancel: CancelAck::NotRequested,
        remote,
    }
}

struct DropWatch {
    backend: Arc<Scripted>,
    id: String,
    armed: bool,
}

impl Drop for DropWatch {
    fn drop(&mut self) {
        if self.armed {
            self.backend.push(Event::Dropped(self.id.clone()));
        }
    }
}

/// A backend handle that owns an `Arc` of the script, so futures can hold it.
pub struct Handle(pub Arc<Scripted>);

impl DecisionBackend for Handle {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.0.descriptor
    }

    fn cost_model(&self) -> CostModel {
        *self.0.cost_model.lock().unwrap()
    }

    fn cost_bound(&self, _projection: &[u8]) -> CostBound {
        if let Some(f) = &*self.0.before_bound.lock().unwrap() {
            f();
        }
        *self.0.bound.lock().unwrap()
    }

    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        let s = self.0.clone();
        let id = call.attempt_id.to_string();
        let n = id
            .rsplit('/')
            .next()
            .and_then(|x| x.parse().ok())
            .unwrap_or(0);
        let projection: Json = serde_json::from_slice(call.projection).unwrap_or(Json::Null);
        let signal = call.cancel.clone();
        s.push(Event::Dispatched(id.clone()));
        s.principals
            .lock()
            .unwrap()
            .push(call.cx.principal_handle.clone());
        let answer = (s.answer)(&Call {
            attempt_id: id.clone(),
            n,
            projection,
        });
        Box::pin(async move {
            let mut watch = DropWatch {
                backend: s.clone(),
                id: id.clone(),
                armed: true,
            };
            let answer = match answer {
                Answer::Gate => {
                    let (tx, mut rx) = oneshot::channel();
                    s.gates.lock().unwrap().insert(id.clone(), tx);
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
                            s.push(Event::CancelSeen(id.clone()));
                            let on = *s.on_cancel.lock().unwrap();
                            match on {
                                OnCancel::Stop(charge) => {
                                    watch.armed = false;
                                    s.push(Event::Finished(id.clone()));
                                    return AttemptReport {
                                        result: Err((AdapterFailure::Cancelled, "stopped".into())),
                                        charge,
                                        cancel: CancelAck::Stopped,
                                        remote: *s.remote.lock().unwrap(),
                                    };
                                }
                                OnCancel::Unconfirmed => {
                                    watch.armed = false;
                                    s.push(Event::Finished(id.clone()));
                                    return AttemptReport {
                                        result: Err((
                                            AdapterFailure::Cancelled,
                                            "stopped waiting".into(),
                                        )),
                                        charge: Charge::Unknown,
                                        cancel: CancelAck::Unconfirmed,
                                        remote: *s.remote.lock().unwrap(),
                                    };
                                }
                                OnCancel::Ignore => std::future::pending::<Answer>().await,
                            }
                        }
                    }
                }
                a => a,
            };
            let r = match answer {
                Answer::Output(o, c) => report(Ok(o), c, *s.remote.lock().unwrap()),
                Answer::Fail(f, d, c) => report(Err((f, d)), c, *s.remote.lock().unwrap()),
                Answer::Panic => panic!("scripted adapter panic"),
                Answer::Gate => std::future::pending::<AttemptReport>().await,
            };
            watch.armed = false;
            s.push(Event::Finished(id));
            r
        })
    }

    fn batch_capability(&self, _projection: &[u8]) -> Option<BatchCapability> {
        if *self.0.batch_enabled.lock().unwrap() {
            Some(BatchCapability {
                compatibility: "scripted-general-batch/1".into(),
                work_units: 1,
            })
        } else {
            None
        }
    }

    fn infer_batch<'a>(&'a self, call: BatchAttemptCall) -> BoxFuture<'a, BatchAttemptReport> {
        let s = self.0.clone();
        Box::pin(async move {
            let ids = call
                .members
                .iter()
                .map(|member| member.attempt_id.clone())
                .collect::<Vec<_>>();
            s.push(Event::BatchDispatched(call.batch_id, ids));
            let mut reports = Vec::new();
            let mut observed = 0_u64;
            let mut estimated = 0_u64;
            let mut unknown = false;
            for member in call.members {
                s.principals
                    .lock()
                    .unwrap()
                    .push(member.cx.principal_handle.clone());
                let projection: Json =
                    serde_json::from_slice(&member.projection).unwrap_or(Json::Null);
                let task = projection["task"].as_str().unwrap_or_default().to_string();
                let n = member
                    .attempt_id
                    .rsplit('/')
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let answer = (s.answer)(&Call {
                    attempt_id: member.attempt_id.clone(),
                    n,
                    projection,
                });
                if s.batch_omit_task.lock().unwrap().as_deref() == Some(task.as_str()) {
                    continue;
                }
                let (result, charge) = match answer {
                    Answer::Output(output, charge) => (Ok(output), charge),
                    Answer::Fail(failure, detail, charge) => (Err((failure, detail)), charge),
                    Answer::Gate => {
                        std::future::poll_fn(|cx| member.cancel.poll_raised(cx)).await;
                        s.push(Event::CancelSeen(member.attempt_id.clone()));
                        (
                            Err((AdapterFailure::Cancelled, "batch member stopped".into())),
                            Charge::Unknown,
                        )
                    }
                    Answer::Panic => panic!("scripted batch panic"),
                };
                match charge {
                    Charge::Observed { units } => observed = observed.saturating_add(units),
                    Charge::Estimated { units } => estimated = estimated.saturating_add(units),
                    Charge::Unknown => unknown = true,
                }
                reports.push(BatchMemberReport {
                    attempt_id: member.attempt_id,
                    result,
                });
            }
            let charge = if unknown || (observed > 0 && estimated > 0) {
                Charge::Unknown
            } else if estimated > 0 {
                Charge::Estimated { units: estimated }
            } else {
                Charge::Observed { units: observed }
            };
            BatchAttemptReport {
                result: Ok(reports),
                charge,
                cancel: CancelAck::NotRequested,
                remote: *s.remote.lock().unwrap(),
            }
        })
    }
}

/// What a scripted sink does with a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkAnswer {
    Ack,
    Fail,
    /// Never answer.
    Hang,
    Panic,
    /// Panic in `deliver` itself, before a future exists.
    PanicNow,
    /// Wait for [`ScriptedSink::open`].
    Gate,
}

pub struct ScriptedSink {
    pub answer: Mutex<SinkAnswer>,
    received: Mutex<Vec<RunRecord>>,
    gate: tokio::sync::Semaphore,
}

impl ScriptedSink {
    pub fn new(answer: SinkAnswer) -> Arc<Self> {
        Arc::new(ScriptedSink {
            answer: Mutex::new(answer),
            received: Mutex::new(vec![]),
            gate: tokio::sync::Semaphore::new(0),
        })
    }

    pub fn received(&self) -> Vec<RunRecord> {
        self.received.lock().unwrap().clone()
    }

    /// Let `n` gated deliveries through.
    pub fn open(&self, n: usize) {
        self.gate.add_permits(n);
    }
}

pub struct SinkHandle(pub Arc<ScriptedSink>);

impl EvidenceSink for SinkHandle {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        let s = self.0.clone();
        s.received.lock().unwrap().push(record.clone());
        let answer = *s.answer.lock().unwrap();
        if answer == SinkAnswer::PanicNow {
            panic!("scripted sink panic before a future");
        }
        let id = record.decision_id.clone();
        Box::pin(async move {
            match answer {
                SinkAnswer::Ack => Ok(format!("receipt:{id}")),
                SinkAnswer::Fail => Err("scripted sink failure".into()),
                SinkAnswer::Hang => std::future::pending().await,
                SinkAnswer::Panic => panic!("scripted sink panic"),
                SinkAnswer::PanicNow => unreachable!(),
                SinkAnswer::Gate => {
                    s.gate.acquire().await.unwrap().forget();
                    Ok(format!("receipt:{id}"))
                }
            }
        })
    }
}
