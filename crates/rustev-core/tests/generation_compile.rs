//! Spec 037 compile-time cases; no generation execution or provider calls.
mod common;
use rustev_contract::Identified;
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, TextStructure};
use rustev_contract::generation::*;
use rustev_contract::plan::PlanStepDetail;
use rustev_core::{Category as C, Compiled, Refusal, compile_with_templates};
use serde_json::{Value, json};

fn fixture() -> (Definition, PromptTemplate, BackendDescriptor) {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.policy=serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let p:PromptTemplate=serde_json::from_value(json!({"schema":"rustev.prompt-template/1","id":"item-text","version":"1.0.0","instructions":"Describe the item.","params":[]})).unwrap();
    let mut g: Value = serde_json::from_str(include_str!(
        "../../rustev-contract/tests/fixtures/generate-step.json"
    ))
    .unwrap();
    let body = &mut g["body"]["generate"];
    body["for_each"] = json!("none");
    body["template"]["params"] = json!({});
    body["template"]["digest"] = json!(p.id().unwrap());
    body["projection"] = json!([]);
    body["carry"] = json!([]);
    body["backstop"]["fields"] =
        json!({"headline":{"text":"An option"},"why":{"text":"A useful choice."}});
    d.steps = vec![serde_json::from_value(g).unwrap()];
    let mut b = common::linear_head();
    b.operations = vec![common::support(
        Operation::Generate,
        OutputKind::TextRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 4096,
            languages: vec!["en-GB".into()],
            streaming: false,
        },
        32,
    )];
    (d, p, b)
}
fn run(d: &Definition, p: &PromptTemplate, b: &BackendDescriptor) -> Result<Compiled, Refusal> {
    compile_with_templates(
        d,
        std::slice::from_ref(b),
        &[],
        &[],
        None,
        std::slice::from_ref(p),
    )
}
fn edit(d: &mut Definition, f: impl FnOnce(&mut Value)) {
    let mut v = serde_json::to_value(&d.steps[0]).unwrap();
    f(&mut v["body"]["generate"]);
    d.steps[0] = serde_json::from_value(v).unwrap();
}
fn input(d: &mut Definition, name: &str, ty: Value, model: bool) {
    d.inputs.push(serde_json::from_value(json!({"name":name,"type":ty,"provenance":[if model{"model-derived"}else{"system-of-record"}],"freshness":"not_required"})).unwrap());
    // Inputs are allowed to be unresolved independently of generation.
    d.policy.on_unresolved.push(serde_json::from_value(json!({"target":format!("input:{name}"),"reasons":"any","action":{"escalate":{"reason":"missing"}}})).unwrap());
}
#[test]
fn generation_plan_identity_reload_and_compatibility() {
    let (d, p, b) = fixture();
    let c = run(&d, &p, &b).unwrap();
    assert_eq!(c.plan.schema, "rustev.plan/3");
    assert_eq!(
        c.plan.steps[0].derivation,
        rustev_contract::judgment::Derivation::ModelDerived
    );
    let PlanStepDetail::Generate(g) = &c.plan.steps[0].detail else {
        panic!()
    };
    assert_eq!(g.max_items, 1);
    assert_eq!(g.worst_case_requests, 2);
    assert_eq!(g.template, p);
    assert_eq!(
        Compiled::load(&c.plan, std::slice::from_ref(&b), &[])
            .unwrap()
            .id,
        c.id
    );
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/generation.plan.json");
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, c.canonical()).unwrap();
    }
    assert_eq!(std::fs::read(path).unwrap(), c.canonical());
    for change in 0..4 {
        let mut d = d.clone();
        let mut p = p.clone();
        let mut b = b.clone();
        match change {
            0 => {
                p.instructions.push(' ');
                edit(&mut d, |g| g["template"]["digest"] = json!(p.id().unwrap()));
            }
            1 => edit(&mut d, |g| g["backstop"]["locale"] = json!("en-US")),
            2 => edit(&mut d, |g| {
                g["vocabularies"]["tag_vocabulary"][0] = json!("calm")
            }),
            _ => b.artifact = common::artifact('b'),
        }
        assert_ne!(run(&d, &p, &b).unwrap().id, c.id);
    }
    let mut v1 = d.clone();
    v1.schema = "rustev.definition/1".into();
    let e = run(&v1, &p, &b).unwrap_err();
    assert_eq!(e.category, C::InvalidDefinition);
    assert_eq!(e.detail, "requires-definition-2");
}
#[test]
fn structure_and_template_negative_rows() {
    type Mutation = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(C, Mutation)> = vec![
        // An unknown step outside the template is a generation error (3.5).
        (
            C::InvalidGeneration,
            Box::new(|g| g["projection"] = json!(["step:missing"])),
        ),
        (
            C::InvalidGeneration,
            Box::new(|g| g["carry"] = json!(["step:missing/name"])),
        ),
        (
            C::InvalidGeneration,
            Box::new(|g| {
                g["backstop"]["fields"]
                    .as_object_mut()
                    .unwrap()
                    .remove("headline");
            }),
        ),
        (
            C::InvalidGeneration,
            Box::new(|g| g["backstop"]["fields"]["undeclared"] = json!({"text":"x"})),
        ),
        (
            C::InvalidGeneration,
            Box::new(|g| g["backstop"]["fields"]["headline"] = json!({"list":["quiet"]})),
        ),
        (
            C::InvalidGeneration,
            Box::new(|g| g["output"]["fields"][2]["type"]["list"]["vocabulary"] = json!("unknown")),
        ),
        (
            C::InvalidGeneration,
            Box::new(|g| g["when"] = json!({"actions":["missing"]})),
        ),
        (
            C::Template,
            Box::new(|g| g["backstop"]["fields"]["headline"] = json!({"text":"{{input.unknown}}"})),
        ),
        (
            C::Template,
            Box::new(|g| {
                g["output"]["fields"][2]["required"] = json!(true);
                g["backstop"]["fields"]["tags"] = json!({"list":[]});
            }),
        ),
        (
            C::Template,
            Box::new(|g| g["backstop"]["fields"]["headline"] = json!({"text":"a".repeat(121)})),
        ),
        (
            C::Template,
            Box::new(|g| g["template"]["digest"] = json!(common::id('0'))),
        ),
    ];
    for (expected, f) in cases {
        let (mut d, p, b) = fixture();
        edit(&mut d, f);
        let e = run(&d, &p, &b).unwrap_err();
        assert_eq!(e.category, expected, "{e}");
    }
    for text in [
        "{{> partial}}",
        "{{=<% %>=}}",
        "{{#if x}}",
        "{{#input.x}}",
        "{{_name}}",
    ] {
        let (mut d, p, b) = fixture();
        edit(&mut d, |g| {
            g["backstop"]["fields"]["headline"] = json!({"text":text})
        });
        assert_eq!(run(&d, &p, &b).unwrap_err().category, C::Template);
    }
    let (mut d, p, b) = fixture();
    edit(&mut d, |g| {
        g["backstop"]["fields"]["headline"] = json!({"text":"a".repeat(121)});
        g["backstop"]["overflow"] = json!("truncate");
    });
    assert!(run(&d, &p, &b).is_ok());
}
#[test]
fn exact_backstops_parameters_and_ownership() {
    for (source, model, category) in [
        ("{{input.title}}", false, C::Template),
        ("{{{input.title}}}", false, C::Template),
        ("Title: {{input.title}}", true, C::Template),
    ] {
        let (mut d, p, b) = fixture();
        input(&mut d, "title", json!({"text":{"max_bytes":20}}), model);
        edit(&mut d, |g| {
            g["backstop"]["fields"]["headline"] = json!({"text":source})
        });
        assert_eq!(run(&d, &p, &b).unwrap_err().category, category);
    }
    let (mut d, p, b) = fixture();
    input(&mut d, "price", json!("integer"), false);
    edit(&mut d, |g| {
        g["output"]["fields"][0]["name"] = json!("price");
        g["carry"] = json!(["input:price"]);
    });
    assert_eq!(run(&d, &p, &b).unwrap_err().category, C::InvalidGeneration);
    let (mut d, mut p, b) = fixture();
    input(&mut d, "title", json!({"text":{"max_bytes":20}}), false);
    p.params = vec![PromptParam {
        name: "style".into(),
        ty: PromptParamType::Text { max_bytes: 30 },
    }];
    p.instructions = "{{param.style}}".into();
    edit(&mut d, |g| {
        g["template"]["digest"] = json!(p.id().unwrap());
        g["template"]["params"] = json!({"style":"input:title"});
    });
    assert_eq!(run(&d, &p, &b).unwrap_err().category, C::Template);
    edit(&mut d, |g| {
        g["template"]["params"] = json!({"style":{"literal":{"text":"brief"}}})
    });
    assert!(run(&d, &p, &b).is_ok());
}
#[test]
fn generated_outputs_never_flow_into_decisions_or_generation() {
    for location in 0..6 {
        let (mut d, p, b) = fixture();
        match location {
            0=>edit(&mut d,|g|g["projection"]=json!(["step:item_text"])),
            1=>edit(&mut d,|g|g["template"]["params"]=json!({"style":"step:item_text"})),
            2=>edit(&mut d,|g|g["for_each"]=json!({"over":"step:item_text","bind":"item","items":"none","max_items":{"literal":1}})),
            3=>d.policy.rules[0].when=Cond::Empty("step:item_text".into()),
            4=>d.policy.rules[0].then=serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"selected","value":{"ref":"step:item_text"}}]}})).unwrap(),
            _=>edit(&mut d,|g|g["backstop"]["fields"]["headline"]=json!({"text":"{{step.item_text}}"})),
        }
        assert_eq!(
            run(&d, &p, &b).unwrap_err().category,
            C::ModelOutputFlow,
            "location {location}"
        );
    }
}
#[test]
fn capability_and_request_budgets() {
    let (d, p, b) = fixture();
    for change in 0..6 {
        let mut b = b.clone();
        match change {
            0 => b.operations.clear(),
            1 => b.operations[0].max_options = 1,
            2 => {
                if let OutputKind::TextRecord {
                    max_output_bytes, ..
                } = &mut b.operations[0].output
                {
                    *max_output_bytes = 1;
                }
            }
            3 => {
                if let OutputKind::TextRecord { languages, .. } = &mut b.operations[0].output {
                    *languages = vec!["pl-PL".into()];
                }
            }
            4 => b.input_limit.max_bytes = 1,
            _ => b.operations[0].output = OutputKind::Label,
        }
        let e = run(&d, &p, &b).unwrap_err();
        assert_eq!(e.category, C::NoCapableBackend);
        assert!(!e.shortfalls[0].reasons.is_empty());
    }
    let mut d = d.clone();
    d.limits.max_semantic_requests = 1;
    assert_eq!(run(&d, &p, &b).unwrap_err().category, C::Budget);
    d.limits.on_excess = ExcessPolicy::TruncateVisible;
    assert!(run(&d, &p, &b).is_ok());
    let mut e = common::execution(rustev_contract::execution::CostPolicy::Unlimited, vec![]);
    e.steps.clear();
    e.max_attempts_per_decision = 1;
    assert_eq!(
        compile_with_templates(&d, &[b], &[], &[], Some(&e), &[p])
            .unwrap_err()
            .category,
        C::InvalidExecution
    );
}

