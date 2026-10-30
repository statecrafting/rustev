//! SYNTHETIC fixtures shared by crash and spend acceptance tests.
#![allow(dead_code)]
#[path = "../../../rustev-runtime/tests/common/mod.rs"]
pub mod common;

use common::*;
use rustev_contract::ids::DatasetId;
use rustev_contract::live::{AuthorizationReference, AuthorizationRequest};
use rustev_contract::scope::{Handle as Subject, Scope};
use rustev_contract::{Document, Identified};
use rustev_live::documents::{LiveCheckpoint, LiveConfig, Mode};
use rustev_live::journal::{BudgetKind, SetClock, SpendJournal};
use rustev_live::probe::ProbeSnapshot;
use rustev_live::*;
use rustev_runtime::{DispatchAnswer, LiveAuthorizer};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Dir(pub PathBuf);
impl Dir {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rustev-acceptance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    pub fn journal(&self) -> SpendJournal {
        SpendJournal::open(
            self.0.join("journal"),
            BudgetKind::Testing,
            Arc::new(SetClock::new(0)),
        )
        .unwrap()
    }
    pub fn options(&self) -> RunOptions {
        RunOptions {
            out: self.0.join("out"),
            resume: false,
            rerun_interrupted: false,
            probe_only: false,
            run_deadline_ms: None,
        }
    }
    pub fn checkpoint(&self) -> LiveCheckpoint {
        LiveCheckpoint::parse(&std::fs::read(self.0.join("out/checkpoint.json")).unwrap()).unwrap()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Host(pub Option<&'static str>);
impl LiveAuthorizer for Host {
    fn authorize(&self, r: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
        let reference = AuthorizationReference::new(format!("SYNTHETIC:{}", r.attempt_id)).unwrap();
        Ok(Some(
            if self.0.is_some_and(|suffix| r.decision_id.ends_with(suffix)) {
                DispatchAnswer::Refuse {
                    reference: Some(reference),
                    detail: "SYNTHETIC".into(),
                }
            } else {
                DispatchAnswer::Allow {
                    answers: r.digest().unwrap(),
                    reference,
                    valid_for_ms: 2_000,
                }
            },
        ))
    }
}
pub fn spec() -> RunSpec {
    let probe = support_snapshot("pro", &[2, 5], 0);
    let case = support_snapshot("free", &[1], 0);
    let d = linear_head();
    let config = LiveConfig::parse(&serde_json::to_vec(&json!({
        "schema":"rustev.live-config/1", "budget":"testing", "run_cap_units":1000,
        "repeats":1, "evaluation_time":"source", "bindings":[{
            "identity":{"backend_id":d.backend_id,"artifact":d.artifact}, "cost_unit":"nano_usd",
            "max_in_flight":1,"min_interval_ms":0,"case_deadline_ms":2000,"rate_limit_pause_max_ms":50
        }], "acquisitions":{}, "probe":{"calls":1,"interval_ms":0,"snapshot":"SYNTHETIC",
            "digest":probe.id().unwrap(),"provenance":"synthetic"}
    })).unwrap()).unwrap();
    let snapshot =
        ProbeSnapshot::from_bytes(&config.probe, &probe.canonical().unwrap(), &[]).unwrap();
    RunSpec {
        reports: None,
        config,
        mode: Mode::Library,
        run_id: "run".into(),
        dataset_id: DatasetId::parse(&format!("sha256:{}", "a".repeat(64))).unwrap(),
        split: "test".into(),
        candidates: BTreeMap::from([("a".into(), support_compiled(None))]),
        cases: ["x", "y"]
            .into_iter()
            .map(|id| LiveCase {
                case_id: id.into(),
                input: CaseInput::Available {
                    snapshot_bytes: case.canonical().unwrap(),
                    snapshot_id: case.id().unwrap(),
                    source_time: Some(ts(NOW)),
                    scope: None,
                },
            })
            .collect(),
        probe_snapshot: snapshot,
        probe_time: ts(NOW),
        principal_handle: b"tenant-a".to_vec(),
        subject: Subject::new("subject-1").unwrap(),
        scope: Scope::principal(
            Subject::new("tenant-a").unwrap(),
            Subject::new("rev-2").unwrap(),
            Subject::new("scope-p").unwrap(),
        ),
        input_digests: BTreeMap::new(),
    }
}

struct ReportAdapter;
impl rustev_eval::metrics::TaskAdapter for ReportAdapter {
    fn adapter(&self) -> rustev_eval::dataset::AdapterRef {
        rustev_eval::dataset::AdapterRef {
            name: "SYNTHETIC".into(),
            version: "1".into(),
        }
    }
    fn rules(&self) -> rustev_eval::config::AdapterRules {
        rustev_eval::config::AdapterRules::Bound(
            rustev_contract::ids::ContentDigest::parse(&format!("sha256:{}", "a".repeat(64)))
                .unwrap(),
        )
    }
    fn check_label(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn correct(
        &self,
        _: &str,
        _: &BTreeMap<String, rustev_contract::judgment::OutValue>,
        _: &str,
    ) -> bool {
        true
    }
}

pub fn reporting_spec() -> RunSpec {
    use rustev_contract::eval_report::{DatasetProvenance, Split};
    use rustev_eval::dataset::{DatasetCase, DatasetManifest, Label};
    use rustev_eval::metrics::TaskAdapter;
    let mut s = spec();

    s.split = "model_selection".into();

    let CaseInput::Available { snapshot_id, .. } = &s.cases[0].input else {
        unreachable!()
    };
    let snapshot = snapshot_id.clone();
    s.cases.truncate(1);
    let manifest = DatasetManifest {
        schema: rustev_eval::dataset::DATASET.into(),
        name: "SYNTHETIC live repeats".into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "live tests".into(),
        },
        adapter: ReportAdapter.adapter(),
        cases: s
            .cases
            .iter()
            .map(|c| DatasetCase {
                id: c.case_id.clone(),
                source: format!("synthetic:{}", c.case_id),
                snapshot: snapshot.clone(),
                split: Split::ModelSelection,
                label: Label::Missing,
                subgroups: vec![],
                gold: None,
            })
            .collect(),
    };
    s.dataset_id = manifest.id().unwrap();
    let config = rustev_eval::config::EvaluatorConfig::parse(&serde_json::to_vec(&json!({
        "schema":"rustev.evaluator-config/2",
        "adapter":rustev_eval::config::ConfigAdapter::v2(ReportAdapter.adapter(), ReportAdapter.rules()),
        "label_interpretation":"SYNTHETIC", "agreement":{"params":[]},
        "probability_step":null,"reliability_bins":["0","1"],"subgroups":[],
        "latency_quantiles":["0.5","0.95"],"cost_units_comparable":true,
        "metrics": report_formulas(),
        "gates":[],"temperature_grid":[]
    })).unwrap()).unwrap();
    let repeat_config = rustev_eval::repeat_config::RepeatConfig::parse(br#"{
        "schema":"rustev.repeat-config/1",
        "bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.9","resamples":1000,"seed":"29"},
        "noise":{"rule":{"k_sd":{"k":"2"}},"floor":"0","overrides":{}},
        "slices":[],"stability_metrics":["coverage"],"top_k":null
    }"#).unwrap();
    s.reports = Some(ReportInputs {
        manifest,
        split: Split::ModelSelection,
        config,
        adapter: Box::new(ReportAdapter),
        repeat_config,
        top_k: None,
    });
    s
}

fn report_formulas() -> Vec<rustev_eval::config::MetricFormula> {
    [
        ("coverage", "comparable cases / all cases"),
        ("acceptance_coverage", "proposals / comparable cases"),
        ("labeled_proposal_coverage", "labeled proposals / proposals"),
        (
            "error_among_accepted",
            "wrong labeled proposals / labeled proposals",
        ),
        (
            "candidate_outcome_change",
            "cases whose outcome key changed / comparable cases",
        ),
        (
            "mean_log_loss",
            "-sum(ln(p[label])) / n over labeled scored cases",
        ),
        ("brier", "sum_case sum_class (p - y)^2 / n"),
        (
            "reliability",
            "per bin: mean top-label confidence and accuracy",
        ),
        (
            "latency_ms",
            "nearest-rank quantile of elapsed minus queued",
        ),
        ("queue_ms", "nearest-rank quantile of queued"),
        ("cost_observed_units", "sum of observed units"),
        ("cost_estimated_units", "sum of estimated units"),
        ("cost_unknown_attempts", "attempts with an unknown charge"),
        ("cost_liability_units", "units kept as liability"),
    ]
    .iter()
    .map(|(n, f)| rustev_eval::config::MetricFormula {
        name: n.to_string(),
        formula: f.to_string(),
    })
    .collect()
}
