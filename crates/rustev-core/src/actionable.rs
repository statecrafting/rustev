//! Provenance-conditional checks and actionable outcomes (spec 033).
//!
//! Increment 1 recognizes every construct of the spec so a version 1
//! definition refuses it (3.1.1, category 11, `requires-definition-2`) and
//! a version 2 definition is refused until the compilation phase of 3.8
//! lands; nothing here evaluates a check yet.

use rustev_contract::definition::{Cond, Definition, HandlerAction, OutcomeDecl};

/// The first construct of spec 033 in `def`, with its subject, in the order
/// the definition declares them: the `actionability` member, then policy
/// handlers, rules and adjustments.
pub(crate) fn first_construct(def: &Definition) -> Option<(String, &'static str)> {
    if def.actionability.is_some() {
        return Some(("definition".into(), "an actionability declaration"));
    }
    let policy = &def.policy;
    for (n, h) in policy.on_unresolved.iter().enumerate() {
        match h.action {
            HandlerAction::PropagateWith { .. } => {
                return Some((format!("policy:handler:{n}"), "a propagate_with handler"));
            }
            HandlerAction::NeedsInput { .. } => {
                return Some((format!("policy:handler:{n}"), "a needs_input handler"));
            }
            _ => {}
        }
    }
    for r in &policy.rules {
        let subject = format!("policy:rule:{}", r.id);
        if cond_uses(&r.when) {
            return Some((subject, "a provenance_within condition"));
        }
        if matches!(r.then, OutcomeDecl::NeedsInput { .. }) {
            return Some((subject, "a needs_input outcome"));
        }
    }
    // An adjustment has a condition and no outcome of its own (spec 002,
    // 3.11), so only its condition can hold a construct of this spec.
    for a in &policy.adjustments {
        if cond_uses(&a.when) {
            return Some((
                format!("policy:adjustment:{}", a.id),
                "a provenance_within condition",
            ));
        }
    }
    None
}

/// True when the condition contains `provenance_within`.
pub(crate) fn cond_uses(c: &Cond) -> bool {
    match c {
        Cond::ProvenanceWithin { .. } => true,
        Cond::All(cs) | Cond::Any(cs) => cs.iter().any(cond_uses),
        Cond::Not(c) => cond_uses(c),
        _ => false,
    }
}
