//! Scripted supplies for the pure spec 036 evaluator.
mod common;
use rustev_contract::definition::{Definition, StepBody};
use rustev_contract::evidence::StepStatus;
use rustev_contract::judgment::Unresolved;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified};
use rustev_core::{Compiled, Evaluation, compile_with_sources};
use serde_json::{Value, json};

fn source() -> SourceDescriptor {
    serde_json::from_value(
        json!({"schema":"rustev.source/1","name":"catalog","version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges",
        "request":[{"name":"q","type":{"text":{"max_bytes":512}},"required":true},
                   {"name":"optional","type":"integer","required":false}],"max_request_bytes":2048,
        "item":[{"name":"id","type":"integer","provenance":"system-of-record",
        "max_grade":"cached","reliability":"reliable","freshness":"not_required"}],
        "max_page_items":4,"max_page_bytes":1024,"paging":"offset",
        "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]}),
    )
    .unwrap()
}
fn definition() -> Definition {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]})).unwrap();
    d.steps = vec![serde_json::from_value(json!({"id":"lookup","body":{"acquire":{
        "source":"catalog","operation":"search","request":{"q":{"lit":{"text":"tea"}}},"when":"always",
        "limits":{"max_pages":2,"max_items":5,"max_attempts":1,"retry_on":[],"delay":"none","deadline_share":"0.2","as_of_skew_ms":100},
        "stop":"none","on_failure":"unresolved"}}})).unwrap()];
    d.limits.acquisition = Some(
        serde_json::from_value(json!({"max_calls":8,"max_items":20,"max_bytes":16384})).unwrap(),
    );
    d
}
fn compile(d: &Definition) -> Compiled {
    compile_with_sources(d, &[], &[], &[source()], None).unwrap()
}
fn snapshot() -> Snapshot {
    serde_json::from_value(json!({"schema":"rustev.snapshot/1","entries":[]})).unwrap()
}
fn start(c: &Compiled) -> Evaluation<'_> {
    Evaluation::start(c, &snapshot(), Timestamp::from_ms(1000).unwrap()).unwrap()
}
fn page(e: &Evaluation<'_>, step: &str, ids: &[i64], next: Value) -> Value {
    let p = e
        .pending_acquisitions()
        .into_iter()
        .find(|p| p.step == step)
        .unwrap();
    assert_eq!(p.request.id().unwrap(), p.request_id);
    json!({"schema":"rustev.acquisition-page/1","request":p.request_id,"as_of_ms":900,"items":ids.iter().map(|id| json!({"id":id})).collect::<Vec<_>>(),"next":next})
}
fn supply(e: &mut Evaluation<'_>, step: &str, p: Value) {
    let pending = e
        .pending_acquisitions()
        .into_iter()
        .find(|p| p.step == step)
        .unwrap();
    e.supply_page(
        step,
        pending.tier_index,
        pending.page_index,
        &serde_json::to_vec(&p).unwrap(),
    )
    .unwrap();
}
fn acquisition(d: &mut Definition) -> &mut rustev_contract::acquisition_step::Acquire {
    let StepBody::Acquire(a) = &mut d.steps[0].body else {
        panic!()
    };
    a
}
fn exact(id: &str, op: &str, args: Value) -> rustev_contract::definition::StepDecl {
    serde_json::from_value(json!({"id":id,"body":{"exact":{"op":op,"version":1,"args":args}}}))
        .unwrap()
}

