//! Callback protocol conformance (spec 042, 3.17.3) for
//! `rustev.remote-context/1` and `rustev.remote-sink/1`.
//!
//! The suite is split by who is tested. The endpoint suite checks an
//! endpoint's own behavior: describe and its versions, request and response
//! bounds, idempotency windows (repeat, conflict, expiry), invalidation
//! polling and its gaps, and duplicate and conflicting deliveries. It runs
//! against the Rust host server (`callback_server`) here, and against a
//! host's own endpoint by URL:
//!
//! ```text
//! RUSTEV_CALLBACK_ENDPOINT=https://host.example/base \
//! RUSTEV_CALLBACK_BEARER_FILE=/path/to/token \
//!   cargo test -p rustev-serve --lib conformance::callback:: -- --ignored
//! ```
//!
//! The endpoint under test hosts at least one of the two protocols. For the
//! window cases it is configured with an idempotency window and a
//! deduplication window of at most 5,000 ms, and for the record bound case a
//! `max_record_bytes` below 4,061 bytes (the larger fixture record), and
//! accepts `rustev.run/1` records. Fetches are made for the principal
//! reference `user-7` (mapping `m1`, scope `tenant`); the endpoint may answer
//! them with any failure it would give that principal. Without a driver (a
//! host endpoint by URL), the invalidation case checks the structure of the
//! polls the endpoint answers rather than publishing items itself.
//!
//! The client suite checks the clients against a scripted endpoint in this
//! crate: timeouts at each phase, late answers, oversized responses, unknown
//! versions and cancellation outcomes.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustev_contract::Document;
use rustev_contract::definition::{ProvenanceClass, TypeDecl};
use rustev_contract::ids::ContentDigest;
use rustev_contract::remote::{CancelSupport, RemoteCode, WirePrincipal};
use rustev_contract::remote_context::{
    self as rc, CancelAnswer, ContextField, ContextTerms, EndpointClass, ExchangeRecord,
    FetchResult, Freshness, HostedSource, InvalidationKind, InvalidationScope, InvalidationSupport,
    Staleness,
};
use rustev_contract::remote_sink::{self as rs, DeliveryResult, RecordSchema, Signer};
use rustev_contract::run::RunRecord;
use rustev_contract::scope::Handle;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::DurationMs;
use rustev_core::seams::{CallContext, ContextSource};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{CaseSource, Keep};
use crate::budget::Budget;
use crate::callback_server::{
    CONTEXT_CANCEL_PATH, CONTEXT_DESCRIBE_PATH, CONTEXT_FETCH_PATH, CONTEXT_INVALIDATIONS_PATH,
    CallbackConfig, CallbackServer, HostedContext, HostedSink, SINK_DELIVER_PATH,
    SINK_DESCRIBE_PATH,
};
use crate::context_client::{ContextClientConfig, RemoteContextClient};
use crate::http::{Endpoint, HttpReply, HttpTransport, Secret};
use crate::phases::{PhasePolicy, TransportPhases};
use crate::sinks::RemoteSink;
use crate::taxonomy::code_of_detail;
use crate::wire::SentTracker;

const SNAPSHOT: &[u8] = include_bytes!("../../../rustev-otel/tests/fixtures/support.snapshot.json");
/// 4,061 bytes: above the record bound the Rust host is given.
const RECORD_LARGE: &[u8] = include_bytes!(
    "../../../../tools/rustev-conformance/expected/runtime/runtime.budget/d1.record.json"
);
/// Two small records with different digests.
const RECORD_A: &[u8] = include_bytes!(
    "../../../../tools/rustev-conformance/expected/runtime/runtime.cancel/d1.record.json"
);
const RECORD_B: &[u8] = include_bytes!(
    "../../../../tools/rustev-conformance/expected/runtime/runtime.cancel-stop/d1.record.json"
);
/// The longest window a window case waits out.
const MAX_WAIT_MS: u64 = 5_000;
/// The largest request the oversized-request case sends.
const MAX_SENT_BYTES: u64 = 32 * 1024 * 1024;
const CONTROL_BYTES: usize = 1024 * 1024;

type Outcome = Result<(), String>;

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn principal() -> WirePrincipal {
    WirePrincipal::encode(b"user-7", "m1", "tenant").expect("principal reference")
}

fn field(name: &str) -> ContextField {
    ContextField {
        name: name.into(),
        ty: TypeDecl::Text { max_bytes: 1024 },
        provenance: ProvenanceClass::UserSupplied,
        freshness: Freshness::Observation {
            staleness: Staleness::Unknown,
        },
        grade: None,
    }
}

/// A run-unique tag, so a rerun against a long-lived endpoint reuses no id.
fn tag() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("cc{nanos}")
}

/// `bytes` as a run record for `decision_id`: its canonical bytes and digest.
fn record(bytes: &[u8], decision_id: &str) -> (RunRecord, String, ContentDigest) {
    let mut run = RunRecord::parse(bytes).expect("fixture record");
    run.decision_id = decision_id.into();
    let canonical = run.record_canonical().expect("canonical record");
    let run = RunRecord::parse(&canonical).expect("renamed record");
    let digest = run.record_digest().expect("record digest");
    (run, String::from_utf8(canonical).expect("utf-8"), digest)
}

fn deliver_body(bytes: &[u8], decision_id: &str, version: u32) -> (Vec<u8>, ContentDigest) {
    let (_, record, digest) = record(bytes, decision_id);
    let body = rs::DeliverRequest {
        schema: rs::DELIVER.into(),
        version,
        decision_id: Handle::new(decision_id).expect("decision id"),
        record_schema: RecordSchema::V1,
        record,
        record_digest: digest.clone(),
        registration_id: digest.clone(),
        signer: Signer::Unsigned,
        service_build: "conformance".into(),
        traceparent: None,
    }
    .record_canonical()
    .expect("deliver request");
    (body, digest)
}

/// What a run of the endpoint suite may do beyond the protocol. A host's
/// own endpoint reached by URL has none.
trait Driver: Send + Sync {
    /// Publish one invalidation item and return its sequence number.
    fn publish(&self, source_id: &str, field: &str) -> u64;
    /// Invalidation items the endpoint retains for pollers.
    fn retained(&self) -> usize;
    /// Hosted calls started so far.
    fn dispatched(&self) -> u64;
}

