//! Mechanics-only backend swap for spec 008. The lodging package's authored
//! SYNTHETIC cases run through the rules backend and through Jev using static
//! SYNTHETIC recorded responses served on loopback. No live endpoint or
//! provider is used, and no quality or readiness claim follows.

mod common;
#[path = "../../../packages/rustev-pkg-lodging/tests/common/run.rs"]
mod run;

use std::sync::Arc;

use common::*;
use rustev_backend_rules::RulesBackend;
use rustev_contract::Identified;
use rustev_contract::execution::CostPolicy;
use rustev_core::seams::DecisionBackend;
use rustev_core::{Compiled, compile, compile_with};
use rustev_jev::binding::JevBinding;
use rustev_pkg_lodging as pkg;
use serde_json::Value as Json;

fn rules_backend() -> RulesBackend {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../backends/rustev-backend-rules/tests/fixtures/lodging.rules.json");
    RulesBackend::from_bytes(&std::fs::read(path).unwrap()).unwrap()
}

fn recorded_reply(fixture: &Json, request: &Json) -> Canned {
    let kind = request["questions"]["q0"]["type"].as_str().unwrap();
    ok_json(&fixture["responses"][kind])
}

fn plans(rules: &RulesBackend, jev: &dyn DecisionBackend) -> (Compiled, Compiled) {
    let definition = pkg::definition(&pkg::Params::reference()).unwrap();
    let rules_plan = compile(&definition, &[rules.descriptor().clone()], &[]).unwrap();
    let estimated = execution(
        CostPolicy::Estimated {
            max_units: 5_000_000_000,
        },
        vec![],
    );
    let jev_plan = compile_with(&definition, &[jev.descriptor().clone()], &[], &estimated).unwrap();
    (rules_plan, jev_plan)
}

#[test]
fn recorded_only_jev_swap_yields_comparable_lodging_reports() {
    let tmp = env!("CARGO_TARGET_TMPDIR");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let rules_dir = run::fresh_dir(tmp, "lodging-swap/rules");
    let jev_dir = run::fresh_dir(tmp, "lodging-swap/jev");
    let (rules_plan, jev_plan, hits, records) = runtime.block_on(async {
        let response = fixture("synthetic-lodging-recorded-answers.json");
        let gateway = gateway(move |request| recorded_reply(&response, request)).await;
        let (jev, sink) = adapter(&gateway.url(), JevBinding::gateway());
        let rules = rules_backend();
        let (rules_plan, jev_plan) = plans(&rules, &jev);
        let rules_out = run::run_cases(Arc::new(rules), &rules_plan, &rules_dir).await;
        let jev_out = run::run_cases(Arc::new(jev), &jev_plan, &jev_dir).await;
        assert_eq!(rules_out.len(), pkg::CASES.len());
        assert_eq!(jev_out.len(), pkg::CASES.len());
        (rules_plan, jev_plan, gateway.hits(), sink.records().len())
    });
    assert!(hits > 0, "the swap must exercise the loopback adapter");
    assert_eq!(records, hits, "an exchange record per request");
    assert_loopback_only();

    assert_eq!(rules_plan.plan.definition, jev_plan.plan.definition);
    assert_eq!(
        rules_plan.plan.definition.policy,
        jev_plan.plan.definition.policy
    );
    assert_ne!(rules_plan.plan.id().unwrap(), jev_plan.plan.id().unwrap());

    for split in ["training", "model_selection", "calibration", "final_test"] {
        let base = format!("lodging-swap/ranking-{split}");
        let rules_out = run::fresh_dir(tmp, &format!("{base}/rules"));
        let jev_out = run::fresh_dir(tmp, &format!("{base}/jev"));
        let (rules_code, rules_report) = run::eval(pkg::RANKING, split, &rules_dir, &rules_out);
        let (jev_code, jev_report) = run::eval(pkg::RANKING, split, &jev_dir, &jev_out);
        assert_eq!(
            (rules_code, jev_code),
            (0, 0),
            "{base}: rules={rules_report} jev={jev_report}"
        );
        for key in ["dataset", "config", "adapter_digest"] {
            assert_eq!(rules_report[key], jev_report[key], "{base}: {key}");
        }
        assert_eq!(run::measured(&rules_report, "coverage"), Some((1, 1.0)));
        assert_eq!(run::measured(&jev_report, "coverage"), Some((1, 1.0)));
        let (_, verdict) = run::gate(&rules_out, &jev_out, "error");
        let status = verdict["status"].as_str().unwrap_or_default();
        assert!(["pass", "fail", "unknown"].contains(&status), "{verdict}");
    }
}
