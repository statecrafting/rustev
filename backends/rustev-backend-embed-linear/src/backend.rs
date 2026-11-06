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

use crate::cache::{
    CacheConfig, CacheLimits, CacheLookup, CacheMiss, CacheStats, EmbeddingCache,
    RepeatabilityEvidence, RunOutcome,
};
use crate::manifest::{EmbedManifest, EncoderKind, Platform};
use crate::onnx::OnnxEncoder;
use crate::{
    EmbedSetupError, PlanMismatch, PreparedRequest, RequestPreparer, SetupCode, VerifiedBundle,
};

/// The host's configuration (3.7.1). None of it is identity (3.5.2).
#[derive(Debug, Clone)]
pub struct EmbedConfig {
    pub bundle_dir: PathBuf,
    /// The absolute path of the ONNX Runtime library (3.6.1); unused by a
    /// synthetic bundle.
    pub runtime_path: Option<PathBuf>,
    pub expected_artifact: ArtifactId,
    pub workers: usize,
    pub max_queued: usize,
    /// The in-process embedding cache (3.14), off when `None`. Enabling it
    /// where in-process bitwise repeatability is not shown is refused
    /// `cache_not_equivalent`.
    pub embedding_cache: Option<CacheLimits>,
}

type Run = dyn Fn(&[u32], &CancelSignal) -> Result<Vec<f32>, ()> + Send + Sync;
type Output = Result<RawOutput, (AdapterFailure, String)>;
type Cache = Option<Arc<Mutex<EmbeddingCache>>>;
/// An `onnx` bundle's encoder run and the cache its setup decided.
type OnnxSetup = (Arc<Run>, Cache);

/// Local logits over a verified bundle, with bounded outstanding work.
pub struct EmbedLinearBackend {
    preparer: Arc<RequestPreparer>,
    descriptor: BackendDescriptor,
    pool: Arc<Pool>,
    cache: Cache,
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
        let (preparer, onnx) = Self::prepare(&config)?;
        let (run, cache) = match onnx {
            Some((run, cache)) => (run, Some(cache)),
            None => (default_run(&preparer)?, None),
        };
        Self::start(config, preparer, run, cache)
    }

    /// Synthetic fixtures only: substitute a controllable token-vector run.
    /// The runner receives the attempt signal so a fixture can model cooperative
    /// termination or a run that ignores it. Errors and panics become runtime_error.
    #[cfg(feature = "synthetic")]
    pub fn open_with_synthetic_runner(
        config: EmbedConfig,
        runner: impl Fn(&[u32], &CancelSignal) -> Result<Vec<f32>, ()> + Send + Sync + 'static,
    ) -> Result<Self, EmbedSetupError> {
        let (preparer, onnx) = Self::prepare(&config)?;
        let cache = onnx.map(|(_, cache)| cache);
        Self::start(config, preparer, Arc::new(runner), cache)
    }

    /// Verify the bundle; for an `onnx` bundle also decide the cache and
    /// load the encoder (`onnx_run`).
    fn prepare(
        config: &EmbedConfig,
    ) -> Result<(Arc<RequestPreparer>, Option<OnnxSetup>), EmbedSetupError> {
        if !(1..=16).contains(&config.workers) || config.max_queued > 1024 {
            return Err(setup(
                SetupCode::ManifestInvalid,
                "workers/max_queued",
                "invalid worker or queue limit",
            ));
        }
        let mut bundle = crate::verify_bundle(&config.bundle_dir, &config.expected_artifact)?;
        let onnx = match bundle.manifest.encoder.kind {
            EncoderKind::Onnx => Some(onnx_run(config, &mut bundle)?),
            EncoderKind::Synthetic => None,
        };
        Ok((Arc::new(RequestPreparer::new(bundle)?), onnx))
    }

    /// Start the workers. `cache` is the one an `onnx` bundle's setup
    /// decided; a synthetic bundle's is opened here.
    fn start(
        config: EmbedConfig,
        preparer: Arc<RequestPreparer>,
        run: Arc<Run>,
        cache: Option<Cache>,
    ) -> Result<Self, EmbedSetupError> {
        let cache = match cache {
            Some(cache) => cache,
            None => open_cache(&config, &preparer.bundle().manifest, None)?,
        };
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
            cache,
        };
        for n in 0..config.workers {
            let (pool, preparer, run, cache) = (
                backend.pool.clone(),
                backend.preparer.clone(),
                run.clone(),
                backend.cache.clone(),
            );
            std::thread::Builder::new()
                .name(format!("rustev-embed-{n}"))
                .spawn(move || worker(pool, preparer, run, cache))
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

    /// The embedding cache's counters (3.14.6); all zero when it is off.
    pub fn cache_stats(&self) -> CacheStats {
        self.cache
            .as_ref()
            .map_or_else(CacheStats::default, |c| lock(c).cache_stats())
    }

    /// Remove every cached embedding (3.14.5). A run that started before
    /// this call is not stored.
    pub fn clear_cache(&self) {
        if let Some(cache) = &self.cache {
            lock(cache).clear();
        }
    }

    /// Remove every cached embedding of this principal handle before
    /// returning (3.14.5). A run of any principal that started before this
    /// call is not stored.
    pub fn erase_principal(&self, handle: &[u8]) {
        if let Some(cache) = &self.cache {
            lock(cache).erase_principal(handle);
        }
    }
}

