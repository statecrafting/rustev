//! Hand-authored synthetic measures establish mechanics only.
#![allow(dead_code)]
use rustev_contract::decimal::Decimal;
use rustev_contract::{Document, Identified};
use rustev_eval::baseline::{Baseline, record};
use rustev_eval::comparison::{NamedCandidate, RepeatEvidence};
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::DatasetManifest;
use rustev_eval::eval_cases::EvalCases;
use rustev_eval::repeat::{RepeatInputs, RepeatReport, Run, Source, repeat};
use rustev_eval::repeat_config::RepeatConfig;
use rustev_eval::report::Evaluated;
use rustev_eval::selection::{Selection, SelectionGate, evaluate_selection};
use serde_json::{Value, json};
pub fn digest(n: u8) -> String {
    format!("sha256:{n:064x}")
}
pub fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}
pub fn gate() -> SelectionGate {
    SelectionGate::parse(json!({"schema":"rustev.selection-gate/1","gate":"test","per":"variant","metrics":{"mean_log_loss":"lower_is_better","coverage":"higher_is_better","latency_ms/p0.5":"lower_is_better","cost_per_1000/observed":"lower_is_better"},"min_comparable_coverage":"1","min_labeled_coverage":"1","require":[{"metric":"mean_log_loss","not_worse_than":"baseline","within":"noise"}],"rank_by":["mean_log_loss"]}).to_string().as_bytes()).unwrap()
}
#[derive(Clone)]
pub struct Fixture {
    pub manifest: DatasetManifest,
    pub config: RepeatConfig,
    pub b: Evaluated,
    pub c: Evaluated,
    pub bc: EvalCases,
    pub cc: EvalCases,
    pub source: Source,
}
impl Fixture {
    pub fn new() -> Self {
        let manifest: DatasetManifest = serde_json::from_value(json!({"schema":"rustev.dataset/1","name":"synthetic-selection","provenance":{"synthetic":{"generator":"hand-authored mechanics"}},"adapter":{"name":"test","version":"1"},"cases":[{"id":"a","source":"a","snapshot":digest(1),"split":"model_selection","label":{"value":"yes"},"subgroups":["slice"]},{"id":"b","source":"b","snapshot":digest(2),"split":"model_selection","label":{"value":"yes"},"subgroups":[]}]})).unwrap();
        let config = RepeatConfig::parse(json!({"schema":"rustev.repeat-config/1","bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.95","resamples":1000,"seed":"29"},"noise":{"rule":{"k_sd":{"k":"2"}},"floor":"0","overrides":{}},"slices":[{"name":"slice","min_cases":1}],"stability_metrics":["mean_log_loss"],"top_k":null}).to_string().as_bytes()).unwrap();
        let names = [
            "coverage",
            "acceptance_coverage",
            "labeled_proposal_coverage",
            "error_among_accepted",
            "mean_log_loss",
            "candidate_outcome_change",
            "latency_ms/p0.5",
        ];
        let ec: EvaluatorConfig = serde_json::from_value(json!({"schema":"rustev.evaluator-config/2","adapter":{"name":"test","version":"1","rules":{"bound":digest(3)}},"label_interpretation":"synthetic","agreement":{"params":[]},"probability_step":null,"reliability_bins":["0","1"],"subgroups":["slice"],"latency_quantiles":["0.5"],"cost_units_comparable":true,"metrics":names.iter().map(|n| json!({"name":n,"formula":"synthetic"})).collect::<Vec<_>>(),"gates":[],"temperature_grid":[]})).unwrap();
        let build = |candidate: bool| {
            let metrics: Vec<_> = names.iter().filter(|n| candidate || **n != "candidate_outcome_change").map(|n| json!({"name":n,"value":if *n == "latency_ms/p0.5" && candidate { json!({"unknown":{"reason":"historical"}}) } else { json!({"measured":{"value":match *n { "mean_log_loss" => 0.5, "error_among_accepted" | "candidate_outcome_change" => 0.0, "latency_ms/p0.5" => 10.0, _ => 1.0 },"n":2}}) }})).collect();
            let report: rustev_contract::eval_report::EvalReport = serde_json::from_value(json!({"schema":"rustev.eval-report/1","plan":digest(if candidate {5} else {4}),"artifacts":[],"calibrations":[],"dataset":{"id":manifest.id().unwrap(),"split":"model_selection","provenance":manifest.provenance},"evaluator_config":ec.id().unwrap(),"status":"complete","metrics":metrics})).unwrap();
            let mut counts: Vec<_> = ["coverage","acceptance_coverage","labeled_proposal_coverage","error_among_accepted","probability_scored"].iter().map(|n| json!({"metric":n,"numerator":if *n == "error_among_accepted" {0} else {2},"denominator":2,"breakdown":{}})).collect();
            if candidate {
                counts.push(json!({"metric":"candidate_outcome_change","numerator":0,"denominator":2,"breakdown":{}}));
            }
            let detail = serde_json::from_value(json!({"schema":"rustev.eval-detail/1","report":report.record_digest().unwrap(),"dataset":manifest.id().unwrap(),"split":"model_selection","config":ec.id().unwrap(),"cohort":"all","counts":counts,"cases":[{"case":"a","scope":null,"outcome":"proposal:yes","reason":null},{"case":"b","scope":null,"outcome":"proposal:yes","reason":null}]})).unwrap();
            let mut measures = json!({"coverage":{"count":{"num":1,"den":1}},"acceptance_coverage":{"count":{"num":1,"den":1}},"labeled_proposal_coverage":{"count":{"num":1,"den":1}},"error_among_accepted":{"count":{"num":0,"den":1}},"mean_log_loss":{"mean":{"num":0.5,"den":1}}});
            if candidate {
                measures["candidate_outcome_change"] = json!({"count":{"num":0,"den":1}});
            } else {
                measures["latency_ms/p0.5"] = json!({"sample":{"value":10.0}});
            }
            let cases = serde_json::from_value(json!({"schema":"rustev.eval-cases/1","report":report.record_digest().unwrap(),"top_k":null,"cases":[{"case":"a","run":digest(6),"outcome":"proposal:yes","top_k":null,"measures":measures},{"case":"b","run":digest(7),"outcome":"proposal:yes","top_k":null,"measures":measures}]})).unwrap();
            (
                Evaluated {
                    report,
                    detail,
                    config: ec.clone(),
                },
                cases,
            )
        };
        let (b, bc) = build(false);
        let (c, cc) = build(true);
        Self {
            manifest,
            config,
            b,
            c,
            bc,
            cc,
            source: Source::Replay,
        }
    }
    pub fn runs(&self, candidate: bool) -> Vec<Run<'_>> {
        vec![if candidate {
            Run {
                evaluated: &self.c,
                cases: &self.cc,
            }
        } else {
            Run {
                evaluated: &self.b,
                cases: &self.bc,
            }
        }]
    }
    pub fn inputs<'a>(&'a self, runs: &'a [Run<'a>]) -> RepeatInputs<'a> {
        RepeatInputs {
            source: self.source,
            config: &self.config,
            manifest: &self.manifest,
            runs,
        }
    }
    pub fn reports(&self) -> (Baseline, RepeatReport, RepeatReport) {
        let br = self.runs(false);
        let cr = self.runs(true);
        let bi = self.inputs(&br);
        let ci = self.inputs(&cr);
        let b = repeat(&bi).unwrap();
        let c = repeat(&ci).unwrap();
        (record("base", &bi, &b, 0).unwrap(), b, c)
    }
    pub fn select(&self, gate: &SelectionGate) -> Selection {
        let (base, b, c) = self.reports();
        self.select_reports(gate, &base, &b, &c)
    }
    pub fn select_reports(
        &self,
        gate: &SelectionGate,
        base: &Baseline,
        b: &RepeatReport,
        c: &RepeatReport,
    ) -> Selection {
        let br = self.runs(false);
        let cr = self.runs(true);
        evaluate_selection(
            gate,
            base,
            &RepeatEvidence {
                report: b,
                inputs: self.inputs(&br),
            },
            &[NamedCandidate {
                name: "candidate",
                evidence: RepeatEvidence {
                    report: c,
                    inputs: self.inputs(&cr),
                },
            }],
        )
        .unwrap()
    }
    pub fn loss(&mut self, candidate: bool, a: f64, b: f64) {
        let (e, cases) = if candidate {
            (&mut self.c, &mut self.cc)
        } else {
            (&mut self.b, &mut self.bc)
        };
        for (case, v) in cases.cases.iter_mut().zip([a, b]) {
            case.measures.insert(
                "mean_log_loss".into(),
                rustev_eval::eval_cases::CaseMeasure::Mean { num: v, den: 1 },
            );
        }
        e.report
            .metrics
            .iter_mut()
            .find(|m| m.name == "mean_log_loss")
            .unwrap()
            .value = rustev_contract::eval_report::MetricValue::Measured {
            value: (a + b) / 2.0,
            n: 2,
        };
        Self::bind(e, cases);
    }
    pub fn bind(e: &mut Evaluated, cases: &mut EvalCases) {
        e.report.evaluator_config = e.config.id().unwrap();
        e.detail.config = e.report.evaluator_config.clone();
        e.detail.report = e.report.record_digest().unwrap();
        cases.report = e.detail.report.clone();
    }
    pub fn held_out(&mut self) {
        for c in &mut self.manifest.cases {
            c.split = rustev_contract::eval_report::Split::FinalTest;
        }
        for (e, c) in [(&mut self.b, &mut self.bc), (&mut self.c, &mut self.cc)] {
            e.report.dataset.id = self.manifest.id().unwrap();
            e.report.dataset.split = rustev_contract::eval_report::Split::FinalTest;
            e.detail.dataset = e.report.dataset.id.clone();
            e.detail.split = e.report.dataset.split;
            Self::bind(e, c);
        }
    }
}
pub fn edit_gate(g: &SelectionGate, f: impl FnOnce(&mut Value)) -> SelectionGate {
    let mut v = serde_json::to_value(g).unwrap();
    f(&mut v);
    SelectionGate::parse(v.to_string().as_bytes()).unwrap()
}
pub fn golden(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/selection")
        .join(name);
    if std::env::var_os("UPDATE_SELECTION_GOLDENS").is_some() {
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes, "golden {name}");
}
impl Fixture {
    /// Replace case b with an abstention or an unavailable retained bundle,
    /// rebuilding report metrics from the hand-authored case contributions.
    pub fn second_outcome(&mut self, candidate: bool, incomparable: bool) {
        use rustev_contract::eval_report::MetricValue;
        use rustev_eval::eval_cases::{CaseMeasure, form_of, metric_cases};
        use rustev_eval::repeat_stats::{Figure, run_stats};
        let (e, c) = if candidate {
            (&mut self.c, &mut self.cc)
        } else {
            (&mut self.b, &mut self.bc)
        };
        let outcome = if incomparable {
            "incomparable"
        } else {
            "abstention:uncertain"
        };
        e.detail.cases[1].outcome = outcome.into();
        e.detail.cases[1].reason = incomparable.then(|| "bundle-missing".into());
        c.cases[1].outcome = outcome.into();
        if incomparable {
            c.cases[1].run = None;
            c.cases[1].measures.clear();
            c.cases[1]
                .measures
                .insert("coverage".into(), CaseMeasure::Count { num: 0, den: 1 });
        } else {
            c.cases[1].measures.insert(
                "acceptance_coverage".into(),
                CaseMeasure::Count { num: 0, den: 1 },
            );
            c.cases[1].measures.remove("labeled_proposal_coverage");
            c.cases[1].measures.remove("error_among_accepted");
        }
        for count in &mut e.detail.counts {
            count.breakdown.clear();
            match count.metric.as_str() {
                "coverage" => {
                    count.numerator = if incomparable { 1 } else { 2 };
                    count.denominator = 2;
                    if incomparable {
                        count.breakdown.insert("bundle-missing".into(), 1);
                    }
                }
                "acceptance_coverage" => {
                    count.numerator = 1;
                    count.denominator = if incomparable { 1 } else { 2 };
                    if !incomparable {
                        count.breakdown.insert("abstention".into(), 1);
                    }
                }
                "labeled_proposal_coverage" => {
                    count.numerator = 1;
                    count.denominator = 1;
                }
                "error_among_accepted" => {
                    count.numerator = 0;
                    count.denominator = 1;
                }
                "probability_scored" => {
                    count.numerator = if incomparable { 1 } else { 2 };
                    count.denominator = count.numerator;
                }
                "candidate_outcome_change" => {
                    count.numerator = 0;
                    count.denominator = if incomparable { 1 } else { 2 };
                }
                _ => {}
            }
        }
        for metric in &mut e.report.metrics {
            if let Some(form) = form_of(&metric.name) {
                let cases = metric_cases(&metric.name, form, &[c]).unwrap();
                metric.value = match run_stats(&cases, 1, false).unwrap().pooled {
                    Figure::Known(value) => MetricValue::Measured { value, n: 2 },
                    Figure::Unknown { reason } => MetricValue::Unknown { reason },
                };
            }
        }
        Self::bind(e, c);
    }
}
