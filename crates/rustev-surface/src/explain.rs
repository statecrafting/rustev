//! Pure explanation command shared by the bindings (spec 040, 3.15.2).
use std::collections::BTreeMap;

use rustev_contract::{
    Document,
    explanation::{Query, RefusalCode},
    redaction::RedactionPolicy,
    scope::Scope,
    time::Timestamp,
};
use rustev_eval::{
    explain::{self, Evidence, ExplainError, ExplainOptions},
    resolve::{NoExternal, Resolver},
};
use serde::Deserialize;
use serde_json::Value;

/// Named byte inputs keep document parsing in the library, never in JavaScript.
#[derive(Deserialize)]
#[serde(untagged)]
enum Input {
    Record(Record),
    Bundle(Bundle),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    record: String,
    plan: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    bundle: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diff {
    other: Input,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffQuery {
    diff: Diff,
}

pub struct Explained {
    pub explanation: Vec<u8>,
    pub markdown: String,
}
fn error(code: RefusalCode, subject: &str, detail: impl ToString) -> ExplainError {
    ExplainError {
        code,
        subject: subject.into(),
        detail: detail.to_string(),
    }
}
fn invalid(detail: impl ToString) -> ExplainError {
    error(RefusalCode::InvalidOptions, "options", detail)
}
impl Input {
    fn bundle(&self) -> bool {
        matches!(self, Self::Bundle(_))
    }
    fn evidence<'a>(
        &self,
        files: &'a BTreeMap<String, Vec<u8>>,
        config: Option<&'a (Scope, Timestamp)>,
        resolver: &'a dyn Resolver,
    ) -> Result<Evidence<'a>, ExplainError> {
        let read = |name: &str| {
            files
                .get(name)
                .map(Vec::as_slice)
                .ok_or_else(|| error(RefusalCode::Parse, name, "missing byte input"))
        };
        Ok(match self {
            Self::Record(r) => Evidence::Record {
                record: read(&r.record)?,
                plan: r.plan.as_deref().map(read).transpose()?,
            },
            Self::Bundle(b) => {
                let (scope, now_ms) = config.expect("bundle options validated");
                Evidence::Bundle {
                    bundle: read(&b.bundle)?,
                    scope,
                    now_ms: *now_ms,
                    resolver,
                }
            }
        })
    }
}

/// Validate all options before parsing evidence or calling the resolver.
/// `args` holds named `evidence`, `query`, and `options`; documents stay bytes.
pub fn explain(
    files: &BTreeMap<String, Vec<u8>>,
    args: &Value,
    resolver: Option<&dyn Resolver>,
) -> Result<Explained, ExplainError> {
    let a: Input = serde_json::from_value(args["evidence"].clone()).map_err(invalid)?;
    let q = &args["query"];
    let b = if q.get("diff").is_some() {
        Some(
            serde_json::from_value::<DiffQuery>(q.clone())
                .map_err(|e| error(RefusalCode::InvalidQuery, "query", e))?
                .diff
                .other,
        )
    } else {
        None
    };
    let options = args["options"]
        .as_object()
        .ok_or_else(|| invalid("options must be an object"))?;
    for key in options.keys() {
        if !matches!(key.as_str(), "nowMs" | "scope" | "redaction") {
            return Err(invalid(format!("unknown option {key}")));
        }
    }
    let bundle = a.bundle() || b.as_ref().is_some_and(Input::bundle);
    let config = if bundle {
        let scope = serde_json::from_value(
            options
                .get("scope")
                .cloned()
                .ok_or_else(|| invalid("scope is required"))?,
        )
        .map_err(invalid)?;
        let ms = options
            .get("nowMs")
            .and_then(Value::as_f64)
            .filter(|n| n.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(n))
            .ok_or_else(|| invalid("nowMs must be a non-negative safe integer"))?;
        Some((scope, Timestamp::from_ms(ms as i64).map_err(invalid)?))
    } else {
        if options.contains_key("nowMs") || options.contains_key("scope") || resolver.is_some() {
            return Err(invalid("nowMs, scope and resolve require a bundle side"));
        }
        None
    };
    let policy = options
        .get("redaction")
        .map(|name| {
            let name = name
                .as_str()
                .ok_or_else(|| invalid("redaction must name byte input"))?;
            let bytes = files
                .get(name)
                .ok_or_else(|| error(RefusalCode::Parse, "redaction", "missing byte input"))?;
            RedactionPolicy::parse(bytes).map_err(|e| error(RefusalCode::Parse, "redaction", e))
        })
        .transpose()?;
    let opts = ExplainOptions {
        redaction: policy.as_ref(),
    };
    let resolver = resolver.unwrap_or(&NoExternal);
    let ae = a.evidence(files, config.as_ref(), resolver)?;
    let doc = if let Some(b) = b {
        explain::explain_diff(&ae, &b.evidence(files, config.as_ref(), resolver)?, &opts)?
    } else {
        let query: Query = serde_json::from_value(q.clone())
            .map_err(|e| error(RefusalCode::InvalidQuery, "query", e))?;
        explain::explain(&ae, &query, &opts)?
    };
    Ok(Explained {
        explanation: doc
            .record_canonical()
            .map_err(|e| error(RefusalCode::ExplanationTooLarge, "explanation", e))?,
        markdown: explain::render_markdown(&doc),
    })
}
