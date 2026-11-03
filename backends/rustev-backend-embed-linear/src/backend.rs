//! Bounded synthetic dispatch, with handoff and completion serialized with cancellation.
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::ids::ArtifactId;
use rustev_contract::output::RawOutput;
use rustev_contract::plan::Plan;
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CancelAck, CancelSignal,
    DecisionBackend, RemoteEnd,
};

use crate::manifest::EncoderKind;
use crate::{EmbedSetupError, PlanMismatch, PreparedRequest, RequestPreparer, SetupCode};

/// Host settings for this increment. The embedding cache is not implemented.
#[derive(Debug, Clone)]
pub struct EmbedConfig {
    pub bundle_dir: PathBuf,
    /// Used only by ONNX bundles, whose encoder is not implemented yet.
    pub runtime_path: Option<PathBuf>,
    pub expected_artifact: ArtifactId,
    pub workers: usize,
    pub max_queued: usize,
}

type Run = dyn Fn(&[u32], &CancelSignal) -> Result<Vec<f32>, ()> + Send + Sync;
type Output = Result<RawOutput, (AdapterFailure, String)>;

/// Local logits over a verified bundle, with bounded outstanding work.
pub struct EmbedLinearBackend {
    preparer: Arc<RequestPreparer>,
    descriptor: BackendDescriptor,
    pool: Arc<Pool>,
}

fn setup(code: SetupCode, item: &str, detail: &str) -> EmbedSetupError {
    EmbedSetupError {
        code,
        item: item.into(),
        detail: detail.into(),
    }
}

impl EmbedLinearBackend {
    pub fn open(config: EmbedConfig) -> Result<Self, EmbedSetupError> {
        let preparer = Self::prepare(&config)?;
        Self::start(config, preparer.clone(), default_run(&preparer)?)
    }

    /// Synthetic fixtures only: substitute a controllable token-vector run.
    /// The runner receives the attempt signal so a fixture can model cooperative
    /// termination or a run that ignores it. Errors and panics become runtime_error.
    #[cfg(feature = "synthetic")]
    pub fn open_with_synthetic_runner(
        config: EmbedConfig,
        runner: impl Fn(&[u32], &CancelSignal) -> Result<Vec<f32>, ()> + Send + Sync + 'static,
    ) -> Result<Self, EmbedSetupError> {
        let preparer = Self::prepare(&config)?;
        Self::start(config, preparer, Arc::new(runner))
    }

    fn prepare(config: &EmbedConfig) -> Result<Arc<RequestPreparer>, EmbedSetupError> {
        if !(1..=16).contains(&config.workers) || config.max_queued > 1024 {
            return Err(setup(
                SetupCode::ManifestInvalid,
                "workers/max_queued",
                "invalid worker or queue limit",
            ));
        }
        let bundle = crate::verify_bundle(&config.bundle_dir, &config.expected_artifact)?;
        if bundle.manifest.encoder.kind == EncoderKind::Onnx {
            // The record passes native feasibility stages 1 to 3 on macOS,
            // but qualifies no ONNX platform (stage 4 and Node are pending).
            return Err(setup(
                SetupCode::UnqualifiedPlatform,
                "encoder.kind",
                "no ONNX platform is qualified; ONNX encoder is not implemented",
            ));
        }
        Ok(Arc::new(RequestPreparer::new(bundle)?))
    }

