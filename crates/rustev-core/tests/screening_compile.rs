//! SYNTHETIC compile-only fixtures for spec 035 increment 2.
mod common;
use common::*;
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, InputExcess, OutputKind, ScreenProfile};
use rustev_contract::screening::*;
use rustev_contract::{Document, Identified, schema};
use rustev_core::{Category, Compiled, Evaluation, Refusal, compile};
use serde_json::json;

fn definition() -> Definition {
    Definition::parse(&serde_json::to_vec(&json!({
        "schema": "rustev.definition/2", "name": "screened", "package": "synthetic", "version": "1.0.0",
        "inputs": [
            {"name":"request", "type":{"record":{"fields":[
                {"name":"text","type":{"text":{"max_bytes":32}}},
                {"name":"behaviour_text","type":{"text":{"max_bytes":16}}}
            ]}}, "provenance":["user-supplied"],"freshness":"not_required"},
            {"name":"candidates", "type":{"list":{"item":{"record":{"fields":[
                {"name":"id","type":{"enum":{"options":["a","b"]}}},
                {"name":"description","type":{"text":{"max_bytes":48}}}
            ]}},"max_items":2}}, "provenance":["third-party"],"freshness":"not_required"}
        ],
        "steps": [],
        "policy":{"on_unresolved":[],"rules":[{"id":"ok","when":"always","then":{"propose":{"action":"ok","params":[]}}}],"adjustments":[]},
        "limits":{"max_semantic_requests":20,"on_excess":"refuse","max_projection_bytes":65536,"distribution_tolerance":"0.01","deadline_ms":1000},
        "screening":{"max_text_units":9,"entries":[
            {"id":"caller_text","fields":["input:request/text"],"item_id":"none","screener":"screen","on_block":"refuse","address":"computed","on_unavailable":"fail_closed"},
            {"id":"behaviour","fields":["input:request/behaviour_text"],"item_id":"none","screener":"screen","on_block":"drop_field","address":"computed","on_unavailable":"fail_closed"},
            {"id":"catalogue","fields":["input:candidates[]/description"],"item_id":{"field":"id"},"screener":"screen","on_block":"drop_text","address":"computed","on_unavailable":"skip_generation"}
        ]}
    })).unwrap()).unwrap()
}
fn backends() -> Vec<BackendDescriptor> {
    let mut screen = label_only();
    screen.backend_id = "screen".into();
    screen.operations = vec![support(Operation::Proposition, OutputKind::Screen, 2)];
    screen.operations[0].screen = Some(ScreenProfile {
        policy_version: "synthetic/1".into(),
        categories: vec!["injection".into()],
        max_text_bytes: 48,
        text_unit_bytes: 16,
    });
    vec![screen, label_only()]
}
fn model(def: &mut Definition, project: &[&str]) {
    def.steps.push(StepDecl {
        id: "model".into(),
        body: StepBody::Semantic(semantic(
            Operation::Classify,
            "synthetic/1",
            "choose",
            &["a", "b"],
            RequiredKind::Label,
            project,
        )),
    });
}
fn run(def: &Definition) -> Result<Compiled, Refusal> {
    compile(def, &backends(), &[])
}
fn entry_mut(def: &mut Definition, index: usize) -> &mut ScreeningEntry {
    &mut def.screening.as_mut().unwrap().entries[index]
}
fn refuse(def: &Definition, category: Category, subject: &str) -> Refusal {
    let r = run(def).unwrap_err();
    assert_eq!(r.category, category, "{r}");
    assert_eq!(r.subject, subject, "{r}");
    r
}

#[test]
fn three_entry_lowering_plan_bindings_and_projection_bytes() {
    let def = definition();
    let c = run(&def).unwrap();
    assert_eq!(c.plan.schema, schema::PLAN_V3);
    assert_eq!(c.plan.definition, def);
    assert_eq!(c.plan.definition_id, def.id().unwrap());
    assert_eq!(
        c.plan.order,
        [
            "screen:caller_text:0",
            "screen:behaviour:0",
            "screen:catalogue:0:items",
            "screen:catalogue:0"
        ]
    );
    golden("screening.definition.json", &def.canonical().unwrap());
    golden("screening.plan.json", &c.canonical());
    assert_eq!(Compiled::load(&c.plan, &backends(), &[]).unwrap().id, c.id);
    let snapshot = snapshot(vec![
        entry(
            "request",
            json!({"text":"same","behaviour_text":"same"}),
            ProvenanceClass::UserSupplied,
            NOW,
        ),
        entry(
            "candidates",
            json!([{"id":"a","description":"same"},{"id":"b","description":"same"}]),
            ProvenanceClass::ThirdParty,
            NOW,
        ),
    ]);
    let eval = Evaluation::start(&c, &snapshot, ts(NOW)).unwrap();
    let pending = eval.pending();
    assert_eq!(pending.len(), 4);
    for r in &pending {
        golden("screening.projection.json", &r.projection);
        assert_eq!(r.projection, pending[0].projection);
    }
}

