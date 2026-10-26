//! Synthetic spec 039 compilation fixtures, without model execution.
mod common;
use rustev_contract::{
    definition::*, descriptor::BackendDescriptor, plan::PlanStepDetail, ranking::*,
};
use rustev_core::{Category as C, Compiled, Refusal, compile};
use serde_json::json;

fn fixture() -> (Definition, BackendDescriptor) {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.limits.max_semantic_requests = 64;
    d.inputs = vec![serde_json::from_value(json!({"name":"items","type":{"list":{"item":{"record":{"fields":[{"name":"id","type":{"text":{"max_bytes":8}}}]}},"max_items":5}},"provenance":["system-of-record"],"freshness":"not_required"})).unwrap()];
    d.steps = serde_json::from_value(json!([
        {"id":"pool","body":{"exact":{"op":"filter_with_reasons","version":1,"args":{"list":"input:items","id_field":"id","rules":[]}}}},
        {"id":"ranked","body":{"rank":{"task":"fit","question":"Which fits?","candidates":"step:pool","items":"none","bind":"candidate","project":[],"mode":{"listwise":{"scores":"optional"}},"bias":"none","tie_break":"id_ascending","backend":{"min_determinism":"unspecified","artifact":"any","binding":"auto"},"fallback":"none"}}},
        {"id":"chosen","body":{"select":{"from":"step:ranked","k":3,"backfill":{"from":"step:pool","exclude_selected":true},"on_backend_failure":"backfill_all"}}}
    ])).unwrap();
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let mut b = common::linear_head();
    b.ranking = Some(serde_json::from_value(json!({"modes":["listwise","pointwise","pairwise"],"max_candidates":1024,"listwise_scores":"optional","listwise_partial":"possible"})).unwrap());
    (d, b)
}
fn run(d: &Definition, b: &BackendDescriptor) -> Result<Compiled, Refusal> {
    compile(d, std::slice::from_ref(b), &[])
}
fn rank(d: &mut Definition) -> &mut Rank {
    let StepBody::Rank(r) = &mut d.steps[1].body else {
        panic!()
    };
    r
}
fn select(d: &mut Definition) -> &mut Select {
    let StepBody::Select(s) = &mut d.steps[2].body else {
        panic!()
    };
    s
}
fn pair() -> RankMode {
    RankMode::Pairwise {
        schedule: PairwiseSchedule::RoundRobin,
        max_comparisons: 64,
    }
}
fn refusal(d: &Definition, b: &BackendDescriptor, category: C) -> Refusal {
    let e = run(d, b).unwrap_err();
    assert_eq!(e.category, category, "{e}");
    e
}

#[test]
fn modes_pin_plan_bytes_identity_and_reload() {
    for (name, mode, bias, count) in [
        (
            "listwise",
            RankMode::Listwise {
                scores: RankScores::Optional,
            },
            Bias::None,
            1,
        ),
        ("pointwise", RankMode::Pointwise, Bias::None, 5),
        (
            "pairwise",
            pair(),
            Bias::PermutationAverage {
                orders: 2,
                seed: "seed".into(),
            },
            20,
        ),
        (
            "merge",
            RankMode::Pairwise {
                schedule: PairwiseSchedule::MergeSort,
                max_comparisons: 64,
            },
            Bias::Shuffle {
                seed: "seed".into(),
            },
            9,
        ),
    ] {
        let (mut d, b) = fixture();
        rank(&mut d).mode = mode;
        rank(&mut d).bias = bias;
        let c = run(&d, &b).unwrap();
        let PlanStepDetail::Rank(r) = &c.plan.steps[1].detail else {
            panic!()
        };
        assert_eq!(r.pool_bound, 5);
        assert_eq!(r.max_requests, count);
        assert_eq!(c.plan.schema, "rustev.plan/3");
        assert_eq!(
            c.plan.steps[1].derivation,
            rustev_contract::judgment::Derivation::ModelDerived
        );
        assert_eq!(
            c.plan.steps[2].derivation,
            rustev_contract::judgment::Derivation::MixedDerived
        );
        common::golden(&format!("ranking.{name}.plan.json"), &c.canonical());
        common::golden(&format!("ranking.{name}.plan-id"), c.id.as_str().as_bytes());
        assert_eq!(
            Compiled::load(&c.plan, std::slice::from_ref(&b), &[])
                .unwrap()
                .id,
            c.id
        );
        assert!(matches!(
            Compiled::load_checked(&c.plan, &[], &[]),
            Err(rustev_core::LoadError::MissingDescriptor { .. })
        ));
    }
}

