//! Spec 070 increment 2b, 3.8: a field a `vocabulary_after` rule can fill is
//! graded `hint` statically and dynamically, whatever its origin; a hard
//! gate over it is refused with category 14 unless a corroboration step
//! raises it.
mod common;
use rustev_contract::{
    definition::{Definition, Operation, ProvenanceClass as P, StepBody},
    descriptor::{BackendDescriptor, OutputKind, TextStructure},
    extraction::{ExtractionVocabulary, Fallback},
    extraction_documents::ExtractRules,
    extraction_results::ExtractionOrigin,
    generation::Vocabulary,
    grade::ValueGrade,
    output::RawOutput,
    plan::{Plan, PlanStepDetail},
};
use rustev_core::{
    Category, Compiled, Evaluation, Refusal, Supplied, compile::compile_with_extraction_documents,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn backend() -> BackendDescriptor {
    let mut b = common::linear_head();
    b.operations = vec![common::support(
        Operation::Extract,
        OutputKind::TypedRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 65536,
            max_options: 32,
            option_logprobs: true,
        },
        32,
    )];
    b
}

fn template() -> rustev_contract::generation::PromptTemplate {
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let PlanStepDetail::Extract(b) = &plan.steps[0].detail else {
        panic!()
    };
    b.template.clone()
}

fn program() -> ExtractRules {
    serde_json::from_value(json!({"schema":"rustev.extract-rules/2","name":"airports","version":"1",
        "sources":[{"name":"text","ref":"input:message"}],
        "fields":[{"field":"area_name","rules":[{"name":"from","when":"always",
            "then":{"vocabulary_after":{"source":"text","terms":["from"],"vocabulary":"airports"}}}]}]}))
    .unwrap()
}

/// The acceptance definition with an `airports` vocabulary, the rules
/// program filling `area_name`, and `area_ok` accepting that field.
fn definition() -> Definition {
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let mut d = plan.definition;
    let StepBody::Extract(e) = &mut d.steps[0].body else {
        panic!()
    };
    e.vocabularies.insert(
        "airports".into(),
        ExtractionVocabulary::Envelope(Vocabulary::Literal(vec!["LHR".into(), "JFK".into()])),
    );
    e.fallback = serde_json::from_value::<Fallback>(
        json!({"rules_program":"airports@1","fill":"absent_fields"}),
    )
    .unwrap();
    d.steps[1] = serde_json::from_value(json!({"id":"area_ok","body":{"accept_model_derived":{
        "field":"step:request_filters/area_name","min_mass":"none",
        "on_fail":{"drop_field":{"notice":"area_dropped"}},"validate":"none"}}}))
    .unwrap();
    d
}

fn compile(d: &Definition) -> Result<Compiled, Refusal> {
    compile_with_extraction_documents(d, &[backend()], &[], &[], None, &[template()], &[program()])
}

fn gate_on(d: &mut Definition, reference: &str) {
    gate_with(
        d,
        json!({"exact":{"cmp":{"left":{"ref":reference},"op":"eq","right":{"lit":{"text":"LHR"}}}}}),
    );
}

/// A hard gate: a policy rule before the default one.
fn gate_with(d: &mut Definition, when: Value) {
    let rule = serde_json::from_value(json!({"id":"gated","when":when,
        "then":{"propose":{"action":"recommend","params":[]}}}))
    .unwrap();
    d.policy.rules.insert(0, rule);
}

#[test]
fn a_hard_gate_over_a_vocabulary_field_is_refused_uncorroborated() {
    let d = definition();
    let c = compile(&d).unwrap();
    // Without a gate the plan compiles.
    assert_eq!(c.plan.schema, "rustev.plan/3");
    // Section 5: a hard gate reading an accept step over the field.
    let mut d = definition();
    gate_on(&mut d, "step:area_ok");
    let e = compile(&d).unwrap_err();
    assert_eq!(e.category, Category::UncorroboratedHint, "{e:?}");
    assert!(e.detail.contains("uncorroborated_hint"), "{e:?}");
    // Another field of the same step keeps its ungraded ceiling.
    let mut d = definition();
    d.steps[1] = serde_json::from_value(json!({"id":"area_ok","body":{"accept_model_derived":{
        "field":"step:request_filters/insufficient","min_mass":"none",
        "on_fail":{"drop_field":{"notice":"dropped"}},"validate":"none"}}}))
    .unwrap();
    gate_with(
        &mut d,
        json!({"exact":{"cmp":{"left":{"ref":"step:area_ok"},"op":"eq","right":{"lit":{"bool":true}}}}}),
    );
    d.policy.on_unresolved = serde_json::from_value(json!([{"target":"step:area_ok",
        "reasons":"any","action":{"escalate":{"reason":"unaccepted"}}}]))
    .unwrap();
    compile(&d).unwrap();
}

