//! Spec 041, 3.5: origin and evidence-quality metrics over evidence records
//! built from the spec 035 and 036 goldens, hand-computed, with their counts
//! and per-case measures.

use std::collections::BTreeMap;

use rustev_contract::eval_report::{MetricValue, Split};
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::ids::SnapshotId;
use rustev_contract::judgment::OutValue;
use rustev_contract::plan::Plan;
use rustev_eval::dataset::{DatasetCase, Label};
use rustev_eval::eval_cases::{CaseMeasure, form_of};
use rustev_eval::measured::{NewMetric, Scope};
use rustev_eval::measures::FieldDecl;
use rustev_eval::origin::{OriginCase, origin_metrics};
use rustev_eval::repeat_stats::Form;
use rustev_eval::scoring::OriginFamily;
use serde_json::{Value, json};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../rustev-core/tests/golden/");

fn golden(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("{GOLDEN}{name}")).unwrap()).unwrap()
}

fn plan(v: Value) -> Plan {
    serde_json::from_value(v).unwrap()
}

fn evidence(v: Value) -> EvidenceRecord {
    serde_json::from_value(v).unwrap()
}

fn case(id: &str) -> DatasetCase {
    DatasetCase {
        id: id.into(),
        source: id.into(),
        snapshot: SnapshotId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap(),
        split: Split::FinalTest,
        label: Label::Missing,
        subgroups: vec![],
        gold: None,
    }
}

fn find<'a>(out: &'a [NewMetric], name: &str) -> &'a NewMetric {
    out.iter()
        .find(|m| m.metric.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn value(m: &NewMetric) -> f64 {
    match m.metric.value {
        MetricValue::Measured { value, .. } => value,
        ref v => panic!("{}: {v:?}", m.metric.name),
    }
}

fn breakdown(m: &NewMetric) -> Vec<(&str, u64)> {
    m.count
        .breakdown
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect()
}

fn count(num: u64, den: u64) -> Option<CaseMeasure> {
    Some(CaseMeasure::Count { num, den })
}

/// The field declarations the extraction family reads (3.4.7).
fn fields() -> Vec<FieldDecl> {
    ["party_size", "features", "area_name"]
        .into_iter()
        .map(|f| FieldDecl {
            name: format!("filters.{f}"),
            step: "request_filters".into(),
            field: f.into(),
        })
        .collect()
}

/// Evaluates `family` over these cases, each a plan and an evidence record,
/// plus one abstained case outside every new metric.
fn run(family: OriginFamily, cases: &[(Plan, Option<EvidenceRecord>)]) -> Vec<NewMetric> {
    run_gold(family, cases, &[])
}

/// As `run`, with the gold record of each case that has one.
fn run_gold(
    family: OriginFamily,
    cases: &[(Plan, Option<EvidenceRecord>)],
    gold: &[Option<Value>],
) -> Vec<NewMetric> {
    let ids: Vec<DatasetCase> = (0..cases.len())
        .map(|i| {
            let mut c = case(&format!("c{i}"));
            c.gold = gold
                .get(i)
                .cloned()
                .flatten()
                .map(|g| serde_json::from_value(g).unwrap());
            c
        })
        .collect();
    let params = BTreeMap::<String, OutValue>::new();
    let mut scope: Vec<Scope<'_>> = ids
        .iter()
        .map(|case| Scope::Proposal {
            case,
            params: &params,
        })
        .collect();
    scope.push(Scope::Out("abstained".into()));
    let mut origin: Vec<Option<OriginCase<'_>>> = cases
        .iter()
        .map(|(plan, evidence)| {
            Some(OriginCase {
                plan,
                evidence: evidence.as_ref(),
            })
        })
        .collect();
    origin.push(None);
    origin_metrics(&[family], &fields(), &scope, &origin)
}

#[test]
fn screening_rates_per_screener_exclude_empty_values() {
    let p = plan(golden("screening.plan.json"));
    let records = ["fail-open", "drop-item", "pass"]
        .map(|n| evidence(golden(&format!("screening.eval.{n}.json"))));
    let cases: Vec<_> = records.into_iter().map(|e| (p.clone(), Some(e))).collect();
    let out = run(OriginFamily::Screening, &cases);
    let names: Vec<&str> = out.iter().map(|m| m.metric.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "origin/screening/screen/block_rate",
            "origin/screening/screen/unavailable_rate"
        ]
    );
    // fail-open: empty, pass, unavailable, pass; drop-item: pass, blocked,
    // blocked, pass; pass: four passes. Empty is outside the denominator.
    let block = find(&out, "origin/screening/screen/block_rate");
    assert_eq!(block.cases, [count(0, 3), count(2, 4), count(0, 4), None]);
    assert_eq!(value(block), 2.0 / 11.0);
    assert_eq!((block.count.numerator, block.count.denominator), (2, 11));
    assert_eq!(breakdown(block), [("abstained", 1), ("empty", 1)]);
    let unavailable = find(&out, "origin/screening/screen/unavailable_rate");
    assert_eq!(unavailable.cases[0], count(1, 3));
    assert_eq!(value(unavailable), 1.0 / 11.0);
}