/// The cache of 3.14, when configured. The synthetic encoder is bitwise
/// repeatable by construction (3.19.1). An `onnx` bundle on the qualified
/// `platform` has evidence only when the committed record measured bitwise
/// in-process repeatability for its settings
/// ([`crate::runtime::repeatability_evidence`]); anywhere else, or with no
/// platform, enabling the cache is refused `cache_not_equivalent`.
fn open_cache(
    config: &EmbedConfig,
    manifest: &EmbedManifest,
    platform: Option<Platform>,
) -> Result<Cache, EmbedSetupError> {
    let Some(limits) = config.embedding_cache else {
        return Ok(None);
    };
    let evidence = match (manifest.encoder.kind, platform) {
        #[cfg(feature = "synthetic")]
        (EncoderKind::Synthetic, _) => RepeatabilityEvidence::Synthetic,
        (EncoderKind::Onnx, Some(platform)) => {
            let artifact = manifest
                .artifact_id()
                .map_err(|e| setup(SetupCode::ManifestInvalid, "manifest", &e.to_string()))?;
            crate::runtime::repeatability_evidence(manifest, &artifact, platform)
        }
        _ => RepeatabilityEvidence::Unproven,
    };
    let cache = EmbeddingCache::new(
        CacheConfig {
            embedding_cache: Some(limits),
        },
        manifest,
        platform.map_or("", Platform::key),
        &evidence,
    )?;
    Ok(Some(Arc::new(Mutex::new(cache))))
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
            // 3.14.2: the key is the post-truncation sequence and the
            // principal handle; heads apply after the lookup.
            let miss = match &self.cache {
                None => None,
                Some(cache) => match lock(cache).lookup(
                    &call.cx.principal_handle,
                    &request.ids,
                    &request.attention_mask,
                ) {
                    CacheLookup::Miss(miss) => Some(miss),
                    CacheLookup::Hit(embedding) => {
                        // 3.11.4: a hit is charged the per-call part only.
                        let output = head_output(self.preparer.bundle(), request.head, &embedding)
                            .map_err(|code| {
                                (AdapterFailure::Permanent, format!("embed:{code}:head"))
                            });
                        return report(output, cost.units_per_call, false);
                    }
                },
            };
            let units = cost.units_per_call + cost.units_per_token * request.ids.len() as u64;
            let job = Arc::new(Mutex::new(Job {
                miss,
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
    /// The cache ticket of a miss; the run's embedding is admitted with it.
    miss: Option<CacheMiss>,
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

fn worker(pool: Arc<Pool>, preparer: Arc<RequestPreparer>, run: Arc<Run>, cache: Cache) {
    loop {
        let (job, request, cancel, miss) = {
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
            let miss = j.miss.take();
            state.running += 1;
            drop(j);
            (job, request, cancel, miss)
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
            let output = head_output(bundle, request.head, &embedding)?;
            Ok((output, embedding))
        }))
        .unwrap_or(Err("runtime_error"))
        .map_err(|code| (AdapterFailure::Permanent, format!("embed:{code}:encoder")));
        let wake = {
            let mut state = lock(&pool.state);
            let mut j = lock(&job);
            state.running -= 1;
            j.phase = Phase::Done;
            let result = result.map(|(output, embedding)| {
                // 3.14.4: only a completed run whose attempt neither was
                // cancelled nor abandoned is admitted; the cache refuses a
                // ticket from before a `clear` or `erase_principal` (3.14.5).
                if let (Some(cache), Some(miss)) = (&cache, miss) {
                    let outcome = if j.abandoned || cancel.is_raised() {
                        RunOutcome::Cancelled
                    } else {
                        RunOutcome::Completed
                    };
                    lock(cache).admit(miss, &embedding, outcome);
                }
                output
            });
            if !j.abandoned {
                j.result = Some(result);
            }
            let wake = j.waker.take();
            drop(j);
            drop(job);
            drop(cancel);
            wake
        };
        if let Some(w) = wake {
            w.wake();
        }
    }
}

/// The head product over a pooled embedding, keyed by every option (3.9.3).
fn head_output(
    bundle: &crate::VerifiedBundle,
    head: usize,
    embedding: &[f32],
) -> Result<RawOutput, &'static str> {
    let logits = bundle.heads[head].logits(embedding).ok_or("non_finite")?;
    Ok(RawOutput::Logits(
        bundle.manifest.heads[head]
            .options
            .iter()
            .cloned()
            .zip(logits)
            .collect(),
    ))
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

/// The ONNX encoder of an `onnx` bundle (3.6, 3.9.1): the platform must be
/// in the qualified table before any runtime is touched; a configured cache
/// is decided next (3.14.3), so `cache_not_equivalent` needs no runtime;
/// then the runtime is verified and loaded and the session created from the
/// verified graph bytes, which are released afterwards. A single unpadded
/// sequence has an attention mask of ones.
fn onnx_run(
    config: &EmbedConfig,
    bundle: &mut VerifiedBundle,
) -> Result<OnnxSetup, EmbedSetupError> {
    let platform = crate::runtime::qualified_platform()?;
    let cache = open_cache(config, &bundle.manifest, Some(platform))?;
    let environment =
        crate::runtime::load(config.runtime_path.as_deref(), &bundle.manifest, platform)?;
    let encoder = OnnxEncoder::new(&environment, &bundle.graph, &bundle.manifest)?;
    bundle.graph = Vec::new();
    let run: Arc<Run> = Arc::new(move |ids, cancel| {
        encoder
            .encode(ids, &vec![1; ids.len()], cancel)
            .map_err(|_| ())
    });
    Ok((run, cache))
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