#[test]
fn a_corroboration_step_raises_the_hint() {
    let mut d = definition();
    let mut set = d.inputs[0].clone();
    set.name = "known_airports".into();
    set.provenance = vec![P::SystemOfRecord];
    set.ty =
        serde_json::from_value(json!({"list":{"item":{"text":{"max_bytes":8}},"max_items":8}}))
            .unwrap();
    d.inputs.push(set);
    d.steps.push(serde_json::from_value(json!({"id":"area_checked","body":{"corroborate":{
        "claim":"step:area_ok","upgrade_to":"estimated",
        "by":[{"predicate":{"member_ref":{"value":{"ref":"step:area_ok"},"set":"input:known_airports"}},
            "on_false":"contradicts"}],
        "mode":"all","on_contradiction":"conflict"}}}))
    .unwrap());
    gate_on(&mut d, "step:area_checked");
    d.policy.on_unresolved = serde_json::from_value(json!([{"target":"step:area_checked",
        "reasons":"any","action":{"escalate":{"reason":"area unchecked"}}}]))
    .unwrap();
    compile(&d).unwrap();
    // The same gate on the accept step itself is refused.
    d.policy.rules.remove(0);
    gate_on(&mut d, "step:area_ok");
    assert_eq!(
        compile(&d).unwrap_err().category,
        Category::UncorroboratedHint
    );
}

fn run(supplied: RawOutput, message: &str) -> rustev_contract::evidence::EvidenceRecord {
    let c = compile(&definition()).unwrap();
    let snapshot = common::snapshot(vec![common::entry(
        "message",
        json!(message),
        P::SystemOfRecord,
        common::NOW,
    )]);
    let mut e = Evaluation::start(&c, &snapshot, common::ts(common::NOW)).unwrap();
    e.supply("request_filters", &[], Supplied::Output(supplied))
        .unwrap();
    e.finish("synthetic-extraction").unwrap().1
}

fn output(fields: Value) -> RawOutput {
    RawOutput::TypedRecord {
        fields: serde_json::from_value(fields).unwrap(),
        logprobs: vec![],
        provider_reported: BTreeMap::new(),
    }
}

fn step_meta(
    ev: &rustev_contract::evidence::EvidenceRecord,
    step: &str,
) -> rustev_contract::grade::ValueMeta {
    let v = serde_json::to_value(ev).unwrap();
    let steps = v["steps"].as_array().unwrap();
    let s = steps.iter().find(|s| s["step"] == step).unwrap();
    serde_json::from_value(s["meta"].clone()).unwrap_or_else(|_| panic!("{step}: {s}"))
}

#[test]
fn every_present_value_of_the_field_is_hint_whatever_its_origin() {
    let full = json!({"window":{"start":"2027-01-01","end":"2027-01-03"},"party_size":4,
        "features":["pool"],"insufficient":false});
    // Section 5: the same field extracted by the model is graded hint.
    let mut model = full.clone();
    model["area_name"] = json!("Soho");
    let ev = run(output(model), "from LHR");
    let extraction = ev.extraction.as_ref().unwrap();
    let area = extraction[0]
        .fields
        .iter()
        .find(|f| f.name == "area_name")
        .unwrap();
    assert_eq!(area.origin, ExtractionOrigin::Extracted);
    let meta = step_meta(&ev, "request_filters");
    let fields = meta.fields.as_ref().unwrap();
    assert_eq!(fields["area_name"].grade, ValueGrade::Hint);
    assert_eq!(fields["party_size"].grade, ValueGrade::Ungraded);
    assert_eq!(meta.grade, ValueGrade::Hint);
    // The accept step reads it as hint.
    assert_eq!(step_meta(&ev, "area_ok").grade, ValueGrade::Hint);
    // Filled by the rules when the model leaves it absent.
    let ev = run(output(full.clone()), "from LHR");
    let extraction = ev.extraction.as_ref().unwrap();
    let area = extraction[0]
        .fields
        .iter()
        .find(|f| f.name == "area_name")
        .unwrap();
    assert_eq!(area.origin, ExtractionOrigin::Fallback);
    assert_eq!(
        step_meta(&ev, "request_filters").fields.unwrap()["area_name"].grade,
        ValueGrade::Hint
    );
    // An absent value carries no grade.
    let ev = run(output(full), "from nowhere");
    assert_eq!(
        step_meta(&ev, "request_filters").fields.unwrap()["area_name"].grade,
        ValueGrade::Ungraded
    );
}
