//! Synthetic extraction compilation refusals, without backend calls or plans.
mod common;
use rustev_contract::{
    Identified,
    definition::{Definition, ProvenanceClass as P, StepBody, TypeDecl},
    extraction::{Extract, ExtractionType as T, ExtractionVocabulary as V, Fallback},
    extraction_documents::ExtractRules,
    generation::{PromptTemplate, ScreenOutput, Vocabulary},
};
use rustev_core::{Category as C, Refusal, compile::compile_with_extraction_documents};
use serde_json::json;

fn fixture() -> Definition {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    let mut input = d.inputs[0].clone();
    input.name = "message".into();
    input.ty = TypeDecl::Text { max_bytes: 128 };
    input.provenance = vec![P::SystemOfRecord];
    d.inputs = vec![input];
    d.policy = serde_json::from_value(json!({"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[],"on_unresolved":[]})).unwrap();
    d.steps = vec![
        serde_json::from_str(include_str!(
            "../../rustev-contract/tests/fixtures/extract-step.json"
        ))
        .unwrap(),
    ];
    let e = extract(&mut d);
    e.projection = vec!["input:message".into()];
    e.vocabularies.insert(
        "feature_vocabulary".into(),
        V::Envelope(Vocabulary::Literal(vec!["pool".into()])),
    );
    e.fallback = Fallback::None;
    e.template.digest = prompt().id().unwrap();
    d
}
fn extract(d: &mut Definition) -> &mut Extract {
    let StepBody::Extract(e) = &mut d.steps[0].body else {
        panic!()
    };
    e
}
fn refusal(d: &Definition, programs: &[ExtractRules]) -> Refusal {
    compile_with_extraction_documents(d, &[], &[], &[], None, &[prompt()], programs).unwrap_err()
}
fn check(d: &Definition, category: C, detail: &str) {
    let e = refusal(d, &[]);
    assert_eq!(e.category, category, "{e:?}");
    assert_eq!(e.subject, "step:request_filters", "{e:?}");
    assert!(e.detail.contains(detail), "{e:?}");
}
fn program() -> ExtractRules {
    serde_json::from_value(json!({"schema":"rustev.extract-rules/1","name":"filters-fallback","version":"1",
        "sources":[{"name":"text","ref":"input:message"}],
        "fields":[{"field":"party_size","rules":[{"name":"one","when":"always","then":{"literal":2}}]}]})).unwrap()
}
fn fallback(d: &mut Definition) {
    extract(d).fallback = serde_json::from_value(
        json!({"rules_program":"filters-fallback@1","fill":"absent_fields"}),
    )
    .unwrap();
}
#[test]
fn valid_structure_remains_closed_and_v1_guard_is_unchanged() {
    let mut d = fixture();
    check(&d, C::InvalidDefinition, "extraction-not-compiled");
    d.schema = "rustev.definition/1".into();
    check(&d, C::InvalidDefinition, "requires-definition-2");
}
#[test]
fn directly_constructed_invalid_projection_is_refused() {
    // extraction_compile::structure's serde_json::from_value::<Extract> check
    // returns "invalid extraction projection" for all five via the contract's
    // extraction::projection: both valid_ref(input:, 1, 2) and valid_ref(step:, 1, 1)
    // fail. In extraction::valid_ref, "other:x" fails strip_prefix for both;
    // "input:" and "input:message/" fail !p.is_empty() for an input path part;
    // "input:message/a/b" fails (min..=max).contains(&parts.len()) (3 > 2);
    // "step:" fails !p.is_empty() for its step id. Each input/step reference
    // also fails the other prefix, so none reaches structure's resolution loop.
    for projection in [
        "other:x",
        "input:",
        "input:message/",
        "input:message/a/b",
        "step:",
    ] {
        let mut d = fixture();
        extract(&mut d).projection = vec![projection.into()];
        check(&d, C::InvalidExtraction, "invalid extraction projection");
    }
}
#[test]
fn projections_resolve_inputs_and_record_paths() {
    let mut d = fixture();
    extract(&mut d).projection = vec!["input:missing".into()];
    check(&d, C::InvalidExtraction, "unknown projection input");
    extract(&mut d).projection = vec!["input:message/missing".into()];
    check(&d, C::InvalidExtraction, "has no field");
    extract(&mut d).projection = vec!["step:missing".into()];
    check(&d, C::InvalidExtraction, "must name an accept step");
    extract(&mut d).projection.clear();
    check(&d, C::InvalidExtraction, "projection");
    extract(&mut d).projection = vec!["input:message".into(); 9];
    check(&d, C::InvalidExtraction, "projection");
}
#[test]
fn extract_step_count_is_bounded() {
    let mut d = fixture();
    d.limits.max_semantic_requests = 8;
    for i in 1..9 {
        let mut s = d.steps[0].clone();
        s.id = format!("extract_{i}");
        d.steps.push(s);
    }
    let e = refusal(&d, &[]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:extract_8");
    assert!(e.detail.contains("eight"));
    d.steps.pop();
    check(&d, C::InvalidDefinition, "extraction-not-compiled");
}
#[test]
fn limits_repair_screen_and_declared_vocabularies_apply_to_typed_callers() {
    let mut d = fixture();
    extract(&mut d).limits.deadline_share = serde_json::from_value(json!("0")).unwrap();
    check(&d, C::InvalidExtraction, "deadline_share");
    extract(&mut d).limits.deadline_share = serde_json::from_value(json!("1.00001")).unwrap();
    check(&d, C::InvalidExtraction, "deadline_share");
    extract(&mut d).limits.deadline_share = serde_json::from_value(json!("1")).unwrap();
    extract(&mut d).repair.max_attempts = 1;
    check(&d, C::InvalidExtraction, "no repair");
    extract(&mut d).repair.max_attempts = 0;
    extract(&mut d).screen.output = ScreenOutput::Screener {
        backend: extract(&mut d).backend.clone(),
    };
    check(&d, C::InvalidExtraction, "output screening");
    extract(&mut d).screen.output = ScreenOutput::None;
    extract(&mut d).vocabularies.clear();
    check(&d, C::InvalidExtraction, "undeclared extraction vocabulary");
}
#[test]
fn literal_vocabulary_bounds_reuse_generation_validation() {
    for options in [
        vec![],
        vec!["x".repeat(65)],
        (0..257).map(|n| n.to_string()).collect(),
        vec!["a".into(), "a".into()],
    ] {
        let mut d = fixture();
        extract(&mut d).vocabularies.insert(
            "feature_vocabulary".into(),
            V::Envelope(Vocabulary::Literal(options)),
        );
        let e = refusal(&d, &[]);
        assert_eq!(e.category, C::InvalidExtraction, "{e:?}");
        assert_eq!(e.subject, "step:request_filters");
    }
}
fn input_vocabulary(d: &mut Definition, ty: TypeDecl) {
    let mut input = d.inputs[0].clone();
    input.name = "vocab".into();
    input.ty = ty;
    d.inputs.push(input);
    extract(d).vocabularies.insert(
        "feature_vocabulary".into(),
        V::Input {
            input: "input:vocab".into(),
        },
    );
}
#[test]
fn input_vocabularies_resolve_types_provenance_and_enum_bound() {
    let mut d = fixture();
    input_vocabulary(&mut d, TypeDecl::Bool);
    check(&d, C::InvalidExtraction, "needs text list or enum");
    d.inputs[1].ty = TypeDecl::Enum {
        options: (0..4097).map(|n| n.to_string()).collect(),
    };
    check(&d, C::InvalidExtraction, "4096");
    let TypeDecl::Enum { options } = &mut d.inputs[1].ty else {
        panic!()
    };
    options.pop();
    check(&d, C::InvalidDefinition, "extraction-not-compiled");
    d.inputs[1].provenance.push(P::ModelDerived);
    check(&d, C::InvalidExtraction, "admits model-derived");
    d.inputs.pop();
    check(&d, C::InvalidExtraction, "unknown vocabulary input");
}
#[test]
fn list_vocabulary_runtime_bound_is_not_a_compile_bound() {
    let mut d = fixture();
    input_vocabulary(
        &mut d,
        TypeDecl::List {
            item: Box::new(TypeDecl::Text { max_bytes: 128 }),
            max_items: 5000,
        },
    );
    check(&d, C::InvalidDefinition, "extraction-not-compiled");
}
#[test]
fn untrusted_projection_fails_closed_but_trusted_text_passes() {
    for p in [P::UserSupplied, P::ModelDerived] {
        let mut d = fixture();
        d.inputs[0].provenance.push(p);
        check(&d, C::InvalidExtraction, "untrusted text");
        d.inputs[0].ty = TypeDecl::Bool;
        check(&d, C::InvalidDefinition, "extraction-not-compiled");
    }
}
#[test]
fn fallback_must_be_supplied_and_bind() {
    let mut d = fixture();
    fallback(&mut d);
    check(&d, C::InvalidExtraction, "not supplied");
    let p = program();
    assert_eq!(
        refusal(&d, std::slice::from_ref(&p)).detail,
        "extraction-not-compiled"
    );
    let mut incompatible = p.clone();
    incompatible.fields[0].field = "unknown".into();
    let e = refusal(&d, &[incompatible]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
    assert!(e.detail.contains("undeclared field"));
    let mut incompatible = p.clone();
    incompatible.sources[0].reference = "input:other".into();
    let e = refusal(&d, &[incompatible]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
    assert!(e.detail.contains("not in the input projection"));
    extract(&mut d).output.fields[1].ty = T::Bool {};
    let e = refusal(&d, &[p]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
    assert!(e.detail.contains("incompatible value"));
}
#[test]
fn structural_errors_precede_resolution_and_resolution_precedes_types() {
    let mut d = fixture();
    fallback(&mut d);
    extract(&mut d).repair.max_attempts = 1;
    d.inputs[0].provenance.push(P::UserSupplied);
    check(&d, C::InvalidExtraction, "no repair");
    extract(&mut d).repair.max_attempts = 0;
    check(&d, C::InvalidExtraction, "not supplied");
    let e = refusal(&d, &[program()]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
    assert!(e.detail.contains("untrusted text"));
}

#[test]
fn cost_limit_must_be_positive() {
    let mut d = fixture();
    extract(&mut d).limits.max_cost_units = rustev_contract::generation::CostLimit::Units { n: 0 };
    check(&d, C::InvalidExtraction, "bound");
}
fn accept(d: &mut Definition, field: &str) {
    let mut a: rustev_contract::definition::StepDecl = serde_json::from_str(include_str!(
        "../../rustev-contract/tests/fixtures/accept-step.json"
    ))
    .unwrap();
    let StepBody::AcceptModelDerived(body) = &mut a.body else {
        panic!()
    };
    body.field = field.into();
    body.validate = rustev_contract::extraction::Validation::None;
    d.steps.push(a);
    extract(d).projection = vec!["step:window_ok".into()];
}
#[test]
fn oversized_input_projection_is_refused_even_with_a_step_reference() {
    for projection in [
        vec!["input:message".into(), "step:window_ok".into()],
        vec!["step:window_ok".into(), "input:message".into()],
    ] {
        let mut d = fixture();
        accept(&mut d, "input:message");
        extract(&mut d).projection = projection;
        d.limits.max_projection_bytes = 127;
        check(&d, C::Budget, "max_projection_bytes");
    }
}
#[test]
fn accepted_input_projection_preserves_untrusted_text() {
    let mut d = fixture();
    accept(&mut d, "input:message");
    check(&d, C::InvalidDefinition, "extraction-not-compiled");
    d.inputs[0].provenance.push(P::UserSupplied);
    check(&d, C::InvalidExtraction, "untrusted text");
}
#[test]
fn graph_cycle_precedes_projection_type_checks() {
    let mut d = fixture();
    accept(&mut d, "step:request_filters/area_name");
    let e = refusal(&d, &[]);
    assert_eq!(e.category, C::Cycle, "{e:?}");
    assert_eq!(e.subject, "step:request_filters");
}
#[test]
fn fallback_conflicting_documents_are_refused() {
    let mut d = fixture();
    fallback(&mut d);
    let p = program();
    let mut other = p.clone();
    other.fields[0].field = "area_name".into();
    let e = refusal(&d, &[p.clone(), other]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
    assert!(e.detail.contains("conflicting"));
    assert_eq!(
        refusal(&d, &[p.clone(), p]).detail,
        "extraction-not-compiled"
    );
}

#[test]
fn fallback_list_rules_are_checked_by_the_shared_binder() {
    let mut d = fixture();
    fallback(&mut d);
    let mut p = program();
    p.fields[0].field = "features".into();
    p.fields[0].rules[0].then = serde_json::from_value(json!({"literal":"pool"})).unwrap();
    let mut duplicate = p.fields[0].rules[0].clone();
    duplicate.name = "two".into();
    p.fields[0].rules.push(duplicate);
    let e = refusal(&d, std::slice::from_ref(&p));
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
    assert!(e.detail.contains("duplicate literal"));
    p.fields[0].rules.pop();
    p.fields[0].rules[0].then = serde_json::from_value(
        json!({"integer_after":{"source":"text","terms":["party"],"max_digits":2}}),
    )
    .unwrap();
    let e = refusal(&d, &[p]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
    assert!(e.detail.contains("requires literal items"));
}

fn prompt() -> PromptTemplate {
    serde_json::from_value(json!({"schema":"rustev.prompt-template/1","id":"filters","version":"1.0.0","instructions":"Extract filters.","params":[]})).unwrap()
}
#[test]
fn template_resolution_requires_matching_identity_and_bindings() {
    let mut d = fixture();
    let e = compile_with_extraction_documents(&d, &[], &[], &[], None, &[], &[]).unwrap_err();
    assert_eq!(e.category, C::Template);
    assert!(e.detail.contains("missing or identity differs"));
    extract(&mut d).template.params.insert(
        "extra".into(),
        serde_json::from_value(json!({"literal":{"bool":true}})).unwrap(),
    );
    check(&d, C::Template, "missing or extra");
    let mut other = prompt();
    other.instructions.push('!');
    extract(&mut d).template.params.clear();
    let e = compile_with_extraction_documents(&d, &[], &[], &[], None, &[prompt(), other], &[])
        .unwrap_err();
    assert_eq!(e.category, C::Template);
    assert!(e.detail.contains("conflicting"));
}
#[test]
fn prompt_bindings_and_instruction_paths_are_checked() {
    for (binding, instructions, category) in [
        (json!({"literal":{"bool":true}}), "{{param.n}}", C::Template),
        (json!({"param":"unresolved"}), "{{param.n}}", C::Template),
        (json!("input:message"), "{{param.n}}", C::Template),
        (
            json!({"literal":{"integer":2}}),
            "{{param.unknown}}",
            C::Template,
        ),
    ] {
        let mut d = fixture();
        d.inputs[0].provenance = vec![P::ModelDerived];
        d.inputs[0].ty = TypeDecl::Integer;
        let mut p = prompt();
        p.params = serde_json::from_value(json!([{"name":"n","type":"integer"}])).unwrap();
        p.instructions = instructions.into();
        extract(&mut d).template.digest = p.id().unwrap();
        extract(&mut d)
            .template
            .params
            .insert("n".into(), serde_json::from_value(binding).unwrap());
        let e = compile_with_extraction_documents(&d, &[], &[], &[], None, &[p], &[]).unwrap_err();
        assert_eq!(e.category, category, "{e:?}");
    }
}
#[test]
fn extraction_request_and_projection_budgets_are_category_seven() {
    let mut d = fixture();
    d.limits.max_semantic_requests = 0;
    check(&d, C::Budget, "max_semantic_requests");
    d.limits.max_semantic_requests = 1;
    d.limits.max_projection_bytes = 1;
    check(&d, C::Budget, "max_projection_bytes");
    d.limits.max_projection_bytes = 65536;
    check(&d, C::InvalidDefinition, "extraction-not-compiled");
    let policy = serde_json::from_value(json!({"schema":"rustev.execution/1","max_attempts_per_decision":0,"cost":"unlimited","steps":[]})).unwrap();
    let e = compile_with_extraction_documents(&d, &[], &[], &[], Some(&policy), &[prompt()], &[])
        .unwrap_err();
    assert_eq!(e.category, C::Budget);
    assert!(e.detail.contains("max_attempts_per_decision"));
}
#[test]
fn fallback_canonical_document_is_validated() {
    let mut d = fixture();
    fallback(&mut d);
    let mut p = program();
    p.schema = "wrong".into();
    let e = refusal(&d, &[p]);
    assert_eq!(e.category, C::InvalidExtraction);
    assert!(e.detail.contains("schema"));
}
#[cfg(feature = "parameterized")]
#[test]
fn profile_vocabulary_and_prompt_parameter_resolve_before_compilation() {
    let mut definition = fixture();
    let mut p = prompt();
    p.params = serde_json::from_value(json!([{"name":"n","type":"integer"}])).unwrap();
    p.instructions = "Extract {{param.n}} filters.".into();
    extract(&mut definition).template.digest = p.id().unwrap();
    let mut d = serde_json::to_value(definition).unwrap();
    d["steps"][0]["body"]["extract"]["template"]["params"]["n"] = json!({"param":"count"});
    d["parameters"] = json!([{"name":"options","type":{"list":{"item":{"text":{"max_bytes":64}},"max_items":256}},"lock":"free","bounds":"none","default":"none"}]);
    d["steps"][0]["body"]["extract"]["vocabularies"]["feature_vocabulary"] =
        json!({"param":"options"});
    d["parameters"].as_array_mut().unwrap().push(json!({"name":"count","type":"integer","lock":"free","bounds":"none","default":{"value":2}}));
    let mut profile = json!({"schema":"rustev.profile/1","profile":"test","version":"1.0.0","definition":{"name":d["name"],"version":d["version"]},"extends":"none","values":{"options":["pool"]}});
    let bytes = serde_json::to_vec(&d).unwrap();
    let instantiate = |p: &serde_json::Value| {
        rustev_core::instantiate::instantiate(
            &[&bytes],
            Some(&serde_json::to_vec(p).unwrap()),
            &[],
            None,
            None,
        )
    };
    let instantiated = instantiate(&profile).unwrap();
    let e = compile_with_extraction_documents(
        &instantiated.definition,
        &[],
        &[],
        &[],
        None,
        std::slice::from_ref(&p),
        &[],
    )
    .unwrap_err();
    assert_eq!(e.detail, "extraction-not-compiled");
    profile["values"]["options"] = json!(["pool", "pool"]);
    let e = compile_with_extraction_documents(
        &instantiate(&profile).unwrap().definition,
        &[],
        &[],
        &[],
        None,
        &[p],
        &[],
    )
    .unwrap_err();
    assert_eq!(e.category, C::InvalidExtraction);
    assert_eq!(e.subject, "step:request_filters");
}

#[test]
fn unresolved_profile_vocabulary_is_refused() {
    let mut d = fixture();
    extract(&mut d).vocabularies.insert(
        "feature_vocabulary".into(),
        serde_json::from_value(json!({"param":"options"})).unwrap(),
    );
    check(&d, C::InvalidExtraction, "unresolved vocabulary");
}

#[test]
fn exact_template_reference_passes_and_text_reference_is_refused() {
    let mut d = fixture();
    d.inputs[0].ty = TypeDecl::Integer;
    let mut p = prompt();
    p.params = serde_json::from_value(json!([{"name":"n","type":"integer"}])).unwrap();
    p.instructions = "Extract {{param.n}} filters.".into();
    extract(&mut d).template.params.insert(
        "n".into(),
        serde_json::from_value(json!("input:message")).unwrap(),
    );
    extract(&mut d).template.digest = p.id().unwrap();
    let e =
        compile_with_extraction_documents(&d, &[], &[], &[], None, std::slice::from_ref(&p), &[])
            .unwrap_err();
    assert_eq!(e.detail, "extraction-not-compiled");
    d.inputs[0].ty = TypeDecl::Text { max_bytes: 128 };
    p.params =
        serde_json::from_value(json!([{"name":"n","type":{"text":{"max_bytes":128}}}])).unwrap();
    extract(&mut d).template.digest = p.id().unwrap();
    let e = compile_with_extraction_documents(&d, &[], &[], &[], None, &[p], &[]).unwrap_err();
    assert_eq!(e.category, C::Template);
}
