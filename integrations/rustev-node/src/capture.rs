//! Caller-only capture assembly (spec 063, 3.8). No retained decision state.
use std::cell::RefCell;
use std::collections::HashMap;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rustev_contract::Document;
use rustev_contract::replay::{Capture, CaptureStatus};
use rustev_contract::time::{DurationMs, Timestamp};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble_with_sources};
use rustev_runtime::CaptureConfig;
use serde_json::{Value, json};

use crate::errors::fail;

pub struct Options {
    pub config: CaptureConfig,
    pub created: Timestamp,
    lifetime: u64,
    cap: u64,
    retain: String,
}

fn refused(code: &str, field: &str) -> napi::Error {
    // Never echo caller content into an error detail.
    fail("CaptureConfigRefused", code, json!({"field": field}))
}

impl Options {
    pub fn parse(v: Value) -> Result<Self> {
        let number = |field: &str, default: Option<u64>| -> Result<u64> {
            match v.get(field) {
                None => default.ok_or_else(|| refused("out_of_range", field)),
                Some(n) => n
                    .as_f64()
                    .filter(|n| n.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(n))
                    .map(|n| n as u64)
                    .ok_or_else(|| refused("out_of_range", field)),
            }
        };
        let max_bytes = number("maxBytes", None)?;
        if max_bytes == 0 {
            return Err(refused("zero_limit", "maxBytes"));
        }
        if max_bytes > 16_777_216 {
            return Err(refused("limit_too_large", "maxBytes"));
        }
        let scope = serde_json::from_value(v.get("scope").cloned().unwrap_or(Value::Null))
            .map_err(|_| refused("invalid_scope", "scope"))?;
        let created = Timestamp::from_ms(number("createdAtMs", None)? as i64)
            .map_err(|_| refused("out_of_range", "createdAtMs"))?;
        let lifetime = number("lifetimeMs", Some(604_800_000))?;
        let cap = number("hostCapMs", Some(2_592_000_000))?;
        if cap == 0 || cap > 2_592_000_000 {
            return Err(refused("out_of_range", "hostCapMs"));
        }
        if lifetime == 0 || lifetime > cap || created.checked_add(DurationMs(lifetime)).is_err() {
            return Err(refused("out_of_range", "lifetimeMs"));
        }
        let retain = match v.get("retain") {
            None => "digest_only",
            Some(Value::String(s))
                if matches!(s.as_str(), "digest_only" | "embedded" | "external") =>
            {
                s
            }
            _ => return Err(refused("out_of_range", "retain")),
        }
        .to_owned();
        Ok(Self {
            config: CaptureConfig { scope, max_bytes },
            created,
            lifetime,
            cap,
            retain,
        })
    }

    pub fn assemble(&self, input: &AssemblyInput<'_>) -> Outcome {
        let status = match input.capture.map(|c| c.status) {
            Some(CaptureStatus::Complete) => "complete",
            Some(CaptureStatus::LimitExceeded) => "limit_exceeded",
            Some(CaptureStatus::Cancelled) => "cancelled",
            _ => "abandoned",
        };
        let mut out = Outcome {
            status: status.into(),
            bundle: None,
            items: None,
            error: None,
        };
        if status == "limit_exceeded" {
            return out;
        }
        let items = RefCell::new(HashMap::new());
        let external = |_, bytes: &[u8]| {
            // Assembly supplies canonical documents, each with its schema.
            let doc: Value = serde_json::from_slice(bytes).expect("canonical document");
            let reference = rustev_contract::canonical::tagged_digest(
                doc["schema"].as_str().expect("document schema"),
                bytes,
            );
            items
                .borrow_mut()
                .insert(reference.clone(), Uint8Array::new(bytes.to_vec()));
            reference
        };
        let choice = RetentionChoice {
            lifetime_ms: self.lifetime,
            host_cap_ms: Some(self.cap),
            content: match self.retain.as_str() {
                "embedded" => Content::Embedded,
                "external" => Content::External(&external),
                _ => Content::DigestOnly,
            },
        };
        // Acquisition source descriptors join with increment 3.
        match assemble_with_sources(input, &choice, &[]) {
            Ok(bundle) => match bundle.record_canonical() {
                Ok(bytes) => {
                    out.bundle = Some(Uint8Array::new(bytes));
                    if self.retain == "external" {
                        out.items = Some(items.into_inner());
                    }
                }
                Err(_) => out.error = Some(json!({"code": "not_canonical", "detail": null})),
            },
            Err(e) => {
                use rustev_eval::assemble::AssemblyError::*;
                let code = match e {
                    Lifetime { .. } => "lifetime",
                    Binding { .. } => "binding",
                    Plan(_) => "plan",
                    NotCanonical { .. } => "not_canonical",
                    TooLarge { .. } => "too_large",
                    Bundle(_) => "bundle",
                };
                out.error = Some(json!({"code": code, "detail": null}));
            }
        }
        out
    }
}

#[napi(object)]
pub struct Outcome {
    pub status: String,
    pub bundle: Option<Uint8Array>,
    pub items: Option<HashMap<String, Uint8Array>>,
    pub error: Option<Value>,
}

pub fn captured(outcome: &rustev_runtime::CaptureOutcome) -> Option<&Capture> {
    match outcome {
        rustev_runtime::CaptureOutcome::Captured(c) => Some(c),
        rustev_runtime::CaptureOutcome::NotAdmitted => None,
    }
}
