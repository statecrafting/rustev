//! Synthetic decision-stage extraction, with no backend or semantic-quality claims.
mod common;
use rustev_contract::{
    Document,
    definition::{Definition, Operation, ProvenanceClass as P, StepBody},
    descriptor::{BackendDescriptor, OutputKind, TextStructure},
    extraction::*,
    extraction_results::*,
    judgment::{Derivation, OutValue, Outcome, Unresolved},
    output::RawOutput,
    plan::{Plan, PlanStepDetail},
    snapshot::Snapshot,
};
use rustev_core::{Compiled, Evaluation, Supplied, compile::compile_with_extraction_documents};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn definition() -> Definition {
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    plan.definition
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
    compile_documents(d, &[])
}
fn compile_documents(
    d: &Definition,
    calibrations: &[rustev_contract::calibration::CalibrationArtifact],
) -> Compiled {
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let PlanStepDetail::Extract(b) = &plan.steps[0].detail else {
        panic!()
    };
    let rules = serde_json::from_value(json!({"schema":"rustev.extract-rules/1","name":"filters-fallback","version":"1","sources":[{"name":"text","ref":"input:message"}],"fields":[{"field":"party_size","rules":[{"name":"party","when":"always","then":{"integer_after":{"source":"text","terms":["party"],"max_digits":2}}}]}]})).unwrap();
    compile_with_extraction_documents(
        d,
        &[backend(), screener()],
        calibrations,
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
fn accept(d: &mut Definition) -> &mut AcceptModelDerived {
    let StepBody::AcceptModelDerived(a) = &mut d.steps[1].body else {
        panic!()
    };
    a
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
    let mut d = definition();
    d.policy.rules[0].then = serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"window","value":{"ref":"step:window_ok"}}]}})).unwrap();
    d.policy.on_unresolved = serde_json::from_value(json!([{"target":"step:window_ok","reasons":"any","action":{"escalate":{"reason":"missing window"}}}])).unwrap();
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
    let Outcome::Propose { params, .. } = &first.0.outcome else {
        panic!()
    };
    assert!(matches!(
        &params["window"],
        OutValue::Attributed {
            derivation: Derivation::ModelDerived,
            origin: AcceptedOrigin::Extracted,
            ..
        }
    ));
    assert_eq!(
        first.1.acceptance.as_ref().unwrap()[0].outcome,
        AcceptanceOutcome::Accepted
    );
    assert!(first.0.lineage.steps.contains(&"window_ok".into()));
    assert!(
        !String::from_utf8(first.1.canonical().unwrap())
            .unwrap()
            .contains("private text")
    );
    golden("extraction.evidence.json", &first.1.canonical().unwrap());
    golden("extraction.judgment.json", &first.0.canonical().unwrap());
}
#[test]
fn validation_absences_and_acceptance_outcomes() {
    for (fields, name, reason) in [
        (json!({}), "party_size", AbsenceReason::Missing),
        (
            json!({"party_size":"4"}),
            "party_size",
            AbsenceReason::WrongType,
        ),
        (
            json!({"party_size":31}),
            "party_size",
            AbsenceReason::OutOfBounds,
        ),
        (
            json!({"area_name":"x".repeat(65)}),
            "area_name",
            AbsenceReason::TooLong,
        ),
        (
            json!({"area_name":"a\nb"}),
            "area_name",
            AbsenceReason::ControlCharacter,
        ),
        (
            json!({"window":{"start":"2027-02-30","end":"2027-03-02"}}),
            "window",
            AbsenceReason::InvalidDate,
        ),
        (
            json!({"features":["unknown"]}),
            "features",
            AbsenceReason::NotInVocabulary,
        ),
        (
            json!({"features":["pool","pool"]}),
            "features",
            AbsenceReason::DuplicateItem,
        ),
        (
            json!({"features":vec!["pool";11]}),
            "features",
            AbsenceReason::TooManyItems,
        ),
    ] {
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
    let (j, e) = run(
        &definition(),
        output(json!({"window":{"start":"2020-01-01","end":"2020-01-02"}})),
    );
    assert_eq!(
        e.acceptance.unwrap()[0].outcome,
        AcceptanceOutcome::Dropped {
            cause: AcceptanceFailure::ValidationFailed
        }
    );
    let notice = j
        .notices
        .iter()
        .find(|n| n.reason == "extracted_window_dropped")
        .unwrap();
    assert_eq!(
        notice.action.as_ref().unwrap().addressed_to,
        rustev_contract::actionability::Party::Host
    );
    let mut d = definition();
    accept(&mut d).on_fail = OnFail::Unresolved;
    d.policy.on_unresolved = serde_json::from_value(
        json!([{"target":"step:window_ok","reasons":"any","action":"propagate"}]),
    )
    .unwrap();
    let (_, e) = run(&d, output(json!({})));
    assert_eq!(
        e.acceptance.unwrap()[0].outcome,
        AcceptanceOutcome::Unresolved
    );
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
fn mass_threshold_and_runtime_absences() {
    let mut d = definition();
    let a = accept(&mut d);
    a.field = "step:request_filters/insufficient".into();
    a.validate = Validation::None;
    a.min_mass = serde_json::from_value(
        json!({"option":"true","at_least":"0.5","basis":{"uncalibrated_threshold":"synthetic"}}),
    )
    .unwrap();
    for (logprobs, expected) in [
        (
            json!([]),
            AcceptanceOutcome::Dropped {
                cause: AcceptanceFailure::DistributionAbsent,
            },
        ),
        (
            json!([{"field":"insufficient","item":"none","values":{"false":0.0,"true":-2.0}}]),
            AcceptanceOutcome::Dropped {
                cause: AcceptanceFailure::MinMassNotMet,
            },
        ),
        (
            json!([{"field":"insufficient","item":"none","values":{"false":0.0,"true":0.0}}]),
            AcceptanceOutcome::Accepted,
        ),
    ] {
        let mut supplied = output(json!({"insufficient":true}));
        let Supplied::Output(RawOutput::TypedRecord { logprobs: lp, .. }) = &mut supplied else {
            panic!()
        };
        *lp = serde_json::from_value(logprobs).unwrap();
        assert_eq!(run(&d, supplied).1.acceptance.unwrap()[0].outcome, expected);
    }
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
                    AbsenceReason::ScreenUnavailable
                } else {
                    AbsenceReason::ScreenBlocked
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
            reason: AbsenceReason::SourceUnavailable
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
                reason: AbsenceReason::RequestFailed { reason }
            }
        );
    }
}