#[test]
fn downstream_exact_preserves_acquisition_request_and_guard_lineage() {
    let mut d = definition();
    let mut input = common::support_routing().inputs.remove(0);
    input.ty = rustev_contract::definition::TypeDecl::Integer;
    input.name = "query".into();
    d.inputs.push(input.clone());
    input.name = "guard_input".into();
    d.inputs.push(input);
    let a = acquisition(&mut d);
    a.request.insert(
        "optional".into(),
        serde_json::from_value(json!({"ref":"step:request_value"})).unwrap(),
    );
    a.when = serde_json::from_value(json!({"cmp":{
        "left":{"ref":"step:guard_value"},"op":"gt","right":{"lit":{"integer":0}}
    }}))
    .unwrap();
    for (id, input) in [("request_value", "query"), ("guard_value", "guard_input")] {
        d.steps.push(exact(
            id,
            "arith",
            json!({"expr":{"ref":format!("input:{input}")}}),
        ));
    }
    d.steps.push(exact(
        "count",
        "arith",
        json!({"expr":{"len":{"ref":"step:lookup/items"}}}),
    ));
    let mut rule = d.policy.rules[0].clone();
    rule.id = "has_items".into();
    rule.when = serde_json::from_value(json!({"exact":{"cmp":{
        "left":{"ref":"step:count"},"op":"eq","right":{"lit":{"integer":2}}
    }}}))
    .unwrap();
    d.policy.rules.insert(0, rule);
    d.policy.on_unresolved = serde_json::from_value(json!([{
        "target":"step:count", "reasons":"any", "action":{"escalate":{"reason":"failure"}}
    }]))
    .unwrap();
    let c = compile(&d);
    let snapshot = common::snapshot(
        ["query", "guard_input"]
            .into_iter()
            .map(|name| {
                common::entry(
                    name,
                    json!(3),
                    rustev_contract::definition::ProvenanceClass::UserSupplied,
                    900,
                )
            })
            .collect(),
    );
    let mut e = Evaluation::start(&c, &snapshot, Timestamp::from_ms(1000).unwrap()).unwrap();
    assert!(e.exact_value("count").is_none());
    let p = page(&e, "lookup", &[1], json!({"offset":{"integer":1}}));
    supply(&mut e, "lookup", p);
    assert!(e.exact_value("count").is_none());
    let p = page(&e, "lookup", &[2], json!("none"));
    supply(&mut e, "lookup", p);
    assert_eq!(e.exact_value("count").unwrap().to_json(), 2);
    let (j, ev) = e.finish("lineage").unwrap();
    // Spec 036 3.4.6: the acquisition plus transitive guard and request reads.
    assert_eq!(j.lineage.inputs, vec!["guard_input", "query"]);
    assert_eq!(
        j.lineage.steps,
        vec!["count", "guard_value", "lookup", "request_value"]
    );
    let acquisition = &ev.acquisitions.as_ref().unwrap()[0];
    assert_eq!(acquisition.step, "lookup");
    assert_eq!(acquisition.tiers[0].source, "catalog");
    assert_eq!(acquisition.tiers[0].operation, "search");
    assert_eq!(acquisition.items.len(), 2);
    for item in &acquisition.items {
        assert_eq!(
            item.meta.basis,
            rustev_contract::grade::Basis::Acquired {
                step: "lookup".into()
            }
        );
        assert_eq!(item.meta.sources, vec!["catalog"]);
    }
}

#[test]
fn paging_stops_and_counts_truncation() {
    for (pages, items, stop, next, expected) in [
        (2, 5, json!("none"), json!("none"), "exhausted"),
        (
            2,
            5,
            json!({"items_at_least":{"count":{"lit":{"integer":2}}}}),
            json!({"offset":{"integer":3}}),
            "stop_condition",
        ),
        (
            1,
            5,
            json!("none"),
            json!({"offset":{"integer":3}}),
            "page_limit",
        ),
        (
            2,
            2,
            json!("none"),
            json!({"offset":{"integer":3}}),
            "item_limit",
        ),
    ] {
        let mut d = definition();
        let a = acquisition(&mut d);
        a.limits.max_pages = pages;
        a.limits.max_items = items;
        a.stop = serde_json::from_value(stop).unwrap();
        let c = compile(&d);
        let mut e = start(&c);
        assert!(start(&c).finish("pending").is_err());
        let p = page(&e, "lookup", &[1, 2, 3], next);
        supply(&mut e, "lookup", p);
        assert!(e.pending_acquisitions().is_empty());
        let v = e.exact_value("lookup").unwrap().to_json();
        assert_eq!(v["stopped_by"], expected);
        assert_eq!(v["truncated"], if items == 2 { 1 } else { 0 });
        let (j, ev) = e.finish("done").unwrap();
        assert_eq!(j.notices.len(), usize::from(items == 2));
        let a = &ev.acquisitions.unwrap()[0];
        assert_eq!(a.items.len(), items.min(3) as usize);
        assert_eq!(
            a.items[0].meta.grade,
            rustev_contract::grade::ValueGrade::Cached
        );
    }
    let c = compile(&definition());
    let mut e = start(&c);
    let p = page(&e, "lookup", &[1], json!({"offset":{"integer":1}}));
    supply(&mut e, "lookup", p);
    assert_eq!(e.pending_acquisitions()[0].page_index, 1);
    let p = page(&e, "lookup", &[2], json!("none"));
    supply(&mut e, "lookup", p);
    assert_eq!(
        e.exact_value("lookup").unwrap().to_json()["items"],
        json!([{"id":1},{"id":2}])
    );
}

