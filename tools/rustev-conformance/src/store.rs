//! A scripted store for runtime cases (spec 063, 3.13.2): an in-memory
//! store with namespace and scope generations, as
//! `crates/rustev-runtime/tests/persistent.rs`'s test double, implementing
//! G1 to G5 of 063 3.7. The script declares its contract, failures that
//! hold from a decision's start, the decisions that run on a restarted
//! runtime over the same store, and the invalidations made before a
//! decision starts. Both runners interpret it and must produce the same
//! bytes.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rustev_contract::cache_entry::EntryGenerations;
use rustev_contract::optimization::StoreContract;
use rustev_core::seams::BoxFuture;
use rustev_runtime::{
    Invalidated, InvalidationResult, PersistentInvalidation, PutAnswer, PutTerms, RuntimeStore,
    StoreCall, StoreFailure, StoreSelector, Stored,
};
use serde::Deserialize;
use serde_json::{Value as Json, json};

/// One scripted store, shared by every runtime of a case.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreScript {
    pub contract: StoreContract,
    /// From a decision's start, the store's failures (absent: none).
    #[serde(default)]
    pub failures: BTreeMap<String, Failures>,
    /// These decisions run on a new runtime over the same store: a restart.
    #[serde(default)]
    pub restart_before: Vec<String>,
    /// Before these decisions start, `Runtime::invalidate(plan, None)`;
    /// its result is the output `<decision>.invalidation.json`.
    #[serde(default)]
    pub invalidate_before: Vec<String>,
    /// The plan (an input name) a decision runs, when not the case's.
    #[serde(default)]
    pub plan_for: BTreeMap<String, String>,
    /// Whether `invalidate` removes the entries it covers (G2 makes removal
    /// optional; a kept entry fails the generation check).
    #[serde(default = "yes")]
    pub remove_on_invalidate: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failures {
    #[serde(default)]
    pub fail_get: bool,
    #[serde(default)]
    pub fail_put: bool,
    #[serde(default)]
    pub fail_invalidate: bool,
    /// Answer every `put` with this instead of storing.
    #[serde(default)]
    pub put_answer: Option<String>,
}

#[derive(Default)]
struct State {
    /// By (namespace, key id): the entry's scope and bytes.
    entries: BTreeMap<(String, String), (String, Vec<u8>)>,
    namespace_generation: BTreeMap<String, u64>,
    scope_generation: BTreeMap<(String, String), u64>,
    failures: Failures,
}

pub struct ScriptedStore {
    script: StoreScript,
    state: Mutex<State>,
}

fn failed(what: &str) -> StoreFailure {
    StoreFailure {
        detail: format!("SYNTHETIC {what} failure"),
    }
}

impl ScriptedStore {
    pub fn new(script: StoreScript) -> Arc<Self> {
        Arc::new(ScriptedStore {
            script,
            state: Mutex::default(),
        })
    }

    pub fn script(&self) -> &StoreScript {
        &self.script
    }

    /// The failures that hold from `decision`'s start.
    pub fn begin(&self, decision: &str) {
        self.state.lock().unwrap().failures = self
            .script
            .failures
            .get(decision)
            .cloned()
            .unwrap_or_default();
    }

    fn generations(s: &State, namespace: &str, scope: &str) -> EntryGenerations {
        EntryGenerations {
            namespace: s.namespace_generation.get(namespace).copied().unwrap_or(0),
            scope: s
                .scope_generation
                .get(&(namespace.to_string(), scope.to_string()))
                .copied()
                .unwrap_or(0),
        }
    }
}

impl RuntimeStore for ScriptedStore {
    fn contract(&self) -> &StoreContract {
        &self.script.contract
    }

    fn get<'a>(
        &'a self,
        namespace: &'a str,
        scope: &'a str,
        key_id: &'a str,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<Stored, StoreFailure>> {
        Box::pin(async move {
            let s = self.state.lock().unwrap();
            if s.failures.fail_get {
                return Err(failed("get"));
            }
            Ok(Stored {
                bytes: s
                    .entries
                    .get(&(namespace.to_string(), key_id.to_string()))
                    .map(|(_, b)| b.clone()),
                generations: Self::generations(&s, namespace, scope),
            })
        })
    }

    fn put<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        bytes: Vec<u8>,
        terms: PutTerms,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<PutAnswer, StoreFailure>> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            if s.failures.fail_put {
                return Err(failed("put"));
            }
            match s.failures.put_answer.as_deref() {
                None => {}
                Some("stored") => return Ok(PutAnswer::Stored),
                Some("generation_advanced") => return Ok(PutAnswer::GenerationAdvanced),
                Some("over_budget") => return Ok(PutAnswer::OverBudget),
                Some("conflict") => return Ok(PutAnswer::Conflict),
                Some(other) => return Err(failed(&format!("unknown put answer {other}"))),
            }
            if Self::generations(&s, namespace, &terms.scope) != terms.generations {
                return Ok(PutAnswer::GenerationAdvanced);
            }
            let k = (namespace.to_string(), key_id.to_string());
            if s.entries.contains_key(&k) {
                return Ok(PutAnswer::Conflict);
            }
            s.entries.insert(k, (terms.scope, bytes));
            Ok(PutAnswer::Stored)
        })
    }

    fn delete<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<(), StoreFailure>> {
        Box::pin(async move {
            self.state
                .lock()
                .unwrap()
                .entries
                .remove(&(namespace.to_string(), key_id.to_string()));
            Ok(())
        })
    }

    fn invalidate<'a>(
        &'a self,
        namespace: &'a str,
        selector: StoreSelector,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<Invalidated, StoreFailure>> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            if s.failures.fail_invalidate {
                return Err(failed("invalidate"));
            }
            let before = s.entries.len();
            let remove = self.script.remove_on_invalidate;
            let generation = match &selector {
                StoreSelector::Namespace => {
                    if remove {
                        s.entries.retain(|(n, _), _| n != namespace);
                    }
                    let g = s.namespace_generation.entry(namespace.into()).or_default();
                    *g += 1;
                    *g
                }
                StoreSelector::Scope(scope) => {
                    if remove {
                        s.entries
                            .retain(|(n, _), (sc, _)| n != namespace || sc != scope);
                    }
                    let g = s
                        .scope_generation
                        .entry((namespace.into(), scope.clone()))
                        .or_default();
                    *g += 1;
                    *g
                }
            };
            Ok(Invalidated {
                generation,
                removed: (before - s.entries.len()) as u64,
            })
        })
    }
}

/// An invalidation's result as the binding's `plan.invalidate()` resolves
/// it (063 3.6), keys sorted, one line: a failed or pending store
/// invalidation is `"pending"`.
pub fn invalidation_bytes(r: Option<&InvalidationResult>) -> Vec<u8> {
    let v = match r {
        None => Json::Null,
        Some(r) => json!({
            "complete": r.complete,
            "generation": r.generation,
            "persistent": match &r.persistent {
                None => Json::Null,
                Some(PersistentInvalidation::Done { generation, removed }) =>
                    json!({ "generation": generation, "removed": removed }),
                Some(_) => json!("pending"),
            },
            "removedBytes": r.removed_bytes,
            "removedEntries": r.removed_entries,
        }),
    };
    // serde_json's map is ordered by key here (no `preserve_order`).
    let mut b = serde_json::to_vec(&v).expect("a JSON value");
    b.push(b'\n');
    b
}
