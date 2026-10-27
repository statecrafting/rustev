//! Synthetic extraction compilation, plan identities and refusals without backend calls.
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
fn valid_structure_requires_backend_and_v1_guard_is_unchanged() {
    let mut d = fixture();
    check(&d, C::NoCapableBackend, "no backend satisfies extract");
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
    check(&d, C::NoCapableBackend, "no backend satisfies extract");
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
    check(&d, C::NoCapableBackend, "no backend satisfies extract");
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
    check(&d, C::NoCapableBackend, "no backend satisfies extract");
}
#[test]
fn untrusted_projection_fails_closed_but_trusted_text_passes() {
    for p in [P::UserSupplied, P::ModelDerived] {
        let mut d = fixture();
        d.inputs[0].provenance.push(p);
        check(&d, C::InvalidExtraction, "untrusted text");
        d.inputs[0].ty = TypeDecl::Bool;
        check(&d, C::NoCapableBackend, "no backend satisfies extract");
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
        "no backend satisfies extract"
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
        let mut model_input = d.inputs[0].clone();
        model_input.name = "flag".into();
        model_input.ty = TypeDecl::Bool;
        model_input.provenance = vec![P::ModelDerived];
        d.inputs.push(model_input);
        accept(&mut d, "input:flag");
        extract(&mut d).projection = projection;
        d.limits.max_projection_bytes = 127;
        check(&d, C::Budget, "max_projection_bytes");
    }
}
#[test]
fn accepted_input_projection_preserves_untrusted_text() {
    let mut d = fixture();
    accept(&mut d, "input:message");
    let e = refusal(&d, &[]);
    assert_eq!(e.category, C::InvalidAcceptance);
    assert_eq!(e.subject, "step:window_ok");
    d.inputs[0].provenance.push(P::ModelDerived);
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
        "no backend satisfies extract"
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
        (
            json!("input:message"),
            "{{param.n}}",
            C::UnacceptedModelValue,
        ),
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
    check(&d, C::NoCapableBackend, "no backend satisfies extract");
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
    assert_eq!(e.detail, "no backend satisfies extract");
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
    assert_eq!(e.detail, "no backend satisfies extract");
    d.inputs[0].ty = TypeDecl::Text { max_bytes: 128 };
    p.params =
        serde_json::from_value(json!([{"name":"n","type":{"text":{"max_bytes":128}}}])).unwrap();
    extract(&mut d).template.digest = p.id().unwrap();
    let e = compile_with_extraction_documents(&d, &[], &[], &[], None, &[p], &[]).unwrap_err();
    assert_eq!(e.category, C::Template);
}

