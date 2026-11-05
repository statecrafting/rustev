//! Spec 061, 3.6 to 3.8: host baselines, the reproducibility check and
//! selection over host sets, and provenance separation.

mod host_support;

use host_support::{HostRunDocs, RunSpec, config, repeat_config, set_docs};
use rustev_contract::Document;
use rustev_contract::eval_report::Split;
use rustev_eval::baseline::Reproducibility;
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::host_repeat::{HostSet, HostSetInputs};
use rustev_eval::host_run::HostRunInputs;
use rustev_eval::host_select::{
    HOST_BASELINE, HostBaseline, HostSelection, HostSetEvidence, NamedHostCandidate, Provenance,
    evaluate_selection, one_provenance, provenance, record, reproducibility, verify_baseline,
};
use rustev_eval::repeat::{BandDoc, FigureDoc};
use rustev_eval::repeat_config::RepeatConfig;
use rustev_eval::selection::{ClauseOutcome, SelectionGate, Verdict};
use serde_json::{Value, json};

/// One host set: its run documents and its import.
struct Set {
    docs: Vec<HostRunDocs>,
    set: HostSet,
    runs: usize,
}

impl Set {
    fn new(c: &EvaluatorConfig, rc: &RepeatConfig, specs: &[RunSpec]) -> Set {
        let docs = set_docs(c, specs);
        let set = host_support::import(c, rc, &docs).unwrap();
        Set {
            runs: docs.len(),
            docs,
            set,
        }
    }

    fn inputs(&self) -> Vec<HostRunInputs<'_>> {
        self.docs.iter().map(HostRunDocs::inputs).collect()
    }
}

fn evidence<'a>(
    s: &'a Set,
    runs: &'a [HostRunInputs<'a>],
    c: &'a EvaluatorConfig,
    rc: &'a RepeatConfig,
) -> HostSetEvidence<'a> {
    assert_eq!(runs.len(), s.runs);
    HostSetEvidence {
        report: &s.set.report,
        cases: &s.set.cases,
        inputs: HostSetInputs {
            config: rc,
            evaluator: c,
            runs,
        },
    }
}

/// The baseline flow: three runs, no unknown latency.
fn base_specs() -> Vec<RunSpec> {
    (0..3).map(RunSpec::new).collect()
}

/// A candidate flow version: three runs with `edit` applied.
fn candidate(edit: impl Fn(&mut RunSpec)) -> Vec<RunSpec> {
    (0..3)
        .map(|r| {
            let mut s = RunSpec::new(r);
            s.flow_version = "v2".into();
            edit(&mut s);
            s
        })
        .collect()
}

/// The consumer's gate of 3.7.6.
fn gate_json() -> Value {
    json!({
        "schema": "rustev.selection-gate/1", "gate": "host-flow", "per": "variant",
        "metrics": {
            "rubric/brevity": "higher_is_better",
            "rubric/tone_index": "higher_is_better",
            "host/latency_ms/p0.5": "lower_is_better",
            "host/latency_ms/p0.95": "lower_is_better",
            "host/cost_per_1000": "lower_is_better"
        },
        "min_comparable_coverage": "0.8", "min_labeled_coverage": "0.4",
        "require": [
            {"all_slices": {"metric": "rubric/brevity", "not_worse_than": "baseline", "within": "noise"}}
        ],
        "and_any": [
            {"metric": "host/latency_ms/p0.5", "better_than": "baseline"},
            {"metric": "host/latency_ms/p0.95", "better_than": "baseline"},
            {"metric": "host/cost_per_1000", "better_than": "baseline"}
        ],
        "rank_by": ["host/cost_per_1000"]
    })
}

fn gate(v: &Value) -> SelectionGate {
    SelectionGate::parse(&serde_json::to_vec(v).unwrap()).unwrap()
}

/// Select over named candidate sets against the baseline set.
fn select(g: &SelectionGate, base: &Set, cands: &[(&str, &Set)]) -> HostSelection {
    let c = config();
    let rc = repeat_config();
    let base_runs = base.inputs();
    let b = evidence(base, &base_runs, &c, &rc);
    let baseline = record("base", &b, 0).unwrap();
    let runs: Vec<Vec<HostRunInputs<'_>>> = cands.iter().map(|(_, s)| s.inputs()).collect();
    let named: Vec<NamedHostCandidate<'_>> = cands
        .iter()
        .zip(&runs)
        .map(|((n, s), r)| NamedHostCandidate {
            name: n,
            evidence: evidence(s, r, &c, &rc),
        })
        .collect();
    evaluate_selection(g, &baseline, &b, &named).unwrap()
}