#[test]
fn a_plan_without_the_construct_makes_every_metric_unknown() {
    // A screening plan has no corroboration step.
    let p = plan(golden("screening.plan.json"));
    let e = evidence(golden("screening.eval.pass.json"));
    let out = run(OriginFamily::Corroboration, &[(p.clone(), Some(e))]);
    assert_eq!(out.len(), 1);
    let m = &out[0];
    assert_eq!(m.metric.name, "origin/corroboration/hint_disagreement_rate");
    assert_eq!(
        m.metric.value,
        MetricValue::Unknown {
            reason: "zero denominator; excluded abstained: 1, not_in_plan: 1".into()
        }
    );
    // A family named per screener or step reports nothing for a plan that
    // binds none.
    let mut q = golden("screening.plan.json");
    q.as_object_mut().unwrap().remove("screening");
    assert!(run(OriginFamily::Screening, &[(plan(q), None)]).is_empty());
    let unbound = run(OriginFamily::AcquisitionTier, &[(p, None)]);
    assert!(unbound.is_empty());
}

#[test]
fn corroboration_counts_contradicted_over_decided_outcomes() {
    let mut p = golden("screening.plan.json");
    p["steps"][0]["detail"] = json!("corroborate");
    let p = plan(p);
    let outcome = |o: &str| {
        json!({"claim": "x", "claim_digest": "sha256:0", "grade_before": "hint",
               "grade_after": "hint", "outcome": o, "checks": []})
    };
    let with = |outcomes: &[&str]| {
        let mut e = golden("screening.eval.pass.json");
        let step = e["steps"][0].clone();
        e["steps"] = Value::Array(
            outcomes
                .iter()
                .map(|o| {
                    let mut s = step.clone();
                    s["corroboration"] = outcome(o);
                    s
                })
                .collect(),
        );
        evidence(e)
    };
    let cases = [
        (p.clone(), Some(with(&["contradicted", "corroborated"]))),
        (p.clone(), Some(with(&["uncorroborated"]))),
        (p.clone(), Some(with(&["corroborated", "uncorroborated"]))),
        (p, None),
    ];
    let out = run(OriginFamily::Corroboration, &cases);
    let m = &out[0];
    assert_eq!(m.cases, [count(1, 2), None, count(0, 1), None, None]);
    assert_eq!(value(m), 1.0 / 3.0);
    assert_eq!(
        breakdown(m),
        [
            ("abstained", 1),
            ("no_evidence", 1),
            ("no_outcome", 1),
            ("uncorroborated", 2)
        ]
    );
}

fn tier(outcome: Value) -> Value {
    json!({"name": "t", "outcome": outcome, "relaxation": 0, "source": "s",
           "operation": "o", "request_constructed": true, "items_received": 0,
           "unique_items_added": 0, "duplicates": 0, "pages": []})
}

