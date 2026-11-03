//! Caller-side preparation (spec 011, 3.8). No encoder or worker is started here.

use rustev_contract::bounded::{ParseLimits, parse_bounded};
use serde_json::Value;
use tokenizers::pipeline::{EncodeOptions, Override, PipelineTokenizer};

use crate::manifest::{Head, Truncation, field_ref};
use crate::{EmbedSetupError, SetupCode, VerifiedBundle};

/// Permanent request refusals reached before encoder handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestCode {
    ProjectionTooLarge,
    TaskNotFound,
    FieldMissing,
    FieldType,
    TextEmpty,
    TextTooLong,
    TooManyTokens,
    RuntimeError,
}

impl RequestCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectionTooLarge => "projection_too_large",
            Self::TaskNotFound => "task_not_found",
            Self::FieldMissing => "field_missing",
            Self::FieldType => "field_type",
            Self::TextEmpty => "text_empty",
            Self::TextTooLong => "text_too_long",
            Self::TooManyTokens => "too_many_tokens",
            Self::RuntimeError => "runtime_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestError {
    pub code: RequestCode,
    pub item: String,
}

impl RequestError {
    fn new(code: RequestCode, item: &str) -> Self {
        Self {
            code,
            item: item.into(),
        }
    }

    /// The seam's detail, capped at 256 UTF-8 bytes without splitting a character.
    pub fn detail(&self) -> String {
        let mut s = format!("embed:{}:{}", self.code.as_str(), self.item);
        let mut end = s.len().min(256);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail())
    }
}
impl std::error::Error for RequestError {}

/// The selected head and single unpadded sequence, ready for encoder handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRequest {
    pub head: usize,
    pub ids: Vec<u32>,
    pub attention_mask: Vec<u32>,
}

/// Owns verified bytes and a tokenizer read from those bytes. This is the
/// preparation half of the backend, not a `DecisionBackend` implementation.
pub struct RequestPreparer {
    pub(crate) bundle: VerifiedBundle,
    tokenizer: PipelineTokenizer,
}

impl RequestPreparer {
    pub fn new(bundle: VerifiedBundle) -> Result<Self, EmbedSetupError> {
        let invalid = |detail: String| EmbedSetupError {
            code: SetupCode::ManifestInvalid,
            item: "tokenizer".into(),
            detail,
        };
        if bundle.manifest.tokenizer.implementation != "tokenizers@1.0.0-rc.3" {
            return Err(invalid(
                "implementation differs from the pinned tokenizer".into(),
            ));
        }
        let text = std::str::from_utf8(&bundle.tokenizer).map_err(|e| invalid(e.to_string()))?;
        // Same pinned legacy-format upgrade as the feasibility probe. The
        // verified source bytes and their identity are retained unchanged.
        let canonical = tokenizers::canonicalize_str(text).map_err(|e| invalid(e.to_string()))?;
        let tokenizer = tokenizers::from_json(&canonical).map_err(|e| invalid(e.to_string()))?;
        Ok(Self { bundle, tokenizer })
    }

    pub fn bundle(&self) -> &VerifiedBundle {
        &self.bundle
    }

    /// Match, assemble and tokenize on the caller. Failures here incur no
    /// worker handoff and therefore no charge (3.11.4).
    pub fn prepare(&self, projection: &[u8]) -> Result<PreparedRequest, RequestError> {
        use RequestCode as C;
        let m = &self.bundle.manifest;
        let max = m.limits.max_projection_bytes as usize;
        if projection.len() > max {
            return Err(RequestError::new(C::ProjectionTooLarge, "projection"));
        }
        let limits = ParseLimits {
            max_bytes: max,
            max_depth: 16,
            max_string_bytes: max,
            max_collection_len: 4096,
            max_total_values: 50_000,
            max_number_bytes: max,
            allow_fractional_numbers: false,
        };
        // A malformed projection cannot select a task; never echo its data.
        let doc: Value = parse_bounded(projection, &limits)
            .map_err(|_| RequestError::new(C::TaskNotFound, "projection"))?;
        let head = m
            .heads
            .iter()
            .position(|h| matches_head(h, &doc))
            .ok_or_else(|| RequestError::new(C::TaskNotFound, "task"))?;
        let text = assemble(&m.heads[head], &doc, m.limits.max_text_bytes as usize)?;
        let options = EncodeOptions {
            padding: Override::Off,
            truncation: Override::Off,
            ..EncodeOptions::default()
        };
        let encodings = self
            .tokenizer
            .encode(text.as_str(), &options)
            .wait()
            .map_err(|_| RequestError::new(C::RuntimeError, "tokenizer"))?;
        if encodings.len() != 1 {
            return Err(RequestError::new(C::RuntimeError, "tokenizer.sequence"));
        }
        let mut ids: Vec<u32> = encodings[0].ids().iter().map(|t| t.id()).collect();
        let max = m.tokenizer.max_tokens as usize;
        if ids.len() > max {
            if m.tokenizer.truncation == Truncation::Refuse {
                return Err(RequestError::new(C::TooManyTokens, "tokenizer.max_tokens"));
            }
            // Cut after special-token processing, without replacing the last
            // retained token with a separator token (3.8.4).
            ids.truncate(max);
        }
        Ok(PreparedRequest {
            head,
            attention_mask: vec![1; ids.len()],
            ids,
        })
    }
}

fn matches_head(h: &Head, doc: &Value) -> bool {
    doc.get("operation") == serde_json::to_value(h.operation).ok().as_ref()
        && doc.get("task").and_then(Value::as_str) == Some(h.task.as_str())
        && doc.get("question").and_then(Value::as_str) == Some(h.question.as_str())
        && doc.get("options") == Some(&serde_json::json!(h.options))
}

fn assemble(h: &Head, doc: &Value, max: usize) -> Result<String, RequestError> {
    use RequestCode as C;
    let mut text = String::new();
    for (i, f) in h.text.fields.iter().enumerate() {
        let error = |code| RequestError::new(code, &f.reference);
        let (key, member) = field_ref(&f.reference).ok_or_else(|| error(C::FieldMissing))?;
        let mut value = doc.get("values").and_then(|v| v.get(key));
        if let Some(member) = member {
            value = match value {
                Some(Value::Object(record)) => record.get(member),
                None | Some(Value::Null) => None,
                _ => return Err(error(C::FieldType)),
            };
        }
        let value = match value {
            None | Some(Value::Null) => return Err(error(C::FieldMissing)),
            Some(Value::String(s)) => s,
            _ => return Err(error(C::FieldType)),
        };
        let separator = if i == 0 { "" } else { &h.text.separator };
        let colon = if f.label.is_empty() { "" } else { ": " };
        if text.len() + separator.len() + f.label.len() + colon.len() + value.len() > max {
            return Err(error(C::TextTooLong));
        }
        text.push_str(separator);
        text.push_str(&f.label);
        text.push_str(colon);
        text.push_str(value);
    }
    if text.trim().is_empty() {
        return Err(RequestError::new(C::TextEmpty, "text"));
    }
    Ok(text)
}