fn descriptor() -> rustev_contract::descriptor::BackendDescriptor {
    use rustev_contract::{
        definition::Operation,
        descriptor::{OutputKind, TextStructure},
    };
    let mut b = common::linear_head();
    b.operations = vec![common::support(
        Operation::Extract,
        OutputKind::TypedRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 65536,
            max_options: 32,
            option_logprobs: false,
        },
        32,
    )];
    b
}
fn compiled(
    d: &Definition,
    p: &PromptTemplate,
    b: &rustev_contract::descriptor::BackendDescriptor,
    rules: &[ExtractRules],
) -> Result<rustev_core::Compiled, Refusal> {
    compile_with_extraction_documents(
        d,
        std::slice::from_ref(b),
        &[],
        &[],
        None,
        std::slice::from_ref(p),
        rules,
    )
}
#[test]
fn extraction_plan_identity_and_reload() {
    use rustev_contract::{
        Document, extraction_results::BoundExtractionFallback, plan::PlanStepDetail,
    };
    let mut d = fixture();
    fallback(&mut d);
    let mut vocabulary = d.inputs[0].clone();
    vocabulary.name = "feature_vocabulary".into();
    vocabulary.ty = TypeDecl::List {
        item: Box::new(TypeDecl::Text { max_bytes: 64 }),
        max_items: 10,
    };
    d.inputs.push(vocabulary);
    extract(&mut d).vocabularies.insert(
        "feature_vocabulary".into(),
        V::Input {
            input: "input:feature_vocabulary".into(),
        },
    );
    let p = prompt();
    let b = descriptor();
    let rules = program();
    let c = compiled(&d, &p, &b, std::slice::from_ref(&rules)).unwrap();
    assert_eq!(c.plan.schema, "rustev.plan/3");
    assert_eq!(
        c.plan.steps[0].derivation,
        rustev_contract::judgment::Derivation::ModelDerived
    );
    let PlanStepDetail::Extract(e) = &c.plan.steps[0].detail else {
        panic!()
    };
    assert_eq!(e.template_id, p.id().unwrap());
    assert_eq!(e.template, p);
    assert_eq!(e.descriptor, b.id().unwrap());
    let BoundExtractionFallback::Bound {
        identity, program, ..
    } = &e.fallback
    else {
        panic!()
    };
    assert_eq!(*identity, rules.id().unwrap());
    assert_eq!(program.canonical().unwrap(), rules.canonical().unwrap());
    assert_eq!(
        rustev_core::Compiled::load(&c.plan, std::slice::from_ref(&b), &[])
            .unwrap()
            .id,
        c.id
    );
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/extraction.plan.json");
    let id_path = path.with_extension("id");
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, c.canonical()).unwrap();
        std::fs::write(&id_path, c.id.to_string()).unwrap();
    }
    assert_eq!(std::fs::read(path).unwrap(), c.canonical());
    assert_eq!(std::fs::read_to_string(id_path).unwrap(), c.id.to_string());
    for change in 0..5 {
        let mut d = d.clone();
        let mut p = p.clone();
        let mut b = b.clone();
        let mut rules = rules.clone();
        match change {
            0 => {
                p.instructions.push('!');
                extract(&mut d).template.digest = p.id().unwrap();
            }
            1 => extract(&mut d).output.fields[1].ty = T::Integer { bounds: [1, 31] },
            2 => {
                rules.fields[0].rules[0].then =
                    serde_json::from_value(json!({"literal":3})).unwrap()
            }
            3 => b.artifact = common::artifact('b'),
            _ => extract(&mut d).fallback = Fallback::None,
        }
        assert_ne!(compiled(&d, &p, &b, &[rules]).unwrap().id, c.id);
    }
    let snapshot = rustev_contract::snapshot::Snapshot {
        schema: "rustev.snapshot/2".into(),
        entries: vec![],
    };
    assert!(rustev_core::Evaluation::start(&c, &snapshot, common::ts(common::NOW)).is_ok());
}
#[test]
fn extraction_backend_shortfalls_and_stable_binding() {
    use rustev_contract::descriptor::OutputKind;
    let d = fixture();
    let p = prompt();
    let b = descriptor();
    for change in 0..7 {
        let mut b = b.clone();
        let mut d = d.clone();
        match change {
            0 => b.operations.clear(),
            1 => b.operations[0].max_options = 1,
            2 => {
                if let OutputKind::TypedRecord {
                    max_output_bytes, ..
                } = &mut b.operations[0].output
                {
                    *max_output_bytes = 1;
                }
            }
            3 => b.input_limit.max_bytes = 1,
            4 => {
                extract(&mut d).backend.artifact =
                    rustev_contract::definition::ArtifactPin::Pinned(common::artifact('b'))
            }
            5 => {
                extract(&mut d).backend.binding =
                    rustev_contract::definition::BindingChoice::Backend("unknown".into())
            }
            _ => {
                if let OutputKind::TypedRecord { max_options, .. } = &mut b.operations[0].output {
                    *max_options = 1;
                }
            }
        }
        let r = compiled(&d, &p, &b, &[]).unwrap_err();
        assert_eq!(r.category, C::NoCapableBackend, "case {change}: {r}");
        assert!(!r.shortfalls[0].reasons.is_empty());
    }
    let mut other = b.clone();
    other.backend_id = "zz-last".into();
    let a = compile_with_extraction_documents(
        &d,
        &[other.clone(), b.clone()],
        &[],
        &[],
        None,
        std::slice::from_ref(&p),
        &[],
    )
    .unwrap();
    let b = compile_with_extraction_documents(&d, &[b, other], &[], &[], None, &[p], &[]).unwrap();
    assert_eq!(a.id, b.id);
}
#[test]
fn model_gate_exact_policy_proposal_and_presence_reads() {
    for r in ["input:message", "step:request_filters/party_size"] {
        for consumer in 0..3 {
            let mut d = fixture();
            d.inputs[0].ty = TypeDecl::Integer;
            d.inputs[0].provenance = vec![P::ModelDerived];
            let mut default = d.policy.rules[0].clone();
            default.id = "last".into();
            d.policy.rules.push(default);
            match consumer {
                0 => d.steps.push(serde_json::from_value(json!({"id":"read","body":{"exact":{"op":"arith","version":1,"args":{"expr":{"ref":r}}}}})).unwrap()),
                1 => d.policy.rules[0].when = serde_json::from_value(json!({"exact":{"cmp":{"left":{"ref":r},"op":"eq","right":{"lit":{"integer":1}}}}})).unwrap(),
                _ => d.policy.rules[0].then = serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"value","value":{"ref":r}}]}})).unwrap(),
            }
            let e = compiled(&d, &prompt(), &descriptor(), &[]).unwrap_err();
            assert_eq!(e.category as u8, 27, "{e}");
            assert!(e.detail.contains(r));
        }
    }
    let mut d = fixture();
    d.inputs[0].ty = TypeDecl::Integer;
    d.inputs[0].provenance = vec![P::ModelDerived];
    let mut default = d.policy.rules[0].clone();
    default.id = "last".into();
    d.policy.rules.push(default);
    d.policy.rules[0].when =
        serde_json::from_value(json!({"exact":{"present":"input:message"}})).unwrap();
    assert_ne!(
        compiled(&d, &prompt(), &descriptor(), &[])
            .unwrap_err()
            .category,
        C::UnacceptedModelValue
    );
    d.policy.rules[0].then = serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"value","value":{"lit":{"text":"input:message"}}}]}})).unwrap();
    assert_ne!(
        compiled(&d, &prompt(), &descriptor(), &[])
            .unwrap_err()
            .category,
        C::UnacceptedModelValue
    );
}