struct Target {
    transport: HttpTransport,
    driver: Option<Box<dyn Driver>>,
    tag: String,
}

impl Target {
    async fn post(&self, path: &str, body: Vec<u8>, max: usize) -> Result<HttpReply, String> {
        self.transport
            .post(
                path,
                body,
                max,
                &Budget::from_ms(10_000),
                &SentTracker::new(),
            )
            .await
            .map_err(|e| format!("{path}: {e:?}"))
    }
}

#[derive(Default)]
struct Report {
    ran: BTreeSet<String>,
    failures: Vec<String>,
}

impl Report {
    fn note(&mut self, case: &str, outcome: Outcome) {
        self.ran.insert(case.into());
        if let Err(e) = outcome {
            self.failures.push(format!("{case}: {e}"));
        }
    }
}

fn is_client_error(status: u16) -> bool {
    (400..500).contains(&status)
}

/// Every endpoint-side case the endpoint's describe answers make
/// applicable.
async fn endpoint_suite(t: &Target) -> Report {
    let mut report = Report::default();
    match context_describe(t).await {
        Err(e) => report.note("context.describe", Err(e)),
        Ok(None) => eprintln!("callback conformance: the context protocol is not hosted"),
        Ok(Some(d)) => {
            report.note("context.describe", Ok(()));
            context_cases(t, &d, &mut report).await;
        }
    }
    match sink_describe(t).await {
        Err(e) => report.note("sink.describe", Err(e)),
        Ok(None) => eprintln!("callback conformance: the sink protocol is not hosted"),
        Ok(Some(d)) => {
            report.note("sink.describe", Ok(()));
            sink_cases(t, &d, &mut report).await;
        }
    }
    report
}

/// The describe answer, or `None` when the endpoint hosts no source.
async fn context_describe(t: &Target) -> Result<Option<rc::DescribeResponse>, String> {
    let body = rc::DescribeRequest {
        schema: rc::DESCRIBE.into(),
        versions: vec![1],
    }
    .record_canonical()
    .map_err(err)?;
    let first = t
        .post(CONTEXT_DESCRIBE_PATH, body.clone(), CONTROL_BYTES)
        .await?;
    if first.status == 404 {
        return Ok(None);
    }
    ensure!(first.status == 200, "describe answered {}", first.status);
    let d = rc::DescribeResponse::parse(&first.body).map_err(|e| format!("describe: {e}"))?;
    ensure!(d.version == 1, "describe chose version {}", d.version);
    // Describe happens once per client; its digest must be stable.
    let again = t.post(CONTEXT_DESCRIBE_PATH, body, CONTROL_BYTES).await?;
    ensure!(
        again.body == first.body,
        "a second describe answered other bytes"
    );
    Ok((!d.sources.is_empty()).then_some(d))
}

struct Fetcher<'a> {
    t: &'a Target,
    source: &'a HostedSource,
    digest: ContentDigest,
}

impl Fetcher<'_> {
    fn body(&self, attempt: &str, trace: &str, budget_ms: u64, version: u32) -> Vec<u8> {
        self.body_for(attempt, trace, budget_ms, version, &self.all_fields())
    }

    fn body_for(
        &self,
        attempt: &str,
        trace: &str,
        budget_ms: u64,
        version: u32,
        fields: &[String],
    ) -> Vec<u8> {
        rc::FetchRequest {
            schema: rc::FETCH.into(),
            version,
            source_id: self.source.source_id.clone(),
            describe_digest: self.digest.clone(),
            attempt_id: attempt.into(),
            budget_ms,
            trace_id: trace.into(),
            traceparent: None,
            principal: principal(),
            fields: fields.to_vec(),
        }
        .record_canonical()
        .expect("fetch request")
    }

    fn all_fields(&self) -> Vec<String> {
        self.source.fields.iter().map(|f| f.name.clone()).collect()
    }

    fn attempt(&self, case: &str) -> String {
        format!("{}-{case}/context/1", self.t.tag)
    }

    /// The answer's bytes and result, read within the declared response
    /// bound: an answer above it fails the case.
    async fn fetch(&self, body: Vec<u8>) -> Result<(Vec<u8>, FetchResult), String> {
        let max = self.source.terms.max_response_bytes.min(usize::MAX as u64) as usize;
        let r = self.t.post(CONTEXT_FETCH_PATH, body, max).await?;
        ensure!(r.status == 200, "fetch answered {}", r.status);
        let answer = rc::FetchResponse::parse(&r.body).map_err(|e| format!("fetch: {e}"))?;
        Ok((r.body, answer.result))
    }
}

fn failure_code(result: &FetchResult) -> Option<(RemoteCode, &str)> {
    match result {
        FetchResult::Failure { code, detail } => Some((code.0, detail.as_str())),
        _ => None,
    }
}

async fn context_cases(t: &Target, d: &rc::DescribeResponse, report: &mut Report) {
    let f = Fetcher {
        t,
        source: &d.sources[0],
        digest: d.record_digest().expect("describe digest"),
    };
    report.note("context.versions", context_versions(&f).await);
    report.note("context.bounds", context_bounds(&f).await);
    report.note(
        "context.describe_mismatch",
        context_describe_mismatch(&f).await,
    );
    let window = context_window(&f).await;
    let attempt = window.as_ref().ok().cloned();
    report.note("context.idempotency", window.map(|_| ()));
    report.note(
        "context.cancel",
        context_cancel(&f, attempt.as_deref()).await,
    );
    report.note("context.invalidations", context_invalidations(&f).await);
    if let Some(attempt) = attempt {
        report.note(
            "context.idempotency_expiry",
            context_expiry(&f, &attempt).await,
        );
    }
}