#[test]
fn invalid_pages_never_become_values() {
    for (pointer, value) in [
        ("/request", json!(format!("sha256:{}", "a".repeat(64)))),
        (
            "/items",
            json!([{"id":1},{"id":2},{"id":3},{"id":4},{"id":5}]),
        ),
        ("/items/0", json!({})),
        ("/items/0", json!({"id":1,"extra":1})),
        ("/items/0/id", json!("wrong")),
        ("/next", json!({"cursor":{"text":"x"}})),
        ("/next", json!({"offset":{"integer":3}})),
        ("/as_of_ms", json!(1101)),
        ("/schema", json!("rustev.acquisition-page/2")),
    ] {
        let c = compile(&definition());
        let mut e = start(&c);
        let mut p = page(&e, "lookup", &[1], json!("none"));
        *p.pointer_mut(pointer).unwrap() = value;
        supply(&mut e, "lookup", p);
        let (_, ev) = e.finish("bad").unwrap();
        assert!(
            matches!(
                ev.steps[0].status,
                StepStatus::Unresolved(Unresolved::InvalidBackendOutput { .. })
            ),
            "{pointer}"
        );
        assert!(ev.acquisitions.unwrap()[0].items.is_empty());
    }
}

#[test]
fn guard_skip_propagates_and_probes_are_false() {
    for truth in [false, true] {
        let mut d = definition();
        acquisition(&mut d).when=serde_json::from_value(json!({"cmp":{"left":{"lit":{"integer":1}},"op":"eq","right":{"lit":{"integer":if truth {1} else {2}}}}})).unwrap();
        d.steps.push(exact(
            "copy",
            "arith",
            json!({"expr":{"len":{"ref":"step:lookup/items"}}}),
        ));
        for (id, predicate) in [
            ("present", json!({"present":"step:lookup"})),
            (
                "constructed",
                json!({"is_constructed":{"ref":"step:lookup"}}),
            ),
        ] {
            d.steps
                .push(exact(id, "compare", json!({"predicate":predicate})));
        }
        let c = compile(&d);
        let mut e = start(&c);
        if truth {
            let p = page(&e, "lookup", &[1], json!("none"));
            supply(&mut e, "lookup", p);
        } else {
            assert!(e.pending_acquisitions().is_empty());
        }
        assert_eq!(e.exact_value("present").unwrap().to_json(), json!(truth));
        assert_eq!(
            e.exact_value("constructed").unwrap().to_json(),
            json!(false)
        );
        let (_, ev) = e.finish("guard").unwrap();
        if !truth {
            assert_eq!(
                ev.steps[0].status,
                StepStatus::Skipped {
                    guard: "lookup".into()
                }
            );
            assert_eq!(ev.steps[1].status, ev.steps[0].status);
        }
    }
}

#[test]
fn acquisition_request_from_composed_key_is_constructed() {
    let mut d = definition();
    acquisition(&mut d).request.insert(
        "q".into(),
        serde_json::from_value(json!({"ref":"step:key"})).unwrap(),
    );
    d.steps.insert(0,exact("key","compose_key",json!({"parts":[{"value":{"lit":{"text":"A"}},"normalize":"ascii_token"},{"value":{"lit":{"text":"B"}},"normalize":"exact"}],"separator":"/"})));
    let c = compile(&d);
    let mut e = start(&c);
    assert_eq!(
        serde_json::to_value(&e.pending_acquisitions()[0].request.fields).unwrap(),
        json!([{"set":{"value":"a/B"}},"unset"])
    );
    let p = page(&e, "lookup", &[1], json!("none"));
    supply(&mut e, "lookup", p);
    assert_eq!(
        e.exact_value("lookup").unwrap().to_json()["request_constructed"],
        true
    );
    let (_, ev) = e.finish("key").unwrap();
    assert!(!ev.steps[1].constructed);
}