#[test]
fn ungrouped_check_accepts_model_input_by_branch_or_default() {
    for on_fail in [
        json!({"by_provenance":{"model-derived":"abstain"},"default":"none"}),
        json!({"by_provenance":{},"default":"abstain"}),
    ] {
        let mut d = fixture();
        d.inputs[0].ty = TypeDecl::Integer;
        d.inputs[0].provenance = vec![P::ModelDerived];
        d.steps.push(serde_json::from_value(json!({"id":"read","body":{"exact":{"op":"arith","version":1,"args":{"expr":{"ref":"input:message"}}}}})).unwrap());
        assert_eq!(
            compiled(&d, &prompt(), &descriptor(), &[])
                .unwrap_err()
                .category,
            C::UnacceptedModelValue
        );
        d.actionability = Some(serde_json::from_value(json!({
            "caller_controlled":[],"remediations":[],"questions":[],
            "constraint_groups":[],
            "checks":[{"id":"guard","subject":"message",
                "test":{"cmp":{"left":{"ref":"input:message"},"op":"gt","right":{"lit":{"integer":0}}}},
                "on_fail":on_fail}]
        })).unwrap());
        compiled(&d, &prompt(), &descriptor(), &[]).unwrap();
        // Extraction execution is deferred in this increment. Keep the exact
        // consumer to verify the existing check evaluator independently.
        d.steps.remove(0);
        let c = compiled(&d, &prompt(), &descriptor(), &[]).unwrap();
        // Spec 033 3.4.1 and 3.4.6: even outside any group, a failed
        // terminal check prevents the exact read and extraction request.
        let snapshot = serde_json::from_value(json!({"schema":"rustev.snapshot/2","entries":[{
            "field":"message","value":-1,"provenance":"model-derived",
            "as_of_ms":1800000000000i64,"source":"synthetic"
        }]}))
        .unwrap();
        let ev = rustev_core::Evaluation::start(
            &c,
            &snapshot,
            rustev_contract::time::Timestamp::from_ms(1800000000000).unwrap(),
        )
        .unwrap();
        assert!(ev.pending().is_empty());
        let (judgment, _) = ev.finish("ungrouped").unwrap();
        assert!(
            matches!(judgment.outcome.unresolved(), Some(rustev_contract::judgment::Unresolved::Abstained { rule }) if rule == "guard")
        );
    }
}

#[test]
fn protected_constraint_and_model_projections_are_gated() {
    let mut d = fixture();
    d.inputs[0].ty = TypeDecl::Integer;
    d.inputs[0].provenance = vec![P::ModelDerived];
    let mut other = d.inputs[0].clone();
    other.name = "other".into();
    other.provenance = vec![P::SystemOfRecord];
    d.inputs.push(other);
    d.actionability = Some(serde_json::from_value(json!({"caller_controlled":[],"remediations":[],"questions":[],
        "constraint_groups":[{"name":"protected","checks":["guard"]}],
        "checks":[{"id":"guard","subject":"other","test":{"cmp":{"left":{"ref":"input:message"},"op":"eq","right":{"lit":{"integer":1}}}},
        "on_fail":{"by_provenance":{},"default":"none"}}]})).unwrap());
    let e = compiled(&d, &prompt(), &descriptor(), &[]).unwrap_err();
    assert_eq!(e.category as u8, 27, "{e}");
    assert_eq!(e.subject, "check:guard");
    for kind in 0..3 {
        let mut d = fixture();
        let mut consumer = if kind == 0 {
            d.steps[0].clone()
        } else if kind == 1 {
            common::support_routing()
                .steps
                .into_iter()
                .find(|s| matches!(s.body, StepBody::Semantic(_)))
                .unwrap()
        } else {
            let plan: rustev_contract::plan::Plan =
                serde_json::from_str(include_str!("golden/generation.plan.json")).unwrap();
            plan.definition.steps[0].clone()
        };
        consumer.id = "consumer".into();
        match &mut consumer.body {
            StepBody::Extract(e) => e.projection = vec!["step:request_filters".into()],
            StepBody::Semantic(s) => {
                s.project = vec!["step:request_filters/party_size".into()];
                s.calibration = rustev_contract::definition::CalibrationRef::None;
                s.requires = rustev_contract::definition::RequiredKind::Distribution;
            }
            StepBody::Generate(g) => {
                g.projection = vec!["step:request_filters/party_size".into()];
                g.template = extract(&mut d).template.clone();
            }
            _ => unreachable!(),
        }
        d.steps.push(consumer);
        let e = compiled(&d, &prompt(), &descriptor(), &[]).unwrap_err();
        assert_eq!(e.category as u8, 27, "kind {kind}: {e}");
    }
}