fn widened(outcome: Value, stopped_by: &str, tiers: &[Value]) -> EvidenceRecord {
    let mut e = golden("screening.eval.pass.json");
    e["acquisitions"] = json!([{
        "step": "lookup", "kind": "widen", "outcome": outcome, "stopped_by": stopped_by,
        "truncated": 0, "request_constructed": true,
        "tiers": tiers.iter().cloned().map(tier).collect::<Vec<_>>(), "items": []
    }]);
    evidence(e)
}

#[test]
fn acquisition_tiers_count_where_each_decision_stopped() {
    let p = plan(golden("acquisition-widen.plan.json"));
    let (ok, empty, not_needed, skipped) = (
        json!("ok"),
        json!("empty"),
        json!("not_needed"),
        json!("skipped"),
    );
    let cases = [
        // Stopped at tier 0; tier 1 was not needed.
        (
            p.clone(),
            Some(widened(
                json!("resolved"),
                "stop_condition",
                &[ok.clone(), not_needed],
            )),
        ),
        // Ran out of tiers at tier 1.
        (
            p.clone(),
            Some(widened(
                json!("resolved"),
                "tiers_exhausted",
                &[empty, ok.clone()],
            )),
        ),
        // Every tier skipped by its guard.
        (
            p.clone(),
            Some(widened(
                json!({"skipped": {"guard": "g"}}),
                "none",
                &[skipped.clone(), skipped],
            )),
        ),
        // Never reached the step.
        (p, Some(evidence(golden("screening.eval.pass.json")))),
    ];
    let out = run(OriginFamily::AcquisitionTier, &cases);
    let names: Vec<&str> = out.iter().map(|m| m.metric.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "origin/acquisition_tier/lookup/tier/0",
            "origin/acquisition_tier/lookup/tier/1",
            "origin/acquisition_tier/lookup/exhausted"
        ]
    );
    let t0 = find(&out, "origin/acquisition_tier/lookup/tier/0");
    assert_eq!(t0.cases, [count(1, 1), count(0, 1), None, None, None]);
    assert_eq!(value(t0), 0.5);
    assert_eq!(
        breakdown(t0),
        [("abstained", 1), ("not_run", 1), ("skipped", 1)]
    );
    assert_eq!(
        value(find(&out, "origin/acquisition_tier/lookup/tier/1")),
        0.5
    );
    let exhausted = find(&out, "origin/acquisition_tier/lookup/exhausted");
    assert_eq!(exhausted.cases[..2], [count(0, 1), count(1, 1)]);
    assert_eq!(value(exhausted), 0.5);
}

#[test]
fn every_origin_rate_is_a_count_ratio() {
    for name in [
        "origin/corroboration/hint_disagreement_rate",
        "origin/screening/screen/block_rate",
        "origin/acquisition_tier/lookup/tier/0",
        "origin/acquisition_tier/lookup/exhausted",
    ] {
        assert_eq!(form_of(name), Some(Form::CountRatio), "{name}");
    }
}

/// A generation evidence entry with these slot origins in one item.
fn generated(state: Value, origins: &[Value]) -> Value {
    let slots: serde_json::Map<String, Value> = origins
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let mut f = json!({"origin": o, "truncated": false, "source": "fresh"});
            if o.get("absent").is_none() {
                f["byte_length"] = json!(1);
                f["value_digest"] = json!(format!("sha256:{}", "a".repeat(64)));
            }
            (format!("f{i}"), f)
        })
        .collect();
    // The step's own summary, as spec 037 derives it from the slots.
    let mut n = BTreeMap::<&str, u64>::new();
    let mut reasons = BTreeMap::<String, u64>::new();
    for o in origins {
        let (kind, reason) = match o.as_str() {
            Some(k) => (k, None),
            None => {
                let (k, v) = o.as_object().unwrap().iter().next().unwrap();
                (k.as_str(), v["reason"].as_str())
            }
        };
        *n.entry(kind).or_default() += 1;
        if let Some(r) = reason {
            *reasons.entry(r.to_string()).or_default() += 1;
        }
    }
    let get = |k| n.get(k).copied().unwrap_or(0);
    let produced = get("generated") + get("repaired");
    let text_origin = if produced + get("backstop") == 0 {
        "none"
    } else if get("backstop") + get("absent") == 0 {
        "generated"
    } else if produced == 0 {
        "backstop"
    } else {
        "partial"
    };
    let items = if state == json!("ran") {
        json!([{"id": [], "context": {}, "generated": slots}])
    } else {
        json!([])
    };
    json!({"step": "item_text", "state": state, "items": items,
           "text_origin": text_origin,
           "counts": {"generated": get("generated"), "repaired": get("repaired"),
                      "backstop": get("backstop"), "absent": get("absent"),
                      "reasons": reasons}})
}

