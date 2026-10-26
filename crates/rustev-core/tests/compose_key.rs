//! Exact constructed keys and their value provenance (spec 036, 3.7).
mod common;
use rustev_contract::definition::Definition;
use rustev_contract::evidence::StepStatus;
use rustev_contract::judgment::Unresolved;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::{Compiled, Evaluation};
use serde_json::{Value, json};

fn definition() -> Definition {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.steps.clear();
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]})).unwrap();
    d
}
fn compile(d: &Definition) -> Compiled {
    rustev_core::compile(d, &[], &[]).unwrap()
}
fn start(c: &Compiled) -> Evaluation<'_> {
    let snapshot: Snapshot =
        serde_json::from_value(json!({"schema":"rustev.snapshot/1","entries":[]})).unwrap();
    Evaluation::start(c, &snapshot, Timestamp::from_ms(1000).unwrap()).unwrap()
}
fn exact(id: &str, op: &str, args: Value) -> rustev_contract::definition::StepDecl {
    serde_json::from_value(json!({"id":id,"body":{"exact":{"op":op,"version":1,"args":args}}}))
        .unwrap()
}

#[test]
fn compose_normalization_faults() {
    for (input, normalization, expected) in [
        ("  A__ B--C \t", "ascii_token", Some("a-b-c:7")),
        (" A_É ", "exact", Some(" A_É :7")),
        (" --A__ ", "ascii_token", Some("-a-:7")),
        ("É", "ascii_token", None),
        ("a/b", "ascii_token", None),
        ("a:b", "exact", None),
        (" \t", "ascii_token", None),
        ("", "exact", None),
    ] {
        let mut d = definition();
        d.steps.push(exact("key","compose_key",json!({"parts":[{"value":{"lit":{"text":input}},"normalize":normalization},{"value":{"lit":{"integer":7}},"normalize":"exact"}],"separator":":"})));
        let c = compile(&d);
        let e = start(&c);
        assert_eq!(
            e.exact_value("key").map(|v| v.to_json()),
            expected.map(|s| json!(s))
        );
        let (_, ev) = e.finish("compose").unwrap();
        if expected.is_some() {
            assert!(ev.steps[0].constructed);
            assert_eq!(
                ev.steps[0].meta.as_ref().unwrap().grade,
                rustev_contract::grade::ValueGrade::Hint
            );
        } else {
            assert!(!ev.steps[0].constructed);
            assert!(matches!(
                ev.steps[0].status,
                StepStatus::Unresolved(Unresolved::InvalidInput { .. })
            ));
        }
    }
}

#[test]
fn composed_key_bounds_types_and_exact_propagation() {
    for (size, valid) in [(510, true), (511, false)] {
        let mut d = definition();
        d.steps.push(exact("key","compose_key",json!({"parts":[{"value":{"lit":{"text":"a".repeat(size)}},"normalize":"exact"},{"value":{"lit":{"enum":"x"}},"normalize":"exact"}],"separator":"|"})));
        let c = compile(&d);
        let e = start(&c);
        assert_eq!(e.exact_value("key").is_some(), valid);
    }
    let mut d = definition();
    let parts = json!([{"value":{"lit":{"integer":-12}},"normalize":"exact"},{"value":{"lit":{"text":"X"}},"normalize":"ascii_token"}]);
    for separator in [":", "/", "|", "."] {
        d.steps = vec![
            exact(
                "key",
                "compose_key",
                json!({"parts":parts,"separator":separator}),
            ),
            exact(
                "length",
                "compare",
                json!({"predicate":{"is_constructed":{"ref":"step:key"}}}),
            ),
            exact(
                "probe",
                "compare",
                json!({"predicate":{"is_constructed":{"ref":"step:length"}}}),
            ),
        ];
        let c = compile(&d);
        let e = start(&c);
        assert_eq!(e.exact_value("probe").unwrap().to_json(), true);
        let (_, ev) = e.finish("propagation").unwrap();
        assert!(ev.steps.iter().all(|s| s.constructed));
    }
    for args in [
        json!({"parts":[],"separator":":"}),
        json!({"parts":parts,"separator":"-"}),
        json!({"parts":[{"value":{"lit":{"bool":true}},"normalize":"exact"},{"value":{"lit":{"text":"x"}},"normalize":"exact"}],"separator":":"}),
    ] {
        d.steps = vec![exact("key", "compose_key", args)];
        assert!(rustev_core::compile(&d, &[], &[]).is_err());
    }
}