#[test]
fn extraction_threshold_requires_option_logprobs() {
    let mut d = fixture();
    let mut last = d.policy.rules[0].clone();
    last.id = "last".into();
    d.policy.rules.push(last);
    d.policy.rules[0].when = serde_json::from_value(json!({"mass_at_least":{"step":"request_filters/insufficient","option":"true","threshold":"0.5","uncalibrated_threshold":{"declared":{"reason":"synthetic"}}}})).unwrap();
    let mut b = descriptor();
    let e = compiled(&d, &prompt(), &b, &[]).unwrap_err();
    assert_eq!(e.category, C::KindMismatch, "{e}");
    assert!(
        e.shortfalls[0]
            .reasons
            .contains(&rustev_core::compile::ShortfallReason::OptionLogprobs)
    );
    if let rustev_contract::descriptor::OutputKind::TypedRecord {
        option_logprobs, ..
    } = &mut b.operations[0].output
    {
        *option_logprobs = true;
    }
    assert_eq!(
        compiled(&d, &prompt(), &b, &[]).unwrap_err().category,
        C::UnhandledUnresolved
    );
    d.policy.on_unresolved = serde_json::from_value(json!([{"target":"step:request_filters","reasons":"any","action":{"escalate":{"reason":"missing extraction"}}}])).unwrap();
    let c = compiled(&d, &prompt(), &b, &[]).unwrap();
    assert_eq!(
        rustev_core::Compiled::load(&c.plan, &[b], &[]).unwrap().id,
        c.id
    );
}

#[test]
fn missing_fallback_program_is_refused_with_a_capable_backend() {
    let mut d = fixture();
    fallback(&mut d);
    let r = compiled(&d, &prompt(), &descriptor(), &[]).unwrap_err();
    assert_eq!(r.category, C::InvalidExtraction);
    assert_eq!(r.subject, "step:request_filters");
    assert_eq!(r.detail, "fallback program not supplied");
    assert!(compiled(&d, &prompt(), &descriptor(), &[program()]).is_ok());
}

fn mass_condition(d: &mut Definition, option: &str) {
    d.policy.rules[0].when = serde_json::from_value(json!({"mass_at_least":{
        "step":"request_filters/insufficient", "option":option, "threshold":"0.5",
        "uncalibrated_threshold":{"declared":{"reason":"synthetic"}}
    }}))
    .unwrap();
}

fn distribution_fixture() -> (Definition, rustev_contract::descriptor::BackendDescriptor) {
    let mut d = fixture();
    let mut last = d.policy.rules[0].clone();
    last.id = "last".into();
    d.policy.rules.push(last);
    d.policy.on_unresolved = serde_json::from_value(json!([{"target":"step:request_filters","reasons":"any","action":{"escalate":{"reason":"missing extraction"}}}])).unwrap();
    let mut b = descriptor();
    if let rustev_contract::descriptor::OutputKind::TypedRecord {
        option_logprobs, ..
    } = &mut b.operations[0].output
    {
        *option_logprobs = true;
    }
    (d, b)
}

#[test]
fn extraction_mass_options_are_checked_separately_from_fan_out_binds() {
    for enum_field in [false, true] {
        let (mut d, b) = distribution_fixture();
        let declared = if enum_field {
            extract(&mut d).output.fields.last_mut().unwrap().ty = T::Enum {
                vocabulary: "feature_vocabulary".into(),
            };
            "pool"
        } else {
            "true"
        };
        mass_condition(&mut d, "undeclared");
        let r = compiled(&d, &prompt(), &b, &[]).unwrap_err();
        assert_eq!(r.category, C::InvalidDefinition);
        assert!(r.detail.contains("is not an option"), "{r}");
        mass_condition(&mut d, declared);
        assert!(compiled(&d, &prompt(), &b, &[]).is_ok());
    }
}

#[test]
fn dynamic_list_vocabulary_mass_condition_is_conservatively_refused() {
    let (mut d, b) = distribution_fixture();
    extract(&mut d).output.fields.last_mut().unwrap().ty = T::Enum {
        vocabulary: "feature_vocabulary".into(),
    };
    input_vocabulary(
        &mut d,
        TypeDecl::List {
            item: Box::new(TypeDecl::Text { max_bytes: 64 }),
            max_items: 10,
        },
    );
    // Input lists resolve at evaluation (038 3.3.2); this increment does not
    // invent a static option set or admit policy thresholds over that set.
    mass_condition(&mut d, "pool");
    let r = compiled(&d, &prompt(), &b, &[]).unwrap_err();
    assert_eq!(r.category, C::InvalidDefinition);
    assert!(r.detail.contains("is not a semantic step"), "{r}");
    d.policy.rules.remove(0);
    assert!(compiled(&d, &prompt(), &b, &[]).is_ok());
}

