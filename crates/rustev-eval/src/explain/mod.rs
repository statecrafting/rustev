//! Pure explanations of retained decision evidence (spec 040).
mod bounds;
mod evidence;
mod facts;
mod models;
mod outcome;
mod redact;
mod scopes;
mod selected;
mod text;

use crate::resolve::Resolver;
use rustev_contract::explanation::*;
use rustev_contract::redaction::RedactionPolicy;
use rustev_contract::scope::Scope;
use rustev_contract::time::Timestamp;

pub enum Evidence<'a> {
    Record {
        record: &'a [u8],
        plan: Option<&'a [u8]>,
    },
    Bundle {
        bundle: &'a [u8],
        scope: &'a Scope,
        now_ms: Timestamp,
        resolver: &'a dyn Resolver,
    },
}
#[derive(Default)]
pub struct ExplainOptions<'a> {
    pub redaction: Option<&'a RedactionPolicy>,
}
#[derive(Debug)]
pub struct ExplainError {
    pub code: RefusalCode,
    pub subject: String,
    pub detail: String,
}
impl std::fmt::Display for ExplainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.code.as_str(),
            self.subject,
            self.detail
        )
    }
}
impl std::error::Error for ExplainError {}
impl ExplainError {
    fn new(code: RefusalCode, subject: &str, detail: impl ToString) -> Self {
        Self {
            code,
            subject: subject.into(),
            detail: detail.to_string(),
        }
    }
    fn parse(subject: &str, e: impl ToString) -> Self {
        Self::new(RefusalCode::Parse, subject, e)
    }
}
pub fn explain(
    evidence: &Evidence<'_>,
    query: &Query,
    options: &ExplainOptions<'_>,
) -> Result<Explanation, ExplainError> {
    query
        .validate()
        .map_err(|c| ExplainError::new(c, "query", "invalid query"))?;
    if !matches!(
        query,
        Query::WhyOutcome | Query::TextOrigin | Query::Models | Query::WhySelected { .. }
    ) {
        return Err(ExplainError::new(
            RefusalCode::InvalidQuery,
            "query",
            "not yet implemented",
        ));
    }
    if let Some(p) = options.redaction {
        p.validate()
            .map_err(|e| ExplainError::parse("redaction", e))?;
    }
    let evidence = evidence::Set::read(evidence)?;
    bounds::build(&evidence, query, options, bounds::Bounds::default())
}
