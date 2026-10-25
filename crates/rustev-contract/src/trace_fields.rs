//! The field catalogue of span schema `rustev.otel/1` (spec 028, 3.5 and
//! 3.6): the closed table of exportable field names and each one's class
//! rule. A name outside it is never exported, and a reader withholds it.
//!
//! Span attributes are named as in 3.5.4. An event attribute is catalogued
//! as `<event name>.<attribute key>` (`rustev.input.source` for the `source`
//! attribute of a `rustev.input` event), and a metric attribute that is no
//! span attribute as `<instrument>.<attribute key>`. Two names are
//! templates whose last segment is instance-specific:
//! `rustev.outcome.param.<name>` (the plan's parameter name) and
//! `rustev.exchange.extra.<key>` (a key the provider supplies).

use crate::redaction::FieldClass;

/// The span schema version of `rustev.otel/1` (3.2.4).
pub const SPAN_SCHEMA_VERSION: u32 = 1;
/// The pinned semantic-conventions schema URL (3.2.2).
pub const SCHEMA_URL: &str = "https://opentelemetry.io/schemas/1.41.1";

/// Span names (3.5.1). Constant; they never carry a value (3.4.4).
pub mod span {
    pub const DECISION: &str = "rustev.decision";
    pub const STEP: &str = "rustev.step";
    pub const ATTEMPT: &str = "rustev.attempt";
    pub const POLICY: &str = "rustev.policy";
    pub const ALL: [&str; 4] = [DECISION, STEP, ATTEMPT, POLICY];
}

/// Event names (3.5.5).
pub mod event {
    pub const INPUT: &str = "rustev.input";
    pub const NOTICE: &str = "rustev.notice";
    pub const TRANSITION: &str = "rustev.transition";
    pub const EXCLUDED: &str = "rustev.excluded";
    pub const POSITION: &str = "rustev.position";
    pub const ALL: [&str; 5] = [INPUT, NOTICE, TRANSITION, EXCLUDED, POSITION];
}

/// The fixed fields (3.4.4): always `allow`, never overridable.
pub mod fixed {
    pub const SCHEMA_VERSION: &str = "rustev.otel.schema_version";
    pub const POLICY_ID: &str = "rustev.redaction.policy_id";
    pub const KEY_ID: &str = "rustev.redaction.key_id";
    pub const WITHHELD: &str = "rustev.redaction.withheld";
}

/// The template prefix of outcome parameters (3.5.4).
pub const PARAM_PREFIX: &str = "rustev.outcome.param.";
/// The template prefix of provider extras (3.5.4).
pub const EXTRA_PREFIX: &str = "rustev.exchange.extra.";
/// How a dropped provider extra is listed in `rustev.redaction.withheld`:
/// once per span, never by its key (3.4.6).
pub const EXTRA_WITHHELD: &str = "rustev.exchange.extra.*";

/// How a catalogue field's class is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassRule {
    /// Always this class.
    Class(FieldClass),
    /// The lineage rule of 3.3.2, from the value and its judgment lineage.
    Lineage,
    /// A fixed field (3.4.4).
    Fixed,
}

/// One catalogue entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogueField {
    /// The exact name, or for a template the prefix ending in `.`.
    pub name: &'static str,
    pub rule: ClassRule,
    /// A template: matches `name` followed by one non-empty segment.
    pub template: bool,
    /// A GenAI content field (3.5.7). None exists until #76 B5 or B6 adds
    /// content to the records.
    pub content: bool,
}

const fn f(name: &'static str, class: FieldClass) -> CatalogueField {
    CatalogueField {
        name,
        rule: ClassRule::Class(class),
        template: false,
        content: false,
    }
}

const fn fixed_field(name: &'static str) -> CatalogueField {
    CatalogueField {
        name,
        rule: ClassRule::Fixed,
        template: false,
        content: false,
    }
}

use FieldClass::{
    ContentDigest as CD, Enum as E, FreeTextDetail as FT, Id as ID, ModelIdentity as MI,
    Number as N, PlanMetadata as PM, ThirdParty as TP,
};