/// Unknown and unsupported versions: describe offering none the endpoint
/// supports is refused, a fetch naming one is a capability failure, and an
/// unknown document version is a malformed request.
async fn context_versions(f: &Fetcher<'_>) -> Outcome {
    let body = rc::DescribeRequest {
        schema: rc::DESCRIBE.into(),
        versions: vec![2],
    }
    .record_canonical()
    .map_err(err)?;
    let r = f.t.post(CONTEXT_DESCRIBE_PATH, body, CONTROL_BYTES).await?;
    ensure!(
        is_client_error(r.status),
        "describe offering only version 2 answered {}",
        r.status
    );
    let (_, result) = f
        .fetch(f.body(&f.attempt("version"), &f.t.tag, 2_000, 2))
        .await?;
    ensure!(
        failure_code(&result).map(|c| c.0) == Some(RemoteCode::Capability),
        "a fetch of version 2 answered {result:?}"
    );
    let body = String::from_utf8(f.body(&f.attempt("schema"), &f.t.tag, 2_000, 1))
        .map_err(err)?
        .replace(rc::FETCH, &rc::FETCH.replace("/1", "/2"));
    let r =
        f.t.post(CONTEXT_FETCH_PATH, body.into_bytes(), CONTROL_BYTES)
            .await?;
    ensure!(
        is_client_error(r.status),
        "an unknown fetch document version answered {}",
        r.status
    );
    Ok(())
}

/// A request above the declared request bound is refused; an answer stays
/// within the response bound and holds only the fields asked for.
async fn context_bounds(f: &Fetcher<'_>) -> Outcome {
    let bound = f.source.terms.max_request_bytes;
    ensure!(
        bound < MAX_SENT_BYTES,
        "precondition: max_request_bytes {bound} is above {MAX_SENT_BYTES}"
    );
    let mut body = f.body(&f.attempt("oversized"), &f.t.tag, 2_000, 1);
    body.resize(bound as usize + 1, b' ');
    let r = f.t.post(CONTEXT_FETCH_PATH, body, CONTROL_BYTES).await?;
    ensure!(
        is_client_error(r.status),
        "a request of {} bytes against a bound of {bound} answered {}",
        bound + 1,
        r.status
    );
    let wanted = vec![f.source.fields[0].name.clone()];
    let body = f.body_for(&f.attempt("subset"), &f.t.tag, 2_000, 1, &wanted);
    let (_, result) = f.fetch(body).await?;
    if let FetchResult::Snapshot(s) = &result {
        ensure!(
            s.entries.iter().all(|e| wanted.contains(&e.field)),
            "the answer holds a field not asked for"
        );
    }
    Ok(())
}

async fn context_describe_mismatch(f: &Fetcher<'_>) -> Outcome {
    let other = Fetcher {
        t: f.t,
        source: f.source,
        digest: ContentDigest::parse(&format!("sha256:{}", "0".repeat(64))).map_err(err)?,
    };
    let (_, result) = other
        .fetch(other.body(&f.attempt("mismatch"), &f.t.tag, 2_000, 1))
        .await?;
    ensure!(
        failure_code(&result).map(|c| c.0) == Some(RemoteCode::IdentityMismatch),
        "a fetch naming another describe digest answered {result:?}"
    );
    Ok(())
}

/// Repeat and conflict inside the window; returns the attempt id used.
async fn context_window(f: &Fetcher<'_>) -> Result<String, String> {
    let attempt = f.attempt("window");
    let before = f.t.driver.as_ref().map(|d| d.dispatched());
    let (first, _) = f.fetch(f.body(&attempt, &f.t.tag, 2_000, 1)).await?;
    // Only the budget differs: a legitimate repeat with less time left.
    let (repeat, _) = f.fetch(f.body(&attempt, &f.t.tag, 1_500, 1)).await?;
    ensure!(
        repeat == first,
        "a repeat inside the window answered other bytes"
    );
    if let (Some(before), Some(d)) = (before, &f.t.driver) {
        ensure!(d.dispatched() == before + 1, "a repeat ran the fetch again");
    }
    let other = format!("{}-other", f.t.tag);
    let (_, result) = f.fetch(f.body(&attempt, &other, 2_000, 1)).await?;
    ensure!(
        matches!(failure_code(&result), Some((RemoteCode::MalformedRequest, d)) if d.contains("attempt_id_conflict")),
        "the same attempt id with other content answered {result:?}"
    );
    Ok(attempt)
}

/// After the window, the attempt id is free again.
async fn context_expiry(f: &Fetcher<'_>, attempt: &str) -> Outcome {
    let window = f.source.terms.idempotency_window_ms;
    ensure!(
        window <= MAX_WAIT_MS,
        "precondition: idempotency_window_ms {window} is above {MAX_WAIT_MS}"
    );
    tokio::time::sleep(Duration::from_millis(window + 250)).await;
    let other = format!("{}-expired", f.t.tag);
    let (_, result) = f.fetch(f.body(attempt, &other, 2_000, 1)).await?;
    ensure!(
        !matches!(failure_code(&result), Some((_, d)) if d.contains("attempt_id_conflict")),
        "the attempt id still conflicts after its window"
    );
    Ok(())
}

/// An unknown attempt is `unknown_attempt`; a finished one is never
/// `stopped`.
async fn context_cancel(f: &Fetcher<'_>, finished: Option<&str>) -> Outcome {
    let unknown = f.attempt("never-sent");
    let mut ids = vec![unknown.clone()];
    ids.extend(finished.map(str::to_string));
    let body = rc::CancelRequest {
        schema: rc::CANCEL.into(),
        attempt_ids: ids.clone(),
    }
    .record_canonical()
    .map_err(err)?;
    let r = f.t.post(CONTEXT_CANCEL_PATH, body, CONTROL_BYTES).await?;
    ensure!(r.status == 200, "cancel answered {}", r.status);
    let answers = rc::CancelResponse::parse(&r.body).map_err(err)?.answers;
    ensure!(
        answers.iter().map(|a| &a.attempt_id).eq(ids.iter()),
        "cancel answered other attempt ids"
    );
    ensure!(
        answers[0].answer == CancelAnswer::UnknownAttempt,
        "an unknown attempt answered {:?}",
        answers[0].answer
    );
    if let Some(a) = answers.get(1) {
        ensure!(
            a.answer != CancelAnswer::Stopped,
            "a finished attempt answered stopped"
        );
    }
    Ok(())
}

