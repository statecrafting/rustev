//! Spec 070 increment 3, 3.7 and 3.9: `fill: rules_first` evaluates the rules
//! before the request is listed, skips it when every required field is
//! filled, and otherwise requests only the fields the rules left absent.
mod common;
use rustev_contract::{
    Document,
    definition::{Definition, Operation, ProvenanceClass as P, StepBody},
    descriptor::{BackendDescriptor, OutputKind, TextStructure},
    evidence::EvidenceRecord,
    extraction::{AbsenceReason, Fallback, RequestFailure},
    extraction_documents::{ExtractRequest, ExtractRules, extraction_schema_digest},
    extraction_results::{ExtractionOrigin, ExtractionRequestOutcome, SkipReason},
    judgment::Unresolved,
    output::RawOutput,
    plan::{Plan, PlanStepDetail},
};
use rustev_core::{
    Category, Compiled, Evaluation, Supplied, compile::compile_with_extraction_documents,
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

/// Rules for `party_size`, `area_name` and `window`; `features` and
/// `insufficient` are never filled by the rules.
fn program() -> ExtractRules {
    serde_json::from_value(
        json!({"schema":"rustev.extract-rules/2","name":"booking","version":"1",
        "sources":[{"name":"text","ref":"input:message"}],
        "fields":[
            {"field":"party_size","rules":[{"name":"party","when":"always",
                "then":{"integer_after":{"source":"text","terms":["party"],"max_digits":2}}}]},
            {"field":"area_name","rules":[{"name":"area","when":"always",
                "then":{"token_after":{"source":"text","terms":["area"],"charset":["upper","lower"],
                    "min_len":2,"max_len":32}}}]},
            {"field":"window","rules":[{"name":"dates","when":"always","then":{"record":{
                "start":{"date":{"source":"text","format":"dmy_slash","occurrence":1}},
                "end":{"date":{"source":"text","format":"dmy_slash","occurrence":2}}}}}]}
        ]}),
    )
    .unwrap()
}

fn definition(fallback: Value) -> Definition {
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let mut d = plan.definition;
    let StepBody::Extract(e) = &mut d.steps[0].body else {
        panic!()
    };
    e.fallback = serde_json::from_value::<Fallback>(fallback).unwrap();
    d
}

fn rules_first(required: Option<Value>) -> Definition {
    let mut f = json!({"rules_program":"booking@1","fill":"rules_first"});
    if let Some(r) = required {
        f["required"] = r;
    }
    definition(f)
}

fn compile(d: &Definition) -> Compiled {
    compile_with_extraction_documents(d, &[backend()], &[], &[], None, &[template()], &[program()])
        .unwrap()
}

fn start<'a>(c: &'a Compiled, message: &str) -> Evaluation<'a> {
    let snapshot = common::snapshot(vec![common::entry(
        "message",
        json!(message),
        P::SystemOfRecord,
        common::NOW,
    )]);
    Evaluation::start(c, &snapshot, common::ts(common::NOW)).unwrap()
}

fn output(fields: Value) -> Supplied {
    Supplied::Output(RawOutput::TypedRecord {
        fields: serde_json::from_value(fields).unwrap(),
        logprobs: vec![],
        provider_reported: BTreeMap::new(),
    })
}

const ALL: &str = "party 4 area Soho from 1/3/2027 to 4/3/2027";
const TWO: &str = "party 4 area Soho";

fn names(r: &ExtractRequest) -> Vec<&str> {
    r.output.fields.iter().map(|f| f.name.as_str()).collect()
}

fn field<'e>(
    ev: &'e EvidenceRecord,
    name: &str,
) -> &'e rustev_contract::extraction_results::ExtractionFieldEvidence {
    ev.extraction.as_ref().unwrap()[0]
        .fields
        .iter()
        .find(|f| f.name == name)
        .unwrap()
}