#[test]
fn input_acceptance_preserves_producer_and_supplied_values() {
    let mut d = definition();
    d.steps.remove(0);
    d.inputs[0].ty = rustev_contract::definition::TypeDecl::Integer;
    d.inputs[0].provenance = vec![P::ModelDerived, P::SystemOfRecord];
    let StepBody::AcceptModelDerived(a) = &mut d.steps[0].body else {
        panic!()
    };
    a.field = "input:message".into();
    a.validate = Validation::None;
    for provenance in [P::ModelDerived, P::SystemOfRecord] {
        let c = compile(&d);
        let mut s = common::snapshot(vec![common::entry(
            "message",
            json!(7),
            provenance,
            common::NOW,
        )]);
        s.schema = "rustev.snapshot/2".into();
        if provenance == P::ModelDerived {
            s.entries[0].producer = Some(serde_json::from_value(json!({"artifact_id":common::id('a'),"template_digest":"none","produced_at_ms":common::NOW})).unwrap());
        }
        let e = Evaluation::start(&c, &s, common::ts(common::NOW)).unwrap();
        assert_eq!(
            e.exact_value("window_ok"),
            Some(&rustev_core::value::Value::Integer(7))
        );
        let (_, e) = e.finish("input").unwrap();
        let a = &e.acceptance.unwrap()[0];
        assert_eq!(
            a.outcome,
            if provenance == P::ModelDerived {
                AcceptanceOutcome::Accepted
            } else {
                AcceptanceOutcome::PassedThrough
            }
        );
        assert_eq!(e.inputs[0].producer, s.entries[0].producer);
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
        ("no party", false, AbsenceReason::FallbackNoMatch),
        ("party 99", false, AbsenceReason::FallbackInvalid),
        ("party 4", true, AbsenceReason::SourceUnavailable),
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
            reason: AbsenceReason::VocabularyUnavailable
        }
    );
}

