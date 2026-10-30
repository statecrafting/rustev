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

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rustev_contract::Document;
use rustev_contract::ids::ContentDigest;
use rustev_contract::remote::{CancelSupport, RemoteCode, WirePrincipal};
use rustev_contract::remote_context::{
    self as rc, CancelItem, ContextCode, FetchResult, HostedSource, InvalidationsResponse,
};
use rustev_contract::snapshot::Snapshot;
use rustev_core::seams::{BoxFuture, CallContext, ContextSource};
use tokio::sync::oneshot;

use crate::budget::Budget;
use crate::callback_server::{
    CONTEXT_CANCEL_PATH, CONTEXT_DESCRIBE_PATH, CONTEXT_FETCH_PATH, CONTEXT_INVALIDATIONS_PATH,
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
        code: Option<RemoteCode>,
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
            code: code.map(ContextCode),
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