#[test]
fn independent_pages_are_byte_identical_in_every_interleaving() {
    fn schedules(prefix: Vec<usize>, counts: [u8; 3], out: &mut Vec<Vec<usize>>) {
        if counts == [2; 3] {
            out.push(prefix);
            return;
        }
        for i in 0..3 {
            if counts[i] < 2 {
                let mut p = prefix.clone();
                p.push(i);
                let mut c = counts;
                c[i] += 1;
                schedules(p, c, out);
            }
        }
    }
    let mut d = definition();
    let a = d.steps[0].clone();
    d.steps = (0..3)
        .map(|i| {
            let mut a = a.clone();
            a.id = format!("lookup{i}");
            a
        })
        .collect();
    let c = compile(&d);
    let mut orders = vec![];
    schedules(vec![], [0; 3], &mut orders);
    assert_eq!(orders.len(), 90);
    let mut baseline = None;
    for order in orders {
        let mut e = start(&c);
        let mut seen = [0; 3];
        for i in order {
            let step = format!("lookup{i}");
            let next = if seen[i] == 0 {
                json!({"offset":{"integer":1}})
            } else {
                json!("none")
            };
            let p = page(&e, &step, &[i as i64], next);
            supply(&mut e, &step, p);
            seen[i] += 1;
        }
        let (j, ev) = e.finish("same").unwrap();
        let bytes = (j.canonical().unwrap(), ev.canonical().unwrap());
        if let Some(b) = &baseline {
            assert_eq!(b, &bytes);
        } else {
            baseline = Some(bytes);
        }
    }
}

#[test]
fn failures_roll_back_and_supply_coordinates_are_checked() {
    let c = compile(&definition());
    for reason in [
        Unresolved::missing(["lookup".into()]),
        Unresolved::BackendUnavailable {
            detail: "source:catalog unavailable".into(),
        },
        Unresolved::InvalidBackendOutput {
            detail: "invalid".into(),
        },
        Unresolved::BudgetExhausted {
            resource: "acquisition_calls".into(),
        },
        Unresolved::BudgetExhausted {
            resource: "cost".into(),
        },
        Unresolved::DeadlineExceeded,
    ] {
        let mut e = start(&c);
        let p = page(&e, "lookup", &[1], json!({"offset":{"integer":1}}));
        supply(&mut e, "lookup", p);
        assert!(
            e.supply_acquisition_failure("lookup", 0, 0, reason.clone())
                .is_err()
        );
        assert!(
            e.supply_acquisition_failure("lookup", 1, 1, reason.clone())
                .is_err()
        );
        e.supply_acquisition_failure("lookup", 0, 1, reason.clone())
            .unwrap();
        assert!(e.exact_value("lookup").is_none());
        let (_, ev) = e.finish("failed").unwrap();
        assert_eq!(ev.steps[0].status, StepStatus::Unresolved(reason));
        let a = &ev.acquisitions.unwrap()[0];
        assert!(a.items.is_empty());
        assert_eq!(a.tiers[0].pages.len(), 2);
        assert_eq!(a.tiers[0].unique_items_added, 0);
    }
    let mut e = start(&c);
    for reason in [
        Unresolved::missing(["other".into()]),
        Unresolved::invalid([], "invalid"),
        Unresolved::BackendUnavailable {
            detail: "source:catalogue unavailable".into(),
        },
        Unresolved::BudgetExhausted {
            resource: "semantic_calls".into(),
        },
    ] {
        assert!(
            e.supply_acquisition_failure("lookup", 0, 0, reason)
                .is_err()
        );
    }
    let p = page(&e, "lookup", &[1], json!({"offset":{"integer":1}}));
    let bytes = serde_json::to_vec(&p).unwrap();
    assert!(e.supply_page("lookup", 1, 0, &bytes).is_err());
    supply(&mut e, "lookup", p);
    assert!(e.supply_page("lookup", 0, 0, &bytes).is_err());
    e.supply_page("lookup", 0, 1, b"not json").unwrap();
    let (_, ev) = e.finish("invalid").unwrap();
    assert!(ev.acquisitions.unwrap()[0].items.is_empty());
}