#[cfg(feature = "parameterized")]
#[test]
fn profile_distribution_options_are_checked_after_instantiation() {
    let (mut d, b) = distribution_fixture();
    extract(&mut d).output.fields.last_mut().unwrap().ty = T::Enum {
        vocabulary: "feature_vocabulary".into(),
    };
    mass_condition(&mut d, "pool");
    let mut source = serde_json::to_value(d).unwrap();
    source["parameters"] = json!([{"name":"options","type":{"list":{"item":{"text":{"max_bytes":64}},"max_items":256}},"lock":"free","bounds":"none","default":"none"}]);
    source["steps"][0]["body"]["extract"]["vocabularies"]["feature_vocabulary"] =
        json!({"param":"options"});
    let profile = json!({"schema":"rustev.profile/1","profile":"test","version":"1.0.0","definition":{"name":source["name"],"version":source["version"]},"extends":"none","values":{"options":["pool"]}});
    for option in ["pool", "undeclared"] {
        source["policy"]["rules"][0]["when"]["mass_at_least"]["option"] = json!(option);
        let bytes = serde_json::to_vec(&source).unwrap();
        let instantiated = rustev_core::instantiate::instantiate(
            &[&bytes],
            Some(&serde_json::to_vec(&profile).unwrap()),
            &[],
            None,
            None,
        )
        .unwrap();
        let result = compiled(&instantiated.definition, &prompt(), &b, &[]);
        if option == "pool" {
            assert!(result.is_ok(), "{result:?}");
        } else {
            let r = result.unwrap_err();
            assert_eq!(r.category, C::InvalidDefinition);
            assert!(r.detail.contains("is not an option"), "{r}");
        }
    }
}

#[test]
fn long_input_enum_option_refuses_primary_and_execution_fallback() {
    use rustev_contract::{
        descriptor::OutputKind,
        execution::{CostPolicy, Delay, FailureClass},
    };
    use rustev_core::compile::ShortfallReason;
    let mut d = fixture();
    input_vocabulary(
        &mut d,
        TypeDecl::Enum {
            options: vec!["\u{0001}".repeat(200)],
        },
    );
    extract(&mut d).output.fields = vec![rustev_contract::extraction::ExtractionField {
        name: "value".into(),
        ty: T::Enum {
            vocabulary: "feature_vocabulary".into(),
        },
    }];
    let mut limited = descriptor();
    if let OutputKind::TypedRecord {
        max_output_bytes, ..
    } = &mut limited.operations[0].output
    {
        *max_output_bytes = 600;
    }
    // The old bound was 422 bytes; escaping the declared option needs 1238.
    let r = compiled(&d, &prompt(), &limited, &[]).unwrap_err();
    assert_eq!(r.category, C::NoCapableBackend);
    let check = |r: &Refusal| {
        assert!(
            r.shortfalls
                .iter()
                .flat_map(|s| &s.reasons)
                .any(|s| matches!(
                    s,
                    ShortfallReason::OutputLimit {
                        max: 600,
                        needed: 1238
                    }
                ))
        )
    };
    check(&r);
    limited.backend_id = "zz-fallback".into();
    let execution = common::execution(
        CostPolicy::Unlimited,
        vec![common::step_exec(
            "request_filters",
            1,
            &[],
            Delay::None,
            &["zz-fallback"],
            &[FailureClass::Permanent],
        )],
    );
    let r = compile_with_extraction_documents(
        &d,
        &[descriptor(), limited],
        &[],
        &[],
        Some(&execution),
        &[prompt()],
        &[],
    )
    .unwrap_err();
    assert_eq!(r.category, C::InvalidExecution);
    check(&r);
}

