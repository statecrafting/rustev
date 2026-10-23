//! Parse refusals for spec 034 inputs when this build excludes that feature.

use crate::{Category, Refusal};
use rustev_contract::{
    bounded::parse_bounded,
    limits::{DEFINITION_V1, PLAN_V1},
    plan::Plan,
};
use serde_json::Value;

fn refusal(schema: &str, member: &str) -> Refusal {
    Refusal {
        category: Category::Parse,
        subject: schema.into(),
        detail: format!("{schema} member {member}: this build has no `parameterized` feature"),
        shortfalls: vec![],
    }
}

fn reference(v: &Value) -> bool {
    match v {
        Value::Object(o) => {
            (o.len() == 1 && o.get("param").is_some_and(Value::is_string))
                || o.values().any(reference)
        }
        Value::Array(a) => a.iter().any(reference),
        _ => false,
    }
}

/// Recognize only spec 034 declarations and item members, not unrelated JSON keys.
/// Malformed JSON retains the ordinary document parser's diagnostic.
pub fn definition(bytes: &[u8]) -> Result<(), Refusal> {
    let Ok(v) = parse_bounded::<Value>(bytes, &DEFINITION_V1) else {
        return Ok(());
    };
    let schema = v["schema"].as_str().unwrap_or("definition");
    for member in ["parameters", "variants"] {
        if v.get(member).is_some() {
            return Err(refusal(schema, member));
        }
    }
    for path in [
        "/inputs",
        "/steps",
        "/policy/rules",
        "/policy/adjustments",
        "/policy/on_unresolved",
    ] {
        if let Some(items) = v.pointer(path).and_then(Value::as_array) {
            for item in items {
                for member in ["only_in", "enabled"] {
                    if member == "enabled" && path == "/inputs" {
                        continue;
                    }
                    if item.get(member).is_some() {
                        return Err(refusal(schema, member));
                    }
                }
            }
        }
    }
    if reference(&v) {
        return Err(refusal(schema, "param"));
    }
    Ok(())
}

/// Refuse the record before decoding it, including malformed records.
pub fn plan_bytes(bytes: &[u8]) -> Result<(), Refusal> {
    if let Ok(v) = parse_bounded::<Value>(bytes, &PLAN_V1)
        && v.get("instantiation").is_some()
    {
        return Err(refusal(
            v["schema"].as_str().unwrap_or("plan"),
            "instantiation",
        ));
    }
    Ok(())
}

/// Typed callers cannot load an instantiated plan through a feature-off core.
pub fn plan(plan: &Plan) -> Result<(), Refusal> {
    if plan.instantiation.is_some() {
        return Err(refusal(&plan.schema, "instantiation"));
    }
    Ok(())
}

/// Typed definitions can still contain parameter references in raw exact args.
#[cfg(not(feature = "parameterized"))]
pub(crate) fn typed_definition(
    def: &rustev_contract::definition::Definition,
) -> Result<(), Refusal> {
    for step in &def.steps {
        if let rustev_contract::definition::StepBody::Exact(exact) = &step.body
            && reference(&exact.args)
        {
            return Err(refusal(&def.schema, "param"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn refuses_each_reserved_member_without_reclassifying_unrelated_keys() {
        for (member, value) in [
            ("parameters", json!({"parameters": []})),
            ("variants", json!({"variants": []})),
            ("only_in", json!({"inputs": [{"only_in": ["a"]}]})),
            ("enabled", json!({"steps": [{"enabled": true}]})),
            (
                "param",
                json!({"limits": {"deadline_ms": {"param": "deadline"}}}),
            ),
        ] {
            let error = definition(&serde_json::to_vec(&value).unwrap()).unwrap_err();
            assert_eq!(error.category, Category::Parse);
            assert!(error.detail.contains(member));
            assert!(error.detail.contains("no `parameterized` feature"));
        }
        assert!(definition(br#"{"schema":"rustev.definition/2","steps":[{"body":{"exact":{"args":{"enabled":true,"variants":[]}}}}]}"#).is_ok());
        assert!(definition(b"invalid JSON").is_ok());
    }

    #[test]
    fn plan_three_without_record_remains_available() {
        assert!(plan_bytes(br#"{"schema":"rustev.plan/3"}"#).is_ok());
        let error = plan_bytes(br#"{"schema":"rustev.plan/3","instantiation":{}}"#).unwrap_err();
        assert_eq!(error.category, Category::Parse);
        assert!(error.detail.contains("rustev.plan/3 member instantiation"));
    }
}
