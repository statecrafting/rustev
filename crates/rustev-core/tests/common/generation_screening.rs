//! Shared synthetic screening declarations for generation tests.
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, ScreenProfile};
use serde_json::json;

pub fn configure(
    d: &mut Definition,
    b: &BackendDescriptor,
    unavailable: &str,
) -> BackendDescriptor {
    d.inputs.push(
        serde_json::from_value(json!({
            "name":"message", "type":{"text":{"max_bytes":32}},
            "provenance":["third-party"], "freshness":"not_required"
        }))
        .unwrap(),
    );
    d.policy.on_unresolved.push(
        serde_json::from_value(json!({
            "target":"input:message", "reasons":"any", "action":{"escalate":{"reason":"missing"}}
        }))
        .unwrap(),
    );
    let StepBody::Generate(g) = &mut d.steps[0].body else {
        panic!()
    };
    g.projection = vec!["input:message".into()];
    d.screening = Some(
        serde_json::from_value(json!({
            "max_text_units":2, "entries":[{
                "id":"message", "fields":["input:message"], "item_id":"none",
                "screener":"screen", "on_block":"drop_field", "address":"computed",
                "on_unavailable":unavailable
            }]
        }))
        .unwrap(),
    );
    d.limits.max_semantic_requests = 3;
    let mut screen = b.clone();
    screen.backend_id = "screen".into();
    screen.operations = vec![super::common::support(
        Operation::Proposition,
        OutputKind::Screen,
        2,
    )];
    screen.operations[0].screen = Some(ScreenProfile {
        policy_version: "synthetic/1".into(),
        categories: vec!["injection".into()],
        max_text_bytes: 32,
        text_unit_bytes: 16,
    });
    screen
}