/// The catalogue (3.5.4, 3.5.5, 3.6.1), sorted by name.
pub const CATALOGUE: &[CatalogueField] = &[
    f("error.type", E),
    f("gen_ai.operation.name", E),
    f("gen_ai.provider.name", MI),
    f("gen_ai.request.model", MI),
    f("gen_ai.response.id", ID),
    f("gen_ai.response.model", MI),
    f("gen_ai.token.type", E),
    f("gen_ai.usage.input_tokens", N),
    f("gen_ai.usage.output_tokens", N),
    f("rustev.acquisition.attempt_id", ID),
    f("rustev.acquisition.cache", E),
    f("rustev.acquisition.cache.age_ms", N),
    f("rustev.acquisition.cancellation", E),
    f("rustev.acquisition.constructed", E),
    f("rustev.acquisition.cost.bound", E),
    f("rustev.acquisition.cost.charge", N),
    f("rustev.acquisition.cost.reserved", N),
    f("rustev.acquisition.derivation", E),
    f("rustev.acquisition.digest", CD),
    f("rustev.acquisition.dispatched_ms", N),
    f("rustev.acquisition.end", E),
    f("rustev.acquisition.ended_ms", N),
    f("rustev.acquisition.guard", PM),
    f("rustev.acquisition.item.digest", CD),
    f("rustev.acquisition.operation", PM),
    f("rustev.acquisition.outcome", E),
    f("rustev.acquisition.page", N),
    f("rustev.acquisition.rank", N),
    f("rustev.acquisition.relaxation", N),
    f("rustev.acquisition.remote_state", E),
    f("rustev.acquisition.request", CD),
    f("rustev.acquisition.request_constructed", E),
    f("rustev.acquisition.source", PM),
    f("rustev.acquisition.step", PM),
    f("rustev.acquisition.stopped_by", E),
    f("rustev.acquisition.tier", PM),
    f("rustev.acquisition.tier_index", N),
    f("rustev.acquisition.truncated", N),
    f("rustev.acquisition.unique_items_added", N),
    f("rustev.attempt.artifact", PM),
    f("rustev.attempt.backend_id", PM),
    f("rustev.attempt.cancel", E),
    f("rustev.attempt.charge_kind", E),
    f("rustev.attempt.charge_units", N),
    f("rustev.attempt.cost.bound", E),
    f("rustev.attempt.cost.reserved", N),
    f("rustev.attempt.end", E),
    f("rustev.attempt.failure_class", E),
    f("rustev.attempt.failure_detail", FT),
    f("rustev.attempt.id", ID),
    f("rustev.attempt.ordinal", N),
    f("rustev.attempt.remote_state", E),
    f("rustev.attempt.retry_index", N),
    f("rustev.attempt.target", N),
    f("rustev.batch.id", ID),
    f("rustev.batch.member", N),
    f("rustev.cost.bound_violations", N),
    f("rustev.cost.estimated", N),
    f("rustev.cost.final", E),
    f("rustev.cost.liability", N),
    f("rustev.cost.limit", N),
    f("rustev.cost.mode", E),
    f("rustev.cost.observed", N),
    f("rustev.cost.within_guaranteed_cap", E),
    f("rustev.deadline.remaining_ms", N),
    f("rustev.decision.id", ID),
    f("rustev.definition.id", PM),
    f("rustev.derivation", E),
    f("rustev.events_cut", N),
    f("rustev.evidence.sink.result", E),
    f("rustev.exchange.bytes_sent", E),
    CatalogueField {
        name: EXTRA_PREFIX,
        rule: ClassRule::Class(TP),
        template: true,
        content: false,
    },
    f("rustev.exchange.late", E),
    f("rustev.exchange.member_detail", FT),
    f("rustev.exchange.reported_cost", N),
    f("rustev.exchange.reported_currency", E),
    f("rustev.exchange.request_digest", CD),
    f("rustev.exchange.response_digest", CD),
    f("rustev.exchange.retry_after", TP),
    f("rustev.exchange.route", MI),
    f("rustev.exchange.status", N),
    f("rustev.exchange.truncated", E),
    f("rustev.excluded.candidate", ID),
    f("rustev.excluded.reasons", PM),
    f("rustev.execution.policy_id", PM),
    f("rustev.fallbacks.kind", E),
    f("rustev.generation.backend_id", PM),
    f("rustev.generation.field", PM),
    f("rustev.generation.helpers_version", PM),
    f("rustev.generation.max_items", N),
    f("rustev.generation.template_id", CD),
    f("rustev.generation.template_name", PM),
    f("rustev.generation.worst_case_projection_bytes", N),
    f("rustev.generation.worst_case_requests", N),
    f("rustev.input.as_of_ms", N),
    f("rustev.input.field", PM),
    f("rustev.input.provenance", E),
    f("rustev.input.source", ID),
    f("rustev.input.status", E),
    f("rustev.input.unresolved_reason", E),
    f("rustev.lineage.inputs", PM),
    f("rustev.lineage.steps", PM),
    f("rustev.notice.kind", E),
    f("rustev.notice.reason", PM),
    f("rustev.notice.subject", PM),
    fixed_field(fixed::SCHEMA_VERSION),
    f("rustev.outcome", E),
    f("rustev.outcome.action", PM),
    f("rustev.outcome.escalate_reason", PM),
    CatalogueField {
        name: PARAM_PREFIX,
        rule: ClassRule::Lineage,
        template: true,
        content: false,
    },
    f("rustev.outcome.unresolved_detail", FT),
    f("rustev.outcome.unresolved_fields", PM),
    f("rustev.outcome.unresolved_reason", E),
    f("rustev.plan.id", PM),
    f("rustev.policy.excluded_count", N),
    f("rustev.policy.rules_fired", PM),
    f("rustev.policy.truncated", N),
    f("rustev.position.candidate", ID),
    f("rustev.position.position", N),
    CatalogueField {
        name: "rustev.position.score",
        rule: ClassRule::Lineage,
        template: false,
        content: false,
    },
    fixed_field(fixed::KEY_ID),
    fixed_field(fixed::POLICY_ID),
    fixed_field(fixed::WITHHELD),
    f("rustev.screening.artifact", PM),
    f("rustev.screening.backend_id", PM),
    f("rustev.screening.cache", E),
    f("rustev.screening.categories", E),
    f("rustev.screening.consequence", E),
    f("rustev.screening.descriptor", PM),
    f("rustev.screening.entry", PM),
    f("rustev.screening.field", PM),
    f("rustev.screening.item", ID),
    f("rustev.screening.items", ID),
    f("rustev.screening.source_attempt", ID),
    f("rustev.screening.step", PM),
    f("rustev.screening.text_digest", CD),
    f("rustev.screening.units.basis", E),
    f("rustev.screening.units.bound_violation.computed", N),
    f("rustev.screening.units.bound_violation.reported", N),
    f("rustev.screening.units.charged", N),
    f("rustev.screening.verdict", E),
    f("rustev.served.disclosure", E),
    f("rustev.snapshot.id", CD),
    f("rustev.step.backend_id", PM),
    f("rustev.step.derivation", E),
    f("rustev.step.id", PM),
    f("rustev.step.instance", ID),
    f("rustev.step.kind", E),
    f("rustev.step.operator", PM),
    f("rustev.step.optimization.mechanism", E),
    f("rustev.step.optimization.outcome", E),
    f("rustev.step.result", E),
    f("rustev.step.reused_from", ID),
    f("rustev.step.status", E),
    f("rustev.step.target", N),
    f("rustev.step.unresolved_reason", E),
    f("rustev.step.value_kind", E),
    f("rustev.telemetry.lost.reason", E),
    f("rustev.termination", E),
    f("rustev.time_anchor", E),
    f("rustev.timing", E),
    f("rustev.timing.deadline_expired", E),
    f("rustev.timing.deadline_ms", N),
    f("rustev.timing.elapsed_ms", N),
    f("rustev.timing.queued_ms", N),
    f("rustev.transition.after", N),
    f("rustev.transition.class", E),
    f("rustev.transition.delay_ms", N),
    f("rustev.transition.kind", E),
    f("rustev.transition.reason", FT),
    f("rustev.transition.to", N),
    f("rustev.unresolved.scope", E),
];

