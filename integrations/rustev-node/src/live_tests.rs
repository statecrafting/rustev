//! I-3: compare the binding orchestration with direct prepare and execute.
use super::*;
use rustev_contract::Document;
#[path = "../../../crates/rustev-runtime/tests/common/mod.rs"]
mod common;
use common::*;
use rustev_core::seams::CancelSignal;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct Fixture(PathBuf, Value);
impl Fixture {
    fn new(status: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rustev-node-parity-{}-{status}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = |p: &str| dir.join(p).to_string_lossy().into_owned();
        let write = |p: &str, bytes: Vec<u8>| {
            std::fs::create_dir_all(dir.join(p).parent().unwrap()).unwrap();
            std::fs::write(dir.join(p), bytes).unwrap();
        };
        write("candidates/a/plan.json", support_compiled(None).canonical());
        for d in descriptors_with_fallbacks() {
            write(
                &format!("candidates/a/descriptors/{}.json", d.backend_id),
                d.canonical().unwrap(),
            );
        }
        write(
            "candidates/a/calibrations/topic.json",
            cal()[0].canonical().unwrap(),
        );
        let snapshot = support_snapshot("free", &[1], 0);
        let probe = support_snapshot("pro", &[2, 5], 0);
        write("snapshots/one.json", snapshot.canonical().unwrap());
        write("probe.json", probe.canonical().unwrap());
        let adapter = TaskAdapterDoc::parse(br#"{"schema":"rustev.task-adapter/1","name":"test","version":"1","labels":{"values":["yes"],"prefixes":[]},"rules":[{"action":"route_ticket","label":"yes"}]}"#).unwrap();
        write("adapter.json", adapter.canonical().unwrap());
        write("config.json", serde_json::to_vec(&json!({"schema":"rustev.evaluator-config/2",
            "adapter": rustev_eval::config::ConfigAdapter::v2(adapter.adapter(), adapter.rules()),
            "label_interpretation":"SYNTHETIC", "agreement":{"params":[]}, "probability_step":null,
            "reliability_bins":["0","1"],"subgroups":[],"latency_quantiles":[],"cost_units_comparable":false,
            "metrics":[
                {"name":"coverage","formula":"comparable cases / all cases"},
                {"name":"acceptance_coverage","formula":"proposals / comparable cases"},
                {"name":"labeled_proposal_coverage","formula":"labeled proposals / proposals"},
                {"name":"error_among_accepted","formula":"wrong labeled proposals / labeled proposals"},
                {"name":"candidate_outcome_change","formula":"changed outcomes / comparable cases"},
                {"name":"mean_log_loss","formula":"mean negative log probability of label"},
                {"name":"brier","formula":"mean sum of squared probability errors"},
                {"name":"reliability","formula":"per bin mean confidence and accuracy"},
                {"name":"latency_ms","formula":"quantile of elapsed minus queued"},
                {"name":"queue_ms","formula":"quantile of queued"},
                {"name":"cost_observed_units","formula":"sum of observed units"},
                {"name":"cost_estimated_units","formula":"sum of estimated units"},
                {"name":"cost_unknown_attempts","formula":"attempts with unknown charge"},
                {"name":"cost_liability_units","formula":"units kept as liability"}
            ],"gates":[],"temperature_grid":[]
        })).unwrap());
        write("repeat.json", br#"{"schema":"rustev.repeat-config/1","bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.95","resamples":1000,"seed":"1"},"noise":{"rule":"interval_width","floor":"0","overrides":{}},"slices":[],"stability_metrics":["coverage"],"top_k":null}"#.to_vec());
        write("dataset.json", serde_json::to_vec(&json!({"schema":"rustev.dataset/1","name":"SYNTHETIC",
            "provenance":{"synthetic":{"generator":"SYNTHETIC tests"}},"adapter":{"name":"test","version":"1"},
            "cases":[{"id":"one","source":"one","snapshot":snapshot.id().unwrap(),"split":"training","label":"missing","subgroups":[]}]
        })).unwrap());
        let d = linear_head();
        write("live.json", serde_json::to_vec(&json!({"schema":"rustev.live-config/1","budget":"testing",
            "run_cap_units":if status == "spend_capped" {1} else {1000},"repeats":1,"evaluation_time":{"fixed":{"ms":NOW}},
            "bindings":[{"identity":{"backend_id":d.backend_id,"artifact":d.artifact},"cost_unit":"nano_usd",
                "max_in_flight":1,"min_interval_ms":0,"case_deadline_ms":2000,"rate_limit_pause_max_ms":0}],
            "acquisitions":{},"probe":{"calls":1,"interval_ms":0,"snapshot":path("probe.json"),"digest":probe.id().unwrap(),"provenance":"synthetic"},
            "authorize":[{"backend_id":d.backend_id,"artifact":d.artifact,"operations":["classify","proposition","rubric"],"max_exposure_units":100}]
        })).unwrap());
        let args = json!({"live":true,"liveConfig":path("live.json"),"journal":path("journal"),"runId":"synthetic",
            "dataset":path("dataset.json"),"split":"training","config":path("config.json"),"adapter":path("adapter.json"),
            "scope":{"mode":"tenant_only","tenant":"synthetic","context_revision":"1"},"nowMs":NOW,
            "repeatConfig":path("repeat.json"),"candidates":path("candidates"),"snapshots":path("snapshots"),"out":path("out"),
            "probeOnly":status == "probed"});
        Self(dir, args)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                visit(root, &p, result);
            } else {
                result.insert(
                    p.strip_prefix(root).unwrap().into(),
                    std::fs::read(p).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
#[test]
fn all_statuses_match_direct_rust_execution_byte_for_byte() {
    for (status, code) in [
        ("evaluated_live", 0),
        ("probed", 0),
        ("probe_failed", 15),
        ("spend_capped", 14),
        ("interrupted", 16),
    ] {
        let f = Fixture::new(status);
        validate(&f.1).unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let setup = || {
            let head = Scripted::new(linear_head(), move |c| {
                if status == "probe_failed" {
                    Answer::Fail(
                        rustev_core::seams::AdapterFailure::Permanent,
                        "SYNTHETIC".into(),
                        rustev_contract::run::Charge::Unknown,
                    )
                } else {
                    ok(c.task(), 1)
                }
            });
            let rig = rig(config(2, 0, 3), &[(head, 8)]);
            let cancel = CancelSignal::new();
            if status == "interrupted" {
                cancel.raise();
            }
            (rig, cancel)
        };
        let (rig, cancel) = setup();
        let result = run(&f.1, &rig.rt, &rig.clock, &cancel, &rt)
            .unwrap_or_else(|e| panic!("{status}: {e:?}"));
        assert_eq!(result.status, status);
        assert_eq!(result.code, code);
        let expected = files(&f.0.join("out"));
        std::fs::remove_dir_all(f.0.join("out")).unwrap();
        std::fs::remove_file(f.0.join("journal")).unwrap();
        let (rig, cancel) = setup();
        let (spec, options, adapter) = load(&f.1, &rig.rt).unwrap();
        let authorizer = RuleAuthorizer::new(spec.config.clone()).unwrap();
        let prepared = prepare(spec, options).unwrap();
        let journal = prepared.open_journal().unwrap();
        let direct = rt
            .block_on(prepared.execute(&rig.rt, journal.clone(), &authorizer, &rig.clock, &cancel))
            .unwrap();
        if direct.status == RunStatus::EvaluatedLive {
            publish_live_outputs(
                COMMAND,
                f.1["out"].as_str().unwrap(),
                &adapter,
                &direct.repeat_reports,
                direct.comparison.as_ref().unwrap(),
            )
            .unwrap();
        }
        assert_eq!(u32::from(exit_code(direct.status)), result.code);
        let actual = files(&f.0.join("out"));
        assert_eq!(
            expected.keys().collect::<Vec<_>>(),
            actual.keys().collect::<Vec<_>>()
        );
        for (path, bytes) in expected {
            assert_eq!(bytes, actual[&path], "{status}: {}", path.display());
        }
        let counts = &direct.live_run.counts;
        let stdout = format!(
            "{status} synthetic run_spend={} headroom={} completed={} unavailable={} refused={} interrupted={} pending={} new_journal\n",
            journal.run_spend("synthetic"),
            direct.journal_after.headroom,
            counts["completed"],
            counts["unavailable"],
            counts["refused"],
            counts["interrupted"],
            counts["pending"]
        );
        assert_eq!(result.out.as_ref(), stdout.as_bytes());
    }
}
