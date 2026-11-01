//! The `rustev.remote-context/1` HTTP client (spec 042, 3.10 and 3.12): a
//! `ContextSource` backed by a context source in another process.
//!
//! It describes the endpoint once, at construction, and never per decision;
//! an unknown version, a missing source or another endpoint class is a
//! capability failure there. Each fetch goes through the client's bounded
//! [`SeamQueue`]: a full queue fails at once with `remote_seam_queue_full`,
//! nothing is enqueued without bound, and a fetch carries the remaining
//! budget less the declared transit margin. A fetch is sent at most once per
//! attempt id `<decision id>/context/<n>`, with no invisible retry. When the
//! caller stops waiting, the exchange is cancelled at the endpoint (when it
//! offers cancellation) and an answer that still arrives is discarded,
//! counted and recorded as late; it never reaches a snapshot. Each exchange
//! yields a digest-only `rustev.remote-context-exchange/1` record to the
//! host's exchange sink, whose failures are counted and change nothing.
//!
//! When describe carries a source's spec 036 descriptor, the client also
//! yields a [`RemoteAcquisitionSource`]: an ordinary installed
//! `AcquisitionSource` sending `acquire` through the same queue (3.10.3).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rustev_contract::Document;
use rustev_contract::acquisition::{AcquisitionRequest, OperationCost};
use rustev_contract::ids::ContentDigest;
use rustev_contract::remote::{CancelSupport, MAX_TEXT_BYTES, RemoteCode, WirePrincipal, cut_to};
use rustev_contract::remote_context::{
    self as rc, AcquireCode, AcquireResult, CancelItem, FetchResult, HostedSource,
    InvalidationsResponse,
};
use rustev_contract::run::{Charge, CostBound};
use rustev_contract::serve::Embedded;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::source::SourceDescriptor;
use rustev_core::seams::{
    AcquireCall, AcquireReport, AcquisitionFailure, AcquisitionSource, AdapterFailure, BoxFuture,
    CallContext, CancelAck, ContextSource,
};
use tokio::sync::oneshot;

use crate::budget::Budget;
use crate::callback_server::{
    CONTEXT_ACQUIRE_PATH, CONTEXT_CANCEL_PATH, CONTEXT_DESCRIBE_PATH, CONTEXT_FETCH_PATH,
    CONTEXT_INVALIDATIONS_PATH,
};
use crate::http::{HttpTransport, TransportError};
use crate::seam_queue::{QUEUE_FULL, SeamCounters, SeamQueue};
use crate::taxonomy::{classify_status, failure_detail, is_local_only};
use crate::wire::SentTracker;

/// Describe answers and control documents are small.
const CONTROL_BYTES: usize = 1024 * 1024;
/// Attempt counters remembered at most; beyond it they restart, and a
/// reused attempt id with other content is refused by the endpoint
/// (`attempt_id_conflict`), never answered with another fetch's snapshot.
const MAX_COUNTERS: usize = 65_536;

/// How one client talks to one source.
#[derive(Debug, Clone)]
pub struct ContextClientConfig {
    /// The source id the endpoint describes.
    pub source_id: String,
    /// In-flight limit and queue capacity, each 1 to 65,536 (3.12.1).
    pub in_flight: u32,
    pub queue_capacity: u32,
    /// Subtracted from the remaining budget before it is sent (3.10.2).
    pub transit_margin_ms: u64,
    /// The budget of the describe exchange at construction.
    pub describe_budget_ms: u64,
    /// The principal reference's mapping version and declared scope (spec
    /// 020, 3.6); the reference is the call's principal handle.
    pub mapping_version: String,
    pub principal_scope: String,
}

/// Receives each exchange record; an error is counted, never propagated.
pub type ExchangeSink = Arc<dyn Fn(&rc::ExchangeRecord) -> Result<(), String> + Send + Sync>;

struct Inner {
    transport: HttpTransport,
    config: ContextClientConfig,
    source: HostedSource,
    describe_digest: ContentDigest,
    queue: SeamQueue,
    counters: Mutex<HashMap<String, u32>>,
    exchanges: Option<ExchangeSink>,
    exchange_failures: AtomicU64,
}