#[test]
fn accepted_insufficient_field_selects_declared_questions() {
    let mut d = definition();
    accept(&mut d).field = "step:request_filters/insufficient".into();
    accept(&mut d).validate = Validation::None;
    let mut question_input = d.inputs[0].clone();
    question_input.name = "details".into();
    question_input.provenance = vec![P::UserSupplied];
    d.inputs.push(question_input);
    let mut raw = serde_json::to_value(d).unwrap();
    raw["actionability"] = json!({"caller_controlled":["user-supplied"],"remediations":[],"checks":[],"constraint_groups":[],"questions":[{"id":"ask_details","fields":["details"],"text":"What else do you need?"}]});
    raw["policy"]["on_unresolved"] =
        json!([{"target":"step:window_ok","reasons":"any","action":"as_unmet"}]);
    raw["policy"]["rules"] = json!([
        {"id":"ask","when":{"exact":{"cmp":{"left":{"ref":"step:window_ok"},"op":"eq","right":{"lit":{"bool":true}}}}},"then":{"needs_input":{"fields":["details"],"questions":["ask_details"]}}},
        {"id":"otherwise","when":"always","then":{"propose":{"action":"recommend","params":[]}}}
    ]);
    let d = serde_json::from_value(raw).unwrap();
    let (j, _) = run(&d, output(json!({"insufficient":true})));
    let Outcome::NeedsInput(n) = j.outcome else {
        panic!("expected needs_input")
    };
    assert_eq!(n.questions[0].id, "ask_details");
    assert_eq!(
        n.action.addressed_to,
        rustev_contract::actionability::Party::Caller
    );
    let (j, _) = run(&d, output(json!({"insufficient":false})));
    assert!(matches!(j.outcome, Outcome::Propose { .. }));
}

#[test]
fn calibrated_mass_threshold_uses_bound_artifact() {
    use rustev_contract::{Identified, calibration::CalibrationArtifact};
    let mut d = definition();
    let base = compile(&d);
    let PlanStepDetail::Extract(binding) = &base.plan.steps[0].detail else {
        panic!()
    };
    let artifact: CalibrationArtifact = serde_json::from_value(json!({
        "schema":"rustev.calibration/1",
        "binding":{"artifact":backend().artifact,"task":format!("filters@{}", binding.template_id),
            "question":"request_filters/insufficient","dataset":common::id('c'),"method":"temperature/1"},
        "options":["false","true"],"parameters":{"temperature":{"temperature":"2"}}
    })).unwrap();
    let cid = artifact.id().unwrap();
    let a = accept(&mut d);
    a.field = "step:request_filters/insufficient".into();
    a.validate = Validation::None;
    a.min_mass = serde_json::from_value(
        json!({"option":"true","at_least":"0.8","basis":{"calibration":cid}}),
    )
    .unwrap();
    let c = compile_documents(&d, &[artifact]);
    // At temperature 2, log odds 2 gives about .731, not the raw .881.
    for (odds, cause) in [
        (Some(4.0), None),
        (Some(2.0), Some(AcceptanceFailure::MinMassNotMet)),
        (None, Some(AcceptanceFailure::DistributionAbsent)),
    ] {
        let mut supplied = output(json!({"insufficient":true}));
        let Supplied::Output(RawOutput::TypedRecord { logprobs, .. }) = &mut supplied else {
            panic!()
        };
        if let Some(odds) = odds {
            *logprobs = serde_json::from_value(
                json!([{"field":"insufficient","item":"none","values":{"false":0.0,"true":odds}}]),
            )
            .unwrap();
        }
        let mut e = Evaluation::start(&c, &snapshot(), common::ts(common::NOW)).unwrap();
        e.supply("request_filters", &[], supplied).unwrap();
        if let Some(cause) = cause {
            assert_acceptance_absent(&e, cause);
        }
        let (_, evidence) = e.finish("calibrated").unwrap();
        assert_eq!(
            evidence.acceptance.unwrap()[0].outcome,
            cause.map_or(AcceptanceOutcome::Accepted, |cause| {
                AcceptanceOutcome::Dropped { cause }
            })
        );
        assert_eq!(
            evidence.extraction.unwrap()[0].fields[4].calibration,
            if odds.is_some() {
                ExtractionCalibration::Applied(cid.clone())
            } else {
                ExtractionCalibration::None
            }
        );
    }
}

fn assert_acceptance_absent(e: &Evaluation<'_>, cause: AcceptanceFailure) {
    assert_eq!(
        e.exact_value("window_ok"),
        Some(&rustev_core::value::Value::Absent(
            rustev_core::value::Absence {
                reason: rustev_contract::grade::AbsentReason::AcceptanceFailed { cause },
                predicate: None,
            }
        ))
    );
}