fn with_generation(steps: Vec<Value>) -> EvidenceRecord {
    let mut e = golden("screening.eval.pass.json");
    e["generation"] = Value::Array(steps);
    evidence(e)
}

#[test]
fn text_backstop_rates_count_slots_and_aggregate_origins() {
    let p = plan(golden("generation.plan.json"));
    let ran = json!("ran");
    let backstop = |r: &str| json!({"backstop": {"reason": r}});
    let cases = [
        // Two of three slots backstopped: partial.
        (
            p.clone(),
            Some(with_generation(vec![generated(
                ran.clone(),
                &[
                    json!("generated"),
                    backstop("deadline"),
                    backstop("invalid_output"),
                ],
            )])),
        ),
        // Every slot a backstop: full backstop.
        (
            p.clone(),
            Some(with_generation(vec![generated(
                ran.clone(),
                &[backstop("deadline")],
            )])),
        ),
        // Generated and absent: partial, with no backstop slot.
        (
            p.clone(),
            Some(with_generation(vec![generated(
                ran.clone(),
                &[
                    json!("repaired"),
                    json!({"absent": {"reason": "not_attempted"}}),
                ],
            )])),
        ),
        // The step was skipped.
        (
            p.clone(),
            Some(with_generation(vec![generated(
                json!({"skipped": {"reason": "outcome_not_matched"}}),
                &[],
            )])),
        ),
    ];
    let out = run(OriginFamily::TextBackstop, &cases);
    assert_eq!(out.len(), 11);
    let rate = find(&out, "origin/text_backstop/backstop_rate");
    assert_eq!(
        rate.cases,
        [count(2, 3), count(1, 1), count(0, 2), None, None]
    );
    assert_eq!(value(rate), 3.0 / 6.0);
    assert_eq!(breakdown(rate), [("abstained", 1), ("skipped", 1)]);
    assert_eq!(
        value(find(&out, "origin/text_backstop/backstop_rate/deadline")),
        2.0 / 6.0
    );
    assert_eq!(
        value(find(
            &out,
            "origin/text_backstop/backstop_rate/not_attempted"
        )),
        0.0
    );
    let partial = find(&out, "origin/text_backstop/partial_rate");
    assert_eq!(partial.cases[..3], [count(1, 1), count(0, 1), count(1, 1)]);
    assert_eq!(value(partial), 2.0 / 3.0);
    assert_eq!(
        value(find(&out, "origin/text_backstop/full_backstop_rate")),
        1.0 / 3.0
    );
    // A plan without a generate step: unknown, never zero.
    let other = run(
        OriginFamily::TextBackstop,
        &[(plan(golden("screening.plan.json")), None)],
    );
    assert!(other.iter().all(|m| matches!(
        &m.metric.value,
        MetricValue::Unknown { reason } if reason.contains("not_in_plan: 1")
    )));
}