/// The client. Clones share the queue and counters.
#[derive(Clone)]
pub struct RemoteContextClient {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn transport_code(error: &TransportError) -> RemoteCode {
    match error {
        TransportError::Unsent(_) => RemoteCode::TransportUnsent,
        TransportError::Interrupted(_) => RemoteCode::TransportInterrupted,
        TransportError::TooLarge { .. } => RemoteCode::MalformedResponse,
        TransportError::Expired { sent: false } => RemoteCode::TransportUnsent,
        TransportError::Expired { sent: true } => RemoteCode::RemoteDeadline,
        TransportError::PhaseExpired(p) if p.bytes_sent => RemoteCode::RemoteDeadline,
        TransportError::PhaseExpired(_) => RemoteCode::TransportUnsent,
    }
}

/// Spec 003 3.9.2's escaping of an attempt id component.
fn escape(component: &str) -> String {
    component
        .replace('%', "%25")
        .replace('/', "%2F")
        .replace(',', "%2C")
}

impl RemoteContextClient {
    /// Describe the endpoint and select `config.source_id`.
    pub async fn connect(
        transport: HttpTransport,
        config: ContextClientConfig,
        exchanges: Option<ExchangeSink>,
    ) -> Result<Self, String> {
        let queue = SeamQueue::new(config.in_flight, config.queue_capacity)?;
        let request = rc::DescribeRequest {
            schema: rc::DESCRIBE.into(),
            versions: vec![1],
        }
        .record_canonical()
        .map_err(|e| e.to_string())?;
        let reply = transport
            .post(
                CONTEXT_DESCRIBE_PATH,
                request,
                CONTROL_BYTES,
                &Budget::from_ms(config.describe_budget_ms),
                &SentTracker::new(),
            )
            .await
            .map_err(|e| failure_detail(transport_code(&e), "describe"))?;
        if let Some(code) = classify_status(reply.status) {
            return Err(failure_detail(code, "describe"));
        }
        // An endpoint class other than host-operated does not parse.
        let described = rc::DescribeResponse::parse(&reply.body)
            .map_err(|e| failure_detail(RemoteCode::Capability, &e.to_string()))?;
        if described.version != 1 {
            return Err(failure_detail(RemoteCode::Capability, "unknown version"));
        }
        let Some(source) = described
            .sources
            .iter()
            .find(|s| s.source_id == config.source_id)
            .cloned()
        else {
            return Err(failure_detail(
                RemoteCode::Capability,
                "source not described",
            ));
        };
        let describe_digest = described.record_digest().map_err(|e| e.to_string())?;
        Ok(RemoteContextClient {
            inner: Arc::new(Inner {
                transport,
                config,
                source,
                describe_digest,
                queue,
                counters: Mutex::new(HashMap::new()),
                exchanges,
                exchange_failures: AtomicU64::new(0),
            }),
        })
    }

    /// What the endpoint described for this source.
    pub fn source(&self) -> &HostedSource {
        &self.inner.source
    }

    pub fn describe_digest(&self) -> &ContentDigest {
        &self.inner.describe_digest
    }

    /// Queue depth, refusals and late discards (3.12.4).
    pub fn counters(&self) -> SeamCounters {
        self.inner.queue.counters()
    }

    /// Exchange records the host's exchange sink failed to take.
    pub fn exchange_failures(&self) -> u64 {
        self.inner.exchange_failures.load(Ordering::Relaxed)
    }

    /// Ask the endpoint to cancel attempts (3.10.4).
    pub async fn cancel(
        &self,
        attempt_ids: Vec<String>,
        budget_ms: u64,
    ) -> Result<Vec<CancelItem>, String> {
        let body = rc::CancelRequest {
            schema: rc::CANCEL.into(),
            attempt_ids,
        }
        .record_canonical()
        .map_err(|e| e.to_string())?;
        let reply = self
            .inner
            .transport
            .post(
                CONTEXT_CANCEL_PATH,
                body,
                CONTROL_BYTES,
                &Budget::from_ms(budget_ms),
                &SentTracker::new(),
            )
            .await
            .map_err(|e| failure_detail(transport_code(&e), "cancel"))?;
        if let Some(code) = classify_status(reply.status) {
            return Err(failure_detail(code, "cancel"));
        }
        let answers = rc::CancelResponse::parse(&reply.body)
            .map_err(|e| failure_detail(RemoteCode::MalformedResponse, &e.to_string()))?
            .answers;
        if self.inner.source.terms.cancellation != CancelSupport::Confirmed
            && answers
                .iter()
                .any(|a| a.answer == rc::CancelAnswer::Stopped)
        {
            return Err(failure_detail(
                RemoteCode::MalformedResponse,
                "stopped without confirmed cancellation",
            ));
        }
        Ok(answers)
    }

