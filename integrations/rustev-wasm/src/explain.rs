//! The pure engine API for retained explanations (040 3.15.2).
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

use super::{CommandResult, Inputs};
use rustev_surface::out::{Out, code};

/// A pure engine: no runtime configuration, executor or clock.
#[wasm_bindgen]
#[derive(Default)]
pub struct Engine;
#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Self
    }

    /// The wrapper supplies named bytes and JSON, and constructs public errors.
    #[wasm_bindgen(skip_typescript)]
    pub fn explain(
        &self,
        inputs: &Inputs,
        args: &str,
        resolve: Option<js_sys::Function>,
    ) -> CommandResult {
        let args = match serde_json::from_str(args) {
            Ok(args) => args,
            Err(_) => {
                return failure(
                    "invalid-options",
                    serde_json::json!({"detail": "expected JSON"}),
                );
            }
        };
        let resolver = resolve.map(super::Callback);
        match rustev_surface::explain::explain(
            &inputs.files,
            &args,
            resolver
                .as_ref()
                .map(|r| r as &dyn rustev_eval::resolve::Resolver),
        ) {
            Ok(result) => CommandResult {
                code: code::OK,
                out: Vec::new(),
                artifacts: BTreeMap::from([
                    ("explanation".into(), result.explanation),
                    ("markdown".into(), result.markdown.into_bytes()),
                ]),
            },
            Err(e) => failure(
                e.code.as_str(),
                serde_json::json!({"subject": e.subject, "detail": e.detail}),
            ),
        }
    }
}

fn failure(code: &str, detail: serde_json::Value) -> CommandResult {
    CommandResult {
        code: code::USAGE,
        out: Out::new("explain", code).set("detail", &detail).bytes(),
        artifacts: BTreeMap::new(),
    }
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
export type ExplainEvidence = { record: Uint8Array; plan?: Uint8Array } | { bundle: Uint8Array };
export type ExplainQuery = { why_selected: { item: string } } | { why_excluded: { item: string } }
  | { why_order: { a: string; b: string } } | 'why_outcome' | 'text_origin' | 'models'
  | { diff: { other: ExplainEvidence } };
export interface ExplainOptions {
  nowMs?: number;
  scope?: { mode: 'principal'; tenant: string; context_revision: string; principal_scope: string }
    | { mode: 'tenant_only'; tenant: string; context_revision: string };
  resolve?: (reference: string, maxBytes: number) => Uint8Array | null;
  redaction?: Uint8Array;
}
export interface ExplainResult { explanation: Uint8Array; markdown: string }
export interface Engine {
  explain(evidence: ExplainEvidence, query: ExplainQuery, options?: ExplainOptions): Promise<ExplainResult>;
}
"#;