#[test]
fn failed_corroboration_clears_constructed_provenance() {
    for (expected_key, succeeds) in [("other:key", false), ("a:b", true)] {
        let mut d = definition();
        d.steps = vec![
            exact(
                "key",
                "compose_key",
                json!({"parts":[
                    {"value":{"lit":{"text":"a"}},"normalize":"exact"},
                    {"value":{"lit":{"text":"b"}},"normalize":"exact"}
                ],"separator":":"}),
            ),
            serde_json::from_value(json!({"id":"checked","body":{"corroborate":{
                "claim":"step:key","upgrade_to":"authoritative",
                "by":[{"predicate":{"cmp":{
                    "left":{"ref":"step:key"},"op":"eq",
                    "right":{"lit":{"text":expected_key}}
                }},"on_false":"contradicts"}],
                "mode":"all","on_contradiction":"abstain"
            }}}))
            .unwrap(),
            exact(
                "downstream",
                "compare",
                json!({"predicate":{"is_constructed":{"ref":"step:checked"}}}),
            ),
        ];
        let c = compile(&d);
        let e = start(&c);
        assert_eq!(e.exact_value("key").unwrap().to_json(), json!("a:b"));
        assert_eq!(e.exact_value("checked").is_some(), succeeds);
        assert_eq!(e.exact_value("downstream").is_some(), succeeds);
        let steps = ["key", "checked", "downstream"]
            .into_iter()
            .filter_map(|id| e.exact_value(id).cloned().map(|v| (id.to_string(), v)))
            .collect();
        let (_, evidence) = e.finish("corroboration").unwrap();
        // Probe the predicate using the evaluation's values and recorded provenance.
        // Exact compare steps propagate unresolved dependencies before evaluating it.
        let constructed = evidence
            .steps
            .iter()
            .filter(|s| s.constructed)
            .map(|s| format!("step:{}", s.step))
            .collect();
        let scope = rustev_core::expr::Scope {
            constructed: Some(&constructed),
            now: Timestamp::from_ms(1000).unwrap(),
            inputs: &Default::default(),
            steps: &steps,
            present: &Default::default(),
            item: None,
            v2: true,
        };
        for id in ["checked", "downstream"] {
            let predicate =
                serde_json::from_value(json!({"is_constructed":{"ref":format!("step:{id}")}}))
                    .unwrap();
            assert_eq!(
                rustev_core::expr::eval_predicate(&predicate, &scope).unwrap(),
                succeeds
            );
        }
        let checked = evidence.steps.iter().find(|s| s.step == "checked").unwrap();
        assert_eq!(checked.constructed, succeeds);
        if !succeeds {
            assert_eq!(
                checked.status,
                StepStatus::Unresolved(Unresolved::Abstained {
                    rule: "checked".into()
                })
            );
        }
        let downstream = evidence
            .steps
            .iter()
            .find(|s| s.step == "downstream")
            .unwrap();
        assert_eq!(downstream.constructed, succeeds);
    }
}

#[test]
fn constructed_policy_distinguishes_keys_from_ordinary_exact_values() {
    for (op, args, expected) in [
        (
            "compose_key",
            json!({"parts":[{"value":{"lit":{"text":"a"}},"normalize":"exact"},{"value":{"lit":{"text":"b"}},"normalize":"exact"}],"separator":":"}),
            "constructed",
        ),
        ("arith", json!({"expr":{"lit":{"integer":7}}}), "ordinary"),
    ] {
        let mut d = definition();
        d.steps = vec![exact("value", op, args)];
        d.policy = serde_json::from_value(json!({"on_unresolved":[{"target":"step:value","reasons":"any","action":"propagate"}],"rules":[
            {"id":"constructed","when":{"exact":{"is_constructed":{"ref":"step:value"}}},"then":{"escalate":{"reason":"constructed"}}},
            {"id":"fallback","when":"always","then":{"escalate":{"reason":"ordinary"}}}
        ],"adjustments":[]})).unwrap();
        let c = compile(&d);
        let (judgment, evidence) = start(&c).finish("policy").unwrap();
        assert_eq!(
            serde_json::to_value(judgment).unwrap()["outcome"],
            json!({"escalate":{"reason":expected}})
        );
        assert_eq!(evidence.steps[0].constructed, expected == "constructed");
    }
}

#[test]
fn compose_key_bound_counts_utf8_bytes() {
    for (count, valid) in [(255, true), (256, false)] {
        let mut d = definition();
        d.steps = vec![exact(
            "key",
            "compose_key",
            json!({"parts":[
            {"value":{"lit":{"text":"é".repeat(count)}},"normalize":"exact"},
            {"value":{"lit":{"text":"x"}},"normalize":"exact"}
        ],"separator":"/"}),
        )];
        let c = compile(&d);
        assert_eq!(start(&c).exact_value("key").is_some(), valid);
    }
}