    /// Poll invalidations after `sequence`, or the baseline with `None`
    /// (3.10.6). Applying them is the caller's.
    pub async fn invalidations(
        &self,
        sequence: Option<u64>,
        budget_ms: u64,
    ) -> Result<InvalidationsResponse, String> {
        let body = rc::InvalidationsRequest {
            schema: rc::INVALIDATIONS.into(),
            sequence,
        }
        .record_canonical()
        .map_err(|e| e.to_string())?;
        let reply = self
            .inner
            .transport
            .post(
                CONTEXT_INVALIDATIONS_PATH,
                body,
                CONTROL_BYTES,
                &Budget::from_ms(budget_ms),
                &SentTracker::new(),
            )
            .await
            .map_err(|e| failure_detail(transport_code(&e), "invalidations"))?;
        if let Some(code) = classify_status(reply.status) {
            return Err(failure_detail(code, "invalidations"));
        }
        let answer = InvalidationsResponse::parse(&reply.body)
            .map_err(|e| failure_detail(RemoteCode::MalformedResponse, &e.to_string()))?;
        let mut last = sequence;
        for item in &answer.items {
            if last.is_some_and(|l| item.sequence <= l) {
                return Err(failure_detail(
                    RemoteCode::MalformedResponse,
                    "sequence not increasing",
                ));
            }
            last = Some(item.sequence);
        }
        if sequence.is_some_and(|s| answer.sequence < s)
            || last.is_some_and(|l| answer.sequence < l)
        {
            return Err(failure_detail(
                RemoteCode::MalformedResponse,
                "sequence went back",
            ));
        }
        Ok(answer)
    }

    fn next_attempt(&self, decision: &str) -> String {
        let mut counters = lock(&self.inner.counters);
        if counters.len() >= MAX_COUNTERS && !counters.contains_key(decision) {
            counters.clear();
        }
        let n = counters.entry(decision.to_string()).or_insert(0);
        *n += 1;
        format!("{}/context/{}", escape(decision), n)
    }