fn acceptance_fixture() -> Definition {
    let mut d = fixture();
    d.steps.push(
        serde_json::from_str(include_str!(
            "../../rustev-contract/tests/fixtures/accept-step.json"
        ))
        .unwrap(),
    );
    d
}
fn acceptance(d: &mut Definition) -> &mut rustev_contract::extraction::AcceptModelDerived {
    let StepBody::AcceptModelDerived(a) = &mut d.steps[1].body else {
        panic!()
    };
    a
}
fn logprob_descriptor() -> rustev_contract::descriptor::BackendDescriptor {
    let mut b = descriptor();
    if let rustev_contract::descriptor::OutputKind::TypedRecord {
        option_logprobs, ..
    } = &mut b.operations[0].output
    {
        *option_logprobs = true;
    }
    b
}
fn threshold() -> rustev_contract::extraction::MinMass {
    serde_json::from_value(
        json!({"option":"true","at_least":"0.5","basis":{"uncalibrated_threshold":"synthetic"}}),
    )
    .unwrap()
}
#[test]
fn acceptance_example_golden_and_member_identity() {
    use rustev_contract::{Document, plan::PlanStepDetail};
    let d = acceptance_fixture();
    let c = compiled(&d, &prompt(), &descriptor(), &[]).unwrap();
    assert_eq!(c.plan.schema, "rustev.plan/3");
    assert_eq!(c.plan.registry, "rustev.exact/2");
    assert_eq!(
        c.plan.steps[1].derivation,
        rustev_contract::judgment::Derivation::ModelDerived
    );
    assert!(
        matches!(&c.plan.steps[1].detail, PlanStepDetail::AcceptModelDerived(a) if **a == *acceptance(&mut d.clone()))
    );
    assert_eq!(
        rustev_core::Compiled::load(&c.plan, &[descriptor()], &[])
            .unwrap()
            .id,
        c.id
    );
    assert!(
        rustev_core::Evaluation::start(
            &c,
            &rustev_contract::snapshot::Snapshot {
                schema: "rustev.snapshot/2".into(),
                entries: vec![]
            },
            common::ts(common::NOW)
        )
        .is_ok()
    );
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/acceptance.plan.json");
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, c.canonical()).unwrap();
        std::fs::write(path.with_extension("id"), c.id.to_string()).unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), c.canonical());
    assert_eq!(
        std::fs::read_to_string(path.with_extension("id")).unwrap(),
        c.id.to_string()
    );
    assert_eq!(
        rustev_contract::plan::Plan::parse(&c.canonical()).unwrap(),
        c.plan
    );
    let mut mass = d.clone();
    acceptance(&mut mass).field = "step:request_filters/insufficient".into();
    acceptance(&mut mass).validate = rustev_contract::extraction::Validation::None;
    let base = compiled(&mass, &prompt(), &logprob_descriptor(), &[]).unwrap();
    for change in 0..4 {
        let mut d = mass.clone();
        match change {
            0 => acceptance(&mut d).field = "step:request_filters/party_size".into(),
            1 => acceptance(&mut d).validate = serde_json::from_value(json!({"predicate":{"cmp":{"left":{"ref":"step:request_filters/insufficient"},"op":"eq","right":{"lit":{"bool":false}}}}})).unwrap(),
            2 => acceptance(&mut d).min_mass = threshold(),
            _ => acceptance(&mut d).on_fail = rustev_contract::extraction::OnFail::Unresolved,
        }
        assert_ne!(
            compiled(&d, &prompt(), &logprob_descriptor(), &[])
                .unwrap()
                .id,
            base.id,
            "member {change}"
        );
    }
}
#[test]
fn acceptance_refuses_each_invalid_member_and_validation_cycle() {
    for case in 0..8 {
        let mut d = acceptance_fixture();
        match case {
            0 => acceptance(&mut d).field = "input:message".into(),
            1 => acceptance(&mut d).field = "step:request_filters/missing".into(),
            2 => acceptance(&mut d).validate = serde_json::from_value(json!({"predicate":{"cmp":{"left":{"ref":"step:request_filters/party_size"},"op":"gt","right":{"lit":{"integer":0}}}}})).unwrap(),
            3 => acceptance(&mut d).validate = serde_json::from_value(json!({"predicate":{"cmp":{"left":{"ref":"step:request_filters/window/start"},"op":"gt","right":{"lit":{"bool":true}}}}})).unwrap(),
            4 => acceptance(&mut d).min_mass = threshold(),
            5 => {
                acceptance(&mut d).field = "step:request_filters/insufficient".into();
                acceptance(&mut d).validate = rustev_contract::extraction::Validation::None;
                let mut m = threshold();
                if let rustev_contract::extraction::MinMass::Threshold(m) = &mut m { m.option = "unknown".into(); }
                acceptance(&mut d).min_mass = m;
            }
            6 => acceptance(&mut d).on_fail = rustev_contract::extraction::OnFail::DropField { notice: "INVALID".into() },
            _ => acceptance(&mut d).field = "step:missing/window".into(),
        }
        let e = compiled(&d, &prompt(), &descriptor(), &[]).unwrap_err();
        assert_eq!(e.category, C::InvalidAcceptance, "case {case}: {e}");
        assert_eq!(e.subject, "step:window_ok");
    }
    let mut d = acceptance_fixture();
    let mut other = d.steps[1].clone();
    other.id = "other_ok".into();
    d.steps.push(other);
    for (index, r) in [(1, "step:other_ok/start"), (2, "step:window_ok/start")] {
        let StepBody::AcceptModelDerived(a) = &mut d.steps[index].body else {
            panic!()
        };
        a.validate = serde_json::from_value(
            json!({"predicate":{"cmp":{"left":{"ref":r},"op":"gt","right":{"ref":"now"}}}}),
        )
        .unwrap();
    }
    assert_eq!(
        compiled(&d, &prompt(), &descriptor(), &[])
            .unwrap_err()
            .category,
        C::Cycle
    );
}
#[test]
fn accepted_paths_feed_exact_consumers_and_model_projections() {
    let mut d = acceptance_fixture();
    d.steps.push(serde_json::from_value(json!({"id":"read","body":{"exact":{"op":"timestamp_diff","version":1,"args":{"later":{"ref":"step:window_ok/start"},"earlier":{"ref":"now"},"unit_ms":1}}}})).unwrap());
    let c = compiled(&d, &prompt(), &descriptor(), &[]).unwrap();
    assert_eq!(
        c.plan.steps[2].derivation,
        rustev_contract::judgment::Derivation::MixedDerived
    );
    let mut other = d.steps[0].clone();
    other.id = "again".into();
    let StepBody::Extract(e) = &mut other.body else {
        panic!()
    };
    e.projection = vec!["step:window_ok/start".into()];
    d.steps.push(other);
    let wire = serde_json::to_value(&d).unwrap();
    let d: Definition = serde_json::from_value(wire).unwrap();
    compiled(&d, &prompt(), &descriptor(), &[]).unwrap();
    let mut direct = serde_json::to_value(&d).unwrap();
    direct["steps"][3]["body"]["extract"]["projection"] =
        json!(["step:request_filters/window/start"]);
    let direct: Definition = serde_json::from_value(direct).unwrap();
    assert_eq!(
        compiled(&direct, &prompt(), &descriptor(), &[])
            .unwrap_err()
            .category,
        C::UnacceptedModelValue
    );
}
#[test]
fn accepted_value_cannot_control_selection_weights() {
    let mut d: Definition =
        serde_json::from_str(include_str!("golden/lodging.definition.json")).unwrap();
    d.schema = "rustev.definition/2".into();
    d.inputs
        .iter_mut()
        .find(|i| i.name == "traveler.claims")
        .unwrap()
        .provenance = vec![P::ModelDerived];
    d.steps.push(serde_json::from_value(json!({"id":"accepted","body":{"accept_model_derived":{"field":"input:traveler.claims","validate":"none","min_mass":"none","on_fail":"unresolved"}}})).unwrap());
    // Replace every read, including the weights table, with the accepted value.
    let wire = serde_json::to_string(&d)
        .unwrap()
        .replace("input:traveler.claims", "step:accepted");
    let mut wire: serde_json::Value = serde_json::from_str(&wire).unwrap();
    wire["steps"].as_array_mut().unwrap().last_mut().unwrap()["body"]["accept_model_derived"]["field"] =
        json!("input:traveler.claims");
    let d: Definition = serde_json::from_value(wire).unwrap();
    let e = rustev_core::compile(&d, &common::descriptors(), &[]).unwrap_err();
    assert_eq!(e.category, C::UnacceptedModelValue, "{e}");
    assert!(e.detail.contains("policy_control"), "{e}");
}

