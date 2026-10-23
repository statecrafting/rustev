use super::*;
use rustev_contract::{Document, limits::REPLAY_V1};

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    pub facts: usize,
    pub bytes: usize,
}
impl Default for Bounds {
    fn default() -> Self {
        Self {
            facts: MAX_FACTS,
            bytes: REPLAY_V1.max_bytes,
        }
    }
}
pub(super) fn too_large() -> ExplainError {
    ExplainError::new(
        RefusalCode::ExplanationTooLarge,
        "explanation",
        "keyed facts or document exceed bounds",
    )
}
fn construct(
    set: &evidence::Set,
    query: &Query,
    options: &ExplainOptions<'_>,
    bounds: Bounds,
    limit: usize,
) -> Result<(Explanation, usize), ExplainError> {
    let mut b = facts::Builder {
        set,
        policy: options.redaction,
        facts: vec![],
        total: 0,
        limit,
        bytes: 0,
        bounds,
    };
    let answer = match query {
        Query::WhyOutcome => outcome::build(&mut b)?,
        Query::TextOrigin => text::build(&mut b)?,
        _ => {
            return Err(ExplainError::new(
                RefusalCode::InvalidQuery,
                "query",
                "not yet implemented",
            ));
        }
    };
    let mut side = set.side.clone();
    redact::side(&mut side, options.redaction);
    let doc = Explanation {
        schema: rustev_contract::schema::EXPLANATION.into(),
        explainer: EXPLAINER.into(),
        facet_sources: vec![FacetSource {
            name: Facet::ServedIdentity,
            spec: "040-decision-explanation".into(),
            path: "supply/<n>:/served".into(),
        }],
        sides: vec![side],
        redaction: options.redaction.map(|p| Redaction {
            policy: p.policy_id(),
        }),
        query: query.clone(),
        answer,
        truncation: (b.total > limit).then_some(Truncation {
            total: b.total as u64,
        }),
        facts: b.facts,
    };
    let bytes = doc
        .record_canonical()
        .map_err(|e| ExplainError::parse("explanation", e))?;
    if bytes.len() > bounds.bytes || rustev_contract::bounded::scan(&bytes, &REPLAY_V1).is_err() {
        return Err(too_large());
    }
    doc.validate()
        .map_err(|e| ExplainError::parse("explanation", e))?;
    Ok((doc, b.total))
}
pub(super) fn build(
    set: &evidence::Set,
    query: &Query,
    options: &ExplainOptions<'_>,
    bounds: Bounds,
) -> Result<Explanation, ExplainError> {
    let (mut best, total) = construct(set, query, options, bounds, 0)?;
    let (mut low, mut high) = (0, total);
    // Rebuild over a prefix; omitted facts are counted but never allocated.
    // Binary search bounds work to logarithmically many bounded documents.
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        match construct(set, query, options, bounds, middle) {
            Ok((doc, _)) => {
                best = doc;
                low = middle;
            }
            Err(e) if e.code == RefusalCode::ExplanationTooLarge => high = middle - 1,
            Err(e) => return Err(e),
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tiny_bounds_keep_a_prefix_and_refuse_oversized_keys() {
        let digest = format!("sha256:{}", "0".repeat(64));
        let set = evidence::Set {
            docs: [("record".into(), json!({"termination":"completed", "core":{"recorded":{"judgment":{
                "outcome":{"propose":{"action":"send", "params":{"a":{"text":"écho"},"b":{"text":"second"}}}},
                "trace":[],"notices":[],"derivation":"exact-derived","lineage":{"inputs":[],"steps":[]}
            }}}}))].into(),
            absent: Default::default(), plan: None,
            side: serde_json::from_value(json!({"name":"a","form":"record","record":{"digest":digest},
                "plan":null,"bundle":null,"capture":null,"decision_id":"d","plan_id":digest})).unwrap(),
        };
        let options = ExplainOptions::default();
        let tiny = Bounds {
            facts: 2,
            ..Bounds::default()
        };
        let e = build(&set, &Query::TextOrigin, &options, tiny).unwrap();
        assert_eq!(e.facts.len(), 2);
        assert_eq!(e.truncation.unwrap().total, 4);
        assert_eq!(e.facts[0].value, "écho");
        assert_eq!(
            build(&set, &Query::WhyOutcome, &options, tiny)
                .unwrap_err()
                .code,
            RefusalCode::ExplanationTooLarge
        );
        let full = build(&set, &Query::TextOrigin, &options, Bounds::default()).unwrap();
        assert!(full.truncation.is_none());
        let bytes = full.record_canonical().unwrap().len() - 1;
        let e = build(
            &set,
            &Query::TextOrigin,
            &options,
            Bounds {
                bytes,
                ..Bounds::default()
            },
        )
        .unwrap();
        assert_eq!(e.facts.len(), 2);
        assert!(e.record_canonical().unwrap().len() <= bytes);
        let policy = RedactionPolicy {
            schema: rustev_contract::redaction::REDACTION.into(),
            classes: [(
                rustev_contract::redaction::FieldClass::PlanMetadata,
                rustev_contract::redaction::Action::Truncate { bytes: 1 },
            )]
            .into(),
            fields: vec![],
            content_events: vec![],
        };
        let mut value = json!("écho");
        let cut = redact::apply(
            &mut value,
            rustev_contract::redaction::FieldClass::PlanMetadata,
            Some(&policy),
            false,
        )
        .unwrap();
        assert_eq!(value, "");
        assert!(cut.is_some());
    }
}
