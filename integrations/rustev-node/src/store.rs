//! Spec 063 3.6 and 3.7: the store seam and invalidation. A host
//! `RuntimeStore` is installed as the runtime's `RuntimeStore`; each method
//! is an asynchronous host callback under 3.4 (`crate::callback`). Every
//! answer is bounded here: generations, counts and budgets are safe
//! integers, `get` bytes are at most 4 MiB (`RECORD_V1`), and a `put`
//! answer is one of four words. Anything else, a throw, a full queue, a
//! deadline, a cancellation or a late settlement is a `StoreFailure`, which
//! the runtime treats under the plan's `on_store_failure`, never a hit.
//! Rustev serves an entry only when its digest, key material and both
//! generations check (spec 044 3.4.3); the atomicity the store claims is
//! the host's (3.7), recorded and never verified.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use rustev_contract::cache_entry::EntryGenerations;
use rustev_contract::limits::RECORD_V1;
use rustev_contract::optimization::StoreContract;
use rustev_core::seams::{BoxFuture, CancelSignal};
use rustev_runtime::{
    Clock, Invalidated, PutAnswer, PutTerms, RuntimeStore, StoreCall, StoreFailure, StoreSelector,
    Stored,
};

use crate::callback::{self, HostFn};
use crate::errors::fail;
use crate::seams::{SeamStats, capacity};

/// The largest safe integer (2^53 - 1): every generation, count and budget
/// crosses as a JavaScript number no larger (3.6.3).
pub const MAX_SAFE: u64 = 9_007_199_254_740_991;

/// One store call as the JavaScript layer receives it; which fields are set
/// depends on the method. Numbers are safe integers.
#[napi(object, object_from_js = false)]
pub struct StoreCallJs {
    /// Names the call's `AbortSignal`, as an abort names it.
    pub call_id: String,
    pub namespace: String,
    pub scope: Option<String>,
    pub key_id: Option<String>,
    pub bytes: Option<Uint8Array>,
    pub namespace_generation: Option<f64>,
    pub scope_generation: Option<f64>,
    pub max_scope_bytes: Option<f64>,
    pub max_namespace_bytes: Option<f64>,
    pub expires_ms: Option<f64>,
    /// For `invalidate`: absent selects the namespace.
    pub selector_scope: Option<String>,
    pub remaining_ms: Option<f64>,
}

/// A store method's settlement as the JavaScript layer adapts it.
#[napi(object, object_to_js = false)]
pub struct StoreAnswerJs {
    pub bytes: Option<Uint8Array>,
    pub namespace_generation: Option<f64>,
    pub scope_generation: Option<f64>,
    pub put: Option<String>,
    pub generation: Option<f64>,
    pub removed: Option<f64>,
}

type MethodFn = HostFn<StoreCallJs, StoreAnswerJs>;
type AbortFn = ThreadsafeFunction<String, (), String, napi::Status, false, true>;

/// The host store as the JavaScript layer hands it over: its contract as
/// JSON, its queue and the four adapted methods.
#[napi(object, object_to_js = false)]
pub struct StoreSeam {
    pub contract: String,
    pub queue_capacity: u32,
    pub get: MethodFn,
    pub put: MethodFn,
    pub delete: MethodFn,
    pub invalidate: MethodFn,
    pub abort: AbortFn,
}

tokio::task_local! {
    /// The `timeoutMs` of the `plan.invalidate` running on this task (3.6.4):
    /// the store call it makes has no decision deadline.
    pub static INVALIDATE_TIMEOUT_MS: u64;
}

pub struct HostStore {
    contract: StoreContract,
    capacity: usize,
    get: Arc<MethodFn>,
    put: Arc<MethodFn>,
    delete: Arc<MethodFn>,
    invalidate: Arc<MethodFn>,
    abort: Arc<AbortFn>,
    clock: Arc<dyn Clock>,
    next: AtomicU64,
    pub stats: Arc<SeamStats>,
}

/// 3.6.1: the closed `StoreContract`, nine non-empty strings.
fn contract(json: &str) -> Result<StoreContract> {
    let refused = |detail: String| {
        fail(
            "ConfigRefused",
            "invalid_store_contract",
            serde_json::json!({ "detail": detail }),
        )
    };
    let c: StoreContract = serde_json::from_str(json).map_err(|e| refused(e.to_string()))?;
    if let Some((name, _)) = c.terms().iter().find(|(_, v)| v.trim().is_empty()) {
        return Err(refused(format!("the {name} term is empty")));
    }
    Ok(c)
}