#[test]
fn every_required_field_filled_skips_the_request() {
    // Section 5: no request listed; outcome skipped{rules_filled}.
    let c = compile(&rules_first(Some(json!([
        "party_size",
        "area_name",
        "window"
    ]))));
    let e = start(&c, ALL);
    assert!(e.pending().is_empty());
    assert!(e.extraction_request("request_filters").is_none());
    let (_, ev) = e.finish("rules-first").unwrap();
    let x = &ev.extraction.as_ref().unwrap()[0];
    assert_eq!(
        x.request_outcome,
        ExtractionRequestOutcome::Skipped {
            reason: SkipReason::RulesFilled
        }
    );
    for name in ["party_size", "area_name", "window"] {
        assert_eq!(
            field(&ev, name).origin,
            ExtractionOrigin::Fallback,
            "{name}"
        );
    }
    // A field the rules left absent keeps its rules absence reason.
    for name in ["features", "insufficient"] {
        assert_eq!(
            field(&ev, name).origin,
            ExtractionOrigin::Absent {
                reason: AbsenceReason::FallbackNoMatch
            },
            "{name}"
        );
    }
    // Declared order is kept, and the record round-trips.
    let order: Vec<_> = x.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        order,
        [
            "window",
            "party_size",
            "features",
            "area_name",
            "insufficient"
        ]
    );
    EvidenceRecord::parse(&ev.canonical().unwrap()).unwrap();
}

#[test]
fn omitted_required_means_every_top_level_field() {
    // The rules never fill `features`, so the request is listed.
    let c = compile(&rules_first(None));
    let e = start(&c, ALL);
    assert_eq!(e.pending().len(), 1);
    let r = e.extraction_request("request_filters").unwrap();
    assert_eq!(names(r), ["features", "insufficient"]);
}

#[test]
fn a_partial_request_lists_the_absent_fields_with_a_subset_digest() {
    // Section 5: rules fill 2 of 5 fields.
    let c = compile(&rules_first(None));
    let e = start(&c, TWO);
    let r = e.extraction_request("request_filters").unwrap().clone();
    assert_eq!(names(&r), ["window", "features", "insufficient"]);
    assert_eq!(
        r.schema_digest,
        extraction_schema_digest(&r.output).unwrap()
    );
    let full = &c.plan.steps[0].detail;
    let PlanStepDetail::Extract(b) = full else {
        panic!()
    };
    assert_ne!(r.schema_digest, b.schema_digest);
    assert_eq!(
        r.vocabularies.keys().collect::<Vec<_>>(),
        ["feature_vocabulary"]
    );
    ExtractRequest::parse(&r.canonical().unwrap()).unwrap();
    // Replay re-evaluates the rules: the same subset and request identity.
    let scope = rustev_contract::scope::Scope::tenant_only(
        rustev_contract::scope::Handle::new("tenant").unwrap(),
        rustev_contract::scope::Handle::new("revision").unwrap(),
    );
    let again = start(&c, TWO);
    assert_eq!(again.extraction_request("request_filters").unwrap(), &r);
    assert_eq!(
        e.request_document("request_filters", &[], 0, &scope)
            .unwrap()
            .canonical()
            .unwrap(),
        again
            .request_document("request_filters", &[], 0, &scope)
            .unwrap()
            .canonical()
            .unwrap()
    );
    assert_eq!(e.pending()[0].projection, again.pending()[0].projection);
}

#[test]
fn rules_filling_nothing_list_the_absent_fields_request_byte_for_byte() {
    let first = compile(&rules_first(None));
    let absent = compile(&definition(
        json!({"rules_program":"booking@1","fill":"absent_fields"}),
    ));
    let a = start(&first, "nothing here");
    let b = start(&absent, "nothing here");
    assert_eq!(
        a.extraction_request("request_filters")
            .unwrap()
            .canonical()
            .unwrap(),
        b.extraction_request("request_filters")
            .unwrap()
            .canonical()
            .unwrap()
    );
    // The fill mode enters the plan, so the PlanIds (and the envelopes that
    // carry them) differ while the requests agree.
    assert_ne!(first.id, absent.id);
}