#[test]
fn structural_refusals_are_phase_b_before_unknown_operators() {
    for case in 0..10 {
        let (mut d, b) = fixture();
        match case {
            0 => d.schema = "rustev.definition/1".into(),
            1 => {
                rank(&mut d).mode = RankMode::Pointwise;
                rank(&mut d).bias = Bias::Shuffle {
                    seed: "seed".into(),
                };
            }
            2 => {
                rank(&mut d).bias = Bias::PermutationAverage {
                    orders: 9,
                    seed: "seed".into(),
                }
            }
            3 => {
                rank(&mut d).mode = pair();
                rank(&mut d).bias = Bias::PermutationAverage {
                    orders: 3,
                    seed: "seed".into(),
                };
            }
            4 => rank(&mut d).bias = Bias::Shuffle { seed: "!".into() },
            5 => select(&mut d).k = 0,
            6 => {
                rank(&mut d).mode = RankMode::Pairwise {
                    schedule: PairwiseSchedule::RoundRobin,
                    max_comparisons: 0,
                }
            }
            7 => {
                select(&mut d).backfill = Backfill::From {
                    from: "step:pool".into(),
                    exclude_selected: false,
                }
            }
            8 => select(&mut d).backfill = Backfill::None,
            _ => rank(&mut d).candidates = "input:items".into(),
        }
        if let StepBody::Exact(e) = &mut d.steps[0].body {
            e.op = "unknown".into();
        }
        refusal(&d, &b, C::InvalidDefinition);
    }
}

#[test]
fn type_and_exact_source_refusals_precede_phase_f() {
    for case in 0..8 {
        let (mut d, mut b) = fixture();
        b.ranking = None;
        let mut last = d.policy.rules[0].clone();
        last.id = "last".into();
        d.policy.rules.push(last);
        let category = match case {
            0 => {
                select(&mut d).from = "step:pool".into();
                C::KindMismatch
            }
            1 => {
                select(&mut d).backfill = Backfill::From {
                    from: "step:ranked".into(),
                    exclude_selected: true,
                };
                C::KindMismatch
            }
            2 => {
                d.inputs[0].provenance = vec![ProvenanceClass::ModelDerived];
                C::ExactOrderSource
            }
            3 => {
                select(&mut d).backfill = Backfill::None;
                select(&mut d).on_backend_failure = BackendFailure::Propagate;
                d.inputs[0].provenance = vec![ProvenanceClass::ModelDerived];
                rank(&mut d).tie_break = TieBreak::OrderOf("step:pool".into());
                C::ExactOrderSource
            }
            4 => {
                d.policy.rules[0].when = Cond::Empty("step:ranked".into());
                C::KindMismatch
            }
            5 => {
                d.policy.rules[0].when = Cond::TopLabel {
                    step: "chosen".into(),
                    label: "x".into(),
                };
                C::KindMismatch
            }
            6 => {
                rank(&mut d).project = vec!["step:ranked".into()];
                C::Cycle
            }
            _ => {
                let mut other = d.steps[1].clone();
                other.id = "other".into();
                d.steps.push(other);
                rank(&mut d).candidates = "step:other".into();
                C::KindMismatch
            }
        };
        refusal(&d, &b, category);
    }
}

#[test]
fn capability_shortfalls_are_phase_f_before_budget() {
    use rustev_core::compile::ShortfallReason as S;
    for case in 0..3 {
        let (mut d, mut b) = fixture();
        d.limits.max_semantic_requests = 0;
        match case {
            0 => b.ranking = None,
            1 => b.ranking.as_mut().unwrap().max_candidates = 1,
            _ => {
                rank(&mut d).mode = RankMode::Listwise {
                    scores: RankScores::Required,
                };
                b.ranking.as_mut().unwrap().listwise_scores =
                    rustev_contract::descriptor::ListwiseScores::Never;
            }
        }
        let e = refusal(&d, &b, C::NoCapableBackend);
        assert!(matches!(
            (&e.shortfalls[0].reasons[0], case),
            (S::RankMode, 0) | (S::MaxCandidates { .. }, 1) | (S::ListwiseScores, 2)
        ));
    }
}

#[test]
fn phase_g_bounds_and_visible_truncation() {
    let (mut d, b) = fixture();
    rank(&mut d).mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::RoundRobin,
        max_comparisons: 9,
    };
    refusal(&d, &b, C::Budget);
    rank(&mut d).mode = RankMode::Pointwise;
    if let TypeDecl::List { max_items, .. } = &mut d.inputs[0].ty {
        *max_items = 1025;
    }
    refusal(&d, &b, C::Budget);
    if let TypeDecl::List { max_items, .. } = &mut d.inputs[0].ty {
        *max_items = 5;
    }
    d.limits.max_semantic_requests = 4;
    refusal(&d, &b, C::Budget);
    d.limits.on_excess = ExcessPolicy::TruncateVisible;
    run(&d, &b).unwrap();
}