impl HostStore {
    pub fn new(s: StoreSeam, clock: Arc<dyn Clock>) -> Result<HostStore> {
        Ok(HostStore {
            contract: contract(&s.contract)?,
            capacity: capacity("store.queueCapacity", s.queue_capacity)?,
            get: Arc::new(s.get),
            put: Arc::new(s.put),
            delete: Arc::new(s.delete),
            invalidate: Arc::new(s.invalidate),
            abort: Arc::new(s.abort),
            clock,
            next: AtomicU64::new(0),
            stats: Arc::new(SeamStats {
                name: "store".into(),
                callbacks: true,
                ..Default::default()
            }),
        })
    }

    fn blank(&self, namespace: &str, call: &StoreCall) -> StoreCallJs {
        StoreCallJs {
            call_id: format!("store/{}", self.next.fetch_add(1, Ordering::SeqCst)),
            namespace: namespace.into(),
            scope: None,
            key_id: None,
            bytes: None,
            namespace_generation: None,
            scope_generation: None,
            max_scope_bytes: None,
            max_namespace_bytes: None,
            expires_ms: None,
            selector_scope: None,
            remaining_ms: call.remaining_ms.map(|m| m as f64),
        }
    }

    /// One method call under 3.4. A full queue fails at once
    /// (`host_seam_queue_full`). The runtime bounds a decision's call by its
    /// deadline and cancellation and raises the call's signal, which is
    /// forwarded to the host as an abort; a call with no deadline
    /// (`plan.invalidate`) is bounded here by its `timeoutMs`, on the
    /// engine's clock. A call the runtime stops waiting on is abandoned:
    /// its slot is released and its host signal aborted.
    fn call<'a>(
        &'a self,
        method: &Arc<MethodFn>,
        js: StoreCallJs,
        call: StoreCall,
    ) -> BoxFuture<'a, std::result::Result<StoreAnswerJs, StoreFailure>> {
        let Some(slot) = self.stats.admit(self.capacity) else {
            return Box::pin(async { Err(failure("host_seam_queue_full")) });
        };
        let timeout = match call.remaining_ms {
            Some(_) => None,
            None => INVALIDATE_TIMEOUT_MS.try_with(|t| *t).ok(),
        };
        let deadline = timeout.map(|t| self.clock.now().saturating_add(t));
        let id = js.call_id.clone();
        let mut pending = callback::call(method, js, slot, &self.stats);
        let mut timer = deadline.map(|d| self.clock.sleep_until(d));
        let signal: CancelSignal = call.cancel;
        let abort = AbortOnDrop {
            abort: self.abort.clone(),
            id,
            armed: true,
        };
        Box::pin(async move {
            let mut abort = abort;
            let r = std::future::poll_fn(|cx| {
                if let std::task::Poll::Ready(r) = std::pin::Pin::new(&mut pending).poll(cx) {
                    // Settled, resolved or rejected: nothing left to abort.
                    abort.armed = false;
                    return std::task::Poll::Ready(r.map_err(|e| failure(&e)));
                }
                if let Some(t) = timer.as_mut()
                    && t.as_mut().poll(cx).is_ready()
                {
                    return std::task::Poll::Ready(Err(failure("timeout")));
                }
                if signal.poll_raised(cx).is_ready() {
                    return std::task::Poll::Ready(Err(failure("cancelled")));
                }
                std::task::Poll::Pending
            })
            .await;
            // Unsettled at a timeout or cancellation: the drop aborts it.
            r
        })
    }
}

/// Aborts the host call's signal unless it settled first (3.4.4, 3.6.4).
struct AbortOnDrop {
    abort: Arc<AbortFn>,
    id: String,
    armed: bool,
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.abort.call(
                std::mem::take(&mut self.id),
                ThreadsafeFunctionCallMode::NonBlocking,
            );
        }
    }
}

fn failure(detail: &str) -> StoreFailure {
    StoreFailure {
        detail: detail.into(),
    }
}

/// A safe integer, or the answer is out of bounds (3.6.3).
fn safe(v: Option<f64>, what: &str) -> std::result::Result<u64, StoreFailure> {
    match v {
        Some(n) if n.fract() == 0.0 && (0.0..=MAX_SAFE as f64).contains(&n) => Ok(n as u64),
        Some(n) => Err(failure(&format!(
            "the store's {what} {n} is not a safe integer"
        ))),
        None => Err(failure(&format!("the store's answer names no {what}"))),
    }
}

