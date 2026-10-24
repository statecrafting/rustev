//! SYNTHETIC scripted supplies for spec 035. No backend or runtime runs.
mod common;
use common::*;
use rustev_contract::actionability::Party;
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, ScreenProfile};
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::judgment::{Derivation, Outcome, Unresolved};
use rustev_contract::output::{RawOutput, ScreenUnits};
use rustev_contract::screening::*;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, schema};
use rustev_core::{Compiled, Evaluation, Supplied, compile};
use serde_json::json;

const CANARY: &str = "CANARY-screen-only";
fn definition() -> Definition {
    let mut d: serde_json::Value =
        serde_json::from_slice(include_bytes!("golden/screening.definition.json")).unwrap();
    d["actionability"] = json!({"caller_controlled":["user-supplied"],"remediations":[],"questions":[],"constraint_groups":[],"checks":[]});
    Definition::parse(&serde_json::to_vec(&d).unwrap()).unwrap()
}
fn compiled(d: &Definition) -> Compiled {
    let mut screen = label_only();
    screen.backend_id = "screen".into();
    screen.operations = vec![support(Operation::Proposition, OutputKind::Screen, 2)];
    screen.operations[0].screen = Some(ScreenProfile {
        policy_version: "synthetic/1".into(),
        categories: vec!["injection".into()],
        max_text_bytes: 48,
        text_unit_bytes: 16,
    });
    let backends: Vec<BackendDescriptor> = vec![screen, label_only()];
    compile(d, &backends, &[]).unwrap()
}
fn snapshot_text(text: &str) -> Snapshot {
    snapshot(vec![
        entry(
            "request",
            json!({"text":text,"behaviour_text":"behaviour"}),
            ProvenanceClass::UserSupplied,
            NOW,
        ),
        entry(
            "candidates",
            json!([{"id":"a","description":CANARY},{"id":"b","description":"unchanged"}]),
            ProvenanceClass::ThirdParty,
            NOW,
        ),
    ])
}
fn output(blocked: bool) -> Supplied {
    Supplied::Output(RawOutput::Screen {
        blocked,
        categories: if blocked {
            vec!["injection".into()]
        } else {
            vec![]
        },
        units: ScreenUnits::NotReported,
    })
}
fn supply(ev: &mut Evaluation<'_>, step: &str, key: &[&str], value: Supplied) {
    ev.supply(step, &strings(key), value).unwrap();
}
fn drain(ev: &mut Evaluation<'_>) {
    while let Some(r) = ev.pending().first() {
        let value = if r.step.starts_with("screen:") {
            output(false)
        } else {
            Supplied::Output(RawOutput::Label("a".into()))
        };
        ev.supply(&r.step, &r.instance, value).unwrap();
    }
}
fn model(d: &mut Definition, project: &[&str]) {
    d.steps.push(StepDecl {
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
fn documents(e: &EvidenceRecord, name: &str) {
    let bytes = e.record_canonical().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(CANARY));
    EvidenceRecord::parse(&bytes).unwrap();
    golden(&format!("screening.eval.{name}.json"), &bytes);
}
#[test]
fn first_refusal_wins_in_supply_order_and_replays() {
    for (second, party) in [(false, Party::Caller), (true, Party::Upstream)] {
        let mut d = definition();
        d.screening.as_mut().unwrap().entries[2].on_block = OnBlock::Refuse;
        model(&mut d, &["input:request", "input:candidates"]);
        let c = compiled(&d);
        let run = || {
            let snap = snapshot_text(CANARY);
            let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
            assert_eq!(ev.pending().len(), 4);
            let (step, key) = if second {
                ("screen:catalogue:0", vec!["b"])
            } else {
                ("screen:caller_text:0", vec![])
            };
            supply(&mut ev, step, &key, output(true));
            assert!(ev.pending().is_empty());
            assert!(ev.supply("screen:behaviour:0", &[], output(true)).is_err());
            let (j, e) = ev.finish("screen-decision").unwrap();
            let Outcome::Refused {
                refusals,
                addressed_to,
            } = &j.outcome
            else {
                panic!("{j:?}")
            };
            assert_eq!(*addressed_to, party);
            assert_eq!(refusals.len(), 1);
            assert_eq!(
                refusals[0].check,
                if second {
                    "screen:catalogue"
                } else {
                    "screen:caller_text"
                }
            );
            assert_eq!(
                refusals[0].subject,
                if second {
                    "input:candidates[b]/description"
                } else {
                    "input:request/text"
                }
            );
            assert!(!refusals[0].action.transient);
            assert_eq!(j.derivation, Derivation::MixedDerived);
            assert!(j.lineage.steps.contains(&step.into()));
            documents(
                &e,
                if second {
                    "refuse-upstream"
                } else {
                    "refuse-caller"
                },
            );
            j.record_canonical().unwrap()
        };
        assert_eq!(run(), run());
    }
}
#[test]
fn markers_drop_items_and_exact_readers() {
    for drop_item in [false, true] {
        let mut d = definition();
        if drop_item {
            d.screening.as_mut().unwrap().entries[2].on_block = OnBlock::DropItem;
        }
        model(&mut d, &["input:request", "input:candidates"]);
        d.steps.push(StepDecl {
            id: "count".into(),
            body: StepBody::Exact(ExactDecl {
                op: "count_where".into(),
                version: 1,
                args: json!({"list":"input:candidates","predicate":{"all":[]}}),
            }),
        });
        let c = compiled(&d);
        let snap = snapshot_text(CANARY);
        let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
        supply(&mut ev, "screen:caller_text:0", &[], output(false));
        supply(&mut ev, "screen:behaviour:0", &[], output(true));
        supply(&mut ev, "screen:catalogue:0", &["a"], output(true));
        supply(&mut ev, "screen:catalogue:0", &["b"], output(false));
        let pending = ev.pending();
        assert_eq!(pending.len(), 1);
        assert!(
            String::from_utf8_lossy(&pending[0].projection)
                .contains(r#"{"withheld":"screen_blocked"}"#)
        );
        let p: serde_json::Value = serde_json::from_slice(&pending[0].projection).unwrap();
        assert_eq!(
            p["values"]["input:request"]["behaviour_text"],
            withheld_screen_blocked()
        );
        assert_eq!(p["values"]["input:request"]["text"], CANARY);
        let items = p["values"]["input:candidates"].as_array().unwrap();
        assert_eq!(
            items.last().unwrap(),
            &json!({"id":"b","description":"unchanged"})
        );
        if drop_item {
            assert_eq!(items.len(), 1);
        } else {
            assert_eq!(items[0]["description"], withheld_screen_blocked());
        }
        assert_eq!(
            ev.exact_value("count").unwrap().to_json(),
            json!(if drop_item { 1 } else { 2 })
        );
        drain(&mut ev);
        let (_, e) = ev.finish("screen-decision").unwrap();
        if drop_item {
            let count = e.steps.iter().find(|s| s.step == "count").unwrap();
            assert!(format!("{:?}", count.status).contains("MixedDerived"));
        }
        documents(
            &e,
            if drop_item {
                "drop-item"
            } else {
                "drop-text-and-field"
            },
        );
    }
}
#[test]
fn unavailability_policies_invalid_categories_and_empty() {
    for policy in [
        OnUnavailable::FailClosed,
        OnUnavailable::SkipGeneration,
        OnUnavailable::FailOpen,
    ] {
        let mut d = definition();
        d.screening.as_mut().unwrap().entries[2].on_unavailable = policy;
        model(&mut d, &["input:candidates"]);
        let c = compiled(&d);
        let snap = snapshot_text("");
        let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
        assert!(
            !ev.pending()
                .iter()
                .any(|r| r.step == "screen:caller_text:0")
        );
        supply(&mut ev, "screen:behaviour:0", &[], output(false));
        supply(
            &mut ev,
            "screen:catalogue:0",
            &["a"],
            Supplied::Failed(Unresolved::DeadlineExceeded),
        );
        supply(&mut ev, "screen:catalogue:0", &["b"], output(false));
        if policy != OnUnavailable::FailOpen {
            for r in ev.pending() {
                assert!(!String::from_utf8_lossy(&r.projection).contains(CANARY));
            }
            assert!(ev.pending().is_empty());
            assert_eq!(
                ev.status_of("step:model"),
                Err(Unresolved::ScreenUnavailable {
                    fields: vec!["input:candidates[]/description".into()]
                })
            );
        } else {
            assert!(String::from_utf8_lossy(&ev.pending()[0].projection).contains(CANARY));
        }
        drain(&mut ev);
        let (j, e) = ev.finish("screen-decision").unwrap();
        assert!(!matches!(j.outcome, Outcome::Refused { .. }));
        let screens = e.screening.as_ref().unwrap();
        assert_eq!(screens[0].verdict, ScreenVerdict::Empty);
        documents(
            &e,
            match policy {
                OnUnavailable::FailClosed => "fail-closed",
                OnUnavailable::SkipGeneration => "skip-generation",
                OnUnavailable::FailOpen => "fail-open",
            },
        );
    }
    for categories in [
        vec!["undeclared".into()],
        vec!["injection".into(), "injection".into()],
    ] {
        let c = compiled(&definition());
        let snap = snapshot_text(CANARY);
        let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
        supply(
            &mut ev,
            "screen:caller_text:0",
            &[],
            Supplied::Output(RawOutput::Screen {
                blocked: true,
                categories,
                units: ScreenUnits::NotReported,
            }),
        );
        drain(&mut ev);
        let (j, e) = ev.finish("screen-decision").unwrap();
        assert!(!matches!(j.outcome, Outcome::Refused { .. }));
        assert!(matches!(
            &e.screening.as_ref().unwrap()[0].verdict,
            ScreenVerdict::Unavailable {
                reason: Unresolved::InvalidBackendOutput { .. }
            }
        ));
    }
}
#[test]
fn pass_evidence_and_digest() {
    let c = compiled(&definition());
    let snap = snapshot_text(CANARY);
    let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
    drain(&mut ev);
    let (_, e) = ev.finish("screen-decision").unwrap();
    let screens = e.screening.as_ref().unwrap();
    assert_eq!(screens.len(), 4);
    assert_eq!(screens[0].text_digest, screen_text_digest(CANARY));
    assert!(screens.iter().all(|e| e.verdict == ScreenVerdict::Pass));
    assert_eq!(e.schema, schema::EVIDENCE_V2);
    documents(&e, "pass");
}

#[test]
fn unavailable_is_per_instance_and_exact_copies_keep_markers() {
    let mut d = definition();
    d.steps.push(StepDecl {
        id: "ids".into(),
        body: StepBody::Exact(ExactDecl {
            op: "filter_with_reasons".into(),
            version: 1,
            args: json!({"list":"input:candidates","id_field":"id","rules":[]}),
        }),
    });
    model(&mut d, &["bind:item"]);
    let StepBody::Semantic(decl) = &mut d.steps[1].body else {
        unreachable!()
    };
    decl.for_each = vec![ForEach {
        over: "step:ids".into(),
        bind: "item".into(),
        items: ItemSource::List {
            list: "input:candidates".into(),
            id_field: "id".into(),
        },
    }];
    let c = compiled(&d);
    let snap = snapshot_text("");
    let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
    supply(&mut ev, "screen:behaviour:0", &[], output(false));
    supply(
        &mut ev,
        "screen:catalogue:0",
        &["a"],
        Supplied::Failed(Unresolved::DeadlineExceeded),
    );
    supply(&mut ev, "screen:catalogue:0", &["b"], output(false));
    let pending = ev.pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].instance, strings(&["b"]));
    for r in pending {
        assert!(!String::from_utf8_lossy(&r.projection).contains(CANARY));
    }
    drain(&mut ev);
    ev.finish("per-item").unwrap();

    let mut d = definition();
    d.steps.push(StepDecl {
        id: "first".into(),
        body: StepBody::Exact(ExactDecl {
            op: "merge_fields".into(),
            version: 1,
            args: json!({"base":"input:request","overlay":"input:request","rules":[{"field":"text","rule":"base_only"},{"field":"behaviour_text","rule":"base_only"}]}),
        }),
    });
    model(&mut d, &["step:first"]);
    let c = compiled(&d);
    let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
    assert_eq!(
        ev.exact_value("first").unwrap().to_json()["behaviour_text"],
        json!("behaviour")
    );
    supply(&mut ev, "screen:behaviour:0", &[], output(true));
    supply(&mut ev, "screen:catalogue:0", &["a"], output(false));
    supply(&mut ev, "screen:catalogue:0", &["b"], output(false));
    let p: serde_json::Value = serde_json::from_slice(&ev.pending()[0].projection).unwrap();
    assert_eq!(p["values"]["step:first"]["text"], "");
    assert_eq!(ev.exact_value("first").unwrap().to_json()["text"], "");
    assert_eq!(
        p["values"]["step:first"]["behaviour_text"],
        withheld_screen_blocked()
    );
    assert_eq!(
        ev.exact_value("first").unwrap().to_json()["behaviour_text"],
        json!("behaviour")
    );
}

#[test]
fn unavailable_refusing_text_has_screening_backend_cause_and_truncation() {
    let mut d = definition();
    model(&mut d, &["input:request"]);
    let mut fallback = d.policy.rules[0].clone();
    fallback.id = "fallback".into();
    d.policy.rules.push(fallback);
    d.policy.rules[0].when = Cond::TopLabel {
        step: "model".into(),
        label: "a".into(),
    };
    d.policy.on_unresolved = serde_json::from_value(
        json!([{"target":"step:model","reasons":"any","action":"propagate"}]),
    )
    .unwrap();
    let c = compiled(&d);
    let snap = snapshot_text(CANARY);
    let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
    ev.note_failure(
        "screen:caller_text:0",
        &[],
        rustev_core::FailureContext {
            origin: rustev_contract::actionability::Origin::Host,
            determinism: Determinism::Bitwise,
        },
    );
    supply(
        &mut ev,
        "screen:caller_text:0",
        &[],
        Supplied::Failed(Unresolved::DeadlineExceeded),
    );
    drain(&mut ev);
    let (j, _) = ev.finish("outage").unwrap();
    let Outcome::UnresolvedAction {
        reason: Unresolved::ScreenUnavailable { fields },
        action,
    } = j.outcome
    else {
        panic!("{j:?}")
    };
    assert_eq!(fields, strings(&["input:request/text"]));
    assert_eq!(action.addressed_to, Party::Host);
    assert_eq!(
        action.caused_by,
        vec![rustev_contract::actionability::Cause::Backend {
            step: "screen:caller_text:0".into(),
            origin: rustev_contract::actionability::Origin::Host
        }]
    );

    let mut d = definition();
    d.limits.on_excess = ExcessPolicy::TruncateVisible;
    d.limits.max_semantic_requests = 1;
    let c = compiled(&d);
    let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
    assert_eq!(ev.pending().len(), 1);
    drain(&mut ev);
    let (_, e) = ev.finish("truncated").unwrap();
    assert_eq!(
        e.screening
            .as_ref()
            .unwrap()
            .iter()
            .filter(|e| matches!(
                e.verdict,
                ScreenVerdict::Unavailable {
                    reason: Unresolved::BudgetExhausted { .. }
                }
            ))
            .count(),
        3
    );
}

#[test]
fn start_checks_win() {
    let mut d = definition();
    d.actionability.as_mut().unwrap().checks = serde_json::from_value(json!([{
        "id":"stop", "subject":"request", "test":{"all":[]},
        "on_fail":{"by_provenance":{},"default":"none"}
    }]))
    .unwrap();
    // An existing terminal check must prevent screening from starting.
    let mut value = serde_json::to_value(&d).unwrap();
    value["actionability"]["checks"][0]["test"] = json!({"not":{"all":[]}});
    value["actionability"]["checks"][0]["on_fail"]["default"] = json!("abstain");
    let d = Definition::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    let c = compiled(&d);
    let snap = snapshot_text(CANARY);
    let ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
    assert!(ev.pending().is_empty());
    let (j, e) = ev.finish("check-first").unwrap();
    assert!(!matches!(j.outcome, Outcome::Propose { .. }));
    assert!(e.screening.is_none());
    EvidenceRecord::parse(&e.record_canonical().unwrap()).unwrap();
}