#[test]
fn the_model_fills_only_what_was_requested_and_never_replaces_a_rules_value() {
    let c = compile(&rules_first(None));
    let mut e = start(&c, TWO);
    e.supply(
        "request_filters",
        &[],
        output(json!({"window":{"start":"2027-01-01","end":"2027-01-03"},
            "features":["pool"],"insufficient":false})),
    )
    .unwrap();
    let (_, ev) = e.finish("rules-first").unwrap();
    let x = &ev.extraction.as_ref().unwrap()[0];
    assert_eq!(x.request_outcome, ExtractionRequestOutcome::Succeeded);
    assert_eq!(field(&ev, "party_size").origin, ExtractionOrigin::Fallback);
    assert_eq!(field(&ev, "area_name").origin, ExtractionOrigin::Fallback);
    for name in ["window", "features", "insufficient"] {
        assert_eq!(
            field(&ev, name).origin,
            ExtractionOrigin::Extracted,
            "{name}"
        );
    }
    // A requested field the model leaves absent stays absent: no second pass.
    let mut e = start(&c, TWO);
    e.supply(
        "request_filters",
        &[],
        output(json!({"insufficient":false})),
    )
    .unwrap();
    let (_, ev) = e.finish("rules-first").unwrap();
    assert_eq!(
        field(&ev, "window").origin,
        ExtractionOrigin::Absent {
            reason: AbsenceReason::Missing
        }
    );
}

#[test]
fn a_member_for_a_rules_filled_field_is_a_record_level_fault() {
    // Section 5: record-level invalid_output; the rules value is kept.
    let c = compile(&rules_first(None));
    let mut e = start(&c, TWO);
    e.supply(
        "request_filters",
        &[],
        output(json!({"party_size":9,"insufficient":false})),
    )
    .unwrap();
    let (_, ev) = e.finish("rules-first").unwrap();
    assert_eq!(
        ev.extraction.as_ref().unwrap()[0].request_outcome,
        ExtractionRequestOutcome::Failed {
            reason: RequestFailure::InvalidBackendOutput
        }
    );
    assert_eq!(field(&ev, "party_size").origin, ExtractionOrigin::Fallback);
    assert_eq!(
        field(&ev, "insufficient").origin,
        ExtractionOrigin::Absent {
            reason: AbsenceReason::RequestFailed {
                reason: RequestFailure::InvalidBackendOutput
            }
        }
    );
}

#[test]
fn a_failed_partial_request_keeps_the_rules_fields() {
    // Section 5: requested fields request_failed{reason}; no second pass.
    let c = compile(&rules_first(None));
    let mut e = start(&c, TWO);
    e.supply(
        "request_filters",
        &[],
        Supplied::Failed(Unresolved::BackendUnavailable {
            detail: "synthetic outage".into(),
        }),
    )
    .unwrap();
    let (_, ev) = e.finish("rules-first").unwrap();
    assert_eq!(field(&ev, "area_name").origin, ExtractionOrigin::Fallback);
    for name in ["window", "features", "insufficient"] {
        assert_eq!(
            field(&ev, name).origin,
            ExtractionOrigin::Absent {
                reason: AbsenceReason::RequestFailed {
                    reason: RequestFailure::BackendUnavailable
                }
            },
            "{name}"
        );
    }
}

#[test]
fn compile_time_budgets_count_a_rules_first_request() {
    // Section 5: refused, category 7, as without rules first.
    let mut d = rules_first(None);
    d.limits.max_semantic_requests = 0;
    let e = compile_with_extraction_documents(
        &d,
        &[backend()],
        &[],
        &[],
        None,
        &[template()],
        &[program()],
    )
    .unwrap_err();
    assert_eq!(e.category, Category::Budget, "{e:?}");
    assert!(e.detail.contains("max_semantic_requests"), "{e:?}");
}

#[test]
fn the_fill_mode_and_required_enter_the_plan_id() {
    let ids: Vec<_> = [
        rules_first(None),
        rules_first(Some(json!(["party_size"]))),
        rules_first(Some(json!(["area_name"]))),
        definition(json!({"rules_program":"booking@1","fill":"absent_fields"})),
    ]
    .iter()
    .map(|d| compile(d).id)
    .collect();
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            assert_ne!(a, b);
        }
    }
}