#[test]
fn iteration_and_remaining_compile_rows() {
    let (mut d, p, b) = fixture();
    d.limits.max_semantic_requests = 8;
    input(
        &mut d,
        "candidates",
        json!({"list":{"item":{"record":{"fields":[{"name":"id","type":{"text":{"max_bytes":8}}},{"name":"name","type":{"text":{"max_bytes":12}}},{"name":"price","type":"integer"}]}},"max_items":4}}),
        false,
    );
    input(
        &mut d,
        "selected",
        json!({"list":{"item":{"text":{"max_bytes":8}},"max_items":4}}),
        false,
    );
    d.policy.rules[0].then=serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"selected","value":{"ref":"input:selected"}}]}})).unwrap();
    edit(&mut d, |g| {
        g["for_each"] = json!({"over":"proposal:selected","bind":"item","items":{"list":{"list":"input:candidates","id_field":"id"}},"max_items":{"literal":4}});
        g["projection"] = json!(["bind:item/name"]);
        g["carry"] = json!(["bind:item/price"]);
        g["backstop"]["fields"]["headline"] = json!({"text":"Option: {{item.name}}"});
    });
    let c = run(&d, &p, &b).unwrap();
    let PlanStepDetail::Generate(g) = &c.plan.steps[0].detail else {
        panic!()
    };
    assert_eq!(g.max_items, 4);
    assert_eq!(g.worst_case_requests, 8);
    for source in ["candidates", "selected"] {
        let mut untrusted = d.clone();
        untrusted
            .inputs
            .iter_mut()
            .find(|i| i.name == source)
            .unwrap()
            .provenance
            .push(ProvenanceClass::UserSupplied);
        let refusal = run(&untrusted, &p, &b).unwrap_err();
        assert_eq!(refusal.category, C::InvalidGeneration);
        assert!(refusal.detail.contains(&format!("input:{source}")));
    }
    for bind in ["_item", "input"] {
        let mut bad = d.clone();
        edit(&mut bad, |g| g["for_each"]["bind"] = json!(bind));
        assert_eq!(
            run(&bad, &p, &b).unwrap_err().category,
            C::InvalidGeneration
        );
    }
    for source in ["{{item.name}}", "{{{item.name}}}", "{{#item.name}}"] {
        let mut bad = d.clone();
        edit(&mut bad, |g| {
            g["backstop"]["fields"]["headline"] = json!({"text":source})
        });
        assert_eq!(run(&bad, &p, &b).unwrap_err().category, C::Template);
    }
    for collision in 0..2 {
        let mut bad = d.clone();
        edit(&mut bad, |g| {
            if collision == 0 {
                g["output"]["fields"][0]["name"] = json!("price");
            } else {
                g["carry"] = json!(["bind:item/price", "input:price"]);
            }
        });
        assert_eq!(
            run(&bad, &p, &b).unwrap_err().category,
            C::InvalidGeneration
        );
    }
    let (mut d, mut p, b) = fixture();
    input(&mut d, "count", json!("integer"), true);
    p.params = vec![PromptParam {
        name: "count".into(),
        ty: PromptParamType::Integer,
    }];
    edit(&mut d, |g| {
        g["template"]["digest"] = json!(p.id().unwrap());
        g["template"]["params"] = json!({"count":"input:count"});
    });
    assert_eq!(run(&d, &p, &b).unwrap_err().category, C::ModelOutputFlow);
}