    fn record(
        &self,
        attempt_id: &str,
        request: Option<ContentDigest>,
        response: Option<ContentDigest>,
        code: Option<AcquireCode>,
        late: bool,
    ) {
        let Some(sink) = &self.inner.exchanges else {
            return;
        };
        let record = rc::ExchangeRecord {
            schema: rc::EXCHANGE.into(),
            attempt_id: attempt_id.into(),
            request_digest: request,
            response_digest: response,
            code,
            phases: vec![],
            late,
        };
        if sink(&record).is_err() {
            self.inner.exchange_failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn fetch_once(&self, fields: &[String], cx: &CallContext) -> Result<Snapshot, String> {
        let inner = &self.inner;
        let slot = inner
            .queue
            .try_enter()
            .map_err(|_| QUEUE_FULL.to_string())?;
        let Some(principal) = WirePrincipal::encode(
            &cx.principal_handle,
            &inner.config.mapping_version,
            &inner.config.principal_scope,
        ) else {
            return Err(failure_detail(
                RemoteCode::MalformedRequest,
                "no principal reference",
            ));
        };
        let budget = Budget::start(cx.remaining_ms);
        let told = budget.told_ms(inner.config.transit_margin_ms);
        if told == 0 {
            return Err(failure_detail(
                RemoteCode::RemoteDeadline,
                "budget spent before sending",
            ));
        }
        let attempt_id = self.next_attempt(&cx.trace_id);
        let request = rc::FetchRequest {
            schema: rc::FETCH.into(),
            version: 1,
            source_id: inner.config.source_id.clone(),
            describe_digest: inner.describe_digest.clone(),
            attempt_id: attempt_id.clone(),
            budget_ms: told,
            trace_id: cx.trace_id.clone(),
            traceparent: None,
            principal,
            fields: fields.to_vec(),
        };
        let request_digest = request.record_digest().ok();
        let body = request.record_canonical().map_err(|e| e.to_string())?;
        if body.len() as u64 > inner.source.terms.max_request_bytes {
            return Err(failure_detail(
                RemoteCode::MalformedRequest,
                "request exceeds the source's bound",
            ));
        }
        let (tx, rx) = oneshot::channel();
        let client = self.clone();
        let wanted = fields.to_vec();
        let id = attempt_id.clone();
        tokio::spawn(async move {
            let inner = &client.inner;
            let Ok(_running) = slot.start(budget.deadline()).await else {
                let _ = tx.send(Err(failure_detail(
                    RemoteCode::TransportUnsent,
                    "queue_deadline",
                )));
                return;
            };
            let max = inner.source.terms.max_response_bytes.min(usize::MAX as u64) as usize;
            let reply = inner
                .transport
                .post(CONTEXT_FETCH_PATH, body, max, &budget, &SentTracker::new())
                .await;
            let (result, response_digest, answered) = match reply {
                Err(e) => (Err(failure_detail(transport_code(&e), "")), None, false),
                Ok(r) => match classify_status(r.status) {
                    Some(code) => (Err(failure_detail(code, "")), None, true),
                    None => {
                        let (result, digest) = judge(&r.body, &wanted);
                        (result, digest, true)
                    }
                },
            };
            let code = result
                .as_ref()
                .err()
                .map(|d| crate::taxonomy::code_of_detail(d).unwrap_or(RemoteCode::RemoteError));
            let late = match tx.send(result) {
                Ok(()) => false,
                Err(_) if answered => {
                    inner.queue.count_late();
                    true
                }
                Err(_) => false,
            };
            let code = code.map(AcquireCode::from);
            client.record(&id, request_digest, response_digest, code, late);
        });
        let mut guard = Abandon {
            client: self.clone(),
            attempt_id,
            armed: true,
        };
        let result = rx
            .await
            .unwrap_or_else(|_| Err(failure_detail(RemoteCode::RemoteError, "exchange ended")));
        guard.armed = false;
        result
    }
}

/// The answer's snapshot, or the failure it names.
fn judge(body: &[u8], wanted: &[String]) -> (Result<Snapshot, String>, Option<ContentDigest>) {
    let malformed = |why: &str| Err(failure_detail(RemoteCode::MalformedResponse, why));
    let Ok(answer) = rc::FetchResponse::parse(body) else {
        return (malformed("unparsable answer"), None);
    };
    let digest = answer.record_digest().ok();
    let result = match answer.result {
        FetchResult::Failure { code, .. } if is_local_only(code.0) => {
            malformed("a local-only code from the endpoint")
        }
        FetchResult::Failure { code, detail } => Err(failure_detail(code.0, &detail)),
        FetchResult::Snapshot(s) if s.entries.iter().any(|e| !wanted.contains(&e.field)) => {
            malformed("an unrequested field")
        }
        FetchResult::Snapshot(s) => Ok(s),
    };
    (result, digest)
}

/// Cancels at the endpoint when the caller stops waiting.
struct Abandon {
    client: RemoteContextClient,
    attempt_id: String,
    armed: bool,
}

impl Drop for Abandon {
    fn drop(&mut self) {
        if !self.armed || self.client.inner.source.terms.cancellation == CancelSupport::None {
            return;
        }
        let client = self.client.clone();
        let id = std::mem::take(&mut self.attempt_id);
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async move {
                let _ = client.cancel(vec![id], 1_000).await;
            });
        }
    }
}

impl ContextSource for RemoteContextClient {
    fn fetch<'a>(
        &'a self,
        fields: &'a [String],
        cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>> {
        Box::pin(self.fetch_once(fields, cx))
    }
}

/// One described source's acquisitions (3.10.3): an ordinary installed
/// `AcquisitionSource` whose descriptor is exactly the one describe
/// returned. It validates nothing spec 036 3.3.4 validates: a page's bytes
/// go to the runtime as the endpoint sent them. Clones share the client's
/// queue, so fetches and acquisitions share its in-flight limit (3.12.1).
#[derive(Clone)]
pub struct RemoteAcquisitionSource {
    client: RemoteContextClient,
    descriptor: Arc<SourceDescriptor>,
}

impl RemoteContextClient {
    /// The source's acquisitions, when describe carried its descriptor.
    pub fn acquisition_source(&self) -> Option<RemoteAcquisitionSource> {
        let hosted = self.inner.source.descriptor.as_ref()?;
        // Describe parsing already checked the document and its identity.
        let descriptor = hosted.document.document().ok()?;
        Some(RemoteAcquisitionSource {
            client: self.clone(),
            descriptor: Arc::new(descriptor),
        })
    }
}

/// The failure class an acquire code maps to (3.10.3): `unauthorized` and
/// `denied` are `denied`, never retried; the others as spec 009 3.6 maps
/// them (a remote `cancelled` the caller did not ask for is transient).
fn acquire_class(code: AcquireCode) -> AcquisitionFailure {
    match code {
        AcquireCode::Denied | AcquireCode::Context(rc::ContextCode(RemoteCode::Unauthorized)) => {
            AcquisitionFailure::Denied
        }
        AcquireCode::Context(rc::ContextCode(code)) => match crate::taxonomy::failure_class(code) {
            AdapterFailure::Overloaded => AcquisitionFailure::Overloaded,
            AdapterFailure::Permanent => AcquisitionFailure::Permanent,
            _ => AcquisitionFailure::Transient,
        },
    }
}

fn acquire_detail(code: AcquireCode, detail: &str) -> String {
    match code {
        AcquireCode::Context(rc::ContextCode(code)) => failure_detail(code, detail),
        AcquireCode::Denied if detail.is_empty() => "remote:denied".into(),
        AcquireCode::Denied => cut_to(&format!("remote:denied: {detail}"), MAX_TEXT_BYTES),
    }
}

/// A failed attempt, before it becomes a report.
struct Failed {
    code: AcquireCode,
    detail: String,
    charge: Charge,
}

impl Failed {
    fn local(code: RemoteCode, detail: &str, sent: bool) -> Self {
        Failed {
            code: code.into(),
            detail: detail.into(),
            // Nothing that never left the process was charged.
            charge: if sent {
                crate::taxonomy::charge_for(code, None, true)
            } else {
                Charge::Observed { units: 0 }
            },
        }
    }
    fn report(self) -> AcquireReport {
        AcquireReport {
            result: Err((
                acquire_class(self.code),
                acquire_detail(self.code, &self.detail),
            )),
            charge: self.charge,
            cancel: CancelAck::NotRequested,
        }
    }
}

type Answer = Result<(Vec<u8>, Charge), Failed>;

/// The answer's page bytes and charge, or the failure it names.
fn judge_acquire(body: &[u8]) -> (Answer, Option<ContentDigest>) {
    let Ok(answer) = rc::AcquireResponse::parse(body) else {
        return (
            Err(Failed::local(
                RemoteCode::MalformedResponse,
                "unparsable answer",
                true,
            )),
            None,
        );
    };
    let digest = answer.record_digest().ok();
    let result = match answer.result {
        AcquireResult::Failure {
            code: AcquireCode::Context(rc::ContextCode(code)),
            ..
        } if is_local_only(code) => Err(Failed::local(
            RemoteCode::MalformedResponse,
            "a local-only code from the endpoint",
            true,
        )),
        AcquireResult::Failure { code, detail } => Err(Failed {
            code,
            detail,
            charge: answer.charge,
        }),
        AcquireResult::Page(page) => Ok((page.bytes().to_vec(), answer.charge)),
    };
    (result, digest)
}

impl RemoteAcquisitionSource {
    pub fn descriptor_id(&self) -> &rustev_contract::ids::SourceId {
        &self
            .client
            .inner
            .source
            .descriptor
            .as_ref()
            .expect("an acquisition source has a descriptor")
            .id
    }

