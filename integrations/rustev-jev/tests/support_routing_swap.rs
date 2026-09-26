//! The support-routing backend swap (spec 007 3.4 as amended by spec 016):
//! the package's SYNTHETIC cases run through `rustev-runtime` twice, once
//! with spec 005's SYNTHETIC rules program and once with this adapter
//! answering from hand-written SYNTHETIC Gateway responses on loopback, and
//! each run is captured and evaluated with the package's queue and priority
//! sets. The test lives here, with the package as a dev-dependency, because
//! no crate may depend on a crate under `integrations/` (spec 001 3.4.4;
//! spec 016 3.3). Mechanics only (R-04): no report here is a quality claim.

mod common;
#[path = "../../../packages/rustev-pkg-support-routing/tests/common/mod.rs"]
mod pkg_common;
#[path = "../../../packages/rustev-pkg-support-routing/tests/common/run.rs"]
mod run;

use std::sync::Arc;

use common::*;
use rustev_backend_rules::RulesBackend;
use rustev_contract::Identified;
use rustev_contract::definition::{
    Cond, Definition, OutcomeDecl, PolicyDecl, UncalibratedThreshold,
};
use rustev_contract::execution::CostPolicy;
use rustev_core::seams::DecisionBackend;
use rustev_core::{Category, Compiled, compile, compile_with};
use rustev_jev::binding::JevBinding;
use rustev_pkg_support_routing as pkg;
use serde_json::{Value as Json, json};

const JEV_TOPIC: &str = "Jev's topic distribution is provider-reported and not calibrated (spec 012 I-2); 0.6 top mass is a declared margin rule, not a probability (R-31, spec 016)";

fn rules_backend() -> RulesBackend {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../backends/rustev-backend-rules/tests/fixtures/support-routing.rules.json");
    RulesBackend::from_bytes(&std::fs::read(p).unwrap()).unwrap()
}

/// The hand-written answer for the case whose ticket appears in `request`,
/// by question type.
fn swap_gateway_reply(fixture: &Json, request: &Json) -> Canned {
    let text = request.to_string();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| text.contains(c["message"].as_str().unwrap()))
        .unwrap_or_else(|| panic!("no SYNTHETIC answer for {text}"));
    let kind = request["questions"]["q0"]["type"].as_str().unwrap();
    ok_json(&case["responses"][kind])
}

/// A policy section with `topic`'s threshold declaration set to `u`.
fn with_topic_declaration(policy: &PolicyDecl, u: &UncalibratedThreshold) -> PolicyDecl {
    let mut p = policy.clone();
    for r in &mut p.rules {
        if let Cond::TopMassBelow {
            step,
            uncalibrated_threshold,
            ..
        } = &mut r.when
            && step == "topic"
        {
            *uncalibrated_threshold = u.clone();
        }
    }
    p
}

/// Every proposal a policy can make: action and parameter names, in order.
fn outputs(policy: &PolicyDecl) -> Vec<Json> {
    policy
        .rules
        .iter()
        .map(|r| match &r.then {
            OutcomeDecl::Propose { action, params } => json!({
                "action": action,
                "params": params.iter().map(|p| json!([p.name, serde_json::to_value(&p.value).unwrap()])).collect::<Vec<_>>(),
            }),
            other => json!(serde_json::to_value(other).unwrap()),
        })
        .collect()
}

struct Plans {
    rules: Compiled,
    rules_cal: rustev_contract::calibration::CalibrationArtifact,
    jev: Compiled,
    jev_def: Definition,
}

fn plans(rules: &RulesBackend, jev: &dyn DecisionBackend) -> Plans {
    let rules_cal = pkg_common::calibration_for(rules.artifact());
    let rules_def = pkg::definition(&pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(
        rules_cal.id().unwrap(),
    )))
    .unwrap();
    let rules_plan = compile(
        &rules_def,
        &[rules.descriptor().clone()],
        std::slice::from_ref(&rules_cal),
    )
    .unwrap();
    let jev_def = pkg::definition(&pkg::Params::with_topic(
        pkg::TopicCalibration::Uncalibrated {
            reason: JEV_TOPIC.into(),
        },
    ))
    .unwrap();
    let estimated = execution(
        CostPolicy::Estimated {
            max_units: 5_000_000_000,
        },
        vec![],
    );
    let jev_plan = compile_with(&jev_def, &[jev.descriptor().clone()], &[], &estimated).unwrap();
    Plans {
        rules: rules_plan,
        rules_cal,
        jev: jev_plan,
        jev_def,
    }
}