#[test]
fn acceptance_mass_calibration_binding_and_uncalibrated_notice() {
    use rustev_contract::{
        calibration::CalibrationArtifact,
        extraction::{MassBasis, MinMass},
    };
    let mut d = acceptance_fixture();
    acceptance(&mut d).field = "step:request_filters/insufficient".into();
    acceptance(&mut d).validate = rustev_contract::extraction::Validation::None;
    acceptance(&mut d).min_mass = threshold();
    let b = logprob_descriptor();
    let c = compiled(&d, &prompt(), &b, &[]).unwrap();
    assert!(c.plan.notices.iter().any(|n| n.reason == "synthetic"));
    let art: CalibrationArtifact = serde_json::from_value(json!({"schema":"rustev.calibration/1",
        "binding":{"artifact":b.artifact,"task":format!("filters@{}", prompt().id().unwrap()),"question":"request_filters/insufficient","dataset":format!("sha256:{}","c".repeat(64)),"method":"temperature/1"},
        "options":["false","true"],"parameters":{"temperature":{"temperature":"1"}}})).unwrap();
    let run = |d: &Definition, arts: &[CalibrationArtifact]| {
        compile_with_extraction_documents(
            d,
            std::slice::from_ref(&b),
            arts,
            &[],
            None,
            &[prompt()],
            &[],
        )
    };
    for case in 0..6 {
        let mut art = art.clone();
        match case {
            1 => art.binding.task = "wrong".into(),
            2 => art.binding.question = "wrong".into(),
            3 => art.options.reverse(),
            4 => art.binding.artifact = common::artifact('b'),
            5 => {
                art.parameters =
                    serde_json::from_value(json!({"temperature":{"temperature":"0"}})).unwrap()
            }
            _ => {}
        }
        let MinMass::Threshold(m) = &mut acceptance(&mut d).min_mass else {
            panic!()
        };
        m.basis = MassBasis::Calibration(art.id().unwrap().to_string());
        assert_eq!(run(&d, &[]).unwrap_err().category, C::CalibrationBinding);
        let result = run(&d, std::slice::from_ref(&art));
        if case == 0 {
            let c = result.unwrap();
            assert_eq!(
                rustev_core::Compiled::load(&c.plan, std::slice::from_ref(&b), &[art])
                    .unwrap()
                    .id,
                c.id
            );
        } else {
            assert_eq!(
                result.unwrap_err().category,
                C::CalibrationBinding,
                "case {case}"
            );
        }
    }
    acceptance(&mut d).min_mass = threshold();
    let MinMass::Threshold(m) = &mut acceptance(&mut d).min_mass else {
        panic!()
    };
    m.basis = MassBasis::UncalibratedThreshold(String::new());
    assert_eq!(run(&d, &[]).unwrap_err().category, C::UncalibratedThreshold);
}