async fn poll(f: &Fetcher<'_>, sequence: Option<u64>) -> Result<rc::InvalidationsResponse, String> {
    let body = rc::InvalidationsRequest {
        schema: rc::INVALIDATIONS.into(),
        sequence,
    }
    .record_canonical()
    .map_err(err)?;
    let r =
        f.t.post(CONTEXT_INVALIDATIONS_PATH, body, CONTROL_BYTES)
            .await?;
    ensure!(r.status == 200, "a poll answered {}", r.status);
    let answer = rc::InvalidationsResponse::parse(&r.body).map_err(err)?;
    // Sequence numbers strictly increase after the one asked from, and the
    // answer's number is the last item's: a dropped item shows as a jump.
    let mut last = sequence.unwrap_or(0);
    for item in &answer.items {
        ensure!(
            item.sequence > last,
            "item {} after {last} is not increasing",
            item.sequence
        );
        last = item.sequence;
    }
    ensure!(
        answer.items.is_empty() || answer.sequence == last,
        "the answer's sequence {} is not its last item's {last}",
        answer.sequence
    );
    Ok(answer)
}

/// Under `poll`: the baseline poll carries no items; published items follow
/// it contiguously; items dropped from the endpoint's log show as a gap.
async fn context_invalidations(f: &Fetcher<'_>) -> Outcome {
    if !matches!(
        f.source.terms.invalidation,
        InvalidationSupport::Poll { .. }
    ) {
        eprintln!("callback conformance: the source does not offer polling");
        return Ok(());
    }
    let baseline = poll(f, None).await?;
    ensure!(baseline.items.is_empty(), "the baseline poll carried items");
    let b = baseline.sequence;
    let Some(driver) = &f.t.driver else {
        let next = poll(f, Some(b)).await?;
        ensure!(next.sequence >= b, "the sequence went back");
        return Ok(());
    };
    let id = &f.source.source_id;
    let name = &f.source.fields[0].name;
    let s = driver.publish(id, name);
    let next = poll(f, Some(b)).await?;
    ensure!(
        next.items.iter().map(|i| i.sequence).eq([s]) && s == b + 1,
        "a published item is not the next after the baseline"
    );
    let published: Vec<u64> = (0..=driver.retained())
        .map(|_| driver.publish(id, name))
        .collect();
    let first = published[0];
    let after = poll(f, Some(s)).await?;
    ensure!(
        after.items.first().is_some_and(|i| i.sequence > first),
        "items dropped from the log do not show as a gap"
    );
    Ok(())
}

/// The sink's describe answer, or `None` when no sink is hosted.
async fn sink_describe(t: &Target) -> Result<Option<rs::DescribeResponse>, String> {
    let body = |versions| {
        rs::DescribeRequest {
            schema: rs::DESCRIBE.into(),
            versions,
        }
        .record_canonical()
        .expect("describe request")
    };
    let r = t
        .post(SINK_DESCRIBE_PATH, body(vec![1]), CONTROL_BYTES)
        .await?;
    if r.status == 404 {
        return Ok(None);
    }
    ensure!(r.status == 200, "sink describe answered {}", r.status);
    let d = rs::DescribeResponse::parse(&r.body).map_err(|e| format!("sink describe: {e}"))?;
    ensure!(d.version == 1, "sink describe chose version {}", d.version);
    ensure!(
        d.record_schemas.contains(&RecordSchema::V1),
        "precondition: the sink does not accept rustev.run/1"
    );
    let r = t
        .post(SINK_DESCRIBE_PATH, body(vec![2]), CONTROL_BYTES)
        .await?;
    ensure!(
        is_client_error(r.status),
        "sink describe offering only version 2 answered {}",
        r.status
    );
    Ok(Some(d))
}

async fn deliver(t: &Target, body: Vec<u8>) -> Result<Result<DeliveryResult, u16>, String> {
    let r = t.post(SINK_DELIVER_PATH, body, CONTROL_BYTES).await?;
    if r.status != 200 {
        return Ok(Err(r.status));
    }
    Ok(Ok(rs::DeliverResponse::parse(&r.body).map_err(err)?.result))
}

async fn sink_cases(t: &Target, d: &rs::DescribeResponse, report: &mut Report) {
    let id = format!("{}-delivery", t.tag);
    report.note("sink.deliveries", sink_deliveries(t, d, &id).await);
    report.note("sink.versions", sink_versions(t).await);
    report.note("sink.bounds", sink_bounds(t, d).await);
    report.note("sink.deduplication_expiry", sink_expiry(t, d, &id).await);
}

/// Accepted once; the same digest again is `duplicate`, another digest
/// under the same decision id `conflict`.
async fn sink_deliveries(t: &Target, d: &rs::DescribeResponse, id: &str) -> Outcome {
    let (body, digest) = deliver_body(RECORD_A, id, 1);
    match deliver(t, body.clone()).await? {
        Ok(DeliveryResult::Accepted {
            durability,
            accepted_digest,
            ..
        }) => {
            ensure!(accepted_digest == digest, "accepted another digest");
            ensure!(
                d.durability_states.contains(&durability),
                "accepted with an undescribed durability {durability}"
            );
        }
        other => return Err(format!("a first delivery answered {other:?}")),
    }
    match deliver(t, body).await? {
        Ok(DeliveryResult::Duplicate { accepted_digest }) if accepted_digest == digest => {}
        other => return Err(format!("a repeated delivery answered {other:?}")),
    }
    let (other, _) = deliver_body(RECORD_B, id, 1);
    match deliver(t, other).await? {
        Ok(DeliveryResult::Conflict) => Ok(()),
        other => Err(format!(
            "another digest under the same id answered {other:?}"
        )),
    }
}

async fn sink_versions(t: &Target) -> Outcome {
    let (body, _) = deliver_body(RECORD_A, &format!("{}-version", t.tag), 2);
    match deliver(t, body).await? {
        Ok(DeliveryResult::Rejected {
            code: RemoteCode::Capability,
        }) => Ok(()),
        other => Err(format!("a delivery of version 2 answered {other:?}")),
    }
}

