use super::*;
use rustev_contract::redaction::{Action, FieldClass, truncate_str};
use serde_json::Value;

pub(super) fn withheld(policy: Option<&RedactionPolicy>, class: FieldClass) -> Option<Unavailable> {
    let p = policy?;
    let action = match p.class_action(class) {
        Action::Drop => WithholdingAction::Drop,
        Action::Digest => WithholdingAction::Digest,
        _ => return None,
    };
    Some(Unavailable::Redacted {
        policy: p.policy_id(),
        field_class: class,
        action,
    })
}
pub(super) fn item(item: &mut Item, policy: Option<&RedactionPolicy>, class: FieldClass) {
    if let Item::String(s) = item {
        if let Some(u) = withheld(policy, class) {
            *item = Item::Withheld(Withheld { not_available: u });
        } else if let Some(Action::Truncate { bytes }) = policy.map(|p| p.class_action(class)) {
            *s = truncate_str(s, bytes as usize).0;
        }
    }
}
// A composite copy is withheld as a whole if any of its members is withheld.
// This preserves the verbatim-copy invariant instead of inventing a new payload.
pub(super) fn apply(
    value: &mut Value,
    class: FieldClass,
    policy: Option<&RedactionPolicy>,
    composite: bool,
) -> Result<Option<Truncated>, Unavailable> {
    let Some(p) = policy else { return Ok(None) };
    if let Some(u) = withheld(Some(p), class) {
        return Err(u);
    }
    let mut cut = None;
    match value {
        Value::String(s) => {
            if let Action::Truncate { bytes } = p.class_action(class) {
                let (t, changed) = truncate_str(s, bytes as usize);
                *s = t;
                if changed {
                    cut = Some(Truncated {
                        policy: p.policy_id(),
                        bytes: bytes.into(),
                    });
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                cut = apply(v, class, Some(p), composite)?.or(cut);
            }
        }
        Value::Object(o) => {
            for (k, v) in o {
                let c = if composite {
                    member_class(k, class)
                } else {
                    class
                };
                cut = apply(v, c, Some(p), composite)?.or(cut);
            }
        }
        _ => {}
    }
    Ok(cut)
}
fn member_class(key: &str, parent: FieldClass) -> FieldClass {
    match key {
        "attempt_id" | "decision_id" | "candidate" | "instance" | "ids" | "eligible" | "id" => {
            FieldClass::Id
        }
        "detail" | "reason" if parent != FieldClass::PlanMetadata => FieldClass::FreeTextDetail,
        "step" | "field" | "fields" | "inputs" | "steps" | "subject" | "backend_id"
        | "artifact" | "action" => FieldClass::PlanMetadata,
        "digest" | "snapshot_id" => FieldClass::ContentDigest,
        "dispatched_ms" | "ended_ms" | "queued_ms" | "elapsed_ms" | "deadline_ms" | "cost"
        | "target" | "after" | "to" | "delay_ms" => FieldClass::Number,
        "derivation" | "kind" | "class" | "provenance" | "end" | "result" | "cancellation"
        | "remote" => FieldClass::Enum,
        _ => parent,
    }
}
pub(super) fn side(side: &mut Side, policy: Option<&RedactionPolicy>) {
    item(&mut side.decision_id, policy, FieldClass::Id);
    item(&mut side.plan_id, policy, FieldClass::PlanMetadata);
    if withheld(policy, FieldClass::PlanMetadata).is_some() {
        side.plan = None;
    }
    if let Some(u) = withheld(policy, FieldClass::ContentDigest) {
        side.record = DigestRef::Withheld(Withheld { not_available: u });
        side.bundle = None;
    }
    if withheld(policy, FieldClass::Enum).is_some() {
        side.capture = None;
    }
}