#[test]
fn dropped_acceptance_retains_absence_and_validation_causes() {
    for (fields, fault, cause) in [
        (
            json!({}),
            false,
            AcceptanceFailure::Absence(AbsenceReason::Missing),
        ),
        (
            json!({"window":{"start":"2020-01-01","end":"2020-01-02"}}),
            false,
            AcceptanceFailure::ValidationFailed,
        ),
        (
            json!({"window":{"start":"2027-01-01","end":"2027-01-02"}}),
            true,
            AcceptanceFailure::ValidationFault,
        ),
    ] {
        let mut d = definition();
        if fault {
            let mut missing = d.inputs[0].clone();
            missing.name = "missing".into();
            missing.ty = rustev_contract::definition::TypeDecl::Integer;
            d.inputs.push(missing);
            accept(&mut d).validate = serde_json::from_value(json!({"predicate":{"cmp":{"left":{"ref":"input:missing"},"op":"gt","right":{"lit":{"integer":0}}}}})).unwrap();
        }
        let c = compile(&d);
        let mut e = Evaluation::start(&c, &snapshot(), common::ts(common::NOW)).unwrap();
        e.supply("request_filters", &[], output(fields)).unwrap();
        assert_acceptance_absent(&e, cause);
        assert_eq!(
            e.finish("absence").unwrap().1.acceptance.unwrap()[0].outcome,
            AcceptanceOutcome::Dropped { cause }
        );
    }
}

fn proposed_questions_definition() -> Definition {
    let mut raw = serde_json::to_value(definition()).unwrap();
    raw["inputs"].as_array_mut().unwrap().push(json!({"name":"details","type":{"text":{"max_bytes":128}},"provenance":["user-supplied"],"freshness":"not_required"}));
    raw["steps"][0]["body"]["extract"]["output"]["fields"].as_array_mut().unwrap().push(json!({"name":"questions","type":{"list":{"item":{"enum":{"vocabulary":"questions"}},"max_items":3}}}));
    // Literal vocabulary order is intentionally different from allowed order.
    raw["steps"][0]["body"]["extract"]["vocabularies"]["questions"] =
        json!(["third", "first", "second"]);
    raw["steps"][1]["body"]["accept_model_derived"]["field"] =
        json!("step:request_filters/questions");
    raw["steps"][1]["body"]["accept_model_derived"]["validate"] = json!("none");
    raw["actionability"] = json!({"caller_controlled":["user-supplied"],"remediations":[],"checks":[],"constraint_groups":[],"questions":[
        {"id":"first","fields":["details"],"text":"First declared question?"},
        {"id":"second","fields":["details"],"text":"Second declared question?"},
        {"id":"third","fields":["details"],"text":"Third declared question?"}
    ]});
    raw["policy"]["rules"] = json!([{"id":"ask","when":"always","then":{"needs_input":{"fields":["details"],"questions":{"proposed_by":"window_ok","allowed":["first","second","third"]}}}}]);
    serde_json::from_value(raw).unwrap()
}

#[test]
fn proposed_questions_use_allowed_order_declared_text_and_selected_continuation() {
    use rustev_contract::{
        Identified,
        canonical::{record_canonical_bytes, tagged_digest},
    };
    let d = proposed_questions_definition();
    let c = compile(&d);
    for (fields, selected) in [
        (
            json!({"questions":["third","first"]}),
            vec!["first", "third"],
        ),
        (json!({"questions":["second"]}), vec!["second"]),
        (json!({"questions":[]}), vec!["first", "second", "third"]),
        (json!({}), vec!["first", "second", "third"]),
        (
            json!({"questions":["Injected question text?"]}),
            vec!["first", "second", "third"],
        ),
    ] {
        let (j, _) = run(&d, output(fields));
        let Outcome::NeedsInput(n) = &j.outcome else {
            panic!("{:?}", j.outcome)
        };
        assert_eq!(
            n.questions
                .iter()
                .map(|q| q.id.as_str())
                .collect::<Vec<_>>(),
            selected
        );
        for q in &n.questions {
            assert_eq!(
                q,
                d.actionability
                    .as_ref()
                    .unwrap()
                    .questions
                    .iter()
                    .find(|t| t.id == q.id)
                    .unwrap()
            );
        }
        assert!(j.lineage.steps.contains(&"window_ok".into()));
        let bytes = record_canonical_bytes(&json!({"plan_id":c.id,"snapshot_id":snapshot().id().unwrap(),"evaluation_time_ms":common::NOW,"site":"rule:ask","fields":["details"],"questions":selected})).unwrap();
        assert_eq!(
            n.continuation.to_string(),
            tagged_digest("rustev.continuation/1", &bytes)
        );
    }
}