fn verdict<'a>(sel: &'a HostSelection, name: &str) -> &'a Verdict {
    &sel.candidates
        .iter()
        .find(|c| c.name == name)
        .unwrap()
        .verdict
}

fn unknown_reason(sel: &HostSelection, name: &str) -> String {
    let c = sel.candidates.iter().find(|c| c.name == name).unwrap();
    let Verdict::Unknown { clause } = &c.verdict else {
        panic!("{name}: {:?}", c.verdict)
    };
    let r = c.clauses.iter().find(|r| &r.clause == clause).unwrap();
    match &r.result {
        ClauseOutcome::Unknown { reason } => reason.clone(),
        other => format!("{other:?}"),
    }
}

#[test]
fn provenance_is_told_apart_by_schema() {
    assert_eq!(provenance(HOST_BASELINE), Some(Provenance::Host));
    assert_eq!(provenance("rustev.baseline/1"), Some(Provenance::Rustev));
    assert_eq!(
        provenance("rustev.repeat-report/1"),
        Some(Provenance::Rustev)
    );
    assert_eq!(provenance("rustev.dataset/1"), None);
    for mixed in [
        &[HOST_BASELINE, "rustev.repeat-report/1"][..],
        &["rustev.baseline/1", "rustev.host-repeat-report/1"],
        &["rustev.host-repeat-report/1", "rustev.repeat-report/1"],
    ] {
        assert_eq!(one_provenance(mixed).unwrap_err().code, "mixed-provenance");
    }
    assert_eq!(
        one_provenance(&[HOST_BASELINE, "rustev.host-repeat-report/1"]).unwrap(),
        Some(Provenance::Host)
    );
    // A Rustev baseline is not a host baseline, by schema.
    let rustev = json!({"schema": "rustev.baseline/1"});
    assert!(HostBaseline::parse(&serde_json::to_vec(&rustev).unwrap()).is_err());
}

#[test]
fn a_host_baseline_binds_its_set() {
    let c = config();
    let rc = repeat_config();
    let base = Set::new(&c, &rc, &base_specs());
    let runs = base.inputs();
    let e = evidence(&base, &runs, &c, &rc);
    let b = record("base", &e, 1_700_000_000_000).unwrap();
    let text = String::from_utf8(b.record_canonical().unwrap()).unwrap();
    assert!(text.contains("\"assertion\":\"host\"") && text.contains("\"source\":\"host\""));
    assert_eq!(HostBaseline::parse(text.as_bytes()).unwrap(), b);
    assert_eq!(b.n, 3);
    assert_eq!(
        b.host_repeat_report,
        base.set.report.record_digest().unwrap()
    );
    verify_baseline(&b, &e).unwrap();
    let mut tampered = b.clone();
    tampered.cohorts[0].metrics[0].pooled = FigureDoc::Value(0.25);
    assert_eq!(verify_baseline(&tampered, &e).unwrap_err().code, "baseline");
    assert_eq!(record("bad name", &e, 0).unwrap_err().code, "baseline");
    let mut cases = base.set.cases.clone();
    cases[0].cases.pop();
    let changed = HostSetEvidence {
        cases: &cases,
        ..evidence(&base, &runs, &c, &rc)
    };
    assert_eq!(record("base", &changed, 0).unwrap_err().code, "reconcile");
}

