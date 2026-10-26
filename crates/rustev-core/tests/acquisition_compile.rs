//! Synthetic compile-only coverage for spec 036.
mod common;
use rustev_contract::Document;
use rustev_contract::definition::{Definition, StepBody};
use rustev_contract::source::SourceDescriptor;
use rustev_core::{Category, Compiled, Refusal, compile_with_sources};
use serde_json::{Value, json};

fn source(name: &str) -> SourceDescriptor {
    serde_json::from_value(
        json!({"schema":"rustev.source/1","name":name,"version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges",
        "request":[{"name":"q","type":{"text":{"max_bytes":64}},"required":true},
                   {"name":"optional","type":"integer","required":false}],
        "max_request_bytes":1024,
        "item":[{"name":"id","type":"integer","provenance":"system-of-record",
        "max_grade":"cached","reliability":"reliable","freshness":"not_required"},
        {"name":"unused","type":"integer","provenance":"model-derived",
        "max_grade":"cached","reliability":"reliable","freshness":"not_required"}],
        "max_page_items":10,"max_page_bytes":1024,"paging":"offset",
        "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]}),
    )
    .unwrap()
}
fn base(widen: bool) -> Definition {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[
        {"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]})).unwrap();
    let limits = json!({"max_pages":2,"max_items":10,"max_attempts":2,
        "retry_on":["transient"],"delay":"none","deadline_share":"0.4","as_of_skew_ms":100});
    let body = if widen {
        json!({"widen":{"item":{"fields":[{"name":"id","type":"integer"}]},
            "sources":[{"source":"catalog","operation":"search","map":{"id":"id"}},
                       {"source":"other","operation":"search","map":{"id":"id"}}],
            "base_request":{"q":{"lit":{"text":"tea"}},"optional":{"lit":{"integer":1}}},
            "groups":{"query":["q"],"extra":["optional"]},"protected":["query"],
            "tiers":[{"name":"specific","source_index":0,"request_patch":{},"when":"always","relax":[]},
                     {"name":"broad","source_index":1,"request_patch":{},"when":"always","relax":["extra"]}],
            "max_tiers":2,"dedupe_key":"id","order":"tier_then_source_rank","stop":"none",
            "on_tier_failure":"continue","limits":limits}})
    } else {
        json!({"acquire":{"source":"catalog","operation":"search","request":{"q":{"lit":{"text":"tea"}}},
            "when":"always","limits":limits,"stop":"none","on_failure":"unresolved"}})
    };
    d.steps = vec![serde_json::from_value(json!({"id":"lookup","body":body})).unwrap()];
    d.limits.acquisition = Some(
        serde_json::from_value(json!({"max_calls":8,"max_items":10,"max_bytes":8192})).unwrap(),
    );
    d
}
fn run(d: &Definition) -> Result<Compiled, Refusal> {
    compile_with_sources(d, &[], &[], &[source("catalog"), source("other")], None)
}
fn mutate(widen: bool, pointer: &str, value: Value) -> Definition {
    let mut v = serde_json::to_value(base(widen)).unwrap();
    *v.pointer_mut(pointer).unwrap() = value;
    serde_json::from_value(v).unwrap()
}
fn golden(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
fn refusal_golden(name: &str, r: &Refusal) {
    golden(
        name,
        format!("{}\n{}\n{}\n", r.category as u8, r.subject, r.detail).as_bytes(),
    );
}
#[test]
fn acquisitions_compile_roundtrip_and_bind_only_named_sources() {
    for w in [false, true] {
        let d = base(w);
        let c = run(&d).unwrap();
        assert_eq!(
            rustev_contract::plan::Plan::parse(&c.canonical()).unwrap(),
            c.plan
        );
        assert_eq!(Definition::parse(&d.canonical().unwrap()).unwrap(), d);
        assert_eq!(
            c.plan.acquisitions.as_ref().unwrap()[0].max_calls,
            if w { 8 } else { 4 }
        );
        assert_eq!(
            c.plan.sources.as_ref().unwrap().len(),
            if w { 2 } else { 1 }
        );
        golden(
            if w {
                "acquisition-widen.plan.json"
            } else {
                "acquisition-acquire.plan.json"
            },
            &c.canonical(),
        );
        let mut supplied = vec![source("other"), source("catalog"), source("unrelated")];
        assert_eq!(
            c.id,
            compile_with_sources(&d, &[], &[], &supplied, None)
                .unwrap()
                .id
        );
        supplied[1].version = "1.0.1".into();
        assert_ne!(
            c.id,
            compile_with_sources(&d, &[], &[], &supplied, None)
                .unwrap()
                .id
        );
        let mut changed = d.clone();
        changed.limits.acquisition.as_mut().unwrap().max_bytes += 1;
        assert_ne!(c.id, run(&changed).unwrap().id);
    }
}
#[test]
fn protected_groups_required_fields_and_mapping_refusals() {
    for (pointer, value, category) in [
        (
            "/steps/0/body/widen/tiers/1/relax",
            json!(["query"]),
            Category::InvalidAcquisition,
        ),
        (
            "/steps/0/body/widen/tiers/1/relax",
            json!(["unknown"]),
            Category::InvalidAcquisition,
        ),
        (
            "/steps/0/body/widen/tiers/1/request_patch",
            json!({"q":{"lit":{"text":"other"}}}),
            Category::InvalidAcquisition,
        ),
        (
            "/steps/0/body/widen/tiers/1/name",
            json!("specific"),
            Category::InvalidAcquisition,
        ),
        (
            "/steps/0/body/widen/sources/1/map",
            json!({}),
            Category::SourceMapping,
        ),
        (
            "/steps/0/body/widen/sources/1/map/id",
            json!("missing"),
            Category::SourceMapping,
        ),
    ] {
        assert_eq!(
            run(&mutate(true, pointer, value)).unwrap_err().category,
            category,
            "{pointer}"
        );
    }
    let protected = run(&mutate(
        true,
        "/steps/0/body/widen/tiers/1/relax",
        json!(["query"]),
    ))
    .unwrap_err();
    refusal_golden("acquisition-protected.refusal", &protected);
    let mapping = run(&mutate(
        true,
        "/steps/0/body/widen/sources/1/map/id",
        json!("missing"),
    ))
    .unwrap_err();
    refusal_golden("acquisition-mapping.refusal", &mapping);
    for req in [
        json!({}),
        json!({"q":{"lit":{"text":"tea"}},"unknown":{"lit":{"integer":1}}}),
    ] {
        assert_eq!(
            run(&mutate(false, "/steps/0/body/acquire/request", req))
                .unwrap_err()
                .category,
            Category::InvalidAcquisition
        );
    }
    let mut d = base(true);
    if let StepBody::Widen(w) = &mut d.steps[0].body {
        w.protected.clear();
        w.tiers[0].relax = vec!["query".into()];
    }
    assert_eq!(run(&d).unwrap_err().category, Category::InvalidAcquisition);
}
#[test]
fn typed_bounds_bindings_and_retries_are_rechecked() {
    let mut d = base(false);
    if let StepBody::Acquire(a) = &mut d.steps[0].body {
        a.limits.max_pages = 0;
    }
    assert_eq!(run(&d).unwrap_err().category, Category::InvalidAcquisition);
    let mut d = base(false);
    d.limits.acquisition.as_mut().unwrap().max_calls = 4097;
    assert_eq!(run(&d).unwrap_err().category, Category::Parse);
    let d = base(false);
    for supplied in [vec![], vec![source("catalog"), source("catalog")]] {
        let r = compile_with_sources(&d, &[], &[], &supplied, None).unwrap_err();
        assert_eq!(r.category, Category::SourceBinding);
        assert_eq!(r.subject, "step:lookup");
    }
    let mut s = source("catalog");
    s.operations.as_mut().unwrap()[0].idempotent = false;
    assert_eq!(
        compile_with_sources(&d, &[], &[], &[s], None)
            .unwrap_err()
            .category,
        Category::InvalidAcquisition
    );
}
#[test]
fn zero_acquisition_item_and_byte_budgets_parse_but_refuse_compilation() {
    for field in ["max_items", "max_bytes"] {
        let mut wire = serde_json::to_value(base(false)).unwrap();
        wire["limits"]["acquisition"][field] = json!(0);
        let d = Definition::parse(&serde_json::to_vec(&wire).unwrap()).unwrap();
        let r = run(&d).unwrap_err();
        assert_eq!(r.category, Category::Budget, "{field}");
        assert_eq!(r.category as u8, 7);
    }
}

#[test]
fn present_accepts_bare_steps_but_refuses_member_paths() {
    use rustev_contract::definition::{Cond, Handler, HandlerAction, Predicate, ReasonSet};

    let mut d = base(false);
    d.policy.on_unresolved = vec![Handler {
        target: "step:lookup".into(),
        reasons: ReasonSet::Any,
        action: HandlerAction::Escalate {
            reason: "review".into(),
        },
    }];
    let mut checked = d.policy.rules[0].clone();
    checked.id = "checked".into();
    checked.when = Cond::Exact(Predicate::Present("step:lookup".into()));
    d.policy.rules.insert(0, checked);
    run(&d).unwrap();

    for reference in ["step:lookup/items", "input:lookup/items"] {
        d.policy.rules[0].when = Cond::Exact(Predicate::Present(reference.into()));
        let r = run(&d).unwrap_err();
        assert_eq!(r.category, Category::InvalidDefinition);
        assert_eq!(
            r.detail,
            format!("present() takes an input or a bare step reference, not {reference:?}")
        );
    }
}

#[test]
fn every_budget_ceiling_has_a_refusal_golden() {
    for (field, value) in [("max_calls", 7), ("max_items", 9), ("max_bytes", 8191)] {
        let d = mutate(true, &format!("/limits/acquisition/{field}"), json!(value));
        let r = run(&d).unwrap_err();
        assert_eq!(r.category, Category::Budget);
        refusal_golden(&format!("acquisition-{field}.refusal"), &r);
    }
    let mut d = base(false);
    d.steps[0].body = mutate(
        false,
        "/steps/0/body/acquire/limits/deadline_share",
        json!("0.6"),
    )
    .steps
    .remove(0)
    .body;
    let mut second = d.steps[0].clone();
    second.id = "second".into();
    d.steps.push(second);
    d.limits.acquisition.as_mut().unwrap().max_items = 20;
    let r = run(&d).unwrap_err();
    assert_eq!(r.category, Category::Budget);
    refusal_golden("acquisition-deadline_share.refusal", &r);
    let policy = common::execution(
        rustev_contract::execution::CostPolicy::Hard { max_units: 11 },
        vec![],
    );
    let r = compile_with_sources(&base(false), &[], &[], &[source("catalog")], Some(&policy))
        .unwrap_err();
    assert_eq!(r.category, Category::Budget);
    refusal_golden("acquisition-hard-cost.refusal", &r);
}

#[test]
fn guarded_acquisition_requires_skipped_handling_except_for_presence_probes() {
    use rustev_contract::definition::{
        Cond, Handler, HandlerAction, Predicate, ReasonKind, ReasonSet,
    };
    let mut d = base(false);
    if let StepBody::Acquire(a) = &mut d.steps[0].body {
        a.when = rustev_contract::acquisition_step::Guard::Predicate(Predicate::All(vec![]));
    }
    let c = run(&d).unwrap();
    golden("acquisition-guarded.plan.json", &c.canonical());
    let without_skipped: Vec<_> = ReasonKind::ALL
        .into_iter()
        .filter(|r| *r != ReasonKind::Skipped)
        .collect();
    d.policy.on_unresolved = vec![Handler {
        target: "step:lookup".into(),
        reasons: ReasonSet::Only(without_skipped),
        action: HandlerAction::Escalate {
            reason: "review".into(),
        },
    }];
    let mut checked = d.policy.rules[0].clone();
    checked.id = "checked".into();
    d.policy.rules.insert(0, checked);
    d.policy.rules[0].when = Cond::Nonempty("step:lookup/items".into());
    assert_eq!(run(&d).unwrap_err().category, Category::UnhandledUnresolved);
    d.policy.on_unresolved[0].reasons = ReasonSet::Any;
    assert!(run(&d).is_ok());
    d.policy.on_unresolved[0].action = HandlerAction::Propagate;
    let r = run(&d).unwrap_err();
    assert_eq!(r.category, Category::UnhandledUnresolved);
    assert_eq!(r.detail, "skipped_not_propagatable");
    d.policy.on_unresolved[0].action = HandlerAction::Escalate {
        reason: "review".into(),
    };
    d.policy.on_unresolved[0].reasons = ReasonSet::Only(
        ReasonKind::ALL
            .into_iter()
            .filter(|r| *r != ReasonKind::Skipped)
            .collect(),
    );
    for probe in [
        Predicate::Present("step:lookup".into()),
        Predicate::IsConstructed {
            reference: "step:lookup".into(),
        },
    ] {
        d.policy.rules[0].when = Cond::Exact(probe);
        assert!(run(&d).is_ok());
    }
}

#[test]
fn guarded_acquisition_skipped_handlers_allow_only_as_unmet_and_escalate() {
    use rustev_contract::actionability::Address;
    use rustev_contract::definition::{
        Cond, Handler, HandlerAction, Predicate, ReasonKind, ReasonSet,
    };
    let mut d = base(false);
    if let StepBody::Acquire(a) = &mut d.steps[0].body {
        a.when = rustev_contract::acquisition_step::Guard::Predicate(Predicate::All(vec![]));
    }
    d.inputs = serde_json::from_value(json!([{
        "name":"query", "type":{"text":{"max_bytes":64}},
        "provenance":["user-supplied"], "freshness":"not_required"
    }]))
    .unwrap();
    d.actionability = Some(
        serde_json::from_value(json!({
            "caller_controlled":["user-supplied"], "remediations":[],
            "questions":[], "constraint_groups":[], "checks":[]
        }))
        .unwrap(),
    );
    let mut checked = d.policy.rules[0].clone();
    checked.id = "checked".into();
    checked.when = Cond::Nonempty("step:lookup/items".into());
    d.policy.rules.insert(0, checked);
    for reasons in [ReasonSet::Any, ReasonSet::Only(ReasonKind::ALL.to_vec())] {
        d.policy.on_unresolved = vec![Handler {
            target: "step:lookup".into(),
            reasons,
            action: HandlerAction::AsUnmet,
        }];
        for action in [
            HandlerAction::Propagate,
            HandlerAction::PropagateWith {
                remediation: None,
                address: Address::Computed,
            },
            HandlerAction::NeedsInput {
                fields: vec!["query".into()],
                questions: vec![],
            },
        ] {
            d.policy.on_unresolved[0].action = action;
            let r = run(&d).unwrap_err();
            assert_eq!(r.category, Category::UnhandledUnresolved);
            assert_eq!(r.category as u8, 8);
            assert_eq!(r.subject, "step:lookup");
            assert_eq!(r.detail, "skipped_not_propagatable");
        }
        for action in [
            HandlerAction::AsUnmet,
            HandlerAction::Escalate {
                reason: "review".into(),
            },
        ] {
            d.policy.on_unresolved[0].action = action;
            run(&d).expect("as_unmet and escalate handle skipped acquisitions");
        }
    }
}

#[test]
fn exact_requests_types_cycles_and_source_mappings() {
    let mut d = base(false);
    d.inputs = vec![
        serde_json::from_value(json!({"name":"q","type":{"text":{"max_bytes":64}},
        "provenance":["model-derived"],"freshness":"not_required"}))
        .unwrap(),
    ];
    if let StepBody::Acquire(a) = &mut d.steps[0].body {
        a.request.insert(
            "q".into(),
            rustev_contract::definition::Expr::Ref("input:q".into()),
        );
    }
    let r = run(&d).unwrap_err();
    assert_eq!(r.category, Category::InvalidAcquisition);
    assert!(r.detail.contains("exact_request"));
    d.inputs[0].provenance = vec![rustev_contract::definition::ProvenanceClass::SystemOfRecord];
    assert!(run(&d).is_ok());
    for (value, cat) in [
        (json!({"lit":{"integer":1}}), Category::KindMismatch),
        (json!({"ref":"step:lookup/items"}), Category::Cycle),
    ] {
        assert_eq!(
            run(&mutate(false, "/steps/0/body/acquire/request/q", value))
                .unwrap_err()
                .category,
            cat
        );
    }
    let d = mutate(
        true,
        "/steps/0/body/widen/item/fields/0/type",
        json!("decimal"),
    );
    assert_eq!(run(&d).unwrap_err().category, Category::SourceMapping);
    let d = mutate(false, "/steps/0/body/acquire/operation", json!("missing"));
    assert_eq!(run(&d).unwrap_err().category, Category::SourceBinding);
}

#[test]
fn all_typed_local_bounds_are_checked_as_invalid_acquisition() {
    use rustev_contract::acquisition_step::AcquireStop;
    use rustev_contract::definition::{Expr, Literal};
    use rustev_contract::execution::FailureClass;
    for case in 0..10 {
        let mut d = base(false);
        let StepBody::Acquire(a) = &mut d.steps[0].body else {
            unreachable!()
        };
        match case {
            0 => a.limits.max_pages = 65,
            1 => a.limits.max_items = 0,
            2 => a.limits.max_items = 100001,
            3 => a.limits.max_attempts = 9,
            4 => a.limits.retry_on.clear(),
            5 => a.limits.retry_on.push(FailureClass::Permanent),
            6 => a.limits.as_of_skew_ms = 60001,
            7 => a.limits.deadline_share = rustev_contract::Decimal::ZERO,
            8 => a.limits.deadline_share = rustev_contract::Decimal::from_i64(2),
            _ => {
                a.stop = AcquireStop::ItemsAtLeast {
                    count: Expr::Lit(Literal::Integer(0)),
                }
            }
        }
        assert_eq!(
            run(&d).unwrap_err().category,
            Category::InvalidAcquisition,
            "case {case}"
        );
    }
    for case in 0..8 {
        let mut d = base(true);
        let StepBody::Widen(w) = &mut d.steps[0].body else {
            unreachable!()
        };
        match case {
            0 => w.sources.clear(),
            1 => w.tiers.clear(),
            2 => w.max_tiers = 9,
            3 => w.tiers[0].source_index = 2,
            4 => w.groups.clear(),
            5 => {
                w.groups.insert("duplicate".into(), vec!["q".into()]);
            }
            6 => w.protected.push("unknown".into()),
            _ => w.item.fields.clear(),
        }
        assert_eq!(
            run(&d).unwrap_err().category,
            Category::InvalidAcquisition,
            "case {case}"
        );
    }
}

#[test]
fn plan_identity_covers_guard_request_tiers_groups_mapping_and_limits() {
    let original = run(&base(true)).unwrap().id;
    for (path, value) in [
        ("/steps/0/body/widen/tiers/0/when", json!({"all":[]})),
        (
            "/steps/0/body/widen/base_request/q",
            json!({"lit":{"text":"coffee"}}),
        ),
        ("/steps/0/body/widen/tiers/0/name", json!("narrow")),
        ("/steps/0/body/widen/tiers/1/source_index", json!(0)),
        ("/steps/0/body/widen/protected", json!([])),
        ("/steps/0/body/widen/sources/1/map/id", json!("unused")),
        ("/steps/0/body/widen/limits/deadline_share", json!("0.5")),
        ("/steps/0/body/widen/limits/max_pages", json!(1)),
        (
            "/steps/0/body/widen/stop",
            json!({"unique_at_least":{"lit":{"integer":1}}}),
        ),
    ] {
        assert_ne!(
            run(&mutate(true, path, value)).unwrap().id,
            original,
            "{path}"
        );
    }
}