#[test]
fn screened_models_and_drop_item_exact_readers_follow_screens() {
    let mut def = definition();
    model(&mut def, &["input:request", "input:candidates"]);
    let c = run(&def).unwrap();
    assert_eq!(c.plan.order.last().unwrap(), "model");
    entry_mut(&mut def, 2).on_block = OnBlock::DropItem;
    def.steps.push(StepDecl {
        id: "count".into(),
        body: StepBody::Exact(ExactDecl {
            op: "count_where".into(),
            version: 1,
            args: json!({"list":"input:candidates","predicate":{"all":[]}}),
        }),
    });
    let c = run(&def).unwrap();
    assert!(
        c.plan.order.iter().position(|s| s == "count").unwrap()
            > c.plan
                .order
                .iter()
                .position(|s| s == "screen:catalogue:0")
                .unwrap()
    );
    assert_eq!(
        c.plan
            .steps
            .iter()
            .find(|s| s.id == "count")
            .unwrap()
            .derivation,
        rustev_contract::judgment::Derivation::MixedDerived
    );
}

#[test]
fn category_18_and_19_goldens() {
    let mut bad = definition();
    entry_mut(&mut bad, 0).on_unavailable = OnUnavailable::FailOpen;
    let r = refuse(&bad, Category::InvalidScreening, "screening:caller_text");
    golden("screening.refusal-18.txt", r.to_string().as_bytes());
    let mut bad = definition();
    bad.screening.as_mut().unwrap().entries.remove(1);
    model(&mut bad, &["input:request"]);
    let r = refuse(&bad, Category::UnscreenedText, "step:model");
    assert!(r.detail.contains("input:request/behaviour_text"));
    golden("screening.refusal-19.txt", r.to_string().as_bytes());
}

#[test]
fn invalid_entries_and_field_shapes() {
    type Change = fn(&mut Definition);
    let cases: Vec<(&str, Change)> = vec![
        ("caller_text", |d| {
            entry_mut(d, 0).on_block = OnBlock::DropText
        }),
        ("catalogue", |d| {
            entry_mut(d, 2).on_block = OnBlock::DropField
        }),
        ("behaviour", |d| {
            entry_mut(d, 1).fields = vec!["input:request/text".into()]
        }),
        ("caller_text", |d| {
            entry_mut(d, 0).fields.push("input:request/text".into())
        }),
        ("caller_text", |d| entry_mut(d, 1).id = "caller_text".into()),
        ("bad:id", |d| entry_mut(d, 0).id = "bad:id".into()),
        ("caller_text", |d| {
            entry_mut(d, 0).fields = vec!["input:unknown".into()]
        }),
        ("catalogue", |d| {
            entry_mut(d, 2).fields = vec!["input:candidates[]/id".into()]
        }),
        ("catalogue", |d| {
            entry_mut(d, 2).item_id = ScreeningItemId::None
        }),
        ("caller_text", |d| {
            entry_mut(d, 0).item_id = ScreeningItemId::Field(ScreeningItemField {
                field: "text".into(),
            })
        }),
        ("behaviour", |d| {
            entry_mut(d, 1).address =
                serde_json::from_value(json!({"to":"host","reason":"test"})).unwrap()
        }),
    ];
    for (entry, change) in cases {
        let mut d = definition();
        change(&mut d);
        refuse(
            &d,
            Category::InvalidScreening,
            &format!("screening:{entry}"),
        );
    }
}