    /// The shared client queue's depth, refusals and late discards.
    pub fn counters(&self) -> SeamCounters {
        self.client.counters()
    }

    async fn acquire_once(&self, call: AcquireCall<'_>) -> AcquireReport {
        let inner = &self.client.inner;
        let Ok(slot) = inner.queue.try_enter() else {
            // Spec 036's unavailable outcome with the queue's reason (3.12.1).
            return AcquireReport {
                result: Err((AcquisitionFailure::Overloaded, QUEUE_FULL.into())),
                charge: Charge::Observed { units: 0 },
                cancel: CancelAck::NotRequested,
            };
        };
        let Some(principal) = WirePrincipal::encode(
            &call.cx.principal_handle,
            &inner.config.mapping_version,
            &inner.config.principal_scope,
        ) else {
            return Failed::local(
                RemoteCode::MalformedRequest,
                "no principal reference",
                false,
            )
            .report();
        };
        let budget = Budget::start(call.cx.remaining_ms);
        let told = budget.told_ms(inner.config.transit_margin_ms);
        if told == 0 {
            return Failed::local(
                RemoteCode::RemoteDeadline,
                "budget spent before sending",
                false,
            )
            .report();
        }
        let request = match call
            .request
            .canonical()
            .map_err(|e| e.to_string())
            .and_then(|b| Embedded::<AcquisitionRequest>::parse(&b).map_err(|e| e.to_string()))
        {
            Ok(r) => r,
            Err(e) => return Failed::local(RemoteCode::MalformedRequest, &e, false).report(),
        };
        let request = rc::AcquireRequest {
            schema: rc::ACQUIRE.into(),
            version: 1,
            source: call.request.source.clone(),
            operation: call.operation.into(),
            request,
            request_id: call.request_id.clone(),
            attempt_id: call.attempt_id.into(),
            budget_ms: told,
            trace_id: call.cx.trace_id.clone(),
            traceparent: None,
            principal,
        };
        let request_digest = request.record_digest().ok();
        let body = match request.record_canonical() {
            Ok(b) if b.len() as u64 <= inner.source.terms.max_request_bytes => b,
            Ok(_) => {
                return Failed::local(
                    RemoteCode::MalformedRequest,
                    "request exceeds the source's bound",
                    false,
                )
                .report();
            }
            Err(e) => {
                return Failed::local(RemoteCode::MalformedRequest, &e.to_string(), false).report();
            }
        };
        let (tx, rx) = oneshot::channel::<Answer>();
        let client = self.client.clone();
        let id = call.attempt_id.to_string();
        let sent = SentTracker::new();
        let tracker = sent.clone();
        tokio::spawn(async move {
            let inner = &client.inner;
            let Ok(_running) = slot.start(budget.deadline()).await else {
                let _ = tx.send(Err(Failed::local(
                    RemoteCode::TransportUnsent,
                    "queue_deadline",
                    false,
                )));
                return;
            };
            let max = inner.source.terms.max_response_bytes.min(usize::MAX as u64) as usize;
            let reply = inner
                .transport
                .post(CONTEXT_ACQUIRE_PATH, body, max, &budget, &tracker)
                .await;
            let (result, response_digest, answered) = match reply {
                Err(e) => {
                    let code = transport_code(&e);
                    (
                        Err(Failed::local(code, "", tracker.may_have_sent())),
                        None,
                        false,
                    )
                }
                Ok(r) => match classify_status(r.status) {
                    Some(code) => (Err(Failed::local(code, "", true)), None, true),
                    None => {
                        let (result, digest) = judge_acquire(&r.body);
                        (result, digest, true)
                    }
                },
            };
            let code = result.as_ref().err().map(|f| f.code);
            let late = match tx.send(result) {
                Ok(()) => false,
                Err(_) if answered => {
                    inner.queue.count_late();
                    true
                }
                Err(_) => false,
            };
            client.record(&id, request_digest, response_digest, code, late);
        });
        let mut guard = Abandon {
            client: self.client.clone(),
            attempt_id: call.attempt_id.to_string(),
            armed: true,
        };
        let answer = tokio::select! {
            answer = rx => answer.unwrap_or_else(|_| {
                Err(Failed::local(RemoteCode::RemoteError, "exchange ended", true))
            }),
            () = call.cancel.raised() => {
                guard.armed = false;
                return self.cancelled(&sent, call.attempt_id).await;
            }
        };
        guard.armed = false;
        match answer {
            Ok((page, charge)) => AcquireReport {
                result: Ok(page),
                charge,
                cancel: CancelAck::NotRequested,
            },
            Err(failed) => failed.report(),
        }
    }