#[test]
fn proposed_questions_require_acceptance_of_a_matching_literal_enum_list() {
    let base = serde_json::to_value(proposed_questions_definition()).unwrap();
    for (pointer, value) in [
        (
            "/policy/rules/0/then/needs_input/questions/proposed_by",
            json!("missing"),
        ),
        (
            "/policy/rules/0/then/needs_input/questions/proposed_by",
            json!("request_filters"),
        ),
        (
            "/policy/rules/0/then/needs_input/questions/allowed",
            json!([]),
        ),
        (
            "/policy/rules/0/then/needs_input/questions/allowed",
            json!(["first"]),
        ),
        (
            "/steps/1/body/accept_model_derived/field",
            json!("input:message"),
        ),
        (
            "/steps/1/body/accept_model_derived/field",
            json!("step:request_filters/insufficient"),
        ),
        (
            "/steps/0/body/extract/output/fields/5/type",
            json!({"enum":{"vocabulary":"questions"}}),
        ),
        (
            "/steps/0/body/extract/output/fields/5/type/list/item",
            json!({"text":{"max_bytes":64}}),
        ),
        (
            "/steps/0/body/extract/vocabularies/questions",
            json!({"input":"input:message"}),
        ),
    ] {
        let mut raw = base.clone();
        *raw.pointer_mut(pointer).unwrap() = value;
        let d: Definition = serde_json::from_value(raw).unwrap();
        let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
        let PlanStepDetail::Extract(binding) = &plan.steps[0].detail else {
            panic!()
        };
        let error = compile_with_extraction_documents(
            &d,
            &[backend()],
            &[],
            &[],
            None,
            std::slice::from_ref(&binding.template),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.category,
            rustev_core::Category::InvalidAcceptance,
            "{pointer}: {error:?}"
        );
    }
    // A repeated allowed id is refused before the acceptance gate (033 3.7).
    let mut raw = base;
    raw["policy"]["rules"][0]["then"]["needs_input"]["questions"]["allowed"] =
        json!(["first", "second", "third", "first"]);
    let d: Definition = serde_json::from_value(raw).unwrap();
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let PlanStepDetail::Extract(binding) = &plan.steps[0].detail else {
        panic!()
    };
    let error = compile_with_extraction_documents(
        &d,
        &[backend()],
        &[],
        &[],
        None,
        std::slice::from_ref(&binding.template),
        &[],
    )
    .unwrap_err();
    assert_eq!(error.category, rustev_core::Category::InvalidDefinition);
    assert!(error.detail.contains("each question once"), "{error:?}");
}

#[test]
fn handler_question_selection_uses_the_same_accepted_values() {
    let mut d = proposed_questions_definition();
    let rustev_contract::definition::OutcomeDecl::NeedsInput { fields, questions } =
        d.policy.rules.remove(0).then
    else {
        panic!()
    };
    d.policy.rules = definition().policy.rules;
    d.policy
        .on_unresolved
        .push(rustev_contract::definition::Handler {
            target: "input:details".into(),
            reasons: rustev_contract::definition::ReasonSet::Any,
            action: rustev_contract::definition::HandlerAction::NeedsInput { fields, questions },
        });
    let (j, _) = run(&d, output(json!({"questions":["second"]})));
    let Outcome::NeedsInput(n) = j.outcome else {
        panic!()
    };
    assert_eq!(
        n.questions
            .iter()
            .map(|q| q.id.as_str())
            .collect::<Vec<_>>(),
        ["second"]
    );
}

