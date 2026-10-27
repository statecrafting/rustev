//! Synthetic decision-stage extraction, with no backend or semantic-quality claims.
mod common;
use rustev_contract::{
    Document,
    definition::{Definition, Operation, ProvenanceClass as P, StepBody},
    descriptor::{BackendDescriptor, OutputKind, TextStructure},
    extraction::{AbsenceReason as Absence, *},
    extraction_results::*,
    judgment::{Derivation, Outcome, Unresolved},
    output::RawOutput,
    plan::{Plan, PlanStepDetail},
    snapshot::Snapshot,
};
use rustev_core::{Compiled, Evaluation, Supplied, compile::compile_with_extraction_documents};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn definition() -> Definition {
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let mut d = plan.definition;
    d.steps.truncate(1);
    d
}
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
fn compile(d: &Definition) -> Compiled {
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let PlanStepDetail::Extract(b) = &plan.steps[0].detail else {
        panic!()
    };
    let rules = serde_json::from_value(json!({"schema":"rustev.extract-rules/1","name":"filters-fallback","version":"1","sources":[{"name":"text","ref":"input:message"}],"fields":[{"field":"party_size","rules":[{"name":"party","when":"always","then":{"integer_after":{"source":"text","terms":["party"],"max_digits":2}}}]}]})).unwrap();
    compile_with_extraction_documents(
        d,
        &[backend()],
        &[],
        &[],
        None,
        std::slice::from_ref(&b.template),
        &[rules],
    )
    .unwrap()
}
fn snapshot() -> Snapshot {
    common::snapshot(vec![common::entry(
        "message",
        json!("party 4"),
        P::SystemOfRecord,
        common::NOW,
    )])
}
fn output(fields: Value) -> Supplied {
    Supplied::Output(RawOutput::TypedRecord {
        fields: serde_json::from_value(fields).unwrap(),
        logprobs: vec![],
        provider_reported: BTreeMap::new(),
    })
}
fn extract(d: &mut Definition) -> &mut Extract {
    let StepBody::Extract(e) = &mut d.steps[0].body else {
        panic!()
    };
    e
}
fn run(
    d: &Definition,
    supplied: Supplied,
) -> (
    rustev_contract::judgment::Judgment,
    rustev_contract::evidence::EvidenceRecord,
) {
    let c = compile(d);
    let mut e = Evaluation::start(&c, &snapshot(), common::ts(common::NOW)).unwrap();
    assert_eq!(e.pending().len(), 1);
    e.supply("request_filters", &[], supplied).unwrap();
    assert!(e.pending().is_empty());
    e.finish("synthetic-extraction").unwrap()
}
fn golden(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
#[test]
fn example_requests_evidence_and_judgment_are_deterministic() {
    let d = definition();
    let c = compile(&d);
    let mut e = Evaluation::start(&c, &snapshot(), common::ts(common::NOW)).unwrap();
    let request = e.extraction_request("request_filters").unwrap();
    assert_eq!(request.schema, "rustev.extract-request/1");
    assert_eq!(request.instructions, "Extract filters.");
    assert_eq!(request.projection["input:message"], "party 4");
    let scope = rustev_contract::scope::Scope::tenant_only(
        rustev_contract::scope::Handle::new("tenant").unwrap(),
        rustev_contract::scope::Handle::new("revision").unwrap(),
    );
    let request_doc = e
        .request_document("request_filters", &[], 0, &scope)
        .unwrap();
    rustev_contract::request::RequestDoc::parse(&request_doc.canonical().unwrap()).unwrap();
    let supplied = output(
        json!({"window":{"start":"2027-01-01","end":"2027-01-03"},"party_size":4,"features":["pool"],"area_name":"private text","insufficient":false}),
    );
    e.supply("request_filters", &[], supplied.clone()).unwrap();
    let first = e.finish("synthetic-extraction").unwrap();
    let second = run(&d, supplied);
    assert_eq!(first.0.canonical().unwrap(), second.0.canonical().unwrap());
    assert_eq!(first.1.canonical().unwrap(), second.1.canonical().unwrap());
    assert!(matches!(first.0.outcome, Outcome::Propose { .. }));
    assert!(first.1.acceptance.is_none());
    let fields = &first.1.extraction.as_ref().unwrap()[0].fields;
    assert_eq!(fields[0].origin, ExtractionOrigin::Extracted);
    assert_eq!(fields[0].derivation, Some(Derivation::ModelDerived));
    assert!(
        !String::from_utf8(first.1.canonical().unwrap())
            .unwrap()
            .contains("private text")
    );
    golden("extraction.evidence.json", &first.1.canonical().unwrap());
    golden("extraction.judgment.json", &first.0.canonical().unwrap());
}
#[test]
fn extraction_distributions() {
    let mut supplied = output(json!({"insufficient":true}));
    let Supplied::Output(RawOutput::TypedRecord { logprobs, .. }) = &mut supplied else {
        panic!()
    };
    *logprobs = serde_json::from_value(
        json!([{"field":"insufficient","item":"none","values":{"false":0.0,"true":0.0}}]),
    )
    .unwrap();
    let (_, evidence) = run(&definition(), supplied);
    assert_eq!(
        evidence.extraction.unwrap()[0].fields[4].distribution,
        FieldDistribution::Masses(vec![0.5, 0.5])
    );
}

#[test]
fn validation_absences() {
    use Absence::*;
    for (fields, reason) in [
        (json!({}), Missing),
        (json!({"party_size":"4"}), WrongType),
        (json!({"party_size":31}), OutOfBounds),
        (json!({"area_name":"x".repeat(65)}), TooLong),
        (json!({"area_name":"a\nb"}), ControlCharacter),
        (
            json!({"window":{"start":"2027-02-30","end":"2027-03-02"}}),
            InvalidDate,
        ),
        (json!({"features":["unknown"]}), NotInVocabulary),
        (json!({"features":["pool","pool"]}), DuplicateItem),
        (json!({"features":vec!["pool";11]}), TooManyItems),
    ] {
        let name = fields
            .as_object()
            .unwrap()
            .keys()
            .next()
            .map(String::as_str)
            .unwrap_or("party_size")
            .to_owned();
        let (_, e) = run(&definition(), output(fields));
        let f = e
            .extraction
            .unwrap()
            .remove(0)
            .fields
            .into_iter()
            .find(|f| f.name == name)
            .unwrap();
        assert_eq!(f.origin, ExtractionOrigin::Absent { reason });
    }
}

#[test]
fn fallback_fill_modes_failure_and_screening() {
    for fill in ["absent_fields", "request_failure"] {
        let mut d = definition();
        extract(&mut d).fallback =
            serde_json::from_value(json!({"rules_program":"filters-fallback@1","fill":fill}))
                .unwrap();
        for (supplied, expected) in [
            (output(json!({})), fill == "absent_fields"),
            (
                Supplied::Failed(Unresolved::BackendUnavailable {
                    detail: "offline".into(),
                }),
                true,
            ),
        ] {
            let (_, e) = run(&d, supplied);
            let f = &e.extraction.as_ref().unwrap()[0].fields[1];
            assert_eq!(f.origin == ExtractionOrigin::Fallback, expected);
            if expected {
                assert_eq!(f.derivation, Some(Derivation::ExactDerived));
            }
        }
        let (_, e) = run(&d, output(json!({"party_size":9})));
        assert_eq!(
            e.extraction.as_ref().unwrap()[0].fields[1].value(),
            Some(&json!(9))
        );
        let c = compile(&d);
        for unavailable in [false, true] {
            let mut e = Evaluation::start(&c, &snapshot(), common::ts(common::NOW)).unwrap();
            e.withhold_extraction("request_filters", unavailable, &[])
                .unwrap();
            let (_, e) = e.finish("screen").unwrap();
            assert_eq!(
                e.extraction.as_ref().unwrap()[0].fields[1].origin,
                ExtractionOrigin::Fallback
            );
        }
    }
}

#[test]
fn runtime_absences() {
    let c = compile(&definition());
    for unavailable in [false, true] {
        let mut e = Evaluation::start(&c, &snapshot(), common::ts(common::NOW)).unwrap();
        e.withhold_extraction("request_filters", unavailable, &[])
            .unwrap();
        let (_, e) = e.finish("screen").unwrap();
        assert_eq!(
            e.extraction.unwrap()[0].fields[0].origin,
            ExtractionOrigin::Absent {
                reason: if unavailable {
                    Absence::ScreenUnavailable
                } else {
                    Absence::ScreenBlocked
                }
            }
        );
    }
    let mut e = Evaluation::start(
        &c,
        &Snapshot {
            schema: "rustev.snapshot/2".into(),
            entries: vec![],
        },
        common::ts(common::NOW),
    )
    .unwrap();
    assert!(e.pending().is_empty());
    assert!(e.cancel_extraction("request_filters").is_err());
    assert_eq!(
        e.finish("missing").unwrap().1.extraction.unwrap()[0].fields[0].origin,
        ExtractionOrigin::Absent {
            reason: Absence::SourceUnavailable
        }
    );
    for reason in [
        RequestFailure::BackendUnavailable,
        RequestFailure::InvalidBackendOutput,
        RequestFailure::DeadlineExceeded,
        RequestFailure::BudgetExhausted,
        RequestFailure::Cancelled,
    ] {
        let mut e = Evaluation::start(&c, &snapshot(), common::ts(common::NOW)).unwrap();
        if reason == RequestFailure::Cancelled {
            e.cancel_extraction("request_filters").unwrap();
        } else {
            let u = match reason {
                RequestFailure::BackendUnavailable => Unresolved::BackendUnavailable {
                    detail: "offline".into(),
                },
                RequestFailure::InvalidBackendOutput => Unresolved::InvalidBackendOutput {
                    detail: "invalid".into(),
                },
                RequestFailure::DeadlineExceeded => Unresolved::DeadlineExceeded,
                _ => Unresolved::BudgetExhausted {
                    resource: "requests".into(),
                },
            };
            e.supply("request_filters", &[], Supplied::Failed(u))
                .unwrap();
        }
        assert_eq!(
            e.finish("failure").unwrap().1.extraction.unwrap()[0].fields[0].origin,
            ExtractionOrigin::Absent {
                reason: Absence::RequestFailed { reason }
            }
        );
    }
}

#[test]
fn fallback_and_vocabulary_absences() {
    let mut d = definition();
    extract(&mut d).fallback = serde_json::from_value(
        json!({"rules_program":"filters-fallback@1","fill":"absent_fields"}),
    )
    .unwrap();
    let c = compile(&d);
    for (text, withheld, reason) in [
        ("no party", false, Absence::FallbackNoMatch),
        ("party 99", false, Absence::FallbackInvalid),
        ("party 4", true, Absence::SourceUnavailable),
    ] {
        let s = common::snapshot(vec![common::entry(
            "message",
            json!(text),
            P::SystemOfRecord,
            common::NOW,
        )]);
        let mut e = Evaluation::start(&c, &s, common::ts(common::NOW)).unwrap();
        if withheld {
            e.withhold_extraction("request_filters", false, &["input:message".into()])
                .unwrap();
        } else {
            e.supply("request_filters", &[], output(json!({}))).unwrap();
        }
        let (_, evidence) = e.finish("fallback-absence").unwrap();
        assert_eq!(
            evidence.extraction.unwrap()[0].fields[1].origin,
            ExtractionOrigin::Absent { reason }
        );
    }
    let mut d = definition();
    let mut vocabulary = d.inputs[0].clone();
    vocabulary.name = "vocabulary".into();
    vocabulary.ty = rustev_contract::definition::TypeDecl::List {
        item: Box::new(rustev_contract::definition::TypeDecl::Text { max_bytes: 64 }),
        max_items: 10,
    };
    d.inputs.push(vocabulary);
    extract(&mut d).vocabularies.insert(
        "feature_vocabulary".into(),
        ExtractionVocabulary::Input {
            input: "input:vocabulary".into(),
        },
    );
    let (_, e) = run(&d, output(json!({"features":["pool"]})));
    assert_eq!(
        e.extraction.unwrap()[0].fields[2].origin,
        ExtractionOrigin::Absent {
            reason: Absence::VocabularyUnavailable
        }
    );
}