    fn start(
        config: EmbedConfig,
        preparer: Arc<RequestPreparer>,
        run: Arc<Run>,
    ) -> Result<Self, EmbedSetupError> {
        let pool = Arc::new(Pool {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                running: 0,
                shutdown: false,
            }),
            ready: Condvar::new(),
            capacity: config.workers + config.max_queued,
        });
        let backend = Self {
            descriptor: crate::derive_descriptor(
                &preparer.bundle().manifest,
                preparer.bundle().artifact.clone(),
            ),
            preparer,
            pool,
        };
        for n in 0..config.workers {
            let (pool, preparer, run) =
                (backend.pool.clone(), backend.preparer.clone(), run.clone());
            std::thread::Builder::new()
                .name(format!("rustev-embed-{n}"))
                .spawn(move || worker(pool, preparer, run))
                .map_err(|_| {
                    setup(
                        SetupCode::GraphRejected,
                        "workers",
                        "could not start worker",
                    )
                })?;
        }
        Ok(backend)
    }

    pub fn check_plan(&self, plan: &Plan) -> Result<(), Vec<PlanMismatch>> {
        self.preparer.check_plan(plan)
    }
}

impl Drop for EmbedLinearBackend {
    fn drop(&mut self) {
        lock(&self.pool.state).shutdown = true;
        self.pool.ready.notify_all();
        // Running threads own their state until they end. Never join an
        // uncooperative run here, and never release its occupied slot early.
    }
}

impl DecisionBackend for EmbedLinearBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }
    fn cost_model(&self) -> CostModel {
        CostModel::Bounded
    }
    fn cost_bound(&self, _: &[u8]) -> CostBound {
        crate::cost_bound(&self.preparer.bundle().manifest)
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        Box::pin(async move {
            if call.cx.remaining_ms.0 == 0 {
                return cancelled(0, false);
            }
            let request = match self.preparer.prepare(call.projection) {
                Ok(request) => request,
                Err(e) => return report(Err((AdapterFailure::Permanent, e.detail())), 0, false),
            };
            if call.cancel.is_raised() {
                return cancelled(0, false);
            }
            let cost = &self.preparer.bundle().manifest.cost;
            let units = cost.units_per_call + cost.units_per_token * request.ids.len() as u64;
            let job = Arc::new(Mutex::new(Job {
                request: Some(request),
                phase: Phase::Queued,
                result: None,
                cancel: call.cancel.child(),
                units,
                waker: None,
                abandoned: false,
            }));
            {
                let mut state = lock(&self.pool.state);
                if state.running + state.queue.len() == self.pool.capacity {
                    return report(
                        Err((
                            AdapterFailure::Overloaded,
                            "embed:queue_full:workers".into(),
                        )),
                        0,
                        false,
                    );
                }
                state.queue.push_back(job.clone());
            }
            self.pool.ready.notify_one();
            let pending = Pending {
                pool: &self.pool,
                job,
            };
            pending.await
        })
    }
}

struct Pool {
    state: Mutex<State>,
    ready: Condvar,
    capacity: usize,
}
struct State {
    queue: VecDeque<Arc<Mutex<Job>>>,
    running: usize,
    shutdown: bool,
}
#[derive(PartialEq)]
enum Phase {
    Queued,
    Running,
    Done,
}
struct Job {
    request: Option<PreparedRequest>,
    phase: Phase,
    result: Option<Output>,
    cancel: CancelSignal,
    units: u64,
    waker: Option<Waker>,
    abandoned: bool,
}
struct Pending<'a> {
    pool: &'a Pool,
    job: Arc<Mutex<Job>>,
}

impl Future for Pending<'_> {
    type Output = AttemptReport;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = lock(&self.pool.state);
        let mut job = lock(&self.job);
        let raised = job.cancel.poll_raised(cx).is_ready();
        if let Some(result) = job.result.take() {
            return Poll::Ready(report(
                result,
                if job.request.is_some() { 0 } else { job.units },
                raised,
            ));
        }
        if raised {
            let running = job.phase == Phase::Running;
            let units = if running { job.units } else { 0 };
            state.queue.retain(|j| !Arc::ptr_eq(j, &self.job));
            job.abandoned = true;
            job.waker = None;
            return Poll::Ready(cancelled(units, running));
        }
        job.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let mut state = lock(&self.pool.state);
        state.queue.retain(|j| !Arc::ptr_eq(j, &self.job));
        let mut job = lock(&self.job);
        job.abandoned = true;
        job.result = None;
        job.waker = None;
        // Raise outside locks: a task waker may immediately poll again.
        let cancel = job.cancel.clone();
        drop(job);
        drop(state);
        cancel.raise();
    }
}