#[test]
fn refusal_category_goldens() {
    for category in 22..=24 {
        let (mut d, p, b) = fixture();
        edit(&mut d, |g| match category {
            22 => {
                g["backstop"]["fields"]
                    .as_object_mut()
                    .unwrap()
                    .remove("headline");
            }
            23 => g["template"]["digest"] = json!(common::id('0')),
            _ => g["projection"] = json!(["step:item_text"]),
        });
        let r = run(&d, &p, &b).unwrap_err();
        assert_eq!(r.category as u8, category);
        let bytes = format!("{}\n{}\n{}\n", r.category as u8, r.subject, r.detail);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/golden/generation-{category}.refusal"));
        if std::env::var_os("RUSTEV_BLESS").is_some() {
            std::fs::write(&path, &bytes).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), bytes);
    }
}

#[test]
fn generated_flow_is_refused_in_exact_semantic_threshold_and_acquisition() {
    for location in 0..4 {
        let (mut d, p, b) = fixture();
        match location {
            0=>d.steps.push(serde_json::from_value(json!({"id":"read","body":{"exact":{"op":"arith","version":1,"args":{"expr":{"ref":"step:item_text"}}}}})).unwrap()),
            1=>{
                let mut s=common::support_routing().steps.into_iter().find(|s|matches!(s.body,StepBody::Semantic(_))).unwrap();
                if let StepBody::Semantic(ref mut s)=s.body {s.project=vec!["step:item_text".into()];}
                d.steps.push(s);
            },
            2=>d.policy.rules[0].when=Cond::TopMassBelow{step:"item_text".into(),threshold:common::dec("0.5"),uncalibrated_threshold:UncalibratedThreshold::NotDeclared},
            _=>{
                let plan:Value=serde_json::from_str(include_str!("golden/acquisition-widen.plan.json")).unwrap();
                let mut s=plan["definition"]["steps"][0].clone();
                s["body"]["widen"]["base_request"]["q"]=json!({"ref":"step:item_text"});
                d.steps.push(serde_json::from_value(s).unwrap());
            },
        }
        assert_eq!(
            run(&d, &p, &b).unwrap_err().category,
            C::ModelOutputFlow,
            "location {location}"
        );
    }
}