#[test]
fn freshness_boundary_and_remaining_page_validation_rules() {
    for (age, stale) in [(100, false), (101, true)] {
        let mut src = source();
        src.operations.as_mut().unwrap()[0].item[0].freshness =
            rustev_contract::acquisition::Freshness::MaxAgeMs { value: 100 };
        let c = compile_with_sources(&definition(), &[], &[], &[src], None).unwrap();
        let mut e = start(&c);
        let mut p = page(&e, "lookup", &[1], json!("none"));
        p["as_of_ms"] = json!(1000 - age);
        supply(&mut e, "lookup", p);
        let (_, ev) = e.finish("freshness").unwrap();
        if stale {
            assert_eq!(
                ev.steps[0].status,
                StepStatus::Unresolved(Unresolved::stale(["lookup/id".into()]))
            );
        } else {
            assert!(matches!(ev.steps[0].status, StepStatus::Resolved { .. }));
        }
    }
    for (paging, next, bytes) in [
        (
            json!({"cursor":{"max_cursor_bytes":2}}),
            json!({"cursor":{"text":"long"}}),
            1024,
        ),
        (json!("none"), json!({"offset":{"integer":1}}), 1024),
        (json!("offset"), json!("none"), 128),
    ] {
        let mut src = source();
        src.operations.as_mut().unwrap()[0].paging = serde_json::from_value(paging).unwrap();
        src.operations.as_mut().unwrap()[0].max_page_bytes = bytes;
        let mut d = definition();
        acquisition(&mut d).limits.max_pages = 1;
        let c = compile_with_sources(&d, &[], &[], &[src], None).unwrap();
        let mut e = start(&c);
        let p = page(&e, "lookup", &[1], next);
        supply(&mut e, "lookup", p);
        let (_, ev) = e.finish("invalid").unwrap();
        assert!(matches!(
            ev.steps[0].status,
            StepStatus::Unresolved(Unresolved::InvalidBackendOutput { .. })
        ));
    }
}

#[test]
fn unresolved_guards_and_requests_propagate_without_dispatch() {
    for guard in [false, true] {
        let mut d = definition();
        let bad = exact(
            "bad",
            "compose_key",
            json!({"parts":[{"value":{"lit":{"text":""}},"normalize":"exact"},{"value":{"lit":{"integer":1}},"normalize":"exact"}],"separator":":"}),
        );
        if guard {
            acquisition(&mut d).when =
                serde_json::from_value(json!({"is_constructed":{"ref":"step:bad"}})).unwrap();
        } else {
            acquisition(&mut d).request.insert(
                "q".into(),
                serde_json::from_value(json!({"ref":"step:bad"})).unwrap(),
            );
        }
        d.steps.insert(0, bad);
        let c = compile(&d);
        let e = start(&c);
        assert!(e.pending_acquisitions().is_empty());
        let (_, ev) = e.finish("unresolved").unwrap();
        assert_eq!(ev.steps[0].status, ev.steps[1].status);
    }
    // The guard is evaluated before a faulty request expression.
    let mut d = definition();
    acquisition(&mut d).when = serde_json::from_value(
        json!({"cmp":{"left":{"lit":{"integer":1}},"op":"eq","right":{"lit":{"integer":2}}}}),
    )
    .unwrap();
    acquisition(&mut d).request.insert(
        "optional".into(),
        serde_json::from_value(
            json!({"add":{"left":{"lit":{"integer":9223372036854775807i64}},"right":{"lit":{"integer":1}}}}),
        )
        .unwrap(),
    );
    let c = compile(&d);
    let (_, ev) = start(&c).finish("guard_first").unwrap();
    assert!(matches!(ev.steps[0].status, StepStatus::Skipped { .. }));
}