/// A record above the declared bound is refused, never accepted.
async fn sink_bounds(t: &Target, d: &rs::DescribeResponse) -> Outcome {
    let id = format!("{}-oversized", t.tag);
    let (body, _) = deliver_body(RECORD_LARGE, &id, 1);
    let (_, record, _) = record(RECORD_LARGE, &id);
    ensure!(
        record.len() as u64 > d.max_record_bytes,
        "precondition: max_record_bytes {} admits the {}-byte fixture",
        d.max_record_bytes,
        record.len()
    );
    match deliver(t, body).await? {
        Ok(DeliveryResult::Rejected { .. }) | Err(400..=499) => Ok(()),
        other => Err(format!("an oversized record answered {other:?}")),
    }
}

/// After the deduplication window, the decision id takes another digest.
async fn sink_expiry(t: &Target, d: &rs::DescribeResponse, id: &str) -> Outcome {
    let window = d.deduplication_window_ms;
    ensure!(
        window <= MAX_WAIT_MS,
        "precondition: deduplication_window_ms {window} is above {MAX_WAIT_MS}"
    );
    tokio::time::sleep(Duration::from_millis(window + 250)).await;
    let (body, _) = deliver_body(RECORD_B, id, 1);
    match deliver(t, body).await? {
        Ok(DeliveryResult::Accepted { .. }) => Ok(()),
        other => Err(format!("a delivery after the window answered {other:?}")),
    }
}

struct HostDriver {
    server: CallbackServer,
    retained: usize,
}

impl Driver for HostDriver {
    fn publish(&self, source_id: &str, field: &str) -> u64 {
        self.server
            .publish_invalidation(
                source_id,
                InvalidationKind::Correction,
                vec![field.into()],
                InvalidationScope::AllScopes,
            )
            .expect("published")
    }
    fn retained(&self) -> usize {
        self.retained
    }
    fn dispatched(&self) -> u64 {
        self.server.dispatched()
    }
}

fn host(config: CallbackConfig) -> CallbackServer {
    let server = CallbackServer::new(config);
    server
        .host_context(
            "support",
            HostedContext {
                source: Arc::new(CaseSource(Snapshot::parse(SNAPSHOT).expect("fixture"))),
                fields: vec![field("ticket.message"), field("account.tier")],
            },
        )
        .expect("hosted source");
    server
        .host_sink(HostedSink {
            sink: Arc::new(Keep::default()),
            record_schemas: vec![RecordSchema::V1],
            durability: "committed".into(),
            deduplication_window_ms: 300,
            max_record_bytes: 3_000,
        })
        .expect("hosted sink");
    server
}