#[test]
fn generation_fallbacks_match_capacities_and_count_attempts() {
    use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
    let (d, p, b) = fixture();
    let mut fallback = b.clone();
    fallback.backend_id = "zz-fallback".into();
    if let OutputKind::TextRecord {
        max_output_bytes, ..
    } = &mut fallback.operations[0].output
    {
        *max_output_bytes += 1;
    }
    let mut e = common::execution(
        CostPolicy::Unlimited,
        vec![common::step_exec(
            "item_text",
            1,
            &[],
            Delay::None,
            &["zz-fallback"],
            &[FailureClass::Permanent],
        )],
    );
    let backends = [b, fallback];
    assert!(
        compile_with_templates(&d, &backends, &[], &[], Some(&e), std::slice::from_ref(&p)).is_ok()
    );
    e.max_attempts_per_decision = 3;
    assert_eq!(
        compile_with_templates(&d, &backends, &[], &[], Some(&e), &[p])
            .unwrap_err()
            .category,
        C::InvalidExecution
    );
}

#[test]
fn interim_screening_refuses_untrusted_text_and_accepts_system_of_record_only() {
    for projection in ["input:message", "proposal:message"] {
        for untrusted in [false, true] {
            let (mut d, p, b) = fixture();
            input(&mut d, "message", json!({"text":{"max_bytes":20}}), false);
            if untrusted {
                d.inputs[0].provenance.push(ProvenanceClass::UserSupplied);
            }
            d.policy.rules[0].then=serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"message","value":{"ref":"input:message"}}]}})).unwrap();
            edit(&mut d, |g| g["projection"] = json!([projection]));
            let result = run(&d, &p, &b);
            if untrusted {
                let refusal = result.unwrap_err();
                assert_eq!(refusal.category, C::InvalidGeneration);
                assert_eq!(refusal.subject, "step:item_text");
                assert!(refusal.detail.contains("input:message"));
            } else {
                assert!(result.is_ok(), "{result:?}");
            }
        }
    }
}