#[test]
fn future_producer_timestamp_is_invalid_input_with_existing_precedence() {
    use rustev_contract::evidence::InputStatus;
    let mut d = definition();
    d.steps.remove(0);
    d.inputs[0].ty = rustev_contract::definition::TypeDecl::Integer;
    d.inputs[0].provenance = vec![P::ModelDerived];
    let StepBody::AcceptModelDerived(a) = &mut d.steps[0].body else {
        panic!()
    };
    a.field = "input:message".into();
    a.validate = Validation::None;
    let c = compile(&d);
    for (offset, conflict) in [(0, false), (1, false), (1, true)] {
        let mut s = common::snapshot(vec![common::entry(
            "message",
            json!(7),
            P::ModelDerived,
            common::NOW,
        )]);
        s.schema = "rustev.snapshot/2".into();
        s.entries[0].producer = Some(serde_json::from_value(json!({"artifact_id":common::id('a'),"template_digest":"none","produced_at_ms":common::NOW + offset})).unwrap());
        if conflict {
            s.entries.push(common::entry(
                "message",
                json!(8),
                P::ModelDerived,
                common::NOW,
            ));
        }
        let (_, e) = Evaluation::start(&c, &s, common::ts(common::NOW))
            .unwrap()
            .finish("producer-time")
            .unwrap();
        match &e.inputs[0].status {
            InputStatus::Resolved { .. } => assert_eq!(offset, 0),
            InputStatus::Unresolved(Unresolved::InvalidInput { fields, .. }) => {
                assert_eq!(offset, 1);
                assert!(!conflict);
                assert_eq!(fields, &["message"]);
            }
            InputStatus::Unresolved(Unresolved::Conflict { .. }) => assert!(conflict),
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn screener() -> BackendDescriptor {
    let mut b = backend();
    b.backend_id = "screen".into();
    b.operations = vec![common::support(
        Operation::Proposition,
        OutputKind::Screen,
        2,
    )];
    b.operations[0].screen = Some(rustev_contract::descriptor::ScreenProfile {
        policy_version: "synthetic/1".into(),
        categories: vec!["injection".into()],
        max_text_bytes: 128,
        text_unit_bytes: 16,
    });
    b
}
fn screening(d: &mut Definition, unavailable: &str) {
    d.inputs[0].provenance = vec![P::ThirdParty];
    d.screening = Some(
        serde_json::from_value(json!({"entries":[{
        "id":"message", "fields":["input:message"], "item_id":"none",
        "screener":"screen", "on_block":"drop_field",
        "on_unavailable":unavailable, "address":"computed"
    }], "max_text_units":32}))
        .unwrap(),
    );
}
fn verdict(blocked: bool) -> Supplied {
    Supplied::Output(RawOutput::Screen {
        blocked,
        categories: vec![],
        units: rustev_contract::output::ScreenUnits::NotReported,
    })
}
#[test]
fn screening_resolves_before_extraction_and_never_dispatches_withheld_text() {
    for (mode, blocked, offline, reason) in [
        (
            "fail_closed",
            true,
            false,
            Some(AbsenceReason::ScreenBlocked),
        ),
        (
            "fail_closed",
            false,
            true,
            Some(AbsenceReason::ScreenUnavailable),
        ),
        (
            "skip_generation",
            false,
            true,
            Some(AbsenceReason::ScreenUnavailable),
        ),
        ("fail_open", false, true, None),
        ("fail_closed", false, false, None),
    ] {
        for fallback in [false, true] {
            let mut d = definition();
            screening(&mut d, mode);
            if fallback {
                extract(&mut d).fallback = serde_json::from_value(json!({
                    "rules_program":"filters-fallback@1", "fill":"absent_fields"
                }))
                .unwrap();
            }
            let c = compile(&d);
            let mut s = snapshot();
            s.entries[0].provenance = P::ThirdParty;
            let mut e = Evaluation::start(&c, &s, common::ts(common::NOW)).unwrap();
            assert_eq!(e.pending().len(), 1);
            assert_eq!(e.pending()[0].step, "screen:message:0");
            assert!(e.extraction_request("request_filters").is_none());
            e.supply(
                "screen:message:0",
                &[],
                if offline {
                    Supplied::Failed(Unresolved::BackendUnavailable {
                        detail: "offline".into(),
                    })
                } else {
                    verdict(blocked)
                },
            )
            .unwrap();
            if reason.is_none() {
                assert_eq!(e.pending()[0].step, "request_filters");
                assert_eq!(
                    e.extraction_request("request_filters").unwrap().projection["input:message"],
                    json!("party 4")
                );
                e.supply("request_filters", &[], output(json!({}))).unwrap();
            }
            assert!(e.pending().is_empty());
            let (_, evidence) = e.finish("screened-extract").unwrap();
            let extraction = &evidence.extraction.unwrap()[0];
            if let Some(reason) = reason {
                assert_eq!(
                    extraction.request_outcome,
                    ExtractionRequestOutcome::Withheld { reason }
                );
                if !fallback {
                    assert!(
                        extraction
                            .fields
                            .iter()
                            .all(|f| f.origin == ExtractionOrigin::Absent { reason })
                    );
                }
            }
            if fallback {
                let field = &extraction.fields[1];
                assert_eq!(field.origin, ExtractionOrigin::Fallback);
                assert_eq!(field.value(), Some(&json!(4)));
                assert_eq!(field.derivation, Some(Derivation::ExactDerived));
            }
        }
    }
}

#[test]
fn a_block_names_the_outcome_when_another_screener_is_unavailable() {
    let mut d = definition();
    screening(&mut d, "fail_closed");
    let mut note = d.inputs[0].clone();
    note.name = "note".into();
    d.inputs.push(note);
    let mut screening = serde_json::to_value(d.screening.as_ref().unwrap()).unwrap();
    let mut entry = screening["entries"][0].clone();
    entry["id"] = json!("note");
    entry["fields"] = json!(["input:note"]);
    screening["entries"].as_array_mut().unwrap().push(entry);
    d.screening = Some(serde_json::from_value(screening).unwrap());
    extract(&mut d).projection.push("input:note".into());
    let c = compile(&d);
    let mut s = snapshot();
    s.entries[0].provenance = P::ThirdParty;
    let mut note = s.entries[0].clone();
    note.field = "note".into();
    s.entries.push(note);
    let mut e = Evaluation::start(&c, &s, common::ts(common::NOW)).unwrap();
    let steps: Vec<_> = e.pending().iter().map(|r| r.step.clone()).collect();
    assert_eq!(steps.len(), 2, "{steps:?}");
    for step in steps {
        let supplied = if step.starts_with("screen:message") {
            verdict(true)
        } else {
            Supplied::Failed(Unresolved::BackendUnavailable {
                detail: "offline".into(),
            })
        };
        e.supply(&step, &[], supplied).unwrap();
    }
    assert!(e.extraction_request("request_filters").is_none());
    let (_, evidence) = e.finish("blocked-and-unavailable").unwrap();
    assert_eq!(
        evidence.extraction.unwrap()[0].request_outcome,
        ExtractionRequestOutcome::Withheld {
            reason: AbsenceReason::ScreenBlocked
        }
    );
}

#[test]
fn screened_mixed_fallback_survives_acceptance_to_judgment() {
    let mut d = definition();
    screening(&mut d, "fail_closed");
    d.inputs[0].provenance = vec![P::ModelDerived];
    let mut exact = d.inputs[0].clone();
    exact.name = "exact".into();
    exact.provenance = vec![P::SystemOfRecord];
    d.inputs.push(exact);
    let e = extract(&mut d);
    e.projection.push("input:exact".into());
    e.fallback =
        serde_json::from_value(json!({"rules_program":"mixed@1","fill":"absent_fields"})).unwrap();
    let a = accept(&mut d);
    a.field = "step:request_filters/party_size".into();
    a.validate = Validation::None;
    d.policy.rules[0].then = serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"party","value":{"ref":"step:window_ok"}}]}})).unwrap();
    d.policy.on_unresolved = serde_json::from_value(json!([{"target":"step:window_ok","reasons":"any","action":{"escalate":{"reason":"absent"}}}])).unwrap();
    let rules = serde_json::from_value(json!({"schema":"rustev.extract-rules/1","name":"mixed","version":"1",
        "sources":[{"name":"model","ref":"input:message"},{"name":"exact","ref":"input:exact"}],
        "fields":[{"field":"party_size","rules":[{"name":"both","when":{"contains_any":{"source":"exact","terms":["party"]}},
        "then":{"integer_after":{"source":"model","terms":["party"],"max_digits":2}}}]}]})).unwrap();
    let plan: Plan = serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
    let PlanStepDetail::Extract(b) = &plan.steps[0].detail else {
        panic!()
    };
    let c = compile_with_extraction_documents(
        &d,
        &[backend(), screener()],
        &[],
        &[],
        None,
        std::slice::from_ref(&b.template),
        &[rules],
    )
    .unwrap();
    let mut s = snapshot();
    s.schema = "rustev.snapshot/2".into();
    s.entries[0].provenance = P::ModelDerived;
    s.entries[0].producer = Some(serde_json::from_value(json!({"artifact_id":common::id('a'),"template_digest":"none","produced_at_ms":common::NOW})).unwrap());
    s.entries.push(common::entry(
        "exact",
        json!("party"),
        P::SystemOfRecord,
        common::NOW,
    ));
    let mut ev = Evaluation::start(&c, &s, common::ts(common::NOW)).unwrap();
    ev.supply("screen:message:0", &[], verdict(false)).unwrap();
    ev.supply("request_filters", &[], output(json!({})))
        .unwrap();
    let (j, evidence) = ev.finish("mixed-fallback").unwrap();
    let f = &evidence.extraction.unwrap()[0].fields[1];
    assert_eq!(f.origin, ExtractionOrigin::Fallback);
    assert_eq!(f.derivation, Some(Derivation::MixedDerived));
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!()
    };
    assert!(matches!(
        &params["party"],
        OutValue::Attributed {
            derivation: Derivation::MixedDerived,
            ..
        }
    ));
    assert!(j.lineage.inputs.contains(&"exact".into()));
    assert!(j.lineage.inputs.contains(&"message".into()));
    assert_eq!(
        j.lineage.producers.as_ref().unwrap()["message"],
        s.entries[0].producer.clone().unwrap()
    );
}