#[test]
fn swapping_rules_for_jev_yields_comparable_reports_and_one_authority_path() {
    let tmp = env!("CARGO_TARGET_TMPDIR");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let rules_dir = run::fresh_dir(tmp, "support-routing-swap/rules");
    let jev_dir = run::fresh_dir(tmp, "support-routing-swap/jev");
    let (p, jev_hits, jev_records) = rt.block_on(async {
        let fixture = fixture("synthetic-support-routing-answers.json");
        let gw = gateway(move |req| swap_gateway_reply(&fixture, req)).await;
        let (jev, sink) = adapter(&gw.url(), JevBinding::gateway());
        let rules = rules_backend();
        let p = plans(&rules, &jev);
        let cals = [p.rules_cal.clone()];
        let r = run::run_cases(Arc::new(rules), &p.rules, &cals, &rules_dir).await;
        let j = run::run_cases(Arc::new(jev), &p.jev, &[], &jev_dir).await;
        assert_eq!((r.len(), j.len()), (pkg::CASES.len(), pkg::CASES.len()));
        // sr-f04's hand-written topic mass is 0.5: below the margin rule.
        let f04 = j.iter().find(|c| c.case == "sr-f04").unwrap();
        assert!(f04.outcome.contains("ambiguous-topic"), "{}", f04.outcome);
        (p, gw.hits(), sink.records().len())
    });
    // Three questions per case, one request each (batching off, R-28).
    assert_eq!(jev_hits, 3 * pkg::CASES.len());
    assert_eq!(jev_records, jev_hits, "an exchange record per request");
    assert_loopback_only();

    // 3.4.3 as amended (spec 016 3.1): one authority path.
    let (rp, jp) = (
        &p.rules.plan.definition.policy,
        &p.jev.plan.definition.policy,
    );
    assert_ne!(
        rp, jp,
        "the declaration must differ, or this test proves nothing"
    );
    let declared = UncalibratedThreshold::Declared {
        reason: JEV_TOPIC.into(),
    };
    assert_eq!(
        with_topic_declaration(rp, &declared),
        *jp,
        "the policy sections differ beyond topic's threshold declaration"
    );
    assert_eq!(
        with_topic_declaration(jp, &UncalibratedThreshold::NotDeclared),
        *rp
    );
    assert_eq!(outputs(rp), outputs(jp), "the output declarations differ");

    // 3.4.2: comparable reports under one dataset, split and configuration,
    // and the gate's verdict recorded as mechanics evidence.
    for set in [pkg::QUEUE, pkg::PRIORITY] {
        for split in ["training", "model_selection", "calibration", "final_test"] {
            let base = format!("support-routing-swap/{}-{split}", set.name);
            let rules_out = run::fresh_dir(tmp, &format!("{base}/rules"));
            let jev_out = run::fresh_dir(tmp, &format!("{base}/jev"));
            let (rc, rr) = run::eval(set, split, &rules_dir, &rules_out);
            let (jc, jr) = run::eval(set, split, &jev_dir, &jev_out);
            assert_eq!((rc, jc), (0, 0), "{base}: {rr} {jr}");
            for k in ["dataset", "config", "adapter_digest"] {
                assert_eq!(rr[k], jr[k], "{base}: {k} differs");
            }
            assert_eq!(run::measured(&rr, "coverage"), Some((4, 1.0)), "{base}");
            assert_eq!(run::measured(&jr, "coverage"), Some((4, 1.0)), "{base}");
            let (_, verdict) = run::gate(&rules_out, &jev_out, "error");
            let status = verdict["status"].as_str().unwrap_or_default();
            assert!(
                ["pass", "fail", "unknown"].contains(&status),
                "{base}: {verdict}"
            );
        }
    }
}

#[test]
fn a_jev_topic_threshold_without_a_declaration_is_refused() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let gw = auto_gateway(None).await;
        let (jev, _) = adapter(&gw.url(), JevBinding::gateway());
        let p = plans(&rules_backend(), &jev);
        let mut def = p.jev_def.clone();
        def.policy = with_topic_declaration(&def.policy, &UncalibratedThreshold::NotDeclared);
        let refused = compile(&def, &[jev.descriptor().clone()], &[]).unwrap_err();
        assert_eq!(refused.category, Category::UncalibratedThreshold);
        assert_eq!(gw.hits(), 0);
    });
}