#[test]
fn future_source_script_is_only_a_page_supplier() {
    use rustev_contract::run::{Charge, CostBound};
    use rustev_core::seams::*;
    struct Script(SourceDescriptor);
    impl AcquisitionSource for Script {
        fn descriptor(&self) -> &SourceDescriptor {
            &self.0
        }
        fn cost_bound(&self, _: &rustev_contract::acquisition::AcquisitionRequest) -> CostBound {
            CostBound::Bounded { max_units: 3 }
        }
        fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
            Box::pin(async move {
                assert_eq!(&call.request.id().unwrap(), call.request_id);
                AcquireReport { result: Ok(serde_json::to_vec(&json!({"schema":"rustev.acquisition-page/1","request":call.request_id,"as_of_ms":900,"items":[],"next":"none"})).unwrap()),charge:Charge::Unknown,cancel:CancelAck::NotRequested }
            })
        }
    }
    let c = compile(&definition());
    let mut e = start(&c);
    let p = e.pending_acquisitions().remove(0);
    let cx = CallContext {
        remaining_ms: rustev_contract::time::DurationMs(100),
        trace_id: "trace".into(),
        principal_handle: vec![],
    };
    let script = Script(source());
    let source: &dyn AcquisitionSource = &script;
    let cancel = CancelSignal::new();
    let mut future = source.acquire(AcquireCall {
        operation: &p.operation,
        request: &p.request,
        request_id: &p.request_id,
        attempt_id: "decision/lookup/0,0/0",
        cancel: cancel.child(),
        cx: &cx,
    });
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(report) = future.as_mut().poll(&mut context) else {
        panic!("scripted page must be ready on its first poll")
    };
    e.supply_page(&p.step, p.tier_index, p.page_index, &report.result.unwrap())
        .unwrap();
    assert!(e.finish("script").is_ok());
}

#[test]
fn skipped_policy_handlers_and_probe_guards() {
    use rustev_contract::definition::{Cond, Handler, HandlerAction, Predicate, ReasonSet};
    use rustev_contract::judgment::Outcome;
    let mut d = definition();
    acquisition(&mut d).when = serde_json::from_value(
        json!({"cmp":{"left":{"lit":{"integer":1}},"op":"eq","right":{"lit":{"integer":2}}}}),
    )
    .unwrap();
    for action in [
        HandlerAction::AsUnmet,
        HandlerAction::Escalate {
            reason: "skipped path".into(),
        },
    ] {
        let mut d = d.clone();
        d.policy.on_unresolved = vec![Handler {
            target: "step:lookup".into(),
            reasons: ReasonSet::Any,
            action: action.clone(),
        }];
        let mut first = d.policy.rules[0].clone();
        first.id = "has_items".into();
        first.when = Cond::Nonempty("step:lookup/items".into());
        d.policy.rules.insert(0, first);
        let c = compile(&d);
        let (j, _) = start(&c).finish("policy").unwrap();
        assert_eq!(
            j.outcome,
            Outcome::Escalate {
                reason: if matches!(action, HandlerAction::AsUnmet) {
                    "review"
                } else {
                    "skipped path"
                }
                .into()
            }
        );
    }
    for probe in [
        Predicate::Present("step:lookup".into()),
        Predicate::IsConstructed {
            reference: "step:lookup".into(),
        },
    ] {
        let mut d = d.clone();
        let mut second = d.steps[0].clone();
        second.id = "second".into();
        let StepBody::Acquire(a) = &mut second.body else {
            panic!()
        };
        a.when = rustev_contract::acquisition_step::Guard::Predicate(Predicate::Not(Box::new(
            probe.clone(),
        )));
        d.steps.push(second);
        d.policy.on_unresolved = vec![Handler {
            target: "step:lookup".into(),
            reasons: ReasonSet::Only(
                rustev_contract::definition::ReasonKind::ALL
                    .into_iter()
                    .filter(|k| *k != rustev_contract::definition::ReasonKind::Skipped)
                    .collect(),
            ),
            action: HandlerAction::Escalate {
                reason: "failure".into(),
            },
        }];
        let mut first = d.policy.rules[0].clone();
        first.id = "probe".into();
        first.when = Cond::Exact(probe);
        d.policy.rules.insert(0, first);
        let c = compile(&d);
        let mut e = start(&c);
        assert_eq!(e.pending_acquisitions()[0].step, "second");
        let p = page(&e, "second", &[], json!("none"));
        supply(&mut e, "second", p);
        let (j, _) = e.finish("probes").unwrap();
        assert_eq!(
            j.outcome,
            Outcome::Escalate {
                reason: "review".into()
            }
        );
    }
}