fn ranking_fixture() -> (Definition, rustev_contract::descriptor::BackendDescriptor) {
    let mut d = fixture();
    d.limits.max_semantic_requests = 64;
    d.inputs.push(serde_json::from_value(json!({"name":"items","type":{"list":{"item":{"record":{"fields":[{"name":"id","type":{"text":{"max_bytes":8}}}]}},"max_items":5}},"provenance":["system-of-record"],"freshness":"not_required"})).unwrap());
    let steps: Vec<rustev_contract::definition::StepDecl> = serde_json::from_value(json!([
        {"id":"pool","body":{"exact":{"op":"filter_with_reasons","version":1,"args":{"list":"input:items","id_field":"id","rules":[]}}}},
        {"id":"ranked","body":{"rank":{"task":"fit","question":"Which fits?","candidates":"step:pool","items":"none","bind":"candidate","project":[],"mode":{"listwise":{"scores":"optional"}},"bias":"none","tie_break":"id_ascending","backend":{"min_determinism":"unspecified","artifact":"any","binding":"auto"},"fallback":"none"}}},
        {"id":"chosen","body":{"select":{"from":"step:ranked","k":3,"backfill":{"from":"step:pool","exclude_selected":true},"on_backend_failure":"backfill_all"}}}
    ])).unwrap();
    d.steps.extend(steps);
    let mut b = descriptor();
    b.ranking = Some(serde_json::from_value(json!({"modes":["listwise"],"max_candidates":1024,"listwise_scores":"optional","listwise_partial":"possible"})).unwrap());
    (d, b)
}

#[test]
fn rank_projection_refuses_unaccepted_extract_field() {
    let (mut d, b) = ranking_fixture();
    let StepBody::Rank(r) = &mut d.steps[2].body else {
        panic!()
    };
    r.project = vec!["step:request_filters/party_size".into()];
    let e = compiled(&d, &prompt(), &b, &[]).unwrap_err();
    assert_eq!(e.category, C::UnacceptedModelValue);
    assert_eq!(e.subject, "step:ranked");
    assert!(e.detail.contains("step:request_filters/party_size"));
}

#[test]
fn rank_projection_accepts_checked_model_input() {
    let (mut d, b) = ranking_fixture();
    d.inputs[0].ty = TypeDecl::Integer;
    d.inputs[0].provenance = vec![P::ModelDerived];
    d.actionability = Some(serde_json::from_value(json!({
        "caller_controlled":[],"remediations":[],"questions":[],"constraint_groups":[],
        "checks":[{"id":"guard","subject":"message",
            "test":{"cmp":{"left":{"ref":"input:message"},"op":"gt","right":{"lit":{"integer":0}}}},
            "on_fail":{"by_provenance":{"model-derived":"abstain"},"default":"none"}}]
    })).unwrap());
    let StepBody::Rank(r) = &mut d.steps[2].body else {
        panic!()
    };
    r.project = vec!["input:message".into()];
    compiled(&d, &prompt(), &b, &[]).unwrap();
}

#[test]
fn rank_projection_accepts_accepted_extract_field() {
    let (mut d, b) = ranking_fixture();
    d.steps.insert(
        1,
        serde_json::from_str(include_str!(
            "../../rustev-contract/tests/fixtures/accept-step.json"
        ))
        .unwrap(),
    );
    let StepBody::Rank(r) = &mut d.steps[3].body else {
        panic!()
    };
    r.project = vec!["step:window_ok/start".into()];
    compiled(&d, &prompt(), &b, &[]).unwrap();
}

#[test]
fn ranking_exact_order_sources_refuse_unaccepted_extract_fields() {
    for consumer in ["order_of", "from", "backfill"] {
        let (mut d, b) = ranking_fixture();
        let source = "step:request_filters/party_size".to_string();
        let expected = if consumer == "order_of" {
            let StepBody::Rank(r) = &mut d.steps[2].body else {
                panic!()
            };
            r.tie_break = rustev_contract::ranking::TieBreak::OrderOf(source.clone());
            "step:ranked"
        } else {
            let StepBody::Select(s) = &mut d.steps[3].body else {
                panic!()
            };
            if consumer == "from" {
                s.from = source.clone();
            } else {
                s.backfill = rustev_contract::ranking::Backfill::From {
                    from: source.clone(),
                    exclude_selected: true,
                };
            }
            "step:chosen"
        };
        let e = compiled(&d, &prompt(), &b, &[]).unwrap_err();
        assert_eq!(e.category, C::UnacceptedModelValue, "{consumer}: {e}");
        assert_eq!(e.subject, expected);
        assert!(e.detail.contains(&source));
    }
}