fn transport(url: &str, bearer: Option<Secret>) -> HttpTransport {
    HttpTransport::new(Endpoint::parse(url).expect("endpoint"), None, bearer, None)
        .expect("transport")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn endpoint_cases_against_the_rust_host_server() {
    let retained = 4;
    let server = host(CallbackConfig {
        idempotency_window_ms: 300,
        max_request_bytes: 8_192,
        invalidation: InvalidationSupport::Poll {
            interval_ms: 1_000,
            max_lag_ms: 5_000,
        },
        max_invalidations: retained,
        ..CallbackConfig::default()
    });
    let handle = server.serve_loopback().await.expect("loopback");
    let every = [
        "context.describe",
        "context.versions",
        "context.bounds",
        "context.describe_mismatch",
        "context.idempotency",
        "context.idempotency_expiry",
        "context.cancel",
        "context.invalidations",
        "sink.describe",
        "sink.deliveries",
        "sink.versions",
        "sink.bounds",
        "sink.deduplication_expiry",
    ];
    // With the driver, then without it, as a host endpoint reached by URL.
    for driven in [true, false] {
        let driver: Option<Box<dyn Driver>> = driven.then(|| {
            Box::new(HostDriver {
                server: server.clone(),
                retained,
            }) as Box<dyn Driver>
        });
        let target = Target {
            transport: transport(&handle.url(), None),
            driver,
            tag: tag(),
        };
        let report = endpoint_suite(&target).await;
        assert!(
            report.failures.is_empty(),
            "driven={driven}: {:#?}",
            report.failures
        );
        assert_eq!(report.ran, every.iter().map(|s| s.to_string()).collect());
    }
}

/// The Rust host never sends an answer above its declared response bound: it
/// answers a failure instead.
#[tokio::test]
async fn the_rust_host_answers_a_failure_rather_than_a_body_above_its_bound() {
    let server = host(CallbackConfig {
        max_response_bytes: 200,
        ..CallbackConfig::default()
    });
    let handle = server.serve_loopback().await.expect("loopback");
    let target = Target {
        transport: transport(&handle.url(), None),
        driver: None,
        tag: tag(),
    };
    let d = context_describe(&target).await.unwrap().expect("described");
    let f = Fetcher {
        t: &target,
        source: &d.sources[0],
        digest: d.record_digest().unwrap(),
    };
    let (_, result) = f
        .fetch(f.body(&f.attempt("large"), &target.tag, 2_000, 1))
        .await
        .unwrap();
    assert_eq!(
        failure_code(&result).map(|c| c.0),
        Some(RemoteCode::RemoteError),
        "{result:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "runs against the endpoint named by RUSTEV_CALLBACK_ENDPOINT"]
async fn endpoint_cases_against_a_host_endpoint_by_url() {
    let url = std::env::var("RUSTEV_CALLBACK_ENDPOINT")
        .expect("RUSTEV_CALLBACK_ENDPOINT names the endpoint's base URL");
    let bearer = std::env::var("RUSTEV_CALLBACK_BEARER_FILE").ok().map(|p| {
        let token = std::fs::read_to_string(p).expect("bearer file");
        Secret::new(token.trim())
    });
    let target = Target {
        transport: transport(&url, bearer),
        driver: None,
        tag: tag(),
    };
    let report = endpoint_suite(&target).await;
    eprintln!("callback conformance ran: {:?}", report.ran);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert!(
        report
            .ran
            .iter()
            .any(|c| c != "context.describe" && c != "sink.describe"),
        "the endpoint hosts neither protocol"
    );
}

// The client suite: the clients against a scripted endpoint.

#[derive(Debug, Clone, Copy, PartialEq)]
enum Answer {
    Now,
    /// Read the request, never send a response byte.
    StallHeaders,
    /// Send the headers and half the body, then stop.
    StallBody,
    /// A body one byte above the client's bound.
    Oversized,
    /// Answer after this many milliseconds.
    After(u64),
}

#[derive(Debug, Clone)]
struct Script {
    context_version: u32,
    third_party: bool,
    cancellation: CancelSupport,
    max_request_bytes: u64,
    max_response_bytes: u64,
    fetch: Answer,
    cancel: CancelAnswer,
    sink_version: u32,
    deliver: Answer,
}

impl Default for Script {
    fn default() -> Self {
        Script {
            context_version: 1,
            third_party: false,
            cancellation: CancelSupport::Confirmed,
            max_request_bytes: 65_536,
            max_response_bytes: 65_536,
            fetch: Answer::Now,
            cancel: CancelAnswer::Unconfirmed,
            sink_version: 1,
            deliver: Answer::Now,
        }
    }
}

/// RemoteSink reads a delivery answer up to this many bytes.
const SINK_ANSWER_BYTES: usize = 65_536;

#[derive(Default)]
struct Scripted {
    script: Mutex<Script>,
    seen: Mutex<Vec<(String, Vec<u8>)>>,
}

impl Scripted {
    fn set(&self, f: impl FnOnce(&mut Script)) {
        f(&mut self.script.lock().unwrap());
    }

    fn requests(&self, path: &str) -> Vec<Vec<u8>> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .filter(|(p, _)| p == path)
            .map(|(_, b)| b.clone())
            .collect()
    }

    fn respond(&self, path: &str, body: &[u8]) -> (Answer, Vec<u8>) {
        let s = self.script.lock().unwrap().clone();
        let with_version = |bytes: Vec<u8>, version: u32| {
            let text = String::from_utf8(bytes).expect("utf-8");
            text.replacen("\"version\":1", &format!("\"version\":{version}"), 1)
        };
        let (answer, bytes, bound) = match path {
            CONTEXT_DESCRIBE_PATH => {
                let d = rc::DescribeResponse {
                    schema: rc::DESCRIBE.into(),
                    version: 1,
                    sources: vec![HostedSource {
                        source_id: "scripted".into(),
                        fields: vec![field("ticket.message"), field("account.tier")],
                        terms: ContextTerms {
                            cancellation: s.cancellation,
                            idempotency_window_ms: 60_000,
                            max_request_bytes: s.max_request_bytes,
                            max_response_bytes: s.max_response_bytes,
                            invalidation: InvalidationSupport::None,
                            endpoint_class: EndpointClass::HostOperated,
                        },
                    }],
                };
                let mut text = with_version(d.record_canonical().unwrap(), s.context_version);
                if s.third_party {
                    text = text.replace("\"host_operated\"", "\"third_party\"");
                }
                (Answer::Now, text.into_bytes(), 0)
            }
            CONTEXT_FETCH_PATH => {
                let req = rc::FetchRequest::parse(body).expect("fetch request");
                let mut snapshot = Snapshot::parse(SNAPSHOT).unwrap();
                snapshot.entries.retain(|e| req.fields.contains(&e.field));
                let bytes = rc::FetchResponse {
                    schema: rc::FETCH.into(),
                    result: FetchResult::Snapshot(snapshot),
                }
                .record_canonical()
                .unwrap();
                (s.fetch, bytes, s.max_response_bytes as usize)
            }
            CONTEXT_CANCEL_PATH => {
                let req = rc::CancelRequest::parse(body).expect("cancel request");
                let bytes = rc::CancelResponse {
                    schema: rc::CANCEL.into(),
                    answers: req
                        .attempt_ids
                        .into_iter()
                        .map(|attempt_id| rc::CancelItem {
                            attempt_id,
                            answer: s.cancel.clone(),
                        })
                        .collect(),
                }
                .record_canonical()
                .unwrap();
                (Answer::Now, bytes, 0)
            }
            SINK_DESCRIBE_PATH => {
                let d = rs::DescribeResponse {
                    schema: rs::DESCRIBE.into(),
                    version: 1,
                    record_schemas: vec![RecordSchema::V1],
                    durability_states: vec!["stored".into()],
                    deduplication_window_ms: 60_000,
                    max_record_bytes: 1 << 20,
                };
                let text = with_version(d.record_canonical().unwrap(), s.sink_version);
                (Answer::Now, text.into_bytes(), 0)
            }
            SINK_DELIVER_PATH => {
                let req = rs::DeliverRequest::parse(body).expect("deliver request");
                let bytes = rs::DeliverResponse {
                    schema: rs::DELIVER.into(),
                    result: DeliveryResult::Accepted {
                        durability: "stored".into(),
                        external_id: "scripted".into(),
                        accepted_digest: req.record_digest,
                    },
                }
                .record_canonical()
                .unwrap();
                (s.deliver, bytes, SINK_ANSWER_BYTES)
            }
            other => panic!("unexpected path {other}"),
        };
        match answer {
            Answer::Oversized => {
                let mut bytes = bytes;
                bytes.resize(bound + 1, b' ');
                (answer, bytes)
            }
            _ => (answer, bytes),
        }
    }
}

/// A raw HTTP/1.1 endpoint that answers as its script says.
struct ScriptedEndpoint {
    script: Arc<Scripted>,
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for ScriptedEndpoint {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn scripted() -> ScriptedEndpoint {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let script = Arc::new(Scripted::default());
    let state = script.clone();
    let task = tokio::spawn(async move {
        // Owned here, so aborting the accept loop closes every connection.
        let mut connections = tokio::task::JoinSet::new();
        while let Ok((stream, _)) = listener.accept().await {
            connections.spawn(exchange(state.clone(), stream));
        }
    });
    ScriptedEndpoint { script, url, task }
}

async fn exchange(s: Arc<Scripted>, mut stream: TcpStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let path = head.split(' ').nth(1).unwrap_or_default().to_string();
    let length = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while buf.len() < head_end + length {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = buf[head_end..head_end + length].to_vec();
    s.seen.lock().unwrap().push((path.clone(), body.clone()));
    let (answer, bytes) = s.respond(&path, &body);
    match answer {
        Answer::StallHeaders => std::future::pending::<()>().await,
        Answer::After(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
        _ => {}
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        bytes.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    if answer == Answer::StallBody {
        let _ = stream.write_all(&bytes[..bytes.len() / 2]).await;
        let _ = stream.flush().await;
        std::future::pending::<()>().await;
    }
    let _ = stream.write_all(&bytes).await;
    let _ = stream.shutdown().await;
}

type Exchanges = Arc<Mutex<Vec<ExchangeRecord>>>;

async fn context_client(
    url: &str,
    in_flight: u32,
    phases: TransportPhases,
    exchanges: Option<Exchanges>,
) -> Result<RemoteContextClient, String> {
    let config = ContextClientConfig {
        source_id: "scripted".into(),
        in_flight,
        queue_capacity: 1,
        transit_margin_ms: 25,
        describe_budget_ms: 2_000,
        mapping_version: "m1".into(),
        principal_scope: "tenant".into(),
    };
    let sink = exchanges.map(|kept| -> crate::context_client::ExchangeSink {
        Arc::new(move |r: &ExchangeRecord| {
            kept.lock().unwrap().push(r.clone());
            Ok(())
        })
    });
    RemoteContextClient::connect(transport(url, None).with_phases(phases), config, sink).await
}

fn cx(decision: &str, ms: u64) -> CallContext {
    CallContext {
        remaining_ms: DurationMs(ms),
        trace_id: decision.into(),
        principal_handle: b"user-7".to_vec(),
    }
}

fn fields() -> Vec<String> {
    vec!["ticket.message".into(), "account.tier".into()]
}

fn code(result: &Result<Snapshot, String>) -> Option<RemoteCode> {
    result.as_ref().err().and_then(|e| code_of_detail(e))
}

async fn until(what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "waited for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Timeouts at each phase: waiting for an in-flight slot (nothing sent),
/// the first response byte under a phase ceiling, the response body under
/// the budget, and a budget spent by the transit margin before sending.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_client_times_out_at_each_phase() {
    let endpoint = scripted().await;
    endpoint.script.set(|s| s.fetch = Answer::StallHeaders);
    let ceiling = PhasePolicy {
        resolution_ms: 10_000,
        connect_ms: 10_000,
        secure_handshake_ms: 10_000,
        request_write_ms: 10_000,
        first_response_byte_ms: 150,
        response_body_ms: 10_000,
        cancellation_ack_ms: 10_000,
    };
    let phased = context_client(&endpoint.url, 1, TransportPhases::Enabled(ceiling), None)
        .await
        .unwrap();
    let first = {
        let c = phased.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            (
                c.fetch(&fields(), &cx("d-first", 3_000)).await,
                started.elapsed(),
            )
        })
    };
    until("the first fetch", || {
        endpoint.script.requests(CONTEXT_FETCH_PATH).len() == 1
    })
    .await;
    let queued = phased.fetch(&fields(), &cx("d-queued", 60)).await;
    assert_eq!(
        code(&queued),
        Some(RemoteCode::TransportUnsent),
        "{queued:?}"
    );
    assert!(queued.unwrap_err().contains("queue_deadline"));
    let (result, elapsed) = first.await.unwrap();
    assert_eq!(
        code(&result),
        Some(RemoteCode::RemoteDeadline),
        "{result:?}"
    );
    assert!(
        elapsed < Duration::from_millis(2_000),
        "the first-byte ceiling, not the budget, ended it: {elapsed:?}"
    );
    assert_eq!(endpoint.script.requests(CONTEXT_FETCH_PATH).len(), 1);

    endpoint.script.set(|s| s.fetch = Answer::StallBody);
    let plain = context_client(&endpoint.url, 1, TransportPhases::Disabled, None)
        .await
        .unwrap();
    let body = plain.fetch(&fields(), &cx("d-body", 300)).await;
    assert_eq!(code(&body), Some(RemoteCode::RemoteDeadline), "{body:?}");
    assert_eq!(endpoint.script.requests(CONTEXT_FETCH_PATH).len(), 2);

    let spent = plain.fetch(&fields(), &cx("d-spent", 20)).await;
    assert_eq!(code(&spent), Some(RemoteCode::RemoteDeadline), "{spent:?}");
    assert_eq!(endpoint.script.requests(CONTEXT_FETCH_PATH).len(), 2);
}

/// A caller that stops waiting cancels the attempt at the endpoint; the
/// answer that still arrives is discarded, counted and recorded as late.
/// Under `none` no cancel is sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_client_discards_a_late_answer_and_cancels_the_attempt() {
    for cancellation in [CancelSupport::Confirmed, CancelSupport::None] {
        let endpoint = scripted().await;
        endpoint.script.set(|s| {
            s.fetch = Answer::After(400);
            s.cancellation = cancellation;
        });
        let kept = Exchanges::default();
        let client = context_client(
            &endpoint.url,
            1,
            TransportPhases::Disabled,
            Some(kept.clone()),
        )
        .await
        .unwrap();
        let gave_up = tokio::time::timeout(
            Duration::from_millis(50),
            client.fetch(&fields(), &cx("d-late", 2_000)),
        )
        .await;
        assert!(gave_up.is_err(), "the caller stopped waiting");
        until("the late answer", || {
            client.counters().late_responses_discarded == 1
        })
        .await;
        let records = kept.lock().unwrap().clone();
        assert!(
            records.iter().any(|r| r.attempt_id == "d-late/context/1"
                && r.late
                && r.response_digest.is_some()),
            "{records:?}"
        );
        let cancels = endpoint.script.requests(CONTEXT_CANCEL_PATH);
        if cancellation == CancelSupport::None {
            assert!(cancels.is_empty(), "no cancel under none");
        } else {
            let ids: Vec<String> = cancels
                .iter()
                .flat_map(|b| rc::CancelRequest::parse(b).unwrap().attempt_ids)
                .collect();
            assert_eq!(ids, ["d-late/context/1"]);
        }
    }
}

/// `stopped` is accepted only under confirmed cancellation; `unconfirmed`
/// and `unknown_attempt` under any.
#[tokio::test]
async fn context_client_checks_cancellation_outcomes() {
    let endpoint = scripted().await;
    for (support, answer, ok) in [
        (CancelSupport::Confirmed, CancelAnswer::Stopped, true),
        (CancelSupport::Confirmed, CancelAnswer::Unconfirmed, true),
        (CancelSupport::Confirmed, CancelAnswer::UnknownAttempt, true),
        (CancelSupport::BestEffort, CancelAnswer::Stopped, false),
        (CancelSupport::BestEffort, CancelAnswer::Unconfirmed, true),
        (CancelSupport::None, CancelAnswer::Stopped, false),
    ] {
        endpoint.script.set(|s| {
            s.cancellation = support;
            s.cancel = answer.clone();
        });
        let client = context_client(&endpoint.url, 1, TransportPhases::Disabled, None)
            .await
            .unwrap();
        let result = client.cancel(vec!["d1/context/1".into()], 1_000).await;
        match result {
            Ok(items) if ok => assert_eq!(items[0].answer, answer),
            Err(e) if !ok => assert_eq!(
                code_of_detail(&e),
                Some(RemoteCode::MalformedResponse),
                "{e}"
            ),
            other => panic!("{support:?} {answer:?}: {other:?}"),
        }
    }
}

/// Oversized bodies both ways, and unknown versions and classes at
/// describe.
#[tokio::test]
async fn context_client_refuses_oversized_bodies_and_unknown_versions() {
    let endpoint = scripted().await;
    endpoint.script.set(|s| {
        s.fetch = Answer::Oversized;
        s.max_response_bytes = 4_096;
    });
    let client = context_client(&endpoint.url, 1, TransportPhases::Disabled, None)
        .await
        .unwrap();
    let large = client.fetch(&fields(), &cx("d-large", 2_000)).await;
    assert_eq!(
        code(&large),
        Some(RemoteCode::MalformedResponse),
        "{large:?}"
    );

    endpoint.script.set(|s| s.max_request_bytes = 64);
    let client = context_client(&endpoint.url, 1, TransportPhases::Disabled, None)
        .await
        .unwrap();
    let sent = endpoint.script.requests(CONTEXT_FETCH_PATH).len();
    let request = client.fetch(&fields(), &cx("d-request", 2_000)).await;
    assert_eq!(
        code(&request),
        Some(RemoteCode::MalformedRequest),
        "{request:?}"
    );
    assert_eq!(endpoint.script.requests(CONTEXT_FETCH_PATH).len(), sent);

    for set in [
        (|s: &mut Script| s.context_version = 2) as fn(&mut Script),
        |s: &mut Script| s.third_party = true,
    ] {
        let endpoint = scripted().await;
        endpoint.script.set(set);
        let refused = context_client(&endpoint.url, 1, TransportPhases::Disabled, None).await;
        let Err(e) = refused else {
            panic!("construction accepted {:?}", endpoint.script.script)
        };
        assert_eq!(code_of_detail(&e), Some(RemoteCode::Capability), "{e}");
    }
}

fn sink_endpoint(url: &str) -> rustev_contract::serve::RemoteEndpoint {
    serde_json::from_value(serde_json::json!({
        "address": url, "credentials_file": "none",
        "max_in_flight": 1, "queue_capacity": 1, "readiness_affecting": true,
    }))
    .expect("remote endpoint")
}

fn sink_request(decision_id: &str) -> rs::DeliverRequest {
    let (body, _) = deliver_body(RECORD_A, decision_id, 1);
    rs::DeliverRequest::parse(&body).unwrap()
}

/// The sink client: a timeout after the request left may have reached the
/// sink and yields no receipt, a full queue sends nothing, an oversized
/// answer is `malformed_response`, and an unknown version is refused at
/// describe.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sink_client_timeouts_bounds_and_versions() {
    let endpoint = scripted().await;
    let sink = Arc::new(
        RemoteSink::connect(&sink_endpoint(&endpoint.url), 300)
            .await
            .unwrap(),
    );
    let ok = sink.send(sink_request("s-ok")).await.unwrap();
    assert!(ok.starts_with("remote-sink:stored:sha256:"), "{ok}");

    endpoint.script.set(|s| s.deliver = Answer::StallHeaders);
    let first = {
        let sink = sink.clone();
        tokio::spawn(async move { sink.send(sink_request("s-first")).await })
    };
    until("the first delivery", || {
        endpoint.script.requests(SINK_DELIVER_PATH).len() == 2
    })
    .await;
    // Polled once, the second delivery takes the queue's one slot and waits
    // for the in-flight one.
    let queued = sink.send(sink_request("s-queued"));
    tokio::pin!(queued);
    tokio::select! {
        biased;
        r = &mut queued => panic!("the second delivery did not wait: {r:?}"),
        () = std::future::ready(()) => {}
    }
    let full = sink.send(sink_request("s-full")).await.unwrap_err();
    assert_eq!(full, "remote_seam_queue_full; may_have_received: false");
    assert_eq!(
        first.await.unwrap().unwrap_err(),
        "deadline; may_have_received: true"
    );
    let queued = queued.await.unwrap_err();
    assert!(queued.contains("may_have_received"), "{queued}");

    for (answer, expected) in [
        (Answer::StallBody, "deadline; may_have_received: true"),
        (
            Answer::Oversized,
            "malformed_response; may_have_received: true",
        ),
    ] {
        endpoint.script.set(|s| s.deliver = answer);
        let e = sink.send(sink_request("s-answer")).await.unwrap_err();
        assert_eq!(e, expected, "{answer:?}");
    }

    endpoint.script.set(|s| s.sink_version = 2);
    assert!(
        RemoteSink::connect(&sink_endpoint(&endpoint.url), 300)
            .await
            .is_err()
    );
}