#[test]
fn reproducibility_at_and_beyond_the_band() {
    let c = config();
    let rc = repeat_config();
    let base = Set::new(&c, &rc, &base_specs());
    let runs = base.inputs();
    let mut b = record("base", &evidence(&base, &runs, &c, &rc), 0).unwrap();
    assert_eq!(
        reproducibility(&b, &base.set.report, &rc),
        Reproducibility::Reproducible
    );
    // Another run set of the same flow, shifted: exactly at the band is
    // reproducible, one ulp beyond is not (exit 13 in the CLI).
    let other = Set::new(
        &c,
        &rc,
        &(0..3)
            .map(|r| RunSpec {
                score_shift: 0.05 * r as f64 + 0.02,
                ..RunSpec::new(r)
            })
            .collect::<Vec<_>>(),
    );
    let pooled = |r: &rustev_eval::host_repeat::HostRepeatReport, cohort: usize| match &r.cohorts
        [cohort]
        .metrics
        .iter()
        .find(|m| m.name == "rubric/brevity")
        .unwrap()
        .pooled
    {
        FigureDoc::Value(v) => *v,
        other => panic!("{other:?}"),
    };
    for cohort in 0..3 {
        let delta = (pooled(&other.set.report, cohort) - pooled(&base.set.report, cohort)).abs();
        let m = b.cohorts[cohort]
            .metrics
            .iter_mut()
            .find(|m| m.name == "rubric/brevity")
            .unwrap();
        m.band = BandDoc::Value {
            value: delta,
            basis: rustev_eval::repeat::BasisDoc::KSd,
        };
    }
    // The latency stability metric is unchanged between the two sets.
    assert_eq!(
        reproducibility(&b, &other.set.report, &rc),
        Reproducibility::Reproducible
    );
    let m = b.cohorts[1]
        .metrics
        .iter_mut()
        .find(|m| m.name == "rubric/brevity")
        .unwrap();
    let BandDoc::Value { value, .. } = &mut m.band else {
        unreachable!()
    };
    *value = f64::from_bits(value.to_bits() - 1);
    assert!(matches!(
        reproducibility(&b, &other.set.report, &rc),
        Reproducibility::NotReproducible { exceedances } if exceedances.len() == 1 && exceedances[0].cohort == "north"
    ));
    // Another flow version is not the same configuration.
    let v2 = Set::new(&c, &rc, &candidate(|_| {}));
    let b = record("base", &evidence(&base, &runs, &c, &rc), 0).unwrap();
    assert!(matches!(
        reproducibility(&b, &v2.set.report, &rc),
        Reproducibility::Unknown { .. }
    ));
}

#[test]
fn selection_qualifies_on_latency_and_on_cost() {
    let c = config();
    let rc = repeat_config();
    let base = Set::new(&c, &rc, &base_specs());
    let faster = Set::new(&c, &rc, &candidate(|s| s.latency_base = 500));
    let cheaper = Set::new(&c, &rc, &candidate(|s| s.cost_shift -= 2_400_000));
    let g = gate(&gate_json());
    let sel = select(&g, &base, &[("faster", &faster), ("cheaper", &cheaper)]);
    assert_eq!(*verdict(&sel, "faster"), Verdict::Qualifies);
    assert_eq!(*verdict(&sel, "cheaper"), Verdict::Qualifies);
    assert_eq!(sel.exit_code(), 0);
    // Ranked by cost, lower first.
    assert_eq!(sel.ranked, ["cheaper", "faster"]);
    let text = String::from_utf8(sel.record_canonical().unwrap()).unwrap();
    assert!(text.contains("\"assertion\":\"host\"") && text.contains("\"flow_version\":\"v2\""));
    assert_eq!(HostSelection::parse(text.as_bytes()).unwrap(), sel);
    // The faster candidate wins on latency, not cost.
    let f = sel.candidates.iter().find(|c| c.name == "faster").unwrap();
    let r = |n: &str| &f.clauses.iter().find(|c| c.clause == n).unwrap().result;
    assert!(matches!(r("and_any[0]"), ClauseOutcome::Pass { .. }));
    assert!(matches!(r("and_any[2]"), ClauseOutcome::Fail { .. }));
}