#[test]
fn cursor_identity_and_oldest_retained_metadata() {
    let mut src = source();
    src.operations.as_mut().unwrap()[0].paging = rustev_contract::acquisition::Paging::Cursor {
        max_cursor_bytes: 16,
    };
    let c = compile_with_sources(&definition(), &[], &[], &[src], None).unwrap();
    let mut e = start(&c);
    let first = e.pending_acquisitions()[0].request_id.clone();
    let p = page(&e, "lookup", &[1], json!({"cursor":{"text":"next"}}));
    supply(&mut e, "lookup", p);
    assert_ne!(first, e.pending_acquisitions()[0].request_id);
    let mut p = page(&e, "lookup", &[2], json!("none"));
    p["as_of_ms"] = json!(800);
    supply(&mut e, "lookup", p);
    assert_eq!(e.exact_value("lookup").unwrap().to_json()["as_of_ms"], 800);
    let (_, ev) = e.finish("cursor").unwrap();
    let meta = ev.steps[0].meta.as_ref().unwrap();
    assert_eq!(
        meta.as_of_ms,
        rustev_contract::grade::AsOf::Ms(Timestamp::from_ms(800).unwrap())
    );
    assert_eq!(meta.sources, vec!["catalog"]);
}

#[test]
fn false_guard_does_not_wait_for_request_dependencies() {
    let mut d = definition();
    let mut second = d.steps[0].clone();
    second.id = "second".into();
    let StepBody::Acquire(a) = &mut second.body else {
        panic!()
    };
    a.when = serde_json::from_value(
        json!({"cmp":{"left":{"lit":{"integer":1}},"op":"eq","right":{"lit":{"integer":2}}}}),
    )
    .unwrap();
    a.request.insert(
        "optional".into(),
        serde_json::from_value(json!({"ref":"step:lookup/truncated"})).unwrap(),
    );
    d.steps.push(second);
    d.steps.push(exact(
        "probe",
        "compare",
        json!({"predicate":{"present":"step:second"}}),
    ));
    let c = compile(&d);
    let mut e = start(&c);
    assert_eq!(e.pending_acquisitions().len(), 1);
    assert_eq!(e.exact_value("probe").unwrap().to_json(), false);
    e.supply_acquisition_failure("lookup", 0, 0, Unresolved::DeadlineExceeded)
        .unwrap();
    let (_, ev) = e.finish("early_guard").unwrap();
    assert_eq!(
        ev.steps[1].status,
        StepStatus::Skipped {
            guard: "second".into()
        }
    );
}

#[test]
fn skipped_projection_issues_no_semantic_request() {
    let mut d = definition();
    acquisition(&mut d).when = serde_json::from_value(
        json!({"cmp":{"left":{"lit":{"integer":1}},"op":"eq","right":{"lit":{"integer":2}}}}),
    )
    .unwrap();
    let mut semantic = common::support_routing()
        .steps
        .into_iter()
        .find(|s| s.id == "explicit_deadline")
        .unwrap();
    let StepBody::Semantic(s) = &mut semantic.body else {
        panic!()
    };
    s.project = vec!["step:lookup/items".into()];
    d.steps.push(semantic);
    let c = compile_with_sources(&d, &common::descriptors(), &[], &[source()], None).unwrap();
    let e = start(&c);
    assert!(e.pending().is_empty());
    assert!(e.pending_acquisitions().is_empty());
    let (_, ev) = e.finish("semantic_skip").unwrap();
    assert_eq!(ev.steps[1].status, ev.steps[0].status);

    acquisition(&mut d).when = serde_json::from_value(json!("always")).unwrap();
    let c = compile_with_sources(&d, &common::descriptors(), &[], &[source()], None).unwrap();
    let mut e = start(&c);
    let p = page(&e, "lookup", &[1], json!("none"));
    supply(&mut e, "lookup", p);
    let pending = e.pending();
    assert_eq!(pending.len(), 1);
    let projection: Value = serde_json::from_slice(&pending[0].projection).unwrap();
    assert_eq!(projection["values"]["step:lookup/items"], json!([{"id":1}]));
}