fn worker(pool: Arc<Pool>, preparer: Arc<RequestPreparer>, run: Arc<Run>) {
    loop {
        let (job, request, cancel) = {
            let mut state = lock(&pool.state);
            while state.queue.is_empty() && !state.shutdown {
                state = pool.ready.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if state.shutdown {
                return;
            }
            let job = state.queue.pop_front().expect("nonempty queue");
            let mut j = lock(&job);
            if j.cancel.is_raised() {
                j.phase = Phase::Done;
                j.result = Some(Err((
                    AdapterFailure::Cancelled,
                    "embed:cancelled:queue".into(),
                )));
                let wake = j.waker.take();
                drop(j);
                drop(state);
                if let Some(w) = wake {
                    w.wake();
                }
                continue;
            }
            j.phase = Phase::Running;
            let request = j.request.take().expect("queued request");
            let cancel = j.cancel.clone();
            state.running += 1;
            drop(j);
            (job, request, cancel)
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let vectors = run(&request.ids, &cancel).map_err(|_| "runtime_error")?;
            let bundle = preparer.bundle();
            let embedding = crate::head::pool(
                &vectors,
                &request.attention_mask,
                bundle.manifest.encoder.hidden_size as usize,
                bundle.manifest.pooling.normalize,
            )
            .ok_or("non_finite")?;
            let logits = bundle.heads[request.head]
                .logits(&embedding)
                .ok_or("non_finite")?;
            Ok(RawOutput::Logits(
                bundle.manifest.heads[request.head]
                    .options
                    .iter()
                    .cloned()
                    .zip(logits)
                    .collect(),
            ))
        }))
        .unwrap_or(Err("runtime_error"))
        .map_err(|code| (AdapterFailure::Permanent, format!("embed:{code}:encoder")));
        drop(cancel);
        let wake = {
            let mut state = lock(&pool.state);
            let mut j = lock(&job);
            state.running -= 1;
            j.phase = Phase::Done;
            if !j.abandoned {
                j.result = Some(result);
            }
            let wake = j.waker.take();
            drop(j);
            drop(job);
            wake
        };
        if let Some(w) = wake {
            w.wake();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
fn report(result: Output, units: u64, raised: bool) -> AttemptReport {
    if raised && result.is_err() {
        return cancelled(units, false);
    }
    AttemptReport {
        result,
        charge: Charge::Observed { units },
        cancel: if raised {
            CancelAck::Stopped
        } else {
            CancelAck::NotRequested
        },
        remote: RemoteEnd::Finished,
    }
}
fn cancelled(units: u64, running: bool) -> AttemptReport {
    AttemptReport {
        result: Err((AdapterFailure::Cancelled, "embed:cancelled:attempt".into())),
        charge: Charge::Observed { units },
        cancel: if running {
            CancelAck::Unconfirmed
        } else {
            CancelAck::Stopped
        },
        remote: if running {
            RemoteEnd::PossiblyContinuing
        } else {
            RemoteEnd::Finished
        },
    }
}

fn default_run(preparer: &RequestPreparer) -> Result<Arc<Run>, EmbedSetupError> {
    #[cfg(feature = "synthetic")]
    {
        let encoder = crate::synthetic::SyntheticEncoder::for_bundle(preparer.bundle())
            .expect("setup checked synthetic kind");
        Ok(Arc::new(move |ids, _| Ok(encoder.encode(ids))))
    }
    #[cfg(not(feature = "synthetic"))]
    {
        let _ = preparer;
        Err(setup(
            SetupCode::FeatureDisabled,
            "synthetic",
            "enable the synthetic feature",
        ))
    }
}