/// One instance-specific segment of a template name: non-empty, at most
/// 128 bytes, with no `.`, `*`, whitespace or control character (so
/// `rustev.outcome.param.a.b` and `rustev.exchange.extra.*` are no field).
fn is_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s
            .chars()
            .any(|c| c == '.' || c == '*' || c.is_whitespace() || c.is_control())
}

/// The catalogue entry `name` names, exactly or as a template instance.
pub fn lookup(name: &str) -> Option<&'static CatalogueField> {
    CATALOGUE.iter().find(|e| {
        if e.template {
            name.strip_prefix(e.name).is_some_and(is_segment)
        } else {
            e.name == name
        }
    })
}

/// The catalogue name of attribute `key` of event `event`.
pub fn event_field(event: &str, key: &str) -> String {
    format!("{event}.{key}")
}

/// The name a dropped field is listed under in `rustev.redaction.withheld`
/// (3.4.6): its own name, except provider extras.
pub fn withheld_name(field: &str) -> &str {
    if field.starts_with(EXTRA_PREFIX) {
        EXTRA_WITHHELD
    } else {
        field
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_sorted_and_unique() {
        for w in CATALOGUE.windows(2) {
            assert!(w[0].name < w[1].name, "{} / {}", w[0].name, w[1].name);
        }
    }

    #[test]
    fn templates_match_one_segment() {
        assert!(lookup("rustev.outcome.param.amount").is_some());
        assert!(lookup("rustev.outcome.param.").is_none());
        assert!(lookup("rustev.outcome.param.a.b").is_none());
        assert_eq!(
            lookup("rustev.exchange.extra.x_cache").map(|e| e.rule),
            Some(ClassRule::Class(FieldClass::ThirdParty))
        );
        assert!(lookup("rustev.exchange.extra.*").is_none());
        assert!(lookup("rustev.nope").is_none());
    }

    #[test]
    fn fixed_fields_are_the_four_of_3_4_4() {
        let fixed: Vec<_> = CATALOGUE
            .iter()
            .filter(|e| e.rule == ClassRule::Fixed)
            .map(|e| e.name)
            .collect();
        assert_eq!(
            fixed,
            vec![
                fixed::SCHEMA_VERSION,
                fixed::KEY_ID,
                fixed::POLICY_ID,
                fixed::WITHHELD
            ]
        );
    }

    #[test]
    fn no_content_field_exists_yet() {
        assert!(CATALOGUE.iter().all(|e| !e.content));
    }

    #[test]
    fn extras_are_withheld_without_their_key() {
        assert_eq!(
            withheld_name("rustev.exchange.extra.secret"),
            EXTRA_WITHHELD
        );
        assert_eq!(withheld_name("rustev.decision.id"), "rustev.decision.id");
    }
}