#[test]
fn extraction_accuracy_and_absence_compare_retained_values_with_gold() {
    let p = plan(golden("extraction.plan.json"));
    let base = golden("extraction.evidence.json");
    let absent_party = {
        let mut e = base.clone();
        let f = e["extraction"][0]["fields"][1].as_object_mut().unwrap();
        f.insert("origin".into(), json!({"absent": {"reason": "missing"}}));
        for k in ["value", "value_digest", "derivation"] {
            f.remove(k);
        }
        evidence(e)
    };
    let fallback = {
        let mut e = base.clone();
        e["extraction"][0]["fields"][1]["origin"] = json!("fallback");
        e["extraction"][0]["fields"][1]["derivation"] = json!("exact-derived");
        evidence(e)
    };
    let cases = [
        (p.clone(), Some(evidence(base.clone()))),
        (p.clone(), Some(evidence(base.clone()))),
        (p.clone(), Some(absent_party)),
        (p.clone(), Some(fallback)),
        (p, Some(evidence(base))),
    ];
    let gold = [
        // Right size and features; area_name keeps only its digest.
        Some(
            json!({"fields": {"filters.party_size": 4, "filters.features": ["pool"],
                               "filters.area_name": "x"}}),
        ),
        // Wrong size; the gold says features are absent.
        Some(json!({"fields": {"filters.party_size": 5, "filters.features": "absent"}})),
        Some(json!({"fields": {"filters.party_size": 4}})),
        // A kind that does not pair with an integer.
        Some(json!({"fields": {"filters.party_size": true}})),
        None,
    ];
    let out = run_gold(OriginFamily::Extraction, &cases, &gold);
    let names: Vec<&str> = out.iter().map(|m| m.metric.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "origin/extraction/filters.party_size/accuracy",
            "origin/extraction/filters.party_size/absence_rate",
            "origin/extraction/filters.features/accuracy",
            "origin/extraction/filters.features/absence_rate",
            "origin/extraction/filters.area_name/accuracy",
            "origin/extraction/filters.area_name/absence_rate",
        ]
    );
    let size = find(&out, "origin/extraction/filters.party_size/accuracy");
    assert_eq!(
        size.cases,
        [count(1, 1), count(0, 1), None, None, None, None]
    );
    assert_eq!(
        breakdown(size),
        [
            ("absent", 1),
            ("abstained", 1),
            ("no_gold", 1),
            ("origin:extracted", 2),
            ("type_mismatch", 1)
        ]
    );
    let absence = find(&out, "origin/extraction/filters.party_size/absence_rate");
    assert_eq!(value(absence), 1.0 / 5.0);
    assert_eq!(
        breakdown(absence),
        [
            ("abstained", 1),
            ("origin:extracted", 3),
            ("origin:fallback", 1)
        ]
    );
    let features = find(&out, "origin/extraction/filters.features/accuracy");
    assert_eq!(features.cases[..2], [count(1, 1), count(0, 1)]);
    let area = find(&out, "origin/extraction/filters.area_name/accuracy");
    assert_eq!(area.cases[0], None);
    assert_eq!(
        breakdown(area),
        [("abstained", 1), ("no_gold", 4), ("value_not_retained", 1)]
    );
}

#[test]
fn every_family_is_available() {
    for f in [
        OriginFamily::TextBackstop,
        OriginFamily::Extraction,
        OriginFamily::Screening,
        OriginFamily::Corroboration,
        OriginFamily::AcquisitionTier,
    ] {
        assert!(f.available());
    }
}

#[test]
fn a_repeated_family_is_reported_once() {
    let p = plan(golden("screening.plan.json"));
    let e = evidence(golden("screening.eval.pass.json"));
    let ids = [case("c0")];
    let params = BTreeMap::<String, OutValue>::new();
    let scope = [Scope::Proposal {
        case: &ids[0],
        params: &params,
    }];
    let origin = [Some(OriginCase {
        plan: &p,
        evidence: Some(&e),
    })];
    let twice = [OriginFamily::Screening, OriginFamily::Screening];
    assert_eq!(origin_metrics(&twice, &[], &scope, &origin).len(), 2);
}

#[test]
#[should_panic(expected = "one origin case per scope entry")]
fn scope_and_cases_must_align() {
    origin_metrics(
        &[OriginFamily::Screening],
        &[],
        &[Scope::Out("abstained".into())],
        &[],
    );
}