    /// The caller cancelled: nothing was sent, or the endpoint is asked to
    /// stop and only a confirmed `stopped` is reported as one (3.10.4).
    async fn cancelled(&self, sent: &SentTracker, attempt_id: &str) -> AcquireReport {
        let report = |cancel, charge| AcquireReport {
            result: Err((
                AcquisitionFailure::Transient,
                failure_detail(RemoteCode::Cancelled, ""),
            )),
            charge,
            cancel,
        };
        if sent.try_abort() {
            return report(CancelAck::Stopped, Charge::Observed { units: 0 });
        }
        if self.client.inner.source.terms.cancellation == CancelSupport::None {
            return report(CancelAck::Unconfirmed, Charge::Unknown);
        }
        let support = self.client.inner.source.terms.cancellation;
        let ack = match self
            .client
            .cancel(vec![attempt_id.to_string()], 1_000)
            .await
        {
            Ok(answers) => remote_cancel_ack(support, &answers, attempt_id),
            Err(_) => CancelAck::Unconfirmed,
        };
        report(ack, Charge::Unknown)
    }
}

/// The acknowledgement of a sent attempt's cancel: `stopped` only when the
/// endpoint declared `confirmed` cancellation and answered `stopped` for
/// this attempt; `best_effort` never yields `stopped` (3.10.4).
fn remote_cancel_ack(
    support: CancelSupport,
    answers: &[CancelItem],
    attempt_id: &str,
) -> CancelAck {
    let stopped = answers
        .iter()
        .any(|a| a.attempt_id == attempt_id && a.answer == rc::CancelAnswer::Stopped);
    if support == CancelSupport::Confirmed && stopped {
        CancelAck::Stopped
    } else {
        CancelAck::Unconfirmed
    }
}

impl AcquisitionSource for RemoteAcquisitionSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    /// The operation's declared bound; an estimated or unknown cost model
    /// states no units, so it is `unknown`.
    fn cost_bound(&self, request: &AcquisitionRequest) -> CostBound {
        let declared = self
            .descriptor
            .operations
            .iter()
            .flatten()
            .find(|o| o.name == request.operation)
            .map(|o| &o.cost);
        match declared {
            Some(OperationCost::Bounded { max_units }) => CostBound::Bounded {
                max_units: *max_units,
            },
            _ => CostBound::Unknown,
        }
    }

    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        Box::pin(self.acquire_once(call))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(id: &str, answer: rc::CancelAnswer) -> CancelItem {
        CancelItem {
            attempt_id: id.into(),
            answer,
        }
    }

    #[test]
    fn stopped_needs_confirmed_support_and_this_attempts_stop() {
        let stopped = [answer("a", rc::CancelAnswer::Stopped)];
        assert_eq!(
            remote_cancel_ack(CancelSupport::Confirmed, &stopped, "a"),
            CancelAck::Stopped
        );
        // Best effort never yields stopped, even when the endpoint says so.
        assert_eq!(
            remote_cancel_ack(CancelSupport::BestEffort, &stopped, "a"),
            CancelAck::Unconfirmed
        );
        assert_eq!(
            remote_cancel_ack(CancelSupport::None, &stopped, "a"),
            CancelAck::Unconfirmed
        );
        // Another attempt's stop says nothing about this one.
        assert_eq!(
            remote_cancel_ack(CancelSupport::Confirmed, &stopped, "b"),
            CancelAck::Unconfirmed
        );
        let unconfirmed = [answer("a", rc::CancelAnswer::Unconfirmed)];
        assert_eq!(
            remote_cancel_ack(CancelSupport::Confirmed, &unconfirmed, "a"),
            CancelAck::Unconfirmed
        );
    }
}