/// A number the runtime gives the host, saturated at the largest safe one.
fn number(v: u64) -> f64 {
    v.min(MAX_SAFE) as f64
}

impl RuntimeStore for HostStore {
    fn contract(&self) -> &StoreContract {
        &self.contract
    }

    fn get<'a>(
        &'a self,
        namespace: &'a str,
        scope: &'a str,
        key_id: &'a str,
        call: StoreCall,
    ) -> BoxFuture<'a, std::result::Result<Stored, StoreFailure>> {
        let mut js = self.blank(namespace, &call);
        js.scope = Some(scope.into());
        js.key_id = Some(key_id.into());
        let work = self.call(&self.get, js, call);
        Box::pin(async move {
            let a = work.await?;
            let generations = EntryGenerations {
                namespace: safe(a.namespace_generation, "namespace generation")?,
                scope: safe(a.scope_generation, "scope generation")?,
            };
            let bytes = match a.bytes {
                Some(b) if b.len() > RECORD_V1.max_bytes => {
                    return Err(failure("the store's entry exceeds 4 MiB"));
                }
                b => b.map(|b| b.to_vec()),
            };
            Ok(Stored { bytes, generations })
        })
    }

    fn put<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        bytes: Vec<u8>,
        terms: PutTerms,
        call: StoreCall,
    ) -> BoxFuture<'a, std::result::Result<PutAnswer, StoreFailure>> {
        let mut js = self.blank(namespace, &call);
        js.key_id = Some(key_id.into());
        js.bytes = Some(Uint8Array::new(bytes));
        js.scope = Some(terms.scope);
        js.namespace_generation = Some(number(terms.generations.namespace));
        js.scope_generation = Some(number(terms.generations.scope));
        js.max_scope_bytes = Some(number(terms.max_scope_bytes));
        js.max_namespace_bytes = Some(number(terms.max_namespace_bytes));
        js.expires_ms = Some(terms.expires.as_ms() as f64);
        let work = self.call(&self.put, js, call);
        Box::pin(async move {
            match work.await?.put.as_deref() {
                Some("stored") => Ok(PutAnswer::Stored),
                Some("generation_advanced") => Ok(PutAnswer::GenerationAdvanced),
                Some("over_budget") => Ok(PutAnswer::OverBudget),
                Some("conflict") => Ok(PutAnswer::Conflict),
                other => Err(failure(&format!(
                    "the store's put answer {other:?} is not one of stored, generation_advanced, over_budget, conflict"
                ))),
            }
        })
    }

    fn delete<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        call: StoreCall,
    ) -> BoxFuture<'a, std::result::Result<(), StoreFailure>> {
        let mut js = self.blank(namespace, &call);
        js.key_id = Some(key_id.into());
        let work = self.call(&self.delete, js, call);
        Box::pin(async move { work.await.map(|_| ()) })
    }

    fn invalidate<'a>(
        &'a self,
        namespace: &'a str,
        selector: StoreSelector,
        call: StoreCall,
    ) -> BoxFuture<'a, std::result::Result<Invalidated, StoreFailure>> {
        let mut js = self.blank(namespace, &call);
        if let StoreSelector::Scope(scope) = selector {
            js.selector_scope = Some(scope);
        }
        let work = self.call(&self.invalidate, js, call);
        Box::pin(async move {
            let a = work.await?;
            Ok(Invalidated {
                generation: safe(a.generation, "generation")?,
                removed: safe(a.removed, "removed count")?,
            })
        })
    }
}

/// `plan.invalidate`'s result (3.6): a failed or timed-out store call
/// leaves the selection pending, `complete: false`, never an exception.
pub fn invalidation_json(r: &rustev_runtime::InvalidationResult) -> serde_json::Value {
    use rustev_runtime::PersistentInvalidation as P;
    let persistent = match &r.persistent {
        None => serde_json::Value::Null,
        Some(P::Done {
            generation,
            removed,
        }) => serde_json::json!({ "generation": generation, "removed": removed }),
        Some(_) => "pending".into(),
    };
    serde_json::json!({
        "generation": r.generation,
        "removedEntries": r.removed_entries,
        "removedBytes": r.removed_bytes,
        "persistent": persistent,
        "complete": r.complete,
    })
}