#[test]
fn identity_members_each_change_plan_id() {
    let (d, b) = fixture();
    let original = run(&d, &b).unwrap().id;
    for case in 0..15 {
        let (mut d, mut b) = (d.clone(), b.clone());
        match case {
            0 => b.backend_id.push('x'),
            1 => b.artifact = common::artifact('b'),
            2 => {
                b.ranking.as_mut().unwrap().listwise_partial =
                    rustev_contract::descriptor::ListwisePartial::Never
            }
            3 => rank(&mut d).mode = RankMode::Pointwise,
            4 => {
                rank(&mut d).mode = RankMode::Listwise {
                    scores: RankScores::None,
                }
            }
            5 => rank(&mut d).bias = Bias::Shuffle { seed: "s".into() },
            6 => {
                rank(&mut d).bias = Bias::PermutationAverage {
                    orders: 2,
                    seed: "s".into(),
                }
            }
            7 => rank(&mut d).tie_break = TieBreak::OrderOf("step:pool".into()),
            8 => {
                if let TypeDecl::List { max_items, .. } = &mut d.inputs[0].ty {
                    *max_items = 6;
                }
            }
            9 => select(&mut d).k = 4,
            10 => select(&mut d).on_backend_failure = BackendFailure::Propagate,
            11 => {
                select(&mut d).backfill = Backfill::None;
                select(&mut d).on_backend_failure = BackendFailure::Propagate;
            }
            12 => rank(&mut d).question.push('?'),
            13 => rank(&mut d).task.push('x'),
            _ => {
                rank(&mut d).items = ItemSource::List {
                    list: "input:items".into(),
                    id_field: "id".into(),
                }
            }
        }
        assert_ne!(run(&d, &b).unwrap().id, original, "case {case}");
    }
}

#[test]
fn selection_accepts_ordered_id_consumers_and_unsupported_fallback() {
    let (mut d, mut b) = fixture();
    d.policy.rules.insert(0, serde_json::from_value(json!({"id":"nonempty","when":{"nonempty":"step:chosen"},"then":{"propose":{"action":"recommend","params":[{"name":"items","value":{"ref":"step:chosen"}}]}}})).unwrap());
    d.policy.on_unresolved.push(serde_json::from_value(json!({"target":"step:chosen","reasons":"any","action":{"escalate":{"reason":"missing"}}})).unwrap());
    run(&d, &b).unwrap();
    b.ranking = None;
    rank(&mut d).fallback = RankFallback::Unsupported;
    assert!(matches!(
        run(&d, &b).unwrap().plan.steps[1].detail,
        PlanStepDetail::Unsupported { .. }
    ));
}

#[test]
fn wire_controls_keep_category_11_and_kind_fallback_is_refused() {
    use rustev_contract::Document;
    let (mut d, b) = fixture();
    select(&mut d).k = 0;
    let e = rustev_core::compile_bytes(&d.canonical().unwrap(), std::slice::from_ref(&b), &[])
        .unwrap_err();
    assert_eq!(e.category, C::InvalidDefinition);
    let mut raw = serde_json::to_value(fixture().0).unwrap();
    raw["steps"][1]["body"]["rank"]["fallback"] =
        json!({"kind":{"requires":"scores","reason":"fallback"}});
    let e = rustev_core::compile_bytes(&serde_json::to_vec(&raw).unwrap(), &[b], &[]).unwrap_err();
    assert_eq!(e.category, C::InvalidDefinition);
    assert_eq!(e.subject, "step:ranked");
}

#[test]
fn bias_parameters_and_pairwise_parameters_each_enter_identity() {
    let (mut d, b) = fixture();
    rank(&mut d).bias = Bias::PermutationAverage {
        orders: 2,
        seed: "a".into(),
    };
    let baseline = run(&d, &b).unwrap().id;
    rank(&mut d).bias = Bias::PermutationAverage {
        orders: 3,
        seed: "a".into(),
    };
    assert_ne!(run(&d, &b).unwrap().id, baseline);
    rank(&mut d).bias = Bias::PermutationAverage {
        orders: 2,
        seed: "b".into(),
    };
    assert_ne!(run(&d, &b).unwrap().id, baseline);
    rank(&mut d).bias = Bias::None;
    rank(&mut d).mode = pair();
    let baseline = run(&d, &b).unwrap().id;
    rank(&mut d).mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::RoundRobin,
        max_comparisons: 65,
    };
    assert_ne!(run(&d, &b).unwrap().id, baseline);
    rank(&mut d).mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::MergeSort,
        max_comparisons: 64,
    };
    assert_ne!(run(&d, &b).unwrap().id, baseline);
}