#[test]
fn screening_block_bounds_and_list_identity_are_checked() {
    for change in 0..3 {
        let mut d = definition();
        let b = d.screening.as_mut().unwrap();
        match change {
            0 => b.entries.clear(),
            1 => b.entries = vec![b.entries[0].clone(); 33],
            _ => b.max_text_units = 0,
        }
        refuse(&d, Category::InvalidScreening, "screening:entries");
    }
    for change in 0..3 {
        let mut d = definition();
        match change {
            0 => entry_mut(&mut d, 0).fields.clear(),
            1 => entry_mut(&mut d, 0).fields = vec!["input:request/text".into(); 33],
            _ => entry_mut(&mut d, 0).screener.clear(),
        }
        refuse(&d, Category::InvalidScreening, "screening:caller_text");
    }
    let mut d = definition();
    let mut other = d.inputs[1].clone();
    other.name = "other".into();
    d.inputs.push(other);
    entry_mut(&mut d, 2)
        .fields
        .push("input:other[]/description".into());
    refuse(&d, Category::InvalidScreening, "screening:catalogue");
    for key in ["unknown", "description"] {
        let mut d = definition();
        if key == "description" {
            let TypeDecl::List { item, .. } = &mut d.inputs[1].ty else {
                panic!()
            };
            let TypeDecl::Record { fields, .. } = item.as_mut() else {
                panic!()
            };
            fields[1].ty = TypeDecl::Integer;
        }
        entry_mut(&mut d, 2).item_id =
            ScreeningItemId::Field(ScreeningItemField { field: key.into() });
        refuse(&d, Category::InvalidScreening, "screening:catalogue");
    }
}

#[test]
fn caller_override_uses_category_17() {
    let mut d = definition();
    entry_mut(&mut d, 0).address =
        serde_json::from_value(json!({"to":"caller","reason":"test"})).unwrap();
    refuse(&d, Category::CallerCannotAct, "screening:caller_text");
}

#[test]
fn v1_and_authored_verdict_are_category_11() {
    let mut d = definition();
    d.schema = schema::DEFINITION.into();
    refuse(&d, Category::InvalidDefinition, "screening");
    let mut d = definition();
    model(&mut d, &[]);
    let StepBody::Semantic(s) = &mut d.steps[0].body else {
        panic!()
    };
    s.requires = RequiredKind::ScreenVerdict;
    refuse(&d, Category::InvalidDefinition, "step:model");
}

#[test]
fn screener_capability_shortfalls_name_lowered_step() {
    for change in 0..5 {
        let mut b = backends();
        match change {
            0 => b[0].operations[0].screen = None,
            1 => b[0].operations[0].screen.as_mut().unwrap().max_text_bytes = 1,
            2 => b[0].input_limit.max_bytes = 1,
            3 => b[0].input_limit.on_excess = InputExcess::TruncateStart,
            _ => {
                b.remove(0);
            }
        }
        let r = compile(&definition(), &b, &[]).unwrap_err();
        assert_eq!(r.category, Category::NoCapableBackend);
        assert_eq!(r.subject, "step:screen:caller_text:0");
        assert!(!r.shortfalls[0].reasons.is_empty());
    }
}

#[test]
fn text_unit_and_semantic_request_budgets() {
    let mut d = definition();
    assert!(run(&d).is_ok());
    d.screening.as_mut().unwrap().max_text_units = 8;
    refuse(&d, Category::Budget, "screening");
    d.screening.as_mut().unwrap().max_text_units = 9;
    d.limits.max_semantic_requests = 3;
    refuse(&d, Category::Budget, "limits");
}

#[test]
fn generated_steps_cannot_be_referenced() {
    let mut d = definition();
    model(&mut d, &["step:screen:caller_text:0"]);
    refuse(&d, Category::InvalidDefinition, "step:model");
    let mut d = definition();
    d.policy.on_unresolved.push(Handler {
        target: "step:screen:caller_text:0".into(),
        reasons: ReasonSet::Any,
        action: HandlerAction::Propagate,
    });
    refuse(&d, Category::InvalidDefinition, "policy:on_unresolved[0]");
}