#[test]
fn screening_covers_whole_records_and_accepted_input_field_paths() {
    for accepted in [false, true] {
        let mut d = definition();
        screening(&mut d, "fail_closed");
        d.inputs[0].ty = serde_json::from_value(json!({"record":{"fields":[
            {"name":"text","type":{"text":{"max_bytes":64}}},
            {"name":"other","type":{"text":{"max_bytes":64}}}
        ]}}))
        .unwrap();
        let fields = if accepted {
            d.inputs[0].provenance = vec![P::ModelDerived];
            let mut a = d.steps[1].clone();
            a.id = "input_ok".into();
            let StepBody::AcceptModelDerived(body) = &mut a.body else {
                panic!()
            };
            body.field = "input:message".into();
            body.validate = Validation::None;
            d.steps.push(a);
            extract(&mut d).projection = vec!["step:input_ok/text".into()];
            vec!["input:message/text"]
        } else {
            vec!["input:message/text", "input:message/other"]
        };
        d.screening.as_mut().unwrap().entries[0].fields =
            fields.iter().map(|s| (*s).into()).collect();
        let c = compile(&d);
        let mut s = snapshot();
        s.entries[0].value = json!({"text":"blocked", "other":"readable"});
        s.entries[0].provenance = d.inputs[0].provenance[0];
        if accepted {
            s.schema = "rustev.snapshot/2".into();
            s.entries[0].producer = Some(serde_json::from_value(json!({"artifact_id":common::id('a'),"template_digest":"none","produced_at_ms":common::NOW})).unwrap());
        }
        let mut ev = Evaluation::start(&c, &s, common::ts(common::NOW)).unwrap();
        assert!(ev.pending().iter().all(|r| r.step.starts_with("screen:")));
        ev.supply("screen:message:0", &[], verdict(true)).unwrap();
        if !accepted {
            assert!(ev.extraction_request("request_filters").is_none());
            ev.supply("screen:message:1", &[], verdict(false)).unwrap();
        }
        assert!(ev.pending().is_empty());
        let (_, evidence) = ev.finish("nested-screen").unwrap();
        assert_eq!(
            evidence.extraction.unwrap()[0].request_outcome,
            ExtractionRequestOutcome::Withheld {
                reason: AbsenceReason::ScreenBlocked
            }
        );
        if !accepted {
            d.screening.as_mut().unwrap().entries[0].fields.pop();
            let plan: Plan =
                serde_json::from_str(include_str!("golden/acceptance.plan.json")).unwrap();
            let PlanStepDetail::Extract(b) = &plan.steps[0].detail else {
                panic!()
            };
            let error = compile_with_extraction_documents(
                &d,
                &[backend(), screener()],
                &[],
                &[],
                None,
                std::slice::from_ref(&b.template),
                &[],
            )
            .unwrap_err();
            assert_eq!(error.category, rustev_core::Category::UnscreenedText);
            assert!(error.detail.contains("input:message/other"));
        }
    }
}