#[test]
fn interim_screening_follows_nested_text_and_exact_step_lineage() {
    for projection in ["input:records", "step:first"] {
        let (mut d, p, b) = fixture();
        input(
            &mut d,
            "records",
            json!({"list":{"item":{"record":{"fields":[{"name":"message","type":{"text":{"max_bytes":20}}}]}},"max_items":2}}),
            false,
        );
        d.inputs[0]
            .provenance
            .push(ProvenanceClass::AuthenticatedAppField);
        d.steps.push(serde_json::from_value(json!({"id":"first","body":{"exact":{"op":"filter_with_reasons","version":1,"args":{"list":"input:records","id_field":"message","rules":[]}}}})).unwrap());
        edit(&mut d, |g| g["projection"] = json!([projection]));
        let refusal = run(&d, &p, &b).unwrap_err();
        assert_eq!(refusal.category, C::InvalidGeneration);
        assert!(refusal.detail.contains("input:records"));
    }
}

#[test]
fn backend_binding_ids_are_literals_even_when_they_resemble_step_references() {
    for backend_id in ["item_text", "step:item_text"] {
        let (mut d, p, mut b) = fixture();
        b.backend_id = backend_id.into();
        edit(&mut d, |g| {
            g["backend"]["binding"] = json!({"backend": backend_id});
        });
        let compiled = run(&d, &p, &b).unwrap();
        let PlanStepDetail::Generate(binding) = &compiled.plan.steps[0].detail else {
            panic!("expected generate binding");
        };
        assert_eq!(binding.backend_id, backend_id);
    }
}

#[test]
fn semantic_step_cannot_supply_a_backstop_path() {
    let (mut d, p, b) = fixture();
    d.steps.push(StepDecl {
        id: "model".into(),
        body: StepBody::Semantic(common::semantic(
            Operation::Classify,
            "label",
            "Choose",
            &["a", "b"],
            RequiredKind::Label,
            &[],
        )),
    });
    d.policy.on_unresolved.push(serde_json::from_value(json!({"target":"step:model","reasons":"any","action":{"escalate":{"reason":"missing"}}})).unwrap());
    let source = "Answer: {{step.model}}";
    edit(&mut d, |g| {
        g["backstop"]["fields"]["headline"] = json!({"text":source});
    });
    let refusal = run(&d, &p, &b).unwrap_err();
    assert_eq!(refusal.category, C::Template, "{refusal:?}");
    assert!(refusal.detail.contains("NotExact"), "{refusal:?}");
}
