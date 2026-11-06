//! Scripted acquisition sources for runtime cases (spec 063, 3.13.2): each
//! source answers its calls from data, per decision in call order, so the
//! Rust runner and a binding's runner give the runtime the same pages and
//! failures and must produce the same run-record bytes.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rustev_contract::acquisition::{AcquisitionRequest, OperationCost};
use rustev_contract::run::{Charge, CostBound};
use rustev_contract::source::SourceDescriptor;
use rustev_core::seams::{
    AcquireCall, AcquireReport, AcquisitionFailure, AcquisitionSource, BoxFuture, CancelAck,
};
use serde::Deserialize;
use serde_json::{Value as Json, json};

/// One scripted acquisition source.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceScript {
    /// The input name of its `rustev.source/1` descriptor.
    pub descriptor: String,
    pub max_concurrency: usize,
    /// Per decision id, the answer of each call in order (the last repeats).
    pub answers: BTreeMap<String, Vec<SourceAnswer>>,
}

/// What one call does.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceAnswer {
    /// A `rustev.acquisition-page/1` for the call's request.
    Page {
        items: Vec<Json>,
        next: Json,
        as_of_ms: i64,
        charge: Charge,
    },
    Fail {
        class: SourceClass,
        detail: String,
        charge: Charge,
    },
    /// Bytes that are not a page.
    Invalid { charge: Charge },
    /// Never answers: the runtime's window or cancellation ends the call.
    /// A host answer would cross threads and could never be ready when the
    /// runtime polls once, so no answer is the outcome both runners share.
    Ignore,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    Transient,
    Overloaded,
    Permanent,
    Denied,
}

/// The page bytes both runners write: object keys sorted at every level,
/// no whitespace.
pub fn page_bytes(request_id: &str, items: &[Json], next: &Json, as_of_ms: i64) -> Vec<u8> {
    let page = json!({ "as_of_ms": as_of_ms, "items": items, "next": next,
                       "request": request_id, "schema": "rustev.acquisition-page/1" });
    let mut out = String::new();
    write_sorted(&page, &mut out);
    out.into_bytes()
}

fn write_sorted(v: &Json, out: &mut String) {
    match v {
        Json::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Json::String((*k).clone()).to_string());
                out.push(':');
                write_sorted(&m[*k], out);
            }
            out.push('}');
        }
        Json::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_sorted(x, out);
            }
            out.push(']');
        }
        v => out.push_str(&v.to_string()),
    }
}

/// A source answering from its script.
pub struct ScriptedSource {
    descriptor: SourceDescriptor,
    script: SourceScript,
    calls: Mutex<BTreeMap<String, usize>>,
}

impl ScriptedSource {
    pub fn new(descriptor: SourceDescriptor, script: SourceScript) -> Arc<Self> {
        Arc::new(ScriptedSource {
            descriptor,
            script,
            calls: Mutex::new(BTreeMap::new()),
        })
    }

    /// Calls received so far, over every decision.
    pub fn calls(&self) -> usize {
        self.calls.lock().unwrap().values().sum()
    }
}

impl AcquisitionSource for ScriptedSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    /// From the operation's declared cost, as 063 3.5.3 derives it.
    fn cost_bound(&self, request: &AcquisitionRequest) -> CostBound {
        let op = self
            .descriptor
            .operations
            .iter()
            .flatten()
            .find(|o| o.name == request.operation);
        match op.map(|o| &o.cost) {
            Some(OperationCost::Bounded { max_units }) => CostBound::Bounded {
                max_units: *max_units,
            },
            _ => CostBound::Unknown,
        }
    }

    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        let decision = call.cx.trace_id.clone();
        let n = {
            let mut calls = self.calls.lock().unwrap();
            let n = calls.entry(decision.clone()).or_default();
            *n += 1;
            *n
        };
        let answer = match self.script.answers.get(&decision) {
            Some(l) if !l.is_empty() => l[n.min(l.len()) - 1].clone(),
            _ => SourceAnswer::Fail {
                class: SourceClass::Permanent,
                detail: format!("no scripted answer for {decision}"),
                charge: Charge::Observed { units: 0 },
            },
        };
        let request_id = serde_json::to_value(call.request_id)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let cancel = call.cancel.clone();
        Box::pin(async move {
            let (result, charge) = match answer {
                SourceAnswer::Page {
                    items,
                    next,
                    as_of_ms,
                    charge,
                } => (Ok(page_bytes(&request_id, &items, &next, as_of_ms)), charge),
                SourceAnswer::Fail {
                    class,
                    detail,
                    charge,
                } => (
                    Err((
                        match class {
                            SourceClass::Transient => AcquisitionFailure::Transient,
                            SourceClass::Overloaded => AcquisitionFailure::Overloaded,
                            SourceClass::Permanent => AcquisitionFailure::Permanent,
                            SourceClass::Denied => AcquisitionFailure::Denied,
                        },
                        detail,
                    )),
                    charge,
                ),
                SourceAnswer::Invalid { charge } => (Ok(b"invalid page".to_vec()), charge),
                SourceAnswer::Ignore => std::future::pending().await,
            };
            // An answer that arrives with cancellation already raised still
            // stands; the source did not act on the cancellation.
            AcquireReport {
                result,
                charge,
                cancel: if cancel.is_raised() {
                    CancelAck::Unconfirmed
                } else {
                    CancelAck::NotRequested
                },
            }
        })
    }
}