#[test]
fn unscreened_third_party_and_fanout_id() {
    let mut d = definition();
    d.screening.as_mut().unwrap().entries.pop();
    model(&mut d, &["input:candidates"]);
    let r = refuse(&d, Category::UnscreenedText, "step:model");
    assert!(r.detail.contains("input:candidates[]/description"));
    let mut d = definition();
    let TypeDecl::List { item, .. } = &mut d.inputs[1].ty else {
        panic!()
    };
    let TypeDecl::Record { fields, .. } = item.as_mut() else {
        panic!()
    };
    fields[0].ty = TypeDecl::Text { max_bytes: 1 };
    d.steps.push(StepDecl {
        id: "ids".into(),
        body: StepBody::Exact(ExactDecl {
            op: "filter_with_reasons".into(),
            version: 1,
            args: json!({"list":"input:candidates","id_field":"id","rules":[]}),
        }),
    });
    model(&mut d, &[]);
    let StepBody::Semantic(s) = &mut d.steps[1].body else {
        panic!()
    };
    s.for_each = vec![ForEach {
        over: "step:ids".into(),
        bind: "item".into(),
        items: ItemSource::None,
    }];
    let r = refuse(&d, Category::UnscreenedText, "step:model");
    assert!(r.detail.contains("input:candidates[]/id"));
    entry_mut(&mut d, 2)
        .fields
        .push("input:candidates[]/id".into());
    refuse(&d, Category::InvalidScreening, "screening:catalogue");
    entry_mut(&mut d, 2).on_block = OnBlock::DropItem;
    d.screening.as_mut().unwrap().max_text_units = 11;
    assert!(run(&d).is_ok());
}

#[test]
fn existing_reference_plan_ids_and_all_bytes_are_unchanged() {
    let a = compile(
        &support_routing(),
        &descriptors(),
        &[topic_calibration("1.5")],
    )
    .unwrap();
    let b = compile(&lodging(), &descriptors(), &[]).unwrap();
    for (name, c) in [("support-routing", a), ("lodging", b)] {
        assert_eq!(
            c.id.to_string(),
            std::fs::read_to_string(golden_dir().join(format!("{name}.plan-id")))
                .unwrap()
                .trim()
        );
        assert_eq!(
            c.canonical(),
            std::fs::read(golden_dir().join(format!("{name}.plan.json"))).unwrap()
        );
    }
}

#[test]
fn definitions_without_screening_keep_their_identity_and_accept_text() {
    let mut d = definition();
    d.screening = None;
    d.schema = schema::DEFINITION.into();
    model(&mut d, &["input:candidates"]);
    let c = run(&d).unwrap();
    assert!(c.plan.screening.is_none());
    golden("screening.absent.plan-id", c.id.to_string().as_bytes());
    assert_eq!(Compiled::load(&c.plan, &backends(), &[]).unwrap().id, c.id);
}

#[test]
fn exact_extremum_does_not_copy_other_text_fields() {
    let mut d = definition();
    d.steps.push(StepDecl {
        id: "copied".into(),
        body: StepBody::Exact(ExactDecl {
            op: "extremum".into(),
            version: 1,
            args: json!({"list":"input:candidates","field":"description","which":"min"}),
        }),
    });
    // Extremum currently orders numeric values, while its copy table also
    // pins the selected-field rule for any future orderable text kind.
    let TypeDecl::List { item, .. } = &mut d.inputs[1].ty else {
        panic!()
    };
    let TypeDecl::Record { fields, .. } = item.as_mut() else {
        panic!()
    };
    fields[1].ty = TypeDecl::Integer;
    d.screening.as_mut().unwrap().entries.pop();
    model(&mut d, &["step:copied"]);
    run(&d).unwrap();
}

#[test]
fn fail_open_refuses_fixed_and_declared_caller_classes() {
    for p in [
        ProvenanceClass::UserSupplied,
        ProvenanceClass::AuthenticatedAppField,
    ] {
        let mut d = definition();
        d.inputs[0].provenance = vec![p];
        entry_mut(&mut d, 0).on_unavailable = OnUnavailable::FailOpen;
        refuse(&d, Category::InvalidScreening, "screening:caller_text");
    }
    let mut d = definition();
    entry_mut(&mut d, 2).on_unavailable = OnUnavailable::FailOpen;
    assert!(run(&d).is_ok());
    d.inputs[1].provenance = vec![ProvenanceClass::AttributedClaim];
    d.actionability=Some(serde_json::from_value(json!({"caller_controlled":["attributed-claim"],"remediations":[],"questions":[],"constraint_groups":[],"checks":[]})).unwrap());
    refuse(&d, Category::InvalidScreening, "screening:catalogue");
}

#[test]
fn text_units_round_up_and_list_multiplicity_counts() {
    let mut d = definition();
    let TypeDecl::Record { fields, .. } = &mut d.inputs[0].ty else {
        panic!()
    };
    fields[0].ty = TypeDecl::Text { max_bytes: 33 };
    refuse(&d, Category::Budget, "screening");
    d.screening.as_mut().unwrap().max_text_units = 10;
    assert!(run(&d).is_ok());
}