#[test]
fn selection_rejects_on_a_slice_and_on_coverage() {
    let c = config();
    let rc = repeat_config();
    let base = Set::new(&c, &rc, &base_specs());
    let worse = Set::new(
        &c,
        &rc,
        &candidate(|s| {
            s.latency_base = 500;
            s.score_shift -= 0.15;
        }),
    );
    // Fails one more case than the baseline: measured, never unknown.
    let failing = Set::new(
        &c,
        &rc,
        &candidate(|s| {
            s.latency_base = 500;
            s.extra_failed = vec!["c4"];
        }),
    );
    let g = gate(&gate_json());
    let sel = select(&g, &base, &[("worse", &worse), ("failing", &failing)]);
    assert_eq!(
        *verdict(&sel, "worse"),
        Verdict::Rejected {
            clause: "require[0]".into()
        }
    );
    let w = sel.candidates.iter().find(|c| c.name == "worse").unwrap();
    let slices = &w
        .clauses
        .iter()
        .find(|c| c.clause == "require[0]")
        .unwrap()
        .slices;
    assert!(
        slices
            .iter()
            .any(|s| matches!(s.result, ClauseOutcome::Fail { .. }))
    );
    assert_eq!(
        *verdict(&sel, "failing"),
        Verdict::Rejected {
            clause: "candidate.runs[0].comparable_coverage".into()
        }
    );
    assert_eq!(sel.exit_code(), 11);
}

#[test]
fn selection_unknowns() {
    let c = config();
    let rc = repeat_config();
    let base = Set::new(&c, &rc, &base_specs());
    let g = gate(&gate_json());
    // N = 1 under k_sd: the candidate's band is unknown, so `within: noise`
    // is unknown.
    let single = Set::new(&c, &rc, &candidate(|s| s.latency_base = 500)[..1]);
    // An unknown latency in one run: no `and_any` clause passes.
    let blind = Set::new(
        &c,
        &rc,
        &candidate(|s| {
            if s.run == "run-1" {
                s.unknown_latency = Some("c2");
            }
        }),
    );
    // Another cost unit.
    let euro = Set::new(
        &c,
        &rc,
        &candidate(|s| {
            s.latency_base = 500;
            s.cost_unit = Some("eur".into());
        }),
    );
    let sel = select(
        &g,
        &base,
        &[("single", &single), ("blind", &blind), ("euro", &euro)],
    );
    assert!(unknown_reason(&sel, "single").contains("band"), "{sel:?}");
    assert!(
        matches!(verdict(&sel, "blind"), Verdict::Unknown { clause } if clause.starts_with("and_any["))
    );
    assert_eq!(unknown_reason(&sel, "euro"), "cost unit mismatch");
    assert_eq!(sel.exit_code(), 12);

    // An opaque scorer's metric.
    let mut v = gate_json();
    v["require"] = json!([{"metric": "rubric/tone_index", "not_worse_than": "baseline", "within": {"absolute": "1"}}]);
    let faster = Set::new(&c, &rc, &candidate(|s| s.latency_base = 500));
    let sel = select(&gate(&v), &base, &[("faster", &faster)]);
    assert!(unknown_reason(&sel, "faster").contains("opaque scorer"));
    assert_eq!(sel.exit_code(), 12);
}

#[test]
fn a_held_out_host_set_is_unknown_under_a_held_out_clause() {
    let c = config();
    let rc = repeat_config();
    let held = |edit: &dyn Fn(&mut RunSpec)| {
        (0..3)
            .map(|r| {
                let mut s = RunSpec::new(r);
                s.split = Split::FinalTest;
                edit(&mut s);
                s
            })
            .collect::<Vec<_>>()
    };
    let base = Set::new(&c, &rc, &held(&|_| {}));
    let cand = Set::new(
        &c,
        &rc,
        &held(&|s| {
            s.flow_version = "v2".into();
            s.latency_base = 500;
        }),
    );
    let mut v = gate_json();
    v["require"]
        .as_array_mut()
        .unwrap()
        .push(json!({"held_out": {"max_evaluations": 3}}));
    let sel = select(&gate(&v), &base, &[("cand", &cand)]);
    assert_eq!(unknown_reason(&sel, "cand"), "host-ledger-unsupported");
    assert_eq!(sel.exit_code(), 12);
}

#[test]
fn a_candidate_changed_after_import_is_unknown() {
    let c = config();
    let rc = repeat_config();
    let base = Set::new(&c, &rc, &base_specs());
    let mut changed = Set::new(&c, &rc, &candidate(|s| s.latency_base = 500));
    // A latency edited after import: the record no longer re-assembles to
    // the imported report.
    changed.docs[0].record.cases[0].latency_us = rustev_eval::host_run::HostLatency::Us(1);
    let sel = select(&gate(&gate_json()), &base, &[("changed", &changed)]);
    assert!(
        unknown_reason(&sel, "changed").starts_with("reconcile"),
        "{sel:?}"
    );
}
